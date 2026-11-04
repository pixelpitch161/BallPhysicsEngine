# Sound system part 5: notes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Notes section becomes a slot grid lined up with the keys, with a per-shape key and scale, every play
order, slot changes by tempo, time, hits or keyframes, and about 40 chord presets.

**Architecture:** Settings on the `sound.voice` card (`notes_*` keys, `Value::Slots`); music theory in a pure
module `bpe-sound/src/music/` (scales, chord naming, voice leading, presets); note picking behind part 2's
`NotePicker` trait; slot changes in `bpe-sound/src/voices/slots.rs` on the shared bar clock; the editor's
`sound_tab/notes/` replaces part 2's `keys.rs` usage but reuses its `keyboard()` drawing.

**Tech Stack:** Rust 2024, egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-04-sound-system-design.md` ("Part 5"). Mockup: canvas board **NotesGrid**
(`project/NotesGrid.dc.html` at https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA). Decisions log:
`docs/superpowers/plans/sound-system/decisions.md`. **Starts after part 2 is merged**; parallel with parts 3 and 6;
merge order 3, **5**, 6 (rebase on main before the PR; keep shared-file edits additive).

## Global Constraints

- Part 1 and 2 constraints hold. FL naming: MIDI 60 = **C5**.
- Key per shape only: Root C..B and the 12 scales: Major, Minor, Dorian, Phrygian, Lydian, Mixolydian,
  Harmonic minor, Melodic minor, Major pentatonic, Minor pentatonic, Blues, Chromatic.
- Keyboard default **C4..B6**, 1..8 octaves. Slots **1..16**.
- Plays: Random, Shuffle, Up, Down, Up-down, Down-up, Converge, Diverge, Random walk, Order clicked, By ball,
  By side, All at once (Strum **0..200 ms**; Up, Down, Random); Repeat each N; Octave range **+0..+3**.
- Slots change: By tempo (N beats or bars on the scene's bar clock), By time (N s), By hits (N hits on this
  shape), Only on keyframes; order Forward, Reverse, Ping-pong, Random, Shuffle. A Slot keyframe (`notes_jump`,
  `Ease::Hold`) jumps whatever the mode.
- Ringing notes finish when the slot changes; an Always drone (part 4) moves to the new notes, gliding when Mono
  and Glide are on. Part 5 exposes `SlotClock::changed()` for that; part 4 wires the drone.
- Chord presets: about **40** progressions in **8 moods** (Pop, Emotional, Epic, Dark, Dreamy, Jazzy, Lo-fi,
  Classic); choosing one fills the slots and sets By tempo every **1 bar**; voicing Triads, Sevenths, Spread;
  Add bass (root an octave below).

## Review Focus

1. **Notes outside the keyboard's range** (range shrunk after notes were set): notes stay and still play; the
   grid shows a small "+2 above" marker. Task 5 kittest.
2. **Changing key with notes in place**: the dialog's "Move notes" maps each note to the nearest in-scale degree
   by scale step, never dropping notes; "Keep notes" leaves them. Task 2 test.
3. **Slot change exactly on a hit's sample**: the hit uses the **new** slot (changes apply before hits within a
   sample). Task 4 test.
4. **Empty slot in the cycle**: a hit in an empty slot plays nothing (no fallback to another slot). Task 3 test.
5. **Two shapes, By tempo every bar, one started mid-clip by a seek**: they still change together on bar lines.
   Task 4 test.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-kit/src/sound/voice.rs` | add `notes_*` settings (root, scale, plays, strum, repeat, octave range, change mode/every/order, `notes_jump`) |
| `crates/bpe-sound/src/music/{mod.rs, scale.rs, chord_name.rs, voicing.rs, presets.rs}` (new) | theory, pure |
| `crates/bpe-sound/src/voices/pick.rs` (new) | every `NotePicker` |
| `crates/bpe-sound/src/voices/slots.rs` (new) | `SlotClock` |
| `crates/bpe-studio/src/sound_tab/notes/{mod.rs, grid.rs, header.rs, presets_menu.rs}` (new) | the UI |

---

### Task 1: Notes settings

**Files:** modify `crates/bpe-kit/src/sound/voice.rs`.

**Interfaces:**
- Produces on `VoiceSettings`: `notes_root: Root { C, Cs, D, Ds, E, F, Fs, G, Gs, A, As, B }` (labels "C", "C♯"…
  — the default font lacks ♯: use "C#" text), `notes_scale: Scale` (the 12 above, default Major),
  `notes_plays: Plays` (default Random), `notes_strum` (0..200 ms), `notes_strum_dir: StrumDir { Up, Down, Random }`,
  `notes_repeat` (1..16, 1), `notes_octave_range` (0..3, 0), `notes_change: Change { ByTempo, ByTime, ByHits,
  OnlyKeyframes }` (default ByTempo), `notes_every_beats` (1..64, 4, unit beats; a `notes_every_unit: BeatsOrBars`
  default Bars with every = 1), `notes_every_seconds` (0.1..60, 2), `notes_every_hits` (1..256, 4),
  `notes_order: SlotOrder { Forward, Reverse, PingPong, Random, Shuffle }`, `notes_jump: i64` (1..16, keyframe,
  always `Ease::Hold` — enforce in `set_key` for this key via a `SettingDef` flag `hold_only` added to the derive
  if missing; record in decisions.md).
