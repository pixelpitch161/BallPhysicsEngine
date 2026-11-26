# All-drops water core, part C2: deep water and landings (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or
> superpowers:subagent-driven-development) to implement this plan task-by-task. Steps use checkbox (`- [ ]`)
> syntax for tracking.

**Goal:** Pools that keep their height and start at rest, because the grid's pressure holds deep water up from
below; fewer steps and sweeps a frame now that the pools no longer need them; a hard landing that lands the same
way at Fine, Extreme and 2 px; shelves that drain to a film and skin-held puddles; and the grid core retired but
for its pressure solve.

**Architecture:** A "deep water" part joins the all-drops step after the sort and before the sweeps. The sorted
drops' predicted speeds and volume are gathered onto the layout's cells (whole-number sums), each cell works out
what must flow out of it (the grid core's `cs_grid`), the depth of every cell below the nearest cell that is not
full is found (two separable passes), the grid core's one-sided multigrid pressure is solved from the pressure
kept from the step before, and each drop deeper than `D` takes the pressure's change of speed, times its depth
share `w`, on the place it would reach. It is all in the all-drops shader module, so a frame is still one
submission. Frames whose water had no deep cell two frames earlier skip the solve and the push (they only
measure depth once, on their last step), so thin-water scenes pay almost nothing for it and are the same to the
bit with it on or off.

**Tech stack:** Rust 2024, wgpu 30 compute (WGSL), glam, bytemuck. Tests on `bpe_gfx::test_device()`.

**Spec:** `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md`, sections 2 to 7 and 11, the C2
pass marks and the risks. C1's plan and its "As built and measured" (`core-c1-engine.md`) hand C2 the items it
lists at its end. The grid core's solve is in `crates/bpe-water/src/grid/shaders/{grid,pressure,walls}.wgsl` and
`grid/mod.rs::levels`.

## Global constraints

- wgpu's default limits: a storage binding at most 128 MiB and at most 8 storage buffers a pass
  (`passes.rs::every_pass_has_its_entry_and_fits_the_limits` checks every entry).
- Same card, same result. Every sum onto the cells is a whole number (`atomicAdd` on `i32`, the grid core's
  `S_M`/`S_P` scales). The pressure sweeps are red-black (each cell written by one thread, reading the other
  colour). A frame's choice to run the deep part comes from frame `k − 2`'s report, as the step count does.
- **Thin water is never touched.** A drop with `w` exactly 0 has nothing written by the deep part, so a scene
  with no deep water plays the same to the bit with the deep part on or off. The deep part never changes a
  drop's speed except through its place (the speed is still how far the drop went).
- **Only the pressure's change.** A drop takes `w × (corrected − gathered)` at its place, computed straight
  from the pressure as `−(p − p_neighbour)` on each open face. Never the grid's smoothed speeds.
- The processor side uses `bpe_core::maths::Maths` where it computes physics (`clippy.toml`). WGSL may use its own.
- No exe is built or installed. No films except the card run in Task 10.
- Lines at most 120 wide (`cargo fmt --all`), `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Tests that use the card call `bpe_gfx::test_device()`, `cargo test` with `--test-threads=2`. Run only the
  tests this part touches (project rule); CI runs the rest, and the engine tests on the owner's card.
- C3 (the plain drawing) is built at the same time in another thread. It owns `bridge.rs`, `plain.rs`,
  `src/draw/`, the picture-making parts of `tests/shots.rs` (`screen()` and where frames are drawn) and
  `crates/bpe-kit/tests/water_core.rs`. This part changes those only where Task 9 must for the grid core to go,
  and only if C3 has not merged by then.

## Review focus

The conditions the spec implies but no headline test covers, most likely to bite first. Each has its test in the
task that owns the code.

1. **Deep water appearing mid-run** (a pour filling a pool past `D`). The first frame that runs the deep part
   starts its pressure from cold, so it primes (`prime` cycles) on its first step. The pool must not jolt. Test
   `a_pool_filled_by_a_pour_gets_its_push_without_a_jolt`, Task 4.
2. **A copy made while the deep part is running** carries the kept pressure, the deep flags and the clock. Test
   `a_copy_of_a_deep_pool_plays_on_the_same`, Task 2.
3. **Water against an open course edge** (edges not shut): beyond the course is not full, so nothing there is
   deep and no push points out of the course. Test `an_open_edge_is_not_deep`, Task 2.
4. **A tiny layout** whose pressure ladder has one level: the solve must still sweep (the grid core's
   `cycles` loop skipped such grids and left only `iters` plain sweeps, which were 0). Test
   `a_small_course_still_solves`, Task 2.
5. **A deep pool whose drops all leave** (an open floor): the deep count falls to 0 and the deep part stops,
   with no push left behind. Covered by `drops_that_leave_are_counted_out` with a deep pool added, Task 4.

---

## File structure

| Path | What it is |
| --- | --- |
| `crates/bpe-water/src/drops/deep.rs` | New: `Deep` (the cells' buffers, the pressure's levels, the pass numbers), `levels()`, `Deep::encode_*`, the walls at creation |
| `crates/bpe-water/src/drops/shaders/deep.wgsl` | New: `cs_deep_walls`, `cs_deep_gather`, `cs_deep_cells`, `cs_deep_column`, `cs_deep_across`, `cs_deep_sor`, `cs_deep_down`, `cs_deep_up`, `cs_deep_push` |
| `crates/bpe-water/src/drops/shaders/common.wgsl` | `Fr` gains the deep numbers and the levels; bindings 19 to 25 |
| `crates/bpe-water/src/drops/shaders/sort.wgsl` | `cs_sum_a` becomes a parallel scan of each block |
| `crates/bpe-water/src/drops/shaders/report.wgsl` | `Report.deep`: cells deep at the frame's last step |
| `crates/bpe-water/src/drops/{mod.rs,passes.rs,numbers.rs,clock.rs}` | The deep part in the frame, its numbers, its passes, its report |
| `crates/bpe-water/src/drop.rs` | `fill_stacked` snug against side faces |
| `crates/bpe-water/src/probe.rs` | `pieces` (drops nearer than 1.6 spacings are one piece) |
| `crates/bpe-water/src/scenes.rs` | `deep_narrow` (the deep pool, narrower, for 2 px), `fill_pool` (a pour filling a box) |
| `crates/bpe-water/tests/engine.rs` | The deep part's behaviour tests |
| `crates/bpe-water/tests/shots.rs` | New measuring runs: `landing`, `shelves`; `pools` gains the layer's spacing and the last 5 s |
| `crates/bpe-water/src/grid/`, `tests/{core,streams,measure}.rs` | Deleted in Task 9 (the solve lives in `deep.wgsl`) |
| `card/all-drops-core-c2/run.ps1` | The card run (from C1's, on branch `claude/project-thread-zl1ff7`) |

---

### Task 1: A parallel scan of the bins (speed, the same water)

`cs_sum_a` gives each block of 512 bins one thread, which walks the block in order: 512 dependent loads and
stores, about the same on every scene. A step on the card costs about 0.3 ms whatever its drops (C1's pace
table), and this walk is a likely part of it.

**Files:** `src/drops/shaders/sort.wgsl`, `src/drops/mod.rs` (dispatch `cs_sum_a` one workgroup a block).

- [ ] **Step 1:** Rewrite `cs_sum_a` as one workgroup of 256 threads a block: each thread loads two bins' counts,
  a workgroup scan (Hillis and Steele over 256, as `cs_sum_b`) gives the exclusive starts, the block's total goes
  to `blocks[b]`. Whole numbers, so the starts are the same as before to the bit.
- [ ] **Step 2:** Run `cargo test -p bpe-water --test engine -- --test-threads=2`. Expected: all pass, including
  `two_runs_and_a_copy_are_the_same` and `joining_a_steps_end_to_the_next_fall_changes_nothing`. Then `repeats`
  (weird and pool at 12 and 6 px) on the software card before and after: the same prints (written in "As built").
- [ ] **Step 3:** `pace` on the software card, weird at 12 px, before and after. Commit "bpe-water: the bins'
  running sums in parallel".

### Task 2: The deep part: gather, cells, depth, pressure, push

**Files:**
- Create: `src/drops/deep.rs`, `src/drops/shaders/deep.wgsl`
- Modify: `src/drops/passes.rs` (`SOURCE` gains deep.wgsl; `P` and `ENTRIES` gain the nine passes),
  `src/drops/shaders/common.wgsl`, `src/drops/mod.rs`, `src/drops/numbers.rs`, `tests/engine.rs`

**Numbers** (`Numbers`, each with its `BPE_WATER_<NAME>`):

| Field | Default | Meaning |
| --- | --- | --- |
| `deep` | 1 | 1: the deep part runs where there is deep water; 0: never |
| `deep_d`, `deep_b` | 24.0, 16.0 | Pixels: no push within `deep_d` of a cell that is not full, all of it deeper than `deep_d + deep_b`; each at least 2 cells |
| `deep_full` | 0.9 | A cell is full at this fill (the drops' volume and the solids' share) |
| `deep_most` | 0.25 | The most cells the push moves a drop in a step |
| `cycles`, `pre`, `post`, `coarse` | 1, 2, 2, 8 | V-cycles a step; sweeps a level down, up, and on the coarsest |
| `prime` | 12 | V-cycles on the first step the deep part runs after it did not (the start, or deep water appearing) |
| `omega`, `scale` | 1.0, 1.0 | The sweeps' over-relaxation; how much of a coarser level's answer is added |
| `push_out`, `most_out` | 0.25, 0.1 | The share of a cell's excess fill pushed out a step, and the most (cells of volume) |
| `fill_rate` | 0.5 | The share of a cell's room that may fill a step before it pushes back |
| `face_floor` | 0.005 | The least mass on a face for it to have a speed |

**The frame's numbers** (`FrameGpu` and `Fr`, appended): `deep: [f32; 4]` = (`deep_d`, `deep_b` in cells, each
at least 2, `deep_full`, `deep_most`); `mg: [f32; 4]` = (omega, scale, push_out, most_out); `mg2: [f32; 4]` =
(fill_rate, face_floor, the liquid's edge off a face = `wall_off − 0.25`, the edge of the course's own liquid =
`EDGE + 0.05 − 0.25`); `dgrid: [u32; 4]` = (levels, window `R` = ceil(D + B) + 1 cells, 0, 0);
`lv: [[u32; 4]; 12]` (the grid core's `levels(cols, rows)`).

**Bindings** (`common.wgsl`): 19 `sums: array<atomic<i32>>` (6 words a cell: mass and momentum on its left
face, on its top face, volume at its middle, spare), 20 `wallc: array<vec4<f32>>` (open share of its left face,
of its top face, the solids' share of its middle, 0), 21 `cellp` (every level: pressure × dt, what must flow
out, the open shares' sum, solved or the level's floor), 22 `coef` (every level: the open shares left, right,
top, bottom), 23 `dfa: array<f32>` (a cell's fill, then its `w`), 24 `dfb: array<f32>` (its distance down its
column to a cell that is not full), 25 `deep_count: array<atomic<u32>>` (one word).

**The pass numbers:** the step uniform's buffer gains 24 slots after the export's (`MOST_STEPS + 1 + k`), each
holding `(k, 0, 0, 0)`, written once at creation. A pressure pass is bound at slot `k = 2 × level + colour` and
reads `st.n.x`. No new uniform binding.

**The passes**, after `cs_order` and before the first sweep, on the sorted drops (`y_in` is `by.y[0]`):

1. `cs_deep_gather` (a drop each; bindings 11, 14, 19): a live sorted drop adds its mass (its liquid's weight ×
   0.25) and momentum to the 2 × 2 left faces and 2 × 2 top faces round its predicted place, and 0.25 of volume
   to the 2 × 2 cell middles, bilinearly, as whole numbers (the grid core's `cs_p2g` without its affine part and
   its hold). Its speed is `(y_in.xy − velb.xy) / dt`: the place it would reach less where it was.
2. `cs_deep_cells` (a cell each; 19, 20, 21, 22, 23): the grid core's `cs_grid` without the thin-liquid branch
   (`most_out` everywhere) and without writing speeds: faces' speeds from the sums, open shares from `wallc` and
   the course's shut edges, `fill = volume + 1 − (1 − solids)(1 − edges)`, solved when volume > 1e-4 and the open
   shares' sum > 1e-3, `want = s − out`, the kept pressure as the start. Writes `cellp`, `coef` and `dfa = fill`
   (the last column and row of nodes, which are no cells: 1 when the edges are shut, else 0).
3. `cs_deep_column` (a cell each; 23, 24): `dfb` = the distance in cells from this cell down or up its column to
   the nearest cell whose `dfa` is under `deep_full`, looked for outward up to `R` (outside the grid counts as full
   when the edges are shut, else not), `R + 1` when none.
4. `cs_deep_across` (a cell each; 19, 23, 24, 25): `d² = min over |dx| ≤ R of dx² + dfb(x + dx)²`, looked for
   outward and stopped once `dx²` passes the best; `dfa = w = smoothstep(D, D + B, sqrt(d²))`. Clears the cell's
   six sums for the next step. On the frame's last step (`st.n.w == 1`) a cell with `w > 0` adds 1 to
   `deep_count`.
5. The pressure, as the grid core's `step()` encodes it, at `cycles` V-cycles (`prime` on a priming step):
   `cs_deep_sor` (2 colours × `pre` on the way down, `coarse` on the coarsest, `post` on the way up),
   `cs_deep_down`, `cs_deep_up`, the grid core's own passes with `st.lv`, `st.a.x` and `st.b.z` read from
   `fr.lv`, `fr.mg.x`, `fr.mg.y` and `tick.n.x` from `st.n.x`. **When the ladder has one level** the V-cycle is
   `coarse` sweeps of level 0.
6. `cs_deep_push` (a drop each; 11, 14, 15, 21, 22, 23): `w` bilinear from the cells' `dfa` round the drop. If
   `w ≤ 0` (or not a number) it returns **without writing**. Otherwise `du` is the mean, over the open left faces
   of the 2 × 2 round it weighted bilinearly, of `−(p[k] − p[k − 1])`, and `dv` the same over the top faces with
   the cell above; `shift = w × (du, dv) × dt`, at most `deep_most` cells; the place is kept off the solids
   (`keep_off`) and written back to `y_in`. Its `n` is kept.

**The walls:** `cs_deep_walls` (a cell each; 15, 20) is the grid core's `cs_walls` with the liquid's edge at
`wall_off − 0.25` off a face (drops rest `wall_off` off a face and fill the half spacing round them, so the
liquid's edge is the face itself at the default 0.25). It runs once, in `Water::new`'s own submission. The edges
of the course's liquid are at `EDGE + 0.05 − 0.25` (drops stop at `EDGE + 0.05`).

**In `Water`:** `deep: Deep` with the buffers above (cells = `layout.nodes()`; `cellp`/`coef` for every
level), made at creation; `copy()` copies `cellp` and the deep flags (below). The sums, `dfa`, `dfb` and
`deep_count` need no copy (cleared or rewritten before they are read). `deep_d`, `deep_b`, `R` and the levels
are fixed when the body is made (the layout fixes them).

**The frame flow** (Task 4 adds the skip; in this task the deep part runs on every step when `deep` is 1): for
each step `j`, after `P::Order`: gather, cells, column, across, the V-cycles, push. A priming step is the first
step of the first frame that runs the deep part.

- [ ] **Step 1: Write the failing tests** in `tests/engine.rs`:
  - `a_deep_pool_keeps_its_height`: `still_box(12.0, 0.6)` for 2 s. Height kept (as `pools` measures it: from
    the floor's face to the top drop) at least 99 % with the deep part on; with `deep` 0 below 99 % (so the test
    shows the part doing the work). No drop outside the box.
  - `the_deep_part_leaves_thin_water_alone`: `weird(12.0)` and `thin_pour(12.0, 2.0)` for 0.5 s each: the same
    `probe::print_of` with `deep` 1 and 0.
  - `a_copy_of_a_deep_pool_plays_on_the_same`: `still_box(12.0, 0.6)` plus a pour into it, copied at frame 31
    (deep part running), both played to 1 s: the same print as each other and as a run without a copy.
  - `an_open_edge_is_not_deep`: a pool resting on the course's bottom edge with the edges open (no floor): the
    drops fall out of the course and no drop moves up (no push points out of the course); `count()` is 0 after
    1 s.
  - `a_small_course_still_solves`: a 60 × 60 px course at 12 px cells (one level) full of water held by its
    walls keeps 99 % of its height over 1 s.
- [ ] **Step 2:** Run `cargo test -p bpe-water --test engine -- --test-threads=2 deep small open`. Expected: the
  new tests fail (no `deep` number yet: they don't compile).
- [ ] **Step 3:** Implement the numbers, `deep.rs`, `deep.wgsl`, the bindings and the frame flow. Unit tests in
  `deep.rs`: `levels` as the grid core's (moved with it), and `the_window_covers_the_blend` (R for 1.5, 2, 3, 6,
  8 and 12 px cells is at least `(D + B) / cell + 1`).
- [ ] **Step 4:** Run the same command. Expected: PASS. Then every `engine.rs` test.
- [ ] **Step 5:** Commit "bpe-water: deep water held up by the grid's pressure".

### Task 3: A pool starts at rest

**Files:** `src/drop.rs` (`fill_stacked`), `src/drops/mod.rs` (priming after `add`), `tests/engine.rs`

- **Priming:** `Water::add()` sets `prime_next`. The first step of the next frame that runs the deep part
  runs `prime` V-cycles instead of `cycles` (the drops still, their predicted speeds only a step of gravity, so
  the kept pressure is the pool's weight before anything moves). Copies carry the flag.
- **Snug fills:** C1 found a drop by a side wall thrown up it, faster the wider the gap between the wall's face
  and the fill's nearest column (0.67 cell: 605 px/s). `fill_stacked` lays its columns on the global lattice; now
  it finds the clear span of the box along its middle row (marching from each side of the box inwards to the
  first point `rest_off` clear of the solids, then settling on it as `settle_on` does). An end that was in a
  solid is snug: the column there sits exactly `rest_off` off the face. With both ends snug the columns are
  spread evenly between them, `n = floor(span / 0.5) + 1` columns (spacing within `0.5 / (n − 1)` cell of half a
  cell); with one end snug they are laid half a cell apart from it; with none, as today. Every column then settles
  onto its floor as today.

- [ ] **Step 1: Write the failing tests:**
  - `a_fill_starts_at_rest` (C1's, unreachable then): `still_box(12.0, 0.3)` filled by `fill_stacked`. The lowest
    row is 0.25 cell off the floor within 1e-3, and over the first 0.25 s the rms speed is below 20 px/s.
  - `no_drop_climbs_a_side_wall`: `still_box(6.0, 0.6)` with the box shifted a third of a cell (`origin` moved so
    the wall's face falls off the lattice). Over the first 0.5 s no drop within 2 cells of a side wall moves up
    faster than 60 px/s.
  - In `drop.rs`: `a_fill_sits_snug_against_its_walls` (a box between two walls whose faces are 0.3 and 0.6 cells
    off the lattice: the outer columns are `rest_off` off each face within 1e-3, and the columns are evenly spaced
    within 1e-4); `a_fill_with_a_free_side_keeps_the_lattice` (one side in the open: the columns are half a cell
    apart from the snug side).
- [ ] **Step 2:** Run `cargo test -p bpe-water -- --test-threads=2 fill start climbs`. Expected: they fail.
- [ ] **Step 3:** Implement. Expected: PASS; `a_fill_is_stacked_on_the_floor` and the `drop.rs` fill tests still
  pass.
- [ ] **Step 4:** Commit "bpe-water: a pool starts at rest".

### Task 4: The deep part runs only where there is deep water

**Files:** `src/drops/clock.rs` (`Report.deep`), `src/drops/shaders/report.wgsl`, `src/drops/mod.rs`,
`tests/engine.rs`

- `Report` gains `deep: u32` (its `spare[0]`): `cs_report` copies `deep_count`. The frame's encoder clears
  `deep_count` at its start.
- Frame `k` reads `deep(k − 2)` (as the clock reads the fastest speed). For `k < 2`, and on the frame after
  `add()`, it counts as deep when the body has drops (so a fill at the start primes at once).
- **Deep frame:** the whole deep part on every step; its first step primes if the frame before was not a deep
  frame (or `prime_next`). **Other frames:** only gather, cells, column and across, on the last step (to count),
  and no pressure or push. A frame with no drops runs none of it.
- The choice depends only on reports and adds, so runs, copies and runs of the program still repeat.

- [ ] **Step 1: Write the failing tests:**
  - `a_pool_filled_by_a_pour_gets_its_push_without_a_jolt`: `fill_pool(12.0)` (a 70 px pour into an empty
    300 × 400 px box for 2 s). Its deep part starts part way (print the frame), and over the 10 frames either side
    the rms speed of drops more than `D + B` below the surface never jumps by more than 30 px/s from one frame to
    the next. After 3 s it holds 99 % of the height it reaches at 2.2 s.
  - `deep_frames_follow_the_water`: `thin_pour(12.0, 2.0)`: no frame runs the deep part (`Water::deep_frames()`
    is 0) through 0.5 s. `still_box(12.0, 0.6)`: every frame does.
  - `drops_that_leave_are_counted_out` gains a deep pool over an open floor: `deep_frames` stops growing once
    it has drained.
- [ ] **Step 2:** Run them. Expected: fail. **Step 3:** Implement. Expected: PASS, and Task 2's tests still pass.
- [ ] **Step 4:** Commit "bpe-water: the deep part only where the water is deep".

### Task 5: How a hard landing breaks up, measured, then fixed

**Files:** `src/probe.rs` (`pieces`), `tests/shots.rs` (`landing`), then whatever the measuring points to.

- `probe::pieces(drops, layout, near_spacings, small_px, region) -> Pieces { pieces, drops, small_share,
  biggest }`: drops nearer than `near_spacings` (1.6) spacings are one piece (union-find over bins as wide as that
  distance); a piece is small when its bounding box is under `small_px` across both ways (two draft cells: 16 px);
  `small_share` is the share of the region's drops in small pieces. `region` is a world rectangle.
- `landing` (`#[ignore]`, by hand): `weird` at Surface pull `WATER_PULL` (0.2 and 0.5) at `WATER_CELLS` (8, 6, 3,
  2). Prints, every 0.25 s from 0.25 to 3 s, `pieces` over the region above the floor's pool (world y < 600) and
  over the block's surroundings (a 400 × 300 px box round the block); with `WATER_DOTS`, a dot picture of the
  block's surroundings every frame from 0.2 to 0.8 s (`probe::picture` cropped, numbered).

