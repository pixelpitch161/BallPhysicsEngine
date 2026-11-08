# Scene Settings and Per-Thing Starters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Scene defaults' shared module fallbacks with a Scene settings root (whole-scene settings plus Effects, Music and Rules that never pass down), and give every new thing its own starter modules.

**Architecture:** bpe-core learns that some thing kinds don't pass their slots down (`ThingKind::passes_down`), and that a module can be owned by a thing (`Module.owner`), with naming, rename, delete-prune, make-own and duplicate rules. bpe-kit moves all starter logic into a new `starters.rs` (per-kind table, `fill_slot`, `keep_followers`, `move_thing`), turns the `scene` kind into Scene settings, and drops the "Scene's" chord and instrument fallbacks from the mix. bpe-studio wires the tree and inspector to those calls.

**Tech Stack:** Rust workspace, egui + egui_kittest (studio tests), serde/RON scene files.

**Spec:** `docs/superpowers/specs/2026-10-31-scene-settings-design.md`

## Global Constraints

- No backward compatibility for old scene files. Built-in scenes and `scenes/next/*.ron` are regenerated (`BPE_BLESS=1`).
- The root's kind id stays `"scene"` and its id stays `ThingId::ROOT`. Only its display name changes, to `"Scene settings"`.
- Starter name: `"<thing name> <slot name lowercased>"`, e.g. `"Ball look"`, `"Ball 2 sound"`.
- Scene settings slots: `[Sound, Shader, Rules]`, labelled `"Music"`, `"Effects"`, `"Rules"` on its card. Its default slots are empty. Its Sound slot accepts only `sound.drums` and `sound.melody`.
- Shared sound items: a library instrument made by `built_in_instrument(Instrument::Pluck)` (name as that returns) and a library progression `Progression::starter()`. Each is made once per scene and reused.
- `InstrumentRef(None)` means built-in Pluck. `ProgressionRef(None)` means `Progression::starter()`.
- UI copy: `"Scene settings"`, `"Follow folder"`, `"Make own"`, `"Make unique"`, `"Remove"`. The strings `"Change here"`, `"Inherit (nothing above)"`, `"Scene's"`, `"Plays Scene defaults'"` disappear from bpe-studio.
- Golden re-bless: `BLESS_GOLDEN=1` once in debug and once with `--release`. Disk fills easily, so build with `CARGO_INCREMENTAL=0` and delete `target/debug` if the volume fills.
- Test env needs `libasound2-dev` and `mesa-vulkan-drivers`. Release `cargo test` always fails bpe-runtime `pushing_an_earlier_step_panics_in_debug` (pre-existing).
- Leave `crates/bpe-app` (the old editor) untouched.

## Review Focus

1. **Moving a folder whose children follow it into another folder.** Every follower in the moved subtree keeps its module, not only the dragged thing. Test in Task 4 (`moving_a_folder_keeps_its_followers_looks`).
2. **Removing or swapping a folder's module while children follow it.** The followers keep the old module as owned copies. Test in Task 4 (`turning_a_folder_slot_off_keeps_followers`).
3. **Deleting a thing whose starter another thing shares.** The module survives, its owner is cleared, and undo restores everything. Tests in Task 2 (`removing_keeps_shared_starters_and_clears_owner`) and Task 6 (`undo_after_delete_brings_starters_back`).
4. **Duplicating a thing.** The copy gets its own copies of the source's owned starters, not shared links, so editing the copy doesn't touch the original. Test in Task 2 (`copy_owned_starters_gives_the_copy_its_own`). The spec doesn't cover this, but it follows from "own fresh modules".
5. **Renaming when a starter was renamed by hand, or the new name is blank or the same.** A hand-renamed starter keeps its name, and blank or same names change nothing. Test in Task 2 (`rename_thing_renames_only_matching_starters`).

---

### Task 1: Core: kinds that don't pass slots down

**Files:**
- Modify: `crates/bpe-core/src/registry.rs` (ThingKind trait)
- Modify: `crates/bpe-core/src/resolve.rs` (`find_slot`, `resolve_with`)
- Modify: `crates/bpe-core/src/edit.rs` (`link`, `EditError`)
- Modify: `crates/bpe-core/src/testkit.rs` (add a test kind)

