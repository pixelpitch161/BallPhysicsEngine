//! Soft things: jelly balls, ropes, cloth and a small amount of liquid, all made of points
//! moved by one position-based (XPBD) solver.
//!
//! Jelly balls are rings of points tied to their neighbours and held round by the air inside
//! them, ropes are chains, cloth is a grid of points with a few pinned in place, and liquid is
//! a cloud of particles that keep an even spacing (position-based fluid). Everything runs at the
//! simulation's fixed steps on the CPU, in index order, so it is as repeatable as the balls.
//!
//! This is the last stage of each substep, but the soft world itself moves once per whole step
//! (with its own solver passes), so how many substeps the balls need doesn't change it. Each
//! point is bounced off the walls between passes, with the same code as balls.

mod build;
mod fluid;

pub use build::build;
use fluid::Fluid;

use crate::broadphase::Grid;
use crate::{Ball, Event, EventKind, Outline, RingNow, Simulation};
use bpe_scene::{Scene, Vec2};
use std::ops::Range;

/// What kind of soft thing a body is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Jelly,
    Rope,
    Cloth,
    Fluid,
}

/// One point of a soft body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Where it was at the start of the current solver pass.
    pub(crate) prev: Vec2,
    /// 0 for pinned points, which never move.
    pub(crate) inverse_mass: f32,
    pub radius: f32,
    pub body: u32,
}

impl Point {
    pub fn pinned(&self) -> bool {
        self.inverse_mass == 0.0
    }
}

/// Two points kept a set distance apart.
#[derive(Clone, Copy, Debug)]
struct Link {
    a: u32,
    b: u32,
    rest: f32,
    /// How much it gives, as 1 / (2π × its springiness in Hz)², in seconds squared: 0 is rigid,
    /// and a link with give `g` wobbles like a spring at 1 / (2π √g) times a second.
    give: f32,
    /// Cut links hold nothing.
    cut: bool,
}

/// A closed ring of points that keeps its area, like a balloon.
#[derive(Clone, Debug)]
struct Ring {
    points: Range<u32>,
    area: f32,
    /// How much it gives, like [`Link::give`].
    give: f32,
}

/// One soft thing and where its points and links are.
#[derive(Clone, Debug)]
pub struct Body {
    pub kind: BodyKind,
    pub points: Range<u32>,
    links: Range<u32>,
    /// Linear-light colour.
    pub color: [f32; 4],
    /// Cloth only: points across and down, stored row by row.
    pub columns: u32,
    pub rows: u32,
    /// Hardest impact this step, and when this body last made a sound.
    pub(crate) impact: Option<(Vec2, f32, u32)>,
    pub(crate) last_event: f64,
    /// Width to draw ropes, in pixels.
    pub thickness: f32,
}

/// Every soft body in the scene.
#[derive(Clone, Default)]
pub struct SoftWorld {
    pub points: Vec<Point>,
    links: Vec<Link>,
    rings: Vec<Ring>,
    pub bodies: Vec<Body>,
    /// Solver passes per step for jelly, rope and cloth.
    passes: u32,
    fluid: Fluid,
    /// Points of different bodies (or far apart in one body) close enough to touch this step.
    pairs: Vec<(u32, u32)>,
    grid: Grid,
    /// Whether a point belongs to the liquid, which has its own solver.
    in_fluid: Vec<bool>,
    /// Scratch for queries.
    nearby: Vec<u32>,
}

/// Seconds a body waits after making a sound before it can make another, so a jelly ball
/// rolling along the floor doesn't buzz.
/// The soft world and how much of the current step it still has to catch up on.
#[derive(Clone, Default)]
pub(crate) struct BodyState {
    pub world: SoftWorld,
    /// Seconds of substeps since the soft world last moved.
    owed: f32,
    /// Scratch for impacts found in a step.
    impacts: Vec<(u32, Vec2, f32, u32)>,
}

impl BodyState {
    pub fn new(scene: &Scene) -> Self {
        Self { world: build(&scene.bodies), owed: 0.0, impacts: Vec::new() }
    }

    /// No soft bodies at all.
    pub fn empty() -> Self {
        Self { world: build(&bpe_scene::Bodies::default()), owed: 0.0, impacts: Vec::new() }
    }
}

