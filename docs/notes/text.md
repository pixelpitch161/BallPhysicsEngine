# Text thing (spec `docs/superpowers/specs/2026-11-07-text-thing-design.md`, plans `docs/superpowers/plans/text/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

Part 1 is in (plan `part1.md`, "As built" at its end): the **Text** thing (`bpe-kit/src/things/text/`, kind `text`,
family "Text", `sticks_to_screen` true, slots Look only; its Look card is `look.shape`, whose `fits` takes text, so every
shape layer and sub-layer works on words). The engine is `bpe-kit/src/text/`: `fonts.rs` (the 52 built-ins `BUILT_IN`
in seven `FontGroup`s, 55 files in `assets/fonts/text/` with their licences; the folder is the first of `BPE_FONTS`,
`<exe dir>/fonts`, the repo's `assets/fonts/text` holding `Anton.ttf` (`fonts_dir`); `find`/`find_in` look in the
scene's fonts first, then the built-ins, else `Found::Fallback` IBM Plex Sans SemiBold (`things::balls::shapes::FONT`)
with a `problem()` for the Font row; `problem_of` says it without reading the file; `is_font`, `family_name`),
`layout.rs` (`layout(font, LayoutSpec)` cached: rustybuzz shapes each paragraph, a glyph the font lacks takes no room,
greedy word wrap that never drops a word, skrifa outlines at the weight's axis place flattened within `TOLERANCE` 0.2
px, contours merged with i_overlay NonZero so variable fonts' overlaps leave no seam, the Slant sheared; the block's
middle at 0; `MOST_LETTERS` 4000, `MOST_POINTS` 2048 a letter) and `arrange.rs` (`PathAt` Straight / Circle / Wave
(shift in waves) / Edge (a closed outline, clockwise, `gap` off its centre line); degenerate paths read as straight).
Fonts loaded in the editor are kept in the scene (`Scene.fonts`, `bpe-core/src/scene_fonts.rs`: `SceneFont` saved as
base64, `add_font`/`remove_font`; skipped when empty, so scenes without fonts save as before; still format 12).
`TextSettings` (`things/text/mod.rs`): Words group (words, font, weight, slant, size = capital height, letter/line
spacing, align, wrap, case, spin) and Path group (path Straight / Round a circle / Along a wave / Along a shape and
each one's settings; `along` a `Target`, the first shape it names). `TextSim` keeps only the pose (Spin; no wall yet,
`affects_physics` only for spin). `text_now(scene, params, world, id, time)` (`TextNow`: pose, layout, placed letters,
font problem; `bounds`, `to_world`, `to_own`) is the one way the draw and the editor see a Text. `TextDraw` writes the
Look's records once (like a shape, one set across the whole text) and pushes one `Frame::push_letter` a letter: a
`PathInstance` whose points hold every contour of the letter, a contour's first point flagged z = `CONTOUR_START` 2
and its last z = `CONTOUR_END` 1, w = its length − 1 (`path_signed` in paths.wgsl and `shape_at` in
shape_layers.wgsl wrap within the contour; old shapes keep z = w = 0 and draw byte-identically, pinned by
`paths_tests::shapes_draw_as_before`), held to its box by a 4-corner `clip`, `c[3]` the whole text's reach so gradients
run across the text. Six looks (`looks.rs`: `add_text`; Hook title, Spinning word ring, Neon sign, Glass letters,
Shadow lettering (adds a Point light), Flowing gradient; the lit ones give the scene a Lighting card). Editor: the Add
thing menu's Text heading ("Empty text", a Looks submenu), a tree icon (a T), the own card titled "Words" with a
multi-line Words box (unit `lines`) and the **font picker** (unit `font`, `font_picker.rs`: search, Loaded fonts with
Remove, the groups with each name drawn in its own font through `style::ensure_text_fonts`/`text_family`, Load
font… leaving a `FontRequest` the inspector turns into `Edit::LoadFont`/`RemoveFont`; a non-font is refused with
`NOT_A_FONT`), the Body's hint, no particles layers; in the preview `preview/text_handles.rs` (grab by the letters'
box, the Size dot, the Wrap bar, the Radius ring, all through `set_or_key`) and `preview/text_typing.rs`
(double-click: a box over the text, its words selected, every key shown at once, one undo step a session through
`Studio.typing`, ended by Escape or a click away). Tests: `text/` unit tests, `bpe-gfx/src/paths_tests.rs`,
`bpe-kit/tests/text_pictures.rs`, `bpe-cli/tests/export.rs::a_scene_with_text_exports_a_frame`. **Installing:** the exe
needs `assets/fonts/text/` copied to a `fonts` folder beside it (else every Text shows in the fallback and says so).

