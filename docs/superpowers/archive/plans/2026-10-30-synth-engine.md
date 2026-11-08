# Synth Engine and Instruments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sound modules play library Instruments, either one of the 13 built-in sounds or a modular synth patch built from oscillator, filter, envelope and LFO blocks in the node panel. Every sound module renders as its own channel through a voice engine.

**Architecture:** A new crate, `bpe-synth`, holds the DSP. It has no idea what a scene is. It provides block structs, a `bpe_graph` catalog, a compiler from a patch graph to a flat `Program` over 64-frame stereo registers, and a `VoicePool` that plays notes through a program. `bpe-core` gains `Instrument` library items, built the same way as Progressions in part 1. `bpe-mix` stops rendering notes one-shot and summing them. It renders one channel per sound module in parallel, then sums the channels in a fixed order. Built-in instruments keep the one-shot `NoteCache` path inside their channel. The studio gets an Instrument picker, an instrument section on sound cards, patch editing in the node panel, and a keyboard strip.

**Tech Stack:** Rust 2024, egui 0.37 / egui_kittest, the existing `bpe-graph`, `hound` for WAV files.

**Spec:** `docs/superpowers/specs/2026-10-30-music-synth-design.md`, sections 2.1 to 2.5, 2.4's channel rendering, 6 (preview speed), 7 (errors and limits) and 8 (testing). Effects (2.6), channels, buses and the master (2.7, 3), and physics inputs (4) belong to parts 3 and 4.

## Global Constraints

- Sample rate is `bpe_mix::SAMPLE_RATE` (48 kHz). DSP is f32, a block is 64 frames, and audio is stereo.
- Rendering is deterministic: a voice or channel runs on one thread, and channels are summed in ascending channel order. The same scene gives the same samples every time.
- Our own DSP, not fundsp. No new audio dependency apart from `hound`, which is already in the workspace.
- Old scenes need not load or sound the same (Aedan, 2026-10-30). Starter scenes, presets and golden soundtrack hashes move to the new model. Event-log goldens must not change.
- UI text: plain words, b and # for accidentals (the editor font has no ♭, ♯ or ▶).
- The voice limit is per channel, default 16, and the oldest voice is stolen with a 5 ms fade. A hard cap of 256 sounding voices applies across the whole mix. A voice ends once it has stayed under -90 dBFS for 50 ms after the gate closes, or 10 s after the gate closes, whichever comes first.
- A patch that fails to compile plays silence and shows its problem on the block and in the status line. Non-finite samples are replaced with 0, the voice is ended, and the status line names the instrument.
- Part 2 scope notes: (a) macros take keyframes and pulses in part 4, together with physics modulation; this part gives them sliders and per-module overrides; (b) the instrument's own effects graph arrives with effects in part 3, so `Instrument` has no `fx` field yet; (c) feedback loops need the Delay block from part 3, so this part rejects every loop; (d) the Drum machine module keeps its built-in kit.

## Review Focus

1. **A broken patch**, such as no Voice out, a required input left unwired, or a loop: the scene still renders, that channel is silent, and the status line says why. Covered in Task 3 and Task 7.
2. **A module pointing at a removed instrument id**: it plays Scene defaults' instrument, or Pluck when Scene defaults has none, and nothing panics. Covered in Task 7.
3. **Extreme knob values**: resonance at its maximum, FM index 10, cutoff above Nyquist, unison 8 at full detune. The output stays finite and the peak stays within 4.0. Covered in Task 2.
4. **Thousands of overlapping notes**: the per-channel limit and the global 256 cap hold, and two renders are bit-identical. Covered in Task 4 and Task 7.
5. **Notes near the video's end, and a note length of 0**: the soundtrack keeps its length, the tails are cut at the end, and a zero-length note still plays its attack and release. Covered in Task 4.

---

### Task 1: Audio pins in bpe-graph

**Files:**
- Modify: `crates/bpe-graph/src/lib.rs`, `crates/bpe-studio/src/nodes/draw.rs` (pin colour), and any exhaustive `match` on `PinType` that the compiler flags.

**Interfaces:**
- Produces: `PinType::Audio`, named "sound". `Audio.accepts(Number)` and `Number.accepts(Audio)` are both true. An audio signal used as control takes its block average, and a control value used as audio is held across the block.

