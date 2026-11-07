//! What a Particles thing's particles did this step (particles part 7): hit a ball, hit a shape,
//! landed, were born in a burst, or a burst went off. Moments are kept on the thing's state for
//! the world step they happen in (never logged: rain would flood the log). Its sound plays them
//! (the strongest, at most Most notes a second, logged as [`EventKind::Moment`] events) and the
//! rules' When Particles block counts them.

use bpe_core::{ResolvedThing, Settings, SlotId, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party};
use glam::Vec2;

use super::state::{LANDED, ParticlesState, Speck};
use crate::sound::{MomentStrength, VOICE, VoiceSettings};

/// The most moments kept for one world step; the rest are only counted.
pub const MOST_MOMENTS: usize = 4096;
/// How many kinds of moment there are.
pub const MOMENT_KINDS: usize = 5;
/// A hit at this speed (px/s) is full strength, as the shapes' sound has it.
pub const STRONG: f32 = 1500.0;
/// The half size (px) that is full strength when the strength follows size.
pub const BIG: f32 = 6.0;

/// The particle met a ball this step (bounced off one or vanished at it).
pub const MET_BALL: u8 = 1;
/// It met a shape (bounced off one or vanished at it).
pub const MET_SHAPE: u8 = 2;
/// It stopped against a shape for the first time.
pub const MET_LAND: u8 = 4;
/// Where a [`Speck`]'s `with` and `impact` keep the ball it met this step.
pub const MET_AT_BALL: usize = 0;
/// Where they keep the shape it met (bounced off, vanished at or landed on).
pub const MET_AT_SHAPE: usize = 1;

/// What a moment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MomentKind {
    /// A particle bounced off a ball or vanished at one.
    HitBall,
    /// A particle bounced off a shape or vanished at one.
    HitShape,
    /// A particle stopped on a shape (At shapes: Stop), once a particle.
    Land,
    /// A particle was born in a burst (Bursts, On hits, Once at the start).
    Born,
    /// A burst went off.
    Burst,
}

impl MomentKind {
    pub const ALL: [MomentKind; MOMENT_KINDS] =
        [MomentKind::HitBall, MomentKind::HitShape, MomentKind::Land, MomentKind::Born, MomentKind::Burst];

    /// Its place in [`MomentKind::ALL`], the item of a Moment event's `b`.
    pub fn code(self) -> u32 {
        self as u32
    }

    pub fn of_code(code: u32) -> Option<MomentKind> {
        MomentKind::ALL.get(code as usize).copied()
    }
}

/// One moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub kind: MomentKind,
    /// Where it happened.
    pub place: Vec2,
    /// How the particle was going (a burst: still).
    pub velocity: Vec2,
    /// How fast it went into what it met (px/s); a birth's own speed; a burst's: the speed of the
    /// ball that set it off, or 0.
    pub speed: f32,
    /// The particle's half size (px); a burst's: how many it made.
    pub size: f32,
    /// The ball's or shape's thing it met.
    pub with: Option<ThingId>,
    /// The particle's dice (a burst's: its first particle's, or 0).
    pub seed: u32,
}

impl ParticlesState {
    /// Starts world step `step`'s moments, forgetting the last step's (several particle steps in
    /// one world step add up).
    pub fn start_moments(&mut self, step: u64) {
        if self.moments_step < step {
            self.moments.clear();
            self.moment_counts = [0; MOMENT_KINDS];
            self.moments_step = step;
        }
    }

    /// Notes moment `m`: counted, and kept while there is room.
    pub fn note_moment(&mut self, m: Moment) {
        self.moment_counts[m.kind as usize] += 1;
        if self.moments.len() < MOST_MOMENTS {
            self.moments.push(m);
        }
    }

    /// The moments of world step `step` (none when the last ones are from another step).
    pub fn moments_at(&self, step: u64) -> (&[Moment], [u32; MOMENT_KINDS]) {
        if self.moments_step == step { (&self.moments, self.moment_counts) } else { (&[], [0; MOMENT_KINDS]) }
    }
}

