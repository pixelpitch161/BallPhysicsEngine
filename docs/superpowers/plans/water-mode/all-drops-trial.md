# Water mode: the all-drops engine trial (throwaway)

Date: 2026-11-25. Brief: `/mnt/project-files/ballphysics/water-mode/all-drops-trial-brief.md` (Aedan, after the
"Water Reality Check" review: "Option B sounds good, but I want to see option C first before committing to B").
Branch `claude/project-thread-awquwz`, a draft pull request for the record only. **Never merged.** Commits that
carry card results say `[skip ci]`.

## The question

Is an all-drops engine (no grid pressure: every drop feels its neighbours directly) clearly better than "keep the
grid body and rebuild thin water as drops" (Option B) as the base for Water mode, judged against real water, on
the RX 9070 XT, at the detail finished videos use? The all-drops engine gets a fair best shot (tuned), and its
weak side (deep pools, cost) is measured, not assumed.

## The method picked, and why

**Position-based drops** (position based fluids, Macklin and Mueller 2013, with the changes below), all on the
card in wgpu compute. (Planned with a kept pressure; that did not survive, see "As built".)

Why this and not IISPH or DFSPH:

- The water's gravity is 8 x the scene's, so fast water crosses one to two drop spacings in a step even at 960
  to 1920 steps a second. The pressure-solving particle methods need about a third of a spacing a step to stay
  stable, which is 4 to 6 times more steps. The position-based method cannot blow up at any step size: a drop's
  place is corrected directly and its speed follows from where it ended up.
- One sweep a step is three passes over the drops' neighbours; DFSPH is ten or more.
- Walls are exact (a drop is moved out of the solid field), which grip and drag need.

Its known weak side is the one the brief names: deep water squashes and jitters, because a place correction only
reaches one neighbourhood a sweep. The three changes are aimed at that and at thin water:

1. **The pressure is kept from step to step** (each drop's multiplier, never above nothing: the liquid pushes,
   it never pulls). Each step starts from last step's pressure and one sweep corrects it, so a pool at rest
   converges to its true weight instead of sagging. Knob `warm` (0 is plain position based fluids).
2. **Solids count as liquid for the density** (the share of a drop's neighbourhood behind the face, in closed
   form), so the first row of drops at a wall is spaced like the rest and the pool keeps its height.
