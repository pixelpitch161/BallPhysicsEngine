# Sound system part 1: engine and playback — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A sound engine that renders ahead of the playhead, plays through the speakers in the editor, is exported
into the mp4, and carries a master limiter and loudness meter; plus every shared data-model change the later
parts need, so parts 3, 5 and 6 can be built in parallel without touching the same core code.

**Architecture:** New crates `bpe-dsp` (pure DSP blocks) and `bpe-sound` (the engine, which owns its own
deterministic physics world and renders 64-sample blocks). The editor runs one `Engine` on a sound thread feeding
a lock-free ring that a cpal callback drains; the device's played-sample count is the editor clock. `bpe-cli`
renders the same engine offline to a float WAV and muxes it as AAC 320 kbps.

**Tech Stack:** Rust 2024, `cpal 0.18` (audio out), `rubato 0.16` (resampler), `rtrb 0.3` (SPSC ring), `hound 3.5`
(WAV), egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` (read "Architecture" and "Part 1"). Decisions
log: `docs/superpowers/plans/sound-system/decisions.md` (append any call you make).

## Global Constraints

- 48 kHz stereo, `f32` samples, `f64` for phase and clocks; blocks of exactly 64 samples (`BLOCK = 64`).
- No allocation, locks, logging or file access inside `Engine::render` or the cpal callback.
- Same scene → same samples. Editor path from 0 = export path, sample for sample.
- Master default: true-peak ceiling **-1.0 dBTP**; LUFS meter (EBU R128) with a **-14 LUFS** mark.
- Export audio: **AAC 320 kbps, 48 kHz**. A scene without sound still exports a silent track.
- Tempo 20..300 BPM (decimals), default 120; time signature top 1..16 (default 4), bottom 2/4/8/16 (default 4).
- `Event.at` never enters `Log::hash`; physics goldens must not change.
- Scene format becomes **8**; format 7 files still load.
- Editor colours only from `style/` (the design-system test); IBM Plex fonts; knob rings use `Thing.colour`.
- Plain-language UI text; no built-in scenes or sounds.
- Windows cross-build must keep working: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio`.

## Review Focus

1. **No audio device** (CI, headless, a PC with no speakers): the editor must still open, play (wall clock) and
   export. Test in Task 9.
