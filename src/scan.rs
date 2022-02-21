// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Scanning the library directory, putting metadata into SQLite.
//!
//! Musium implements a two-stage process to indexing:
//!
//! 1. Find all flac files in the library path, and put their tags in SQLite.
//! 2. Read the tags from SQLite and build a contistent index from them.
//!
//! This module implements step 1. Using an intermediate step has a few
//! advantages:
//!
//! * Scanning is decoupled from running the server, we can update whenever we
//!   want, not just at server startup.
//! * Conversely, starting the server does not have to wait for scanning all
//!   files. Reading rows from SQLite is much faster than opening every file
//!   individually and reading the first few kilobytes, because it performs
//!   mostly sequential IO in a single file, instead of very random IO over many
//!   many different files all over the place.
//! * We can do incremental updates. We don't have to read the tags from files
//!   that haven't changed.

use std::thread::JoinHandle;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};

use walkdir;

use crate::config::Config;
use crate::database::{Database, FileMetadataInsert, FileMetaId};
use crate::database;
use crate::error;
use crate::loudness;
use crate::mvar::{MVar, Var};
use crate::prim::Mtime;
use crate::thumb_cache::ThumbCache;
use crate::{MetaIndex, MemoryMetaIndex};

type FlacReader = claxon::FlacReader<fs::File>;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ScanStage {
    /// Discovering flac files in the library path.
    Discovering= 0,

    /// Determining which files to process.
    ///
    /// `status.files_discovered` is now final.
    PreProcessingMetadata = 1,

    /// Reading metadata from files.
    ///
    /// `status.files_to_process_metadata` is now final.
    ExtractingMetadata = 2,

    /// Joining all metadata into an in-memory index.
    ///
    /// `status.files_processed_metadata` is now final.
    IndexingMetadata = 3,

    /// Determining which files to analyze loudness for.
    PreProcessingLoudness = 4,

    /// Analyzing loudness and track waveforms.
    ///
    /// `status.tracks_to_process_loudness` and
    /// `status.albums_to_process_loudness` are now final.
    AnalyzingLoudness = 5,

    /// Determining which thumbnails to generate.
    ///
    /// `status.tracks_processed_loudness` and
    /// `status.albums_processed_loudness` are now final.
    PreProcessingThumbnails = 6,

    /// Generating thumbnails.
    ///
    /// `status.files_to_process_thumbnails` is now final.
    GeneratingThumbnails = 7,

    /// Loading thumbnails.
    ///
    /// `status.files_to_process_thumbnails` is now final.
    LoadingThumbnails = 8,

    /// Done.
    Done = 9,
}

/// Counters to report progress during scanning.
///
/// All of these counters start out at 0 and increase over time. They never
/// decrease.
#[derive(Copy, Clone, Debug)]
pub struct Status {
    /// Current stage in the scanning process.
    pub stage: ScanStage,

    /// Number of files found in the library.
    pub files_discovered: u64,

    /// Of the `files_discovered`, the number of files that need to be processed.
    pub files_to_process_metadata: u64,

    /// Of the `files_to_process_metadata`, the number processed so far.
    pub files_processed_metadata: u64,

    /// The number of tracks that need their loudness analyzed.
    pub tracks_to_process_loudness: u64,

    /// The number of tracks for which loudness has been analyzed.
    pub tracks_processed_loudness: u64,

    /// The number of albums that need their loudness analyzed.
    pub albums_to_process_loudness: u64,

    /// The number of albums for which their loudness has been analyzed.
    pub albums_processed_loudness: u64,

    /// The number of files for which we need to generate a thumbnail.
    pub files_to_process_thumbnails: u64,

    /// Of the `files_to_process_thumbnails`, the number processed so far.
    pub files_processed_thumbnails: u64,
}

