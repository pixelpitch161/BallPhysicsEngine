//! Deterministic 3D ball physics for scenes with a 3D view: balls inside a sphere, a cube or an
//! endless tunnel.
//!
//! It follows the same rules as the 2D [`crate::Simulation`]: fixed steps, substeps for fast balls,
//! bounciness, air drag, bounce rules (speed gain, growing, recolouring, new balls), arena size and
//! shrinking, keyframes, and ball-to-ball collisions on a grid. It reports hits as the same events
//! as 2D, with `depth` set, so sound, rules, captions and the timeline work unchanged.

use crate::{Event, EventKind, noise, rotate_hue, unit_noise};
use bpe_scene::{Color, Container3d, Scene, Space, Spawner, Vec2, Vec3};

/// Furthest a ball moves in one substep, in pixels, as in 2D.
const SAFE_MOVE: f32 = 16.0;
const MAX_SUBSTEPS: u32 = 64;
const TRAIL_SAMPLE_RATE: u32 = 120;
const MAX_TRAIL_SAMPLES: usize = 256;
const MAX_ARENA_SCALE: f32 = 4.0;
const SHRINK_EASE_SECONDS: f32 = 0.06;
/// The tunnel's back wall stops this many ball diameters short of the next unbroken ring.
const TUNNEL_ROOM: f32 = 3.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Ball3 {
    pub position: Vec3,
    pub velocity: Vec3,
    pub radius: f32,
    /// Linear-light RGBA.
    pub color: [f32; 4],
    pub wall_hits: u32,
}

/// One of the tunnel's rings at the current moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    pub index: u32,
    /// Distance along the tunnel.
    pub z: f32,
    /// Direction of the middle of the gap, radians clockwise from +x.
    pub angle: f32,
    /// Half the gap's width in radians.
    pub gap_half: f32,
}

/// When a tunnel ring broke, and how it was turned then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateBreak {
    pub time: f64,
    pub gate: Gate,
}

/// The container's size, turn and (for a tunnel) back wall at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceArena {
    /// Size compared to the container's size, after shrinking.
    pub scale: f32,
    pub scale_rate: f32,
    /// Radians the container has turned around the up axis.
    pub angle: f32,
    /// Radians per second it turns.
    pub spin: f32,
    /// How far the tunnel's back wall has moved forward.
    pub back: f32,
    /// How fast the back wall moves right now.
    pub back_rate: f32,
}

impl Default for SpaceArena {
    fn default() -> Self {
        Self { scale: 1.0, scale_rate: 0.0, angle: 0.0, spin: 0.0, back: 0.0, back_rate: 0.0 }
    }
}

/// What a ball touched in one substep, before it becomes an event.
#[derive(Clone, Copy)]
enum Touch {
    Wall,
    Escape(u32),
    Ball(u32),
}

#[derive(Clone)]
pub struct SpaceSimulation {
    scene: Scene,
    space: Space,
    balls: Vec<Ball3>,
    step: u64,
    dt: f32,
    arena: SpaceArena,
    arena_size: f32,
    shrunk: f32,
    shrink_owed: f32,
    turned: f64,
    animated: bool,
    /// Balls that appear later, as (step, ball), latest first.
    pending: Vec<(u64, Ball3)>,
    /// One entry per tunnel ring, set once it breaks.
    breaks: Vec<Option<GateBreak>>,
    /// The first unbroken tunnel ring: rings break in order, since each one blocks the rest.
    frontier: usize,
    previous: Vec<Vec3>,
    grid: BallGrid,
    trail: Vec<Vec3>,
    trail_len: usize,
    trail_every: u64,
    trail_head: usize,
    trail_filled: usize,
}

impl SpaceSimulation {
    /// A 3D simulation of the scene. A flat scene gets the default 3D settings.
    pub fn new(scene: &Scene) -> Self {
        let space = scene.space().cloned().unwrap_or_default();
        let steps_per_second = scene.physics.steps_per_second.max(1);
        let mut pending = spawn_plan(scene, &space, steps_per_second);
        let split = pending.partition_point(|(step, _)| *step == 0);
        let balls: Vec<Ball3> = pending.drain(..split).map(|(_, ball)| ball).collect();
        pending.reverse();
        let trail_every = (steps_per_second / TRAIL_SAMPLE_RATE).max(1) as u64;
        let sample_rate = steps_per_second as f32 / trail_every as f32;
        let trail_len = ((scene.look.trail_seconds.max(0.0) * sample_rate).ceil() as usize + 1).min(MAX_TRAIL_SAMPLES);
        let rings = match space.container {
            Container3d::Tunnel { rings, .. } => rings.min(10_000) as usize,
            _ => 0,
        };
        let mut sim = Self {
            scene: scene.clone(),
            arena_size: space.container.size().max(1.0),
            space,
            trail: vec![Vec3::ZERO; balls.len() * trail_len],
            balls,
            step: 0,
            dt: 1.0 / steps_per_second as f32,
            arena: SpaceArena::default(),
            shrunk: 1.0,
            shrink_owed: 0.0,
            turned: 0.0,
            animated: scene.animates_simulation(),
            pending,
            breaks: vec![None; rings],
            frontier: 0,
            previous: Vec::new(),
            grid: BallGrid::default(),
            trail_len,
            trail_every,
            trail_head: 0,
            trail_filled: 0,
        };
        sim.update_arena();
        let mut balls = std::mem::take(&mut sim.balls);
        for ball in &mut balls {
            sim.place_inside(ball);
        }
        sim.balls = balls;
        sim.record_trail();
        sim
    }

