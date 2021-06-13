// Musium -- Music playback daemon with web-based library browser
// Copyright 2017 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

extern crate alsa;
extern crate claxon;
extern crate crossbeam;
extern crate libc;
extern crate nix;
extern crate serde_json;
extern crate unicode_normalization;

mod album_table;
mod build;
mod database;
mod search;
mod word_index;

pub mod config;
pub mod error;
pub mod history;
pub mod playback;
pub mod player;
pub mod prim;
pub mod scan;
pub mod serialization;
pub mod server;
pub mod status;
pub mod string_utils;
pub mod systemd;
pub mod thumb_cache;

use std::collections::btree_map;
use std::collections::BTreeSet;
use std::io;
use std::mem;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::u32;
use std::u64;

use crate::prim::{ArtistId, Artist, AlbumId, Album, TrackId, Track, Lufs, StringRef, FilenameRef, get_track_id};
use crate::word_index::{MemoryWordIndex};
use crate::string_utils::StringDeduper;
use crate::build::{BuildMetaIndex, Issue, Progress, artists_different, albums_different};
use crate::status::StatusSink;

pub trait MetaIndex {
    /// Return the number of tracks in the index.
    fn len(&self) -> usize;

    /// Resolve a `StringRef` to a string slice.
    ///
    /// Returns an empty string if the ref is out of bounds. May return a
    /// garbage string when the ref is at the wrong offset.
    fn get_string(&self, sr: StringRef) -> &str;

    /// Resolve a `StringRef` to a filename string slice.
    ///
    /// Same as `get_string()`, but for filenames, which have a different arena.
    fn get_filename(&self, sr: FilenameRef) -> &str;

    /// Return track metadata.
    fn get_track(&self, id: TrackId) -> Option<&Track>;

    /// Return album metadata.
    fn get_album(&self, id: AlbumId) -> Option<&Album>;

    /// Return all tracks that are part of the album.
    fn get_album_tracks(&self, id: AlbumId) -> &[(TrackId, Track)];

    /// Return all tracks, ordered by id.
    fn get_tracks(&self) -> &[(TrackId, Track)];

    /// Return all albums, ordered by id.
    fn get_albums(&self) -> &[(AlbumId, Album)];

    /// Return all album artists, ordered by id.
    fn get_artists(&self) -> &[(ArtistId, Artist)];

    /// Look up an artist by id.
    fn get_artist(&self, _: ArtistId) -> Option<&Artist>;

    /// Return all albums by the given artist.
    ///
    /// The albums are sorted by ascending release date.
    ///
    /// Includes the artist too, because the associations are stored as a flat
    /// array of (artist id, album id) pairs.
    fn get_albums_by_artist(&self, _: ArtistId) -> &[(ArtistId, AlbumId)];

    /// Return all (artist id, album id) pairs.
    ///
    /// The resulting index is sorted by artist id first, and then by ascending
    /// release date of the album.
    fn get_album_ids_ordered_by_artist(&self) -> &[(ArtistId, AlbumId)];

    /// Search for artists where the word occurs in the name.
    fn search_artist(&self, words: &[String], into: &mut Vec<ArtistId>);

    /// Search for albums where the word occurs in the title or artist.
    fn search_album(&self, words: &[String], into: &mut Vec<AlbumId>);

    /// Search for tracks where the word occurs in the title or track artist.
    ///
    /// A word in the track artist will only match on words that are not also
    /// part of the album artist. That is, this search will not turn up all
    /// tracks by an artist, only those for which `search_album` would not
    /// already find the entire album.
    fn search_track(&self, words: &[String], into: &mut Vec<TrackId>);
}

#[derive(Debug)]
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

/// Indices into a sorted array based on the most significant byte of an id.
///
/// This is a middle ground between storing an additional hash table, which
/// would require O(n) storage, but enable O(1) lookup of an artist, album, or
/// track, and the full binary search, which requires no additional storage,
/// but makes lookups O(log n).
///
/// A hash table would have two cache misses (one for the table, one for the
/// actual value). A binary search has log(n) cache misses (one for every try).
///
/// With the bookmarks, we store ranges into the full array indexed on the most
/// significant byte of the id. We spend 1028 additional bytes for the
/// bookmarks. Lookups are now O(log2(n) - 8). For 10k tracks, log2(n) is only
/// around 13, so we cut the majority of steps off of the binary search, and
/// with that also the cache misses. Furthermore, because the bookmarks table is
/// small unlike a full hash table, it is likely to be cached, so accessing it
/// is essentially free.
struct Bookmarks {
    bookmarks: Box<[u32; 257]>,
}

