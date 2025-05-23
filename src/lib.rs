// Musium -- Music playback daemon with web-based library browser
// Copyright 2017 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// Disable some of Clippy's opinions that I disagree with.
#![allow(clippy::identity_op)]
#![allow(clippy::len_without_is_empty)]
#![allow(clippy::len_zero)]
#![allow(clippy::let_and_return)]
#![allow(clippy::new_without_default)]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::redundant_field_names)]
// Harmful lint, see https://github.com/rust-lang/rust-clippy/issues/4295#issuecomment-1554996707.
#![allow(clippy::mutex_atomic)]

extern crate alsa;
extern crate bs1770;
extern crate claxon;
extern crate crossbeam;
extern crate libc;
extern crate serde_json;
extern crate unicode_normalization;

mod album_table;
mod build;
mod exec_pre_post;
mod filter;
mod loudness;
mod search;
mod waveform;
mod word_index;

pub mod config;
pub mod database;
pub mod database_utils;
pub mod error;
pub mod history;
pub mod matcher;
pub mod mvar;
pub mod playback;
pub mod playcount;
pub mod player;
pub mod prim;
pub mod scan;
pub mod serialization;
pub mod server;
pub mod shuffle;
pub mod string_utils;
pub mod systemd;
pub mod thumb_cache;
pub mod thumb_gen;
pub mod user_data;

use crate::build::{AlbumArtistsDeduper, BuildError, BuildMetaIndex};
use crate::error::{Error, Result};
use crate::prim::{
    Album, AlbumArtistsRef, AlbumId, Artist, ArtistId, FilenameRef, Lufs, StringRef, Track, TrackId,
};
use crate::prim::{AlbumWithId, ArtistWithId, Color, TrackWithId};
use crate::string_utils::StringDeduper;
use crate::word_index::MemoryWordIndex;

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

    /// Return all the artists of a given album.
    fn get_album_artists(&self, range: AlbumArtistsRef) -> &[ArtistId];

    /// Return all tracks that are part of the album.
    fn get_album_tracks(&self, id: AlbumId) -> &[TrackWithId];

    /// Return all tracks, ordered by id.
    fn get_tracks(&self) -> &[TrackWithId];

    /// Return all albums, ordered by id.
    fn get_albums(&self) -> &[AlbumWithId];

    /// Return all album artists, ordered by id.
    fn get_artists(&self) -> &[ArtistWithId];

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
#[derive(Clone)]
struct Bookmarks {
    bookmarks: Box<[u32; 257]>,
}

