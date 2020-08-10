# API

Endpoints:

 * `GET /track/:track_id.flac`: Return the track itself.
 * `GET /album/:album_id`:      Return json album metadata.
 * `GET /albums`:               Return a json list of all albums.
 * `GET /cover/:album_id`:      Return cover art in original resolution.
 * `GET /thumb/:album_id`:      Return downsampled cover art.
 * `GET /search?q=`:            Return json search results.
 * `PUT /queue/:track_id`:      Enqueue the track with the given id.
