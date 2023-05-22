# Playcounts

Musium can compute playcounts from the listens table in its database. With this
we can rank the most popular artists, albums, and tracks over various time
windows. However, naively counting the number of listens in a particular time
window (what Last.fm does for example) leads to a popularity metric that doesn’t
quite reflect our perception of popularity. This document motivates an
alternative counting method used by Musium.

## Exponential decay

Playcounts can be interesting on different time scales. For example, just
because you listened to a _lot_ of emo music as a teenager doesn’t mean that
this should continue to dominate your top artists forever. We can also compare
the rank of artists on different time scales to identify new risers or forgotton
classics.

One way to achieve this is to count all listens that fall in a given time
window. There are two issues with this approach:

 * Popularity over time can vary abrubtly. If I listen to an artist on one day,
   it remains among the top of my 30d most listened artists, until it falls out
   of the range and suddenly has popularity zero.
 * It is challenging to compute incrementally with a single pass over the data.

An alternative is to count listens with exponential decay, with different
half-lives for different time scales. For a 30d half-life, if I listened to an
artist one month ago, the count evaluated now would be half of what it was that
day. In three months (four months after listening), the count would be 6% of the
original count. It no longer contributes significantly, but popularity also does
not drop to zero abrubtly.

Exponential decay can be implemented efficiently and updated on the fly
when new listens come in.

## Leaky bucket rate limiting

For track playcounts, counting every listen with a weight of 1.0 is appropriate.
But for albums and artists, counting every time we listen to a track with a
weight of 1.0 leads to a skewed sense of popularity:

 * If you listen to both _The Dark Side of the Moon_ and _Wish You Were Here_
   by Pink Floyd in full, then the former would seem twice as popular as the
   latter, because it has 10 tracks and the latter only 5, even though you
   listened to both albums once.
 * If on the other hand we counted by listening time, then _Wish You Were Here_
   would seem slightly more popular, because its running time is longer than
   _The Dark Side of the Moon_.
 * If in a given week you listen to those two albums by Pink Floyd on one day,
   but you also listen to Billie Eilish’ new single every day, then Pink Floyd
   would seem more than twice as popular as Billie Eilish, with 15 listens vs. 7.
   In one sense this is appropriate, you spent more time listening to Pink Floyd
   than to Billie Eilish. But on the other hand, Billie Eilish is a recurring
   obsession that you listened to all week, while you only listened to Pink
   Floyd on one day.

So while total time spent listening (for which track playcounts are a reasonable
approximation) is an important aspect of popularity, it doesn’t reflect well
_how often_ you listen to that artist or album. We need something closer to
“number of days on which we listened to …”.

To strike a balance, we can apply rate limiting to album and artist playcounts.
If you haven’t listened to the artist for some time, the next play counts as a
full play, but if you are continuously listening, this is part of the same
session, and new listens in the same session should not get full weight. One way
to implement this rate limiting is with a leaky bucket. Every play consumes a
count of 1.0 from the bucket, but if less is available in the bucket, then we
count less. The bucket fills back up at a time scale that is long enough to not
double-count the same session, but short compared to the exponential decay
half-life. For example, we might give albums an initial capacity of 2.0, and
refill to 2.0 in 8 hours. This way, if you listen a full album in one session,
whether it has only a few tracks or dozens, either would count as roughly 2.0.
But if you listen to the same album 8 hours later, it would count as another
full listen. Taking an initial capacity of 2.0 ensures that when we listen to
multiple tracks — really listening to the _album_, in some sense — the album is
counted with more weight than when we happen to play that album because we were
playing singles, one track from many different albums.
