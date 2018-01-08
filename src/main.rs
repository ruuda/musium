extern crate metaindex;
extern crate simple_server;
extern crate walkdir;

use std::env;
use std::process;
use std::ffi::OsStr;
use std::path::PathBuf;

use simple_server::{Request, Response, ResponseBuilder, Server};

fn route(request: Request<&[u8]>,
         response: &mut ResponseBuilder)
    // TODO: Try to get rid of the vec.
    -> Result<Response<Vec<u8>>, simple_server::Error> {
    Ok(response.body("Hi".as_bytes().to_vec())?)
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

    // Have a basic server to serve an API.
    let server = Server::new(|request, mut response| {
        println!("Request: {} {}", request.method(), request.uri());
        let parts: Vec<_> = request.uri().path().split('/').filter(|x| x.len() > 0).collect();

        if parts.len() == 2 {
            let section = parts[0];
            let id = parts[1];
            match section {
                "track" if id.ends_with(".flac") => {
                    Ok(response.body("serve raw track bytes".as_bytes())?)
                }
                "track" => {
                    Ok(response.body("track metadata".as_bytes())?)
                }
                "album" if id.ends_with(".jpg") => {
                    Ok(response.body("album cover art".as_bytes())?)
                }
                "album" => {
                    Ok(response.body("album metadata".as_bytes())?)
                }
                "artist" => {
                    Ok(response.body("artist metadata".as_bytes())?)
                }
                _ => {
                    Ok(response.body("BAD REQUEST".as_bytes())?)
                }
            }
        } else {
            Ok(response.body("BAD REQUEST".as_bytes())?)
        }
    });
    server.listen("0.0.0.0", "8233");
}
