//! Deterministic 2D ball physics.
//!
//! The simulation always advances in fixed steps (`Physics::steps_per_second`), never by
//! the screen's frame time. Running the same scene twice gives exactly the same balls and
//! the same wall hits, which is what keeps the preview, the exported video and the sound
//! in sync.

mod arena;

pub use arena::{ArenaState, Outline, Piece, closed_outline, outline_pieces, ring_outline};

use bpe_scene::{Color, Container, ObstaclePart, Scene, Spawner, Vec2};

/// How often ball positions are recorded for drawing trails, in samples per second.
const TRAIL_SAMPLE_RATE: u32 = 120;
const MAX_TRAIL_SAMPLES: usize = 256;
/// The arena never grows past this multiple of its starting size.
const MAX_ARENA_SCALE: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    /// Linear-light RGBA.
    pub color: [f32; 4],
    pub wall_hits: u32,
}

/// What a ball hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// The arena's wall, or one of its rings.
    Wall,
    /// A peg or wall piece, by its index in [`Scene::obstacle_parts`].
    Obstacle(u32),
    /// The ball escaped through the gap of this ring, which shattered.
    Escape(u32),
}

/// One ball touching a wall or peg while moving into it, or escaping a ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    /// Seconds from the start of the clip.
    pub time: f64,
    pub ball: u32,
    /// Where the ball touched the wall.
    pub position: Vec2,
    /// Speed into the wall just before the bounce, in pixels per second. For an escape, the
    /// ball's speed.
    pub impact_speed: f32,
    pub kind: HitKind,
}

/// When and how a ring broke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingBreak {
    pub time: f64,
    /// The ring's outline at that moment.
    pub outline: Outline,
}

#[derive(Clone)]
pub struct Simulation {
    scene: Scene,
    balls: Vec<Ball>,
    step: u64,
    dt: f32,
    arena: ArenaState,
    /// Starting arena size that shrinking is measured against.
    arena_size: f32,
    /// One entry per ring of a `Container::Rings` arena, set once it breaks.
    breaks: Vec<Option<RingBreak>>,
    obstacles: Vec<ObstaclePart>,
    /// Balls that appear later, as (step, ball), latest first.
    pending: Vec<(u64, Ball)>,
    /// Scratch space for wall pieces and positions before the step.
    pieces: Vec<(u32, Piece)>,
    previous: Vec<Vec2>,
    /// Ball positions sampled every `trail_every` steps, `trail_len` per ball, as a ring.
    trail: Vec<Vec2>,
    trail_len: usize,
    trail_every: u64,
    trail_head: usize,
    trail_filled: usize,
}

impl Simulation {
    pub fn new(scene: &Scene) -> Self {
        let steps_per_second = scene.physics.steps_per_second.max(1);
        let mut pending = spawn_plan(scene, steps_per_second);
        let split = pending.partition_point(|(step, _)| *step == 0);
        let balls: Vec<Ball> = pending.drain(..split).map(|(_, ball)| ball).collect();
        pending.reverse();
        let trail_every = (steps_per_second / TRAIL_SAMPLE_RATE).max(1) as u64;
        let sample_rate = steps_per_second as f32 / trail_every as f32;
        let trail_len = ((scene.look.trail_seconds.max(0.0) * sample_rate).ceil() as usize + 1).min(MAX_TRAIL_SAMPLES);
        let rings = match scene.container {
            Container::Rings { count, .. } => count as usize,
            _ => 0,
        };
        let mut sim = Self {
            scene: scene.clone(),
            trail: vec![Vec2::ZERO; balls.len() * trail_len],
            balls,
            step: 0,
            dt: 1.0 / steps_per_second as f32,
            arena: ArenaState::default(),
            arena_size: scene.container.size().max(1.0),
            breaks: vec![None; rings],
            obstacles: scene.obstacle_parts(),
            pending,
            pieces: Vec::new(),
            previous: Vec::new(),
            trail_len,
            trail_every,
            trail_head: 0,
            trail_filled: 0,
        };
        sim.update_arena();
        sim.record_trail();
        sim
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn balls(&self) -> &[Ball] {
        &self.balls
    }

    pub fn time(&self) -> f64 {
        self.step as f64 / self.scene.physics.steps_per_second.max(1) as f64
    }

    /// The arena's current turn and size.
    pub fn arena(&self) -> ArenaState {
        self.arena
    }

    /// Pegs and wall pieces, as [`Scene::obstacle_parts`] lists them.
    pub fn obstacles(&self) -> &[ObstaclePart] {
        &self.obstacles
    }

    /// One entry per ring of an escape-rings arena: when it broke, if it has.
    pub fn ring_breaks(&self) -> &[Option<RingBreak>] {
        &self.breaks
    }

    /// Outlines of the arena as it is now: the closed arena, or each unbroken ring with its index.
    pub fn outlines(&self) -> Vec<(Option<u32>, Outline)> {
        let container = &self.scene.container;
        if let Some(outline) = closed_outline(container, self.arena) {
            return vec![(None, outline)];
        }
        let time = self.time();
        (0..self.breaks.len() as u32)
            .filter(|&k| self.breaks[k as usize].is_none())
            .filter_map(|k| ring_outline(container, k, time, self.arena).map(|o| (Some(k), o)))
            .collect()
    }

    /// Seconds between trail samples.
    pub fn trail_interval(&self) -> f32 {
        self.dt * self.trail_every as f32
    }

    /// Past positions of one ball, newest first, ending at most `Look::trail_seconds` ago.
    /// The ball's current position is not included.
    pub fn trail(&self, ball: usize) -> impl Iterator<Item = Vec2> + '_ {
        let base = ball * self.trail_len;
        (0..self.trail_filled).map(move |k| {
            let slot = (self.trail_head + self.trail_len - k) % self.trail_len;
            self.trail[base + slot]
        })
    }

