# Modular Ball Studio, Stage 1 (Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `bpe-core`, the data model of the new modular Ball Studio: things in a tree, modules in a library, one settings schema, a plugin registry, value resolution, addresses, save and load, and an automatic plugin contract check.

**Architecture:** Two new crates. `bpe-settings-derive` is a proc macro that turns a plain Rust struct into a settings schema. `bpe-core` holds the scene data (sparse `Params` maps keyed by setting name, everything in `BTreeMap`s for a fixed order), the registry traits that plugins implement, and pure functions that edit, resolve and save scenes. Nothing in this stage simulates, draws or plays sound; later stages build on these types.

**Tech Stack:** Rust 2024 edition workspace, serde 1, ron 0.12, glam 0.34 (serde feature), syn 2 / quote 1 / proc-macro2 1 for the derive, bpe-graph (existing, unchanged).

**Spec:** `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (sections 3, 5.6, 6, 7 stage 1, 8)

## Global Constraints

- Old crates (`bpe-scene`, `bpe-sim`, `bpe-render`, `bpe-audio`, `bpe-export`, `bpe-app`, `bpe-music`, `bpe-graph`) are not modified. `cargo test --workspace` must still pass at the end of every task.
- Deviation from spec section 6, decided here: `bpe-graph` stays unchanged, because old crates build `Node` and `Graph` with struct literals and new fields would break them. The template id, the custom flag and the Simple/More marks live on `bpe-core`'s `ModuleGraph` (Task 4) instead.
- New crates use `version.workspace`, `edition.workspace`, `license.workspace` and workspace dependencies. They are picked up by the existing `members = ["crates/*"]`.
- Every map whose order can reach a saved file, a resolve result or a later simulation is a `BTreeMap` or `BTreeSet`. No `HashMap`.
- Saved scenes are RON with a top-level `format: 2`.
- Exact error text: old file → `This scene was made with the old Ball Studio`; newer file → `This scene was made with a newer version of Ball Studio`.
- `ThingId(0)` is always Scene defaults, of kind `"scene"`. It cannot be removed or re-parented.
- Ids are never reused within a scene, including after saving and loading.
- Code style follows the repo: `rustfmt.toml` (max width 120), plain-English doc comments on public items, the same density as `bpe-graph/src/lib.rs`.

## Review Focus

1. **A hand-edited file with a wrong-typed value** (text where a number belongs) should still open. The value is dropped and the setting falls back to its default. Test: `load_drops_values_of_the_wrong_type` (Task 5).
2. **A very deep tree** (a long chain of riders) must not overflow the stack during resolve or cycle checks. Test: `resolve_handles_a_10000_deep_chain` (Task 8).
3. **A module linked into the wrong slot** (a Look module in a Sound slot, from a hand edit) must not be used. It is reported, and resolution falls back up the tree. Test: `wrong_slot_module_is_reported_and_skipped` (Task 8).
4. **Removing a thing whose descendants are targeted** by other things must report every dangling target, not just the removed thing's own. Test: `removing_a_subtree_reports_targets_into_it` (Task 8).
5. **Ids after save and load:** a removed thing's id must not come back for the next added thing after a reload. Test: `ids_are_not_reused_after_reload` (Task 6).

---

## File Structure

```
crates/bpe-settings-derive/
  Cargo.toml
  src/lib.rs            #[derive(Settings)] and #[derive(Choice)]
crates/bpe-core/
  Cargo.toml
  src/lib.rs            module list and re-exports; `extern crate self as bpe_core;`
  src/color.rs          Color, ported from bpe-scene/src/color.rs
  src/value.rs          Value, ValueType, Params
  src/settings.rs       SettingDef, Depth, Flags, Settings, SettingValue, Choice traits and impls
  src/ids.rs            ThingId, ModuleId, KindId, SlotId
  src/registry.rs       ThingKind, ModuleKind, Registry
  src/scene.rs          Scene, Thing, Attach, Transform, SlotLink, Target
  src/library.rs        Library, Module, ModuleGraph
  src/save.rs           save, load, LoadError
  src/edit.rs           tree and library edits, EditError
  src/resolve.rs        resolve, Resolved, ResolvedThing, ResolvedSlot, Problem
  src/address.rs        Address, Owner, get and set
  src/contract.rs       check_registry
  src/testkit.rs        test plugins (cfg(any(test, feature = "testkit")))
  tests/derive.rs       derive macro tests
```

---

### Task 1: Values and colours

**Files:**
- Create: `crates/bpe-core/Cargo.toml`, `crates/bpe-core/src/lib.rs`, `crates/bpe-core/src/color.rs`, `crates/bpe-core/src/value.rs`
- Modify: `Cargo.toml` (workspace dependencies: add `bpe-core = { path = "crates/bpe-core" }`, `bpe-settings-derive = { path = "crates/bpe-settings-derive" }`)

**Interfaces:**
- Produces:
  - `pub struct Color { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }`: copy `bpe-scene/src/color.rs` whole (hex serde, `rgb`, `additive`, `is_additive`, `to_hex`, `from_hex`, `from_hsv`, `to_hsv`, `shift_hue` and its tests).
  - `pub enum Value { Number(f64), Int(i64), Bool(bool), Vec2([f32; 2]), Color(Color), Colors(Vec<Color>), Choice(String), Text(String), Target(Target) }`. `Target` is defined in Task 4, so add this variant in Task 4.
  - `pub enum ValueType { Number, Int, Bool, Vec2, Color, Colors, Choice(&'static [&'static str]), Text, Target }` with `pub fn accepts(&self, v: &Value) -> bool`. A Choice accepts only names in its list.
  - `#[serde(transparent)] pub struct Params(pub BTreeMap<String, Value>)` with `get(&self, key) -> Option<&Value>`, `set(&mut self, key: &str, v: Value)`, `remove(&mut self, key) -> Option<Value>`, `is_empty`, `iter`, and `merged(&self, over: &Params) -> Params` (keys in `over` win).

