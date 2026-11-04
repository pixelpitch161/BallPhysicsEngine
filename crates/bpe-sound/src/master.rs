//! The master: input gain, the true-peak limiter, output gain, every knob smoothed.

use crate::BLOCK;
use bpe_dsp::{Smoother, TruePeakLimiter, db_to_gain};
use bpe_kit::sound::MixerSettings;

pub struct Master {
    input: Smoother,
    output: Smoother,
    limiter: TruePeakLimiter,
    ceiling: f32,
    release: f32,
}

impl Master {
    pub fn new(settings: &MixerSettings) -> Self {
        let mut m = Self {
            input: Smoother::new(Smoother::KNOB_MS),
            output: Smoother::new(Smoother::KNOB_MS),
            limiter: TruePeakLimiter::new(),
            ceiling: f32::NAN,
            release: f32::NAN,
        };
        m.set(settings);
        m.input.reset(db_to_gain(settings.master_input));
        m.output.reset(db_to_gain(settings.master_output));
        m
    }

    /// New settings, glided to.
    pub fn set(&mut self, s: &MixerSettings) {
        self.input.set_target(db_to_gain(s.master_input));
        self.output.set_target(db_to_gain(s.master_output));
        if s.master_ceiling != self.ceiling || s.master_release != self.release {
            self.ceiling = s.master_ceiling;
            self.release = s.master_release;
            self.limiter.set(s.master_ceiling, s.master_release);
        }
    }

    /// Silence, as new, with the gains where they are headed.
    pub fn reset(&mut self) {
        self.limiter.reset();
        let (i, o) = (self.input.target(), self.output.target());
        self.input.reset(i);
        self.output.reset(o);
    }

    /// Frames the master delays the sound by.
    pub fn latency(&self) -> usize {
        self.limiter.latency()
    }

    /// The master's gain reduction in the last block, in dB (0 or less).
    pub fn gain_reduction_db(&self) -> f32 {
        self.limiter.gain_reduction_db()
    }

    /// Runs one block in place.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let g = self.input.next();
            *a *= g;
            *b *= g;
        }
        self.limiter.process(l, r);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let g = self.output.next();
            *a *= g;
            *b *= g;
        }
    }
}
