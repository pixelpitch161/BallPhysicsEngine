# Rules and Nodes (Stage 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every module a Nodes depth (a node panel under the preview, template graphs with a native fast path, custom modules that run through a graph VM) and add the Rules slot: per-thing rules driven by the event log that target other things by id, tag or kind and grow, split, tint, spawn, remove, kick, set settings, play notes and fire events.

**Architecture:** A new `bpe-vm` crate (ported from `bpe-sim/src/rules.rs`) compiles a `bpe_graph::Graph` for one of four CPU roles (Rules, Bounce, Motion, Sound) into a `Program` and runs it: pull-based values, push-based triggers, a `Host` trait for what the world answers. `bpe-runtime` gains an action queue applied at the start of the next step, a Rules stage, ball tints and graph-aware module hooks (all additive default methods). `bpe-kit` adds the `rules.graph` module, template graphs for bounce/motion/sound with a compiled fallback path when a module is custom, and a `resolve_scene` that marks modules whose graph does not compile. `bpe-studio` ports the old node editor into a bottom panel that slides up under the preview, enables the Nodes switch, and shows a custom module's exposed inputs (and a Rules module's sentences) back on the Simple/More cards.

**Tech Stack:** Rust 2024, egui/eframe 0.37 + egui_kittest, bpe-graph (untouched), glam, ron, BTreeMap/BTreeSet everywhere.

**Spec:** `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (sections 3.5, 3.6, 4.3, 4.6, 5.3, 5.5, 5.6, 6, 8). Stage 1 plan `2026-10-29-modular-core.md` and stage 2 plan `2026-10-29-first-video.md` define the crates this builds on.

## Global Constraints

- Old crates (`bpe-app`, `bpe-sim`, `bpe-render`, `bpe-audio`, `bpe-scene`, `bpe-export`) stay untouched. `bpe-graph` stays untouched: graph meta lives on `bpe_core::ModuleGraph`.
- Dependencies point one way: `core` ← `vm`, `runtime`, `gfx`, `mix` ← `kit` ← `studio`, `cli`. `bpe-vm` depends on `bpe-core`, `bpe-graph` and `glam` only.
- `BTreeMap`/`BTreeSet` everywhere; iteration in `ThingId` order; two runs of a scene give the same event log hash.
- Stages 3, 5, 6, 7 run in parallel threads: changes to shared files (`world.rs`, `plugins.rs`, `inspector.rs`, `app.rs`, `kit/lib.rs`, `resolve.rs`) are additive (new fields with defaults, new default trait methods, new match arms, new call sites), never rewrites. New code goes in new folders: `crates/bpe-vm`, `crates/bpe-kit/src/modules/rules`, `crates/bpe-kit/src/graphs.rs`, `crates/bpe-studio/src/nodes`, `crates/bpe-runtime/src/actions.rs`.
- Spec 5.3: rule actions are queued and all apply at the start of the next step, in the order queued. Spec 5.5: nothing that reads the log writes back.
- Spec 5.6: a graph that fails to compile shows a red badge on the slot card and the tree row (hover shows the compiler message); the thing falls back to its parent's module for that slot; the preview keeps playing.
- Spec 3.5: an unedited module runs on its native fast path; editing marks it custom and it runs through the compiled graph; "Revert to template" goes back. Spec 8: a template run through the compiled path matches the native path (event log hash for Bounce/Motion, samples for Sound).
- Spec 3.6: targets are `Target { Me, Thing(ThingId), Tag(String), Kind(KindId), Anything }` stored by id. In a graph a target is a `Param::Text` in the grammar `me` | `anything` | `thing:<id>` | `tag:<name>` | `kind:<id>`.
- Spec 4.6: the node panel title shows the path, "Red balls › Rules". Blocks offered depend on the slot's role.
- Editor copy: plain words, no jargon; every control labelled (`labelled_by`); side panels call `ui.set_min_width(ui.available_width())`.
- Windows build: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`; the exe stays `ball-studio-next.exe`.

## Review Focus

1. A rule that spawns or splits on every hit must not run away: `MAX_BALLS = 4096` per thing and `ACTION_BUDGET = 256` actions per module per step cap it (Task 4 test `spawning_stops_at_the_ball_cap`, Task 3 test `a_run_stops_at_the_action_budget`).
2. A custom graph that divides by zero or takes a negative square root must leave every ball finite and keep the scene playing (Task 2 test `bad_maths_gives_zero_not_nan`, Task 4 test `a_non_finite_action_is_ignored`).
3. A graph that fails to compile (a block from a newer version, a loop, a type mismatch) shows the badge and falls back to the parent's module while the preview keeps playing (Task 6 test `a_broken_module_falls_back_to_the_parent`, Task 12 test `a_broken_graph_shows_a_red_badge`).
4. A rule targeting a thing that was removed matches nothing and never panics; the picker says the thing is missing (Task 5 test `a_missing_target_matches_nothing`, Task 11 test `the_target_picker_names_a_missing_thing`).
5. Seeking back after rules spawned balls and tinted them replays identically because checkpoints hold the VM state, the pending actions and the tints (Task 10 golden test `seeking_replays_the_stage4_scenes_identically`).

---

## File Structure

**Create**
- `crates/bpe-vm/Cargo.toml`, `crates/bpe-vm/src/lib.rs`: crate root, re-exports, `Role`.
- `crates/bpe-vm/src/val.rs`: `Val`, `Ball`, `BallInfo`.
- `crates/bpe-vm/src/target.rs`: target text grammar.
- `crates/bpe-vm/src/catalog.rs`: every block spec, per role.
- `crates/bpe-vm/src/program.rs`: `Program`, `Block`, `compile`.
- `crates/bpe-vm/src/host.rs`: `Host`, `NoHost`.
- `crates/bpe-vm/src/run.rs`: `Vm`, `Happening`, `Trigger`, `Act`, `Outputs`, evaluation.
- `crates/bpe-vm/src/sentences.rs`: rule chains as lists of node ids, for the inspector.
- `crates/bpe-runtime/src/actions.rs`: `Action`, `MAX_BALLS`, `World::apply`.
- `crates/bpe-kit/src/graphs.rs`: roles per slot, binding templates to params, custom detection, compile, broken-module problems, `resolve_scene`, `WorldHost`.
- `crates/bpe-kit/src/modules/rules/{mod.rs, sim.rs, recipes.rs}`: the Rules module kind, its sim and its recipes.
- `crates/bpe-kit/src/modules/bounce/graph.rs`, `modules/motion/graph.rs`, `modules/sound/graph.rs`: the template graphs and the compiled paths.
- `crates/bpe-studio/src/nodes/{mod.rs, editor.rs, palette.rs, draw.rs, params.rs, cards.rs, tests.rs}`: the node panel, the ported editor, parameter controls with the target picker, and the inspector-side cards.
- `scenes/next/stage4-*.ron`: five stage scenes.

**Modify (additive)**
- `Cargo.toml`: workspace member dep `bpe-vm`.
- `crates/bpe-core/src/resolve.rs`: `ProblemKind::BrokenGraph(String)`, `resolve_with`.
- `crates/bpe-runtime/src/{bodies.rs, plugins.rs, world.rs, lib.rs}`: tint column, hooks, Rules stage, scope, seed, fingerprint.
- `crates/bpe-mix/src/lib.rs`: `ModuleMix::notes_with`, `mix_with`.
- `crates/bpe-kit/src/{lib.rs, scenes.rs, modules/mod.rs, modules/bounce/{mod,sim}.rs, modules/motion/{mod,sim}.rs, modules/sound/{mod,mix}.rs, things/balls/draw.rs, things/scene/sim.rs}`, `crates/bpe-kit/tests/golden.rs`, `scenes/next/golden*.txt`.
- `crates/bpe-cli/src/lib.rs`: `resolve_scene`.
- `crates/bpe-studio/src/{main.rs, app.rs, inspector.rs, tree.rs}` and `Cargo.toml`.

---

### Task 1: bpe-vm crate: values, targets, catalog and compiler

**Files:**
- Create: `crates/bpe-vm/Cargo.toml`, `crates/bpe-vm/src/{lib.rs, val.rs, target.rs, catalog.rs, program.rs}`
- Modify: `Cargo.toml` (workspace `[workspace.dependencies] bpe-vm = { path = "crates/bpe-vm" }`)
- Test: inline `#[cfg(test)]` in each file

