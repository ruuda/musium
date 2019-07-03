# API

## Album list

    GET /albums

Returns a json list of all albums.

### Schema

Schema of elements:

|||
|---|----|
| **id**          | Album id. |
| **title**       | Album title. |
| **artist**      | Album artist. |
| **sort_artist** | Artist name for sorting. The sort name has been lowercased, and some non-<abbr>ASCII</abbr> characters have been replaced. |
| **date**        | Original released date in <abbr>YYYY-MM-DD</abbr> format. The date may be truncated to either <abbr>YYYY</abbr> or <abbr>YYYY-MM</abbr> if an exact date is unknown. |

### Example

    [
      {
        "id": "20bb8ce215e62df3",
        "title": "Disintegration",
        "artist": "The Cure",
        "sort_artist": "cure the",
        "date": "1989-05-01"
      },
      {
        "id": "ca6f753b8902a05a",
        "title": "Seventeen Seconds",
        "artist": "The Cure",
        "sort_artist": "cure the",
        "date": "1980-04-22"
      },
    ]

## Album details

    GET /album/:album_id

Return a json object with album metadata and the track list. Schema:

 * All of the keys also present in the [album list][#album-list].
 * `tracks: Array Track`: Tracks on the album, sorted by disc number and then by
   track number.


Endpoints:

 * `GET  /track/:track_id.flac`: Return the track itself.
 * `GET  /album/:album_id`:      Return json album metadata.
 * `GET  /albums`:               Return a json list of all albums.
 * `GET  /cover/:album_id`:      Return cover art in original resolution.
 * `GET  /thumb/:album_id`:      Return downsampled cover art.
 * `GET  /search?q=`:            Return json search results.
 * `GET  /queue`:                Return the current play queue.
 * `PUT  /queue/:track_id`:      Enqueue the track with the given id.
 * `GET  /volume`:               Return the current volume.
 * `POST /volume/up`:            Increase the volume by 1 dB.
 * `POST /volume/down`:          Decrease the volume by 1 dB.
