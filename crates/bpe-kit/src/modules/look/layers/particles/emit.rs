//! When and where particles were let off: every emission whose particles may still be alive
//! at a moment, found again from the log and the balls' history each frame, so nothing is kept
//! between frames and any moment is drawn the same however it was reached.

use bpe_core::ThingId;
use bpe_runtime::{EventKind, Log, Party, Wall};
use glam::Vec2;

use super::{Emit, ParticlesLayer};
use crate::modules::look::layers::trail::draw::Balls;

/// The speed at which a hit is at full strength, for particles that follow the impact.
pub const FULL_HIT_SPEED: f32 = 600.0;

/// The most particles one layer draws in a frame.
pub const MAX_PARTICLES: usize = 4000;

/// One letting-off of particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    /// When, in seconds into the clip.
    pub time: f64,
    /// The ball (body row) that let them off; `None` for a shape.
    pub row: Option<usize>,
    /// For a shape's: the ball that hit it.
    pub by: Option<Party>,
    pub place: Vec2,
    /// Off the bounce (from what was hit towards the ball), or the way it was heading.
    pub normal: Vec2,
    /// How fast the ball was going then.
    pub velocity: Vec2,
    /// How many particles.
    pub count: u32,
    /// Its own number, for every random choice of its particles.
    pub seed: u32,
}

/// A number from `parts`, the same every time.
pub fn hash(parts: &[u32]) -> u32 {
    let mut x: u32 = 0x9e37_79b9;
    for &p in parts {
        x ^= p.wrapping_mul(0x85eb_ca6b);
        x = x.rotate_left(13).wrapping_mul(0xc2b2_ae35);
        x ^= x >> 16;
    }
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x
}

/// How many particles a hit at `speed` lets off.
fn per_hit(layer: &ParticlesLayer, speed: f32) -> u32 {
    let count = layer.count.max(0) as f32;
    let impact = if layer.follow_impact { (speed / FULL_HIT_SPEED).min(1.5) } else { 1.0 };
    (count * impact).round() as u32
}

/// The first step that can still have living particles at `now`, `life` seconds long.
fn first_step(now: f64, life: f32, rate: u32) -> u64 {
    ((now - life.max(0.0) as f64) * rate as f64).floor().max(0.0) as u64
}

/// Where ball `i` was `ago` seconds back: between it and its newest sample, then along its
/// history; where it is without one.
pub fn ball_at(balls: &Balls, i: usize, ago: f32) -> Vec2 {
    let head = balls.bodies.position[i];
    let h = balls.history;
    let Some(newest) = h.points(i).next() else { return head };
    if ago <= balls.newest_ago {
        let f = if balls.newest_ago > 0.0 { ago / balls.newest_ago } else { 0.0 };
        return head.lerp(newest, f.clamp(0.0, 1.0));
    }
    h.at(i, ago - balls.newest_ago, balls.interval).unwrap_or(head)
}

/// How fast ball `i` was going `ago` seconds back, from where it was either side of then.
pub fn ball_velocity(balls: &Balls, i: usize, ago: f32) -> Vec2 {
    if ago <= 0.0 || balls.history.is_empty() {
        return balls.bodies.velocity[i];
    }
    let step = balls.interval.max(1e-3);
    let newer = (ago - step).max(0.0);
    let span = ago + step - newer;
    (ball_at(balls, i, newer) - ball_at(balls, i, ago + step)) / span
}

/// The emissions of `layer` worn by the balls `balls` (of thing `thing`, rows that `wears`)
/// whose particles may be alive at `now`, oldest first.
pub fn ball_emissions(
    layer: &ParticlesLayer,
    thing: ThingId,
    balls: &Balls,
    wears: &dyn Fn(usize) -> bool,
    log: &Log,
    rate: u32,
    now: f64,
) -> Vec<Emission> {
    let mut out = Vec::new();
    let life = layer.life.max(0.0);
    if life <= 0.0 {
        return out;
    }
    let bodies = balls.bodies;
    match layer.emit {
        Emit::OnHit | Emit::WhenItPops => {
            let hits = layer.emit == Emit::OnHit;
            for e in log.since(first_step(now, life, rate)) {
                let time = e.time(rate);
                if time > now {
                    break;
                }
                let wanted = if hits { EventKind::Hit } else { EventKind::Remove };
                if e.kind != wanted || (hits && e.speed < layer.least_speed) {
                    continue;
                }
                let sides = [(Some(e.a), e.normal), (e.b, -e.normal)];
                for (k, (side, normal)) in sides.into_iter().enumerate() {
                    let Some(side) = side.filter(|s| s.thing == thing) else { continue };
                    let row = side.item as usize;
                    if row >= bodies.len() || !wears(row) {
                        continue;
                    }
                    let ago = (now - time) as f32;
                    let count = if hits { per_hit(layer, e.speed) } else { layer.count.max(0) as u32 };
                    if count == 0 {
                        continue;
                    }
                    out.push(Emission {
                        time,
                        row: Some(row),
                        by: None,
                        place: e.point,
                        normal: if hits { normal } else { Vec2::NEG_Y },
                        velocity: ball_velocity(balls, row, ago),
                        count,
                        seed: hash(&[thing.0, side.item, e.step as u32, (e.step >> 32) as u32, k as u32, hits as u32]),
                    });
                }
            }
        }
        Emit::WhileMoving | Emit::OnlyWhenFast | Emit::Steady => {
            let per = layer.rate.max(0.0) as f64;
            if per <= 0.0 {
                return out;
            }
            let first = ((now - life as f64) * per).ceil().max(0.0) as i64;
            let last = (now * per).floor() as i64;
            for k in first..=last {
                let time = k as f64 / per;
                let ago = (now - time) as f32;
                for i in 0..bodies.len() {
                    if !bodies.alive[i] || !wears(i) {
                        continue;
                    }
                    let velocity = ball_velocity(balls, i, ago);
                    let speed = velocity.length();
                    let moving = match layer.emit {
                        Emit::WhileMoving => speed >= 1.0,
                        Emit::OnlyWhenFast => speed >= layer.least_speed.max(1.0),
                        _ => true,
                    };
                    if !moving {
                        continue;
                    }
                    out.push(Emission {
                        time,
                        row: Some(i),
                        by: None,
                        place: ball_at(balls, i, ago),
                        normal: velocity.try_normalize().unwrap_or(Vec2::NEG_Y),
                        velocity,
                        count: 1,
                        seed: hash(&[thing.0, i as u32, k as u32, (k >> 32) as u32, 7]),
                    });
                }
            }
        }
    }
    out
}

