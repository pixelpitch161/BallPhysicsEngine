//! Which hits on a shape play a note: hard enough, not too soon after the same ball's last one,
//! and from the balls the shape listens to. A Balls or Emitter thing's sound (W10) hears its own
//! balls instead: what they hit, and their fires, catches and breaks.

use crate::BlockHit;
use bpe_core::{Target, ThingId};
use bpe_kit::sound::VoiceSettings;
use bpe_runtime::{Event, EventKind, World};

/// A hit at this speed (px/s) or more is full strength, unless a sound's Full strength at says
/// otherwise.
pub const STRONG_HIT: f32 = 1500.0;
/// Balls the retrigger guard remembers at once.
const GUARD_SLOTS: usize = 2048;
/// How far a ball's slot may be from where it hashes to.
const PROBE: usize = 8;

/// How hard a hit at `speed` px/s is, 0..1, on the straight line to [`STRONG_HIT`].
pub fn strength(speed: f32) -> f32 {
    StrengthCurve::default().of(speed)
}

/// How a sound turns a hit's speed into its strength: full at `full_at` px/s, bent by `curve`
/// (-100..100, 0 straight; above 0 soft hits count for more).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrengthCurve {
    pub full_at: f32,
    pub curve: f32,
}

impl Default for StrengthCurve {
    fn default() -> Self {
        StrengthCurve { full_at: STRONG_HIT, curve: 0.0 }
    }
}

impl StrengthCurve {
    /// From the voice's Full strength at and Strength curve knobs.
    pub fn of_knobs(full_at: f32, curve: f32) -> Self {
        StrengthCurve { full_at, curve }
    }

    /// How hard a hit at `speed` px/s is, 0..1.
    pub fn of(&self, speed: f32) -> f32 {
        if !speed.is_finite() {
            return 0.0;
        }
        let full = if self.full_at.is_finite() { self.full_at.max(1.0) } else { STRONG_HIT };
        let x = (speed.max(0.0) / full).min(1.0);
        let curve = if self.curve.is_finite() { self.curve / 100.0 } else { 0.0 };
        crate::mods::bend(x, curve)
    }
}

/// Which of its own balls' moments a Balls or Emitter thing's sound plays (W10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BallMoments {
    /// One of its balls hit a ball (its own or another thing's).
    pub hit_ball: bool,
    /// One of its balls hit something without balls: a shape, a Text, a wall.
    pub hit_shape: bool,
    /// The emitter fired one of its balls.
    pub fire: bool,
    /// One of its balls came into a catcher, or (an emitter) a ball came into it.
    pub catch: bool,
    /// A rule broke one of its balls (or the thing).
    pub broken: bool,
}

impl BallMoments {
    pub fn of(v: &VoiceSettings) -> Self {
        BallMoments {
            hit_ball: v.moments_hit_ball,
            hit_shape: v.moments_hit_shape,
            fire: v.moments_fire,
            catch: v.moments_catch,
            broken: v.moments_break,
        }
    }

    /// The moments of thing `thing_kind`'s sound `v`: `None` unless the thing has balls of its own.
    pub fn of_thing(thing_kind: &str, v: &VoiceSettings) -> Option<Self> {
        bpe_kit::sound::hears_own_balls(thing_kind).then(|| Self::of(v))
    }
}

/// A shape's hit filter (or a Balls or Emitter thing's, when `balls` is set).
#[derive(Clone, Debug)]
pub struct HitFilter {
    /// The thing whose sound it is.
    pub shape: ThingId,
    /// Set on a Balls or Emitter thing's sound: it hears its own balls, these moments of them.
    pub balls: Option<BallMoments>,
    /// 0..1: hits softer than this play nothing.
    pub min_strength: f32,
    /// How a hit's speed becomes its strength.
    pub curve: StrengthCurve,
    /// Samples a ball must wait before it plays again.
    pub retrigger: u64,
    pub heard_from: Target,
    /// Each ball's key (`thing << 32 | item`, plus one; 0 for empty) and its last played sample.
    guard: Vec<(u64, u64)>,
}

impl HitFilter {
    /// Allocates its table: build it off the audio path.
    pub fn new(shape: ThingId, min_strength: f32, retrigger_s: f32, heard_from: Target) -> Self {
        let mut f = HitFilter {
            shape,
            balls: None,
            min_strength: 0.0,
            curve: StrengthCurve::default(),
            retrigger: 0,
            heard_from,
            guard: vec![(0, 0); GUARD_SLOTS],
        };
        f.set(min_strength, retrigger_s, StrengthCurve::default());
        f
    }

    /// The filter of a Balls or Emitter thing's sound (`Some`), hearing its own balls' `moments`;
    /// `None` keeps a shape's.
    pub fn hearing(mut self, moments: Option<BallMoments>) -> Self {
        self.balls = moments;
        self
    }

    /// `event` as this sound hears it. A Balls or Emitter thing's sound hears from its own ball:
    /// a hit whose `b` is one of its balls (and whose `a` isn't) comes back turned round, its ball
    /// as `a` and what the ball met as `b`. Anything else comes back as it is.
    pub fn seen(&self, event: &Event) -> Event {
        let mut e = event.clone();
        if self.balls.is_some()
            && e.kind == EventKind::Hit
            && e.a.thing != self.shape
            && let Some(b) = e.b.filter(|b| b.thing == self.shape)
        {
            e.b = Some(e.a);
            e.a = b;
            e.normal = -e.normal;
        }
        e
    }

