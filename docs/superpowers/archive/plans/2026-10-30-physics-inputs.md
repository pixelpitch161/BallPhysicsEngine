# Physics as modulation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let what the balls do (at the hit, while they fly, and across the whole scene) drive any
followable synth or effect knob, through input blocks in patch and effects graphs and a Follow…
menu in the inspector.

**Architecture:** bpe-synth gets three input blocks (`hit_input`, `live_input`, `scene_input`)
that read a `Sources` bundle handed to a voice or effects run when it starts (`Block::bind`), and
"knob pins": extra number inputs on a block that add to a knob while wired (`Block::set_knob`).
bpe-runtime records ball tracks and scene samples at 60 Hz into the `Log`, apart from its hash.
bpe-kit turns the log into `PhysicsInputs` (per-hit values and tracks, scene series, chords per
channel), bpe-mix hands them to voices and effects, and renders channels that read another
channel's level after it. The studio wires inputs into knobs from a Follow… menu.

**Tech Stack:** Rust workspace (bpe-synth, bpe-runtime, bpe-mix, bpe-kit, bpe-studio with egui
0.37 + kittest, bpe-cli).

**Spec:** `docs/superpowers/specs/2026-10-30-music-synth-design.md` §4 (also §2.2 Inputs row,
§2.3 "physics values captured at the hit are fixed for the voice's life", §8 Physics inputs test,
§8 Inspector "kittest checks for … Follow…").

## Global Constraints

- Each input block "outputs 0 to 1, with **In low / In high** (the physics range that maps to 0
  and 1), **Curve** (linear, exponential, S) and **Smoothing** (ms)".
- At-hit and live inputs: "patch graphs only"; at-hit values are "fixed for the voice".
- Live: "records these at 60 Hz for a ball for up to 12 s after each of its hits … The voice
  interpolates them to block rate. Once the recording ends, the last value holds."
- Scene-wide: "any graph; on the video clock". Hits per second "over a 0.5 s window".
- Channel level: "another channel's level after its inserts … Channels that read another
  channel's level render after it; a loop between channels is a compile problem".
- "Rule kept: sound never feeds back into physics. Physics inputs only read the run's log."
- Determinism: same scene renders the same samples twice; the log's `hash()` must not change
  because of anything this plan records.
- Studio copy: the editor's default font lacks ♭ ♯ ▶ glyphs — plain words only. Inspector panel
  max 640 px.
- Build with `CARGO_INCREMENTAL=0`; delete `target/release` after release builds (disk is tight).
- Release `cargo test` always fails bpe-runtime `pushing_an_earlier_step_panics_in_debug`
  (debug-only should_panic) — run the workspace suite in debug.

## Design decisions this plan makes (spec is silent or impossible as written)

1. **Spin is left out** of the at-hit and live lists: balls in this engine never rotate (`Bodies`
   has no angular velocity), so it would always be 0.
2. **Follow… covers knobs that can move mid-note** (`followable(kind)`, Task 2), on patch macros
   and effects-list knobs. Built-in instrument knobs (Pluck decay/brightness) are not graph
   knobs and get no Follow….
3. **Knob pins add to the knob.** A wire into a knob pin adds its block-average value to the
   knob's own value (clamped to the knob's range), so a macro or slider keeps setting the base.
   Follow… wires `input block → Scale and offset (scale = depth, offset 0) → knob pin`, depth =
   `max − value` when the value is in the lower half of the range, else `min − value`.
4. **Raw units:** speeds px/s, size = radius px, angle degrees (0 grazing, 90 straight on),
   height/left-right 0..1 in the frame (1 = top / right), distance = screen px from the shot's
   centre, energy = Σ ½·(r/20)²·|v|² / 10⁶ over live, non-silent balls, channel level dBFS per
   64-frame block. Default In low/In high per input are in Task 1's table.
5. **Bars are 4 beats**, counted from the first beat of the grid.
6. **Hit angle** uses the ball's velocity in the track sample taken on the hit step (just after
   the bounce): `atan2(|v·n|, |v − (v·n)n|)` in degrees.
7. **Colour hue/brightness** are the ball's Look `fill` paint evaluated with `colour_at` at the
   hit (classic and styled looks; anything else gives white): hue/value from HSV.
