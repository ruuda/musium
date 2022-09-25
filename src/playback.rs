// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for playing back audio using Alsa.

use std::mem;
use std::result;
use std::ffi::CString;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::mpsc::SyncSender;
use std::thread::Thread;
use std::thread;

use alsa::PollDescriptors;
use alsa;
use libc;

use crate::config::Config;
use crate::exec_pre_post::QueueEvent;
use crate::player::{Format, Millibel, PlayerState};
use crate::prim::Hertz;

type Result<T> = result::Result<T, alsa::Error>;

fn print_available_cards() -> Result<()> {
    let cards = alsa::card::Iter::new();
    let mut found_any = false;

    for res_card in cards {
        let card = res_card?;
        println!("{}Name: {}", if found_any { "\n" } else { "" }, card.get_name()?);
        println!("  Long name:  {}", card.get_longname()?);

        let non_block = false;
        let ctl = alsa::ctl::Ctl::from_card(&card, non_block)?;
        let info = ctl.card_info()?;
        println!("  Card id:    {}", info.get_id()?);
        println!("  Driver:     {}", info.get_driver()?);
        println!("  Components: {}", info.get_components()?);
        println!("  Mixer name: {}", info.get_mixername()?);

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
            std::process::exit(1);
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
        Err(error) if error.errno() == alsa::nix::errno::Errno::EBUSY => {
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
        hwp.set_rate(format.sample_rate.0, alsa::ValueOr::Nearest)?;
        hwp.set_format(sample_format)?;
        hwp.set_access(alsa::pcm::Access::MMapInterleaved)?;
        hwp.set_period_size_near(256, alsa::ValueOr::Nearest)?;
        // TODO: Pick a good buffer size.
        hwp.set_buffer_size_near(2048)?;
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
        assert_eq!(hwp.get_rate()?, format.sample_rate.0);
        assert_eq!(hwp.get_format()?, sample_format);
    }

    Ok(())
}

enum WriteResult {
    /// We performed a state transition, but did not write; try again.
    Continue,

    /// We need to change the format before we can continue playback.
    ChangeFormat(Format),

    /// The queue is empty, playback is done for now.
    QueueEmpty,

    /// Done for now, check back later.
    ///
    /// This can happen either when the playback buffer is full, or when the
    /// decode buffer is empty. After a yield we release the stake lock, so if
    /// the decode thread has new blocks for the decode buffer, we will pick
    /// that up. And then we poll the file descriptor, until it is ready to
    /// write to again.
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

    // Query how many frames are available for writing. If the device is in a
    // failed state, for example because of an underrun, then this fails, and
    // we need to recover. Recover once, if that does not help, propagate the
    // error.
    let n_available = match pcm.avail_update() {
        Ok(n) => n,
        Err(err) => {
            println!("DEBUG: write_samples was in error, the error was {:?}.", err);
            let silent = false;
            pcm.try_recover(err, silent)?;
            println!("DEBUG: we are recovered now.");
            let n = pcm.avail_update()?;
            println!("DEBUG: avail_update after recover: {}.", n);
            n
        }
    } as usize;

    if n_available > 0 {
        let n_consumed = match player.peek_mut() {
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
                    dst[..n].copy_from_slice(&src[..n]);
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
        State::Running => Ok(WriteResult::Yield),
        State::Draining => match next_format {
            Some(_) => Ok(WriteResult::Yield),
            None if player.is_queue_empty() => Ok(WriteResult::QueueEmpty),
            None => panic!("PCM is unexpectedly in draining state."),
        }
        State::Setup => match next_format {
            Some(format) => Ok(WriteResult::ChangeFormat(format)),
            None if player.is_queue_empty() => Ok(WriteResult::QueueEmpty),
            // The queue is not empty, but we have no data nonetheless, which
            // means the decoder is behind ... yield and hope that next round it
            // caught up.
            None => Ok(WriteResult::Yield),
        }
        State::Prepared if n_available == 0 => {
            // If the PCM is ready for playback, and we topped up the buffer to
            // the point where we can write no more, then start playback, and
            // then yield, because there is nothing more to write anyway.
            pcm.start()?;
            Ok(WriteResult::Yield)
        }
        State::Prepared if player.is_queue_empty() => {
            // If the buffer is not topped up, but we don't have anything else
            // to put in the buffer, then we can also start, and immediately
            // drain.
            pcm.start()?;
            pcm.drain()?;
            Ok(WriteResult::QueueEmpty)
        }
        State::Prepared => {
            // If the PCM is ready for playback, but we are not in one of the
            // above two cases, then we could fill the buffer a bit more before
            // we start, which is a good idea to reduce the risk of buffer
            // underrun.
            Ok(WriteResult::Yield)
        }
        State::XRun => {
            pcm.prepare()?;
            Ok(WriteResult::Continue)
        }
        State::Suspended => {
            pcm.resume()?;
            Ok(WriteResult::Continue)
        }
        unexpected => panic!("Unexpected PCM state: {:?}", unexpected),
    }
}

enum FillResult {
    /// We need to change the format before we can continue playback.
    ChangeFormat(Format),

    /// The queue is empty, playback is done for now.
    QueueEmpty,

    /// Buffers are full for now, but we should check back later.
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
            Ok(WriteResult::Continue) => continue,
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
        sample_rate: Hertz(44_100),
        bits_per_sample: 16,
    };
    if let Err(err) = set_format(&device, format) {
        panic!("Failed to set format for device {} to format {:?}: {:?}",
            card_name, format, err,
        );
    }

    // There is also "direct mode" that works with mmaps, but it is not
    // supported by the kernel on ARM, and I want to run this on a Raspberry Pi,
    // so for simplicity I will use the mode that is supported everywhere.
    let mut io = device.io_bytes();

    loop {
        let (result, target_volume, needs_decode) = {
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
                // If we are in this loop, then we are already playing, so for
                // the sake of being responsive to songs starting, we don't have
                // to have a low timeout here. But for volume changes we might.
                let max_sleep_ms = 15;
                alsa::poll::poll(&mut fds, max_sleep_ms).expect("TODO: Failed to wait for events.");
            }
            FillResult::ChangeFormat(new_format) => {
                mem::drop(io);
                set_format(&device, new_format).expect("TODO: Failed to set format.");
                println!("Changed format to {:?}", new_format);
                format = new_format;
                io = device.io_bytes();
            }
        }
    }
}

