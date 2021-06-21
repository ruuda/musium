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
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use musium::config::Config;
use musium::error;
use musium::prim::AlbumId;
use musium::scan;
use musium::server::{MetaServer, serve};
use musium::string_utils::normalize_words;
use musium::thumb_cache::ThumbCache;
use musium::{MetaIndex, MemoryMetaIndex};

fn make_index(db_path: &Path) -> MemoryMetaIndex {
    let mut issues = Vec::new();
    let index = MemoryMetaIndex::from_database(&db_path, &mut issues).expect("Failed to build index.");
    for issue in &issues {
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

    index
}

enum GenThumb {
    Resizing {
        out_fname_png: PathBuf,
        out_fname_jpg: PathBuf,
        child: process::Child,
    },
    Compressing {
        out_fname_png: PathBuf,
        child: process::Child,
    },
}

impl GenThumb {

    /// Start an extract-and-resize operation.
    pub fn new(cache_dir: &Path, album_id: AlbumId, filename: &str) -> claxon::Result<Option<GenThumb>> {
        use crate::process::{Command, Stdio};

        let mut out_fname_jpg: PathBuf = PathBuf::from(cache_dir);
        out_fname_jpg.push(format!("{}.jpg", album_id));

        let mut out_fname_png: PathBuf = PathBuf::from(cache_dir);
        out_fname_png.push(format!("{}.png", album_id));

        // Early-out on existing files. The user would need to clear the cache
        // manually.
        if out_fname_jpg.is_file() {
            return Ok(None)
        }

        let opts = claxon::FlacReaderOptions {
            metadata_only: true,
            read_picture: claxon::ReadPicture::CoverAsVec,
            read_vorbis_comment: false,
        };
        let reader = claxon::FlacReader::open_ext(filename, opts)?;

        let cover = match reader.into_pictures().pop() {
            Some(c) => c,
            None => return Ok(None),
        };

        // TODO: Add a nicer way to report progress.
        println!("{:?} <- {}", &out_fname_jpg, filename);

        let mut convert = Command::new("convert")
            // Read from stdin.
            .arg("-")
            // Some cover arts have an alpha channel, but we are going to encode
            // to jpeg which does not support it. First blend the image with a
            // black background, then drop the alpha channel. We also need a
            // -flatten to ensure that the subsequent distort operation uses the
            // "Edge" virtual pixel mode, rather than sampling the black
            // background. If it samples the black background, the edges of the
            // thumbnail become darker, which is especially noticeable for
            // covers with white edges, and also shows up as a "pop" in the
            // album view when the full-resolution image loads.
            .args(&[
                "-background", "black",
                "-alpha", "remove",
                "-alpha", "off",
                "-flatten"
            ])
            // Resize in a linear color space, sRGB is not suitable for it
            // because it is nonlinear. "RGB" in ImageMagick is linear.
            .args(&["-colorspace", "RGB"])
            // See also the note about -flatten above. I think Edge is the
            // default, but let's be explicit about it.
            .args(&["-virtual-pixel", "Edge"])
            // Lanczos2 is a bit less sharp than Cosine, but less sharp edges
            // means that the image compresses better, and less artifacts. But
            // still, Lanczos was too blurry in my opinion.
            .args(&["-filter", "Cosine"])
            // Twice the size of the thumb in the webinterface, so they appear
            // pixel-perfect on a high-DPI display, or on a mobile phone.
            .args(&["-distort", "Resize", "140x140!"])
            .args(&["-colorspace", "sRGB"])
            // Remove EXIF metadata, including the colour profile if there was
            // any -- we convert to sRGB anyway.
            .args(&["-strip"])
            // Write lossless, we will later compress to jpeg with Guetzli,
            // which has a better compressor.
            .arg(&out_fname_png)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .spawn()
            // TODO: Handle errors properly.
            .expect("Failed to spawn Imagemagick's 'convert'.");

        {
            let stdin = convert.stdin.as_mut().expect("Failed to open stdin.");
            stdin.write_all(cover.data()).unwrap();
        }

        let result = GenThumb::Resizing {
            out_fname_png: out_fname_png,
            out_fname_jpg: out_fname_jpg,
            child: convert
        };
        Ok(Some(result))
    }

    /// Wait for one step of the process, start and return the next one if any.
    pub fn poll(self) -> Option<GenThumb> {
        use crate::process::Command;
        match self {
            GenThumb::Resizing { out_fname_png, out_fname_jpg, mut child } => {
                // TODO: Use a custom error type, remove all `expect()`s.
                child.wait().expect("Failed to run Imagemagick's 'convert'.");
                let guetzli = Command::new("guetzli")
                    .args(&["--quality", "97"])
                    .arg(&out_fname_png)
                    .arg(&out_fname_jpg)
                    // TODO: Handle errors properly.
                    .spawn().expect("Failed to spawn Guetzli.");
                let result = GenThumb::Compressing {
                    out_fname_png: out_fname_png,
                    child: guetzli,
                };
                Some(result)
            }
            GenThumb::Compressing { out_fname_png, mut child } => {
                // TODO: Handle errors properly.
                child.wait().expect("Failed to run Guetzli.");

                // Delete the intermediate png file.
                fs::remove_file(&out_fname_png).expect("Failed to delete intermediate file.");

                None
            }
        }
    }

    fn is_done(&mut self) -> bool {
        let child = match self {
            GenThumb::Resizing { ref mut child, .. } => child,
            GenThumb::Compressing { ref mut child, .. } => child,
        };
        match child.try_wait() {
            Ok(Some(_)) => true,
            _ => false,
        }
    }
}

struct GenThumbs<'a> {
    cache_dir: &'a Path,
    pending: Vec<GenThumb>,
    max_len: usize,
}