- [ ] **Step 1: Write failing tests** in `value.rs`

```rust
#[test]
fn params_round_trip_through_ron() {
    let mut p = Params::default();
    p.set("glow", Value::Number(0.7));
    p.set("color", Value::Color(Color::rgb(242, 92, 92)));
    let text = ron::to_string(&p).unwrap();
    assert_eq!(ron::from_str::<Params>(&text).unwrap(), p);
}

#[test]
fn merged_prefers_the_override() {
    let mut base = Params::default();
    base.set("glow", Value::Number(0.2));
    base.set("size", Value::Number(4.5));
    let mut over = Params::default();
    over.set("glow", Value::Number(0.7));
    let m = base.merged(&over);
    assert_eq!(m.get("glow"), Some(&Value::Number(0.7)));
    assert_eq!(m.get("size"), Some(&Value::Number(4.5)));
}

#[test]
fn choice_type_accepts_only_listed_names() {
    let t = ValueType::Choice(&["circle", "box"]);
    assert!(t.accepts(&Value::Choice("box".into())));
    assert!(!t.accepts(&Value::Choice("star".into())));
    assert!(!t.accepts(&Value::Number(1.0)));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core`. Expected: FAIL (crate or items missing).
- [ ] **Step 3: Implement** the crate, `Color` port and `value.rs` as above. `Value` derives `Clone, Debug, PartialEq, Serialize, Deserialize`.
- [ ] **Step 4: Run** `cargo test -p bpe-core && cargo test --workspace`. Expected: PASS, including the ported colour tests.
- [ ] **Step 5: Commit** `git commit -m "Add bpe-core with colours and sparse setting values"`

---

### Task 2: Settings schema and derive macro

**Files:**
- Create: `crates/bpe-settings-derive/Cargo.toml` (`proc-macro = true`; deps `syn = { version = "2", features = ["full"] }`, `quote = "1"`, `proc-macro2 = "1"`), `crates/bpe-settings-derive/src/lib.rs`, `crates/bpe-core/src/settings.rs`, `crates/bpe-core/tests/derive.rs`

**Interfaces:**
- Consumes: `Value`, `ValueType`, `Params`, `Color` (Task 1).
- Produces in `bpe_core::settings` (re-exported at crate root along with the derives):
  - `pub enum Depth { Simple, More }`
  - `pub struct Flags { pub keyframe: bool, pub random: bool, pub pulse: bool }`
  - `pub struct SettingDef { pub key: &'static str, pub label: &'static str, pub help: &'static str, pub ty: ValueType, pub default: Value, pub range: Option<(f64, f64)>, pub unit: &'static str, pub group: &'static str, pub depth: Depth, pub flags: Flags }` with `pub fn clamp(&self, v: Value) -> Value` (clamps Number and Int into `range`, leaves others).
  - `pub trait SettingValue: Sized { fn value_type() -> ValueType; fn to_value(&self) -> Value; fn from_value(v: &Value) -> Option<Self>; }`, implemented for `f32`, `f64`, `i32`, `u32`, `bool`, `[f32; 2]`, `glam::Vec2`, `Color`, `Vec<Color>`, `String`, and for any `T: Choice`.
  - `pub trait Choice: Sized + Copy { const NAMES: &'static [&'static str]; fn name(self) -> &'static str; fn from_name(n: &str) -> Option<Self>; }`
  - `pub trait Settings: Sized + Default { fn defs() -> &'static [SettingDef]; fn from_params(p: &Params) -> Self; fn to_params(&self) -> Params; }`. `from_params` uses the default for missing or wrong-typed keys. `to_params` writes only fields that differ from `Default`.
- Macro attributes on fields: `#[setting(label = "...", unit = "...", group = "...", range(min, max), simple, keyframe, random, pulse)]`. `help` is the field's `///` doc text joined into one line. `key` is the field name. `depth` is `Simple` when `simple` is present, else `More`. `label` is required (compile error without it). The generated `defs()` builds the list once in a `std::sync::OnceLock`. Generated paths start with `::bpe_core::`. `bpe-core/src/lib.rs` has `extern crate self as bpe_core;`. `#[derive(Choice)]` works on enums of unit variants, and its names are the variant names in snake_case.

- [ ] **Step 1: Write failing tests** in `tests/derive.rs`