- [ ] **Step 1:** Implement `pieces` with unit tests (`two_rows_far_apart_are_two_pieces`, `a_line_at_the_spacing_is_one_piece`,
  `small_share_counts_lone_drops`) and `landing`.
- [ ] **Step 2: Measure** on the software card (cloud), Surface pull 0.2 and 0.5, at 6 and 3 px: the pieces table,
  and the dots of the landing frame by frame side by side. Then test the spec's three causes, one knob each, at 6
  px (each a `BPE_WATER_*` number, defaults keeping today's water):
  1. **The sweeps' push turns into speed:** `part_even` (0 today): squeezed liquid (`rho/rho0 > 1`) evens its speeds
     toward its neighbours' mean by this share times its squeeze. If the landing at 6 px matches 3 px with it,
     cause 1.
  2. **A one-drop sheet has only the skin:** measure the sheet's thickness in drops where it tears at 6 and 3 px
     (from the dots). If it tears at the same thickness in drops (so at twice the pixels at 6 px), cause 2; the fix
     is `thin_pull`: the skin's pull between drops in thin liquid scaled by `(cell / 3 px)` where the liquid round a
     drop is under `deep_full`, so a sheet tears at the same thickness in pixels.
  3. **The step is too big:** 6 px with `floor` 32 (the 3 px step in spacings is the same, so this is expected not
     to help; it rules the cause in or out).
  Also checked, as it is the other thing that is not the same in spacings at every detail: the evening of speeds,
  which damps drop-sized noise four times faster at 3 px than at 6 px (`visc_base` × 4 at 6 px).
- [ ] **Step 3:** Write what was found in this plan ("As built and measured"), with the numbers. Implement the fix
  the measuring points to as a default, with its knob. Add `a_hard_landing_stays_whole`: `weird(6.0)` at Surface
  pull 0.2 for 1.5 s: `small_share` over the block's surroundings under a bound set from the measuring with room
  for the card's rounding (C1's water fails it).
