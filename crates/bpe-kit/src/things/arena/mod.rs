//! The arena: a closed wall the balls stay inside of.

pub mod sim;

pub use sim::{ArenaSim, ArenaState};

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};

pub struct ArenaKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ArenaShape {
    #[default]
    Circle,
    Box,
    Polygon,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ArenaSettings {
    /// A round ring, a box, or a regular polygon.
    #[setting(label = "Shape", simple, random)]
    pub shape: ArenaShape,
    /// How big the ring or polygon is, from the middle to the wall.
    #[setting(label = "Radius", range(20, 2000), unit = "px", simple, keyframe, random)]
    pub radius: f32,
    /// How thick the wall is drawn.
    #[setting(label = "Thickness", range(0, 40), unit = "px", simple, keyframe)]
    pub thickness: f32,
    /// How fast the arena turns, clockwise.
    #[setting(label = "Spin", range(-360, 360), unit = "deg/s", simple, keyframe, random)]
    pub spin: f32,
    /// Box only.
    #[setting(label = "Width", range(20, 4000), unit = "px")]
    pub width: f32,
    /// Box only.
    #[setting(label = "Height", range(20, 4000), unit = "px")]
    pub height: f32,
    /// Polygon only.
    #[setting(label = "Sides", range(3, 12), random)]
    pub sides: u32,
    /// How much of its size the arena loses each second.
    #[setting(label = "Shrink", range(0, 1), unit = "per s", keyframe)]
    pub shrink: f32,
    /// The arena stops shrinking at this size.
    #[setting(label = "Smallest size", range(10, 2000), unit = "px")]
    pub min_size: f32,
}

impl Default for ArenaSettings {
    fn default() -> Self {
        Self {
            shape: ArenaShape::Circle,
            radius: 508.0,
            thickness: 3.0,
            spin: 0.0,
            width: 900.0,
            height: 1500.0,
            sides: 6,
            shrink: 0.0,
            min_size: 100.0,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Sound, SlotId::Particles, SlotId::Rules, SlotId::Shader];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Sound];

impl ThingKind for ArenaKind {
    fn id(&self) -> &'static str {
        "arena"
    }
    fn name(&self) -> &'static str {
        "Arena"
    }
    fn icon(&self) -> &'static str {
        "arena"
    }
    fn help(&self) -> &'static str {
        "A closed wall the balls bounce around inside."
    }
    fn settings(&self) -> &'static [SettingDef] {
        ArenaSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}
