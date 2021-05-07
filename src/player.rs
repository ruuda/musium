// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Ensures that the right samples are queued for playback.

use std::fmt;
use std::fs;
use std::mem;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::thread;

use claxon;
use claxon::metadata::StreamInfo;

use crate::history::PlaybackEvent;
use crate::history;
use crate::playback;
use crate::{AlbumId, Lufs, MetaIndex, TrackId};

type FlacReader = claxon::FlacReader<fs::File>;

/// A unique identifier for a queued track.
///
/// This identifier is used to track the queued track through its lifetimes
/// (queued, playing, history). Having a unique id per queued track allows e.g.
/// distinguishing the same track queued twice in succession.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueueId(pub u64);

impl fmt::Display for QueueId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// A dimensionless number expressed on a logarithmic scale.
///
/// The representation is millibel, or in other words, this is a decibel as
/// a decimal fixed-point number with two decimal digits after the point.
///
/// Example: -7.32 dB would be stored as `Millibel(-732)`.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Millibel(pub i16);

impl fmt::Display for Millibel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2} dB", (self.0 as f32) * 0.01)
    }
}

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
    pub fn duration_ms(&self) -> u64 {
        // Multiply by 1000 to go from seconds to milliseconds, divide by 2
        // because there are 2 channels. We need to work with u64 here, because
        // around 100s of stereo 44.1 kHz audio, the sample count times 500
        // overflows a u32 (and usize can be 32 bits). We can't move the 500
        // into the denominator, because the common sample rate of 44.1 kHz is
        // not a multiple of 500.
        self.len() as u64 * 500 / self.format.sample_rate_hz as u64
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
    /// A unique identifier for this particular queuement of the track.
    queue_id: QueueId,

    /// Track id of the track to be played.
    track_id: TrackId,

    /// Album id of the track to be played.
    album_id: AlbumId,

    /// Perceived track loudness in Loudness Units Full Scale.
    track_loudness: Lufs,

    /// Perceived album loudness in Loudness Units Full Scale.
    album_loudness: Lufs,

    /// Decoded blocks of audio data.
    blocks: Vec<Block>,

    /// Number of samples already sent to the audio card.
    ///
    /// Divide by the sample rate and number of channels to get the playback
    /// position in seconds.
    samples_played: u64,

    /// The sample rate of the track in Hz.
    ///
    /// Only known after decoding has started; until then it is None.
    sample_rate_hz: Option<u32>,

    /// Decoder for this track.
    decode: Decode,
}

impl QueuedTrack {
    pub fn new(
        queue_id: QueueId,
        track_id: TrackId,
        album_id: AlbumId,
        track_loudness: Lufs,
        album_loudness: Lufs,
    ) -> QueuedTrack {
        QueuedTrack {
            queue_id: queue_id,
            track_id: track_id,
            album_id: album_id,
            track_loudness: track_loudness,
            album_loudness: album_loudness,
            blocks: Vec::new(),
            samples_played: 0,
            sample_rate_hz: None,
            decode: Decode::NotStarted,
        }
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.blocks.iter().map(|b| b.duration_ms()).sum()
    }

