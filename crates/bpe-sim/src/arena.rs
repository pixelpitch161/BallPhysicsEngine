//! Arena geometry and collisions: closed arenas (ring, box, polygon) that keep balls inside,
//! open walls (rings with gaps, pegs, lines) that balls bounce off from either side, and the
//! shapes the renderer draws for them.

use crate::Ball;
use bpe_scene::{Container, Vec2, ring_radius};
use std::f32::consts::{PI, TAU};

/// The arena's turn and size at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArenaState {
    /// Radians the whole arena has turned, clockwise.
    pub angle: f32,
    /// Size compared to the scene's arena size, after shrinking.
    pub scale: f32,
    /// Change of `scale` per second right now, so moving walls can push balls.
    pub scale_rate: f32,
    /// Radians per second the whole arena turns.
    pub spin: f32,
}

impl Default for ArenaState {
    fn default() -> Self {
        Self { angle: 0.0, scale: 1.0, scale_rate: 0.0, spin: 0.0 }
    }
}

/// An outline to draw: a ring or regular polygon (optionally with a gap), or a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outline {
    Ring {
        center: Vec2,
        /// Radius, or the corner radius of a polygon.
        radius: f32,
        /// 0 for round, otherwise 3 or more.
        sides: u32,
        /// Direction of the gap's middle, radians clockwise from +x. For a polygon this is also
        /// the direction of one side's outward normal.
        angle: f32,
        /// Half the gap's width in radians. 0 means no gap.
        gap_half: f32,
        thickness: f32,
    },
    Box {
        center: Vec2,
        half: Vec2,
        angle: f32,
        thickness: f32,
    },
}

impl Outline {
    /// For a ring or polygon, the point on the outline `u` radians clockwise from the gap's
    /// middle. For a box, the point in that direction from its centre.
    pub fn point(&self, u: f32) -> Vec2 {
        match *self {
            Outline::Ring { center, radius, sides, angle, .. } => {
                if sides < 3 {
                    return center + dir(angle + u) * radius;
                }
                let step = TAU / sides as f32;
                let side = (u / step).round();
                let apothem = radius * (PI / sides as f32).cos();
                center + dir(angle + u) * (apothem / (u - side * step).cos())
            }
            Outline::Box { center, half, angle, .. } => {
                let d = dir(u);
                let scale = (half.x / d.x.abs().max(1e-6)).min(half.y / d.y.abs().max(1e-6));
                let local = d * scale;
                center + dir(angle) * local.x + dir(angle).perp() * local.y
            }
        }
    }

    pub fn center(&self) -> Vec2 {
        match *self {
            Outline::Ring { center, .. } | Outline::Box { center, .. } => center,
        }
    }

    /// The same outline with its gap closed.
    pub fn ring_without_gap(outline: Outline) -> Outline {
        match outline {
            Outline::Ring { center, radius, sides, angle, thickness, .. } => {
                Outline::Ring { center, radius, sides, angle, gap_half: 0.0, thickness }
            }
            other => other,
        }
    }

    pub fn thickness(&self) -> f32 {
        match *self {
            Outline::Ring { thickness, .. } | Outline::Box { thickness, .. } => thickness,
        }
    }
}

/// One ring of a `Container::Rings` arena.
pub fn ring_outline(container: &Container, index: u32, time: f64, arena: ArenaState) -> Option<Outline> {
    let Container::Rings {
        center,
        count,
        inner_radius,
        spacing,
        sides,
        gap,
        spin,
        spin_step,
        gap_offset,
        thickness,
        ..
    } = *container
    else {
        return None;
    };
    if index >= count {
        return None;
    }
    let k = index as f64;
    let degrees = (spin as f64 + k * spin_step as f64) * time + k * gap_offset as f64;
    let angle = (degrees.to_radians() - std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU) as f32;
    Some(Outline::Ring {
        center,
        radius: ring_radius(inner_radius, spacing, index) * arena.scale,
        sides: if sides >= 3 { sides } else { 0 },
        angle: (angle + arena.angle).rem_euclid(TAU),
        gap_half: (gap.clamp(0.0, 360.0) * 0.5).to_radians(),
        thickness,
    })
}

