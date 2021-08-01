// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Signal processing functions for high-pass filtering.

use std::f64;
use std::f32;

use crate::prim::Hertz;

/// A digital state variable filter.
///
/// Modelled after <https://www.earlevel.com/main/2003/03/02/the-digital-state-variable-filter/>.
#[derive(Clone)]
pub struct StateVariableFilter {
    /// The `f` parameter.
    f: f32,

    /// The `q` parameter.
    q: f32,

    /// Band-pass output, delayed by one tick.
    ///
    /// This is a state variable, `tick` reads from it.
    pub bandpass: f32,

    /// Low-pass output, delayed by two ticks.
    ///
    /// This is a state variable, `tick` reads from it.
    pub lowpass: f32,

    /// High-pass output, delayed by zero ticks.
    ///
    /// This is not a state variable, `tick` only writes it.
    pub highpass: f32,
}

impl StateVariableFilter {
    /// Initialize a new state variable filter.
    ///
    /// `q` normally ranges from 2, down to 0.0, where the filter oscillates.
    /// A value of `sqrt(2)` yields a flat pass-band response, higher values
    /// produce a softer “knee”, lower values introduce resonance.
    pub fn new(sample_rate: Hertz, cutoff: Hertz, q: f64) -> Self {
        let f = 2.0 * (f64::consts::PI * cutoff.0 as f64 / sample_rate.0 as f64).sin();

        Self {
            f: f as f32,
            q: q as f32,
            bandpass: 0.0,
            lowpass: 0.0,
            highpass: 0.0,
        }
    }

    /// Change the sample rate and/or cutoff frequency.
    pub fn set_cutoff(&mut self, sample_rate: Hertz, cutoff: Hertz) {
        self.f = (2.0 * (f64::consts::PI * cutoff.0 as f64 / sample_rate.0 as f64).sin()) as f32;
    }

    /// Set all state variables to 0.
    pub fn reset(&mut self) {
        self.lowpass = 0.0;
        self.highpass = 0.0;
        self.bandpass = 0.0;
    }

    /// Feed one sample into the filter.
    ///
    /// After this, the filtered signal is available in `self.lowpass` and
    /// `self.highpass`. The bit depth of the output is the same as that of the
    /// input; this filter works for any bit depth up to 24 bits per sample.
    ///
    /// Because peaks can move due to resampling, the output may exceed the
    /// input range slightly; the output may need to be scaled down slightly to
    /// avoid clipping.
    #[inline(always)]
    pub fn tick(&mut self, x0: f32) {
        // See the image at https://www.earlevel.com/main/2003/03/02/the-digital-state-variable-filter/.
        // TODO: Embed the diagram as ascii art, in case the page goes offline.
        let bandpass_f = self.bandpass * self.f;
        let lowpass = bandpass_f + self.lowpass;
        let bandpass_q = self.bandpass * self.q;
        let highpass = x0 - lowpass - bandpass_q;
        let highpass_f = highpass * self.f;
        let bandpass = highpass_f + self.bandpass;
        self.lowpass = lowpass;
        self.bandpass = bandpass;
        self.highpass = highpass;
    }

    /// Feed one sample, return the high-pass result, clipped if needed.
    ///
    /// This scales down the output by 6 dB and then clips, to prevent
    /// wrapping that might occasionally result from the filter producing higher
    /// peaks than were present in the original signal.
    #[inline]
    pub fn tick_highpass_clip(&mut self, x0: i32, bits_per_sample: u32) -> i32 {
        self.tick(x0 as f32);
        // A factor 0.5 in amplitude is about -6 dB in volume. We lose one bit
        // of precision because of this, but we need to, because the filter can
        // produce values that are out of range. (One way to see this: imagine
        // sampling a sine at an interval where the sample points are close to
        // the zero crossings of the sine ... the magnitudes of these samples
        // will be low. Now shift the sine by pi/2, so we sample the peaks.
        // Suddenly we need more range to represent the same wave!)
        let y0 = self.highpass * 0.5;

        // If the signal is still too large, clip it.
        let max = (1_i32 << bits_per_sample) - 1;
        let min = -max - 1;
        (y0 as i32).max(min).min(max)
    }
}