    /// Moves a new ball inside the container, for balls placed for a bigger 2D arena.
    fn place_inside(&self, ball: &mut Ball3) {
        let arena = self.arena;
        let p = &mut ball.position;
        match self.space.container {
            Container3d::Sphere { radius } => {
                let room = (radius * arena.scale - ball.radius).max(0.0);
                *p = p.clamp_length_max(room);
            }
            Container3d::Cube { size } => {
                let room = (size * 0.5 * arena.scale - ball.radius).max(0.0);
                let turn = glam::Quat::from_rotation_y(arena.angle);
                *p = turn * (turn.inverse() * *p).clamp(Vec3::splat(-room), Vec3::splat(room));
            }
            Container3d::Tunnel { radius, .. } => {
                let room = (radius * arena.scale - ball.radius).max(0.0);
                let across = Vec2::new(p.x, p.y).clamp_length_max(room);
                let back = arena.back + ball.radius;
                let front = self.gates().next().map_or(f32::INFINITY, |gate| gate.z - ball.radius);
                *p = across.extend(p.z.min(front).max(back));
            }
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The 3D settings the simulation runs with.
    pub fn space(&self) -> &Space {
        &self.space
    }

    pub fn balls(&self) -> &[Ball3] {
        &self.balls
    }

    /// Whole steps run so far.
    pub fn steps(&self) -> u64 {
        self.step
    }

    /// Roughly how many bytes this simulation holds, for limiting snapshots.
    pub fn memory(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.balls.capacity() * std::mem::size_of::<Ball3>()
            + self.trail.capacity() * std::mem::size_of::<Vec3>()
            + self.previous.capacity() * std::mem::size_of::<Vec3>()
            + self.pending.capacity() * std::mem::size_of::<(u64, Ball3)>()
            + self.breaks.capacity() * std::mem::size_of::<Option<GateBreak>>()
    }

    pub fn time(&self) -> f64 {
        self.step as f64 / self.scene.physics.steps_per_second.max(1) as f64
    }

    pub fn arena(&self) -> SpaceArena {
        self.arena
    }

    /// One entry per tunnel ring: when it broke, if it has.
    pub fn gate_breaks(&self) -> &[Option<GateBreak>] {
        &self.breaks
    }

    /// The tunnel's unbroken rings as they are now, nearest first.
    pub fn gates(&self) -> impl Iterator<Item = Gate> + '_ {
        (self.frontier..self.breaks.len()).filter_map(|k| self.gate(k as u32))
    }

    /// Ring `index` of a tunnel as it is now.
    pub fn gate(&self, index: u32) -> Option<Gate> {
        let Container3d::Tunnel { spacing, gap, spin, gap_offset, .. } = self.space.container else { return None };
        let direction = if index % 2 == 0 { 1.0 } else { -1.0 };
        let turn = gap_offset * index as f32 + direction * spin * self.time() as f32;
        Some(Gate {
            index,
            z: spacing.max(1.0) * (index + 1) as f32,
            angle: turn.to_radians().rem_euclid(std::f32::consts::TAU),
            gap_half: (gap.clamp(0.0, 359.0) * 0.5).to_radians(),
        })
    }

    /// Seconds between trail samples.
    pub fn trail_interval(&self) -> f32 {
        self.dt * self.trail_every as f32
    }

    /// Past positions of one ball, newest first. The ball's current position is not included.
    pub fn trail(&self, ball: usize) -> impl Iterator<Item = Vec3> + '_ {
        let base = ball * self.trail_len;
        (0..self.trail_filled).map(move |k| {
            let slot = (self.trail_head + self.trail_len - k) % self.trail_len;
            self.trail[base + slot]
        })
    }

    /// Runs whole steps until the simulation reaches `time`, appending events to `events`.
    pub fn advance_to(&mut self, time: f64, events: &mut Vec<Event>) {
        let target = (time * self.scene.physics.steps_per_second as f64).floor() as u64;
        while self.step < target {
            self.step(events);
        }
    }

    pub fn step(&mut self, events: &mut Vec<Event>) {
        while self.pending.last().is_some_and(|(step, _)| *step <= self.step) {
            let (_, mut ball) = self.pending.pop().unwrap();
            self.place_inside(&mut ball);
            self.add_ball(ball);
        }
        let ceiling = self.speed_ceiling();
        for ball in &mut self.balls {
            ball.velocity = ball.velocity.clamp_length_max(ceiling);
        }
        let substeps = self.substeps();
        self.step += 1;
        self.update_arena();
        let rate = self.scene.physics.steps_per_second.max(1) as f64;
        for i in 0..substeps {
            let time = ((self.step - 1) as f64 + (i + 1) as f64 / substeps as f64) / rate;
            self.substep(self.dt / substeps as f32, time, events);
        }
        if self.step.is_multiple_of(self.trail_every) {
            self.record_trail();
        }
    }

