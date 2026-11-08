# Modular Ball Studio, Stage 2 (First Video) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first video from the new editor: a scene with Scene defaults, an Arena and Balls, with Look, Motion, Bounce and Sound modules, simulated by a new staged runtime, drawn by a new renderer, mixed by a new mixer, edited in `ball-studio-next.exe` (tree, inspector, preview, undo, save) and exported as an mp4 with sound.

**Architecture:** Five new crates on top of `bpe-core`. `bpe-runtime` owns the world (packed tables per thing), the fixed step loop with named stages, the event log and playback with checkpoints; plugins add work through `ThingSim` and `ModuleSim`. `bpe-gfx` draws a `Frame` that `ThingDraw` plugins fill, with the ported ball, wall, flash and bloom shaders. `bpe-mix` turns the event log into notes through `ModuleMix` plugins and renders them with the ported instruments. `bpe-kit` holds every plugin, the registry and the stage scenes. `bpe-cli` holds the ffmpeg export pipeline (as a library) and the `bpe-render-next` command. `bpe-studio` is the editor, `ball-studio-next`.

**Tech Stack:** Rust 2024 workspace, bpe-core (stage 1), glam 0.34, wgpu 30, bytemuck, pollster, hound 3.5, eframe/egui 0.37 (wgpu), egui-wgpu 0.37, cpal 0.18, rfd 0.17, anyhow, egui_kittest 0.37 (dev), png 0.18 (dev), ffmpeg on PATH for export.

**Spec:** `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (sections 3.9, 4, 5, 6, 7 stage 2, 8, 9)

## Global Constraints

- Old crates (`bpe-scene`, `bpe-sim`, `bpe-render`, `bpe-audio`, `bpe-export`, `bpe-app`, `bpe-music`, `bpe-graph`) and `bpe-core` / `bpe-settings-derive` are not modified. Proven code is ported by copying it into the new crates. `cargo test --workspace` must pass at the end of every task.
- Deviation from spec section 6, decided here: the shared export pipeline cannot move out of `bpe-export` (old crates stay untouched), so it is ported into `bpe-cli`, which is a library plus the `bpe-render-next` binary; `bpe-studio` depends on `bpe-cli` for export. Instruments are ported into `bpe-mix/src/instruments` rather than moved. `bpe-vm` is stage 4. Trails are a Trail slot in stage 5, so the stage 2 Look module has no trail settings; `look.styled` is also left to stage 5.
- Every map whose order can reach a saved file, a resolved result, a frame or a simulation is a `BTreeMap` or `BTreeSet`. No `HashMap`. No rayon. Iteration over things is in `ThingId` order, over items in index order.
- The runtime steps at the scene's `steps_per_second` (default 240), fixed step, never by wall-clock time. Running a scene twice, or seeking and playing on, gives an identical event log (checked by hash).
- Plugin ids: things `"scene"`, `"arena"`, `"balls"`; modules `"look.classic"`, `"motion.basic"`, `"bounce.basic"`, `"sound.notes"`. Every settings struct passes `bpe_core::check_registry` (at most 6 Simple settings).
- Video output unchanged from the old editor: H.264 yuv420p bt709 mp4 with AAC 192k 48 kHz audio, `-movflags +faststart`, even output sizes, crf 18 preset medium by default.
- A new scene (spec 4.9): Scene defaults, a circle Arena, one Balls thing holding a single ball. Library: `Plain look` (look.classic, white), `Ball look` (look.classic, colour `#ff5ca8`), `Default motion`, `Default bounce`, `Default sound`. The root links Plain look, Default motion, Default bounce and Default sound; the Balls thing links Ball look and inherits the rest; the Arena inherits everything.
- Exact texts: old file → `This scene was made with the old Ball Studio` (from bpe-core); missing ffmpeg → `could not start ffmpeg (<path>). Install ffmpeg and make sure it is on your PATH`; cancelled export → `export cancelled`.
- Code style follows the repo: `rustfmt.toml` (max width 120), plain-English doc comments on public items, the density of `bpe-core/src/resolve.rs`. Commit after every task.

## Review Focus

1. **A ball placed outside its arena** (dragged past the ring, or a block wider than the box) must be pulled back inside on the first step without NaN or a huge speed. Test: `a_ball_starting_outside_is_pulled_in` (Task 2).
2. **An arena smaller than its ball** (a shrinking ring, or thickness above the radius) must pin the ball at the centre without NaN. Test: `an_arena_smaller_than_the_ball_pins_it_without_nan` (Task 2).
3. **A scene whose linked module is missing** (hand-edited, or a module removed) must still simulate, draw and sound, with the problem reported. Test: `a_missing_look_module_still_draws` (Task 7).
4. **A huge scene** (a 200 by 200 block) must not freeze the editor: seeking stops at its time budget and carries on next frame. Test: `seek_within_stops_at_its_budget` (Task 4).
5. **Exporting without ffmpeg, or cancelling** must give a clear message and leave no half-written file. Tests: `export_without_ffmpeg_fails_clearly_and_leaves_no_file`, `a_cancelled_export_leaves_no_file` (Task 9).

---

## File Structure

```
crates/bpe-runtime/
  Cargo.toml                 deps: bpe-core, glam
  src/lib.rs                 module list and re-exports
  src/events.rs              Party, EventKind, Event, Log (push, slices by step, hash)
  src/bodies.rs              Bodies: packed columns for round moving items
  src/walls.rs               Outline, Wall, Contact, keep_inside, reflect (ported from bpe-sim/src/arena.rs)
  src/plugins.rs             ThingSim, ModuleSim, ThingState/ModuleState, Sims registry, Limits
  src/world.rs               World: build, step (stages and substeps), fingerprint
  src/playback.rs            Playback: checkpoints, seek, seek_within
crates/bpe-gfx/
  Cargo.toml                 deps: bpe-core, bpe-runtime, wgpu, bytemuck, glam, pollster, anyhow; dev: png
  src/lib.rs                 Renderer, Readback, headless_device (ported from bpe-render/src/lib.rs)
  src/frame.rs               Frame, BallInstance, ShapeInstance, FlashInstance, ThingDraw, Draws, build_frame
  src/shaders/scene.wgsl     background, outlines, classic balls, flashes (ported, trimmed)
  src/shaders/post.wgsl      bloom and composite (ported)
crates/bpe-mix/
  Cargo.toml                 deps: bpe-core, bpe-runtime, hound, anyhow
  src/lib.rs                 SAMPLE_RATE, Soundtrack, Note, MixSettings, MixCx, ModuleMix, Mixes, mix
  src/chords.rs              Chords (ported Progression + chord name parsing)
  src/level.rs               normalize_loudness, limit_peaks, mix_notes (ported)
  src/instruments/mod.rs     Instrument (Choice), render, NoteCache
  src/instruments/dsp.rs     ported from bpe-audio/src/instruments/dsp.rs
  src/instruments/synth.rs   ported from bpe-audio/src/instruments/synth.rs
crates/bpe-kit/
  Cargo.toml                 deps: bpe-core, bpe-runtime, bpe-gfx, bpe-mix, glam; dev: bpe-scene, bpe-sim, bpe-render
  src/lib.rs                 Kit { registry, sims, draws, mixes }, kit(), new_scene, mix_scene
  src/things/scene/{mod,sim,draw}.rs
  src/things/arena/{mod,sim,draw}.rs
  src/things/balls/{mod,sim,draw}.rs
  src/modules/look/{mod,draw}.rs
  src/modules/motion/{mod,sim}.rs
  src/modules/bounce/{mod,sim}.rs
  src/modules/sound/{mod,mix}.rs
  src/scenes.rs              the five stage scenes
  tests/golden.rs, golden.txt, golden-debug.txt
  tests/speed.rs             (ignored) old engine vs new
crates/bpe-cli/
  Cargo.toml                 deps: bpe-core, bpe-runtime, bpe-gfx, bpe-mix, bpe-kit, anyhow; [[bin]] bpe-render-next
  src/lib.rs                 ExportOptions, Encoder, export_video, find_ffmpeg, output_size (ported from bpe-export)
  src/main.rs                the command line
  tests/export.rs            export check with ffprobe
crates/bpe-studio/
  Cargo.toml                 [[bin]] ball-studio-next; deps: the above, eframe, egui-wgpu, cpal, rfd, anyhow; dev: egui_kittest
  src/main.rs                window setup
  src/app.rs                 Studio: state, frame layout, shortcuts, edits, files
  src/history.rs             undo and redo
  src/theme.rs               ported from bpe-app/src/theme.rs
  src/settings.rs            last scene file (ported, file last-scene-next.txt)
  src/tree.rs                scene tree panel
  src/inspector.rs           header, kind settings, slot cards
  src/widgets.rs             one widget per SettingDef, reset on double-click or Alt-click
  src/preview.rs             renderer in egui, drag to move, aim and resize
  src/transport.rs           play, scrub, hit lane, loop, mute
  src/audio_out.rs           ported from bpe-app/src/audio_out.rs
  src/sound_job.rs           background soundtrack mixing
  src/export_window.rs       export dialog with progress
scenes/next/*.ron            the five stage scenes, saved
```

---

### Task 1: Events and the log

**Files:**
- Create: `crates/bpe-runtime/Cargo.toml`, `crates/bpe-runtime/src/lib.rs`, `crates/bpe-runtime/src/events.rs`
- Modify: `Cargo.toml` (workspace dependencies: `bpe-runtime`, `bpe-gfx`, `bpe-mix`, `bpe-kit`, `bpe-cli` as path deps)

