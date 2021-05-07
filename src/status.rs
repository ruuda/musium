// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Progress reporting functionality used during scanning.

use std::io;

use crate::scan::Issue;
use crate::systemd;

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
        write!(self.out, "[{}] Discovering ...", num_found)?;
        self.out.flush()?;
        self.at_line_start = false;
        Ok(())
    }

    fn report_index_progress(&mut self, num_indexed: u32, num_total: u32) -> io::Result<()> {
        if !self.at_line_start {
            write!(self.out, "\r")?;
        }
        write!(self.out, "[{} / {}] Indexing ...", num_indexed, num_total)?;
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

/// Log progress as system status updates, but issues to stdout (the journal).
struct SystemdStatusSink<W: io::Write> {
    out: W,
}
