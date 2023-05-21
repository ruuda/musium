// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of playcounts and other statistics.

use crate::prim::Instant;

/// An instant with 4-second granularity, used to track last played events.
///
/// To limit memory requirements, we would like to store the _last played_
/// timestamp in 32 bits, but that would put a Y2K38 timebomb in the
/// application. So we just shift everything by two bits; that that reduces the
/// resolution to multiples of 4 seconds, but it extends the range by 210 years.
/// By then I will not be alive any more anyway, and 4-second granularity is
/// plenty for tracking listening habits.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CoarseInstant {
    pub posix_quadseconds_utc: i32,
}

impl CoarseInstant {
    const JANUARY_2000: CoarseInstant = CoarseInstant {
        posix_quadseconds_utc: 946684800 / 4
    }

    /// Return the seconds elapsed since `t0`, which should be before `self`.
    #[inline(always)]
    pub fn seconds_since_f32(&self, t0: CoarseInstant) -> f32 {
        let quadsecs = self.posix_quadseconds_utc - t0.posix_quadseconds_utc;
        (quadsecs as f32) * 4.0
    }

    /// Return the seconds elapsed since `t0`, which should be before `self`.
    ///
    /// This may overflow in theory if the elapsed time gets close to ~70 years,
    /// but I'll worry about that in 50 years when my listening history gets to
    /// that point, if I'm still alive then ...
    #[inline(always)]
    pub fn seconds_since_i32(&self, t0: CoarseInstant) -> i32 {
        let quadsecs = self.posix_quadseconds_utc - t0.posix_quadseconds_utc;
        quadsecs * 4
    }
}

impl From<Instant> for CoarseInstant {
    pub fn from(t: Instant) -> CoarseInstant {
        CoarseInstant {
            posix_quadseconds_utc: t.posix_seconds_utc / 4
        }
    }
}
