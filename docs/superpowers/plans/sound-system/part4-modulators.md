# Sound system part 4: modulators — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every modulator kind works (ADSR, Distance, LFO, Hit; up to 8 of each), any of them reaches any knob by
dragging or right-click, rings show ranges, Plays adds drones and in-range playing, and any setting of any thing
can Follow sound.

**Architecture:** Layer schemas `mod.distance`, `mod.lfo`, `mod.hit` in `bpe-kit` beside part 2's `mod.adsr`.
Audio-rate evaluation in `bpe-sound/src/mods/` feeding part 2's `ModMatrix` (sources become all modulator
layers). A control-rate `ControlSampler` in `bpe-sound` computes the same modulator values from physics alone for
Follow sound; `bpe-core`'s pulse `Inputs` gains a `&dyn SoundLevels` so `bpe-gfx`'s frame fill can read them.
The editor's modulator row, editors, drag-to-modulate and rings live in `bpe-studio/src/sound_tab/mods/`.

**Tech Stack:** Rust 2024, egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` ("Part 4", "Follow sound"). Mockup: canvas
board **Mods** (`project/Mods.dc.html` at https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA). Decisions log:
`docs/superpowers/plans/sound-system/decisions.md`. **Starts after part 3 is merged.** Task 8 (drone follows
slots, Follow a bus's level) needs parts 5 and 6 on main; if either is not merged yet when you reach Task 8, do
the rest, open the PR, and finish Task 8 in a follow-up commit once they land (note it in decisions.md).

## Global Constraints

- Part 1–3 constraints hold.
- Tiles: ADSR 1, ADSR 2, Distance 1, Distance 2, LFO 1, LFO 2, then **+** (ADSR, Distance, LFO or Hit; up to **8
  of each**). Only ADSR 1 on in a new card.
- Combining: routes **add** on most knobs; on **Gain and Level** (and Output Volume) each route **scales**.
- A loop between modulators runs **one block (1.3 ms) late**.
- Per note: ADSR and Hit always; Distance only when Follows = Each note's own ball; LFO when Start = Per note.
- Distance: Centre or Edge; Near, Far (px), Curve, Smooth, Invert; output 1 at Near or closer, 0 at Far or
  beyond. Follows Nearest (default), Each note's own ball, Average, How many in range (Full at N). Turning one on
  sets **Far 200 px** and routes it to **Osc A Gain and Osc B Gain, −inf..0 dB**.
- LFO shapes Sine, Triangle, Saw, Square, Random (S&H), Smooth random; Rate Hz or Tempo **1/32..8 bars**, dotted,
  triplet; Rate, Phase, Fade in, Delay, Smooth; Start Restart on hit / Free / Per note.
- Hit picks Strength, Place on edge (0..1 clockwise from the first corner), Side, Ball number, Random; held per
  note; Low, High, Curve, Smooth.
- Plays: On hit (default), Always (held drone), While a ball is in range of a chosen Distance.
- Follow sound: any setting of any thing, sources a shape's modulators or its Level (and, with part 6, a bus's
  level). Physics-moving settings read the previous step's value. A per-note modulator's Follow value is its
  **largest** value among the shape's sounding notes (decision; log it).

## Review Focus

1. **Deleting a modulator that drives knobs and Follow-sound settings elsewhere**: routes and pulses go with it
   in one undo step; undo brings all back. Task 1 test.
2. **Distance with Follows = Each note's own ball when that ball is removed (popped)**: the voice keeps the last
   value and releases normally. Task 3 test.
3. **Drone (Always) with no notes in the slot**: silence, no stuck voice; adding a note starts it. Task 6 test.
4. **Dragging a modulator handle onto its own knob, or onto a non-numeric setting**: refused with a hover note.
   Task 5 kittest.
5. **Follow sound on a physics setting (ball size) in an export without sound enabled anywhere else**: export and
   preview move identically (deterministic, physics-only). Task 7 test.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-kit/src/sound/mods.rs` (new) | `mod.distance`, `mod.lfo`, `mod.hit` schemas; starter adds them |
