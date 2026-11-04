//! The balls' Look as a stack of layers: the Look card `look.layers` holds a Body at the bottom,
//! any number of drawn layers above it (Pattern, Outline, Shine, Glow) and the Blend layer, then
//! the reactions (Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning),
//! then the group layers that spread one look across a whole group (the group gradient).
//! Each layer kind is a `#[derive(Settings)]` struct in its own file; `gpu.rs` turns a resolved
//! stack into what the renderer draws.

pub mod blend;
pub mod body;
pub mod glow;
pub mod gpu;
pub mod group;
pub mod group_shader;
pub mod mask;
pub mod outline;
pub mod pattern;
pub mod reactions;
pub mod shader;
pub mod shading;
pub mod shine;

use bpe_core::{Choice, LayerKind, LayerSection, ModuleKind, SettingDef, SlotId, Value};

pub use blend::BlendLayer;
pub use body::BodyLayer;
pub use glow::GlowLayer;
pub use group::{Changes, Easing, GroupGradient, GroupMix, Spread};
pub use group_shader::{GroupShader, GroupShaderMix, Shows};
pub use mask::{MaskFrom, MaskLayer, MaskMoves, MaskShape};
pub use outline::{OutlineLayer, OutlineSits};
pub use pattern::PatternLayer;
pub use reactions::{ColourOnHit, ColourOverTime, FlashOnHit, StretchWhenFast, Turning};
pub use shader::ShaderLayer;
pub use shading::{ShadingKind, ShadingLayer, Space};
pub use shine::{ShineLayer, ShineLight, ShineMaterial};

/// The card kind that holds the stack.
pub const CARD: &str = "look.layers";

/// The Look card of balls: a stack of layers.
pub struct LayersLookKind;

impl ModuleKind for LayersLookKind {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Look"
    }
    fn help(&self) -> &'static str {
        "How each ball looks: a stack of layers painted bottom to top, and reactions to what happens."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "balls"
    }
}

/// Where a layer's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ColourFrom {
    /// The ball's body colour (an outline takes it a little darker, so it shows).
    #[default]
    SameAsBody,
    Own,
}

/// How a layer's own colours run across each ball, when they are a gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GradientShape {
    /// Straight across the ball, at the layer's Direction.
    #[default]
    Straight,
    /// Outward: from the ball's middle to its edge (an outline across its ring, a glow from the
    /// ball's edge out).
    Round,
}

impl GradientShape {
    /// Its pattern number in paint.wgsl: 1 linear, 2 radial.
    pub fn pattern(self) -> f32 {
        match self {
            Self::Straight => 1.0,
            Self::Round => 2.0,
        }
    }
}

/// How a kind finds the settings a layer's own values add.
type DynamicDefs = fn(&dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef];

/// One layer kind of the stack: its id, name, help, section, settings and whether it repeats.
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub section: LayerSection,
    pub repeats: bool,
    pub base: bool,
    /// The card kind its layers belong to.
    pub card: &'static str,
    /// Whether layers of this kind may carry masks.
    pub maskable: bool,
    pub settings: fn() -> &'static [SettingDef],
    /// The settings a layer's own values add (a Shader layer's own code).
    pub dynamic: DynamicDefs,
}

/// No settings beyond the kind's own.
pub(crate) fn no_more(_: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
    &[]
}

impl LayerKind for Kind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.settings)()
    }
    fn section(&self) -> LayerSection {
        self.section
    }
    fn repeats(&self) -> bool {
        self.repeats
    }
    fn card(&self) -> &'static str {
        self.card
    }
    fn base(&self) -> bool {
        self.base
    }
    fn maskable(&self) -> bool {
        self.maskable
    }
    fn dynamic_defs(&self, get: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
        (self.dynamic)(get)
    }
    /// Only a ball's drawn layers but Blend can be given to some balls of a group.
    fn per_ball(&self) -> bool {
        self.card == CARD && self.id != BLEND
    }
}

