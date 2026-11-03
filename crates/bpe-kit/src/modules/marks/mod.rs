//! Brushes: what a thing paints on a canvas as it moves and where it hits.

pub mod sim;

use bpe_core::{Choice, Color, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::{Follow, Paint};

pub use sim::MarksSim;

pub struct MarksBrushesKind;

/// What sets the width of a paint trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum WidthBy {
    /// The same width everywhere.
    Fixed,
    /// The ball's size.
    #[default]
    Size,
    /// The ball's speed: faster is wider.
    Speed,
}

/// The tip of the brush that lays a trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Tip {
    #[default]
    Round,
    Square,
}

/// When a ball paints its trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum When {
    #[default]
    Always,
    /// Only after the ball has hit something.
    AfterFirstHit,
    /// Only while the ball moves faster than a set speed.
    FasterThan,
}

/// What a splat looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SplatShape {
    #[default]
    Splat,
    Ring,
    Star,
    Crack,
    Burst,
    Dots,
    /// A picture file, stamped square.
    Picture,
}

/// Whether a picture splat keeps its own colours or takes the mark's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PictureColours {
    /// The picture as it is.
    #[default]
    Own,
    /// The picture's brightness, painted in the mark's colour.
    Tinted,
}

/// How a splat is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Turn {
    /// Facing the way the hit came from.
    #[default]
    FaceHit,
    Random,
}

/// What a hit's splat lands on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SplatOnto {
    /// The wall or obstacle that was hit, on its own paint skin. A hit with no skin to paint
    /// (a ball, or a thing that takes no paint) lands on the canvases instead.
    #[default]
    ThingHit,
    /// The canvases that take this brush.
    Canvases,
    /// Both the thing hit and the canvases.
    Both,
}

impl SplatOnto {
    /// Whether splats land on the skin of the thing hit.
    pub fn things(self) -> bool {
        self != SplatOnto::Canvases
    }
    /// Whether every splat lands on canvases, not only those with no skin to paint.
    pub fn canvases(self) -> bool {
        self != SplatOnto::ThingHit
    }
}

/// How a splat paints the skin of the thing it hits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SplatSkin {
    /// The splat's shape, printed where the hit was.
    #[default]
    Print,
    /// Colour smeared along the surface on both sides of the hit.
    Spread,
}

/// What a chip knocked out by a hit looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ChipShape {
    #[default]
    Splat,
    Ring,
    Star,
    Crack,
    Burst,
    Dots,
}

/// Who claimed ground belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ClaimTeam {
    /// One team per thing.
    ByThing,
    /// One team per colour.
    #[default]
    ByColour,
}

