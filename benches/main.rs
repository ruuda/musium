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

    let sink = std::io::sink();
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

fn bench_get_artist_bsearch(b: &mut Bencher) {
    let index = build_index();
    let mut album = index.get_albums().iter().cycle();
    let artists = index.get_artists();
    b.iter(|| {
        let id = album.next().unwrap().1.artist_id;
        let artist = artists
            .binary_search_by_key(&id, |pair| pair.0)
            .ok()
            .map(|idx| &artists[idx].1)
            .unwrap();
        black_box(artist);
    });
}

fn bench_get_artist_isearch(b: &mut Bencher) {
    let index = build_index();
    let mut album = index.get_albums().iter().cycle();
    let artists = index.get_artists();
    b.iter(|| {
        let id = album.next().unwrap().1.artist_id;
        let k = (((id.0 >> 32) * artists.len() as u64) >> 32) as usize;
        let mid = artists[k].0;
        let (low, high) = if id < mid { (0, k) } else { (k, artists.len()) };
        let artist = artists[low..high]
            .binary_search_by_key(&id, |pair| pair.0)
            .ok()
            .map(|idx| &artists[idx].1)
            .unwrap();
        black_box(artist);
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("get_artist", bench_get_artist);
    c.bench_function("get_artist_bsearch", bench_get_artist_bsearch);
    c.bench_function("get_artist_isearch", bench_get_artist_isearch);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        // Do not use p = 0.05, we are not doing social studies here. I want to
        // actually be sure, and not be wrong 1 in 20 times, because I will run
        // the benchmark more than 20 times for sure.
        .significance_level(0.0005)
        .confidence_level(0.99)
        // The default 100 samples yielded results that could differ by 10% per
        // run; use more samples then.
        .sample_size(500);
    targets = criterion_benchmark
}

criterion_main!(benches);