    /// Runs whole steps until the simulation reaches `time`, appending wall hits to `hits`.
    pub fn advance_to(&mut self, time: f64, hits: &mut Vec<WallHit>) {
        let target = (time * self.scene.physics.steps_per_second as f64).floor() as u64;
        while self.step < target {
            self.step(hits);
        }
    }

    pub fn step(&mut self, hits: &mut Vec<WallHit>) {
        let dt = self.dt;
        while self.pending.last().is_some_and(|(step, _)| *step <= self.step) {
            let (_, ball) = self.pending.pop().unwrap();
            self.add_ball(ball);
        }
        let physics = &self.scene.physics;
        let restitution = physics.restitution;
        let drag = (1.0 - physics.air_drag * dt).max(0.0);
        self.previous.clear();
        self.previous.extend(self.balls.iter().map(|b| b.position));
        for ball in &mut self.balls {
            ball.velocity = (ball.velocity + physics.gravity * dt) * drag;
            ball.position += ball.velocity * dt;
        }
        if physics.ball_collisions {
            collide_balls(&mut self.balls, restitution);
        }
        self.step += 1;
        self.update_arena();
        let time = self.time();
        let container = &self.scene.container;
        let closed = closed_outline(container, self.arena);
        let ring_outlines: Vec<(u32, Outline)> = if closed.is_none() {
            (0..self.breaks.len() as u32)
                .filter(|&k| self.breaks[k as usize].is_none())
                .filter_map(|k| ring_outline(container, k, time, self.arena).map(|o| (k, o)))
                .collect()
        } else {
            Vec::new()
        };
        self.pieces.clear();
        for (k, outline) in &ring_outlines {
            let mut pieces = Vec::new();
            outline_pieces(outline, &mut pieces);
            self.pieces.extend(pieces.into_iter().map(|p| (*k, p)));
        }
        let growth = self.arena.scale_rate / self.arena.scale.max(1e-3);
        let pivot = container.center();

        // Touches slower than this are balls resting on something, not bounces. They still push
        // the ball out but don't count as hits, so resting balls don't buzz or flicker.
        let resting = self.scene.physics.gravity.length() * dt * 4.0 + 1.0;
        let mut events = Vec::new();
        for (index, ball) in self.balls.iter_mut().enumerate() {
            let prev = self.previous[index];
            let mut wall_hit = match &closed {
                Some(outline) => arena::collide_closed(ball, outline, self.arena, restitution),
                None => None,
            };
            for (k, piece) in &self.pieces {
                let spin = arena::ring_spin(container, *k, self.arena);
                // Where the ball was last step as seen by the turning ring, so a ring that swept
                // past the ball can't make it look as if the ball came from the other side.
                let (sin, cos) = (spin * dt).sin_cos();
                let r = (prev - pivot) * (1.0 + growth * dt);
                let prev = pivot + Vec2::new(r.x * cos - r.y * sin, r.x * sin + r.y * cos);
                let thickness = container.thickness();
                if let Some((position, speed)) =
                    arena::collide_piece(ball, prev, piece, thickness, pivot, spin, growth, restitution)
                    && wall_hit.is_none_or(|(_, s)| speed > s)
                {
                    wall_hit = Some((position, speed));
                }
            }
            if let Some((position, impact_speed)) = wall_hit.filter(|(_, speed)| *speed >= resting) {
                events.push(WallHit { time, ball: index as u32, position, impact_speed, kind: HitKind::Wall });
            }
            for (part, obstacle) in self.obstacles.iter().enumerate() {
                let hit = match *obstacle {
                    ObstaclePart::Disc { center, radius, .. } => arena::collide_disc(ball, center, radius, restitution),
                    ObstaclePart::Segment { from, to, thickness, .. } => {
                        let piece = Piece::Segment { from, to };
                        arena::collide_piece(ball, prev, &piece, thickness, Vec2::ZERO, 0.0, 0.0, restitution)
                    }
                };
                if let Some((position, impact_speed)) = hit.filter(|(_, speed)| *speed >= resting) {
                    let kind = HitKind::Obstacle(part as u32);
                    events.push(WallHit { time, ball: index as u32, position, impact_speed, kind });
                }
            }
            for (k, outline) in &ring_outlines {
                let k = *k as usize;
                if self.breaks[k].is_none() && arena::inside(outline, prev) && !arena::inside(outline, ball.position) {
                    self.breaks[k] = Some(RingBreak { time, outline: *outline });
                    let impact_speed = ball.velocity.length();
                    let kind = HitKind::Escape(k as u32);
                    events.push(WallHit { time, ball: index as u32, position: ball.position, impact_speed, kind });
                }
            }
        }

        let mut spawns = Vec::new();
        let center = container.center();
        for event in events {
            let ball = &mut self.balls[event.ball as usize];
            let rules = &self.scene.rules;
            let children = match event.kind {
                HitKind::Escape(_) => rules.spawn_per_escape,
                HitKind::Wall | HitKind::Obstacle(_) => {
                    apply_bounce_rules(ball, &self.scene);
                    rules.spawn_per_hit
                }
            };
            // Balls split off at a bounce start where the parent is. Balls made by an escape
            // start back in the middle, so there is always a ball left to escape the next ring.
            let start = if let HitKind::Escape(_) = event.kind { center } else { ball.position };
            if event.kind == HitKind::Wall {
                self.shrink_on_hit();
            }
            spawns.extend((0..children).map(|_| (event.ball, start)));
            hits.push(event);
        }
        for (parent, start) in spawns {
            if self.balls.len() >= self.scene.rules.max_balls as usize {
                break;
            }
            let mut child = split_ball(&self.balls[parent as usize], self.balls.len(), self.scene.rules.spawn_spread);
            child.position = start;
            self.add_ball(child);
        }
        if self.step.is_multiple_of(self.trail_every) {
            self.record_trail();
        }
    }

