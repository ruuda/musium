// Musium -- Music playback daemon with web-based library browser
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

extern crate claxon;
extern crate musium;
extern crate serde_json;
extern crate tiny_http;
extern crate url;
extern crate walkdir;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, Write};
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Request, Response, ResponseBox, Server};
use tiny_http::Method::{Get, Put};

use musium::config::Config;
use musium::error;
use musium::player::Player;
use musium::{AlbumId, MetaIndex, MemoryMetaIndex, TrackId};

fn header_content_type(content_type: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("Failed to create content-type header, value is not ascii.")
}

fn header_expires_seconds(age_seconds: i64) -> Header {
    let now = chrono::Utc::now();
    let at = now.checked_add_signed(chrono::Duration::seconds(age_seconds)).unwrap();
    // The format from https://tools.ietf.org/html/rfc7234#section-5.3.
    let value = at.format("%a, %e %b %Y %H:%M:%S GMT").to_string();
    Header::from_bytes(&b"Expires"[..], value)
        .expect("Failed to create content-type header, value is not ascii.")
}

struct MetaServer {
    index: Arc<MemoryMetaIndex>,
    cache_dir: PathBuf,
    player: Player<MemoryMetaIndex>,
}

impl MetaServer {
    fn new(
        index: Arc<MemoryMetaIndex>,
        cache_dir: PathBuf,
        player: Player<MemoryMetaIndex>,
    ) -> MetaServer {
        MetaServer {
            index: index,
            cache_dir: cache_dir,
            player: player,
        }
    }

    fn handle_not_found(&self) -> ResponseBox {
        Response::from_string("Not Found")
            .with_status_code(404) // "404 Not Found"
            .boxed()
    }

    fn handle_bad_request(&self, reason: &'static str) -> ResponseBox {
        Response::from_string(reason)
            .with_status_code(400) // "400 Bad Request"
            .boxed()
    }

    fn handle_error(&self, reason: &'static str) -> ResponseBox {
        Response::from_string(reason)
            .with_status_code(500) // "500 Internal Server Error"
            .boxed()
    }

    fn handle_static_file(&self, fname: &str, mime_type: &str) -> ResponseBox {
        let file = match fs::File::open(fname) {
            Ok(f) => f,
            Err(..) => return self.handle_error("Failed to read static file."),
        };
        Response::from_file(file)
            .with_header(header_content_type(mime_type))
            .boxed()
    }

    fn handle_album_cover(&self, id: &str) -> ResponseBox {
        let album_id = match AlbumId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid album id."),
        };

        let tracks = self.index.get_album_tracks(album_id);
        let (_track_id, track) = tracks.first().expect("Albums have at least one track.");
        let fname = self.index.get_filename(track.filename);

        let opts = claxon::FlacReaderOptions {
            metadata_only: true,
            read_picture: claxon::ReadPicture::CoverAsVec,
            read_vorbis_comment: false,
        };
        let reader = match claxon::FlacReader::open_ext(fname, opts) {
            Ok(r) => r,
            Err(..) => return self.handle_error("Failed to open flac file."),
        };

