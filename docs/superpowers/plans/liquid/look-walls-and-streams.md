# Liquid look: walls, thin streams, pinholes and the pour

Part 3 ("the look") of `same-water-every-detail.md`, from the owner's 2026-11-21 review request ("visuals, with
and without lighting"). Part 1 (physics) is PR #204; part 2 (speed) is another thread, which owns
`crates/bpe-gfx/src/liquid_sim/mod.rs` and `shaders/liquid_sim.wgsl`: **this part does not touch the solver.**
It works in the liquid's drawing (`liquid.rs`, `shaders/liquid.wgsl`, `liquid_drops.wgsl`), the wet grid
(`liquid_sim/wet.rs`, `shaders/liquid_wet.wgsl`, which only the draw reads) and the Pour's births
(`bpe-kit/src/things/liquid/births.rs`). Built without approval stops; one Opus review before the pull request.

## What is wrong (full size, main 50daa70, the owner's "Water Weird" scene and the look scenes)

Seen at 1:1 in `LOOK_FILE=<Water Weird> LOOK_ONLY=file` at Normal and Fine, and in `liquid-water`,
`liquid-lit`, `liquid-deep-lit`:

1. **A pale band and a scalloped edge where liquid meets a wall** (the "bright dashed line" on a white floor):
   each drop and its mirror are cut at the wall's plane in the splat, so the surface pass takes the cut for a
   free edge. It rounds the edge over (Roundness), puts the highlight and the sky's reflection on it, and bends
   the picture behind it (Bend 14 px), which reads the white wall into the liquid's last pixels. The cut itself
   sits wherever the field crosses the Edge level, which is a pixel or two inside the wall in places.
2. **Notches along a curved wall, and bumps over it.** A drop only gets its wall when the wet grid knows "the
   liquid's side" of all the nodes round it. `cs_wet` decides the side by comparing the liquid's mass two cells
   either way from the node; for a node two or three cells inside the liquid both are liquid, so the side is a
   coin toss, the face distance comes out negative, and drops near it are drawn unmirrored and uncut: a notch.
   Thin films on a wall have no mass two cells off, so their side is never known: drawn over the wall.
3. **Thin streams and spray are strings of beads.** A stream that falls stretches: its drops move apart along
   their way. Each is drawn as a round blob (boosted when alone), so an arc off the plate, the lower part of a
   pour and the runs down the side walls are chains of beads with a highlight on each. The boost also has a
   blind band: it starts at 0.45 of full while the surface is cut at 0.5, so liquid between the two is neither
   boosted nor over the level and drops out (holes in a thin pour).
4. **Pinholes in the pool under a pour** at Normal and Fine: voids smaller than a cell, which the physics
   cannot see, drawn as dark dots with a bright rim.
5. **A Pour with Rate under 1 is a curtain of spray**: `births::pour` scatters its drops over the whole Width,
   so at Rate 0.32 the liquid is born at a third of its resting density, 60 px wide, and is only drawn at all
   because of the lone-drop boost (as a pitted column three times too wide for the water in it).
6. With a Lighting card the same wall edge becomes a lit bevel along every wall (the height written for the
   lights has the same rounded rim there).

## Design decisions

### 1. The wet grid keeps each node's place against its wall, with no "side"

`liquid_wet.wgsl::cs_wet` writes two more numbers a node, so the buffer is `6 × cells` floats
(`wet.rs::WetGrid::new`; it was 4): slot 4 `line`, the node's signed distance from its wall's line in cells
exactly as the walls band has it (`solid[2n].z`: below 0 inside the outline), and slot 5 `band`: 0 off the
band (and in a new, zeroed buffer), 1 for a filled wall, `2 + half` for a hollow one (`half` its half
thickness in cells). Slots 0 to 3 (the two halves of wetness, `face`, `side`) stay as they are: the wet film
(`fs_wet`) still uses them.

Why: a signed distance from the line is smooth straight through a thin wall, so it interpolates exactly beside
a flat wall whichever nodes the drop's cell has, and whoever reads it knows its own side from the sign at its
own place. Nothing has to guess where the liquid is.

`wall_at(q)` (the same function in `liquid_drops.wgsl` and `liquid.wgsl`): bilinear `line` and `half` at grid
point `q`, valid only when all four nodes are in the band and are all filled or all hollow. Gives the distance
from the face on `q`'s own side in cells (`line` for a filled wall, `|line| − half` for a hollow one) and
which side (`sign(line)`).

