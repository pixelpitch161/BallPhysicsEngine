//! The picture's own looks: effect calls, the background paint and the grade.

use super::*;

/// When a shader effect runs: on the things before the flashes and particles are drawn over
/// them, or on the whole picture after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Things,
    Picture,
}

/// One run of a shader effect over the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectCall {
    /// Tells this call's history (for effects that read their previous frame) apart from any
    /// other's, frame after frame.
    pub key: u64,
    /// The effect file: a header and `fn effect(uv, p)`, see `effects`.
    pub wgsl: Cow<'static, str>,
    /// The effect's params packed in header order, colours taking three.
    pub params: [f32; 16],
    /// How much of the effect shows (its Mix): 1 all of it, 0 none (skipped); past 1 pushes it
    /// further, below 0 the other way.
    pub blend: f32,
    /// 0 for the whole picture, else the [`thing_code`] of the one thing it paints.
    pub apply_to: f32,
    pub stage: Stage,
    /// How the effect's result mixes onto what it took (Normal replaces it).
    pub mode: Blend,
    /// What of the picture the effect works on (read at the Picture stage on the main picture).
    pub takes: Takes,
    /// For [`Takes::Glow`]: the brightness above which the picture counts as glow.
    pub glow_above: f32,
    /// How many seconds of earlier frames the effect remembers (0 for none), for the editor's
    /// warm-up after a seek.
    pub memory: f32,
}

impl EffectCall {
    /// A call of `wgsl` over the whole picture at `stage`, fully mixed in, Normal, taking the
    /// whole picture, its params all 0.
    pub fn new(key: u64, wgsl: impl Into<Cow<'static, str>>, stage: Stage) -> Self {
        Self {
            key,
            wgsl: wgsl.into(),
            params: [0.0; 16],
            blend: 1.0,
            apply_to: 0.0,
            stage,
            mode: Blend::Normal,
            takes: Takes::Whole,
            glow_above: 0.9,
            memory: 0.0,
        }
    }
}

/// What part of the picture an effect works on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Takes {
    /// The whole picture.
    #[default]
    Whole,
    /// The light that falls on lit things (needs a Lighting card; else the effect changes
    /// nothing).
    Light,
    /// Only what is brighter than the call's `glow_above`.
    Glow,
}

impl Takes {
    /// The shader's code: 0 whole, 1 light, 2 glow.
    pub fn code(self) -> f32 {
        match self {
            Takes::Whole => 0.0,
            Takes::Light => 1.0,
            Takes::Glow => 2.0,
        }
    }
}

/// How the background pass paints: one gradient of the frame's atlas, its place measured on the
/// screen or in the world (see `Frame::background_on_screen`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundPaint {
    /// The gradient's atlas row; `u32::MAX` paints black.
    pub row: u32,
    /// How the place runs: the direction in degrees, the span, and the centre xy.
    pub place: [f32; 4],
    /// Where along the gradient the paint starts, 0 to 1 (a Time or Beat paint gives its whole
    /// place here).
    pub t0: f32,
    /// Gradients per second the place slides along.
    pub scroll: f32,
    /// 0 clamp, 1 repeat, 2 mirror.
    pub repeat: f32,
    /// 0 paints the colour at `t0` everywhere, 1 works the place out along the direction, 2
    /// outward from the centre, 3 by the form `pattern`.
    pub mode: f32,
    /// The gradient's grain, in 1/255 steps (see [`crate::PaintGpu::grain`]).
    pub grain: f32,
    /// A plain two-colour blend (linear light) the shader mixes itself, exactly, instead of
    /// reading the baked row, whose half floats are a hair off for very dark colours.
    pub ends: Option<[[f32; 4]; 2]>,
    /// With `mode` 3, the form laid over the place (`PaintGpu::pattern`): the place measured from
    /// the centre in spans, turned by the direction.
    pub pattern: f32,
    /// The form's own numbers (`PaintGpu::form`).
    pub form: [f32; 4],
}

impl Default for BackgroundPaint {
    fn default() -> Self {
        Self {
            row: u32::MAX,
            place: [90.0, 1920.0, 0.0, 0.0],
            t0: 0.0,
            scroll: 0.0,
            repeat: 0.0,
            mode: 0.0,
            grain: 0.0,
            ends: None,
            pattern: 0.0,
            form: [0.0; 4],
        }
    }
}

/// The scene's colour grade, run in screen space on the finished, display-ready picture (after
/// the effects, bloom and tonemapping). At its defaults it changes nothing: the composite skips
/// it altogether.
#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    /// -1 to 1: the colour is multiplied by 1 plus this.
    pub brightness: f32,
    /// -1 to 1: spreads the colours from (or pulls them into) the middle grey.
    pub contrast: f32,
    /// -1 (grey) to 1 (twice as colourful).
    pub saturation: f32,
    /// -1 (cool, bluer) to 1 (warm, redder).
    pub warmth: f32,
    /// -1 (greener) to 1 (magenta).
    pub tint: f32,
    /// 0 to 1: how dark the corners go.
    pub vignette: f32,
    /// 0 to 1: how far from the middle the darkening starts.
    pub vignette_size: f32,
    /// The colours the picture's brightness is mapped to, dark to light.
    pub map: bpe_core::Gradient,
    /// 0 to 1: how much of the mapped colour replaces the picture's own.
    pub map_amount: f32,
}

impl Default for Grade {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            warmth: 0.0,
            tint: 0.0,
            vignette: 0.0,
            vignette_size: 0.5,
            map: bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 0), bpe_core::Color::WHITE]),
            map_amount: 0.0,
        }
    }
}

impl Grade {
    /// Whether the grade changes nothing: every value at its default. The map only counts once
    /// its amount is above 0, and the vignette's size once the vignette is above 0.
    pub fn is_default(&self) -> bool {
        let d = Self::default();
        self.brightness == d.brightness
            && self.contrast == d.contrast
            && self.saturation == d.saturation
            && self.warmth == d.warmth
            && self.tint == d.tint
            && self.vignette == d.vignette
            && (self.vignette == 0.0 || self.vignette_size == d.vignette_size)
            && self.map_amount == d.map_amount
    }
}
