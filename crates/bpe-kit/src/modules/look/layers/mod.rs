//! The balls' Look as a stack of layers: the Look card `look.layers` holds a Body at the bottom,
//! any number of drawn layers above it (Pattern, Outline, Shine, Glow) and the Blend layer, then
//! the reactions (Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning),
//! then the sub-layers (each belongs to one drawn layer and changes only it: `subs.rs`), then
//! the group layers that spread one look across a whole group (the group gradient).
//! Each layer kind is a `#[derive(Settings)]` struct in its own file; `gpu.rs` turns a resolved
//! stack into what the renderer draws.

pub mod around;
pub mod around_subs;
pub mod blend;
pub mod body;
pub mod cards_to_layers;
pub mod cut;
pub mod glow;
pub mod gpu;
pub mod group;
pub mod group_shader;
pub mod outline;
pub mod particles;
pub mod pattern;
pub mod reactions;
pub mod shader;
pub mod shading;
pub mod shine;
pub mod subs;
pub mod trail;
pub mod vary;
pub mod wavefront;

use bpe_core::{Choice, LayerKind, LayerSection, ModuleKind, SettingDef, SlotId, Value};

pub use blend::BlendLayer;
pub use body::BodyLayer;
pub use cut::{CutFrom, CutLayer, CutMoves, CutShape, ShapeCut, ShapeCutFrom, ShapeCutMoves};
pub use glow::GlowLayer;
pub use group::{Changes, Easing, GroupGradient, GroupMix, Spread};
pub use group_shader::{GroupShader, GroupShaderMix, Shows};
pub use outline::{OutlineLayer, OutlineSits};
pub use pattern::PatternLayer;
pub use reactions::{ColourOnHit, ColourOverTime, FlashOnHit, StretchWhenFast, Turning};
pub use shader::ShaderLayer;
pub use shading::{ShadingKind, ShadingLayer, Space};
pub use shine::{ShineLayer, ShineLight, ShineMaterial};
pub use subs::{
    GlowWhere, RecolourMode, ShadowMoves, SubColourFrom, SubFlash, SubGlow, SubHue, SubOutline, SubRecolour, SubShadow,
};

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
        crate::things::balls::rows::has_balls(thing_kind)
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
    /// Whether layers of this kind may carry sub-layers.
    pub takes_subs: bool,
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
    fn takes_subs(&self) -> bool {
        self.takes_subs
    }
    /// A Shader layer takes no Shader sub-layer: the picture a shader sub-layer reads is its
    /// layer worked out without effects.
    fn takes_sub(&self, sub: &str) -> bool {
        if self.section == LayerSection::Around {
            return AROUND_SUBS.contains(&sub);
        }
        !((self.id == SHADER && sub == SUB_SHADER)
            || (self.id == super::shape::SHADER && sub == super::shape::SUB_SHADER))
    }
    fn dynamic_defs(&self, get: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
        (self.dynamic)(get)
    }
    /// Only a ball's drawn layers but Blend can be given to some balls of a group.
    fn per_ball(&self) -> bool {
        self.card == CARD && self.id != BLEND
    }
    /// A ball layer's numbers, pairs, colours and gradients can vary across the balls, but those
    /// the whole thing shares (see [`vary::layer_vary`]).
    fn vary(&self, def: &SettingDef) -> Option<bpe_core::VaryUse> {
        if self.card != CARD {
            return None;
        }
        vary::layer_vary(self.id, def)
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
pub const SUB_CUT: &str = "sub.cut";
pub const SUB_GLOW: &str = "sub.glow";
pub const SUB_OUTLINE: &str = "sub.outline";
pub const SUB_SHADOW: &str = "sub.shadow";
pub const SUB_SHADING: &str = "sub.shading";
pub const SUB_SHINE: &str = "sub.shine";
pub const SUB_SHADER: &str = "sub.shader";
pub const SUB_RECOLOUR: &str = "sub.recolour";
pub const SUB_FLASH: &str = "sub.flash";
pub const SUB_HUE: &str = "sub.hue";
pub use particles::{PARTICLES, SHAPE_PARTICLES};
pub use trail::TRAIL;

/// The sub-layers a trail or particles layer takes, on balls and on shapes: those that change its
/// colours or add a glow, an outline or a shadow round what it draws.
pub const AROUND_SUBS: [&str; 12] = [
    SUB_GLOW,
    SUB_OUTLINE,
    SUB_SHADOW,
    SUB_RECOLOUR,
    SUB_FLASH,
    SUB_HUE,
    super::shape::SUB_GLOW,
    super::shape::SUB_OUTLINE,
    super::shape::SUB_SHADOW,
    super::shape::SUB_RECOLOUR,
    super::shape::SUB_FLASH,
    super::shape::SUB_HUE,
];

/// A trail or particles layer kind of `card`: in the "Trails and particles" list, repeating,
/// carrying the [`AROUND_SUBS`].
pub(crate) fn around_kind(
    card: &'static str,
    id: &'static str,
    name: &'static str,
    help: &'static str,
    settings: fn() -> &'static [SettingDef],
) -> Kind {
    Kind {
        id,
        name,
        help,
        section: LayerSection::Around,
        repeats: true,
        base: false,
        card,
        takes_subs: true,
        settings,
        dynamic: no_more,
    }
}
/// The kind id masks had before sub-layers (scene format 8): they open as Cut sub-layers.
pub const OLD_MASK: &str = "layer.mask";
pub const FLASH: &str = "react.flash";
pub const HUE: &str = "react.hue";
pub const STRETCH: &str = "react.stretch";
pub const RAINBOW: &str = "react.rainbow";
pub const TURNING: &str = "react.turning";
pub const GROUP_GRADIENT: &str = "group.gradient";
pub const GROUP_SHADER: &str = "group.shader";

/// Every layer kind of the balls' Look, in the order the add menus list them.
pub fn kinds() -> Vec<Kind> {
    use LayerSection::{Drawn, Reaction};
    use bpe_core::Settings;
    let drawn = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: Drawn,
        repeats: true,
        base: false,
        card: CARD,
        takes_subs: true,
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
        takes_subs: false,
        settings,
        dynamic: no_more,
    };
    let mut kinds = vec![
        Kind {
            id: BODY,
            name: "Body",
            help: "The ball itself: its shape and colours. Always the bottom layer.",
            section: Drawn,
            repeats: false,
            base: true,
            card: CARD,
            takes_subs: false,
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
            takes_subs: false,
            ..drawn(BLEND, "Blend", "How the whole finished ball mixes with what is behind it.", BlendLayer::defs)
        },
        around_kind(
            CARD,
            TRAIL,
            "Trail",
            "Something left behind each ball along where it has been: a streak, ribbon, ghosts, dots, a line or smoke.",
            trail::TrailLayer::defs,
        ),
        around_kind(
            CARD,
            PARTICLES,
            "Particles",
            "Bits let off by each ball: on hits, while it moves, when it pops or steadily.",
            particles::ParticlesLayer::defs,
        ),
        around_kind(
            CARD,
            wavefront::WAVEFRONT,
            "Wavefront",
            "A line through each round's balls, in order round the round: a wavefront.",
            wavefront::WavefrontLayer::defs,
        ),
        reaction(FLASH, "Flash on hit", "A flash of light where the ball hits something.", FlashOnHit::defs),
        reaction(HUE, "Colour on hit", "Every hit turns the ball's colour round the colour wheel.", ColourOnHit::defs),
        reaction(STRETCH, "Stretch when fast", "Fast balls stretch along their path.", StretchWhenFast::defs),
        reaction(RAINBOW, "Colour over time", "The ball's colour cycles round the rainbow.", ColourOverTime::defs),
        reaction(TURNING, "Turning", "The ball's shape and pattern turn.", Turning::defs),
    ];
    kinds.extend(sub_kinds());
    kinds.extend([
        // The sub-layers come between the reactions and the group layers.
        Kind {
            id: GROUP_GRADIENT,
            name: "Gradient",
            help: "Colours spread across the whole group, mixed with each ball's own.",
            section: LayerSection::Group,
            repeats: false,
            base: false,
            card: CARD,
            takes_subs: false,
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
            takes_subs: false,
            settings: group_shader::defs,
            dynamic: shader::dynamic_defs,
        },
    ]);
    kinds
}