/// Where the colour of the marks comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MarkColour {
    /// The ball's own colour.
    #[default]
    Ball,
    /// The brush's own Colours.
    Own,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MarksBrushes {
    /// Whether the thing paints a trail behind it.
    #[setting(label = "Paint trail", group = "Paint trail", simple)]
    pub paint_on: bool,
    /// What sets the trail's width.
    #[setting(label = "Width from", when = "paint_on", group = "Paint trail", random)]
    pub paint_width_by: WidthBy,
    /// The trail's width. Size: half the width is this many ball radii. Fixed: the width is this
    /// many times 10 px. Speed: the width in px is this times the speed in px/s, over 100.
    #[setting(label = "Width", when = "paint_on", range(0.1, 4), group = "Paint trail", keyframe, random)]
    pub paint_width: f32,
    /// The shape of the brush tip.
    #[setting(label = "Tip", when = "paint_on", group = "Paint trail", random)]
    pub paint_tip: Tip,
    /// When the trail is painted.
    #[setting(label = "Paint when", when = "paint_on", group = "Paint trail")]
    pub paint_when: When,
    /// The slowest speed that still paints.
    #[setting(
        label = "Faster than",
        when = "paint_when=faster_than",
        range(0, 2000),
        unit = "px/s",
        group = "Paint trail"
    )]
    pub paint_min_speed: f32,
    /// How far the paint lasts, in pixels of travel; 0 never runs out.
    #[setting(label = "Paint load", when = "paint_on", range(0, 5000), unit = "px", group = "Paint trail")]
    pub paint_load: f32,
    /// How much of its length the trail narrows at each end: 0 keeps it blunt, 1 narrows it over 40 px.
    #[setting(label = "Taper", when = "paint_on", range(0, 1), group = "Paint trail")]
    pub paint_taper: f32,
    /// How smoothly a speed-based width changes: 0 follows the speed at once, 1 hardly moves.
    #[setting(label = "Smoothing", when = "paint_width_by=speed", range(0, 1), group = "Paint trail")]
    pub paint_smooth: f32,
    /// How much wet paint the thing picks up as it rolls through it (0 picks up nothing).
    #[setting(label = "Pickup", range(0, 1), group = "Paint trail")]
    pub paint_pickup: f32,
    /// How long picked-up paint takes to wear off.
    #[setting(label = "Pickup fade", when = "paint_pickup", range(0, 30), unit = "s", group = "Paint trail")]
    pub paint_pickup_fade: f32,
    /// Whether hits leave a splat.
    #[setting(label = "Splat", group = "Splat", simple)]
    pub splat_on: bool,
    /// What the splat looks like.
    #[setting(label = "Shape", when = "splat_on", group = "Splat", random)]
    pub splat_shape: SplatShape,
    /// The picture file a Picture splat stamps.
    #[setting(label = "Picture", when = "splat_on and splat_shape=picture", unit = "picture", group = "Splat")]
    pub splat_picture: String,
    /// Whether the picture keeps its colours or is tinted by the mark's.
    #[setting(label = "Picture colours", when = "splat_on and splat_shape=picture", group = "Splat")]
    pub splat_picture_colours: PictureColours,
    /// How big the splat is.
    #[setting(label = "Size", when = "splat_on", range(2, 400), unit = "px", group = "Splat", keyframe, random)]
    pub splat_size: f32,
    /// How much the speed of the hit scales the size: 0 keeps it, 1 follows the speed fully.
    #[setting(label = "Size by speed", when = "splat_on", range(0, 1), group = "Splat", keyframe)]
    pub splat_by_speed: f32,
    /// How the splat is turned.
    #[setting(label = "Turn", when = "splat_on", group = "Splat")]
    pub splat_turn: Turn,
    /// How many different splats of the shape are mixed.
    #[setting(label = "Variants", when = "splat_on", range(1, 8), group = "Splat")]
    pub splat_variants: u32,
    /// The slowest hit that leaves a splat.
    #[setting(label = "Hit speed", when = "splat_on", range(0, 2000), unit = "px/s", group = "Splat")]
    pub splat_min_speed: f32,
    /// Whether the splat goes on the wall or obstacle that was hit, on the canvases, or both.
    #[setting(label = "Lands on", when = "splat_on", group = "Splat")]
    pub splat_onto: SplatOnto,
    /// How a splat paints the thing it hits: its shape printed there, or spread along the surface.
    #[setting(label = "Skin look", when = "splat_on and splat_onto!=canvases", group = "Splat")]
    pub splat_skin: SplatSkin,
    #[setting(label = "Scrape path", group = "Scrape", simple)]
    pub scrape_on: bool,
    #[setting(label = "Width from", when = "scrape_on", group = "Scrape")]
    pub scrape_width_by: WidthBy,
    /// Size: half the width is this many ball radii. Fixed: this many times 10 px. Speed: px = this × speed / 100.
    #[setting(label = "Width", when = "scrape_on", range(0.1, 4), group = "Scrape", keyframe)]
    pub scrape_width: f32,
    #[setting(label = "Chip at hits", group = "Scrape")]
    pub chip_on: bool,
    #[setting(label = "Chip shape", when = "chip_on", group = "Scrape")]
    pub chip_shape: ChipShape,
    #[setting(label = "Chip size", when = "chip_on", range(2, 400), unit = "px", group = "Scrape", keyframe)]
    pub chip_size: f32,
    #[setting(label = "Size by speed", when = "chip_on", range(0, 1), group = "Scrape")]
    pub chip_by_speed: f32,
    #[setting(label = "Hit speed", when = "chip_on", range(0, 2000), unit = "px/s", group = "Scrape")]
    pub chip_min_speed: f32,
    /// How much one pass takes away.
    #[setting(label = "Strength", when = "scrape_on or chip_on", range(0, 1), group = "Scrape")]
    pub scrape_strength: f32,
    #[setting(label = "Claim path", group = "Claim", simple)]
    pub claim_on: bool,
    #[setting(label = "Width from", when = "claim_on", group = "Claim")]
    pub claim_width_by: WidthBy,
    #[setting(label = "Width", when = "claim_on", range(0.1, 4), group = "Claim", keyframe)]
    pub claim_width: f32,
    #[setting(label = "Claim at hits", group = "Claim")]
    pub claim_hits: bool,
    #[setting(label = "Hit size", when = "claim_hits", range(2, 400), unit = "px", group = "Claim")]
    pub claim_hit_size: f32,
    #[setting(label = "Size by speed", when = "claim_hits", range(0, 1), group = "Claim")]
    pub claim_hit_by_speed: f32,
    #[setting(label = "Hit speed", when = "claim_hits", range(0, 2000), unit = "px/s", group = "Claim")]
    pub claim_min_speed: f32,
    #[setting(label = "Team", when = "claim_on or claim_hits", group = "Claim")]
    pub claim_team: ClaimTeam,
    /// The ball's own colour, or the Colours below.
    #[setting(label = "Colour from", group = "Colours", simple, random)]
    pub colour_from: MarkColour,
    /// The marks' own colours, by where, in what order, how fast or when they were made.
    #[setting(
        paint,
        label = "Colours",
        prefix = "colour_",
        group = "Colours",
        when = "colour_from=own",
        follows = "place|order|speed|time|beat",
        simple
    )]
    pub colour: Paint,
}

