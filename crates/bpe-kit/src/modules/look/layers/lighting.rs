//! How a layer takes the scene's light (lighting spec, part 1a): its **Lit** (0 keeps its own
//! colour whatever the lamps do, 1 takes their light whole) and whether it **Blocks light** (casts
//! shadows). The settings sit in each drawn and around layer's "Lighting" fold; they change
//! nothing until the scene has a Lighting card. A ball's Bulge is its Look's Surface section's
//! (part 2a, `look::surface`).

use bpe_core::{Choice, Params, SettingDef, Settings, Value};

use super::{BODY, GLOW, OUTLINE, PATTERN, SHADER, SHADING, SHINE};

/// The settings' keys.
pub const LIT: &str = "lit";
pub const BLOCKS_LIGHT: &str = "blocks_light";
pub const UNDER_UV: &str = "under_uv";
pub const UV_GLOW: &str = "uv_glow";
pub const CATCHES_BEAMS: &str = "catches_beams";
pub const BENDS_LIGHT: &str = "bends_light";
pub const MAKES_RAINBOWS: &str = "makes_rainbows";

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
    /// How much it shows the Lighting card's Rainbow where the light falls on it, like rain in
    /// sunshine (glass and light part 4b).
    #[setting(label = "Makes rainbows", span(0, 1), keyframe, group = "Lighting")]
    pub makes_rainbows: f32,
}

impl Default for AroundLit {
    fn default() -> Self {
        Self {
            lit: 1.0,
            blocks_light: false,
            catches_beams: 0.0,
            under_uv: UnderUv::Normal,
            uv_glow: 1.5,
            makes_rainbows: 0.0,
        }
    }
}

/// A particles layer's Lighting (glass and light part 3): an around layer's, and how much each
/// particle bends lamp light passing through it, like a bubble or a glass bead.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ParticlesLit {
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
    /// How much each particle bends lamp light passing through it (its index of refraction), so
    /// bubbles and beads focus light into bright lines: 1 lets it through straight. Needs the
    /// Lighting card's Glass caustics, and works best on particles that don't block light.
    #[setting(label = "Bends light", span(1, 2), keyframe, group = "Lighting")]
    pub bends_light: f32,
    /// How much it shows the Lighting card's Rainbow where the light falls on it, like rain in
    /// sunshine (glass and light part 4b).
    #[setting(label = "Makes rainbows", span(0, 1), keyframe, group = "Lighting")]
    pub makes_rainbows: f32,
}

impl Default for ParticlesLit {
    fn default() -> Self {
        Self {
            lit: 1.0,
            blocks_light: false,
            catches_beams: 0.0,
            under_uv: UnderUv::Normal,
            uv_glow: 1.5,
            bends_light: 1.0,
            makes_rainbows: 0.0,
        }
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

/// A ball's or a shape's body: lit and casting shadows.
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

/// The Lighting settings a layer kind adds, by its id: none for Blend, the reactions, the
/// sub-layers and the group layers.
pub fn defs_of(kind: &str) -> &'static [SettingDef] {
    use crate::modules::look::shape;
    match kind {
        BODY | shape::BODY => Solid::defs(),
        PATTERN | OUTLINE | SHADING | SHADER | shape::PAINT | shape::OUTLINE | shape::SHADER => Lit::defs(),
        super::trail::TRAIL => AroundLit::defs(),
        super::particles::PARTICLES | super::particles::SHAPE_PARTICLES => ParticlesLit::defs(),
        super::wavefront::WAVEFRONT => AroundLit::defs(),
        GLOW | SHINE | shape::GLOW => Unlit::defs(),
        _ => &[],
    }
}

/// A layer's Lighting as the renderer reads it ([`bpe_gfx::LayerGpu::light`]): Lit, Blocks light
/// (0 or 1), then 0 and 0 (the draw fills in the body's Bulge and the thing's surface index). A
/// layer without the settings is neither lit nor blocking. A particles layer's Bends light
/// (glass and light part 3) rides in `[2]` (0 when it doesn't bend), which its sprites carry
/// (`Particle::light[2]`, [`super::around::Drew::stamp`]).
pub fn of(p: &Params) -> [f32; 4] {
    let number = |key| match p.get(key) {
        Some(Value::Number(v)) => *v as f32,
        Some(Value::Int(v)) => *v as f32,
        Some(Value::Bool(b)) => f32::from(u8::from(*b)),
        _ => 0.0,
    };
    let bends = number(BENDS_LIGHT);
    let bends = if bends.is_finite() && bends > bpe_gfx::BENDS_FROM { bends } else { 0.0 };
    [number(LIT), number(BLOCKS_LIGHT), bends, 0.0]
}

/// A layer's part 4 lighting as the renderer reads it ([`bpe_gfx::LayerGpu::more`], a trail's or
/// particle's `extra`): UV glow (0 for Normal), Only under UV (0 or 1), Catches beams (with Makes
/// rainbows packed in by [`bpe_gfx::pack_tags`], glass and light part 4b) and `link` (its thing's
/// link number for Lights up). Only under UV hides a layer only in a `lit` scene (one
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
    let makes = match p.get(MAKES_RAINBOWS) {
        Some(Value::Number(v)) if v.is_finite() => *v as f32,
        _ => 0.0,
    };
    let catches = bpe_gfx::pack_tags(catches, makes);
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
        assert_eq!(light("layer.body"), Some([1.0, 1.0, 0.0, 0.0]));
        assert_eq!(light("shape.body"), Some([1.0, 1.0, 0.0, 0.0]));
        assert_eq!(light("layer.glow"), Some([0.0; 4]));
        assert_eq!(light("react.flash"), Some([0.0; 4]), "reactions have no Lighting");
    }

    #[test]
    fn a_particles_layer_carries_how_much_it_bends_light() {
        assert!(defs_of(super::super::particles::PARTICLES).iter().any(|d| d.key == BENDS_LIGHT));
        assert!(!defs_of(super::super::trail::TRAIL).iter().any(|d| d.key == BENDS_LIGHT), "trails don't bend");
        let mut p = Params::default();
        p.set(BENDS_LIGHT, Value::Number(1.33));
        assert!((of(&p)[2] - 1.33).abs() < 1e-6);
        p.set(BENDS_LIGHT, Value::Number(1.0));
        assert_eq!(of(&p)[2], 0.0, "1 lets light through straight");
    }

    #[test]
    fn a_particles_things_sprites_can_bend_light_but_its_smoke_cant() {
        use crate::things::particles::look::{SMOKE, SPRITES};
        let reg = crate::kit().registry;
        let offers = |kind: &str| reg.layer(kind).unwrap().light_defs().iter().any(|d| d.key == BENDS_LIGHT);
        assert!(offers(SPRITES));
        assert!(!offers(SMOKE));
    }

    #[test]
    fn a_glass_liquid_carries_how_much_it_bends_lamp_light() {
        use crate::things::particles::liquid::{LiquidLayer, Surface};
        let glass = LiquidLayer { surface: Surface::Glass, ..LiquidLayer::default() };
        assert!((glass.style(1.0).c[2] - 1.33).abs() < 1e-6);
        let plastic = LiquidLayer { surface: Surface::Plastic, ..LiquidLayer::default() };
        assert_eq!(plastic.style(1.0).c[2], 0.0);
    }
}