/// Moves soft bodies forward and collides them with walls and balls. Called after every
/// substep; the bodies move once the substeps add up to a whole step.
pub(crate) fn step(
    sim: &mut Simulation,
    dt: f32,
    time: f64,
    closed: Option<Outline>,
    rings: &[RingNow],
    events: &mut Vec<Event>,
) {
    if sim.bodies.world.is_empty() {
        return;
    }
    sim.bodies.owed += dt;
    if sim.bodies.owed < sim.dt * 0.999 {
        return;
    }
    sim.bodies.owed = 0.0;
    let passes = sim.bodies.world.passes();
    let h = sim.dt / passes as f32;
    let gravity = sim.scene.physics.gravity;
    let drag = sim.scene.physics.air_drag;
    let frame = sim.wall_frame(closed, sim.bodies.world.restitution());
    sim.bodies.world.find_pairs();
    for pass in 0..passes {
        sim.bodies.world.predict(pass, passes, h, gravity, drag);
        sim.bodies.world.solve(pass, passes, h);
        for i in 0..sim.bodies.world.points.len() {
            let world = &sim.bodies.world;
            if !world.moved_on(i, pass, passes) || world.points[i].pinned() {
                continue;
            }
            let p = world.points[i];
            let mut ball = Ball {
                position: p.position,
                velocity: p.velocity,
                radius: p.radius,
                color: [0.0; 4],
                wall_hits: 0,
                group: 0,
                alive: true,
            };
            let hit = sim.bounce_point(&mut ball, p.prev, &frame, rings);
            let world = &mut sim.bodies.world;
            world.points[i].position = ball.position;
            world.points[i].velocity = ball.velocity;
            if let Some((at, speed)) = hit {
                world.note_impact(i, at, speed, crate::NO_BALL);
            }
        }
    }
    let contacts = &mut sim.broadphase.contacts;
    sim.bodies.world.collide_balls(&mut sim.balls, 0.3, |ball| contacts.wake(ball));
    let resting = sim.scene.physics.gravity.length() * sim.dt * 8.0 + 20.0;
    let state = &mut sim.bodies;
    state.impacts.clear();
    state.world.take_impacts(time, resting, &mut state.impacts);
    for &(body, position, speed, ball) in &state.impacts {
        let group = sim.balls.get(ball as usize).map_or(crate::NO_GROUP, |b| b.group);
        events.push(Event { time, ball, group, position, depth: 0.0, speed, kind: EventKind::Soft { body } });
    }
}

/// The give (see [`Link::give`]) of a link that wobbles `hz` times a second. 0 Hz or less is
/// as loose as a link can be, and infinity is rigid.
pub(crate) fn give(hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz.max(0.05);
    1.0 / (w * w)
}

const SOUND_GAP: f64 = 0.08;
/// Liquid solver passes per step. The liquid moves less per step than cloth, so fewer passes do.
const FLUID_PASSES: u32 = 2;
/// Bounciness of soft points against walls: the squash and spring of the body does the rest.
const SOFT_RESTITUTION: f32 = 0.15;

impl SoftWorld {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// How many solver passes this step makes, for the walls to be checked after each.
    pub fn passes(&self) -> u32 {
        if self.points.len() == self.fluid.count() { FLUID_PASSES } else { self.passes.max(FLUID_PASSES) }
    }

    /// Starts one solver pass of `dt` seconds: moves every free point by gravity and its speed.
    /// `pass` counts from 0; the liquid only moves on its own fewer passes.
    pub fn predict(&mut self, pass: u32, passes: u32, dt: f32, gravity: Vec2, drag: f32) {
        let fluid_now = self.fluid_pass(pass, passes);
        let fluid_dt = dt * passes as f32 / FLUID_PASSES as f32;
        for (i, p) in self.points.iter_mut().enumerate() {
            let liquid = self.in_fluid[i];
            if liquid && !fluid_now {
                continue;
            }
            let h = if liquid { fluid_dt } else { dt };
            p.prev = p.position;
            if p.inverse_mass > 0.0 {
                p.velocity = (p.velocity + gravity * h) * (1.0 - drag * h).max(0.0);
                p.position += p.velocity * h;
            }
        }
    }

