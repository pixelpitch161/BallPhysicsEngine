# Sound system part 3: oscillator layers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each oscillator gets a top-to-bottom chain of Modulation, Filter and Distortion layers, shown as folded
strips with live mini views, run per voice between the oscillator's Gain and Level.

**Architecture:** Layer schemas (`chain.modulation`, `chain.filter`, `chain.distortion`) in `bpe-kit`, stored as
`Slot.layers` of section `LayerSection::Chain` with `lane` 0 (Osc A) or 1 (Osc B). DSP in `bpe-dsp` (ZDF SVF,
ladder, comb, formant, shelves/peak, waveshapers with the part 1 oversampler, phase-mod delay). `bpe-sound`'s
`chain::Chain` (the part 2 hook) runs the layers per voice and handles cross-oscillator sources and loops. The
editor's `sound_tab/chain_box.rs` replaces part 2's `layers_box` placeholder.

**Tech Stack:** Rust 2024, `wide 0.7`, egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` ("Part 3"). Mockup: canvas board
**ChainFold** (https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA, `project/ChainFold.dc.html`). Decisions log:
`docs/superpowers/plans/sound-system/decisions.md`. **Starts after part 2 is merged.** Runs in parallel with parts
5 and 6; merge order **3, 5, 6** (this part merges first; keep shared-file edits additive).

## Global Constraints

- Part 1 and 2 constraints hold (48 kHz, 64-sample blocks, no allocation on the audio path, determinism).
- Order per oscillator: osc → **Gain** → strips top to bottom → **Level** → Pan. Layers run **per voice**.
- Solo on a strip plays the chain **cut at that layer** (its output, nothing below it, still through Level).
- FM and PD are **phase modulation of the sound reaching the layer**, a moving fractional delay of at most **2 ms**
  (cubic interpolation); PD uses the source one-sided (CZ-style). A source that loops back runs **one sample late**
  and shows a loop mark.
- Distortion Quality 1×, 2×, **4× (default)**, 8×; Bitcrush and Downsample always 1×.
- Aliasing: a C8 saw (4186 Hz) through every distortion at 4× keeps folded components under **−70 dB**.
- **Before locking FM and PD**, render A/B clips (one clip per mode, the same patch with the source first in the
  chain vs after a filter) to `/mnt/project-files/ballphysics/sound-system/part3/fm-pd-ab/` and link them in the
  PR; the build does not wait for the owner (decisions.md).

## Review Focus

1. **Resonance at maximum with Cutoff swept by an LFO at audio rate** (part 4 later; simulate with a per-sample
   ramp now): the filter must stay bounded. Task 2 test.
2. **A layer whose source is a layer that was then deleted**: the route falls back to "Osc A raw" of its own
   oscillator, never panics. Task 5 test.
3. **Drag a strip from Osc A to Osc B** (paste across): its source still points somewhere valid; loop detection
   reruns. Task 7 kittest.
4. **Twenty layers on one oscillator with 16 voices**: render stays within budget (8× real time in release).
   Task 5 bench.
5. **Distortion with Drive at max on silence**: no DC thump or denormal slow-down. Task 3 test.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-kit/src/sound/chain.rs` (new) | layer kinds and settings for the three chain layers |
| `crates/bpe-dsp/src/filter/` (new) `svf.rs`, `ladder.rs`, `comb.rs`, `formant.rs`, `response.rs` | filters and their drawn responses |
| `crates/bpe-dsp/src/shaper.rs` (new) | the ten distortion types, bias, tone, DC blocker |
| `crates/bpe-dsp/src/pm_delay.rs` (new) | the phase-modulation delay for FM and PD |
| `crates/bpe-sound/src/chain.rs` | per-voice chain: layers, sources, solo, loops |
| `crates/bpe-sound/src/chain_graph.rs` (new) | which layer reads which; loop detection; render order |
| `crates/bpe-studio/src/sound_tab/chain_box.rs`, `strip.rs`, `strip_views.rs` (new) | folded strips UI |

---

### Task 1: Chain layer schemas

**Files:** create `crates/bpe-kit/src/sound/chain.rs`; modify `sound/mod.rs` (register).

