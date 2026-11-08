# Progressions and note choice (music part 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the chords text on Scene defaults with Progression library items (key, meter, chords in beats) that every sound module can share or own, edit them in a chord lane with presets, show them on the timeline, and give the Notes module richer note choice.

**Architecture:** Progression data and the `ProgressionRef` setting type live in bpe-core beside the module library. Music theory (chord tones, scales, numerals, names, presets, the chord clock on the beat grid) lives in bpe-mix `chords.rs` and `presets.rs`. The Notes module resolves its progression through `MixSettings`, which bpe-kit fills from the scene. The editor gets a `chords/` folder with the lane, palette, chord editor and preset list, a progression picker row in the settings grid, and a Chords track on the timeline.

**Tech Stack:** Rust 2024 workspace, egui/eframe + egui_kittest for the studio, serde/RON for saves.

**Spec:** `docs/superpowers/specs/2026-10-30-music-synth-design.md` (section 1, and section 9 part 1). Mockup of the lane: https://claude.ai/artifact/YbxbzQ7UcHw8mv8sk2bjS5

## Global Constraints

- Old scenes need not load or sound the same; no migration of the old `chords` text.
- Chord lengths are whole beats on the scene's beat grid (`bpe_kit::music::beats_for`, 120 bpm when the grid is empty). A chord is placed by beat index, so chords follow a song's detected beats.
- A hit's chord is the chord at the hit's **video time** (`cx.video_time(sim_time)`), never sim time (two-clock rule).
- Roots are absolute pitch classes 0..=11 (0 = C). Numerals, function colours and names are computed, never stored.
- Modes: Major, Minor, Dorian, Phrygian, Lydian, Mixolydian, Locrian. Meter default 4/4.
- Extensions: Triad, Seven, Maj7, Sus2, Sus4, Add9, Nine, Eleven, Thirteen. Qualities: Major, Minor, Diminished, Augmented.
- At least 20 presets ship, each with a name, a mood and a mode.
- Chord length: 1 beat minimum, 256 beats maximum.
- Every inspector copy is plain words; numerals use ♭/♯ and ° as in the mockup.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` stay clean.
- Before Task 1: `git fetch origin main && git merge origin/main` (the camera thread changes bpe-mix; take its side of any conflict and re-apply ours).

## Review Focus

1. **A progression with no chords** (all deleted in the lane): sound modules using it fall back to the built-in starter progression and keep playing; the lane shows an empty state with the palette still usable.
2. **A module linked to a progression that was removed from the library** (by undo history or hand-edited file): it plays the Scene's progression, and the picker shows "Scene's".
3. **Hits after the beat grid ends** (grid shorter than the run, e.g. song shorter than video): the chord clock extends the last beat period, so chords keep cycling instead of sticking on one chord.
4. **Changing the key** with slash chords and borrowed chords in the lane: every root and bass moves by the same interval mod 12, nothing goes out of 0..=11.
5. **Full chord with a spread that runs past note 127 or a range holding no chord tone**: notes above the range are dropped, never wrapped or clamped into wrong pitches; a hit with nothing left is silent.

Each line has a pinned test in the task that owns the code (Tasks 2, 3, 4, 4, 5).

---

### Task 1: Progression data in bpe-core

**Files:**
- Create: `crates/bpe-core/src/progression.rs`
- Modify: `crates/bpe-core/src/lib.rs` (module + re-exports), `crates/bpe-core/src/library.rs` (field), `crates/bpe-core/src/scene.rs` (`next_progression`), `crates/bpe-core/src/value.rs` (`ValueType::Progression`), `crates/bpe-core/src/settings.rs` (`SettingValue for ProgressionRef`), `crates/bpe-core/src/edit.rs` (edits + `EditError::MissingProgression`, `ProgressionInUse`)
- Test: in-file `#[cfg(test)]` modules of `progression.rs` and `edit.rs`

