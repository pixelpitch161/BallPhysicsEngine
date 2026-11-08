# Text thing, part 4: letter moves — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use checkbox (`- [ ]`)
> syntax. Project rule: one Opus review of the whole part before the PR, no review after every task.

**Goal:** letters that move and things that read a Text. A Text's layers can be laid over **each letter** or **each
line** instead of the whole text, and worn by only some letters; a list of **letter moves** (Wave, Jump on the beat,
Jump on hit, Flicker, Follow sound, Shake, Spin each letter, Scatter and come back, Draw on) moves, turns, stretches,
fades and traces each letter, pure in time; a light can ride or aim at a Text's **pen** (the point Draw on is
tracing); a group of balls and a Particles thing can take their word from **a Text thing** (its font, layout and
path). Four new looks (Word made of balls #15, Particle word #16, Laser writing #17, Text follows sound #22) and the
Neon sign gains its flicker.

**Architecture:** letter moves are layers of a new kind family in the `look.shape` card's `LayerSection::Reaction`,
offered only on a Text (a new `LayerKind::fits(thing_kind)`). Drawing reads them in `text_copies` after `arrange` and
the Show (`things/text/moves.rs`), folding every move into each letter's `LetterMove` (which grows a turn, a stretch
and a Draw on trace). Per-letter and per-line layers are made by copying the Text's one set of records once per
distinct unit and patching where each such layer's gradient is laid (`modules/look/shape/units.rs`); the shapes
shader gains one branch (a record's own middle for Round from the middle, only read when its new field is set, so
shapes draw byte for byte as before). `DrawCx` and `TextAt` carry the Follow sound reader, so a Follow sound move
reads each letter's level at its own moment. Moves are drawing only (as the Show is): walls, parts and goldens
don't move.

