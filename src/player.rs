// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Ensures that the right samples are queued for playback.

use std::fs;
use std::mem;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::thread;

use claxon;
use claxon::metadata::StreamInfo;

use ::playback;
use ::{MetaIndex, TrackId};

type FlacReader = claxon::FlacReader<fs::File>;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Format {
    pub sample_rate_hz: u32,
    pub bits_per_sample: u32,
}

/// A block of interleaved samples, queued for playback.
pub struct Block {
    /// The samples, interleaved left, right.
    ///
    /// Samples are encoded in little endian (which is native both for x86,
    /// and ARM on the Raspberry Pi) in the number of bits per sample specified
    /// by the format.
    sample_bytes: Box<[u8]>,

    /// The number of bytes consumed.
    pos: usize,

    /// The bit depth and sample rate of this block.
    format: Format,
}

impl Block {
    pub fn new(format: Format, sample_bytes: Vec<u8>) -> Block {
        Block {
            sample_bytes: sample_bytes.into_boxed_slice(),
            pos: 0,
            format: format,
        }
    }

    /// Return a slice of the unconsumed samples.
    pub fn slice(&self) -> &[u8] {
        &self.sample_bytes[self.pos..]
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// Consume n samples.
    fn consume(&mut self, n: usize) {
        self.pos += n * (self.format.bits_per_sample / 8) as usize;
        debug_assert!(self.pos <= self.sample_bytes.len());
    }

    /// Return the number of unconsumed samples left.
    pub fn len(&self) -> usize {
        (self.sample_bytes.len() - self.pos) / (self.format.bits_per_sample / 8) as usize
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> usize {
        // Multiply by 1000 to go from seconds to milliseconds, divide by 2
        // because there are 2 channels.
        self.len() * 500 / self.format.sample_rate_hz as usize
    }

    /// Return the size of the block (including consumed samples) in bytes.
    pub fn size_bytes(&self) -> usize {
        self.sample_bytes.len()
    }
}

/// The decoding state of a queued track.
pub enum Decode {
    /// No decode started yet.
    NotStarted,
    /// Track partially decoded, can be resumed.
    Partial(FlacReader),
    /// Decode in progress, the decoder thread has the reader for now.
    Running,
    /// Decoding is complete.
    Done,
}

pub struct QueuedTrack {
    track: TrackId,
    decode: Decode,
    blocks: Vec<Block>,
}

impl QueuedTrack {
    pub fn new(track: TrackId) -> QueuedTrack {
        QueuedTrack {
            track: track,
            decode: Decode::NotStarted,
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

/// A task to be executed by the decoder thread.
pub enum DecodeTask {
    /// Continue decoding with the given reader.
    Continue(FlacReader),

    /// Start decoding a new track.
    Start(TrackId),
}

/// The result of a decode task.
///
/// If the file has been fully decoded, the reader is `None`, if there is more
/// to decode, it is returned here.
pub struct DecodeResult {
    block: Block,
    reader: Option<FlacReader>,
}

impl DecodeTask {
    /// Decode until the end of the file, or until we produced more than `stop_after_bytes`.
    pub fn run<I: MetaIndex>(self, index: &I, stop_after_bytes: usize) -> DecodeResult {
        match self {
            DecodeTask::Continue(reader) => DecodeTask::decode(reader, stop_after_bytes),
            DecodeTask::Start(track_id) => DecodeTask::start(index, track_id, stop_after_bytes),
        }
    }

    fn start<I: MetaIndex>(index: &I, track_id: TrackId, stop_after_bytes: usize) -> DecodeResult {
        let track = match index.get_track(track_id) {
            Some(t) => t,
            None => panic!("Track {} does not exist, how did it end up queued?"),
        };
        let fname = index.get_filename(track.filename);
        // TODO: Add a proper way to do logging.
        println!("Opening {:?} for decode.", fname);
        let reader = match FlacReader::open(fname) {
            Ok(r) => r,
            // TODO: Don't crash the full daemon on decode errors.
            Err(err) => panic!("Failed to open {:?} for reading: {:?}", fname, err),
        };
        DecodeTask::decode(reader, stop_after_bytes)
    }

    fn decode(reader: FlacReader, stop_after_bytes: usize) -> DecodeResult {
        let streaminfo = reader.streaminfo();
        match streaminfo.bits_per_sample {
            16 => DecodeTask::decode_i16(reader, streaminfo, stop_after_bytes),
            24 => DecodeTask::decode_i24(reader, streaminfo, stop_after_bytes),
            n  => panic!("Unsupported bit depth: {}", n),
        }
    }

    fn decode_i16(
        mut reader: FlacReader,
        streaminfo: StreamInfo,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 16);
        assert_eq!(streaminfo.channels, 2);

        // The block size counts inter-channel samples, and we assume that all
        // files are stereo, so multiply by two.
        let max_samples_per_frame = streaminfo.max_block_size as usize * 2;
        let max_bytes_per_frame = max_samples_per_frame * 2;
        let mut is_done = false;
        let mut out = Vec::with_capacity(stop_after_bytes + max_bytes_per_frame);

        {
            let mut frame_reader = reader.blocks();
            let mut buffer = Vec::with_capacity(max_samples_per_frame);

            // Decode as long as we expect to stay under the byte limit, but do
            // decode at least one frame, otherwise we would not make progress.
            while out.is_empty() || out.len() < stop_after_bytes  {
                let frame = match frame_reader.read_next_or_eof(buffer) {
                    Ok(None) => {
                        is_done = true;
                        break
                    }
                    Ok(Some(b)) => b,
                    Err(err) => panic!("TODO: Handle decode error: {:?}", err),
                };

                for (l, r) in frame.stereo_samples() {
                    // Encode the samples in little endian.
                    let bytes: [u8; 4] = [
                        ((l >> 0) & 0xff) as u8,
                        ((l >> 8) & 0xff) as u8,
                        ((r >> 0) & 0xff) as u8,
                        ((r >> 8) & 0xff) as u8,
                    ];
                    out.extend_from_slice(&bytes[..]);
                }

                buffer = frame.into_buffer();
            }
        }

        let format = Format {
            sample_rate_hz: streaminfo.sample_rate,
            bits_per_sample: 16,
        };
        let block = Block::new(format, out);
        DecodeResult {
            block: block,
            reader: if is_done { None } else { Some(reader) }
        }
    }

    fn decode_i24(
        mut reader: FlacReader,
        streaminfo: StreamInfo,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 24);
        assert_eq!(streaminfo.channels, 2);

        // The block size counts inter-channel samples, and we assume that all
        // files are stereo, so multiply by two.
        let max_samples_per_frame = streaminfo.max_block_size as usize * 2;
        let max_bytes_per_frame = max_samples_per_frame * 3;
        let mut is_done = false;
        let mut out = Vec::with_capacity(stop_after_bytes + max_bytes_per_frame);

        {
            let mut frame_reader = reader.blocks();
            let mut buffer = Vec::with_capacity(max_samples_per_frame);

            // Decode as long as we expect to stay under the byte limit, but do
            // decode at least one frame, otherwise we would not make progress.
            while out.is_empty() || out.len() < stop_after_bytes  {
                let frame = match frame_reader.read_next_or_eof(buffer) {
                    Ok(None) => {
                        is_done = true;
                        break
                    }
                    Ok(Some(b)) => b,
                    Err(err) => panic!("TODO: Handle decode error: {:?}", err),
                };

                for (l, r) in frame.stereo_samples() {
                    // Encode the samples in little endian.
                    let bytes: [u8; 6] = [
                        ((l >>  0) & 0xff) as u8,
                        ((l >>  8) & 0xff) as u8,
                        ((l >> 16) & 0xff) as u8,
                        ((r >>  0) & 0xff) as u8,
                        ((r >>  8) & 0xff) as u8,
                        ((r >> 16) & 0xff) as u8,
                    ];
                    out.extend_from_slice(&bytes[..]);
                }

                buffer = frame.into_buffer();
            }
        }

        let format = Format {
            sample_rate_hz: streaminfo.sample_rate,
            bits_per_sample: 24,
        };
        let block = Block::new(format, out);
        DecodeResult {
            block: block,
            reader: if is_done { None } else { Some(reader) }
        }
    }
}

pub struct PlayerState {
    /// The tracks pending playback. Element 0 is being played currently.
    ///
    /// Invariant: If the queued track at index i has no decoded blocks, then
    /// for every index j > i, the queued track at index j has no decoded
    /// blocks either. In other words, all decoded blocks are at the beginning
    /// of the queue.
    queue: Vec<QueuedTrack>,

    /// The index of the track for which a decode is in progress.
    ///
    /// The decoder itself will be moved into the decoder thread temporarily.
    /// When the decode is done, the decoder thread needs to add the new blocks,
    /// and put the `FlacReader` back, but the queue could have changed in the
    /// meantime, so we need to track the index of where to restore later.
    current_decode: Option<usize>,
}


impl PlayerState {
    pub fn new() -> PlayerState {
        PlayerState {
            queue: vec![
                QueuedTrack::new(TrackId(0x29b4bebda0c8710d)),
                QueuedTrack::new(TrackId(0xb9b7641fbd52f102)),
                QueuedTrack::new(TrackId(0x829506fd64ad710b)),
                QueuedTrack::new(TrackId(0xba542e474fb39101)),
                QueuedTrack::new(TrackId(0x29b4bebda0c87107)),
                QueuedTrack::new(TrackId(0x639f0d068574320b)),
                QueuedTrack::new(TrackId(0x1c154369c48bf100)),
                QueuedTrack::new(TrackId(0xb9b7641fbd52f106)),
                QueuedTrack::new(TrackId(0xb1a431f57167a104)),
                QueuedTrack::new(TrackId(0x9b21f06be23fb108)),
                QueuedTrack::new(TrackId(0x737135ec9131c101)),
                QueuedTrack::new(TrackId(0x752f4652a82cc101)),
                QueuedTrack::new(TrackId(0x32385e9e354a1102)),
                QueuedTrack::new(TrackId(0x29b4bebda0c87101)),
                QueuedTrack::new(TrackId(0x8ead13ff2b95f102)),
                QueuedTrack::new(TrackId(0x11c86a504f455101)),
            ],
            current_decode: None,
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

        let n_running = self
            .queue
            .iter()
            .filter(|qt| match qt.decode {
                Decode::Running => true,
                _               => false,
            })
            .count();
        assert!(n_running <= 1, "At most one decode should be in progress.");
    }

    /// Return the next block to play from, if any.
    pub fn peek_mut(&mut self) -> Option<&mut Block> {
        match self.queue.first_mut() {
            Some(qt) => qt.blocks.first_mut(),
            None => None,
        }
    }

    /// Return whether the queue is empty.
    pub fn is_queue_empty(&self) -> bool {
        self.queue.is_empty()
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
            match &queued_track.decode {
                Decode::Done => queued_track.blocks.is_empty(),
                _ => false,
            }
        };
        if track_done {
            let track = self.queue.remove(0);
            // If a decode is in progress, the index of the track it is decoding
            // changed because of the `remove` above.
            if let Some(i) = self.current_decode {
                self.current_decode = Some(i - 1);
            }

            println!("Track {} fully consumed. Queue size: {}", track.track, self.queue.len());
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

    /// Return whether there are queue items that have not yet been fully decoded.
    pub fn can_decode(&self) -> bool {
        for queued_track in self.queue.iter() {
            match &queued_track.decode {
                Decode::Done => continue,
                _ => return true,
            }
        }
        return false
    }

    /// Return whether we should start decoding more.
    ///
    /// In general, we prefer to decode a lot in a big batch, and then sleep for
    /// a long time, over decoding little bits all the time. This saves power
    /// because it allows the CPU to be downclocked when we are not decoding,
    /// and if the buffer is long enough, it may even be possible to spin down
    /// disks.
    ///
    /// However, when the disks are not spinning, if we need to access those
    /// disks to resume decoding, it can take 10 to 15 seconds for them to spin
    /// up again, therefore we should start decoding early enough, such that the
    /// IO is complete before we run out of samples to play.
    pub fn needs_decode(&self) -> bool {
        // Choose a safe margin; if spinning up the disks takes 10 to 15
        // seconds, starting 30 seconds in advance should be sufficient.
        let min_buffer_ms = 30_000;

        let is_buffer_low = self.pending_duration_ms() < min_buffer_ms;
        is_buffer_low && self.can_decode()
    }

    /// Return a decode task, if there is something to decode.
    pub fn take_decode_task(&mut self) -> Option<DecodeTask> {
        assert!(
            self.current_decode.is_none(),
            "Can only take decode task when none is already in progress.",
        );

        for (i, queued_track) in self.queue.iter_mut().enumerate() {
            match &queued_track.decode {
                &Decode::Done => continue,
                _ => {}
            }

            // If the decode is not done, then we will now start or continue,
            // so put on "running", and then inspect the previous state.
            let mut decode = Decode::Running;
            mem::swap(&mut decode, &mut queued_track.decode);

            match decode {
                Decode::NotStarted => {
                    self.current_decode = Some(i);
                    return Some(DecodeTask::Start(queued_track.track));
                }
                Decode::Partial(reader) => {
                    self.current_decode = Some(i);
                    return Some(DecodeTask::Continue(reader));
                }
                Decode::Running => {
                    panic!("No decode can be running when current_decode is None.");
                }
                Decode::Done => {
                    panic!("Unreachable, we skipped the iteration on Done.");
                }
            }
        }

        None
    }

    /// Store the result after completing a decode task.
    ///
    /// If the file has not been fully decoded yet, the reader needs to be
    /// returned as well.
    pub fn return_decode_task(&mut self, result: DecodeResult) {
        let queued_track = match self.current_decode {
            Some(i) => &mut self.queue[i],
            None => panic!("Can only return from a decode task if one is in progress."),
        };
        match queued_track.decode {
            Decode::Running => {},
            _ => panic!("If we decoded for this track, it must have been marked running."),
        }
        queued_track.blocks.push(result.block);
        queued_track.decode = match result.reader {
            Some(r) => Decode::Partial(r),
            None => Decode::Done,
        };
        self.current_decode = None;
    }
}

/// Decode the queue until we reach a set memory limit.
fn decode_burst<I: MetaIndex>(index: &I, state_mutex: &Mutex<PlayerState>) {
    // The decode thread is a trade-off between power consumption and memory
    // usage: decoding a lot in one go and then sleeping for a long time is more
    // efficient than decoding a bit all the time, because the CPU can be
    // downclocked in between the bursts. Also, if the time between disk
    // accesses is long enough, it might even be possible to spin down the disks
    // until the next batch of decodes, which keeps the system quiet too.
    // However, we do need to be able to hold all decoded samples in memory
    // then, and there is some risk of the decode being wasted work when the
    // queue changes. 85 MB will hold about 8 minutes of 16-bit 44.1 kHz audio.
    // TODO: Make this configurable.
    let stop_after_bytes = 85_000_000;
    let mut previous_result = None;

    loop {
        // Get the latest memory usage, and take the next task to execute. This
        // only holds the mutex briefly, so we can do the decode without holding
        // the mutex.
        let (task, bytes_used, pending_duration_ms) = {
            let mut state = state_mutex.lock().unwrap();

            if let Some(result) = previous_result.take() {
                state.return_decode_task(result);
            }

            let bytes_used = state.pending_size_bytes();
            if bytes_used >= stop_after_bytes {
                println!("Buffer full, stopping decode for now.");
                return
            }

            let task = match state.take_decode_task() {
                None => return,
                Some(t) => t,
            };

            (task, bytes_used, state.pending_duration_ms())
        };

        // If the buffer is running low, then our priority shouldn't be to
        // decode efficiently in bursts, it should be to put something in the
        // buffer as soon as possible. In that case we set the number of bytes
        // to decode very low, so we can make the result available quickly.
        // As a rough estimate, say we can decode 16 bit 44.1 kHz stereo audio
        // at 5× realtime speed, then the duration of the buffered audio
        // determines our budget for decoding. If we set the budget at 0, the
        // decoder will still decode at least one frame.
        let decode_bytes_per_ms = 44_100 * 4 * 5 / 1000;
        let decode_bytes_budget = decode_bytes_per_ms * pending_duration_ms;
        let bytes_left = decode_bytes_budget.min(stop_after_bytes - bytes_used);
        println!("Pending buffer stats:");
        println!("  Duration: {:.3} seconds", pending_duration_ms as f32 / 1000.0);
        println!("  Memory:   {:.3} / {:.3} MB", bytes_used as f32 * 1e-6, stop_after_bytes as f32 * 1e-6);
        println!("  Budget:   {:.3} MB", bytes_left as f32 * 1e-6);
        let result = task.run(index, bytes_left);
        println!("Decoded {:.3} MB.", result.block.len() as f32 * 1e-6);
        previous_result = Some(result);
    }
}

/// The main loop for the decode thread.
///
/// Decodes until the in-memory buffer is full, then parks itself. When
/// unparked, if the buffer is running low, it starts a new burst of decode and
/// then parks itself again, etc.
fn decode_main<I: MetaIndex>(index: &I, state_mutex: &Mutex<PlayerState>) {
    loop {
        let should_decode = {
            let state = state_mutex.lock().unwrap();
            state.needs_decode()
        };

        if should_decode {
            decode_burst(index, state_mutex);
        }

        println!("Decoder going to sleep.");
        thread::park();
        println!("Decoder woken up.");
    }
}

pub struct Player<I: MetaIndex + Sync + Send + 'static> {
    state: Arc<Mutex<PlayerState>>,
    index: Arc<I>,
    decode_thread: JoinHandle<()>,
    playback_thread: JoinHandle<()>,
}

impl<I: MetaIndex + Sync + Send + 'static> Player<I> {
    pub fn new(index: Arc<I>, card_name: String) -> Player<I> {
        let state = Arc::new(Mutex::new(PlayerState::new()));

        // Start the decode thread. It runs indefinitely, but we do need to
        // periodically unpark it when there is new stuff to decode.
        let state_mutex_for_decode = state.clone();
        let index_for_decode = index.clone();
        let builder = std::thread::Builder::new();
        let decode_join_handle = builder
            .name("decoder".into())
            .spawn(move || {
                decode_main(&*index_for_decode, &*state_mutex_for_decode);
            }).unwrap();

        let state_mutex_for_playback = state.clone();
        let decode_thread_for_playback = decode_join_handle.thread().clone();

        let builder = std::thread::Builder::new();
        let playback_join_handle = builder
            .name("playback".into())
            .spawn(move || {
                playback::main(
                    &card_name[..],
                    &*state_mutex_for_playback,
                    &decode_thread_for_playback,
                );
            }).unwrap();

        Player {
            state: state,
            index: index,
            decode_thread: decode_join_handle,
            playback_thread: playback_join_handle,
        }
    }

    /// Wait for the playback and decode thread to finish.
    pub fn join(self) {
        // Note: currently there is no way to to signal these threads to stop,
        // so this will block indefinitely.
        self.playback_thread.join().unwrap();
        self.decode_thread.join().unwrap();
    }
}
