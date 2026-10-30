//! Bitcrush: fewer bits and a lower sample rate, for grit.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

pub struct Bitcrush {
    levels: f32,
    hold: u32,
    mix: f32,
    count: u32,
    held: [f32; 2],
}

impl Bitcrush {
    pub fn new(k: &Knobs) -> Self {
        let bits = k.number("bits", 8.0).clamp(1.0, 16.0).round();
        Self {
            levels: (bits - 1.0).exp2(),
            hold: k.number("downsample", 1.0).clamp(1.0, 32.0).round() as u32,
            mix: k.number("mix", 1.0).clamp(0.0, 1.0),
            count: 0,
            held: [0.0; 2],
        }
    }
}

impl Block for Bitcrush {
    fn reset(&mut self, _cx: &Ctx) {
        self.count = 0;
        self.held = [0.0; 2];
    }

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        for s in 0..BLOCK {
            if self.count == 0 {
                for side in 0..2 {
                    // Mid-rise steps, so even one bit has two levels and never sits on zero.
                    let x = input[side][s].clamp(-1.0, 0.999_999);
                    self.held[side] = ((x * self.levels).floor() + 0.5) / self.levels;
                }
            }
            self.count = (self.count + 1) % self.hold;
            for side in 0..2 {
                outputs[0][side][s] = input[side][s] * (1.0 - self.mix) + self.held[side] * self.mix;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{run, sine};
    use crate::{BLOCK, Ctx, Knobs};
    use bpe_graph::Param;

    #[test]
    fn one_bit_has_two_levels() {
        let k = Knobs::default().with("bits", Param::Number(1.0));
        let out =
            run(&mut Bitcrush::new(&k), &mut Ctx::default(), 100, |b, _| vec![Some(sine(440.0, 48_000.0, b * BLOCK))]);
        let mut levels: Vec<u32> = out.iter().map(|f| f[0].to_bits()).collect();
        levels.sort();
        levels.dedup();
        assert!(levels.len() <= 2, "{levels:?}");
    }
}