**Interfaces:**
- Produces:
  - `pub struct ProgressionId(pub u32)` (same derives as `ModuleId`, `#[serde(transparent)]`)
  - `pub enum Mode { Major, Minor, Dorian, Phrygian, Lydian, Mixolydian, Locrian }` (`Default = Major`, serde, `Mode::ALL: [Mode; 7]`, `fn label(self) -> &'static str`)
  - `pub struct Key { pub root: u8, pub mode: Mode }`
  - `pub enum Quality { Major, Minor, Diminished, Augmented }`, `pub enum Extension { Triad, Seven, Maj7, Sus2, Sus4, Add9, Nine, Eleven, Thirteen }` (both `Default` = first, `ALL` consts)
  - `pub struct Chord { pub root: u8, pub quality: Quality, pub ext: Extension, pub bass: Option<u8>, pub beats: u16 }`
  - `pub struct Progression { pub name: String, pub key: Key, pub meter: (u8, u8), pub chords: Vec<Chord> }` with `Progression::starter() -> Progression` (key C major, 4/4, name "Starter": C 8, Dsus4 8, Am 8, C 8, Am 4, Dsus4 4 beats; this replaces the old text default) and `fn total_beats(&self) -> u32`, `fn transpose_to(&mut self, root: u8)` (moves every chord root and bass by the same interval mod 12)
  - `Library.progressions: BTreeMap<ProgressionId, Progression>` (`skip_serializing_if = "BTreeMap::is_empty"`), `Scene.next_progression: u32`
  - `pub struct ProgressionRef(pub Option<ProgressionId>)` — `None` = "Scene's". `SettingValue`: `value_type() = ValueType::Progression`, stored as `Value::Int(-1)` for `None`, else `Value::Int(id)`.
  - `ValueType::Progression`
  - `Scene::add_progression(&mut self, p: Progression) -> ProgressionId`, `Scene::set_progression(&mut self, id, p: Progression) -> Result<(), EditError>`, `Scene::copy_progression(&mut self, id) -> Result<ProgressionId, EditError>` (name gets " copy"), `Scene::remove_progression(&mut self, reg: &Registry, id) -> Result<(), EditError>` (refuses with `ProgressionInUse(Vec<Address>)` while any thing setting, slot override or module setting of type Progression points at it), `Scene::progression_users(&self, reg, id) -> Vec<Address>`
  - `Chord::clamp(self) -> Chord` (root, bass mod 12; beats clamped to 1..=256), applied by `set_progression`.

- [ ] **Step 1: Write failing tests** in `progression.rs`:
  - `a_progression_ref_round_trips_through_a_value`: `ProgressionRef(None).to_value() == Value::Int(-1)`; `from_value(&Value::Int(3)) == Some(ProgressionRef(Some(ProgressionId(3))))`; `from_value(&Value::Number(1.0)) == None`.
  - `transposing_moves_roots_and_bass_and_wraps`: starter transposed to root 10 (B♭): first chord root 10, Dsus4 root 0, a chord with `bass: Some(4)` becomes `Some(2)`; all values in 0..=11.
  - `starter_lasts_forty_beats`: `Progression::starter().total_beats() == 40`.
  - `a_progression_saves_and_loads`: RON round trip equal.
- [ ] **Step 2: Write failing tests** in `edit.rs`:
  - `progressions_get_fresh_ids_and_copies`: two `add_progression` give ids 0 and 1; `copy_progression(0)` gives 2 with name "Starter copy".
  - `a_progression_in_use_cannot_be_removed`: with a test-kit module kind that has a `ProgressionRef` setting set to id 0, `remove_progression(reg, 0)` is `Err(ProgressionInUse(_))`; after setting it to `-1`, `Ok(())`.
  - `set_progression_clamps_chords`: a chord with `beats: 0, root: 14` is stored as `beats: 1, root: 2`.
- [ ] **Step 3: Run** `cargo test -p bpe-core progression` — expect compile failures.
- [ ] **Step 4: Implement** the types and edits listed above. `Value::Int` storage keeps `Value` unchanged; `ValueType::Progression` must be handled in every exhaustive `match` on `ValueType` across the workspace (`cargo check --workspace` lists them; render it like `Int` anywhere but the studio, which Task 6 handles). Add `EditError` display strings: "That chord progression isn't in the library any more" and "N settings still use this chord progression".
- [ ] **Step 5: Run** `cargo test -p bpe-core && cargo check --workspace` — PASS.
- [ ] **Step 6: Commit** `feat(core): progression library items and the ProgressionRef setting`.

---

### Task 2: Music theory and presets in bpe-mix

**Files:**
- Rewrite: `crates/bpe-mix/src/chords.rs` (remove `Chords`/`Chords::parse`; keep and extend the name parser)
- Create: `crates/bpe-mix/src/presets.rs`
- Modify: `crates/bpe-mix/src/lib.rs` (exports)
- Test: in-file tests of both

