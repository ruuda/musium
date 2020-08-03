// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::result;

use alsa;

type Result<T> = result::Result<T, alsa::Error>;

pub fn open_device() -> Result<alsa::PCM> {
    let device = "default";
    let non_block = false;
    let pcm = alsa::PCM::new(device, alsa::Direction::Playback, non_block)?;

    let req_rate = 44_100;
    let req_channels = 2;
    let req_format = alsa::pcm::Format::s16();

    {
        let hwp = alsa::pcm::HwParams::any(&pcm)?;
        hwp.set_channels(req_channels)?;
        hwp.set_rate(req_rate, alsa::ValueOr::Nearest)?;
        hwp.set_format(req_format)?;
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

        let actual_rate = hwp.get_rate()?;
        let actual_channels = hwp.get_channels()?;
        let actual_format = hwp.get_format()?;

        // TODO: Raise a nice error when the format is not supported.
        assert_eq!(actual_rate, req_rate);
        assert_eq!(actual_channels, req_channels);
        assert_eq!(actual_format, req_format);
    }

    Ok(pcm)
}

pub enum WriteResult {
    Done,
    NeedMore,
}

pub fn write_samples_i16(
    pcm: &alsa::PCM,
    mmap: &mut alsa::direct::pcm::MmapPlayback<i16>,
    samples: &[u32],
) -> Result<WriteResult> {
    use alsa::pcm::State;
    // TODO: Confirm that all samples have been written, or report back how much
    // was consumed.
    if mmap.avail() > 0 {
        mmap.write(&mut samples.iter().map(|s| (s >> 16) as i16));
    }
    match mmap.status().state() {
        State::Running => return Ok(WriteResult::Done),
        State::Prepared => pcm.start()?,
        State::XRun => pcm.prepare()?,
        State::Suspended => pcm.resume()?,
        unexpected => panic!("Unexpected PCM state: {:?}", unexpected),
    };
    Ok(WriteResult::NeedMore)
}

// TODO: Continue playback following https://github.com/diwic/alsa-rs/blob/master/synth-example/src/main.rs.
