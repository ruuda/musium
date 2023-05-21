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

create unique index if not exists ix_ratings_unique_second
on ratings (cast(strftime('%s', created_at) as integer));

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

create table if not exists tags
( id         integer primary key
, file_id    integer not null references files (id) on delete cascade
, field_name string  not null
, value      string  not null
);

create index if not exists ix_tags_file_id on tags (file_id);

-- BS1770.4 integrated loudness over the track, in LUFS.
create table if not exists track_loudness
( track_id              integer primary key
, file_id               integer not null references files (id) on delete cascade
, bs17704_loudness_lufs real    not null
);

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

-- "Waveform" data per track, used to render waveforms in the UI.
-- See waveform.rs for the data format.
create table if not exists waveforms
( track_id integer primary key
, file_id  integer not null references files (id) on delete cascade
, data     blob    not null
);

create table if not exists thumbnails
( album_id integer primary key
, file_id  integer not null references files (id) on delete cascade
, data     blob    not null
);
-- @end ensure_schema_exists

-- @query insert_file(metadata: InsertFile) ->1 i64
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
( :filename                   -- :str
, :mtime                      -- :i64
, :imported_at                -- :str
, :streaminfo_channels        -- :i64
, :streaminfo_bits_per_sample -- :i64
, :streaminfo_num_samples     -- :i64?
, :streaminfo_sample_rate     -- :i64
)
returning id;

-- @query insert_tag(file_id: i64, field_name: str, value: str)
insert into
  tags (file_id, field_name, value)
  values (:file_id, :field_name, :value);

-- Delete a file and everything referencing it (cascade to tags, waveforms, etc.)
--
-- Note that album loudness is not deleted, it is not based on any single file.
-- @query delete_file(file_id: i64)
delete from files where id = :file_id;

-- @query iter_file_mtime() ->* FileMetadataSimple
select
    id       -- :i64
  , filename -- :str
  , mtime    -- :i64
from
  files
order by
  filename asc;

-- @query iter_files() ->* FileMetadata
select
    id                         -- :i64
  , filename                   -- :str
  , mtime                      -- :i64
  , streaminfo_channels        -- :i64
  , streaminfo_bits_per_sample -- :i64
  , streaminfo_num_samples     -- :i64?
  , streaminfo_sample_rate     -- :i64
from
  files
order by
  filename asc;

-- Iterate all `(field_name, value)` pairs for the given file.
-- @query iter_file_tags(file_id: i64) ->* (str, str)
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

-- @query insert_album_thumbnail(album_id: i64, file_id: i64, data: bytes)
insert into thumbnails (album_id, file_id, data)
values (:album_id, :file_id, :data)
on conflict (album_id) do update set data = :data;

-- @query insert_album_loudness(album_id: i64, file_id: i64, loudness: f64)
insert into album_loudness (album_id, file_id, bs17704_loudness_lufs)
values (:album_id, :file_id, :loudness)
on conflict (album_id) do update set bs17704_loudness_lufs = :loudness;

-- @query insert_track_loudness(track_id: i64, file_id: i64, loudness: f64)
insert into track_loudness (track_id, file_id, bs17704_loudness_lufs)
values (:track_id, :file_id, :loudness)
on conflict (track_id) do update set bs17704_loudness_lufs = :loudness;

-- @query insert_track_waveform(track_id: i64, file_id: i64, data: bytes)
insert into waveforms (track_id, file_id, data)
values (:track_id, :file_id, :data)
on conflict (track_id) do update set data = :data;

-- @query insert_listen_started(listen: Listen) ->1 i64
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
  ( :started_at       -- :str
  , :file_id          -- :i64
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

-- Return whether a thumbnail for the album exists (1 if it does, 0 otherwise).
-- @query select_thumbnail_exists(album_id: i64) ->1 i64
select count(*) from thumbnails where album_id = :album_id;

-- For every album, return the earliest listen in the listens table.
--
-- Yields tuples `(album_id, started_at_iso8601)`.
-- @query iter_album_first_listens() ->* (i64, str)
select
  -- We rely on the fact here that asciibetical sorting of ISO-8601 strings
  -- with the same time zone offset is also chronological, and our listens all
  -- have Z suffix (+00 UTC offset).
  album_id, min(started_at)
from
  listens
group by
  album_id;

-- Iterate the listens in chronological order.
-- @query iter_listens() ->* ListenAt
select
    track_id /* :i64 */,
    -- Note that we have an index on this expression, so this should be just an
    -- index scan.
    cast(strftime('%s', started_at) as integer) as started_at_second /* :i64 */
from
    listens
where
    completed_at is not null
order by
    started_at_second asc;

-- Insert a rating for a given track.
--
-- When the `created_at` timestamp is not unique, this replaces the previous
-- rating that was present for that timestamp. This might happen when the user
-- edits the rating in quick succession; then we only store the last write.
-- @query insert_or_replace_rating(track_id: i64, created_at: str, rating: i64)
insert or replace into
  ratings (track_id, created_at, rating, source)
values
  (:track_id, :created_at, :rating, 'musium');

-- Backfill a rating for a given track.
--
-- The timestamp must be unique on the second.
-- @query insert_rating(track_id: i64, created_at: str, rating: i64, source: str)
insert into
  ratings (track_id, created_at, rating, source)
values
  (:track_id, :created_at, :rating, :source);

-- @query iter_ratings() ->* TrackRating
select
    id       -- :i64
  , track_id -- :i64
  , rating   -- :i64
from
  ratings
order by
  -- Order by ascending creation time to ensure we can clamp to rating ranges,
  -- should we need to. We have an index on this expression.
  cast(strftime('%s', created_at) as integer) asc;