**Interfaces:**
- Consumes: Task 1 types.
- Produces (all `pub`):
  - `fn scale(key: Key) -> [u8; 7]` — pitch classes of the mode starting on the key root. Mode intervals: Major 0 2 4 5 7 9 11, Minor 0 2 3 5 7 8 10, Dorian 0 2 3 5 7 9 10, Phrygian 0 1 3 5 7 8 10, Lydian 0 2 4 6 7 9 11, Mixolydian 0 2 4 5 7 9 10, Locrian 0 1 3 5 6 8 10.
  - `fn diatonic(key: Key, degree: usize, beats: u16) -> Chord` — triad stacked in thirds from the scale (quality from the stacked intervals).
  - `fn borrowed(key: Key) -> Vec<Chord>` — major-ish modes: iv, ♭VI, ♭VII, II; minor-ish modes (third is 3): V (major), IV (major), ♭II (major). 4 beats each.
  - `fn intervals(c: &Chord) -> Vec<u8>` — semitones above root. Triads: maj 0 4 7, min 0 3 7, dim 0 3 6, aug 0 4 8. Sus2 → third replaced by 2, Sus4 → by 5. Seven: +10 (dim: +10 = half-diminished, aug: +10). Maj7: +11. Add9: +14. Nine: +10 +14 (Maj quality keeps 10; matches common "9"). Eleven: +10 +14 +17. Thirteen: +10 +14 +21.
  - `fn bass_note(c: &Chord) -> u8` — `c.bass.unwrap_or(c.root)`.
  - `fn chord_name(c: &Chord, key: Key) -> String` — root spelled with flats when the key signature is flat (keys F, B♭, E♭, A♭, D♭, G♭ major and their relative modes), else sharps; suffixes `""`, `m`, `dim`, `aug`, `7`, `m7`, `m7♭5`, `maj7`, `m(maj7)`, `sus2`, `sus4`, `add9`, `madd9`, `9`, `m9`, `11`, `m11`, `13`, `m13`; slash bass as `/E`.
  - `fn numeral(c: &Chord, key: Key) -> String` — degree from the major-scale table relative to the key root (`I ♭II II ♭III III IV ♯IV V ♭VI VI ♭VII VII`), lowercase for minor and diminished, `°` for diminished, `+` for augmented, `7`/`M7` for Seven/Maj7. In minor-ish modes ♭III, ♭VI, ♭VII are the diatonic ones (mockup behaviour).
  - `enum Function { Home, Away, Pull, Borrowed }`, `fn function(c: &Chord, key: Key) -> Function` — diatonic degree 1/3/6 Home, 2/4 Away, 5/7 Pull, not diatonic (root or quality, ignoring sus) Borrowed.
  - `fn parse_chord(name: &str) -> Option<Chord>` — accepts everything `chord_name` writes plus the old forms (`F#m`, `Bb7`, `Dsus4`, `Amaj7`), and `/bass`; `beats` 4.
  - `enum PicksFrom { ChordTones, ChordAndScale, Scale }` (derive `Choice`, default ChordTones)
  - `fn candidates(c: &Chord, key: Key, from: PicksFrom, low: u8, high: u8) -> Vec<u8>` — MIDI notes in `low..=high` (either order) whose pitch class is in the set, ascending.
  - `fn voicing(c: &Chord, low: u8, high: u8, spread: u8) -> Vec<u8>` — bass note at its lowest octave ≥ `low`, then chord tones stacked upward across `spread` octaves (1..=3); notes above `high` or 127 are dropped.
  - `struct ChordClock` with `ChordClock::new(p: &Progression, beats: &[f64]) -> ChordClock` and `fn at(&self, video_time: f64) -> (usize, &Chord)` — the chord whose beat span covers the time, cycling the progression. Beat times come from `beats`; past its end (or with fewer than 2 beats) beats continue at the last period (0.5 s when unknown). Times before 0 use chord 0. An empty progression uses `Progression::starter()`.
  - `presets.rs`: `struct Preset { pub name: &'static str, pub mood: &'static str, pub mode: Mode, pub degrees: &'static [(u8, Extension)] /* 0-based scale degrees */, pub beats: u16 }`, `const PRESETS: &[Preset]` (≥ 20; include the twelve from the mockup), `fn Preset::chords(&self, root: u8) -> Vec<Chord>`, `fn moods() -> Vec<&'static str>` (unique, in list order).

