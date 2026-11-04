//! The Distance modulator: how near the balls are to the shape, from its middle or its edge.
//! 1 at Near or closer, 0 at Far or beyond, a smooth S between (bent by Curve). Read from the
//! physics once a block (the world is at most a step ahead) and smoothed.

use super::{BLOCK_S, ModCx, Modulator, NoteCx, bend};
use bpe_core::{Target, ThingId};
use bpe_dsp::lfo::smooth_coefficient;
use bpe_kit::sound::{DistanceSettings, Follows, Measure};
use bpe_runtime::{Party, World};
use glam::Vec2;

/// A Distance's settings for one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceParams {
    pub measure: Measure,
    pub near: f32,
    pub far: f32,
    /// -1..1.
    pub curve: f32,
    pub smooth_ms: f32,
    pub invert: bool,
    pub follows: Follows,
    pub full_at: f32,
}

impl Default for DistanceParams {
    fn default() -> Self {
        Self::of(&DistanceSettings::default())
    }
}

impl DistanceParams {
    pub fn of(s: &DistanceSettings) -> Self {
        DistanceParams {
            measure: s.measure,
            near: s.near,
            far: s.far,
            curve: s.curve / 100.0,
            smooth_ms: s.smooth,
            invert: s.invert,
            follows: s.follows,
            full_at: s.full_at as f32,
        }
    }
}

/// How far `p` is from shape `shape` in `world`, measured as `measure` says (from the middle,
/// or the nearest point of the outline's line, inside or out). `None` without the shape.
pub fn distance_to(world: &World, shape: ThingId, measure: Measure, p: Vec2) -> Option<f32> {
    let wall = world.walls.get(&shape);
    match measure {
        Measure::Centre => {
            let c = match wall {
                Some(w) => w.outline.center(),
                None => world.poses.get(&shape)?.position,
            };
            Some((p - c).length())
        }
        Measure::Edge => {
            let w = wall?;
            Some((p - w.outline.nearest(p).0).length())
        }
    }
}

/// 1 at `near` or closer, 0 at `far` or beyond, a smooth S between.
pub fn closeness(d: f32, near: f32, far: f32) -> f32 {
    if d <= near {
        return 1.0;
    }
    if d >= far {
        return 0.0;
    }
    let t = (d - near) / (far - near);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// The Distance's value now, before smoothing: `None` when it follows one ball that is gone
/// (the value then holds). `ball` is the note's own ball for Each note's own ball.
pub fn distance_value(
    p: &DistanceParams,
    heard_from: &Target,
    world: &World,
    shape: ThingId,
    ball: Option<Party>,
) -> Option<f32> {
    let measure = |q: Vec2| distance_to(world, shape, p.measure, q);
    let raw = match p.follows {
        Follows::EachNotesBall => {
            let ball = ball?;
            let b = world.bodies.get(&ball.thing)?;
            let i = ball.item as usize;
            if i >= b.position.len() || !b.alive.get(i).copied().unwrap_or(false) {
                return None;
            }
            closeness(measure(b.position[i])?, p.near, p.far)
        }
        follows => {
            let (mut nearest, mut sum, mut count, mut in_range) = (f32::INFINITY, 0.0f64, 0usize, 0usize);
            for (thing, b) in &world.bodies {
                if !world.names(heard_from, shape, *thing) {
                    continue;
                }
                for (i, q) in b.position.iter().enumerate() {
                    if !b.alive.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    let Some(d) = measure(*q) else { continue };
                    nearest = nearest.min(d);
                    sum += d as f64;
                    count += 1;
                    if d <= p.far {
                        in_range += 1;
                    }
                }
            }
            match follows {
                Follows::HowMany => (in_range as f32 / p.full_at.max(1.0)).min(1.0),
                Follows::Average if count > 0 => closeness((sum / count as f64) as f32, p.near, p.far),
                Follows::Average => 0.0,
                _ => closeness(nearest, p.near, p.far),
            }
        }
    };
    let v = bend(raw, p.curve);
    Some(if p.invert { 1.0 - v } else { v })
}

/// One Distance (shared by the shape, or one per note for Each note's own ball).
#[derive(Clone, Debug)]
pub struct DistanceMod {
    p: DistanceParams,
    heard_from: Target,
    ball: Option<Party>,
    value: f32,
    fresh: bool,
}

impl DistanceMod {
    /// Allocates nothing later; `heard_from` is cloned here, off the audio path.
    pub fn new(heard_from: Target) -> Self {
        DistanceMod { p: DistanceParams::default(), heard_from, ball: None, value: 0.0, fresh: true }
    }

    pub fn set_params(&mut self, p: DistanceParams) {
        self.p = p;
    }

    pub fn params(&self) -> &DistanceParams {
        &self.p
    }
}

impl Modulator for DistanceMod {
    fn value(&self) -> f32 {
        self.value
    }

    fn next_block(&mut self, cx: &ModCx) {
        let Some(raw) = distance_value(&self.p, &self.heard_from, cx.world, cx.shape, self.ball) else {
            // Its ball has gone: hold.
            self.fresh = false;
            return;
        };
        if std::mem::take(&mut self.fresh) {
            self.value = raw;
        } else {
            let c = smooth_coefficient(self.p.smooth_ms.max(0.0) as f64 / 1000.0, BLOCK_S);
            self.value += (raw - self.value) * c;
        }
    }

    fn note_on(&mut self, cx: &NoteCx) {
        if self.p.follows == Follows::EachNotesBall && cx.ball != self.ball {
            self.ball = cx.ball;
            self.fresh = true;
        }
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.value = 0.0;
        self.ball = None;
    }
}
