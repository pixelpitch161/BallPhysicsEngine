//! A four-pole low-pass ladder with drive. The tanh in the feedback path keeps it stable at any
//! resonance.

use super::filter::cutoff_at;
use crate::{BLOCK, Block, Buf, Ctx, Knobs, at, wired};

pub struct Ladder {
    cutoff: f32,
    feedback: f32,
    drive: f32,
    key_track: f32,
    stages: [[f32; 4]; 2],
    /// The cutoff the step was worked out for, and the step.
    step: (f32, f32),
}

impl Ladder {
    pub fn new(k: &Knobs) -> Self {
        Self {
            cutoff: k.number("cutoff", 2_000.0),
            feedback: k.number("resonance", 0.0).clamp(0.0, 1.0) * 3.95,
            drive: 1.0 + k.number("drive", 0.0).clamp(0.0, 1.0) * 9.0,
            key_track: k.number("key_track", 0.0).clamp(0.0, 1.0),
            stages: [[0.0; 4]; 2],
            step: (-1.0, 0.0),
        }
    }
}

impl Block for Ladder {
    fn reset(&mut self, _cx: &Ctx) {
        self.stages = [[0.0; 4]; 2];
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        // Loudness lost to the feedback, made back up a little.
        let makeup = 1.0 + self.feedback * 0.5;
        for s in 0..BLOCK {
            let hz = cutoff_at(self.cutoff, at(inputs, 1, s, 0.0), self.key_track, cx.note, cx.rate);
            if self.step.0 != hz {
                self.step = (hz, 1.0 - (-std::f32::consts::TAU * hz / cx.rate).exp());
            }
            let g = self.step.1;
            for side in 0..2 {
                let y = &mut self.stages[side];
                let x = (input[side][s] * self.drive - self.feedback * y[3]).tanh();
                y[0] += g * (x - y[0]);
                y[1] += g * (y[0] - y[1]);
                y[2] += g * (y[1] - y[2]);
                y[3] += g * (y[2] - y[3]);
                *y = y.map(super::filter::flush);
                outputs[0][side][s] = y[3] * makeup / self.drive.sqrt();
            }
        }
    }
}
