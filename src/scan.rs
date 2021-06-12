// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Scanning the library directory, putting metadata into SQLite.

use std::ffi::OsStr;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::path::{Path, PathBuf};
use std::os::unix::fs::MetadataExt;

use walkdir;

use crate::database::{Database, Mtime};

pub enum ScanStage {
    /// Discovering flac files in the library path.
    Discovering = 0,
    /// Determining which files to process. `status.files_discovered` is now final.
    PreProcessing = 1,
    /// Processing files. `status.files_to_process` is now final.
    Processing = 2,
    /// Done processing. `status.files_processed` is now final.
    Done = 3,
}

/// Counters to report progress during scanning.
///
/// All of these counters start out at 0 and increase over time. They never
/// decrease.
pub struct ScanStatus {
    /// Number of files found in the library.
    pub files_discovered: AtomicU64,

    /// Of the `files_discovered`, the number of files that need to be processed.
    pub files_to_process: AtomicU64,

    /// Of the `files_to_process`, the number processed so far.
    pub files_processed: AtomicU64,

    /// Current stage in the scanning process, `u8` value of `ScanStage`.
    pub stage: AtomicU8,
}

pub fn scan(
    database: &Database,
    path: &Path,
    status: &ScanStatus,
) {
    status.stage.store(ScanStage::Discovering as u8, Ordering::SeqCst);
    let mut files_current = enumerate_flac_files(path, status);

    status.stage.store(ScanStage::PreProcessing as u8, Ordering::SeqCst);
    files_current.sort();
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
    status: &ScanStatus,
) -> Vec<(PathBuf, Mtime)> {
    let flac_ext = OsStr::new("flac");

    walkdir::WalkDir::new(path)
        .follow_links(true)
        .max_open(128)
        .into_iter()
        .filter_entry(|e|
            true
            && e.file_type().is_file()
            && e.path().extension() == Some(flac_ext)
        )
        .filter_map(|e| match e {
            Ok(entry) => {
                match entry.metadata() {
                    Ok(m) => {
                        // Increment the counter in the status, so we can follow
                        // progress live.
                        status.files_discovered.fetch_add(1, Ordering::SeqCst);
                        Some((entry.into_path(), Mtime(m.mtime())))
                    },
                    // TODO: Add a nicer way to report errors.
                    Err(err) => { eprintln!("{}", err); None }
                }
            }
            Err(err) => { eprintln!("{}", err); None }
        })
        .collect()
}
