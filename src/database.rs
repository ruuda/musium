// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Interaction with Musium's SQLite database.

use std::path::PathBuf;

use sqlite;
use sqlite::Value;

use crate::player::QueueId;
use crate::prim::{AlbumId, ArtistId, TrackId};

pub type Result<T> = sqlite::Result<T>;

/// Row id of a row in the `listens` table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ListenId(i64);

/// Row id of a row in the `file_metadata` table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FileMetaId(pub i64);

/// Wraps the SQLite connection with some things to manipulate the DB.
pub struct Database<'conn> {
    pub connection: &'conn sqlite::Connection,
    insert_started: sqlite::Statement<'conn>,
    update_completed: sqlite::Statement<'conn>,
    insert_file_metadata: sqlite::Statement<'conn>,
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
        create table if not exists file_metadata
        -- First an id, and properties about the file, but not its contents.
        -- We can use this to see if a file needs to be re-scanned. The mtime
        -- is the raw time_t value returned by 'stat'.
        ( id                             integer primary key
        , filename                       string  not null unique
        , mtime                          integer not null
        -- ISO-8601 timestamp at which we added the file.
        , imported_at                    string not null

        -- The next columns come from the streaminfo block.
        , streaminfo_channels            integer not null
        , streaminfo_bits_per_sample     integer not null
        , streaminfo_num_samples         integer null
        , streaminfo_sample_rate         integer not null

        -- The remaining columns are all tags. They are all nullable,
        -- because no tag is guaranteed to be present.
        , tag_album                      string null
        , tag_albumartist                string null
        , tag_albumartistsort            string null
        , tag_artist                     string null
        , tag_musicbrainz_albumartistid  string null
        , tag_musicbrainz_albumid        string null
        , tag_musicbrainz_trackid        string null
        , tag_discnumber                 string null
        , tag_tracknumber                string null
        , tag_originaldate               string null
        , tag_date                       string null
        , tag_title                      string null
        , tag_bs17704_track_loudness     string null
        , tag_bs17704_album_loudness     string null
        );
        ",
    )?;

    Ok(())
}

/// Container for a row when inserting a new listen.
pub struct Listen<'a> {
    pub started_at: &'a str,
    pub queue_id: QueueId,
    pub track_id: TrackId,
    pub album_id: AlbumId,
    pub album_artist_id: ArtistId,
    pub track_title: &'a str,
    pub track_artist: &'a str,
    pub album_title: &'a str,
    pub album_artist: &'a str,
    pub duration_seconds: u16,
    pub track_number: u8,
    pub disc_number: u8,
}

/// Last modified time of a file, as reported by the file system.
///
/// This is only used to determine whether a file changed since we last read it,
/// the meaning of the inner value is not relevant, only that it implements
/// `Ord`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Mtime(pub i64);

