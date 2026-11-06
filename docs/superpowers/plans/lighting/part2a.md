# Lighting part 2a: Stage 3D and base surfaces

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), built on part 1a
(`part1a.md`, merged as #106). Built straight after this plan (no approval stops), one Opus review at the end,
then the PR, merged by the thread itself once tests and the review pass (owner's standing instruction for the
overnight lighting run, 2026-11-06). A short video and stills go to project files `ballphysics/lighting/part2a/`.

## What 2a delivers

- **Stage 3D** on the Lighting card (Depth: **Flat** / **Stage**). The physics stays 2D. Balls are spheres centred
  on the picture plane (z = 0, z toward the camera), the background is a **back wall** at z = −Back wall, an
  optional **floor** runs under them, shapes stand out from the wall by **Shape depth**, lights sit at z = their
  Height. The existing ball looks are the impostors: they draw exactly as in Flat, and the light pass gives each
  pixel its 3D point and normal.
- **3D shadows**: balls shadow by analytic sphere tests (soft, from the lamp's size), shapes by their outline
  extruded from the wall to their front (the 2D blocker field marched with the ray's depth), onto the wall, the
  floor, the balls and the shapes. **Contact shading** in Stage is sphere ambient occlusion plus 1a's near-blocker
  darkening.
- **The floor**: None / At the bottom (of the picture) / Under the lowest shape / At a height; **Looking down** (how
  far above the floor the stage is seen from); Floor surface **Matte / Glossy / Mirror** (the **Glossy floor**
  idea: the floor reflects the lit picture, blurred by distance for Glossy); Wall surface Matte / Glossy.
- **Base surfaces** for balls and shapes: **Matte, Plastic, Chrome, Metal, Glass, Toon** in a **Surface** section at
  the top of the Look card (whole thing), each a preset of the same editable numbers: Roughness, Metal, Clear coat,
  Bends light, See-through, Toon steps, Reflects (A sky gradient / Nothing), and **Bulge** (moved here from the
  Body's Lighting fold). Surfaces work in Flat and Stage.
- **Sky** on the Lighting card's Room: Sky light (diffuse light from a top/bottom gradient) and its two colours,
  which also make what Chrome, Metal and Glass reflect.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **Camera and the floor.** The spec keeps the default Stage camera "orthographic front, same framing as Flat" and
   leaves tilt to part 5. Seen exactly from the front a floor is edge-on and invisible, so 2a sees the stage planes
   through a small **oblique** projection: a point (x, y, z) shows at (x, y + z·k), k = tan(Looking down). The
   picture plane (z = 0) is untouched, so balls, shapes' fronts and everything drawn are exactly where Flat draws
   them; only the wall and floor are read through k: a wall pixel at (x, y) is the wall point (x, y + D·k, −D); the
   floor y = F meets the wall at the **seam** y = F − D·k and a floor pixel at (x, y ≥ seam) is (x, F, (y − F)/k).
   Balls resting on the floor line touch the floor at z = 0. With no floor, k = 0 (a plain front view). Part 5's
   real camera moves replace this; the setting stays as the floor's view in the default camera.
2. **Floor and wall colour** are the scene's background where it shows (the background *is* the wall and floor);
   they differ by how light falls on them (normal toward the camera vs up), the seam's contact shading and the
   floor's reflection. A separate floor colour is not added (the background's gradients already paint top and
   bottom differently).
3. **Pixels of the stage vs things.** The surface picture's alpha says how much of a pixel things cover. In Stage a
   pixel covered by things is lit at its own 3D point; uncovered, at the wall or floor point; partly covered (edges),
   both, mixed by the cover. Trails, particles and other sheets that don't block light write no cover (as in 1a),
   so where they lie over the stage they take the stage's light: a known gap, noted for 2b (they are lit as if on
   the wall).
4. **Which surface a pixel has.** The surface picture's spare **B channel** carries a **surface index** (premultiplied
   like the rest; read back as `round(B / A)`). Every record of a thing carries its index in `LayerGpu.light[3]`;
   index 0 is a plain sheet (trails, particles, ghosts, things with no Surface). The index is the thing's place
   among the resolved things that have a Look card (1-based, `surface_index`), so the draw (records) and the light
   hook (the table) agree without shared state. The table (`LightScene.surfaces`) is filled by the Balls, Emitter
   and Shape light hooks. At an edge between two things of different surfaces one pixel can take the other's
   surface (a 1 px seam inside highlights only): accepted.
5. **How much of a pixel takes light (for highlights).** The lit picture is colour × Lit; the share is
   `max(W) / max(C)` (1 on a thing pixel that is black, so a black plastic ball still shines). Lit 0 layers keep
   their colour and get no highlight.
6. **Shading.** Per light: Flat keeps 1a's diffuse exactly (`max(1 + n·l − l.z, 0)`); Stage uses Lambert `max(n·l,
   0)` with the 3D distance for falloff. Spots keep 1a's cone measured across the picture in both. Specular is GGX
   (Smith-Schlick G, Schlick F), times π to match the diffuse's scale, with the roughness widened by the lamp's size
   (Karis' sphere-light trick) so tiny lamps on chrome don't alias. F0 = `((n − 1)/(n + 1))²` from Bends light,
   mixed to the base colour by Metal. **Matte's preset has Bends light 1.0**, so F0 = 0: no highlight, no reflection,
   exactly 1a's picture in Flat (a test pins it). Metal takes away the diffuse. Clear coat is a second lobe
   (roughness 0.05, F0 0.04) that dims what is under it by its Fresnel. Reflects: A sky gradient reflects
   `sky(R blurred toward N by roughness) × (ambient + Sky light)`; Nothing reflects nothing. **Toon** (Toon steps >
   0) bands each light's diffuse into steps and makes the highlight a hard dot. The mix is
   `C + (D − 1)·W·(1 − metal) − W·metal + share·S`, D the diffuse light (ambient, sky, lights), S the specular and
   reflected light; clamped to 6e4 as 1a.
7. **Glass in 2a** has no refraction (2b: it needs the picture behind the ball). It is a dark, clear-coated, sharply
   highlighted surface with strong Fresnel reflections whose See-through takes away diffuse and lets light through:
   a See-through ball's shadow is lighter and tinted by its Body colour (coloured shadows). Stated so nothing is
   silently dropped.
8. **Spheres.** The Balls and Emitter light hooks push every alive ball: centre, the body radius the physics uses,
   See-through and its Body colour (linear, the thing's Body colour, not each ball's varied colour). A ball's 3D
   point is (x, y, its drawn height) with 1a's normal from the height's slope, so any Body shape and Bulge keep
   working; its shadow uses the sphere. A sphere skips itself (the pixel lies on or inside it).
9. **Crowds.** A uniform grid over the spheres is built on the **CPU** each frame (`lighting.rs::SphereGrid`, about
   one cell per sphere, cells at least twice the largest radius plus a margin of twice the largest radius for
   penumbras and contact shading, at most 256 × 256 cells), uploaded with the spheres. A shadow ray walks it only over
   the stretch where its depth is within a ball's reach of the picture plane (and inside the grid), in half-cell
   steps (at most 512). The spec said "built on the GPU"; the CPU build is deterministic, cheap for thousands of
   balls and needs no compute pipeline. Penumbras wider than the margin are cut at cell edges: accepted, noted.
10. **Shapes in 3D.** In Stage, layered balls write no blocking in the surface picture (`Globals.light.z` = 1 tells
    the walk), so the jump-flood field holds only flat blockers (shapes and anything else with Blocks light on).
    That field, extruded from the wall (z = −D) to the shapes' front (z = Shape depth − D), is the shapes' 3D
    occluder: the shadow march of 1a runs with the ray's depth and stops where the ray rises past the front (or
    leaves the wall's depth). Shadow Length is Flat only. Shapes' fronts face the camera (normal (0, 0, 1)); their
    sides don't show in the front view.
11. **Glossy floor.** A second pass (`fs_floor`) after the light pass, only in Stage with a floor whose surface isn't
    Matte: on floor pixels it mixes in the lit picture mirrored about the floor line (floor in front of the picture
    plane, what stands on it) and, where nothing covers that mirrored pixel, about the seam (the wall). Fresnel of
    the constant oblique angle (F0 0.04) times Reflection for Glossy, 0.9 × Reflection for Mirror. Glossy blurs
    with the distance from the mirror line (Floor blur, 8 taps). Reflections fade off the picture's edge.
12. **Lights in Stage.** A light's Height is its depth (z toward the camera); default 160 px, so lamps hang in front
    of the balls; a negative Height puts a light between the balls and the wall (rim light). A Sun comes from its
    Direction lifted by its Elevation toward the camera.
13. **Defaults.** Depth Flat (1a's picture unchanged). Stage: Back wall 240 px, Shape depth 240 px (shape fronts
    level with the balls' middles), Floor Under the lowest shape (falls back to the bottom of the picture when there
    is no shape), Floor at 600 px (At a height), Looking down 14°, Floor surface Glossy, Floor blur 0.25, Reflection
    1, Wall surface Matte. Room: Sky light 0, Sky top `#CFD8E8`, Sky bottom `#2A2724`. Surface: Matte (Roughness 1,
    Metal 0, Clear coat 0, Bends light 1.0, See-through 0, Toon steps 0, Reflects A sky gradient, Bulge 1).
    Presets: Plastic (0.3, 0, 0, 1.5, 0, 0), Chrome (0.05, 1, 0, 1.5, 0, 0), Metal (0.35, 1, 0, 1.5, 0, 0), Glass
    (0.02, 0, 0.6, 1.5, 0.85, 0), Toon (0.35, 0, 0, 1.5, 0, 3). Choosing a Surface writes its preset numbers in the
    same edit; the numbers stay editable (the choice then just names where they started).