    /// Return the duration of the consumed samples in milliseconds.
    pub fn position_ms(&self) -> u64 {
        match self.sample_rate_hz {
            // Multiply by 1000 to go from seconds to milliseconds, divide by 2
            // because there are 2 channels. We need to work with u64 here, because
            // around 100s of stereo 44.1 kHz audio, the sample count times 500
            // overflows a u32 (and usize can be 32 bits). We can't move the 500
            // into the denominator, because the common sample rate of 44.1 kHz
            // is not a multiple of 500.
            Some(hz) => self.samples_played * 500 / (hz as u64),
            // When the sample rate is not known, we definitely have not started
            // playback.
            None => 0
        }
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
    pub fn run(self, index: &dyn MetaIndex, stop_after_bytes: usize) -> DecodeResult {
        match self {
            DecodeTask::Continue(reader) => DecodeTask::decode(reader, stop_after_bytes),
            DecodeTask::Start(track_id) => DecodeTask::start(index, track_id, stop_after_bytes),
        }
    }

    fn start(index: &dyn MetaIndex, track_id: TrackId, stop_after_bytes: usize) -> DecodeResult {
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

        out.shrink_to_fit();

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
    /// Counter that assigns queue ids.
    next_unused_id: QueueId,

    /// The target volume, controlled by the user.
    ///
    /// A volume of 0 indicates that the material plays at the target loudness,
    /// negative values make it softer, positive values make it louder. The full
    /// volume range available depends on the perceived loudness of the current
    /// track or album.
    volume: Millibel,

    /// The loudness of the softest material we want to play back.
    ///
    /// The goal of loudness normalization is to make everything sound as loud
    /// as the material with minimal loudness, by turning down the volume for
    /// everything that is louder than that.
    ///
    /// To know how much to turn down the volume, we need to know the loudness
    /// of the softest material we want to play back. It is possible to set the
    /// target to the actual minimal loudness encountered in the library, but
    /// that means that once you add an even softer album or track, the meaning
    /// of the volume control will change, as the volume control is relative to
    /// this target. So instead, it is also possible to set this to a fixed but
    /// reasonably low loudness, such as -23.0 LUFS.
    ///
    /// The target loudness is static and should not change during the lifetime
    /// of the player.
    target_loudness: Lufs,

    /// Loudness of the currently playing track, either album or track loudness.
    ///
    /// When we start playing a track, we decide whether to use the album
    /// loudness or track loudness, and then we keep using that loudness for the
    /// entire duration of the track.
    current_track_loudness: Option<Lufs>,

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

    /// Sender for playback events.
    ///
    /// These events get consumed by the history thread, who logs them.
    events: SyncSender<PlaybackEvent>,
}


impl PlayerState {
    pub fn new(events: SyncSender<PlaybackEvent>) -> PlayerState {
        PlayerState {
            next_unused_id: QueueId(0),
            volume: Millibel(-1500),
            target_loudness: Lufs::new(-2300),
            current_track_loudness: None,
            queue: Vec::new(),
            current_decode: None,
            events: events,
        }
    }

    /// Assert that invariants hold, for use in testing, or debugging.
    #[allow(dead_code)] // Not dead, used in tests.
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

        let has_current_track_loudness = self.current_track_loudness.is_some();
        let has_track = self.queue.len() > 0;
        assert_eq!(has_current_track_loudness, has_track);
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

    /// Return the desired playback volume relative to full scale.
    ///
    /// This applies loudness normalization on top of the player target volume,
    /// to get the absolute playback volume.
    pub fn target_volume_full_scale(&self) -> Option<Millibel> {
        let track_loudness = self.current_track_loudness?;

        let loudness_adjustment_millibel = self.target_loudness.0.get() - track_loudness.0.get();
        let volume_mbfs = self.volume.0 + loudness_adjustment_millibel;

        Some(Millibel(volume_mbfs))
    }

    /// Update `current_track_loudness` based on the previous album and current queue.
    ///
    /// If there are tracks from the same album following or preceding in the
    /// queue, then we want to use the album loudness. If not, then we will use
    /// the track loudness.
    fn update_current_track_loudness(&mut self, previous_album: AlbumId) {
        let current_track = match self.queue.get(0) {
            Some(t) => t,
            None => {
                self.current_track_loudness = None;
                return;
            }
        };

        let loudness = match self.queue.get(1) {
            Some(next_track) if current_track.album_id == next_track.album_id => current_track.album_loudness,
            _ if current_track.album_id == previous_album => current_track.album_loudness,
            _ => current_track.track_loudness,
        };

        self.current_track_loudness = Some(loudness);
    }

    pub fn enqueue(&mut self, track: QueuedTrack) {
        // If this is the first track we add, opt for the album loudness instead
        // of the track loudness: the user might enqueue more tracks from the
        // same album shortly, and then we should play all of them at album
        // loudness. If the user enqueues a track from a different album next,
        // then too bad, we'll have used a less accurate loudness estimate for
        // the initial track, but the difference shouldn't be *that* big.
        if self.queue.is_empty() {
            self.current_track_loudness = Some(track.album_loudness);
        }

        self.queue.push(track);
    }

    /// Consume n samples from the peeked block.
    pub fn consume(&mut self, n: usize) {
        assert!(n > 0, "Must consume at least one sample.");

        let track_done = {
            let queued_track = &mut self.queue[0];

            // If this is the first time that we consume samples from this
            // track, then that means it was just started.
            if queued_track.samples_played == 0 {
                self.events.send(
                    PlaybackEvent::Started(queued_track.queue_id, queued_track.track_id)
                ).expect("Failed to send completion event to history thread.");
            }

            queued_track.samples_played += n as u64;

            let block_done = {
                let block = &mut queued_track.blocks[0];
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

            self.events.send(PlaybackEvent::Completed(track.queue_id, track.track_id))
                .expect("Failed to send completion event to history thread.");

            let previous_album = track.album_id;
            self.update_current_track_loudness(previous_album);
        }

        #[cfg(debug)]
        self.assert_invariants();
    }

    /// Return the duration of all unconsumed samples in milliseconds.
    pub fn pending_duration_ms(&self) -> u64 {
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
                    return Some(DecodeTask::Start(queued_track.track_id));
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
        // Store the sample rate in the queued track as well as in the block, so
        // we can compute the playback position in seconds even in case of a
        // buffer underrun, when there are no blocks.
        queued_track.sample_rate_hz = Some(result.block.format.sample_rate_hz);
        queued_track.blocks.push(result.block);
        queued_track.decode = match result.reader {
            Some(r) => Decode::Partial(r),
            None => Decode::Done,
        };
        self.current_decode = None;
    }
}

/// Decode the queue until we reach a set memory limit.
fn decode_burst(index: &dyn MetaIndex, state_mutex: &Mutex<PlayerState>) {
    // The decode thread is a trade-off between power consumption and memory
    // usage: decoding a lot in one go and then sleeping for a long time is more
    // efficient than decoding a bit all the time, because the CPU can be
    // downclocked in between the bursts. Also, if the time between disk
    // accesses is long enough, it might even be possible to spin down the disks
    // until the next batch of decodes, which keeps the system quiet too.
    // However, we do need to be able to hold all decoded samples in memory
    // then, and there is some risk of the decode being wasted work when the
    // queue changes. 85 MB will hold about 8 minutes of 16-bit 44.1 kHz audio,
    // 105 MB will hold about 10 minutes of 16-bit 44.1 kHz audio.
    // TODO: Make this configurable.
    let stop_after_bytes = 105_000_000;
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
        let decode_bytes_budget = decode_bytes_per_ms * pending_duration_ms as usize;
        let bytes_left = decode_bytes_budget.min(stop_after_bytes - bytes_used);
        println!("Pending buffer stats:");
        println!("  Duration: {:.3} seconds", pending_duration_ms as f32 / 1000.0);
        println!("  Memory:   {:.3} / {:.3} MB", bytes_used as f32 * 1e-6, stop_after_bytes as f32 * 1e-6);
        println!("  Budget:   {:.3} MB", bytes_left as f32 * 1e-6);

        // Decode at most 10 MB at a time. This ensures that we produce the data
        // in blocks of at most 10 MB, which in turn ensures that we can free
        // the memory early when we are done playing. Without this, when the
        // buffer runs low and we need to do a new decode, we might not be able
        // to decode as much, because most of the memory is taken up by
        // already-played samples in a large block where the playhead is at the
        // end of the block.
        let mut stop_after_bytes = bytes_left.min(10_000_000);

        // On the other hand, although we want to be quick and start playback
        // soon, if we produce too few samples per block, then the playback
        // thread will play those few samples and halt again, and then we get a
        // stuttering / plopping sound until the decoder has caught up. So also
        // put a lower bound on how little we can decode. Assuming 44.1 kHz
        // here, 50ms of audio is 2200 samples, which is 8800 bytes at 16 bit
        // stereo. For high bit rate or high sample rate this means there is a
        // bit more risk of stutter.
        stop_after_bytes = stop_after_bytes.max(8800);

        let result = task.run(index, stop_after_bytes);
        println!("Decoded {:.3} MB.", result.block.size_bytes() as f32 * 1e-6);
        previous_result = Some(result);
    }
}

/// The main loop for the decode thread.
///
/// Decodes until the in-memory buffer is full, then parks itself. When
/// unparked, if the buffer is running low, it starts a new burst of decode and
/// then parks itself again, etc.
fn decode_main(index: &dyn MetaIndex, state_mutex: &Mutex<PlayerState>) {
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

pub struct Player {
    state: Arc<Mutex<PlayerState>>,
    index: Arc<dyn MetaIndex + Send + Sync>,
    decode_thread: JoinHandle<()>,
    playback_thread: JoinHandle<()>,
    history_thread: JoinHandle<()>,
}

pub struct TrackSnapshot {
    /// Queue id of the queued track.
    pub queue_id: QueueId,

    /// Track id of the queued track.
    pub track_id: TrackId,

    /// The current playback position in the track, in milliseconds.
    pub position_ms: u64,

    /// The duration of the decoded but unplayed audio data, in milliseconds.
    pub buffered_ms: u64,

    /// Whether decoding is in progress for this track.
    ///
    /// This value goes to true when decoding is in progress, but when decoding
    /// is in progress for a long time, it usually means that the decode thread
    /// is blocked on IO. This can happen, for example when using spinning disks
    /// that need to spin up, or seek to the file.
    pub is_buffering: bool,
}

pub struct QueueSnapshot {
    /// The queued tracks, index 0 is the currently playing track.
    pub tracks: Vec<TrackSnapshot>,
}

impl Player {
    pub fn new(
        index: Arc<dyn MetaIndex + Send + Sync>,
        card_name: String,
        volume_name: String,
        db_path: PathBuf,
    ) -> Player {
        // Build the channel to send playback events to the history thread. That
        // thread is expected to process them immediately and be idle most of
        // the time, so pick a small channel size.
        let (sender, receiver) = mpsc::sync_channel(5);

        let state = Arc::new(Mutex::new(PlayerState::new(sender)));

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
                    &card_name,
                    &volume_name,
                    &*state_mutex_for_playback,
                    &decode_thread_for_playback,
                );
            }).unwrap();

        let builder = std::thread::Builder::new();
        let index_for_history = index.clone();

        let history_join_handle = builder
            .name("history".into())
            .spawn(move || {
                history::main(
                    db_path,
                    &*index_for_history,
                    receiver,
                );
            }).unwrap();

        Player {
            state: state,
            index: index,
            decode_thread: decode_join_handle,
            playback_thread: playback_join_handle,
            history_thread: history_join_handle,
        }
    }

