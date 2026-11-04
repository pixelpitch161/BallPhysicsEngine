# Sound system part 2: the shape's voice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A shape plays a note when a ball hits it: the Sound tab (layout B), two wavetable oscillators with Warp,
unison and sub, ADSR 1 on Osc A Gain, a one-slot keyboard, Playing and Output settings, poly/mono voices.

**Architecture:** Settings live in a `sound.voice` card in `SlotId::Sound` on the shape (schemas in `bpe-kit`),
DSP blocks in `bpe-dsp` (tables, unison oscillator, envelope), the voice engine in `bpe-sound`
(`ShapeVoice: VoiceSource`, voice allocation, a general modulation-route evaluator that part 4 extends), and the
Sound tab in `bpe-studio/src/sound_tab/`.

**Tech Stack:** Rust 2024, `wide 0.7` (SIMD), egui 0.37 + the part 1 knob widget.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` ("Where the sound settings live", "Part 2").
Mockup: canvas board **Wide** at https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA (read it with the Artifact tool,
`path: "project/Wide.dc.html"`). Decisions log: `docs/superpowers/plans/sound-system/decisions.md`.
**Starts after part 1 is merged**; base on `main`.

## Global Constraints

- Everything in part 1's Global Constraints still holds (48 kHz, 64-sample blocks, no allocation on the audio path,
  determinism, editor colours only from `style/`).
- FL Studio note names: MIDI 60 = **C5**, lowest octave C0 (MIDI 0 = C0).
- Starter patch exactly: Osc A **Triangle** on, Osc B off; ADSR 1 on → Osc A Gain; Delay 0, Attack **2 ms**,
  Hold 0, Decay **350 ms**, Sustain **0**, Release **200 ms**, Gate Fixed **100 ms**; note **C5**; Bus 1.
- Sound tab widens the inspector to **760 px**, draggable **640..960**; Settings restores the normal width.
- Poly default, Voices **16** (1..32), oldest stolen with a **5 ms** fade; Mono with Glide; no Legato.
- Retrigger guard default **30 ms**, per ball. Every sound knob takes keyframes.
- Aliasing: every wave and warp mode swept to C8 keeps folded components under **−90 dB**.

## Review Focus

1. **A shape hit by 200 balls at once** (a group release): voices cap at the limit, steals fade, no CPU spike
   past budget. Test in Task 6 (`a_burst_of_hits_stays_within_the_voice_limit`).
2. **Unison 16 + Detune at maximum on a C8 note**: no aliasing and no denormal slowdown. Task 3 alias test covers
   unison detune at max.
3. **Knob turned fast while a note rings** (Gain 0 → +12 dB in one frame): no click. Task 6 declick test.
4. **Enable sound ticked, unticked, ticked**: settings come back exactly; undo of the first tick removes the card.
   Task 8 kittest.
5. **Deleting the shape while its notes ring**: the engine drops the voice on the next patch with a short fade, no
   panic. Task 7 test (`removing_a_ringing_shape_fades_it_out`).

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-kit/src/sound/voice.rs` (new) | `VoiceSettings` (card `sound.voice`), `OscSettings`, enums, starter patch |
| `crates/bpe-kit/src/sound/adsr.rs` (new) | layer kind `mod.adsr`, `AdsrSettings` |
| `crates/bpe-kit/src/things/shape/mod.rs` | `SLOTS` gains `SlotId::Sound` (not default) |
| `crates/bpe-dsp/src/tables.rs`, `warp.rs` (new) | band-limited wavetable bank: wave × warp mode × 64 steps × octave mips |
| `crates/bpe-dsp/src/osc.rs` (new) | `UnisonOsc` (8-lane SIMD), sub osc, noise (white, pink) |
| `crates/bpe-dsp/src/envelope.rs` (new) | `Envelope` DAHDSR with curves and gate |
| `crates/bpe-sound/src/voices/mod.rs`, `shape.rs`, `alloc.rs`, `hit_filter.rs`, `notes.rs` (new) | the shape voice |
| `crates/bpe-sound/src/modmatrix.rs` (new) | route evaluation: add vs scale, sources by `LayerId` |
| `crates/bpe-sound/src/chain.rs` (new) | `Chain` per voice per oscillator: part 2 is empty pass-through; part 3 fills |
| `crates/bpe-sound/src/patch.rs` | builds `ShapeVoice`s from `sound.voice` cards |
| `crates/bpe-studio/src/sound_tab/` (new) | `mod.rs`, `osc.rs`, `wave_view.rs`, `mod_row.rs`, `adsr_view.rs`, `keys.rs`, `playing.rs`, `output.rs`, `write.rs` |
| `crates/bpe-studio/src/inspector.rs`, `app.rs`, `settings.rs` | Settings / Sound buttons, wide width |
| `crates/bpe-studio/src/widgets.rs` | `row_menu` made `pub(crate)` for knobs |

