// Mindec -- Music metadata indexer
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

#[macro_use] extern crate criterion;

extern crate mindec;
extern crate walkdir;

use std::ffi::OsStr;
use std::path::PathBuf;

use criterion::{Bencher, Criterion, black_box};
use mindec::{AlbumId, MetaIndex, MemoryMetaIndex};

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
    MemoryMetaIndex::from_paths(&paths[..], sink).expect("Failed to build index.")
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
        use std::u64;
        let id = album.next().unwrap().1.artist_id;
        let mut artist = None;

        unsafe {
            let mut low: usize = 0;
            let mut high: usize = artists.len();
            let mut id_low: u64 = (artists.get_unchecked(0).0).0;
            let mut id_high: u64 = (artists.get_unchecked(high - 1).0).0;

            if id.0 == id_low {
                artist = Some(&artists.get_unchecked(0).1);
                black_box(artist);
                return
            }
            if id.0 == id_high {
                artist = Some(&artists.get_unchecked(high - 1).1);
                black_box(artist);
                return
            }

            while low + 1 < high {
                let khilo = (high - low) as u64 - 2;
                let iindx = (id.0 - id_low) >> 32;
                let ihilo = (id_high - id_low) >> 32;
                let mid = low + 1 + ((khilo * iindx + ihilo / 2) / ihilo) as usize;
                let id_mid = (artists.get_unchecked(mid).0).0;

                if id_mid == id.0 {
                    artist = Some(&artists.get_unchecked(mid).1);
                    break
                } else if id_mid < id.0 {
                    low = mid;
                    id_low = id_mid;
                } else {
                    high = mid;
                    id_high = id_mid;
                }
            }
        }
        black_box(artist);
    });
}

fn bench_get_album(b: &mut Bencher) {
    let index = build_index();
    // Create a list of album ids that has a different order than the albums,
    // to ensure that the memory access pattern is random.
    let mut aids: Vec<AlbumId> = index.get_albums().iter().map(|p| p.0).collect();
    aids.sort_by_key(|id| id.0.wrapping_mul(17179869107));
    let mut album_ids = aids.iter().cycle();
    b.iter(|| {
        let id = album_ids.next().unwrap();
        let album = index.get_album(*id).unwrap();
        black_box(album);
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("get_album", bench_get_album);

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