    fn gravity(&self) -> Vec3 {
        let g = self.scene.physics.gravity;
        Vec3::new(g.x, g.y, self.space.gravity_depth)
    }

    fn speed_ceiling(&self) -> f32 {
        SAFE_MOVE * MAX_SUBSTEPS as f32 * self.scene.physics.steps_per_second.max(1) as f32
    }

    fn substeps(&self) -> u32 {
        let fastest = self.balls.iter().map(|b| b.velocity.length()).fold(0.0f32, f32::max);
        let travel = (fastest + self.gravity().length() * self.dt + self.arena.back_rate.abs()) * self.dt;
        ((travel / SAFE_MOVE).ceil() as u32).clamp(1, MAX_SUBSTEPS)
    }

    fn substep(&mut self, dt: f32, time: f64, events: &mut Vec<Event>) {
        let physics = &self.scene.physics;
        let restitution = physics.restitution;
        let gravity = self.gravity();
        let drag = (1.0 - physics.air_drag * dt).max(0.0);
        // Touches slower than this are balls resting on something, not bounces.
        let resting = gravity.length() * self.dt * 4.0 + 1.0;
        self.previous.clear();
        self.previous.extend(self.balls.iter().map(|b| b.position));
        for ball in &mut self.balls {
            ball.velocity = (ball.velocity + gravity * dt) * drag;
            ball.position += ball.velocity * dt;
        }

        let mut touches: Vec<(u32, Vec3, f32, Touch)> = Vec::new();
        if physics.ball_collisions && self.balls.len() > 1 {
            self.grid.collide(&mut self.balls, restitution, resting * 2.0, &mut touches);
        }

        let arena = self.arena;
        let growth = arena.scale_rate / arena.scale.max(1e-3);
        let frontier = self.gates().next();
        for (index, ball) in self.balls.iter_mut().enumerate() {
            let hit = match self.space.container {
                Container3d::Sphere { radius } => collide_sphere(ball, radius * arena.scale, growth, restitution),
                Container3d::Cube { size } => collide_cube(ball, size * 0.5 * arena.scale, arena, growth, restitution),
                Container3d::Tunnel { radius, .. } => {
                    let mut hit = collide_tube(ball, radius * arena.scale, growth, restitution);
                    let back = collide_back(ball, arena.back, arena.back_rate, restitution);
                    hit = hardest(hit, back);
                    if let Some(gate) = frontier {
                        let prev = self.previous[index];
                        match collide_gate(ball, prev, &gate, restitution) {
                            GateTouch::Bounce(position, speed) => hit = hardest(hit, Some((position, speed))),
                            GateTouch::Through => {
                                // Only the first ball through breaks it.
                                if self.breaks[gate.index as usize].is_none() {
                                    self.breaks[gate.index as usize] = Some(GateBreak { time, gate });
                                    let speed = ball.velocity.length();
                                    touches.push((index as u32, ball.position, speed, Touch::Escape(gate.index)));
                                }
                            }
                            GateTouch::None => {}
                        }
                    }
                    hit
                }
            };
            if let Some((position, speed)) = hit.filter(|(_, speed)| *speed >= resting) {
                touches.push((index as u32, position, speed, Touch::Wall));
            }
        }
        while self.frontier < self.breaks.len() && self.breaks[self.frontier].is_some() {
            self.frontier += 1;
        }

        // Ball-to-ball touches were found first; keep the event log in ball order within a substep.
        touches.sort_by_key(|t| t.0);
        let ceiling = self.speed_ceiling();
        let mut spawns = Vec::new();
        for (ball_index, position, speed, touch) in touches {
            let ball = &mut self.balls[ball_index as usize];
            let rules = &self.scene.rules;
            let (kind, children) = match touch {
                Touch::Wall => {
                    apply_bounce_rules(ball, &self.scene, ceiling);
                    self.shrink_owed += self.scene.arena_motion.shrink_per_hit / self.arena_size;
                    (EventKind::Wall, rules.spawn_per_hit)
                }
                Touch::Escape(k) => (EventKind::Escape(k), rules.spawn_per_escape),
                Touch::Ball(other) => (EventKind::Ball(other), 0),
            };
            spawns.extend((0..children).map(|_| ball_index));
            events.push(event(time, ball_index, position, speed, kind));
        }
        for parent in spawns {
            if self.balls.len() >= self.scene.rules.max_balls as usize {
                break;
            }
            let child = split_ball(&self.balls[parent as usize], self.balls.len(), self.scene.rules.spawn_spread);
            self.add_ball(child);
        }
    }

    fn add_ball(&mut self, ball: Ball3) {
        self.trail.extend(std::iter::repeat_n(ball.position, self.trail_len));
        self.balls.push(ball);
    }

