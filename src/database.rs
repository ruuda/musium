// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Interaction with Musium's SQLite database.

use sqlite;

use crate::player::QueueId;
use crate::prim::{AlbumId, ArtistId, TrackId};

pub type Result<T> = sqlite::Result<T>;

/// Row id of a row in the `listens` table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ListenId(i64);

/// Wraps the SQLite connection with some things to manipulate the DB.
pub struct Database<'conn> {
    pub connection: &'conn sqlite::Connection,
    pub insert_started: sqlite::Statement<'conn>,
    pub update_completed: sqlite::Statement<'conn>,
    pub insert_file: sqlite::Statement<'conn>,
    pub get_file_mtime: sqlite::Statement<'conn>,
}

pub fn ensure_schema_exists(connection: &sqlite::Connection) -> Result<()> {
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
        ",
    )?;

    // We can record timestamps in sub-second granularity, but external systems
    // do not always support this. Last.fm only has second granularity. So if we
    // produce a listen, submit it to Last.fm, and later import it back, then we
    // should not get a duplicate. Therefore, create a unique index on the the
    // time truncated to seconds (%s formats seconds since epoch).
    connection.execute(
        "
        create unique index if not exists ix_listens_unique_second
        on listens (cast(strftime('%s', started_at) as integer));
        ",
    )?;

    // Next is the table with tag data. This is the raw data extracted from
    // Vorbis comments; it is not indexed, so it is not guaranteed to be
    // sensible. We store the raw data and index it when we load it, because
    // indexing itself is pretty fast; it's disk access to the first few bytes
    // of tens of thousands of files what makes indexing slow.
    connection.execute(
        "
        create table if not exists files
        -- First an id, and properties about the file, but not its contents.
        -- We can use this to see if a file needs to be re-scanned. The mtime
        -- is the raw time_t value returned by 'stat'.
        ( id                             integer primary key
        , filename                       string  not null unique
        , mtime                          integer not null

        -- The next columns come from the streaminfo block.
        , streaminfo_channels            integer not null
        , streaminfo_bits_per_sample     integer not null
        , streaminfo_samples             integer null
        , streaminfo_sample_rate         integer not null

        -- The remaining columns are all tags. They are all nullable,
        -- because no tag is guaranteed to be present.
        , tag_album                      string
        , tag_albumartist                string
        , tag_albumartistsort            string
        , tag_musicbrainz_albumartistid  string
        , tag_musicbrainz_albumid        string
        , tag_musicbrainz_trackid        string
        , tag_discnumber                 integer
        , tag_tracknumber                integer
        , tag_originaldate               string
        , tag_date                       string
        , tag_title                      string
        , tag_bs17704_track_loudness     string
        , tag_bs17704_album_loudness     string
        );
        ",
    )?;

    Ok(())
}

impl<'conn> Database<'conn> {
    /// Prepare statements.
    ///
    /// Does not ensure that all tables exist, use [`create_schema`] for that.
    pub fn new(connection: &sqlite::Connection) -> Result<Database> {
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
            ",
        )?;

        let update_completed = connection.prepare(
            "
            update listens
              set completed_at = ?
            where
              id = ?
              and queue_id = ?
              and track_id = ?;
            ",
        )?;

        let insert_file = connection.prepare(
            "
            insert into files
            ( filename
            , mtime
            , streaminfo_channels
            , streaminfo_bits_per_sample
            , streaminfo_samples
            , streaminfo_sample_rate
            , tag_album
            , tag_albumartist
            , tag_albumartistsort
            , tag_musicbrainz_albumartistid
            , tag_musicbrainz_albumid
            , tag_musicbrainz_trackid
            , tag_discnumber
            , tag_tracknumber
            , tag_originaldate
            , tag_date
            , tag_title
            , tag_bs17704_track_loudness
            , tag_bs17704_album_loudness
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
            ( ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
            , ?, ?, ?, ?, ?, ?, ?, ?
            );
            ",
        )?;

        let get_file_mtime = connection.prepare(
            "
            select mtime from files where filename = ?;
            ",
        )?;

        let result = Database {
            connection: connection,
            insert_started: insert_started,
            update_completed: update_completed,
            insert_file: insert_file,
            get_file_mtime: get_file_mtime,
        };

        Ok(result)
    }

    /// Insert a listen into the "listens" table, return its row id.
    pub fn insert_listen_started(
        &mut self,
        started_at: &str,
        queue_id: QueueId,
        track_id: TrackId,
        album_id: AlbumId,
        album_artist_id: ArtistId,
        track_title: &str,
        track_artist: &str,
        album_title: &str,
        album_artist: &str,
        duration_seconds: u16,
        track_number: u8,
        disc_number: u8,
    ) -> Result<ListenId> {
        self.insert_started.reset()?;
        self.insert_started.bind(1, started_at)?;
        self.insert_started.bind(2, queue_id.0 as i64)?;
        self.insert_started.bind(3, track_id.0 as i64)?;
        self.insert_started.bind(4, album_id.0 as i64)?;
        self.insert_started.bind(5, album_artist_id.0 as i64)?;
        self.insert_started.bind(6, track_title)?;
        self.insert_started.bind(7, track_artist)?;
        self.insert_started.bind(8, album_title)?;
        self.insert_started.bind(9, album_artist)?;
        self.insert_started.bind(10, duration_seconds as i64)?;
        self.insert_started.bind(11, track_number as i64)?;
        self.insert_started.bind(12, disc_number as i64)?;

        let result = self.insert_started.next()?;
        // This query returns no rows, it should be done immediately.
        assert_eq!(result, sqlite::State::Done);

        // The "sqlite" crate does not have a wrapper for this function.
        let id = unsafe {
            sqlite3_sys::sqlite3_last_insert_rowid(self.connection.as_raw())
        } as i64;

        Ok(ListenId(id))
    }

    /// Update the completed time of a previously inserted listen.
    ///
    /// Also takes the queue id and track id as a sanity check.
    pub fn update_listen_completed(
        &mut self,
        listen_id: ListenId,
        completed_time: &str,
        queue_id: QueueId,
        track_id: TrackId,
    ) -> Result<()> {
        self.update_completed.reset()?;
        self.update_completed.bind(1, completed_time)?;
        self.update_completed.bind(2, listen_id.0)?;
        self.update_completed.bind(3, queue_id.0 as i64)?;
        self.update_completed.bind(4, track_id.0 as i64)?;

        let result = self.update_completed.next()?;
        // This query returns no rows, it should be done immediately.
        assert_eq!(result, sqlite::State::Done);

        Ok(())
    }
}
