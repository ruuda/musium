# Rating

Musium can store user ratings per track in the library. Musium supports the
following levels:

<dl>
<dt><strong>Dislike</strong></dt>
<dd>For tracks that you would probably skip,
if they came up in a randomly generated playlist.</dd>

<dt><strong>Neutral</strong></dt>
<dd>This is the default level for unrated tracks.</dd>

<dt><strong>Like</strong></dt>
<dd>This track stands out as a good track on the album.</dd>

<dt><strong>Love</strong></dt>
<dd>This track is among the best tracks in the entire library.</dd>
</dl>

## Storage

Ratings are saved to [the database](configuration.md#db_path) as a numeric
rating level ranging from -1 (dislike) to 2 (love).

## Background

Rating music on a five-point scale is difficult. Even for a single person, using
the available range consistently is hard. Musium is targeted at curated music
libraries, so the fact that an album is present in the library already indicates
that the album contains tracks worth listening to. A five-point scale spends too
much resolution on the low end of the scale.

There is something to say for a simple binary “love” or “like” status, like on
Last.fm and many music players. Further nuance than this can depend a lot on the
situation and context, playlists are probably a better way for finer-grained
classification. However, I do think it is worth distinguishing between “that one
nice track on this album” and “this is one of the best tracks in the library”,
hence the two levels.