**Interfaces:**
- Consumes: `bpe_graph::{Graph, Node, Param, PinType, PinSpec, ParamSpec, NodeSpec, SpecList, Catalog, Diagnostic, validate, order}`, `bpe_core::{Target, ThingId, SlotId}`.
- Produces:
  - `pub enum Role { Rules, Bounce, Motion, Sound }` with `Role::of_slot(slot: SlotId) -> Option<Role>` (Rules→Rules, Bounce→Bounce, Motion→Motion, Sound→Sound, else None), `Role::name(self) -> &'static str` ("Rules", "Bounce", "Motion", "Sound"), `Role::inputs(self) -> &'static [(&'static str, PinType)]` and `Role::outputs(self) -> &'static [(&'static str, PinType)]` (the in-block's output pins and the out-block's input pins, in order, empty for Rules).
  - `pub fn catalog(role: Role) -> &'static SpecList` (one `OnceLock` per role).
  - `pub enum Val { Number(f32), Vector(Vec2), Color([f32; 4]), Bool(bool), Ball(Option<Ball>) }` with `Val::number(&self) -> f32` (Bool→0/1, Vector→length, else 0), `vector()`, `boolean()` (Number≠0, Ball→is some), `color()`, `ball()`, `Val::pin_type(&self) -> PinType`.
  - `pub struct Ball { pub thing: ThingId, pub item: u32 }` (Copy, Ord), `pub struct BallInfo { pub position: Vec2, pub velocity: Vec2, pub radius: f32, pub hits: u32 }`.
  - `pub fn target_from_text(text: &str) -> Target` (unknown text → `Target::Anything`; `thing:x` with a bad number → Anything) and `pub fn target_to_text(t: &Target) -> String`.
  - `pub const SETTING_KINDS: [&str; 3] = ["setting", "setting.color", "setting.bool"]` and `pub fn is_setting(kind: &str) -> bool`.
  - `pub struct Program { pub role: Role, pub blocks: Vec<Block>, pub triggers: Vec<usize>, pub input: Option<usize>, pub output: Option<usize> }`, `pub struct Block { pub id: u32, pub kind: String, pub params: BTreeMap<String, Param>, pub inputs: Vec<Option<(usize, u8)>>, pub then: Vec<Vec<usize>> }` (`then[pin]` = the blocks each Trigger output pin wires into, in node id order; `inputs[pin]` = the block index and output pin wired into each input pin).
  - `pub fn compile(graph: &Graph, role: Role) -> Result<Program, Vec<Diagnostic>>`.
  - `Program::is_empty(&self) -> bool`: no triggers (Rules) or no output block (other roles).

The catalog, by role. Values (every role): `number` (param `value` Number −10000..10000) → `value: Number`; `vector` (param `xy` Vector) → Vector; `color` (param `color` Color) → Color; `random` (inputs `min`, `max` Number; params min 0, max 1) → Number; `time` → Number seconds; `gravity` → Vector; `math` (inputs `a`, `b`; param `op` Choice [add, subtract, multiply, divide, min, max, power, modulo]) → Number; `fn` (input `x`; param `f` Choice [sin, cos, abs, floor, round, sqrt, sign, negate]) → Number; `clamp` (inputs `x`, `min`, `max`) → Number; `compare` (inputs `a`, `b`; param `op` Choice [less, at most, more, at least, equal, different]) → Bool; `logic` (inputs `a`, `b` Bool; param `op` Choice [and, or, not, xor]) → Bool; `choose` (inputs `if` Bool, `then`, `else` Number) → Number; `choose.vector`, `choose.color` (same with Vector/Color); `vec.math` (inputs `a`, `b` Vector; `op` [add, subtract]) → Vector; `vec.scale` (`v` Vector, `by` Number) → Vector; `vec.make` (`x`, `y`) → Vector; `vec.split` (`v`) → `x`, `y`; `vec.length` → Number; `vec.normalize` → Vector; `vec.rotate` (`v`, `degrees`) → Vector; `vec.limit` (`v`, `max`) → Vector; `reflect` (`v`, `normal` Vector, `keep` Number param 1) → Vector; `color.mix` (`a`, `b` Color, `t` Number) → Color; `color.hsv` (`hue`, `saturation`, `value` Number) → Color; `setting` (params `key` Text, `label` Text, `value` Number, `min` Number, `max` Number) → Number; `setting.color` (`key`, `label`, `color`) → Color; `setting.bool` (`key`, `label`, `on` Bool) → Bool. Category "Values" ("Settings" for the three setting blocks).

