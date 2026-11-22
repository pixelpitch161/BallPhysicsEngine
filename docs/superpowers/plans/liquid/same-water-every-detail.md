# Liquid review: the same water at every Detail, and faster

Asked by the owner on 2026-11-21, after the spray fix (`spray-and-bounce.md`, PR #203): "There is a huge
difference in how the liquid behaves when changing the detail level, especially in the Water Weird project
file. On Extreme, the liquid behaves much closer to how I would expect in real life, but on Normal it behaves
much more viscous and like sludge rather than a water-like liquid. Additionally, the playback is extremely slow
and rendering takes a very long time." He asked for a thorough review of every aspect of liquid: physics,
visuals (with and without lighting), performance, bugs.

Debugged systematically (reproduce, measure, one change at a time, on a scratch test and scratch copies of the
shader that are not kept). This file records what was measured, then the fixes in three parts. Part 1 is built
in the thread that wrote this plan; parts 2 and 3 each get a fresh thread.

The owner's scene ("Water Weird"): a hollow 840 x 1680 box, a filled turned plate and a filled circle in it, a
Liquid thing that is no shape's child pouring onto the plate (one Pour, Rate 0.32, Width 60), Detail Fine,
Export detail Extreme, Size Bucket, the scene's gravity 500 (so the liquid's is 4,000 px/s²).

## What was measured (main c. PR #203, release build, software Vulkan)

**The complaint, reproduced.** The owner's scene at each Detail, mean speed² per drop at 6 s: Normal 436k,
Fine 507k, Very fine 641k, Extreme 884k px²/s². In pictures Normal is a calm blob with mounds and a dotted
pour; Extreme has sheets, arcs and crowns.

**A slosh test** (still water in the 600 x 900 test box at level 0.3, Normal; gravity pulled sideways by 250
px/s² for the first 0.4 s; the centre of mass's swing read every 0.05 s). The period is 1.45 s (theory for
that box: 1.46 s), so the wave speed is right. The swing's size from one half swing to the next: 0.66, 0.59,
0.46, 0.15, then dead. Water in a box that size keeps swinging for many seconds.

### Cause 1: the sums on the grid truncate, which is a friction

`fixed()` in `liquid_sim.wgsl` turns each drop's share of mass, volume and momentum into an integer with
`i32(x * scale)`, which rounds **toward zero**. Every one of the 36 adds a drop makes per substep therefore
loses up to one unit, always in the direction that slows it:

- Momentum (scale `S_P` 4096): about half a unit per add per axis, so each transfer takes a fixed bite out of
  the speed, the same bite however slowly the drop moves. That is dry friction, not viscosity. At Normal,
  Bucket it is about 320 px/s² (8 % of the liquid's gravity); a lone drop at Size Pool loses about half of
  gravity. Slow flows (a pool levelling, the last of a slosh) stop dead; that is the sludge.
- Mass (scale `S_M` 2^20) is biased low by a little, and velocity = momentum / mass, so speeds are inflated by
  about 1.7e-5 per transfer: +11 % a second at Normal's 38 substeps, +26 % at Extreme's 64.

So slow liquid is braked and fast liquid is fed, with a balance that depends on the number of substeps: the
Detail levels cannot agree. With `round()` in `fixed()` (nothing else changed) the slosh's half-swing ratios
are 0.88 to 0.94 and it is still swinging at 5 s.

### Cause 2: the pressure reads the grid's summed liquid, which depends on where the walls sit

With the friction gone the still pool no longer comes to rest (mean speed² about 1,000 px²/s² and rising;
it was 65 with the friction). The friction was hiding this:

- A drop's squeeze J is 1 / (the liquid volume the grid holds round it, walls counted as full: PR #203). How
  much a wall adds depends on where its face falls between grid nodes. Still box at Normal with rounding, mean
  speed² at 4 s by the box's inner width: 600 px 1,000; 594 px 5,470; 596 px 4,700 (drops thrown at 1,070
  px/s); 598 px 3,154; 604 px 2,258. By the wall's thickness: 36 px 13,000 and rising; 40 px 23 (calm);
  44 px 400 to 1,800. Detail changes the grid, so every Detail level puts the walls somewhere else: the same
  scene is calm at one Detail and simmering at another.
