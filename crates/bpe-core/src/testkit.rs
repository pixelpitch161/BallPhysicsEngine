//! Small example plugins for tests, here and in later crates.

use crate::{
    Color, LayerKind, LayerSection, ModuleKind, Registry, Scene, SettingDef, Settings, SlotId, Target, Thing, ThingId,
    ThingKind,
};

/// The scene settings.
pub struct SceneKind;
/// A folder that only groups things.
pub struct FolderKind;
/// A set of balls.
pub struct BallsKind;
/// A plain coloured look.
pub struct PlainLookKind;
/// A trail streak.
pub struct StreakKind;

#[derive(Settings)]
pub struct SceneSettings {
    /// How long the video runs.
    #[setting(label = "Length", range(1, 600), unit = "s", simple)]
    pub duration: f32,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self { duration: 30.0 }
    }
}

#[derive(Settings)]
pub struct BallsSettings {
    /// How many balls there are.
    #[setting(label = "Count", range(1, 10000), simple)]
    pub count: u32,
    /// How big each ball is.
    #[setting(label = "Size", range(0.5, 200), unit = "px", simple, keyframe)]
    pub radius: f32,
}

impl Default for BallsSettings {
    fn default() -> Self {
        Self { count: 1, radius: 6.0 }
    }
}

#[derive(Settings)]
pub struct PlainLook {
    #[setting(label = "Colour", simple, keyframe)]
    pub color: Color,
    /// How brightly the halo shines.
    #[setting(label = "Glow", range(0, 2), simple, keyframe, pulse)]
    pub glow: f32,
}

impl Default for PlainLook {
    fn default() -> Self {
        Self { color: Color::WHITE, glow: 0.0 }
    }
}

#[derive(Settings)]
pub struct Streak {
    #[setting(label = "Width", range(0, 40), unit = "px", simple, keyframe)]
    pub width: f32,
    /// Which hits leave a streak.
    #[setting(label = "Shows when hitting")]
    pub target: Target,
}

impl Default for Streak {
    fn default() -> Self {
        Self { width: 0.0, target: Target::Anything }
    }
}

const BALL_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce, SlotId::Particles];

impl ThingKind for SceneKind {
    fn id(&self) -> &'static str {
        "scene"
    }
    fn name(&self) -> &'static str {
        "Scene settings"
    }
    fn icon(&self) -> &'static str {
        "scene"
    }
    fn help(&self) -> &'static str {
        "Settings every thing uses unless it sets its own."
    }
    fn settings(&self) -> &'static [SettingDef] {
        SceneSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
}

impl ThingKind for FolderKind {
    fn id(&self) -> &'static str {
        "folder"
    }
    fn name(&self) -> &'static str {
        "Folder"
    }
    fn icon(&self) -> &'static str {
        "folder"
    }
    fn help(&self) -> &'static str {
        "Groups things so they can share settings."
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn slots(&self) -> &'static [SlotId] {
        &SlotId::ALL
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
}

impl ThingKind for BallsKind {
    fn id(&self) -> &'static str {
        "balls"
    }
    fn name(&self) -> &'static str {
        "Balls"
    }
    fn icon(&self) -> &'static str {
        "ball"
    }
    fn help(&self) -> &'static str {
        "One or more bouncing balls."
    }
    fn settings(&self) -> &'static [SettingDef] {
        BallsSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        BALL_SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce]
    }
}

impl ModuleKind for PlainLookKind {
    fn id(&self) -> &'static str {
        "look.plain"
    }
    fn name(&self) -> &'static str {
        "Plain"
    }
    fn help(&self) -> &'static str {
        "One flat colour with an optional glow."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        PlainLook::defs()
    }
    fn vary(&self, _key: &str) -> Option<crate::VaryUse> {
        Some(crate::VaryUse::Live)
    }
}

impl ModuleKind for StreakKind {
    fn id(&self) -> &'static str {
        "trail.streak"
    }
    fn name(&self) -> &'static str {
        "Streak"
    }
    fn help(&self) -> &'static str {
        "A streak behind each ball."
    }
    fn slot(&self) -> SlotId {
        SlotId::Trail
    }
    fn settings(&self) -> &'static [SettingDef] {
        Streak::defs()
    }
}

/// A card that holds a stack of layers.
pub struct StackLookKind;
/// The stack's base layer.
pub struct BaseLayerKind;
/// A drawn layer that may repeat.
pub struct RingLayerKind;
/// A reaction, at most one.
pub struct PopLayerKind;
/// A drawn layer that can't carry sub-layers, at most one.
pub struct TintLayerKind;
/// A sub-layer that limits its layer.
pub struct CutLayerKind;
/// A second sub-layer kind, so order means something. The Code layer refuses it.
pub struct GlowSubKind;
/// A drawn layer whose `code` names one more setting it has: `extra`.
pub struct CodeLayerKind;
/// A trail-or-particles layer (section Around) that repeats and takes sub-layers.
pub struct SparkLayerKind;

