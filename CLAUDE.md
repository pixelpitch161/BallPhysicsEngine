# Ball Physics Engine: guide for Claude

Ball Studio is a Rust + wgpu editor and renderer for vertical 9:16 bouncing-ball videos (mp4) for
TikTok, Reels and Shorts. The owner is a music producer on Windows 11 who is not a programmer. They run a ready-built
`ball-studio.exe` and never run commands themselves.

## One engine, stripped to its core

On 2026-11-03 the owner had everything except the core removed, to rework the program from a smaller base. The
version before that is **Version 1**, kept on the `version-1` branch (with its exes in the owner's
`bin\version-1\` folder). Removed in that pass: the old engine (`bpe-app`, `bpe-scene`, `bpe-sim`, `bpe-audio`,
`bpe-render`, `bpe-export`, `bpe-music`), node graphs and rules (`bpe-graph`, `bpe-vm`), the Explore and Batch
windows, the canvas and paint marks, the whole sound and chords system (`bpe-synth`, `bpe-mix`), and the Obstacle,
Liquid, Soft body, Swarm, Field, Meter, Emitter and Text things. A new sound system is being built in six parts
(spec `docs/superpowers/specs/2026-11-04-sound-system-design.md`, plans in `docs/superpowers/plans/sound-system/`,
decisions log `decisions.md` there); all six parts are in: 1 (engine, playback, export audio, Master), 2 (shape
voice, Sound tab), 3 (oscillator layer chains), 4 (modulators, drones, Follow sound), 5 (notes, scales, slots) and
6 (buses, console-strip Mixer, bus effects, stems and MIDI export).

Rework step 2 (same day) dropped shared modules: every thing owns its settings. A thing's cards (Look,
Motion, Bounce, Shader) live in `Thing.slots` as `Slot { kind, params, stash }` (balls have no Shader card and shapes none of Motion or Shader: effects on a thing are Shader layers of its Look; only the scene settings keep a Shader card, the picture's Effects; trails and particles are Look layers, and `SlotId::Trail`/`Particles` are kept only so files before format 10 open); nothing is shared,
linked, inherited from a folder or locked, and there is no Library, presets or randomize (dice). Right-clicking a
setting opens Copy Value / Paste Value (…) / Create Keyframe; there are no keyframe diamonds in the inspector.

**Vary across balls** (Ball Emitter part 5, plan `docs/superpowers/plans/ball-emitter/part5-vary.md`): on a Balls
group (or an Emitter) the row menu adds Vary across balls / Stop varying. A varied setting stores `"<key>~to"`,
`"<key>~by"` (`VaryBy`: Number, Round, Launch angle, Place, Random, Speed, Hits) and `"<key>~ease"` beside it, layered
keys keeping their prefix (`bpe-core/src/vary.rs`: `vary_of`, `with_varies`, `BallSpread::t`, `Scene::start_varying`/
`stop_varying`/`vary_use`; `def_at` answers the parts, To keys like any setting). Which settings may vary is the
kinds' say: `ThingKind::vary` (balls: Size live, Launch speed at launch, so no Speed/Hits), `ModuleKind::vary`
(Motion, Bounce), `LayerKind::vary` (ball layers but group layers, Blend and the trail/particle settings the whole
layer shares, `layers/vary.rs::layer_vary`). Places in a spread are quantised to `VARY_LEVELS` (1024) steps, and
everything is shared per step: the runtime's `rows.rs` keeps a prepared Motion/Bounce state per ball
(`ModuleSim::per_ball`, `forces_one`; `Slotted::state_for(row)`), `BallsState` keeps each ball's own size and launch
(`spread_of`), and the draw keeps a stack per step (`layers/vary.rs::VariedLook`, `balls/draw.rs::Worn`; records
keyed by stack and worn layers; trails pick each ball's copy through `Balls.own`, particles work out each copy's
emissions then cap them together). Nothing changes when nothing varies (goldens). The editor's `vary.rs` has
`VaryCx` (on `GridCx.vary`), `VaryEdit`, the two-ended slider and the "Varies by [spread] [easing]" line; a pair,
colour or gradient gets a "to" row; `Edit::Vary` starts/stops; timeline lanes of To read "(to)".

| | |
| --- | --- |
| Editor crate / binary | `bpe-studio` / `ball-studio-next` |
| Command-line renderer | `bpe-cli` / `bpe-render-next` |
| Scenes | none built in: the editor always opens to a blank project (Scene settings + Camera) |
| Design spec | `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (written before the strip-down) |

The shipped `bin\ball-studio.exe` on the owner's PC is built from **`bpe-studio`** (`-p bpe-studio`), even though
its binary is named `ball-studio-next`. Older specs and plans in `docs/superpowers/` describe removed features.

Old scene files don't need to keep loading or looking the same. The owner has said there's no backward
compatibility until they are happy with the software.

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
  partners). `Slot.used_ids` keeps a removed layer's id from being given out again (`next_layer_id`). Scene files are format 10 (formats 7 to 9 still load and are upgraded; format 9 Trail and Particles cards become
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
  with the samples and `place` reads through them, so trails (`Path.corner`) and particles turn where the ball
  really bounced instead of cutting across between two samples. `World::build` starts from the settings as
  keyed at 0 s. A touch is resting, not a hit, below `Limits::resting_along(normal)` (the scene's gravity, or more
  along a ball's own pull, `Limits.press`). `step_at` turns a frame time into its step (editor and export agree).
  `Bodies.number` is each row's ball number (`row_of`; riding an Item uses it). `Bodies.held` keeps a row still
  (not moved, bounced or paired, its speed kept at 0: an emitter holding a caught ball before sending it back). Events `Spawn` (tag 8:
  an emitter fired a ball, `b` = (emitter, round)) and `Enter` (tag 9: a ball came into a catcher, `b` = (catcher,
  its round)) sit beside Hit and Remove; every log reader filters by kind. `pairs::pop` is public. `sound_fingerprint` is the
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
- `bpe-kit`: **every plugin**, with one folder per thing kind in `src/things/` (balls, camera, folder, scene, shape)
  and one per card kind in `src/modules/` (look, motion, bounce, shader). A ball's Look is
  `look.layers` (`modules/look/layers/`: Body, Pattern, Outline, Shine (its Light is white or its own colours/gradient, `ShineLight`), Glow, Shading, Shader, Blend, the reactions
  Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning, the sub-layers, and the group layers Gradient
  (`layers/group.rs`: a colour per ball across the group, mixed into the body on the CPU or into every/chosen
  layer by the shader's group record, code 8) and Shader (`layers/group_shader.rs`: an effect spread across the
  group, record code 9, over the whole ball and optionally a ring around it, or into the Body/chosen layers);
  `layers/trail/` (settings, `kinds.rs` the path maths, `draw.rs` the six kinds: Streak, Ribbon, Ghosts (drawn
  with the balls, in their own program), Dots, Line, Smoke) and `layers/particles/` (`emit.rs`: emissions found
  again each frame from the log and the balls' history, at most `MAX_PARTICLES` 4000 a layer; `sim.rs`: each
  particle worked out at the moment drawn, closed form or 1/120 s steps for swirl, pull and bounces; `draw.rs`:
  sprites or Mini balls), with `around.rs`/`around_subs.rs` for their sub-layers and `paint/recolour.rs` for
  the CPU colour subs. Nothing is kept between frames, so scrubbing and export draw the same.
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
  `landings`; `round_marks` for the editor) and sits on a focus of a circle or ellipse (`emitter/foci.rs` over `shape::foci`; the Sits on hint says what would spoil meeting at the other focus). Its
  marker, flashes and ripples are `draw.rs` (sprites), worked out from its state at the moment drawn. Group
  gradient `Spread` has Round and Launch angle; the around layer **Wavefront** (`layers/wavefront.rs`) draws a
  strip through each round's balls (`trail::draw::strip_with_subs`). Camera Follow "balls" follows emitters too.
  A folder usually has
  `mod.rs` (settings + kind), `sim.rs` (physics) and `draw.rs`. `lib.rs::kit()` registers them all. `paint/` is the
  colour and gradient system (not the removed canvas). `starters.rs` gives a new thing its starting cards (`add_thing`, `fill_slot`) and
  `scenes.rs` holds the test scenes, compiled only for tests (the `test-scenes` feature, a dev-dependency). They pin
  the old look/motion values with `classic_*` helpers so goldens don't move. Never add built-in or example scenes
  to the program unless the owner asks.
- `bpe-dsp`: sound building blocks at 48 kHz in 64-frame blocks (`SAMPLE_RATE`, `BLOCK`): dB helpers, a
  `Smoother`, `FlushGuard` (denormals off), polyphase IIR halfband oversamplers (2/4/8×), a 4× true-peak detector,
  the two-stage lookahead `TruePeakLimiter` (3.4 ms) and an EBU R128 `LoudnessMeter`. For the synth: `warp.rs` (the
  five waves and ten Warp modes, drawn plain; the editor's wave view uses `warp::draw`), `tables.rs` (band-limited
  wavetables: a `TableSet` per wave and Warp mode, `WARP_STEPS` steps × `MIPS` copies of 16384..1024 samples, baked on
  first use into the shared `WaveBank`; `octaves`/`mips_at` pick the copies, crossfading only over the top quarter
  of an octave, `XFADE_FROM`), `osc.rs` (the unison oscillator: up to 16 copies, Detune/Spread/Blend, f64 phase
  between blocks and u32 fixed-point inside one, 4-point Lagrange four samples at a time), `noise.rs` (seeded
  white and pink) and `envelope.rs` (DAHDSR with a curve per segment). For the chain layers: `filter/` (`StereoFilter`:
  Cytomic SVF low/high/band pass, notch, peak and shelves at 12 or 24 dB, a ZDF ladder, a comb and a three-band
  formant, coefficients glided over a block; `response_db` draws the real curve), `shaper.rs` (`Shaper`: the ten
  distortion curves through a `StereoOversampler`, four sub-samples at a time in `f32x4`, first-order antiderivative
  antialiasing on the cornered curves at 2× and up, bias taken back out, a 5 Hz DC blocker and the Tone tilt;
  `transfer` draws the curve) and `pm_delay.rs` (`PmDelay`: FM and PD as a moving Hermite delay of at most 2 ms,
  `am`, `ring`). Bus effects in `fx/`: `eq.rs` (`Eq8`, 8
  bands, `band_response_db` for the editor's curve), `compressor.rs` (`Key::SelfInput/External/Hits`),
  `delay.rs` (Standard/Ping-pong/Multi-tap, Digital/Analog/Tape), `reverb_plate.rs`, `reverb_hall.rs`,
  `reverb_spring.rs` (shared parts in `reverb_common.rs`), `modfx.rs` (Chorus, Phaser, Flanger), `width.rs` and
  `analyser.rs` (256 log bins).
- `bpe-sound`: the sound engine. `Engine` runs its own `Playback` of the physics and renders 64-frame blocks with
  every hit on its exact sample (`Event.at` is where in the step a hit happened, never hashed; `hits.rs`
  `HitCursor`), the voices (`VoiceSource`, built per scene by `SoundPatch::build`; `swap_patch` keeps ringing notes
  via `adopt`) and the `Master` (input, output, true-peak ceiling -1 dBTP, release) from the scene's `sound.mixer`
  card (`bpe-kit/src/sound/mixer.rs`; `ensure_mixer` puts it on every scene). `Engine::seek` goes back
  `PRE_ROLL` (4 s) and renders silently so tails ring. `render` never allocates (a counting-allocator test).
  `TempoMap` reads the scene's Tempo / Beats in a bar / Beat note. `offline.rs`: `render_to_wav` (streamed float
  WAV) and `has_sound`. The `test-tone` feature gives a thing tagged `test-tone` (or `test-tone-decay=<s>`) a test
  blip voice; tests use it, the program never does. **Shape voice** (part 2): a shape's `sound.voice` card
  (`bpe-kit/src/sound/voice.rs`: `VoiceSettings` with Osc A/B as prefixed `OscSettings` (`a_`/`b_` keys), Playing,
  Notes, Output and `routes`; ADSR layers `sound.adsr` in `adsr.rs`; `starter_voice` is what Enable sound adds;
  the chain layers `chain.modulation`, `chain.filter`, `chain.distortion` and `chain.eq` (8 bands like the bus EQ,
  run per sample with `Eq8::tick`) are in `bpe-kit/src/sound/chain.rs`,
  a Modulation's `source` text being "" (the other osc), "a", "b" or a layer id) becomes a `ShapeVoice`
  (`voices/shape.rs`, built by `patch::shape_voice`): hits pass the `HitFilter` (min strength, retrigger, Heard from),
  `NotePicker` picks a note of slot 0, `Voices` (`alloc.rs`) gives it a slot (Poly up to 16, the oldest fading in 5 ms;
  Mono glides). Every number knob has a place in `knobs::layout` (keys read once a block from `ShapeSpec.keyed`),
  `ModMatrix` (`modmatrix.rs`) applies routes as Serum does: an Amount is a share of the knob's own travel from where it is set, clamped at its ends (Hz knobs turn in ratios, `turns_in_ratios`, the same in the engine and the editor's knob and ring); the one exception is an ADSR on a level knob (Gain, Level, Sub level, Volume, `combine_of(key, from_envelope)`), a VCA per sample (`scale_by_routes`), other routes on level knobs adding dB per sample (`offset_by_routes`), the
  oscillator layer chains (part 3) are `chain_graph.rs` (`ChainGraph`: which layer reads which, by lane; a Modulation
  layer's source is an oscillator raw or the sound after any layer; a tap that would close a circle is a loop and
  reads one sample late, the first layer in card order hearing on time; `loops_of` for the editor's loop mark) and
  `chain.rs` (`ChainSpec` built per patch, `VoiceChain` per note: Osc → Gain → layers → Level → Pan, Solo cuts a lane
  at its layer, a tapped oscillator that is off still runs unheard, `adopt` keeps ringing filters by layer id). Chain
  knobs have fixed places (`knobs::chain(place, key)`, 38 keys: 14 shared then 24 EQ band numbers from `EQ_BANDS_AT`, `MAX_CHAIN` 32 layers a card) so keys and routes reach
  them. The Master's Input/Output keys are read every block too (`MasterKeys`). `Engine::audition`, `stop_notes` and
  `render_paused` (stopped: no hits or physics) play
  keys clicked on the Sound tab; `Meters` also carries a level per shape (`write_shape`/`take_shape` by thing id) and the notes it
  sounds as a 128-bit mask (`write_notes`/`take_notes`, from `VoiceSource::sounding_notes`; the piano roll lights them).
  Sound goldens: `tests/golden-sound*.txt` (an FNV hash of the starter patch; `golden-sound-chain*` a patch with
  layers on both oscillators), blessed like the physics goldens. `tests/ab_clips.rs` (ignored) renders the FM/PD A/B
  clips.
  **Notes** (part 5): the key/plays/slot-change settings and their enums are in `bpe-kit/src/sound/notes.rs`
  (`Root`, `Scale`, `Plays`, `Change`, `SlotOrder`, `MAX_SLOTS` 16); `notes_jump` ("Jump to slot") is the one
  `hold` setting (`Flags.hold`: `Scene::set_key` makes its keys `Ease::Hold`). `bpe-sound/src/music/` is pure
  theory: `scale.rs` (`in_scale`, `move_into_key`, never drops a note), `chord_name.rs` (FL names, slash chords),
  `voicing.rs` (`voice_progression`, least movement) and `presets.rs` (`PRESETS`, 40 in 8 `Mood`s, `realise`).
  `voices/pick.rs` holds the play orders (`AnyPick`, pure in the hit step except Random walk; `ChordPick` strums),
  `voices/slots.rs` the `SlotClock` (`begin_block`/`until`/`hit`/`end_block`; tempo and time lines from the
  absolute sample; a change on a hit's own sample comes first) and `slot_changes`/`slot_change_times` for the
  timeline. `Engine::seek` calls `VoiceSource::catch_up` with every hit before the pre-roll so slots and orders
  are exact after a seek (and `swap_patch` does for a voice new mid-clip); `Meters::write_slot`/`shape_slot` report the slot playing. By side uses
  `things::shape::side_on` (no allocation; the outline is made at patch build).
  **Buses** (part 6): the mixer card holds Bus 1..3 (`bus1_…` keys: name, on, fader, pan, mute, solo, send1..3;
  Bus 1 on, 2 and 3 off) and their effects as layers (`bpe-kit/src/sound/fx.rs`, `fx.eq` … `fx.width`, section
  Effect, `Layer.lane` = bus 0..2; `fx.filter`/`fx.distortion` reuse the chain's settings without Key track and Solo). `buses.rs`: `Buses` runs them in send order (post-fader sends; a send that
  would loop is dropped, `would_loop`), solo/mute/on fades, and lines up latency with `Lag` (`lag.rs`); the
  engine takes `latency()` back so hits stay on their samples. A shape's main bus and sends are its
  `ShapeRoute` (`out_bus`, `out_send1..3`). `fx_chain.rs`: `FxChain` per bus (Mix crossfade, bypass fade,
  keyed numbers via `Keyed`, `adopt` on a patch swap). `Meters` has bus, effect and EQ-analyser readings.
  Export extras (`offline.rs` `render_sound` with `Extras`): `stems` (24-bit WAV per bus + Master,
  `stem_paths`) and `midi` (`midi.rs`: `NoteLog` from `Engine::record_notes`, `write_midi`, format 1, PPQ
  960).
  **Modulators** (part 4): kit layers `sound.distance`, `sound.lfo`, `sound.hit` (`bpe-kit/src/sound/mods.rs`,
  `MOD_KINDS`, `add_modulator`, at most `MAX_OF_EACH` 8 of a kind, `turn_on_distance`) beside the ADSRs;
  `mods/` holds `LfoMod`, `DistanceMod` (`distance_value`, Centre/Edge, Nearest/Each note's ball/Average/How
  many) and `HitMod` (`hit_values`: strength, place on edge, side, ball number, random). Shared ones run once a
  block in `ModsSpec.order` (`mods/order.rs`), per-note ones live in each note; every number knob of the card is a
  route target (`patch::routable`). An LFO's shape is a drawn curve (`LfoShape::Drawn`, `curve`, `LfoPreset`;
  `bpe_dsp::lfo::curve_at` with `bend_at`; `lfo_curve.rs` edits it: add/move/delete points, bends, Step/Ramp paint,
  flips) or Random/Smooth random; `once` holds the end; `Meters::mod_position` gives the editor's playhead. `Meters` carries each shape's modulator values and last hits
  (`mod_value`, `recent_hits`). `VoiceSettings.play_when` (`PlayWhen`: On hit / Always / In range of
  `play_zone`) makes drones (`ShapeVoice::hold`: held notes are never retriggered by hits; only notes in
  `ShapeVoice.held` count as drone notes; `BlockCx.playing` is false while stopped, when drones wait and clicked
  keys (gate at most 0.4 s, their own voice beside a drone) sound alone). Switched-off modulators' routes are
  left out at patch build. **Follow sound**: `control.rs` `ControlSampler` (built from
  `patch::shape_specs`, no wavetables) works modulators, `Level` and `Bus(n)` out from the log's accepted hits
  and the world at a moment (`bpe_dsp::level_at` closed-form envelopes) with no audio; `FollowSound` is its
  `SoundMaker` and `bpe_sound::kit()` the kit with it (the editor and the renderer use it, so physics follows
  sound everywhere). `tests/follow.rs` checks it against the engine (`Engine::voice_time`).
- `bpe-cli`: headless export. With sound it renders the WAV first (into a temp folder) and muxes it as AAC 320 kbps
  48 kHz (`AUDIO_ARGS`); a scene without sound gets a silent track. Frames draw with `build_frame_with` and a
  `ControlSampler` when a setting follows sound. `--stems` and `--midi` (`ExportOptions`)
  save the stems and the MIDI file beside the video.
- `bpe-studio`: the egui 0.37 editor. Its look is the Ball Studio Design System (spec
  `docs/superpowers/specs/2026-11-03-design-system-editor-design.md`): `style/` holds every token, the IBM Plex
  fonts (`assets/fonts/`), track colours and drawn controls (switch, track picker, key diamonds). No other editor
  file may make up a colour (a test checks; a real scene colour line ends in `// scene colour`). Every thing but
  the scene settings has a `Thing.colour` (`TrackColour([u8; 3])`, any colour, saved as `"#rrggbb"` (older files with bare names open with new colours); a new thing gets a random readable one far from the hues in use, `Scene::new_colour`; editor only, never read by physics or export; picked with `style::track_picker` from the tree's tag and row menu, the timeline swatch and the inspector header). Its layout has the tree on the left, the inspector on the right, the preview
  in the middle and the timeline at the bottom. Key files are `app.rs`, `inspector.rs`, `preview.rs`, `timeline/`,
  `widgets.rs` (settings grids and the right-click value menu), `palettes.rs` and `export_window.rs`.
  Motion and Bounce show as one **Physics** card (`Card::Physics`, `PHYSICS` in `inspector.rs`: one header, Remove,
  "+ Add slot" entry and jump-bar chip; each card's grid inside); the scene still keeps the two cards.
  A selected shape is moulded in the preview (`shape_handles.rs`: corners, sides, bend handles, a circle's edge,
  Shift mirrors across the shape's middle line, right-click Add point / Create curve / Straighten / Delete point /
  Turn into points); a `look.shape` card shows the Look editor without the ball view or Reactions, and its Body
  shows the shape's `BODY_HEADINGS` settings (changing Sides or Start from removes the path in the same edit).
  A `look.layers` card shows the Look editor (`look_editor/`: the ball view, the Layers and Reactions lists with
  their menus, drag to reorder and rename, a layer's sub-layers in one box under it (`layer_list::piece`: "+
  Sub-layer", fold arrow, drag within the box, row menu; `Edit::AddSubLayer`/`MoveSubLayer`/`PasteSubLayer`,
  `Inspector.folded_pieces`), then the picked layer's settings; the
  copied layer/look live in `Inspector.clipboard`). `layer_rows.rs` draws a cut's From/Layer and a Shader's
  Effect, `code_box.rs` the own-code box, `handles.rs` the handles on the ball view and `brush.rs` painting strokes
  there. A Balls thing's Look card starts with a Ball / Group switch (`Inspector.look_mode`; in the Balls card when there is
  no layered Look card): Group mode
  (`look_editor/group.rs`, `group_view.rs`) shows the whole group where it starts (custom balls, the launch arrow
  and the gradient's line drag there; balls can be picked for layers of their own), the shape chips and group
  settings, Across the group (the group layers: gradient and shaders) and folds for Variety, Release, Launch and
  Odd ones out. Those settings are left off the Balls card itself (`GROUP_HEADINGS`). An emitter's own card is the
  Emitter card (`emitter_card/`, a child module of `inspector.rs`): chips Emit | Catch | Marker, the mini view
  (`mini_view.rs`: rays, drag the square to aim, fan edges, ray ends for speed), the rounds strip (`strip.rs`, from
  `Inspector.rounds`, which the app fills from the playback) and the Sits on menu; its Look has the Ball / Group
  switch, Group mode showing one round laid out (`ViewCx::settings` stands it in as a custom group). In the
  preview a selected emitter shows its catch circle and start ring/line and snaps onto a circle's middle when
  dropped near it (`preview::seat_near`); the timeline ticks its rounds. The
  timeline (`timeline/`) shows a Hits lane and a group per thing with keys; only the selected thing's group is open,
  over a bars-and-beats grid from the tempo (`beat_lines`, thinned under 4 px; keys snap to beats when the grid
  snaps). Sound: `audio_out.rs` opens the default device with cpal (`AudioOut::open`, `None` without one: the
  playhead then follows the wall clock) and feeds it from an rtrb ring (`Feeder`, resampling with rubato when the
  device isn't 48 kHz); `sound_job.rs` is the sound thread (`SoundJob`: Play, Pause, Seek, Patch, Rebuild; the
  editor never waits on it) and `SoundJob::clock` is the playhead while playing. A physics edit rebuilds the
  sound, anything else patches it; a seek clears the speakers' ring with a 2 ms fade-in. The transport has the
  speaker button and listening volume (`settings::ListenSettings`, never in the scene). The left panel switches
  Things / Mixer (`LeftPanel`; `mixer/`: the tempo row and `master::strip` with `style::Knob` knobs, the level
  meter, loudness readouts and the -14 LUFS graph from `style/meter.rs`). The Mixer is console strips
  (`mixer/strips.rs`: Bus 1..3 and a compact Master; effect slots with `slot_menu.rs`, drag between buses via
  `cx::move_effect`; `mixer/cx.rs` `MixCx`/`MixEdit` gathers edits like `TabCx`). A slot or "Master" sets
  `Mixer.pick` (`MixerPick`); the editor opens inside the Mixer under the strips, across the three bus columns in place of
  the sends and pan (the Mixer opens at the tree's width each time; `mixer/editors/`: `eq.rs` curve (shared with the chain EQ strip through `eq_curve.rs`), `comp.rs`, `delay.rs`, `reverb.rs`, `master.rs`, the rest from settings).
  Hovering a strip rings the shapes on that bus in the preview (`preview::plays_into`). The shape's Output
  panel picks Bus 1..3, has sends to the others and warns when a bus is off. A selected shape's inspector has
  Settings | Sound chips (`InspectorTab`, `Inspector.tab`); the Sound tab (`sound_tab/`) widens the inspector to
  its own panel id and saved width (760 px, `settings::load_sound_width`). `sound_tab/mod.rs` has `TabCx` (knobs
  with the value menu) and gathers `TabEdit`s applied after drawing; `write.rs` Enable/Disable/Copy/Paste sound
  (paste renumbers layers), `osc.rs` the Osc A/B panels with `wave_view.rs`, `mods/` the modulator row and editors,
  `adsr_view.rs` the draggable envelope and what it modulates, `keys.rs` the keyboard (FL names: C5 = MIDI 60; a
  click toggles the note and auditions it, also while stopped), `playing.rs` and `output.rs` (per-shape meter).
  Each oscillator's layer box is `chain_box.rs` ("+ Add layer" with Paste layer, drag a strip's row to reorder in
  its lane, `paste` keeps a Modulation's source when the card has it), `strip.rs` (a folded row: arrow, on switch,
  name and `layer_summary`, a 76×32 mini view, the Mix mini-knob, the loop mark; open: type, slope or quality chips,
  Solo "S", the 64 px view and the knobs; right-click Rename in place / Duplicate / Copy layer / Paste layer /
  Delete; open strips in `SoundTab.open_strips`) and `strip_views.rs` (filter response with its cutoff/reso
  handle, transfer curve, modulated wave). The copied chain layer is `Clipboard.chain_layer`.
  `notes/` is the Notes panel (part 5): `header.rs` (Key, Chord
  presets, octaves), `grid.rs` (a row per slot whose cells sit over the keys via `keys::KeyGeom`; gutter number and
  chord name, row menu, drag to reorder, out-of-range markers), `presets_menu.rs`, `mod.rs` (`NotesState`, the pure
  `apply_op`, the Move notes / Keep notes modal, Plays and Slots change). Copied notes live in
  `Inspector.clipboard.notes`; `TabEdit::OwnStep` makes an edit its own undo step. The timeline gives a shape whose
  slots take turns a group with slot-change ticks (`lanes::slots_take_turns`, `Timeline.slot_marks`), and hold
  keys have no ease menu.
  `sound_tab/mods/` is the modulator row (part 4): `row.rs` (tiles grouped by kind with a four-arrow handle that
  drags a `ModDrag` onto a knob, right-click Rename/Duplicate/Delete, **+**), `drag.rs` (the ghost and the "Can't
  modulate this" note, `check`, `on_knob`), `routes.rs` (the "<name> modulates" list: range in real units, Amount,
  Inv, ±, ×, and the settings that follow it), and the editors `distance.rs`, `lfo.rs` (the wide LFO page; `lfo_view.rs` draws and edits the drawn shape, `SoundTab.lfo_tool`), `hit.rs` (ADSR is
  `adsr_view.rs`). Knobs show a ring per route (`style::ModRing`, drag its end to set the amount, Alt for both
  ways) and a Modulate by submenu. `playing.rs` has the Plays chips; `preview/zones.rs` draws and drags the
  Distances' Near/Far lines while the Sound tab is open. **Follow sound** in the editor: `follow.rs` (`menu`,
  `FollowCx` on `GridCx.follow`, `now`, `set`, `followers`); a settings row's menu has Follow sound ▸ shape ▸
  modulator | Level, the buses, and Stop following sound; a following slider shows a band ("Follows …"); the
  pulse lane says what it follows. `Studio.follow` holds a `ControlSampler` for `build_frame_with` while any
  setting follows sound.

## Working rules

- The project follows the superpowers workflow: brainstorm, then a spec in `docs/superpowers/specs/`, then a plan
  in `docs/superpowers/plans/`, then build, then verify. Read the latest spec/plan for the area you touch.
- The owner wants plain-language messages and a double-click exe, never instructions to run commands.
- Keep code Windows-friendly. Exes are cross-built from Linux with MinGW.

## Building and testing in a cloud container

A fresh container needs:

```sh
apt-get update && apt-get install -y libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers \
  libxkbcommon-x11-0 libxkbcommon0 xvfb xdotool imagemagick gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

- Tests: `cargo test --workspace` takes several minutes cold, so run it in the background. Without
  `mesa-vulkan-drivers`, GPU picture tests fail with "no graphics adapter". That's an environment problem, not a
  bug. In `--release`, `bpe-runtime`'s `pushing_an_earlier_step_panics_in_debug` always fails (it is debug-only).
- Lint: `cargo clippy --workspace` and `cargo fmt` (max width 120). `cargo fmt` rewraps lines, so scripted text
  replacements can silently miss. Check that the old text is present first.
- Windows exe: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio` (and `-p bpe-cli` for the
  renderer). Rename `ball-studio-next.exe` to `ball-studio.exe` when delivering it.
- Disk is a fixed allowance and `target/` grows large. Use `CARGO_INCREMENTAL=0` and delete `target/debug` when the
  disk fills.
- Screenshots: run the release editor under Xvfb at 1440x960 and capture with `import -window root`. When killing
  it, use `pkill -f "[b]all-studio-next"` (a plain pattern also kills your own shell).

## Golden tests (determinism)

The physics is deterministic, and goldens pin it: `crates/bpe-kit/tests/golden*.txt` (plus `-debug` variants).

Any change that moves balls changes golden lines. Re-bless with `BLESS_GOLDEN=1`, **once in debug and once with
`--release`**. Then diff against main: only the lines you meant to change may differ. Goldens differ slightly on
native Windows (libm trig), which is a known gap.

## egui / kittest pitfalls

- A drag-only widget on top of a click widget swallows clicks. Use `Sense::click_and_drag()` on the button itself.
- There is no `egui::SidePanel`; use `egui::Panel::left(id)`. Side panels shrink to their content on the first
  frame, so call `ui.set_min_width(ui.available_width())` first.
- `ui.add_space` inside `egui::Grid` panics in debug.
- kittest's accessibility check needs every control named via `.labelled_by(label.id)`. ComboBox can't be named,
  so pickers are `menu_button`s. `get_by_*` panics on several matches; use `query_all_by_label(..).next()`.
  Rows below the fold need `scroll_to_me()` in tests.
- Never read `pointer.press_origin()` inside a `clicked()` handler, because egui clears it on release.
- The default fonts lack glyphs like ◉ ○ ♭ ♯ ▶. Paint icons or use plain words.

## Engine pitfalls

- Card sim states are `MotionState { settings }` and `BounceState { settings }` (in
  `bpe-kit/src/modules/{motion,bounce}/sim.rs`). `downcast_ref::<BasicBounce>()` compiles but silently returns
  `None`.
- `World::matching(target, me)` lists the things a target points at (Me = the thing itself); `World::names(target,
  me, other)` asks whether it points at one thing (the camera's hit trigger uses it).
- A thing with no Look card draws nothing, and a card of an unknown kind is reported and ignored.