**Tech stack:** Rust, wgpu/WGSL, egui 0.37 + egui_kittest. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-07-text-thing-design.md`, section "Letter moves (part 4)" and "Looks by
part" (4). Read parts 1 to 3's "As built" first.

## Global constraints

- Nothing moves for scenes without a Text: physics goldens (debug and release), sound goldens, rules goldens,
  `unlit_pictures`, `paths_tests::shapes_draw_as_before`.
- A Text without moves, with every layer Across the whole text and worn by every letter, draws exactly as in part 3
  (the same instances and records; the part 1 to 3 drawing tests stay as they are).
- Pure in time: every move is worked out at the moment drawn from the settings, the beat, the log and the sound
  reader; nothing is kept between frames.
- Letter moves change only the picture: a walled Text's wall, its parts and the physics fingerprint never read them.
- Plain words in every label and help; no colours made up in the editor; scene format stays 12.

## Decisions (each settles something the spec left open)

1. **Which layers fit which things.** `bpe_core::LayerKind` gains `fn fits(&self, thing_kind: &str) -> bool`
   (default true). `Scene::add_layer`/`add_layer_in` refuse a kind that doesn't fit the card's thing
   (`EditError::NotAccepted`); `paste_look`/`paste_layer` skip (look) or refuse (layer) such kinds; the editor's add
   menus list only fitting kinds. The letter moves fit `text` only.
2. **Letters by number.** `bpe_core::ThingKind` gains `fn layers_by_number(&self) -> bool` (default false; true for
   Text). `set_layer_only` then also accepts a Text's drawn layers and moves (any section but Sub/Around/Group): `only`
   holds 0-based letter numbers (letter k of the layout's drawn letters, as parts count them; each riding copy counts
   its own). The editor shows them 1-based.
3. **Across** (Text only): a new setting `across` on the shape's Colour and gradient, Outline and Glow layers: **Across
   the whole text** (default, `whole_text`) / **Each letter** / **Each line**, label "Across", right under the colours.
   It decides where the layer's own gradient is laid and measured from: its Straight extent, its Round-from-the-middle
   middle and reach, and every other form's centre and turn. Each letter turns the gradient with the letter (a
   letter round a circle carries its own "down"); Each line keeps the text's turn. Shader layers and sub-layers stay
   whole (their in-shape space is the instance's). On a Shape the row is hidden (`look::shape::TEXT_ONLY` keys) and the
   value is ignored by `ShapeStack` (only the Text's draw reads it).
4. **Per-unit records.** When some shown layer is Across Each letter/line, or worn by only some letters, the Text
   writes its whole records once (as now, stamps applied), then for each distinct (letter-unit, line-unit, worn set) a
   patched copy (`units::records_for_unit`), each letter's instance naming its copy. Patches, per patched layer record
   (and only when its colours are a gradient, `paint.row ≥ 0`): code 1 (Straight) `p1` = the unit's extent along the
   turned direction; code 2 (Round) `paint.place.zw` = the unit's middle, `p0.z` = its reach (largest distance from the
   middle, ≥ 1e-3); any other form `paint.place.xy` = middle + turned centre and `paint.angle` += the unit's turn in
   degrees. Unworn layers get `amount[0] = 0` and `LayerGpu::UNWORN`. At most `MOST_UNIT_SETS` (2048) copies a frame;
   past that the rest share the whole records. The shader's `shape_colour` reads `p0.z > 0` (and no band) as "Round from
   this record's own middle" (`length(q − place.zw) / p0.z`); shapes write 0 there, so they are unchanged.
5. **Letter moves**: layer kinds (`modules/look/shape/moves.rs` settings, `things/text/moves.rs` maths), section
   Reaction, card `look.shape`, `repeats`, no sub-layers, fit Text only. Every move has **Amount** (0 … 1, scales what
   it does, keyframe, pulse) first. Kinds and settings:
   - **Wave** `text.move.wave`: Height (px, 30), Letters a wave (8), Speed (waves/s, 0.5), Tilt (°, 0).
     `y = −Height·sin(2π(Speed·t − k/Letters))`, turn `Tilt·cos(…)`.
   - **Jump on the beat** `text.move.beat`: Height (40), Every (beats, 1), Takes (s, 0.3), Each letter later (s, 0.04),
     Squash (0 … 1, 0.3). The scene's tempo (`things::scene::draw::beat_at` maths through `World.beat_seconds`); a jump is
     `4u(1 − u)` over Takes after each beat, letter k's beat moments shifted by k·later; squash stretches y up and x in at
     the top.
   - **Jump on hit** `text.move.hit`: Hits of (`Target`, default Me), Height (50), Takes (0.35), Which letters (The hit
     letter / Every letter), Each letter later (s, 0.03). Me on a walled Text: each letter its own hits (`b.item`) for
     The hit letter; on a Text riding balls: each copy its ball's hits; else every hit of the target (any side) moves
     every letter. The latest hit before the moment only (`4u(1 − u)`).
   - **Flicker** `text.move.flicker`: Style (the lights' `FlickerStyle`), Speed (8), Seed (0), Together (off: each
     letter its own). Opacity × `light::draw::flicker(t, Amount, Speed, Style, Seed + k)`.
   - **Follow sound** `text.move.sound`: Level (0 … 1, keyframe, pulse: the row to set to Follow sound), Jump (px, 60),
     Stretch (0.5), Grow (0.1), Each letter later (s, 0), Each its own (0 … 1, 0.3), Seed. Letter k's level is the
     resolved Level, and when Level follows sound (a `PulseSource::Sound` pulse on its address) with a reader, the
     pulse's share moved to `t − k·later` (`resolved + amount·(level(t − d) − level(t))`, clamped to 0 … 1), times
     `1 − own·rand(seed, k)`. Up by Jump·level, y stretched by 1 + Stretch·level, both by 1 + Grow·level.
   - **Shake** `text.move.shake`: Distance (px, 6), Turn (°, 4), Speed (12), Seed. Smooth seeded noise (a sum of three
     sines with seeded phases) per letter and axis.
   - **Spin each letter** `text.move.spin`: Speed (°/s, 90), Each letter later (°, 0), Swing (°, 0: turn round;
     more: rock back and forth that far, at Speed/360 a second).
   - **Scatter and come back** `text.move.scatter`: Starts at (s, 1), Distance (px, 400), Out takes (0.6), Stays out
     (0.4), Back takes (0.8), Turn (°, 180), Fade (0 … 1, 0), Every (s, 0 = once), Seed. Each letter a seeded way out;
     ease out going, ease in-out back.
   - **Draw on** `text.move.draw`: Starts at (s, 0), Takes (s, 3), Order (One after another / All together), Pen width
     (px, 6), Then (Lines only / Lines and fill / Fill only), Fill takes (s, 0.5). One after another runs a single pen
     at an even speed along every letter's contours in turn (each letter's share by its outline length); All together
     traces every letter at once. While tracing a letter draws as a band of the pen width along the part traced
     (contours out and back, so the band covers only that part); traced, it draws as the band (Lines only), the band
     with its fill fading in over Fill takes (Lines and fill), or the fill fading in while the band fades out (Fill
     only). Untraced letters don't show.
   Moves are folded in list order: offsets add, turns add, scales and stretches multiply, opacities multiply, and the
   first Draw on wins. Offsets are in the letter's own frame (up is the letter's up). A move worn by only some letters
   (`only`) moves those. A switched-off move does nothing. Fast letters take the turn and a uniform scale (the stretch's
   geometric mean); a Draw on letter walks its outline.
6. **`LetterMove`** (`show.rs`) gains `turn: f32` (rad), `stretch: Vec2` (×, in the letter's frame) and
   `trace: Option<Trace { share, band: f32 (half width), fill: f32, band_shown: f32 }>`; `is_whole` checks them too.
   `TextNow::contours_moved` applies scale × stretch in the letter's own frame about its middle, then its turn, then the
   offset; `TextNow::traced(k)` gives the band contours of Draw on's share.
7. **The pen** (`moves::pen(at) -> Option<Vec2>`): where Draw on is tracing now, in the world (a screen Text's mapped
   back through the camera, `Camera::to_world`), for the first copy: the moving head of the letter being traced (All
   together: the first letter still tracing). `None` before Draw on starts, after it ends, or without Draw on.
8. **Lights read the pen.** `RidesOn` gains **The pen**: a light inside a Text sits on its pen (and shows nothing
   while there is none); The middle sits on the Text's middle; anything else inside a Text sits at its own Position.
   `Points` gains **At the pen**: inside a Text it aims at the pen from where it sits, and a laser or beam stops there
   (its trace's length is at most the distance); with no pen it shows nothing. Worked out in `light::draw::places`
   (which has the `DrawCx`); `places_in` (the editor's handles) keeps its own Position for these.
9. **A word from a Text thing.** `things/text/shape.rs::letters_of(tree, id) -> Vec<Vec<Vec2>>`: the Text's saved
   settings (keys and pulses aren't followed), its words with every value in braces left out, laid out in its font and
   placed on its path at 0 s (Along a shape reads as Straight), in its own frame (middle at 0), not turned by its
   Rotation. Cached by the layout cache. **Balls**: the group's Word shape gains **Made of** Typed words / A Text thing
   and **Text** (a `Target`, its first Text): the lattice fill of those contours (`shapes::fill_letters`, the old
   `spell` lattice). **Particles**: Made of gains **A Text thing** with **Text** (Target): `spots::fill_spots` of those
   contours (`SpotsKey::TextThing { hash, spacing }` carrying the contours, equal by hash). Both sims read the scene
   (`ThingSim::reads_tree(&self, resolved)` now takes the thing's resolved settings, so only these read it) and hash
   the Text's saved settings and its scene font through `fingerprint_more` (now hashed for beside things too).
10. **The draw reads sound.** `DrawCx` and `TextAt` gain `sound: Option<&dyn SoundAt>` (frame.rs passes the frame's;
    the probe none). `bpe_sound::follows_sound` is unchanged: a move's Level following sound is a pulse like any other.
11. **Looks** (`LOOKS` grows to 20):
    - **Word made of balls**: a Text "HI" (Bowlby One, 300 px, in the world, drawing nothing: its Look card removed;
      the Words card spells the balls) and a group of balls Word from it, radius 7, step 16, no gravity, and a rule
      "When a ball of the word is hit → Change its Gravity to 1"; a big ball falls on the word from above.
      If a rule can't change one ball's gravity, the group falls under gravity 0 with Drag and only the big ball falls
      (said in the help).
    - **Particle word**: a Particles thing made of A Text thing ("GONE", Space Grotesk 160 px, drawing nothing), Keep a
      crowd, Spring home, and Wind with gusts that blow it away and let it form again.
    - **Laser writing**: "LASER" in Orbitron 200 px in the world, Draw on One after another over 4 s, Lines only, pen
      6 px, a red Paint and a red Glow giving off light; a red Laser inside it at (0, −700) pointing At the pen, and a
      Point light riding the pen (the hot tip). Adds the Lighting card.
    - **Text follows sound**: "BASS" in Anton 260 px on the screen with a Follow sound move whose Level follows Bus 1's
      level (`PulseSource::Sound { shape: ROOT, source: Bus(0) }`), Each letter later 0.05 s, Each its own 0.3.
    - **Neon sign** gains a Flicker move (Neon buzz style, Amount 0.25).
12. **Editor.** For a Text the Look editor shows a **Letter moves** list (the Reaction section, "+ Add move", rows like
    reactions) under the Layers; a layer's or move's row menu on a Text has **Only some letters…** (a box taking "1, 3,
    5-7", Enter applies) and **Apply to every letter**, and its summary says "only N letters". The Across row shows only
    for a Text. Light Rides on / Points gain their choices from the settings. Balls' Word and Particles' Made of rows
    show their Text picker from the settings (a `Target` row).

## Files

| File | What |
| --- | --- |
| `crates/bpe-core/src/registry.rs`, `layers.rs` | `LayerKind::fits`, `ThingKind::layers_by_number`, add/paste/only checks |
| `crates/bpe-runtime/src/plugins.rs`, `world.rs` | `reads_tree(resolved)`, beside `fingerprint_more` |
| `crates/bpe-gfx/src/frame.rs`, `shaders/shape_layers.wgsl` | `DrawCx.sound`, per-record middle for Round |
| `crates/bpe-kit/src/modules/look/shape/{mod,layers,moves,units}.rs` | `across`, `TEXT_ONLY`, move kinds, per-unit records |
| `crates/bpe-kit/src/things/text/{draw,show,moves,shape,looks,mod}.rs` | moves, pen, letters for others, looks |
| `crates/bpe-kit/src/things/light/{mod,draw}.rs` | The pen, At the pen |
| `crates/bpe-kit/src/things/balls/{mod,shapes,sim}.rs` | Word made of a Text thing |
| `crates/bpe-kit/src/things/particles/{mod,spots,state,sim}.rs` | A Text thing source |
| every `DrawCx { … }` site | `sound` |
| `crates/bpe-studio/src/look_editor/{mod,layer_list}.rs`, `inspector.rs` | Letter moves list, only letters, Across hidden on shapes |
| `CLAUDE.md` | Text part 4 paragraph |

## Tasks

### Task 1: core and runtime
- [ ] `LayerKind::fits`, `ThingKind::layers_by_number`, add/paste/only rules (decisions 1, 2); `reads_tree(resolved)`;
  beside fingerprints hash `fingerprint_more`; `DrawCx.sound`.
- Verify: core tests (a kind that doesn't fit is refused; `only` on a Text's paint layer is kept, on a shape's refused);
  every golden unchanged.

### Task 2: letter moves
- [ ] Move kinds and settings, registered; `LetterMove` grows; `moves.rs` maths; `text_copies` folds them; `TextAt.sound`.
- Verify: unit tests per move (Wave's phase along the letters, a beat jump peaks mid-Takes after a beat, a hit jumps
  only the hit letter, Flicker dims, Follow sound reads a fake reader per letter with the delay, Shake is seeded and
  smooth, Spin, Scatter out and back, Draw on's share and Then, a move worn by some letters moves only those, a hidden
  move does nothing); a Text without moves draws exactly as before.

### Task 3: drawing letters that move and are traced, the pen
- [ ] `contours_moved` with turn/stretch, fast letters' turn/scale, Draw on bands and fills, `pen`.
- Verify: drawing tests (a traced letter is a band instance with `b.x` = half the pen width and fewer points; Lines and
  fill pushes both; the pen moves along the word and is `None` after); `text_pictures` gains a Draw on picture (half
  way: the left of the word lit, the right dark).

### Task 4: Across and only
- [ ] `across` setting, `TEXT_ONLY`, `units.rs`, the shader branch, the draw's per-unit records.
- Verify: unit tests (Each letter's Straight extent is the letter's; Round's middle and reach; another form's centre and
  turn; unworn layers at 0); `paths_tests::shapes_draw_as_before` unchanged; `text_pictures`: an Each letter gradient
  repeats on every letter (the same colour at each letter's left edge).

### Task 5: lights and the pen
- [ ] `RidesOn::ThePen`, `Points::AtThePen`, `places`, beam length.
- Verify: a Point light inside a Text riding the pen sits on `pen`; a laser At the pen ends at the pen; none without one.

### Task 6: words from a Text thing
- [ ] `shape.rs::letters_of`, balls Made of, particles Made of, fingerprints.
- Verify: balls fill a Text's letters (inside their contours, none in an O's hole); a Text's words change the balls'
  places after a rebuild (fingerprint changes); particles' spots fill it; without a Text both are as before.

### Task 7: looks
- [ ] The four looks and Neon's flicker (decision 11).
- Verify: every look resolves cleanly; Word made of balls's balls sit in the word and the big ball knocks some loose
  within 5 s; Laser writing's pen exists at 2 s and the laser ends there; Text follows sound's pulse.

### Task 8: editor
- [ ] Letter moves list, Only some letters, Across hidden on shapes.
- Verify: kittest: a Text's Look shows "Letter moves" and "+ Add move" adds a Wave; a shape's Look has neither moves
  nor the Across row; "Only some letters" writes `only`.

### Task 9: finish
- [ ] `cargo fmt`, `cargo clippy --workspace`, tests crate by crate (bpe-gfx `--test-threads=2`), goldens unchanged in
  debug and release (text goldens gain the Word made of balls scene only if added; diff against main).
- [ ] CLAUDE.md paragraph; this plan's "As built".
- [ ] One Opus review, fixes, PR, merge.

## Not doing (this part)

- Moves moving the wall or the parts (drawing only, like the Show); Across on Shader layers or sub-layers.
- Keys and pulses of a Text read by a word made of balls or particles (its saved settings are read).
- The experimental folds (part 5: Echo, Mirror, Colour split, Glitch, Melt).

## As built

- **Core/runtime.** `LayerKind::fits(thing_kind)` (default true; `layers::fits_thing`, checked by add/paste/paste_look)
  keeps letter moves and other Text-only layers off shapes; `ThingKind::layers_by_number` (Text: true) lets a layer's
  `only` hold 0-based letter numbers (`set_layer_only`). `ThingSim::reads_tree(&self, resolved)` now sees the resolved
  scene, and `beside_fingerprint` hashes `fingerprint_more` too.
- **Moves** (`modules/look/shape/moves.rs`, kinds `text.move.wave|beat|hit|flicker|sound|shake|spin|scatter|draw`,
  section Reaction of `look.shape`, `MOVES`): each with Amount (keyframe, pulse) and its own settings. Drawing reads
  them in `things/text/moves.rs`: `moves_of(look)` (enabled, with `only`), `apply(at, moves, now)` folds every move
  into each shown letter's `LetterMove` in list order (offsets add in the letter's frame, turns add, scale and stretch
  multiply, opacity multiplies, the first Draw on wins). Pure in time: Wave (sine along the text), Jump on the beat
  (`beat_at`), Jump on hit (`hits_for`: per letter on a walled Text for Me/The hit letter, per ball on a riding copy,
  else every hit of the target; Later, Decay), Flicker (`light::draw::flicker`), Follow sound (its Level pulse read at
  each letter's moment through `TextAt.sound`, `DrawCx.sound`), Shake, Spin each letter, Scatter and come back, Draw
  on (`DrawPlan`: One after another by outline length, or All together; `traced` gives the out-and-back pieces of the
  traced share and the pen; Lines only / Lines and fill / Fill only). `pen(at)` is the pen in the world. Moves are
  drawing only: walls, parts and goldens don't move.
- **Drawing** (`text/draw.rs`): `TextNow::mover`/`contours_moved`/`middle_moved`; a traced letter draws as a band of
  half the Pen width (`b.x`) over the traced pieces, plus its fill when shown; fast letters (glyph tiles) take the turn
  and a mean scale; traced and shattered letters never use tiles.
- **Across / only** (`modules/look/shape/units.rs`): Colours, Outline and Glow (own colours) have **Across** Whole text
  / Each letter / Each line (`TEXT_ONLY`, hidden on shapes). `unit_records` copies the Text's records once per distinct
  (letter or line, worn set), at most `MOST_UNIT_SETS` 2048, `lay_over` patching where a gradient lies (Straight: the
  unit's extent along the turned direction; Round: `place.zw` = the unit's middle and `p0.z` its reach, read by
  shape_layers.wgsl only when set and the record isn't a band; other forms: the centre and angle moved); a layer a
  letter doesn't wear is `unworn` (amount 0, `LayerGpu::UNWORN`). Every letter wearing everything uses the whole set.
- **The pen** (`things/light/`): Rides on **The pen** (`RidesOn::ThePen`) and Points **At the pen**
  (`Points::AtThePen`, a laser's beam ends there) for a light inside a Text; nothing shows while there is no pen.
- **Words from a Text** (`things/text/shape.rs`: `text_named`, `letters_of` from the saved settings with values in
  braces dropped, Along a shape read as Straight, `letters_hash`): Balls' Word **Made of A Text thing**
  (`WordFrom`, `word_text`; `shapes::with_text_word`, `fill_letters`) and Particles' Form a word **A Text thing**
  (`text_thing`; `SpotsKey::TextThing`, `refresh_spots`); both `reads_tree` and hash `letters_hash` in
  `fingerprint_more`.
- **Looks** (20 now): Word made of balls, Particle word, Laser writing (lit), Text follows sound (Bus 1); Neon sign
  flickers.
- **Editor**: the Look editor's **Letter moves** list on a Text (`+ Add move`), the row menu's **Only some letters…**
  (a box taking "1, 3-5", `parse_letters`, `letters_words`) and Apply to every letter; Across hidden on shapes.
- **Tests**: `moves_tests.rs` (13), `units.rs` (5), `text/draw.rs` (draw on, each letter, only, spin), `shape.rs`,
  looks `the_part_4_looks_play_out`, `tests/text_pictures.rs::a_gradient_starts_again_on_each_letter`, look_editor
  kittests.

## After review

Fixed: a light inside a walled Text rides its wall again unless it wants the pen or the middle of an unwalled Text
(`text_places` falls back to `places_in`, as the handles read it); Word made of balls sets Hit balls on its own two
Balls things; Jump on hit reads only the log's hits a hop can still be going from (`Log::since`); Follow sound's
per-letter delay reads levels clamped 0..1 as the pulse does; Round across each letter/line also reads the unit's
middle on band pieces (Draw on's line, Bounce inside); per-unit sets are capped by records too (`MOST_UNIT_RECORDS`
32768); a Balls word from a Text is spaced for the largest varied ball, and the Group view shows that word; the
Particles' letters are laid out again only when `letters_hash` changes; `mover` scales evenly without turning into the
letter's frame (part 3's maths for the Show); with letter moves a whole-text gradient lies over every letter as laid
out (fast letters always over their laid-out boxes), so it doesn't follow or jump with the moves; the Lights panel puts
a new light inside a selected Text and names it "on <Text>"; "Only some letters…" starts from the layer's letters each
time it opens and isn't offered on Blend layers or sub-layers; the light-sensing fingerprint hashes a Text's own
settings (its pen).

Known limits: `DrawPlan` and the draw each place every letter's contours a frame, and a pen light runs `text_now`
again; Round on an Outline or Glow (banded records) stays one gradient along the band.