- [ ] **Step 1: Write the failing test.** `audio_and_number_pins_connect_both_ways`: assert `PinType::Audio.accepts(PinType::Number)`, `PinType::Number.accepts(PinType::Audio)`, `!PinType::Audio.accepts(PinType::Color)` and `PinType::Audio.name() == "sound"`.
- [ ] **Step 2: Run** `cargo test -p bpe-graph audio_and_number`. It should FAIL (no variant).
- [ ] **Step 3: Implement** the variant. In `draw.rs`, give audio pins the colour `theme::HOME`.
- [ ] **Step 4: Run** `cargo test -p bpe-graph && cargo check --workspace --all-targets`. It should PASS.
- [ ] **Step 5: Commit** `feat(graph): audio pins`.

---

### Task 2: bpe-synth blocks

**Files:**
- Create: `crates/bpe-synth/Cargo.toml` (deps: `bpe-graph`, `hound`), `crates/bpe-synth/src/lib.rs`, and `crates/bpe-synth/src/blocks/{mod,osc,noise,filter,ladder,drive,amp,adsr,lfo,hold,maths}.rs`.
- Modify: root `Cargo.toml` (`bpe-synth = { path = "crates/bpe-synth" }`).

**Interfaces:**
- Produces:
  - `pub const BLOCK: usize = 64;` and `pub type Buf = [[f32; BLOCK]; 2];`.
  - `pub struct Ctx { pub rate: f32, pub note: f32 /* MIDI pitch, glided */, pub velocity: f32, pub gate: bool, pub since_on: f32 /* s */, pub seconds_per_beat: f32 }`.
  - `pub trait Block: Send { fn reset(&mut self, cx: &Ctx); fn process(&mut self, cx: &Ctx, inputs: &[&Buf], outputs: &mut [Buf]); fn idle(&self) -> bool { true } }`, where `idle` is false while an ADSR is above zero. Each block reads its knobs once at construction: `fn new(params: &Knobs) -> Self`, with `pub struct Knobs(BTreeMap<String, bpe_graph::Param>)` and helpers `number(name, default)` and `choice(name, default)`.
  - Blocks and their knobs (names exactly as the catalog in Task 3 lists them):
    - `Osc`:
      - Knobs: `wave` (sine, triangle, saw, square, pulse), `width` 0.01–0.99, `semitones` -48–48, `cents` -100–100, `unison` 1–8, `detune` 0–100 cents, `spread` 0–1, `fm` 0–10 (phase-modulation index), `phase` (note, free).
      - Inputs: `pitch` (semitone offset), `fm`, `sync`.
      - Output: `out`.
      - It follows the note's pitch, uses polyBLEP on saw, square and pulse, and spreads its unison voices across the stereo field.
    - `Noise`: `colour` (white, pink). Output `out`. Seeded from the note's frame and key, so the same note gives the same noise.
    - `Svf`: `mode` (low, high, band, notch), `cutoff` 20–20000 Hz, `resonance` 0–1, `key_track` 0–1. Inputs `in` and `cutoff_mod` (octaves). Output `out`. Uses the Cytomic/Simper trapezoidal SVF, and clamps the cutoff to 0.45 × the sample rate.
    - `Ladder`: `cutoff`, `resonance` 0–1, `drive` 0–1. Same pins as `Svf`.
    - `Drive`: `mode` (soft, hard, fold), `amount` 0–1 (gain 1 to 40), `bias` -1–1.
    - `Amp`: `gain` -60–12 dB. Inputs `in` and `mod` (multiplier).
    - `Adsr`: `attack`, `decay` and `release` 0–10 s, `sustain` 0–1, `curve` -1–1. Output `env`. Gated by `cx.gate`.
    - `Lfo`: `wave` (sine, triangle, saw, square, random), `rate` 0.01–50 Hz, `sync` (off, 1/1, 1/2, 1/4, 1/8, 1/16, 1/4T, 1/8T), `retrigger` bool, `phase` 0–1. Output `out` in -1 to 1.
    - `Hold` (sample and hold): `source` (noise, input), `rate`, `sync`. Input `in`.
    - Maths: `Mix` (in1–in4 with `level1`–`level4`), `Add`, `Multiply`, `Scale` (`scale`, `offset`), `Crossfade` (a, b, `mix`), `PitchToHz` (in = MIDI pitch, out = Hz), `NoteIn` (outputs pitch, velocity, gate, time), `Macro` (`value`, `min`, `max`; output = min + value × (max − min)), `VoiceOut` (`left` required, `right` optional; mono copies to both sides).
