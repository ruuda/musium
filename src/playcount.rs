// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of playcounts and other statistics.

use std::collections::BinaryHeap;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::database::{self, Transaction};
use crate::database_utils::connect_readonly;
use crate::prim::{AlbumId, ArtistId, TrackId};
use crate::{MemoryMetaIndex, MetaIndex};

/// A point in time with second granularity.
///
/// This is like a traditional POSIX timestamp, with two differences:
///
/// * The epoch is 2000-01-01 rather than 1970-01-01.
/// * The value is unsigned rather than signed.
///
/// The timestamp is 32 bits, because we need lots of them (one per track at
/// least), so the memory savings add up.
///
/// Together, this means that listens before 2000-01-01 cannot be represented,
/// but the upside is that we sidestep the Y2K38 problem by extending the range
/// from ~70 to ~140 years and shifting it by 30, for a range from 2000-01-01 up
/// to 2136-02-07. I will not be alive long enough for this to become a problem,
/// and Audioscrobbler/Last.fm only exists since 2002 it’s unlikely that older
/// listens even exist.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Instant {
    /// Nominal seconds since 2000-01-01 00:00 UTC.
    ///
    /// Like in POSIX time, leap seconds are ignored, so this is just the
    /// POSIX time but with an offset, so the epoch is not the traditional
    /// Unix epoch of 1970-01-01.
    pub seconds_since_jan_2000: u32,
}

/// We divide time in “epochs” of 4 hours and 33 minutes.
///
/// When applying exponential decay, if the elapsed time is very short, then the
/// decay factor is very close to 1, and when we multiply many times with a
/// number very close to but not quite 1, the result may be different than
/// making one big jump due to accumulation of numerical errors. To avoid this,
/// we only apply the exponential decay periodically, if we have moved at least
/// to the next “epoch”.
///
/// The duration of an epoch is 2<sup>14</sup> = 16384 seconds, such that we can
/// compute the epoch number from a timestamp through an inexpensive bitshift.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Epoch(u32);

/// Measures a non-negative time elapsed in seconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Duration {
    pub seconds: u32,
}

/// Measures a non-negative time elapsed in epochs.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EpochDuration {
    pub epochs: u32,
}

impl Instant {
    #[inline(always)]
    pub fn from_posix_timestamp(timestamp: i64) -> Instant {
        // 2000-01-01T00:00:00Z as a Posix timestamp.
        let jan_2000_posix_seconds = 946684800;
        Instant {
            seconds_since_jan_2000: (timestamp - jan_2000_posix_seconds) as u32,
        }
    }

    /// Return the epoch that this instant falls in (rounds the time down).
    #[inline(always)]
    pub fn epoch(&self) -> Epoch {
        Epoch(self.seconds_since_jan_2000 >> 14)
    }

    /// Return the time elapsed since `t0`, which should be before `self`.
    #[inline(always)]
    pub fn duration_since(&self, t0: Instant) -> Duration {
        Duration {
            seconds: self.seconds_since_jan_2000 - t0.seconds_since_jan_2000,
        }
    }
}

impl Epoch {
    /// Return the time elapsed since `t0`, which should be before `self`.
    #[inline(always)]
    pub fn duration_since(&self, t0: Epoch) -> EpochDuration {
        EpochDuration {
            epochs: self.0 - t0.0,
        }
    }
}

/// Configures how the leaky bucket rate limiter behaves.
///
/// Note, the current amount per bucket is stored in [`ExpCounter`], not in this
/// config stuct.
pub struct RateLimit {
    /// The capacity of the bucket, also called the “burst” amount.
    pub capacity: f32,

    /// The rate at which the bucket refills until it reaches `capacity` again.
    pub fill_rate_per_second: f32,
}

/// Exponential moving averages at different timescales plus leaky bucket rate limiter.
pub struct ExpCounter {
    /// Time at which the counts were last updated.
    pub t: Instant,

