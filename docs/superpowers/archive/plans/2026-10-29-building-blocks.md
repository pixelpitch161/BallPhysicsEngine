# Modular Ball Studio, Stage 3 (Building Blocks) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The building blocks the spec lists for stage 3: Obstacle and Folder things, attaching (riding), rounded and capsule arenas, ball-to-ball collisions, tags as interaction targets, the remaining Look/Motion/Bounce/Sound options, per-thing overrides with reset/push/make-unique, a Library window, multi-select and drag-to-reorder in the tree, five more stage scenes, and a Windows `ball-studio-next.exe`.

**Architecture:** Everything is additive on the stage 2 crates. `bpe-core` gains target matching and a few library edits. `bpe-runtime` gains poses (so things can ride on each other), solid colliders that balls bounce off from outside (pegs, bars, rings), a grid broadphase with a one-pass ball-pair solver, and three new `ModuleSim` hooks with default bodies so stage 2 modules keep compiling. `bpe-kit` gains the `folder` and `obstacle` kinds in their own folders plus new settings on the four stage 2 modules. `bpe-studio` gains a selection set, drag and drop in the tree, override marks in the inspector, a Target picker and a Library window. Nothing in the old crates or `bpe-graph` changes.

**Tech Stack:** Rust 2024, bpe-core / bpe-runtime / bpe-gfx / bpe-mix / bpe-kit / bpe-studio (stage 2), glam 0.34, wgpu 30 (WGSL), eframe/egui 0.37 with its drag-and-drop helpers (`dnd_drag_source`, `dnd_release_payload`, `dnd_hover_payload`), egui_kittest 0.37 (dev).

**Spec:** `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (sections 3.1 Obstacle and Folder, 3.4, 3.6, 4.2, 4.3, 4.4, 4.5, 5.3, 5.4, 7 stage 3, 8). Stage 2 report: `/mnt/project-files/ballphysics/modular-revamp/stage2/REPORT.md` (gaps list).

## Global Constraints

- Old crates (`bpe-scene`, `bpe-sim`, `bpe-render`, `bpe-audio`, `bpe-export`, `bpe-app`, `bpe-music`, `bpe-graph`) are not modified. `bpe-core` and `bpe-settings-derive` may only gain items. `cargo test --workspace` passes at the end of every task; `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` are clean before the PR.
- The five stage 2 golden log lines in `crates/bpe-kit/tests/golden.txt` and `golden-debug.txt` stay byte-identical. New behaviour is off by default (`hit_balls: false`, `pops_after: 0`, `corner: 0`, `wind: 0`, `plays_when: Hitting`) so stage 2 scenes simulate and sound exactly as before.
- Additive to shared types: `Contact`, `Outline::Ring`, `Outline::Box`, `Wall`, `Bodies` keep their fields; new runtime concepts are new variants, new structs or new trait methods with default bodies. Stages 4 to 7 run in parallel and rebase onto this.
- Every new map is a `BTreeMap`/`BTreeSet`. Every simulation loop runs in `scene.order` (tree order, parents first) or `ThingId` order, never hash order.
- Every new setting carries a label, a plain-sentence help line, a range when it is a number, and the right `simple`/`keyframe`/`random`/`pulse` flags; at most 6 settings per kind are Simple (`bpe_core::contract::MAX_SIMPLE`).
- Every new egui control is labelled (`labelled_by`) so `egui_kittest`'s accessibility check passes; pickers are `menu_button`s, not ComboBoxes; icons are painted, not glyphs (see team memory `studio-next-gotchas`).
- Attach rules, decided here: a `Group`ed thing keeps its own world position (as stage 2 did). A `Ride`r ignores its own position: it sits at the parent's pose plus `offset` (rotated with the parent when `turn_with_parent`), turns with the parent when `turn_with_parent`, and free bodies (balls) take that pose and the parent's velocity at spawn only. Inheritance of modules is the same for both.
- Inspector write policy, decided here: a setting on a Linked slot writes to the module itself when this thing is the module's only user, otherwise to this thing's override (orange dot). An inherited slot shows its values disabled with a "Change here" button that links the inherited module to the thing.
- Text fixed here: "Change here", "Make unique", "Push to module", "Reset to module", "Push all to module", "Reset all", "shared ×N", "Editing N things", "A thing can't ride on itself or on something inside it" (existing `EditError::WouldLoop`), Library window title "Library", tree hint "Drag rows to reorder · drop on a row to group · hold Alt to ride".

## Review Focus

1. **A peg grid with hundreds of pegs and a block of hundreds of balls** must still step in real time; the per-ball check must skip whole obstacles by their bounding reach. Test: `a_big_peg_board_steps_a_second_in_under_a_second` (Task 6).
2. **A ball spawned inside a solid** (a peg placed on top of a ball) must be pushed out, never stuck or sent non-finite. Test: `a_ball_born_inside_a_peg_is_pushed_out_finitely` (Task 3).
3. **A rider whose parent is removed or disabled** must not crash the build or the step: it falls back to its own transform. Test: `a_rider_without_a_parent_pose_uses_its_own_transform` (Task 2).
4. **Deleting a module from the Library while a thing's override points at it** is refused with the users named; dropping a module onto a thing whose kind lacks the slot is refused with the slot named. Tests: `delete_is_refused_while_in_use_and_names_the_users`, `dropping_a_module_on_a_thing_without_the_slot_is_refused` (Task 12).
5. **A multi-selection mixing kinds** (a ball and an arena) must apply a Size edit only to the things that have a Size, and never panic on the others. Test: `a_mixed_selection_only_edits_things_that_have_the_setting` (Task 11).

---

## File Structure

```
crates/bpe-core/src/
  scene.rs          + Scene::target_matches, Scene::descendants, Thing::rides, Scene::common_parent
  edit.rs           + rename_module, duplicate_module, make_folder, set_override
  (nothing else changes)

crates/bpe-runtime/src/
  pose.rs           new: Pose, World::anchor
  solids.rs         new: Solid, Solids, keep_outside (swept disc/capsule/ring collision)
  pairs.rs          new: Grid broadphase (ported), PairRule, collide_pairs
  plugins.rs        + ModuleSim::{bounciness, pairs, effects, respond_against} (defaults)
  walls.rs          + Outline::Rounded variant (keep_inside, point, inside, center, thickness)
  world.rs          + order, poses, solids, tags, attach tables; build in tree order; solids and pairs stages
  lib.rs            re-exports

crates/bpe-gfx/src/
  frame.rs          + ShapeInstance::capsule, ::disc; outline() handles Rounded (kind 1, b.z = corner)
  shaders/scene.wgsl  rounded box (b.z) and capsule (kind 2) distances

crates/bpe-mix/src/lib.rs   mix() passes a thing the events it is party a OR b to

crates/bpe-kit/src/
  things/folder/{mod,sim}.rs        new
  things/obstacle/{mod,sim,draw}.rs new
  things/arena/{mod,sim}.rs         + corner, Capsule
  things/balls/sim.rs               riders spawn at the anchor pose
  things/balls/draw.rs              gradient_by, hue_per_hit
  modules/look/mod.rs               + gradient_by, gradient_full, hue_per_hit
  modules/motion/{mod,sim}.rs       + wind
  modules/bounce/{mod,sim}.rs       + hit_balls, balls_to_hit, friction, pops_after, special*
  modules/sound/{mod,mix}.rs        + plays_when, sounds_for, pitch_by, min_gap
  scenes.rs                         + stage3(), all()
  lib.rs                            registry/sims/draws additions
  tests/golden.rs, golden.txt, golden-debug.txt, pictures.rs   use scenes::all

crates/bpe-studio/src/
  selection.rs      new: Selection helpers (primary + extra, range select)
  tree.rs           folders (collapse, ▸), ↳ riders, ctrl/shift select, drag and drop, context items
  inspector.rs      parent/attach header, override marks, write policy, multi-edit, card menu items
  widgets.rs        GridCx (target choices, override marks), Target picker, settings_grid_marked
  library.rs        new: the Library window
  preview.rs        riders drag their offset, obstacle handles, Rounded edges, dashed folder outline
  app.rs            extra selection, Library button, status from tree
  theme.rs          + OVERRIDE colour
  main.rs           mod library, mod selection