- [ ] **Step 1: Write the failing tests** in each block file, using a helper `run(block, cx, inputs, blocks: usize) -> Vec<[f32; 2]>`:
  - `osc_pitch_is_within_a_cent`: a sine at note 69 has a zero-crossing period of 440 Hz ± 1 cent over 1 s.
  - `saw_and_square_alias_below_minus_60_db_at_5_khz`: the energy at non-harmonic FFT bins is at least 60 dB under the fundamental. Use a plain DFT at the alias bins, with no new dependency.
  - `svf_low_pass_is_3_db_down_at_cutoff`: at resonance 0 and cutoff 1 kHz, a 1 kHz sine comes out within 1 dB of -3 dB.
  - `adsr_stages_land_within_one_block`: with attack 0.1, decay 0.1, sustain 0.5 and release 0.2, the envelope reaches its peak by frame 4800 ± 64, sits at 0.5 ± 0.01 at 0.3 s, and reaches 0 by 0.2 s after the gate closes (± 64 frames).
  - `synced_lfo_follows_the_tempo`: at 1/4 sync and 0.5 s per beat, the cycle is 24 000 frames ± 1.
  - `extreme_knobs_stay_finite` (Review Focus 3):
    - Resonance 1 on both filters, fm 10, a cutoff of 30 kHz and unison 8 at detune 100.
    - Over 2 s, every sample is finite and |sample| ≤ 4.0.
  - `noise_repeats_for_the_same_note`.
- [ ] **Step 2: Run** `cargo test -p bpe-synth`. It should FAIL.
- [ ] **Step 3: Implement** the blocks.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. It should PASS.
- [ ] **Step 5: Commit** `feat(synth): oscillator, noise, filters, drive, envelopes, LFO and maths blocks`.

---

### Task 3: Catalog and compiler

**Files:**
- Create: `crates/bpe-synth/src/catalog.rs` and `crates/bpe-synth/src/program.rs`.

**Interfaces:**
- Consumes: Task 1 `PinType::Audio`, Task 2 blocks.
- Produces:
  - `pub fn catalog() -> &'static SpecList`. It holds one `NodeSpec` per block, with kinds named `osc`, `noise`, `filter`, `ladder`, `drive`, `amp`, `adsr`, `lfo`, `hold`, `mix`, `add`, `multiply`, `scale`, `crossfade`, `pitch_to_hz`, `note`, `macro`, `voice_out` (and `sampler` from Task 5). The categories are Sources, Shaping, Modulators, Maths, Inputs and Output. `cycles: false`.
  - `pub fn compile(graph: &Graph) -> Result<Program, Vec<Diagnostic>>`. It orders the blocks with `bpe_graph::order` and gives each output pin a register. Unwired inputs get `None`, and the block uses its default. Disabled blocks pass their first input through. The errors are:
    - "Add a Voice out block" (when there are 0, or more than 1).
    - "Wires go round in a loop" (on the blocks in the loop).
    - The diagnostics from `bpe_graph::validate`.
  - `pub struct Program { /* ops, register count */ }` with `pub fn instantiate(&self, cx: &Ctx) -> Patch`, and `Patch::process(&mut self, cx: &Ctx, out: &mut Buf)`. `Patch::idle()` is true when every block is idle.
  - `pub fn macros(graph: &Graph) -> Vec<MacroKnob>` with `pub struct MacroKnob { pub node: u32, pub param: String, pub label: String /* "<block title> <knob label>" */, pub min: f32, pub max: f32, pub value: f32 }`, taken from `Node.exposed`. `pub fn with_macros(graph: &Graph, values: &BTreeMap<String, f32>) -> Graph` overrides the values, keyed `"<node>.<param>"`.
  - `pub fn blank_patch() -> Graph`: Note → Osc (saw) → Filter → Amp ← ADSR, then Amp → Voice out.