- [ ] **Step 1: Write failing tests**:
  - `scales_match_the_modes`: `scale(Key{root:9,mode:Minor}) == [9,11,0,2,4,5,7]`; Dorian on D `== [2,4,5,7,9,11,0]`.
  - `diatonic_triads_in_a_minor`: names `["Am","Bdim","C","Dm","Em","F","G"]`, numerals `["i","ii°","♭III","iv","v","♭VI","♭VII"]`.
  - `extensions_and_names`: `Chord{root:9,Minor,Seven}` → "Am7"; `Chord{root:11,Diminished,Seven}` → "Bm7♭5"; `Chord{root:0,Major,Maj7,bass:Some(4)}` → "Cmaj7/E"; in E♭ major root 3 prints "E♭".
  - `parse_reads_what_name_writes`: for every quality × extension × root, `parse_chord(&chord_name(c, C major))` gives the same root, quality and ext (skip combinations whose name collides, e.g. aug with sus); `parse_chord("Bb7")`, `parse_chord("F#m")` work; `parse_chord("H")` is None.
  - `functions_colour_like_the_mockup`: in A minor, Am Home, Dm Away, Em Pull, E (major) Borrowed.
  - `candidates_respect_the_range`: C major triad, ChordTones, 60..=72 → `[60,64,67,72]`; range 61..=63 → empty.
  - `voicing_drops_notes_above_the_range` (Review Focus 5): `voicing(C, 120, 127, 3)` contains only notes ≤ 127 with pitch classes {0,4,7}, and none below 120.
  - `the_clock_follows_beats_and_cycles`: two chords of 2 beats, beats at 0, 0.5, 1.0, 1.5 → `at(0.2).0 == 0`, `at(1.1).0 == 1`, `at(2.1).0 == 0`.
  - `the_clock_extends_past_the_grid` (Review Focus 3): beats `[0.0, 0.5]` only, chords of 1 beat each ×3 → `at(1.2).0 == 2`, `at(1.6).0 == 0`.
  - `an_empty_progression_plays_the_starter` (Review Focus 1): `ChordClock::new(&Progression{chords: vec![], ..starter}, &[])` → `at(0.0).1 == &starter.chords[0]`.
  - `presets_are_plenty_and_valid`: `PRESETS.len() >= 20`; every preset's chords have beats ≥ 1 and every degree < 7.
- [ ] **Step 2: Run** `cargo test -p bpe-mix chords presets` — FAIL (missing items).
- [ ] **Step 3: Implement** the interfaces above. Remove `Chords`; fix the compile errors this causes only inside bpe-mix (callers in bpe-kit are Task 3/4).
- [ ] **Step 4: Run** `cargo test -p bpe-mix` — PASS.
- [ ] **Step 5: Commit** `feat(mix): keys, modes, chord names, numerals, presets and the chord clock`.

---

### Task 3: Progressions reach the mixer; Scene defaults' Chords setting

**Files:**
- Modify: `crates/bpe-mix/src/lib.rs` (`MixSettings`), `crates/bpe-kit/src/things/scene/mod.rs` (`chords` setting), `crates/bpe-kit/src/lib.rs` (`mix_settings`, `new_scene`), `crates/bpe-studio/src/sound_job.rs` (fingerprint + its test at line ~210)
- Test: `crates/bpe-kit/src/lib.rs` tests, `crates/bpe-studio/src/sound_job.rs` tests

**Interfaces:**
- Consumes: Task 1, Task 2 (`ChordClock`).
- Produces:
  - `MixSettings` loses `chords: String`, gains `progressions: BTreeMap<ProgressionId, Progression>`, `scene_chords: Option<ProgressionId>`, `beats: Vec<f64>` (video seconds), `half_height: f32` (half the video height, for Height order).
  - `MixSettings::progression(&self, r: ProgressionRef) -> &Progression` — `Some(id)` present → it; else `scene_chords` present → it; else a `static` starter (use `std::sync::LazyLock`). (Review Focus 2.)
  - `SceneSettings.chords: ProgressionRef` (label "Chords", group "Sound", `simple`, `random`); default `ProgressionRef(None)`.
  - `new_scene` adds `Progression::starter()` to the library and points the root's `chords` at it.
  - `mix_settings` fills `beats` from `music::beats_for(&root)`; when that is empty, a 120 bpm grid over the duration plus one beat (`beat_grid(0.0, None, 0.0, duration)` already does this).

