//! How a layer takes the scene's light (lighting spec, part 1a): its **Lit** (0 keeps its own
//! colour whatever the lamps do, 1 takes their light whole), whether it **Blocks light** (casts
//! shadows) and, on a ball's Body, its **Bulge** (how much the ball bulges to the light). The
//! settings sit in each drawn and around layer's "Lighting" fold; they change nothing until the
//! scene has a Lighting card.

use bpe_core::{Choice, Params, SettingDef, Settings, Value};

use super::{BODY, GLOW, OUTLINE, PATTERN, SHADER, SHADING, SHINE};

/// The settings' keys.
pub const LIT: &str = "lit";
pub const BLOCKS_LIGHT: &str = "blocks_light";
pub const BULGE: &str = "bulge";
pub const UNDER_UV: &str = "under_uv";
pub const UV_GLOW: &str = "uv_glow";
pub const CATCHES_BEAMS: &str = "catches_beams";

/// How a layer looks under ultraviolet light (lighting part 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum UnderUv {
    /// As it always looks; UV light does nothing to it.
    #[default]
    Normal,
    /// Glows where UV light falls on it.
    Glows,
    /// Hidden until UV light falls on it, then glows.
    OnlyUnderUv,
}

/// A trail's, particles' or wavefront's Lighting: lit, not blocking, and catching the beams of
/// the lights like smoke.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct AroundLit {
    /// How much the scene's light changes this layer: 0 keeps its own colour, 1 takes the light
    /// whole, more takes it harder.
    #[setting(label = "Lit", span(0, 1), keyframe, group = "Lighting")]
    pub lit: f32,
    /// Whether this layer casts shadows.
    #[setting(label = "Blocks light", group = "Lighting")]
    pub blocks_light: bool,
    /// How much it catches the light passing through it like smoke, glowing where a beam crosses it.
    #[setting(label = "Catches beams", span(0, 2), keyframe, group = "Lighting")]
    pub catches_beams: f32,
    /// How the layer looks under ultraviolet light: Normal, Glows (bright in UV light) or Only
    /// under UV (hidden until UV light falls on it).
    #[setting(label = "Under UV", group = "Lighting")]
    pub under_uv: UnderUv,
    /// How brightly the layer glows where UV light falls on it.
    #[setting(label = "UV glow", when = "under_uv!=normal", span(0, 4), keyframe, group = "Lighting")]
    pub uv_glow: f32,
}

impl Default for AroundLit {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: false, catches_beams: 0.0, under_uv: UnderUv::Normal, uv_glow: 1.5 }
    }
}

/// Lit by the lamps, not blocking them.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Lit {
    /// How much the scene's light changes this layer: 0 keeps its own colour, 1 takes the light
    /// whole, more takes it harder.
    #[setting(label = "Lit", span(0, 1), keyframe, group = "Lighting")]
    pub lit: f32,
    /// Whether this layer casts shadows.
    #[setting(label = "Blocks light", group = "Lighting")]
    pub blocks_light: bool,
    /// How the layer looks under ultraviolet light: Normal, Glows (bright in UV light) or Only
    /// under UV (hidden until UV light falls on it).
    #[setting(label = "Under UV", group = "Lighting")]
    pub under_uv: UnderUv,
    /// How brightly the layer glows where UV light falls on it.
    #[setting(label = "UV glow", when = "under_uv!=normal", span(0, 4), keyframe, group = "Lighting")]
    pub uv_glow: f32,
}

impl Default for Lit {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: false, under_uv: UnderUv::Normal, uv_glow: 1.5 }
    }
}

/// A light of its own (a glow, a shine): the lamps leave it as it is.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Unlit {
    /// How much the scene's light changes this layer: 0 keeps its own colour, 1 takes the light
    /// whole, more takes it harder.
    #[setting(label = "Lit", span(0, 1), keyframe, group = "Lighting")]
    pub lit: f32,
    /// Whether this layer casts shadows.
    #[setting(label = "Blocks light", group = "Lighting")]
    pub blocks_light: bool,
    /// How the layer looks under ultraviolet light: Normal, Glows (bright in UV light) or Only
    /// under UV (hidden until UV light falls on it).
    #[setting(label = "Under UV", group = "Lighting")]
    pub under_uv: UnderUv,
    /// How brightly the layer glows where UV light falls on it.
    #[setting(label = "UV glow", when = "under_uv!=normal", span(0, 4), keyframe, group = "Lighting")]
    pub uv_glow: f32,
}

impl Default for Unlit {
    fn default() -> Self {
        Self { lit: 0.0, blocks_light: false, under_uv: UnderUv::Normal, uv_glow: 1.5 }
    }
}

/// A shape's body: lit and casting shadows.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Solid {
    /// How much the scene's light changes this layer: 0 keeps its own colour, 1 takes the light
    /// whole, more takes it harder.
    #[setting(label = "Lit", span(0, 1), keyframe, group = "Lighting")]
    pub lit: f32,
    /// Whether this layer casts shadows.
    #[setting(label = "Blocks light", group = "Lighting")]
    pub blocks_light: bool,
    /// How the layer looks under ultraviolet light: Normal, Glows (bright in UV light) or Only
    /// under UV (hidden until UV light falls on it).
    #[setting(label = "Under UV", group = "Lighting")]
    pub under_uv: UnderUv,
    /// How brightly the layer glows where UV light falls on it.
    #[setting(label = "UV glow", when = "under_uv!=normal", span(0, 4), keyframe, group = "Lighting")]
    pub uv_glow: f32,
}

