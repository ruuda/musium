// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::convert::TryFrom;
use std::fs;
use std::io;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::Method::{self, Delete, Get, Post, Put};
use tiny_http::{Header, Request, Response, ResponseBox, Server};

use crate::config::Config;
use crate::database as db;
use crate::database::Connection;
use crate::database_utils;
use crate::mvar::Var;
use crate::playcount::TimeVector;
use crate::player::{Millibel, Player, QueueId};
use crate::prim::{AlbumId, ArtistId, Hertz, TrackId};
use crate::scan::BackgroundScanner;
use crate::serialization;
use crate::string_utils::normalize_words;
use crate::systemd;
use crate::thumb_cache::ThumbCache;
use crate::user_data::{Rating, UserData};
use crate::{MemoryMetaIndex, MetaIndex};

fn header_content_type(content_type: &str) -> Header {
    Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .expect("Failed to create content-type header, value is not ascii.")
}

fn header_expires_seconds(age_seconds: i64) -> Header {
    let now = chrono::Utc::now();
    let at = now
        .checked_add_signed(chrono::Duration::seconds(age_seconds))
        .unwrap();
    // The format from https://tools.ietf.org/html/rfc7234#section-5.3.
    let value = at.format("%a, %e %b %Y %H:%M:%S GMT").to_string();
    Header::from_bytes(&b"Expires"[..], value)
        .expect("Failed to create content-type header, value is not ascii.")
}

pub struct MetaServer {
    config: Config,
    index_var: Var<MemoryMetaIndex>,
    thumb_cache_var: Var<ThumbCache>,
    user_data: Arc<Mutex<UserData>>,
    player: Player,
    scanner: BackgroundScanner,
}

impl MetaServer {
    pub fn new(
        config: Config,
        index_var: Var<MemoryMetaIndex>,
        thumb_cache_var: Var<ThumbCache>,
        user_data: Arc<Mutex<UserData>>,
        player: Player,
    ) -> MetaServer {
        MetaServer {
            config: config,
            index_var: index_var.clone(),
            thumb_cache_var: thumb_cache_var.clone(),
            user_data: user_data,
            player: player,
            scanner: BackgroundScanner::new(index_var, thumb_cache_var),
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

        let index = &*self.index_var.get();
        let tracks = index.get_album_tracks(album_id);
        let track = &tracks
            .first()
            .expect("Albums have at least one track.")
            .track;
        let fname = index.get_filename(track.filename);

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
        // TODO: DRY this track id parsing and loading part.
        let album_id = match AlbumId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid album id."),
        };

        let thumb_cache = self.thumb_cache_var.get();

        let img = match thumb_cache.get(album_id) {
            None => return self.handle_not_found(),
            Some(bytes) => bytes,
        };

        Response::from_data(img)
            .with_header(header_content_type("image/jpeg"))
            .with_header(header_expires_seconds(3600 * 24 * 30))
            .boxed()
    }

    fn handle_waveform(&self, db: &mut Connection, id: &str) -> ResponseBox {
        use crate::waveform::Waveform;

        // TODO: DRY this track id parsing and loading part.
        let track_id = match TrackId::parse(id) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let waveform = db.begin().and_then(|mut tx| {
            let result = db::select_track_waveform(&mut tx, track_id.0 as i64)?;
            tx.commit()?;
            Ok(result)
        });

        let waveform = match waveform {
            Ok(Some(data)) => Waveform::from_bytes(data),
            Ok(None) => return self.handle_not_found(),
            Err(err) => {
                eprintln!("Error while loading waveform: {:?}", err);
                return self.handle_error("Database error.");
            }
        };

        let mut svg = Vec::new();
        waveform
            .write_svg(&mut svg)
            .expect("Write to memory does not fail.");

        Response::from_data(svg)
            .with_header(header_content_type("image/svg+xml"))
            .with_header(header_expires_seconds(3600 * 24 * 30))
            .boxed()
    }

    fn handle_track(&self, path: &str) -> ResponseBox {
        // Track urls are of the form `/track/f7c153f2b16dc101.flac`.
        if !path.ends_with(".flac") {
            return self.handle_bad_request("Expected a path ending in .flac.");
        }

        let id_part = &path[..path.len() - ".flac".len()];
        let track_id = match TrackId::parse(id_part) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let index = &*self.index_var.get();
        let track = match index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

        let fname = index.get_filename(track.filename);

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

        let index = &*self.index_var.get();
        let album = match index.get_album(album_id) {
            Some(a) => a,
            None => return self.handle_not_found(),
        };

        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);

