//! Turning a soft body's settings into points and links.

use std::f32::consts::{PI, TAU};

use glam::Vec2;

use super::solver::{Link, Ring, SoftWorld, area, give};
use super::{Pinned, SoftSettings, SoftShape};

/// Most points one body may have, so a typo can't freeze the app.
const MAX_POINTS: u32 = 20_000;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The soft world for one body at `position`, turned by `rotation` (radians clockwise).
pub fn build(settings: &SoftSettings, position: Vec2, rotation: f32) -> SoftWorld {
    let mut world = SoftWorld::new(settings.shape, settings.passes);
    match settings.shape {
        SoftShape::Jelly => jelly(&mut world, settings, position),
        SoftShape::Rope => rope(&mut world, settings, position, rotation),
        SoftShape::Cloth => cloth(&mut world, settings, position, rotation),
    }
    world
}

fn jelly(world: &mut SoftWorld, s: &SoftSettings, center: Vec2) {
    let n = s.detail.clamp(8, 64);
    let radius = s.size.max(2.0);
    let squish = s.softness.clamp(0.0, 1.0);
    // The ball weighs as much as a solid ball its size, shared among its edge points.
    let mass = radius * radius / n as f32;
    let point_radius = (radius * (PI / n as f32).sin()).max(1.0);
    // The topmost point is the one held when the jelly is pinned at all.
    let held = match s.pinned {
        Pinned::Usual | Pinned::Nothing => None,
        _ => Some(n * 3 / 4),
    };
    for i in 0..n {
        let angle = i as f32 / n as f32 * TAU;
        // The point sits so its edge, not its middle, is at the ball's radius.
        let at = center + Vec2::from_angle(angle) * (radius - point_radius);
        world.point(at, s.velocity, point_radius, if held == Some(i) { 0.0 } else { mass });
    }
    // The skin stretches a little; the second-neighbour links stop it folding. Softer jelly has
    // springier skin.
    let skin = give(lerp(200.0, 15.0, squish));
    let fold = give(lerp(80.0, 5.0, squish));
    for i in 0..n {
        world.link(i, (i + 1) % n, skin);
    }
    for i in 0..n {
        world.link(i, (i + 2) % n, fold);
    }
    let rest = area(&world.points, &(0..n));
    // Softer jelly gives way to more of a squeeze before the air inside pushes back.
    world.rings.push(Ring { points: 0..n, area: rest, give: give(lerp(60.0, 8.0, squish)) });
}

fn rope(world: &mut SoftWorld, s: &SoftSettings, center: Vec2, rotation: f32) {
    let n = s.detail.clamp(2, 400).min(MAX_POINTS);
    let half = Vec2::from_angle(rotation) * (s.size.max(1.0) * 0.5);
    let (from, to) = (center - half, center + half);
    world.thickness = s.thickness.max(0.5);
    let radius = (s.thickness * 0.5).max(1.0);
    let mass = radius * radius;
    let (pin_start, pin_end) = match s.pinned {
        Pinned::Usual | Pinned::BothEnds | Pinned::TopRow | Pinned::TopCorners => (true, true),
        Pinned::Nothing => (false, false),
        Pinned::Start => (true, false),
        Pinned::End => (false, true),
    };
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let pinned = (i == 0 && pin_start) || (i == n - 1 && pin_end);
        world.point(from.lerp(to, t), Vec2::ZERO, radius, if pinned { 0.0 } else { mass });
    }
    let stiffness = 1.0 - s.softness.clamp(0.0, 1.0);
    let springiness = if stiffness >= 1.0 { 0.0 } else { give(2.0 + 60.0 * stiffness * stiffness) };
    let rest = from.distance(to) * s.slack.max(0.1) / (n - 1) as f32;
    for i in 0..n - 1 {
        world.links.push(Link { a: i, b: i + 1, rest, give: springiness, cut: false });
    }
}

