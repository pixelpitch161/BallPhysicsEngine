# Water mode part 2a: the water core, with streams better than today's (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `bpe-water` holds the Water mode's physics: the trial's pressure-solving core, stepped and
stored in tiles, with whole-number sums, and a real hold-together for thin streams and films. It is judged on
the owner's RX 9070 XT against today's Liquid core and ends with side-by-side clips. No editor work, no Water
scene runs yet (that is part 2b).

**Architecture:** `bpe-water::sim` is a set of WGSL compute passes and a Rust host (`WaterGpu` per device, `Water`
per body). Drops carry everything (place, speed, the APIC matrix, colour, how stretched they are). Each step: the
hold-together pulls stretched drops together through a node grid with a smooth 3-node kernel; the drops spread to a
staggered grid with a 2-node kernel; a one-sided pressure solve (multigrid over red-black sweeps) stops the liquid
squashing; the drops gather their speed back and move, kept off the solid field. The grid is a pool of 16 x 16
cell tiles found through a table, so only tiles that hold liquid are stored and stepped. `bpe-water::bridge`
hands the drops to today's liquid drawing, which is how the two cores are compared with the same picture.

**Tech stack:** Rust (pinned toolchain), wgpu 30 compute (WGSL), `bpe-gfx` for the device helpers and (bridge
only) today's liquid drawing.

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md`, section 3 ("The physics core"), section 2's
"Size" line (tiles), section 9 (checks) and Parts item 2a. The trial's record:
`docs/superpowers/plans/water-mode/physics-trial.md`. The trial's code: branch `claude/project-thread-fd5dex`
(`crates/bpe-gfx/src/shaders/liquid_trial.wgsl`, `crates/bpe-gfx/src/liquid_sim/trial.rs`,
`crates/bpe-kit/tests/liquid_trial.rs`); it is read and ported, never merged.

## Global constraints

- **Balls scenes do not change.** No golden or pinned picture moves. `crates/bpe-gfx/src/liquid_sim/`,
  `crates/bpe-gfx/src/shaders/liquid*.wgsl` and `crates/bpe-kit/src/things/liquid/` are not edited at all.
- **Same card, same result.** Two runs of the same scene on one device give the same drops, bit for bit, and so
  does a run copied part-way and played on. Sums over drops are whole numbers (`fixed()`, atomic adds) or are
  worked out per cell in a fixed order; no pass reads a neighbour that the same pass writes.
- **No read-back in a step.** The host never waits for the card while stepping.
- **Pass mark:** on full-size frames and clips from the RX 9070 XT, Water Weird's pour is as clean on the new core
  as on today's. If it cannot be reached, today's core is moved into `bpe-water` instead and the report says so.
- **Goal lines** (each with its own test scene and clip, each reported reached or not): a two-cell pour stays one
  stream down a whole screen; a sheet off an edge stays a sheet until it meets something; a stream's width is
  steady (no jitter, no beads); streams are clean at draft detail (8 px cells) as well as fine; a stream breaks
  into drops only when Surface pull says so.
- **The trial's table still holds** on the RX 9070 XT: faster than real time at Normal, Fine and Extreme; a still
  deep pool at rest (rms under 2 px/s after 20 s); pool height kept (98 % or more); a 12 px opening flows at
  every Detail; no more than 0.2 % of drops through walls.
- The hold-together may not calm the liquid by damping whole-liquid motion or by taking speed off the grid. If
  still water moves, find what is out of balance (the trial's rule).
- Speed and look numbers given to the owner as facts are measured on the RX 9070 XT. Estimates are called so.
- `bpe-water` may use platform maths (it never feeds the balls' physics); it has no `clippy.toml` ban.
- wgpu without a window goes through `bpe_gfx::headless_device`; tests use `bpe_gfx::test_device()`.
- Do not build or install the exe. Tests: only this part's; CI runs the rest.
- No built-in or example scenes for the owner: the scenes here are test scenes behind the `test-scenes` feature.

## Review focus

Inputs the spec implies but does not spell out. Each has a test in the task that owns the code.

1. **A drop born or thrown outside every stored tile, or off the course:** it must not write outside the grid's
   buffers, and it must not take the solve with it. Off the course it stops at the edge (Task 3,
   `a_drop_off_the_course_is_kept_in`).
2. **More wet tiles than the pool holds:** the tiles that got no slot are simply not solved that step (their
   drops fall freely) and it is counted, never a crash or a wrong write (Task 3, `a_full_pool_of_tiles_degrades`).
3. **A solid thinner than a cell** (a pen wall at draft detail): it is thickened for the physics to one cell, so
   no drop crosses it at any speed the core allows (Task 1, `a_thin_wall_holds_a_fast_pour`).
4. **Liquid shut in completely** (a full closed box, then a pour into it): the solve has no way out; drops must
   not blow up or tunnel (Task 1, `a_full_box_stays_shut`).
5. **A copy taken mid-run** (the editor's seek, a final redone from a restart point): the copy continues exactly
   as the original does (Task 3, `a_copy_plays_on_the_same`).

---

## Design decisions (all of them)

### The crate

1. **`crates/bpe-water`**, after `bpe-gfx` and before `bpe-kit` in the crate order. Dependencies: `bpe-gfx`,
   `wgpu`, `bytemuck`, `glam`, `anyhow`, `pollster`. Feature `test-scenes` (the scenes of decision 21). Dev
   dependencies: itself with `test-scenes`, `png` (debug pictures). `bpe-kit` gets `bpe-water` as a **dev**
   dependency only (the comparison in Task 4); no shipped crate depends on it until part 2b.
2. **Files** (one job each):
   - `src/lib.rs`: the crate's doc and re-exports.
   - `src/solid.rs`: `SolidField` (decision 5), built on the processor from `Solid` shapes.
   - `src/sim/mod.rs`: `WaterGpu`, `Water`, `Water::step`, `Water::copy`, `Water::read`.
   - `src/sim/drop.rs`: `Drop`, `Born` and the pour and fill helpers.
   - `src/sim/numbers.rs`: `Numbers` (the solver's fixed numbers), `Feel` (a liquid's), their GPU twins.
   - `src/sim/tiles.rs`: the tile table and pool (decision 14) and the levels of the pressure.
   - `src/sim/passes.rs`: the entry points' table, bind groups, the order of a step's passes.
   - `src/shaders/common.wgsl` (structs, bindings, tile addressing), `walls.wgsl`, `tiles.wgsl`,
     `hold.wgsl`, `drops.wgsl` (spread and gather), `grid.wgsl`, `pressure.wgsl`: joined in that order into one
     module by `passes.rs` (`concat!` of `include_str!`).
   - `src/bridge.rs`: `Bridge` (decision 19).
   - `src/probe.rs`: read-back measurements and a debug picture (decision 20).
   - `src/scenes.rs` (feature `test-scenes`): decision 21.
   - `tests/core.rs`, `tests/streams.rs`, `tests/tiles.rs`: behaviour tests (run in CI, small grids).
   - `tests/measure.rs`: ignored tests run by hand in release (tables, timings, frames).
   - `crates/bpe-kit/tests/water_core.rs`: ignored; the side-by-side with today's core (Task 4).
3. **Units.** Inside the sim everything is in cells (a cell is 1; cell `(x, y)` is the square from node `(x, y)`
   to node `(x + 1, y + 1)`), as in the trial. The host's API is in world pixels with a cell size:
   `Layout { origin: Vec2, cell: f32, cols: u32, rows: u32 }` with `to_grid` and `to_world`.
4. **A drop** (`Drop`, 48 bytes, `#[repr(C)]`, `Pod`): `x: [f32; 2]`, `v: [f32; 2]`, `c: [f32; 4]` (APIC: xx, xy,
   yx, yy), `e: f32` (how stretched it is, J - 1, decision 9), `kind: u32` (bits 0 to 3 its Liquid, bit 8 it
   stops at the course's edge), `serial: u32`, `colour: u32` (rgba8, linear). Heat, age and the blended feel
   are part 4's and will grow it; nothing in 2a reads more.

### The solid field

5. **`SolidField`**: a grid of signed distances in pixels (below 0 inside solid) at `SolidField::STEP` = 4 px a
   sample over the layout's rectangle, with `build(layout, &[Solid]) -> SolidField` on the processor.
   `Solid` is `Polygon { points, filled, thickness }` (a filled shape, or a hollow one whose wall is `thickness`
   wide), `Circle { centre, radius, filled, thickness }` and `Line { a, b, thickness }`. This is the spec's "one
   solid field" in its simplest form; part 2b builds it from carving, part 3 from pieces and walls, part 5 adds
   the solids' speeds. **Thin solids are thickened for the physics**: every hollow wall and line is taken as at
   least one cell thick (`thickness.max(cell)`), which replaces the trial's thin-wall code (`line_between`,
   `across`, `doubt`) and its pitfall of a node lying on a wall's line. Why: with a true signed distance and a
   wall no thinner than a cell, the distance at a cell's corner nodes always sees the wall, and its slope is a
   usable normal everywhere a drop can be.
