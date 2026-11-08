# Lighting (spec `docs/superpowers/specs/2026-11-06-lighting-design.md`, plans `docs/superpowers/plans/lighting/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

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
radius and how deep in a blocker it sits) and normals from the height: `C + (L − 1) × clamp(W, 0, C)`. Thin blockers block
whole (plan `lighting/thin-blockers.md`): `fs_seed` keeps a field texel any blocking pixel touches (`foot_blocks`,
`BLOCK_LEAST` 0.1; 0.3 on a Stage, whose balls mark a quarter) and closes diagonal gaps; `visibility` marches with
`march`/`confirm` (the picture itself checked where the coarse field says "inside") and, when the middle ray crosses
a blocker, `LAMP_RAYS` rays across the lamp (union of their clear stretches); `tests/thin_blockers.rs`. See-through
blockers let their share through in Flat (plan `lighting/see-through-blockers.md`): the surface picture's R is blocks ×
opacity with the antialiased edge taken whole (`scene.wgsl::hard_block`, `AA_COVER` from trails, `layers.wgsl::surface_whole`
for balls and shapes; readers wanting the old value use `cover_block` = min(R, A), so the blend composite adds the layer's cover to the surface
picture's alpha in Flat), a Surface's See-through is folded into
`LayerGpu.light[1]` by `surface::Stamps::apply` (`keeps_of`, never under 1/1024), the field's G is the nearest blocker's opacity (`blocker_opacity`), and
`march` walks through a pixel under `SOLID` (0.98), multiplying `Ray.lit`; `visibility` gives `1 − ō(1 − shown)`;
Stage is unchanged; `tests/see_through_shadows.rs`. No Lighting
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

## Light shows and the Effects stack

(Lighting parts 6 and 8; moved from the crate map.)

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

**Haze With the air (W13)**: `HazeMoves::WithTheAir` (shader code 1, the Drifting clouds) drifts as far as the
scene's air carried them (`Air.drift`, the ambient wind × dt summed each step by the scene sim) × Speed; Still
without an Air card. `LightingSettings::haze_drift` works the offset out on the CPU for every mode (Drifting and
Swirling: direction × Speed × 100 px/s × time, as the shader did) and `SceneDraw` puts it in `haze_more.xy`; the
shader subtracts it (`haze_density`). While the haze moves with the air the scene sim `reads_air`
(`bpe_runtime::air_read`): every world senses the Air keys and the physics fingerprint counts the Air card and its
keys, so an Air edit restarts the playback whose air it drifts with (the sound's too, as its fingerprint includes the
physics'). Haze gusts don't travel as waves.
