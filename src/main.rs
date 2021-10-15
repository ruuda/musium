// Musium -- Music playback daemon with web-based library browser
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

extern crate claxon;
extern crate crossbeam;
extern crate musium;
extern crate serde_json;
extern crate tiny_http;
extern crate url;
extern crate walkdir;

use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::io;
use std::path::Path;
use std::process;
use std::sync::Arc;

use musium::config::Config;
use musium::error::Result;
use musium::mvar::MVar;
use musium::server::{MetaServer, serve};
use musium::string_utils::normalize_words;
use musium::thumb_cache::ThumbCache;
use musium::{MetaIndex, MemoryMetaIndex};

fn make_index(db_path: &Path) -> Result<MemoryMetaIndex> {
    let (index, builder) = MemoryMetaIndex::from_database(&db_path)?;

    for issue in &builder.issues {
        println!("{}\n", issue);
    }

    println!(
        "Index has {} artists, {} albums, and {} tracks.",
        index.get_artists().len(),
        index.get_albums().len(),
        index.len()
    );

    let mut track_louds = Vec::new();
    for &(track_id, ref track) in index.get_tracks() {
        if let Some(lufs) = track.loudness {
            track_louds.push((lufs, track_id));
        }
    }
    track_louds.sort();
    let track_loud_min = track_louds[0];
    let track_loud_max = track_louds[track_louds.len() - 1];
    println!(
        "\nSoftest track: {} by {} at {}.",
        index.get_string(index.get_track(track_loud_min.1).unwrap().title),
        index.get_string(index.get_track(track_loud_min.1).unwrap().artist),
        track_loud_min.0,
    );
    println!(
        "Loudest track: {} by {} at {}.",
        index.get_string(index.get_track(track_loud_max.1).unwrap().title),
        index.get_string(index.get_track(track_loud_max.1).unwrap().artist),
        track_loud_max.0,
    );
    println!(
        "Track loudness p5, p50, p95: {}, {}, {}",
        track_louds[ 5 * track_louds.len() / 100].0,
        track_louds[50 * track_louds.len() / 100].0,
        track_louds[95 * track_louds.len() / 100].0,
    );

    let mut album_louds = Vec::new();
    for &(album_id, ref album) in index.get_albums() {
        if let Some(lufs) = album.loudness {
            album_louds.push((lufs, album_id));
        }
    }
    album_louds.sort();
    let album_loud_min = album_louds[0];
    let album_loud_max = album_louds[album_louds.len() - 1];
    println!(
        "\nSoftest album: {} by {} at {}.",
        index.get_string(index.get_album(album_loud_min.1).unwrap().title),
        index.get_string(index.get_artist(index.get_album(album_loud_min.1).unwrap().artist_id).unwrap().name),
        album_loud_min.0,
    );
    println!(
        "Loudest album: {} by {} at {}.",
        index.get_string(index.get_album(album_loud_max.1).unwrap().title),
        index.get_string(index.get_artist(index.get_album(album_loud_max.1).unwrap().artist_id).unwrap().name),
        album_loud_max.0,
    );
    println!(
        "Album loudness p5, p50, p95: {}, {}, {}\n",
        album_louds[ 5 * album_louds.len() / 100].0,
        album_louds[50 * album_louds.len() / 100].0,
        album_louds[95 * album_louds.len() / 100].0,
    );

    println!("Artist word index: {}", index.words_artist.size());
    println!("Album word index:  {}", index.words_album.size());
    println!("Track word index:  {}", index.words_track.size());

    Ok(index)
}

/// Return whether the two strings are equal after extracting normalized words.
fn equals_normalized(x1: &str, x2: &str) -> bool {
    // TODO: Figure out a faster way to do this.
    let mut w1 = Vec::new();
    let mut w2 = Vec::new();
    normalize_words(&x1[..], &mut w1);
    normalize_words(&x2[..], &mut w2);
    w1 == w2
}

