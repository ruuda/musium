// Musium -- Music playback daemon with web-based library browser
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Logic for playing back audio using Alsa.

use std::ffi::CString;
use std::result;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::Thread;

use alsa;
use alsa::PollDescriptors;
use libc;

use crate::config::Config;
use crate::exec_pre_post::QueueEvent;
use crate::filter::Filters;
use crate::history::PlaybackEvent;
use crate::player::{Millibel, PlayerState, SampleDataSlice};
use crate::prim::Hertz;

const EBUSY: i32 = 16;

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

fn open_device(alsa_name: &str) -> Result<(alsa::PCM, alsa::Mixer)> {
    let non_block = false;
    let pcm = match alsa::PCM::new(alsa_name, alsa::Direction::Playback, non_block) {
        Ok(pcm) => pcm,
        Err(error) if error.errno() == EBUSY => {
            println!("Could not open audio interface for exclusive access, it is already use.");
            return Err(error);
        }
        Err(error) => return Err(error),
    };

    let non_block = false;
    let mixer = alsa::Mixer::new(alsa_name, non_block)?;

    Ok((pcm, mixer))
}

fn get_volume_control<'a>(mixer: &'a alsa::Mixer, name: &str) -> Option<alsa::mixer::Selem<'a>> {
    let mut selem_id = alsa::mixer::SelemId::empty();
    selem_id.set_name(&CString::new(name).expect("Invalid volume control name."));
    let selem = mixer.find_selem(&selem_id)?;
    assert!(selem.has_playback_volume());
    Some(selem)
}

/// Set the sample format to S16LE and the sample rate as given.
///
/// Returns the number of channels that the device needs us to fill.
fn set_format_get_channels(pcm: &alsa::PCM, sample_rate: Hertz) -> Result<u32> {
    let hwp = alsa::pcm::HwParams::any(pcm)?;

    // We want an even number of channels (stereo), and the lowest we can
    // get, so round up to the nearest multiple of 2. You'd think this is just
    // 2, but the Behringer UMC404HD has 4 channels as both the minimum and
    // maximum, so we have to fill all of them.
    let n_channels = match hwp.get_channels_min()? {
        n if n & 1 == 0 => n,
        n => n + 1,
    };
    hwp.set_channels(n_channels)?;

    hwp.set_rate(sample_rate.0, alsa::ValueOr::Nearest)?;

    // We always output 16 bits per sample, regardless of the input bit depth,
    // which is usually 16 bits but can be 24 occasionally. The Behringer
    // UMC404HD that I use supports either 16 or 32 bits per sample in the
    // hardware itself, so let's just truncate to 16 bits and simplify the
    // output mode; for normal listening, what Musium is intended for, you
    // will not hear the difference.
    hwp.set_format(alsa::pcm::Format::S16LE)?;
    hwp.set_access(alsa::pcm::Access::MMapInterleaved)?;

    // Set the buffer and period size, in frames. The period is how often
    // the hardware will interrupt; it is how often poll returns. The Alsa
    // wiki says:
    // > The buffer size always has to be greater than one period size.
    // > Commonly this is 2*period size, but some hardware can do 8 periods
    // > per buffer.
    // 2048 frames at 44.1 kHz is 46ms, so that is how responsive we can be
    // to stopping the music.
    hwp.set_period_size_near(512, alsa::ValueOr::Nearest)?;
    hwp.set_buffer_size_near(2048)?;

    pcm.hw_params(&hwp)?;
    drop(hwp);

    let hwp = pcm.hw_params_current()?;
    let swp = pcm.sw_params_current()?;
    let buffer_len = hwp.get_buffer_size()?;
    let period_len = hwp.get_period_size()?;
    swp.set_start_threshold(buffer_len - period_len)?;
    swp.set_avail_min(period_len)?;
    pcm.sw_params(&swp)?;

    // Double-check that we got the expected parameters.
    assert_eq!(hwp.get_channels()?, n_channels);
    assert_eq!(hwp.get_rate()?, sample_rate.0);
    assert_eq!(hwp.get_format()?, alsa::pcm::Format::S16LE);

    Ok(n_channels)
}

enum WriteResult {
    /// We performed a state transition, but did not write; try again.
    Continue,

    /// We need to change the sample rate before we can continue playback.
    ChangeSampleRate(Hertz),

    /// The queue is empty, playback is done for now.
    QueueEmpty,

    /// Done for now, check back later.
    ///
    /// This can happen either when the playback buffer is full, or when the
    /// decode buffer is empty. After a yield we release the state lock, so if
    /// the decode thread has new blocks for the decode buffer, we will pick
    /// that up. And then we poll the file descriptor, until it is ready to
    /// write to again.
    Yield,
}