impl Bookmarks {
    pub fn new<I>(iter: I) -> Bookmarks
    where
        I: Iterator<Item = u64>,
    {
        let mut bookmarks = [0; 257];
        let mut bc: i32 = -1;
        let mut len: u32 = 0;
        for id in iter {
            let b = (id >> 56) as u8;
            while bc < b as i32 {
                bc += 1;
                bookmarks[bc as usize] = len;
            }
            assert!(len < u32::MAX);
            len += 1;
        }
        while bc < 256 {
            bc += 1;
            bookmarks[bc as usize] = len;
        }
        Bookmarks {
            bookmarks: Box::new(bookmarks),
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

#[derive(Clone)]
pub struct MemoryMetaIndex {
    artists: Vec<ArtistWithId>,
    albums: Vec<AlbumWithId>,
    tracks: Vec<TrackWithId>,

    // Per artist, all albums, ordered by ascending release date.
    albums_by_artist: Vec<(ArtistId, AlbumId)>,

    // Bookmarks for quick indexing into the above arrays.
    artist_bookmarks: Bookmarks,
    album_bookmarks: Bookmarks,
    track_bookmarks: Bookmarks,
    albums_by_artist_bookmarks: Bookmarks,

    strings: Vec<String>,
    filenames: Vec<String>,
    album_artists: Vec<ArtistId>,

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
fn build_albums_by_artist_index(
    albums: &[AlbumWithId],
    album_artists: &AlbumArtistsDeduper,
) -> Vec<(ArtistId, AlbumId)> {
    // Add a bit of headroom, most albums have one artist, but some albums have
    // multiple.
    let mut entries_with_date = Vec::with_capacity(albums.len() * 40 / 32);

    for kv in albums {
        for album_artist_id in album_artists.get(kv.album.artist_ids) {
            entries_with_date.push((
                *album_artist_id,
                kv.album_id,
                kv.album.original_release_date,
            ));
        }
    }

    entries_with_date
        .sort_by_key(|&(artist_id, album_id, release_date)| (artist_id, release_date, album_id));

    let mut entries = Vec::with_capacity(entries_with_date.len());

    for (artist_id, album_id, _release_date) in entries_with_date {
        entries.push((artist_id, album_id));
    }

    entries
}

impl MemoryMetaIndex {
    /// Convert the builder into a memory-backed index.
    fn new(builder: &BuildMetaIndex) -> MemoryMetaIndex {
        let mut artists: Vec<ArtistWithId> = Vec::with_capacity(builder.artists.len());
        let mut albums: Vec<AlbumWithId> = Vec::with_capacity(builder.albums.len());
        let mut tracks: Vec<TrackWithId> = Vec::with_capacity(builder.tracks.len());
        let mut album_artists = AlbumArtistsDeduper::new();
        let mut strings = StringDeduper::new();
        let mut filenames = Vec::new();

        for (id, track) in builder.tracks.iter() {
            let (id, mut track) = (*id, track.clone());

            // Give the track the final stringrefs, into the merged arrays.
            track.title = StringRef(strings.insert(builder.strings.get(track.title.0)));
            track.artist = StringRef(strings.insert(builder.strings.get(track.artist.0)));
            filenames.push(builder.filenames[track.filename.0 as usize].clone());
            track.filename = FilenameRef(filenames.len() as u32 - 1);

            tracks.push(TrackWithId {
                track_id: id,
                track,
            });
        }

        // This should be enforced by the repr(align), but confirm this at
        // runtime to double check that I am using the right types.
        let tracks_addr = tracks[..].as_ptr() as *const u8;
        let align_off = tracks_addr.align_offset(32);
        assert_eq!(
            align_off, 0,
            "Tracks table must align to 32 bytes so elements do not straddle cache lines."
        );

        for (id, album) in builder.albums.iter() {
            let (id, mut album) = (*id, album.clone());

            album.title = StringRef(strings.insert(builder.strings.get(album.title.0)));
            album.artist = StringRef(strings.insert(builder.strings.get(album.artist.0)));

            // We could simply copy the album artists vec from the builder, and
            // use the indices unmodified, but then the entries would be in
            // arbitrary order. We remap them here such that the data is in the
            // same order as the albums, so if you iterate the albums, this is
            // more cache efficient.
            album.artist_ids =
                album_artists.insert(builder.album_artists.get(album.artist_ids).iter().cloned());

            // We may have edited a file after listening to it (for example to
            // fix some tags). In that case the mtime will be recent, and that
            // will mess with the “first seen” field. But if we have listens for
            // this album that are older, then we can use those to correct the
            // first seen time.
            if let Some(first_listen) = builder.album_first_listens.get(&id) {
                album.first_seen = album.first_seen.min(*first_listen);
            }

            albums.push(AlbumWithId {
                album_id: id,
                album,
            });
        }

        for (id, artist) in builder.artists.iter() {
            let (id, mut artist) = (*id, artist.clone());

            artist.name = StringRef(strings.insert(builder.strings.get(artist.name.0)));
            artist.name_for_sort =
                StringRef(strings.insert(builder.strings.get(artist.name_for_sort.0)));

            artists.push(ArtistWithId {
                artist_id: id,
                artist,
            });
        }

        strings.upgrade_quotes();

        let albums_by_artist = build_albums_by_artist_index(&albums[..], &album_artists);

        MemoryMetaIndex {
            artist_bookmarks: Bookmarks::new(artists.iter().map(|p| p.artist_id.0)),
            album_bookmarks: Bookmarks::new(albums.iter().map(|p| p.album_id.for_bookmark())),
            track_bookmarks: Bookmarks::new(tracks.iter().map(|p| p.track_id.0)),
            albums_by_artist_bookmarks: Bookmarks::new(albums_by_artist.iter().map(|p| (p.0).0)),
            artists: artists,
            albums: albums,
            tracks: tracks,
            albums_by_artist: albums_by_artist,
            strings: strings.into_vec(),
            filenames: filenames,
            album_artists: album_artists.into_vec(),
            words_artist: MemoryWordIndex::new(&builder.words_artist),
            words_album: MemoryWordIndex::new(&builder.words_album),
            words_track: MemoryWordIndex::new(&builder.words_track),
        }
    }

    /// Create a new empty index.
    ///
    /// This is useful as a placeholder value when the real index is still being
    /// contstructed.
    pub fn new_empty() -> MemoryMetaIndex {
        MemoryMetaIndex {
            artist_bookmarks: Bookmarks::new(std::iter::empty()),
            album_bookmarks: Bookmarks::new(std::iter::empty()),
            track_bookmarks: Bookmarks::new(std::iter::empty()),
            albums_by_artist_bookmarks: Bookmarks::new(std::iter::empty()),
            artists: Vec::new(),
            albums: Vec::new(),
            tracks: Vec::new(),
            albums_by_artist: Vec::new(),
            album_artists: Vec::new(),
            strings: Vec::new(),
            filenames: Vec::new(),
            words_artist: MemoryWordIndex::new(std::iter::empty()),
            words_album: MemoryWordIndex::new(std::iter::empty()),
            words_track: MemoryWordIndex::new(std::iter::empty()),
        }
    }

    /// Build an index from the data stored in the database.
    ///
    /// Also returns the intermediate builder. It contains any issues
    /// discovered, and the mtimes per album, which can be used to check if any
    /// thumbnails need updating.
    pub fn from_database(
        tx: &mut database::Transaction,
    ) -> Result<(MemoryMetaIndex, BuildMetaIndex)> {
        let mut builder = BuildMetaIndex::new();
        let mut tasks = Vec::new();

        for file in database::iter_files(tx)? {
            match builder.insert_meta(file?) {
                Ok(task) => tasks.push(task),
                Err(BuildError::DbError(err)) => return Err(Error::from(err)),
                Err(BuildError::FileFailed) => continue,
            }
        }

        for task in tasks {
            match builder.insert_full(tx, task) {
                Ok(()) => continue,
                Err(BuildError::DbError(err)) => return Err(Error::from(err)),
                Err(BuildError::FileFailed) => continue,
            }
        }

        builder.insert_first_listens(tx)?;

        let memory_index = MemoryMetaIndex::new(&builder);

        Ok((memory_index, builder))
    }

    /// Reload loudness data and thumbnail colors.
    ///
    /// When we do a scan, we first build the index, then in the background we can extract
    /// thumbnails and analyze loudness, but that means that new data is not yet integrated into
    /// the index. This method reloads that data.
    pub fn reload_from_database(&mut self, tx: &mut database::Transaction) -> Result<()> {
        // TODO: We do individual queries here, this could be more efficient if
        // we select everything in order from the database, and then update in
        // a merge-join fashion.
        for item in self.albums.iter_mut() {
            item.album.loudness = database::select_album_loudness_lufs(tx, item.album_id.0 as i64)?
                .map(Lufs::from_f64);
            item.album.color = database::select_album_color(tx, item.album_id.0 as i64)?
                .as_deref()
                .and_then(Color::parse)
                .unwrap_or_default();
        }

        for item in self.tracks.iter_mut() {
            item.track.loudness = database::select_track_loudness_lufs(tx, item.track_id.0 as i64)?
                .map(Lufs::from_f64);
        }

        Ok(())
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
            .binary_search_by_key(&id, |kv| kv.track_id)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].track)
    }

    #[inline]
    fn get_album(&self, id: AlbumId) -> Option<&Album> {
        let slice = self
            .album_bookmarks
            .range(&self.albums[..], id.for_bookmark());
        slice
            .binary_search_by_key(&id, |kv| kv.album_id)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].album)
    }

