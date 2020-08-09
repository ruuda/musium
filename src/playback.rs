// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for playing back audio using Alsa.

use std::mem;
use std::result;
use std::sync::Mutex;
use std::thread::Thread;

use alsa;
use alsa::PollDescriptors;
use nix::errno::Errno;

use player::{Format, PlayerState};

type Result<T> = result::Result<T, alsa::Error>;

fn print_available_cards() -> Result<()> {
    let cards = alsa::card::Iter::new();
    let mut found_any = false;

    for res_card in cards {
        if found_any {
            println!();
        }

        let card = res_card?;
        println!("Name:       {}", card.get_name()?);
        println!("Long name:  {}", card.get_longname()?);

        let non_block = false;
        let ctl = alsa::ctl::Ctl::from_card(&card, non_block)?;
        let info = ctl.card_info()?;
        println!("Card id:    {}", info.get_id()?);
        println!("Driver:     {}", info.get_driver()?);
        println!("Components: {}", info.get_components()?);
        println!("Mixer name: {}", info.get_mixername()?);

        found_any = true;
    }

    if !found_any {
        println!("No cards found.");
        println!("You may need to be a member of the 'audio' group.");
    }

    Ok(())
}

fn open_device(card_name: &str) -> Result<alsa::PCM> {
    let cards = alsa::card::Iter::new();
    let mut opt_card_index = None;

    for res_card in cards {
        let card = res_card?;
        if card.get_name()? == card_name {
            opt_card_index = Some(card.get_index());
        }
    }

    let card_index = match opt_card_index {
        Some(i) => i,
        None => {
            println!("Could not find a card with name '{}'.", card_name);
            println!("Valid options:\n");
            print_available_cards()?;
            panic!("TODO: Add a better error handler.");
        }
    };

    // Select the card by index (":{}") to get direct access to the hardware,
    // play back stereo on the front two speakers. Adding "plug:" in front makes
    // Alsa take care of conversions where needed. This is bad on the one hand,
    // because I would not want e.g. silent sample rate conversion, but on the
    // other hand, I have a UCM404HD, and it supports exactly 4 channels in "hw"
    // mode, so then I would have to manually fill the two other channels with
    // silence, and I don't feel like doing that right now. Even when selecting
    // "front" without "plug", the minimum number of channels is 4, even though
    // https://alsa-project.org/wiki/DeviceNames claims that for "front" we
    // would get stereo.
    let device = format!("plug:front:{}", card_index);
    let non_block = false;
    let pcm = match alsa::PCM::new(&device, alsa::Direction::Playback, non_block) {
        Ok(pcm) => pcm,
        Err(error) if error.errno() == Some(Errno::EBUSY) => {
            println!("Could not open audio interface for exclusive access, it is already use.");
            return Err(error);
        }
        err => return err,
    };

    Ok(pcm)
}

fn set_format(pcm: &alsa::PCM, format: Format) -> Result<()> {
    let sample_format = match format.bits_per_sample {
        16 => alsa::pcm::Format::S16LE,
        // Note the "3" in the format here: this means that every sample is 3
        // bytes. The regular S24LE format uses 4 bytes per sample, with the
        // most significant byte being zero.
        24 => alsa::pcm::Format::S243LE,
        // Files with unsupported bit depths are filtered out at index time.
        // They could still occur here if the index is outdated, but that is not
        // something that deserves special error handling, just crash it.
        n  => panic!("Unsupported: {} bits per sample. Please re-index.", n),
    };

    {
        let hwp = alsa::pcm::HwParams::any(&pcm)?;
        // TOOD: Confirm by first querying the device without "plug:" that it
        // supports this sample rate and format without plugin involvement (to
        // ensure that the plugin is only responsible for channel count
        // conversion). Alternatively, do the channel conversion manually.
        hwp.set_channels(2)?;
        hwp.set_rate(format.sample_rate_hz, alsa::ValueOr::Nearest)?;
        hwp.set_format(sample_format)?;
        hwp.set_access(alsa::pcm::Access::MMapInterleaved)?;
        // TODO: Pick a good buffer size.
        hwp.set_buffer_size(2048)?;
        hwp.set_period_size(256, alsa::ValueOr::Nearest)?;
        pcm.hw_params(&hwp)?;
    }

    {
        let hwp = pcm.hw_params_current()?;
        let swp = pcm.sw_params_current()?;
        let buffer_len = hwp.get_buffer_size()?;
        let period_len = hwp.get_period_size()?;
        swp.set_start_threshold(buffer_len - period_len)?;
        swp.set_avail_min(period_len)?;
        pcm.sw_params(&swp)?;

        assert_eq!(hwp.get_channels()?, 2);
        assert_eq!(hwp.get_rate()?, format.sample_rate_hz);
        assert_eq!(hwp.get_format()?, sample_format);
    }

    Ok(())
}

enum WriteResult {
    ChangeFormat(Format),
    QueueEmpty,
    NeedMore,
    Yield,
}

