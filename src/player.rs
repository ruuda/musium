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
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

use crate::config::Config;
use crate::error::Error;
use crate::exec_pre_post;
use crate::history;
use crate::history::PlaybackEvent;
use crate::mvar::Var;
use crate::playback;
use crate::playcount::PlayCounter;
use crate::prim::Hertz;
use crate::shuffle;
use crate::user_data::{Rating, UserData};
use crate::{AlbumId, Lufs, MemoryMetaIndex, MetaIndex, TrackId};
use claxon;
use claxon::metadata::StreamInfo;

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

impl QueueId {
    #[inline]
    pub fn parse(src: &str) -> Option<QueueId> {
        u64::from_str_radix(src, 16).ok().map(QueueId)
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

impl FromStr for Millibel {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Millibel, &'static str> {
        match s.strip_suffix(" dB") {
            None => Err("Expected integer dB value of the form '10 dB', but the dB suffix is missing."),
            Some(num) => match i16::from_str(num) {
                Err(_) => Err("Expected integer dB value of the form '10 dB', but the number is invalid."),
                // The value is decibel, but the representation millibel.
                Ok(x) => Ok(Millibel(x * 100)),
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Format {
    pub sample_rate: Hertz,
    pub bits_per_sample: u32,
}

impl Default for Format {
    fn default() -> Format {
        Format {
            sample_rate: Hertz(44_100),
            bits_per_sample: 16,
        }
    }
}

/// A block of interleaved samples, queued for playback.
pub struct Block {
    /// The samples, interleaved left, right.
    data: SampleData,

    /// The number of samples consumed.
    pos: usize,

    /// The sample rate of this block.
    ///
    /// The bit depth is already implied by the `data` enum.
    sample_rate: Hertz,
}

/// A 16-bit stereo sample, left and right channels.
pub struct SampleI16(pub i16, pub i16);

/// A 24-bit stereo sample, left and right channels.
pub struct SampleI24([i16; 3]);

impl SampleI24 {
    pub fn new(left: i32, right: i32) -> SampleI24 {
        let mut data_i16 = [0_i16; 3];
        let data: &mut [u8; 6] = unsafe { mem::transmute(&mut data_i16) };
        data[0] = ((left >> 0) & 0xff) as u8;
        data[1] = ((left >> 8) & 0xff) as u8;
        data[2] = ((left >> 16) & 0xff) as u8;
        data[3] = ((right >> 0) & 0xff) as u8;
        data[4] = ((right >> 8) & 0xff) as u8;
        data[5] = ((right >> 16) & 0xff) as u8;
        SampleI24(data_i16)
    }
    pub fn as_channels(&self) -> (i32, i32) {
        let data: &[u8; 6] = unsafe { mem::transmute(&self.0) };
        let ul = (data[0] as i32) | ((data[1] as i32) << 8) | ((data[2] as i32) << 16);
        let ur = (data[3] as i32) | ((data[4] as i32) << 8) | ((data[5] as i32) << 16);
        // When we reconstruct into i32, we are missing the top byte. This means
        // the value as i32 is incorrect, because it may be missing the sign bits.
        // We can fix that with a double shift: the right-shift is sign-extending
        // on i32.
        (
            (ul << 8) >> 8,
            (ur << 8) >> 8,
        )
    }
}

/// Decoded stereo audio data, with left and right samples.
pub enum SampleData {
    /// Decoded 16 bits per sample stereo audio.
    I16(Vec<SampleI16>),

    /// Decoded 24 bits per sample stereo audio.
    I24(Vec<SampleI24>),
}

/// Like [`SampleData`], but borrowed rather than owned.
pub enum SampleDataSlice<'a> {
    I16(&'a [SampleI16]),
    I24(&'a [SampleI24]),
}

impl SampleData {
    pub fn len(&self) -> usize {
        match self {
            SampleData::I16(data) => data.len(),
            SampleData::I24(data) => data.len(),
        }
    }
}

impl Block {
    pub fn new_i16(sample_rate: Hertz, data: Vec<SampleI16>) -> Block {
        assert!(!data.is_empty(), "Blocks must not be empty.");
        Block {
            data: SampleData::I16(data),
            pos: 0,
            sample_rate,
        }
    }

    pub fn new_i24(sample_rate: Hertz, data: Vec<SampleI24>) -> Block {
        assert!(!data.is_empty(), "Blocks must not be empty.");
        Block {
            data: SampleData::I24(data),
            pos: 0,
            sample_rate,
        }
    }

    pub fn slice(&self) -> SampleDataSlice {
        match &self.data {
            SampleData::I16(data) => SampleDataSlice::I16(&data[self.pos..]),
            SampleData::I24(data) => SampleDataSlice::I24(&data[self.pos..]),
        }
    }

    pub fn sample_rate(&self) -> Hertz {
        self.sample_rate
    }

    /// Consume n samples.
    fn consume(&mut self, n: usize) {
        self.pos += n;
        debug_assert!(self.pos <= self.data.len());
    }

    /// Return the number of unconsumed samples left.
    pub fn len(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        // Multiply by 1000 to go from seconds to milliseconds. We need to
        // work with u64 here, because around 100s of stereo 44.1 kHz audio,
        // the sample count times 1000 overflows a u32 (and usize can be 32
        // bits). We can't move the 1000 into the denominator, because the
        // common sample rate of 44.1 kHz is not a multiple of 1000.
        self.len() as u64 * 1000 / self.sample_rate.0 as u64
    }

    /// Return the size of the block (including consumed samples) in bytes.
    pub fn size_bytes(&self) -> usize {
        match &self.data {
            SampleData::I16(data) => data.capacity() * 4,
            SampleData::I24(data) => data.capacity() * 6,
        }
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
    pub queue_id: QueueId,

    /// Track id of the track to be played.
    pub track_id: TrackId,

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
    sample_rate: Option<Hertz>,

    /// Decoder for this track.
    decode: Decode,
}

impl QueuedTrack {
    pub fn new(
        queue_id: QueueId,
        track_id: TrackId,
        track_loudness: Lufs,
        album_loudness: Lufs,
    ) -> QueuedTrack {
        QueuedTrack {
            queue_id: queue_id,
            track_id: track_id,
            track_loudness: track_loudness,
            album_loudness: album_loudness,
            blocks: Vec::new(),
            samples_played: 0,
            sample_rate: None,
            decode: Decode::NotStarted,
        }
    }

    pub fn album_id(&self) -> AlbumId {
        self.track_id.album_id()
    }

    /// Return the duration of the unconsumed samples in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.blocks.iter().map(|b| b.duration_ms()).sum()
    }

    /// Return the duration of the consumed samples in milliseconds.
    pub fn position_ms(&self) -> u64 {
        match self.sample_rate {
            // Multiply by 1000 to go from seconds to milliseconds. We need to
            // work with u64 here, because around 100s of stereo 44.1 kHz audio,
            // the sample count times 1000 overflows a u32 (and usize can be 32
            // bits). We can't move the 1000 into the denominator, because the
            // common sample rate of 44.1 kHz is not a multiple of 1000.
            Some(Hertz(hz)) => self.samples_played * 1000 / (hz as u64),
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
enum DecodeTask {
    /// Continue decoding with the given reader.
    Continue(QueueId, FlacReader),

    /// Start decoding a new track.
    Start(QueueId, TrackId),
}

/// The result of a decode task.
///
/// If the file has been fully decoded, the reader is `None`, if there is more
/// to decode, it is returned here.
pub struct DecodeResult {
    queue_id: QueueId,
    block: Block,
    reader: Option<FlacReader>,
}

/// Open a file, and `fadvise` that we will read it entirely.
///
/// In the decoder we might sometimes open a file, then decode it partially
/// (because our buffer is full), and then resume decoding only a long time
/// later. Possibly a disk will spin down. Tell the kernel that we are going to
/// want the entire thing, so we can later finish decoding without having to
/// spin the disk up again (for this particular file).
///
/// A related scenario that sometimes happens is that the disk is spun down, but
/// a part of a file is still cached in the page cache. Then if we play it,
/// playback starts immediately, but then gets stuck half-way because the
/// decoder is waiting for the rest of the file. There too, it can help to tell
/// the kernel early that we will need the entire thing. (Though probably it’s
/// still too late, because decoding is fast, so we would have hit the blocking
/// IO anyway within a few seconds.)
fn open_with_readahead(fname: &str) -> crate::error::Result<FlacReader> {
    use std::os::unix::io::AsRawFd;
    let file = fs::File::open(fname)?;
    let fd = file.as_raw_fd();
    let offset = 0;
    let len = file.metadata()?.len() as libc::off64_t;
    unsafe {
        let _ = libc::posix_fadvise64(fd, offset, len, libc::POSIX_FADV_SEQUENTIAL);
        let _ = libc::posix_fadvise64(fd, offset, len, libc::POSIX_FADV_WILLNEED);
    }
    let reader = match FlacReader::new(file) {
        Ok(r) => r,
        Err(err) => return Err(Error::FormatError(fname.into(), err)),
    };
    Ok(reader)
}

impl DecodeTask {
    /// Decode until the end of the file, or until we produced more than `stop_after_bytes`.
    pub fn run(
        self,
        index: &dyn MetaIndex,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        match self {
            DecodeTask::Continue(qid, reader) => {
                DecodeTask::decode(qid, reader, stop_after_bytes)
            }
            DecodeTask::Start(qid, track_id) => {
                DecodeTask::start(index, qid, track_id, stop_after_bytes)
            }
        }
    }

    fn start(
        index: &dyn MetaIndex,
        queue_id: QueueId,
        track_id: TrackId,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        let track = match index.get_track(track_id) {
            Some(t) => t,
            None => panic!("Track {} does not exist, how did it end up queued?", track_id),
        };
        let fname = index.get_filename(track.filename);
        println!("Decode: opening file, file={:?}", fname);

        let reader = match open_with_readahead(fname) {
            Ok(r) => r,
            Err(err) => {
                println!("Error in {:?}: {:?}", fname, err);
                return DecodeResult {
                    queue_id,
                    block: Block::new_i16(Hertz(44_100), Vec::new()),
                    reader: None,
                };
            }
        };

        DecodeTask::decode(queue_id, reader, stop_after_bytes)
    }

    fn decode(queue_id: QueueId, reader: FlacReader, stop_after_bytes: usize) -> DecodeResult {
        let streaminfo = reader.streaminfo();
        match streaminfo.bits_per_sample {
            16 => DecodeTask::decode_i16(queue_id, reader, streaminfo, stop_after_bytes),
            24 => DecodeTask::decode_i24(queue_id, reader, streaminfo, stop_after_bytes),
            n  => panic!("Unsupported bit depth: {}", n),
        }
    }

    fn decode_i16(
        queue_id: QueueId,
        mut reader: FlacReader,
        streaminfo: StreamInfo,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 16);
        assert_eq!(streaminfo.channels, 2);

        // The block size counts inter-channel samples, and our element is a
        // stereo sample, so we don't need to multiply by two here!
        let max_samples_per_frame = streaminfo.max_block_size as usize;
        let mut is_done = false;
        let mut out = Vec::with_capacity(stop_after_bytes / 4 + max_samples_per_frame);

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
                    out.push(SampleI16(l as i16, r as i16));
                }

                buffer = frame.into_buffer();
            }
        }

        DecodeResult {
            queue_id,
            block: Block::new_i16(Hertz(streaminfo.sample_rate as i32), out),
            reader: if is_done { None } else { Some(reader) }
        }
    }

    fn decode_i24(
        queue_id: QueueId,
        mut reader: FlacReader,
        streaminfo: StreamInfo,
        stop_after_bytes: usize,
    ) -> DecodeResult {
        assert_eq!(streaminfo.bits_per_sample, 24);
        assert_eq!(streaminfo.channels, 2);

        // The block size counts inter-channel samples, and our element is a
        // stereo sample, so we don't need to multiply by two here!
        let max_samples_per_frame = streaminfo.max_block_size as usize;
        let mut is_done = false;
        let mut out = Vec::with_capacity(stop_after_bytes / 6 + max_samples_per_frame);

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
                    out.push(SampleI24::new(l, r));
                }

                buffer = frame.into_buffer();
            }
        }

        DecodeResult {
            queue_id,
            block: Block::new_i24(Hertz(streaminfo.sample_rate as i32), out),
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

    /// Target cutoff frequency for the high-pass filter.
    ///
    /// Setting this to 0 effectively disables the high pass filter.
    high_pass_cutoff: Hertz,

    /// The tracks pending playback. Element 0 is being played currently.
    ///
    /// Invariant: If the queued track at index i has no decoded blocks, then
    /// for every index j > i, the queued track at index j has no decoded
    /// blocks either. In other words, all decoded blocks are at the beginning
    /// of the queue.
    queue: Vec<QueuedTrack>,

    /// Sender for playback events.
    ///
    /// These events get consumed by the history thread, who logs them.
    events: SyncSender<PlaybackEvent>,

    /// Random number generator used for shuffling.
    rng: shuffle::Prng,
}


impl PlayerState {
    pub fn new(volume: Millibel, high_pass_cutoff: Hertz, events: SyncSender<PlaybackEvent>) -> PlayerState {
        PlayerState {
            next_unused_id: QueueId(0),
            volume,
            target_loudness: Lufs::new(-2300),
            current_track_loudness: None,
            high_pass_cutoff,
            queue: Vec::new(),
            events,
            rng: shuffle::Prng::new(),
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
            .filter(|qt| matches!(qt.decode, Decode::Running))
            .count();
        assert!(n_running <= 1, "At most one decode should be in progress.");

        for (qt0, qt1) in self.queue.iter().zip(self.queue.iter().skip(1)) {
            assert!(
                matches!(
                    (&qt0.decode, &qt1.decode),
                    (Decode::Done, Decode::Done)
                    | (Decode::Done, Decode::Running)
                    | (Decode::Done, Decode::Partial(..))
                    | (Decode::Running, Decode::NotStarted)
                    | (Decode::Partial(..), Decode::NotStarted)
                    | (Decode::NotStarted, Decode::NotStarted)
                ),
                "Decoding must happen at the front of the queue.",
            );
        }

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

    /// Return the current cutoff frequency for the high pass filter.
    pub fn target_high_pass_cutoff(&self) -> Hertz {
        self.high_pass_cutoff
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
            Some(next_track) if current_track.album_id() == next_track.album_id() => current_track.album_loudness,
            _ if current_track.album_id() == previous_album => current_track.album_loudness,
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

    /// Dequeue the track, if it exists and is not currently playing.
    pub fn dequeue(&mut self, queue_id: QueueId) {
        match self.queue.iter().position(|qt| qt.queue_id == queue_id) {
            // If the track is currently playing, we cannot remove it from the
            // queue.
            Some(0) => return,
            None => return,
            Some(i) => self.queue.remove(i),
        };
    }

    /// Shuffle the queue.
    pub fn shuffle(&mut self, index: &MemoryMetaIndex) {
        if self.queue.len() < 3 {
            // The track at index 0 is being played, we cannot move it, and then
            // we need at least 2 more tracks to be able to shuffle anything at
            // all.
            return;
        }

        let tracks = &mut self.queue[1..];
        shuffle::shuffle(index, &mut self.rng, tracks);

        // After the shuffle, the invariant that decoded samples are at the
        // front of the queue may be violated, so we need to restore that.
        let mut should_clear = false;
        for queued_track in self.queue.iter_mut() {
            if should_clear {
                // Note, if the decode was running and we set it to not started
                // now, the decode result will simply be dropped once the decode
                // thread finishes the task.
                queued_track.decode = Decode::NotStarted;
                queued_track.blocks.clear();
            } else {
                // If we still have any tracks done decoding in the front that's
                // great, we can keep the samples, but as soon as there is any
                // decode not done, everything after that needs to be cleared.
                match queued_track.decode {
                    Decode::Done => continue,
                    Decode::Running => should_clear = true,
                    Decode::Partial(..) => should_clear = true,
                    Decode::NotStarted => should_clear = true,
                }
            }
        }

        #[cfg(debug)]
        self.assert_invariants();
    }

    /// Clear the play queue. Does not affect the currently playing track.
    pub fn clear_queue(&mut self) {
        self.queue.truncate(1);
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

            self.events.send(PlaybackEvent::Completed(track.queue_id, track.track_id))
                .expect("Failed to send completion event to history thread.");

            let previous_album = track.album_id();
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
        false
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
    fn take_decode_task(&mut self) -> Option<DecodeTask> {
        for queued_track in self.queue.iter_mut() {
            match queued_track.decode {
                Decode::Done => continue,
                Decode::Running => panic!("Can only take decode task when none is already in progress."),
                _ => {}
            }

            // If the decode is not done, then we will now start or continue,
            // so put on "running", and then inspect the previous state.
            let mut decode = Decode::Running;
            mem::swap(&mut decode, &mut queued_track.decode);

            let queue_id = queued_track.queue_id;

            match decode {
                Decode::NotStarted => {
                    return Some(DecodeTask::Start(queue_id, queued_track.track_id));
                }
                Decode::Partial(reader) => {
                    return Some(DecodeTask::Continue(queue_id, reader));
                }
                Decode::Running => {
                    unreachable!("Would have panicked already.");
                }
                Decode::Done => {
                    unreachable!("We skipped the iteration on Done.");
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
        for queued_track in self.queue.iter_mut() {
            match queued_track.decode {
                Decode::Done => {
                    // The track before us is already done, so this could not
                    // have been our task.
                    assert_ne!(queued_track.queue_id, result.queue_id);
                }
                Decode::Running => {
                    // We found the track that we were decoding.
                    assert_eq!(queued_track.queue_id, result.queue_id);

                    // Store the sample rate in the queued track as well as in
                    // the block, so we can compute the playback position in
                    // seconds even in case of a buffer underrun, when there are
                    // no blocks.
                    queued_track.sample_rate = Some(result.block.sample_rate);
                    queued_track.blocks.push(result.block);
                    queued_track.decode = match result.reader {
                        Some(r) => Decode::Partial(r),
                        None => Decode::Done,
                    };

                    break;
                }
                Decode::Partial(..) => {
                    panic!("If a decode was running, there cannot have been a partial one.");
                }
                Decode::NotStarted => {
                    // When we get to a not started entry, that's the end of
                    // where we have consecutive samples for. If we have a
                    // decode result, it is further down the queue; the queue
                    // may have changed, and the result is no longer relevant.
                    break;
                }
            }
        }
    }
}

/// Decode the queue until we reach a set memory limit.
fn decode_burst(index: &MemoryMetaIndex, state_mutex: &Mutex<PlayerState>) {
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
        // Decode at most 10 MB at a time. This ensures that we produce the data
        // in blocks of at most 10 MB, which in turn ensures that we can free
        // the memory early when we are done playing. Without this, when the
        // buffer runs low and we need to do a new decode, we might not be able
        // to decode as much, because most of the memory is taken up by
        // already-played samples in a large block where the playhead is at the
        // end of the block.
        let result = task.run(index, bytes_left.min(10_000_000));
        println!(
            "Buffer: duration={:.3}s, memory={:.3}/{:.3} MB, budget={:.3} MB, decoded={:.3} MB",
            pending_duration_ms as f32 / 1000.0,
            bytes_used as f32 * 1e-6,
            stop_after_bytes as f32 * 1e-6,
            bytes_left as f32 * 1e-6,
            result.block.size_bytes() as f32 * 1e-6,
        );
        previous_result = Some(result);
    }
}

/// The main loop for the decode thread.
///
/// Decodes until the in-memory buffer is full, then parks itself. When
/// unparked, if the buffer is running low, it starts a new burst of decode and
/// then parks itself again, etc.
fn decode_main(
    index: Var<MemoryMetaIndex>,
    state_mutex: &Mutex<PlayerState>,
) {
    loop {
        let should_decode = {
            let state = state_mutex.lock().unwrap();
            state.needs_decode()
        };


        if should_decode {
            let current_index = index.get();
            decode_burst(&current_index, state_mutex);
        }

        thread::park();
    }
}

pub struct Player {
    state: Arc<Mutex<PlayerState>>,
    decode_thread: JoinHandle<()>,
    playback_thread: JoinHandle<()>,
    history_thread: JoinHandle<()>,
    exec_pre_post_thread: JoinHandle<()>,
    events: SyncSender<PlaybackEvent>,
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

/// Runtime playback parameters: volume and filter cutoff.
pub struct Params {
    pub volume: Millibel,
    pub high_pass_cutoff: Hertz,
}

impl Player {
    pub fn new(
        index_var: Var<MemoryMetaIndex>,
        user_data: Arc<Mutex<UserData>>,
        counter: PlayCounter,
        config: &Config,
    ) -> Player {
        // Build the channel to send playback events to the history thread. That
        // thread is expected to process them immediately and be idle most of
        // the time, so pick a small channel size.
        let (hist_sender, hist_receiver) = mpsc::sync_channel(5);

        // Same for playback start and end queue events, for the exec thread.
        let (queue_events_sender, queue_events_receiver) = mpsc::sync_channel(5);

        let state = Arc::new(Mutex::new(PlayerState::new(
            config.volume,
            config.high_pass_cutoff,
            hist_sender.clone(),
        )));

        // Start the decode thread. It runs indefinitely, but we do need to
        // periodically unpark it when there is new stuff to decode.
        let state_mutex_for_decode = state.clone();
        let index_for_decode = index_var.clone();
        let builder = std::thread::Builder::new();
        let decode_join_handle = builder
            .name("decoder".into())
            .spawn(move || {
                decode_main(
                    index_for_decode,
                    &state_mutex_for_decode,
                );
            }).unwrap();

        let state_mutex_for_playback = state.clone();
        let decode_thread_for_playback = decode_join_handle.thread().clone();
        let config_for_playback = config.clone();
        let hist_sender_for_playback = hist_sender.clone();

        let builder = thread::Builder::new();
        let playback_join_handle = builder
            .name("playback".into())
            .spawn(move || {
                playback::main(
                    &config_for_playback,
                    state_mutex_for_playback,
                    &decode_thread_for_playback,
                    queue_events_sender,
                    hist_sender_for_playback,
                );
            }).unwrap();

        let builder = thread::Builder::new();
        let index_for_history = index_var;

        let db_path = config.db_path.clone();
        let history_join_handle = builder
            .name("history".into())
            .spawn(move || {
                let result = history::main(
                    &db_path,
                    index_for_history,
                    user_data,
                    counter,
                    hist_receiver,
                );
                // The history thread should not exit. When it does, that's a
                // problem.
                eprintln!("History thread exited: {:?}", result);
                std::process::exit(1);
            }).unwrap();

        let builder = std::thread::Builder::new();
        let config_exec = config.clone();
        let exec_pre_post_handle = builder
            .name("exec_pre_post".into())
            .spawn(move || exec_pre_post::main(
                &config_exec,
                queue_events_receiver,
            )).unwrap();

        Player {
            state: state,
            decode_thread: decode_join_handle,
            playback_thread: playback_join_handle,
            history_thread: history_join_handle,
            exec_pre_post_thread: exec_pre_post_handle,
            events: hist_sender,
        }
    }

    /// Wait for the playback and decode thread to finish.
    pub fn join(self) {
        // Note: currently there is no way to to signal these threads to stop,
        // so this will block indefinitely.
        self.playback_thread.join().unwrap();
        self.decode_thread.join().unwrap();
        self.history_thread.join().unwrap();
        self.exec_pre_post_thread.join().unwrap();
    }

    /// Send a track rating to the history thread for saving to the database.
    pub fn set_track_rating(&self, track_id: TrackId, rating: Rating) {
        self.events.send(PlaybackEvent::Rated { track_id, rating }).unwrap();
    }

    /// Enqueue the track for playback at the end of the queue.
    pub fn enqueue(&self, index: &MemoryMetaIndex, track_id: TrackId) -> QueueId {
        let album_id = track_id.album_id();
        let track = index.get_track(track_id).expect("Can only enqueue existing tracks.");
        let album = index.get_album(album_id).expect("Track must belong to album.");
        let track_loudness = track.loudness.unwrap_or_default();
        let album_loudness = album.loudness.unwrap_or_default();

        // If the queue is empty, then the playback thread may be parked,
        // so we may need to wake it after enqueuing something.
        let (queue_id, needs_wake) = {
            let mut state = self.state.lock().unwrap();
            let needs_wake = state.is_queue_empty();
            let id = state.next_unused_id;
            state.next_unused_id = QueueId(id.0 + 1);
            let qt = QueuedTrack::new(id, track_id, track_loudness, album_loudness);
            state.enqueue(qt);
            (id, needs_wake)
        };

        if needs_wake {
            self.playback_thread.thread().unpark();
        }

        queue_id
    }

    /// Enqueue the track for playback at the end of the queue.
    pub fn dequeue(&self, queue_id: QueueId) {
        self.state.lock().unwrap().dequeue(queue_id);
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
                is_buffering: matches!(queued_track.decode, Decode::Running),
            };
            tracks.push(t);
        }

        QueueSnapshot {
            tracks: tracks,
        }
    }

    /// Shuffle the queue.
    pub fn shuffle(&self, index: &MemoryMetaIndex) {
        self.state.lock().unwrap().shuffle(index);

        // After a shuffle, a new track may be following the current one, so
        // even if decoding was caught up before the shuffle, after the shuffle
        // we may need to start decoding right now.
        self.decode_thread.thread().unpark();
    }

    /// Shuffle the queue.
    pub fn clear_queue(&self) {
        self.state.lock().unwrap().clear_queue();
    }

    fn get_params_internal(state: &PlayerState) -> Params {
        Params {
            volume: state.volume,
            high_pass_cutoff: state.high_pass_cutoff,
        }
    }

    /// Return the current playback parameters.
    pub fn get_params(&self) -> Params {
        let state = self.state.lock().unwrap();
        Self::get_params_internal(&state)
    }

    /// Add a (possibly negative) amount to the current volume, return the new params.
    pub fn change_volume(&self, add: Millibel) -> Params {
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

        Self::get_params_internal(&state)
    }

    /// Add a (possibly negative) amount to the high pass filter cutoff, return the new params.
    pub fn change_cutoff(&self, add: Hertz) -> Params {
        let mut state = self.state.lock().unwrap();
        state.high_pass_cutoff.0 += add.0;
        state.high_pass_cutoff.0 = state.high_pass_cutoff.0.max(0);
        Self::get_params_internal(&state)
    }
}
