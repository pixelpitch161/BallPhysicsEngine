//! The master: input gain, the true-peak limiter, output gain, every knob smoothed. Input and
//! Output can be keyed on the timeline; their keys are read every block.

use crate::BLOCK;
use crate::voices::knobs::track_value;
use bpe_core::Track;
use bpe_dsp::{Smoother, TruePeakLimiter, db_to_gain};
use bpe_kit::sound::MixerSettings;
use bpe_runtime::{ChangeKey, RulesState};

/// The keys on the Master's Input and Output, if any.
#[derive(Clone, Debug, Default)]
pub struct MasterKeys {
    pub input: Option<Track>,
    pub output: Option<Track>,
}

pub struct Master {
    input: Smoother,
    output: Smoother,
    limiter: TruePeakLimiter,
    ceiling: f32,
    release: f32,
    /// Input and Output as set (dB), for when they have no keys.
    base: (f32, f32),
    keys: MasterKeys,
    /// Where a rule's change of Input and Output is kept (W13).
    ruled: [ChangeKey; 2],
}

impl Master {
    pub fn new(settings: &MixerSettings) -> Self {
        let mut m = Self {
            input: Smoother::new(Smoother::KNOB_MS),
            output: Smoother::new(Smoother::KNOB_MS),
            limiter: TruePeakLimiter::new(),
            ceiling: f32::NAN,
            release: f32::NAN,
            base: (settings.master_input, settings.master_output),
            keys: MasterKeys::default(),
            ruled: [crate::ruled::mixer_key("master_input"), crate::ruled::mixer_key("master_output")],
        };
        m.set(settings);
        m.input.reset(db_to_gain(settings.master_input));
        m.output.reset(db_to_gain(settings.master_output));
        m
    }

    /// New settings, glided to.
    pub fn set(&mut self, s: &MixerSettings) {
        self.base = (s.master_input, s.master_output);
        self.input.set_target(db_to_gain(s.master_input));
        self.output.set_target(db_to_gain(s.master_output));
        if s.master_ceiling != self.ceiling || s.master_release != self.release {
            self.ceiling = s.master_ceiling;
            self.release = s.master_release;
            self.limiter.set(s.master_ceiling, s.master_release);
        }
    }

    /// New keys for Input and Output, read from the next [`Master::at`].
    pub fn set_keys(&mut self, keys: MasterKeys) {
        self.keys = keys;
    }

    /// Heads Input and Output for where their keys and the rules' changes (`rules`, W13) are at
    /// `time` (seconds). Never allocates.
    pub fn at(&mut self, time: f64, rules: Option<&RulesState>) {
        let input = self.keys.input.as_ref().and_then(|t| track_value(t, time)).unwrap_or(self.base.0);
        let output = self.keys.output.as_ref().and_then(|t| track_value(t, time)).unwrap_or(self.base.1);
        let input = crate::ruled::at(rules, &self.ruled[0], input, time);
        let output = crate::ruled::at(rules, &self.ruled[1], output, time);
        self.input.set_target(db_to_gain(input));
        self.output.set_target(db_to_gain(output));
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

    /// Runs one block in place: Input, the limiter, Output, with the ceiling last. Output's cut
    /// comes after the limiter (it lowers the finished sound); its boost goes in before it, so a
    /// louder Output pushes into the limiter and never past the ceiling. (A peak limiter scales:
    /// boosting then limiting at the ceiling is limiting lower then boosting.)
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK]) {
        let mut cut = [1.0f32; BLOCK];
        for ((a, b), c) in l.iter_mut().zip(r.iter_mut()).zip(cut.iter_mut()) {
            let o = self.output.next();
            let g = self.input.next() * o.max(1.0);
            *c = o.min(1.0);
            *a *= g;
            *b *= g;
        }
        self.limiter.process(l, r);
        for ((a, b), c) in l.iter_mut().zip(r.iter_mut()).zip(cut) {
            *a *= c;
            *b *= c;
        }
    }
}
