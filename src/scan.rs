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
use std::path::{Path, PathBuf};
use std::os::unix::fs::MetadataExt;
use std::sync::mpsc::SyncSender;

use walkdir;

use crate::database;
use crate::database::{Database, FileMetaId, Mtime};

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

    files_current.sort();

    let mut rows_to_delete = Vec::new();
    let mut paths_to_scan = Vec::new();
    get_updates(
        files_current,
        &mut db,
        &mut rows_to_delete,
        &mut paths_to_scan,
    ).expect("Failed to query SQLite database for file_metadata.");

    eprintln!("{} rows to delete, {} paths to scan", rows_to_delete.len(), paths_to_scan.len());


    status.stage = ScanStage::Processing;
    status_sender.send(status).unwrap();

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
    paths_to_scan: &mut Vec<PathBuf>,
) -> database::Result<()> {
    let mut iter_curr = current_sorted.into_iter();
    let mut iter_db = db.iter_file_metadata()?;

    let mut val_curr = iter_curr.next();
    let mut val_db = iter_db.next();

    // Iterate in merge-join style over the two iterators, which we can do
    // because they are sorted. TODO: Confirm that their sort order matches.
    loop {
        match (val_curr.take(), val_db.take()) {
            (Some((p0, m0)), Some(Ok((id, p1, m1)))) => {
                if p0 > p1 {
                    // P1 is in the database, but not the filesystem.
                    rows_to_delete.push(id);
                    val_curr = Some((p0, m0));
                    val_db = iter_db.next();
                } else if p0 < p1 {
                    // P0 is in the filesystem, but not in the database.
                    paths_to_scan.push(p0);
                    val_curr = iter_curr.next();
                    val_db = Some(Ok((id, p1, m1)));
                } else if m0 != m1 {
                    // The path matches, but the mtimes differ.
                    rows_to_delete.push(id);
                    paths_to_scan.push(p0);
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
            (Some((p0, _)), None) => {
                paths_to_scan.push(p0);
                val_curr = iter_curr.next();
            }
            (None, None) => break,
            (_, Some(Err(err))) => return Err(err),
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::database;
    use crate::database::{Database, Mtime};
    use super::get_updates;
    use std::path::PathBuf;

    #[test]
    fn get_updates_empty_db() {
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
        );

        assert_eq!(
            &paths_to_scan[..],
            &[
                PathBuf::from("/foo/bar.flac"),
                PathBuf::from("/foo/baz.flac"),
            ],
        );
    }
}