| `crates/bpe-dsp/src/lfo.rs` (new) | LFO shapes, closed-form value at a phase |
| `crates/bpe-sound/src/mods/{mod.rs, distance.rs, lfo.rs, hit.rs, order.rs}` (new) | audio-rate modulators |
| `crates/bpe-sound/src/control.rs` (new) | `ControlSampler`: modulator values from physics alone |
| `crates/bpe-core/src/animation.rs` | `Inputs.sound`, `SoundLevels` trait |
| `crates/bpe-gfx/src/frame.rs` | pass `Inputs.sound` through |
| `crates/bpe-studio/src/sound_tab/mods/{row.rs, distance.rs, lfo.rs, hit.rs, routes.rs, drag.rs}` (new) | UI |
| `crates/bpe-studio/src/preview/zones.rs` (new) | dotted Distance zones, draggable |
| `crates/bpe-studio/src/widgets.rs`, `style/knob.rs` | Modulate by / Follow sound menu items, rings |

---

### Task 1: Modulator schemas and starter

**Files:** create `crates/bpe-kit/src/sound/mods.rs`; modify `sound/voice.rs` (`starter_voice` adds Distance 1,
Distance 2, LFO 1, LFO 2 disabled after the ADSRs), `crates/bpe-core/src/edit.rs` (removing a layer also removes
pulses whose source is `PulseSource::Sound { source: Modulator(that layer) }` anywhere in the scene).

**Interfaces:**
- Produces: `DistanceSettings` (`measure: Measure { Centre, Edge }`, `near` 0..4000 px (0), `far` (200),
  `curve` −100..100, `smooth` 0..500 ms (20), `invert`, `follows: Follows { Nearest, EachNotesBall, Average,
  HowMany }`, `full_at` 1..64 (4), `heard_from: Target`); `LfoSettings` (`shape: LfoShape`, `sync: bool`,
  `rate_hz` 0.01..40 (2), `rate_note: NoteLength` (1/32..8 bars + dotted/triplet; default 1/4), `phase` 0..360,
  `fade_in` 0..10 s, `delay` 0..10 s, `smooth` 0..100 %, `start: LfoStart { Restart, Free, PerNote }` (Free));
  `HitSettings` (`pick: HitPick { Strength, Place, Side, BallNumber, Random }`, `low` 0..1 (0), `high` (1),
  `curve`, `smooth` 0..500 ms). `pub const MAX_OF_EACH: usize = 8;`
  `pub fn turn_on_distance(card: &mut Slot, layer: LayerId)` — sets `far = 200`, adds routes to `a_gain` and
  `b_gain` with amount 1.0 (scale → −inf..0 dB).
- [ ] **Step 1: Failing tests:** `starter_has_six_tiles_only_adsr1_on`, `turning_on_distance_routes_both_gains`,
  `at_most_eight_of_each`, `deleting_a_modulator_removes_its_routes_and_follow_pulses_in_one_edit` (undo restores).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Kit: Distance, LFO and Hit modulators"`

### Task 2: LFO and Hit at audio rate

**Files:** create `crates/bpe-dsp/src/lfo.rs`, `crates/bpe-sound/src/mods/{mod.rs, lfo.rs, hit.rs}`.

**Interfaces:**
- Produces: `bpe_dsp::lfo::shape_at(shape, phase: f64, rng_step: u64) -> f32` (0..1 unipolar; Random holds per
  cycle from a counter RNG; Smooth random cosine-interpolates between them). `pub trait Modulator { fn value(&self)
  -> f32; fn next_block(&mut self, cx: &ModCx) ; fn note_on(&mut self, cx: &NoteCx); fn reset(&mut self); }`
  in `mods/mod.rs`, with `ModCx { start_sample, tempo, world, shape: ThingId }`. `LfoMod` (Free: phase from
  absolute sample position so it matches after seeks; Restart: from the last accepted hit on the shape; PerNote:
  one per voice from its note-on), delay then linear fade-in, smoothing one-pole. `HitMod` (per note: value fixed
  at note-on from the hit: Strength = `min(speed/STRONG_HIT,1)`, Place = arc-length fraction of the contact point
  along the outline from corner 0 clockwise (use `bpe_kit::things::shape::place_on_edge(world, shape, point) ->
  f32`, add it), Side = side index / (sides − 1), BallNumber = number / (group size − 1), Random = seeded;
  then mapped `low..high` → 0..1 with curve, smoothed).
- [ ] **Step 1: Failing tests:** `lfo_tempo_quarter_at_120_is_2hz`, `free_lfo_phase_matches_after_seek`,
  `random_lfo_is_deterministic`, `hit_place_is_quarter_at_the_second_corner_of_a_square`, `hit_low_high_maps_and_clamps`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: LFO and Hit modulators"`

### Task 3: Distance at audio rate

**Files:** create `crates/bpe-sound/src/mods/distance.rs`.

