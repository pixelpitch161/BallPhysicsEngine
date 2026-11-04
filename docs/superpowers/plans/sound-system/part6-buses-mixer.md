# Sound system part 6: buses and the Mixer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three effect buses with sends, sidechain and eleven effect layers, FL-style console strips in the Mixer
panel, a full master editor, and stems and MIDI in the export.

**Architecture:** Bus settings on the scene settings thing's `sound.mixer` card (`bus1_…`..`bus3_…`), bus effects
as its `Slot.layers` (section `LayerSection::Effect`, `lane` = bus index). Effects DSP in `bpe-dsp/src/fx/`; the
bus graph (topological send order, sidechain taps, solo/mute) in `bpe-sound/src/buses.rs`; console strips and the
effect editors in `bpe-studio/src/mixer/`; editors open in the wide inspector (the part 2 width machinery).

**Tech Stack:** Rust 2024, `hound 3.5` (stems), `midly 0.5` (MIDI), egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` ("Part 6"). Mockups: canvas boards
**MixerStrips** and **EqEditor** (`project/MixerStrips.dc.html`, `project/EqEditor.dc.html` at
https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA). Decisions log: `docs/superpowers/plans/sound-system/decisions.md`.
**Starts after part 2 is merged**; parallel with parts 3 and 5; merges **last** of the three (rebase on main).

## Global Constraints

- Part 1 and 2 constraints hold. Bus effects run in **stereo, once per bus** (not per voice).
- Bus 1 on; Buses 2 and 3 **off** by default. A shape sent to an off bus is silent and its Output warns.
- Sends are **post-fader**. Buses may send to each other; a send that would make a loop is refused (greyed with a
  note). Processing order: shapes → buses in topological send order → master.
- Bus knobs and faders take **keyframes, no modulators**; any setting can Follow a bus's level (part 4 wires
  Follow sound; part 6 exposes `SoundSource::Bus(n)` levels — add the variant in Task 1).
- EQ: 8 bands (Cut, Low shelf, Bell, Notch, High shelf), Frequency/Gain/Q, cut slopes 12/24/48 dB, Both/Mid/Side,
  analyser Post/Pre/Off; starts with band 1 a **30 Hz low cut**, the rest flat (off).
- Compressor sidechain: **Self (default), another bus, or Hits on a chosen shape**; sidechain Low cut and High cut.
- Delay: Standard, Ping-pong, Multi-tap (4 taps); ms or synced 1/64..2 bars (dotted, triplet); Digital, Analog,
  Tape.
- Reverb: Plate (Dattorro), Hall (modulated FDN), Spring (dispersive allpass chain with drip); Freeze.
- Limiter lookahead fixed **1.5 ms**. Master ceiling default **−1 dBTP**; loudness graph with a **−14** mark.
- Stems: one WAV per bus and one for the master, **48 kHz 24-bit**. MIDI: one track per shape, notes at the exact
  hit times, with the tempo.
- Mixer panel 420 px; a slot opens its editor in the wide inspector (640..960, default 760).

## Review Focus

1. **Bus 2 off but a shape's Send to Bus 2 up**: no sound from that send, and the send knob shows the bus is off.
   Task 2 test + Task 6 kittest.
2. **Reverb with Freeze on, then off**: the tail resumes decaying, never grows. Task 4 test.
3. **Delay time keyframed (Tape character)**: pitch bends smoothly, no zipper or clicks; Digital re-pitches with a
   crossfade instead of jumping. Task 4 test.
4. **Sidechain from Hits on a shape that has been deleted**: compressor falls back to Self. Task 3 test.
5. **Export with stems while a bus is muted or soloed**: stems follow the mute/solo the mix uses, and stems summed
   equal the mix before the master limiter. Task 8 test.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-kit/src/sound/mixer.rs` | bus fields on `MixerSettings` |