    fn update_arena(&mut self) {
        let time = self.time();
        if self.animated {
            self.scene.apply_animation(time as f32, true);
        }
        let motion = &self.scene.arena_motion;
        self.arena.spin = motion.spin.to_radians();
        if self.animated {
            if self.step > 0 {
                self.turned += motion.spin as f64 * self.dt as f64;
            }
        } else {
            self.turned = motion.spin as f64 * time;
        }
        self.arena.angle = self.turned.to_radians().rem_euclid(std::f64::consts::TAU) as f32;
        let (size, shrink_speed, min_size) = (motion.size.max(0.01), motion.shrink_speed, motion.min_size);
        self.update_back();
        if self.step == 0 {
            self.arena.scale = size;
            return;
        }
        let owed = self.shrink_owed * (self.dt / SHRINK_EASE_SECONDS).min(1.0);
        self.shrink_owed -= owed;
        let change = -shrink_speed / self.arena_size * self.dt - owed;
        let low = (min_size.max(1.0) / self.arena_size).min(1.0);
        self.shrunk = (self.shrunk + change).clamp(low, MAX_ARENA_SCALE);
        let scale = self.shrunk * size;
        self.arena.scale_rate = (scale - self.arena.scale) / self.dt;
        self.arena.scale = scale;
    }

    /// Moves the tunnel's back wall forward, stopping short of the next unbroken ring.
    fn update_back(&mut self) {
        let Container3d::Tunnel { speed, .. } = self.space.container else { return };
        if self.step == 0 {
            return;
        }
        let biggest = self.balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
        let stop = match self.gates().next() {
            Some(gate) => gate.z - (biggest * 2.0 * TUNNEL_ROOM).max(40.0),
            None => f32::INFINITY,
        };
        let back = (self.arena.back + speed.max(0.0) * self.dt).min(stop).max(self.arena.back);
        self.arena.back_rate = (back - self.arena.back) / self.dt;
        self.arena.back = back;
    }

    fn record_trail(&mut self) {
        if self.trail_len == 0 {
            return;
        }
        self.trail_head = (self.trail_head + 1) % self.trail_len;
        for (i, ball) in self.balls.iter().enumerate() {
            self.trail[i * self.trail_len + self.trail_head] = ball.position;
        }
        self.trail_filled = (self.trail_filled + 1).min(self.trail_len);
    }
}

fn event(time: f64, ball: u32, position: Vec3, speed: f32, kind: EventKind) -> Event {
    Event { time, ball, position: Vec2::new(position.x, position.y), depth: position.z, speed, kind }
}

/// The harder of two hits.
fn hardest(a: Option<(Vec3, f32)>, b: Option<(Vec3, f32)>) -> Option<(Vec3, f32)> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.1 > a.1 { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Bounces a ball off a wall whose normal (pointing at the ball) is `normal` and which moves at
/// `wall`. Returns the impact speed if the ball was moving into it.
fn bounce(ball: &mut Ball3, normal: Vec3, wall: Vec3, restitution: f32) -> Option<f32> {
    let relative = ball.velocity - wall;
    let into = -relative.dot(normal);
    if into <= 0.0 {
        return None;
    }
    ball.velocity = relative + normal * into * (1.0 + restitution) + wall;
    Some(into)
}

fn collide_sphere(ball: &mut Ball3, radius: f32, growth: f32, restitution: f32) -> Option<(Vec3, f32)> {
    let inner = (radius - ball.radius).max(0.0);
    let distance = ball.position.length();
    if distance <= inner || distance == 0.0 {
        return None;
    }
    let normal = ball.position / distance;
    ball.position = normal * inner;
    let contact = normal * radius;
    bounce(ball, -normal, contact * growth, restitution).map(|speed| (contact, speed))
}

fn collide_cube(ball: &mut Ball3, half: f32, arena: SpaceArena, growth: f32, restitution: f32) -> Option<(Vec3, f32)> {
    let turn = glam::Quat::from_rotation_y(arena.angle);
    let limit = (half - ball.radius).max(0.0);
    let mut hit = None;
    for axis in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
        let normal = turn * axis;
        let excess = ball.position.dot(normal) - limit;
        if excess <= 0.0 {
            continue;
        }
        ball.position -= normal * excess;
        let contact = ball.position + normal * ball.radius;
        let wall = Vec3::Y.cross(contact) * arena.spin + contact * growth;
        if let Some(speed) = bounce(ball, -normal, wall, restitution) {
            hit = hardest(hit, Some((contact, speed)));
        }
    }
    hit
}

/// Keeps a ball inside the tunnel's tube, which runs along z.
fn collide_tube(ball: &mut Ball3, radius: f32, growth: f32, restitution: f32) -> Option<(Vec3, f32)> {
    let inner = (radius - ball.radius).max(0.0);
    let across = Vec2::new(ball.position.x, ball.position.y);
    let distance = across.length();
    if distance <= inner || distance == 0.0 {
        return None;
    }
    let normal = (across / distance).extend(0.0);
    ball.position.x = normal.x * inner;
    ball.position.y = normal.y * inner;
    let contact = Vec3::new(normal.x * radius, normal.y * radius, ball.position.z);
    let wall = Vec3::new(contact.x, contact.y, 0.0) * growth;
    bounce(ball, -normal, wall, restitution).map(|speed| (contact, speed))
}

/// Keeps a ball in front of the tunnel's back wall, which moves forward at `rate`.
fn collide_back(ball: &mut Ball3, back: f32, rate: f32, restitution: f32) -> Option<(Vec3, f32)> {
    let limit = back + ball.radius;
    if ball.position.z >= limit {
        return None;
    }
    ball.position.z = limit;
    let contact = Vec3::new(ball.position.x, ball.position.y, back);
    bounce(ball, Vec3::Z, Vec3::new(0.0, 0.0, rate), restitution).map(|speed| (contact, speed))
}