    /// Adds a ball partway through, with a trail that starts where it is.
    fn add_ball(&mut self, ball: Ball) {
        self.trail.extend(std::iter::repeat_n(ball.position, self.trail_len));
        self.balls.push(ball);
    }

    /// Turns and shrinks the arena for the current time.
    fn update_arena(&mut self) {
        let motion = &self.scene.arena_motion;
        let time = self.time();
        self.arena.spin = motion.spin.to_radians();
        self.arena.angle = (motion.spin as f64 * time).to_radians().rem_euclid(std::f64::consts::TAU) as f32;
        if self.step == 0 {
            return;
        }
        let rate = -motion.shrink_speed / self.arena_size;
        let (low, high) = self.scale_limits();
        let scale = (self.arena.scale + rate * self.dt).clamp(low, high);
        self.arena.scale_rate = if scale == self.arena.scale { 0.0 } else { rate };
        self.arena.scale = scale;
    }

    fn shrink_on_hit(&mut self) {
        let amount = self.scene.arena_motion.shrink_per_hit / self.arena_size;
        if amount != 0.0 {
            let (low, high) = self.scale_limits();
            self.arena.scale = (self.arena.scale - amount).clamp(low, high);
        }
    }

    fn scale_limits(&self) -> (f32, f32) {
        let low = (self.scene.arena_motion.min_size.max(1.0) / self.arena_size).min(1.0);
        (low, MAX_ARENA_SCALE)
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

/// A simulation plus every wall hit so far, which can be moved to any point in time.
/// Seeking backwards replays from the start, which is cheap and keeps results exact.
#[derive(Clone)]
pub struct Playback {
    sim: Simulation,
    hits: Vec<WallHit>,
}

impl Playback {
    pub fn new(scene: &Scene) -> Self {
        Self { sim: Simulation::new(scene), hits: Vec::new() }
    }

    pub fn simulation(&self) -> &Simulation {
        &self.sim
    }

    /// All wall hits up to the current time, oldest first.
    pub fn hits(&self) -> &[WallHit] {
        &self.hits
    }

    /// Wall hits within the last `seconds`.
    pub fn recent_hits(&self, seconds: f64) -> &[WallHit] {
        let since = self.sim.time() - seconds;
        let start = self.hits.partition_point(|h| h.time <= since);
        &self.hits[start..]
    }

    pub fn seek(&mut self, time: f64) {
        if time < self.sim.time() {
            *self = Self::new(self.sim.scene());
        }
        self.sim.advance_to(time, &mut self.hits);
    }
}

/// Runs a whole scene from the start and returns every wall hit, for building the soundtrack.
pub fn collect_wall_hits(scene: &Scene) -> Vec<WallHit> {
    let mut sim = Simulation::new(scene);
    let mut hits = Vec::new();
    sim.advance_to(scene.video.duration as f64, &mut hits);
    hits
}

/// Every ball the scene's spawners make, with the step it appears on, earliest first.
fn spawn_plan(scene: &Scene, steps_per_second: u32) -> Vec<(u64, Ball)> {
    let ball = |position, velocity, radius, color| Ball { position, velocity, radius, color, wall_hits: 0 };
    let mut plan = Vec::new();
    for spawner in &scene.spawners {
        match spawner {
            Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient } => {
                let size = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * *spacing;
                let origin = *center - size * 0.5;
                for row in 0..*rows {
                    let t = if *rows > 1 { row as f32 / (*rows - 1) as f32 } else { 0.0 };
                    let color = Color::sample_gradient(gradient, t);
                    for column in 0..*columns {
                        let position = origin + Vec2::new(column as f32, row as f32) * *spacing;
                        plan.push((0, ball(position, *velocity, *radius, color)));
                    }
                }
            }
            Spawner::Single { position, velocity, radius, color } => {
                plan.push((0, ball(*position, *velocity, *radius, color.to_linear())))
            }
            Spawner::Stream { position, velocity, radius, count, interval, jitter, gradient } => {
                for i in 0..*count {
                    let t = if *count > 1 { i as f32 / (*count - 1) as f32 } else { 0.0 };
                    let offset = Vec2::new(unit_noise(i as u64 * 2 + 1) * jitter, 0.0);
                    let step = (i as f64 * interval.max(0.0) as f64 * steps_per_second as f64).round() as u64;
                    let color = Color::sample_gradient(gradient, t);
                    plan.push((step, ball(*position + offset, *velocity, *radius, color)));
                }
            }
        }
    }
    // Stable, so balls that appear together keep the spawners' order.
    plan.sort_by_key(|(step, _)| *step);
    plan
}

/// The balls a scene starts with.
pub fn spawn_balls(scene: &Scene) -> Vec<Ball> {
    spawn_plan(scene, scene.physics.steps_per_second.max(1))
        .into_iter()
        .take_while(|(step, _)| *step == 0)
        .map(|(_, ball)| ball)
        .collect()
}

/// A new ball split off `parent`, heading off at a repeatable random angle with a new colour.
fn split_ball(parent: &Ball, index: usize, spread: f32) -> Ball {
    let angle = (unit_noise(index as u64) * spread).to_radians();
    let (sin, cos) = angle.sin_cos();
    let v = parent.velocity;
    Ball {
        position: parent.position,
        velocity: Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos),
        radius: parent.radius,
        color: rotate_hue(parent.color, 137.5f32.to_radians()),
        wall_hits: 0,
    }
}

