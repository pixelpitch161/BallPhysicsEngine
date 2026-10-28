//! Deterministic 2D ball physics.
//!
//! The simulation always advances in fixed steps (`Physics::steps_per_second`), never by
//! the screen's frame time. Running the same scene twice gives exactly the same balls and
//! the same wall hits, which is what keeps the preview, the exported video and the sound
//! in sync.

mod arena;
pub mod beat_sync;
pub mod bodies;
pub mod broadphase;
pub mod contacts;
pub mod events;
mod grid;
pub mod rules;
pub mod shapes;
pub mod space;

pub use arena::{ArenaState, Outline, Piece, closed_outline, outline_pieces, ring_outline};
pub use contacts::Touch;
pub use events::{Counts, Event, EventKind, HitKind, NO_BALL, WallHit};

use bpe_music::AudioFeatures;
use bpe_scene::{Audio, Color, Container, Levels, Listener, ObstaclePart, Placement, Scene, Spawner, Vec2};
use grid::ObstacleGrid;
use shapes::ShapeWalls;
use std::sync::Arc;

/// The music at `time` as modulators read it (WS5). The renderer uses the same, so physics and
/// visuals stay on the same beat.
pub fn music_levels(music: &bpe_music::AudioFeatures, time: f64) -> Levels {
    let at = music.at(time);
    let beat = resting_beat_phase(Some(music), time);
    Levels { bass: at.bass, mid: at.mid, treble: at.treble, level: at.level, beat, onset: at.onset }
}

/// How far through the current beat `time` is, from 0 on the beat to 1 just before the next.
/// 1 (resting, as if the next beat were about to come) without beats, before the first and after
/// the last, so beat-driven settings stay still rather than stuck on a beat.
pub fn resting_beat_phase(music: Option<&bpe_music::AudioFeatures>, time: f64) -> f32 {
    match music {
        Some(m) if m.beats.len() >= 2 && time >= m.beats[0] && time < m.beats[m.beats.len() - 1] => m.beat_phase(time),
        _ => 1.0,
    }
}

/// How often ball positions are recorded for drawing trails, in samples per second.
const TRAIL_SAMPLE_RATE: u32 = 120;
const MAX_TRAIL_SAMPLES: usize = 256;
/// The arena never grows past this multiple of its starting size.
const MAX_ARENA_SCALE: f32 = 4.0;
/// Furthest a ball moves in one substep, in pixels. Faster balls split a step into substeps
/// so they still bounce off every wall and peg in their way. At the default 240 steps a
/// second this keeps one substep up to 3840 px/s.
const SAFE_MOVE: f32 = 16.0;
/// Most substeps in one step. This sets the highest speed the simulation allows even with no
/// top speed: 64 x 16 px x 240 steps, about 245,000 px/s.
const MAX_SUBSTEPS: u32 = 64;
/// Roughly how long the arena takes to shrink by a hit's amount.
const SHRINK_EASE_SECONDS: f32 = 0.06;

/// What every bounce off the walls shares within one substep: see `Simulation::wall_frame`.
#[derive(Clone, Copy)]
pub(crate) struct WallFrame {
    closed: Option<Outline>,
    pivot: Vec2,
    thickness: f32,
    growth: f32,
    restitution: f32,
    /// A custom-shape arena's shape index and placement.
    custom: Option<(usize, Placement)>,
}

/// An unbroken escape ring during one step.
pub(crate) struct RingNow {
    index: u32,
    outline: Outline,
    /// Radians per second it turns.
    spin: f32,
    /// Rotation and scale it went through in the last step.
    moved: Vec2,
    /// Its wall pieces in `Simulation::pieces`.
    pieces: std::ops::Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ball {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    /// Linear-light RGBA.
    pub color: [f32; 4],
    pub wall_hits: u32,
    /// Which spawner made the ball, by its place in the scene's list, so it is drawn with that
    /// spawner's style. Balls split off another ball keep their parent's.
    pub group: u16,
    /// False once a rule has removed the ball. Removed balls keep their place in the list, so
    /// ball numbers in the event log stay valid, but they don't move, collide or show.
    pub alive: bool,
}

/// Things from outside the scene that the simulation follows: the song's beats and levels.
/// Physics may follow the song, never the bounce sounds it makes.
#[derive(Clone, Debug, Default)]
pub struct SimInputs {
    /// Beat times in seconds, earliest first. Each becomes a beat event.
    pub beats: Vec<f64>,
    pub music: Option<Arc<AudioFeatures>>,
}

impl SimInputs {
    /// Inputs that follow a song: its beats and levels.
    pub fn from_music(music: Arc<AudioFeatures>) -> Self {
        Self { beats: music.beats.clone(), music: Some(music) }
    }
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
    /// How many of `balls` have been removed; they stay in the list so indices hold.
    removed: usize,
    step: u64,
    dt: f32,
    arena: ArenaState,
    /// Starting arena size that shrinking is measured against.
    arena_size: f32,
    /// One entry per ring of a `Container::Rings` arena, set once it breaks.
    breaks: Vec<Option<RingBreak>>,
    obstacles: Vec<ObstaclePart>,
    /// Finds the pegs and wall pieces near a ball.
    obstacle_grid: ObstacleGrid,
    /// Scratch space for obstacle lookups.
    nearby: Vec<u32>,
    /// A substep's wall hits before the bounce rules run, kept to reuse.
    found: Vec<Event>,
    /// Balls that appear later, as (step, ball), latest first.
    pending: Vec<(u64, Ball)>,
    /// Arena size still to shrink away from hits, eased in over a few steps.
    shrink_owed: f32,
    /// How far shrinking has taken the arena, as a fraction of its size. Its animated size
    /// multiplies this.
    shrunk: f32,
    /// Whether keyframes change how balls move, so settings are looked up every step.
    animated: bool,
    /// Degrees the arena has turned, added up step by step when its spin is animated.
    turned: f64,
    /// Scratch space for wall pieces and positions before the step.
    pieces: Vec<Piece>,
    previous: Vec<Vec2>,
    /// Ball positions sampled every `trail_every` steps, `trail_len` per ball, as a ring.
    trail: Vec<Vec2>,
    trail_len: usize,
    trail_every: u64,
    trail_head: usize,
    trail_filled: usize,
    inputs: SimInputs,
    /// The next beat in `inputs.beats` to announce.
    next_beat: usize,
    /// Beat sync's state (the controller stage).
    controller: beat_sync::Controller,
    /// Ball-to-ball contact state.
    broadphase: broadphase::Broadphase,
    /// Jelly balls, ropes, cloth and small fluid.
    bodies: bodies::BodyState,
    /// Compiled rules and their counters.
    rules: rules::RuleState,
    /// One entry per scene shape that the arena or an obstacle uses, by shape index.
    shape_walls: Vec<Option<ShapeWalls>>,
    /// How a custom-shape arena moved since the last step, as a turn and scale to apply to
    /// old positions. Nothing after a step's first substep, as walls move once per step.
    custom_moved: Vec2,
    /// The scene shape a custom-shape arena uses, by index.
    custom_shape: Option<usize>,
}

impl Simulation {
    /// A simulation of `scene` with no song.
    pub fn new(scene: &Scene) -> Self {
        Self::with_inputs(scene, &SimInputs::default())
    }

