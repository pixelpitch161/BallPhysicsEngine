# W13: one wind, and rules on more cards (plan)

Spec: `docs/superpowers/specs/2026-11-08-one-wind-rule-cards-design.md` (review findings G4, G8). One pull request.
Nothing changes for a scene whose balls don't follow the air and whose rules don't touch the four cards: every
physics, particle, rules and sound golden must stay byte for byte, and no picture test may move.

## Part A: balls follow the air (runtime + kit)

### A1. The Motion card (`bpe-kit/src/modules/motion/`)

- `BasicMotion.follow_air: f32`, `#[setting(label = "Follow the air", range(0, 1e9), span(0, 20), unit = "per s",
  keyframe, pulse, group = "Air")]`, default 0. Help: how quickly these balls take on the air's speed where they
  are (the Air card's wind, swirled by the balls, blocked by shapes); 0 leaves them out of the air.
- Rename `wind`'s label to **Steady push** (key stays `wind`), doc: a steady push like a tilted table; for the
  scene's wind use Follow the air. Card help: "Falls under gravity, is slowed by the air, can be pushed steadily or
  carried by the scene's air."
- `BasicMotionSim::air_grip(state) -> f32`: `follow_air` when finite and > 0, else 0. `air_key() ->
  Some("follow_air")`.

### A2. Runtime hooks (`bpe-runtime`)

- `plugins.rs` `ModuleSim`: `fn air_grip(&self, _state: &dyn ThingState) -> f32 { 0.0 }` and `fn air_key(&self) ->
  Option<&'static str> { None }` (the Motion setting that makes balls feel the air).
- `ThingSim::heats_air(&self, _resolved: &ResolvedThing) -> bool { false }`: the thing puts heat into the air.
- `rules/sim.rs` `BlockSim::changes(&self, _params: &Params) -> Option<(Option<SlotId>, String)> { None }`: the card
  and key a block changes (Change setting answers).
- New `air.rs` in the runtime: `pub trait AirSim: Send + Sync { fn wake(&self, world: &mut World); fn catch_up(&self,
  world: &mut World); fn velocities(&self, world: &World, points: &[Vec2]) -> Option<Vec<Vec2>>; }` and
  `pub fn air_felt(scene: &Scene, sims: &Sims) -> bool`: some enabled thing's Motion card (saved scene, so every
  world and part agrees) whose module has an `air_key` has that key non-zero, or `key~to` non-zero, or a track or
  pulse on `{thing, Motion, key}`, or an enabled rule's block `changes()` names `(Some(Motion), key)`.
  `Sims.air: Option<Arc<dyn AirSim>>`. Exported from `lib.rs`.
