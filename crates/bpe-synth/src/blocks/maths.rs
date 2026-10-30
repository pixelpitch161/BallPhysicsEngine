//! Maths and input blocks: mix, add, multiply, scale and offset, crossfade, pitch to Hz, the
//! note's values, macros and the voice's output.

use crate::{BLOCK, Block, Buf, Ctx, Knobs, SILENT, midi_to_hz, wired};

/// Applies `f` frame by frame to the given inputs (left and right separately).
fn each(outputs: &mut [Buf], mut f: impl FnMut(usize, usize) -> f32) {
    for side in 0..2 {
        for s in 0..BLOCK {
            outputs[0][side][s] = f(side, s);
        }
    }
}

#[inline]
fn get(inputs: &[Option<&Buf>], i: usize, side: usize, s: usize, default: f32) -> f32 {
    wired(inputs, i).map_or(default, |b| b[side][s])
}

/// Up to four inputs, each with a level.
pub struct Mix {
    levels: [f32; 4],
}

impl Mix {
    pub fn new(k: &Knobs) -> Self {
        Self { levels: [1, 2, 3, 4].map(|i| k.number(&format!("level{i}"), 1.0).clamp(0.0, 2.0)) }
    }
}

impl Block for Mix {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        each(outputs, |side, s| (0..4).map(|i| get(inputs, i, side, s, 0.0) * self.levels[i]).sum());
    }
}

/// a + b, or a × b.
pub struct Arith {
    multiply: bool,
}

impl Arith {
    pub fn add() -> Self {
        Self { multiply: false }
    }
    pub fn multiply() -> Self {
        Self { multiply: true }
    }
}

impl Block for Arith {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let multiply = self.multiply;
        each(outputs, |side, s| {
            if multiply {
                get(inputs, 0, side, s, 1.0) * get(inputs, 1, side, s, 1.0)
            } else {
                get(inputs, 0, side, s, 0.0) + get(inputs, 1, side, s, 0.0)
            }
        });
    }
}

/// in × scale + offset.
pub struct Scale {
    scale: f32,
    offset: f32,
}

impl Scale {
    pub fn new(k: &Knobs) -> Self {
        Self { scale: k.number("scale", 1.0), offset: k.number("offset", 0.0) }
    }
}

impl Block for Scale {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        each(outputs, |side, s| get(inputs, 0, side, s, 0.0) * self.scale + self.offset);
    }
}

/// From a to b as the mix goes from 0 to 1; a wired mix input overrides the knob.
pub struct Crossfade {
    mix: f32,
}

impl Crossfade {
    pub fn new(k: &Knobs) -> Self {
        Self { mix: k.number("mix", 0.5) }
    }
}

impl Block for Crossfade {
    fn set_knob(&mut self, name: &str, value: f32) {
        if name == "mix" {
            self.mix = value.clamp(0.0, 1.0);
        }
    }

    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        each(outputs, |side, s| {
            let m = get(inputs, 2, side, s, self.mix).clamp(0.0, 1.0);
            get(inputs, 0, side, s, 0.0) * (1.0 - m) + get(inputs, 1, side, s, 0.0) * m
        });
    }
}

/// A MIDI pitch to Hz.
pub struct PitchToHz;

impl Block for PitchToHz {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        each(outputs, |side, s| midi_to_hz(get(inputs, 0, side, s, 60.0).clamp(-60.0, 200.0)));
    }
}

/// The note's pitch, velocity, gate (1 while held) and seconds since it started.
pub struct NoteIn;

impl Block for NoteIn {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let values = [cx.note, cx.velocity, if cx.gate { 1.0 } else { 0.0 }];
        for (o, v) in outputs.iter_mut().zip(values) {
            *o = [[v; BLOCK]; 2];
        }
        if let Some(time) = outputs.get_mut(3) {
            for s in 0..BLOCK {
                let t = cx.since_on + s as f32 / cx.rate;
                time[0][s] = t;
                time[1][s] = t;
            }
        }
    }
}

/// One of the patch's macros: min + value × (max - min).
pub struct Macro {
    value: f32,
    min: f32,
    max: f32,
}

impl Macro {
    pub fn new(k: &Knobs) -> Self {
        let (min, max) = (k.number("min", 0.0), k.number("max", 1.0));
        Self { value: min + k.number("value", 0.5).clamp(0.0, 1.0) * (max - min), min, max }
    }
}

impl Block for Macro {
    fn set_knob(&mut self, name: &str, value: f32) {
        if name == "value" {
            self.value = self.min + value.clamp(0.0, 1.0) * (self.max - self.min);
        }
    }

    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        outputs[0] = [[self.value; BLOCK]; 2];
    }
}

/// The voice's sound: left and right in, one stereo buffer out (which the program reads).
/// A mono patch wires only the left; it goes to both sides.
pub struct VoiceOut;

impl Block for VoiceOut {
    fn reset(&mut self, _cx: &Ctx) {}
    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let (left, right) = (wired(inputs, 0), wired(inputs, 1));
        outputs[0] = match (left, right) {
            (Some(l), Some(r)) => [l[0], r[1]],
            (Some(l), None) => *l,
            (None, Some(r)) => *r,
            (None, None) => SILENT,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{constant, run};

    #[test]
    fn crossfade_and_scale() {
        let k = Knobs::default().with("mix", bpe_graph::Param::Number(0.25));
        let out =
            run(&mut Crossfade::new(&k), &mut Ctx::default(), 1, |_, _| vec![Some(constant(1.0)), Some(constant(3.0))]);
        assert_eq!(out[0][0], 1.5);
        let k =
            Knobs::default().with("scale", bpe_graph::Param::Number(2.0)).with("offset", bpe_graph::Param::Number(1.0));
        let out = run(&mut Scale::new(&k), &mut Ctx::default(), 1, |_, _| vec![Some(constant(3.0))]);
        assert_eq!(out[0][0], 7.0);
        let out = run(&mut PitchToHz, &mut Ctx::default(), 1, |_, _| vec![Some(constant(69.0))]);
        assert!((out[0][0] - 440.0).abs() < 1e-3);
    }
}
