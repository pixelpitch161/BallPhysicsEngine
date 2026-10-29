//! Emitter: releases balls (or liquid drops) into a target thing, over time or on a hit. What it
//! releases joins the target, so it uses the target's look, motion, bounce and sound.

pub mod sim;

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};

pub struct EmitterKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Trigger {
    /// Steadily, at the rate.
    #[default]
    Time,
    /// One item for every hit by the things it listens to.
    Hit,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EmitterSettings {
    /// What the released items join: a thing, a tag, or anything with bodies.
    #[setting(label = "Target", simple)]
    pub target: Target,
    /// How many items a second.
    #[setting(label = "Rate", range(0, 2000), unit = "per s", simple, keyframe, random(5, 200))]
    pub rate: f32,
    /// How fast each item leaves.
    #[setting(label = "Speed", range(0, 3000), unit = "px/s", simple, keyframe, random(100, 1200))]
    pub speed: f32,
    /// Which way, in degrees clockwise from the right. -90 is straight up.
    #[setting(label = "Direction", range(-360, 360), unit = "°", simple, keyframe, random)]
    pub direction: f32,
    /// How wide a cone the items scatter over.
    #[setting(label = "Spread", range(0, 360), unit = "°", simple, keyframe, random)]
    pub spread: f32,
    /// Items released all at once when it starts.
    #[setting(label = "Burst", range(0, 1000))]
    pub burst: u32,
    /// When it starts releasing.
    #[setting(label = "Start", range(0, 600), unit = "s")]
    pub start: f32,
    /// When it stops; 0 never stops.
    #[setting(label = "Stop", range(0, 600), unit = "s")]
    pub stop: f32,
    /// The most items it will ever release.
    #[setting(label = "Limit", range(1, 100000))]
    pub limit: u32,
    /// Items leave from a line this wide, across the direction.
    #[setting(label = "Width", range(0, 2000), unit = "px")]
    pub width: f32,
    /// Items are this many times the target's own size.
    #[setting(label = "Size", range(0.1, 5), keyframe, random)]
    pub size_scale: f32,
    /// Release steadily, or one item for every hit it hears.
    #[setting(label = "On")]
    pub on: Trigger,
    /// Whose hits it listens to when it releases on hits.
    #[setting(label = "Listen to", when = "on=hit")]
    pub listen: Target,
    /// Changes the scatter without changing anything else.
    #[setting(label = "Seed", range(0, 1000000), random)]
    pub seed: u32,
}

impl Default for EmitterSettings {
    fn default() -> Self {
        Self {
            target: Target::Anything,
            rate: 20.0,
            speed: 500.0,
            direction: -90.0,
            spread: 20.0,
            burst: 0,
            start: 0.0,
            stop: 0.0,
            limit: 2000,
            width: 0.0,
            size_scale: 1.0,
            on: Trigger::Time,
            listen: Target::Anything,
            seed: 1,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Rules];

impl ThingKind for EmitterKind {
    fn id(&self) -> &'static str {
        "emitter"
    }
    fn name(&self) -> &'static str {
        "Emitter"
    }
    fn icon(&self) -> &'static str {
        "emitter"
    }
    fn help(&self) -> &'static str {
        "Releases balls or drops into a target thing, over time or on a hit."
    }
    fn settings(&self) -> &'static [SettingDef] {
        EmitterSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
}