2. **Device not at 48 kHz** (44.1 kHz is common): sound must play at the right pitch. Test in Task 9 (resampler).
3. **Seeking while playing / looping at the end**: no stale audio from before the seek, no click. Test in Task 10.
4. **A physics edit while playing** must restart the sound world (no hits from the old layout). Test in Task 10.
5. **Very long scene (600 s) export**: the WAV must be written in streaming fashion, not one giant Vec held twice.
   Test in Task 6 (memory bound by chunked writes; check with a 60 s scene that peak buffer stays < 4 MB).

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-core/src/ids.rs` | `SlotId::Sound`, `SlotId::Mixer` |
| `crates/bpe-core/src/registry.rs` | `LayerSection::{Chain, Modulator, Effect}` |
| `crates/bpe-core/src/layers.rs` | `Layer.lane: Option<u8>` |
| `crates/bpe-core/src/sound_values.rs` (new) | `ModRoute`, `NoteSlot` |
| `crates/bpe-core/src/value.rs` | `Value::Routes`, `Value::Slots`, `ValueType::{Routes, Slots}` |
| `crates/bpe-core/src/animation.rs` | `PulseSource::Sound { shape, source }`, `SoundSource` |
| `crates/bpe-core/src/scene.rs` | `FORMAT = 8`; loading 7 still works |
| `crates/bpe-runtime/src/events.rs`, `world.rs`, `pairs.rs` | `Event.at` |
| `crates/bpe-dsp/` (new) | `smooth.rs`, `denormal.rs`, `oversample.rs`, `limiter.rs`, `loudness.rs`, `lib.rs` |
| `crates/bpe-kit/src/things/scene/mod.rs` | tempo settings, `SlotId::Mixer` allowed |
| `crates/bpe-kit/src/sound/mod.rs` (new) | registers sound card kinds; `mixer.rs`: `MixerSettings` (master part) |
| `crates/bpe-kit/src/lib.rs` | `ensure_mixer`, `new_studio_scene` adds it |
| `crates/bpe-sound/` (new) | `lib.rs`, `engine.rs`, `tempo.rs`, `hits.rs`, `voice.rs`, `master.rs`, `meters.rs`, `offline.rs`, `wav.rs` |
| `crates/bpe-cli/src/lib.rs` | audio render + mux |
| `crates/bpe-studio/src/sound_job.rs` (new) | the sound thread, patches, seeks, pre-roll |
| `crates/bpe-studio/src/audio_out.rs` (new) | cpal stream, ring, resampler, device clock |
| `crates/bpe-studio/src/style/knob.rs` (new) | the shared knob widget (used by parts 2–6) |
| `crates/bpe-studio/src/style/meter.rs` (new) | level meter and LUFS readout drawing |
| `crates/bpe-studio/src/mixer/mod.rs` (new) | Things / Mixer switch content, tempo row, Master strip (part 6 adds buses) |
| `crates/bpe-studio/src/app.rs`, `transport.rs`, `timeline/mod.rs`, `settings.rs` | clock, speaker button, bars grid, left panel switch |
| `CLAUDE.md` | crate map |

---

### Task 1: Shared data model for every sound part

**Files:**
- Modify: `crates/bpe-core/src/ids.rs`, `registry.rs`, `layers.rs`, `value.rs`, `animation.rs`, `scene.rs`, `lib.rs`
- Create: `crates/bpe-core/src/sound_values.rs`
- Modify: `crates/bpe-studio/src/inspector.rs` (card list skips the two new slots), `crates/bpe-core/src/resolve.rs` test
- Test: unit tests in each file; `crates/bpe-core/tests/` none needed

**Interfaces:**
- Produces:
  - `SlotId::Sound`, `SlotId::Mixer` appended to the enum and to `SlotId::ALL` (length 8; `slot as usize` indexes
    still hold). `SlotId::name()`: "Sound", "Mixer". `SlotId::in_card_list(self) -> bool` (false for both).
  - `LayerSection::Chain`, `LayerSection::Modulator`, `LayerSection::Effect`.
  - `Layer.lane: Option<u8>` (`#[serde(skip_serializing_if = "Option::is_none")]`), copied by `copy_layer`,
    `duplicate_layer`; `add_layer` takes it via a new `add_layer_in(…, lane: Option<u8>)` (keep `add_layer` as
    `add_layer_in(…, None)`). `move_layer` only moves among layers of the same section **and** lane.
  - `pub struct ModRoute { pub source: LayerId, pub target: String, pub amount: f32, pub bipolar: bool, pub invert: bool }`
  - `pub struct NoteSlot { pub notes: Vec<u8>, pub order_clicked: Vec<u8> }` (`notes` sorted, unique)
  - `Value::Routes(Vec<ModRoute>)`, `Value::Slots(Vec<NoteSlot>)`; `ValueType::Routes`, `ValueType::Slots`; both
    never keyframed (same rule as `Strokes`/`Points`).
  - `PulseSource::Sound { shape: ThingId, source: SoundSource }`, `pub enum SoundSource { Modulator(LayerId), Level }`.
    `PulseSource` loses `Copy` if it had it; `NAMED` stays `[Hits]` (sound sources are built per scene).
  - `remove_layer` also removes every `ModRoute` in the card's `routes` value whose `source` is that layer or
    whose `target` starts with `"<id>/"`.
  - `Scene::FORMAT = 8`.

- [ ] **Step 1: Write failing tests**
  - `ids.rs`: `sound_and_mixer_slots_are_last_and_hidden` — `SlotId::ALL.len() == 8`, `ALL[6] == Sound`,
    `ALL[7] == Mixer`, `!Sound.in_card_list()`, `Look.in_card_list()`.
  - `layers.rs`: `moving_a_chain_layer_stays_in_its_lane` — card with chain layers lanes [0,0,1]; moving the
    lane-1 layer to index 0 is refused (`EditError`), moving within lane 0 works.
  - `edit.rs`: `removing_a_modulator_drops_its_routes` — a card with `routes = [ModRoute{source: L1, target: "a_gain"},
    ModRoute{source: L2, target: "1/cutoff"}]`; removing L1 leaves only the second; removing layer 1 drops the second.
  - `save.rs`: `format_7_files_still_load` and `routes_and_slots_round_trip` (RON save/load equality).
- [ ] **Step 2: Run** `cargo test -p bpe-core` — Expected: the new tests fail to compile/fail.
- [ ] **Step 3: Implement** the items in Produces. Fix every exhaustive `match` on `SlotId`, `LayerSection` and
  `Value` across the workspace (`cargo check --workspace` lists them). Inspector card list at
  `inspector.rs:341` and `:971` filters with `in_card_list()`. Loading: accept `format <= 8`, refuse `> 8`.
- [ ] **Step 4: Run** `cargo test -p bpe-core && cargo check --workspace` — Expected: PASS, no warnings.
- [ ] **Step 5: Commit** `git commit -m "Core: sound slots, chain/modulator/effect layers, routes and note slots"`