    #[inline]
    fn get_album_artists(&self, range: AlbumArtistsRef) -> &[ArtistId] {
        &self.album_artists[range.begin as usize..range.end as usize]
    }

    #[inline]
    fn get_album_tracks(&self, id: AlbumId) -> &[TrackWithId] {
        // Look for track 0 of disc 0. This is the first track of the album,
        // if it exists. Otherwise binary search would find the first track
        // after it.
        let tid = TrackId::new(id, 0, 0);

        // We can leverage the bookmarks, because they index on most significant
        // byte of the id, but the track ids of the same album only differ in
        // the least significant two bytes, so all tracks of a given album are
        // in the same bookmarks slice.
        let slice = self.track_bookmarks.range(&self.tracks[..], tid.0);

        let begin = match slice.binary_search_by_key(&tid, |kv| kv.track_id) {
            Ok(i) => i,
            Err(i) => i,
        };
        // Then do a linear scan over the tracks to find the first track that
        // does not belong to the album any more. We could do another binary
        // search to find the end instead, but a binary search would take about
        // 13 random memory accesses for 12k tracks, whereas most albums have
        // less tracks than that, and the linear scan has a very regular memory
        // access pattern.
        let next_album_tid = TrackId::new(AlbumId(id.0 + 1), 0, 0);
        let end = begin
            + slice[begin..]
                .iter()
                .position(|kv| kv.track_id >= next_album_tid)
                .unwrap_or(slice.len() - begin);

        &slice[begin..end]
    }

    #[inline]
    fn get_tracks(&self) -> &[TrackWithId] {
        &self.tracks
    }

    #[inline]
    fn get_albums(&self) -> &[AlbumWithId] {
        &self.albums
    }

    #[inline]
    fn get_artists(&self) -> &[ArtistWithId] {
        &self.artists
    }

    #[inline]
    fn get_artist(&self, id: ArtistId) -> Option<&Artist> {
        let slice = self.artist_bookmarks.range(&self.artists[..], id.0);
        slice
            .binary_search_by_key(&id, |kv| kv.artist_id)
            .ok()
            // TODO: Remove bounds check.
            .map(|idx| &slice[idx].artist)
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
