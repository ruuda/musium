# Tagging

Musium reads metadata from flac tags (also called Vorbis comments) and maps
those to its internal schema. Musium expects files to be tagged properly, and it
expects the tags to be consistent across files.

Because Musium uses internal ids based on MusicBrainz ids of albums and artists,
MusicBrainz ids are required. Files tagged with [MusicBrainz Picard][picard] are
fine in most cases, although for albums credited to multiple artists, additional
configuration might be needed, see the [section about multiple album
artists](#multiple-album-artists) below.

[picard]: https://picard.musicbrainz.org/

## Schema

The Musium library consists of a collection of albums. Every album belongs to
one or more artists, and every artist has one or more albums. Every album has
one or more tracks. The track artist can differ from the album artist (for
example, to accomodate feat. artists).

For artists, albums, and tracks, Musium stores the following attributes:

### Artist

 * Name
 * Sort name

### Album

 * Title
 * Original release date
 * Artist name as credited

### Track

 * Disc number
 * Track number
 * Title
 * Artist
 * Duration

## Tags

Musium reads metadata from the following tags. Unless specified otherwise,
all tags are mandatory and must occur exactly once. Note that duration is not
read from the metadata, it is determined from the flac header instead.

### discnumber

Disc number, a non-negative integer less than 16. Defaults to 1 if not provided.

### tracknumber

Track number, a non-negative integer less than 256.

### title

Track title.

### artist

Track artist.

### album

Title of the album.

### albumartist

Name of the album artist.

### albumartists

Optionally, for a collaboration album, the names of each artist separately. This
tag should then occur multiple times, once per artist. See also [the section on
multiple album artists](#multiple-album-artists) below. When `albumartists` are
provided, `albumartist` should contain the joined name. For example, if
`albumartists` is provided twice, once with value _John Legend_ and once with
_The Roots_, `albumartist` might be set to _John Ledgend & The Roots_.

### albumartistsort

Sort name of the album artist (e.g. name without articles). Defaults to the
value of `albumartist` when not provided. For an album with multiple artists
this tag is ignored, use `albumartistssort` (note the double s) instead for
collaboration albums.

### albumartistssort

Optionally, the sort name of each album artist separately, to match
`albumartists`. Defaults to the values in `albumartists` when not provided.

### originaldate

Original release date of the album in <abbr>YYYY-MM-DD</abbr> format.
If an exact date is not known, <abbr>YYYY-MM</abbr> and <abbr>YYYY</abbr>
can be used instead.

### date

If `originaldate` is not provided, this field is used instead.

### musicbrainz_albumartistid

MusicBrainz id to group albums under. This can occur multiple times, see also
[the section on multiple album artists](#multiple-album-artists) below.

### musicbrainz_albumid

MusicBrainz id to group tracks under.

## Consistency

Tags contain redundant information, which must be consistent. For example, all
tracks on the same album should have the same album artist and album title.

Musium uses the MusicBrainz album id to determine what album a track belongs to,
and the MusicBrainz album artist id to determine which artist an album belongs
to. The directory structure of the files is irrelevant. If there is an
inconsistency, Musium reports it, and it will then make an arbitrary choice
about what version to keep.

## Multiple album artists

Collaboration albums are sometimes credited to multiple artists. (As opposed to
compliation albums, which are usually credited to a single special artist named
_Various Artists_.) There are different ways to represent this with tags:

 1. Treat every combination of existing artists as a new artist, with a unique
    `musicbrainz_albumartistid`.
 2. Credit the album only to a single primary artist.
 3. Credit the album to multiple artists, with multiple
    `musicbrainz_albumartistid` tags, and multiple `albumartists` tags in
    addition to the standard `albumartist` tag.

Option **1** and **2** are more widely supported across music players, but their
downside is that the album will not show up in the discography of every credited
artist. For example, if you tag [_Wake Up!_][wakeup] by _John Legend & The Roots_
with only _John Legend_ as the album artist, then it will not show up in the
album list of _The Roots_, and if you tag it with _John Legend & The Roots_ as
album artist, then it will neither show up under _John Legend_ nor under _The
Roots_. If the track artist of individual tracks lists all artists, Musium is
still able to find those tracks individually in search by searching for any of
the artists.

Before Musium version **TODO**, albums in Musium belonged to exactly one artist,
so option **1** and **2** were the only ways of tagging a collaboration album.
Since Musium **TODO**, Musium supports multiple artists per album, and option
**3** is possible. However, this requires an additional tag that Picard does not
write by default. Musium needs:

 * The name of the collaboration that the album is credited to in the
   `albumartist` tag. This tag should occur exactly once.
 * The MusicBrainz ids of the individual artists in the
   `musicbrainz_albumartistid` tag. This tag should occur multiple times, once
   per artist.
 * The names of the individual artists in the `albumartists` tag. This tag
   should occur multiple times, once per artist, and in the same order as the
   ids.
 * Optionally, `albumartistssort`. If this tag is present at all, it must occur
   as many times as `albumartists`, and list artists in the same order.

To make Picard write the `albumartists` tags you need the [Additional Artist
Variables plugin][plugin], which is distributed with Picard, but not enabled by
default. In the Picard settings:

 * Enable _Additional Artist Variables_ under _Plugins_.
 * In _Scripting_, create a new tagger script.
 * Add the following expression:

```
$setmulti(albumartists,%_artists_album_all_std_multi%)
$setmulti(albumartistssort,%_artists_album_all_sort_multi%)
```

This should be sufficient to make Musium handle the collaboration albums.

Note, in some cases it is not so clear what constitutes a collaboration. For
example, after releasing multiple albums together, the members of drum ’n bass
duo [_Fred V & Grafix_][fvng] each released solo albums under the names [_Fred
V_][fv] and [_Grafix_][gfx], and their earlier albums are not considered
collaborations (at least in MusicBrainz), _Fred V & Grafix_ is its own separate
artist. On the other hand, drum ’n bass artists [_Sub Focus_][sf] and
[_Wilkinson_][wn] did release [a collaboration album][portals] that is credited
to both of the existing artists, the collaboration is not considered a new
artist. When artists collaborate under a new name (for example,
[_Kaskade_][kaskade] and [_Deadmau5_][mau5] as [Kx5][kx5]), this is generally
represented as a new artist, instead of a collaboration. The line can be blurry,
it’s up to you how you prefer to tag your files.

[wakeup]:  https://musicbrainz.org/release-group/563d758b-aa16-4e35-8986-6d402ea3cef8
[fvng]:    https://musicbrainz.org/artist/d01d66ae-be95-42c3-86c4-fff502690a33
[fv]:      https://musicbrainz.org/artist/d5259aab-5a5d-42a8-a0b8-fb1bcd6b7ac9
[gfx]:     https://musicbrainz.org/artist/c02a5966-b5c4-483a-8326-483edcf3680e
[sf]:      https://musicbrainz.org/artist/8cf49f40-b8fe-4a63-b4ea-f922d6145bb4
[wn]:      https://musicbrainz.org/artist/c9fa114f-8426-4286-a289-9f16c8e092b5
[portals]: https://musicbrainz.org/release-group/7ffd3bc6-a53c-4e7d-bf87-98e5738c1e48
[kaskade]: https://musicbrainz.org/artist/29ed4a49-fb99-4a5c-8713-609cabe6f34a
[mau5]:    https://musicbrainz.org/artist/4a00ec9d-c635-463a-8cd4-eb61725f0c60
[kx5]:     https://musicbrainz.org/artist/9c57432d-484f-4181-b73d-f78dbb7a63be
[plugin]:  https://github.com/rdswift/picard-plugins/blob/430ecb4cfdf77c97a463a2a69b2e7d690ff8d282/plugins/additional_artists_variables/docs/README.md
