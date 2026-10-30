//! The oscillator: follows the note's pitch, with band-limited waves, unison, FM and hard sync.

use super::tables::{self, Shape};
use crate::{BLOCK, Block, Buf, Ctx, Knobs, Rng, midi_to_hz, wired};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wave {
    Sine,
    Triangle,
    Saw,
    Square,
    Pulse,
}

pub struct Osc {
    wave: Wave,
    width: f32,
    /// Semitones plus cents.
    offset: f32,
    unison: usize,
    fm: f32,
    free: bool,
    phases: [f32; 8],
    /// Each unison voice's pitch offset in semitones and its left and right gains.
    voices: [(f32, f32, f32); 8],
    /// Each unison voice's pitch as a ratio of the note's.
    ratios: [f32; 8],
    last_sync: f32,
}

impl Osc {
    pub fn new(k: &Knobs) -> Self {
        let wave = match k.choice("wave", "saw") {
            "sine" => Wave::Sine,
            "triangle" => Wave::Triangle,
            "square" => Wave::Square,
            "pulse" => Wave::Pulse,
            _ => Wave::Saw,
        };
        let unison = (k.number("unison", 1.0).round() as usize).clamp(1, 8);
        let detune = k.number("detune", 20.0).clamp(0.0, 100.0);
        let spread = k.number("spread", 0.5).clamp(0.0, 1.0);
        let norm = 1.0 / (unison as f32).sqrt();
        let mut voices = [(0.0, norm, norm); 8];
        if unison > 1 {
            for (i, v) in voices.iter_mut().take(unison).enumerate() {
                let x = i as f32 / (unison - 1) as f32 * 2.0 - 1.0;
                let angle = (x * spread + 1.0) * std::f32::consts::FRAC_PI_4;
                let s2 = std::f32::consts::SQRT_2;
                *v = (x * detune / 100.0, angle.cos() * s2 * norm, angle.sin() * s2 * norm);
            }
        }
        Self {
            wave,
            width: k.number("width", 0.5).clamp(0.01, 0.99),
            offset: k.number("semitones", 0.0).clamp(-48.0, 48.0) + k.number("cents", 0.0).clamp(-100.0, 100.0) / 100.0,
            unison,
            fm: k.number("fm", 0.0).clamp(0.0, 10.0),
            free: k.choice("phase", "note") == "free",
            phases: [0.0; 8],
            voices,
            ratios: voices.map(|(semis, ..)| (semis / 12.0).exp2()),
            last_sync: 0.0,
        }
    }

    /// Each unison voice's phase step and table for a note stepping `base` cycles per frame.
    #[inline]
    fn fill(&self, base: f32, nyquist: f32, incs: &mut [f32; 8], tabs: &mut [&'static [f32]; 8]) {
        for v in 0..self.unison {
            incs[v] = (base * self.ratios[v]).min(nyquist);
            tabs[v] = self.table(incs[v]);
        }
    }

    /// The wave table for a phase step of `inc`; a sine needs none.
    #[inline]
    fn table(&self, inc: f32) -> &'static [f32] {
        match self.wave {
            Wave::Sine => &[],
            Wave::Triangle => tables::table(Shape::Triangle, inc),
            Wave::Square => tables::table(Shape::Square, inc),
            Wave::Saw | Wave::Pulse => tables::table(Shape::Saw, inc),
        }
    }

    #[inline]
    fn shape(&self, table: &[f32], phase: f32) -> f32 {
        match self.wave {
            Wave::Sine => (phase * std::f32::consts::TAU).sin(),
            Wave::Triangle | Wave::Saw | Wave::Square => tables::read(table, phase),
            Wave::Pulse => {
                let w = self.width;
                tables::read(table, phase) - tables::read(table, phase - w) - (2.0 * w - 1.0)
            }
        }
    }
}