8. **Tracks are recorded only when `World.record_tracks` is set**; the sound job and the CLI
   set it from `bpe_kit::uses_live_inputs(scene)`. Scene samples are always recorded (tiny).
9. **Channel level's channel** is named by its module name (`channel` text knob); the first
   sound module with that name wins. A loop is reported in the status line ("<module>: Channel
   level loops back through <other>, so it reads silence") and the looping reads get -90 dB.

## Review Focus

1. A patch with a live input on a ball that never hits again after the note (track shorter than
   the voice): the last value must hold, never jump to 0 → test `live_input_holds_the_last_value`
   (Task 1).
2. Smoothing must not glide from 0 on the first block of a voice (an at-hit value would fade in)
   → test `smoothing_starts_at_the_value` (Task 1).
3. Editing an effects list that has a followed knob (reorder, add, remove) must keep the follow
   wiring of the blocks that stay → test `rechain_keeps_follows` (Task 2).
4. Rewinding the editor's playback must not leave tracks or scene samples from the future in the
   log → test `rewind_drops_later_motion` (Task 3).
5. A camera move that changes where hits sit in the frame but not the hits themselves must
   re-render channels reading height → test `physics_values_are_in_the_fingerprint` (Task 4).

---

### Task 1: Input blocks and Sources (bpe-synth)

**Files:**
- Create: `crates/bpe-synth/src/inputs.rs` (types, input tables, `uses`)
- Create: `crates/bpe-synth/src/blocks/physics.rs` (the `PhysicsIn` block)
- Modify: `crates/bpe-synth/src/lib.rs` (Block::bind, exports), `blocks/mod.rs`,
  `catalog.rs` (3 specs + `make`), `program.rs` (instantiate_with, run_with, Patch::bind,
  effects-mode check)

**Interfaces:**
- Produces:
  - `pub struct InputDef { pub name: &'static str, pub label: &'static str, pub low: f32, pub high: f32 }`
  - `pub const HIT_INPUTS: &[InputDef]`, `LIVE_INPUTS`, `SCENE_INPUTS` with exactly:
    - hit: speed "Hit speed" 0..1200; angle "Hit angle" 0..90; side "Side hit" 0..1; tag "Hit thing's tag" 0..1; size "Ball size" 0..60; hue "Colour hue" 0..1; brightness "Colour brightness" 0..1; height "Height in the frame" 0..1; left_right "Left-right in the frame" 0..1; hits "Hit count" 0..20; age "Age" 0..30
    - live: speed "Speed" 0..1200; height "Height in the frame" 0..1; left_right "Left-right in the frame" 0..1; distance "Distance from the camera centre" 0..960
    - scene: balls "Ball count" 0..50; energy "Motion energy" 0..5; hit_rate "Hits per second" 0..20; beat "Beat phase" 0..1; bar "Bar phase" 0..1; chord_root "Chord root" 0..1; chord_quality "Chord quality" 0..1; bass "Song bass" 0..1; mid "Song mid" 0..1; high "Song high" 0..1; channel "Channel level" -48..-6
  - `pub struct HitPhysics { pub values: [f32; 11] /* HIT_INPUTS order; tag slot unused */, pub tags: Vec<String> /* the other thing's */, pub track: Option<Arc<Track>> }` with `pub fn key(&self) -> u64` (hash of every f32's bits, tags and track samples).
  - `pub struct Track { pub rate: f32, pub samples: Vec<[f32; 4]> /* LIVE_INPUTS order, sample i at i/rate s after the note */ }`, `pub fn at(&self, seconds: f32) -> [f32; 4]` (linear, last holds, empty → zeros).
  - `pub struct SceneSignals { pub rate: f32, pub balls: Vec<f32>, pub energy: Vec<f32>, pub hit_rate: Vec<f32>, pub bass: Vec<f32>, pub mid: Vec<f32>, pub high: Vec<f32>, pub beats: Vec<f64>, pub chords: Vec<[f32; 2]>, pub levels: BTreeMap<String, Arc<Vec<f32>>> }` (Default, Clone, Debug, PartialEq); `pub fn value(&self, input: &str, channel: &str, time: f64) -> f32`; `pub fn key(&self) -> u64`. Series are sampled at `rate` (60) and interpolated; chords step; beat/bar phase worked out exactly from `beats` (0 before the first beat; after the last, the last gap repeats); levels are dBFS per `BLOCK` frames at 48 kHz, -90 outside.
  - `#[derive(Clone, Default)] pub struct Sources { pub hit: Option<Arc<HitPhysics>>, pub scene: Option<Arc<SceneSignals>> }`
  - `Block::bind(&mut self, _sources: &Sources) {}` (default no-op).
  - `Program::instantiate_with(&self, cx: &Ctx, sources: &Sources) -> Patch` (`instantiate` = with `Sources::default()`); `Patch::bind(&mut self, sources: &Sources)`; `Program::run_with(&self, audio: &mut [f32], cx: Ctx, sources: &Sources)` (`run` = default).
  - `#[derive(Default, Debug, PartialEq)] pub struct Uses { pub hit: bool, pub live: bool, pub scene: bool, pub channels: BTreeSet<String> }`, `pub fn uses(graph: &Graph) -> Uses` (enabled blocks only).
- Catalog: kinds `hit_input` ("At the hit"), `live_input` ("Live"), `scene_input` ("Scene"), group "Inputs", one Number output `out`. Knobs: `input` choice (names from the table, default first), `low`/`high` numbers labelled "In low"/"In high" (range -10000..10000, defaults 0/1), `curve` choice linear|exponential|S, `smoothing` "Smoothing ms" 0..2000 (default 0 for hit, 30 for live and scene); `hit_input` adds `tag` text "Tag"; `scene_input` adds `channel` text "Channel".
- Mapping: `t = clamp((raw-low)/(high-low), 0, 1)` (0 when high == low); exponential `t²`; S `t²(3−2t)`; smoothing one-pole per block `a = 1 − exp(−(BLOCK/rate)/(ms/1000))`, first block takes the value directly. Hit inputs read raw once in `bind`; live read `track.at(cx.since_on)`; scene read `scene.value(input, channel, cx.time)`. tag: 1 when `hit.tags` contains the `tag` knob.
- Effects-mode compile adds, for each hit/live input: `Diagnostic { node, message: "At-the-hit and live inputs only work in instrument patches" }`.

- [ ] **Step 1: Write the failing tests** in `blocks/physics.rs`:
  - `hit_input_maps_its_range`: hit speed raw 600, low 0 high 1200 → 0.5 linear; exponential → 0.25; S → 0.5; raw 2000 → 1.0; raw -5 → 0.0.
  - `smoothing_starts_at_the_value`: hit speed 1200 with smoothing 500 ms → first block output 1.0.
  - `live_input_follows_the_track_and_holds_the_last_value` (Review Focus 1): track rate 60, height samples [1.0, 0.5, 0.0]; smoothing 0; outputs at since_on 0 → 1.0, 1/120 s → 0.75 (±0.01), 1.0 s → 0.0, 5.0 s → 0.0; a track whose last sample is 0.3 reads 0.3 at 20 s.
  - `scene_input_reads_the_video_clock`: balls series [0, 10, 20] at rate 60, low 0 high 20: cx.time 1/60 → 0.5; beat phase with beats [1.0, 1.5, 2.0] at time 1.25 → 0.5, bar at 1.25 → 0.125 (4 beats per bar, beat 0 is bar start).
  - `tag_input_matches_the_other_things_tags`: tags ["pegs"], tag knob "pegs" → 1, "walls" → 0.
  - `channel_level_reads_its_channel`: levels {"Kick": [-6.0; 10]}, channel "Kick", low -48 high -6 → 1.0; channel "Snare" → 0.0.
  - In `program.rs`: `hit_inputs_are_patch_only` — `chain_graph` with a `hit_input` wired into an Amp's mod → `compile_effects` Err with the message above; the same block in `blank_patch` compiles. `uses_lists_inputs` — patch with a live_input and a scene_input channel "Kick" → `Uses { hit: false, live: true, scene: true, channels: {"Kick"} }`.
- [ ] **Step 2: Run** `CARGO_INCREMENTAL=0 cargo test -p bpe-synth physics` — Expected: FAIL to compile (types missing).
- [ ] **Step 3: Implement** inputs.rs, the block, the catalog specs, `bind`, `instantiate_with`, `run_with`, the effects check and `uses`.
- [ ] **Step 4: Run** `cargo test -p bpe-synth` — Expected: all pass (existing tests unchanged).
- [ ] **Step 5: Commit** `feat(synth): physics input blocks and sources`.

### Task 2: Knob pins, Follow helpers, chains that keep follows (bpe-synth)

**Files:**
- Create: `crates/bpe-synth/src/follow.rs`
- Modify: `catalog.rs` (append knob pins), `program.rs` (Op mods), `lib.rs` (Block::set_knob, exports), the blocks named below, `effects.rs` (straight_chain, rechain)

**Interfaces:**
- Consumes: Task 1 `InputDef` tables, input block kinds.
- Produces:
  - `pub fn followable(kind: &str) -> &'static [&'static str]`: osc [semitones, cents, width, detune, fm]; filter [cutoff, resonance]; ladder [cutoff, resonance, drive]; drive [amount, tone, mix]; amp [gain]; eq [low_gain, mid_gain, high_gain]; bitcrush [bits, downsample, mix]; delay [feedback, mix]; chorus [rate, depth, mix]; reverb [mix, width]; width [width]; compressor [threshold, makeup]; crossfade [mix]; lfo [rate]; macro [value]; everything else [].
  - Catalog appends, after each block's own inputs, one Number pin per followable knob named after the knob, help "Added to the knob while wired". Pins keep their old indices.
  - `Block::set_knob(&mut self, _name: &str, _value: f32) {}`; implemented by every block listed above for every knob listed (recompute coefficients; keep running state).
  - Program: per op, knob pins become `mods` (knob name, register, base knob value, min, max); before the op runs, value = clamp(base + block-average of the register's left side, min, max), and `set_knob` is called when it differs (by bits) from the last value applied. Only real pins go to `process`.
  - follow.rs: `pub struct FollowInput { pub kind: String /* "hit_input" | "live_input" | "scene_input" */, pub input: String, pub channel: String }`; `pub fn follow(graph: &Graph, node: u32, param: &str, with: &FollowInput) -> Option<Graph>` (None if the knob isn't followable or the node is missing; replaces an existing follow of that knob; new blocks sit 200 and 380 units left of the target, below it); `pub fn unfollow(graph: &Graph, node: u32, param: &str) -> Graph` (removes the wire and every block that then feeds nothing); `pub fn followed(graph: &Graph, node: u32, param: &str) -> Option<String>` (the input's label, e.g. "Height in the frame", or "Channel level (Kick)").
  - effects.rs: `straight_chain` ignores "side" blocks — blocks whose outputs reach only knob pins, directly or through other side blocks — and their links. `pub struct ChainBlock { pub id: Option<u32>, pub kind: String, pub params: BTreeMap<String, Param>, pub enabled: bool }`; `pub fn rechain(graph: &Graph, chain: &[ChainBlock]) -> Graph`: keeps nodes (and their side blocks and knob-pin links) whose `id` is in `chain`, applies params/enabled, adds new blocks (`id: None`), relinks In → chain → Out in order; side blocks of removed nodes go too.

- [ ] **Step 1: Write the failing tests:**
  - `every_followable_knob_moves_the_sound` (program.rs): for each kind in the catalog with `followable` knobs (skip `macro`, `lfo`, which have no audio in), build In → block (audio into pin 0) → Out as an effects graph with a `macro` block (value 1, min 0, max 1) through a `scale` block into the knob pin; run 0.5 s of seeded noise (`Rng::new(7) * 0.5`) with scale = `max − min` and with scale 0; assert the outputs differ (max abs diff > 1e-4) for every (kind, knob). For `lfo.rate` and `macro.value`: a patch where they feed an Amp's mod, same assertion.
  - `a_knob_pin_adds_to_the_knob` : Amp gain −60 dB base, macro 1.0 → scale 60 → gain pin: output RMS equals the unmodulated 0 dB RMS within 1%; scale 100 clamps at +12 dB (ratio 3.98 ±1%).
  - follow.rs `follow_wires_and_unfollow_cleans_up`: `follow(blank_patch, filter, "cutoff", live height)` → graph has one `live_input` (input "height", low 0, high 1) and one `scale` (scale 18000 = 20000 − 2000, offset 0) wired into the filter's cutoff pin; compiles; `followed(..) == Some("Height in the frame")`; following again with hit speed replaces it (still one input block); `unfollow` returns a graph equal to `blank_patch()` in nodes and links; `follow(.., "mode", ..)` is None.
  - effects.rs `rechain_keeps_follows` (Review Focus 3): chain [drive, delay], follow delay.mix with scene beat; `straight_chain` gives both; `rechain` swapping order keeps the scene_input wired to the delay's mix pin; removing the delay drops the scene_input and scale; adding a reverb (id None) appends it.
- [ ] **Step 2: Run** `cargo test -p bpe-synth` — Expected: the new tests fail.
- [ ] **Step 3: Implement** knob pins, `set_knob` on each listed block, follow.rs, `straight_chain` side blocks, `rechain`.
- [ ] **Step 4: Run** `cargo test -p bpe-synth` and `cargo test -p bpe-studio --bin ball-studio-next fx_list` — Expected: pass (catalog pins don't break the effects list tests).
- [ ] **Step 5: Commit** `feat(synth): knob pins, follow and chains that keep follows`.

### Task 3: Tracks and scene samples in the log (bpe-runtime)

**Files:**
- Modify: `crates/bpe-runtime/src/events.rs` (Log fields and methods), `world.rs` (record), `playback.rs` (truncate), `lib.rs` (exports)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct TrackSample { pub step: u64, pub position: Vec2, pub velocity: Vec2, pub radius: f32 }`
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct SceneSample { pub step: u64, pub balls: u32, pub energy: f32 }`
  - Log: `tracks: BTreeMap<Party, Vec<TrackSample>>`, `scene: Vec<SceneSample>`; `push_track(&mut self, ball: Party, s: TrackSample)`, `track(&self, ball: Party) -> &[TrackSample]`, `push_scene(&mut self, s: SceneSample)`, `scene_samples(&self) -> &[SceneSample]`, `truncate_motion(&mut self, step: u64)` (drops samples after `step`); `memory()` counts them; `hash()` unchanged.
  - `pub const MOTION_RATE: u32 = 60; pub const TRACK_SECONDS: f64 = 12.0;`
  - World: `pub record_tracks: bool` (default false). At the end of `step`: every `max(1, steps_per_second / 60)` steps push a SceneSample (alive non-silent balls; energy per decision 4). When `record_tracks`: a ball whose `hits` count rose this step gets a sample this step; on sample steps every alive non-silent ball within `TRACK_SECONDS` of its last hit gets one. World keeps `last_hit: BTreeMap<ThingId, Vec<(u32, Option<u64>)>>` (hits seen, step of the last rise).
  - Playback `seek_within` rewind also calls `truncate_motion(world.step)`.

- [ ] **Step 1: Write the failing tests** (world.rs tests module, using the existing test kit in `testing.rs`):
  - `tracks_follow_a_ball_for_twelve_seconds_after_a_hit`: a ball dropped in a box with `record_tracks = true`, 20 s at 240 steps/s: no samples before its first hit step; the first sample's step equals that hit's step; samples on the 60 Hz grid; the last sample is within 12 s (+1 sample) of its last hit. With `record_tracks = false` the track is empty.
  - `scene_samples_count_the_balls`: 3 balls, 1 s → 60 samples (±1), `balls == 3`, energy > 0 once moving.
  - `motion_never_changes_the_hash`: same run with and without `record_tracks` → equal `log.hash()`.
  - `rewind_drops_later_motion` (Review Focus 4, playback.rs): seek to 3 s, back to 1 s → no track or scene sample with step > 1 s × rate; seek to 3 s again → samples equal the first pass's.
- [ ] **Step 2: Run** `cargo test -p bpe-runtime motion tracks scene_samples rewind` — Expected: FAIL (missing API).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-runtime` — Expected: pass (golden logs unchanged).
- [ ] **Step 5: Commit** `feat(runtime): record ball tracks and scene samples for sound`.

### Task 4: Physics into voices and effects (bpe-mix, bpe-synth voices)

**Files:**
- Modify: `crates/bpe-synth/src/voices.rs` (PlayNote, render_channel_with), `crates/bpe-mix/src/lib.rs` (Note.hit, PhysicsInputs, mix_premaster), `channels.rs`, `buses.rs`; every `Note { .. }` and `PlayNote { .. }` literal (kit modules set `hit: None` for now).

**Interfaces:**
- Consumes: Task 1 `Sources`, `HitPhysics`, `SceneSignals`, `uses`, `Program::instantiate_with`, `run_with`.
- Produces:
  - `PlayNote` loses `Copy` and gains `pub physics: Option<Arc<HitPhysics>>`; a mono retrigger calls `Patch::bind` with the new note's physics.
  - `pub fn render_channel_with(program, notes, voices, seconds_per_beat, rate, frames, threads, scene: Option<Arc<SceneSignals>>) -> ChannelAudio` (`render_channel_on` passes None).
  - `Note` gains `pub hit: Option<u64>` (the log index of the event that played it).
  - `#[derive(Clone, Debug, Default)] pub struct PhysicsInputs { pub hits: BTreeMap<u64, Arc<HitPhysics>>, pub scene: SceneSignals /* the scene's chords, no levels */, pub chords: BTreeMap<u32, Vec<[f32; 2]>> /* per channel */ }`; `pub fn channel_signals(&self, channel: u32) -> SceneSignals` (scene with that channel's chords when it has its own).
  - `mix_premaster(settings, resolved, library, log, mixes, animation, video_time, physics: &PhysicsInputs, cache)`; `mix`/`mix_with` pass `&PhysicsInputs::default()`.
  - Patch channels: each PlayNote gets `physics.hits.get(&hit)`; programs get the channel's signals. Instrument fx and inserts `run_with` the channel's signals; buses and master fx the scene's (with every level, Task 5).
  - Fingerprints: patch groups hash `HitPhysics::key()` per note when `uses(graph).hit || .live`, and `SceneSignals::key()` when `.scene`; every effects graph with `uses(fx).scene` hashes the signals it runs with.
- [ ] **Step 1: Write the failing tests** (channels.rs tests):
  - `a_filter_following_falling_height_closes_monotonically` (spec §8): patch = `blank_patch` with the filter's cutoff followed by live height (Task 2 `follow`), filter cutoff base 200; one note, gate 1 s, track rate 60 falling linearly 1 → 0 over 1 s. Render; for ten 100 ms windows compute brightness = RMS(first difference)/RMS; each window ≤ the previous × 1.01, and the first > 2 × the last.
  - `hit_values_reach_the_voice`: patch whose Amp mod is a hit_input speed (low 0 high 1200); two notes with HitPhysics speed 300 and 1200 → RMS ratio of the second to the first ≈ 4 (±5%).
  - `physics_values_are_in_the_fingerprint` (Review Focus 5): render, then change only one note's HitPhysics height → `fresh == 1`; same physics → `fresh == 0`; a built-in channel ignores physics (fresh 0).
  - `scene_inputs_reach_effects`: insert graph In → Amp (gain −60, gain pin ← scene_input beat via scale 60) → Out on a constant input; with beats every 0.5 s the output rises and falls each half second (RMS of 0.4–0.5 s window > 10 × that of 0.0–0.05 s window).
- [ ] **Step 2: Run** `cargo test -p bpe-mix` — Expected: new tests fail to compile.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-mix -p bpe-synth` and `cargo build --workspace --all-targets` — Expected: pass / builds.
- [ ] **Step 5: Commit** `feat(mix): physics inputs reach voices and effects`.

### Task 5: Channel level and sidechain order (bpe-mix)

**Files:**
- Modify: `crates/bpe-mix/src/channels.rs` (waves), `buses.rs`, `lib.rs`

**Interfaces:**
- Consumes: Task 1 `uses(..).channels`, `SceneSignals.levels`; Task 4 `PhysicsInputs`.
- Produces:
  - `fn reads(settings, channel) -> BTreeSet<u32>`: channels named by `scene_input` channel knobs in the channel's instrument patches, instrument fx and inserts (names via `settings.channel_names`; first id with that name; unknown names read silence).
  - Rendering runs in waves: each wave is every channel whose reads are all rendered; after a wave, each of its channels' strip output becomes a level series (dBFS of the stereo RMS per 64-frame block, floor -90) added to the signals of later waves. Channels in a loop render last, reading -90 for the looping channels, with the problem line "<name>: Channel level loops back through <other>, so it reads silence".
  - Cache retain happens once, after all waves. Buses and master fx get every level.
- [ ] **Step 1: Write the failing tests:**
  - `a_channel_ducks_under_another` : channel 1 = pluck notes every 0.5 s; channel 2 = a sustained saw patch note whose insert Amp gain pin is fed by `scene_input` channel "Kick" (low -48, high -6) through a scale of -40 (dB). Name channel 1 "Kick". RMS of channel 2 in the 20 ms after each kick < 0.5 × its RMS just before the kick.
  - `a_level_loop_is_a_problem_not_a_hang`: channels "A" and "B" each read the other → render returns, problems contain "A: Channel level loops back through B, so it reads silence" (or with B first), both channels have sound.
  - `reading_a_channel_renders_after_it`: wave order check through a test-only `waves(settings, notes) -> Vec<Vec<u32>>` = [[1],[2]] for the kick setup.
- [ ] **Step 2: Run** `cargo test -p bpe-mix level` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-mix` including the ignored busy-minute test (`-- --ignored busy`) — Expected: pass, timings within part 3's (2.49 s / 1.95 s on 4 cores) + 10%.
- [ ] **Step 5: Commit** `feat(mix): channel level inputs render after the channels they read`.

### Task 6: Physics from the run (bpe-kit, sound job, CLI)

**Files:**
- Create: `crates/bpe-kit/src/music/physics.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (mix_scene_cached builds PhysicsInputs; `uses_live_inputs`), `music/mod.rs`, `modules/sound/mix.rs`, `modules/drums/mix.rs`, `modules/melody/mix.rs` (set `hit: Some(index)` from `cx.hit_index(e)`), `crates/bpe-studio/src/sound_job.rs`, `crates/bpe-cli/src/lib.rs` (set `world.record_tracks`)

**Interfaces:**
- Consumes: Tasks 1, 3, 4.
- Produces:
  - `pub fn uses_live_inputs(scene: &Scene) -> bool` (any library instrument patch with `uses(..).live`).
  - `pub fn physics_inputs(scene: &Scene, resolved: &Resolved, log: &Log, settings: &MixSettings, video_time: &dyn Fn(f64) -> f64) -> PhysicsInputs`:
    - `hits` only when some instrument patch uses hit or live inputs: for every Hit and Note event (by log index): speed `e.speed`; angle (decision 6, 0 without a track sample); side from the normal (contact at `-normal`: top 0, bottom 1/3, left 2/3, right 1, by the larger axis); size = radius from the hit-step track sample, else the thing's ball radius setting; hue/brightness (decision 7); height `clamp(0.5 − sy/(2·half_height), 0, 1)`, left-right `clamp(0.5 + sx/(2·half_width), 0, 1)` from `log.camera_at(step).to_screen(point)`; hits = that ball's hit count so far; age = time since its Spawn event, else since 0; tags of the other party's thing; track = the ball's samples from the hit step to +12 s, resampled to 60 Hz from the hit time (speed |v|, height, left-right, distance |screen| ), each through the camera at its step.
    - `scene`: rate 60 over the duration on the video clock: balls and energy from `scene_samples` (step → run time → video time), hit_rate = Hit events in the last 0.5 s ÷ 0.5, song bass/mid/high from the song's bands (`bands_of`, as pulses use), beats = `settings.beats`, chords from Scene defaults' progression (root = pitch class / 11, quality Major 0, Minor 1/3, Diminished 2/3, Augmented 1).
    - `chords`: per sound module whose progression differs from Scene defaults'.
- [ ] **Step 1: Write the failing tests** (physics.rs):
  - `a_falling_ball_reads_falling_height`: new_scene, `record_tracks = true`, run 4 s; take the ball's first hit's track; between its highest point and the next hit, the height values are non-increasing.
  - `hit_values_from_a_known_hit`: hand-built log with one Hit (speed 600, normal (0,-1), point at the screen's centre) → speed 600, side 1/3, height 0.5, left-right 0.5, hits 1.
  - `scene_series_count_hits_per_second`: 10 hits evenly in 1 s → hit_rate at 1.0 s ≈ 10 (±2).
  - `no_physics_work_without_inputs`: new_scene (plucks only) → `hits` empty.
  - `uses_live_inputs_sees_patches`.
- [ ] **Step 2: Run** `cargo test -p bpe-kit physics` — Expected: FAIL.
- [ ] **Step 3: Implement**, wire into `mix_scene_cached`, the modules, the sound job and the CLI.
- [ ] **Step 4: Run** `cargo test -p bpe-kit` and the golden soundtrack tests — Expected: pass; golden hashes unchanged (no starter scene uses inputs).
- [ ] **Step 5: Commit** `feat(kit): physics inputs from the run`.

### Task 7: Follow… in the inspector (bpe-studio)

**Files:**
- Create: `crates/bpe-studio/src/follow_menu.rs` (the menu and the followed icon)
- Modify: `instruments.rs` (macro rows), `fx_list.rs` (knob rows; edits through `rechain`), `inspector.rs` / `app.rs` if new edits need routing

**Interfaces:**
- Consumes: Task 2 `follow`, `unfollow`, `followed`, `followable`, `rechain`, `ChainBlock`; Task 1 tables.
- Produces:
  - `pub fn menu(ui: &mut Ui, patch: bool, channels: &[String]) -> Option<FollowInput>`: "Follow…" submenu with "At the hit" and "Live" submenus (only when `patch`) and "Scene" (always), each listing the table labels; "Channel level" opens a submenu of `channels` (other sound modules' names).
  - `pub fn mark(ui: &mut Ui, label: Option<&str>)`: a small wave icon named "Follows <label>" with that hover text.
  - Macro rows (patch instruments): right-click the label → Follow… (and "Stop following" when followed) → `InstrumentEdit::Set` with the followed graph. Effect knob rows: right-click the label → Follow… (scene only, plus at-hit/live when the list is an instrument patch's — never: effects lists are effects) → `FxEdit::Set(rechain/follow result)`. Only knobs in `followable(kind)` get the menu.
  - `fx_list` builds its edits with `rechain` so follows survive.
- [ ] **Step 1: Write the failing kittests:**
  - fx_list `follow_a_knob_from_the_list`: chain [delay]; right-click "Mix" label → "Follow…" → "Scene" → "Beat phase" → edit is `Set(g)` with `followed(g, delay, "mix") == Some("Beat phase")`; showing `g` still lists the delay (not "Custom routing") and shows "Follows Beat phase".
  - instruments `follow_a_macro`: patch instrument with filter cutoff exposed; right-click "Filter Cutoff" → Follow… → Live → "Height in the frame" → an `InstrumentEdit::Set` whose graph has a live_input; then "Stop following" removes it.
  - `effects_offer_only_scene_inputs`: in an effects list the Follow… menu has no "At the hit".
- [ ] **Step 2: Run** `cargo test -p bpe-studio --bin ball-studio-next follow` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio --bin ball-studio-next` — Expected: pass.
- [ ] **Step 5: Commit** `feat(studio): Follow… menu on macros and effect knobs`.

### Task 8: Demo, screenshots, report

**Files:**
- Create: `/mnt/project-files/ballphysics/music-synth/part4/` (REPORT.md, screenshots, demo .ron and half-size .mp4)

- [ ] **Step 1:** Make `height-sweep.ron`: pitch-climb with its patch's filter cutoff following live height and a Drive insert's amount following hit speed; render with bpe-cli at half size; confirm the CLI output lists loudness and no problems.
- [ ] **Step 2:** Screenshots under Xvfb: Follow… menu open on a macro, the followed icon on an effect knob, the patch in the node panel showing the input → scale → knob pin.
- [ ] **Step 3:** `cargo clippy --workspace --all-targets -- -D warnings` and the full `cargo test --workspace` (debug) — Expected: clean / green but the known release-only test.
- [ ] **Step 4:** REPORT.md: what's new, the design decisions above, rulings, deferred minors.
- [ ] **Step 5: Commit** any repo changes `docs: part 4 report`.
