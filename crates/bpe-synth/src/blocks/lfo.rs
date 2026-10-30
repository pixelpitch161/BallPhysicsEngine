//! The LFO, free in Hz or synced to the tempo, and sample and hold.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, Rng, at, wired};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wave {
    Sine,
    Triangle,
    Saw,
    Square,
    Random,
}

/// Beats in one cycle for a sync choice, or `None` for free running in Hz.
fn sync_beats(choice: &str) -> Option<f32> {
    Some(match choice {
        "1/1" => 4.0,
        "1/2" => 2.0,
        "1/4" => 1.0,
        "1/8" => 0.5,
        "1/16" => 0.25,
        "1/4T" => 2.0 / 3.0,
        "1/8T" => 1.0 / 3.0,
        _ => return None,
    })
}

/// Cycles per second for a rate knob and a sync choice.
fn cycles_per_second(k: &Knobs, cx: &Ctx, beats: Option<f32>) -> f32 {
    match beats {
        Some(beats) => 1.0 / (beats * cx.seconds_per_beat.max(0.01)),
        None => k.number("rate", 2.0).clamp(0.01, 50.0),
    }
}

pub struct Lfo {
    knobs: Knobs,
    wave: Wave,
    sync: Option<f32>,
    retrigger: bool,
    start: f32,
    phase: f64,
    rng: Rng,
    held: f32,
}

impl Lfo {
    pub fn new(k: &Knobs) -> Self {
        let wave = match k.choice("wave", "sine") {
            "triangle" => Wave::Triangle,
            "saw" => Wave::Saw,
            "square" => Wave::Square,
            "random" => Wave::Random,
            _ => Wave::Sine,
        };
        Self {
            knobs: k.clone(),
            wave,
            sync: sync_beats(k.choice("sync", "off")),
            retrigger: k.flag("retrigger", true),
            start: k.number("phase", 0.0).clamp(0.0, 1.0),
            phase: 0.0,
            rng: Rng::new(1),
            held: 0.0,
        }
    }
}

impl Block for Lfo {
    fn reset(&mut self, cx: &Ctx) {
        self.rng = Rng::new(cx.seed ^ 0x1F0);
        self.held = self.rng.next();
        self.phase = self.start as f64;
    }

    fn process(&mut self, cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let hz = cycles_per_second(&self.knobs, cx, self.sync) as f64;
        if !self.retrigger {
            // Free running: the phase follows the soundtrack's clock, not the note.
            self.phase = (cx.time * hz + self.start as f64).fract();
        } else if cx.trigger {
            self.phase = self.start as f64;
        }
        let inc = hz / cx.rate as f64;
        for s in 0..BLOCK {
            let p = self.phase as f32;
            let v = match self.wave {
                Wave::Sine => (p * std::f32::consts::TAU).sin(),
                Wave::Triangle => 1.0 - 4.0 * (p - 0.25 - (p - 0.25).floor() - 0.5).abs(),
                Wave::Saw => 2.0 * p - 1.0,
                Wave::Square => {
                    if p < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Random => self.held,
            };
            outputs[0][0][s] = v;
            outputs[0][1][s] = v;
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = self.rng.next();
            }
        }
    }
}

/// Sample and hold: picks up noise or its input at a steady rate and holds it.
pub struct Hold {
    knobs: Knobs,
    from_input: bool,
    sync: Option<f32>,
    phase: f64,
    rng: Rng,
    held: f32,
}

impl Hold {
    pub fn new(k: &Knobs) -> Self {
        Self {
            knobs: k.clone(),
            from_input: k.choice("source", "noise") == "input",
            sync: sync_beats(k.choice("sync", "off")),
            phase: 0.0,
            rng: Rng::new(1),
            held: 0.0,
        }
    }
}

impl Block for Hold {
    fn reset(&mut self, cx: &Ctx) {
        self.rng = Rng::new(cx.seed ^ 0x401D);
        self.phase = 1.0;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let inc = cycles_per_second(&self.knobs, cx, self.sync) as f64 / cx.rate as f64;
        let from_input = self.from_input && wired(inputs, 0).is_some();
        for s in 0..BLOCK {
            if self.phase >= 1.0 {
                self.phase = self.phase.fract();
                self.held = if from_input { at(inputs, 0, s, 0.0) } else { self.rng.next() };
            }
            outputs[0][0][s] = self.held;
            outputs[0][1][s] = self.held;
            self.phase += inc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use bpe_graph::Param;

    #[test]
    fn synced_lfo_follows_the_tempo() {
        let k = Knobs::default().with("sync", Param::Choice("1/4".into())).with("wave", Param::Choice("saw".into()));
        let mut cx = Ctx { seconds_per_beat: 0.5, ..Ctx::default() };
        let out = run(&mut Lfo::new(&k), &mut cx, 1200, |_, _| vec![]);
        let wraps: Vec<usize> = (1..out.len()).filter(|&i| out[i][0] < out[i - 1][0]).collect();
        let cycle = wraps[1] - wraps[0];
        assert!((cycle as i64 - 24_000).abs() <= 1, "cycle {cycle}");
    }

    #[test]
    fn hold_steps_at_its_rate() {
        let k = Knobs::default().with("rate", Param::Number(10.0));
        let out = run(&mut Hold::new(&k), &mut Ctx::default(), 750, |_, _| vec![]);
        let changes = (1..out.len()).filter(|&i| out[i][0] != out[i - 1][0]).count();
        assert!((9..=10).contains(&changes), "{changes} steps");
    }
}
