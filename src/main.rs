// Mindec -- Music metadata indexer
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

extern crate claxon;
extern crate futures;
extern crate hyper;
extern crate mindec;
extern crate serde_json;
extern crate url;
extern crate walkdir;

use std::env;
use std::time::{Duration, SystemTime};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process;
use std::rc::Rc;

use futures::future::Future;
use hyper::header::{AccessControlAllowOrigin, ContentLength, ContentType, Expires, HttpDate};
use hyper::mime;
use hyper::server::{Http, Request, Response, Service};
use hyper::{Get, StatusCode};
use mindec::{AlbumId, MetaIndex, MemoryMetaIndex, TrackId};

struct MetaServer {
    index: MemoryMetaIndex,
    cache_dir: PathBuf,
}

type BoxFuture = Box<Future<Item=Response, Error=hyper::Error>>;

impl MetaServer {
    fn new(index: MemoryMetaIndex, cache_dir: &str) -> MetaServer {
        MetaServer {
            index: index,
            cache_dir: PathBuf::from(cache_dir),
        }
    }

    fn handle_not_found(&self) -> BoxFuture {
        let not_found = "Not Found";
        let response = Response::new()
            .with_status(StatusCode::NotFound)
            .with_header(ContentLength(not_found.len() as u64))
            .with_body(not_found);
        Box::new(futures::future::ok(response))
    }

    fn handle_bad_request(&self, reason: &'static str) -> BoxFuture {
        let response = Response::new()
            .with_status(StatusCode::BadRequest)
            .with_header(ContentLength(reason.len() as u64))
            .with_body(reason);
        Box::new(futures::future::ok(response))
    }

    fn handle_error(&self, reason: &'static str) -> BoxFuture {
        let response = Response::new()
            .with_status(StatusCode::InternalServerError)
            .with_header(ContentLength(reason.len() as u64))
            .with_body(reason);
        Box::new(futures::future::ok(response))
    }

    fn handle_static_file(&self, fname: &str, mime_type: &str) -> BoxFuture {
        let mut data = Vec::new();
        let mut file = match fs::File::open(fname) {
            Ok(f) => f,
            Err(..) => return self.handle_error("Failed to read static file."),
        };
        match file.read_to_end(&mut data) {
            Ok(..) => {}
            Err(..) => return self.handle_error("Failed to read cached thumbnail."),
        }
        let mime = mime_type.parse::<mime::Mime>().unwrap();
        let response = Response::new()
            .with_header(AccessControlAllowOrigin::Any)
            .with_header(ContentType(mime))
            .with_header(ContentLength(data.len() as u64))
            .with_body(data);
        Box::new(futures::future::ok(response))
    }

    fn handle_track_cover(&self, _request: &Request, id: &str) -> BoxFuture {
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
            let mime = match cover.mime_type.parse::<mime::Mime>() {
                Ok(m) => m,
                Err(..) => {
                    // TODO: Add a proper logging mechanism.
                    println!("Warning invalid mime type: '{}' in track {} ({}).", cover.mime_type, id, fname);
                    return self.handle_error("Invalid mime type.")
                }
            };
            let data = cover.into_vec();
            let expires = SystemTime::now() + Duration::from_secs(3600 * 24 * 30);
            let response = Response::new()
                .with_header(AccessControlAllowOrigin::Any)
                .with_header(Expires(HttpDate::from(expires)))
                .with_header(ContentType(mime))
                .with_header(ContentLength(data.len() as u64))
                .with_body(data);
            Box::new(futures::future::ok(response))
        } else {
            // The file has no embedded front cover.
            self.handle_not_found()
        }
    }

    fn handle_thumb(&self, _request: &Request, id: &str) -> BoxFuture {
        // TODO: DRY this track id parsing and loadong part.
        let album_id = match AlbumId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid album id."),
        };

        let mut fname: PathBuf = PathBuf::from(&self.cache_dir);
        fname.push(format!("{}.jpg", album_id));
        let mut file = match fs::File::open(fname) {
            Ok(f) => f,
            // TODO: This is not entirely accurate. Also, try to generate the
            // thumbnail if it does not exist.
            Err(..) => return self.handle_not_found(),
        };
        let mut data = Vec::new();
        match file.read_to_end(&mut data) {
            Ok(..) => {}
            Err(..) => return self.handle_error("Failed to read cached thumbnail."),
        }
        let expires = SystemTime::now() + Duration::from_secs(3600 * 24 * 30);
        let mime = "image/jpeg".parse::<mime::Mime>().unwrap();
        let response = Response::new()
            .with_header(AccessControlAllowOrigin::Any)
            .with_header(Expires(HttpDate::from(expires)))
            .with_header(ContentType(mime))
            .with_header(ContentLength(data.len() as u64))
            .with_body(data);
        Box::new(futures::future::ok(response))
    }

    fn handle_track(&self, _request: &Request, path: &str) -> BoxFuture {
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

        // TODO: Rather than reading the file into memory in userspace, use
        // sendfile. Hyper seems to be over-engineered for my use case, just
        // writing to a TCP socket would be simpler.
        let mut file = match fs::File::open(fname) {
            Ok(f) => f,
            Err(_) => return self.handle_error("Failed to open file."),
        };
        let len_hint = file.metadata().map(|m| m.len()).unwrap_or(4096);
        let mut body = Vec::with_capacity(len_hint as usize);
        if let Err(_) = file.read_to_end(&mut body) {
            return self.handle_error("Failed to read file.")
        }

        // TODO: Handle requests with Range header.
        let audio_flac = "audio/flac".parse::<mime::Mime>().unwrap();
        let response = Response::new()
            .with_header(AccessControlAllowOrigin::Any)
            .with_header(ContentType(audio_flac))
            .with_header(ContentLength(body.len() as u64))
            .with_body(body);
        Box::new(futures::future::ok(response))
    }

    fn handle_album(&self, _request: &Request, id: &str) -> BoxFuture {
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
        let response = Response::new()
            .with_header(ContentType::json())
            .with_header(AccessControlAllowOrigin::Any)
            .with_body(w.into_inner());
        Box::new(futures::future::ok(response))
    }

    fn handle_albums(&self, _request: &Request) -> BoxFuture {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        self.index.write_albums_json(&mut w).unwrap();
        let response = Response::new()
            .with_header(ContentType::json())
            .with_header(AccessControlAllowOrigin::Any)
            .with_body(w.into_inner());
        Box::new(futures::future::ok(response))
    }

    fn handle_artist(&self, _request: &Request, _id: &str) -> BoxFuture {
        let response = Response::new().with_body("Artist");
        Box::new(futures::future::ok(response))
    }

    fn handle_search(&self, request: &Request) -> BoxFuture {
        let raw_query = match request.query() {
            Some(q) => q.as_ref(),
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
        mindec::normalize_words(query.as_ref(), &mut words);

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

        let response = Response::new()
            .with_header(AccessControlAllowOrigin::Any)
            .with_header(ContentType::json())
            .with_body(w.into_inner());
        Box::new(futures::future::ok(response))
    }
}

