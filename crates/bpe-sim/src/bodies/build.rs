//! Turning a scene's jelly balls, ropes, cloth and liquid into points and links.

use super::{Body, BodyKind, Link, Point, Ring, SoftWorld, area, give};
use bpe_scene::{Bodies, Cloth, ClothPin, Color, Jelly, Rope, Vec2};
use std::f32::consts::TAU;

/// Most points one body may have, so a typo can't freeze the app.
const MAX_BODY_POINTS: u32 = 20_000;
/// Most points in the whole soft world, liquid included.
const MAX_POINTS: usize = 60_000;

fn body(kind: BodyKind, color: [f32; 4]) -> Body {
    Body {
        kind,
        points: 0..0,
        links: 0..0,
        color,
        columns: 0,
        rows: 0,
        impact: None,
        last_event: f64::NEG_INFINITY,
        thickness: 0.0,
    }
}

impl SoftWorld {
    fn point(&mut self, position: Vec2, velocity: Vec2, radius: f32, mass: f32, body: u32) {
        let inverse_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
        self.points.push(Point { position, velocity, prev: position, inverse_mass, radius, body });
        self.in_fluid.push(false);
    }

    fn link(&mut self, a: u32, b: u32, give: f32) {
        let rest = self.points[a as usize].position.distance(self.points[b as usize].position);
        self.links.push(Link { a, b, rest, give, cut: false });
    }

    fn room_for(&self, more: u32) -> bool {
        more <= MAX_BODY_POINTS && self.points.len() + more as usize <= MAX_POINTS
    }

    fn add_jelly(&mut self, jelly: &Jelly) {
        let count = jelly.count();
        let n = jelly.points.clamp(8, 64);
        let radius = jelly.radius.max(2.0);
        let squish = jelly.squishiness.clamp(0.0, 1.0);
        // Each ball weighs as much as a solid ball its size, shared among its edge points.
        let mass = radius * radius / n as f32;
        let point_radius = (radius * (std::f32::consts::PI / n as f32).sin()).max(1.0);
        let size =
            Vec2::new(jelly.columns.saturating_sub(1) as f32, jelly.rows.saturating_sub(1) as f32) * jelly.spacing;
        let origin = jelly.center - size * 0.5;
        for k in 0..count {
            if !self.room_for(n) {
                return;
            }
            let (column, row) = (k % jelly.columns.max(1), k / jelly.columns.max(1));
            let center = origin + Vec2::new(column as f32, row as f32) * jelly.spacing;
            let t = if count > 1 { k as f32 / (count - 1) as f32 } else { 0.0 };
            let color = Color::sample_gradient(&jelly.gradient, t);
            let index = self.bodies.len() as u32;
            let mut b = body(BodyKind::Jelly, color);
            let first = self.points.len() as u32;
            for i in 0..n {
                let angle = i as f32 / n as f32 * TAU;
                // The point sits so its edge, not its middle, is at the ball's radius.
                self.point(
                    center + Vec2::from_angle(angle) * (radius - point_radius),
                    jelly.velocity,
                    point_radius,
                    mass,
                    index,
                );
            }
            b.points = first..first + n;
            let links_start = self.links.len() as u32;
            // The skin stretches a little; the second-neighbour links stop it folding. Softer
            // jelly has springier skin.
            let skin = give(lerp(200.0, 15.0, squish));
            let fold = give(lerp(80.0, 5.0, squish));
            for i in 0..n {
                self.link(first + i, first + (i + 1) % n, skin);
            }
            for i in 0..n {
                self.link(first + i, first + (i + 2) % n, fold);
            }
            b.links = links_start..self.links.len() as u32;
            let rest = area(&self.points, &b.points);
            // Softer jelly gives way to more of a squeeze before the air inside pushes back.
            let springiness = give(lerp(60.0, 8.0, squish));
            self.rings.push(Ring { points: b.points.clone(), area: rest, give: springiness });
            self.bodies.push(b);
        }
    }

