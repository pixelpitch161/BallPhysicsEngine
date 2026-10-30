//! Gain, with a modulation input that multiplies it (an envelope, usually).

use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

pub struct Amp {
    gain: f32,
}

impl Amp {
    pub fn new(k: &Knobs) -> Self {
        Self { gain: (k.number("gain", 0.0).clamp(-60.0, 12.0) / 20.0 * std::f32::consts::LN_10).exp() }
    }
}

impl Block for Amp {
    fn reset(&mut self, _cx: &Ctx) {}

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        let modulation = wired(inputs, 1);
        for side in 0..2 {
            for s in 0..BLOCK {
                let m = modulation.map_or(1.0, |b| b[side][s]);
                outputs[0][side][s] = input[side][s] * self.gain * m;
            }
        }
    }
}
