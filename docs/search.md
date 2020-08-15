# Search

Each of the three Musium data types (artists, albums, and tracks) can be
searched from a single search box, that searches all three simultaneously.
To facilitate search, Musium maintains indexes from words to artists, albums,
and tracks. Indexes are sorted on normalized word, so we can locate the words
with a given prefix in logarithmic time.

## Single search

Musium has a one search box. It should be able to find what you need from a
single query, without the need to select what you are searching for, and without
the need for separate browsers for artists, albums, and tracks, with independent
search functions.

## Minimal results

Search should find everything that is relevant, but nothing more. For example,
when searching for “queen”, we should show the artist Queen, but not every
individual track by that artist, otherwise “Dancing Queen” by Abba would get
lost in that noise. Similarly, a search for an artist should not list all albums
by that artist, unless they are relevant results by themselves. This happens for
self-titled albums, but also for the word “who” in “Who’s Next” by The Who, for
instance.

Consider a search for “abba dancing queen”.

 * Suppose we index the track artist in addition to the title, then we would
   find “Dancing Queen” by Abba with this query. But we would also find it for
   the prefix “abba”, which is undesirable.
 * Suppose we do not index the track artist, then we would not find the track at
   all, because the word “abba” does not occur in the title.

On the other hand, if the track artist differs from the album artist (maybe
because it includes a feat. artist, because the track is part of a compilation
album), then we cannot reach that track through the artist search results, so
then we do need to include the track itself, to make it discoverable.

Similarly, a seac

Conclusion:

 * Words that occur in the track artist and also in the album artist, should not
   make the track show up, because we can also reach the track through the
   artist search result.
 * If we consider the track for a different reason, then the presence of a word
   that does not occur in the track title, but which does occur in the track
   artist, should not cause the track to be excluded.
 * Words that occur in the track artist, but not in the album artist, should
   make the track show up.

## Search combinations

We want to support the following queries:

 * Track title only, e.g. “dancing queen”
 * Album title only, e.g. “arrival”
 * Album artist only, e.g. “abba”
 * Track title and artist, e.g. “abba dancing queen”
 * Album title and artist, e.g. “abba arrival”

The following queries are out of scope:

 * Track title and album, e.g. “arrival dancing queen”
 * Artist, album, and track, e.g. “abba arrival dancing queen”

## Indexes

Based on the above considerations, we need the following indexes:

 * Album artist words.
 * Album title + album artist words, with a marker to tell whether the entry is
   for the album title or album artist.
 * Track title + track artist words, with a marker to tell whether the entry is
   for the track title or artist, and if it is for the artist, a marker to tell
   whether the word occurs in the album artist too.