    fn add_rope(&mut self, rope: &Rope) {
        let n = rope.points.clamp(2, 400);
        if !self.room_for(n) {
            return;
        }
        let index = self.bodies.len() as u32;
        let mut b = body(BodyKind::Rope, rope.color.to_linear());
        b.thickness = rope.thickness.max(0.5);
        let radius = (rope.thickness * 0.5).max(1.0);
        let first = self.points.len() as u32;
        let mass = radius * radius;
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            let pinned = (i == 0 && rope.pin_start) || (i == n - 1 && rope.pin_end);
            self.point(rope.from.lerp(rope.to, t), Vec2::ZERO, radius, if pinned { 0.0 } else { mass }, index);
        }
        b.points = first..first + n;
        let links_start = self.links.len() as u32;
        let stiffness = rope.stiffness.clamp(0.0, 1.0);
        let springiness = if stiffness >= 1.0 { 0.0 } else { give(2.0 + 60.0 * stiffness * stiffness) };
        let length = rope.from.distance(rope.to) * rope.slack.max(0.1);
        let rest = length / (n - 1) as f32;
        for i in 0..n - 1 {
            self.links.push(Link { a: first + i, b: first + i + 1, rest, give: springiness, cut: false });
        }
        b.links = links_start..self.links.len() as u32;
        self.bodies.push(b);
    }

    fn add_cloth(&mut self, cloth: &Cloth) {
        let (columns, rows) = (cloth.columns.clamp(2, 120), cloth.rows.clamp(2, 120));
        if !self.room_for(columns * rows) {
            return;
        }
        let index = self.bodies.len() as u32;
        let mut b = body(BodyKind::Cloth, cloth.color.to_linear());
        b.columns = columns;
        b.rows = rows;
        let step = Vec2::new(cloth.size.x / (columns - 1) as f32, cloth.size.y / (rows - 1) as f32);
        let radius = (step.min_element() * 0.5).max(1.0);
        let origin = cloth.center - cloth.size * 0.5;
        let first = self.points.len() as u32;
        let mass = radius * radius;
        for row in 0..rows {
            for column in 0..columns {
                let pinned = row == 0
                    && match cloth.pin {
                        ClothPin::None => false,
                        ClothPin::TopRow => true,
                        ClothPin::TopCorners => column == 0 || column == columns - 1,
                    };
                let at = origin + Vec2::new(column as f32, row as f32) * step;
                self.point(at, Vec2::ZERO, radius, if pinned { 0.0 } else { mass }, index);
            }
        }
        b.points = first..first + columns * rows;
        let id = |c: u32, r: u32| first + r * columns + c;
        let s = cloth.stiffness.clamp(0.0, 1.0);
        let (stretch, shear, bend) = (give(20.0 + 100.0 * s), give(0.3 + 4.0 * s * s), give(0.2 + 2.0 * s * s));
        let links_start = self.links.len() as u32;
        // Rows and columns first, so a cut through them can be found by position.
        for r in 0..rows {
            for c in 0..columns {
                if c + 1 < columns {
                    self.link(id(c, r), id(c + 1, r), stretch);
                }
                if r + 1 < rows {
                    self.link(id(c, r), id(c, r + 1), stretch);
                }
            }
        }
        for r in 0..rows - 1 {
            for c in 0..columns - 1 {
                self.link(id(c, r), id(c + 1, r + 1), shear);
                self.link(id(c + 1, r), id(c, r + 1), shear);
            }
        }
        for r in 0..rows {
            for c in 0..columns {
                if c + 2 < columns {
                    self.link(id(c, r), id(c + 2, r), bend);
                }
                if r + 2 < rows {
                    self.link(id(c, r), id(c, r + 2), bend);
                }
            }
        }
        b.links = links_start..self.links.len() as u32;
        self.bodies.push(b);
    }

    fn add_fluid(&mut self, fluid: &bpe_scene::Fluid) {
        if fluid.count == 0 || fluid.on_gpu() {
            return;
        }
        let count = fluid.count;
        if !self.room_for(count) {
            return;
        }
        let index = self.bodies.len() as u32;
        let mut b = body(BodyKind::Fluid, fluid.color.to_linear());
        let radius = fluid.radius.max(1.0);
        let spacing = radius * 2.0;
        let columns = ((fluid.size.x / spacing).floor() as u32).clamp(1, count);
        let rows = count.div_ceil(columns);
        let origin =
            fluid.center - Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * spacing * 0.5;
        let first = self.points.len() as u32;
        for k in 0..count {
            let (c, r) = (k % columns, k / columns);
            // Every other row shifted half a step, so the liquid starts evenly packed.
            let shift = if r % 2 == 1 { spacing * 0.5 } else { 0.0 };
            let at = origin + Vec2::new(c as f32 * spacing + shift, r as f32 * spacing * 0.866);
            self.point(at, fluid.velocity, radius, radius * radius, index);
            *self.in_fluid.last_mut().unwrap() = true;
        }
        b.points = first..first + count;
        self.fluid.setup(b.points.clone(), radius, fluid.thickness, &self.points);
        self.bodies.push(b);
    }
}

/// The soft world for a scene's bodies. Empty when there are none.
pub fn build(bodies: &Bodies) -> SoftWorld {
    let mut world = SoftWorld { passes: bodies.iterations.clamp(1, 64), ..SoftWorld::default() };
    for jelly in &bodies.jelly {
        world.add_jelly(jelly);
    }
    for rope in &bodies.ropes {
        world.add_rope(rope);
    }
    for cloth in &bodies.cloths {
        world.add_cloth(cloth);
    }
    world.add_fluid(&bodies.fluid);
    world
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
