// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logging of historical playback events.

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use chrono::{SecondsFormat, Utc};

use crate::database_utils;
use crate::database as db;
use crate::database::{Connection, Listen, Result};
use crate::mvar::Var;
use crate::player::QueueId;
use crate::{MetaIndex, MemoryMetaIndex, TrackId};
use crate::user_data::{Rating, UserData};
use crate::playcount::PlayCounter;

/// Changes in the playback state or library to be recorded.
pub enum PlaybackEvent {
    Started(QueueId, TrackId),
    Completed(QueueId, TrackId),
    QueueEnded,

    /// The user modified the rating for the given track.
    Rated {
        track_id: TrackId,
        rating: Rating,
    },
}

/// Main for the thread that logs historical playback events.
pub fn main(
    db_path: &Path,
    index_var: Var<MemoryMetaIndex>,
    user_data: Arc<Mutex<UserData>>,
    mut counter: PlayCounter,
    events: Receiver<PlaybackEvent>,
) -> Result<()> {
    let connection = database_utils::connect_read_write(db_path)?;
    let mut db = Connection::new(&connection);

    let mut last_listen_id = None;

    for event in events {
        let now = Utc::now();
        let use_zulu_suffix = true;
        let now_str = now.to_rfc3339_opts(SecondsFormat::Millis, use_zulu_suffix);

        match event {
            PlaybackEvent::Started(queue_id, track_id) => {
                let index = index_var.get();
                let track = index.get_track(track_id).unwrap();
                let album = index.get_album(track_id.album_id()).unwrap();
                let album_artists = index.get_album_artists(album.artist_ids);
                let listen = Listen {
                    started_at: &now_str[..],
                    file_id: track.file_id.0,
                    queue_id: queue_id.0 as i64,
                    track_id: track_id.0 as i64,
                    album_id: track_id.album_id().0 as i64,
                    // We record only the first album artist, to keep the
                    // structure of the table simple.
                    album_artist_id: album_artists[0].0 as i64,
                    track_title: index.get_string(track.title),
                    album_title: index.get_string(album.title),
                    track_artist: index.get_string(track.artist),
                    album_artist: index.get_string(album.artist),
                    duration_seconds: track.duration_seconds as i64,
                    track_number: track_id.track_number() as i64,
                    disc_number: track_id.disc_number() as i64,
                };
                let mut tx = db.begin()?;
                let result = db::insert_listen_started(&mut tx, listen)?;
                tx.commit()?;
                last_listen_id = Some(result);
            }
            PlaybackEvent::Completed(queue_id, track_id) => {
                if let Some(listen_id) = last_listen_id {
                    let mut tx = db.begin()?;
                    db::update_listen_completed(
                        &mut tx,
                        listen_id,
                        queue_id.0 as i64,
                        track_id.0 as i64,
                        &now_str[..],
                    )?;
                    tx.commit()?;
                } else {
                    panic!(
                        "Completed queue entry {}, track {}, before starting.",
                        queue_id, track_id,
                    );
                }
            }
            PlaybackEvent::QueueEnded => {
                // When the queue ends, flush the WAL. This is not really
                // needed, but I back up my database with rsync once in a
                // while, and I like to have everything in one file instead
                // of having to sync the WAL as well. We checkpoint after
                // the queue ends, before the post-playback program runs.
                connection.execute("PRAGMA wal_checkpoint(PASSIVE);")?;

                // Recompute the playcounts as well. We load these from the
                // database rather than updating the counts on the go for two
                // reasons:
                // 1. We need to anyway at startup, so this way there is only
                //    one code path.
                // 2. We can pick up imported listens, though only when they
                //    are not backdated to before our last update there. When
                //    they are backdated, it requires an application restart.
                // The count import is incremental. Computing the ranking is
                // not, but that's fast enough anyway. (The full import +
                // ranking is 140ms on a Raspberry Pi for ~22k tracks.)
                let index = index_var.get();
                let mut tx = db.begin()?;
                counter.count_from_database(&index, &mut tx)?;
                tx.commit()?;
                let counts = counter.into_counts();
                let album_user_data = counts.compute_album_user_data();
                user_data.lock().unwrap().set_albums(album_user_data);
                counter = counts.into_counter();
            }
            PlaybackEvent::Rated { track_id, rating } => {
                let mut tx = db.begin()?;
                db::insert_or_replace_rating(
                    &mut tx,
                    track_id.0 as i64,
                    &now_str,
                    rating as i64,
                )?;
                tx.commit()?;
                user_data.lock().unwrap().set_track_rating(track_id, rating);
            }
        }
    }

    Ok(())
}
