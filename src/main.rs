// Musium -- Music playback daemon with web-based library browser
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// Disable some of Clippy's opinions that I disagree with.
#![allow(clippy::len_zero)]

extern crate claxon;
extern crate crossbeam;
extern crate musium;
extern crate serde_json;
extern crate tiny_http;
extern crate url;
extern crate walkdir;

use std::env;
use std::fs;
use std::io;
use std::io::{BufRead, Write};
use std::process;
use std::sync::{Arc, Mutex};

use musium::config::Config;
use musium::database;
use musium::database_utils;
use musium::error::Result;
use musium::mvar::MVar;
use musium::server::{serve, MetaServer};
use musium::string_utils::normalize_words;
use musium::thumb_cache::ThumbCache;
use musium::user_data::UserData;
use musium::{MemoryMetaIndex, MetaIndex};

fn make_index(tx: &mut database::Transaction) -> Result<MemoryMetaIndex> {
    let (index, builder) = MemoryMetaIndex::from_database(tx)?;

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
    for kv in index.get_tracks() {
        if let Some(lufs) = kv.track.loudness {
            track_louds.push((lufs, kv.track_id));
        }
    }
    track_louds.sort();
    if track_louds.len() > 0 {
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
            track_louds[5 * track_louds.len() / 100].0,
            track_louds[50 * track_louds.len() / 100].0,
            track_louds[95 * track_louds.len() / 100].0,
        );
    }

    let mut album_louds = Vec::new();
    for kv in index.get_albums() {
        if let Some(lufs) = kv.album.loudness {
            album_louds.push((lufs, kv.album_id));
        }
    }
    album_louds.sort();
    if album_louds.len() > 0 {
        let album_loud_min = album_louds[0];
        let album_loud_max = album_louds[album_louds.len() - 1];
        println!(
            "\nSoftest album: {} by {} at {}.",
            index.get_string(index.get_album(album_loud_min.1).unwrap().title),
            index.get_string(index.get_album(album_loud_min.1).unwrap().artist),
            album_loud_min.0,
        );
        println!(
            "Loudest album: {} by {} at {}.",
            index.get_string(index.get_album(album_loud_max.1).unwrap().title),
            index.get_string(index.get_album(album_loud_max.1).unwrap().artist),
            album_loud_max.0,
        );
        println!(
            "Album loudness p5, p50, p95: {}, {}, {}\n",
            album_louds[5 * album_louds.len() / 100].0,
            album_louds[50 * album_louds.len() / 100].0,
            album_louds[95 * album_louds.len() / 100].0,
        );
    }

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
    normalize_words(x1, &mut w1);
    normalize_words(x2, &mut w2);
    w1 == w2
}

fn match_listens(index: &MemoryMetaIndex, tx: &mut database::Transaction) -> Result<()> {
    let mut total = 0_u32;
    let mut matched = 0_u32;
    let mut missed = 0_u32;
    let mut ambiguous = 0_u32;

    for listen_opt in database::iter_lastfm_missing_listens(tx)? {
        let listen = listen_opt?;

        let mut words = Vec::new();
        let mut tracks = Vec::new();
        normalize_words(&listen.title, &mut words);
        normalize_words(&listen.track_artist, &mut words);
        // TODO: Add a way to turn off prefix search for the last word.
        index.search_track(&words[..], &mut tracks);

        let mut found = false;

        for track_id in tracks {
            let track = index
                .get_track(track_id)
                .expect("Search result should be in index.");
            let album = index
                .get_album(track_id.album_id())
                .expect("Track album should be in index.");
            let track_ok = equals_normalized(index.get_string(track.title), &listen.title);
            let artist_ok = equals_normalized(index.get_string(track.artist), &listen.track_artist);
            let album_ok = equals_normalized(index.get_string(album.title), &listen.album);
            if track_ok && artist_ok && album_ok {
                if !found {
                    found = true;
                    matched += 1;
                } else {
                    println!("AMBIGUOUS {listen:?}");
                    ambiguous += 1;
                }
            }
        }

        if !found {
            println!("MISSING: {listen:?}");
            missed += 1;
        }

        total += 1;
    }

    println!(
        "Matched {} out of {} listens ({:.1}%), missed {} ({:.1}%), ambiguous {} ({:.1}%).",
        matched,
        total,
        (matched as f32 * 100.0) / (total as f32),
        missed,
        (missed as f32 * 100.0) / (total as f32),
        ambiguous,
        (ambiguous as f32 * 100.0) / (total as f32),
    );

    Ok(())
}

