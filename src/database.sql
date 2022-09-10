-- Musium -- Music playback daemon with web-based library browser
-- Copyright 2022 Ruud van Asseldonk
--
-- Licensed under the Apache License, Version 2.0 (the "License");
-- you may not use this file except in compliance with the License.
-- A copy of the License has been included in the root of the repository.

-- @query iter_file_metadata() ->* FileMetadata
SELECT
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
FROM
  file_metadata
ORDER BY
  filename ASC;

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