    /// Whether the liquid moves on this pass: it takes `FLUID_PASSES` evenly spread passes.
    fn fluid_pass(&self, pass: u32, passes: u32) -> bool {
        let every = (passes / FLUID_PASSES).max(1);
        (pass + 1).is_multiple_of(every) && (pass + 1) / every <= FLUID_PASSES
    }

    /// Pulls points back into shape: links, ring areas, touching points and the liquid's
    /// spacing. Then sets each point's speed from how far it moved this pass.
    pub fn solve(&mut self, pass: u32, passes: u32, dt: f32) {
        let fluid_now = self.fluid_pass(pass, passes) && self.fluid.count() > 0;
        // XPBD: a link's give counts for more the shorter the pass.
        let per_pass = 1.0 / (dt * dt).max(1e-12);
        for link in &self.links {
            if link.cut {
                continue;
            }
            let (a, b) = (self.points[link.a as usize], self.points[link.b as usize]);
            let w = a.inverse_mass + b.inverse_mass;
            if w <= 0.0 {
                continue;
            }
            let d = b.position - a.position;
            let length = d.length();
            if length < 1e-6 {
                continue;
            }
            let lambda = -(length - link.rest) / (w * (1.0 + link.give * per_pass));
            let n = d / length;
            self.points[link.a as usize].position -= n * (lambda * a.inverse_mass);
            self.points[link.b as usize].position += n * (lambda * b.inverse_mass);
        }
        for ring in &self.rings {
            solve_area(&mut self.points, ring, per_pass);
        }
        self.solve_pairs();
        if fluid_now {
            let fluid_dt = dt * passes as f32 / FLUID_PASSES as f32;
            self.fluid.solve(&mut self.points, fluid_dt);
        }
        for (i, p) in self.points.iter_mut().enumerate() {
            let liquid = self.in_fluid[i];
            if liquid && !fluid_now {
                continue;
            }
            let h = if liquid { dt * passes as f32 / FLUID_PASSES as f32 } else { dt };
            if p.inverse_mass > 0.0 {
                p.velocity = (p.position - p.prev) / h;
            }
        }
        if fluid_now {
            self.fluid.viscosity(&mut self.points);
        }
    }

    /// Whether point `i` moved on this pass, so its walls need checking.
    pub fn moved_on(&self, i: usize, pass: u32, passes: u32) -> bool {
        !self.in_fluid[i] || self.fluid_pass(pass, passes)
    }

