// Musium -- Music playback daemon with web-based library browser
// Copyright 2022 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of track and album loudness, and track waveforms.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use bs1770::{ChannelLoudnessMeter};

use crate::database::Database;
use crate::database;
use crate::prim::{AlbumId, TrackId};
use crate::waveform;
use crate::{MetaIndex, MemoryMetaIndex};

/// Tracks the state of loudness analysis for one album.
struct AlbumTask {
    album_id: AlbumId,

    /// Track ids of the tracks that we haven't started analyzing.
    tracks_pending: Vec<TrackId>,

    /// Results of tracks that we have analyzed, in no particular order.
    tracks_done: Vec<[ChannelLoudnessMeter; 2]>,

    /// The total number of tracks in this album.
    num_tracks: usize,
}

impl AlbumTask {
    fn execute(self) {
        // We should only execute the album task once all track tasks for the
        // album are done.
        assert!(self.tracks_pending.is_empty());
        assert_eq!(self.tracks_done.len(), self.num_tracks);

        println!("TODO");
    }
}

struct TrackResult {
    album_id: AlbumId,
    meters: [ChannelLoudnessMeter; 2],
}

/// Tracks the state of loudness analysis for one track.
struct TrackTask {
    album_id: AlbumId,
    track_id: TrackId,
    path: PathBuf,
}

impl TrackTask {
    pub fn execute(self) -> TrackResult {
        unimplemented!();
    }
}

enum Task {
    AnalyzeTrack(TrackTask),
    AnalyzeAlbum(AlbumTask),
}

impl Task {
    /// Execute this task.
    ///
    /// Returns the result in case of a `Task::AnalyzeTrack` task.
    pub fn execute(self) -> Option<TrackResult> {
        match self {
            Task::AnalyzeTrack(task) => return Some(task.execute()),
            Task::AnalyzeAlbum(task) => task.execute(),
        }
        None
    }
}

/// A task queue for album and track loudness analysis.
///
/// There are two types of tasks. When asked for the next task, the task queue
/// will return (in this order of priority):
///
/// 1. `Task::AnalyzeAlbum` if there is an album whose tracks are done.
/// 2. `Task::AnalyzeTrack` if there is a track to analyze.
/// 3. `None` if we are done for now.
pub struct TaskQueue {
    index: Arc<MemoryMetaIndex>,
    tasks: Vec<AlbumTask>,
}

impl TaskQueue {
    pub fn new(index: Arc<MemoryMetaIndex>) -> TaskQueue {
        TaskQueue {
            index: index,
            tasks: Vec::new(),
        }
    }

    /// Add a task to analyze the loudness of the given album and its tracks.
    pub fn push_task_album(&mut self, album_id: AlbumId) {
        let tracks = self.index.get_album_tracks(album_id);
        let task = AlbumTask {
            album_id: album_id,
            tracks_pending: tracks.iter().map(|(id, _)| *id).collect(),
            tracks_done: Vec::with_capacity(tracks.len()),
            num_tracks: tracks.len(),
        };
        self.tasks.push(task);
    }

    /// Add tasks to analyze the loudness of missing albums and tracks.
    ///
    /// This adds tasks for all albums and tracks that are in the index, but not
    /// in the database. Note, this is a somewhat expensive query, since we
    /// check the existence of every album and track. It's better to rely on
    /// incremental building, but this can be used to backfill an old database.
    pub fn push_tasks_missing(&mut self, db: &mut Database) -> database::Result<()> {
        let index = self.index.clone();

        // Note, this is not the most efficient index query, because we have to
        // locate the tracks per album. We could do better by iterating the
        // tracks instead, but it complicates this code, and we are issueing a
        // SELECT against the database for every track anyway, so the difference
        // should be dwarfed by the SQLite call anyway.
        'albums: for (album_id, _album) in index.get_albums() {
            // If the album is not there, we need to add it.
            if db.select_album_loudness(*album_id)?.is_none() {
                self.push_task_album(*album_id);
                continue 'albums
            }

            // If one of the tracks is not there, we also add the full album.
            for (track_id, _track) in index.get_album_tracks(*album_id) {
                if db.select_track_loudness(*track_id)?.is_none() {
                    self.push_task_album(*album_id);
                    continue 'albums
                }

                // TODO: Check the waveforms as well.
            }
        }

        Ok(())
    }

    fn finish_track(&mut self, album_id: AlbumId, meters: [ChannelLoudnessMeter; 2]) {
        for album_task in self.tasks.iter_mut().rev() {
            if album_task.album_id == album_id {
                album_task.tracks_done.push(meters);
                return;
            }
        }

        panic!("Finished a track for an album that was never queued.");
    }

    /// Store the result of the previous task, if any, then get the next task.
    fn get_next_task(&mut self, prev_result: Option<TrackResult>) -> Option<Task> {
        if let Some(track_result) = prev_result {
            self.finish_track(track_result.album_id, track_result.meters);
        }

        // If we have an album for which all tracks have been analyzed, our next
        // task is to process that album. We iterate in reverse order so we can
        // pop at the end, so removal is efficient.
        for i in (0..self.tasks.len()).rev() {
            if self.tasks[i].num_tracks == self.tasks[i].tracks_done.len() {
                return Some(Task::AnalyzeAlbum(self.tasks.swap_remove(i)));
            }
        }

        // Apparently we have no album that is done; find the next track that we
        // can analyze. Also start from the end, because then we can pop albums
        // when they are done.
        for album_task in self.tasks.iter_mut().rev() {
            let track_id = match album_task.tracks_pending.pop() {
                Some(id) => id,
                None => continue,
            };

            let track = self
                .index
                .get_track(track_id)
                .expect("We got this track from this index.");
            let fname = self.index.get_filename(track.filename);
            let task = TrackTask {
                album_id: album_task.album_id,
                track_id: track_id,
                path: PathBuf::from(fname),
            };
            return Some(Task::AnalyzeTrack(task));
        }

        // If we get here, there is nothing to do for now.
        None
    }

    fn is_done(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Process all loudness analysis on a threadpool.
    ///
    /// The thread pool has as many threads as cores on the system. This method
    /// blocks until processing is done.
    pub fn process_all_in_thread_pool(self) {
        eprintln!("{} tasks in loudness queue", self.tasks.len());
        let task_queue = Arc::new(Mutex::new(self));

        // TODO: Share this thread pool with the thumbnail generation pool.
        let n_threads = num_cpus::get();
        let mut threads = Vec::with_capacity(n_threads);
        for i in 0..n_threads {
            let task_queue_i = task_queue.clone();
            let name = format!("Loudness analysis thread {}", i);
            let builder = thread::Builder::new().name(name);
            let join_handle = builder.spawn(move || {
                let mut prev_result = None;
                // Run the thread until there is no more task to execute. If
                // there is currently no task, it doesn't mean there will be no
                // tasks in the future, but those future tasks can only appear
                // after finishing an existing one, so this thread is no longer
                // useful.
                while let Some(task) = task_queue_i
                    .lock()
                    .unwrap()
                    .get_next_task(prev_result)
                {
                    prev_result = task.execute();
                }
            }).unwrap();
            threads.push(join_handle);
        }
        for join_handle in threads.drain(..) {
            join_handle.join().unwrap();
        }

        // We shouldn't have exited all threads before the work was done.
        assert!(task_queue.lock().unwrap().is_done());
    }
}
