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
    SearchExact(TrackId),

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
            if title == &listen.title {
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

    if tracks.len() > 1 {
        return Match::Ambiguous;
    }

    for track_id in tracks {
        let track = index.get_track(track_id).expect("Search result should be in index.");
        let album = index.get_album(track_id.album_id()).expect("Track album should be in index.");
        let track_title = index.get_string(track.title);
        let album_title = index.get_string(album.title);
        if &listen.title == track_title && &listen.album == album_title {
            return Match::SearchExact(track_id);
        } else {
            return Match::SearchFail;
        }
    }

    Match::None
}

pub fn match_listens(
    index: &MemoryMetaIndex,
    tx: &mut db::Transaction,
) -> Result<()> {
    let mut misses: u32 = 0;
    let mut ambiguous: u32 = 0;
    let mut match_mbid_title: u32 = 0;
    let mut match_search_exact: u32 = 0;
    let mut search_fail: u32 = 0;

    for listen_opt in db::iter_lastfm_missing_listens(tx)? {
        let listen = listen_opt?;
        match match_listen(index, &listen) {
            Match::MbidTitle(..) => match_mbid_title += 1,
            Match::SearchExact(..) => match_search_exact += 1,
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

    let matched = match_mbid_title + match_search_exact;
    let total = matched + misses + ambiguous + search_fail;

    println!("Matched {} of {} ({:.1}%).", matched, total, (matched as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) missed.", misses, total, (misses as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) ambiguous.", ambiguous, total, (ambiguous as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) SearchFail.", search_fail, total, (search_fail as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) MbidTitle.", match_mbid_title, total, (match_mbid_title as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) Search.", match_search_exact, total, (match_search_exact as f32 * 100.0) / total as f32);

    Ok(())
}