### 2. Drops and their mirrors spill a little past the face; the surface pass makes the cut

- `liquid_drops.wgsl::wall_face` uses `wall_at`: the normal is the slope of the bilinear `line` (central
  differences half a cell either way, accepted at length 0.5 to 1.5) times the drop's side. A drop from a
  quarter cell past the face to its reach from it gets the plane, as before.
- `fs_drop` keeps the drop and its mirror up to `SPILL` 3 px past the plane (soft over the last pixel), not
  cut at it. On the liquid's side the field is then the same as if liquid carried on behind the wall, and the
  surface pass's slope (±1 px) and Mixing shows (±3 px) never see a step.
- `liquid.wgsl::fs_surface`, for drops on the card (`st.v.x ≥ 1`): `wall_px`, the pixel's distance from the
  wall's face (from `wall_at` at the pixel's grid point, in picture pixels), cuts the cover:
  `cover *= clamp(wall_px + 1, 0, 1)`: whole half a pixel outside the face, gone a pixel inside it, so the
  liquid runs half a pixel under the wall's own soft edge and no hairline of background shows between them.
  Everything inside a wall's material is cut, mirrored or not (a spilled drop, a film whose mirror failed).
- Because the field is even across the face, its slope along the wall's normal is nothing there, `inside`
  (height over the level by the slope) is large, and the surface is flat at the wall: no rim, no highlight,
  no bent picture, no outline band, and for the real lights no bevel in the height. A free surface that
  meets a wall keeps its rim up to the contact point. Nothing else is needed for the lit passes: they share
  the field and `inside`.
- The surface pass's group gets the wet grid (binding 5, as `fs_wet` has it) and the coarse field (binding 7,
  for decision 4). Each Liquid thing's surface group is built with its body's wet grid in
  `LiquidPass::prepare`; the Particles Liquid layer's shared group binds a small empty buffer (its
  `st.v.x` is 0, so it is never read).

Not done: a corner still mirrors one wall only (the nearer); the cut near a corner follows the bilinear
distance, which rounds the corner's inside by under a pixel.

### 3. Thin liquid is drawn as streaks, and the boost has no blind band

In `liquid_drops.wgsl::drop_out`, from the drop's `j` (`vd = 1 / j`, the liquid round it: about 1 inside,
0.5 at a flat surface, 0.1 alone) and its speed:

- **Boost**: `boost = clamp(SPARSE / vd, 1, most)`, `SPARSE` 0.62, `most = max(1.6 × level, 1)` (a lone drop's
  peak, as before). A liquid's field is about `vd ×` the full field, and the surface is cut at Edge × full
  (Edge 0.5 by default), so with this the field of any liquid thinner than `SPARSE` of full is lifted to
  about `SPARSE ×` full: over the level at every density, with no gap between "alone" and "enough". At Edge
  above about 0.6 sparse liquid can still drop out; that is what a lean Edge asks for.
- **Streaks**: `thin = 1 − smoothstep(0.45, 0.8, vd)`; the drop's splat is stretched along its velocity by
  `s = 1 + thin × min(speed × shutter / across, MOST_STRETCH − 1)`, where `shutter` is Streaks / 120 s (half a
  frame at 60 a second, the usual film shutter) and `across` is the round droplet's width, 0.76 × reach. It is
  narrowed across by `s^−0.4`, so a streak is longer and a little thinner than its droplet, never fatter.
  `MOST_STRETCH` 6. The mirror image's velocity is mirrored too. Liquid at rest and the inside of a body
  (`thin` 0) are drawn exactly as before.
- New Look setting **Streaks** (`LookSettings::streaks`, key `streaks`, group "Shape of the surface", after
  Smoothing; range 0 to 1e9, span 0 to 4, default 1, keyframe): "How far fast drops and thin streams smear
  along their way: 0 draws round drops." It rides in the style's spare `c.w`. It only draws, so
  `LookSettings::stepped` leaves it out and it restarts nothing. The stains' flat style sets it to 0.

Why streaks and not a physical thin film: a stream's drawn width cannot follow its volume once its drops are
further apart than a splat (any kernel, however shaped, sums to the same thing), so the honest choices are
beads or a smear along the motion, and the smear is what a camera's shutter does to falling water.

### 4. Pinholes close in the picture

