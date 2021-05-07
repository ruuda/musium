// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Progress reporting functionality used during scanning.

use std::io;

use crate::scan::Issue;
use crate::systemd;

/// A trait for status reporting during indexing.
///
/// We can report status to stdout, when running in a terminal, or to systemd's
/// status mechanism, when running as a systemd service. Possibly in the future
/// we could support hot reload in the webinterface, and write status to some
/// place where the webserver can serve it.
pub trait StatusSink {
    fn report_discover_progress(&mut self, num_found: u32) -> io::Result<()>;
    fn report_index_progress(&mut self, num_indexed: u32, num_total: u32) -> io::Result<()>;
    fn report_issue(&mut self, issue: &Issue) -> io::Result<()>;
    fn report_done_indexing(&mut self) -> io::Result<()>;
}

/// Log updates to the writer, using carriage returns to keep vertical space brief.
pub struct WriteStatusSink<W: io::Write> {
    out: W,
    at_line_start: bool
}

impl<W: io::Write> WriteStatusSink<W> {
    pub fn new(out: W) -> WriteStatusSink<W> {
        WriteStatusSink {
            out: out,
            at_line_start: true,
        }
    }
}

impl<W: io::Write> StatusSink for WriteStatusSink<W> {
    fn report_discover_progress(&mut self, num_found: u32) -> io::Result<()> {
        if !self.at_line_start {
            write!(self.out, "\r")?;
        }
        write!(self.out, "{} files discovered", num_found)?;
        self.out.flush()?;
        self.at_line_start = false;
        Ok(())
    }

    fn report_index_progress(&mut self, num_indexed: u32, num_total: u32) -> io::Result<()> {
        if !self.at_line_start {
            write!(self.out, "\r")?;
        }
        write!(self.out, "{} / {} files indexed", num_indexed, num_total)?;
        self.out.flush()?;
        self.at_line_start = false;
        Ok(())
    }

    fn report_issue(&mut self, issue: &Issue) -> io::Result<()> {
        if !self.at_line_start {
            write!(self.out, "\n")?;
        }
        writeln!(self.out, "{}\n", issue)?;
        self.at_line_start = true;
        Ok(())
    }

    fn report_done_indexing(&mut self) -> io::Result<()> {
        if !self.at_line_start {
            write!(self.out, "\n")?;
        }
        Ok(())
    }
}

/// Log progress as system status updates, but issues to the writer.
pub struct SystemdStatusSink<W: io::Write> {
    out: W,
}

impl<W: io::Write> SystemdStatusSink<W> {
    pub fn new(out: W) -> SystemdStatusSink<W> {
        SystemdStatusSink {
            out: out,
        }
    }
}

impl<W: io::Write> StatusSink for SystemdStatusSink<W> {
    fn report_discover_progress(&mut self, num_found: u32) -> io::Result<()> {
        // Log the status, but also extend the startup timeout by 10 seconds
        // (until we log again). Indexing a large library from a spinning disk
        // can take a few minutes, and without this, systemd would fail the unit
        // after the default timeout.
        let status = format!(
            "STATUS={} files discovered\nEXTEND_TIMEOUT_USEC=10000000\n",
            num_found,
        );
        systemd::notify(status)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "sd_notify error"))
    }

    fn report_index_progress(&mut self, num_indexed: u32, num_total: u32) -> io::Result<()> {
        // See also the note above about extending the timeout.
        let status = format!(
            "STATUS={} / {} files indexed\nEXTEND_TIMEOUT_USEC=10000000\n",
            num_indexed,
            num_total,
        );
        systemd::notify(status)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "sd_notify error"))
    }

    fn report_issue(&mut self, issue: &Issue) -> io::Result<()> {
        writeln!(self.out, "{}\n", issue)
    }

    fn report_done_indexing(&mut self) -> io::Result<()> {
        // Note, we don't signal READY=1 yet, the webserver does that when it is
        // online. Extend the timeout by a minute, to give us some time to load
        // the thumbnails from disk, which happens after indexing.
        systemd::notify(
            "STATUS=Indexing complete\nEXTEND_TIMEOUT_USEC=60000000\n".into()
        )
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "sd_notify error"))
    }
}
