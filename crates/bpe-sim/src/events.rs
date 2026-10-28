//! The event log: everything that happens in a scene, in time order.
//!
//! Sound, flashes, particles, captions, the timeline, rules and the scorer all read this one
//! log. It is append-only while the simulation runs and identical every time the same scene is
//! simulated.

use bpe_scene::Vec2;

/// [`Event::ball`] for events that don't belong to a ball, such as beats.
pub const NO_BALL: u32 = u32::MAX;

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// A ball hit the arena's wall, or one of its rings.
    Wall,
    /// A ball hit a peg or wall piece, by its index in [`bpe_scene::Scene::obstacle_parts`].
    Obstacle(u32),
    /// The ball escaped through the gap of this ring, which shattered.
    Escape(u32),
    /// A ball hit another ball, by the other ball's index.
    Ball(u32),
    /// A jelly ball, rope or cloth touched something.
    Soft { body: u32 },
    /// A ball appeared after the start: dropped by a stream, split off or made by a rule.
    Spawn,
    /// A rule removed the ball.
    Remove,
    /// A beat of the song (or the BPM grid). `index` counts from 0.
    Beat { index: u32, downbeat: bool },
    /// A rule's timer went off.
    Timer(u16),
    /// A rule did something that shows or sounds: a flash, a burst of particles, a note, a
    /// caption. `rule` indexes [`bpe_scene::Scene::logic`]; `action` is the block's id.
    Action { rule: u16, action: u16 },
}

/// One thing that happened, at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    /// Seconds from the start of the clip.
    pub time: f64,
    /// The ball involved, or [`NO_BALL`].
    pub ball: u32,
    /// Where it happened: where the ball touched the wall, or where the ball is.
    pub position: Vec2,
    /// Distance into the screen in 3D, 0 in 2D.
    pub depth: f32,
    /// For hits, the speed into the wall just before the bounce, in pixels per second. For
    /// escapes and spawns, the ball's speed. 0 when it doesn't apply.
    pub speed: f32,
    pub kind: EventKind,
}

/// The old name of [`Event`], kept for one release.
pub type WallHit = Event;
/// The old name of [`EventKind`], kept for one release.
pub type HitKind = EventKind;

impl EventKind {
    /// Something physically touched something: what makes a bounce sound and a flash.
    pub fn is_hit(self) -> bool {
        matches!(self, EventKind::Wall | EventKind::Obstacle(_) | EventKind::Ball(_) | EventKind::Soft { .. })
    }

    /// A hit or an escape: the events the editor has always shown as bounces.
    pub fn is_bounce(self) -> bool {
        self.is_hit() || matches!(self, EventKind::Escape(_))
    }
}

impl Event {
    /// An event with no ball, such as a beat or timer.
    pub fn global(time: f64, kind: EventKind) -> Self {
        Self { time, ball: NO_BALL, position: Vec2::ZERO, depth: 0.0, speed: 0.0, kind }
    }

    pub fn is_hit(&self) -> bool {
        self.kind.is_hit()
    }

    /// The ball's index, when there is one.
    pub fn ball_index(&self) -> Option<usize> {
        (self.ball != NO_BALL).then_some(self.ball as usize)
    }
}

/// Running totals at one moment, for captions and the scorer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counts {
    /// Balls on screen.
    pub balls: u32,
    /// Hits of any kind (walls, pegs, balls, soft bodies).
    pub hits: u32,
    pub wall_hits: u32,
    pub escapes: u32,
    pub spawns: u32,
    pub removes: u32,
    pub beats: u32,
    /// Rule actions taken.
    pub actions: u32,
    /// Each rule counter's value, by counter number.
    pub counters: Vec<f64>,
}

impl Counts {
    /// Adds one event to the totals.
    pub fn add(&mut self, event: &Event) {
        match event.kind {
            EventKind::Wall => {
                self.hits += 1;
                self.wall_hits += 1;
            }
            EventKind::Obstacle(_) | EventKind::Ball(_) | EventKind::Soft { .. } => self.hits += 1,
            EventKind::Escape(_) => self.escapes += 1,
            EventKind::Spawn => {
                self.spawns += 1;
                self.balls += 1;
            }
            EventKind::Remove => {
                self.removes += 1;
                self.balls = self.balls.saturating_sub(1);
            }
            EventKind::Beat { .. } => self.beats += 1,
            EventKind::Timer(_) => {}
            EventKind::Action { .. } => self.actions += 1,
        }
    }
}

/// The events from `from` (inclusive) to `to` (exclusive) in a time-sorted log.
pub fn between(events: &[Event], from: f64, to: f64) -> &[Event] {
    let start = events.partition_point(|e| e.time < from);
    let end = events.partition_point(|e| e.time < to);
    &events[start..end.max(start)]
}