`full_surface`'s caller (`fs_surface`, full looks only) reads the coarse field `c` at the pixel (the 13 × 13
px mean `fs_coarse` already makes for the depth; bilinear). Where the liquid round the pixel is nearly whole,
the pixel counts as liquid: `fill = smoothstep(lo, hi, c / full)` with `full = π reach²` (cells; the field of
resting liquid), `hi = clamp(1 − 0.003 × cell_px², 0.8, 0.97)` (a hole 0.8 of a cell across takes that share
out of the window) and `lo = hi − 0.12`. `cover = max(cover, fill)` and `inside = max(inside, fill ×
roundness)`, so a closed pinhole is flat, with no rim or glint. A straight free edge reads 0.5, a hole wider
than about a cell under `lo`: neither is touched. Concave creases narrower than the window round over a
little, as surface tension would.

The coarse pass runs before the surface pass already; `fs_coarse` reads the field after the splat, so the
mirrors count and a wall does not make the liquid beside it look thin (the 3 px spill is all the window sees
past a face, so within about 5 px of a wall pinholes stay: accepted).

### 5. A Pour under Rate 1 is a narrower, solid stream

`births::pour`: drops are spread over `width × min(rate, 1)` (centred on the spout), not the whole Width. The
number born (`width × speed × rate`) is unchanged, so the stream is born at its resting density: a third of
the Rate is a third of the width, as a tap half open. Rate 1 and above are unchanged (above 1 it still comes
out packed across the whole Width). The Rate's help text says so.

This moves where a Rate < 1 pour's drops are born, so `golden_liquid` and the pour test are run (the golden
pins counts and is expected not to move; re-bless in debug and release only if it does, and say why).

## Files

| File | Change |
| --- | --- |
| `crates/bpe-gfx/src/shaders/liquid_wet.wgsl` | slots 4 `line`, 5 `band` |
| `crates/bpe-gfx/src/liquid_sim/wet.rs` | `WetGrid::new`: 6 × cells; doc comments |
| `crates/bpe-gfx/src/shaders/liquid_drops.wgsl` | `wall_at`, `wall_face`, boost, streaks, spill |
| `crates/bpe-gfx/src/shaders/liquid.wgsl` | `wall_at`, `wall_px` cut, pinhole fill, binding 7 in the surface pass |
| `crates/bpe-gfx/src/liquid.rs` | surface layout bindings 5 and 7, per-liquid surface group, the empty buffer, `c.w` doc |
| `crates/bpe-kit/src/things/liquid/cards.rs` | `streaks` setting; Rate's help |
| `crates/bpe-kit/src/things/liquid/draw.rs` | `c[3] = streaks`; stains 0 |
| `crates/bpe-kit/src/things/liquid/births.rs` | pour width × min(rate, 1) |
| `crates/bpe-kit/tests/full_size_look.rs` | `LOOK_FILE` / `LOOK_TIMES` (draw a saved scene file) |
| `crates/bpe-kit/tests/liquid_look_pictures.rs`, `things/liquid/tests*.rs` | tests below |
| `docs/notes/liquid.md` | a new "as built" section (not edits to others') |

## Edge cases

- A new body's wet grid is zeros: `band` 0 everywhere, so no wall is known until the first liquid step (drops
  at 0 s draw unmirrored and uncut, as today while the side was unknown).