- [ ] **Step 1: Write failing tests**:
  - bpe-kit `a_new_scene_owns_a_starter_progression`: `new_scene(reg)` has one progression named "Starter" and the root's `chords` value is `Value::Int(id)`.
  - bpe-kit `mix_settings_carry_progressions_and_beats`: for `new_scene`, `mix_settings(..).progression(ProgressionRef(None)).name == "Starter"` and `beats.len() >= 40` for the 20 s default.
  - bpe-kit `a_missing_progression_falls_back_to_the_scenes` (Review Focus 2): `MixSettings::progression(ProgressionRef(Some(ProgressionId(99))))` returns the scene's progression.
  - bpe-studio: update the sound_job test that set chords text to instead `set_progression` a one-chord Am progression and assert the fingerprint changes.
- [ ] **Step 2: Run** `cargo test -p bpe-kit mix_settings new_scene && cargo test -p bpe-studio sound_job` — FAIL.
- [ ] **Step 3: Implement.** In `sound_fingerprint`, replace `s.chords` text with `format!("{:?}", scene.library.progressions)` mixed in (all progressions; cheap and safe). Temporarily keep `NoteSound` compiling by building a `ChordClock` from `cx.settings.progression(ProgressionRef(None))` in `sound/mix.rs` (Task 4 replaces this).
- [ ] **Step 4: Run** `cargo test --workspace` — PASS except soundtrack/golden checks that hash audio; if any such test fails only because the chord timing moved to beats, re-bless it as its file header says and note it in the commit message. Event-log goldens (`bpe-kit/tests/golden*.rs`) must not change.
- [ ] **Step 5: Commit** `feat(kit): scenes keep progressions; the mixer reads them on the beat grid`.

---

### Task 4: Note choice in the Notes module

**Files:**
- Modify: `crates/bpe-kit/src/modules/sound/mod.rs` (settings), `crates/bpe-kit/src/modules/sound/mix.rs` (native path, graph host, multi-note output), `crates/bpe-kit/src/modules/sound/graph.rs` only if a template binds `pitch_by` (rename to `order`)
- Test: `crates/bpe-kit/src/modules/sound/mix.rs` tests

**Interfaces:**
- Consumes: Tasks 1–3 (`ProgressionRef`, `candidates`, `voicing`, `ChordClock`, `PicksFrom`, `MixSettings::progression`).
- Produces — new/changed `NoteSound` fields (keep existing ones not listed; `pitch_by: PitchBy` becomes `order: Order`):
  - `chords: ProgressionRef` (label "Chords", `simple`) — default `None`.
  - `picks_from: PicksFrom` (label "Picks from", group "Notes") — default ChordTones.
  - `order: Order` with `enum Order { Random, Speed, Hits, ArpUp, ArpDown, ArpUpDown, Height }` (label "Order", `simple`, `random`) — default Random.
  - `plays: Plays` with `enum Plays { Single, FullChord, Root }` (label "Plays", `simple`) — default Single.
  - `full_chord_speed: f32` (label "Full chord above", unit "px/s", range 0..10000, `when = "plays=full_chord"`) — default 0 (always).
  - `chord_spread: u32` (label "Chord spread", unit "octaves", range 1..3, `when = "plays=full_chord"`) — default 1.
- Behaviour, per hit (native path):
  - Chord = `ChordClock::at(video_time)` of `cx.settings.progression(s.chords)`; key = that progression's key. Build one `ChordClock` per module per mix call.
  - Single: pick from `candidates(chord, key, picks_from, low, high)` by `order`: Random/Speed/Hits as today; Height uses the hit's y (top of frame = highest note; `e.point.y / cx.settings.half_height`, down is positive y); Arp up/down/up-down keep a position per module that advances by one on every note played and wraps (up-down bounces without repeating ends), and resets to 0 when the chord index changes.
  - Root: the lowest candidate whose pitch class is `bass_note(chord)` (silent when none).
  - FullChord: when `full_chord_speed == 0` or the hit speed ≥ it, every note of `voicing(chord, low, high, spread)`, each at velocity × 0.8; otherwise as Single.
  - `Played` gets `keys: Vec<u8>` instead of `key: u8`; `note()` emits one `Note` per key with the same frame. A graph that overrides key output 1 shifts every key of the native set by `graph_key - native_keys[0]`, then drops keys outside 0..=127.
  - `SoundHost::chord_note` uses the same `candidates` list.

