//! What a Particles thing keeps between steps: every living particle, in birth order, and the
//! counters its births run on. It lives in `World.states`, so checkpoints and copies of the world
//! carry it and every replay is the same.

use std::collections::BTreeMap;
use std::sync::Arc;

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
    /// [`DEAD`], [`RESTING`], [`STUCK`], [`LOOSE`], [`LANDED`].
    pub flags: u32,
    /// The place of a word, picture or grid it was born at (its index + 1), 0 when none: its
    /// home follows the place as the source moves and turns.
    pub spot: u32,
    /// What it met this step ([`super::moments::MET_BALL`], `MET_SHAPE`, `MET_LAND`), turned into
    /// moments after the move.
    pub met: u8,
    /// The id of the ball's and of the shape's thing it met first this step (0 none), at
    /// [`super::moments::MET_AT_BALL`] and `MET_AT_SHAPE`.
    pub with: [u32; 2],
    /// How fast it went into each, px/s.
    pub impact: [f32; 2],
    /// The particle step it came to rest on (stuck to a ball or settled on a shape), 0 while it
    /// moves. Its age stands still while it rests.
    pub rest: u32,
    /// The particle step its resting fade began on, 0 when it isn't fading.
    pub fade_from: u32,
    /// What it rests on: the ball's thing or the shape (its id), 0 for the floor.
    pub on: u32,
    /// The row of the ball it is stuck to.
    pub on_row: u32,
    /// Where on the ball it is stuck, in the ball's radii from its middle.
    pub offset: Vec2,
    /// The seed of the parent particle it was born from (a fed thing's), else its own.
    pub kin_seed: u32,
    /// How far through its life the parent particle was then, 0 to 1.
    pub kin_life: f32,
}

/// The particle is to be removed at the end of the step.
pub const DEAD: u32 = 1;
/// It rests: stuck to a ball or settled on a shape (no force moves it).
pub const RESTING: u32 = 2;
/// It rests stuck to a ball (else settled on a shape or the floor).
pub const STUCK: u32 = 4;
/// Just shaken off a ball: it doesn't stick to one again until it is clear of it.
pub const LOOSE: u32 = 8;
/// The particle has landed on a shape: it lands only once (a moment for its sound and rules).
pub const LANDED: u32 = 16;

impl Speck {
    /// A particle at `place`, still, born on particle step `born`, with `seed` and the rest zero.
    pub fn at(place: Vec2, born: u32, seed: u32) -> Speck {
        Speck {
            place,
            previous: place,
            velocity: Vec2::ZERO,
            home: place,
            born,
            life: 1.0,
            size: 1.0,
            turn: 0.0,
            spin: 0.0,
            flip: 0.0,
            seed,
            ball: 0,
            flags: 0,
            spot: 0,
            met: 0,
            with: [0; 2],
            impact: [0.0; 2],
            rest: 0,
            fade_from: 0,
            on: 0,
            on_row: 0,
            offset: Vec2::ZERO,
            kin_seed: seed,
            kin_life: 0.0,
        }
    }

    /// Whether it rests (stuck or settled).
    pub fn resting(&self) -> bool {
        self.flags & RESTING != 0
    }

    /// How old it is at particle step `now`, in seconds: standing still while it rests.
    pub fn age(&self, now: u32) -> f32 {
        let until = if self.resting() && self.rest > 0 { self.rest.min(now) } else { now };
        until.saturating_sub(self.born) as f32 * super::step::DT
    }

