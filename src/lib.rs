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
            println!("{}", reader.get_tag("title").next().unwrap());
            println!("{}", reader.get_tag("tracknumber").next().unwrap());
            println!("{}", reader.get_tag("artist").next().unwrap());
            println!("{}", reader.get_tag("musicbrainz_trackid").next().unwrap());
            println!("{}", reader.get_tag("musicbrainz_albumid").next().unwrap());
            println!("{}", reader.get_tag("musicbrainz_albumartistid").next().unwrap());
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