**Interfaces:**
- Produces, on `ThingKind`, all with defaults:
  - `fn passes_down(&self) -> bool { true }`
  - `fn slot_label(&self, slot: SlotId) -> &'static str { slot.name() }`
  - `fn accepts(&self, slot: SlotId, module_kind: &str) -> bool { true }`
- Produces `EditError::NotAccepted` (returned by `Scene::link` when `!kind.accepts(slot, &module.kind)`).
- Produces testkit kind `"sealed"` (`SealedKind`): slots `SlotId::ALL`, default slots `[]`, `passes_down() == false`, registered in `testkit::registry()`.

- [ ] **Step 1: Write failing tests in `resolve.rs` tests**

```rust
#[test]
fn a_kind_that_does_not_pass_down_is_skipped_by_children() {
    // ROOT(look A) > sealed(look B) > balls: the balls get A, the sealed thing gets B.
    let reg = testkit::registry();
    let mut scene = Scene::empty(&reg);
    let sealed = scene.add_thing(&reg, "sealed", ThingId::ROOT).unwrap();
    let balls = scene.add_thing(&reg, "balls", sealed).unwrap();
    let a = scene.add_module(&reg, "look.plain", "A").unwrap();
    let b = scene.add_module(&reg, "look.plain", "B").unwrap();
    scene.link(&reg, ThingId::ROOT, SlotId::Look, a).unwrap();
    scene.link(&reg, sealed, SlotId::Look, b).unwrap();
    let r = resolve(&scene, &reg);
    assert_eq!(r.things[&sealed].slots[&SlotId::Look].module, b);
    assert_eq!(r.things[&balls].slots[&SlotId::Look].module, a);
    let mut problems = Vec::new();
    assert_eq!(resolve_slot(&scene, &reg, balls, SlotId::Look, &mut problems).unwrap().module, a);
}
```

Add to `edit.rs` tests `link_refuses_a_module_kind_the_thing_does_not_accept`, using a testkit kind whose `accepts` returns false for `look.plain` on Look. Extend `SealedKind` with `accepts(Look, "look.plain") == false` only if that keeps the resolve test valid. Otherwise add a second tiny test kind `"picky"`.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p bpe-core does_not_pass_down link_refuses`
Expected: FAIL (unknown kind "sealed" / no `passes_down`).

- [ ] **Step 3: Implement**

`find_slot`: walk `once(id).chain(ancestors)`, skipping any ancestor (not `id` itself) whose kind has `passes_down() == false`. `resolve_with`: keep two per-thing arrays. `here` is what fills this thing's own slots (as today). `passed` is what children see: equal to `here` when the kind passes down, else equal to the parent's `passed`. Children read their parent's `passed`.

- [ ] **Step 4: Run `cargo test -p bpe-core` and see everything pass**

- [ ] **Step 5: Commit** `feat(core): thing kinds can keep their slots to themselves`

---

### Task 2: Core: owned starter modules

**Files:**
- Modify: `crates/bpe-core/src/library.rs` (`Module.owner`)
- Modify: `crates/bpe-core/src/edit.rs` (new methods, `remove_thing`)
- Modify: `crates/bpe-core/src/save.rs` tests (round trip)

**Interfaces:**
- Consumes: `ThingKind::passes_down` (Task 1).
- Produces:
  - `Module.owner: Option<ThingId>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`.
  - `pub fn starter_name(thing: &str, slot: SlotId) -> String`, returning `format!("{thing} {}", slot.name().to_lowercase())`.
  - `Scene::add_starter(&mut self, reg: &Registry, thing: ThingId, kind: &str) -> Result<ModuleId, EditError>` makes a module of `kind` named `starter_name(thing name, slot)` with `owner = Some(thing)`, and links it into the kind's slot on `thing`.
  - `Scene::rename_thing(&mut self, id: ThingId, name: &str) -> bool` trims the name, does nothing (false) for a blank, the root's missing id or an unchanged name, and renames owned modules whose name equals `starter_name(old, slot)`.
  - `Scene::remove_thing` (existing signature) also removes every module whose owner is a removed thing and that `users_of` reports empty, and sets `owner = None` on such modules still in use.
  - `Scene::make_own(&mut self, reg: &Registry, id: ThingId, slot: SlotId) -> Result<ModuleId, EditError>` resolves the slot (`resolve_slot`), copies that module (kind, params, graph, macros, channel) as a new module named `starter_name` with `owner = Some(id)`, and links it. Errors with `NotLinked` when nothing fills the slot.
  - `Scene::copy_owned_starters(&mut self, from: ThingId, to: ThingId)`: for each slot of `to` linked to a module owned by `from`, links a fresh copy owned by `to` and named for `to`, keeping that slot's overrides.

- [ ] **Step 1: Write failing tests in `edit.rs` tests** (testkit registry)

```rust
#[test] fn add_starter_names_owns_and_links() {
    // balls named "Balls": module "Balls look", owner Some(balls), linked in Look, users_of == [balls]
}
#[test] fn rename_thing_renames_only_matching_starters() {
    // starter "Balls look" + a starter renamed by hand to "Mine": rename to "Red" gives
    // "Red look" and "Mine"; rename_thing(id, "  ") and rename_thing(id, "Red") return false.
}
#[test] fn removing_drops_unused_owned_starters_only() {
    // owned starter (gone), unowned module linked only by the thing (kept), library count checked
}
#[test] fn removing_keeps_shared_starters_and_clears_owner() {
    // b links a's starter; remove a; module still there with owner None, b still linked
}
#[test] fn removing_a_folder_drops_its_childrens_starters() {}
#[test] fn make_own_copies_the_followed_module() {
    // folder(look F with glow 2.0) > balls(Inherit): make_own gives a new module, owner balls,
    // params glow 2.0, named "Balls look"; F unchanged; NotLinked when nothing fills the slot
}
#[test] fn copy_owned_starters_gives_the_copy_its_own() {
    // a owns "A look"; b's slots cloned from a; after copy_owned_starters(a, b) b links a
    // different module owned by b named "<b name> look" with the same params
}
```

In `save.rs` tests, add `owner_survives_save_and_load`.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo test -p bpe-core starter make_own owner removing`