- [ ] **Step 1: Write the failing tests:**
  - `blank_patch_compiles_and_sounds`: 0.5 s of note 60 has an RMS above 0.01.
  - `no_voice_out_is_an_error_and_plays_silence` (Review Focus 1): the error message is "Add a Voice out block".
  - `a_loop_is_rejected`, which names the looping blocks.
  - `control_into_audio_and_back`: an LFO wired into the Amp's `mod`, then Amp into an Add's control input, compiles.
  - `macros_list_exposed_knobs_and_override`.
  - `two_patches_render_identically`: the same samples bit for bit.
- [ ] **Step 2: Run** `cargo test -p bpe-synth`. It should FAIL.
- [ ] **Step 3: Implement.** A control output fills its register with a linear ramp from last block's value to the new one. An audio register used as control is averaged.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. It should PASS.
- [ ] **Step 5: Commit** `feat(synth): catalog and patch compiler`.

---

### Task 4: Voices

**Files:**
- Create: `crates/bpe-synth/src/voices.rs`

**Interfaces:**
- Consumes: Task 3 `Program`.
- Produces:
  - `pub struct VoiceSettings { pub note_length: NoteLength, pub limit: u16, pub mono: bool, pub glide: f32, pub retrigger: bool }`, where `NoteLength` is `Beats(f32)` or `Seconds(f32)`. The default is `Beats(1.0)`, 16, false, 0.1 and true. These types live in bpe-core (Task 6) and are re-exported here, so the field names must match.
  - `pub struct PlayNote { pub frame: usize, pub key: u8, pub velocity: f32, pub left: f32, pub right: f32, pub steal_at: Option<usize> }`.
  - `pub fn render_channel(program: &Program, notes: &[PlayNote], voices: &VoiceSettings, seconds_per_beat: f32, frames: usize) -> ChannelAudio` with `pub struct ChannelAudio { pub samples: Vec<f32> /* interleaved, frames*2 */, pub bad_samples: bool }`.
  - `pub fn global_steals(notes: &mut [(usize /*channel*/, PlayNote)], gate_frames: impl Fn(usize) -> usize, cap: usize /*256*/)`. It sets `steal_at` on the oldest notes whenever more than `cap` overlap, counting each note from its frame through its gate plus 1 s. It is deterministic and runs before the parallel render.
- [ ] **Step 1: Write the failing tests:**
  - `a_voice_ends_after_its_release`: a note with a 0.1 s gate and a 0.2 s release is silent (< -90 dBFS) from 0.35 s on.
  - `the_oldest_voice_is_stolen_with_a_fade`: with limit 2 and three overlapping notes, the first note fades over 240 frames and the step between neighbouring frames never exceeds 0.05.
  - `mono_glides_between_notes`: with glide 0.1 s, the pitch moves from note 60 to 72 over 4800 ± 64 frames, and with retrigger off the envelope doesn't restart.
  - `thousands_of_notes_are_capped_and_repeatable` (Review Focus 4): 3 000 notes in 2 s on 4 channels; `global_steals` leaves at most 256 sounding at any frame, and two runs are bit-identical.
  - `notes_at_the_end_are_cut_and_zero_length_plays` (Review Focus 5): a note 10 frames before the end keeps the buffer length exact, and `Seconds(0.0)` still has energy in its first 10 ms.
  - `non_finite_output_ends_the_voice_and_flags_it`, using a test-only block that outputs NaN.
- [ ] **Step 2: Run** `cargo test -p bpe-synth voices`. It should FAIL.
- [ ] **Step 3: Implement.** Process voices in note order, block by block, and sum them with each note's `left` and `right` gains.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. It should PASS.
- [ ] **Step 5: Commit** `feat(synth): voices, stealing, mono and glide`.

---

### Task 5: Sampler block

**Files:**
- Create: `crates/bpe-synth/src/blocks/sampler.rs`
- Modify: `catalog.rs` (kind `sampler`, category Sources)

**Interfaces:**
- Knobs:
  - `file` (Text): a WAV file, or a folder of WAVs named by note (`C4.wav`, `F#3.wav`).
  - `root` 0–127 (default 60): used for a single file.
  - `start` 0–1 and `loop` bool.
