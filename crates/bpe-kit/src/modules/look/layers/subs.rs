//! The settings of the sub-layer kinds that are new with sub-layers: Glow, Outline, Shadow,
//! Recolour, Flash on hit and Colour on hit. A sub-layer belongs to one layer (its
//! `Layer::parent`) and changes only that layer: the layer and its sub-layers, worked top to
//! bottom, make one piece that mixes into the stack where the layer sits. Cut, Shading, Shine and
//! Shader sub-layers reuse the settings of the layers of the same name (without Blend: a piece
//! mixes in by its layer's Blend).

use std::sync::OnceLock;

use bpe_core::{Choice, Color, Gradient, SettingDef, Settings};

use super::OutlineSits;

/// Where a sub-layer Glow shines, against the piece's edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GlowWhere {
    /// Round the outside of the piece.
    #[default]
    Outside,
    /// Inside the piece, in from its edge.
    Inside,
    /// Both sides of the edge.
    Both,
}

impl GlowWhere {
    /// Its number in sub_layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Outside => 0.0,
            Self::Inside => 1.0,
            Self::Both => 2.0,
        }
    }
}

/// Where a sub-layer's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SubColourFrom {
    /// The layer's own colour where it is (an outline takes it a little darker, so it shows).
    #[default]
    SameAsLayer,
    Own,
}

/// Glow: soft light round the piece's current shape.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubGlow {
    /// How brightly it shines.
    #[setting(label = "Strength", range(0, 2), simple, keyframe, pulse)]
    pub strength: f32,
    /// How far the light reaches from the edge, in ball radii.
    #[setting(label = "Size", range(0.02, 4), simple, keyframe, pulse)]
    pub size: f32,
    /// Outside the piece, inside it, or both.
    #[setting(label = "Where", simple)]
    pub side: GlowWhere,
    /// Same as the layer glows in the layer's own colour.
    #[setting(label = "Colour from", simple)]
    pub colour_from: SubColourFrom,
    /// Its own colour, or colours.
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// 0 is invisible, 1 is full.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
}

impl Default for SubGlow {
    fn default() -> Self {
        Self {
            strength: 0.8,
            size: 0.4,
            side: GlowWhere::Outside,
            colour_from: SubColourFrom::SameAsLayer,
            colour: Gradient::solid(Color::WHITE),
            opacity: 1.0,
        }
    }
}

/// Outline: a band along the piece's current edge, painted over it.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubOutline {
    /// How wide the band is.
    #[setting(label = "Width", range(0, 400), span(0, 60), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// Inside the piece's edge, centred on it, or outside it.
    #[setting(label = "Sits", simple)]
    pub sits: OutlineSits,
    /// Same as the layer takes the layer's colour a little darker, so it shows.
    #[setting(label = "Colour from", simple)]
    pub colour_from: SubColourFrom,
    /// Its own colour, or colours.
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
}

impl Default for SubOutline {
    fn default() -> Self {
        Self {
            width: 4.0,
            sits: OutlineSits::Outside,
            colour_from: SubColourFrom::SameAsLayer,
            colour: Gradient::solid(Color::WHITE),
            opacity: 1.0,
        }
    }
}

/// What a shadow's offset stays with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShadowMoves {
    /// The screen: the shadow falls the same way however the ball turns.
    #[default]
    Screen,
    /// The ball: the shadow turns as the ball turns.
    Ball,
}

impl ShadowMoves {
    /// Its number in sub_layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Screen => 0.0,
            Self::Ball => 1.0,
        }
    }
}

/// Shadow: a soft copy of the piece's shape, offset and under it.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubShadow {
    /// The way the shadow falls, in degrees: 0 to the right, 90 straight down.
    #[setting(label = "Angle", span(-180, 180), unit = "°", simple, keyframe, pulse)]
    pub angle: f32,
    /// How far it falls, in ball radii.
    #[setting(label = "Distance", range(0, 2), simple, keyframe, pulse)]
    pub distance: f32,
    /// How soft its edge is, in ball radii.
    #[setting(label = "Softness", range(0, 1), keyframe, pulse)]
    pub softness: f32,
    /// Its colour.
    #[setting(label = "Colour", simple, keyframe)]
    pub colour: Color,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
    /// Whether it falls the same way on the screen or turns with the ball.
    #[setting(label = "Moves with")]
    pub moves: ShadowMoves,
}

/// Where a new Shadow falls: down and to the right, as from the light of a new Shine.
pub const SHADOW_ANGLE: f32 = 45.0;

impl Default for SubShadow {
    fn default() -> Self {
        Self {
            angle: SHADOW_ANGLE,
            distance: 0.15,
            softness: 0.3,
            colour: Color::rgb(0, 0, 0),
            opacity: 0.6,
            moves: ShadowMoves::Screen,
        }
    }
}