- [ ] **Step 3: Implement the methods above**

`make_own` and `copy_owned_starters` reuse `duplicate_module` and then set name and owner.

- [ ] **Step 4: Run `cargo test -p bpe-core` and see everything pass**

- [ ] **Step 5: Commit** `feat(core): modules a thing owns, named, renamed and removed with it`

---

### Task 3: Kit: the root becomes Scene settings

**Files:**
- Modify: `crates/bpe-kit/src/things/scene/mod.rs`
- Modify: `crates/bpe-kit/src/modules/shader/draw.rs:19-25`
- Modify: `crates/bpe-kit/src/lib.rs` (`mix_settings`)
- Modify: `crates/bpe-mix/src/lib.rs` (`MixSettings`), `crates/bpe-mix/src/channels.rs:45-56,151`
- Modify: `crates/bpe-kit/src/modules/sound/mod.rs`, `crates/bpe-kit/src/modules/melody/mod.rs` (doc comments only)
- Modify: `crates/bpe-kit/src/random.rs` (root chords tests)

**Interfaces:**
- Consumes: `passes_down`, `slot_label`, `accepts` (Task 1).
- Produces:
  - `SceneKind`: `name() == "Scene settings"`, help `"The video, the world and the song, plus effects, music and rules for the whole scene."`, `slots() == &[Sound, Shader, Rules]`, `default_slots() == &[]`, `passes_down() == false`, `slot_label`: Sound → `"Music"`, Shader → `"Effects"`, Rules → `"Rules"`, `accepts(Sound, k)` true only for `"sound.drums" | "sound.melody"`.
  - `SceneSettings` without `chords`.
  - `MixSettings` without `scene_chords` and `scene_instrument`. `MixSettings::progression(r)` returns the library one or the starter. `instrument_for(settings, id, channel)` returns the library one or Pluck.

- [ ] **Step 1: Write failing tests**

In `things/scene/mod.rs`:

```rust
#[test] fn scene_settings_holds_no_chords_and_keeps_its_slots_to_itself() {
    let k = SceneKind;
    assert_eq!(k.name(), "Scene settings");
    assert!(!SceneSettings::defs().iter().any(|d| d.key == "chords"));
    assert_eq!(k.slots(), &[SlotId::Sound, SlotId::Shader, SlotId::Rules]);
    assert!(k.default_slots().is_empty() && !k.passes_down());
    assert_eq!((k.slot_label(SlotId::Sound), k.slot_label(SlotId::Shader)), ("Music", "Effects"));
    assert!(k.accepts(SlotId::Sound, "sound.drums") && !k.accepts(SlotId::Sound, "sound.notes"));
}
```

