// Musium -- Music playback daemon with web-based library browser
// Copyright 2024 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! The matcher locates library tracks from title and other metadata.
//!
//! This is used for importing listening history.

use crate::string_utils::normalize_words;
use crate::{MetaIndex, MemoryMetaIndex};
use crate::build::parse_uuid_52bits;
use crate::error::Result;
use crate::prim::{AlbumId, TrackId};
use crate::{database as db};

enum Match {
    /// An exact match on album Musicbrainz id and track title.
    MbidTitle(TrackId),

    /// An exact match on track title and album, after searching on title and artist.
    ///
    /// “Exact” is still modulo ASCII case, so e.g. `of` vs. `Of` does not
    /// affect the match.
    SearchExact(TrackId),

    /// Like `SearchExact`, except the matched album title is a prefix of the listen.
    ///
    /// In other words, the listen album may have a suffix, e.g. `[Bonus Track]`.
    SearchAlbumPrefix(TrackId),

    /// Like `SearchExact`, but the match is only after normalization.
    ///
    /// Normalization is the same as used for search. It removes various forms
    /// of punctuation and diacritics.
    SearchNormalized(TrackId),

    /// Searching had results, but no exact match.
    SearchFail,

    /// Searching turned up multiple matches.
    Ambiguous,

    /// No match found.
    None,
}

fn match_listen(
    index: &MemoryMetaIndex,
    listen: &db::LastfmListen,
) -> Match {
    let mut album_id = None;
    if !listen.album_mbid.is_empty() {
        album_id = parse_uuid_52bits(&listen.album_mbid).map(AlbumId);
    }

    // If we have an album Musicbrainz id (from which our track ids are
    // derived), try to look for an exact match on track title on that album.
    if let Some(id) = album_id {
        for track_and_id in index.get_album_tracks(id) {
            let title = index.get_string(track_and_id.track.title);
            if title.eq_ignore_ascii_case(&listen.title) {
                return Match::MbidTitle(track_and_id.track_id);
            }
        }
    }

    // If that did not work, we'll try searching.
    let mut words = Vec::new();
    let mut tracks = Vec::new();
    normalize_words(&listen.title, &mut words);
    normalize_words(&listen.track_artist, &mut words);
    // TODO: Add a way to turn off prefix search for the last word.
    index.search_track(&words[..], &mut tracks);

    let n_candidates = tracks.len();
    let mut results = Vec::with_capacity(n_candidates);

    for track_id in tracks {
        let track = index.get_track(track_id).expect("Search result should be in index.");
        let album = index.get_album(track_id.album_id()).expect("Track album should be in index.");
        let track_title = index.get_string(track.title);
        let album_title = index.get_string(album.title);

        let track_exact = track_title.eq_ignore_ascii_case(&listen.title);
        let album_exact = album_title.eq_ignore_ascii_case(&listen.album);

        if track_exact && album_exact {
            results.push(Match::SearchExact(track_id));
            continue;
        }

        let prefix_len = album_title.len();
        let listen_album = &listen.album.as_bytes()[..listen.album.len().min(prefix_len)];

        // Sometimes the album in the listening history has "[Bonus Track]" or
        // "[Deluxe Edition]" suffix or something, but in my collection I prefer
        // to remove those. So try if we have a prefix match.
        if track_exact && album_title.as_bytes().eq_ignore_ascii_case(listen_album) {
            results.push(Match::SearchAlbumPrefix(track_id));
            continue;
        }

        // The most common reason for not finding an exact match is because I
        // turn straight quotes into typographer's quotes (' -> ’), but the
        // scrobble contains the straight one. To mitigate this kind of thing,
        // use the same normalizer as the search function. This also makes the
        // match case-insensitive.
        let track_fuzzy = track_exact || equals_normalized(track_title, &listen.title);
        let album_fuzzy = album_exact || equals_normalized(album_title, &listen.album);
        if track_fuzzy && album_fuzzy {
            results.push(Match::SearchNormalized(track_id));
            continue;
        }
    }

    match results.len() {
        0 if n_candidates > 0 => Match::SearchFail,
        0 => Match::None,
        1 => results.pop().unwrap(),
        _ => Match::Ambiguous,
    }
}

fn equals_normalized(x1: &str, x2: &str) -> bool {
    let mut w1 = Vec::new();
    let mut w2 = Vec::new();
    normalize_words(x1, &mut w1);
    normalize_words(x2, &mut w2);
    w1 == w2
}

pub fn match_listens(
    index: &MemoryMetaIndex,
    tx: &mut db::Transaction,
) -> Result<()> {
    let mut misses: u32 = 0;
    let mut ambiguous: u32 = 0;
    let mut match_mbid_title: u32 = 0;
    let mut match_search_exact: u32 = 0;
    let mut match_search_album_prefix: u32 = 0;
    let mut match_search_normalized: u32 = 0;
    let mut search_fail: u32 = 0;

    for listen_opt in db::iter_lastfm_missing_listens(tx)? {
        let listen = listen_opt?;
        match match_listen(index, &listen) {
            Match::MbidTitle(..) => match_mbid_title += 1,
            Match::SearchExact(..) => match_search_exact += 1,
            Match::SearchAlbumPrefix(..) => match_search_album_prefix += 1,
            Match::SearchNormalized(..) => match_search_normalized += 1,
            Match::Ambiguous => {
                ambiguous += 1;
                println!("AMBIGUOUS {listen:?}");
            }
            Match::SearchFail => {
                search_fail += 1;
                println!("SEARCH_FAIL {listen:?}");
            }
            Match::None => {
                misses += 1;
                println!("MISS {listen:?}");
            }
        }
    }

    let matched = match_mbid_title + match_search_exact + match_search_album_prefix + match_search_normalized;
    let total = matched + misses + ambiguous + search_fail;

    println!("Matched {} of {} ({:.1}%).", matched, total, (matched as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) SearchExact", match_search_exact, total, (match_search_exact as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) MbidTitle", match_mbid_title, total, (match_mbid_title as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) SearchAlbumPrefix", match_search_album_prefix, total, (match_search_album_prefix as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) SearchNormalized", match_search_normalized, total, (match_search_normalized as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) Miss", misses, total, (misses as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) Ambiguous", ambiguous, total, (ambiguous as f32 * 100.0) / total as f32);
    println!(" - {:6} of {:6} ({:4.1}%) SearchFail", search_fail, total, (search_fail as f32 * 100.0) / total as f32);

    Ok(())
}
