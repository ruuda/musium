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
extern crate walkdir;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;

use futures::future::Future;
use hyper::header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use hyper::service::{Service};
use hyper::{Body, Method, Response, Server, StatusCode};
use mindec::{AlbumId, MetaIndex, MemoryMetaIndex, TrackId};

struct MetaServer {
    index: Arc<MemoryMetaIndex>,
    cache_dir: PathBuf,
}

type Request = hyper::Request<Body>;
type BoxFuture = Box<Future<Item = Response<Body>, Error = io::Error> + Send>;

impl MetaServer {
    fn new(index: Arc<MemoryMetaIndex>, cache_dir: &str) -> MetaServer {
        MetaServer {
            index: index,
            cache_dir: PathBuf::from(cache_dir),
        }
    }

    fn handle_not_found(&self) -> BoxFuture {
        let not_found = "Not Found";
        let response = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(CONTENT_LENGTH, not_found.len())
            .body(Body::from(not_found))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_bad_request(&self, reason: &'static str) -> BoxFuture {
        let response = Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(CONTENT_LENGTH, reason.len())
            .body(Body::from(reason))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_error(&self, reason: &'static str) -> BoxFuture {
        let response = Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(CONTENT_LENGTH, reason.len())
            .body(Body::from(reason))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_static_file(&self, fname: &str, mime_type: &'static str) -> BoxFuture {
        let mut data = Vec::new();
        let mut file = match fs::File::open(fname) {
            Ok(f) => f,
            Err(..) => return self.handle_error("Failed to read static file."),
        };
        match file.read_to_end(&mut data) {
            Ok(..) => {}
            Err(..) => return self.handle_error("Failed to read cached thumbnail."),
        }
        let response = Response::builder()
            .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
            .header(CONTENT_TYPE, mime_type)
            .header(CONTENT_LENGTH, data.len())
            .body(Body::from(data))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_track_cover(&self, _request: &Request, id: &str) -> BoxFuture {
        // TODO: DRY this track id parsing and loadong part.
        let track_id = match TrackId::parse(id) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let track = match self.index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

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
            let max_age = "max-age=2592000"; // 30 days.
            let response = Response::builder()
                .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
                .header(CACHE_CONTROL, max_age)
                .header(CONTENT_TYPE, &cover.mime_type)
                .header(CONTENT_LENGTH, cover.len())
                .body(Body::from(cover.into_vec()))
                .unwrap();
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
        let max_age = "max-age=2592000"; // 30 days.
        let response = Response::builder()
            .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
            .header(CACHE_CONTROL, max_age)
            .header(CONTENT_TYPE, "image/jpeg")
            .header(CONTENT_LENGTH, data.len())
            .body(Body::from(data))
            .unwrap();
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
        let response = Response::builder()
            .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
            .header(CONTENT_TYPE, "audio/flac")
            .header(CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .unwrap();
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
        let response = Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
            .body(Body::from(w.into_inner()))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_albums(&self, _request: &Request) -> BoxFuture {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        self.index.write_albums_json(&mut w).unwrap();
        let response = Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .header(ACCESS_CONTROL_ALLOW_ORIGIN, "Any")
            .body(Body::from(w.into_inner()))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_artist(&self, _request: &Request, _id: &str) -> BoxFuture {
        let response = Response::builder()
            .body(Body::from("Artist"))
            .unwrap();
        Box::new(futures::future::ok(response))
    }

    fn handle_search(&self, _request: &Request) -> BoxFuture {
        let response = Response::builder()
            .body(Body::from("Search"))
            .unwrap();
        Box::new(futures::future::ok(response))
    }
}

impl Service for MetaServer {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = io::Error;
    type Future = BoxFuture;

    fn call(&mut self, request: Request) -> Self::Future {
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
            (&Method::GET, Some("cover"),  Some(t)) => self.handle_track_cover(&request, t),
            (&Method::GET, Some("thumb"),  Some(t)) => self.handle_thumb(&request, t),
            (&Method::GET, Some("track"),  Some(t)) => self.handle_track(&request, t),
            (&Method::GET, Some("album"),  Some(a)) => self.handle_album(&request, a),
            (&Method::GET, Some("albums"), None)    => self.handle_albums(&request),
            (&Method::GET, Some("artist"), Some(a)) => self.handle_artist(&request, a),
            (&Method::GET, Some("search"), None)    => self.handle_search(&request),
            // Web endpoints.
            (&Method::GET, None,              None) => self.handle_static_file("app/index.html", "text/html"),
            (&Method::GET, Some("style.css"), None) => self.handle_static_file("app/style.css", "text/css"),
            (&Method::GET, Some("app.js"),    None) => self.handle_static_file("app/output/app.js", "text/javascript"),
            // Fallback.
            (&Method::GET, _, _) => self.handle_not_found(),
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
            .map(|e| e.unwrap())
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
    println!("Index has {} tracks.", index.len());
    index
}

fn generate_thumbnail(cache_dir: &str, album_id: AlbumId, filename: &str) -> claxon::Result<()> {
    use std::process::{Command, Stdio};
    let opts = claxon::FlacReaderOptions {
        metadata_only: true,
        read_picture: claxon::ReadPicture::CoverAsVec,
        read_vorbis_comment: false,
    };
    let reader = claxon::FlacReader::open_ext(filename, opts)?;
    if let Some(cover) = reader.into_pictures().pop() {
        let mut out_fname: PathBuf = PathBuf::from(cache_dir);
        out_fname.push(format!("{}.jpg", album_id));

        // Early-out on existing files. The user would need to clear the cache
        // manually.
        if out_fname.is_file() {
            return Ok(())
        }

        println!("{:?} <- {}", &out_fname, filename);
        let mut convert = Command::new("convert")
            // Read from stdin.
            .arg("-")
            .args(&["-colorspace", "LAB"])
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
            .args(&["-quality", "95"])
            .arg(out_fname)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .spawn()
            .expect("Failed to spawn Imagemagick's 'convert'.");
        {
            let mut stdin = convert.stdin.as_mut().expect("Failed to open stdin.");
            stdin.write_all(cover.data()).unwrap();
        }
        // TODO: Use a custom error type, remove all `expect()`s.
        convert.wait().expect("Failed to run Imagemagick's 'convert'.");
    }
    Ok(())
}

fn generate_thumbnails(index: &MemoryMetaIndex, cache_dir: &str) {
    let mut prev_album_id = AlbumId(0);
    for &(_tid, ref track) in index.get_tracks() {
        if track.album_id != prev_album_id {
            let fname = index.get_filename(track.filename);
            generate_thumbnail(cache_dir, track.album_id, fname).unwrap();
            prev_album_id = track.album_id;
        }
    }
}

fn print_usage() {
    println!("usage: ");
    println!("  mindec serve /path/to/music/library /path/to/cache");
    println!("  mindec cache /path/to/music/library /path/to/cache");
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
            let index = Arc::new(make_index(&dir));
            println!("Indexing complete, starting server on port 8233.");

            let make_service = move || {
                let service = MetaServer::new(index.clone(), &cache_dir);
                let result: Result<_, io::Error> = Ok(service);
                result
            };
            let addr = ([0, 0, 0, 0], 8233).into();
            let server = Server::bind(&addr)
                .serve(make_service)
                .map_err(|e| eprintln!("Server error: {}", e));
            hyper::rt::run(server);
        }
        "cache" => {
            let index = make_index(&dir);
            generate_thumbnails(&index, &cache_dir);
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}
