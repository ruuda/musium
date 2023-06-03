// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for shuffling playlists.

use std::cmp;
use std::collections::HashMap;
use std::iter;

use nanorand::{Rng, RandomGen};

use crate::{MetaIndex, MemoryMetaIndex};
use crate::player::{QueuedTrack};
use crate::prim::{AlbumId, ArtistId};

pub type Prng = nanorand::WyRand;

/// Trait to decouple metadata lookups from shuffling.
///
/// This is to make the shuffling easier to test without having to construct
/// full `QueuedTrack` instances and a full index.
trait Shuffle {
    type Track;

    fn get_album_id(&self, track: &Self::Track) -> AlbumId;
    fn get_artist_id(&self, album_id: AlbumId) -> ArtistId;
}

/// Shuffle implementation that is actually used in the server.
impl Shuffle for MemoryMetaIndex {
    type Track = QueuedTrack;

    fn get_album_id(&self, track: &QueuedTrack) -> AlbumId {
        track.track_id.album_id()
    }

    fn get_artist_id(&self, album_id: AlbumId) -> ArtistId {
        // For "artist", we take the first album of the album artists. Two
        // alternatives come to mind: counting every collaboration as a unique
        // artist (more smaller groups), or counting every connected component
        // in the graph of artists with edges for collaboration albums (fewer
        // larger groups). If we make artists "more distinct", then we risk
        // placing their tracks consecutively in the final order because we
        // consider them distinct. If we make artists "less distinct", then we
        // risk having too few of them to properly interleave. So one artist per
        // album is probably okay, but also, it’s just the easiest thing to
        // implement.
        let album = self.get_album(album_id).expect("Queued tracks should exist on album.");
        let artist_ids = self.get_album_artists(album.artist_ids);
        artist_ids[0]
    }
}

/// Shuffler for use in tests.
///
/// In the tests we use a triple of bits as the track type:
///
/// * Index 0 identifies the artist.
/// * Index 1 identifies the album within the artist.
/// * Index 2 identifies the track on the album.
///
/// This makes it easy to construct such ids as literals without having to build
/// up large dictionaries etc. It's also easy to fuzz.
pub struct TestShuffler;

impl Shuffle for TestShuffler {
    type Track = [u8; 3];

    fn get_album_id(&self, track: &[u8; 3]) -> AlbumId {
        AlbumId(((track[0] as u64) << 16) | ((track[1] as u64) << 8))
    }

    fn get_artist_id(&self, album_id: AlbumId) -> ArtistId {
        ArtistId(album_id.0 >> 8)
    }
}

/// Index into the queued tracks slice, used internally for shuffling.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct TrackRef(pub u32);

/// Given a list of indexes into `tracks`, put `tracks` in that order.
fn apply_permutation<T>(permutation: &[TrackRef], tracks: &mut [T]) {
    debug_assert_eq!(permutation.len(), tracks.len());

    // Invariant: pos[i] holds the current index (into `tracks`) of the element
    // that was originally at index i.
    let mut pos: Vec<u32> = (0..permutation.len() as u32).collect();

    // Invariant: pos[i] = k <=> inv[k] = i.
    let mut inv: Vec<u32> = (0..permutation.len() as u32).collect();

    for (i, TrackRef(orig_index)) in permutation.iter().enumerate() {
        let j = pos[*orig_index as usize] as usize;
        tracks.swap(i, j);

        let (ii, ij) = (inv[i] as usize, inv[j] as usize);
        pos.swap(ii, ij);
        inv.swap(i, j);
    }
}

fn shuffle<Meta: Shuffle>(
    meta: Meta,
    rng: &mut Prng,
    tracks: &mut [Meta::Track],
) {
    // First we partition all tracks into albums. Rather than moving around the
    // full QueuedTrack all the time, we store indices into the tracks slice.
    let mut albums = HashMap::<AlbumId, Vec<TrackRef>>::new();
    for (i, track) in tracks.iter().enumerate() {
        let album_id = meta.get_album_id(track);
        albums.entry(album_id).or_default().push(TrackRef(i as u32));
    }

    // Then we shuffle the tracks in every album using a regular shuffle.
    // Subsequent interleavings will preserve the relative order of those
    // tracks.
    for album_tracks in albums.values_mut() {
        rng.shuffle(album_tracks);
    }

    // Then we group everything back on artist.
    let mut artists = HashMap::<ArtistId, Vec<Vec<TrackRef>>>::new();
    for (album_id, album_tracks) in albums {
        let artist_id = meta.get_artist_id(album_id);
        artists.entry(artist_id).or_default().push(album_tracks);
    }

    // Then we combine all albums into one partition per artist, using our
    // interleaving shuffle, then we interleave-shuffle the per-artist
    // partitions once more into the final order.
    let artist_partitions: Vec<Vec<TrackRef>> = artists
        .into_values()
        .map(|album_partitions| shuffle_interleave(rng, album_partitions))
        .collect();

    let permutation = shuffle_interleave(rng, artist_partitions);

    // Finally put the right track at the right index.
    apply_permutation(&permutation, tracks);
}

