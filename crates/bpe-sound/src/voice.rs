//! What makes sound: a voice source belongs to one thing and renders its sound block by block.

use crate::{BLOCK, BlockHit, TempoMap};
use bpe_core::ThingId;
use bpe_runtime::World;
use std::any::Any;

/// What a voice can read while it renders one block.
pub struct BlockCx<'a> {
    /// The block's first sample, from the start.
    pub start_sample: u64,
    /// The block's start, in seconds.
    pub time: f64,
    pub tempo: &'a TempoMap,
    /// The engine's physics, at or a little past the block's end.
    pub world: &'a World,
}

/// The sound of one thing. Any thing kind can own one (only shapes do, from part 2).
pub trait VoiceSource: Send {
    /// The thing it belongs to.
    fn thing(&self) -> ThingId;
    /// Adds this block's sound into `out_l` and `out_r`. `hits` is every hit in the block, on
    /// any thing; a voice picks out its own. Must not allocate.
    fn on_block(&mut self, cx: &BlockCx, hits: &[BlockHit], out_l: &mut [f32; BLOCK], out_r: &mut [f32; BLOCK]);
    /// Which bus it plays into, 1..=3.
    fn bus(&self) -> u8 {
        1
    }
    /// Silences it, as new (on a seek).
    fn reset(&mut self);
    /// Takes over what `old` (the same thing's voice from before a patch swap) is still
    /// playing, so ringing notes carry on.
    fn adopt(&mut self, _old: &mut dyn VoiceSource) {}
    /// Fades everything out over `ms` (its thing went away); then it stays silent.
    fn fade_out(&mut self, _ms: f32) {}
    /// Whether it is making no sound and will make none (a faded-out voice can then go).
    fn is_silent(&self) -> bool {
        true
    }
    /// Plays `note` from the editor (a key clicked), outside the hits.
    fn audition(&mut self, _note: u8) {}
    /// The loudest sample of the last block, for its meter.
    fn level(&self) -> f32 {
        0.0
    }
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A test sound: a 1 kHz blip at -12 dBFS on each hit on its thing, 2 ms long, or dying away
/// over `decay` seconds (to -120 dB) when that is set.
#[cfg(any(test, feature = "test-tone"))]
pub struct TestTone {
    thing: ThingId,
    decay: f64,
    /// Each sounding blip's age in samples (`u64::MAX` for none).
    notes: [u64; 16],
}

#[cfg(any(test, feature = "test-tone"))]
impl TestTone {
    /// Samples a blip lasts when it has no decay.
    pub const BLIP: u64 = 96;
    pub const LEVEL_DB: f32 = -12.0;

    pub fn new(thing: ThingId, decay: f64) -> Self {
        Self { thing, decay: decay.max(0.0), notes: [u64::MAX; 16] }
    }

    /// The tag that gives a thing a test tone: `"test-tone"`, or `"test-tone-decay=2"`.
    pub fn from_tag(thing: ThingId, tag: &str) -> Option<Self> {
        if tag == "test-tone" {
            return Some(Self::new(thing, 0.0));
        }
        let decay: f64 = tag.strip_prefix("test-tone-decay=")?.parse().ok()?;
        Some(Self::new(thing, decay))
    }

    fn length(&self) -> u64 {
        if self.decay > 0.0 { (self.decay * crate::SAMPLE_RATE as f64) as u64 } else { Self::BLIP }
    }

    fn sample(decay: f64, age: u64) -> f32 {
        let level = bpe_dsp::db_to_gain(Self::LEVEL_DB) as f64;
        let rate = crate::SAMPLE_RATE as f64;
        let wave = (std::f64::consts::TAU * 1_000.0 * (age + 1) as f64 / rate).sin();
        let env = if decay > 0.0 { (-(age as f64) / (decay * rate) * 1e6f64.ln()).exp() } else { 1.0 };
        (level * wave * env) as f32
    }
}

#[cfg(any(test, feature = "test-tone"))]
impl VoiceSource for TestTone {
    fn thing(&self) -> ThingId {
        self.thing
    }

    fn on_block(&mut self, _cx: &BlockCx, hits: &[BlockHit], out_l: &mut [f32; BLOCK], out_r: &mut [f32; BLOCK]) {
        let length = self.length();
        let mine = |h: &&BlockHit| h.event.a.thing == self.thing || h.event.b.is_some_and(|b| b.thing == self.thing);
        let mut starting = hits.iter().filter(mine).peekable();
        for i in 0..BLOCK {
            while starting.next_if(|h| h.offset as usize <= i).is_some() {
                // A free slot, or else the oldest note makes room.
                let k = (0..16).max_by_key(|&k| self.notes[k]).expect("sixteen slots");
                self.notes[k] = 0;
            }
            let mut v = 0.0;
            for n in &mut self.notes {
                if *n != u64::MAX {
                    v += Self::sample(self.decay, *n);
                    *n += 1;
                    if *n >= length {
                        *n = u64::MAX;
                    }
                }
            }
            out_l[i] += v;
            out_r[i] += v;
        }
    }

    fn reset(&mut self) {
        self.notes = [u64::MAX; 16];
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
