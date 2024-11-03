// Musium -- Music playback daemon with web-based library browser
// Copyright 2021 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Signal processing functions for high-pass filtering.

use std::f32;
use std::f64;

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
        // Reproduced from https://www.earlevel.com/main/2003/03/02/the-digital-state-variable-filter/.
        //            ┌────────────────────────────────────────────┐
        //            │                                            │
        //            ├──► Highpass   ┌───► Bandpass             ┌─┴─┐
        //            │               │                          │ + ├───► Band reject
        //            │   f           │              f           └─┬─┘
        //      ┌───┐ │ ┌───┐   ┌───┐ │  ┌────┐    ┌───┐   ┌───┐   │
        // ────►│ + ├─┴─┤ × ├───┤ + ├─┴─►│z^-1├─┬──┤ × ├───┤ + ├──┬┴┬────► Lowpass
        //      └───┘   └───┘   └───┘    └────┘ │  └───┘   └───┘  │ │
        //      ▲   ▲             ▲             │           ▲     │ │
        //      │–  │–    q       └─────────────┤           │     │ │
        //      │   │   ┌───┐                   │         ┌─┴──┐  │ │
        //      │   └───┤ × ├───────────────────┘         │z^-1├──┘ │
        //      │       └───┘                             └────┘    │
        //      └───────────────────────────────────────────────────┘
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
    /// The expected range for the input is `i16::MIN / 2 .. i16::MAX / 2`. That
    /// is, the same range as i16, but scaled by half.
    ///
    /// A factor 0.5 in amplitude is about -6 dB in volume. We lose one bit
    /// of precision because of this, but we need to, because the filter can
    /// produce values that are out of range. (One way to see this: imagine
    /// sampling a sine at an interval where the sample points are close to
    /// the zero crossings of the sine ... the magnitudes of these samples
    /// will be low. Now shift the sine by pi/2, so we sample the peaks.
    /// Suddenly we need more range to represent the same wave!)
    /// We correct for bit depth before feeding into the filter, so that we
    /// can mix inputs from different bit depths and reuse the filter state.
    ///
    /// Returns the output as 16 bits per sample.
    #[inline(always)]
    pub fn tick_highpass_clip_i16(&mut self, x0: f32) -> i16 {
        debug_assert!(x0 * 2.0 >= i16::MIN as f32 - 1.0, "Out of range: {:.1} >= {}", x0 * 2.0, i16::MIN);
        debug_assert!(x0 * 2.0 <= i16::MAX as f32 + 1.0, "Out of range: {:.1} >= {}", x0 * 2.0, i16::MAX);
        self.tick(x0);

        // If the signal is still too large, clip it.
        self.highpass.clamp(i16::MIN as f32, i16::MAX as f32) as i16
    }
}

/// Holds high-pass filters, one for each channel.
pub struct Filters {
    /// One filter per channel.
    filters: [StateVariableFilter; 2],

    /// The current sample rate.
    sample_rate: Hertz,

    /// The cutoff frequency.
    cutoff: Hertz,
}

impl Filters {
    pub fn new(cutoff: Hertz) -> Self {
        // A q of sqrt(2) leads to the flattest possible pass-band.
        let q = 2.0_f64.sqrt();

        // The sample rate and cutoff will be adjusted later, these are just
        // dummy values for now.
        let sample_rate = Hertz(44_100);
        let filter = StateVariableFilter::new(
            sample_rate,
            cutoff,
            q,
        );
        Self {
            filters: [filter.clone(), filter],
            sample_rate,
            cutoff,
        }
    }

    /// Return the sample rate that this filter is currently configured for.
    pub fn get_sample_rate(&self) -> Hertz {
        self.sample_rate
    }

    /// Return the cutoff frequency that this filter is currently configured for.
    pub fn get_cutoff(&self) -> Hertz {
        self.cutoff
    }

    /// Update the filter parameters to work for a new format, if the sample rate changed.
    pub fn set_sample_rate(&mut self, sample_rate: Hertz) {
        for f in self.filters.iter_mut() {
            f.set_cutoff(sample_rate, self.cutoff);
        }
        self.sample_rate = sample_rate;
    }

    /// Update the filter parameters to adjust the cutoff frequency.
    pub fn set_cutoff(&mut self, cutoff: Hertz) {
        for f in self.filters.iter_mut() {
            f.set_cutoff(self.sample_rate, cutoff);
        }
        self.cutoff = cutoff;
    }

    /// Feed one 16-bit sample for both channels, return high-passed result in 16 bits per sample.
    #[inline]
    pub fn tick_i16(&mut self, left: i16, right: i16) -> (i16, i16) {
        (
            self.filters[0].tick_highpass_clip_i16((left as f32) * 0.5),
            self.filters[1].tick_highpass_clip_i16((right as f32) * 0.5),
        )
    }

    /// Feed one 24-bit sample for both channels, return high-passed result in 16 bits per sample.
    #[inline]
    pub fn tick_i24(&mut self, left: i32, right: i32) -> (i16, i16) {
        (
            // Here we divide by an additional 256 to correct for the additional
            // 8 bits of range. 0.5 / 256 is a (negative) power of 2, so this
            // merely adjusts the exponent of the float, we don't lose precision.
            self.filters[0].tick_highpass_clip_i16((left as f32) * (0.5 / 256.0)),
            self.filters[1].tick_highpass_clip_i16((right as f32) * (0.5 / 256.0)),
        )
    }
}
