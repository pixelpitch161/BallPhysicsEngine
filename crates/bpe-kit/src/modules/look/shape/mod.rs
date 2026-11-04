//! A Shape's Look: a stack of layers like the balls', whose bottom layer, the Body, is the shape
//! itself, drawn plain white. The Body has no settings of its own: picking it shows the shape's
//! outline settings, which are the thing's own (see `things::shape::BODY_HEADINGS`). Above it
//! sit the paint layers ([`kinds`]): Colour and gradient, Outline, Glow, Shader and Blend; `gpu.rs`
//! turns a resolved stack into the records the paths pipeline walks.

pub mod gpu;
pub mod layers;
pub mod subs;

use std::sync::OnceLock;

use bpe_core::{LayerKind, LayerSection, ModuleKind, SettingDef, SlotId, Value, ValueType};

pub use gpu::ShapeStack;
pub use layers::{LineRuns, PaintLayer, PaintRuns, ShapeGlow, ShapeOutline};
pub use subs::{ShapeShading, ShapeShadingKind, ShapeShadowMoves, ShapeSpace, ShapeSubGlow, ShapeSubShadow};

use crate::modules::look::layers::{Kind, shader, sub_kind};

/// The card kind.
pub const CARD: &str = "look.shape";
/// The Body layer kind.
pub const BODY: &str = "shape.body";
/// Colour and gradient.
pub const PAINT: &str = "shape.paint";
pub const OUTLINE: &str = "shape.outline";
pub const GLOW: &str = "shape.glow";
pub const SHADER: &str = "shape.shader";
pub const BLEND: &str = "shape.blend";
pub const SUB_CUT: &str = "shape.sub.cut";
pub const SUB_GLOW: &str = "shape.sub.glow";
pub const SUB_OUTLINE: &str = "shape.sub.outline";
pub const SUB_SHADOW: &str = "shape.sub.shadow";
pub const SUB_SHADING: &str = "shape.sub.shading";
pub const SUB_SHINE: &str = "shape.sub.shine";
pub const SUB_SHADER: &str = "shape.sub.shader";
pub const SUB_RECOLOUR: &str = "shape.sub.recolour";
pub const SUB_FLASH: &str = "shape.sub.flash";
pub const SUB_HUE: &str = "shape.sub.hue";

/// The help every shape Shader layer shows.
pub const SHADER_HELP: &str =
    "An effect painted inside the shape. It sees the layers under it, leaving out other Shader layers.";

/// What a shape Shader's Stays with offers: turning with the shape, or upright on the screen.
/// "shape" reads as the balls' `Space::Ball` (its default), the one that turns.
pub const STAYS_WITH: [&str; 2] = ["shape", "screen"];

/// The settings of a shape's Shader layer: the balls' Shader layer's, with Stays with offering
/// the shape and the screen, and the words about balls made about shapes.
pub fn shader_defs() -> &'static [SettingDef] {
    static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        shader::defs()
            .iter()
            .map(|d| match d.key {
                "space" => SettingDef {
                    help: "Whether the effect turns with the shape or stays upright on the screen.",
                    ty: ValueType::Choice(&STAYS_WITH),
                    default: Value::Choice(STAYS_WITH[0].into()),
                    ..d.clone()
                },
                "scale" => SettingDef {
                    help: "How big the effect is drawn: bigger spreads it wider over the shape.",
                    ..d.clone()
                },
                _ => d.clone(),
            })
            .collect()
    })
}

/// The paint layers of a shape's Look, in the add menu's order, then its sub-layer kinds.
pub fn kinds() -> Vec<Kind> {
    use bpe_core::Settings;
    let drawn = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: LayerSection::Drawn,
        repeats: true,
        base: false,
        card: CARD,
        takes_subs: true,
        settings,
        dynamic: crate::modules::look::layers::no_more,
    };
    vec![
        drawn(
            PAINT,
            "Colour and gradient",
            "One colour, or a gradient across the shape, from its middle or along its edge.",
            PaintLayer::defs,
        ),
        drawn(OUTLINE, "Outline", "A line along the shape's edges.", ShapeOutline::defs),
        drawn(GLOW, "Glow", "Soft light outside the shape's edges.", ShapeGlow::defs),
        Kind { dynamic: shader::dynamic_defs, ..drawn(SHADER, "Shader", SHADER_HELP, shader_defs) },
        Kind {
            repeats: false,
            takes_subs: false,
            ..drawn(
                BLEND,
                "Blend",
                "How the whole finished shape mixes with what is behind it.",
                crate::modules::look::layers::BlendLayer::defs,
            )
        },
    ]
    .into_iter()
    .chain(sub_kinds())
    .collect()
}