**Interfaces:**
- Produces: `DistanceMod` reading ball positions from `ModCx.world` once per physics step and linearly
  interpolating to audio rate (positions at the step before and after the block). Centre: `|p − shape centre|`;
  Edge: unsigned distance to the shape's outline (use `bpe_runtime::walls` nearest-point functions on the shape's
  current outline). Output `1 − smoothstep(near, far, d)` bent by curve, inverted if set, smoothed. Follows:
  Nearest = min distance over balls accepted by `heard_from`; EachNotesBall = the note's own ball (per-note
  instance; if that ball is gone, hold the last value); Average = mean distance; HowMany = count in range (d ≤
  far) / `full_at`, clamped. Allocation-free: ball lists come from the world's slices.
- [ ] **Step 1: Failing tests:** scripted ball path in a test scene: `distance_is_one_inside_near_zero_past_far`,
  `edge_measures_to_the_outline_inside_and_out`, `how_many_counts_balls_in_range`, `each_notes_ball_holds_after_the_ball_pops`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: Distance modulator"`

### Task 4: All modulators into the matrix, mod-of-mod order

**Files:** create `crates/bpe-sound/src/mods/order.rs`; modify `voices/shape.rs`, `modmatrix.rs`.

**Interfaces:**
- Consumes: part 2 `ModMatrix`, part 3 layer knobs.
- Produces: `pub fn mod_order(routes: &[ModRoute], mods: &[LayerId]) -> (Vec<LayerId>, Vec<(LayerId, LayerId)>
  /* late edges */)` — topological order of modulators whose knobs are modulated by other modulators; edges that
  close a loop read the source's **previous block** value. Shared modulators (Free LFO, shared Distance) are
  evaluated once per block per shape; per-note ones per voice. `ModMatrix` sources now cover every modulator
  layer; targets cover osc knobs, chain layer knobs (`"<id>/<key>"`), other modulators' knobs, and `out_*`.
- [ ] **Step 1: Failing tests:** `lfo2_on_lfo1_rate_changes_its_speed`, `a_mod_loop_runs_one_block_late` (output
  equals a hand-run with one-block delay), `per_note_adsr_on_filter_cutoff_is_independent_per_voice`,
  `adsr_times_distance_on_gain_multiplies`, sound golden `modulators_patch_golden`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: every modulator reaches every knob"`

### Task 5: Modulator row, editors, drag to modulate, rings

