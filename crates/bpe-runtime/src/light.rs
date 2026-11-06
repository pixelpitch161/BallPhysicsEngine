//! Light sensing as the runtime sees it (lighting part 7): how lit a thing or a ball is, read
//! through [`LightAt`], a CPU light probe the kit provides from the lights and the world. It is
//! optional and one-way: a world has one only when something physical follows light (Follow light
//! on a physics setting, a Motion or Bounce card spread by Light or Shadow, Moths), and nothing the
//! probe or the renderer reads ever depends on it.

use std::sync::Arc;

use bpe_core::{LightLevels, LightSource, Resolved, Scene, ThingId};
use glam::Vec2;

use crate::{Log, World};

/// How a Motion card's balls are drawn to (or pushed from) the lights: Moths and light pressure.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LightPull {
    /// Pixels per second squared at a light amount of 1, towards the light (below 0: away).
    pub strength: f32,
    /// Closer than about this the pull eases off (`d² / (d² + near²)`), so moths circle a lamp.
    pub near: f32,
    /// Whether a lamp hidden by a shadow pulls only as much as its light gets through.
    pub shadows: bool,
}

/// One ball's light: its level (see [`level_of`]) and how much of its light its shadows take,
/// both 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallLight {
    pub light: f32,
    pub shadow: f32,
}

/// A scene's light, worked out from its lights and the world at a moment.
pub trait LightAt: Send + Sync {
    /// How far `source` of thing `thing` is up at `time` (0..1), with the world as it is at
    /// that time and the log up to it.
    fn level(&self, world: &World, log: &Log, thing: ThingId, source: &LightSource, time: f64) -> f32;
    /// Body row `row` of `thing`: its light and shadow.
    fn ball(&self, world: &World, log: &Log, thing: ThingId, row: usize, time: f64) -> BallLight;
    /// The pull on body row `row` of `thing` from every lamp, in pixels per second squared.
    fn pull(&self, world: &World, log: &Log, thing: ThingId, row: usize, time: f64, pull: &LightPull) -> Vec2;
}

/// Makes a scene's [`LightAt`]; the kit's sims carry one, so every world built from them (the
/// editor's, the export's and the sound engine's) senses light the same way.
pub trait LightMaker: Send + Sync {
    fn make(&self, scene: &Scene, resolved: &Resolved) -> Arc<dyn LightAt>;
    /// A hash of everything in `scene` its [`LightAt`] reads (and nothing else), so physics that
    /// follows light restarts the run on those edits only.
    fn fingerprint(&self, scene: &Scene) -> u64;
}

/// A [`LightAt`] read at one world and log, as the pulses read it.
pub struct LightLevelsAt<'a> {
    pub light: &'a dyn LightAt,
    pub world: &'a World,
    pub log: &'a Log,
}

impl LightLevels for LightLevelsAt<'_> {
    fn level(&self, thing: ThingId, source: &LightSource, time: f64) -> f32 {
        self.light.level(self.world, self.log, thing, source, time)
    }
}

/// A light amount (the light's brightness, 1 = the picture as if unlit) as a level 0 to 1:
/// `1 − e^(−amount)`, so 0 in the dark, about 0.63 at 1 and 0.95 at 3, never stuck at the top
/// under bright lamps. Negative light (darkness) reads 0.
pub fn level_of(amount: f32) -> f32 {
    if amount.is_finite() && amount > 0.0 { 1.0 - (-amount).exp() } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_amount_reads_as_a_level() {
        assert_eq!(level_of(0.0), 0.0);
        assert_eq!(level_of(-2.0), 0.0);
        assert_eq!(level_of(f32::NAN), 0.0);
        assert!((level_of(1.0) - 0.632).abs() < 1e-3);
        assert!(level_of(3.0) > 0.95 && level_of(3.0) < 1.0);
    }
}