```rust
use bpe_core::{Choice, Color, Depth, Params, Settings, Value};

#[derive(Clone, Copy, Debug, PartialEq, Default, Choice)]
enum Shape { #[default] Circle, RoundBox }

#[derive(Debug, PartialEq, Settings)]
struct Look {
    /// How brightly the halo shines.
    #[setting(label = "Glow", range(0.0, 2.0), simple, keyframe, random, pulse)]
    glow: f32,
    #[setting(label = "Colour", simple)]
    color: Color,
    #[setting(label = "Shape")]
    shape: Shape,
}
impl Default for Look {
    fn default() -> Self { Self { glow: 0.5, color: Color::WHITE, shape: Shape::Circle } }
}

#[test]
fn defs_describe_each_field() {
    let d = Look::defs();
    assert_eq!(d.iter().map(|d| d.key).collect::<Vec<_>>(), ["glow", "color", "shape"]);
    assert_eq!(d[0].help, "How brightly the halo shines.");
    assert_eq!(d[0].range, Some((0.0, 2.0)));
    assert_eq!(d[0].default, Value::Number(0.5));
    assert!(d[0].flags.keyframe && d[0].flags.random && d[0].flags.pulse);
    assert_eq!(d[2].depth, Depth::More);
    assert_eq!(Shape::NAMES, &["circle", "round_box"]);
}

#[test]
fn to_params_is_sparse_and_from_params_fills_defaults() {
    assert!(Look::default().to_params().is_empty());
    let mut p = Params::default();
    p.set("glow", Value::Number(1.5));
    p.set("shape", Value::Choice("round_box".into()));
    let look = Look::from_params(&p);
    assert_eq!(look, Look { glow: 1.5, shape: Shape::RoundBox, ..Look::default() });
    assert_eq!(Look::from_params(&look.to_params()), look);
}

#[test]
fn wrong_typed_values_fall_back_to_default() {
    let mut p = Params::default();
    p.set("glow", Value::Text("bright".into()));
    assert_eq!(Look::from_params(&p).glow, 0.5);
}

#[test]
fn clamp_keeps_numbers_in_range() {
    assert_eq!(Look::defs()[0].clamp(Value::Number(9.0)), Value::Number(2.0));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core --test derive`. Expected: FAIL (derives missing).
- [ ] **Step 3: Implement** `settings.rs` and both derives. Store `f32` and `f64` as `Value::Number`, integers as `Value::Int`. `from_value` for integers accepts `Int` only.
- [ ] **Step 4: Run** `cargo test -p bpe-core && cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add the settings schema and its derive macro"`

---

### Task 3: Ids, slots and the registry

**Files:**
- Create: `crates/bpe-core/src/ids.rs`, `crates/bpe-core/src/registry.rs`, `crates/bpe-core/src/testkit.rs`
- Modify: `crates/bpe-core/Cargo.toml` (feature `testkit = []`), `src/lib.rs`

**Interfaces:**
- Consumes: `SettingDef`, `Settings` (Task 2).
- Produces:
  - `pub struct ThingId(pub u32)` and `pub struct ModuleId(pub u32)`, both `Copy, Ord, Hash, Serialize, Deserialize`. `ThingId::ROOT = ThingId(0)`.
  - `pub type KindId = String;`
  - `pub enum SlotId { Look, Trail, Motion, Bounce, Sound, Particles, Rules, Shader }`, `Copy, Ord, Serialize, Deserialize`, with `pub const ALL: [SlotId; 8]` and `pub fn name(self) -> &'static str` ("Look", "Trail", ...).
  - `pub trait ThingKind: Send + Sync + 'static { fn id(&self) -> &'static str; fn name(&self) -> &'static str; fn icon(&self) -> &'static str; fn help(&self) -> &'static str; fn settings(&self) -> &'static [SettingDef]; fn slots(&self) -> &'static [SlotId]; fn default_slots(&self) -> &'static [SlotId]; }`
  - `pub trait ModuleKind: Send + Sync + 'static { fn id(&self) -> &'static str; fn name(&self) -> &'static str; fn help(&self) -> &'static str; fn slot(&self) -> SlotId; fn settings(&self) -> &'static [SettingDef]; fn template(&self) -> Option<bpe_graph::Graph> { None } }`
  - `pub struct Registry`, with `new()`, `add_thing(&mut self, k: impl ThingKind)`, `add_module(&mut self, k: impl ModuleKind)`, `thing(&self, id: &str) -> Option<&dyn ThingKind>`, `module(&self, id: &str) -> Option<&dyn ModuleKind>`, `things(&self) -> impl Iterator<Item = &dyn ThingKind>`, `modules_for(&self, slot: SlotId) -> impl Iterator<Item = &dyn ModuleKind>`. Registering a second kind with the same id panics with `"plugin id registered twice: <id>"`. Iteration is in id order.
  - `testkit` (public behind `cfg(any(test, feature = "testkit"))`): unit structs `SceneKind`, `FolderKind`, `BallsKind`, `PlainLookKind`, `PluckKind` implementing the traits, and `pub fn registry() -> Registry` registering:
    - `"scene"`: slots and default slots = all eight. Settings `SceneSettings { duration: f32 = 30.0, range(1, 600), unit "s" }`.
    - `"folder"`: slots = all, default slots = none, no settings.
    - `"balls"`: slots `[Look, Trail, Motion, Bounce, Sound, Particles, Rules]`, default slots `[Look, Motion, Bounce, Sound]`. Settings `BallsSettings { count: u32 = 1, range(1, 10000), simple; radius: f32 = 6.0, range(0.5, 200), unit "px", simple, keyframe, random }`.
    - `"look.plain"` (slot Look): `PlainLook { color: Color = WHITE, simple, keyframe; glow: f32 = 0.0, range(0, 2), simple, keyframe, random, pulse }`.
    - `"sound.pluck"` (slot Sound): `Pluck { volume: f32 = 0.0, range(-40, 6), unit "dB", simple; target: Target = Anything }`. `Target` comes in Task 4; until then omit `target` and add it there.

