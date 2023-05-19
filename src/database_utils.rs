// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Interaction with Musium's SQLite database.

use std::path::Path;

pub type Result<T> = sqlite::Result<T>;

fn connect_internal<P: AsRef<Path>>(
    path: P,
    flags: sqlite::OpenFlags,
) -> Result<sqlite::Connection> {
    // We use set_no_mutex, because the the connection will not be shared among
    // different threads.
    let flags = flags.set_no_mutex();
    let mut connection = sqlite::Connection::open_with_flags(path, flags)?;
    let timeout_ms = 10_000;
    connection.set_busy_timeout(timeout_ms)?;
    // Use the faster WAL mode, see https://www.sqlite.org/wal.html.
    connection.execute("PRAGMA journal_mode = WAL;")?;
    connection.execute("PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}

pub fn connect_readonly<P: AsRef<Path>>(path: P) -> Result<sqlite::Connection> {
    let flags = sqlite::OpenFlags::new().set_read_only();
    connect_internal(path, flags)
}

pub fn connect_read_write<P: AsRef<Path>>(path: P) -> Result<sqlite::Connection> {
    let flags = sqlite::OpenFlags::new().set_read_write().set_create();
    connect_internal(path, flags)
}