Rules only: `ball` "About a ball" (input `ball` Ball) → `position` Vector, `velocity` Vector, `speed`, `radius`, `hits` Number, `exists` Bool; `ball.is` (input `ball`; param `target` Text "anything") → Bool; `ball.pick` (param `target`) → Ball; `count` (param `target`) → Number; `counter` (param `id` Number 0..15) → Number. When (category "When", outputs `then` Trigger, `ball` Ball, `other` Ball, `speed` Number, `where` Vector): `when.start`; `when.hit` (params `who` Text "me", `what` Text "anything"); `when.spawn` (`who`); `when.remove` (`who`); `when.enter` (`who`, `what`); `when.leave` (`who`, `what`); `when.beat`; `when.event` (`who`, `name` Text); `when.timer` (params `every` Number 1, `first` Number 1); `when.at` (param `time` Number 1); `when.counter` (params `id`, `reaches` Number). Only if (category "Only if", input `when` Trigger, outputs `then` and `else` Trigger): `if.chance` (input `percent` Number param 50); `if.every` (param `n` Number 2); `if.once`; `if.cooldown` (param `seconds` 1); `if.compare` (inputs `a`, `b`; param `op` as compare); `if.is` (input `ball` Ball; param `target`). Do (category "Do", input `when` Trigger, output `then` Trigger; `ball` input defaults to the firing's ball): `do.grow` (`ball`, `by` Number 2); `do.speed` (`ball`, `factor` 1.1); `do.kick` (`ball`, `velocity` Vector); `do.color` (`ball`, `color` Color, `amount` Number 1); `do.remove` (`ball`); `do.split` (`ball`, `pieces` Number 2); `do.spawn` (params `into` Text "me"; inputs `at` Vector, `velocity` Vector, `size` Number 24, `color` Color alpha 0); `do.gravity` (input `gravity` Vector); `do.set` (params `target` Text "me", `slot` Choice [Motion, Bounce], `key` Text; input `value` Number); `do.event` (`ball`; param `name` Text); `do.note` (`ball`, `loudness` Number 600); `do.counter` (params `id`; input `add` Number 1).

Role blocks: `bounce.in` → ball, velocity, normal, speed, radius, hits, hit (Bool), time; `bounce.out` ← velocity Vector, radius Number, `takes over` Bool. `motion.in` → ball, position, velocity, gravity, dt, time, radius; `motion.out` ← velocity. `sound.in` → ball, speed, index (the event's index in the log), time, x, y (−1..1 across the scene), hits; `sound.out` ← play Bool, key Number, velocity Number, volume Number (dB), pan Number, decay Number, brightness Number; `sound.chord` (input `index` Number) → `key` Number, `has` Bool. Category "This module". `Role::inputs/outputs` list exactly these pins.

Compile: run `validate(graph, catalog(role))` and `order(graph)` (cycles are errors); every node kind must be in the role's catalog; at most one in-block and one out-block for Bounce/Motion/Sound (a second one is a Diagnostic on that node); disabled nodes are dropped with their wires; `triggers` lists the `when.*` blocks in node id order. Diagnostics carry the node and a plain message ("\"foo\" isn't a block this graph knows." comes from validate as is).

- [ ] **Step 1: Write the failing tests**

```rust
// target.rs
#[test] fn targets_round_trip_through_text() {
    for t in [Target::Me, Target::Anything, Target::Thing(ThingId(7)), Target::Tag("red".into()), Target::Kind("balls".into())] {
        assert_eq!(target_from_text(&target_to_text(&t)), t);
    }
    assert_eq!(target_from_text("thing:x"), Target::Anything);
    assert_eq!(target_from_text(""), Target::Anything);
}
// catalog.rs
#[test] fn every_role_has_its_blocks_and_only_rules_has_when_blocks() {
    assert!(catalog(Role::Rules).spec("when.hit").is_some());
    assert!(catalog(Role::Bounce).spec("when.hit").is_none());
    assert!(catalog(Role::Bounce).spec("bounce.in").is_some());
    assert!(catalog(Role::Motion).spec("bounce.in").is_none());
    assert!(catalog(Role::Sound).spec("sound.chord").is_some());
    assert_eq!(Role::of_slot(SlotId::Look), None);
    assert_eq!(Role::Bounce.inputs().len(), 8);
    assert_eq!(Role::Sound.outputs()[0], ("play", PinType::Bool));
    for role in [Role::Rules, Role::Bounce, Role::Motion, Role::Sound] {
        for spec in catalog(role).all() { assert!(!spec.help.is_empty(), "{}", spec.kind); }
    }
}
// program.rs
#[test] fn a_chain_compiles_into_trigger_lists() {
    let c = catalog(Role::Rules);
    let mut g = Graph::new("r");
    let hit = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
    let grow = g.add(c.spec("do.grow").unwrap(), [100.0, 0.0]);
    let n = g.add(c.spec("number").unwrap(), [0.0, 100.0]);
    assert!(g.connect(Link { from: (hit, 0), to: (grow, 0) }, c));
    assert!(g.connect(Link { from: (n, 0), to: (grow, 2) }, c));
    let p = compile(&g, Role::Rules).unwrap();
    assert_eq!(p.triggers, vec![0]);
    assert_eq!(p.blocks[0].then[0], vec![1]);
    assert_eq!(p.blocks[1].inputs[2], Some((2, 0)));
    assert!(!p.is_empty());
}
#[test] fn an_unknown_block_a_loop_and_two_outputs_are_errors() {
    let c = catalog(Role::Bounce);
    let mut g = Graph::new("b");
    g.add(c.spec("bounce.out").unwrap(), [0.0, 0.0]);
    g.add(c.spec("bounce.out").unwrap(), [0.0, 50.0]);
    let errors = compile(&g, Role::Bounce).unwrap_err();
    assert!(errors.iter().any(|d| d.message.contains("one")), "{errors:?}");
    let mut g = Graph::new("w");
    g.add(c.spec("when.hit").or(catalog(Role::Rules).spec("when.hit")).unwrap(), [0.0, 0.0]);
    assert!(compile(&g, Role::Bounce).is_err());
    let mut g = Graph::new("loop");
    let a = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
    let b = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
    g.links.push(Link { from: (a, 0), to: (b, 0) });
    g.links.push(Link { from: (b, 0), to: (a, 0) });
    assert!(compile(&g, Role::Bounce).unwrap_err().iter().any(|d| d.message.contains("loop")));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-vm`
Expected: FAIL to compile (crate missing).

- [ ] **Step 3: Implement the crate**

`Cargo.toml`: `[package] name = "bpe-vm"`, edition 2024, deps `bpe-core`, `bpe-graph`, `glam` from the workspace. Build the specs with `NodeSpec::new(kind, title, category, help).input(PinSpec::new(..)).output(..).param(ParamSpec::number(..))`; put the shared Values list in one function and the per-role lists in `fn specs(role) -> Vec<NodeSpec>`. `compile` keeps a `BTreeMap<u32, usize>` from node id to block index.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-vm`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/bpe-vm
git commit -m "Add bpe-vm: block catalogs per role and the graph compiler"
```

---

### Task 2: bpe-vm dataflow evaluation (Bounce, Motion, Sound)

**Files:**
- Create: `crates/bpe-vm/src/host.rs`, `crates/bpe-vm/src/run.rs`
- Modify: `crates/bpe-vm/src/lib.rs`
- Test: inline

**Interfaces:**
- Produces:
  - `pub trait Host { fn seed(&self) -> u64 { 0 } fn gravity(&self) -> Vec2 { Vec2::ZERO } fn ball(&self, _ball: Ball) -> Option<BallInfo> { None } fn matches(&self, _ball: Ball, target: &Target) -> bool { matches!(target, Target::Anything) } fn balls(&self, _target: &Target) -> Vec<Ball> { Vec::new() } fn things_of(&self, _target: &Target) -> Vec<ThingId> { Vec::new() } fn chord_note(&self, _time: f32, _index: u64) -> Option<f32> { None } }` and `pub struct NoHost;` implementing it.
  - `pub struct Outputs(pub Vec<Option<Val>>)` with `number(&self, pin: usize, default: f32) -> f32`, `vector(&self, pin, default: Vec2) -> Vec2`, `boolean(&self, pin, default: bool) -> bool`, `color(&self, pin, default: [f32;4]) -> [f32;4]`; unwired pins give the default.
  - `pub struct Vm { .. }` with `Vm::new(seed: u64) -> Vm` and the associated `pub fn eval(program: &Program, host: &dyn Host, time: f32, inputs: &[Val], salt: u64) -> Outputs` (stateless: `random` uses a splitmix64 of `host.seed() ^ salt ^ node id`; `counter` reads 0; `time` block gives `time`).
  - `pub const MAX_DEPTH: usize = 64`.
  - Non-finite results become 0: `math` divide by zero → 0, `sqrt` of a negative → 0, every Number/Vector output passes through `finite_or_zero`.

- [ ] **Step 1: Write the failing tests**

```rust
fn bounce_graph(build: impl FnOnce(&mut Graph, u32, u32)) -> Program { /* adds bounce.in and bounce.out, calls build(&mut g, in_id, out_id), compiles */ }

#[test] fn a_wired_output_is_evaluated_and_an_unwired_one_defaults() {
    let p = bounce_graph(|g, i, o| {
        let c = catalog(Role::Bounce);
        let s = g.add(c.spec("vec.scale").unwrap(), [0.0, 0.0]);
        let n = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
        g.node_mut(n).unwrap().params.insert("value".into(), Param::Number(0.5));
        g.connect(Link { from: (i, 1), to: (s, 0) }, c);
        g.connect(Link { from: (n, 0), to: (s, 1) }, c);
        g.connect(Link { from: (s, 0), to: (o, 0) }, c);
    });
    let inputs = [Val::Ball(None), Val::Vector(Vec2::new(10.0, -4.0)), Val::Vector(Vec2::Y), Val::Number(5.0), Val::Number(24.0), Val::Number(0.0), Val::Bool(true), Val::Number(0.0)];
    let out = Vm::eval(&p, &NoHost, 0.0, &inputs, 0);
    assert_eq!(out.vector(0, Vec2::ZERO), Vec2::new(5.0, -2.0));
    assert_eq!(out.number(1, 24.0), 24.0);
}
#[test] fn bad_maths_gives_zero_not_nan() { /* math divide a=1 b=0 → 0; fn sqrt of -4 → 0; vec.normalize of zero → zero */ }
#[test] fn random_is_the_same_for_the_same_seed_and_salt_and_differs_otherwise() { /* two evals with salt 3 equal; salt 4 differs; within min..max */ }
#[test] fn choose_compare_clamp_rotate_and_reflect_work() {
    // compare 3 > 2 → choose(then 1, else 2) = 1; clamp(5, 0, 1) = 1; rotate (1,0) by 90 → (0,1) within 1e-5;
    // reflect v=(0,10) normal=(0,-1) keep=0.5 → (0,-5)
}
#[test] fn a_chord_note_comes_from_the_host() { /* host with chord_note returning Some(60.0) for index 0: sound.chord index 0 → key 60, has true; NoHost → has false */ }
#[test] fn evaluation_stops_at_the_depth_limit() { /* a chain of 70 math blocks a→b→c… evaluates without panicking (the far end reads 0) */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-vm eval`
Expected: FAIL: `Vm`, `Host` not found.

- [ ] **Step 3: Implement `run.rs` values**

`fn value(&self, cx: &Cx, block: usize, pin: u8, depth: usize) -> Val` pulling through `Block.inputs`; `Cx { program, host, time, inputs: &[Val], firing: Option<&Firing>, salt }`. Port `reflect` as `v - normal * (1 + keep) * v.dot(normal)` (same formula as `bpe_runtime::reflect` so the bounce template matches native). `color.hsv` standard HSV→RGB with alpha 1. `vec.rotate` by degrees anticlockwise in scene space.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-vm`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-vm
git commit -m "Evaluate bounce, motion and sound graphs in the VM"
```

---

### Task 3: bpe-vm rules: triggers, conditions, actions and sentences

**Files:**
- Create: `crates/bpe-vm/src/sentences.rs`
- Modify: `crates/bpe-vm/src/run.rs`, `crates/bpe-vm/src/lib.rs`
- Test: inline

**Interfaces:**
- Produces:
  - `pub enum Trigger { Hit, Spawn, Remove, Enter, Leave, Beat, Note, Custom(String) }`, `pub struct Happening { pub kind: Trigger, pub a: Ball, pub b: Option<Ball>, pub point: Vec2, pub normal: Vec2, pub speed: f32 }`.
  - `pub enum Act { Grow { ball: Ball, by: f32 }, Speed { ball: Ball, factor: f32 }, Kick { ball: Ball, velocity: Vec2 }, Tint { ball: Ball, color: [f32; 4] }, Remove { ball: Ball }, Split { ball: Ball, pieces: u32 }, Spawn { thing: ThingId, position: Vec2, velocity: Vec2, radius: f32, color: [f32; 4] }, Gravity(Vec2), Set { thing: ThingId, slot: SlotId, key: String, value: f32 }, Fire { name: String, ball: Ball }, Note { ball: Ball, loudness: f32 } }`.
  - `pub const ACTION_BUDGET: usize = 256`.
  - `Vm::run(&mut self, program: &Program, host: &dyn Host, time: f32, happenings: &[Happening], acts: &mut Vec<Act>)`: fires `when.start` once at the first run; for each happening, every `when.hit/spawn/remove/enter/leave/beat/note/event` whose `who` matches `a` (and `what` matches `b`, with no `b` matching only `anything`) fires with `Firing { ball: Some(a), other: b, speed, point }`; `when.timer` fires when `time >= first + k * every` for the next k (skipping ahead if several are due, firing once); `when.at` fires once when `time >= time`; `when.counter` fires once each time its counter reaches `reaches` from below. A firing walks `then` lists depth-first in order; `if.*` blocks pass to `then` or `else`; `do.*` push an `Act` and continue to `then`. Stops pushing at `ACTION_BUDGET` acts per run. `do.spawn` picks the first `things_of(into)` thing; `do.set` emits one Act per `things_of(target)`; `do.*` with an unwired `ball` use the firing's ball and do nothing when there is none.
  - `Vm::fired_at(&self, node: u32) -> Option<f32>`: the time the block last fired, for the live view. `Vm::counter(&self, id: u32) -> f32`.
  - `pub fn sentences(graph: &Graph) -> Vec<Vec<u32>>`: one list per enabled `when.*` node in id order: the node followed by every node reached through Trigger wires, depth-first, each node at most once.

- [ ] **Step 1: Write the failing tests**

```rust
struct TwoBalls; // Host: matches Me for thing 2, balls(Me) = 2 balls, things_of(Me) = [2], ball(..) gives speed 100
#[test] fn a_hit_grows_the_ball_and_plays_a_note() {
    // when.hit (me, anything) → do.grow by 3 → do.note
    let mut vm = Vm::new(1);
    let mut acts = Vec::new();
    vm.run(&p, &TwoBalls, 0.5, &[hit(Ball { thing: ThingId(2), item: 1 })], &mut acts);
    assert_eq!(acts, vec![Act::Grow { ball: b21, by: 3.0 }, Act::Note { ball: b21, loudness: 600.0 }]);
    assert_eq!(vm.fired_at(hit_node), Some(0.5));
}
#[test] fn hits_by_other_things_do_not_fire_a_me_rule() { /* a = thing 3 → no acts */ }
#[test] fn timers_fire_on_schedule_and_skip_ahead() { /* every 1 first 1: run at 0.5 → none; 1.0 → one; 3.7 → one (not two); start fires only on the first run */ }
#[test] fn chance_once_every_and_cooldown_gate_a_chain() { /* if.once: 2 hits → 1 act; if.every n=2: 4 hits → 2; if.cooldown 1s: hits at 0.0 and 0.5 → 1, at 1.5 → 2; if.chance 100 → always, 0 → never */ }
#[test] fn counters_add_compare_and_fire_once_on_reaching() { /* do.counter id 1 add 1 per hit; when.counter reaches 3 → fires at the third hit only; `counter` value block reads 3 */ }
#[test] fn a_run_stops_at_the_action_budget() { /* when.hit → do.grow with 300 hits → exactly 256 acts */ }
#[test] fn sentences_follow_trigger_wires() { /* when.hit → if.once → do.grow, plus when.timer → do.spawn: [[hit, once, grow], [timer, spawn]] */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-vm rules`
Expected: FAIL: `Vm::run` not found.

- [ ] **Step 3: Implement `run.rs` triggers and `sentences.rs`**

State in `Vm`: `started: bool`, `fired: BTreeSet<u32>` (once), `last: BTreeMap<u32, f32>` (cooldown and live view), `every: BTreeMap<u32, u32>`, `timer_next: BTreeMap<u32, u32>` (k), `counters: BTreeMap<u32, f32>`, `rng: u64` (xorshift64*, seeded `seed | 1`). Random in `run` uses the stateful rng; in `eval` the salted one.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-vm`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-vm
git commit -m "Run rule graphs: triggers, conditions, actions and sentences"
```

---

### Task 4: Runtime actions and ball tints

**Files:**
- Create: `crates/bpe-runtime/src/actions.rs`
- Modify: `crates/bpe-runtime/src/bodies.rs` (tint column), `crates/bpe-runtime/src/lib.rs` (`pub mod actions; pub use actions::{Action, MAX_BALLS}`)
- Test: inline in `actions.rs`

**Interfaces:**
- Produces:
  - `Bodies.tint: Vec<[f32; 4]>`: linear RGBA, alpha 0 means "the look's colour"; `push` sets `[0.0; 4]`; `memory` counts it.
  - `pub enum Action { Grow { ball: Party, by: f32 }, Speed { ball: Party, factor: f32 }, Kick { ball: Party, velocity: Vec2 }, Tint { ball: Party, color: [f32; 4] }, Remove { ball: Party }, Split { ball: Party, pieces: u32 }, Spawn { thing: ThingId, position: Vec2, velocity: Vec2, radius: f32, color: [f32; 4] }, Gravity(Vec2), Set { thing: ThingId, slot: SlotId, key: String, value: Value }, Fire { name: String, ball: Party, point: Vec2 }, Note { ball: Party, loudness: f32 } }` (Clone, Debug, PartialEq).
  - `pub const MAX_BALLS: usize = 4096`, `pub const SPLIT_SPREAD_DEGREES: f32 = 40.0`.
  - `impl World { pub fn apply(&mut self, actions: Vec<Action>, sims: &Sims, log: &mut Log) }` (in `actions.rs`): in order; a ball that is gone or a thing without bodies is skipped; radius after Grow/Spawn/Split clamped to 0.5..=400; Spawn/Split refused once `bodies.len() >= MAX_BALLS`; Split into `pieces` (2..=8): the original keeps its row at radius `r / sqrt(pieces)`, the others are pushed with the same tint and radius, velocities spread evenly over `SPLIT_SPREAD_DEGREES` around the original; Spawn logs `EventKind::Spawn` (a = new party, point = position), Split logs one Spawn per new piece, Remove logs `EventKind::Remove`, Fire logs `Custom(name)` at `point`, Note logs `EventKind::Note` with `speed = loudness`; all at `self.step`. Non-finite vectors or numbers make the action a no-op. `Set` is implemented in Task 5 (leave a `todo!`-free no-op arm here).

- [ ] **Step 1: Write the failing tests** (use `crate::testing::dots_in_ring`)

```rust
#[test] fn grow_speed_kick_tint_and_remove_change_one_ball() { /* apply to item 0; radius 24→27; speed ×2; velocity += kick; tint set; remove → alive false and a Remove event at the current step */ }
#[test] fn spawn_adds_a_ball_and_logs_it() { /* bodies.len() 1→2, tint carried, Spawn event a = (DOTS, 1), point = position */ }
#[test] fn split_spreads_the_pieces_and_keeps_area() { /* pieces 2: len 2, both radius 24/sqrt(2), speeds equal, angle between velocities ≈ 40°, one Spawn event */ }
#[test] fn spawning_stops_at_the_ball_cap() { /* push MAX_BALLS balls; Spawn and Split are ignored; no events */ }
#[test] fn a_non_finite_action_is_ignored() { /* Kick with NaN, Grow with inf → unchanged */ }
#[test] fn actions_on_gone_balls_are_skipped() { /* item 5 of a 1-ball thing; thing 99 */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-runtime actions`
Expected: FAIL: `Action` not found.

- [ ] **Step 3: Implement `actions.rs` and the tint column**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-runtime`
Expected: PASS (existing tests included; `push_fills_every_column` extended with `tint`).

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-runtime
git commit -m "Queueable rule actions and a tint per ball in the runtime"
```

---

### Task 5: Runtime hooks, the Rules stage and graph-aware fingerprints

**Files:**
- Modify: `crates/bpe-runtime/src/plugins.rs`, `crates/bpe-runtime/src/world.rs`, `crates/bpe-runtime/src/actions.rs` (the `Set` arm), `crates/bpe-runtime/src/testing.rs` (a test rules sim), `crates/bpe-runtime/src/lib.rs`
- Test: inline in `world.rs`

**Interfaces:**
- Consumes: Task 4's `Action`, `World::apply`.
- Produces:
  - `ModuleSim` default methods: `fn prepare_graph(&self, params: &Params, _graph: Option<&ModuleGraph>) -> Box<dyn ThingState> { self.prepare(params) }`; `fn forces_at(&self, state: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32, _time: f32) { self.forces(state, bodies, gravity, dt) }`; `fn rules(&self, _state: &mut dyn ThingState, _cx: &mut RuleCx) {}`.
  - `pub struct RuleCx<'a> { pub thing: ThingId, pub world: &'a World, pub events: &'a [Event], pub actions: &'a mut Vec<Action> }` (in `plugins.rs`).
  - `World` additions: `pub seed: u64` (0 until a thing sim sets it), `pub pending: Vec<Action>`, private `tags: BTreeMap<ThingId, BTreeSet<String>>`, private `scope: BTreeMap<ThingId, BTreeSet<ThingId>>` (rules owner → the things whose Rules slot resolves `from` that owner, owner included), and `pub fn kind_of(&self, id: ThingId) -> Option<&str>`, `pub fn tags_of(&self, id: ThingId) -> Option<&BTreeSet<String>>`, `pub fn in_scope(&self, owner: ThingId, thing: ThingId) -> bool`, `pub fn matches(&self, me: ThingId, thing: ThingId, target: &Target) -> bool` (Me → in_scope(me, thing); Thing → equal; Tag → tags_of contains; Kind → kind_of equals; Anything → true), `pub fn things_of(&self, me: ThingId, target: &Target) -> Vec<ThingId>` (every built thing matching, id order), `pub fn slot_params(&self, thing: ThingId, slot: SlotId) -> Option<&Params>`.
  - `SIM_SLOTS` becomes `[Motion, Bounce, Rules]`. `World::build` prepares Motion/Bounce for every thing as today, through `prepare_graph` with `scene.library.modules[s.module].graph.as_ref()`; it prepares Rules only for things where `s.from == id` (the owner) and records `scope[from]` for every thing with a resolved Rules slot. `Prepared` entries become `Slotted { kind: String, params: Params, graph: Option<ModuleGraph>, state: Box<dyn ThingState> }`; `module_state` is unchanged.
  - Step order: at the start of `step`, `apply(take(pending))` (Spawn stage); Forces call `forces_at(.., self.time())`; after the substeps the Rules stage runs every prepared Rules module in `ThingId` order with `RuleCx { events: log.since(self.step), .. }`, then `self.pending = actions`.
  - `Action::Set { thing, slot, key, value }`: when `thing` has that slot prepared, `params.set(key, value)` and re-prepare through `prepare_graph`.
  - `fingerprint` also hashes, for every SIM_SLOT, the module's `ModuleGraph` when `custom` or the slot is Rules (serialise with `ron::to_string`, hash the bytes), and the thing's tags.
  - `testing.rs`: `pub struct GrowOnHit;` a `ModuleSim` with kind `"rules.test"` whose `rules` pushes `Action::Grow { ball: e.a, by: 1.0 }` for every Hit event whose `a.thing` is in scope, plus `pub fn dots_with_rules(count: usize, owner_is_root: bool) -> (Scene, Resolved, Sims)` registering it on the Rules slot of the root (inherited) or the dots thing.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn rules_run_after_the_step_and_act_on_the_next_one() {
    let (scene, resolved, sims) = dots_with_rules(1, false);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(5.0, &sims, &mut log);
    let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit).count() as f32;
    assert!(hits >= 3.0);
    assert_eq!(w.bodies[&DOTS].radius[0], 24.0 + hits);  // the last hit's growth is still pending
    // step once more: nothing pending afterwards unless a new hit
}
#[test] fn a_root_rules_module_runs_once_with_every_thing_in_scope() {
    let (scene, resolved, sims) = dots_with_rules(1, true);
    let w = World::build(&scene, &resolved, &sims);
    assert!(w.module_state(ThingId::ROOT, SlotId::Rules).is_some());
    assert!(w.module_state(DOTS, SlotId::Rules).is_none());
    assert!(w.in_scope(ThingId::ROOT, DOTS));
    assert!(w.matches(ThingId::ROOT, DOTS, &Target::Me));
    assert!(!w.matches(DOTS, WALL, &Target::Me));
}
#[test] fn a_missing_target_matches_nothing() { /* Target::Thing(ThingId(99)), Tag("nope"), Kind("ghost") all false; things_of empty */ }
#[test] fn set_changes_a_bounce_setting_live() { /* Action::Set bounce "keep" (the test bounce's param) → module_state's prepared value changes */ }
#[test] fn custom_graphs_and_tags_change_the_fingerprint() { /* ModuleGraph{custom:true} on the Bounce module vs none; add a tag to DOTS → differs */ }
#[test] fn the_pending_queue_survives_a_clone() { /* World clone after a step with pending → equal pending */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-runtime world`
Expected: FAIL: `dots_with_rules`, `in_scope` not found.

