// Musium -- Music playback daemon with web-based library browser
// Copyright 2022 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for executing the pre-playback and post-idle command.
//!
//! The pre-playback and post-idle commands are handled by a thread that runs
//! indefinitely. Queue start and end events should be sent to this thread over
//! a channel. By using a separate thread, we ensure that:
//!
//! * The pre-playback and post-idle command never execute at the same time.
//! * The playback thread *can* wait for the pre-playback command to finish,
//!   but it doesn't have to, we can have it continue early.
//! * When playback resumes within the idle timeout, we can drop the post-idle
//!   and execute the pre-play right away.
//! * All events get processed in order. If playback resumes while we are
//!   executing the post-idle command, that is not an issue.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::{Condvar, Mutex};
use std::time::{Instant, Duration};
use std::process::Command;

use wait_timeout::ChildExt;

use crate::config::Config;

/// Events to send to the exec thread.
pub enum QueueEvent {
    /// Playback just started.
    ///
    /// After the pre-playback program has finished, the exec thread will set
    /// the mutex value to false and then signal the condvar.
    StartPlayback(Arc<(Mutex<bool>, Condvar)>),

    /// Playback ended at the given instant.
    EndPlayback(Instant),
}

fn execute_program_with_timeout(exe_path: &Path, stage_name: &'static str) {
    println!("Execute: stage={stage_name}, cmd={}", exe_path.to_string_lossy());
    let mut proc = match Command::new(exe_path).spawn() {
        Ok(proc) => proc,
        Err(err) => {
            println!(
                "Failed to spawn {} program {}: {}",
                stage_name,
                exe_path.to_string_lossy(),
                err,
            );
            return
        }
    };

    // Wait for the program to exit for some time. If it is not done by then,
    // kill it, and continue regardless of whether that was successful. There
    // is little more we can do then anyway.
    match proc.wait_timeout(Duration::from_secs(30)) {
        Ok(Some(_status)) => {
            println!("Execute: program exited, stage={stage_name}");
        }
        Ok(None) => {
            println!("The {} program did not exit within 30 seconds, killing it ...", stage_name);
            let _ignored_result = proc.kill();
        }
        Err(err) => {
            println!("Execute: failed to wait for program, stage={stage_name}, err={err}");
        }
    }
}

pub fn main(config: &Config, events: Receiver<QueueEvent>) -> ! {
    // Wait for playback to start.
    let mut start_event = events.recv().expect("QueueEvent sender should run indefinitely.");
    loop {
        let is_running_condvar = match start_event {
            QueueEvent::StartPlayback(arc) => arc,
            QueueEvent::EndPlayback(..) => panic!("Received EndPlayback before StartPlayback."),
        };

        if let Some(exe) = config.exec_pre_playback_path.as_ref() {
            execute_program_with_timeout(exe, "pre-playback");
        }

        // Signal to the playback thread that it can continue.
        *is_running_condvar.0.lock().unwrap() = false;
        is_running_condvar.1.notify_one();

        // Now we wait for playback to end.
        let event = events.recv().expect("QueueEvent sender should run indefinitely.");
        let playback_ended_at = match event {
            QueueEvent::StartPlayback(..) => panic!("Received StartPlayback before EndPlayback."),
            QueueEvent::EndPlayback(at) => at,
        };

        // After playback ends, we need to wait for the idle timeout to expire.
        // However, if playback resumes, then we should stop waiting immediately
        // and execute the pre-playback command again. We can do both in one go
        // by waiting for the next event with a deadline.
        let timeout = Duration::from_secs(config.idle_timeout_seconds);
        let deadline = playback_ended_at + timeout;
        if let Ok(next_event) = events.recv_timeout(deadline.duration_since(Instant::now())) {
            start_event = next_event;
            continue
        }

        // If we get here, then we waited for the full timeout, and playback did
        // not resume, which means we are idle now.
        if let Some(exe) = config.exec_post_idle_path.as_ref() {
            execute_program_with_timeout(exe, "post-idle");
        }

        // Wait for playback to start again.
        start_event = events.recv().expect("QueueEvent sender should run indefinitely.");
    }
}
