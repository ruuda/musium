// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// Configuration module inspired by the one in Tako (github.com/ruuda/tako),
// which is copyright 2018 Arian van Putten, Ruud van Asseldonk, Tako Marks,
// and licensed under the Apache 2.0 License.

//! Configuration file parser.

use std::path::PathBuf;
use std::fmt;

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct Config {
    pub listen: String,
    pub library_path: PathBuf,
    pub covers_path: PathBuf,
    pub data_path: PathBuf,
    // TODO: Make this optional; pick the first one by default.
    pub audio_device: String,
    pub audio_volume_control: String,
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "  listen = {}\n", self.listen)?;
        write!(f, "  library_path = {}\n", self.library_path.to_string_lossy())?;
        write!(f, "  covers_path = {}\n", self.covers_path.to_string_lossy())?;
        write!(f, "  data_path = {}\n", self.data_path.to_string_lossy())?;
        write!(f, "  audio_device = {}\n", self.audio_device)?;
        write!(f, "  audio_volume_control = {}", self.audio_volume_control)?;
        Ok(())
    }
}

impl Config {
    pub fn parse<I, S>(lines: I) -> Result<Config>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut listen = None;
        let mut library_path = None;
        let mut covers_path = None;
        let mut data_path = None;
        let mut audio_device = None;
        let mut audio_volume_control = None;

        for (lineno, line_raw) in lines.into_iter().enumerate() {
            let line = line_raw.as_ref();

            // Allow empty lines in the config file.
            if line.len() == 0 {
                continue
            }

            // Skip lines starting with '#' to allow comments.
            if line.starts_with("#") {
                continue
            }

            if let Some(n) = line.find('=') {
                let key = line[..n].trim();
                let value = line[n + 1..].trim();
                match key {
                    "listen" => listen = Some(String::from(value)),
                    "library_path" => library_path = Some(PathBuf::from(value)),
                    "covers_path" => covers_path = Some(PathBuf::from(value)),
                    "data_path" => data_path = Some(PathBuf::from(value)),
                    "audio_device" => audio_device = Some(String::from(value)),
                    "audio_volume_control" => audio_volume_control = Some(String::from(value)),
                    _ => {
                        let msg = "Unknown key. Expected one of \
                            'listen', 'library_path', 'covers_path', \
                            or 'audio_device'.";
                        return Err(Error::InvalidConfig(lineno, msg))
                    }
                }
            } else {
                let msg = "Line contains no '='. \
                    Expected key-value pair like 'audio_device = UCM404HD 192k'.";
                return Err(Error::InvalidConfig(lineno, msg))
            }
        }

        let config = Config {
            listen: match listen {
                Some(b) => b,
                None => String::from("0.0.0.0:8233"),
            },
            library_path: match library_path {
                Some(p) => p,
                None => return Err(Error::IncompleteConfig(
                    "Library path not set. Expected 'library_path ='-line."
                )),
            },
            covers_path: match covers_path {
                Some(p) => p,
                None => return Err(Error::IncompleteConfig(
                    "Covers path not set. Expected 'covers_path ='-line."
                )),
            },
            data_path: match data_path {
                Some(p) => p,
                None => return Err(Error::IncompleteConfig(
                    "Data path not set. Expected 'data_path ='-line."
                )),
            },
            audio_device: match audio_device {
                Some(d) => d,
                None => return Err(Error::IncompleteConfig(
                    "Audio device not set. Expected 'audio_device ='-line."
                )),
            },
            audio_volume_control: match audio_volume_control {
                Some(d) => d,
                None => return Err(Error::IncompleteConfig(
                    "Audio volume control not set. Expected 'audio_volume_control ='-line."
                )),
            },
        };

        Ok(config)
    }

    pub fn db_path(&self) -> PathBuf {
        let mut db_path = self.data_path.clone();
        db_path.push("musium.sqlite3");
        db_path
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;
    use super::Config;

    #[test]
    pub fn config_can_be_parsed() {
        let config_lines = [
            "# This is a comment.",
            "listen = localhost:8000",
            "library_path = /home/user/music",
            "covers_path = /home/user/.cache/musium/covers",
            "data_path = /home/user/.local/share/musium",
            "",
            "audio_device = UCM404HD 192k",
            "audio_volume_control = UMC404HD 192k Output",
        ];
        let config = Config::parse(&config_lines).unwrap();
        assert_eq!(&config.listen[..], "localhost:8000");
        assert_eq!(config.library_path.as_path(), Path::new("/home/user/music"));
        assert_eq!(config.covers_path.as_path(), Path::new("/home/user/.cache/musium/covers"));
        assert_eq!(config.data_path.as_path(), Path::new("/home/user/.local/share/musium"));
        assert_eq!(&config.audio_device[..], "UCM404HD 192k");
        assert_eq!(&config.audio_volume_control[..], "UMC404HD 192k Output");
    }
}