- [ ] **Step 3: Implement the hooks, build, step stages and fingerprint**

In the Rules stage take each owner's `Slotted` out of `self.modules` (`remove`), run, put it back, so `&self` can be lent to `RuleCx`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-runtime`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-runtime
git commit -m "Run Rules modules each step and apply their actions next step"
```

---

### Task 6: Core: modules whose graph is broken fall back to the parent

**Files:**
- Modify: `crates/bpe-core/src/resolve.rs`
- Test: inline

**Interfaces:**
- Produces: `ProblemKind::BrokenGraph(String)` (the compiler message); `pub fn resolve_with(scene: &Scene, reg: &Registry, broken: &BTreeMap<ModuleId, String>) -> Resolved` where a `Linked` slot whose module is in `broken` is treated as `Inherit` and reported as `Problem { thing, slot: Some(slot), kind: BrokenGraph(msg) }` on every thing that links it; `resolve` calls `resolve_with` with an empty map. `resolve_slot` gains the same `broken` parameter as `resolve_slot_with`; the existing `resolve_slot` signature stays.

- [ ] **Step 1: Write the failing test**

```rust
#[test] fn a_broken_module_falls_back_to_the_parent() {
    let reg = testkit::registry();
    let mut scene = Scene::empty(&reg);
    let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
    let root_look = scene.add_module(&reg, "look.plain", "Root").unwrap();
    scene.link(&reg, ThingId::ROOT, SlotId::Look, root_look).unwrap();
    let own = scene.add_module(&reg, "look.plain", "Own").unwrap();
    scene.link(&reg, balls, SlotId::Look, own).unwrap();
    let broken = BTreeMap::from([(own, "A loop".to_string())]);
    let r = resolve_with(&scene, &reg, &broken);
    assert_eq!(r.things[&balls].slots[&SlotId::Look].module, root_look);
    assert_eq!(r.problems, vec![Problem { thing: balls, slot: Some(SlotId::Look), kind: ProblemKind::BrokenGraph("A loop".into()) }]);
    assert!(resolve(&scene, &reg).problems.is_empty());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p bpe-core broken`
Expected: FAIL: `resolve_with` not found.

- [ ] **Step 3: Implement `resolve_with`**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-core
git commit -m "Resolve around modules whose graph does not compile"
```