/// How a Recolour sub-layer changes the piece's colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RecolourMode {
    /// Towards one colour.
    #[default]
    Tint,
    /// Round the colour wheel.
    Hue,
    /// Each colour replaced by a gradient's, by how light it is.
    Gradient,
    /// Brightness, contrast and saturation.
    Light,
}

impl RecolourMode {
    /// Its number in sub_layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Tint => 0.0,
            Self::Hue => 1.0,
            Self::Gradient => 2.0,
            Self::Light => 3.0,
        }
    }
}

/// Recolour: changes the piece's colours only.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubRecolour {
    /// How it changes the colours.
    #[setting(label = "Mode", simple)]
    pub mode: RecolourMode,
    /// The colour it tints towards.
    #[setting(label = "Colour", when = "mode=tint", simple, keyframe)]
    pub tint: Color,
    /// How far round the colour wheel it turns the colours.
    #[setting(label = "Turn", when = "mode=hue", span(-180, 180), unit = "°", simple, keyframe, pulse)]
    pub turn: f32,
    /// The colours that take the place of the piece's, darkest first.
    #[setting(label = "Colours", when = "mode=gradient", simple, keyframe)]
    pub colours: Gradient,
    /// How much of the new colour it takes.
    #[setting(label = "Amount", range(0, 1), when = "mode=tint|gradient", simple, keyframe, pulse)]
    pub amount: f32,
    /// Lighter or darker.
    #[setting(label = "Brightness", range(-1, 1), when = "mode=light", keyframe, pulse)]
    pub brightness: f32,
    /// More or less difference between light and dark.
    #[setting(label = "Contrast", range(0, 2), when = "mode=light", keyframe, pulse)]
    pub contrast: f32,
    /// More or less colourful: 0 is grey.
    #[setting(label = "Saturation", range(0, 2), when = "mode=light", keyframe, pulse)]
    pub saturation: f32,
    /// 0 leaves the colours as they were, 1 changes them fully.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
}

impl Default for SubRecolour {
    fn default() -> Self {
        let colours = Gradient::even(&[Color::rgb(0x1b, 0x1f, 0x6e), Color::rgb(0xff, 0xb3, 0x47)]);
        Self {
            mode: RecolourMode::Tint,
            tint: Color::rgb(0xff, 0x6b, 0x9d),
            turn: 60.0,
            colours,
            amount: 0.5,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            opacity: 1.0,
        }
    }
}

/// Flash on hit: the piece lights up when its ball (or shape) is hit.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubFlash {
    /// How bright the flash is.
    #[setting(label = "Strength", range(0, 3), simple, keyframe, pulse)]
    pub strength: f32,
    /// How long it takes to fade.
    #[setting(label = "Length", range(0.02, 2), unit = "s", simple, keyframe)]
    pub length: f32,
    /// The flash's colour.
    #[setting(label = "Colour", simple, keyframe)]
    pub colour: Color,
    /// Harder hits flash brighter.
    #[setting(label = "Follow impact")]
    pub follow_impact: bool,
}

impl Default for SubFlash {
    fn default() -> Self {
        Self { strength: 0.8, length: 0.3, colour: Color::WHITE, follow_impact: true }
    }
}

/// Colour on hit: the piece's colours turn round the wheel on every hit.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SubHue {
    /// How far round the colour wheel each hit turns the colours.
    #[setting(label = "Turn per hit", range(-180, 180), unit = "°", simple, keyframe)]
    pub turn_per_hit: f32,
}

impl Default for SubHue {
    fn default() -> Self {
        Self { turn_per_hit: 30.0 }
    }
}

/// `defs` without its Blend setting: a sub-layer mixes into its piece, which mixes into the stack
/// by its layer's Blend. `made` keeps the list for the program's life.
pub fn without_blend(made: &'static OnceLock<Vec<SettingDef>>, defs: &'static [SettingDef]) -> &'static [SettingDef] {
    made.get_or_init(|| defs.iter().filter(|d| d.key != "blend").cloned().collect())
}

/// A Shading sub-layer's settings: a Shading layer's, without Blend.
pub fn shading_defs() -> &'static [SettingDef] {
    static MADE: OnceLock<Vec<SettingDef>> = OnceLock::new();
    without_blend(&MADE, super::ShadingLayer::defs())
}

/// A Shine sub-layer's settings: a Shine layer's, without Blend.
pub fn shine_defs() -> &'static [SettingDef] {
    static MADE: OnceLock<Vec<SettingDef>> = OnceLock::new();
    without_blend(&MADE, super::ShineLayer::defs())
}

/// A Shader sub-layer's settings: a Shader layer's, without Blend.
pub fn shader_defs() -> &'static [SettingDef] {
    static MADE: OnceLock<Vec<SettingDef>> = OnceLock::new();
    without_blend(&MADE, super::shader::defs())
}

/// The help a Shader sub-layer shows.
pub const SHADER_HELP: &str = "An effect painted over its layer. It sees only its layer and the sub-layers above it.";
