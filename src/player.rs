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

#[derive(Copy, Clone, Eq, PartialEq)]
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
    /// Decode until the end of the file, or until we produced `max_bytes`.
    pub fn run<I: MetaIndex>(self, index: &I, max_bytes: usize) -> DecodeResult {
        match self {
            DecodeTask::Continue(reader) => DecodeTask::decode(reader, max_bytes),
            DecodeTask::Start(track_id) => DecodeTask::start(index, track_id, max_bytes),
        }
    }

    fn start<I: MetaIndex>(index: &I, track_id: TrackId, max_bytes: usize) -> DecodeResult {
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
        DecodeTask::decode(reader, max_bytes)
    }

    fn decode(reader: FlacReader, max_bytes: usize) -> DecodeResult {
        let streaminfo = reader.streaminfo();
        match streaminfo.bits_per_sample {
            16 => DecodeTask::decode_i16(reader, streaminfo, max_bytes),
            14 => DecodeTask::decode_i24(reader, streaminfo, max_bytes),
            n  => panic!("Unsupported bit depth: {}", n),
        }
    }

    fn decode_i16(
        mut reader: FlacReader,
        streaminfo: StreamInfo,
        max_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 16);
        assert_eq!(streaminfo.channels, 2);
        let mut out = Vec::with_capacity(max_bytes);

        // The block size counts inter-channel samples, and we assume that all
        // files are stereo, so multiply by two.
        let max_samples_per_frame = streaminfo.max_block_size as usize * 2;
        let max_bytes_per_frame = max_samples_per_frame * 2;
        let mut is_done = false;

        {
            let mut frame_reader = reader.blocks();
            let mut buffer = Vec::with_capacity(max_samples_per_frame);

            // Decode as long as we expect to stay under the byte limit, but do
            // decode at least one frame, otherwise we would not make progress.
            while out.is_empty() || out.len() + max_bytes_per_frame < max_bytes  {
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
        max_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 24);
        assert_eq!(streaminfo.channels, 2);
        let mut out = Vec::with_capacity(max_bytes);

        // The block size counts inter-channel samples, and we assume that all
        // files are stereo, so multiply by two.
        let max_samples_per_frame = streaminfo.max_block_size as usize * 2;
        let max_bytes_per_frame = max_samples_per_frame * 3;
        let mut is_done = false;

        {
            let mut frame_reader = reader.blocks();
            let mut buffer = Vec::with_capacity(max_samples_per_frame);

            // Decode as long as we expect to stay under the byte limit, but do
            // decode at least one frame, otherwise we would not make progress.
            while out.is_empty() || out.len() + max_bytes_per_frame < max_bytes  {
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
                QueuedTrack::new(TrackId(0x1c154369c48bf100)),
                QueuedTrack::new(TrackId(0x29b4bebda0c87101)),
                QueuedTrack::new(TrackId(0x29b4bebda0c87107)),
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
            self.queue.remove(0);
            // If a decode is in progress, the index of the track it is decoding
            // changed because of the `remove` above.
            if let Some(i) = self.current_decode {
                self.current_decode = Some(i - 1);
            }
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
    let max_bytes = 85_000_000;
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

            let task = match state.take_decode_task() {
                None => return,
                Some(t) => t,
            };

            (task, state.pending_size_bytes(), state.pending_duration_ms())
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
        let bytes_left = decode_bytes_budget.min(max_bytes - bytes_used);
        println!("Decoding with budget of {} bytes ...", bytes_left);
        let result = task.run(index, bytes_left);
        println!("Decoded {} bytes.", result.block.len());
        previous_result = Some(result);
    }
}

/// The main loop for the decode thread.
///
/// Decodes until the in-memory buffer is full, then parks itself. When
/// unparked, if the buffer is running low, it starts a new burst of decode and
/// then parks itself again, etc.
fn decode_main<I: MetaIndex>(index: &I, state_mutex: &Mutex<PlayerState>) {
    // The minimum duration of decoded samples. If the buffered content is more
    // than this, there is no need to decode yet; it is better to sleep and do
    // a burst of decode later, than to decode a little bit all the time. The 30
    // seconds are chosen as a safe margin to spin up any disks from which we
    // may need to read files. If the disks are in power saving mode, a read can
    // take 10 to 15 seconds, so we need to start the read early enough for
    // continuous playback.
    // TODO: Make this configurable.
    let min_buffer_ms = 30_000;

    loop {
        let should_decode = {
            let state = state_mutex.lock().unwrap();
            state.pending_duration_ms() < min_buffer_ms
        };

        if should_decode {
            decode_burst(index, state_mutex);
        }

        thread::park();
    }
}

pub struct Player<I: MetaIndex + Sync + Send + 'static> {
    state: Arc<Mutex<PlayerState>>,
    index: Arc<I>,
    decode_thread: JoinHandle<()>,
    playback_thread: Option<JoinHandle<()>>,
    card_name: String,
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

        Player {
            state: state,
            index: index,
            decode_thread: decode_join_handle,
            playback_thread: None,
            card_name: card_name,
        }
    }

    fn start_playback(&mut self) {
        assert!(self.playback_thread.is_none(), "Playback already in progress.");

        let state_mutex_for_playback: Arc<Mutex<PlayerState>> = self.state.clone();
        let decode_thread_for_playback: std::thread::Thread = self.decode_thread.thread().clone();
        let card_name_for_playback = self.card_name.clone();

        let builder = std::thread::Builder::new();
        let playback_join_handle = builder
            .name("playback".into())
            .spawn(move || {
                // TODO: Set thread priority to high.
                println!("Playback thread starting ...");
                playback::main(
                    &card_name_for_playback[..],
                    &*state_mutex_for_playback,
                    &decode_thread_for_playback,
                );
                println!("Playback done.");
            }).unwrap();

        self.playback_thread = Some(playback_join_handle);
    }

    pub fn play(&mut self) {
        match self.playback_thread {
            None => self.start_playback(),
            Some(ref _handle) => {
                // TODO: Figure out a way to check if the thread is still
                // running, and if not, delete it and then start a new one.
            }
        }
    }

    pub fn wait(&mut self) {
        if let Some(handle) = self.playback_thread.take() {
            handle.join().unwrap();
        }
    }
}
