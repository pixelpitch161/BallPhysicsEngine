//! Solid shapes inside the arena that balls bounce off from outside: pegs, bars and rings.

use glam::Vec2;

use crate::pose::Pose;
use crate::walls::Contact;

/// One solid piece of an obstacle, in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Solid {
    /// A round peg.
    Disc { center: Vec2, radius: f32 },
    /// A bar with round ends: every point within `radius` of the line from `from` to `to`.
    Capsule { from: Vec2, to: Vec2, radius: f32 },
    /// A hollow ring whose centre line is `radius` from `center`; balls bounce off both sides.
    Ring { center: Vec2, radius: f32, thickness: f32 },
}

impl Solid {
    /// A circle around the whole piece: its centre and radius.
    pub fn bounds(&self) -> (Vec2, f32) {
        match *self {
            Solid::Disc { center, radius } => (center, radius),
            Solid::Capsule { from, to, radius } => ((from + to) * 0.5, (to - from).length() * 0.5 + radius),
            Solid::Ring { center, radius, thickness } => (center, radius + thickness * 0.5),
        }
    }
}

/// Every solid piece of one thing, with the pose they move with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solids {
    pub pose: Pose,
    pub parts: Vec<Solid>,
    /// Each part's bounding circle, for skipping balls that are nowhere near it.
    pub bounds: Vec<(Vec2, f32)>,
    /// How far the farthest point of any part is from `pose.position`.
    pub reach: f32,
}

impl Solids {
    pub fn new(pose: Pose, parts: Vec<Solid>) -> Solids {
        let bounds: Vec<(Vec2, f32)> = parts.iter().map(Solid::bounds).collect();
        let reach = bounds.iter().map(|(c, r)| (*c - pose.position).length() + r).fold(0.0, f32::max);
        Solids { pose, parts, bounds, reach }
    }
}

/// The contact of a ball at `position` whose surface normal (from the solid towards the ball) is
/// `normal`, when the ball is moving into the solid.
fn contact(position: Vec2, velocity: Vec2, radius: f32, normal: Vec2, pose: &Pose) -> Option<Contact> {
    let point = position - normal * radius;
    let wall_velocity = pose.velocity + (point - pose.position).perp() * pose.spin;
    let speed = (wall_velocity - velocity).dot(normal);
    (speed > 0.0).then_some(Contact { point, normal, speed, wall_velocity })
}

/// Where the path from `start` first reaches `reach` from the origin, as a fraction of the path,
/// when it starts outside and gets there within the path.
fn ray_hits_circle(start: Vec2, path: Vec2, reach: f32) -> Option<f32> {
    let (a, b, c) = (path.length_squared(), start.dot(path), start.length_squared() - reach * reach);
    if a < 1e-9 || c <= 0.0 || b >= 0.0 {
        return None;
    }
    let discriminant = b * b - a * c;
    if discriminant < 0.0 {
        return None;
    }
    let t = (-b - discriminant.sqrt()) / a;
    (t <= 1.0).then_some(t.max(0.0))
}

/// The closest point to `p` on the segment `a`-`b`.
fn closest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-12 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

/// Where the path from `start` first comes within `reach` of the segment `a`-`b`, when it
/// starts outside and gets there within the path.
fn ray_hits_capsule(start: Vec2, path: Vec2, a: Vec2, b: Vec2, reach: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut consider = |t: Option<f32>| {
        if let Some(t) = t
            && best.is_none_or(|b| t < b)
        {
            best = Some(t);
        }
    };
    consider(ray_hits_circle(start - a, path, reach));
    consider(ray_hits_circle(start - b, path, reach));
    let ab = b - a;
    let len = ab.length();
    if len > 1e-6 {
        let x = ab / len;
        let y = x.perp();
        let (s, p) = (start - a, path);
        let (sy, py) = (s.dot(y), p.dot(y));
        for side in [1.0, -1.0] {
            // The side line at y = side * reach, approached from beyond it.
            let line = side * reach;
            if sy * side <= reach || py.abs() < 1e-9 {
                continue;
            }
            let t = (line - sy) / py;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let hit_x = (s + p * t).dot(x);
            if (0.0..=len).contains(&hit_x) {
                consider(Some(t));
            }
        }
    }
    best
}

