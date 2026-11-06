//! How a layer takes the scene's light (lighting spec, part 1a): its **Lit** (0 keeps its own
//! colour whatever the lamps do, 1 takes their light whole) and whether it **Blocks light** (casts
//! shadows). The settings sit in each drawn and around layer's "Lighting" fold; they change
//! nothing until the scene has a Lighting card. A ball's Bulge is its Look's Surface section's
//! (part 2a, `look::surface`).

use bpe_core::{Params, SettingDef, Settings, Value};

use super::{BODY, GLOW, OUTLINE, PATTERN, SHADER, SHADING, SHINE};

/// The settings' keys.
pub const LIT: &str = "lit";
pub const BLOCKS_LIGHT: &str = "blocks_light";

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
}

impl Default for Lit {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: false }
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
}

impl Default for Unlit {
    fn default() -> Self {
        Self { lit: 0.0, blocks_light: false }
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
}

impl Default for Solid {
    fn default() -> Self {
        Self { lit: 1.0, blocks_light: true }
    }
}

/// The Lighting settings a layer kind adds, by its id: none for Blend, the reactions, the
/// sub-layers and the group layers.
pub fn defs_of(kind: &str) -> &'static [SettingDef] {
    use crate::modules::look::shape;
    match kind {
        BODY | shape::BODY => Solid::defs(),
        PATTERN | OUTLINE | SHADING | SHADER | shape::PAINT | shape::OUTLINE | shape::SHADER => Lit::defs(),
        super::trail::TRAIL | super::particles::PARTICLES | super::particles::SHAPE_PARTICLES => Lit::defs(),
        super::wavefront::WAVEFRONT => Lit::defs(),
        GLOW | SHINE | shape::GLOW => Unlit::defs(),
        _ => &[],
    }
}

/// A layer's Lighting as the renderer reads it ([`bpe_gfx::LayerGpu::light`]): Lit, Blocks light
/// (0 or 1), then 0 and 0 (the draw fills in the body's Bulge and the thing's surface index). A
/// layer without the settings is neither lit nor blocking.
pub fn of(p: &Params) -> [f32; 4] {
    let number = |key| match p.get(key) {
        Some(Value::Number(v)) => *v as f32,
        Some(Value::Int(v)) => *v as f32,
        Some(Value::Bool(b)) => f32::from(u8::from(*b)),
        _ => 0.0,
    };
    [number(LIT), number(BLOCKS_LIGHT), 0.0, 0.0]
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
}
