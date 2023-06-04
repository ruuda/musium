#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use musium::shuffle::{Prng, TestShuffler, shuffle};

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    random_seed: u64,
    // See also the definition of `TestShuffler` for why track is [u8; 3].
    tracks: Vec<[u8; 3]>,
}

fuzz_target!(|input: FuzzInput| {
    let mut rng = Prng::new_seed(input.random_seed);
    let mut tracks = input.tracks;

    shuffle(TestShuffler, &mut rng, &mut tracks);

    // TODO: Assert that the shuffle is optimal.
});