    pub fn with_inputs(scene: &Scene, inputs: &SimInputs) -> Self {
        let steps_per_second = scene.physics.steps_per_second.max(1);
        let mut pending = spawn_plan(scene, steps_per_second);
        let split = pending.partition_point(|(step, _)| *step == 0);
        let balls: Vec<Ball> = pending.drain(..split).map(|(_, ball)| ball).collect();
        pending.reverse();
        let trail_every = (steps_per_second / TRAIL_SAMPLE_RATE).max(1) as u64;
        let sample_rate = steps_per_second as f32 / trail_every as f32;
        // Long enough for the scene's trails and for every ball style's own.
        let longest = scene.spawners.iter().filter_map(|s| s.style()).map(|s| s.trail_seconds);
        let longest = longest.fold(scene.look.trail_seconds, f32::max);
        let trail_len = ((longest.max(0.0) * sample_rate).ceil() as usize + 1).min(MAX_TRAIL_SAMPLES);
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
            obstacle_grid: ObstacleGrid::default(),
            nearby: Vec::new(),
            found: Vec::new(),
            pending,
            shrink_owed: 0.0,
            shrunk: 1.0,
            animated: scene.animates_simulation(),
            turned: 0.0,
            pieces: Vec::new(),
            previous: Vec::new(),
            trail_len,
            trail_every,
            trail_head: 0,
            trail_filled: 0,
            next_beat: inputs.beats.partition_point(|&b| b < 0.0),
            inputs: inputs.clone(),
            controller: beat_sync::Controller::default(),
            broadphase: broadphase::Broadphase::default(),
            bodies: bodies::BodyState::new(scene),
            rules: rules::RuleState::new(scene),
            removed: 0,
            shape_walls: shape_walls(scene),
            custom_moved: Vec2::X,
            custom_shape: match &scene.container {
                Container::Custom { shape, .. } => scene.shape_index(shape).map(|i| i as usize),
                _ => None,
            },
        };
        sim.controller = beat_sync::Controller::new(scene, inputs);
        let shapes = &scene.shapes;
        sim.obstacle_grid = ObstacleGrid::new(&sim.obstacles, |s| shapes.get(s as usize).map_or(1.0, |s| s.reach()));
        sim.update_arena();
        sim.record_trail();
        sim
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Every ball so far, including removed ones (see [`Ball::alive`]).
    pub fn balls(&self) -> &[Ball] {
        &self.balls
    }

    /// The song and beats this simulation follows.
    pub fn inputs(&self) -> &SimInputs {
        &self.inputs
    }

    /// Runs the scene's rules on this step's events from `first_event` on, acting on `balls`.
    /// The flat simulation does this itself at the end of [`Simulation::step`]; 3D mode calls
    /// it after its own balls have stepped.
    pub fn run_rules(&mut self, first_event: usize, events: &mut Vec<Event>, balls: &mut dyn rules::RuleBalls) {
        if self.rules.is_empty() {
            return;
        }
        let now = self.time();
        rules::run(&mut self.rules, now, first_event, events, balls);
    }

    /// Rule state: compiled rules, counters and how often each block fired.
    pub fn rules(&self) -> &rules::RuleState {
        &self.rules
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
        match closed_outline(&self.scene.container, self.arena) {
            Some(outline) => vec![(None, outline)],
            None => self.ring_outlines().into_iter().map(|(k, o)| (Some(k), o)).collect(),
        }
    }

    /// Each unbroken escape ring as it is now, with its index.
    fn ring_outlines(&self) -> Vec<(u32, Outline)> {
        let (container, time) = (&self.scene.container, self.time());
        (0..self.breaks.len() as u32)
            .filter(|&k| self.breaks[k as usize].is_none())
            .filter_map(|k| ring_outline(container, k, time, self.arena).map(|o| (k, o)))
            .collect()
    }

    /// Roughly how many bytes this simulation takes up.
    fn memory(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.balls.capacity() * std::mem::size_of::<Ball>()
            + self.trail.capacity() * std::mem::size_of::<Vec2>()
            + self.previous.capacity() * std::mem::size_of::<Vec2>()
            + self.pending.capacity() * std::mem::size_of::<(u64, Ball)>()
    }

    /// Seconds between trail samples.
    pub fn trail_interval(&self) -> f32 {
        self.dt * self.trail_every as f32
    }

    /// How many of a ball's trail samples reach `seconds` back, for drawing trails of that length.
    pub fn trail_samples(&self, seconds: f32) -> usize {
        let sample_rate = 1.0 / self.trail_interval();
        ((seconds.max(0.0) * sample_rate).ceil() as usize + 1).min(MAX_TRAIL_SAMPLES).min(self.trail_len)
    }

    /// Past positions of one ball, newest first, ending at most as long ago as the longest trail
    /// the scene draws (see [`Simulation::trail_samples`]).
    /// The ball's current position is not included.
    pub fn trail(&self, ball: usize) -> impl Iterator<Item = Vec2> + '_ {
        let base = ball * self.trail_len;
        (0..self.trail_filled).map(move |k| {
            let slot = (self.trail_head + self.trail_len - k) % self.trail_len;
            self.trail[base + slot]
        })
    }

    /// Runs whole steps until the simulation reaches `time`, appending what happens to `events`.
    pub fn advance_to(&mut self, time: f64, events: &mut Vec<Event>) {
        let target = (time * self.scene.physics.steps_per_second as f64).floor() as u64;
        while self.step < target {
            self.step(events);
        }
    }

    /// One fixed step. The stages always run in this order: balls due now appear, beat sync
    /// steers, the arena moves, then each substep moves balls, collides them with each other,
    /// with walls and pegs, and steps soft bodies; then rules react to the step's events, and
    /// trails are recorded.
    pub fn step(&mut self, events: &mut Vec<Event>) {
        let first_event = events.len();
        let dt = self.dt;
        let now = self.time();
        while self.pending.last().is_some_and(|(step, _)| *step <= self.step) {
            let (_, ball) = self.pending.pop().unwrap();
            self.spawn(ball, now, events);
        }
        beat_sync::Controller::before_step(self);
        let ceiling = self.speed_ceiling();
        for ball in &mut self.balls {
            ball.velocity = ball.velocity.clamp_length_max(ceiling);
        }
        let substeps = self.substeps();
        self.step += 1;
        self.update_arena();
        let container = &self.scene.container;
        let closed = closed_outline(container, self.arena);
        let growth = self.arena.scale_rate / self.arena.scale.max(1e-3);
        let mut rings = Vec::new();
        self.pieces.clear();
        if closed.is_none() {
            for (k, outline) in self.ring_outlines() {
                let spin = arena::ring_spin(container, k, self.arena);
                // How the ring moved since the last step, to see the ball's last position from
                // where the ring is now. Otherwise a ring that swept past a ball would make it
                // look as if the ball came from the other side.
                let moved = Vec2::from_angle(spin * dt) * (1.0 + growth * dt);
                let start = self.pieces.len();
                outline_pieces(&outline, &mut self.pieces);
                rings.push(RingNow { index: k, outline, spin, moved, pieces: start..self.pieces.len() });
            }
        }
        let rate = self.scene.physics.steps_per_second.max(1) as f64;
        // Custom shapes (marked: physics workstream): the arena shape moves like the rings.
        self.custom_moved = Vec2::from_angle(self.arena.spin * dt) * (1.0 + growth * dt);
        for i in 0..substeps {
            // The walls only move once per step, so after the first substep they stay put.
            if i == 1 {
                rings.iter_mut().for_each(|ring| ring.moved = Vec2::X);
                self.custom_moved = Vec2::X;
            }
            let time = ((self.step - 1) as f64 + (i + 1) as f64 / substeps as f64) / rate;
            self.announce_beats(time, events);
            self.substep(dt / substeps as f32, time, closed, &rings, events);
        }
        // In 3D the 3D balls step after this, and their side runs the rules once both are done.
        if matches!(self.scene.view, bpe_scene::View::Flat) && !self.rules.is_empty() {
            let mut state = std::mem::take(&mut self.rules);
            let now = self.time();
            rules::run(&mut state, now, first_event, events, &mut rules::FlatBalls(self));
            self.rules = state;
        }
        if self.step.is_multiple_of(self.trail_every) {
            self.record_trail();
        }
    }

