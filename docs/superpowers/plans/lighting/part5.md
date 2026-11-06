# Lighting part 5: lens flares, lens streaks, colour fringes and Stage camera moves

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), part 5: **Lens flares**, **Anamorphic
streaks**, **Colour fringes** ("Lighting card or Effects stack entries") and **Stage camera moves** ("tilt, orbit,
height and lens on the Camera thing for Stage only"). Built on 1a, 2a (#109) and 8 (#112). Built straight after
this plan (no approval stops), one Opus review at the end, then the PR, merged by the thread itself once tests and
the review pass (owner's overnight instruction, 2026-11-06). A short video and stills go to project files
`ballphysics/lighting/part5/`. Part 2b and part 4 build at the same time: this part keeps out of `lighting.wgsl`
and touches `lighting.rs` only to hand its bind groups on.

## What part 5 delivers

- Three new built-in effects in the Effects stack (part 8's card), so they mix with everything, take Mix, Blend and
  Takes (Whole picture / Light only / Glow only), keyframe and Follow sound like any effect, and work with or
  without a Lighting card:
  - **Lens flare** (`picture.lens_flare`): ghosts thrown through the middle of the picture, a halo ring, and a
    starburst round every bright spot, all from the picture's own bright parts (so lamps, glowing balls, flashes,
    lasers and anything else bright flare, moving as they move).
  - **Lens streaks** (`picture.lens_streaks`): the anamorphic streak: every bright thing throws a long thin streak
    across (any angle; 2 points make a line through the light, more make a star filter), tinted, brighter on hits.
  - **Colour fringes** (`picture.fringes`): red and blue split apart toward the edges like a vintage lens, as a
    smooth rainbow (not three copies), wider on the beat and on hits.
- **Stage camera moves** on the Camera thing: a "Stage camera" switch and Orbit, Tilt, Height, Lens (mm), Turns
  round (the depth it orbits) and Edges (Zoom in to fill / Stretch). In a Stage scene the video is seen through a
  3D camera that swings round the stage: balls stay round spheres, the back wall and floor slide behind them with
  real parallax, shapes show their sides, trails and glows stay with the balls. Keyframes and Follow sound work on
  every number. Off, or not Stage, the picture is exactly as today.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **The lens effects are Effects stack layers**, not Lighting card settings: then they need no Lighting card, take
   the stack's Takes (so "Light only" flares only the light), order (a kaleidoscope after streaks folds the
   streaks; streaks after it streak the folded picture) and Mix/Blend for free. The spec said "after bloom"; in the
   stack they run where they are placed, before the scene's bloom, which then softens them a touch like a real
   lens. Stated, not silently changed.
2. **A lens pre-pass.** Streaks and flares need long blurs a single effect pass can't afford, so an effect header
   may say `// lens: rays`, `// lens: soft` or both. Such an effect defines `fn bright(uv, p) -> vec3<f32>` (what
   counts as a source; it reads `sample()`, so Takes works) beside `fn effect`. Before the call, the chain renders
   `bright` at half size, then:
   - **soft**: a mip chain of it (5 levels, the bloom's 13-tap downsample), read by `lens_soft(uv, level)`;
   - **rays**: for each of `rays` points at `ray_turn + i·360°/rays`, a one-sided exponential streak by Kawase's
     streak filter (pass n samples 4 taps 4ⁿ texels apart, weights aⁿ⁽ᵏ⁾, a = e^(−1/L), L the length in half-size
     texels from `ray_length` × the half picture's height; N = ⌈log₄ 4L⌉ passes, 1..8), energy-normalised by
     (1 − a), all points added into one picture, read by `lens_rays(uv)`.
   The pre-pass reads the ray settings by name (`rays`, `ray_turn`, `ray_length`) from the header; missing keys
   mean no rays. Own code (`picture.own`) can use the same header lines. The live view draws at most
   `MOST_LENS_RAYS` (32) points (the data keeps what is typed; the spec's rule for counts).
3. **Prelude gains** bindings 8 (`lens_rays_tex`) and 9 (`lens_soft_tex`) and `lens_rays(uv)`/`lens_soft(uv,
   level)`; an effect without a lens line reads black. Existing effects are untouched (the binding is unused).
4. **Stage camera = re-viewing the lit stage.** Lighting stays exactly as it is (worked out in the front view, the
   one every light pass, shadow field and the light field already use). A new pass after lighting, before the
   picture effects, casts a ray per pixel from the moved camera into the stage and colours each hit from the lit
   front picture. This keeps part 5 out of `lighting.wgsl` (2b and 4 are changing it) and makes every later
   lighting feature move with the camera for free. True 3D re-lighting from the moved camera (view-dependent
   highlights moving across chrome) is left to the bake (part 3); stated.
5. **The rest camera.** The front picture of 2a is an oblique view (a point (x, y, z) shows at (x, y + z·k)). The
   stage camera's rest pose is the off-axis perspective camera that frames the picture plane exactly as Flat and
   sits k·dist above the middle (dist from the Lens), which tends to 2a's oblique view as the lens gets long. Every
   hit is coloured from the front picture at **where the rest camera saw it** (`uv_rest`: the hit projected from the
   rest camera onto the picture plane). So with no move (Orbit, Tilt and Height 0) the picture is the front
   picture exactly, at any Lens, and moves change it smoothly. The Lens therefore sets how strongly depth shows as
   the camera moves (wider shows more); at rest it changes nothing. Stated in its help.
6. **What a ray hits** (nearest first; world x right, y down, z toward the camera; the picture plane z = 0):
   - **balls**: the stage's spheres (2a's `SphereGpu`, through 2a's sphere grid, an Amanatides–Woo walk over the
     stretch where the ray is within the largest radius of the picture plane);
   - **shapes**: the blockers' field (2a's jump flood, world px) extruded from the wall (z = −Back wall) to the
     shapes' front (Shape depth − Back wall), sphere-traced (at most 64 steps). The front face takes the front
     picture; a **side** takes the front colour just inside the edge (2 px along −∇field) × `SIDE_SHADE` 0.62,
     faded in by the move amount (decision 9) so the rest pose stays exact;
   - **the back wall** (z = −Back wall) and **the floor** (y = the floor line, in front of the wall), as 2a places
     them; with no floor the wall goes all the way down;
   - **the picture plane** (z = 0) on the way: everything flat drawn there (trails, particles, glows, wavefront,
     arcs) as a see-through sheet over whatever the ray hits behind it.
7. **Splitting the front picture into wall and sheets.** One more cheap draw, the **background alone** (the
   scene's background pipeline, unlit, as drawn under the things), against the picture before lighting tells how
   much of each pixel is things: a = max(surface cover, α), α = max channel of |unlit − background| over the larger
   of the two (0 where nothing is drawn over the background, exactly, since both draws are the same shader). The
   **wall picture** W is the lit picture where a < 0.02, filled in under everything else by a full-resolution
   pull–push (premultiplied 2 × 2 averages down to one texel, then each finer level keeps what it knows and takes
   the coarser one where it doesn't). The sheet at a front pixel is `lit − (1 − a)·W` (premultiplied), and a ray
   crossing the plane there shows `sheet + (1 − a)·behind`. In the front view `behind` is W at the same pixel, so
   the sum is `lit` exactly; moved, the wall slides behind the sheets. Glows over a black wall come out as sheets
   (α = 1) and ride with their balls; haze and light blooms (added by the light pass, not drawn) stay with the
   wall.
8. **Edges.** The front picture only holds what the front view saw, so a moved camera can look past its edges (or
   in front of the picture's floor). **Edges: Zoom in to fill** (default) zooms the moved view in, as little as
   needed, so every border ray's plane point and wall/floor point land inside the front picture (the CPU checks 16
   border rays and bisects the zoom over 1..8 in 24 steps). **Stretch** keeps the framing and lets the front
   picture's edge stretch. Zooming in costs sharpness (the front picture is magnified); stated in the help.
9. **The move amount** m = clamp(max(|Orbit|, |Tilt|)/2° + |Height|/20 px, 0, 1). At m = 0 every look-up is the
   exact `uv_rest`; as m rises, sphere look-ups are pulled 1.5 px inside the ball's drawn edge (so no background
   fringe rings a turning ball) and shape sides take their shaded colour. The back of a ball (seen past ±90°) takes
   the front's colour mirrored.
10. **Anti-aliasing.** One ray at the pixel's middle; where the four rays at its corners (shared rotated grid) don't
    all hit the same thing (ball id, shape, plane/wall), the pixel averages its middle and corner rays (5).
    Draft: middle only. Every look-up at rest lands on the pixel's own centre, so the rest pose is not blurred.
11. **Camera maths** (`bpe-gfx/src/stage_camera.rs`, shared by the renderer and the editor). Front basis: right
    r₀ = (cos turn, sin turn, 0), down d₀ = (−sin turn, cos turn, 0). Shot height H = video height / zoom, fov from
    Lens (35 mm full-frame on the picture's long side: fov = 2·atan(18/lens)), dist = (H/2)/tan(fov/2) (lens clamped
    to 1..10 000 mm in the maths for f32). Rest origin C₀ = (cx, cy − k·dist, dist). Moved camera before turning:
    C_r = (cx, cy − k·dist − Height, dist), ray(s) = (0, 0, −dist) + r₀·sx/(zoom·f) + d₀·sy/(zoom·f) + (0, k·dist +
    Height, 0) for screen units s (1080 across, middle 0, y down) and fill zoom f. Then R = R_y(Orbit) · R_x(Tilt)
    about the pivot P = (cx, cy, Turns round): origin P + R(C_r − P), direction R·ray. Positive Orbit swings the
    camera right, positive Tilt raises it (looking down more). `uv_rest(X)`: C₀.xy + (X.xy − C₀.xy)·dist/(dist −
    X.z), then the front view's world→uv. The editor maps the pointer through the same camera onto the picture
    plane, so handles and clicks land where things are drawn.
12. **Where the settings are.** On the Camera thing, group "Stage camera" (after "World box"), every number
    keyframed: Stage camera (switch, default off), Orbit (°, span −45..45, hard −89..89: at ±90° the camera looks
    along the picture plane and past it would look at the back of a wall that isn't there), Tilt (°, span
    −30..30, hard −89..89, same reason), Height (px, span −500..500), Lens (mm, span 12..200, hard 1..1e9: no lens
    is 0 mm or less; default 35), Turns round (px, span −500..500, default 0: the picture plane; negative is toward
    the wall), Edges (Zoom in to fill / Stretch). The camera's light hook (`CameraDraw`, a draw that draws nothing)
    puts them on `LightScene.camera` when the switch is on and the Lighting card is Stage; the renderer solves the
    view. The World view of the editor never uses it.
13. **Takes Light after a camera move.** Effects on the light read the picture before lighting and the lit colour;
    the camera pass re-views those two as well (at the nearest solid hit's look-up), so Light only still lines up.
14. **No change without it.** No Lighting card, Flat, or the switch off: no camera pass, no background draw, no new
    texture. The new effects run only when a scene has them. Unlit and lit hash tests stay as they are.

## Files and types

### bpe-gfx
- `effects/mod.rs`: `Header.lens: Lens` (`Lens { rays: bool, soft: bool }`, from `// lens:`; unknown words are an
  error), `assemble_bright(wgsl)` (the prelude, the file and a fragment entry returning `bright`), BUILTINS 29
  (`lens_flare`, `lens_streaks`, `fringes` after `gravity_lens`), the doc comment and the count tests.
- `effects/prelude.wgsl`: bindings 8/9 and `lens_rays`, `lens_soft`.
- `effects/lens.rs` (new): `LensPass` (pipelines `fs_soft_down`, `fs_ray`; ray uniforms pooled; textures made at
  half size on first need) with `run(device, queue, encoder, bright: &View, rays: Option<RaySettings>, soft:
  bool)`; `RaySettings { count, turn_deg, length }`, `ray_passes(length_texels) -> (a, n)`.
- `effects/lens.wgsl` (new): the soft downsample and the Kawase ray pass.
- `effects/chain.rs`: the bind group layout gains 8/9; `Compiled` keeps an optional bright pipeline; `run` runs the
  pre-pass before a lens call and binds its pictures; helper to read a param by key.
- `effects/builtin/lens_flare.wgsl`, `lens_streaks.wgsl`, `fringes.wgsl` (new).
- `stage_camera.rs` (new): `StageCamera { orbit, tilt, height, lens, pivot, fill }`, `StageView` (`solve`, `ray`,
  `plane_at`, `screen_of`, `uv_rest`, `motion`), the GPU uniform `StageCameraGpu`, `Frame::stage_view()`.
- `stage_camera_pass.rs` (new, `StageCameraPass`): the background picture, seed, pull, push and camera passes, its
  textures (made on first need), run after `lighting.run` in `lib.rs::render`.
- `shaders/stage_camera.wgsl` (new): appended to `lighting.wgsl` (so the structs, `world_to_uv`, `field_at` and the
  sphere grid stay one source), entries `fs_cam_seed`, `fs_cam_pull`, `fs_cam_push`, `fs_cam_view` (three outputs:
  picture, unlit, lit colour); bind group 3 its own.
- `frame.rs`: `LightScene.camera: Option<StageCamera>`.
- `lighting.rs`: keeps the last run's group 0 / 1 / 2 bind groups and exposes the three layouts and the surface
  picture (`pub(crate)`), nothing else.
- `lib.rs`: run the camera pass, swap the light pictures the chain reads, `pub use stage_camera::*`.

### bpe-kit
- `things/camera/mod.rs`: the new settings and `FillEdges` choice; `things/camera/draw.rs` (new): `CameraDraw`
  with `light()`; registered in `lib.rs::draws()`.
- `bpe-core/src/settings.rs`: `SECTION_ORDER` gains "Stage camera" after "World box" if the groups rank there.

### bpe-studio
- `preview.rs`: `View.stage: Option<StageView>`; `to_screen`/`to_world` go through it; `Studio.stage_view` set from
  the frame drawn in the Camera view; the World view clears `frame.light.camera`.

## Tests (what passing looks like)

- `effects` unit tests: the lens header parses (`rays soft`), a bad word is an error, every builtin assembles and
  validates (also its bright entry), counts 29/26; `ray_passes` gives N = 1 for L ≤ 1 and the energy factor.
- `bpe-gfx` picture tests (`lens_tests.rs`, headless): **streaks**: a small bright dot, streaks on → the row through
  it brightens left and right far more than the column above and below; turned 90° the other way round.
  **flare**: a bright dot left of the middle → the ghost side (right of the middle) brightens, an unlit frame stays
  black. **fringes**: a white dot near the right edge splits: red outward, blue inward. A dim picture under the
  threshold is left as it is (flare/streaks add nothing).
- `stage_camera.rs` unit tests: at rest every screen point's ray meets the picture plane at the front view's world
  point and `uv_rest` of any point on that ray is the pixel; `plane_at`/`screen_of` round-trip under a move;
  positive orbit moves the origin right; fill zoom is 1 at rest and > 1 when orbiting with a back wall; lens 1e9
  stays finite.
- `bpe-kit/tests/stage_camera_pictures.rs`: the stage demo with the switch on and no move is byte-identical to the
  switch off (the rest pose is exact); orbiting 25° changes it; a ball stays round (its painted width and height
  within 15% of each other) and the wall slides more than the balls (parallax: a wall feature moves farther than a
  ball's middle between Orbit −15° and +15°); Flat with the switch on is byte-identical to off.
- `unlit_pictures`, `lit_pictures`, `lit_flat_pinned`, `stage_pictures` unchanged; physics and sound goldens
  unchanged (draw-only).
- Editor: `View` round trip with a stage view (unit test); the inspector shows the Stage camera group (kittest
  search for its label).

## Things not to do

- Do not edit `lighting.wgsl` (2b and 4 are in it); append to it at build time instead.
- No hard ranges for taste; only the ones in decision 12, each with its reason.
- No state kept between frames (the lens pre-pass and the camera pass are worked out each frame).
- No built-in or example scenes in the program; the demo scenes stay test-only.
- No colours made up in the editor.
