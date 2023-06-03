// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for shuffling playlists.

use crate::{MetaIndex, MemoryMetaIndex};
use crate::player::{QueuedTrack};
use crate::prim::{TrackId, AlbumId, ArtistId};

use std::collections::HashMap;

type Rng = rand_chacha::ChaCha8Rng;

/// Intermediate representation of a queued track used for shuffling.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct TrackRef {
    /// Index into the original slice of tracks.
    index: usize,

    album_id: AlbumId,

    /// The first artist of the album.
    ///
    /// For shuffle we only take the album's first artist into account. We could
    /// instead use unique combination of artists (more groups), or the
    /// connected component of artists connected by collaborating on an album
    /// (fewer groups), but more groups, when the groups are not really
    /// different, creates more risk of playing the same artist consecutively,
    /// while fewer groups leaves us with fewer other groups to interleave with,
    /// and besides searching for the connected component in the graph is
    /// overkill for simply shuffling a playlist. So we just pick the first
    /// album artist to partitions on.
    artist_id: ArtistId,
}

fn shuffle(
    index: &MemoryMetaIndex,
    rng: &mut Rng,
    tracks: &mut [QueuedTrack],
) {
    let mut refs = Vec::with_capacity(tracks.len());

    for (i, track) in tracks.iter().enumerate() {
        let album_id = track.track_id.album_id();
        let album = index
            .get_album(album_id)
            .expect("Queued track must exist in the index.");
        let artists = index.get_album_artists(album.artist_ids);
        let artist_id = artists[0];
        refs.push(TrackRef {
            index: i,
            album_id: album_id,
            artist_id: artist_id,
        });
    }

    refs = shuffle_internal_outer(rng, refs);

    todo!("Apply the permutation.");
}

fn shuffle_internal_outer(rng: &mut Rng, tracks: Vec<TrackRef>) -> Vec<TrackRef> {
    let mut partitions = HashMap::<_, Vec<TrackRef>>::new();
    let result = Vec::with_capacity(tracks.len());

    // Partition on artist first.
    for track in tracks {
        partitions
            .entry(track.artist_id)
            .or_default()
            .push(track);
    }

    todo!("Shuffle the artists internally.");
    todo!("Merge.");

    result
}

fn shuffle_internal_inner(rng: &mut Rng, tracks: Vec<TrackRef>) -> Vec<TrackRef> {
    let mut partitions = HashMap::<_, Vec<TrackRef>>::new();
    let result = Vec::with_capacity(tracks.len());

    // Partition on album.
    for track in tracks {
        partitions
            .entry(track.album_id)
            .or_default()
            .push(track);
    }

    todo!("Shuffle the albums internally.");
    todo!("Merge");

    result
}