14. **No limits.** Every new number is a `span` except where the maths breaks, each with a reason: Back wall and
    Shape depth `range(0, …)` (a wall in front of the balls, or a shape sunk into the wall, means nothing); Looking
    down `range(0, 89)` (at 90° the wall vanishes; the shader uses at least 0.5° so the floor has area); Toon steps
    `range(0, …)` as a count (0 = smooth); Bends light `range(1, …)` (below 1 the Fresnel maths turns inside out);
    Roughness is stored as typed, clamped to [0.02, 1] in the shader (the spec's rule for negative roughness).
15. **Depth's names** become `flat` and `stage` (1a's `flat2_d` showed as "Flat2 d"); no upgrade (no files rely on
    it; the owner allows breaking old files).
16. **Lighting card fits** stay as 1a. New card sections "Stage" and "Surface" join `SECTION_ORDER` ("Stage" after
    "Room", "Surface" before "Lighting").
17. **Deferred** (named so nothing is dropped): any Surface on the floor and wall beyond Matte/Glossy/Mirror, the
    Surface sub-layer, advanced surfaces, live reflections of the scene in balls, refraction (all 2b); lit sheets on
    the picture plane (2b); Rainbow split (2b); Stage camera moves (5); the spec's GPU-built grid (decision 9).

## Files and types

