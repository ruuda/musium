# API

Endpoints:

 * `GET  /api/track/:track_id.flac`: Return the track itself.
 * `GET  /api/album/:album_id`:      Return json album metadata.
 * `GET  /api/albums`:               Return a json list of all albums.
 * `GET  /api/artist/:artist_id`:    Return a json object with artist details, and albums in chronological order.
 * `GET  /api/cover/:album_id`:      Return cover art in original resolution.
 * `GET  /api/thumb/:album_id`:      Return downsampled cover art.
 * `GET  /api/search?q=`:            Return json search results.
 * `GET  /api/queue`:                Return the current play queue.
 * `PUT  /api/queue/:track_id`:      Enqueue the track with the given id.
 * `GET  /api/volume`:               Return the current volume.
 * `POST /api/volume/up`:            Increase the volume by 1 dB.
 * `POST /api/volume/down`:          Decrease the volume by 1 dB.
