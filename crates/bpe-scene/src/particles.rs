//! Hit particles: sparks, smoke, confetti and more that burst out where things happen.
//!
//! Each [`Emitter`] watches the event log for one kind of event and bursts particles there.
//! Particles are only pictures: they never make sound or change how balls move. Their
//! position is worked out from the event and their age alone, so scrubbing the timeline and
//! exporting show exactly the same particles.

use crate::{Color, Dice};
use serde::{Deserialize, Serialize};

/// Most particles one event can burst into.
pub const MAX_BURST: u32 = 400;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Emitter {
    pub enabled: bool,
    /// Which events burst particles.
    pub on: EventFilter,
    pub preset: ParticlePreset,
    /// Particles per event, at full impact speed.
    pub count: u32,
    /// Starting speed in pixels per second.
    pub speed: f32,
    /// How widely particles fan out, in degrees. 360 sends them every way.
    pub spread: f32,
    /// Particle size in pixels.
    pub size: f32,
    /// Seconds each particle lasts.
    pub life: f32,
    /// Pull in pixels per second squared. Positive pulls down.
    pub gravity: f32,
    /// Fraction of speed lost per second to air.
    pub drag: f32,
    pub color: ParticleColor,
    pub blend: Blend,
    /// Gentle hits burst fewer, slower particles than hard ones.
    pub follow_impact: bool,
    /// Particles bounce off walls instead of flying through them.
    pub bounce: bool,
    /// Mixed into each particle's random numbers, so two emitters on the same events differ.
    pub seed: u64,
}

/// Which events an emitter answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EventFilter {
    pub kind: EventMatch,
    /// For [`EventMatch::Action`]: the burst name a rule fires, such as "confetti". Empty
    /// matches every burst.
    pub action: String,
    /// Only events of this ball, by its number in the scene. `None` is every ball.
    pub ball: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventMatch {
    /// Any wall, peg or ball hit.
    AnyHit,
    Wall,
    /// A peg or wall piece inside the arena.
    Obstacle,
    /// A ball escaping through a ring's gap.
    Escape,
    /// Two balls bumping into each other.
    Ball,
    /// A jelly ball, rope or cloth touching something.
    Soft,
    /// A new ball appearing.
    Spawn,
    /// A beat of the song.
    Beat,
    /// A rule's "Burst particles" action.
    Action,
}