In `shader/draw.rs`: `the_roots_effect_is_drawn_once_as_a_picture_effect`. Link a shader on ROOT plus an arena child, resolve, draw both things, and assert exactly one `EffectCall` whose key uses `ThingId::ROOT`. This replaces the existing "the arena inherits" assertion. In `bpe-mix/channels.rs`, replace the `"Scene's"` test with `a_missing_instrument_plays_pluck`.

- [ ] **Step 2: Run them and see them fail** (`cargo test -p bpe-kit scene_settings the_roots_effect`, `cargo test -p bpe-mix a_missing_instrument`)

- [ ] **Step 3: Implement**

Remove the root special case in `EffectDraw::draw`, because nothing inherits the root's shader now. Delete the root-chords lock and roll tests in `random.rs` (`random.rs:521`, `:552`) and replace them with one asserting a sound module's `chords` is rolled. Fix every compile error from the removed fields.

- [ ] **Step 4: Run `cargo test -p bpe-mix -p bpe-kit --lib`**

Expect `new_scene`, scenes and layer tests to fail until Tasks 4 and 5. Note the failing names in the commit body and move on only if every failure is a root-inheritance or new-scene test.

- [ ] **Step 5: Commit** `feat(kit): Scene defaults becomes Scene settings; chords and instruments have no scene fallback`

---

### Task 4: Kit: starters, the folder rule, and keeping followers

**Files:**
- Create: `crates/bpe-kit/src/starters.rs` (moves `Starter`, `starter_modules` and `add_thing` out of `lib.rs`; `lib.rs` re-exports `pub use starters::{add_thing, fill_slot, keep_followers, move_thing};`)
- Modify: `crates/bpe-kit/src/lib.rs` (`new_scene`, its tests)

**Interfaces:**
- Consumes: `Scene::add_starter`, `make_own`, `passes_down` (Tasks 1–2). `SceneKind` (Task 3).
- Produces:
  - `fn starter_kind(thing_kind: &str, slot: SlotId) -> Option<(&'static str, &'static [(&'static str, Value)])>`. The table is fixed by the spec: balls/soft/arena/obstacle/text → `look.classic`; balls/soft → `motion.basic`; balls/soft/arena/obstacle → `bounce.basic`, `sound.notes`; liquid → `look.liquid`, `motion.liquid`, `bounce.basic` with `bounciness = 0.0`; swarm → `look.swarm`, `motion.flock`. Any other (kind, slot) falls back to the first `reg.modules_for(slot)` that the kind `accepts` (the caller does that lookup).
  - `pub fn fill_slot(scene: &mut Scene, reg: &Registry, id: ThingId, slot: SlotId) -> Result<(), EditError>`. If a passing-down ancestor fills `slot` (check with `resolve_slot` on a `SlotLink::Inherit` probe), it sets `SlotLink::Inherit`. Otherwise it calls `add_starter` with the starter kind, applies the table's params, and for `sound.notes` sets `instrument` to `shared_pluck(scene)` and `chords` to `shared_progression(scene)`.
  - `pub fn add_thing(scene, reg, kind, parent) -> Result<ThingId, EditError>` calls core `add_thing`, then `fill_slot` for every `default_slots()` entry.
  - `fn shared_pluck(scene: &mut Scene) -> InstrumentRef` and `fn shared_progression(scene: &mut Scene) -> ProgressionRef` find the library item by source/name or add it once.
  - `pub fn keep_followers(scene: &mut Scene, reg: &Registry, before: &Resolved)`: for every thing and every slot that is `SlotLink::Inherit`, where `before` filled it with module M and the current resolve fills it with something else or nothing, it links an owned copy of M (same as `make_own`, from M).
  - `pub fn move_thing(scene: &mut Scene, reg: &Registry, id: ThingId, parent: ThingId, attach: Attach) -> Result<(), EditError>` resolves, calls `set_parent`, then `keep_followers`.
  - `new_scene(reg)`: root chords line removed, no root modules, no root buses change (keep `add_starter_buses`). It adds `"arena"` and `"balls"` via `add_thing` (ids 1 and 2), names the ball `"Ball"` and renames its starters with `rename_thing`. It sets the ball look's fill to `Color::rgb(0xff, 0x5c, 0xa8)` and positions the ball at `(0, -200)`.

