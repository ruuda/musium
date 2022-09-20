-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2022 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

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
