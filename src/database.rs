// This file was generated by Squiller 0.4.0 (commit 7bc113622d).
// Input files:
// - database.sql

#![allow(unknown_lints)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::needless_question_mark)]

use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::hash_map::HashMap;

use sqlite::{State::{Row, Done}, Statement};

pub type Result<T> = sqlite::Result<T>;

pub struct Connection<'a> {
    connection: &'a sqlite::Connection,
    statements: HashMap<*const u8, Statement<'a>>,
}

pub struct Transaction<'tx, 'a> {
    connection: &'a sqlite::Connection,
    statements: &'tx mut HashMap<*const u8, Statement<'a>>,
}

pub struct Iter<'i, 'a, T> {
    statement: &'i mut Statement<'a>,
    decode_row: fn(&Statement<'a>) -> Result<T>,
}

impl<'a> Connection<'a> {
    pub fn new(connection: &'a sqlite::Connection) -> Self {
        Self {
            connection,
            // TODO: We could do with_capacity here, because we know the number
            // of queries.
            statements: HashMap::new(),
        }
    }

    /// Begin a new transaction by executing the `BEGIN` statement.
    pub fn begin<'tx>(&'tx mut self) -> Result<Transaction<'tx, 'a>> {
        self.connection.execute("BEGIN;")?;
        let result = Transaction {
            connection: self.connection,
            statements: &mut self.statements,
        };
        Ok(result)
    }
}

impl<'tx, 'a> Transaction<'tx, 'a> {
    /// Execute `COMMIT` statement.
    pub fn commit(self) -> Result<()> {
        self.connection.execute("COMMIT;")
    }

    /// Execute `ROLLBACK` statement.
    pub fn rollback(self) -> Result<()> {
        self.connection.execute("ROLLBACK;")
    }
}

impl<'i, 'a, T> Iterator for Iter<'i, 'a, T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Result<T>> {
        match self.statement.next() {
            Ok(Row) => Some((self.decode_row)(self.statement)),
            Ok(Done) => None,
            Err(err) => Some(Err(err)),
        }
    }
}