- [ ] **Step 1: Write failing tests** in `registry.rs`

```rust
#[test]
fn registry_finds_kinds_by_id_and_slot() {
    let r = testkit::registry();
    assert_eq!(r.thing("balls").unwrap().default_slots(), &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound]);
    assert_eq!(r.modules_for(SlotId::Look).map(|m| m.id()).collect::<Vec<_>>(), ["look.plain"]);
    assert!(r.thing("magnet").is_none());
}

#[test]
#[should_panic(expected = "plugin id registered twice: balls")]
fn duplicate_ids_panic() {
    let mut r = testkit::registry();
    r.add_thing(testkit::BallsKind);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core registry`. Expected: FAIL.
- [ ] **Step 3: Implement** `ids.rs`, `registry.rs` and `testkit.rs`. `Registry` stores `BTreeMap<&'static str, Box<dyn ThingKind>>` and the same for modules.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add ids, slots and the plugin registry"`

---

### Task 4: Scene, things and library

**Files:**
- Create: `crates/bpe-core/src/scene.rs`, `crates/bpe-core/src/library.rs`
- Modify: `src/value.rs` (add `Value::Target`, `ValueType::Target`), `src/settings.rs` (`SettingValue for Target`), `src/testkit.rs` (add `Pluck.target`)

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces (all `Clone, Debug, PartialEq, Serialize, Deserialize`; `#[serde(default)]` on structs; fields equal to their default are skipped when saving where serde allows):
  - `pub enum Target { Me, Thing(ThingId), Tag(String), Kind(KindId), Anything }` (default `Anything`).
  - `pub struct Transform { pub position: glam::Vec2, pub rotation: f32 }`
  - `pub enum Attach { Group, Ride { offset: glam::Vec2, turn_with_parent: bool, item: Option<u32> } }` (default `Group`).
  - `pub enum SlotLink { Inherit, Linked { module: ModuleId, overrides: Params }, Off }`
  - `pub struct Thing { pub id: ThingId, pub name: String, pub kind: KindId, pub tags: BTreeSet<String>, pub parent: Option<ThingId>, pub attach: Attach, pub transform: Transform, pub enabled: bool, pub params: Params, pub slots: BTreeMap<SlotId, SlotLink> }`. `parent` is `None` only for the root. A slot missing from `slots` behaves as `Inherit` and is not shown in the inspector.
  - `pub struct ModuleGraph { pub graph: bpe_graph::Graph, pub template: String, pub custom: bool, pub simple: BTreeSet<(u32, String)> }` (node id, setting name pairs shown at Simple depth). Stored now, used from stage 4.
  - `pub struct Module { pub id: ModuleId, pub name: String, pub slot: SlotId, pub kind: KindId, pub params: Params, pub graph: Option<ModuleGraph> }`
  - `pub struct Library { pub modules: BTreeMap<ModuleId, Module> }`
  - `pub struct Scene { pub format: u32, pub name: String, pub things: BTreeMap<ThingId, Thing>, pub order: Vec<ThingId>, pub library: Library, pub next_thing: u32, pub next_module: u32 }`. `order` lists every thing once, root first. Siblings appear in `order` in tree order.
  - `impl Scene { pub const FORMAT: u32 = 2; pub fn empty(reg: &Registry) -> Scene; pub fn children(&self, id: ThingId) -> Vec<ThingId>; pub fn ancestors(&self, id: ThingId) -> Vec<ThingId> /* nearest first, iterative */; pub fn users_of(&self, m: ModuleId) -> Vec<ThingId>; }`
  - `Scene::empty` makes the root (`ThingId(0)`, kind `"scene"`, name `"Scene defaults"`) with every default slot of the scene kind set to `Inherit`. It panics if `reg` has no `"scene"` kind. It sets `next_thing = 1`.

- [ ] **Step 1: Write failing tests** in `scene.rs`

```rust
#[test]
fn empty_scene_has_only_the_root() {
    let s = Scene::empty(&testkit::registry());
    assert_eq!(s.order, vec![ThingId::ROOT]);
    assert_eq!(s.things[&ThingId::ROOT].name, "Scene defaults");
    assert_eq!(s.format, 2);
    assert_eq!(s.next_thing, 1);
}

#[test]
fn ancestors_are_nearest_first() {
    let mut s = Scene::empty(&testkit::registry());
    let a = testkit::push(&mut s, "folder", ThingId::ROOT);
    let b = testkit::push(&mut s, "balls", a);
    assert_eq!(s.ancestors(b), vec![a, ThingId::ROOT]);
    assert_eq!(s.children(ThingId::ROOT), vec![a]);
}
```

`testkit::push(scene, kind, parent) -> ThingId` is a test helper that inserts a bare thing (no slots) and appends it to `order`. Add it to `testkit.rs`.

- [ ] **Step 2: Run** `cargo test -p bpe-core scene`. Expected: FAIL.
- [ ] **Step 3: Implement** `scene.rs` and `library.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add the scene tree, things and module library"`

---

### Task 5: Save and load

**Files:**
- Create: `crates/bpe-core/src/save.rs`