fn write_samples(
    pcm: &alsa::PCM,
    filters: &mut Filters,
    n_channels: usize,
    io: &mut alsa::pcm::IO<u8>,
    player: &mut PlayerState,
) -> Result<WriteResult> {
    use alsa::pcm::State;

    let mut next_rate = None;
    let mut n_consumed = 0;

    // Query how many frames are available for writing. If the device is in a
    // failed state, for example because of an underrun, then this fails, and
    // we need to recover. Recover once, if that does not help, propagate the
    // error.
    let n_available = pcm.avail_update().unwrap_or_else(|err| {
        println!("DEBUG: write_samples was in error, the error was {:?}.", err);
        // Previously we used try_recover here, but all it does is call
        // prepare, which we would do anyway below. See [1].
        // [1]: https://git.alsa-project.org/?p=alsa-lib.git;a=blob;f=src/pcm/pcm.c;
        // h=bc18954b92da124bafd3a67913bd3c8900dd012f;hb=HEAD#l7864
        0
    }) as usize;

    if n_available > 0 {
        n_consumed = match player.peek_mut() {
            Some(ref block) if filters.get_sample_rate() != block.sample_rate() => {
                // Next block has a different sample rate, finish what is still
                // in the buffer, so we can switch afterwards.
                pcm.drain()?;
                next_rate = Some(block.sample_rate());
                0
            }
            Some(block) => {
                io.mmap(n_available, |dst| {
                    // The destination buffer is in bytes, we have 2 * n_channels
                    // bytes per frame (a left and right sample, both 16 bits).
                    let n = (dst.len() / (2 * n_channels)).min(block.len());

                    // Write left and right samples into the output buffer at frame i.
                    let mut put_i = |i: usize, l: i16, r: i16| {
                        // Duplicate the left and right channels to as
                        // many channels as we need. You'd think that is
                        // only 2, but on the UMC404HD, it's 4 channels.
                        for j in 0..n_channels / 2 {
                            let lb = l.to_le_bytes();
                            let rb = r.to_le_bytes();
                            unsafe {
                                *dst.get_unchecked_mut(i * n_channels * 2 + 4 * j + 0) = lb[0];
                                *dst.get_unchecked_mut(i * n_channels * 2 + 4 * j + 1) = lb[1];
                                *dst.get_unchecked_mut(i * n_channels * 2 + 4 * j + 2) = rb[0];
                                *dst.get_unchecked_mut(i * n_channels * 2 + 4 * j + 3) = rb[1];
                            }
                        }
                    };
                    match block.slice() {
                        SampleDataSlice::I16(src) => {
                            for (i, s) in src.iter().take(n).enumerate() {
                                let (l, r) = filters.tick_i16(s.0, s.1);
                                put_i(i, l, r);
                            }
                        }
                        SampleDataSlice::I24(src) => {
                            for (i, s) in src.iter().take(n).enumerate() {
                                let s_i32 = s.as_channels();
                                let (l, r) = filters.tick_i24(s_i32.0, s_i32.1);
                                put_i(i, l, r);
                            }
                        }
                    }

                    // We need to return the number of frames written.
                    n

                // TODO: This can apparently cause Error("snd_pcm_mmap_commit", Sys(EPIPE).
                // How to handle it?
                })?
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
        // We are playing, we tried to replenish the buffer, but there are no
        // decoded samples available, or there was nothing to fill. In the first
        // case we need to release the state lock and hope that the decoder has
        // caught up in the next iteration, in the second case we just need to
        // wait a bit for the audio device to make room in the buffer, so either
        // way, we yield.
        State::Running if n_consumed == 0 => Ok(WriteResult::Yield),

        // We are playing. We replenished the buffer a bit, but not fully. But
        // we did make progress. Run another iteration immediately, there might
        // be more decoded samples available in a next block.
        State::Running if n_consumed < n_available => Ok(WriteResult::Continue),

        // We are playing, and we replenished the buffer fully (since n_consumed
        // >= n_available because of the above pattern, and n_consumed <=
        // n_available because we cannot write more samples than there is space
        // available). Sleep until there is space in the buffer again.
        State::Running => Ok(WriteResult::Yield),

        State::Draining => match next_rate {
            Some(_) => Ok(WriteResult::Yield),
            None if player.is_queue_empty() => Ok(WriteResult::QueueEmpty),
            None => panic!("PCM is unexpectedly in draining state."),
        }
        State::Setup => match next_rate {
            Some(rate) => Ok(WriteResult::ChangeSampleRate(rate)),
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
        // If the PCM is ready for playback, but we are not in one of the above
        // two cases, then we could fill the buffer a bit more before we start,
        // which is a good idea to reduce the risk of buffer underrun.
        State::Prepared if n_consumed > 0 && n_consumed < n_available => {
            // If we wrote anything, we can try again immediately, there might
            // be more decoded samples in a next block.
            Ok(WriteResult::Continue)
        }
        State::Prepared => {
            // But if we had no samples, yield, so the decoder can take the
            // state lock.
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
        State::Open => {
            // The device is in "Open" state if we haven't configured the sample
            // format yet. We only know the format after there is a block, but
            // when decode is in progress, the block is not yet there, so we yield.
            match player.peek_mut() {
                Some(block) => Ok(WriteResult::ChangeSampleRate(block.sample_rate())),
                None => Ok(WriteResult::Yield),
            }
        }
        unexpected => panic!("Unexpected PCM state: {:?}", unexpected),
    }
}

enum FillResult {
    /// We need to change the sample rate before we can continue playback.
    ChangeSampleRate(Hertz),

    /// The queue is empty, playback is done for now.
    QueueEmpty,

    /// Buffers are full for now, but we should check back later.
    Yield,
}

fn ensure_buffers_full(
    device: &alsa::PCM,
    filters: &mut Filters,
    n_channels: usize,
    io: &mut alsa::pcm::IO<u8>,
    player: &mut PlayerState,
) -> FillResult {
    loop {
        match write_samples(device, filters, n_channels, io, player) {
            Err(err) => {
                println!("Error while writing samples: {:?}", err);
                println!("Resuming ...");
                continue
            }
            Ok(WriteResult::Continue) => continue,
            Ok(WriteResult::ChangeSampleRate(rate)) => return FillResult::ChangeSampleRate(rate),
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
    alsa_name: &str,
    volume_name: &str,
    state_mutex: &Mutex<PlayerState>,
    decode_thread: &Thread,
) {
    let (mut device, mut mixer) = open_device(alsa_name).expect("TODO: Failed to open device.");
    let mut vc = get_volume_control(&mixer, volume_name).expect("TODO: Failed to get volume control.");
    let mut fds = device.get().expect("TODO: Failed to get fds from device.");

    let mut volume = None;
    let mut n_channels = 0;

    // Set a sentinel value at the start, so we are guaranteed that the first
    // thing we do is change the format.
    let cutoff = Hertz(0);
    let mut filters = Filters::new(cutoff);
    let mut next_rate = None;

    loop {
        // If the sample format changed, then we re-open the device. Up to Linux
        // 5.10.94, it was possible to change the format on an existing device,
        // but later versions of Linux have a regression where if you call
        // `snd_pcm_hw_params` a second time with a different sample rate, it
        // always returns error code 22 (invalid argument). We work around this
        // by closing and re-opening the device.
        if let Some(rate) = next_rate.take() {
            drop(fds);
            drop(device);

            (device, mixer) = open_device(alsa_name).expect("TODO: Failed to open device.");
            vc = get_volume_control(&mixer, volume_name).expect("TODO: Failed to get volume control.");
            fds = device.get().expect("TODO: Failed to get fds from device.");

            n_channels = match set_format_get_channels(&device, rate) {
                Ok(n) => {
                    println!("Set format: bits_per_sample=16, rate={rate}, channels={n} pcm={alsa_name}");
                    n
                },
                Err(err) => panic!(
                    "Failed to set format for device {alsa_name} to sample rate {rate}: {err:?}",
                ),
            };

            filters.set_sample_rate(rate);
        }

        // There is also "direct mode" that works with mmaps, but it is not
        // supported by the kernel on ARM, and I want to run this on a Raspberry Pi,
        // so for simplicity I will use the mode that is supported everywhere.
        let mut io = device.io_bytes();

        let (result, target_volume, needs_decode) = {
            let mut state = state_mutex.lock().unwrap();
            let result = ensure_buffers_full(
                &device,
                &mut filters,
                n_channels as usize,
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
                match vc.set_playback_db_all(alsa::mixer::MilliBel(v as i64), alsa::Round::Floor) {
                    Ok(()) => println!("Set volume: {:.1} dB", v as f32 * 0.01),
                    // Log when setting the volume fails, there is little more
                    // we can do aside from crashing the entire application.
                    Err(err) => println!("Failed to set volume to {:.1} dB: {err:?}", v as f32 * 0.01),
                }
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
            FillResult::ChangeSampleRate(new_rate) => {
                next_rate = Some(new_rate);
                continue;
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
    history_events: SyncSender<PlaybackEvent>,
) {
    use std::time::{Duration, Instant};

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

            play_queue(
                &config.audio_device,
                &config.audio_volume_control,
                &state_mutex,
                decode_thread,
            );

            // Inform the history thread that the queue ended, so it can
            // checkpoint the WAL.
            history_events
                .send(PlaybackEvent::QueueEnded)
                .expect("History thread runs indefinitely, sending does not fail.");

            // Signal the exec thread to start the idle timeout and execute the
            // post-idle program afterwards.
            queue_events
                .send(QueueEvent::EndPlayback(Instant::now()))
                .expect("Exec thread runs indefinitely, sending does not fail.");
        }
        thread::park();
    }
}