impl Bookmarks {
    pub fn new<I>(iter: I) -> Bookmarks where I: Iterator<Item = u64> {
        let mut bookmarks = [0; 257];
        let mut bc: i32 = -1;
        let mut len: u32 = 0;
        for id in iter {
            let b = (id >> 56) as u8;
            while bc < b as i32 {
                bc = bc + 1;
                bookmarks[bc as usize] = len;
            }
            assert!(len < u32::MAX);
            len += 1;
        }
        while bc < 256 {
            bc = bc + 1;
            bookmarks[bc as usize] = len;
        }
        Bookmarks {
            bookmarks: Box::new(bookmarks)
        }
    }

    /// Return the subslice of `xs` that contains the given id.
    pub fn range<'a, T>(&self, xs: &'a [T], id: u64) -> &'a [T] {
        let b = (id >> 56) as usize;
        let begin = self.bookmarks[b] as usize;
        let end = self.bookmarks[b + 1] as usize;
        &xs[begin..end]
    }
}

pub struct MemoryMetaIndex {
    // TODO: Use an mmappable data structure. For now this will suffice.
    artists: Vec<(ArtistId, Artist)>,
    albums: Vec<(AlbumId, Album)>,
    tracks: Vec<(TrackId, Track)>,
    // Per artist, all albums, ordered by ascending release date.
    albums_by_artist: Vec<(ArtistId, AlbumId)>,

    // Bookmarks for quick indexing into the above arrays.
    artist_bookmarks: Bookmarks,
    album_bookmarks: Bookmarks,
    track_bookmarks: Bookmarks,
    albums_by_artist_bookmarks: Bookmarks,

    strings: Vec<String>,
    filenames: Vec<String>,

    // TODO: Don't make these pub, this is just for debug printing stats.
    pub words_artist: MemoryWordIndex<ArtistId>,
    pub words_album: MemoryWordIndex<AlbumId>,
    pub words_track: MemoryWordIndex<TrackId>,
}

/// Build the sorted mapping of artist id to album id.
///
/// Entries are sorted by artist id first, so we can use bookmarks and do a
/// binary search. Albums for a single artist are ordered by ascending release
/// date.
fn build_albums_by_artist_index(albums: &[(AlbumId, Album)]) -> Vec<(ArtistId, AlbumId)> {
    let mut entries_with_date = Vec::with_capacity(albums.len());
    let mut entries = Vec::with_capacity(albums.len());

    for &(album_id, ref album) in albums {
        entries_with_date.push((album.artist_id, album_id, album.original_release_date));
    }

    entries_with_date.sort_by_key(|&(artist_id, album_id, release_date)|
        (artist_id, release_date, album_id)
    );

    for (artist_id, album_id, _release_date) in entries_with_date {
        entries.push((artist_id, album_id));
    }

    entries
}

/// Invokes `process` for all elements in the builder, in sorted order.
///
/// The arguments passed to process are `(i, id, value)`, where `i` is the
/// index of the builder. The collection iterated over is determined by
/// `project`. If the collections contain duplicates, all of them are passed to
/// `process`.
fn for_each_sorted<'a, P, I, T, F>(
    builders: &'a [BuildMetaIndex],
    project: P,
    mut process: F,
) where
  P: Fn(&'a BuildMetaIndex) -> btree_map::Iter<'a, I, T>,
  I: Clone + Eq + Ord + 'a,
  T: Clone + Eq + 'a,
  F: FnMut(usize, I, T),
{
    let mut iters: Vec<_> = builders
        .iter()
        .map(project)
        .collect();
    let mut candidates: Vec<_> = iters
        .iter_mut()
        .map(|i| i.next())
        .collect();

    // Apply the processing function to all elements from the builders in order.
    while let Some((i, _)) = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, id_val)| id_val.map(|(id, _val)| (i, id)))
            .min_by_key(|&(_, id)| id)
    {
        let mut next = iters[i].next();
        mem::swap(&mut candidates[i], &mut next);

        // Current now contains the value of `candidates[i]` before the swap,
        // which is not none, so the unwrap is safe.
        let current = next.unwrap();
        process(i, current.0.clone(), current.1.clone());
    }

    // Nothing should be left.
    for candidate in candidates {
        debug_assert!(candidate.is_none());
    }
}


