//! Liquid: a body of particles that flows, splashes and settles level. Small liquids run on the
//! processor; above the particle limit the graphics card takes over, behind the same thing.

pub mod draw;
pub mod pbf;
pub mod sim;

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};

pub struct LiquidKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LiquidStart {
    /// Fills the bottom of the arena.
    #[default]
    Pool,
    /// A block of liquid around the thing, ready to fall.
    Block,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidSettings {
    /// How many particles. Above the limit it runs on the graphics card.
    #[setting(label = "Particles", range(10, 400000), simple, random(500, 20000))]
    pub count: u32,
    /// A pool at the bottom of the arena, or a block around the thing.
    #[setting(label = "Start as", simple)]
    pub start: LiquidStart,
    /// For a pool: how much of the arena's height is liquid.
    #[setting(label = "Fill", when = "start=pool", range(0.05, 1), simple, random)]
    pub fill: f32,
    /// A block's width.
    #[setting(label = "Width", when = "start=block", range(10, 2000), unit = "px")]
    pub width: f32,
    /// A block's height.
    #[setting(label = "Height", when = "start=block", range(10, 3000), unit = "px")]
    pub height: f32,
    /// Pour the liquid in from the thing's position over this long. 0 puts it all there at once.
    #[setting(label = "Pour over", range(0, 60), unit = "s")]
    pub pour_seconds: f32,
    /// Above this many particles the liquid runs on the graphics card.
    #[setting(label = "Processor limit", range(0, 400000))]
    pub cpu_limit: u32,
    /// Changes the pour's scatter without changing anything else.
    #[setting(label = "Seed", range(0, 1000000), random)]
    pub seed: u32,
}

impl Default for LiquidSettings {
    fn default() -> Self {
        Self {
            count: 2000,
            start: LiquidStart::Pool,
            fill: 0.35,
            width: 300.0,
            height: 300.0,
            pour_seconds: 0.0,
            cpu_limit: 4000,
            seed: 1,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Shader];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce];

impl ThingKind for LiquidKind {
    fn id(&self) -> &'static str {
        "liquid"
    }
    fn family(&self) -> &'static str {
        "Flowing"
    }
    fn name(&self) -> &'static str {
        "Liquid"
    }
    fn icon(&self) -> &'static str {
        "liquid"
    }
    fn help(&self) -> &'static str {
        "A body of liquid that flows, splashes and settles."
    }
    fn settings(&self) -> &'static [SettingDef] {
        LiquidSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}
