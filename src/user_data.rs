// Musium -- Music playback daemon with web-based library browser
// Copyright 2023 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Mutable metadata that stems from the user’s library usage, e.g. playcounts.
//!
//! The index itself is immutable, determined completely by the track metadata
//! at scan time. The data in the index is _inherent_ to the tracks, and should
//! (up to tagging preferences) be the same for different users who have the
//! same album in their collection.
//!
//! There is also _extrinsic_ data associated with tracks. This data is not
//! inherent to the track, but stems from the user’s usage. For example, the
//! playcount and rating. Unlike the data in the index, this user data is
//! mutable, it can change during the lifetime of the server.
//!
//! This module is concerned with that mutable user data.

// TODO: Remove once we add playcounts.
#![allow(dead_code)]

use std::collections::HashMap;
use std::convert::TryFrom;

use crate::album_table::AlbumTable;
use crate::database as db;
use crate::playcount::{PlayCounter, PlayCounts, TimeVector};
use crate::prim::{AlbumId, ArtistId, TrackId};
use crate::MemoryMetaIndex;

/// Track rating.
///
/// Musium is meant for curated libraries, which means the user should on
/// average like most tracks in the library. Just the fact that the album is
/// present means that at least some tracks on that album are worth listening
/// to, and usually that means most tracks on the album are at least okay. So
/// one level of dislike is sufficient. For likes, setting a scale is difficult,
/// but I think it can be worth distinguishing between “this track was that one
/// nice one on this album” and “this is one of my favorite tracks ever”.
#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[repr(i8)]
pub enum Rating {
    /// Would usually skip this track when it ended up in the queue.
    Dislike = -1,
    /// No strong opinion, default for unrated tracks.
    #[default]
    Neutral = 0,
    /// Like, the track stands out as a good track on the ablbum.
    Like = 1,
    /// Love, the track stands out as a good track in the library.
    Love = 2,
}

impl TryFrom<i64> for Rating {
    type Error = &'static str;
    fn try_from(r: i64) -> Result<Self, Self::Error> {
        match r {
            -1 => Ok(Rating::Dislike),
            0 => Ok(Rating::Neutral),
            1 => Ok(Rating::Like),
            2 => Ok(Rating::Love),
            _ => Err("Invalid rating, must be in {-1, 0, 1, 2}."),
        }
    }
}

#[derive(Default)]
pub struct TrackState {
    rating: Rating,
    // TODO: Add playcount.
}

#[derive(Copy, Clone, Default)]
pub struct AlbumState {
    /// Ranking for the _discover_ sorting method.
    ///
    /// The discovery sorting methods identifies albums that were popular in the
    /// past, but not recently. See the [`playcount`] module for more details.
    pub score_discover: f32,

    // Playcount on the shortest timescale.
    pub score_trending: f32,

    // Log playcount on the longer timescales.
    //
    // Could be used directly to sort by top albums, but in the UI this is not
    // _that_ useful. Instead, we can mix it with the time embedding to provide
    // a list of "for now" albums for this time of the day, where we don't
    // suggest albums with a low playcount just because the one time we played
    // them was at this time of the day.
    pub score_longterm: f32,

    // Vector embedding of the play times.
    //
    // Used to weigh the discover score, and compute the "for now" score.
    pub time_embedding: TimeVector,
}

/// Scores (for ranking) evaluated at a given point in time.
#[derive(Copy, Clone, Default)]
pub struct ScoreSnapshot {
    /// Trending score, see [`AlbumState::score_trending`].
    pub trending: f32,

    /// Discovery score, adjusted for the current moment.
    pub discover: f32,

    /// "For now" score, based on the time of day, week, and year.
    pub for_now: f32,
}

impl AlbumState {
    /// Evaluate scores for the current moment.
    ///
    /// The `at` time vector should be the embedding of the desired time to
    /// evaluate at, and then normalized.
    pub fn score(&self, at: &TimeVector) -> ScoreSnapshot {
        // The cosine distance between our time vector and the query time vector.
        // We put it in the range [0, 1] so that when we multiply with a negative
        // discover score, it doesn't flip the sign.
        debug_assert!(self.time_embedding.norm().is_finite());
        let time_cos = self.time_embedding.dot(at) / self.time_embedding.norm();
        let time_weight = time_cos.mul_add(0.5, 0.5);

        ScoreSnapshot {
            trending: self.score_trending,
            discover: self.score_discover * time_weight,
            for_now: self.score_longterm * time_weight * time_weight,
        }
    }
}

#[derive(Default)]
pub struct ArtistState {
    // TODO: Add playcount.
}

/// Mutable metadata for tracks, albums, and artists, stemming from user usage.
pub struct UserData {
    tracks: HashMap<TrackId, TrackState>,
    albums: AlbumTable<AlbumState>,
    artists: HashMap<ArtistId, ArtistState>,
}

impl Default for UserData {
    fn default() -> Self {
        use std::collections::hash_map::RandomState;
        let s = RandomState::new();
        Self {
            // TODO: Use a cheaper hasher.
            tracks: HashMap::with_hasher(s.clone()),
            albums: AlbumTable::new(0, AlbumState::default()),
            artists: HashMap::with_hasher(s),
        }
    }
}

impl UserData {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the user data from events saved in the database.
    pub fn load_from_database(
        index: &MemoryMetaIndex,
        tx: &mut db::Transaction,
    ) -> db::Result<(Self, PlayCounts)> {
        let mut stats = Self::default();

        for opt_rating in db::iter_ratings(tx)? {
            let rating = opt_rating?;
            let tid = TrackId(rating.track_id as u64);
            let rating =
                Rating::try_from(rating.rating).expect("Invalid rating value in the database.");
            stats.set_track_rating(tid, rating);
        }

        let mut counter = PlayCounter::new();
        counter.count_from_database(index, tx)?;
        let counts = counter.into_counts();
        stats.set_albums(counts.compute_album_user_data());

        Ok((stats, counts))
    }

    pub fn set_track_rating(&mut self, track_id: TrackId, rating: Rating) {
        self.tracks.entry(track_id).or_default().rating = rating;
    }

    pub fn get_track_rating(&self, track_id: TrackId) -> Rating {
        self.tracks
            .get(&track_id)
            .map(|t| t.rating)
            .unwrap_or_default()
    }

    /// Take a snapshot of the scores for the given album, evaluated at the given query time.
    ///
    /// See also [`AlbumState::score`].
    pub fn get_album_scores(&self, album_id: AlbumId, at: &TimeVector) -> ScoreSnapshot {
        // If an album is not present, we don't have playcounts, so it is
        // ranked as low as possible for all scores.
        self.albums
            .get(album_id)
            .map(|state| state.score(at))
            .unwrap_or_default()
    }

    /// Replace the album scores with new scores.
    ///
    /// This should be tied to the computations [`PlayCounts::compute_album_user_data`].
    pub fn set_albums(&mut self, albums: AlbumTable<AlbumState>) {
        self.albums = albums;
    }
}