impl MemoryMetaIndex {
    /// Combine builders into a memory-backed index.
    fn new(builders: &[BuildMetaIndex], issues: &mut Vec<Issue>) -> MemoryMetaIndex {
        assert!(builders.len() > 0);
        let mut artists: Vec<(ArtistId, Artist)> = Vec::new();
        let mut albums: Vec<(AlbumId, Album)> = Vec::new();
        let mut tracks: Vec<(TrackId, Track)> = Vec::new();
        let mut strings = StringDeduper::new();
        let mut filenames = Vec::new();
        let mut words_artist = BTreeSet::new();
        let mut words_album = BTreeSet::new();
        let mut words_track = BTreeSet::new();

        for_each_sorted(builders, |b| b.tracks.iter(), |i, id, mut track| {
            // Give the track the final stringrefs, into the merged arrays.
            track.title = StringRef(
                strings.insert(builders[i].strings.get(track.title.0))
            );
            track.artist = StringRef(
                strings.insert(builders[i].strings.get(track.artist.0))
            );
            filenames.push(builders[i].filenames[track.filename.0 as usize].clone());
            track.filename = FilenameRef(filenames.len() as u32 - 1);

            if let Some(&(prev_id, ref _prev)) = tracks.last() {
                assert!(prev_id != id, "Duplicate track should not occur.");
            }

            tracks.push((id, track));
        });

        for_each_sorted(builders, |b| b.albums.iter(), |i, id, mut album| {
            album.title = StringRef(
                strings.insert(builders[i].strings.get(album.title.0))
            );

            if let Some(&(prev_id, ref prev)) = albums.last() {
                if prev_id == id {
                    if let Some(detail) = albums_different(&strings, id, prev, &album) {
                        // Report the file where the conflicting data came from.
                        let fname_index = builders[i].album_sources[&id];
                        let filename = builders[i].filenames[fname_index.0 as usize].clone();
                        let issue = detail.for_file(filename);
                        issues.push(issue);
                    }
                    return // Like `continue`, returns from the closure.
                }
            }

            albums.push((id, album));
        });

        for_each_sorted(builders, |b| b.artists.iter(), |i, id, mut artist| {
            artist.name = StringRef(
                strings.insert(builders[i].strings.get(artist.name.0))
            );
            artist.name_for_sort = StringRef(
                strings.insert(builders[i].strings.get(artist.name_for_sort.0))
            );

            if let Some(&(prev_id, ref prev)) = artists.last() {
                if prev_id == id {
                    if let Some(detail) = artists_different(&strings, id, prev, &artist) {
                        // Report the file where the conflicting data came from.
                        let fname_index = builders[i].artist_sources[&id];
                        let filename = builders[i].filenames[fname_index.0 as usize].clone();
                        let issue = detail.for_file(filename);
                        issues.push(issue);
                    }
                    return // Like `continue`, returns from the closure.
                }
            }

            artists.push((id, artist));
        });

        for builder in builders {
            words_artist.extend(builder.words_artist.iter().cloned());
            words_album.extend(builder.words_album.iter().cloned());
            words_track.extend(builder.words_track.iter().cloned());
        }

        strings.upgrade_quotes();

        // Albums know their artist; build the reverse mapping so we can look up
        // albums by a given artist. We could build it incrementally and merge
        // it, but instead of doing that and having to worry about duplicates,
        // we can just build it once at the end.
        let albums_by_artist = build_albums_by_artist_index(&albums[..]);

        MemoryMetaIndex {
            artist_bookmarks: Bookmarks::new(artists.iter().map(|p| (p.0).0)),
            album_bookmarks: Bookmarks::new(albums.iter().map(|p| (p.0).0)),
            track_bookmarks: Bookmarks::new(tracks.iter().map(|p| (p.0).0)),
            albums_by_artist_bookmarks: Bookmarks::new(albums_by_artist.iter().map(|p| (p.0).0)),
            artists: artists,
            albums: albums,
            tracks: tracks,
            albums_by_artist: albums_by_artist,
            strings: strings.into_vec(),
            filenames: filenames,
            words_artist: MemoryWordIndex::new(&words_artist),
            words_album: MemoryWordIndex::new(&words_album),
            words_track: MemoryWordIndex::new(&words_track),
        }
    }

