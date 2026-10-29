//! Soft bodies: a jelly ball, a rope or a sheet of cloth made of points moved by one
//! position-based solver.

pub mod build;
pub mod draw;
pub mod sim;
pub mod solver;

pub use build::build;
pub use draw::SoftDraw;
pub use sim::{Impact, SoftSim, SoftState};
pub use solver::{Point, SoftWorld};

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct SoftKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SoftShape {
    #[default]
    Jelly,
    Rope,
    Cloth,
}

/// Which points are held in place. `Usual` is what each shape mostly wants: a rope hangs from
/// both ends, cloth from its top row, a jelly ball is free.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Pinned {
    #[default]
    Usual,
    Nothing,
    Start,
    End,
    BothEnds,
    TopRow,
    TopCorners,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SoftSettings {
    /// A jelly ball, a rope or a sheet of cloth.
    #[setting(label = "Shape", simple, random)]
    pub shape: SoftShape,
    /// A jelly ball's radius, a rope's length, or a cloth's width.
    #[setting(label = "Size", range(20, 1500), unit = "px", simple, random(60, 400))]
    pub size: f32,
    /// How easily it squashes, swings and bends.
    #[setting(label = "Softness", range(0, 1), simple, random)]
    pub softness: f32,
    /// How many points make it: a jelly ball's ring, a rope's beads, a cloth's columns.
    #[setting(label = "Points", range(2, 120), simple)]
    pub detail: u32,
    /// Which points are held in place.
    #[setting(label = "Pinned", simple)]
    pub pinned: Pinned,
    /// Cloth only.
    #[setting(label = "Height", range(20, 1500), unit = "px")]
    pub height: f32,
    /// Cloth only: rows of points from top to bottom.
    #[setting(label = "Rows", range(2, 120))]
    pub rows: u32,
    /// Rope only: how thick it is drawn and collides.
    #[setting(label = "Thickness", range(1, 60), unit = "px")]
    pub thickness: f32,
    /// Rope only: how much longer than the gap between its ends it is.
    #[setting(label = "Slack", range(1, 3))]
    pub slack: f32,
    /// Jelly only: how fast, and which way, it starts moving.
    #[setting(label = "Launch speed", unit = "px/s")]
    pub velocity: Vec2,
    /// More passes make it stiffer and steadier, for more work.
    #[setting(label = "Solver passes", range(1, 64))]
    pub passes: u32,
}

impl Default for SoftSettings {
    fn default() -> Self {
        Self {
            shape: SoftShape::Jelly,
            size: 120.0,
            softness: 0.5,
            detail: 24,
            pinned: Pinned::Usual,
            height: 400.0,
            rows: 20,
            thickness: 8.0,
            slack: 1.15,
            velocity: Vec2::ZERO,
            passes: 8,
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound];

impl ThingKind for SoftKind {
    fn id(&self) -> &'static str {
        "soft"
    }
    fn name(&self) -> &'static str {
        "Soft body"
    }
    fn icon(&self) -> &'static str {
        "soft"
    }
    fn help(&self) -> &'static str {
        "A jelly ball, a rope or a sheet of cloth that squashes, swings and bends."
    }
    fn settings(&self) -> &'static [SettingDef] {
        SoftSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        SLOTS
    }
}