impl Status {
    pub fn new() -> Status {
        Status {
            stage: ScanStage::Discovering,
            files_discovered: 0,
            files_to_process_metadata: 0,
            files_processed_metadata: 0,
            tracks_to_process_loudness: 0,
            tracks_processed_loudness: 0,
            albums_to_process_loudness: 0,
            albums_processed_loudness: 0,
            files_to_process_thumbnails: 0,
            files_processed_thumbnails: 0,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::cmp::Ordering;
        let indicator = |target| match self.stage.cmp(&target) {
            Ordering::Less => ' ',
            Ordering::Equal => '>',
            Ordering::Greater => '•',
        };
        writeln!(
            f,
            "{} Discovering files:     {}",
            indicator(ScanStage::Discovering),
            self.files_discovered,
        )?;
        writeln!(
            f,
            "{} Extracting metadata:   {} of {} files",
            indicator(ScanStage::ExtractingMetadata),
            self.files_processed_metadata,
            self.files_to_process_metadata,
        )?;
        writeln!(
            f,
            "{} Indexing metadata",
            indicator(ScanStage::IndexingMetadata),
        )?;
        writeln!(
            f,
            "{} Analyzing loudness:    {} of {} tracks, {} of {} albums",
            indicator(ScanStage::AnalyzingLoudness),
            self.tracks_processed_loudness,
            self.tracks_to_process_loudness,
            self.albums_processed_loudness,
            self.albums_to_process_loudness,
        )?;
        writeln!(
            f,
            "{} Generating thumbnails: {} of {} albums",
            indicator(ScanStage::GeneratingThumbnails),
            self.files_processed_thumbnails,
            self.files_to_process_thumbnails,
        )?;
        writeln!(
            f,
            "{} Loading thumbnails",
            indicator(ScanStage::LoadingThumbnails),
        )?;
        Ok(())
    }
}

pub fn scan(
    connection: &sqlite::Connection,
    library_path: &Path,
    status: &mut Status,
    status_sender: &mut SyncSender<Status>,
) -> database::Result<()> {
    let mut db = Database::new(&connection)?;

    let mut files_current = enumerate_flac_files(library_path, status_sender, status);

    status.stage = ScanStage::PreProcessingMetadata;
    status_sender.send(*status).unwrap();

    // Sort the files in memcpm order. The default Ord instance of PathBuf is
    // not what we want, it orders / before space (presumably because it does
    // tuple ordering on the path segments?). Memcmp order matches SQLite's
    // default string ordering.
    files_current.sort_by(|a, b| a.0.as_os_str().cmp(b.0.as_os_str()));

    let mut rows_to_delete = Vec::new();
    let mut paths_to_scan = Vec::new();
    get_updates(
        files_current,
        &mut db,
        &mut rows_to_delete,
        &mut paths_to_scan,
    )?;

    status.stage = ScanStage::ExtractingMetadata;
    status.files_to_process_metadata = paths_to_scan.len() as u64;
    status_sender.send(*status).unwrap();

    // Delete rows for outdated files, we will insert new rows below.
    delete_outdated_file_metadata(&mut db, &rows_to_delete)?;

    // Format the current time, we store this in the `imported_at` column in the
    // `file_metadata` table.
    let now = chrono::Utc::now();
    let use_zulu_suffix = true;
    let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, use_zulu_suffix);

    insert_file_metadata_for_paths(
        &mut db,
        &paths_to_scan[..],
        &now_str,
        status_sender,
        status,
    )?;

    // If we deleted anything vacuum the database to ensure it's packed tightly
    // again. Deletes are expected to be infrequent and the database is expected
    // to be small (a few megabytes*), so the additional IO is not an issue.
    // *Now that we also store loudness info and waveform data, it has grown to
    // a few dozen megabytes. But still, it should be fine to vacuum after a
    // scan.
    if rows_to_delete.len() > 0 {
        db.connection.execute("VACUUM")?;
    }

    Ok(())
}