---

### Task 1: The voice card and ADSR layer schemas

**Files:** create `crates/bpe-kit/src/sound/voice.rs`, `adsr.rs`; modify `sound/mod.rs`, `things/shape/mod.rs`.

**Interfaces:**
- Consumes (part 1): `SlotId::Sound`, `LayerSection::Modulator`, `Value::Routes`, `ModRoute`, `Value::Slots`,
  `NoteSlot`, `bpe_kit::sound::register`.
- Produces:
  - `#[derive(Choice)] pub enum Wave { Sine, Triangle, Saw, Square, Noise }`, `NoiseColour { White, Pink }`,
    `WarpMode { Sync, BendUp, BendDown, AsymUp, AsymDown, Pwm, Mirror, Flip, Quantize, Squeeze }`
    (labels "Sync", "Bend +", "Bend −", "Asym +", "Asym −", "PWM", "Mirror", "Flip", "Quantize", "Squeeze"),
    `PhaseMode { Restart, Free }`, `SubWave { Off, Sine, Square }`, `SubOctave { One, Two }`,
    `PlayMode { Poly, Mono }`, `GateMode { Fixed, UntilNextHit }`, `GateUnit { Ms, Beats }`.
  - `pub struct OscSettings` (derive Settings; expanded twice with `prefix = "a_"` / `"b_"`): `on: bool`,
    `wave`, `noise`, `warp_mode`, `warp` (0..100 %, default 0, keyframe), `gain` (dB −inf..+12 shown as
    −60..+12 with −60 = −inf, default 0, keyframe), `level` (same, default 0), `octave` (−4..4), `semi` (−24..24),
    `cents` (−100..100, keyframe), `phase` (0..360°), `phase_mode` (default Restart), `unison` (1..16, default 1),
    `detune` (0..100 cents, default 20, keyframe), `spread` (0..100 %, default 50), `blend` (0..100 %, default 50),
    `random_phase` (0..100 %, default 0), `pan` (−100..100, keyframe), `sub` (default Off), `sub_octave`,
    `sub_level` (dB, default −6). Every Number field `keyframe`.
  - `pub struct VoiceSettings` (card kind `sound.voice`, slot `SlotId::Sound`, `fits` only `"shape"`):
    `enabled: bool` (default true), `a: OscSettings` (prefix `a_`, `on` default true, wave Triangle),
    `b: OscSettings` (prefix `b_`, `on` default false, wave Saw), `play_mode`, `play_voices` (1..32, 16),
    `play_glide` (0..2000 ms, 0, `when = "play_mode=mono"`), `play_min_strength` (0..100 %, 0),
    `play_retrigger` (0..500 ms, 30), `play_heard_from: Target` (default all balls), `notes_slots: Slots`
    (default `[NoteSlot{notes:[60]}]`), `notes_low_octave` (0..8, default 4), `notes_octaves` (1..8, default 3),
    `out_volume` (dB −60..+6, 0, keyframe), `out_pan` (−100..100, keyframe), `out_follow_hit` (0..100 %, 0),
    `out_bus` (1..3, default 1), `out_send1..3` (dB −60..+6 = off at −60; part 6 shows them), `routes: Routes`.
    Plain keys use `_`, never `/`.
  - `pub struct AdsrSettings` (layer kind `mod.adsr`, section Modulator, name "ADSR"): `delay`, `attack`, `hold`,
    `decay`, `release` (ms, ranges 0..10000, log feel), `sustain` (0..100 %), `velocity` (0..100 %, 50),
    `attack_curve`, `decay_curve`, `release_curve` (−100..100, 0 = linear, default −40/40/40 for exponential
    feel: attack convex, decay and release concave), `gate_mode`, `gate_unit`, `gate_ms` (1..10000, 100),
    `gate_beats` (1/16..16, 0.25).
  - `pub fn starter_voice(reg: &Registry) -> Slot` — the starter patch above, with two `mod.adsr` layers
    "ADSR 1" (enabled) and "ADSR 2" (disabled), and `routes = [ModRoute { source: adsr1, target: "a_gain",
    amount: 1.0, bipolar: false, invert: false }]`.
