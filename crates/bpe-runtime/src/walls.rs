//! Closed walls (rings, polygons, boxes, rounded boxes) and how a ball meets them.
//!
//! The maths is the old editor's closed-arena collision, split in two: [`keep_inside`] moves a
//! ball back inside and reports the contact, and the mover's Bounce module decides what the
//! velocity does with [`reflect`].

use std::f32::consts::{PI, TAU};

use glam::Vec2;

/// A closed outline: a ring or regular polygon, a box, or a box with round corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outline {
    Ring {
        center: Vec2,
        /// Radius, or the corner radius of a polygon.
        radius: f32,
        /// 0 for round, otherwise 3 or more.
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
    /// A box whose corners are rounded with `corner`, the radius of the centre line's corner
    /// arcs; `corner == half.min_element()` makes a capsule.
    Rounded {
        center: Vec2,
        half: Vec2,
        corner: f32,
        angle: f32,
        thickness: f32,
    },
}

/// Signed distance from `local` (relative to the centre, in the box's own frame) to the centre
/// line of a box with half size `half` and corners rounded by `corner`; negative inside.
fn rounded_distance(local: Vec2, half: Vec2, corner: f32) -> f32 {
    let q = local.abs() - (half - Vec2::splat(corner));
    q.max(Vec2::ZERO).length() + q.max_element().min(0.0) - corner
}

/// A wall that balls stay inside of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub outline: Outline,
    /// Radians per second, clockwise.
    pub spin: f32,
    /// Fraction of its size the wall grows per second; negative while shrinking.
    pub growth: f32,
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