enum GateTouch {
    None,
    Bounce(Vec3, f32),
    /// The ball's centre passed through the gap.
    Through,
}

/// Whether a direction (radians) lies within a gate's gap.
fn in_gap(gate: &Gate, angle: f32) -> bool {
    let offset = (angle - gate.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    offset.abs() <= gate.gap_half
}

fn collide_gate(ball: &mut Ball3, prev: Vec3, gate: &Gate, restitution: f32) -> GateTouch {
    if ball.position.z + ball.radius <= gate.z {
        return GateTouch::None;
    }
    let angle = ball.position.y.atan2(ball.position.x);
    if in_gap(gate, angle) {
        // Inside the gap the ball can pass; once its centre is past the ring, it got through.
        return if prev.z < gate.z && ball.position.z >= gate.z { GateTouch::Through } else { GateTouch::None };
    }
    ball.position.z = gate.z - ball.radius;
    let contact = Vec3::new(ball.position.x, ball.position.y, gate.z);
    match bounce(ball, Vec3::NEG_Z, Vec3::ZERO, restitution) {
        Some(speed) => GateTouch::Bounce(contact, speed),
        None => GateTouch::None,
    }
}

fn apply_bounce_rules(ball: &mut Ball3, scene: &Scene, ceiling: f32) {
    let rules = &scene.rules;
    ball.wall_hits += 1;
    if rules.speed_gain != 1.0 {
        ball.velocity *= rules.speed_gain;
    }
    let speed = ball.velocity.length();
    if rules.limit_speed && speed > rules.max_speed && speed > 0.0 {
        ball.velocity *= rules.max_speed / speed;
    } else if speed > ceiling {
        ball.velocity *= ceiling / speed;
    }
    if rules.grow != 0.0 {
        ball.radius = (ball.radius + rules.grow).clamp(0.5, rules.max_radius.max(0.5));
    }
    if !rules.recolor.is_empty() {
        let index = (ball.wall_hits as usize - 1) % rules.recolor.len();
        ball.color = rules.recolor[index].to_linear();
    }
}

/// A new ball split off `parent`, heading off at a repeatable random angle with a new colour.
fn split_ball(parent: &Ball3, index: usize, spread: f32) -> Ball3 {
    let angle = (unit_noise(index as u64) * spread).to_radians();
    // Turn around a repeatable direction at right angles to the parent's flight.
    let heading = parent.velocity.normalize_or(Vec3::X);
    let around = noise(index as u64 ^ 0x5eed) * std::f32::consts::TAU;
    let side = heading.any_orthonormal_vector();
    let axis = glam::Quat::from_axis_angle(heading, around) * side;
    Ball3 {
        position: parent.position,
        velocity: glam::Quat::from_axis_angle(axis, angle) * parent.velocity,
        radius: parent.radius,
        color: rotate_hue(parent.color, 137.5f32.to_radians()),
        wall_hits: 0,
    }
}

/// Where a spawner's balls start along z, and their speed along z.
fn spawner_depth(spawner: &Spawner) -> (f32, f32) {
    match *spawner {
        Spawner::Grid { depth, depth_velocity, .. }
        | Spawner::Single { depth, depth_velocity, .. }
        | Spawner::Stream { depth, depth_velocity, .. } => (depth, depth_velocity),
    }
}

/// Every ball the spawners make, as 3D balls, with the step each appears on, earliest first.
/// Balls are scattered in depth by [`Space::scatter`], so a flat scene switched to 3D fills the space.
fn spawn_plan(scene: &Scene, space: &Space, steps_per_second: u32) -> Vec<(u64, Ball3)> {
    let reach = space.container.size() * 0.5;
    let scatter = space.scatter.clamp(0.0, 1.0);
    let mut plan = Vec::new();
    let mut add = |step: u64, position: Vec2, velocity: Vec2, radius: f32, color: [f32; 4], depth: (f32, f32)| {
        let seed = plan.len() as u64 * 7 + 0xd3;
        let z = depth.0 + unit_noise(seed) * reach * scatter;
        let vz = depth.1 + unit_noise(seed + 1) * velocity.length() * scatter;
        let ball = Ball3 { position: position.extend(z), velocity: velocity.extend(vz), radius, color, wall_hits: 0 };
        plan.push((step, ball));
    };
    for spawner in &scene.spawners {
        let depth = spawner_depth(spawner);
        match spawner {
            Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient, .. } => {
                let size = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * *spacing;
                let origin = *center - size * 0.5;
                for row in 0..*rows {
                    let t = if *rows > 1 { row as f32 / (*rows - 1) as f32 } else { 0.0 };
                    let color = Color::sample_gradient(gradient, t);
                    for column in 0..*columns {
                        let position = origin + Vec2::new(column as f32, row as f32) * *spacing;
                        add(0, position, *velocity, *radius, color, depth);
                    }
                }
            }
            Spawner::Single { position, velocity, radius, color, .. } => {
                add(0, *position, *velocity, *radius, color.to_linear(), depth)
            }
            Spawner::Stream { position, velocity, radius, count, interval, jitter, gradient, .. } => {
                for i in 0..*count {
                    let t = if *count > 1 { i as f32 / (*count - 1) as f32 } else { 0.0 };
                    let offset = Vec2::new(unit_noise(i as u64 * 2 + 1) * jitter, 0.0);
                    let step = (i as f64 * interval.max(0.0) as f64 * steps_per_second as f64).round() as u64;
                    let color = Color::sample_gradient(gradient, t);
                    add(step, *position + offset, *velocity, *radius, color, depth);
                }
            }
        }
    }
    // Stable, so balls that appear together keep the spawners' order.
    plan.sort_by_key(|(step, _)| *step);
    plan
}