/// Enumerate all flac files and their mtimes.
///
/// The order of the result is unspecified.
///
/// In a past investigation, before SQLite was used as an intermediate step, it
/// turned out that enumerating all files, collecting them into a vector, and
/// then scanning from the vector, is faster than iterating the walkdir on the
/// go. See also `docs/performance.md`. Now that we use SQLite as intermediate
/// step, it is convenient to have the vector, to compute the set difference,
/// in order to determine which files need to be scanned.
pub fn enumerate_flac_files(
    path: &Path,
    status_sender: &mut SyncSender<Status>,
    status: &mut Status,
) -> Vec<(PathBuf, Mtime)> {
    let flac_ext = OsStr::new("flac");

    let result = walkdir::WalkDir::new(path)
        .follow_links(true)
        .max_open(128)
        .into_iter()
        .filter_map(|e| match e {
            Ok(entry) => {
                let is_flac = true
                    && entry.file_type().is_file()
                    && entry.path().extension() == Some(flac_ext);

                match entry.metadata() {
                    Ok(m) if is_flac => {
                        // Increment the counter in the status, so we can follow
                        // progress live. Occasionally also send the status, but
                        // don't do this too often, because then we'd spend more
                        // time printing status than scanning files.
                        status.files_discovered += 1;

                        // Report at increments of 32 because those are cheap to
                        // test for. Also, because 32 % 10 == 2, we cover all
                        // even digits for the last digit, which masks a bit
                        // that we are not reporting all statuses.
                        if status.files_discovered % 32 == 0 {
                            status_sender.send(*status).unwrap();
                        }

                        Some((entry.into_path(), Mtime(m.mtime())))
                    },
                    Ok(_not_flac) => None,
                    // TODO: Add a nicer way to report errors.
                    Err(err) => { eprintln!("{}", err); None }
                }
            }
            Err(err) => { eprintln!("{}", err); None }
        })
        .collect();

    // Send the final discovery status, because we may have discovered some new
    // files since the last update.
    status_sender.send(*status).unwrap();

    result
}

/// Given the current files, and the database, figure out their difference.
///
/// Any files present in the database, but not present currently, should be
/// removed and end up in `rows_to_delete`. Any files present currently, but
/// not in the database, should be added and end up in `paths_to_scan`. Files
/// that are present in both, but with a different mtime, end up in both.
pub fn get_updates(
    current_sorted: Vec<(PathBuf, Mtime)>,
    db: &mut Database,
    rows_to_delete: &mut Vec<FileMetaId>,
    paths_to_scan: &mut Vec<(PathBuf, Mtime)>,
) -> database::Result<()> {
    let mut iter_curr = current_sorted.into_iter();
    let mut iter_db = db
        .iter_file_metadata_filename_mtime()?
        .map(|result|
            result.map(|row| (
                FileMetaId(row.id),
                PathBuf::from(row.filename),
                Mtime(row.mtime)
            ))
        );

    let mut val_curr = iter_curr.next();
    let mut val_db = iter_db.next();

    // Iterate in merge-join style over the two iterators, which we can do
    // because they are sorted. The SQLite "order by" uses the default "binary"
    // collation, which is memcmp order on the UTF-8 bytes. The same is true for
    // our `Path` sorting here, because we sorted it with `.as_os_str()`, which
    // then has memcmp order as well. Note that we need to be careful to use
    // `.as_os_str()` here too: if we use Path's Ord instance, then it orders by
    // path segment, which does not match memcmp order!
    loop {
        match (val_curr.take(), val_db.take()) {
            (Some((p0, m0)), Some(Ok((id, p1, m1)))) => {
                if p0.as_os_str() > p1.as_os_str() {
                    // P1 is in the database, but not the filesystem.
                    rows_to_delete.push(id);
                    val_curr = Some((p0, m0));
                    val_db = iter_db.next();
                } else if p0.as_os_str() < p1.as_os_str() {
                    // P0 is in the filesystem, but not in the database.
                    paths_to_scan.push((p0, m0));
                    val_curr = iter_curr.next();
                    val_db = Some(Ok((id, p1, m1)));
                } else if m0 != m1 {
                    // The path matches, but the mtimes differ.
                    rows_to_delete.push(id);
                    paths_to_scan.push((p0, m0));
                    val_curr = iter_curr.next();
                    val_db = iter_db.next();
                } else {
                    // The path and mtimes match, we can skip this file.
                    val_curr = iter_curr.next();
                    val_db = iter_db.next();
                }
            }
            (None, Some(Ok((id, _, _, )))) => {
                rows_to_delete.push(id);
                val_db = iter_db.next();
            }
            (Some(path_mtime), None) => {
                paths_to_scan.push(path_mtime);
                val_curr = iter_curr.next();
            }
            (None, None) => break,
            (_, Some(Err(err))) => return Err(err),
        }
    }

    Ok(())
}