- The top two cells of any liquid read less than full (the kernel hangs out into the air), so they carry no
  pressure, and any sheet thinner than about four cells carries none at all. At Normal a 30 px sheet is "dust
  that sticks"; at Extreme the same sheet is ten cells thick and behaves as water. The surface rows also
  settle into separate lines with a gap.

### What is not the cause (each tested alone, no effect on the slosh or the simmer)

Surface pull at 0; Settles at 0; the squeeze damping (`BULK`) at 0; "holes close" off; a little real
viscosity; skipping the far side of a thin wall when a drop gathers or scatters (and counting the far side as
full, which was worse). "Thick walls are quiet" turned out to be the grid position again (36 px was the worst).

### Cause 3: "holes close" stirs liquid that has no holes

"Holes close" (PR #198, narrowed by #203) moves a drop down the slope of the liquid round it at `CLOSE_RATE`
120 a second. With the pressure read off the grid, the pressure itself ironed the liquid's density flat to
about a hundredth, so in sound liquid the slope was nearly nothing. With the carried squeeze the density is
free to sit within a tenth of full by how the drops happen to lie, and a tenth a cell is a drift of 12 cells a
second: in the still box 600 px wide it pumped the water up to 6,000 px²/s² and rising (the same box with it
off: 85), while at 594 px it happened not to. Lower `BULK` or bigger time steps were first blamed for this;
they are not the cause (with "holes close" off, `BULK` 0.35 rests at 89).

It cannot simply go: with it off the pool under a pour is full of holes for good (drops bunch into strings with
gaps between: 3.4 drops in each cell that holds any and many empty cells, and nothing else evens them out).
So it is kept but **only where liquid is really missing**: the rim of a real hole or crack reads 0.6 to 0.9 of
full (walls counted), sound liquid 0.9 to 1.1. It now acts fully below 0.8 and not at all above 0.95
(`HOLE_BELOW`). The owner's pour then still leaves a solid pool, and every still box rests.

### What stays different between Detail levels (and why that is honest)

With part 1's model the owner's scene has the same flow at every Detail (a jet plunging to the floor, a fast
layer along the floor to the left wall, up the wall and back along the top: the rolling mounds either side of
the jet are real, the jet's push holds them up), but its speed still grows with Detail: the pool's rms speed at
5 s is 480 px/s at Normal, 534 at Fine, 712 at Extreme. The floor layer is about 15 px thick. Extreme draws it
with five cells; Normal has two, and a grid cannot carry a layer thinner than about three cells without
smearing it into the liquid above. Detail is, literally, how thin a sheet the liquid can make. No change to
the model removes that; only more cells do, which is why part 2 (speed) matters as much as part 1.

Also: Extreme and Very fine hit the 64-substep cap (`MOST_SUBSTEPS`), so their stiffness is capped (Extreme's
sound speed is about 6,180 px/s against Normal's 10,777). That makes them a little softer, not calmer; left
alone.

## Part 1: the squeeze each drop carries (this thread)

Six changes (four planned, two found through the code review: see "As built"), all in
`crates/bpe-gfx/src/shaders/liquid_sim.wgsl`, plus one new number on each drop. No setting, no file format
change, nothing in the inspector.

### 1. `fixed()` rounds to nearest

`return i32(round(clamp(x * scale, -LIMIT, LIMIT)));`. Any later fixed-point sum must do the same.

### 2. Each drop carries its own squeeze

`Drop` grows from 96 to 112 bytes: a new last member `more: vec4<f32>` (Rust `more: [f32; 4]`), of which
`more.x` is **the squeeze it carries, as J − 1** (0 at rest, below 0 squeezed, above 0 stretched). It is kept
as J − 1 and not J so that float precision is spent on the small number: pressure is the stiffness (millions,
in grid units) times it. `more.yzw` are spare (0). 2^20 drops x 112 bytes is 112 MiB, still under the 128 MiB
a binding may be. Everything that reads the drops as bare floats moves from 24 to 28 floats a drop
(`DROP_FLOATS` in `liquid_drops.wgsl` and `liquid_dots.wgsl`; `liquid_stats.wgsl` has its own `Drop`).

`p.j` stays exactly what it was (1 / the liquid alone round the drop): the picture reads it to draw a lone
drop as a droplet, and `alive()` checks it. It no longer drives the pressure.

- **Born at rest**: `Drop::new` leaves `more` 0, so every Fill, Pour, Burst and reaction drop starts
  unstressed however unevenly it was placed. `cs_rewind` (Un-mix played back) sets it to 0 too.
- **`cs_p2g`**: `e = p.more.x` (0 if not finite). Pressure `-stiff * e` when `e < 0`; pull
  `-stiff * pull * e` when `0 <= e < tear`; nothing beyond. The squeeze damping keeps its condition
  (`e < tear`). The loop that gathered the walls' share at the drop (9 nodes, 18 buffer reads) goes: the
  carried squeeze needs no walls. That takes back the 8 % a step that PR #203 cost.
- **`cs_g2p`**, after the gather (which already has `vd`, the volume with the walls' share, its `slope`, and
  the new velocity matrix `c`):
  - What the liquid round it says: `r = |slope| / vd`; `full = 1 / (1 + 0.2023 r + 0.6425 r²)` is the share
    of a drop's reach that lies inside the liquid when it sits at a flat open surface with that `r` (fitted to
    the quadratic spline applied twice, drops to grid and grid to drop: a drop on the surface line reads 0.5
    with `r` about 1.1, one deep inside reads 1 with `r` 0); `dens = vd / max(full, 0.2)`; `jref = 1 / max(dens, 0.05)`. So a flat surface, a thin sheet
    and the liquid's inside all read about 1.
  - Torn (`e >= tear`): `e = min(jref − 1, tear)`: it joins again as soon as the liquid round it is dense
    enough, taking what the liquid says.
  - Else carried: `e += (1 + e) * dt * (c.x + c.w)` (the flow's own squeezing), then a safety net with a wide
    dead band so that the pressure does not hear the grid's sampling noise: with `TOL` 0.15, if
    `1 + e > jref (1 + TOL)` (the liquid is more packed than the drop thinks) it moves to that edge at
    `RELAX_DOWN` 30 a second; if `1 + e < jref (1 − TOL)` (less packed) at `RELAX_UP` 4 a second. Then
    `e` is kept in `[-0.9, tear]`.
  - The asymmetry matters: over-packing must be answered fast (it is what bursts), while "less liquid than I
    think" is the normal state of any drop near an edge the estimate gets wrong, and answering it fast was
    what made the corners simmer.
- Balls are not walls here: the liquid round a sunk ball is short of drops on the ball's side just as it
  was at a wall before change 5, and no node is held there, so it should still pack a little round sunk balls
  (the first quirk the spray fix left). Reasoned, not measured: part 3 should look at it, and mirroring drops
  in a ball as in a wall is the likely cure.

### 3. Nodes across a thin wall from the liquid move along the wall with it

A drop within a cell and a half of a hollow shape's wall (a band, `t.z >= 0`) spreads onto nodes on the far
side of the wall. `cs_grid` treated those as ordinary wall nodes (speed into the wall removed, speed away
kept), so they moved "away from the wall" on their own side, which is toward nothing, and fed that back to the
drop. Such a node is a **ghost**: all of its speed along the normal (relative to the wall) is removed, both
ways, and the speed along the wall is kept. Other wall nodes keep the old rule. Filled shapes (`t.z < 0`) are
unchanged.

Which nodes are ghosts (corrected after the code review, see "As built"): only a band node near enough the
wall's line to hold weight from across it, `|s.z| < 1.5 (|n.x| + |n.y|) - 0.5 - t.z` (drops stop half a cell
short of the face and reach a cell and a half each way along x and y). For those, `cs_grid` reads the liquid's
volume `GHOST_LOOK` 1.5 cells off each face of the wall, straight out from the node's nearest point on the
wall's line; the node is a ghost when the other side holds more than `GHOST_MORE` 4 times its own.

### 4. "Holes close" only where liquid is missing

In `cs_g2p`: `hole = 1 − smoothstep(HOLE_BELOW − 0.15, HOLE_BELOW, vd)` with `HOLE_BELOW` 0.95 multiplies the
move (cause 3). `CLOSE_RATE`, `CLOSE_ACROSS` and its rules at walls stay as they are.

### 5. A wall pushes back as the liquid would (added after the code review)

The pressure force on a node is a sum over the drops round it. Within two cells of a wall that sum is short
of drops on the wall's side (drops stop half a cell off the face), so the node is pushed at the wall by the
drops on its other side with nothing to answer them, and only nodes within 0.75 of a cell of the face are
stopped from moving into the wall. The liquid therefore packed against every wall until that layer was
squeezed enough to hold the nodes (measured: half as many drops again in the half cell next to a wall), and a
layer squeezed more than its depth asks is lighter than the liquid beside it: it rose up every side wall at
140 to 190 px/s and came back down the middle, for good. Read off the grid, the old pressure hid this (it
ironed the density flat again), and the thin-wall rule as first built hid it by accident (see "As built").

So each drop near a wall is **mirrored** in it (`cs_p2g`, after its own nine nodes): the mirror is the plane
`WALL_GAP` (a quarter cell) short of where drops stop, so drops at rest and their mirrors are evenly spaced.
A drop within a cell and a half of the plane scatters once more from its mirrored place: only to nodes on the
liquid's side of the plane, only the push along the wall's normal, with the drop's own pressure plus what the
depth between drop and mirror adds (`weight * g . n * 2 * off`, so a floor carries the liquid's weight).
Loose drops (no pressure) have no mirror; a mirror off the grid's edge is skipped.

It has to be the drops' own sum, mirrored. A smooth stand-in (the walls' share as a force: the integral of the
same kernel over the wall's place) was built first and is a hundredth or two out against the real drops' sum,
depending on where the nodes fall; at the bottom of a pool thirty cells deep a hundredth of the pressure is a
third of gravity, and the wall layer then sank at 70 to 190 px/s instead of rising.

Not done: only the nearest wall is mirrored (a drop in a corner has one mirror); with liquid on both sides of
a thin wall both sides are mirrored in full although the far liquid already pushes (measured calm: 8 px²/s²
with a hollow box standing in a pool); the picture's edges need none (drops there stop on the nodes that stop
them, which is the ordinary arrangement and balanced as it is: measured 8 px²/s²); balls are not mirrored.

### 6. A drop a wall stops is squeezed by it (added after the code review)

The grid's flow carries a drop into a wall's margin and `cs_g2p` puts it back on the line; the carried squeeze
goes by the grid's flow and knew nothing of that, so drops collected on the line unsqueezed (with 36 px walls
the liquid within a cell and a half of the wall read 12 to 17 % over-full, the safety net kept answering it
and stirred the pool: 2,300 px²/s² and rising at 5 s). Now a drop moving into the wall when it is put back is
squeezed by `WALL_SQUEEZE` 2 times how far it was put back, in cells (its own room is half a cell), and never
by more than twice what it is moving into the wall in that substep, so a drop two walls pass between them in
a corner is not squeezed for that (squeezed by the bare distance, three drops were shot through a 3 px wall).

### Measured with the scratch copy of this model (before building it properly)

- Still box, Normal, mean speed² at 4 s: 60 to 210 at every width (594 to 604) and wall thickness (36, 40)
  tried; over 10 s 80 to 160 (width 600) and 20 to 34 (594); fastest drop under 51 px/s; 0 to 2 stacked drops;
  the surface is one clean line.
- Slosh half-swing ratios 0.84 to 0.96.
- The owner's scene, mean speed² at 5 s: Normal 611k, Fine 655k, Very fine 783k, Extreme 847k (it was 436k to
  884k: Normal gains 40 %, Extreme is as lively as before).

