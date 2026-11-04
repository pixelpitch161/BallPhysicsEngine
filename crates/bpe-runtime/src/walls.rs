//! Closed walls (rings, polygons, boxes and free outlines) and how a ball meets them.
//!
//! Balls meet a wall from either side. [`keep_clear`] moves a ball back to its own side and
//! reports the contact, and the mover's Bounce module decides what the velocity does with
//! [`reflect`]. A ball's side is the side its centre was on at the start of the step, so a fast
//! ball can't slip through a thin band. Inside a hollow ring or box the maths is the old editor's
//! closed-arena collision, unchanged.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bpe_core::shape_path::{inside_polygon, nearest_on_segment};
use glam::Vec2;

/// The most corners a ring polygon can have.
pub const MAX_SIDES: usize = 64;

/// A closed outline: a ring or regular polygon, a box, or a free outline of straight pieces.
#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    Ring {
        center: Vec2,
        /// Radius, or the corner radius of a polygon.
        radius: f32,
        /// 0 for round, otherwise 3 to [`MAX_SIDES`].
        sides: u32,
        /// Radians clockwise from +x. For a polygon, the direction of one side's outward normal.
        angle: f32,
        thickness: f32,
    },
    Box {
        center: Vec2,
        half: Vec2,
        angle: f32,
        thickness: f32,
    },
    /// A free outline: `points` (a closed list, in shape units: the shape's own frame before it
    /// turns, with its middle at 0) scaled by `scale`, turned by `angle` and placed at `center`.
    Path {
        center: Vec2,
        angle: f32,
        scale: f32,
        points: Arc<[Vec2]>,
        thickness: f32,
    },
}

/// A wall that balls bounce off.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub outline: Outline,
    /// Filled: balls are kept outside it. Otherwise it is a band balls stay on their own side of.
    pub solid: bool,
    /// Radians per second, clockwise.
    pub spin: f32,
    /// Fraction of its size the wall grows per second; negative while shrinking.
    pub growth: f32,
    /// How fast the whole wall is moving, in pixels per second, when it rides something.
    pub velocity: Vec2,
}

/// Where a ball met a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Where the ball touched the wall.
    pub point: Vec2,
    /// From the wall towards the ball.
    pub normal: Vec2,
    /// How fast the ball was moving into the wall, relative to it, in pixels per second.
    pub speed: f32,
    /// How fast the wall was moving there.
    pub wall_velocity: Vec2,
}

fn dir(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

/// The nearest point to `p` on the closed polygon `points`, and that piece's unit normal (either
/// way round).
fn nearest_on_polygon(points: &[Vec2], p: Vec2) -> (Vec2, Vec2) {
    let n = points.len();
    let mut best = (f32::INFINITY, points.first().copied().unwrap_or(Vec2::ZERO), Vec2::X);
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let (_, q, d) = nearest_on_segment(a, b, p);
        if d < best.0 {
            best = (d, q, (b - a).perp().normalize_or(Vec2::X));
        }
    }
    (best.1, best.2)
}

impl Outline {
    /// For a ring or polygon, the point on the outline `u` radians clockwise from `angle`. For a
    /// box or free outline, the point on its edge in that direction from its centre (for a free
    /// outline the furthest one, when the ray crosses it more than once).
    pub fn point(&self, u: f32) -> Vec2 {
        match self {
            &Outline::Ring { center, radius, sides, angle, .. } => {
                if sides < 3 {
                    return center + dir(angle + u) * radius;
                }
                let step = TAU / sides as f32;
                let side = (u / step).round();
                let apothem = radius * (PI / sides as f32).cos();
                center + dir(angle + u) * (apothem / (u - side * step).cos())
            }
            &Outline::Box { center, half, angle, .. } => {
                let d = dir(u);
                let scale = (half.x / d.x.abs().max(1e-6)).min(half.y / d.y.abs().max(1e-6));
                let local = d * scale;
                center + dir(angle) * local.x + dir(angle).perp() * local.y
            }
            Outline::Path { center, angle, scale, points, .. } => {
                let d = dir(u);
                let mut far: f32 = 0.0;
                let n = points.len();
                for i in 0..n {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let e = b - a;
                    let den = d.perp_dot(e);
                    if den.abs() < 1e-12 {
                        continue;
                    }
                    // Ray t·d meets a + s·e.
                    let t = a.perp_dot(e) / den;
                    let s = a.perp_dot(d) / den;
                    if t >= 0.0 && (0.0..=1.0).contains(&s) {
                        far = far.max(t);
                    }
                }
                *center + Vec2::from_angle(*angle).rotate(d * far * *scale)
            }
        }
    }