### bpe-core
- `settings.rs`: `SECTION_ORDER` gains "Surface" (before "Lighting") and "Stage" (after "Room").

### bpe-gfx
- `frame.rs`:
  - `LightingGpu` gains `stage: [f32; 4]` (1 when Stage, back wall depth D, shape front z, k), `floor: [f32; 4]`
    (floor kind 0 none / 1 at a world y / 2 at the picture's bottom, floor y, floor surface 0/1/2, wall surface 0/1),
    `floor_more: [f32; 4]` (blur, reflection, 0, 0), `sky_top: [f32; 4]` (linear rgb, Sky light),
    `sky_bottom: [f32; 4]`.
  - `SurfaceGpu { a: [roughness, metal, clear coat, see-through], b: [F0, toon steps, reflects (0/1), kind] }`
    (kind 0 sheet, 1 ball, 2 shape), `SurfaceGpu::SHEET` (matte), `SurfaceGpu::matte()`.
  - `SphereGpu { a: [x, y, radius, see-through], b: [r, g, b (linear body colour), 0] }`.
  - `LightScene` gains `surfaces: Vec<SurfaceGpu>` (index 0 the sheet) and `spheres: Vec<SphereGpu>`;
    `LightScene::set_surface(index, SurfaceGpu)` grows the table with sheets.
- `lighting.rs`: a second bind group (group 1, "stage": uniform, spheres, grid cells, grid items, surfaces), a
  second pipeline layout for the light and floor passes, `fs_floor` pipeline and a `stage` HDR target (made only when
  a frame needs the floor pass), `SphereGrid::build(&[SphereGpu]) -> SphereGrid { params, cells, items }`, the light
  pass writing into the stage target and the floor pass into `out` when reflecting.
- `lib.rs`: `light_globals` sets `Globals.light.z` = 1 in Stage.
- `shaders/lighting.wgsl`: group 1 declarations; `fs_seed` unchanged; `fs_light` restructured: per pixel its
  surface (index, share), 3D point/normal (Flat as 1a; Stage thing/wall/floor), `shade_at` (lights: diffuse, GGX,
  clear coat, toon, shadows by `visibility` (Flat), `sphere_shadow` + `slab_visibility` (Stage)), sky and ambient,
  reflections of the sky, contact (1a's, plus `sphere_ao` in Stage); `fs_floor`.
- `shaders/layers.wgsl`: `light_piece` writes B = the record's surface index; in Stage (`g.light.z`) a ball walk's
  layers write no blocking (`LIGHT_BALL` private flag set by the ball walk in mode 2).
- `shaders/layers_walk.wgsl`: sets `LIGHT_BALL`.

### bpe-kit
- `modules/look/surface.rs` (new): `SurfaceKind` (Matte, Plastic, Chrome, Metal, Glass, Toon), `Reflects`
  (ASkyGradient, Nothing), `SurfaceSettings` (derive: `surface`, `roughness`, `metal`, `clear_coat`, `bends_light`,
  `see_through`, `toon_steps`, `reflects`, `bulge`; group "Surface"), `SurfaceKind::preset()`,
  `preset_values(kind) -> Vec<(&str, Value)>`, `SurfaceSettings::gpu(kind_code)`, `surface_index(scene, id)`.
- `modules/look/layers/mod.rs` (`LayersLookKind::settings` → `SurfaceSettings::defs()`), `modules/look/shape/mod.rs`
  (the shape look kind likewise).
- `modules/look/layers/lighting.rs`: Bulge leaves the Body (BodyLit becomes Solid); `of` reads Lit and Blocks.
- `modules/look/layers/gpu.rs`, `things/balls/draw.rs`: the body record's `light[2]` = the card's Bulge; every
  record of the thing gets `light[3]` = its surface index (ghosts, mini balls, particles keep 0).
- `modules/look/shape/gpu.rs`, `things/shape/draw.rs`: shape records get the index.
- `things/balls/draw.rs` + `things/emitter/draw.rs`: `light` hook → surface table entry and spheres
  (`push_ball_light`).
- `things/shape/draw.rs`: `light` hook → surface table entry (kind 2).
- `modules/lighting.rs`: `Depth { Flat, Stage }`, `Floor`, `FloorSurface`, `WallSurface`; settings Back wall, Shape
  depth, Floor, Floor at, Looking down, Floor surface, Floor blur, Reflection, Wall surface (group "Stage", `when =
  "depth=stage"`), Sky light / Sky top / Sky bottom (Room); `gpu()` fills the new fields; `floor_line(world)` (the
  lowest shape's bottom) worked out in `things/scene/draw.rs`.
- `scenes.rs`: `stage_room` (test) and `stage_demo` (the video), test-only.

### bpe-studio
- `inspector.rs`: a Look card (balls and shapes) shows its Surface section (`look_editor::surface::show`) above the
  Look editor, folded like other sections.
- `look_editor/surface.rs` (new): the Surface picker as a row of chips with painted sphere swatches
  (`style::surface_swatch`), then the numbers' grid; choosing a Surface also writes its preset numbers in the same
  edit; a faint note when the scene has no Lighting card ("Surfaces show when the scene has lighting").
- `style/swatch.rs` (new): `surface_swatch(painter, rect, kind_index)` with its own tokens.

## Tests (what passing looks like)

- **No card**: `unlit_pictures` unchanged (hashes from before lighting).
- **Flat Matte is 1a**: `tests/golden-lit-flat.txt`, tile averages (27 × 48 tiles) of `lit_room` and `lit_demo` at two
  moments, blessed on main before any change here; Flat Matte renders match within 1/255 per tile.
- **Stage pictures** (`tests/stage_pictures.rs`, small renders on lavapipe):
  - a ball before the wall under a lamp up-left: the wall down-right of the ball is darker than the mirrored spot;
  - floor: the floor just under a resting ball is darker than floor far from it (shadow and contact);
  - Glossy floor: a red ball standing on the floor shows red below the floor line; Matte doesn't;
  - Plastic has a brighter highlight than Matte on the same ball; Chrome's top and bottom differ by the sky;
  - Toon: the ball's lit pixels fall in few brightness bands;
  - a Glass ball's shadow is lighter than a Matte ball's;
  - a shape casts a shadow on the wall in Stage (wall beside the shape, away from the lamp, darker than toward it);
  - a light behind the balls (negative Height) lights their rims, not their middles.
- **Kit units**: presets; Matte has F0 0; surface indices are 1-based by Look-card order and match between records
  and the table; Stage `gpu()` fields; Look cards resolve the Surface defaults; Body has no Bulge any more.
- **Grid units** (bpe-gfx): every sphere is found from every cell its padded disc touches; an empty list builds.
- Physics and sound goldens unchanged (no physics or sound code touched; run both).
- `cargo clippy --workspace`, `cargo fmt`, the editor's kittest tests (the Surface chips are named; design-system
  colour test).

## Things not to do

- No lighting code runs and no pipeline changes when the scene has no Lighting card.
- Don't change Flat's picture for Matte surfaces (the pinned tile test).
- Don't touch physics, sound or their goldens.
- No hard ranges for taste; no built-in scenes in the program; no colours outside `style/`.
- Don't keep anything between frames.

## After the review (as built)

- Fixed: lamps inside a ball or a shape's band shine out in Stage (the march stops short as in Flat; a sphere holding
  the lamp never blocks it); shadow rays walk the sphere grid cell by cell (Amanatides–Woo) and test at most 96 balls
  a cell, each ball listed out to its own radius plus `SPHERE_MARGIN` (2) radii; the "Under the lowest shape" floor is
  the shape's middle plus its farthest corner (spinning doesn't move it); a Body with Blocks light off casts no sphere
  shadow; blended edges between two surfaces read no surface rather than a third; the Stage rows show only in Stage;
  Copy / Paste Look carry the card's Surface (`CopiedLook.card`); highlights have a least width (no pixel sparkles).