    /// “Count” left in the bucket for leaky-bucket rate limiting.
    pub bucket: f32,

    /// Exponentially decaying counts for different half-lives.
    pub n: [f32; 5],
}

impl ExpCounter {
    /// Half-lives for which we keep a moving average.
    ///
    /// ## Spacing of the half-lives
    ///
    /// The half lives quadruple every time (from short to long). This provides
    /// a nice logarithmic spacing on the “long half-life” to “short half-life”
    /// spectrum, and most values work out to align close to a natural interval,
    /// with the lowest bucket being ~14 days, and the next one two months.
    ///
    /// This spacing also enables us to efficiently compute exponential decay
    /// factors from the long-duration one: raise it to the fourth power to get
    /// the decay factor for the next half-life. There is some risk of
    /// accumulating numerical errors here (computing using powf directly is
    /// more precise than repeated multiplication), but even for a timestep of
    /// 4096 seconds (lower than one epoch), the relative error at the shortest
    /// half-life bucket is only 0.0002%.
    ///
    /// ## Definition
    ///
    /// The unit for the half life is the epoch (2<sup>14</sup> seconds), so we
    /// can multiply with the difference of the epochs of the timestamps without
    /// additional scaling factor multiplication (which we would need if we
    /// measured the half-life in seconds).
    ///
    /// Table can be generated with the following program:
    /// ```python
    /// xs = [(10 / 4**i) * (365.25 * 24 * 3600 / 2**14) for i in range(5)]
    /// for x in xs:
    ///     print(f"        {x:.6f}, // {x * 2**14 / (3600 * 24):.0f} days")
    /// ```
    ///
    /// ## Comparing counters
    ///
    /// Due to the exponential decay, suppose we listen once per day for an
    /// infinite time, then the counter with a 10-year half-life would have a
    /// higher count than the counter with 2-week half-life, even though at both
    /// timescales the behavior is the same. To correct for this, we can divide
    /// by the integral of the decay over the time window, which will be greater
    /// for longer half-lives.
    ///
    /// For exponential decay of the form `0.5^(t / half_life)`, the integral
    /// from `t=0` to `t=t_1` is given by
    ///
    ///     half_life / ln(2) * [1 - 0.5^(t_1 / half_life)]
    ///
    /// and if we plug in ∞ for `t_1` then we see the scale factors are just
    /// proportional to the half lives, so we can divide by that. (We care only
    /// about relative counts, so we can skip the `ln(2)` factor.)
    ///
    /// TODO: We can choose for `t_1` the first time at which the album was
    /// seen, then new albums don't have as much of a penalty in the
    /// long-running average.
    const HALF_LIFE_EPOCHS: [f32; 5] = [
        19261.230469, // 10 years   / 3652 days
        4815.307617,  // 2.5 years  / 913 days
        1203.826904,  // 7.5 months / 228 days
        300.956726,   // 2 months   / 57 days
        75.239182,    // 2 weeks    / 14 days
    ];

    /// Return how much to decay the counters by after the elapsed time.
    #[inline]
    pub fn decay_factors(duration: EpochDuration) -> [f32; 5] {
        let dt = duration.epochs as f32;
        Self::HALF_LIFE_EPOCHS.map(|t| 0.5_f32.powf(dt / t))
    }

    pub fn new() -> ExpCounter {
        ExpCounter {
            t: Instant {
                seconds_since_jan_2000: 0,
            },
            // The bucket starts out empty, but the timestamp also starts out
            // very long ago, so by the time we count something, the bucket will
            // have long replenished.
            bucket: 0.0,
            n: [0.0; 5],
        }
    }

    /// Replenish the bucket to its value at time `t1` (but do not update `self.t`).
    #[inline]
    fn refill_bucket(&mut self, rate_limit: &RateLimit, t1: Instant) {
        let elapsed_seconds = t1.duration_since(self.t).seconds as f32;
        self.bucket = rate_limit
            .fill_rate_per_second
            .mul_add(elapsed_seconds, self.bucket)
            .min(rate_limit.capacity);
    }