impl Block for Osc {
    fn reset(&mut self, cx: &Ctx) {
        let mut rng = Rng::new(cx.seed ^ 0x05C1);
        for (i, p) in self.phases.iter_mut().enumerate() {
            // Unison voices start spread out so they don't all click in on the same edge.
            *p = if self.free {
                rng.next() * 0.5 + 0.5
            } else if i == 0 {
                0.0
            } else {
                (i as f32 * 0.618_034).fract()
            };
        }
        self.last_sync = 0.0;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let out = &mut outputs[0];
        let fm_in = wired(inputs, 1);
        let sync_in = wired(inputs, 2);
        let nyquist = 0.49;
        // Without a pitch input the steps and tables hold for the whole block.
        let pitch_in = wired(inputs, 0);
        let note = cx.note + self.offset;
        let base_at = |s: usize| {
            let pitch = note + pitch_in.map_or(0.0, |b| b[0][s]);
            midi_to_hz(pitch.clamp(-60.0, 200.0)) / cx.rate
        };
        let mut incs = [0.0f32; 8];
        let mut tabs: [&[f32]; 8] = [&[]; 8];
        if pitch_in.is_none() {
            self.fill(base_at(0), nyquist, &mut incs, &mut tabs);
        }
        for s in 0..BLOCK {
            if pitch_in.is_some() {
                self.fill(base_at(s), nyquist, &mut incs, &mut tabs);
            }
            if let Some(sync) = sync_in {
                let v = sync[0][s];
                if self.last_sync <= 0.0 && v > 0.0 {
                    self.phases = [0.0; 8];
                }
                self.last_sync = v;
            }
            let fm = fm_in.map_or(0.0, |b| b[0][s] * self.fm / std::f32::consts::TAU);
            let (mut l, mut r) = (0.0, 0.0);
            for v in 0..self.unison {
                let (_, gl, gr) = self.voices[v];
                let x = self.shape(tabs[v], self.phases[v] + fm);
                l += x * gl;
                r += x * gr;
                self.phases[v] = (self.phases[v] + incs[v]).fract();
            }
            out[0][s] = l;
            out[1][s] = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{magnitude, run};
    use bpe_graph::Param;

    fn osc(wave: &str) -> Osc {
        Osc::new(&Knobs::default().with("wave", Param::Choice(wave.into())))
    }

    #[test]
    fn osc_pitch_is_within_a_cent() {
        let mut cx = Ctx { note: 69.0, ..Ctx::default() };
        let out = run(&mut osc("sine"), &mut cx, 750, |_, _| vec![]);
        let left: Vec<f32> = out.iter().map(|f| f[0]).collect();
        // Count rising zero crossings, measuring between the first and the last.
        let ups: Vec<f64> = (1..left.len())
            .filter(|&i| left[i - 1] <= 0.0 && left[i] > 0.0)
            .map(|i| i as f64 - 1.0 + (-left[i - 1] / (left[i] - left[i - 1])) as f64)
            .collect();
        let hz = (ups.len() - 1) as f64 / ((ups[ups.len() - 1] - ups[0]) / 48_000.0);
        let cents = 1200.0 * (hz / 440.0).log2();
        assert!(cents.abs() < 1.0, "{hz} Hz is {cents} cents off");
    }

    /// The loudest alias of a `wave` at `hz`, in dB under the fundamental.
    fn worst_alias(wave: &str, hz: f64) -> f64 {
        let note = 69.0 + 12.0 * (hz / 440.0).log2() as f32;
        let mut cx = Ctx { note, ..Ctx::default() };
        let out = run(&mut osc(wave), &mut cx, 750, |_, _| vec![]);
        let left: Vec<f32> = out.iter().map(|f| f[0]).collect();
        let fundamental = magnitude(&left, hz, 48_000.0);
        let mut worst = f64::NEG_INFINITY;
        for k in 2..60 {
            let f = hz * k as f64;
            if f < 24_000.0 {
                continue;
            }
            // Where harmonic k folds back to.
            let folded = (f % 48_000.0).min(48_000.0 - f % 48_000.0);
            if (folded / hz - (folded / hz).round()).abs() * hz < 30.0 {
                continue;
            }
            let db = 20.0 * (magnitude(&left, folded, 48_000.0) / fundamental).log10();
            worst = worst.max(db);
        }
        worst
    }

    #[test]
    fn saw_and_square_alias_below_minus_60_db_at_5_khz() {
        for wave in ["saw", "square", "pulse"] {
            let db = worst_alias(wave, 5_000.0);
            assert!(db < -60.0, "{wave} aliases at {db} dB");
        }
    }

    #[test]
    fn unison_spreads_across_the_sides() {
        let k = Knobs::default()
            .with("unison", Param::Number(4.0))
            .with("spread", Param::Number(1.0))
            .with("detune", Param::Number(30.0));
        let out = run(&mut Osc::new(&k), &mut Ctx::default(), 100, |_, _| vec![]);
        assert!(out.iter().any(|f| (f[0] - f[1]).abs() > 0.1));
    }
}