/// Try to increase the scheduling priority of the current thread.
///
/// The playback thread is responsible for re-filling the audio card's buffer.
/// If it is too late, it might miss the deadline, and cause a buffer underrun.
/// To try and avoid this, instruct the OS to prioritize this thread when
/// multiple threads are runnable. We use two ways of doing this:
///
/// 1. The niceness, which affects scheduling relative to other threads for
///    SCHED_OTHER (the default) threads. Lower niceness is higher priority.
///
/// 2. The scheduling policy. We set it to "RR" (round robin), which makes
///    the thread higher priority than all other SCHED_OTHER threads, but will
///    make it share time in round-robin fashion with other SCHED_RR threads, if
///    there are any. See "man 7 sched" for details.
fn try_increase_thread_priority() {
    // We use niceness -11 (relative to the default 0), because that is what
    // Pipewire and Pulseaudio use by default.
    let new_nice = unsafe { libc::nice(-11) };
    if new_nice == -1 {
        println!("Playback thread likely failed to set niceness.");
        println!("Consider using setrlimit, granting CAP_SYS_NICE, \
                  or setting LimitNICE=-11:-11 when using systemd.");
    } else {
        println!("Playback thread new niceness: {}", new_nice);
    }

    let sched_policy = libc::SCHED_RR;
    let sched_param = libc::sched_param {
        // Priority runs from 0 (low) to 99 (high) on Linux,
        // but in any case this value is ignored for SCHED_RR threads.
        sched_priority: 50,
    };
    let sched_retval = unsafe {
        libc::pthread_setschedparam(
            libc::pthread_self(),
            sched_policy,
            &sched_param,
        )
    };
    match sched_retval {
        0 => println!(
            "Playback thread is now SCHED_RR (high priority)."
        ),
        libc::EPERM => println!(
            "Playback thread was not allowed to change its scheduling \
             policy to SCHED_RR. Consider granting CAP_SYS_NICE."
        ),
        _ => println!(
            "An unknown error occurred when setting playback thread \
             scheduling policy: {}.",
             sched_retval,
        ),
    }
}

/// Play audio from the queue, then park the thread.
///
/// When the thread that runs this is unparked, check if there is anything in
/// the queue to play, and if so, open the Alsa device and start playing. When
/// the queue is empty, the device is released, and the thread parks itself
/// again.
///
/// This thread tries to boost its own priority. It is usually not necessary,
/// especially not at 16 bit / 44.1 kHz, but for 24 bit / 192 kHz audio, I
/// noticed occasional periods of silence of ~1s, caused by a buffer underrun
/// and then having to reset the device. Since increasing the thread priority,
/// I have not observed such stutters any more even at 24/192.
pub fn main(
    config: &Config,
    state_mutex: Arc<Mutex<PlayerState>>,
    decode_thread: &Thread,
    queue_events: SyncSender<QueueEvent>,
) {
    use std::time::{Instant, Duration};

    try_increase_thread_priority();

    loop {
        let has_audio = {
            let state = state_mutex.lock().unwrap();
            !state.is_queue_empty()
        };
        if has_audio {
            // We are resuming playback now from an idle state. Let the exec
            // thread execute the pre-playback program. For simplicity, the
            // exec thread runs even if no such program is configured.
            let is_running_condvar = Arc::new((Mutex::new(true), Condvar::new()));
            queue_events
                .send(QueueEvent::StartPlayback(is_running_condvar.clone()))
                .expect("Exec thread runs indefinitely, sending does not fail.");

            // If a pre-playback program is configured, we should wait for it to
            // finish, but only up to 10 seconds. The exec thread will set
            // is_running to false and wake us with the condvar.
            if config.exec_pre_playback_path.is_some() {
                let _ignored_guard = is_running_condvar.1.wait_timeout_while(
                    is_running_condvar.0.lock().unwrap(),
                    Duration::from_secs(10),
                    |&mut is_running| is_running,
                ).unwrap();
            }

            println!("Starting playback ...");
            play_queue(
                &config.audio_device,
                &config.audio_volume_control,
                &*state_mutex,
                decode_thread,
            );
            println!("Playback done, sleeping ...");

            // Signal the exec thread to start the idle timeout and execute the
            // post-idle program afterwards.
            queue_events
                .send(QueueEvent::EndPlayback(Instant::now()))
                .expect("Exec thread runs indefinitely, sending does not fail.");
        }
        thread::park();
    }
}
