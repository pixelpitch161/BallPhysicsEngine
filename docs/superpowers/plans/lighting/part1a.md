# Lighting part 1a: lights, the Lighting card, direct light and shadows (Flat 2D)

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved). This part builds the base every later
lighting part stands on. Built straight after the plan (no approval stops), one Opus review at the end, then the PR,
merged by the thread itself once tests and the review pass (owner's standing instruction, 2026-11-06).

## What 1a delivers

- **Light things** (`light`, family Lights): Point, Spot, Sun, Area, Strip. A light inside a Balls, Emitter or Shape
  thing rides on it (Each ball / The middle / Ball number / A point on the edge). Every number keyframes and can
  Follow sound (all marked `keyframe`).
- **The scene Lighting card** (`scene.lighting` in the new `SlotId::Lighting`, scene only): Room (Depth: Flat 2D,
  Ambient colour and strength, Background takes light), Shadows (Style Soft / Hard / Toon / Off, Softness, Length,
  Contact shading and its reach, Toon steps), Quality (Draft / Good / High / Ultra).
- **Direct light with soft shadows** on the GPU: a surface picture of every thing, a signed distance field of what
  blocks light (jump flood), each light marched through it for soft 2D shadows, then the mix
  `colour + (light − 1) × lit colour` before the picture effects, bloom and grade.
- **Lit and Blocks light on every drawn and around layer** (ball layers, shape layers, trails, particles, wavefront),
  and **Roundness** (0 flat disc, 1 full sphere) for a ball's shading normal.
- **Scene format 11.**
- **The no-card test**: scenes with no Lighting card render byte-identical to main (hashes made on main before any
  renderer change).
- A **lighting time readout** in the status line (GPU timestamps where the device has them, else the light count).

## Decisions made while planning (each a fill-in of the spec, stated here)

1. **The Light card is the light thing's own settings** (like the Camera's own card), not a slot card: a light is
   nothing but its light, so it needs no `SlotId::Light` and no module kind. The inspector shows it as the thing's
   card titled "Light". The light's place is its own keyframable **Position** setting (like the Camera), so the
   Placement card is hidden for lights; when riding, Position is the offset from what it rides.
2. **Riding is read from the tree parent** (the spec: "the kit reads the parent"), not from `Attach::Ride`, so a light
   dropped into a Balls/Emitter/Shape row rides it. Adding a light while a Balls, Emitter or Shape thing is selected
   puts the light inside it. A light whose parent is the root or a folder sits at its Position.
3. **Shadows in Flat are 2D occlusion through a signed distance field of the blockers** (every pixel of every layer
   with Blocks light on). This one field serves balls and shapes alike, so crowds need no grid in 1a; the analytic
   sphere shadows the spec mentions come with Stage (2a), where they are 3D. Shadow **Length** is how far behind its
   blocker a shadow reaches (Flat). The march skips out of the blocker the pixel is on (a ball shades itself by its
   normal, not by its own shadow) and stops short of the lamp by its Size and, when riding, its ball's radius (a
   torch inside a ball isn't blacked out by that ball).