fn shuffle_interleave(rng: &mut Prng, mut partitions: Vec<Vec<TrackRef>>) -> Vec<TrackRef> {
    // Shuffle partitions and then use a stable sort to sort by ascending
    // length. This way, for partitions that are the same size, the merge order
    // is random, which aids the randomness of our shuffle.
    rng.shuffle(&mut partitions);
    partitions.sort_by_key(|v| v.len());

    let mut result = Vec::new();
    for partition in partitions {
        // From the new partition and our intermediate result, determine the
        // longest one, and break ties randomly.
        let (long, short) = match (partition.len(), result.len()) {
            (n, m) if n < m => (result, partition),
            (n, m) if n > m => (partition, result),
            _ if bool::random(rng) => (partition, result),
            _ => (result, partition),
        };

        // We are going to partition the longer vector into spans. Figure out
        // the length of each span. Some spans may have to be 1 element longer,
        // shuffle the lengths.
        let n_spans = cmp::min(short.len() + 1, long.len());
        let span_len = long.len() / n_spans;
        let remainder = long.len() - span_len * n_spans;
        let mut span_lens = Vec::with_capacity(n_spans);
        span_lens.extend(iter::repeat(span_len + 1).take(remainder));
        span_lens.extend(iter::repeat(span_len).take(n_spans - remainder));
        rng.shuffle(&mut span_lens);

        result = Vec::with_capacity(long.len() + short.len());
        let mut src_spans = &long[..];
        let mut src_seps = &short[..];

        let last_span_len = if n_spans > short.len() {
            span_lens.pop().expect("We should not have empty partitions.")
        } else {
            0
        };

        // Fill the output vec with a span and separator alternatingly.
        for span_len in span_lens {
            result.extend_from_slice(&src_spans[..span_len]);
            result.push(src_seps[0]);
            src_spans = &src_spans[span_len..];
            src_seps = &src_seps[1..];
        }

        // Then after the final separator, there can be a final span.
        debug_assert_eq!(src_spans.len(), last_span_len);
        result.extend_from_slice(src_spans);
    }

    result
}

/// Note, see also the `TestShuffler` impl about the track representation.
///
/// Tracks in the tests are slices of the form [Artist, Album, Track]. We can
/// write them as ascii literals for easy visualisation.
#[cfg(test)]
mod test {
    use nanorand::Rng;
    use super::{Prng, TestShuffler, TrackRef, shuffle, apply_permutation};

    #[test]
    fn apply_permutation_is_correct_simple() {
        let p = [TrackRef(3), TrackRef(2), TrackRef(1), TrackRef(0)];
        let mut v = [0, 1, 2, 3];
        apply_permutation(&p, &mut v);
        assert_eq!(v, [3, 2, 1, 0]);

        let p = [TrackRef(0), TrackRef(2), TrackRef(3), TrackRef(1)];
        let mut v = [0, 1, 2, 3];
        apply_permutation(&p, &mut v);
        assert_eq!(v, [0, 2, 3, 1]);
    }

    #[test]
    fn apply_permutation_is_correct_random() {
        let mut rng = Prng::new_seed(42);

        for len in 1..100_u32 {
            for _ in 0..len * 10 {
                // When our initial data is just 0, 1, 2, 3, etc., then a given
                // permutation will reorder it such that the result is equal to
                // the permutation itself.
                let mut v: Vec<u32> = (0..len).collect();
                let mut v_expected = v.clone();
                rng.shuffle(&mut v_expected);
                let p: Vec<_> = v_expected.iter().cloned().map(TrackRef).collect();
                apply_permutation(&p, &mut v);
                assert_eq!(v, v_expected);
            }
        }
    }

    /// Test that `shuffle` produces one of the given optimal shuffles.
    ///
    /// The input to the process is itself a shuffle of the first expected
    /// entry.
    fn test_shuffle(expected: &[&[[u8; 3]]]) {
        let mut rng = Prng::new_seed(42);

        for _ in 0..10_000 {
            let mut tracks: Vec<_> = expected[0].into();
            rng.shuffle(&mut tracks);
            shuffle(TestShuffler, &mut rng, &mut tracks);
            assert!(
                expected.contains(&&tracks[..]),
                "Unexpected shuffle: {:?}",
                tracks.iter().map(|x| std::str::from_utf8(x).unwrap()).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn shuffle_interleaves_artist() {
        // With this input, there is only one possible optimal shuffle.
        test_shuffle(&[&[*b"A00", *b"B00", *b"A00"]]);

        // Here we have freedom to flip B and C, but A surrounds them.
        test_shuffle(&[
            &[*b"A00", *b"B00", *b"A00", *b"C00", *b"A00"],
            &[*b"A00", *b"C00", *b"A00", *b"B00", *b"A00"],
        ]);

        // Here we interleave, but either artist can go first.
        test_shuffle(&[
            &[*b"A00", *b"B00", *b"A00", *b"B00"],
            &[*b"B00", *b"A00", *b"B00", *b"A00"],
        ]);
    }
}
