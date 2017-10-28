// Metaindex -- A music metadata indexing library
// Copyright 2017 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// TODO: Remove once near-stable.
#![allow(dead_code)]

extern crate claxon;
extern crate crossbeam;

use std::ascii::AsciiExt;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

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

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TrackId(u64);

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AlbumId(u64);

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ArtistId(u64);

/// Index into a byte array that contains length-prefixed strings.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
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

struct BuildMetaIndex {
    artists: BTreeMap<ArtistId, Artist>,
    albums: BTreeMap<AlbumId, Album>,
    tracks: BTreeMap<TrackId, Track>,
    strings: BTreeMap<String, u32>,
    filenames: Vec<String>,
}

fn parse_date(_date_str: &str) -> Option<Date> {
    // TODO
    let date = Date {
        year: 0,
        month: 0,
        day: 0,
    };
    Some(date)
}

fn parse_uuid(_uuid: &str) -> Option<u64> {
    // TODO
    Some(1)
}

impl BuildMetaIndex {
    pub fn new() -> BuildMetaIndex {
        BuildMetaIndex {
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            tracks: BTreeMap::new(),
            strings: BTreeMap::new(),
            filenames: Vec::new(),
        }
    }

    /// Insert a string in the strings map, returning its id.
    ///
    /// The id is just an opaque integer. When the strings are written out
    /// sorted at a later time, the id can be converted into a `StringRef`.
    fn insert_string(&mut self, string: &str) -> u32 {
        // If the string exists already, return its id, otherwise insert it.
        // This does involve two lookups in the case of insert, but it does save
        // an allocation that turns the &str into a String when an insert is
        // required. We expect inserts to occur less than half of the time
        // (usually the sort artist is the same as the artist, and many tracks
        // share the same artist), therefore opt for the check first.
        let next_id = self.strings.len() as u32;
        if let Some(id) = self.strings.get(string) { return *id }
        self.strings.insert(string.to_string(), next_id);
        next_id
    }

    pub fn insert(&mut self, filename: &str, tags: &mut claxon::metadata::Tags) {
        let mut disc_number = None;
        let mut track_number = None;
        let mut title = None;
        let mut album = None;
        let mut artist = None;
        let mut artist_for_sort = None;
        let mut album_artist = None;
        let mut album_artist_for_sort = None;
        let mut original_date = None;
        let mut date = None;

        let mut mbid_track = 0;
        let mut mbid_album = 0;
        let mut mbid_artist = 0;

        let filename_id = self.filenames.len() as u32;
        self.filenames.push(filename.to_string());

        for (tag, value) in tags {
            match &tag.to_ascii_lowercase()[..] {
                // TODO: Replace unwraps here with proper parse error reporting.
                "album"                     => album = Some(self.insert_string(value)),
                "albumartist"               => album_artist = Some(self.insert_string(value)),
                "albumartistsort"           => album_artist_for_sort = Some(self.insert_string(value)),
                "artist"                    => artist = Some(self.insert_string(value)),
                "artistsort"                => artist_for_sort = Some(self.insert_string(value)),
                "discnumber"                => disc_number = Some(u16::from_str(value).unwrap()),
                "musicbrainz_albumartistid" => mbid_artist = parse_uuid(value).unwrap(),
                "musicbrainz_albumid"       => mbid_album = parse_uuid(value).unwrap(),
                "musicbrainz_trackid"       => mbid_track = parse_uuid(value).unwrap(),
                "originaldate"              => original_date = parse_date(value),
                "date"                      => date = parse_date(value),
                "title"                     => title = Some(self.insert_string(value)),
                "tracknumber"               => track_number = Some(u16::from_str(value).unwrap()),
                _ => {}
            }
        }

        if disc_number == None { panic!("discnumber not set") }
        if track_number == None { panic!("tracknumber not set") }
        if title == None { panic!("title not set") }
        if album == None { panic!("album not set") }
        if artist == None { panic!("artist not set") }
        if album_artist == None { panic!("album artist not set") }

        if mbid_track == 0 { panic!("musicbrainz_trackid not set") }
        if mbid_album == 0 { panic!("musicbrainz_albumid not set") }
        if mbid_artist == 0 { panic!("musicbrainz_albumartistid not set") }

        let track_id = TrackId(mbid_track);
        let album_id = AlbumId(mbid_album);
        let artist_id = ArtistId(mbid_artist);

        let track = Track {
            album_id: album_id,
            disc_number: disc_number.expect("discnumber not set"),
            track_number: track_number.expect("tracknumber not set"),
            title: StringRef(title.expect("title not set")),
            artist: StringRef(artist.expect("artist not set")),
            artist_for_sort: StringRef(artist_for_sort.or(artist).unwrap()),
            duration_seconds: 1, // TODO: Get from streaminfo.
            filename: StringRef(filename_id),
        };
        let album = Album {
            artist_id: artist_id,
            title: StringRef(album.expect("album not set")),
            original_release_date: original_date.or(date).expect("neither originaldate nor date set"),
        };
        let artist = Artist {
            name: StringRef(album_artist.expect("albumartist not set")),
            name_for_sort: StringRef(album_artist_for_sort.or(album_artist).unwrap()),
        };

        // TODO: Check for consistency if duplicates occur.
        self.tracks.insert(track_id, track);
        self.albums.insert(album_id, album);
        self.artists.insert(artist_id, artist);
    }
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

    pub fn process<I>(paths: &Mutex<I>)
    where I: Iterator, <I as Iterator>::Item: AsRef<Path> {
        let mut builder = BuildMetaIndex::new();
        loop {
            let opt_path = paths.lock().unwrap().next();
            let path = match opt_path {
                Some(p) => p,
                None => return,
            };
            let opts = claxon::FlacReaderOptions {
                metadata_only: true,
                read_vorbis_comment: true,
            };
            let reader = claxon::FlacReader::open_ext(path.as_ref(), opts).unwrap();
            builder.insert(path.as_ref().to_str().expect("TODO"), &mut reader.tags());
            println!("{}", path.as_ref().to_str().unwrap());
        }
    }

    /// Index the given files, and store the index in the target directory.
    ///
    /// Although this streams most metadata to disk, a few parts of the index
    /// have to be kept in memory for efficient sorting, so the paths iterator
    /// should not yield *too* many elements.
    pub fn from_paths<I>(paths: I) -> Result<MemoryMetaIndex>
    where I: Iterator,
          <I as IntoIterator>::Item: AsRef<Path>,
          <I as IntoIterator>::IntoIter: Send {
        let paths_iterator = paths.into_iter().fuse();
        let mutex = Mutex::new(paths_iterator);

        let num_threads = 24;
        crossbeam::scope(|scope| {
            for _ in 0..num_threads {
                scope.spawn(|| MemoryMetaIndex::process(&mutex));
            }
        });
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
