//! The LFO: a wave of its own, shared by the shape (Free, Restart on hit) or one per note.

use super::{BLOCK_S, ModCx, Modulator, NoteCx};
use crate::{BLOCK, SAMPLE_RATE, TempoMap};
use bpe_dsp::lfo::{LfoWave, fade_at, smooth_coefficient, wave_at, wave_end};
use bpe_kit::sound::{LfoSettings, LfoShape, LfoStart};
use std::sync::{Arc, OnceLock};

/// The wave an LFO shape plays.
pub fn wave_of(shape: LfoShape) -> LfoWave {
    match shape {
        LfoShape::Drawn => LfoWave::Curve,
        LfoShape::Random => LfoWave::Random,
        LfoShape::SmoothRandom => LfoWave::SmoothRandom,
    }
}

/// A drawn shape's points as the engine reads them, shared without copying.
pub type Curve = Arc<[[f32; 3]]>;

/// No points (shared, so making one never allocates after the first).
pub fn no_curve() -> Curve {
    static EMPTY: OnceLock<Curve> = OnceLock::new();
    EMPTY.get_or_init(|| Arc::from(Vec::new())).clone()
}

/// The drawn shape of `s` (allocates: build it with the patch).
pub fn curve_of(s: &LfoSettings) -> Curve {
    Arc::from(bpe_kit::sound::curve_numbers(&s.curve))
}

/// The LFO's rate in Hz: the Rate knob as it is now (`rate_hz`, keys and routes applied), or,
/// synced, its note length at the scene's tempo (a quarter note is a beat at the tempo, a bar
/// is the time signature's).
pub fn lfo_rate_hz(s: &LfoSettings, rate_hz: f64, tempo: &TempoMap) -> f64 {
    if !s.sync {
        return rate_hz.max(0.0);
    }
    let seconds = if s.rate_note.is_bars() {
        s.rate_note.wholes() * tempo.bar_seconds()
    } else {
        s.rate_note.wholes() * 4.0 * 60.0 / tempo.bpm()
    } * s.rate_feel.factor();
    if seconds > 0.0 { 1.0 / seconds } else { 0.0 }
}

/// An LFO's settings for one block.
#[derive(Clone, Debug, PartialEq)]
pub struct LfoParams {
    pub wave: LfoWave,
    /// The drawn shape (for [`LfoWave::Curve`]).
    pub curve: Curve,
    /// Plays one cycle and holds its end.
    pub once: bool,
    pub rate_hz: f64,
    /// Where it starts, in cycles (0..1).
    pub phase: f64,
    pub fade_in: f64,
    pub delay: f64,
    /// 0..1: how much its corners are rounded.
    pub smooth: f32,
    pub start: LfoStart,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            wave: LfoWave::Sine,
            curve: no_curve(),
            once: false,
            rate_hz: 2.0,
            phase: 0.0,
            fade_in: 0.0,
            delay: 0.0,
            smooth: 0.0,
            start: LfoStart::Free,
        }
    }
}

impl LfoParams {
    /// The params of `s` with the rate (in Hz, already worked out) and the knobs as given
    /// (allocates the curve: for tests and building).
    pub fn of(s: &LfoSettings, rate_hz: f64) -> Self {
        LfoParams {
            wave: wave_of(s.shape),
            curve: curve_of(s),
            once: s.once,
            rate_hz,
            phase: s.phase as f64 / 360.0,
            fade_in: s.fade_in as f64,
            delay: s.delay as f64,
            smooth: (s.smooth / 100.0).clamp(0.0, 1.0),
            start: s.start,
        }
    }
}

/// The value an LFO with `p` has `since` seconds after it started, at `cycles` cycles in
/// (not counting Phase): the same sum the engine and Follow sound both use. Played once, it
/// holds where the cycle ends.
pub fn lfo_value(p: &LfoParams, cycles: f64, since: f64, seed: u64) -> f32 {
    let v = if p.once && cycles >= 1.0 {
        wave_end(p.wave, &p.curve, p.phase, seed)
    } else {
        wave_at(p.wave, &p.curve, cycles + p.phase, seed)
    };
    v * fade_at(since, p.delay, p.fade_in)
}