### Task 2: Exact hit timing (`Event.at`)

**Files:**
- Modify: `crates/bpe-runtime/src/events.rs`, `world.rs` (wall hits ~line 471), `pairs.rs` (ball hits), every
  other `Event { … }` literal (testing.rs, bpe-kit paint/eval.rs, camera/sim.rs, studio ball_view.rs, gfx frame.rs:
  add `at: 0.0`)
- Test: `crates/bpe-runtime/src/world.rs` tests

**Interfaces:**
- Produces: `Event.at: f32` — fraction 0..=1 of the step at which the contact happened (0 = start of the step).
  `Event::sample_in_step(&self, samples_per_step: f64) -> f64` = `at as f64 * samples_per_step`.

- [ ] **Step 1: Failing tests**
  - `hit_fraction_matches_where_the_ball_crossed` — a ball at x=0 moving +x at 2400 px/s (10 px per 240 Hz
    step), a wall plane 4 px past its edge at the start of the step: the hit's `at` is within 0.02 of 0.4.
  - `hit_fraction_is_not_hashed` — two logs equal except `at` hash equal (extend
    `hash_changes_with_any_field_and_not_with_time`).
  - Physics goldens unchanged: `cargo test -p bpe-kit --test golden` passes without re-blessing.
- [ ] **Step 2: Run** `cargo test -p bpe-runtime` — Expected: FAIL.
- [ ] **Step 3: Implement.** For a wall hit: `at = clamp(d0 / (d0 - d1), 0, 1)` where `d0` is the ball centre's
  signed distance to the contact plane (point, normal) minus radius at `previous`, and `d1` the same at the
  position before `keep_clear_from` moved it (keep a copy). For a ball–ball hit, the same with the two centres'
  relative motion and the sum of radii. Degenerate (`d0 <= 0` or `d0 - d1 <= 1e-6`) → `at = 0`.
- [ ] **Step 4: Run** `cargo test -p bpe-runtime && cargo test -p bpe-kit --test golden` (debug) and with
  `--release` — Expected: PASS, no golden diff.
- [ ] **Step 5: Commit** `git commit -m "Runtime: hits carry where in the step they happened"`

### Task 3: `bpe-dsp` crate with the master's blocks

**Files:**
- Create: `crates/bpe-dsp/Cargo.toml` (no deps except `wide = "0.7"`), `src/lib.rs`, `smooth.rs`, `denormal.rs`,
  `oversample.rs`, `limiter.rs`, `loudness.rs`, `db.rs`
- Modify: root `Cargo.toml` workspace deps (`bpe-dsp`, `bpe-sound`, `cpal = "0.18"`, `rubato = "0.16"`,
  `rtrb = "0.3"`, `hound = "3.5"`, `wide = "0.7"`)
- Port from: `git show origin/version-1:crates/bpe-synth/src/blocks/dynamics.rs` and `maths.rs`

**Interfaces:**
- Produces:
  - `pub const SAMPLE_RATE: f32 = 48_000.0; pub const BLOCK: usize = 64;`
  - `db.rs`: `db_to_gain(db: f32) -> f32` (−inf → 0), `gain_to_db(g: f32) -> f32` (0 → `f32::NEG_INFINITY`).
  - `Smoother { new(time_ms: f32) , set_target(f32), next() -> f32, fill(&mut [f32]), value() }` — one-pole,
    reaches 99 % in `time_ms`; default 20 ms for knobs.
  - `denormal::FlushGuard::new()` (sets FTZ/DAZ on x86/x86_64 via `_mm_setcsr`, restores on drop; no-op elsewhere).
  - `oversample::Halfband` (polyphase IIR halfband, ≥ 100 dB stopband) and
    `Oversampler { new(factor: u8 /*1,2,4,8*/), up(&[f32], &mut [f32]), down(&[f32], &mut [f32]), latency() }`.
  - `limiter::TruePeakLimiter { new(), set(ceiling_db, release_ms), process(l: &mut [f32], r: &mut [f32]),
    gain_reduction_db() -> f32, latency() -> usize }` — 1.5 ms lookahead, 4× oversampled peak detection,
    guaranteed output true peak ≤ ceiling.
  - `loudness::LoudnessMeter { new(), process(l: &[f32], r: &[f32]), momentary() , short_term(), integrated(),
    true_peak_db(), reset() }` — EBU R128 K-weighting, 400 ms / 3 s windows, gated integrated.

