-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2022 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

-- #begin create_schema()
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

create unique index if not exists ix_listens_unique_second
on listens (cast(strftime('%s', started_at) as integer));

create table if not exists files
-- First an id, and properties about the file, but not its contents.
-- We can use this to see if a file needs to be re-scanned. The mtime
-- is the raw time_t value returned by 'stat'.
( id                         integer primary key
, filename                   string  not null unique
, mtime                      integer not null
-- ISO-8601 timestamp at which we added the file.
, imported_at                string not null
-- The next columns come from the streaminfo block.
, streaminfo_channels        integer not null
, streaminfo_bits_per_sample integer not null
, streaminfo_num_samples     integer null
, streaminfo_sample_rate_hz  integer not null
);

create table if not exists file_tags
( id      integer primary key
, file_id integer not null references files (id)
, tag     string not null
, value   string not null
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

-- Cover art thumbnails, in jpeg.
create table if not exists thumbnails
( album_id integer primary key
, data     blob not null
);

-- #end create_schema

-- @query insert_listen(listen: Listen) -> i64
INSERT INTO listens
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
VALUES
  ( ":started_at: &str"
  , ":queue_id: i64"
  , ":track_id: i64"
  , ":album_id: i64"
  , ":album_artist_id: i64"
  , ":track_title: &str"
  , ":track_artist: &str"
  , ":album_title: &str"
  , ":album_artist: &str"
  , ":duration_seconds: i64"
  , ":track_number: i64"
  , ":disc_number: i64"
  , 'musium'
  )
RETURNING
  id;


-- @query update_listen_completed(
--     listen_id: i64,
--     queue_id: i64,
--     track_id: i64,
--     completed_at: &str,
-- )
UPDATE
  listens
SET
  completed_at = :completed_at
WHERE
  id = :listen_id
  AND queue_id = :queue_id
  AND track_id = :track_id;
  

-- Insert a new file and its streaminfo metadata, return the file id.
-- @query insert_file(file: InsertFile) -> i64
INSERT INTO files
  ( filename
  , mtime
  , imported_at
  , streaminfo_channels
  , streaminfo_bits_per_sample
  , streaminfo_num_samples
  , streaminfo_sample_rate_hz
  )
VALUES
  ( ":filename: &str"
  , ":mtime: i64"
  , ":imported_at: &str"
  , ":streaminfo_num_channels: i64"
  , ":streaminfo_bits_per_sample: i64"
  , ":streaminfo_num_samples: i64"
  , ":streaminfo_sample_rate_hz: i64"
  )
RETURNING
  id;


-- Add a `TAG=VALUE` pair to the given file.
-- @query insert_file_tag(file_id: i64, tag: &str, value: &str)
INSERT INTO file_tags
  (file_id, tag, value)
VALUES
  (:file_id, :tag, :value);


-- @query iter_files_simple() -> Iterator<FileSimple>
SELECT
  id AS "id: i64",
  filename AS "filename: String",
  mtime AS "mtime: i64"
FROM
  files
ORDER BY
  filename ASC;


-- Iterate all files by ascending file name, yield streaminfo and metadata.
--
-- This is to be paired with [`iter_file_tags`] in a merge join style.
-- @query iter_file_streaminfo() -> Iterator<FileStreamInfo>
SELECT
  file_id as "file_id: i64",
  filename as "filename: String",
  streaminfo_channels as "channels: i64",
  streaminfo_bits_per_sample as "bits_per_sample: i64",
  streaminfo_num_samples as "num_samples: i64",
  streaminfo_sample_rate_hz as "sample_rate_hz: i64"
FROM
  files
ORDER BY
  files.filename ASC;


-- Iterate all tags of all files by ascending file name.
--
-- This is to be paired with [`iter_file_tags`] in a merge join style.
-- @query iter_file_tags() -> Iterator<FileTag>
SELECT
  file_tags.file_id as "file_id: i64",
  file_tags.tag_name as "tag_name: String",
  file_tags.tag_value as "tag_value: String",
FROM
  files,
  file_tags
WHERE
  files.id = file_tag.file_id
ORDER BY
  files.filename ASC;