- [ ] **Step 1: Failing tests** (`sound/voice.rs`): `starter_patch_matches_the_spec` (all values above),
  `voice_card_fits_only_shapes`, `osc_keys_are_prefixed` (`a_gain`, `b_gain` defs exist, no key contains `/`),
  `shapes_allow_a_sound_card_but_do_not_start_with_one`.
- [ ] **Step 2: Run** `cargo test -p bpe-kit sound::` — FAIL. **Step 3: Implement.** **Step 4:** PASS.
- [ ] **Step 5: Commit** `git commit -m "Kit: the shape's sound card and ADSR layer"`

### Task 2: Band-limited wavetables and Warp

**Files:** create `crates/bpe-dsp/src/tables.rs`, `warp.rs`. Port ideas from
`git show origin/version-1:crates/bpe-synth/src/blocks/tables.rs`.

**Interfaces:**
- Produces:
  - `pub const FRAME: usize = 2048; pub const WARP_STEPS: usize = 64; pub const MIPS: usize = 11;`
  - `pub fn warp_phase(mode: WarpModeId, amount: f32, phase: f32) -> f32` and `pub fn warp_shape(mode, amount,
    sample: f32) -> f32` (`warp.rs`): Sync (phase × (1 + 15·a), wrapped, rendered band-limited by baking),
    Bend± (phase power curve), Asym± (piecewise phase squeeze toward start/end), PWM (pulse width 50 %→2 % for
    Square, phase squeeze for others), Mirror (phase folded at 0.5 with amount = fold point), Flip (invert the
    second half by amount), Quantize (sample steps 256 → 2), Squeeze (phase compressed toward the middle).
    `WarpModeId` is a plain `u8` enum in bpe-dsp mirroring `WarpMode` (bpe-dsp knows nothing of bpe-kit).
  - `pub struct WaveBank` with `WaveBank::new()` (cheap) and `prepare(&mut self, wave, mode) -> TableSet` —
    builds, **off the audio thread** (on the sound thread when a patch is built), the 64 warp steps × 11 mips for
    one (wave, mode): each step is baked at 2048 samples by FFT, harmonics above the mip's limit zeroed, mip m
    holding `max(2048 >> m, 256)` samples. One set is ≈ 2 MB; the bank keeps an LRU of at most 12 sets and hands
    out `Arc<TableSet>`, so a patch never waits on the audio thread. Baking every set up front would be ≈ 500 MB,
    which is why it is lazy (note this in decisions.md).
  - `pub struct Table<'a>` view with `fn sample(&self, phase: f64, step: f32, freq_hz: f32) -> f32` choosing the
    mip from frequency (no harmonics above 20 kHz… more precisely, mip whose top harmonic × freq < 24 kHz), linear
    between steps and between the two nearest mips, cubic within a frame.
  - `pub fn draw(wave, mode, amount, points: usize) -> Vec<f32>` for the editor's waveform view (non-band-limited
    shape, cheap).