    /// The highest speed the simulation allows, in pixels per second, even with no top speed.
    /// Above this a ball would need more than `MAX_SUBSTEPS` substeps a step to stay inside.
    fn speed_ceiling(&self) -> f32 {
        SAFE_MOVE * MAX_SUBSTEPS as f32 * self.scene.physics.steps_per_second.max(1) as f32
    }

    /// How many pieces to split the next step into, so that no ball moves further than
    /// `SAFE_MOVE` at a time and fast balls still bounce off every wall and peg in their way.
    fn substeps(&self) -> u32 {
        let fastest = self.balls.iter().map(|b| b.velocity.length()).fold(0.0f32, f32::max);
        let travel = (fastest + self.scene.physics.gravity.length() * self.dt) * self.dt;
        // Balls that bump into each other with the contact solver mustn't jump past one
        // another either (marked: physics workstream).
        let safe = broadphase::safe_move(self).unwrap_or(SAFE_MOVE);
        // A NaN casts to 0, which the clamp turns into a single step.
        ((travel / safe).ceil() as u32).clamp(1, MAX_SUBSTEPS)
    }

    /// Adds a beat event for each beat up to `until` seconds.
    fn announce_beats(&mut self, until: f64, events: &mut Vec<Event>) {
        while let Some(&beat) = self.inputs.beats.get(self.next_beat) {
            if beat > until {
                break;
            }
            let index = self.next_beat as u32;
            let downbeat = match &self.inputs.music {
                Some(music) if !music.downbeats.is_empty() => {
                    let k = music.downbeats.partition_point(|&d| d < beat - 1e-6);
                    music.downbeats.get(k).is_some_and(|&d| (d - beat).abs() <= 1e-6)
                }
                _ => index.is_multiple_of(4),
            };
            events.push(Event::global(beat, EventKind::Beat { index, downbeat }));
            self.next_beat += 1;
        }
    }

    /// Moves every ball forward by `dt` and bounces it off whatever it hit. `time` is the
    /// clip time at the end of this substep, for the events it records.
    fn substep(&mut self, dt: f32, time: f64, closed: Option<Outline>, rings: &[RingNow], events: &mut Vec<Event>) {
        let physics = &self.scene.physics;
        let drag = (1.0 - physics.air_drag * dt).max(0.0);
        self.previous.clear();
        self.previous.extend(self.balls.iter().map(|b| b.position));
        for (i, ball) in self.balls.iter_mut().enumerate().filter(|(_, b)| b.alive) {
            // Balls at rest in a pile stay exactly where they are until something wakes them
            // (marked: physics workstream).
            if self.broadphase.is_asleep(i) {
                continue;
            }
            ball.velocity = (ball.velocity + physics.gravity * dt) * drag;
            ball.position += ball.velocity * dt;
        }
        broadphase::ball_ball(self, dt, time, closed, events);
        self.collide_walls(time, closed, rings, events);
        bodies::step(self, dt, time, closed, rings, events);
    }

    /// Bounces balls off the arena, rings, pegs and walls, and applies the bounce rules.
    fn collide_walls(&mut self, time: f64, closed: Option<Outline>, rings: &[RingNow], events: &mut Vec<Event>) {
        let restitution = self.scene.physics.restitution;
        let pivot = self.scene.container.center();

        // Touches slower than this are balls resting on something, not bounces. They still push
        // the ball out but don't count as hits, so resting balls don't buzz or flicker.
        let resting = self.scene.physics.gravity.length() * self.dt * 4.0 + 1.0;
        let mut found = std::mem::take(&mut self.found);
        found.clear();
        // Taken out while bouncing, so the bounce helpers can borrow the rest of the simulation.
        let frame = self.wall_frame(closed, restitution);
        let arena_state = self.arena;
        let mut balls = std::mem::take(&mut self.balls);
        for (index, ball) in balls.iter_mut().enumerate() {
            if !ball.alive {
                continue;
            }
            let prev = self.previous[index];
            // The closed arena is checked here, with copies made before the loop, so the
            // compiler can work out a box's or polygon's directions once instead of per ball.
            let closed_hit = closed.as_ref().and_then(|o| arena::collide_closed(ball, o, arena_state, restitution));
            let wall_hit = if frame.custom.is_none() && rings.is_empty() {
                closed_hit
            } else {
                self.other_arena_hit(ball, prev, &frame, rings, closed_hit)
            };
            if let Some((position, speed)) = wall_hit.filter(|(_, speed)| *speed >= resting) {
                found.push(Event { time, ball: index as u32, position, depth: 0.0, speed, kind: EventKind::Wall });
            }
            self.bounce_obstacles(ball, prev, restitution, |part, hit| {
                if let Some((position, speed)) = hit.filter(|(_, speed)| *speed >= resting) {
                    let kind = EventKind::Obstacle(part as u32);
                    found.push(Event { time, ball: index as u32, position, depth: 0.0, speed, kind });
                }
            });
            for ring in rings {
                let (k, outline) = (ring.index as usize, &ring.outline);
                let seen = pivot + ring.moved.rotate(prev - pivot);
                if self.breaks[k].is_none() && arena::inside(outline, seen) && !arena::inside(outline, ball.position) {
                    self.breaks[k] = Some(RingBreak { time, outline: *outline });
                    let speed = ball.velocity.length();
                    let kind = EventKind::Escape(k as u32);
                    let position = ball.position;
                    found.push(Event { time, ball: index as u32, position, depth: 0.0, speed, kind });
                }
            }
        }
        self.balls = balls;

        let mut spawns = Vec::new();
        let center = pivot;
        let ceiling = self.speed_ceiling();
        for &event in &found {
            let ball = &mut self.balls[event.ball as usize];
            let rules = &self.scene.rules;
            let children = match event.kind {
                EventKind::Escape(_) => rules.spawn_per_escape,
                _ => {
                    apply_bounce_rules(ball, &self.scene, ceiling);
                    rules.spawn_per_hit
                }
            };
            // Balls split off at a bounce start where the parent is. Balls made by an escape
            // start back in the middle, so there is always a ball left to escape the next ring.
            let start = if let EventKind::Escape(_) = event.kind { center } else { ball.position };
            if event.kind == EventKind::Wall {
                self.shrink_on_hit();
            }
            spawns.extend((0..children).map(|_| (event.ball, start)));
            events.push(event);
        }
        self.found = found;
        for (parent, start) in spawns {
            if self.alive_balls() >= self.scene.rules.max_balls as usize {
                break;
            }
            let mut child = split_ball(&self.balls[parent as usize], self.balls.len(), self.scene.rules.spawn_spread);
            child.position = start;
            self.spawn(child, time, events);
        }
    }

    /// What every bounce off the walls shares within one substep, worked out once.
    pub(crate) fn wall_frame(&self, closed: Option<Outline>, restitution: f32) -> WallFrame {
        let container = &self.scene.container;
        WallFrame {
            closed,
            pivot: container.center(),
            thickness: container.thickness(),
            growth: self.arena.scale_rate / self.arena.scale.max(1e-3),
            restitution,
            custom: self.custom_arena(),
        }
    }

    /// Bounces something round (a soft body's point, or a probe) off every wall: the arena,
    /// its rings, pegs, lines and shapes. Returns the hardest hit.
    pub(crate) fn bounce_point(
        &mut self,
        ball: &mut Ball,
        prev: Vec2,
        frame: &WallFrame,
        rings: &[RingNow],
    ) -> Option<(Vec2, f32)> {
        let mut best = self.arena_hit(ball, prev, frame, rings);
        self.bounce_obstacles(ball, prev, frame.restitution, |_, hit| {
            if let Some(h) = hit
                && best.is_none_or(|(_, s)| h.1 > s)
            {
                best = Some(h);
            }
        });
        best
    }