- [ ] **Step 1: Failing tests** (in each module)
  - `smoother_reaches_99_percent_in_its_time`; `db_round_trips` (−60..+12 within 1e-4).
  - `halfband_up_down_is_transparent` (1 kHz sine through 4× up/down: error < −90 dB after latency).
  - `limiter_never_exceeds_ceiling_as_true_peak` — white noise at +12 dBFS and a 0 dBFS inter-sample-peak test
    signal (fs/4 sine at 45° phase): 4× oversampled peak of output ≤ −1.0 dBTP + 0.05 dB.
  - `limiter_is_transparent_below_ceiling` — −10 dBFS sine passes unchanged (after latency) within 1e-4.
  - `loudness_matches_tech_3341` — 1 kHz stereo sine at −23 dBFS reads −23.0 LUFS ±0.1 (momentary, short-term,
    integrated after 10 s); −20 dBFS → −20.0 ±0.1; integrated gating test 3341 case 7 style: 20 s at −36 then
    20 s at −23 → integrated −23.0 ±0.1.
- [ ] **Step 2: Run** `cargo test -p bpe-dsp` — Expected: FAIL.
- [ ] **Step 3: Implement** the blocks. No allocation after `new`.
- [ ] **Step 4: Run** `cargo test -p bpe-dsp --release` — Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "New bpe-dsp crate: smoother, oversampler, true-peak limiter, loudness meter"`

### Task 4: Tempo, the mixer card and its starting state

**Files:**
- Modify: `crates/bpe-kit/src/things/scene/mod.rs` (fields + `slots()` adds `SlotId::Mixer`), `crates/bpe-kit/src/lib.rs`
- Create: `crates/bpe-kit/src/sound/mod.rs`, `crates/bpe-kit/src/sound/mixer.rs`

**Interfaces:**
- Produces:
  - `SceneSettings.tempo: f32` (`#[setting(label = "Tempo", range(20, 300), unit = "BPM", group = "Sound")]`,
    default 120.0), `time_top: u32` ("Beats in a bar", 1..16, default 4), `time_bottom: TimeBottom`
    (`Choice`: `Two`, `Four`, `Eight`, `Sixteen`; labels "2", "4", "8", "16"; default Four). None keyframed.
    Tempo does not affect physics (`affects_physics` false).
  - `MixerSettings` (`#[derive(Settings)]`, card kind `sound.mixer`, `SlotId::Mixer`, fits only `scene`):
    `master_input` (dB −24..+24, default 0, keyframe), `master_output` (dB −24..+24, 0, keyframe),
    `master_ceiling` (dBTP −12..0, default −1.0), `master_release` (ms 1..1000, default 60).
    Part 6 adds the bus fields to this same struct.
  - `bpe_kit::sound::register(reg: &mut Registry)` called from `kit()`.
  - `pub fn ensure_mixer(scene: &mut Scene, reg: &Registry)` — adds the `sound.mixer` card to the scene settings
    thing if missing; `new_studio_scene` and the studio's `replace_scene` call it (next to `ensure_camera`).

- [ ] **Step 1: Failing tests** in `bpe-kit/src/lib.rs` tests:
  `the_blank_project_has_a_mixer_card_with_a_minus_one_ceiling` (card present in `SlotId::Mixer`, resolved
  `master_ceiling == -1.0`), `tempo_defaults_to_120_in_four_four`, `tempo_does_not_restart_the_physics`
  (fingerprint equal after changing tempo), `ensure_mixer_is_idempotent`. Update
  `the_blank_project_is_scene_settings_and_a_camera` only if it inspects slots.