### Tests (written first, in `crates/bpe-kit/src/things/liquid/tests.rs`)

1. `a_slosh_in_a_box_keeps_swinging`: the still box at Normal, gravity pulled sideways 250 px/s² for 0.4 s,
   the centre of mass's x read every 0.05 s to 4.5 s. It must turn at least five times, a half swing takes
   0.6 to 0.86 s, each of the first four half swings keeps more than 0.7 of the one before (they kept 0.66,
   0.59, 0.46 and 0.15) and the fifth is still more than 0.4 of the first.
2. `still_water_rests_wherever_the_walls_sit_on_the_grid`: the still box at widths 594 and 604 with 3 px
   walls, and at width 600 with 36 px walls: after 5 s more than 1,000 drops, rms speed under 25 px/s, no drop
   thrown 120 px above the level, stacked under 0.5 %.
3. The existing `still_water_in_a_box_...` and `water_poured_onto_a_shape_...` keep passing unchanged.
4. `bpe-gfx/src/liquid_sim/tests.rs` as it is (it checks the solver on the card directly), plus
   `a_lone_drop_falls_as_fast_as_gravity_says`: one drop, gravity 60 cells/s² (Size Pool at Normal), 32
   substeps, falls within 2 % of g t² / 2 over a second (with the truncation it fell 0.05 cells of 30: it hung
   in the air).

