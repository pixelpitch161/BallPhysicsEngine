//! Where and when a Particles thing's particles are born, and what each starts as. Every number
//! a birth uses comes from its own dice (`hash` of its seed and a fixed number per use), its seed
//! from the scene's seed, the thing and its birth number: never a shared random stream, so adding
//! a thing or a particle never changes another's.

use bpe_core::ThingId;
use bpe_runtime::{EventKind, Log, Outline, World};
use glam::Vec2;

use super::beats::Moments;
use super::state::{HomeFrame, ParticlesState, Speck};
use super::{AreaShape, BornFrom, Edge, Emit, Heading, ParticlesSettings, RidesOn};
use crate::modules::look::layers::particles::emit::{hash, impact};

/// The most particles one moment (a hit, a burst) lets off.
pub const MOST_AT_ONCE: u32 = 100_000;
/// The most particles one moment of a parent particle (it died, landed, popped…) lets off.
pub const MOST_FED: u32 = 1000;

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
    /// The word's, picture's or grid's place it is born at (index + 1), 0 when none.
    pub spot: u32,
    /// The parent particle it is born from (its seed and how far through its life it was).
    pub kin: Option<(u32, f32)>,
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
    /// Where a word's, picture's or grid's places are now.
    pub homes: HomeFrame,
}

/// Where place `spot` (index + 1) of a word, picture or grid is now, as a birth there.
pub fn spot_place(homes: &HomeFrame, spot: u32) -> Option<Place> {
    let at = homes.at(spot)?;
    Some(Place { at, out: (at - homes.centre).try_normalize(), impact: 1.0, spot, ..Place::default() })
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
        // Born only from the parent's particles (`fed_places`); on its own, at its place.
        BornFrom::ItsParentsParticles => here(cx.anchor),
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
                    spot: 0,
                    kin: None,
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
        BornFrom::AWordOrPicture | BornFrom::AGrid => {
            let n = cx.homes.spots.len();
            if n == 0 {
                return None;
            }
            return spot_place(&cx.homes, (hash(&[seed, 8]) as usize % n) as u32 + 1);
        }
    })
}