        serialization::write_album_json(
            index,
            &self.user_data.lock().unwrap(),
            &mut w,
            album_id,
            album,
        )
        .unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_artist(&self, id: &str) -> ResponseBox {
        let artist_id = match ArtistId::parse(id) {
            Some(aid) => aid,
            None => return self.handle_bad_request("Invalid artist id."),
        };

        let index = &*self.index_var.get();
        let artist = match index.get_artist(artist_id) {
            Some(a) => a,
            None => return self.handle_not_found(),
        };

        let albums = index.get_albums_by_artist(artist_id);
        let now = TimeVector::now();

        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        serialization::write_artist_json(
            index,
            &self.user_data.lock().unwrap(),
            &now,
            &mut w,
            artist,
            albums,
        )
        .unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_albums(&self) -> ResponseBox {
        let index = &*self.index_var.get();
        let now = TimeVector::now();
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        serialization::write_albums_json(index, &self.user_data.lock().unwrap(), &now, &mut w)
            .unwrap();

        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_rating(&self, track_id: &str, rating_str: &str) -> ResponseBox {
        let rating = match i64::from_str(rating_str)
            .map_err(|_| "Failed to parse rating.")
            .and_then(Rating::try_from)
        {
            Ok(r) => r,
            Err(_) => return self.handle_bad_request("Invalid rating."),
        };

        let track_id = match TrackId::parse(track_id) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let index = &*self.index_var.get();

        // Confirm that the track exists before we store its rating.
        let _track = match index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

        // Send the new rating to the history thread.
        self.player.set_track_rating(track_id, rating);

        // The history thread will write to the database and update the user
        // data afterwards.
        Response::empty(202).boxed()
    }

    fn handle_queue(&self) -> ResponseBox {
        let index = &*self.index_var.get();
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let queue = self.player.get_queue();
        serialization::write_queue_json(
            index,
            &self.user_data.lock().unwrap(),
            &mut w,
            &queue.tracks[..],
        )
        .unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_enqueue(&self, id: &str) -> ResponseBox {
        let track_id = match TrackId::parse(id) {
            Some(tid) => tid,
            None => return self.handle_bad_request("Invalid track id."),
        };

        let index = &*self.index_var.get();

        // Confirm that the track exists before we enqueue it.
        let _track = match index.get_track(track_id) {
            Some(t) => t,
            None => return self.handle_not_found(),
        };

        let queue_id = self.player.enqueue(index, track_id);
        let queue_id_json = format!(r#""{}""#, queue_id);

        Response::from_string(queue_id_json)
            .with_status_code(201) // "201 Created"
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_dequeue(&self, id: &str) -> ResponseBox {
        let queue_id = match QueueId::parse(id) {
            Some(qid) => qid,
            None => return self.handle_bad_request("Invalid queue id."),
        };
        self.player.dequeue(queue_id);
        Response::empty(200).boxed()
    }

    fn handle_queue_shuffle(&self) -> ResponseBox {
        let index = &*self.index_var.get();
        self.player.shuffle(index);
        self.handle_queue()
    }

    fn handle_queue_clear(&self) -> ResponseBox {
        self.player.clear_queue();
        self.handle_queue()
    }

    fn handle_get_volume(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let params = self.player.get_params();
        serialization::write_player_params_json(&mut w, &params).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_change_volume(&self, add: Millibel) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let params = self.player.change_volume(add);
        serialization::write_player_params_json(&mut w, &params).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_change_cutoff(&self, add: Hertz) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let params = self.player.change_cutoff(add);
        serialization::write_player_params_json(&mut w, &params).unwrap();
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
        }
        let query = match opt_query {
            Some(q) => q,
            None => return self.handle_bad_request("Missing search query."),
        };

        let mut words = Vec::new();
        normalize_words(query.as_ref(), &mut words);

        let mut artists = Vec::new();
        let mut albums = Vec::new();
        let mut tracks = Vec::new();

        let index = &*self.index_var.get();
        index.search_artist(&words[..], &mut artists);
        index.search_album(&words[..], &mut albums);
        index.search_track(&words[..], &mut tracks);

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
            index,
            &mut w,
            &artists[..n_artists],
            &albums[..n_albums],
            &tracks[..n_tracks],
        )
        .unwrap();

        Response::from_data(w.into_inner())
            .with_status_code(200)
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_get_scan_status(&self) -> ResponseBox {
        // TODO: We could add a long polling query parameter here, and version
        // the status. Then in the request, include the previous version. If the
        // current version is newer, respond immediately. If not, block for some
        // time to wait for a new status, then return the current status. That
        // way, we could make extremely responsive status updates.
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let status = self.scanner.get_status();
        serialization::write_scan_status_json(&mut w, status).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_start_scan(&self) -> ResponseBox {
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        let status = self.scanner.start(self.config.clone());
        serialization::write_scan_status_json(&mut w, Some(status)).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    fn handle_stats(&self) -> ResponseBox {
        let index = &*self.index_var.get();
        let buffer = Vec::new();
        let mut w = io::Cursor::new(buffer);
        serialization::write_stats_json(index, &mut w).unwrap();
        Response::from_data(w.into_inner())
            .with_header(header_content_type("application/json"))
            .boxed()
    }

    /// Router function for all /api/«endpoint» calls.
    #[rustfmt::skip]
    fn handle_api_request(
        &self,
        db: &mut Connection,
        method: &Method,
        endpoint: &str,
        arg1: Option<&str>,
        arg2: Option<&str>,
        arg3: Option<&str>,
        query: &str,
    ) -> ResponseBox {
        match (method, endpoint, arg1) {
            // API endpoints.
            (&Get, "cover",    Some(t)) => self.handle_album_cover(t),
            (&Get, "thumb",    Some(t)) => self.handle_thumb(t),
            (&Get, "waveform", Some(t)) => self.handle_waveform(db, t),
            (&Get, "track",    Some(t)) => self.handle_track(t),
            (&Get, "album",    Some(a)) => self.handle_album(a),
            (&Get, "artist",   Some(a)) => self.handle_artist(a),
            (&Get, "albums",   None)    => self.handle_albums(),
            (&Get, "search",   None)    => self.handle_search(query),
            (&Get, "stats",    None)    => self.handle_stats(),

            // Rating.
            (&Put, "track", Some(t)) => match (arg2, arg3) {
                (Some("rating"), Some(r)) => self.handle_rating(t, r),
                _ => {
                    println!("{arg2:?} {arg3:?}");
                    self.handle_bad_request("No such endpoint.")
                }
            }

            // Play queue manipulation.
            (&Get,    "queue",  None)            => self.handle_queue(),
            (&Put,    "queue",  Some(t))         => self.handle_enqueue(t),
            (&Delete, "queue",  Some(t))         => self.handle_dequeue(t),
            (&Post,   "queue",  Some("shuffle")) => self.handle_queue_shuffle(),
            (&Post,   "queue",  Some("clear"))   => self.handle_queue_clear(),

            // Volume control, volume up/down change the volume by 1 dB.
            (&Get,  "volume", None)         => self.handle_get_volume(),
            (&Post, "volume", Some("up"))   => self.handle_change_volume(Millibel( 1_00)),
            (&Post, "volume", Some("down")) => self.handle_change_volume(Millibel(-1_00)),
            (&Post, "filter", Some("up"))   => self.handle_change_cutoff(Hertz(5)),
            (&Post, "filter", Some("down")) => self.handle_change_cutoff(Hertz(-5)),

            // Background library scanning.
            (&Get,  "scan", Some("status")) => self.handle_get_scan_status(),
            (&Post, "scan", Some("start"))  => self.handle_start_scan(),

            _ => self.handle_bad_request("No such (method, endpoint, argument) combination."),
        }
    }

    fn handle_request(&self, db: &mut Connection, request: Request) {
        // Break url into the part before the ? and the part after. The part
        // before we split on slashes.
        let mut url_iter = request.url().splitn(2, '?');

        // The individual parts in between the slashes.
        let mut p0 = None;
        let mut p1 = None;
        let mut p2 = None;
        let mut p3 = None;
        let mut p4 = None;

        if let Some(base) = url_iter.next() {
            let mut parts = base.splitn(6, '/').filter(|x| x.len() > 0);

            p0 = parts.next();
            p1 = parts.next();
            p2 = parts.next();
            p3 = parts.next();
            p4 = parts.next();
        }

        let query = url_iter.next().unwrap_or("");

        // A very basic router. See also docs/api.md for an overview.
        #[rustfmt::skip]
        let response = match (request.method(), p0, p1) {
            // API endpoints go through the API router, to keep this match arm
            // a bit more concise.
            (method, Some("api"), Some(endpoint)) => self.handle_api_request(db, method, endpoint, p2, p3, p4, query),

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
            Ok(()) => {}
            Err(err) => println!("Error while responding to request: {:?}", err),
        }
    }
}

pub fn serve(bind: &str, service: Arc<MetaServer>) -> ! {
    let server = match Server::http(bind) {
        Ok(s) => s,
        Err(..) => {
            eprintln!("Failed to start server, could not bind to {}.", bind);
            std::process::exit(1);
        }
    };

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
        let join_handle = builder
            .spawn(move || {
                let connection = database_utils::connect_readonly(&service_i.config.db_path)
                    .expect("Failed to connect to database.");
                let mut db = Connection::new(&connection);
                loop {
                    let request = match server_i.recv() {
                        Ok(rq) => rq,
                        Err(e) => {
                            println!("Error: {:?}", e);
                            break;
                        }
                    };
                    service_i.handle_request(&mut db, request);
                }
            })
            .unwrap();
        threads.push(join_handle);
    }

    // When running under systemd, the service is ready when the server is
    // accepting connections, which is now.
    systemd::notify_ready_if_can_notify();

    // Block until the server threads exit, which will not happen.
    for handle in threads {
        handle.join().unwrap();
    }

    unreachable!("The server runs indefinitely, joins should not return.")
}