### Checks before the pull request

`cargo test -p bpe-gfx liquid -- --test-threads=2`, `cargo test -p bpe-kit liquid -- --test-threads=2`, the
liquid picture tests (`liquid_pictures`, `liquid_look_pictures`, `lit_liquid_pictures`), `restarts`,
`golden_liquid` in debug and release (it pins counts, which this should not move; re-bless only if it does and
say why), clippy and fmt. The full-size look check for the liquid scenes (`LOOK_ONLY=liquid`), pictures opened
at 1:1. CI runs everything else.

### What not to do

- Do not let "holes close" act on sound liquid again (cause 3), and do not turn it off (the pool under a pour
  fills with holes).
- `BULK` and `CFL` are not changed here. Part 2 may try a bigger `CFL` (see there).
- Do not make the dead band (`CARRY_SLACK`) narrow or the upward relax fast: both bring the simmer back.
- Do not swap the mirrored drops for anything smooth or cheaper without measuring every still box (change 5):
  the walls' push has to match the drops' own to a small part of a hundredth.
- Do not calm liquid near walls by taking speed off grid nodes: that was the accident (see "As built").
- Do not feed `p.j` to the pressure again, and do not change what `p.j` holds (the picture depends on it).
- Do not remove the walls' share from the gather in `cs_g2p`: `jref` and "holes close" still need it.