- [ ] **Step 4:** The C1 shots at 6 and 3 px in the cloud (slope, ledge, beads, weird, 2 s each) by eye against
  C1's stills: no worse. Commit "bpe-water: a hard landing lands the same at every detail".

### Task 6: Fewer steps and sweeps

**Files:** `src/drops/numbers.rs` (`for_cell`, `iters`), `tests/engine.rs`, `drops/numbers.rs` tests.

- [ ] **Step 1:** On the software card at 12 and 6 px, and at 3 px for the pools, try floors (steps a frame) of
  16, 12 and 8 at 6 and 8 px and 32, 24 and 16 at 3 px and finer, and `iters` of 4, 3 and 2. For each: `pools`
  (height kept, rms after the rest, the surface layer's spacing), `landing` (pieces share), the course shares
  (`openings`, 10 s) and the weird, slope and ledge stills at 2 s.
- [ ] **Step 2:** Pick the lowest floor and fewest sweeps that keep: pools ≥ 99 % and ≤ 1 px/s; the landing's
  small share within 5 points of 3 px; thin water no worse than C1's by eye; course shares within 3 points of
  C1's. Write the table in "As built and measured". Set `for_cell` and `iters`; update `floors_follow_the_detail`
  and `the_skin_gets_its_steps` (a floor under the skin's count is lifted by the skin, not the floor).
- [ ] **Step 3:** Run every `engine.rs` test. Commit "bpe-water: fewer steps and sweeps where the pools allow".

### Task 7: Shelves drain to a film and puddles

**Files:** `tests/shots.rs` (`shelves`), then whatever the measuring points to (`finish.wgsl` grip and drag).

- `shelves` (`#[ignore]`): the course at `WATER_CELLS` for 30 s at Stickiness 0 and 0.5: the share of drops left
  on the top shelf (world y < −640), and the shelf's water height every 10 px along it (the topmost drop over
  the floor's face, counting only a column with at least 3 drops), printed as the tallest and the mean.
- [ ] **Step 1:** Measure at 6 px in the cloud. Pass: the tallest is at most `2 × 10 px × Surface pull` (10 px at
  0.5) plus one spacing, and the share is reported beside the trial's 7.3 % and C1's 7.4 %.
- [ ] **Step 2:** If it does not pass, find what holds the water (drag within `drag_reach` of the floor at
  Stickiness 0.5 is 17 a second; the pressure in a layer a few drops deep is only what its squeeze gives; the
  skin at the layer's front), one knob at a time, and fix what it is. Note the numbers in "As built".
- [ ] **Step 3:** Commit "bpe-water: shelves drain" (or note that nothing needed changing).

### Task 8: A skin calm at any step count (time-boxed: two hours)

C1 capped a frame at 32 steps because the skin's bend term throws a pour's drops out of the spout from about 40
steps a frame, and a pour in zero gravity bursts even at 32. **Only if** a bend from smoothed directions (the
liquid's direction averaged over a reach before the bend is taken), or the bend applied at most 1920 times a second
while the pressure steps faster, is calm at 64 and 128 steps a frame on `a_pour_stays_calm_at_the_most_steps_a_frame`
and on a zero-gravity pour, and keeps the beads on the plates and the 6 px stream's break into drops: raise
`most_steps` to 128 and keep the test at the new ceiling. Otherwise leave 32 and write what was tried.

### Task 9: The grid core retired but for its solve

**Files:** delete `src/grid/` (its solve is now `deep.wgsl` and `deep.rs`), `tests/core.rs`, `tests/streams.rs`,
`tests/measure.rs`; modify `src/lib.rs`, `src/drops/numbers.rs` (no `From<&Feel> for grid::Feel`),
`src/scenes.rs` (no `start_grid`, `GridRunning`), `src/drop.rs` (no `Pour::drops`, `Dice`, `fill` if nothing else
uses them), `src/probe.rs` (doc lines), and, only if C3 has not merged: `src/bridge.rs` (no `FeedBuffers::Grid`)
and `crates/bpe-kit/tests/water_core.rs` (no `grid` engine).

- [ ] **Step 1:** Fetch main. If C3 has merged, merge main first. Delete and fix up. `cargo test -p bpe-water
  --no-run`, `cargo clippy -p bpe-water -p bpe-kit --all-targets -- -D warnings`.
- [ ] **Step 2:** Run `cargo test -p bpe-water -- --test-threads=2` and `cargo test -p bpe-kit --test water_core
  -- --test-threads=2`. Commit "bpe-water: the grid core retired; its pressure solve lives on in the deep part".

### Task 10: The card run, the notes, the review and the merge

**Files:** `card/all-drops-core-c2/run.ps1` (removed before the merge), this plan ("As built and measured"),
`docs/notes/water.md` ("Part C2"), `CLAUDE.md` (the `bpe-water` line: the grid core goes), the crate's
`Cargo.toml` description.

- [ ] **Step 1:** Push, open the draft pull request, let CI run (its card job runs `engine.rs` on the owner's card).
- [ ] **Step 2:** The card run, through Remote Control on `F:\!Ai Stuff\BallPhysicsEngine` (recipe: project memory
  `liquid-speed`, `all-drops-trial`, and C1's run): films (the six shots at 6 and 3 px, slope, weird and pool at
  2 px), style (Surface pull 0.2 and 1), `pace` (8, 6, 3 and 2 px, and a one-screen course two thirds full at 2 px
  for Make final's estimate), `pools` (6 and 3 px for 20 s, the 2 px pool for 20 s), `openings` and `shelves`
  (Stickiness 0 and 0.5), `repeats`, `shifts`, `landing` (8, 6, 3 and 2 px at Surface pull 0.2 and 0.5), and the
  owner's Water Weird file through `water_core.rs` at Fine and Extreme. Results to its own branch as `[skip ci]`;
  films to project files `water-core/c2/`; end the session when it reports.
- [ ] **Step 3:** Judge against the C2 pass marks and C1's (below), side by side with the trial's and C1's films.
  Write the verdict and numbers here.
- [ ] **Step 4:** Notes, CLAUDE.md, Cargo description; fmt and clippy.
- [ ] **Step 5:** One Opus review of the whole branch (superpowers:requesting-code-review); fix what it finds;
  merge when every check is green. Tell Aedan with the films.

## Pass marks and how each is checked

| Mark (spec) | Where it is checked |
| --- | --- |
| A deep pool at rest keeps ≥ 99 % of its height at Fine, Extreme and 2 px; after 20 s at most 1 px/s rms, not growing over the last 5 s; with the default sweeps | `pools` on the card (Task 10), `a_deep_pool_keeps_its_height` in CI |
| Water Weird at Surface pull 0.2 and 0.5, Fine, Extreme and 2 px: leaves the block as one body, no rain; small share within 5 points between details (Draft Normal a *target*) | `landing` on the card, the owner's file through `water_core.rs`, films by eye; `a_hard_landing_stays_whole` in CI |
| No deep water: the deep part on or off gives the same drops to the bit | `the_deep_part_leaves_thin_water_alone` |
| Shelves at Stickiness 0 and 0.5 after 30 s: a film and puddles ≤ 2 × skin length; never a standing pool; the share reported | `shelves` on the card |
| C1's marks still hold | Every `engine.rs` test; the films side by side; course shares within 3 points |
| *Goal line*: a slosh keeps ≥ 75 % (Fine) and 85 % (Extreme) of its swing after four half swings | `pools` |
| *Targets*: no slower than the grid core at Fine and Draft Normal (0.15 to 0.18); within 1.5 × at Extreme (0.6; 0.8 for the 166,000-drop pool); at most 1.5 at 2 px (2.0 for the 373,000-drop pool); a one-screen course two thirds full at 2 px measured | `pace` on the card |

## Things not to do

- Don't give a drop the grid's smoothed speeds, and don't start the drops' multipliers from the grid's pressure
  (two solvers chasing two resting states fight; spec section 6).
- Don't write anything to a drop whose `w` is 0, and never push water within `D` of a cell that is not full.
- Don't keep the drops' multipliers from step to step (the trial's `warm`: a pool boils).
- Don't run the pressure on frames with no deep water, and don't read the report of frame `k − 1` or `k` to
  decide (only `k − 2`).
- Don't lower a floor or the sweeps further than the pool marks, the landing mark and C1's thin-water marks allow.
- Don't touch C3's files (`bridge.rs`, `plain.rs`, `draw/`, the drawing in `shots.rs`, `water_core.rs`) beyond
  Task 9's compile fixes, and only if C3 hasn't merged.
- Don't touch today's Liquid thing, `bpe-gfx/src/liquid_sim/`, or anything in a Balls scene.
- Don't build or install the exe.

## As built and measured

### Task 1: the bins' running sums in parallel

`cs_sum_a` is a workgroup of 256 summing a block of 512 bins (two each, then a scan in shared memory) where one
thread walked the block. The weird and pool shots' prints at 12 and 6 px are the same as before it
(`88314bab027c171b`, `450f80101cf360f1`, `ff742efa2be059ff`, `4190424dabccde80`).

### Tasks 2 to 4: the deep part

Built as planned, with these changes, each found by measuring the still box (`still_box(12.0, 0.6)`, `pools`) on
the software card:

1. **The deep part runs before the sweeps,** as the plan says. (Run after them first, it read `velb` as where a
   drop was when the last sweep had already put its new speed there: the pool boiled at 550 px/s.)
2. **Depth is measured from the liquid's own edge,** not from any cell that is not full: a cell the drops reach
   that is not full, or an open cell they do not reach beside an open one they do. A cell no drop reaches past a
   solid is not an edge. (Measured from every cell that was not full, the air beyond the box's 18 px wall made the
   water beside the wall shallow: that column took no push while the water beside it did, slid down the wall at
   700 px/s and turned the pool over.)
3. **How full a cell is, for the excess and for "full", is the drops' own fullness** (each drop's density by its
   neighbours and the solids at the step before's last sweep, carried in `velb.w` through the sort and gathered as
   a sixth sum), not the cell's share of liquid. Beside a face the share reads 0.67 to 0.9 where the drops rest
   off the face, so the cell asked for liquid it could never get and drew the water beside it into it for ever. A
   cell is full when its drops are at least `deep_full` full and they and the solids cover at least half of it,
   or it is at least half solid.
4. **The pressure is worked out only where it pushes** (`w > 0`); above that the drops hold themselves up and
   press on the deep water, which pushes back where it is over-full. (Worked out over all the liquid, it left a
   pressure in the top layer that nothing took, and the layer rippled more and more: 100 to 350 px/s at 3 s.)
5. **`push_out` is 0.05** (0.25 in the grid core): the share of a cell's excess pushed out a step. At 0.25 the
   pool shimmered at about 19 px/s; at 0 it sank slowly (98.2 % at 3 s); at 0.05 it holds 98.7 to 98.9 % with
   about 10 px/s, all of it in the top layer by the walls. `fill_rate` stays 0.5: at 0 the pool blew up.
6. **The deep frames:** frame `k` runs the deep part when frame `k − 2` counted deep cells, or on the two frames
   after `add()` (their reports come from before it); the first of those primes. No frame runs it just for being
   one of the first two, so a pour with nothing deep never runs it (`deep_frames_follow_the_water`).
7. **`a_small_course_still_solves`:** a ladder of one level cannot happen (a layout always has at least 7 nodes a
   side, so at least two levels); the test is a narrow column between walls, held as well as without the part.
8. `probe::pieces` has one unit test, `pieces_tell_a_sheet_from_spray`, for the three the plan names.

Still box at 12 px, software card, 8 s: height kept 98.7 % (C1 96.1 %), rms 10 px/s (C1 2.2 px/s after its start
bounce), the slosh keeps 93 % after four half swings (C1 83 %).

The tests' bounds, set from this: `a_deep_pool_keeps_its_height` asks 98.5 % at 12 px (99 % is the pass mark at
Fine and finer, checked by `pools` on the card); `no_drop_climbs_a_side_wall` asks no drop above 200 px/s (C1
605; one frame at 177 now). `a_pool_filled_by_a_pour_gets_its_push_without_a_jolt` compares the pool with and
without the deep part ten frames after its first deep frame (speeds within 30 px/s): a pool already sagged when
the part starts holds less squeeze than the part expects, and the drops' own squeeze lets go upward, so the
height alone is no test of a jolt.

**Snug fills (Task 3):** `fill_stacked` finds each side's end along the rectangle's middle row; a side ending in a
solid has its column exactly `rest_off` off the face, and with both snug the columns are spread evenly between
them.

### Task 5: a hard landing

**Measured** (`landing`, software card, Water Weird's shot `weird`, the share of the water in pieces under 16 px
across, the mean of the samples from 1 to 3 s):

| Surface pull | Detail | Round the block | Above the floor's pool |
| --- | --- | --- | --- |
| 0.2 | 6 px | 5.9 % | 39.9 % |
| 0.2 | 3 px | 4.6 % | 30.0 % |
| 0.5 | 6 px | 10.1 % | 32.2 % |
| 0.5 | 3 px | 16.4 % | 25.9 % |

The block's own landing was already alike (no rain over the block at either detail; at 0.5 the 3 px numbers are
higher only because its upslope jet ends in a line of round beads). What differed was the water's look and what
followed: at 6 px the sheet running down the block is ragged, with loose clumps riding a drop or two above it,
and where the sheet meets the disc it bursts into a fountain of spray; at 3 px the sheet is smooth and wraps the
disc as an arc that breaks into a line of beads.

**The causes, one knob each, at 6 px and Surface pull 0.2** (above the pool at 1, 1.5 and 2 s; 3 px: 34, 28, 31):

| Knob | Above the pool | By eye |
| --- | --- | --- |
| none (C1) | 40, 34, 47 | ragged sheet, spray fountain at the disc |
| 1. squeezed liquid evens its speeds (`part_even` 4, 20) | 34, 38, 38 / 34, 27, 33 | a little less ragged |
| 2. thin liquid's skin times the detail (`thin_pull` 1) | 41, 44, 46 | worse: the stream and sheet pull into beads and clumps |
| 3. the 3 px steps (`floor` 32) | 44, 45, 38 | no better (as expected: the steps are the same in spacings) |
| the evening × 4 (`visc_base` 400) | 34, 33, 36 | the sheet smooth, as at 3 px; but the falling stream turns into a ladder of separate rows (the evening holds each row together as it stretches) |
| the evening × 2 (`visc_base` 200) | 34, 32, 35 | half way |

So none of the spec's three is the cause. The cause is that drops that meet bounce apart: at an impact and in the
churned sheet, the sweeps push drops apart and that push becomes speed, and at 6 px nothing takes it back before
it throws a drop clear (at 3 px the evening, four times faster there in pixels, mostly does). A stronger evening
everywhere cures it but stiffens the stream, since the evening also resists drops parting and sliding past each
other.

**The fix: `meet_even`** (`finish.wgsl`): each step a share of the speed at which a drop and its neighbours close
on each other, along the line between them and only while they close, is taken away (the mean of its
neighbours' by the kernel). Pairs give and take alike, so momentum is kept. Drops that part or slide past each
other are left alone, so the stream stretches and the slosh swings as before. A step crosses the same share of a
spacing at every detail, so the same share a step is the same water at every detail.

| `meet_even` | 6 px above the pool (1, 1.5, 2 s) | 3 px | Slosh at 6 px (left after four half swings) |
| --- | --- | --- | --- |
| 0 | 40, 34, 47 | 34, 28, 31 | 90 % |
| 0.1 | 29, 23, 30 | | |
| 0.3 | 25, 24, 22 | 27, 25, 29 | 88 % |

0.3 is the default: at 6 px the sheet is smooth with small lumps, wraps the disc and breaks into beads as at 3 px;
at 3 px it changes little (fewer stray droplets at the floor's splash). The deep pool at 6 px keeps 100 % of its
height either way. `a_hard_landing_stays_whole` asks under 30 % above the pool at 6 px and Surface pull 0.2 (the
mean of 1, 1.25 and 1.5 s: C1 38 %, now 23 %).

The C1 shots at 2 s (slope, ledge, beads, weird; 6 and 3 px; `meet_even` 0.3 against 0) by eye: no worse
anywhere, and at 6 px clearly less spray (the slope's and the weird's floors, the weird's disc), with the ledge's
run-off a smooth sheet down to the wall where it was ragged. At 3 px little changes.

### Task 8: a skin calm at any step count (time-boxed)

Measured on the RX 9070 XT with a probe like `a_pour_stays_calm_at_the_most_steps_a_frame` (the thin pour at 2 px,
Surface pull 1, 30 frames, with gravity and with none): at 64 steps a frame the pour stays a stream with gravity
(no drop above the spout, the fastest sideways 0.58 of the pour's speed) and nearly so with none (1.8 px above,
0.92 sideways at frame 30, over the test's 0.75); at 128 it bursts (drops 42 px above the spout, 3 to 7 times the
pour's speed sideways), with gravity or without. The bend worked out on only 32 steps of the frame, each as strong
as the steps since the last (a `bend_steps` knob), was worse: it burst at 64 too. So the cause is not how often the
bend is taken, and `most_steps` stays 32; the knob was taken out again. The smoothed-directions bend was not tried
in the time box.

### Task 9: the grid core retired

`src/grid/`, `tests/core.rs`, `tests/streams.rs` and `tests/measure.rs` are gone, with `start_grid`,
`GridRunning`, the `Feel` conversion, and the random births only the grid used (`Pour::drops`, `Dice`);
`a_pour_fans_out_smoothly` checks the rows instead. `fill` stays (the engine tests and the fill tests use it).
C3 had merged, so `bridge.rs` and `water_core.rs` needed nothing.
