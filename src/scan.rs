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

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;

use walkdir;

use crate::database;
use crate::database::{Database, FileMetadata, FileMetaId, Mtime};

type FlacReader = claxon::FlacReader<fs::File>;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ScanStage {
    /// Discovering flac files in the library path.
    Discovering,
    /// Determining which files to process. `status.files_discovered` is now final.
    PreProcessing,
    /// Processing files. `status.files_to_process` is now final.
    Processing,
    /// Done processing. `status.files_processed` is now final.
    Done,
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
    pub files_to_process: u64,

    /// Of the `files_to_process`, the number processed so far.
    pub files_processed: u64,
}

impl Status {
    pub fn new() -> Status {
        Status {
            stage: ScanStage::Discovering,
            files_discovered: 0,
            files_to_process: 0,
            files_processed: 0,
        }
    }

    /// Take the maximum of two statuses.
    ///
    /// `ScanStatus` forms a monoid / join lattice / CRDT, and this is its merge
    /// operation.
    pub fn merge(&self, other: &Status) -> Status {
        Status {
            stage: self.stage.max(other.stage),
            files_discovered: self.files_discovered.max(other.files_discovered),
            files_to_process: self.files_to_process.max(other.files_to_process),
            files_processed: self.files_processed.max(other.files_processed),
        }
    }
}

pub fn scan(
    db_path: &Path,
    library_path: &Path,
    status_sender: &mut SyncSender<Status>,
) {
    let connection = sqlite::open(db_path).expect("Failed to open SQLite database.");
    database::ensure_schema_exists(&connection).expect("Failed to create schema in SQLite database.");
    let mut db = Database::new(&connection).expect("Failed to prepare SQLite statements.");

    let mut status = Status::new();
    let mut files_current = enumerate_flac_files(library_path, status_sender, &mut status);

    status.stage = ScanStage::PreProcessing;
    status_sender.send(status).unwrap();

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
    ).expect("Failed to query SQLite database for file metadata.");

    status.stage = ScanStage::Processing;
    status.files_to_process = paths_to_scan.len() as u64;
    status_sender.send(status).unwrap();

    // Delete rows for outdated files, we will insert new rows below.
    delete_outdated_file_metadata(&mut db, &rows_to_delete);

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
        &mut status,
    );

    // If we deleted anything vacuum the database to ensure it's packed tightly
    // again. Deletes are expected to be infrequent and the database is expected
    // to be small (a few megabytes), so the additional IO is not an issue.
    if rows_to_delete.len() > 0 {
        db
            .connection
            .execute("VACUUM")
            .expect("Failed to vacuum SQLite database.");
    }

    status.stage = ScanStage::Done;
    status_sender.send(status).unwrap();
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
    let mut iter_db = db.iter_file_metadata()?;

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
) {
    db
        .connection
        .execute("BEGIN")
        .expect("Failed to begin SQLite transaction.");

    for row in rows_to_delete {
        db
            .delete_file_metadata(*row)
            .expect("Failed to delete file_metadata row.");
    }

    db
        .connection
        .execute("COMMIT")
        .expect("Failed to commit SQLite transaction.");
}

pub fn insert_file_metadata_for_paths(
    db: &mut Database,
    paths_to_scan: &[(PathBuf, Mtime)],
    now_str: &str,
    status_sender: &mut SyncSender<Status>,
    status: &mut Status,
) {
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
    let (tx_file, rx_file) = sync_channel(num_threads * 4);

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
                .name(format!("File reading thread {}", i))
                .spawn(move || read_files(paths_to_scan, counter_ref, tx))
                .expect("Failed to spawn OS thread.");
        }

        // After spawning all threads, close the original sender. This way, we
        // will know that all threads are done, if we get an error on the
        // receiving side.
        std::mem::drop(tx_file);

        // Do all inserts inside a transaction for better insert performance.
        db.connection.execute("BEGIN").expect("Failed to begin SQLite transaction.");

        for (i, flac_reader) in rx_file.iter() {
            let (ref path, mtime) = paths_to_scan[i];
            insert_file_metadata(db, now_str, &path, mtime, flac_reader)
                .expect("Failed to insert file metadata in SQLite database.");

            // Keep the status up to date, and send it once in a while. We send
            // it more often here than when enumerating files, because reading
            // the files is more IO-intensive than enumerating them, so this
            // is slower, so the overhead of updating more often is relatively
            // small.
            status.files_processed += 1;
            if status.files_processed % 8 == 0 {
                status_sender.send(*status).unwrap();
            }

            // Break the inserts down into multiple transactions. This way we
            // can kill scanning, and we will not lose most of the files scanned
            // so far. The number here is a trade off between insert performance
            // and tolerance for wasted work if we stop early.
            // TODO: Tweak this number.
            if status.files_processed % 128 == 0 {
                db.connection.execute("COMMIT").expect("Failed to commit SQLite transaction.");
                db.connection.execute("BEGIN").expect("Failed to begin SQLite transaction.");
            }
        }

        // Sanity check: did we get everything? Every thread should have
        // incremented once without sending, and we have one increment per
        // processed file for files that did get processed.
        assert_eq!(counter.load(Ordering::SeqCst), paths_to_scan.len() + num_threads);

        db.connection.execute("COMMIT").expect("Failed to commit SQLite transaction.");

        // Send the final discovery status, we may have processed some files
        // since the last update.
        status_sender.send(*status).unwrap();
    });
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
                // TODO: Add a nicer way to report such errors.
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
            // TODO: Add a nicer way to report this error.
            eprintln!("Warning: Path {:?} is not valid UTF-8.", path);
            return Ok(())
        }
    };

    // Start with all fields that are known from the streaminfo, with tags
    // unfilled.
    let streaminfo = flac_reader.streaminfo();
    let mut m = FileMetadata {
        filename: path_utf8,
        mtime: mtime,
        imported_at: now_str,

        streaminfo_channels: streaminfo.channels,
        streaminfo_bits_per_sample: streaminfo.bits_per_sample,
        streaminfo_num_samples: streaminfo.samples,
        streaminfo_sample_rate: streaminfo.sample_rate,

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

#[cfg(test)]
mod test {
    use crate::database;
    use crate::database::{Database, FileMetaId, Mtime};
    use super::get_updates;
    use std::path::PathBuf;

    #[test]
    fn get_updates_empty_db() {
        // In this case we have an empty database but a non-empty file system,
        // so we expect all files to be scanned.
        let connection = sqlite::open(":memory:").unwrap();
        database::ensure_schema_exists(&connection).unwrap();
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
        database::ensure_schema_exists(&connection).unwrap();
        connection.execute(
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

        let mut db = Database::new(&connection).unwrap();

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
        database::ensure_schema_exists(&connection).unwrap();
        connection.execute(
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

        let mut db = Database::new(&connection).unwrap();

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
        database::ensure_schema_exists(&connection).unwrap();
        connection.execute(
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

        let mut db = Database::new(&connection).unwrap();

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
        database::ensure_schema_exists(&connection).unwrap();
        connection.execute(
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

        let mut db = Database::new(&connection).unwrap();

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
        database::ensure_schema_exists(&connection).unwrap();
        connection.execute(
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

        let mut db = Database::new(&connection).unwrap();

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