- `World`: `pub air_felt: bool` (set in `build` from `air_felt(scene, sims)`), `air_at: BTreeMap<ThingId, Vec<(Vec2,
  f32)>>` (each ball's air velocity and grip this step). In `build`, after the `after_build` loop: when felt and
  `sims.air` is there, `wake`. `lone` copies `air_felt` and keeps the ROOT state (the air) when felt; `air_at`
  empty.
- `World::step`: after `work_out_pulls`, `work_out_air(sims)`: clear; when felt, `air.catch_up(self)`, then for each
  thing whose Motion module grips some live, free ball, its points → `velocities` → `air_at` (grip 0 and zero air
  for the others). `move_bodies`: after the Motion forces, before the light pull, `v += (air − v)·(1 −
  e^(−grip·dt))` for each live, free ball with grip > 0.
- `is_beside`: a thing whose sim `heats_air(r)` is in the main world when `air_felt(scene, sims)`.
- Fingerprint (`fingerprint_of`): the "beside card" block (Air card and its tracks/pulses) runs when any
  `own_cards` thing **or** `air_felt`. The keys loop uses the whole physics animation (Air keys included) when felt,
  so Air pulses following sound or light hash their maker's fingerprint. `World::build`'s `sensed` keeps Air keys
  when felt too.

### A3. The kit's air (`bpe-kit`)

- `air.rs`: `Air.drift: Vec2` (see Part B). `KitAir` (in `air.rs`) implements `AirSim`: `wake` takes the ROOT air or
  puts in `Air::new(spec of the Air card or default)`, wakes it (step kept as `max(step, due now)`); `catch_up`
  brings it to `particles::step::due(world.step, world.steps_per_second)` with `step::threads()`; `velocities` maps
  `Air::at`. `lib.rs::kit()` sets `s.air`.
- `ParticlesSim::heats_air(r)`: `Forces::read(r.slots.get(&Motion)).heats_air()`.
- `ChangeSetting::changes`: `Some((card.slot(), setting))` when the setting isn't empty.

### A4. Tests

- `bpe-kit/src/modules/motion/sim.rs`: a ball with Follow the air 5 in an Air card wind of 300 px/s to the right (no
  gravity) reaches about 300 px/s; with 0 it doesn't move sideways; a ball following still air behind a moving ball
  is pushed along its wake (moves); a ball behind a shape in a wind is slower than one in the open.
- `bpe-runtime`/kit: `air_felt` true for a set, varied, keyed or ruled Follow the air, false otherwise; a scene
  with a heating Particles thing and following balls has the thing in `main_part` and the main and whole-scene
  worlds step the balls identically for 2 s; the fingerprint changes when the Air card's wind changes only while
  balls feel the air.
- Goldens unchanged (run `golden*` in debug and release).

## Part B: haze drifts with the air

- `HazeMoves::WithTheAir` ("With the air", code 1.0 as Drifting). Drift direction stays `when = drifting`.
- `SceneSim::after_step`: if the world has an air (`air_mut`), `drift += spec.ambient(Vec2::ZERO, world.time()) ·
  world.sim_dt()`. Deterministic, not physics.
- `LightingGpu.haze_more.xy` becomes the drift offset in px. `LightingSettings::gpu` leaves `[0, 0]`;
  `LightingSettings::haze_drift(time, air: Option<Vec2>) -> [f32; 2]`: Drifting/Swirling `dir × speed × 100 ×
  time`, With the air `air × speed`, Still 0. `SceneDraw` sets it from `cx.time` and `air_of(cx.world).drift`.
- `lighting.wgsl::haze_density`: `q = (p - push - gu.haze_more.xy) / size` (was `... - haze_more.xy * speed * t *
  100 / size`); swirling's own time terms stay. Every picture with moving haze must stay the same (same maths, moved
  to the CPU); check `cargo test -p bpe-kit --test '*'` picture tests.
- Tests: `haze_drift` numbers; a draw test that With the air under an Air card wind gives the drift the world kept.

## Part C: rules change Health and Air

- `CardChoice` gains `Health`, `Air`, `Sound`, `Mixer` (slots `Health`, `Air`, `Sound`, `Mixer`).
- `World::is_physics`: `SIM_SLOTS`, `BESIDE_SLOTS` and `SlotId::Health`.
- `animate`'s destination loop: `(thing, Some(Health))` → `rules.health.set_card(thing, sim.prepare(&params))`
  through `sims.health(kind)` (new `Health::set_card`, keeps each ball's health). `(ROOT, Some(Air))` already falls
  to the scene sim's `settings_changed` (the air re-reads its card).
- Tests (`bpe-kit/src/rules/`): a Hit rule that Sets the Air card's Wind to 400 makes the air's spec 400 from the
  next step (`air_of(world)`), and gone after Lasts; a rule that Sets Health's Least strength high stops hits
  taking health; a rule Setting Starting health gives later spawned balls the new start.

## Part D: rules change Sound and Mixer (`bpe-sound`)

- `bpe-runtime` `RulesState::whole_changes(thing, slot)` (iterator of `(&str, &[Change])`, range scan, no
  allocation) and `has_whole_changes(thing, slot)`.
- New `bpe-sound/src/ruled.rs`: `pub fn number(list, base: f32, time) -> f32` (a change's Int value read as a
  number, the fold of `Change::on` on `Value::Number`), `pub fn at(rules, key: &ChangeKey, base, time) -> f32`.
- Voices: `ShapeSpec.knob_keys: Vec<(String, usize)>` (sorted, from the layout) built in `patch.rs`;
  `ShapeVoice::read_keys(time, rules)` folds `whole_changes(thing, Sound)` on the knobs they name; the control
  sampler's `knobs_at` does the same from `world.rules()`.
- `ShapeRoute`: `thing: Option<ThingId>`, `at(time, rules)` folds `out_send1..3`.
- `Buses::begin(time, rules)`, `Bus::at` folds `ChangeKey`s made at build for fader, pan, sends; `Master::at(time,
  rules)` for `master_input`/`master_output`; `Keyed` holds every keyframe number of its layer (key, base, track,
  `ChangeKey`) and `at(time, rules)` folds; `FxCx.rules`; `FxChain::process` re-reads a slot when it is keyed or the
  mixer has whole changes. The control sampler's bus fader folds too.
- `pub fn voice_rule_keys(scene, thing) -> BTreeSet<String>` and `pub fn mixer_rule_keys(scene) ->
  BTreeSet<String>` (what a rule reaches), for the editor's menu.
- Tests (`bpe-sound/tests/rule_changes.rs`): a shape whose Hit rule Sets its Volume to −60 dB for good is near
  silent after the first hit and loud before (render with the test-tone-free starter voice, compare RMS); a rule
  pulling Bus 1's fader to −60 does the same; a filter cutoff change reaches the knob (`voice_rule_keys` contains the
  layered key; ShapeVoice knob value moves); `render` never allocates with a running change (the counting-allocator
  test gains a rule); the control sampler's level follows a Volume change.

