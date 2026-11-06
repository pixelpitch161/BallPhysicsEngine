//! Where and when a Particles thing's particles are born, and what each starts as. Every number
//! a birth uses comes from its own dice (`hash` of its seed and a fixed number per use), its seed
//! from the scene's seed, the thing and its birth number: never a shared random stream, so adding
//! a thing or a particle never changes another's.

use bpe_core::ThingId;
use bpe_runtime::{EventKind, Log, Outline, World};
use glam::Vec2;

use super::state::{ParticlesState, Speck};
use super::{AreaShape, BornFrom, Edge, Emit, Heading, ParticlesSettings, RidesOn};
use crate::modules::look::layers::particles::emit::{hash, impact};

/// The most particles one moment (a hit, a burst) lets off.
pub const MOST_AT_ONCE: u32 = 100_000;

/// A place a particle can be born at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub at: Vec2,
    /// How fast what it comes from is moving (a ball it rides, the ball that hit).
    pub velocity: Vec2,
    /// The way out of what it comes from (away from a hit, from the middle ridden), when known.
    pub out: Option<Vec2>,
    /// The parent ball it comes from (body row + 1), 0 when none.
    pub ball: u32,
    /// How much harder than usual it flies (a hard hit).
    pub impact: f32,
}

/// A number from 0 to 1 from `seed` and `k`.
pub fn unit(seed: u32, k: u32) -> f32 {
    (hash(&[seed, k]) >> 8) as f32 / (1u32 << 24) as f32
}

/// A number from −1 to 1 from `seed` and `k`.
pub fn signed(seed: u32, k: u32) -> f32 {
    unit(seed, k) * 2.0 - 1.0
}

/// What births read of the world at a moment.
pub struct BirthCx<'a> {
    pub world: &'a World,
    pub log: &'a Log,
    /// Where the thing itself is, and its turn (radians).
    pub anchor: Vec2,
    pub angle: f32,
}

/// The middle and half size of what the camera sees at the moment, `margin` added round it.
pub fn screen(world: &World, margin: f32) -> (Vec2, Vec2) {
    let half = world.camera.shot_bounds_half(world.video) + Vec2::splat(margin.max(-1e6));
    (world.camera.center, half.max(Vec2::ZERO))
}

/// The points round `outline`, evenly spread by length.
fn outline_points(outline: &Outline) -> Vec<Vec2> {
    crate::things::light::draw::outline_points(outline)
}

/// A point `t` (0 to 1) of the way round the closed polygon `points`, by length, and the way out
/// of it there (away from its middle).
fn round_points(points: &[Vec2], t: f32) -> Option<(Vec2, Vec2)> {
    let n = points.len();
    if n == 0 {
        return None;
    }
    let len = |i: usize| points[i].distance(points[(i + 1) % n]);
    let total: f32 = (0..n).map(len).sum();
    let mut left = t.clamp(0.0, 1.0) * total;
    let middle = points.iter().copied().sum::<Vec2>() / n as f32;
    for i in 0..n {
        let l = len(i);
        if left <= l || i == n - 1 {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let at = a.lerp(b, if l > 0.0 { (left / l).clamp(0.0, 1.0) } else { 0.0 });
            return Some((at, (at - middle).try_normalize().unwrap_or(Vec2::NEG_Y)));
        }
        left -= l;
    }
    None
}

/// Whether `p` is inside the closed polygon `points` (even-odd).
fn inside(points: &[Vec2], p: Vec2) -> bool {
    let n = points.len();
    let mut on = false;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            on = !on;
        }
    }
    on
}

/// The alive balls of `parent` as `(row, place, velocity, radius)`.
fn balls_of(world: &World, parent: ThingId) -> Vec<(usize, Vec2, Vec2, f32)> {
    let Some(b) = world.bodies.get(&parent) else { return Vec::new() };
    (0..b.len()).filter(|&r| b.alive[r]).map(|r| (r, b.position[r], b.velocity[r], b.radius[r])).collect()
}