6. **On the card** the field is one buffer of `f32` distances in cells at the grid's nodes (`solid`), written when
   the field changes. Per cell, `cs_walls` works out (once a change, per stored tile): the open share of its
   left and top faces (from the corner nodes' distances less `GAP` = 0.25, cut-cell, as the trial's
   `open_share`), and the walls' share of the cell middle's kernel (`hat_beyond`). A drop is kept half a cell
   off a face by the field's bilinear distance and slope at the drop (`keep_off`), with sub-moves of at most
   0.4 cell.
7. **The course's edge**: drops with bit 8 stop `EDGE` = 2 cells inside the grid, as the trial. Others leave
   and are parked (not stepped) once off the grid.

### The pressure core (ported from the trial; see its record for why each choice)

8. APIC drops; a staggered grid (u on left faces, v on top faces, pressure in cell middles); bilinear spreading
   (20 atomic adds a drop); the one-sided solve (P >= 0; a cell with room may fill, an over-full one empties);
   two multigrid V-cycles of red-black sweeps a step with the trial's three fixes (coarse faces halved but not
   the way out to the air, a floor per coarse cell, no lack handed down from a cell at its floor); the kept
   pressure is state; free slip (`cs_slip`); walls count as full liquid in a cell's density; one projection a
   step at 240 steps a second; a drop moves at most 8 cells a step. The trial's numbers start as they ended
   (`Numbers::default()`: cycles 2, pre 2, post 2, coarse 16, push_out 0.25, most_out 0.1, fill 0.25,
   face_floor 0.005, sub_move 0.4, most_move 8). Every number can be replaced by a `BPE_WATER_*` environment
   variable in the measuring tests only (`Numbers::from_env`), never in the library's defaults.