- It plays the nearest sample in the folder, pitched by playback rate with linear interpolation. Loaded files are cached in a process-wide `Mutex<BTreeMap<PathBuf, Arc<Vec<[f32; 2]>>>>`. A missing file plays silence, and `pub fn missing_samples() -> Vec<String>` lists such files for the status line.
- [ ] **Step 1: Write the failing tests:**
  - `a_sample_plays_at_its_root_and_an_octave_up_twice_as_fast`, using a 440 Hz WAV written to a temp dir with `hound`.
  - `a_folder_picks_the_nearest_note`.
  - `a_missing_file_is_silent_and_listed`.
- [ ] **Step 2: Run** `cargo test -p bpe-synth sampler`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. It should PASS.
- [ ] **Step 5: Commit** `feat(synth): sampler block`.

---

### Task 6: Instruments in the library

**Files:**
- Create: `crates/bpe-core/src/instrument.rs`
- Modify: `crates/bpe-core/src/{lib,library,scene,value,edit}.rs`

**Interfaces:**
- Produces (bpe-core):
  - `InstrumentId(u32)`.
  - `pub struct Instrument { pub name: String, pub source: InstrumentSource, pub voices: VoiceSettings }`.
  - `pub enum InstrumentSource { BuiltIn { which: String /* bpe_mix::Instrument snake_case name */, knobs: BTreeMap<String, f32> /* pluck: decay, brightness */ }, Patch { graph: bpe_graph::Graph } }`.
  - `VoiceSettings`, `NoteLength` (from Task 4, defined here).
  - `Library.instruments: BTreeMap<InstrumentId, Instrument>` (skip when empty) and `Scene.next_instrument`.
  - `pub struct InstrumentRef(pub Option<InstrumentId>)`: a SettingValue with `ValueType::Instrument`, stored as `Value::Int(-1 | id)`. `None` means "Scene defaults' instrument".
  - `Module.macros: BTreeMap<String, f32>`: per-module overrides of its instrument's macros, keyed `"<node>.<param>"` (skip when empty).
  - Edits, mirroring progressions:
    - `add_instrument`, `set_instrument`, `copy_instrument` (adds " copy" to the name), `instrument_users(reg, id) -> Vec<Address>` and `remove_instrument` (`EditError::InstrumentInUse` / `MissingInstrument`).
    - `set_macro(module, key, Option<f32>)`.
- [ ] **Step 1: Write the failing tests:**
  - `an_instrument_ref_round_trips`.
  - `copying_an_instrument_names_it_copy`.
  - `an_instrument_in_use_cannot_be_removed`.
  - `an_instrument_saves_and_loads` (RON round trip of a patch instrument).
