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
use crate::playcount::{AlbumData, CountData, PlayCounter, PlayCounts, RevNotNan, TimeVector, TrackData};
use crate::prim::{AlbumId, TrackId, TrackWithId};
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

/// Scores (for ranking) evaluated at a given point in time.
#[derive(Copy, Clone, Default)]
pub struct AlbumScore {
    /// Trending score, see [`AlbumState::score_trending`].
    pub trending: f32,

    /// Discovery score, adjusted for the current moment.
    pub discover: f32,

    /// "For now" score, based on the time of day, week, and year.
    pub for_now: f32,
}

/// Scores (for ranking) evaluated at a given point in time.
#[derive(Copy, Clone, Default)]
pub struct TrackScore {
    pub ft0: f32,
    pub ft1: f32,
    pub fc0: f32,
    pub fc1: f32,
    pub off: f32,
    pub rating: Rating,
}

/// Mutable metadata for tracks, albums, and artists, stemming from user usage.
pub struct UserData {
    /// User-saved track rating, for tracks that the user rated.
    track_ratings: HashMap<TrackId, Rating>,

    /// Playcount-derived data per track.
    /// TODO: Construct an AlbumTable, but for tracks.
    track_data: HashMap<TrackId, TrackData>,

    /// Playcount-derived data per album.
    album_data: AlbumTable<AlbumData>,
}

impl Default for UserData {
    fn default() -> Self {
        use std::collections::hash_map::RandomState;
        let s = RandomState::new();
        Self {
            // TODO: Use a cheaper hasher.
            track_ratings: HashMap::with_hasher(s.clone()),
            track_data: HashMap::with_hasher(s.clone()),
            album_data: AlbumTable::new(0, AlbumData::default()),
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
        let count_data = counts.compute_user_data(&index);
        stats.set_counts(count_data);

        Ok((stats, counts))
    }

    pub fn set_track_rating(&mut self, track_id: TrackId, rating: Rating) {
        *self.track_ratings.entry(track_id).or_default() = rating;
    }

    pub fn get_track_rating(&self, track_id: TrackId) -> Rating {
        self.track_ratings
            .get(&track_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Evaluate album scores for the given moment.
    ///
    /// The `at` time vector should be the embedding of the desired time to
    /// evaluate at, and then normalized.
    pub fn get_album_score(&self, album_id: AlbumId, at: &TimeVector) -> AlbumScore {
        // If an album is not present, we don't have playcounts, so it is
        // ranked as low as possible for all scores.
        let data = match self.album_data.get(album_id) {
            Some(data) => data,
            None => return AlbumScore::default(),
        };

        // The cosine distance between our time vector and the query time vector.
        // We put it in the range [0, 1] so that when we multiply with a negative
        // discover score, it doesn't flip the sign.
        debug_assert!(data.time_embedding.norm().is_finite());
        let time_cos = data.time_embedding.dot(at) / data.time_embedding.norm();
        let time_weight = time_cos.mul_add(0.5, 0.5);

        // Change the range from [0, 1] to [0.31, 1] with more mass near 1.
        let time_weight_mellow = time_weight.mul_add(0.9, 0.1).sqrt();

        AlbumScore {
            trending: data.score_trending,
            discover: data.score_discover * time_weight_mellow,
            for_now: data.score_longterm * time_weight * time_weight,
        }
    }

    /// Compute the scores for all tracks on the album.
    pub fn get_track_scores(&self, tracks: &[TrackWithId]) -> Vec<TrackScore> {
        // Bonus tracks on a B side should have less weight than the A side,
        // but how do we even know if an album has a B side, vs. a compilation
        // album with two equal sides, or a collection with many discs? If disc
        // 1 is at least 2/3 of the tracks, we say the rest is b_side.
        let n_disc1 = tracks.iter().filter(|t| t.track_id.disc_number() == 1).count();
        let has_b_side = n_disc1 >= (tracks.len() * 2 / 3);

        let mut result = Vec::with_capacity(tracks.len());
        let mut buffer = Vec::with_capacity(tracks.len());

        for t in tracks {
            let counts = self.track_data.get(&t.track_id).cloned().unwrap_or_default();
            let rating = self.track_ratings.get(&t.track_id).cloned().unwrap_or_default();

            // We adjust the target playcount based on the track rating. A liked
            // track should be played about 2.7 times as much as a regular one,
            // a loved one even more, and a disliked one only 1/20 as much.
            // These numbers are tweaked by eyeballing the output across many
            // of my albums and adjusting until it feels right. We also add a
            // penalty for B-sides, and for very short tracks.
            let is_b_side = has_b_side && t.track_id.disc_number() != 1;
            let multiplier = match rating {
                Rating::Love => 1.0 / 6.000,
                Rating::Like => 1.0 / 2.718,
                Rating::Neutral if is_b_side => 4.0,
                Rating::Neutral if t.track.duration_seconds < 60 => 4.0,
                Rating::Neutral => 1.0,
                Rating::Dislike => 20.0,
            };

            let score = TrackScore {
                ft0: counts.playcount_longterm,
                ft1: counts.playcount_recently,
                fc0: counts.playcount_longterm * multiplier,
                fc1: counts.playcount_recently * multiplier,
                off: 0.0,
                rating,
            };
            buffer.push(RevNotNan(score.fc0));
            result.push(score);
        }

        // Compute the median of the adjusted longterm playcounts. This is the
        // "target" per track for this album.
        let median_longterm = median(&mut buffer);
        buffer.clear();

        // Subtract the median from the longterm playcount, so we get a sense of
        // whether this track is overplayed or underplayed, and then normalize
        // to the log of the median playcount. Underplayed by 1 listen when we
        // listened to most trackf 20 times already, is ~noise. But underplayed
        // when we listened to most tracks only once, is real signal. I thought
        // at first log might be too aggressive and use sqrt, but that one is
        // too tame, then values rarely go over or under 1.
        let norm = (median_longterm + 0.35).sqrt().recip();
        //let norm = (2.7 + median_longterm).ln().recip();
        for score in result.iter_mut() {
            score.off = score.fc0 * norm;

            // Then add the unadjusted recent playcount to it. Recent plays
            // restore the balance on the longterm excess/deficit at least for
            // now; if a track is underplayed by 3 listens, we don't want to
            // listen to it 3 times in a row to compensate, we'll listen to it
            // again in a few weeks.
            score.off += score.ft1;

            buffer.push(RevNotNan(score.off));
        }

        let median_offset = median(&mut buffer);
        for score in result.iter_mut() {
            score.off -= median_offset;
        }

        result
    }

    /// Replace the album and track data with freshly computed counts.
    ///
    /// This should be tied to the computations [`PlayCounts::compute_user_data`].
    pub fn set_counts(&mut self, counts: CountData) {
        self.track_data = counts.tracks;
        self.album_data = counts.albums;
    }
}

/// Compute the median of a non-empty buffer. Sorts the buffer.
fn median(buffer: &mut Vec<RevNotNan>) -> f32 {
    buffer.sort();
    match buffer.len() / 2 {
        n if 2 * n != buffer.len() => buffer[n].0,
        n => 0.5 * (buffer[n].0 + buffer[n - 1].0),
    }
}
