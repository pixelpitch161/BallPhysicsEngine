//! Distortion: soft (tanh), hard (clip) or fold.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Soft,
    Hard,
    Fold,
}

pub struct Drive {
    mode: Mode,
    gain: f32,
    bias: f32,
}

impl Drive {
    pub fn new(k: &Knobs) -> Self {
        let mode = match k.choice("mode", "soft") {
            "hard" => Mode::Hard,
            "fold" => Mode::Fold,
            _ => Mode::Soft,
        };
        Self {
            mode,
            gain: 1.0 + k.number("amount", 0.3).clamp(0.0, 1.0) * 39.0,
            bias: k.number("bias", 0.0).clamp(-1.0, 1.0),
        }
    }

    #[inline]
    fn shape(&self, x: f32) -> f32 {
        match self.mode {
            Mode::Soft => x.tanh(),
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
    fn reset(&mut self, _cx: &Ctx) {}

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        let centre = self.shape(self.bias);
        for side in 0..2 {
            for s in 0..BLOCK {
                outputs[0][side][s] = self.shape(input[side][s] * self.gain + self.bias) - centre;
            }
        }
    }
}
