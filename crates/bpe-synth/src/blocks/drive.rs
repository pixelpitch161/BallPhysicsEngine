//! Distortion: soft (tanh), hard (clip), fold or tube, with a tone control and a dry/wet mix.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Soft,
    Hard,
    Fold,
    Tube,
}

pub struct Drive {
    mode: Mode,
    gain: f32,
    bias: f32,
    /// The tone filter's one-pole coefficient, or `None` when the tone is fully open.
    tone: Option<f32>,
    tone_knob: f32,
    mix: f32,
    lowpass: [f32; 2],
}

impl Drive {
    pub fn new(k: &Knobs) -> Self {
        let mode = match k.choice("mode", "soft") {
            "hard" => Mode::Hard,
            "fold" => Mode::Fold,
            "tube" => Mode::Tube,
            _ => Mode::Soft,
        };
        Self {
            mode,
            gain: 1.0 + k.number("amount", 0.3).clamp(0.0, 1.0) * 39.0,
            bias: k.number("bias", 0.0).clamp(-1.0, 1.0),
            tone: None,
            tone_knob: k.number("tone", 1.0).clamp(0.0, 1.0),
            mix: k.number("mix", 1.0).clamp(0.0, 1.0),
            lowpass: [0.0; 2],
        }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        match self.mode {
            Mode::Soft => x.tanh(),
            // Leans one way, as a valve does: even harmonics.
            Mode::Tube => (x + 0.2).tanh() - 0.2f32.tanh(),
            Mode::Hard => x.clamp(-1.0, 1.0),
            // A triangle fold: past 1 the wave turns back down.
            Mode::Fold => {
                let t = (x + 1.0).rem_euclid(4.0);
                if t < 2.0 { t - 1.0 } else { 3.0 - t }
            }
        }
    }
}

impl Block for Drive {
    fn reset(&mut self, cx: &Ctx) {
        self.lowpass = [0.0; 2];
        // From 1 kHz at 0 to wide open at 1.
        self.tone = (self.tone_knob < 1.0).then(|| {
            let hz = 1_000.0 * 20f32.powf(self.tone_knob);
            1.0 - (-std::f32::consts::TAU * hz / cx.rate).exp()
        });
    }

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        let centre = self.shape(self.bias);
        for side in 0..2 {
            for s in 0..BLOCK {
                let mut wet = self.shape(input[side][s] * self.gain + self.bias) - centre;
                if let Some(c) = self.tone {
                    self.lowpass[side] = super::filter::flush(self.lowpass[side] + (wet - self.lowpass[side]) * c);
                    wet = self.lowpass[side];
                }
                outputs[0][side][s] =
                    if self.mix >= 1.0 { wet } else { input[side][s] * (1.0 - self.mix) + wet * self.mix };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{constant, run, sine};
    use bpe_graph::Param;

    #[test]
    fn tube_is_asymmetric() {
        let k = Knobs::default().with("mode", Param::Choice("tube".into())).with("amount", Param::Number(0.0));
        let up = run(&mut Drive::new(&k), &mut Ctx::default(), 1, |_, _| vec![Some(constant(0.8))]);
        let down = run(&mut Drive::new(&k), &mut Ctx::default(), 1, |_, _| vec![Some(constant(-0.8))]);
        assert!((up[10][0].abs() - down[10][0].abs()).abs() > 0.05, "{} {}", up[10][0], down[10][0]);
    }

    #[test]
    fn drive_mix_zero_is_dry() {
        let k = Knobs::default().with("amount", Param::Number(1.0)).with("mix", Param::Number(0.0));
        let out =
            run(&mut Drive::new(&k), &mut Ctx::default(), 10, |b, _| vec![Some(sine(440.0, 48_000.0, b * BLOCK))]);
        for (i, f) in out.iter().enumerate() {
            assert_eq!(f[0], sine(440.0, 48_000.0, i / BLOCK * BLOCK)[0][i % BLOCK]);
        }
    }
}