### The hold-together (new; the part 2a exists for)

9. **What today's core really does**, read from `liquid_sim.wgsl` (`scatter`, `gather`): each drop carries its
   squeeze `e` = J - 1, moved by the flow's own divergence and tied loosely to the density round it; a
   stretched drop (`0 < e < tear`) pulls with a pressure `-stiff * pull * e`, a torn one (`e >= tear`) neither
   pushes nor pulls and joins again where the liquid is dense. So its streams hold because of a **spring in
   tension that grows as the liquid is pulled apart**, felt through a smooth 3-node kernel. That is why the
   trial's two tries failed: a pressure allowed "a little below nothing" pulls equally hard however far apart
   the drops are (nothing evens out their spacing, so they gather in lumps), through a 2-node kernel whose
   slope jumps at every cell line.
10. **The method.** The new core keeps the solve for pushing (liquid cannot squash) and adds today's kind of
    pull for holding together, as its own force:
    - Each drop carries `e`. After the gather, `e += (1 + e) * dt * (c.xx + c.yy)` (the flow's divergence at the
      drop). Where the liquid round it is full (its cell's density at 1 or more), `e` relaxes to 0 at
      `hold_settle` a second: inside a pool nothing is stretched. `e` is kept in `[-0.5, tear]`.
    - **Hold scatter** (`cs_hold_p2g`): every drop adds its mass to the 3 x 3 nodes round it (quadratic
      B-spline, 9 atomic adds); a drop with `0 < e < tear` also adds the pull's impulse
      `dt * vol * q * grad w`, `q = -pull * e` (18 more), plus a damping term on its own divergence
      (`hold_damp * sqrt(pull) * (c.xx + c.yy)`, today's `BULK`), so the spring does not ring.
    - **Hold grid** (`cs_hold_grid`): each node's change of speed is impulse over mass. A node inside a solid
      counts the solid as liquid that cannot move, scaled by the Liquid's Stickiness: a stretched drop beside a
      wall is pulled to it, which is what makes a film hug a shape and leave its edge as one stream.
    - **Hold gather**, fused into the spread (`cs_p2g`): the drop takes the nodes' change of speed with the same
      kernel, then spreads to the staggered grid as the trial does. Mass and force use one kernel, so a lone
      drop pulls on nothing (no self-force) and two drops pull on each other equally.
    - The solve then runs as before. The pull can only draw liquid together until its cells are full; from
      there the solve holds it.
    - A torn drop (`e >= tear`) is loose: no pull. It joins again when its cell's density is over
      `hold_join` (0.6): `e` is set from the density.
11. **The numbers and what the owner's settings mean.** `Feel { gravity, surface_pull, spray, stickiness }` per
    Liquid. `pull` (cells squared a second squared per unit stretch) is `PULL * surface_pull^2 / cell^2`-scaled
    so the same setting holds the same stream at every Detail, capped at `(0.3 / dt)^2` so the explicit spring
    is stable in one step; `tear` is `lerp(2.0, 0.2, spray)`. `PULL`, `hold_damp`, `hold_settle`, `hold_join`
    and the cap are found by Task 2's measurements and written into the "As built" section with the runs that
    chose them. Starting values: pull 5000 cells^2/s^2 at the default Surface pull 0.2 and 8 px cells, tear
    1.0, hold_damp 0.7, hold_settle 20 /s.
12. **How it is judged before the card** (Task 2, in the cloud, at 8 px and 6 px cells): the stream measures of
    `probe.rs` on the stream scenes (decision 21), each also looked at as a debug picture, and the side-by-side
    with today's core through the bridge at full size. The measures: **pieces** (connected groups of drops
    with a joining distance of 1.5 cells, counted in the air between the spout and the first thing the stream
    meets), **strays** (the share of airborne drops more than 2 cells from the biggest piece), **width** (the
    stream's width at five heights, its mean and its change over 2 s), **gaps** (the longest empty stretch
    along the stream). Today's core is measured the same way on the same scenes through its own Body, so
    "as clean as today's" and "better than today's" are numbers as well as pictures.
13. **If v1 is not enough, in this order** (each tried only when the measures say the one before fell short,
    each recorded in "As built"): (a) a spacing term: drops in loose liquid shift a little down the slope of
    the node density (today's "holes close", turned to evening out a stream); (b) the pull felt through a wider
    kernel (nodes of a grid twice as coarse) for streams more than 3 cells from breaking; (c) the pull solved
    with the pressure (a compliant cell: the solve's diagonal gains `1 / (pull * dt^2)`), which lifts the
    stability cap at fine detail; (d) the fallback of the global constraints: today's core moved into
    `bpe-water`. Not on the list, on purpose: damping the liquid's speed, redrawing streams in the picture to
    hide broken physics, or adding or removing drops (a stream of drops that split would break "same card,
    same result" without a full ordering pass; it is a later idea, noted for part 8).

### Tiles and sums

14. **Tiles.** A tile is 16 x 16 cells. `table[ty * tcols + tx]` is the tile's slot in the pool or `NONE`.
    Per-cell buffers (`sums`, `wallc`, `vel`, `vel_out`, the hold nodes, `cellp` and `coef` level 0) are
    `slots * 256` cells long. A cell's index is `slot * 256 + (y & 15) * 16 + (x & 15)`; a neighbour in another
    tile goes through the table (`cell_at(x, y) -> u32`, `NONE` when that tile has no slot, read as "no liquid,
    open air"). The pool has `Water::tiles_room` slots, set when the body is made (default: every tile of the
    layout up to 4096 slots, about 1 million cells; a bigger course gets a pool smaller than its table).
15. **Which tiles are stored.** Each step, before the spread: `cs_mark` (per drop) stamps the step's number on
    the tile of every node its kernels reach (its 3 x 3 nodes, so up to 4 tiles); `cs_tiles` (per table entry)
    gives a stamped tile without a slot one from the free list and takes the slot back from a tile not stamped
    for `KEEP_STEPS` = 8 steps, clearing what it held. Slots are handed out with one atomic counter over a free
    stack. **Which slot a tile gets does not change any number worked out** (every pass addresses cells by
    position through the table), so the hand-out order need not be fixed; a test proves it by running the same
    scene with the pool shuffled (`slots_do_not_change_the_drops`). A new tile's walls are worked out by
    `cs_walls` for that slot in the same step.
16. **The pressure's levels in tiles.** Levels 1 to 4 (cells of 2, 4, 8 and 16 fine cells) live inside each
    slot as a pyramid (64 + 16 + 4 + 1 cells after the 256), addressed through the same table. Levels 5 and up
    are dense over the table (`tcols x trows`, then halved to a handful). Sweeps of levels 0 to 4 are
    dispatched over the pool's slots (a workgroup is a slot; a slot not in use returns at once); levels 5 and up
    over their small dense grids.
17. **Dispatch without read-back.** The host dispatches every per-tile pass over the whole pool
    (`tiles_room` workgroups); an unused slot costs one read. (Indirect dispatch from a count on the card is a
    later speed-up; measured first.)
18. **Sums.** Drops to grid: fixed-point atomic adds, to nearest (`fixed()`, scales `S_M` = 2^20 for mass and
    volume, `S_P` = 4096 for momentum), as today. Grid passes: each cell from its own sums and its neighbours'
    values of the pass before, in the code's fixed order. Sweeps: red-black, so a cell only reads the other
    colour. `cs_down`: the four cells in a fixed order. No float reduction over many items anywhere.

### Comparing, measuring, scenes

19. **`Bridge`** (`src/bridge.rs`): `Bridge::new(&Water)`, `Bridge::source(&mut self, &Water) ->
    bpe_gfx::liquid_sim::DropsSource`. One compute pass (`cs_bridge`) writes each drop as today's 112-byte
    `bpe_gfx::liquid_sim::Drop` (place, speed, C, colour, `j` from the cell density, kind, serial) and the wet
    grid's wall numbers from the solid field, so today's drawing (`Frame.liquids`, `LiquidStyle`) draws the new
    core's drops exactly as it draws its own. It exists for the comparison and for part 2b's plain look; part 6
    replaces it.
20. **`probe.rs`**: `Water::read() -> Vec<Drop>` (waits; tests and measuring only), `Stats::of(&[Drop], &Layout)`
    (count, rms speed, fastest, the 95th-percentile height, drops outside a box, crowded pairs), `Stream::of(...)`
    (decision 12's measures), `picture(&[Drop], &SolidField, &Layout, scale) -> image bytes` and `save_png`
    (drops as discs on the solids: a debug picture for the builder, never shown as the liquid's look).
21. **Test scenes** (`scenes.rs`, feature `test-scenes`), each `fn(detail_cell: f32) -> Scene` where
    `Scene { layout, solids, feel, fill: Option<Rect>, pours: Vec<Pour>, seconds }` and `Scene::start(&WaterGpu)
    -> Running` (`Running::step()` births and steps one liquid step):
    - `still_box(level)`, `slosh()`, `course()`: the trial's box (600 wide), its slosh start and its course
      (two pools, a corridor, 12 px and 24 px openings), for the trial's table.
    - `weird()`: the owner's Water Weird rebuilt by hand: the 840 x 1680 hollow box, the tilted quad at
      (120, -240) size 180, the disc at (-180, -120) size 120, a pour at (120, -570) aimed down, width 60, rate
      0.32, speed 300 (x sqrt 8 for the Bucket size), gravity 500 x 8 px/s^2.
    - `thin_pour()`: a pour two cells wide from the top of a screen with nothing under it but the floor 1900 px
      below (goal line 1 and 3).
    - `sheet()`: a pour onto a level shelf that ends in the air; the liquid runs off the edge (goal line 2).
    - `spray_and_rope()`: `thin_pour` onto a small disc, run at Surface pull 0, 0.2 and 1 (goal line 5).
    Draft detail (goal line 4) is every stream scene at 8 px and 12 px cells.
22. **The step's clock.** 2a steps at a fixed 240 liquid steps a second (`Numbers::rate`), as the trial was
    measured. The spec's "the water picks its own step" needs the fastest drop read back late; it is left to
    part 2b with the playback, and noted.

## Things not to do

- Do not edit today's Liquid code, even to share a helper: copy what is needed into `bpe-water`.
- Do not merge or cherry-pick the trial branch. Port by reading.
- Do not tune by eye alone: every change to the hold-together is run through the measures of decision 12 and
  the still-pool and volume checks, so a prettier stream cannot quietly cost the pools.
- Do not hide broken streams in the picture (longer streaks, more smoothing in the bridge's drawing). The
  bridge uses today's Look defaults unchanged for both cores.
- Do not hand-pick the best seed, time or scene for the clips: the clips are whole runs of the named scenes.
- Do not let a test wait on a number tuned to one card: behaviour tests use bounds with room (lavapipe and the
  Radeon differ in their last bits).

## Tasks

### Task 1: the crate, the solid field, the trial's core ported (dense grid)

**Files:** create `crates/bpe-water/Cargo.toml`, `src/lib.rs`, `src/solid.rs`, `src/sim/{mod,drop,numbers,passes}.rs`,
`src/shaders/{common,walls,drops,grid,pressure}.wgsl`, `src/probe.rs`, `src/scenes.rs`, `tests/core.rs`.

**Produces:** `WaterGpu::new(&Device, &Queue) -> Arc<WaterGpu>`; `Water::new(gpu, Layout, &SolidField, Numbers,
&[Feel]) -> Water`; `Water::add(&mut self, &[Drop])`; `Water::step(&mut self)`; `Water::count() -> u32`;
`Water::read() -> Vec<Drop>`; `Water::copy(&self) -> Water`; the scenes of decision 21; `Stats`.

- [ ] Write `tests/core.rs` first (they fail to build): `still_water_rests` (`still_box(0.3)` at 8 px, 3 s: rms
      speed under 3 px/s, no drop above the start level + 1 cell); `a_pool_keeps_its_height` (`still_box(0.6)`,
      5 s: 95th-percentile height within 2 % of the start); `two_runs_are_the_same` (`weird()` 1 s twice: equal
      drops, compared as bytes); `a_thin_wall_holds_a_fast_pour` (a 3 px line under a pour at 2000 px/s, 2 s: no
      drop below the line); `a_full_box_stays_shut` (a closed 80 px box filled to the top with a pour aimed in
      for 1 s: every drop finite, none outside the box's outer face); `water_runs_through_a_narrow_opening`
      (`course()` 6 s: over 300 drops through the 12 px opening, none outside the outer box).
- [ ] Port the core with a dense grid behind `cell_at` (the table is the identity for now). Solid field as
      decision 5 and 6.
- [ ] Run `cargo test --release -p bpe-water --test core -- --test-threads=1`. Passing: all six.
- [ ] Commit.

### Task 2: the hold-together

**Files:** create `src/shaders/hold.wgsl`, `tests/streams.rs`, `tests/measure.rs`; modify `drops.wgsl`,
`numbers.rs`, `passes.rs`, `probe.rs`, `scenes.rs`.

**Produces:** `Stream::of`, `Feel`, the hold passes, the chosen numbers.

- [ ] `measure.rs::streams` (ignored): runs the stream scenes on the new core and prints decision 12's measures
      at 1, 2, 4 s, saving debug pictures to `WATER_OUT`. First run with the pull off: the trial's failure, as
      numbers (the baseline).
- [ ] Build decision 10. Measure. Tune decision 11's numbers. Record each run that changed a decision in
      "As built".
- [ ] Re-run Task 1's tests: the pools must not have paid.
- [ ] `tests/streams.rs` (small, CI): `a_thin_pour_stays_one_stream` (`thin_pour` at 8 px, 2 s: 1 piece between
      spout and floor, strays under 2 %), `a_sheet_stays_a_sheet` (`sheet`: 1 piece from the edge to the floor),
      `a_stream_keeps_its_width` (width at mid-height changes by under 25 % over the last second),
      `surface_pull_decides_the_break_up` (`spray_and_rope`: more pieces at pull 0 than at 0.2 than at 1, and
      1 piece at 1), `a_lone_drop_pulls_on_nothing` (one stretched drop in the air falls exactly as a loose one
      does: no self-force).
- [ ] If the measures fall short, decision 13 in order. The gate before Task 3: the cloud's side-by-side of
      `weird()` (through Task 4's harness, built here as far as needed) is judged as clean as today's.
- [ ] Commit.

### Task 3: tiles

**Files:** create `src/sim/tiles.rs`, `src/shaders/tiles.wgsl`, `tests/tiles.rs`; modify every shader's addressing
(`common.wgsl`), `pressure.wgsl` (levels in tiles), `passes.rs`.

- [ ] Tests first: `tiles_change_no_drop` (the same scene on a pool that holds every tile and on the dense
      addressing of Task 1 kept behind a test switch: equal drops), `slots_do_not_change_the_drops` (the free
      stack shuffled: equal drops), `only_wet_tiles_are_stored` (a pour in a 4-screen layout: tiles in use
      under 5 % of the table), `a_full_pool_of_tiles_degrades`, `a_drop_off_the_course_is_kept_in`,
      `a_copy_plays_on_the_same`.
- [ ] Build decisions 14 to 17. Re-run Tasks 1 and 2's tests.
- [ ] Commit.

### Task 4: the bridge and the side-by-side

**Files:** create `src/bridge.rs`, `src/shaders/bridge.wgsl`, `crates/bpe-kit/tests/water_core.rs`; modify
`crates/bpe-kit/Cargo.toml` (dev-dependency).

- [ ] `water_core.rs::side_by_side` (ignored; env `WATER_OUT`, `WATER_SCENES`, `WATER_CELLS`, `WATER_SECONDS`,
      `WATER_FPS`): for each scene, today's core runs the matching Balls scene (built in the test from the same
      numbers; Water Weird from the owner's file when `WATER_WEIRD` names it) and the new core runs
      `scenes::*` through the bridge; both are drawn at 1080 x 1920 by `bpe-gfx`'s renderer with today's Look
      defaults and written as numbered PNGs side by side, with a label strip. A second ignored test prints
      decision 12's measures for both cores.
- [ ] `tests/core.rs::the_bridge_draws_every_drop` (CI): the bridge's buffer holds `count` drops at the same
      places.
- [ ] Commit.

### Task 5: on the owner's card

- [ ] By the Remote Control recipe (project memory `liquid-speed`): build the measuring tests for Windows in
      the PC's `bpe-ci` box, run them natively. No install, the scratch build removed afterwards.
- [ ] `measure.rs::table`: the trial's table on the new core (speed at Normal, Fine, Extreme on `weird`,
      `still_box`, `course`; still pool rms and height at 20 s; the course's openings and walls at 30 s; slosh),
      beside today's core's numbers from the trial's record.
- [ ] `water_core.rs::side_by_side` at full size for every stream scene and `weird`, at 8 px and 6 px (and
      3 px for `weird`); frames joined into clips with ffmpeg on the PC; the measures for both cores.
- [ ] Look at the full-size frames and 1:1 crops (jagged edges, beads, strays, flicker between frames) before
      anything is called reached.

### Task 6: notes, review, merge

- [ ] `docs/notes/water.md`: "Part 2a: the water core (as built)". `CLAUDE.md`: the crate map line for
      `bpe-water` and the area index line already there gains the plan. This file's "As built and measured".
- [ ] `cargo fmt --all`, `cargo clippy -p bpe-water -p bpe-kit --all-targets -- -D warnings`.
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Pull request, checks green, merge. Tell the design thread. Clips to the owner.

## As built and measured

(Filled in as the work is done. Runs are on lavapipe in the cloud container unless they say the owner's card.)

### What made the trial's streams break up (found while porting)

1. **Air braked fast drops.** In a solved cell's outflow sum the trial read a face no drop reaches as speed 0. A
   lone fast drop (or a stream's tip, or a stream with a gap of a cell) then looked like liquid running into still
   liquid, and the pressure braked it: the thin pour fell freely to about 2,600 px/s and then dropped to a
   "terminal" 700 to 1,100 px/s, piling up on its own gaps into slow flat clusters. Fixed in `cs_grid`: a face with
   no speed takes the speed of the face across the cell. After it the pour falls at free fall (within 1 %) down
   the whole screen, and the Water Weird layout shows arcs instead of rain. The still box rests as before
   (rms 0.3 px/s).
2. **Thin liquid left shapes in bunches.** A stream a drop wide leaves a shape (or a place where two streams part)
   in bursts, a cell's worth at a time. Today's core does the same thing more smoothly (its soft pressure), and
   its thin streams are simply loose drops flying one path. The fix here is the **spacing step** (decision 13a,
   as built): in `cs_g2p` a drop in thin, moving liquid moves a little along its own flight, from where the other
   drops round it are many to where they are few (`Numbers::even`, cells squared a second; its own part in the
   smooth density is taken out, so a lone drop never moves). It moves as if a moment earlier or later on its own
   path (place and speed together): moved in place only, each drop of a curving thread went on along a path beside
   its own and the thread frayed. Gated to thin liquid (under about 0.35 to 0.5 of full round it) moving faster
   than 12 to 36 cells a second, so pools are untouched (the still boxes rest at 0.3 to 0.4 px/s with it on).
3. **Crowded drops in thin liquid were thrown apart.** An over-full cell pushes out a tenth of a cell a step
   (`most_out`), right for a body of liquid (with a hundredth, the course's funnel pool visibly squashed), but a
   bunch in a thread or a film was thrown sideways by it. Where the cell and the eight round it are under about
   0.65 to 0.9 full (`near` in `cs_grid`), the push is `most_out_thin` (0.005) instead.

### Tried and dropped

- **The carried stretch with a pull that grows to a tear at about 1 (the first hold-together).** It ropes a
  straight pour (width 12 to 15 px against 20 to 30) but beads every arc: when a stretched thread tears, what is
  left pulls itself into lumps. Today's core lets go at a stretch of about 0.18, so its threads are loose drops:
  the pull is not what keeps them whole. The pull stays in the code as what Surface pull turns up (beads), weak
  at the default.
- **Only solving cells near a body of liquid (thin liquid fully loose).** Loose drops stack on one spot, and the
  first step they count as a body the push throws them apart: puffs all down the stream.
- More sweeps, a bigger or smaller fill allowance: no change to the bunching.