#[derive(Settings, Default)]
pub struct TintLayer {
    #[setting(label = "Amount", range(0, 1))]
    pub amount: f32,
}

#[derive(Settings)]
pub struct CutLayer {
    #[setting(label = "Size", range(0, 1), keyframe, pulse)]
    pub size: f32,
    #[setting(label = "Target")]
    pub target: i32,
    #[setting(label = "Strokes")]
    pub strokes: crate::Strokes,
}

impl Default for CutLayer {
    fn default() -> Self {
        Self { size: 0.5, target: -1, strokes: crate::Strokes::default() }
    }
}

#[derive(Settings, Default)]
pub struct CodeLayer {
    #[setting(label = "Code")]
    pub code: String,
}

/// The setting `extra` of a Code layer whose code is "extra".
pub static EXTRA_DEFS: std::sync::LazyLock<[SettingDef; 1]> = std::sync::LazyLock::new(|| {
    [SettingDef {
        key: "extra",
        label: "Extra",
        help: "",
        ty: crate::ValueType::Number,
        default: crate::Value::Number(0.5),
        range: Some((0.0, 1.0)),
        span: None,
        unit: "",
        group: "",
        depth: crate::Depth::Simple,
        flags: crate::Flags { keyframe: true, pulse: false, hold: false, area: bpe_core::Area::No },
        when: "",
    }]
});
/// A group layer, at most one.
pub struct WashLayerKind;
/// A group layer that may repeat.
pub struct SwirlLayerKind;

#[derive(Settings)]
pub struct BaseLayer {
    #[setting(label = "Size", range(0, 4), keyframe)]
    pub size: f32,
    #[setting(label = "Colour", keyframe)]
    pub color: Color,
    #[setting(label = "Colours", keyframe)]
    pub colours: crate::Gradient,
}

impl Default for BaseLayer {
    fn default() -> Self {
        Self { size: 1.0, color: Color::WHITE, colours: crate::Gradient::default() }
    }
}

#[derive(Settings)]
pub struct RingLayer {
    #[setting(label = "Width", range(0, 60), unit = "px", keyframe, pulse)]
    pub width: f32,
}

impl Default for RingLayer {
    fn default() -> Self {
        Self { width: 4.0 }
    }
}

#[derive(Settings)]
pub struct PopLayer {
    #[setting(label = "Amount", range(0, 1), keyframe)]
    pub amount: f32,
}

impl Default for PopLayer {
    fn default() -> Self {
        Self { amount: 0.5 }
    }
}

impl ModuleKind for StackLookKind {
    fn id(&self) -> &'static str {
        "look.stack"
    }
    fn name(&self) -> &'static str {
        "Stack"
    }
    fn help(&self) -> &'static str {
        "Layers drawn bottom to top."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
}

macro_rules! layer_kind {
    ($kind:ident, $id:literal, $name:literal, $settings:ident, $section:ident, $repeats:literal, $base:literal, $takes_subs:literal) => {
        layer_kind!($kind, $id, $name, $settings, $section, $repeats, $base, $takes_subs, "look.stack");
    };
    ($kind:ident, $id:literal, $name:literal, $settings:ident, $section:ident, $repeats:literal, $base:literal, $takes_subs:literal, $card:literal) => {
        impl LayerKind for $kind {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $name
            }
            fn help(&self) -> &'static str {
                "A test layer."
            }
            fn settings(&self) -> &'static [SettingDef] {
                $settings::defs()
            }
            fn section(&self) -> LayerSection {
                LayerSection::$section
            }
            fn repeats(&self) -> bool {
                $repeats
            }
            fn card(&self) -> &'static str {
                $card
            }
            fn base(&self) -> bool {
                $base
            }
            fn takes_subs(&self) -> bool {
                $takes_subs
            }
            fn vary(&self, def: &SettingDef) -> Option<crate::VaryUse> {
                crate::vary::can_vary_type(&def.ty).then_some(crate::VaryUse::Live)
            }
            fn takes_sub(&self, sub: &str) -> bool {
                !($id == "layer.code" && sub == "sub.glow")
            }
            fn dynamic_defs(&self, get: &dyn Fn(&str) -> Option<crate::Value>) -> &'static [SettingDef] {
                if $id == "layer.code" && get("code") == Some(crate::Value::Text("extra".into())) {
                    &*EXTRA_DEFS
                } else {
                    &[]
                }
            }
        }
    };
}