fn match_listens(
    index: &MemoryMetaIndex,
    in_path: String,
    out_path: String,
) -> Result<()> {
    let fi = fs::File::open(in_path)?;
    let r = io::BufReader::new(fi);
    let mut lines = r.lines();

    let fo = fs::File::create(out_path)?;
    let mut w = io::BufWriter::new(fo);

    // Skip the header row for reading, print the header row for writing.
    lines.next();
    write!(w, "seconds_since_epoch\ttrack_id\n")?;

    let mut total = 0_u32;
    let mut matched = 0_u32;

    for opt_line in lines {
        let line = opt_line?;
        let mut parts = line.split('\t');
        let time_str = parts.next().expect("Expected seconds_since_epoch");
        let track_title = parts.next().expect("Expected track");
        let artist_name = parts.next().expect("Expected artist");
        let album_name = parts.next().expect("Expected album");

        let mut words = Vec::new();
        let mut tracks = Vec::new();
        normalize_words(&track_title[..], &mut words);
        normalize_words(&artist_name[..], &mut words);
        // TODO: Add a way to turn off prefix search for the last word.
        index.search_track(&words[..], &mut tracks);

        let mut found = false;

        for track_id in tracks {
            let track = index.get_track(track_id).expect("Search result should be in index.");
            let album = index.get_album(track.album_id).expect("Track album should be in index.");
            let track_ok = equals_normalized(index.get_string(track.title), track_title);
            let artist_ok = equals_normalized(index.get_string(track.artist), artist_name);
            let album_ok = equals_normalized(index.get_string(album.title), album_name);
            if track_ok && artist_ok && album_ok {
                if !found {
                    write!(w, "{}\t{}\n", time_str, track_id)?;
                    found = true;
                    matched += 1;
                } else {
                    println!(
                        "AMBIGUOUS {}: at {} listened {} by {} from {}",
                        track_id, time_str, track_title, artist_name, album_name,
                    );
                }
            }
        }

        if !found {
            println!(
                "MISSING: at {} listened {} by {} from {}",
                time_str, track_title, artist_name, album_name,
            );
        }

        total += 1;
    }

    println!(
        "Matched {} out of {} listens. ({:.1}%)",
        matched, total, (matched as f32 * 100.0) / (total as f32),
    );

    Ok(())
}

fn run_scan(config: &Config) -> Result<()> {
    // Running a scan requires an index var that the scan can update. When
    // triggered from the server this updates the servers index, but when we
    // run a standalone scan, the new value is not used. We still need to
    // provide the var though.
    let dummy_index = MemoryMetaIndex::new_empty();
    let index_var = Arc::new(MVar::new(Arc::new(dummy_index)));

    let (scan_thread, rx) = musium::scan::run_scan_in_thread(config, index_var);

    {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();

        write!(lock, "\n\n\n").unwrap();

        for status in rx {
            // Move the cursor up a line, and clear that line. We need to clear
            // it, because "convert" sometimes prints warnings. We could swallow
            // its stderr, but this allows the warning to at least be visible
            // very briefly.
            let up_clear = "\x1b[F\x1b[K";
            write!(lock, "{}{}{}{}", up_clear, up_clear, up_clear, status).unwrap();
            lock.flush().unwrap();
        }
    }

    // The unwrap unwraps the join, not the scan's result.
    scan_thread.join().unwrap()
}

fn print_usage() {
    println!("\
Usage:

  musium scan musium.conf
  musium serve musium.conf
  musium match musium.conf listenbrainz.tsv matched.tsv

SCAN

  Update the file database, generate album art thumbnails.

SERVE

  Start the server. Requires running a scan first for serving an up-to-date
  library.

MATCH

  Match listens (see process_listens.py) to tracks.");
}

fn load_config(config_fname: &str) -> Result<Config> {
    let f = fs::File::open(config_fname)?;
    let buf_reader = io::BufReader::new(f);
    let lines: io::Result<Vec<String>> = buf_reader.lines().collect();
    Config::parse(lines?.iter())
}

fn main() -> Result<()> {
    if env::args().len() < 3 {
        print_usage();
        process::exit(1);
    }

    let cmd = env::args().nth(1).unwrap();
    let config_path = env::args().nth(2).unwrap();
    let config = load_config(&config_path)?;
    println!("Configuration:\n{}\n", config);

    match &cmd[..] {
        "serve" => {
            let config_clone = config.clone();
            let index = make_index(&config.db_path())?;
            let arc_index = Arc::new(index);
            let index_var = Arc::new(MVar::new(arc_index.clone()));
            println!("Indexing complete.");

            println!("Loading cover art thumbnails ...");
            let thumb_cache = ThumbCache::new(
                arc_index.get_album_ids_ordered_by_artist(),
                &config.covers_path,
            ).expect("Failed to load cover art thumbnails.");
            println!("Thumb cache size: {}", thumb_cache.size());
            let arc_thumb_cache = Arc::new(thumb_cache);
            let thumb_cache_var = Arc::new(MVar::new(arc_thumb_cache));

            println!("Starting server on {}.", config.listen);
            let db_path = config.db_path();
            let player = musium::player::Player::new(
                index_var.clone(),
                config.audio_device,
                config.audio_volume_control,
                db_path,
                config.high_pass_cutoff,
            );
            let service = MetaServer::new(
                config_clone,
                index_var.clone(),
                thumb_cache_var,
                player,
            );
            serve(&config.listen, Arc::new(service));
        }
        "scan" => {
            run_scan(&config)?;
            Ok(())
        }
        "match" => {
            let in_path = env::args().nth(3).unwrap();
            let out_path = env::args().nth(4).unwrap();
            let index = make_index(&config.library_path)?;
            match_listens(&index, in_path, out_path)
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
