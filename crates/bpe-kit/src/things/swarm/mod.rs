//! Swarm: thousands of points of light that swirl with the air, flock, follow the balls and
//! keep off the walls. It runs on the graphics card only and never touches the physics.

pub mod draw;
pub mod sim;

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};

pub struct SwarmKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SwarmStart {
    /// Spread over the whole arena.
    #[default]
    Fill,
    /// Packed around the thing.
    Centre,
    /// A ring around the thing.
    Ring,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SwarmSettings {
    /// How many points of light.
    #[setting(label = "Points", range(100, 1000000), simple, random(500, 50000))]
    pub count: u32,
    /// Where the points begin.
    #[setting(label = "Start as", simple)]
    pub start: SwarmStart,
    /// Let the points out from the thing over this long. 0 starts them all in place.
    #[setting(label = "Release over", range(0, 60), unit = "s")]
    pub release_seconds: f32,
    /// Changes where the points start without changing anything else.
    #[setting(label = "Seed", range(0, 1000000), random)]
    pub seed: u32,
}

impl Default for SwarmSettings {
    fn default() -> Self {
        Self { count: 50_000, start: SwarmStart::Fill, release_seconds: 0.0, seed: 1 }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion];

impl ThingKind for SwarmKind {
    fn id(&self) -> &'static str {
        "swarm"
    }
    fn family(&self) -> &'static str {
        "Flowing"
    }
    fn name(&self) -> &'static str {
        "Swarm"
    }
    fn icon(&self) -> &'static str {
        "swarm"
    }
    fn help(&self) -> &'static str {
        "Thousands of points of light that swirl, flock and follow the balls."
    }
    fn settings(&self) -> &'static [SettingDef] {
        SwarmSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        SLOTS
    }
}
