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
    /// How hard the hit was, as a share of the layer's Speed its particles fly at (1 when it
    /// doesn't follow the impact).
    pub impact: f32,
    /// A place on the side of the walls the particles start on: the ball's middle, or just off
    /// a shape's edge the way they fly.
    pub from: Vec2,
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

/// How hard a hit at `speed` is, for particles that follow the impact: 0.6 for the softest, 1
/// at [`FULL_HIT_SPEED`], 1.2 at most (as the old Particles card had it); 1 when they don't.
pub fn impact(follow: bool, speed: f32) -> f32 {
    if follow { 0.6 + 0.4 * (speed.max(0.0) / FULL_HIT_SPEED).min(1.5) } else { 1.0 }
}

/// How many particles a hit at `speed` lets off: more for harder hits when they follow the
/// impact, and at least one.
fn per_hit(layer: &ParticlesLayer, speed: f32) -> u32 {
    let count = layer.count.max(0);
    if count == 0 {
        return 0;
    }
    ((count as f32 * impact(layer.follow_impact, speed)).round() as u32).max(1)
}

/// The first step that can still have living particles at `now`, `life` seconds long.
fn first_step(now: f64, life: f32, rate: u32) -> u64 {
    ((now - life.max(0.0) as f64) * rate as f64).floor().max(0.0) as u64
}