---

### Task 7: Kit graph helpers and the world host

**Files:**
- Create: `crates/bpe-kit/src/graphs.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub mod graphs; pub use graphs::{resolve_scene, graph_problems, role_of};`), `crates/bpe-kit/Cargo.toml` (add `bpe-vm`, `bpe-graph`, `ron`)
- Test: inline

**Interfaces:**
- Consumes: `bpe_vm::{Role, catalog, compile, Program, Host, Ball, BallInfo, Happening, Trigger, Act}`, `bpe_runtime::{World, Event, EventKind, Party, Action}`, Task 6's `resolve_with`.
- Produces:
  - `pub fn role_of(slot: SlotId) -> Option<Role>` (= `Role::of_slot`).
  - `pub fn bind(template: &Graph, params: &Params) -> Graph`: a copy where every setting block (`bpe_vm::is_setting`) whose `key` param names a key in `params` gets its `value`/`color`/`on` param from the value (Number/Int → `Param::Number`, Color → `Param::Color` linear, Bool → `Param::Bool`).
  - `pub fn bound_keys(template: &Graph) -> BTreeSet<String>`: the setting blocks' keys.
  - `pub fn is_custom(graph: &Graph, bound_template: &Graph) -> bool`: true when the graphs differ once node positions, notes and the graph name are ignored.
  - `pub fn module_graph(kind: &dyn ModuleKind, module: &Module) -> Option<ModuleGraph>`: the module's stored graph when `custom`, else the kind's template bound to the module's params (`custom: false`, `template: kind.id()`, `simple` = the setting blocks whose key is a Simple-depth setting of the kind); None when the kind has no template.
  - `pub fn settings_from_graph(graph: &Graph, defs: &[SettingDef]) -> Params`: the setting blocks' values back as params for keys in `defs` (for Revert).
  - `pub fn compile_module(slot: SlotId, graph: &Graph) -> Result<Program, String>`: compile for the slot's role; the error is the first diagnostic's message, prefixed by the block title when it has a node.
  - `pub fn graph_problems(scene: &Scene) -> BTreeMap<ModuleId, String>`: every library module with `graph.custom` (or in the Rules slot with a graph) that fails `compile_module`.
  - `pub fn resolve_scene(scene: &Scene, reg: &Registry) -> Resolved` = `resolve_with(scene, reg, &graph_problems(scene))`.
  - `pub struct WorldHost<'a> { pub world: &'a World, pub me: ThingId }` implementing `Host`: `seed` = world.seed, `gravity`, `ball` from `world.bodies` (alive only), `matches` via `world.matches`, `balls(target)` = every alive ball of every thing matching, `things_of` = `world.things_of` filtered to things with bodies for `Target::Me` ordering (owner first), `chord_note` None.
  - `pub fn happening(e: &Event) -> Happening` and `pub fn action(act: Act, point: Vec2) -> Action` (Fire takes the firing point).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn binding_writes_params_into_setting_blocks_and_reads_them_back() { /* a template with setting "bounciness" default 1 bound with bounciness 0.5 → node value 0.5; settings_from_graph gives bounciness 0.5; bound_keys = {"bounciness"} */ }
