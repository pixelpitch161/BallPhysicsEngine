//! White or pink noise, seeded by the note so the same note gives the same noise.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, Rng};

pub struct Noise {
    pink: bool,
    rng: [Rng; 2],
    /// Paul Kellet's pink filter state, per side.
    b: [[f32; 7]; 2],
}

impl Noise {
    pub fn new(k: &Knobs) -> Self {
        Self { pink: k.choice("colour", "white") == "pink", rng: [Rng::new(1), Rng::new(2)], b: [[0.0; 7]; 2] }
    }
}

impl Block for Noise {
    fn reset(&mut self, cx: &Ctx) {
        self.rng = [Rng::new(cx.seed.wrapping_mul(2)), Rng::new(cx.seed.wrapping_mul(2) + 1)];
        self.b = [[0.0; 7]; 2];
    }

    fn process(&mut self, _cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        for side in 0..2 {
            for s in 0..BLOCK {
                let w = self.rng[side].next();
                outputs[0][side][s] = if self.pink {
                    let b = &mut self.b[side];
                    b[0] = 0.99886 * b[0] + w * 0.0555179;
                    b[1] = 0.99332 * b[1] + w * 0.0750759;
                    b[2] = 0.96900 * b[2] + w * 0.153852;
                    b[3] = 0.86650 * b[3] + w * 0.3104856;
                    b[4] = 0.55000 * b[4] + w * 0.5329522;
                    b[5] = -0.7616 * b[5] - w * 0.0168980;
                    let pink = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
                    b[6] = w * 0.115926;
                    pink * 0.11
                } else {
                    w
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use bpe_graph::Param;

    #[test]
    fn noise_repeats_for_the_same_note() {
        for colour in ["white", "pink"] {
            let k = Knobs::default().with("colour", Param::Choice(colour.into()));
            let a = run(&mut Noise::new(&k), &mut Ctx { seed: 7, ..Ctx::default() }, 10, |_, _| vec![]);
            let b = run(&mut Noise::new(&k), &mut Ctx { seed: 7, ..Ctx::default() }, 10, |_, _| vec![]);
            let c = run(&mut Noise::new(&k), &mut Ctx { seed: 8, ..Ctx::default() }, 10, |_, _| vec![]);
            assert_eq!(a, b);
            assert_ne!(a, c);
            assert!(a.iter().all(|f| f[0].abs() <= 1.5));
        }
    }
}