- [ ] **Step 2: Run** `cargo test -p bpe-kit --lib` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-kit` — PASS (goldens untouched).
- [ ] **Step 5: Commit** `git commit -m "Kit: scene tempo and time signature, the mixer card with the master"`

### Task 5: `bpe-sound` engine skeleton

**Files:**
- Create: `crates/bpe-sound/Cargo.toml` (deps: bpe-core, bpe-runtime, bpe-kit, bpe-dsp, glam), `src/lib.rs`,
  `engine.rs`, `tempo.rs`, `hits.rs`, `voice.rs`, `master.rs`, `meters.rs`, `patch.rs`
- Test: unit tests + `crates/bpe-sound/tests/engine.rs`

**Interfaces:**
- Consumes: `Event.at` (Task 2), `MixerSettings`, `SceneSettings` tempo (Task 4), `TruePeakLimiter`,
  `LoudnessMeter`, `Smoother`, `FlushGuard` (Task 3), `bpe_runtime::Playback`.
- Produces:
  - `tempo.rs`: `pub struct TempoMap { bpm: f64, top: u32, bottom: u32 }` with `from_settings(&SceneSettings)`,
    `beat_seconds() -> f64` (quarter-note beat scaled by bottom: `60/bpm * 4/bottom`), `bar_seconds()`,
    `beat_at(time) -> f64`, `bar_at(time) -> f64`, `seconds_of_beats(beats) -> f64`. One place for all tempo maths.
  - `hits.rs`: `pub struct BlockHit { pub offset: u32 /* sample in block */, pub event: Event }`;
    `pub struct HitCursor` turning the playback log into per-block hit lists (sample index of a hit =
    `round((step - 1) * sps + at * sps)` where `sps = 48000 / steps_per_second` as f64; hits land in the block
    that contains that sample).
  - `voice.rs`: `pub trait VoiceSource: Send { fn thing(&self) -> ThingId; fn on_block(&mut self, cx: &BlockCx,
    hits: &[BlockHit], out_l: &mut [f32; BLOCK], out_r: &mut [f32; BLOCK]); fn bus(&self) -> u8; fn reset(&mut self); }`
    and `pub struct BlockCx<'a> { pub start_sample: u64, pub time: f64, pub tempo: &'a TempoMap, pub world: &'a World }`.
    Part 2 adds the shape voice; part 1 has only `#[cfg(any(test, feature = "test-tone"))] TestTone` (a 2 ms,
    1 kHz blip at −12 dBFS on each hit on its shape).
  - `patch.rs`: `pub struct SoundPatch` — everything the engine reads from the scene, built by
    `SoundPatch::build(scene: &Scene, resolved: &Resolved, kit: &Kit) -> SoundPatch` (master settings, tempo, and
    a `Vec<Box<dyn VoiceSource>>`). `VoiceSource` also has `fn adopt(&mut self, old: &mut dyn VoiceSource)` (default
    no-op) and `fn as_any_mut(&mut self) -> &mut dyn Any`: on `swap_patch` the engine calls `new.adopt(old)` for
    voices whose `thing()` matches, so ringing notes carry over (part 2 implements it for shapes). Patches are built
    on the sound thread outside `render`, so building may allocate.
  - `master.rs`: `pub struct Master` (input gain → limiter → output gain, all smoothed) with `process`.
  - `meters.rs`: `pub struct Meters` of atomics (`AtomicU32` holding f32 bits): peak L/R, momentary, short-term,
    integrated LUFS, true peak, master gain reduction. `pub fn read(&self) -> MeterReading` (plain copy).
    Lock-free; written once per block.
  - `engine.rs`: `pub struct Engine` with `new(scene: &Scene, resolved: &Resolved, kit: &Kit) -> Engine`,
    `render(&mut self, out: &mut [f32; BLOCK * 2])` (interleaved stereo), `sample_position() -> u64`,
    `time() -> f64`, `seek(&mut self, time: f64)` (see Task 10), `swap_patch(&mut self, patch: SoundPatch)`,
    `meters(&self) -> Arc<Meters>`. `render` steps the engine's own `Playback` forward to cover the block, collects
    its hits, calls each voice, sums into buses (part 1: everything to the master), runs the master, updates meters.
  - `SAMPLES_PER_BLOCK_TIME: f64 = 64.0 / 48000.0`.

- [ ] **Step 1: Failing tests** (`tests/engine.rs`, using `bpe_kit::scenes` test scenes with a `TestTone` on the
  shape via the `test-tone` feature)
  - `rendering_twice_gives_the_same_samples` — 5 s render, compare all samples bit-equal.
  - `a_hit_starts_on_its_exact_sample` — scripted one-hit scene; the first non-zero sample index equals the
    computed index from the hit's step and `at` (±0).
  - `silence_without_sound_cards` — the blank project renders exact zeros.
  - `render_does_not_allocate` — a counting `#[global_allocator]` in this test binary: after warm-up, 1000 blocks
    allocate 0 times (physics stepping included: if `World::step` allocates, record it in decisions.md and exclude
    the step call by stepping into a preallocated log; the voice/bus/master path must be 0).
  - `tempo_map_maths` (unit): 120 BPM 4/4 → beat 0.5 s, bar 2 s; 6/8 at 120 → beat 0.25 s, bar 1.5 s.
- [ ] **Step 2: Run** `cargo test -p bpe-sound --features test-tone` — FAIL.
- [ ] **Step 3: Implement.** Wrap `render` in `FlushGuard`. Engine physics uses its own `Playback` built from the
  same scene/resolved; no sharing with the editor's playback.
- [ ] **Step 4: Run** the same — PASS.
- [ ] **Step 5: Commit** `git commit -m "New bpe-sound crate: the block engine, tempo map, exact hits and master"`

