//! A Water scene's Forces (Water mode part 4b) and its Drains' whirlpools as the water engine's
//! forces, read from their cards once a frame (`docs/superpowers/plans/water-mode/part4b-forces-and-ferrofluid.md`).
//!
//! Strength 1 is a gravity's pull for a Magnet ([`FORCE_UNIT`]), [`SPIN_SPEED`] round a Whirlpool's
//! core and a [`WIND_SPEED`] wind; a Whirlpool and Wind draw the liquid toward their speed at their
//! rate times the Strength's size up to 1, so Strength 0 does nothing. A Drain whose Whirlpool is
//! above 0 spins the water round it as a whirlpool reaching twice its Radius.

use std::hash::{DefaultHasher, Hash};

use bpe_core::maths::Maths;
use bpe_core::{Resolved, Scene, SlotId, Target, ThingId, Value};
use bpe_water::drops::forces::PSI_SLOPE_MOST;
use bpe_water::{Force, MOST_DRAINS, MOST_FORCES, Push};
use glam::Vec2;

use super::{Keyed, MOST_LIQUIDS};
use crate::modules::water::{DrainCard, DrainKind, ForceCard, ForceKind};
use crate::things::water::{DRAIN, FORCE, LIQUID};

/// Pixels a second squared a Magnet of Strength 1 pulls at most: the Water card's usual gravity.
pub const FORCE_UNIT: f32 = 4000.0;
/// Pixels a second a Wind of Strength 1 blows at.
pub const WIND_SPEED: f32 = 1000.0;
/// How fast (a second) a wind draws a lone drop toward its speed.
pub const WIND_RATE: f32 = 3.0;
/// Pixels a second round a Whirlpool's core at Strength 1 (and a Drain's at Whirlpool 1).
pub const SPIN_SPEED: f32 = 600.0;
/// How fast (a second) a whirlpool draws the liquid toward its spin.
pub const SPIN_RATE: f32 = 4.0;
/// How deep a magnet's valleys between spikes are at Spike size 1 (a share of its reach).
pub const SPIKE_DEPTH: f32 = 0.6;

/// What the stage says about a Heater or a Cooler.
pub const HEAT_LATER: &str =
    "Heaters and coolers arrive with heat, in a later part of Water mode: this one does nothing yet.";
/// What the stage says about a force that names no Liquid the water has.
pub const NO_LIQUID_TO_MOVE: &str = "This force has no Liquid to move: the Liquid it names is turned off or gone, or past the 16 the water tells apart.";
/// What the stage says about a force past the most the water feels at once.
pub const TOO_MANY_FORCES: &str = "The water feels at most 16 forces at once: this one isn't felt.";
/// What the stage says about a spinning Drain past the most forces the water feels at once.
pub const TOO_MANY_SPINS: &str =
    "The water feels at most 16 forces at once: this Drain still drains, but its whirlpool doesn't spin.";

/// One Force, or a Drain's whirlpool, as the water sees it.
#[derive(Clone, Debug)]
pub struct ForceSetup {
    pub id: ThingId,
    /// A Force's kind (a Drain's whirlpool is a Whirlpool).
    pub kind: ForceKind,
    /// Whether it is a Drain's whirlpool (its card is the Drain card).
    pub drain: bool,
    /// Which liquids it moves: bit k is liquid k.
    pub liquids: u32,
    pub(super) card: Keyed,
    place: Vec2,
}

/// The wind's gusts at `time` seconds, -1 to 1: three slow waves.
pub fn gust(time: f64) -> f32 {
    let t = time as f32;
    ((1.3 * t).det_sin() + (2.9 * t + 1.0).det_sin() + (4.7 * t + 2.0).det_sin()) / 3.0
}

/// Force `id`'s card at `time` seconds, if it has one.
pub fn force_card_at(scene: &Scene, resolved: &Resolved, id: ThingId, time: f64) -> Option<ForceCard> {
    Keyed::of(scene, resolved, id, SlotId::Main).map(|k| k.card(time))
}

/// Which of `liquids` (the enabled Liquids, in the scene's order) `target` names, as bits.
pub fn liquid_mask(scene: &Scene, liquids: &[ThingId], target: &Target) -> u32 {
    let all = |pick: &dyn Fn(&ThingId) -> bool| {
        liquids.iter().take(MOST_LIQUIDS).enumerate().filter(|(_, l)| pick(l)).fold(0u32, |m, (k, _)| m | 1 << k)
    };
    match target {
        Target::Thing(id) => all(&|l| l == id),
        Target::Kind(kind) if kind.as_str() == LIQUID => all(&|_| true),
        Target::Anything => all(&|_| true),
        Target::Tag(tag) => all(&|l| scene.things[l].tags.contains(tag)),
        _ => 0,
    }
}