/// The hits since the last particle step that let particles off: their places.
pub fn hit_places(
    s: &ParticlesSettings,
    st: &ParticlesState,
    cx: &BirthCx,
    since: u64,
    at_once: u32,
) -> Vec<(Place, u32)> {
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
        let place =
            Place { at: e.point, velocity, out: out_way.try_normalize(), ball, impact: strength, spot: 0, kin: None };
        out.push((place, count.min(at_once)));
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
        Heading::RoundTheMiddle => place.out.map(|o| o.perp()).unwrap_or(fixed),
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
    let (kin_seed, kin_life) = place.kin.unwrap_or((seed, 0.0));
    let speck = Speck {
        velocity,
        life,
        size,
        turn,
        spin,
        flip: unit(seed, 18) * std::f32::consts::TAU,
        ball: place.ball,
        spot: place.spot,
        kin_seed,
        kin_life,
        ..Speck::at(place.at, now, seed)
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

/// The places a fed thing's particles are born at this step, from `parent`'s moments (or, Every
/// step, its particles), with how many at each.
pub fn fed_places(s: &ParticlesSettings, parent: &ParticlesState, id: ThingId) -> Vec<(Place, u32)> {
    let mut out = Vec::new();
    if s.fed_when == super::FedWhen::EveryStep {
        let rate = s.rate.max(0.0) * super::step::DT;
        if !rate.is_finite() {
            return out;
        }
        for p in &parent.specks {
            let n = (rate + unit(hash(&[p.seed, parent.step, id.0]), 91)).floor() as u32;
            if n > 0 {
                let place = Place {
                    at: p.place,
                    velocity: p.velocity,
                    out: None,
                    ball: p.ball,
                    impact: 1.0,
                    spot: 0,
                    kin: Some((p.seed, p.life_at(parent.step))),
                };
                out.push((place, n.min(MOST_FED)));
            }
        }
        return out;
    }
    let wants = s.fed_when.wants();
    for m in parent.feeds.iter().filter(|m| m.kind.bit() & wants != 0) {
        let place = Place {
            at: m.place,
            velocity: m.velocity,
            out: m.normal.try_normalize(),
            ball: m.ball,
            impact: 1.0,
            spot: 0,
            kin: Some((m.seed, m.life)),
        };
        out.push((place, s.count.min(MOST_FED)));
    }
    out
}

/// One lot of births.
enum Batch {
    /// Count births from the source's own places, picked by each birth's dice.
    Source(u32),
    /// Count births at a place (a hit).
    At(Place, u32),
    /// One birth at each of these places of a word, picture or grid (index + 1).
    Spots(Vec<u32>),
}

/// Whether the source has places of its own (a word, picture or grid) that a crowd fills.
pub fn has_spots(s: &ParticlesSettings) -> bool {
    matches!(s.from, BornFrom::AWordOrPicture | BornFrom::AGrid)
}

/// Which engine births are planned for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum For {
    /// The exact engine: births past Most particles are dropped here, Keep a crowd makes up
    /// exactly what it is short of, and a moment lets off at most [`MOST_AT_ONCE`].
    Exact,
    /// A Huge swarm, which doesn't know here how many are alive: the graphics card drops births
    /// past Most particles, Keep a crowd is given [`crowd_spares`] a step to take what it is short
    /// of from, and a moment lets off up to [`MOST_SWARM_AT_ONCE`].
    Swarm,
}

/// The most particles one moment lets off in a Huge swarm.
pub const MOST_SWARM_AT_ONCE: u32 = super::MOST_SWARM;

/// How many spare births a Huge swarm keeping a crowd of `crowd` is given each step: enough for
/// the crowd's deaths in a step at its shortest life, twice over, at least 64, at most the crowd.
pub fn crowd_spares(s: &ParticlesSettings, crowd: u32) -> u32 {
    let shortest = (s.life * (1.0 - s.life_spread.abs())).max(1e-3);
    let deaths = (crowd as f64 * super::step::DT as f64 / shortest as f64).ceil();
    ((deaths * 2.0).min(u32::MAX as f64) as u32).max(64).min(crowd)
}

/// A Huge swarm's births this step, oldest first (as [`births`], for `For::Swarm`; its own
/// particles stay on the graphics card, so `st.specks` is empty here). Every one tried is counted
/// in `st.births`; no moments are noted.
pub fn plan(st: &mut ParticlesState, cx: &BirthCx, since: u64, first: bool) -> Vec<Speck> {
    let kept = std::mem::take(&mut st.specks);
    births_for(st, cx, since, first, None, For::Swarm);
    std::mem::replace(&mut st.specks, kept)
}

/// Makes this step's births into `st`: how many the Source says, from where it says, dropping
/// those past Most particles. `first` is the build's step (Keep a crowd, Once at the start and the
/// first burst are born then); `since` is the world step the last particle step ended on;
/// `parent` the Particles thing feeding it, when its particles are born from its parent's.
pub fn births(st: &mut ParticlesState, cx: &BirthCx, since: u64, first: bool, parent: Option<&ParticlesState>) {
    births_for(st, cx, since, first, parent, For::Exact);
}

/// [`births`] for `engine`.
fn births_for(
    st: &mut ParticlesState,
    cx: &BirthCx,
    since: u64,
    first: bool,
    parent: Option<&ParticlesState>,
    engine: For,
) {
    let s = st.settings.clone();
    let at_once = match engine {
        For::Exact => MOST_AT_ONCE,
        For::Swarm => MOST_SWARM_AT_ONCE,
    };
    let mut batches: Vec<Batch> = Vec::new();
    let spots = cx.homes.spots.len();
    if s.from == BornFrom::ItsParentsParticles {
        if let Some(parent) = parent.filter(|_| !first) {
            batches.extend(fed_places(&s, parent, st.id).into_iter().map(|(p, n)| Batch::At(p, n)));
        }
    } else {
        match s.emit {
            Emit::Steady => {
                let n = riding_count(&s, cx.world, st.parent) as f64;
                st.carry += s.rate.max(0.0) as f64 * n * super::step::DT as f64;
                if !st.carry.is_finite() {
                    st.carry = 0.0;
                }
                let born = st.carry.floor().min(at_once as f64);
                st.carry -= born;
                st.carry = st.carry.min(1.0);
                batches.push(Batch::Source(born as u32));
            }
            Emit::Bursts => {
                if first || st.step >= st.next_burst {
                    let gap = ((s.every.max(0.001) / super::step::DT).round() as u32).max(1);
                    st.next_burst = st.step + gap;
                    batches.push(Batch::Source(s.count.min(at_once)));
                }
            }
            Emit::OnHits => {
                if !first {
                    batches.extend(hit_places(&s, st, cx, since, at_once).into_iter().map(|(p, n)| Batch::At(p, n)));
                }
            }
            // A swarm doesn't know which places are taken: it fills them all at the start only.
            Emit::KeepACrowd if has_spots(&s) && engine == For::Swarm => {
                if first {
                    batches.push(Batch::Spots((1..=spots as u32).collect()));
                }
            }
            // A word's, picture's or grid's crowd fills every place, each death refilled where it was.
            Emit::KeepACrowd if has_spots(&s) => {
                let mut taken = vec![false; spots];
                for sp in &st.specks {
                    if let Some(t) = (sp.spot as usize).checked_sub(1).and_then(|i| taken.get_mut(i)) {
                        *t = true;
                    }
                }
                let free: Vec<u32> =
                    taken.iter().enumerate().filter(|(_, t)| !**t).map(|(i, _)| i as u32 + 1).collect();
                batches.push(Batch::Spots(free));
            }
            Emit::OnceAtTheStart if has_spots(&s) => {
                if first {
                    batches.push(Batch::Spots((1..=spots as u32).collect()));
                }
            }
            Emit::KeepACrowd => {
                let crowd = s.count.min(at_once);
                let n = match engine {
                    For::Exact => crowd.saturating_sub(st.specks.len() as u32),
                    For::Swarm if first => crowd,
                    For::Swarm => crowd_spares(&s, crowd),
                };
                batches.push(Batch::Source(n));
            }
            Emit::OnceAtTheStart => {
                if first {
                    batches.push(Batch::Source(s.count.min(at_once)));
                }
            }
            Emit::OnTheBeat => {
                let moments = Moments::of(cx.world, s.beat_every, s.bars, s.beat_offset);
                let dt = super::step::DT as f64;
                let now = st.step as f64 * dt;
                let before = if first { f64::NEG_INFINITY } else { now - dt };
                let beats = moments.count_in(before, now);
                batches.push(Batch::Source(s.count.min(at_once).saturating_mul(beats).min(at_once)));
            }
        }
    }
    // In a swarm (no particles here) at most Most a step: more could never fit on the card.
    let most = match engine {
        For::Exact => s.most.clamp(1, super::MOST_EVER),
        For::Swarm => s.most.clamp(1, MOST_SWARM_AT_ONCE),
    } as usize;
    let born = |st: &mut ParticlesState, place: Option<Place>, index: u64| {
        let seed = seed_of(cx.world.seed, st.id, index, s.seed);
        let at = match place {
            Some(p) => p,
            None => match place_for(&s, cx, st.parent, seed) {
                Some(p) => p,
                None => return,
            },
        };
        let speck = spawn(&s, &at, seed, st.step);
        if speck.flags & super::state::DEAD == 0 {
            st.specks.push(speck);
        }
    };
    // Bursts note a Burst each, and a Born for each particle they make (moments, part 7); a fed
    // thing's births follow its parent's particles, not its Emit.
    let bursts = engine == For::Exact
        && s.from != BornFrom::ItsParentsParticles
        && matches!(s.emit, Emit::Bursts | Emit::OnHits | Emit::OnceAtTheStart | Emit::OnTheBeat);
    for batch in batches {
        let first_born = st.specks.len();
        let hit = match &batch {
            Batch::At(place, _) => Some(*place),
            _ => None,
        };
        match batch {
            Batch::Source(count) | Batch::At(_, count) if count == 0 => {}
            Batch::Source(count) => {
                for _ in 0..count {
                    let index = st.births;
                    st.births += 1;
                    if st.specks.len() < most {
                        born(st, None, index);
                    }
                }
            }
            Batch::At(place, count) => {
                for _ in 0..count {
                    let index = st.births;
                    st.births += 1;
                    if st.specks.len() < most {
                        born(st, Some(place), index);
                    }
                }
            }
            Batch::Spots(list) => {
                for spot in list {
                    // A full crowd stops trying (a huge grid under a small cap).
                    if st.specks.len() >= most {
                        break;
                    }
                    let index = st.births;
                    st.births += 1;
                    born(st, spot_place(&cx.homes, spot), index);
                }
            }
        }
        // A burst all of whose particles the cap refused is no burst.
        if bursts && st.specks.len() > first_born {
            note_burst(st, hit.as_ref(), cx.anchor, first_born);
        }
    }
}

/// Notes a burst that made the particles of `st` from `first` on, at `place` (a hit's) or the
/// source's `anchor`, and a Born for each of them.
fn note_burst(st: &mut ParticlesState, place: Option<&Place>, anchor: Vec2, first: usize) {
    use super::moments::{Moment, MomentKind};
    let born: Vec<Moment> = st.specks[first..]
        .iter()
        .map(|p| Moment {
            kind: MomentKind::Born,
            place: p.place,
            velocity: p.velocity,
            speed: p.velocity.length(),
            size: p.size,
            with: None,
            seed: p.seed,
        })
        .collect();
    st.note_moment(Moment {
        kind: MomentKind::Burst,
        place: place.map_or(anchor, |p| p.at),
        velocity: Vec2::ZERO,
        speed: place.map_or(0.0, |p| p.velocity.length()),
        size: born.len() as f32,
        with: None,
        seed: born.first().map_or(0, |m| m.seed),
    });
    for m in born {
        st.note_moment(m);
    }
}