### Task 6: Offline render and WAV

**Files:**
- Create: `crates/bpe-sound/src/offline.rs`, `wav.rs`
- Test: `crates/bpe-sound/tests/offline.rs`

**Interfaces:**
- Produces: `pub fn render_to_wav(scene: &Scene, kit: &Kit, path: &Path, seconds: f64, progress: impl FnMut(f64) -> bool) -> anyhow::Result<()>`
  (32-bit float stereo 48 kHz via `hound`, written in 1 s chunks so memory stays flat; returning `false` from
  `progress` cancels) and `pub fn render_samples(scene, kit, seconds) -> Vec<f32>` for tests.
  `pub fn has_sound(scene: &Scene) -> bool` (any enabled `sound.voice` card).

- [ ] **Step 1: Failing tests:** `wav_matches_render_samples` (read back with hound, bit-equal);
  `long_render_keeps_memory_flat` (60 s blank+tone scene; track max chunk buffer size < 4 MB via a counter in
  `offline.rs` exposed `#[cfg(test)]`); `cancel_stops_early`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `git commit -m "Sound: offline render to a float WAV"`

### Task 7: Export carries the soundtrack

**Files:**
- Modify: `crates/bpe-cli/Cargo.toml` (add `bpe-sound`, `tempfile = "3"`), `crates/bpe-cli/src/lib.rs:255-300`

**Interfaces:**
- Consumes: `bpe_sound::render_to_wav`.
- Produces: `export_with` renders audio first to `tempfile::NamedTempFile` (".wav") with progress counted as the
  first 10 % of the bar (`progress` callback still receives frame counts: map audio progress to
  `(0..total/10)` then frames to the rest), then ffmpeg gets `-i <wav>` instead of `anullsrc`, `-map 1:a`,
  `-c:a aac -b:a 320k -ar 48000`. Length = video length (`-shortest` stays).

- [ ] **Step 1: Failing test** in `crates/bpe-cli/tests/` (gated on ffmpeg being present, as existing export tests
  are): `exported_mp4_has_320k_aac_with_the_tone` — export a 2 s test-tone scene, `ffprobe` shows `aac`,
  `bit_rate` ≥ 300000, and decoding the audio (`ffmpeg -f f32le -`) gives non-silent samples near the first hit.
  Also `blank_scene_exports_a_silent_track`.
- [ ] **Step 2–4:** run, implement, run `cargo test -p bpe-cli` — PASS.
- [ ] **Step 5: Commit** `git commit -m "Export: the real soundtrack, AAC 320 kbps"`

### Task 8: The shared knob and meter widgets

**Files:**
- Create: `crates/bpe-studio/src/style/knob.rs`, `crates/bpe-studio/src/style/meter.rs`; modify `style/mod.rs`
- Test: kittest tests in `knob.rs`

**Interfaces:**
- Produces (used by every later part):
  - `pub struct Knob<'a> { value: &'a mut f64, range: RangeInclusive<f64>, default: f64, label: &'a str,
    format: &'a dyn Fn(f64) -> String, bipolar: bool, rings: &'a [ModRing], colour: Color32, log: bool }`
    with builder methods and `impl Widget`. Size 40×40 dial + label + value (as the canvas boards: track arc
    270°, value arc in `MUTED`, pointer in `TEXT_STRONG`, mod rings at radius 18 in `colour`).
    Behaviour: vertical drag (200 px = full range; Shift = ×0.1), wheel nudges 1 %, double-click dial resets to
    `default`, double-click value text opens a `TextEdit` that parses numbers with units, right-click yields
    `KnobResponse::context` so callers attach the existing value menu (`widgets.rs` right-click Copy/Paste/Create
    keyframe). Accessible: `labelled_by` the label.
  - `pub struct ModRing { pub from: f64, pub to: f64 }` (in knob units).
  - `pub fn knob(ui, value, range, default, label) -> egui::Response` convenience.
  - `meter.rs`: `pub fn level_meter(ui, peak_db_l: f32, peak_db_r: f32, height: f32)`,
    `pub fn loudness_row(ui, reading: &MeterReading)` (Momentary / Short-term / Whole clip / True peak readouts in
    IBM Plex Mono) and `pub fn loudness_graph(ui, history: &[f32], mark: f32 /* -14 */)`.
- [ ] **Step 1: Failing kittest tests:** `dragging_up_raises_the_value`, `shift_drag_is_ten_times_finer`,
  `double_click_resets`, `typing_a_value_sets_it` ("−6 dB" → −6.0), `knob_is_named_by_its_label`.
