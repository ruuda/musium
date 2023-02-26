// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Defines an in-memory thumbnail cache.

use std::fmt;
use std::path::Path;

use crate::AlbumId;
use crate::album_table::AlbumTable;
use crate::database as db;
use crate::database::Transaction;
use crate::database_utils;

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

pub struct ThumbCacheSize {
    image_data_bytes: usize,
    table_bytes: usize,
    max_probe_len: usize,
}

impl fmt::Display for ThumbCacheSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f,
            "{:4} kB ({:3} kB images, {:3} kB table), max probe length: {}",
            (self.image_data_bytes + self.table_bytes) / 1000,
            self.image_data_bytes / 1000,
            self.table_bytes / 1000,
            self.max_probe_len,
        )
    }
}

impl ThumbCache {
    /// Return an empty thumb cache, for use as placeholder when loading.
    pub fn new_empty() -> ThumbCache {
        Self {
            data: Box::new([]),
            references: AlbumTable::new(0, ImageReference { begin: 0, end: 0 }),
        }
    }

    /// Read cover art thumbnails from the database into memory.
    ///
    /// See also [`load_from_database`].
    pub fn load_from_database_at(db_path: &Path) -> db::Result<ThumbCache> {
        let inner = database_utils::connect_readonly(&db_path)?;
        let mut conn = db::Connection::new(&inner);
        let mut tx = conn.begin()?;
        let result = ThumbCache::load_from_database(&mut tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Read the cover art thumbnails from the database into memory.
    ///
    /// The thumbnails are stored sequentially in an internal buffer in the
    /// order as returned by the database.
    pub fn load_from_database(tx: &mut Transaction) -> db::Result<ThumbCache> {
        let (count, total_size) = db::select_thumbnails_count_and_total_size(tx)?;
        let mut buffer = Vec::with_capacity(total_size as usize);

        let dummy = ImageReference { begin: 0, end: 0 };
        let mut references = AlbumTable::new(count as usize, dummy);

        for thumb_result in db::iter_thumbnails(tx)? {
            let thumb = thumb_result?;
            let begin = buffer.len() as u32;
            buffer.extend_from_slice(&thumb.data);
            assert!(
                buffer.len() < u32::MAX as usize,
                "Can't have more than 4 GiB of thumbnails.",
            );
            let end = buffer.len() as u32;
            let img_ref = ImageReference { begin, end };
            let album_id = AlbumId(thumb.album_id as u64);
            references.insert(album_id, img_ref);
        }

        assert_eq!(
            buffer.len(),
            total_size as usize,
            "We should have gotten as much data out of the database as expected.",
        );

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

    pub fn size(&self) -> ThumbCacheSize {
        use std::mem;
        assert_eq!(mem::size_of::<(AlbumId, ImageReference)>(), 16);
        ThumbCacheSize {
            image_data_bytes: self.data.len(),
            table_bytes: self.references.capacity() * 16,
            max_probe_len: self.references.max_probe_len(),
        }
    }
}