**Interfaces:**
- Consumes: `LayerSection::Chain`, `Layer.lane`, `add_layer_in` (part 1).
- Produces:
  - `ModulationSettings` (`chain.modulation`, name "Modulation"): `kind: ModKind { Fm, PhaseDistortion, Am, Ring }`
    (labels "FM", "Phase distortion", "AM", "Ring"), `source: Text` — `"a"`, `"b"` (raw oscillator) or
    `"<layer id>"` (after that layer), default the *other* oscillator raw; `amount` (0..100 %, keyframe),
    `output` (dB −24..+24), `mix` (0..100 %, default 100), `solo: bool`.
  - `FilterSettings` (`chain.filter`, "Filter"): `kind: FilterKind { LowPass, HighPass, BandPass, Notch, Peak,
    LowShelf, HighShelf, Ladder, Comb, Formant }`, `slope: Slope { Db12, Db24 }` (`when` not Ladder/Comb/Formant),
    `cutoff` (20..20000 Hz log, default 2000, keyframe), `reso` (0..100 %), `drive` (0..100 %), `key_track`
    (0..100 %), `gain` (dB −24..+24, `when` Peak/shelves), `vowel` (0..4 continuous A,E,I,O,U, `when` Formant),
    `shift` (−12..12 semitones, `when` Formant), `mix`, `solo`.
  - `DistortionSettings` (`chain.distortion`, "Distortion"): `kind: DistKind { SoftClip, HardClip, Tube, Tape,
    Diode, Overdrive, Wavefold, Bitcrush, Downsample, Rectify }`, `drive` (dB 0..+48), `bits` (1..16, `when`
    Bitcrush), `rate` (100..48000 Hz log, `when` Downsample), `bias` (−100..100 %), `tone` (−100..100: tilt
    after), `output` (dB), `quality: Quality { X1, X2, X4, X8 }` default X4 (`when` not Bitcrush/Downsample),
    `mix`, `solo`.
  - `pub fn layer_summary(layer: &Layer, params: &Params) -> String` — the strip's one-liner ("Low pass 24 · 1.8
    kHz", "Tube · +12 dB · 4x", "FM from Osc B · after Filter").
- [ ] **Step 1: Failing tests:** `chain_layers_register_in_the_chain_section`, `default_modulation_source_is_the_other_osc`,
  `summaries_read_like_the_mockup`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Kit: Modulation, Filter and Distortion chain layers"`

### Task 2: Filters

**Files:** create `crates/bpe-dsp/src/filter/{mod.rs, svf.rs, ladder.rs, comb.rs, formant.rs, response.rs}`. Port
`origin/version-1:crates/bpe-synth/src/blocks/filter.rs` (Cytomic SVF) and `ladder.rs`.

**Interfaces:**
- Produces: `pub trait MonoFilter { fn set(&mut self, p: &FilterParams); fn process(&mut self, x: f32) -> f32; fn reset(&mut self); }`;
  `FilterParams { kind: FilterKindId, slope24: bool, cutoff_hz: f32, reso: f32, drive: f32, gain_db: f32, vowel: f32, shift: f32 }`;
  `pub struct StereoFilter` (two channels, per-sample parameter smoothing of `g` coefficient);
  `pub fn response_db(p: &FilterParams, freq_hz: f32) -> f32` (analytic, for the editor's curve and tests).
  SVF types use TPT with `g = tan(π fc / fs)` (f64 coefficient maths); 24 dB = two SVFs with Butterworth Q split;
  Ladder = Huovilainen/Zavalishin ZDF with tanh drive; Comb = feedback comb at the note's period (cutoff = 1/delay
  when Key track 0, note pitch × 2^(key_track·…) otherwise); Formant = three parallel band-passes per vowel table
  (F1–F3 for A E I O U, morphed linearly), shifted by `shift`.
- [ ] **Step 1: Failing tests:** `curves_match_textbook_responses` (sine sweep measured gain vs `response_db`
  within 0.5 dB from 20 Hz to 20 kHz for every SVF type × slope at cutoff 1 kHz, reso 0 and 70 %);
  `ladder_self_oscillates_at_full_reso` (bounded, near cutoff); `resonance_sweep_under_fast_modulation_is_bounded`
  (cutoff ramped 20 Hz↔20 kHz every 64 samples at reso 100 %: |y| < 8 for 10 s); `comb_with_key_track_plays_the_note`;
  `formant_vowels_peak_at_their_f1` (A: F1 ≈ 800 Hz ±10 %).
