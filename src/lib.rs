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
extern crate unicode_normalization;

mod flat_trie; // TODO: Rename.

use std::ascii::AsciiExt;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::u32;

use unicode_normalization::UnicodeNormalization;

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
//
// Let's dig a bit further: I really only need to uniquely identify album
// artists, then albums by that artist, and then tracks on those albums. And I
// would like to do so based on their metadata only, not involving global
// counters, because I want something that is deterministic but which can be
// parallelized. So how many bits do we need for the album artist? Let's say I
// the upper bound is 50k artists, and I want a collision probability of at most
// 0.1% at that number of artists. The lowest multiple of 8 that I can get away
// with is 48 bits.

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
    strings_to_id: BTreeMap<String, u32>,
    strings: Vec<String>,
    filenames: Vec<String>,
    words_track_title: BTreeSet<(String, TrackId)>,
    words_album_title: BTreeSet<(String, AlbumId)>,
    words_album_artist: BTreeSet<(String, ArtistId)>,
    // When the track artist differs from the album artist, the words that occur
    // in the track artist but not in the album artist, are included here.
    words_track_artist: BTreeSet<(String, TrackId)>,
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

fn get_track_id(album_id: AlbumId,
                disc_number: u8,
                track_number: u8)
                -> TrackId {
    // Then take the bits from the album id, so all the tracks within one album
    // are adjacent. This is desirable, because two tracks fit in a cache line,
    // halving the memory access cost of looking up an entire album. It also
    // makes memory access more predictable. Finally, if the 52 most significant
    // bits uniquely identify the album (which we assume), then all tracks are
    // guaranteed to be adjacent, and we can use an efficient range query to
    // find them.
    let high = album_id.0 & 0xffff_ffff_ffff_f000;

    // Finally, within an album the disc number and track number should uniquely
    // identify the track.
    let mid = ((disc_number & 0xf) as u64) << 8;
    let low = track_number as u64;

    TrackId(high | mid | low)
}

fn push_word(dest: &mut Vec<String>, word: &mut String) {
    if word.len() == 0 {
        return
    }

    let mut w = String::new();
    mem::swap(&mut w, word);
    dest.push(w);
}

/// Fills the vector with the words in the string in normalized form.
///
/// This first normalizes words to Unicode Normalization Form KD, which
/// decomposes characters with accents into the character and the accent
/// separately. The "KD" form, as opposed to the "D" form, also replaces more
/// things that have the same semantic meaning, such as replacing superscripts
/// with normal digits. Finally (not part of the KD normalization), everything
/// is lowercased, and accents, some punctuation, and single-character words are
/// removed.
fn normalize_words(title: &str, dest: &mut Vec<String>) {
    // We assume that in the majority of the cases, the transformations
    // below do not change the number of bytes.
    let mut word = String::new();
    let mut num_dots = 0;

    // Drop some punctuation characters and accents. We remove punctuation that
    // is unlikely to contain a lot of information about the title. (Deadmau5
    // can go and use some normal titles next time.) We remove accents to make
    // searching easier without having to type the exact accent.
    let drop = "“”‘’'\"()[]«»,❦\u{300}\u{301}\u{302}\u{303}\u{307}\u{308}\u{327}";
    let keep = "$€#&=*%∆";

    // Cut words at the following punctuation characters, but still include them
    // as a word of their own. This ensures that words are broken up properly,
    // but it still allows searching for this punctuation. This is important,
    // because some artists are under the illusion that it is cool to use
    // punctuation as part of a name.
    let cut = "/\\@_+-:!?<>";

    // Loop over the characters, normalized and lowercased.
    for ch in title.nfkd().flat_map(|nch| nch.to_lowercase()) {
        match ch {
            // Split words at whitespace or at the cut characters.
            _ if ch.is_whitespace() => {
                push_word(dest, &mut word);
            }
            _ if cut.contains(ch) => {
                push_word(dest, &mut word);
                dest.push(ch.to_string());
            }
            // The period is special: generally we don't want to include it as a
            // word, and simply ignore it altogether. (E.g. "S.P.Y" turns into
            // "spy".) But the ellipisis (...) we do want to keep. There are
            // even tracks titled "...". So we need to detect the ellipsis.
            '.' => {
                num_dots += 1;
                if num_dots == 3 {
                    dest.push("...".to_string());
                    word = String::new();
                }
                continue
            }
            // Normalize a few characters to more common ones.
            // Sometimes used in "n°", map to "no".
            '°' => word.push('o'),
            '♯' => word.push('#'),
            'ø' => word.push('o'),
            'æ' => word.push_str("ae"),
            'œ' => word.push_str("oe"),
            // A hyphen, use the ascii one instead.
            '\u{2010}' => word.push('-'),
            // I do want to be able to find my Justice albums with a normal
            // keyboard.
            '✝' => {
                push_word(dest, &mut word);
                dest.push("cross".to_string());
            }
            '∞' => {
                push_word(dest, &mut word);
                dest.push("infinity".to_string());
            }
            // Drop characters that we don't care for, keep characters that we
            // definitely care for.
            _ if drop.contains(ch) => {}
            _ if keep.contains(ch) || ch.is_alphanumeric() => word.push(ch),
            _ => panic!("Unknown character {} ({}) in title: {}", ch, ch.escape_unicode(), title),
        }

        // Reset the ellipsis counter after every non-period character.
        num_dots = 0;
    }

    // Push the final word.
    push_word(dest, &mut word);
}

