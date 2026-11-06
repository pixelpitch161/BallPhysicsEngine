//! A light in the picture: nothing drawn of its own, only its light, through the
//! [`ThingDraw::light`] hook. It reads where it is from its Position, or from what it rides when
//! its parent in the tree is a Balls, Emitter or Shape thing.

use bpe_core::{Settings, ThingId};
use bpe_gfx::{DrawCx, Frame, LightGpu, LightScene, ThingDraw};
use bpe_runtime::{EventKind, Outline, World};
use glam::Vec2;

use super::{AreaShape, LightSettings, LightType, Points, RidesOn, StripAlong};
use crate::things::rig::pattern::Drive;

pub struct LightDraw;

/// How many points a round outline is cut into for a strip running along it.
const ROUND_POINTS: usize = 96;
/// A hit this fast or faster flares a light fully.
const FULL_HIT: f32 = 600.0;

impl ThingDraw for LightDraw {
    fn kind(&self) -> &'static str {
        super::LIGHT
    }

    fn draw(&self, _cx: &DrawCx, _frame: &mut Frame) {}

    fn light(&self, cx: &DrawCx, scene: &mut LightScene) {
        // The showing lighting scene stands in for the light's own settings, then its rigs play
        // their patterns on each lamp.
        let scenes = crate::modules::light_scenes::now(cx);
        let s = LightSettings::from_params(&scenes.params(cx.resolved.id, &cx.resolved.params));
        let spots = places(cx, &s);
        let drives = crate::things::rig::fixtures::drives(cx, &scenes, spots.len());
        for (spot, drive) in spots.iter().zip(drives) {
            push_light(cx, &s, spot, &drive, scene);
        }
    }
}

/// One place a light shines from: a light riding each ball is one per ball.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub at: Vec2,
    /// How the ridden thing is moving (for Points: Where it's going).
    pub velocity: Vec2,
    /// The radius of the ridden ball, so its own shadow doesn't swallow its light.
    pub skip: f32,
    /// The ridden ball (thing, row), for its own flare.
    pub ball: Option<(ThingId, u32)>,
}

/// Where the light shines from at this moment.
pub fn places(cx: &DrawCx, s: &LightSettings) -> Vec<Place> {
    // A rig only groups lights: a light rides what its rig sits in.
    places_in(cx.world, crate::things::rig::fixtures::ridden(cx.tree, cx.resolved.id), s)
}

/// Where a light riding `parent` (its tree parent, past any rigs) shines from in `world`: its Position, or what it
/// rides moved by its Position. The editor's handles use this too, so they sit on the light.
pub fn places_in(world: &World, parent: Option<ThingId>, s: &LightSettings) -> Vec<Place> {
    let own = Place { at: s.position, ..Place::default() };
    let Some(parent) = parent.filter(|&p| p != ThingId::ROOT) else { return vec![own] };
    if let Some(bodies) = world.bodies.get(&parent) {
        let ball = |row: usize| Place {
            at: bodies.position[row] + s.position,
            velocity: bodies.velocity[row],
            skip: bodies.radius[row],
            ball: Some((parent, row as u32)),
        };
        let alive = || (0..bodies.len()).filter(|&r| bodies.alive[r]);
        return match s.rides_on {
            RidesOn::EachBall => alive().map(ball).collect(),
            RidesOn::BallNumber => {
                let row = bodies.row_of(s.ball_number.saturating_sub(1)).filter(|&r| bodies.alive[r]);
                row.map(ball).into_iter().collect()
            }
            // A ball group has no edge: the middle of its balls.
            RidesOn::TheMiddle | RidesOn::APointOnTheEdge => {
                let n = alive().count();
                if n == 0 {
                    return Vec::new();
                }
                let (sum, vel) = alive()
                    .fold((Vec2::ZERO, Vec2::ZERO), |(p, v), r| (p + bodies.position[r], v + bodies.velocity[r]));
                vec![Place { at: sum / n as f32 + s.position, velocity: vel / n as f32, ..Place::default() }]
            }
        };
    }
    if let Some(wall) = world.walls.get(&parent) {
        let pose = world.poses.get(&parent).copied().unwrap_or_default();
        let at = match s.rides_on {
            RidesOn::APointOnTheEdge => point_round(&outline_points(&wall.outline), s.edge_at),
            _ => outline_centre(&wall.outline),
        };
        return vec![Place {
            at: at + s.position,
            velocity: if wall.velocity != Vec2::ZERO { wall.velocity } else { pose.velocity },
            ..Place::default()
        }];
    }
    // A parent the world doesn't move (a folder): the light sits at its Position.
    vec![own]
}