/// The 3D side of a [`crate::Playback`]: the 3D simulation, with snapshots taken at the same
/// steps as the playback's own, so seeking backwards restores both together.
#[derive(Clone)]
pub(crate) struct SpaceTrack {
    pub(crate) sim: SpaceSimulation,
    checkpoints: Vec<SpaceSimulation>,
}

impl SpaceTrack {
    pub(crate) fn new(scene: &Scene) -> Self {
        let sim = SpaceSimulation::new(scene);
        Self { checkpoints: vec![sim.clone()], sim }
    }

    /// Runs one step, adding its events to the shared log in time order. `from` is where this
    /// step's events start in `events`, which the 2D side may have added to already.
    pub(crate) fn step(&mut self, events: &mut Vec<Event>, from: usize) {
        self.sim.step(events);
        events[from..].sort_by(|a, b| a.time.total_cmp(&b.time));
    }

    /// Goes back to the snapshot taken at `step`, which the 2D side has just gone back to.
    pub(crate) fn restore(&mut self, step: u64) {
        let index = self.checkpoints.partition_point(|sim| sim.step <= step).max(1) - 1;
        self.sim = self.checkpoints[index].clone();
    }

    pub(crate) fn save_checkpoint(&mut self) {
        self.checkpoints.push(self.sim.clone());
    }

    /// Keeps only the snapshots at multiples of `every` steps, as the 2D side does.
    pub(crate) fn thin(&mut self, every: u64) {
        self.checkpoints.retain(|sim| sim.step.is_multiple_of(every));
    }

    pub(crate) fn memory(&self) -> usize {
        self.checkpoints.iter().map(SpaceSimulation::memory).sum()
    }
}

/// A copy of a 3D scene for the 2D simulation that keeps time beside the 3D one: no balls and
/// a plain arena, so it costs next to nothing.
pub fn time_keeper(scene: &Scene) -> Scene {
    Scene {
        spawners: Vec::new(),
        obstacles: Vec::new(),
        container: Scene::default().container,
        arena_motion: Default::default(),
        animation: Vec::new(),
        ..scene.clone()
    }
}

/// The balls a 3D scene starts with.
pub fn spawn_balls_3d(scene: &Scene) -> Vec<Ball3> {
    let space = scene.space().cloned().unwrap_or_default();
    spawn_plan(scene, &space, scene.physics.steps_per_second.max(1))
        .into_iter()
        .take_while(|(step, _)| *step == 0)
        .map(|(_, ball)| ball)
        .collect()
}

/// A uniform grid for finding touching balls without allocating every step: balls are sorted
/// into buckets by a hash of their cell, then each ball checks the buckets of the 27 cells
/// around it.
#[derive(Clone, Default)]
struct BallGrid {
    starts: Vec<u32>,
    sorted: Vec<u32>,
    keys: Vec<u32>,
}

impl BallGrid {
    fn collide(
        &mut self,
        balls: &mut [Ball3],
        restitution: f32,
        loud: f32,
        touches: &mut Vec<(u32, Vec3, f32, Touch)>,
    ) {
        let max_radius = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
        if max_radius <= 0.0 || !max_radius.is_finite() {
            return;
        }
        let cell = max_radius * 2.0;
        let buckets = (balls.len() * 2).next_power_of_two();
        let mask = buckets as u32 - 1;
        let cell_of = |p: Vec3| {
            let c = (p / cell).floor();
            (c.x as i32, c.y as i32, c.z as i32)
        };
        let hash = |(x, y, z): (i32, i32, i32)| {
            ((x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ (z as u32).wrapping_mul(0xcb1a_b31f))
                & mask
        };
        self.starts.clear();
        self.starts.resize(buckets + 1, 0);
        self.keys.clear();
        for ball in balls.iter() {
            let key = hash(cell_of(ball.position));
            self.keys.push(key);
            self.starts[key as usize + 1] += 1;
        }
        for i in 0..buckets {
            self.starts[i + 1] += self.starts[i];
        }
        self.sorted.clear();
        self.sorted.resize(balls.len(), 0);
        let mut fill = self.starts.clone();
        for (i, &key) in self.keys.iter().enumerate() {
            self.sorted[fill[key as usize] as usize] = i as u32;
            fill[key as usize] += 1;
        }
        let mut near = [0u32; 27];
        for i in 0..balls.len() {
            let (cx, cy, cz) = cell_of(balls[i].position);
            let mut count = 0;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        near[count] = hash((cx + dx, cy + dy, cz + dz));
                        count += 1;
                    }
                }
            }
            // Two cells can share a bucket; check each bucket once.
            near.sort_unstable();
            let mut last = u32::MAX;
            for &bucket in &near {
                if bucket == last {
                    continue;
                }
                last = bucket;
                let (start, end) = (self.starts[bucket as usize] as usize, self.starts[bucket as usize + 1] as usize);
                for k in start..end {
                    let j = self.sorted[k] as usize;
                    if j > i
                        && let Some((contact, speed)) = resolve_pair(balls, i, j, restitution)
                        && speed >= loud
                    {
                        touches.push((i as u32, contact, speed, Touch::Ball(j as u32)));
                    }
                }
            }
        }
    }
}