- The Surface section lives in `inspector.rs` (`surface_section`, `surface_chip`) rather than `look_editor/surface.rs`.
- Known gaps for 2b: the oblique view shows the wall k × Back wall above a shape's drawn footprint (shape shadows sit
  that much high); a black Lit 0 layer on a shiny ball takes highlights (the light pass can't tell it from black Lit
  1); penumbras wider than two radii are clipped by the grid's margin.

## Merged with parts 1b and 1c

Part 1b split the light pass into `fs_direct` (lamps on the surface and in the air, into a picture) and `fs_mix`
(colour + (light − 1) × lit colour, then haze and blooms). 2a now plugs in at `fs_mix`: a pixel whose surface shines
(`shines`) or any pixel on a Stage is shaded by `surfaced` (part 2a's `shade` for the thing and the wall or floor, the
light field's light added to the diffuse); every other pixel takes 1b's path untouched. The stage's bindings are
group 2 of the mix and floor passes. `tests/golden-lit-flat*.txt` were blessed again on main after 1b and 1c (whose
light changed Flat pictures), so they still pin "2a changes no Flat Matte picture". Known gaps: in Stage the light
field still sees balls as clear (they don't write the blockers' field), and a background that gives off light is not
added on Stage or shiny pixels.

## Merged with part 8

Part 8 made a Light able to be negative and gave the balls' draw a `light` hook for electric arcs; the balls' one
`light` now pushes the Stage spheres and surface first, then the arcs. On shaded pixels `with_field` keeps the
summed light (lamps, room, field) at 0 or more, as 1b's Flat mix does.
