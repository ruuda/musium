// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logging of historical playback events.

use std::sync::mpsc::Receiver;

use crate::{MetaIndex, TrackId};
use crate::player::QueueId;

/// Changes in the playback state to be recorded.
pub enum PlaybackEvent {
    Started(QueueId, TrackId),
    Completed(QueueId, TrackId),
}

/// Main for the thread that logs historical playback events.
pub fn main(index: &dyn MetaIndex, events: Receiver<PlaybackEvent>) {
    for event in events {
        let now = chrono::Utc::now();
        match event {
            PlaybackEvent::Started(queue_id, track_id) => {
                println!("Started {} {} at {}", queue_id, track_id, now);
            }
            PlaybackEvent::Completed(queue_id, track_id) => {
                println!("Completed {} {} at {}", queue_id, track_id, now);
            }
        }
    }
}
