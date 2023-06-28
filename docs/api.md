# API

The Musium daemon can be controlled with a <abbr>REST</abbr> <abbr>API</abbr>.
This <abbr>API</abbr> is used by the webinterface. Because Musium is a work in
progress, no stability promise is made about the <abbr>API</abbr>, and the
<abbr>API</abbr> is not versioned.

This page gives an overview of the endpoints that exist, it is not full
reference-level material. The easiest way to learn more is to query the
<abbr>API</abbr> with Curl.

## Library

### `GET` /api/track/:track_id.flac
Return the track itself, as a flac file.

### `GET` /api/album/:album_id
Return json album metadata.

### `GET` /api/albums
Return a json list of all albums, ordered by album id.

### `GET` /api/artist/:artist_id
Return a json object with artist details, and albums in chronological order.

### `GET` /api/cover/:album_id
Return cover art in original resolution.

### `GET` /api/thumb/:album_id
Return downsampled cover art.

### `GET` /api/search?q=:query
Return json search results.

### `GET` /api/stats
Return json library statistics.

## Queue

### `GET` /api/queue
Return the current play queue. The track at the front of the queue is the
currently playing track, and it includes information about the playback
position.

### `PUT` /api/queue/:track_id
Enqueue the track with the given id.

### `POST` /api/queue/shuffle
Shuffle the queue. Returns the new queue.

## Volume

### `GET` /api/volume
Return the current volume.

### `POST` /api/volume/up
Increase the volume by 1 dB. Returns the new volume.

### `POST` /api/volume/down
Decrease the volume by 1 dB. Returns the new volume.

## Scanning

### `GET` /api/scan/status
Return the status of the current scan as a json object. Returns `null` if no
scan has ever been started.

### `POST` /api/scan/start
Start a scan of the library directory. If a scan is already in progress, this is
a no-op. Returns the status of the scan.