/// Spin of ring `index` in radians per second, including the arena's own spin.
pub fn ring_spin(container: &Container, index: u32, arena: ArenaState) -> f32 {
    match *container {
        Container::Rings { spin, spin_step, .. } => (spin + spin_step * index as f32).to_radians() + arena.spin,
        _ => arena.spin,
    }
}

/// The outline of a closed arena (ring, box or polygon) right now.
pub fn closed_outline(container: &Container, arena: ArenaState) -> Option<Outline> {
    match *container {
        Container::Circle { center, radius, thickness, .. } => {
            Some(Outline::Ring { center, radius: radius * arena.scale, sides: 0, angle: 0.0, gap_half: 0.0, thickness })
        }
        Container::Rect { center, size, thickness, .. } => {
            Some(Outline::Box { center, half: size * 0.5 * arena.scale, angle: arena.angle, thickness })
        }
        Container::Polygon { center, radius, sides, rotation, thickness, .. } => Some(Outline::Ring {
            center,
            radius: radius * arena.scale,
            sides: sides.max(3),
            angle: (rotation.to_radians() + PI * 0.5 + arena.angle).rem_euclid(TAU),
            gap_half: 0.0,
            thickness,
        }),
        // Rings are open walls, and custom shapes have their own collisions (see `shapes`).
        Container::Rings { .. } | Container::Custom { .. } => None,
    }
}

fn dir(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

/// Velocity of a wall point that turns at `spin` rad/s and scales at `growth` per second
/// around `center`.
pub(crate) fn wall_velocity(point: Vec2, center: Vec2, spin: f32, growth: f32) -> Vec2 {
    let r = point - center;
    r.perp() * spin + r * growth
}

/// Bounces the ball off a wall whose surface normal (pointing towards the ball) is `normal`
/// and which moves at `wall`. Returns the impact speed if the ball was moving into the wall.
pub(crate) fn bounce(ball: &mut Ball, normal: Vec2, wall: Vec2, restitution: f32) -> Option<f32> {
    let relative = ball.velocity - wall;
    let into = -relative.dot(normal);
    if into <= 0.0 {
        return None;
    }
    ball.velocity = relative + normal * into * (1.0 + restitution) + wall;
    Some(into)
}

/// Keeps a ball inside a closed arena. Returns the contact point and impact speed of the
/// hardest hit, if the ball was moving into a wall.
// Inlined so a loop over every ball works out a box's or polygon's directions only once.
#[inline(always)]
pub fn collide_closed(ball: &mut Ball, outline: &Outline, arena: ArenaState, restitution: f32) -> Option<(Vec2, f32)> {
    let growth = arena.scale_rate / arena.scale.max(1e-3);
    match *outline {
        Outline::Ring { center, radius, sides: 0, thickness, .. } => {
            let inner = radius - thickness * 0.5 - ball.radius;
            let offset = ball.position - center;
            let distance = offset.length();
            if distance <= inner || distance == 0.0 {
                return None;
            }
            let normal = offset / distance;
            ball.position = center + normal * inner.max(0.0);
            let contact = ball.position + normal * ball.radius;
            let wall = wall_velocity(contact, center, arena.spin, growth);
            bounce(ball, -normal, wall, restitution).map(|speed| (contact, speed))
        }
        Outline::Ring { center, radius, sides, angle, thickness, .. } => {
            let apothem = radius * (PI / sides as f32).cos();
            let planes = (0..sides).map(|i| (dir(angle + TAU * i as f32 / sides as f32), apothem));
            collide_planes(ball, center, planes, thickness, arena.spin, growth, restitution)
        }
        Outline::Box { center, half, angle, thickness } => {
            let (x, y) = (dir(angle), dir(angle).perp());
            let planes = [(x, half.x), (-x, half.x), (y, half.y), (-y, half.y)];
            collide_planes(ball, center, planes.into_iter(), thickness, arena.spin, growth, restitution)
        }
    }
}

/// Keeps a ball on the inner side of each plane `(outward normal, distance from centre)`.
#[inline(always)]
fn collide_planes(
    ball: &mut Ball,
    center: Vec2,
    planes: impl Iterator<Item = (Vec2, f32)>,
    thickness: f32,
    spin: f32,
    growth: f32,
    restitution: f32,
) -> Option<(Vec2, f32)> {
    let mut hit: Option<(Vec2, f32)> = None;
    for (normal, distance) in planes {
        let limit = distance - thickness * 0.5 - ball.radius;
        let excess = (ball.position - center).dot(normal) - limit;
        if excess <= 0.0 {
            continue;
        }
        ball.position -= normal * excess;
        let contact = ball.position + normal * ball.radius;
        let wall = wall_velocity(contact, center, spin, growth);
        if let Some(speed) = bounce(ball, -normal, wall, restitution)
            && hit.is_none_or(|(_, s)| speed > s)
        {
            hit = Some((contact, speed));
        }
    }
    hit
}

/// A piece of open wall that balls bounce off from either side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// Part of a circle, from `start` clockwise through `sweep` radians.
    Arc {
        center: Vec2,
        radius: f32,
        start: f32,
        sweep: f32,
    },
    Segment {
        from: Vec2,
        to: Vec2,
    },
}