## Part E: the editor (`bpe-studio`)

- `rules_panel/setting_menu.rs`: `CARDS` gains Health, Air, Sound, Mixer. For Sound only `voice_rule_keys(scene,
  thing)`, for Mixer `mixer_rule_keys(scene)`; other cards every changeable setting. Test: the menu of a shape with a
  Sound card lists Volume and not Wave; the scene's lists Air · Wind and Mixer · Bus 1 Fader.
- Inspector: nothing new beyond the Motion card's new row (drawn by the derive).

## Part F: notes, checks

- `docs/notes/engine.md` (Motion, air felt), `particles.md` (heating particles in main, haze drift), `rules.md`
  (four cards), `sound.md` (rule changes read once a block), `lighting.md` (With the air). As built here at the end.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --
  --test-threads=2`, goldens in release, full-size look check for the haze (`LOOK_ONLY` a hazy scene) because the
  shader changed.
- One Opus review of the whole diff before the PR; then PR, CI green, merge.

## Things not to do

- Don't step the air when no ball feels it (keeps goldens and speed).
- Don't read the air per substep (once a step, like Moths), and never mutate the air from the Motion module.
- Don't allocate on the sound path: all `ChangeKey`s and key lists are made at patch build.
- Don't make Sound/Mixer changes physics (`is_physics` stays false for them).

## As built

- Built as planned, parts A to F. The air reaches the runtime through `Sims.air` (`AirSim`, the kit's `KitAir`);
  `air_felt` reads the saved scene; `World::work_out_air` reads the air once a step into `World.air_at`; the pull goes
  in `move_bodies` after the Motion forces and before Moths.
- `voice_rule_keys(scene, resolved, thing)` and `mixer_rule_keys(scene, registry)` return sorted `Vec<String>`s
  (binary-searched by the menu) rather than sets. The voice's keys are its knob layout's keys (`ShapeSpec.knob_keys`)
  plus Sends.
- Added beyond the plan, after the review: `ThingSim::reads_air` (the scene sim, while the haze moves With the air)
  and `bpe_runtime::air_read` (air felt, or read): the world senses the Air keys and the fingerprint counts the Air
  card and its keys, so the main playback (whose air the haze drifts with) restarts on Air edits even when no ball
  feels the air. A Particles thing fed by its parent's particles puts no heat into the air while balls follow it
  (its parent may be beside; `heats_air` is false for it). `RulesState::add_change` no longer clears the list under
  a new Set for good; `prune` drops what such a Set hides 0.1 s later (`HIDDEN_KEPT`), so the sound engine, a step
  behind its physics, folds what the physics folded (no click when a Set re-fires). Values are unchanged (a Set at
  weight 1 gives its value whatever came before). `voice_rule_keys` builds only that thing's spec (`patch::spec_of`,
  no light probe). Emitter Aim plans test flights in still air (known gap).
- The haze offset moved from the shader to the CPU (`LightingSettings::haze_drift`) for every mode; Drifting and
  Swirling give the same numbers as before, so no picture test moved.
- Tests: `bpe-kit/src/air/follow_tests.rs` (carried, still, sheltered by a solid shape, air felt cases, fingerprints,
  haze drift), `particles/air_tests.rs` (heating particles step with the physics; the editor's and the whole scene's
  balls match), `rules/health_tests.rs` (Least strength, Starting health for later balls, a rule's wind, seeks),
  `modules/lighting.rs` (haze_drift), `bpe-sound/tests/rule_changes.rs`, a rules block in `tests/alloc.rs`, and the
  studio menu test. A ball that stirs the air at Balls stir 1 reaches about 73 % of the wind in a second (it holds
  the air back round it); the tests that measure the wind set Balls stir to 0.
- Known gaps as in the spec.
