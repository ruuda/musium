// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::io;
use std::path::PathBuf;
use std::result;

#[derive(Debug)]
pub enum Error {
    /// Error in config file on a given line.
    InvalidConfig(usize, &'static str),

    /// A key is missing in the config.
    IncompleteConfig(&'static str),

    /// Running an external program ("command") failed.
    CommandError(&'static str, io::Error),

    /// IO error.
    IoError(io::Error),

    /// An FLAC file at a given location could not be read.
    FormatError(PathBuf, claxon::Error),

    /// Interaction with the SQLite database failed.
    DatabaseError(sqlite::Error),
}

impl Error {
    pub fn from_claxon(path: PathBuf, err: claxon::Error) -> Error {
        match err {
            claxon::Error::IoError(err) => Error::IoError(err),
            _ => Error::FormatError(path, err),
        }
    }
}

// TODO: Implement Display to make these a bit more user-friendly.

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

impl From<sqlite::Error> for Error {
    fn from(err: sqlite::Error) -> Error {
        Error::DatabaseError(err)
    }
}

pub type Result<T> = result::Result<T, Error>;