## Part 2: speed (fresh thread)

Nothing here could be timed on a real graphics card (the cloud has a software one: 6 s of the owner's scene took
78 s at Normal, 168 s at Fine, 456 s at Very fine, 682 s at Extreme). The cost on a real card is the number of
passes: liquid steps a second (240, or 120) x substeps (38 at Normal to the cap of 64) x five dispatches
(clear grid, drops to grid, grid, grid to drops, tick), each with 36 atomic adds a drop. In order of expected
gain against risk:

1. **Measure first, on the owner's card.** Add a "Liquid: N drops, S substeps, X ms a frame" readout if the
   status line does not already give all three, and ask the owner for the three numbers at Fine and Extreme in
   his scene before and after. Without them nothing below can be judged.
2. **Fewer dispatches a substep.** Fold `Tick` into another pass (it is its own dispatch only to bump a
   counter) and clear the grid inside the grid pass of the substep before (the grid pass already visits every
   node; G2P only reads the two speed words). Five dispatches become three.
3. **Only the rows that hold liquid.** The clear and grid passes run over the whole grid (at Extreme 360 x 640
   = 230,400 nodes) even when the liquid is a puddle. Keep the highest and lowest row any drop reached (the
   stats pass already reads the drops) with a margin, and dispatch only those.
4. **Bind groups made once.** `Body::step` builds 23 bind groups every liquid step; they only change when a
   buffer is replaced (room grows) or the walls are put. Cache them.
