# Small links between features (W12): plan

Spec: `docs/superpowers/specs/2026-11-08-feature-links-design.md`. One pull request, built in the order below. Every
step keeps `cargo clippy --workspace --all-targets -- -D warnings` clean. No ball moves: the goldens (debug and
release) must come out unchanged; if one changes, something is wrong.

## Things not to do

- Don't change any setting key or the save format (format 12 stays). New settings get defaults that draw as before.
- Don't change how balls, walls or the log are stepped. Moves on shapes, aims and flashes are drawing only.
- Don't make lights or rigs physics things (no sim). Don't make the camera's follow change what it follows by default.
- Don't put letters in the effects' list or Ripples except when the frame already asks (effects in the scene /
  `frame.ripples`), so unlit, effect-free pictures stay byte-identical.

## Step 1. G2: Points: At a thing (`bpe-kit/src/things/light/`)

- `mod.rs`: `Points::AtAThing` ("At a thing", help: "At a thing, or one of its balls by number."). `LightSettings`
  gains `aim_at: Target` (label "Aims at", `when = "kind!=point and points=at_a_thing"`, group Light, default
  `Target::Kind("balls")`) and `aim_ball: u32` (label "Ball number", same when, `range(0, 1e9)`, `span(0, 100)`,
  default 1; help "0 aims at the middle of the balls").
- New `bpe-kit/src/things/point.rs` (`pub mod point` in `things/mod.rs`): `pub fn point_of(world, id, ball: u32) ->
  Option<Vec2>` (bodies: row of number `ball - 1` if alive, else (ball 0 or missing) the middle of live balls, `None`
  with none alive; walls: `light::draw::outline_centre`; else `world.poses`).
- `draw.rs`: `pub fn aim_at(world, s, me) -> Option<Vec2>`: `None` unless `points == AtAThing` and kind isn't Point;
  the first `world.matching(&s.aim_at, me)` other than `me` with a `point_of`. `places()` sets `aim` on every place
  from it. `aim_angle`: `AtAThing` reads `place.aim` like `AtThePen`. The beam length stays cut only for `AtThePen`.
- Editor `bpe-studio/src/preview/light_handles.rs`: `lamp()` fills each place's aim through `aim_at`; `aims()` is
  false for `AtThePen` and `AtAThing`.
- Tests (`things/light/draw.rs` tests): a spot at (0,0) aiming at a ball at (100,100) points at 45°; ball number 2
  picks the second ball; 0 the middle; a missing target keeps Direction; `points` menu offers "At a thing".

## Step 2. G3: moves on shapes

- `modules/look/shape/moves.rs`: `MoveKind` gains `shapes: bool`; `fits` is Text, or Shape when `shapes`. True for
  wave, beat, hit, flicker, sound, shake, spin, scatter; false for draw-on; `effects.rs` kinds false. Help texts say
  "the letters (or the shape)" where they name letters.
