// Musium -- Music playback daemon with web-based library browser
// Copyright 2022 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of track and album loudness, and track waveforms.

use std::path::{PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use bs1770::{ChannelLoudnessMeter};
use claxon::FlacReader;
use claxon;
use sqlite;

use crate::database::Database;
use crate::database;
use crate::error;
use crate::prim::{AlbumId, TrackId};
use crate::waveform::Waveform;
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
    fn execute(self, db: &mut Database) -> error::Result<()> {
        // We should only execute the album task once all track tasks for the
        // album are done.
        assert!(self.tracks_pending.is_empty());
        assert_eq!(self.tracks_done.len(), self.num_tracks);

        let num_windows = self
            .tracks_done
            .iter()
            .map(|windows| windows[0].as_100ms_windows().len())
            .sum();

        // Concatenate the loudness meters of all tracks on the album.
        let mut windows = [
            bs1770::Windows100ms { inner: Vec::with_capacity(num_windows) },
            bs1770::Windows100ms { inner: Vec::with_capacity(num_windows) },
        ];
        for track_meters in self.tracks_done {
            windows[0].inner.extend_from_slice(track_meters[0].as_100ms_windows().inner);
            windows[1].inner.extend_from_slice(track_meters[1].as_100ms_windows().inner);
        }

        // Now the process is the same as for individual tracks. We can reduce
        // the channels in-place this time, because we are not going to use the
        // power values for anything else any more.
        let [mut channel0, channel1] = windows;
        bs1770::reduce_stereo_in_place(channel0.as_mut(), channel1.as_ref());
        let mean_power = bs1770::gated_mean(channel0.as_ref());

        db.insert_album_loudness(self.album_id, mean_power.loudness_lkfs() as f64)?;

        Ok(())
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
    pub fn execute(self, db: &mut Database) -> error::Result<TrackResult> {
        use error::Error;
        let path = self.path;

        let mut reader = FlacReader::open(&path)
            .map_err(|err| Error::FormatError(path.clone(), err))?;

        let streaminfo = reader.streaminfo();
        // The maximum amplitude is 1 << (bits per sample - 1), because one bit
        // is the sign bit.
        let normalizer = 1.0 / (1_u64 << (streaminfo.bits_per_sample - 1)) as f32;

        assert_eq!(streaminfo.channels, 2, "Only stereo files should enter loudness analysis.");
        let mut meters = [
            ChannelLoudnessMeter::new(streaminfo.sample_rate),
            ChannelLoudnessMeter::new(streaminfo.sample_rate),
        ];

        let mut blocks = reader.blocks();
        let mut buffer = Vec::new();

        // Decode the full track, feed the samples in the meters.
        while let Some(block) = blocks.read_next_or_eof(buffer)
            .map_err(|err| Error::FormatError(path.clone(), err))?
        {
            for (ch, meter) in meters.iter_mut().enumerate() {
                meter.push(block.channel(ch as u32).iter().map(|s| *s as f32 * normalizer));
            }
            buffer = block.into_buffer();
        }

        // We can now determine the track loudness.
        let zipped = bs1770::reduce_stereo(
            meters[0].as_100ms_windows(),
            meters[1].as_100ms_windows(),
        );
        let mean_power = bs1770::gated_mean(zipped.as_ref());
        let waveform = Waveform::from_meters(&meters);

        db.insert_track_loudness(self.track_id, mean_power.loudness_lkfs() as f64)?;
        db.insert_track_waveform(self.track_id, waveform.as_bytes())?;

        let result = TrackResult {
            album_id: self.album_id,
            meters: meters,
        };
        Ok(result)
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
    pub fn execute(self, db: &mut Database) -> error::Result<Option<TrackResult>> {
        match self {
            Task::AnalyzeTrack(task) => return task.execute(db).map(Some),
            Task::AnalyzeAlbum(task) => task.execute(db).map(|()| None),
        }
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

    pub fn num_pending_albums(&self) -> usize {
        self.tasks.len()
    }

    pub fn num_pending_tracks(&self) -> usize {
        self.tasks
            .iter()
            .map(|t| t.tracks_pending.len())
            .sum()
    }

    /// Process all loudness analysis on a threadpool.
    ///
    /// The thread pool more threads than cores on the system, to to ensure that
    /// we can saturate all cores if IO to read the tracks is slow. This method
    /// blocks until processing is done.
    pub fn process_all_in_thread_pool(self, db_path: PathBuf) -> error::Result<()> {
        let task_queue = Arc::new(Mutex::new(self));

        // TODO: Share this thread pool with the thumbnail generation pool.

        // Some experimentation on my system showed that 4 times the number of
        // CPU threads could keep my 16-thread CPU busy, sometimes at 100%, but
        // most of the time it was bottlenecked on IO from a spinning disk.
        let n_threads = 4 * num_cpus::get();
        let mut threads: Vec<JoinHandle<error::Result<()>>> = Vec::with_capacity(n_threads);
        for i in 0..n_threads {
            let db_path_i = db_path.clone();
            let task_queue_i = task_queue.clone();
            let name = format!("Loudness analysis thread {}", i);
            let builder = thread::Builder::new().name(name);
            let join_handle = builder.spawn(move || {
                // We are going to be writing to the database from a few
                // threads, so choose the non-serialized mode (we do not share
                // the connection across threads, so no_mutex is safe). Also set
                // a retry on busy.
                let flags = sqlite::OpenFlags::new()
                    .set_no_mutex()
                    .set_read_write();
                let mut connection = sqlite::Connection::open_with_flags(db_path_i, flags)?;
                let timeout_ms = 2_000;
                connection.set_busy_timeout(timeout_ms)?;

                let mut db = Database::new(&connection)?;

                // Run the thread until there is no more task to execute. If
                // there is currently no task, it doesn't mean there will be no
                // tasks in the future, but those future tasks can only appear
                // after finishing an existing one, so this thread is no longer
                // useful.
                let mut prev_result = None;
                loop {
                    let task = {
                        let mut queue = task_queue_i.lock().unwrap();
                        match queue.get_next_task(prev_result) {
                            Some(task) => task,
                            None => break,
                        }
                    };
                    prev_result = task.execute(&mut db)?;
                }

                Ok(())
            }).unwrap();
            threads.push(join_handle);
        }
        for join_handle in threads.drain(..) {
            // The first unwrap is on joining, that should not fail because we
            // set panic=abort. The ? then propagates any errors that might have
            // happened in the thread.
            join_handle.join().unwrap()?;
        }

        // We shouldn't have exited all threads before the work was done.
        assert!(task_queue.lock().unwrap().is_done());

        Ok(())
    }
}
