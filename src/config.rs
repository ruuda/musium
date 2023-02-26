// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

// Configuration module inspired by the one in Tako (github.com/ruuda/tako),
// which is copyright 2018 Arian van Putten, Ruud van Asseldonk, Tako Marks,
// and licensed under the Apache 2.0 License.

//! Configuration file parser.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::error::{Error, Result};
use crate::prim::Hertz;

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: String,
    pub library_path: PathBuf,
    pub db_path: PathBuf,
    // TODO: Make this optional; pick the first one by default.
    pub audio_device: String,
    pub audio_volume_control: String,
    pub high_pass_cutoff: Hertz,
    pub exec_pre_playback_path: Option<PathBuf>,
    pub exec_post_idle_path: Option<PathBuf>,
    pub idle_timeout_seconds: u64,
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "  listen                 = {}\n", self.listen)?;
        write!(f, "  library_path           = {}\n", self.library_path.to_string_lossy())?;
        write!(f, "  db_path                = {}\n", self.db_path.to_string_lossy())?;
        write!(f, "  audio_device           = {}\n", self.audio_device)?;
        write!(f, "  audio_volume_control   = {}\n", self.audio_volume_control)?;
        write!(f, "  high_pass_cutoff       = {}\n", self.high_pass_cutoff)?;
        match self.exec_pre_playback_path.as_ref() {
            Some(path) => write!(f, "  exec_pre_playback_path = {}\n", path.to_string_lossy())?,
            None => write!(f, "  exec_pre_playback_path is not set\n")?,
        }
        match self.exec_post_idle_path.as_ref() {
            Some(path) => write!(f, "  exec_post_idle_path    = {}\n", path.to_string_lossy())?,
            None => write!(f, "  exec_post_idle_path    is not set\n")?,
        }
        write!(f, "  idle_timeout_seconds   = {}", self.idle_timeout_seconds)?;

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
        let mut db_path = None;
        let mut audio_device = None;
        let mut audio_volume_control = None;
        let mut high_pass_cutoff = None;
        let mut exec_pre_playback_path = None;
        let mut exec_post_idle_path = None;
        let mut idle_timeout_seconds = 180;

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
                    "db_path" => db_path = Some(PathBuf::from(value)),
                    "audio_device" => audio_device = Some(String::from(value)),
                    "audio_volume_control" => audio_volume_control = Some(String::from(value)),
                    "high_pass_cutoff" => match Hertz::from_str(value) {
                        Ok(hz) => high_pass_cutoff = Some(hz),
                        Err(msg) => return Err(Error::InvalidConfig(lineno, msg)),
                    }
                    "exec_pre_playback_path" => exec_pre_playback_path = Some(PathBuf::from(value)),
                    "exec_post_idle_path" => exec_post_idle_path = Some(PathBuf::from(value)),
                    "idle_timeout_seconds" => match u64::from_str(value) {
                        Ok(seconds) => idle_timeout_seconds = seconds,
                        Err(_) => {
                            let msg = "Invalid idle_timout_seconds value, must be an integer.";
                            return Err(Error::InvalidConfig(lineno, msg));
                        }
                    }
                    _ => {
                        let msg = "Unknown key. See the configuration docs for supported keys.";
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
            db_path: match db_path {
                Some(p) => p,
                None => return Err(Error::IncompleteConfig(
                    "Database path not set. Expected 'db_path ='-line."
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
            high_pass_cutoff: match high_pass_cutoff {
                Some(hz) => hz,
                None => Hertz(0),
            },
            exec_pre_playback_path: exec_pre_playback_path,
            exec_post_idle_path: exec_post_idle_path,
            idle_timeout_seconds: idle_timeout_seconds,
        };

        Ok(config)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;
    use super::{Config, Hertz};

    #[test]
    pub fn config_can_be_parsed() {
        let config_lines = [
            "# This is a comment.",
            "listen = localhost:8000",
            "library_path = /home/user/music",
            "db_path = /home/user/.local/share/musium/db.sqlite3",
            "",
            "audio_device = UCM404HD 192k",
            "audio_volume_control = UMC404HD 192k Output",
            "high_pass_cutoff = 50 Hz",
        ];
        let config = Config::parse(&config_lines).unwrap();
        assert_eq!(&config.listen[..], "localhost:8000");
        assert_eq!(config.library_path.as_path(), Path::new("/home/user/music"));
        assert_eq!(config.db_path.as_path(), Path::new("/home/user/.local/share/musium/db.sqlite3"));
        assert_eq!(&config.audio_device[..], "UCM404HD 192k");
        assert_eq!(&config.audio_volume_control[..], "UMC404HD 192k Output");
        assert_eq!(config.high_pass_cutoff, Hertz(50));
    }
}