**Files:** create `crates/bpe-studio/src/sound_tab/mods/{row.rs, distance.rs, lfo.rs, hit.rs, routes.rs, drag.rs}`;
replace part 2's `mod_row.rs` (keep `adsr_view.rs`, add its full Modulates list); modify `style/knob.rs` (rings
from routes, ring end drag, Alt-drag bipolar), `widgets.rs` (knob/row menu item "Modulate by ▸" listing this
shape's modulators).

**Interfaces:**
- Produces: board Mods layout. Tiles show name, target count and the four-arrow handle; `+` menu (ADSR, Distance,
  LFO, Hit; disabled at 8); right-click Rename, Duplicate, Delete. Hover a tile → its rings highlight on every
  knob it reaches. Dragging a handle shows a drag ghost; dropping on a knob adds a `ModRoute` (amount 0.5);
  dropping on the modulator's own knobs or a non-number shows hover "Can't modulate this". Routes list under each
  editor: target name ("Osc A · Cutoff"), range in real units ("1.8 kHz → 6.2 kHz"), Amount −100..+100 % knob,
  Inv chip, remove ×. Distance editor: Centre | Edge chips, Near, Far, Curve, Smooth knobs, Invert, Follows
  menu (+ Full at), Heard from, a live "Now" meter bar. LFO editor: shape chips with a drawn preview, Hz | Tempo
  switch, Rate (Hz knob or note menu), Phase, Fade in, Delay, Smooth, Start chips. Hit editor: pick chips, Low,
  High, Curve, Smooth, recent-hits bar view (last 16 accepted hits, from `Meters.recent_hits` ring per shape).
- [ ] **Step 1: Failing kittest tests:** `dropping_a_handle_on_cutoff_adds_a_route`, `dropping_on_its_own_knob_is_refused`,
  `modulate_by_menu_adds_a_route`, `dragging_a_ring_end_sets_the_amount`, `alt_drag_makes_it_bipolar`,
  `plus_adds_a_hit_tile_and_stops_at_eight`, `hovering_a_tile_highlights_its_rings`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: modulator row, editors and drag to modulate"`

### Task 6: Plays (drone, in range) and Distance zones in the preview

**Files:** modify `crates/bpe-kit/src/sound/voice.rs` (`play_plays: Plays { OnHit, Always, InRange }`,
`play_zone: Text` = a Distance layer id), `crates/bpe-sound/src/voices/shape.rs`; create
`crates/bpe-studio/src/preview/zones.rs`.

**Interfaces:**
- Produces: Always — one held voice per note in the current slot, started at time 0 (and after seeks, at the
  seek time with pre-roll), gate never closes; In range — notes held while the chosen Distance's value > 0
  (Nearest), released when it drops to 0; hits still drive ADSR retriggers and Hit modulators. Zones:
  while the Sound tab is open on a shape, each enabled Distance draws dotted Near and Far outlines (circles for
  Centre, offset outlines for Edge) in the shape's colour; dragging a line sets `near`/`far` through the normal
  edit path (own undo step per drag).
- [ ] **Step 1: Failing tests:** `always_drones_from_the_start`, `always_with_empty_slot_is_silent_until_a_note`,
  `in_range_holds_while_a_ball_is_close`, kittest `dragging_the_far_line_sets_far`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: drones and in-range playing; zones in the preview"`

### Task 7: Follow sound

**Files:** create `crates/bpe-sound/src/control.rs`; modify `crates/bpe-core/src/animation.rs`,
`crates/bpe-gfx/src/frame.rs`, `crates/bpe-runtime/src/world.rs` (physics pulses), `crates/bpe-cli/src/lib.rs`
and `crates/bpe-studio/src/preview.rs` (pass the sampler), `crates/bpe-studio/src/widgets.rs` (menu item),
`timeline/mod.rs` (Follow lanes like pulse lanes).

**Interfaces:**
- Produces:
  - `bpe_core::SoundLevels` trait: `fn level(&self, shape: ThingId, source: &SoundSource, time: f64) -> f32`;
    `Inputs { hits: f32, sound: Option<&'a dyn SoundLevels> }` (gains a lifetime; update callers);
    `pulse_strength` handles `PulseSource::Sound` via `inputs.sound`, 0 when absent.
  - `bpe_sound::ControlSampler::new(scene, resolved, kit)` implementing `SoundLevels` from the physics log and
    world alone (accepted hits by the same `HitFilter`, ADSR closed-form value at `time − note_on`, LFO
    `shape_at`, Distance from positions at that step, Hit held values; per-note modulators report the max over
    sounding notes; Level = the shape's envelope × gain product without audio, i.e. the max over notes of the
    product of its scaling routes and knob gains, in 0..1). Deterministic and audio-free.
  - World physics pulses: a physics setting following sound uses `ControlSampler` at the previous step's time.
  - Right-click any setting row → "Follow sound ▸ <shape> ▸ <modulator | Level>" creates a `Pulse` with
    `amount` = 25 % of the setting's span; the slider shows the range band in the shape's colour; the modulator's
    Routes list shows it as "<thing name> · <setting>".
- [ ] **Step 1: Failing tests:** `a_glow_following_adsr1_pulses_on_hits` (frame values at hit+10 ms > at hit−10 ms),
  `export_and_preview_follow_identically` (two `ControlSampler`s, same values at 100 sampled times),
  `physics_follow_uses_the_previous_step` (ball size following Distance: fingerprint deterministic; the value used
  at step n equals the sampler at step n−1), kittest `follow_sound_menu_adds_a_band`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Follow sound: any setting can move with a shape's sound"`

### Task 8: Integrations with parts 5 and 6, then finish

- [ ] **Step 1:** With part 5 on main: Always drones move to the new slot's notes when `SlotClock::changed()`,
  gliding when Mono + Glide; test `drone_follows_slot_changes`.
- [ ] **Step 2:** With part 6 on main: `SoundSource::Bus(n)` in `ControlSampler` (bus level = RMS-free estimate:
  the sum of its shapes' Levels scaled by fader, clamped 0..1; log the approximation in decisions.md); menu lists
  "Bus 1..3"; test `following_a_bus_level`.
- [ ] **Step 3:** fmt, clippy `-D warnings`, tests debug + release, bless sound goldens.
- [ ] **Step 4:** Update `CLAUDE.md` (modulators, ControlSampler, Follow sound). Windows cross-build. One Opus review;
  fix.
- [ ] **Step 5:** PR (attribution block, assign + review `pixelpitch161`); exes to
  `/mnt/project-files/ballphysics/sound-system/part4/`.