/// The wall pieces of a ring or polygon outline with a gap.
pub fn outline_pieces(outline: &Outline, out: &mut Vec<Piece>) {
    match *outline {
        Outline::Ring { center, radius, sides, angle, gap_half, .. } => {
            if gap_half >= PI {
                return;
            }
            if sides < 3 {
                out.push(Piece::Arc { center, radius, start: angle + gap_half, sweep: TAU - 2.0 * gap_half });
                return;
            }
            // Walk around the outline from one side of the gap to the other, breaking at corners.
            let step = TAU / sides as f32;
            let mut breaks = vec![gap_half];
            breaks.extend((0..sides).map(|i| step * (i as f32 + 0.5)).filter(|&u| u > gap_half && u < TAU - gap_half));
            breaks.push(TAU - gap_half);
            for pair in breaks.windows(2) {
                if pair[1] - pair[0] > 1e-6 {
                    out.push(Piece::Segment { from: outline.point(pair[0]), to: outline.point(pair[1]) });
                }
            }
        }
        Outline::Box { center, half, angle, .. } => {
            let (x, y) = (dir(angle) * half.x, dir(angle).perp() * half.y);
            let corners = [center + x + y, center - x + y, center - x - y, center + x - y];
            for i in 0..4 {
                out.push(Piece::Segment { from: corners[i], to: corners[(i + 1) % 4] });
            }
        }
    }
}

impl Piece {
    fn closest(&self, p: Vec2) -> Vec2 {
        match *self {
            Piece::Arc { center, radius, start, sweep } => {
                let offset = p - center;
                let along = (offset.y.atan2(offset.x) - start).rem_euclid(TAU);
                if along <= sweep && offset != Vec2::ZERO {
                    center + offset.normalize() * radius
                } else {
                    let (a, b) = (center + dir(start) * radius, center + dir(start + sweep) * radius);
                    if a.distance_squared(p) <= b.distance_squared(p) { a } else { b }
                }
            }
            Piece::Segment { from, to } => {
                let d = to - from;
                let t = if d == Vec2::ZERO { 0.0 } else { ((p - from).dot(d) / d.length_squared()).clamp(0.0, 1.0) };
                from + d * t
            }
        }
    }

    /// If the ball's centre moved through the wall between `prev` and `now`, the point on the
    /// wall's line nearest `now` and the wall normal on the side it came from.
    fn crossing(&self, prev: Vec2, now: Vec2) -> Option<(Vec2, Vec2)> {
        match *self {
            Piece::Arc { center, radius, start, sweep } => {
                let (a, b) = (prev.distance(center) - radius, now.distance(center) - radius);
                if (a < 0.0) == (b < 0.0) || a == b {
                    return None;
                }
                let t = a / (a - b);
                let hit = prev + (now - prev) * t - center;
                if (hit.y.atan2(hit.x) - start).rem_euclid(TAU) > sweep {
                    return None;
                }
                let radial = (now - center).try_normalize().or(hit.try_normalize())?;
                let normal = if a > 0.0 { radial } else { -radial };
                Some((center + radial * radius, normal))
            }
            Piece::Segment { from, to } => {
                let d = to - from;
                let side = |p: Vec2| d.perp_dot(p - from);
                let (a, b) = (side(prev), side(now));
                if (a < 0.0) == (b < 0.0) || a == b {
                    return None;
                }
                let hit = prev + (now - prev) * (a / (a - b));
                let t = (hit - from).dot(d) / d.length_squared().max(1e-12);
                if !(0.0..=1.0).contains(&t) {
                    return None;
                }
                let perp = d.perp().try_normalize()?;
                let normal = if a > 0.0 { perp } else { -perp };
                let on_line = from + d * ((now - from).dot(d) / d.length_squared());
                Some((on_line, normal))
            }
        }
    }

