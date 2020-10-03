// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logging of historical playback events.

use std::path::{PathBuf};
use std::sync::mpsc::Receiver;
use sqlite;
use sqlite3_sys;

use crate::{MetaIndex, TrackId};
use crate::player::QueueId;

/// Changes in the playback state to be recorded.
pub enum PlaybackEvent {
    Started(QueueId, TrackId),
    Completed(QueueId, TrackId),
}

type Result<T> = sqlite::Result<T>;

/// Wraps the SQLite connection with some things to manipulate the DB.
struct Database<'conn> {
    connection: &'conn sqlite::Connection,
    insert_started: sqlite::Statement<'conn>,
    update_completed: sqlite::Statement<'conn>,
    last_insert_id: Option<i64>,
}

/// Ensure that the "listens" table exists, prepare statements.
fn initialize_db(connection: &sqlite::Connection) -> Result<Database> {
    connection.execute(
        "
        create table if not exists listens
        ( id               integer primary key

        -- ISO-8601 time with UTC offset at which we started playing.
        , started_at       string  not null unique

        -- ISO-8601 time with UTC offset at which we finished playing.
        -- NULL if the track is still playing.
        , completed_at     string  null     check (started_at < completed_at)

        -- Musium ids.
        , queue_id         integer null
        , track_id         integer not null
        , album_id         integer not null
        , album_artist_id  integer not null

        -- General track metadata.
        , track_title      string  not null
        , album_title      string  not null
        , track_artist     string  not null
        , album_artist     string  not null
        , duration_seconds integer not null
        , track_number     integer null
        , disc_number      integer null

        -- Source of the listen. Should be either 'musium' if we produced the
        -- listen, or 'listenbrainz' if we backfilled it from Listenbrainz.
        , source           string  not null

        -- ISO-8601 time with UTC offset at which we scrobbled the track to Last.fm.
        -- NULL if the track has not been scrobbled by us.
        , scrobbled_at     string  null     check (started_at < scrobbled_at)
        );
        "
    )?;

    // We can record timestamps in sub-second granularity, but external systems
    // do not always support this. Last.fm only has second granularity. So if we
    // produce a listen, submit it to Last.fm, and later import it back, then we
    // should not get a duplicate. Therefore, create a unique index on the the
    // time truncated to seconds (%s formats seconds since epoch).
    connection.execute(
        "
        create unique index if not exists ix_listens_unique_second
        on listens (strftime('%s', started_at));
        "
    )?;

    let insert_started = connection.prepare(
        "
        insert into listens
        ( started_at
        , queue_id
        , track_id
        , album_id
        , album_artist_id
        , track_title
        , album_title
        , track_artist
        , album_artist
        , duration_seconds
        , track_number
        , disc_number
        , source
        )
        values
        ( ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'musium');
        "
    )?;

    let update_completed = connection.prepare(
        "
        update listens
          set completed_at = ?
        where
          id = ?
          and queue_id = ?
          and track_id = ?;
        "
    )?;

    let result = Database {
        connection: connection,
        insert_started: insert_started,
        update_completed: update_completed,
        last_insert_id: None,
    };
    Ok(result)
}

/// Insert a new row into the "listens" table.
fn insert_started(
    db: &mut Database,
    index: &dyn MetaIndex,
    time: chrono::DateTime<chrono::Utc>,
    queue_id: QueueId,
    track_id: TrackId,
) -> Result<()> {
    let use_zulu_suffix = true;
    let time_str = time.to_rfc3339_opts(chrono::SecondsFormat::Millis, use_zulu_suffix);
    let track = index.get_track(track_id).unwrap();
    let album = index.get_album(track.album_id).unwrap();
    let artist = index.get_artist(album.artist_id).unwrap();

    db.insert_started.reset()?;
    db.insert_started.bind(1, &time_str[..])?;
    db.insert_started.bind(2, queue_id.0 as i64)?;
    db.insert_started.bind(3, track_id.0 as i64)?;
    db.insert_started.bind(4, track.album_id.0 as i64)?;
    db.insert_started.bind(5, album.artist_id.0 as i64)?;
    db.insert_started.bind(6, index.get_string(track.title))?;
    db.insert_started.bind(7, index.get_string(album.title))?;
    db.insert_started.bind(8, index.get_string(track.artist))?;
    db.insert_started.bind(9, index.get_string(artist.name))?;
    db.insert_started.bind(10, track.duration_seconds as i64)?;
    db.insert_started.bind(11, track.track_number as i64)?;
    db.insert_started.bind(12, track.disc_number as i64)?;

    let result = db.insert_started.next()?;
    // This query returns no rows, it should be done immediately.
    assert_eq!(result, sqlite::State::Done);

    // The "sqlite" crate does not have a wrapper for this function.
    let id = unsafe {
        sqlite3_sys::sqlite3_last_insert_rowid(db.connection.as_raw())
    } as i64;

    db.last_insert_id = Some(id);
    Ok(())
}

/// Update a row to insert the completed time.
fn update_completed(
    db: &mut Database,
    row_id: i64,
    time: chrono::DateTime<chrono::Utc>,
    queue_id: QueueId,
    track_id: TrackId,
) -> Result<()> {
    let use_zulu_suffix = true;
    let time_str = time.to_rfc3339_opts(chrono::SecondsFormat::Millis, use_zulu_suffix);

    db.update_completed.reset()?;
    db.update_completed.bind(1, &time_str[..])?;
    db.update_completed.bind(2, row_id)?;
    db.update_completed.bind(3, queue_id.0 as i64)?;
    db.update_completed.bind(4, track_id.0 as i64)?;

    let result = db.update_completed.next()?;
    // This query returns no rows, it should be done immediately.
    assert_eq!(result, sqlite::State::Done);

    Ok(())
}

fn append_event(
    db: &mut Database,
    index: &dyn MetaIndex,
    event: PlaybackEvent,
) -> Result<()> {
    let now = chrono::Utc::now();

    match event {
        PlaybackEvent::Started(queue_id, track_id) => {
            insert_started(db, index, now, queue_id, track_id)?;
        }
        PlaybackEvent::Completed(queue_id, track_id) => {
            if let Some(row_id) = db.last_insert_id {
                update_completed(db, row_id, now, queue_id, track_id)?;
            }
        }
    }

    Ok(())
}

/// Main for the thread that logs historical playback events.
pub fn main(
    db_path: PathBuf,
    index: &dyn MetaIndex,
    events: Receiver<PlaybackEvent>,
) {
    let connection = sqlite::open(db_path).expect("Failed to open SQLite database.");
    let mut db = initialize_db(&connection).expect("Failed to initialize SQLite database.");
    for event in events {
        match append_event(&mut db, index, event) {
            Ok(()) => {},
            Err(err) => eprintln!("Failed to write event to SQLite database: {}", err),
        }
    }
}