/// Where in its shape (0..1 across one cycle) an LFO with `p` is at `cycles` cycles in, for the
/// editor's playhead.
pub fn lfo_position(p: &LfoParams, cycles: f64) -> f32 {
    let at = if p.once && cycles >= 1.0 { p.phase + 1.0 - 1e-9 } else { cycles + p.phase };
    if at.is_finite() { (at - at.floor()) as f32 } else { 0.0 }
}

/// The random seed of LFO `layer` on shape `shape`.
pub fn seed_of(shape: bpe_core::ThingId, layer: bpe_core::LayerId) -> u64 {
    bpe_dsp::lfo::hash(((shape.0 as u64) << 32) ^ layer.0 as u64 ^ 0x4C46_4F00)
}

/// One LFO.
#[derive(Clone, Debug)]
pub struct LfoMod {
    p: LfoParams,
    seed: u64,
    /// Cycles since it started (Phase not counted).
    cycles: f64,
    /// The sample it started on (0 for Free).
    started: u64,
    /// A hit (or note) restarted it at this sample, this block.
    restart: Option<u64>,
    /// Starting over: the phase is worked out from where the block is.
    fresh: bool,
    value: f32,
}

impl LfoMod {
    pub fn new(seed: u64) -> Self {
        LfoMod { p: LfoParams::default(), seed, cycles: 0.0, started: 0, restart: None, fresh: true, value: 0.0 }
    }

    /// The settings for the next block (rate in Hz as the knob is now, or `rate_hz` when the
    /// rate is in Hz; synced rates come from `tempo`).
    pub fn set(&mut self, s: &LfoSettings, tempo: &TempoMap, rate_hz: f64) {
        self.p = LfoParams::of(s, lfo_rate_hz(s, rate_hz, tempo));
    }

    /// A new random seed (a per-note LFO takes one from its note).
    pub fn reseed(&mut self, seed: u64) {
        self.seed = seed;
    }

    pub fn set_params(&mut self, p: LfoParams) {
        self.p = p;
    }

    pub fn params(&self) -> &LfoParams {
        &self.p
    }

    /// Where in its shape it is now (0..1 across a cycle), for the editor's playhead.
    pub fn position(&self) -> f32 {
        lfo_position(&self.p, self.cycles)
    }

    /// Takes over another's place in its wave (a patch swap), so it carries on smoothly.
    pub fn take_over(&mut self, old: &LfoMod) {
        self.cycles = old.cycles;
        self.started = old.started;
        self.restart = old.restart;
        self.fresh = old.fresh;
        self.value = old.value;
    }
}

impl Modulator for LfoMod {
    fn value(&self) -> f32 {
        self.value
    }

    fn next_block(&mut self, cx: &ModCx) {
        let end = cx.start_sample + BLOCK as u64;
        let rate = self.p.rate_hz.max(0.0);
        let fresh = std::mem::take(&mut self.fresh);
        if let Some(at) = self.restart.take() {
            self.started = at.min(end);
            self.cycles = rate * (end - self.started) as f64 / SAMPLE_RATE as f64;
        } else if fresh {
            // Free runs from the start of the clip: work the phase out from the time, so a seek
            // lands where playing through would have.
            self.started = 0;
            self.cycles = rate * end as f64 / SAMPLE_RATE as f64;
        } else {
            self.cycles += rate * BLOCK_S;
        }
        let since = (end - self.started.min(end)) as f64 / SAMPLE_RATE as f64;
        let raw = lfo_value(&self.p, self.cycles, since, self.seed);
        if fresh || self.p.smooth <= 0.0 || rate <= 0.0 {
            self.value = raw;
        } else {
            // At 100 % it takes half a cycle to follow a jump.
            let c = smooth_coefficient(self.p.smooth as f64 * 0.5 / rate, BLOCK_S);
            self.value += (raw - self.value) * c;
        }
    }

    fn note_on(&mut self, cx: &NoteCx) {
        if self.p.start != LfoStart::Free {
            self.restart = Some(cx.sample);
        }
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.restart = None;
        self.value = 0.0;
    }
}