impl Default for Solid {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: true, under_uv: UnderUv::Normal, uv_glow: 1.5 }
    }
}

/// A ball's Body: lit, casting shadows, and round.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BodyLit {
    /// How much the scene's light changes this layer: 0 keeps its own colour, 1 takes the light
    /// whole, more takes it harder.
    #[setting(label = "Lit", span(0, 1), keyframe, group = "Lighting")]
    pub lit: f32,
    /// Whether this layer casts shadows.
    #[setting(label = "Blocks light", group = "Lighting")]
    pub blocks_light: bool,
    /// How much the ball bulges to the light: 0 is a flat disc, 1 a ball, more a taller dome.
    #[setting(label = "Bulge", span(0, 2), keyframe, group = "Lighting")]
    pub bulge: f32,
    /// How the layer looks under ultraviolet light: Normal, Glows (bright in UV light) or Only
    /// under UV (hidden until UV light falls on it).
    #[setting(label = "Under UV", group = "Lighting")]
    pub under_uv: UnderUv,
    /// How brightly the layer glows where UV light falls on it.
    #[setting(label = "UV glow", when = "under_uv!=normal", span(0, 4), keyframe, group = "Lighting")]
    pub uv_glow: f32,
}

impl Default for BodyLit {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: true, bulge: 1.0, under_uv: UnderUv::Normal, uv_glow: 1.5 }
    }
}

/// The Lighting settings a layer kind adds, by its id: none for Blend, the reactions, the
/// sub-layers and the group layers.
pub fn defs_of(kind: &str) -> &'static [SettingDef] {
    use crate::modules::look::shape;
    match kind {
        BODY => BodyLit::defs(),
        shape::BODY => Solid::defs(),
        PATTERN | OUTLINE | SHADING | SHADER | shape::PAINT | shape::OUTLINE | shape::SHADER => Lit::defs(),
        super::trail::TRAIL | super::particles::PARTICLES | super::particles::SHAPE_PARTICLES => AroundLit::defs(),
        super::wavefront::WAVEFRONT => AroundLit::defs(),
        GLOW | SHINE | shape::GLOW => Unlit::defs(),
        _ => &[],
    }
}

/// A layer's Lighting as the renderer reads it ([`bpe_gfx::LayerGpu::light`]): Lit, Blocks light
/// (0 or 1), Bulge and 0. A layer without the settings is neither lit nor blocking.
pub fn of(p: &Params) -> [f32; 4] {
    let number = |key| match p.get(key) {
        Some(Value::Number(v)) => *v as f32,
        Some(Value::Int(v)) => *v as f32,
        Some(Value::Bool(b)) => f32::from(u8::from(*b)),
        _ => 0.0,
    };
    [number(LIT), number(BLOCKS_LIGHT), number(BULGE), 0.0]
}

/// A layer's part 4 lighting as the renderer reads it ([`bpe_gfx::LayerGpu::more`], a trail's or
/// particle's `extra`): UV glow (0 for Normal), Only under UV (0 or 1), Catches beams and `link`
/// (its thing's link number for Lights up). Only under UV hides a layer only in a `lit` scene (one
/// with a Lighting card), so without the card the layer shows as it always did.
pub fn extra_of(p: &Params, link: f32, lit: bool) -> [f32; 4] {
    let mode = match p.get(UNDER_UV) {
        Some(Value::Choice(c)) => c.as_str(),
        _ => "normal",
    };
    let glow = match p.get(UV_GLOW) {
        Some(Value::Number(v)) if v.is_finite() => (*v as f32).max(0.0),
        _ => 0.0,
    };
    let catches = match p.get(CATCHES_BEAMS) {
        Some(Value::Number(v)) if v.is_finite() => (*v as f32).max(0.0),
        _ => 0.0,
    };
    let (uv, only) = match mode {
        "glows" => (glow, 0.0),
        "only_under_uv" => (glow, if lit { 1.0 } else { 0.0 }),
        _ => (0.0, 0.0),
    };
    [uv, only, catches, link]
}

/// Gives a thing's records its link number, or, in a scene without a Lighting card (`lit` false),
/// takes their UV away so they draw exactly as before.
/// `light` is the frame's lighting, which learns whether anything glows under UV.
pub fn stamp(records: &mut [bpe_gfx::LayerGpu], link: f32, light: Option<&mut bpe_gfx::LightScene>) {
    let Some(light) = light else {
        records.iter_mut().for_each(|r| r.more = [0.0; 4]);
        return;
    };
    for r in records {
        r.more[3] = link;
        light.uv |= r.more[0] > 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balls_and_shapes_block_light_and_their_glows_are_unlit() {
        let reg = crate::kit().registry;
        let scene = crate::scenes::lit_room(&reg);
        let resolved = bpe_core::resolve(&scene, &reg);
        let mut seen = Vec::new();
        for t in resolved.things.values() {
            if let Some(look) = t.slots.get(&bpe_core::SlotId::Look) {
                for l in &look.layers {
                    seen.push((l.kind.as_str().to_owned(), of(&look.layer_params(l.id))));
                }
            }
        }
        let light = |kind: &str| seen.iter().find(|(k, _)| k == kind).map(|(_, l)| *l);
        assert_eq!(light("layer.body"), Some([1.0, 1.0, 1.0, 0.0]));
        assert_eq!(light("shape.body"), Some([1.0, 1.0, 0.0, 0.0]));
        assert_eq!(light("layer.glow"), Some([0.0; 4]));
        assert_eq!(light("react.flash"), Some([0.0; 4]), "reactions have no Lighting");
    }
}