    /// Bounces a ball off the arena's own walls (its outline, custom shape and unbroken rings)
    /// and returns the hardest hit. `prev` is turned along with any wall that moved this step,
    /// so a turning wall never looks as if it passed the ball.
    #[inline]
    fn arena_hit(&mut self, ball: &mut Ball, prev: Vec2, frame: &WallFrame, rings: &[RingNow]) -> Option<(Vec2, f32)> {
        // Done here rather than in `other_arena_hit` so it's inlined into the loop over every
        // ball, where the compiler works out the box's directions once rather than per ball.
        let wall_hit =
            frame.closed.as_ref().and_then(|o| arena::collide_closed(ball, o, self.arena, frame.restitution));
        self.other_arena_hit(ball, prev, frame, rings, wall_hit)
    }

    /// The custom-shape arena and rings part of `arena_hit`, given the hit off the closed arena.
    fn other_arena_hit(
        &mut self,
        ball: &mut Ball,
        prev: Vec2,
        frame: &WallFrame,
        rings: &[RingNow],
        mut wall_hit: Option<(Vec2, f32)>,
    ) -> Option<(Vec2, f32)> {
        let WallFrame { pivot, thickness, growth, restitution, custom, .. } = *frame;
        // A custom-shape arena (marked: physics workstream).
        if let Some((shape, place)) = custom
            && let Some(walls) = self.shape_walls[shape].as_mut()
        {
            let seen = pivot + self.custom_moved.rotate(prev - pivot);
            let motion = (pivot, self.arena.spin, growth);
            if let Some(hit) = walls.collide(ball, seen, place, thickness, motion, restitution, true)
                && wall_hit.is_none_or(|(_, s)| hit.1 > s)
            {
                wall_hit = Some(hit);
            }
        }
        for ring in rings {
            // A ring that broke earlier in this step is already gone.
            if self.breaks[ring.index as usize].is_some() {
                continue;
            }
            let seen = pivot + ring.moved.rotate(prev - pivot);
            for piece in &self.pieces[ring.pieces.clone()] {
                if let Some((position, speed)) =
                    arena::collide_piece(ball, seen, piece, thickness, pivot, ring.spin, growth, restitution)
                    && wall_hit.is_none_or(|(_, s)| speed > s)
                {
                    wall_hit = Some((position, speed));
                }
            }
        }
        wall_hit
    }

    /// Bounces a ball off each peg, line and shape near its path, lowest index first, and
    /// calls `hit` with each one's result.
    fn bounce_obstacles(
        &mut self,
        ball: &mut Ball,
        prev: Vec2,
        restitution: f32,
        mut hit: impl FnMut(usize, Option<(Vec2, f32)>),
    ) {
        if self.obstacle_grid.enabled() {
            // Only the pegs and walls near the ball's path, in the same order as checking all
            // of them. Bouncing off one moves the ball, so the rest are looked up again.
            let mut nearby = std::mem::take(&mut self.nearby);
            self.obstacle_grid.query(prev, ball.position, ball.radius, None, &mut nearby);
            let mut next = 0;
            while let Some(&part) = nearby.get(next) {
                let before = ball.position;
                hit(part as usize, self.obstacle_hit(ball, prev, part as usize, restitution));
                next += 1;
                if ball.position != before {
                    self.obstacle_grid.query(prev, ball.position, ball.radius, Some(part), &mut nearby);
                    next = 0;
                }
            }
            self.nearby = nearby;
        } else {
            for part in 0..self.obstacles.len() {
                hit(part, self.obstacle_hit(ball, prev, part, restitution));
            }
        }
    }

    /// Bounces a ball off obstacle part `part`.
    fn obstacle_hit(&mut self, ball: &mut Ball, prev: Vec2, part: usize, restitution: f32) -> Option<(Vec2, f32)> {
        match self.obstacles[part] {
            ObstaclePart::Disc { center, radius, .. } => arena::collide_disc(ball, prev, center, radius, restitution),
            ObstaclePart::Segment { from, to, thickness, .. } => {
                let piece = Piece::Segment { from, to };
                arena::collide_piece(ball, prev, &piece, thickness, Vec2::ZERO, 0.0, 0.0, restitution)
            }
            ObstaclePart::Shape { shape, place, thickness, .. } => {
                let walls = self.shape_walls.get_mut(shape as usize).and_then(Option::as_mut)?;
                walls.collide(ball, prev, place, thickness, (Vec2::ZERO, 0.0, 0.0), restitution, false)
            }
        }
    }

    /// Jelly balls, ropes, cloth and liquid as they are now.
    pub fn soft(&self) -> &bodies::SoftWorld {
        &self.bodies.world
    }

    /// Cuts rope or cloth number `body` at `along` (0 to 1 along it). For rules.
    pub fn cut_soft(&mut self, body: usize, along: f32) -> bool {
        self.bodies.world.cut(body, along)
    }

    /// Pins or frees point `point` of soft body `body`. For rules.
    pub fn pin_soft(&mut self, body: usize, point: usize, pinned: bool) -> bool {
        self.bodies.world.set_pinned(body, point, pinned)
    }

    /// A custom-shape arena right now: its shape index and placement.
    fn custom_arena(&self) -> Option<(usize, Placement)> {
        Some((self.custom_shape?, self.arena_placement()?))
    }

    /// Where a custom-shape arena is right now, after turning and shrinking.
    pub fn arena_placement(&self) -> Option<Placement> {
        match self.scene.container {
            Container::Custom { center, radius, rotation, .. } => Some(Placement {
                center,
                size: radius * self.arena.scale,
                rotation: rotation.to_radians() + self.arena.angle,
            }),
            _ => None,
        }
    }

    /// The flattened outline of scene shape `shape`, in the shape's own units, if the arena or
    /// an obstacle uses it.
    pub fn shape_lines(&self, shape: usize) -> Option<&[bpe_scene::shapes::Polyline]> {
        self.shape_walls.get(shape)?.as_ref().map(|w| w.lines())
    }

    /// Angle each ball has turned from spinning, in radians, for drawing its highlight.
    pub fn ball_angle(&self, ball: usize) -> f32 {
        self.broadphase.angle(ball)
    }

    /// Whether a ball is resting asleep in a pile.
    pub fn ball_asleep(&self, ball: usize) -> bool {
        self.broadphase.is_asleep(ball)
    }

    /// Every wall as straight pieces and arcs, as it is right now: the arena, unbroken rings,
    /// lines, custom shapes and (as full circles) pegs. For the graphics card's swarms and
    /// liquid, which bake them into a distance field. Clears `out` first.
    pub fn wall_pieces(&self, out: &mut Vec<Piece>) {
        out.clear();
        if let Some(outline) = closed_outline(&self.scene.container, self.arena) {
            match outline {
                Outline::Ring { center, radius, sides: 0, .. } => {
                    out.push(Piece::Arc { center, radius, start: 0.0, sweep: std::f32::consts::TAU })
                }
                _ => outline_pieces(&Outline::ring_without_gap(outline), out),
            }
        }
        for (_, outline) in self.ring_outlines() {
            outline_pieces(&outline, out);
        }
        if let Some((shape, place)) = self.custom_arena()
            && let Some(walls) = &self.shape_walls[shape]
        {
            out.extend(walls.world_pieces(place).map(|(from, to)| Piece::Segment { from, to }));
        }
        for part in &self.obstacles {
            match *part {
                ObstaclePart::Disc { center, radius, .. } => {
                    out.push(Piece::Arc { center, radius, start: 0.0, sweep: std::f32::consts::TAU })
                }
                ObstaclePart::Segment { from, to, .. } => out.push(Piece::Segment { from, to }),
                ObstaclePart::Shape { shape, place, .. } => {
                    if let Some(Some(walls)) = self.shape_walls.get(shape as usize) {
                        out.extend(walls.world_pieces(place).map(|(from, to)| Piece::Segment { from, to }));
                    }
                }
            }
        }
    }