/// How many places a riding source has right now (balls ridden one by one; 1 otherwise), for
/// Steady's rate a ball.
pub fn riding_count(s: &ParticlesSettings, world: &World, parent: Option<ThingId>) -> usize {
    if s.from != BornFrom::RidingItsParent
        || !matches!(s.rides_on, RidesOn::EachBall | RidesOn::TheEdge | RidesOn::Inside)
    {
        return 1;
    }
    match parent.and_then(|p| world.bodies.get(&p)) {
        Some(b) => b.alive.iter().filter(|a| **a).count(),
        None => 1,
    }
}

/// Where a birth with dice `seed` is, from the source (not a hit). `None` when there is nowhere
/// (a riding source whose parent has no balls left).
pub fn place_for(s: &ParticlesSettings, cx: &BirthCx, parent: Option<ThingId>, seed: u32) -> Option<Place> {
    let (u1, u2, u3) = (unit(seed, 1), unit(seed, 2), unit(seed, 3));
    let turn = Vec2::from_angle(cx.angle + s.line_turn.to_radians());
    let here = |at: Vec2| Place { at, out: (at - cx.anchor).try_normalize(), impact: 1.0, ..Place::default() };
    Some(match s.from {
        BornFrom::WholeScreen => {
            let (c, h) = screen(cx.world, s.margin);
            let (x, y) = (c.x + (u1 * 2.0 - 1.0) * h.x, c.y + (u2 * 2.0 - 1.0) * h.y);
            let at = match s.edge {
                Edge::Everywhere => Vec2::new(x, y),
                Edge::Top => Vec2::new(x, c.y - h.y),
                Edge::Bottom => Vec2::new(x, c.y + h.y),
                Edge::Left => Vec2::new(c.x - h.x, y),
                Edge::Right => Vec2::new(c.x + h.x, y),
            };
            Place { at, impact: 1.0, ..Place::default() }
        }
        BornFrom::APoint => here(cx.anchor),
        BornFrom::ALine => here(cx.anchor + turn * (u1 - 0.5) * s.length.max(0.0)),
        BornFrom::AnArea => {
            let local = match s.area {
                AreaShape::Rectangle => Vec2::new((u1 - 0.5) * s.width.max(0.0), (u2 - 0.5) * s.height.max(0.0)),
                AreaShape::Circle => Vec2::from_angle(u1 * std::f32::consts::TAU) * s.radius.max(0.0) * u2.sqrt(),
                AreaShape::Ring => {
                    let (r0, r1) = (s.inner.max(0.0).min(s.radius.max(0.0)), s.radius.max(0.0));
                    let r = (u2 * (r1 * r1 - r0 * r0) + r0 * r0).sqrt();
                    Vec2::from_angle(u1 * std::f32::consts::TAU) * r
                }
            };
            here(cx.anchor + turn.rotate(local))
        }
        BornFrom::RidingItsParent => {
            let Some(parent) = parent.filter(|p| *p != ThingId::ROOT) else { return Some(here(cx.anchor)) };
            let balls = balls_of(cx.world, parent);
            if cx.world.bodies.contains_key(&parent) {
                if balls.is_empty() {
                    return None;
                }
                let middle = || {
                    let n = balls.len() as f32;
                    let at = balls.iter().map(|b| b.1).sum::<Vec2>() / n;
                    let v = balls.iter().map(|b| b.2).sum::<Vec2>() / n;
                    Place { at, velocity: v, impact: 1.0, ..Place::default() }
                };
                let pick = balls[(hash(&[seed, 7]) as usize) % balls.len()];
                let on = |b: (usize, Vec2, Vec2, f32), at: Vec2, out: Option<Vec2>| Place {
                    at,
                    velocity: b.2,
                    out,
                    ball: b.0 as u32 + 1,
                    impact: 1.0,
                };
                return Some(match s.rides_on {
                    RidesOn::EachBall => on(pick, pick.1, None),
                    RidesOn::TheMiddle => middle(),
                    RidesOn::BallNumber => {
                        let bodies = &cx.world.bodies[&parent];
                        let row = bodies.row_of(s.ball_number.saturating_sub(1)).filter(|&r| bodies.alive[r])?;
                        let b = balls.iter().copied().find(|b| b.0 == row)?;
                        on(b, b.1, None)
                    }
                    RidesOn::TheEdge => {
                        let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                        on(pick, pick.1 + out * pick.3, Some(out))
                    }
                    RidesOn::Inside => {
                        let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                        on(pick, pick.1 + out * pick.3 * u2.sqrt(), Some(out))
                    }
                });
            }
            if let Some(wall) = cx.world.walls.get(&parent) {
                let centre = wall.outline.center();
                let velocity = wall.velocity;
                let points = outline_points(&wall.outline);
                let place = |at: Vec2, out: Option<Vec2>| Place { at, velocity, out, impact: 1.0, ..Place::default() };
                return Some(match s.rides_on {
                    RidesOn::TheEdge => match round_points(&points, u1) {
                        Some((at, out)) => place(at, Some(out)),
                        None => place(centre, None),
                    },
                    RidesOn::Inside => {
                        let (lo, hi) = points
                            .iter()
                            .fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
                                (lo.min(*p), hi.max(*p))
                            });
                        let tried = (0..8).find_map(|k| {
                            let p = lo + (hi - lo) * Vec2::new(unit(seed, 20 + 2 * k), unit(seed, 21 + 2 * k));
                            inside(&points, p).then_some(p)
                        });
                        let at = tried.unwrap_or(centre);
                        place(at, (at - centre).try_normalize())
                    }
                    _ => place(centre, None),
                });
            }
            // A parent the world doesn't move (a folder): its own place.
            let _ = u3;
            here(cx.anchor)
        }
    })
}

