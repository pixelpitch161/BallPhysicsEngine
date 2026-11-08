# Engine crates in detail

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

## Crate map

Dependencies go bottom to top:

- `bpe-settings-derive`: `#[derive(Settings, Choice)]` turns a plain struct into a settings schema. Each field's
  `#[setting(...)]` attribute (label, range, `span`, unit, `simple`, `keyframe`, `pulse`, `when`) drives the
  inspector UI, keyframes and pulses at once. `span(a, b)` is a slider's usual span when there is no hard `range`.
- `bpe-core`: the data model. Things sit in a tree (`scene.rs`), each with its own params and its own cards in
  `slots`. Also values, colours/gradients, blend modes, animation (keyframes and pulses on hits), save/load
  (`save.rs`), edits (`edit.rs`: `add_slot`, `remove_slot`, `set_slot_kind`) and **resolve** (`resolve.rs`: each
  card's defaults under the thing's own values). A setting's `Address` is `{ thing, slot, key }`.
  `registry.rs` holds the plugin registry. `ModuleKind` is the trait for a card kind (the name is historical).
  A card can hold **layers** (`layers.rs`, `Slot.layers`): each `Layer { id, kind, name, enabled, only }` is of a
  `LayerKind` whose `LayerSection` is Drawn, Reaction or Group, kept in that order in the Vec (index 0 is the base
  layer, which can't move, hide or go; group layers sit at the end and move only among themselves). `only` lists
  the ball numbers of a group that wear a drawn layer (empty = every ball; `set_layer_only`). `copy_look` leaves
  group layers out and `paste_look` keeps the target's own. `Value::Points` holds a list of points. A layer's settings
  live in the card's params under layered keys `"<layer id>/<key>"` (`layered_key`, `split_layered`); edits are
  `add_layer`, `remove_layer`, `move_layer`, `duplicate_layer`, `rename_layer`, `set_layer_enabled`,
  `copy_layer`/`paste_layer` and `copy_look`/`paste_look`. A **sub-layer** is a layer too (section
  `LayerSection::Sub`) whose `Layer.parent` is the drawn layer it changes; it sits right after its parent in
  `Slot.layers` (one level, at most `MAX_SUBS` 12 a layer, `takes_subs()`/`takes_sub()` on the kinds), and moving,
  duplicating, copying or deleting a layer carries its sub-layers (`add_sub_layer`). Ten kinds, `sub.*` on balls and
  `shape.sub.*` on shapes (`layers/subs.rs`, `layers/cut.rs`, `shape/subs.rs`): Cut (the old Mask: From Layer
  below / Layer / Shape / Brush / Light / Dark), Glow, Outline, Shadow, Shading, Shine, Shader, Recolour, Flash on
  hit and Colour on hit, worked in order over the layer alone. Format 8 masks become cuts on load
  (`Registry::add_upgrade(9, masks_to_cuts)`). A layer kind can
  add settings from its own values (`LayerKind::dynamic_defs`, read through `layer_def`): a Shader layer's own code.
  Brush strokes are a value, `Value::Strokes` (points in ball space, never keyframed). A shape's moulded outline is
  `Value::Path(ShapePath)` (`shape_path.rs`: corners in shape units, x right, y down, middle at 0, size 1, each side
  straight or a cubic with two handles; `even(n)`, `circle()`, `add_point`, `delete_point`, `create_curve`, mirror
  partners). `Slot.used_ids` keeps a removed layer's id from being given out again (`next_layer_id`). Scene files are format 12 (12 added rules and 11 the Lighting card, no upgrade; formats 7 to 9 still load and are upgraded; format 9 Trail and Particles cards become
  Look layers with their keyframes, `add_upgrade(10, cards_to_layers)`). **Around layers** (`LayerSection::Around`)
  sit after the drawn layers and before the reactions: a ball's `layer.trail` and `layer.particles`, a shape's
  `shape.particles`; they take the sub-layers Glow, Outline, Shadow, Recolour, Flash on hit and Colour on hit
  (`AROUND_SUBS`), and move only among themselves. Sound data: `SlotId::Sound` and
  `SlotId::Mixer` (last in `SlotId::ALL`, `in_card_list()` false: the inspector doesn't list them), layer sections
  `Chain`/`Modulator`/`Effect` (`Layer.lane` puts chain and effect layers in lanes; `move_layer` keeps them in
  their lane; `add_layer_in`), `Value::Routes` (`ModRoute`, key `ROUTES_KEY`; removing a modulator drops its
  routes), `Value::Slots` (`NoteSlot`, key `SLOTS_KEY`) and `Value::Curve` (`CurvePoint { x, y, bend }`, an LFO's drawn
  shape, at most `MAX_CURVE_POINTS` 128, `curve_is_valid`) in `sound_values.rs`, and `PulseSource::Sound`
  (`SoundSource::Modulator/Level/Bus`): Follow sound. `Inputs { hits, sound }` hands pulses a `SoundLevels`; a
  Sound pulse may go on any keyframe setting (`set_pulse`), a Hits pulse only on `pulse` ones.
- `bpe-runtime`: the world, the fixed 240 Hz staged step loop, the shared collision pass (`pairs.rs`, `walls.rs`),
  the camera, the event log (`events.rs`) and playback/snapshots. `history.rs` keeps each ball's past positions
  at 60 a second (and how far it has `travelled`) for as long as its trails and particles ask (`HistoryWant`:
  `seconds`, at least 2, or `forever` for a Line that stays; `around_history` in the kit works it out); a ball's
  past starts at the step it was `born` (no padding before its spawn). Between samples it keeps each ball's
  bounce **corners** (`History::corner`: where its centre touched on the way in, noted in the wall pass, and where
  the substep left it, noted after each substep from the log's hits, `World::note_corners`); `trace` merges them
  with the samples (`trace_after` oldest first from a moment, `item_at`, `first`) and `place` reads through them, so trails (`Path.corner`) and particles turn where the ball
  really bounced instead of cutting across between two samples. `World::build` starts from the settings as
  keyed at 0 s. A touch is resting, not a hit, below `Limits::resting_along(normal)` (the scene's gravity, or more
  along a ball's own pull, `Limits.press`). `step_at` turns a frame time into its step (editor and export agree).
  `Bodies.number` is each row's ball number (`row_of`; riding an Item uses it). `Bodies.held` keeps a row still
  (not moved, bounced or paired, its speed kept at 0: an emitter holding a caught ball before sending it back). Events `Spawn` (tag 8:
  an emitter fired a ball, `b` = (emitter, round)) and `Enter` (tag 9: a ball came into a catcher, `b` = (catcher,
  its round)) sit beside Hit and Remove; every log reader filters by kind. `pairs::pop` is public. `World.fingerprint` is
  the physics fingerprint worked out at build when a sim asks (`ThingSim::needs_fingerprint`); `World::lone(keep)` copies
  the world with only the kept things, no history; `World::lend_cards` puts one thing's Motion/Bounce on another. `sound_fingerprint` is the
  physics fingerprint without the history part (the editor rebuilds the sound only when it changes). The generic `ThingSim` hooks (spawn, forces,
  settle, after_step) stay for new thing kinds even where no current kind uses them. Walls are **two-sided**:
  `keep_clear` keeps a ball on the side its centre was on at the start of the step (`Bodies.previous`); a hollow
  `Outline::Ring`/`Box` with the ball inside keeps the exact old maths (goldens), `Outline::Path` is a free outline
  (even-odd inside), `Outline::Ellipse` is exact (`nearest_on_ellipse`: Eberly's bisection in f64), and `Wall.solid`
  makes a filled shape that balls bounce off from outside only. `Touch` (a Bounce module's `ModuleSim::touch`, the
  Bounce card's "Touch walls with"): Edge, or Middle, where the ball's centre bounces on the outline's centre line
  (`MIDDLE_GAP` 0.01 px inside, size and band left out), at the exact crossing (`middle_crossing`) and carries on for
  the rest of the substep, so balls from one focus of an ellipse meet at the other within 0.01 px. Follow sound
  (`sound.rs`): `SoundAt` (a scene's sound levels from the world and log), `SoundMaker` in `Sims.sound` (a world
  whose physics settings follow sound gets one at build and reads it at the previous step's end in `animate`),
  `LevelsAt` adapts it to core's `SoundLevels`; `fingerprint` then counts only what Follow sound reads (`SoundMaker::fingerprint`, `bpe_sound::control::heard_fingerprint`: tempo, playing/notes, envelopes, modulators, routes, osc on/Gain/Level, Volume, bus, bus on/mute/solo/fader).
- `bpe-gfx`: the renderer. Things draw into a frame (`frame.rs`) and the GPU draws it: gradients, blend, grade,
  shader effects, particles and trails. Trails are strips of `TrailVertex` (`style`: soft edge, band) in
  `Frame.trails` (under the balls) and `Frame.trails_front` (over them); particles are `Particle` sprites
  (`sprites.rs`, `shaders/sprites.wgsl`: a shape code per sprite, soft edge and band, a negative alpha adds light)
  in `Frame.particles`/`particles_front`. A thing whose trails or particles blend other than Normal/Add reports
  them through `ThingDraw::around`/`draw_around` and `build_frame_with` draws each through `draw_routed_at`. Shapes draw through `shaders/paths.wgsl` (`Frame.paths`, `PathInstance` (kind 0 circle, 1 points, 2 an exact
  ellipse from `Frame::push_ellipse`, its half-axes in `b.zw`, nearest point by `ellipse_nearest`),
  `Frame.push_path`: a filled or banded outline from `Frame.path_points`, group 2 binding 4 beside the layer records,
  or a circle with no points). A shape with paint layers names its records in `PathInstance.layers` (first, count,
  program); `shaders/shape_layers.wgsl` works out where a pixel is against the outline (`shape_at`: distance, edge,
  position along the edge) and the `shape_walk.wgsl` template walks the shape records (table on `LayerGpu`: body 0,
  colour and gradient 10, outline 2, glow 4, shader 6, sub-layers 20..29 as balls'). Shader layers in shapes run in the same in-ball programs as
  balls' (`Made.shape`, `effects/in_shape.wgsl`; `sample` dispatches on `in_ball.shape`). Paths and layered balls
  share one pipeline layout.
  Balls draw through `shaders/layers.wgsl`, one uber-shader: a thing writes
  its `LayerGpu` records once a frame (`frame.look_layers`) and one `LayeredInstance` per ball names them. The
  older `BallInstance`/`LookInstance` pipelines are still in the renderer but nothing in the kit draws with them.
  The stack walk is `shaders/layers_walk.wgsl`, a template pasted in by `layers_source`. Records cover Shading (5) and
  Shader (6) layers; a layer's sub-layer records (codes 20..29, table on `LayerGpu`) follow its own (whose `head[3]`
  counts them). The walk is one loop over every record (so each costly function has one call site and compiles
  once): it works a layer out as a `Piece` (`shaders/sub_layers.wgsl`: colour and cover inside/outside, light, its
  edge distance), runs its sub-layers over the piece, then mixes the piece in at the next layer; a cut re-read for a
  shadow uses the light `cut_distance`. What a Glow/Outline/Shadow sub-layer adds fades with its layer's opacity.
  `LayeredInstance.hit` and `PathInstance.hit` carry `[age, strength, hits so far, 0]` (`NO_HIT`) for Flash and
  Colour on hit, filled by the kit only when the stack `needs_hits()`. Brush strokes reach the GPU as 128² Rg8 tiles
  (R cover, G distance to the stroke edge) in a `MaskAtlas` (`masks.rs`, `Frame.mask_tiles`). Shader layers run **inside** the ball: a
  stack with them draws with a generated program (`programs.rs`, `LookProgram`, `Frame.programs` and
  `Frame.layered_runs`) made of `layers.wgsl` plus each effect once, renamed by `effects::prefixed` and given the
  `effects/in_ball.wgsl` prelude, whose `sample()` walks the layers under the shader layer. `in_ball_problem(code)`
  checks own code without a GPU. The `SOLO` record bit makes a layer show its cuts alone (the editor's view).
  Group records sit after a ball's own: 8 the group gradient's colour, 9 a group shader, which runs in the same
  program with uv in the group's square (`LayeredInstance.place`: the ball's start offset and the group's side);
  `layers_group_shaded.wgsl` mixes a group shader into the layers that name it (`amount.z/w` of their records).
  A ball with a Blend layer under other drawn layers draws in two parts (`LayeredInstance.range.z`: the first record
  above the Blend layer, bit 16 for the upper part): the layers under it go through the blend, the ones above paint
  normally over them (`ThingDraw::draw_over`, `Stack::split`). Shapes split the same way (`PathInstance.layers[3]`, `ShapeStack::split`), and the lower part rescales its soft edge by the upper part's cover (`TOP_COVER`, `split_lower_mixed`) so the two parts add up to the whole at the edge. **Z-order**: `Thing.z_order` (-100..100, default 0, in the Placement card) sorts a thing's drawing: every `BlendLayer` has a `z` and a `Place` (Back, Behind, BehindOver, Front, Blend, Over) and `Frame::sort_layers` orders them; z < 0 draws under the scene's back, z > 0 over the balls. A scene **material** (a Things-stage effect on the whole picture, `Chain::material`) paints the main picture once, after the layers drawn before it (z < 0, behind the balls) are mixed in, then each layer drawn after it (`BlendLayer::above`) on that layer before it is mixed in (plain z > 0 layers go through a layer picture then, put on by the composite's plain mode; each layer has its own effect history, `slots` keyed by canvas; the object picture shows only the things of the picture being painted, and the full one again for the Picture stage; in a layer, `sample`/`prev` give straight colour, light with no cover counting by its brightest channel, `bpe_cover`). A record whose own colours are a gradient carries a
  `paint` (pattern 1 straight across the ball, 2 outward), the Body's too when its colours follow Each ball
  (`Follow::EachBall`, the Body's default). In-ball programs live in a `ProgramStore`; the editor shares one between
  its renderers and builds programs on a background thread (`ProgramStore::in_background`,
  `preview::shared_programs`), exports build them while drawing.
  **Lighting** (spec `docs/superpowers/specs/2026-11-06-lighting-design.md`, plans `docs/superpowers/plans/lighting/`;
  `lighting.rs`, `shaders/lighting.wgsl`): a frame with `Frame.light` (`LightScene`) draws the things again in light
  passes picked by `Globals.light.x`: 1 lit colour (colour × Lit), 2 surface (blocks, height, cover), 3 the light
  things give off (only when `LightScene.glows`, `Frame::gives_light`), 4 the background's own light alone. Then a
  jump-flood distance field of blockers, `fs_direct` (each Light thing's light on surfaces and in the air, soft
  shadows), the **light field** (2D radiance cascades: cascade i has probes 2^i apart, 4^(i+1) directions, rays
  marched by a second jump flood on `gi_scene` (opaque where something blocks or gives light) and faded over Glow
  reach; resolved into four ways per texel, `gi_a/b/c`; a texel inside a blocker reads the ways facing out at its
  nearest open texel so nothing lights itself; Bounce is a second run fed by lit colour × light), haze (air light
  × Beam in haze, plus the field) and Light blooms (lamp bulbs + emission, 4 mips), mixed by `fs_mix`
  (`c + (light − 1)·w`, light ≥ 0 per channel). A layer's emission is folded into its record (`LayerGpu.emit`,
  trails' and particles' `emit`); in mode 3 a Shine/Shading/Shader layer that gives nothing leaves the light under
  it, and in mode 1 at Lit 0 it leaves the lit share under it. The **Gives off light** sub-layer
  (`bpe-kit/.../layers/gives_light.rs`, `sub.light`/`shape.sub.light`: Colour from, Strength, Negative, Flare on
  hit) goes on any drawn or around layer and on a Body (the only sub-layer a Body takes); a layer that gives off
  light counts as Lit 0. The scene's background has Gives off light settings (`bg_glow*`).
- `bpe-kit`: **every plugin**, with one folder per thing kind in `src/things/` (balls, camera, folder, scene, shape,
  text, …), the text engine in `src/text/` (fonts, layout, arrange) and one per card kind in `src/modules/` (look, motion, bounce, effects, lighting, light scenes). A ball's Look is
  `look.layers` (`modules/look/layers/`: Body, Pattern, Outline, Shine (its Light is white or its own colours/gradient, `ShineLight`), Glow, Shading, Shader, Blend, the reactions
  Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning, the sub-layers, and the group layers Gradient
  (`layers/group.rs`: a colour per ball across the group, mixed into the body on the CPU or into every/chosen
  layer by the shader's group record, code 8) and Shader (`layers/group_shader.rs`: an effect spread across the
  group, record code 9, over the whole ball and optionally a ring around it, or into the Body/chosen layers);
  `layers/trail/` (settings, `kinds.rs` the path maths, `draw.rs` the six kinds: Streak, Ribbon, Ghosts (drawn
  with the balls, in their own program), Dots, Line, Smoke; Lines are simplified within `LINE_TOLERANCE` 0.15 px
  by a sleeve fit (`kinds::simplify`, `Sleeve`, always keeping a power-of-two grid of samples, `grid_for`), a
  layer's Lines drawn at the finest **level** (2^l times coarser, `pick_level`, `MOST_LEVEL` 8) whose points fit
  `strip_budget` (`MOST_STRIP_POINTS` shared with Shadow/Glow/Outline strips); a staying Line is worked out from
  the history oldest first and **remembered between frames** (`line.rs`: a `Memo` per thing, layer, ball and level,
  checked against the history, rewound to a grid sample after scrubbing back, the same as worked out afresh), so
  a crowd's forever Lines all reach their start (plan `docs/superpowers/plans/trails/staying-lines.md`)) and
  `layers/particles/` (`emit.rs`: emissions found
  again each frame from the log and the balls' history, at most `MAX_PARTICLES` 4000 a layer; `sim.rs`: each
  particle worked out at the moment drawn, closed form or 1/120 s steps for swirl, pull and bounces; `draw.rs`:
  sprites or Mini balls), with `around.rs`/`around_subs.rs` for their sub-layers and `paint/recolour.rs` for
  the CPU colour subs. Nothing is kept between frames (but the self-checking staying-Line memos, which change
  nothing drawn), so scrubbing and export draw the same.
  `gpu.rs` turns a stack into records (a ball wearing only some layers gets them at opacity 0, `records_for`) and has `topmost_at` for clicks on the ball; `paint/brush.rs`
  rasterises strokes to tiles). The **Shape** (`things/shape/`, replacing the old Arena) is an outline balls bounce
  off from both sides: `ShapeSettings` (the `BODY_HEADINGS` settings Start from (Polygon, Circle, Ellipse), Sides,
  Size, Width/Height/Turn for an ellipse, Fill, Thickness and the Corners `path`, plus Spin, Shrink, Smallest size;
  `unit()` is an ellipse's larger half-axis, else Size); `sim.rs::base_outline` picks the exact Ring or Box form for
  circles, even polygons and axis rectangles, `Outline::Ellipse` for an ellipse (a Ring when round) and an
  `Outline::Path` otherwise. `foci.rs::foci` gives an ellipse's two foci (a circle's middle twice) as the wall stands;
  an ellipse is one side for By side and Place goes round from its own top. Its Look is `look.shape`
  (`modules/look/shape/`): the base `shape.body` (plain white) and the paint layers Colour and gradient (Across /
  From the middle / Along the edge, Flow), Outline, Glow, Shader and Blend (whole-shape); `shape/gpu.rs::ShapeStack`
  turns them into records and `things/shape/draw.rs::wall_form` draws every wall form in the shape's own frame. `look.basic`
  fits no thing kind now. Test scenes are still written with `look.basic` on balls and `classic_cards` turns it
  into layers (`layers_from_basic`). Balls (`things/balls/`) are one ball or, with `group` on, a group in a
  `shape` (`shapes.rs`: Grid, Line, Ring, Filled ring (rings of balls from the middle out, `filled_ring`), Filled circle, Triangle, Honeycomb, Spiral, Heart, Scatter, Word from
  the IBM Plex font, Custom points), with Variety (size, speed, colour, turn spread by `seed`), Release (all at
  once, one by one, waves, via `ThingSim::spawn`) and Launch (same, fan, outward, random). A ball's number in the
  group (`sim::numbers`) is its place in the shape, not its row. Test scenes use `BallsSettings::block`/`ring`,
  which keep the old Block/Ring positions exactly. `balls/rows.rs`: `BallRows` (each row's number, size share,
  round, launch direction, place in its group, arrivals, fade start, place round its round) kept by both Balls and
  Emitter states, `group_of` (`GroupOf`: what the shared ball drawing reads), `variety`, `has_balls`; the Balls draw
  (`balls/draw.rs`) draws any thing with ball rows (fades, arrivals as hits for reactions). The **Emitter**
  (`things/emitter/`, family Bouncers): fires balls of its own design (its own Look and Physics) in rounds
  (`fire.rs`: directions All round / Fan / One way / Random, Start from Point / Ring / Line, Gap and Order, round
  sizes ×/+, `layout` one round's places; `sim.rs`: Fire when At start / Every / When a wave lands, Rounds or
  Endless, Earlier rounds Keep / Fade out / Remove, Most balls; catching by a swept circle test: Remove, Send back
  out (Same way back / Out my way, Delay holds), Pass through; Leave trails (Remove only, drawing-only: `BallRows.caught` notes who caught a ball and when,
  `Balls.gone` lets its trail run out into the catch point, `Cut::after_gone`); waves = arrivals of one (owner, round) within 0.5 s,
  `landings`; `round_marks` for the editor) and sits on a focus of a circle or ellipse (`emitter/foci.rs` over `shape::foci`; the Sits on hint says what would spoil meeting at the other focus). **Aim** (part 3, `emitter/aim.rs`): Straight in / Keep my directions, Target
  (`aim_target`, 0 = the other focus it sits on), Arrive after, Can't reach (Skip / Fire anyway). Exact in the world's
  own steps (semi-implicit Euler, one substep a step; a ball launched on step L sets off from `now(L)`): Straight in
  `v0 = Δ/(N·dt) − a·dt·(N+1)/2`, Keep solves the step polyline across then along its direction (`keep_way`); the
  pull is gravity × Gravity × + Wind from the world's Motion card (`World::card`); the target's future place is a
  `Track` (a shape focus exactly through Spin/Shrink, else pose + velocity). `fire_round` plans Keep leaving steps,
  `launch_ball` re-aims at launch (`Launch.arrive`); an aimed round at 0 s fires in `after_build`. Out my way aims
  caught balls. `aim::check` (warnings, `Inspector.aim_checks`) and `next_round`/`path_points` (preview paths).
  **Through bounces** (part 4, plan `docs/superpowers/plans/ball-emitter/part4-test-fired.md`, reworked in part 4b,
  `part4b-together.md`, and 4c, `part4c-all-bounces.md`; `emitter/test_fire.rs`): ways are found by test-firing a
  lone copy (`Lone`: `World::lone` keeps the things without balls and a quiet copy of the emitter,
  `EmitterState::quiet`; Out my way lends the caught ball's cards, `World::lend_cards`) from the moment it would set
  off; `passes` gives the signed miss of the step polyline against the target's track, grouped by arc (bounces
  before); `fly` stops past Bounces at most (`aim_bounces_most`), at rest a second, `QUIET_DISTANCE` (6000 px) without
  touching a wall, or after `aim::MOST_STEPS` (2 min). No ball goes faster than the speed that splits the world's
  steps (`SAFE_MOVE`/dt) nor, as soon as all can, than Speed (`ceiling_of`). Settings: Arrive (As soon as all can /
  After: Arrive after), Bounces at least/most, Leave (Together / When needed), Ball directions (Adjust each ball /
  Keep mine, Leave together only), Turn at most. Keep mine is Adjust each ball with the turn held to `MOST_TWEAK`
  (0.3°). `aim_shots`: **scan** (`scan`): balls setting off from one place form a `Group` and share their flights;
  each ball's window (±Turn at most) is flown at `SCAN_BUDGET`/(balls × speeds) directions (5..17 a ball), to Bounces
  at most, at Speed (Keep mine where paths bend: `KEEP_SPEEDS`; with a time to arrive by: `TIMED_SPEEDS` round what
  gets there); flights keep only their passes; a `Lead` is a flight within `ROUGH` or two neighbouring flights
  (directions, or one direction at neighbouring speeds) on either side of the target on an arc (`brackets`).
  `close_lead` closes one in (Illinois, `CLOSE_FLIGHTS`, flights stopping past its arc) once per lead (`OnceLock`;
  every ball wanting it pays its share of the steps, `Group.sharers`, so answers never hang on threads). `soonest`:
  each ball's leads by guessed time, closed until one holds; Keep mine where paths bend also searches its own speeds
  (`own_ways`, part 4's `Search::ways`) when the scan found it none. T = the latest soonest (or Arrive after). Together:
  `meet` checks up to `CHECK_TRIES` ways slowed or sped by e/T (one flight each: where nothing bends the paths the
  very same way; fewest bounces first, else least change of speed first), then works on the nearest
  `MEET_TRIES` with `solve_turn` (a secant on speed along the ways through the target on one arc, each speed's turn
  found by `through`; a pass far from the nearest try's time is another run of ways, `BRANCH`); a ball is **at** the
  target at T (`off_at`, ≤ `REACH` 1 px, else the nearest within `ROUGH` 3 px), its steps `BALL_STEPS` or
  `MEET_FLIGHTS` flights to T; then `borrow_ways`. When needed: each ball's way nearest Speed that is there by T,
  then part 4's `settle` (each leaves so it arrives at T). The Edge bounce isn't quite the same at every speed
  (the wall's normal is taken where the ball was slid back to), so a way slowed a lot drifts a few px over several
  bounces: Keep mine, which can only turn 0.3°, loses those balls. `aim_round`/`aim_catch` (rounds in parallel, ≤ 8 threads) are cached process-wide in `aim_cache.rs` by
  (`World.fingerprint`, emitter, round or catch); `RoundAim.arrive` is the round's moment (`fire_round` gives it to
  its launches); `EmitterState.aimed` keeps the rounds launching and the latest (`BallAim.path` for the preview). A
  round at 0 s aimed through bounces fires on the first step (not `after_build`). A catching target ends a flight's
  arcs at the one it is entered on. A run under `aim_cache::StopWith` whose flag is set gives up and keeps nothing. Its
  marker, flashes and ripples are `draw.rs` (sprites), worked out from its state at the moment drawn. Group
  gradient `Spread` has Round and Launch angle; the around layer **Wavefront** (`layers/wavefront.rs`) draws a
  strip through each round's balls (`trail::draw::strip_with_subs`). Camera Follow "balls" follows emitters too.
  A folder usually has
  `mod.rs` (settings + kind), `sim.rs` (physics) and `draw.rs`. `lib.rs::kit()` registers them all. `paint/` is the
  colour and gradient system (not the removed canvas). `starters.rs` gives a new thing its starting cards (`add_thing`, `fill_slot`) and
  `scenes.rs` holds the test scenes, compiled only for tests (the `test-scenes` feature, a dev-dependency). They pin
  the old look/motion values with `classic_*` helpers so goldens don't move. Never add built-in or example scenes
  to the program unless the owner asks.
- `bpe-cli`: headless export. With sound it renders the WAV first (into a temp folder) and muxes it as AAC 320 kbps
  48 kHz (`AUDIO_ARGS`); a scene without sound gets a silent track. Frames draw with `build_frame_with` and a
  `ControlSampler` when a setting follows sound. `--stems` and `--midi` (`ExportOptions`)
  save the stems and the MIDI file beside the video.

## One wind (W13)

Balls follow the scene's air: the Motion card's **Follow the air** (`follow_air`, group Air, per s; `BasicMotion::
air_grip`, `ModuleSim::air_grip`/`air_key`) pulls each ball towards the air's velocity at its place,
`v += (air − v)·(1 − e^(−grip·dt))`, after the Motion forces and before the Moths pull (`World::move_bodies`). The
Motion card's Wind is labelled **Steady push** (same key, same maths). The air is the kit's (`bpe-kit/src/air.rs`);
the runtime reaches it through `Sims.air` (`bpe_runtime::AirSim`, the kit's `KitAir`: wake, catch up, velocities).
`bpe_runtime::air_felt(scene, sims)` says whether some enabled thing's Motion card follows the air (set, varied from
0, keyed, pulsed, or the setting of an enabled rule's Change, via `BlockSim::changes`); it is read from the saved
scene, so every world agrees (`World.air_felt`). Then each world wakes the air at build and, each step,
`World::work_out_air` catches it up and reads it once per ball into `World.air_at`; `lone` keeps the ROOT state; the
world senses the full animation (Air keys following sound or light); `fingerprint_of` hashes the Air card and its
keys (and the full physics animation); a Particles thing whose Rise heats the air is not beside
(`ThingSim::heats_air`). Nothing changes while no ball feels the air (no golden moved). A ball that stirs the air
holds back the air round it, so with Balls stir at 1 it reaches about 73 % of the wind in a second
(`air/follow_tests.rs`). The scene sim adds the ambient wind × dt to `Air.drift` each step (what haze With the air
drifts by). `bpe_runtime::air_read` is air felt or a thing that `reads_air` (haze With the air): such worlds sense
the Air keys and the fingerprint counts the Air card. Emitter Aim's test flights (`emitter/test_fire.rs`) have no
air (a known gap). Tests: `bpe-kit/src/air/follow_tests.rs`.