impl BuildMetaIndex {
    pub fn new(issues: SyncSender<Issue>) -> BuildMetaIndex {
        BuildMetaIndex {
            artists: BTreeMap::new(),
            albums: BTreeMap::new(),
            tracks: BTreeMap::new(),
            strings: Vec::new(),
            strings_to_id: BTreeMap::new(),
            filenames: Vec::new(),
            words_track_title: BTreeSet::new(),
            words_album_title: BTreeSet::new(),
            words_album_artist: BTreeSet::new(),
            words_track_artist: BTreeSet::new(),
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
        // TODO: Unicode-normalize the string.
        if let Some(id) = self.strings_to_id.get(string) { return *id }
        self.strings_to_id.insert(string.to_string(), next_id);
        self.strings.push(string.to_string());
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
        let f_track_artist = match artist {
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

        let artist_id = ArtistId(mbid_artist);
        let album_id = AlbumId(mbid_album);
        let track_id = get_track_id(album_id, f_disc_number, f_track_number);

        // Split the title, album, and album artist, on words, and add those to
        // the indexes, to allow finding the track/album/artist later by word.
        let mut words = Vec::new();
        normalize_words(&self.strings[f_title as usize], &mut words);
        for w in words.drain(..) { self.words_track_title.insert((w, track_id)); }
        normalize_words(&self.strings[f_album as usize], &mut words);
        for w in words.drain(..) { self.words_album_title.insert((w, album_id)); }
        normalize_words(&self.strings[f_album_artist as usize], &mut words);
        for w in words.drain(..) { self.words_album_artist.insert((w, artist_id)); }

        // If the track artist differs from the album artist, add words for the
        // track artist, but only for the words that do not occur in the album
        // artist. This allows looking up e.g. a "feat. artist", without
        // polluting the index with every track by that artist.
        if f_track_artist != f_album_artist {
            normalize_words(&self.strings[f_track_artist as usize], &mut words);
            for w in words.drain(..) {
                let pair = (w, artist_id);
                if !self.words_album_artist.contains(&pair) {
                    self.words_track_artist.insert((pair.0, track_id));
                }
            }
        }

        let track = Track {
            album_id: album_id,
            disc_number: f_disc_number,
            track_number: f_track_number,
            title: StringRef(f_title),
            artist: StringRef(f_track_artist),
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
        for s in &builder.strings {
            m = m.max(s.len());
        }
        println!("max string len: {}", m);
        println!("word counts: {} track, {} album, {} artist, {} feat. artist",
            builder.words_track_title.iter().map(|p| p.0.clone()).collect::<BTreeSet<String>>().len(),
            builder.words_album_title.len(),
            builder.words_album_artist.len(),
            builder.words_track_artist.len(),
        );
        let bytes: BTreeSet<u8> = builder.words_track_title
            .iter()
            .map(|&(ref w, _)| w.as_bytes()[0])
            .collect();
        println!("{} unique first bytes ({:?})", bytes.len(), bytes);
        let mut count_by_first = BTreeMap::new();
        for &(ref w, _) in &builder.words_track_title {
            let bs = w.as_bytes();
            let map = count_by_first.entry(bs[0]).or_insert(BTreeSet::new());
            if bs.len() > 1 {
                map.insert(bs[1]);
            }
        }
        let mut lens = Vec::new();
        for (w, ref m) in count_by_first {
            println!("{:02x}: {}", w, m.len());
            lens.push(m.len());
        }
        lens.sort();
        println!("Min, median, max len: {} {} {}", lens[0], lens[lens.len() / 2], lens[lens.len() - 1]);
        lens.clear();
        let mut ws = builder.words_track_title
            .iter()
            .map(|&(ref w, _)| w.clone())
            .collect::<BTreeSet<String>>();
        ws.extend(builder.words_album_title
            .iter()
            .map(|&(ref w, _)| w.clone()));
        ws.extend(builder.words_album_artist
            .iter()
            .map(|&(ref w, _)| w.clone()));
        ws.extend(builder.words_track_artist
            .iter()
            .map(|&(ref w, _)| w.clone()));

        let mut tbuilder = flat_trie::FlatTreeBuilder::new();
        for (i, w) in ws.iter().enumerate() {
            tbuilder.insert(w.as_bytes(), i as u32);
            println!("{}", w);
        }

        println!("Number of distinct words: {}", ws.len());
        let mut lens: Vec<usize> = ws.iter().map(|w| w.len()).collect();
        lens.sort();
        let p = lens.iter().position(|x| *x == 12).unwrap();
        println!("# keys shorter than 11 bits: {} / {}", lens.len() - p, lens.len());
        println!("Word len q0, q50, q75, q90, q100: {} {} {} {} {}",
                 lens[0],
                 lens[lens.len() * 50 / 100],
                 lens[lens.len() * 75 / 100],
                 lens[lens.len() * 90 / 100],
                 lens[lens.len() - 1]);
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