- [ ] **Step 2–4:** FAIL, implement, PASS (release). **Step 5: Commit** `git commit -m "DSP: filters (SVF, ladder, comb, formant)"`

### Task 3: Distortion

**Files:** create `crates/bpe-dsp/src/shaper.rs`.

**Interfaces:**
- Consumes: part 1 `Oversampler`.
- Produces: `pub struct Shaper` with `set(&ShaperParams)`, `process(&mut self, buf: &mut [f32; BLOCK])`,
  `latency() -> usize`, and `pub fn transfer(kind, drive_db, bias, x: f32) -> f32` (the static curve, also drawn by
  the editor). Types: Soft (tanh), Hard (clamp), Tube (asymmetric, bias-shifted `x/(1+|x|)`-style with even
  harmonics), Tape (soft knee + slight hysteresis-like lowpass), Diode (exponential one-sided), Overdrive
  (two-stage cubic), Wavefold (sine fold), Bitcrush (quantise to `bits`), Downsample (sample-and-hold at `rate`),
  Rectify (full-wave, bias = half-wave blend). Bias adds DC before the curve; a 10 Hz DC blocker after. Tone is a
  first-order tilt (±6 dB at the extremes, pivot 1 kHz). Output gain after. Quality selects the oversampler.
- [ ] **Step 1: Failing tests:** `c8_saw_through_every_type_is_alias_free_at_4x` (−70 dB; skip Bitcrush/Downsample);
  `bias_adds_even_harmonics_to_tube` (2nd harmonic > −40 dB with bias 50 %, < −80 dB with bias 0);
  `max_drive_on_silence_stays_silent` (|y| < 1e-6, no DC); `transfer_matches_process_at_low_freq` (20 Hz sine).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "DSP: ten distortion types with oversampling"`

### Task 4: Phase-modulation delay (FM and PD)

**Files:** create `crates/bpe-dsp/src/pm_delay.rs`.

**Interfaces:**
- Produces: `pub struct PmDelay` (ring of 2 ms + 4 samples, cubic Hermite read) with
  `process(&mut self, x: f32, modulator: f32, amount: f32, one_sided: bool) -> f32`: delay = `centre + depth·m`
  where for FM `m` in −1..1 and `centre = depth = amount × 1 ms`; for PD (one_sided) `m' = (m+1)/2` and delay =
  `amount × 2 ms × m'`. AM: `x·(1 + amount·m)/(1+amount)`; Ring: `x·(1−amount + amount·m)`.
  `pub fn fm_index_of(amount: f32, carrier_hz: f32) -> f32` (for the editor's readout: index ≈ 2π f_c × depth).
- [ ] **Step 1: Failing tests:** `fm_first_in_chain_matches_analytic_fm` (sine carrier 440 Hz, sine modulator
  440 Hz, small index: sideband levels match Bessel J0/J1 within 1 dB); `pd_is_one_sided`; `am_and_ring_levels`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "DSP: phase-modulation delay for FM and PD"`

### Task 5: The per-voice chain

**Files:** modify `crates/bpe-sound/src/chain.rs`; create `chain_graph.rs`; modify `voices/shape.rs`.

**Interfaces:**
- Consumes: Tasks 1–4; part 2's `Chain` hook and `OscVoice`.
- Produces:
  - `chain_graph.rs`: `pub struct ChainGraph { order: Vec<Node>, loops: Vec<LayerId> }` built per patch from both
    lanes: `Node::{OscRaw(lane), Layer(LayerId)}`; a modulation layer's source taps a node's output; a tap that
    points at a node rendered **later in the same sample** is a loop and reads that node's previous-sample value
    (`loops` lists such layers for the editor's loop mark). Missing sources fall back to the layer's own lane raw.
  - Because cross-oscillator taps need per-sample interleaving, a voice renders **sample by sample** through
    `ChainGraph` when any modulation layer taps the other lane; otherwise each lane renders blockwise (fast path).
    Filter/distortion parameters are updated per block and smoothed per sample.
  - Solo: if a strip in a lane has `solo`, that lane's output is the output of that layer (layers below skipped).
  - `pub fn render_voice(&mut self, a_in: &[f32], b_in: &[f32], a_out: &mut [f32], b_out: &mut [f32])`.
- [ ] **Step 1: Failing tests** (`crates/bpe-sound/tests/chain.rs`): `layers_run_top_to_bottom`
  (filter then distortion ≠ distortion then filter); `solo_cuts_the_chain_at_its_layer`; `a_loop_renders_finite`
  (A FM from B after its filter, B FM from A: 10 s, all samples finite, |y| ≤ 4); `loop_is_reported`;
  `deleted_source_falls_back`; `mix_zero_bypasses`; `twenty_layers_16_voices_8x_real_time` (release, ignored in debug);
  sound golden `chain_patch_golden` (bless in both profiles).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: per-voice layer chains with cross-oscillator sources"`

### Task 6: Folded strips UI

**Files:** create `crates/bpe-studio/src/sound_tab/chain_box.rs`, `strip.rs`, `strip_views.rs`; modify
`sound_tab/osc.rs` (`layers_box` calls `chain_box::show`).

**Interfaces:**
- Consumes: `Knob`, `layer_summary`, `response_db`, `transfer`, `ChainGraph` loops (exposed to the editor via
  `bpe_sound::chain_graph::loops_of(card: &Slot) -> Vec<LayerId>`, pure function).
- Produces: `chain_box::show(ui, cx, lane: u8, tab: &mut SoundTab)` — header "Osc A layers" + "+ Add layer" menu
  (Modulation, Filter, Distortion); strips as in board ChainFold: fold arrow, on switch, name + summary, mini view
  (76 × 32: filter curve / transfer curve / modulated wave), Mix mini-knob (28 px). Open strip: type `menu_button`,
  slope chips, Solo chip "S", a 64 px view, knob row. Filter view handle: drag x → cutoff (log), y → reso. Loop
  mark: a small circular-arrow icon painted next to the source name, hover "Feeds back one sample late".
  `SoundTab.open_strips: BTreeSet<(ThingId, LayerId)>`.
- [ ] **Step 1: Failing kittest tests:** `add_layer_adds_a_strip_to_that_osc`, `clicking_a_strip_opens_it_and_several_can_be_open`,
  `dragging_the_filter_handle_sets_cutoff_and_reso`, `solo_chip_sets_solo`, `loop_mark_shows_on_a_looping_source`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: layer chains as folded strips"`

### Task 7: Reorder, rename, duplicate, copy and paste strips

**Files:** modify `strip.rs`, `chain_box.rs`; `Inspector.clipboard` gains `Clipboard::ChainLayer(CopiedLayer)`.

**Interfaces:**
- Consumes: core `move_layer` (lane-aware, part 1), `duplicate_layer`, `rename_layer`, `copy_layer`, `paste_layer`.
- Produces: drag a strip by its header to reorder within its lane; right-click menu Rename (in place), Duplicate,
  Copy layer, Paste layer (into this lane, after this strip; a pasted Modulation keeps its source if valid in the
  target card, else falls back), Delete (routes to its knobs are dropped by core `remove_layer`).
- [ ] **Step 1: Failing kittest tests:** `drag_reorders_within_the_lane`, `paste_into_the_other_osc_keeps_a_valid_source`,
  `delete_removes_routes_to_its_knobs`, `rename_in_place`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: reorder, copy and paste layer strips"`

### Task 8: FM / PD A/B clips and finish

- [ ] **Step 1:** Add `#[ignore]` test `render_fm_pd_ab_clips` in `bpe-sound/tests/ab_clips.rs` that writes four
  4 s WAVs (FM first, FM after filter, PD first, PD after filter) with `render_to_wav`; run it with
  `--ignored` and copy the WAVs to `/mnt/project-files/ballphysics/sound-system/part3/fm-pd-ab/`.
- [ ] **Step 2:** `cargo fmt`, clippy `-D warnings`, `cargo test --workspace` debug + release, bless sound goldens.
- [ ] **Step 3:** Update `CLAUDE.md` (chain layers, chain graph). Windows cross-build.
- [ ] **Step 4:** One Opus review; fix.
- [ ] **Step 5:** PR (attribution block, assign + review `pixelpitch161`), linking the A/B clips; exes to
  `/mnt/project-files/ballphysics/sound-system/part3/`.