- [ ] **Step 1: Write failing tests** (use `MixSettings` and `Event` builders as the existing tests in this file do):
  - `notes_follow_the_chord_on_the_video_clock`: progression Am(4 beats) F(4 beats), beat grid 0.5 s; a hit at sim time 2.1 s with identity warp plays a pitch class in {5,9,0}; with a warp mapping 2.1 → 1.0 it plays one in {9,0,4}.
  - `a_module_can_use_its_own_progression`: two modules, one `chords: None`, one pointing at a C-major-only progression; their notes' pitch classes stay in their own chords.
  - `arp_up_climbs_and_wraps`: C triad, range 60..=67, five hits → keys `[60,64,67,60,64]`.
  - `arp_up_down_bounces_without_repeating_ends`: same range, six hits → `[60,64,67,64,60,64]`.
  - `the_arp_restarts_on_a_new_chord`: C(1 beat) then F(1 beat); hits at beats 0, 0.5, 1.0 → third note is the lowest F-chord candidate.
  - `root_only_plays_the_bass_note`: Cmaj7/E, range 36..=60 → the single key is 40.
  - `full_chord_above_a_speed`: `full_chord_speed: 500`; a 400 px/s hit makes 1 note, a 600 px/s hit makes 3 (C triad, spread 1, range 48..=84) on the same frame.
  - `full_chord_in_a_tiny_range_is_silent_when_nothing_fits` (Review Focus 5): range 61..=62 with C triad → no notes.
  - `height_order_gives_high_notes_at_the_top`: hit at top of frame plays the highest candidate, at the bottom the lowest.
- [ ] **Step 2: Run** `cargo test -p bpe-kit sound` — FAIL.
- [ ] **Step 3: Implement** the settings and behaviour above. Keep `seed`-based Random picks identical in form (`mix64(seed ^ index) % len`).
- [ ] **Step 4: Run** `cargo test -p bpe-kit` — PASS (re-bless audio-hash files only as in Task 3 Step 4).
- [ ] **Step 5: Commit** `feat(sound): each Notes module picks its chords, arps, roots and full chords`.

---

### Task 5: Dice roll progressions and keys

**Files:**
- Modify: `crates/bpe-kit/src/random.rs`
- Test: in-file tests

**Interfaces:**
- Consumes: Task 1 (`ProgressionRef`, `Scene::set_progression`), Task 2 (`PRESETS`).
- Produces: `roll_value` leaves `ValueType::Progression` settings alone (returns `None`). New `fn roll_progressions(scene: &mut Scene, reg: &Registry, scope: &Scope, roll: Roll, rolled: &mut Rolled)` called at the end of `roll`: for each Progression setting the scope's addresses include (scene scope: every progression in the library), unless the address is held (locked) — with chance `amount`, replace that progression's chords with a random preset of the same mode family in its current key (`Rng::new(roll.seed, salt(&address))`); with chance `structural_chance(amount)` also roll the key root (transpose). Push the setting's address into `rolled.changed`.

- [ ] **Step 1: Write failing tests**:
  - `rolling_the_scene_picks_a_preset_in_key`: new scene, roll amount 1 seed 7 → the starter's chords equal some `PRESETS[i].chords(root)` for the progression's (possibly new) root.
  - `a_locked_chords_setting_keeps_its_progression`: lock root `chords` → roll leaves the progression unchanged.
  - `the_same_seed_rolls_the_same_chords`: two rolls of clones with one seed are equal.
- [ ] **Step 2: Run** `cargo test -p bpe-kit random` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-kit random` — PASS.
- [ ] **Step 5: Commit** `feat(dice): roll chord progressions from the presets`.

---

### Task 6: Progression picker row in the inspector

**Files:**
- Modify: `crates/bpe-studio/src/widgets.rs` (`Row::Progression`, `GridOutcome.progression: Option<ProgressionPick>`), `crates/bpe-studio/src/inspector.rs` (apply picks; show the lane under the card, Task 7)
- Test: `crates/bpe-studio/src/inspector.rs` tests

**Interfaces:**
- Consumes: Task 1 edits.
- Produces:
  - `pub enum ProgressionPick { Scenes, Use(ProgressionId), New, MakeUnique }`
  - `Row::Progression(&SettingDef)` emitted by `rows_for` for `ValueType::Progression`. Its control is a combo box listing "Scene's" (not on Scene defaults itself), every library progression by name ("· used by N" suffix when N > 1), then "New progression" and "Make unique". The `GridCx` gets `progressions: &'a [(ProgressionId, String, usize)]` (id, name, users).
  - Inspector applies a pick: `Scenes` → set the setting to `Value::Int(-1)`; `Use(id)` → `Value::Int(id)`; `New` → `add_progression(Progression::starter())` then use it; `MakeUnique` → `copy_progression(current resolved id)` then use it. All through the normal `set_at` write path so overrides on shared modules work.