/// The hits since the last particle step that let particles off: their places.
pub fn hit_places(s: &ParticlesSettings, st: &ParticlesState, cx: &BirthCx, since: u64) -> Vec<(Place, u32)> {
    let world = cx.world;
    let events = cx.log.events();
    let from = events.partition_point(|e| e.step <= since);
    let mut out = Vec::new();
    for e in &events[from..] {
        if e.kind != EventKind::Hit || e.speed < s.least_speed {
            continue;
        }
        // The ball that hit: the mover, or the other ball of a pair when only it is named.
        let named = |thing: ThingId| world.names(&s.who_hits, st.id, thing) && world.bodies.contains_key(&thing);
        let who = if named(e.a.thing) {
            e.a
        } else if let Some(b) = e.b.filter(|b| named(b.thing)) {
            b
        } else {
            continue;
        };
        let velocity =
            world.bodies.get(&who.thing).and_then(|b| b.velocity.get(who.item as usize)).copied().unwrap_or(Vec2::ZERO);
        let strength = impact(s.follow_impact, e.speed);
        let count = if s.count == 0 { 0 } else { ((s.count as f32 * strength).round() as u32).max(1) };
        let ball = if Some(who.thing) == st.parent { who.item + 1 } else { 0 };
        let out_way = if who == e.a { e.normal } else { -e.normal };
        let place = Place { at: e.point, velocity, out: out_way.try_normalize(), ball, impact: strength };
        out.push((place, count.min(MOST_AT_ONCE)));
    }
    out
}