/// Container for a row when inserting a new file.
pub struct FileMetadata<'a> {
    pub filename: &'a str,
    pub mtime: Mtime,
    pub imported_at: &'a str,
    pub streaminfo_channels: u32,
    pub streaminfo_bits_per_sample: u32,
    pub streaminfo_num_samples: Option<u64>,
    pub streaminfo_sample_rate: u32,
    pub tag_album: Option<&'a str>,
    pub tag_albumartist: Option<&'a str>,
    pub tag_albumartistsort: Option<&'a str>,
    pub tag_artist: Option<&'a str>,
    pub tag_musicbrainz_albumartistid: Option<&'a str>,
    pub tag_musicbrainz_albumid: Option<&'a str>,
    pub tag_musicbrainz_trackid: Option<&'a str>,
    pub tag_discnumber: Option<&'a str>,
    pub tag_tracknumber: Option<&'a str>,
    pub tag_originaldate: Option<&'a str>,
    pub tag_date: Option<&'a str>,
    pub tag_title: Option<&'a str>,
    pub tag_bs17704_track_loudness: Option<&'a str>,
    pub tag_bs17704_album_loudness: Option<&'a str>,
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

        let insert_file_metadata = connection.prepare(
            "
            insert into file_metadata
            ( filename
            , mtime
            , imported_at
            , streaminfo_channels
            , streaminfo_bits_per_sample
            , streaminfo_num_samples
            , streaminfo_sample_rate
            , tag_album
            , tag_albumartist
            , tag_albumartistsort
            , tag_artist
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
            )
            values
            -- These are 21 columns.
            ( ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);
            ",
        )?;


        let result = Database {
            connection: connection,
            insert_started: insert_started,
            update_completed: update_completed,
            insert_file_metadata: insert_file_metadata,
        };

        Ok(result)
    }

    /// Insert a listen into the "listens" table, return its row id.
    pub fn insert_listen_started(
        &mut self,
        listen: Listen,
    ) -> Result<ListenId> {
        self.insert_started.reset()?;
        self.insert_started.bind(1, listen.started_at)?;
        self.insert_started.bind(2, listen.queue_id.0 as i64)?;
        self.insert_started.bind(3, listen.track_id.0 as i64)?;
        self.insert_started.bind(4, listen.album_id.0 as i64)?;
        self.insert_started.bind(5, listen.album_artist_id.0 as i64)?;
        self.insert_started.bind(6, listen.track_title)?;
        self.insert_started.bind(7, listen.album_title)?;
        self.insert_started.bind(8, listen.track_artist)?;
        self.insert_started.bind(9, listen.album_artist)?;
        self.insert_started.bind(10, listen.duration_seconds as i64)?;
        self.insert_started.bind(11, listen.track_number as i64)?;
        self.insert_started.bind(12, listen.disc_number as i64)?;

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

    /// Insert a listen into the "listens" table, return its row id.
    pub fn insert_file_metadata(&mut self, file: FileMetadata) -> Result<()> {
        self.insert_file_metadata.reset()?;

        self.insert_file_metadata.bind(1, file.filename)?;
        self.insert_file_metadata.bind(2, file.mtime.0)?;
        self.insert_file_metadata.bind(3, file.imported_at)?;
        self.insert_file_metadata.bind(4, file.streaminfo_channels as i64)?;
        self.insert_file_metadata.bind(5, file.streaminfo_bits_per_sample as i64)?;
        self.insert_file_metadata.bind(6, file.streaminfo_num_samples.map(|x| x as i64))?;
        self.insert_file_metadata.bind(7, file.streaminfo_sample_rate as i64)?;
        self.insert_file_metadata.bind(8, file.tag_album)?;
        self.insert_file_metadata.bind(9, file.tag_albumartist)?;
        self.insert_file_metadata.bind(10, file.tag_albumartistsort)?;
        self.insert_file_metadata.bind(11, file.tag_artist)?;
        self.insert_file_metadata.bind(12, file.tag_musicbrainz_albumartistid)?;
        self.insert_file_metadata.bind(13, file.tag_musicbrainz_albumid)?;
        self.insert_file_metadata.bind(14, file.tag_musicbrainz_trackid)?;
        self.insert_file_metadata.bind(15, file.tag_discnumber)?;
        self.insert_file_metadata.bind(16, file.tag_tracknumber)?;
        self.insert_file_metadata.bind(17, file.tag_originaldate)?;
        self.insert_file_metadata.bind(18, file.tag_date)?;
        self.insert_file_metadata.bind(19, file.tag_title)?;
        self.insert_file_metadata.bind(20, file.tag_bs17704_track_loudness)?;
        self.insert_file_metadata.bind(21, file.tag_bs17704_album_loudness)?;

        let result = self.insert_file_metadata.next()?;
        // This query returns no rows, it should be done immediately.
        assert_eq!(result, sqlite::State::Done);

        Ok(())
    }

    /// Iterate the `file_metadata` table, sorted by filename.
    pub fn iter_file_metadata<'db>(&'db mut self) -> Result<FileMetaIter<'db>> {
        let cursor = self.connection.prepare(
            "
            select
              id, filename, mtime
            from
              file_metadata
            order by
              filename asc;
            ",
        )?
        .into_cursor();
        Ok(FileMetaIter {
            cursor: cursor
        })
    }
}

pub struct FileMetaIter<'db> {
    cursor: sqlite::Cursor<'db>
}

impl<'db> Iterator for FileMetaIter<'db> {
    type Item = Result<(FileMetaId, PathBuf, Mtime)>;

    fn next(&mut self) -> Option<Self::Item> {
        self.cursor.next().transpose().map(|row: Result<_>|
            match row {
                Ok([Value::Integer(id), Value::String(path), Value::Integer(mtime)]) => Ok((
                    FileMetaId(*id),
                    path.into(),
                    Mtime(*mtime),
                )),
                Ok(..) => panic!("Invalid row returned from iter_file_metas query."),
                Err(err) => Err(err),
            }
        )
    }
}