- [ ] **Step 2–4:** run `cargo test -p bpe-studio knob` — FAIL, implement, PASS; design-system colour test passes.
- [ ] **Step 5: Commit** `git commit -m "Studio: knob and meter widgets"`

### Task 9: Audio output and the sound thread

**Files:**
- Create: `crates/bpe-studio/src/audio_out.rs`, `crates/bpe-studio/src/sound_job.rs`
- Modify: `crates/bpe-studio/Cargo.toml` (cpal, rubato, rtrb, bpe-sound), `app.rs` (clock), `settings.rs`
  (listening volume + speaker on, persisted)

**Interfaces:**
- Consumes: `Engine` (Task 5).
- Produces:
  - `audio_out.rs`: `pub struct AudioOut` — `open() -> Option<AudioOut>` (default output device, f32 or i16
    format, any rate; `None` if no device or it fails, never panics), `ring_writer()` (an `rtrb::Producer<f32>`
    of 0.25 s at 48 kHz interleaved), `played_samples() -> u64` (an `AtomicU64` the callback advances by
    48 kHz-equivalent frames), `set_volume(f32)`, `set_muted(bool)`, `device_rate() -> u32`. When the device rate
    ≠ 48000, the callback side resamples with `rubato::SincFixedOut` (sinc len 128, Blackman-Harris) from a small
    48 kHz pre-buffer. Underrun → writes silence and does not advance `played_samples`.
  - `sound_job.rs`: `pub struct SoundJob` — `start(scene, resolved, kit, out: &AudioOut) -> SoundJob` spawns a
    thread that owns the `Engine`, keeps the ring ≥ 50 ms (2400 frames) ahead, and takes commands over a bounded
    `std::sync::mpsc`: `Play`, `Pause`, `Seek(f64)`, `Patch(SoundPatch)`, `Rebuild(Scene, Resolved)`, `Audition(…)`
    (part 2 uses), `Quit`. `clock() -> Option<f64>` = time at the speakers = `seek_base + played_since_seek / 48000`.
    `meters() -> Arc<Meters>`.
  - `Studio.sound: Option<SoundJob>`; `Studio::tick` uses `sound.clock()` while playing when present, otherwise
    the wall clock as today. When the ring is starved (render slower than real time), the clock simply stops
    advancing (picture waits with sound).
  - `Studio.listen: ListenSettings { on: bool, volume: f32 /* 0..1, default 0.8 */ }` saved with
    `settings::save_listen`.
- [ ] **Step 1: Failing tests** (no real device needed):
  - `no_device_falls_back_to_the_wall_clock` — `Studio::headless()` has `sound == None` and playing advances time.
  - `resampler_keeps_pitch` — feed the resampling path (factor 48000→44100) a 1 kHz sine; FFT peak of the
    output at 1000 Hz ±2 Hz at 44.1 kHz.
  - `the_job_stays_ahead` — a `SoundJob` with a fake sink (expose `SoundJob::start_with_sink` taking an
    `rtrb::Producer` and an `Arc<AtomicU64>` played counter for tests): after 100 ms of consuming at real rate,
    the ring holds ≥ 2400 frames.
- [ ] **Step 2–4:** run `cargo test -p bpe-studio sound_job audio_out` — FAIL, implement, PASS.
- [ ] **Step 5: Commit** `git commit -m "Studio: sound thread and speaker output; the picture follows the sound clock"`

### Task 10: Seeking, pre-roll and edits while playing

**Files:**
- Modify: `crates/bpe-sound/src/engine.rs` (`seek`), `crates/bpe-studio/src/sound_job.rs`, `app.rs`
  (`seek`, `scene_changed`, `set_playing`)

**Interfaces:**
- Produces: `Engine::seek(&mut self, time: f64)` — resets every voice, bus and the master, seeks its physics to
  `max(0, time − PRE_ROLL)` where `pub const PRE_ROLL: f64 = 4.0`, then renders silently (discarding output) up
  to `time` rounded down to a block boundary. `Studio::scene_changed`: if the physics fingerprint changed →
  `SoundJob::Rebuild`; otherwise → `SoundJob::Patch(SoundPatch::build(...))`. `Studio::seek` and loop wrap send
  `Seek` and clear the ring (`SoundJob` drains the consumer by bumping a generation counter the callback checks;
  stale frames are dropped, a 2 ms fade-in hides the cut).
