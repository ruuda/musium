# Tagging

Musium reads metadata from flac tags (also called Vorbis comments) and maps
those to its internal schema. Musium expects files to be tagged properly, and it
expects the tags to be consistent across files.

Because Musium uses internal ids based on MusicBrainz ids of albums and artists,
MusicBrainz ids are required.

Files tagged with [MusicBrainz Picard][picard] should be fine.

[picard]: https://picard.musicbrainz.org/

## Schema

Musium follows a tree-like data model. The library is a collection of artists.
Every artist has one or more albums, and every album has one or more
tracks. The track artist can differ from the album artist (for example, to
accomodate feat. artists), but an album belongs to exactly one artist.

For artists, albums, and tracks, Musium stores the following attributes:

### Artist

 * Name
 * Sort name

### Album

 * Title
 * Original release date

### Track

 * Disc number
 * Track number
 * Title
 * Artist
 * Duration

## Tags

Musium reads metadata from the following tags. Unless specified otherwise,
all tags are mandatory.

 * `discnumber`: Disc number, a non-negative integer less than 256.
   Defaults to 1 if not provided.
 * `tracknumber`: Track number, a non-negative integer less than 256.
 * `title`: Track title.
 * `artist`: Track artist.
 * `album`: Title of the album.
 * `albumartist`: Name of the album artist.
 * `albumartistsort`: Sort name of the album artist (e.g. name without articles).
 * `originaldate`: Original release date of the album in <abbr>YYYY-MM-DD</abbr> format.
   If an exact date is not known, <abbr>YYYY-MM</abbr> and <abbr>YYYY</abbr> can
   be used instead.
 * `date`: If `originaldate` is not provided, this field is used instead.
 * `musicbrainz_albumartistid`: MusicBrainz id to group albums under.
 * `musicbrainz_albumid`: MusicBrainz id to group tracks under.

Note that duration is not read from the metadata. It is determined from the flac
header instead.

## Consistency

Tags contain redundant information, which must be consistent. For example, all
tracks on the same album should have the same album artist and album title.

Musium uses the MusicBrainz album id, to determine what album a track belongs to,
and the MusicBrainz album artist id to determine which artist an album belongs
to. If there is an inconsistency, Musium reports it, and it will then make an
arbitrary choice about what version to keep.
