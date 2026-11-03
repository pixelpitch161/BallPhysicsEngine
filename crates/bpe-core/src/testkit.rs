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
/// A drawn layer that can't carry masks, at most one.
pub struct TintLayerKind;
/// A mask on a drawn layer.
pub struct CutLayerKind;
/// A drawn layer whose `code` names one more setting it has: `extra`.
pub struct CodeLayerKind;

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
        flags: crate::Flags { keyframe: true, pulse: false },
        when: "",
    }]
});

#[derive(Settings)]
pub struct BaseLayer {
    #[setting(label = "Size", range(0, 4), keyframe)]
    pub size: f32,
    #[setting(label = "Colour", keyframe)]
    pub color: Color,
}

impl Default for BaseLayer {
    fn default() -> Self {
        Self { size: 1.0, color: Color::WHITE }
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
    ($kind:ident, $id:literal, $name:literal, $settings:ident, $section:ident, $repeats:literal, $base:literal, $maskable:literal) => {
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
                "look.stack"
            }
            fn base(&self) -> bool {
                $base
            }
            fn maskable(&self) -> bool {
                $maskable
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
layer_kind!(CutLayerKind, "layer.cut", "Mask", CutLayer, Mask, true, false, false);
layer_kind!(CodeLayerKind, "layer.code", "Code", CodeLayer, Drawn, true, false, true);

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
    r.add_layer(CodeLayerKind);
    r
}

/// Adds a bare thing (no slots) under `parent` at the end of the scene's order.
pub fn push(scene: &mut Scene, kind: &str, parent: ThingId) -> ThingId {
    let id = ThingId(scene.next_thing);
    scene.next_thing += 1;
    let colour = Some(scene.next_colour());
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