pub fn delete_outdated_file_metadata(
    db: &mut Database,
    rows_to_delete: &[FileMetaId],
) -> database::Result<()> {
    db.connection.execute("BEGIN")?;

    for row in rows_to_delete {
        db.delete_file_metadata(*row)?;
    }

    db.connection.execute("COMMIT")
}

pub fn insert_file_metadata_for_paths(
    db: &mut Database,
    paths_to_scan: &[(PathBuf, Mtime)],
    now_str: &str,
    status_sender: &mut SyncSender<Status>,
    status: &mut Status,
) -> database::Result<()> {
    use std::sync::mpsc::sync_channel;
    // When we are IO bound, we need enough threads to keep the IO scheduler
    // queues fed, so it can schedule optimally and minimize seeks. Therefore,
    // pick a fairly high amount of threads. When we are CPU bound, there is
    // some overheads to more threads, but 8 threads vs 64 threads is a
    // difference of maybe 0.05 seconds for 16k tracks, while for the IO-bound
    // case, it can bring down the time from ~140 seconds to ~70 seconds, which
    // is totally worth it.
    let num_threads = 64;

    // We are going to have many threads read files, but only this thread will
    // insert them into the database (because the database is not `Send`). If
    // the database and files to read live on the same disk, it could take some
    // time to insert, so ensure that we have enough of a buffer to not make the
    // reader threads idle.
    let (tx_file, rx_file) = sync_channel(num_threads * 10);

    // Threads will take the next path to scan, and this is the index to take it
    // from.
    let counter = std::sync::atomic::AtomicUsize::new(0);

    // A note about error handling below: we `.expect` any SQLite failure
    // immediately; there is no way to exit early from this function except for
    // panicking. This is because exiting the scope will join any threads that
    // are still running, and those threads will block trying to send their
    // files into the tx end of the channel, but nothing is consuming the rx end
    // of the channel.
    crossbeam::scope(|scope| {
        for i in 0..num_threads {
            let tx = tx_file.clone();
            let counter_ref = &counter;
            scope
                .builder()
                .name(format!("read_files_{}", i))
                .spawn(move || read_files(paths_to_scan, counter_ref, tx))
                .expect("Failed to spawn OS thread.");
        }

        // After spawning all threads, close the original sender. This way, we
        // will know that all threads are done, if we get an error on the
        // receiving side.
        std::mem::drop(tx_file);

        // Do all inserts inside a transaction for better insert performance.
        // Previously I also did COMMIT and BEGIN periodically inside the loop,
        // to ensure that less work is wasted if you kill scanning half-way.
        // However, the commit triggers IO that goes in the queue with dozens of
        // reads, so it can take a long time to finish (multiple seconds on a
        // spinning disk). This then makes progress reporting very choppy: it is
        // stuck at the last number reported before commit, then after this
        // thread unblocks, it races through all data that has been buffered
        // since the last commit, and it blocks again. I don't like this choppy
        // progress while scanning proceeds smoothly in the background, so don't
        // do intermediate commits, just put everything in a single transaction.
        // Scanning does not take *that* long anyway, for me it takes a few
        // minutes for 17k files on a spinning disk, so if you kill it early,
        // just restart from scratch, the loss is not that big.
        db.connection.execute("BEGIN")?;

        for (i, flac_reader) in rx_file.iter() {
            let (ref path, mtime) = paths_to_scan[i];
            insert_file_metadata(db, now_str, &path, mtime, flac_reader)?;

            // Keep the status up to date, and send it once in a while. We send
            // it more often here than when enumerating files, because reading
            // the files is more IO-intensive than enumerating them, so this
            // is slower, so the overhead of updating more often is relatively
            // small.
            status.files_processed_metadata += 1;
            if status.files_processed_metadata % 8 == 0 {
                status_sender.send(*status).unwrap();
            }
        }

        // Sanity check: did we get everything? Every thread should have
        // incremented once without sending, and we have one increment per
        // processed file for files that did get processed.
        assert_eq!(counter.load(Ordering::SeqCst), paths_to_scan.len() + num_threads);

        db.connection.execute("COMMIT")?;

        // Send the final discovery status, we may have processed some files
        // since the last update.
        status_sender.send(*status).unwrap();

        Ok(())
    })
}