**Interfaces:**
- Consumes: `Scene`, `Registry`, `SettingDef::clamp`.
- Produces:
  - `pub fn save(scene: &Scene) -> String` (pretty RON).
  - `pub enum LoadError { OldEditor, Newer(u32), Broken(String) }` implementing `Display` with the exact texts from Global Constraints. `Broken` shows `This scene file is damaged: <parser message>`.
  - `pub fn load(text: &str, reg: &Registry) -> Result<Scene, LoadError>`. Steps: (1) parse a probe struct `{ format: Option<u32> }` that ignores other fields; `None` gives `OldEditor`, above `Scene::FORMAT` gives `Newer`; (2) parse the full `Scene`; (3) for each thing's `params` (defs from its kind) and each module's `params` (defs from its module kind), and for each `Linked` override (defs from the linked module's kind), drop values the def's `ty` doesn't accept and clamp the rest. Keys with no def and kinds the registry doesn't know are left untouched (resolve reports them). (4) Raise `next_thing` and `next_module` above the largest id present.

- [ ] **Step 1: Write failing tests** in `save.rs`

```rust
#[test]
fn scene_round_trips() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    testkit::push(&mut s, "balls", ThingId::ROOT);
    assert_eq!(load(&save(&s), &r).unwrap(), s);
}

#[test]
fn old_editor_files_are_refused_with_a_clear_message() {
    let old = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/reference-reel.ron")).unwrap();
    let e = load(&old, &testkit::registry()).unwrap_err();
    assert_eq!(e.to_string(), "This scene was made with the old Ball Studio");
}

#[test]
fn newer_files_are_refused() {
    let e = load("(format: 3)", &testkit::registry()).unwrap_err();
    assert_eq!(e.to_string(), "This scene was made with a newer version of Ball Studio");
}

#[test]
fn load_clamps_out_of_range_values() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let b = testkit::push(&mut s, "balls", ThingId::ROOT);
    s.things.get_mut(&b).unwrap().params.set("radius", Value::Number(5000.0));
    let back = load(&save(&s), &r).unwrap();
    assert_eq!(back.things[&b].params.get("radius"), Some(&Value::Number(200.0)));
}

#[test]
fn load_drops_values_of_the_wrong_type() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let b = testkit::push(&mut s, "balls", ThingId::ROOT);
    s.things.get_mut(&b).unwrap().params.set("radius", Value::Text("big".into()));
    assert_eq!(load(&save(&s), &r).unwrap().things[&b].params.get("radius"), None);
}
```

Check the path to `scenes/reference-reel.ron` exists in the repo before relying on it. If it moved, use any old `.ron` scene under `scenes/`.

- [ ] **Step 2: Run** `cargo test -p bpe-core save`. Expected: FAIL.
- [ ] **Step 3: Implement** `save.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Save and load scenes in the new format"`

---

### Task 6: Tree edits

**Files:**
- Create: `crates/bpe-core/src/edit.rs`

**Interfaces:**
- Consumes: `Scene`, `Registry`.
- Produces:
  - `pub enum EditError { UnknownKind(KindId), Missing(ThingId), MissingModule(ModuleId), Root, WouldLoop, SlotNotSupported(SlotId), WrongSlot, NotLinked, InUse(Vec<ThingId>), WrongType, UnknownSetting(String) }`, with `Display` in plain English. `WouldLoop` shows `A thing can't ride on itself or on something inside it`.
  - `impl Scene`:
    - `pub fn add_thing(&mut self, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError>`. The new thing takes id `next_thing` (then increment), is named after the kind's `name()` plus a number when that name is taken ("Balls", "Balls 2"), gets its kind's default slots as `Inherit`, and goes last among the parent's children in `order`.
    - `pub fn set_parent(&mut self, id: ThingId, parent: ThingId, attach: Attach) -> Result<(), EditError>`. Refuses the root (`Root`) and refuses when `parent` is `id` or one of its descendants (`WouldLoop`). Use an iterative walk of `ancestors(parent)`. Moves `id`'s subtree in `order` to the end of `parent`'s children, keeping its internal order.
    - `pub fn move_before(&mut self, id: ThingId, sibling: ThingId) -> Result<(), EditError>` reorders among siblings (same parent required, else `Missing`).
    - `pub fn remove_thing(&mut self, id: ThingId) -> Result<Vec<ThingId>, EditError>` removes the subtree and returns the removed ids. Refuses the root.
    - `pub fn add_slot(&mut self, reg: &Registry, id: ThingId, slot: SlotId) -> Result<(), EditError>` inserts `Inherit`. Gives `SlotNotSupported` if the kind doesn't list it.

- [ ] **Step 1: Write failing tests** in `edit.rs`

```rust
#[test]
fn add_thing_names_and_slots_from_the_kind() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    assert_eq!((s.things[&a].name.as_str(), s.things[&b].name.as_str()), ("Balls", "Balls 2"));
    assert_eq!(s.things[&a].slots.keys().copied().collect::<Vec<_>>(), [SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound]);
    assert!(matches!(s.add_thing(&r, "magnet", ThingId::ROOT), Err(EditError::UnknownKind(_))));
}

#[test]
fn riding_on_a_descendant_is_refused() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "balls", f).unwrap();
    assert!(matches!(s.set_parent(f, b, Attach::Group), Err(EditError::WouldLoop)));
    assert!(matches!(s.set_parent(ThingId::ROOT, f, Attach::Group), Err(EditError::Root)));
}

#[test]
fn set_parent_moves_the_whole_subtree() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "balls", f).unwrap();
    let g = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    s.set_parent(f, g, Attach::Group).unwrap();
    assert_eq!(s.order, vec![ThingId::ROOT, g, f, b]);
}

#[test]
fn ids_are_not_reused_after_reload() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    s.remove_thing(a).unwrap();
    let mut back = crate::save::load(&crate::save::save(&s), &r).unwrap();
    assert_ne!(back.add_thing(&r, "balls", ThingId::ROOT).unwrap(), a);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core edit`. Expected: FAIL.
- [ ] **Step 3: Implement** the tree edits.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add, move and remove things in the scene tree"`

---

### Task 7: Library edits

**Files:**
- Modify: `crates/bpe-core/src/edit.rs`

**Interfaces:**
- Consumes: Task 6.
- Produces, on `impl Scene`:
  - `pub fn add_module(&mut self, reg: &Registry, kind: &str, name: &str) -> Result<ModuleId, EditError>`. The slot comes from the module kind and the params start empty.
  - `pub fn link(&mut self, reg: &Registry, id: ThingId, slot: SlotId, m: ModuleId) -> Result<(), EditError>` sets `Linked { module: m, overrides: empty }`. It gives `WrongSlot` if the module's slot differs, and `SlotNotSupported` if the thing's kind doesn't offer the slot.
  - `pub fn set_slot_off(&mut self, id: ThingId, slot: SlotId)` and `pub fn set_slot_inherit(&mut self, id: ThingId, slot: SlotId)`.
  - `pub fn make_unique(&mut self, id: ThingId, slot: SlotId) -> Result<ModuleId, EditError>`. It copies the linked module under the name `"<module name> (<thing name>)"`, merges the overrides into the copy's params, links the thing to the copy and clears the overrides. It gives `NotLinked` unless the slot is `Linked`.
  - `pub fn push_to_module(&mut self, id: ThingId, slot: SlotId) -> Result<(), EditError>` merges the overrides into the shared module and clears them.
  - `pub fn reset_override(&mut self, id: ThingId, slot: SlotId, key: &str) -> Result<(), EditError>`.
  - `pub fn remove_module(&mut self, m: ModuleId) -> Result<(), EditError>` gives `InUse(users)` while any thing links to it.

- [ ] **Step 1: Write failing tests**

```rust
fn shared_look() -> (Registry, Scene, ThingId, ThingId, ModuleId) {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let m = s.add_module(&r, "look.plain", "Neon glow").unwrap();
    s.link(&r, a, SlotId::Look, m).unwrap();
    s.link(&r, b, SlotId::Look, m).unwrap();
    (r, s, a, b, m)
}

fn override_glow(s: &mut Scene, id: ThingId, v: f64) {
    if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look) {
        overrides.set("glow", Value::Number(v));
    }
}

#[test]
fn make_unique_copies_with_overrides_baked_in() {
    let (_, mut s, a, b, m) = shared_look();
    override_glow(&mut s, a, 0.7);
    let copy = s.make_unique(a, SlotId::Look).unwrap();
    assert_ne!(copy, m);
    assert_eq!(s.library.modules[&copy].name, "Neon glow (Balls)");
    assert_eq!(s.library.modules[&copy].params.get("glow"), Some(&Value::Number(0.7)));
    assert_eq!(s.users_of(m), vec![b]);
}

#[test]
fn push_to_module_changes_every_user() {
    let (_, mut s, a, _, m) = shared_look();
    override_glow(&mut s, a, 0.7);
    s.push_to_module(a, SlotId::Look).unwrap();
    assert_eq!(s.library.modules[&m].params.get("glow"), Some(&Value::Number(0.7)));
}

#[test]
fn modules_in_use_cannot_be_removed() {
    let (_, mut s, a, b, m) = shared_look();
    assert!(matches!(s.remove_module(m), Err(EditError::InUse(u)) if u == vec![a, b]));
}

#[test]
fn linking_into_the_wrong_slot_is_refused() {
    let (r, mut s, a, _, m) = shared_look();
    assert!(matches!(s.link(&r, a, SlotId::Sound, m), Err(EditError::WrongSlot)));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core edit`. Expected: FAIL.
- [ ] **Step 3: Implement** the library edits.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Share, override and copy modules"`

---

### Task 8: Resolution

**Files:**
- Create: `crates/bpe-core/src/resolve.rs`

**Interfaces:**
- Consumes: Tasks 3-7.
- Produces:
  - `pub struct ResolvedSlot { pub kind: KindId, pub module: ModuleId, pub from: ThingId, pub params: Params }`. `params` is full: every def's key is present. `from` is the thing whose link supplied the module.
  - `pub struct ResolvedThing { pub id: ThingId, pub kind: KindId, pub params: Params /* full */, pub slots: BTreeMap<SlotId, ResolvedSlot> }`
  - `pub enum ProblemKind { UnknownKind(KindId), MissingModule(ModuleId), WrongSlot(ModuleId), UnknownModuleKind(KindId), MissingTarget(ThingId) }`
  - `pub struct Problem { pub thing: ThingId, pub slot: Option<SlotId>, pub kind: ProblemKind }`
  - `pub struct Resolved { pub things: BTreeMap<ThingId, ResolvedThing>, pub problems: Vec<Problem> }`
  - `pub fn resolve(scene: &Scene, reg: &Registry) -> Resolved`
- Rules (spec 3.3):
  - Disabled things and their subtrees are left out of `things`.
  - A thing of an unknown kind gives `UnknownKind`, and it and its subtree are left out.
  - Resolve each slot the thing's kind offers, independently. Walk from the thing up through `ancestors` (iteratively). The first `Off` ends the walk with no slot. At the first `Linked { module, overrides }`: if the module is missing (`MissingModule`), its slot differs (`WrongSlot`) or its kind is unknown (`UnknownModuleKind`), record the problem against the thing that owns the link and keep walking up. Otherwise the slot is that module, with params = every def's default, then the module's params, then that link's overrides. `Inherit` or an absent slot keeps walking. Reaching past the root gives no slot.
  - Thing params = every def's default, then the thing's params.
  - Every `Value::Target(Target::Thing(id))` in any resolved params whose `id` isn't in `scene.things` gives `MissingTarget(id)` on the thing being resolved.
  - Problems are listed in thing order, then slot order.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn child_link_beats_parent_and_overrides_win() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    s.add_slot(&r, f, SlotId::Look).unwrap();
    let b = s.add_thing(&r, "balls", f).unwrap();
    let dim = s.add_module(&r, "look.plain", "Dim").unwrap();
    let neon = s.add_module(&r, "look.plain", "Neon").unwrap();
    s.library.modules.get_mut(&neon).unwrap().params.set("glow", Value::Number(1.0));
    s.link(&r, f, SlotId::Look, dim).unwrap();
    let res = resolve(&s, &r);
    assert_eq!(res.things[&b].slots[&SlotId::Look].module, dim);
    assert_eq!(res.things[&b].slots[&SlotId::Look].from, f);
    s.link(&r, b, SlotId::Look, neon).unwrap();
    // override on b
    if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Look) {
        overrides.set("glow", Value::Number(0.3));
    }
    let look = &resolve(&s, &r).things[&b].slots[&SlotId::Look];
    assert_eq!((look.module, look.params.get("glow")), (neon, Some(&Value::Number(0.3))));
    assert_eq!(look.params.get("color"), Some(&Value::Color(Color::WHITE)));
}

