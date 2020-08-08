// Mindec -- Music metadata indexer
// Copyright 2020 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

use std::result;
use std::i16;

use alsa;
use alsa::PollDescriptors;
use nix::errno::Errno;

type Result<T> = result::Result<T, alsa::Error>;

pub fn print_available_cards() -> Result<()> {
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

pub fn open_device(card_name: &str) -> Result<alsa::PCM> {
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

    // Pick the "front" output on the chosen device. This output should give us
    // stereo analog output and direct access to the hardware, according to
    // https://alsa-project.org/wiki/DeviceNames. It doesn't look like we can
    // choose between different outputs on the same card (e.g. headphones or
    // built-in speakers), but for now this will do. It is possible to prefix
    // the device name with "plug:" to get automatic sample rate conversion, but
    // I don't want sample rate conversion, I want a hard failure for
    // unsupported configurations.
    let device = format!("front:{}", card_index);
    let non_block = false;
    let pcm = match alsa::PCM::new(&device, alsa::Direction::Playback, non_block) {
        Ok(pcm) => pcm,
        Err(error) if error.errno() == Some(Errno::EBUSY) => {
            println!("Could not open audio interface for exclusive access, it is already use.");
            return Err(error);
        }
        err => return err,
    };

    let req_rate = 44_100;
    let req_channels = 2;
    let req_format = alsa::pcm::Format::s16();

    {
        let hwp = alsa::pcm::HwParams::any(&pcm)?;
        hwp.set_channels(req_channels)?;
        hwp.set_rate(req_rate, alsa::ValueOr::Nearest)?;
        hwp.set_format(req_format)?;
        println!("Set mmap interleaved");
        hwp.set_access(alsa::pcm::Access::MMapInterleaved)?;
        println!("After mmap interleaved");
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
    NeedMore,
    Yield,
    Done,
}

pub struct Block<T> {
    // TODO: Use boxed slice instead of vec.
    samples: Vec<T>,
    pos: usize,
}

impl<T> Block<T> {
    pub fn new(samples: Vec<T>) -> Block<T> {
        Block {
            samples: samples,
            pos: 0,
        }
    }
}

impl<T> Iterator for Block<T> where T: Copy {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.pos < self.samples.len() {
            self.pos += 1;
            Some(self.samples[self.pos - 1])
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.samples.len() - self.pos;
        (left, Some(left))
    }
}

impl<T> ExactSizeIterator for Block<T> where T: Copy {
    fn len(&self) -> usize {
        self.samples.len() - self.pos
    }
}

pub fn write_samples_i16(
    pcm: &alsa::PCM,
    mmap: &mut alsa::direct::pcm::MmapPlayback<i16>,
    blocks: &mut Vec<Block<i16>>,
) -> Result<WriteResult> {
    use alsa::pcm::State;

    while mmap.avail() > 0 {
        // TODO: Use a ring buffer instead of a vec.
        match blocks.pop() {
            Some(mut block) => {
                mmap.write(&mut block);

                // If we did not consume the entire block, put it back, so we can
                // consume the rest of it later.
                if block.len() > 0 {
                    blocks.push(block);
                }
            }
            None => {
                // Play what is still there, then stop.
                pcm.drain()?;
                break
            }
        }
    }

    match mmap.status().state() {
        State::Running => return Ok(WriteResult::Yield),
        State::Draining => return Ok(WriteResult::Done),
        State::Setup if blocks.len() == 0 => return Ok(WriteResult::Done),
        State::Prepared => pcm.start()?,
        State::XRun => pcm.prepare()?,
        State::Suspended => pcm.resume()?,
        unexpected => panic!("Unexpected PCM state: {:?}", unexpected),
    };
    Ok(WriteResult::NeedMore)
}

// TODO: Continue playback following https://github.com/diwic/alsa-rs/blob/master/synth-example/src/main.rs.

pub fn main(card_name: &str) {
    let mut blocks = Vec::new();

    for _ in 0..2 {
        let mut block = Vec::with_capacity(88200);
        for k in 0..44100 {
            let t = (k as f32) / 100.0;
            let a = (t * 3.141592 * 2.0).sin();
            let i = (a * (i16::MAX as f32) * 0.1) as i16;
            block.push(i); // L
            block.push(i); // R
        }
        blocks.push(Block::new(block));
    }

    let device = open_device(card_name).expect("TODO: Failed to open device.");
    let mut fds = device.get().expect("TODO: Failed to get fds from device.");

    let mut mmap = device.direct_mmap_playback::<i16>().expect("TODO: Failed to use mmap playback.");

    loop {
        match write_samples_i16(&device, &mut mmap, &mut blocks).expect("TODO: Failed to write samples.") {
            WriteResult::NeedMore => continue,
            WriteResult::Yield => {
                let max_sleep_ms = 100;
                alsa::poll::poll(&mut fds, max_sleep_ms).expect("TODO: Failed to wait for events.");
            }
            WriteResult::Done => break,
        }
    }
}