/// Pushes two overlapping balls apart and bounces them. Returns the contact point and closing
/// speed if they were moving together.
fn resolve_pair(balls: &mut [Ball3], i: usize, j: usize, restitution: f32) -> Option<(Vec3, f32)> {
    let (left, right) = balls.split_at_mut(j);
    let (a, b) = (&mut left[i], &mut right[0]);
    let offset = b.position - a.position;
    let distance = offset.length();
    let overlap = a.radius + b.radius - distance;
    if overlap <= 0.0 || distance == 0.0 {
        return None;
    }
    let normal = offset / distance;
    let (mass_a, mass_b) = (a.radius * a.radius * a.radius, b.radius * b.radius * b.radius);
    let total = (mass_a + mass_b).max(1e-6);
    a.position -= normal * overlap * (mass_b / total);
    b.position += normal * overlap * (mass_a / total);
    let closing = (a.velocity - b.velocity).dot(normal);
    if closing <= 0.0 {
        return None;
    }
    let impulse = closing * (1.0 + restitution) / total;
    a.velocity -= normal * impulse * mass_b;
    b.velocity += normal * impulse * mass_a;
    Some((a.position + normal * a.radius, closing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::View;

    fn space_scene(container: Container3d) -> Scene {
        let mut scene = Scene::default();
        scene.view = View::Space(Box::new(Space { container, ..Space::default() }));
        scene.video.duration = 10.0;
        scene
    }

    fn single(position: Vec2, velocity: Vec2, radius: f32, depth: f32, depth_velocity: f32) -> Spawner {
        Spawner::Single { position, velocity, radius, color: Color::WHITE, depth, depth_velocity }
    }

    fn run(scene: &Scene, seconds: f64) -> (SpaceSimulation, Vec<Event>) {
        let mut sim = SpaceSimulation::new(scene);
        let mut events = Vec::new();
        sim.advance_to(seconds, &mut events);
        (sim, events)
    }

    #[test]
    fn same_scene_gives_same_events() {
        for container in [Container3d::Sphere { radius: 400.0 }, Container3d::Cube { size: 700.0 }, Container3d::tunnel(350.0)] {
            let mut scene = space_scene(container);
            scene.physics.ball_collisions = true;
            let (a_sim, a) = run(&scene, 6.0);
            let (b_sim, b) = run(&scene, 6.0);
            assert!(!a.is_empty());
            assert_eq!(a, b);
            assert_eq!(a_sim.balls(), b_sim.balls());
        }
    }

    /// Seeking through a playback, backwards and forwards and in odd steps, gives exactly the
    /// events and balls of one straight run, including after its snapshots are thinned.
    #[test]
    fn playback_seeks_match_a_straight_run() {
        for container in [Container3d::Sphere { radius: 400.0 }, Container3d::Cube { size: 700.0 }, Container3d::tunnel(350.0)] {
            let mut scene = space_scene(container);
            scene.physics.ball_collisions = true;
            scene.rules.spawn_per_escape = 1;
            let mut straight = crate::Playback::new(&scene);
            straight.seek(4.0);
            let mut jumpy = crate::Playback::new(&scene);
            for time in [2.0, 0.6, 3.3, 0.1, 3.9, 2.2, 4.0] {
                jumpy.seek(time);
            }
            assert_eq!(jumpy.events(), straight.events());
            assert_eq!(jumpy.space().unwrap().balls(), straight.space().unwrap().balls());
            // The flat side has no balls: it only keeps time.
            assert!(straight.simulation().balls().is_empty());
            assert_eq!(straight.counts_at(4.0).balls as usize, straight.space().unwrap().balls().len());
            assert_eq!(crate::collect_wall_hits(&scene).len(), {
                let mut whole = crate::Playback::new(&scene);
                whole.seek(scene.video.duration as f64);
                whole.events().len()
            });
        }
    }

    #[test]
    fn balls_stay_inside_the_sphere_and_cube() {
        for container in [Container3d::Sphere { radius: 400.0 }, Container3d::Cube { size: 700.0 }] {
            let mut scene = space_scene(container.clone());
            scene.arena_motion.spin = 60.0;
            let mut sim = SpaceSimulation::new(&scene);
            let mut events = Vec::new();
            for step in 0..2400 {
                sim.step(&mut events);
                for ball in sim.balls() {
                    assert!(ball.position.is_finite(), "step {step}");
                    let reach = match container {
                        Container3d::Sphere { radius } => ball.position.length() + ball.radius - radius,
                        Container3d::Cube { size } => {
                            let local = glam::Quat::from_rotation_y(-sim.arena().angle) * ball.position;
                            local.abs().max_element() + ball.radius - size * 0.5
                        }
                        _ => unreachable!(),
                    };
                    assert!(reach < 0.05, "{} ball outside by {reach} at step {step}", container.name());
                }
            }
            assert!(events.iter().all(|e| e.kind == EventKind::Wall && e.depth.is_finite()));
        }
    }

    #[test]
    fn events_report_depth() {
        let (_, events) = run(&space_scene(Container3d::Sphere { radius: 400.0 }), 5.0);
        assert!(events.iter().any(|e| e.depth.abs() > 10.0));
        assert!(events.windows(2).all(|w| w[0].time <= w[1].time));
    }

    #[test]
    fn tunnel_rings_break_in_order_and_the_back_wall_moves() {
        let mut scene = space_scene(Container3d::tunnel(300.0));
        scene.spawners = vec![single(Vec2::new(0.0, 0.0), Vec2::new(300.0, -200.0), 20.0, 100.0, 300.0)];
        scene.physics.gravity = Vec2::new(0.0, 600.0);
        let (sim, events) = run(&scene, 20.0);
        let escapes: Vec<u32> =
            events.iter().filter_map(|e| if let EventKind::Escape(k) = e.kind { Some(k) } else { None }).collect();
        assert!(escapes.len() >= 3, "only {escapes:?}");
        assert!(escapes.windows(2).all(|w| w[1] == w[0] + 1), "{escapes:?}");
        assert!(sim.arena().back > 1000.0);
        // Nothing is behind the back wall or ahead of the first unbroken ring.
        let next = sim.gates().next().map_or(f32::INFINITY, |g| g.z);
        for ball in sim.balls() {
            assert!(ball.position.z - ball.radius >= sim.arena().back - 0.01);
            assert!(ball.position.z < next + 0.01);
            assert!(Vec2::new(ball.position.x, ball.position.y).length() + ball.radius <= 300.01);
        }
    }

    #[test]
    fn escapes_split_balls_in_the_tunnel() {
        let mut scene = space_scene(Container3d::tunnel(300.0));
        scene.rules.spawn_per_escape = 2;
        scene.rules.max_balls = 40;
        scene.spawners = vec![single(Vec2::ZERO, Vec2::new(250.0, 0.0), 15.0, 50.0, 200.0)];
        let (sim, _) = run(&scene, 15.0);
        assert!(sim.balls().len() > 1);
        assert!(sim.balls().len() <= 40);
    }

    #[test]
    fn ball_collisions_push_balls_apart_and_make_events() {
        let mut scene = space_scene(Container3d::Sphere { radius: 500.0 });
        scene.physics.ball_collisions = true;
        scene.physics.gravity = Vec2::ZERO;
        let single = |x: f32, vx: f32| single(Vec2::new(x, 0.0), Vec2::new(vx, 0.0), 10.0, 0.0, 0.0);
        scene.spawners = vec![single(-50.0, 200.0), single(50.0, -200.0)];
        scene.space_mut().unwrap().scatter = 0.0;
        let (sim, events) = run(&scene, 0.5);
        let [a, b] = sim.balls() else { unreachable!() };
        assert!(a.velocity.x < 0.0 && b.velocity.x > 0.0);
        assert!(events.iter().any(|e| e.kind == EventKind::Ball(1)));
    }

    #[test]
    fn a_crowd_with_ball_collisions_holds_together() {
        let mut scene = space_scene(Container3d::Sphere { radius: 420.0 });
        scene.physics.ball_collisions = true;
        let (sim, _) = run(&scene, 5.0);
        assert_eq!(sim.balls().len(), 784);
        for ball in sim.balls() {
            assert!(ball.position.is_finite() && ball.velocity.is_finite());
            assert!(ball.position.length() + ball.radius <= 420.05);
        }
    }

    #[test]
    fn zero_and_one_ball_scenes_run() {
        let mut scene = space_scene(Container3d::Cube { size: 400.0 });
        scene.spawners.clear();
        let (sim, events) = run(&scene, 2.0);
        assert!(sim.balls().is_empty() && events.is_empty());
        scene.spawners = vec![single(Vec2::ZERO, Vec2::new(100.0, 0.0), 500.0, 0.0, 0.0)];
        // A ball bigger than the box sits in the middle without blowing up.
        let (sim, _) = run(&scene, 2.0);
        assert!(sim.balls()[0].position.is_finite());
    }

    #[test]
    fn bounce_rules_and_shrinking_work_in_3d() {
        let mut scene = space_scene(Container3d::Sphere { radius: 400.0 });
        scene.rules.grow = 1.0;
        scene.rules.recolor = vec![Color::rgb(255, 0, 0)];
        scene.rules.spawn_per_hit = 1;
        scene.rules.max_balls = 900;
        scene.arena_motion.shrink_speed = 20.0;
        scene.arena_motion.min_size = 250.0;
        let (sim, _) = run(&scene, 10.0);
        assert_eq!(sim.balls().len(), 900);
        assert!((sim.arena().scale * 400.0 - 250.0).abs() < 1.0);
        assert!(sim.balls().iter().any(|b| b.radius > 5.0));
    }

    #[test]
    fn trail_holds_recent_positions() {
        let (sim, _) = run(&space_scene(Container3d::Sphere { radius: 400.0 }), 1.0);
        assert!(sim.trail(0).count() > 1);
    }
}
