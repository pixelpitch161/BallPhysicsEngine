//! The state-variable filter (Cytomic's trapezoidal SVF): low, high, band or notch.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, at, wired};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Low,
    High,
    Band,
    Notch,
}

pub struct Svf {
    mode: Mode,
    cutoff: f32,
    k: f32,
    key_track: f32,
    /// (ic1eq, ic2eq) per side.
    state: [(f32, f32); 2],
    /// The cutoff the coefficients were worked out for, and (g, a1, a2, a3).
    coeffs: (f32, [f32; 4]),
}

/// Passes values under 2 unchanged and bends louder ones smoothly towards 4, so a screaming
/// resonance stays loud but bounded.
#[inline]
pub(crate) fn knee(x: f32) -> f32 {
    let a = x.abs();
    if a <= 2.0 { x } else { x.signum() * (2.0 + 2.0 * ((a - 2.0) * 0.5).tanh()) }
}

/// Zero for a value too small to hear, so a filter ringing down to silence never goes
/// denormal (which makes the processor tens of times slower).
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-15 { 0.0 } else { x }
}

/// Clamps a cutoff to what the filter can do at `rate`.
pub(crate) fn clamp_cutoff(hz: f32, rate: f32) -> f32 {
    if hz.is_finite() { hz.clamp(20.0, rate * 0.45) } else { 20.0 }
}

/// The cutoff for this frame: the knob moved by `octaves` and by key tracking from middle C.
#[inline]
pub(crate) fn cutoff_at(knob: f32, octaves: f32, key_track: f32, note: f32, rate: f32) -> f32 {
    clamp_cutoff(knob * (octaves.clamp(-12.0, 12.0) + key_track * (note - 60.0) / 12.0).exp2(), rate)
}

impl Svf {
    pub fn new(k: &Knobs) -> Self {
        let mode = match k.choice("mode", "low") {
            "high" => Mode::High,
            "band" => Mode::Band,
            "notch" => Mode::Notch,
            _ => Mode::Low,
        };
        let resonance = k.number("resonance", 0.0).clamp(0.0, 1.0);
        Self {
            mode,
            cutoff: k.number("cutoff", 2_000.0),
            // 0 resonance is a Butterworth response (3 dB down at the cutoff); 1 rings but holds.
            k: std::f32::consts::SQRT_2 * (1.0 - resonance) + 0.02 * resonance,
            key_track: k.number("key_track", 0.0).clamp(0.0, 1.0),
            state: [(0.0, 0.0); 2],
            coeffs: (-1.0, [0.0; 4]),
        }
    }

    fn coefficients(&mut self, hz: f32, rate: f32) -> [f32; 4] {
        if self.coeffs.0 != hz {
            let g = (std::f32::consts::PI * hz / rate).tan();
            let a1 = 1.0 / (1.0 + g * (g + self.k));
            self.coeffs = (hz, [g, a1, g * a1, g * g * a1]);
        }
        self.coeffs.1
    }
}

impl Block for Svf {
    fn reset(&mut self, _cx: &Ctx) {
        self.state = [(0.0, 0.0); 2];
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        let k = self.k;
        for s in 0..BLOCK {
            let hz = cutoff_at(self.cutoff, at(inputs, 1, s, 0.0), self.key_track, cx.note, cx.rate);
            let [_, a1, a2, a3] = self.coefficients(hz, cx.rate);
            for side in 0..2 {
                let v0 = input[side][s];
                let (ic1, ic2) = &mut self.state[side];
                let v3 = v0 - *ic2;
                let v1 = a1 * *ic1 + a2 * v3;
                let v2 = *ic2 + a2 * *ic1 + a3 * v3;
                *ic1 = flush(knee(2.0 * v1 - *ic1));
                *ic2 = flush(knee(2.0 * v2 - *ic2));
                outputs[0][side][s] = knee(match self.mode {
                    Mode::Low => v2,
                    Mode::Band => v1,
                    Mode::High => v0 - k * v1 - v2,
                    Mode::Notch => v0 - k * v1,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{magnitude, run, sine};
    use bpe_graph::Param;

    fn gain_at(hz: f32, cutoff: f32, mode: &str) -> f64 {
        let k = Knobs::default().with("cutoff", Param::Number(cutoff)).with("mode", Param::Choice(mode.into()));
        let out = run(&mut Svf::new(&k), &mut Ctx::default(), 1500, |b, _| vec![Some(sine(hz, 48_000.0, b * BLOCK))]);
        let tail: Vec<f32> = out[48_000..].iter().map(|f| f[0]).collect();
        20.0 * magnitude(&tail, hz as f64, 48_000.0).log10()
    }

    #[test]
    fn a_filter_ringing_down_to_silence_reaches_zero() {
        // Denormal leftovers once slowed every block they passed through.
        let k = Knobs::default().with("resonance", Param::Number(0.9));
        let out = run(&mut Svf::new(&k), &mut Ctx::default(), 3000, |b, _| {
            vec![Some(if b < 10 { sine(440.0, 48_000.0, b * BLOCK) } else { crate::SILENT })]
        });
        assert!(out[out.len() - 1000..].iter().all(|f| f[0] == 0.0 || f[0].abs() > 1e-30));
        assert_eq!(out[out.len() - 1][0], 0.0);
    }

    #[test]
    fn svf_low_pass_is_3_db_down_at_cutoff() {
        let db = gain_at(1_000.0, 1_000.0, "low");
        assert!((db + 3.0).abs() < 1.0, "{db} dB at the cutoff");
        assert!(gain_at(100.0, 1_000.0, "low").abs() < 0.5);
        assert!(gain_at(8_000.0, 1_000.0, "low") < -30.0);
        assert!(gain_at(100.0, 1_000.0, "high") < -30.0);
    }
}
