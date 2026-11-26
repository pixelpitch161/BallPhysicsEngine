# All-drops water core, part C1: the engine proper (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Water mode's real liquid engine: all drops (position-based), rebuilt from the trial's lessons as
`bpe_water::Water`, with a clock that picks its own steps, a frame sent to the card as one submission, a sort
that repeats with no cap, and pours born as even rows.

**Architecture:** The trial's step (predict, sort, sweeps, finish) moves into `crates/bpe-water/src/drops/` with
its drops stored by field. A body moves on a frame (1/60 s) at a time, and each frame's `n` steps are chosen from
the fastest speed the card reported two frames earlier, so the host never waits. The grid core moves aside to
`bpe_water::grid` unchanged, kept for its tests and for part C2's pressure solve.

**Tech stack:** Rust 2024, wgpu 30 compute (WGSL), glam, bytemuck. Tests on `bpe_gfx::test_device()`.

**Spec:** `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md` (sections 1 to 5, 8, 10 and 11, and
the C1 pass marks). The trial it builds on: `docs/superpowers/plans/water-mode/all-drops-trial.md` and the code
on branch `origin/claude/project-thread-awquwz` (`crates/bpe-water/src/drops/`, `shaders/alldrops.wgsl`,
`trial.rs`, `plain.rs`, `tests/alldrops.rs`, `card/all-drops-trial/run.ps1`). Read the trial shader before Task 3:
it is the reference that is known to work. Use `git show origin/claude/project-thread-awquwz:<path>`.

## Global constraints

