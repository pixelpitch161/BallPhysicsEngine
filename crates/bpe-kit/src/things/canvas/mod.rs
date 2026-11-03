//! The canvas: a paint surface the marks layer draws into. Brushes (the Marks module on balls,
//! walls and obstacles) paint trails and splat their hits onto every canvas that asks for them.

pub mod claims;
pub mod draw;
pub mod drops;
pub mod evolve;
pub mod grid;
pub mod ribbon;
pub mod sim;
pub mod skin;

pub use claims::{ClaimMap, Claims, TeamKey, colour_name};
pub use draw::CanvasDraw;
pub use grid::{CanvasGrid, Cell, DRIP_JOINED, TICK_HZ, painter, tick_every, wet};
pub use sim::{CanvasSim, CanvasState, canvas_area};
pub use skin::{SkinSettings, is_solid, outline_local, parts_local, skin_area, skin_settings, skin_state, wall_local};

use bpe_core::{Blend, Choice, Color, Gradient, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

pub struct CanvasThingKind;

/// What brushes do to the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CanvasKind {
    /// Brushes lay paint on it.
    #[default]
    Paint,
    /// Balls scrape it off.
    Cover,
    /// Sand or snow that balls plough through.
    Powder,
    /// Misted glass that balls wipe clear.
    Fog,
}

/// How a cover looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CoverLook {
    /// Plain, slightly grainy.
    #[default]
    Matte,
    /// Shiny, with a sweep of light.
    Foil,
}

/// What a powder canvas is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PowderLook {
    #[default]
    Sand,
    /// Soft, with bluish shadows and a few sparkles.
    Snow,
}

/// What a fog canvas is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FogLook {
    /// Grainy ice crystals.
    #[default]
    Frost,
    /// Smooth mist that runs down in drops.
    Steam,
}