#[test]
fn off_stops_inheritance() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let m = s.add_module(&r, "look.plain", "Plain").unwrap();
    s.link(&r, ThingId::ROOT, SlotId::Look, m).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    assert!(resolve(&s, &r).things[&b].slots.contains_key(&SlotId::Look));
    s.set_slot_off(b, SlotId::Look);
    assert!(!resolve(&s, &r).things[&b].slots.contains_key(&SlotId::Look));
}

#[test]
fn wrong_slot_module_is_reported_and_skipped() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let plain = s.add_module(&r, "look.plain", "Plain").unwrap();
    let pluck = s.add_module(&r, "sound.pluck", "Pluck").unwrap();
    s.link(&r, ThingId::ROOT, SlotId::Look, plain).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    s.things.get_mut(&b).unwrap().slots.insert(SlotId::Look, SlotLink::Linked { module: pluck, overrides: Params::default() });
    let res = resolve(&s, &r);
    assert_eq!(res.things[&b].slots[&SlotId::Look].module, plain);
    assert_eq!(res.problems[0].kind, ProblemKind::WrongSlot(pluck));
}

#[test]
fn removing_a_subtree_reports_targets_into_it() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    let inner = s.add_thing(&r, "balls", f).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let m = s.add_module(&r, "sound.pluck", "Pluck").unwrap();
    s.library.modules.get_mut(&m).unwrap().params.set("target", Value::Target(Target::Thing(inner)));
    s.link(&r, b, SlotId::Sound, m).unwrap();
    s.remove_thing(f).unwrap();
    let res = resolve(&s, &r);
    assert!(res.problems.iter().any(|p| p.thing == b && p.kind == ProblemKind::MissingTarget(inner)));
}