fn run_scan(config: &Config) -> Result<()> {
    // Running a scan requires an index var that the scan can update. When
    // triggered from the server this updates the servers index, but when we
    // run a standalone scan, the new value is not used. We still need to
    // provide the var though.
    let dummy_index = MemoryMetaIndex::new_empty();
    let dummy_thumb_cache = ThumbCache::new_empty();
    let index_var = Arc::new(MVar::new(Arc::new(dummy_index)));
    let thumb_cache_var = Arc::new(MVar::new(Arc::new(dummy_thumb_cache)));

    let (scan_thread, rx) = musium::scan::run_scan_in_thread(config, index_var, thumb_cache_var);

    {
        let stdout = io::stdout();
        let mut lock = stdout.lock();

        write!(lock, "\n\n\n\n\n").unwrap();

        for status in rx {
            // Move the cursor up a line, and clear that line. We need to clear
            // it, because "convert" sometimes prints warnings. We could swallow
            // its stderr, but this allows the warning to at least be visible
            // very briefly.
            let up_clear = "\x1b[F\x1b[K";
            write!(lock, "{0}{0}{0}{0}{0}{0}{0}{1}", up_clear, status).unwrap();
            lock.flush().unwrap();
        }
    }

    // The unwrap unwraps the join, not the scan's result.
    scan_thread.join().unwrap()
}

const USAGE: &'static str = "\
Usage:

  musium scan musium.conf
  musium serve musium.conf
  musium match musium.conf
  musium count musium.conf

SCAN

  Update the file database, generate album art thumbnails.

SERVE

  Start the server. Requires running a scan first for serving an up-to-date
  library.

MATCH

  Match listens (see process_listens.py) to tracks.

COUNT

  Print listen count statistics.";

fn print_usage() {
    println!("{}", USAGE);
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

            let conn = database_utils::connect_readonly(&config.db_path)?;
            let mut db = database::Connection::new(&conn);
            let mut tx = db.begin()?;

            println!("Loading index ...");
            let index = make_index(&mut tx)?;
            println!("Index loaded.");

            println!("Loading user data and playcounts ...");
            let (user_data, counts) = UserData::load_from_database(&index, &mut tx)?;
            let user_data_arc = Arc::new(Mutex::new(user_data));
            println!("User data loaded.");

            let arc_index = Arc::new(index);
            let index_var = Arc::new(MVar::new(arc_index));

            println!("Loading cover art thumbnails ...");
            let thumb_cache = ThumbCache::load_from_database(&mut tx)?;
            println!("Thumb cache size: {}", thumb_cache.size());
            let arc_thumb_cache = Arc::new(thumb_cache);
            let thumb_cache_var = Arc::new(MVar::new(arc_thumb_cache));

            tx.commit()?;
            std::mem::drop(db);
            std::mem::drop(conn);

            println!("Starting server on {}.", config.listen);
            let player = musium::player::Player::new(
                index_var.clone(),
                user_data_arc.clone(),
                counts.into_counter(),
                &config,
            );
            let service = MetaServer::new(
                config_clone,
                index_var,
                thumb_cache_var,
                user_data_arc,
                player,
            );
            serve(&config.listen, Arc::new(service));
        }
        "scan" => {
            run_scan(&config)?;
            Ok(())
        }
        "count" => {
            let conn = database_utils::connect_readonly(&config.db_path)?;
            let mut db = database::Connection::new(&conn);
            let mut tx = db.begin()?;
            let index = make_index(&mut tx)?;
            musium::playcount::main(&index, &config.db_path)
        }
        "match" => {
            let conn = database_utils::connect_read_write(&config.db_path)?;
            let mut db = database::Connection::new(&conn);
            let mut tx = db.begin()?;
            let index = make_index(&mut tx)?;
            tx.commit()?;
            match_listens(&index, &mut db.begin()?)
        }
        "match2" => {
            let conn = database_utils::connect_read_write(&config.db_path)?;
            let mut db = database::Connection::new(&conn);
            let mut tx = db.begin()?;
            let index = make_index(&mut tx)?;
            tx.commit()?;
            musium::matcher::match_listens(&index, &mut db.begin()?)
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
