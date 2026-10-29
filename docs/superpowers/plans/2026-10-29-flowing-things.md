# Flowing Things (Stage 6) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Emitters, one Liquid thing (CPU below a particle limit, GPU above), Swarms and force Fields become real things in the new editor's scene tree, with their own modules, targets by tag, golden-logged test scenes and a Windows build.

**Architecture:** The runtime gains four small, default-bodied hooks on `ThingSim` (spawn, forces, settle, after_step) so that new kinds take part in the spec's Spawn, Forces, Integrate-then-settle and Events stages without the runtime knowing them; bodies may be *silent* (no Hit events) and the world answers `Target`s by tag and kind. Emitters, Fields and the CPU liquid live entirely in `bpe-kit` as new thing folders plus four module kinds. The GPU liquid and the swarm are ported from `bpe-render/src/gpu` into a `flow` module of `bpe-gfx` that steps a private copy of the world at 120 Hz from data the kit's draw parts put on the `Frame`, never feeding back into the CPU physics. The editor adds the kinds through a kit helper that links each kind's shared default modules, draws dashed gizmos for invisible things and shows a submenu when a slot has several module kinds.

**Tech Stack:** Rust 2024 workspace, glam 0.34, wgpu 30 (WGSL compute + render), egui/eframe 0.37 with `egui_kittest`, ffmpeg for the export check. `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli` for the Windows exe.

**Spec:** `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (kinds table 3.1, tags and targets 3.6, preview 4.5, step stages 5.3, events 5.5, testing 8). Stage 2's report lists what this stage fills in: `/mnt/project-files/ballphysics/modular-revamp/stage2/REPORT.md`.

## Global Constraints

- Old crates (`bpe-scene`, `bpe-sim`, `bpe-render`, `bpe-app`, ...) are untouched; they are read as reference only.
- `bpe-graph` is untouched. `BTreeMap`/`BTreeSet` everywhere, never `HashMap`.
- Changes to shared files (`bpe-runtime` world and plugins, `bpe-gfx` frame and renderer, studio tree/inspector/preview, `bpe-cli`) are additive: new fields, new default-bodied trait methods, new match arms. Stages 3, 4, 5 and 7 edit the same files in parallel; rebase on `main` before merging.
- Every plugin passes `bpe_core::check_registry`: at most `MAX_SIMPLE = 6` Simple settings per kind; keyframe/pulse flags only on Number, Int, Vec2 and Color settings; defaults inside ranges; defaults round-trip through RON.
- The new editor stays `ball-studio-next.exe` beside the old one; `bpe-render-next` exports.
- Determinism: everything iterates in `ThingId` order then item order; randomness comes only from a seeded generator kept in the thing's state; GPU sums use fixed-point integer atomics as today.
- Spec 5.3: GPU liquid and swarm step at 120 Hz from a private copy of the world and never feed back into the CPU physics.
- Editor gotchas from stage 2: side panels need `ui.set_min_width(ui.available_width())`; every control is `.labelled_by(label.id)` for kittest; pickers are `menu_button`s, not `ComboBox`; icons are painted, not glyphs.
- Commit after every task; run `cargo fmt --all` before each commit; `cargo clippy --workspace -- -D warnings` clean at the end.

## Review Focus

1. **An emitter whose target is removed or points at a swarm/GPU liquid**: it must release nothing and log nothing, never panic (Task 4 test `an_emitter_without_a_live_target_releases_nothing`).
2. **A liquid in a scene with no arena**: the lattice must still fill a sensible box and particles must stay finite (Task 6 test `a_liquid_without_a_wall_fills_a_video_sized_box`).
3. **A field over a silent liquid and over balls at once with `Target::Anything`**: both move, only the liquid's Enter/Leave events are skipped (Task 5 test `anything_moves_liquid_too_but_logs_no_liquid_entries`).
4. **Scrubbing backwards past a GPU checkpoint, then forwards again**: the picture must match a straight run bit for bit (Task 9 test `seeking_back_restores_a_checkpoint_bit_for_bit`).
5. **Changing a swarm's count while playing**: the CPU playback must keep running (its fingerprint is unchanged) while the GPU layer rebuilds (Task 11 test `swarm_settings_do_not_change_the_physics_fingerprint`).

---

### Task 1: Runtime hooks for spawning, forces, settling, events and targets

**Files:**
- Modify: `crates/bpe-runtime/src/plugins.rs` (ThingSim)
- Modify: `crates/bpe-runtime/src/bodies.rs` (silent flag)
- Modify: `crates/bpe-runtime/src/world.rs` (tags, matches, step phases, fingerprint)
- Test: inline `#[cfg(test)]` in `world.rs`, using `crates/bpe-runtime/src/testing.rs`

**Interfaces:**
- Consumes: `World`, `Bodies`, `Log`, `Sims`, `Target`, `Thing::tags`.
- Produces (used by every later task):
  ```rust
  // plugins.rs, on ThingSim, all with empty default bodies
  fn spawn(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
  fn forces(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
  fn settle(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
  fn after_step(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
  // bodies.rs
  pub struct Bodies { ..., /// Hits are not logged and not counted. pub silent: bool }
  // world.rs
  pub fn matches(&self, target: &Target, me: ThingId) -> Vec<ThingId>  // things with a sim, id order
  pub fn tags(&self, thing: ThingId) -> &BTreeSet<String>
  pub fn kind(&self, thing: ThingId) -> Option<&str>
  ```
- Step order becomes: `step += 1` → **spawn** hook per kind → clamp speeds / `previous` / substep count → `scripted` → substeps → **after_step** hook per kind. Each substep: **forces** hook per kind → module forces + integrate → **settle** hook per kind → collide/respond (Hit logged only when `!bodies.silent`).

- [ ] **Step 1: Write the failing tests** in `world.rs` tests, with a `TestEmitter` sim (kind `"emitter"`, registered in `testing.rs` next to `TestDots`) whose `spawn` pushes one body into `DOTS` every step and logs a `Spawn` event, whose `forces` adds `Vec2::X * 1000.0 * dt` to every `DOTS` velocity, whose `settle` records that it ran after integrate (position changed since `previous`), and whose `after_step` logs a `Custom("tick")` event:

```rust
#[test]
fn hooks_run_in_spec_order_and_can_add_bodies() {
    let (scene, resolved, sims) = dots_with_emitter();   // dots_in_ring(1) plus one "emitter" thing
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(0.5, &sims, &mut log);
    assert_eq!(w.bodies[&DOTS].len(), 1 + 120);
    let kinds: Vec<_> = log.events().iter().take(3).map(|e| e.kind.clone()).collect();
    assert_eq!(kinds[0], EventKind::Spawn);                      // spawn before anything else in a step
    assert!(log.events().iter().any(|e| e.kind == EventKind::Custom("tick".into())));
    assert!(w.bodies[&DOTS].velocity[0].x > 300.0, "the forces hook pushed the ball");
}

#[test]
fn silent_bodies_log_no_hits() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let mut w = World::build(&scene, &resolved, &sims);
    w.bodies.get_mut(&DOTS).unwrap().silent = true;
    let mut log = Log::default();
    w.advance_to(5.0, &sims, &mut log);
    assert!(log.is_empty());
    assert_eq!(w.bodies[&DOTS].hits[0], 0);
}

#[test]
fn targets_resolve_by_tag_kind_thing_and_me() {
    let (mut scene, _, sims) = dots_in_ring(1);
    scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
    let resolved = resolve(&scene, &reg());
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.matches(&Target::Tag("red".into()), WALL), vec![DOTS]);
    assert_eq!(w.matches(&Target::Tag("blue".into()), WALL), vec![]);
    assert_eq!(w.matches(&Target::Kind("wall".into()), DOTS), vec![WALL]);
    assert_eq!(w.matches(&Target::Thing(DOTS), WALL), vec![DOTS]);
    assert_eq!(w.matches(&Target::Thing(ThingId(99)), WALL), vec![]);
    assert_eq!(w.matches(&Target::Me, WALL), vec![WALL]);
    assert_eq!(w.matches(&Target::Anything, WALL), vec![WALL, DOTS]);  // id order, me included
    assert_eq!(w.kind(DOTS), Some("balls"));
}

#[test]
fn tags_change_the_fingerprint() { /* add a tag to DOTS, assert fingerprint differs */ }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p bpe-runtime hooks_run silent_bodies targets_resolve tags_change`
Expected: compile errors (`spawn` not a member, `silent` missing, `matches` missing).

- [ ] **Step 3: Implement.** Add the four hooks with empty bodies; add `silent: bool` (default false; `push` leaves it; `memory` unchanged); add `tags: BTreeMap<ThingId, BTreeSet<String>>` to `World`, filled in `build` from `scene.things[id].tags` for every thing with a sim; `matches` walks `self.kinds` in order. Restructure `step`/`substep` as in the Interfaces block: collect `kinds` once per step (already done for scripted) and reuse it for all hooks; wrap the existing destructured module block in its own scope so the hooks can take `&mut World`. In `substep`, skip the Hit push and `hits += 1` when `bodies.silent`. In `fingerprint`, feed every tag of the thing (sorted, from the `BTreeSet`) after the attach bytes.

- [ ] **Step 4: Run the whole runtime suite and the golden test**

Run: `cargo test -p bpe-runtime && cargo test -p bpe-kit --test golden`
Expected: all PASS; goldens unchanged (the restructuring must not move a single hit).

- [ ] **Step 5: Commit** `feat(runtime): spawn, forces, settle and after-step hooks, silent bodies, targets`

---

### Task 2: A seeded random generator for the kit

