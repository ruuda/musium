// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Interaction with Musium's SQLite database.

use sqlite;
use sqlite::Statement;

use crate::player::QueueId;
use crate::prim::{TrackId};

pub type Result<T> = sqlite::Result<T>;

/// Row id of a row in the `listens` table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ListenId(i64);

/// Row id of a row in the `file_metadata` table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct FileMetaId(pub i64);

/// Generate a struct that implements `sqlite::Bindable`.
///
/// Also generates a function that can prepare an insert query.
macro_rules! sql_bind {
    {
        // This "meta" captures doc comments, if applicable.
        $( #[$struct_attrs:meta] )*
        pub struct $Name:ident < $lt:lifetime >  {
            $( pub $field:ident: $type:ty, )*
        }

        // Capture a simple SQL query. This macro expands the * and ? to fill
        // the column names and values, and there is room for additional columns
        // at the end.
        query {
            INSERT INTO $table_name:ident
            ( * $( , $extra_column:ident )* )
            VALUES
            ( ? $( , $extra_value:tt )* )
            ;
        }
    } => {
        $( #[$struct_attrs] )*
        pub struct $Name<$lt> {
            $( pub $field: $type, )*
        }

        impl<$lt> $Name<$lt> {
            pub fn prepare_query(connection: &sqlite::Connection) -> Result<Statement> {
                // We build the SQL statement in memory first. This is more
                // tractable than trying to build a huge literal with concat!.
                // It looks scary, but it builds a statement of this form:
                //
                //   INSERT INTO table_name
                //   (field0, field1, ..., extra_column0, extra_column1, ...)
                //   VALUES
                //   (:field0, :field1, ..., extra_value0, extra_value1, ...);

                let mut statement = "INSERT INTO ".to_string();
                statement.push_str(stringify!($table_name));
                let mut is_first = true;
                $(
                    statement.push(if is_first { '(' } else { ',' });
                    statement.push_str(stringify!($field));
                    is_first = false;
                    let _ = is_first;  // Silence dead code warning.
                )*
                $(
                    statement.push(',');
                    statement.push_str(stringify!($extra_column));
                )*
                statement.push_str(") VALUES ");
                is_first = true;
                $(
                    statement.push(if is_first { '(' } else { ',' });
                    statement.push(':');
                    statement.push_str(stringify!($field));
                    is_first = false;
                    let _ = is_first;  // Silence dead code warning.
                )*
                $(
                    statement.push(',');
                    statement.push_str(stringify!($extra_value));
                )*
                statement.push_str(");");
                connection.prepare(&statement)
            }
        }

        impl<'a> sqlite::Bindable for &'a $Name<$lt> {
            fn bind(self, statement: &mut Statement, i: usize) -> Result<()> {
                let mut offset = i;
                $(
                    statement.bind(offset, self.$field)?;
                    offset += 1;
                )*
                let _ = offset; // Silence unused variable warning.
                Ok(())
            }
        }
    };
}

/// Generate a struct with `sqlite::Readable` implemented.
///
/// Also generates a function to prepare a SELECT query.
macro_rules! sql_read {
    {
        // This "meta" captures doc comments, if applicable.
        $( #[$struct_attrs:meta] )*
        pub struct $Name:ident {
            $( pub $field:ident: $type:ty, )*
        }

        query {
            SELECT * FROM $table_name:ident $( $extra_token:tt )*
        }
    } => {
        $( #[$struct_attrs] )*
        pub struct $Name {
            $( pub $field: $type, )*
        }

        impl $Name {
            pub fn prepare_query(connection: &sqlite::Connection) -> Result<Statement> {
                // We build the SQL statement in memory first. This is more
                // tractable than trying to build a huge literal with concat!.
                let mut statement = "SELECT ".to_string();
                let mut is_first = true;
                $(
                    if !is_first { statement.push(','); }
                    statement.push_str(stringify!($field));
                    is_first = false;
                    let _ = is_first;  // Silence dead code warning.
                )*
                statement.push_str(" FROM ");
                statement.push_str(stringify!($table_name));
                $(
                    statement.push(' ');
                    statement.push_str(stringify!($extra_token));
                )*
                connection.prepare(&statement)
            }
        }

        impl sqlite::Readable for $Name {
            fn read(statement: &Statement, i: usize) -> Result<Self> {
                let mut offset = i;
                $(
                    let $field: $type = statement.read(offset)?;
                    offset += 1;
                )*
                let _ = offset; // Silence unused variable warning.
                let result = Self {
                    $( $field, )*
                };
                Ok(result)
            }
        }
    };
}

/// Generate an iterator that executes queries the given type.
///
/// If the type was generated with `sql_read!`, then this macro will generate an
/// iterator that runs the query associated with the type, and iterates the
/// result, yielding instances of the type.
macro_rules! sql_iter {
    ($Name:ident => $Iter:ident) => {
        pub struct $Iter<'conn> {
            statement: Statement<'conn>,
        }

        impl<'conn> $Iter<'conn> {
            fn new(connection: &'conn sqlite::Connection) -> Result<Self> {
                let result = $Iter {
                    statement: $Name::prepare_query(connection)?
                };
                Ok(result)
            }
        }

        impl<'conn> Iterator for $Iter<'conn> {
            type Item = Result<$Name>;

            fn next(&mut self) -> Option<Self::Item> {
                match self.statement.next() {
                    Err(err) => Some(Err(err)),
                    Ok(sqlite::State::Done) => None,
                    Ok(sqlite::State::Row) => Some(self.statement.read(0)),
                }
            }
        }
    }
}

/// Wraps the SQLite connection with some things to manipulate the DB.
pub struct Database<'conn> {
    pub connection: &'conn sqlite::Connection,
    insert_started: Statement<'conn>,
    update_completed: Statement<'conn>,
    insert_file_metadata: Statement<'conn>,
    delete_file_metadata: Statement<'conn>,
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
    // NOTE: For this index, we need at least SQLite 3.20 (released 2017-08-01).
    // Earlier versions prohibit "strftime" because it can be non-deterministic
    // in some cases.
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

sql_bind! {
    /// Container for a row when inserting a new listen.
    pub struct Listen<'a> {
        pub started_at: &'a str,
        pub queue_id: i64,
        pub track_id: i64,
        pub album_id: i64,
        pub album_artist_id: i64,
        pub track_title: &'a str,
        pub track_artist: &'a str,
        pub album_title: &'a str,
        pub album_artist: &'a str,
        pub duration_seconds: i64,
        pub track_number: i64,
        pub disc_number: i64,
    }

    query {
        INSERT INTO listens (*, source) VALUES (?, "musium");
    }
}

sql_bind! {
    /// Container for a row when inserting into `file_metadata`.
    pub struct FileMetadataInsert<'a> {
        pub filename: &'a str,
        pub mtime: i64,
        pub imported_at: &'a str,
        pub streaminfo_channels: i64,
        pub streaminfo_bits_per_sample: i64,
        pub streaminfo_num_samples: Option<i64>,
        pub streaminfo_sample_rate: i64,
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

    query {
        INSERT INTO file_metadata (*) VALUES (?);
    }
}

sql_read! {
    /// Holds the columns from `file_metadata` needed to see if files need updating.
    #[derive(Debug)]
    pub struct FileMetadataSimple {
        pub id: i64,
        pub filename: String,
        pub mtime: i64,
    }

    query {
        SELECT * FROM file_metadata ORDER BY filename ASC;
    }
}

sql_iter!(FileMetadataSimple => FileMetadataSimpleIter);

sql_read! {
    /// Container for a row when iterating `file_metadata`.
    #[derive(Debug)]
    pub struct FileMetadata {
        pub filename: String,
        pub streaminfo_channels: i64,
        pub streaminfo_bits_per_sample: i64,
        pub streaminfo_num_samples: Option<i64>,
        pub streaminfo_sample_rate: i64,
        pub tag_album: Option<String>,
        pub tag_albumartist: Option<String>,
        pub tag_albumartistsort: Option<String>,
        pub tag_artist: Option<String>,
        pub tag_musicbrainz_albumartistid: Option<String>,
        pub tag_musicbrainz_albumid: Option<String>,
        pub tag_discnumber: Option<String>,
        pub tag_tracknumber: Option<String>,
        pub tag_originaldate: Option<String>,
        pub tag_date: Option<String>,
        pub tag_title: Option<String>,
        pub tag_bs17704_track_loudness: Option<String>,
        pub tag_bs17704_album_loudness: Option<String>,
    }

    query {
        SELECT * FROM file_metadata ORDER BY filename ASC;
    }
}

sql_iter!(FileMetadata => FileMetadataIter);

impl<'conn> Database<'conn> {
    /// Prepare statements.
    ///
    /// Does not ensure that all tables exist, use [`create_schema`] for that.
    pub fn new(connection: &sqlite::Connection) -> Result<Database> {
        let insert_started = Listen::prepare_query(connection)?;
        let insert_file_metadata = FileMetadataInsert::prepare_query(connection)?;

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

        let delete_file_metadata = connection.prepare(
            "
            delete from file_metadata where id = ?;
            "
        )?;

        let result = Database {
            connection: connection,
            insert_started: insert_started,
            update_completed: update_completed,
            insert_file_metadata: insert_file_metadata,
            delete_file_metadata: delete_file_metadata,
        };

        Ok(result)
    }

    /// Insert a listen into the "listens" table, return its row id.
    pub fn insert_listen_started(
        &mut self,
        listen: Listen,
    ) -> Result<ListenId> {
        self.insert_started.reset()?;
        self.insert_started.bind(1, &listen)?;

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

    /// Insert a row into the `file_metadata` table.
    pub fn insert_file_metadata(&mut self, file: FileMetadataInsert) -> Result<()> {
        self.insert_file_metadata.reset()?;
        self.insert_file_metadata.bind(1, &file)?;

        let result = self.insert_file_metadata.next()?;
        // This query returns no rows, it should be done immediately.
        assert_eq!(result, sqlite::State::Done);

        Ok(())
    }

    /// Delete a row from the `file_metadata` table.
    pub fn delete_file_metadata(&mut self, id: FileMetaId) -> Result<()> {
        self.delete_file_metadata.reset()?;
        self.delete_file_metadata.bind(1, id.0)?;
        let result = self.delete_file_metadata.next()?;
        // This query returns no rows, it should be done immediately.
        assert_eq!(result, sqlite::State::Done);
        Ok(())
    }

    /// Iterate the `file_metadata` table, sorted by filename.
    ///
    /// Returns only the id, filename, and mtime.
    pub fn iter_file_metadata_filename_mtime<'db>(
        &'db mut self,
    ) -> Result<FileMetadataSimpleIter<'db>> {
        FileMetadataSimpleIter::new(&self.connection)
    }

    /// Iterate the `file_metadata` table, sorted by filename.
    ///
    /// Returns the columns needed to build the `MetaIndex`.
    pub fn iter_file_metadata(&mut self) -> Result<FileMetadataIter> {
        FileMetadataIter::new(&self.connection)
    }
}
