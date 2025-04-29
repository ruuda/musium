// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of playcounts and other statistics.

use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::path::Path;

use crate::album_table::AlbumTable;
use crate::database::{self, Transaction};
use crate::database_utils::connect_readonly;
use crate::prim::{AlbumId, ArtistId, TrackId};
use crate::user_data::AlbumState;
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
    /// 2000-01-01T00:00:00Z as a Posix timestamp.
    const JAN_2000_POSIX_SECONDS: i64 = 946684800;

    #[inline(always)]
    pub fn from_posix_timestamp(timestamp: i64) -> Instant {
        Instant {
            seconds_since_jan_2000: (timestamp - Instant::JAN_2000_POSIX_SECONDS) as u32,
        }
    }

    pub fn to_posix_timestamp(&self) -> i64 {
        self.seconds_since_jan_2000 as i64 + Instant::JAN_2000_POSIX_SECONDS
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

    /// Embed the instant into the time vector space, see also [`TimeVector`].
    pub fn embed(&self) -> TimeVector {
        const SECONDS_PER_YEAR: u32 = 365 * 24 * 3600 + 6 * 3600;
        const SECONDS_PER_WEEK: u32 = 7 * 24 * 3600;
        const SECONDS_PER_DAY: u32 = 24 * 3600;

        // We convert to radians to map to the circle; precompute as much of
        // the multiplication as we can.
        const NORM_YEAR: f32 = std::f32::consts::TAU / (SECONDS_PER_YEAR as f32);
        const NORM_DAY: f32 = std::f32::consts::TAU / (SECONDS_PER_DAY as f32);

        // We center the day transitions around noon UTC, this matters for how
        // we map the weekdays below.
        let t = self.seconds_since_jan_2000 - 12 * 3600;
        let t_day = t % SECONDS_PER_DAY;
        let t_week = t % SECONDS_PER_WEEK;
        let t_year = t % SECONDS_PER_YEAR;

        let r_day = (t_day as f32) * NORM_DAY;
        let r_year = (t_year as f32) * NORM_YEAR;

        // We map weekdays non-linearly, as follows (where the angle goes from
        // 0 to 1 for a full rotation):
        const MON: f32 = 7.5 / 9.0;
        const TUE: f32 = 8.5 / 9.0;
        const WED: f32 = 0.5 / 9.0;
        const THU: f32 = 1.5 / 9.0;
        const FRI: f32 = 1.1 / 4.0;
        const SAT: f32 = 4.0 / 9.0;
        const SUN: f32 = 6.0 / 9.0;
        let r_week = match t_week / SECONDS_PER_DAY {
            // Jan 1st 2000 was a Saturday,
            0 => SAT + (SUN - SAT) * r_day,
            1 => SUN + (MON - SUN) * r_day,
            2 => MON + (TUE - MON) * r_day,
            // Wrap around 0 happens between Tue and Wed.
            3 => TUE + (1.0 + WED - TUE) * r_day,
            4 => WED + (THU - WED) * r_day,
            5 => THU + (FRI - THU) * r_day,
            6 => FRI + (SAT - FRI) * r_day,
            _ => unreachable!("There are only 7 days in a week."),
        };

        // TODO: Restore the above mapping.
        let r_week = t_week as f32 * (std::f32::consts::TAU / SECONDS_PER_WEEK as f32);

        TimeVector([
            r_year.cos(),
            r_year.sin(),
            r_week.cos(),
            r_week.sin(),
            r_day.cos(),
            r_day.sin(),
        ])
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

/// A vector representation of the time of day, week, and year.
///
/// The rationale behind this is that we can compare how "similar" moments are
/// using the cosine difference, which we can use to classify tracks as morning
/// vs. evening, or weekend vs. weekday, or summer vs. winter. Based on this we
/// hope to suggest better tracks to listen to based on the current moment. E.g.
/// in the early morning we may suggest some chill jazz but not heavy dancefloor
/// banger.
///
/// Because years, weeks, and days are all cyclic, we treat them as circles, and
/// we embed the moment as x, y coordinate on the circle. This ensures that
/// taking the cosine distance is meaningful.
///
/// We populate the space as follows:
/// - Dimension 0, 1: Time of year
/// - Dimension 2, 3: Time of week[^1]
/// - Dimension 4, 5: Time of day (24h)
///
/// [^1]: For the time of the week, we don't map the time uniformly to the
/// circle. We care more about "weekday" vs. "weekend", so the weekdays are
/// relatively squashed.
///
/// We map instants to time vectors without regard for time zone. Local times
/// are irrelevant as long as all listens are mostly in the same time zone, so
/// that the morning/afternoon distinction makes sense. If you move time zones,
/// we should adjust for that, but that's not a problem I have so I'm not
/// solving it right now.
pub struct TimeVector([f32; 6]);

impl TimeVector {
    const fn zero() -> TimeVector {
        TimeVector([0.0; 6])
    }

    fn mul_add(&self, factor: f32, term: TimeVector) -> TimeVector {
        TimeVector([
            self.0[0].mul_add(factor, term.0[0]),
            self.0[1].mul_add(factor, term.0[1]),
            self.0[2].mul_add(factor, term.0[2]),
            self.0[3].mul_add(factor, term.0[3]),
            self.0[4].mul_add(factor, term.0[4]),
            self.0[5].mul_add(factor, term.0[5]),
        ])
    }

    /// For debugging, format as human-readable direction that the vector points in.
    ///
    /// Note, this is only approximate. We assume for example that every month
    /// is exactly 1/12 of a year, where a year is 365.25 days. It's about the
    /// rough direction anyway so this is fine.
    #[rustfmt::skip]
    fn fmt_dir(&self) -> String {
        use std::f32::consts::TAU;

        let mut r_year = self.0[1].atan2(self.0[0]);
        let mut r_week = self.0[3].atan2(self.0[2]);
        let mut r_day = self.0[5].atan2(self.0[4]);

        // During embedding, we consider midnight the day boundary and we
        // subtract half a day from the timestamp, so here we add it back.
        r_week += std::f32::consts::TAU / 14.0;

        r_year += if r_year < 0.0 { TAU } else { 0.0 };
        r_week += if r_week < 0.0 { TAU } else { 0.0 };
        r_day += if r_day < 0.0 { TAU } else { 0.0 };

        let month = (r_year * (11.999 / TAU)) as usize;
        let day = (r_week * (6.999 / TAU)) as usize;
        let hour = (r_day * (23.999 / TAU)) as usize;

        const MONTHS: [&'static str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun",
            "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        // Days start on Saturday, see [`Instant::embed`].
        const DAYS: [&'static str; 7] = [
            "Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri",
        ];
        // Hours start at noon UTC, see [`Instant::embed`].
        const HOURS: [&'static str; 24] = [
            "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23",
            "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11",
        ];

        format!("{} {} {}:00Z", MONTHS[month], DAYS[day], HOURS[hour])
    }
}

impl std::ops::Add<&TimeVector> for TimeVector {
    type Output = TimeVector;

    fn add(self, rhs: &TimeVector) -> TimeVector {
        TimeVector([
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
            self.0[5] + rhs.0[5],
        ])
    }
}

impl std::ops::Mul<f32> for TimeVector {
    type Output = TimeVector;

    fn mul(self, rhs: f32) -> TimeVector {
        TimeVector([
            self.0[0] * rhs,
            self.0[1] * rhs,
            self.0[2] * rhs,
            self.0[3] * rhs,
            self.0[4] * rhs,
            self.0[5] * rhs,
        ])
    }
}

/// Exponential moving averages at different timescales plus leaky bucket rate limiter.
pub struct ExpCounter {
    /// Time at which the counts were last updated.
    pub t: Instant,

    /// “Count” left in the bucket for leaky-bucket rate limiting.
    pub bucket: f32,

    /// Exponentially decaying counts for different half-lives.
    pub n: [f32; 5],

    /// Exponential moving average of the time vector of each play.
    pub time_embedding: TimeVector,
}

impl ExpCounter {
    /// Half-lives for which we keep a moving average.
    ///
    /// ## Spacing of the half-lives
    ///
    /// The half lives quadruple every time (from short to long). This provides
    /// a nice logarithmic spacing on the “long half-life” to “short half-life”
    /// spectrum, and most values work out to align close to a natural interval.
    /// In the past we used ~14 days as the lowest bucket, and the next one two
    /// months, but it turns out that half life lingers on for longer than what
    /// I feel is “the past two weeks”, so we reduced all intervals to 7 days,
    /// one month, 4 months, etc. 4 months is a good interval to capture “what
    /// is hot this season”, while the double of 7-8 months shows you winter
    /// music in summer and vice versa.
    ///
    /// This spacing also enables us to efficiently compute exponential decay
    /// factors from the long-duration one: raise it to the fifth power to get
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
    /// xs = [(5 / 4**i) * (365.25 * 24 * 3600 / 2**14) for i in range(5)]
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
    /// ```txt
    /// half_life / ln(2) * [1 - 0.5^(t_1 / half_life)]
    /// ```
    ///
    /// and if we plug in ∞ for `t_1` then we see the scale factors are just
    /// proportional to the half lives, so we can divide by that. (We care only
    /// about relative counts, so we can skip the `ln(2)` factor.)
    ///
    /// TODO: We can choose for `t_1` the first time at which the album was
    /// seen, then new albums don't have as much of a penalty in the
    /// long-running average.
    const HALF_LIFE_EPOCHS: [f32; 5] = [
        // For the top two buckets we make an exception, that one we keep at 10
        // years.
        19260.0, // 3650 days / 10 years
        // 9630.615234, // 1826 days / 5 years
        2407.653809, // 457 days / 1.25 years
        601.913452,  // 114 days / ~3.75 months / 16 weeks
        150.478363,  // 29 days / 1 month
        37.619591,   // 7 days
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
            time_embedding: TimeVector::zero(),
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

        // In addition to updating the counters, we update the time vector for
        // this item. We used a fixed decay factor of 0.9 (so every new play
        // weighs 0.1), but we do take into account the rate limit.
        let v = t1.embed();
        let f = count * 0.1;
        self.time_embedding = self.time_embedding.mul_add(1.0 - f, v * f);
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

/// A playcounter counts plays.
///
/// Internally it has a counter per entry (artist, album, track) with
/// exponential moving averages. The last updated time can differ across those,
/// which makes counts not directly comparable. To make the counter values
/// comparable, we have to advance all counters to the same timestamp, which is
/// what [`into_counts`] does.
pub struct PlayCounter {
    /// The timestamp of the last inserted listen.
    last_counted_at: Instant,
    artists: HashMap<ArtistId, ExpCounter>,
    albums: HashMap<AlbumId, ExpCounter>,
    tracks: HashMap<TrackId, ExpCounter>,
}

/// Playcounts are the result of using a playcounter.
///
/// In a `PlayCounts` struct all the last updated times of the counters are
/// equalized, which makes the count values comparable. This makes this form of
/// the counter suitable for doing statistics on.
///
/// To resume counting, call `into_counter`.
pub struct PlayCounts {
    counter: PlayCounter,
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
        capacity: 2.0,
        fill_rate_per_second: 1.0 / (3600.0 * 13.0),
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

    /// Traverse all listens in the `listens` table and count them.
    ///
    /// This imports only the listens that are newer than the most recently
    /// counted one, so per session this is incremental.
    pub fn count_from_database(
        &mut self,
        index: &MemoryMetaIndex,
        tx: &mut Transaction,
    ) -> database::Result<()> {
        let start_second = self.last_counted_at.to_posix_timestamp();
        let mut n = 0;
        for listen_opt in database::iter_listens_since(tx, start_second)? {
            let listen = listen_opt?;
            let at = Instant::from_posix_timestamp(listen.started_at_second);
            let track_id = TrackId(listen.track_id as u64);
            self.count(index, at, track_id);
            n += 1;
        }
        println!("Playcount: imported new listens from database, n={n}");
        Ok(())
    }

    /// Advance all counters to the time of the last inserted listen.
    ///
    /// This makes the counters comparable, hence we can return [`PlayCounts`].
    pub fn into_counts(mut self) -> PlayCounts {
        self.advance_counters(self.last_counted_at);
        PlayCounts { counter: self }
    }
}

impl PlayCounts {
    pub fn into_counter(self) -> PlayCounter {
        self.counter
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
            get_top_n(n_top, &mut expr, &self.counter.artists),
            get_top_n(n_top, &mut expr, &self.counter.albums),
            get_top_n(n_top, &mut expr, &self.counter.tracks),
        )
    }

    /// Recompute the albums table for the mutable user data.
    pub fn compute_album_user_data(&self) -> AlbumTable<AlbumState> {
        let mut albums = AlbumTable::new(self.counter.albums.len(), AlbumState::default());
        for (album_id, counter) in self.counter.albums.iter() {
            let state = AlbumState {
                discover_score: score_falling(counter),
                trending_score: score_trending(counter),
            };
            albums.insert(*album_id, state);
        }
        albums
    }
}

fn print_ranking(
    title: &'static str,
    description: String,
    index: &MemoryMetaIndex,
    counts: &PlayCounts,
    top_artists: &[(RevNotNan, ArtistId)],
    top_albums: &[(RevNotNan, AlbumId)],
    top_tracks: &[(RevNotNan, TrackId)],
) {
    println!("\n{title} ARTISTS ({description})\n");
    for (i, (count, artist_id)) in top_artists.iter().enumerate() {
        let artist = index.get_artist(*artist_id).unwrap();
        let artist_name = index.get_string(artist.name);
        let counter = counts.counter.artists.get(artist_id).unwrap();

        println!(
            "  {:2} {:7.3} {} {} {}",
            i + 1,
            count.0,
            counter.time_embedding.fmt_dir(),
            artist_id,
            artist_name
        );
    }

    println!("\n{title} ALBUMS ({description})\n");
    for (i, (count, album_id)) in top_albums.iter().enumerate() {
        let album = index.get_album(*album_id).unwrap();
        let album_title = index.get_string(album.title);
        let album_artist = index.get_string(album.artist);
        let counter = counts.counter.albums.get(album_id).unwrap();

        println!(
            "  {:2} {:7.3} {} {} {:25}  {}",
            i + 1,
            count.0,
            counter.time_embedding.fmt_dir(),
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
        let counter = counts.counter.tracks.get(track_id).unwrap();

        println!(
            "  {:2} {:7.3} {} {} {:25}  {}",
            i + 1,
            count.0,
            counter.time_embedding.fmt_dir(),
            track_id,
            track_title,
            track_artist
        );
    }
}

/// Score for sorting entries by _trending_.
///
/// Trending entries (tracks, albums, artists) are entries that have a high
/// playcount on a short timescale, while still mixing in a bit of a longer
/// time horizon.
fn score_trending(counter: &ExpCounter) -> f32 {
    (2.0 * counter.n[4]) + (0.5 * counter.n[3]) + (0.1 * counter.n[2])
}

/// Score for sorting entries by _falling_.
///
/// Falling entries (tracks, albums, artists) are entries that have a high
/// playcount on a long-term timescale, but low playcount recently.
fn score_falling(counter: &ExpCounter) -> f32 {
    // The comments include some empirical data for albums from my own listening
    // history.

    // The top 50 ranges from ~3.3 to ~2.5, with #25 at ~2.7.
    let f0 = counter.n[1].ln() - counter.n[3];

    // The top 50 ranges from ~2.1 to ~1.1, with #25 at ~1.4.
    let f1 = counter.n[2].ln() - counter.n[3];

    // The top 50 ranges from ~3.3 to ~2.0, with #25 at ~2.4.
    let f2 = counter.n[2].ln() - counter.n[4];

    // Weights chosen empirically.
    f0 + f1 * 0.2 + f2 * 0.6
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
    let counts = counter.into_counts();

    for timescale in 0..5 {
        let n_days = ExpCounter::HALF_LIFE_EPOCHS[timescale] * 0.1896;
        let n_months = ExpCounter::HALF_LIFE_EPOCHS[timescale] * 0.1896 * (12.0 / 365.25);

        let (top_artists, top_albums, top_tracks) =
            counts.get_top_by(150, |counter: &ExpCounter| RevNotNan(counter.n[timescale]));
        print_ranking(
            "TOP",
            format!(
                "timescale {}, {:.0} days / {:.0} months",
                timescale, n_days, n_months
            ),
            index,
            &counts,
            &top_artists,
            &top_albums,
            &top_tracks,
        );
    }

    let (trending_artists, trending_albums, trending_tracks) =
        counts.get_top_by(350, |c| RevNotNan(score_trending(c)));
    print_ranking(
        "TRENDING",
        "see code for formula".to_string(),
        index,
        &counts,
        &trending_artists,
        &trending_albums,
        &trending_tracks,
    );

    let (falling_artists, falling_albums, falling_tracks) =
        counts.get_top_by(350, |c| RevNotNan(score_falling(c)));
    print_ranking(
        "FALLING",
        "see code for formula".to_string(),
        index,
        &counts,
        &falling_artists,
        &falling_albums,
        &falling_tracks,
    );

    Ok(())
}

#[cfg(test)]
pub mod test {
    use super::Instant;
    use chrono::{DateTime, Utc};

    fn fmt_dir(dt: DateTime<Utc>) -> String {
        Instant::from_posix_timestamp(dt.timestamp())
            .embed()
            .fmt_dir()
    }

    #[test]
    #[rustfmt::skip]
    fn time_vector_embed_format_works_as_expected() {
        use chrono::{TimeZone, Utc};

        // Month, day of week, hour of day.
        // 2025-04-14 is a Monday.
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 14).and_hms( 9, 5, 0)), "Apr Mon 09:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 15).and_hms(11, 5, 0)), "Apr Tue 11:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 16).and_hms(13, 5, 0)), "Apr Wed 13:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 17).and_hms(15, 5, 0)), "Apr Thu 15:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 18).and_hms(17, 5, 0)), "Apr Fri 17:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 19).and_hms(19, 5, 0)), "Apr Sat 19:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 4, 20).and_hms(21, 5, 0)), "Apr Sun 21:00Z");

        assert_eq!(fmt_dir(Utc.ymd(2025,  1, 15).and_hms( 7, 5, 0)), "Jan Wed 07:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  2, 15).and_hms( 9, 5, 0)), "Feb Sat 09:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  3, 15).and_hms(11, 5, 0)), "Mar Sat 11:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  4, 15).and_hms(13, 5, 0)), "Apr Tue 13:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  5, 15).and_hms(15, 5, 0)), "May Thu 15:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  6, 15).and_hms(17, 5, 0)), "Jun Sun 17:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  7, 15).and_hms(19, 5, 0)), "Jul Tue 19:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  8, 15).and_hms(21, 5, 0)), "Aug Fri 21:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025,  9, 15).and_hms(23, 5, 0)), "Sep Mon 23:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 10, 15).and_hms( 1, 5, 0)), "Oct Wed 01:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 11, 15).and_hms( 2, 5, 0)), "Nov Sat 02:00Z");
        assert_eq!(fmt_dir(Utc.ymd(2025, 12, 15).and_hms( 6, 5, 0)), "Dec Mon 06:00Z");
    }
}