**Interfaces:**
- Consumes: `bpe_core::ThingId`, `glam::Vec2`.
- Produces (spec 5.5):
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)] pub struct Party { pub thing: ThingId, pub item: u32 }`
  - `#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)] pub enum EventKind { Hit, Spawn, Remove, Enter, Leave, Beat, Note, Custom(String) }` with `pub fn tag(&self) -> u8` (0..=7 in that order).
  - `#[derive(Clone, Debug, PartialEq)] pub struct Event { pub step: u64, pub kind: EventKind, pub a: Party, pub b: Option<Party>, pub point: Vec2, pub normal: Vec2, pub speed: f32 }` with `pub fn time(&self, steps_per_second: u32) -> f64` (`step / rate`).
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Log` holding `Vec<Event>`, with `push(&mut self, e: Event)` (panics in debug if `e.step` is below the last step: the log is step-sorted), `events(&self) -> &[Event]`, `len`, `is_empty`, `truncate(&mut self, len: usize)`, `since(&self, step: u64) -> &[Event]` (events with `step >= step`), `between(&self, from: u64, to: u64) -> &[Event]` (from inclusive, to exclusive), and `hash(&self) -> u64`: FNV-1a (offset `0xcbf2_9ce4_8422_2325`, prime `0x100_0000_01b3`) over, per event, `step` LE bytes, `kind.tag()`, the bytes of a `Custom` name, `a.thing.0`, `a.item`, `b` as (1, thing, item) or (0), `point.x/y` bits, `normal.x/y` bits, `speed` bits.

- [ ] **Step 1: Write failing tests** in `events.rs`

```rust
#[test]
fn slices_follow_steps() {
    let mut log = Log::default();
    for step in [1, 1, 3, 4, 4, 9] {
        log.push(Event { step, ..hit() });
    }
    assert_eq!(log.since(4).len(), 3);
    assert_eq!(log.between(1, 4).len(), 3);
    assert_eq!(log.between(5, 5).len(), 0);
}

#[test]
fn hash_changes_with_any_field_and_not_with_time() {
    let mut a = Log::default();
    a.push(hit());
    let mut b = Log::default();
    b.push(Event { speed: 2.0, ..hit() });
    assert_ne!(a.hash(), b.hash());
    let mut c = Log::default();
    c.push(hit());
    assert_eq!(a.hash(), c.hash());
    assert_eq!(Log::default().hash(), 0xcbf2_9ce4_8422_2325);
}
```

`hit()` builds `Event { step: 1, kind: EventKind::Hit, a: Party { thing: ThingId(2), item: 0 }, b: Some(Party { thing: ThingId(1), item: 0 }), point: Vec2::new(1.0, 2.0), normal: Vec2::Y, speed: 1.0 }`.

- [ ] **Step 2: Run** `cargo test -p bpe-runtime`. Expected: FAIL (crate missing).
- [ ] **Step 3: Implement** the crate and `events.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add bpe-runtime with the event log"`

---

### Task 2: Bodies and walls

**Files:**
- Create: `crates/bpe-runtime/src/bodies.rs`, `crates/bpe-runtime/src/walls.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Bodies { pub position: Vec<Vec2>, pub velocity: Vec<Vec2>, pub radius: Vec<f32>, pub previous: Vec<Vec2>, pub alive: Vec<bool>, pub hits: Vec<u32> }` with `push(&mut self, position: Vec2, velocity: Vec2, radius: f32) -> u32` (sets previous = position, alive, hits 0), `len`, `alive_count`, `memory(&self) -> usize`.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub enum Outline { Ring { center: Vec2, radius: f32, sides: u32, angle: f32, thickness: f32 }, Box { center: Vec2, half: Vec2, angle: f32, thickness: f32 } }` (`sides` 0 is round, 3 or more a regular polygon; `angle` radians clockwise from +x). Methods `center()`, `thickness()`, `point(u: f32) -> Vec2` and `inside(&self, p: Vec2) -> bool`, ported from `bpe-sim/src/arena.rs`.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Wall { pub outline: Outline, pub spin: f32, pub growth: f32 }` (`spin` radians per second clockwise; `growth` fraction of size per second, negative while shrinking).
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Contact { pub point: Vec2, pub normal: Vec2, pub speed: f32, pub wall_velocity: Vec2 }` (`normal` points from the wall towards the ball; `speed` is the relative speed into the wall).
  - `pub fn keep_inside(position: &mut Vec2, velocity: Vec2, radius: f32, wall: &Wall) -> Option<Contact>`: moves the position back inside each violated plane or ring (ported `collide_closed` / `collide_planes`, position part), and returns the hardest contact whose `speed > 0` (the ball was moving into it). Velocity is left alone.
  - `pub fn reflect(velocity: Vec2, contact: &Contact, bounciness: f32) -> Vec2`: `relative + normal * speed * (1 + bounciness) + wall_velocity` (ported `bounce`).
  - `pub fn wall_velocity(point: Vec2, center: Vec2, spin: f32, growth: f32) -> Vec2` (ported).

- [ ] **Step 1: Write failing tests** in `walls.rs`

```rust
fn ring(radius: f32) -> Wall {
    Wall { outline: Outline::Ring { center: Vec2::ZERO, radius, sides: 0, angle: 0.0, thickness: 2.0 }, spin: 0.0, growth: 0.0 }
}

#[test]
fn a_ball_moving_into_the_ring_bounces_back() {
    let mut p = Vec2::new(99.0, 0.0);
    let v = Vec2::new(100.0, 0.0);
    let c = keep_inside(&mut p, v, 5.0, &ring(100.0)).unwrap();
    assert!((p.x - 94.0).abs() < 1e-4);
    assert_eq!(c.normal, Vec2::new(-1.0, 0.0));
    assert!((c.speed - 100.0).abs() < 1e-4);
    assert!((reflect(v, &c, 1.0).x + 100.0).abs() < 1e-4);
    assert!((reflect(v, &c, 0.5).x + 50.0).abs() < 1e-4);
}

#[test]
fn a_ball_moving_away_is_pushed_in_but_not_bounced() {
    let mut p = Vec2::new(99.0, 0.0);
    assert!(keep_inside(&mut p, Vec2::new(-10.0, 0.0), 5.0, &ring(100.0)).is_none());
    assert!((p.x - 94.0).abs() < 1e-4);
}

#[test]
fn a_ball_starting_outside_is_pulled_in() {
    for outline in [
        Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 0, angle: 0.0, thickness: 2.0 },
        Outline::Ring { center: Vec2::ZERO, radius: 100.0, sides: 6, angle: 0.3, thickness: 2.0 },
        Outline::Box { center: Vec2::ZERO, half: Vec2::new(100.0, 60.0), angle: 0.2, thickness: 2.0 },
    ] {
        let mut p = Vec2::new(900.0, 700.0);
        keep_inside(&mut p, Vec2::ZERO, 5.0, &Wall { outline, spin: 0.0, growth: 0.0 });
        assert!(p.is_finite() && p.length() < 101.0, "{outline:?} left the ball at {p}");
    }
}

#[test]
fn an_arena_smaller_than_the_ball_pins_it_without_nan() {
    let mut p = Vec2::new(3.0, 1.0);
    let c = keep_inside(&mut p, Vec2::new(50.0, 0.0), 20.0, &ring(10.0));
    assert!(p.is_finite() && p.length() < 1e-3);
    assert!(c.is_none_or(|c| c.point.is_finite() && c.normal.is_finite()));
}