**Files:**
- Create: `crates/bpe-kit/src/rng.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub mod rng;`)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub struct Rng(u64);
  impl Rng {
      pub fn new(seed: u32, salt: u32) -> Rng;   // splitmix64 of (seed << 32 | salt)
      pub fn next_u32(&mut self) -> u32;
      pub fn unit(&mut self) -> f32;              // [0, 1)
      pub fn range(&mut self, lo: f32, hi: f32) -> f32;
  }
  ```

- [ ] **Step 1: Write the failing test**: `the_same_seed_gives_the_same_stream` (two `Rng::new(7, 1)` give equal first 100 `unit()`s, all in `[0,1)`, and `Rng::new(7, 2)` differs), `range_stays_inside` (1000 draws of `range(-3.0, 5.0)` within bounds).
- [ ] **Step 2: Run** `cargo test -p bpe-kit rng` → FAIL (module missing).
- [ ] **Step 3: Implement** splitmix64 (`z = (s += 0x9e3779b97f4a7c15); z = (z ^ z>>30) * 0xbf58476d1ce4e5b9; z = (z ^ z>>27) * 0x94d049bb133111eb; z ^ z>>31`), `unit = (next_u32() >> 8) as f32 / (1 << 24) as f32`.
- [ ] **Step 4: Run** → PASS. **Step 5: Commit** `feat(kit): seeded random generator`

---

### Task 3: Four module kinds for liquids and swarms

**Files:**
- Create: `crates/bpe-kit/src/modules/look_liquid/mod.rs` (`LiquidLookKind`, `LiquidLook`)
- Create: `crates/bpe-kit/src/modules/motion_liquid/{mod.rs,sim.rs}` (`FlowingKind`, `Flowing`, `FlowingSim`)
- Create: `crates/bpe-kit/src/modules/look_swarm/mod.rs` (`SwarmLookKind`, `SwarmLook`, `SwarmColors` choice)
- Create: `crates/bpe-kit/src/modules/motion_flock/mod.rs` (`FlockingKind`, `Flocking`)
- Modify: `crates/bpe-kit/src/modules/mod.rs`, `crates/bpe-kit/src/lib.rs` (registry, sims, re-exports)

**Interfaces:**
- Produces settings structs (all `#[derive(Clone, Debug, PartialEq, Settings)]`, defaults from the old `bpe-scene/src/gpu.rs`):
  - `look.liquid` "Liquid surface" (slot Look): `color: Color` (#2e8cff, simple, keyframe, random), `clarity: f32` 0..1 = 0.45 (simple, keyframe), `refraction: f32` 0..1 = 0.5 (simple, keyframe), `highlight: f32` 0..2 = 0.8 (simple, keyframe), `smoothing: f32` 0..20 px = 6 (More, keyframe), `glow: f32` 0..2 = 0.1 (More, keyframe).
  - `motion.liquid` "Flowing" (slot Motion): `gravity_scale: f32` -5..5 = 1.0 (simple, keyframe, pulse), `viscosity: f32` 0..1 = 0.2 (simple, keyframe), `stiffness: f32` 0..1 = 0.5 (simple, keyframe), `ball_push: f32` 0..3 = 1.0 (simple, keyframe). `FlowingSim: ModuleSim` whose `forces` adds `gravity * gravity_scale * dt` to alive bodies (no drag; viscosity is the liquid's job).
  - `look.swarm` "Glowing dots" (slot Look): `size: f32` 0.5..8 px = 1.5 (simple, keyframe), `brightness: f32` 0..2 = 0.25 (simple, keyframe, pulse), `colors_by: SwarmColors {Speed, NearestBall, Gradient}` = Speed (simple), `colors: Vec<Color>` = [#2040ff, #2ed8e6, #fff2c0] (simple, random).
  - `motion.flock` "Flocking" (slot Motion): simple: `top_speed` 20..2000 = 260, `flow` 0..3 = 1.0, `flock` 0..2 = 0.4, `attraction` 0..3 = 0.6, `separation` 0..2 = 0.3, `gravity_scale` -5..5 = 0.0 (keyframe, pulse on `flow`, `attraction`, `gravity_scale`); More: `drag` 0..5 = 0.6, `swirl_size` 20..1000 px = 180 (old flow_scale), `swirl_speed` 0..1 = 0.05, `reach` 10..2000 px = 220, `avoid_walls` 0..3 = 1.0. No `ModuleSim` (GPU only).

- [ ] **Step 1: Write the failing tests**: in `lib.rs` tests extend `every_kit_plugin_passes_the_contract` (already asserts the registry is clean) and add `the_new_module_kinds_are_registered_on_their_slots`: `registry().modules_for(SlotId::Look)` ids are `["look.classic", "look.liquid", "look.swarm"]` and `modules_for(SlotId::Motion)` are `["motion.basic", "motion.flock", "motion.liquid"]`. In `motion_liquid/sim.rs` test `flowing_pulls_with_scaled_gravity_and_no_drag`: one body, gravity (0, 100), scale 2, dt 0.5 → velocity (0, 100).
- [ ] **Step 2: Run** `cargo test -p bpe-kit new_module_kinds flowing_pulls` → FAIL.
- [ ] **Step 3: Implement** the four kinds following `modules/look/mod.rs` and `modules/motion/{mod,sim}.rs` as templates; register in `registry()` and `FlowingSim` in `sims()`.
- [ ] **Step 4: Run** `cargo test -p bpe-kit` → PASS (contract included).
- [ ] **Step 5: Commit** `feat(kit): liquid surface, flowing, glowing dots and flocking modules`

---

### Task 4: The Emitter thing

**Files:**
- Create: `crates/bpe-kit/src/things/emitter/{mod.rs,sim.rs}`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs` (registry, sims, re-exports)
- Modify: `crates/bpe-kit/src/things/balls/sim.rs` (`pub fn item_radius(world: &World, thing: ThingId) -> Option<f32>` lives in `emitter/sim.rs` and reads `BallsState`; later Task 6 adds the liquid arm)

**Interfaces:**
- Produces `EmitterKind` id `"emitter"`, name "Emitter", icon `"emitter"`, slots `[Rules]`, default slots `[]`; `EmitterSettings`:
  - Simple: `target: Target` = Anything, `rate: f32` 0..2000 per s = 20 (keyframe, random), `speed: f32` 0..3000 px/s = 500 (keyframe, random), `direction: f32` -360..360 ° = -90 (up; keyframe, random), `spread: f32` 0..360 ° = 20 (keyframe, random).
  - More: `burst: u32` 0..1000 = 0 (released all at once at `start`), `start: f32` 0..600 s = 0, `stop: f32` 0..600 s = 0 (0 = never), `limit: u32` 1..100000 = 2000, `width: f32` 0..2000 px = 0 (released along a line this wide, across the direction), `size_scale: f32` 0.1..5 = 1.0, `on: Trigger {Time, Hit}` = Time, `listen: Target` = Anything (whose hits release one item each when `on` is Hit), `seed: u32` 0..1000000 = 1.
- `EmitterState { settings, rng: Rng, owed: f32, released: u32, next_target: usize, burst_done: bool }`.
- `EmitterSim::spawn`: when `time` (of this step) is in `[start, stop or ∞)`: targets = `world.matches(&target, me)` filtered to things that have `bodies`; none → return. Time mode: `owed += rate * dt` (plus `burst` once at the first step at or after `start`); while `owed >= 1` and `released < limit`, release one into `targets[next_target % len]`, `next_target += 1`. Hit mode: one item per `Hit` event in `log.since(world.step - 1)` whose `a.thing` is in `world.matches(&listen, me)`. Releasing: position = thing position + across × `rng.range(-width/2, width/2)`, velocity = `speed` × dir(`direction` + `rng.range(-spread/2, spread/2)`), radius = `item_radius(world, target).unwrap_or(12.0) * size_scale`; `bodies.push`; log `Spawn` with `a = Party{target, row}`, `b = Some(Party{me, 0})`, `point` = position, `normal` = velocity direction, `speed` = speed.
- `affects_physics`: true for everything.

- [ ] **Step 1: Write the failing tests** (`emitter/sim.rs`, using a new `testing::scene_with(kind, name, position, f)` helper that adds a thing of `kind` to `new_scene`):

```rust
#[test]
fn an_emitter_releases_at_its_rate_into_its_target_and_logs_spawns() {
    // emitter at (0, 300) aiming up, target Thing(BALLS), rate 10, 2 s
    assert_eq!(w.bodies[&BALLS].len(), 1 + 20);
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Spawn).count(), 20);
    let first = log.events().iter().find(|e| e.kind == EventKind::Spawn).unwrap();
    assert_eq!(first.a, Party { thing: BALLS, item: 1 });
    assert_eq!(first.b, Some(Party { thing: EMITTER, item: 0 }));
    assert!(first.normal.y < -0.9, "aims up");
    assert_eq!(w.bodies[&BALLS].radius[1], 24.0, "takes the target's size");
}
#[test] fn a_burst_comes_all_at_once_and_the_limit_holds() { /* burst 30, rate 0, limit 25 → 25 spawned at step 1 */ }
#[test] fn hit_mode_releases_one_per_listened_hit() { /* on=Hit, listen=Thing(BALLS); spawns == hits of BALLS before the last step */ }
#[test] fn an_emitter_without_a_live_target_releases_nothing() { /* target Thing(ThingId(99)) and target Tag("none") → no bodies added, log empty of Spawn, no panic */ }
#[test] fn two_runs_of_a_spraying_emitter_match() { /* spread 90, width 200: logs equal */ }
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit emitter` → FAIL (kind unknown).
- [ ] **Step 3: Implement** `mod.rs` (settings + kind), `sim.rs` (state, spawn as above, `item_radius`), register.
- [ ] **Step 4: Run** `cargo test -p bpe-kit` → PASS.
- [ ] **Step 5: Commit** `feat(kit): the Emitter thing releases balls into its target`

---

### Task 5: The Field thing

**Files:**
- Create: `crates/bpe-kit/src/things/field/{mod.rs,sim.rs}`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs`

**Interfaces:**
- Produces `FieldKind` id `"field"`, name "Field", icon `"field"`, slots `[Rules]`, default slots `[]`; `FieldSettings`:
  - Simple: `kind: FieldKind {Wind, Well, Repel, Vortex}` = Wind (random), `strength: f32` 0..5000 px/s² = 600 (keyframe, random, pulse), `radius: f32` 10..3000 px = 300 (keyframe, random), `shape: FieldShape {Circle, Box, Everywhere}` = Circle, `targets: Target` = Anything, `direction: f32` -360..360 ° = 0 (wind blows this way; keyframe, random).
  - More: `width: f32` 10..4000 = 600, `height: f32` 10..4000 = 400 (Box), `falloff: Falloff {Flat, Linear, Smooth}` = Smooth, `pull: f32` -1..1 = 0.3 (Vortex: fraction of `strength` pulling to the centre), `log_events: bool` = true.
- `pub fn weight(settings, center, rotation, p: Vec2) -> f32` (0 outside; 1 inside for Flat; `1 - d/radius` Linear; smoothstep for Smooth; box uses the larger of the two normalised axis distances; Everywhere → 1).
- `FieldSim::forces(me, world, dt)`: for each target with bodies, each alive body: `v += accel(settings, r) * weight * dt` where Wind = dir(direction) × strength; Well = −r̂ × strength; Repel = +r̂ × strength; Vortex = r̂.perp() × strength − r̂ × strength × pull.
- `FieldSim::after_step`: when `log_events`, for each non-silent target body compare `weight > 0` now against the stored `inside: BTreeMap<ThingId, Vec<bool>>` (grown with `false` for new rows); log `Enter`/`Leave` with `a = body`, `b = Some(Party{me, 0})`, `point` = body position, `normal`/`speed` zero.
- Fields have no draw part (invisible; Task 12 adds gizmos).

- [ ] **Step 1: Write the failing tests** (`field/sim.rs`):

```rust
#[test] fn wind_pushes_only_its_tagged_targets() { /* two Balls things, one tagged "light"; wind Tag("light") strength 1000, Everywhere, 1 s, zero gravity → tagged vx grows by ~1000, other unchanged */ }
#[test] fn a_well_pulls_in_and_a_repeller_pushes_out() { /* ball at (200,0), field at origin radius 500 Flat; Well → vx < 0; Repel → vx > 0 */ }
#[test] fn a_vortex_swirls_clockwise_with_a_little_pull() { /* ball at (200,0): vy > 0 (y down = clockwise), vx < 0 */ }
#[test] fn entering_and_leaving_are_logged_once_each() { /* ball flying through a circle: exactly one Enter then one Leave with a = ball, b = field */ }
#[test] fn anything_moves_liquid_too_but_logs_no_liquid_entries() { /* Balls + a silent Bodies thing (set silent in test via world) under Anything → both change velocity; Enter events only for the balls */ }
#[test] fn falloff_weights_follow_their_shapes() { assert_eq!(weight(flat, ..), 1.0); assert!((weight(linear at half radius) - 0.5).abs() < 1e-5); assert_eq!(weight(.., outside), 0.0); }
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit field` → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(kit): the Field thing pushes its targets and logs enter and leave`

---

### Task 6: The Liquid thing on the CPU

**Files:**
- Create: `crates/bpe-kit/src/things/liquid/{mod.rs,sim.rs,pbf.rs,draw.rs}`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs`, `crates/bpe-kit/src/things/emitter/sim.rs` (`item_radius` liquid arm)
- Reference: `crates/bpe-sim/src/bodies/fluid.rs` (kernels, lambda, clamp, XSPH)

**Interfaces:**
- Produces `LiquidKind` id `"liquid"`, name "Liquid", icon `"liquid"`, slots `[Look, Motion, Bounce, Shader]`, default slots `[Look, Motion, Bounce]`; `LiquidSettings`:
  - Simple: `count: u32` 10..400000 = 2000 (random), `start: LiquidStart {Pool, Block}` = Pool, `fill: f32` 0.05..1 = 0.35 (Pool: fraction of the wall's height filled from the bottom).
  - More: `width: f32` 10..2000 = 300, `height: f32` 10..3000 = 300 (Block, centred on the thing), `pour_seconds: f32` 0..60 = 0 (particles appear over this long from the thing's position; 0 = all at once), `cpu_limit: u32` 0..400000 = 4000 (above this many particles the liquid runs on the graphics card), `seed: u32` = 1.
- `pub fn spacing(settings, area: f32) -> f32` = `sqrt(area / count)`; particle radius = `spacing * 0.5`; smoothing radius `h = spacing * 2.0`.
- `pub fn wall_of(world: &World, position: Vec2) -> Option<(ThingId, Wall)>` = the first wall in id order whose outline `inside(position)`, else the first wall. `pub fn region(world, settings, thing) -> Rect{min,max}`: Pool = the wall's bounding box with `min.y` raised so the box's lower `fill` is kept; no wall → the box `(-540,-960)..(540,960)` (default video size); Block = width×height around the thing.
- `pub fn lattice(region, spacing, keep: impl Fn(Vec2) -> bool, count) -> Vec<Vec2>`: hex rows from the bottom up, `spacing` apart, every other row shifted by `spacing/2`, kept while `keep(p)` (inside the wall with the particle radius as margin, tested with `keep_inside` on a copy), stopping at `count`.
- `LiquidSim::build`: when `count > cpu_limit` store `LiquidState { settings, spacing, gpu: true, .. }` and no bodies (Task 11 draws it on the GPU); else push `count` bodies (`silent = true`), velocity zero; with `pour_seconds > 0` every body starts `alive = false` at the thing position and `spawn` revives `count * dt / pour_seconds` per step (owed accumulator) with a small seeded jitter and velocity `(0, 200)`.
- `pbf.rs`: `pub struct Pbf { h, rest_density, iterations, viscosity, grid cell = h }` with `pub fn relax(&self, bodies: &mut Bodies, dt: f32, walls: &BTreeMap<ThingId, Wall>, pushers: &[(Vec2 center, f32 radius)], ball_push: f32)`: builds a `BTreeMap<(i32,i32), Vec<u32>>` grid of alive rows, then `iterations` times: lambda per particle (poly6 density, spiky gradient, CFM epsilon 100), Δp clamped to `h * 0.125`, apply, push out of each pusher ball by `ball_push` (one way), `keep_inside` every wall; then `v += Δp_total / dt`, then XSPH `v += viscosity * Σ w (v_j − v_i)`. Iterations = `2 + (stiffness * 3).round()`. Rest density from `lattice` numerically at build (sum of poly6 over a 7×7 hex patch).
- `LiquidSim::settle(me, world, dt)`: reads `Flowing` from `world.module_state(me, SlotId::Motion)` when its kind is `motion.liquid` (else defaults), gathers pushers = every alive body of every non-silent thing, calls `relax`.
- `LiquidDraw` (CPU path only in this task): with a `look.liquid` Look, pushes a `LiquidLayer { thing, points: Points::Cpu(alive positions), radius, color, clarity, refraction, highlight, smoothing, glow }` onto `frame.liquids` (Task 7 adds the field); with any other Look, draws each alive particle as a `BallInstance` in the look's colour (glow 0).
- `affects_physics`: `true` for every key.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn a_pool_fills_the_bottom_of_the_arena_with_silent_particles() { /* ring radius 400, count 500, fill 0.3 → 500 bodies, silent, all inside, max y > 200, min y > -100 */ }
#[test] fn the_lattice_is_hexagonal_and_stops_at_count() { /* spacing 10 in a 100×100 box, keep all → first two rows differ by (5, -8.66), len == count */ }
#[test] fn a_liquid_without_a_wall_fills_a_video_sized_box() { /* remove the arena → region is (-540,-960)..(540,960) lower fill; particles finite after 1 s */ }
#[test] fn the_liquid_settles_without_exploding_and_stays_inside() { /* 800 particles, 2 s under gravity 900; every position inside the wall, max speed < 3000, two runs equal */ }
#[test] fn balls_push_liquid_but_liquid_leaves_the_balls_log_alone() { /* One ball dropped into a pool: ball log equals the same scene with the liquid removed; some particle moved sideways */ }
#[test] fn a_pour_appears_over_time() { /* pour_seconds 2, count 400: alive at 1 s between 150 and 250, all alive by 2.5 s */ }
#[test] fn a_big_count_runs_on_the_gpu_and_has_no_bodies() { /* count 5000, cpu_limit 4000 → no bodies entry, state.gpu == true */ }
#[test] fn an_emitter_fills_a_liquid_with_particle_sized_drops() { /* emitter target Thing(LIQUID) with count 50 CPU pool → new rows have radius == spacing*0.5 */ }
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit liquid` → FAIL.
- [ ] **Step 3: Implement** `mod.rs`, `sim.rs`, `pbf.rs`, `draw.rs` (CPU path), the `item_radius` arm, registration.
- [ ] **Step 4: Run** `cargo test -p bpe-kit` → PASS; `cargo test -p bpe-kit --test golden` unchanged.
- [ ] **Step 5: Commit** `feat(kit): the Liquid thing with a CPU position-based solver`

---

### Task 7: Frame layers and the flow driver in bpe-gfx

**Files:**
- Modify: `crates/bpe-gfx/src/frame.rs` (new instance lists), `crates/bpe-gfx/src/lib.rs` (`pub mod flow;`, re-exports, `render_with_flow`)
- Create: `crates/bpe-gfx/src/flow/mod.rs` (`FlowSource`, `FlowLayers`, `FlowStatus`, stepping and checkpoints), `crates/bpe-gfx/src/flow/inputs.rs` (per-step ball inputs), `crates/bpe-gfx/src/flow/sdf.rs` + `crates/bpe-gfx/src/shaders/flow/sdf.wgsl` (wall distance field), `crates/bpe-gfx/src/flow/util.rs` (storage buffers, bind group helpers, hash2)
- Reference: `crates/bpe-render/src/gpu/{mod,sdf,util}.rs`, `crates/bpe-render/src/shaders/gpu/{common,sdf}.wgsl`

**Interfaces:**
- Produces on `Frame` (plain data, keeps `Clone, Debug, PartialEq, Default`):
  ```rust
  pub struct LiquidLayer { pub thing: ThingId, pub points: Points, pub radius: f32, pub color: [f32;4], pub clarity: f32, pub refraction: f32, pub highlight: f32, pub smoothing: f32, pub glow: f32 }
  pub enum Points { Cpu(Vec<[f32; 2]>), Gpu(GpuLiquid) }
  pub struct GpuLiquid { pub count: u32, pub region: [f32; 4] /* min x, min y, max x, max y */, pub pour_seconds: f32, pub pour_from: [f32; 2], pub seed: u32, pub viscosity: f32, pub stiffness: f32, pub ball_push: f32, pub gravity_scale: f32, pub bounciness: f32 }
  pub struct SwarmLayer { pub thing: ThingId, pub count: u32, pub start: SwarmStart, pub center: [f32; 2], pub seed: u32, pub release_seconds: f32, pub look: SwarmLook, pub motion: Flock }
  pub enum SwarmStart { Fill, Centre, Ring }
  pub struct SwarmLook { pub size: f32, pub brightness: f32, pub colors_by: u32 /* 0 speed, 1 nearest ball, 2 gradient */, pub colors: Vec<[f32; 4]> }
  pub struct Flock { pub top_speed, drag, flow, swirl_size, swirl_speed, flock, separation, attraction, reach, avoid_walls, gravity_scale: f32 }
  // Frame gains: pub liquids: Vec<LiquidLayer>, pub swarms: Vec<SwarmLayer>; clear() empties them.
  ```
  ```rust
  pub struct FlowSource<'a> { pub scene: &'a Scene, pub resolved: &'a Resolved, pub sims: &'a Sims, pub key: u64 /* physics fingerprint */, pub budget: Option<Duration> }
  pub struct FlowStatus { pub caught_up: bool }
  impl Renderer {
      /// Like `render`, and steps the GPU liquids and swarms to `frame.time` first.
      pub fn render_with_flow(&mut self, device, queue, frame: &Frame, source: &FlowSource) -> FlowStatus;
  }
  // flow/mod.rs
  pub const SIM_HZ: u32 = 120; pub const BATCH_STEPS: u32 = 64; pub const MAX_BALLS: usize = 64;
  pub const CHECKPOINT_STEPS: u64 = 240; pub const CHECKPOINT_BYTES: usize = 256 << 20;
  pub struct FlowLayers { key: u64, world: World, log: Log, world_checkpoints: Vec<World>, step: u64, layers: BTreeMap<ThingId, Layer>, inputs: Inputs, sdf: Option<Sdf> }
  impl FlowLayers {
      pub fn new() -> Self;
      /// Rebuilds everything when `source.key` or the wall changed; drops layers no longer in the frame.
      pub fn prepare(&mut self, device, queue, frame: &Frame, source: &FlowSource);
      /// Steps to floor(time × SIM_HZ) in batches of BATCH_STEPS, restoring the nearest checkpoint first when going back; stops early past `budget`.
      pub fn step_to(&mut self, device, queue, time: f64, source: &FlowSource) -> FlowStatus;
      pub fn step(&self) -> u64;
  }
  // flow/inputs.rs: per step, the first MAX_BALLS alive bodies of every non-silent thing (position, previous, radius) packed 256-byte aligned for BATCH_STEPS steps, bound with dynamic offsets (STEP_STRIDE = 256 × ceil(MAX_BALLS × 16 / 256)).
  // flow/sdf.rs: pub struct Sdf { texture, view, params: SdfParams{center, half_size, cell} } baked once per Outline (unrotated, unit scale) on the GPU, FIELD_LONG_SIDE 768, MAX_DISTANCE 64 px, 8×8 tiles; per-step uniform carries the live wall's center, angle and scale so shaders query in the wall's frame.
  ```
- The private world steps at the scene's rate (240 Hz): each GPU step advances it `steps_per_second / SIM_HZ` world steps, and the inputs are the bodies after those steps. World checkpoints every `CHECKPOINT_STEPS` GPU steps, dropped by halving like `Playback` when GPU checkpoint bytes pass `CHECKPOINT_BYTES`.
- `Renderer::render` (unchanged signature) draws `frame.liquids` with `Points::Cpu` and skips GPU layers; `render_with_flow` prepares, steps, then draws.

- [ ] **Step 1: Write the failing tests** (`flow/mod.rs` tests, skipped without `headless_device`):

```rust
#[test] fn the_sdf_measures_a_ring() { /* bake Ring radius 300 thickness 4 → read back; sample at centre ≈ -298 (inside negative, clamped at -64), at (400,0) ≈ +64 (clamped), at (299,0) within 2 px of 0 */ }
#[test] fn the_driver_follows_time_and_rebuilds_on_a_new_key() { /* prepare with a frame holding one swarm layer; step_to(1.0) → step() == 120; step_to(0.5) → 60 (restored checkpoint 0 and re-stepped); new key → step() == 0 after prepare */ }
#[test] fn inputs_pack_the_first_balls_of_each_thing() { /* world with 70 balls → 64 packed; stride 256-aligned; second thing's balls after the first's */ }
#[test] fn render_with_flow_on_a_frame_without_layers_matches_render() { /* same pixels */ }
```

- [ ] **Step 2: Run** `cargo test -p bpe-gfx flow` → FAIL.
- [ ] **Step 3: Implement** the data types, the driver (stepping only; layers are trait objects `Layer { fn step(encoder, inputs, sdf, step) ; fn checkpoint(); fn restore(); fn bytes() }` with no concrete layer yet except a `Noop` used by the test), the SDF bake (port `sdf.rs`/`sdf.wgsl`, same constants), the inputs ring buffer, `render_with_flow` calling `prepare` + `step_to` then the existing passes.
- [ ] **Step 4: Run** → PASS; `cargo test -p bpe-gfx` all green.
- [ ] **Step 5: Commit** `feat(gfx): frame layers and the flow driver with a GPU wall distance field`

---

### Task 8: GPU swarm layer

**Files:**
- Create: `crates/bpe-gfx/src/flow/swarm.rs`, `crates/bpe-gfx/src/shaders/flow/swarm.wgsl` (compute), `crates/bpe-gfx/src/shaders/flow/swarm_draw.wgsl`
- Modify: `crates/bpe-gfx/src/flow/mod.rs` (build a `SwarmGpu` per `SwarmLayer`), `crates/bpe-gfx/src/lib.rs` (pass order)
- Reference: `crates/bpe-render/src/gpu/swarm.rs`, `shaders/gpu/{swarm,swarm_draw,common}.wgsl`

**Interfaces:**
- `pub struct SwarmGpu { params: SwarmLayer (minus look), positions: Buffer (vec2 × count), velocities: Buffer, grid counts/ids (CELL = 24 px over the video rect), checkpoints: Vec<(u64 step, Buffer, Buffer)> }` implementing `Layer`; rebuilt when the sim part of its `SwarmLayer` (`count, start, center, seed, release_seconds, motion`) differs from the stored one; the look part is uploaded every frame.
- Per step: clear grid → count → scatter → flock pass: curl-noise flow (`swirl_size`, `swirl_speed` × time), separation/cohesion from the grid, attraction to the nearest input ball within `reach`, wall avoidance from the SDF (`avoid_walls`), gravity × `gravity_scale`, drag, speed clamp to `top_speed`; points released over `release_seconds` from `center` (all at once when 0). Starts: Fill = seeded uniform over the inside of the wall (rejection on the SDF), Centre = a 40 px disc at `center`, Ring = radius 200 around `center`.
- Draw: additive point sprites of `look.size` px (clamped so a point covers ≥ 1 output pixel), brightness `look.brightness`, colour by speed / nearest ball colour (the ball instances' colours in `frame.balls`, nearest by position) / gradient over the point index, drawn into the HDR target after shapes and before liquids and balls.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn a_swarm_stays_inside_the_wall_and_is_deterministic() { /* 20_000 points, ring 400, 1 s: read back positions; all finite, ≥ 99% inside radius 400 + 2; second driver equal bytes */ }
#[test] fn a_swarm_draws_light() { /* render a frame with the swarm, black background → bright pixel count > 500 */ }
#[test] fn changing_only_the_look_keeps_the_layer() { /* step_to(1.0), change brightness, prepare → step() still 120 */ }
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Port** `swarm.rs` and both shaders into the new module, keeping the old constants (CELL 24, fixed-point grid sums). **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(gfx): GPU swarm layer with flocking, flow and additive dots`

---

### Task 9: GPU liquid layer with checkpoints

**Files:**
- Create: `crates/bpe-gfx/src/flow/liquid.rs`, `crates/bpe-gfx/src/shaders/flow/liquid.wgsl`
- Modify: `crates/bpe-gfx/src/flow/mod.rs`
- Reference: `crates/bpe-render/src/gpu/fluid.rs`, `shaders/gpu/fluid.wgsl`

**Interfaces:**
- `pub struct LiquidGpu { params: GpuLiquid, positions, velocities, predicted, lambdas, grid, checkpoints }` implementing `Layer`; rebuilt when its `GpuLiquid` differs. Steps: predict → grid count/scatter (fixed-point) → lambda → delta (clamped `h × 0.125`) → ball push from the inputs (`ball_push`, one way) → SDF wall projection with `bounciness` on the normal velocity → velocity update → XSPH viscosity; spacing from `region` area and `count` as the CPU solver; pour from `pour_from` over `pour_seconds`.
- `pub fn positions_buffer(&self) -> &Buffer` and `pub fn count(&self) -> u32` for the surface pass (Task 10).
- Checkpoint = copies of positions + velocities + an `alive_count` uniform every `CHECKPOINT_STEPS`; `bytes()` reports them; the driver drops every other checkpoint past `CHECKPOINT_BYTES`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn a_gpu_liquid_settles_inside_its_wall() { /* 12_000 particles, box 800×1400, 1 s, gravity (0, 900): all finite, ≥ 99.5% inside, ≤ 1% faster than 3000 px/s */ }
#[test] fn seeking_back_restores_a_checkpoint_bit_for_bit() { /* step_to(3.0) then step_to(1.5) then step_to(3.0): positions bytes equal a straight step_to(3.0) */ }
#[test] fn checkpoints_stay_under_the_budget() { /* 200_000 particles, step_to(20.0): checkpoint bytes ≤ CHECKPOINT_BYTES, and > 1 checkpoint kept */ }
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Port** `fluid.rs` and `fluid.wgsl`. **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(gfx): GPU liquid layer with position-based fluid and checkpoints`

---

### Task 10: The liquid surface

**Files:**
- Create: `crates/bpe-gfx/src/flow/surface.rs`, `crates/bpe-gfx/src/shaders/flow/surface.wgsl`
- Modify: `crates/bpe-gfx/src/lib.rs` (`Targets` gains `behind` HDR copy + half-size `thickness` and `blur` R16Float textures; pass order), `crates/bpe-gfx/src/frame.rs` (nothing new)
- Reference: `crates/bpe-render/src/shaders/gpu/fluid_surface.wgsl`

**Interfaces:**
- `pub struct Surface { splat: RenderPipeline, blur: RenderPipeline, composite: RenderPipeline, cpu_points: GrowableBuffer, layout }` with `pub fn draw(&mut self, device, queue, encoder, targets, globals_group, layer: &LiquidLayer, points: PointsBinding)` where `PointsBinding::Cpu(&[[f32;2]]) | Gpu(&Buffer, count)`.
- Passes per layer: splat additive thickness (quad per point, radius × 2.5, smooth falloff) into `thickness` (half res) → separable blur with `smoothing` px (two passes, `blur` texture) → composite into `hdr` with `premultiplied` blend: alpha = smoothstep(0.35, 0.65, thickness) × (1 − clarity × 0.5), colour = `color` lit by `highlight` × normal·light (normal from the thickness gradient), refraction samples `behind` offset by gradient × refraction × 24 px, `glow` added as light.
- Render order: background → shapes → swarm dots → copy `hdr` → `behind` → every liquid layer → balls → flashes → bloom → composite.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn cpu_points_draw_a_coloured_surface() { /* 500 points in a 200×200 block, blue look, black bg → blue-dominant pixel count > 300, and no bright pixel far from the block */ }
#[test] fn the_surface_sits_behind_the_balls_and_in_front_of_the_wall() { /* a white ball over the pool keeps a white centre pixel; a wall pixel under the pool is tinted */ }
#[test] fn clarity_one_is_see_through() { /* clarity 1.0 refraction 0: pixel at pool centre is darker-than-opaque (alpha ≤ 0.5) */ }
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** surface.rs + surface.wgsl, the extra targets, the pass order, the GPU path for `Points::Gpu` via `LiquidGpu::positions_buffer`. **Step 4: Run** `cargo test -p bpe-gfx` → PASS.
- [ ] **Step 5: Commit** `feat(gfx): liquid surface with refraction, drawn between walls and balls`

---

### Task 11: Swarm thing, GPU liquid in the kit, and what the exporter needs

**Files:**
- Create: `crates/bpe-kit/src/things/swarm/{mod.rs,sim.rs,draw.rs}`
- Modify: `crates/bpe-kit/src/things/liquid/draw.rs` (GPU path), `crates/bpe-kit/src/lib.rs` (registry/sims/draws, `pub fn flow_source<'a>(scene, resolved, sims, budget) -> FlowSource<'a>`), `crates/bpe-cli/src/lib.rs` (`render_with_flow`, loop until `caught_up`)

**Interfaces:**
- `SwarmKind` id `"swarm"`, name "Swarm", icon `"swarm"`, slots `[Look, Motion]`, default slots `[Look, Motion]`; `SwarmSettings`: Simple `count: u32` 100..1000000 = 50000 (random), `start: SwarmStart {Fill, Centre, Ring}` = Fill; More `release_seconds: f32` 0..60 = 0, `seed: u32` = 1.
- `SwarmSim::build` stores `SwarmState { settings }` only, no bodies; `affects_physics(_) = false` (the swarm never touches the CPU physics).
- `SwarmDraw` pushes one `SwarmLayer` from the settings, the thing's position, its `look.swarm` Look (defaults when the Look is another kind) and its `motion.flock` Motion (defaults otherwise).
- `LiquidDraw` GPU path: when `state.gpu`, pushes `LiquidLayer { points: Points::Gpu(GpuLiquid{ region, pour, seed, viscosity, stiffness, ball_push, gravity_scale, bounciness from a `bounce.basic` Bounce or 0 }) }`.
- `bpe_kit::flow_source` builds the `FlowSource` with `key = bpe_runtime::fingerprint(scene, resolved, sims)`.
- Exporter: `renderer.render_with_flow(device, queue, &frame, &source)` where `source.budget = None`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test] fn a_swarm_has_no_bodies_and_draws_one_layer() { /* World has no bodies entry; frame.swarms.len() == 1 with count 50000, colors_by 0 */ }
#[test] fn swarm_settings_do_not_change_the_physics_fingerprint() { /* change count → fingerprint equal; move the ball → differs */ }
#[test] fn a_big_liquid_draws_a_gpu_layer_with_its_bounce() { /* count 5000 → frame.liquids[0].points is Points::Gpu with bounciness 0.0 from "Soft landing" (Task 12) or default */ }
```
Plus in `bpe-cli` tests (needs ffmpeg; skip when missing, as the existing export test does): `exporting_a_scene_with_a_swarm_and_gpu_liquid_makes_a_clip` (1 s, 12 frames at scale 0.25 → file exists, ffprobe duration ≈ 1 s).

- [ ] **Step 2: Run** `cargo test -p bpe-kit swarm gpu_layer && cargo test -p bpe-cli` → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(kit,cli): the Swarm thing, GPU liquids in the kit, and exporting them`

---

### Task 12: Adding things with their default modules, and gizmos

**Files:**
- Create: `crates/bpe-kit/src/gizmo.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub fn add_thing(scene, reg, kind, parent) -> Result<ThingId, EditError>`, `pub use gizmo::{Gizmo, gizmos}`)

**Interfaces:**
```rust
/// Adds a thing and links the kit's shared default modules for its kind, reusing a library module of the
/// same kind and name when the scene already has one: liquid → "Liquid surface" (look.liquid), "Flowing"
/// (motion.liquid), "Soft landing" (bounce.basic with bounciness 0.0); swarm → "Glowing dots" (look.swarm),
/// "Flocking" (motion.flock). Other kinds are plain `scene.add_thing`.
pub fn add_thing(scene: &mut Scene, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError>;

pub enum Gizmo { Dot(Vec2), Circle(Vec2, f32), Rect { center: Vec2, half: Vec2, angle: f32 }, Arrow(Vec2, Vec2), Line(Vec2, Vec2) }
/// Dashed outlines the editor draws for a thing that has nothing visible of its own, or whose region
/// matters: emitter (dot, arrow of length 80 along `direction`, two spread lines), field (circle / rect /
/// nothing for Everywhere, plus an arrow for wind), liquid Block (rect) and Pool (line at the fill level),
/// swarm (dot). Empty for other kinds.
pub fn gizmos(thing: &Thing, resolved: &ResolvedThing, world: &World) -> Vec<Gizmo>;
/// The handle radius used by the preview to grab a thing by its gizmo.
pub const GIZMO_GRAB: f32 = 14.0;
```

- [ ] **Step 1: Write the failing tests**: `adding_a_liquid_links_its_three_shared_modules` (module names and kinds as above; a second liquid shares the same `ModuleId`s), `adding_a_swarm_links_dots_and_flocking`, `adding_balls_is_unchanged` (same as `scene.add_thing`), `an_emitter_gizmo_points_along_its_direction` (arrow end at position + 80 × dir(−90°)), `a_box_field_gizmo_is_a_rect` and `a_pool_gizmo_is_its_fill_line`.
- [ ] **Step 2: Run** `cargo test -p bpe-kit add_thing gizmo` → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(kit): add things with their default modules; gizmos for invisible things`

---

### Task 13: Stage 6 scenes, golden logs and pictures

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (`pub fn stage6(reg) -> Vec<(String, Scene)>`, `pub fn all(reg)` = stage2 ++ stage6; Builder gains `thing(kind, name, position, params) -> ThingId`, `tag(thing, tag)`, `set(thing, key, value)`)
- Modify: `crates/bpe-kit/tests/golden.rs`, `tests/golden.txt`, `tests/golden-debug.txt`, `tests/pictures.rs` (iterate `scenes::all`; pictures use `render_with_flow` with `flow_source(.., None)`), `scenes/next/*.ron` (blessed)

**Interfaces:** the five scenes (names are what the golden file keys on):
1. **Fountain** — ring arena 500, Balls "Drops" radius 9 at (0, 380) with velocity zero, an Emitter at (0, 380) target `Thing(Drops)` rate 25 speed 900 spread 30 limit 400, bouncy 0.9, gradient look.
2. **Wind tunnel** — box 900×1500, gravity (0, 300); two Balls blocks: "Feathers" (tag `light`, radius 6, 6×6) and "Stones" (tag `heavy`, radius 16, 3×3); Field "Breeze" Wind direction 0 strength 900 Everywhere targets `Tag(light)`; Field "Whirl" Vortex radius 350 strength 1200 at (0, 200) targets `Anything`; 20 s.
3. **Small pool** — ring 480, Liquid "Pool" count 1500 fill 0.4 (CPU), one Ball radius 40 dropped from (0, −500) with bounciness 0.6; gravity (0, 900).
4. **Deep pool** — box 900×1500, Liquid "Deep" count 12000 (GPU; `cpu_limit` default 4000), Ball block 3×2 radius 30 from (0, −400); gravity (0, 900). Kept at 12k so lavapipe finishes the picture test.
5. **Fireflies** — ring 500, Swarm "Fireflies" count 20000 start Fill, gradient colours, one Ball radius 30 velocity (400, −300), swarm Motion attraction 1.2.

- [ ] **Step 1: Write the failing tests**: in `scenes.rs` tests extend `stage_scenes_resolve_and_round_trip` to `all` with 10 names; in `golden.rs` add the five names to `the_scenes_are_lively` with floors (Fountain ≥ 400 Spawn events; Wind tunnel ≥ 4 Enter events; Small pool ≥ 3; Deep pool ≥ 3; Fireflies ≥ 10) and a new `liquids_and_swarms_never_move_the_balls_log`: Small pool's ball log equals the same scene with the liquid thing removed; Fireflies' log equals the scene without the swarm.
- [ ] **Step 2: Run** `cargo test -p bpe-kit --test golden` → FAIL (names missing).
- [ ] **Step 3: Implement** the scenes; bless: `BPE_BLESS=1 cargo test -p bpe-kit scenes`, `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden` and the same with `--release`; `BPE_SAVE_RENDERS=1 cargo test -p bpe-kit --test pictures` and look at `target/pictures/*.png` (surface visible in both pools, dots in Fireflies, drops in Fountain).
- [ ] **Step 4: Run** `cargo test -p bpe-kit` (debug) and `cargo test -p bpe-kit --release --test golden` → PASS; seeking test passes for all ten.
- [ ] **Step 5: Commit** `test(kit): five stage 6 scenes with golden logs and pictures`

---

### Task 14: Editor: tree, inspector and preview for the new things

**Files:**
- Modify: `crates/bpe-studio/src/tree.rs` (Add → `bpe_kit::add_thing`; `paint_icon` arms `"emitter" | "liquid" | "swarm" | "field"`)
- Modify: `crates/bpe-studio/src/inspector.rs` (when `reg.modules_for(slot)` has more than one kind, "New {slot} module…" is a `menu_button` submenu with one entry per kind, labelled by the kind's name; one kind keeps today's button)
- Modify: `crates/bpe-studio/src/preview.rs` (`render_with_flow` with `budget = Some(CATCH_UP_BUDGET)`, `catching_up |= !status.caught_up`; draw gizmos: dashed `theme::MUTED` for every enabled emitter/field/liquid/swarm while paused, `theme::ACCENT` for the selected one; `hit_test` gains `Handle::Body(id)` for a gizmo `Dot`/`Circle` centre within `GIZMO_GRAB` screen px, tested after balls and before wall edges)
- Modify: `crates/bpe-studio/src/app.rs` only if the preview needs a new field (none expected)

- [ ] **Step 1: Write the failing kittest tests**: tree `adding_a_liquid_from_the_tree_links_liquid_surface` (click "Add" → "Liquid"; the new thing's Look slot is `Linked` to a module named "Liquid surface"); inspector `the_new_module_menu_lists_every_kind_of_the_slot` (select the ball, open the Look picker → "New Look module…" shows "Classic", "Liquid surface", "Glowing dots"; clicking "Glowing dots" links a `look.swarm` module); preview `hit_testing_finds_a_field_by_its_gizmo` (field at (300, 0) radius 100 → `hit_test` at its centre is `Handle::Body(field)`, and `apply_drag` moves it); `icons_exist_for_every_kit_kind` (paint_icon paints at least one shape for every `registry().things()` id, checked by a `Painter` with a recording layer, or simply that the match has no `_` fallthrough for kit kinds via a `const` list test).
- [ ] **Step 2: Run** `cargo test -p bpe-studio` → FAIL.
- [ ] **Step 3: Implement** the four changes.
- [ ] **Step 4: Run** `cargo test -p bpe-studio` → PASS; launch under Xvfb (`xvfb-run -a target/debug/ball-studio-next`), add each new thing, drag a field, scrub a Deep pool scene and take screenshots with `import -window root` into the stage folder.
- [ ] **Step 5: Commit** `feat(studio): add, inspect, drag and preview emitters, liquids, swarms and fields`

---

### Task 15: Docs, lint, full verification

**Files:**
- Modify: `README.md` (Ball Studio Next section: the four new things, the `cpu_limit` switch, what the GPU layers do and don't do)
- Modify: `crates/bpe-kit/README.md` if present (kind list)

- [ ] **Step 1:** `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings` → clean.
- [ ] **Step 2:** `cargo test --workspace --no-fail-fast` (background, ~10 min) → all PASS; `cargo test -p bpe-kit --release --test golden` → PASS.
- [ ] **Step 3:** Windows build: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`; `bpe-render-next` renders `scenes/next/deep-pool.ron` and `fireflies.ron` to mp4 (Linux build, lavapipe, `--scale 0.5 --max-frames 120` if slow); copy exe, mp4s, screenshots and `REPORT.md` (what shipped, gaps: emitters cannot feed GPU liquids or swarms, fields do not reach GPU layers, one wall per GPU layer, first 64 balls as GPU inputs) to `/mnt/project-files/ballphysics/modular-revamp/stage6/`.
- [ ] **Step 4: Commit** `docs: describe the flowing things`; then the fresh review, rebase on `main`, PR, merge.