    /// Advance the time to the given instant.
    ///
    /// This applies decay without incrementing the count.
    #[inline]
    pub fn advance(&mut self, rate_limit: &RateLimit, t1: Instant) {
        debug_assert!(t1 >= self.t, "New time must be later than previous time.");
        self.refill_bucket(rate_limit, t1);

        // Note, we round to epochs first, and then take the diff, to ensure
        // that the decay gets applied at consistent times across all counters.
        let elapsed_epochs = t1.epoch().duration_since(self.t.epoch());
        let decay_factors = Self::decay_factors(elapsed_epochs);

        for (ni, factor) in self.n.iter_mut().zip(decay_factors) {
            *ni *= factor;
        }

        self.t = t1;
    }

    /// Advance the time to the given instant and increment the count.
    #[inline]
    pub fn increment(&mut self, rate_limit: &RateLimit, t1: Instant) {
        debug_assert!(t1 >= self.t, "New time must be later than previous time.");
        self.refill_bucket(rate_limit, t1);

        // Take 1.0 out of the bucket, or as much as we can get if there is not
        // that much “count” left in the bucket.
        let count = self.bucket.min(1.0);
        self.bucket -= count;

        // Apply any decay that has happened since the last update. See also
        // the comment in `advance`.
        let elapsed_epochs = t1.epoch().duration_since(self.t.epoch());
        let decay_factors = Self::decay_factors(elapsed_epochs);

        for (ni, factor) in self.n.iter_mut().zip(decay_factors) {
            *ni = ni.mul_add(factor, count);
        }

        self.t = t1;
    }
}

impl Default for ExpCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Boilerplate to make `BinaryHeap` accept floats.
///
/// While at it, this also reverses the ordering so we get a min-heap instead of
/// a max-heap without needing to manually negate the numbers.
#[derive(PartialEq)]
pub struct RevNotNan(pub f32);

impl Eq for RevNotNan {}

impl PartialOrd for RevNotNan {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0).map(|ord| ord.reverse())
    }
}

impl Ord for RevNotNan {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).expect("Counts must not be NaN.")
    }
}

pub struct PlayCounter {
    /// The timestamp of the last inserted listen.
    last_counted_at: Instant,
    artists: HashMap<ArtistId, ExpCounter>,
    albums: HashMap<AlbumId, ExpCounter>,
    tracks: HashMap<TrackId, ExpCounter>,
}

impl PlayCounter {
    pub fn new() -> PlayCounter {
        PlayCounter {
            last_counted_at: Instant {
                seconds_since_jan_2000: 0,
            },
            artists: HashMap::new(),
            albums: HashMap::new(),
            tracks: HashMap::new(),
        }
    }

    /// For artists, we want some balance between "unique days listened to
    /// this artist" (which would correspond to a capacity of 1 and a fill
    /// rate of 1/day) and "time listened to this artist" (which would
    /// correspond to a capacity of ~1 and a high fill rate). After much
    /// tweaking, I ended up with the following which I think reasonably
    /// matches my feeling for what I listened to vs. what the algorithm
    /// outputs.
    const LIMIT_ARTIST: RateLimit = RateLimit {
        capacity: 3.0,
        fill_rate_per_second: 1.0 / (3600.0 * 8.0),
    };

    /// Similar for albums, give a burst of >1.0 so albums where we listen to
    /// the full album count more than when we just listened one track. But make
    /// the fill rate longer, so we only count every few hours. You can listen
    /// to the album in the morning and the afternoon and it would be counted
    /// more than listening a single time, but listening to half the album, then
    /// some other tracks, and then the other half, would only count as slightly
    /// more than a single session.
    const LIMIT_ALBUM: RateLimit = RateLimit {
        capacity: 2.5,
        fill_rate_per_second: 1.0 / (3600.0 * 6.0),
    };

