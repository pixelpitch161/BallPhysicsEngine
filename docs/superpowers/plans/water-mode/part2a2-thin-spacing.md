# Water mode part 2a-2: thin water keeps its spacing (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On the owner's own `Water Weird.ron`, the new water core's film, thread and arcs look like real liquid at
Fine and at Extreme on the owner's RX 9070 XT (even films, a whole thread, no lumps, hairs or ribs), still pools
rest as before, and the speed is reported.

**Architecture:** Three changes to `bpe-water`, each found by measuring the owner's scene (this file's "What the
measuring found"). (1) **Elbow room**: drops on the move that are nearer each other than a rest spacing move apart,
places only, never speeds, found through a grid of half-cell "spots" with whole-number sums. (2) **A pour is a
stream, not a spray**: a Pour's Spread fans the stream smoothly across the spout (default none, as the Water Pour
card's), where today's gives every drop a random way of its own. (3) **Steps by detail**: the liquid steps often
enough that fast water crosses under about a cell a step (`Numbers::for_cell`), so a film that hits a shape is
turned over several steps and not thrown off in one.

**Tech stack:** Rust (pinned toolchain), wgpu 30 compute (WGSL), `bpe-water`, `bpe-kit`'s `water_core.rs` test.

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` section 3 ("The physics core") and Parts item
2a. Before this: `docs/superpowers/plans/water-mode/part2a.md` ("Why the owner's thread breaks at Extreme").
Notes: `docs/notes/water.md`.

**The owner's words (2026-11-25):** "It doesn't necessarily have to look similar to the previous liquid iteration.
I want it to look better and to look as much like actual liquid would behave as possible." So the judge is real
water, and today's core is the "no worse than" line, not the target.

## Global constraints

- **Balls scenes do not change.** No golden or pinned picture moves. Nothing under `crates/bpe-gfx/src/liquid_sim/`,
  `crates/bpe-gfx/src/shaders/liquid*.wgsl` or `crates/bpe-kit/src/things/liquid/` is edited.
- **Same card, same result.** Two runs of a scene on one device give the same drops to the bit, and a copy taken
  part-way plays on the same. Every new sum over drops is a whole-number atomic add; nothing new depends on the
  order drops are handled in.
- **No read-back in a step.** The host never waits for the card while stepping.
- **Still pools rest as before**: a settled pool plays on the same to the bit with the elbow room on and off (it
  acts only on water on the move), and 2a's pool tests pass unchanged.
- **Nothing calms the liquid by taking speed off it.** Elbow room moves places only.
- **Pass mark** (the coordinator's brief, with the owner's words above): films of the owner's `Water Weird.ron`
  from the RX 9070 XT at Fine and at Extreme, looked at full size and as 1:1 crops, show the thread off the disc
  whole and the films even; pools still rest; the speed is measured there and reported.
- Physics Rust in `bpe-water` uses `bpe_core::maths` (its `clippy.toml` refuses the platform's).
- Do not build or install the exe. Tests: only this part's; CI runs the rest. No scenes for the owner (test scenes
  stay behind `test-scenes`). Tiles are not built here (their own part).
- Card results come back through a Remote Control session as a `[skip ci]` commit under `card/` and are taken out
  of the tree (`git rm`) before the merge; the films go to `/mnt/project-files/water-core/`.

## Review focus

Inputs the brief implies but does not spell out. Each has a test in the task that owns the code.

1. **A course too big for the spots** (four spots a cell, 12 bytes each: past the card's largest storage binding,
   128 MiB by default, at about 2.8 million cells): the body is still made and still steps, with the elbow room
   off, never a failed buffer (Task 1, `spots_fit_or_are_left_out`, `no_elbow_room_still_runs`).
2. **Many drops on one point** (a pour into a corner, drops stopped at the course's edge): they part without a
   burst, each its own way, and no number breaks (Task 1, `a_heap_on_one_spot_spreads_without_a_burst`).
3. **A drop at the grid's very edge or off it**: its spot is held inside the buffer; a drop that no longer steps
   is not counted (Task 1, `drops_at_the_grids_edge_are_safe`).
4. **Still water** must not shimmer because drops are nudged: the gate on speed (Task 1,
   `still_water_is_left_alone`).
5. **A pour with a wide Spread** (45 degrees) or a width of nothing: the fan stays inside its Spread and the
   drops' ways are finite (Task 2, `a_pour_fans_out_smoothly`).

---

## What the measuring found (before any code was kept)

Runs on lavapipe in the cloud container, the hand-built Water Weird layout (`scenes::weird`) at 3 px cells unless
they say otherwise; the layout shows the owner's fault there too. Tools: `measure.rs::look` with `WATER_DUMP`
(the drops written out) and scripts that count drops along the block's top and in the air after its corner.

1. **The drops sit at random.** A pour's births are random within the stream, and nothing in the core spaces
   drops below a cell. Half the drops had another within a fifth of a cell (the median nearest neighbour 0.19
   cell, where an even liquid's is 0.4 to 0.5), and a tenth were on top of another. A film two cells thick is
   then lumpy by chance alone: along the block's top its thickness varied by 35 % from one 6-pixel stretch to
   the next (counting noise for drops placed at random would be 24 %).
2. **Bunches are thrown, not spread.** Where a bunch fills a cell, the pressure parts it by giving the drops
   speed (`most_out`), and in the air nothing takes that speed back: each bunch flies on as a short row across
   the stream. These are the "hairs".
3. **Today's pour fans at random.** `births::pour` gives every drop its own way within Spread (4 degrees by
   default), at random. On the new core that is not a fan but sideways speed in every drop: the stream arrives
   three to four times wider than it left and a quarter as dense, lands in pulses, and feeds the film in pulses
   (the film's thickness in the air after the block varied by 45 %, thinnest stretch a quarter of the thickest).
   With no Spread: 20 %, thinnest half of the thickest. A smooth fan of the same 4 degrees is as bad as the
   random one: it is the thinned-out stream that lands badly, so a fan is the owner's to ask for, and Water
   mode's Pour card already defaults to none.
4. **A film that hits a shape head-on needs more than one step to turn.** At 240 steps a second and 3 px cells
   the film off the block moves 2.4 cells a step and is two cells thick: the place where it is stopped on the
   disc is smaller than one step's travel, so it is turned in jolts, and the film over the top of the disc
   leaves in ribs. At 960 steps (0.6 cell a step) the same film turns smoothly and the ribs are gone.
5. Tried and dropped here: no speed push at all from over-full cells (`most_out` 0): straighter thread at 240
   steps, more beads at 960, and pools lose what keeps them from squashing; six multigrid cycles, `fill` 0.25,
   the full push in thin liquid, the pull and the along-flight evening off: no change to the film's evenness.
   Elbow room for still water too: a still box no longer rested (6 px/s against 0.4), at any reach; hence the
   gate on speed.

## Design decisions (all of them)

### Elbow room

1. **What it is.** In `cs_g2p`, after a drop has gathered its speed, it is moved (its place only) away from the
   other drops nearer than `apart_reach`, by a share of each overlap. Its speed, its APIC matrix and its stretch
   are untouched, so squeezed liquid takes its room back without being thrown, and a bunch in a thread swells
   into a bead and not into a row of flying drops.
2. **How neighbours are found: spots.** A spot is a square half a cell wide (four a cell, the rest spacing of four
   drops a cell). A new buffer `spots` holds, per spot, three whole numbers: how many drops are in it and the sums
   of their places within it across and down (0 to `SPOT_ONE` = 4096 each). `cs_hold_p2g` (already one pass over
   the drops at the start of a step) adds each stepping drop to its spot with three atomic adds; `cs_clear`
   clears a node's four spots. In `cs_g2p` a drop reads the nine spots round it; each spot's drops are taken
   together at their middle (their sum over their count), its own spot's without itself (its own whole-number
   place is taken off the sum, exactly as it was added). So the sums are whole numbers, the answer does not
   depend on any order, and there are no lists and no caps on drops a spot. Why not exact pairs: lists of
   drops a cell filled by atomics hold their drops in whatever order the card ran them, and which drops fit a
   full list would differ from run to run.
3. **The push.** For each of the nine spots with `n` other drops whose middle is `far` away (cells), with
   `far < reach`: the drop moves by `gain * min(n, 4) * (reach - far)` straight away from that middle. Two
   drops on the very same point (`far` under a thousandth of a cell) part each its own way, an angle from its
   serial (scrambled, `(serial * 2654435761) >> 22` of 1024 parts of a turn, so drops born one after the other
   part widely), so a heap on one point spreads. The whole move is capped at `apart_most` cells a
   step and dropped if it is no number. The moved place then goes through `keep_off` like every sub-move, so
   elbow room cannot put a drop in a solid.
4. **Only on the move.** The move is scaled by `smoothstep(apart_pace / 3, apart_pace, speed)` of the drop's own
   gathered speed (`apart_pace` 300 px/s: none under 100, in full from 300): water at rest is not touched at
   all (a settled pool is the same to the bit), films and threads (500 px/s and up) have it in full. (At 150
   a deep pool took seconds longer to settle: its first slosh was fast enough to be nudged, and the nudges
   kept its top moving. At 300 it settles as with no elbow room.)
5. **The numbers** (`Numbers`, all replaceable by `BPE_WATER_*` in the measuring tests): `apart` 150 a second (the
   share of an overlap undone a step is `1 - exp(-apart * dt)`, so the same in a second at any step rate),
   `apart_reach` 0.45 cell (held to 0.5: nine spots reach no further), `apart_most` 0.25 cell a step,
   `apart_pace` 300 px/s. On the card they travel in a new `Step.r` (`StepGpu.r`): gain, reach, most, pace in
   cells a second.
6. **Too big a course.** `spots` is `cells * 4 * 12` bytes. When that is over the device's
   `max_storage_buffer_binding_size`, the body is made with a small stand-in buffer and `Step.r.x` 0;
   `cs_clear` and `cs_hold_p2g` skip the spots when `r.x` is 0, and `elbow_room` returns nothing. Tiles (their
   own part) lift this.

### A pour is a stream

7. `bpe_water::Pour::fanned(u: f32) -> Vec2`: the unit way of a drop born at `u` across the spout (-0.5 to 0.5,
   held to that): `aim + spread * 2 * u` degrees. `Pour::drops` uses it in place of a random way. `Pour::default()`
   has `spread` 0 (the Water Pour card's default, `modules/water/places.rs`).
8. The side-by-side (`bpe-kit/tests/water_core.rs`, `NewCore::step`) keeps today's births for the new core (their
   places, colours, serials) and replaces each drop's way by `fanned(u)` with `u` from its place across the
   spout; the Spread is 0 unless `WATER_FAN=1` keeps the scene's. Today's core is fed today's births unchanged.
   Why: the owner's file is a Balls scene whose pour has today's default Spread of 4 degrees, never set by the
   owner; rebuilt in Water mode its pour has the Pour card's default, none.

### Steps by detail

9. `Numbers::for_cell(cell: f32) -> Numbers`: the defaults with `rate = 240 * k`, `k = ceil(PACE_SPEED /
   (PACE_CELLS * cell * 240))` held to 1 to 8, `PACE_SPEED` 2000 px/s (fast water on one screen), `PACE_CELLS`
   0.75. So 12 px cells step 240 times a second, 8 px and 6 px 480, 3 px 960. `Numbers::default()` stays at 240
   (the behaviour tests' grids are coarse). The side-by-side and the measuring tests start from
   `Numbers::for_cell` and then apply `BPE_WATER_*` (`Numbers::from_env_for(cell)`; `from_env()` stays as it
   is). The spec's "the water picks its own step from how fast it is moving" is still part 2b's (it needs the
   fastest drop read back): this rule is the stand-in until then, and the measured speeds say what it costs.
10. Per-step numbers that would act four times as often at 960: `most_out`, `most_out_thin`, `even_most`,
    `apart_most` are caps a step and stay so (2a measured pools at 240, 960 and 1920 steps with them); `apart`
    and `even` are rates a second already.

### The liquid pushes back before a cell is full

10a. `Numbers::fill` 1 becomes 0.5 (the share of a cell's room that may fill in a step before the cell pushes
    back). Found on the owner's file at Fine: the film over the disc left in teeth there, but not on the
    hand-built layout. The only difference was where the grid's cells lay: the hand-built layout with its grid
    moved a quarter of a cell (`WATER_SHIFT=0.25`) showed the same teeth. A film thinner than a cell (5 px on
    6 px cells) met the disc with no pressure at all until a cell was full, then all at once, and when that
    happened hung on the cells' places. At 0.5 (and 0.25) the film is turned smoothly on all four shifts
    tried; the thread below is cleaner and alike across shifts (how ragged, `probe::ragged`, mean of four
    shifts: 0.84 px at fill 1, 0.58 at 0.5). Part 2a chose 1 because water landing on a shape left it in
    bursts at less: that was the sprayed pour at 240 steps, and is not so now (2a's tests pass at 0.5).

### Measuring

11. `probe::Spacing::of(drops: &[Drop]) -> Spacing { median: f32, close: f32 }`: each drop's distance to its
    nearest other (cells) through a hash grid: the median, and the share nearer than a quarter cell.
    `probe::along(drops, layout, from: Vec2, to: Vec2, reach: f32, bins: usize) -> Vec<u32>`: how many drops lie
    within `reach` pixels of the world segment `from`-`to`, by `bins` equal stretches along it.
12. `measure.rs`: `look` gains `WATER_DUMP=<folder>` (the drops as raw numbers, for a closer look outside the
    test) and prints `Spacing`; a new ignored `pace` test times a scene with no read-back until the end
    (`WATER_SCENES`, `WATER_CELLS`, `WATER_SECONDS`): wall seconds a second of scene.

13. `probe::ragged(drops, layout, reach) -> (f32, f32)`: for each drop, how far the drops within `reach`
    pixels lie off the line through them (the lesser spread of that little cloud): the mean and the value nine
    tenths are under. `look` prints it for the Water Weird layout's film over the disc and its thread.
    `measure.rs` also takes `WATER_SHIFT` (the grid moved by a part of a cell), `WATER_SPRAY`, `WATER_SETTLES`.

## Not cured in this part: barbs at Extreme

At 3 px cells the film is 1.6 cells thick where it meets the disc nearly head-on. Seen step by step
(`/tmp`-scale close-ups, 8 steps apart): a small knot of slow drops sits where the film lands, grows, and is
carried off up the disc as a bulge about every 25 thousandths of a second; each bulge is thicker than the film,
so its drops leave the disc's curve at different places and it opens into a short row across the film: a barb.
The thread below the disc is whole and mostly clean; the film over the top of the disc carries a barb every
30 to 40 px. Tried without effect on the barbs (how ragged the film reads stays 2.7 to 3.5 px): `fill` 1, 0.75,
0.5; the over-full push off, gentle everywhere, strong everywhere; six multigrid cycles; the pull off; the
along-flight evening off; 480 steps; elbow room for slow drops too. Tried and worse: giving thin fast liquid
back the speed it loses where a solid turns it (more splash, the thread ragged too). At 8 and 6 px cells the
film is thinner than a cell, each drop is turned by the solid alone, and there are no barbs. So the fault is
in how the pressure handles a film one to three cells thick that is stopped head-on, and it is the next thing
to design, not a number to tune.

## Things not to do

- Do not edit today's Liquid code or its births, even to fix its fan: Balls scenes do not change.
- Do not give thin liquid back the speed a solid's turn costs it (tried: worse).
- Do not gate the elbow room on "thin" by a density measure: a two-cell film on a wall reads as full liquid by
  every kernel wide enough to be smooth, and a pool's top reads as thin. The gate is the drop's speed.
- Do not turn the elbow room into a push on speeds, or let it act on still water.
- Do not hide lumps in the picture: the bridge and today's Look defaults stay as they are for both cores.
- Do not hand-pick moments for the films: whole runs from the start, and the frames at 3 s and 6 s.
- Do not wait in a behaviour test on a number tuned to one card: bounds with room.

## Tasks

### Task 1: elbow room

**Files:** modify `crates/bpe-water/src/shaders/{common,hold,drops}.wgsl`, `src/sim/{mod,numbers,passes}.rs`,
`src/probe.rs`; tests in `crates/bpe-water/tests/{core,streams}.rs` and `src/sim/mod.rs` (unit).

**Produces:** `Numbers::{apart, apart_reach, apart_most, apart_pace}`; `StepGpu.r`; `probe::Spacing`;
`sim::spots_bytes(cells: u64, limit: u64) -> Option<u64>`; `Water::set_numbers(&mut self, Numbers)`.

- [ ] Tests first (they fail or do not build):
  - `streams.rs::drops_on_the_move_keep_apart`: `scenes::weird(8.0)` at `Numbers::default()`, 1.2 s; the drops
    faster than 300 px/s: `Spacing.close < 0.15` and `Spacing.median > 0.3`. (Without elbow room: about 0.6
    and 0.2.)
  - `core.rs::still_water_is_left_alone`: `still_box(8.0, 0.3)` with `apart: 0.0` to 0.5 s (the fill's own
    drops find their places in the first tenth of a second, some at 100 px/s); a copy is given the defaults
    (`Water::set_numbers`, new); both play to 2 s: `probe::print_of` equal.
  - `streams.rs::two_drops_on_one_spot_part_without_a_kick`: an empty tall box, two drops born on one point at
    600 px/s down; after 0.25 s they are over 0.3 cell apart and each one's speed is within 0.5 % of the other's.
  - `core.rs::a_heap_on_one_spot_spreads_without_a_burst`: 400 drops born on one point at 600 px/s down in the
    open air of a tall box; after 0.2 s: none broken, they span over 5 and under 30 cells (100 cells of liquid
    are 10 across at rest), under a fifth have another within a quarter cell, and none is over 500 px/s faster
    than a free fall (1,400 px/s). (With no elbow room they stay on the one point for good: drops at one place
    gather the same speed.)
  - `core.rs::drops_at_the_grids_edge_are_safe`: drops born at grid places (0, 0), (cols - 1, rows - 1) and just
    past both, without the `STOPS` bit, among a pour; 60 steps: the count is kept, the pour's drops are finite.
  - `core.rs::no_elbow_room_still_runs`: `apart: 0.0` on `weird(8.0)` for 0.3 s: runs, drops finite.
  - `sim/mod.rs` unit `spots_fit_or_are_left_out`: `spots_bytes(1000, 1 << 27) == Some(48_000)`;
    `spots_bytes(3_000_000, 1 << 27) == None`.
- [ ] Build decisions 1 to 6 and 11's `Spacing`. `passes.rs::ENTRIES`: binding 12 on `cs_clear`, `cs_hold_p2g`,
      `cs_g2p`.
- [ ] Run `cargo test --release -p bpe-water --test core --test streams -- --test-threads=2` and
      `cargo test -p bpe-water --lib`. Passing: every test, 2a's unchanged.
- [ ] Commit.

### Task 2: the pour's fan

**Files:** modify `crates/bpe-water/src/sim/drop.rs`, `crates/bpe-kit/tests/water_core.rs`.

- [ ] Test first, `drop.rs` unit `a_pour_fans_out_smoothly`: a Pour aimed 90 with Spread 10: `fanned(-0.5)`,
      `fanned(0.0)`, `fanned(0.5)` are at 80, 90 and 100 degrees (to 1e-4); `fanned(3.0) == fanned(0.5)`; with
      Spread 0 every drop of `drops(...)` has the same way; with Spread 45 and width 0 every drop's way is
      finite and within 45 degrees of Aim; `Pour::default().spread == 0.0`.
- [ ] Build decisions 7 and 8.
- [ ] Run `cargo test -p bpe-water --lib` and build `cargo test --release -p bpe-kit --test water_core --no-run`.
- [ ] Commit.

### Task 3: steps by detail, the measuring tools

**Files:** modify `crates/bpe-water/src/sim/numbers.rs`, `src/probe.rs`, `tests/measure.rs`, `tests/streams.rs`,
`crates/bpe-kit/tests/water_core.rs`.

- [ ] Tests first: `numbers.rs` unit `finer_cells_step_more_often`: `for_cell` of 12, 8, 6, 3 gives 240, 480,
      480, 960; of 0.5 gives 1920; of 0 or a NaN gives 240; nothing else differs from the defaults.
      `streams.rs::a_film_leaves_a_block_even`: `weird(6.0)` at `Numbers::for_cell(6.0)`; at ten moments from
      1.0 s to 1.9 s, `probe::along` over the 78 px after the block's left corner along the block's top line
      (reach 30 px, 13 bins of 6 px): the mean over the moments of (thinnest bin / thickest bin) is over 0.3.
      (Without this part: about 0.17; with it about 0.5.)
- [ ] Build decisions 9, 11 (`along`) and 12; `water_core.rs` and `measure.rs` take `Numbers::from_env_for(cell)`.
- [ ] Run the stream tests and the lib tests. Commit.

### Task 4: on the owner's card

- [ ] Push the branch. A Remote Control session on `F:\!Ai Stuff\BallPhysicsEngine` (recipe: project memory
      `liquid-speed`): builds `bpe-kit`'s `water_core` test and `bpe-water`'s `core`, `streams` and `measure`
      tests for `x86_64-pc-windows-gnu` in the PC's `bpe-ci` box (scratch folder), runs them natively from
      `%TEMP%` once no check is running, and pushes texts, frames at 3 s and 6 s and clips under `card/` as a
      `[skip ci]` commit.
- [ ] Runs: `core` and `streams` (the card's own pass or fail); `side_by_side` on the owner's file at Fine and at
      Extreme, both cores, 6 s at 30 frames a second, 1080 x 1920, with `WATER_DOTS`; the same with `WATER_FAN=1`
      (for the record); `measure look` on `box,deep,course` at 8, 6 and 3 px (pools at rest, their height);
      `measure pace` on `weird` at 8, 6 and 3 px (the speed).
- [ ] Look at the full-size frames and their 1:1 crops (lumps, hairs, ribs, flicker between frames, the pool)
      before anything is called reached. If the thread is not whole at a detail, try that detail at the next
      step rate up and record both.

### Task 5: notes, review, merge

- [ ] `docs/notes/water.md`: "Part 2a-2: thin water keeps its spacing (as built)"; this file's "As built and
      measured"; `git rm -r card/`.
- [ ] `cargo fmt --all`; `cargo clippy -p bpe-water -p bpe-kit --all-targets -- -D warnings`.
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Pull request out of draft, checks green, merge. Films to the owner; project memory brought up to date.

## As built and measured

(Filled in as the tasks land.)