/// Turns a linear-light colour around the colour wheel, keeping its brightness.
fn rotate_hue(color: [f32; 4], angle: f32) -> [f32; 4] {
    let (sin, cos) = angle.sin_cos();
    let third: f32 = 1.0 / 3.0;
    let root = third.sqrt();
    let a = cos + (1.0 - cos) * third;
    let b = third * (1.0 - cos) - root * sin;
    let c = third * (1.0 - cos) + root * sin;
    let [r, g, bl, alpha] = color;
    [(r * a + g * b + bl * c).max(0.0), (r * c + g * a + bl * b).max(0.0), (r * b + g * c + bl * a).max(0.0), alpha]
}

/// A repeatable pseudo-random number between -1 and 1.
fn unit_noise(seed: u64) -> f32 {
    let mut x = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

fn apply_bounce_rules(ball: &mut Ball, scene: &Scene) {
    let rules = &scene.rules;
    ball.wall_hits += 1;
    if rules.speed_gain != 1.0 {
        ball.velocity *= rules.speed_gain;
    }
    let speed = ball.velocity.length();
    if speed > rules.max_speed && speed > 0.0 {
        ball.velocity *= rules.max_speed / speed;
    }
    if rules.grow != 0.0 {
        ball.radius = (ball.radius + rules.grow).clamp(0.5, rules.max_radius.max(0.5));
    }
    if !rules.recolor.is_empty() {
        let index = (ball.wall_hits as usize - 1) % rules.recolor.len();
        ball.color = rules.recolor[index].to_linear();
    }
}

/// Elastic collisions between balls, found with a uniform grid. Heavier (bigger) balls move less.
fn collide_balls(balls: &mut [Ball], restitution: f32) {
    let max_radius = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
    if balls.len() < 2 || max_radius <= 0.0 {
        return;
    }
    let cell = max_radius * 2.0;
    let key = |p: Vec2| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
    let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    for (i, ball) in balls.iter().enumerate() {
        grid.entry(key(ball.position)).or_default().push(i);
    }
    let mut cells: Vec<_> = grid.keys().copied().collect();
    cells.sort_unstable();
    for (cx, cy) in cells {
        let members = &grid[&(cx, cy)];
        for &i in members {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(others) = grid.get(&(cx + dx, cy + dy)) else { continue };
                    for &j in others {
                        if j > i {
                            resolve_pair(balls, i, j, restitution);
                        }
                    }
                }
            }
        }
    }
}