/// The emissions of a shape's `layer` (read as a ball's), the shape `shape` with its `wall`,
/// whose particles may be alive at `now`, oldest first.
pub fn shape_emissions(
    layer: &ParticlesLayer,
    shape: ThingId,
    wall: Option<&Wall>,
    log: &Log,
    rate: u32,
    now: f64,
) -> Vec<Emission> {
    let mut out = Vec::new();
    let life = layer.life.max(0.0);
    if life <= 0.0 {
        return out;
    }
    match layer.emit {
        Emit::Steady => {
            let (Some(wall), per) = (wall, layer.rate.max(0.0) as f64) else { return out };
            if per <= 0.0 {
                return out;
            }
            let first = ((now - life as f64) * per).ceil().max(0.0) as i64;
            let last = (now * per).floor() as i64;
            for k in first..=last {
                let seed = hash(&[shape.0, k as u32, (k >> 32) as u32, 11]);
                let u = (seed >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
                let place = wall.outline.point(u);
                let (_, line) = wall.outline.nearest(place + Vec2::from_angle(u) * 1e-3);
                // Either side of the edge.
                let normal = if seed & 1 == 0 { line } else { -line };
                out.push(Emission {
                    time: k as f64 / per,
                    row: None,
                    by: None,
                    place,
                    normal,
                    velocity: Vec2::ZERO,
                    count: 1,
                    seed,
                });
            }
        }
        _ => {
            for e in log.since(first_step(now, life, rate)) {
                let time = e.time(rate);
                if time > now {
                    break;
                }
                if e.kind != EventKind::Hit || e.speed < layer.least_speed {
                    continue;
                }
                let Some(b) = e.b.filter(|b| b.thing == shape) else { continue };
                let count = per_hit(layer, e.speed);
                if count == 0 {
                    continue;
                }
                out.push(Emission {
                    time,
                    row: None,
                    by: Some(e.a),
                    place: e.point,
                    normal: e.normal,
                    velocity: Vec2::ZERO,
                    count,
                    seed: hash(&[shape.0, b.item, e.a.thing.0, e.a.item, e.step as u32, (e.step >> 32) as u32]),
                });
            }
        }
    }
    out
}

/// The newest of `emissions` (oldest first) whose particles add up to at most `cap`, still
/// oldest first; an emission that doesn't fit whole keeps its newest share.
pub fn capped(mut emissions: Vec<Emission>, cap: usize) -> Vec<Emission> {
    let mut left = cap;
    let mut keep = emissions.len();
    for (i, e) in emissions.iter_mut().enumerate().rev() {
        if left == 0 {
            break;
        }
        let n = (e.count as usize).min(left);
        e.count = n as u32;
        left -= n;
        keep = i;
    }
    emissions.drain(..keep);
    emissions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(time: f64, count: u32) -> Emission {
        Emission {
            time,
            row: Some(0),
            by: None,
            place: Vec2::ZERO,
            normal: Vec2::NEG_Y,
            velocity: Vec2::ZERO,
            count,
            seed: 0,
        }
    }

    #[test]
    fn the_cap_keeps_the_newest() {
        let list = vec![at(0.0, 3000), at(0.1, 3000), at(0.2, 500)];
        let kept = capped(list, MAX_PARTICLES);
        assert_eq!(kept.iter().map(|e| e.count).collect::<Vec<_>>(), [500, 3000, 500], "the oldest's newest 500");
        let kept = capped(vec![at(0.0, 3000), at(0.1, 3500), at(0.2, 500)], MAX_PARTICLES);
        assert_eq!(kept.iter().map(|e| e.count).collect::<Vec<_>>(), [3500, 500]);
        assert_eq!(kept[0].time, 0.1);
        let kept = capped(vec![at(0.0, 10), at(0.1, 10)], 15);
        assert_eq!(kept.iter().map(|e| e.count).collect::<Vec<_>>(), [5, 10]);
    }

    #[test]
    fn hashes_differ_by_their_parts() {
        assert_eq!(hash(&[1, 2, 3]), hash(&[1, 2, 3]));
        assert_ne!(hash(&[1, 2, 3]), hash(&[1, 2, 4]));
        assert_ne!(hash(&[1, 2]), hash(&[2, 1]));
    }

    #[test]
    fn impact_scales_a_hit_s_count() {
        let layer = ParticlesLayer { count: 10, ..ParticlesLayer::default() };
        assert_eq!(per_hit(&layer, FULL_HIT_SPEED), 10);
        assert_eq!(per_hit(&layer, FULL_HIT_SPEED * 3.0), 15, "at most half again");
        assert_eq!(per_hit(&layer, FULL_HIT_SPEED / 2.0), 5);
        let flat = ParticlesLayer { follow_impact: false, ..layer };
        assert_eq!(per_hit(&flat, 1.0), 10);
    }
}