    /// For tracks we don't want to rate limit, but to keep the code uniform
    /// we have this which is generous enough that it should never trigger.
    const LIMIT_TRACK: RateLimit = RateLimit {
        capacity: 256.0,
        fill_rate_per_second: 1.0,
    };

    pub fn count(&mut self, index: &MemoryMetaIndex, at: Instant, track_id: TrackId) {
        debug_assert!(
            at >= self.last_counted_at,
            "Counts must be done in ascending order."
        );
        let album_id = track_id.album_id();
        let album = match index.get_album(album_id) {
            Some(album) => album,
            // TODO: Report this, so we can try to match on something other than
            // the track id?
            None => return,
        };

        let counter_track = self.tracks.entry(track_id).or_default();
        counter_track.increment(&Self::LIMIT_TRACK, at);

        let counter_album = self.albums.entry(album_id).or_default();
        counter_album.increment(&Self::LIMIT_ALBUM, at);

        for artist_id in index.get_album_artists(album.artist_ids) {
            let counter_artist = self.artists.entry(*artist_id).or_default();
            counter_artist.increment(&Self::LIMIT_ARTIST, at);
        }

        self.last_counted_at = at;
    }

    /// Advance all counters (without incrementing) to time `t`.
    ///
    /// This enables the `n` value of the counters to be directly compared
    /// between different counters.
    pub fn advance_counters(&mut self, t: Instant) {
        for counter in self.artists.values_mut() {
            counter.advance(&Self::LIMIT_ARTIST, t);
        }
        for counter in self.albums.values_mut() {
            counter.advance(&Self::LIMIT_ALBUM, t);
        }
        for counter in self.tracks.values_mut() {
            counter.advance(&Self::LIMIT_TRACK, t);
        }
    }

    /// Advance all counters to the time of the last inserted listen.
    pub fn equalize_counters(&mut self) {
        self.advance_counters(self.last_counted_at);
    }

    /// Return the top `n` elements for the given expression.
    ///
    /// This assumes that all counters are at the same time. If not, the result
    /// is nonsensical. Make sure to call `advance_counters` first.
    ///
    /// As an example, to get the top artists, albums, and tracks by playcount
    /// at a given timescale, use predicate `|counter| counter.n[timescale]`.
    ///
    /// `timescale` is an index into `ExpCounter::n`, lower indexes have higher
    /// half-life (so count long-term trends), while higher indexes have a lower
    /// half-life (so they are more sensitive to recent trends).
    pub fn get_top_by<F>(
        &self,
        n_top: usize,
        mut expr: F,
    ) -> (
        Vec<(RevNotNan, ArtistId)>,
        Vec<(RevNotNan, AlbumId)>,
        Vec<(RevNotNan, TrackId)>,
    )
    where
        F: FnMut(&ExpCounter) -> RevNotNan,
    {
        fn get_top_n<K: Copy + Ord, F: FnMut(&ExpCounter) -> RevNotNan>(
            n_top: usize,
            expr: &mut F,
            counters: &HashMap<K, ExpCounter>,
        ) -> Vec<(RevNotNan, K)> {
            let mut result = BinaryHeap::new();

            for (k, counter) in counters.iter() {
                let count = expr(counter);

                if result.len() < n_top {
                    result.push((count, *k));
                    continue;
                }

                let should_insert = match result.peek() {
                    None => true,
                    Some((other_count, _)) => count.0 > other_count.0,
                };
                if should_insert {
                    result.pop();
                    result.push((count, *k));
                }
            }

            result.into_sorted_vec()
        }

        (
            get_top_n(n_top, &mut expr, &self.artists),
            get_top_n(n_top, &mut expr, &self.albums),
            get_top_n(n_top, &mut expr, &self.tracks),
        )
    }

