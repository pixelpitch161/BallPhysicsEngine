# Liquid fixes, part B: lighting and look without artifacts

Design: the liquid review (2026-11-17, report https://claude.ai/artifact/HpzbSCF5chBk44hy2rVxrs, handover
`/mnt/project-files/ballphysics/liquid-thing/review/handover.md`). Aedan approved "Both, feel first"; part A (water
feel) merged as PR #198. This plan is built without approval stops.

## What is wrong (review renders, main b850811; still true on de95642)

- **L1 the liquid shadows itself.** With Blocks light (default on), `shadows.wgsl::march` leaves the pixel's own
  blocker and then meets the same liquid again at a crest, a spike or across a hole: dark shafts through the body.
- **L2 depth by a 24-step march straight up** (`liquid.wgsl::depth_below`): a thin gap in a splash spike reads as
  the top (bright columns under crests), a hole is met at a step that repeats with depth (a "ladder" of ghost rings
  under holes), faint bands in still water.
- **L3 Ripples** (default 0.3) bump the normal over the whole body ("orange peel"), and their phase runs with the
  flow speed, so a stream shows as a band sliding at another speed.
- **L4 walls**: no drops beyond a wall, so the field falls under the Edge level a little short of the wall's face:
  a gap and a convex "mercury" corner, a dashed highlight along the top near the wall, stray holes by it.
- **L5 clear water gets no glints under real lights** (the height written for the lights has nothing on the top).

## Design decisions

1. **L1, the liquid never shadows its own body.** `LightScene` gets `liquid_surfaces: Vec<u32>` (each Liquid
   thing's surface index, pushed by `LiquidDraw::light` when it blocks light). The lighting uniform's spare
   `bent.z`/`bent.w` hold the lowest index and one past the highest (0, 0 when none; a Liquid thing's indices are
   consecutive, `things::liquid::draw::surface_index`). In `march`:
   - `own` = the start pixel's `surface_code` when it is in that range (else −1). A pixel on its own liquid leaves
     it as it leaves a solid blocker (`leaves`), also when the liquid is see-through (Glass), so clear water does
     not tint its own light either.
   - When a ray meets a blocker (after `confirm`, and in the clear-tint branch) whose picture pixel's surface code
     is `own`, it walks out of it by the field (as the leave loop does), steps 1.5 texels on, and carries on as
     after a stretch only the field's coarseness blocked (`open`, `beside`). Other things (balls, shapes, another
     liquid) still shadow it, and it still shadows everything else.
   - Known gap (as today's leave loop): a ball fully inside the liquid, between a pixel and where the ray leaves
     the liquid, doesn't shadow that pixel.
2. **L2, depth from a smooth coarse field.** Two new small passes, run for a Liquid thing's full look (`st.v.w`)
   right after its splat, at a quarter of the field's size each way (`DEPTH_DIV` = 4, R16Float textures):
   - `fs_coarse`: each coarse texel is the mean of the field's alpha over a 12 × 12 pixel window round it (thin
     gaps, thin spikes and holes under ~8 px vanish; the result is smooth).
   - `fs_depth`: marches the coarse field (bilinear) up the screen's world-up one coarse texel a step, up to
     the style's range (as `depth_below`'s, at most 256 steps), and writes how far up the level is crossed
     (linear between the last two samples) in world units; the range when it never is.
   - `full_surface` reads the coarse depth bilinearly. Near the top (coarse depth under 24 px) it blends to the
     existing fine march limited to 24 px (exact at the top, for foam's On top, film rings, bubbles popping,
     caustics' fade-in); the blend is `smoothstep(12 px, 24 px, coarse)`. Deep down only the coarse depth counts,
     so holes, gaps and rows of drops make no rings, columns or bands.
   - The Particles Liquid layer (no full look) is untouched; its group binds the same textures, unused.
3. **L3, ripples on the top only, not sliding with the flow.** In `full_surface` the ripples' normal tilt and
   height fade out with depth: `1 − smoothstep(0.5 s, 2 s, depth)`, s = ripple size. The flow no longer changes
   the noise's phase (it made the bands); it adds up to 50% to the ripples' strength near the top instead. The
   waterline itself is ruffled: in `fs_surface`, for a full look with ripples, the edge's `inside` (px) moves by
   the ripple height where the edge faces up (`dot(outward, up)`), at most a pixel or two at the defaults, so the
   highlight along the top breaks up like small waves.
4. **L4, the liquid meets the walls.** The splat reads each body's wet grid (always kept: face distance per grid
   node, from the wall's face on the liquid's side, `liquid_wet.wgsl`):
   - In `vs_drop`, a drop within its reach of a wall's face (bilinear face distance `f` in [−0.25, reach], the
     four nodes all in the band) gets the face's plane: normal from the face distance's slope (central
     differences, accepted when its length is 0.5 to 1.5), point `x − f n`.
   - A second draw of the same drops (`vs_mirror`, same fragment) puts each such drop's mirror image across that
     plane; a drop not near a wall is put off screen.
   - Both the drop and its mirror are cut at the plane in `fs_drop` (world position passed through; `discard`
     past it), so the field is whole up to the wall's face and nothing shows past it.
   - Drops past a face (`f` < −0.25, spilled outside a ring) and nodes whose side isn't known yet draw as today.
   - The "optional slight climb" (meniscus) is left out.