pub const BODY: &str = "layer.body";
pub const PATTERN: &str = "layer.pattern";
pub const OUTLINE: &str = "layer.outline";
pub const SHINE: &str = "layer.shine";
pub const GLOW: &str = "layer.glow";
pub const SHADING: &str = "layer.shading";
pub const SHADER: &str = "layer.shader";
pub const BLEND: &str = "layer.blend";
pub const MASK: &str = "layer.mask";
pub const FLASH: &str = "react.flash";
pub const HUE: &str = "react.hue";
pub const STRETCH: &str = "react.stretch";
pub const RAINBOW: &str = "react.rainbow";
pub const TURNING: &str = "react.turning";
pub const GROUP_GRADIENT: &str = "group.gradient";
pub const GROUP_SHADER: &str = "group.shader";

/// Every layer kind of the balls' Look, in the order the add menus list them.
pub fn kinds() -> Vec<Kind> {
    use LayerSection::{Drawn, Mask, Reaction};
    use bpe_core::Settings;
    let drawn = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: Drawn,
        repeats: true,
        base: false,
        card: CARD,
        maskable: true,
        settings,
        dynamic: no_more,
    };
    let reaction = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: Reaction,
        repeats: false,
        base: false,
        card: CARD,
        maskable: false,
        settings,
        dynamic: no_more,
    };
    vec![
        Kind {
            id: BODY,
            name: "Body",
            help: "The ball itself: its shape and colours. Always the bottom layer.",
            section: Drawn,
            repeats: false,
            base: true,
            card: CARD,
            maskable: false,
            settings: BodyLayer::defs,
            dynamic: no_more,
        },
        drawn(PATTERN, "Pattern", "A pattern painted over the layers under it.", PatternLayer::defs),
        drawn(OUTLINE, "Outline", "A ring along the ball's edge.", OutlineLayer::defs),
        drawn(SHINE, "Shine", "Light on the ball, so it looks round and shiny.", ShineLayer::defs),
        drawn(GLOW, "Glow", "A soft halo of light around the ball.", GlowLayer::defs),
        drawn(
            SHADING,
            "Shading",
            "Darkens part of the ball: along a line, outside a circle or where painted.",
            ShadingLayer::defs,
        ),
        Kind { dynamic: shader::dynamic_defs, ..drawn(SHADER, "Shader", shader::HELP, shader::defs) },
        Kind {
            repeats: false,
            maskable: false,
            ..drawn(BLEND, "Blend", "How the whole finished ball mixes with what is behind it.", BlendLayer::defs)
        },
        reaction(FLASH, "Flash on hit", "A flash of light where the ball hits something.", FlashOnHit::defs),
        reaction(HUE, "Colour on hit", "Every hit turns the ball's colour round the colour wheel.", ColourOnHit::defs),
        reaction(STRETCH, "Stretch when fast", "Fast balls stretch along their path.", StretchWhenFast::defs),
        reaction(RAINBOW, "Colour over time", "The ball's colour cycles round the rainbow.", ColourOverTime::defs),
        reaction(TURNING, "Turning", "The ball's shape and pattern turn.", Turning::defs),
        Kind {
            id: MASK,
            name: "Mask",
            help: "Limits the layer above it to part of the ball. A layer's masks multiply.",
            section: Mask,
            repeats: true,
            base: false,
            card: CARD,
            maskable: false,
            settings: MaskLayer::defs,
            dynamic: no_more,
        },
        Kind {
            id: GROUP_GRADIENT,
            name: "Gradient",
            help: "Colours spread across the whole group, mixed with each ball's own.",
            section: LayerSection::Group,
            repeats: false,
            base: false,
            card: CARD,
            maskable: false,
            settings: GroupGradient::defs,
            dynamic: no_more,
        },
        Kind {
            id: GROUP_SHADER,
            name: "Shader",
            help: group_shader::HELP,
            section: LayerSection::Group,
            repeats: true,
            base: false,
            card: CARD,
            maskable: false,
            settings: group_shader::defs,
            dynamic: shader::dynamic_defs,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    #[test]
    fn every_layer_kind_is_registered_for_the_look_card() {
        let reg = registry();
        let ids: Vec<&str> = reg.layers_for(CARD).map(|k| k.id()).collect();
        assert_eq!(
            ids,
            [
                BODY,
                PATTERN,
                OUTLINE,
                SHINE,
                GLOW,
                SHADING,
                SHADER,
                BLEND,
                FLASH,
                HUE,
                STRETCH,
                RAINBOW,
                TURNING,
                MASK,
                GROUP_GRADIENT,
                GROUP_SHADER
            ],
            "in the add menus' order"
        );
        assert!(reg.layer(BODY).unwrap().base());
        assert_eq!(reg.layers_for(CARD).filter(|k| k.base()).count(), 1);
        let shape = crate::modules::look::shape::CARD;
        let bases: Vec<&str> = reg.layers_for(shape).filter(|k| k.base()).map(|k| k.id()).collect();
        assert_eq!(bases, [crate::modules::look::shape::BODY], "a shape's look has its own Body");
        for k in reg.layers() {
            for d in k.settings() {
                assert!(!d.key.contains('/'), "{}: {}", k.id(), d.key);
            }
        }
        assert!(bpe_core::check_registry(&reg).is_empty(), "{:?}", bpe_core::check_registry(&reg));
    }

    #[test]
    fn blend_is_every_ball_s_or_none() {
        let reg = registry();
        let mut scene = crate::new_studio_scene(&reg);
        let ball = crate::add_thing(&mut scene, &reg, "balls", bpe_core::ThingId::ROOT).unwrap();
        let slot = bpe_core::SlotId::Look;
        let blend = scene.add_layer(&reg, ball, slot, BLEND).unwrap();
        assert!(scene.set_layer_only(&reg, ball, slot, blend, vec![1]).is_err());
        let outline = scene.add_layer(&reg, ball, slot, OUTLINE).unwrap();
        assert!(scene.set_layer_only(&reg, ball, slot, outline, vec![1]).is_ok());
    }

    #[test]
    fn drawn_layers_but_body_and_blend_have_opacity_and_blend_and_body_has_opacity() {
        let reg = registry();
        for k in reg.layers_for(CARD) {
            let has = |key: &str| k.settings().iter().any(|d| d.key == key);
            let drawn = k.section() == LayerSection::Drawn && !matches!(k.id(), BODY | BLEND);
            assert_eq!(has("opacity") && has("blend"), drawn, "{}", k.id());
            // The Body has an opacity but no blend: nothing is under it inside the ball.
            assert_eq!(has("opacity"), drawn || k.id() == BODY, "{}", k.id());
            assert_eq!(
                k.repeats(),
                drawn || k.section() == LayerSection::Mask || k.id() == GROUP_SHADER,
                "{}: drawn layers, masks and group shaders repeat",
                k.id()
            );
            assert_eq!(k.maskable(), drawn, "{}: drawn layers but Body and Blend take masks", k.id());
        }
        assert_eq!(reg.layer(MASK).unwrap().section(), LayerSection::Mask);
    }

    #[test]
    fn each_look_card_fits_its_own_kind_and_the_old_look_none() {
        let reg = registry();
        let layers = reg.module(CARD).unwrap();
        let shape = reg.module(crate::modules::look::shape::CARD).unwrap();
        let basic = reg.module("look.basic").unwrap();
        assert!(layers.fits("balls") && !layers.fits("shape"));
        assert!(shape.fits("shape") && !shape.fits("balls"));
        assert!(["balls", "shape", "scene", "folder", "camera"].iter().all(|k| !basic.fits(k)));
    }
}
