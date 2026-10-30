//! The envelope: attack, decay, sustain and release, gated by the note.

use crate::{BLOCK, Block, Buf, Ctx, Knobs};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

pub struct Adsr {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    curve: f32,
    stage: Stage,
    /// Seconds into the current stage.
    t: f32,
    level: f32,
    /// The level the current attack or release started from.
    from: f32,
}

/// Bends a 0 to 1 ramp: positive curves start slow, negative ones start fast.
#[inline]
fn bend(x: f32, curve: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if curve >= 0.0 { x.powf(1.0 + 3.0 * curve) } else { 1.0 - (1.0 - x).powf(1.0 - 3.0 * curve) }
}

impl Adsr {
    pub fn new(k: &Knobs) -> Self {
        Self {
            attack: k.number("attack", 0.005).clamp(0.0, 10.0),
            decay: k.number("decay", 0.2).clamp(0.0, 10.0),
            sustain: k.number("sustain", 0.7).clamp(0.0, 1.0),
            release: k.number("release", 0.3).clamp(0.0, 10.0),
            curve: k.number("curve", 0.0).clamp(-1.0, 1.0),
            stage: Stage::Off,
            t: 0.0,
            level: 0.0,
            from: 0.0,
        }
    }

    fn start(&mut self, stage: Stage) {
        self.stage = stage;
        self.t = 0.0;
        self.from = self.level;
    }

    fn step(&mut self, dt: f32) -> f32 {
        self.t += dt;
        match self.stage {
            Stage::Attack => {
                if self.t >= self.attack {
                    self.level = 1.0;
                    self.start(Stage::Decay);
                } else {
                    self.level = self.from + (1.0 - self.from) * bend(self.t / self.attack, self.curve);
                }
            }
            Stage::Decay => {
                if self.t >= self.decay {
                    self.level = self.sustain;
                    self.start(Stage::Sustain);
                } else {
                    self.level = 1.0 - (1.0 - self.sustain) * bend(self.t / self.decay, -self.curve);
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                if self.t >= self.release {
                    self.level = 0.0;
                    self.stage = Stage::Off;
                } else {
                    self.level = self.from * (1.0 - bend(self.t / self.release, -self.curve));
                }
            }
            Stage::Off => self.level = 0.0,
        }
        self.level
    }
}

impl Block for Adsr {
    fn reset(&mut self, _cx: &Ctx) {
        self.level = 0.0;
        self.stage = Stage::Off;
    }

    fn process(&mut self, cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        if cx.trigger {
            self.start(Stage::Attack);
        }
        if !cx.gate && !matches!(self.stage, Stage::Release | Stage::Off) {
            self.start(Stage::Release);
        }
        let dt = 1.0 / cx.rate;
        for s in 0..BLOCK {
            let v = self.step(dt);
            outputs[0][0][s] = v;
            outputs[0][1][s] = v;
        }
    }

    fn idle(&self) -> bool {
        self.stage == Stage::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use bpe_graph::Param;

    #[test]
    fn adsr_stages_land_within_one_block() {
        let k = Knobs::default()
            .with("attack", Param::Number(0.1))
            .with("decay", Param::Number(0.1))
            .with("sustain", Param::Number(0.5))
            .with("release", Param::Number(0.2));
        // Gate closes at 0.5 s.
        let out = run(&mut Adsr::new(&k), &mut Ctx::default(), 1200, |b, cx| {
            cx.gate = b * BLOCK < 24_000;
            vec![]
        });
        let env: Vec<f32> = out.iter().map(|f| f[0]).collect();
        let peak = env.iter().position(|&v| v >= 0.999).unwrap();
        assert!((peak as i64 - 4_800).abs() <= 64, "peak at {peak}");
        assert!((env[14_400] - 0.5).abs() < 0.01);
        let silent = env.iter().skip(24_000).position(|&v| v == 0.0).unwrap() + 24_000;
        assert!((silent as i64 - 33_600).abs() <= 64, "silent at {silent}");
    }

    #[test]
    fn retriggering_starts_from_the_current_level() {
        let mut env = Adsr::new(&Knobs::default().with("attack", Param::Number(0.1)));
        let out = run(&mut env, &mut Ctx::default(), 200, |b, cx| {
            cx.trigger = b == 0 || b == 100;
            vec![]
        });
        // No jump back to zero at the retrigger.
        assert!(out[100 * BLOCK][0] > 0.5);
    }
}