#[test]
fn resolve_handles_a_10000_deep_chain() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let m = s.add_module(&r, "look.plain", "Plain").unwrap();
    s.link(&r, ThingId::ROOT, SlotId::Look, m).unwrap();
    let mut parent = ThingId::ROOT;
    for _ in 0..10_000 { parent = s.add_thing(&r, "folder", parent).unwrap(); }
    let leaf = s.add_thing(&r, "balls", parent).unwrap();
    assert_eq!(resolve(&s, &r).things[&leaf].slots[&SlotId::Look].module, m);
}
```

If the 10,000-deep test is slow in debug builds because `add_thing` rescans `order`, keep the scan and pass the test in under 10 s. Don't add caches in this stage.

- [ ] **Step 2: Run** `cargo test -p bpe-core resolve`. Expected: FAIL.
- [ ] **Step 3: Implement** `resolve.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Resolve each thing's settings from overrides, modules and parents"`

---

### Task 9: Addresses

**Files:**
- Create: `crates/bpe-core/src/address.rs`

**Interfaces:**
- Consumes: Tasks 6-8.
- Produces:
  - `pub enum Owner { Thing(ThingId), Module(ModuleId) }` and `pub struct Address { pub owner: Owner, pub slot: Option<SlotId>, pub key: String }`, all `Clone, Debug, PartialEq, Eq, Ord, Serialize, Deserialize`.
  - `impl Scene`:
    - `pub fn def_at(&self, reg: &Registry, a: &Address) -> Option<&'static SettingDef>` finds the def for the address's place: the thing kind's def (no slot), the def of the slot's linked module kind, or the module kind's def.
    - `pub fn value_at(&self, reg: &Registry, a: &Address) -> Option<Value>` is the effective value. For `Thing` with a slot it comes from `resolve` rules for that one slot. For `Thing` without a slot it is the thing's params over the defaults. For `Module` it is the module's params over the defaults.
    - `pub fn set_at(&mut self, reg: &Registry, a: &Address, v: Value) -> Result<(), EditError>` checks the type (`WrongType`), clamps, and stores the value sparsely: setting a value equal to the inherited value removes the key. `Thing` with a slot writes the override and needs that slot to be `Linked` on that thing (`NotLinked`). An unknown key gives `UnknownSetting`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn set_at_writes_sparse_clamped_overrides() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let m = s.add_module(&r, "look.plain", "Plain").unwrap();
    s.link(&r, b, SlotId::Look, m).unwrap();
    let glow = Address { owner: Owner::Thing(b), slot: Some(SlotId::Look), key: "glow".into() };
    s.set_at(&r, &glow, Value::Number(5.0)).unwrap();
    assert_eq!(s.value_at(&r, &glow), Some(Value::Number(2.0)));
    s.set_at(&r, &glow, Value::Number(0.0)).unwrap(); // equals the module's value
    assert!(matches!(&s.things[&b].slots[&SlotId::Look], SlotLink::Linked { overrides, .. } if overrides.is_empty()));
}