    /// Finds the points close enough to touch this step, and the liquid's neighbours.
    pub fn find_pairs(&mut self) {
        self.pairs.clear();
        let reach = self.points.iter().map(|p| p.radius).fold(0.0f32, f32::max) * 2.0;
        // Also stops on a NaN radius.
        if self.points.is_empty() || reach.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return;
        }
        // A little extra room, for points that move closer during the step.
        let cell = reach * 1.25;
        self.grid.rebuild(cell, self.points.iter().map(|p| p.position));
        // Liquid particles keep their spacing through the liquid solver, so only pairs with at
        // least one point that isn't liquid are needed: found from that point's side.
        for i in 0..self.points.len() {
            if self.in_fluid[i] {
                continue;
            }
            let (cx, cy) = self.grid.cell_of(self.points[i].position);
            self.nearby.clear();
            let in_fluid = &self.in_fluid;
            self.nearby.extend(self.grid.around(cx, cy).filter(|&j| j as usize > i || in_fluid[j as usize]));
            self.nearby.sort_unstable();
            for &j in &self.nearby {
                let (a, b) = (&self.points[i], &self.points[j as usize]);
                // Points of a rope or cloth don't touch each other, and points next to each other
                // in a jelly ball are held apart by their link.
                if a.body == b.body && self.bodies[a.body as usize].kind != BodyKind::Jelly {
                    continue;
                }
                if a.body == b.body && ring_neighbours(&self.bodies[a.body as usize], i as u32, j) {
                    continue;
                }
                let limit = (a.radius + b.radius) * 1.25;
                if a.position.distance_squared(b.position) < limit * limit {
                    let pair = if (j as usize) < i { (j, i as u32) } else { (i as u32, j) };
                    self.pairs.push(pair);
                }
            }
        }
        self.fluid.find_neighbours(&self.points, &self.grid);
    }

    /// Pushes apart touching points from different bodies.
    fn solve_pairs(&mut self) {
        for &(i, j) in &self.pairs {
            let (a, b) = (self.points[i as usize], self.points[j as usize]);
            let w = a.inverse_mass + b.inverse_mass;
            if w <= 0.0 {
                continue;
            }
            let d = b.position - a.position;
            let distance = d.length();
            let overlap = a.radius + b.radius - distance;
            if overlap <= 0.0 {
                continue;
            }
            let n = if distance > 1e-6 { d / distance } else { Vec2::Y };
            self.points[i as usize].position -= n * (overlap * a.inverse_mass / w);
            self.points[j as usize].position += n * (overlap * b.inverse_mass / w);
        }
    }

    /// Bounces a point off a wall with the given hit, for the walls code: speeds are handled
    /// by the wall itself, so this only remembers the hardest impact for sound.
    pub fn note_impact(&mut self, point: usize, at: Vec2, speed: f32, ball: u32) {
        let body = &mut self.bodies[self.points[point].body as usize];
        if body.impact.is_none_or(|(_, s, _)| speed > s) {
            body.impact = Some((at, speed, ball));
        }
    }

    /// Touches between soft points and balls: both are pushed apart by their weights and
    /// bounce a little. Records impacts for sound, and calls `wake` for each ball pushed, so a
    /// sleeping ball doesn't keep a push it would never act on.
    pub fn collide_balls(&mut self, balls: &mut [Ball], restitution: f32, mut wake: impl FnMut(usize)) {
        if balls.is_empty() || self.points.is_empty() || self.grid.is_empty() {
            return;
        }
        let cell = self.grid.cell_size();
        for (index, ball) in balls.iter_mut().enumerate() {
            if !ball.alive {
                continue;
            }
            let reach = ball.radius + cell;
            let (x0, y0) = self.grid.cell_of(ball.position - Vec2::splat(reach));
            let (x1, y1) = self.grid.cell_of(ball.position + Vec2::splat(reach));
            // A ball much bigger than a cell checks a block of cells, but never more than every point.
            if ((x1 - x0 + 1) as i64) * ((y1 - y0 + 1) as i64) > self.points.len() as i64 {
                self.nearby.clear();
                self.nearby.extend(0..self.points.len() as u32);
            } else {
                self.nearby.clear();
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        self.nearby.extend(self.grid.cell(x, y));
                    }
                }
                self.nearby.sort_unstable();
            }
            let ball_inverse = 1.0 / (ball.radius * ball.radius).max(1e-6);
            for k in 0..self.nearby.len() {
                let i = self.nearby[k] as usize;
                let p = self.points[i];
                let d = p.position - ball.position;
                let distance = d.length();
                let overlap = ball.radius + p.radius - distance;
                if overlap <= 0.0 {
                    continue;
                }
                let n = if distance > 1e-6 { d / distance } else { Vec2::NEG_Y };
                wake(index);
                let w = ball_inverse + p.inverse_mass;
                ball.position -= n * (overlap * ball_inverse / w);
                self.points[i].position += n * (overlap * p.inverse_mass / w);
                let closing = (ball.velocity - p.velocity).dot(n);
                if closing > 0.0 {
                    let impulse = closing * (1.0 + restitution) / w;
                    ball.velocity -= n * (impulse * ball_inverse);
                    self.points[i].velocity += n * (impulse * p.inverse_mass);
                    self.note_impact(i, ball.position + n * ball.radius, closing, index as u32);
                }
            }
        }
    }

    /// The bounciness soft points use against walls.
    pub fn restitution(&self) -> f32 {
        SOFT_RESTITUTION
    }

    /// Bodies that hit something hard enough this step, as (body, where, speed, ball), each at
    /// most once every `SOUND_GAP`. Clears the step's impacts.
    pub fn take_impacts(&mut self, time: f64, resting: f32, out: &mut Vec<(u32, Vec2, f32, u32)>) {
        for (index, body) in self.bodies.iter_mut().enumerate() {
            if let Some((at, speed, ball)) = body.impact.take()
                && speed >= resting
                && time - body.last_event >= SOUND_GAP
            {
                body.last_event = time;
                out.push((index as u32, at, speed, ball));
            }
        }
    }

    /// Cuts rope or cloth `body` at `along` (0 to 1 along its links). Returns whether
    /// anything was cut.
    pub fn cut(&mut self, body: usize, along: f32) -> bool {
        let Some(b) = self.bodies.get(body) else { return false };
        if !matches!(b.kind, BodyKind::Rope | BodyKind::Cloth) || b.links.is_empty() {
            return false;
        }
        let count = b.links.len();
        let k = b.links.start as usize + ((along.clamp(0.0, 1.0) * count as f32) as usize).min(count - 1);
        let was = self.links[k].cut;
        self.links[k].cut = true;
        !was
    }

    /// Pins or frees point `point` of `body` (counted from the body's first point).
    pub fn set_pinned(&mut self, body: usize, point: usize, pinned: bool) -> bool {
        let Some(b) = self.bodies.get(body) else { return false };
        let Some(i) = b.points.clone().nth(point) else { return false };
        let p = &mut self.points[i as usize];
        p.inverse_mass = if pinned {
            0.0
        } else {
            let mass = match b.kind {
                BodyKind::Rope | BodyKind::Cloth => p.radius * p.radius,
                _ => p.radius * p.radius * 4.0,
            };
            1.0 / mass.max(1e-6)
        };
        if pinned {
            p.velocity = Vec2::ZERO;
        }
        true
    }

    /// The links of a body that hold, as point index pairs, for drawing ropes and cloth.
    pub fn links_of(&self, body: usize) -> impl Iterator<Item = (u32, u32)> + '_ {
        let range = self.bodies.get(body).map_or(0..0, |b| b.links.start as usize..b.links.end as usize);
        self.links[range].iter().filter(|l| !l.cut).map(|l| (l.a, l.b))
    }

    /// Area of a jelly ball now compared to its starting area.
    pub fn squash(&self, body: usize) -> f32 {
        let Some(b) = self.bodies.get(body) else { return 1.0 };
        let Some(ring) = self.rings.iter().find(|r| r.points == b.points) else { return 1.0 };
        area(&self.points, &ring.points) / ring.area.max(1e-6)
    }
}