fn write_samples(
    pcm: &alsa::PCM,
    current_format: Format,
    io: &mut alsa::pcm::IO<u8>,
    player: &mut PlayerState,
) -> Result<WriteResult> {
    use alsa::pcm::State;

    let mut next_format = None;
    let mut n_consumed = 0;

    // Query how many frames are available for writing. If the device is in a
    // failed state, for example because of an underrun, then this fails, and
    // we need to recover. Recover once, if that does not help, propagate the
    // error.
    let n_available = match pcm.avail_update() {
        Ok(n) => n,
        Err(err) => {
            let silent = true;
            pcm.try_recover(err, silent)?;
            pcm.avail_update()?
        }
    } as usize;

    if n_available > 0 {
        n_consumed = match player.peek_mut() {
            Some(ref block) if current_format != block.format() => {
                // Next block has a different sample rate or bit depth, finish
                // what is still in the buffer, so we can switch afterwards.
                pcm.drain()?;
                next_format = Some(block.format());
                0
            }
            Some(block) => {
                let num_channels = 2;
                let samples_written = num_channels * io.mmap(n_available, |dst| {
                    let src = block.slice();
                    let n = dst.len().min(src.len());
                    &mut dst[..n].copy_from_slice(&src[..n]);
                    // We have to return the number of frames (count independent
                    // of the number of channels), but we have bytes.
                    n / (num_channels * current_format.bits_per_sample as usize / 8)
                })?;
                samples_written
            }
            None => 0,
        };

        if n_consumed > 0 {
            player.consume(n_consumed);
        } else if player.is_queue_empty() {
            // The queue is empty, play what is still there, then stop.
            pcm.drain()?;
        }
    }

    match pcm.state() {
        State::Running  => return Ok(WriteResult::Yield),
        State::Draining => match next_format {
            Some(_) => return Ok(WriteResult::Yield),
            None if player.is_queue_empty() => return Ok(WriteResult::QueueEmpty),
            None => panic!("PCM is unexpectedly in draining state."),
        }
        State::Setup => match next_format {
            Some(format) => return Ok(WriteResult::ChangeFormat(format)),
            None if player.is_queue_empty() => return Ok(WriteResult::QueueEmpty),
            // The queue is not empty, but we have no data nonetheless, which
            // means the decoder is behind ... yield and hope that next round it
            // caught up.
            None => return Ok(WriteResult::Yield),
        }
        State::Prepared if n_consumed > 0 => pcm.start()?,
        State::Prepared => return Ok(WriteResult::Yield),
        State::XRun => pcm.prepare()?,
        State::Suspended => pcm.resume()?,
        unexpected => panic!("Unexpected PCM state: {:?}", unexpected),
    };
    Ok(WriteResult::NeedMore)
}

enum FillResult {
    ChangeFormat(Format),
    QueueEmpty,
    Yield,
}

fn ensure_buffers_full(
    device: &alsa::PCM,
    format: Format,
    io: &mut alsa::pcm::IO<u8>,
    player: &mut PlayerState,
) -> FillResult {
    loop {
        match write_samples(device, format, io, player).expect("TODO: Failed to write samples.") {
            WriteResult::NeedMore => continue,
            WriteResult::ChangeFormat(new_format) => return FillResult::ChangeFormat(new_format),
            WriteResult::Yield => return FillResult::Yield,
            WriteResult::QueueEmpty => return FillResult::QueueEmpty,
        }
    }
}

/// Run a loop that keeps plays back what is in the queue.
///
/// When the queue becomes empty, this thread exits, and the Alsa device is
/// released. A new thread can be started once there is new content in the
/// queue.
pub fn main(
    card_name: &str,
    state_mutex: &Mutex<PlayerState>,
    decode_thread: &Thread,
) {
    let device = open_device(card_name).expect("TODO: Failed to open device.");
    let mut fds = device.get().expect("TODO: Failed to get fds from device.");

    let mut format = Format {
        sample_rate_hz: 44_100,
        bits_per_sample: 16,
    };
    set_format(&device, format).expect("TODO: Failed to set format.");

    // There is also "direct mode" that works with mmaps, but it is not
    // supported by the kernel on ARM, and I want to run this on a Raspberry Pi,
    // so for simplicity I will use the mode that is supported everywhere.
    let mut io = device.io();

    loop {
        let (result, is_buffer_low) = {
            let mut state = state_mutex.lock().unwrap();
            let result = ensure_buffers_full(
                &device,
                format,
                &mut io,
                &mut state
            );

            // If the tracks to play are on a spinning disk that is currently
            // not spinning to save power, spinning it up could take 10 to 15
            // seconds, so we may already need to wake up the decoder thread now
            // if we don't want to run out of data later. Use a safe margin of
            // 30s. TODO: Deduplicate this check.
            let is_buffer_low = state.pending_duration_ms() < 30_000;

            (result, is_buffer_low)
        };

        if is_buffer_low {
            decode_thread.unpark();
        }

        match result {
            FillResult::QueueEmpty => return,
            FillResult::Yield => {
                let max_sleep_ms = if is_buffer_low { 20 } else { 5_000 };
                alsa::poll::poll(&mut fds, max_sleep_ms).expect("TODO: Failed to wait for events.");
            }
            FillResult::ChangeFormat(new_format) => {
                mem::drop(io);
                set_format(&device, new_format).expect("TODO: Failed to set format.");
                format = new_format;
                io = device.io();
            }
        }
    }
}