#[test]
fn set_at_refuses_bad_writes() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let radius = Address { owner: Owner::Thing(b), slot: None, key: "radius".into() };
    assert!(matches!(s.set_at(&r, &radius, Value::Bool(true)), Err(EditError::WrongType)));
    let nope = Address { key: "wobble".into(), ..radius.clone() };
    assert!(matches!(s.set_at(&r, &nope, Value::Number(1.0)), Err(EditError::UnknownSetting(_))));
    let look = Address { owner: Owner::Thing(b), slot: Some(SlotId::Look), key: "glow".into() };
    assert!(matches!(s.set_at(&r, &look, Value::Number(1.0)), Err(EditError::NotLinked)));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core address`. Expected: FAIL.
- [ ] **Step 3: Implement** `address.rs`. Share the slot walk with `resolve.rs` through a private `fn resolve_slot(scene, reg, id, slot, problems: &mut Vec<Problem>) -> Option<ResolvedSlot>` there, rather than duplicating it.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Name every setting by address, and read and write through it"`

---

### Task 10: Plugin contract check

**Files:**
- Create: `crates/bpe-core/src/contract.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `pub fn check_registry(reg: &Registry) -> Vec<String>`, an empty list when every plugin passes. Each failure is one line naming the plugin id and setting key. Checks:
  - the registry has a `"scene"` thing kind;
  - per kind: setting keys are unique; labels are not empty; every `range` has `min < max` and holds the default; every Choice default is in its list; every keyframe- or pulse-flagged setting is Number, Int, Vec2 or Color; at most 6 settings are `Simple` (keeps the first view short, spec 3.5); `default_slots` is a subset of `slots`;
  - per kind: params round-trip, meaning the `Params` built from every default survive `ron::to_string` then `ron::from_str` unchanged, and every default is accepted by its own `ty`;
  - per module kind: a `template()` graph, when present, has no cycle (`bpe_graph::order(&graph).is_ok()`). Full validation against a block catalog comes in stage 4;
  - `Scene::empty(reg)` then `resolve` gives no problems.
- Later stages add keyframe, randomize and template-vs-native checks here.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn testkit_plugins_pass() {
    assert_eq!(check_registry(&testkit::registry()), Vec::<String>::new());
}

#[test]
fn a_default_outside_its_range_is_caught() {
    #[derive(Settings)]
    struct Bad { #[setting(label = "Size", range(1.0, 2.0))] size: f32 }
    impl Default for Bad { fn default() -> Self { Self { size: 9.0 } } }
    struct BadKind;
    impl ThingKind for BadKind { /* id "bad", settings Bad::defs(), no slots */ }
    let mut r = testkit::registry();
    r.add_thing(BadKind);
    let fails = check_registry(&r);
    assert_eq!(fails.len(), 1);
    assert!(fails[0].contains("bad") && fails[0].contains("size"));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core contract`. Expected: FAIL.
- [ ] **Step 3: Implement** `contract.rs`.
- [ ] **Step 4: Run** the whole workspace: `cargo test --workspace` then `cargo clippy -p bpe-core -p bpe-settings-derive -- -D warnings`. Expected: all tests pass and clippy reports no warnings.
- [ ] **Step 5: Commit** `git commit -m "Check every registered plugin against the core's rules"`

---

## Done when

- `cargo test --workspace` passes with every old test unchanged.
- `bpe-core` exposes everything listed in the Interfaces blocks above. Stage 2's plan (`bpe-runtime`, `bpe-gfx`, `bpe-mix`, `bpe-kit`, `bpe-studio`) consumes them.
- No Windows build is shipped for this stage (spec stage 1: tests only).