- [ ] **Step 1: Failing tests** (`bpe-sound/tests/engine.rs`):
  - `seek_with_pre_roll_matches_a_straight_render` — a scene whose tone has a 2 s decay (use `TestTone` with
    `decay_seconds` param): render straight to 10 s; separately `seek(6.0)` then render to 10 s; samples from
    6 s on match within −120 dB.
  - `a_physics_edit_rebuilds_and_a_tempo_edit_patches` (studio test via `Studio::headless` with a fake job that
    records commands).
  - `seeking_clears_old_audio` — after `Seek`, the first frames the fake sink reads are from the new time
    (marker: the tone's sample index).
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `git commit -m "Sound: pre-roll on seek; edits while playing patch or rebuild"`

### Task 11: Timeline bars and beats, speaker button

**Files:**
- Modify: `crates/bpe-studio/src/timeline/mod.rs` (ruler + lanes background), `transport.rs` (speaker button and
  volume), `style/` (an icon painter for the speaker; no new colours outside style)

**Interfaces:**
- Consumes: `TempoMap` (Task 5).
- Produces: `fn beat_lines(ruler: &Ruler, tempo: &TempoMap) -> Vec<(f32, bool /* bar */, Option<u32> /* bar no */)>`
  in `timeline/mod.rs` — faint beat lines (`LINE_SOFT`), bar lines (`LINE`), bar numbers on the ruler from 1.
  Hides beat lines when they would be < 4 px apart, then bar lines < 4 px (every 2nd, 4th… bar). Key snapping
  (`snap`) also snaps to beats when the grid is on. Transport: a speaker toggle (mute) and a 60 px volume slider
  "Listening volume", hover text "Only what you hear; the export is not changed".
- [ ] **Step 1: Failing tests:** `bar_lines_follow_the_tempo` (120 BPM 4/4, 8 s → bars at 0, 2, 4, 6 s),
  `dense_beats_are_thinned`, kittest `the_speaker_button_mutes_without_editing_the_scene`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `git commit -m "Timeline: bars and beats from the tempo; transport: speaker and listening volume"`

### Task 12: Things / Mixer switch with the Master strip

**Files:**
- Create: `crates/bpe-studio/src/mixer/mod.rs`, `crates/bpe-studio/src/mixer/master.rs`
- Modify: `app.rs` (left panel: a two-chip switch on top; Mixer mode sets the panel's width to 420 px and
  restores the tree width on going back), `Panels` gains `mixer: Mixer`

**Interfaces:**
- Consumes: `Knob`, meters widgets (Task 8), `MixerSettings` (Task 4), `Meters` (Task 5).
- Produces: `pub enum LeftPanel { Things, Mixer }` in `app.rs` (`Studio.left: LeftPanel`, not saved in the
  scene); `mixer::show(ui, &mut Mixer, &mut Scene, &Kit, &Resolved, meters: Option<&MeterReading>, time: f64)`.
  Part 1 content: the Tempo row (Tempo field, Beats in a bar field, bottom picker as a `menu_button`), then a
  "Master" strip: Input, Output, Ceiling, Release knobs (edit `master_*` on the mixer card through the normal
  scene edit path so undo works), stereo peak meter, loudness readouts and the loudness graph with the −14 mark.
  `pub fn bus_strips(...)` is an empty extension point part 6 fills. Keep the Master strip a separate function
  `master::strip(ui, …)` so part 6 only adds columns beside it.
- [ ] **Step 1: Failing kittest tests:** `switching_to_mixer_shows_tempo_and_master`, `turning_the_ceiling_knob_edits_the_mixer_card_and_undoes`,
  `switching_back_restores_the_tree_width`.
- [ ] **Step 2–4:** FAIL, implement, PASS.
- [ ] **Step 5: Commit** `git commit -m "Studio: Things / Mixer switch with tempo and the Master strip"`

### Task 13: Finish the part

- [ ] **Step 1:** Update `CLAUDE.md` crate map: `bpe-dsp`, `bpe-sound`, the new slots, the sound thread, export
  audio, and add `libasound2-dev` note already present (cpal). Note the `test-tone` feature.
- [ ] **Step 2:** Run `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (background),
  `cargo test --workspace --release` (expect only `pushing_an_earlier_step_panics_in_debug` to fail), physics
  goldens unchanged (diff vs main is empty).
- [ ] **Step 3:** Windows cross-build of `bpe-studio` and `bpe-cli` succeeds.
- [ ] **Step 4:** One Opus review of the whole branch (per project rules), fix findings.
- [ ] **Step 5:** Push and open the PR (attribution block first, assign and request review from `pixelpitch161`).
  Copy the built exes to `/mnt/project-files/ballphysics/sound-system/part1/` (the owner's PC is off; no F:
  install until he says "merge").
