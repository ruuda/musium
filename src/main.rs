extern crate futures;
extern crate hyper;
extern crate metaindex;
extern crate walkdir;

use std::env;
use std::process;
use std::ffi::OsStr;
use std::path::PathBuf;

use futures::future::Future;
use hyper::server::{Http, Request, Response, Service};

struct MetaServer;

impl Service for MetaServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item=Self::Response, Error=Self::Error>>;

    fn call(&self, request: Request) -> Self::Future {
        println!("Request: {:?}", request);
        let parts: Vec<&str> = request.uri().path().split('/').filter(|x| x.len() > 0).collect();

        let response = if parts.len() == 2 {
            let section = parts[0];
            let id = parts[1];
            match section {
                "track" if id.ends_with(".flac") => {
                    Response::<hyper::Body>::new()
                        .with_body("serve raw track bytes".as_bytes())
                }
                "track" => {
                    Response::<hyper::Body>::new()
                        .with_body("track metadata".as_bytes())
                }
                "album" if id.ends_with(".jpg") => {
                    Response::<hyper::Body>::new()
                        .with_body("album cover art".as_bytes())
                }
                "album" => {
                    Response::<hyper::Body>::new()
                        .with_body("album metadata".as_bytes())
                }
                "artist" => {
                    Response::<hyper::Body>::new()
                        .with_body("artist metadata".as_bytes())
                }
                _ => {
                    Response::<hyper::Body>::new()
                        .with_body("BAD REQUEST".as_bytes())
                }
            }
        } else {
            Response::<hyper::Body>::new()
                .with_body("BAD REQUEST".as_bytes())
        };

        Box::new(futures::future::ok(response))
    }
}

fn main() {
    if env::args().len() < 2 {
        println!("usage: index /path/to/music/library");
        process::exit(1);
    }

    let dir = env::args().nth(1).unwrap();
    let wd = walkdir::WalkDir::new(&dir)
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

    assert!(metaindex::MemoryMetaIndex::from_paths(paths.iter()).is_ok());
    println!("Indexing complete, starting server on port 8233.");
    let addr = ([0, 0, 0, 0], 8233).into();
    let server = Http::new().bind(&addr, || Ok(MetaServer)).unwrap();
    server.run().unwrap();
}