impl EventMatch {
    pub const ALL: [EventMatch; 9] = [
        EventMatch::AnyHit,
        EventMatch::Wall,
        EventMatch::Obstacle,
        EventMatch::Escape,
        EventMatch::Ball,
        EventMatch::Soft,
        EventMatch::Spawn,
        EventMatch::Beat,
        EventMatch::Action,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EventMatch::AnyHit => "Every hit",
            EventMatch::Wall => "Wall hits",
            EventMatch::Obstacle => "Peg hits",
            EventMatch::Escape => "Escapes",
            EventMatch::Ball => "Ball bumps",
            EventMatch::Soft => "Jelly touches",
            EventMatch::Spawn => "New balls",
            EventMatch::Beat => "Beats",
            EventMatch::Action => "Rule bursts",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParticlePreset {
    /// Thin bright streaks that fly off and fall.
    Sparks,
    /// Soft puffs that grow and fade.
    Smoke,
    /// Spinning paper squares that flutter down.
    Confetti,
    /// Twinkling four-pointed stars.
    Stars,
    /// Sharp splinters that tumble away.
    Shards,
    /// A circle that grows out from the hit.
    Rings,
}

impl ParticlePreset {
    pub const ALL: [ParticlePreset; 6] = [
        ParticlePreset::Sparks,
        ParticlePreset::Smoke,
        ParticlePreset::Confetti,
        ParticlePreset::Stars,
        ParticlePreset::Shards,
        ParticlePreset::Rings,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ParticlePreset::Sparks => "Sparks",
            ParticlePreset::Smoke => "Smoke",
            ParticlePreset::Confetti => "Confetti",
            ParticlePreset::Stars => "Stars",
            ParticlePreset::Shards => "Shards",
            ParticlePreset::Rings => "Rings",
        }
    }

    /// Number the particle shader uses to pick the shape.
    pub fn shader_index(self) -> u32 {
        self as u32
    }

    /// An emitter with settings that suit this kind of particle.
    pub fn emitter(self, on: EventMatch) -> Emitter {
        let base = Emitter { on: EventFilter { kind: on, ..EventFilter::default() }, preset: self, ..Emitter::default() };
        match self {
            ParticlePreset::Sparks => base,
            ParticlePreset::Smoke => Emitter {
                count: 6,
                speed: 60.0,
                spread: 360.0,
                size: 14.0,
                life: 1.4,
                gravity: -30.0,
                drag: 1.5,
                color: ParticleColor::Fixed(Color::rgb(0xb8, 0xbc, 0xc8)),
                blend: Blend::Alpha,
                ..base
            },
            ParticlePreset::Confetti => Emitter {
                count: 24,
                speed: 420.0,
                spread: 120.0,
                size: 7.0,
                life: 2.4,
                gravity: 420.0,
                drag: 1.8,
                color: ParticleColor::Gradient(vec![
                    Color::rgb(0xff, 0x3e, 0x5e),
                    Color::rgb(0xff, 0xd8, 0x2e),
                    Color::rgb(0x5c, 0xe6, 0x5c),
                    Color::rgb(0x2e, 0xb8, 0xff),
                    Color::rgb(0x9a, 0x5c, 0xff),
                ]),
                blend: Blend::Alpha,
                ..base
            },
            ParticlePreset::Stars => Emitter {
                count: 5,
                speed: 140.0,
                spread: 360.0,
                size: 9.0,
                life: 0.9,
                gravity: 0.0,
                drag: 2.5,
                color: ParticleColor::Fixed(Color::rgb(0xff, 0xf2, 0xc0)),
                ..base
            },
            ParticlePreset::Shards => Emitter {
                count: 10,
                speed: 260.0,
                spread: 160.0,
                size: 6.0,
                life: 1.2,
                gravity: 500.0,
                drag: 0.6,
                color: ParticleColor::Wall,
                ..base
            },
            ParticlePreset::Rings => Emitter {
                count: 1,
                speed: 0.0,
                spread: 0.0,
                size: 60.0,
                life: 0.5,
                gravity: 0.0,
                drag: 0.0,
                ..base
            },
        }
    }
}

/// Where a particle's colour comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParticleColor {
    /// The colour of the ball that caused the event.
    FromBall,
    /// The colour of the wall or ring that was hit.
    Wall,
    Fixed(Color),
    /// Each particle picks a colour along these, at random.
    Gradient(Vec<Color>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Blend {
    /// Light adds up, so overlapping particles glow.
    Add,
    /// Particles cover what is behind them, like paper or smoke.
    Alpha,
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            enabled: true,
            on: EventFilter::default(),
            preset: ParticlePreset::Sparks,
            count: 12,
            speed: 380.0,
            spread: 140.0,
            size: 2.5,
            life: 0.6,
            gravity: 300.0,
            drag: 2.0,
            color: ParticleColor::FromBall,
            blend: Blend::Add,
            follow_impact: true,
            bounce: false,
            seed: 0,
        }
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self { kind: EventMatch::AnyHit, action: String::new(), ball: None }
    }
}

impl Emitter {
    /// Whether this emitter can show anything at all.
    pub fn is_active(&self) -> bool {
        self.enabled && self.count > 0 && self.life > 0.0 && self.size > 0.0
    }

    /// How many particles one event bursts into, for an impact `strength` from 0 to 1.5
    /// (1 being the scene's full-volume speed).
    pub fn burst_count(&self, strength: f32) -> u32 {
        let count = self.count.min(MAX_BURST);
        if !self.follow_impact || count == 0 {
            return count;
        }
        let scaled = (count as f32 * strength.clamp(0.0, 1.5)).round() as u32;
        scaled.clamp(1, MAX_BURST)
    }