    /// Traverse all listens in the `listens` table and count them.
    pub fn count_from_database(
        &mut self,
        index: &MemoryMetaIndex,
        tx: &mut Transaction,
    ) -> database::Result<()> {
        for listen_opt in database::iter_listens(tx)? {
            let listen = listen_opt?;
            let at = Instant::from_posix_timestamp(listen.started_at_second);
            let track_id = TrackId(listen.track_id as u64);
            self.count(index, at.into(), track_id);
        }
        Ok(())
    }

    /// Return the _discover_ ranking of the albums.
    ///
    /// The discovery sorting method is a mix of trending albums (by album and
    /// albums that contain trending tracks) and falling albums. See also
    /// [`score_trending`] and [`score_falling`].
    ///
    /// Be sure to call [`equalize_counters`] before calling this to ensure the
    /// counts are comparable.
    ///
    /// Returns the album ids, with higher ranked albums at the start, and lower
    /// ranked albums at the end.
    pub fn get_discover_rank(&self) -> Vec<AlbumId> {
        // For the discovery rank, we interleave the trending and falling
        // entries for albums as well as albums that contain trending/falling
        // tracks. Often they coincide, but sometimes there is one track that
        // stands out on the album, so we take both into account.
        let mut albums: [Vec<(RevNotNan, AlbumId)>; 4] = [
            self.albums
                .iter()
                .map(|(album_id, counter)| (score_trending(counter), *album_id))
                .collect(),
            self.albums
                .iter()
                .map(|(album_id, counter)| (score_falling(counter), *album_id))
                .collect(),
            self.tracks
                .iter()
                .map(|(track_id, counter)| (score_trending(counter), track_id.album_id()))
                .collect(),
            self.tracks
                .iter()
                .map(|(track_id, counter)| (score_falling(counter), track_id.album_id()))
                .collect(),
        ];
        for albums in albums.iter_mut() {
            albums.sort();
        }
        let mut iters = [
            albums[0].iter().rev(),
            albums[1].iter().rev(),
            albums[2].iter().rev(),
            albums[3].iter().rev(),
        ];

        // All the above counters are for all the playcounts, and all tracks are
        // on an album, so the size of the result is equal to the number of
        // albums we counted plays for.
        let mut processed = HashSet::with_capacity(albums[0].len());
        let mut result = Vec::with_capacity(albums[0].len());

        loop {
            let mut made_progress = false;
            for iter in &mut iters {
                match iter.next() {
                    None => continue,
                    Some((_, album_id)) => {
                        made_progress = true;
                        let is_new = processed.insert(album_id);
                        if is_new {
                            result.push(*album_id);
                        }
                    }
                }
            }
            if !made_progress {
                break;
            }
        }

        result
    }
}

fn print_ranking(
    title: &'static str,
    description: String,
    index: &MemoryMetaIndex,
    top_artists: &[(RevNotNan, ArtistId)],
    top_albums: &[(RevNotNan, AlbumId)],
    top_tracks: &[(RevNotNan, TrackId)],
) {
    println!("\n{title} ARTISTS ({description})\n");
    for (i, (count, artist_id)) in top_artists.iter().enumerate() {
        let artist = index.get_artist(*artist_id).unwrap();
        let artist_name = index.get_string(artist.name);

        println!(
            "  {:2} {:7.3} {} {}",
            i + 1,
            count.0,
            artist_id,
            artist_name
        );
    }

    println!("\n{title} ALBUMS ({description})\n");
    for (i, (count, album_id)) in top_albums.iter().enumerate() {
        let album = index.get_album(*album_id).unwrap();
        let album_title = index.get_string(album.title);
        let album_artist = index.get_string(album.artist);

        println!(
            "  {:2} {:7.3} {} {:25}  {}",
            i + 1,
            count.0,
            album_id,
            album_title,
            album_artist
        );
    }

    println!("\n{title} TRACKS ({description})\n");
    for (i, (count, track_id)) in top_tracks.iter().enumerate() {
        let track = index.get_track(*track_id).unwrap();
        let track_title = index.get_string(track.title);
        let track_artist = index.get_string(track.artist);

        println!(
            "  {:2} {:7.3} {} {:25}  {}",
            i + 1,
            count.0,
            track_id,
            track_title,
            track_artist
        );
    }
}