    /// The straight pieces of a custom-shape arena and of shape obstacles as they are now, as
    /// (from, to, half thickness).
    pub fn shape_pieces(&self, mut f: impl FnMut(Vec2, Vec2, f32)) {
        if let Some((shape, place)) = self.custom_arena()
            && let Some(walls) = &self.shape_walls[shape]
        {
            let half = self.scene.container.thickness() * 0.5;
            walls.world_pieces(place).for_each(|(from, to)| f(from, to, half));
        }
        for part in &self.obstacles {
            if let ObstaclePart::Shape { shape, place, thickness, .. } = *part
                && let Some(Some(walls)) = self.shape_walls.get(shape as usize)
            {
                walls.world_pieces(place).for_each(|(from, to)| f(from, to, thickness * 0.5));
            }
        }
    }

    /// Adds a ball partway through and records its spawn event.
    fn spawn(&mut self, ball: Ball, time: f64, events: &mut Vec<Event>) {
        let index = self.balls.len() as u32;
        let (position, speed) = (ball.position, ball.velocity.length());
        events.push(Event { time, ball: index, position, depth: 0.0, speed, kind: EventKind::Spawn });
        self.add_ball(ball);
    }

    /// Takes a ball out of the scene and records its remove event. Its place in the list stays.
    fn remove(&mut self, index: usize, time: f64, events: &mut Vec<Event>) {
        let Some(ball) = self.balls.get_mut(index).filter(|b| b.alive) else { return };
        ball.alive = false;
        self.removed += 1;
        let position = ball.position;
        ball.velocity = Vec2::ZERO;
        events.push(Event { time, ball: index as u32, position, depth: 0.0, speed: 0.0, kind: EventKind::Remove });
    }

    /// Balls still in play.
    pub fn alive_balls(&self) -> usize {
        self.balls.len() - self.removed
    }

    /// Adds a ball partway through, with a trail that starts where it is.
    fn add_ball(&mut self, ball: Ball) {
        self.trail.extend(std::iter::repeat_n(ball.position, self.trail_len));
        self.balls.push(ball);
    }