/// What shows where a cover is scraped away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Underneath {
    /// Whatever is behind the canvas.
    #[default]
    Scene,
    /// One colour.
    Colour,
    /// A gradient from top to bottom.
    Gradient,
    /// A picture file.
    Picture,
    /// A big word on a colour.
    Text,
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
    #[setting(label = "Layer", when = "kind!=fog", group = "Look")]
    pub layer: CanvasLayerPos,
    /// Which things' brushes mark it.
    #[setting(label = "Brushes from", simple)]
    pub brushes_from: Target,
    /// What it shows before anything paints it.
    #[setting(label = "Starts as", when = "kind=paint", group = "Look")]
    pub starts_as: StartsAs,
    /// The colour it starts as.
    #[setting(label = "Start colour", when = "starts_as=colour or kind=cover|powder|fog", group = "Look")]
    pub start_colour: Color,
    /// How the paint looks.
    #[setting(label = "Style", when = "kind=paint", group = "Look")]
    pub style: CanvasStyle,
    /// Where the light comes from, in degrees clockwise from the top.
    #[setting(
        label = "Light angle",
        when = "style=wet_paint or kind=powder",
        range(0, 360),
        unit = "°",
        group = "Look"
    )]
    pub light_angle: f32,
    /// How shiny wet paint is.
    #[setting(label = "Gloss", when = "kind=paint and style=wet_paint", range(0, 1), group = "Look", keyframe)]
    pub gloss: f32,
    /// How grainy the washes are.
    #[setting(label = "Grain", when = "kind=paint and style=watercolour", range(0, 1), group = "Look")]
    pub grain: f32,
    /// How dark the edges of a wash are.
    #[setting(label = "Edge darkening", when = "kind=paint and style=watercolour", range(0, 1), group = "Look")]
    pub edge_dark: f32,
    /// How far the glow reaches.
    #[setting(label = "Glow size", when = "kind=paint and style=neon_ink", range(2, 80), unit = "px", group = "Look")]
    pub glow_size: f32,
    /// How bright the glow is.
    #[setting(label = "Glow strength", when = "kind=paint and style=neon_ink", range(0, 3), group = "Look", keyframe)]
    pub glow_strength: f32,
    /// How sharp the paint is drawn: texture pixels per world pixel.
    #[setting(label = "Detail", when = "kind=paint", range(0.25, 1), group = "Look")]
    pub detail: f32,
    /// How long paint stays wet; wet paint can bleed, drip and be picked up.
    #[setting(label = "Dry time", when = "kind=paint", range(0, 30), unit = "s", group = "Over time")]
    pub dry_time: f32,
    /// How much wet colours run into each other.
    #[setting(label = "Bleed", when = "kind=paint", range(0, 1), group = "Over time")]
    pub bleed: f32,
    /// How much thick wet paint runs down.
    #[setting(label = "Drips", when = "kind=paint", range(0, 1), group = "Over time")]
    pub drips: f32,
    /// How long marks take to fade away; 0 never fades.
    #[setting(label = "Fade", when = "kind=paint", range(0, 60), unit = "s", group = "Over time")]
    pub fade: f32,
    /// How it mixes with what is under it.
    #[setting(label = "Blend", when = "kind!=fog", group = "Look")]
    pub blend: Blend,
    /// How much of it shows.
    #[setting(label = "Opacity", range(0, 1), group = "Look", keyframe)]
    pub opacity: f32,
    #[setting(label = "Cover look", when = "kind=cover", group = "Look")]
    pub cover_look: CoverLook,
    #[setting(label = "Powder", when = "kind=powder", group = "Look")]
    pub powder_look: PowderLook,
    #[setting(label = "Fog", when = "kind=fog", group = "Look")]
    pub fog_look: FogLook,
    /// How high scraped powder piles up along the track.
    #[setting(label = "Ridges", when = "kind=powder", range(0, 1), group = "Look")]
    pub ridges: f32,
    /// How much the fog blurs what is behind it.
    #[setting(label = "Blur", when = "kind=fog", range(0, 40), unit = "px", group = "Look")]
    pub blur: f32,
    /// How thick the fog looks.
    #[setting(label = "Density", when = "kind=fog", range(0, 1), group = "Look", keyframe)]
    pub density: f32,
    #[setting(label = "Underneath", when = "kind=cover", group = "Underneath")]
    pub under: Underneath,
    #[setting(label = "Colour", when = "kind=cover and under=colour|text", group = "Underneath")]
    pub under_colour: Color,
    #[setting(label = "Colours", when = "kind=cover and under=gradient", group = "Underneath")]
    pub under_colours: Gradient,
    #[setting(label = "Picture", when = "kind=cover and under=picture", unit = "picture", group = "Underneath")]
    pub under_picture: String,
    #[setting(label = "Text", when = "kind=cover and under=text", group = "Underneath")]
    pub under_text: String,
    #[setting(
        label = "Text size",
        when = "kind=cover and under=text",
        range(8, 600),
        unit = "px",
        group = "Underneath"
    )]
    pub under_text_size: f32,
    #[setting(label = "Text colour", when = "kind=cover and under=text", group = "Underneath")]
    pub under_text_colour: Color,
    /// Seconds a scraped place takes to fill back in; 0 never.
    #[setting(label = "Grows back", when = "kind=cover|powder|fog", range(0, 30), unit = "s", group = "Over time")]
    pub grows_back: f32,
    /// How often steam runs down in drops from what is wiped.
    #[setting(label = "Drops", when = "kind=fog and fog_look=steam", range(0, 1), group = "Over time")]
    pub drops: f32,
    /// The colour war's tiles: 0 is smooth ground.
    #[setting(label = "Tile size", when = "kind=paint", range(0, 200), unit = "px", group = "Colour war")]
    pub tile_size: f32,
    /// How fast claimed ground spreads by itself.
    #[setting(label = "Grow", when = "kind=paint", range(0, 1), group = "Colour war")]
    pub grow: f32,
    /// Whether teams can take each other's ground.
    #[setting(label = "Take over", when = "kind=paint", group = "Colour war")]
    pub take_over: bool,
    /// How dark the lines between teams are.
    #[setting(label = "Border", when = "kind=paint", range(0, 1), group = "Colour war")]
    pub border: f32,
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
            cover_look: CoverLook::Matte,
            powder_look: PowderLook::Sand,
            fog_look: FogLook::Frost,
            ridges: 0.6,
            blur: 14.0,
            density: 0.55,
            under: Underneath::Scene,
            under_colour: Color::rgb(0x1b, 0x10, 0x30),
            under_colours: Gradient::even(&[Color::rgb(0xff, 0x5c, 0xa8), Color::rgb(0x3e, 0x9b, 0xff)]),
            under_picture: String::new(),
            under_text: "YOU WIN".to_string(),
            under_text_size: 160.0,
            under_text_colour: Color::rgb(0xff, 0xd2, 0x3e),
            grows_back: 0.0,
            drops: 0.5,
            tile_size: 0.0,
            grow: 0.0,
            take_over: true,
            border: 0.0,
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
        let get = |k: &str| match k {
            "style" => Some(Value::Choice(name.to_string())),
            "kind" => Some(Value::Choice("paint".to_string())),
            _ => None,
        };
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

    fn keys_for(overrides: &[(&str, &str)]) -> Vec<&'static str> {
        let get = |k: &str| {
            let o = overrides.iter().find(|(key, _)| *key == k).map(|(_, v)| v.to_string());
            let default = match k {
                "kind" => "paint",
                "fit" => "arena",
                "style" => "flat",
                "starts_as" => "empty",
                "under" => "scene",
                "fog_look" => "frost",
                _ => return None,
            };
            Some(Value::Choice(o.unwrap_or_else(|| default.to_string())))
        };
        let mut keys: Vec<&'static str> =
            CanvasSettings::defs().iter().filter(|d| d.applies(None, &get)).map(|d| d.key).collect();
        keys.sort_unstable();
        keys
    }

    fn sorted(keys: &[&'static str]) -> Vec<&'static str> {
        let mut v = keys.to_vec();
        v.sort_unstable();
        v
    }

    #[test]
    fn each_kind_shows_its_settings() {
        let paint = "kind fit brushes_from layer starts_as style detail dry_time bleed drips fade blend opacity tile_size grow take_over border";
        assert_eq!(keys_for(&[]), sorted(&paint.split(' ').collect::<Vec<_>>()));
        let cover = "kind fit brushes_from layer start_colour blend opacity cover_look under grows_back";
        assert_eq!(keys_for(&[("kind", "cover")]), sorted(&cover.split(' ').collect::<Vec<_>>()));
        let powder = "kind fit brushes_from layer start_colour light_angle blend opacity powder_look ridges grows_back";
        assert_eq!(keys_for(&[("kind", "powder")]), sorted(&powder.split(' ').collect::<Vec<_>>()));
        let fog = "kind fit brushes_from start_colour opacity fog_look blur density grows_back";
        assert_eq!(keys_for(&[("kind", "fog")]), sorted(&fog.split(' ').collect::<Vec<_>>()));
        let mut steam: Vec<&str> = fog.split(' ').collect();
        steam.push("drops");
        assert_eq!(keys_for(&[("kind", "fog"), ("fog_look", "steam")]), sorted(&steam));
        let mut text: Vec<&str> = cover.split(' ').collect();
        text.extend(["under_colour", "under_text", "under_text_size", "under_text_colour"]);
        assert_eq!(keys_for(&[("kind", "cover"), ("under", "text")]), sorted(&text));
        let mut grad: Vec<&str> = cover.split(' ').collect();
        grad.push("under_colours");
        assert_eq!(keys_for(&[("kind", "cover"), ("under", "gradient")]), sorted(&grad));
        let mut pic: Vec<&str> = cover.split(' ').collect();
        pic.push("under_picture");
        assert_eq!(keys_for(&[("kind", "cover"), ("under", "picture")]), sorted(&pic));
    }

    #[test]
    fn old_canvases_load_as_paint() {
        assert_eq!(CanvasSettings::from_params(&bpe_core::Params::default()), CanvasSettings::default());
    }
}