#[test]
fn a_spinning_wall_carries_the_ball() {
    let wall = Wall { spin: 1.0, ..ring(100.0) };
    let mut p = Vec2::new(99.0, 0.0);
    let c = keep_inside(&mut p, Vec2::new(10.0, 0.0), 5.0, &wall).unwrap();
    // The wall at (99, 0) turning clockwise (y down) moves in +y at 99 px/s.
    assert!((c.wall_velocity.y - 99.0).abs() < 1.0 && c.wall_velocity.x.abs() < 1e-3);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime walls`. Expected: FAIL.
- [ ] **Step 3: Implement** `bodies.rs` and `walls.rs`. Port the maths line for line, splitting `bounce` into `keep_inside` (position) and `reflect` (velocity). For a polygon or box, correct every violated plane and keep the contact with the largest `speed`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add body tables and closed-wall contacts to the runtime"`

---

### Task 3: Plugins, the world and the step loop

**Files:**
- Create: `crates/bpe-runtime/src/plugins.rs`, `crates/bpe-runtime/src/world.rs`

**Interfaces:**
- Consumes: Tasks 1-2, `bpe_core::{Scene, Thing, Resolved, ResolvedThing, Params, SlotId, ThingId}`.
- Produces in `plugins.rs`:
  - `pub trait ThingState: Any + Send + Sync { fn clone_box(&self) -> Box<dyn ThingState>; fn as_any(&self) -> &dyn Any; fn as_any_mut(&mut self) -> &mut dyn Any; }` with a blanket `impl<T: Any + Clone + Send + Sync> ThingState for T`, `impl Clone for Box<dyn ThingState>`, and `pub type ModuleState = dyn ThingState`.
  - `pub fn state<T: 'static>(s: &dyn ThingState) -> &T` and `state_mut`, which panic with `"plugin state is not a <type name>"`.
  - `pub struct Limits { pub ceiling: f32, pub resting: f32 }`: the simulation's own top speed (`16 px × 64 substeps × steps_per_second`) and the speed below which a touch is resting, not a hit (`|gravity| × dt × 4 + 1`).
  - `pub trait ThingSim: Send + Sync + 'static { fn kind(&self) -> &'static str; fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World); fn scripted(&self, _thing: ThingId, _world: &mut World) {} fn affects_physics(&self, _key: &str) -> bool { true } }`
  - `pub trait ModuleSim: Send + Sync + 'static { fn kind(&self) -> &'static str; fn prepare(&self, params: &Params) -> Box<dyn ThingState>; fn forces(&self, _state: &dyn ThingState, _bodies: &mut Bodies, _gravity: Vec2, _dt: f32) {} fn respond(&self, state: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, limits: &Limits) { /* default: reflect with bounciness 1 */ } fn takes_over(&self, _state: &dyn ThingState) -> bool { false } }`
  - `#[derive(Default)] pub struct Sims` with `add_thing(&mut self, s: impl ThingSim)`, `add_module(&mut self, s: impl ModuleSim)` (panic `"sim registered twice: <id>"`), `thing(&self, kind: &str) -> Option<&dyn ThingSim>`, `module(&self, kind: &str) -> Option<&dyn ModuleSim>`.
- Produces in `world.rs`:
  - `#[derive(Clone)] pub struct World { pub step: u64, pub steps_per_second: u32, pub gravity: Vec2, pub bodies: BTreeMap<ThingId, Bodies>, pub walls: BTreeMap<ThingId, Wall>, pub states: BTreeMap<ThingId, Box<dyn ThingState>>, modules: BTreeMap<ThingId, BTreeMap<SlotId, (String, Box<dyn ThingState>)>> }` (module kind id and prepared state per thing and slot, for Motion and Bounce only).
  - `impl World`: `pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World` (steps_per_second 240 and gravity zero until the root's sim sets them; things built in `ThingId` order; a thing whose kind has no sim is skipped; Motion and Bounce slots prepared through their `ModuleSim`, slots whose module kind has no sim are skipped), `pub fn time(&self) -> f64`, `pub fn dt(&self) -> f32`, `pub fn limits(&self) -> Limits`, `pub fn step(&mut self, sims: &Sims, log: &mut Log)`, `pub fn advance_to(&mut self, time: f64, sims: &Sims, log: &mut Log)` (steps while `step < floor(time × rate)`), `pub fn memory(&self) -> usize`, `pub fn module_state(&self, thing: ThingId, slot: SlotId) -> Option<(&str, &dyn ThingState)>`.
  - `pub fn fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64`: FNV over every resolved thing's id, kind, `transform`, `enabled`, params whose key `affects_physics`, and the kind and params of its Motion and Bounce slots, plus `steps_per_second`-like root keys through the same `affects_physics` filter. Two scenes that differ only in Look, Sound or visual root keys fingerprint the same.
- Step order (spec 5.3), inside `step`: `step += 1`; every `Bodies` velocity clamped to `limits.ceiling`; substeps `n = clamp(ceil((fastest + |gravity| dt) dt / 16), 1, 64)`; Scripted: each thing's sim `scripted` in id order; then for each substep: Forces (each body thing in id order: its Motion module's `forces`, or nothing when it has none; Integrate: `previous = position; position += velocity × dt` for alive items); Collide and Respond: for each body thing and each wall thing in id order, `keep_inside` each alive item; on a contact pick the Bounce module: the wall thing's when it `takes_over`, else the mover's, else the default reflect; call `respond`; then `hits += 1` and a `Hit` event (`a` mover, `b` wall, `point`, `normal`, `speed`, `step`) when `speed >= limits.resting`. Spawn and Rules stages are empty hooks this stage.

- [ ] **Step 1: Write failing tests** in `world.rs`, using two tiny test sims defined under `#[cfg(test)]` there: `TestRoot` (kind `"scene"`, build sets rate 240 and gravity from params `gravity`), `TestWall` (kind `"wall"`, build inserts a ring of radius from param `radius`), `TestDots` (kind `"dots"`, build pushes one body per `count` at `transform.position` with velocity `velocity`). Use `bpe_core::testkit` style registries built inline.

```rust
#[test]
fn two_runs_give_the_same_log_and_a_ball_bounces() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let run = || { let mut w = World::build(&scene, &resolved, &sims); let mut log = Log::default(); w.advance_to(5.0, &sims, &mut log); (w, log) };
    let (w1, log1) = run();
    let (_, log2) = run();
    assert_eq!(log1, log2);
    assert!(log1.len() >= 3, "a launched ball under gravity should hit the ring a few times in 5 s");
    assert_eq!(log1.events()[0].kind, EventKind::Hit);
    assert!(w1.bodies[&DOTS].position[0].length() < 100.0);
}

#[test]
fn things_run_in_id_order_and_events_are_step_sorted() {
    let (scene, resolved, sims) = dots_in_ring(50);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(3.0, &sims, &mut log);
    assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step));
    assert!(log.events().windows(2).filter(|p| p[0].step == p[1].step).all(|p| p[0].a <= p[1].a));
}

#[test]
fn the_wall_can_take_over_the_bounce() {
    // A dead wall (bounciness 0 through a test Bounce module with takes_over) stops the ball.
    let (scene, resolved, sims) = dots_in_ring_with_dead_wall();
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(2.0, &sims, &mut Log::default());
    assert!(w.bodies[&DOTS].velocity[0].length() < 50.0);
}

#[test]
fn look_changes_keep_the_fingerprint() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let before = fingerprint(&scene, &resolved, &sims);
    scene.things.get_mut(&DOTS).unwrap().params.set("tint", Value::Number(3.0)); // a key TestDots says is visual
    let resolved = resolve(&scene, &reg());
    assert_eq!(fingerprint(&scene, &resolved, &sims), before);
    scene.things.get_mut(&DOTS).unwrap().transform.position.x += 1.0;
    let resolved = resolve(&scene, &reg());
    assert_ne!(fingerprint(&scene, &resolved, &sims), before);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime world`. Expected: FAIL.
- [ ] **Step 3: Implement** `plugins.rs` and `world.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add the world, its plugins and the staged step loop"`

---

### Task 4: Playback with checkpoints

**Files:**
- Create: `crates/bpe-runtime/src/playback.rs`

**Interfaces:**
- Produces: `#[derive(Clone)] pub struct Playback` with `pub fn new(scene: &Scene, resolved: &Resolved, sims: Arc<Sims>) -> Self`, `world(&self) -> &World`, `log(&self) -> &Log`, `time(&self) -> f64`, `sims(&self) -> &Arc<Sims>`, `recent(&self, seconds: f64) -> &[Event]` (events in the last `seconds` up to now), `seek(&mut self, time: f64)`, `seek_within(&mut self, time: f64, budget: Option<Duration>) -> bool` (ported from `bpe_sim::Playback::seek_within`: checkpoints every 0.5 s, kept under 192 MB by halving their rate, budget checked every 8 steps), `memory(&self) -> usize`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn seeking_back_and_forth_matches_a_straight_run() {
    let (scene, resolved, sims) = dots_in_ring(20);
    let mut straight = Playback::new(&scene, &resolved, sims.clone());
    straight.seek(6.0);
    let mut jumpy = Playback::new(&scene, &resolved, sims);
    for t in [2.0, 5.5, 1.2, 6.0] {
        jumpy.seek(t);
    }
    assert_eq!(straight.log().hash(), jumpy.log().hash());
    assert_eq!(straight.world().bodies[&DOTS].position, jumpy.world().bodies[&DOTS].position);
}

#[test]
fn seek_within_stops_at_its_budget() {
    let (scene, resolved, sims) = dots_in_ring(40_000);
    let mut p = Playback::new(&scene, &resolved, sims);
    let done = p.seek_within(20.0, Some(Duration::from_millis(20)));
    assert!(!done && p.time() > 0.0 && p.time() < 20.0);
    assert!(p.seek_within(p.time(), Some(Duration::from_millis(20))));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-runtime playback`. Expected: FAIL.
- [ ] **Step 3: Implement** `playback.rs`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime && cargo clippy -p bpe-runtime -- -D warnings`. Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `git commit -m "Add playback with checkpoints and budgeted seeking"`

---

### Task 5: The kit's data plugins, new scene and stage scenes

**Files:**
- Create: `crates/bpe-kit/Cargo.toml`, `crates/bpe-kit/src/lib.rs`, `src/things/{scene,arena,balls}/mod.rs`, `src/modules/{look,motion,bounce,sound}/mod.rs`, `src/scenes.rs`, `scenes/next/` (five `.ron` files written by the test below)

**Interfaces:**
- Consumes: `bpe_core::{Settings, Choice, ThingKind, ModuleKind, Registry, Scene, SlotId, Color, Value}`.
- Produces the settings structs (field = key; label; default; range; unit; depth; flags). `simple` fields are listed first in each struct:
  - `SceneSettings` (kind `"scene"`, name `"Scene defaults"`, slots and default slots all eight): `duration: f32 = 20` (Length, 1..=600, s, simple); `gravity: Vec2 = [0, 500]` (Gravity, -5000..=5000, px/s², simple, keyframe, random); `background_top: Color = #02022a` (simple, keyframe, random); `background_bottom: Color = #010006` (simple, keyframe, random); `bloom: f32 = 0.15` (0..=1, simple, keyframe, random, pulse); `chords: String = "C 4.2, Dsus4 4, Am 5, C 4, Am 3.1, Dsus4 3"` (Chords, simple, group Sound, help "Chord names with how many seconds each lasts, repeating"); `width: u32 = 1080` (2..=7680, px, group Video); `height: u32 = 1920`; `fps: u32 = 60` (1..=240); `steps_per_second: u32 = 240` (60..=2000); `exposure: f32 = 1` (0.1..=4, keyframe, pulse); `loudness: f32 = -14` (-30..=-6, dB, group Sound); `seed: u32 = 1` (0..=1_000_000, group Sound, random).
  - `ArenaSettings` (kind `"arena"`, name `"Arena"`, icon `"arena"`, slots `[Look, Bounce, Sound, Particles, Rules, Shader]`, default `[Look, Bounce, Sound]`): `shape: ArenaShape = Circle` (Choice circle/box/polygon, simple, random); `radius: f32 = 508` (20..=2000, px, simple, keyframe, random); `thickness: f32 = 3` (0..=40, px, simple, keyframe); `spin: f32 = 0` (-360..=360, deg/s, simple, keyframe, random); `width: f32 = 900` (20..=4000, px, help "Box only"); `height: f32 = 1500`; `sides: u32 = 6` (3..=12, random); `shrink: f32 = 0` (0..=1, "per s", keyframe, help "How much of its size the arena loses each second"); `min_size: f32 = 100` (10..=2000, px).
  - `BallsSettings` (kind `"balls"`, name `"Balls"`, icon `"ball"`, slots `[Look, Trail, Motion, Bounce, Sound, Particles, Rules]`, default `[Look, Motion, Bounce, Sound]`): `arrangement: Arrangement = Single` (Choice single/block/ring, simple, random); `radius: f32 = 24` (0.5..=200, px, simple, keyframe, random); `velocity: Vec2 = [300, -200]` (Launch speed, -5000..=5000, px/s, simple, random); `columns: u32 = 10` (1..=200, simple); `rows: u32 = 10` (1..=200, simple); `spacing: f32 = 19` (1..=400, px, simple); `count: u32 = 24` (1..=2000, help "Ring only"); `ring_radius: f32 = 200` (1..=2000, px).
  - `ClassicLook` (kind `"look.classic"`, name `"Classic"`, slot Look): `color: Color = WHITE` (simple, keyframe, random); `glow: f32 = 0.12` (0..=2, simple, keyframe, random, pulse); `flash: f32 = 0.4` (Flash, 0..=3, simple, keyframe, random); `gradient: Vec<Color> = []` (Colours, random, help "Spread across the balls from the first row to the last; empty uses Colour"); `flash_size: f32 = 26` (0..=300, px, keyframe); `flash_seconds: f32 = 0.25` (0.02..=2, s).
  - `BasicMotion` (kind `"motion.basic"`, name `"Basic"`, slot Motion): `gravity_scale: f32 = 1` (Gravity, -5..=5, simple, keyframe, random, pulse); `drag: f32 = 0` (Air drag, 0..=5, "per s", simple, keyframe, random).
  - `BasicBounce` (kind `"bounce.basic"`, name `"Basic"`, slot Bounce): `bounciness: f32 = 1` (0..=1.5, simple, keyframe, random, pulse); `speed_gain: f32 = 1` (0.5..=1.5, simple, keyframe, random); `top_speed: f32 = 3000` (50..=20000, px/s, simple, keyframe); `grow: f32 = 0` (Grow per hit, -20..=20, px, simple, keyframe, random); `max_size: f32 = 60` (0.5..=400, px); `takes_over: bool = false` (Takes over, help "Decides every bounce against this thing, even when the other thing is the one moving").
  - `NoteSound` (kind `"sound.notes"`, name `"Notes"`, slot Sound): `instrument: Instrument = Pluck` (Choice from `bpe_mix::Instrument`, simple, random); `volume: f32 = 0` (-40..=6, dB, simple, keyframe); `low_note: u32 = 48` (0..=127, simple); `high_note: u32 = 84` (0..=127, simple); `quietest_hit: f32 = 20` (0..=3000, px/s); `loudest_hit: f32 = 600` (1..=10000, px/s); `stereo_width: f32 = 0.6` (0..=1); `decay: f32 = 0.9` (0.05..=5, s, help "Pluck only"); `brightness: f32 = 0.5` (0..=1, help "Pluck only").
  - `pub fn registry() -> Registry` registering all seven kinds. `pub fn new_scene(reg: &Registry) -> Scene` as in Global Constraints (scene name `"New scene"`, Balls thing renamed `"Ball"`, at position `[0, -200]`; the Arena at the origin).
  - `scenes.rs`: `pub fn stage2(reg: &Registry) -> Vec<(String, Scene)>` building, in this order: `"One ball"` (= `new_scene`); `"Reel block"` (28×28 block, spacing 19, radius 4.5, at `[37, -127]`, velocity `[-480, 265]`, gravity `[0, 160]`, duration 23, gradient `#3e9bff #8a4dff #ff3ec8 #ff6a5c #ffb82e`, arena radius 508 thickness 3 colour `#d8dcd0`); `"Box drop"` (box 900×1500, 10×10 block spacing 40 radius 12 at `[0, -500]`, velocity `[200, 0]`, gravity `[0, 900]`, bounciness 0.85, drag 0.05); `"Spinning hexagon"` (polygon 6 sides radius 450 spin 25, ring of 24 balls radius 10 ring radius 200, velocity `[0, 0]`, speed gain 1.02, top speed 2500, glow 0.5); `"Shrinking ring"` (circle radius 500 shrink 0.03 min size 200, one ball radius 30 velocity `[600, -300]`, grow 1.5 max size 90, arena Bounce with bounciness 1.05 and takes_over, instrument marimba). Each scene's modules are library modules on the root or on the thing as needed.

- [ ] **Step 1: Write failing tests** in `lib.rs` and `scenes.rs`

```rust
#[test]
fn every_kit_plugin_passes_the_contract() {
    assert_eq!(bpe_core::check_registry(&registry()), Vec::<String>::new());
}

#[test]
fn a_new_scene_is_an_arena_and_one_ball_with_shared_modules() {
    let reg = registry();
    let s = new_scene(&reg);
    let kinds: Vec<&str> = s.order.iter().map(|id| s.things[id].kind.as_str()).collect();
    assert_eq!(kinds, ["scene", "arena", "balls"]);
    assert_eq!(s.library.modules.len(), 5);
    let res = bpe_core::resolve(&s, &reg);
    assert!(res.problems.is_empty());
    let ball = &res.things[&ThingId(2)];
    assert_eq!(ball.slots[&SlotId::Look].params.get("color"), Some(&Value::Color(Color::rgb(0xff, 0x5c, 0xa8))));
    assert_eq!(ball.slots[&SlotId::Bounce].from, ThingId::ROOT);
    assert_eq!(res.things[&ThingId(1)].slots[&SlotId::Look].from, ThingId::ROOT);
}

#[test]
fn stage_scenes_resolve_and_round_trip() {
    let reg = registry();
    let scenes = scenes::stage2(&reg);
    assert_eq!(scenes.len(), 5);
    for (name, scene) in &scenes {
        assert!(bpe_core::resolve(scene, &reg).problems.is_empty(), "{name}");
        let text = bpe_core::save(scene);
        assert_eq!(&bpe_core::load(&text, &reg).unwrap(), scene, "{name}");
        if std::env::var_os("BPE_BLESS").is_some() {
            let file = format!("{}/../../scenes/next/{}.ron", env!("CARGO_MANIFEST_DIR"), name.to_lowercase().replace(' ', "-"));
            std::fs::write(file, text).unwrap();
        }
    }
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement** the data plugins, `registry`, `new_scene` and `scenes::stage2`. `bpe-kit` depends on `bpe-mix` for `Instrument`, so create `crates/bpe-mix` now with only `Cargo.toml`, `lib.rs` and `instruments/mod.rs` holding the `Instrument` Choice enum (the rest comes in Task 8).
- [ ] **Step 4: Run** `BPE_BLESS=1 cargo test -p bpe-kit` then `cargo test -p bpe-kit`. Expected: PASS, and five files under `scenes/next/`.
- [ ] **Step 5: Commit** `git commit -m "Add the kit's data plugins, the new scene and the stage scenes"`

---

### Task 6: The kit's simulation parts and golden logs

**Files:**
- Create: `src/things/scene/sim.rs`, `src/things/arena/sim.rs`, `src/things/balls/sim.rs`, `src/modules/motion/sim.rs`, `src/modules/bounce/sim.rs`, `crates/bpe-kit/tests/golden.rs`, `tests/golden.txt`, `tests/golden-debug.txt`
- Modify: `src/lib.rs` (`pub fn sims() -> Sims`, `pub struct Kit`, `pub fn kit() -> Kit`)

**Interfaces:**
- Consumes: `bpe_runtime::{ThingSim, ModuleSim, World, Bodies, Wall, Outline, Contact, Limits, reflect, state}`.
- Produces:
  - `SceneSim` (`"scene"`): `build` sets `world.steps_per_second` and `world.gravity` from `SceneSettings::from_params`; `affects_physics` is true only for `gravity`, `steps_per_second`, `duration`.
  - `ArenaSim` (`"arena"`): `build` inserts `world.walls[id]` from the settings and `thing.transform` (ring: `sides 0`; polygon: `sides`, angle `rotation + π/2`; box: half `[width, height] / 2`), and a state `ArenaState { settings: ArenaSettings, base: Outline, angle: f32, scale: f32 }`; `scripted` advances `angle += spin dt` (radians, clockwise), `scale = max(scale × (1 − shrink dt), min_size / radius)`, rewrites `world.walls[id]` with the scaled, turned outline and `spin`, `growth = −shrink` while still above the minimum, else 0.
  - `BallsSim` (`"balls"`): `build` pushes bodies at `transform.position` plus the arrangement's offsets, in row-major order for a block (rows top to bottom), clockwise from the top for a ring (`count` points on `ring_radius`), one at the centre for single; every item gets `velocity`. State: `BallsState { settings: BallsSettings }`.
  - `BasicMotionSim`: `prepare` → `BasicMotion`; `forces`: `v = (v + gravity × gravity_scale × dt) × max(1 − drag × dt, 0)` for alive items.
  - `BasicBounceSim`: `prepare` → `BasicBounce`; `respond`: `v = reflect(v, contact, bounciness) × speed_gain`, then clamped to `min(top_speed, limits.ceiling)`; `radius = clamp(radius + grow, 0.5, max(max_size, 0.5))`; `takes_over` from the setting.
  - `pub struct Kit { pub registry: Registry, pub sims: Arc<Sims>, pub draws: Draws, pub mixes: Mixes }` and `pub fn kit() -> Kit` (draws and mixes filled in Tasks 7 and 8; until then the fields exist with empty registries).
  - `tests/golden.rs`: for each stage scene, `Playback::new` + `seek(20.0)`, then `name\tevents\thash` lines compared to `golden.txt` (release) or `golden-debug.txt` (debug), rewritten with `BLESS_GOLDEN=1`; plus the replay check: a second playback seeking `[7.0, 3.0, 20.0]` must hash the same; and `Reel block` must log at least 1000 hits in 20 s and `One ball` at least 10.

- [ ] **Step 1: Write failing tests** in `tests/golden.rs` (as described) and in `balls/sim.rs`:

```rust
#[test]
fn a_block_is_laid_out_row_by_row_and_a_ring_clockwise_from_the_top() {
    let (scene, resolved, kit) = scene_with_balls("block", |p| { p.set("columns", Value::Int(2)); p.set("rows", Value::Int(2)); p.set("spacing", Value::Number(10.0)); });
    let w = World::build(&scene, &resolved, &kit.sims);
    assert_eq!(w.bodies[&BALLS].position, vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(-5.0, 5.0), Vec2::new(5.0, 5.0)]);
    let (scene, resolved, kit) = scene_with_balls("ring", |p| { p.set("count", Value::Int(4)); p.set("ring_radius", Value::Number(10.0)); });
    let w = World::build(&scene, &resolved, &kit.sims);
    let p = &w.bodies[&BALLS].position;
    assert!((p[0] - Vec2::new(0.0, -10.0)).length() < 1e-4 && (p[1] - Vec2::new(10.0, 0.0)).length() < 1e-4);
}

#[test]
fn grow_and_speed_gain_apply_on_each_hit() {
    let (scene, resolved, kit) = one_ball_with_bounce(|p| { p.set("grow", Value::Number(2.0)); p.set("speed_gain", Value::Number(1.1)); p.set("max_size", Value::Number(30.0)); });
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(10.0);
    let hits = playback.log().len() as f32;
    assert!(hits >= 3.0);
    assert_eq!(playback.world().bodies[&BALLS].radius[0], (24.0 + 2.0 * hits).min(30.0));
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement** the sim parts, `sims()`, `Kit`, `kit()`.
- [ ] **Step 4: Run** `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden && BLESS_GOLDEN=1 cargo test --release -p bpe-kit --test golden`, then `cargo test -p bpe-kit`. Expected: PASS, two golden files written.
- [ ] **Step 5: Commit** `git commit -m "Simulate arenas and balls through the kit's plugins, with golden logs"`

---

### Task 7: The renderer and the kit's drawing parts

**Files:**
- Create: `crates/bpe-gfx/Cargo.toml`, `src/lib.rs`, `src/frame.rs`, `src/shaders/scene.wgsl`, `src/shaders/post.wgsl`, `crates/bpe-kit/src/things/{scene,arena,balls}/draw.rs`, `crates/bpe-kit/src/modules/look/draw.rs`, `crates/bpe-kit/tests/pictures.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub fn draws() -> Draws`, fill `Kit::draws`)

**Interfaces:**
- Produces in `bpe-gfx`:
  - `pub const OUTPUT_FORMAT: wgpu::TextureFormat = Rgba8Unorm` (sRGB-encoded bytes, as before).
  - `#[repr(C)] #[derive(Clone, Copy, Pod, Zeroable)] pub struct BallInstance { pub center: [f32; 2], pub radius: f32, pub glow: f32, pub color: [f32; 4] }` (linear light; an additive colour has alpha −1, see `blend_alpha` ported from `bpe-render/src/frame.rs`).
  - `pub struct ShapeInstance { pub a: [f32; 4], pub b: [f32; 4], pub c: [f32; 4], pub color: [f32; 4] }` with `pub fn outline(outline: &Outline, color: [f32; 4], glow: f32) -> Self` (ported; kinds 0 ring/polygon, 1 box).
  - `pub struct FlashInstance { pub center: [f32; 2], pub size: f32, pub color: [f32; 4] }`.
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Frame { pub width: u32, pub height: u32, pub time: f32, pub background_top: [f32; 4], pub background_bottom: [f32; 4], pub bloom: f32, pub exposure: f32, pub shapes: Vec<ShapeInstance>, pub balls: Vec<BallInstance>, pub flashes: Vec<FlashInstance> }` with `clear(&mut self)` (keeps the globals, empties the lists).
  - `pub struct DrawCx<'a> { pub thing: &'a Thing, pub resolved: &'a ResolvedThing, pub world: &'a World, pub log: &'a Log, pub time: f64 }`
  - `pub trait ThingDraw: Send + Sync + 'static { fn kind(&self) -> &'static str; fn draw(&self, cx: &DrawCx, frame: &mut Frame); }` and `#[derive(Default)] pub struct Draws` with `add(&mut self, d: impl ThingDraw)` and `thing(&self, kind) -> Option<&dyn ThingDraw>`.
  - `pub fn build_frame(scene: &Scene, resolved: &Resolved, world: &World, log: &Log, time: f64, draws: &Draws, frame: &mut Frame)`: clears the lists, then every resolved thing in id order whose kind has a draw.
  - `pub struct Renderer` with `new(device, width, height)`, `size`, `resize`, `output_texture`, `output_view`, `pixel_size(&self, frame: &Frame) -> f32`, `render(&mut self, device, queue, frame: &Frame)`, `read_output(&self, device, queue) -> anyhow::Result<Vec<u8>>`; `pub struct Readback` and `pub fn headless_device()` ported unchanged. `Globals { view: [2/w, 2/h, pixel, time], bg_top, bg_bottom, post: [bloom, exposure, 0, 0] }`. Passes: background, shapes, balls, flashes into HDR; six bloom levels; composite. The classic ball shader reads its halo strength from the instance (`glow`), flashes carry their strength in their colour.
- Produces in `bpe-kit`:
  - `SceneDraw`: sets the frame's `width`, `height`, backgrounds (`to_linear`), `bloom`, `exposure`, `time`.
  - `look::draw::Look` helper: `pub fn colors(look: &ClassicLook, count: usize, row_of: impl Fn(usize) -> f32) -> impl Fn(usize) -> [f32; 4]`: the gradient sampled by `row_of(i)` in 0..=1 when `gradient` is non-empty, else `color`.
  - `ArenaDraw`: one `ShapeInstance::outline` of `world.walls[id].outline` in the Look's colour with glow `0.015 × glow / 0.12` (the old wall glow at the default ball glow), and nothing when the Look slot is missing or Off (white, glow 0 when the module is missing but the slot is present: `ResolvedThing.slots` has no entry, so draw white).
  - `BallsDraw`: a `BallInstance` per alive body with colour from `colors` (block rows top to bottom give `row_of`; ring and single give 0); flashes for `log.since(now − flash_seconds)` Hit events whose `a.thing == id`: `intensity = min(speed / 600, 1.5) × exp(−3 age / flash_seconds) × 0.35 × flash / 0.4`, colour `(c + 1) / 2 × intensity` as before, size `flash_size`, skipped under 0.002.
  - `tests/pictures.rs`: `headless_device()` or skip with a message; render each stage scene at 270×480 at 2.0 s; assert the pixel count, that some pixel is brighter than 100, and that `Reel block` has more bright pixels than `One ball`; save `target/pictures/<name>.png` when `BPE_SAVE_RENDERS` is set (png crate).

- [ ] **Step 1: Write failing tests**: `tests/pictures.rs` as described, `frame.rs` unit test `build_frame_fills_from_each_thing` (a fake `Draws` with one draw per kind, asserting the frame's lists and globals), and in `bpe-kit/src/things/balls/draw.rs`:

```rust
#[test]
fn a_missing_look_module_still_draws() {
    let reg = registry();
    let mut scene = new_scene(&reg);
    // Point the ball at a module that doesn't exist.
    scene.things.get_mut(&ThingId(2)).unwrap().slots.insert(SlotId::Look, SlotLink::Linked { module: ModuleId(99), overrides: Params::default() });
    let resolved = bpe_core::resolve(&scene, &reg);
    assert!(!resolved.problems.is_empty());
    let kit = kit();
    let world = World::build(&scene, &resolved, &kit.sims);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
    assert_eq!(frame.balls.len(), 1);
    assert_eq!(frame.shapes.len(), 1);
    assert_eq!(frame.balls[0].color, [1.0, 1.0, 1.0, 1.0]); // falls back to the root's Plain look
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-gfx -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement** `bpe-gfx` (port `Renderer::new`, `render`, `read_output`, `Targets`, `GrowableBuffer`, `begin_pass`, `Readback`, `headless_device` from `bpe-render/src/lib.rs`; trim `scene.wgsl` to background, `vs_shape`/`fs_shape` for kinds 0 and 1, `vs_ball`/`fs_ball` classic path with `in.glow`, `vs_flash`/`fs_flash` with strength 1; keep `post.wgsl`) and the kit draw parts.
- [ ] **Step 4: Run** `BPE_SAVE_RENDERS=1 cargo test -p bpe-gfx -p bpe-kit` and look at `target/pictures/*.png`. Expected: PASS; a ring with balls in each picture.
- [ ] **Step 5: Commit** `git commit -m "Draw arenas and balls with the new renderer"`

---

### Task 8: The mixer, instruments and the kit's sound part

**Files:**
- Create: `crates/bpe-mix/src/{lib,chords,level}.rs`, `src/instruments/{dsp,synth}.rs`, `crates/bpe-kit/src/modules/sound/mix.rs`
- Modify: `crates/bpe-mix/src/instruments/mod.rs`, `crates/bpe-kit/src/lib.rs` (`pub fn mixes() -> Mixes`, `pub fn mix_scene(scene, resolved, log, kit) -> Soundtrack`)

**Interfaces:**
- Produces in `bpe-mix`:
  - `pub const SAMPLE_RATE: u32 = 48_000`; `pub struct Soundtrack { pub samples: Vec<f32> }` (interleaved stereo) with `silent(seconds)`, `frames()`, `write_wav(path)` (ported).
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Choice)] pub enum Instrument { #[default] Pluck, ElectricPiano, GrandPiano, Marimba, Vibraphone, Glass, Harp, Bells, Kalimba, Supersaw, Pad, SynthBass }` with `pub fn label(self) -> &'static str` ("Electric piano"...) and `pub fn render(self, key: u8, velocity: f32, decay: f32, brightness: f32) -> Vec<f32>` (pluck uses `decay` and `brightness`; others ported from `Builtin::render`).
  - `pub struct NoteCache` (`BTreeMap<(Instrument, u8, u8, u32, u32), Arc<Vec<f32>>>` keyed by instrument, key, velocity bucket of 8, decay bits, brightness bits) with `get(&mut self, ...)`; rendering missing notes on all cores with a scoped thread pool as `Voice::prepare` did.
  - `pub struct Chords` with `pub fn parse(text: &str, low: u8, high: u8) -> Chords` (steps `"<chord> <seconds>"` separated by commas; a step whose chord name is unknown or whose seconds don't parse is skipped; no steps at all gives one 1 s step of C major notes) and `notes_at(&self, time: f64) -> &[u8]`; `pub fn pitch_classes(name: &str) -> Option<Vec<u8>>` ported.
  - `#[derive(Clone, Debug, PartialEq)] pub struct Note { pub frame: usize, pub instrument: Instrument, pub key: u8, pub velocity: f32, pub left: f32, pub right: f32, pub decay: f32, pub brightness: f32 }`
  - `pub struct MixSettings { pub duration: f32, pub steps_per_second: u32, pub chords: String, pub loudness: f32, pub seed: u64, pub half_width: f32 }`
  - `pub struct MixCx<'a> { pub settings: &'a MixSettings, pub chords: &'a Chords, pub frames: usize, pub hit_index: &'a dyn Fn(&Event) -> u64 }` (`hit_index` is the event's position among all hits in the log, for repeatable note picks).
  - `pub trait ModuleMix: Send + Sync + 'static { fn kind(&self) -> &'static str; fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>); }` and `#[derive(Default)] pub struct Mixes` with `add`, `module(kind)`.
  - `pub fn mix(settings: &MixSettings, resolved: &Resolved, log: &Log, mixes: &Mixes) -> Soundtrack`: a silent track of `duration`; for each resolved thing in id order with a Sound slot whose kind has a mix, call `notes` with the log's events where `a.thing == id`; render through `NoteCache`; sum with ported `mix_notes` (note order preserved); `normalize_loudness(loudness)`; `limit_peaks` (ceiling 0.89).
  - `level.rs`: `normalize_loudness`, `limit_peaks`, `mix_notes`, `mix64` ported.
- Produces in `bpe-kit`: `NoteSoundMix`: for each Hit event with `speed >= quietest_hit`: `time = event.time(rate)`, `frame = round(time × 48000)` (skip past the end), `candidates = chords.notes_at(time)` restricted to `low_note..=high_note` (the module parses `settings.chords` once with its own range), `pick = mix64(seed ^ hit_index) % len`, `velocity = clamp((speed − quietest) / max(loudest − quietest, 1), 0, 1)`, `gain = velocity × sqrt(velocity) × 10^(volume/20)`, pan `clamp(point.x / half_width, −1, 1) × stereo_width`, `angle = (pan + 1) π/4`, left `gain cos`, right `gain sin`.
  - `pub fn mix_scene(scene: &Scene, resolved: &Resolved, log: &Log, kit: &Kit) -> Soundtrack` builds `MixSettings` from the root's `SceneSettings` (`half_width = width / 2`).

- [ ] **Step 1: Write failing tests**

```rust
// bpe-mix/src/chords.rs
#[test]
fn chords_parse_and_skip_what_they_dont_know() {
    let c = Chords::parse("C 2, Xy 3, Am 1", 60, 72);
    assert_eq!(c.notes_at(0.5), &[60, 64, 67, 72]);
    assert_eq!(c.notes_at(2.5), &[60, 64, 69, 72]);
    assert_eq!(c.notes_at(3.5), &[60, 64, 67, 72]); // the cycle is 3 s
    assert_eq!(Chords::parse("", 60, 62).notes_at(0.0), &[60, 62]);
}

// bpe-kit/src/modules/sound/mix.rs
#[test]
fn a_hit_plays_a_note_at_its_frame_and_silence_stays_silent() {
    let kit = kit();
    let reg = &kit.registry;
    let scene = new_scene(reg);
    let resolved = bpe_core::resolve(&scene, reg);
    let mut log = Log::default();
    log.push(Event { step: 240, kind: EventKind::Hit, a: Party { thing: ThingId(2), item: 0 }, b: Some(Party { thing: ThingId(1), item: 0 }), point: Vec2::new(-540.0, 0.0), normal: Vec2::X, speed: 500.0 });
    let track = mix_scene(&scene, &resolved, &log, &kit);
    assert_eq!(track.frames(), 20 * 48_000);
    let before: f32 = track.samples[..47_990 * 2].iter().map(|s| s.abs()).sum();
    let after: f32 = track.samples[48_000 * 2..48_100 * 2].iter().map(|s| s.abs()).sum();
    assert_eq!(before, 0.0);
    assert!(after > 0.0);
    let (l, r): (f32, f32) = track.samples[48_000 * 2..49_000 * 2].chunks(2).fold((0.0, 0.0), |(l, r), f| (l + f[0].abs(), r + f[1].abs()));
    assert!(l > r, "a hit on the left should sound mostly on the left");
    assert!(track.samples.iter().all(|s| s.abs() <= 0.9));
    assert!(mix_scene(&scene, &resolved, &Log::default(), &kit).samples.iter().all(|&s| s == 0.0));
    assert_eq!(mix_scene(&scene, &resolved, &log, &kit).samples, track.samples);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-mix -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement** `bpe-mix` (port `dsp.rs` and `synth.rs` whole; `Instrument::render` from `Builtin::render` minus the drum kit; `level.rs` from `bpe-audio/src/lib.rs`) and the kit's sound part.
- [ ] **Step 4: Run** `cargo test -p bpe-mix -p bpe-kit && cargo clippy -p bpe-mix -p bpe-gfx -p bpe-kit -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Mix the event log into notes with the ported instruments"`

---

### Task 9: Export pipeline and the command line

**Files:**
- Create: `crates/bpe-cli/Cargo.toml`, `src/lib.rs`, `src/main.rs`, `tests/export.rs`

**Interfaces:**
- Produces:
  - `pub struct ExportOptions { pub output: PathBuf, pub ffmpeg: PathBuf, pub crf: u8, pub preset: String, pub scale: f32, pub max_frames: Option<u32>, pub encoder: Encoder }` with `new(output)` (ffmpeg from `find_ffmpeg()` or `"ffmpeg"`, crf 18, preset "medium", scale 1, encoder Auto).
  - `pub enum Encoder { Auto, Cpu, Nvidia, Amd, Intel }` with `name`, `from_name`, and the ffmpeg args, `gpu_encoders`, `resolve_encoder` ported from `bpe-export`.
  - `pub fn find_ffmpeg() -> Option<PathBuf>` and `pub fn output_size(width: u32, height: u32, scale: f32) -> (u32, u32)` ported.
  - `pub fn export_video(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, kit: &Kit, options: &ExportOptions, progress: impl FnMut(u32, u32) -> bool) -> anyhow::Result<Encoder>`: resolve; `Playback` run to the end for the log; `mix_scene` to a temp wav; spawn ffmpeg with the same arguments as `bpe-export` (rawvideo rgba in, wav in, chosen encoder, aac 192k 48 kHz, faststart, `-shortest`); render each frame at `index / fps` with `build_frame` and `Readback` three deep, feeding ffmpeg from a writer thread; cancel deletes the file and fails with `export cancelled`; a GPU encoder failure retries on the CPU.
  - `main.rs`: `bpe-render-next <scene.ron> [output.mp4] [--scale F] [--frames N] [--encoder auto|cpu|nvidia|amd|intel] [--crf N] [--preset NAME]`; output defaults to the scene file's name with `.mp4`; prints progress every 60 frames and the encoder used; exits 1 with the error on failure. `bpe-render-next --new output.mp4` exports the new scene without a file.
  - `tests/export.rs`: skip unless `headless_device()` works and `find_ffmpeg()` is some. `exports_a_short_clip_with_sound`: `One ball` scene, `max_frames 30`, `scale 0.25`, into a temp dir; then `ffprobe -v error -show_entries stream=codec_type,width,height:format=duration -of csv` must list a `video` stream 270×480 and an `audio` stream, and the duration must be within 0.1 s of 0.5. `export_without_ffmpeg_fails_clearly_and_leaves_no_file`: `ffmpeg: "/no/such/ffmpeg"` → error text starts with `could not start ffmpeg` and the output path doesn't exist. `a_cancelled_export_leaves_no_file`: progress returns false on the fifth frame → error contains `export cancelled`, no file.

- [ ] **Step 1: Write failing tests** in `tests/export.rs` as described.
- [ ] **Step 2: Run** `cargo test -p bpe-cli`. Expected: FAIL.
- [ ] **Step 3: Implement** `bpe-cli`.
- [ ] **Step 4: Run** `cargo test -p bpe-cli` and `cargo run -p bpe-cli --release -- scenes/next/reel-block.ron /tmp/reel.mp4 --scale 0.5 --frames 300`. Expected: tests PASS; a 5 s clip plays with plucks.
- [ ] **Step 5: Commit** `git commit -m "Export the new scenes to mp4 from the command line"`

---

### Task 10: Editor shell: window, state, files, undo

**Files:**
- Create: `crates/bpe-studio/Cargo.toml`, `src/main.rs`, `src/app.rs`, `src/history.rs`, `src/theme.rs`, `src/settings.rs`

**Interfaces:**
- Produces:
  - `main.rs`: eframe wgpu window titled `"Ball Studio Next"`, 1440×960, min 1000×640, opening the scene file given on the command line, else the last one, else `new_scene`. `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`.
  - `pub struct Studio { pub kit: Arc<Kit>, pub scene: Scene, pub resolved: Resolved, pub saved: Scene, pub path: Option<PathBuf>, pub playback: Playback, pub fingerprint: u64, pub time: f64, pub playing: bool, pub looping: bool, pub muted: bool, pub selected: Option<ThingId>, pub history: History, pub status: String, pub confirm: Option<Pending>, pub panels: Panels /* tree, inspector, preview, transport, export, sound job: added by Tasks 11-13 */ }`
  - `impl Studio`: `new(cc, path)`, `after_edit(&mut self, before: Scene)` (records history unless unchanged, re-resolves, rebuilds `playback` when `bpe_runtime::fingerprint` changed, marks the sound dirty, clamps `time` to the duration), `replace_scene(&mut self, scene, path)`, `undo`, `redo`, `open` (rfd; on `LoadError` sets `status` to its text), `save(&mut self, choose: bool) -> bool` (rfd, `bpe_core::save`, remembers the path), `new_scene`, `unsaved(&self) -> bool`, shortcuts Ctrl+N/O/S/Shift+S/Z/Y, Space play, Home start, Delete removes the selected thing.
  - `history.rs`: `pub struct History { undo: Vec<Scene>, redo: Vec<Scene>, merging: bool, last_edit: Option<Instant> }` with `record(&mut self, before: Scene, separate: bool)` (merge edits within 600 ms while the pointer is down, limit 200, ported), `end_merging`, `undo(&mut self, current: &mut Scene) -> bool`, `redo`.
  - Frame layout (spec 4.1) with `egui::Panel`: top toolbar (New, Open…, Save, Save as…, Undo, Redo, scene name edit, saved state text, Export video… on the right), left `tree` panel (default 260, resizable), right `inspector` panel (default 420), bottom `transport` (default 110), bottom status line, central preview. Tasks 11-13 fill the panels; until then they show their names.
  - `confirm` dialog "Save changes?" before New, Open and closing (ported `Pending::{Close, Open, Replace}`).

- [ ] **Step 1: Write failing tests** in `history.rs` and `app.rs`

```rust
#[test]
fn quick_edits_merge_and_separate_edits_dont() {
    let mut h = History::default();
    let (a, b, c) = (scene_named("a"), scene_named("b"), scene_named("c"));
    h.record(a.clone(), false);
    h.record(b.clone(), false); // still merging: dropped
    assert_eq!(h.undo.len(), 1);
    h.record(c.clone(), true);
    assert_eq!(h.undo.len(), 2);
    let mut current = scene_named("d");
    assert!(h.undo(&mut current) && current == c);
    assert!(h.redo(&mut current) && current.name == "d");
}

#[test]
fn opening_an_old_scene_reports_it_and_keeps_the_current_one() {
    let mut studio = Studio::headless();
    let old = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/reference-reel.ron");
    studio.open_path(Path::new(old));
    assert_eq!(studio.status, "This scene was made with the old Ball Studio");
    assert_eq!(studio.scene.things.len(), 3);
}

#[test]
fn a_look_edit_keeps_the_playback_and_a_gravity_edit_restarts_it() {
    let mut studio = Studio::headless();
    studio.seek(2.0);
    let before = studio.scene.clone();
    let look = Address { owner: Owner::Module(ModuleId(1)), slot: None, key: "glow".into() };
    studio.scene.set_at(&studio.kit.registry, &look, Value::Number(1.0)).unwrap();
    studio.after_edit(before);
    assert!(studio.playback.time() >= 2.0);
    let before = studio.scene.clone();
    let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
    studio.scene.set_at(&studio.kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
    studio.after_edit(before);
    assert_eq!(studio.playback.time(), 0.0);
    assert_eq!(studio.history.undo.len(), 2);
}
```

`Studio::headless()` builds the state without a window (no audio, no renderer), for tests.

- [ ] **Step 2: Run** `cargo test -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement** the shell. Port `theme.rs` whole and `settings.rs` with the file name `last-scene-next.txt`.
- [ ] **Step 4: Run** `cargo test -p bpe-studio` and `cargo run -p bpe-studio` under `xvfb-run` for a smoke start (it must open and close without panicking: `timeout 10 xvfb-run -a cargo run -p bpe-studio; test $? -eq 124`).
- [ ] **Step 5: Commit** `git commit -m "Add the new editor's shell with files and undo"`

---

### Task 11: Scene tree

**Files:**
- Create: `crates/bpe-studio/src/tree.rs`

**Interfaces:**
- Produces: `pub struct Tree { pub filter: String, pub renaming: Option<ThingId>, pub add_open: bool }` and `pub fn show(ui: &mut Ui, tree: &mut Tree, scene: &mut Scene, kit: &Kit, resolved: &Resolved, selected: &mut Option<ThingId>) -> TreeOutcome` where `pub struct TreeOutcome { pub edited: bool, pub removed: Option<ThingId> }`.
  - Rows in `scene.order`, indented by depth, Scene defaults first. Each row: a painted icon (a ring for arenas, a disc for balls, a square for the scene), the name, the tags as small text, an eye toggle for `enabled`, a red dot when `resolved.problems` names the thing (hover lists them in plain words: "Look links a module that is missing (id 99)"). Clicking selects. Double-click or the right-click menu's Rename edits the name in place (Enter or focus loss ends it). Right-click: Rename, Duplicate (copies params, slots and transform under the same parent with " copy" added to the name), Delete (not on the root). The filter box hides rows whose name, kind and tags don't contain the text.
  - `+ Add thing` at the bottom opens a menu listing `kit.registry.things()` except `"scene"`, each with its name and `help()`; a pick calls `scene.add_thing(reg, kind, parent)` with the selected thing's parent (or the root) and selects the new thing. New Balls and Arena things link nothing (they inherit).

- [ ] **Step 1: Write failing kittest tests** in `tree.rs` (harness around `show` with a `State { tree, scene, resolved, selected }`, `with_size(260, 600)`, `allow_missing_glyphs`):

```rust
#[test]
fn adding_a_thing_lists_and_selects_it() {
    let mut h = harness();
    h.get_by_label("+ Add thing").click();
    h.run();
    h.get_by_label("Balls").click();
    h.run();
    let s = h.state();
    assert_eq!(s.scene.things.len(), 4);
    assert_eq!(s.selected, Some(ThingId(3)));
    assert!(h.query_by_label("Balls 2").is_some());
}

#[test]
fn delete_removes_the_selected_thing_but_never_the_root() {
    let mut h = harness();
    h.state_mut().selected = Some(ThingId(2));
    h.get_by_label("Ball").click();
    h.run();
    h.press_keys(Modifiers::NONE, Key::Delete);
    h.run();
    assert!(!h.state().scene.things.contains_key(&ThingId(2)));
    h.state_mut().selected = Some(ThingId::ROOT);
    h.press_keys(Modifiers::NONE, Key::Delete);
    h.run();
    assert!(h.state().scene.things.contains_key(&ThingId::ROOT));
}

#[test]
fn the_filter_hides_other_rows() {
    let mut h = harness();
    h.get_by_label("Filter").type_text("are");
    h.run();
    assert!(h.query_by_label("Arena").is_some());
    assert!(h.query_by_label("Ball").is_none());
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-studio tree`. Expected: FAIL.
- [ ] **Step 3: Implement** `tree.rs` and wire it into the left panel; `Studio` handles `removed` (clears the selection) and `edited` (through `after_edit`).
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add the scene tree with add, rename, delete and filter"`

---

### Task 12: Inspector and setting widgets

**Files:**
- Create: `crates/bpe-studio/src/widgets.rs`, `crates/bpe-studio/src/inspector.rs`

**Interfaces:**
- Produces in `widgets.rs`:
  - `pub fn setting(ui: &mut Ui, def: &SettingDef, value: &mut Value, inherited: Option<&Value>) -> bool` (true when changed): one grid row with the label (help on hover; double-click or Alt-click resets to `inherited` or the def's default, ported `resettable`) and the widget by `def.ty`: Number → `Slider` over `range` with `unit` as suffix (log scale when the range spans more than 1000×), Int → integer slider, Bool → checkbox, Vec2 → two `DragValue`s, Color → `color_edit_button_srgba` with additive support (ported `picker_color`/`scene_color`), Colors → the ported colour list (add, remove, drag not needed), Choice → `ComboBox` showing `pretty(name)` (`"electric_piano"` → `"Electric piano"`), Text → `TextEdit::singleline`, Target → a label (stage 3).
  - `pub fn pretty(name: &str) -> String`.
  - `pub fn settings_grid(ui, id: &str, defs: &[SettingDef], depth: Depth, params: &mut Params, get: impl Fn(&str) -> Value) -> bool`: rows for every def at `Simple` depth, or all defs grouped under `def.group` headings at `More`; the shown value comes from `get(key)` (the effective value) and a change writes the sparse value back (removing the key when it equals the default).
- Produces in `inspector.rs`:
  - `pub struct Inspector { pub depth: BTreeMap<(ThingId, Option<SlotId>), Depth> }` and `pub fn show(ui, inspector: &mut Inspector, scene: &mut Scene, kit: &Kit, resolved: &Resolved, selected: Option<ThingId>) -> bool` (true when the scene changed).
  - Header: name edit, kind name and help, position and rotation (`DragValue`s on `transform`; hidden for the root), `enabled` checkbox.
  - Kind settings card with a `Simple | More` switch (`Nodes` shown disabled with hover "Nodes come in a later stage"), rows through `settings_grid` writing `scene.set_at(reg, Address { owner: Thing(id), slot: None, key }, value)`.
  - One slot card per entry of `thing.slots`, in `SlotId::ALL` order. Top row: slot name; module picker `ComboBox`: the current state's label (`"<module name>"` plus `" · shared ×N"` when `users_of` > 1; `"from <thing name>"` for an inherited slot, naming `ResolvedSlot.from`; `"Off"`), with entries for every library module of that slot, `"New <slot> module…"` (adds a module of the slot's first kind named `"<thing name> <slot>"` and links it; when the card showed a module, copies that module's params first), `"Inherit"`, `"Off"`; the depth switch; a red badge with the problem text when `resolved.problems` names this thing and slot. Rows: the resolved module's defs through `settings_grid`, writing `set_at(Address { owner: Module(module), slot: None, key })` (this stage edits the shared module; overrides are stage 3); nothing for `Off`.
  - `+ Add slot` menu listing `kind.slots()` not yet in `thing.slots` that have at least one module kind registered, each adding the slot as `Inherit`.
  - Scene defaults shows the same inspector (spec 4.3).

- [ ] **Step 1: Write failing kittest tests** in `inspector.rs` (harness around `show` with the new scene, `with_size(420, 1600)`):

```rust
#[test]
fn more_shows_more_rows_than_simple() {
    let mut h = harness(Some(ThingId(2)));
    let simple = h.get_all_by_role(Role::Slider).count();
    h.get_by_label("More").click();
    h.run();
    assert!(h.get_all_by_role(Role::Slider).count() > simple);
}

#[test]
fn editing_a_slider_writes_through_set_at() {
    let mut h = harness(Some(ThingId(2)));
    let rect = slider_rect(&h, "Size");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let v = s.scene.things[&ThingId(2)].params.get("radius").cloned();
    assert!(matches!(v, Some(Value::Number(r)) if r > 100.0));
}

#[test]
fn the_module_picker_can_switch_a_slot_off_and_back_to_inherit() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Ball look").click();
    h.run();
    h.get_by_label("Off").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Off);
    h.get_by_label("Off").click();
    h.run();
    h.get_by_label("Inherit").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Look], SlotLink::Inherit);
    assert!(h.query_by_label_contains("from Scene defaults").is_some());
}

#[test]
fn a_new_module_copies_the_current_one() {
    let mut h = harness(Some(ThingId(1)));
    h.get_by_label_contains("from Scene defaults").click();
    h.run();
    h.get_by_label("New Look module…").click();
    h.run();
    let s = h.state();
    let SlotLink::Linked { module, .. } = &s.scene.things[&ThingId(1)].slots[&SlotId::Look] else { panic!() };
    assert_eq!(s.scene.library.modules[module].name, "Arena Look");
    assert_eq!(s.scene.library.modules[module].kind, "look.classic");
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-studio inspector`. Expected: FAIL.
- [ ] **Step 3: Implement** `widgets.rs` and `inspector.rs`, wire into the right panel through `after_edit`.
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "Add the inspector with slot cards and schema-driven settings"`

---

### Task 13: Preview, transport, sound and export window

**Files:**
- Create: `crates/bpe-studio/src/preview.rs`, `src/transport.rs`, `src/audio_out.rs`, `src/sound_job.rs`, `src/export_window.rs`

**Interfaces:**
- `preview.rs`: `pub struct Preview { renderer: Renderer, texture: TextureId, frame: Frame, drag: Option<Drag> }` and `pub fn show(slot: &mut Option<Preview>, ui, eframe_frame, studio parts...) -> Outcome { catching_up: bool, selected: Option<ThingId>, edited: bool, clicked: bool }`: renders at the shown size (never above the video size) through `build_frame` + `Renderer::render`, registered as an egui native texture (ported from `bpe-app/src/preview.rs`); `playback.seek_within(time, 12 ms)`; while paused: hit-test things (Balls: within the body's radius × 1.5 of any item at step 0, read from a fresh `World::build` when `time == 0`, else the current bodies; Arena: within 12 px of the outline), drag a thing to move `transform.position`, drag a Balls thing's arrow tip (drawn from its position along `velocity / 4`) to set `velocity`, drag an Arena's edge to set `radius` (or `width`/`height` for a box); a drag seeks to 0 first; a click selects; a click while playing pauses. Handles drawn with the theme's accent; a hint line under the preview.
- `transport.rs`: `pub fn show(ui, studio...)`: round play/pause button, time `m:ss.ff`, a scrub bar across the duration with the hit lane (one tick per Hit event in the log, from `playback.log()`), loop toggle, mute toggle, "Restart" (Home). Scrubbing pauses and resumes after.
- `audio_out.rs`: ported `AudioOut` (cpal) unchanged apart from the sample rate constant.
- `sound_job.rs`: `pub struct SoundJob` mixing on a thread 250 ms after the last edit that changed the sound fingerprint (physics fingerprint ⊕ FNV of every Sound slot's kind and params ⊕ root `chords`, `loudness`, `seed`, `duration`, `width`); the result sets `AudioOut`'s track and the transport's hit lane comes from the playback's own log (so it is right even before the mix lands).
- `export_window.rs`: a window with output path (rfd save dialog, default `<scene name>.mp4` beside the scene file), quality (crf slider 14..=28), draft scale (100/50/25 %), encoder combo (`gpu_encoders` probed once), Export button; progress bar and Cancel while running on a thread with its own `headless_device()`; done message with the file name; errors shown in red.

- [ ] **Step 1: Write failing tests** in `preview.rs` (pure functions, no GPU):

```rust
#[test]
fn hit_testing_finds_balls_before_arenas() {
    let studio = Studio::headless();
    let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
    assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, -200.0))), Some(Handle::Body(ThingId(2))));
    assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(508.0, 0.0))), Some(Handle::Edge(ThingId(1))));
    assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, 300.0))), None);
}

