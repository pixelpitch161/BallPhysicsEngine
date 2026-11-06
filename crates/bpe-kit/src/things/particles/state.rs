//! What a Particles thing keeps between steps: every living particle, in birth order, and the
//! counters its births run on. It lives in `World.states`, so checkpoints and copies of the world
//! carry it and every replay is the same.

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_runtime::Outline;
use glam::Vec2;

use super::ParticlesSettings;
use super::forces::Forces;
use super::meets::MeetsSettings;

/// One particle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speck {
    pub place: Vec2,
    /// Where it was at the step before.
    pub previous: Vec2,
    pub velocity: Vec2,
    /// Where it was born: its home.
    pub home: Vec2,
    /// The particle step it was born on.
    pub born: u32,
    /// How long it lives, in seconds.
    pub life: f32,
    /// Its half size, in pixels.
    pub size: f32,
    /// Its turn, in radians.
    pub turn: f32,
    /// How fast it turns, in radians a second.
    pub spin: f32,
    /// Flutter's phase, in radians: how far it has flipped.
    pub flip: f32,
    /// Its own dice.
    pub seed: u32,
    /// The parent ball it came from (body row + 1), 0 when none.
    pub ball: u32,
    /// [`DEAD`] once it is to go.
    pub flags: u32,
}

/// The particle is to be removed at the end of the step.
pub const DEAD: u32 = 1;

impl Speck {
    /// How old it is at particle step `now`, in seconds.
    pub fn age(&self, now: u32) -> f32 {
        now.saturating_sub(self.born) as f32 * super::step::DT
    }

    /// How far through its life it is at particle step `now`, 0 to 1.
    pub fn life_at(&self, now: u32) -> f32 {
        (self.age(now) / self.life.max(1e-6)).clamp(0.0, 1.0)
    }
}

/// A Particles thing's state.
#[derive(Clone, Debug)]
pub struct ParticlesState {
    pub id: ThingId,
    /// Its tree parent, for riding and for its hits.
    pub parent: Option<ThingId>,
    /// Its Source as it is now (keys move it, `settings_changed`).
    pub settings: ParticlesSettings,
    /// Its Motion card's forces, as they are now.
    pub forces: Forces,
    /// Its Meets things card, as it is now.
    pub meets: MeetsSettings,
    /// Every wall's outline at the last particle step: which side of it a particle is on is
    /// judged against it.
    pub walls_before: BTreeMap<ThingId, Outline>,
    /// The living particles, oldest first.
    pub specks: Vec<Speck>,
    /// Particle steps taken: the world's step over 2.
    pub step: u32,
    /// Births tried so far, every one counted (those dropped at the cap too).
    pub births: u64,
    /// Steady's share of a particle not yet born.
    pub carry: f64,
    /// The particle step the next burst falls on.
    pub next_burst: u32,
    /// How long the last particle step took, in milliseconds (the editor's status line; never
    /// read by the step).
    pub last_ms: f32,
}

impl ParticlesState {
    pub fn new(id: ThingId, parent: Option<ThingId>, settings: ParticlesSettings) -> Self {
        Self {
            id,
            parent,
            settings,
            forces: Forces::default(),
            meets: MeetsSettings::default(),
            walls_before: BTreeMap::new(),
            specks: Vec::new(),
            step: 0,
            births: 0,
            carry: 0.0,
            next_burst: 0,
            last_ms: 0.0,
        }
    }

    /// Bytes its particles take, for playback budgets.
    pub fn memory(&self) -> usize {
        self.specks.capacity() * std::mem::size_of::<Speck>()
    }
}