- [ ] **Step 1–5:** tests `notes_defaults`, `jump_keys_are_always_hold`; implement; commit
  `git commit -m "Kit: key, scale, play order and slot change settings"`.

### Task 2: Music theory module

**Files:** create `crates/bpe-sound/src/music/*.rs`.

**Interfaces:**
- Produces:
  - `scale.rs`: `pub fn scale_steps(scale) -> &'static [u8]`; `pub fn in_scale(root, scale, midi: u8) -> bool`;
    `pub fn move_into_key(notes: &[u8], from: (Root, Scale), to: (Root, Scale)) -> Vec<u8>` (each note moves by the
    root interval taken the nearest way, −6..+5 semitones, keeping its scale degree; notes outside the old scale
    go to the nearest note of the new scale, ties down).
  - `chord_name.rs`: `pub fn chord_name(notes: &[u8]) -> String` — root-position and inversions for major, minor,
    dim, aug, sus2, sus4, 6, m6, 7, maj7, m7, m7♭5 (written "m7b5"), dim7, add9, 9, m9; slash for inversions ("C/G");
    single note → its name ("C5"); otherwise "" (no name).
  - `voicing.rs`: `pub fn voice_progression(chords: &[ChordSpec], range: RangeInclusive<u8>, style: Voicing,
    add_bass: bool) -> Vec<Vec<u8>>` — first chord root position centred in range; each next picks the inversion
    minimising total semitone movement (ties: lower), Spread opens the 3rd up an octave.
  - `presets.rs`: `pub struct Preset { pub mood: Mood, pub name: &'static str, pub numerals: &'static str, pub
    degrees: &'static [ChordSpec] }`, `pub static PRESETS: [Preset; 40]` (5 per mood; e.g. Pop: I–V–vi–IV,
    vi–IV–I–V, I–vi–IV–V, I–IV–vi–V, IV–I–V–vi; Emotional: vi–IV–I–V, i–VI–III–VII, …; Jazzy: ii–V–I–vi with
    sevenths; Lo-fi: IVmaj7–iii7–vi7–Imaj7…). `pub fn realise(p: &Preset, root, scale, range, style, bass) ->
    Vec<NoteSlot>`.
- [ ] **Step 1: Failing tests:** `major_and_minor_steps`, `move_into_key_keeps_degrees` (C major [60, 64, 67] → G major gives **[55, 59, 62]**: every note moves by the
  root interval taken the nearest way, −6..+5 semitones, then snaps to the new scale), `chord_names_table` (12 cases incl. "Am", "F", "C/G", "G7", "Dm7b5"), `voice_leading_minimises_motion`
  (I–V–vi–IV in C: total motion ≤ 6 semitones per change), `forty_presets_in_eight_moods`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: scales, chord names, voice leading and presets"`

### Task 3: Play orders

**Files:** create `crates/bpe-sound/src/voices/pick.rs`; modify `voices/shape.rs` to build the picker from
`notes_plays`.

**Interfaces:**
- Consumes: part 2 `NotePicker`, `PickCx { hit_index: u64, ball_number: u32, side: u32, slot: &NoteSlot,
  octave_range: u8, rng: &mut Rng }`. Part 5 adds `ball_number` (from `bpe_kit` balls `sim::numbers`, looked up
  through the world: expose `World::ball_number(thing, item) -> u32` if missing) and `side` (`b.item` of the wall
  hit mapped to a side index by the shape sim; expose `bpe_kit::things::shape::side_of(world, shape, point) -> u32`).
- Produces: one struct per order implementing `NotePicker`, each with a deterministic state reset on `seek`:
  Random (independent, seeded), Shuffle (bag without repeats), Up, Down, Up-down and Down-up (no repeated ends),
  Converge (outer → inner: lowest, highest, 2nd lowest…), Diverge (reverse of converge), Random walk (±1 step,
  clamped), Order clicked (`order_clicked`), By ball (`notes[ball_number % len]`), By side (`notes[side % len]`),
  All at once (all notes; `strum` spreads note starts by `strum / (n−1)` in Up/Down/Random order). Repeat each N:
  the picker advances every N hits. Octave range: the sequence runs over notes + 12·k for k in 0..=range.
- [ ] **Step 1: Failing tests** (`tests/pick.rs`): for notes [60, 64, 67] and 12 hits, golden lists per order
  (write them out in the test), `strum_spreads_starts`, `repeat_each_holds_n_hits`, `octave_range_extends_the_run`,
  `empty_slot_plays_nothing`, `deterministic_after_seek`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: every play order"`

### Task 4: Slot changes

**Files:** create `crates/bpe-sound/src/voices/slots.rs`; modify `voices/shape.rs`.

**Interfaces:**
- Consumes: `TempoMap` (bar clock), `notes_jump` resolved per block from its track (via `Resolved` animation
  sampling the engine already does per block — use `bpe_core::animation` `Track::value_at`).