impl<'a> GenThumbs<'a> {
    pub fn new(cache_dir: &'a Path, max_parallelism: usize) -> GenThumbs<'a> {
        GenThumbs {
            cache_dir: cache_dir,
            pending: Vec::new(),
            max_len: max_parallelism,
        }
    }

    fn wait_until_at_most_in_use(&mut self, max_used: usize) {
        while self.pending.len() > max_used {
            let mut found_one = false;

            // Round 1: Try to find a process that is already finished, and make
            // progress on that.
            for i in 0..self.pending.len() {
                if self.pending[i].is_done() {
                    let gen = self.pending.remove(i);
                    if let Some(next_gen) = gen.poll() {
                        self.pending.push(next_gen);
                    }
                    found_one = true;
                    break
                }
            }

            if found_one {
                continue
            }

            // Round 2: All processes are still running, wait for the oldest one.
            let gen = self.pending.remove(0);
            if let Some(next_gen) = gen.poll() {
                self.pending.push(next_gen);
            }
        }
    }

    pub fn add(&mut self, album_id: AlbumId, filename: &str) -> claxon::Result<()> {
        let max_used = self.max_len - 1;
        self.wait_until_at_most_in_use(max_used);
        if let Some(gen) = GenThumb::new(self.cache_dir, album_id, filename)? {
            self.pending.push(gen);
        }
        Ok(())
    }

    pub fn drain(&mut self) {
        self.wait_until_at_most_in_use(0);
    }
}


fn generate_thumbnails(index: &MemoryMetaIndex, cache_dir: &Path) {
    let max_parallelism = 32;
    let mut gen_thumbs = GenThumbs::new(cache_dir, max_parallelism);
    let mut prev_album_id = AlbumId(0);
    for &(_tid, ref track) in index.get_tracks() {
        if track.album_id != prev_album_id {
            let fname = index.get_filename(track.filename);
            gen_thumbs.add(track.album_id, fname).expect("Failed to start thumbnail generation.");
            prev_album_id = track.album_id;
        }
    }
    gen_thumbs.drain();
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
) -> io::Result<()> {
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

fn run_scan(config: Config) {
    // Status updates should print much faster than they are produced, so use
    // a small buffer for them.
    let (mut tx, rx) = std::sync::mpsc::sync_channel(15);

    let scan_thread = std::thread::spawn(move || {
        scan::scan(
            &config.db_path(),
            &config.library_path,
            &mut tx,
        );
    });

    {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let mut prev_status = scan::Status::new();

        for status in rx {
            match (prev_status.stage, status.stage) {
                (scan::ScanStage::Discovering, scan::ScanStage::PreProcessing) => writeln!(lock).unwrap(),
                (_, scan::ScanStage::Done) => writeln!(lock).unwrap(),
                _ => {}
            }
            match status.stage {
                scan::ScanStage::Done => break,
                scan::ScanStage::Discovering => {
                    write!(
                        lock,
                        "\rScanning: {} files discovered",
                        status.files_discovered,
                    ).unwrap();
                }
                scan::ScanStage::PreProcessing | scan::ScanStage::Processing => {
                    write!(
                        lock,
                        "\rProcessing: {} of {}",
                        status.files_processed,
                        status.files_to_process,
                    ).unwrap();
                }
            }
            lock.flush().unwrap();
            prev_status = status;
        }
    }

    scan_thread.join().unwrap();
}

fn print_usage() {
    println!("Usage:\n");
    println!("  musium serve musium.conf");
    println!("  musium cache musium.conf");
    println!("  musium match musium.conf listenbrainz.tsv matched.tsv");
    println!("
serve -- Start the server.
cache -- Generate album art thumbnails.
match -- Match listens (see process_listens.py) to tracks.");
}

fn load_config(config_fname: &str) -> error::Result<Config> {
    let f = fs::File::open(config_fname)?;
    let buf_reader = io::BufReader::new(f);
    let lines: io::Result<Vec<String>> = buf_reader.lines().collect();
    Config::parse(lines?.iter())
}

fn main() {
    if env::args().len() < 3 {
        print_usage();
        process::exit(1);
    }

    let cmd = env::args().nth(1).unwrap();
    let config_path = env::args().nth(2).unwrap();
    let config = load_config(&config_path).unwrap();
    println!("Configuration:\n{}\n", config);

    match &cmd[..] {
        "serve" => {
            let index = make_index(&config.db_path());
            let arc_index = std::sync::Arc::new(index);
            println!("Indexing complete.");
            println!("Loading cover art thumbnails ...");

            let thumb_cache = ThumbCache::new(
                arc_index.get_album_ids_ordered_by_artist(),
                &config.covers_path,
            ).expect("Failed to load cover art thumbnails.");
            println!("Thumb cache size: {}", thumb_cache.size());

            println!("Starting server on {}.", config.listen);
            let db_path = config.db_path();
            let player = musium::player::Player::new(
                arc_index.clone(),
                config.audio_device,
                config.audio_volume_control,
                db_path,
            );
            let service = MetaServer::new(arc_index.clone(), thumb_cache, player);
            serve(&config.listen, Arc::new(service));
        }
        "cache" => {
            let index = make_index(&config.db_path());
            generate_thumbnails(&index, &config.covers_path);
        }
        "scan" => {
            run_scan(config);
        }
        "match" => {
            let in_path = env::args().nth(3).unwrap();
            let out_path = env::args().nth(4).unwrap();
            let index = make_index(&config.library_path);
            match_listens(&index, in_path, out_path).unwrap();
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