- [ ] **Step 1: Write failing kittest tests**:
  - `a_chords_setting_has_a_progression_picker`: selecting the root shows a combo labelled "Chords" whose text is "Starter".
  - `make_unique_gives_the_module_its_own_progression`: on the ball's sound card (linking a new sound module), pick "Make unique" → the library has 2 progressions and the module's `chords` points at the new one.
  - `scenes_is_offered_on_modules_not_on_scene_defaults`.
- [ ] **Step 2: Run** `cargo test -p bpe-studio progression` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio` — PASS.
- [ ] **Step 5: Commit** `feat(studio): pick, share or make unique a chord progression`.

---

### Task 7: The chord lane, palette, chord editor and presets

**Files:**
- Create: `crates/bpe-studio/src/chords/mod.rs` (`show`, state), `crates/bpe-studio/src/chords/lane.rs`, `crates/bpe-studio/src/chords/editor.rs`, `crates/bpe-studio/src/chords/presets.rs`
- Modify: `crates/bpe-studio/src/main.rs` (mod), `crates/bpe-studio/src/inspector.rs` (call `chords::show` under any card whose kind has a Progression setting, and under Scene defaults), `crates/bpe-studio/src/theme.rs` (function colours: `HOME #5fc8a8`, `AWAY #f2c46d`, `PULL #ff8a7a`, `BORROWED #8fb4ff`)
- Test: in-file kittest tests in `chords/mod.rs`

**Interfaces:**
- Consumes: Task 2 theory + presets, Task 1 `Scene::set_progression`.
- Produces:
  - `pub struct ChordUi { pub selected: Option<usize>, pub presets_open: bool, pub mood: Option<&'static str> }` kept per progression id in `Inspector.chord_ui: BTreeMap<ProgressionId, ChordUi>`.
  - `pub fn show(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi) -> ChordOutcome` with `pub struct ChordOutcome { pub changed: bool, pub audition: Option<Audition> }` and `pub enum Audition { Chord(Chord), All }`. The inspector writes a changed progression back with `set_progression` (one undo step per drag, as sliders do).
  - Layout (mockup A): key bar (root combo spelled per key, mode combo, meter combo 3/4 4/4 6/8, Play button labelled "▶ Play", "Presets" button); lane with a bar ruler (labels 1, 2, …), blocks filled with the function colour showing name and "numeral · length", width = beats × lane width / max(16, total rounded up to a bar + 1 bar), horizontal `ScrollArea` when wider than the panel; right-edge drag resizes in beats (Shift: whole bars); block drag reorders; Delete removes the selected block (only when the lane has focus, so it never deletes a thing); palette row of 7 diatonic buttons (numeral over name, top border in the function colour) that add after the selected chord, and a "Borrowed" row from `borrowed(key)`; chord editor for the selected block: Quality, Colour (extension), Bass note (root, each chord tone, "Other…" combo of 12), Length (1 beat, 2 beats, 1 bar, 2 bars, and a drag value in beats), a name text field that applies `parse_chord` on Enter and shows "Not a chord name I know" in the error colour when it fails.
  - Empty progression: lane shows "No chords yet. Add one from the palette below." (Review Focus 1).
  - Presets (mockup B) open as a popup under the Presets button: mood filter chips ("All" + `moods()`), list rows of name, numerals (computed in the progression's root and the preset's mode), "mood · mode"; clicking one replaces the chords (and the mode) and closes the popup.
- [ ] **Step 1: Write failing kittest tests** (harness renders `show` on a starter progression):
  - `clicking_a_palette_chord_adds_it_after_the_selection`.
  - `dragging_a_block_edge_changes_its_length_in_beats`.
  - `delete_removes_the_selected_chord_only_when_the_lane_has_focus`.
  - `changing_the_key_transposes_every_chord` (Review Focus 4): with a slash chord in the lane, key C → D moves roots and bass by 2.
  - `a_preset_replaces_the_chords_in_the_current_key`.
  - `typing_a_chord_name_sets_the_chord`; `a_bad_name_shows_a_message_and_changes_nothing`.
  - `an_empty_progression_shows_the_empty_state`.