5. **Walls' share once, not per drop per substep.** `cs_g2p` calls `solid_share` for 9 nodes a drop a substep
   (two buffer reads and a spline each). Work it out per node when the walls are put (they are put once unless
   a shape moves) into a spare word of the `solid` buffer.
6. **Preview at half the liquid steps when playback falls behind**, exports untouched: a liquid already has a
   120-a-second mode; make the preview choose it when a frame's liquid work overruns, and say so in the
   status line. Needs the fingerprint rules read first (`docs/notes/liquid.md`, "Part 1, as built").
7. **Bigger time steps** (`CFL` 0.15 in `sim.rs`; the substep count is the whole cost). Measured with part
   1's scratch model and "holes close" off: at `CFL` 0.3 (half the substeps, twice as fast) the still box stays
   bounded but simmers more (300 to 750 px²/s² at 600 px wide, 90 to 290 at 594, against 80 to 160 and 20 to 34
   at 0.15; fastest drop 73 to 79 px/s against under 51). Try 0.2 and 0.25 with part 1's tests (still boxes,
   slosh) and the owner's scene, and take the biggest that passes them without loosening them.
8. Not worth it (measured or reasoned): fewer drops a cell (the holes get worse), 16-bit drops (the carried
   squeeze needs the precision).

Each item is its own commit with the readout's numbers before and after (software card here, the owner's card
when he sends them) and the liquid tests of part 1 still green.

## Part 3: the look (fresh thread)

Seen while measuring (full-size, the owner's scene and the test box, no Lighting card):

1. **A bright dashed white line along the floor under a pool** (the wall-face mirror splats' highlight where
   the liquid meets a white wall).
2. **The pour at Normal and Fine is a blotchy dotted column**: at Rate under 1 the Pour scatters single drops
   over its Width (`births.rs::pour`), so the stream is a spray of lone droplets, each drawn boosted. A stream
   of the same flow, narrower and solid at the spout (Width x Rate wide), would look like water and behave the
   same at every Detail; lone drops in a falling stream could also be drawn as stretched streaks along their
   velocity.
3. **Single-file beads running down the side walls** (drops splashed onto a wall slide down it one cell from
   the face and are drawn as a chain of droplets). Decide between letting them fall free and drawing them as a
   thin film.
4. **Small pinholes near where the pour lands** at Normal even with "holes close" on: the draw's reach per
   drop is tied to the cell; a slightly wider reach inside the liquid (not at its edge) would hide them.
5. The full-size look check with a Lighting card (Flat and Stage, `liquid-lit`, `liquid-deep-lit`,
   `lit-liquid-smoke`) after part 1, at 1:1, for seams at walls, flicker between the 1.0 and 1.05 s frames and
   banding in the depth colour. Part 1 changes how drops sit at walls and at the surface, so this has to be
   looked at after it, not before.

## Part 4 (an idea, not planned): a splash below the grid

What is left of the owner's complaint after part 1 is real resolution: at Normal the pour arrives as single
drops, and a single drop that lands in liquid just takes the liquid's speed (the grid shares momentum between
everything on a node, which loses the drop's energy), where Extreme's resolved jet turns into a fast layer
along the floor. A model below the grid could give a landing lone drop the speed it lost back as pressure
(squeeze = lost speed / the speed of sound, which is what stopping water does), so it pushes its neighbours
apart: a small splash. Tried once on a scratch copy (the speed lost while torn off, summed and applied when the
drop joins): no measurable change in the owner's scene at Normal (518k either way), not looked into further.
It needs its own investigation and its own guard against the spray PR #203 removed (the pour test's speed
limit), and should only be picked up if the owner still finds Normal too calm after parts 1 to 3.