/// Where ball `i` was `ago` seconds back: between it and its newest sample, then along its
/// history, through the bounces between samples; where it is without one.
pub fn ball_at(balls: &Balls, i: usize, ago: f32) -> Vec2 {
    let head = balls.bodies.position[i];
    let h = balls.history;
    let Some(newest) = h.newest_step() else { return head };
    // Seconds a step, and the moments (in steps) of the head and of `ago` back.
    let dt = (balls.interval / h.every() as f32).max(1e-9) as f64;
    let head_t = newest as f64 + balls.newest_ago.max(0.0) as f64 / dt;
    h.place(i, head_t - ago.max(0.0) as f64 / dt, head_t, head).unwrap_or(head)
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

/// When each of `balls` (of thing `thing`) was there, in seconds: from the step its history
/// first saw it to the step it popped (infinite while it is alive). A ball that popped before
/// step `since` (no Remove left in the log from then) was never there for what is looked at.
fn lifetimes(thing: ThingId, balls: &Balls, log: &Log, rate: u32, since: u64) -> Vec<(f64, f64)> {
    let bodies = balls.bodies;
    let rate_f = rate.max(1) as f64;
    let mut lives: Vec<(f64, f64)> = (0..bodies.len())
        .map(|i| {
            let came = balls.history.born(i).map_or(0.0, |s| s as f64 / rate_f);
            (came, if bodies.alive[i] { f64::INFINITY } else { f64::NEG_INFINITY })
        })
        .collect();
    for e in log.since(since) {
        if e.kind == EventKind::Remove
            && e.a.thing == thing
            && let Some(life) = lives.get_mut(e.a.item as usize)
            && !bodies.alive[e.a.item as usize]
        {
            life.1 = e.time(rate);
        }
    }
    lives
}

/// The emissions of `layer` worn by the balls `balls` (of thing `thing`, rows that `wears`)
/// whose particles may be alive at `now`, oldest first.
/// Only the newest whose particles add up to `cap` are worked out (see [`capped`]).
#[allow(clippy::too_many_arguments)]
pub fn ball_emissions(
    layer: &ParticlesLayer,
    thing: ThingId,
    balls: &Balls,
    wears: &dyn Fn(usize) -> bool,
    log: &Log,
    rate: u32,
    now: f64,
    cap: usize,
) -> Vec<Emission> {
    let mut out = Vec::new();
    let life = layer.life.max(0.0);
    if life <= 0.0 {
        return out;
    }
    let bodies = balls.bodies;
    // Newest first, until there are enough; turned round at the end.
    let mut total = 0usize;
    match layer.emit {
        Emit::OnHit | Emit::WhenItPops => {
            let hits = layer.emit == Emit::OnHit;
            for e in log.since(first_step(now, life, rate)).iter().rev() {
                if total >= cap {
                    break;
                }
                let time = e.time(rate);
                if time > now {
                    continue;
                }
                let wanted = if hits { EventKind::Hit } else { EventKind::Remove };
                if e.kind != wanted || (hits && e.speed < layer.least_speed) {
                    continue;
                }
                let sides = [(Some(e.a), e.normal), (e.b, -e.normal)];
                for (k, (side, normal)) in sides.into_iter().enumerate().rev() {
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
                    total += count as usize;
                    out.push(Emission {
                        time,
                        row: Some(row),
                        by: None,
                        place: e.point,
                        normal: if hits { normal } else { Vec2::NEG_Y },
                        velocity: ball_velocity(balls, row, ago),
                        count,
                        seed: hash(&[thing.0, side.item, e.step as u32, (e.step >> 32) as u32, k as u32, hits as u32]),
                        impact: if hits { impact(layer.follow_impact, e.speed) } else { 1.0 },
                        from: ball_at(balls, row, ago),
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
            let lives = lifetimes(thing, balls, log, rate, first_step(now, life, rate));
            for k in (first..=last).rev() {
                if total >= cap {
                    break;
                }
                let time = k as f64 / per;
                let ago = (now - time) as f32;
                for i in (0..bodies.len()).rev() {
                    // Only while the ball was there: from when it came to when it popped.
                    let (came, went) = lives[i];
                    if time < came || time > went || !wears(i) {
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
                    let place = ball_at(balls, i, ago);
                    total += 1;
                    out.push(Emission {
                        time,
                        row: Some(i),
                        by: None,
                        place,
                        normal: velocity.try_normalize().unwrap_or(Vec2::NEG_Y),
                        velocity,
                        count: 1,
                        seed: hash(&[thing.0, i as u32, k as u32, (k >> 32) as u32, 7]),
                        impact: 1.0,
                        from: place,
                    });
                }
            }
        }
    }
    out.reverse();
    capped(out, cap)
}

/// The emissions of a shape's `layer` (read as a ball's), the shape `shape` with its `wall`,
/// whose particles may be alive at `now`, oldest first.
/// Only the newest whose particles add up to `cap` are worked out (see [`capped`]).
pub fn shape_emissions(
    layer: &ParticlesLayer,
    shape: ThingId,
    wall: Option<&Wall>,
    log: &Log,
    rate: u32,
    now: f64,
    cap: usize,
) -> Vec<Emission> {
    let mut out = Vec::new();
    let mut total = 0usize;
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
            for k in (first..=last).rev() {
                if total >= cap {
                    break;
                }
                total += 1;
                let seed = hash(&[shape.0, k as u32, (k >> 32) as u32, 11]);
                let u = (seed >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
                let here = wall.outline.point(u);
                // The edge's normal, outward from the middle (the way `point` looks), so the side
                // a seed picks turns with the shape.
                let outward = (here - wall.outline.center()).try_normalize().unwrap_or(Vec2::from_angle(u));
                // A quarter pixel out, so the normal is the edge's whatever the rounding.
                let (_, line) = wall.outline.nearest(here + outward * 0.25);
                // Either side of the edge.
                let line = if seed & 1 == 0 { line } else { -line };
                // Let off from the edge as it was then, not as it has turned and shrunk to since.
                let time = k as f64 / per;
                let (place, normal) = rewound(wall, here, line, (now - time) as f32, rate);
                out.push(Emission {
                    time,
                    row: None,
                    by: None,
                    place,
                    normal,
                    velocity: Vec2::ZERO,
                    count: 1,
                    seed,
                    impact: 1.0,
                    from: place + normal * OFF_THE_EDGE,
                });
            }
        }
        _ => {
            for e in log.since(first_step(now, life, rate)).iter().rev() {
                if total >= cap {
                    break;
                }
                let time = e.time(rate);
                if time > now {
                    continue;
                }
                if e.kind != EventKind::Hit || e.speed < layer.least_speed {
                    continue;
                }
                let Some(b) = e.b.filter(|b| b.thing == shape) else { continue };
                let count = per_hit(layer, e.speed);
                if count == 0 {
                    continue;
                }
                total += count as usize;
                out.push(Emission {
                    time,
                    row: None,
                    by: Some(e.a),
                    place: e.point,
                    normal: e.normal,
                    velocity: Vec2::ZERO,
                    count,
                    seed: hash(&[shape.0, b.item, e.a.thing.0, e.a.item, e.step as u32, (e.step >> 32) as u32]),
                    impact: impact(layer.follow_impact, e.speed),
                    from: e.point + e.normal * OFF_THE_EDGE,
                });
            }
        }
    }
    out.reverse();
    capped(out, cap)
}

/// Where the point `p` on `wall` (as it is now) was `ago` seconds back, and which way the
/// direction `way` there pointed then, undoing the wall's turn at its spin now: a spinning
/// shape's particles stay where they left rather than turn with it. Its shrinking and moving
/// are not undone: the wall only knows how it changes now (its growth stops at its smallest
/// size and a ridden ball turns round on every bounce), so undoing them would make the whole
/// cloud jump the moment they change; the particles move along with the shape instead.
fn rewound(wall: &Wall, p: Vec2, way: Vec2, ago: f32, _rate: u32) -> (Vec2, Vec2) {
    if ago <= 0.0 || wall.spin == 0.0 {
        return (p, way);
    }
    let centre = wall.outline.center();
    let turn = Vec2::from_angle(-wall.spin * ago);
    (centre + turn.rotate(p - centre), turn.rotate(way))
}

/// How far off a shape's edge, the way its particles fly, their side of it is judged.
const OFF_THE_EDGE: f32 = 4.0;

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
            impact: 1.0,
            from: Vec2::ZERO,
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
        assert_eq!(per_hit(&layer, FULL_HIT_SPEED * 3.0), 12, "at most a fifth more");
        assert_eq!(per_hit(&layer, 0.0), 6, "the softest touch still lets some off");
        let one = ParticlesLayer { count: 1, ..layer.clone() };
        assert_eq!(per_hit(&one, 0.0), 1);
        let flat = ParticlesLayer { follow_impact: false, ..layer };
        assert_eq!(per_hit(&flat, 1.0), 10);
        assert_eq!(impact(true, FULL_HIT_SPEED * 0.5), 0.8);
        assert_eq!(impact(false, 0.0), 1.0);
    }

    /// Two balls moving right at 240 px/s from x = 0, the second let go at step `late`, the
    /// first popped at step `popped` (when given), stepped to 1 s at 240 steps a second.
    fn two_balls(late: u64, popped: Option<u64>) -> (bpe_runtime::Bodies, bpe_runtime::History, Log) {
        let mut bodies = bpe_runtime::Bodies::default();
        let mut history = bpe_runtime::History::new(240, bpe_runtime::HistoryWant { seconds: 2.0, forever: false });
        let mut log = Log::default();
        bodies.push(Vec2::ZERO, Vec2::new(240.0, 0.0), 10.0);
        history.note(0, 1);
        for step in 1..=240u64 {
            if step == late {
                bodies.push(Vec2::new(0.0, 100.0), Vec2::new(240.0, 0.0), 10.0);
            }
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v / 240.0;
                }
            }
            if Some(step) == popped {
                bodies.alive[0] = false;
                let a = Party { thing: ThingId(2), item: 0 };
                let point = bodies.position[0];
                log.push(bpe_runtime::Event {
                    step,
                    kind: EventKind::Remove,
                    a,
                    b: None,
                    point,
                    normal: Vec2::ZERO,
                    speed: 0.0,
                    at: 0.0,
                });
            }
            history.record(step, &bodies.position);
        }
        (bodies, history, log)
    }

    fn steady(bodies: &bpe_runtime::Bodies, history: &bpe_runtime::History, log: &Log) -> Vec<Emission> {
        let layer = ParticlesLayer { emit: Emit::Steady, rate: 60.0, life: 2.0, ..ParticlesLayer::default() };
        let colour = |_: usize| [1.0; 4];
        let number = |i: usize| i as u32;
        let turned = |_: usize, _: f32| None;
        let balls = Balls {
            bodies,
            history,
            interval: 1.0 / 60.0,
            newest_ago: 0.0,
            colour: &colour,
            number: &number,
            turned: &turned,
            last_hits: &[],
            own: None,
            gone: &|_| None,
        };
        ball_emissions(&layer, ThingId(2), &balls, &|_| true, log, 240, 1.0, usize::MAX)
    }

    /// A ball let go late lets nothing off from before it was there.
    #[test]
    fn a_late_ball_lets_nothing_off_before_it_came() {
        let (bodies, history, log) = two_balls(120, None);
        let late: Vec<Emission> = steady(&bodies, &history, &log).into_iter().filter(|e| e.row == Some(1)).collect();
        assert!(!late.is_empty());
        assert!(late.iter().all(|e| e.time >= 0.5 - 1e-9), "{:?}", late.iter().map(|e| e.time).collect::<Vec<_>>());
        assert_eq!(late.len(), 31, "one every 60th of a second from 0.5 s to 1 s");
        // Each one is where the ball was then, on its own row.
        for e in &late {
            assert!((e.place.x - (e.time as f32 - 0.5) * 240.0).abs() < 4.5, "{e:?}");
        }
    }

    /// A ball that popped keeps the particles it let off before the pop, and lets off no more.
    #[test]
    fn a_popped_ball_keeps_what_it_let_off_before() {
        let (bodies, history, log) = two_balls(1000, Some(180));
        let first: Vec<Emission> = steady(&bodies, &history, &log).into_iter().filter(|e| e.row == Some(0)).collect();
        assert!(!first.is_empty(), "the particles in the air stay");
        assert!(first.iter().all(|e| e.time <= 0.75 + 1e-9));
        assert_eq!(first.len(), 46, "one every 60th of a second up to 0.75 s");
    }

    /// A steady emission off a turning, shrinking shape stays where it was let off: drawn later,
    /// as the shape has turned on, it starts from the same place.
    #[test]
    fn a_turning_shape_s_steady_particles_stay_where_they_left() {
        let layer = ParticlesLayer { emit: Emit::Steady, rate: 20.0, life: 2.0, ..ParticlesLayer::default() };
        let log = Log::default();
        let shrink: f32 = 0.0;
        let wall = |angle: f32, scale: f32| Wall {
            outline: bpe_runtime::Outline::Ring {
                center: Vec2::new(10.0, -20.0),
                radius: 300.0 * scale,
                sides: 5,
                angle,
                thickness: 2.0,
            },
            solid: false,
            spin: 1.0,
            growth: -shrink,
            velocity: Vec2::ZERO,
        };
        let a = shape_emissions(&layer, ThingId(1), Some(&wall(1.0, 1.0)), &log, 240, 1.0, usize::MAX);
        let later = (1.0 - shrink / 240.0).powi(120);
        let b = shape_emissions(&layer, ThingId(1), Some(&wall(1.5, later)), &log, 240, 1.5, usize::MAX);
        let mut same = 0;
        for e in &a {
            let Some(f) = b.iter().find(|f| f.seed == e.seed) else { continue };
            assert!((e.place - f.place).length() < 0.05, "{:?} then {:?}", e.place, f.place);
            assert!((e.normal - f.normal).length() < 1e-3, "{:?} then {:?}", e.normal, f.normal);
            same += 1;
        }
        assert!(same > 10, "{same}");
    }

    #[test]
    fn a_shape_s_steady_particles_dont_jump_when_its_motion_changes() {
        let layer = ParticlesLayer { emit: Emit::Steady, rate: 20.0, life: 2.0, ..ParticlesLayer::default() };
        let log = Log::default();
        let outline =
            bpe_runtime::Outline::Ring { center: Vec2::ZERO, radius: 300.0, sides: 5, angle: 0.0, thickness: 2.0 };
        // The same shape at the same moment, moving one way, then (after a bounce of the ball
        // it rides, or reaching its smallest size) another: what is in the air stays put.
        let moving =
            Wall { outline: outline.clone(), solid: false, spin: 0.0, growth: -0.2, velocity: Vec2::new(500.0, 0.0) };
        let turned = Wall { growth: 0.0, velocity: Vec2::new(-500.0, 0.0), ..moving.clone() };
        let a = shape_emissions(&layer, ThingId(1), Some(&moving), &log, 240, 1.5, usize::MAX);
        let b = shape_emissions(&layer, ThingId(1), Some(&turned), &log, 240, 1.5, usize::MAX);
        assert!(!a.is_empty());
        for (e, f) in a.iter().zip(&b) {
            assert!((e.place - f.place).length() < 1e-3, "{:?} then {:?}", e.place, f.place);
        }
    }

    #[test]
    fn only_the_newest_emissions_up_to_the_cap_are_worked_out() {
        let outline =
            bpe_runtime::Outline::Box { center: Vec2::ZERO, half: Vec2::splat(100.0), angle: 0.0, thickness: 2.0 };
        let wall = Wall { outline, solid: false, spin: 0.0, growth: 0.0, velocity: Vec2::ZERO };
        let layer = ParticlesLayer { emit: Emit::Steady, rate: 10_000.0, life: 3.0, ..ParticlesLayer::default() };
        let log = Log::default();
        let all = shape_emissions(&layer, ThingId(1), Some(&wall), &log, 240, 5.0, usize::MAX);
        assert_eq!(all.len(), 30_001);
        let kept = shape_emissions(&layer, ThingId(1), Some(&wall), &log, 240, 5.0, 4000);
        assert_eq!(kept.len(), 4000);
        assert_eq!(&kept[..], &all[all.len() - 4000..], "the newest, oldest first");
    }
}