- Run within wgpu's default limits: a storage binding of at most 128 MiB, and at most 8 storage buffers a stage.
- Each per-drop buffer is at most 16 bytes a drop, so `MOST_DROPS` = 8,388,608 (128 MiB / 16).
- The processor side uses `bpe_core::maths::Maths` (the crate's `clippy.toml` refuses `f32::sin` and the like).
  WGSL may use its own maths.
- Same card, same result: every sum over neighbours goes in the order (bin, serial, index), and the fastest speed
  is a whole-number maximum. No step reads a value that depends on how the card schedules threads.
- Frames are 1/60 s. Floors, in steps a frame: 8 at 12 px cells, 16 at 8 and 6 px, 32 at 3 px and finer (the
  trial's 480, 960 and 1920 a second). At most `most_steps` = 128 a frame.
- Surface pull's skin is `coh × pull × surface_pull² / cell³` (cells/s²) with `pull` = 4.0e5 and `coh` = 2,
  uncapped unless a frame is at `most_steps`.
- The defaults change: `Feel::default().surface_pull` and `FeelCard::default().surface_pull` become 0.5.
- The grid core's behaviour does not change. Its tests keep passing under their new paths.
- Tests that use the card call `bpe_gfx::test_device()`. Run `cargo test` with `--test-threads=2`.
- Lines are at most 120 wide (`cargo fmt --all`). `cargo clippy --workspace --all-targets -- -D warnings` stays
  clean.
- No exe is built or installed. No demo films, except the card run in Task 8 that the spec asks for.

## Review focus

The five conditions the spec implies but no headline test covers, most likely to bite first. Each one has its
test in the task that owns the code.

1. **A tap narrower than one drop at draft** (for example a 3 px stream at 8 px cells, where `s` = 4 px). It must
   still pour its amount, as single drops with gaps. Test `a_thin_tap_pours_its_amount_as_single_drops`, Task 5.
2. **A pour that starts mid-frame or mid-step.** Its first row must have travelled exactly `speed × (step end −
   Starts at)`. Test `a_pour_starts_at_its_time`, Task 5.
3. **An empty body**, or a body whose every drop has left. `frame()` must not dispatch zero workgroups or read
   stale reports, and it falls back to the floor. Test `an_empty_body_moves_on`, Task 4.
4. **Open course edges with a pour running for a long time.** The count must stay bounded, with no live drop lost
   when the count shrinks. Test `drops_that_leave_are_counted_out`, Task 4.
5. **A copy made while a pour is part-way through a row** (its distance remainder and column remainder are
   non-zero). It must play on to the bit. Test `a_copy_with_pours_plays_on_the_same`, Task 5.

---

## File structure

| Path | What it is |
| --- | --- |
| `crates/bpe-water/src/grid/` | The grid core, moved from `src/sim/` unchanged (`mod.rs`, `numbers.rs`, `passes.rs`) |
| `crates/bpe-water/src/grid/shaders/` | Its shaders, moved from `src/shaders/` (`common`, `walls`, `hold`, `drops`, `grid`, `pressure`) |
| `crates/bpe-water/src/drop.rs` | Moved from `src/sim/drop.rs`: `Drop`, `STOPS`, `WATER`, `Dice`, `fill`, `Pour`, `Poured`, plus the new rows and stacked fill |
| `crates/bpe-water/src/drops/mod.rs` | The new engine: `WaterGpu`, `Water` |
| `crates/bpe-water/src/drops/numbers.rs` | `Numbers`, `Feel`, `FeelGpu`, `StepGpu`, `steps_for` |
| `crates/bpe-water/src/drops/buffers.rs` | The drops' buffers by field, growing, copying |
| `crates/bpe-water/src/drops/passes.rs` | Entry points, their bindings, pipelines |
| `crates/bpe-water/src/drops/clock.rs` | `Report` (the read-back ring), the frame's step count |
| `crates/bpe-water/src/drops/shaders/` | `common.wgsl`, `sort.wgsl`, `sweep.wgsl`, `finish.wgsl`, `report.wgsl` (joined with `concat!`) |
| `crates/bpe-water/src/scenes.rs` | `Scene::start` (new engine, `Running`), `Scene::start_grid` (`GridRunning`), the trial's shots added |
| `crates/bpe-water/src/plain.rs` | The trial's drawing on the processor, behind `test-scenes`, for films until C3 |
| `crates/bpe-water/src/bridge.rs` | Takes a `Feed` (a buffer of `Drop`s, a count, a layout), so either engine feeds it |
| `crates/bpe-water/tests/engine.rs` | The new engine's behaviour tests (run with the others) |
| `crates/bpe-water/tests/shots.rs` | Measuring runs, by hand and on the card (`#[ignore]`): `shots`, `pace`, `pools`, `openings`, `repeats` |
| `crates/bpe-water/tests/core.rs`, `streams.rs`, `measure.rs` | The grid core's tests, on `start_grid` and `grid::` |
| `crates/bpe-kit/tests/water_core.rs` | `WATER_CORES=old,grid,drops` (default `old,drops`) |
| `crates/bpe-kit/src/modules/water/liquid.rs` | `FeelCard` default Surface pull 0.5 |
| `card/all-drops-core-c1/run.ps1` | The card run (from the trial's `run.ps1`) |

---

### Task 1: Move the grid core aside

**Files:**
- Move: `src/sim/{mod.rs,numbers.rs,passes.rs}` to `src/grid/`, `src/sim/drop.rs` to `src/drop.rs`, and
  `src/shaders/{common,walls,hold,drops,grid,pressure}.wgsl` to `src/grid/shaders/` (`git mv`). `bridge.wgsl`
  stays in `src/shaders/`.
- Modify: `src/lib.rs`, `src/grid/passes.rs` (the `include_str!` paths), `src/scenes.rs`, `src/bridge.rs`,
  `src/probe.rs`, `tests/core.rs`, `tests/streams.rs`, `tests/measure.rs`, `crates/bpe-kit/tests/water_core.rs`.

**Interfaces:**
- Produces: `bpe_water::grid::{Water, WaterGpu, Numbers, Feel, MOST_DROPS, spots_bytes}` (today's
  `bpe_water::{Water, WaterGpu, Numbers, Feel, MOST_DROPS}`); `bpe_water::{Drop, Pour, Poured, STOPS, WATER,
  fill}` from `crate::drop`; `scenes::Scene::start_grid(&self, &Arc<grid::WaterGpu>, grid::Numbers) ->
  GridRunning` (today's `start`/`Running`, renamed).
- The grid core's environment prefix becomes `BPE_GRID_` (was `BPE_WATER_`, which the new engine takes).
- Until Task 2, `scenes::Scene.feels` stays `Vec<grid::Feel>`.

- [ ] **Step 1:** Do the moves and the renames. lib.rs keeps the old top-level names pointing at `grid::` for
  this task only, so the commit builds. Task 3 points them at the new engine.
- [ ] **Step 2:** Run `cargo test -p bpe-water -- --test-threads=2` and
  `cargo test -p bpe-kit --test water_core -- --test-threads=2`. Expected: the same tests pass as on main, with
  none added or lost.
- [ ] **Step 3:** Run `cargo clippy -p bpe-water -p bpe-kit --all-targets -- -D warnings` and `cargo fmt --all`.
  Commit "bpe-water: the grid core moves to `grid`".

### Task 2: The new engine's numbers and Feel

**Files:**
- Create: `src/drops/numbers.rs`, `src/drops/mod.rs` (only `pub mod numbers;` for now)
- Modify: `src/lib.rs`, `src/scenes.rs` (`Scene.feels: Vec<Feel>` of the new kind, converted with
  `grid::Feel::from(&Feel)` in `start_grid`), `crates/bpe-kit/src/modules/water/liquid.rs`

**Interfaces:**
- Produces, in `drops::numbers` (re-exported as `bpe_water::{Numbers, Feel}`):
  - `pub struct Feel { gravity: f32 (1.0), surface_pull: f32 (0.5), thickness: f32 (0.02), stickiness: f32 (0.0),
    weight: f32 (1.0), settles: f32 (0.0) }`, all `pub`, with defaults in brackets.
  - `impl From<&Feel> for grid::Feel` (gravity, surface_pull, stickiness, weight and settles copied; spray at
    the grid's default).
  - `pub struct Numbers`, whose fields and defaults are in the table below.
  - `Numbers::for_cell(cell: f32) -> Numbers`: the floor is the steps a frame from the global constraints, and
    everything else is the default.
  - `Numbers::from_env_for(cell: f32) -> Numbers`: each field can be replaced by `BPE_WATER_<FIELD IN CAPS>`.
  - `Numbers::at_rest(&self) -> (f32, f32)` and `Numbers::wall_table() -> [[f32; 4]; 17]`, as in the trial's
    `DropNumbers`.
  - `Numbers::skin(&self, feel: &Feel, cell: f32) -> f32`: the uncapped skin in cells/s².
  - `Numbers::evening(&self, feel: &Feel) -> f32`: `visc_base + visc_thick × thickness` (px²/s).
  - `Numbers::steps_for(&self, fastest: f32, feels: &[Feel], cell: f32) -> u32`: the frame's step count, from
    `fastest` (px/s, headroom already added).
  - `#[repr(C)] FeelGpu { a: [f32; 4], b: [f32; 4] }`: a = (gravity ×, skin, skin × curv, grip in cells/s²),
    b = (damping 8 × settles², drag /s, evening share for this step, weight). It is made by
    `Numbers::feel_gpu(&self, feel, cell, gravity_cells, dt, capped: bool) -> FeelGpu`. The evening share is
    `min(evening × dt × 20 / reach_px², 0.5)`. The skin is capped at `pull_cfl / dt²` only when `capped`.

| Field | Default | Meaning |
| --- | --- | --- |
| `floor` | 16 | Fewest steps a frame (`for_cell` sets 8, 16 or 32) |
| `most_steps` | 128 | Most steps a frame |
| `move_most` | 1.3 | Furthest the fastest drop moves in a step, in drop spacings |
| `iters` | 4 | Sweeps a step |
| `reach` | 1.5 | Cells |
| `relax` | 1.0 | Share of a sweep's correction taken |
| `eps` | 4.0 | Softening, as a share of a drop's own stiffness at rest |
| `wall_off` | 0.25 | Cells a drop rests off a face |
| `wall_share` | 1.0 | Share of the solids counted as liquid |
| `sub_move` | 0.4 | Cells per sub-move in predict |
| `most_move` | 4.0 | Cells a drop may move in a step (the speed cap is this over dt) |
| `most_fix` | 0.25 | Cells a sweep may move a drop |
| `visc_base`, `visc_thick` | 100, 2500 | Evening, px²/s (Thickness 0.02 gives the trial's 150) |
| `pull`, `coh`, `curv`, `pull_cfl` | 4.0e5, 2.0, 0.25, 0.005 | The skin, as the trial |
| `adh` | 2.0 | Grip at Stickiness 1, in gravities |
| `drag`, `drag_stick`, `drag_reach` | 2.0, 30.0, 0.75 | Drag /s, its Stickiness share, cells |
| `full_spout` | 0.9 | Fullness at a spout that holds its pour back |
| `held_rows` | 4 | Rows a held pour may owe |

`steps_for` is:

```text
spacing = cell / 2
n_speed = ceil(fastest / 60 / (move_most × spacing))
n_skin  = ceil(sqrt(max over feels of skin(feel, cell) / pull_cfl) / 60)
n       = clamp(max(floor, n_speed, n_skin), 1, most_steps)
```

- [ ] **Step 1: Write the failing unit tests** in `drops/numbers.rs`:
  - `the_density_at_rest_is_four_drops_a_cell`, the trial's test with `DropNumbers` replaced by `Numbers`.
  - `floors_follow_the_detail`: `for_cell(12.0).floor == 8`, `for_cell(8.0).floor == 16`,
    `for_cell(6.0).floor == 16`, `for_cell(3.0).floor == 32`, `for_cell(2.0).floor == 32`,
    `for_cell(1.5).floor == 32`.
  - `the_skin_gets_its_steps`: with `fastest` 0, Surface pull 1 at 3 px gives 41 steps, Surface pull 0.5 at 2 px
    gives 37, and Surface pull 1 at 1.5 px gives 115 (the builder check's table, to within 1 each). Surface pull
    0.5 at 6 px gives the floor of 16.
  - `fast_water_gets_its_steps`: at 6 px, `fastest` 3900 gives `ceil(3900 / 60 / 3.9)` = 17, and `fastest` 1e7
    gives 128.
  - `thickness_sets_the_evening`: `evening(&Feel::default())` is 150 within 1e-3.
- [ ] **Step 2:** Run `cargo test -p bpe-water --lib numbers`. Expected: it does not compile, because `Numbers`
  is missing.
- [ ] **Step 3:** Implement `drops/numbers.rs`. Then change `FeelCard::default()` in bpe-kit so `surface_pull` is
  0.5, and fix any bpe-kit test that pins 0.2.
- [ ] **Step 4:** Run `cargo test -p bpe-water --lib` and `cargo test -p bpe-kit --lib modules::water`. Expected:
  PASS.
- [ ] **Step 5:** Commit "bpe-water: the all-drops engine's numbers and Feel".

### Task 3: The engine: buffers, passes, a step with a sort that repeats

**Files:**
- Create: `src/drops/buffers.rs`, `src/drops/passes.rs`, `src/drops/shaders/{common,sort,sweep,finish}.wgsl`,
  `tests/engine.rs`
- Modify: `src/drops/mod.rs`, `src/lib.rs` (`pub use drops::{Water, WaterGpu, MOST_DROPS}`; the grid core is
  reached only as `grid::`), `src/scenes.rs` (`Scene::start` for the new engine)

**Interfaces:**
- Consumes: `Numbers`, `Feel`, `FeelGpu` (Task 2); `SolidField`, `Layout`, `Drop`, `STOPS` (unchanged).
- Produces:
  - `WaterGpu::new(&wgpu::Device, &wgpu::Queue) -> Arc<WaterGpu>`, `.device()`, `.queue()`.
  - `Water::new(gpu: Arc<WaterGpu>, field: &SolidField, gravity: Vec2, numbers: Numbers, feels: &[Feel]) ->
    Water`.
  - `Water::add(&mut self, born: &[Drop]) -> u32`: adds between frames and gives each drop the body's next
    serial (the input `serial` is ignored). Returns how many were added. Never more than `MOST_DROPS` in all.
  - `Water::frame(&mut self)`: in this task, `numbers.floor` steps of `1/(60 × floor)` s each, all in one
    submission. Task 4 adds the clock.
  - `Water::{count, frames, steps, layout, numbers, set_numbers, set_gravity, set_feels, set_edges_shut, gpu}`.
    `set_*` take effect from the next frame. `steps()` is the total taken.
  - `Water::read(&self) -> Vec<Drop>`: waits, and exports in chunks of at most 2^21 drops (each chunk's binding
    under 128 MiB). For the new engine, `Drop.c` is `[fullness, 0, n.x, n.y]` and `Drop.e` is 0.
  - `Water::copy(&self) -> Water`: waits, and plays on to the bit.
  - `Water::export(&self) -> &wgpu::Buffer` with `Water::feed(&self) -> bridge::Feed`. The `Drop` layout is for
    the bridge, filled on request.
  - `Water::drop_buffers(&self) -> DropBuffers<'_> { xv, nf, id, count }`, for C3's drawing.
  - `scenes::Scene::start(&self, &Arc<WaterGpu>, Numbers) -> Running`, with `Running::{frame, run_to(seconds),
    copy}` and `run_to` rounding to whole frames.

**Buffers (by field, 16 bytes a drop each):**

- Persistent, ping-ponged by the sort: `xv` (x.xy, v.xy, in cells and cells/s), `nf` (n.xy the liquid's direction
  from last step, fullness, spare), and `id` (kind, serial, colour, spare as `vec4<u32>`).
- Working: `xp` (the predicted place and spare), `ya` / `yb` (place and n, for the sweeps), `lamb` (dλ, λ, ρ /
  ρ0, spare), `velb` (new speed).
- Per bin: `counts` (atomic), `starts` (bins + 2).
- Per block: `blocks`. Plus `idx` (u32 a drop), the solid field, and the step's uniform.

They grow by doubling, from at least 4096 drops, copying the persistent fields. No pass binds more than 8 storage
buffers. Where splitting into fields pushes a pass over (`cs_apply` and `cs_finish` are the widest), read `id`
only where `kind` is needed, or pack.

**Passes for one step**, in order. Steps marked `(trial)` port `alldrops.wgsl`'s pass of that name:

1. `cs_predict` (trial, with `warm` gone): a drop with `i ≥ count_before` first copies itself in from the births
   buffer (Task 5; empty here). Then gravity, Settles, the speed cap, sub-moves kept off the solids, and its
   bin's count.
2. `cs_sum_a` (trial), `cs_sum_b` (**parallel**: one workgroup of 256 threads, each summing a contiguous run of
   blocks, a workgroup scan of the 256 sums, then each run's prefixes written), and `cs_sum_c` (trial).
3. `cs_scatter` (trial).
4. `cs_order`: as the trial, but **ranking by (serial, index) with no cap** on a bin's size. It also clears
   `counts` for the next step by striding over every bin, which replaces `cs_zero`.
5. `cs_lambda` and `cs_apply` (trial), `iters` times, ping-ponging `ya` and `yb`.
6. `cs_finish` (trial, without `warm`, `tens` or `coh_in`): the evening share comes from the drop's own Feel
   (`feel.b.z`).

**Tests** (`tests/engine.rs`, with each test returning early when `test_device()` fails, as the trial's do):

- [ ] **Step 1: Write the failing tests.**
  - `a_pool_stays_in_its_box_and_keeps_its_height`: the trial's test on `scenes::still_box(12.0, 0.3)` with
    `run_to(1.0)` and the same assertions (kept `0.93..1.05`, rms below 60 px/s, no drop outside the box).
  - `two_runs_and_a_copy_are_the_same`: the trial's repeat test on `scenes::weird(12.0)`, through `Running`.
  - `a_heap_on_one_point_spreads_and_repeats`: 2000 drops at one point in the middle of a 600 × 900 box at 12 px.
    After 0.5 s every drop is inside the box, no two drops are within 0.05 cell of each other, and two runs
    give the same `probe::print_of`. This is the "no cap" test: the trial's 192-drop cap broke it.
  - `a_lone_drop_falls_freely`: one drop with no solid near it. After 0.25 s its fall is `½ g t²` within 1 %.
  - `a_thin_wall_holds_a_fast_pour`: the grid core's test of the same name in `core.rs`, on the new engine
    (drops given 2000 px/s straight at a 3 px wall; none crosses it).
  - `an_empty_body_moves_on`: `frame()` ten times with no drops. Nothing panics and `count()` is 0.
- [ ] **Step 2:** Run `cargo test -p bpe-water --test engine -- --test-threads=2`. Expected: it does not compile.
- [ ] **Step 3:** Implement the buffers, the passes, `Water` and `Scene::start`.
- [ ] **Step 4:** Run the same command. Expected: 6 passed. Then `cargo test -p bpe-water -- --test-threads=2`,
  where the grid core's tests still pass.
- [ ] **Step 5:** Commit "bpe-water: the all-drops engine (`Water`), a frame of steps in one submission".

### Task 4: The clock, and drops that leave

**Files:**
- Create: `src/drops/clock.rs`, `src/drops/shaders/report.wgsl`
- Modify: `src/drops/mod.rs`, `tests/engine.rs`

**Interfaces:**
- Produces:
  - `#[repr(C)] Report { fastest: u32 (the f32 bits of the fastest live drop's speed in px/s), live: u32,
    spare: [u32; 2], spout: [f32; MOST_POURS] }`, with `MOST_POURS` = 64.
  - `Water::steps_last_frame(&self) -> u32`.
- `Water` keeps a ring of 3 reports. Each slot has a storage buffer the report pass writes and a `MAP_READ`
  staging copy, plus the host-side values for frames `k − 1` and `k − 2`.

**What `frame()` does at frame `k`:**

1. If `k ≥ 2`, wait for frame `k − 2`'s report and read it. If `k < 2`, use the start's values: `fastest` is the
   largest speed among the drops added so far and the active pours' speeds, and `live` is `count`. (`Water` keeps
   that largest speed as it adds drops.)
2. Shrink the count: `count -= (count at the end of frame k − 2) − live(k − 2)`. Dead drops (outside the course)
   sort into the last bin, births go in before each step's sort, and a dead drop never comes back. So after any
   sort the dead drops sit last, and the last slots are dead.
3. `fastest = max(report fastest, largest active pour speed) + |gravity| × 2/60`, and then
   `n = numbers.steps_for(fastest, feels, cell)`. The skin is capped (`capped = true`) only when `n ==
   most_steps`.
4. Write the step's uniform once with `dt = 1 / (60 n)` and the speed cap `most_move / dt`. Encode the `n` steps.
5. After the last step, `cs_report`: the fastest live speed by `atomicMax` on its bits, and `live` from
   `starts[dead bin]`. Then the spout fullness for each pour (Task 5; zeros here). Copy it into slot `k % 3`.
   Submit.
- `copy()` waits for the card, reads the reports for `k − 1` and `k − 2` into the copy's host-side values, and
  carries `frames`, `steps` and the next serial.

- [ ] **Step 1: Write the failing tests.**
  - `the_clock_takes_more_steps_for_faster_water`: on a still pool at 8 px, `steps_last_frame()` equals the floor
    (16) after 1 s. Drops fired at 6000 px/s on an empty 8 px course give more than 16 from the third frame on
    (`steps_for` gives 20 there).
  - `two_runs_and_a_copy_are_the_same` (Task 3's test) still passes, and now also covers frames whose counts
    differ.
  - `drops_that_leave_are_counted_out`: a course with open edges (`set_edges_shut(false)`) and 500 drops fired
    off its bottom. After 1 s `count()` is 0. In a second body with a pool kept inside the course plus drops
    leaving, every pool drop is still there by serial.
- [ ] **Step 2:** Run `cargo test -p bpe-water --test engine -- --test-threads=2`. Expected: the new tests fail.
- [ ] **Step 3:** Implement `clock.rs`, `report.wgsl` and the frame flow.
- [ ] **Step 4:** Run the same command. Expected: PASS.
- [ ] **Step 5:** Commit "bpe-water: the water picks its own steps".

### Task 5: Births as even rows, and fills stacked on the floor

**Files:**
- Modify: `src/drop.rs`, `src/drops/mod.rs`, `src/drops/shaders/sort.wgsl` (`cs_predict` takes births),
  `src/drops/shaders/report.wgsl` (spout fullness), `src/scenes.rs`, `tests/engine.rs`

**Interfaces:**
- Produces:
  - `Poured { owed: f32, born: u32, extra: f32 }`, where `extra` is the column remainder (the grid core ignores
    it).
  - `Pour::rows(&self, state: &mut Poured, layout: &Layout, from: f32, dt: f32) -> Vec<Drop>`: this pour's rows
    for the step from `from` to `from + dt` seconds.
  - `Water::set_pours(&mut self, pours: &[Pour])`: at most `MOST_POURS`. Their `Poured` states live in `Water`,
    and copies carry them.
  - `fill_stacked(layout: &Layout, field: &SolidField, lo: Vec2, hi: Vec2, kind: u32) -> Vec<Drop>`.
- `Scene::start` uses `set_pours` and `fill_stacked`. `start_grid` keeps `Pour::drops` and `fill`.

`Pour::rows` works like this:

```text
s     = cell / 2 (px);  wide = rate × width (px);  exact = wide / s (columns)
time window: only the part of [from, from + dt) inside [self.from, self.until) counts; travelled = speed × that
owed += travelled
pitch = s if exact ≥ 1, else s / exact            (under one drop: single drops, gaps between them)
while owed ≥ pitch:
    owed -= pitch
    m = 1 if exact < 1, else floor(exact) + (1 if extra + fract(exact) ≥ 1)
    extra = extra + fract(exact) - (1 if that row took the extra drop)
    the row's m drops at u_i = (i + 0.5) / m - 0.5, place = spout + across × u_i × (m × s) + way × owed,
    speed = fanned(u_i) × speed
```

- The room check: `cs_report` writes each pour's fullness at its spout (the density sum over the neighbouring
  bins at `place`, with the solids' share, over ρ0). If frame `k − 2`'s value is at least `full_spout`, that
  pour's rows are not born this frame, and `owed` stops at `held_rows × pitch`.
- The births of every step in the frame are built on the host. They go into one births buffer with each step's
  start and length, are uploaded once, and `cs_predict` copies a step's drops in at `count_before`.
- `fill_stacked`: for each lattice column (x at `c × 0.5 + 0.25` cells, as `fill`), it finds the face below the
  box's lowest point in that column by marching the field down (at most the box's height). Drops are stacked
  from `face − wall_off − 0.5 k` upward to `hi.y`. With no face within the box, it stacks from `lo.y`. Every place
  must be at least `wall_off` clear of a solid.

- [ ] **Step 1: Write the failing tests.** Unit tests in `drop.rs`:
  - `rows_pour_their_amount`: at 8, 6, 3 and 2 px with width 19.2 and rate 1 over 1000 rows,
    `drops × s² / (rate × width × travelled)` is within 2 % of 1. The same holds at rate 0.5.
  - `a_thin_tap_pours_its_amount_as_single_drops`: width 3 px at 8 px cells gives rows of one drop, and the
    amount is within 2 %.
  - `a_pour_starts_at_its_time`: a pour with `from` 0.0123, stepped in steps of 1/960 s. Its first row's place
    along the way is `speed × (step end − 0.0123) − pitch × rows before it` within 1e-4 px.
  - `rows_are_even`: within one row the drops are exactly `s` apart, and consecutive rows are `s` apart along the
    way.

  In `tests/engine.rs`:
  - `a_tap_pours_its_amount`: `scenes::thin_pour` (width as the scene, course edges shut) at 12 and 8 px. After
    1 s,
    `count() × s²` is within 2 % of `rate × width × speed × (1 s − from)`.
  - `a_tap_is_born_even`: at 8 px after 0.5 s, no drop within 10 spacings of the spout has sideways speed above 5 %
    of the pour's speed.
  - `a_turned_down_tap_is_narrower`: at rate 0.5, the stream's width 5 spacings below the spout is half the width
    at rate 1, within one spacing.
  - `a_submerged_spout_holds_back`: a spout inside a full box. Over 1 s the count grows by at most `held_rows ×
    columns` plus the first two frames' rows.
  - `a_copy_with_pours_plays_on_the_same`: Task 3's repeat test with a copy made at frame 7, where the pour's
    remainders are not zero.
  - `a_fill_starts_at_rest`: `still_box(12.0, 0.3)` filled by `fill_stacked`. The lowest row is 0.25 cell off the
    floor within 1e-3, and over the first 0.25 s the rms speed is below 20 px/s.
- [ ] **Step 2:** Run `cargo test -p bpe-water -- --test-threads=2 rows engine`. Expected: the new tests fail.
- [ ] **Step 3:** Implement it.
- [ ] **Step 4:** Run the same command. Expected: PASS.
- [ ] **Step 5:** Commit "bpe-water: pours born as even rows, fills stacked on the floor".

### Task 6: Measuring runs, the plain drawing on the processor, the bridge and the trial's shots

**Files:**
- Create: `src/plain.rs` (the trial's, behind `test-scenes`, taking `&[Drop]` from either engine),
  `tests/shots.rs`
- Modify: `src/scenes.rs` (the trial's `slope`, `ledge`, `pool_pour` and `beads`, and `shot(name, cell)`),
  `src/bridge.rs` (`Bridge::new(device, queue, field)`, `update(&Feed)`, `source(&Feed)`, where
  `Feed { buffer, count, layout }` is made by `Water::feed()` and `grid::Water::feed()`), `src/lib.rs`, and
  `crates/bpe-kit/tests/water_core.rs` (`WATER_CORES` takes `old`, `grid` and `drops`, default `old,drops`)

**Interfaces:**
- `tests/shots.rs`, all `#[ignore]`, run by hand. Each prints a table whose first line names the card:
  - `shots`: films and stills in the plain drawing (`WATER_SHOTS`, `WATER_CELLS`, `WATER_SECONDS`, `WATER_FPS`,
    `WATER_STILLS`, `WATER_FILM_SCALE`, `WATER_DOTS`, `WATER_PULL`, `WATER_STICK`, `WATER_SHIFT`, as the trial's).
  - `pace`: wall seconds for each scene second, physics alone, no read-back. It also prints the mean steps a frame
    and the time a step.
  - `pools`: the trial's pool table.
  - `openings`: the trial's course table.
  - `repeats`: twice from the start, and from a copy, at 6 and 3 px. The film frames are numbered to match the
    trial's, so they line up side by side.

- [ ] **Step 1:** Port the trial's code, adapted to frames: a film frame is drawn every `60 / WATER_FPS`
  frames, and `WATER_FPS` must divide 60.
- [ ] **Step 2:** Run `WATER_OUT=<scratch> cargo test --release -p bpe-water --test shots -- --ignored --nocapture
  --test-threads=1 pace` on the cloud's software card, with `WATER_SHOTS=weird WATER_CELLS=12`. Expected: a
  table printed, and the program exits 0.
- [ ] **Step 3:** Run `cargo test -p bpe-kit --test water_core -- --test-threads=2`. Expected:
  `the_bridge_draws_every_drop` passes for both engines (parameterise it over `grid` and `drops`).
- [ ] **Step 4:** Commit "bpe-water: the trial's shots and measures on the new engine".

### Task 7: Fewer passes and per-step time

**Files:** `src/drops/mod.rs`, `src/drops/shaders/*.wgsl`

- [ ] **Step 1:** Measure the time a step with `pace` on the software card at 12 px on `weird`, as the baseline.
- [ ] **Step 2:** Join the finish with the next step's predict, inside a frame (the last step of a frame keeps a
  plain finish), so a step takes 14 dispatches at 4 sweeps (6 + 2 × 4) instead of the trial's 15. Keep the
  join only if `two_runs_and_a_copy_are_the_same` and every `engine.rs` test still pass, and the time a step
  drops.
- [ ] **Step 3:** Measure again and write both numbers in the plan's "As built". If C1's target (at most 0.25 s
  a scene second at Fine on the card) looks out of reach after Task 8's card run, try re-sorting every second
  step with bins `reach + 0.25` wide. Keep that only if every drop still feels exactly the neighbours within its
  reach (a test compares the neighbour counts each way on `weird`).
- [ ] **Step 4:** Commit "bpe-water: fewer passes a step".

### Task 8: The card run, the notes, the review and the merge

**Files:**
- Create: `card/all-drops-core-c1/run.ps1` (from the trial's: the six shots at 6, 3 and 2 px; Surface pull 0.2 /
  0.5 / 1 on slope, ledge and beads; `pace`, `pools`, `openings`, `repeats`; Water Weird from Aedan's file
  through `water_core.rs`)
- Modify: this plan ("As built and measured"), `docs/notes/water.md` (a "Part C1" section), `CLAUDE.md` (the
  `bpe-water` crate map line), `crates/bpe-water/Cargo.toml` (description)

- [ ] **Step 1:** Push, open the pull request as a draft, and run the full checks in CI.
- [ ] **Step 2:** The card run. Start a Remote Control session on `F:\!Ai Stuff\BallPhysicsEngine` (the recipe
  is in project memory `liquid-speed` and `all-drops-trial`). It builds in WSL `bpe-ci`, runs `run.ps1`
  natively, joins the films with the PC's ffmpeg, and pushes the results to its own branch as a `[skip ci]`
  commit. Films go to project files `water-core/c1/`. End the session as soon as the run is done.
- [ ] **Step 3:** Judge against the C1 pass marks: each shot side by side with the trial's films at Fine,
  Extreme and 2 px; the course's shares within 3 points of the trial's; repeats; births; and pace against the
  0.25 target. Write the verdict and numbers in "As built and measured". Remove the card results from the branch
  (`git rm`) before merging.
- [ ] **Step 4:** Notes, CLAUDE.md line, Cargo description. Run `cargo fmt --all` and
  `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5:** One Opus review of the whole branch (superpowers:requesting-code-review), with its findings
  fixed. Merge when every check is green. Tell Aedan, with the films, and hand C2 and C3 to the coordinator as
  two threads.

## Things not to do

- Don't change the grid core's behaviour, numbers or tests (only their paths and names).
- Don't keep the trial's dropped knobs: `warm`, `warm_most`, `tens`, `tens_low` and `coh_in` go.
- Don't squeeze a pour's rows to match its amount, and never place a born drop by a random number.
- Don't read the card's report from the frame just sent (k − 1 or k). The step count must come from frame
  `k − 2`, or runs stop repeating.
- Don't build the deep-water push, landing fixes or the plain drawing on the card. Those are C2 and C3.
- Don't touch today's Liquid thing, `bpe-gfx/src/liquid_sim/`, or anything in a Balls scene.

## As built and measured

Built on `claude/project-thread-x7f3q2` (pull request #231). Two card runs on the RX 9070 XT (results on draft
pull requests #232 and #233, never merged; the second run's films are in project files `water-core/c1/`).

**Verdict against the C1 pass marks (second card run, commit 1b8cae9):**

| Mark | Result |
| --- | --- |
| Thin water the same as the trial's or better, by eye, at Fine (6 px), Extreme (3 px) and 2 px | Passed: slope, ledge, weird and beads at every detail and at Surface pull 0.2, 0.5 and 1, and slope and weird on all four grid shifts at 6 and 3 px. The 2 px stream is a clean thread again (it burst at the spout in the first run, below) |
| Course shares within 3 points of the trial's | Passed: after 30 s, 6 px 7.4 / 0.0 / 5.3 / 34.5 / 52.9 % against the trial's 7.5 / 0.0 / 2.9 / 36.3 / 53.3 (2.4 points at most); 3 px 7.2 / 0.0 / 3.3 / 37.8 / 51.6 against 7.3 / 0.0 / 3.2 / 37.9 / 51.6 |
| Repeats | Passed: weird and pool at 6 and 3 px give the same print in two runs of the program and from a copy at half time; the engine tests repeat with a pour, and with steps that change while drops leave |
| Births | Passed (`a_tap_pours_its_amount`, `a_tap_is_born_even`, `a_pour_stays_calm_at_the_most_steps_a_frame`) |
| No drop crosses a solid; a lone drop falls freely | Passed (`a_thin_wall_holds_a_fast_pour`, `a_lone_drop_falls_freely`) |
| *Target*: at most 0.25 s a scene second at Fine | **Missed**: 0.283 (slope) to 0.388 (pool), against the trial's 0.354 to 0.425 |

Pace, physics alone, wall seconds a scene second (steps a frame): 6 px weird 0.307 (16.1), slope 0.283 (16.0),
ledge 0.338 (17.9), pool 0.388 (16.2), course 0.333 (16.2); 3 px weird 0.642, slope 0.620, ledge 0.681, pool
1.195, course 0.861 (all 32; the trial took 0.81 to 1.47); 2 px weird 0.802, slope 0.741, pool 2.106 (32). The
pool at rest: 6 px 0.55 px/s after 20 s and 97.0 % of its height, 3 px 0.81 px/s and 97.2 %; at 2 px still
39 px/s after 8 s (the trial's 2 px pool also moved 39 px/s), all C2's to fix. Slosh 60, 63 and 64 % left after
four half swings. Aedan's Water Weird through the bridge: pieces and wholeness at Fine and Extreme the same as
the trial's within noise (Fine still 320 to 400 pieces against today's 69 to 109: C2's landing mark).

**Changes from the plan:**

- **At most 32 steps a frame, not 128** (`Numbers::most_steps`). The first card run's 2 px stream burst at the
  spout (drops thrown sideways and back above it), and a 3 px slope on one grid shift too; those frames took 52
  to 77 and 34 to 48 steps. On the software card a thin pour at 3 px, Surface pull 0.5, is calm at 16 and 32
  steps and bursts from about 40 (at 64: 2000 px/s sideways and 42 px above the spout). The skin's bend term
  (the trial's: the difference of "which way the liquid lies", which points into the liquid, taken away) is
  only calm at the trial's pace, and the trial never took more than 32 steps a frame (its rate stopped at 1920
  a second). Turning the bend round is calm at any step count but loses the beads on the plates and the
  stream's break into drops at 6 px, and sprays a 2 px pour into a cone; no bend at all loses the beads too. So
  the bend stays the trial's, a frame takes at most 32 steps, and a skin that wants more is capped (2 px at
  Surface pull 0.5 to 74 %, 3 px at Surface pull 1 to 62 %, 2 px at Surface pull 1 to 18 %), all as in the
  trial. At 3 px and finer every frame is now the trial's 32 steps. `a_pour_stays_calm_at_the_most_steps_a_frame`
  pins it (it fails at the old ceiling). The plan's skin rule stays, for a raised `most_steps`.
- **Re-sorting every second step was not tried.** Two drops at the clock's `move_most` (1.3 spacings a step) close
  1.3 cells between two sorts, far more than the plan's 0.25-cell margin, so "every drop still feels exactly the
  neighbours within its reach" can't hold for fast water. The speed is C2's, by its lower floors.
- **The join is kept** (Task 7): bit-identical, the same within noise on the software card (baseline: weird at
  12 px, 8 steps a frame, 1.481 ms a step, 0.711 s a scene second), and 4 to 6 % faster on the card at Fine
  (weird 0.307 against 0.324, ledge 0.338 against 0.361; the pool the same).
- Births (Task 5): `Pour::rows` takes f64 times, so a pour's start lands within 1e-4 px. The engine's `Pour` has
  no Rate: its width is the stream's (Rate × Width, applied by whoever builds it). A spout's room is judged a reach
  behind it, counting the new drop itself (the plan's density at the spout point read only 0.6 in a full pool).
  Births' fastest speed counts in the clock for the frame they are born in.
- Fills (Task 5 and the review): each column is set down on a face within `FILL_SNAP` (16 px, at least two cells)
  below the box's foot, a quarter cell off it; a column over an opening or in mid-air stays in the box.
  `a_fill_starts_at_rest` is not reachable in C1: a pool squeezes about 3 % under its weight and bounces for
  about 0.2 s (up to 100 px/s); `a_fill_is_stacked_on_the_floor` checks the stacking and the rest is C2's. During
  that bounce a drop by a side wall is thrown up it (0.67 cell from the wall: 605 px/s; 0.5: 332; 0.33: 209).
- The bridge (Task 6) has its own kernel for the all-drops engine's buffers (`cs_bridge_drops`, no export pass);
  `water_core.rs` gives this engine walls `LEAST_WALL` (1.5) cells thick, as its own scenes have, and leaves out
  today's starting drops inside that band. `tests/shots.rs` is the new engine only; `WATER_THICK` replaces
  `WATER_SPRAY`, and `pace` prints steps a frame and ms a step.
- The review's fixes: births capped at `MOST_BIRTHS` a frame; the clock's fall headroom uses the strongest
  liquid's Gravity; `reach` never below 0.5; drops that left the course sort unranked into the last bin and
  `read()` returns only drops in the course; the spout's fullness is read clamped inside the grid.

**Handed to C2:** the pace target (lower floors); a pool's start bounce, the wall jet and the 2 px pool that is
not at rest; Water Weird's pieces at Fine. A pour in zero gravity still bursts at the spout at 32 steps (as the
trial's would). A skin that stays calm at any step count (a bend from smoothed normals, or the skin applied at
no more than 1920 steps a second while the pressure takes more) would let fast water at fine detail take more
steps and the slider's top reach real ropes at 2 px, where the cap now holds it to 18 %.