/// A sub-layer kind of `card`: it repeats and takes no sub-layers of its own.
pub(crate) fn sub_kind(
    card: &'static str,
    id: &'static str,
    name: &'static str,
    help: &'static str,
    settings: fn() -> &'static [SettingDef],
) -> Kind {
    Kind {
        id,
        name,
        help,
        section: LayerSection::Sub,
        repeats: true,
        base: false,
        card,
        takes_subs: false,
        settings,
        dynamic: no_more,
    }
}

/// The balls' ten sub-layer kinds, in the Add sub-layer menu's order.
pub fn sub_kinds() -> Vec<Kind> {
    use bpe_core::Settings;
    let sub = |id, name, help, settings| sub_kind(CARD, id, name, help, settings);
    vec![
        sub(
            SUB_CUT,
            "Cut",
            "Limits its layer to part of the ball. Later sub-layers follow the cut edge.",
            CutLayer::defs,
        ),
        sub(
            SUB_GLOW,
            "Glow",
            "Soft light round its layer's shape, as it stands after the sub-layers above.",
            SubGlow::defs,
        ),
        sub(SUB_OUTLINE, "Outline", "A line along its layer's edge, painted over it.", SubOutline::defs),
        sub(SUB_SHADOW, "Shadow", "A soft offset copy of its layer's shape, under it.", SubShadow::defs),
        sub(SUB_SHADING, "Shading", "Darkens its layer only.", subs::shading_defs),
        sub(SUB_SHINE, "Shine", "Lights its layer only, as a Shine lights the ball.", subs::shine_defs),
        Kind { dynamic: shader::dynamic_defs, ..sub(SUB_SHADER, "Shader", subs::SHADER_HELP, subs::shader_defs) },
        sub(SUB_RECOLOUR, "Recolour", "Changes its layer's colours only.", SubRecolour::defs),
        sub(SUB_FLASH, "Flash on hit", "Its layer lights up when the ball is hit.", SubFlash::defs),
        sub(SUB_HUE, "Colour on hit", "Its layer's colours turn round the wheel on every hit.", SubHue::defs),
    ]
}