    /// Wait for the playback and decode thread to finish.
    pub fn join(self) {
        // Note: currently there is no way to to signal these threads to stop,
        // so this will block indefinitely.
        self.playback_thread.join().unwrap();
        self.decode_thread.join().unwrap();
        self.history_thread.join().unwrap();
    }

    /// Enqueue the track for playback at the end of the queue.
    pub fn enqueue(&self, track_id: TrackId) -> QueueId {
        let track = self.index.get_track(track_id).expect("Can only enqueue existing tracks.");
        let album = self.index.get_album(track.album_id).expect("Track must belong to album.");
        let track_loudness = track.loudness.unwrap_or(Lufs::default());
        let album_loudness = album.loudness.unwrap_or(Lufs::default());

        // If the queue is empty, then the playback thread may be parked,
        // so we may need to wake it after enqueuing something.
        let (queue_id, needs_wake) = {
            let mut state = self.state.lock().unwrap();
            let needs_wake = state.is_queue_empty();
            let id = state.next_unused_id;
            state.next_unused_id = QueueId(id.0 + 1);
            let qt = QueuedTrack::new(id, track_id, track.album_id, track_loudness, album_loudness);
            state.enqueue(qt);
            (id, needs_wake)
        };

        if needs_wake {
            self.playback_thread.thread().unpark();
        }

        queue_id
    }

