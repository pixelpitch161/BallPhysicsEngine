//! Small example plugins for tests, here and in later crates.

use crate::{Color, ModuleKind, Registry, Scene, SettingDef, Settings, SlotId, Target, Thing, ThingId, ThingKind};

/// Scene defaults.
pub struct SceneKind;
/// A folder that only groups things.
pub struct FolderKind;
/// A group whose slots stay its own (children never inherit them). It refuses `sound.pluck`
/// in its Sound slot, to test `accepts`.
pub struct SealedKind;
/// A set of balls.
pub struct BallsKind;
/// A plain coloured look.
pub struct PlainLookKind;
/// A plucked-string hit sound.
pub struct PluckKind;

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
    #[setting(label = "Size", range(0.5, 200), unit = "px", simple, keyframe, random)]
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
    #[setting(label = "Glow", range(0, 2), simple, keyframe, random, pulse)]
    pub glow: f32,
}

impl Default for PlainLook {
    fn default() -> Self {
        Self { color: Color::WHITE, glow: 0.0 }
    }
}

#[derive(Settings)]
pub struct Pluck {
    #[setting(label = "Volume", range(-40, 6), unit = "dB", simple, keyframe)]
    pub volume: f32,
    /// Which hits make the sound.
    #[setting(label = "Plays when hitting")]
    pub target: Target,
    #[setting(label = "Chords")]
    pub chords: crate::ProgressionRef,
    #[setting(label = "Instrument")]
    pub instrument: crate::InstrumentRef,
}

impl Default for Pluck {
    fn default() -> Self {
        Self {
            volume: 0.0,
            target: Target::Anything,
            chords: crate::ProgressionRef(None),
            instrument: crate::InstrumentRef(None),
        }
    }
}

const BALL_SLOTS: &[SlotId] =
    &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce, SlotId::Sound, SlotId::Particles, SlotId::Rules];

impl ThingKind for SceneKind {
    fn id(&self) -> &'static str {
        "scene"
    }
    fn name(&self) -> &'static str {
        "Scene defaults"
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

impl ThingKind for SealedKind {
    fn id(&self) -> &'static str {
        "sealed"
    }
    fn name(&self) -> &'static str {
        "Sealed"
    }
    fn icon(&self) -> &'static str {
        "folder"
    }
    fn help(&self) -> &'static str {
        "Keeps its slots to itself."
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
    fn passes_down(&self) -> bool {
        false
    }
    fn accepts(&self, slot: SlotId, module_kind: &str) -> bool {
        !(slot == SlotId::Sound && module_kind == "sound.pluck")
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
        &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound]
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

impl ModuleKind for PluckKind {
    fn id(&self) -> &'static str {
        "sound.pluck"
    }
    fn name(&self) -> &'static str {
        "Pluck"
    }
    fn help(&self) -> &'static str {
        "A short plucked note on each hit."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        Pluck::defs()
    }
}

/// A registry with every test plugin.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(FolderKind);
    r.add_thing(SealedKind);
    r.add_thing(BallsKind);
    r.add_module(PlainLookKind);
    r.add_module(PluckKind);
    r
}

/// Adds a bare thing (no slots) under `parent` at the end of the scene's order.
pub fn push(scene: &mut Scene, kind: &str, parent: ThingId) -> ThingId {
    let id = ThingId(scene.next_thing);
    scene.next_thing += 1;
    let thing =
        Thing { id, name: format!("{kind} {}", id.0), kind: kind.into(), parent: Some(parent), ..Thing::default() };
    scene.things.insert(id, thing);
    scene.order.push(id);
    id
}
