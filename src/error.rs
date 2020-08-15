// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::io;
use std::result;

#[derive(Debug)]
pub enum Error {
    /// Error in config file on a given line.
    InvalidConfig(usize, &'static str),

    /// A key is missing in the config.
    IncompleteConfig(&'static str),

    /// IO error.
    IoError(io::Error),
}

// TODO: Implement Display to make these a bit more user-friendly.

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

pub type Result<T> = result::Result<T, Error>;