scenes/next/*.ron   five new stage 3 scenes
README.md           a paragraph on stage 3
```

---

### Task 1: Core: targets, folders and library edits

**Files:**
- Modify: `crates/bpe-core/src/scene.rs`, `crates/bpe-core/src/edit.rs`

**Interfaces:**
- Produces (all on `impl Scene` unless said):
  - `pub fn target_matches(&self, target: &Target, me: ThingId, other: ThingId) -> bool`: `Me` → `other == me`; `Thing(t)` → `other == t`; `Tag(s)` → `other` has tag `s`; `Kind(k)` → `other`'s kind is `k`; `Anything` → true; an `other` not in the scene → false (except `Anything`).
  - `pub fn descendants(&self, id: ThingId) -> Vec<ThingId>`: everything under `id` in tree order, not `id` itself.
  - `pub fn common_parent(&self, ids: &[ThingId]) -> Option<ThingId>`: the parent of the first non-root id.
  - `impl Thing { pub fn rides(&self) -> bool }`: `matches!(self.attach, Attach::Ride { .. })`.
  - `pub fn rename_module(&mut self, m: ModuleId, name: &str) -> Result<(), EditError>` (`MissingModule`; a blank name is kept as is).
  - `pub fn duplicate_module(&mut self, m: ModuleId) -> Result<ModuleId, EditError>`: a copy named `"<name> copy"` with the same kind, slot, params and graph; nothing links to it.
  - `pub fn make_folder(&mut self, reg: &Registry, ids: &[ThingId]) -> Result<ThingId, EditError>`: adds a `"folder"` thing under `common_parent(ids)` (Err `Root` when `ids` is empty or only the root), moves it before the first of `ids` with `move_before`, then `set_parent`s each id (skipping the root and anything already inside another listed id) into it with `Attach::Group`, keeping their order. Returns the folder.
  - `pub fn set_override(&mut self, reg: &Registry, id: ThingId, slot: SlotId, key: &str, v: Value) -> Result<(), EditError>`: `set_at` with `Owner::Thing(id)` and `Some(slot)` (a convenience the editor uses in loops).

- [ ] **Step 1: Write the failing tests** in `scene.rs` and `edit.rs` `mod tests` (testkit registry has kinds `scene`, `folder`, `balls`, modules `look.plain`, `sound.pluck`):

```rust
#[test]
fn targets_match_by_thing_tag_kind_and_self() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    s.things.get_mut(&b).unwrap().tags.insert("drums".into());
    assert!(s.target_matches(&Target::Anything, a, b));
    assert!(s.target_matches(&Target::Me, a, a) && !s.target_matches(&Target::Me, a, b));
    assert!(s.target_matches(&Target::Thing(b), a, b) && !s.target_matches(&Target::Thing(b), a, a));
    assert!(s.target_matches(&Target::Tag("drums".into()), a, b) && !s.target_matches(&Target::Tag("drums".into()), b, a));
    assert!(s.target_matches(&Target::Kind("folder".into()), a, b) && !s.target_matches(&Target::Kind("balls".into()), a, b));
    assert!(!s.target_matches(&Target::Thing(ThingId(99)), a, ThingId(99)));
    assert!(s.target_matches(&Target::Anything, a, ThingId(99)));
}

#[test]
fn descendants_are_in_tree_order_without_the_root_of_the_walk() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
    let a = s.add_thing(&r, "balls", f).unwrap();
    let g = s.add_thing(&r, "folder", f).unwrap();
    let b = s.add_thing(&r, "balls", g).unwrap();
    let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    assert_eq!(s.descendants(f), vec![a, g, b]);
    assert_eq!(s.descendants(c), Vec::<ThingId>::new());
    assert!(!s.things[&a].rides());
}
```

```rust
#[test]
fn make_folder_gathers_the_selection_in_place() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
    let f = s.make_folder(&r, &[b, c, ThingId::ROOT]).unwrap();
    assert_eq!(s.things[&f].kind, "folder");
    assert_eq!(s.order, vec![ThingId::ROOT, a, f, b, c]);
    assert_eq!(s.things[&b].parent, Some(f));
    assert!(matches!(s.make_folder(&r, &[ThingId::ROOT]), Err(EditError::Root)));
}

#[test]
fn library_modules_can_be_renamed_and_duplicated() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let m = s.add_module(&r, "look.plain", "Neon").unwrap();
    s.library.modules.get_mut(&m).unwrap().params.set("glow", Value::Number(1.0));
    s.rename_module(m, "Neon glow").unwrap();
    let copy = s.duplicate_module(m).unwrap();
    assert_eq!(s.library.modules[&copy].name, "Neon glow copy");
    assert_eq!(s.library.modules[&copy].params, s.library.modules[&m].params);
    assert!(s.users_of(copy).is_empty());
    assert!(matches!(s.rename_module(ModuleId(9), "x"), Err(EditError::MissingModule(_))));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-core`. Expected: FAIL (methods missing).
- [ ] **Step 3: Implement** the methods above. `make_folder` filters `ids` to things that exist, are not the root, and have no ancestor in `ids`.
- [ ] **Step 4: Run** `cargo test -p bpe-core`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Core: target matching, descendants, folders from a selection, library renames"`

---

### Task 2: Runtime: poses and riding

**Files:**
- Create: `crates/bpe-runtime/src/pose.rs`
- Modify: `crates/bpe-runtime/src/world.rs`, `crates/bpe-runtime/src/lib.rs`, `crates/bpe-runtime/src/testing.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq)] pub struct Pose { pub position: Vec2, pub angle: f32, pub velocity: Vec2, pub spin: f32 }` (angle radians clockwise, spin rad/s).
  - `impl Pose { pub fn from_transform(t: &Transform) -> Pose; pub fn local(&self, offset: Vec2, turn: bool) -> Pose }`: `local` places `offset` on this pose: position = `self.position + (if turn { Vec2::from_angle(self.angle).rotate(offset) } else { offset })`; angle = `if turn { self.angle } else { 0.0 }`; velocity = `self.velocity + (if turn { (position - self.position).perp() * self.spin } else { Vec2::ZERO })`; spin = `if turn { self.spin } else { 0.0 }`.
  - `World` new public fields: `pub poses: BTreeMap<ThingId, Pose>` (every placed thing's current pose; sims write it in `build` and `scripted`), and private `order: Vec<ThingId>` (scene order filtered to built things), `attach: BTreeMap<ThingId, (ThingId, Attach, Transform)>` (parent, attach, own transform, for every built non-root thing).
  - `impl World { pub fn anchor(&self, thing: ThingId) -> Pose }`: for `Ride { offset, turn_with_parent, item }` with a parent: the parent's pose (`poses[parent]`, or for `item: Some(i)` with `bodies[parent]` having row `i`: `Pose { position: position[i], velocity: velocity[i], ..Default }`), then `.local(offset, turn_with_parent)`. When the parent has no pose and no such body, or for `Group`, returns `Pose::from_transform(own transform)`. Never panics.
  - `World::build` iterates `scene.order` (skipping things not in `resolved`) and records `order` and `attach`. `World::step` runs the Scripted stage over `order`.
  - Re-export `Pose` from `lib.rs`.

- [ ] **Step 1: Write the failing tests** in `pose.rs` and `world.rs` `mod tests` (extend `testing.rs` with `pub fn ring_with_rider(offset: Vec2, turn: bool) -> (Scene, Resolved, Sims)`: `WALL` ring radius 100 spinning 1 rad/s (add a `spin` setting to the test `WallSettings` and `TestWall::scripted` writing `poses[WALL]`), `DOTS` riding on it with `Attach::Ride { offset, turn_with_parent: turn, item: None }`, and `TestDots::build` placing its ball at `world.anchor(id)`):

```rust
#[test]
fn local_turns_the_offset_and_adds_the_rim_speed() {
    let p = Pose { position: Vec2::new(10.0, 0.0), angle: FRAC_PI_2, velocity: Vec2::new(1.0, 0.0), spin: 2.0 };
    let r = p.local(Vec2::new(5.0, 0.0), true);
    assert!((r.position - Vec2::new(10.0, 5.0)).length() < 1e-5, "{r:?}");
    assert!((r.velocity - Vec2::new(1.0 - 10.0, 0.0)).length() < 1e-4, "{r:?}");
    assert_eq!((r.angle, r.spin), (FRAC_PI_2, 2.0));
    let g = p.local(Vec2::new(5.0, 0.0), false);
    assert_eq!(g, Pose { position: Vec2::new(15.0, 0.0), velocity: Vec2::new(1.0, 0.0), ..Pose::default() });
}
```

```rust
#[test]
fn a_rider_starts_at_its_parents_pose_with_its_speed() {
    let (scene, resolved, sims) = ring_with_rider(Vec2::new(50.0, 0.0), true);
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(50.0, 0.0));
    // Clockwise spin of 1 rad/s at (50, 0) moves +y at 50 px/s, on top of the launch speed.
    let v = w.bodies[&DOTS].velocity[0];
    assert!((v.y - (-200.0 + 50.0)).abs() < 1e-3 && (v.x - 300.0).abs() < 1e-3, "{v:?}");
}

#[test]
fn a_rider_without_a_parent_pose_uses_its_own_transform() {
    let (mut scene, _, sims) = ring_with_rider(Vec2::new(50.0, 0.0), true);
    scene.things.get_mut(&WALL).unwrap().enabled = false;
    scene.things.get_mut(&DOTS).unwrap().transform.position = Vec2::new(7.0, 8.0);
    let resolved = resolve(&scene, &reg());
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(7.0, 8.0));
    assert_eq!(w.anchor(ThingId(99)), Pose::default());
}

#[test]
fn build_and_scripted_follow_tree_order_not_id_order() {
    // A folder-like parent added after its child in id order must still be placed first.
    let (mut scene, _, sims) = ring_with_rider(Vec2::new(50.0, 0.0), false);
    let reg = reg();
    let late = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
    scene.things.get_mut(&late).unwrap().transform.position = Vec2::new(300.0, 0.0);
    scene.set_parent(DOTS, late, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: false, item: None }).unwrap();
    let resolved = resolve(&scene, &reg);
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(300.0, 0.0));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime`. Expected: FAIL.
- [ ] **Step 3: Implement** `pose.rs`, the `World` fields, `anchor`, tree-order build and scripted. `TestWall::build`/`scripted` write `poses[id]` with the wall's centre, angle and spin; `TestDots::build` uses `anchor`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime -p bpe-kit`. Expected: PASS, golden logs unchanged.
- [ ] **Step 5: Commit** `git commit -m "Runtime: poses, riding on a parent, tree-order build"`

---

### Task 3: Runtime: solids that balls bounce off from outside

**Files:**
- Create: `crates/bpe-runtime/src/solids.rs`
- Modify: `crates/bpe-runtime/src/world.rs`, `crates/bpe-runtime/src/lib.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum Solid { Disc { center: Vec2, radius: f32 }, Capsule { from: Vec2, to: Vec2, radius: f32 }, Ring { center: Vec2, radius: f32, thickness: f32 } }` in world space.
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Solids { pub pose: Pose, pub parts: Vec<Solid>, pub reach: f32 }`: `reach` is the distance from `pose.position` to the farthest point of any part. `impl Solids { pub fn new(pose: Pose, parts: Vec<Solid>) -> Solids }` computes `reach`.
  - `pub fn keep_outside(position: &mut Vec2, previous: Vec2, velocity: Vec2, radius: f32, solid: &Solid, pose: &Pose) -> Option<Contact>`: moves the ball out of the solid and returns the contact when it was moving into it. Swept: a disc uses the old `collide_disc` quadratic (path `previous → position` against radius `reach = radius + disc radius`); a capsule checks the two end discs and the slab between the parallel edge lines and takes the earliest hit `t ∈ [0, 1]`; a ring puts the ball back on the side of the centre line it came from (sign of `|previous − center| − radius`). `Contact.wall_velocity` is the pose's velocity at the contact point (`pose.velocity + (point − pose.position).perp() * pose.spin`). A ball with its centre exactly on a part's core (distance 0) is pushed along `-Vec2::Y`.
  - `World` gains `pub solids: BTreeMap<ThingId, Solids>`. In `substep`, after the walls loop and before pair collisions, every body of every thing is tested against every solid of every other thing in `ThingId` order: skip when `(position − solids.pose.position).length() > solids.reach + radius + SAFE_MOVE`; otherwise `keep_outside` per part in order with `previous = position − velocity * dt`; the Bounce decision (`takes_over` on the solid's thing, else the mover's; `respond_against` from Task 4) and the Hit event with `b = Party { thing: solid's thing, item: part index }` follow the wall loop exactly.
  - Re-export `Solid`, `Solids`, `keep_outside`.

- [ ] **Step 1: Write the failing tests** in `solids.rs` `mod tests` (plus one world test using a `TestPeg` sim in `testing.rs`: kind `"peg"`, settings `radius`, build writes `solids[id] = Solids::new(pose, vec![Disc])`):

```rust
#[test]
fn a_ball_hitting_a_peg_is_pushed_out_and_bounces_back() {
    let peg = Solid::Disc { center: Vec2::ZERO, radius: 10.0 };
    let mut p = Vec2::new(0.0, -12.0);
    let c = keep_outside(&mut p, Vec2::new(0.0, -20.0), Vec2::new(0.0, 100.0), 5.0, &peg, &Pose::default()).unwrap();
    assert!((p.y + 15.0).abs() < 1e-4, "{p:?}");
    assert_eq!(c.normal, Vec2::new(0.0, -1.0));
    assert!((c.speed - 100.0).abs() < 1e-4);
    assert!((reflect(Vec2::new(0.0, 100.0), &c, 1.0).y + 100.0).abs() < 1e-4);
}

#[test]
fn a_fast_ball_cannot_tunnel_through_a_thin_bar() {
    let bar = Solid::Capsule { from: Vec2::new(-100.0, 0.0), to: Vec2::new(100.0, 0.0), radius: 1.0 };
    let mut p = Vec2::new(0.0, 8.0); // already past the bar this substep
    let c = keep_outside(&mut p, Vec2::new(0.0, -8.0), Vec2::new(0.0, 4000.0), 2.0, &bar, &Pose::default()).unwrap();
    assert!(p.y < 0.0 && (p.y + 3.0).abs() < 1e-3, "{p:?}");
    assert_eq!(c.normal, Vec2::new(0.0, -1.0));
    // The bar's ends are round.
    let mut e = Vec2::new(104.0, 0.0);
    assert!(keep_outside(&mut e, Vec2::new(104.0, 0.0), Vec2::new(-10.0, 0.0), 2.0, &bar, &Pose::default()).is_none());
    e = Vec2::new(102.0, 0.0);
    assert!(keep_outside(&mut e, Vec2::new(105.0, 0.0), Vec2::new(-10.0, 0.0), 2.0, &bar, &Pose::default()).is_some());
    assert!((e.x - 103.0).abs() < 1e-3);
}

#[test]
fn a_ring_bounces_balls_from_both_sides() {
    let ring = Solid::Ring { center: Vec2::ZERO, radius: 100.0, thickness: 4.0 };
    let mut inside = Vec2::new(97.0, 0.0);
    let c = keep_outside(&mut inside, Vec2::new(90.0, 0.0), Vec2::new(50.0, 0.0), 3.0, &ring, &Pose::default()).unwrap();
    assert!((inside.x - 95.0).abs() < 1e-3 && c.normal == Vec2::new(-1.0, 0.0));
    let mut outside = Vec2::new(103.0, 0.0);
    let c = keep_outside(&mut outside, Vec2::new(110.0, 0.0), Vec2::new(-50.0, 0.0), 3.0, &ring, &Pose::default()).unwrap();
    assert!((outside.x - 105.0).abs() < 1e-3 && c.normal == Vec2::new(1.0, 0.0));
}

#[test]
fn a_moving_peg_carries_its_speed_into_the_contact() {
    let peg = Solid::Disc { center: Vec2::new(0.0, 0.0), radius: 10.0 };
    let pose = Pose { velocity: Vec2::new(30.0, 0.0), ..Pose::default() };
    let mut p = Vec2::new(12.0, 0.0);
    let c = keep_outside(&mut p, Vec2::new(14.0, 0.0), Vec2::ZERO, 5.0, &peg, &pose).unwrap();
    assert_eq!(c.wall_velocity, Vec2::new(30.0, 0.0));
    assert!((c.speed - 30.0).abs() < 1e-4);
}

#[test]
fn a_ball_born_inside_a_peg_is_pushed_out_finitely() {
    let peg = Solid::Disc { center: Vec2::ZERO, radius: 10.0 };
    let mut p = Vec2::ZERO;
    let c = keep_outside(&mut p, Vec2::ZERO, Vec2::ZERO, 5.0, &peg, &Pose::default());
    assert!(p.is_finite() && (p.length() - 15.0).abs() < 1e-3);
    assert!(c.is_none(), "not moving into it, so no hit");
    let cap = Solid::Capsule { from: Vec2::new(-10.0, 0.0), to: Vec2::new(10.0, 0.0), radius: 2.0 };
    let mut q = Vec2::new(3.0, 0.0);
    keep_outside(&mut q, Vec2::new(3.0, 0.0), Vec2::ZERO, 1.0, &cap, &Pose::default());
    assert!(q.is_finite() && q.y.abs() >= 3.0 - 1e-3);
}
```

And in `world.rs`:

```rust
#[test]
fn balls_bounce_off_a_peg_and_log_it_against_its_part() {
    let (scene, resolved, sims) = dots_and_peg(); // ring 100, a peg radius 10 at (0, 40), one dot falling from (0, -50) with no sideways speed
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let hit = log.events().iter().find(|e| e.b.is_some_and(|b| b.thing == PEG)).expect("the dot lands on the peg");
    assert_eq!(hit.b, Some(Party { thing: PEG, item: 0 }));
    assert!(hit.normal.y < 0.0);
    assert!(w.bodies[&DOTS].position[0].is_finite());
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime`. Expected: FAIL.
- [ ] **Step 3: Implement** `solids.rs` and the solids stage in `substep`. The `respond` call uses the wall loop's decider code, lifted into a private `fn decide(...)` used by both loops.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime -p bpe-kit`. Expected: PASS, goldens unchanged.
- [ ] **Step 5: Commit** `git commit -m "Runtime: pegs, bars and rings that balls bounce off from outside"`

---

### Task 4: Runtime: Bounce hooks and ball-to-ball collisions

**Files:**
- Create: `crates/bpe-runtime/src/pairs.rs`
- Modify: `crates/bpe-runtime/src/plugins.rs`, `crates/bpe-runtime/src/world.rs`, `crates/bpe-runtime/src/lib.rs`, `crates/bpe-runtime/src/testing.rs`

**Interfaces:**
- Produces in `plugins.rs` (all with default bodies):
  - `#[derive(Clone, Debug, PartialEq)] pub struct PairRule { pub target: Target, pub friction: f32 }`.
  - `ModuleSim::bounciness(&self, _state: &dyn ThingState) -> f32 { 1.0 }`.
  - `ModuleSim::pairs(&self, _state: &dyn ThingState) -> Option<PairRule> { None }`: `Some` means the things using this module bump into other balls matching `target`.
  - `ModuleSim::effects(&self, _state, _bodies: &mut Bodies, _item: usize, _speed: f32, _limits: &Limits) -> bool { false }`: what a real hit does to the ball itself (grow, speed gain); returns true when the ball should be removed (popped).
  - `ModuleSim::respond_against(&self, state, contact, bodies, item, limits, _special: bool) { self.respond(state, contact, bodies, item, limits) }`: `special` is true when the thing hit matches the module's special target (Task 8); the wall and solid loops call this instead of `respond`.
  - `ModuleSim::special_target(&self, _state) -> Option<Target> { None }`.
- Produces in `pairs.rs`:
  - `pub struct Grid` ported from `bpe-sim/src/broadphase.rs` (`rebuild`, `cell_of`, `cells`, `runs_around`, `run`, `entry`), items being `u32` indices into a flat list.
  - `pub(crate) fn collide_pairs(world: &mut World, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log)`: gathers every alive ball of every thing whose Bounce `pairs()` is `Some` into a flat list in `(ThingId, item)` order; grid cell = 2 × the largest radius; for every pair `i < j` in grid order (cells sorted, then `i` by index, then the 3×3 neighbourhood column by column, then `j` by index) where both rules match the other's thing (`world.matches(&rule_i.target, thing_i, thing_j)` and the reverse) and the discs overlap: normal from i to j, inverse masses `1/r²`, push apart by `(overlap − 0.05) * 0.6` split by inverse mass, closing speed `vn = (v_i − v_j)·n`; if `vn > 0`: impulse with restitution `(b_i + b_j)/2` (each side's `bounciness()`), tangential impulse limited to `friction × normal impulse` with friction `(f_i + f_j)/2`; if `vn ≥ limits.resting`: `hits += 1` on both, log `Hit { a: (thing_i, i), b: Some((thing_j, j)), point: midpoint on i's surface, normal: n from j towards i (that is `-n`), speed: vn }`, then `effects()` for each side with `vn` (a `true` return sets `alive = false` and logs `Remove { a: that ball, b: None, point: its position, normal: ZERO, speed: 0 }`).
- `World` gains `pub tags: BTreeMap<ThingId, BTreeSet<String>>` (filled at build from the scene) and `pub fn matches(&self, t: &Target, me: ThingId, other: ThingId) -> bool` (same rules as `Scene::target_matches`, using `kinds` and `tags`).
- `World::substep` runs `collide_pairs` after the solids stage. The wall and solid loops call `effects` after `respond_against` when `contact.speed >= limits.resting` and apply the pop the same way.
- `fingerprint` hashes each thing's tags (sorted) after its kind.

- [ ] **Step 1: Write the failing tests** (`testing.rs` gets a `TestBumpy` Bounce module kind `"bounce.bumpy"` with settings `bounciness`, `target: Target`, `pops_after: u32`, implementing `bounciness`, `pairs` (friction 0) and `effects` (counts hits, pops at `pops_after`), and `pub fn dots_pair(gap: f32, target: Target) -> (Scene, Resolved, Sims)`: no gravity, two Dots things each one ball of radius 10 at `(-gap, 0)` and `(gap, 0)` moving towards each other at 100 px/s, both using `bounce.bumpy`; the second has tag `"soft"`):

```rust
#[test]
fn two_balls_heading_at_each_other_bounce_and_log_one_hit() {
    let (scene, resolved, sims) = dots_pair(30.0, Target::Anything);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let hits: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Hit).collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!((hits[0].a.thing, hits[0].b.map(|b| b.thing)), (DOTS, Some(DOTS2)));
    assert!((hits[0].speed - 200.0).abs() < 5.0);
    let (a, b) = (w.bodies[&DOTS].velocity[0], w.bodies[&DOTS2].velocity[0]);
    assert!(a.x < -90.0 && b.x > 90.0, "{a:?} {b:?}");
}

#[test]
fn a_target_that_does_not_match_lets_balls_pass_through() {
    let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("hard".into()));
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    assert!(log.is_empty());
    assert!(w.bodies[&DOTS].position[0].x > 0.0);
    let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("soft".into()));
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(1.0, &sims, &mut Log::default());
    // Only the second ball is tagged soft: the first wants it, it doesn't want the first, so no bump.
    assert!(w.bodies[&DOTS].position[0].x > 0.0);
    assert!(w.matches(&Target::Tag("soft".into()), DOTS, DOTS2));
    assert!(!w.matches(&Target::Tag("soft".into()), DOTS2, DOTS));
}

#[test]
fn pair_collisions_are_deterministic_in_a_pile() {
    let (scene, resolved, sims) = dots_pile(400); // 400 balls of radius 6 in a ring of 150 with gravity, all bumpy
    let run = || { let mut w = World::build(&scene, &resolved, &sims); let mut log = Log::default(); w.advance_to(3.0, &sims, &mut log); (w, log) };
    let (w1, l1) = run();
    let (w2, l2) = run();
    assert_eq!(l1.hash(), l2.hash());
    assert_eq!(w1.bodies[&DOTS].position, w2.bodies[&DOTS].position);
    assert!(l1.events().iter().any(|e| e.b.is_some_and(|b| b.thing == DOTS)), "balls hit each other");
    assert!(w1.bodies[&DOTS].position.iter().all(|p| p.is_finite() && p.length() < 160.0));
}

#[test]
fn a_ball_pops_after_its_hits_and_logs_a_remove() {
    let (scene, resolved, sims) = dots_pair_popping(2); // bumpy with pops_after 2, in a ring so the walls count too
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(5.0, &sims, &mut log);
    let removes: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Remove).collect();
    assert_eq!(removes.len(), 1);
    assert!(!w.bodies[&DOTS].alive[0]);
    assert!(log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == DOTS).count() == 2);
}

#[test]
fn tags_change_the_fingerprint() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let before = fingerprint(&scene, &resolved, &sims);
    scene.things.get_mut(&DOTS).unwrap().tags.insert("x".into());
    assert_ne!(fingerprint(&scene, &resolve(&scene, &reg()), &sims), before);
}
```

And in `pairs.rs`, a grid test ported from the old crate: `pairs_are_visited_once_in_a_fixed_order` (50 points on a 7-wide lattice; every pair at distance < 2 cells appears exactly once, `i < j`, and the order is identical on two builds).

- [ ] **Step 2: Run** `cargo test -p bpe-runtime`. Expected: FAIL.
- [ ] **Step 3: Implement** the hooks, `pairs.rs`, `World::tags`/`matches`, the pair stage and the pop handling in the wall and solid loops.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime -p bpe-kit`. Expected: PASS, goldens unchanged (no stage 2 module returns a `PairRule`).
- [ ] **Step 5: Commit** `git commit -m "Runtime: ball-to-ball collisions, bounce hooks for effects and targets"`

---

### Task 5: Rounded arenas: the Rounded outline, shader and arena settings

**Files:**
- Modify: `crates/bpe-runtime/src/walls.rs`, `crates/bpe-gfx/src/frame.rs`, `crates/bpe-gfx/src/shaders/scene.wgsl`, `crates/bpe-kit/src/things/arena/mod.rs`, `crates/bpe-kit/src/things/arena/sim.rs`, `crates/bpe-studio/src/preview.rs` (match arms only)

**Interfaces:**
- Produces:
  - `Outline::Rounded { center: Vec2, half: Vec2, corner: f32, angle: f32, thickness: f32 }`: a box with round corners; `corner` is the centre line's corner radius, at most `half.min_element()` (a capsule when equal). `point`, `center`, `thickness`, `inside`, `keep_inside` and `ArenaState::outline` (scale and turn) handle it. `keep_inside`: in local coordinates, inner half `h = half − thickness/2 − radius` per axis (a non-positive axis pins the ball on that axis like `Box`), inner corner `c = (corner − thickness/2 − radius).max(0)`, core `k = h − c`; `q = p.clamp(−k, k)`, `d = p − q`; when `|d| > c` move the ball to `q + d/|d| * c` with outward normal `d/|d|` and report the contact.
  - `ShapeInstance::outline` writes `Rounded` as kind 1 with `b = [half.x, half.y, corner, 0]`; the shader's box distance subtracts `b.z` from the SDF of a box shrunk by `b.z`. `ShapeInstance::capsule(from: Vec2, to: Vec2, half_width: f32, color, glow) -> Self` is kind 2: `a = [mid.x, mid.y, angle, 0]`, `b = [half_length, 0, 0, 0]`, `c = [2, half_width, glow, 0]`; the shader rotates into the capsule's frame and uses `segment_distance`. `ShapeInstance::disc(center, radius, color, glow)` is a kind 0 ring with `thickness = 2 * radius` (filled).
  - `ArenaShape::Capsule`; `ArenaSettings.corner: f32` (label "Corner radius", help "Box only: how round the corners are.", range 0..=1000, px, keyframe, random). `ArenaSim::build` makes `Outline::Box` when `shape == Box && corner <= 0`, `Outline::Rounded` when `corner > 0` (corner clamped to `half.min_element()`), and `Rounded` with `corner = half.min_element()` for `Capsule`. `ArenaSim::affects_physics` stays `true`.
  - `preview::outline_point_towards` and `apply_drag` treat `Rounded` like `Box` (width/height).

- [ ] **Step 1: Write the failing tests** in `walls.rs`, `frame.rs` and `arena/sim.rs`:

```rust
#[test]
fn a_rounded_box_keeps_balls_off_its_round_corners() {
    let wall = Wall { outline: Outline::Rounded { center: Vec2::ZERO, half: Vec2::new(100.0, 100.0), corner: 40.0, angle: 0.0, thickness: 2.0 }, spin: 0.0, growth: 0.0 };
    // In the corner, where a sharp box would allow (94, 94), the round one does not.
    let mut p = Vec2::new(94.0, 94.0);
    let c = keep_inside(&mut p, Vec2::new(10.0, 10.0), 5.0, &wall).unwrap();
    let core = 100.0 - 1.0 - 5.0 - 34.0; // half - thickness/2 - radius - inner corner
    assert!(((p - Vec2::splat(core)).length() - 34.0).abs() < 1e-3, "{p:?}");
    assert!((c.normal + Vec2::splat(FRAC_1_SQRT_2)).length() < 1e-3, "{c:?}");
    // Along a flat side it behaves like the box.
    let mut q = Vec2::new(0.0, 99.0);
    keep_inside(&mut q, Vec2::ZERO, 5.0, &wall);
    assert!((q.y - 94.0).abs() < 1e-4);
    assert!(wall.outline.inside(Vec2::new(90.0, 90.0)) == false && wall.outline.inside(Vec2::new(60.0, 60.0)));
    let rim = wall.outline.point(0.7);
    assert!((rim.length() - 100.0).abs() < 20.0 && !wall.outline.inside(rim * 1.02) && wall.outline.inside(rim * 0.98));
}
```

```rust
#[test]
fn rounded_and_capsule_instances_carry_their_shape() {
    let r = Outline::Rounded { center: Vec2::ZERO, half: Vec2::new(30.0, 40.0), corner: 10.0, angle: 0.1, thickness: 2.0 };
    let s = ShapeInstance::outline(&r, [1.0; 4], 0.0);
    assert_eq!((s.c[0], s.b), (1.0, [30.0, 40.0, 10.0, 0.0]));
    let cap = ShapeInstance::capsule(Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0), 3.0, [1.0; 4], 0.2);
    assert_eq!((cap.c[0], cap.c[1], cap.b[0]), (2.0, 3.0, 10.0));
    assert_eq!(cap.a[..2], [0.0, 0.0]);
    let disc = ShapeInstance::disc(Vec2::new(1.0, 2.0), 7.0, [1.0; 4], 0.0);
    assert_eq!((disc.c[0], disc.c[1], disc.b[0]), (0.0, 7.0, 7.0));
}
```

```rust
#[test]
fn corner_and_capsule_make_rounded_outlines_and_zero_corner_stays_a_box() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let arena = scene.things.get_mut(&ARENA).unwrap();
    arena.params.set("shape", Value::Choice("box".into()));
    arena.params.set("width", Value::Number(200.0));
    arena.params.set("height", Value::Number(400.0));
    let world = |scene: &Scene| World::build(scene, &resolve(scene, &kit.registry), &kit.sims);
    assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Box { .. }));
    scene.things.get_mut(&ARENA).unwrap().params.set("corner", Value::Number(500.0));
    assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Rounded { corner, .. } if corner == 100.0));
    scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("capsule".into()));
    scene.things.get_mut(&ARENA).unwrap().params.remove("corner");
    assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Rounded { corner, half, .. } if corner == 100.0 && half == Vec2::new(100.0, 200.0)));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime -p bpe-gfx -p bpe-kit`. Expected: FAIL (variant missing).
- [ ] **Step 3: Implement**: the variant and its maths, the shader (`shape_distance`: kind 1 uses `q = abs(rotate(p, -angle)) - (half - corner)`, `d = length(max(q, 0)) + min(max(q.x, q.y), 0) - corner`, `abs(d)`; kind 2 (`c.x > 1.5`) uses `segment_distance(rotate(p, -angle), (-b.x, 0), (b.x, 0))`; `vs_shape` extent for kind 2 is `b.x`), the arena settings and the preview arms. Add the picture test `a_rounded_box_arena_draws_round_corners` to `crates/bpe-kit/tests/pictures.rs`: render the "Ball pit" scene at 270×480 and assert the pixel at the wall's sharp-corner position is dark while a pixel on the straight wall is bright (skipped without a GPU, like the other picture test).
- [ ] **Step 4: Run** `cargo test --workspace` (background). Expected: PASS, goldens unchanged (Box drop has `corner` 0).
- [ ] **Step 5: Commit** `git commit -m "Rounded and capsule arenas, capsule and disc shapes"`

---

### Task 6: Kit: the Folder and Obstacle kinds

**Files:**
- Create: `crates/bpe-kit/src/things/folder/mod.rs`, `things/folder/sim.rs`, `things/obstacle/mod.rs`, `things/obstacle/sim.rs`, `things/obstacle/draw.rs`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs`, `crates/bpe-kit/src/things/balls/sim.rs`, `crates/bpe-kit/src/things/arena/sim.rs` (anchor + pose)

**Interfaces:**
- `FolderKind`: id `"folder"`, name `"Folder"`, icon `"folder"`, help `"Groups things. Anything it links in a slot is used by everything inside it."`, settings `FolderSettings {}` (empty, `#[derive(Default, Settings)]`), `slots() == &SlotId::ALL`, `default_slots() == &[]`. `FolderSim`: `build` and `scripted` write `world.poses[id] = world.anchor(id)`.
- `ObstacleKind`: id `"obstacle"`, name `"Obstacle"`, icon `"obstacle"`, help `"A peg, bar, peg grid, polygon or ring inside the arena that balls bounce off."`, slots `[Look, Bounce, Sound, Particles, Rules, Shader]`, default `[Look, Bounce, Sound]`.
  - `#[derive(Choice)] pub enum ObstacleShape { Peg, Bar, PegGrid, Polygon, Ring }` (default Peg); `#[derive(Choice)] pub enum Path { None, Line, Circle }`.
  - `ObstacleSettings` (Simple: shape, radius, length, thickness, spin, path): `shape` (random); `radius: f32 = 30` ("Size", "Peg, grid peg, polygon or ring: from the middle to the edge.", 2..=800 px, keyframe, random); `length: f32 = 400` ("Length", "Bar only.", 10..=4000 px, keyframe, random); `thickness: f32 = 8` ("Thickness", "Bar, polygon and ring: how thick the wall is.", 1..=200 px, keyframe); `spin: f32 = 0` ("Spin", -720..=720 deg/s, keyframe, random); `path: Path = None` ("Moves along", "A straight line back and forth, or a circle."); More: `path_distance: Vec2 = (300, 0)` ("Travel", "Line: how far each way. Circle: x is the circle's radius.", px, keyframe); `path_seconds: f32 = 4` ("Round trip", 0.2..=120 s); `path_phase: f32 = 0` ("Start phase", 0..=1 turns); `sides: u32 = 6` ("Sides", "Polygon only.", 3..=12, random); `columns: u32 = 7` ("Columns", "Peg grid only.", 1..=100); `rows: u32 = 9` ("Rows", 1..=100); `spacing: f32 = 90` ("Spacing", 4..=1000 px); `stagger: bool = true` ("Stagger", "Peg grid: shift every other row by half a step, like a Plinko board.").
  - `pub fn parts(s: &ObstacleSettings) -> Vec<Solid>` in local space: Peg → one `Disc` at origin; Bar → one `Capsule` from `(-length/2, 0)` to `(length/2, 0)` radius `thickness/2`; PegGrid → discs row by row from the top like `balls::offsets` (a staggered odd row is shifted `+spacing/2` and has one peg fewer); Polygon → one `Capsule` per edge of the regular polygon of `radius` with `sides`, first corner at `-90°`, capsule radius `thickness/2`; Ring → one `Solid::Ring { radius, thickness }`.
  - `pub fn path_offset(s: &ObstacleSettings, time: f64) -> (Vec2, Vec2)`: (offset, velocity): None → zero; Line → `d * sin(φ)`, velocity `d * cos(φ) * ω` with `φ = TAU * (time / path_seconds + path_phase)`, `ω = TAU / path_seconds`; Circle → `(cos φ, sin φ) * d.x`, velocity `(-sin φ, cos φ) * d.x * ω`.
  - `ObstacleSim`: `build` and `scripted` compute `base = world.anchor(id)`, `(offset, v) = path_offset(s, world.time())`, `angle = base.angle + spin.to_radians() * time`, pose = `Pose { position: base.position + offset, angle, velocity: base.velocity + v, spin: base.spin + spin.to_radians() }`, then world parts = local parts rotated by `angle` and moved to `position`; writes `world.solids[id]` and `world.poses[id]`. `affects_physics` true for everything.
  - `ObstacleDraw`: Disc → `ShapeInstance::disc`, Capsule → `ShapeInstance::capsule`, Ring → `ShapeInstance::outline(&Outline::Ring { sides: 0, .. })`, with the look's colour and `wall_glow`.
  - `BallsSim::build` starts at `world.anchor(thing.id)`: each ball at `pose.position + Vec2::from_angle(pose.angle).rotate(offset)` with velocity `settings.velocity + pose.velocity + (ball − pose.position).perp() * pose.spin`. `ArenaSim::build` and `scripted` take `base.center` and angle from `world.anchor` and write `world.poses[id]` (centre, current angle, zero velocity, spin). For stage 2 scenes (no riders) every anchor is the thing's own transform, so nothing changes.
- `registry()`, `sims()`, `draws()` register the two kinds.

- [ ] **Step 1: Write the failing tests** (in `obstacle/sim.rs`, `folder/sim.rs`, `balls/sim.rs`, and `lib.rs`):

```rust
#[test]
fn parts_follow_the_shape_settings() {
    let mut s = ObstacleSettings::default();
    assert_eq!(parts(&s), vec![Solid::Disc { center: Vec2::ZERO, radius: 30.0 }]);
    s.shape = ObstacleShape::Bar;
    assert_eq!(parts(&s), vec![Solid::Capsule { from: Vec2::new(-200.0, 0.0), to: Vec2::new(200.0, 0.0), radius: 4.0 }]);
    s.shape = ObstacleShape::PegGrid;
    s.columns = 3; s.rows = 2; s.spacing = 10.0; s.stagger = true;
    let pegs = parts(&s);
    assert_eq!(pegs.len(), 5, "a staggered second row has one peg fewer");
    assert!(matches!(pegs[3], Solid::Disc { center, .. } if (center - Vec2::new(-5.0, 5.0)).length() < 1e-4), "{pegs:?}");
    s.shape = ObstacleShape::Polygon; s.sides = 4;
    assert_eq!(parts(&s).len(), 4);
    s.shape = ObstacleShape::Ring;
    assert_eq!(parts(&s), vec![Solid::Ring { center: Vec2::ZERO, radius: 30.0, thickness: 8.0 }]);
}

#[test]
fn a_bar_on_a_line_path_moves_back_and_forth() {
    let s = ObstacleSettings { shape: ObstacleShape::Bar, path: Path::Line, path_distance: Vec2::new(100.0, 0.0), path_seconds: 4.0, ..Default::default() };
    assert!((path_offset(&s, 1.0).0 - Vec2::new(100.0, 0.0)).length() < 1e-3);
    assert!((path_offset(&s, 3.0).0 - Vec2::new(-100.0, 0.0)).length() < 1e-3);
    assert!(path_offset(&s, 0.0).1.x > 100.0, "fastest in the middle");
    let (scene, resolved, kit) = scene_with_obstacle(|p| { p.set("shape", Value::Choice("bar".into())); p.set("spin", Value::Number(90.0)); });
    let mut w = World::build(&scene, &resolved, &kit.sims);
    w.advance_to(1.0, &kit.sims, &mut Log::default());
    let pose = w.poses[&OBSTACLE];
    assert!((pose.angle - FRAC_PI_2).abs() < 0.01, "{pose:?}");
    assert!(matches!(w.solids[&OBSTACLE].parts[0], Solid::Capsule { from, .. } if from.x.abs() < 1.0 && from.y < -190.0));
}

#[test]
fn a_big_peg_board_steps_a_second_in_under_a_second() {
    // 20 x 20 pegs and a 20 x 20 block of balls, released mode.
    let (scene, resolved, kit) = peg_board(20, 20, 20);
    let mut w = World::build(&scene, &resolved, &kit.sims);
    let started = Instant::now();
    w.advance_to(1.0, &kit.sims, &mut Log::default());
    assert!(started.elapsed() < Duration::from_secs(if cfg!(debug_assertions) { 6 } else { 1 }), "{:?}", started.elapsed());
}
```

```rust
#[test]
fn balls_riding_a_spinning_arena_start_with_its_rim_speed() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ARENA).unwrap().params.set("spin", Value::Number(90.0)); // clockwise quarter turn per second
    scene.set_parent(BALLS, ARENA, Attach::Ride { offset: Vec2::new(0.0, -300.0), turn_with_parent: true, item: None }).unwrap();
    scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
    let resolved = resolve(&scene, &kit.registry);
    let w = World::build(&scene, &resolved, &kit.sims);
    assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(0.0, -300.0));
    let v = w.bodies[&BALLS].velocity[0];
    // The rim at (0, -300) turning clockwise (y down) moves in +x at 300 * pi/2 px/s.
    assert!((v.x - 300.0 * FRAC_PI_2).abs() < 0.5 && v.y.abs() < 1e-3, "{v:?}");
}

#[test]
fn a_peg_riding_a_ball_follows_it() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let peg = scene.add_thing(&kit.registry, "obstacle", BALLS).unwrap();
    scene.set_attach(peg, Attach::Ride { offset: Vec2::new(0.0, 60.0), turn_with_parent: false, item: Some(0) }).unwrap();
    scene.things.get_mut(&peg).unwrap().params.set("radius", Value::Number(5.0));
    let resolved = resolve(&scene, &kit.registry);
    let mut w = World::build(&scene, &resolved, &kit.sims);
    w.advance_to(0.5, &kit.sims, &mut Log::default());
    let ball = w.bodies[&BALLS].position[0];
    assert!((w.poses[&peg].position - (ball + Vec2::new(0.0, 60.0))).length() < 1e-3);
}
```

```rust
#[test]
fn folders_add_with_no_slots_and_place_their_riders() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let f = scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
    assert!(scene.things[&f].slots.is_empty());
    scene.things.get_mut(&f).unwrap().transform.position = Vec2::new(100.0, 50.0);
    scene.set_parent(BALLS, f, Attach::Ride { offset: Vec2::new(10.0, 0.0), turn_with_parent: false, item: None }).unwrap();
    let resolved = resolve(&scene, &kit.registry);
    let w = World::build(&scene, &resolved, &kit.sims);
    assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(110.0, 50.0));
    assert_eq!(bpe_core::check_registry(&kit.registry), Vec::<String>::new());
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement** the two kinds, the anchor use in balls and arena, the registrations, and `testing::scene_with_obstacle(f) -> (Scene, Resolved, Kit)` / `testing::peg_board(columns, rows, balls_per_side)` / `pub const OBSTACLE: ThingId = ThingId(3)`.
- [ ] **Step 4: Run** `cargo test -p bpe-kit`. Expected: PASS, goldens unchanged.
- [ ] **Step 5: Commit** `git commit -m "Kit: Folder and Obstacle things, riders spawn on their parent"`

---

### Task 7: Kit: more Look and Motion options

**Files:**
- Modify: `crates/bpe-kit/src/modules/look/mod.rs`, `modules/look/draw.rs`, `things/balls/draw.rs`, `modules/motion/mod.rs`, `modules/motion/sim.rs`

**Interfaces:**
- `ClassicLook` gains (all More): `gradient_by: GradientBy = Row` (`#[derive(Choice)] enum GradientBy { Row, Order, Speed, Hits }`, label "Colours follow", help "What spreads the Colours across the balls: the row they start in, their order, how fast they go, or how often they have hit."), `gradient_full: f32 = 1000` ("Colours end at", "Speed in px/s or number of hits at which the last colour is reached.", 1..=20000, random), `hue_per_hit: f32 = 0` ("Hue shift per hit", "Every hit turns the colour this far round the colour wheel.", -180..=180 deg, keyframe, random).
- `look::draw::colors(look, count, row_of)` becomes `pub fn colors<'a>(look: &'a ClassicLook, bodies: &'a Bodies, row_of: impl Fn(usize) -> f32 + 'a) -> impl Fn(usize) -> [f32; 4] + 'a`: `t` from `gradient_by` (`Order`: `i / (len − 1)`; `Speed`: `|v| / gradient_full`; `Hits`: `hits / gradient_full`, clamped to 0..=1), then the hue shifted by `hue_per_hit * hits` degrees via `Color::shift_hue` on the sampled colour (`Color::from_linear` → shift → `to_linear`). `hue_per_hit == 0` and `gradient_by == Row` with an empty gradient give exactly the stage 2 colours.
- `BasicMotion` gains `wind: Vec2 = ZERO` (More, label "Wind", help "A steady push on these, in pixels per second squared.", px/s², keyframe, random, pulse); `forces` adds `wind * dt` with the gravity pull.

- [ ] **Step 1: Write the failing tests** in `balls/draw.rs` and `motion/sim.rs`:

```rust
#[test]
fn colours_can_follow_hits_and_shift_hue_per_hit() {
    let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| { p.set("columns", Value::Int(2)); p.set("rows", Value::Int(1)); });
    let m = resolve_slot(&scene, &kit.registry, BALLS, SlotId::Look, &mut Vec::new()).unwrap().module;
    let look = scene.library.modules.get_mut(&m).unwrap();
    look.params.set("gradient", Value::Colors(vec![Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
    look.params.set("gradient_by", Value::Choice("hits".into()));
    look.params.set("gradient_full", Value::Number(2.0));
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    world.bodies.get_mut(&BALLS).unwrap().hits[1] = 2;
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
    assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(frame.balls[1].color, [0.0, 0.0, 1.0, 1.0]);
    scene.library.modules.get_mut(&m).unwrap().params.set("hue_per_hit", Value::Number(120.0));
    let resolved = resolve(&scene, &kit.registry);
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
    // Red turned 240° (two hits of 120°) is blue-ish; the first ball with no hits stays red.
    assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
    assert!(frame.balls[1].color[2] > 0.9, "{:?}", frame.balls[1].color);
}
```

```rust
#[test]
fn wind_pushes_balls_sideways() {
    let (scene, resolved, kit) = crate::testing::one_ball_with_motion(|p| p.set("wind", Value::Vec2([400.0, 0.0])));
    let mut w = World::build(&scene, &resolved, &kit.sims);
    let x0 = w.bodies[&BALLS].position[0].x;
    w.advance_to(0.25, &kit.sims, &mut Log::default());
    // 300 px/s launch plus 400 px/s² wind for a quarter second: about 75 + 12.5 px.
    assert!((w.bodies[&BALLS].position[0].x - x0 - 87.5).abs() < 2.0);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement**, with `testing::one_ball_with_motion(f)` like `one_ball_with_bounce` for the Motion slot.
- [ ] **Step 4: Run** `cargo test -p bpe-kit`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Kit: colours by speed and hits, hue shift per hit, wind"`

---

### Task 8: Kit: Bounce options, ball hits and targets

**Files:**
- Modify: `crates/bpe-kit/src/modules/bounce/mod.rs`, `modules/bounce/sim.rs`

**Interfaces:**
- `BasicBounce` gains: `hit_balls: bool = false` (Simple, label "Hits other balls", help "These balls bump into other balls instead of passing through them."); More: `balls_to_hit: Target = Anything` ("Which balls", "Only balls on these things are bumped into; both sides have to agree."); `friction: f32 = 0` ("Friction", "How much sideways speed rubs off when balls touch each other.", 0..=1, keyframe, random); `pops_after: u32 = 0` ("Pops after", "The ball disappears after this many hits. 0 never pops it.", 0..=1000, random); group "Against some things": `special: bool = false` ("Different against some things", "Use the settings below when hitting the things picked here."), `special_target: Target = Anything` ("Those things"), `special_bounciness: f32 = 1.0` ("Bounciness against them", 0..=1.5, keyframe, random), `special_grow: f32 = 0` ("Grow against them", -20..=20 px, keyframe, random).
- `BasicBounceSim` implements `bounciness` (`b.bounciness`), `pairs` (`Some(PairRule { target: balls_to_hit, friction })` when `hit_balls`), `effects` (speed gain, grow with max_size, top speed, then `hits >= pops_after` when `pops_after > 0` → true), `special_target` (`Some(special_target)` when `special`), and `respond_against(.., special)`: `reflect` with `special_bounciness` when `special` else `bounciness`, then `effects` with `special_grow` in place of `grow` when `special`. The runtime's wall and solid loops: `special = sim.special_target(state).is_some_and(|t| world.matches(&t, mover, other))` (Task 4 added the hook; here the loops start passing it).

- [ ] **Step 1: Write the failing tests** in `bounce/sim.rs`:

```rust
#[test]
fn hit_balls_makes_a_block_collide_and_the_log_names_both_balls() {
    let (scene, resolved, kit) = crate::testing::scene_with_balls("block", |p| { p.set("columns", Value::Int(2)); p.set("rows", Value::Int(1)); p.set("spacing", Value::Number(60.0)); p.set("velocity", Value::Vec2([0.0, 0.0])); });
    let mut scene = scene;
    let m = resolve_slot(&scene, &kit.registry, BALLS, SlotId::Bounce, &mut Vec::new()).unwrap().module;
    scene.library.modules.get_mut(&m).unwrap().params.set("hit_balls", Value::Bool(true));
    scene.things.get_mut(&BALLS).unwrap().transform.position = Vec2::new(0.0, 300.0);
    // Push the two balls towards each other.
    let resolved = resolve(&scene, &kit.registry);
    let mut w = World::build(&scene, &resolved, &kit.sims);
    { let b = w.bodies.get_mut(&BALLS).unwrap(); b.velocity[0] = Vec2::new(200.0, 0.0); b.velocity[1] = Vec2::new(-200.0, 0.0); }
    let mut log = Log::default();
    w.advance_to(0.2, &kit.sims, &mut log);
    let pair = log.events().iter().find(|e| e.b == Some(Party { thing: BALLS, item: 1 })).expect("the balls met");
    assert_eq!(pair.a, Party { thing: BALLS, item: 0 });
    assert!(pair.speed > 350.0, "{pair:?}");
}

#[test]
fn a_special_target_changes_the_bounce_against_it_only() {
    let (scene, resolved, kit) = crate::testing::one_ball_with_bounce(|p| {
        p.set("special", Value::Bool(true));
        p.set("special_target", Value::Target(Target::Kind("arena".into())));
        p.set("special_bounciness", Value::Number(0.0));
    });
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(20.0);
    let body = &playback.world().bodies[&BALLS];
    assert!((body.position[0].length() - 482.5).abs() < 1.0, "dead against the arena: {:?}", body.position[0]);
    let (scene, resolved, kit) = crate::testing::one_ball_with_bounce(|p| {
        p.set("special", Value::Bool(true));
        p.set("special_target", Value::Target(Target::Tag("pegs".into())));
        p.set("special_bounciness", Value::Number(0.0));
    });
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(5.0);
    assert!(playback.world().bodies[&BALLS].velocity[0].length() > 300.0, "lively against everything else");
}

#[test]
fn pops_after_removes_the_ball_and_logs_it() {
    let (scene, resolved, kit) = crate::testing::one_ball_with_bounce(|p| p.set("pops_after", Value::Int(3)));
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(20.0);
    let log = playback.log();
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Hit).count(), 3);
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Remove).count(), 1);
    assert!(!playback.world().bodies[&BALLS].alive[0]);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement**. `respond` keeps its stage 2 body (so `respond_against(.., false)` is identical to before).
- [ ] **Step 4: Run** `cargo test -p bpe-kit -p bpe-runtime`. Expected: PASS, goldens unchanged.
- [ ] **Step 5: Commit** `git commit -m "Kit: balls that hit each other, pops, and a special bounce against chosen things"`

---

### Task 9: Kit and mixer: Sound options and both roles of a hit

**Files:**
- Modify: `crates/bpe-mix/src/lib.rs` (`mix` only), `crates/bpe-kit/src/modules/sound/mod.rs`, `modules/sound/mix.rs`

**Interfaces:**
- `bpe_mix::mix` passes a thing every event where it is party `a` **or** party `b` (the doc comment of `ModuleMix::notes` says so). Each module reads `e.a.thing == thing` to know its role.
- `NoteSound` gains: `pitch_by: PitchBy = Random` (Simple, `#[derive(Choice)] enum PitchBy { Random, Speed, Hits }`, label "Note picked by", help "Which note of the chord a hit plays: a random one, climbing with the hit's speed, or climbing with how often the ball has hit."); More: `plays_when: PlaysWhen = Hitting` (`enum PlaysWhen { Hitting, BeingHit, Both }`, label "Plays when", help "Whether this sounds when its own balls hit something, when something hits it, or both."); `sounds_for: Target = Anything` ("Only hits against", "Hits against anything else are silent."); `min_gap: f32 = 0` ("Shortest gap", "Two notes from this thing closer than this play as one.", 0..=1 s).
- `NoteSoundMix::notes`: skips events whose role is not wanted (`plays_when`); the other party is `b` when `a.thing == thing` else `a`; `sounds_for` is checked with `(cx.matches)(&target, thing, other.thing)` through a new `MixCx` field `pub matches: &'a dyn Fn(&Target, ThingId, ThingId) -> bool` (built in `mix` from the kinds in `resolved` and a new `MixSettings` field `pub tags: BTreeMap<ThingId, BTreeSet<String>>`); the note index: `Random` as today; `Speed`: `(speed / loudest_hit * (len - 1)).round()`; `Hits`: the number of earlier hits of the same party in `events` (counted while iterating, so it is the ball's hit count), modulo the chord's length; `min_gap`: a note is dropped when the last note this module placed for this thing is less than `min_gap` seconds earlier.
- `bpe_kit::mix_settings` fills `tags` from the scene (signature becomes `mix_settings(scene: &Scene, resolved: &Resolved)`; `mix_scene` already receives the scene).

- [ ] **Step 1: Write the failing tests** in `sound/mix.rs`:

```rust
#[test]
fn a_thing_being_hit_sounds_only_when_asked_and_only_for_its_targets() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let resolved = resolve(&scene, &kit.registry);
    let mut log = Log::default();
    log.push(hit(240, 0.0, 500.0)); // ball (2) hits arena (1)
    let plain = mix_scene(&scene, &resolved, &log, &kit);
    // Give the arena its own sound that plays when it is hit: the mix now carries two notes.
    let m = scene.add_module(&kit.registry, "sound.notes", "Wall sound").unwrap();
    scene.library.modules.get_mut(&m).unwrap().params.set("plays_when", Value::Choice("being_hit".into()));
    scene.link(&kit.registry, ThingId(1), SlotId::Sound, m).unwrap();
    let resolved = resolve(&scene, &kit.registry);
    let both = mix_scene(&scene, &resolved, &log, &kit);
    assert_ne!(both.samples, plain.samples);
    // Pointing the wall's sound at a tag nothing has silences it again.
    scene.library.modules.get_mut(&m).unwrap().params.set("sounds_for", Value::Target(Target::Tag("pegs".into())));
    let resolved = resolve(&scene, &kit.registry);
    assert_eq!(mix_scene(&scene, &resolved, &log, &kit).samples, plain.samples);
}

#[test]
fn notes_can_climb_with_hits_and_quick_repeats_merge() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let m = resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Sound, &mut Vec::new()).unwrap().module;
    scene.library.modules.get_mut(&m).unwrap().params.set("pitch_by", Value::Choice("hits".into()));
    let resolved = resolve(&scene, &kit.registry);
    let mut log = Log::default();
    for step in [240, 480, 720] { log.push(hit(step, 0.0, 500.0)); }
    let settings = bpe_kit::mix_settings(&scene, &resolved);
    let mut notes = Vec::new();
    let cx = test_cx(&settings, &log);
    NoteSoundMix.notes(&resolved.things[&ThingId(2)].slots[&SlotId::Sound].params, ThingId(2), log.events(), &cx, &mut notes);
    assert_eq!(notes.len(), 3);
    assert!(notes[0].key < notes[1].key && notes[1].key < notes[2].key, "{:?}", notes.iter().map(|n| n.key).collect::<Vec<_>>());
    scene.library.modules.get_mut(&m).unwrap().params.set("min_gap", Value::Number(1.5));
    let resolved = resolve(&scene, &kit.registry);
    let mut few = Vec::new();
    NoteSoundMix.notes(&resolved.things[&ThingId(2)].slots[&SlotId::Sound].params, ThingId(2), log.events(), &cx, &mut few);
    assert_eq!(few.len(), 2, "1.0 s, (2.0 s dropped), 3.0 s");
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit -p bpe-mix`. Expected: FAIL.
- [ ] **Step 3: Implement**, with a `test_cx` helper in the test module building a `MixCx` with an always-true `matches`. Update `bpe-cli` and `bpe-studio` callers of `mix_settings` if any (`grep -rn mix_settings crates/`).
- [ ] **Step 4: Run** `cargo test --workspace` (background). Expected: PASS; the stage 2 sound tests still pass because `Hitting` is the default.
- [ ] **Step 5: Commit** `git commit -m "Sound: notes by hits or speed, shortest gap, sounds for being hit, target filters"`

---

### Task 10: Five stage 3 scenes, golden logs, pictures, export check

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs`, `crates/bpe-kit/tests/golden.rs`, `tests/golden.txt`, `tests/golden-debug.txt`, `tests/pictures.rs`, `crates/bpe-cli/tests/export.rs`
- Create: `scenes/next/peg-board.ron`, `spinning-bars.ron`, `ball-pit.ron`, `riders.ron`, `pitch-climb.ron` (blessed)

**Interfaces:**
- `pub fn stage3(reg: &Registry) -> Vec<(String, Scene)>` and `pub fn all(reg: &Registry) -> Vec<(String, Scene)>` (stage 2 then stage 3). The scenes (built with the stage 2 `Builder`, extended with `fn add(&mut self, kind, parent, name, position, params) -> ThingId`, `fn ride(&mut self, id, parent, offset, turn)`, `fn tag(&mut self, id, tag)`):
  1. **Peg board**: box arena 900×1500 corner 60; obstacle "Pegs" PegGrid 7×9 spacing 100 radius 9 at (0, 100), tag `pegs`, own Sound "Peg sound" (`plays_when: BeingHit`, instrument Marimba, `sounds_for: Anything`); Balls "Drops" block 8×6 spacing 30 radius 8 at (0, −650) velocity (0, 0), `hit_balls` on (own Bounce "Bumpy" bounciness 0.6), root Sound off for the balls' slot (`Off` on Drops) so only pegs sound; gravity (0, 900); 20 s.
  2. **Spinning bars**: circle arena 500; folder "Bars" with three Bar obstacles (length 600, thickness 10) riding the folder at offset zero with `turn_with_parent`, rotations 0°, 60°, 120°, the folder itself having no motion, each bar `spin` 40; Balls ring of 18 radius 12 ring radius 380 velocity (0, 0); Bounce speed gain 1.01.
  3. **Ball pit**: capsule arena width 700 height 1400; Balls block 10×14 spacing 48 radius 20 at (0, −300) velocity (120, 0); root Bounce `hit_balls` on, bounciness 0.5, friction 0.3; look gradient 3 colours by `Hits`, `gradient_full` 12; gravity (0, 1200); 15 s.
  4. **Riders**: hexagon arena radius 450 spin 30; Balls "Rim riders" ring of 6 radius 14 ring radius 60 riding the arena at offset (0, −330) with `turn_with_parent`, velocity 0; obstacle "Peg" radius 24 riding the arena at (0, 200) with `turn_with_parent`; Bounce `pops_after` 40; `hue_per_hit` 12.
  5. **Pitch climb**: one ball radius 30 at (0, −300) velocity (350, 0); an obstacle "Soft bar" Bar length 500 thickness 12 at (0, 350), tag `soft`; Bounce `special` on against `Tag("soft")` with `special_bounciness` 0.3; Sound `pitch_by: Hits`, Marimba; Motion wind (60, 0).
- `golden.rs`, `pictures.rs` use `scenes::all`. The golden files gain five lines each (`BLESS_GOLDEN=1` once in debug and once in `--release`), the stage 2 lines unchanged. `the_scenes_are_lively` requires ≥ 1 hit for the new scenes and ≥ 100 for "Peg board" and "Ball pit". `the_shipped_scene_files_match_the_code` covers `all`.
- `export.rs` gains `exports_a_stage3_scene_with_an_obstacle` (Peg board, 30 frames, quarter size) checking duration ≈ 0.5 s and both streams, skipped without GPU/ffmpeg.

- [ ] **Step 1: Write the scenes and the test changes**, run `cargo test -p bpe-kit --test golden`. Expected: FAIL ("event logs changed" / missing lines).
- [ ] **Step 2: Bless** with `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden` and `BLESS_GOLDEN=1 cargo test --release -p bpe-kit --test golden`, and `BPE_BLESS=1 cargo test -p bpe-kit stage_scenes_resolve_and_round_trip`. Check with `git diff tests/golden.txt` that the first five lines are untouched.
- [ ] **Step 3: Run** `cargo test --workspace` (background). Expected: PASS.
- [ ] **Step 4: Commit** `git commit -m "Five stage 3 scenes with golden logs"`

---

### Task 11: Studio: selection, folders, drag and drop in the tree

**Files:**
- Create: `crates/bpe-studio/src/selection.rs`
- Modify: `crates/bpe-studio/src/tree.rs`, `crates/bpe-studio/src/app.rs`, `crates/bpe-studio/src/main.rs`, `crates/bpe-studio/src/theme.rs` (`OVERRIDE` colour `Color32::from_rgb(0xff, 0xa8, 0x4a)`)

**Interfaces:**
- `selection.rs`: `#[derive(Default)] pub struct Selection { pub primary: Option<ThingId>, pub extra: BTreeSet<ThingId> }` with `pub fn set(&mut self, id)` (primary only), `pub fn toggle(&mut self, id)` (Ctrl), `pub fn range(&mut self, to: ThingId, visible: &[ThingId])` (Shift: everything between the primary and `to` in `visible` goes into `extra`, primary unchanged), `pub fn all(&self, scene: &Scene) -> Vec<ThingId>` (primary first, then `extra` in tree order, only ids still in the scene), `pub fn contains(&self, id) -> bool`, `pub fn remove(&mut self, ids: &[ThingId])`, `pub fn clear(&mut self)`.
- `Studio.selected: Option<ThingId>` is replaced by `pub selection: Selection` (`selection.primary` everywhere `selected` was read). `tree::show(ui, tree, scene, kit, resolved, selection: &mut Selection) -> TreeOutcome`; `TreeOutcome` gains `pub message: Option<String>` (a refused drop's words) which `app.rs` copies into `status`.
- `Tree` gains `pub collapsed: BTreeSet<ThingId>`. Rows: a ▸/▾ toggle (painted triangle, `Sense::click`, named "Collapse <name>"/"Expand <name>") before the icon for things with children; children of a collapsed thing are not drawn; a riding thing shows a painted ↳ hook left of its icon; icons for `"folder"` (a tab-topped rectangle) and `"obstacle"` (a short bar with a dot).
- Clicks: plain → `selection.set`; Ctrl (`modifiers.command`) → `toggle`; Shift → `range` over the visible row order. Delete removes every selected thing (`remove_thing` on each that still exists, parents first) as one undo step.
- Drag and drop: each row's name is wrapped in `ui.dnd_drag_source(ui.make_persistent_id(("row", id)), id, ...)` (payload `ThingId`); every row (`response.rect` of the whole row, union with the toggle and eye) checks `dnd_hover_payload::<ThingId>()` to paint a line above/below or a highlight, and `dnd_release_payload::<ThingId>()` to act. `pub fn drop_action(row_rect: Rect, pointer: Pos2, alt: bool) -> Drop` with `pub enum Drop { Before, Into(Attach), After }`: the top quarter is `Before`, the bottom quarter `After`, the middle `Into(Group)` or `Into(Ride { offset: ZERO, turn_with_parent: true, item: None })` with Alt. `pub fn apply_drop(scene: &mut Scene, dragged: ThingId, onto: ThingId, drop: Drop) -> Result<(), EditError>`: `Before`/`After` → `set_parent(dragged, onto.parent, dragged's current attach)` then `move_before(dragged, onto)` or before `onto`'s next sibling (end when none); `Into(a)` → `set_parent(dragged, onto, a)`; `Ride` keeps the thing's world position by setting `offset = dragged.position − onto.position` when `onto` is not the root. Dropping onto the root as `Before`/`After` is refused with `EditError::Root`. A `ModuleId` payload (Task 12) on a row links it: `scene.link(reg, id, module.slot, module)`, refused errors become `message`.
- Context menu gains: "Make folder from selection" (`make_folder` with `selection.all`), "Select things using the same module ▸" (one entry per Linked slot: selects `users_of(module)`), "Ride on parent" / "Group under parent" (`set_attach`), "Attach to ▸" (every thing except itself and its descendants; `set_parent` with `Ride` keeping the world position).
- The "+ Add thing" menu lists kinds with their help on hover (as today) and adds the new thing under the primary selection when it is a folder, else beside it.

- [ ] **Step 1: Write the failing tests** in `selection.rs` and `tree.rs`:

```rust
#[test]
fn toggling_ranging_and_listing_a_selection() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let c = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
    let d = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
    let mut s = Selection::default();
    s.set(ThingId(1));
    s.toggle(d);
    assert_eq!(s.all(&scene), vec![ThingId(1), d]);
    s.toggle(d);
    assert_eq!(s.all(&scene), vec![ThingId(1)]);
    s.range(c, &scene.order);
    assert_eq!(s.all(&scene), vec![ThingId(1), ThingId(2), c]);
    s.remove(&[ThingId(1)]);
    assert_eq!(s.primary, None);
    assert!(s.contains(c) && !s.contains(d));
}
```

```rust
#[test]
fn drop_zones_split_a_row_in_three() {
    let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 20.0));
    assert_eq!(drop_action(rect, egui::pos2(10.0, 2.0), false), Drop::Before);
    assert_eq!(drop_action(rect, egui::pos2(10.0, 19.0), false), Drop::After);
    assert_eq!(drop_action(rect, egui::pos2(10.0, 10.0), false), Drop::Into(Attach::Group));
    assert!(matches!(drop_action(rect, egui::pos2(10.0, 10.0), true), Drop::Into(Attach::Ride { turn_with_parent: true, .. })));
}

#[test]
fn applying_drops_regroups_reorders_and_refuses_loops() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let f = scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
    apply_drop(&mut scene, ThingId(2), f, Drop::Into(Attach::Group)).unwrap();
    assert_eq!(scene.things[&ThingId(2)].parent, Some(f));
    apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Before).unwrap();
    assert_eq!(scene.order, vec![ThingId::ROOT, ThingId(2), ThingId(1), f]);
    apply_drop(&mut scene, f, ThingId(2), Drop::After).unwrap();
    assert_eq!(scene.order, vec![ThingId::ROOT, ThingId(2), f, ThingId(1)]);
    scene.things.get_mut(&ThingId(2)).unwrap().transform.position = Vec2::new(50.0, 0.0);
    apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Into(Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None })).unwrap();
    assert!(matches!(scene.things[&ThingId(2)].attach, Attach::Ride { offset, .. } if offset == Vec2::new(50.0, 0.0)));
    assert!(matches!(apply_drop(&mut scene, ThingId(1), ThingId(2), Drop::Into(Attach::Group)), Err(EditError::WouldLoop)));
    assert!(matches!(apply_drop(&mut scene, f, ThingId::ROOT, Drop::Before), Err(EditError::Root)));
}

#[test]
fn ctrl_click_extends_the_selection_and_delete_removes_it_all() {
    let mut h = harness();
    h.get_by_label("Arena").click();
    h.run();
    h.event(egui::Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }); // no-op keeps the harness honest
    click_with(&mut h, "Ball", Modifiers::COMMAND);
    assert_eq!(h.state().selection.all(&h.state().scene), vec![ThingId(1), ThingId(2)]);
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(h.state().scene.things.len(), 1);
}

#[test]
fn a_collapsed_folder_hides_its_children_and_a_rider_shows_a_hook() {
    let mut h = harness();
    { let s = h.state_mut(); let f = s.scene.add_thing(&s.kit.registry, "folder", ThingId::ROOT).unwrap(); s.scene.set_parent(ThingId(2), f, Attach::Group).unwrap(); s.resolved = resolve(&s.scene, &s.kit.registry); }
    h.run();
    assert!(h.query_by_label("Ball").is_some());
    h.get_by_label("Collapse Folder").click();
    h.run();
    assert!(h.query_by_label("Ball").is_none());
    h.get_by_label("Expand Folder").click();
    h.run();
    assert!(h.query_by_label("Ball").is_some());
}

#[test]
fn dragging_a_row_onto_a_folder_groups_it_there() {
    let mut h = harness();
    { let s = h.state_mut(); s.scene.add_thing(&s.kit.registry, "folder", ThingId::ROOT).unwrap(); s.resolved = resolve(&s.scene, &s.kit.registry); }
    h.run();
    let from = h.get_by_label("Ball").rect().center();
    let to = h.get_by_label("Folder").rect().center();
    drag(&mut h, from, to); // press, several moves, release, as the inspector's drag_slider does
    assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(ThingId(3)));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement**. `click_with` and `drag` are test helpers in `tree.rs`'s test module (pointer events with modifiers, as `inspector.rs`'s `drag_slider` does). If `dragging_a_row_onto_a_folder_groups_it_there` cannot be made to pass under kittest because egui's drag threshold needs more frames, add intermediate `PointerMoved` steps; the pure `drop_action`/`apply_drop` tests stay the contract.
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Studio: multi-select, folders and drag-and-drop in the tree"`

---

### Task 12: Studio: overrides, write policy, Target picker, parent and attach, multi-edit

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs`, `crates/bpe-studio/src/widgets.rs`, `crates/bpe-studio/src/app.rs` (call site)

**Interfaces:**
- `widgets.rs`:
  - `pub struct TargetChoices { pub things: Vec<(ThingId, String)>, pub tags: Vec<String>, pub kinds: Vec<(String, String)> }` with `pub fn from_scene(scene: &Scene, reg: &Registry) -> TargetChoices` (things in tree order, tags sorted, kinds `(id, name)` in registry order without `"scene"`).
  - `pub fn target_words(t: &Target, choices: &TargetChoices) -> String`: "Anything", "Itself", "Thing: <name>" (or "Thing: missing (id N)"), "Tag: <tag>", "Kind: <name>".
  - `pub struct GridCx<'a> { pub choices: &'a TargetChoices, pub overridden: &'a dyn Fn(&str) -> bool, pub inherited: &'a dyn Fn(&str) -> Option<Value> }` and `pub struct GridOutcome { pub changes: Vec<(&'static str, Value)>, pub resets: Vec<&'static str>, pub pushes: Vec<&'static str> }`.
  - `pub fn settings_grid_marked(ui, id, defs, depth, get, cx: &GridCx) -> GridOutcome`: like `settings_grid` plus an orange dot (`theme::OVERRIDE`, painted, named "Changed here") before the label of an overridden setting, the Alt/double-click reset falling back to `cx.inherited(key)`, and a right-click menu on the label with "Reset to module" and "Push to module" for overridden settings. `settings_grid` stays as a thin wrapper with no marks and no target choices.
  - The `ValueType::Target` widget: a `menu_button` labelled by the row showing `target_words`, with entries Anything, Itself, a "Thing ▸" submenu, a "Tag ▸" submenu (disabled with "No tags yet" when empty) and a "Kind ▸" submenu; picking one marks the response changed.
- `inspector.rs`:
  - `pub fn show(ui, inspector, scene, kit, resolved, selection: &Selection) -> bool`.
  - Header rows for non-root things: "Parent" (menu button naming the parent; entries: Scene defaults and every thing that is not the thing or its descendant; picking → `set_parent(id, p, current attach)`), "Attach" (two selectable labels "Grouped" / "Rides on it"; switching to Ride sets `offset = position − parent position`, `turn_with_parent: true`), and when riding: "Offset" x/y DragValues, "Turns with it" checkbox, "Item" (a DragValue from −1 meaning the whole thing; shown only when the parent's kind is `"balls"`). The "Position" row is replaced by "Offset" while riding.
  - A banner "Editing N things" (muted) when `selection.extra` is not empty.
  - `pub(crate) enum Write { Module(ModuleId), Override(ThingId, SlotId), Inherited { from: ThingId, module: ModuleId } }` and `pub(crate) fn write_target(scene: &Scene, resolved: &Resolved, id: ThingId, slot: SlotId) -> Option<Write>` implementing the write policy from Global Constraints (`Linked` sole user → `Module`; `Linked` shared → `Override`; `Inherit` with a resolved slot → `Inherited`; `Off` or unresolved → `None`).
  - Slot card: `Write::Module` → plain grid writing `Owner::Module`; `Override` → marked grid writing `Owner::Thing(id) + slot`, resets → `reset_override`, pushes → `push_to_module`-style single key (`set_at` on the module then `reset_override`); `Inherited` → grid disabled (`ui.add_enabled_ui(false, ..)`) plus a "Change here" button (links the module to this thing). The picker menu gains, after the module list: "Make unique" (Linked), "Push all to module" and "Reset all" (Linked with overrides), "Open in Library" (sets `inspector.open_library = Some(module)` which `app.rs` reads).
  - Multi-edit: every `Edit::Set` for a kind setting is applied to each selected thing of the same kind; every slot setting to each selected thing whose resolved slot has the same module kind, through its own `write_target` (an `Inherited` one is linked first, so the edit becomes its override).
  - `problem_words` unchanged.

- [ ] **Step 1: Write the failing tests** in `widgets.rs` and `inspector.rs`:

```rust
#[test]
fn target_words_name_things_tags_and_kinds() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().tags.insert("walls".into());
    let c = TargetChoices::from_scene(&scene, &kit.registry);
    assert_eq!(target_words(&Target::Anything, &c), "Anything");
    assert_eq!(target_words(&Target::Me, &c), "Itself");
    assert_eq!(target_words(&Target::Thing(ThingId(2)), &c), "Thing: Ball");
    assert_eq!(target_words(&Target::Thing(ThingId(9)), &c), "Thing: missing (id 9)");
    assert_eq!(target_words(&Target::Tag("walls".into()), &c), "Tag: walls");
    assert_eq!(target_words(&Target::Kind("arena".into()), &c), "Kind: Arena");
    assert_eq!(c.tags, vec!["walls".to_owned()]);
    assert!(c.kinds.iter().all(|(id, _)| id != "scene"));
}
```

```rust
#[test]
fn a_sole_user_edits_its_module_and_a_shared_module_gets_an_override() {
    let mut h = harness(Some(ThingId(2)));
    // "Ball look" is the ball's alone: the Glow slider writes into the module.
    let rect = slider_rect(&h, "Glow");
    drag_slider(&mut h, rect, 0.9);
    { let s = h.state(); let SlotLink::Linked { module, overrides } = &s.scene.things[&ThingId(2)].slots[&SlotId::Look] else { panic!() };
      assert!(overrides.is_empty()); assert!(matches!(s.scene.library.modules[module].params.get("glow"), Some(Value::Number(g)) if *g > 1.0)); }
    // Share it with the arena: now the ball's edits are its own, with a dot.
    { let s = h.state_mut(); let m = match &s.scene.things[&ThingId(2)].slots[&SlotId::Look] { SlotLink::Linked { module, .. } => *module, _ => unreachable!() };
      s.scene.link(&s.kit.registry, ThingId(1), SlotId::Look, m).unwrap(); s.resolved = resolve(&s.scene, &s.kit.registry); }
    h.run();
    assert!(h.query_by_label_contains("shared ×2").is_some());
    let rect = slider_rect(&h, "Glow");
    drag_slider(&mut h, rect, 0.1);
    let s = h.state();
    let SlotLink::Linked { overrides, .. } = &s.scene.things[&ThingId(2)].slots[&SlotId::Look] else { panic!() };
    assert!(matches!(overrides.get("glow"), Some(Value::Number(g)) if *g < 0.5), "{overrides:?}");
    assert!(h.query_by_label("Changed here").is_some());
}

#[test]
fn an_inherited_card_is_read_only_until_changed_here() {
    let mut h = harness(Some(ThingId(2)));
    // Motion is inherited from Scene defaults.
    assert!(h.get_by_role_and_label(Role::Slider, "Air drag").accesskit_node().is_disabled());
    h.get_by_label("Change here").click(); // the first inherited card is Motion
    h.run();
    let s = h.state();
    assert!(matches!(s.scene.things[&ThingId(2)].slots[&SlotId::Motion], SlotLink::Linked { .. }));
    assert_eq!(s.scene.users_of(bpe_core::ModuleId(1)), vec![ThingId::ROOT, ThingId(2)]);
}

#[test]
fn make_unique_and_reset_all_are_in_the_picker() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Ball look").click();
    h.run();
    h.get_by_label("Make unique").click();
    h.run();
    let s = h.state();
    assert_eq!(s.scene.library.modules.len(), 6);
    assert!(s.scene.library.modules.values().any(|m| m.name == "Ball look (Ball)"));
}

#[test]
fn a_mixed_selection_only_edits_things_that_have_the_setting() {
    let mut h = harness(Some(ThingId(2)));
    { let s = h.state_mut(); let c = s.scene.add_thing(&s.kit.registry, "balls", ThingId::ROOT).unwrap(); s.selection.extra.extend([ThingId(1), c]); s.resolved = resolve(&s.scene, &s.kit.registry); }
    h.run();
    assert!(h.query_by_label("Editing 3 things").is_some());
    let rect = slider_rect(&h, "Size");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let big = |id| matches!(s.scene.things[&id].params.get("radius"), Some(Value::Number(r)) if *r > 100.0);
    assert!(big(ThingId(2)) && big(ThingId(3)));
    assert!(s.scene.things[&ThingId(1)].params.get("radius").is_none(), "the arena's Radius is a different setting and stays");
}

#[test]
fn the_parent_and_attach_rows_reparent_and_ride() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Scene defaults").click(); // the Parent picker shows the current parent
    h.run();
    h.get_by_label("Arena").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(ThingId(1)));
    h.get_by_label("Rides on it").click();
    h.run();
    let s = h.state();
    assert!(matches!(s.scene.things[&ThingId(2)].attach, Attach::Ride { offset, turn_with_parent: true, .. } if offset == Vec2::new(0.0, -200.0)));
    assert!(h.query_by_label("Offset").is_some() && h.query_by_label("Position").is_none());
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS (the stage 2 inspector tests still pass: `a_new_module_copies_the_current_one` clicks the inherited Look card's picker, which still exists).
- [ ] **Step 5: Commit** `git commit -m "Studio: per-thing overrides, Target picker, parent and attach rows, multi-edit"`

---

### Task 13: Studio: the Library window and preview handles for riders, obstacles and folders

**Files:**
- Create: `crates/bpe-studio/src/library.rs`
- Modify: `crates/bpe-studio/src/app.rs`, `crates/bpe-studio/src/main.rs`, `crates/bpe-studio/src/preview.rs`, `crates/bpe-studio/src/tree.rs` (ModuleId drop)

**Interfaces:**
- `library.rs`: `#[derive(Default)] pub struct LibraryWindow { pub open: bool, pub selected: Option<ModuleId>, pub depth: BTreeMap<ModuleId, Depth>, renaming: Option<ModuleId>, rename_text: String }` and `pub fn show(ctx: &egui::Context, lib: &mut LibraryWindow, scene: &mut Scene, kit: &Kit, resolved: &Resolved) -> LibraryOutcome` with `pub struct LibraryOutcome { pub edited: bool, pub select: Option<Vec<ThingId>> }`. An `egui::Window` titled "Library", default 520×640, closable, with a left list and a right pane:
  - List grouped by `SlotId::ALL` (a muted heading per slot that has modules or module kinds; "No modules yet" under an empty heading); each module row: a drag source (`dnd_drag_source` with payload `ModuleId`), the name as a selectable label (double-click renames in place like the tree), "×N" users muted, a red "missing kind" note when `reg.module(kind)` is `None`; a "+ New <slot>" menu per heading listing the slot's module kinds (`add_module(kind, "<Kind name>")`).
  - Right pane for the selected module: name, kind name and help, users ("Used by: A, B" or "Not used yet"), buttons "Select users" (outcome `select`), "Duplicate" (`duplicate_module`, selects the copy), "Delete" (disabled while in use with hover text `EditError::InUse(users).to_string()` + " · Select users"), then a Simple/More switch and `settings_grid` writing `Owner::Module(m)`.
- `app.rs`: `Panels` gains `pub library: LibraryWindow`; the toolbar gains a "Library" toggle button (before "Export video…", hover "Shared modules: rename, duplicate, delete, drag onto a thing to link"); `inspector.open_library` opens the window on that module; `LibraryOutcome.select` replaces the selection; edits go through the normal `after_edit` path.
- `tree.rs`: a `ModuleId` payload hovering a row highlights it; releasing links (`scene.link`), refusals become `message` ("Balls has no Shader slot" through `EditError::SlotNotSupported`'s Display).
- `preview.rs`:
  - `hit_test` adds `Handle::Body(id)` for obstacles (within `solids.reach.max(12 px) ` of `poses[id].position`) after balls and before arena edges, and for selected folders (within 12 screen px of their pose).
  - `apply_drag` for `Handle::Body` on a riding thing writes `attach.offset` so that the thing lands under the pointer: `offset = world − parent_pose.position`, rotated back by the parent's angle when `turn_with_parent` (the parent pose comes from `studio.playback.world().poses` or the parent's transform); `apply_drag` therefore takes `&World` as a new parameter.
  - `draw_handles`: obstacles get a small ring handle at their pose like balls; selected folders and other things without bodies or walls draw a dashed circle (radius 24 screen px) at their position with the folder's name.
  - `Rounded` outlines are drawn and grabbed like boxes (Task 5 added the arms).

- [ ] **Step 1: Write the failing tests** in `library.rs` and `preview.rs`:

```rust
#[test]
fn the_library_lists_modules_by_slot_with_their_users() {
    let mut h = harness(); // LibraryWindow open over the new scene, 520x640, same State pattern as the tree
    assert!(h.query_by_label_contains("Plain look").is_some());
    assert!(h.query_by_label_contains("Ball look").is_some());
    h.get_by_label_contains("Ball look").click();
    h.run();
    assert!(h.query_by_label_contains("Used by: Ball").is_some());
    h.get_by_label("Duplicate").click();
    h.run();
    assert!(h.state().scene.library.modules.values().any(|m| m.name == "Ball look copy"));
}

#[test]
fn delete_is_refused_while_in_use_and_names_the_users() {
    let mut h = harness();
    h.get_by_label_contains("Ball look").click();
    h.run();
    assert!(h.get_by_label("Delete").accesskit_node().is_disabled());
    { let s = h.state_mut(); s.scene.set_slot_inherit(ThingId(2), SlotId::Look); s.resolved = resolve(&s.scene, &s.kit.registry); }
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    assert_eq!(h.state().scene.library.modules.len(), 4);
}

#[test]
fn new_module_buttons_add_one_per_slot_kind() {
    let mut h = harness();
    h.get_by_label("+ New Look").click();
    h.run();
    h.get_by_label("Classic").click();
    h.run();
    assert_eq!(h.state().scene.library.modules.len(), 6);
}
```

```rust
#[test]
fn dropping_a_module_on_a_thing_without_the_slot_is_refused() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let f = scene.add_thing(&kit.registry, "arena", ThingId::ROOT).unwrap();
    let motion = scene.library.modules.values().find(|m| m.name == "Default motion").unwrap().id;
    let err = crate::tree::link_dropped(&mut scene, &kit.registry, f, motion).unwrap_err();
    assert_eq!(err.to_string(), "This kind of thing has no Motion slot");
    let look = scene.library.modules.values().find(|m| m.name == "Ball look").unwrap().id;
    crate::tree::link_dropped(&mut scene, &kit.registry, f, look).unwrap();
    assert!(matches!(scene.things[&f].slots[&SlotId::Look], SlotLink::Linked { module, .. } if module == look));
}
```

```rust
#[test]
fn dragging_a_rider_moves_its_offset_and_obstacles_are_grabbable() {
    let mut studio = Studio::headless();
    let before = studio.scene.clone();
    let peg = studio.scene.add_thing(&studio.kit.registry, "obstacle", ThingId::ROOT).unwrap();
    studio.scene.things.get_mut(&peg).unwrap().transform.position = Vec2::new(100.0, 100.0);
    studio.scene.set_parent(ThingId(2), ThingId(1), Attach::Ride { offset: Vec2::new(0.0, -200.0), turn_with_parent: true, item: None }).unwrap();
    studio.after_edit(before);
    let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
    assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(100.0, 100.0))), Some(Handle::Body(peg)));
    let world = studio.playback.world().clone();
    apply_drag(&mut studio.scene, &world, Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO }, Vec2::new(30.0, -100.0));
    assert!(matches!(studio.scene.things[&ThingId(2)].attach, Attach::Ride { offset, .. } if offset == Vec2::new(30.0, -100.0)));
    assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(0.0, -200.0), "the own position is left alone");
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement**, with `pub(crate) fn link_dropped(scene, reg, onto: ThingId, module: ModuleId) -> Result<(), EditError>` in `tree.rs` (looks the module's slot up, then `link`).
- [ ] **Step 4: Run** `cargo test --workspace` (background) and `cargo clippy --workspace -- -D warnings`. Expected: PASS, clean.
- [ ] **Step 5: Commit** `git commit -m "Studio: the Library window, dragging riders and obstacles in the preview"`

---

### Task 14: Review, screenshots, Windows build, docs, PR

**Files:**
- Modify: `README.md` (a short "Stage 3" paragraph under the Ball Studio Next section: obstacles, folders, riding, ball hits, Library, overrides)
- Deliver: `/mnt/project-files/ballphysics/modular-revamp/stage3/` with `ball-studio-next.exe`, `bpe-render-next.exe`, the five new `.ron` scenes, `stage3-*.png` screenshots (tree with a folder and riders, inspector with override dots and the Target picker, Library window, Peg board playing, Ball pit playing), one exported mp4 (Peg board, full size), and `REPORT.md`.

- [ ] **Step 1: Fresh review**: dispatch one reviewer subagent over `git diff main...HEAD` with the spec, this plan and the Global Constraints; fix every must-fix and should-fix it finds in one commit, `git commit -m "Fix the review findings: ..."`.
- [ ] **Step 2: Screenshots** under Xvfb with the scratchpad `shot2.sh` pattern from stage 2 (release editor on a scene, `xdotool` clicks by coordinates, `import -window root`).
- [ ] **Step 3: Windows build**: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`; export `Peg board` with `bpe-render-next` on Linux; copy everything to the stage3 folder with `REPORT.md` (what works, gaps left for stages 4-8, rulings, deferred minors).
- [ ] **Step 4: Verify** `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` all pass on the rebased branch (`git fetch origin main && git rebase origin/main`).
- [ ] **Step 5: PR and merge**: push, open the PR (body starts with the required attribution block; lists the behaviour rulings and the golden file additions), merge it, write the `stage3-building-blocks` team memory file, reply to Aedan.

---

## Done when

- `cargo test --workspace` passes with the stage 2 golden lines unchanged and five new ones; clippy and fmt are clean.
- In `ball-studio-next`: an Obstacle can be added (peg, bar, peg grid, polygon, ring), spun and moved on a path; things can be grouped into folders, collapsed, reordered and re-parented by dragging, and set to ride on a parent (Alt-drop, the Attach row or the context menu); balls can hit each other and pop; a Target picker fills every Target setting; a shared module's settings edited on one thing show orange dots with Reset/Push, "Make unique" copies it; the Library window lists, renames, duplicates, deletes and drags modules onto things; Ctrl/Shift-click selects several things and edits apply to all of them.
- `/mnt/project-files/ballphysics/modular-revamp/stage3/` holds the Windows exe, scenes, screenshots, an mp4 and `REPORT.md`; the PR is merged; the memory file is written.

## Deviations noted at review time

- `Wall` gained a `velocity` field after all (a riding arena's travel speed must reach `contact`, or a ball pushed by its trailing wall never registers); the only literals were in this repo and were updated.
- The sole-user write rule has one more clause: a thing that already has overrides keeps writing overrides even when it is the module's only user.
- `EditError::WouldLoop` is worded "A thing can't go inside itself or its own children." in the tree's status line.
- The Library disables Delete for a module in use (the users are named in the details pane) rather than refusing with a message.
- Main moved on (stage 5) before this stage merged, so the branch carries a merge commit instead of a rebase; the stage 2 and stage 5 golden lines are byte-identical to main's.
- The solid sweep now starts where the substep began, which re-blessed three stage 3 golden lines once (Peg board, Spinning bars, Pitch climb).
