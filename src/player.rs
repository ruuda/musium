// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Ensures that the right samples are queued for playback.

use std::fs;
use std::mem;

use claxon;

use ::TrackId;

type FlacReader = claxon::FlacReader<fs::File>;

pub struct Format {
    pub sample_rate_hz: u32,
    pub bits_per_sample: u32,
}

/// A block of interleaved samples, queued for playback.
pub struct Block {
    /// The samples, interleaved left, right.
    samples: Box<[i32]>,

    /// The number of samples consumed.
    pos: usize,

    /// The bit depth and sample rate of this block.
    format: Format,
}

impl Block {
    pub fn new(format: Format, samples: Vec<i32>) -> Block {
        Block {
            samples: samples.into_boxed_slice(),
            pos: 0,
            format: format,
        }
    }

    /// Return a slice of the unconsumed samples.
    pub fn slice(&self) -> &[i32] {
        &self.samples[self.pos..]
    }

    /// Consume n samples.
    fn consume(&mut self, n: usize) {
        self.pos += n;
        debug_assert!(self.pos <= self.samples.len());
    }

    /// Return the number of unconsumed samples left.
    pub fn len(&self) -> usize {
        self.samples.len() - self.pos
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> usize {
        // Multiply by 1000 to go from seconds to milliseconds, divide by 2
        // because there are 2 channels.
        self.len() * 500 / self.format.sample_rate_hz as usize
    }

    /// Return the size of the block (including consumed samples) in bytes.
    pub fn size_bytes(&self) -> usize {
        self.samples.len() * mem::size_of::<i32>()
    }
}

pub struct QueuedTrack {
    track: TrackId,
    reader: Option<FlacReader>,
    blocks: Vec<Block>,
}

impl QueuedTrack {
    pub fn new(track: TrackId) -> QueuedTrack {
        QueuedTrack {
            track: track,
            reader: None,
            blocks: Vec::new(),
        }
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> usize {
        self.blocks.iter().map(|b| b.duration_ms()).sum()
    }

    /// Return the size of the blocks (including consumed samples) in bytes.
    pub fn size_bytes(&self) -> usize {
        self.blocks.iter().map(|b| b.size_bytes()).sum()
    }
}

pub struct Player {
    /// The tracks pending playback. Element 0 is being played currently.
    ///
    /// Invariant: If the queued track at index i has no decoded blocks, then
    /// for every index j > i, the queued track at index j has no decoded
    /// blocks either. In other words, all decoded blocks are at the beginning
    /// of the queue.
    queue: Vec<QueuedTrack>,
}

impl Player {
    pub fn new() -> Player {
        Player {
            queue: Vec::new(),
        }
    }

    /// Assert that invariants hold, for use in testing, or debugging.
    fn assert_invariants(&self) {
        let mut saw_empty = false;
        for (i, qt) in self.queue.iter().enumerate() {
            if saw_empty {
                assert_eq!(
                    qt.blocks.len(), 0,
                    "Expected no decoded blocks at queue index {}.", i
                );
            }
            saw_empty = qt.blocks.len() == 0;
        }
    }

    /// Return the next block to play from, if any.
    pub fn peek_mut(&mut self) -> Option<&mut Block> {
        match self.queue.first_mut() {
            Some(qt) => qt.blocks.first_mut(),
            None => None,
        }
    }

    /// Consume n samples from the peeked block.
    pub fn consume(&mut self, n: usize) {
        let track_done = {
            let mut queued_track = &mut self.queue[0];
            let block_done = {
                let mut block = &mut queued_track.blocks[0];
                block.consume(n);
                block.len() == 0
            };
            if block_done {
                queued_track.blocks.remove(0);
            }
            queued_track.blocks.len() == 0
        };
        if track_done {
            self.queue.remove(0);
        }

        #[cfg(debug)]
        self.assert_invariants();
    }

    /// Return the duration of all unconsumed samples in milliseconds.
    pub fn pending_duration_ms(&self) -> usize {
        self.queue.iter().map(|qt| qt.duration_ms()).sum()
    }

    /// Return the size of all blocks in bytes.
    pub fn pending_size_bytes(&self) -> usize {
        self.queue.iter().map(|qt| qt.size_bytes()).sum()
    }
}