fn resolve_pair(balls: &mut [Ball], i: usize, j: usize, restitution: f32) {
    let (left, right) = balls.split_at_mut(j);
    let (a, b) = (&mut left[i], &mut right[0]);
    let offset = b.position - a.position;
    let distance = offset.length();
    let overlap = a.radius + b.radius - distance;
    if overlap <= 0.0 || distance == 0.0 {
        return;
    }
    let normal = offset / distance;
    let (mass_a, mass_b) = (a.radius * a.radius, b.radius * b.radius);
    let total = mass_a + mass_b;
    a.position -= normal * overlap * (mass_b / total);
    b.position += normal * overlap * (mass_a / total);
    let closing = (a.velocity - b.velocity).dot(normal);
    if closing > 0.0 {
        let impulse = closing * (1.0 + restitution) / total;
        a.velocity -= normal * impulse * mass_b;
        b.velocity += normal * impulse * mass_a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_scene_gives_same_result() {
        let scene = Scene::default();
        let a = collect_wall_hits(&scene);
        let b = collect_wall_hits(&scene);
        assert!(!a.is_empty());
        assert_eq!(a, b);
    }

    #[test]
    fn balls_stay_inside_the_ring() {
        let scene = Scene::default();
        let Container::Circle { center, radius, .. } = scene.container else { unreachable!() };
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        for second in 1..=10 {
            sim.advance_to(second as f64, &mut hits);
            for ball in sim.balls() {
                assert!((ball.position - center).length() + ball.radius <= radius + 0.01);
            }
        }
    }

    #[test]
    fn first_wall_hit_matches_reference_reel() {
        // The reference reel's block first touches the ring about 0.45 s in.
        let hits = collect_wall_hits(&Scene::default());
        let first = hits.first().unwrap().time;
        assert!((0.35..0.55).contains(&first), "first hit at {first}");
    }

    #[test]
    fn elastic_ring_keeps_energy_roughly_constant() {
        let mut scene = Scene::default();
        scene.video.duration = 20.0;
        let energy = |sim: &Simulation| -> f32 {
            sim.balls().iter().map(|b| 0.5 * b.velocity.length_squared() - scene.physics.gravity.dot(b.position)).sum()
        };
        let mut sim = Simulation::new(&scene);
        let start = energy(&sim);
        sim.advance_to(20.0, &mut Vec::new());
        let end = energy(&sim);
        assert!((end - start).abs() / start.abs() < 0.05, "energy {start} -> {end}");
    }

    #[test]
    fn ball_collisions_push_balls_apart() {
        let mut scene = Scene::default();
        scene.physics.ball_collisions = true;
        scene.physics.gravity = Vec2::ZERO;
        scene.spawners = vec![
            Spawner::Single {
                position: Vec2::new(-50.0, 0.0),
                velocity: Vec2::new(200.0, 0.0),
                radius: 10.0,
                color: Color::WHITE,
            },
            Spawner::Single {
                position: Vec2::new(50.0, 0.0),
                velocity: Vec2::new(-200.0, 0.0),
                radius: 10.0,
                color: Color::WHITE,
            },
        ];
        let mut sim = Simulation::new(&scene);
        sim.advance_to(0.5, &mut Vec::new());
        let [a, b] = sim.balls() else { unreachable!() };
        assert!(a.velocity.x < 0.0 && b.velocity.x > 0.0);
    }

    #[test]
    fn trail_holds_recent_positions_newest_first() {
        let scene = Scene::default();
        let mut sim = Simulation::new(&scene);
        sim.advance_to(1.0, &mut Vec::new());
        let trail: Vec<_> = sim.trail(0).collect();
        assert_eq!(trail.len(), sim.trail_len);
        assert!(
            trail[0].distance(sim.balls()[0].position)
                < trail[trail.len() - 1].distance(sim.balls()[0].position) + 1e-3
        );
    }

    fn escapes(hits: &[WallHit]) -> Vec<u32> {
        hits.iter().filter_map(|h| if let HitKind::Escape(k) = h.kind { Some(k) } else { None }).collect()
    }

    #[test]
    fn every_preset_is_repeatable() {
        for preset in Scene::presets() {
            let mut scene = preset.scene;
            scene.video.duration = scene.video.duration.min(12.0);
            assert_eq!(collect_wall_hits(&scene), collect_wall_hits(&scene), "{}", preset.name);
        }
    }

    #[test]
    fn escape_rings_break_from_the_inside_out() {
        let scene = Scene::escape_rings();
        let hits = collect_wall_hits(&scene);
        let broken = escapes(&hits);
        assert!(broken.len() >= 5, "only {} rings broke: {broken:?}", broken.len());
        assert!(broken.windows(2).all(|w| w[0] < w[1]), "rings broke out of order: {broken:?}");
    }

    #[test]
    fn ball_stays_inside_the_smallest_unbroken_ring() {
        let scene = Scene::escape_rings();
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        for _ in 0..(scene.video.duration * 240.0) as usize {
            sim.step(&mut hits);
            let ball = &sim.balls()[0];
            if let Some((_, outline)) = sim.outlines().first() {
                let Outline::Ring { center, radius, .. } = *outline else { unreachable!() };
                // Its centre never gets past an unbroken ring: crossing through the gap breaks it.
                let distance = ball.position.distance(center);
                assert!(distance < radius, "ball at {distance}, ring at {radius}");
            }
        }
    }

    #[test]
    fn escapes_split_the_ball() {
        let scene = Scene::escape_and_multiply();
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        sim.advance_to(scene.video.duration as f64, &mut hits);
        let count = escapes(&hits).len();
        assert!(count >= 2, "{count} escapes");
        assert!(sim.balls().len() > 1);
        assert!(sim.balls().len() <= scene.rules.max_balls as usize);
    }

    #[test]
    fn balls_stay_inside_a_spinning_hexagon() {
        let scene = Scene::spinning_hexagon();
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        for second in 1..=scene.video.duration as u32 {
            sim.advance_to(second as f64, &mut hits);
            let [(_, outline)] = sim.outlines()[..] else { unreachable!() };
            for ball in sim.balls() {
                assert!(arena::inside(&outline, ball.position), "ball escaped at {second}s");
            }
        }
        assert!(hits.len() > 50);
    }

    #[test]
    fn spinning_box_keeps_balls_in() {
        let mut scene = Scene::box_arena();
        scene.arena_motion.spin = 90.0;
        let mut sim = Simulation::new(&scene);
        sim.advance_to(10.0, &mut Vec::new());
        let [(_, outline)] = sim.outlines()[..] else { unreachable!() };
        assert!(sim.balls().iter().all(|b| arena::inside(&outline, b.position)));
    }

    #[test]
    fn multiplying_stops_at_the_ball_limit() {
        let scene = Scene::multiplying_balls();
        let mut sim = Simulation::new(&scene);
        sim.advance_to(scene.video.duration as f64, &mut Vec::new());
        assert_eq!(sim.balls().len(), scene.rules.max_balls as usize);
        // New balls get their own colours.
        assert_ne!(sim.balls()[0].color, sim.balls()[1].color);
        let trail: Vec<_> = sim.trail(sim.balls().len() - 1).collect();
        assert!(!trail.is_empty());
    }

    #[test]
    fn shrinking_ring_closes_in_and_keeps_the_ball() {
        let scene = Scene::shrinking_ring();
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        sim.advance_to(scene.video.duration as f64, &mut hits);
        let size = scene.container.size();
        let expected = scene.arena_motion.min_size / size;
        assert!((sim.arena().scale - expected).abs() < 1e-3, "scale {}", sim.arena().scale);
        let ball = &sim.balls()[0];
        assert!(ball.position.length() + ball.radius <= scene.arena_motion.min_size + 0.5);
        // Hits come faster at the end than at the start.
        let early = hits.iter().filter(|h| h.time < 5.0).count();
        let late = hits.iter().filter(|h| h.time >= scene.video.duration as f64 - 5.0).count();
        assert!(late > early, "{early} early hits, {late} late hits");
    }

    #[test]
    fn shrink_per_hit_stops_at_the_smallest_size() {
        let mut scene = Scene::growing_ball();
        scene.arena_motion.shrink_per_hit = 20.0;
        scene.arena_motion.min_size = 200.0;
        let mut sim = Simulation::new(&scene);
        sim.advance_to(10.0, &mut Vec::new());
        assert!((sim.arena().scale * scene.container.size() - 200.0).abs() < 0.01);
    }

    #[test]
    fn plinko_drops_every_ball_and_keeps_them_in_the_box() {
        let scene = Scene::plinko();
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        assert_eq!(sim.balls().len(), 1);
        sim.advance_to(scene.video.duration as f64, &mut hits);
        assert_eq!(sim.balls().len(), 160);
        let Container::Rect { size, .. } = scene.container else { unreachable!() };
        for ball in sim.balls() {
            assert!(ball.position.x.abs() <= size.x / 2.0 && ball.position.y.abs() <= size.y / 2.0);
        }
        assert!(hits.iter().any(|h| matches!(h.kind, HitKind::Obstacle(_))));
        // Most balls end up resting in the bins at the bottom.
        let settled = sim.balls().iter().filter(|b| b.position.y > 500.0).count();
        assert!(settled > 120, "{settled} balls in the bins");
    }

    #[test]
    fn stream_balls_appear_on_time() {
        let scene = Scene::plinko();
        let mut sim = Simulation::new(&scene);
        sim.advance_to(1.0, &mut Vec::new());
        // One at 0 s, then one every 0.12 s: 0, 0.12, ... 0.96.
        assert_eq!(sim.balls().len(), 9);
    }

    #[test]
    fn hue_rotation_keeps_grey_grey() {
        let grey = [0.5, 0.5, 0.5, 1.0];
        let turned = rotate_hue(grey, 1.0);
        assert!(turned.iter().zip(grey).all(|(a, b)| (a - b).abs() < 1e-5));
    }

    #[test]
    fn fast_spinning_polygon_rings_never_leak() {
        let mut scene = Scene::escape_and_multiply();
        if let Container::Rings { spin, spin_step, gap, .. } = &mut scene.container {
            *spin = 300.0;
            *spin_step = -40.0;
            *gap = 20.0;
        }
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        for _ in 0..(10.0 * 240.0) as usize {
            sim.step(&mut hits);
            let Some((_, outline)) = sim.outlines().into_iter().next() else { break };
            let escaped = escapes(&hits).len();
            for (i, ball) in sim.balls().iter().enumerate() {
                assert!(arena::inside(&outline, ball.position), "ball {i} leaked after {escaped} escapes");
            }
        }
    }
}
