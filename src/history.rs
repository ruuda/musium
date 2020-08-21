// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logging of historical playback events.

use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crate::{MetaIndex, TrackId};
use crate::player::QueueId;
use crate::serialization;

/// Changes in the playback state to be recorded.
pub enum PlaybackEvent {
    Started(QueueId, TrackId),
    Completed(QueueId, TrackId),
}

fn append_event(
    play_log_path: &Path,
    index: &dyn MetaIndex,
    event: PlaybackEvent,
) -> io::Result<()> {
    let now = chrono::Utc::now();
    let file = fs::OpenOptions::new().append(true).create(true).open(play_log_path)?;
    let mut writer = io::BufWriter::new(file);

    match event {
        PlaybackEvent::Started(queue_id, track_id) => {
            serialization::write_playback_event(
                index, &mut writer, now, "started", queue_id, track_id,
            )?;
        }
        PlaybackEvent::Completed(queue_id, track_id) => {
            serialization::write_playback_event(
                index, &mut writer, now, "completed", queue_id, track_id,
            )?;
        }
    }

    write!(writer, "\n")
}

/// Main for the thread that logs historical playback events.
pub fn main(
    play_log_path: Option<PathBuf>,
    index: &dyn MetaIndex,
    events: Receiver<PlaybackEvent>,
) {
    for event in events {
        // Only log if a file is provided. We do still run the thread and the
        // loop if logging is disabled, to make sure we drain the receiver.
        if let Some(ref p) = play_log_path {
            match append_event(&p, index, event) {
                Ok(()) => {},
                Err(err) => eprintln!("Failed to write to play log: {}", err),
            }
        }
    }
}