- [ ] **Step 2: Run** `cargo test -p bpe-studio chords` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio` — PASS.
- [ ] **Step 5: Commit** `feat(studio): chord lane with palette, chord editor and presets`.

---

### Task 8: Audition through the module's instrument

**Files:**
- Modify: `crates/bpe-mix/src/lib.rs` (or new `crates/bpe-mix/src/audition.rs`), `crates/bpe-studio/src/audio_out.rs`, `crates/bpe-studio/src/inspector.rs` / `app.rs` (route `ChordOutcome.audition`)
- Test: bpe-mix unit test; audio_out has no device in CI, so test only the clip mixing helper.

**Interfaces:**
- Consumes: Task 2 (`voicing`, `intervals`), `Instrument::render(key, velocity, decay, brightness)` from `bpe-mix/src/instruments/mod.rs:74`.
- Produces:
  - `pub fn audition(chords: &[Chord], instrument: Instrument, seconds_per_beat: f64, decay: f32, brightness: f32) -> Vec<f32>` — interleaved stereo at `SAMPLE_RATE`; each chord voiced with `voicing(c, 48, 72, 1)` plus its bass an octave below, velocity 0.7, placed at its beat start; a single-chord audition is one chord of 4 beats.
  - `AudioOut::play_clip(&self, clip: Arc<Vec<f32>>)` — mixes a one-shot clip over the output from its start, independent of the track and of `playing`; a new clip replaces the old one. `fn mix_clip(out: &mut [f32], clip: &[f32], pos: &mut usize)` is the testable helper.
  - The inspector uses the card's module instrument (Scene defaults: the root's Sound module) and seconds per beat from the scene's tempo (120 bpm when 0).
- [ ] **Step 1: Write failing tests**: `audition_places_chords_on_beats` (two 1-beat chords at 0.5 s per beat: energy starts within 1 ms of 0 and of 0.5 s); `mix_clip_adds_then_stops_at_the_end`.
- [ ] **Step 2: Run** `cargo test -p bpe-mix audition && cargo test -p bpe-studio mix_clip` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same — PASS.
- [ ] **Step 5: Commit** `feat: hear a chord or the whole progression from the lane`.

---

### Task 9: Chords track on the timeline

**Files:**
- Create: `crates/bpe-studio/src/timeline/chords.rs`
- Modify: `crates/bpe-studio/src/timeline/mod.rs` (draw it first, above the lanes)
- Test: in-file kittest test

**Interfaces:**
- Consumes: Task 2 `ChordClock`, `chord_name`, `function`; Task 3 beats (`bpe_kit::music::beats_for` on root settings).
- Produces: `pub fn chord_spans(p: &Progression, beats: &[f64], duration: f64) -> Vec<(f64, f64, Chord)>` (video start, end, chord) covering 0..duration; `pub fn show(ui, ruler: &Ruler, spans: &[(f64, f64, Chord)], key: Key)` drawing blocks in function colours with names (name hidden when narrower than its text). The progression shown is the selected thing's Sound module's resolved `chords` (or Scene defaults'). Read-only.
- [ ] **Step 1: Write failing tests**: `spans_cover_the_video_and_cycle` (starter at 120 bpm over 25 s: first span (0, 4.0, C), spans contiguous, last end == 25.0); kittest `the_timeline_shows_a_chords_track_with_names` (label "C" visible).
- [ ] **Step 2: Run** `cargo test -p bpe-studio timeline` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(timeline): a Chords track`.

---

### Task 10: Scenes, presets and the full check

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs`, `crates/bpe-kit/src/scenes_stage5.rs`, `crates/bpe-studio/src/starters.rs`, `crates/bpe-studio/src/presets.rs` — any scene that set the old chords text or relies on it gets a progression (use `PRESETS` by name where a scene wants a mood; e.g. the 128 bpm hexagon gets "Epic minor" in A).
- Modify: `README.md` only if it documents the chords text.

- [ ] **Step 1:** `grep -rn "chords" crates --include=*.rs` and convert every remaining text use; nothing may still call a `chords` text setting.
- [ ] **Step 2: Run** `cargo test --workspace` and `cargo test --workspace --release` — PASS; event-log goldens unchanged; re-bless only audio hashes, listing them in the commit.
- [ ] **Step 3: Run** `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check` — clean.
- [ ] **Step 4:** Build the Windows editor (`cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio`), take screenshots of the lane, presets popup and timeline track under xvfb, render one starter scene to mp4 with `bpe-render` to check the sound by ear later; put them in `/mnt/project-files/ballphysics/music-synth/part1/`.
- [ ] **Step 5: Commit** `chore: scenes use progressions; part 1 complete`, push, open the PR.
