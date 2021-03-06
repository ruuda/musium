// Musium -- Music playback daemon with web-based library browser
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
use std::thread;
use std::ffi::CString;

use alsa;
use alsa::PollDescriptors;
use nix::errno::Errno;

use crate::player::{Format, Millibel, PlayerState};

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

fn open_device(card_name: &str) -> Result<(alsa::PCM, alsa::Mixer)> {
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
        Err(error) => return Err(error),
    };

    let device = format!("hw:{}", card_index);
    let non_block = false;
    let mixer = alsa::Mixer::new(&device, non_block)?;

    Ok((pcm, mixer))
}

fn get_volume_control<'a>(mixer: &'a alsa::Mixer, name: &str) -> Option<alsa::mixer::Selem<'a>> {
    let mut selem_id = alsa::mixer::SelemId::empty();
    selem_id.set_name(&CString::new(name).expect("Invalid volume control name."));
    let selem = mixer.find_selem(&selem_id)?;
    assert!(selem.has_playback_volume());
    Some(selem)
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
                // TODO: This can apparently cause Error("snd_pcm_mmap_commit", Sys(EPIPE).
                // How to handle it?
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
        match write_samples(device, format, io, player) {
            Err(err) => {
                println!("Error while writing samples: {:?}", err);
                println!("Resuming ...");
                continue
            }
            Ok(WriteResult::NeedMore) => continue,
            Ok(WriteResult::ChangeFormat(new_format)) => return FillResult::ChangeFormat(new_format),
            Ok(WriteResult::Yield) => return FillResult::Yield,
            Ok(WriteResult::QueueEmpty) => return FillResult::QueueEmpty,
        }
    }
}

/// Run a loop that keeps plays back what is in the queue.
///
/// When the queue becomes empty, this function returns, and the Alsa device is
/// released. An outer loop can call it again once there is new content in the
/// queue.
fn play_queue(
    card_name: &str,
    volume_name: &str,
    state_mutex: &Mutex<PlayerState>,
    decode_thread: &Thread,
) {
    let (device, mixer) = open_device(card_name).expect("TODO: Failed to open device.");
    let vc = get_volume_control(&mixer, volume_name).expect("TODO: Failed to get volume control.");
    let mut fds = device.get().expect("TODO: Failed to get fds from device.");

    let mut volume = None;
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
        let (result, target_volume, needs_decode, pending_ms) = {
            let mut state = state_mutex.lock().unwrap();
            let result = ensure_buffers_full(
                &device,
                format,
                &mut io,
                &mut state
            );

            (
                result,
                state.target_volume_full_scale(),
                state.needs_decode(),
                state.pending_duration_ms(),
            )
        };

        if needs_decode {
            decode_thread.unpark();
        }

        if volume != target_volume {
            if let Some(Millibel(v)) = target_volume {
                println!("Changing volume to {:.1} dB", v as f32 * 0.01);
                vc.set_playback_db_all(alsa::mixer::MilliBel(v as i64), alsa::Round::Floor)
                    .expect("Failed to set volume. TODO: Make fn return Alsa error?");
                volume = target_volume;
            }
        }

        match result {
            FillResult::QueueEmpty => return,
            FillResult::Yield => {
                let max_sleep_ms = 5_000.min(pending_ms as i32 / 2);
                alsa::poll::poll(&mut fds, max_sleep_ms).expect("TODO: Failed to wait for events.");
            }
            FillResult::ChangeFormat(new_format) => {
                mem::drop(io);
                set_format(&device, new_format).expect("TODO: Failed to set format.");
                println!("Changed format to {:?}", new_format);
                format = new_format;
                io = device.io();
            }
        }
    }
}

/// Play audio from the queue, then park the thread.
///
/// When the thread that runs this is unparked, check if there is anything in
/// the queue to play, and if so, open the Alsa device and start playing. When
/// the queue is empty, the device is released, and the thread parks itself
/// again.
pub fn main(
    card_name: &str,
    volume_name: &str,
    state_mutex: &Mutex<PlayerState>,
    decode_thread: &Thread,
) {
    // TODO: Set thread priority to high.
    loop {
        let has_audio = {
            let state = state_mutex.lock().unwrap();
            !state.is_queue_empty()
        };
        if has_audio {
            println!("Starting playback ...");
            play_queue(card_name, volume_name, state_mutex, decode_thread);
            println!("Playback done, sleeping ...");
        }
        thread::park();
    }
}
