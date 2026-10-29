//! Balls: one ball, a block of them, or a ring.

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct BallsKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Arrangement {
    #[default]
    Single,
    Block,
    Ring,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BallsSettings {
    /// One ball, a block of rows and columns, or a ring.
    #[setting(label = "Arrangement", simple, random)]
    pub arrangement: Arrangement,
    /// How big each ball is.
    #[setting(label = "Size", range(0.5, 200), unit = "px", simple, keyframe, random)]
    pub radius: f32,
    /// How fast, and which way, the balls start moving.
    #[setting(label = "Launch speed", unit = "px/s", simple, random)]
    pub velocity: Vec2,
    /// Block only.
    #[setting(label = "Columns", range(1, 200), simple)]
    pub columns: u32,
    /// Block only.
    #[setting(label = "Rows", range(1, 200), simple)]
    pub rows: u32,
    /// Block only: from one ball's middle to the next.
    #[setting(label = "Spacing", range(1, 400), unit = "px", simple)]
    pub spacing: f32,
    /// Ring only.
    #[setting(label = "Count", range(1, 2000))]
    pub count: u32,
    /// Ring only.
    #[setting(label = "Ring radius", range(1, 2000), unit = "px")]
    pub ring_radius: f32,
}

impl Default for BallsSettings {
    fn default() -> Self {
        Self {
            arrangement: Arrangement::Single,
            radius: 24.0,
            velocity: Vec2::new(300.0, -200.0),
            columns: 10,
            rows: 10,
            spacing: 19.0,
            count: 24,
            ring_radius: 200.0,
        }
    }
}

const SLOTS: &[SlotId] =
    &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce, SlotId::Sound, SlotId::Particles, SlotId::Rules];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound];

impl ThingKind for BallsKind {
    fn id(&self) -> &'static str {
        "balls"
    }
    fn name(&self) -> &'static str {
        "Balls"
    }
    fn icon(&self) -> &'static str {
        "ball"
    }
    fn help(&self) -> &'static str {
        "One ball or many, launched together."
    }
    fn settings(&self) -> &'static [SettingDef] {
        BallsSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}