Part 2 is in (plan `docs/superpowers/plans/text/part2.md`, "As built" at its end): **live words**. Values in braces
(`text/live.rs`: `parse` into `Piece`s, `Token` hits / balls left / time / countdown / beat / bar / counter / health /
ball number / ball name / standings, each optionally `of <thing name>`, a `|` `Format` (leading zeros, decimals, a clock
`m:ss`); `fill(pieces, LiveCx)` reads them at the moment drawn, unreadable as `-`; `BallNames::from_colours` names balls
by the nearest of 13 colour names of `ball_colors`; **standings** rank on a grid of Re-sort time `resort`, each line
gliding from its old place (`Filled.moves` by `Letter.paragraph`), the past from the balls' history, which `TextSim::
history`/`after_build` keep 2 s of when the words hold standings). **Show** group (`things/text/show.rs`: Show All at
once / Letter / Word / Line by line, Appear and Disappear None / Pop / Fade / Slide / Drop / Typewriter, Takes, Timed by
Seconds / Beats / Hits (Hits of), Starts at, Each takes, Every, Stays for; `appear_times`, `letter_move` → a
`LetterMove` per letter, its opacity in `clip[2]`). Words keys hold. **Set text** rule block `do.text`
(`rules/text.rs`: Replace / Add a line / Show the next line / Clear → `Action::Text`/`TextOp`, kept in
`RulesState.texts` as `RuleText`; Show the next line shows line n alone from `nexts[n−1]`). **Riding**: a Text inside
a Balls, Emitter or Shape thing sits on it (Riding group: Rides on Each ball / The middle / Ball number), its Position
the offset; `text_copies(&TextAt)` gives one `TextNow` a ball (`TextNow.ball`, `shown`), `text_now` the first. **Fast
letters** (Words group, Automatic for riding each ball or standings / Always / Never): a letter is `PathInstance`
kind 3 (`bpe_gfx::PATH_GLYPH`, `Frame::push_glyph`) reading a glyph distance tile (`text/tiles.rs::tile_for`, 128²
like a brush tile, cap height `TILE_CAP` 100 px filling `TILE_FILL` of it) from the frame's mask atlas
(`Frame::mask_tile`; `glyph_signed` in paths.wgsl, the `b.y > 2.5` branch of `shape_at`). Glyph outlines and layouts
are cached for the process (`layout.rs::cached_outline`, `MOST_LAYOUTS`). Looks Hit counter, Countdown timer, Lyrics on
the beat, Labels on balls (inside the picked or first group), Race leaderboard; Hook title pops in word by word.
Editor: the words box (`words_box.rs`: values on `style::TOKEN_BG` chips, **Insert value** menu, `TargetChoices.
counters`), typing keys at the playhead through `set_or_key`, handles over every copy (`text_handles::copies_of`).

Part 3 is in (plan `docs/superpowers/plans/text/part3.md`, "As built" at its end): **real letters**. A Text set in the
world that rides nothing (`sim.rs::can_be_a_wall`) is a wall of its letters: the Words card's **Balls** group
(`TextBalls` Pass through / Bounce off / Bounce inside, Thickness) makes `Outline::Letters { center, angle, letters:
Arc<LetterWalls>, thickness }` (`bpe-runtime/src/letters.rs`: `LetterWall` a letter's contours and box, even-odd
`inside`, box-skipping `nearest`; solid for Bounce off, a band for Bounce inside, `passes` for Pass through), laid out
from `text_now`'s first copy (the scene through `World.tree`, set when a sim's `reads_tree`; Show is drawing only),
rebuilt only when the layout, places or broken set change (`TextState`, `WallLetters`). Each letter is a **part**
(`World.parts`, `bpe-runtime/src/parts.rs`: alive, middles): `Contact.part` names the letter touched, the Hit's
`b.item` is it (0 for every other wall, so goldens hold); `do.break` with a row on a thing with parts breaks one
letter (it leaves the wall; remains `(text, k)`), `do.flash` flashes one or every live letter, Health keeps a row a
letter (`rules/health.rs::rows_of`), `who.rs` names letter n (`is_part`); `breaks::shows_looks` takes text.
Only a walled Text's Words/Path/Balls settings are physics (`ThingSim::affects_physics_in`, `walled_in`; Spin always), and `fingerprint_more` hashes its scene font's bytes (and the things' names when its words hold values). Drawing (`text/draw.rs`): each
letter its own hit data (`letter_hits`), Bounce inside letters as bands (`b.x` half the Thickness, `band_half`), a
broken letter its look (Fade out/Pop through its `LetterMove`, Shatter as `remains::draw::shatter_letter` pieces cut
round its middle; nothing after). Sound: the Sound card fits a Text; `Plays::ByLetter` (`WhoPick { who:
Who::Letter }`) and `Plays::SpellNotes` (`SpellPick`: A–G in the octave of the slot's lowest note, C5 when empty;
`PickCx.letter/letter_char`, `things::text::letter_char`); By side reads sides only on a shape. Lighting: walls'
readers take every contour (`light::draw::outline_contours`: lasers, the probe (`LOOKED` has text), swarm segments,
the lowest shape), glass Texts push benders (`TextDraw::light`). Looks Balls inside a word, Bounce off letters, Pour
in and fill, Letters break apart, Letters that play notes (`looks.rs`: `fitted` keeps the words within `FIT_WIDTH`
900 px, `letters_of` → `walls::inner_points`/`middles`, the balls added beside, falling ones with Gravity 1). Editor:
`TargetChoices.letter_things` ("letter N" in the who picker), the Sound chip for a Text, Texts in the Follow sound
menu and the compressor's shape picker. Goldens `tests/golden-text*.txt` (`scenes::text_scenes`); tests
`text/sim_tests.rs`, `tests/text_letters.rs`, `bpe-sound/tests/text_sound.rs`.

Part 4 is in (plan `docs/superpowers/plans/text/part4.md`, "As built" at its end): **letter moves**, drawing only.
`look.shape` gains Text-only Reaction layers (`LayerKind::fits(thing_kind)`, `modules/look/shape/moves.rs`:
`text.move.wave|beat|hit|flicker|sound|shake|spin|scatter|draw`), folded into each letter's `LetterMove` (now with
turn, stretch and a Draw on `Trace`) by `things/text/moves.rs::apply` in list order; Follow sound reads through
`DrawCx.sound`/`TextAt.sound`; Draw on traces outlines (`traced`, `DrawPlan`) as bands of half the Pen width and
gives the **pen** (`moves::pen`), which a light inside the Text rides (`RidesOn::ThePen`) or a laser aims at
(`Points::AtThePen`). Layers on a Text take an `only` of letter numbers (`ThingKind::layers_by_number`) and Colours,
Outline and Glow an **Across** Whole text / Each letter / Each line (`modules/look/shape/units.rs`: `unit_records`
copies the records a unit, `lay_over` re-lays the gradient, `unworn`; Round reads a record's own middle `place.zw`
when `p0.z` is set). Balls' Word and Particles' Form a word can be **made of a Text thing** (`things/text/shape.rs`:
`text_named`, `letters_of`, `letters_hash`; `reads_tree` + `fingerprint_more`). Looks Word made of balls, Particle
word, Laser writing, Text follows sound; Neon sign flickers. Editor: Letter moves list, row menu Only some letters….

Part 5 is in (plan `docs/superpowers/plans/text/part5.md`, "As built" at its end): **experimental effects**, drawing
only. Text-only layers `text.fx.echo|mirror|split|glitch|melt` in `look.shape`'s Reaction section
(`modules/look/shape/effects.rs`, `EFFECTS`, `is_effect`; the moves' `MoveKind`), listed by the editor under
**Experimental** apart from Letter moves (`list_header_where`). `things/text/fx.rs`: `Fx::of`; **Melt** cuts each
letter's contours finer and runs them down the picture by a seeded column drip (`MeltNow`, `melt_contours`, Starts
again every with a fade) before they are pushed; **Echo**, **Mirror** (ways, round the text's/picture's middle or a
point, Mirrored copies flip their points, Cut to wedges clips), **Colour split** and **Glitch** (screen bands torn
sideways, cut through the instances' convex clips: `clip_to`) work on the Text's pushed instances (`apply`, depth
sorted, coverings before added light, `MOST_FX_INSTANCES`/`MOST_FX_POINTS`), Colour split and Glitch harder after
a hit of Hits of (`hit_age`). Colours go through **`PathStyle`** in the spare `PathInstance.clip[3]` (channel red
light / green covering / blue light, hue turn, colourise; `paths.wgsl::path_styled`, only when non-zero, so shapes
draw as before). Melt or a flipping Mirror turns fast letters off. Looks Glitch, Melting text, Kaleidoscope words,
Echo stack (24 looks). Tests `things/text/fx_tests.rs`, `paths_tests` (style), `tests/text_pictures.rs`.
