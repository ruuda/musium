// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::fs;
use std::io;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Request, Response, ResponseBox, Server};
use tiny_http::Method::{Get, Post, Put, self};

use crate::player::{Millibel, Player};
use crate::prim::{ArtistId, AlbumId, TrackId};
use crate::serialization;
use crate::string_utils::normalize_words;
use crate::systemd;
use crate::thumb_cache::ThumbCache;
use crate::{MetaIndex, MemoryMetaIndex};

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

pub struct MetaServer {
    index: Arc<MemoryMetaIndex>,
    thumb_cache: ThumbCache,
    player: Player,
}

impl MetaServer {
    pub fn new(
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

    fn handle_artist(&self, id: &str) -> ResponseBox {
        let artist_id = match ArtistId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid artist id."),
        };

        let artist = match self.index.get_artist(artist_id) {
            Some(a) => a,
            None => return self.handle_not_found(),
        };

        let albums = self.index.get_albums_by_artist(artist_id);

        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        serialization::write_artist_json(&*self.index, &mut w, artist, albums).unwrap();

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
        serialization::write_queue_json(
            &*self.index,
            &mut w,
            &queue.tracks[..],
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

    /// Router function for all /api/«endpoint» calls.
    fn handle_api_request(&self, method: &Method, endpoint: &str, arg: Option<&str>, query: &str) -> ResponseBox {
        match (method, endpoint, arg) {
            // API endpoints.
            (&Get, "cover",  Some(t)) => self.handle_album_cover(t),
            (&Get, "thumb",  Some(t)) => self.handle_thumb(t),
            (&Get, "track",  Some(t)) => self.handle_track(t),
            (&Get, "album",  Some(a)) => self.handle_album(a),
            (&Get, "artist", Some(a)) => self.handle_artist(a),
            (&Get, "albums", None)    => self.handle_albums(),
            (&Get, "search", None)    => self.handle_search(query),
            (&Get, "queue",  None)    => self.handle_queue(),
            (&Put, "queue",  Some(t)) => self.handle_enqueue(t),

            // Volume control, volume up/down change the volume by 1 dB.
            (&Get,  "volume", None)         => self.handle_get_volume(),
            (&Post, "volume", Some("up"))   => self.handle_change_volume(Millibel( 1_00)),
            (&Post, "volume", Some("down")) => self.handle_change_volume(Millibel(-1_00)),

            _ => self.handle_bad_request("No such (method, endpoint, argument) combination."),
        }
    }

    fn handle_request(&self, request: Request) {
        // Break url into the part before the ? and the part after. The part
        // before we split on slashes.
        let mut url_iter = request.url().splitn(2, '?');

        // The individual parts in between the slashes.
        let mut p0 = None;
        let mut p1 = None;
        let mut p2 = None;

        if let Some(base) = url_iter.next() {
            let mut parts = base.splitn(4, '/').filter(|x| x.len() > 0);

            p0 = parts.next();
            p1 = parts.next();
            p2 = parts.next();
        }

        let query = url_iter.next().unwrap_or("");

        // A very basic router. See also docs/api.md for an overview.
        let response = match (request.method(), p0, p1) {
            // API endpoints go through the API router, to keep this match arm
            // a bit more concise.
            (method, Some("api"), Some(endpoint)) => self.handle_api_request(method, endpoint, p2, query),

            // Web endpoints.
            (&Get, None,                  None) => self.handle_static_file("app/index.html", "text/html"),
            (&Get, Some("style.css"),     None) => self.handle_static_file("app/style.css", "text/css"),
            (&Get, Some("dark.css"),      None) => self.handle_static_file("app/dark.css", "text/css"),
            (&Get, Some("manifest.json"), None) => self.handle_static_file("app/manifest.json", "text/javascript"),
            (&Get, Some("app.js"),        None) => self.handle_static_file("app/output/app.js", "text/javascript"),
            (&Get, Some(path),            None) if path.ends_with(".svg") => {
                let mut file_path = "app/".to_string();
                file_path.push_str(path);
                self.handle_static_file(&file_path, "image/svg+xml")
            }
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

pub fn serve(bind: &str, service: Arc<MetaServer>) {
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

    // When running under systemd, the service is readly when the server is
    // accepting connections, which is now.
    if systemd::can_notify() {
        systemd::notify("STATUS=Online\nREADY=1\n".into())
            .expect("Failed signal ready to systemd.");
    }

    // Block until all threads have stopped, which only happens in case of an
    // error on all of them.
    for thread in threads.drain(..) {
        thread.join().unwrap();
    }
}