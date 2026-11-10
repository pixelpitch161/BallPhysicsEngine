# Built-in scenes part 2: starters 9 to 14, Gap in a shape, Merge on touch, Melody list

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three small engine pieces every scene can use (a shape with gaps balls get out through, a rule action that
merges two touching balls into one, a melody each hit steps through), plus Pitch follows size for a ball's sound, and
the last six starters in the Scenes window.

**Architecture:** Gaps are part of the runtime's `Wall` (`Wall.gaps: Option<Arc<Gaps>>`), so every reader of walls
that goes through `keep_clear_from` / `on_own_side` / `middle_crossing` (balls, particles' specks, rule placement)
sees them; the Shape sim builds them from three new body settings and the drawing cuts the outline in the path
shaders through a new instance field. Merge is one more `Action` applied at the next step's start, made by a new kit
Do block `do.merge`. The melody is a second note list on the Sound card (`notes_melody`, the same `Vec<NoteSlot>`
value the slots use, one entry a step) read by a new `Plays::Melody`; the sound engine counts the melody's hits from
the start of the clip. The scenes are code in `bpe-kit/src/library/starters/`, as in part 1.

**Tech Stack:** Rust (pinned toolchain), wgpu/WGSL, egui 0.37 / kittest, the workspace crates.

**Spec:** `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md` ("Three small new pieces" and the starters
table). Part 1's plan and its As built: `part1.md`. Handover: `/mnt/project-files/ballphysics/builtin-scenes/part1-handover.md`.

## Global constraints

- **Goldens unchanged.** Every new setting's default is today's behaviour: Gaps 0 builds `Wall.gaps = None` and the
  wall code takes exactly today's path; no scene without `do.merge` queues a Merge; Plays is never Melody unless
  picked; Pitch follows size 0 adds nothing. `BLESS_GOLDEN` must not be needed. Check with
  `cargo test -p bpe-kit --test golden` (and the sound goldens) before the PR.
- Gaps only exist on a **hollow** shape (the settings' `when` is `fill=hollow`; a filled shape ignores them).
- Scene rules from part 1 hold: scenes start from `new_studio_scene`, use `Lib`, one simple sound per starter,
  every thing named, no unused cards, no files from disk.
- No editor file outside `style/` makes up a colour. kittest names through `labelled_by`.
- `cargo fmt --all` (width 120), `cargo clippy --workspace --all-targets -- -D warnings`, tests with
  `--test-threads=2`. Run only what changed while working; the whole suite once before the PR, in the background,
  with `CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only CARGO_INCREMENTAL=0`.
- The exe is not built or installed.

## Decisions made without Aedan (he asked for the parts to run to completion; also in the As built and handover)

1. **Where a gap is measured.** "Gap at" and "Gap size" use the measure the shape's Place and the "Along the edge"
   gradient already use: the way round from the first corner, clockwise, as a share of the whole length; for an
   ellipse, the angle it is drawn with from its own top (as Place and the shaders measure it). So a gap sits exactly
   where a hit's Place says.
2. **Gap ends are round caps** of the band's half thickness, and a ball can bounce off one.
3. **Gap size is capped** so some wall is always left: with N gaps each is at most `0.9 / N` of the way round.
4. **A ball's side across a gap.** A wall without gaps keeps today's rule (its side is the side its centre was on
   at the start of the step). With gaps, a ball whose centre crossed the outline this step where the outline is open
   now belongs on the side it went to; anywhere else it is put back, as today.
5. **What ignores gaps** (listed in the notes): lights and shadows (the probe and the light passes see the whole
   outline), ripples, mirror outlines, a shattered shape's pieces, the emitter's aim planner (it plans against
   walls through its own maths), particles' Huge swarm segments. These are drawing or planning details; the physics
   of balls and specks and the shape's own drawing follow the gaps.
6. **Merge mass** is area (radius²), as the ball-on-ball collisions use. The merged ball keeps That's look, number,
   tags, health and own settings; the Other ball's are forgotten. "Grow: Keep size" multiplies That's radius by
   Amount (1 keeps it).
7. **Merging across things**: That and Other can be in different things (two Balls groups); the merged ball stays in
   That's thing.
8. **Melody steps** count every accepted hit on that sound from the start of the clip (they never start again when
   a slot changes; slots are not read while Plays is Melody). Repeat each holds a step for that many hits. Octave
   range is not used by a melody. A step may hold up to 16 notes (a chord), the same as a slot.
9. **Pitch follows size** (Sound card, Playing group): semitones per doubling of the ball's size, positive lowers the
   pitch of bigger balls, 0 (default) off; measured against **Size for the written note** (px, default 20). It reads
   the ball that played the note (a Balls or Emitter sound's own ball, or the ball that hit a shape). The pitch moves
   smoothly (not snapped to the key). Not keyframed (read when the patch is built).
10. **Melody editor**: under the keyboard while Plays is Melody: a row of step chips (note names, a chord as "C5+E5"),
    the picked step shown and toggled on the keyboard, and Add step / Remove step / Clear buttons. A piano-roll strip
    shows the steps as bars. **Import MIDI** gains a second button **Import MIDI as a melody** (kittest can't name a
    ComboBox, and a second button is plainer than a menu).

## Review focus

1. **Gaps and the side rule**: a ball that leaves through a gap must not be pulled back inside; a ball moving along
   the closed part must never slip through; a fast ball must not tunnel through the band beside a gap. Tests in Task 1.
2. **Goldens**: no golden line moves (Task 1 and Task 6).
3. **Merge in a crowd**: two firings about the same pair (A hits B and B hits A) merge once; a ball merged away in
   this pass is never merged again; the 4096 actions limit holds. Tests in Task 2.
4. **Melody after a seek**: the melody's place after a seek is the same as playing from the start. Test in Task 3.
5. **Scenes**: every new starter passes the library checks and looks right at full size (Task 5).

---

### Task 1: Gap in a shape

**Files:**
- Modify `crates/bpe-runtime/src/walls.rs` (`Gaps`, `Wall.gaps`, gapped keep-clear, sides, middle crossing),
  `crates/bpe-runtime/src/lib.rs` (export `Gaps`), every `Wall { .. }` literal in the workspace (add `gaps: None`).
- Modify `crates/bpe-kit/src/things/shape/mod.rs` (settings), `sim.rs` (build the gaps), `draw.rs` (pass them).
- Modify `crates/bpe-gfx/src/frame/instances.rs` (`PathInstance.gaps`), `frame/paths.rs`
  (`Frame::cut_gaps(index, ..)`), `renderer/mod.rs` (`PATH_ATTRIBUTES` location 7), `shaders/paths.wgsl` (`PathIn`,
  `PathOut`, `path_along`, `gap_cut`, used in `path_signed`), `shaders/shape_layers.wgsl` (`shape_at`).

**Runtime interface (`bpe_runtime`):**
- `pub struct Gaps { pub spans: Vec<(f32, f32)>, pub guide: Arc<[Vec2]>, pub places: Arc<[f32]> }`, derive Clone,
  Debug, PartialEq. `spans` = (middle, half width) in place units (0..1). `guide` = the outline in its own frame
  (turned back by `Outline::angle()`, from `Outline::center()`, divided by `Outline::reach()`), from the first corner
  clockwise, closed; `places[k]` the place of `guide[k]` (rising, below 1). `Gaps::round()` (no guide) measures by the
  angle from the own frame's top (`atan2(x, -y)`, an ellipse's own point divided by its half axes first).
- `impl Gaps { pub fn place(&self, outline: &Outline, q: Vec2) -> f32; pub fn at(&self, outline, place) -> Vec2;
  pub fn open(&self, place) -> Option<(f32, f32)> /* the span's ends */; pub fn open_at(&self, outline, q) -> bool }`.
  `at` snaps the guide point onto the true outline (`Outline::nearest`).
- `Wall.gaps: Option<Arc<Gaps>>` (None: closed, today's maths everywhere).
- `keep_clear_from`: with gaps (and not solid) → `keep_gapped(position, previous, before, velocity, radius, limit,
  wall, gaps)`: the wanted side from `gapped_side`; up to 4 pushes: the nearest point on the whole outline; where it is
  closed, today's `keep_on_side` push; where it is open, the nearest cap end is a round cap: the ball is kept `limit`
  from it. Contact normal from the cap or the band.
- `gapped_side(previous, position, before, wall) -> bool` (inside?): `before.inside(previous)`, unless
  `wall.outline.inside(position)` differs and the crossing along previous→position (24 halvings on `inside`) is open:
  then the side it is on now.
- `on_own_side` and `middle_crossing` use the same side; `middle_crossing` returns `None` where the crossing is open.

**Kit settings (`ShapeSettings`, group Body, `when = "fill=hollow"`):** `gaps: u32` ("Gaps", range 0..8, simple),
`gap_size: f32` ("Gap size", range 0.01..0.5, keyframe, pulse not needed, default 0.12, `when = "gaps"` as well via
`fill=hollow`), `gap_at: f32` ("Gap at", range 0..1, keyframe, default 0). Keys restart physics (all settings already
do). `BODY_HEADINGS` test list gains the three keys. `ShapeSettings::gap_spans() -> Vec<(f32, f32)>`.

**Kit sim:** `ShapeState.gaps: Option<Arc<Gaps>>` built in `build` and `settings_changed` from `gap_spans` and the base
outline (`gaps_for(&settings, &base, rotation)`: Ring sides 0 and Ellipse → `Gaps::round`; others the guide from
`outline_path().flatten(FLATTEN)` mapped into the outline's own frame, places by length); `wall()` puts it on the
Wall. Rebuilt only when the gap settings or the outline's form change (keys on Size alone keep the same guide).

**Drawing:** `PathInstance.gaps = [first record, count, 0, 0]`; each gap is two `path_points` records:
`[from, to, 0, 0]` (place, wrapping when from > to) and `[ax, ay, bx, by]` (the cap ends in the instance's shape
units). `wall_form` is unchanged; `draw_part` calls `frame.cut_gaps(index, &gap_ends)` after `push_form` when the wall
has gaps. Shaders: `path_along(q, b)` (circle: angle; ellipse: angle of its nearest point over its axes; points: the
length along) and `gap_cut(q, along, sd, gaps) -> f32`: inside a span, `sign(sd) × distance to the nearer cap end ×
scale`; `path_signed` and `shape_at` apply it when `gaps.y > 0.5`. A shape with no gaps draws as before (gaps all 0).

- [ ] **Step 1: Tests** (runtime `walls.rs` tests): `a_ball_leaves_a_ring_through_its_gap` (ring r 300, gap at the
  right, ball moving right from the middle: ends outside, no contact on the way), `a_ball_bounces_off_the_closed_part`
  (moving left: contact, kept inside), `a_ball_hits_a_gap_end` (moving at a cap end: contact with the normal from the
  cap), `a_ball_outside_comes_back_in_through_the_gap`, `a_fast_ball_beside_the_gap_does_not_tunnel`,
  `gapped_side_is_today_without_a_crossing`, `places_go_round_from_the_first_corner` (a square's guide: 0, 0.25, 0.5,
  0.75 at its corners; a circle's top 0, right 0.25). Kit (`shape/sim.rs` tests): `no_gaps_build_a_closed_wall`,
  `gaps_spin_with_the_shape` (spin 90°/s, after 1 s the gap's place is a quarter on from where it was in world
  terms), `a_ball_escapes_through_a_gap_and_leaves_fires` (rule When enter Leaves on the ring fires once),
  `gap_size_is_capped`. Picture (`bpe-kit/tests/` new `gap_pictures.rs`, `test_device()`): a ring with a gap at 3
  o'clock: a pixel on the ring there is the background; on the ring at 9 o'clock it is the ring's colour; the same
  with a glow layer and a polygon.
- [ ] **Step 2:** Run `cargo test -p bpe-runtime walls` and `cargo test -p bpe-kit shape`; expected FAIL (missing).
- [ ] **Step 3:** Implement as above.
- [ ] **Step 4:** Run the tests; expected PASS. Run `cargo test -p bpe-kit --test golden -- --test-threads=2`
  (expected unchanged) and `restarts`.
- [ ] **Step 5: Commit** "Shapes: gaps balls get out through".

### Task 2: Merge on touch

**Files:** Modify `crates/bpe-runtime/src/rules/actions.rs` (`Action::Merge`, `World::merge`); create the block in
`crates/bpe-kit/src/rules/balls.rs` (`Merge`, `MergeSettings`, `MergeGrow`); register in the kit (where `Split`
registers). Tests in `crates/bpe-kit/src/rules/tests_part5.rs` (or a new `tests_merge.rs`).

**Interfaces:**
- `Action::Merge { thing: ThingId, row: u32, other: ThingId, other_row: u32, size_match: Option<f32>, grow: Grow }`
  where `pub enum Grow { AddAreas, Times(f32) }` (runtime). Applying: both live and not the same ball, neither
  thing broken; `size_match` Some(p): `|ra − rb| ≤ p × max(ra, rb)` or nothing happens; new radius `sqrt(ra² + rb²)`
  or `ra × times` (finite, at least 0.5); position the area-weighted middle (kept at That's place when across a wall,
  `clear_way`); velocity `(ra² va + rb² vb) / (ra² + rb²)` (a frozen ball's own held velocity counts); `previous`
  and `start` set to the new place; the Other ball is removed (`pop`, a Remove event) and its rule state forgotten
  (`forget_row`, frozen entries dropped).
- Kit `do.merge` "Merge balls" (Do): `who: Who` (default That), `with: Who` (default Other), `same_size: bool`
  ("Only same size", default true), `size_match: f32` ("Size match", %, 0..100, default 5, `when = "same_size"`),
  `grow: MergeGrow` ("Grow": Add areas / Keep size), `times: f32` ("Amount", ×, `when = "grow=keep_size"`, default
  1). Help: "Two touching balls become one: That keeps its look and grows by the Other's size." Its `act` pairs each
  target of Who with each of With (each one ball) and queues a Merge per pair.

- [ ] **Step 1: Tests:** `two_balls_that_touch_merge_into_one` (two equal balls in a Balls group fired at each other,
  rule When hit (Who group, Hits group) → Merge: after the hit there is one live ball, its radius `r √2`, its momentum
  the sum), `merge_only_same_size_skips_different_sizes`, `keep_size_times_grows_that`, `a_pair_merges_once`
  (both firings of the pair queue; one merge), `merging_logs_a_remove_for_the_other`, `merge_across_things_keeps_thats_thing`.
- [ ] **Step 2:** Run; expected FAIL. **Step 3:** Implement. **Step 4:** Run; expected PASS; goldens unchanged.
- [ ] **Step 5: Commit** "Rules: Merge balls".

### Task 3: Melody list

**Files:** `crates/bpe-kit/src/sound/notes.rs` (`Plays::Melody` last in `ALL`, `MelodyEnd { Wrap, Hold }`,
`MAX_MELODY` 1024), `crates/bpe-kit/src/sound/voice.rs` (`notes_melody: Vec<NoteSlot>` "Melody", `notes_melody_end:
MelodyEnd` "At the end", both `when = "notes_plays=melody"`), `crates/bpe-sound/src/voices/slots.rs` (`NotesSpec`
gains `melody_end`), `crates/bpe-sound/src/patch.rs` (`ShapeSpec.melody: Arc<[NoteSlot]>`, capped at `MAX_MELODY`),
`crates/bpe-sound/src/voices/pick.rs` (`pub fn melody_step(len, hits, repeat, end) -> Option<usize>`),
`crates/bpe-sound/src/voices/shape.rs` (`melody_hits` counted in `pick_for`, reset with the voice, carried by
`adopt`), `crates/bpe-sound/src/music/midi_import.rs` (`MidiImport.melody: Vec<NoteSlot>`: every chord in time
order, up to 1024), `crates/bpe-studio/src/sound_tab/notes/` (`melody.rs`: the step row, picked step, buttons, strip)
and `sound_tab/write.rs` (`import_midi_melody`), `sound_tab/mod.rs` (the button and `TabEdit::ImportMidiMelody`).

- [ ] **Step 1: Tests:** `melody_step_wraps_and_holds` (pure), `each_hit_plays_the_next_melody_step` (bpe-sound test:
  a shape with Plays Melody [60, 62, [64, 67]] hit 4 times plays 60, 62, 64+67, 60 (Wrap) and 60, 62, 64+67, 64+67
  (Hold), through the engine's note log), `a_seek_keeps_the_melodys_place`, `midi_import_makes_a_melody_in_time_order`,
  editor `melody_steps_can_be_added_and_removed` (kittest names "Add step", "Remove step", "Step 1").
- [ ] **Step 2:** Run; FAIL. **Step 3:** Implement. **Step 4:** Run; PASS; sound goldens unchanged.
- [ ] **Step 5: Commit** "Sound: a melody each hit steps through".

### Task 4: Pitch follows size

**Files:** `crates/bpe-kit/src/sound/voice.rs` (`play_size_pitch: f32` "Pitch follows size", st per doubling,
range −48..48; `play_size_at: f32` "Size for the written note", px, 1..2000, default 20, `when = "play_size_pitch"`),
`crates/bpe-sound/src/patch.rs` (`ShapeSpec.size_pitch: (f32, f32)`), `voices/shape.rs` (`start_note` adds
`−amount × log2(radius / at)` to the note's pitch, clamped to ±48, when the ball is in the world).

- [ ] **Step 1: Test** `a_bigger_ball_plays_lower` (bpe-sound: two balls of radius 20 and 40 hitting a shape with 12
  st per doubling: the second note's pitch is 12 below; with 0 both play the note). Use the engine's note log (or
  `ShapeVoice` pitch read through the notes' frequency in a short render).
- [ ] **Step 2–4:** FAIL, implement, PASS. **Step 5: Commit** "Sound: pitch follows a ball's size".

### Task 5: Starters 9 to 14

**Files:** create `crates/bpe-kit/src/library/starters/{spinning_triangle,neon_tube,lit_room,breakable_bricks,
bounce_off_words,sparks_on_hit}.rs`; modify `starters/mod.rs`, `library/mod.rs` (`ENTRIES` order 9..14), `build.rs`
(only helpers a scene uses: `fill` for the Health card, `light`), `tests/library.rs` (`levels_so_far` (14, 0, 0)),
`bpe-studio/src/scenes_window/tests.rs` (tiles below the fold: `scroll_to_me()` before clicking).

| id | Seconds | Things | How |
| --- | --- | --- | --- |
| spinning-triangle | 20 | "Triangle" (polygon 3, size 440, spin 20°/s, look `shape.glow_ring`), "Ball" (r 30, 650 px/s, `electric`, a Trail layer) | Triangle has the Sound card: minor pentatonic, Plays Up. |
| neon-tube | 20 | "Hexagon" (polygon 6, size 430, thickness 12, `shape.neon_tube`), "Neon ball" (r 34, `glow_orb`) | Background near black, bloom 0.6. Hexagon has the Sound card, Plays Random over a major pentatonic. |
| lit-room | 20 | "Room" (rectangle 900 × 1500, look `shape.solid` in warm grey, thickness 30), "Lamp" (Point light at the top, size 60 for soft shadows), "Balls" (group of 5, r 46, `marble`) | The scene gets its Lighting card through `add_light`; low ambient so shadows read. Room has the Sound card. |
| breakable-bricks | 25 | "Room" (box), Folder "Bricks" with 15 filled bricks (3 rows × 5), each a Health card (Starting health 3, At zero Break, Shatter), "Ball" (r 26, 900 px/s) | Ball has the Sound card (Balls voice). |
| bounce-off-words | 20 | Text look Balls inside a word ("Bounce word", words "BOUNCE"), its balls | Uses the Text look as it comes; the word's Sound card plays its letters. |
| sparks-on-hit | 15 | "Floor" ring or box with `shape.sparks` plus a Smoke particles layer, "Ball" (`ember`) | Floor has the Sound card. |

Each scene is tuned until it passes the library checks; poster seconds where the idea shows.

- [ ] **Step 1:** Bump `levels_so_far` to (14, 0, 0); run; FAIL.
- [ ] **Step 2:** Write the six scenes. **Step 3:** `cargo test -p bpe-kit --test library -- --test-threads=2` and
  `cargo test -p bpe-sound --test library_sound`; PASS. Window tests: `cargo test -p bpe-studio scenes_window`.
- [ ] **Step 4:** Full-size look check `LOOK_ONLY=library` (release); open the new scenes' pictures and crops, plus a
  gap scene in `full_size_look.rs` ("gaps": a ring with two gaps, a glow ring and a hexagon with one); fix what looks
  wrong. **Step 5: Commit.**

### Task 6: Notes, checks, review, PR

- [ ] Notes: `docs/notes/engine.md` (Gaps: walls and drawing), `rules.md` (Merge), `sound.md` (Melody, Pitch follows
  size), `library.md` (part 2 As built, new helpers); this plan's As built.
- [ ] `cargo fmt --all`, clippy, the whole suite once in the background (goldens unchanged, release goldens too).
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Push, draft PR, subscribe; when green and reviewed, merge.
- [ ] Handover `/mnt/project-files/ballphysics/builtin-scenes/part2-handover.md`, memory, message the coordinator.

## As built

All six tasks landed as planned, with these changes made while building:

- **Gaps**: the drawing cuts the band in the shader at the gap ends measured by the shader's own `along` (computed on
  the CPU by `form_along`), so it lines up on moulded outlines too. The guide type is `walls::Guide`.
- **Merge**: tests needed the balls' Bounce cards' Hit other balls on (off by default); the notes say so.
- **Melody**: the melody lives in `NotesSpec` (not `ShapeSpec`); a step holds at most 16 notes. While Plays is
  Melody the slot grid is hidden (a line says the slots rest) and the keyboard edits the picked step. The editor's
  clear button is **Clear melody**. Import MIDI as a melody also sets Plays to Melody and the keyboard range.
- **Pitch follows size**: `ShapeVoice::playing_pitches` was added for its test. The size setting shows only when the
  amount isn't 0.
- **Text with other words**: `add_text_saying` (and `Lib::text_saying`) gives a Text look its words before the
  letters are laid out, so Balls inside a word places its balls in BOUNCE's letters.
- **Scenes**: Breakable bricks uses three balls fanned out ("Balls") and Starting health 2, as one ball with no
  gravity bounced on a repeating path and broke one brick in 25 s; it now clears the wall in about 21 s. Lit room's
  five marble balls launch each a different way at 650 px/s with bounciness 1, so they don't settle in a row. Sparks
  on hit throws 36 brighter sparks a hit plus a puff of smoke, its poster just after a hit (4.75 s). Bounce off words
  has no bloom (its look glows on its own). Helpers added: `fill`, `light`, `look_in`, `text_saying`.
- The Scenes window tests needed no change.