/// Read files from `paths` as long as `counter` is less than `paths.len()`,
/// send them through the sender.
fn read_files(
    paths: &[(PathBuf, Mtime)],
    counter: &AtomicUsize,
    sender: SyncSender<(usize, FlacReader)>,
) {
    loop {
        let i = counter.fetch_add(1, Ordering::SeqCst);
        if i >= paths.len() {
            break;
        }
        let (path, _mtime) = &paths[i];
        let opts = claxon::FlacReaderOptions {
            metadata_only: true,
            read_picture: claxon::ReadPicture::Skip,
            read_vorbis_comment: true,
        };
        let reader = match claxon::FlacReader::open_ext(path, opts) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Failure while reading {:?}: {}", path, err);
                continue;
            }
        };
        sender.send((i, reader)).unwrap();
    }
}

/// Insert a row in the `file_metadata` table for the given flac file.
fn insert_file_metadata(
    db: &mut Database,
    now_str: &str,
    path: &Path,
    mtime: Mtime,
    flac_reader: FlacReader,
) -> database::Result<()> {
    let path_utf8 = match path.to_str() {
        Some(s) => s,
        None => {
            eprintln!("Warning: Path {:?} is not valid UTF-8. Skipping.", path);
            return Ok(())
        }
    };

    // Start with all fields that are known from the streaminfo, with tags
    // unfilled.
    let streaminfo = flac_reader.streaminfo();
    let mut m = FileMetadataInsert {
        filename: path_utf8,
        mtime: mtime.0,
        imported_at: now_str,

        streaminfo_channels: streaminfo.channels as i64,
        streaminfo_bits_per_sample: streaminfo.bits_per_sample as i64,
        streaminfo_num_samples: streaminfo.samples.map(|x| x as i64),
        streaminfo_sample_rate: streaminfo.sample_rate as i64,

        tag_album: None,
        tag_albumartist: None,
        tag_albumartistsort: None,
        tag_artist: None,
        tag_musicbrainz_albumartistid: None,
        tag_musicbrainz_albumid: None,
        tag_musicbrainz_trackid: None,
        tag_discnumber: None,
        tag_tracknumber: None,
        tag_originaldate: None,
        tag_date: None,
        tag_title: None,
        tag_bs17704_track_loudness: None,
        tag_bs17704_album_loudness: None
    };

    // Then walk all tags, and set the corresponding column if we find a known one.
    for (tag, value) in flac_reader.tags() {
        match &tag.to_ascii_lowercase()[..] {
            "album"                     => m.tag_album = Some(value),
            "albumartist"               => m.tag_albumartist = Some(value),
            "albumartistsort"           => m.tag_albumartistsort = Some(value),
            "artist"                    => m.tag_artist = Some(value),
            "discnumber"                => m.tag_discnumber = Some(value),
            "musicbrainz_albumartistid" => m.tag_musicbrainz_albumartistid = Some(value),
            "musicbrainz_albumid"       => m.tag_musicbrainz_albumid = Some(value),
            "musicbrainz_trackid"       => m.tag_musicbrainz_trackid = Some(value),
            "originaldate"              => m.tag_originaldate = Some(value),
            "date"                      => m.tag_date = Some(value),
            "title"                     => m.tag_title = Some(value),
            "tracknumber"               => m.tag_tracknumber = Some(value),
            "bs17704_track_loudness"    => m.tag_bs17704_track_loudness = Some(value),
            "bs17704_album_loudness"    => m.tag_bs17704_album_loudness = Some(value),
            _ => {}
        }
    }

    db.insert_file_metadata(m)
}

