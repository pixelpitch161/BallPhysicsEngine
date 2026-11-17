//! What the sound reads of the liquid (liquid part 5). A Liquid thing's Sound card plays its
//! moments (balls splashing, going in and coming out, which part 2 logs, and the pours and bubbles
//! logged here when its sound asks for them); any sound reads how deep a ball is under a liquid
//! and the colour round it ([`wet_of`]), and a Liquid modulator reads how each liquid moves, mixes,
//! stands and fills ([`stats_of`]). All of it comes back from the card with the feel, in
//! integers, so it is the same on every play. A scene that hears a liquid keeps every Liquid thing
//! in the main world ([`scene_hears_liquid`]), so their events reach the log the sound reads.

use bpe_core::{ResolvedSlot, Scene, Settings, SlotId, ThingId};
use bpe_gfx::liquid_sim::{MOST_LIQUIDS, STAT_WORDS, STATS_LEN, stat_at};
use bpe_runtime::{Event, EventKind, Party, World};
use glam::Vec2;

use super::KIND;
use super::state::{Layout, host_of, state_of};
use crate::sound::{LIQUID_MOD, PitchFrom, VOICE, VoiceSettings};

/// What a Liquid event's party `b` carries as its item, past part 2's Enter, Leave and Splash: a
/// pour pouring, and a bubble from a ball moving under the liquid.
pub const POURING: u32 = 3;
pub const BUBBLE: u32 = 4;

/// The most Pour events a pour logs a second.
pub const POUR_RATE: u32 = 30;
/// Bubbles a second a ball moving at [`BUBBLE_SPEED`] gives off, and the most it gives off.
pub const BUBBLES_A_SECOND: f32 = 8.0;
pub const BUBBLE_SPEED: f32 = 100.0;
pub const MOST_BUBBLES: f32 = 60.0;

/// Which of its own moments a Liquid thing's sound logs (the ones part 2 doesn't log anyway).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LiquidSound {
    pub pours: bool,
    pub bubbles: bool,
}

impl LiquidSound {
    /// From the thing's Sound card, if it is on.
    pub fn read(card: Option<&ResolvedSlot>) -> LiquidSound {
        let Some(card) = card.filter(|c| c.kind == VOICE) else { return LiquidSound::default() };
        let v = VoiceSettings::from_params(&card.params);
        LiquidSound { pours: v.enabled && v.moments_pour, bubbles: v.enabled && v.moments_bubbles }
    }

    /// A number for the fingerprints.
    pub fn code(self) -> u64 {
        u64::from(self.pours) | (u64::from(self.bubbles) << 1)
    }
}

/// Whether some sound in `scene` hears a liquid: a Liquid thing's own sound, a Liquid modulator,
/// or a sound muffled under a liquid or pitched by its depth or colour. All Liquid things then
/// step with the physics (they share one body), so their events and readings are in its world.
pub fn scene_hears_liquid(scene: &Scene) -> bool {
    let mut liquids = scene.things.values().filter(|t| t.kind == KIND && t.enabled).peekable();
    if liquids.peek().is_none() {
        return false;
    }
    scene.things.values().filter(|t| t.enabled).any(|t| {
        let Some(card) = t.slots.get(&SlotId::Sound).filter(|c| c.kind == VOICE) else { return false };
        let v = VoiceSettings::from_params(&card.params);
        v.enabled
            && (t.kind == KIND
                || v.play_under > 0.0
                || matches!(v.play_pitch_from, PitchFrom::Depth | PitchFrom::Colour)
                || card.layers.iter().any(|l| l.enabled && l.kind == LIQUID_MOD))
    })
}

/// A ball in the liquid, as the last feel found it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wet {
    /// Pixels of liquid above its top (0 at or above the surface).
    pub depth: f32,
    /// The colour of the drops touching it (linear rgb).
    pub colour: [f32; 3],
    /// The liquids touching it and each one's share of the contacts, most first (`n` of them).
    pub liquids: [(ThingId, f32); 4],
    pub n: usize,
}

/// How a ball is wet: `None` when no liquid touched it in the last feel (or the world has none).
/// Allocates nothing (the sound reads it as it plays).
pub fn wet_of(world: &World, ball: Party) -> Option<Wet> {
    host_of(world)?.host.as_ref()?.wet.get(&(ball.thing, ball.item)).copied()
}