- [ ] **Step 2: Run** `cargo test -p bpe-core instrument`. It should FAIL.
- [ ] **Step 3: Implement** (bpe-core gains a `bpe-graph` dependency if it doesn't have one).
- [ ] **Step 4: Run** `cargo test -p bpe-core`. It should PASS.
- [ ] **Step 5: Commit** `feat(core): instruments as library items`.

---

### Task 7: Channel rendering in bpe-mix

**Files:**
- Modify: `crates/bpe-mix/{Cargo.toml (+bpe-synth), src/lib.rs, src/audition.rs}`, plus the `Note` users in `crates/bpe-kit/src/modules/{sound,melody,drums}/mix.rs`.

**Interfaces:**
- Consumes: Tasks 3, 4 and 6.
- Produces:
  - `Note` changes:
    - `instrument: NoteInstrument`, where `enum NoteInstrument { Library(InstrumentId), BuiltIn(Instrument) }`. Drums use `BuiltIn(Drums)`.
    - New field `channel: u32`. `notes_with` sets it to the sound module's `ModuleId.0` after the module returns its notes, so modules don't set it themselves.
    - `decay` and `brightness` are removed; they now live in a built-in instrument's knobs.
  - `MixSettings` gains `instruments: BTreeMap<InstrumentId, bpe_core::Instrument>`, `scene_instrument: Option<InstrumentId>` and `seconds_per_beat: f32` (the median gap of `beats`, or 0.5).
  - `mix_with` works in four steps:
    1. Group notes by channel.
    2. Run `global_steals`.
    3. Render each channel on all cores (`std::thread::scope`, as `NoteCache::prepare` does). A built-in instrument uses `NoteCache` one-shots placed with `mix_notes`; a patch uses `render_channel`, and a patch that fails to compile renders silence.
    4. Sum the channels in ascending channel order, then apply `normalize_loudness` and `limit_peaks` as now.
  - `Soundtrack.problems: Vec<String>` lists, in plain sentences:
    - "<instrument>: the patch doesn't compile (<first diagnostic>)".
    - "<instrument> made a bad sample and was stopped".
    - The missing sample files.
  - `pub fn instrument_for(settings: &MixSettings, id: Option<InstrumentId>) -> InstrumentChoice`. It resolves a missing id to Scene defaults' instrument, then to built-in Pluck (Review Focus 2).
  - `audition(chords, instrument: &InstrumentChoice, seconds_per_beat, ...)` now plays through the same engine, so the chord lane's Play uses patches too.
- [ ] **Step 1: Write the failing tests:**
  - `each_module_renders_as_its_own_channel_and_sums_in_order`: the mix equals the sum of the two channels rendered alone, within 1e-6 before the loudness step.
  - `a_patch_channel_sounds_and_a_broken_one_is_silent_with_a_problem` (Review Focus 1).
  - `a_missing_instrument_plays_scene_defaults` (Review Focus 2).
  - `built_in_channels_render_as_before`: a built-in-only log matches the old path's samples for the same notes.
  - `the_mix_is_the_same_twice`.
- [ ] **Step 2: Run** `cargo test -p bpe-mix`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-mix`. It should PASS. `cargo check --workspace --all-targets` will still fail in bpe-kit until Task 8.
- [ ] **Step 5: Commit** `feat(mix): render one channel per sound module through the voice engine`.

---

### Task 8: Sound modules link instruments

**Files:**
- Modify: `crates/bpe-kit/src/modules/sound/{mod,mix}.rs`, `crates/bpe-kit/src/modules/melody/{mod,mix}.rs`, `crates/bpe-kit/src/{lib.rs,scenes.rs,random.rs}`, `crates/bpe-studio/src/{starters.rs,presets.rs,sound_job.rs}`, and `scenes/next/*.ron` (re-blessed).

**Interfaces:**
- Consumes: Tasks 6 and 7.
- Produces:
  - `NoteSound.instrument` and `MelodySound.instrument` become `InstrumentRef` (label "Instrument", simple, random). `NoteSound.decay` and `brightness` are removed.
  - `new_scene` adds a library instrument "Pluck" (BuiltIn pluck, with decay 0.3 and brightness 0.5) and points the root sound module at it.
  - `mix_settings` fills `instruments`, `scene_instrument` and `seconds_per_beat`.
  - A Builder helper in `scenes.rs`, `fn instrument(&mut self, module: ModuleId, built_in: Instrument)`, keeps the scene recipes short.
  - Dice: `roll_instruments` picks among the built-ins and the scene's library instruments, honours locks, and skips `None` refs (the same rules as `roll_progressions`).
  - `sound_job` fingerprint: hash `library.instruments` and each module's `macros`.
- [ ] **Step 1: Write the failing tests:**
  - `a_new_scene_plays_its_library_pluck`.
  - `rolling_picks_an_instrument_and_respects_locks`.
  - The existing sound and melody tests, updated to the new field.
- [ ] **Step 2: Run** `cargo test -p bpe-kit`. It should FAIL.
- [ ] **Step 3: Implement**, then re-bless with `BPE_BLESS=1 cargo test -p bpe-kit --lib scenes::` (run it twice; the first run races the check).
- [ ] **Step 4: Run** `cargo test --workspace`. It should PASS, with event-log goldens unchanged and only soundtrack hashes re-blessed. List them in the commit message.
- [ ] **Step 5: Commit** `feat(kit): sound modules link library instruments`.

---

### Task 9: Starter patches

**Files:**
- Create: `crates/bpe-synth/src/starters.rs`

**Interfaces:**
- Produces:
  - `pub fn starters() -> Vec<(&'static str, Graph, VoiceSettings)>`.
  - The patches, in this order: Soft pluck, Glass FM bell, Supersaw, Warm pad, Sub bass, 808, Chip square, Noise snap, Wobble bass (an LFO synced at 1/8 into the filter's cutoff_mod), Marimba-ish (FM).
  - Each exposes 2 to 4 macros, such as Cutoff, Release, Detune and Drive.
- [ ] **Step 1: Write the failing tests:**
  - `every_starter_compiles_sounds_and_repeats`: each one compiles, note 60 for 1 s has RMS > 0.005 and peak ≤ 1.5, and two renders hash the same.
  - `every_starter_has_two_to_four_macros`.
- [ ] **Step 2: Run** `cargo test -p bpe-synth starters`. It should FAIL.
- [ ] **Step 3: Implement.** Voice it by ear only after the tests pass; the hashes are what CI checks.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. It should PASS.
- [ ] **Step 5: Commit** `feat(synth): ten starter patches`.

---

### Task 10: Instrument picker and instrument section in the inspector

**Files:**
- Create: `crates/bpe-studio/src/instruments.rs` (the section under the picker)
- Modify: `crates/bpe-studio/src/widgets.rs` (`ValueType::Instrument` row), `crates/bpe-studio/src/inspector.rs`, `crates/bpe-studio/src/main.rs`

**Interfaces:**
- Consumes: Tasks 6, 8 and 9.
- Produces:
  - `TargetChoices.instruments: Vec<(InstrumentId, String, usize /*users*/)>`.
  - `instrument_picker(ui, row, x, choices)`, a ComboBox salted with the row id. It lists, in order:
    - "Scene's (<name>)", on modules only.
    - A "Built-in" header with the 13 built-ins. Picking one finds or creates a library instrument of that built-in with default knobs, then links it.
    - The library instruments, each with "· used by N".
    - "New patch", "Starter patch ▸" (a submenu of the 10 starters) and "Make unique".
  - The sentinel values `NEW_PATCH = -2`, `MAKE_UNIQUE = -3`, `BUILT_IN_BASE = -100 - index` and `STARTER_BASE = -200 - index` are turned into real ids by `make_picked_instrument`, the same way part 1's `make_picked_progression` works.
  - `instruments::show(ui, scene, module: ModuleId, id: InstrumentId) -> Vec<InstrumentEdit>`, drawn under the card's settings grid:
    - The instrument's name, with an "Edit patch" button for patches.
    - A built-in instrument's knobs; for Pluck, Decay 0.05–5 s and Brightness 0–1.
    - Voices: Note length (beats or seconds), Voice limit 1–64, Mono, Glide, Retrigger.
    - Macros as sliders. A slider the module has overridden shows the override dot; right-click offers "Reset to instrument" and "Push to instrument", using the existing override marks.
  - The edits are applied after drawing (`set_instrument` and `set_macro`).
- [ ] **Step 1: Write the failing kittest tests:**
  - `a_sound_card_has_an_instrument_picker_with_built_ins_first`.
  - `picking_a_starter_adds_it_to_the_library_and_links_it`.
  - `make_unique_copies_the_instrument`.
  - `a_macro_override_is_marked_and_resets`.
  - `pluck_knobs_show_for_the_built_in_pluck`.
  - The existing `scene_defaults_fits_the_panel` test must still pass.
- [ ] **Step 2: Run** `cargo test -p bpe-studio instrument`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. It should PASS.
- [ ] **Step 5: Commit** `feat(studio): pick, share and shape instruments`.

---

### Task 11: Patches in the node panel

**Files:**
- Modify: `crates/bpe-studio/src/nodes/panel.rs`, `crates/bpe-studio/src/nodes/tests.rs`, `crates/bpe-studio/src/app.rs`

**Interfaces:**
- Consumes: Task 3 (`catalog()`, `compile`), Task 10 (the "Edit patch" request).
- Produces:
  - `NodePanel.open: Option<PanelTarget>` with `enum PanelTarget { Slot(ThingId, SlotId), Patch(InstrumentId) }`. `open_patch(id)` opens a patch.
  - For a patch, the panel edits `InstrumentSource::Patch.graph` with `bpe_synth::catalog()`. The header reads "Instrument › <name>" and "used by N". The status line shows compile errors, and the blocks show `node_errors`.
  - Every change writes back with `set_instrument`. An undo from outside reloads the panel, using the existing `stored` check.
  - `Inspector.patch_request: Option<InstrumentId>` is taken by the app, the same way `nodes_request` is.
- [ ] **Step 1: Write the failing tests:**
  - `edit_patch_opens_the_patch_in_the_panel`.
  - `wiring_blocks_in_the_panel_changes_the_instrument`.
  - `removing_voice_out_shows_the_error` (Review Focus 1).
- [ ] **Step 2: Run** `cargo test -p bpe-studio nodes`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. It should PASS.
- [ ] **Step 5: Commit** `feat(studio): edit synth patches in the node panel`.

---

### Task 12: Keyboard strip and audition through any instrument

**Files:**
- Create: `crates/bpe-studio/src/keyboard.rs`
- Modify: `crates/bpe-studio/src/instruments.rs`, `crates/bpe-studio/src/chords/mod.rs` (`ChordClip` carries an `InstrumentChoice` instead of a built-in), `crates/bpe-studio/src/inspector.rs`

**Interfaces:**
- Consumes: Task 7 (`audition` through the engine), part 1's `AudioOut::play_clip_with`.
- Produces:
  - `keyboard::show(ui, octave: &mut i8) -> Option<u8>`: a one-octave strip (C to B, black keys drawn over white), "Oct −" and "Oct +" buttons (octaves 1 to 7, default 4), and key labels like "C4". Clicking a key returns its MIDI note.
  - The inspector turns the note into a 1-beat `ChordClip` with a single-note chord, played through the module's instrument with its macros applied.
- [ ] **Step 1: Write the failing tests:**
  - `clicking_a_key_plays_that_note` (C4 → 60 and, after "Oct +", C5 → 72).
  - `octave_stops_at_the_ends`.
  - `chord_play_uses_a_patch_instrument`: the clip's instrument is the module's patch.
- [ ] **Step 2: Run** `cargo test -p bpe-studio keyboard`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. It should PASS.
- [ ] **Step 5: Commit** `feat(studio): a keyboard strip to play the instrument`.

---

### Task 13: Per-channel cache in the sound job

**Files:**
- Modify: `crates/bpe-studio/src/sound_job.rs`, `crates/bpe-mix/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct ChannelCache(BTreeMap<u32, (u64 /*fingerprint*/, Arc<Vec<f32>>)>)` in bpe-mix, and `mix_cached(settings, ..., cache: &mut ChannelCache) -> Soundtrack`.
  - A channel's fingerprint hashes its notes, its instrument, the module's macros and the voice settings. Only changed channels re-render.
  - The sound job keeps one cache across jobs.
- [ ] **Step 1: Write the failing tests:**
  - `changing_one_channel_re_renders_only_that_channel`: it counts renders through a test hook.
  - `cached_and_uncached_mixes_match`, bit-identical.
- [ ] **Step 2: Run** `cargo test -p bpe-mix cached`. It should FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace`. It should PASS.
- [ ] **Step 5: Commit** `perf(mix): re-render only the channels that changed`.