pub fn run_scan_in_thread(
    config: &Config,
    index_var: Var<MemoryMetaIndex>,
    thumb_cache_var: Var<ThumbCache>,
) -> (
    JoinHandle<error::Result<()>>,
    Receiver<Status>,
) {
    // Status updates should print much faster than they are produced, so use
    // a small buffer for them.
    let (mut tx, rx) = std::sync::mpsc::sync_channel(15);

    let db_path = config.db_path();
    let library_path = config.library_path.clone();
    let covers_path = config.covers_path.clone();

    let scan_thread = std::thread::Builder::new()
        .name("scan".to_string())
        .spawn(move || {
            let mut status = Status::new();

            let connection = sqlite::open(&db_path)?;

            // Scan all files, put the metadata in the database.
            scan(
                &connection,
                &library_path,
                &mut status,
                &mut tx,
            )?;

            status.stage = ScanStage::IndexingMetadata;
            tx.send(status).unwrap();

            // Build a new index from the latest data in the database. Then
            // immediately publish that new index so it can be accessed by the
            // webinterface, even before the thumbnails are ready (because
            // generating those may take a while).
            let (index, builder) = MemoryMetaIndex::from_database(&db_path)?;
            let index_arc = Arc::new(index);
            index_var.set(index_arc.clone());

            // TODO: Move issue reporting to a better place. Maybe take the builder and
            // index as an argument to this method.
            eprintln!();
            for issue in &builder.issues {
                eprintln!("{}", issue);
            }
            eprintln!("\n\n\n");

            {
                status.stage = ScanStage::PreProcessingLoudness;
                tx.send(status).unwrap();

                let mut loudness_tasks = loudness::TaskQueue::new(
                    &*index_arc,
                    &mut status,
                    &mut tx,
                );
                let mut db = Database::new(&connection)?;
                loudness_tasks.push_tasks_missing(&mut db)?;
                loudness_tasks.status.stage = ScanStage::AnalyzingLoudness;
                loudness_tasks.status_sender.send(*loudness_tasks.status).unwrap();

                loudness_tasks.process_all_in_thread_pool(&db_path)?;
            }

            // If there are any new or updated albums, regenerate thumbnails for
            // those.
            crate::thumb_gen::generate_thumbnails(
                &*index_arc,
                &builder,
                &covers_path,
                &mut status,
                &mut tx,
            )?;

            status.stage = ScanStage::LoadingThumbnails;
            tx.send(status).unwrap();

            // Load the new set of thumbnails, publish them to the webinterface.
            let thumb_cache = ThumbCache::new(
                index_arc.get_album_ids_ordered_by_artist(),
                &covers_path,
            )?;
            let thumb_cache_arc = Arc::new(thumb_cache);
            thumb_cache_var.set(thumb_cache_arc);

            status.stage = ScanStage::Done;
            tx.send(status).unwrap();
            Ok(())
        })
        .expect("Failed to spawn scan thread.");

    (scan_thread, rx)
}

/// A scan that is happening in a background thread.
struct BackgroundScan {
    /// The most recent scan status.
    status: Arc<MVar<Status>>,

    /// Thread that watches the scan and writes new values to `status`.
    ///
    /// The actual scan runs in yet another thread, and it sends status updates
    /// over a channel, to allow for reactive UIs. However, a background scan
    /// triggered by the webinterface, the webinterface polls the status, we
    /// don’t push. So the supervisor thread sits here, listening for status
    /// updates, and it writes them to the `status` mutex when there is one.
    _supervisor: JoinHandle<()>,
}

impl BackgroundScan {
    pub fn new(
        config: Config,
        index_var: Var<MemoryMetaIndex>,
        thumb_cache_var: Var<ThumbCache>,
    ) -> Self {
        let status = Arc::new(MVar::new(Status::new()));

        let status_for_supervisor = status.clone();
        let supervisor = std::thread::Builder::new()
            .name("scan_supervisor".to_string())
            .spawn(move || {
                let status = status_for_supervisor;
                let (scan_thread, rx) = run_scan_in_thread(
                    &config,
                    index_var,
                    thumb_cache_var,
                );
                for new_status in rx {
                    status.set(new_status);
                }
                scan_thread
                    .join()
                    .expect("Scan thread panicked.")
                    .expect("Scan failed.");

                let final_status = status.get();
                assert_eq!(
                    final_status.stage,
                    ScanStage::Done,
                    "Final status update should be Done after scan thread exits.",
                );
            })
        .expect("Failed to spawn scan supervisor thread.");

        Self {
            status,
            _supervisor: supervisor,
        }
    }