/// A new particle at `place`, with dice `seed`, born on particle step `now`.
pub fn spawn(s: &ParticlesSettings, place: &Place, seed: u32, now: u32) -> Speck {
    let life = (s.life * (1.0 + s.life_spread * signed(seed, 10))).max(1e-3);
    let speed = s.speed * (1.0 + s.speed_spread * signed(seed, 11)) * place.impact;
    let moving = place.velocity.try_normalize();
    let fixed = Vec2::from_angle(s.angle.to_radians());
    let way = match s.heading {
        Heading::AWay => fixed,
        Heading::AllRound => Vec2::from_angle(unit(seed, 12) * std::f32::consts::TAU),
        Heading::OffTheBounce => place.out.unwrap_or(fixed),
        Heading::AlongThePath => moving.unwrap_or(fixed),
        Heading::Behind => moving.map(|m| -m).unwrap_or(fixed),
    };
    let way = if s.heading == Heading::AllRound {
        way
    } else {
        Vec2::from_angle(signed(seed, 13) * 0.5 * s.spread.to_radians()).rotate(way)
    };
    let velocity = way * speed + place.velocity * s.inherit;
    let size = (s.size * (1.0 + s.size_spread * signed(seed, 14))).max(0.0);
    let turn = s.turn.to_radians() + s.turn_spread * unit(seed, 15) * std::f32::consts::TAU;
    let mut spin = (s.spin * (1.0 + s.spin_spread * signed(seed, 16))).to_radians();
    if s.spin_both && unit(seed, 17) < 0.5 {
        spin = -spin;
    }
    let speck = Speck {
        place: place.at,
        previous: place.at,
        velocity,
        home: place.at,
        born: now,
        life,
        size,
        turn,
        spin,
        flip: unit(seed, 18) * std::f32::consts::TAU,
        seed,
        ball: place.ball,
        flags: 0,
    };
    if speck.place.is_finite() && speck.velocity.is_finite() {
        speck
    } else {
        Speck { flags: super::state::DEAD, ..speck }
    }
}

/// The seed of the thing's birth number `index`.
pub fn seed_of(world_seed: u64, id: ThingId, index: u64, own: u32) -> u32 {
    hash(&[world_seed as u32, (world_seed >> 32) as u32, id.0, index as u32, (index >> 32) as u32, own])
}

/// Makes this step's births into `st`: how many the Source says, from where it says, dropping
/// those past Most particles. `first` is the build's step (Keep a crowd, Once at the start and the
/// first burst are born then); `since` is the world step the last particle step ended on.
pub fn births(st: &mut ParticlesState, cx: &BirthCx, since: u64, first: bool) {
    let s = st.settings.clone();
    let mut places: Vec<(Option<Place>, u32)> = Vec::new();
    // `None`: the source's own place, picked from each birth's dice.
    match s.emit {
        Emit::Steady => {
            let n = riding_count(&s, cx.world, st.parent) as f64;
            st.carry += s.rate.max(0.0) as f64 * n * super::step::DT as f64;
            if !st.carry.is_finite() {
                st.carry = 0.0;
            }
            let born = st.carry.floor().min(MOST_AT_ONCE as f64);
            st.carry -= born;
            st.carry = st.carry.min(1.0);
            places.push((None, born as u32));
        }
        Emit::Bursts => {
            if first || st.step >= st.next_burst {
                let gap = ((s.every.max(0.001) / super::step::DT).round() as u32).max(1);
                st.next_burst = st.step + gap;
                places.push((None, s.count.min(MOST_AT_ONCE)));
            }
        }
        Emit::OnHits => {
            if !first {
                places.extend(hit_places(&s, st, cx, since).into_iter().map(|(p, n)| (Some(p), n)));
            }
        }
        Emit::KeepACrowd => {
            let alive = st.specks.len() as u32;
            places.push((None, s.count.min(MOST_AT_ONCE).saturating_sub(alive)));
        }
        Emit::OnceAtTheStart => {
            if first {
                places.push((None, s.count.min(MOST_AT_ONCE)));
            }
        }
    }
    let most = s.most.clamp(1, super::MOST_EVER) as usize;
    for (place, count) in places {
        for _ in 0..count {
            let index = st.births;
            st.births += 1;
            if st.specks.len() >= most {
                continue;
            }
            let seed = seed_of(cx.world.seed, st.id, index, s.seed);
            let at = match place {
                Some(p) => p,
                None => match place_for(&s, cx, st.parent, seed) {
                    Some(p) => p,
                    None => continue,
                },
            };
            let speck = spawn(&s, &at, seed, st.step);
            if speck.flags & super::state::DEAD == 0 {
                st.specks.push(speck);
            }
        }
    }
}