/// Adds the light at `place` to the scene, as its rigs drive it.
fn push_light(cx: &DrawCx, s: &LightSettings, place: &Place, drive: &Drive, scene: &mut LightScene) {
    let colour = drive.colour.unwrap_or_else(|| s.linear_colour());
    let strength = s.strength * (1.0 + s.flare * flare(cx, s, place)) * drive.strength;
    let place = &Place { at: place.at + Vec2::new(drive.sway, 0.0), ..*place };
    let dir = Vec2::from_angle(aim_angle(s, place, cx.time) + drive.turn.to_radians());
    let half_cone = (0.5 * s.cone).to_radians();
    let edge = s.cone_edge.max(0.0).to_radians();
    let (cos_inner, cos_outer) = ((half_cone - edge).max(0.0).cos(), half_cone.cos());
    let height = if s.kind == LightType::Sun { s.elevation.to_radians().sin() } else { s.height };
    let shadow = s.shadow_colour.to_linear();
    let mut size = s.size.max(0.0);
    let (mut first, mut count) = (0.0, 0.0);
    if s.kind == LightType::Strip {
        size += 0.5 * s.width.max(0.0);
        let points = match s.strip_along {
            StripAlong::Line => {
                let half = dir * (0.5 * s.length);
                vec![place.at - half, place.at + half]
            }
            StripAlong::AShapesOutline => strip_outline(cx.world, s, cx.resolved.id),
        };
        first = scene.segments.len() as f32;
        let closed = s.strip_along == StripAlong::AShapesOutline;
        let n = points.len();
        let pairs = if closed { n } else { n.saturating_sub(1) };
        for k in 0..pairs {
            let (a, b) = (points[k], points[(k + 1) % n]);
            scene.segments.push([a.x, a.y, b.x, b.y]);
        }
        count = pairs as f32;
    }
    scene.lights.push(LightGpu {
        p0: [place.at.x, place.at.y, height, s.kind.code()],
        p1: [colour[0] * strength, colour[1] * strength, colour[2] * strength, 0.0],
        p2: [s.reach.max(0.0), s.falloff.code(), size, s.shadow_strength],
        p3: [dir.x, dir.y, cos_inner, cos_outer],
        p4: [s.width.max(0.0), s.length.max(0.0), first, count],
        p5: [shadow[0], shadow[1], shadow[2], if s.casts { 1.0 } else { 0.0 }],
        p6: [
            place.skip,
            s.sun_size.max(0.0).to_radians().tan(),
            if s.area_shape == AreaShape::Disc { 1.0 } else { 0.0 },
            s.softness.max(0.0),
        ],
    });
}

/// Which way the light at `place` points at `time`, in radians clockwise from the right: its
/// Direction, turned by Spin, or the way the ball it rides is going.
pub fn aim_angle(s: &LightSettings, place: &Place, time: f64) -> f32 {
    match s.points {
        Points::FixedAngle => s.direction.to_radians(),
        Points::WhereItsGoing if place.velocity.length_squared() > 1e-6 => place.velocity.y.atan2(place.velocity.x),
        Points::WhereItsGoing => s.direction.to_radians(),
        Points::Spins => (s.direction + s.spin * time as f32).to_radians(),
    }
}

/// How much the light flares now, 0 to 1 and more for stacked hits: each hit by what it hears
/// (its own ball, when it rides one) flares it by its strength, fading over Flare length.
fn flare(cx: &DrawCx, s: &LightSettings, place: &Place) -> f32 {
    if s.flare == 0.0 {
        return 0.0;
    }
    let world = cx.world;
    let rate = world.steps_per_second.max(1);
    let length = s.flare_length.max(1e-3) as f64;
    let first = ((cx.time - length) * rate as f64).ceil().max(0.0) as u64;
    let last = (cx.time * rate as f64 + 1e-6).floor().max(0.0) as u64 + 1;
    let heard = if place.ball.is_none() { world.matching(&s.flare_from, cx.resolved.id) } else { Vec::new() };
    let mut sum = 0.0f32;
    for e in cx.log.between(first, last) {
        if e.kind != EventKind::Hit {
            continue;
        }
        let by = match place.ball {
            Some((thing, row)) => {
                (e.a.thing == thing && e.a.item == row) || e.b.is_some_and(|b| b.thing == thing && b.item == row)
            }
            None => heard.contains(&e.a.thing),
        };
        if !by {
            continue;
        }
        let age = ((cx.time - e.time(rate)) / length).clamp(0.0, 1.0) as f32;
        sum += (e.speed / FULL_HIT).min(1.0) * (1.0 - age) * (1.0 - age);
    }
    sum
}