/// Turns what particle `s` met this step into a moment of `st` (marks cleared).
pub fn collect(st: &mut ParticlesState, s: &mut Speck) {
    if s.met == 0 {
        return;
    }
    // Each kind tells its own meeting: a ball's, or a shape's (a landing is on a shape).
    let at = |kind, k: usize| Moment {
        kind,
        place: s.place,
        velocity: s.velocity,
        speed: s.impact[k],
        size: s.size,
        with: (s.with[k] != 0).then_some(ThingId(s.with[k])),
        seed: s.seed,
    };
    let (ball, shape) = (at(MomentKind::HitBall, MET_AT_BALL), at(MomentKind::HitShape, MET_AT_SHAPE));
    let land = at(MomentKind::Land, MET_AT_SHAPE);
    if s.met & MET_LAND != 0 {
        s.flags |= LANDED;
        st.note_moment(land);
    }
    if s.met & MET_SHAPE != 0 {
        st.note_moment(shape);
    }
    if s.met & MET_BALL != 0 {
        st.note_moment(ball);
    }
    s.met = 0;
    s.with = [0; 2];
    s.impact = [0.0; 2];
}

/// What of a Particles thing's Sound card its moments read: which moments play, how many a
/// second at most, and how loud.
#[derive(Clone, Debug, PartialEq)]
pub struct MomentSound {
    /// Which kinds play, by [`MomentKind`] place.
    pub plays: [bool; MOMENT_KINDS],
    /// The most notes a second.
    pub most: f32,
    pub strength: MomentStrength,
}

impl MomentSound {
    /// The thing's moment sound, when its Sound card is on and some moment plays.
    pub fn of(resolved: &ResolvedThing) -> Option<MomentSound> {
        let card = resolved.slots.get(&SlotId::Sound).filter(|c| c.kind == VOICE)?;
        let v = VoiceSettings::from_params(&card.params);
        let plays = [v.moments_hit_ball, v.moments_hit_shape, v.moments_land, v.moments_born, v.moments_burst];
        (v.enabled && plays.contains(&true)).then_some(MomentSound {
            plays,
            most: if v.moments_most.is_finite() { v.moments_most.clamp(0.0, 1e6) } else { 0.0 },
            strength: v.moments_strength,
        })
    }

    /// A hash of it, for the sound's fingerprint.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut put = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        for p in self.plays {
            put(p as u8);
        }
        for b in self.most.to_bits().to_le_bytes() {
            put(b);
        }
        put(self.strength as u8);
        h
    }

    /// How loud moment `m` plays, 0 to 1.
    pub fn strength_of(&self, m: &Moment) -> f32 {
        let speed = if m.speed.is_finite() { (m.speed.max(0.0) / STRONG).min(1.0) } else { 0.0 };
        let size = if m.size.is_finite() { (m.size.max(0.0) / BIG).min(1.0) } else { 0.0 };
        match self.strength {
            MomentStrength::Speed => speed,
            MomentStrength::Size => size,
            MomentStrength::SpeedAndSize => speed * size,
            MomentStrength::TheSame => 1.0,
        }
    }
}

/// Logs the strongest of `st`'s playing moments of world step `step` as [`EventKind::Moment`]
/// events, as many as its note budget allows: the budget grows by Most a second for `seconds`
/// (the world step), at most max(1, Most / 10), and a note takes one whole.
pub fn log_notes(st: &mut ParticlesState, sound: &MomentSound, step: u64, seconds: f32, log: &mut Log) {
    let cap = (sound.most * 0.1).max(1.0);
    st.note_budget = (st.note_budget + sound.most * seconds.max(0.0)).min(cap);
    if !st.note_budget.is_finite() {
        st.note_budget = 0.0;
    }
    if st.note_budget < 1.0 || st.moments_step != step {
        return;
    }
    let mut picks: Vec<(f32, usize)> = st
        .moments
        .iter()
        .enumerate()
        .filter(|(_, m)| sound.plays[m.kind as usize])
        .map(|(i, m)| (sound.strength_of(m), i))
        .collect();
    if picks.is_empty() {
        return;
    }
    let room = (st.note_budget.floor() as usize).min(picks.len());
    // The strongest first; equal ones in the order they happened.
    picks.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    picks.truncate(room);
    picks.sort_by_key(|&(_, i)| i);
    st.note_budget -= room as f32;
    for (strength, i) in picks {
        let m = st.moments[i];
        log.push(Event {
            step,
            kind: EventKind::Moment,
            a: Party { thing: st.id, item: m.seed },
            b: Some(Party { thing: st.id, item: m.kind.code() }),
            point: m.place,
            normal: m.velocity.try_normalize().unwrap_or(Vec2::ZERO),
            speed: strength * STRONG,
            at: 1.0,
        });
    }
}