    /// Return a snapshot of the queue.
    pub fn get_queue(&self) -> QueueSnapshot {
        let state = self.state.lock().unwrap();

        let mut tracks = Vec::with_capacity(state.queue.len());
        for queued_track in state.queue.iter() {
            let t = TrackSnapshot {
                queue_id: queued_track.queue_id,
                track_id: queued_track.track_id,
                position_ms: queued_track.position_ms(),
                buffered_ms: queued_track.duration_ms(),
                is_buffering: match queued_track.decode {
                    Decode::Running => true,
                    _ => false,
                },
            };
            tracks.push(t);
        }

        QueueSnapshot {
            tracks: tracks,
        }
    }

    /// Return the current playback volume.
    pub fn get_volume(&self) -> Millibel {
        let state = self.state.lock().unwrap();
        state.volume
    }

    /// Add a (possibly negative) amount to the current volume, return the new volume.
    pub fn change_volume(&self, add: Millibel) -> Millibel {
        let mut state = self.state.lock().unwrap();
        state.volume.0 += add.0;

        // It makes no sense to crank up the volume further than the target
        // loudness: an extremely loud track at 0 LUFS played at a volume of
        // 0 dB would be toned bown by target_loudness to reach the target
        // loudness, so we can turn up the volume by that amount to make things
        // louder without exceeding full scale.
        state.volume = state.volume.min(Millibel(-state.target_loudness.0.get()));
        // -60 dB is low enough to be pretty much silent.
        state.volume = state.volume.max(Millibel(-6000));

        state.volume
    }
}
