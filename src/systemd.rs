// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Minimal bindings to libsystemd.

use std::os::raw::{c_char, c_int};
use std::ffi::CStr;

#[link(name = "systemd")]
extern {
    fn sd_notify(unset_environment: c_int, state: *const c_char) -> c_int;
}

/// Check whether we could notify systemd.
///
/// Notifying the system daemon though libsystemd goes through a socket that
/// systemd passes in the NOTIFY_SOCKET environment variable, so if that
/// variable is not present, then we definitely can not notify systemd.
fn can_notify() -> bool {
    std::env::var("NOTIFY_SOCKET").is_ok()
}

/// Notify systemd.
///
/// Expects a string of newline-delimited key-value pairs in the form of
/// `KEY=value`. Standardized values are:
///
/// * `READY=1` to signal startup completion.
/// * `STATUS=message` to set a single-line status.
/// * `EXTEND_TIMEOUT_USEC={microseconds}` to request a longer time to start.
fn notify(kv_pairs: &CStr) -> Result<(), ()> {
    let unset_environment = 0; // False
    let result = unsafe {
        sd_notify(unset_environment, kv_pairs.as_ptr())
    };
    if result <= 0 {
        Err(())
    } else {
        Ok(())
    }
}

/// Signal readiness to systemd, if systemd is listening.
pub fn notify_ready_if_can_notify() {
    if can_notify() {
        let message = CStr::from_bytes_with_nul(b"READY=1\0").unwrap();
        notify(&message).expect("Failed to notify systemd of readiness.");
    }
}