- [ ] **Step 1: Write failing tests in `starters.rs`**

```rust
#[test] fn each_kind_starts_with_its_own_modules() {
    // for each (kind, [(slot, module kind)]) in the spec table: add two things of the kind at
    // the root; each slot is Linked to a module of that kind, owned by the thing, named
    // "<name> <slot>"; the second thing's modules differ from the first's.
}
#[test] fn emitters_fields_cameras_and_folders_start_empty() {}
#[test] fn sounds_share_one_pluck_and_one_starter_progression() {
    // two balls: library has exactly one instrument and one progression; both sounds point at them
}
#[test] fn a_thing_added_in_a_filled_folder_follows_it() {
    // folder with an owned look: new balls inside get Look = Inherit, other slots own starters
}
#[test] fn moving_out_of_a_folder_keeps_the_module_through_a_copy() {}
#[test] fn moving_a_folder_keeps_its_followers_looks() {
    // folder F(look L) > balls(Inherit); move F under another folder G(look M):
    // balls still resolve to L's params (now an owned copy or L itself), never M
}
#[test] fn turning_a_folder_slot_off_keeps_followers() {
    // resolve, set F's Look Off, keep_followers: balls own a copy of L
}
#[test] fn a_new_scene_has_scene_settings_an_arena_and_a_pink_ball() {
    // root has no slots linked; arena owns "Arena look"/"Arena bounce"/"Arena sound";
    // ball named "Ball" owns "Ball look" (pink), "Ball motion", "Ball bounce", "Ball sound";
    // resolve has no problems
}
```

Move `adding_a_liquid_links_its_three_shared_modules` (lib.rs) here and rewrite it as `a_liquid_gets_its_own_surface_flow_and_soft_landing`, so the second liquid gets new modules. Update `lib.rs:473` and `:483` to the new shape.

- [ ] **Step 2: Run them and see them fail** (`cargo test -p bpe-kit starters`)

- [ ] **Step 3: Implement as specified in Interfaces**

- [ ] **Step 4: Run `cargo test -p bpe-kit starters new_scene`**

- [ ] **Step 5: Commit** `feat(kit): every new thing gets its own starter modules; folders keep followers`

---

### Task 5: Kit: built-in scenes, scene files, goldens and remaining kit tests

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (Builder helpers `module_of`, `root_look`, `motion`, `bounce`, `sound`, `chords`, `scene_settings`, `add`, plus `Builder::new`)
- Modify: `crates/bpe-kit/src/scenes_stage5.rs` (`style_balls` and root shader scenes)
- Modify: kit tests linking Look/Trail/Particles/Motion/Bounce on `ThingId::ROOT`: `layer_tests.rs`, `modules/particles/draw.rs`, `modules/trail/draw.rs`, `things/balls/draw.rs`, `modules/drums/mix.rs`, `modules/melody/mix.rs`, `music/*`, `testing.rs`, and any other `cargo test -p bpe-kit` failure
- Regenerate: `scenes/next/*.ron`, `crates/bpe-kit/tests/golden*.txt`

**Interfaces:**
- Consumes: `add_thing`, `fill_slot`, `rename_thing` (Tasks 2 and 4).
- Produces: a private `Builder.shared: BTreeMap<SlotId, ModuleId>` made in `Builder::new` with one unowned module per slot: `"Shared look"` (`look.classic`), `"Shared motion"`, `"Shared bounce"`, `"Shared sound"` (`sound.notes`, Pluck, shared progression). `fn share(&mut self, thing: ThingId)`: for Look/Motion/Bounce/Sound, if the thing's link is an owned starter of the same module kind as `shared[slot]`, it links `shared[slot]` and removes the starter. `new()` calls `share(ARENA)` and `share(BALLS)` for all slots except BALLS' Look (the ball keeps its own pink look). `add()` calls `share(id)`.
  - `root_look` → `shared_look`, `motion`/`bounce`/`sound` edit `shared[slot]`. `chords(name, root)` edits the shared progression (`shared_progression`).
  - So every built-in scene shares modules the way it inherited them before, and should look and sound the same.

