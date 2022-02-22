// Musium -- Music playback daemon with web-based library browser
// Copyright 2022 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Computation of track and album loudness, and track waveforms.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{SyncSender, Receiver, sync_channel};
use std::sync::{Arc, Mutex};

use bs1770::{ChannelLoudnessMeter};
use claxon::FlacReader;
use claxon;

use crate::database::Database;
use crate::database;
use crate::error;
use crate::prim::{AlbumId, TrackId};
use crate::scan::Status;
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
    fn execute(self, inserts: &SyncSender<Insert>) {
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

        inserts.send(Insert::Album {
            album_id: self.album_id,
            loudness: bs1770::gated_mean(channel0.as_ref()),
        }).unwrap();
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
    pub fn execute(self, inserts: &SyncSender<Insert>) -> error::Result<TrackResult> {
        use error::Error;
        let path = self.path;

        let f = std::fs::File::open(&path)?;

        // Hint to the OS that we are going to read the entire file, and we are
        // going to do it sequentially, so it can read the entire file at once
        // and hopefully avoid a few seeks.
        let offset = 0;
        let len = f.metadata()?.len();
        let advice = libc::POSIX_FADV_SEQUENTIAL | libc::POSIX_FADV_WILLNEED;
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::posix_fadvise64(f.as_raw_fd(), offset, len as i64, advice);
        }

        let mut reader = FlacReader::new(f)
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

        inserts.send(Insert::Track {
            track_id: self.track_id,
            loudness: bs1770::gated_mean(zipped.as_ref()),
            waveform: Waveform::from_meters(&meters),
        }).unwrap();

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

enum TaskResult {
    None,
    Track(TrackResult),
    Album,
}

/// A database insert operation.
///
/// We serialize inserts into the database to avoid write contention, this is
/// what gets sent over a channel.
enum Insert {
    Track {
        track_id: TrackId,
        loudness: bs1770::Power,
        waveform: Waveform,
    },
    Album {
        album_id: AlbumId,
        loudness: bs1770::Power,
    }
}

fn process_inserts(
    db_path: &Path,
    inserts: Receiver<Insert>,
) -> database::Result<()> {
    let connection = database::connect_read_write(db_path)?;
    let mut db = Database::new(&connection)?;

    // Reduce the number of fsyncs (and thereby improve performance), at the
    // cost of losing durability (but not consistency). This is fine, if we lose
    // power and some work is lost, we can re-do it. But more likely we kill the
    // process because it takes a long time or because it crashed, and then we
    // still have the progress.
    db.connection.execute("PRAGMA synchronous = NORMAL;")?;

    // We commit after every album, instead of at every single write, to reduce
    // the number of syncs. This makes a big difference for disk utilisation
    // when the disk to read from and the disk that contain the database are the
    // same disk.
    db.connection.execute("BEGIN")?;

    for insert in inserts {
        match insert {
            Insert::Track { track_id, loudness, waveform } => {
                db.insert_track_loudness(track_id, loudness.loudness_lkfs() as f64)?;
                db.insert_track_waveform(track_id, waveform.as_bytes())?;
            }
            Insert::Album { album_id, loudness } => {
                db.insert_album_loudness(album_id, loudness.loudness_lkfs() as f64)?;
                db.connection.execute("COMMIT")?;
                db.connection.execute("BEGIN")?;
            }
        }
    }

    db.connection.execute("COMMIT")?;

    // Integrate the WAL into the rest of the database, now that we are done
    // writing. Also vacuum the database to clean up any index pages that may
    // have become redundant.
    db.connection.execute("PRAGMA wal_checkpoint(TRUNCATE);")?;
    db.connection.execute("VACUUM;")?;

    Ok(())
}

impl Task {
    /// Execute this task.
    ///
    /// Returns the result in case of a `Task::AnalyzeTrack` task.
    pub fn execute(self, inserts: &SyncSender<Insert>) -> error::Result<TaskResult> {
        match self {
            Task::AnalyzeTrack(task) => task.execute(inserts).map(TaskResult::Track),
            Task::AnalyzeAlbum(task) => {
                task.execute(inserts);
                Ok(TaskResult::Album)
            }
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
pub struct TaskQueue<'a> {
    index: &'a MemoryMetaIndex,
    tasks: Vec<AlbumTask>,
    pub status: &'a mut Status,
    pub status_sender: &'a mut SyncSender<Status>,
}

impl<'a> TaskQueue<'a> {
    pub fn new(
        index: &'a MemoryMetaIndex,
        status: &'a mut Status,
        status_sender: &'a mut SyncSender<Status>,
    ) -> TaskQueue<'a> {
        TaskQueue {
            tasks: Vec::new(),
            index,
            status,
            status_sender,
        }
    }

    /// Add a task to analyze the loudness of the given album and its tracks.
    ///
    /// Also updates the `*_to_process_loudness` fields in the status, but does
    /// not publish a status update.
    pub fn push_task_album(&mut self, album_id: AlbumId) {
        let tracks = self.index.get_album_tracks(album_id);
        let task = AlbumTask {
            album_id: album_id,
            tracks_pending: tracks.iter().map(|(id, _)| *id).collect(),
            tracks_done: Vec::with_capacity(tracks.len()),
            num_tracks: tracks.len(),
        };
        self.status.albums_to_process_loudness += 1;
        self.status.tracks_to_process_loudness += tracks.len() as u64;
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
        // TODO: Instead, enumerate both the index and the database in
        // parallel, and do a merge-diff.
        // TODO: This will not invalidate loudness after replacing the
        // tracks. We would need to store an mtime, or the id of the file
        // row in the database for that.
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

                if db.select_track_waveform(*track_id)?.is_none() {
                    self.push_task_album(*album_id);
                    continue 'albums
                }
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
    fn get_next_task(&mut self, prev_result: TaskResult) -> Option<Task> {
        match prev_result {
            TaskResult::Track(track_result) => {
                self.finish_track(track_result.album_id, track_result.meters);
                self.status.tracks_processed_loudness += 1;
                self.status_sender.send(self.status.clone()).unwrap();
            }
            TaskResult::Album => {
                self.status.albums_processed_loudness += 1;
                self.status_sender.send(self.status.clone()).unwrap();
            }
            TaskResult::None => {}
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
    /// The thread pool more threads than cores on the system, to to ensure that
    /// we can saturate all cores if IO to read the tracks is slow. This method
    /// blocks until processing is done.
    pub fn process_all_in_thread_pool(
        self,
        db_path: &Path,
    ) -> error::Result<()> {
        // Even if we have nothing to do, we will vacuum the database, which can
        // take a few hundred milliseconds, and we'd rather not do that if it is
        // not necessary, so exit early if we can.
        if self.is_done() {
            return Ok(())
        }

        let task_queue = Arc::new(Mutex::new(self));

        // TODO: Share this thread pool with the thumbnail generation pool.

        crossbeam::scope::<_, error::Result<()>>(|scope| {
            // Use as many threads as the CPU has threads, so that in theory we
            // can keep it busy. But in practice, all of this is going to be
            // severely IO-bound with a fast CPU and a spinning disk.
            let n_threads = num_cpus::get();
            let mut threads:
                Vec<crossbeam::ScopedJoinHandle<error::Result<()>>> =
                Vec::with_capacity(n_threads);

            // Make a channel where we receive database writes. Previously every
            // thread would have its own database connection and they would all
            // write, but this lead to contention and "database is locked"
            // errors. So instead we let all threads send their inserts to this
            // channel, and one thread will serialize all writes.
            let (insert_sender, insert_receiver) = sync_channel(32);

            for i in 0..n_threads {
                let task_queue_i = task_queue.clone();
                let mut sender_i = insert_sender.clone();
                let process = move || {
                    // Run the thread until there is no more task to execute. If
                    // there is currently no task, it doesn't mean there will be no
                    // tasks in the future, but those future tasks can only appear
                    // after finishing an existing one, so this thread is no longer
                    // useful.
                    let mut prev_result = TaskResult::None;
                    loop {
                        let task = {
                            let mut queue = task_queue_i.lock().unwrap();
                            match queue.get_next_task(prev_result) {
                                Some(task) => task,
                                None => break,
                            }
                        };
                        prev_result = task.execute(&mut sender_i)?;
                    }

                    Ok(())
                };
                let join_handle = scope
                    .builder()
                    .name(format!("Loudness {}", i))
                    .spawn(process)
                    .expect("Failed to spawn OS thread.");
                threads.push(join_handle);
            }

            // While those threads are running the loudness analysis, this
            // thread can do the database inserts for them. This function
            // returns when all senders have closed their channel, which happens
            // after all threads exit.
            std::mem::drop(insert_sender);
            process_inserts(db_path, insert_receiver)?;

            for join_handle in threads.drain(..) {
                // The first unwrap is on joining, that should not fail because we
                // set panic=abort. The ? then propagates any errors that might have
                // happened in the thread.
                join_handle.join()?;
            }
            Ok(())
        })?;

        // We shouldn't have exited all threads before the work was done.
        assert!(task_queue.lock().unwrap().is_done());

        Ok(())
    }
}