---

### Task 14: Speed check, scenes and the full check

**Files:**
- Modify: `crates/bpe-mix/src/lib.rs` (an `#[ignore]` test `a_busy_minute_renders_quickly`; no benches directory).
- Modify: `crates/bpe-kit/src/scenes.rs` (one starter scene uses a starter patch: Pitch climb gets "Glass FM bell" on its pegs, and Drum beat's tune gets "Warm pad").

- [ ] **Step 1:** Write `a_busy_minute_renders_quickly` (release, run by hand):
  - 60 s and 2 000 notes on a patch with 3 unison oscillators, a filter and two ADSRs.
  - It asserts under 4 s × 8 / `available_parallelism()` cores.
  - It also asserts that re-rendering after one macro change on one of two channels takes under 1 s.
- [ ] **Step 2: Run** `cargo test --workspace` and `cargo test --workspace --release --no-fail-fast`. They should PASS, except the known debug-only `pushing_an_earlier_step_panics_in_debug`. Then run `cargo test -p bpe-mix --release -- --ignored a_busy_minute` and report the time.
- [ ] **Step 3: Run** `cargo clippy --workspace --all-targets` (no new warnings) and `cargo fmt --check`.
- [ ] **Step 4: Check the real editor.**
  - Build the Windows editor and renderer (`--target x86_64-pc-windows-gnu`).
  - Under Xvfb, take screenshots of the instrument picker, a patch in the node panel and the keyboard strip.
  - Render Pitch climb to mp4.
  - Put everything in `/mnt/project-files/ballphysics/music-synth/part2/`.
- [ ] **Step 5: Commit** `chore: starter scenes use patches; part 2 complete`, push and open the PR.
