//! Deterministic 2D ball physics.
//!
//! The simulation always advances in fixed steps (`Physics::steps_per_second`), never by
//! the screen's frame time. Running the same scene twice gives exactly the same balls and
//! the same wall hits, which is what keeps the preview, the exported video and the sound
//! in sync.

use bpe_scene::{Color, Container, Scene, Spawner, Vec2};

/// How often ball positions are recorded for drawing trails, in samples per second.
const TRAIL_SAMPLE_RATE: u32 = 120;
const MAX_TRAIL_SAMPLES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    /// Linear-light RGBA.
    pub color: [f32; 4],
    pub wall_hits: u32,
}

/// One ball touching the container wall while moving into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    /// Seconds from the start of the clip.
    pub time: f64,
    pub ball: u32,
    /// Where the ball touched the wall.
    pub position: Vec2,
    /// Speed into the wall just before the bounce, in pixels per second.
    pub impact_speed: f32,
}

#[derive(Clone)]
pub struct Simulation {
    scene: Scene,
    balls: Vec<Ball>,
    step: u64,
    dt: f32,
    /// Ball positions sampled every `trail_every` steps, `trail_len` per ball, as a ring.
    trail: Vec<Vec2>,
    trail_len: usize,
    trail_every: u64,
    trail_head: usize,
    trail_filled: usize,
}

impl Simulation {
    pub fn new(scene: &Scene) -> Self {
        let balls = spawn_balls(scene);
        let steps_per_second = scene.physics.steps_per_second.max(1);
        let trail_every = (steps_per_second / TRAIL_SAMPLE_RATE).max(1) as u64;
        let sample_rate = steps_per_second as f32 / trail_every as f32;
        let trail_len = ((scene.look.trail_seconds.max(0.0) * sample_rate).ceil() as usize + 1).min(MAX_TRAIL_SAMPLES);
        let mut sim = Self {
            scene: scene.clone(),
            trail: vec![Vec2::ZERO; balls.len() * trail_len],
            balls,
            step: 0,
            dt: 1.0 / steps_per_second as f32,
            trail_len,
            trail_every,
            trail_head: 0,
            trail_filled: 0,
        };
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
        let physics = &self.scene.physics;
        let drag = (1.0 - physics.air_drag * dt).max(0.0);
        for ball in &mut self.balls {
            ball.velocity = (ball.velocity + physics.gravity * dt) * drag;
            ball.position += ball.velocity * dt;
        }
        if physics.ball_collisions {
            collide_balls(&mut self.balls, physics.restitution);
        }
        self.step += 1;
        let time = self.time();
        for (index, ball) in self.balls.iter_mut().enumerate() {
            if let Some((position, impact_speed)) = collide_wall(ball, &self.scene.container, physics.restitution) {
                apply_bounce_rules(ball, &self.scene);
                hits.push(WallHit { time, ball: index as u32, position, impact_speed });
            }
        }
        if self.step.is_multiple_of(self.trail_every) {
            self.record_trail();
        }
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

pub fn spawn_balls(scene: &Scene) -> Vec<Ball> {
    let mut balls = Vec::new();
    for spawner in &scene.spawners {
        match spawner {
            Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient } => {
                let size = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * *spacing;
                let origin = *center - size * 0.5;
                for row in 0..*rows {
                    let t = if *rows > 1 { row as f32 / (*rows - 1) as f32 } else { 0.0 };
                    let color = Color::sample_gradient(gradient, t);
                    for column in 0..*columns {
                        balls.push(Ball {
                            position: origin + Vec2::new(column as f32, row as f32) * *spacing,
                            velocity: *velocity,
                            radius: *radius,
                            color,
                            wall_hits: 0,
                        });
                    }
                }
            }
            Spawner::Single { position, velocity, radius, color } => balls.push(Ball {
                position: *position,
                velocity: *velocity,
                radius: *radius,
                color: color.to_linear(),
                wall_hits: 0,
            }),
        }
    }
    balls
}

/// Keeps the ball inside the container. Returns the contact point and impact speed when
/// the ball was moving into the wall.
fn collide_wall(ball: &mut Ball, container: &Container, restitution: f32) -> Option<(Vec2, f32)> {
    match container {
        Container::Circle { center, radius, thickness, .. } => {
            let inner = radius - thickness * 0.5 - ball.radius;
            let offset = ball.position - *center;
            let distance = offset.length();
            if distance <= inner || distance == 0.0 {
                return None;
            }
            let normal = offset / distance;
            ball.position = *center + normal * inner.max(0.0);
            reflect(ball, normal, restitution).map(|speed| (ball.position + normal * ball.radius, speed))
        }
        Container::Rect { center, size, thickness, .. } => {
            let half = *size * 0.5 - Vec2::splat(thickness * 0.5 + ball.radius);
            let local = ball.position - *center;
            let mut hit: Option<(Vec2, f32)> = None;
            for (axis, normal) in [(0, Vec2::X), (1, Vec2::Y)] {
                for sign in [1.0, -1.0] {
                    let n = normal * sign;
                    if local[axis] * sign > half[axis] {
                        ball.position[axis] = center[axis] + half[axis] * sign;
                        if let Some(speed) = reflect(ball, n, restitution) {
                            let contact = ball.position + n * ball.radius;
                            if hit.is_none_or(|(_, s)| speed > s) {
                                hit = Some((contact, speed));
                            }
                        }
                    }
                }
            }
            hit
        }
    }
}

/// Bounces the velocity off a wall with outward `normal`. Returns the impact speed if the
/// ball was moving into the wall.
fn reflect(ball: &mut Ball, normal: Vec2, restitution: f32) -> Option<f32> {
    let into = ball.velocity.dot(normal);
    if into <= 0.0 {
        return None;
    }
    ball.velocity -= normal * into * (1.0 + restitution);
    Some(into)
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
}