    /// Return a copy of the current status.
    pub fn get_status(&self) -> Status {
        self.status.get()
    }
}

pub struct BackgroundScanner {
    background_scan: Mutex<Option<BackgroundScan>>,

    /// The latest index.
    ///
    /// The scanner replaces the inner value when the scan is complete.
    index_var: Var<MemoryMetaIndex>,

    /// The latest thumb cache.
    ///
    /// The scanner replaces the inner value when thumb generation is complete.
    thumb_cache_var: Var<ThumbCache>,
}

impl BackgroundScanner {
    pub fn new(
        index_var: Var<MemoryMetaIndex>,
        thumb_cache_var: Var<ThumbCache>,
    ) -> Self {
        Self {
            background_scan: Mutex::new(None),
            index_var: index_var,
            thumb_cache_var: thumb_cache_var,
        }
    }

    /// Start a new scan, if no scan is running at the moment.
    ///
    /// Returns the status of the scan that's in progress.
    pub fn start(&self, config: Config) -> Status {
        let mut bg_scan = self.background_scan.lock().unwrap();

        // If there is an existing scan, we don't need to start a new one,
        // unless the previous scan is already done.
        if let Some(ref sc) = *bg_scan {
            let status = sc.get_status();
            match status.stage {
                ScanStage::Done => { /* We need to start a new scan. */ },
                _ => return status,
            }
        }

        let new_scan = BackgroundScan::new(
            config,
            self.index_var.clone(),
            self.thumb_cache_var.clone(),
        );
        let status = new_scan.get_status();
        *bg_scan = Some(new_scan);

        status
    }

    /// Return the status of the current scan, if any.
    pub fn get_status(&self) -> Option<Status> {
        self.background_scan.lock().unwrap().as_ref().map(|sc| sc.get_status())
    }
}

#[cfg(test)]
mod test {
    use crate::database::{Database, FileMetaId};
    use super::{Mtime, get_updates};
    use std::path::PathBuf;

    #[test]
    fn get_updates_empty_db() {
        // In this case we have an empty database but a non-empty file system,
        // so we expect all files to be scanned.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();

        let current_sorted = vec![
            (PathBuf::from("/foo/bar.flac"), Mtime(1)),
            (PathBuf::from("/foo/baz.flac"), Mtime(1)),
        ];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(
            &paths_to_scan[..],
            &[
                (PathBuf::from("/foo/bar.flac"), Mtime(1)),
                (PathBuf::from("/foo/baz.flac"), Mtime(1)),
            ],
        );
        assert_eq!(&rows_to_delete, &Vec::<FileMetaId>::new());
    }

    #[test]
    fn get_updates_nothing_changed() {
        // In this case nothing changed on the file system with respect to the
        // database, so we expect no files to be scanned and no rows deleted.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();
        db.connection.execute(
            "
            insert into
              file_metadata
                ( filename
                , mtime
                , imported_at
                , streaminfo_channels
                , streaminfo_bits_per_sample
                , streaminfo_sample_rate
                )
            values
              ('/foo/baz.flac', 1, 'N/A', 0, 0, 0),
              ('/foo/bar.flac', 2, 'N/A', 0, 0, 0);
            "
        ).unwrap();


        let current_sorted = vec![
            (PathBuf::from("/foo/bar.flac"), Mtime(2)),
            (PathBuf::from("/foo/baz.flac"), Mtime(1)),
        ];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(&paths_to_scan, &Vec::<(PathBuf, Mtime)>::new());
        assert_eq!(&rows_to_delete, &Vec::<FileMetaId>::new());
    }

