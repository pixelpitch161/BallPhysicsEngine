//! Wet balls (liquid part 3): how wet each ball is, from the event log alone (so a seek, a second
//! play and an export agree), and the pieces that draw a wet ball's sheen, tint and drip. A ball
//! is wet through while it is in the liquid and dries after it comes out. The events are logged
//! only by a liquid in the main world (one that pushes balls), so only such a liquid wets balls.

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_gfx::{Frame, Particle};
use bpe_runtime::{EventKind, Log, World};
use glam::Vec2;

use super::feel::{ENTER, LEAVE};
use super::look::WetBallsLayer;

/// Below this a ball counts as dry.
const DRY: f32 = 0.01;

/// How wet each ball (by thing and body row) is at `time` (seconds, the log's clock at `rate`
/// steps a second) from Liquid thing `liquid`'s In and Out events: 1 while in it, then
/// `exp(−t / dries)` since it came out. Dry balls are left out.
pub fn wetness(log: &Log, rate: u32, time: f64, liquid: ThingId, dries: f32) -> BTreeMap<(ThingId, u32), f32> {
    // The last In or Out of each ball up to `time`: (in, when).
    let mut last: BTreeMap<(ThingId, u32), (bool, f64)> = BTreeMap::new();
    for e in log.since(0) {
        if e.kind != EventKind::Liquid {
            continue;
        }
        let Some(b) = e.b.filter(|b| b.thing == liquid) else { continue };
        let at = e.time(rate);
        if at > time {
            continue;
        }
        match b.item {
            ENTER => {
                last.insert((e.a.thing, e.a.item), (true, at));
            }
            LEAVE => {
                last.insert((e.a.thing, e.a.item), (false, at));
            }
            _ => {}
        }
    }
    let dries = dries.max(0.05) as f64;
    last.into_iter()
        .filter_map(|(ball, (inside, at))| {
            let wet = if inside { 1.0 } else { (-(time - at).max(0.0) / dries).exp() as f32 };
            (wet >= DRY).then_some((ball, wet))
        })
        .collect()
}

/// Draws each wet ball of `world` in front of the balls: a tint of `colour` (linear) over it, a
/// highlight up and to the left and a drip hanging under it, as `layer` says, each by how wet it
/// is.
pub fn draw_wet_balls(
    frame: &mut Frame,
    world: &World,
    wet: &BTreeMap<(ThingId, u32), f32>,
    colour: [f32; 3],
    layer: &WetBallsLayer,
) {
    for (&(thing, row), &k) in wet {
        let Some(bodies) = world.bodies.get(&thing) else { continue };
        let r = row as usize;
        if r >= bodies.len() || !bodies.alive[r] {
            continue;
        }
        let (at, radius) = (bodies.position[r], bodies.radius[r]);
        if !at.is_finite() || radius.is_nan() || radius <= 0.0 {
            continue;
        }
        let tint = layer.tint * k * 0.5;
        if tint > 0.0 {
            let mut p = Particle::new(6, at, 0.0, radius, [colour[0], colour[1], colour[2], tint.min(1.0)]);
            p.style[2] = 0.15;
            frame.particles_front.push(p);
        }
        let shine = layer.shine * k;
        if shine > 0.0 {
            let spot = at + Vec2::new(-0.38, -0.38) * radius;
            let mut p = Particle::new(6, spot, 0.0, radius * 0.28, [1.0, 1.0, 1.0, -(shine * 0.8).min(2.0)]);
            p.style[2] = 0.8;
            frame.particles_front.push(p);
        }
        let drip = layer.drips * k;
        if drip > 0.0 {
            let size = radius * 0.16 * drip.min(2.0).sqrt();
            let hang = at + Vec2::new(0.0, radius + size * 0.6);
            let c = [colour[0], colour[1], colour[2], (0.9 * k).min(1.0)];
            let mut p = Particle::new(6, hang, 0.0, size, c);
            p.style[2] = 0.3;
            frame.particles_front.push(p);
        }
    }
}
