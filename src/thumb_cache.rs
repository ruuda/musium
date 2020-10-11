// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Defines an in-memory thumbnail cache.

use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;

use crate::{AlbumId, ArtistId};
use crate::album_table::AlbumTable;

/// References a single image in the larger concatenated array.
#[derive(Copy, Clone, Debug)]
struct ImageReference {
    begin: u32,
    end: u32,
}

/// A memory-backed dictionary of album id to cover art thumbnail.
///
/// We want to store all thumbnails in memory to be able to serve them quickly.
/// Thumbnails are pretty small (ranging roughly from 5 kB to 20 kB, with some
/// outliers in either direction), even for a few thousand albums, this only
/// costs a few dozen megabytes of memory. Perhaps if you want serve a really
/// large collection from a really small device (e.g. 50k albums from a
/// Raspberry Pi model 1), then this is a problem. If that ever happens, we
/// could opt to make the data array file mmap-backed instead, but for now we
/// will just keep everything in memory.
///
/// The reason we want the thumbnails in memory, is that browsers regularly
/// evict them from the cache, so it has to sometimes re-request the thumbs when
/// scrolling through the album list, and especially in search, where
/// search-as-you-type and the discography for artist results can cause many
/// different thumbnails to be loaded in a very short time.
///
/// I run Musium with the thumbnails on an external disk, and this disk goes
/// into power saving mode after a while. Accessing the disk at that point can
/// incur something like a 15-second latency, as the disk has to spin up again.
/// So when you haven't used Musium for while and you open the webinterface and
/// start a search, if a thumbnail is not cached, that is going to cause a
/// request, which is going to take 15 seconds to load. And because browsers
/// only make 6-ish connections to the same host in parallel, once those
/// connections are used up for thumbs, the entire UI stalls, because the
/// browser queues new requests, even if the search requests could immediately
/// be served from memory. This was a real bad user experience, so therefore I
/// want to make sure that we can serve thumbnails without hitting the disk.
pub struct ThumbCache {
    data: Box<[u8]>,
    references: AlbumTable<ImageReference>,
}

impl ThumbCache {
    /// Read the cover art thumbnails for the given albums into memory.
    ///
    /// The thumbnails are stored sequentially in an internal buffer in the
    /// order given. Therefore this function accepts a slice that includes the
    /// album id, even though it is not used: this way the artist-sorted album
    /// list from the index can be used as input, which means that covers by the
    /// same artist end up adjacent in memory. It probably does not make a big
    /// difference for performance, because thumbs are large relative to cache
    /// lines, but it doesn’t hurt either.
    pub fn new(albums: &[(ArtistId, AlbumId)], thumb_dir: &Path) -> io::Result<ThumbCache> {
        use std::u32;
        let mut fname = thumb_dir.to_path_buf();

        // Make an conservative initial guess of 5 kB per image. We probably
        // need more, but we can at least save the initial few relocations. We
        // could do better by first stat-ing all files and computing exactly how
        // much we need, but this happens only once during statup, so let's not
        // worry about performance too much here.
        let mut buffer = Vec::with_capacity(albums.len() * 5_000);

        let dummy = ImageReference { begin: 0, end: 0 };
        let mut references = AlbumTable::new(albums.len(), dummy);

        for (_, album_id) in albums {
            fname.push(format!("{}.jpg", album_id));
            match fs::File::open(&fname) {
                Ok(mut f) => {
                    let begin = buffer.len() as u32;
                    f.read_to_end(&mut buffer)?;
                    assert!(
                        buffer.len() < u32::MAX as usize,
                        "Can't have more than 4 GiB of thumbnails.",
                    );
                    let end = buffer.len() as u32;
                    let img_ref = ImageReference { begin, end };
                    references.insert(*album_id, img_ref);
                }
                // If there is no thumb for this album, simply skip it.
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            };
            fname.pop();
        }

        let result = ThumbCache {
            data: buffer.into_boxed_slice(),
            references: references
        };

        Ok(result)
    }

    pub fn get(&self, album_id: AlbumId) -> Option<&[u8]> {
        let img_ref = self.references.get(album_id)?;
        let img = &self.data[img_ref.begin as usize..img_ref.end as usize];
        Some(img)
    }
}