/// The outline a strip runs along: the first thing its Shape setting names that is a wall.
fn strip_outline(world: &World, s: &LightSettings, me: ThingId) -> Vec<Vec2> {
    world
        .matching(&s.strip_shape, me)
        .into_iter()
        .find_map(|id| world.walls.get(&id))
        .map(|w| outline_points(&w.outline))
        .unwrap_or_default()
}

/// The middle of an outline.
fn outline_centre(outline: &Outline) -> Vec2 {
    match outline {
        Outline::Ring { center, .. }
        | Outline::Box { center, .. }
        | Outline::Ellipse { center, .. }
        | Outline::Path { center, .. } => *center,
    }
}

/// An outline's centre line in the world as a closed list of points, starting at its top.
pub fn outline_points(outline: &Outline) -> Vec<Vec2> {
    let round = |center: Vec2, half: Vec2, angle: f32| -> Vec<Vec2> {
        let turn = Vec2::from_angle(angle);
        (0..ROUND_POINTS)
            .map(|k| {
                let t = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * k as f32 / ROUND_POINTS as f32;
                center + turn.rotate(Vec2::new(half.x * t.cos(), half.y * t.sin()))
            })
            .collect()
    };
    match outline {
        Outline::Ring { center, radius, sides, .. } if *sides < 3 => round(*center, Vec2::splat(*radius), 0.0),
        Outline::Ring { .. } | Outline::Box { .. } => outline.corners(),
        Outline::Ellipse { center, half, angle, .. } => round(*center, *half, *angle),
        Outline::Path { center, angle, scale, points, .. } => {
            let turn = Vec2::from_angle(*angle);
            points.iter().map(|p| *center + turn.rotate(*p * *scale)).collect()
        }
    }
}

/// The point `share` of the way round a closed list of points (0 and 1 its first point).
pub fn point_round(points: &[Vec2], share: f32) -> Vec2 {
    let n = points.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    let side = |k: usize| points[k].distance(points[(k + 1) % n]);
    let total: f32 = (0..n).map(side).sum();
    if total <= 0.0 {
        return points[0];
    }
    let mut left = share.rem_euclid(1.0) * total;
    for k in 0..n {
        let d = side(k);
        if left <= d {
            return points[k].lerp(points[(k + 1) % n], left / d.max(1e-9));
        }
        left -= d;
    }
    points[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_goes_round_by_its_share() {
        let square = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        assert_eq!(point_round(&square, 0.0), Vec2::ZERO);
        assert_eq!(point_round(&square, 0.125), Vec2::new(5.0, 0.0));
        assert_eq!(point_round(&square, 0.5), Vec2::new(10.0, 10.0));
        assert_eq!(point_round(&square, 1.25), Vec2::new(10.0, 0.0));
    }

    #[test]
    fn a_light_points_by_its_direction_its_spin_or_its_ball() {
        let s = LightSettings { direction: 90.0, spin: 30.0, ..LightSettings::default() };
        let place = Place { velocity: Vec2::new(-5.0, 0.0), ..Place::default() };
        let close = |a: f32, b: f32| (a - b.to_radians()).abs() < 1e-5;
        assert!(close(aim_angle(&s, &place, 2.0), 90.0));
        let spins = LightSettings { points: Points::Spins, ..s.clone() };
        assert!(close(aim_angle(&spins, &place, 2.0), 150.0));
        let going = LightSettings { points: Points::WhereItsGoing, ..s.clone() };
        assert!(close(aim_angle(&going, &place, 2.0), 180.0));
        assert!(close(aim_angle(&going, &Place::default(), 2.0), 90.0), "standing still: its Direction");
    }

    #[test]
    fn a_light_at_the_root_sits_at_its_position() {
        let kit = crate::kit();
        let scene = crate::new_studio_scene(&kit.registry);
        let world = World::build(&scene, &bpe_core::resolve(&scene, &kit.registry), &kit.sims);
        let s = LightSettings { position: Vec2::new(3.0, 4.0), ..LightSettings::default() };
        let at = places_in(&world, Some(ThingId::ROOT), &s);
        assert_eq!(at.len(), 1);
        assert_eq!(at[0].at, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn a_round_outline_starts_at_its_top() {
        let ring = Outline::Ring { center: Vec2::new(5.0, 5.0), radius: 10.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let points = outline_points(&ring);
        assert_eq!(points.len(), ROUND_POINTS);
        assert!(points[0].distance(Vec2::new(5.0, -5.0)) < 1e-4, "{:?}", points[0]);
    }
}
