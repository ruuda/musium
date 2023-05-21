// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of playcounts and other statistics.

use std::collections::HashMap;

use crate::prim::Instant;
use crate::prim::{ArtistId, AlbumId, TrackId};
use crate::{MetaIndex, MemoryMetaIndex};

/// An instant with ~hour granularity, used to track last played events.
///
/// The timestamp is reduced in resolution with a dual purpose:
///
/// * To limit memory requirements, we would like to store the _last played_
///   timestamp in 32 bits, but that would put a Y2K38 timebomb in the
///   application. So we just shift everything by a few bits; that that reduces
///   the resolution to multiples of 4096 seconds, but it extends the range by
///   about 300,000 years, long enough not to care.
/// * Exponential moving averages need to be updated every time we move them
///   forward in time by multiplying by a number that is very close to 1. But if
///   the number is too close, floating-point inaccuracies may accumulate
///   quickly. So instead of stepping the clock for every listen, we step it
///   less frequently, in bigger steps. This also allows us to skip updating the
///   moving averages every time we record a new listen.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CoarseInstant {
    /// Seconds since Posix epoch, in multiples of 4096 seconds.
    ///
    /// 4ki stands for 4 _kilobinary_, 4 × 1024.
    pub posix_4kiseconds_utc: i32,
}

/// Measures time elapsed in multiples of 4096 seconds (~1 hour).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CoarseDuration {
    pub duration_4kiseconds: i32,
}

impl CoarseInstant {
    /// Return the time elapsed since `t0`, which should be before `self`.
    #[inline(always)]
    pub fn duration_since(&self, t0: CoarseInstant) -> CoarseDuration {
        CoarseDuration {
            duration_4kiseconds: self.posix_4kiseconds_utc - t0.posix_4kiseconds_utc,
        }
    }
}

impl From<Instant> for CoarseInstant {
    fn from(t: Instant) -> CoarseInstant {
        CoarseInstant {
            posix_4kiseconds_utc: (t.posix_seconds_utc / 4096) as i32
        }
    }
}

impl CoarseDuration {
    /// A duration of 205 minutes (12 kibiseconds, three times 4096 seconds).
    const MINUTES_205: CoarseDuration = CoarseDuration { duration_4kiseconds: 3 };
}

pub struct ExpCounter {
    /// Time at which the counts were last updated.
    pub t: CoarseInstant,

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
    /// more precise than repeated multiplication), but even for the lowest
    /// possible timestep of 4096 seconds, the relative error at the shortest
    /// half-life bucket is only 0.0002%.
    ///
    /// ## Definition
    ///
    /// The unit for the half life is 4096 seconds (4kisecond), so we can
    /// multiply with the inner value of `CoarseDuration` without additional
    /// scaling factor multiplication (which we would need if we measured the
    /// half-life in seconds).
    ///
    /// Table can be generated with the following program:
    /// ```python
    /// xs = [(10 / 4**i) * (365.25 * 24 * 3600 / 4096) for i in range(5)]
    /// for x in xs:
    ///     print(f"        {x:.6f}, // {x * 4096 / (3600 * 24):i} days")
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
    const HALF_LIFE_4KISECONDS: [f32; 5] = [
        77044.921875, // 10 years
        19261.230469, // 2.5 years (30 months)
        4815.307617,  // 7.5 months
        1203.826904,  // 2 months (57 days)
        300.956726,   // 2 weeks (14 days)
    ];

    /// Return how much to decay the counters by after the elapsed time.
    #[inline]
    pub fn decay_factors(duration: CoarseDuration) -> [f32; 5] {
        let dt = duration.duration_4kiseconds as f32;
        Self::HALF_LIFE_4KISECONDS.map(|t| 0.5_f32.powf(dt / t))
    }

    pub fn new() -> ExpCounter {
        ExpCounter {
            t: CoarseInstant { posix_4kiseconds_utc: 0 },
            n: [0.0; 5],
        }
    }

    /// Advance the time to the given instant.
    ///
    /// This applies decay without incrementing the count.
    #[inline]
    pub fn advance(&mut self, t1: CoarseInstant) {
        debug_assert!(t1 >= self.t, "New time must be later than previous time.");
        let elapsed = t1.duration_since(self.t);
        let decay_factors = Self::decay_factors(elapsed);

        for (ni, factor) in self.n.iter_mut().zip(decay_factors) {
            *ni *= factor;
        }
        self.t = t1;
    }

    /// Advance the time to the given instant and increment the count.
    ///
    /// `weight` specifies the amount to increment by. The standard count is
    /// 1.0, but for example for the album counter, multiple consecutive listens
    /// of tracks on the same album should maybe not all count as 1, because
    /// that would create a bias towards albums with more tracks when you listen
    /// entire albums at once.
    #[inline]
    pub fn increment(&mut self, t1: CoarseInstant, weight: f32) {
        debug_assert!(t1 >= self.t, "New time must be later than previous time.");
        let elapsed = t1.duration_since(self.t);
        let decay_factors = Self::decay_factors(elapsed);

        for (ni, factor) in self.n.iter_mut().zip(decay_factors) {
            *ni = ni.mul_add(factor, weight);
        }
        self.t = t1;
    }
}

impl Default for ExpCounter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PlayCounter {
    artists: HashMap<ArtistId, ExpCounter>,
    albums: HashMap<AlbumId, ExpCounter>,
    tracks: HashMap<TrackId, ExpCounter>,
}

impl PlayCounter {
    pub fn new() -> PlayCounter {
        PlayCounter {
            artists: HashMap::new(),
            albums: HashMap::new(),
            tracks: HashMap::new(),
        }
    }

    pub fn count(
        &mut self,
        index: &MemoryMetaIndex,
        at: CoarseInstant,
        track_id: TrackId,
    ) {
        // The track playcount is fairly straightforward, just count 1.0.
        let counter_track = self.tracks.entry(track_id).or_default();
        counter_track.increment(at, 1.0);

        // Get the duration of the track, normalized to the average duration of
        // 264 seconds (which happens to be the mean across my collection).
        let track = index.get_track(track_id).expect("Track must exist.");
        let time_weight = track.duration_seconds as f32 * (1.0 / 264.0);

        // The album playcount is more subtle. If we counted 1.0 for every track
        // in the album, then albums with more tracks would get higher counts if
        // we often listen full albums.
        //
        // To mitigate this, if we already counted this album in the same ~hour
        // window, or in the two ~hours before it, then reduce the weight. We
        // still give some non-zero weight, because we *did* spend time
        // listening to this album, so in the extreme where we listen to one
        // album on repeat all day, it should be counted more than an album that
        // we listen only once.
        //
        // TODO: Should we have two coarse duration types, one with at least
        // ~minute resolution, so the weight can be better tweaked, and there is
        // less arbitrary behavior at bucket boundaries? Some exponentially
        // decaying penalty that decays in ~hours?
        let album_id = track_id.album_id();
        let counter_album = self.albums.entry(album_id).or_default();
        let w_album = if at.duration_since(counter_album.t) < CoarseDuration::MINUTES_205 {
            time_weight * 0.1
        } else {
            1.0
        };
        counter_album.increment(at, w_album);

        // For the artists, counting by time listened seems approprriate.
        let album = index.get_album(album_id).expect("Album should exist.");
        for artist_id in index.get_album_artists(album.artist_ids) {
            let counter_artist = self.artists.entry(*artist_id).or_default();
            counter_artist.increment(at, time_weight);
        }
    }
}