#[test] fn moving_a_block_is_not_custom_but_rewiring_is() { /* positions/notes ignored; removing a link → custom */ }
#[test] fn a_scene_with_a_broken_custom_graph_resolves_around_it() { /* new_scene, give the ball's look... use the Bounce module: graph with two bounce.out + custom:true → graph_problems has the module; resolve_scene problem BrokenGraph on the ball */ }
#[test] fn the_world_host_answers_from_the_world() { /* dots world: ball info, matches Tag after tagging, balls(Anything) count, happening(Hit) maps parties */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-kit graphs`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `graphs.rs`**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-kit graphs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-kit
git commit -m "Kit helpers for template graphs, custom modules and a world host"
```

---

### Task 8: The Rules module, tints on screen and notes from rules

**Files:**
- Create: `crates/bpe-kit/src/modules/rules/{mod.rs, sim.rs, recipes.rs}`
- Modify: `crates/bpe-kit/src/modules/mod.rs`, `crates/bpe-kit/src/lib.rs` (register `RulesKind`, `RulesSim`; re-export `RulesKind, RulesSim, RulesState, recipes`), `crates/bpe-kit/src/things/balls/draw.rs` (tint), `crates/bpe-kit/src/modules/sound/mix.rs` (Note events), `crates/bpe-kit/src/things/scene/sim.rs` (`world.seed`)
- Test: inline

**Interfaces:**
- Consumes: Task 7's `WorldHost`, `happening`, `action`, `compile_module`; Task 5's `RuleCx`.
- Produces:
  - `pub struct RulesKind;` id `"rules.graph"`, name `"Rules"`, slot `Rules`, settings `&[]`, help "What happens when these things hit, spawn, time out or count up.", `template()` = an empty graph named "Rules" with one note "Add a rule with the Add rule menu, or drop a When block here."
  - `pub struct RulesState { pub program: Option<Program>, pub vm: Option<Vm>, pub error: Option<String> }` (Clone).
  - `pub struct RulesSim;` kind `"rules.graph"`: `prepare` → empty state; `prepare_graph` compiles the graph (error kept in `error`); `rules(state, cx)`: creates the Vm on first run with `cx.world.seed ^ cx.thing.0 as u64`, maps `cx.events` through `happening`, runs, maps acts through `action` into `cx.actions`.
  - `BallsDraw`: a ball's colour is `mix(look colour, tint.rgb, tint.a)`.
  - `NoteSoundMix`: `EventKind::Note` events play like hits with `speed` as loudness.
  - `SceneSim::build` sets `world.seed = settings.seed as u64`.
  - `pub struct Recipe { pub name: &'static str, pub help: &'static str, pub add: fn(&mut Graph) -> Vec<(u32, String)> }` and `pub fn recipes() -> Vec<Recipe>`: "Grow on hit" (when.hit → do.grow 2), "Shrink on hit" (do.grow −2), "Speed up on hit" (do.speed 1.05), "Split on hit" (when.hit → if.compare count(me) < 64 → do.split 2), "Colour on hit" (do.color random hue: when.hit → do.color with color.hsv(random 0..1, 1, 1)), "Spawn every second" (when.timer every 1 → do.spawn into me at (0,0) velocity (0,0) size 24), "Remove when fast" (when.hit → if.compare speed > 2000 → do.remove), "Play a note on hit" (do.note), "Count hits" (when.hit → do.counter 1 add 1), "Pulse gravity" (when.timer every 2 → do.gravity (0, 2000) then when.timer every 2 first 2 → do.gravity (0, 900)). Each `add` places its chain below existing blocks and returns the `(node id, param)` pairs to show at Simple depth (the Do block's main number).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_grow_on_hit_rule_grows_the_ball_in_the_world() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let rules = scene.add_module(&kit.registry, "rules.graph", "Grow").unwrap();
    let mut g = RulesKind.template().unwrap();
    let simple = recipes().iter().find(|r| r.name == "Grow on hit").unwrap().add;
    simple(&mut g);
    scene.library.modules.get_mut(&rules).unwrap().graph = Some(ModuleGraph { graph: g, template: "rules.graph".into(), custom: true, simple: Default::default() });
    scene.add_slot(&kit.registry, ThingId(2), SlotId::Rules).unwrap();
    scene.link(&kit.registry, ThingId(2), SlotId::Rules, rules).unwrap();
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    playback.seek(10.0);
    let hits = playback.log().events().iter().filter(|e| e.kind == EventKind::Hit).count() as f32;
    assert!(hits >= 3.0);
    assert!(playback.world().bodies[&ThingId(2)].radius[0] >= 24.0 + 2.0 * (hits - 1.0));
}
#[test] fn every_recipe_compiles_and_runs() { /* for each recipe: add to a fresh template, compile_module(Rules) ok, simple pairs name real nodes and params */ }
#[test] fn a_note_action_plays_a_note() { /* log with one EventKind::Note event speed 600 → mix_scene not silent */ }
#[test] fn a_tinted_ball_is_drawn_in_its_tint() { /* set bodies.tint[0] = [1,0,0,1]; build_frame → balls[0].color red */ }
#[test] fn a_rules_graph_that_does_not_compile_keeps_the_scene_running() { /* broken graph → resolve_scene problem; Playback runs 2 s without panic */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-kit rules`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the module, the recipes, the tint and the Note mix**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-kit`
Expected: PASS (the contract test still passes with the new kind).

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-kit
git commit -m "The Rules module: graphs that grow, split, tint, spawn and play notes"
```

---

### Task 9: Template graphs and compiled paths for bounce, motion and sound

**Files:**
- Create: `crates/bpe-kit/src/modules/bounce/graph.rs`, `crates/bpe-kit/src/modules/motion/graph.rs`, `crates/bpe-kit/src/modules/sound/graph.rs`
- Modify: `crates/bpe-kit/src/modules/bounce/{mod.rs, sim.rs}`, `modules/motion/{mod.rs, sim.rs}`, `modules/sound/{mod.rs, mix.rs}`, `crates/bpe-mix/src/lib.rs` (`ModuleMix::notes_with`, `mix_with`), `crates/bpe-kit/src/lib.rs` (`mix_scene` uses `mix_with` with `&scene.library`), `crates/bpe-kit/src/testing.rs`
- Test: inline + `crates/bpe-kit/src/graphs.rs` contract test

**Interfaces:**
- Consumes: Task 7 helpers, `bpe_vm::{Vm, Val, Outputs, NoHost}`.
- Produces:
  - `bounce::graph::template() -> Graph`: `bounce.in` → `reflect(keep = setting bounciness)` → `choose.vector(if hit, vec.scale(.., setting speed_gain), ..)` → `vec.limit(setting top_speed)` → `bounce.out.velocity`; radius: `choose(if hit, clamp(radius + setting grow, 0.5, setting max_size), radius)` → `bounce.out.radius`; `setting.bool takes_over` → `bounce.out.takes over`. Settings blocks keyed `bounciness, speed_gain, top_speed, grow, max_size, takes_over` with the same labels and ranges as `BasicBounce`.
  - `BasicBounceKind::template()` returns it. `BasicBounceSim::prepare_graph` stores `BounceState { settings: BasicBounce, program: Option<Program> }` (program only when `graph.custom` and it compiles; a failed compile keeps the native path); `respond` with a program: inputs per `Role::Bounce.inputs()` (`hit` = `contact.speed >= limits.resting`), `salt = (item as u64) << 32 | hits`, writes `velocity` (clamped to `limits.ceiling`) and `radius` (clamped 0.5..=400); `takes_over` = evaluated once in `prepare_graph` with zero inputs and kept.
  - `motion::graph::template()`: `motion.out.velocity = vec.scale(vec.math(add, velocity, vec.scale(gravity, setting gravity_scale × dt)), max(0, 1 − setting drag × dt))`; `BasicMotionSim::forces_at` evaluates per alive ball when custom.
  - `sound::graph::template()`: `play = logic(and, compare(speed at least setting quietest_hit), sound.chord(index).has)`, `key = sound.chord(index).key`, `velocity = clamp((speed − quietest_hit) / max(loudest_hit − quietest_hit, 1), 0, 1)`, `volume = setting volume`, `pan = clamp(x, −1, 1) × setting stereo_width`, `decay = setting decay`, `brightness = setting brightness`; settings blocks keyed `volume, low_note, high_note, quietest_hit, loudest_hit, stereo_width, decay, brightness` (`low_note`/`high_note` feed nothing in the graph; the chord host reads them from params, so they remain bound settings).
  - `bpe_mix::ModuleMix::notes_with(&self, params: &Params, _graph: Option<&ModuleGraph>, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) { self.notes(params, thing, events, cx, out) }` and `pub fn mix_with(settings: &MixSettings, resolved: &Resolved, library: &Library, log: &Log, mixes: &Mixes) -> Soundtrack` (`mix` calls it with an empty library).
  - `NoteSoundMix::notes_with` with a custom graph: `SoundHost { chords: &Chords, seed }` whose `chord_note(time, index)` = `candidates[mix64(seed ^ index) as usize % len]`; per event evaluate with inputs per `Role::Sound.inputs()` (`x = point.x / half_width`, `y = point.y / half_width`), push a `Note` when `play`, gain from `velocity` and `volume` exactly as the native path (`velocity * velocity.sqrt() * 10^(volume/20)`, angle from `pan`).
  - `testing.rs`: `pub fn with_custom_template(scene: &mut Scene, module: ModuleId, reg: &Registry)` stores `module_graph(kind, module)` with `custom: true` (the unchanged template, forced through the compiled path).