/// Moves a ball out of a solid piece and reports the contact when it was moving into it. The
/// path from `previous` to `position` is swept, so a fast ball can't jump through a thin bar.
/// A ball born inside a piece is pushed straight out, along `-y` when there is no better
/// direction, and never sent anywhere non-finite.
#[inline(always)]
pub fn keep_outside(
    position: &mut Vec2,
    previous: Vec2,
    velocity: Vec2,
    radius: f32,
    solid: &Solid,
    pose: &Pose,
) -> Option<Contact> {
    match *solid {
        Solid::Disc { center, radius: disc } => {
            let reach = disc + radius;
            let swept = ray_hits_circle(previous - center, *position - previous, reach);
            if let Some(t) = swept {
                *position = previous + (*position - previous) * t;
            }
            let offset = *position - center;
            let distance = offset.length();
            if swept.is_none() && distance >= reach {
                return None;
            }
            let normal = if distance > 1e-6 { offset / distance } else { Vec2::NEG_Y };
            *position = center + normal * reach;
            contact(*position, velocity, radius, normal, pose)
        }
        Solid::Capsule { from, to, radius: bar } => {
            let reach = bar + radius;
            let swept = ray_hits_capsule(previous, *position - previous, from, to, reach);
            if let Some(t) = swept {
                *position = previous + (*position - previous) * t;
            }
            let closest = closest_on_segment(*position, from, to);
            let offset = *position - closest;
            let distance = offset.length();
            if swept.is_none() && distance >= reach {
                return None;
            }
            let normal = if distance > 1e-6 {
                offset / distance
            } else {
                let along = to - from;
                if along.length_squared() > 1e-12 { along.perp().normalize() } else { Vec2::NEG_Y }
            };
            *position = closest + normal * reach;
            contact(*position, velocity, radius, normal, pose)
        }
        Solid::Ring { center, radius: ring, thickness } => {
            let inner = ring - thickness * 0.5 - radius;
            let outer = ring + thickness * 0.5 + radius;
            let offset = *position - center;
            let distance = offset.length();
            // The side the ball came from decides which way it is kept.
            let came_from_inside = (previous - center).length() <= ring;
            if came_from_inside {
                if inner <= 0.0 {
                    *position = center;
                    return None;
                }
                if distance <= inner || distance < 1e-6 {
                    return None;
                }
                let outward = offset / distance;
                *position = center + outward * inner;
                contact(*position, velocity, radius, -outward, pose)
            } else {
                if distance >= outer {
                    return None;
                }
                let outward = if distance > 1e-6 { offset / distance } else { Vec2::NEG_Y };
                *position = center + outward * outer;
                contact(*position, velocity, radius, outward, pose)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflect;

    #[test]
    fn a_ball_hitting_a_peg_is_pushed_out_and_bounces_back() {
        let peg = Solid::Disc { center: Vec2::ZERO, radius: 10.0 };
        let mut p = Vec2::new(0.0, -12.0);
        let c =
            keep_outside(&mut p, Vec2::new(0.0, -20.0), Vec2::new(0.0, 100.0), 5.0, &peg, &Pose::default()).unwrap();
        assert!((p.y + 15.0).abs() < 1e-4, "{p:?}");
        assert_eq!(c.normal, Vec2::new(0.0, -1.0));
        assert!((c.speed - 100.0).abs() < 1e-4);
        assert_eq!(c.point, Vec2::new(0.0, -10.0));
        assert!((reflect(Vec2::new(0.0, 100.0), &c, 1.0).y + 100.0).abs() < 1e-4);
        // Moving away: pushed out, but no hit.
        let mut q = Vec2::new(0.0, -12.0);
        assert!(
            keep_outside(&mut q, Vec2::new(0.0, -12.0), Vec2::new(0.0, -100.0), 5.0, &peg, &Pose::default()).is_none()
        );
        assert!((q.y + 15.0).abs() < 1e-4);
    }

    #[test]
    fn a_fast_ball_cannot_tunnel_through_a_thin_bar() {
        let bar = Solid::Capsule { from: Vec2::new(-100.0, 0.0), to: Vec2::new(100.0, 0.0), radius: 1.0 };
        let mut p = Vec2::new(0.0, 8.0); // already past the bar this substep
        let c =
            keep_outside(&mut p, Vec2::new(0.0, -8.0), Vec2::new(0.0, 4000.0), 2.0, &bar, &Pose::default()).unwrap();
        assert!(p.y < 0.0 && (p.y + 3.0).abs() < 1e-3, "{p:?}");
        assert_eq!(c.normal, Vec2::new(0.0, -1.0));
        // The same through a peg.
        let peg = Solid::Disc { center: Vec2::ZERO, radius: 1.0 };
        let mut q = Vec2::new(0.0, 8.0);
        assert!(
            keep_outside(&mut q, Vec2::new(0.0, -8.0), Vec2::new(0.0, 4000.0), 2.0, &peg, &Pose::default()).is_some()
        );
        assert!((q.y + 3.0).abs() < 1e-3, "{q:?}");
        // The bar's ends are round.
        let mut e = Vec2::new(104.0, 0.0);
        assert!(
            keep_outside(&mut e, Vec2::new(104.0, 0.0), Vec2::new(-10.0, 0.0), 2.0, &bar, &Pose::default()).is_none()
        );
        e = Vec2::new(102.0, 0.0);
        assert!(
            keep_outside(&mut e, Vec2::new(105.0, 0.0), Vec2::new(-10.0, 0.0), 2.0, &bar, &Pose::default()).is_some()
        );
        assert!((e.x - 103.0).abs() < 1e-3);
        // A diagonal pass across the bar's middle is caught too.
        let mut d = Vec2::new(30.0, 6.0);
        keep_outside(&mut d, Vec2::new(10.0, -6.0), Vec2::new(4000.0, 2400.0), 2.0, &bar, &Pose::default()).unwrap();
        assert!(d.y < 0.0 && (d.y + 3.0).abs() < 1e-3, "{d:?}");
    }

    #[test]
    fn a_ring_bounces_balls_from_both_sides() {
        let ring = Solid::Ring { center: Vec2::ZERO, radius: 100.0, thickness: 4.0 };
        let mut inside = Vec2::new(97.0, 0.0);
        let c = keep_outside(&mut inside, Vec2::new(90.0, 0.0), Vec2::new(50.0, 0.0), 3.0, &ring, &Pose::default())
            .unwrap();
        assert!((inside.x - 95.0).abs() < 1e-3 && c.normal == Vec2::new(-1.0, 0.0), "{inside:?} {c:?}");
        assert!((c.speed - 50.0).abs() < 1e-4);
        let mut outside = Vec2::new(103.0, 0.0);
        let c = keep_outside(&mut outside, Vec2::new(110.0, 0.0), Vec2::new(-50.0, 0.0), 3.0, &ring, &Pose::default())
            .unwrap();
        assert!((outside.x - 105.0).abs() < 1e-3 && c.normal == Vec2::new(1.0, 0.0), "{outside:?} {c:?}");
        // Crossing the whole band in one go still lands on the side it came from.
        let mut through = Vec2::new(108.0, 0.0);
        keep_outside(&mut through, Vec2::new(92.0, 0.0), Vec2::new(4000.0, 0.0), 3.0, &ring, &Pose::default()).unwrap();
        assert!((through.x - 95.0).abs() < 1e-3, "{through:?}");
        // A ball too big for the inside sits at the centre.
        let mut big = Vec2::new(10.0, 0.0);
        assert!(keep_outside(&mut big, Vec2::new(10.0, 0.0), Vec2::X, 99.0, &ring, &Pose::default()).is_none());
        assert_eq!(big, Vec2::ZERO);
    }

    #[test]
    fn a_moving_peg_carries_its_speed_into_the_contact() {
        let peg = Solid::Disc { center: Vec2::ZERO, radius: 10.0 };
        let pose = Pose { velocity: Vec2::new(30.0, 0.0), ..Pose::default() };
        let mut p = Vec2::new(12.0, 0.0);
        let c = keep_outside(&mut p, Vec2::new(14.0, 0.0), Vec2::ZERO, 5.0, &peg, &pose).unwrap();
        assert_eq!(c.wall_velocity, Vec2::new(30.0, 0.0));
        assert!((c.speed - 30.0).abs() < 1e-4);
        // A spinning peg's rim moves too.
        let spinning = Pose { spin: 2.0, ..Pose::default() };
        let mut q = Vec2::new(12.0, 0.0);
        let c = keep_outside(&mut q, Vec2::new(12.0, 0.0), Vec2::new(-1.0, 0.0), 5.0, &peg, &spinning).unwrap();
        assert!((c.wall_velocity - Vec2::new(0.0, 20.0)).length() < 1e-4, "{c:?}");
    }

    #[test]
    fn a_ball_born_inside_a_peg_is_pushed_out_finitely() {
        let peg = Solid::Disc { center: Vec2::ZERO, radius: 10.0 };
        let mut p = Vec2::ZERO;
        let c = keep_outside(&mut p, Vec2::ZERO, Vec2::ZERO, 5.0, &peg, &Pose::default());
        assert!(p.is_finite() && (p.length() - 15.0).abs() < 1e-3);
        assert!(c.is_none(), "not moving into it, so no hit");
        let cap = Solid::Capsule { from: Vec2::new(-10.0, 0.0), to: Vec2::new(10.0, 0.0), radius: 2.0 };
        let mut q = Vec2::new(3.0, 0.0);
        keep_outside(&mut q, Vec2::new(3.0, 0.0), Vec2::ZERO, 1.0, &cap, &Pose::default());
        assert!(q.is_finite() && q.y.abs() >= 3.0 - 1e-3, "{q:?}");
        let dot = Solid::Capsule { from: Vec2::ZERO, to: Vec2::ZERO, radius: 2.0 };
        let mut r = Vec2::ZERO;
        keep_outside(&mut r, Vec2::ZERO, Vec2::ZERO, 1.0, &dot, &Pose::default());
        assert!(r.is_finite() && (r.length() - 3.0).abs() < 1e-3, "{r:?}");
    }

    #[test]
    fn solids_know_their_reach() {
        let pose = Pose { position: Vec2::new(10.0, 0.0), ..Pose::default() };
        let s = Solids::new(
            pose,
            vec![
                Solid::Disc { center: Vec2::new(10.0, 0.0), radius: 5.0 },
                Solid::Capsule { from: Vec2::new(10.0, -30.0), to: Vec2::new(10.0, 30.0), radius: 2.0 },
                Solid::Ring { center: Vec2::new(10.0, 0.0), radius: 20.0, thickness: 4.0 },
            ],
        );
        assert_eq!(s.reach, 32.0);
        assert_eq!(s.bounds[1], (Vec2::new(10.0, 0.0), 32.0));
        assert_eq!(Solids::default().reach, 0.0);
    }
}