| `crates/bpe-kit/src/sound/fx.rs` (new) | the eleven effect layer kinds |
| `crates/bpe-dsp/src/fx/{eq.rs, compressor.rs, delay.rs, reverb_plate.rs, reverb_hall.rs, reverb_spring.rs, modfx.rs, width.rs, analyser.rs}` (new) | effects |
| `crates/bpe-sound/src/buses.rs` (new), `fx_chain.rs` (new) | bus graph, sends, sidechain, solo/mute, per-bus effect chains |
| `crates/bpe-sound/src/stems.rs`, `midi.rs` (new) | stems and MIDI writers |
| `crates/bpe-studio/src/mixer/{strips.rs, slot_menu.rs, editors/…}` (new) | console strips and effect editors |
| `crates/bpe-studio/src/export_window.rs`, `crates/bpe-cli/src/lib.rs` | Sound section of the export |

---

### Task 1: Bus settings and effect layer schemas

**Files:** modify `crates/bpe-kit/src/sound/mixer.rs`; create `crates/bpe-kit/src/sound/fx.rs`; modify
`crates/bpe-core/src/animation.rs` (`SoundSource::Bus(u8)`).

**Interfaces:**
- Produces:
  - `BusSettings` (derive, expanded three times with `prefix = "bus1_"/"bus2_"/"bus3_"` inside `MixerSettings`):
    `name: Text` ("Bus 1".."Bus 3"), `on: bool` (Bus 1 true), `fader` (dB −60..+6, 0, keyframe), `pan`
    (−100..100, keyframe), `mute`, `solo: bool`, `send1`, `send2`, `send3` (dB −60..+6, −60 = off, keyframe; a
    bus's send to itself is never shown).
  - Effect layer kinds, section `Effect`, each with `mix` (0..100 %, 100) and the settings listed in the spec:
    `fx.filter` (reuse part 3's `FilterSettings` fields: depend on `chain.rs` types if part 3 merged first,
    otherwise define `BusFilterSettings` with the same keys; merge order puts part 3 first, so reuse),
    `fx.distortion` (same), `fx.eq` (`band{1..8}_on/_kind/_freq/_gain/_q/_slope/_place`, `analyser`, `output`),
    `fx.compressor` (`threshold` −60..0, `ratio` 1..20, `attack` 0.1..100 ms, `release` 5..2000 ms, `knee` 0..24,
    `makeup` 0..24, `sidechain: Sidechain { SelfInput, Bus, Hits }`, `sc_bus` 1..3, `sc_shape: Target` (one shape),
    `sc_low_cut`, `sc_high_cut`), `fx.delay` (`mode`, `character`, `sync: bool`, `time_ms`, `time_note`
    (Choice 1/64..2 bars with dotted/triplet), `feedback`, `low_cut`, `high_cut`, `reso`, `width`,
    `tap{1..4}_time/_level/_pan`), `fx.reverb` (`kind`, `pre_delay`, `decay`, `size`, `damping`, `low_cut`,
    `high_cut`, `width`, `diffusion`, `modulation`, `early`, `springs`, `tension`, `drip`, `freeze`),
    `fx.limiter` (`ceiling`, `release`), `fx.chorus`/`fx.phaser`/`fx.flanger` (`rate`, `sync`, `rate_note`,
    `depth`, `feedback`, `voices` 2..4, `stages` 2..12, `width`), `fx.width` (`width` 0..200 %, `mono_below`
    20..500 Hz).
  - EQ starting state on add: band 1 `Cut` at 30 Hz 24 dB on; bands 2..8 Bell at 60, 150, 400, 1k, 2.5k, 6k,
    12k Hz, 0 dB, off.
- [ ] **Step 1–5:** tests `three_buses_bus1_on`, `eq_starts_with_a_30hz_cut`, `fx_layers_register_in_the_effect_section`;
  implement; commit `git commit -m "Kit: bus settings and the eleven bus effect layers"`.

### Task 2: Bus graph, sends, mute and solo

**Files:** create `crates/bpe-sound/src/buses.rs`, `fx_chain.rs`; modify `engine.rs` (shapes write into buses
instead of straight to the master; per-shape sends from `out_send1..3`).

**Interfaces:**
- Produces: `pub struct Buses` with `build(mixer: &MixerSettings, layers: &[Layer], params) -> Buses`,
  `fn input(&mut self, bus: u8) -> (&mut [f32; BLOCK], &mut [f32; BLOCK])`, `fn send(&mut self, from_shape_l, r,
  bus: u8, gain)`, `fn process(&mut self, master_l, master_r, sidechain: &SidechainTaps)`;
  `pub fn send_order(sends: [[f32; 3]; 3]) -> Result<[u8; 3], LoopAt>` (topological; a send that would close a
  loop is dropped and reported); `pub fn would_loop(mixer: &MixerSettings, from: u8, to: u8) -> bool` (for the UI).
  Solo: when any bus is soloed, non-soloed buses are silent (their sends too). Mute silences after the fader.
  Off bus: input discarded. `fx_chain.rs`: `pub struct FxChain` runs a bus's effect layers in order with `mix`
  as a dry/wet crossfade (equal-gain for EQ/filter/distortion, equal-power for time effects).
- [ ] **Step 1: Failing tests:** `a_shape_on_bus2_is_silent_while_bus2_is_off`, `post_fader_send_follows_the_fader`,
  `send_loops_are_refused`, `solo_plays_only_soloed_buses`, `mute_silences`, `bus_order_is_topological`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: three buses with sends, mute and solo"`

### Task 3: EQ, compressor and limiter

**Files:** create `crates/bpe-dsp/src/fx/{eq.rs, compressor.rs, analyser.rs}`; reuse part 1 limiter.

**Interfaces:**
- Produces: `Eq8 { set(&[BandParams; 8]), process(l, r), response_db(f) -> f32 }` (RBJ biquads in TPT/SVF form
  for modulation-safe updates; 48 dB cut = 4 cascaded 2nd order; Mid/Side via M = (L+R)/2, S = (L−R)/2);
  `Compressor { set(&CompParams), process(l, r, sc_l, sc_r), gain_reduction_db() }` (feed-forward, RMS-peak
  hybrid detector, soft knee, makeup, mix); `HitDucker` (sidechain from hits: each accepted hit on the chosen
  shape injects a unit pulse into the detector, shaped by attack/release); `Analyser` (2048-point FFT, Hann,
  75 % overlap, smoothed, written to a lock-free triple buffer of 256 log-spaced bins the editor reads).
- [ ] **Step 1: Failing tests:** `eq_curve_by_sine_sweep_matches_response` (±0.3 dB), `mid_side_bands_affect_only_their_part`,
  `compressor_static_curve` (−30 dB in → output per ratio/threshold/knee ±0.5 dB), `compressor_attack_and_release_times`
  (63 % within ±10 %), `hits_sidechain_ducks_on_each_hit`, `missing_sidechain_shape_falls_back_to_self`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "DSP: 8-band EQ, compressor with sidechain, analyser"`

### Task 4: Delay and reverbs

**Files:** create `crates/bpe-dsp/src/fx/{delay.rs, reverb_plate.rs, reverb_hall.rs, reverb_spring.rs}`.

**Interfaces:**
- Produces: `Delay { set(&DelayParams, tempo: &TempoInfo), process(l, r) }` — Standard / Ping-pong / Multi-tap
  (4 taps), feedback path with low cut, high cut (resonant SVF) and character: Digital (clean; time changes
  crossfade over 50 ms), Analog (one-pole darkening + soft tanh per repeat), Tape (wow 0.5 Hz / flutter 6 Hz
  modulated read, tanh saturation, −72 dBFS hiss; time changes glide the read head → pitch bend). Max time 4 s
  (allocated in `prepare`). `Plate` (Dattorro 1997 topology, size scales the delay lengths, modulation on the
  tank allpasses), `Hall` (16-line FDN, Hadamard matrix, per-line damping set from Decay (RT60) and Damping,
  slow random modulation, early reflections tap pattern), `Spring` (1..3 springs, each a chain of 64 first-order
  allpasses for dispersion in a feedback loop with a delay set by Tension, Drip = high-passed chirp emphasis).
  All reverbs: pre-delay up to 500 ms, low/high cut on the wet, width (M/S), Freeze (feedback 1.0, input muted,
  damping bypassed).
- [ ] **Step 1: Failing tests:** `delay_times_to_the_sample` (impulse → echoes at exact sample indices, synced
  1/8 at 120 BPM = 12000 samples), `ping_pong_alternates_sides`, `multi_tap_levels_and_pans`,
  `tape_time_change_bends_without_clicks`, `reverb_decays_at_its_set_time` (RT60 measured by Schroeder
  integration within ±15 % for Decay 1 s and 4 s, each kind), `longest_decay_stays_bounded` (Decay max, 60 s of
  noise in then silence: peak never grows after input stops), `freeze_off_resumes_decay`.
- [ ] **Step 2–4:** FAIL, implement, PASS (release). **Step 5: Commit** `git commit -m "DSP: delay with three characters; plate, hall and spring reverbs"`

### Task 5: Chorus, phaser, flanger and width

**Files:** create `crates/bpe-dsp/src/fx/{modfx.rs, width.rs}`.

**Interfaces:**
- Produces: `Chorus` (2..4 voices, 7–25 ms modulated delays, spread across stereo), `Phaser` (2..12 first-order
  allpass stages, feedback, LFO in Hz or synced), `Flanger` (0.1–8 ms modulated delay, feedback ±), `Width`
  (M/S gain 0..2 with a Linkwitz-Riley 2nd-order crossover at `mono_below`, below which S = 0) plus
  `correlation()` meter (−1..1, 300 ms window).
- [ ] **Step 1: Failing tests:** `width_zero_is_mono`, `mono_below_removes_low_side`, `phaser_notch_count_follows_stages`,
  `flanger_feedback_stays_bounded`, `chorus_keeps_level` (RMS within ±1.5 dB).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "DSP: chorus, phaser, flanger and width"`

### Task 6: Console strips

**Files:** create `crates/bpe-studio/src/mixer/strips.rs`, `slot_menu.rs`; modify `mixer/mod.rs`
(`bus_strips` from part 1) and `mixer/master.rs` (compact strip form beside the buses).

**Interfaces:**
- Consumes: `Knob`, `level_meter`, `would_loop`, engine meters (add `Meters.buses: [[AtomicU32; 2]; 3]` peaks and
  per-bus gain reduction).
- Produces: board MixerStrips: four equal columns (Bus 1, Bus 2, Bus 3, Master). A bus column: on switch + name
  (double-click to rename), "Layers" slot list (each `slot` row: on dot, effect name; scroll past five; last row
  "+" opens the add menu of the eleven kinds), "Sends" (two rows "Bus N  −12 dB", a small knob each; greyed with
  hover "That would feed back into itself" when `would_loop`), Pan knob, fader (vertical, 150 px, dB scale) +
  stereo meter, M and S chips, the fader value. Off buses draw at `faint` with their controls disabled.
  Hovering a column sets `Studio.hover_bus` so the tree and preview tint shapes on that bus (tree row ring,
  preview outline in the shape's own colour). Slot interactions: click opens its editor (Task 8), drag to reorder
  or onto another bus column (moves the layer and sets its `lane`), right-click Rename, Duplicate, Copy, Paste,
  Bypass (layer `enabled = false`), Delete.
  Shape Output (part 2 `output.rs`): Bus picker enables Buses 2 and 3; shows Send knobs for the other two buses;
  warning line "Bus 2 is off" when the chosen bus is off.
- [ ] **Step 1: Failing kittest tests:** `four_columns_bus2_and_3_dimmed`, `adding_a_layer_to_bus1`,
  `dragging_a_slot_to_bus2_moves_it`, `a_looping_send_is_greyed`, `fader_and_pan_edit_and_undo`,
  `shape_output_warns_when_its_bus_is_off`, `bus_rename_by_double_click`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: Mixer console strips"`

### Task 7: Effect editors in the wide inspector

**Files:** create `crates/bpe-studio/src/mixer/editors/{mod.rs, eq.rs, comp.rs, delay.rs, reverb.rs, generic.rs, master.rs}`;
modify `inspector.rs` (a mixer selection mode) and `app.rs` (width).

**Interfaces:**
- Produces: `pub enum MixerPick { Layer(LayerId), Master }` in `Studio.mixer_pick: Option<MixerPick>`. While set
  (and the left panel is in Mixer mode), the inspector shows that editor titled "Bus 1 · Equalizer" at the sound
  width (640..960). Selecting a thing in the tree clears it.
  - `eq.rs` (board EqEditor): Analyser Post/Pre/Off chips, Mix; a 300 px curve area with log frequency grid
    (50..10k labels) and ±18 dB; analyser fill behind; curve in `TEXT_STRONG`; 8 numbered points; drag a point →
    freq + gain, scroll → Q, double-click empty curve → turn on the nearest off band at that frequency,
    right-click a point → Turn off / Delete (reset to flat off); band chips 1..8 under; picked band row: kind chips,
    slope chips, Both/Mid/Side chips, Frequency, Gain, Q, Output knobs.
  - `comp.rs`: knobs, a level graph (input level line, output line, GR fill), sidechain source chips and pickers,
    sidechain filter knobs. `delay.rs`, `reverb.rs` per the spec's settings; `generic.rs` for Filter, Distortion,
    Limiter, Chorus, Phaser, Flanger, Width (knob grid + a small view where the kind has one).
  - `master.rs`: Input, Output, Ceiling, Release knobs, loudness graph with the −14 mark, GR meter, readouts.
- [ ] **Step 1: Failing kittest tests:** `clicking_a_slot_opens_its_editor_wide`, `dragging_an_eq_point_sets_freq_and_gain`,
  `double_click_curve_turns_on_a_band`, `selecting_a_thing_closes_the_bus_editor`, `master_editor_shows_the_minus_14_mark`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: bus effect editors and the master editor"`

### Task 8: Stems and MIDI export

**Files:** create `crates/bpe-sound/src/stems.rs`, `midi.rs`; modify `offline.rs`, `crates/bpe-cli/src/lib.rs`
(`ExportOptions.stems: bool`, `midi: bool`), `crates/bpe-studio/src/export_window.rs` (Sound section).

**Interfaces:**
- Produces: `pub fn render_with_extras(scene, kit, out_dir, base_name, seconds, stems: bool, midi: bool,
  progress) -> anyhow::Result<RenderedSound>` writing `<name>.wav` (mix, float, temp for the mp4) plus
  `<name> - Bus 1.wav`… and `<name> - Master.wav` (24-bit PCM, the bus outputs **after** fader/mute/solo, the
  master **after** the limiter) and `<name>.mid` (format 1, PPQ 960, tempo meta from `TempoMap`, one track per
  shape named after it, notes at exact hit sample times converted to ticks, velocity from hit strength, note
  length = until the voice's gate closes). The engine exposes a `NoteLog` it fills when `record_notes` is on
  (off in the editor). Export window: a "Sound" fold with "Mixed sound" (always), "Also save stems" and "Also
  save MIDI" switches.
- [ ] **Step 1: Failing tests:** `stems_sum_to_the_mix_before_the_limiter` (sum of bus stems equals the master
  input within 24-bit quantisation), `muted_bus_stem_is_silent`, `midi_round_trips_the_played_notes` (parse with
  `midly`: note numbers and tick times match the `NoteLog`), `export_window_has_stem_and_midi_switches`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Export: stems and MIDI"`

### Task 9: Finish the part

- [ ] **Step 1:** Rebase on main after parts 3 and 5 merge; reuse part 3's filter/distortion types for the bus
  versions (stereo wrappers).
- [ ] **Step 2:** fmt, clippy `-D warnings`, tests debug + release, bless sound goldens (bus routing changes
  them only if a golden scene routes off Bus 1: it should not).
- [ ] **Step 3:** Update `CLAUDE.md` (mixer card, buses, effects, stems/MIDI). Windows cross-build.
- [ ] **Step 4:** One Opus review; fix.
- [ ] **Step 5:** PR (attribution block, assign + review `pixelpitch161`); exes to
  `/mnt/project-files/ballphysics/sound-system/part6/`.