- [ ] **Step 1: Write the failing tests**

```rust
// graphs.rs
#[test] fn every_template_matches_its_native_path() {
    let kit = kit();
    for scene in scenes::stage2(&kit.registry).into_iter().chain(scenes::stage4(&kit.registry)) {  // stage4 arrives in Task 10; start with stage2
        let native = log_hash_and_samples(&scene, &kit);
        let mut forced = scene.clone();
        for id in forced.library.modules.keys().copied().collect::<Vec<_>>() {
            let m = &forced.library.modules[&id];
            if m.graph.as_ref().is_some_and(|g| g.custom) { continue; }
            if kit.registry.module(&m.kind).unwrap().template().is_some() && role_of(m.slot).is_some() {
                with_custom_template(&mut forced, id, &kit.registry);
            }
        }
        assert_eq!(log_hash_and_samples(&forced, &kit), native, "{}", scene.name);
    }
}
// bounce/graph.rs
#[test] fn a_custom_bounce_graph_runs_instead_of_the_native_one() { /* template with the reflect keep setting replaced by number 0 (dead bounce) marked custom → ball ends slower than native */ }
#[test] fn a_custom_graph_that_fails_to_compile_keeps_the_native_bounce() { /* program None, behaviour equals native */ }
// motion/graph.rs
#[test] fn a_custom_motion_graph_can_add_wind() { /* add vec.make(400, 0) × dt to velocity → ball drifts right */ }
// sound/graph.rs
#[test] fn a_custom_sound_graph_can_pick_its_own_key() { /* key = number 60 → notes all key 60 */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-kit template`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the three templates and compiled paths**

Build templates with a small local builder (`add(kind, pos) -> u32`, `set(node, param, Param)`, `wire(from, pin, to, pin)`) on `bpe_vm::catalog(role)`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-kit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-kit crates/bpe-mix
git commit -m "Template graphs for bounce, motion and sound with a compiled path for custom modules"
```

---

### Task 10: Stage 4 scenes, goldens and the exporter

**Files:**
- Create: `scenes/next/stage4-grow.ron`, `stage4-split.ron`, `stage4-tint.ron`, `stage4-bounce-graph.ron`, `stage4-wind.ron`
- Modify: `crates/bpe-kit/src/scenes.rs` (`pub fn stage4(reg: &Registry) -> Vec<Scene>`), `crates/bpe-kit/tests/golden.rs`, `scenes/next/golden.txt`, `scenes/next/golden-debug.txt`, `crates/bpe-cli/src/lib.rs` (`resolve_scene`)
- Test: `crates/bpe-kit/tests/golden.rs`

**Interfaces:**
- Consumes: `recipes()`, templates, `with_custom_template`-style builders.
- Produces: `stage4(reg)`: "Grow on hit" (circle arena, 3 balls in a ring, Rules: Grow on hit + Play a note on hit), "Split on hit" (one ball, Rules: Split on hit, bounciness 1.02), "Colour by speed" (block of 4×4 balls, Rules: when.hit → do.color with color.hsv(clamp(speed / 1500, 0, 1), 1, 1)), "Bouncy when slow" (custom bounce graph: keep = choose(compare speed < 300, 1.15, 0.9)), "Wind and melody" (custom motion graph adding `vec.make(fn sin(time × 2) × 400, 0) × dt`; custom sound graph with key = 48 + modulo(hits, 12)). The `scenes/next/*.ron` files match the builders (the existing `BPE_BLESS=1` test rewrites them). The golden test's scene list = stage2 + stage4; `BLESS_GOLDEN=1` rewrites the hashes; the seek-replay and liveliness checks run over all of them.

- [ ] **Step 1: Write the failing tests**: extend `golden.rs` to iterate `stage2(reg).into_iter().chain(stage4(reg))`, add `seeking_replays_the_stage4_scenes_identically` if the existing seek-replay test is not already generic, and in `scenes.rs` add stage4 names to the "files match" test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-kit --test golden`
Expected: FAIL: `stage4` not found.

- [ ] **Step 3: Implement `stage4`, bless the files and goldens, switch bpe-cli to `resolve_scene`**

Run: `BPE_BLESS=1 cargo test -p bpe-kit scenes` then `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden`. Check each new golden hash differs from the stage 2 ones and the liveliness check passes (each scene logs hits for the whole 20 s).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-kit && cargo test -p bpe-cli`
Expected: PASS. Then export a 3 s clip of "Split on hit" with `cargo run -p bpe-cli -- scenes/next/stage4-split.ron /tmp/split.mp4` (whatever the CLI's argument form is) and check ffprobe reports a 3 s video with an audio stream.

- [ ] **Step 5: Commit**

```bash
git add scenes/next crates/bpe-kit crates/bpe-cli
git commit -m "Five stage 4 scenes with golden event logs"
```

---

### Task 11: The node editor, ported into the studio

**Files:**
- Create: `crates/bpe-studio/src/nodes/{mod.rs, editor.rs, palette.rs, draw.rs, params.rs, tests.rs}`
- Modify: `crates/bpe-studio/src/main.rs` (`mod nodes;`), `crates/bpe-studio/Cargo.toml` (add `bpe-graph`, `bpe-vm`)
- Test: `crates/bpe-studio/src/nodes/tests.rs` (kittest)

**Interfaces:**
- Consumes: `bpe_app/src/node_editor/{mod,palette,draw,tests}.rs` as the source to port (read only); `bpe_graph::{Graph, Catalog, Node, Link, Param, PinType}`; `crate::theme`.
- Produces:
  - `pub struct Editor` (the port of `NodeEditor`): `Editor::new(id: impl Hash) -> Editor`, `Editor::show(&mut self, ui: &mut Ui, graph: &mut Graph, catalog: &dyn Catalog, live: Option<&dyn LiveView>, scene: &Scene) -> Outcome`, `pub struct Outcome { pub changed: bool, pub selected: Option<u32> }`, `pub trait LiveView { fn flash(&self, node: u32) -> f32 { 0.0 } fn error(&self, node: u32) -> Option<String> { None } }`. Keeps: pan/zoom, box select, drag, wires with type colours, palette search (right-click or double-click the canvas), delete, duplicate, notes, disable, the expose menu on a parameter. Drops: `crate::keys`, thumbnails, `opened`. Test helpers from the old `tests.rs` (`pin_screen_pos`, `canvas_to_screen`) are ported as `pub(crate)` on `Editor`.
  - `params.rs`: `pub fn param_control(ui: &mut Ui, spec: &ParamSpec, param: &mut Param, scene: &Scene) -> bool`: Number → `Slider` over the spec range (a `DragValue` when the range is the full default), Bool → checkbox, Text → single line, Color → colour button (linear ↔ sRGB), Vector → two drag values, Choice → menu button; a Text param whose spec name is one of `who, what, target, into` shows the **target picker** instead: a `menu_button` labelled with `target_words(scene, &target)` listing "Me", "Anything", every thing by name (`thing:<id>`), every tag in the scene (`tag:<name>`) and every thing kind present (`kind:<id>`); `pub fn target_words(scene: &Scene, target: &Target) -> String` ("Me", "Anything", the thing's name or "Missing thing", "#tag", "Any balls").
  - The parameter's right-click menu: "Show on the card" (toggles `node.exposed`) and, when exposed, "Show at Simple" (reported through `Outcome` as `pub simple_toggled: Option<(u32, String)>`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_block_can_be_added_from_the_palette_and_wired() { /* right-click canvas, type "Number", Enter → graph has a number block; drag from its output pin to a math block's input → one link */ }
