//! Feeds: what a Particles thing's particles did in one particle step that a Particles thing
//! inside it (fed by its parent's particles) is born from: where they died, landed, hit something,
//! popped or stopped rising. A thing keeps only the kinds its fed children ask for, and only for
//! the step it just took; its children step straight after it, in tree order.

use glam::Vec2;

use super::FedWhen;
use super::state::Speck;

/// What a particle did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedKind {
    /// Its life ran out (or it faded away after resting).
    Die,
    /// It came to rest on a shape or the floor.
    Land,
    HitBall,
    HitShape,
    Pop,
    /// Going up, it stopped rising.
    StopRising,
}

impl FeedKind {
    /// Its bit in a set of kinds.
    pub fn bit(self) -> u32 {
        1 << self as u32
    }
}

impl FedWhen {
    /// The moment kinds this is born from (none for Every step, which reads the particles).
    pub fn wants(self) -> u32 {
        use FeedKind::*;
        match self {
            FedWhen::Die => Die.bit(),
            FedWhen::Land => Land.bit(),
            FedWhen::HitABall => HitBall.bit(),
            FedWhen::HitAShape => HitShape.bit(),
            FedWhen::HitAnything => HitBall.bit() | HitShape.bit(),
            FedWhen::Pop => Pop.bit(),
            FedWhen::StopRising => StopRising.bit(),
            FedWhen::EveryStep => 0,
        }
    }
}

/// One moment of one particle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feed {
    pub kind: FeedKind,
    pub place: Vec2,
    /// How fast the particle was going (before what it met changed it).
    pub velocity: Vec2,
    /// The way out of what it met (away from the surface), zero when there is none.
    pub normal: Vec2,
    /// The particle's own dice.
    pub seed: u32,
    /// How far through its life it was, 0 to 1.
    pub life: f32,
    /// The ball it came from (body row + 1), 0 when none.
    pub ball: u32,
}

impl Feed {
    /// A moment of `kind` for particle `s` at particle step `now`.
    pub fn of(kind: FeedKind, s: &Speck, velocity: Vec2, normal: Vec2, now: u32) -> Feed {
        Feed { kind, place: s.place, velocity, normal, seed: s.seed, life: s.life_at(now), ball: s.ball }
    }
}

/// How fast a particle must come in for meeting a shape to be a moment (so one resting on a wall
/// doesn't land again every step), in pixels a second.
pub const LAND_SPEED: f32 = 20.0;

/// The moments one chunk of the move keeps, when `wants` asks for them.
#[derive(Clone, Debug, Default)]
pub struct Feeds {
    pub wants: u32,
    pub list: Vec<Feed>,
    /// The particle step being taken.
    pub now: u32,
}

impl Feeds {
    pub fn new(wants: u32, now: u32) -> Feeds {
        Feeds { wants, list: Vec::new(), now }
    }

    /// Keeps a moment of `kind` for `s` when it is wanted.
    pub fn note(&mut self, kind: FeedKind, s: &Speck, velocity: Vec2, normal: Vec2) {
        if self.wants & kind.bit() != 0 {
            self.list.push(Feed::of(kind, s, velocity, normal, self.now));
        }
    }
}