pub fn ensure_schema_exists(tx: &mut Transaction) -> Result<()> {
    let sql = r#"
        create table if not exists listens
        ( id               integer primary key
        
        -- ISO-8601 time with UTC offset at which we started playing.
        , started_at       string  not null unique
        
        -- ISO-8601 time with UTC offset at which we finished playing.
        -- NULL if the track is still playing.
        , completed_at     string  null     check (started_at < completed_at)
        
        -- References a file from the files table, but there is no foreign key. We want
        -- to keep the listen around even when the file disappears. Also, this needs to
        -- be nullable because in the past we did not record it, so historical listens
        -- may not have it.
        , file_id          integer null
        
        -- Musium ids. The album artist id is the first album artist, in case there are
        -- multiple.
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
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        -- We can record timestamps in sub-second granularity, but external systems
        -- do not always support this. Last.fm only has second granularity. So if we
        -- produce a listen, submit it to Last.fm, and later import it back, then we
        -- should not get a duplicate. Therefore, create a unique index on the the
        -- time truncated to seconds (%s formats seconds since epoch).
        -- NOTE: For this index, we need at least SQLite 3.20 (released 2017-08-01).
        -- Earlier versions prohibit "strftime" because it can be non-deterministic
        -- in some cases.
        create unique index if not exists ix_listens_unique_second
        on listens (cast(strftime('%s', started_at) as integer));
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create table if not exists ratings
        ( id          integer primary key
        -- ISO-8601 time with UTC offset at which we rated the track.
        , created_at  string  not null unique
        -- Musium track that we are rating. We don't enforce a foreign key relation
        -- here, such that when we re-import a track we don't lose the rating data. The
        -- downside is that we may end up with dangling ratings if tracks get deleted
        -- or moved (e.g. a correction in track number), but that's acceptable.
        , track_id    integer not null
        -- The rating for this track.
        , rating      integer not null check ((rating >= -1) and (rating <= 2))
        -- "musium" for ratings created from Musium, otherwise the source that the
        -- rating was imported from, e.g. "last.fm".
        , source      string not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create unique index if not exists ix_ratings_unique_second
        on ratings (cast(strftime('%s', created_at) as integer));
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create table if not exists files
        -- First an id, and properties about the file, but not its contents.
        -- We can use this to see if a file needs to be re-scanned. The mtime
        -- is the raw time_t value returned by 'stat'.
        ( id                             integer primary key
        , filename                       string  not null unique
        , mtime                          integer not null
        
        -- ISO-8601 timestamp at which we added the file.
        , imported_at                    string  not null
        
        -- The next columns come from the streaminfo block.
        , streaminfo_channels            integer not null
        , streaminfo_bits_per_sample     integer not null
        , streaminfo_num_samples         integer     null
        , streaminfo_sample_rate         integer not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create table if not exists tags
        ( id         integer primary key
        , file_id    integer not null references files (id) on delete cascade
        , field_name string  not null
        , value      string  not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create index if not exists ix_tags_file_id on tags (file_id);
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        -- BS1770.4 integrated loudness over the track, in LUFS.
        create table if not exists track_loudness
        ( track_id              integer primary key
        , file_id               integer not null references files (id) on delete cascade
        , bs17704_loudness_lufs real    not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        -- BS1770.4 integrated loudness over the album, in LUFS.
        -- For the file id, we track the maximum file id of all the files in the album.
        -- If any of the files change, it will get a new file id, higher than any pre-
        -- existing file, so if the maximum file id for an album is greater than the
        -- file id stored with the loudness here, then we know we need to recompute the
        -- album loudness.
        create table if not exists album_loudness
        ( album_id              integer primary key
        , file_id               integer not null references files (id) on delete cascade
        , bs17704_loudness_lufs real not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        -- "Waveform" data per track, used to render waveforms in the UI.
        -- See waveform.rs for the data format.
        create table if not exists waveforms
        ( track_id integer primary key
        , file_id  integer not null references files (id) on delete cascade
        , data     blob    not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => {}
    }

    let sql = r#"
        create table if not exists thumbnails
        ( album_id integer primary key
        , file_id  integer not null references files (id) on delete cascade
        , data     blob    not null
        );
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let result = match statement.next()? {
        Row => panic!("Query 'ensure_schema_exists' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

#[derive(Debug)]
pub struct InsertFile<'a> {
    pub filename: &'a str,
    pub mtime: i64,
    pub imported_at: &'a str,
    pub streaminfo_channels: i64,
    pub streaminfo_bits_per_sample: i64,
    pub streaminfo_num_samples: Option<i64>,
    pub streaminfo_sample_rate: i64,
}

pub fn insert_file(tx: &mut Transaction, metadata: InsertFile) -> Result<i64> {
    let sql = r#"
        insert into files
        ( filename
        , mtime
        , imported_at
        , streaminfo_channels
        , streaminfo_bits_per_sample
        , streaminfo_num_samples
        , streaminfo_sample_rate
        )
        values
        ( :filename
        , :mtime
        , :imported_at
        , :streaminfo_channels
        , :streaminfo_bits_per_sample
        , :streaminfo_num_samples
        , :streaminfo_sample_rate
        )
        returning id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, metadata.filename)?;
    statement.bind(2, metadata.mtime)?;
    statement.bind(3, metadata.imported_at)?;
    statement.bind(4, metadata.streaminfo_channels)?;
    statement.bind(5, metadata.streaminfo_bits_per_sample)?;
    statement.bind(6, metadata.streaminfo_num_samples)?;
    statement.bind(7, metadata.streaminfo_sample_rate)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => decode_row(statement)?,
        Done => panic!("Query 'insert_file' should return exactly one row."),
    };
    if statement.next()? != Done {
        panic!("Query 'insert_file' should return exactly one row.");
    }
    Ok(result)
}

pub fn insert_tag(tx: &mut Transaction, file_id: i64, field_name: &str, value: &str) -> Result<()> {
    let sql = r#"
        insert into
          tags (file_id, field_name, value)
          values (:file_id, :field_name, :value);
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, file_id)?;
    statement.bind(2, field_name)?;
    statement.bind(3, value)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_tag' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

/// Delete a file and everything referencing it (cascade to tags, waveforms, etc.)
///
/// Note that album loudness is not deleted, it is not based on any single file.
pub fn delete_file(tx: &mut Transaction, file_id: i64) -> Result<()> {
    let sql = r#"
        delete from files where id = :file_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, file_id)?;
    let result = match statement.next()? {
        Row => panic!("Query 'delete_file' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

#[derive(Debug)]
pub struct FileMetadataSimple {
    pub id: i64,
    pub filename: String,
    pub mtime: i64,
}

pub fn iter_file_mtime<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, FileMetadataSimple>> {
    let sql = r#"
        select
            id
          , filename
          , mtime
        from
          files
        order by
          filename asc;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok(FileMetadataSimple {
        id: statement.read(0)?,
        filename: statement.read(1)?,
        mtime: statement.read(2)?,
    });
    let result = Iter { statement, decode_row };
    Ok(result)
}

#[derive(Debug)]
pub struct FileMetadata {
    pub id: i64,
    pub filename: String,
    pub mtime: i64,
    pub streaminfo_channels: i64,
    pub streaminfo_bits_per_sample: i64,
    pub streaminfo_num_samples: Option<i64>,
    pub streaminfo_sample_rate: i64,
}

pub fn iter_files<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, FileMetadata>> {
    let sql = r#"
        select
            id
          , filename
          , mtime
          , streaminfo_channels
          , streaminfo_bits_per_sample
          , streaminfo_num_samples
          , streaminfo_sample_rate
        from
          files
        order by
          filename asc;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok(FileMetadata {
        id: statement.read(0)?,
        filename: statement.read(1)?,
        mtime: statement.read(2)?,
        streaminfo_channels: statement.read(3)?,
        streaminfo_bits_per_sample: statement.read(4)?,
        streaminfo_num_samples: statement.read(5)?,
        streaminfo_sample_rate: statement.read(6)?,
    });
    let result = Iter { statement, decode_row };
    Ok(result)
}

/// Iterate all `(field_name, value)` pairs for the given file.
pub fn iter_file_tags<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>, file_id: i64) -> Result<Iter<'i, 'a, (String, String)>> {
    let sql = r#"
        select
          field_name, value
        from
          tags
        where
          file_id = :file_id
        order by
          -- We have to order by id, which is increasing with insert order, because some
          -- tags can occur multiple times, and we have to preserve the order in which
          -- we found them in the file.
          id asc;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, file_id)?;
    let decode_row = |statement: &Statement| Ok((
        statement.read(0)?,
        statement.read(1)?,
));
    let result = Iter { statement, decode_row };
    Ok(result)
}

pub fn insert_album_thumbnail(tx: &mut Transaction, album_id: i64, file_id: i64, data: &[u8]) -> Result<()> {
    let sql = r#"
        insert into thumbnails (album_id, file_id, data)
        values (:album_id, :file_id, :data)
        on conflict (album_id) do update set data = :data;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, album_id)?;
    statement.bind(2, file_id)?;
    statement.bind(3, data)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_album_thumbnail' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

pub fn insert_album_loudness(tx: &mut Transaction, album_id: i64, file_id: i64, loudness: f64) -> Result<()> {
    let sql = r#"
        insert into album_loudness (album_id, file_id, bs17704_loudness_lufs)
        values (:album_id, :file_id, :loudness)
        on conflict (album_id) do update set bs17704_loudness_lufs = :loudness;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, album_id)?;
    statement.bind(2, file_id)?;
    statement.bind(3, loudness)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_album_loudness' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

pub fn insert_track_loudness(tx: &mut Transaction, track_id: i64, file_id: i64, loudness: f64) -> Result<()> {
    let sql = r#"
        insert into track_loudness (track_id, file_id, bs17704_loudness_lufs)
        values (:track_id, :file_id, :loudness)
        on conflict (track_id) do update set bs17704_loudness_lufs = :loudness;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    statement.bind(2, file_id)?;
    statement.bind(3, loudness)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_track_loudness' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

pub fn insert_track_waveform(tx: &mut Transaction, track_id: i64, file_id: i64, data: &[u8]) -> Result<()> {
    let sql = r#"
        insert into waveforms (track_id, file_id, data)
        values (:track_id, :file_id, :data)
        on conflict (track_id) do update set data = :data;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    statement.bind(2, file_id)?;
    statement.bind(3, data)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_track_waveform' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

#[derive(Debug)]
pub struct Listen<'a> {
    pub started_at: &'a str,
    pub file_id: i64,
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

pub fn insert_listen_started(tx: &mut Transaction, listen: Listen) -> Result<i64> {
    let sql = r#"
        insert into
          listens
          ( started_at
          , file_id
          , queue_id
          , track_id
          , album_id
          , album_artist_id
          , track_title
          , track_artist
          , album_title
          , album_artist
          , duration_seconds
          , track_number
          , disc_number
          , source
          )
        values
          ( :started_at
          , :file_id
          , :queue_id
          , :track_id
          , :album_id
          , :album_artist_id
          , :track_title
          , :track_artist
          , :album_title
          , :album_artist
          , :duration_seconds
          , :track_number
          , :disc_number
          , 'musium'
          )
        returning
          id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, listen.started_at)?;
    statement.bind(2, listen.file_id)?;
    statement.bind(3, listen.queue_id)?;
    statement.bind(4, listen.track_id)?;
    statement.bind(5, listen.album_id)?;
    statement.bind(6, listen.album_artist_id)?;
    statement.bind(7, listen.track_title)?;
    statement.bind(8, listen.track_artist)?;
    statement.bind(9, listen.album_title)?;
    statement.bind(10, listen.album_artist)?;
    statement.bind(11, listen.duration_seconds)?;
    statement.bind(12, listen.track_number)?;
    statement.bind(13, listen.disc_number)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => decode_row(statement)?,
        Done => panic!("Query 'insert_listen_started' should return exactly one row."),
    };
    if statement.next()? != Done {
        panic!("Query 'insert_listen_started' should return exactly one row.");
    }
    Ok(result)
}

pub fn update_listen_completed(tx: &mut Transaction, listen_id: i64, queue_id: i64, track_id: i64, completed_at: &str) -> Result<()> {
    let sql = r#"
        update listens
          set completed_at = :completed_at
        where
          id = :listen_id
          and queue_id = :queue_id
          and track_id = :track_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, completed_at)?;
    statement.bind(2, listen_id)?;
    statement.bind(3, queue_id)?;
    statement.bind(4, track_id)?;
    let result = match statement.next()? {
        Row => panic!("Query 'update_listen_completed' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

pub fn select_album_loudness_lufs(tx: &mut Transaction, album_id: i64) -> Result<Option<f64>> {
    let sql = r#"
        select bs17704_loudness_lufs from album_loudness where album_id = :album_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, album_id)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => Some(decode_row(statement)?),
        Done => None,
    };
    if result.is_some() {
        if statement.next()? != Done {
            panic!("Query 'select_album_loudness_lufs' should return at most one row.");
        }
    }
    Ok(result)
}

pub fn select_track_loudness_lufs(tx: &mut Transaction, track_id: i64) -> Result<Option<f64>> {
    let sql = r#"
        select bs17704_loudness_lufs from track_loudness where track_id = :track_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => Some(decode_row(statement)?),
        Done => None,
    };
    if result.is_some() {
        if statement.next()? != Done {
            panic!("Query 'select_track_loudness_lufs' should return at most one row.");
        }
    }
    Ok(result)
}

pub fn select_track_waveform(tx: &mut Transaction, track_id: i64) -> Result<Option<Vec<u8>>> {
    let sql = r#"
        select data from waveforms where track_id = :track_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => Some(decode_row(statement)?),
        Done => None,
    };
    if result.is_some() {
        if statement.next()? != Done {
            panic!("Query 'select_track_waveform' should return at most one row.");
        }
    }
    Ok(result)
}

/// Return the sum of the sizes (in bytes) of all thumbnails.
pub fn select_thumbnails_count_and_total_size(tx: &mut Transaction) -> Result<(i64, i64)> {
    let sql = r#"
        select count(*), sum(length(data)) from thumbnails;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok((
        statement.read(0)?,
        statement.read(1)?,
));
    let result = match statement.next()? {
        Row => decode_row(statement)?,
        Done => panic!("Query 'select_thumbnails_count_and_total_size' should return exactly one row."),
    };
    if statement.next()? != Done {
        panic!("Query 'select_thumbnails_count_and_total_size' should return exactly one row.");
    }
    Ok(result)
}

#[derive(Debug)]
pub struct Thumbnail {
    pub album_id: i64,
    pub data: Vec<u8>,
}

pub fn iter_thumbnails<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, Thumbnail>> {
    let sql = r#"
        select album_id, data from thumbnails;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok(Thumbnail {
        album_id: statement.read(0)?,
        data: statement.read(1)?,
    });
    let result = Iter { statement, decode_row };
    Ok(result)
}

/// Return whether a thumbnail for the album exists (1 if it does, 0 otherwise).
pub fn select_thumbnail_exists(tx: &mut Transaction, album_id: i64) -> Result<i64> {
    let sql = r#"
        select count(*) from thumbnails where album_id = :album_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, album_id)?;
    let decode_row = |statement: &Statement| Ok(statement.read(0)?);
    let result = match statement.next()? {
        Row => decode_row(statement)?,
        Done => panic!("Query 'select_thumbnail_exists' should return exactly one row."),
    };
    if statement.next()? != Done {
        panic!("Query 'select_thumbnail_exists' should return exactly one row.");
    }
    Ok(result)
}

/// For every album, return the earliest listen in the listens table.
///
/// Yields tuples `(album_id, started_at_iso8601)`.
pub fn iter_album_first_listens<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, (i64, String)>> {
    let sql = r#"
        select
          -- We rely on the fact here that asciibetical sorting of ISO-8601 strings
          -- with the same time zone offset is also chronological, and our listens all
          -- have Z suffix (+00 UTC offset).
          album_id, min(started_at)
        from
          listens
        group by
          album_id;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok((
        statement.read(0)?,
        statement.read(1)?,
));
    let result = Iter { statement, decode_row };
    Ok(result)
}

#[derive(Debug)]
pub struct ListenAt {
    pub track_id: i64,
    pub started_at_second: i64,
}

/// Iterate the listens in chronological order.
pub fn iter_listens<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, ListenAt>> {
    let sql = r#"
        select
            track_id,
            -- Note that we have an index on this expression, so this should be just an
            -- index scan.
            cast(strftime('%s', started_at) as integer) as started_at_second
        from
            listens
        order by
            started_at_second asc;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok(ListenAt {
        track_id: statement.read(0)?,
        started_at_second: statement.read(1)?,
    });
    let result = Iter { statement, decode_row };
    Ok(result)
}

/// Insert a rating for a given track.
///
/// When the `created_at` timestamp is not unique, this replaces the previous
/// rating that was present for that timestamp. This might happen when the user
/// edits the rating in quick succession; then we only store the last write.
pub fn insert_or_replace_rating(tx: &mut Transaction, track_id: i64, created_at: &str, rating: i64) -> Result<()> {
    let sql = r#"
        insert or replace into
          ratings (track_id, created_at, rating, source)
        values
          (:track_id, :created_at, :rating, 'musium');
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    statement.bind(2, created_at)?;
    statement.bind(3, rating)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_or_replace_rating' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

/// Backfill a rating for a given track.
///
/// The timestamp must be unique on the second.
pub fn insert_rating(tx: &mut Transaction, track_id: i64, created_at: &str, rating: i64, source: &str) -> Result<()> {
    let sql = r#"
        insert into
          ratings (track_id, created_at, rating, source)
        values
          (:track_id, :created_at, :rating, :source);
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    statement.bind(1, track_id)?;
    statement.bind(2, created_at)?;
    statement.bind(3, rating)?;
    statement.bind(4, source)?;
    let result = match statement.next()? {
        Row => panic!("Query 'insert_rating' unexpectedly returned a row."),
        Done => (),
    };
    Ok(result)
}

#[derive(Debug)]
pub struct TrackRating {
    pub id: i64,
    pub track_id: i64,
    pub rating: i64,
}

pub fn iter_ratings<'i, 't, 'a>(tx: &'i mut Transaction<'t, 'a>) -> Result<Iter<'i, 'a, TrackRating>> {
    let sql = r#"
        select
            id
          , track_id
          , rating
        from
          ratings
        order by
          -- Order by ascending creation time to ensure we can clamp to rating ranges,
          -- should we need to. We have an index on this expression.
          cast(strftime('%s', created_at) as integer) asc;
        "#;
    let statement = match tx.statements.entry(sql.as_ptr()) {
        Occupied(entry) => entry.into_mut(),
        Vacant(vacancy) => vacancy.insert(tx.connection.prepare(sql)?),
    };
    statement.reset()?;
    let decode_row = |statement: &Statement| Ok(TrackRating {
        id: statement.read(0)?,
        track_id: statement.read(1)?,
        rating: statement.read(2)?,
    });
    let result = Iter { statement, decode_row };
    Ok(result)
}

// A useless main function, included only to make the example compile with
// Cargo’s default settings for examples.
#[allow(dead_code)]
fn main() {
    let raw_connection = sqlite::open(":memory:").unwrap();
    let mut connection = Connection::new(&raw_connection);

    let tx = connection.begin().unwrap();
    tx.rollback().unwrap();

    let tx = connection.begin().unwrap();
    tx.commit().unwrap();
}
