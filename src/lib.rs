// Metaindex -- A music metadata indexing library
// Copyright 2017 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// TODO: Remove once near-stable.
#![allow(dead_code)]

extern crate claxon;

use std::io;
use std::path::{Path, PathBuf};

// Stats of my personal music library at this point:
//
//     11.5k tracks
//      1.2k albums
//      0.3k album artists
//      1.4k track artists
//
// The observation is that there is an order of magnitude difference between
// the track count and album count, and also between album count and artist
// count. In other words, track data will dominate, and album artist data is
// hardly relevant.
//
// What should I design for? My library will probably grow to twice its size
// over time. Perhaps even to 10x the size. But I am pretty confident that it
// will not grow by 100x. So by designing the system to support 1M tracks, I
// should be safe.
//
// Let's consider IDs for a moment. The 16-byte MusicBrainz UUIDs take up a lot
// of space, and I want to run on low-end systems, in particular the
// first-generation Raspberry Pi, which has 16k L1 cache and 128k L2 cache.
// Saving 50% on IDs can have a big impact there. So under the above assumptions
// of 1M tracks, can I get away with using 8 bytes of the 16-byte UUIDs? Let's
// consider the collision probability. With 8-byte identifiers, to have a 1%
// collision probability, one would need about 608M tracks. That is a lot more
// than what I am designing for. For MusicBrainz, which aims to catalog every
// track ever produced by humanity, this might be too risky. But for my personal
// collection the memory savings are well worth the risk.

struct TrackId(u64);
struct AlbumId(u64);
struct ArtistId(u64);

/// Index into a byte array that contains length-prefixed strings.
struct StringRef(u32);

struct Track {
    album_id: AlbumId,
    disc_number: u16,
    track_number: u16,
    title: StringRef,
    artist: StringRef,
    artist_for_sort: StringRef,
    duration_seconds: u32,
    filename: StringRef,
}

struct Date {
    year: u16,
    month: u8,
    day: u8,
}

struct Album {
    artist_id: ArtistId,
    title: StringRef,
    original_release_date: Date,
}

struct Artist {
    name: StringRef,
    name_for_sort: StringRef,
}

#[test]
fn struct_sizes_are_as_expected() {
    use std::mem;
    assert_eq!(mem::size_of::<Track>(), 32);
    assert_eq!(mem::size_of::<Album>(), 16);
    assert_eq!(mem::size_of::<Artist>(), 8);
}

pub trait MetaIndex {
    /// Returns the number of tracks in the index.
    fn len(&self) -> usize;
}

pub enum Error {
    /// An IO error during writing the index or reading the index or metadata.
    IoError(io::Error),

    /// An FLAC file to be indexed could not be read.
    FormatError(claxon::Error),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

impl From<claxon::Error> for Error {
    fn from(err: claxon::Error) -> Error {
        match err {
            claxon::Error::IoError(io_err) => Error::IoError(io_err),
            _ => Error::FormatError(err),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

pub struct MemoryMetaIndex {

}

impl MemoryMetaIndex {
    /// Create an empty memory-backed metaindex.
    ///
    /// The empty index acts as a unit for combining with other indices.
    pub fn new() -> MemoryMetaIndex {
        MemoryMetaIndex { }
    }

    /// Index the given files, and store the index in the target directory.
    ///
    /// Although this streams most metadata to disk, a few parts of the index
    /// have to be kept in memory for efficient sorting, so the paths iterator
    /// should not yield *too* many elements.
    pub fn from_paths<I>(paths: I) -> Result<MemoryMetaIndex>
    where I: IntoIterator,
          <I as IntoIterator>::Item: AsRef<Path> {
        for path in paths {
            let reader = claxon::FlacReader::open(path.as_ref())?;
            println!("{}", reader.get_tag("title").next().unwrap_or(""));
            println!("{}", reader.get_tag("tracknumber").next().unwrap_or(""));
            println!("{}", reader.get_tag("artist").next().unwrap_or(""));
            println!("{}", reader.get_tag("musicbrainz_trackid").next().unwrap_or(""));
            println!("{}", reader.get_tag("musicbrainz_albumid").next().unwrap_or(""));
            println!("{}", reader.get_tag("musicbrainz_albumartistid").next().unwrap_or(""));
        }

        Ok(MemoryMetaIndex::new())
    }
}

impl MetaIndex for MemoryMetaIndex {
    fn len(&self) -> usize {
        // TODO: Real impl
        0
    }
}

mod tests {
    use super::{MetaIndex, MemoryMetaIndex};

    #[test]
    fn new_is_empty() {
        let mi = MemoryMetaIndex::new();
        assert_eq!(mi.len(), 0);
    }
}