3. **The skin is its own force** (Akinci, Akinci and Teschner 2013: a pull between near drops that turns to a
   push when they touch, plus a pull along the surface's bend), tied to Surface pull the way the grid core ties
   its hold-together (`pull * surface_pull^2 / cell^2`), so the two engines' Surface pull mean the same. With the
   pressure one-sided, nothing else holds the liquid together: break-up into drops is the skin's to decide.

Grip and drag: a pull toward a near face and a loss of speed along it, both from Stickiness (plus a small drag
every liquid has). Loss of energy at impacts: a drop that meets a face keeps none of its speed into it, and
neighbouring drops even out their speeds (XSPH).

Neighbours are found through a sorted cell list (cells one reach wide, 3 x 3 searched): count, running sums,
scatter, then each drop is copied to its sorted place **in the order of its serial within a cell**, so every sum
over neighbours adds in the same order on every run. The drops are physically re-sorted every step (neighbours
sit together in memory).

Limits checked (wgpu 30 defaults, `wgpu-types/src/limits.rs`): 8 storage buffers a stage (the widest pass binds
6), 128 MiB a storage binding (a 64-byte drop: 2 000 000 drops at most; the trial's cap), 65 535 workgroups a
dimension (x 256 = 16.7 million drops or cells a pass).

## What is built

All in `crates/bpe-water/` (the merged grid core is not touched except where said):

- `src/drops/mod.rs`: `DropsGpu` (pipelines), `Drops` (a body: `new`, `add`, `step`, `read`, `copy`, `count`,
  `steps`, `layout`), the same shape as `sim::Water`. Drops come and go as `Drop`s (the grid core's layout), so
  fills, pours, probes and the bridge are shared.
- `src/drops/numbers.rs`: `DropNumbers` (every number, each replaceable by `BPE_DROPS_<NAME>`), `for_cell`.
- `src/shaders/alldrops.wgsl`: the passes `cs_zero`, `cs_predict`, `cs_sum_a/b/c`, `cs_scatter`, `cs_order`,
  `cs_lambda`, `cs_apply`, `cs_finish`, `cs_export`.
- `src/trial.rs` (behind `test-scenes`): `Engine { Grid, Drops }`, `Body` (either engine's body), `Play` (a scene
  on either engine), `rows` (the all-drops pour), the new shots `slope`, `ledge`, `pool_pour`, `beads` (the
  others exist: `course`, `weird`, `still_box`).
- `src/plain.rs`: the plain drawing, on the processor, the same code for both engines: every drop a soft round
  blob, summed and cut at a level, so drops merge and nothing is streaked.
- `src/bridge.rs`, `shaders/bridge.wgsl`: the bridge takes either engine (the all-drops' fullness comes from the
  drop itself), so today's drawing can draw it too.
- `tests/alldrops.rs`: two small tests that run with the others (a pool stays in its box and keeps its height;
  a run repeats, from the start and from a copy) and the measuring runs (ignored): `shots` (films and stills of
  both engines side by side in the plain drawing; `WATER_SHIFT` moves the layout), `pools` (shot 7's table),
  `openings` (the course table), `pace` (speed), `repeats`.
- `crates/bpe-kit/tests/water_core.rs`: `WATER_CORES=old,new,drops` adds the all-drops engine as a third column
  in today's drawing (Water Weird from Aedan's file).

## A step (one sweep)

1. `cs_predict` (each drop): gravity, the liquid's Settles, the speed cap; the place it would reach, in sub-moves
   of 0.4 cell kept off the solids (no thin wall is jumped), plus the kept pressure's move; its cell counted.
2. `cs_sum_a/b/c`: the cells' running sums (512-cell blocks, the blocks, then each cell).
3. `cs_scatter`, `cs_order`: each drop to its sorted place (by cell, then serial).
4. `cs_lambda` (neighbours): the density there (the spiky kernel, with the solids' share), how over-full it is,
   the change of its multiplier (kept at or below nothing).
5. `cs_apply` (neighbours): moved by its own and its neighbours' changes (spiky slope), kept off the solids; its
   new speed is how far it really went.
6. `cs_finish` (neighbours): speeds evened (XSPH), the skin's pull, grip and drag, its surface direction, and
   the kept pressure's move for the next step.

More sweeps (`iters`) repeat 4 and 5.

## Numbers to start from (tuned in the cloud at coarse detail, then on the card)

Reach 1.5 cells (three drop spacings, about 28 neighbours). Steps a second: twice the grid core's (480 at 12 px
cells, 960 at 8 and 6 px, 1920 at 3 px and finer), four sweeps a step, `warm` 0 (as built). Surface pull, Stickiness, Settles and
Gravity come from the liquid's `Feel`, as in the grid core.

## The shots and the measures

As the brief: tap stream onto a slope; sheet off a ledge; thick pour into a pool; pool draining through gaps
(`course`); beads on a slope; Water Weird (Aedan's file, today's drawing as well); deep pool at rest and a
slosh. Both engines, plain drawing first, at Fine (6 px cells) and Extreme (3 px), one run at 2 px. For each:
what real water does is written down first, then both are judged against that.

Measured: wall seconds a scene second at 6, 3 and 2 px cells (and the drop count); the pool table (movement left
after 20 s, height kept, swings of a slosh); the course's shares through each opening; thin water on four
positions of the layout (`WATER_SHIFT`), though the all-drops engine has no grid that the liquid feels (its
cells only find neighbours; the solid field is the one thing that moves with the layout); whether a run repeats
(twice from the start, and from a copy made in the middle).

Card runs: the Remote Control recipe (project memory `liquid-speed`); results as a `[skip ci]` commit under
`card/`, films to `/mnt/project-files/water-core/all-drops-trial/`.

## Not to do

No editor, settings, colour mixing, heat, moving parts, tiles, notes or goldens. The grid core's behaviour must
not change (its tests still pass). No exe built or installed. Not merged.

## How each step is checked

- The engine: `cargo test --release -p bpe-water --test alldrops` (the three small tests) on the cloud's
  software card; `cargo test -p bpe-water` still passes (the grid core untouched).
- The shots and measures: the ignored runs above, in the cloud at 12 and 6 px cells to tune, on Aedan's card for
  everything shown to him.
- The verdict is written against the real-water notes, shot by shot, with the numbers beside it.

## As built and measured

### What changed from the plan while tuning (cloud, software card, 12 and 6 px cells)

- **The kept pressure was dropped** (`warm` 0). With one sweep a step it is a third-order update of the drops'
  places and is unstable for any useful share (checked by its eigenvalues as well as on screen: a pool boils).
  Keeping pressure needs a solve that has converged across the whole pool each step, which is what the grid
  core's multigrid is. Without it a pool's own weight squashes it: the sag goes with depth x gravity x step
  squared over the number of sweeps (12 px box: 4.4 % at 480 steps a second and one sweep, 1.3 % at 960, 0.4 %
  at 1920). So: **four sweeps a step** by default, and more at finer detail (`BPE_DROPS_ITERS`).
- **The softening must be large** (`eps` 4 of a drop's own stiffness). Below about 3 the sweep over-corrects and
  the pool boils.
- **The density uses the same steep kernel as the push** (spiky for both). With the usual smooth one (poly6)
  drops pair up and line up in worm-like strings, because that kernel is flat where two drops touch.
- **The solids' share is a table** (the share of the kernel behind a flat face at 33 distances, and its fall),
  made once on the processor.
- **A pour's drops must be born evenly spaced, at exactly the resting spacing** (`trial::rows`). Born at random
  (the grid core's way) the stream leaves the spout as spray; born 6 % squeezed (rows closer than columns so the
  amount matches to the drop) the stream throws bits sideways and breaks in the first few diameters. The grid
  core notices neither. So the all-drops pour's amount is the grid core's to within one drop's width
  (Water Weird: 18 px of a 19.2 px pour).
- **The skin**: the pull between drops alone gives a column that narrows as it falls and wets a plate as a
  film; the bend term makes beads round. The bend term needs the surface's direction from a smooth kernel (from
  the steep one it is the nearest drop's way and the surface turns ragged). At `curv` 0.5 a thick pour (16 drops
  across) rings and tears itself into blobs from the spout; at 0.25 it is a clean tapering column and beads
  still round up. Default 0.25.
- **The skin's strength is capped by the step**: stable while its pull moves a drop less than about 0.005 cell
  a step from rest (`pull_cfl`), ragged at 0.016. A strong skin at finer detail therefore needs more steps a
  second (the pull grows with 1 / cell cubed).
- Tried and left off (knobs kept): a pull-together inside stretched liquid (`tens`: over-contracts a thick pour
  into a thread and opens bubbles in a pool); the skin's pull only at the surface (`coh_in`: no visible gain).

### Checked in the cloud

- `the_all_drops_pool_stays_in_its_box_and_keeps_its_height`, `the_all_drops_engine_repeats` pass.
- `repeats` (software card): both engines give the same drops twice from the start and from a copy made half
  way (Water Weird and a box, 6 px).
- Physics alone on the software card at 6 px (seconds of wall time for 3 s of scene): slope grid 30, all drops
  9; ledge 31 and 16; beads 27 and 5; pool (41 000 drops) 40 and about 100. Small scenes favour all drops (no
  grid to solve), a big pool the grid.

### On the RX 9070 XT

(Filled in after the card runs: `card/all-drops-trial/`.)