/// Score for sorting entries by _trending_.
///
/// Trending entries (tracks, albums, artists) are entries where the recent
/// playcount is high compared to the long-term playcount.
fn score_trending(counter: &ExpCounter) -> RevNotNan {
    RevNotNan(3.0 * counter.n[4] / (counter.n[3] + counter.n[2] + counter.n[1]))
}

/// Score for sorting entries by _falling_.
///
/// Falling entries (tracks, albums, artists) are entries that have a high
/// playcount on a long-term timescale, but low playcount recently.
fn score_falling(counter: &ExpCounter) -> RevNotNan {
    // Falling at timescale 1 (2.5 years) vs. 3 (2 months).
    // We apply a logarithm to avoid having extremely large counts,
    // it doesn't affect the ranking (though we add a term below,
    // and adding the logarithm is equal to multiplying the ratios,
    // so in a sense, we take falling entries on *both* timescales).
    let fall_t1 = (counter.n[1] / counter.n[3]).ln();

    // Falling at timescale 2 (7.5 months) vs. 4 (14 days).
    let fall_t2 = (counter.n[2] / counter.n[4]).ln();

    // The log values of t2 tend to be 4× higher than those of t1, so multiply
    // by 1/4 to put them on a comparable scale.
    RevNotNan(fall_t2.mul_add(0.25, fall_t1))
}

/// Print playcount statistics about the library.
///
/// This is mostly for debugging and development purposes, playcounts should be
/// integrated into the application at a later time.
pub fn main(index: &MemoryMetaIndex, db_path: &Path) -> crate::Result<()> {
    let conn = connect_readonly(db_path)?;
    let mut db = database::Connection::new(&conn);

    let mut counter = PlayCounter::new();
    let mut tx = db.begin()?;
    counter.count_from_database(index, &mut tx)?;
    tx.commit()?;

    counter.equalize_counters();

    for timescale in 0..5 {
        let n_days = ExpCounter::HALF_LIFE_EPOCHS[timescale] * 0.1896;

        let (top_artists, top_albums, top_tracks) =
            counter.get_top_by(150, |counter: &ExpCounter| RevNotNan(counter.n[timescale]));
        print_ranking(
            "TOP",
            format!("timescale {}, {:.0} days", timescale, n_days),
            index,
            &top_artists,
            &top_albums,
            &top_tracks,
        );
    }

    let (trending_artists, trending_albums, trending_tracks) =
        counter.get_top_by(350, score_trending);
    print_ranking(
        "TRENDING",
        format!("14d vs. 57d (2mo) + 228d (7.5mo) + 913d (2.5y)"),
        index,
        &trending_artists,
        &trending_albums,
        &trending_tracks,
    );

    let (falling_artists, falling_albums, falling_tracks) = counter.get_top_by(350, score_falling);
    print_ranking(
        "FALLING",
        format!("2 months vs. 2.5 years + 14 days vs. 7.5 months"),
        index,
        &falling_artists,
        &falling_albums,
        &falling_tracks,
    );

    let discover_ranks = counter.get_discover_rank();
    println!("\nDISCOVERY RANK\n");
    for (i, album_id) in discover_ranks.iter().take(250).enumerate() {
        let album = index.get_album(*album_id).unwrap();
        let album_title = index.get_string(album.title);
        let album_artist = index.get_string(album.artist);
        println!(
            "  {:3} {} {:25}  {}",
            i, album_id, album_title, album_artist
        );
    }

    Ok(())
}