        if let Some(cover) = reader.into_pictures().pop() {
            let content_type = header_content_type(&cover.mime_type);
            let data = cover.into_vec();
            Response::from_data(data)
                .with_header(content_type)
                .with_header(header_expires_seconds(3600 * 24 * 30))
                .boxed()
        } else {
            // The file has no embedded front cover.
            self.handle_not_found()
        }
    }

    fn handle_thumb(&self, id: &str) -> ResponseBox {
        // TODO: DRY this track id parsing and loadong part.
        let album_id = match AlbumId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid album id."),
        };

        let mut fname: PathBuf = PathBuf::from(&self.cache_dir);
        fname.push(format!("{}.jpg", album_id));
        let file = match fs::File::open(fname) {
            Ok(f) => f,
            // TODO: This is not entirely accurate. Also, try to generate the
            // thumbnail if it does not exist.
            Err(..) => return self.handle_not_found(),
        };
        Response::from_file(file)
            .with_header(header_content_type("image/jpeg"))
            .with_header(header_expires_seconds(3600 * 24 * 30))
            .boxed()
    }

    fn handle_track(&self, path: &str) -> ResponseBox {
        // Track urls are of the form `/track/f7c153f2b16dc101.flac`.
        if !path.ends_with(".flac") {
            return self.handle_bad_request("Expected a path ending in .flac.")
        }

        let id_part = &path[..path.len() - ".flac".len()];
        let track_id = match TrackId::parse(id_part) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let track = match self.index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

        let fname = self.index.get_filename(track.filename);

        // TODO: Rather than reading the file into memory in userspace,
        // use sendfile.
        // TODO: Handle requests with Range header.
        let file = match fs::File::open(fname) {
            Ok(f) => f,
            Err(_) => return self.handle_error("Failed to open file."),
        };

        Response::from_file(file)
            .with_header(header_content_type("audio/flac"))
            .boxed()
    }

    fn handle_album(&self, id: &str) -> ResponseBox {
        let album_id = match AlbumId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid album id."),
        };

        let album = match self.index.get_album(album_id) {
            Some(a) => a,
            None => return self.handle_not_found(),
        };

        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        self.index.write_album_json(&mut w, album_id, album).unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_albums(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        self.index.write_albums_json(&mut w).unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_queue(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let queue = self.player.get_queue();
        let position_seconds = queue.position_ms as f32 * 1e-3;
        let buffered_seconds = queue.buffered_ms as f32 * 1e-3;
        self.index.write_queue_json(
            &mut w,
            &queue.tracks[..],
            position_seconds,
            buffered_seconds,
        ).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_enqueue(&self, id: &str) -> ResponseBox {
        let track_id = match TrackId::parse(id) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        // Confirm that the track exists before we enqueue it.
        let _track = match self.index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

        let queue_id = self.player.enqueue(track_id);
        let queue_id_json = format!(r#""{}""#, queue_id);

        Response::from_string(queue_id_json)
            .with_status_code(201) // "201 Created"
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_search(&self, request: &Request) -> ResponseBox {
        let raw_query = match request.url().strip_prefix("/search?") {
            Some(q) => q,
            None => "",
        };
        let mut opt_query = None;
        for (k, v) in url::form_urlencoded::parse(raw_query.as_bytes()) {
            if k == "q" {
                opt_query = Some(v);
            }
        };
        let query = match opt_query {
            Some(q) => q,
            None => return self.handle_bad_request("Missing search query."),
        };

        let mut words = Vec::new();
        musium::normalize_words(query.as_ref(), &mut words);

        let mut artists = Vec::new();
        let mut albums = Vec::new();
        let mut tracks = Vec::new();

        for word in words {
            // TODO: Take intersection of word results, instead of append.
            self.index.search_artist(&word, &mut artists);
            self.index.search_album(&word, &mut albums);
            self.index.search_track(&word, &mut tracks);
        }

        // Cap the number of search results we serve. We can easily produce many
        // many results (especially when searching for "t", a prefix of "the",
        // or when searching "a"). Searching is quite fast, but parsing and
        // rendering the results in the frontend is slow, and having this many
        // results is not useful anyway, so we cap them.
        let n_artists = artists.len().min(250);
        let n_albums = albums.len().min(250);
        let n_tracks = tracks.len().min(250);

        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        self.index.write_search_results_json(
            &mut w,
            &artists[..n_artists],
            &albums[..n_albums],
            &tracks[..n_tracks],
        ).unwrap();

        Response::from_data(w.into_inner())
            .with_status_code(200)
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_request(&self, request: Request) {
        println!("Request: {:?}", request);

        let mut parts = request
            .url()
            .splitn(3, '/')
            .filter(|x| x.len() > 0);

        let p0 = parts.next();
        let p1 = parts.next();

        // A very basic router. See also docs/api.md for an overview.
        let response = match (request.method(), p0, p1) {
            // API endpoints.
            (&Get, Some("cover"),  Some(t)) => self.handle_album_cover(t),
            (&Get, Some("thumb"),  Some(t)) => self.handle_thumb(t),
            (&Get, Some("track"),  Some(t)) => self.handle_track(t),
            (&Get, Some("album"),  Some(a)) => self.handle_album(a),
            (&Get, Some("albums"), None)    => self.handle_albums(),
            (&Get, Some("search"), None)    => self.handle_search(&request),
            (&Get, Some("queue"),  None)    => self.handle_queue(),
            (&Put, Some("queue"),  Some(t)) => self.handle_enqueue(t),
            // Web endpoints.
            (&Get, None,                  None) => self.handle_static_file("app/index.html", "text/html"),
            (&Get, Some("style.css"),     None) => self.handle_static_file("app/style.css", "text/css"),
            (&Get, Some("manifest.json"), None) => self.handle_static_file("app/manifest.json", "text/javascript"),
            (&Get, Some("app.js"),        None) => self.handle_static_file("app/output/app.js", "text/javascript"),
            // Fallback.
            (&Get, _, _) => self.handle_not_found(),
            _ => self.handle_bad_request("Expected a GET request."),
        };

        match request.respond(response) {
            Ok(()) => {},
            Err(err) => println!("Error while responding to request: {:?}", err),
        }
    }
}

fn serve(bind: &str, service: Arc<MetaServer>) {
    let server = Server::http(bind).expect("TODO: Failed to start server.");
    let server = Arc::new(server);

    // Browsers do not make more than 8 requests in parallel, so having more
    // handler threads is not useful; I expect only a single user to be
    // browsing at a time.
    let n_threads = 8;
    let mut threads = Vec::with_capacity(n_threads);

    for i in 0..n_threads {
        let server_i = server.clone();
        let service_i = service.clone();
        let name = format!("http_server_{}", i);
        let builder = thread::Builder::new().name(name);
        let join_handle = builder.spawn(move || {
            loop {
                let request = match server_i.recv() {
                    Ok(rq) => rq,
                    Err(e) => {
                        println!("Error: {:?}", e);
                        break;
                    }
                };
                service_i.handle_request(request);
            }
        }).unwrap();
        threads.push(join_handle);
    }

    // Block until all threads have stopped, which only happens in case of an
    // error on all of them.
    for thread in threads.drain(..) {
        thread.join().unwrap();
    }
}

fn make_index(dir: &Path) -> MemoryMetaIndex {
    let wd = walkdir::WalkDir::new(dir)
        .follow_links(true)
        .max_open(128);

    let flac_ext = OsStr::new("flac");

    let index;
    {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();

        // First enumerate all flac files, before indexing them. It turns out
        // that this is faster than indexing them on the go (and not first
        // collecting into a vector). See also performance.md in the root of the
        // repository.
        let mut k = 0;
        let mut paths = Vec::new();
        let paths_iter = wd
            .into_iter()
            .filter_map(|e| match e {
                Ok(entry) => Some(entry),
                // TODO: Add a nicer way to report errors.
                Err(err) => { eprintln!("{}", err); None }
            })
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| p.extension() == Some(flac_ext));

        for p in paths_iter {
            // Print progress updates on the number of files discovered.
            // Enumerating the filesystem can take a long time when the OS
            // caches are cold. When the caches are warm it is pretty much
            // instant, but indexing tends to happen with cold caches.
            k += 1;
            if k % 64 == 0 {
                write!(&mut lock, "\r{} files discovered", k).unwrap();
                lock.flush().unwrap();
            }
            paths.push(p);
        }
        writeln!(&mut lock, "\r{} files discovered", k).unwrap();

        index = musium::MemoryMetaIndex::from_paths(&paths[..], &mut lock);
    };

    let index = index.expect("Failed to build index.");
    println!(
        "Index has {} artists, {} albums, and {} tracks.",
        index.get_artists().len(),
        index.get_albums().len(),
        index.len()
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

fn print_usage() {
    println!("usage: ");
    println!("  musium serve musium.conf");
    println!("  musium cache musium.conf");
}

fn load_config(config_fname: &str) -> error::Result<Config> {
    let f = fs::File::open(config_fname)?;
    let buf_reader = io::BufReader::new(f);
    let lines: io::Result<Vec<String>> = buf_reader.lines().collect();
    Config::parse(lines?.iter())
}

fn main() {
    if env::args().len() != 3 {
        print_usage();
        process::exit(1);
    }

    let cmd = env::args().nth(1).unwrap();
    let config_path = env::args().nth(2).unwrap();
    let config = load_config(&config_path).unwrap();
    println!("Configuration:\n{}\n", config);

    match &cmd[..] {
        "serve" => {
            let index = make_index(&config.library_path);
            let arc_index = std::sync::Arc::new(index);
            println!("Indexing complete, starting server on {}.", config.listen);

            let player = musium::player::Player::new(arc_index.clone(), config.audio_device);
            let service = MetaServer::new(arc_index.clone(), config.covers_path, player);
            serve(&config.listen, Arc::new(service));
        }
        "cache" => {
            let index = make_index(&config.library_path);
            generate_thumbnails(&index, &config.covers_path);
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