impl ForceSetup {
    /// The scene's enabled Forces and spinning Drains in its order (`on` says which things are), the
    /// most the water feels at once, and what the stage says about the rest.
    pub(super) fn all_of(
        scene: &Scene,
        resolved: &Resolved,
        liquids: &[ThingId],
        notes: &mut Vec<(ThingId, &'static str)>,
    ) -> Vec<ForceSetup> {
        let mut out = Vec::new();
        // (Drains with a card, as the draft counts them: one past the most drains takes nothing and
        // spins nothing.)
        let mut drains = 0;
        for id in scene.order.iter().copied().filter(|id| resolved.things.contains_key(id)) {
            let kind = scene.things[&id].kind.as_str();
            if kind != FORCE && kind != DRAIN {
                continue;
            }
            let Some(card) = Keyed::of(scene, resolved, id, SlotId::Main) else { continue };
            let setup = if kind == FORCE {
                let c: ForceCard = card.card(0.0);
                if matches!(c.kind, ForceKind::Heater | ForceKind::Cooler) {
                    notes.push((id, HEAT_LATER));
                    continue;
                }
                let liquids = liquid_mask(scene, liquids, &c.liquid);
                if liquids == 0 {
                    notes.push((id, NO_LIQUID_TO_MOVE));
                    continue;
                }
                ForceSetup {
                    id,
                    kind: c.kind,
                    drain: false,
                    liquids,
                    card,
                    place: scene.things[&id].transform.position,
                }
            } else {
                drains += 1;
                if drains > MOST_DRAINS {
                    continue;
                }
                // (A Drain spins the water only where its Whirlpool is above 0, at the start or on a key.)
                let c: DrainCard = card.card(0.0);
                let keyed = card.tracks.iter().any(|(k, t)| {
                    k == "whirlpool" && t.keys.iter().any(|key| matches!(key.value, Value::Number(n) if n > 0.0))
                });
                if c.kind != DrainKind::Drain || !(c.whirlpool > 0.0 || keyed) || liquids.is_empty() {
                    continue;
                }
                let liquids = liquid_mask(scene, liquids, &Target::Anything);
                let place = scene.things[&id].transform.position;
                ForceSetup { id, kind: ForceKind::Whirlpool, drain: true, liquids, card, place }
            };
            if out.len() >= MOST_FORCES {
                notes.push((id, if setup.drain { TOO_MANY_SPINS } else { TOO_MANY_FORCES }));
                continue;
            }
            out.push(setup);
        }
        out
    }

    /// Where it is at `time` seconds (world pixels).
    pub fn place_at(&self, _time: f64) -> Vec2 {
        self.place
    }

    /// The engine's force at `time` seconds (`None` for a Drain that isn't spinning then).
    pub fn force_at(&self, time: f64) -> Option<Force> {
        let place = self.place_at(time);
        let liquids = self.liquids;
        if self.drain {
            let c: DrainCard = self.card.card(time);
            let spin = c.whirlpool;
            let push = Push::Whirlpool { speed: spin * SPIN_SPEED, rate: SPIN_RATE * spin.abs().min(1.0) };
            return (spin > 0.0).then_some(Force { push, place, reach: 2.0 * c.radius.max(0.0), liquids });
        }
        let c: ForceCard = self.card.card(time);
        let s = c.strength;
        let push = match c.kind {
            ForceKind::Magnet => Push::Magnet {
                pull: s * FORCE_UNIT / PSI_SLOPE_MOST,
                spikes: c.spikes,
                depth: if c.spikes > 0 { SPIKE_DEPTH * c.spike_size.clamp(0.0, 1.0) } else { 0.0 },
                aim: c.spike_aim.to_radians(),
                spread: c.spike_spread.clamp(0.0, 360.0).to_radians(),
            },
            ForceKind::Whirlpool => Push::Whirlpool { speed: s * SPIN_SPEED, rate: SPIN_RATE * s.abs().min(1.0) },
            ForceKind::Wind => {
                let (sin, cos) = c.aim.to_radians().det_sin_cos();
                let speed = s * WIND_SPEED * (1.0 + c.gusts.clamp(0.0, 1.0) * gust(time));
                Push::Wind { velocity: Vec2::new(cos, sin) * speed, rate: WIND_RATE * s.abs().min(1.0) }
            }
            ForceKind::Heater | ForceKind::Cooler => return None,
        };
        let reach = if c.kind == ForceKind::Wind { 0.0 } else { c.reach.max(0.0) };
        Some(Force { push, place, reach, liquids })
    }

    /// Adds what the physics reads of it to `h` (the setup's print).
    pub(super) fn hash_into(&self, h: &mut DefaultHasher) {
        (self.id, self.drain, self.liquids, self.place.x.to_bits(), self.place.y.to_bits()).hash(h);
        self.card.hash_into(self.keys(), h);
    }

    /// The card settings the physics reads (all of a Force's; a Drain's spin).
    pub(super) fn keys(&self) -> Option<&'static [&'static str]> {
        self.drain.then_some(&["kind", "radius", "whirlpool"])
    }
}

#[cfg(test)]
mod tests;