    /// Read the files from `paths` and add their metadata to the builder.
    ///
    /// `from_paths` spawns multiple threads, and each thread runs `process`.
    ///
    /// This function increments `counter`, processes `paths[counter]`, and
    /// loops until all paths have been indexed. Multiple threads can do this
    /// in parallel because the counter is atomic.
    fn process(paths: &[PathBuf], counter: &AtomicUsize, builder: &mut BuildMetaIndex) {
        let mut progress_unreported = 0;
        loop {
            let i = counter.fetch_add(1, Ordering::SeqCst);
            if i >= paths.len() {
                break;
            }
            let path = &paths[i];
            let opts = claxon::FlacReaderOptions {
                metadata_only: true,
                read_picture: claxon::ReadPicture::Skip,
                read_vorbis_comment: true,
            };
            let reader = claxon::FlacReader::open_ext(path, opts).unwrap();
            builder.insert(path.to_str().expect("TODO"), &reader.streaminfo(), &mut reader.tags());
            progress_unreported += 1;

            // Don't report every track individually, to avoid synchronisation
            // overhead.
            if progress_unreported == 17 {
                builder.progress.as_mut().unwrap().send(Progress::Indexed(progress_unreported)).unwrap();
                progress_unreported = 0;
            }
        }

        if progress_unreported != 0 {
            builder.progress.as_mut().unwrap().send(Progress::Indexed(progress_unreported)).unwrap();
        }

        builder.progress = None;
    }

    /// Index the given files.
    ///
    /// Reports progress to `out`, which can be `std::io::stdout().lock()`.
    pub fn from_paths(paths: &[PathBuf], out: &mut dyn StatusSink) -> Result<MemoryMetaIndex> {
        let (tx_progress, rx_progress) = sync_channel(8);

        // When we are IO bound, we need enough threads to keep the IO scheduler
        // queues fed, so it can schedule optimally and minimize seeks.
        // Therefore, pick a fairly high amount of threads. When we are CPU
        // bound, there is some overheads to more threads, but 8 threads vs 64
        // threads is a difference of maybe 0.05 seconds for 16k tracks, while
        // for the IO-bound case, it can bring down the time from ~140 seconds
        // to ~70 seconds, which is totally worth it.
        let num_threads = 64;
        let mut builders: Vec<_> = (0..num_threads)
            .map(|_| BuildMetaIndex::new(tx_progress.clone()))
            .collect();

        // Drop the original sender to ensure the channel is closed when all
        // threads are done.
        mem::drop(tx_progress);

        let counter = std::sync::atomic::AtomicUsize::new(0);

        crossbeam::scope(|scope| {
            for builder in builders.iter_mut() {
                let counter = &counter;
                scope.spawn(move || MemoryMetaIndex::process(paths, counter, builder));
            }

            // Print issues live as indexing happens.
            let mut count = 0;
            for progress in rx_progress {
                match progress {
                    Progress::Issue(issue) => out.report_issue(&issue)?,
                    Progress::Indexed(n) => {
                        count += n;
                        out.report_index_progress(count, paths.len() as u32)?;
                    }
                }
            }

            // We return `Ok` here so the return type of the scope closure is
            // `io::Result`, which allows using `?` above; that's a bit nicer
            // than unwrapping everywhere. We do unwrap the result below though,
            // because `out` is likely stdout, so printing a nice error would
            // fail anyway.
            let result: io::Result<()> = Ok(());
            result
        }).unwrap();

        let mut issues = Vec::new();
        let memory_index = MemoryMetaIndex::new(&builders, &mut issues);

        // Report issues that resulted from merging.
        for issue in &issues {
            out.report_issue(issue).unwrap();
        }

        out.report_done_indexing().unwrap();

        Ok(memory_index)
    }
}

impl MetaIndex for MemoryMetaIndex {
    #[inline]
    fn len(&self) -> usize {
        self.tracks.len()
    }

    #[inline]
    fn get_string(&self, sr: StringRef) -> &str {
        &self.strings[sr.0 as usize]
    }