    /// Whether `e` (as [`HitFilter::seen`] gives it) is one this sound plays, before its strength
    /// and the retrigger guard: on a shape, a hit or a particle moment on it from the balls it
    /// hears; on a Balls or Emitter thing, one of its balls' moments it plays.
    fn hears(&self, e: &Event, world: &World) -> bool {
        let me = self.shape;
        let Some(m) = self.balls else {
            if !e.b.is_some_and(|b| b.thing == me) {
                return false;
            }
            return match e.kind {
                EventKind::Hit => world.names(&self.heard_from, me, e.a.thing),
                // A Particles thing's moments are its own particles': no Heard from (particles
                // part 7). Another thing's moment on it is heard as a hit is.
                EventKind::Moment => e.a.thing == me || world.names(&self.heard_from, me, e.a.thing),
                _ => false,
            };
        };
        match e.kind {
            EventKind::Hit => {
                let Some(other) = e.b.filter(|_| e.a.thing == me) else { return false };
                let is_ball = world.bodies.contains_key(&other.thing);
                (if is_ball { m.hit_ball } else { m.hit_shape }) && world.names(&self.heard_from, me, other.thing)
            }
            EventKind::Spawn => m.fire && e.b.is_some_and(|b| b.thing == me),
            EventKind::Enter => m.catch && (e.a.thing == me || e.b.is_some_and(|b| b.thing == me)),
            EventKind::Break => m.broken && e.a.thing == me,
            _ => false,
        }
    }

    /// New thresholds and strength curve (keyframed knobs move them every block).
    pub fn set(&mut self, min_strength: f32, retrigger_s: f32, curve: StrengthCurve) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
        self.retrigger = (retrigger_s.max(0.0) * crate::SAMPLE_RATE) as u64;
        self.curve = curve;
    }

    /// Forgets every ball (on a seek).
    pub fn reset(&mut self) {
        self.guard.iter_mut().for_each(|g| *g = (0, 0));
    }

    /// Takes over another filter's memory of the balls.
    pub fn take_memory(&mut self, other: &HitFilter) {
        if other.guard.len() == self.guard.len() {
            self.guard.copy_from_slice(&other.guard);
        }
    }

    /// The hit's strength if it plays a note on this shape at sound sample `sample`. A Balls or
    /// Emitter thing's sound takes the event as [`HitFilter::seen`] gives it.
    pub fn accept(&mut self, hit: &BlockHit, sample: u64, world: &World) -> Option<f32> {
        let e = &hit.event;
        if !self.hears(e, world) {
            return None;
        }
        let ball = e.a;
        let s = self.curve.of(e.speed);
        if s < self.min_strength {
            return None;
        }
        let key = ((ball.thing.0 as u64) << 32 | ball.item as u64).wrapping_add(1);
        let home = (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 53) as usize % GUARD_SLOTS;
        let mut free = None;
        for k in 0..PROBE {
            let i = (home + k) % GUARD_SLOTS;
            let (held, last) = self.guard[i];
            if held == key {
                if self.retrigger > 0 && sample < last.saturating_add(self.retrigger) && sample >= last {
                    return None;
                }
                self.guard[i].1 = sample;
                return Some(s);
            }
            if held == 0 && free.is_none() {
                free = Some(i);
            }
        }
        // A new ball: its own slot, or (a crowded table) the home slot.
        self.guard[free.unwrap_or(home)] = (key, sample);
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_curve_is_the_straight_line() {
        for speed in [0.0, 1.0, 300.0, 749.9, 1500.0, 1e6, -5.0, f32::NAN, f32::INFINITY] {
            let straight = if speed.is_finite() { (speed.max(0.0) / STRONG_HIT).min(1.0) } else { 0.0 };
            assert_eq!(StrengthCurve::default().of(speed).to_bits(), straight.to_bits(), "{speed}");
        }
    }

    #[test]
    fn full_at_and_curve_reshape_the_strength() {
        let soon = StrengthCurve::of_knobs(500.0, 0.0);
        assert_eq!(soon.of(250.0), 0.5);
        assert_eq!(soon.of(600.0), 1.0, "full sooner");
        let soft = StrengthCurve::of_knobs(1500.0, 60.0);
        let hard = StrengthCurve::of_knobs(1500.0, -60.0);
        let mid = StrengthCurve::default().of(450.0);
        assert!(soft.of(450.0) > mid && hard.of(450.0) < mid, "a curve bends the middle");
        assert_eq!((soft.of(0.0), soft.of(1500.0)), (0.0, 1.0), "the ends stay put");
    }

    #[test]
    fn a_balls_sound_sees_a_hit_from_its_own_ball() {
        use bpe_runtime::Party;
        let hit = Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(7), item: 3 },
            b: Some(Party { thing: ThingId(2), item: 5 }),
            point: glam::Vec2::ZERO,
            normal: glam::Vec2::Y,
            speed: 100.0,
            at: 0.5,
        };
        let shape = HitFilter::new(ThingId(2), 0.0, 0.0, Target::Anything);
        assert_eq!(shape.seen(&hit), hit, "a shape's sound sees the hit as it is");
        let balls = HitFilter::new(ThingId(2), 0.0, 0.0, Target::Anything).hearing(Some(BallMoments::default()));
        let seen = balls.seen(&hit);
        assert_eq!((seen.a, seen.b), (Party { thing: ThingId(2), item: 5 }, Some(hit.a)), "its own ball first");
        assert_eq!(seen.normal, -hit.normal);
        let mover = HitFilter::new(ThingId(7), 0.0, 0.0, Target::Anything).hearing(Some(BallMoments::default()));
        assert_eq!(mover.seen(&hit), hit, "its ball is already the mover");
        let spawn = Event { kind: EventKind::Spawn, ..hit.clone() };
        assert_eq!(balls.seen(&spawn), spawn, "only hits turn round");
    }
}
