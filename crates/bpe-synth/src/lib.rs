//! Synth patches: the DSP blocks a patch is built from, the catalog the node panel shows, the
//! compiler that turns a patch graph into a flat program, and the voices that play notes
//! through it.
//!
//! Everything runs in blocks of [`BLOCK`] stereo frames of f32 at whatever rate [`Ctx::rate`]
//! says (the mixer uses 48 kHz). Nothing here knows about scenes, and nothing is random: the
//! same notes through the same patch give the same samples every time.

// DSP loops index several buffers by the same frame; iterators would hide that.
#![allow(clippy::needless_range_loop)]

pub mod blocks;
mod catalog;
mod program;
mod starters;
mod voices;

pub use blocks::missing_samples;
pub use catalog::catalog;
pub use program::{MacroKnob, Patch, Program, blank_patch, compile, macros, with_macros};
pub use starters::starters;
pub use voices::{ChannelAudio, NoteLength, PlayNote, STEAL_FADE, VoiceSettings, global_steals, render_channel};

use bpe_graph::Param;
use std::collections::BTreeMap;

/// Frames in one block.
pub const BLOCK: usize = 64;

/// One block of stereo audio, left then right. Control signals carry the same values on both
/// sides.
pub type Buf = [[f32; BLOCK]; 2];

/// A silent block.
pub const SILENT: Buf = [[0.0; BLOCK]; 2];

/// What a block knows about the note it is playing, as of the start of the current block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ctx {
    /// Frames per second.
    pub rate: f32,
    /// The note's pitch as a MIDI number, after glide.
    pub note: f32,
    /// 0 to 1.
    pub velocity: f32,
    /// Whether the note is still held.
    pub gate: bool,
    /// True on the first block of a note, and on a mono retrigger: envelopes and retriggered
    /// LFOs start again.
    pub trigger: bool,
    /// Seconds since the note started.
    pub since_on: f32,
    /// Seconds since the soundtrack started, for free-running LFOs.
    pub time: f64,
    pub seconds_per_beat: f32,
    /// Seeds noise, so the same note gives the same noise.
    pub seed: u64,
}

impl Default for Ctx {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            note: 60.0,
            velocity: 1.0,
            gate: true,
            trigger: true,
            since_on: 0.0,
            time: 0.0,
            seconds_per_beat: 0.5,
            seed: 1,
        }
    }
}

impl Ctx {
    /// Moves on by one block.
    pub fn advance(&mut self) {
        let seconds = BLOCK as f32 / self.rate;
        self.since_on += seconds;
        self.time += seconds as f64;
        self.trigger = false;
    }
}

/// One DSP block. It reads its knobs once, when it is made.
pub trait Block: Send {
    /// Starts again for a new note.
    fn reset(&mut self, cx: &Ctx);
    /// Fills `outputs` (one buffer per output pin) from `inputs` (one per input pin; `None`
    /// when nothing is wired in, and the block uses its knob or a sensible default).
    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]);
    /// Whether the block has nothing more to give once its inputs fall silent. Envelopes are
    /// busy until their release ends.
    fn idle(&self) -> bool {
        true
    }
}

/// A block's knob values by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Knobs(pub BTreeMap<String, Param>);

impl Knobs {
    pub fn number(&self, name: &str, default: f32) -> f32 {
        self.0.get(name).and_then(Param::number).filter(|v| v.is_finite()).unwrap_or(default)
    }

    pub fn choice<'a>(&'a self, name: &str, default: &'a str) -> &'a str {
        match self.0.get(name) {
            Some(Param::Choice(c)) | Some(Param::Text(c)) => c,
            _ => default,
        }
    }

    pub fn text<'a>(&'a self, name: &str) -> &'a str {
        match self.0.get(name) {
            Some(Param::Text(t)) | Some(Param::Choice(t)) => t,
            _ => "",
        }
    }

    pub fn flag(&self, name: &str, default: bool) -> bool {
        self.0.get(name).and_then(Param::as_bool).unwrap_or(default)
    }

    pub fn with(mut self, name: &str, value: Param) -> Self {
        self.0.insert(name.into(), value);
        self
    }
}

/// Input `i`'s value at frame `s` on the left side, or `default` when it isn't wired.
#[inline]
pub(crate) fn at(inputs: &[Option<&Buf>], i: usize, s: usize, default: f32) -> f32 {
    match inputs.get(i).copied().flatten() {
        Some(b) => b[0][s],
        None => default,
    }
}

/// Input `i`, or `None` when it isn't wired.
#[inline]
pub(crate) fn wired<'a>(inputs: &[Option<&'a Buf>], i: usize) -> Option<&'a Buf> {
    inputs.get(i).copied().flatten()
}

pub(crate) fn midi_to_hz(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// A small fast deterministic random number generator.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // SplitMix64 once, so nearby seeds start far apart.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Self((z ^ (z >> 31)) | 1)
    }

    /// -1 to 1.
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Runs `block` for `blocks` blocks and returns its first output, frame by frame. `step`
    /// sets the context and gives the inputs for each block.
    pub fn run(
        block: &mut dyn Block,
        cx: &mut Ctx,
        blocks: usize,
        mut step: impl FnMut(usize, &mut Ctx) -> Vec<Option<Buf>>,
    ) -> Vec<[f32; 2]> {
        block.reset(cx);
        let mut out = Vec::with_capacity(blocks * BLOCK);
        for b in 0..blocks {
            let ins = step(b, cx);
            let refs: Vec<Option<&Buf>> = ins.iter().map(Option::as_ref).collect();
            let mut outs = [SILENT; 4];
            block.process(cx, &refs, &mut outs);
            out.extend((0..BLOCK).map(|s| [outs[0][0][s], outs[0][1][s]]));
            cx.advance();
        }
        out
    }

    /// A block of a sine at `hz`, starting at frame `first`.
    pub fn sine(hz: f32, rate: f32, first: usize) -> Buf {
        let mut b = SILENT;
        for s in 0..BLOCK {
            let v = (std::f64::consts::TAU * hz as f64 * (first + s) as f64 / rate as f64).sin() as f32;
            b[0][s] = v;
            b[1][s] = v;
        }
        b
    }

    /// A block holding `v`.
    pub fn constant(v: f32) -> Buf {
        [[v; BLOCK]; 2]
    }

    /// The magnitude of `x` at `hz` (Hann window), as a fraction of a full-scale sine.
    pub fn magnitude(x: &[f32], hz: f64, rate: f64) -> f64 {
        let n = x.len() as f64;
        let (mut re, mut im, mut wsum) = (0.0, 0.0, 0.0);
        for (i, &v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
            let a = std::f64::consts::TAU * hz * i as f64 / rate;
            re += v as f64 * w * a.cos();
            im += v as f64 * w * a.sin();
            wsum += w;
        }
        2.0 * (re * re + im * im).sqrt() / wsum
    }
}
