// Musium -- Music playback daemon with web-based library browser
// Copyright 2024 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! The matcher locates library tracks from title and other metadata.
//!
//! This is used for importing listening history.

use crate::{MetaIndex, MemoryMetaIndex};
use crate::build::parse_uuid_52bits;
use crate::database::LastfmListen;
use crate::error::Result;
use crate::prim::{AlbumId, TrackId};
use crate::{database as db};

enum Match {
    /// An exact match on album Musicbrainz id and track title.
    MbidTitle(TrackId),

    /// An exact match searching on track title and artist, then confirming the album.
    SearchTitleArtist(TrackId),

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

    if let Some(id) = album_id {
        for track_and_id in index.get_album_tracks(id) {
            let title = index.get_string(track_and_id.track.title);
            if title == &listen.title {
                return Match::MbidTitle(track_and_id.track_id);
            }
        }
    }

    Match::None
}

pub fn match_listens(
    index: &MemoryMetaIndex,
    tx: &mut db::Transaction,
) -> Result<()> {
    let mut misses: u32 = 0;
    let mut match_mbid_title: u32 = 0;
    let mut match_search_title_artist: u32 = 0;

    for listen_opt in db::iter_lastfm_missing_listens(tx)? {
        let listen = listen_opt?;
        match match_listen(index, &listen) {
            Match::MbidTitle(..) => match_mbid_title += 1,
            Match::SearchTitleArtist(..) => match_search_title_artist += 1,
            Match::None => {
                misses += 1;
                println!("MISS {listen:?}");
            }
        }
    }

    let matched = match_mbid_title + match_search_title_artist;
    let total = matched + misses;

    println!("Matched {} of {} ({:.1}%).", matched, total, (matched as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) missed.", misses, total, (misses as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) MbidTitle.", match_mbid_title, total, (match_mbid_title as f32 * 100.0) / total as f32);
    println!(" - {} of {} ({:.1}%) SearchTitleArtist.", match_search_title_artist, total, (match_search_title_artist as f32 * 100.0) / total as f32);

    Ok(())
}