- [ ] **Step 1: Before changing the builder, render references from main**

```bash
git worktree add ../bpe-main origin/main
cd ../bpe-main && cargo build -q --release -p bpe-cli
for f in scenes/next/*.ron; do n=$(basename $f .ron)
  target/release/bpe-render-next $f $SCRATCH/before/$n.mp4 --frames 120 --scale 0.25 --encoder cpu
  ffmpeg -loglevel error -sseof -0.05 -i $SCRATCH/before/$n.mp4 -frames:v 1 $SCRATCH/before/$n.png
done
```

`$SCRATCH` is the session scratchpad. If the release binary has another name, use the `[[bin]]` from `crates/bpe-cli/Cargo.toml`.

- [ ] **Step 2: Change the Builder and stage 5 as specified, then fix every remaining kit test**

Tests that linked a module on the root to reach a child now link it on the child, or on a folder above it, keeping each test's intent. Root-shader tests keep the root link.

- [ ] **Step 3: Regenerate files**

Run: `BPE_BLESS=1 cargo test -p bpe-kit scenes`, then `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden --test golden7`, then the same with `--release`.

- [ ] **Step 4: Compare frames**

Run the same loop on this branch, with the regenerated `scenes/next/*.ron`, into `$SCRATCH/after/`. Compare each PNG pair per pixel (a short Python/PIL check: max channel difference ≤ 2). Compare each audio track with `ffmpeg -i x.mp4 -f f32le -ac 1 -` and assert RMS difference < 1e-3. Every scene must match. Fix a mismatch, or explain it in the commit body and in the final report.

- [ ] **Step 5: Run `cargo test -p bpe-kit` (debug) and see it fully pass**

- [ ] **Step 6: Commit** `refactor(kit): built-in scenes share modules explicitly; regenerate scene files and goldens`

---

### Task 6: Studio: tree and inspector

**Files:**
- Modify: `crates/bpe-studio/src/tree.rs` (Rename → `scene.rename_thing`; `duplicate` → after copying slots call `scene.copy_owned_starters(id, copy)`; drag `set_parent` at `tree.rs:315` → `bpe_kit::move_thing`; doc strings "Scene defaults" → "Scene settings")
- Modify: `crates/bpe-studio/src/inspector.rs` (picker, Make own, AddSlot, slot labels, folder edits)
- Modify: `crates/bpe-studio/src/selection.rs`, `preview.rs`, `app.rs` tests that call core `add_thing` and expect inheritance (use `bpe_kit::add_thing`)

**Interfaces:**
- Consumes: `move_thing`, `fill_slot`, `keep_followers`, `rename_thing`, `copy_owned_starters`, `make_own`, `slot_label`, `accepts`.
- Produces:
  - `Edit::Inherit(slot)` is renamed `Edit::Follow(slot)`, and `Edit::MakeOwn(slot)` is new, calling `scene.make_own`.
  - `Write::Inherited` stays as the enum name, but `write_slot` no longer auto-links on edit. An inherited card is read-only, so `write_slot` returns false for it.
  - Picker contents: library modules of that slot whose kind the thing `accepts`; "New … module" kinds filtered by `accepts`; Make unique / Push all / Reset all / Open in Library (unchanged); separator; `"Follow folder"` only when a passing-down ancestor fills the slot; `"Remove"` (was "Off", same `Edit::Off`).
  - Card state text for Inherit: `"Follow folder: <folder name>"`. If nothing above fills it: `"Nothing (pick a module)"`.
  - The read-only followed card shows a `"Make own"` button (replaces `change_here`), hover `"Give this thing its own copy, so changes here are its own"`.
  - `Edit::AddSlot(slot)` calls `bpe_kit::fill_slot`.
  - The card title uses `kind.slot_label(slot)`.
  - For edits on a folder's slot (Link, Off, Follow, NewModule), `apply` resolves before the edit and calls `bpe_kit::keep_followers` after it.

- [ ] **Step 1: Write failing kittest tests in `inspector.rs` tests**

