//! Trails: a streak behind each ball along where it has been.

pub mod draw;

use bpe_core::{Blend, Choice, Color, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::{Follow, Paint};

pub struct StreakTrailKind;

/// Where a trail takes its colour from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TrailSource {
    /// The ball's own colour.
    #[default]
    Ball,
    /// The trail's own Colours.
    Own,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct StreakTrail {
    /// How far back the streak reaches.
    #[setting(label = "Length", range(0.05, 2), unit = "s", simple, keyframe, random)]
    pub seconds: f32,
    /// How wide the streak is at the ball, in ball radii.
    #[setting(label = "Width", range(0.05, 2), simple, keyframe, random)]
    pub width: f32,
    /// How bright the streak is at the ball.
    #[setting(label = "Brightness", range(0, 2), simple, keyframe, random, pulse)]
    pub brightness: f32,
    /// The ball's own colour, or the Colours below.
    #[setting(label = "Colour from", simple, random)]
    pub source: TrailSource,
    /// The streak's own colours: over its life from the ball to the tail, or by where, how fast
    /// or when each part of it was drawn.
    #[setting(
        paint,
        label = "Colours",
        prefix = "",
        when = "source=own",
        follows = "life|place|speed|time|beat",
        simple
    )]
    pub colour: Paint,
    /// How much the streak narrows towards its tail: 0 keeps its width, 1 ends in a point.
    #[setting(label = "Taper", range(0, 1), keyframe, random)]
    pub taper: f32,
    /// How the streak mixes with what is under it: Add glows and adds its light, Normal paints
    /// over it, the others mix with it. With the others, everything with the same Blend and
    /// amount is mixed in once, as one picture, so where streaks overlap they do not stack.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if the streak weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for StreakTrail {
    fn default() -> Self {
        Self {
            seconds: 0.5,
            width: 0.5,
            brightness: 0.5,
            source: TrailSource::Ball,
            colour: Paint { follow: Follow::Life, ..Paint::solid(Color::WHITE) },
            taper: 1.0,
            blend: Blend::Add,
            blend_amount: 1.0,
        }
    }
}

impl ModuleKind for StreakTrailKind {
    fn id(&self) -> &'static str {
        "trail.streak"
    }
    fn name(&self) -> &'static str {
        "Streak"
    }
    fn help(&self) -> &'static str {
        "A fading streak behind each ball along where it has been."
    }
    fn slot(&self) -> SlotId {
        SlotId::Trail
    }
    fn settings(&self) -> &'static [SettingDef] {
        StreakTrail::defs()
    }
}
