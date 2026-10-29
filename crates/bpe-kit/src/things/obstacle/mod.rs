//! Obstacles: pegs, bars, peg grids, polygons and rings inside the arena that balls bounce off.

pub mod draw;
pub mod sim;

pub use sim::{ObstacleSim, ObstacleState, parts, path_offset};

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct ObstacleKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ObstacleShape {
    #[default]
    Peg,
    Bar,
    PegGrid,
    Polygon,
    Ring,
}

/// How an obstacle moves about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MovePath {
    #[default]
    Still,
    Line,
    Circle,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ObstacleSettings {
    /// A round peg, a bar, a grid of pegs, a hollow polygon or a hollow ring.
    #[setting(label = "Shape", simple, random)]
    pub shape: ObstacleShape,
    /// Peg, grid peg, polygon or ring: from the middle to the edge.
    #[setting(label = "Size", range(2, 800), unit = "px", simple, keyframe, random(10, 200))]
    pub radius: f32,
    /// Bar only.
    #[setting(label = "Length", range(10, 4000), unit = "px", simple, keyframe, random(40, 600))]
    pub length: f32,
    /// Bar, polygon and ring: how thick the wall is.
    #[setting(label = "Thickness", range(1, 200), unit = "px", simple, keyframe)]
    pub thickness: f32,
    /// How fast it turns, clockwise.
    #[setting(label = "Spin", range(-720, 720), unit = "deg/s", simple, keyframe, random(-240, 240))]
    pub spin: f32,
    /// Stays still, slides along a straight line and back, or goes round a circle.
    #[setting(label = "Moves along", simple)]
    pub path: MovePath,
    /// Line: how far each way. Circle: x is the circle's radius.
    #[setting(label = "Travel", unit = "px", group = "Movement", keyframe)]
    pub path_distance: Vec2,
    /// How long one trip there and back, or once round, takes.
    #[setting(label = "Round trip", range(0.2, 120), unit = "s", group = "Movement")]
    pub path_seconds: f32,
    /// Where along the trip it starts, as a fraction of one round trip.
    #[setting(label = "Start phase", range(0, 1), unit = "turns", group = "Movement")]
    pub path_phase: f32,
    /// Polygon only.
    #[setting(label = "Sides", range(3, 12), group = "Shape details", random)]
    pub sides: u32,
    /// Peg grid only.
    #[setting(label = "Columns", range(1, 100), group = "Shape details")]
    pub columns: u32,
    /// Peg grid only.
    #[setting(label = "Rows", range(1, 100), group = "Shape details")]
    pub rows: u32,
    /// Peg grid only: from one peg's middle to the next.
    #[setting(label = "Spacing", range(4, 1000), unit = "px", group = "Shape details")]
    pub spacing: f32,
    /// Peg grid: shift every other row by half a step, like a Plinko board.
    #[setting(label = "Stagger", group = "Shape details")]
    pub stagger: bool,
}

impl Default for ObstacleSettings {
    fn default() -> Self {
        Self {
            shape: ObstacleShape::Peg,
            radius: 30.0,
            length: 400.0,
            thickness: 8.0,
            spin: 0.0,
            path: MovePath::Still,
            path_distance: Vec2::new(300.0, 0.0),
            path_seconds: 4.0,
            path_phase: 0.0,
            sides: 6,
            columns: 7,
            rows: 9,
            spacing: 90.0,
            stagger: true,
        }
    }
}

const SLOTS: &[SlotId] =
    &[SlotId::Look, SlotId::Bounce, SlotId::Sound, SlotId::Particles, SlotId::Rules, SlotId::Shader];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Sound];

impl ThingKind for ObstacleKind {
    fn id(&self) -> &'static str {
        "obstacle"
    }
    fn name(&self) -> &'static str {
        "Obstacle"
    }
    fn icon(&self) -> &'static str {
        "obstacle"
    }
    fn help(&self) -> &'static str {
        "A peg, bar, peg grid, polygon or ring inside the arena that balls bounce off."
    }
    fn settings(&self) -> &'static [SettingDef] {
        ObstacleSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}
