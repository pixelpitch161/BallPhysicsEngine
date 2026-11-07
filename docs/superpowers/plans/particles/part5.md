# Particles part 5: looks 1

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 3.6, 2, 10, 11 part 5). Approved by the owner in
the brainstorm thread; built straight after this plan (no approval stops), one Opus review at the end, then the PR,
merged by this thread once checks and the review pass. The exe is not installed by this part (that waits for every
particles part).

Part 1 (the engine, `docs/superpowers/plans/particles/part1.md` and its "As built") is in. Parts 2, 3, 4, 6 and 7 build
at the same time; this part touches shared files only additively.

## What part 5 delivers

Five new Look layer kinds on the `look.particles` card and one setting group every layer has:

| Layer | Kind id | What |
| --- | --- | --- |
| Soft smoke | `particles.smoke` | particles splatted into a soft density field at half size, then shaded: thickness, noise detail drifting, light from one side, darker inside |
| Flame | `particles.flame` | heat splatted the same way, shaded through a Heat colours gradient with flicker, noise that rises and bends the flame |
| More shapes | `particles.shapes` | Leaves, Petals, Flakes, Needles, Birds, Fish, Pixels (snapped to a grid); point along their own turn or the way they go; birds flap and fish swim |
| Glitter | `particles.glitter` | diamond facets that flash with a star when turned to the light |
| Web lines | `particles.web` | lines between particles within Reach (and to balls), fading with distance; dots at the particles |
| Fine detail (every layer) | fold "Fine detail" | N extra small copies round each particle at hashed offsets (no sim cost), optionally shimmering |

Looks: Fire on a ball (Flame + embers), Rainbow smoke trail and Ink in water (Soft smoke), Blossom petals (Petals) are
upgraded; **Glitter** (30), **Web of light** (34) and **Pixel glitch** (39, pixels only: its glitch bands are part 6)
are added to the Add thing menu.