    /// Rough distance below which the piece might touch a ball, for skipping far pieces.
    fn bounds(&self) -> (Vec2, f32) {
        match *self {
            Piece::Arc { center, radius, .. } => (center, radius),
            Piece::Segment { from, to } => ((from + to) * 0.5, from.distance(to) * 0.5),
        }
    }
}

/// Bounces a ball off an open wall piece of the given thickness, from whichever side it is on.
/// `prev` is where the ball was before this step. `spin` and `growth` describe how the piece
/// moves around `pivot`.
#[allow(clippy::too_many_arguments)]
pub fn collide_piece(
    ball: &mut Ball,
    prev: Vec2,
    piece: &Piece,
    thickness: f32,
    pivot: Vec2,
    spin: f32,
    growth: f32,
    restitution: f32,
) -> Option<(Vec2, f32)> {
    let reach = thickness * 0.5 + ball.radius;
    let (middle, size) = piece.bounds();
    if let Piece::Arc { .. } = piece {
        // Only a band around the circle matters, unless the ball jumped across it.
        let (before, after) = (prev.distance(middle) - size, ball.position.distance(middle) - size);
        if after.abs() > reach && (before < 0.0) == (after < 0.0) {
            return None;
        }
    } else if ball.position.distance(middle) > size + reach && prev.distance(middle) > size + reach {
        return None;
    }
    let (wall_point, normal) = match piece.crossing(prev, ball.position) {
        Some((on_line, normal)) => {
            ball.position = on_line + normal * reach;
            (on_line, normal)
        }
        None => {
            let closest = piece.closest(ball.position);
            let offset = ball.position - closest;
            let distance = offset.length();
            if distance >= reach {
                return None;
            }
            let normal = if distance > 1e-6 {
                offset / distance
            } else {
                (prev - closest).try_normalize().unwrap_or(Vec2::NEG_Y)
            };
            ball.position = closest + normal * reach;
            (closest, normal)
        }
    };
    let wall = wall_velocity(wall_point, pivot, spin, growth);
    let contact = ball.position - normal * ball.radius;
    bounce(ball, normal, wall, restitution).map(|speed| (contact, speed))
}

/// Bounces a ball off a round peg from outside. `prev` is where the ball was before this step,
/// so a small, fast ball that jumped over the peg in one step still hits it.
pub fn collide_disc(ball: &mut Ball, prev: Vec2, center: Vec2, radius: f32, restitution: f32) -> Option<(Vec2, f32)> {
    let reach = radius + ball.radius;
    let offset = ball.position - center;
    let distance_squared = offset.length_squared();
    if distance_squared >= reach * reach {
        // Did the path from `prev` pass through the peg? Find where it first touched.
        let path = ball.position - prev;
        let start = prev - center;
        let (a, b, c) = (path.length_squared(), start.dot(path), start.length_squared() - reach * reach);
        let discriminant = b * b - a * c;
        if a < 1e-9 || c <= 0.0 || b >= 0.0 || discriminant < 0.0 {
            return None;
        }
        let t = (-b - discriminant.sqrt()) / a;
        if t > 1.0 {
            return None;
        }
        ball.position = prev + path * t;
    }
    let offset = ball.position - center;
    let distance = offset.length();
    let normal = if distance > 1e-6 { offset / distance } else { Vec2::NEG_Y };
    ball.position = center + normal * reach;
    bounce(ball, normal, Vec2::ZERO, restitution).map(|speed| (center + normal * radius, speed))
}

