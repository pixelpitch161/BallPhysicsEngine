# Picture fixes (workstream W3: review findings V6 to V15)

Source: the 2026-11-08 improvement review ("How the pictures look", Everything else table), approved by Aedan
2026-11-08 15:18Z with everything but the Size section. Trial patches for V6 to V10 (against bb129c0) are in the
project files `ballphysics/visual-review/visuals/*.patch`; they apply cleanly to main b8430c3.

No design brainstorm: each item is a fix to something that looks wrong, with the fix named in the review. The one
taste call (V14) is stated below.

## Items

### V6 Labels on balls (trial patch `look-presets-fixes.patch`)
`bpe-kit/src/things/text/looks.rs::add_text`: the Labels on balls look sets `t.z_order = 1`, so the labels draw over
their balls. Test: a new picture-free check that the look's Text has a z order above the balls' (0), plus the existing
look test. Verify by eye in the full-size look check (`text-labels-on-balls`).

### V7 Trail corners (trial patch `trail-round-corners.patch`)
`bpe-gfx/src/frame.rs::strip`: corners sharper than 60° (`d1·d2 <= 0.5`) get a join: both segments meet at the inner
mitre point when it lies within both segments, and a fan (≤ 15° a step, at most 12) fills the outside round. Edge
values are true distances so the shader's antialiasing carries on. Affects Streak, Line, Glow/Outline/Shadow strips.
Test: a unit test in `frame.rs` that a strip with a 170° bounce covers the outer corner (the point at
`corner + outward * width * 0.9` lies inside some triangle) and that a straight strip gets no extra triangles.

### V8 Smoke trail beads (trial patch `smoke-trail-fill.patch`)
`bpe-kit/src/modules/look/layers/trail/draw.rs`: Smoke fills each gap between two puffs with up to `fill_most`
(1 to 8, keeping every ball under `MAX_PARTICLES`) evenly spaced puffs, as many as the path length over 0.6 puff widths
needs; the filling puffs share one puff's cover (Add: colour / fill; otherwise alpha `1 - (1-a)^(1/fill)`). The places
are fixed shares of fixed history, so they don't shimmer. Test: a unit test that a fast ball's Smoke trail makes more
puffs than a slow one's at the same Puffs rate, and none overlap less than they should (max gap ≤ 0.6 puff width + ε).

### V9 Ready-made looks (trial patch `look-presets-fixes.patch`, plus the glass lamp)
- Race leaderboard: position `(-200, -760)` so the block sits on screen.
- Glass letters: `fitted()` so GLASS fits the screen width, and **a lamp** (new): a Point light added beside the Text
  like Shadow lettering's, above-left, so the glass catches highlights and throws a coloured shadow. Update
  `the_lit_looks_light_the_scene` (Glass letters: 1 light).
- Rainbow smoke trail: colour follows each puff's age (`Follow::Life`), not the clock.
Test: a check that every text look's laid-out words fit in the 1080-wide screen at its place (all looks, so a future
look can't run off either); the light count test.

### V10 Output dither (trial patch `output-dither.patch`)
`bpe-gfx/src/shaders/post.wgsl::fs_composite`: triangular ±1 step dither from a PCG hash (no `sin`), same on every
channel, faded out over the first and last step so pure black and pure white stay exact. Every picture moves by up to
one step: picture tests that pin exact values are re-pinned only where the difference is the dither. Test: a picture
test that a flat dark grey fill has a mean within 0.25 step of the undithered value, and that pure black stays 0.

