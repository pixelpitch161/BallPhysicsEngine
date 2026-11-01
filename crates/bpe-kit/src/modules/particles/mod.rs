//! Particles: puffs of sparks, smoke, confetti, stars, shards or rings where balls hit.

pub mod draw;

use bpe_core::{Blend, Choice, Color, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::{Follow, Paint};

pub struct HitBurstKind;

/// What a burst is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BurstPreset {
    #[default]
    Sparks,
    Smoke,
    Confetti,
    Stars,
    Shards,
    Rings,
}

impl BurstPreset {
    /// The shape's number in particles.wgsl.
    pub fn code(self) -> u32 {
        match self {
            Self::Sparks => 0,
            Self::Smoke => 1,
            Self::Confetti => 2,
            Self::Stars => 3,
            Self::Shards => 4,
            Self::Rings => 5,
        }
    }
}

/// Where a burst takes its colour from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BurstSource {
    /// The colour of the ball that hit.
    #[default]
    Ball,
    /// The burst's own Colours.
    Own,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HitBurst {
    /// What the burst is made of.
    #[setting(label = "Preset", group = "Burst", simple, random)]
    pub preset: BurstPreset,
    /// How many particles each hit lets off.
    #[setting(label = "Count", group = "Burst", range(1, 200), simple, keyframe, random(5, 80))]
    pub count: i32,
    /// How fast the particles fly.
    #[setting(
        label = "Speed",
        group = "Burst",
        range(0, 2000),
        unit = "px/s",
        simple,
        keyframe,
        random(100, 900),
        pulse
    )]
    pub speed: f32,
    /// How wide the burst fans out from the direction of the bounce.
    #[setting(label = "Spread", group = "Burst", range(0, 360), unit = "°", keyframe, random)]
    pub spread: f32,
    /// Hits slower than this let off nothing.
    #[setting(label = "Least speed", group = "Burst", range(0, 2000), unit = "px/s")]
    pub least_speed: f32,
    /// Harder hits let off faster particles.
    #[setting(label = "Follow impact", group = "Burst", random)]
    pub follow_impact: bool,
    /// How big each particle is; for rings, how big the ring grows.
    #[setting(label = "Size", group = "Each particle", range(0.5, 200), unit = "px", simple, keyframe, random(2, 20))]
    pub size: f32,
    /// How long the particles last.
    #[setting(label = "Life", group = "Each particle", range(0.05, 3), unit = "s", simple, keyframe, random)]
    pub life: f32,
    /// How fast the particles fall; negative makes them rise.
    #[setting(label = "Gravity", group = "Each particle", range(-2000, 2000), unit = "px/s²", keyframe, random(-800, 800))]
    pub gravity: f32,
    /// How quickly the particles slow down.
    #[setting(label = "Drag", group = "Each particle", range(0, 10), keyframe, random(0, 4))]
    pub drag: f32,
    /// The colour of the ball that hit, or the Colours below.
    #[setting(label = "Colour from", group = "Colours", simple, random)]
    pub source: BurstSource,
    /// The particles' own colours: over each one's life, a stop picked at random for each, or
    /// by where, how fast or when the hit was.
    #[setting(
        paint,
        label = "Colours",
        group = "Colours",
        prefix = "",
        when = "source=own",
        follows = "life|order|place|speed|time|beat"
    )]
    pub colour: Paint,
    /// How the particles mix with what is under them: Add glows and adds their light, Normal
    /// paints over it, the others mix with it. With the others, everything with the same Blend
    /// and amount is mixed in once, as one picture, so where particles overlap they do not stack.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if the particles weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for HitBurst {
    fn default() -> Self {
        Self {
            preset: BurstPreset::Sparks,
            count: 12,
            speed: 380.0,
            size: 2.5,
            life: 0.6,
            source: BurstSource::Ball,
            colour: Paint { follow: Follow::Life, ..Paint::solid(Color::WHITE) },
            spread: 140.0,
            gravity: 300.0,
            drag: 2.0,
            follow_impact: true,
            least_speed: 0.0,
            blend: Blend::Add,
            blend_amount: 1.0,
        }
    }
}

impl HitBurst {
    /// Whether the particles add their light.
    pub fn additive(&self) -> bool {
        self.blend == Blend::Add
    }
}

impl ModuleKind for HitBurstKind {
    fn id(&self) -> &'static str {
        "particles.burst"
    }
    fn name(&self) -> &'static str {
        "Hit burst"
    }
    fn help(&self) -> &'static str {
        "A puff of sparks, smoke, confetti, stars, shards or rings wherever a ball hits."
    }
    fn slot(&self) -> SlotId {
        SlotId::Particles
    }
    fn settings(&self) -> &'static [SettingDef] {
        HitBurst::defs()
    }
}