impl Outline {
    /// For a ring or polygon, the point on the outline `u` radians clockwise from `angle`. For a
    /// box, the point on its edge in that direction from its centre.
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
            Outline::Rounded { center, half, corner, angle, .. } => {
                // Between the centre and the sharp box's edge the distance changes sign once:
                // halve the gap until it is found.
                let d = dir(u);
                let mut far = (half.x / d.x.abs().max(1e-6)).min(half.y / d.y.abs().max(1e-6));
                let mut near = 0.0;
                for _ in 0..24 {
                    let mid = (near + far) * 0.5;
                    if rounded_distance(d * mid, half, corner) < 0.0 {
                        near = mid;
                    } else {
                        far = mid;
                    }
                }
                let local = d * (near + far) * 0.5;
                center + dir(angle) * local.x + dir(angle).perp() * local.y
            }
        }
    }

    pub fn center(&self) -> Vec2 {
        match *self {
            Outline::Ring { center, .. } | Outline::Box { center, .. } | Outline::Rounded { center, .. } => center,
        }
    }

    pub fn thickness(&self) -> f32 {
        match *self {
            Outline::Ring { thickness, .. } | Outline::Box { thickness, .. } | Outline::Rounded { thickness, .. } => {
                thickness
            }
        }
    }

    /// Radians clockwise from +x that the outline is turned.
    pub fn angle(&self) -> f32 {
        match *self {
            Outline::Ring { angle, .. } | Outline::Box { angle, .. } | Outline::Rounded { angle, .. } => angle,
        }
    }

    /// Whether `p` is inside the outline's centre line.
    pub fn inside(&self, p: Vec2) -> bool {
        match *self {
            Outline::Ring { center, radius, sides, angle, .. } => {
                let offset = p - center;
                if sides < 3 {
                    return offset.length_squared() < radius * radius;
                }
                let apothem = radius * (PI / sides as f32).cos();
                (0..sides).all(|i| offset.dot(dir(angle + TAU * i as f32 / sides as f32)) < apothem)
            }
            Outline::Box { center, half, angle, .. } => {
                let offset = p - center;
                offset.dot(dir(angle)).abs() < half.x && offset.dot(dir(angle).perp()).abs() < half.y
            }
            Outline::Rounded { center, half, corner, angle, .. } => {
                let offset = p - center;
                let local = Vec2::new(offset.dot(dir(angle)), offset.dot(dir(angle).perp()));
                rounded_distance(local, half, corner) < 0.0
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
/// when the ball is moving into it.
fn contact(position: Vec2, velocity: Vec2, radius: f32, outward: Vec2, wall: &Wall) -> Option<Contact> {
    let point = position + outward * radius;
    let wall_velocity = wall_velocity(point, wall.outline.center(), wall.spin, wall.growth);
    let speed = (velocity - wall_velocity).dot(outward);
    (speed > 0.0).then_some(Contact { point, normal: -outward, speed, wall_velocity })
}

/// Moves a ball back inside the wall and reports the hardest contact it was moving into, if
/// any. The velocity is left alone: the ball's Bounce module answers with [`reflect`]. A ball
/// too big for the wall is held at the centre and never touches it.
// Inlined so a loop over every ball works out a box's or polygon's directions only once.
#[inline(always)]
pub fn keep_inside(position: &mut Vec2, velocity: Vec2, radius: f32, wall: &Wall) -> Option<Contact> {
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
        Outline::Rounded { center, half, corner, angle, thickness } => {
            let (x, y) = (dir(angle), dir(angle).perp());
            let offset = *position - center;
            let mut local = Vec2::new(offset.dot(x), offset.dot(y));
            // The ball's centre may roam a smaller rounded box: this one shrunk by the wall's
            // half thickness and the ball's radius. An axis the ball doesn't fit across pins it
            // on the centre line, like a box does.
            let inner = half - Vec2::splat(thickness * 0.5 + radius);
            let pinned = Vec2::new(if inner.x < 0.0 { 0.0 } else { 1.0 }, if inner.y < 0.0 { 0.0 } else { 1.0 });
            local *= pinned;
            let inner = inner.max(Vec2::ZERO);
            let round = (corner - thickness * 0.5 - radius).clamp(0.0, inner.min_element());
            let core = inner - Vec2::splat(round);
            let nearest = local.clamp(-core, core);
            let away = local - nearest;
            let distance = away.length();
            if distance <= round + 1e-4 {
                // In bounds (a hair's breadth out counts, so a settled ball stays put).
                if pinned != Vec2::ONE {
                    *position = center + x * local.x + y * local.y;
                }
                return None;
            }
            let outward_local = away / distance;
            let local = nearest + outward_local * round;
            *position = center + x * local.x + y * local.y;
            let outward = x * outward_local.x + y * outward_local.y;
            contact(*position, velocity, radius, outward, wall)
        }
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
    use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

    fn ring(radius: f32) -> Wall {
        Wall {
            outline: Outline::Ring { center: Vec2::ZERO, radius, sides: 0, angle: 0.0, thickness: 2.0 },
            spin: 0.0,
            growth: 0.0,
        }
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
    fn a_ball_starting_outside_is_pulled_in() {
        for outline in [
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 0, angle: 0.0, thickness: 2.0 },
            Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 6, angle: 0.3, thickness: 2.0 },
            Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 60.0), angle: 0.2, thickness: 2.0 },
            Outline::Rounded {
                center: Vec2::ZERO,
                half: Vec2::new(100.0, 60.0),
                corner: 30.0,
                angle: 0.2,
                thickness: 2.0,
            },
        ] {
            let wall = Wall { outline, spin: 0.0, growth: 0.0 };
            let mut p = Vec2::new(900.0, 700.0);
            keep_inside(&mut p, Vec2::ZERO, 5.0, &wall);
            assert!(p.is_finite() && outline.inside(p), "{outline:?} left the ball at {p}");
            // Settled: a second pass has nothing to correct.
            let settled = p;
            assert!(keep_inside(&mut p, Vec2::ZERO, 5.0, &wall).is_none());
            assert_eq!(p, settled);
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
        let wall = Wall {
            outline: Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 100.0), angle: 0.0, thickness: 2.0 },
            spin: 0.0,
            growth: 0.0,
        };
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
    }

    #[test]
    fn a_rounded_box_keeps_balls_off_its_round_corners() {
        let outline = Outline::Rounded {
            center: Vec2::ZERO,
            half: Vec2::new(100.0, 100.0),
            corner: 40.0,
            angle: 0.0,
            thickness: 2.0,
        };
        let wall = Wall { outline, spin: 0.0, growth: 0.0 };
        // In the corner, where a sharp box would allow (94, 94), the round one does not.
        let mut p = Vec2::new(94.0, 94.0);
        let c = keep_inside(&mut p, Vec2::new(10.0, 10.0), 5.0, &wall).unwrap();
        let core = 100.0 - 1.0 - 5.0 - 34.0; // half - thickness/2 - radius - inner corner
        assert!(((p - Vec2::splat(core)).length() - 34.0).abs() < 1e-3, "{p:?}");
        assert!((c.normal + Vec2::splat(FRAC_1_SQRT_2)).length() < 1e-3, "{c:?}");
        assert!((c.speed - 10.0 * std::f32::consts::SQRT_2).abs() < 1e-3);
        // Along a flat side it behaves like the box.
        let mut q = Vec2::new(0.0, 99.0);
        keep_inside(&mut q, Vec2::ZERO, 5.0, &wall);
        assert!((q.y - 94.0).abs() < 1e-4);
        // Inside, nothing moves.
        let mut still = Vec2::new(50.0, -50.0);
        assert!(keep_inside(&mut still, Vec2::ONE, 5.0, &wall).is_none());
        assert_eq!(still, Vec2::new(50.0, -50.0));
        assert!(!outline.inside(Vec2::new(90.0, 90.0)) && outline.inside(Vec2::new(60.0, 60.0)));
        let rim = outline.point(0.7);
        assert!((100.0..130.0).contains(&rim.length()), "{rim:?}");
        assert!(!outline.inside(rim * 1.02) && outline.inside(rim * 0.98));
        assert!((outline.point(0.0) - Vec2::new(100.0, 0.0)).length() < 1e-3);
        assert_eq!((outline.center(), outline.thickness(), outline.angle()), (Vec2::ZERO, 2.0, 0.0));
        // A ball bigger than the corner sees a sharp box.
        let mut big = Vec2::new(99.0, 99.0);
        keep_inside(&mut big, Vec2::ZERO, 50.0, &wall);
        assert!((big - Vec2::splat(49.0)).length() < 1e-3, "{big:?}");
        // A capsule (corner == half) pins a ball too wide for it on that axis and bounces it off
        // the round ends.
        let capsule = Wall {
            outline: Outline::Rounded {
                center: Vec2::ZERO,
                half: Vec2::new(10.0, 100.0),
                corner: 10.0,
                angle: 0.0,
                thickness: 2.0,
            },
            spin: 0.0,
            growth: 0.0,
        };
        let mut wide = Vec2::new(5.0, 99.0);
        let c = keep_inside(&mut wide, Vec2::new(0.0, 10.0), 20.0, &capsule).unwrap();
        assert!(wide.x.abs() < 1e-3 && (wide.y - 79.0).abs() < 1e-3, "{wide:?}");
        assert!((c.normal.y + 1.0).abs() < 1e-3, "{c:?}");
        // Turned, the same in its own frame.
        let turned = Wall {
            outline: Outline::Rounded {
                center: Vec2::ZERO,
                half: Vec2::splat(100.0),
                corner: 40.0,
                angle: FRAC_PI_2,
                thickness: 2.0,
            },
            spin: 0.0,
            growth: 0.0,
        };
        let mut t = Vec2::new(-94.0, 94.0);
        keep_inside(&mut t, Vec2::new(-10.0, 10.0), 5.0, &turned).unwrap();
        assert!(((t - Vec2::new(-core, core)).length() - 34.0).abs() < 1e-3, "{t:?}");
    }

    #[test]
    fn a_ball_that_does_not_fit_is_pinned_without_a_contact() {
        let mut p = Vec2::new(3.0, 1.0);
        assert!(keep_inside(&mut p, Vec2::new(50.0, 0.0), 20.0, &ring(10.0)).is_none());
        assert!(p.length() < 1e-3);
        // A box too low for the ball pins it on that axis only; it still bounces off the ends.
        let outline = Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 5.0), angle: 0.0, thickness: 2.0 };
        let wall = Wall { outline, spin: 0.0, growth: 0.0 };
        let mut p = Vec2::new(99.0, 3.0);
        let c = keep_inside(&mut p, Vec2::new(50.0, 10.0), 20.0, &wall).expect("the end wall is hit");
        assert!(p.y.abs() < 1e-3 && (p.x - 79.0).abs() < 1e-3, "{p:?}");
        assert!((c.normal.x + 1.0).abs() < 1e-3 && c.normal.y.abs() < 1e-3, "{c:?}");
    }
}