### V11 Spiral / Sweep seam (new)
`bpe-gfx/src/shaders/paint.wgsl`: `paint_seam(p, local)` for Sweep (3), Along the edge away from a shape (4) and
Spiral (15) returns how much of this pixel lies across the wrap (from the form's continuous turn value and its width a
pixel, worked out analytically like `paint_mask`'s `turn_width`, since `fwidth` of a wrapped value spikes) and the
place on the far side (the middle of the part across). `paint_colour` then mixes the two places' colours, through a
two-pass loop so the heavy paint maths keeps one call site. Loop gradients are unchanged (their ends match). Test: a
picture test drawing a Sweep ball at full size: the pixels along the seam take in-between colours (no pixel jumps the
whole way from the end colour to the start colour between neighbours across the seam).

### V12 Depth of field speckle (new)
`bpe-gfx/src/lighting.rs`, `shaders/lighting.wgsl`: before the depth of field pass, a new pass `fs_dof_small` draws
a quarter-size box-filtered copy of the picture it blurs (each texel the mean of 4×4 pixels, four bilinear taps). The
copy is bound at group 0 binding 2 in place of `lit_picture`, which the pass never reads. `fs_dof` reads each tap from the copy
(bilinear, at the tap's exact place) instead of one rounded full-size pixel, blending from the full-size pixel to the
copy as the pixel's blur grows from 6 to 14 px, so wide blurs average 16 pixels a tap and the speckle goes; narrow
blurs stay as before. The reach test (depth) is unchanged. Test: the existing DOF picture tests; a new one that a
blurred, finely striped wall's pixel-to-pixel noise (mean |p - right neighbour|) is lower than before (stated
threshold measured once). Look by eye in the full-size `stage-*` shots with depth of field.

### V13 Stage camera dark arc (new)
`shaders/stage_camera.wgsl::cam_solid`: when a ray hits ball A, the place it reads in the front picture is kept off
every other ball drawn over A there, and off their antialiased rims: if the read point lies within `r_B + 1.5 px` of
another sphere B listed after A (drawn later, so over it), it is pushed out radially from B to that distance, then
held inside A's own rim as now. Only once the camera moves (`moving`), as the existing rim pull. Test: a Stage camera
picture test with two touching balls of very different colours under an orbit: no pixel inside the moved view of the
far ball is close to the near ball's colour. Look by eye in a new `stage-camera-orbit` full-size shot.

### V14 Highlights (new; the taste call)
`post.wgsl::fs_composite`: below 1.0 the picture is untouched (white is 255, not 247); above, the colour is scaled
by its brightest channel (hue kept, so glowing crowds don't drift toward lavender) and turned toward white by how far
over it is (the formula is under As built), so hot highlights still burn to white smoothly. Replaces the
per-channel shoulder from 0.8. Test: unit-ish picture test: a pure white fill reads 255; a saturated blue at 3× reads
with blue as the largest channel and red/green well below it.

### V15 Thin lines (new)
Defaults that read about 1 px at 1080 wide go to about 2 px: Rain streak width 0.7 → 1.1 (`particles/looks.rs`), Aura
line width 1.5 → 2.0 (`aura.rs` default). Arcs keep their 2.5 core (checked by eye; changed only if they still fade).

## Not doing
- Glass prism / shadow fixes (W2), anything in the Size section.
- Changing the CPU twin `gradient_form::along` (editor previews are small; the seam is a GPU antialiasing fix).

## Verify
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace -- --test-threads=2` (background). Goldens: physics is untouched, so no golden line may move;
  sound goldens likewise.
- Full-size look check, all scenes, plus the new `stage-camera-orbit`; open the changed scenes' PNGs and 1:1 crops.
- One Opus review of the whole diff, then the PR; merge when CI is green.

## As built
- V6 to V10 as the trial patches, plus the Glass letters lamp (white Point at (-220, -360) from the Text: at
  (-300, -700) it sat outside the test room's ring and lit nothing).
- V11 as planned. Measured on the software GPU round a 75 px ball: the biggest step between neighbouring pixels across
  the seam fell from 144 (Sweep) / 142 (Spiral) to 86 / 70 of a 180 jump; at full size the spiral's stair steps are
  gone.
- V12: the copy also keeps the least blur of its 4 × 4 pixels in alpha, and a tap uses the copy only where that blur
  reaches it (without it the copy dragged the in-focus ring's light into the blurred wall as speckle, worse than
  before). The copy is bound at binding 2 in place of `lit_picture` (a new binding 10 took the direct light pass over
  the 16 sampled textures limit). Mean pixel-to-pixel difference in the full-size `stage-depth-of-field` shot, round
  the ring / on the floor: 1.83 / 0.63 without the copy, 1.71 / 0.56 with it (both with the new dither).
- V13: pushing the read point onto the circle round the covering ball smeared it into radial streaks; it is mirrored
  across that circle instead. The `SphereGpu.c.w` z-order decides which ball is over which.
- V14: `whiter = max(white², over / (over + 0.5)) · min(4 (peak − 1), 1)`, with white = the least channel (to 1)
  and over = brightness (luma) − 1. Tried first, all by the peak: `over² / (over² + 4)` and `over² / (over² + 1.5 over
  + 1)` left lamp-lit white walls cream and a red ball's shine pink (`lit_pictures`' white-shine and light-only
  checks); `over / (over + 1.5)` whitened a blue ball's mild highlight too much. Brightness alone (luma) still left
  the red shine pink, so the least channel counts too. A pool of light past white is plain white now (no shoulder
  keeps detail in it): `mirror_light_pictures::a_mirror_ball_throws_spots_*` and
  `bent_light_pictures::a_convex_lens_focuses_*` dim their lamp to 0.4 so the spots and the focus don't land in white, and `paint_pictures::solid_paint_matches_plain_colour` leaves out the pixels the
  old shoulder squeezed (any channel past 0.8).
  `glass_pictures::pearl_and_flake_shine_their_own_ways`' flake margin is 4 jumps (48 against 42; was 6), and
  `paint_tests::linear` no longer undoes a shoulder; `pictures::TOLERANCE` is 3 (highlights past 0.8 aren't squeezed
  any more); `paths_tests::shapes_draw_as_before` re-pinned (white is 255); the fringes test's unmoved middle may
  differ by a dithered step.
- Review fixes: colours are clamped to 65504 before the roll-off (an infinite glow drew black); the corner fan turns
  the inside's way on an exact reversal and takes steps for a quarter-pixel error (thin trails need few); the depth of
  field gate takes the least of the four texels' blur (`textureGather`). Left as known: smoke fill puffs drop at the
  very end of a trail (mostly faded there); the Stage camera's rim clamp can pull a point back under a big covering
  ball.
- V15: Rain 0.7 → 1.1 (2.0 looked like thick dashes at full size), Aura 1.5 → 2.0. Arcs keep 2.5.