- `things/text/moves.rs`: split the per-letter maths out of `apply` into `fn effect_of(...) -> Option<Effect>` over a
  new `MoveCx { tree, world, log, sound, id, time }` (built from `TextAt` by `MoveCx::of_text`). `hits_for` takes the
  `MoveCx`, the copy's ball and the letter count. `follows` takes the `MoveCx`. `pub struct ShapeMove { offset, turn,
  scale, stretch, opacity }` and `pub fn shape_move(cx: &MoveCx, moves: &[(Move, Vec<u32>)]) -> Option<ShapeMove>`
  (letter 0, `only` ignored, Draw on skipped; `None` when no move does anything). Text output must not change (its
  tests and pictures pin it).
- `things/shape/draw.rs::draw_part`: after `wall_form`, read `moves_of(look)` (only when the look has reaction layers)
  and `shape_move`; `fn moved_form(form, m) -> WallForm`: centre + offset, turn + turn, scale × size, half × size,
  stretch on points (shape units) or ellipse axes (a circle becomes an ellipse; the larger axis rescaled to 1 into
  `scale`). Ripples use the moved form. After the Break look: `clip[2] = 1 − (1 − clip[2]) × opacity` (not on a
  shatter).
- Editor `look_editor/mod.rs`: for a shape that isn't a Text, a "Moves" list (`list_header_where` on
  `LayerSection::Reaction`, `!is_effect`), with "None yet: add one to make the shape wave, jump or shake.".
- Tests: `fits` (shape yes for the eight, no for draw-on and effects); `shape_move` of a Wave at a known time gives
  the text letter-0 offset; a shape with Jump on hit hops after a hit on it (`shape_move` reads the log); picture test
  in `bpe-kit/tests/` (or the shape draw tests): a waving shape's path centre moves and its wall doesn't; a Flicker at
  0 sets the path's fade; a Look with no moves pushes the same path as before (byte-equal instance).

## Step 3. G5: camera follows Particles, Lights, Rigs; Fit all sees outlines

- `bpe-runtime/src/plugins.rs`: `ThingSim::keeps(&self, scene: &Scene, me: &Thing, other: ThingId) -> bool` (default
  false): "this thing reads `other` while it steps, so a thing stepped beside the physics must step in it".
  `world.rs::is_beside`: also false when any enabled thing's sim keeps it, or (G7) a setting follows its sound.
- `things/camera/sim.rs`: `CameraSim::keeps`: `CameraSettings::from_params(&me.params)`: follow on and
  `scene.target_matches(&follow_target, me.id, other)`. `reads_tree`: follow on. `fingerprint_more`: with follow on,
  a hash of every followed Light's (and every followed Rig's fixtures') params, parent and keyed Position track.
- `follow_point`: per followed id: bodies as now; a wall: middle as now (its pose), bounds from its outline's points
  (`light::draw::outline_contours`) widened by half its thickness; a Particles state (`ParticlesState`, Exact) once at
  its live specks' mean, bounds round every speck ± size; a Light (kind in `world.kind_of`) through `world.tree`:
  `LightSettings::from_params(own params)`, Position from its track at `world.time()` when keyed, parent
  `rig::fixtures::ridden`, places by `light::draw::places_in`, each lamp a point; a Rig: its fixtures' lamps. Other
  posed things as now.
- Tests (`camera/sim.rs`): follow a Particles thing (kept in main: `World.beside` lacks it; camera converges on its
  particles); follow a Light at (500, 300) → converges there; a Rig of two lights → their middle; Fit all of one wide
  shape zooms out to show its outline (before: zoom unchanged); `is_beside` false for a followed Particles thing and
  true when the camera follows Balls.

## Step 4. G6 and G7: Follow light / Follow sound menus

- `bpe-studio/src/follow.rs`: `lit_parts` gives Text the four "things" parts. `menu()` takes Particles things with a
  voice that is on; comment updated; buses whenever `shapes` (now any voice) isn't empty.
- `bpe-runtime/src/world.rs::is_beside`: a beside thing is kept when `scene.pulses` has a Sound pulse whose `shape`
  is it, or a `Bus(_)` source while the thing is heard (`ThingSim::heard`).
- `bpe-sound/src/control.rs::Shape::sync`: events `crate::hits::plays(&e.kind)` (adds moments).
- Tests: `follow.rs` menu tests (Text in lit menu; Particles in sound menu; buses with only Particles);
  `bpe-sound/tests/follow.rs` or `particle_sound.rs`: a Particles thing's Level read by `ControlSampler` is above 0
  just after a moment and matches the engine's voice time like the shape case; runtime test: `main_part` keeps a
  Particles thing whose Level a setting follows.

## Step 5. G9: Particles fade, lights flash

- `rules/breaks.rs::shows_looks`: add Particles. `Break`'s `BlockSim::keeps`: who names the thing (Thing id, Kind,
  Tag, Anything for a Particles kind). Help text unchanged.
- Particles draw (`things/particles/draw.rs`): a broken thing's `remains::whole_now(cx)` alpha multiplies every sprite's
  opacity (and stops drawing at 0).
- `things/light/draw.rs::flare`: also adds `EventKind::Flash` events whose `a.thing` is the light or one of its rigs
  (`rig::fixtures::rigs_of`), weight `(speed / FULL_FLASH_SPEED) × (1 − age)²`, gain `max(flare, 0)` or 1 when Flare is
  0; `push_light` multiplies strength by `1 + flare × hits + gain × flashes`. `rules/flash.rs` help: "…; a light or
  rig flares.". `FULL_FLASH_SPEED` public from `bpe_runtime::rules` if not already.
- Tests: `breaks.rs` shows_looks(particles); a Particles thing broken with Fade out draws half-faded sprites half way
  through (draw test with a world whose rules hold the remains); a Flash on a light at t raises the pushed
  `LightGpu` colour by about 2× at t and back by flare length; on a rig, every fixture.

## Step 6. G12: letters for effects and Ripples

- `bpe-gfx/src/frame.rs`: `push_ripple_shape(centre, contours: &[Vec<[f32; 2]>], half)` (one owner, every contour,
  `MOST_RIPPLE_SEGMENTS` across them); `push_ripple_outline` calls it.
- `things/text/draw.rs::draw_part` (part without OVER): when `frame.ripples`, each shown letter's outline (world points;
  a screen Text's through `camera.to_world`) as one ripple shape; when `effects::draw::scene_has_effects`, a
  `BallMark` per shown letter (middle, half the box diagonal × size, the Text's colour `surface::thing_colour`, seed
  from (thing, letter), hit age from the letter's own hits or the Text's).
- Tests: a frame with ripples gets sources owned per letter; a scene with effects lists one mark per letter.

## Step 7. G13 and G14: names, picker words, units, launch, fonts

- Labels to "Hits from": `modules/lighting.rs` scene_heard, `look/shape/moves.rs` hits_of, `look/shape/effects.rs`
  ×2, `health/mod.rs` from, `sound/voice.rs` play_heard_from (+ `sound_tab/playing.rs` label), particles `mod.rs`
  who_hits and `more_forces.rs` who_hits, `text/mod.rs` hits_of, `rig/mod.rs` heard_from, `light/mod.rs` flare_from.
  `sound/mods.rs` Distance/Light heard_from → "Which balls" (+ `sound_tab/mods/{distance,light}.rs`). Tests that look
  for the old labels follow.
- `widgets.rs::target_picker`: sub-menus "Thing", "Tagged", "Any" (as `who_picker`); `target_words` for a kind says
  "Any <kind>" as `who_words` does.
- Units: "deg/s" → "°/s" (`shape/mod.rs`, `text/mod.rs`, `look/shape/moves.rs`, `paint/inside.rs`), "deg" → "°"
  (`look/basic.rs`, `layers/reactions.rs`, `sound/voice.rs`, `sound/mods.rs`).
- Launch: `widgets.rs` Vec2 widget with unit "px/s": two DragValues "speed" (px/s, ≥ 0) and "angle" (°), converting
  to x/y on change only (an untouched pair keeps its exact x/y); `value_words` for such a pair: "500 px/s at −34°".
- Fonts: `balls/mod.rs` `word_font: String` (label "Font", unit "font", when "group and shape=word and
  made_of=typed", default `text::fonts::FALLBACK_NAME`); `balls/shapes.rs` letter outlines from it (the fallback name
  keeps `FONT`); balls sim `reads_tree` and `fingerprint_more` include it when not the fallback. Particles
  `mod.rs` `font: String` (when "from=a_word_or_picture and word_from=text"), `spots.rs::word_spots` takes the font
  bytes (fallback name keeps Weight's Plex files; another font is found with Weight's number 400/500/600);
  `more_fingerprint` hashes its id when not the fallback.
- Tests: no unit is "deg"/"deg/s" (a registry walk); px/s pair round trip (300, −200) → speed/angle → x/y within
  1e-3; balls Word in Anton (when the fonts folder is there) gives different spots than Plex, and the fallback name
  gives the same spots as before (byte-equal).

## Step 8. Verify

- `cargo fmt --all`, clippy, `cargo test --workspace -- --test-threads=2` (background), release goldens
  (`cargo test --release -p bpe-kit golden` and `-p bpe-sound golden`): all unchanged.
- Full-size look check on the changed pictures (`LOOK_ONLY` text and shape scenes; add a "shape moves" / "aimed light"
  scene to `full_size_look.rs`): look for jagged edges, seams and anything odd.
- One Opus review of the whole diff; fix; notes in `docs/notes/{lighting,text,rules,particles,sound,editor,engine}.md`
  ("as built", W12); PR; CI green; merge.

## As built

Built as planned, in one pull request. Differences and additions:

- G2: the second setting is labelled **Aims at ball** (a "Ball number" label was already on the card). A Flash on a
  Rig flares each of its lights (`rig::fixtures::rigs_of`).
- G3: the moves were refactored into `MoveCx` / `Prepared` / `effect_of` so Text and Shape share them;
  `MoveKind.shapes` says which fit a shape.
- G5: the camera reads Lights and Rigs from `world.tree` (`reads_tree`) and hashes them in `fingerprint_more`; the
  new `ThingSim::keeps` hook is how it (and the Break rule, through `BlockSim::keeps`) keeps a Particles thing in the
  main world.
- G12: a screen Text's ripple sources and marks go to the world in `Frame::lift_overlay_extras` (the overlay now
  gets the frame's `ripples` flag), which also lifts a screen shape's ripple outline.
- G14: the Balls' word font is read at SemiBold (600) and spells through `shapes::typed_font`/`word_in`; Particles'
  through `SpotsKey::Word.font` (`WordFont`). Both restart on a font change via `fingerprint_more`.
- The full-size look check has a new "links" group (`links-shape-moves`, `links-aimed-spot`, `links-text-ripples`,
  `links-word-fonts`).
- As-built notes: `docs/notes/{lighting,text,rules,particles,sound,editor,engine}.md`.
