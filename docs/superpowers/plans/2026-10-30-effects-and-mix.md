# Effects, Channels, Buses and Master Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Part 3 of the music redesign. It adds effect blocks and effects graphs in four places, each shown as a list when it is a straight chain. It also adds per-channel pan, inserts and sends, return buses, and an always-on master stage with real LUFS, a clipper and a true-peak limiter, plus meters and tails.

**Architecture:**
- **bpe-synth** gains effect blocks, plus a second compile mode where `fx_in`/`fx_out` replace `note`/`voice_out`. A compiled effects program runs over a whole interleaved buffer.
- **bpe-core** stores the graphs:
  - `Instrument.fx`
  - `Module.channel`
  - `Library.buses`
  - `Library.master_fx`

  An empty graph means "no effects": audio passes through.
- **bpe-mix** renders in this order: voices → instrument fx → channel inserts → sends → buses → master fx → master stage. Each stage is cached by fingerprint.
- **bpe-kit** wires settings and the song, which is mixed before the master stage.
- **bpe-studio** gets the list view, an Effects target in the node panel, the Channel, Buses and Master sections, and the meters.

**Tech Stack:** Rust workspace, egui 0.37 with kittest, our own DSP (no fundsp), hound.

**Spec:** `docs/superpowers/specs/2026-10-30-music-synth-design.md`, sections 2.6, 2.7, 3, 6 and 7, and the part 3 line of section 9. Parts 1 and 2 are merged (PRs #47 and #49).

## Global Constraints

- Blocks are 64 frames (`bpe_synth::BLOCK`) at 48 kHz. Everything is deterministic: the same input always gives the same bits.
- Loudness target: real integrated loudness (ITU-R BS.1770 K-weighting, gated), default -14 LUFS, range -30 to -6. The whole render is measured first, then one gain is applied.
- Clipper: off (default), soft or hard, with drive in dB. Clipping is 4x oversampled.
- Limiter: true-peak (4x oversampled detection), lookahead 5 ms, ceiling default -1 dBTP (range -6 to 0), release in ms, or auto-release.
- New scenes start with two return buses: "Reverb" (a Reverb block) and "Delay" (a quarter-note ping-pong Delay). Sends are -inf dB by default.
- Signal path: voices → instrument effects → channel inserts → volume and pan → sends (post-fader) → master effects graph → master stage. Return buses go through their effects graph into the master effects graph. The song is mixed into the master effects output, before the master stage.
- Tails: effects run to the end of the video, and the master output fades out over the last 0.5 s.
- A broken effects graph passes audio through, shows the problem on its block, and adds a status line. Non-finite samples become 0 and add a status line.
- UI copy: plain words with no ♭ ♯ ▶ glyphs (the editor font lacks them). The inspector is at most 640 px wide.
- Old scenes need not load. There is no backward compatibility (Aedan, 2026-10-30).

## Review Focus

1. **An effects graph that is empty, only has In → Out, or has no fx_in wired** must pass audio through bit-exactly (Task 1 test `empty_and_bare_graphs_pass_through`).
2. **Feedback and resonance at their maxima** (delay feedback 1, reverb decay 60 s, compressor ratio 20 at a -60 dB threshold) must stay finite and below +12 dBFS for 60 s (Task 3 `extreme_effect_knobs_stay_finite`).
3. **A silent scene, or one with a single click**, must not get a huge gain from loudness matching. Gain is capped at +24 dB, and a silent mix stays silent (Task 6 `silence_and_a_click_stay_sane`).
4. **Removing a bus that sends point at** must leave the sends dangling but harmless: they are ignored, with no panic and no problem line (Task 7 `a_send_to_a_removed_bus_is_ignored`).
5. **A mono or very short video** (1 s, shorter than the reverb pre-delay plus the 0.5 s fade) must render with the right length and no panic (Task 7 `a_one_second_scene_mixes`).

---

### Task 1: Effects graphs compile and run

**Files:**
- Modify: `crates/bpe-synth/src/catalog.rs`, `crates/bpe-synth/src/program.rs`, `crates/bpe-synth/src/lib.rs`
- Create: `crates/bpe-synth/src/effects.rs`

**Interfaces:**
- Produces:
  - Catalog blocks `fx_in` ("In", category "Output", one audio output `out`) and `fx_out` ("Out", one required audio input `left` plus an optional `right`).
  - `pub fn compile_effects(graph: &Graph) -> Result<Program, Vec<Diagnostic>>`
  - `impl Program { pub fn run(&self, audio: &mut [f32], cx: Ctx) }` processes interleaved stereo in place, block by block, with `cx.advance()` per block and the last partial block zero-padded.
  - `pub fn effects_or_through(graph: &Graph) -> (Option<Program>, Option<String>)` gives `(None, None)` for an empty graph, `(Some, None)` when it compiles, and `(None, Some(first message))` when it doesn't.
  - `pub fn straight_chain(graph: &Graph) -> Option<Vec<u32>>` gives the block ids between In and Out in order, or `Some(vec![])` for an empty or bare graph.
  - `pub fn chain_graph(blocks: &[(String, BTreeMap<String, Param>, bool)]) -> Graph` takes (kind, knobs, enabled) and lays the blocks out 180 px apart.
  - `pub fn effects_catalog_kinds() -> Vec<&'static str>` lists the effect kinds the "Add effect" menu offers.

- [ ] **Step 1: Write the failing tests** in `effects.rs`:
  - `empty_and_bare_graphs_pass_through`:
    - `Graph::new("x")` gives `(None, None)`.
    - `chain_graph(&[])` compiles, and running it on a 1,000-frame noise buffer leaves it bit-identical.
  - `chain_round_trips`:
    - `chain_graph` with [drive (amount 0.5), amp (enabled false)] → `straight_chain` → the same kinds in order.
    - Adding a second wire from In into the amp gives `None`.
  - `wrong_ends_are_rejected`:
    - An effects graph with `voice_out` gives the diagnostic "Use In and Out, not Voice out, in effects".
    - A patch with `fx_out` gives `compile` diagnostic "Out is for effects; use Voice out".
    - Two `fx_out` give "Use only one Out block".
  - `a_drive_chain_changes_the_sound`: a chain of [drive, amount 1] on a 0.5-amplitude sine changes the peak.
- [ ] **Step 2: Run** `cargo test -p bpe-synth effects`. Expected: FAIL to compile (the functions don't exist).
- [ ] **Step 3: Implement.**
  - `compile` and `compile_effects` share one body, `compile_as(graph, Mode::{Patch, Effects})`.
  - The register of `fx_in` output 0 becomes `Program.input: Option<usize>`. `run` copies each input block into it before the ops run.
  - `fx_out` is handled like `voice_out`: one register, its right side copied from the left when the right isn't wired.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. Expected: all pass, including the part 2 tests.
- [ ] **Step 5: Commit** "feat(synth): effects graphs with In and Out".

### Task 2: Tone effect blocks

**Files:**
- Create: `crates/bpe-synth/src/blocks/eq.rs`, `crates/bpe-synth/src/blocks/space.rs` (width and envelope follower), `crates/bpe-synth/src/blocks/crush.rs`
- Modify: `blocks/filter.rs`, `blocks/drive.rs`, `blocks/mod.rs`, `catalog.rs`

**Interfaces:**
- Catalog entries (kind: knobs with defaults). All are in category "Effects" unless noted.
  - `eq`:
    - `low_cut` 20 (20-1000 Hz)
    - `low_gain` 0 at `low_freq` 120
    - `mid_gain` 0 at `mid_freq` 1000 with `mid_q` 0.7
    - `high_gain` 0 at `high_freq` 8000
    - `high_cut` 20000
    - gains range -18 to 18 dB
    - RBJ biquads: low shelf, peak, high shelf, plus 12 dB/oct cuts that are bypassed at their extremes.
  - `filter` gains:
    - `sweep` with choices "off", "lfo", "1/1", "1/2", "1/4", "1/8", "1/16"
    - `sweep_rate` 1 Hz
    - `sweep_depth` 0 to 4 octaves, default 0
    - The sweep is free-running on `cx.time`, so effects stay in step across blocks.
  - `drive`:
    - Gains the mode "tube", which is asymmetric: `tanh(x + 0.2) - tanh(0.2)`.
    - Gains `tone` 0 to 1 (a one-pole low-pass after shaping, from 1 kHz to 20 kHz, default 1) and `mix` 0 to 1 (default 1).
  - `bitcrush`: `bits` 1 to 16 (default 8), `downsample` 1 to 32 (default 1), `mix` 0 to 1 (default 1).
  - `width`: `width` 0 to 2 (default 1). It works on mid/side: side × width.
  - `follower` (category "Modulators"): audio in, number out, `attack` 0.01 s, `release` 0.2 s. It gives the peak envelope of max(|L|, |R|).

- [ ] **Step 1: Write failing tests** (in each file):
  - `eq_peak_gain_at_centre`: `mid_gain` +6 at 1 kHz on a 1 kHz sine gives +6 dB ±0.5. At 100 Hz the change is under 1 dB.
  - `eq_flat_is_transparent`: the defaults change a noise buffer by less than -80 dB RMS.
  - `filter_sweep_moves_the_cutoff`: with an LFO sweep at depth 2, the RMS over windows varies by more than 3 dB on noise.
  - `tube_is_asymmetric`: the output to ±0.8 differs in magnitude.
  - `drive_mix_zero_is_dry`: bit-exact.
  - `one_bit_has_two_levels`: `bits` 1 on a sine gives at most 2 distinct values per side.
  - `width_zero_is_mono`: L == R exactly.
  - `follower_tracks_a_burst`: a burst of 0.5 for 0.1 s reads above 0.45 by the end of the burst and below 0.1 at 0.5 s after.
- [ ] **Step 2: Run** `cargo test -p bpe-synth blocks`. Expected: FAIL.
- [ ] **Step 3: Implement** the blocks and register them in `make` and the catalog. Flush denormals in every recursive filter state with `filter::flush`.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(synth): EQ, filter sweep, tube drive, bitcrush, width, follower".

### Task 3: Time effect blocks

**Files:**
- Create: `crates/bpe-synth/src/blocks/delay.rs`, `crates/bpe-synth/src/blocks/chorus.rs`, `crates/bpe-synth/src/blocks/reverb.rs`
- Modify: `blocks/mod.rs`, `catalog.rs`

**Interfaces:**
- `delay`:
  - `time_mode` with choices "ms", "1/4", "1/8", "1/16", "1/2", "1/1"
  - `feel` with choices "straight", "dotted", "triplet"
  - `ms` 250 (1 to 2000)
  - `feedback` 0.35 (0 to 1)
  - `ping_pong` false
  - `low_cut` 20, `high_cut` 20000 (in the feedback path)
  - `mix` 0.3
  - A tempo division is `fraction × 4 beats × seconds_per_beat`, so "1/4" is one beat. "dotted" is ×1.5 and "triplet" is ×2/3.
  - It uses an integer delay in frames, rounded.
- `chorus`: `rate` 0.8 Hz, `depth` 3 ms, `voices` 1 to 4 (default 2), `mix` 0.5. Each voice is a modulated fractional delay (linear interpolation) around 12 ms, with voice phases spread evenly and L/R in opposite phase.
- `reverb`:
  - `size` 0.6, `decay` 2 s (0.1 to 60, this is the RT60), `damping` 0.4, `predelay` 10 ms (0 to 200), `width` 1, `mix` 0.3.
  - It is a Freeverb-style network: 8 combs and 4 allpasses per side, with the right side's lengths +23. The comb lengths are scaled by `0.5 + size`.
  - Comb feedback is `g = 10^(-3 × comb_len_s / decay)`, so each comb reaches -60 dB at `decay`.
  - The comb lengths are modulated ±0.5 frames by slow sines at 0.1 to 0.3 Hz.
  - Its help text includes "Inside a patch it runs on every voice." The same note goes on the delay.

- [ ] **Step 1: Write failing tests:**
  - `delay_is_exact_to_the_sample`:
    - ms mode at 250 ms, mix 1, feedback 0: an impulse at frame 0 comes out at frame 12000 with magnitude 1 ±1e-6, and at nothing else.
    - "1/8 dotted" at spb 0.5 gives 0.375 s = 18000 frames.
  - `ping_pong_alternates`: the first echo is on L only, the second on R only.
  - `reverb_tail_reaches_minus_60_at_decay`: decay 2 s, mix 1, predelay 0. Measure the energy-decay (Schroeder) curve of the impulse response. The fit from -5 to -35 dB, extrapolated to -60, gives a time within ±10% of 2 s.
  - `chorus_keeps_level`: the RMS of the output on a sine is within 3 dB of the input.
  - `extreme_effect_knobs_stay_finite`: feedback 1, decay 60, chorus depth max, on 60 s of saw. Every sample is finite with |s| < 4.
- [ ] **Step 2: Run** `cargo test -p bpe-synth`. Expected: FAIL.
- [ ] **Step 3: Implement** with heap ring buffers sized at reset from the knobs (the max delay is 2 s + 1 block). Clamp feedback to ≤ 0.98 internally. Flush denormals.
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(synth): delay, chorus and reverb blocks".

### Task 4: Dynamics: compressor, clipper and limiter

**Files:**
- Create: `crates/bpe-synth/src/blocks/dynamics.rs`
- Modify: `blocks/mod.rs`, `catalog.rs`, `lib.rs`

**Interfaces:**
- `pub struct Oversampler4` is a 4x polyphase half-band pair (a 2x up and 2x down cascade, with a fixed FIR of 31 taps per stage).
  - `fn new() -> Self`
  - `fn up(&mut self, x: f32) -> [f32; 4]`
  - `fn down(&mut self, x: [f32; 4]) -> f32`
  - `pub fn true_peak(samples: &[f32]) -> f32` gives the largest |x| of the 4x-upsampled interleaved stereo.
- `pub fn soft_clip(x: f32) -> f32` is `tanh`. `hard_clip` is a clamp to ±1.
- `pub struct TruePeakLimiter`:
  - `new(rate, ceiling_db, release_ms: Option<f32>)`, where `None` means auto-release: 50 ms that grows to 500 ms with the share of the last second spent in reduction.
  - `process(&mut self, l: &mut [f32], r: &mut [f32]) -> Vec<f32>` works on whole buffers and returns gain reduction in dB per 64-frame block.
  - Lookahead is 5 ms (240 frames). Detection is the 4x true peak of each frame. A sliding minimum of the needed gain over the lookahead window (as `limit_peaks` does today) is smoothed by a 1-frame attack ramp across the window.
- Catalog:
  - `compressor`:
    - inputs `in` (required) and `sidechain` (audio, optional)
    - `threshold` -18 (-60 to 0 dB), `ratio` 4 (1 to 20), `attack` 10 ms, `release` 100 ms, `knee` 6 dB, `makeup` 0 dB
    - The detector is an RMS-ish peak follower on the sidechain, or on the input when the sidechain is unwired.
  - `clipper`: `mode` with choices "soft" and "hard", plus `drive` 0 dB (0 to 24). It is 4x oversampled.
  - `limiter`: `ceiling` -1 (-6 to 0), `release` 100 ms. The block version is streaming, with lookahead delay.

- [ ] **Step 1: Write failing tests:**
  - `compressor_follows_its_ratio`:
    - Threshold -20, ratio 4, knee 0, on a steady -8 dB sine (12 dB over the threshold). After settling, the output is -17 dB ±0.5.
  - `sidechain_ducks`: a loud sidechain lowers a steady input by more than 6 dB, and an unwired sidechain doesn't.
  - `true_peak_sees_intersample_peaks`: an fs/4 sine at 45° phase, amplitude 1, has sample peak 0.707 and true peak above 0.98.
  - `limiter_holds_the_true_peak_ceiling`:
    - Ceiling -1. The test set is: squares at 100 Hz and 5 kHz at +6 dB, the intersample sine at +6 dB, and white noise at +12 dB.
    - Every output true peak is ≤ -1 dBTP + 0.1 dB.
  - `clipper_hard_never_exceeds_one`: the downsampled peak is ≤ 1.02 at drive 24 dB.
- [ ] **Step 2: Run** `cargo test -p bpe-synth dynamics`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-synth`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(synth): compressor, clipper and true-peak limiter".

### Task 5: Where effects live, and channel pan

**Files:**
- Modify:
  - `crates/bpe-core/src/instrument.rs`
  - `crates/bpe-core/src/library.rs`
  - `crates/bpe-core/src/lib.rs` (exports)
  - `crates/bpe-kit/src/things/scene/mod.rs`
  - `crates/bpe-kit/src/modules/{sound,drums,melody}/mod.rs`
  - `crates/bpe-mix/src/lib.rs` (`notes_with` applies pan)
  - `crates/bpe-kit/src/lib.rs` (`new_scene` buses)

**Interfaces:**
- `Instrument.fx: bpe_graph::Graph` (serde default, skipped when it has no nodes).
- `pub struct BusId(pub u32)` (Ord, serde).
- `pub struct Bus { pub id: BusId, pub name: String, pub volume: f32 /* dB */, pub fx: Graph }`.
- `pub struct ChannelStrip { pub inserts: Graph, pub sends: BTreeMap<BusId, f32> /* dB; absent = -inf */ }` (Default, `is_default()`).
- `Module.channel: ChannelStrip`.
- `Library.buses: Vec<Bus>` and `Library.master_fx: Graph`.
- `Library::add_bus(&mut self, name, fx) -> BusId` uses the next free id.
- `SceneSettings` gets a group "Master":
  - `loudness` (moved into this group)
  - `clipper: Clip` with values Off, Soft and Hard
  - `clip_drive` 0 to 24 dB, default 0, `when = "clipper=soft|hard"`
  - `ceiling` -6 to 0 dBTP, default -1
  - `auto_release: bool` = true
  - `release` 10 to 1000 ms, default 100, `when = "!auto_release"`

  Check how `when` writes negation in `SettingDef` and use the existing syntax. If there is none, show `release` always and say "used when Auto release is off".
- The sound, drums and melody settings get `pan: f32`: range -1 to 1, default 0, keyframe, label "Pan".
- `notes_with` reads `pan` from the (possibly animated) slot params, then rebalances each new note's `left` and `right` with constant power around the note's own position:
  ```
  l' = l·cos((p+1)π/4)·√2
  r' = r·sin((p+1)π/4)·√2
  ```
  With p = 0 the note is unchanged.
- `bpe_kit::new_scene` adds the buses "Reverb" (`chain_graph([reverb])`) and "Delay" (`chain_graph([delay with time_mode "1/4", ping_pong true, mix 1])`). The Reverb bus also gets mix 1.

- [ ] **Step 1: Write failing tests:**
  - `bpe-core` `library_round_trips_buses_and_strips`: RON serialises and deserialises a library with a bus, a strip with a send, and an instrument with fx, and the result is equal.
  - `bpe-mix` `pan_moves_a_note`: with the test pluck and `pan` -1, every note has `right` ≈ 0 and `left` ≈ √2·old.
  - `bpe-kit` `new_scenes_have_reverb_and_delay_buses`: two buses named "Reverb" and "Delay". Their fx `straight_chain` are `["reverb"]` and `["delay"]`.
- [ ] **Step 2: Run** `cargo test -p bpe-core -p bpe-mix -p bpe-kit`. Expected: FAIL.
- [ ] **Step 3: Implement.** Re-bless any scene RON or golden tests that change because of the new fields, and say so in the commit.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(core): effects on instruments, channels, buses and master; channel pan".

### Task 6: The master stage

**Files:**
- Create: `crates/bpe-mix/src/master.rs`
- Modify: `crates/bpe-mix/src/lib.rs`, `crates/bpe-mix/src/level.rs` (remove `normalize_loudness` and `limit_peaks` once nothing uses them, or keep `normalize_loudness` only if the song code still needs it)

**Interfaces:**
- `pub struct MasterSettings { pub loudness: f32, pub clipper: ClipMode /* Off|Soft|Hard */, pub drive_db: f32, pub ceiling_db: f32, pub release_ms: Option<f32> }` (Default -14, Off, 0, -1, None).
- `pub struct MasterReport { pub integrated: f32 /* LUFS */, pub true_peak: f32 /* dBTP */, pub short_term: Vec<f32> /* LUFS every 0.1 s, 3 s window */, pub reduction: Vec<f32> /* dB every 0.1 s, max over the window */ }`.
- `pub fn integrated_lufs(samples: &[f32]) -> f32` follows BS.1770-4:
  - The K-weighting has two biquads: a high shelf with f0 1681.97 Hz, G 3.9998 dB, Q 0.7072, and an RLB high-pass with f0 38.135 Hz, Q 0.5003.
  - Blocks are 400 ms with 75% overlap.
  - Gating is absolute at -70 LUFS, then relative at -10 LU.
  - It gives -70 for silence.
- `pub fn master(samples: &mut [f32], s: &MasterSettings) -> MasterReport` works in this order:
  1. Measure.
  2. Apply one gain to reach `loudness` (capped at +24 dB, and none when integrated is -70).
  3. Clip with 4x oversampling when on. The drive comes before the clip and is compensated after it.
  4. Limit with `TruePeakLimiter`.
  5. Fade over the last 0.5 s (min(0.5 s, length)) linearly to 0.
  6. Measure again for the report.
- `Soundtrack` gains `report: Option<MasterReport>`.

- [ ] **Step 1: Write failing tests:**
  - `lufs_of_a_reference_sine`: a 997 Hz stereo sine at amplitude 0.1 for 10 s reads -20.0 ±0.3 LUFS. Pink-ish noise normalised by `master` to -14 reads -14 ±0.5.
  - `master_hits_the_target_and_ceiling`: a test set of four sources run through `master` with the defaults.
    - The sources are: a dense pluck-like burst train, a 100 Hz square, the intersample sine, and noise.
    - Integrated is within 0.5 LU of -14, *or* the limiter is holding the true peak at ≤ -1 dBTP + 0.1. Check the true peak always.
  - `silence_and_a_click_stay_sane`:
    - Silence stays all zeros, with report integrated -70.
    - A single 1-frame click at 0.5 is gained by at most +24 dB and stays under the ceiling.
  - `the_end_fades_out`: the last frame is 0, and the RMS of the last 0.25 s is under half that of the 0.5 s before the fade.
- [ ] **Step 2: Run** `cargo test -p bpe-mix master`. Expected: FAIL.
- [ ] **Step 3: Implement.** `mix_cached` uses `master` in place of `normalize_loudness` and `limit_peaks`. `MixSettings.loudness` becomes `master: MasterSettings`.
- [ ] **Step 4: Run** `cargo test -p bpe-mix`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(mix): master stage with real LUFS, oversampled clipper and true-peak limiter".

### Task 7: Channels, sends, buses and master effects in the mix

**Files:**
- Modify: `crates/bpe-mix/src/channels.rs`, `crates/bpe-mix/src/lib.rs`
- Create: `crates/bpe-mix/src/buses.rs`

**Interfaces:**
- `MixSettings` gains:
  - `strips: BTreeMap<u32, ChannelStrip>`, by module id
  - `buses: Vec<Bus>`
  - `master_fx: Graph`
- `pub(crate) fn render(...) -> Rendered` now gives `channels: BTreeMap<u32, Arc<Vec<f32>>>`, which is each channel after its instrument fx (per group) and its inserts, summed in key order.
- `pub(crate) fn bus_mix(settings, channels, frames, cache) -> (Vec<f32>, Vec<String>)` does this:
  1. Adds up the channels as the dry mix.
  2. For each bus, sums `channel × 10^(send/20)`, runs the bus fx, applies `10^(volume/20)`, and adds the result.
  3. Runs the master fx over the total.

  The result is the pre-master mix.
- `pub fn mix_premaster(...)` takes the same arguments as `mix_cached` and gives a `Soundtrack` without the master stage. `mix_cached` = `mix_premaster` + `master`.
- Cache layers, each `fingerprint → Arc<Vec<f32>>`:
  - the existing group key, now with the instrument fx `sound_key` in its fingerprint
  - per channel: the member group fingerprints + the inserts' `sound_key`
  - per bus: the channel fingerprints with the send levels, + the bus fx `sound_key` + volume
- Effects run with `Ctx { seconds_per_beat: spb, seed: settings.seed, ..Ctx::default() }` from time 0.
- A broken graph adds the problem "<place>: the effects don't work (<first message>), so they're bypassed". The place is "<instrument name> effects", "<module name> inserts", "<bus name> bus" or "Master effects". Module names come from a new `MixSettings.channel_names: BTreeMap<u32, String>`.
- After every effects run, any non-finite sample becomes 0, with the problem "<place> made bad samples".

- [ ] **Step 1: Write failing tests** (`buses.rs` and `channels.rs` tests, using `tests::settings()` and the test pluck):
  - `no_effects_is_the_old_mix`: with no strips, buses or master fx, the pre-master equals the plain channel sum bit-exactly.
  - `a_send_feeds_the_reverb_tail`:
    - A Reverb bus with mix 1 and a send of 0 dB from the pluck channel gives RMS energy after the last note's natural decay (0.8 s later) more than 20 dB above the same scene without the send.
  - `a_send_to_a_removed_bus_is_ignored`: a strip whose send names BusId(99) mixes the same as no send, and `problems` is empty.
  - `a_broken_insert_passes_through`: a strip whose inserts have two `fx_out` mixes like no inserts, and `problems` holds "inserts" and "bypassed".
  - `changing_a_bus_keeps_the_channels`: the second mix after changing only the bus volume renders `fresh == 0` channels.
  - `a_one_second_scene_mixes`: `duration` 1 with a Reverb bus send has the right frame count and finite samples.
  - `the_mix_is_the_same_twice`: two full mixes with every stage on are bit-identical.
- [ ] **Step 2: Run** `cargo test -p bpe-mix`. Expected: FAIL.
- [ ] **Step 3: Implement.** Each instrument group, channel and bus runs on its own thread with `std::thread::scope`, like the channels today.
- [ ] **Step 4: Run** `cargo test -p bpe-mix`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(mix): channel inserts, sends, return buses and master effects".

### Task 8: Kit and studio plumbing

**Files:**
- Modify:
  - `crates/bpe-kit/src/lib.rs` (`mix_settings`, `mix_scene_cached`)
  - `crates/bpe-kit/src/music/song_mix.rs`
  - `crates/bpe-studio/src/sound_job.rs`
  - `crates/bpe-cli/src/lib.rs`
  - `crates/bpe-kit/src/scenes.rs` (re-bless)

**Interfaces:**
- `mix_settings` fills `master` from the Master group, plus `strips`, `buses`, `master_fx` and `channel_names` from the library.
- `mix_scene_cached` = `mix_premaster` → song `mix_in` → `master`. The report is stored on the track.
- `song_mix::mix_in(track, song, offset, gain_db, ducking, target_db)` no longer levels or limits the track. It sets the song layer to `target_db + gain_db` LUFS with `integrated_lufs`, ducks and adds.
- The `sound_job` fingerprint of the mix inputs includes the library's buses, master fx and each module's channel strip, through `sound_key` and `Debug` of the sends.
- `bpe-cli` render prints `Loudness -14.0 LUFS, true peak -1.2 dBTP` after a render.

- [ ] **Step 1: Write failing tests:**
  - `bpe-kit` `a_scene_with_a_song_is_mastered_once`: a scene with a song and a pluck has report integrated within 1 LU of -14 and true peak ≤ -0.9.
  - `bpe-studio` `a_bus_edit_remixes`: in `sound_job` tests, changing a bus volume changes the job's fingerprint, and changing nothing keeps it.
- [ ] **Step 2: Run** `cargo test -p bpe-kit -p bpe-studio`. Expected: FAIL.
- [ ] **Step 3: Implement.** Re-bless the golden soundtrack hashes the new master changes. Leave the Windows re-bless note in REPORT.
- [ ] **Step 4: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(kit): mastered soundtrack with buses; song mixed before the master".

### Task 9: The effects list view and effects in the node panel

**Files:**
- Create: `crates/bpe-studio/src/fx_list.rs`
- Modify: `crates/bpe-studio/src/nodes/panel.rs`, `crates/bpe-studio/src/main.rs` (mod)

**Interfaces:**
- `pub enum FxPlace { Instrument(InstrumentId), Channel(ModuleId), Bus(BusId), Master }`
- `PanelTarget::Effects(FxPlace)` uses `bpe_synth::catalog()` filtered to effects kinds plus maths and modulators, and compiles with `compile_effects` for block problems. Its header is "<place label> effects".
- `pub fn show(ui: &mut Ui, graph: &Graph, salt: impl Hash) -> FxEdit`:
  - `FxEdit` is `None`, `Set(Graph)` or `OpenNodes`.
  - When the graph is a straight chain, each block gets:
    - a header row with the block title, a bypass checkbox ("On"), up and down buttons ("Up" and "Down") and "Remove"
    - its number knobs as sliders, and its choices as combos (from the catalog spec)
  - An "Add effect" combo at the bottom lists `effects_catalog_kinds()` by title.
  - Any edit rebuilds the chain with `chain_graph`, keeping node ids is not required.
  - When it isn't a chain, it shows "Custom routing" and an "Open nodes" button. When it is a chain, a small "Open nodes" button still shows under the list.

- [ ] **Step 1: Write failing kittest tests** in `fx_list.rs`:
  - `add_reorder_bypass_remove`:
    1. Start empty.
    2. Pick "Delay" then "Reverb" from Add effect. The chain is [delay, reverb].
    3. Click Up on Reverb. The chain is [reverb, delay].
    4. Untick On for delay. The graph's delay node has `enabled` false.
    5. Remove reverb. The chain is [delay].
  - `custom_routing_offers_open_nodes`: a graph with a forked wire shows "Custom routing", and clicking Open nodes returns `OpenNodes`.
  - `panel_opens_effects`: in the panel test harness, `PanelTarget::Effects(FxPlace::Master)` shows the header "Master effects" and lists the reverb block.
- [ ] **Step 2: Run** `cargo test -p bpe-studio fx_list`. Expected: FAIL.
- [ ] **Step 3: Implement.** Combos inside scroll areas need `scroll_to_me` in tests, and two labels with the same text need `query_all().next()`.
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(studio): effects list view and effects in the node panel".

### Task 10: Effects, channel, buses and master in the inspector

**Files:**
- Modify:
  - `crates/bpe-studio/src/inspector.rs`
  - `crates/bpe-studio/src/instruments.rs`
  - `crates/bpe-studio/src/app.rs` (edits to apply, panel requests)

**Interfaces:**
- `Edit::InstrumentFx(InstrumentId, Graph)`, `Edit::ChannelInserts(ModuleId, Graph)`, `Edit::Send(ModuleId, BusId, Option<f32>)`, `Edit::Buses(Vec<Bus>)` and `Edit::MasterFx(Graph)`. Each one is undoable through the same history path as `Edit::Instrument`.
- `Inspector.fx_request: Option<FxPlace>` opens the node panel.
- Placement:
  - The instrument section gets a collapsing "Instrument effects" with `fx_list::show`.
  - A sound module's inspector gets a "Channel" section after Instrument. It holds Pan (the setting row is already drawn by the settings grid, so don't duplicate it), "Inserts" (the list view), and "Sends" (one slider per bus from -60 to +6 dB; a value at -60 or a missing one shows "off" and stores `None`).
  - Scene defaults gets:
    - a "Buses" section: a row per bus with a name text box, a Volume slider and Remove, its effects list in a collapsing header, and "Add bus" at the end, which adds "Bus N" with an empty chain
    - a "Master effects" section with the list view
  - The Master group settings draw through the existing settings grid.

- [ ] **Step 1: Write failing kittest tests** in `inspector.rs`:
  - `a_send_knob_sends`: on a scene from `bpe_kit::new_scene` with a ball and pluck, select the sound module and drag the "Reverb" send. The `Edit::Send` value is above -60.
  - `add_bus_and_rename`: on Scene defaults, click Add bus. There are 3 buses, and typing a name renames the third.
  - `master_effects_list_adds_a_limiter`: on Scene defaults, add "Limiter" under Master effects. The library master fx chain is ["limiter"].
  - `the_channel_section_fits_the_panel`: the inspector at 640 px wide has no horizontal overflow, using the existing a_patch_section_fits test's method.
- [ ] **Step 2: Run** `cargo test -p bpe-studio inspector`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(studio): channel, buses and master effects in the inspector".

### Task 11: Meters

**Files:**
- Modify: `crates/bpe-studio/src/transport.rs`, `crates/bpe-studio/src/export_window.rs`, `crates/bpe-studio/src/sound_job.rs` (carry the report)

**Interfaces:**
- `transport::meters(ui, report: Option<&MasterReport>, time: f64)` draws two small bars beside the transport:
  - "LUFS": short-term at the playhead on a -40 to 0 scale, with a tick at the target.
  - "GR": gain reduction at the playhead on a 0 to 12 dB scale.

  It shows "--" when there is no report.
- The export window's finished state shows "Loudness -14.0 LUFS · True peak -1.0 dBTP" from the exported track's report. The batch window gets the same per job row if the row has room; otherwise skip it and write a ruling.

- [ ] **Step 1: Write failing tests:**
  - `meters_read_the_playhead` (kittest on a small harness): with a report whose short-term is -20 at index 10, the label at time 1.0 s reads "-20 LUFS".
  - `export_shows_loudness`: the export window state with a finished report shows the text "True peak".
- [ ] **Step 2: Run** `cargo test -p bpe-studio meters export`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio`. Expected: PASS.
- [ ] **Step 5: Commit** "feat(studio): loudness and gain-reduction meters".

### Task 12: Speed, screenshots and deliverables

**Files:**
- Modify: `crates/bpe-mix/src/channels.rs` (the ignored speed test)

- [ ] **Step 1:** Extend the ignored `a_busy_minute_renders_quickly` test so the channel has a Reverb insert and a Delay insert, and the scene has both buses with sends at -12 dB.
  - The full render must be under 4 s, and re-rendering after one macro change under 1 s.
  - Run it with `cargo test --release -p bpe-mix busy -- --ignored --nocapture`.
  - If it is slow, profile and fix the hot block. The per-bus and per-channel stages already run in parallel.
- [ ] **Step 2:** Run the whole workspace `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. Expected: green, apart from the known release-only `pushing_an_earlier_step_panics_in_debug`.
- [ ] **Step 3:** Save screenshots to `/mnt/project-files/ballphysics/music-synth/part3/`:
  - the channel section with inserts and sends
  - the Scene defaults buses and master
  - the meters

  Also save a rendered `pitch-climb.mp4` with a send to Reverb. Use the Xvfb :77, xdotool and import route.
- [ ] **Step 4: Commit** any test or scene changes: "test: busy minute with effects".
