#![no_main]

use std::collections::HashMap;

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use musium::shuffle::{Prng, TestShuffler, shuffle};

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    random_seed: u64,
    // See also the definition of `TestShuffler` for why track is [u8; 3].
    tracks: Vec<[u8; 3]>,
}

/// Return the theoretical lowest possible 2-badness for the given tracks.
///
/// This is independent of the order of the tracks.
///
/// TODO: Add link to blog post once published.
fn get_optimal_2_badness(tracks: &[[u8; 3]]) -> usize {
    if tracks.is_empty() {
        return 0;
    }

    // Count how often every artist occurs in the list.
    let mut artist_count = HashMap::<u8, usize>::new();
    for track in tracks {
        let artist = track[0];
        let count = artist_count.entry(artist).or_insert(0);
        *count += 1;
    }

    let mut counts: Vec<usize> = artist_count.into_values().collect();
    counts.sort();
    counts.reverse();

    let n = counts[0];
    let m = tracks.len() - n;

    if n <= m + 1 {
        // The case where we have enough to interleave so 2-badness is zero.
        0
    } else {
        // The case where some amount of 2-badness is unavoidable. All of the n
        // tracks by this artist have a 2-badness of n-1, and we have m tracks
        // to break up one pair, so n - 1 - m is left.
        n - 1 - m
    }
}

/// Compute the 2-badness of the playlist, from its definition.
///
/// TODO: Add link to blog post.
fn get_actual_2_badness(tracks: &[[u8; 3]]) -> usize {
    if tracks.is_empty() {
        return 0;
    }

    let mut badness = 0_usize;
    let mut artist = match tracks.first() {
        None => return 0,
        Some([a, _, _]) => a,
    };

    for [a, _, _] in &tracks[1..] {
        if a == artist {
            badness += 1;
        }
        artist = a;
    }

    badness
}

fuzz_target!(|input: FuzzInput| {
    let mut tracks = input.tracks;
    let mut rng = Prng::new_seed(input.random_seed);

    shuffle(TestShuffler, &mut rng, &mut tracks);

    let expected_badness = get_optimal_2_badness(&tracks);
    let actual_badness = get_actual_2_badness(&tracks);

    assert_eq!(actual_badness, expected_badness);
});