5. **L5, glints on the top.** With ripples now only near the top and a little stronger there, the height written
   for the real lights (`bump_h`) gives the top small slopes the lighting's speculars catch. Check by render; if
   clear water still shows none, raise `bump_h` near the waterline (within one ripple size of it), not elsewhere.
6. **A look scene** in `full_size_look.rs` ("liquid-deep-lit"): a ring at Level 0.7 of clear water (Glass), a
   Point light inside the ring at (−150, −400), five balls dropped in. Not a built-in scene (none are added).

## Files

- `crates/bpe-gfx/src/frame/light.rs`: `LightScene::liquid_surfaces`.
- `crates/bpe-gfx/src/lighting/run.rs`: `bent.z/w` from it; `lighting/mod.rs` uniform comment.
- `crates/bpe-gfx/src/shaders/lighting/shadows.wgsl` (`march`, a helper `own_liquid_at`), `bindings.wgsl` comment.
- `crates/bpe-kit/src/things/liquid/draw.rs` (`light` pushes its index).
- `crates/bpe-gfx/src/liquid.rs`: coarse/depth textures and pipelines, the wet grid in the drops group, the
  mirror pipeline, the depth texture in the surface group.
- `crates/bpe-gfx/src/shaders/liquid.wgsl` (`fs_coarse`, `fs_depth`, `full_surface`, `fs_surface`),
  `liquid_drops.wgsl` (`vs_drop` plane, `vs_mirror`, `fs_drop` cut).
- `crates/bpe-kit/tests/full_size_look.rs` (the look scene), tests below, `docs/notes/liquid.md` (as built).

## Not to do

- No new built-in scenes; don't build or install the exe.
- Don't change the sim or the goldens (this is draw-only; physics goldens must not move).
- Don't make the liquid stop shadowing other things, nor other things stop shadowing it.

## Tests and checks

- `bpe-gfx` liquid tests stay green (`--test-threads=2`).
- New picture tests (`bpe-kit/tests/liquid_look_pictures.rs`, `test_device()`):
  - **no ladder**: still water with a hole punched in it (drops removed in a small disc, or a ball sitting inside):
    the column under the hole is within a small tolerance of the columns beside it, 3 to 10 hole sizes down.
  - **meets the wall**: still water in a ring: along the waterline's height, the liquid covers the pixels from
    the ring's inner face inwards (no gap wider than a pixel at 270 × 480).
  - **no self shadow**: lit scene, light inside the ring above the water with a ball pushing a crest: the light on
    the liquid body under the crest isn't darker than beside it by more than a small tolerance.
- `bpe-kit/tests/restarts.rs` unaffected (draw only).
- Full-size look check: `LOOK_ONLY=liquid` before and after; open the crops; look for shafts, columns, rings,
  speckle, gaps at walls, seams where the mirror cut is, flicker between the two close frames.
