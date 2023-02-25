-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2022 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

-- @begin ensure_schema_exists()
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

-- Next is the table with tag data. This is the raw data extracted from
-- Vorbis comments; it is not indexed, so it is not guaranteed to be
-- sensible. We store the raw data and index it when we load it, because
-- indexing itself is pretty fast; it's disk access to the first few bytes
-- of tens of thousands of files what makes indexing slow.
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

-- BS1770.4 integrated loudness over the track, in LUFS.
create table if not exists track_loudness
( track_id              integer primary key
, bs17704_loudness_lufs real not null
);

-- BS1770.4 integrated loudness over the album, in LUFS.
create table if not exists album_loudness
( album_id              integer primary key
, bs17704_loudness_lufs real not null
);

-- "Waveform" data per track, used to render waveforms in the UI.
-- See waveform.rs for the data format.
create table if not exists waveforms
( track_id integer primary key
, data     blob not null
);

create table if not exists thumbnails
( album_id integer primary key
  -- TODO: Would like to reference the files table too, so we can invalidate
  -- when needed.
, data     blob not null
);
-- @end ensure_schema_exists

-- @query insert_file_metadata(metadata: InsertFileMetadata)
insert into
  file_metadata
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
  ( :filename                      -- :str
  , :mtime                         -- :i64
  , :imported_at                   -- :str
  , :streaminfo_channels           -- :i64
  , :streaminfo_bits_per_sample    -- :i64
  , :streaminfo_num_samples        -- :i64?
  , :streaminfo_sample_rate        -- :i64
  , :tag_album                     -- :str?
  , :tag_albumartist               -- :str?
  , :tag_albumartistsort           -- :str?
  , :tag_artist                    -- :str?
  , :tag_musicbrainz_albumartistid -- :str?
  , :tag_musicbrainz_albumid       -- :str?
  , :tag_musicbrainz_trackid       -- :str?
  , :tag_discnumber                -- :str?
  , :tag_tracknumber               -- :str?
  , :tag_originaldate              -- :str?
  , :tag_date                      -- :str?
  , :tag_title                     -- :str?
  , :tag_bs17704_track_loudness    -- :str?
  , :tag_bs17704_album_loudness    -- :str?
);

-- @query delete_file_metadata(file_id: i64)
delete from file_metadata where id = :file_id;

-- @query iter_file_metadata() ->* FileMetadata
select
  filename                      /* :str  */,
  mtime                         /* :i64  */,
  streaminfo_channels           /* :i64  */,
  streaminfo_bits_per_sample    /* :i64  */,
  streaminfo_num_samples        /* :i64? */,
  streaminfo_sample_rate        /* :i64  */,
  tag_album                     /* :str? */,
  tag_albumartist               /* :str? */,
  tag_albumartistsort           /* :str? */,
  tag_artist                    /* :str? */,
  tag_musicbrainz_albumartistid /* :str? */,
  tag_musicbrainz_albumid       /* :str? */,
  tag_discnumber                /* :str? */,
  tag_tracknumber               /* :str? */,
  tag_originaldate              /* :str? */,
  tag_date                      /* :str? */,
  tag_title                     /* :str? */,
  tag_bs17704_track_loudness    /* :str? */,
  tag_bs17704_album_loudness    /* :str? */
from
  file_metadata
order by
  filename asc;

-- @query iter_file_metadata_mtime() ->* FileMetadataSimple
select
    id       -- :i64
  , filename -- :str
  , mtime    -- :i64
from
  file_metadata
order by
  filename asc;

-- @query insert_album_thumbnail(album_id: i64, data: bytes)
INSERT INTO thumbnails (album_id, data)
VALUES (:album_id, :data)
ON CONFLICT (album_id) DO UPDATE SET data = :data;

-- @query insert_album_loudness(album_id: i64, loudness: f64)
INSERT INTO album_loudness (album_id, bs17704_loudness_lufs)
VALUES (:album_id, :loudness)
ON CONFLICT (album_id) DO UPDATE SET bs17704_loudness_lufs = :loudness;

-- @query insert_track_loudness(track_id: i64, loudness: f64)
INSERT INTO track_loudness (track_id, bs17704_loudness_lufs)
VALUES (:track_id, :loudness)
ON CONFLICT (track_id) DO UPDATE SET bs17704_loudness_lufs = :loudness;

-- @query insert_track_waveform(track_id: i64, data: bytes)
INSERT INTO waveforms (track_id, data)
VALUES (:track_id, :data)
ON CONFLICT (track_id) DO UPDATE SET data = :data;

-- @query insert_listen_started(listen: Listen) ->1 i64
insert into
  listens
  ( started_at
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
  ( :started_at       -- :str
  , :queue_id         -- :i64
  , :track_id         -- :i64
  , :album_id         -- :i64
  , :album_artist_id  -- :i64
  , :track_title      -- :str
  , :track_artist     -- :str
  , :album_title      -- :str
  , :album_artist     -- :str
  , :duration_seconds -- :i64
  , :track_number     -- :i64
  , :disc_number      -- :i64
  , 'musium'
  )
returning
  id;

-- @query update_listen_completed(
--   listen_id: i64,
--   queue_id: i64,
--   track_id: i64,
--   completed_at: str,
-- )
update listens
  set completed_at = :completed_at
where
  id = :listen_id
  and queue_id = :queue_id
  and track_id = :track_id;

-- @query select_album_loudness_lufs(album_id: i64) ->? f64
select bs17704_loudness_lufs from album_loudness where album_id = :album_id;

-- @query select_track_loudness_lufs(track_id: i64) ->? f64
select bs17704_loudness_lufs from track_loudness where track_id = :track_id;

-- @query select_track_waveform(track_id: i64) ->? bytes
select data from waveforms where track_id = :track_id;

-- Return the sum of the sizes (in bytes) of all thumbnails.
-- @query select_thumbnails_count_and_total_size() ->1 (i64, i64)
select count(*), sum(length(data)) from thumbnails;

-- @query iter_thumbnails() ->* Thumbnail
select album_id /*: i64 */, data /* :bytes */ from thumbnails;