#[test]
fn dragging_moves_aims_and_resizes() {
    let mut studio = Studio::headless();
    apply_drag(&mut studio.scene, Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO }, Vec2::new(50.0, 60.0));
    assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(50.0, 60.0));
    apply_drag(&mut studio.scene, Drag { handle: Handle::Aim(ThingId(2)), offset: Vec2::ZERO }, Vec2::new(150.0, 60.0));
    assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([400.0, 0.0])));
    apply_drag(&mut studio.scene, Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO }, Vec2::new(300.0, 400.0));
    assert_eq!(studio.scene.things[&ThingId(1)].params.get("radius"), Some(&Value::Number(500.0)));
}
```

and in `sound_job.rs`: `a_look_edit_does_not_change_the_sound_fingerprint`, `a_volume_edit_does` (same shape as the playback fingerprint test).

- [ ] **Step 2: Run** `cargo test -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement** the five files and wire them into `Studio` (the central panel, the bottom panel, the toolbar's Export button, `after_edit` marking the sound dirty, `set_playing` driving `AudioOut`).
- [ ] **Step 4: Run** `cargo test -p bpe-studio && cargo clippy -p bpe-studio -p bpe-cli -- -D warnings`. Then take the stage screenshots: `Xvfb :99 -screen 0 1600x1000x24 &`, `DISPLAY=:99 cargo run --release -p bpe-studio -- scenes/next/reel-block.ron &`, wait 8 s, `DISPLAY=:99 xdotool search --name "Ball Studio Next" windowactivate key space`, wait 3 s, `DISPLAY=:99 import -window root /tmp/stage2-main.png`; then select the ball row (`xdotool mousemove` onto the tree row and click) and shoot `/tmp/stage2-inspector.png`. Expected: the window shows tree, preview with the block of balls, inspector and transport.
- [ ] **Step 5: Commit** `git commit -m "Preview, play, hear and export scenes in the new editor"`

---

### Task 14: Speed check, docs, Windows build

**Files:**
- Create: `crates/bpe-kit/tests/speed.rs`
- Modify: `README.md` (a "Ball Studio Next" section: what it is, that it is a work in progress beside the old editor, `cargo run -p bpe-studio`, `bpe-render-next`, the new `scenes/next` files)

**Interfaces:**
- `tests/speed.rs` (`#[ignore]`, run with `--release`): `ten_thousand_balls_step_no_slower_than_the_old_engine`: old: `bpe_scene::Scene::default()` with its grid changed to 100×100 spacing 9 radius 4, `bpe_sim::Playback::seek(5.0)`; new: `Reel block` changed to 100×100 spacing 9 radius 4, `Playback::seek(5.0)`; both timed after a 1 s warm-up seek; prints both; asserts `new <= old * 1.1`. `ten_thousand_balls_build_a_frame_no_slower`: `FrameData::fill` versus `build_frame` at 5 s, same bound.

- [ ] **Step 1: Write the speed tests.**
- [ ] **Step 2: Run** `cargo test --release -p bpe-kit --test speed -- --ignored --nocapture`. Expected: PASS; note the numbers for the PR.
- [ ] **Step 3: Write the README section.**
- [ ] **Step 4: Run** the whole workspace in the background: `cargo test --workspace --no-fail-fast` and `cargo clippy --workspace -- -D warnings`. Expected: all pass. Then the Windows build: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`; copy `ball-studio-next.exe` and `bpe-render-next.exe`, the five `scenes/next/*.ron`, the two screenshots and a sample mp4 (`Reel block` at full size, 23 s, from `bpe-render-next` on Linux) to `/mnt/project-files/ballphysics/modular-revamp/stage2/`.
- [ ] **Step 5: Commit** `git commit -m "Check the new engine's speed and describe Ball Studio Next"`

---

## Done when

- `cargo test --workspace` passes with every old test unchanged; `cargo clippy --workspace -- -D warnings` is clean.
- `golden.txt` holds five scenes; running twice and seeking give the same hashes.
- `bpe-render-next scenes/next/reel-block.ron` makes an mp4 with sound, and the export check test passes.
- `ball-studio-next` opens with a circle arena and one ball, lets you add things, edit Simple and More settings on slot cards, switch modules, drag things in the preview, play with sound, undo, save, open, and export.
- A Windows `ball-studio-next.exe`, `bpe-render-next.exe`, the stage scenes, screenshots and a sample mp4 are in `/mnt/project-files/ballphysics/modular-revamp/stage2/`.
- Left for later stages (spec 7): obstacles, folders, attaching, tags, the Library window, overrides and multi-select (3); Nodes, Rules (4); `look.styled`, Trail, Particles, Shader slots (5); emitters, liquid, swarm, fields (6); soft bodies, text, timeline keyframes, song and beat sync (7).