fn cloth(world: &mut SoftWorld, s: &SoftSettings, center: Vec2, rotation: f32) {
    let (columns, rows) = (s.detail.clamp(2, 120), s.rows.clamp(2, 120));
    if columns * rows > MAX_POINTS {
        return;
    }
    world.columns = columns;
    world.rows = rows;
    let size = Vec2::new(s.size.max(1.0), s.height.max(1.0));
    let step = Vec2::new(size.x / (columns - 1) as f32, size.y / (rows - 1) as f32);
    let radius = (step.min_element() * 0.5).max(1.0);
    let turn = Vec2::from_angle(rotation);
    let mass = radius * radius;
    let pinned = |column: u32, row: u32| {
        if row != 0 {
            return false;
        }
        let corner = column == 0 || column == columns - 1;
        match s.pinned {
            Pinned::Usual | Pinned::TopRow => true,
            Pinned::Nothing => false,
            Pinned::TopCorners | Pinned::BothEnds => corner,
            Pinned::Start => column == 0,
            Pinned::End => column == columns - 1,
        }
    };
    for row in 0..rows {
        for column in 0..columns {
            let local = Vec2::new(column as f32, row as f32) * step - size * 0.5;
            let at = center + turn.rotate(local);
            world.point(at, Vec2::ZERO, radius, if pinned(column, row) { 0.0 } else { mass });
        }
    }
    let id = |c: u32, r: u32| r * columns + c;
    let stiffness = 1.0 - s.softness.clamp(0.0, 1.0);
    let (stretch, shear, bend) = (
        give(20.0 + 100.0 * stiffness),
        give(0.3 + 4.0 * stiffness * stiffness),
        give(0.2 + 2.0 * stiffness * stiffness),
    );
    // Rows and columns first, so the drawing can tell them from the diagonals.
    for r in 0..rows {
        for c in 0..columns {
            if c + 1 < columns {
                world.link(id(c, r), id(c + 1, r), stretch);
            }
            if r + 1 < rows {
                world.link(id(c, r), id(c, r + 1), stretch);
            }
        }
    }
    for r in 0..rows - 1 {
        for c in 0..columns - 1 {
            world.link(id(c, r), id(c + 1, r + 1), shear);
            world.link(id(c + 1, r), id(c, r + 1), shear);
        }
    }
    for r in 0..rows {
        for c in 0..columns {
            if c + 2 < columns {
                world.link(id(c, r), id(c + 2, r), bend);
            }
            if r + 2 < rows {
                world.link(id(c, r), id(c, r + 2), bend);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jelly_has_a_ring_of_points_and_two_links_each() {
        let w = build(&SoftSettings::default(), Vec2::new(10.0, 20.0), 0.0);
        assert_eq!(w.points.len(), 24);
        assert_eq!(w.links.len(), 48);
        assert_eq!(w.rings.len(), 1);
        assert!(w.rings[0].area > 0.0);
        assert!(w.points.iter().all(|p| !p.pinned()));
        assert!((w.middle() - Vec2::new(10.0, 20.0)).length() < 1e-3);
        assert!((w.squash() - 1.0).abs() < 1e-5);
        let held = build(&SoftSettings { pinned: Pinned::TopRow, ..SoftSettings::default() }, Vec2::ZERO, 0.0);
        assert_eq!(held.points.iter().filter(|p| p.pinned()).count(), 1);
        assert!(held.points.iter().find(|p| p.pinned()).unwrap().position.y < -100.0, "the top point is held");
    }

    #[test]
    fn a_rope_pins_its_usual_ends_and_lies_along_its_rotation() {
        let s = SoftSettings { shape: SoftShape::Rope, size: 300.0, detail: 40, ..SoftSettings::default() };
        let w = build(&s, Vec2::ZERO, std::f32::consts::FRAC_PI_2);
        assert_eq!(w.points.len(), 40);
        assert_eq!(w.links.len(), 39);
        assert!(w.points[0].pinned() && w.points[39].pinned());
        assert_eq!(w.points.iter().filter(|p| p.pinned()).count(), 2);
        assert!((w.points[0].position - Vec2::new(0.0, -150.0)).length() < 1e-3, "{:?}", w.points[0].position);
        assert!((w.points[39].position - Vec2::new(0.0, 150.0)).length() < 1e-3);
        assert_eq!(w.thickness, 8.0);
        // Slack makes the links' rest length longer than the gap.
        let rest: f32 = w.links.iter().map(|l| l.rest).sum();
        assert!((rest - 300.0 * 1.15).abs() < 1e-2, "{rest}");
        let one_end = build(&SoftSettings { pinned: Pinned::Start, ..s.clone() }, Vec2::ZERO, 0.0);
        assert!(one_end.points[0].pinned() && !one_end.points[39].pinned());
    }

    #[test]
    fn a_cloth_is_a_grid_pinned_along_its_top() {
        let s = SoftSettings {
            shape: SoftShape::Cloth,
            size: 200.0,
            height: 100.0,
            detail: 5,
            rows: 3,
            ..SoftSettings::default()
        };
        let w = build(&s, Vec2::ZERO, 0.0);
        assert_eq!((w.columns, w.rows), (5, 3));
        assert_eq!(w.points.len(), 15);
        assert_eq!(w.points.iter().filter(|p| p.pinned()).count(), 5);
        assert!(w.points[..5].iter().all(|p| p.pinned() && (p.position.y + 50.0).abs() < 1e-3));
        let across = 3 * 4;
        let down = 5 * 2;
        let shear = 2 * 4 * 2;
        let bend = 3 * 3 + 5;
        assert_eq!(w.links.len(), across + down + shear + bend);
        let corners = build(&SoftSettings { pinned: Pinned::TopCorners, ..s }, Vec2::ZERO, 0.0);
        assert_eq!(corners.points.iter().filter(|p| p.pinned()).count(), 2);
    }
}