    /// Turns and shrinks the arena for the current time, after setting animated settings to
    /// their values now.
    fn update_arena(&mut self) {
        let time = self.time();
        if self.animated {
            // Modulators on settings that move balls follow the song only (WS5).
            let song = self.inputs.music.as_deref().map(|music| move |t: f64| music_levels(music, t));
            let audio = Audio { song: song.as_ref().map(|f| f as Listener), mix: None };
            self.scene.apply_animation_with(time as f32, true, &audio);
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
        let size = motion.size.max(0.01);
        if self.step == 0 {
            self.arena.scale = size;
            return;
        }
        // Shrinking from hits is eased in rather than done in one jump, so walls move smoothly
        // and push balls instead of jumping past them.
        let owed = self.shrink_owed * (self.dt / SHRINK_EASE_SECONDS).min(1.0);
        self.shrink_owed -= owed;
        let change = -motion.shrink_speed / self.arena_size * self.dt - owed;
        let (low, high) = self.scale_limits();
        self.shrunk = (self.shrunk + change).clamp(low, high);
        let scale = self.shrunk * size;
        self.arena.scale_rate = (scale - self.arena.scale) / self.dt;
        self.arena.scale = scale;
    }

    fn shrink_on_hit(&mut self) {
        self.shrink_owed += self.scene.arena_motion.shrink_per_hit / self.arena_size;
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

/// Seconds of simulation between saved snapshots, at first. Snapshots let a seek backwards
/// start from just before the new time instead of from the very start.
const CHECKPOINT_SECONDS: f64 = 0.5;
/// Most memory the snapshots of one playback may use. Past this, every other one is dropped
/// and they are taken half as often.
const CHECKPOINT_BYTES: usize = 192 << 20;

/// A simulation plus every event so far, which can be moved to any point in time.
/// Seeking backwards restarts from the latest snapshot before that time. The simulation is
/// deterministic, so this gives exactly the same result as replaying from the start.
#[derive(Clone)]
pub struct Playback {
    sim: Simulation,
    events: Vec<Event>,
    /// Snapshots taken every `checkpoint_every` steps, oldest first, with how many events there
    /// were at the time.
    checkpoints: Vec<(Simulation, usize)>,
    checkpoint_every: u64,
    /// Running totals after every [`TALLY_STRIDE`] events, so [`Playback::counts_at`] only
    /// adds up the events since the last one.
    tally: Vec<Counts>,
    /// 3D (WS4): a 3D scene runs this, and `sim` (with no balls) only keeps time and beats.
    space: Option<space::SpaceTrack>,
}

const TALLY_STRIDE: usize = 2048;

impl Playback {
    /// A playback of `scene` with no song.
    pub fn new(scene: &Scene) -> Self {
        Self::with_inputs(scene, &SimInputs::default())
    }

    pub fn with_inputs(scene: &Scene, inputs: &SimInputs) -> Self {
        // 3D (WS4)
        let space = scene.space().map(|_| space::SpaceTrack::new(scene));
        let sim = match space {
            Some(_) => Simulation::with_inputs(&space::time_keeper(scene), inputs),
            None => Simulation::with_inputs(scene, inputs),
        };
        let checkpoint_every = (CHECKPOINT_SECONDS * scene.physics.steps_per_second.max(1) as f64).ceil() as u64;
        let checkpoints = vec![(sim.clone(), 0)];
        let start_balls = match &space {
            Some(space) => space.sim.balls().len() as u32,
            None => sim.balls().len() as u32,
        };
        let tally = vec![Counts { balls: start_balls, ..Counts::default() }];
        Self { sim, events: Vec::new(), checkpoints, checkpoint_every: checkpoint_every.max(1), tally, space }
    }

    /// The simulation. For a 3D scene it has no balls; see [`Playback::space`].
    pub fn simulation(&self) -> &Simulation {
        &self.sim
    }

    /// The 3D simulation, for a 3D scene.
    pub fn space(&self) -> Option<&space::SpaceSimulation> {
        self.space.as_ref().map(|space| &space.sim)
    }

    /// Everything that happened up to the current time, oldest first.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The same as [`Playback::events`]. Use [`Event::is_hit`] to pick out hits.
    pub fn hits(&self) -> &[Event] {
        &self.events
    }

    /// Events within the last `seconds`.
    pub fn recent_events(&self, seconds: f64) -> &[Event] {
        let since = self.sim.time() - seconds;
        let start = self.events.partition_point(|h| h.time <= since);
        &self.events[start..]
    }

    /// The same as [`Playback::recent_events`].
    pub fn recent_hits(&self, seconds: f64) -> &[Event] {
        self.recent_events(seconds)
    }

    /// Events from `from` up to (not including) `to` seconds, as far as the playback has run.
    pub fn events_between(&self, from: f64, to: f64) -> &[Event] {
        events::between(&self.events, from, to)
    }

    /// Running totals at `time`, as far as the playback has run.
    pub fn counts_at(&self, time: f64) -> Counts {
        let end = self.events.partition_point(|e| e.time <= time);
        let block = (end / TALLY_STRIDE).min(self.tally.len() - 1);
        let mut counts = self.tally[block].clone();
        self.count(&mut counts, &self.events[block * TALLY_STRIDE..end]);
        counts
    }

    fn count(&self, counts: &mut Counts, events: &[Event]) {
        for event in events {
            counts.add(event);
            if matches!(event.kind, EventKind::Action { .. })
                && let Some((counter, amount)) = self.sim.rules.counter_change(event)
            {
                if counts.counters.len() <= counter {
                    counts.counters.resize(counter + 1, 0.0);
                }
                counts.counters[counter] += amount;
            }
        }
    }

    /// Brings the running totals in line with the events after they grew or were cut back.
    fn update_tally(&mut self) {
        self.tally.truncate(self.events.len() / TALLY_STRIDE + 1);
        while self.tally.len() * TALLY_STRIDE <= self.events.len() {
            let block = self.tally.len();
            let mut counts = self.tally[block - 1].clone();
            self.count(&mut counts, &self.events[(block - 1) * TALLY_STRIDE..block * TALLY_STRIDE]);
            self.tally.push(counts);
        }
    }

    pub fn seek(&mut self, time: f64) {
        self.seek_within(time, None);
    }

    /// Like [`Playback::seek`], but stops after about `budget` of work if it hasn't got there
    /// yet, so the editor can keep drawing while a long stretch is worked out. Returns whether
    /// the playback reached `time`.
    pub fn seek_within(&mut self, time: f64, budget: Option<std::time::Duration>) -> bool {
        let rate = self.sim.scene.physics.steps_per_second.max(1) as f64;
        let target = (time * rate).floor() as u64;
        if target < self.sim.step {
            let index = self.checkpoints.partition_point(|(sim, _)| sim.step <= target).max(1) - 1;
            let (sim, events) = &self.checkpoints[index];
            self.sim = sim.clone();
            self.events.truncate(*events);
            if let Some(space) = &mut self.space {
                space.restore(self.sim.step);
            }
        }
        let started = budget.map(|_| std::time::Instant::now());
        while self.sim.step < target {
            let from = self.events.len();
            self.sim.step(&mut self.events);
            if let Some(space) = &mut self.space {
                space.step(&mut self.events, from);
                // 3D (WS4): rules see both sides' events and act on the 3D balls.
                self.sim.run_rules(from, &mut self.events, &mut space.sim);
            }
            if self.sim.step.is_multiple_of(self.checkpoint_every)
                && self.checkpoints.last().is_some_and(|(sim, _)| sim.step < self.sim.step)
            {
                self.save_checkpoint();
            }
            if let (Some(started), Some(budget)) = (started, budget)
                && self.sim.step.is_multiple_of(8)
                && started.elapsed() >= budget
            {
                self.update_tally();
                return self.sim.step >= target;
            }
        }
        self.update_tally();
        true
    }

    fn save_checkpoint(&mut self) {
        self.checkpoints.push((self.sim.clone(), self.events.len()));
        if let Some(space) = &mut self.space {
            space.save_checkpoint();
        }
        let mut bytes: usize = self.checkpoints.iter().map(|(sim, _)| sim.memory()).sum();
        bytes += self.space.as_ref().map_or(0, |space| space.memory());
        if bytes > CHECKPOINT_BYTES && self.checkpoints.len() > 2 {
            // Keep the first one and every other one after it, and take them half as often.
            self.checkpoint_every *= 2;
            let every = self.checkpoint_every;
            self.checkpoints.retain(|(sim, _)| sim.step.is_multiple_of(every));
            if let Some(space) = &mut self.space {
                space.thin(every);
            }
        }
    }
}

/// Runs a whole scene from the start and returns every event, for building the soundtrack.
pub fn collect_events(scene: &Scene, inputs: &SimInputs) -> Vec<Event> {
    collect_events_until(scene, inputs, scene.video.duration as f64)
}

/// The same as [`collect_events`], running until `until` seconds instead of the video's end
/// (music workstream: beat sync's Warp runs the simulation past or short of the video's length).
pub fn collect_events_until(scene: &Scene, inputs: &SimInputs, until: f64) -> Vec<Event> {
    if scene.space().is_some() {
        // 3D (WS4): both sides step together, as in a playback, without its snapshots.
        let mut keeper = Simulation::with_inputs(&space::time_keeper(scene), inputs);
        let mut space = space::SpaceTrack::new(scene);
        let mut events = Vec::new();
        let target = (until * scene.physics.steps_per_second as f64).floor() as u64;
        while keeper.step < target {
            let from = events.len();
            keeper.step(&mut events);
            space.step(&mut events, from);
            keeper.run_rules(from, &mut events, &mut space.sim);
        }
        return events;
    }
    let mut sim = Simulation::with_inputs(scene, inputs);
    let mut events = Vec::new();
    sim.advance_to(until, &mut events);
    events
}

/// The same as [`collect_events`] with no song.
pub fn collect_wall_hits(scene: &Scene) -> Vec<Event> {
    collect_events(scene, &SimInputs::default())
}

/// Every ball the scene's spawners make, with the step it appears on, earliest first.
fn spawn_plan(scene: &Scene, steps_per_second: u32) -> Vec<(u64, Ball)> {
    let mut plan = Vec::new();
    for (group, spawner) in scene.spawners.iter().enumerate() {
        let group = group.min(u16::MAX as usize) as u16;
        let ball = |position, velocity, radius, color| Ball {
            position,
            velocity,
            radius,
            color,
            wall_hits: 0,
            group,
            alive: true,
        };
        match spawner {
            Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient, .. } => {
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
            Spawner::Single { position, velocity, radius, color, .. } => {
                plan.push((0, ball(*position, *velocity, *radius, color.to_linear())))
            }
            Spawner::Stream { position, velocity, radius, count, interval, jitter, gradient, .. } => {
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
    Ball {
        position: parent.position,
        velocity: Vec2::from_angle(angle).rotate(parent.velocity),
        radius: parent.radius,
        color: rotate_hue(parent.color, 137.5f32.to_radians()),
        wall_hits: 0,
        group: parent.group,
        alive: true,
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

/// A repeatable pseudo-random number from 0 up to 1 (SplitMix64).
pub fn noise(seed: u64) -> f32 {
    let mut x = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// A repeatable pseudo-random number from -1 up to 1.
fn unit_noise(seed: u64) -> f32 {
    noise(seed) * 2.0 - 1.0
}

/// `ceiling` is the simulation's own speed limit, which holds even with no top speed.
fn apply_bounce_rules(ball: &mut Ball, scene: &Scene, ceiling: f32) {
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

/// Collision walls for every scene shape that the arena or an obstacle uses, by shape index.
fn shape_walls(scene: &Scene) -> Vec<Option<ShapeWalls>> {
    let mut walls: Vec<Option<ShapeWalls>> = vec![None; scene.shapes.len()];
    let mut need = |name: &str, size: f32| {
        if let Some(i) = scene.shape_index(name) {
            let i = i as usize;
            if walls[i].is_none() {
                walls[i] = Some(ShapeWalls::new(&scene.shapes[i], size));
            }
        }
    };
    if let Container::Custom { shape, radius, .. } = &scene.container {
        need(shape, *radius);
    }
    for obstacle in &scene.obstacles {
        if let bpe_scene::Obstacle::Shape { shape, scale, .. } = obstacle {
            need(shape, *scale);
        }
    }
    walls
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Ease, Key, Property, Track, Value};
    use broadphase::{collide_balls, resolve_pair};
    use grid::BallGrid;

    #[test]
    fn counts_match_a_plain_sum_across_tally_blocks_and_seeks() {
        let scene = Scene::presets().into_iter().find(|p| p.name == "Plinko").unwrap().scene;
        let mut playback = Playback::new(&scene);
        let end = scene.video.duration as f64;
        playback.seek(end);
        assert!(playback.events().len() > 3 * TALLY_STRIDE, "needs several blocks, has {}", playback.events().len());
        let plain = |playback: &Playback, time: f64| {
            let mut counts = Counts { balls: playback.tally[0].balls, ..Counts::default() };
            playback.events().iter().filter(|e| e.time <= time).for_each(|e| counts.add(e));
            counts
        };
        for time in [0.0, 0.5, end * 0.3, end * 0.7, end] {
            assert_eq!(playback.counts_at(time), plain(&playback, time), "at {time}");
        }
        playback.seek(end * 0.4);
        playback.seek(end * 0.8);
        for time in [1.0, end * 0.4, end * 0.79] {
            assert_eq!(playback.counts_at(time), plain(&playback, time), "after seeking, at {time}");
        }
    }

    #[test]
    fn rolled_scenes_simulate_cleanly() {
        use bpe_scene::Group;
        let started = std::time::Instant::now();
        for preset in Scene::presets() {
            let scene = &preset.scene;
            let mut groups: Vec<Group> = (0..scene.spawners.len()).map(Group::Balls).collect();
            groups.extend([Group::Shape, Group::Movement, Group::World, Group::Bounces, Group::NewBalls]);
            for seed in 0..6 {
                let rolled = scene.variation(&groups, seed, 1.0);
                let mut sim = Simulation::new(&rolled);
                let mut hits = Vec::new();
                sim.advance_to(4.0, &mut hits);
                let most = (rolled.rules.max_balls + rolled.ball_count()) as usize;
                // Removed balls (in pockets) keep their place in the list, so count those in play.
                assert!(sim.alive_balls() <= most, "{} seed {seed}", preset.name);
                for ball in sim.balls() {
                    assert!(ball.position.is_finite() && ball.velocity.is_finite(), "{} seed {seed}", preset.name);
                    // Nothing flies far out of the frame, unless it escaped all the rings.
                    let escapes = matches!(rolled.container, bpe_scene::Container::Rings { .. });
                    assert!(
                        escapes || ball.position.length() < 3000.0,
                        "{} seed {seed}: {}",
                        preset.name,
                        ball.position
                    );
                }
            }
        }
        eprintln!("rolled scenes took {:?}", started.elapsed());
    }

    /// The ball-pair search as it was before the grid: a hash map of cells, visited in sorted order.
    fn collide_balls_reference(balls: &mut [Ball], restitution: f32) {
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
            for &i in &grid[&(cx, cy)] {
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

    #[test]
    fn ball_grid_matches_the_plain_search() {
        // Packed (the lookup table) and spread out (the hash table), small and large.
        for (count, spread, seed) in [(40, 60.0, 1u64), (900, 300.0, 2), (900, 30000.0, 3), (3000, 900.0, 4)] {
            let balls: Vec<Ball> = (0..count)
                .map(|i| Ball {
                    position: Vec2::new(unit_noise(seed * 7919 + i * 2), unit_noise(seed * 7919 + i * 2 + 1)) * spread,
                    velocity: Vec2::new(unit_noise(i * 3 + 9), unit_noise(i * 5 + 4)) * 300.0,
                    radius: 3.0 + 4.0 * noise(i + seed),
                    color: [1.0; 4],
                    wall_hits: 0,
                    group: 0,
                    alive: true,
                })
                .collect();
            let (mut a, mut b) = (balls.clone(), balls);
            collide_balls(&mut a, 0.9, &mut BallGrid::default());
            collide_balls_reference(&mut b, 0.9);
            assert_eq!(a, b, "{count} balls over {spread} px");
        }
    }

    /// Runs `scene` with and without the peg lookup and checks every hit and ball matches.
    fn assert_grid_matches(scene: &Scene, seconds: f64) {
        let mut fast = Simulation::new(scene);
        let mut plain = Simulation::new(scene);
        plain.obstacle_grid = ObstacleGrid::default();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        fast.advance_to(seconds, &mut a);
        plain.advance_to(seconds, &mut b);
        assert_eq!(a, b, "{}", scene.name);
        assert_eq!(fast.balls(), plain.balls(), "{}", scene.name);
    }

    #[test]
    fn peg_lookup_matches_checking_every_peg() {
        let mut scenes = vec![Scene::plinko()];
        let mut fast = Scene::plinko();
        fast.rules.limit_speed = false;
        fast.rules.speed_gain = 1.2;
        scenes.push(fast);
        let mut colliding = Scene::plinko();
        colliding.physics.ball_collisions = false;
        colliding.physics.gravity = Vec2::new(150.0, 900.0);
        scenes.push(colliding);
        // Rolled obstacles and balls, so pegs and lines of all sizes get tried.
        for seed in 0..8 {
            let base = Scene::plinko();
            let mut groups: Vec<bpe_scene::Group> = (0..base.spawners.len()).map(bpe_scene::Group::Balls).collect();
            groups.extend([bpe_scene::Group::Shape, bpe_scene::Group::World, bpe_scene::Group::Bounces]);
            scenes.push(base.variation(&groups, seed, 1.0));
        }
        for scene in &scenes {
            assert_grid_matches(scene, scene.video.duration.min(12.0) as f64);
        }
    }

    #[test]
    fn seeking_back_gives_the_same_as_replaying() {
        for scene in [Scene::plinko(), Scene::multiplying_balls(), Scene::escape_rings()] {
            let mut playback = Playback::new(&scene);
            playback.seek(9.3);
            for time in [2.71, 0.0, 6.02, 0.5, 8.9, 3.333] {
                playback.seek(time);
                let mut fresh = Playback::new(&scene);
                fresh.seek(time);
                assert_eq!(playback.hits(), fresh.hits(), "{} at {time}", scene.name);
                assert_eq!(playback.simulation().balls(), fresh.simulation().balls(), "{} at {time}", scene.name);
                assert_eq!(playback.simulation().time(), fresh.simulation().time());
                let trail: Vec<_> = playback.simulation().trail(0).collect();
                let fresh_trail: Vec<_> = fresh.simulation().trail(0).collect();
                assert_eq!(trail, fresh_trail);
            }
        }
    }

    #[test]
    fn a_seek_with_a_budget_gets_there_in_the_end() {
        let scene = Scene::plinko();
        let mut playback = Playback::new(&scene);
        let mut rounds = 0;
        while !playback.seek_within(20.0, Some(std::time::Duration::from_micros(200))) {
            rounds += 1;
            assert!(playback.simulation().time() < 20.0);
        }
        assert!(rounds > 0, "a tiny budget should take more than one go");
        let mut fresh = Playback::new(&scene);
        fresh.seek(20.0);
        assert_eq!(playback.hits(), fresh.hits());
    }

    #[test]
    fn snapshots_stay_within_their_memory_limit() {
        let mut scene = Scene::default();
        scene.video.duration = 120.0;
        let mut playback = Playback::new(&scene);
        playback.seek(120.0);
        let bytes: usize = playback.checkpoints.iter().map(|(sim, _)| sim.memory()).sum();
        assert!(bytes <= CHECKPOINT_BYTES + playback.sim.memory(), "{bytes} bytes of snapshots");
        assert!(playback.checkpoints.len() > 10);
    }

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
                depth: 0.0,
                depth_velocity: 0.0,
                style: None,
            },
            Spawner::Single {
                position: Vec2::new(50.0, 0.0),
                velocity: Vec2::new(-200.0, 0.0),
                radius: 10.0,
                color: Color::WHITE,
                depth: 0.0,
                depth_velocity: 0.0,
                style: None,
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

    fn escapes(hits: &[Event]) -> Vec<u32> {
        hits.iter().filter_map(|h| if let EventKind::Escape(k) = h.kind { Some(k) } else { None }).collect()
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

    /// Runs an escape-rings scene and checks that every ball stays inside the smallest unbroken
    /// ring: the only way out is through a gap, which breaks the ring.
    fn assert_no_leaks(scene: &Scene, seconds: f32) {
        let mut sim = Simulation::new(scene);
        let mut hits = Vec::new();
        for step in 0..(seconds * 240.0) as usize {
            sim.step(&mut hits);
            let Some((_, outline)) = sim.outlines().into_iter().next() else { break };
            for (i, ball) in sim.balls().iter().enumerate() {
                assert!(arena::inside(&outline, ball.position), "ball {i} leaked at step {step}");
            }
        }
    }

    #[test]
    fn fast_spinning_polygon_rings_never_leak() {
        let mut scene = Scene::escape_and_multiply();
        if let Container::Rings { spin, spin_step, gap, .. } = &mut scene.container {
            *spin = 300.0;
            *spin_step = -40.0;
            *gap = 20.0;
        }
        assert_no_leaks(&scene, 10.0);
    }

    #[test]
    fn spinning_arena_rings_never_leak() {
        let mut scene = Scene::escape_and_multiply();
        scene.arena_motion.spin = 120.0;
        assert_no_leaks(&scene, scene.video.duration);
    }

    #[test]
    fn rings_that_shrink_on_every_hit_never_leak() {
        let mut scene = Scene::escape_rings();
        scene.arena_motion.shrink_per_hit = 5.0;
        scene.arena_motion.min_size = 100.0;
        assert_no_leaks(&scene, scene.video.duration);
    }

    /// Runs a scene and checks after every step that each ball is a real number, inside the
    /// arena (or the smallest unbroken ring) and not inside a peg.
    fn assert_contained(scene: &Scene, seconds: f32) -> Simulation {
        let mut sim = Simulation::new(scene);
        let mut hits = Vec::new();
        for step in 0..(seconds * scene.physics.steps_per_second as f32) as usize {
            sim.step(&mut hits);
            let outline = sim.outlines().into_iter().next().map(|(_, o)| o);
            for (i, ball) in sim.balls().iter().enumerate() {
                assert!(ball.position.is_finite() && ball.velocity.is_finite(), "ball {i} blew up at step {step}");
                if let Some(outline) = &outline {
                    assert!(arena::inside(outline, ball.position), "ball {i} left the arena at step {step}");
                }
                for part in sim.obstacles() {
                    if let ObstaclePart::Disc { center, radius, .. } = *part {
                        assert!(ball.position.distance(center) >= radius, "ball {i} went into a peg at step {step}");
                    }
                }
            }
        }
        assert!(hits.windows(2).all(|w| w[0].time <= w[1].time), "hits out of order");
        sim
    }

    fn fastest(sim: &Simulation) -> f32 {
        sim.balls().iter().map(|b| b.velocity.length()).fold(0.0, f32::max)
    }

    #[test]
    fn no_top_speed_lets_balls_keep_speeding_up() {
        let mut scene = Scene::growing_ball();
        scene.rules.grow = 0.0;
        scene.rules.speed_gain = 1.1;
        let capped = assert_contained(&scene, 10.0);
        assert!(fastest(&capped) <= scene.rules.max_speed + 1.0);
        scene.rules.limit_speed = false;
        let free = assert_contained(&scene, 10.0);
        assert!(fastest(&free) > scene.rules.max_speed * 3.0, "top speed {}", fastest(&free));
    }

    #[test]
    fn every_preset_holds_together_with_no_top_speed() {
        for preset in Scene::presets() {
            let mut scene = preset.scene;
            scene.rules.limit_speed = false;
            scene.rules.speed_gain = scene.rules.speed_gain.max(1.05);
            let sim = assert_contained(&scene, scene.video.duration.min(15.0));
            println!("{}: fastest {:.0} px/s", preset.name, fastest(&sim));
        }
    }

    #[test]
    fn a_ball_far_too_fast_is_held_at_the_ceiling() {
        let mut scene = Scene::growing_ball();
        scene.rules.limit_speed = false;
        scene.rules.speed_gain = 3.0;
        scene.rules.grow = 0.0;
        let Spawner::Single { velocity, .. } = &mut scene.spawners[0] else { panic!() };
        *velocity = Vec2::new(1e30, -3e29);
        let sim = assert_contained(&scene, 5.0);
        let ceiling = sim.speed_ceiling();
        // Gravity can add a little within one step, but never more than that.
        assert!(fastest(&sim) <= ceiling + scene.physics.gravity.length() * sim.dt + 1.0);
    }

    #[test]
    fn fast_balls_still_hit_every_peg_in_their_way() {
        let mut scene = Scene::plinko();
        scene.rules.limit_speed = false;
        scene.rules.speed_gain = 1.2;
        for spawner in &mut scene.spawners {
            if let Spawner::Stream { velocity, radius, .. } = spawner {
                *velocity = Vec2::new(7000.0, 90000.0);
                *radius = 2.0;
            }
        }
        let sim = assert_contained(&scene, 15.0);
        assert!(fastest(&sim) > 20000.0, "fastest {}", fastest(&sim));
    }

    #[test]
    fn fast_balls_never_leak_through_spinning_rings() {
        let mut scene = Scene::escape_and_multiply();
        scene.rules.limit_speed = false;
        scene.rules.speed_gain = 1.08;
        if let Container::Rings { spin, spin_step, gap, .. } = &mut scene.container {
            *spin = 300.0;
            *spin_step = -40.0;
            *gap = 20.0;
        }
        assert_contained(&scene, 15.0);
    }

    #[test]
    fn slow_scenes_take_one_step_at_a_time() {
        let sim = Simulation::new(&Scene::default());
        assert_eq!(sim.substeps(), 1);
    }

    fn keyframes(property: Property, keys: &[(f32, f32)], ease: Ease) -> Track {
        Track { property, keys: keys.iter().map(|&(time, v)| Key { time, value: Value::Number(v), ease }).collect() }
    }

    #[test]
    fn animated_gravity_takes_over_from_its_keyframe() {
        let mut scene = Scene::big_ball();
        let still = scene.clone();
        scene.animation.push(keyframes(Property::Gravity, &[(1.0, 1200.0), (1.5, -800.0)], Ease::Linear));
        let (mut a, mut b) = (Simulation::new(&still), Simulation::new(&scene));
        a.advance_to(1.0, &mut Vec::new());
        b.advance_to(1.0, &mut Vec::new());
        assert_eq!(a.balls(), b.balls());
        a.advance_to(2.0, &mut Vec::new());
        b.advance_to(2.0, &mut Vec::new());
        assert_ne!(a.balls(), b.balls());
        assert_eq!(b.scene().physics.gravity.y, -800.0);
        assert_eq!(collect_wall_hits(&scene), collect_wall_hits(&scene));
    }

    #[test]
    fn animated_arena_size_moves_the_wall_and_keeps_balls_in() {
        let mut scene = Scene::default();
        scene.video.duration = 8.0;
        let size = keyframes(Property::ArenaSize, &[(0.0, 1.0), (3.0, 0.5), (6.0, 1.3)], Ease::Smooth);
        scene.animation.push(size.clone());
        let Container::Circle { center, radius, .. } = scene.container else { unreachable!() };
        let mut sim = Simulation::new(&scene);
        let mut hits = Vec::new();
        for quarter in 1..=32 {
            let time = quarter as f64 * 0.25;
            sim.advance_to(time, &mut hits);
            let expected = size.value_at(sim.time() as f32).and_then(Value::number).unwrap();
            assert!((sim.arena().scale - expected).abs() < 1e-4, "at {time}: {} vs {expected}", sim.arena().scale);
            for ball in sim.balls() {
                let reach = (ball.position - center).length() + ball.radius;
                assert!(reach <= radius * sim.arena().scale + 0.5, "ball outside at {time}");
            }
        }
    }

    #[test]
    fn animated_spin_adds_up() {
        let mut scene = Scene::spinning_hexagon();
        scene.arena_motion.spin = 0.0;
        scene.animation.push(keyframes(Property::ArenaSpin, &[(0.0, 0.0), (2.0, 90.0)], Ease::Linear));
        let mut sim = Simulation::new(&scene);
        sim.advance_to(2.0, &mut Vec::new());
        // Speeding up steadily from 0 to 90°/s over 2 s turns it 90°.
        let degrees = sim.arena().angle.to_degrees();
        assert!((degrees - 90.0).abs() < 0.5, "turned {degrees}");
        assert_no_leaks(&scene, 6.0);
    }

    #[test]
    fn rings_that_pulse_in_size_never_leak() {
        let mut scene = Scene::escape_rings();
        scene.animation.push(keyframes(
            Property::ArenaSize,
            &[(0.0, 1.0), (1.0, 0.7), (2.0, 1.1), (3.0, 0.8)],
            Ease::Smooth,
        ));
        assert_no_leaks(&scene, 6.0);
    }
}