/// Whether `point` is inside a ring or polygon outline (ignoring any gap).
pub fn inside(outline: &Outline, point: Vec2) -> bool {
    match *outline {
        Outline::Ring { center, radius, sides, angle, .. } => {
            let offset = point - center;
            if sides < 3 {
                return offset.length_squared() < radius * radius;
            }
            let apothem = radius * (PI / sides as f32).cos();
            (0..sides).all(|i| offset.dot(dir(angle + TAU * i as f32 / sides as f32)) < apothem)
        }
        Outline::Box { center, half, angle, .. } => {
            let offset = point - center;
            offset.dot(dir(angle)).abs() < half.x && offset.dot(dir(angle).perp()).abs() < half.y
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_pieces_meet_at_the_gap_and_corners() {
        let outline =
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 6, angle: 0.3, gap_half: 0.2, thickness: 2.0 };
        let mut pieces = Vec::new();
        outline_pieces(&outline, &mut pieces);
        // The gap cuts one side in two, so 7 pieces, each starting where the last ended.
        assert_eq!(pieces.len(), 7);
        for pair in pieces.windows(2) {
            let (Piece::Segment { to, .. }, Piece::Segment { from, .. }) = (pair[0], pair[1]) else { panic!() };
            assert!(to.distance(from) < 1e-3);
        }
        // All points lie on the hexagon's outline.
        let apothem = 100.0 * (PI / 6.0).cos();
        for piece in &pieces {
            let Piece::Segment { from, to } = *piece else { panic!() };
            let mid = (from + to) * 0.5;
            let max = (0..6).map(|i| mid.dot(dir(0.3 + TAU * i as f32 / 6.0))).fold(f32::MIN, f32::max);
            assert!((max - apothem).abs() < 1e-2, "{max} vs {apothem}");
        }
    }

    #[test]
    fn fast_ball_cannot_tunnel_through_a_thin_wall() {
        let piece = Piece::Segment { from: Vec2::new(-100.0, 0.0), to: Vec2::new(100.0, 0.0) };
        let mut ball = Ball {
            position: Vec2::new(0.0, 30.0),
            velocity: Vec2::new(0.0, 3000.0),
            radius: 5.0,
            color: [1.0; 4],
            wall_hits: 0,
            group: 0,
            alive: true,
        };
        let prev = Vec2::new(0.0, -10.0);
        let hit = collide_piece(&mut ball, prev, &piece, 2.0, Vec2::ZERO, 0.0, 0.0, 1.0);
        assert!(hit.is_some());
        assert!(ball.position.y < 0.0 && ball.velocity.y < 0.0);
    }

    #[test]
    fn ball_passes_through_a_ring_gap() {
        let outline =
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 0, angle: 0.0, gap_half: 0.5, thickness: 2.0 };
        let mut pieces = Vec::new();
        outline_pieces(&outline, &mut pieces);
        let mut ball = Ball {
            position: Vec2::new(104.0, 0.0),
            velocity: Vec2::new(500.0, 0.0),
            radius: 5.0,
            color: [1.0; 4],
            wall_hits: 0,
            group: 0,
            alive: true,
        };
        assert!(collide_piece(&mut ball, Vec2::new(96.0, 0.0), &pieces[0], 2.0, Vec2::ZERO, 0.0, 0.0, 1.0).is_none());
        assert_eq!(ball.position, Vec2::new(104.0, 0.0));
        // The same move on the other side, where the ring is solid, bounces back.
        ball.position = Vec2::new(-104.0, 0.0);
        ball.velocity = Vec2::new(-500.0, 0.0);
        assert!(collide_piece(&mut ball, Vec2::new(-96.0, 0.0), &pieces[0], 2.0, Vec2::ZERO, 0.0, 0.0, 1.0).is_some());
        assert!(ball.position.x > -100.0 && ball.velocity.x > 0.0);
    }

    #[test]
    fn small_fast_ball_cannot_skip_a_small_peg() {
        let mut ball = Ball {
            position: Vec2::new(8.0, 0.0),
            velocity: Vec2::new(3000.0, 0.0),
            radius: 0.5,
            color: [1.0; 4],
            wall_hits: 0,
            group: 0,
            alive: true,
        };
        let hit = collide_disc(&mut ball, Vec2::new(-4.5, 0.0), Vec2::ZERO, 1.0, 1.0);
        assert!(hit.is_some());
        assert!(ball.position.x < 0.0 && ball.velocity.x < 0.0);
    }
}