    #[test]
    fn get_updates_add_remove() {
        // One file was added on the file system, one was deleted.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();
        db.connection.execute(
            "
            insert into
              file_metadata
                ( id
                , filename
                , mtime
                , imported_at
                , streaminfo_channels
                , streaminfo_bits_per_sample
                , streaminfo_sample_rate
                )
            values
              (1, '/unchanged.flac', 1, 'N/A', 0, 0, 0),
              (2, '/deleted.flac', 2, 'N/A', 0, 0, 0),
              (3, '/also_deleted.flac', 3, 'N/A', 0, 0, 0),
              (4, '/z.flac', 4, 'N/A', 0, 0, 0);
            "
        ).unwrap();

        let current_sorted = vec![
            (PathBuf::from("/added.flac"), Mtime(3)),
            (PathBuf::from("/unchanged.flac"), Mtime(1)),
        ];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(&paths_to_scan[..], &[(PathBuf::from("/added.flac"), Mtime(3))]);
        assert_eq!(&rows_to_delete[..], &[FileMetaId(3), FileMetaId(2), FileMetaId(4)]);
    }

    #[test]
    fn get_updates_different_mtime() {
        // A file is present in both the file system and database, but the mtime
        // differs.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();
        db.connection.execute(
            "
            insert into
              file_metadata
                ( id
                , filename
                , mtime
                , imported_at
                , streaminfo_channels
                , streaminfo_bits_per_sample
                , streaminfo_sample_rate
                )
            values
              (1, '/file.flac', 100, 'N/A', 0, 0, 0);
            "
        ).unwrap();

        // Same path, but mtime is one more.
        let current_sorted = vec![(PathBuf::from("/file.flac"), Mtime(101))];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(&rows_to_delete[..], &[FileMetaId(1)]);
        assert_eq!(&paths_to_scan[..], &[(PathBuf::from("/file.flac"), Mtime(101))]);
    }

    #[test]
    fn get_updates_sort_order() {
        // The difference should be empty, but the sort order is not trivial
        // because it's not only ASCII.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();
        db.connection.execute(
            "
            insert into
              file_metadata
                ( id
                , filename
                , mtime
                , imported_at
                , streaminfo_channels
                , streaminfo_bits_per_sample
                , streaminfo_sample_rate
                )
            values
              (1, '/Étienne de Crécy/1.flac', 1, 'N/A', 0, 0, 0),
              (2, '/Eidola/1.flac', 1, 'N/A', 0, 0, 0);
            "
        ).unwrap();

        // Same path, but mtime is one more.
        let current_sorted = vec![
            (PathBuf::from("/Eidola/1.flac"), Mtime(1)),
            (PathBuf::from("/Étienne de Crécy/1.flac"), Mtime(1)),
        ];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(&paths_to_scan, &Vec::<(PathBuf, Mtime)>::new());
        assert_eq!(&rows_to_delete, &Vec::<FileMetaId>::new());
    }

    #[test]
    fn get_updates_memcmp_order() {
        // This test confirms that we order paths in memcmp order, not in path
        // component order. When we compare path order, a slash comes before a
        // space, and getting the updates reaches a wrong conclusion.
        let connection = sqlite::open(":memory:").unwrap();
        let mut db = Database::new(&connection).unwrap();
        db.connection.execute(
            "
            insert into
              file_metadata
                ( filename
                , mtime
                , imported_at
                , streaminfo_channels
                , streaminfo_bits_per_sample
                , streaminfo_sample_rate
                )
            values
              ('/foo/1/foo.flac', 1, 'N/A', 0, 0, 0);
            "
        ).unwrap();

        let current_sorted = vec![
            (PathBuf::from("/foo/1 take 2/bar.flac"), Mtime(2)),
            (PathBuf::from("/foo/1/foo.flac"), Mtime(1)),
        ];
        let mut rows_to_delete = Vec::new();
        let mut paths_to_scan = Vec::new();

        get_updates(
            current_sorted,
            &mut db,
            &mut rows_to_delete,
            &mut paths_to_scan,
        ).unwrap();

        assert_eq!(&paths_to_scan[..], &[
            (PathBuf::from("/foo/1 take 2/bar.flac"), Mtime(2))
        ]);
        assert_eq!(&rows_to_delete[..], &[]);
    }
}
