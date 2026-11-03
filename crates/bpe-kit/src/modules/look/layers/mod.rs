//! The balls' Look as a stack of layers: the Look card `look.layers` holds a Body at the bottom,
//! any number of drawn layers above it (Pattern, Outline, Shine, Glow) and the Blend layer, then
//! the reactions (Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning).
//! Each layer kind is a `#[derive(Settings)]` struct in its own file; `gpu.rs` turns a resolved
//! stack into what the renderer draws.

pub mod blend;
pub mod body;
pub mod glow;
pub mod gpu;
pub mod outline;
pub mod pattern;
pub mod reactions;
pub mod shine;

use bpe_core::{Choice, LayerKind, LayerSection, ModuleKind, SettingDef, SlotId};

pub use blend::BlendLayer;
pub use body::BodyLayer;
pub use glow::GlowLayer;
pub use outline::{OutlineLayer, OutlineSits};
pub use pattern::PatternLayer;
pub use reactions::{ColourOnHit, ColourOverTime, FlashOnHit, StretchWhenFast, Turning};
pub use shine::ShineLayer;

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

/// One layer kind of the stack: its id, name, help, section, settings and whether it repeats.
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub section: LayerSection,
    pub repeats: bool,
    pub base: bool,
    pub settings: fn() -> &'static [SettingDef],
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
        CARD
    }
    fn base(&self) -> bool {
        self.base
    }
}

pub const BODY: &str = "layer.body";
pub const PATTERN: &str = "layer.pattern";
pub const OUTLINE: &str = "layer.outline";
pub const SHINE: &str = "layer.shine";
pub const GLOW: &str = "layer.glow";
pub const BLEND: &str = "layer.blend";
pub const FLASH: &str = "react.flash";
pub const HUE: &str = "react.hue";
pub const STRETCH: &str = "react.stretch";
pub const RAINBOW: &str = "react.rainbow";
pub const TURNING: &str = "react.turning";

/// Every layer kind of the balls' Look, in the order the add menus list them.
pub fn kinds() -> Vec<Kind> {
    use LayerSection::{Drawn, Reaction};
    use bpe_core::Settings;
    let drawn =
        |id, name, help, repeats, settings| Kind { id, name, help, section: Drawn, repeats, base: false, settings };
    let reaction =
        |id, name, help, settings| Kind { id, name, help, section: Reaction, repeats: false, base: false, settings };
    vec![
        Kind {
            id: BODY,
            name: "Body",
            help: "The ball itself: its shape and colours. Always the bottom layer.",
            section: Drawn,
            repeats: false,
            base: true,
            settings: BodyLayer::defs,
        },
        drawn(PATTERN, "Pattern", "A pattern painted over the layers under it.", true, PatternLayer::defs),
        drawn(OUTLINE, "Outline", "A ring along the ball's edge.", true, OutlineLayer::defs),
        drawn(SHINE, "Shine", "Light on the ball, so it looks round and shiny.", true, ShineLayer::defs),
        drawn(GLOW, "Glow", "A soft halo of light around the ball.", true, GlowLayer::defs),
        drawn(BLEND, "Blend", "How the whole finished ball mixes with what is behind it.", false, BlendLayer::defs),
        reaction(FLASH, "Flash on hit", "A flash of light where the ball hits something.", FlashOnHit::defs),
        reaction(HUE, "Colour on hit", "Every hit turns the ball's colour round the colour wheel.", ColourOnHit::defs),
        reaction(STRETCH, "Stretch when fast", "Fast balls stretch along their path.", StretchWhenFast::defs),
        reaction(RAINBOW, "Colour over time", "The ball's colour cycles round the rainbow.", ColourOverTime::defs),
        reaction(TURNING, "Turning", "The ball's shape and pattern turn.", Turning::defs),
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
            [BODY, PATTERN, OUTLINE, SHINE, GLOW, BLEND, FLASH, HUE, STRETCH, RAINBOW, TURNING],
            "in the add menus' order"
        );
        assert!(reg.layer(BODY).unwrap().base());
        assert_eq!(reg.layers().filter(|k| k.base()).count(), 1);
        for k in reg.layers() {
            for d in k.settings() {
                assert!(!d.key.contains('/'), "{}: {}", k.id(), d.key);
            }
        }
        assert!(bpe_core::check_registry(&reg).is_empty(), "{:?}", bpe_core::check_registry(&reg));
    }

    #[test]
    fn drawn_layers_but_body_and_blend_have_opacity_and_blend() {
        let reg = registry();
        for k in reg.layers_for(CARD) {
            let has = |key: &str| k.settings().iter().any(|d| d.key == key);
            let drawn = k.section() == LayerSection::Drawn && !matches!(k.id(), BODY | BLEND);
            assert_eq!(has("opacity") && has("blend"), drawn, "{}", k.id());
            assert_eq!(k.repeats(), drawn, "{}: only drawn layers repeat", k.id());
        }
    }

    #[test]
    fn the_look_card_fits_only_balls_and_the_old_look_only_other_things() {
        let reg = registry();
        let layers = reg.module(CARD).unwrap();
        let basic = reg.module("look.basic").unwrap();
        assert!(layers.fits("balls") && !layers.fits("arena"));
        assert!(!basic.fits("balls") && basic.fits("arena"));
    }
}
