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
use tiny_http::Method::{Get, Post, Put};

use musium::config::Config;
use musium::error;
use musium::player::{Millibel, Player};
use musium::prim::{AlbumId, TrackId};
use musium::serialization;
use musium::string_utils::normalize_words;
use musium::thumb_cache::ThumbCache;
use musium::{MetaIndex, MemoryMetaIndex};

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
    thumb_cache: ThumbCache,
    player: Player,
}

impl MetaServer {
    fn new(
        index: Arc<MemoryMetaIndex>,
        thumb_cache: ThumbCache,
        player: Player,
    ) -> MetaServer {
        MetaServer {
            index: index,
            thumb_cache: thumb_cache,
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

        let img = match self.thumb_cache.get(album_id) {
            // TODO: Generate thumbs lazily?
            None => return self.handle_not_found(),
            Some(bytes) => bytes,
        };

        Response::from_data(img)
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
        serialization::write_album_json(&*self.index, &mut w, album_id, album).unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_albums(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        serialization::write_albums_json(&*self.index, &mut w).unwrap();

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
        serialization::write_queue_json(
            &*self.index,
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

    fn handle_get_volume(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let volume = self.player.get_volume();
        serialization::write_volume_json(&mut w, volume).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_change_volume(&self, add: Millibel) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let volume = self.player.change_volume(add);
        serialization::write_volume_json(&mut w, volume).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_search(&self, raw_query: &str) -> ResponseBox {
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
        normalize_words(query.as_ref(), &mut words);

        let mut artists = Vec::new();
        let mut albums = Vec::new();
        let mut tracks = Vec::new();

        self.index.search_artist(&words[..], &mut artists);
        self.index.search_album(&words[..], &mut albums);
        self.index.search_track(&words[..], &mut tracks);

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
        serialization::write_search_results_json(
            &*self.index,
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
        // Break url into the part before the ? and the part after. The part
        // before we split on slashes.
        let mut url_iter = request.url().splitn(2, '?');

        let mut p0 = None;
        let mut p1 = None;

        if let Some(base) = url_iter.next() {
            let mut parts = base.splitn(3, '/').filter(|x| x.len() > 0);

            p0 = parts.next();
            p1 = parts.next();
        }

        let query = url_iter.next().unwrap_or("");

        // A very basic router. See also docs/api.md for an overview.
        let response = match (request.method(), p0, p1) {
            // API endpoints.
            (&Get, Some("cover"),  Some(t)) => self.handle_album_cover(t),
            (&Get, Some("thumb"),  Some(t)) => self.handle_thumb(t),
            (&Get, Some("track"),  Some(t)) => self.handle_track(t),
            (&Get, Some("album"),  Some(a)) => self.handle_album(a),
            (&Get, Some("albums"), None)    => self.handle_albums(),
            (&Get, Some("search"), None)    => self.handle_search(query),
            (&Get, Some("queue"),  None)    => self.handle_queue(),
            (&Put, Some("queue"),  Some(t)) => self.handle_enqueue(t),

            // Volume control, volume up/down change the volume by 1 dB.
            (&Get,  Some("volume"), None)         => self.handle_get_volume(),
            (&Post, Some("volume"), Some("up"))   => self.handle_change_volume(Millibel(100)),
            (&Post, Some("volume"), Some("down")) => self.handle_change_volume(Millibel(-100)),

            // Web endpoints.
            (&Get, None,                    None) => self.handle_static_file("app/index.html", "text/html"),
            (&Get, Some("style.css"),       None) => self.handle_static_file("app/style.css", "text/css"),
            (&Get, Some("dark.css"),        None) => self.handle_static_file("app/dark.css", "text/css"),
            (&Get, Some("icon.svg"),        None) => self.handle_static_file("app/icon.svg", "image/svg+xml"),
            (&Get, Some("icon-lowres.svg"), None) => self.handle_static_file("app/icon-lowres.svg", "image/svg+xml"),
            (&Get, Some("manifest.json"),   None) => self.handle_static_file("app/manifest.json", "text/javascript"),
            (&Get, Some("app.js"),          None) => self.handle_static_file("app/output/app.js", "text/javascript"),
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
            let index = make_index(&config.library_path);
            let arc_index = std::sync::Arc::new(index);
            println!("Indexing complete.");
            println!("Loading cover art thumbnails ...");

            let thumb_cache = ThumbCache::new(
                arc_index.get_album_ids_ordered_by_artist(),
                &config.covers_path,
            ).expect("Failed to load cover art thumbnails.");
            println!("Thumb cache size: {}", thumb_cache.size());

            println!("Starting server on {}.", config.listen);

            let mut db_path = config.data_path.clone();
            db_path.push("musium.sqlite3");
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
            let index = make_index(&config.library_path);
            generate_thumbnails(&index, &config.covers_path);
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
