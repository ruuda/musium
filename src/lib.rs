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
use std::fmt;
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::u32;

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

#[repr(C, packed)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Track {
    album_id: AlbumId,
    title: StringRef,
    artist: StringRef,
    filename: StringRef,
    // Using u16 for duration gives us a little over 18 hours as maximum
    // duration; using u8 for track number gives us at most 255 tracks. This is
    // perhaps a bit limiting, but it does allow us to squeeze a `(TrackId,
    // Track)` into half a cache line, so they never straddle cache line
    // boundaries. And of course more of them fit in the cache. If range ever
    // becomes a problem, we could use some of the disc number bits to extend
    // the duration range or track number range.
    duration_seconds: u16,
    disc_number: u8,
    track_number: u8,
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
    assert_eq!(mem::size_of::<Track>(), 24);
    assert_eq!(mem::size_of::<Album>(), 16);
    assert_eq!(mem::size_of::<Artist>(), 8);
    assert_eq!(mem::size_of::<(TrackId, Track)>(), 32);
}

#[derive(Copy, Clone, Debug)]
pub enum IssueDetail {
    FieldMissingError(&'static str),
    FieldParseFailedError(&'static str),
}

#[derive(Debug)]
pub struct Issue {
    pub filename: String,
    pub detail: IssueDetail,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: ", self.filename)?;
        match self.detail {
            IssueDetail::FieldMissingError(field) =>
                write!(f, "error: field '{}' missing.", field),
            IssueDetail::FieldParseFailedError(field) =>
                write!(f, "error: failed to parse field '{}'.", field),
        }
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Display for AlbumId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Display for ArtistId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

struct BuildMetaIndex {
    artists: BTreeMap<ArtistId, Artist>,
    albums: BTreeMap<AlbumId, Album>,
    tracks: BTreeMap<TrackId, Track>,
    strings: BTreeMap<String, u32>,
    filenames: Vec<String>,
    issues: SyncSender<Issue>,
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

fn parse_uuid(uuid: &str) -> Option<u64> {
    // Validate that the textual format of the UUID is as expected.
    // E.g. `1070cbb2-ad74-44ce-90a4-7fa1dfd8164e`.
    if uuid.len() != 36 { return None }
    if uuid.as_bytes()[8] != b'-' { return None }
    if uuid.as_bytes()[13] != b'-' { return None }
    if uuid.as_bytes()[18] != b'-' { return None }
    if uuid.as_bytes()[23] != b'-' { return None }
    // We parse the first and last 4 bytes and use these as the 8-byte id.
    // See the comments above for the motivation for using only 64 of the 128
    // bits. We take the front and back of the string because it is eary, there
    // are no dashes to strip. Also, the non-random version bits are in the
    // middle, so this way we avoid using those.
    let high = u32::from_str_radix(&uuid[..8], 16).ok()? as u64;
    let low = u32::from_str_radix(&uuid[28..], 16).ok()? as u64;
    Some((high << 32) | low)
}

fn get_track_id(album_artist_id: ArtistId,
                album_id: AlbumId,
                disc_number: u8,
                track_number: u8)
                -> TrackId {
    // Take the most significant bits from the album artist, such that when we
    // sort by track id, all tracks by the same artist are adjacent. This is
    // good for locality of reference.
    let high = album_artist_id.0 & 0xffff_ffff_0000_0000;

    // Then take the bits from the album id, so all the tracks within one album
    // are adjacent. This is desirable, because two tracks fit in a cache line,
    // halving the memory access cost of looking up an entire album. It also
    // makes memory access more predictable. Finally, if the 48 most significant
    // bits uniquely identify the album (which we assume, TODO work out stats),
    // then all tracks are guaranteed to be adjacent, and we can use an
    // efficient range query to find them.
    let mid = album_id.0 & 0x0000_0000_ffff_00000;

    // Finally, within an album the disc number and track number should uniquely
    // identify the track.
    let low = ((disc_number as u64) << 8) | (track_number as u64);

    // TODO: Actually I can apply the same trick for the album id, take the
    // artist id as prefix. And then I can collapse all data structures to
    // remove the redundancies.
    TrackId(high | mid | low)
}

impl BuildMetaIndex {
    pub fn new(issues: SyncSender<Issue>) -> BuildMetaIndex {
        BuildMetaIndex {
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            tracks: BTreeMap::new(),
            strings: BTreeMap::new(),
            filenames: Vec::new(),
            issues: issues,
        }
    }

    fn error_missing_field(&mut self, filename: String, field: &'static str) {
        let issue = Issue {
            filename: filename,
            detail: IssueDetail::FieldMissingError(field),
        };
        self.issues.send(issue).unwrap();
    }

    fn error_parse_failed(&mut self, filename: String, field: &'static str) {
        let issue = Issue {
            filename: filename,
            detail: IssueDetail::FieldParseFailedError(field),
        };
        self.issues.send(issue).unwrap();
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
        // share the same artist), therefore opt for the check first. Empirical
        // evidence: on my personal library, about 22% of the strings need to be
        // inserted (12.6k out of 57.8k total strings).
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
        let mut album_artist = None;
        let mut album_artist_for_sort = None;
        let mut date = None;

        let mut mbid_track = 0;
        let mut mbid_album = 0;
        let mut mbid_artist = 0;

        let filename_id = self.filenames.len() as u32;
        let filename_string = filename.to_string();

        for (tag, value) in tags {
            match &tag.to_ascii_lowercase()[..] {
                // TODO: Replace unwraps here with proper parse error reporting.
                "album"                     => album = Some(self.insert_string(value)),
                "albumartist"               => album_artist = Some(self.insert_string(value)),
                "albumartistsort"           => album_artist_for_sort = Some(self.insert_string(value)),
                "artist"                    => artist = Some(self.insert_string(value)),
                "discnumber"                => disc_number = Some(u8::from_str(value).unwrap()),
                "musicbrainz_albumartistid" => mbid_artist = match parse_uuid(value) {
                    Some(id) => id,
                    None => return self.error_parse_failed(filename_string, "musicbrainz_albumartistid"),
                },
                "musicbrainz_albumid"       => mbid_album = match parse_uuid(value) {
                    Some(id) => id,
                    None => return self.error_parse_failed(filename_string, "musicbrainz_albumid"),
                },
                "musicbrainz_trackid"       => mbid_track = match parse_uuid(value) {
                    Some(id) => id,
                    None => return self.error_parse_failed(filename_string, "musicbrainz_trackid"),
                },
                "originaldate"              => date = parse_date(value),
                "title"                     => title = Some(self.insert_string(value)),
                "tracknumber"               => track_number = Some(u8::from_str(value).unwrap()),
                _ => {}
            }
        }

        if mbid_track == 0 {
            return self.error_missing_field(filename_string, "musicbrainz_trackid")
        }
        if mbid_album == 0 {
            return self.error_missing_field(filename_string, "musicbrainz_albumid")
        }
        if mbid_artist == 0 {
            return self.error_missing_field(filename_string, "musicbrainz_albumartistid")
        }

        // TODO: Make a macro for this, this is terrible.
        let f_disc_number = disc_number.unwrap_or(1);
        let f_track_number = match track_number {
            Some(t) => t,
            None => return self.error_missing_field(filename_string, "tracknumber"),
        };
        let f_title = match title {
            Some(t) => t,
            None => return self.error_missing_field(filename_string, "title"),
        };
        let f_artist = match artist {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "artist"),
        };
        let f_album = match album {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "album"),
        };
        let f_album_artist = match album_artist {
            Some(a) => a,
            None => return self.error_missing_field(filename_string, "albumartist"),
        };
        let f_date = match date {
            Some(d) => d,
            None => return self.error_missing_field(filename_string, "originaldate"),
        };

        let album_id = AlbumId(mbid_album);
        let artist_id = ArtistId(mbid_artist);
        let track_id = get_track_id(artist_id, album_id, f_disc_number, f_track_number);

        let track = Track {
            album_id: album_id,
            disc_number: f_disc_number,
            track_number: f_track_number,
            title: StringRef(f_title),
            artist: StringRef(f_artist),
            duration_seconds: 1, // TODO: Get from streaminfo.
            filename: StringRef(filename_id),
        };
        let album = Album {
            artist_id: artist_id,
            title: StringRef(f_album),
            original_release_date: f_date,
        };
        let artist = Artist {
            name: StringRef(f_album_artist),
            name_for_sort: StringRef(album_artist_for_sort.unwrap_or(f_album_artist)),
        };

        // TODO: Check for consistency if duplicates occur.
        self.filenames.push(filename_string);
        if self.tracks.get(&track_id).is_some() {
            panic!("Duplicate track {}, file {}.", track_id, filename);
        }
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

    pub fn process<I>(paths: &Mutex<I>, issues: SyncSender<Issue>)
    where I: Iterator, <I as Iterator>::Item: AsRef<Path> {
        let mut builder = BuildMetaIndex::new(issues);
        loop {
            let opt_path = paths.lock().unwrap().next();
            let path = match opt_path {
                Some(p) => p,
                None => break,
            };
            let opts = claxon::FlacReaderOptions {
                metadata_only: true,
                read_vorbis_comment: true,
            };
            let reader = claxon::FlacReader::open_ext(path.as_ref(), opts).unwrap();
            builder.insert(path.as_ref().to_str().expect("TODO"), &mut reader.tags());
        }

        let mut m = 0;
        for s in builder.strings {
            m = m.max(s.0.len());
        }
        for (trid, track) in &builder.tracks {
            println!("{}: {}.{} - <title>", trid, track.disc_number, track.track_number);
        }
        println!("max string len: {}", m);
        println!("indexed {} tracks on {} albums by {} artists",
          builder.tracks.len(),
          builder.albums.len(),
          builder.artists.len(),
        );
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
        let (tx_issue, rx_issue) = sync_channel(16);

        let num_threads = 1; //24;
        crossbeam::scope(|scope| {
            for _ in 0..num_threads {
                let issues = tx_issue.clone();
                scope.spawn(|| MemoryMetaIndex::process(&mutex, issues));
            }

            // Print issues live as indexing happens.
            mem::drop(tx_issue);
            scope.spawn(|| {
                for issue in rx_issue {
                    println!("{}", issue);
                }
            });
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
