#[macro_use] extern crate criterion;

extern crate metaindex;
extern crate walkdir;

use std::ffi::OsStr;
use std::path::PathBuf;

use criterion::{Bencher, Criterion, black_box};
use metaindex::{MetaIndex, MemoryMetaIndex};

fn build_index() -> MemoryMetaIndex {
    // TODO: Do not hard-code path.
    // TODO: Do not duplicate this code across benchmark and main.rs.
    let wd = walkdir::WalkDir::new("/home/ruud/music")
        .follow_links(true)
        .max_open(128);

    let flac_ext = OsStr::new("flac");

    let paths: Vec<_> = wd
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| PathBuf::from(e.path()))
        .filter(|p| p.extension() == Some(flac_ext))
        .collect();

    let mut sink = std::io::sink();
    MemoryMetaIndex::from_paths(paths.iter(), sink).expect("Failed to build index.")
}

fn bench_get_artist(b: &mut Bencher) {
    let index = build_index();
    let mut album = index.get_albums().iter().cycle();
    b.iter(|| {
        let id = album.next().unwrap().1.artist_id;
        let artist = index.get_artist(id).unwrap();
        black_box(artist);
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("get_artist", bench_get_artist);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        // Do not use p = 0.05, we are not doing social studies here. I want to
        // actually be sure, and not be wrong 1 in 20 times, because I will run
        // the benchmark more than 20 times for sure.
        .significance_level(0.001)
        .confidence_level(0.99);
    targets = criterion_benchmark
}

criterion_main!(benches);
