//! Stereo width, and an envelope follower that turns a sound's level into a control.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

/// Mid/side width: 0 is mono, 1 unchanged, 2 twice as wide.
pub struct Width {
    width: f32,
}

impl Width {
    pub fn new(k: &Knobs) -> Self {
        Self { width: k.number("width", 1.0).clamp(0.0, 2.0) }
    }
}

impl Block for Width {
    fn reset(&mut self, _cx: &Ctx) {}

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        for s in 0..BLOCK {
            let mid = (input[0][s] + input[1][s]) * 0.5;
            let side = (input[0][s] - input[1][s]) * 0.5 * self.width;
            outputs[0][0][s] = mid + side;
            outputs[0][1][s] = mid - side;
        }
    }
}

/// The level of its input as a control: rises over the attack, falls over the release.
pub struct Follower {
    attack: f32,
    release: f32,
    level: f32,
    rate: f32,
    coeffs: (f32, f32),
}

impl Follower {
    pub fn new(k: &Knobs) -> Self {
        Self {
            attack: k.number("attack", 0.01).max(0.0),
            release: k.number("release", 0.2).max(0.0),
            level: 0.0,
            rate: 0.0,
            coeffs: (1.0, 1.0),
        }
    }
}

/// The one-pole coefficient that gets about 63% of the way in `seconds`.
pub(crate) fn pole(seconds: f32, rate: f32) -> f32 {
    if seconds <= 0.0 { 1.0 } else { 1.0 - (-1.0 / (seconds * rate)).exp() }
}

impl Block for Follower {
    fn reset(&mut self, _cx: &Ctx) {
        self.level = 0.0;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        if self.rate != cx.rate {
            self.rate = cx.rate;
            self.coeffs = (pole(self.attack, cx.rate), pole(self.release, cx.rate));
        }
        let input = wired(inputs, 0);
        for s in 0..BLOCK {
            let now = input.map_or(0.0, |b| b[0][s].abs().max(b[1][s].abs()));
            let c = if now > self.level { self.coeffs.0 } else { self.coeffs.1 };
            self.level = super::filter::flush(self.level + (now - self.level) * c);
            outputs[0][0][s] = self.level;
            outputs[0][1][s] = self.level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{constant, run, sine};
    use crate::{BLOCK, Ctx, Knobs, SILENT};
    use bpe_graph::Param;

    #[test]
    fn width_zero_is_mono() {
        let k = Knobs::default().with("width", Param::Number(0.0));
        let out = run(&mut Width::new(&k), &mut Ctx::default(), 50, |b, _| {
            let mut buf = sine(440.0, 48_000.0, b * BLOCK);
            buf[1] = sine(660.0, 48_000.0, b * BLOCK)[0];
            vec![Some(buf)]
        });
        assert!(out.iter().all(|f| f[0] == f[1]));
    }

    #[test]
    fn follower_tracks_a_burst() {
        let k = Knobs::default();
        let burst = (0.1 * 48_000.0 / BLOCK as f32) as usize;
        let out = run(&mut Follower::new(&k), &mut Ctx::default(), burst + 400, |b, _| {
            vec![Some(if b < burst { constant(0.5) } else { SILENT })]
        });
        assert!(out[burst * BLOCK - 1][0] > 0.45, "{}", out[burst * BLOCK - 1][0]);
        assert!(out[burst * BLOCK + 24_000][0] < 0.1);
    }
}
