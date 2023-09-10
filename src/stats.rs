// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Statistics and other mutable state about library elements.
//!
//! While the index itself is immutable, determined from the static metadata at
//! scan time, there is data associated with tracks that is mutable. For
//! example, the playcount, and the rating.
//!
//! This module is concerned with that mutable data.

// TODO: Remove once we add playcounts.
#![allow(dead_code)]

use std::collections::HashMap;

use crate::prim::{AlbumId, ArtistId, TrackId};

/// Track rating.
///
/// Musium is meant for curated libraries, which means the user should on
/// average like most tracks in the library. Just the fact that the album is
/// present means that at least some tracks on that album are worth listening
/// to, and usually that means most tracks on the album are at least okay. So
/// one level of dislike is sufficient. For likes, setting a scale is difficult,
/// but I think it can be worth distinguishing between “this track was that one
/// nice one on this album” and “this is one of my favorite tracks ever”.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(i8)]
pub enum Rating {
    /// Would usually skip this track when it ended up in the queue.
    Dislike = -1,
    /// No strong opinion, default for unrated tracks.
    Neutral = 0,
    /// Like, the track stands out as a good track on the ablbum.
    Like = 1,
    /// Love, the track stands out as a good track in the library.
    Love = 2,
}

impl Default for Rating {
    fn default() -> Self {
        Rating::Neutral
    }
}

#[derive(Default)]
pub struct TrackState {
    rating: Rating,
    // TODO: Add playcount.
}

#[derive(Default)]
pub struct AlbumState {
    // TODO: Add playcount and last/first seen/played.
}

#[derive(Default)]
pub struct ArtistState {
    // TODO: Add playcount.
}

pub struct Stats {
    tracks: HashMap<TrackId, TrackState>,
    albums: HashMap<AlbumId, AlbumState>,
    artists: HashMap<ArtistId, ArtistState>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            // TODO: Use a cheaper hasher.
            tracks: HashMap::new(),
            albums: HashMap::new(),
            artists: HashMap::new(),
        }
    }

    pub fn set_track_rating(&mut self, track_id: TrackId, rating: Rating) {
        self.tracks.entry(track_id).or_default().rating = rating;
    }

    pub fn get_track_rating(&self, track_id: TrackId) -> Rating {
        self.tracks.get(&track_id).map(|t| t.rating).unwrap_or_default()
    }
}

