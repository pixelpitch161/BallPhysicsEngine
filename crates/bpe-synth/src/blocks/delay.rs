//! Echoes: a delay in milliseconds or in beats, with feedback, ping-pong and a filtered
//! feedback path.

use super::eq::Biquad;
use super::filter::{flush, knee};
use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

/// The longest echo, in seconds.
const LONGEST: f32 = 4.0;

pub struct Delay {
    /// Milliseconds, or `None` to follow the tempo.
    ms: Option<f32>,
    /// Beats per echo when following the tempo.
    beats: f32,
    feedback: f32,
    ping_pong: bool,
    cuts: (f32, f32),
    mix: f32,
    lines: [Vec<f32>; 2],
    pos: usize,
    filters: Vec<Biquad>,
}

impl Delay {
    pub fn new(k: &Knobs) -> Self {
        let fraction = match k.choice("time_mode", "ms") {
            "1/1" => Some(4.0),
            "1/2" => Some(2.0),
            "1/4" => Some(1.0),
            "1/8" => Some(0.5),
            "1/16" => Some(0.25),
            _ => None,
        };
        let feel = match k.choice("feel", "straight") {
            "dotted" => 1.5,
            "triplet" => 2.0 / 3.0,
            _ => 1.0,
        };
        Self {
            ms: fraction.is_none().then(|| k.number("ms", 250.0).clamp(1.0, 2_000.0)),
            beats: fraction.unwrap_or(1.0) * feel,
            // Never quite 1, so the echoes always die away.
            feedback: k.number("feedback", 0.35).clamp(0.0, 0.98),
            ping_pong: k.flag("ping_pong", false),
            cuts: (k.number("low_cut", 20.0), k.number("high_cut", 20_000.0)),
            mix: k.number("mix", 0.3).clamp(0.0, 1.0),
            lines: [Vec::new(), Vec::new()],
            pos: 0,
            filters: Vec::new(),
        }
    }
}

impl Block for Delay {
    fn reset(&mut self, cx: &Ctx) {
        let seconds = match self.ms {
            Some(ms) => ms / 1_000.0,
            None => self.beats * cx.seconds_per_beat,
        };
        let frames = ((seconds.clamp(1.0 / cx.rate, LONGEST) * cx.rate).round() as usize).max(1);
        self.lines = [vec![0.0; frames], vec![0.0; frames]];
        self.pos = 0;
        self.filters.clear();
        if self.cuts.0 > 20.0 {
            self.filters.push(Biquad::pass(self.cuts.0, cx.rate, true));
        }
        if self.cuts.1 < 20_000.0 {
            self.filters.push(Biquad::pass(self.cuts.1, cx.rate, false));
        }
    }

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        let len = self.lines[0].len();
        for s in 0..BLOCK {
            let echo = [self.lines[0][self.pos], self.lines[1][self.pos]];
            let mut fed = echo;
            for f in &mut self.filters {
                for (side, v) in fed.iter_mut().enumerate() {
                    *v = f.tick(side, *v);
                }
            }
            let fed = fed.map(|v| v * self.feedback);
            let (l, r) = (input[0][s], input[1][s]);
            let written = if self.ping_pong { [(l + r) * 0.5 + fed[1], fed[0]] } else { [l + fed[0], r + fed[1]] };
            for side in 0..2 {
                self.lines[side][self.pos] = flush(knee(written[side]));
                outputs[0][side][s] = input[side][s] * (1.0 - self.mix) + echo[side] * self.mix;
            }
            self.pos = (self.pos + 1) % len;
        }
    }

    fn idle(&self) -> bool {
        self.lines.iter().all(|l| l.iter().all(|v| *v == 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use crate::{BLOCK, Ctx, Knobs, SILENT};
    use bpe_graph::Param;

    fn impulse_through(k: &Knobs, blocks: usize, spb: f32) -> Vec<[f32; 2]> {
        let mut cx = Ctx { seconds_per_beat: spb, ..Ctx::default() };
        run(&mut Delay::new(k), &mut cx, blocks, |b, _| {
            let mut buf = SILENT;
            if b == 0 {
                buf[0][0] = 1.0;
                buf[1][0] = 1.0;
            }
            vec![Some(buf)]
        })
    }

    #[test]
    fn delay_is_exact_to_the_sample() {
        let k = Knobs::default()
            .with("time_mode", Param::Choice("ms".into()))
            .with("ms", Param::Number(250.0))
            .with("mix", Param::Number(1.0))
            .with("feedback", Param::Number(0.0));
        let out = impulse_through(&k, 400, 0.5);
        for (i, f) in out.iter().enumerate() {
            if i == 12_000 {
                assert!((f[0] - 1.0).abs() < 1e-6, "{}", f[0]);
            } else {
                assert_eq!(f[0], 0.0, "at {i}");
            }
        }
        let dotted =
            k.clone().with("time_mode", Param::Choice("1/8".into())).with("feel", Param::Choice("dotted".into()));
        let out = impulse_through(&dotted, 400, 0.5);
        assert!((out[18_000][0] - 1.0).abs() < 1e-6);
        assert_eq!(out.iter().filter(|f| f[0] != 0.0).count(), 1);
    }

    #[test]
    fn ping_pong_alternates() {
        let k = Knobs::default()
            .with("ms", Param::Number(100.0))
            .with("mix", Param::Number(1.0))
            .with("feedback", Param::Number(0.5))
            .with("ping_pong", Param::Bool(true));
        let out = impulse_through(&k, 200, 0.5);
        let first = out[4_800];
        let second = out[9_600];
        assert!(first[0].abs() > 0.1 && first[1] == 0.0, "{first:?}");
        assert!(second[1].abs() > 0.1 && second[0] == 0.0, "{second:?}");
        let _ = BLOCK;
    }
}
