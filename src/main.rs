extern crate metaindex;
extern crate walkdir;

use std::env;
use std::process;
use std::ffi::OsStr;
use std::path::PathBuf;

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
}