4. **Penumbra**: visibility is `min` over the march of `0.5 + 0.5 · h·D / (t·s)` (h signed distance, t along the ray,
   D the lamp's distance, s its Size × light Softness × scene Softness). Hard style uses s = 0.5 px; Toon is Hard plus
   the light banded into Toon steps. Contact shading darkens light near blockers by their distance.
5. **The lit colour picture**: exact for normal blending — the frame is drawn a second time with every layer's colour
   (and its added light) scaled by its Lit, so `W = Σ lit_i · contribution_i`. A third draw writes the surface
   (R blocks, G height, A cover). Both reuse every existing pipeline with a new `Globals.light` mode (0 the picture as
   today, 1 lit colour, 2 surface). Things-stage effects and materials run only in mode 0 (their pixels count as unlit,
   a known gap, noted for part 8). In mode 2 blend layers composite as Lighten (blocks and height keep their max).
   The mix clamps W to [0, colour] per channel, so no blend mode can push a pixel negative.
6. **Light added by sub-layers** (a Glow sub-layer's light) is never lit; a Glow layer's own light is scaled by its Lit
   (default 0). Sub-layers otherwise go with their layer's Lit.
7. **Height and normals**: in mode 2 a layered ball writes height `radius · roundness · sqrt(1 − (1 + d)²)` (d the
   ball shape's signed distance in ball units, so a circle gives the exact sphere); the light pass takes the normal
   from the height's slope. Shapes, trails, particles and the background are flat (height 0). Roundness (named **Bulge** in the editor, key `bulge`, because a Body already has a Roundness for its corners) sits in the
   **Body layer's Lighting fold** (the ball's surface) until 2a's Surface section takes it over.
8. **Lit defaults** (`Kind` in the kit): 1 for Body, Pattern, Outline, Shading, Shader, Trail, Particles, Wavefront, shape
   body, Colour and gradient, shape Outline, shape Shader; 0 for Glow, Shine, shape Glow. **Blocks light** defaults on
   for the ball Body and the shape body, off for the rest. Blend layers, reactions, sub-layers and group layers get
   no Lighting fold. Settings keys: `lit`, `blocks_light`, `roundness` (Body only), group "Lighting".
9. **Default values**: light Strength 1.5, Reach 900 px, Falloff Real, Size 24 px, Height 160 px, colour warm white
   `#FFF2DE`; Spot Cone 50°, Cone edge 15°, Direction 90° (down; degrees clockwise from +x, as gradients); Sun
   Elevation 35°, Sun size 2°; Area 240×120 px; Strip length 400 px, width 8 px. Lighting card: Ambient 0.25 white
   (the spec's "adds the card with Ambient 0.25"), Background takes light 1, Shadows Soft, Softness 1, Length 2400 px,
   Contact 0 (reach 40 px), Toon steps 3, Quality Good.
10. **No limits**: every new number uses `span`, except: Reach, Size, Width, Length, Contact reach (`range(0, …)` with
    no top: negative sizes mean nothing), Cone width `range(0, 360)` (a cone wider than a full turn means nothing),
    Toon steps `range(1, …)` (zero bands divides by zero). Strength, Height, Flare and colours are unbounded; a
    negative Strength takes light away (the spec's Negative light falls out of the maths).
11. **Quality tiers** scale the field and the march: Draft (field ¼ size, 16 steps), Good (½, 32), High (½, 64), Ultra
    (full, 96). Live and export draw the same tier.
12. **Light count**: no cap in the data or the renderer; the status line warns past 256 lights ("lighting may be slow").
    Each light is skipped for a pixel past its Reach.
13. **Format 11** has no upgrade step: format 10 files open as they are (no Lighting card = today's picture).
14. **Deferred** (stated so nothing is silently dropped): Gradient-over-time and Picked colours, Drawn falloff curve,
    Lights up (linking), gobo/flicker (part 4), Air (1b), Special (4/8), "Points: At a thing" aims (in 1a: Fixed,
    Where it's going, Spins). Lights up and Picked colour go to part 1c's list (the Lights tab is where linking is
    edited); the rest to the parts the spec names.

## Files and types

### bpe-core
- `ids.rs`: `SlotId::Lighting` between `Shader` and `Sound` (`ALL` 9 long, Sound and Mixer still last); name "Lighting".
- `registry.rs`: `LayerKind::light_defs(&self) -> &'static [SettingDef]` (default `&[]`), included by `layer_def` and
  by resolve's layer defaults (`resolve.rs` loop over `settings().chain(dynamic_defs).chain(light_defs)`).
- `scene.rs`: `Scene::FORMAT = 11`; save tests updated.

### bpe-gfx
- `frame.rs`: `LayerGpu.light: [f32; 4]` (lit, blocks, roundness, 0), size 208; `TrailVertex.light: [f32; 2]`,
  `Particle.light: [f32; 4]` (lit, blocks); `LightGpu` (6 × vec4: place xy/height/kind; colour rgb/strength;
  reach/falloff/size/shadow strength; dir xy/cos inner/cos outer; width/length/first segment/segments; shadow colour
  rgb/casts + skip); `LightingGpu` (ambient rgb, background lit, style, softness, length, contact, contact reach, toon
  steps, quality); `LightScene { settings, lights, segments }` as `Frame.light: Option<LightScene>`;
  `ThingDraw::light(&self, cx, scene: &mut LightScene)` hook, called by `build_frame_with` only when the scene drew a
  `Frame.light` (the scene's draw sets it from the Lighting card).
- `lib.rs`: `Globals.light: [f32; 4]` (x mode, y background lit); `render` draws the picture as today, then, only with
  `frame.light`, draws modes 1 and 2 into the `lit` and `surface` targets (by swapping them in for the HDR picture and
  the globals for light-mode ones), runs `lighting::Lighting::run`, then the Picture stage, bloom and composite as
  before. `light_time_ms()` for the editor.
- `lighting.rs` (new): targets (seed ping-pong `Rgba32Float`, field `R16Float`, both at the tier's scale), pipelines
  (seed, jump, field, light), the lights/segments storage buffers, the uniform, optional timestamp queries.
- `shaders/lighting.wgsl` (new): seed, jump flood, field and light passes.
- WGSL: `Globals` in scene.wgsl and sprites.wgsl gain `grade`, `grade_more`, `light`; `light_piece`/`light_flat`
  helpers; the ball walk, shape walk, trails, sprites, background, paths without layers, old pipelines answer modes 1
  and 2. Mode 0 code is unchanged.

### bpe-kit
- `things/light/` (new): `mod.rs` (`LightKind` thing kind, `LightSettings`, choices `LightType`, `Falloff`,
  `LightColour`, `RidesOn`, `Points`, `AreaShape`), `draw.rs` (`LightDraw`: `ThingDraw` with an empty `draw` and the
  `light` hook turning settings + world into `LightGpu`s, riding, flares).
- `modules/lighting.rs` (new): `LightingKind` (`scene.lighting`, fits only the scene), `LightingSettings`, `ShadowStyle`,
  `Quality`, `Depth`; `LightingSettings::gpu()`.
- `things/scene/draw.rs`: sets `frame.light` from the Lighting card.
- `modules/look/layers/mod.rs` + `shape/mod.rs`: `Kind.light` (`LightDefaults`), `light_defs` from static defs
  (`lighting_defs.rs`: `LitOn`, `LitOff`, `BodyLit` derive structs); `Lighting { lit, blocks, roundness }` read from a
  layer's params (`Lighting::of(params, kind)`).
- `gpu.rs` (balls), `shape/gpu.rs`: write `LayerGpu.light` on each drawn layer's record (and the body's roundness).
- Around layers (`around.rs`): `AroundLayer.lighting`; `BallsDraw::draw_around` and `ShapeDraw::draw_around` stamp
  every trail vertex, particle and layered record that call added.
- `starters.rs`: adding a `light` adds the Lighting card to the scene when it has none.
- `lib.rs`: register `LightKind`, `LightingKind`, `LightDraw`.
- `scenes.rs`: `lit_room` test scene (lights, balls in a ring, a shape) for tests and the demo render.

### bpe-studio
- `tree.rs`: "Lights" family in Add thing listing Point light, Spot light, Sun, Area light, Strip light
  (`Action::AddLight(&str)`), added inside a selected Balls/Emitter/Shape; a lamp icon (rays) for lights.
- `inspector.rs`: no Placement card for lights.
- `look_editor/mod.rs`: a layer's rows include its `light_defs` (they show under the "Lighting" heading).
- `app.rs`: status line shows "Lighting N lights · x.x ms" when the frame is lit (warning past 256 lights).
- `main.rs`/device setup: ask for `TIMESTAMP_QUERY` when the adapter has it.

## Tests (what passing looks like)

- **No-card identical** (`bpe-kit/tests/unlit_pictures.rs`): renders the test scenes (`scenes::all()` subset, small
  size, three moments) and compares FNV hashes with `tests/golden-unlit.txt`, blessed on main *before* any renderer
  change. Runs on lavapipe (skips with a note on any other adapter, whose hashes differ).
- **Lit pictures** (bpe-kit): a ball under a Point light is brighter on the side facing the lamp; a shadow falls away
  from the light behind a blocking ball and not toward it; Lit 0 on a Glow keeps it exactly as unlit; a Spot lights
  inside its cone, not outside; a coloured light tints a white ball its colour; ambient 0 with no light leaves lit
  things black and Lit-0 things unchanged; a light riding Each ball makes one light per ball.
- **Core/kit units**: `SlotId::ALL` order; format 11 save/load; Lighting card fits only the scene; adding a light adds
  the card once; light defaults by kind (`lit` 0 on Glow, 1 on Body; `blocks_light` on Body only); records carry light.
- Physics goldens and sound goldens unchanged (no physics code touched; run both).
- `cargo clippy --workspace`, `cargo fmt`, the editor's kittest tests (tree menu lists the lights; design-system colour
  test).

## Things not to do

- No lighting code runs and no pipeline changes when the scene has no Lighting card (mode 0 shader paths unchanged).
- Nothing physical reads lights; lights have no sim.
- No hard ranges for taste; no built-in scenes in the program; no colours outside `style/`.
