//! The canvas: a paint surface the marks layer draws into. Brushes (the Marks module on balls,
//! walls and obstacles) paint trails and splat their hits onto every canvas that asks for them.

pub mod draw;
pub mod evolve;
pub mod grid;
pub mod ribbon;
pub mod sim;
pub mod skin;

pub use draw::CanvasDraw;
pub use grid::{CanvasGrid, Cell, DRIP_JOINED, TICK_HZ, painter, tick_every, wet};
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
    /// Plain colour.
    #[default]
    Flat,
    /// Glossy while wet, matte once dry, lit from a side.
    WetPaint,
    /// Soft, grainy washes with darker edges.
    Watercolour,
    /// A bright core with a glow round it.
    NeonInk,
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
    /// Where the light comes from, in degrees clockwise from the top.
    #[setting(label = "Light angle", when = "style=wet_paint", range(0, 360), unit = "°", group = "Look")]
    pub light_angle: f32,
    /// How shiny wet paint is.
    #[setting(label = "Gloss", when = "style=wet_paint", range(0, 1), group = "Look", keyframe)]
    pub gloss: f32,
    /// How grainy the washes are.
    #[setting(label = "Grain", when = "style=watercolour", range(0, 1), group = "Look")]
    pub grain: f32,
    /// How dark the edges of a wash are.
    #[setting(label = "Edge darkening", when = "style=watercolour", range(0, 1), group = "Look")]
    pub edge_dark: f32,
    /// How far the glow reaches.
    #[setting(label = "Glow size", when = "style=neon_ink", range(2, 80), unit = "px", group = "Look")]
    pub glow_size: f32,
    /// How bright the glow is.
    #[setting(label = "Glow strength", when = "style=neon_ink", range(0, 3), group = "Look", keyframe)]
    pub glow_strength: f32,
    /// How sharp the paint is drawn: texture pixels per world pixel.
    #[setting(label = "Detail", range(0.25, 1), group = "Look")]
    pub detail: f32,
    /// How long paint stays wet; wet paint can bleed, drip and be picked up.
    #[setting(label = "Dry time", range(0, 30), unit = "s", group = "Over time")]
    pub dry_time: f32,
    /// How much wet colours run into each other.
    #[setting(label = "Bleed", range(0, 1), group = "Over time")]
    pub bleed: f32,
    /// How much thick wet paint runs down.
    #[setting(label = "Drips", range(0, 1), group = "Over time")]
    pub drips: f32,
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
            light_angle: 315.0,
            gloss: 0.6,
            grain: 0.5,
            edge_dark: 0.5,
            glow_size: 18.0,
            glow_strength: 1.0,
            detail: 1.0,
            dry_time: 1.0,
            bleed: 0.0,
            drips: 0.0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    fn applying(style: CanvasStyle) -> Vec<&'static str> {
        let name = style.name();
        let get = |k: &str| (k == "style").then(|| Value::Choice(name.to_string()));
        CanvasSettings::defs().iter().filter(|d| d.applies(None, &get)).map(|d| d.key).collect()
    }

    #[test]
    fn canvas_style_knobs_show_for_their_style() {
        let knobs = [
            ("light_angle", CanvasStyle::WetPaint),
            ("gloss", CanvasStyle::WetPaint),
            ("grain", CanvasStyle::Watercolour),
            ("edge_dark", CanvasStyle::Watercolour),
            ("glow_size", CanvasStyle::NeonInk),
            ("glow_strength", CanvasStyle::NeonInk),
        ];
        for style in [CanvasStyle::Flat, CanvasStyle::WetPaint, CanvasStyle::Watercolour, CanvasStyle::NeonInk] {
            let shown = applying(style);
            for (key, owner) in knobs {
                assert_eq!(shown.contains(&key), style == owner, "{key} for {style:?}");
            }
            for key in ["dry_time", "bleed", "drips"] {
                assert!(shown.contains(&key), "{key} for {style:?}");
            }
        }
    }

    #[test]
    fn canvas_defaults_keep_existing_canvases() {
        let d = CanvasSettings::default();
        assert!(d.style == CanvasStyle::Flat && d.dry_time == 1.0 && d.bleed == 0.0 && d.drips == 0.0);
        assert_eq!(CanvasSettings::from_params(&d.to_params()), d);
    }
}
