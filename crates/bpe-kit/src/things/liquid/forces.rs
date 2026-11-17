//! The liquid's forces (liquid part 4): every Liquid thing's Heat, Drain, Current and Magnet
//! layers, and the walls that paint it, as the card's liquid step reads them (in grid units).

use bpe_core::ThingId;
use bpe_gfx::liquid_sim::{
    FORCE_DRAIN, FORCE_HEAT, FORCE_MAGNET, FORCE_PUMP, FORCE_WHIRLPOOL, FORCE_WIND, ForceGpu, MOST_PAINTING_WALLS,
    StepGpu,
};
use bpe_runtime::World;
use glam::Vec2;

use super::cards::{CurrentKind, MagnetAt};
use super::state::{Layout, LiquidState};

/// How hard a pump of Strength 1 pushes, in pixels a second squared.
pub const PUMP: f32 = 400.0;
/// A whirlpool's spin, and a magnet's pull, at Strength 1 (pixels a second squared).
pub const SPIN: f32 = 300.0;
pub const PULL: f32 = 300.0;
/// A drain's pull at its mouth (pixels a second squared), out to this many of its radii.
pub const DRAIN_PULL: f32 = 480.0;
pub const DRAIN_REACH: f32 = 6.0;
/// Wind of Strength 1 (pixels a second squared, on the surface).
pub const WIND: f32 = 300.0;

/// Adds every force of the liquids `list` to `step`; those past the table's room are left out.
pub fn add_all(step: &mut StepGpu, world: &World, layout: &Layout, list: &[&LiquidState]) {
    let cell = layout.cell.max(1e-3);
    for st in list {
        let Some(k) = st.index else { continue };
        let mask = 1u32 << k;
        let anchor = world.anchor(st.id);
        let turn = Vec2::from_angle(anchor.angle);
        let at = |p: Vec2| layout.to_grid(anchor.position + turn.rotate(p));
        let way = |deg: f32| Vec2::from_angle(deg.to_radians() + anchor.angle);
        let mut add = |f: ForceGpu| {
            if f.a.iter().chain(&f.b).all(|x| x.is_finite()) {
                step.add_force(f);
            }
        };
        let f = &st.forces;
        for h in f.heats.iter().filter(|h| h.on && h.strength != 0.0) {
            let p = at(h.place);
            add(ForceGpu { a: [p.x, p.y, h.radius / cell, h.strength], b: [0.0; 4], c: [FORCE_HEAT, mask, 0, 0] });
        }
        for d in f.drains.iter().filter(|d| d.on) {
            let p = at(d.place);
            let r = d.radius / cell;
            let (pull, spin) = (DRAIN_PULL / cell, DRAIN_PULL / cell * d.whirlpool);
            add(ForceGpu {
                a: [p.x, p.y, r * DRAIN_REACH, pull],
                b: [0.0, 0.0, spin, r],
                c: [FORCE_DRAIN, mask, 0, 0],
            });
        }
        for c in f.currents.iter().filter(|c| c.on && c.strength != 0.0) {
            let p = at(c.place);
            let w = way(c.aim);
            let f = match c.kind {
                CurrentKind::Pump => ForceGpu {
                    a: [p.x, p.y, 0.0, PUMP * c.strength / cell],
                    b: [w.x, w.y, c.length / cell, c.width * 0.5 / cell],
                    c: [FORCE_PUMP, mask, 0, 0],
                },
                CurrentKind::Whirlpool => ForceGpu {
                    a: [p.x, p.y, c.length * 0.5 / cell, 0.15 * SPIN * c.strength.abs() / cell],
                    b: [0.0, 0.0, SPIN * c.strength / cell, 0.0],
                    c: [FORCE_WHIRLPOOL, mask, 0, 0],
                },
                CurrentKind::Wind => ForceGpu {
                    a: [0.0, 0.0, 0.0, WIND * c.strength / cell],
                    b: [w.x, w.y, 0.0, 0.0],
                    c: [FORCE_WIND, mask, 0, 0],
                },
            };
            add(f);
        }
        for m in f.magnets.iter().filter(|m| m.on && m.strength != 0.0) {
            let one = |p: Vec2, extra: f32| ForceGpu {
                a: [p.x, p.y, (m.reach + extra) / cell, PULL * m.strength / cell],
                b: [0.0, 0.0, if m.spikes > 0 { m.spike_size } else { 0.0 }, 0.0],
                c: [FORCE_MAGNET, mask, m.spikes, 0],
            };
            match m.at {
                MagnetAt::ItsPlace => add(one(at(m.place), 0.0)),
                MagnetAt::Balls => {
                    for (&thing, b) in &world.bodies {
                        if !world.names(&m.balls, st.id, thing) {
                            continue;
                        }
                        for r in b.live_rows().filter(|&r| b.alive[r]) {
                            add(one(layout.to_grid(b.position[r]), b.radius[r]));
                        }
                    }
                }
            }
        }
    }
}

/// Sets which liquids each wall (numbered in `walls` order, as the band numbers them) paints.
pub fn set_paints<'a>(
    step: &mut StepGpu,
    world: &World,
    walls: impl Iterator<Item = &'a ThingId>,
    list: &[&LiquidState],
) {
    if list.iter().all(|st| st.meets.paint <= 0.0) {
        return;
    }
    for (number, &wall) in walls.enumerate().take(MOST_PAINTING_WALLS) {
        let mask = list
            .iter()
            .filter(|st| st.meets.paint > 0.0 && world.names(&st.meets.painting, st.id, wall))
            .filter_map(|st| st.index)
            .fold(0u32, |m, k| m | (1 << k));
        step.set_paints(number, mask);
    }
}