    /// How much of it shows through its resting fade at particle step `now`, 0 to 1.
    pub fn rest_fade(&self, now: u32) -> f32 {
        if self.fade_from == 0 {
            return 1.0;
        }
        let gone = now.saturating_sub(self.fade_from) as f32 * super::step::DT;
        (1.0 - gone / super::step::SETTLE_FADE).clamp(0.0, 1.0)
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
    /// Particle steps taken: the world's time × 120, whatever the Physics rate.
    pub step: u32,
    /// The world step the last particle step was taken after (hits since it let particles off).
    pub world_step: u64,
    /// Births tried so far, every one counted (those dropped at the cap too).
    pub births: u64,
    /// Steady's share of a particle not yet born.
    pub carry: f64,
    /// The particle step the next burst falls on.
    pub next_burst: u32,
    /// How long the last particle step took, in milliseconds (the editor's status line; never
    /// read by the step).
    pub last_ms: f32,
    /// What its Look asks the stepping to keep (part 6).
    pub look_wants: super::wants::Wants,
    /// Each particle's last places, for Trails layers (part 6).
    pub trail: super::trail::TrailRing,
    /// The marks the particles left, oldest first, for Marks layers (part 6).
    pub marks: Vec<super::marks::Mark>,
    /// A word's, picture's or grid's places, in the thing's own frame (`spots.rs`).
    pub spots: Arc<Vec<Vec2>>,
    /// What `spots` were worked out from.
    pub spots_key: super::spots::SpotsKey,
    /// How far Homes turn has turned the homes so far, in radians.
    pub home_turn: f32,
    /// What its particles did in world step [`ParticlesState::moments_step`], at most
    /// [`super::moments::MOST_MOMENTS`] of them (particles part 7).
    pub moments: Vec<super::moments::Moment>,
    /// How many of each kind there were, every one counted.
    pub moment_counts: [u32; super::moments::MOMENT_KINDS],
    /// The world step the moments are from.
    pub moments_step: u64,
    /// Its Sound card's moments, when its sound plays some.
    pub sound: Option<super::moments::MomentSound>,
    /// How many notes its sound may log now (Most notes a second).
    pub note_budget: f32,
    /// What its particles did this particle step that its fed children are born from.
    pub feeds: Vec<super::feeds::Feed>,
    /// The feeds its fed children ask for ([`super::feeds::FeedKind::bit`]s).
    pub wants: u32,
    /// Its piles of particles on shapes and the floor.
    pub piles: super::piles::Piles,
    /// What it does to the balls (two-way), worked out at the last particle step.
    pub two_way: super::two_way::TwoWayOut,
    /// In a Huge swarm (particles part 8), the particles on the graphics card (and `specks` stays
    /// empty).
    pub swarm: Option<super::swarm::Swarm>,
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
            world_step: 0,
            births: 0,
            carry: 0.0,
            next_burst: 0,
            last_ms: 0.0,
            look_wants: Default::default(),
            trail: Default::default(),
            marks: Vec::new(),
            feeds: Vec::new(),
            wants: 0,
            piles: super::piles::Piles::default(),
            two_way: super::two_way::TwoWayOut::default(),
            spots: Arc::new(Vec::new()),
            spots_key: super::spots::SpotsKey::None,
            home_turn: 0.0,
            moments: Vec::new(),
            moment_counts: [0; super::moments::MOMENT_KINDS],
            // The first births, made when the world is built, are world step 1's.
            moments_step: 1,
            sound: None,
            note_budget: 1.0,
            swarm: None,
        }
    }

    /// How many particles it has: in a swarm, as the graphics card last said.
    pub fn count(&self) -> usize {
        match &self.swarm {
            Some(s) => s.count_hint() as usize,
            None => self.specks.len(),
        }
    }

    /// Reads the places again when what they depend on changed (`video` is the video's size).
    pub fn refresh_spots(&mut self, video: Vec2) {
        let mut key = super::spots::key_of(&self.settings, video);
        // The picture as it was when the run began: a picture changed on disk partway through
        // doesn't move the places under the particles (a rebuild reads it afresh).
        if let (
            super::spots::SpotsKey::Picture { path, modified, .. },
            super::spots::SpotsKey::Picture { path: was, modified: then, .. },
        ) = (&mut key, &self.spots_key)
            && path == was
        {
            *modified = *then;
        }
        if key != self.spots_key {
            self.spots = super::spots::spots_for(&key);
            self.spots_key = key;
        }
    }

    /// Bytes its particles take, for playback budgets.
    pub fn memory(&self) -> usize {
        self.specks.capacity() * std::mem::size_of::<Speck>()
            + self.trail.memory()
            + self.marks.capacity() * std::mem::size_of::<super::marks::Mark>()
            + self.moments.capacity() * std::mem::size_of::<super::moments::Moment>()
            + self.feeds.capacity() * std::mem::size_of::<super::feeds::Feed>()
            + self.piles.memory()
    }

    /// Where the homes are now: their middle and turn (radians), from the thing's place `anchor`
    /// and turn `angle` (the camera's middle for a grid covering the screen).
    pub fn home_frame(&self, anchor: Vec2, angle: f32, camera: Vec2) -> HomeFrame {
        let s = &self.settings;
        let screen = s.from == super::BornFrom::AGrid && s.grid_covers == super::GridCovers::TheScreen;
        let (centre, angle) = if screen { (camera, 0.0) } else { (anchor, angle) };
        let turn = angle + s.line_turn.to_radians() + self.home_turn;
        HomeFrame {
            centre,
            way: Vec2::from_angle(if turn.is_finite() { turn } else { 0.0 }),
            spots: self.spots.clone(),
        }
    }
}

/// Where a word's, picture's or grid's places are at a moment.
#[derive(Clone, Debug)]
pub struct HomeFrame {
    pub centre: Vec2,
    /// The turn, as the way the x axis points.
    pub way: Vec2,
    pub spots: Arc<Vec<Vec2>>,
}

impl Default for HomeFrame {
    fn default() -> Self {
        HomeFrame { centre: Vec2::ZERO, way: Vec2::X, spots: Arc::new(Vec::new()) }
    }
}

impl HomeFrame {
    /// Place `spot` (index + 1) in the world; `None` for 0 or a place that is gone.
    pub fn at(&self, spot: u32) -> Option<Vec2> {
        let p = *self.spots.get((spot as usize).checked_sub(1)?)?;
        Some(self.centre + self.way.rotate(p))
    }

    /// Where particle `s` belongs: its place's home, else where it was born.
    pub fn home_of(&self, s: &Speck) -> Vec2 {
        self.at(s.spot).unwrap_or(s.home)
    }
}
