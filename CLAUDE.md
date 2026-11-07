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
kinds' say: `ThingKind::vary` (balls: Size live, Launch speed at launch, so no Speed/Hits; an Emitter: Size
live, Speed at launch, `EmitterState::varies`/`own_size`, unaimed balls only), `ModuleKind::vary`
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

## Lighting (spec `docs/superpowers/specs/2026-11-06-lighting-design.md`, plans `docs/superpowers/plans/lighting/`)

Part 1a is in (#106). **Light** things (`bpe-kit/src/things/light/`: `LightKind`, family "Lights", no cards; the
Add thing menu offers Point, Spot, Sun, Area and Strip light through `add_light`, which also gives the scene its
**Lighting** card `scene.lighting` (`modules/lighting.rs`, `SlotId::Lighting`) when it has none). A light's place is
its own keyframable Position (the Placement card is hidden for lights, and the preview's placement ring leaves them
out); inside a Balls, Emitter or Shape thing (`RIDES`) it rides it, read from the tree parent: Each ball, The middle,
Ball number (1-based), A point on the edge (`draw.rs::places`, `outline_points`, `point_round`). `LightDraw::light`
(the `ThingDraw::light` hook, called after every thing draws when the scene draw set `Frame.light`) turns settings
into `LightGpu`s and strip segments. Every drawn and around layer has a "Lighting" fold (`LayerKind::light_defs`,
`layers/lighting.rs`: `lit`, `blocks_light`, and `bulge` on a ball's Body, the spec's Roundness); records carry it in
`LayerGpu.light`, `TrailVertex.light`, `Particle.light` (`around::Drew` stamps trails and particles; ghosts and mini
balls take their layer's). The renderer (`bpe-gfx/src/lib.rs::render`, `lighting.rs`, `shaders/lighting.wgsl`) draws a
lit frame three times through `draw_scene` (`Globals.light.x`: 0 the picture, 1 each layer's colour × Lit, 2 the
surface: R blocks, G a ball's height), then a jump-flood signed distance field of the blockers, then each pixel's
lights with a 2D shadow march (penumbra from the lamp's size, stopping short of the lamp by its size, a ridden ball's
radius and how deep in a blocker it sits) and normals from the height: `C + (L − 1) × clamp(W, 0, C)`. No Lighting
card leaves `Frame.light` `None` and the picture byte-identical (`bpe-kit/tests/unlit_pictures.rs`, hashes made before
lighting; `tests/lit_pictures.rs` checks lit pictures; `scenes::lit_room`/`lit_demo` are test-only). The status line
shows "Lighting: N lights, X ms" (`Renderer::lighting_stats`, GPU timestamps via `timing.rs` when the device has
`TIMING_FEATURES`, which `bpe-studio/src/main.rs` asks for where the card has them).

Part 2a is in (plan `docs/superpowers/plans/lighting/part2a.md`): **Stage 3D** and **surfaces**. The Lighting card's
Depth is Flat (1a's picture, pinned by `tests/lit_flat_pinned.rs` tile averages made before 2a) or Stage, with a
"Stage" section (Back wall, Shape depth, Floor None / At the bottom / Under the lowest shape / At a height, Floor at,
Looking down, Floor surface Matte / Glossy / Mirror, Floor blur, Reflection, Wall surface) and Sky light/top/bottom in
Room (`modules/lighting.rs::gpu(lowest_shape)`, `things/scene/draw.rs::lowest_shape`; `LightingGpu.stage/floor/
floor_more/sky_*`). In Stage each ball is an impostor sphere (`SphereGpu`, pushed by `balls/draw.rs::push_ball_light`,
emitters too) whose shadows are analytic (`lighting.rs::SphereGrid`, a CPU uniform grid of spheres, group 1 of the light
pass) and fall back onto a wall at z = −Back wall seen in an oblique view (k = tan Looking down, wall and floor only);
balls write no 2D blocking in Stage (`LIGHT_BALL`); shapes stand out of the wall (the 2D field extruded,
`slab_visibility`); `fs_floor` mirrors the picture in a Glossy/Mirror floor (Fresnel, blur growing with distance).
Every Look card (`look.layers`, `look.shape`) has a **Surface** section (`modules/look/surface.rs`: Surface Matte /
Plastic / Chrome / Metal / Glass / Toon, each a preset of Roughness, Metal, Clear coat, Bends light (IOR; F0 from it,
Matte's 1.0 gives no shine so Flat Matte is exactly 1a), See-through, Toon steps, then Reflects and Bulge, which moved
here from the Body); `surface_index` (1-based among things with such a card) goes in `LayerGpu.light[3]` and the
surface picture's B channel, and `LightScene.surfaces` holds the `SurfaceGpu` table. The shader (`lighting.wgsl`
`shade`/`mixed`) is GGX with Karis widening, metal tinting, a clear-coat lobe, Toon bands and a hard spot, sky-gradient
reflections, and Glass dimming the diffuse with lighter tinted sphere shadows (no refraction yet). The editor shows the
section as painted chips (`inspector.rs::surface_section`, `style/swatch.rs`) with the numbers folded under them;
picking a chip writes its preset in the same edit. `scenes::stage_demo` is test-only; `tests/stage_pictures.rs` checks
the stage pictures.

Part 2b is in (plan `docs/superpowers/plans/lighting/part2b.md`, "As built" at its end): **advanced surfaces**
Jelly, Pearl, Flake, Brushed, Frosted, Velvet, Lava (13 chips; `surface.rs` `Preset`, numbers Rainbow split, Soak,
Film, Flakes/Flake size, Brushing/Brush lines/angle, Frost, Rim, Inner glow; `SurfaceGpu.c/d/e`), the **Surface
sub-layer** (`sub.surface`/`shape.sub.surface`, not on a Body: `Stamps` gives the layer and its sub-layers a table
index after every thing's, `surface::sub_defs` leaves out `WHOLE_THING_ONLY`; the editor writes the preset on pick),
**Reflects: The scene** (default; `trace_scene` walks spheres, wall, floor and shape fronts up to the Lighting card's
Reflection depth; spheres are pushed in Flat too, `SphereGpu.c` their surface, never for shadows), **glass**
(`GLASS_FLAG` 4096 on a see-through record's `light[3]`, `OWN_FLAG` 8192 on a layer with its own Surface sub-layer; the renderer draws a **behind picture** as `BEHIND_PASS`
with those records left out (`left_behind`) and `transmitted` bends it by Bends light, Rainbow split and Frost;
a shape's **Lens** is its body record's `light[2]`, a dome height in `shape_walk.wgsl`; `surface::glass_of` is the
lasers' hook), **mirror walls** on shapes (Mirror wall Off/Outside/Inside/Both sides = where the reflection shows,
strength, Squeeze (1 on a circle is circle inversion), tint; `MirrorGpu`, `MOST_MIRRORS` 16, `mirrored` in
`fs_mirror`, a pass after the mix that samples the mixed picture), **depth of field** (Stage: Depth of field, Focus at, Blur;
`fs_dof` after the floor pass) and 2a's gaps (shapes' sheared slab in shadows and contact, `thing_point`; the light
field sees Stage balls (`GlowUniform.on.w`, `field_round_ball`); the background's own light on shiny/Stage pixels
(copied to `bg_glow`, read as `above` in the mix); every sphere tested with ≤ 128 balls, `SPHERE_MARGIN` 3; Lit 0
layers write surface index 0). `tests/glass_pictures.rs`; `scenes::surfaces_demo` is test-only.

Part 1c is in: the editor's side. The left panel switches Things / **Lights** / Mixer (`LeftPanel::Lights`,
`bpe-studio/src/lights_panel/`): the Lighting card's Depth and Quality chips, "+ Point … + Strip" (`light_parent`, shared
with the tree), and a strip per `LightSource` (`Light`, `Layer` for any layer whose kind is in `GIVES_OFF_LIGHT`
(`sub.light`, `shape.sub.light`), `Ambient`; `sources` lists them in tree order, each run under its nearest folder or rig (`Group`, a rig's header counting its `fixtures`), riders named by `rig::fixtures::ridden`):
the Strength fader (the real setting at the playhead through `set_or_key`), **M** (a light's `Thing.enabled`, a layer's
`enabled`, Ambient's new `ambient_on`) and **S** (`Studio.light_solo`, editor only: `soloed` gives the preview a
resolved copy without the other lights and with ambient off; a "Solo: …" pill ends it). Light handles
(`preview/light_handles.rs`, `Handle::Light(id, LightPart)`: Lamp(k), Reach, Cone, Aim, Corner, End) show for the
selected light, or every light while the Lights tab is open (hit in three passes, `light_handles::Pass`: the selected light's lamps and dots before the balls, its reach ring and cone edges after them, other lamps last); they read `settings_at` (keyed values at the playhead) and
`light::draw::places_in`/`aim_angle`, drag with `set_or_key` and keep the playhead. **Show** (`Studio.light_show`,
`bpe_gfx::LightShow` on `LightScene.show`, the light pass's `ambient.w`): Picture / Light only / Heat map (`heat` in
lighting.wgsl, the same stops as `style::HEAT`), set only by the editor (`preview::show_light` also drops picture effects,
bloom and grade). Every spanned number setting is a `widgets::span_slider` (relative drags that carry on past the span,
a box that takes any number, only the hard `range` clamps; past the span: knob at the end, an arrow, a bright boxed
value). Lights up (linking) and Picked colour moved to part 4.

Part 4 is in (plan `docs/superpowers/plans/lighting/part4.md`): two more light passes, 5 the **UV picture** (each
layer's colour × its UV glow, `LayerGpu.more.x`) and 6 the **tags picture** (R catches beams, G the thing's **link
number**, A cover); `more` = [UV glow, Only under UV, Catches beams, link], `TrailVertex.extra`/`Particle.extra` the
same, the background's link `Globals.light.w`; tags are written hard (`hard_tags`: the link whole where cover ≥ 0.5, beams soft); stamped by `layers/lighting.rs::stamp`/`AroundLayer::more_in`, all 0
without a Lighting card. A layer's Lighting fold gains Under UV (Normal / Glows / Only under UV) and UV glow, around
layers Catches beams (`AroundLit`, smoke). Lights (`things/light/`): kind **Laser** (code 5, `laser.rs`: `scenery`,
`trace` against walls as capsules and balls as circles, `Meets` Bounce/Stop/Pass/**Bend** (Snell + total internal
reflection, the hook for glass in 2b: the `meets` closure in `draw.rs::push_light`; at most `MOST_BOUNCES` 256), segments lit as a strip, `laser_beams` in the mix), **Gobo** (`p8`/`p9`,
`gobo()` in lighting.wgsl: Stripes, Dots, Window, Leaves, Water caustics, Rings, Clouds), **Flicker** (CPU,
`draw::flicker`, pure in time and seed), **Ultraviolet** + Visible light (`p7.yz`, UV light into `direct.a`), **Lights
up** (`links.rs`: `number_of` = 1 + place among resolved things, `named`; `p10`, the `links` buffer; `lights_up` in `fs_direct` and `shade`) and Colour from
**Picked** (`draw::Picked`: a thing's layer colour, or each ball's own via `ball_colors`). The Lighting card gains
Haze moves (Still/Drifting/Swirling, an `extras` pass writes density; balls stir it through `LightScene.stirs`, a
splat pass), **Underwater** (caustics, depth colour, sun rays, in `fs_mix`) and **Glow in the dark** (`extras` RGB:
the remembered light, two pictures ping-ponged and reprojected through the last camera; a seek or > 0.5 s step is
`Renderer::memory_cold`; `Frame::memory` counts it (8.3 fades, at most 10 s) so the editor's `preview/warm.rs` replays it after a jump or edit, and `forget_effect_history` forgets it). **Heat glow**
(`layers/heat.rs`, `sub.heat`/`shape.sub.heat`, also on a Body: `Stack.body_heat`, `body_records`): a Flash record with
`amount.w` 1; each ball's heat (`ball_heats`, one a ball: the first Heat glow found in its stacks) or the shape's in `hit[3]`; its light goes into pass 3.

Part 7 is in: **light sensing** (plan `docs/superpowers/plans/lighting/part7.md`), optional and one-way: nothing the
lighting or the probe reads may sense light. The CPU **light probe** (`bpe-kit/src/sensing/`: `probe.rs::Probe`, the
runtime's `LightAt`/`LightMaker` in `bpe-runtime/src/light.rs`, `Sims.light` = `sensing::KitLight`) builds the same
`LightScene` the renderer gets (through `LightDraw::light`, its own resolve with `Inputs.light` none) and evaluates
`lighting.wgsl`'s lamps on a flat surface with 2D shadows (balls as circles, a grid past 16 balls with a
penumbra-widening walk; shapes as outlines; glowing balls/shapes as lamps), cached per moment. Levels are
`level_of(L) = 1 − e^(−L)`. **Follow light** is `PulseSource::Light { thing, source: LightSource }` (On, Brightest,
Shadow, InShadow, Strength) read through `Inputs.light` (`LightLevels`); `sensing::can_follow_light` /
`lighting_reads` keep it off anything the lighting reads. **Spreads** `VaryBy::Light`/`Shadow` (`BallSpread.light`/
`.shadow`, `offered(use_, lit)`: card and layer settings only, with a Lighting card). **Moths** are Motion settings
(`moths` Drawn to light / Pushed by light, `moth_pull`, `moth_near`, `moth_shadows`; `ModuleSim::light_pull`), worked
out once a step (`World.pulls`) and added in `move_bodies`. A world makes a probe only when its physics senses light
(`needs_light`), reading the light as it was at the previous step's end, and its fingerprint then hashes
`sensing::fingerprint`. The **Light modulator** `mod.light` (`LightModSettings`, `bpe-sound/src/mods/light.rs`, knobs
`k::light`, `ModsSpec.lights`/`probe` from `patch::probe_of`, `ModCx.light`/`log`, `BlockCx.log`) reads it once a step
and Follow sound reads it too (`heard_fingerprint` adds the light's fingerprint). The editor keeps `Studio.light_probe`
(`uses_light`) for `build_frame_sensing`; `bpe-cli` does the same. Menus: `follow.rs` (`lit_menu`, Follow light ▸),
the Sound tab's Light page (`sound_tab/mods/light.rs`). Goldens: `tests/golden-moths*.txt` (`scenes::moth_scenes`). The probe's
lamp cache is keyed by `World.run` (`RunId`, fresh on every clone, so test flights and snapshots never share
moments) and keeps each ball's sample per moment. The physics' and the sound engine's probes hear no sound (lights
that follow sound read as set there; only the picture's probe hears it). A setting the lighting reads can't follow
light, spread by it, or follow the sound of a shape with a Light modulator (`FollowShape.hears_light`).

Part 5 is in (plan `docs/superpowers/plans/lighting/part5.md`): **lens effects** and the **Stage camera**. Lens flare
(`lens_flare`: ghosts through the middle, a halo, a starburst), Lens streaks (`lens_streaks`, anamorphic, kicked by
hits) and Colour fringes (`fringes`, a radial spectral split that pulses on the beat) are built-in effects of the
Effects stack. A `// lens: rays soft` header line asks for a pre-pass (`bpe-gfx/src/effects/lens.rs`, `LensPass`): the
effect's own `fn bright(uv, p)` drawn at half size (`assemble_bright`), then a soft mip chain (`lens_soft(uv, level)`,
binding 9) and Kawase streaks along `rays`/`ray_turn`/`ray_length` (`lens_rays(uv)`, binding 8; pass n spaces its
4 taps 4ⁿ texels apart, `ray_passes`); inside a ball both read black (`in_ball.wgsl`). The Camera thing has a
**Stage camera** group (Stage camera switch, Orbit, Tilt, Height `stage_height`, Lens mm, Turns round `pivot`, Edges
Zoom in to fill / Stretch); `things/camera/draw.rs::CameraDraw` puts it on `LightScene.camera` in Stage only, and
`Frame::stage_view` solves it (`bpe-gfx/src/stage_camera.rs`: `StageView::solve`, the rest camera that frames the
front shot exactly, `plane_at`/`screen_of`, the fill zoom by bisection). After the light passes the renderer re-views
the lit front picture (`stage_camera_pass.rs`, `shaders/stage_camera.wgsl` pasted after lighting.wgsl, sharing its
three bind groups through `Lighting::layouts`/`bound`): the background drawn alone tells the bare wall from things,
a pull–push fills the wall in under them, and each ray finds a ball (the sphere grid), a shape (the field extruded),
the wall or floor, and the flat things on the picture plane; every hit is coloured where the rest camera saw it
(`cam_rest_uv`); a first look of one ray a pixel (`fs_cam_view`, a code of what it hit in alpha) is followed by
four more rays where a neighbour hit something else (`fs_cam_edges`, not in Draft). Shapes come from the slab
(stepped by the field, the outline found exactly in the full-size surface picture, `cam_blocks`), balls from the
sphere grid. Picture effects then see the moved picture, its unlit picture and lit colour, the object picture seen
again (`see_objects`) and the ball list through the camera (`fx_balls`, `StageView::scale_at`); things stuck to the
screen are drawn after the move (`Renderer::draw_overlay`), unlit. With no move (`motion` 0) the pass is skipped
and the picture is the front one exactly. The
editor's `View.stage` (from `Studio.stage_view`, set when the shot is drawn; the World view clears it) maps the
pointer and handles through the moved camera. Tests: `bpe-gfx/src/lens_tests.rs`, `bpe-kit/tests/stage_camera_pictures.rs`.

Part 3 is in (plan `docs/superpowers/plans/lighting/part3.md`): the **bake** (cinema render) and **glass caustics**.
The Lighting card's Quality group gains On export (Same as live / Bake), Samples, Bounced light (Like live / Traced,
`BakeBounce`, `bake_more[3]`), Light bounces, Bake reflections,
Motion blur, Shutter, Noise clean-up, and Glass caustics (`LightingGpu.bake`/`bake_more`; `LightingSettings::bake`,
`lighting_of`, `bake_of`). A baked frame is `Renderer::render_samples` over the frames `bpe_cli::FrameMaker::samples`
builds: `bpe_gfx::bake::moments` puts each sample within the shutter (the last on the frame), balls between two steps
are lerped back toward `previous` (`World::set_between`/`restore_between`, never touching the physics), each sample
moves the camera within the pixel (`bake::jitter`, Halton 2/3), draws at quality code 4 (field full size, 128 shadow
steps) with `Frame.bake` set (`lighting.wgsl` `baking()`, PCG `rand`: jittered blurred reflections at any roughness,
`trace_scene` up to 16 deep, one wavelength a sample through Rainbow split, DOF taps ×4), and `accumulate.rs` averages
them in Rgba32Float; effects, bloom and grade run once on the average (`EffectChain.effect_time` keeps effect memory
counting frames). Bounced light Like live (default) keeps the live light field's bounce so a bake keeps the look; Traced
runs the field Light bounces times, and in Stage path-traces the bounce light (`fs_trace`: `TRACE_RAYS` cosine rays a pixel through
`scene_hit`, each hit lit by `lamps_on`, up to Light bounces; `fs_denoise` à-trous steps 1/2/4 when Noise clean-up is
on; `fs_trace_add` into `bg_glow`), and the light field then leaves its own Bounce out. **Caustics** (live and baked):
`lighting::caustic_glass` lists see-through spheres that bend light, `photons_per_pair` sizes the photons by Quality,
and `vs_caustic`/`fs_caustic` (bind group 3) send each light's photons through each glass ball (two refractions, a
wavelength each with Rainbow split) onto the wall/floor (Stage) or the plane under the ball (Flat), splatted into the
direct light before it is copied to `bg_glow`. `bpe_cli::bake_for` (the export window's Live / Bake, `ExportOptions.bake`,
`--bake`/`--live`) and `final_frame` (warm-up then one baked frame, timed); the editor's **Preview final frame**
button on the Lighting card and the export window's **Test a frame** open `final_frame.rs`'s window. No hardware ray
tracing: wgpu's ray queries aren't on every card the exe runs on, and the 2D-plus-spheres scene traces fast enough in
shaders. `tests/bake_pictures.rs` (bpe-cli); `scenes::bake_demo` is test-only.

## Rules (spec `docs/superpowers/specs/2026-11-06-rules-design.md`, plans `docs/superpowers/plans/rules/`)

Part 1 is in (plan `part1.md`, "As built" at its end): the headless engine core. `Scene.rules` (`bpe-core/src/rules/`:
`Rules`, `Rule { id, name, enabled, blocks }`, `Block { id, kind, inputs, params }`, `BlockSection` When / If / Logic /
Do, `Who` That / Other / Group / Thing{ball} / Tag / Kind / Anything as `Value::Who`; edits `add_rule` … `link`,
`set_block_param`; a number setting `k` may take its value from the firing via `k~from`/`~times`/`~plus`; format 12).
Every block kind is a plugin: core's `RuleBlockKind` (`Registry::add_block`) and the runtime's `BlockSim`
(`Sims::add_block`); the kit's 14 are in `bpe-kit/src/rules/` (`when.hit/time/beat`, `if.compare/is/chance/every/
cooldown/only/between`, `logic`, `do.motion/change/break`). The runtime (`bpe-runtime/src/rules/`) runs the **Rules
stage** at the end of every step (after the history record): each enabled rule without a problem, in order, Logic
ticks then Whens fire, firings flow depth first, Do blocks queue `Action`s (at most `MOST_ACTIONS` 4096 a rule a
step, else `busy_at`), and a `Rule` event (tag 10, `b.item` = `rule_item(rule, block)`) is logged per block that
fired. The actions apply at the next step's start (`World::apply_rule_actions`, before `animate`): motion changes,
freezes, **Break** (event tag 11, then Remove for a ball; a whole thing loses its wall and drops out of the order and
the frame) and setting **changes** (`changes.rs`: Set/Add/Multiply with glide, ease and lasts, folded over the keyed
value from a pristine copy each step so they never compound; physics keys re-prepare the cards, per-ball keys make
`Slotted.own` states, Size per ball through `World::ruled` in the Balls/Emitter sims, looks per ball in `Worn`,
whole looks in `build_frame_sensing`). Dice are `rules::dice` (seed, rule, block, step, subject). Nothing changes
for a scene without enabled rules (goldens, fingerprint); `tests/golden-rules*.txt` pin `scenes::rule_scenes`.

Part 2 is in (plan `part2.md`): the **editor**. The bottom panel switches Timeline / **Rules** (chips at the left of
the transport row, `settings::BottomView` saved in `bottom-view.txt`, `Studio.bottom`/`set_bottom`; the scrub bar
stays). `bpe-studio/src/rules_panel/`: `mod.rs` (`RulesPanel` in `Panels.rules`: `picked` block, rename, link and
rule drags, copied rule/block; `show` → `Outcome { separate, message }`, a structural edit is its own undo step through
`Studio.separate_edit` and `record_frame`), `layout.rs` (`lay_out`: column = longest path in, row = order added within
the column, links with their input index), `face.rs` (section, name, up to three simple settings or the kind's
`summary`; Change setting shows who, `setting_words`, how; who fields carry the thing's track-colour dot), `fired.rs`
(`Fired` from the log's Rule events: `count` of end-block moments, `recent` within `RING_SECONDS` 0.15, `firings`),
`menu.rs` (When / If / Logic / Do ▸ kinds), `setting_menu.rs` (thing ▸ card ▸ (layer ▸) setting, `cards_of`,
`picked_def`, "(whole group)" where one ball can't change alone) and `inspect.rs` (the inspector for a block, shown by
`Studio::picked_block` while no thing is selected: problems in `style::PROBLEM`, the settings through
`widgets::grid_row_with`, a "from" chip on each of the kind's `firing_keys` (`RuleBlockKind::firing_keys`, `values()` names
what a When's firings carry, both via `block_kind!`'s `values = [..], from = [..]` tail), Compare's Value menu, Change
setting's To shown as the picked setting's own type). `widgets::who_picker` draws every `Value::Who` row
(`TargetChoices.relative`, `ball_things`). Lanes: header (switch, name renamed in place, "fired N times", red problem
dot named "Problem in <rule>", "too busy at"), blocks named "<Section> <name>" ("Logic" alone), out-dots "Link from …",
"Add after …", "Add When to <rule>", right-click menus for headers, blocks and lines. The timeline has a Rules lane
under the Hits lane while the scene has rules (`rules_lane`).

Part 3 is in (plan `part3.md`): **break looks**, drawn only. `do.break`'s Look is Vanish / Fade out (Time) / Pop (Time,
Swell) / Shatter (Pieces, Spread, Spin, Gravity, Hit shapes, Bounce, Fade after, Fade over; `breaks.rs`: `LookSettings`
under the same keys, `show_for` gives a `BreakShow { look: Params, lasts }`, lasts 0 for Vanish and for kinds that
can't show a look (only shape, balls, emitter, light can)). `Action::Break.show` (an `Arc<BreakShow>`); applying it
keeps `RulesState.remains` (`bpe-runtime/src/rules/remains.rs`: `Remains` by (thing, row or `WHOLE`), with the hit,
the gravity, a ball's `BallAt` or a shape's taken wall; `remains_of`, `showing`, pruned when over). The physics never
reads them; the log and goldens are unchanged. `build_frame_sensing` (and the light probe) keep a broken thing while
`showing`. The kit's `rules/remains/` (`showing` per look, `shatter.rs`: `cut` = a seeded Voronoi of the square round
what is drawn, sites on the material (`Material`/`Edge`: inside, or a band along the outline), `piece_at` closed form or
1/120 s steps with `keep_clear` against the world's walls; `draw.rs`: `shatter_shape`, `ball_remains`). Each piece is the
thing's own instance again, moved and turned, with `PathInstance.clip`/`LayeredInstance.clip` = [first corner in
`path_points`, count, fade, 0] (vertex attribute 6 / 12; group 2 binding 4 is now seen by the vertex stage, which fits
a piece's quad to its cell; `clip_keep` in paths.wgsl, used by `shade_path`, `shade_shape_layers`' `keep`,
`shade_layers` and the object passes; all-zero clip draws as before). A broken light's strength follows its look
(`remains::whole_now`); a broken emitter's marker goes at once. `tests/break_pictures.rs` checks the pictures.

Part 5 is in (plan `part5.md`, "As built" at its end): **more events and actions**, 13 kit blocks. Whens
`when.enter` (Enters / Leaves / Either a zone, `rules/events.rs`: each (zone, thing, row) inside set worked out from the
world at prepare and diffed every stage; a catcher's `Enter` events too), `when.fired` (an emitter's balls, `Spawn`
events, or rounds, `EmitterState.fired`), `when.removed` (Removed / Broken), `when.count` (`counts.rs`: hits so far,
ball count or a counter rising/falling to an amount), `when.level` (`level.rs`: a ball's speed, size, light or shadow,
or a shape's/bus's sound crossing a level; `BlockSim::hears_sound`/`sees_light` put the sound/light makers'
fingerprints into the world's), `when.signal`; If `if.counter`; Do `do.split` (`balls.rs`: a ball becomes `into`
balls fanned over Spread, sizes shared by area or kept, each keeping the ball's number, own size, rule changes and
tags, `World::split`, `RulesState::copy_row`), `do.spawn` (new balls in a Balls or Emitter thing through
`ThingSim::add_ball(NewBall)`), `do.fire` (an emitter's round through `ThingSim::fire_now`; Fire when **By a rule**,
`FireWhen::ByARule`, `EmitterState.asked`), `do.tag` (a thing's tag in `world.tags`, a ball's own in
`RulesState::ball_tags`, which `who::matches(cx, ..)` reads; the physics sees only things' tags), `do.count`
(`RulesState::counters`, Add / Set / Reset), `do.signal`. Spawned balls count against `MAX_BALLS` live and
`MAX_ROWS` rows. Counters, tags, splits and spawns apply at the next step's start; a **signal** (`Action::Signal`,
`rules::Signal`) is heard in the same stage by `BlockSim::hear`, in up to `MOST_ROUNDS` 8 more rounds, the last
round's carried to the next step (`RulesState.carried`). A shape's **Balls pass through** (`ShapeSettings.passes`,
`Wall.passes`) keeps its wall for everything but the collision pass, so a shape can be a zone.

Part 6 is in (plan `part6.md`): the **spectacle actions** `do.camera` (Shake / Zoom punch: `Action::Kick`, kept in
`RulesState.kicks` (`Kick`, `MOST_KICKS` 64, weight (1 − age/time)²), added by the camera sim after its own shake and
punch), `do.slow` (Slow motion: `Action::Pace`, part 1's `Change` folded on a pace of 1 (`RulesState::pace_at`, 0.01×
to 8×); each step `World.pace` scales the physics' dt (`World::sim_dt`, the ceiling ÷ pace, the resting threshold, the
shape's spin and shrink), the video clock, keys, camera, schedules and draws run on; `RulesState.lag`,
`World::sim_time`, and `SimCx.time` reads the simulated clock, exactly `step / rate` while `lag` is 0), `do.flash`
(`EventKind::Flash`, tag 20, one per ball, counted in `RulesState.flashes`: balls' `last_hits_but`, the Flash on hit
sprite scan and `GroupCx::hits`, shapes' `shape_hits` read it; nothing else does), `do.light_scene` (`SceneOp`,
`ShownScene` in `RulesState.scene`, resolved at apply by `next_scene`; `light_scenes::now` shows it over the card's own
Show, crossfading from what showed) and `do.note` (`EventKind::Note`, tag 21: `b` = (shape, pitch or `AS_IF_HIT`),
`normal.x` length, `speed` velocity; `HitCursor` places it, `ShapeVoice::rule_note` plays it past the hit filter (a
drone on its own voice), `catch_up` replays its picks, `ControlSampler` hears it, MIDI records it). Tags 12..19 are left
for parts 3 to 5. `tests/golden-rules*.txt` gain "Slow motion"; `bpe-sound/tests/rule_notes.rs`.

## Gradients (spec `docs/superpowers/specs/2026-11-06-gradients-design.md`, plans `docs/superpowers/plans/gradients/`)

Part 1 is in (plan `part1.md`): the gradient value (`bpe-core/src/color.rs`) has a **Mix** (`Mix`: Light, the old
blend in linear light and the default of every settings struct's gradient; Plain, sRGB numbers; Even, OKLab; Round the
wheel / Long way round, OKLCH hue the short or long way, chroma lowered into sRGB by `oklab::fit_gamut`), each stop a
**gap blend** for the gap after it (`Stop.blend`: `GapBlend` Smooth / Eased / Ease in / Ease out / Steps, and
`Stop.middle`, where the halfway colour or the Steps edge sits, a power bend), and **Loop** (`Gradient.looped`: `t` wraps
with `fract` and the last stop's gap runs back round to the first; `set_looped` respaces a gradient whose last stop sits
on 1). The old `transition` field is read into gap blends (`SavedTransition`, read only). New gradients made in the
editor (the "+", palettes, Add matching, Random) mix Even; the Rainbow palette loops Round the wheel. **Companions**
(`gradient_extras.rs`): every gradient setting `k` (a layer's keeps its prefix) has `"<k>~bands"` (`banded(n)`, 2n
stops, at most `MOST_BANDS`) and `"<k>~grain"` (`Gradient.grain`, `#[serde(skip)]`, carried to the GPU only) beside it,
plain numbers that key, pulse and follow sound; `def_at`/`layer_def` answer them, they can't vary across balls, and
`apply_moves` puts them onto the gradients right after `Animation::apply` (`build_frame_sensing`, the light probe).
The GPU atlas rows are 1024 wide (`ATLAS_WIDTH`, the grade map stays 256), `PaintGpu.grain` dithers in `paint_along`
(`paint_px`, set at the top of every fragment entry that paints). `Follow::OrderThereAndBack` and the group gradient's
`Spread::ThereAndBack`; Order on a looped gradient spreads `i / n` (`eval::order_place`) so a ring has no seam. The
editor (`gradient_bar.rs`) has a diamond per gap (drag: the middle), a selected stop, and the tabs under the bar
(`gradient_tabs.rs`: Colours (stop colour, hex, at, gap blend, middle) | Blend (Mix, Loop, Bands, Grain, Every gap),
Shape and Move once parts 2 and 3 add companions there); companion rows are ordinary grid rows under their full key
(`companion_def`, `widgets::def_in`), and their timeline lanes read "Colours bands".

Part 2 is in (plan `part2.md`, "As built" at its end): **gradient shapes**. One form list for every area gradient
(`bpe-core/src/gradient_form.rs`: `GradientForm` Straight, Round, Sweep, Stripes, Rings, Along the edge, Diamond,
Polygon, Spiral, Kaleido, Off-centre, Waves, Marble, Free points, Ripples, Turbulence; GPU codes 1..6 and 13..22,
`along` the CPU twin of `paint.wgsl`'s `paint_form`, `area_colour` what the editor's previews show, `free_points`/
`free_colour`, `points_after` keeps a point per stop). A gradient setting's `Flags.area` (`Area::No`, `Fills(form)`:
placed by its own Turn/Centre/Size/Beyond (`~turn ~centre ~size ~repeat`), `Placed(form)`: placed by its paint's
Angle/Size/Centre/Span) offers the Shape companions `~form` and each form's numbers (`~sides ~star ~arms ~twist
~mirrors ~focus ~stretch ~stretch_turn ~count ~height ~flow ~detail ~bend ~seed ~softness ~points ~spacing ~from
~speed`, `when` written relative, `when_of`); `apply_moves` puts them into `Gradient.layout` (`Layout`, never
saved) and the kit's `paint::form_gpu(g, default, Placing, atlas)` turns them into `PaintGpu.pattern`/`form` (80 bytes).
The old per-site gradient choices are gone (the Paint's Gradient straight/round, the Pattern's Stripes/Rings/Linear,
now Pattern Gradient + `in_colours~form`, Stripes by default; a shade-mode Pattern has its own Shape row).
**Ripples** read a field of the frame's balls and shapes (`bpe-gfx/src/ripples.rs`, `shaders/ripples.wgsl`,
`ripple_common.wgsl`): built only when `Frame.ripples` (`gradient_extras::any_ripples`), half the video size, two
nearest sources of different owners among every / balls / shapes by a jump flood; things push `RippleSource`s
(`push_ripple_ball`, `push_ripple_outline`), and paint skips its own owner (`paint_world`, `paint_own`). The
editor's Shape tab (`gradient_tabs.rs::shape_chips`, CPU preview chips in `gradient_form_chips.rs`) then the form's
rows; Free points show as dots in the stop colours on the ball view (`look_editor/free_points.rs`, `handles.rs`) and
in the preview for a selected shape's layers and the background (`preview/free_points.rs`, `Handle::FreePoint`),
a drag writing the whole `~points` list. Heavy paint maths has one call site per program (llvmpipe inlines it all).

Part 3 is in (plan `part3.md`): **movement**, the Move tab's companions (`gradient_extras::moves()`, each with its
fold in `SettingDef.group`): Cycle (`~cycle`, `~cycle_shift`), Step on the beat (`~step_every` beats, `~step_glide`),
Breathe (`~breathe`, `~breathe_speed`, `~breathe_every`), Hue spin (`~hue_speed`, `~hue_turn`), Stops wander
(`~wander`, `~wander_speed`, `~wander_seed`), Hit wave (`~wave`, `~wave_width`, `~wave_speed`, `~wave_colour` a
Color), Morph (`~morph`, `~morph_to` a Gradient companion, which has no companions of its own: `split` refuses a base
that is a companion) and Shape spin (`~spin`, added onto part 2's `~turn`). `apply_moves(resolved, time)` reads the
beat from the root's tempo (`gradient_extras::Beat::of`, the same as `beat_at`), moves the stops for Wander
(`Gradient::wandered`, never past a neighbour) and puts the rest on the draw-time `Gradient.moves` (`Moves`: shift,
breathe, hue, morph, wave; `#[serde(skip)]`), which `Gradient::sample` honours (slide, an unlooped one wrapping with a
cut; spread round the middle; `mix_linear` toward Morph's; `hue_turned` in OKLCH), so bakes, bands, CPU colours,
lights and the probe all see it; `sample_still` is the plain sampling; a one-colour gradient takes its moved colour
into its stop; `step_stops` gives moved colours; bands bake the moves in. **Hit wave** (`HitWave`: `at`, `apply`,
`lasts`) travels with the gradient's atlas row: `GradientAtlas::waves` (two texels a row, `wave_texels`, `NO_WAVE`)
uploaded to the paint bind group's binding 2 (`Rgba32Float`), read by `paint.wgsl::paint_wave` with
`var<private> paint_hit` (set beside `paint_px` in `shade_layers` and `shade_path` from the instance's hit) in
`paint_along` (sample → vary → wave → grain); the atlas hash eats moves and waves only when there are any. Stacks
(`Stack`/`ShapeStack.wave_memory`, `gradient_extras::wave_memory`) ask for hits while a gradient has a wave; on the
CPU `paint::Inputs.hit_age` (filled by `BallInputs`/`WallInputs`) runs it in `colour_at`. The background, trails,
particles and lights have no hit, so no wave. The Move tab folds each move (`widgets::group_heading`, open state under
the bar's id); one-colour gradients still show no tabs.

Part 4 is in (plan `part4.md`, editor only): the **big editor** (`gradient_editor.rs`), a window opened by the
"Editor" button beside a main gradient row's bar (`gradient_bar_named(.., big)`; drawn by `gradient_tabs::tab_rows`,
so it lives while its row is drawn): a wide bar (`gradient_bar::bar_in`: its widgets under `bar.with("big")`, the
selected stop under the row's `bar`, so both select the same stop), Mix chips and Loop, the tools, a table of stops
(row, colour, hex, at, gap blend menu, gap middle, Remove) with Add colour (`gradient_tools::add_colour`), and
painted previews on a ball and a ring of 12 by Order; controls are named "{label} editor …". The **tools**
(`gradient_tools.rs`, a "Tools" row ending the Colours tab and in the window): Flip (`flipped`: `1 − at`, wrapped
when looped, each new gap the old gap before it mirrored, Ease in ↔ Ease out, middle `1 − m`; exact for middles at
0.5, an involution; its order vector permutes any `ValueType::Points` companion through `points_after_flip`),
Spread evenly, Random (`recoloured`: new `palette_gen` colours, places/gaps/alpha kept) and **Pick from picture**
(`eyedropper.rs`: `start`/`stop`/`target`/`deliver`/`take`, state in egui memory; `preview::pick_colour` turns the
preview into an eyedropper, a click reads `Renderer::read_output` and delivers `pixel_at`, Esc or right-click stops,
nothing else in the preview reacts; `tab_rows` puts the colour on the selected stop keeping its alpha; a colour
untaken after `STALE_PASSES` is dropped). A main gradient row's menu has **Copy gradient with its settings**
(`widgets::copy_with`, `Copied.extras` by companion name); Paste Value then also writes each carried companion the
target offers (`companions_pasted`).

## Particles thing (spec `docs/superpowers/specs/2026-11-06-particles-design.md`, plans `docs/superpowers/plans/particles/`)

Part 1 is in (plan `part1.md`): the **engine**. A **Particles** thing (`bpe-kit/src/things/particles/`, kind
`particles`, family "Particles") makes its own particles, separate from the `layer.particles` Look layer (which stays
as it is). Its own settings are the **Source** (`ParticlesSettings`: From Whole screen (Edge, Margin: the camera's shot
bounds) / An area (Rectangle, Circle, Ring) / A point / A line / Riding its parent (Each ball, The middle, Ball number,
The edge, Inside: the tree parent's balls or wall, `RIDES`); Emit Steady (Rate, a ball when riding each ball) / Bursts /
On hits (Who hits, Least speed, Follow impact) / Keep a crowd / Once at the start; each particle's Life, Speed,
Direction, Angle, Spread, Takes the parent's speed, Size, Turn, Spin, their spreads, Seed; Most particles (cap,
`MOST_EVER`)). Its cards are its own (`ModuleKind::fits`; `motion.basic`/`bounce.basic` don't fit it, and the inspector
shows them as cards of their own, not the Physics card): **Motion** `motion.particles` (`forces.rs`, force layers in
the new `LayerSection::Force`: Gravity, Wind, Drag, Swirl, Wander, Rise, Sway and flutter, each with Amount and Only
for, applied top of the list last), **Meets things** `meets.particles` in the Bounce slot (`meets.rs`: balls push
(`BallGrid`, at most `MOST_NEAR` 32 a particle) and Pass / Bounce off / Vanish, shapes Pass / Bounce / Stop / Vanish
through `keep_clear_from` against each wall's outline at the last particle step, the picture's edges Pass / Wrap round /
Bounce / Vanish) and **Look** `look.particles` (`look.rs`: draw layers Sprites (the `sprites.wgsl` shapes) and Streaks
(stretched by speed), common Where, Only (All / Younger than / Older than / A share / The rest), Colour from (Own paint
following life/order/place/speed/time, or the Parent ball's colour), Opacity, Size ×, Size over life, Fade, Twinkle,
Blend; sub-layers `particles.sub.glow/outline/shadow/recolour/flash/hue` with the shape sub-layers' settings). The
state (`state.rs`: `ParticlesState`, `Speck`s oldest first) lives in `World.states`, stepped in `after_step` at 1/120 s
(`step::catch_up`: the particle steps `due` by the world's time × 120, so lives keep time at any Physics rate; hits
since `ParticlesState.world_step` let particles off; `step.rs`: births, then the move in chunks of `CHUNK` 4096 on `std::thread::scope`, the same
for any thread count, then a stable `retain`); first births in `after_build`; every birth's dice from
`seed_of(world seed, thing, birth number, Seed)` (`births.rs`). `World::animate` calls the thing sim's
`settings_changed` for keys on cards with no module sim, so keyed forces work. The draw (`draw.rs`) draws nothing
itself and lists each shown Look layer as an `Around` (`draw_around` makes sprites through
`around_subs::particle_with_extras`); lighting fields stay 0 (part 7). **Decoration only**: `ThingSim::in_main` is
false, so `World.beside` lists it, `main_part(resolved, sims)` leaves it out, fingerprints hash the main part only,
and `beside_fingerprint` is what a particle playback is built from. The editor's main playback, the sound engine
(`Engine::with_patch`) and the aiming thread build from `main_part`; `bpe-studio/src/particle_job.rs` (`ParticleJob`)
runs the whole scene on its own thread, follows the playhead (`follow`) and publishes `Snapshot`s; the preview lends
their states to its world while it draws (`World::lend_states`/`take_back`), reads the latest snapshot once a
frame, steps the playback to its moment (`Studio::sim_target_with`; the job follows `aim_target`) and lends it only
when its step is the playback's (`particles_there`); "Particles… N%" shows past `PARTICLES_BEHIND` (0.25 s), and a
riding thing's handles use `World::anchor_as` (the main world doesn't build it). The status line shows "Particles: N, X ms".
Export (`bpe-cli`) builds the whole world. `looks.rs`: the Add thing menu's Particles heading (Empty, then
`GROUPS`: Rain, Snow, Hail, Blossom petals, Sandstorm; Fire on a ball, Spark fountain, Embers and ash; Fireflies,
Confetti; Rainbow smoke trail, Ink in water), `add_particles` (riding looks ride a Balls/Emitter/Shape parent).
Editor: `look_editor/particles.rs` (the Forces and Look lists; a Motion card's editor state is keyed apart,
`look_editor::pick_key`), `preview/particle_handles.rs` (area corner/edges, line end). Scene format 12. Goldens:
`tests/golden-particles*.txt` (`scenes::particle_scenes`, test-only).

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
- `bpe-kit`: **every plugin**, with one folder per thing kind in `src/things/` (balls, camera, folder, scene, shape)
  and one per card kind in `src/modules/` (look, motion, bounce, effects, lighting, light scenes). A ball's Look is
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
- **Light shows** (lighting part 6, plan `docs/superpowers/plans/lighting/part6.md`, draw-only): the **Rig** thing
  (`bpe-kit/src/things/rig/`, kind `rig`, family Lights, no Placement card, no sim; `add_rig` adds four spots) drives
  every enabled Light under it (`fixtures.rs`: tree order, nested rigs stack, `ridden` skips rigs so a rig inside a
  Balls group makes its lights ride the balls; `Across::EveryLamp` counts each ball's lamp). `pattern.rs` is pure in
  time: `Clock` from beats/bars/seconds (`beat_seconds`, the same as `TempoMap`) or heard hits (`hits_heard`, Least
  gap), `drive` gives a `Drive { strength, colour, turn, sway }` for Still, Chase, Strobe, Sweep, Alternate colours,
  Random. **Lighting scenes** (`bpe-kit/src/modules/light_scenes.rs`, core `Value::LightScenes`/`LightingScene` in
  `bpe-core/src/light_scenes.rs`, the Lighting card's `scenes`): snapshots of every Light's and Rig's settings
  (`capture`, minus `STRUCTURE`), shown Off / On the timeline (`show_scene`, a hold key) / In turn / On hits with a
  Crossfade (`now(cx)` → `ScenesNow::params`); edits renumber Show scene's keys. `LightDraw::light` applies the scene,
  then the rigs. `DrawCx.tree` is the saved `Scene` (tree and tracks). Editor: `light_scenes_panel.rs` under the
  Lighting card's grid (`Edit::LightScene`).
- **Effects stack** (lighting part 8, plan `docs/superpowers/plans/lighting/part8.md`): the scene's Shader slot
  holds one card, `effects` (`modules/effects/`), whose layers (section `LayerSection::Effect`) are the effects
  applied bottom first: `picture.<id>` per built-in (`bpe_gfx::effects::BUILTINS`, 26) and `picture.own` (own
  code, its settings from its header through `dynamic_defs`, checked by `bpe_gfx::picture_problem`). Each takes
  Mix (unclamped), Blend, Takes (Whole picture / Light only / Glow only, `TakesChoice`; picture stage only) and Glow
  above, then the header's own settings under plain keys (at most 6 Simple, `with_simple_room`). `EffectsDraw`
  pushes one `EffectCall` per shown layer. Old `shader.<id>` cards become a one-layer stack on load
  (`effects/upgrade.rs`, `add_upgrade(u32::MAX, single_effect_to_stack)`). Header lines gain `hard [a, b]`,
  `// choice key = default [a, b, c]` and `// memory: <seconds>` / `// memory: fade <key>` (`memory_seconds`:
  frames until under 1/4096, at most `MAX_MEMORY` 10 s); the prelude gains the ball list (`ball_count`, `ball_uv`,
  `ball_radius`, `ball_color`, `ball_hit`, `ball_seed`, `nearest_ball`, from `Frame.ball_marks`, pushed by the
  balls' draw only when the scene has effects), the beat (`fx.beat`, `beat_phase`, `bar_phase`, `kick`, from the
  scene's tempo, `things::scene::draw::beat_at`) and `keep`/`kept` (a number per pixel in the memory picture).
  Around layers **Electric arcs** (`layers/arcs.rs`, `layer.arcs`: seeded bolts between balls within Reach, firing
  always / on hits / on the beat, lighting a lit scene with strip lights through `ThingDraw::light`,
  `light::draw::push_strip`, at most `MOST_ARC_LIGHTS`) and **Aura field** (`layers/aura.rs`, `layer.aura`:
  contour lines of the balls' summed field by marching squares). A Light's **Negative** flips its strength
  (`fs_light` clamps the summed light at 0). The editor's Effects card is `look_editor/effects.rs`; after a jump
  in time, `preview/warm.rs` replays the effects' remembered seconds on a spare renderer
  (`Playback::rewound`), 6 frames per editor frame, and swaps it in ("Warming up effects… N%").
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
  dropped near it (`preview::seat_near`); the timeline ticks its rounds. While an emitter aims Through bounces,
  `aim_job.rs` (`AimJob`, `Studio.aim_job`) runs the scene to its end on its own thread, filling the aim cache; the
  playback never steps past it (`Studio::sim_target`) and the preview shows "Aiming… N%" (`Studio::aiming`) and the
  latest round's found paths. The
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