impl Default for MarksBrushes {
    fn default() -> Self {
        Self {
            paint_on: true,
            paint_width_by: WidthBy::Size,
            paint_width: 1.0,
            paint_tip: Tip::Round,
            paint_when: When::Always,
            paint_min_speed: 100.0,
            paint_load: 0.0,
            paint_taper: 0.3,
            paint_smooth: 0.5,
            paint_pickup: 0.0,
            paint_pickup_fade: 3.0,
            splat_on: false,
            splat_shape: SplatShape::Splat,
            splat_picture: String::new(),
            splat_picture_colours: PictureColours::Own,
            splat_size: 30.0,
            splat_by_speed: 0.5,
            splat_turn: Turn::FaceHit,
            splat_variants: 4,
            splat_min_speed: 50.0,
            splat_onto: SplatOnto::ThingHit,
            splat_skin: SplatSkin::Print,
            scrape_on: false,
            scrape_width_by: WidthBy::Size,
            scrape_width: 1.0,
            chip_on: false,
            chip_shape: ChipShape::Splat,
            chip_size: 30.0,
            chip_by_speed: 0.5,
            chip_min_speed: 80.0,
            scrape_strength: 1.0,
            claim_on: false,
            claim_width_by: WidthBy::Size,
            claim_width: 1.2,
            claim_hits: false,
            claim_hit_size: 40.0,
            claim_hit_by_speed: 0.5,
            claim_min_speed: 50.0,
            claim_team: ClaimTeam::ByColour,
            colour_from: MarkColour::Ball,
            colour: Paint { follow: Follow::Place, ..Paint::solid(Color::WHITE) },
        }
    }
}

impl ModuleKind for MarksBrushesKind {
    fn id(&self) -> &'static str {
        "marks.brushes"
    }
    fn name(&self) -> &'static str {
        "Brushes"
    }
    fn help(&self) -> &'static str {
        "Paints a trail behind the thing, on the canvases that ask for it, and stamps a splat where it hits, on the thing it hits or on the canvases."
    }
    fn slot(&self) -> SlotId {
        SlotId::Marks
    }
    fn settings(&self) -> &'static [SettingDef] {
        MarksBrushes::defs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Choice;

    #[test]
    fn pickup_and_picture_settings_have_their_defaults() {
        let d = MarksBrushes::default();
        assert_eq!((d.paint_pickup, d.paint_pickup_fade), (0.0, 3.0));
        assert_eq!((d.splat_picture.as_str(), d.splat_picture_colours), ("", PictureColours::Own));
        let defs = MarksBrushes::defs();
        assert_eq!(defs.iter().find(|d| d.key == "paint_pickup").unwrap().when, "");
        assert_eq!(defs.iter().find(|d| d.key == "splat_picture").unwrap().unit, "picture");
        assert_eq!(SplatShape::NAMES.last(), Some(&"picture"));
    }

    #[test]
    fn brush_defaults_round_trip() {
        let d = MarksBrushes::default();
        assert_eq!(MarksBrushes::from_params(&d.to_params()), d);
        assert!(!d.scrape_on && !d.chip_on && !d.claim_on && !d.claim_hits);
        assert_eq!(MarksBrushes::from_params(&bpe_core::Params::default()), d);
    }
}