layer_kind!(BaseLayerKind, "layer.base", "Base", BaseLayer, Drawn, false, true, false);
layer_kind!(RingLayerKind, "layer.ring", "Ring", RingLayer, Drawn, true, false, true);
layer_kind!(PopLayerKind, "react.pop", "Pop", PopLayer, Reaction, false, false, false);
layer_kind!(TintLayerKind, "layer.tint", "Tint", TintLayer, Drawn, false, false, false);
layer_kind!(CutLayerKind, "sub.cut", "Cut", CutLayer, Sub, true, false, false);
layer_kind!(GlowSubKind, "sub.glow", "Glow", TintLayer, Sub, true, false, false);
layer_kind!(CodeLayerKind, "layer.code", "Code", CodeLayer, Drawn, true, false, true);
layer_kind!(SparkLayerKind, "around.spark", "Spark", RingLayer, Around, true, false, true);
layer_kind!(WashLayerKind, "group.wash", "Wash", RingLayer, Group, false, false, false);
layer_kind!(SwirlLayerKind, "group.swirl", "Swirl", RingLayer, Group, true, false, false);
layer_kind!(ChainTestKind, "chain.test", "Filter", RingLayer, Chain, true, false, false, "sound.test");
layer_kind!(ModTestKind, "mod.test", "LFO", PopLayer, Modulator, true, false, false, "sound.test");

/// A sound card holding chain layers and modulators.
pub struct SoundTestKind;
/// A chain layer of the test sound card.
pub struct ChainTestKind;
/// A modulator of the test sound card.
pub struct ModTestKind;

impl ModuleKind for SoundTestKind {
    fn id(&self) -> &'static str {
        "sound.test"
    }
    fn name(&self) -> &'static str {
        "Sound"
    }
    fn help(&self) -> &'static str {
        "A test sound."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
}

/// A registry with every test plugin.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(FolderKind);
    r.add_thing(BallsKind);
    r.add_module(PlainLookKind);
    r.add_module(StreakKind);
    r.add_module(StackLookKind);
    r.add_layer(BaseLayerKind);
    r.add_layer(RingLayerKind);
    r.add_layer(PopLayerKind);
    r.add_layer(TintLayerKind);
    r.add_layer(CutLayerKind);
    r.add_layer(GlowSubKind);
    r.add_layer(CodeLayerKind);
    r.add_layer(SparkLayerKind);
    r.add_layer(WashLayerKind);
    r.add_layer(SwirlLayerKind);
    r.add_module(SoundTestKind);
    r.add_layer(ChainTestKind);
    r.add_layer(ModTestKind);
    for k in TEST_BLOCKS {
        r.add_block(k);
    }
    r
}

/// The settings of the test rule blocks: a number and a who.
#[derive(Settings)]
pub struct TestBlock {
    #[setting(label = "Amount", range(0, 100))]
    pub amount: f32,
    #[setting(label = "Who")]
    pub who: crate::rules::Who,
}

impl Default for TestBlock {
    fn default() -> Self {
        Self { amount: 1.0, who: crate::rules::Who::That }
    }
}

/// A test rule block kind of one section, by id.
#[derive(Clone, Copy)]
pub struct TestBlockKind(pub &'static str, pub crate::rules::BlockSection);

/// One test block kind per section: `when.test`, `if.test`, `logic.test`, `do.test`.
pub const TEST_BLOCKS: [TestBlockKind; 4] = [
    TestBlockKind("when.test", crate::rules::BlockSection::When),
    TestBlockKind("if.test", crate::rules::BlockSection::If),
    TestBlockKind("logic.test", crate::rules::BlockSection::Logic),
    TestBlockKind("do.test", crate::rules::BlockSection::Do),
];

impl crate::rules::RuleBlockKind for TestBlockKind {
    fn id(&self) -> &'static str {
        self.0
    }
    fn name(&self) -> &'static str {
        self.0
    }
    fn help(&self) -> &'static str {
        "A test block."
    }
    fn section(&self) -> crate::rules::BlockSection {
        self.1
    }
    fn settings(&self) -> &'static [SettingDef] {
        TestBlock::defs()
    }
}

/// Adds a bare thing (no slots) under `parent` at the end of the scene's order.
pub fn push(scene: &mut Scene, kind: &str, parent: ThingId) -> ThingId {
    let id = ThingId(scene.next_thing);
    scene.next_thing += 1;
    let colour = Some(scene.new_colour());
    let thing = Thing {
        id,
        name: format!("{kind} {}", id.0),
        kind: kind.into(),
        parent: Some(parent),
        colour,
        ..Thing::default()
    };
    scene.things.insert(id, thing);
    scene.order.push(id);
    id
}