Not added here, because each also needs another part that is building in parallel: Dust clouds (5, part 2's air), Fire
and smoke (4, part 4's feeding), Falling leaves (17, part 4's settle), Flock (23, part 3's flocking). The part that
merges last of the two adds each of them with this part's layers (the coordinator is told when this part merges).

## Decisions made while planning

1. **Fields are a new frame list, drawn by the GPU in two steps.** `bpe_gfx::Field { at, splats, look }` in
   `Frame.fields` (behind the balls) and `Frame.fields_front` (in front). Each field is splatted into its own half-size
   `Rgba16Float` picture (additive: rgb = Σ colour × weight, a = Σ weight) right before the render pass that draws its
   frame's list, then composited by a full-screen triangle in that pass. `at` is how many sprites of the same list
   (`particles` / `particles_front`) are drawn before it, so a field sits exactly where its layer's sprites would
   (its sub-layers' shadows and glows under it, outlines over it). The renderer draws a list's sprites in runs split
   at the fields (`Sprites::draw_range`). This works in every frame the same way: the main frame, the overlay, every
   blend layer (with the `each` pipelines, as sprites have) and every light pass (the composite ends in
   `light_flat`, with the field's own `light`/`emit`/`extra`, all 0 until part 7). `Frame::cull` drops splats out of
   shot and moves `at` with the kept sprites; `fit_lists` clamps `at`.
   Part 6's Liquid can reuse the same field (a third mode).
2. **One splat format for both kinds.** Smoke weighs a splat by its alpha (density); Flame by its heat (alpha × the
   Cools curve over life). The colour carried is the layer's colour for that particle (Colours, Colour from, the
   colour sub-layers), so smoke takes its colours from the particles and a flame's Colours tint its Heat colours
   (white by default: no tint). A splat smaller than 1.5 field texels is widened to that and its weight cut by the
   area, so tiny particles don't flicker in and out.
3. **The composite** (`shaders/fields.wgsl`, pasted after sprites.wgsl and each.wgsl so it shares `Globals`,
   `light_flat` and `each_out`): reads the field at the pixel, works the pixel's world place out from the camera for
   the noise (value-noise fbm, 3 octaves, `g.view.w` time). Smoke: density = a × Thickness × (detail noise), cover =
   1 − e^(−density), colour = Σc/Σw shaded by the density's slope against the light's way (Shading) and darkened
   inside (Dark inside). Flame: the field is read at a place bent by rising noise (Bend), heat = a × Heat × noise ×
   flicker, x = 1 − e^(−heat), colour = Heat colours at x × tint × Brightness. Add (or a colour with no alpha) adds
   the light and covers nothing; otherwise it covers by `cover` (smoke) or smoothstep(0.02, 0.35, x) × the gradient's
   alpha (flame). Blend amount scales it for Normal and Add as `draw::shade` does for sprites. Heat colours are baked
   to 32 stops into the field's uniform (`FieldLook.heat`), so a field needs no gradient atlas row.
4. **Sub-layers on fields.** The layer's particles are first drawn as Smoke sprites through `particle_with_extras`, so
   Shadow, Glow and Outline add what they add per particle, then the sprites the layer itself would draw are replaced
   by the field. Recolour, Flash on hit and Colour on hit change the splats' colours as they change sprites'.
5. **New sprite shapes** in `sprites.wgsl` (codes go on from 9): 10 leaf (pointed both ends, a darker vein), 11 petal
   (narrow base, round front with a notch), 12 flake (six arms with side branches), 13 needle (tapered both ends), 14
   bird (an upright "m" of two bent wings; the pose flaps them), 15 fish (a body with a tail that swings with the pose),
   16 diamond (glitter's facet). `Particle.shape[1]` carries the pose (−1..1 as 0..65535). Their half length is
   `style[0]` as for the long shapes; a bird's quad is square. Pixels are crisp Confetti boxes (code 2) turned 0 and
   snapped on the CPU, so they need no shader code. The `layer.particles` sprite kinds and their menu don't change.
6. **More shapes settings**: Shape (Leaves / Petals / Flakes / Needles / Birds / Fish / Pixels), Point along (Own turn /
   Way it goes, then + Turn by), Length (× the shape's own length), Softness, Flap speed (birds, fish: per s, each
   particle starting at its own phase), Pixel grid (px; 0 doesn't snap). Flutter shows edge-on as for Sprites.
7. **Glitter**: each particle's facet faces `turn + its own random angle (+ Shimmer × time)`; flash =
   max(0, cos(facet − light))^Sharpness. The facet is drawn at Base brightness plus the flash, and when the flash is
   over 0.02 a Star (code 3, added light) of size × Star size × flash in Flash colour. Light from is an angle (the
   way the light comes from); following the scene's lights is part 7's.
8. **Web lines**: particles the layer draws go into a uniform grid (cell = Reach); each looks at the 3 × 3 cells round
   it in a fixed order, at most `MOST_LOOKED` (128) candidates, and keeps its `Links` nearest (at most 32; ties by
   index). A pair is drawn once (i < j, or j didn't keep i). A line is a Streaks sprite (code 9, a capsule) from one to
   the other, in the mean of their colours, its alpha × (1 − d / Reach)^Fade. To balls: lines from each particle to
   every live ball within Ball reach of its edge, in the particle's colour. Dots (× the particle's size, 0 none) are
   drawn after the lines as Dots sprites. At most `MOST_WEB_LINES` (200 000) lines a layer. All CPU, every frame, no
   state.
9. **Fine detail** is a common fold on every layer (Copies 0 off, span 0..8, hard 0..64; Spread × the particle's size;
   Copy size ×; Copy opacity ×; Shimmer turns per s). Copy k of a particle sits at `(signed(seed, 300+k),
   signed(seed, 400+k)) × Spread × size`, turned by `Shimmer × time × TAU × signed(seed, 500+k)`. On sprite layers each
   copy is a smaller sprite through `particle_with_extras`; on fields, a smaller splat; on Web lines, copies of the
   dots. Nothing in the sim changes.
10. **Common grows by one struct** (`FineDetail` in `Common.detail`), so `look_settings!` gets five more fields. Part 6
    adds Mirror, Colour split and Glitch next to them; whichever merges second rebases the macro.
11. **Nothing in the physics, state or goldens changes.** All of part 5 is drawing. The particle goldens and the ball
    goldens stay as they are.

## Files

### `bpe-gfx`
- `frame.rs`: `Splat` (Pod, 32 bytes: place xy, radius, weight; colour rgb, spare), `FieldLook` (Pod uniform: `kind`
  [mode, adds, amount, strength], `detail` [amount, size, drift x, drift y], `shade` [6 numbers by mode], `light`,
  `emit`, `extra`, `heat` [[f32; 4]; 32]), `Field { at, splats, look }`, `FIELD_SMOKE` 0 / `FIELD_FLAME` 1,
  `Frame.fields`, `Frame.fields_front` (default, `clear`, `has_overlay`, the emptiness check, `cull`, `fit_lists`).
- `fields.rs` (new): `Fields` (splat pipeline into Rgba16Float, composite pipeline and its `each` pipelines, the field
  bind group layout, a linear sampler), `FrameFields` (per frame: a `FieldGpu` per field of either list: splat buffer,
  uniform, texture sized half the targets, bind group), `upload`, `splat(encoder, globals, front)`, `composite`.
- `shaders/fields.wgsl` (new): `vs_splat`/`fs_splat`, `vs_field`/`fs_field`/`fs_field_each`, noise.
- `sprites.rs`: `draw_range`. `shaders/sprites.wgsl`: shapes 10..16 and the pose.
- `lib.rs`: `FrameBuffers.fields`, uploads in `render_scene`, splats before the passes that draw a list
  (`draw_scene`'s "scene" and "flashes", `draw_things_part`), the sprite runs split at fields in `draw_back` and
  `draw_front`; exports.

### `bpe-kit`
- `things/particles/look.rs`: the five kinds' settings (`SmokeLayer`, `FlameLayer`, `ShapesLayer`, `GlitterLayer`,
  `WebLayer`), their enums (`MoreShape`, `PointAlong`), `FineDetail` in the macro and `Common`, `Drawn` grows, `kinds()`.
- `things/particles/draw.rs`: shared per-particle colour/alpha (`Seen`), sprite kinds, copies, fields
  (`field_of`), Glitter's flash, `draw_web`.
- `things/particles/web.rs` (new): the neighbour grid and links (`links`), pure and unit tested.
- `things/particles/looks.rs`: the looks above.
- `around_subs.rs`: `Pieces::into_frame_with_field`.
- `things/particles/tests.rs`: unit tests; `tests/particle_pictures.rs` (new, GPU).

### `bpe-studio`
- Nothing new is needed: the layer menu, settings grids and the Add thing menu read the registry and `GROUPS`. The
  existing kittest that adds every look covers the new ones.

## Settings (labels as shown; spans unless the maths breaks)

Fine detail (every layer, fold "Fine detail"): Copies 0 (span 0..8, hard 0..64), Spread 3 × size (span 0..10), Copy
size 0.4 (span 0..1), Copy opacity 0.7 (0..1), Shimmer 0 turns per s (span 0..2).

Soft smoke ("Smoke"): Thickness 1.5 (span 0..6), Puff size 2 × (span 0..6), Detail 0.5 (0..1), Detail size 70 px (span
5..300), Drift 20 px/s (span 0..200), Drift way −90°, Shading 0.6 (0..1), Light from −135° (span −180..180), Dark
inside 0.4 (0..1). Defaults: Fade Out, Size over life Grow, Opacity 0.6.

Flame ("Flame"): Heat colours (black → deep red → orange → yellow → near white), Heat 1.4 (span 0..6), Puff size 1.8 ×,
Cools 0.7 (0..1), Detail 0.6, Detail size 45 px, Rise 140 px/s (span 0..500), Bend 10 px (span 0..60), Flicker 0.4
(0..1), Flicker speed 7 per s (span 0..30), Brightness 1.6 (span 0..6). Defaults: Blend Add, Fade Out, Size over life
Shrink.

More shapes ("Shapes"): Shape Leaves, Point along Own turn, Turn by 0°, Length 1 × (span 0..4), Softness 0.15 (0..1),
Flap speed 4 per s (span 0..20), Pixel grid 6 px (span 0..40).

Glitter ("Glitter"): Light from −60°, Sharpness 24 (span 1..128, hard ≥ 1), Base 0.35 (0..1), Star size 3 × (span
0..10), Flash colour white, Shimmer 0.5 turns per s (span 0..4). Defaults: Fade Both.

Web lines ("Web"): Reach 140 px (span 5..600, hard ≥ 1), Links 6 (span 1..32, hard 1..32), Line width 0.8 px (span
0..6), Fade 1.5 (span 0..5), Line opacity 0.8 (0..1), To balls (off), Ball reach 120 px (span 0..600), Dots 0.8 ×
(span 0..4). Defaults: Blend Add, Fade None.

## Steps and how each is verified

1. **Renderer fields.** `Field`/`Splat`/`FieldLook`, frame list plumbing, `fields.rs`, `fields.wgsl`, the draws.
   `bpe-gfx` unit tests: `cull` keeps a field's `at` pointing between the same sprites; `fit_lists` clamps it; a GPU
   test (skipped without an adapter) draws a smoke field and a flame field into a small target and checks the field's
   middle is coloured and its far corner untouched, Add brighter than nothing, a field between two sprites drawn
   between them (a red sprite under, the field, a blue sprite over: the middle pixel is blue).
2. **New sprite shapes.** A GPU test draws each new code: it covers its middle and leaves the corners; a bird's pose
   moves its wing tips.
3. **Kit layers and Fine detail.** Unit tests on frames: Soft smoke and Flame give one field with a splat per particle
   (copies add more) and no own sprites; a Glow sub-layer on smoke adds sprites before the field (`at` counts them);
   More shapes' codes, Pixels on the grid, Point along the way it goes; Glitter's star only when facing the light;
   Web links are each drawn once, never longer than Reach, at most Links a particle, and the same for any order of
   particles with the same places; Fine detail adds Copies sprites a particle. `cargo test -p bpe-kit particles`.
4. **Looks.** `every_look_adds_a_thing_that_resolves_and_makes_particles` covers the new and upgraded looks; a test
   that each look's layers resolve to the kinds it names (Fire on a ball has a Flame layer, etc.).
5. **Pictures** (`tests/particle_pictures.rs`, GPU): each new layer kind drawn from a Particles thing in a blank
   project shows something where the particles are and nothing far away; Flame with Add is brighter in the middle than
   at the edge; `BPE_SAVE_RENDERS=1` saves them.
6. **Nothing else moved**: `cargo test -p bpe-kit --test golden --test golden_particles` unchanged (no re-bless).
7. `cargo fmt`, `cargo clippy --workspace`, the full `cargo test --workspace` (debug), the editor's kittests, CLAUDE.md's
   Particles section, then the Windows exes build (not installed).

## Not doing (part 5)

- No Liquid, Trails, Marks that stay, Mirror, Colour split or Glitch (part 6); no lighting fold or Gives off light on
  particle layers, and glitter doesn't follow the scene's lights yet (part 7); no air (part 2), no flocking (part 3),
  no feeding or settling (part 4).
- No change to `layer.particles` / `shape.particles` or their sprite menu; no sim or state change; no built-in or
  example scenes; no demo videos.

## As built

- Fine detail's keys are `copies`, `copy_spread`, `copy_size`, `copy_opacity`, `copy_shimmer` (Smoke and Flame have their
  own `detail` and `detail_size`, so the fold's keys can't start with `detail_`).
- Each layer struct has a `starting` hook the macro's `Default` runs, so Soft smoke, Flame, Glitter and Web lines start
  with their own Opacity, Blend, Fade and Size over life.
- Ink in water keeps the Normal blend (Multiply would vanish on the default dark background); Soft smoke with Dark
  inside gives it the inky look.
- Web links are the same for any order of particles when no two distances tie; with ties the nearer index wins, so a
  fixed order (the particles are oldest first) always draws the same lines.
- A bird pointing the way it goes leans into it (up to about 30°) rather than turning on its side.