/// Whether points `i` and `j` of a jelly ring are next to each other or one apart, which their
/// links already keep apart.
fn ring_neighbours(body: &Body, i: u32, j: u32) -> bool {
    let n = body.points.len() as u32;
    let (a, b) = (i - body.points.start, j - body.points.start);
    let gap = a.abs_diff(b).min(n - a.abs_diff(b));
    gap <= 2
}

/// Area inside a ring of points, positive when they go clockwise on screen.
fn area(points: &[Point], ring: &Range<u32>) -> f32 {
    let n = ring.len();
    let mut total = 0.0;
    for k in 0..n {
        let a = points[ring.start as usize + k].position;
        let b = points[ring.start as usize + (k + 1) % n].position;
        total += a.perp_dot(b);
    }
    total * 0.5
}

/// Pushes a ring's points in or out along the outline's normals so it keeps its area.
fn solve_area(points: &mut [Point], ring: &Ring, per_pass: f32) {
    let n = ring.points.len();
    if n < 3 {
        return;
    }
    let start = ring.points.start as usize;
    let c = area(points, &ring.points) - ring.area;
    // The area changes by half the cross product of each point's neighbours' gap.
    let gradient = |k: usize, points: &[Point]| {
        let prev = points[start + (k + n - 1) % n].position;
        let next = points[start + (k + 1) % n].position;
        let d = next - prev;
        Vec2::new(d.y, -d.x) * 0.5
    };
    let mut sum = 0.0;
    for k in 0..n {
        sum += points[start + k].inverse_mass * gradient(k, points).length_squared();
    }
    if sum <= 0.0 {
        return;
    }
    let lambda = -c / (sum * (1.0 + ring.give * per_pass));
    for k in 0..n {
        let g = gradient(k, points);
        let w = points[start + k].inverse_mass;
        points[start + k].position += g * (lambda * w);
    }
}