    #[inline]
    fn get_filename(&self, sr: FilenameRef) -> &str {
        &self.filenames[sr.0 as usize]
    }

    #[inline]
    fn get_track(&self, id: TrackId) -> Option<&Track> {
        let slice = self.track_bookmarks.range(&self.tracks[..], id.0);
        slice
            .binary_search_by_key(&id, |pair| pair.0)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].1)
    }

    #[inline]
    fn get_album(&self, id: AlbumId) -> Option<&Album> {
        let slice = self.album_bookmarks.range(&self.albums[..], id.0);
        slice
            .binary_search_by_key(&id, |pair| pair.0)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].1)
    }

    #[inline]
    fn get_album_tracks(&self, id: AlbumId) -> &[(TrackId, Track)] {
        // Look for track 0 of disc 0. This is the first track of the album,
        // if it exists. Otherwise binary search would find the first track
        // after it.
        let tid = get_track_id(id, 0, 0);
        // TODO: Use bookmarks for this.
        let begin = match self.tracks.binary_search_by_key(&tid, |pair| pair.0) {
            Ok(i) => i,
            Err(i) => i,
        };
        // Then do a linear scan over the tracks to find the first track that
        // does not belong to the album any more. We could do another binary
        // search to find the end instead, but a binary search would take about
        // 13 random memory accesses for 12k tracks, whereas most albums have
        // less tracks than that, and the linear scan has a very regular memory
        // access pattern.
        let end = begin + self.tracks[begin..]
            .iter()
            .position(|&(_tid, ref track)| track.album_id != id)
            .unwrap_or(self.tracks.len() - begin);

        &self.tracks[begin..end]
    }

    #[inline]
    fn get_tracks(&self) -> &[(TrackId, Track)] {
        &self.tracks
    }

    #[inline]
    fn get_albums(&self) -> &[(AlbumId, Album)] {
        &self.albums
    }

    #[inline]
    fn get_artists(&self) -> &[(ArtistId, Artist)] {
        &self.artists
    }

    #[inline]
    fn get_artist(&self, id: ArtistId) -> Option<&Artist> {
        let slice = self.artist_bookmarks.range(&self.artists[..], id.0);
        slice
            .binary_search_by_key(&id, |pair| pair.0)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].1)
    }

    #[inline]
    fn get_albums_by_artist(&self, artist_id: ArtistId) -> &[(ArtistId, AlbumId)] {
        // Use the bookmarks to narrow down the range of artists that we need to
        // look though.
        let mut candidates = self
            .albums_by_artist_bookmarks
            .range(&self.albums_by_artist[..], artist_id.0);

        // Within that slice, we do a linear search for the start of the artist.
        // For a library with ~400 artists like mine, there will only be one or
        // two artists in the slice anyway, and most artists have few (no more
        // than a dozen) albums. We could use a binary search for better
        // complexity, but the one in `slice` is not suitable for this (it does
        // not return the *first* index with the key, only *a* index), so I'll
        // go with the easy thing for now.
        let begin = candidates
            .iter()
            .position(|&(elem_artist_id, _album_id)| elem_artist_id == artist_id)
            .unwrap_or(candidates.len());
        candidates = &candidates[begin..];

        // Then do a linear scan over the albums to find the first albums that
        // does not belong to the artist any more. We could do another binary
        // search to locate the end, but typically artists have few albums, so
        // we go with a predictible memory access pattern here.
        let end = candidates
            .iter()
            .position(|&(elem_artist_id, _album_id)| elem_artist_id != artist_id)
            .unwrap_or(candidates.len());
        candidates = &candidates[..end];

        // Only the albums for the desired artist are in this slice, and they
        // are already sorted on ascending release date.
        &candidates[..end]
    }

    #[inline]
    fn get_album_ids_ordered_by_artist(&self) -> &[(ArtistId, AlbumId)] {
        &self.albums_by_artist[..]
    }

    fn search_artist(&self, words: &[String], into: &mut Vec<ArtistId>) {
        search::search(&self.words_artist, words, into);
    }

    fn search_album(&self, words: &[String], into: &mut Vec<AlbumId>) {
        search::search(&self.words_album, words, into);
    }

    fn search_track(&self, words: &[String], into: &mut Vec<TrackId>) {
        search::search(&self.words_track, words, into);
    }
}

