# Text thing, part 3: real letters — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use checkbox (`- [ ]`)
> syntax. Project rule: one Opus review of the whole part before the PR, no review after every task.

**Goal:** letters that are real objects. A Text set in the world can be a wall (balls bounce off its letters, or
stay inside them), each letter is something a rule can name, break, flash and give health to, a Text can play
notes when its letters are hit (By letter, Spell notes), letters meet lasers and the light probe like shapes do,
and five new looks: Balls inside a word (#7), Bounce off letters (#8), Pour in and fill (#9), Letters break apart
(#10), Letters that play notes (#11).

**Architecture:** the runtime learns two generic things: a wall outline made of many letters
(`Outline::Letters`, every contour of every letter in the Text's own frame, a box per letter), and **parts**: a
thing's items that rules name one by one the way they name a group's balls (`World.parts`, a Text's letters).
A contact names the part it touched (`Contact.part`), and the Hit event carries it in `b.item` (0 for every other
wall, so nothing else moves). The Text's sim lays its letters out with the same code the drawing uses
(`text_now`, through a copy of the saved scene the world keeps when a sim asks for it) and rebuilds the wall only
when its letters change. Rules, Health and Flash read parts where they read balls; the drawing reads each letter's
hits, flashes and break. The sound engine's shape voice already plays any thing with a Sound card; it learns two
play orders that read the hit letter.

**Tech stack:** Rust, wgpu/WGSL, egui 0.37 + egui_kittest. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-07-text-thing-design.md`, sections "Real letters (part 3)", "The thing"
(Balls) and "Looks by part" (3). Read part 1's and part 2's plans' "As built" first.

## Global constraints

- Nothing moves for scenes without a Text: physics goldens (debug and release), sound goldens, rules goldens,
  `unlit_pictures`, `paths_tests::shapes_draw_as_before`. A Hit on a shape still logs `b.item` 0.
- A Text stuck to the screen (the default) or riding a ball, emitter or shape has no wall and no parts, and draws
  exactly as in part 2 (the same `PathInstance`s; the part 2 drawing tests stay as they are).
- Pure in time for the drawing (hits, flashes and breaks read from the log and the rules state at the moment
  drawn). The physics is deterministic: the wall is a function of the settings at the step, the rules' text and
  the world.
- Plain words in every label and help text; no colours made up in the editor.
- Scene format stays 12; new settings are plain settings with defaults.

## Decisions (each settles something the spec left open)

1. **The Balls setting.** A new group **"Balls"** on the Words card: **Balls** `TextBalls` Pass through (default) /
   Bounce off / Bounce inside, and **Thickness** (px, `when = "balls=bounce_inside"`, span 1 … 60, range 0.5 …
   2000, default 10, keyframe). Bounce off is a **solid** wall (`Wall.solid`, thickness 0: a ball that starts inside
   a letter is pushed out at once; one in a letter's hole stays in the hole, which is outside the letter). Bounce
   inside is a **hollow** wall whose band is Thickness (balls stay on the side of each contour's band they started
   on, so balls inside a letter's stroke stay inside, and balls outside bounce off from outside too). Pass through
   is a hollow wall of no thickness that balls pass (`Wall.passes`), so rules' Enters / Leaves, lasers and the light
   probe see the letters.
2. **Which Texts are walls.** Only a Text **in the world** (`Thing.screen` false) that **rides nothing** (its tree
   parent is not a Balls, Emitter or Shape thing, `light::RIDES`) and has at least one letter. A screen Text's
   letters are on the screen, not in the world, and a riding Text is a crowd of labels. The Balls row's help says
   so ("Only for a Text in the world: turn off Stuck to the screen"). The editor shows the Balls group always.
3. **What the wall is made of.** The letters of the first copy `text_now` gives (the words as keyed, a rule's Set
   text, values in braces read at the step from the world *without the log*, the font, every Words and Path
   setting), placed by `arrange`, in the Text's own frame. **Show is drawing only**: a letter not shown yet (or
   gone) is still a wall (the show's Hits timing needs the log, and keeping walls apart from the show keeps the
   physics free of the drawing's timing). A value in braces read without the log counts hits from the balls' own
   counts (`live::fill` already does); the plan accepts that a `{hits}` that counts ball-to-ball hits can read
   differently in the wall than drawn (both are deterministic).
4. **`Outline::Letters`** (bpe-runtime `walls.rs`): `{ center, angle, thickness, letters: Arc<LetterWalls> }`.
   `LetterWalls { list: Vec<LetterWall>, lo, hi, reach }` and `LetterWall { part: u32, points: Vec<Vec2>,
   starts: Vec<u32>, lo: Vec2, hi: Vec2 }` (contour k is `points[starts[k]..starts[k + 1]]`, closed; the box in the
   text's frame). `inside(p)`: inside **any** letter, each letter even-odd over its own contours (a letter's
   contours are merged non-zero already, so even-odd is exact; letters that overlap each other are a union).
   `nearest_and_line` walks the letters by box distance and skips any whose box is further than the best so far;
   it also gives the part (`nearest_part`). `clear_by`: outside the whole box by more than `limit`, or outside every
   letter's box by more than `limit` (only when the ball belongs outside). `point(u)`: the furthest crossing of the
   ray from the middle over every contour. `reach`: the furthest point. `corners`: none. Every other match on
   `Outline` (the kit's `outline_points`, `wall_form`, `outline_centre`, foci, the studio's zones and shape
   handles, `bpe-gfx`'s wall uniform) gets an arm: a new `outline_contours(outline) -> Vec<Vec<Vec2>>` gives every
   contour (one for the old forms, as `outline_points`), and the tracers (`laser::scenery`, the probe's walls,
   swarm and piles segments, the scene's `lowest_shape`) read contours, not one list.
5. **`Contact.part: u32`**: the part of the wall touched (the letter's part number; 0 for every other outline).
   `keep_on_side` takes it from the nearest point it last pushed the ball off; `middle_crossing` from its nearest
   point. The wall pass logs `b: Party { thing: wall, item: contact.part }`.
6. **Parts** (bpe-runtime `parts.rs`, `World.parts: BTreeMap<ThingId, Parts>`): `Parts { alive: Vec<bool>, at:
   Vec<Vec2> }` (each part's middle in the world, for a flash's point, Teleport-free `place_of` and Aim at). A part
   is a letter: number k is the k-th drawn letter of the wall's layout (`layout.letters[k]`), counted from 0; the
   editor and rules say "letter k + 1". The Text sim writes `World.parts` whenever the wall's letters change: the
   count follows the layout (new letters alive), a broken letter stays broken by its number. A Text with no wall has
   no parts. `World::lone` keeps the kept things' parts.
7. **Breaking a letter.** `Action::Break { row: Some(k) }` on a thing with parts (and no bodies): when part k is
   alive, it is marked broken, the Break event logs item k (no Remove), the remains go in as `(thing, k)` with the
   Text's wall as it stood, and the sim leaves the letter out of its wall from the next step (`scripted` sees the
   change). A whole-Text break works as for a shape (wall taken, out of the order, remains `(thing, WHOLE)`); its
   drawing shows every letter with the whole break's look. A letter whose word changes after it broke stays broken
   by number.
8. **Who names a letter.** `who::is_part`, `who::part_number`. `Who::Thing { thing, ball: Some(n) }` on a thing with
   parts names letter n (alive), That / Other name the letter a firing is about (the hit's `b` side), and `matches`
   for When blocks compares the letter number. Do blocks that act on balls only (Motion, Split, Spawn, Tag on one
   ball) do nothing to a letter, as today they do nothing to a shape; Change setting on a letter changes the whole
   Text (the rules' per-row changes are balls' only: `change.rs` drops the row for a thing without bodies).
9. **Flash a letter.** `Action::Flash { row: Some(k) }` on parts logs a Flash with item k at the letter's middle;
   `row: None` flashes every live letter (as a whole group of balls flashes each ball). The drawing gives each
   letter its own hit data: Hit events whose `b` is (Text, k) and Flashes of (Text, k).
10. **Health per letter.** The Health card fits a Text (`modules/health` `fits` gains `text`). A thing with parts
    keeps a health per part, like a group per ball (`HealthOf.balls` reads "has rows": bodies or parts; `fill`,
    `damage`, `of`, `thing_share` count parts as rows). A hit on letter k takes from letter k; At zero Break breaks
    that letter. `{health}` on a Text reads its share's mean as for a group.
11. **Sound.** The Sound card fits a Text (`VoiceKind::fits`; `TextKind` slots Look, Bounce, Sound, Health;
    default Look only). Hits on a Text play its voice as hits on a shape do. `Plays` gains **By letter** (letter
    k + 1 plays the slot's (k + 1)-th note, wrapping, as By side) and **Spell notes** (a hit letter A to G, either
    case, plays its own note name in the octave of the slot's lowest note, C5 = MIDI 60 when the slot is empty;
    any other letter plays nothing). On a shape both read letter 0 (By letter plays the first note, Spell notes
    nothing); By side on a Text reads side 0. The voice reads the hit's `b.item` and, for Spell notes, the
    letter's character from the Text's state (`things::text::letter_char(world, thing, part)`, no allocation).
12. **Lighting.** Letters already block light, take surfaces and are Lights up targets through the drawing (part
    1 stamps the records). New here: the CPU side sees a walled Text like a shape: lasers trace every contour
    (a Bounce off Text is solid glass to the tracer, so Glass letters bend a beam), the light probe's walls and
    the scene's lowest shape read contours. A test pins that a laser bends through a glass Text and that a Text
    is a Lights up target. The Stage's extruded slab is the drawing's surface picture, so letters stand out of the
    wall already; a picture test checks it.
13. **Bounce inside draws bands.** With Bounce inside the letters draw as bands of Thickness along each contour
    (the instance's `b.x` = half the thickness, its box grown by it, the records' `Geometry.half` too), so what
    is drawn is what bounces.
14. **Drawing broken letters.** A broken letter draws while its remains show: Fade out and Pop as a `LetterMove`
    (scale about its middle, opacity), Shatter as pieces cut round the letter's middle (`shatter::cut` with the
    letter's outer contour, then each piece's site and cell moved by the letter's middle, so every piece is the
    letter's own instance clipped to its cell). A broken letter past its look draws nothing.
15. **Physics settings.** `TextSim::affects_physics`: every Words, Path and Balls key (not Show, Values, Riding or
    Fast letters). A new `ThingSim::fingerprint_more(scene, thing, resolved) -> u64` (0 by default; hashed by
    `fingerprint_of` when not 0) hashes a Text's Stuck to the screen and, when its font is one the scene keeps, the
    font's length and a few of its bytes, so loading or removing a font rebuilds a bouncing Text.
16. **The world keeps the scene for sims that ask.** `ThingSim::reads_tree() -> bool` (false); `World.tree:
    Option<Arc<Scene>>` is set in `World::build` when any built thing's sim reads it (a Text), so the Text's sim can
    call `text_now` (fonts, names, the tree). Shared by clones.
17. **Looks** (all in the world, each adds its balls next to the Text): **Balls inside a word** (Bounce inside,
    "BALLS", Bowlby One 360 px, Thickness 12, a dark body with a bright Outline, one ball per letter placed at the
    point of each letter's stroke furthest from its edges, `inner_points`), **Bounce off letters** (Bounce off,
    "BOUNCE", Archivo Black 300 px, a line of 9 balls above it), **Pour in and fill** (Bounce inside, "FILL",
    Bowlby One 420 px, Thickness 8, 60 small balls released one by one from each letter's inner point in turn),
    **Letters break apart** (Bounce off, "SMASH", Bungee 280 px, Health 3 a letter, At zero Break with Shatter,
    Flash on hit sub-layer, a line of balls above), **Letters that play notes** (Bounce off, "FACADE", Fredoka 260
    px, Sound on with Plays Spell notes, Flash on hit, balls above). None of them is shown on the screen.
18. **Editor.** The Balls group shows on the Words card (from the settings). `TargetChoices.letter_things` (every
    Text in the world) and the who picker's number box reads "every letter" / "letter N" there. `sounds(kind)`
    includes Text (Settings | Sound chips, "The sound the letters make when balls hit them"). The Follow sound menu
    and the mixer's shape picker list Texts with sound too. The add-slot menu offers Bounce, Sound and Health (from
    `slots()`).

## Files

| File | What |
| --- | --- |
| `crates/bpe-runtime/src/walls.rs` | `Outline::Letters`, `LetterWalls`, `LetterWall`, `Contact.part`, `nearest_part` |
| `crates/bpe-runtime/src/parts.rs` (new) | `Parts` |
| `crates/bpe-runtime/src/world.rs` | `parts`, `tree`, Hit `b.item`, `lone`, `fingerprint_more` |
| `crates/bpe-runtime/src/plugins.rs` | `reads_tree`, `fingerprint_more` |
| `crates/bpe-runtime/src/rules/{actions,health}.rs` | break/flash parts, health rows |
| `crates/bpe-kit/src/things/text/{mod,sim,draw,looks}.rs` | Balls settings, the wall, parts, per-letter hits, broken letters, bands, looks |
| `crates/bpe-kit/src/things/text/walls.rs` (new) | layout → `LetterWalls`, `inner_points` |
| `crates/bpe-kit/src/rules/{who,change}.rs` | parts |
| `crates/bpe-kit/src/modules/health/mod.rs`, `sound/voice.rs`, `sound/notes.rs` | fits, Plays |
| `crates/bpe-kit/src/things/light/{draw,laser}.rs`, `sensing/probe.rs`, `things/particles/{swarm,piles,births}.rs`, `things/scene/draw.rs`, `things/shape/{draw,foci}.rs` | `outline_contours`, new arms |
| `crates/bpe-kit/src/scenes.rs`, `tests/golden.rs` | `text_scenes`, goldens |
| `crates/bpe-sound/src/{patch.rs,voices/pick.rs,voices/shape.rs}` | By letter, Spell notes |
| `crates/bpe-gfx/src/frame.rs` | the `Letters` arm |
| `crates/bpe-studio/src/{widgets,inspector,follow}.rs`, `mixer/editors/mod.rs`, `preview/zones.rs`, `shape_handles.rs` | letters in the who picker, Sound tab, new arms |
| `CLAUDE.md` | Text part 3 paragraph |

## Tasks

### Task 1: the runtime
- [x] `Outline::Letters` and its methods (decision 4), `Contact.part` (decision 5), `Parts` and `World.parts`
  (decision 6), `World.tree` and `reads_tree` (16), `fingerprint_more` (15), the Hit's `b.item`.
- Verify: walls unit tests: a ball outside a solid letter bounces off it and the contact names the letter; a ball
  in an O's hole stays in it; a ball inside a hollow letter's band stays inside it; the box skip gives the same
  nearest point as the full walk; `point`, `reach`, `clear_by`. Every physics golden unchanged (debug).

### Task 2: the Text's wall
- [x] Settings (decision 1), `walls.rs` (layout + placed → `LetterWalls`), the sim (decisions 2, 3, 6, 7):
  `build`, `settings_changed`, `scripted` (rebuild only when the letters, their places or the broken set change;
  otherwise move the outline's centre and angle), `letter_char`.
- Verify: a ball dropped on a Bounce off word bounces off it and the Hit names the letter; Bounce inside keeps a
  ball inside the letter it starts in; Pass through logs no hits but `when.enter` sees the letters; a screen Text
  and a riding Text have no wall; a Spin turns the wall; a Words key changes the wall at its moment.

### Task 3: rules, health, flash (decisions 7 to 10)
- [x] `actions.rs` Break and Flash on parts; `health.rs` rows from parts; `who.rs`; `change.rs`; health `fits`.
- Verify: a rule "When hit → Break That's other" breaks only the hit letter (wall gone there, a ball now passes
  where it was, Break item k); Health 2 a letter breaks a letter on its second hit; Flash names one letter;
  `Who::Thing { ball: Some(2) }` on a Text names letter 2; rules goldens unchanged.

### Task 4: drawing (decisions 9, 13, 14)
- [x] Per-letter hit data, Bounce inside bands, broken letters (Fade out, Pop, Shatter, gone after).
- Verify: drawing unit tests (a letter's hit data is its own; a broken letter's instances are its pieces, clipped;
  the band's `b.x`); `tests/text_pictures.rs`: a Bounce inside letter is hollow in the middle of its stroke; a
  shattered letter's pieces move apart.

### Task 5: sound (decision 11)
- [x] `Plays::ByLetter`, `Plays::SpellNotes`, `VoiceKind::fits`, the pickers.
- Verify: `bpe-sound` unit tests: By letter picks the k-th note; Spell notes plays C for a "C", nothing for an "X",
  the slot's octave; a Text's voice plays on a hit (an engine test with the test-tone feature or the starter
  voice); sound goldens unchanged.

### Task 6: lighting and the other wall readers (decision 12)
- [x] `outline_contours` and the readers; `laser::scenery` contours.
- Verify: a laser through a Glass, Bounce off Text bends (its path turns at the letter); a Text is a Lights up
  target (`links::number_of` and the stamped record); the probe sees a shadow behind a walled Text.

### Task 7: looks (decision 17)
- [x] The five looks in `looks.rs` with `inner_points`; `LOOKS` grows to 16.
- Verify: every look resolves cleanly; each new look's settings; in "Balls inside a word" each ball starts inside
  its letter's stroke (the wall says inside); "Letters break apart" breaks a letter within 20 s; "Letters that
  play notes" has Spell notes.

### Task 8: editor (decision 18)
- [x] Who picker letters, Sound tab chips for a Text, follow menu, mixer shape picker.
- Verify: kittest: a Text's inspector shows the Balls rows and the Sound chip; the who picker reads "letter 2".

### Task 9: goldens and finish
- [x] `scenes::text_scenes` (test-only): balls in a hollow word, balls off a solid word, a letter breaking by
  health; `tests/golden-text*.txt` blessed in debug and release; every other golden unchanged (diff against main).
- [x] `cargo fmt`, `cargo clippy --workspace`, tests crate by crate (bpe-gfx `--test-threads=2`, deleting test
  executables between crates).
- [x] CLAUDE.md paragraph; this plan's "As built".
- [x] One Opus review, fixes, PR, merge.

## Not doing (this part)

- Letter moves, Across each letter, layers worn by some letters (part 4); the experimental folds (part 5).
- Walls for screen Texts or riding Texts; walls that follow the Show (decision 3).
- A Bounce card of its own for letters: the Bounce card on a Text works as on a shape (the hit thing's Bounce).
- Splitting, spawning, freezing or tagging a single letter.

## As built

- **Runtime, wall, rules, health:** as Tasks 1 to 3. The wall is made only for a Text set in the world that rides
  nothing (`sim.rs::can_be_a_wall`); `TextState` keeps `walled`, the `WallLetters` (layout, places, which letters
  were alive, the walls, each letter's middle) and each letter's character (`chars`, read by `letter_char` without
  allocating). The letters are laid out again only when the settings changed, a letter broke, or the words can move
  by themselves (values, a rule's text, a moving wave, a shape path); otherwise the outline only takes the new pose.
  `breaks::shows_looks` takes text, so a broken letter's Fade out / Pop / Shatter keeps its remains.
- **Drawing:** each letter's hits are its own (`draw.rs::letter_hits`, the log's hits by `b` = (text, k) and flashes
  by `a`), made only when the Text has parts. Bounce inside letters are drawn as bands of the Thickness (`band_half`,
  `PathInstance.b[0]`) and only while the Text is walled. A broken letter always walks its outline (no glyph tile);
  Fade out and Pop scale and fade it through its `LetterMove`; Shatter draws `remains::draw::shatter_letter` pieces
  cut round the letter's middle, each piece's convex cell replacing the letter's 4-corner box clip. No GPU shatter
  picture test: the unit tests check the pieces' clips and that they move apart.
- **Sound:** as Task 5 (`pick.rs`: `Who` Ball / Side / Letter, `SpellPick`, `note_letter`). By side reads sides only
  on a shape (`patch.rs` makes the side path only for shapes). The compressor's sidechain key and the mixer's shape
  picker take Texts.
- **Lighting:** as Task 6, plus glass Texts push benders (`TextDraw::light`, `Bender::round` of every contour), and
  the probe's `LOOKED` list takes text. A laser bends through a glass letter only when its At shapes isn't Stop.
- **Looks:** as Task 7, with what they needed to read well in the 9:16 shot: the words are `fitted` to `FIT_WIDTH`
  900 px; the balls of the falling looks sit at y −250 above the words and get Gravity 1 (new balls have none);
  Balls inside a word is a cyan to pink gradient with a glow; Letters that play notes drops one ball over each
  letter's inner point (an A's apex would split it). The balls are added beside the Text (`BallsBeside`,
  `custom_balls`); Health comes through `fill_slot`, the voice through `starter_voice` with Plays Spell notes.
- **Goldens:** `scenes::text_scenes` holds the five looks and a Spinning word (`tests/golden_text.rs`: 8 s, a line a
  second with events, letter hits, standing letters and a hash, and a seek check).
- **Editor:** as Task 8 (`TargetChoices.letter_things` are Texts set in the world; the number box reads "every
  letter" / "letter N").
- **After review:** only a Text with a wall has its Words, Path and Balls settings in the physics animation and the
  fingerprint (`ThingSim::affects_physics_in`, `sim.rs::walled_in`; Spin always), and `fingerprint_more` is 0 for
  every other Text, so typing into a title on the screen no longer restarts the run or the sound. A broken letter
  leaves the wall in the Break action itself (`rules/actions.rs`), before the step notes its walls as they were, so a
  ball inside it (Bounce inside) is let go instead of pushed into the next letter. Sand piles on every letter
  (`piles::Surfaces` keeps every contour), the light probe samples round every letter, Exact particles pass letters
  balls pass through (as a swarm does), a rule note or a particle moment plays letter 1 under By letter (only a Hit's
  `b.item` is a letter), a Text whose words hold values hashes the things' names (`of <name>`), and a walled Text whose
  letters didn't change only moves its wall and parts each step (no copying). Left as they are: the wall lays words
  out without the log or resolved scene, so `{ball name}` and standings by name read "Ball N" in the wall (a ranking
  in the world is drawn right but bounces off the plain names); a Strip light along a Text runs along one letter; a
  walled Text with no letters when the world is built keeps one health for the whole Text; Other = a letter tags
  nothing (`do.tag` tags balls or things).