```rust
#[test] fn a_new_balls_card_edits_straight_away() {
    // new_scene, select Ball, drag Look glow: the Ball look module changes; no "Change here"
}
#[test] fn follow_folder_is_offered_only_inside_a_filled_folder() {}
#[test] fn make_own_gives_a_followed_card_its_own_module() {}
#[test] fn scene_settings_shows_music_effects_rules_and_no_chords() {
    // select root: label "Scene settings"; no "Chord lane"; Add menu offers Music/Effects/Rules
}
#[test] fn the_music_picker_offers_only_drums_and_melody() {}
```

In `tree.rs` tests: `renaming_a_thing_renames_its_starters`, `duplicating_gives_the_copy_its_own_starters`, `undo_after_delete_brings_starters_back` (use `Studio`/`History` as the existing undo tests do). Delete or rewrite the old tests that reference `"from Scene defaults"`, `"Inherit"`, `"Change here"`, `scenes_is_offered_…`, `scene_defaults_fits_the_panel` (rename it to `scene_settings_fits_the_panel`).

- [ ] **Step 2: Run them and see them fail** (`cargo test -p bpe-studio inspector tree`)

- [ ] **Step 3: Implement as specified**

- [ ] **Step 4: Run `cargo test -p bpe-studio inspector tree selection preview`**

- [ ] **Step 5: Commit** `feat(studio): own cards edit directly; Follow folder and Make own; Scene settings card`

---

### Task 7: Studio: chords, instruments and timeline without the scene fallback

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs:1035-1110` (`chord_editor`, `instrument_section`) and `:1247-1320` (pick handlers)
- Modify: `crates/bpe-studio/src/widgets.rs:20-85` (`TargetChoices`: drop `scene_chords`, `scene_instrument`)
- Modify: `crates/bpe-studio/src/instruments.rs:12-30` (delete `scene_instrument`; `choice` falls back to Pluck)
- Modify: `crates/bpe-studio/src/timeline/chords.rs:15-30` (`shown`)
- Modify: any `chords/` widget offering a "Scene's" entry

**Interfaces:**
- Consumes: `MixSettings` without fallbacks (Task 3).
- Produces: `fn shown(studio: &Studio) -> Option<ProgressionId>`, which returns the selection's Sound chords, else those of the first thing in `scene.order` whose resolved Sound has a `chords` in the library.

- [ ] **Step 1: Write failing tests**

```rust
// inspector.rs
#[test] fn the_chords_row_offers_library_progressions_and_no_scenes_entry() {
    // ball sound: combo lists "Starter" (the shared one) and no label containing "Scene's"
}
#[test] fn a_new_ball_plays_the_shared_pluck() {}
// timeline/chords.rs
#[test] fn the_chord_track_follows_the_selected_things_sound() {
    // two balls with different progressions: selecting each shows its name in "Chords (<name>)";
    // no selection shows the first ball's
}
```

- [ ] **Step 2: Run them and see them fail**

- [ ] **Step 3: Implement.** Remove the "Plays Scene defaults' …" notes and "Scene's" entries.

- [ ] **Step 4: Run `cargo test -p bpe-studio`, then check the copy is gone**

Run `grep -rn "Scene defaults\|Change here\|Scene's\|Inherit (nothing" crates/bpe-studio crates/bpe-kit crates/bpe-mix crates/bpe-core`. Expected: no matches outside comments that explain history (prefer none).

- [ ] **Step 5: Commit** `feat(studio): chords and instruments are per sound module; chord track follows selection`

---

### Task 8: Whole-workspace verification

- [ ] **Step 1:** `CARGO_INCREMENTAL=0 cargo test --workspace` (debug). All pass.
- [ ] **Step 2:** `cargo clippy --workspace --all-targets -- -D warnings`. Clean.
- [ ] **Step 3:** `cargo test --workspace --release` (golden release lines). Only the known `pushing_an_earlier_step_panics_in_debug` fails.
- [ ] **Step 4:** Launch the studio headless kittest smoke (existing app test) and take a screenshot of a new scene with the Ball selected and with Scene settings selected. Save both under `/mnt/project-files/ballphysics/scene-settings/`.
- [ ] **Step 5:** Update the module-level docs that say "Scene defaults" (`resolve.rs:1-2`, `things/scene/mod.rs:1-2`, `scene.rs:79,191,259-262`) and the README section, if one exists. Commit `docs: Scene settings wording`.
