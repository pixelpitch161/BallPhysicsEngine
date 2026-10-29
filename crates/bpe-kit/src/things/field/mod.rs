//! Field: an invisible zone that pushes what passes through it and logs entries and leaves.

pub mod sim;

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};

pub struct FieldKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FieldKindChoice {
    /// A steady push one way.
    #[default]
    Wind,
    /// Pulls everything towards its middle.
    Well,
    /// Pushes everything away from its middle.
    Repel,
    /// Spins everything around its middle.
    Vortex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FieldShape {
    #[default]
    Circle,
    Box,
    /// The whole scene.
    Everywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Falloff {
    /// Full strength right to the edge.
    Flat,
    /// Fades evenly to the edge.
    Linear,
    /// Strong in the middle, gentle at the edge.
    #[default]
    Smooth,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FieldSettings {
    /// What the field does to what passes through it.
    #[setting(label = "Kind", simple, random)]
    pub kind: FieldKindChoice,
    /// How hard it pushes, as an acceleration.
    #[setting(label = "Strength", range(0, 5000), unit = "px/s²", simple, keyframe, random, pulse)]
    pub strength: f32,
    /// How far from its middle a round field reaches.
    #[setting(label = "Radius", range(10, 3000), unit = "px", simple, keyframe, random)]
    pub radius: f32,
    /// Round, a box, or the whole scene.
    #[setting(label = "Shape", simple)]
    pub shape: FieldShape,
    /// What it pushes: a thing, a tag, or anything.
    #[setting(label = "Pushes", simple)]
    pub targets: Target,
    /// Which way the wind blows, in degrees clockwise from the right.
    #[setting(label = "Direction", range(-360, 360), unit = "°", simple, keyframe, random)]
    pub direction: f32,
    /// A box field's width.
    #[setting(label = "Width", range(10, 4000), unit = "px", keyframe)]
    pub width: f32,
    /// A box field's height.
    #[setting(label = "Height", range(10, 4000), unit = "px", keyframe)]
    pub height: f32,
    /// How the push fades towards the edge.
    #[setting(label = "Falloff")]
    pub falloff: Falloff,
    /// For a vortex: how much of the strength also pulls towards the middle. Negative flings out.
    #[setting(label = "Pull", range(-1, 1), keyframe)]
    pub pull: f32,
    /// Log an event when something enters or leaves the field.
    #[setting(label = "Log enter and leave")]
    pub log_events: bool,
}

impl Default for FieldSettings {
    fn default() -> Self {
        Self {
            kind: FieldKindChoice::Wind,
            strength: 600.0,
            radius: 300.0,
            shape: FieldShape::Circle,
            targets: Target::Anything,
            direction: 0.0,
            width: 600.0,
            height: 400.0,
            falloff: Falloff::Smooth,
            pull: 0.3,
            log_events: true,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Rules];

impl ThingKind for FieldKind {
    fn id(&self) -> &'static str {
        "field"
    }
    fn name(&self) -> &'static str {
        "Field"
    }
    fn icon(&self) -> &'static str {
        "field"
    }
    fn help(&self) -> &'static str {
        "An invisible zone: wind, a gravity well, a repeller or a vortex."
    }
    fn settings(&self) -> &'static [SettingDef] {
        FieldSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
}