/// How much of its balls' past a thing's shown trail and particle layers need kept (see
/// [`trail::TrailLayer::reach`], [`particles::ParticlesLayer::reach`]); `None` when they need
/// none.
pub fn around_history(resolved: &bpe_core::ResolvedThing) -> Option<bpe_runtime::HistoryWant> {
    use bpe_core::Settings;
    use bpe_runtime::HistoryWant;
    let mut want: Option<HistoryWant> = None;
    let mut add = |w: HistoryWant| want = Some(want.map_or(w, |o| o.and(w)));
    let Some(look) = resolved.slots.get(&SlotId::Look).filter(|l| l.kind == CARD) else { return want };
    for layer in &look.layers {
        match layer.kind.as_str() {
            TRAIL => {
                let t = trail::TrailLayer::from_params(&look.layer_params(layer.id));
                add(HistoryWant { seconds: t.reach(), forever: t.stays() });
            }
            PARTICLES => {
                let p = particles::ParticlesLayer::from_params(&look.layer_params(layer.id));
                add(HistoryWant { seconds: p.reach(), forever: false });
            }
            _ => {}
        }
    }
    want
}

/// Opens format 8 scenes: every mask becomes a Cut sub-layer with the same settings.
pub fn masks_to_cuts(scene: &mut bpe_core::Scene) {
    for thing in scene.things.values_mut() {
        for card in thing.slots.values_mut() {
            for layer in &mut card.layers {
                if layer.kind == OLD_MASK {
                    layer.kind = SUB_CUT.into();
                }
            }
        }
    }
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
                TRAIL,
                PARTICLES,
                wavefront::WAVEFRONT,
                FLASH,
                HUE,
                STRETCH,
                RAINBOW,
                TURNING,
                SUB_CUT,
                SUB_GLOW,
                SUB_OUTLINE,
                SUB_SHADOW,
                SUB_SHADING,
                SUB_SHINE,
                SUB_SHADER,
                SUB_RECOLOUR,
                SUB_FLASH,
                SUB_HUE,
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
        for k in reg.layers_for(CARD).filter(|k| k.section() != LayerSection::Around) {
            let has = |key: &str| k.settings().iter().any(|d| d.key == key);
            let drawn = k.section() == LayerSection::Drawn && !matches!(k.id(), BODY | BLEND);
            let sub = k.section() == LayerSection::Sub;
            assert_eq!(has("opacity") && has("blend"), drawn, "{}", k.id());
            // The Body has an opacity but no blend: nothing is under it inside the ball.
            if !sub {
                assert_eq!(has("opacity"), drawn || k.id() == BODY, "{}", k.id());
            }
            assert!(!(sub && has("blend")), "{}: a sub-layer mixes by its layer's blend", k.id());
            assert_eq!(
                k.repeats(),
                drawn || sub || k.id() == GROUP_SHADER,
                "{}: drawn layers, sub-layers and group shaders repeat",
                k.id()
            );
            assert_eq!(k.takes_subs(), drawn, "{}: drawn layers but Body and Blend take sub-layers", k.id());
        }
        assert_eq!(reg.layers_for(CARD).filter(|k| k.section() == LayerSection::Sub).count(), 10);
    }

    #[test]
    fn a_format_8_mask_opens_as_a_cut_with_its_settings() {
        let reg = registry();
        let mut scene = crate::new_studio_scene(&reg);
        let ball = crate::add_thing(&mut scene, &reg, "balls", bpe_core::ThingId::ROOT).unwrap();
        let slot = bpe_core::SlotId::Look;
        let outline = scene.add_layer(&reg, ball, slot, OUTLINE).unwrap();
        let cut = scene.add_sub_layer(&reg, ball, slot, outline, SUB_CUT).unwrap();
        let size = bpe_core::Address::on(ball, slot, &bpe_core::layered_key(cut, "size"));
        scene.set_at(&reg, &size, Value::Number(0.25)).unwrap();
        let text = bpe_core::save(&scene).replace("\"sub.cut\"", "\"layer.mask\"").replace("format: 11", "format: 8");
        assert!(text.contains("layer.mask"));
        let back = bpe_core::load(&text, &reg).unwrap();
        let layer = back.card(ball, slot).unwrap().layer(cut).unwrap();
        assert_eq!((layer.kind.as_str(), layer.parent), (SUB_CUT, Some(outline)));
        assert_eq!(back.value_at(&reg, &size), Some(Value::Number(0.25)));
        // A format 9 file keeps whatever it says.
        let nine = bpe_core::save(&scene).replace("\"sub.cut\"", "\"layer.mask\"").replace("format: 11", "format: 9");
        let back = bpe_core::load(&nine, &reg).unwrap();
        assert_eq!(back.card(ball, slot).unwrap().layer(cut).unwrap().kind, OLD_MASK);
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

    #[test]
    fn trails_and_particles_take_only_the_six_sub_layers_that_fit_them() {
        let reg = registry();
        let shape = crate::modules::look::shape::CARD;
        for (card, id) in [(CARD, TRAIL), (CARD, PARTICLES), (shape, SHAPE_PARTICLES)] {
            let k = reg.layer(id).unwrap();
            assert_eq!(k.section(), LayerSection::Around, "{id}");
            assert_eq!(k.card(), card, "{id}");
            assert!(k.repeats() && k.takes_subs(), "{id}");
            let subs: Vec<&str> = reg
                .layers_for(card)
                .filter(|s| s.section() == LayerSection::Sub && k.takes_sub(s.id()))
                .map(|s| s.name())
                .collect();
            assert_eq!(subs, ["Glow", "Outline", "Shadow", "Recolour", "Flash on hit", "Colour on hit"], "{id}");
        }
        assert!(reg.layers_for(shape).all(|k| k.id() != TRAIL), "no trails on shapes");
    }

    #[test]
    fn a_trail_s_settings_follow_its_kind() {
        use bpe_core::{Params, Value};
        let reg = registry();
        let k = reg.layer(TRAIL).unwrap();
        let shown = |kind: &str| -> Vec<&str> {
            let mut p = Params::default();
            p.set("kind", Value::Choice(kind.into()));
            let get = |key: &str| {
                p.get(key).cloned().or_else(|| k.settings().iter().find(|d| d.key == key).map(|d| d.default.clone()))
            };
            k.settings().iter().filter(|d| bpe_core::settings::applies(d.when, None, &get)).map(|d| d.key).collect()
        };
        let streak = shown("streak");
        assert!(streak.contains(&"width") && streak.contains(&"tail_width") && !streak.contains(&"thickness"));
        let line = shown("line");
        assert!(line.contains(&"thickness") && line.contains(&"forever") && !line.contains(&"width"));
        let ghosts = shown("ghosts");
        assert!(ghosts.contains(&"ghosts") && !ghosts.contains(&"seconds") && !ghosts.contains(&"blend"));
        assert!(shown("smoke").contains(&"puffs") && !streak.contains(&"puffs"));
        assert!(shown("ribbon").contains(&"twist") && !streak.contains(&"twist"));
    }

    #[test]
    fn the_history_follows_the_longest_trail_and_a_line_that_stays_keeps_it_all() {
        use bpe_core::{Address, Scene, ThingId, layered_key};
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut s = Scene::empty(reg);
        let b = crate::add_thing(&mut s, reg, "balls", ThingId::ROOT).unwrap();
        let world = |s: &Scene| bpe_runtime::World::build(s, &bpe_core::resolve(s, reg), &kit.sims);
        let fp = |s: &Scene| bpe_runtime::fingerprint(s, &bpe_core::resolve(s, reg), &kit.sims);
        assert!(world(&s).history.is_empty(), "no trail, no history");
        let bare = fp(&s);
        let short = s.add_layer(reg, b, SlotId::Look, TRAIL).unwrap();
        assert_eq!(world(&s).history[&b].cap(), 2 * 60 + 1, "at least two seconds");
        let with_trail = fp(&s);
        assert_ne!(with_trail, bare);
        let long = s.add_layer(reg, b, SlotId::Look, TRAIL).unwrap();
        let set = |s: &mut Scene, id, k: &str, v| {
            s.set_at(reg, &Address::on(b, SlotId::Look, &layered_key(id, k)), v).unwrap()
        };
        set(&mut s, long, "seconds", Value::Number(1.5));
        set(&mut s, long, "faster", Value::Number(1.0));
        assert_eq!(world(&s).history[&b].cap(), 6 * 60 + 1, "1.5 s, four times longer when fast");
        // How a trail looks is not a rebuild.
        set(&mut s, short, "brightness", Value::Number(1.5));
        let fp_long = fp(&s);
        set(&mut s, short, "width", Value::Number(1.5));
        assert_eq!(fp(&s), fp_long);
        set(&mut s, short, "kind", Value::Choice("line".into()));
        set(&mut s, short, "forever", Value::Bool(true));
        assert_eq!(world(&s).history[&b].cap(), usize::MAX);
        assert_ne!(fp(&s), fp_long);
        // Particles read the past too: where and how fast the ball went.
        let mut s2 = Scene::empty(reg);
        let b2 = crate::add_thing(&mut s2, reg, "balls", ThingId::ROOT).unwrap();
        let p = s2.add_layer(reg, b2, SlotId::Look, PARTICLES).unwrap();
        assert_eq!(world(&s2).history[&b2].cap(), 2 * 60 + 1);
        s2.set_at(reg, &Address::on(b2, SlotId::Look, &layered_key(p, "life")), Value::Number(3.0)).unwrap();
        assert_eq!(world(&s2).history[&b2].cap(), 3 * 60 + 1);
    }
}
