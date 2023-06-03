// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for shuffling playlists.

use std::hash::Hash;
use std::collections::HashMap;

use rand::seq::SliceRandom;
use rand::Rng;

use crate::{MetaIndex, MemoryMetaIndex};
use crate::player::{QueuedTrack};
use crate::prim::{TrackId, AlbumId, ArtistId};


type Prng = rand_chacha::ChaCha8Rng;

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

fn partition_on<T: Hash + Eq, F: Fn(&TrackRef) -> T>(
    tracks: Vec<TrackRef>,
    partition_key: F,
) -> Vec<Vec<TrackRef>> {
    let mut partitions = HashMap::<_, Vec<TrackRef>>::new();

    // Partition on artist first.
    for track in tracks {
        partitions
            .entry(partition_key(&track))
            .or_default()
            .push(track);
    }

    partitions.into_values().collect()
}

fn shuffle(
    index: &MemoryMetaIndex,
    rng: &mut Prng,
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

    shuffle_internal(rng, &mut refs);

    todo!("Apply the permutation.");
}

fn shuffle_internal(rng: &mut Prng, tracks: &mut Vec<TrackRef>) {
    // Take out the tracks and leave an empty vec in its place, we will
    // construct the result into there later.
    let mut result = Vec::new();
    std::mem::swap(&mut result, tracks);

    let mut partitions = partition_on(result, |t| t.artist_id);

    partitions.sort_unstable_by_key(|v| v.len());

    // Shuffle every artist by itself before we combine them.
    for partition in &mut partitions {
        shuffle_internal_artist(rng, partition);
    }

    todo!("Merge.");
}

fn shuffle_internal_artist(rng: &mut Prng, tracks: &mut Vec<TrackRef>) {
    // Take out the tracks and leave an empty vec in its place, we will
    // construct the result into there later.
    let mut result = Vec::new();
    std::mem::swap(&mut result, tracks);

    let mut partitions = partition_on(result, |t| t.album_id);

    // Shuffle every album by itself before we combine them,
    // using a regular shuffle.
    for partition in &mut partitions {
        partition.shuffle(rng);
    }

    todo!("Merge");
}

fn merge_shuffle(rng: &mut Prng, mut partitions: Vec<Vec<TrackRef>>) -> Vec<TrackRef> {
    // Shuffle partitions and then use a stable sort to sort by ascending
    // length. This way, for partitions that are the same size, the ties are
    // broken randomly.
    partitions.shuffle(rng);
    partitions.sort_by_key(|v| v.len());

    let mut result = Vec::new();
    for partition in partitions {
        // From the new partition and our intermediate result, determine the
        // longest one, and break ties randomly.
        let (long, short) = match (result.len(), partition.len()) {
            (n, m) if n < m => (result, partition),
            (n, m) if n > m => (partition, result),
            _ if rng.gen_bool(0.5) => (partition, result),
            _ => (result, partition),
        };

        result = long;
    }

    result
}