/// What a liquid (or several together) is doing, from the last feel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stat {
    pub drops: u32,
    /// Their mean speed, pixels a second.
    pub speed: f32,
    /// How high they stand on average: 0 at the bottom of the area, 1 at the top.
    pub height: f32,
    /// Their mean colour (linear rgb).
    pub colour: [f32; 3],
    /// How evenly their colours have blended: 1 one colour all through, 0 two side by side.
    pub mixed: f32,
    /// How much of the liquids there is: drops over the most they keep.
    pub amount: f32,
}

/// The stats of the liquids `pick` takes, together; `None` when the world holds no liquid sums
/// (no card, beside the physics, or nothing back yet). Allocates nothing.
pub fn stats_of(world: &World, pick: impl Fn(ThingId) -> bool) -> Option<Stat> {
    let host = host_of(world)?.host.as_ref()?;
    let sums = host.sums.as_ref()?;
    let mut total = [0u64; STAT_WORDS];
    let mut kept = 0u64;
    for &(l, thing) in host.numbers.iter().filter(|(l, t)| *l < MOST_LIQUIDS && pick(*t)) {
        for (k, t) in total.iter_mut().enumerate() {
            *t += u64::from(sums[l * STAT_WORDS + k]);
        }
        kept += state_of(world, thing).map_or(0, |s| u64::from(s.kept));
    }
    Some(stat_from(&total, kept, &host.layout))
}

/// A [`Stat`] from summed stat words.
fn stat_from(t: &[u64; STAT_WORDS], kept: u64, layout: &Layout) -> Stat {
    let n = t[stat_at::DROPS];
    if n == 0 {
        return Stat { mixed: 1.0, ..Stat::default() };
    }
    let nf = n as f64;
    let mean = |k: usize| t[k] as f64 / nf;
    let colour = [0, 1, 2].map(|c| (mean(stat_at::COLOUR + c) / 63.0) as f32);
    // The largest channel's spread against the most it can be (half of the drops at each end).
    let spread = (0..3)
        .map(|c| {
            let m = mean(stat_at::COLOUR + c);
            (mean(stat_at::SQUARE + c) - m * m).max(0.0) / (31.5 * 31.5)
        })
        .fold(0.0f64, f64::max);
    let row = mean(stat_at::ROW) + 0.5;
    Stat {
        drops: n.min(u64::from(u32::MAX)) as u32,
        speed: (mean(stat_at::SPEED) / 16.0) as f32 * layout.cell,
        height: (1.0 - row / layout.rows.max(1) as f64).clamp(0.0, 1.0) as f32,
        colour,
        mixed: (1.0 - spread).clamp(0.0, 1.0) as f32,
        amount: if kept > 0 { (nf / kept as f64).min(1.0) as f32 } else { 0.0 },
    }
}

/// The stat words, if there are as many as there should be.
pub(crate) fn sums_of(words: &[u32]) -> Option<Vec<u32>> {
    (words.len() >= STATS_LEN).then(|| words[..STATS_LEN].to_vec())
}

/// A Pour event: pour `k` of liquid `liquid` pouring at `point` at `speed` px/s.
pub(crate) fn pour_event(step: u64, liquid: ThingId, k: usize, point: Vec2, v: Vec2) -> Event {
    Event {
        step,
        kind: EventKind::Liquid,
        a: Party { thing: liquid, item: k as u32 },
        b: Some(Party { thing: liquid, item: POURING }),
        point,
        normal: v.normalize_or_zero(),
        speed: v.length(),
        at: 0.0,
    }
}

/// Whether a ball moving at `speed` px/s under a liquid gives off a bubble at world step `step`
/// (`per_second` steps a second): a roll of the shared dice on the step and the ball.
pub(crate) fn bubbles_now(speed: f32, step: u64, per_second: u32, ball: (ThingId, u32), liquid: u8) -> bool {
    if !speed.is_finite() || speed <= 0.0 {
        return false;
    }
    let rate = (speed / BUBBLE_SPEED * BUBBLES_A_SECOND).min(MOST_BUBBLES);
    let chance = rate / per_second.max(1) as f32;
    let roll = bpe_core::dice::hash(&[step as u32, (step >> 32) as u32, ball.0.0, ball.1, u32::from(liquid), 0xB0B]);
    bpe_core::dice::unit24(roll) < chance
}