#[test] fn dragging_moves_the_block_and_delete_removes_it() { /* port of the old tests */ }
#[test] fn the_target_picker_names_a_missing_thing() {
    let scene = bpe_kit::new_scene(&bpe_kit::registry());
    assert_eq!(target_words(&scene, &Target::Thing(ThingId(2))), "Ball");
    assert_eq!(target_words(&scene, &Target::Thing(ThingId(99))), "Missing thing");
    assert_eq!(target_words(&scene, &Target::Tag("red".into())), "#red");
}
#[test] fn picking_a_target_writes_its_id() { /* param_control on a `who` Text param; click the menu, click "Ball" → param text "thing:2" */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-studio nodes`
Expected: FAIL to compile.

- [ ] **Step 3: Port the editor**

Copy `mod.rs`, `palette.rs`, `draw.rs` into `nodes/`, rename `NodeEditor` → `Editor`, remove the keyframe rows and thumbnails, switch colours to `crate::theme`, add the target picker and the expose menu.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-studio nodes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-studio
git commit -m "Port the node editor into the new studio with a target picker"
```

---

### Task 12: The node panel under the preview and the Nodes switch

**Files:**
- Modify: `crates/bpe-studio/src/nodes/mod.rs` (`NodePanel`), `crates/bpe-studio/src/app.rs` (panel, `resolve_scene`, `LiveView` from playback), `crates/bpe-studio/src/inspector.rs` (`depth_switch` with Nodes, `Inspector.nodes` request/state, the "Edited as nodes · Revert to template" row, the compile badge), `crates/bpe-studio/src/tree.rs` (`problem_words` arm for `BrokenGraph`: "Its {slot} graph doesn't compile: {msg}")
- Test: `crates/bpe-studio/src/nodes/tests.rs`, `inspector.rs` tests

**Interfaces:**
- Consumes: Task 11's `Editor`, `bpe_kit::graphs::{module_graph, is_custom, compile_module, settings_from_graph, role_of}`, `bpe_vm::catalog`, `bpe_kit::RulesState`.
- Produces:
  - `pub struct NodePanel { pub open: Option<(ThingId, SlotId)>, editor: Editor, height: f32 }` with `NodePanel::show(&mut self, ctx: &egui::Context, studio_parts…) -> bool changed`, drawn as `egui::Panel::bottom("nodes")` declared after the transport (so it sits above it, under the preview), resizable, default 300, with a title row: "{thing name} › {slot name}", the module name with "shared ×N", the compile status ("Compiles" in MUTED or the first error in ERROR), "Revert to template" (only when custom and the kind has a template), and "Close". The graph shown is the slot's shown module (linked or inherited); when the module has no stored graph or an unedited one, it is (re)created through `module_graph` on open. After every `Outcome.changed`, `custom = is_custom(&graph, &bound template)` (Rules modules are always custom). Revert: `module.params = settings_from_graph(..)` merged over the current params, `module.graph = None`, panel stays open on the fresh template.
  - `Inspector.nodes_request: Option<(ThingId, SlotId)>` set by the Nodes button; `Inspector.nodes_open: Option<(ThingId, SlotId)>` mirrored from the panel so the switch shows Nodes selected; `depth_switch(ui, depth, nodes: Option<NodesSwitch>)` where `NodesSwitch { enabled: bool, open: bool, hover: &'static str }`: enabled for slots with a role; disabled with "Look graphs come with the Visuals stage" for the others; absent for the kind's own card.
  - `LiveView` for the open graph: `Flashes { fired: BTreeMap<u32, f32>, now: f32, errors: BTreeMap<u32, String> }` built each frame from the playback world's Rules state (`RulesState.vm.fired_at`) and the compile diagnostics; a block glows for 0.4 s after firing.
  - `Studio::scene_changed`, `replace_scene`, `headless` use `bpe_kit::resolve_scene`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn the_nodes_switch_opens_the_panel_with_the_path_in_its_title() { /* harness with the ball selected; click "Nodes" on the Bounce card → a label "Ball › Bounce" exists and the editor shows "Basic bounce" blocks (bounce.in title) */ }
#[test] fn editing_the_graph_marks_the_module_custom_and_revert_goes_back() { /* delete the reflect block → module.graph.custom true, the card shows "Edited as nodes"; click "Revert to template" → graph None */ }
#[test] fn a_broken_graph_shows_a_red_badge() { /* custom bounce graph with two outputs → the Bounce card shows the error text and the tree row has the badge words */ }
#[test] fn look_has_no_nodes_yet() { /* the Look card's Nodes button is disabled */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-studio nodes`
Expected: FAIL.

- [ ] **Step 3: Implement the panel, the switch, the badge and the live view**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-studio`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-studio
git commit -m "The node panel slides up under the preview and Nodes opens it"
```

---

### Task 13: Graph settings and rule sentences on the Simple/More cards

**Files:**
- Create: `crates/bpe-studio/src/nodes/cards.rs`
- Modify: `crates/bpe-studio/src/inspector.rs` (slot card body for custom modules and Rules modules)
- Test: `inspector.rs` tests

**Interfaces:**
- Consumes: `bpe_vm::{sentences, is_setting, catalog}`, `bpe_kit::{recipes, graphs::bound_keys}`, Task 11's `param_control`.
- Produces:
  - `pub fn graph_settings(ui: &mut Ui, id: impl Hash, graph: &mut ModuleGraph, catalog: &dyn Catalog, depth: Depth, scene: &Scene) -> bool`: a grid of the graph's shown parameters: at Simple those in `graph.simple`; at More every setting block's value and every `exposed` param, in node id order, labelled by the setting block's `label` (or "{block title} · {param label}"), edited with `param_control`. Native settings of the module whose key is not in `bound_keys(template)` are drawn after it by the existing `settings_grid` (so `instrument` stays a picker on a custom sound module).
  - `pub fn rules_card(ui: &mut Ui, id: impl Hash, graph: &mut ModuleGraph, scene: &Scene, depth: Depth) -> RulesOutcome { changed: bool, open_nodes: bool }`: one row per sentence: the block titles joined with " → " ("When hit → Grow → Play a note"), the Do blocks' number params inline at Simple (the `simple` pairs) and every param of the chain at More; "No rules yet." when empty; an "Add rule" menu button listing `recipes()` by name with help on hover that appends the recipe and adds its pairs to `simple`; a "Remove" button per sentence that removes the chain's nodes; "Open in nodes" sets `open_nodes`.
  - The slot card: when the shown module is custom (or Rules), the card body is `rules_card` / `graph_settings`, headed by "Edited as nodes · Revert to template" for non-Rules.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_rules_card_lists_sentences_and_adds_a_recipe() { /* add a Rules slot to the ball with a rules module; "No rules yet." shown; click "Add rule", click "Grow on hit" → a label containing "When hit → Grow" and a drag value showing 2 */ }
#[test] fn a_custom_module_shows_its_setting_blocks_as_sliders() { /* force the bounce template custom; Simple shows "Bounciness"; drag it → the setting block's value changes and fingerprint changes (playback rebuilt) */ }
#[test] fn a_custom_sound_module_keeps_its_instrument_picker() { /* custom sound template; the card has "Instrument" */ }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p bpe-studio inspector`
Expected: FAIL.

- [ ] **Step 3: Implement `cards.rs` and the card body**

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p bpe-studio`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/bpe-studio
git commit -m "Custom modules and rules show their inputs on the Simple and More cards"
```

---

### Task 14: Whole-workspace checks, review, Windows build and deliverables

**Files:**
- Create: `/mnt/project-files/ballphysics/modular-revamp/stage4/{ball-studio-next.exe, bpe-render-next.exe, REPORT.md, *.png}`
- Modify: whatever the review finds.

- [ ] **Step 1: Run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` (in the background, ~10 min).
Expected: clean, all green.

- [ ] **Step 2: Rebase on main** (`git fetch origin main && git rebase origin/main`), fix conflicts additively, rerun Step 1 if anything changed.

- [ ] **Step 3: Fresh review**: one reviewer subagent over `git diff main...HEAD` with the spec and this plan; fix every real finding; rerun Step 1.

- [ ] **Step 4: Windows build**: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`; copy the exes to the stage4 folder.

- [ ] **Step 5: Screenshots under Xvfb**: the node panel open on a Rules graph (with blocks flashing while playing), the Simple card of a Rules module with sentences, the More card of a custom bounce module, a tree row with a red badge. `import -window root`.

- [ ] **Step 6: REPORT.md**: what works, the five scenes, how to open Nodes, what is deferred (GPU graphs, Enter/Leave/Beat emitters, pitch on Note events, Rules on folders until stage 3 lands).

- [ ] **Step 7: Open the PR** with the attribution block, assign and request review from pixelpitch161, subscribe, merge when green. Write the `stage4-rules-nodes` memory file and reply.

- [ ] **Step 8: Commit** (deliverables are outside the repo; the REPORT is copied into `docs/superpowers/reports/2026-10-29-stage4.md` too).
