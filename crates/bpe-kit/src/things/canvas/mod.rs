//! The canvas: a paint surface the marks layer draws into. Brushes (the Marks module on balls,
//! walls and obstacles) paint trails and splat their hits onto every canvas that asks for them.

pub mod draw;
pub mod grid;
pub mod ribbon;
pub mod sim;
pub mod skin;

pub use draw::CanvasDraw;
pub use grid::{CanvasGrid, Cell};
pub use sim::{CanvasSim, CanvasState, canvas_area};
pub use skin::{SkinSettings, is_solid, outline_local, parts_local, skin_area, skin_settings, skin_state, wall_local};

use bpe_core::{Blend, Choice, Color, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

pub struct CanvasThingKind;

/// What brushes do to the canvas. Only Paint for now; later parts add more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CanvasKind {
    /// Brushes lay paint on it.
    #[default]
    Paint,
}

/// Which rectangle of the world the canvas covers. For Arena and View the canvas's own place is
/// an offset from that rectangle's middle (0,0 covers it exactly); a Custom canvas sits around
/// its own place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Fit {
    /// The box around the first arena.
    #[default]
    Arena,
    /// What the video shows at the start.
    View,
    /// A size of its own, around the canvas's own place.
    Custom,
}

/// Where the canvas is drawn among the other things.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CanvasLayerPos {
    BehindEverything,
    #[default]
    BehindBalls,
    OnTop,
}

/// What the canvas shows before anything paints it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum StartsAs {
    /// Nothing: whatever is behind shows through.
    #[default]
    Empty,
    /// One colour all over.
    Colour,
}

/// How the paint looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CanvasStyle {
    #[default]
    Flat,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CanvasSettings {
    /// What brushes do to it.
    #[setting(label = "Kind", simple)]
    pub kind: CanvasKind,
    /// Which part of the world it covers.
    #[setting(label = "Fits", simple, group = "Area")]
    pub fit: Fit,
    /// How big it is, when it has a size of its own.
    #[setting(label = "Size", when = "fit=custom", unit = "px", group = "Area")]
    pub size: Vec2,
    /// Where it is drawn: behind everything, behind the balls, or on top.
    #[setting(label = "Layer", group = "Look")]
    pub layer: CanvasLayerPos,
    /// Which things' brushes mark it.
    #[setting(label = "Brushes from", simple)]
    pub brushes_from: Target,
    /// What it shows before anything paints it.
    #[setting(label = "Starts as", group = "Look")]
    pub starts_as: StartsAs,
    /// The colour it starts as.
    #[setting(label = "Start colour", when = "starts_as=colour", group = "Look")]
    pub start_colour: Color,
    /// How the paint looks.
    #[setting(label = "Style", group = "Look")]
    pub style: CanvasStyle,
    /// How sharp the paint is drawn: texture pixels per world pixel.
    #[setting(label = "Detail", range(0.25, 1), group = "Look")]
    pub detail: f32,
    /// How long marks take to fade away; 0 never fades.
    #[setting(label = "Fade", range(0, 60), unit = "s", group = "Over time")]
    pub fade: f32,
    /// How it mixes with what is under it.
    #[setting(label = "Blend", group = "Look")]
    pub blend: Blend,
    /// How much of it shows.
    #[setting(label = "Opacity", range(0, 1), group = "Look", keyframe)]
    pub opacity: f32,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            kind: CanvasKind::Paint,
            fit: Fit::Arena,
            size: Vec2::new(1080.0, 1920.0),
            layer: CanvasLayerPos::BehindBalls,
            brushes_from: Target::Anything,
            starts_as: StartsAs::Empty,
            start_colour: Color::WHITE,
            style: CanvasStyle::Flat,
            detail: 1.0,
            fade: 0.0,
            blend: Blend::Normal,
            opacity: 1.0,
        }
    }
}

impl ThingKind for CanvasThingKind {
    fn id(&self) -> &'static str {
        "canvas"
    }
    fn family(&self) -> &'static str {
        "Walls"
    }
    fn name(&self) -> &'static str {
        "Canvas"
    }
    fn icon(&self) -> &'static str {
        "canvas"
    }
    fn help(&self) -> &'static str {
        "A surface the brushes of balls, walls and obstacles paint trails and splats onto."
    }
    fn settings(&self) -> &'static [SettingDef] {
        CanvasSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    // The renderer draws only the world's canvases, never the screen's.
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}
