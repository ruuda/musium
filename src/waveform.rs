// Musium -- Music playback daemon with web-based library browser
// Copyright 2022 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Track visualisation, as a “waveform”.
//!
//! Actually we visualize loudness.

use std::io;

use bs1770::{ChannelLoudnessMeter};

/// A “waveform” of a track.
///
/// The amplitudes are the square roots of the power over a 0.5s window of
/// audio. They are spaced at 0.2s distance (therefore windows overlap).
/// Amplitudes are stored in one byte per value. This is more precision than we
/// really need (4 bits is too few, 6 is sufficient), but it makes processing
/// easier, so for now we spend the extra space.
///
/// A window of 0.5s provides a good trade off between graphs that are too
/// spiky to see the track’s structure at a glance, and graphs that are too
/// smeared out to have any detail, and are therefore less interesting.
///
/// Sampling at 5 Hz (0.2s apart) seems to be sufficient; visually no
/// significant detail is lost compared to sampling at 10 Hz.
///
/// The buffer stores all amplitudes for the left channel first, then all
/// amplitudes for the right channel. This means that the length of the buffer
/// must be even.
pub struct Waveform {
    pub amplitudes: Vec<u8>,
}

impl Waveform {
    /// Construct a waveform from a left and right channel loudness meter.
    pub fn from_meters(meters: &[ChannelLoudnessMeter; 2]) -> Waveform {
        // We will need slightly fewer due to windowing, but this is is a good
        // size to allocate. We need only half the values because we sample at
        // 200ms, but the source is at 100ms. But we need double that because we
        // have two channels.
        let values_per_channel = meters[0].as_100ms_windows().len() / 2;
        let mut powers = Vec::with_capacity(values_per_channel * 2);
        let mut max_power = 0.0;

        for channel_meter in meters.iter() {
            // Step by 2, so we sample every 200ms; the source is at 100ms.
            for window_500ms in channel_meter.as_100ms_windows().inner.windows(5).step_by(2) {
                let power = 0.2 * window_500ms.iter().map(|p| p.0).sum::<f32>();
                if power > max_power {
                    max_power = power;
                }
                powers.push(power);
            }
        }

        let amplitudes = powers
            .iter()
            .map(|&p| (255.0 * (p / max_power + 1e-10).sqrt()) as u8)
            .collect();

        Waveform {
            amplitudes
        }
    }

    /// Load the waveform from a buffer, for loading from the database.
    pub fn from_bytes(data: Vec<u8>) -> Waveform {
        assert_eq!(data.len() % 2, 0, "Buffer length must be even.");
        Waveform {
            amplitudes: data,
        }
    }

    /// View the waveform as a buffer, to be saved in the database.
    pub fn as_bytes(&self) -> &[u8] {
        self.amplitudes.as_ref()
    }

    /// Render the waveform to svg.
    pub fn write_svg<W: io::Write>(&self, out: &mut W) -> io::Result<()> {
        let num_samples = self.amplitudes.len() / 2;
        assert_eq!(num_samples * 2, self.amplitudes.len(), "Buffer length must be even.");

        writeln!(
            out,
            r#"<svg width="{:.1}" height="510" xmlns="http://www.w3.org/2000/svg">"#,
            // For a pleasing aspect ratio, we space every point on the x-axis
            // (which are 200ms apart) 10 units, for ~500 units on the y axis.
            num_samples * 10,
        )?;
        writeln!(out, r#"<path d="M 0 255 "#)?;

        // Pass 1, left channel, from left to right. The y-axis goes down from
        // 0 to 510, with 255 in the middle.
        for (i, &amplitude) in self.amplitudes[..num_samples].iter().enumerate() {
            write!(out, "L {} {} ", i * 10, 255_i32 - amplitude as i32)?;
        }

        // Past 2, right channel, from right to left.
        for (i, &amplitude) in self.amplitudes[num_samples..].iter().enumerate().rev() {
            write!(out, "L {} {} ", i * 10, 255_i32 + amplitude as i32)?;
        }

        writeln!(out, r#"" fill="black"/></svg>"#)
    }
}