    /// The newest an event can be and still have particles on screen, in seconds.
    pub fn max_age(&self) -> f32 {
        self.life.max(0.0)
    }

    /// Rolls new settings around this emitter's preset. `amount` from 0 (no change) to 1 (anything
    /// the preset allows).
    pub fn roll(&mut self, dice: &mut Dice, amount: f32) {
        let fresh = self.preset.emitter(self.on.kind);
        let wobble = |dice: &mut Dice, value: f32, spread: f32| {
            let factor = dice.log_range(1.0 / (1.0 + spread * amount), 1.0 + spread * amount);
            value * factor
        };
        self.count = (wobble(dice, fresh.count as f32, 1.5).round() as u32).clamp(1, MAX_BURST);
        self.speed = wobble(dice, fresh.speed.max(1.0), 1.0);
        self.spread = (fresh.spread + dice.range(-90.0, 90.0) * amount).clamp(0.0, 360.0);
        self.size = wobble(dice, fresh.size, 0.8);
        self.life = wobble(dice, fresh.life, 0.8);
        self.gravity = fresh.gravity + dice.range(-200.0, 200.0) * amount;
        self.drag = wobble(dice, fresh.drag.max(0.1), 1.0);
        if matches!(self.color, ParticleColor::Fixed(_)) || dice.chance(0.3 * amount) {
            self.color = ParticleColor::Fixed(Color::from_hsv(dice.unit(), dice.range(0.3, 0.9), 1.0));
        }
        self.seed = dice.next_u64();
    }
}

/// A hash of a few numbers into 0..1, the same on the CPU and in the particle shader.
pub fn particle_hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_follows_impact_within_limits() {
        let e = Emitter::default();
        assert_eq!(e.burst_count(1.0), 12);
        assert_eq!(e.burst_count(0.0), 1);
        assert_eq!(e.burst_count(9.0), 18);
        let flat = Emitter { follow_impact: false, count: 100_000, ..Emitter::default() };
        assert_eq!(flat.burst_count(0.1), MAX_BURST);
        let none = Emitter { count: 0, ..Emitter::default() };
        assert_eq!(none.burst_count(1.0), 0);
        assert!(!none.is_active());
    }

    #[test]
    fn every_preset_makes_a_working_emitter() {
        for preset in ParticlePreset::ALL {
            let e = preset.emitter(EventMatch::Wall);
            assert!(e.is_active(), "{}", preset.name());
            assert_eq!(e.preset, preset);
            assert_eq!(e.on.kind, EventMatch::Wall);
        }
    }

    #[test]
    fn rolls_stay_sensible_and_repeat_with_the_seed() {
        for preset in ParticlePreset::ALL {
            let mut a = preset.emitter(EventMatch::AnyHit);
            let mut b = a.clone();
            a.roll(&mut Dice::new(7), 1.0);
            b.roll(&mut Dice::new(7), 1.0);
            assert_eq!(a, b);
            assert!(a.is_active() && a.speed.is_finite() && a.life > 0.0 && (0.0..=360.0).contains(&a.spread));
        }
    }

    #[test]
    fn round_trips_through_ron() {
        let e = ParticlePreset::Confetti.emitter(EventMatch::Escape);
        let text = ron::to_string(&e).unwrap();
        assert_eq!(ron::from_str::<Emitter>(&text).unwrap(), e);
        // Missing fields fall back to defaults.
        let partial: Emitter = ron::from_str("(preset: Smoke)").unwrap();
        assert_eq!(partial.preset, ParticlePreset::Smoke);
        assert_eq!(partial.count, Emitter::default().count);
    }

    #[test]
    fn hash_spreads_evenly() {
        let n = 10_000;
        let mean: f32 = (0..n).map(|i| particle_hash(i, 3)).sum::<f32>() / n as f32;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        assert!((0..n).all(|i| (0.0..1.0).contains(&particle_hash(i, i * 7))));
    }
}