impl Service for MetaServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = BoxFuture;

    fn call(&self, request: Request) -> Self::Future {
        println!("Request: {:?}", request);

        let mut parts = request
            .uri()
            .path()
            .splitn(3, '/')
            .filter(|x| x.len() > 0);

        let p0 = parts.next();
        let p1 = parts.next();

        // A very basic router. See also docs/api.md for an overview.
        match (request.method(), p0, p1) {
            // API endpoints.
            (&Get, Some("cover"),  Some(t)) => self.handle_track_cover(&request, t),
            (&Get, Some("thumb"),  Some(t)) => self.handle_thumb(&request, t),
            (&Get, Some("track"),  Some(t)) => self.handle_track(&request, t),
            (&Get, Some("album"),  Some(a)) => self.handle_album(&request, a),
            (&Get, Some("albums"), None)    => self.handle_albums(&request),
            (&Get, Some("artist"), Some(a)) => self.handle_artist(&request, a),
            (&Get, Some("search"), None)    => self.handle_search(&request),
            // Web endpoints.
            (&Get, None,              None) => self.handle_static_file("app/index.html", "text/html"),
            (&Get, Some("style.css"), None) => self.handle_static_file("app/style.css", "text/css"),
            (&Get, Some("app.js"),    None) => self.handle_static_file("app/output/app.js", "text/javascript"),
            // Fallback.
            (&Get, _, _) => self.handle_not_found(),
            _ => self.handle_bad_request("Expected a GET request."),
        }
    }
}

fn make_index(dir: &str) -> MemoryMetaIndex {
    let wd = walkdir::WalkDir::new(&dir)
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

        index = mindec::MemoryMetaIndex::from_paths(paths.iter(), &mut lock);
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
    pub fn new(cache_dir: &str, album_id: AlbumId, filename: &str) -> claxon::Result<Option<GenThumb>> {
        use process::{Command, Stdio};

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
        use process::Command;
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
    cache_dir: &'a str,
    pending: Vec<GenThumb>,
    max_len: usize,
}

impl<'a> GenThumbs<'a> {
    pub fn new(cache_dir: &'a str, max_parallelism: usize) -> GenThumbs<'a> {
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


fn generate_thumbnails(index: &MemoryMetaIndex, cache_dir: &str) {
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
    println!("  mindec serve /path/to/music/library /path/to/cache");
    println!("  mindec cache /path/to/music/library /path/to/cache");
    println!("  mindec play  /path/to/music/library <soundcard name>");
}

fn main() {
    if env::args().len() < 4 {
        print_usage();
        process::exit(1);
    }

    let cmd = env::args().nth(1).unwrap();
    let dir = env::args().nth(2).unwrap();
    let cache_dir = env::args().nth(3).unwrap();

    match &cmd[..] {
        "serve" => {
            let index = make_index(&dir);
            println!("Indexing complete, starting server on port 8233.");
            let service = Rc::new(MetaServer::new(index, &cache_dir));
            let addr = ([0, 0, 0, 0], 8233).into();
            let server = Http::new().bind(&addr, move || Ok(service.clone())).unwrap();
            server.run().unwrap();
        }
        "cache" => {
            let index = make_index(&dir);
            generate_thumbnails(&index, &cache_dir);
        }
        "play" => {
            let card_name = cache_dir;
            let index = make_index(&dir);
            let arc_index = std::sync::Arc::new(index);
            let mut player = mindec::player::Player::new(arc_index, card_name);
            player.join();
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