- Openings (Gaps) have no band nodes: no cut, no mirror there, liquid pours through as drawn before.
- Liquid on both sides of a thin wall: each side is mirrored and cut for itself (before, only "the liquid's
  side"). The spill is 3 px, so across a wall 3 px thick or more nothing reaches the other side's field.
- A drop inside a wall's material (leaked): hidden by the cut while it is in there.
- A moving wall: the wet grid is rewritten every liquid step from the walls band, as now.
- NaN or zero velocity: no stretch (`s` 1). `j` 0 (not measured yet, a newborn): `vd` 1, as before.
- Zoomed or turned camera: the cut is in picture pixels from grid units (`cell / unit`), the streak in world
  units, both through `to_clip`.
- Particles Liquid layer (blobs): untouched. Its picture tests must not move.

## What not to do

- Do not touch `liquid_sim.wgsl` or `liquid_sim/mod.rs` (the speed part is changing them), nor the `Drop`.
- Do not use the wet grid's `side` or `face` for the drops' walls again (decision 1).
- Do not cut the splat at the plane again: the surface pass needs the field past it.
- Do not stretch or boost by anything but `j` and the velocity: both are in every drop already.
- No new scene, no built-in scene, no exe.

## Tests (only what this needs)

1. `bpe-gfx liquid_sim::tests`: the wet grid test also reads `line` and `band` (a node beside the test wall
   holds its distance and band; a node far off holds band 0). Existing wet tests keep passing.
2. `liquid_look_pictures.rs` (270 × 480, numbers):
   - `liquid_meets_a_wall_flat`: still water in the ring; walking along the wall just inside its face under
     the waterline, every pixel is the body's colour within a small tolerance (no pale rim, no notch); it
     fails on main.
   - `a_fast_lone_drop_is_a_streak`: `bpe-gfx` level if simpler: a drop with speed drawn with Streaks 1 covers
     a longer, not wider, run of pixels along its way than with Streaks 0.
   - `pinholes_close` is covered by `deep_water_is_even_across_its_body_and_meets_the_wall` staying green
     (it already asks the body to be even); no new test unless the look check shows one is needed.
3. `things/liquid/tests.rs`: `a_thin_pour_is_born_narrow_and_whole`: a Rate 0.3 pour's newborn drops lie
   within Width × 0.3 of the spout's line, as many as before.
4. Existing: `cargo test -p bpe-kit liquid -- --test-threads=2`, `liquid_pictures`, `liquid_look_pictures`,
   `lit_liquid_pictures`, `restarts` (the new setting), `golden_liquid` debug and release, clippy, fmt.
5. The full-size look check (`LOOK_ONLY=liquid`, and `LOOK_FILE` with the owner's scene at Normal and Fine),
   pictures and 1:1 crops opened: walls, streams, the pour, pinholes, lit scenes (seams at walls, flicker
   between the 1.0 and 1.05 s frames, banding).

## Not in this part

- Balls are not mirrored in the physics (liquid packs a little round a sunk ball) and liquid climbing ring
  walls in hard pours: both are the solver's, which the speed part is changing. Left for after it.
- A Fill's first-second jolt (its lattice does not fit the walls' margins): physics at birth; left.

## As built (2026-11-22)

What changed while building, found by looking at the owner's scene at 1:1 after each step:

- **Decision 2, the mirror plane.** With the cut moved to the surface pass a pale scalloped band was still
  there. The cause was older than the cut: the picture mirrored drops in the wall's face, but drops stop half
  a cell off it, so a row was missing and the field at the wall was only about 1.2 times the Edge level. Drops
  are now mirrored a quarter cell off the face (`MIRROR_OFF`, the physics' own `WALL_GAP` plane), and a drop
  up to `reach + 0.5` cells off the face is mirrored.
- **Decision 2, the normal** is the slope of `line` between the drop's own four nodes (bilinear, analytic),
  not central differences half a cell either way: those needed neighbours outside the three-cell band for
  drops two cells off the face.
- **New: gaps under a body at a wall close** (`liquid.wgsl::wall_lift`, `whole_near`; `LIFT` 1.5 cells,
  `LIFT_FAR` 5 px, `LIFT_DEEP` 3 cells). Sloshing liquid lifts off the floor in pockets a cell or so high that
  no mirror fills. Four versions were tried: reading the field `LIFT` cells off the face and using it where
  the pixel's own cover was under 1 (left every pocket's rim as a ghost outline); always using it (extruded
  holes and edges beside a wall into hard-edged blocks); going by the smooth coarse field a little further
  out, which only flattens and covers the pixel (clean on the floor, but it filled between the side walls and
  the streams running down them, in frayed blocks); and the one kept, which also asks for liquid 1, 2 and 3
  cells further out, so a stream is not taken for a body.
- **New: the bent picture stops short of a wall** (`wall_bend`, `BEND_OFF` 0.4 cells): the rims that are left
  near a wall no longer read the white wall into the liquid.
- **Decision 3, the numbers.** Boost: `SPARSE` 0.38, `SPARSE_END` 0.54, `SPARSE_TO` 0.9
  (`mix(1, clamp(SPARSE_TO / vd, 1, most), 1 − smoothstep(SPARSE, SPARSE_END, vd))`): a flat surface's own
  drops read about 0.5, so the plan's `SPARSE / vd` with 0.62 would have lifted every surface; the lift is
  over by 0.54 instead. Streaks: `THIN` 0.35, `THIN_END` 0.65 (the plan's 0.45 and 0.8), for the same reason:
  a body's own surface drops are only partly thin.
- **Tests** as listed in `docs/notes/liquid.md`; the streak test compares Streaks 0 and 4 on the falling
  stream and on still water instead of measuring one drop.
- **Not done**: the lit scenes' own dullness (the scene's lamp and the lighting mix, not the liquid), the
  paler column under a hole in the body (the depth starts again under it), and the solver's items above.