- Produces: `pub struct SlotClock { … }` with `fn advance(&mut self, block_start: u64, block_len, hits_in_block:
  &[u32 /* offsets of accepted hits */], tempo: &TempoMap) -> SmallVec<[(u32 /*offset*/, usize /*slot*/); 4]>` and
  `current() -> usize`, `changed() -> bool` (since last block). By tempo changes on exact sample of the beat/bar
  boundary computed from absolute sample position (so shapes agree, and seeks agree); By time likewise from
  absolute time; By hits counts accepted hits; Only keyframes never moves on its own. Orders: Forward, Reverse,
  Ping-pong (no repeated ends), Random (seeded by shape + change count), Shuffle (bag). A `notes_jump` key value
  change jumps to that slot at the key's sample. Changes apply **before** hits at the same sample.
- [ ] **Step 1: Failing tests:** `by_tempo_changes_on_bar_lines` (120 BPM, 4/4, every 1 bar → changes at samples
  96000·k), `two_shapes_change_together_after_a_seek`, `by_hits_counts_accepted_hits_only`,
  `a_change_and_a_hit_on_one_sample_uses_the_new_slot`, `jump_key_overrides_the_mode`, `ping_pong_order`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Sound: slots change by tempo, time, hits or keyframes"`

### Task 5: Slot grid and Notes header UI

**Files:** create `crates/bpe-studio/src/sound_tab/notes/{mod.rs, grid.rs, header.rs}`; modify
`sound_tab/mod.rs` to call `notes::show` instead of `keys::show`.

**Interfaces:**
- Consumes: part 2 `keys::keyboard(ui, range, lit) -> Option<u8>`, `chord_name`, `in_scale`, `move_into_key`,
  engine's current slot per shape (add `Meters.slots: [AtomicU8; 64]` by voice index).
- Produces: header: Root `menu_button`, Scale `menu_button`, Plays `menu_button` (+ Strum, Repeat each, Octave
  range knobs when relevant), Slots change `menu_button` with its Every field and Order chips, "Chord presets"
  button (Task 6). Grid (board NotesGrid): one row per slot (18 px), cells aligned to the keyboard's key columns,
  lit cells in the shape colour, row number + chord name at left, picked row outlined `TEXT`, playing row outlined
  `TEXT_STRONG` during playback. "+ Slot" under the last row (max 16). Click a cell toggles; clicking a key toggles
  in the picked row and auditions. In-scale keys get a faint mark (`style::LINE` dot). Right-click a row:
  Duplicate, Clear, Delete, Copy notes, Paste notes (clipboard `Clipboard::Notes(Vec<u8>)`, works across shapes).
  Drag a row by its number to reorder. Changing Root/Scale with notes present opens a small modal "Move the notes
  into the new key?" — "Move notes" | "Keep notes". Notes outside the shown range show a "+N above"/"+N below"
  marker at the row end.
- [ ] **Step 1: Failing kittest tests:** `clicking_a_cell_toggles_that_slots_note`, `plus_slot_adds_up_to_16`,
  `row_menu_duplicate_and_delete`, `copy_notes_paste_into_another_shape`, `changing_key_offers_to_move_notes`,
  `chord_name_shows_on_the_row`, `out_of_range_notes_show_a_marker`, `drag_reorders_rows`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: the slot grid and Notes header"`

### Task 6: Chord presets menu and timeline slot marks

**Files:** create `sound_tab/notes/presets_menu.rs`; modify `crates/bpe-studio/src/timeline/mod.rs` (slot marks in
the shape's group header lane).

**Interfaces:**
- Consumes: `PRESETS`, `realise`.
- Produces: a menu grouped by mood, each entry "I–V–vi–IV   C G Am F" (chords in the shape's key), and options at
  the top: Triads | Sevenths | Spread chips and an "Add bass" switch. Choosing fills slots (one per chord),
  sets `notes_change = ByTempo`, every 1 bar. Timeline: small ticks where the slot changes (computed by running
  `SlotClock` over the clip offline on demand, cached by scene revision), and Slot keys (`notes_jump`) shown as
  normal keys whose value label is "Slot N".
- [ ] **Step 1: Failing tests:** `choosing_a_preset_fills_slots_and_sets_by_tempo`, `preset_labels_use_the_shapes_key`,
  `slot_change_ticks_on_the_timeline`.
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** `git commit -m "Studio: chord presets and slot marks on the timeline"`

### Task 7: Finish the part

- [ ] **Step 1:** Rebase on main (after part 3 merges); resolve additive conflicts in `sound_tab/mod.rs`,
  `voices/shape.rs`, `bpe-kit/src/sound/mod.rs`.
- [ ] **Step 2:** fmt, clippy `-D warnings`, tests debug + release, bless sound goldens if note picking changed a
  golden scene (it should not: default Random over one note).
- [ ] **Step 3:** Update `CLAUDE.md`. Windows cross-build.
- [ ] **Step 4:** One Opus review; fix.
- [ ] **Step 5:** PR (attribution block, assign + review `pixelpitch161`); exes to
  `/mnt/project-files/ballphysics/sound-system/part5/`.