- [ ] **Step 1: Failing tests:** `every_wave_and_mode_is_alias_free_to_c8` — for each wave (not Noise) × mode ×
  warp {0, 0.5, 1}, render a sweep from 20 Hz to **4186 Hz** (C8 in scientific naming, the stricter reading of the spec's "C8";
  FL's C8 is only 2093 Hz) over 2 s at 48 kHz;
  FFT frames of 8192 with Blackman-Harris; energy at non-harmonic bins below −90 dB relative to the fundamental.
  `table_sample_is_continuous_across_mips` (frequency glide 1 kHz→3 kHz: no sample-to-sample jump > the wave's
  own max slope × 1.5). `draw_matches_the_table_shape` (correlation > 0.98 at 220 Hz).
- [ ] **Step 2–4:** run `cargo test -p bpe-dsp --release tables` — FAIL, implement, PASS.
- [ ] **Step 5: Commit** `git commit -m "DSP: band-limited wavetables with ten Warp modes"`

### Task 3: Unison oscillator, sub and noise

**Files:** create `crates/bpe-dsp/src/osc.rs`, `noise.rs`.

**Interfaces:**
- Produces:
  - `pub struct UnisonOsc` — up to 16 voices in two `wide::f32x8` lanes; `prepare(seed: u64)`;
    `set(freq_hz, unison: u8, detune_cents, spread, blend, random_phase, phase_deg, restart: bool)`;
    `note_on(&mut self, rng_seed: u64)` (phases: restart → `phase_deg` for all voices plus `random_phase` × per-voice
    random from the seed; free → keep); `render(&mut self, table: &Table, warp_step: &[f32; BLOCK], gain: &[f32],
    l: &mut [f32; BLOCK], r: &mut [f32; BLOCK])`. Detune spreads voices symmetrically (±detune/2, Serum-like
    curve: inner voices closer), spread pans voices alternately L/R up to `spread`, blend crossfades centre voice
    against sides with equal power.
  - `pub struct SubOsc { set(freq_hz, wave: SubWaveId, octave: u8, level) , render(...) }` (PolyBLEP square,
    pure sine).
  - `pub struct Noise { white(&mut self) -> f32, pink(&mut self) -> f32 }` (xorshift64*; Paul Kellet pink),
    seeded per voice.
- [ ] **Step 1: Failing tests:** `unison_16_at_max_detune_is_alias_free_at_c8` (−90 dB rule, saw),
  `unison_1_equals_a_single_table_read`, `spread_zero_is_mono`, `pink_noise_slopes_minus_3db_per_octave`
  (±1 dB between 100 Hz and 10 kHz bands), `noise_is_deterministic_per_seed`, `no_denormals_after_silence`
  (render 10 s of 0 gain; time per block stays flat ±50 % between first and last second).
- [ ] **Step 2–4:** FAIL, implement, PASS (release).
- [ ] **Step 5: Commit** `git commit -m "DSP: SIMD unison oscillator, sub and noise"`

### Task 4: Envelope with Delay, Hold, curves and gate

**Files:** create `crates/bpe-dsp/src/envelope.rs`.

**Interfaces:**
- Produces: `pub struct Envelope` with `set(EnvParams)`, `trigger(velocity_scale: f32)`, `release()`,
  `steal(fade_ms: f32)`, `next_block(&mut self, out: &mut [f32; BLOCK])`, `is_done() -> bool`,
  `stage() -> Stage`. `EnvParams { delay, attack, hold, decay, sustain, release /* seconds */, curves: [f32; 3] }`.
  Curve `c` in −1..1 maps a linear 0..1 segment `t` to `t^(2^(−3c))` style power (0 = linear). Retrigger while
  sounding starts the attack **from the current level** (no click). Gate handling lives in the voice, not here.
- [ ] **Step 1: Failing tests:** `stages_take_their_set_times` (each stage boundary within 1 sample),
  `retrigger_starts_from_the_current_level`, `steal_fades_in_5ms`, `curves_bend_but_keep_end_points`,
  `sustain_zero_ends_without_release_tail` (`is_done()` after decay when sustain 0 and gate already closed).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "DSP: DAHDSR envelope with curves"`

### Task 5: Modulation routes evaluator

**Files:** create `crates/bpe-sound/src/modmatrix.rs`.

**Interfaces:**
- Consumes: `ModRoute` (part 1).
- Produces:
  - `pub enum Combine { Add, Scale }`; `pub fn combine_of(key: &str) -> Combine` (`Scale` for keys ending
    `_gain` or `_level` of an oscillator and for `out_volume`; `Add` otherwise).
  - `pub struct ModMatrix` built per patch: `ModMatrix::build(routes: &[ModRoute], targets: &TargetTable) ->
    ModMatrix` where `TargetTable` maps a key to `(index: u16, range: (f64, f64), log: bool)`;
    `fn apply(&self, sources: &[f32] /* by source index, 0..1 */, base: &[f32], out: &mut [f32])` — for Add:
    `out = clamp(base + Σ amount·(m or 2m−1 when bipolar)·(hi−lo))` (in the knob's own mapping: log knobs add in
    log space); for Scale (dB knobs): `out_db = base_db + Σ_route 20·log10(1 − |amount|·(1 − m'))` where `m'` is
    `m` or `1 − m` when `invert`, so amount 1 and m = 0 is −inf (silence) and m = 1 leaves the knob.
  - `pub struct SourceId(pub u16)`; `ModMatrix::sources() -> &[LayerId]` lists the modulator layers used.
- [ ] **Step 1: Failing tests:** `adsr_on_gain_scales_to_silence` (m=0 → −inf, m=1 → base), `two_scaling_routes_multiply`,
  `add_routes_sum_and_clamp`, `bipolar_centres_on_the_knob`, `invert_flips`, `combine_of_names_the_vca_knobs`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: modulation routes, adding or scaling"`

### Task 6: The shape voice

**Files:** create `crates/bpe-sound/src/voices/{mod.rs, shape.rs, alloc.rs, hit_filter.rs, notes.rs}`,
`crates/bpe-sound/src/chain.rs`; modify `patch.rs`.

**Interfaces:**
- Consumes: Tasks 1–5; part 1's `VoiceSource`, `BlockCx`, `BlockHit`, `TempoMap`.
- Produces:
  - `pub struct ShapeVoice` implementing `VoiceSource` for one shape's `sound.voice` card.
  - `hit_filter.rs`: `pub struct HitFilter { min_strength, retrigger_s, heard_from: Target }` with
    `fn accept(&mut self, hit: &BlockHit, sample: u64, world: &World) -> Option<f32 /* strength 0..1 */>`;
    strength = `(speed / STRONG_HIT).min(1.0)` with `pub const STRONG_HIT: f32 = 1500.0` px/s; the guard keeps the
    last accepted sample per `(ball thing, item)` in a fixed `Vec` sized at patch build (no allocation per block).
    Only hits where `b` is this shape (wall hits) count.
  - `notes.rs`: `pub trait NotePicker { fn pick(&mut self, cx: &PickCx) -> NotePick }` with
    `NotePick { notes: SmallArray<u8, 16>, strum: f32 }`; part 2 ships `RandomPick` (seeded by shape id + hit
    count) over slot 0. Part 5 adds the other orders behind this trait.
  - `alloc.rs`: `pub struct Voices<V>` fixed capacity 32: `start(note, strength, ball: Party, sample_offset)`,
    steals the oldest with `Envelope::steal(5.0)` when at `play_voices`; Mono: one voice, retriggered, pitch glides
    over `play_glide` ms (exponential in pitch).
  - Per voice: `OscVoice` ×2 (UnisonOsc + SubOsc or Noise), per-oscillator `chain::Chain` (part 2:
    `Chain::empty()`; `process(&mut self, l, r)` pass-through), `Envelope` per enabled ADSR layer, gate timer
    (Fixed ms/beats via `TempoMap::seconds_of_beats`; UntilNextHit releases when the next accepted hit on this
    shape arrives).
  - Signal order per oscillator: osc → **Gain** (smoothed, modulated) → chain → **Level** → pan; sum A+B →
    Output Volume → Output Pan (+ Follow hit: pan += follow × (hit.x / video width × 2 − 1)) → bus `out_bus`.
  - Hits start notes at `BlockHit.offset` (render the block in two parts around each offset).
  - `ShapeVoice::adopt(&mut self, old: &mut dyn VoiceSource)` (part 1's hook) takes the old voice's ringing notes,
    envelopes and phases, and its smoothers glide to the new parameters.
  - `patch.rs`: `SoundPatch::build` creates a `ShapeVoice` for every shape whose `SlotId::Sound` card is
    `enabled`; voices whose shape has gone fade out over 10 ms then drop.
- [ ] **Step 1: Failing tests** (`crates/bpe-sound/tests/shape_voice.rs`, test scenes from `bpe_kit::scenes` with a
  shape given `starter_voice`):
  - `starter_patch_renders_its_golden` (hash of 2 s render; bless with `BLESS_GOLDEN=1` into
    `crates/bpe-sound/tests/golden-sound.txt` and `-debug` variant, like the physics goldens).
  - `a_hit_plays_c5` (FFT peak 261.6 Hz ±1 % in the 50 ms after the hit — C5 in FL naming is MIDI 60 = 261.63 Hz).
  - `a_burst_of_hits_stays_within_the_voice_limit` (200 hits in one block: ≤ 16 sounding, render time of that
    block < 2 ms release).
  - `mono_glide_reaches_pitch_in_its_time`, `retrigger_guard_drops_a_second_hit_within_30ms`,
    `min_strength_drops_soft_hits`, `heard_from_ignores_other_balls`, `until_next_hit_releases_on_the_next_hit`.
  - `fast_gain_change_does_not_click` (Gain −60 → +12 in one patch swap mid-note: max sample-to-sample delta
    < 0.05 beyond the waveform's own max delta).
  - `osc_b_off_costs_nothing` (with Osc B off, a block renders in under 0.7× the time of the same patch with
    Osc B on).
  - `removing_a_ringing_shape_fades_it_out`.
  - Speed: `heavy_patch_renders_8x_real_time` (16 voices, 16 unison both oscs; release only, `#[ignore]` in debug).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: the shape voice plays notes on hits"`

### Task 7: Audition (click a key, hear it)

**Files:** modify `crates/bpe-sound/src/engine.rs`, `crates/bpe-studio/src/sound_job.rs`.

**Interfaces:**
- Produces: `Engine::audition(&mut self, shape: ThingId, note: u8)` — starts a note on that shape's voice outside
  the hit path (strength 0.8, its own gate). `SoundJob` keeps rendering while paused **only** auditions and the
  master tail (physics not stepped, clock not advanced); `SoundJob::audition(shape, note)` sends the command.
- [ ] **Step 1: Failing test:** `audition_sounds_while_paused` (fake sink receives non-silent frames after
  `audition` with the job paused; the clock does not move).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: audition notes while stopped"`

### Task 8: Settings / Sound tab and the wide inspector

**Files:** create `crates/bpe-studio/src/sound_tab/mod.rs`, `write.rs`; modify `inspector.rs`, `app.rs`
(`inspector_panel` width), `settings.rs`.

**Interfaces:**
- Produces:
  - `pub enum InspectorTab { Settings, Sound }` in `inspector.rs`, `Inspector.tab: BTreeMap<ThingId, InspectorTab>`
    (only shapes show the buttons; other kinds always Settings).
  - Top row of the inspector for a shape: two chips "Settings" | "Sound" (style chips, like the Look editor's
    Ball | Group switch).
  - `settings::SOUND_WIDTH_RANGE = 640.0..=960.0`, default 760, saved as `sound_inspector_width` like
    `inspector_width`. `Studio::inspector_panel` uses it while the primary thing's tab is Sound (panel
    `min_size`/`max_size` from that range); switching back restores the saved normal width.
  - `sound_tab::show(ui, &mut SoundTab, scene, kit, resolved, id, time, job: Option<&SoundJob>) -> Option<SoundTabOut>`
    where `SoundTab` holds UI state (picked modulator, open folds) and `SoundTabOut { audition: Option<u8> }`.
  - `write.rs`: `pub fn write(scene: &mut Scene, reg: &Registry, a: &Address, v: Value, time: f64) -> WriteOutcome`
    — if the address has a track, `scene.set_key(reg, a, time, v)` and report `Keyed` (the studio pauses as for
    the inspector's `keyed_edit`), else `scene.set_at(reg, a, v)`.
  - When no `sound.voice` card: only an "Enable sound" switch row. Ticking: `add_slot(…SlotId::Sound,
    "sound.voice")` then replace with `starter_voice`. Unticking: `enabled = false`. Right-click the row: "Copy
    sound", "Paste sound" (the whole `Slot` in `Inspector.clipboard`, a new `Clipboard::Sound(Slot)` variant;
    paste keeps the target's own layer ids fresh: re-id layers and rewrite routes).
- [ ] **Step 1: Failing kittest tests** (`sound_tab/mod.rs`): `a_shape_shows_settings_and_sound_buttons`,
  `a_ball_shows_no_sound_button`, `sound_widens_the_inspector_to_760_and_back`,
  `enable_sound_adds_the_starter_patch_and_undo_removes_it`, `untick_and_tick_again_keeps_the_settings`,
  `copy_and_paste_sound_between_shapes` (routes point at the pasted layers).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: the Sound tab and its wide inspector"`

### Task 9: Oscillator panels and the waveform view

**Files:** create `sound_tab/osc.rs`, `sound_tab/wave_view.rs`.

**Interfaces:**
- Consumes: `Knob` (part 1), `bpe_dsp::tables::draw`, `write`.
- Produces: `osc::show(ui, cx: &mut TabCx, which: Osc /* A | B */)`. Layout (board Wide): header with on switch
  and "Osc A"; wave picker chips (Sine, Triangle, Saw, Square, Noise; Noise shows White | Pink); waveform view
  (`wave_view::show(ui, wave, mode, warp_with_mod)`, 100 % width × 64 px, `GROUND` background, stroke `TEXT`);
  Warp mode `menu_button` + Warp knob; knob rows: Gain, Level, Octave, Semi, Cents, Phase (+ Restart/Free
  chips); Unison, Detune, Spread, Blend, Random phase, Pan; Sub row (Off/Sine/Square chips, −1/−2, Level).
  Below: an empty "Layers" box with a muted line "Layers come in a later update" until part 3 replaces it with
  `chain_box::show` (leave a function `layers_box(ui, cx, which)` as the hook).
  `TabCx<'a> { scene, kit, resolved, id, time, colour: Color32, edits: &mut Vec<(Address, Value)> }`.
  Knob right-click opens the existing value menu (make `widgets::row_menu` `pub(crate)` taking any `Response`).
- [ ] **Step 1: Failing kittest tests:** `turning_warp_redraws_the_wave` (wave view points change),
  `noise_shows_white_and_pink`, `every_osc_knob_is_named`, `right_click_a_knob_creates_a_keyframe`
  (track appears at the playhead), `osc_b_starts_off_and_its_knobs_are_dimmed`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: oscillator panels with a live waveform"`

### Task 10: Modulator row and ADSR 1 editor

**Files:** create `sound_tab/mod_row.rs`, `sound_tab/adsr_view.rs`.

**Interfaces:**
- Produces: `mod_row::show(ui, cx, tab: &mut SoundTab)` — tiles ADSR 1, ADSR 2, Distance 1, Distance 2, LFO 1,
  LFO 2 (the last four drawn as "Off" placeholders, not clickable, until part 4 replaces the row); a tile shows
  name and target count; click picks it. `adsr_view::show(ui, cx, layer: LayerId)`: envelope view (240 × 90 px)
  with corner handles (drag time/level), segment drag bends the curve (vertical drag → curve −100..100), dashed
  gate line; knobs Delay, Attack, Hold, Decay, Sustain, Release, Velocity; Gate chips Fixed | Until next hit, and
  an ms | Beats switch; a "Modulates" list showing "Osc A Gain  −inf dB → 0 dB" (read-only in part 2).
- [ ] **Step 1: Failing kittest tests:** `dragging_the_attack_corner_sets_attack`, `dragging_a_segment_bends_its_curve`,
  `gate_beats_switch_shows_beats`, `the_modulates_list_shows_osc_a_gain`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: ADSR editor with draggable envelope"`

### Task 11: Keyboard, Playing and Output

**Files:** create `sound_tab/keys.rs`, `playing.rs`, `output.rs`.

**Interfaces:**
- Produces: `keys::show(ui, cx, out: &mut SoundTabOut)` — a keyboard from `notes_low_octave` for `notes_octaves`
  (default C4..B6), − and + buttons; octave labels "C4", "C5"… under each C (FL naming); lit keys use the shape's
  track colour; click toggles the note in slot 0 (writes `notes_slots`) and sets `out.audition`. Part 5 replaces
  this module with the slot grid but keeps the keyboard drawing function `keyboard(ui, range, lit: &[u8]) ->
  Option<u8>` reusable.
  `playing::show`: Poly | Mono chips, Voices, Glide (Mono), Min hit strength, Retrigger guard, Heard from (the
  existing target picker widget). `output::show`: Volume, Pan, Follow hit knobs, Bus picker (Bus 1 only, others
  disabled with hover "Buses 2 and 3 come with the Mixer"), a level meter for this shape (engine publishes per-shape
  peaks in `Meters.shapes: [AtomicU32; 64]` by voice index; add to part 1's `Meters` here).
- [ ] **Step 1: Failing kittest tests:** `clicking_a_key_toggles_it_and_auditions`, `octave_buttons_grow_and_shrink`,
  `mono_shows_glide`, `key_labels_use_fl_names` ("C5" under middle C).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: keyboard, Playing and Output sections"`

### Task 12: Finish the part

- [ ] **Step 1:** Check the timeline lists sound keys in the shape's group (add a kittest
  `a_sound_key_shows_in_the_shapes_timeline_group`; fix label lookup for layered keys under `SlotId::Sound`).
- [ ] **Step 2:** `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` (debug and
  release), bless sound goldens in both, physics goldens unchanged.
- [ ] **Step 3:** Update `CLAUDE.md` (sound card, Sound tab, wavetable bank). Windows cross-build of both exes.
- [ ] **Step 4:** One Opus review of the branch; fix findings.
- [ ] **Step 5:** PR with the attribution block; assign and request review from `pixelpitch161`; exes to
  `/mnt/project-files/ballphysics/sound-system/part2/`.