/// A shape's ten sub-layer kinds, in the Add sub-layer menu's order: the balls', with sizes in
/// pixels and no brush.
pub fn sub_kinds() -> Vec<Kind> {
    use crate::modules::look::layers::{ShapeCut, SubFlash, SubHue, SubOutline, SubRecolour, subs as ball};
    use bpe_core::Settings;
    let sub = |id, name, help, settings| sub_kind(CARD, id, name, help, settings);
    vec![
        sub(
            SUB_CUT,
            "Cut",
            "Limits its layer to part of the shape. Later sub-layers follow the cut edge.",
            ShapeCut::defs,
        ),
        sub(
            SUB_GLOW,
            "Glow",
            "Soft light round its layer's shape, as it stands after the sub-layers above.",
            ShapeSubGlow::defs,
        ),
        sub(SUB_OUTLINE, "Outline", "A line along its layer's edge, painted over it.", SubOutline::defs),
        sub(SUB_SHADOW, "Shadow", "A soft offset copy of its layer's shape, under it.", ShapeSubShadow::defs),
        sub(SUB_SHADING, "Shading", "Darkens its layer only.", subs::shading_defs),
        sub(SUB_SHINE, "Shine", "Lights its layer only, as a Shine lights a ball.", ball::shine_defs),
        Kind { dynamic: shader::dynamic_defs, ..sub(SUB_SHADER, "Shader", ball::SHADER_HELP, sub_shader_defs) },
        sub(SUB_RECOLOUR, "Recolour", "Changes its layer's colours only.", SubRecolour::defs),
        sub(SUB_FLASH, "Flash on hit", "Its layer lights up when a ball hits the shape.", SubFlash::defs),
        sub(
            SUB_HUE,
            "Colour on hit",
            "Its layer's colours turn round the wheel every time a ball hits the shape.",
            SubHue::defs,
        ),
    ]
}

/// A shape's Shader sub-layer settings: a shape Shader layer's, without Blend.
fn sub_shader_defs() -> &'static [SettingDef] {
    static MADE: OnceLock<Vec<SettingDef>> = OnceLock::new();
    crate::modules::look::layers::subs::without_blend(&MADE, shader_defs())
}

/// The Look card of shapes.
pub struct ShapeLookKind;

impl ModuleKind for ShapeLookKind {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Look"
    }
    fn help(&self) -> &'static str {
        "How the shape looks: a stack of layers painted bottom to top, on the plain white shape."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == crate::things::shape::KIND
    }
}

/// The shape itself, the bottom layer of its Look.
pub struct ShapeBody;

impl LayerKind for ShapeBody {
    fn id(&self) -> &'static str {
        BODY
    }
    fn name(&self) -> &'static str {
        "Body"
    }
    fn help(&self) -> &'static str {
        "The shape itself. Pick it to change its outline."
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn section(&self) -> LayerSection {
        LayerSection::Drawn
    }
    fn repeats(&self) -> bool {
        false
    }
    fn card(&self) -> &'static str {
        CARD
    }
    fn base(&self) -> bool {
        true
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn a_shape_s_look_offers_its_paint_layers_in_order() {
        let reg = registry();
        let ids: Vec<&str> = reg.layers_for(CARD).map(|k| k.id()).collect();
        assert_eq!(
            ids,
            [
                BODY,
                PAINT,
                OUTLINE,
                GLOW,
                SHADER,
                BLEND,
                SUB_CUT,
                SUB_GLOW,
                SUB_OUTLINE,
                SUB_SHADOW,
                SUB_SHADING,
                SUB_SHINE,
                SUB_SHADER,
                SUB_RECOLOUR,
                SUB_FLASH,
                SUB_HUE
            ]
        );
        for k in reg.layers_for(CARD) {
            assert_eq!(k.card(), CARD);
            let sub = k.section() == LayerSection::Sub;
            let painted = !sub && !matches!(k.id(), BODY | BLEND);
            assert!(!k.per_ball(), "{}", k.id());
            assert_eq!(k.takes_subs(), painted, "{}: paint layers take sub-layers", k.id());
            assert_eq!(k.section() == LayerSection::Drawn, !sub, "{}", k.id());
            assert_eq!(k.repeats(), !matches!(k.id(), BODY | BLEND), "{}", k.id());
            let has = |key: &str| k.settings().iter().any(|d| d.key == key);
            assert_eq!(has("opacity") && has("blend"), painted, "{}", k.id());
        }
        // Shapes have no brush: their Cut and Shading menus can't offer it.
        for id in [SUB_CUT, SUB_SHADING] {
            let k = reg.layer(id).unwrap();
            for d in k.settings() {
                if let ValueType::Choice(names) = d.ty {
                    assert!(!names.contains(&"brush"), "{id}: {}", d.key);
                }
                assert_ne!(d.ty, ValueType::Strokes, "{id}");
            }
        }
        assert!(bpe_core::check_registry(&reg).is_empty(), "{:?}", bpe_core::check_registry(&reg));
    }

    #[test]
    fn a_shape_shader_stays_with_the_shape_or_the_screen_and_reads_own_code() {
        let space = shader_defs().iter().find(|d| d.key == "space").unwrap();
        assert_eq!(space.ty, ValueType::Choice(&STAYS_WITH));
        assert_eq!(space.default, Value::Choice("shape".into()));
        assert_eq!(shader_defs().len(), shader::defs().len());
        let reg = registry();
        let kind = reg.layer(SHADER).unwrap();
        let code = "// name: Mine\n// simple warmth = 0.25 [0, 1] How warm\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv) * p.warmth; }\n";
        let get = |k: &str| match k {
            "effect" => Some(Value::Choice(shader::OWN.into())),
            "code" => Some(Value::Text(code.into())),
            _ => None,
        };
        assert_eq!(kind.dynamic_defs(&get).iter().map(|d| d.key).collect::<Vec<_>>(), ["own.warmth"]);
    }
}
