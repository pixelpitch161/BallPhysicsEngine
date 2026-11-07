//! Two-way (Meets things: Two-way on): the particles push back on the balls. What they did in a
//! particle step (`TwoWayOut`) is worked out there, summed per ball in chunk order; the pushes and
//! slowing reach the balls once, at the first substep of the next world step (the thing's
//! `forces`), and stuck particles' weight pulls every substep until the next particle step. Balls
//! rest on piles in each substep's Settle stage (`hold_up`).

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_runtime::World;
use glam::Vec2;

use super::meets::BallGrid;
use super::piles;
use super::state::ParticlesState;

/// How hard a ball must land on a pile to dig into it, in pixels a second.
pub const DIG_SPEED: f32 = 300.0;
/// How much of a ball's speed into a pile digging takes away.
const DIG_SLOWS: f32 = 0.6;
/// How quickly sand rubs off a ball's sideways speed, per second.
const SAND_FRICTION: f32 = 2.0;

/// What a two-way Particles thing does to the balls until its next particle step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TwoWayOut {
    /// The change of speed to give each ball (thing, row) once, in pixels a second.
    pub pushes: BTreeMap<(ThingId, u32), Vec2>,
    /// The pull on each ball from the particles stuck to it, in pixels a second².
    pub weigh: BTreeMap<(ThingId, u32), Vec2>,
}

/// `v` no longer than `most`; zero when not finite.
fn clamp_len(v: Vec2, most: f32) -> Vec2 {
    if !v.is_finite() {
        return Vec2::ZERO;
    }
    let l = v.length();
    if l > most && l > 0.0 { v * (most / l) } else { v }
}

impl TwoWayOut {
    /// From one particle step's sums: the momentum particles gave each ball of `grid` touching
    /// it (`pushes`) and took from it being pushed along (`slows`), and the mass stuck to each
    /// ball (`stuck`). A ball's mass is its radius².
    pub fn of(
        grid: Option<&BallGrid>,
        pushes: &[Vec2],
        slows: &[Vec2],
        stuck: &BTreeMap<(u32, u32), f32>,
        world: &World,
    ) -> TwoWayOut {
        let ceiling = world.limits().ceiling;
        let mut out = TwoWayOut::default();
        if let Some(grid) = grid {
            for (i, b) in grid.balls.iter().enumerate() {
                let mass = (b.radius * b.radius).max(1e-3);
                let push = pushes.get(i).copied().unwrap_or(Vec2::ZERO) / mass;
                let mut slow = slows.get(i).copied().unwrap_or(Vec2::ZERO) / mass;
                // Slowing at most stops the ball.
                let speed = b.velocity.length();
                if let Some(way) = b.velocity.try_normalize() {
                    let along = slow.dot(way);
                    if along < -speed {
                        slow -= way * (along + speed);
                    }
                }
                let dv = clamp_len(clamp_len(push, ceiling) + clamp_len(slow, ceiling), ceiling);
                if dv != Vec2::ZERO {
                    out.pushes.insert((b.thing, b.row), dv);
                }
            }
        }
        for (&(thing, row), &m) in stuck {
            let Some(b) = world.bodies.get(&ThingId(thing)) else { continue };
            let Some(&r) = b.radius.get(row as usize) else { continue };
            let pull = clamp_len(world.gravity * (m / (r * r).max(1e-3)), ceiling * 10.0);
            if pull != Vec2::ZERO {
                out.weigh.insert((ThingId(thing), row), pull);
            }
        }
        out
    }
}

impl TwoWayOut {
    /// Adds `earlier` pushes not yet given to these, each no longer than `ceiling`.
    pub fn carry(&mut self, earlier: BTreeMap<(ThingId, u32), Vec2>, ceiling: f32) {
        for (k, v) in earlier {
            let e = self.pushes.entry(k).or_default();
            *e = clamp_len(*e + v, ceiling);
        }
    }
}

/// Gives the balls what `st` does to them over a substep of `dt`: the pushes once (then they
/// are spent), the weight every time.
pub fn apply(st: &mut ParticlesState, world: &mut World, dt: f32) {
    let ceiling = world.limits().ceiling;
    let pushes = std::mem::take(&mut st.two_way.pushes);
    for ((thing, row), dv) in pushes.into_iter().chain(st.two_way.weigh.iter().map(|(k, a)| (*k, *a * dt))) {
        let Some(b) = world.bodies.get_mut(&thing) else { continue };
        let r = row as usize;
        if r < b.len() && b.alive[r] && !b.held[r] {
            let v = b.velocity[r] + dv;
            if v.is_finite() {
                b.velocity[r] = clamp_len(v, ceiling);
            }
        }
    }
}

/// Balls rest on `st`'s piles: each ball sinking into one is pushed straight up out of it and
/// bounces off with the Bounciness on piles; landing harder than [`DIG_SPEED`] it digs in, and
/// the sand it cuts is thrown up.
pub fn hold_up(st: &mut ParticlesState, world: &mut World, dt: f32) {
    if st.piles.list.is_empty() {
        return;
    }
    let bounce = st.meets.pile_bounce.clamp(0.0, 1.0);
    let mut thrown: Vec<(Vec<(Vec2, f32)>, Vec2)> = Vec::new();
    for b in world.bodies.values_mut() {
        for r in 0..b.len() {
            if !b.alive[r] || b.held[r] {
                continue;
            }
            let (place, radius) = (b.position[r], b.radius[r]);
            let sunk = piles::sinks(&st.piles, place, radius);
            if sunk <= 0.0 {
                continue;
            }
            let v = b.velocity[r];
            if v.y > DIG_SPEED {
                let cut = piles::cut_by_ball(&mut st.piles, place, radius);
                b.velocity[r].y *= 1.0 - DIG_SLOWS;
                thrown.push((cut, super::step::thrown_velocity(v)));
                continue;
            }
            b.position[r].y -= sunk;
            if v.y > 0.0 {
                b.velocity[r].y = -v.y * bounce;
            }
            b.velocity[r].x *= (-SAND_FRICTION * dt).exp();
        }
    }
    let share = st.meets.thrown_up;
    let mut carry = 0.0;
    for (cut, velocity) in thrown {
        super::step::throw_sand(st, world.seed, &cut, share, velocity, &mut carry);
    }
}