## As built

**Part 1** (2026-11-21): built as planned, with one change found while building. The scratch model had been
measured with "holes close" off; switched back on it stirred still water (cause 3 above was first put down to
`BULK` and the time step), which is why change 4 exists. A torn drop joining again takes what the grid says
even when that is squeezed (`min(jref − 1, tear)`); never letting it join squeezed (`clamp(.., 0, tear)`) was
measured in the owner's scene and made no difference at Normal or Extreme, so the first was kept. Numbers and
the full description: `docs/notes/liquid.md`, "The same water at every Detail, as built".

**After the code review** (2026-11-21, the one review of this part). It found that the thin-wall rule as first
built (commit 047ae49: probes two cells either side of the node itself, masses compared, no limit on how far
from the wall) also fired on nodes 2 to 3.25 cells inside the liquid, taking all their speed along the normal
at random. That was calming still water by accident: with the rule corrected (change 3 as it now reads) the
still box simmered again at two of four wall positions (594 px wide 93 px²/s² at 5 s, 604 px 334, 600 px 4,272,
36 px walls 4,495), with the motion in bands up both side walls. Followed drop by drop (the squeeze each drop
carries and what changed it, by distance from the wall), that led to changes 5 and 6, which are the real
cause: the walls did not hold the liquid's pressure. The review's other findings, also fixed: a drop was
"torn" at its liquid's own tear, which Spray takes down to a hundredth, so drops in sound liquid came loose
and joined again all the time, each time taking the grid's noisy reading (now loose only at `CARRY_LOOSE` 0.3
or the tear if that is more, and what it takes on joining is kept in [-0.9, loose]); `q()` (the mixing sums)
truncated like `fixed()` did (now rounds); this plan's tests section did not match the tests. The names in
change 2 are the plan's; in the code `TOL` is `CARRY_SLACK`, `RELAX_DOWN` and `RELAX_UP` are `CARRY_DOWN` and
`CARRY_UP`, and "torn" is "loose".

Measured with all six changes (release, software Vulkan, Normal): still water at 5 s 30 px²/s² (594 px wide),
106 (600), 74 (604), 71 (36 px walls; 50 to 112 out to 8 s), 32 (40 px walls), fastest drop 14 to 44 px/s;
shallow water (27 px deep) 0 to 2; a liquid filling the picture 8; a hollow box standing in a pool 8. Slosh
half-swing ratios 0.89, 0.86, 0.96, 0.94, 0.93. The owner's scene, mean speed² a drop at 6 s: Normal 533k.
Tried and dropped on the way: taking the squeeze's reference from the liquid alone near a wall (worse:
8,000), the smooth wall push at 0.6 and 0.8 strength (still grows), squeezing a stopped drop by the bare
distance (shoots drops through thin walls).

Also measured with part 1 as first built (before the review), for part 2: `CFL` 0.2 leaves the still box at 280 to 330 px²/s² at 600 px
wide (creeping up over 6 s) and 280 to 400 with 36 px walls, against 75 and 224 at 0.15; `CFL` 0.3 gives 370 to
500 and 1,071 and rising.
