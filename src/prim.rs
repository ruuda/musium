// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Primitive data types for the music library.

use std::fmt;
use std::str::FromStr;

// Stats of my personal music library at this point:
//
//     11.5k tracks
//      1.2k albums
//      0.3k album artists
//      1.4k track artists
//
// The observation is that there is an order of magnitude difference between
// the track count and album count, and also between album count and artist
// count. In other words, track data will dominate, and album artist data is
// hardly relevant.
//
// What should I design for? My library will probably grow to twice its size
// over time. Perhaps even to 10x the size. But I am pretty confident that it
// will not grow by 100x. So by designing the system to support 1M tracks, I
// should be safe.
//
// Let's consider IDs for a moment. The 16-byte MusicBrainz UUIDs take up a lot
// of space, and I want to run on low-end systems, in particular the
// first-generation Raspberry Pi, which has 16k L1 cache and 128k L2 cache.
// Saving 50% on IDs can have a big impact there. So under the above assumptions
// of 1M tracks, can I get away with using 8 bytes of the 16-byte UUIDs? Let's
// consider the collision probability. With 8-byte identifiers, to have a 1%
// collision probability, one would need about 608M tracks. That is a lot more
// than what I am designing for. For MusicBrainz, which aims to catalog every
// track ever produced by humanity, this might be too risky. But for my personal
// collection the memory savings are well worth the risk.
//
// Let's dig a bit further: I really only need to uniquely identify album
// artists, then albums by that artist, and then tracks on those albums. And I
// would like to do so based on their metadata only, not involving global
// counters, because I want something that is deterministic but which can be
// parallelized. So how many bits do we need for the album artist? Let's say
// the upper bound is 50k artists, and I want a collision probability of at most
// 0.1% at that number of artists. The lowest multiple of 8 that I can get away
// with is 48 bits.

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrackId(pub u64);

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AlbumId(pub u64);

#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtistId(pub u64);

/// Index into a byte array that contains length-prefixed strings.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StringRef(pub u32);

/// Index into a byte array that contains length-prefixed strings.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FilenameRef(pub u32);

impl TrackId {
    #[inline]
    pub fn parse(src: &str) -> Option<TrackId> {
        u64::from_str_radix(src, 16).ok().map(TrackId)
    }
}

impl AlbumId {
    #[inline]
    pub fn parse(src: &str) -> Option<AlbumId> {
        u64::from_str_radix(src, 16).ok().map(AlbumId)
    }
}

impl ArtistId {
    #[inline]
    pub fn parse(src: &str) -> Option<ArtistId> {
        u64::from_str_radix(src, 16).ok().map(ArtistId)
    }
}

/// Loudness Units relative to Full Scale.
///
/// The representation is millibel relative to full scale. In other words, this
/// is a decimal fixed-point number with two decimal digits after the point.
///
/// Example: -7.32 LUFS would be stored as `Lufs(-732)`.
///
/// The default value is -9.0 LUFS: across a collection of 16k tracks and 1.3k
/// albums, the median track loudness was found to be -9.10 LUFS, and the median
/// album loudness was found to be -8.98 LUFS, so a value of -9.0 seems a
/// reasonable best guess in the absence of a true measurement.
///
/// A value of 0.0 LUFS is not allowed to support the nonzero optimization, such
/// that an `Option<Lufs>` is 16 bits. This should not be a restriction for
/// empirically measured loudness, which is typically negative in LUFS.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Lufs(pub std::num::NonZeroI16);

impl Lufs {
    pub fn new(centi_loudness_units: i16) -> Lufs {
        Lufs(
            std::num::NonZeroI16::new(centi_loudness_units)
            .expect("A value of 0.0 LUFS is not allowed, use -0.01 LUFS instead.")
        )
    }

    pub fn default() -> Lufs {
        Lufs(std::num::NonZeroI16::new(-900).unwrap())
    }
}

impl fmt::Display for Lufs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2} LUFS", (self.0.get() as f32) * 0.01)
    }
}

impl FromStr for Lufs {
    type Err = &'static str;

    fn from_str(s: &str) -> std::result::Result<Lufs, &'static str> {
        match s.strip_suffix(" LUFS") {
            None => Err("Expected loudness value of the form '-9.999 LUFS', but the LUFS suffix is missing."),
            Some(num) => match f32::from_str(num) {
                Err(_) => Err("Expected loudness value of the form '-9.999 LUFS', but the number is invalid."),
                // Put some reasonable bounds on the loudness value, that on the
                // one hand prevents nonsensical values, and on the other hand
                // ensures that we can convert to i16 without overflow.
                Ok(x) if x < -70.0 => Err("Loudness is too low, should be at least -70.0 LUFS."),
                Ok(x) if x >  70.0 => Err("Loudness is too high, should be at most 70.0 LUFS."),
                Ok(x) if x == 0.0  => Err("Loudness of exactly 0.0 LUFS is disallowed, use -0.01 LUFS instead."),
                Ok(x) => Ok(Lufs(std::num::NonZeroI16::new((x * 100.0) as i16).unwrap())),
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Track {
    pub album_id: AlbumId,
    pub title: StringRef,
    pub artist: StringRef,
    pub filename: FilenameRef,
    // Using u16 for duration gives us a little over 18 hours as maximum
    // duration; using u8 for track number gives us at most 255 tracks. This is
    // perhaps a bit limiting, but it does allow us to squeeze a `(TrackId,
    // Track)` into half a cache line, so they never straddle cache line
    // boundaries. And of course more of them fit in the cache. If range ever
    // becomes a problem, we could use some of the disc number bits to extend
    // the duration range or track number range.
    pub duration_seconds: u16,
    pub disc_number: u8,
    pub track_number: u8,

    // TODO: Because of this field, the `Track` type becomes too big. But we can
    // save this, because `album_id` could be removed if we make the album id a
    // prefix of the track id.
    pub loudness: Option<Lufs>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Date {
        // We assume dates are parsed from YYYY-MM-DD strings.
        // Note that zeros are valid, they are used to indicate
        // unknown months or days.
        debug_assert!(year <= 9999);
        debug_assert!(month <= 12);
        debug_assert!(day <= 31);
        Date {
            year,
            month,
            day,
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Album {
    pub artist_id: ArtistId,
    pub title: StringRef,
    pub original_release_date: Date,
    pub loudness: Option<Lufs>,
}

#[repr(C)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Artist {
    pub name: StringRef,
    pub name_for_sort: StringRef,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if self.month == 0 { return Ok(()) }
        write!(f, "-{:02}", self.month)?;
        if self.day == 0 { return Ok(()) }
        write!(f, "-{:02}", self.day)
    }
}