    pub fn center(&self) -> Vec2 {
        match self {
            Outline::Ring { center, .. } | Outline::Box { center, .. } | Outline::Path { center, .. } => *center,
        }
    }

    pub fn thickness(&self) -> f32 {
        match self {
            Outline::Ring { thickness, .. } | Outline::Box { thickness, .. } | Outline::Path { thickness, .. } => {
                *thickness
            }
        }
    }

    /// Radians clockwise from +x that the outline is turned.
    pub fn angle(&self) -> f32 {
        match self {
            Outline::Ring { angle, .. } | Outline::Box { angle, .. } | Outline::Path { angle, .. } => *angle,
        }
    }

    /// A polygon's or box's corners in the world, in order around it; empty for a round ring or
    /// a free outline.
    pub fn corners(&self) -> Vec<Vec2> {
        let mut buf = [Vec2::ZERO; MAX_SIDES];
        let n = self.corners_into(&mut buf);
        buf[..n].to_vec()
    }

    /// Writes a polygon's or box's corners into `buf`, returning how many.
    fn corners_into(&self, buf: &mut [Vec2; MAX_SIDES]) -> usize {
        match *self {
            Outline::Ring { center, radius, sides, angle, .. } if sides >= 3 => {
                let n = (sides as usize).min(MAX_SIDES);
                for (k, c) in buf.iter_mut().take(n).enumerate() {
                    *c = center + dir(angle + PI / n as f32 + TAU * k as f32 / n as f32) * radius;
                }
                n
            }
            Outline::Box { center, half, angle, .. } => {
                let (x, y) = (dir(angle), dir(angle).perp());
                for (c, (sx, sy)) in buf.iter_mut().zip([(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]) {
                    *c = center + x * (half.x * sx) + y * (half.y * sy);
                }
                4
            }
            _ => 0,
        }
    }

    /// `p` in a free outline's own units.
    fn to_shape(center: Vec2, angle: f32, scale: f32, p: Vec2) -> Vec2 {
        Vec2::from_angle(-angle).rotate(p - center) / scale.max(1e-6)
    }

    /// Whether `p` is inside the outline's centre line (even-odd for a free outline, so a
    /// self-crossing one still has an inside).
    pub fn inside(&self, p: Vec2) -> bool {
        match self {
            &Outline::Ring { center, radius, sides, angle, .. } => {
                let offset = p - center;
                if sides < 3 {
                    return offset.length_squared() < radius * radius;
                }
                let apothem = radius * (PI / sides as f32).cos();
                (0..sides).all(|i| offset.dot(dir(angle + TAU * i as f32 / sides as f32)) < apothem)
            }
            &Outline::Box { center, half, angle, .. } => {
                let offset = p - center;
                offset.dot(dir(angle)).abs() < half.x && offset.dot(dir(angle).perp()).abs() < half.y
            }
            Outline::Path { center, angle, scale, points, .. } => {
                inside_polygon(points, Self::to_shape(*center, *angle, *scale, p))
            }
        }
    }

    /// The nearest point to `p` on the centre line, and the unit normal there pointing from it
    /// towards `p`. When `p` is on the line, the normal is the line's own (either way round).
    pub fn nearest(&self, p: Vec2) -> (Vec2, Vec2) {
        let (q, line) = self.nearest_and_line(p);
        ((q), (p - q).try_normalize().unwrap_or(line))
    }

    /// The nearest point on the centre line and the line's unit normal there (either way round).
    fn nearest_and_line(&self, p: Vec2) -> (Vec2, Vec2) {
        match self {
            &Outline::Ring { center, radius, sides: 0..3, angle, .. } => {
                let d = (p - center).try_normalize().unwrap_or(dir(angle));
                (center + d * radius, d)
            }
            Outline::Path { center, angle, scale, points, .. } => {
                let (q, n) = nearest_on_polygon(points, Self::to_shape(*center, *angle, *scale, p));
                let turn = Vec2::from_angle(*angle);
                (*center + turn.rotate(q * *scale), turn.rotate(n))
            }
            _ => {
                let mut buf = [Vec2::ZERO; MAX_SIDES];
                let n = self.corners_into(&mut buf);
                nearest_on_polygon(&buf[..n], p)
            }
        }
    }
}

/// Velocity of a wall point that turns at `spin` rad/s and scales at `growth` per second
/// around `center`.
pub fn wall_velocity(point: Vec2, center: Vec2, spin: f32, growth: f32) -> Vec2 {
    let r = point - center;
    r.perp() * spin + r * growth
}

/// The contact at `point` against a wall whose surface normal (towards the ball) is `normal`,
/// when the ball is moving into it, relative to the wall's own motion.
fn contact(position: Vec2, velocity: Vec2, radius: f32, outward: Vec2, wall: &Wall) -> Option<Contact> {
    let point = position + outward * radius;
    let wall_velocity = wall.velocity + wall_velocity(point, wall.outline.center(), wall.spin, wall.growth);
    let speed = (velocity - wall_velocity).dot(outward);
    (speed > 0.0).then_some(Contact { point, normal: -outward, speed, wall_velocity })
}

/// Moves a ball back to its own side of the wall and reports the hardest contact it was moving
/// into, if any. Its side is the side `previous` (its centre at the start of the step) was on: a
/// ball that was inside a hollow wall stays inside, one that was outside stays outside, and a
/// solid wall keeps every ball outside (one that starts inside is pushed out of the nearest edge).
/// The velocity is left alone: the ball's Bounce module answers with [`reflect`]. A ball too big
/// for a hollow ring or box it is inside is held at the centre and never touches it.
// Inlined so a loop over every ball works out a box's or polygon's directions only once.
#[inline(always)]
pub fn keep_clear(position: &mut Vec2, previous: Vec2, velocity: Vec2, radius: f32, wall: &Wall) -> Option<Contact> {
    if !wall.solid && wall.outline.inside(previous) {
        match &wall.outline {
            Outline::Path { thickness, .. } => {
                keep_on_side(position, velocity, radius, radius + thickness * 0.5, true, wall)
            }
            _ => keep_inside_exact(position, velocity, radius, wall),
        }
    } else {
        let limit = if wall.solid { radius } else { radius + wall.outline.thickness() * 0.5 };
        keep_on_side(position, velocity, radius, limit, false, wall)
    }
}

/// Keeps the ball's centre at least `limit` from the centre line on one side of it: inside when
/// `inside`, else outside.
#[inline(always)]
fn keep_on_side(
    position: &mut Vec2,
    velocity: Vec2,
    radius: f32,
    limit: f32,
    inside: bool,
    wall: &Wall,
) -> Option<Contact> {
    // A push off one side can leave the ball too close to the next one near a corner, so it is
    // repeated a few times. A settled ball (a hair's breadth counts) stays put.
    const SETTLED: f32 = 1e-3;
    let mut p = *position;
    let mut pushed = None;
    for _ in 0..4 {
        let (near, line) = wall.outline.nearest_and_line(p);
        let away = p - near;
        let p_inside = wall.outline.inside(p);
        if p_inside == inside && away.length() >= limit - SETTLED {
            break;
        }
        // The normal into the ball's own side.
        let side = match away.try_normalize() {
            Some(n) if p_inside == inside => n,
            Some(n) => -n,
            None => {
                // Right on the line: the line's normal, turned to the wanted side.
                let probe = near + line * (1e-3 * limit.max(1e-3));
                if wall.outline.inside(probe) == inside { line } else { -line }
            }
        };
        p = near + side * limit;
        pushed = Some(side);
    }
    let side = pushed?;
    *position = p;
    contact(p, velocity, radius, -side, wall)
}

/// Today's closed-arena collision: moves a ball inside a hollow ring or box back inside.
#[inline(always)]
fn keep_inside_exact(position: &mut Vec2, velocity: Vec2, radius: f32, wall: &Wall) -> Option<Contact> {
    match wall.outline {
        Outline::Ring { center, radius: wall_radius, sides: 0, thickness, .. } => {
            let inner = wall_radius - thickness * 0.5 - radius;
            if inner <= 0.0 {
                // The ball doesn't fit: it sits at the centre with no wall to bounce off.
                *position = center;
                return None;
            }
            let offset = *position - center;
            let distance = offset.length();
            if distance <= inner || distance == 0.0 {
                return None;
            }
            let outward = offset / distance;
            *position = center + outward * inner;
            contact(*position, velocity, radius, outward, wall)
        }
        Outline::Ring { center, radius: wall_radius, sides, angle, thickness } => {
            let apothem = wall_radius * (PI / sides as f32).cos();
            let planes = (0..sides).map(|i| (dir(angle + TAU * i as f32 / sides as f32), apothem));
            keep_inside_planes(position, velocity, radius, center, planes, thickness, wall)
        }
        Outline::Box { center, half, angle, thickness } => {
            let (x, y) = (dir(angle), dir(angle).perp());
            let planes = [(x, half.x), (-x, half.x), (y, half.y), (-y, half.y)];
            keep_inside_planes(position, velocity, radius, center, planes.into_iter(), thickness, wall)
        }
        // Free outlines go through `keep_on_side`.
        Outline::Path { .. } => None,
    }
}

/// Keeps a ball on the inner side of each plane `(outward normal, distance from centre)`.
#[inline(always)]
fn keep_inside_planes(
    position: &mut Vec2,
    velocity: Vec2,
    radius: f32,
    center: Vec2,
    planes: impl Iterator<Item = (Vec2, f32)>,
    thickness: f32,
    wall: &Wall,
) -> Option<Contact> {
    let mut hardest: Option<Contact> = None;
    for (outward, distance) in planes {
        let limit = distance - thickness * 0.5 - radius;
        if limit < 0.0 {
            // The ball doesn't fit across this plane: it is held on the centre line instead,
            // with no wall to bounce off.
            *position -= outward * (*position - center).dot(outward);
            continue;
        }
        let excess = (*position - center).dot(outward) - limit;
        if excess <= 0.0 {
            continue;
        }
        *position -= outward * excess;
        if let Some(c) = contact(*position, velocity, radius, outward, wall)
            && hardest.is_none_or(|h| c.speed > h.speed)
        {
            hardest = Some(c);
        }
    }
    hardest
}

/// The velocity after a bounce: the part into the wall comes back scaled by `bounciness`, and
/// the wall's own motion is added.
pub fn reflect(velocity: Vec2, contact: &Contact, bounciness: f32) -> Vec2 {
    let relative = velocity - contact.wall_velocity;
    relative + contact.normal * contact.speed * (1.0 + bounciness) + contact.wall_velocity
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::ShapePath;
    use std::f32::consts::FRAC_PI_2;

    fn hollow(outline: Outline) -> Wall {
        Wall { outline, solid: false, spin: 0.0, growth: 0.0, velocity: Vec2::ZERO }
    }

    fn ring(radius: f32) -> Wall {
        hollow(Outline::Ring { center: Vec2::ZERO, radius, sides: 0, angle: 0.0, thickness: 2.0 })
    }

    /// A ball that started the step at the wall's middle: it belongs inside.
    fn keep_inside(p: &mut Vec2, v: Vec2, r: f32, wall: &Wall) -> Option<Contact> {
        keep_clear(p, wall.outline.center(), v, r, wall)
    }

    fn path(points: Vec<Vec2>, scale: f32, angle: f32, thickness: f32) -> Outline {
        Outline::Path { center: Vec2::new(10.0, -20.0), angle, scale, points: points.into(), thickness }
    }

    /// An L-shaped outline with its notch at the top right: not convex, so the nearest side isn't
    /// the one a ray hits first. Left side x = -0.7, bottom y = 0.7, inner corner (0.3, -0.3).
    fn l_shape() -> Vec<Vec2> {
        [(-0.7, -1.3), (0.3, -1.3), (0.3, -0.3), (1.3, -0.3), (1.3, 0.7), (-0.7, 0.7)]
            .into_iter()
            .map(Vec2::from)
            .collect()
    }

    #[test]
    fn a_wall_that_rides_along_pushes_a_resting_ball() {
        // The ring moves right at 50 px/s; a still ball at its left edge is run into by it.
        let wall = Wall { velocity: Vec2::new(50.0, 0.0), ..ring(100.0) };
        let mut p = Vec2::new(-95.0, 0.0);
        let c = keep_inside(&mut p, Vec2::ZERO, 5.0, &wall).expect("the wall runs into the ball");
        assert!((c.speed - 50.0).abs() < 1e-3, "{c:?}");
        assert_eq!(c.wall_velocity, Vec2::new(50.0, 0.0));
        assert!((c.normal - Vec2::new(1.0, 0.0)).length() < 1e-3);
        // A still ring leaves a still ball alone.
        let mut p = Vec2::new(-95.0, 0.0);
        assert!(keep_inside(&mut p, Vec2::ZERO, 5.0, &ring(100.0)).is_none());
    }

    #[test]
    fn a_ball_moving_into_the_ring_bounces_back() {
        let mut p = Vec2::new(99.0, 0.0);
        let v = Vec2::new(100.0, 0.0);
        let c = keep_inside(&mut p, v, 5.0, &ring(100.0)).unwrap();
        assert!((p.x - 94.0).abs() < 1e-4);
        assert_eq!(c.normal, Vec2::new(-1.0, 0.0));
        assert!((c.speed - 100.0).abs() < 1e-4);
        assert!((reflect(v, &c, 1.0).x + 100.0).abs() < 1e-4);
        assert!((reflect(v, &c, 0.5).x + 50.0).abs() < 1e-4);
    }

    #[test]
    fn a_ball_moving_away_is_pushed_in_but_not_bounced() {
        let mut p = Vec2::new(99.0, 0.0);
        assert!(keep_inside(&mut p, Vec2::new(-10.0, 0.0), 5.0, &ring(100.0)).is_none());
        assert!((p.x - 94.0).abs() < 1e-4);
    }

    #[test]
    fn a_ball_that_was_inside_is_pulled_back_in() {
        for outline in [
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 0, angle: 0.0, thickness: 2.0 },
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 6, angle: 0.3, thickness: 2.0 },
            Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 60.0), angle: 0.2, thickness: 2.0 },
            Outline::Path { center: Vec2::ZERO, angle: 0.2, scale: 100.0, points: l_shape().into(), thickness: 2.0 },
        ] {
            let wall = hollow(outline.clone());
            let mut p = Vec2::new(900.0, 700.0);
            keep_clear(&mut p, Vec2::new(-30.0, 20.0), Vec2::ZERO, 5.0, &wall);
            assert!(p.is_finite() && outline.inside(p), "{outline:?} left the ball at {p}");
            // Settled: a second pass has nothing to correct.
            let settled = p;
            assert!(keep_clear(&mut p, settled, Vec2::ZERO, 5.0, &wall).is_none());
            assert!((p - settled).length() < 1e-3, "{outline:?}: {p} moved from {settled}");
        }
    }

    #[test]
    fn an_arena_smaller_than_the_ball_pins_it_without_nan() {
        let mut p = Vec2::new(3.0, 1.0);
        let c = keep_inside(&mut p, Vec2::new(50.0, 0.0), 20.0, &ring(10.0));
        assert!(p.is_finite() && p.length() < 1e-3);
        assert!(c.is_none_or(|c| c.point.is_finite() && c.normal.is_finite()));
    }

    #[test]
    fn a_spinning_wall_carries_the_ball() {
        let wall = Wall { spin: 1.0, ..ring(100.0) };
        let mut p = Vec2::new(99.0, 0.0);
        let c = keep_inside(&mut p, Vec2::new(10.0, 0.0), 5.0, &wall).unwrap();
        // The wall at (99, 0) turning clockwise (y down) moves in +y at 99 px/s.
        assert!((c.wall_velocity.y - 99.0).abs() < 1.0 && c.wall_velocity.x.abs() < 1e-3);
    }

    #[test]
    fn a_box_keeps_the_hardest_contact() {
        let wall =
            hollow(Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 100.0), angle: 0.0, thickness: 2.0 });
        let mut p = Vec2::new(99.0, 99.0);
        let c = keep_inside(&mut p, Vec2::new(10.0, 40.0), 5.0, &wall).unwrap();
        assert!((p.x - 94.0).abs() < 1e-4 && (p.y - 94.0).abs() < 1e-4);
        assert_eq!(c.normal, Vec2::new(0.0, -1.0));
        assert!((c.speed - 40.0).abs() < 1e-4);
    }

    #[test]
    fn inside_and_point_agree_on_the_outline() {
        let outline =
            Outline::Ring { center: Vec2::new(10.0, 0.0), radius: 50.0, sides: 6, angle: 0.3, thickness: 2.0 };
        assert!(outline.inside(Vec2::new(10.0, 0.0)));
        assert!(!outline.inside(Vec2::new(100.0, 0.0)));
        let rim = outline.point(1.0);
        assert!(!outline.inside(rim + (rim - outline.center()).normalize()));
        assert!(outline.inside(rim - (rim - outline.center()).normalize()));
        assert_eq!(outline.thickness(), 2.0);
        // A free outline: the furthest crossing of the ray.
        let l = path(l_shape(), 100.0, 0.4, 2.0);
        for u in [0.0, 0.7, 2.0, 4.0, 5.5] {
            let rim = l.point(u);
            let out = (rim - l.center()).normalize();
            assert!(!l.inside(rim + out) && l.inside(rim - out), "u {u}: {rim}");
        }
        assert_eq!((l.angle(), l.thickness(), l.center()), (0.4, 2.0, Vec2::new(10.0, -20.0)));
    }

    #[test]
    fn a_ball_that_does_not_fit_is_pinned_without_a_contact() {
        let mut p = Vec2::new(3.0, 1.0);
        assert!(keep_inside(&mut p, Vec2::new(50.0, 0.0), 20.0, &ring(10.0)).is_none());
        assert!(p.length() < 1e-3);
        // A box too low for the ball pins it on that axis only; it still bounces off the ends.
        let wall = hollow(Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 5.0), angle: 0.0, thickness: 2.0 });
        let mut p = Vec2::new(99.0, 3.0);
        let c = keep_inside(&mut p, Vec2::new(50.0, 10.0), 20.0, &wall).expect("the end wall is hit");
        assert!(p.y.abs() < 1e-3 && (p.x - 79.0).abs() < 1e-3, "{p:?}");
        assert!((c.normal.x + 1.0).abs() < 1e-3 && c.normal.y.abs() < 1e-3, "{c:?}");
    }

    #[test]
    fn even_polygon_corners_match_the_ring_polygon() {
        // The Shape thing builds `Ring { angle: turn + FRAC_PI_2 }` for an even polygon with no
        // moulded outline; its corners must be `ShapePath::even`'s, scaled.
        for n in 3..=12u32 {
            let ring = Outline::Ring {
                center: Vec2::new(5.0, 7.0),
                radius: 450.0,
                sides: n,
                angle: FRAC_PI_2,
                thickness: 3.0,
            };
            let corners = ring.corners();
            let even = ShapePath::even(n);
            for k in 0..n as usize {
                let want = Vec2::new(5.0, 7.0) + even.point(k) * 450.0;
                assert!(
                    corners.iter().any(|c| (*c - want).length() < 1e-2),
                    "{n} sides: corner {k} {want} not in {corners:?}"
                );
            }
        }
    }

    #[test]
    fn a_ball_outside_a_hollow_ring_bounces_off_its_outside() {
        let wall = ring(100.0);
        let mut p = Vec2::new(104.0, 0.0);
        let c = keep_clear(&mut p, Vec2::new(110.0, 0.0), Vec2::new(-50.0, 0.0), 5.0, &wall).expect("hit");
        assert!((p.x - 106.0).abs() < 1e-4, "{p}");
        assert!((c.normal - Vec2::X).length() < 1e-4 && (c.speed - 50.0).abs() < 1e-3, "{c:?}");
        assert!((c.point.x - 101.0).abs() < 1e-4);
        // Clear of it: nothing happens.
        let mut far = Vec2::new(120.0, 0.0);
        let at = far;
        assert!(keep_clear(&mut far, at, Vec2::new(-50.0, 0.0), 5.0, &wall).is_none());
        assert_eq!(far, Vec2::new(120.0, 0.0));
        // One that got all the way in during the step goes back out.
        let mut through = Vec2::new(90.0, 0.0);
        keep_clear(&mut through, Vec2::new(110.0, 0.0), Vec2::new(-5000.0, 0.0), 5.0, &wall).unwrap();
        assert!((through.x - 106.0).abs() < 1e-4, "{through}");
    }

    #[test]
    fn a_ball_inside_a_hollow_path_stays_inside() {
        let outline = path(l_shape(), 100.0, 0.0, 4.0);
        let wall = hollow(outline.clone());
        let start = Vec2::new(10.0 - 30.0, -20.0 + 30.0);
        // Moving down into the bottom side (y = 50 in the world).
        let mut p = Vec2::new(-20.0, 49.0);
        let c = keep_clear(&mut p, start, Vec2::new(0.0, 80.0), 5.0, &wall).expect("hit");
        assert!((p.y - 43.0).abs() < 1e-3, "{p}");
        assert!((c.normal - Vec2::new(0.0, -1.0)).length() < 1e-4, "{c:?}");
        assert!(outline.inside(p));
        // Into the inner corner of the L: pushed out of the nearest side, still inside.
        let mut q = Vec2::new(43.0, -53.0);
        keep_clear(&mut q, start, Vec2::new(10.0, -10.0), 5.0, &wall);
        assert!(outline.inside(q), "{q}");
        let (near, _) = outline.nearest(q);
        assert!((q - near).length() > 7.0 - 1e-3, "{q} is {} from {near}", (q - near).length());
    }

    #[test]
    fn a_solid_box_keeps_balls_out() {
        let solid = Wall {
            solid: true,
            ..hollow(Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 50.0), angle: 0.0, thickness: 0.0 })
        };
        // From outside, moving in.
        let mut p = Vec2::new(0.0, -53.0);
        let c = keep_clear(&mut p, Vec2::new(0.0, -80.0), Vec2::new(0.0, 30.0), 5.0, &solid).expect("hit");
        assert!((p.y + 55.0).abs() < 1e-4, "{p}");
        assert!((c.normal - Vec2::new(0.0, -1.0)).length() < 1e-4 && (c.speed - 30.0).abs() < 1e-3);
        // Starting inside: pushed out of the nearest edge (the right one).
        let mut q = Vec2::new(90.0, 10.0);
        let at = q;
        keep_clear(&mut q, at, Vec2::ZERO, 5.0, &solid);
        assert!((q - Vec2::new(105.0, 10.0)).length() < 1e-4, "{q}");
        // A solid circle too.
        let disc = Wall { solid: true, ..ring(100.0) };
        let mut r = Vec2::new(0.0, 50.0);
        let at = r;
        keep_clear(&mut r, at, Vec2::ZERO, 5.0, &disc);
        assert!((r - Vec2::new(0.0, 105.0)).length() < 1e-3, "{r}");
        // Right in the middle of the circle: still pushed out, no NaN.
        let mut m = Vec2::ZERO;
        let at = m;
        keep_clear(&mut m, at, Vec2::ZERO, 5.0, &disc);
        assert!(m.is_finite() && (m.length() - 105.0).abs() < 1e-3, "{m}");
    }

    #[test]
    fn a_ball_on_the_line_goes_to_its_own_side() {
        let wall = hollow(path(l_shape(), 100.0, 0.0, 2.0));
        let on_line = Vec2::new(-60.0, -20.0);
        let mut p = on_line;
        keep_clear(&mut p, Vec2::new(-40.0, -20.0), Vec2::ZERO, 5.0, &wall);
        assert!((p - Vec2::new(-54.0, -20.0)).length() < 1e-3, "inside: {p}");
        let mut q = on_line;
        keep_clear(&mut q, Vec2::new(-140.0, -20.0), Vec2::ZERO, 5.0, &wall);
        assert!((q - Vec2::new(-66.0, -20.0)).length() < 1e-3, "outside: {q}");
    }

    #[test]
    fn nearest_agrees_with_brute_force() {
        let outlines = [
            Outline::Ring { center: Vec2::new(3.0, 4.0), radius: 80.0, sides: 0, angle: 0.3, thickness: 2.0 },
            Outline::Ring { center: Vec2::new(3.0, 4.0), radius: 80.0, sides: 7, angle: 0.3, thickness: 2.0 },
            Outline::Box { center: Vec2::new(3.0, 4.0), half: Vec2::new(80.0, 30.0), angle: 0.7, thickness: 2.0 },
            path(l_shape(), 70.0, -0.6, 2.0),
        ];
        let mut seed = 12345u32;
        let mut rand = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * 300.0 - 150.0
        };
        for outline in &outlines {
            // The outline traced finely.
            let trace: Vec<Vec2> = match outline {
                Outline::Path { center, angle, scale, points, .. } => (0..points.len())
                    .flat_map(|i| {
                        let (a, b) = (points[i], points[(i + 1) % points.len()]);
                        (0..2000).map(move |k| a.lerp(b, k as f32 / 2000.0))
                    })
                    .map(|q| *center + Vec2::from_angle(*angle).rotate(q * *scale))
                    .collect(),
                _ => (0..20000).map(|k| outline.point(k as f32 / 20000.0 * TAU)).collect(),
            };
            for _ in 0..50 {
                let p = Vec2::new(rand(), rand());
                let (q, n) = outline.nearest(p);
                let brute = trace.iter().map(|t| (*t - p).length()).fold(f32::INFINITY, f32::min);
                assert!(((q - p).length() - brute).abs() < 0.2, "{outline:?} at {p}: {} vs {brute}", (q - p).length());
                assert!((n.length() - 1.0).abs() < 1e-4);
                assert!((p - q).dot(n) >= -1e-3);
            }
        }
    }
}
