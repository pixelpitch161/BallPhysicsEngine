# Water mode part 5: Moving parts (plan)

> **For agentic workers:** built natively in one thread (Aedan's project rule: one Opus review per part, no
> approval stops after the design). Steps use checkbox (`- [ ]`) syntax for tracking. Part 5 is three pull
> requests in this thread, each merged before the next is opened: **5a** moving solids in the water and
> **Keyframed** parts, **5b** **Driven** parts (the water pushes them: joints, Weight, Spring, Drag, Stops, Motor),
> **5c** buckets that **catch liquid** and **tip**. Parts 4 (more liquids) and 4b (forces and ferrofluid) are built
> side by side in other threads from the same main (086a7fa): keep every change scoped, and rebase on whichever
> merges first.

**Goal:** a Piece or Wall can move. A **Keyframed** one follows its Moves card's Position, Turn and Scale keys and
pushes the water as it goes (a paddle sweeping a pool, a gate lifting out of a channel, a wheel turned by its keys),
with its path drawn on the stage. A **Driven** one is moved by the water and gravity on a **Joint**: **Free**
(floats and crates that bob, drift, sink and knock into the course and each other), **Pin** (wheels, flaps and
see-saws that turn on a point), **Slide** (gates and pistons along a line), with Weight, Drag, Spring, Stops and a
Motor. A **bucket** catches the liquid that falls into it and tips once it is as full as Tips when says, pours, and
rights itself. Make final keeps where every part was, so scrubbing and the export show it, and remaking from a
restart point plays the parts on exactly.

**Architecture:** `bpe-water` gains `parts/`: a moving part's solid sampled once in its own frame
(`MoverShape`), its place, turn and size at a moment (`Pose`), the course split into what stands still and what
moves (`Movers`, with `field_at(poses)` composing the field on the processor for the drawing), and (5b) the
rigid-body solver. On the card a new pass lays every moving part over the still field at each step's end
(`cs_movers`), and the deep water's solids by cell are worked out again where they moved; a drop keeps none of
its speed into a face relative to the face's own speed, and drag acts on the speed along the face relative to the
face's. (5b) The push of the water on each driven part is summed on the card in whole numbers and read back with
the clock's report two frames later, and the processor steps the parts 240 times a second with that push and the
added mass of the water they shove. The kit reads the Moves card into those inputs; the drawing places compose the
field at the moment drawn from the parts' poses, which the take keeps for every moment.

**Tech Stack:** Rust, wgpu/WGSL (`bpe-water`), `bpe-kit`, egui 0.37 + kittest (`bpe-studio`), `bpe-cli`.

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` sections 3 ("Moving solids"), 4 (Parts that
move), 5 (what a take holds, What an edit costs), Parts item 5, "The water checkpoint"; the all-drops core design
`2026-11-25-all-drops-water-core-design.md` section 5 ("Room for moving solids") and 11 (Repeats). Hand-over:
`/mnt/project-files/ballphysics/water-mode/part4-5-4b-handover.md`.

## Global Constraints

- Balls scenes byte-for-byte unchanged: no Balls golden or pinned picture moves.
- **A course with no moving part plays as before**: the step's passes, their order and the joined finish and fall
  are unchanged, the new branches sit behind `fr.mv.x > 0u` (how many parts move) and nothing is composed. The pinned
  water pictures (drawn from stored takes) don't move.
- **Liquid first** (spec section 4): the liquid's grid, its method and its numbers never change because a part is
  there. Parts are sampled onto the liquid's grid; a part thinner than two cells is thickened for the physics only
  (the THIN note stays). The only thing a part can change is the number of steps a frame takes, and only upward, by
  the same rule fast water already follows (decision 7).
- **Repeats** (all-drops design section 11): the same card, driver and program give the same water and the same
  parts' motion: twice from the start, from a copy at a frame boundary, and from a restart point. Every sum the
  parts read is a whole-number sum; the working field after a restart is composed by the card exactly as during the
  first run (never uploaded from the processor's twin).
- **Nothing that only changes a look restarts the draft.** The Moves card does (it is physics now).
- Physics stays out of `look.rs`/`looks.rs`; new physics files sit under the print roots the two `build.rs` hash
  (`bpe-water/src/parts/` is added to bpe-water's roots; the kit's `things/water/draft/movers.rs` is already under
  its roots).
- Up to **64 moving parts** a course (`MOST_MOVERS`, spec section 4); past that a part stays put with a note.
- Editor colours only from `style/` tokens; controls named for kittest; no `ComboBox`; no glyphs the default fonts
  lack.
- `bpe-water` may use platform maths (it never feeds the balls' physics); the kit's code stays on `bpe_core::maths`.
- Windows-friendly; cross-built with MinGW; no exe built or installed; Aedan never runs commands.
- Tests: only what part 5 adds or changes. GPU tests through `bpe_gfx::test_device()`, small courses (draft cells,
  a few hundred px). Speed numbers told to Aedan as facts come from the card job on the RX 9070 XT.

## Review Focus

1. **A part pushed into the rock or into another part** (a keyframed gate sliding into a slot, two paddles
   crossing): the two solids join (the field takes the nearer face), no water leaks through either, and the drawing
   shows one body where they overlap. Test: 5a Task 4 `a_paddle_into_the_rock_lets_no_water_through`.
2. **A part moving faster than the water** (a bar spinning two turns a second): the frame takes enough steps that no
   point of the part moves more than `most_move` spacings a step, so no drop ends up inside it or on its far side.
   Test: 5a Task 4 `a_fast_turning_bar_keeps_the_water_out`.
3. **Water shut in by a keyframed part** (a lid closing on a full box): the part keeps to its keys, the drops it
   overlaps are moved out by the field's slope, nothing goes to NaN, the count is kept and no drop flies off faster
   than the speed cap. Test: 5a Task 4 `a_lid_closing_on_water_keeps_every_drop`.
4. **Scale keyed small** (0.1): its thin walls are still at least `LEAST_WALL` cells thick to the water (thickened by
   the smallest Scale it takes). Test: 5a Task 1 `a_shape_keyed_small_keeps_its_walls_thick`.
5. **Slow motion and an edit to a key**: a moment kept inside a frame shows each part at its pose then (interpolated
   as the physics moved it), and moving one key remakes the final only from the key before it. Tests: 5a Task 5
   `a_moment_inside_a_frame_keeps_the_parts_between`, 5a Task 6 `a_moved_key_changes_the_water_from_the_key_before`.

## What exists (read before building)

- `bpe-water/src/solid.rs`: `Layout`, `Solid` (Polygon, Circle, Line, Polyline), `Part { solids, cut, wetting }`,
  `SolidField { layout, distance (px), wetting, owner: Vec<u16> }`, `add_part(part, owner, least)` (cut, then add;
  a node is the part's where its face is nearer or the node is inside it; only nodes within `BAND` 48 px of the
  part's box change), `for_card()` (distances in cells clamped to ±64, then the wettings).
- `bpe-water/src/drops/mod.rs` `Water`: `new(gpu, &field, gravity, numbers, feels)` uploads `field.for_card()` into
  `solid` (binding 15) and runs `cs_deep_walls` once (`put_walls`); `frame()` → `settle`, `steps_now` (the fastest
  drop two frames back plus pours' speeds and two frames' fall, `Numbers::steps_for`), `play_frame(n, keep)`:
  births, then per step Predict (or Born after a joined FinishPredict), the sort, the deep part, `iters` sweeps,
  Finish or FinishPredict, Pack at cuts, the report and the clock's copy back. `copy()`, `state()`,
  `from_state(..)` (`drops/state.rs`, magic `BPESTAT1`).
- Shaders: `common.wgsl` (`Fr`, `St` 16 bytes at `STEP_ROOM` 256, `solid_at`, `keep_off(x, v, stops)` keeps no speed
  into a face, `wet_at` reads `solid[nodes + i]`), `sort.wgsl` `fall()` (sub-moves of `fr.w.z` each through
  `keep_off`), `sweep.wgsl` `apply_at` (the solids' share pushes, then `keep_off`; the last sweep's speed is
  `(x − was)/dt`), `finish.wgsl` `finish_at` (wetting pull, grip, drag along the face), `deep.wgsl`
  (`cs_deep_walls` per node from `solid`; `cellp.x` is the pressure × dt per unit density, `pressure_change`),
  `air.wgsl` (reads `wallc`, `solid_at`). `passes.rs` `ENTRIES` (at most 8 storage buffers a pass, tested).
- Kit: `modules/water/solids.rs` `MovesCard` (Moves Fixed | Keyframed | Driven; Position offset, Turn °, Scale,
  keyframed; Joint, Weight, Drag, Spring, Stops, Stop from/to, Motor, Catches liquid, Tips when), `PieceShape`,
  `WallLine`; `modules/water/pieces.rs` (`placed`, `PieceShape::parts/cut/hull`, `WallLine::solid`);
  `things/water/draft.rs` `WaterSetup` (`of`, `final_of`, `field`, `field_with`, `print`, `inputs_at`, `inputs_on`,
  `set_inputs`, `start`, `resume`, `fills`, notes `THIN`, `MOVES_LATER`); `draft/course.rs` `SolidSetup` (`own()`,
  `part()`, `moves()`, `to_frame/to_world`); `draft/inputs.rs` `WaterInputs` (parts with `from`, a fixed hash and
  keyframed settings' keys; `earliest_change`).
- Take: `bpe-water/src/take/frame.rs` `FrameCodec` (`encode(packed, cells)`, `decode(bytes, layout) -> TakeFrame {
  xv, kinds }`, magic `BPF1`), `drops/capture.rs` (`Moment { frame, steps, of }`, `Captured { moment, drops }`),
  `take/mod.rs` `TAKE_FORMAT` 2; the kit's `make.rs` encodes `Captured`s; `bpe-cli/src/water.rs` `TakeReader::near`
  gives `(TakeFrame, ahead)`.
- The drawing places: `bpe-studio water_mode/{draft,picture,final_view}.rs` (`Live { water, field, look, wet }`,
  `LookDraw::set_field`), `bpe-cli/src/water.rs` (`Drawing`, `Source::{Final, Draft}`), each from `setup.field()`.
- Editor: `water_mode/solid_edit.rs` (`paint` draws the picked solid's outline and handles at its place),
  `stage.rs` (notes for a picked thing), `app::water_tests` (what restarts the draft).

---

# Part 5a: moving solids, and Keyframed parts

## Decisions (5a)

1. **Which parts move.** A Piece or Wall whose Moves card says Keyframed, enabled, in the course: up to
   `MOST_MOVERS` (64) in the scene's order; any past that stays where it is with the note `TOO_MANY_MOVERS` ("The
   water moves at most 64 parts: this one stays where it is."). In 5a a Driven part stays where it is with the note
   `DRIVEN_LATER` ("Driven parts arrive in the next part of Water mode: this one stays where it is for now.",
   replacing `MOVES_LATER`); 5b removes it.
2. **The still field and the moving parts.** The course splits in two: the **still field** is the carved rock with
   every part that doesn't move laid over it in order, exactly as `field_with` builds the whole course today; each
   moving part is laid over the still field at its pose, after everything still, whatever its place in the tree
   **(My call: a moving part is never under a still one; among moving parts the field takes the nearer face, so
   their order only decides a tie)**. Owner numbers stay each part's place in `WaterSetup.solids` plus 1, so the
   drawing's colours don't move.
3. **A moving part never cuts** (My call). Its Cuts is ignored while it moves: a Piece moves as `shape.parts()` (its
   walls, or its filled shape), a Wall as `line.solid()`. The stage says `MOVING_CUTS` ("A moving part can't cut the
   course: it moves as a solid."). A moving tunnel through rock would need the rock to close behind it, which the
   still field can't.
4. **A moving part's shape** is sampled once, in its own frame (origin its place, unturned, Scale 1), at the body's
   cell: `MoverShape { origin, cell, cols, rows, distance (px), radius }`, covering its parts' box plus `BAND` (48 px)
   and one cell, thin walls taken as at least `LEAST_WALL × cell / least_scale` thick, `least_scale` the smallest
   Scale it takes (its base value and every key's, at most 1). Outside its grid it is `FAR`. `radius` is the
   furthest grid node from its origin (for its box at any turn).
5. **Pose** = `{ at: Vec2 (world px), turn: f32 (radians, clockwise as `transform.rotation`), scale: f32 }`. A
   Keyframed part at water time `t`: `at = transform.position + Moves.offset(t)`, `turn = transform.rotation +
   Moves.turn(t).to_radians()`, `scale = Moves.scale(t).clamp(0.01, 100)`; not finite → its place, turn 0, scale 1.
   The kit reads it at each frame's start and end (`k/60`, `(k+1)/60`); **within a frame the part moves evenly**
   (place, turn and scale each linear in the step's time) **(My call: keys are read once a frame, as every other
   keyed setting is)**. Its speed over the frame is `(end − start) / FRAME` (place, turn, and `ṡ/s` for scale).
6. **The card composes the working field each step a part moves** (`cs_movers`, before the step's Predict): over the
   frame's **moved box** (decision 8), each node starts from the still field's distance, Wetting and "still" (0), and
   each moving part at the step's end pose (`t = (j + 1) / n` of the frame) gives `d_k = scale × own(q)` with `q` the
   node in the part's frame; the node takes `min(d, d_k)` and becomes the part's (its Wetting, its number `k + 1`) by
   `add_part`'s rule (`d_k < d` or `d_k < 0`). Then `cs_deep_walls_moved` works out the deep water's solids by cell
   again over the box grown by 2 nodes. A frame in which no part moves composes nothing (the field is still right).
7. **Steps.** A frame in which parts move takes enough steps for their fastest point too: the fastest point's speed
   (`|v| + |ω| × radius × scale + |ṡ| × radius`, px a second, the largest over the parts) joins the fastest water's
   in `steps_now`, as a pour's speed does today. So no point of a part moves more than `most_move` spacings a step,
   and a thin wall moving fast can't step over a drop. (This only ever makes the water's step finer.)
8. **The moved box** (grid nodes, on the host each frame): the union, over the parts whose pose differs between the
   frame's start and end, of the box their circle sweeps (`[min(a, b) − R, max(a, b) + R]`, `R = radius ×
   max(scale)` + 1 cell), clipped to the layout. A part that moves leaves nothing behind it: the box covers where
   it was at the frame's start.
9. **The face's speed.** The solid buffer gains a third section: at each node, which moving part's face is nearest
   (`k + 1`, 0 for still), as f32. `solid_speed(q)` reads it at the nearest node and gives the part's point speed at
   `q` (grid units a second) from its speed over the frame: `v + ω × perp(q − c(t)) + (ṡ/s)(q − c(t))`, `c(t)` its
   place at the step's end, `perp(r) = (−r.y, r.x)` (clockwise turn, y down). `keep_off(x, v, stops)` keeps none of a
   drop's speed into a face **relative to the face's speed** (`vn = dot(v − vs, n)`; `vn < 0` → `v −= vn n`);
   `finish_at`'s drag acts on the speed along the face relative to the face's. Grip and Wetting are unchanged. With
   no part moving, `vs` is `vec2(0)` and the arithmetic is the same as today.
10. **No joined finish and fall while a part moves** (`numbers.join` is ignored for that frame): each step composes
    its field between the two.
11. **Where the drops start.** Fills at the start, taps' room and spouts in a solid are judged on the course as it
    stands at the start (`field()`, the still field with each moving part at its start pose). A tap's spout that a
    part later passes over goes on pouring (its births are kept off the solid by the step as any drop is).
12. **Trapped water** (spec section 3): a Keyframed part keeps to its keys; drops it overlaps are moved out along
    the field's slope by `keep_off` (at most 4 cells a call, as now). No trouble mark yet (part 7 is Later).
13. **The drawing** composes the same field on the processor at the moment drawn, `Movers::field_at(poses)` (the
    shader's sampling in Rust: the same grid, the same bilinear read; it need not match the card to the bit), and
    gives it to `LookDraw::set_field` (and the plain drawing) whenever the poses differ from the last drawn. The
    light maps are drawn again every picture, so shadows follow.
14. **The take keeps the poses.** Every kept moment stores each moving part's pose (`Captured.poses`, 16 bytes a
    part, after the drops in the moment's bytes; `FrameCodec` magic `BPF2`); a moment inside a frame keeps the poses
    at its cut (linear between the frame's start and end, as the physics moved it). `TAKE_FORMAT` goes to 3. A drawn
    moment `ahead` of the kept one moves each part on by its speed (as the drops are).
15. **Restart points and copies** keep the poses at the boundary (`WaterState.poses`); a body made from a state, and
    a fresh body, compose their working field **on the card** at the poses they start from (the same pass as a step,
    over the whole layout) and work out the deep water's solids, so a remade final plays on to the bit.
16. **The print and what an edit remakes.** The print hashes, for each Piece and Wall, whether it moves; for a moving
    one also its Moves card's `moves`, `offset`, `turn`, `scale` and their keys (not the Driven settings, which 5b
    adds). `WaterInputs` gains a part `mover:<k>` per moving part (from 0, its base place and turn and shape in the
    fixed hash, `offset`/`turn`/`scale` keys described), so a moved key remakes from the key before it.
17. **The stage** draws a picked moving part's **path**: its place every 1/30 s of water time over the scene's
    Length, as a thin line (`style::TEXT_SOFT`), a dot at every key of Position (`style::ACCENT`), and its outline
    where it is at the playhead (`style::TEXT_STRONG`, the handles stay at its base place: dragging moves the whole
    path). Its notes (`DRIVEN_LATER`, `MOVING_CUTS`, `TOO_MANY_MOVERS`) show as today's notes do.
18. **The carve preview** (painting a stroke, dragging a piece) shows moving parts at their start pose (known: the
    preview doesn't follow the playhead).

## File structure (5a)

- Create `crates/bpe-water/src/parts/mod.rs` — `MOST_MOVERS`, re-exports; `crates/bpe-water/src/parts/pose.rs` —
  `Pose`; `crates/bpe-water/src/parts/shape.rs` — `MoverShape`; `crates/bpe-water/src/parts/movers.rs` — `Mover`,
  `Movers`, `field_at`, `moved_box`, the card's layout of a mover (`MoverGpu`).
- Create `crates/bpe-water/src/drops/movers.rs` — the body's side: the still and shapes buffers, the frame's moving
  numbers, the compose dispatch (`Water::set_movers`, `move_to`, `poses`).
- Create `crates/bpe-water/src/drops/shaders/movers.wgsl` — `cs_movers`, `cs_deep_walls_moved`, `solid_speed`.
- Modify `crates/bpe-water/src/drops/{mod.rs,passes.rs,state.rs,capture.rs}`, `shaders/{common,sort,finish,deep}.wgsl`,
  `crates/bpe-water/src/{lib.rs,solid.rs}` (`for_card` third section), `src/take/{mod.rs,frame.rs}`, `build.rs`
  (print roots gain `src/parts`).
- Create `crates/bpe-kit/src/things/water/draft/movers.rs` — `WaterSetup::movers`, `poses_at`, the still field,
  notes; modify `draft.rs`, `draft/course.rs`, `draft/inputs.rs`, `make.rs`.
- Modify `crates/bpe-cli/src/water.rs`, `crates/bpe-studio/src/water_mode/{draft,picture,final_view,solid_edit}.rs`.
- Tests: `crates/bpe-water/tests/movers.rs` (new), `bpe-water/src/parts/*` units, `bpe-water/tests/take.rs`,
  `bpe-kit/src/things/water/draft/tests.rs`, `bpe-studio/src/water_mode/solid_tests.rs`, `app::water_tests`.

## Tasks (5a)

### Task 1: Pose and MoverShape

**Files:** create `bpe-water/src/parts/{mod,pose,shape}.rs`; modify `bpe-water/src/lib.rs` (`pub mod parts;`
re-export `Pose`, `MoverShape`, `MOST_MOVERS`), `bpe-water/build.rs` (hash `src/parts`).

**Interfaces — Produces:**
- `pub const MOST_MOVERS: usize = 64;`
- `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Pose { pub at: Vec2, pub turn: f32, pub scale: f32 }` with
  `Default` (`at` 0, `turn` 0, `scale` 1), `pub fn to_world(&self, own: Vec2) -> Vec2`, `pub fn to_own(&self, world:
  Vec2) -> Vec2`, `pub fn lerp(a: Pose, b: Pose, t: f32) -> Pose`, `pub fn sane(self) -> Pose` (non-finite → default
  values, scale clamped 0.01..100).
- `pub struct MoverShape { pub origin: Vec2, pub cell: f32, pub cols: u32, pub rows: u32, pub distance: Vec<f32>,
  pub radius: f32 }`, `pub fn of(solids: &[Solid], cell: f32, least: f32) -> Option<MoverShape>` (`None` when no
  solid is sound), `pub fn at(&self, own: Vec2) -> f32` (bilinear; `SolidField::FAR` outside the grid).

- [ ] **Step 1: Write the failing tests** (in `shape.rs` / `pose.rs`):
  - `a_pose_turns_clockwise_and_back`: `Pose { at: (10, 20), turn: FRAC_PI_2, scale: 2 }.to_world((1, 0))` is
    `(10, 22)` within 1e-4; `to_own(to_world(p)) == p` within 1e-4 for three points; `lerp` at 0.5 is the mean.
  - `a_shape_reads_its_solids_distance`: a 100 × 40 filled rect at cell 4, least 6: `at(p)` within 0.6 px of
    `Solid::distance(p, 6)` at (0,0), (45,0), (60,0), (0,-30), (70,40); `at((400, 0))` is `FAR`; `radius ≥ 50 + 48`.
  - `a_shape_keyed_small_keeps_its_walls_thick` (Review Focus 4): a 2 px Polyline wall at cell 8 built with `least =
    1.5 × 8 / 0.1` reads `≤ −0.5 × 120 + 1` on its middle line.
  - `nothing_sound_makes_no_shape`: `MoverShape::of(&[], 4, 6)` and a two-point polygon give `None`.
- [ ] **Step 2: Run** `cargo test -p bpe-water --lib parts` — fails (no module).
- [ ] **Step 3: Implement** as the interfaces say: the grid covers the sound solids' `bounds(least)` plus `BAND`
  and a cell each way, nodes at `origin + (x, y) × cell`, distance `min` over the solids (`FAR` capped), `radius` the
  furthest grid corner from (0, 0).
- [ ] **Step 4: Run** the same — passes.
- [ ] **Step 5: Commit** "Water part 5a: a moving part's pose and its shape in its own frame".

### Task 2: Movers and the field at a moment (the processor's twin)

**Files:** create `bpe-water/src/parts/movers.rs`; modify `bpe-water/src/solid.rs`.

**Interfaces — Consumes:** Task 1. **Produces:**
- `pub struct Mover { pub shape: Arc<MoverShape>, pub wetting: f32, pub owner: u16 }`.
- `#[derive(Clone)] pub struct Movers { pub still: Arc<SolidField>, pub movers: Vec<Mover> }` with
  `pub fn field_at(&self, poses: &[Pose]) -> SolidField` (still, then each mover `k` at `poses[k]`; a missing pose is
  its default), `pub fn moved_box(&self, from: &[Pose], to: &[Pose]) -> Option<(UVec2, UVec2)>` (decision 8, nodes,
  inclusive; `None` when nothing moves), `pub fn fastest(&self, from: &[Pose], to: &[Pose], seconds: f32) -> f32`
  (decision 7, px a second).
- `SolidField::for_card()` gains a third section of `0.0` per node (the moving part nearest); `SolidField::nodes_on_card
  = 3 × nodes` used by `Water::new`'s buffer.

- [ ] **Step 1: Write the failing tests** (in `movers.rs`):
  - `a_mover_at_its_place_lays_as_a_part`: an open 400 × 400 field at cell 4 with a rock floor; a ramp `Part`
    added with `add_part(.., 2, 6)` vs. `Movers { still, [Mover of the same solids, owner 2] }.field_at(&[Pose {
    at, 0, 1 }])` with the solids in the ramp's own frame: distances within 0.6 px at every node with `|d| <
    BAND`, the same sign everywhere, the same owner and Wetting at every node more than a cell from a face.
  - `a_moved_mover_leaves_nothing_behind`: the same mover moved 100 px right: every node farther than `radius + 48`
    from both places equals the still field to the bit (distance, Wetting, owner); the node at its old middle is
    the still field's.
  - `the_moved_box_covers_where_it_was_and_is`: a turning bar's box holds both circles; equal poses give `None`.
  - `the_fastest_point_counts_the_turn`: a bar of radius 100 turning a quarter turn in 1/60 s at rest place gives
    about `100 × (π/2) × 60` px/s within 2 %.
- [ ] **Step 2: Run** `cargo test -p bpe-water --lib parts::movers` — fails.
- [ ] **Step 3: Implement.** `field_at` clones the still field and, for each mover, visits only the nodes of its
  circle's box: `d_k = pose.scale × shape.at(pose.to_own(p))` (`to_own` undoes the place, the turn and the scale),
  and takes it by `add_part`'s rule (decision 6). Fix every caller of `for_card` that slices by `nodes`
  (none read past the second section today; `Water::new` sizes its buffer from it).
- [ ] **Step 4: Run** — passes; `cargo test -p bpe-water --lib solid` still passes.
- [ ] **Step 5: Commit** "Water part 5a: the still field and moving parts, composed at a moment".

### Task 3: The card lays moving parts each step

**Files:** create `bpe-water/src/drops/movers.rs`, `bpe-water/src/drops/shaders/movers.wgsl`; modify
`drops/{mod,passes}.rs`, `shaders/{common,sort,finish,deep}.wgsl`.

**Interfaces — Consumes:** Task 2. **Produces:**
- `Water::set_movers(&mut self, movers: Movers, poses: &[Pose])` — keeps them, uploads the still field (binding 30,
  `still`: distances in cells, then Wetting, as `for_card`'s first two sections) and the shapes (binding 31,
  `shapes`: every grid's distances in cells, back to back), then composes the working field on the card over the
  whole layout at `poses` and works out the deep water's solids over it (decision 15). Empty `movers.movers`: a
  no-op.
- `Water::move_to(&mut self, ends: &[Pose])` — where each moving part is at the coming frame's end (from the next
  frame; the start is where it is now).
- `Water::poses(&self) -> &[Pose]` — where each is now (the boundary the body is at).
- `Water::movers(&self) -> Option<&Movers>`.
- WGSL: `Fr` gains, **at its end** (part 4b appends there too: whoever merges second puts their block after the
  other's), `mv: vec4<u32>` (parts moving this frame, 0, 0, 0), `mbox: vec4<u32>` (the moved box: x0, y0, x1, y1
  nodes, inclusive) and `movers: array<Mover, 64>` with `struct Mover { a: vec4<f32> /* start: place x, y (grid),
  turn, scale */, b: vec4<f32> /* end */, e: vec4<f32> /* speed x, y (grid a second), turn a second, ṡ/s */, g:
  vec4<f32> /* shape origin x, y (cells, own frame), Wetting, 0 */, h: vec4<u32> /* cols, rows, where its
  distances start in shapes, 0 */ }`. `St` gains `t: vec4<f32>` (x: the step's end as a share of the frame), the
  step binding's size and `min_binding_size` go to 32.
- `solid_speed(q: vec2<f32>) -> vec2<f32>`, `keep_off(x, v, stops)` with the face's speed (decision 9).
- Passes: `P::Movers` (`cs_movers`, bindings `[0, 1, 15, 30, 31]`, workgroup 16 × 16 over the moved box) and
  `P::DeepWallsMoved` (`cs_deep_walls_moved`, `[0, 1, 15, 20]`, 16 × 16 over the box grown by 2). `cs_deep_walls`
  and the moved pass share `fn walls_at(k: u32)`.

- [ ] **Step 1: Write the failing test** (`bpe-water/tests/movers.rs`, GPU): `a_still_mover_is_where_it_is` — a
  body on an open field with one mover (a 60 × 60 block) set at a pose: `Water::read`-free check through a
  measuring helper `Water::solid_field_now(&self) -> Vec<f32>` (tests only, `#[doc(hidden)] pub`, waits): its
  distances equal `movers.field_at(poses).for_card()`'s within 0.01 cells and the third section is 1.0 inside the
  block's band and 0.0 far off. Then `move_to` a pose 40 px right, one `frame()`: the read field now matches
  `field_at` at the new pose (same bounds), and far nodes are the still field's to the bit.
- [ ] **Step 2: Run** `cargo test -p bpe-water --test movers -- --test-threads=2` — fails.
- [ ] **Step 3: Implement** the body's side in `drops/movers.rs` (a `MovingParts` struct the body holds: movers,
  poses now, ends, the two buffers, the frame's `Fr` numbers), the WGSL (decisions 6 and 9; the moved pass's nodes
  `mbox.xy + gid.xy`, beyond `mbox.zw` returns), `play_frame`: when parts move this frame, `Movers` then
  `DeepWallsMoved` before each step's Predict at `st.t.x = (j + 1) / n`, joined passes off (decision 10); the step
  records gain `t`; at the frame's end the poses now become the ends. `steps_now` takes `Movers::fastest`
  (decision 7). `copy()` shares the still and shapes buffers (they never change) and copies the working field and the
  poses.
- [ ] **Step 4: Run** the test — passes. `cargo test -p bpe-water --test engine -- --test-threads=2` still passes
  (no movers: unchanged), and `passes::tests` (at most 8 storage buffers a pass).
- [ ] **Step 5: Commit** "Water part 5a: the card lays moving parts over the course each step".

### Task 4: Moving parts push the water (behaviour)

**Files:** `bpe-water/tests/movers.rs`; fixes in the shaders if a test finds one.

**Interfaces — Consumes:** Task 3 (`set_movers`, `move_to`, `poses`). A helper in the test file:
`fn course(cell) -> (Layout, SolidField)` (a 600 × 400 box of rock with an open inside), `fn plate(w, h) ->
Movers`, `fn play(water, movers, keys: impl Fn(f64) -> Pose, seconds)` (calls `move_to(keys((k+1)/60))` then
`frame()` each frame).

- [ ] **Step 1: Write the failing tests** (8 px cells; `test_device()`):
  - `a_paddle_pushes_the_water`: a pool 100 px deep; a 20 × 140 plate keyed from x −200 to x +150 over 1.5 s:
    after 2 s the drops' mean x has moved right by more than 30 px, no drop is more than half a cell inside any solid
    (field at the final pose), and the count is kept.
  - `a_gate_lifted_lets_the_water_through`: water held left of a 20 px gate that closes a channel, the gate keyed up
    by 200 px between 1.0 and 1.5 s: at 1.0 s under 1 % of the drops are right of the gate, at 4 s more than 20 %.
  - `a_fast_turning_bar_keeps_the_water_out` (Review Focus 2): a 160 × 16 bar turning two turns a second in the pool:
    after 2 s no drop is more than half a cell inside any solid, the count is kept, and `steps_last_frame()` is at
    least the steps `Numbers::steps_for` asks for the bar's tip speed.
  - `a_paddle_into_the_rock_lets_no_water_through` (Review Focus 1): a plate keyed into the rock wall that holds the
    pool: no drop ever on the rock's far side.
  - `a_lid_closing_on_water_keeps_every_drop` (Review Focus 3): a full 120 × 120 box, a lid keyed down 40 px into it
    over 0.5 s: every drop's place and speed finite, the count kept, no drop faster than the speed cap.
  - `moving_water_repeats`: the paddle twice → the same drops to the bit; `state()` at frame 40, `from_state` +
    `set_movers(movers, &state.poses)` → frame 100 equals the first run's to the bit; `copy()` at frame 40 likewise.
- [ ] **Step 2: Run** — the repeat test fails on the missing `WaterState.poses` (Task 5 adds it: write it now and
  mark it with the state there); the others fail or pass by what Task 3 built.
- [ ] **Step 3: Fix** what fails, keeping to the decisions (a fix in the liquid's numbers is not allowed).
- [ ] **Step 4: Run** `cargo test -p bpe-water --test movers -- --test-threads=2` — passes.
- [ ] **Step 5: Commit** "Water part 5a: moving parts push the water, tested".

### Task 5: Restart points and the take keep the poses

**Files:** modify `bpe-water/src/drops/{state,capture}.rs`, `src/take/{mod,frame}.rs`, `bpe-kit/src/things/water/make.rs`
(writer), `bpe-water/tests/take.rs`.

**Interfaces — Produces:** `WaterState.poses: Vec<Pose>` (written after the pours; `BPESTAT2`); `Water::from_state`
keeps them for `set_movers`; `Captured.poses: Vec<Pose>`; `FrameCodec::encode(&mut self, packed: &[[u32; 2]], cells:
u32, poses: &[(Pose, Pose)]) -> Vec<u8>` (each part's pose and its speed: place, turn and scale a second);
`TakeFrame.poses: Vec<(Pose, Pose)>` and `TakeFrame::poses_at(&self, ahead: f64) -> Vec<Pose>` (each moved on by its
speed); `Captured.poses: Vec<(Pose, Pose)>`; `TAKE_FORMAT = 3`.

- [ ] **Step 1: Write the failing tests:** `take::frame` `a_frame_keeps_its_parts_poses` (encode then decode gives
  the poses and their speeds back to the bit; no parts → empty); `tests/take.rs`
  `a_moment_inside_a_frame_keeps_the_parts_between` (Review Focus 5: `frame_keeping(4, ..)` with a mover keyed
  across the frame: the inside moments' poses are the lerp at `steps / of`); `state_round_trips_the_poses`.
- [ ] **Step 2: Run** `cargo test -p bpe-water --lib take && cargo test -p bpe-water --test take -- --test-threads=2`
  — fails.
- [ ] **Step 3: Implement**; the make's writer passes `c.poses` to `encode`.
- [ ] **Step 4: Run** — passes; Task 4's `moving_water_repeats` passes.
- [ ] **Step 5: Commit** "Water part 5a: restart points and the take keep where the parts are".

### Task 6: The kit reads Keyframed parts

**Files:** create `bpe-kit/src/things/water/draft/movers.rs`; modify `draft.rs`, `draft/course.rs`,
`draft/inputs.rs`, `draft/tests.rs`.

**Interfaces — Consumes:** Tasks 2, 3, 5. **Produces:**
- `SolidSetup::motion(&self) -> MovesKind`, `SolidSetup::pose_at(&self, time: f64) -> Pose` (decision 5),
  `SolidSetup::moving_part(&self) -> Option<Vec<Solid>>` (decision 3, own frame), `SolidSetup::least_scale(&self) ->
  f32`.
- `WaterSetup.movers: Vec<usize>` (indices into `solids` that move, at most 64), `WaterSetup::still_with(&self, mask:
  &CarveMask) -> SolidField`, `WaterSetup::movers_on(&self, still: SolidField) -> Option<Movers>` (at the setup's
  cell), `WaterSetup::poses_at(&self, time: f64) -> Vec<Pose>`; `field()`/`field_with(mask)` become the still field
  composed with the movers at `poses_at(0.0)` (unchanged when nothing moves).
- `start(..)` and `resume(..)` give the body its movers (`set_movers` at `poses_at(0)` or `state.poses`);
  `set_inputs` calls `water.move_to(&self.poses_at(f64::from(water.frames() + 1) / 60.0))`.
- Notes: `DRIVEN_LATER`, `MOVING_CUTS`, `TOO_MANY_MOVERS` (texts in decisions 1 and 3); `MOVES_LATER` goes.
- Print and inputs: decision 16.

- [ ] **Step 1: Write the failing tests** (`draft/tests.rs`):
  - `a_keyframed_piece_follows_its_keys`: a Piece keyed Position (0,0) at 0 s → (100, 0) at 1 s, Turn 0 → 90:
    `poses_at(0.5)` is at its place + (50, 0) and a quarter turn's half within 1e-3.
  - `a_moving_piece_is_not_in_the_still_field`: its body's middle is open in `still_with`, solid in `field()`.
  - `the_moves_card_restarts_the_draft` (replaces the Moves line of `what_restarts_the_draft`): Fixed → Keyframed,
    a Position key added or moved, Turn and Scale each change the print; Weight, Joint, Motor on a Keyframed part
    don't; Moves on a Fixed part changes nothing but `moves`.
  - `a_moved_key_changes_the_water_from_the_key_before` (Review Focus 5): keys at 1, 2, 3 s; moving the 3 s key →
    `earliest_change` is 2 s; adding a key at 0.5 s → 0.
  - `notes_for_moving_parts`: Driven → `DRIVEN_LATER`; Keyframed with Cuts → `MOVING_CUTS`; the 65th Keyframed part
    → `TOO_MANY_MOVERS` and stays in the still field.
- [ ] **Step 2: Run** `cargo test -p bpe-kit --lib things::water` — fails.
- [ ] **Step 3: Implement**; the THIN note also looks at moving parts.
- [ ] **Step 4: Run** — passes (the `MOVES_LATER` test lines move to the new notes).
- [ ] **Step 5: Commit** "Water part 5a: Keyframed pieces and walls move in the draft and the final".

### Task 7: The drawing places draw parts where they are

**Files:** modify `bpe-studio/src/water_mode/{draft,picture,final_view}.rs`, `bpe-cli/src/water.rs`.

**Interfaces — Consumes:** Tasks 2, 5, 6. **Produces:** `Live.movers: Option<Movers>` (draft), `Live::field_now(&self)
-> Cow<SolidField>`; the Final view and `bpe-cli` keep `Option<Arc<Movers>>` built once per setup and compose
`field_at(frame.poses moved on by ahead)`; each calls `set_field` only when the poses differ from the last drawn.

- [ ] **Step 1: Write the failing test:** `bpe-cli` `water` test `a_final_frame_draws_a_moving_part_where_it_was`: a
  tiny Water scene (8 px cells, 1 s) with a keyframed block crossing the shot; `final_frame` at 0.2 s and 0.8 s:
  the block's colour fills the pixel at its place then and not at the other (one pixel each, read from the picture).
- [ ] **Step 2: Run** `cargo test -p bpe-cli water -- --test-threads=2` — fails (the block stays at its start).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — passes.
- [ ] **Step 5: Commit** "Water part 5a: the stage, the Final view and the export draw parts where they are".

### Task 8: The stage shows a moving part's path

**Files:** modify `bpe-studio/src/water_mode/solid_edit.rs`, `solid_tests.rs`, `app` water tests.

**Interfaces — Produces:** `solid_edit::path_of(studio: &Studio, id: ThingId) -> Option<(Vec<Vec2>, Vec<Vec2>)>`
(the place every 1/30 s over the Length, and its Position keys' places; `None` unless the part is Keyframed).

- [ ] **Step 1: Write the failing tests:** `a_picked_keyframed_piece_shows_its_path` (two Position keys 100 px apart
  over 1 s: the path starts and ends at the keys' places, holds 31 points per second of Length, and the key dots are
  the two places); `app::water_tests` `a_moves_edit_restarts_the_draft` (was "does not").
- [ ] **Step 2: Run** `cargo test -p bpe-studio water_mode -- --test-threads=2` — fails.
- [ ] **Step 3: Implement** (decision 17): the path drawn under the handles, the outline at the playhead's water time.
- [ ] **Step 4: Run** — passes.
- [ ] **Step 5: Commit** "Water part 5a: the stage draws a moving part's path".

### Task 9: Notes, look check, review, PR (5a)

- [ ] `docs/notes/water.md` "Part 5a: moving solids and Keyframed parts (as built)" (decisions as built, measured
  numbers, Known); this plan's "As built 5a".
- [ ] Full-size look check: a `water-moving` scene in `full_size_look.rs` (the pieces course with a keyframed paddle
  and a keyframed gate, 3 s): `LOOK_OUT=… LOOK_ONLY=water-moving cargo test --release -p bpe-kit --test
  full_size_look -- --ignored --nocapture`; look at the pictures and 1:1 crops for seams where a part meets the rock,
  a part's edge drawn off where the water stops, light leaks under a moved part.
- [ ] Timing: add a moving paddle to `water_final_speed`'s course? No: a separate `water_moving_speed` (card job) that
  reports the compose passes' share of a frame at 2 px; the number goes in the notes as measured on the card.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, the changed tests.
- [ ] One Opus review (whole branch); fix; PR (draft, then ready), merge when every check is green.

---

# Part 5b: Driven parts

## Decisions (5b)

19. **The solver** (`bpe-water/src/parts/solver.rs`): each Driven part is a rigid body on the processor, stepped
    **240 times a second** (4 substeps a frame) by semi-implicit Euler, before the frame is sent: its pose at the
    frame's start and end then go to the card as a Keyframed part's do (decision 6), so the card's side of 5a is
    used as it is. Mass = Weight × area (px², water weighs 1 a px²), centre of mass and turning inertia from its
    `MoverShape` (nodes inside, `d < 0`). The pose's `at` is its place; the body turns about its centre of mass
    (Free), its pin (Pin) or not at all (Slide).
20. **The water's push** on a driven part, summed on the card over the whole frame as whole numbers (fixed point,
    1/1024 cell, 32 lanes by drop number, so the same in any order), per part: the push in x and y and the turn
    about its centre of mass (at the step's pose), and how many drops touched it. Drops add: in `fall()`, the
    displacement each sub-move's `keep_off` gave; in `apply_at`, the solids' share of the shift and `keep_off`'s;
    in `cs_deep_push`, `keep_off`'s; in `finish_at`, the speed grip, Wetting's pull and drag changed × dt. A drop
    adds to the part whose face is nearest it (the field's third section). The deep water adds its pressure on the
    part's faces: for each cell next to the part's solid, `4 × cellp.x × dt × (1 − open share) × w` (w the cell's
    depth share) along each closed face's outward way. The reaction is the opposite of what the drops got.
21. **Read back two frames late, without waiting**: the sums ride in the clock's `Report` (a `movers` block after
    `spout`), reduced from the lanes by a pass after the frame's last step; frame `k` uses frame `k − 2`'s push
    (as the steps do). Its average force is `push × m_drop / (dt × FRAME)`, `m_drop = cell² / 4`.
22. **Added mass** (spec section 4): the part counts half its own area of water as its own (`C_a = 0.5`, My call,
    the same at every detail), with the late push corrected: `(m + m_a) a = F(k − 2) + m_a a(k − 2) + gravity and
    the joint's forces` (the same for turning, with `I_a = C_a × area × r_g²`). A part that still jitters is made
    heavier with a note, never by touching the liquid.
23. **Joints.** Free: x, y and turn. Pin: turns about **Pin at** (a new Vec2 setting, the part's own frame, px,
    default its place); the place stays where it was. Slide: moves along its own up–down (its frame's y axis, turned
    with its Rotation), no turn.
24. **Settings** (Moves card, Driven): Weight (0.01.., under 1 floats), Drag (speed lost: `exp(−5 × Drag × t)` on top
    of the water's own), Spring (pulled back to where it started: natural rate `2π × 1.5 × Spring` a second),
    Stops (Pin: Stop from/to in degrees from its start turn; Slide: **Travel from/to** in px, new settings), Motor
    (Pin: turns a second; Slide: **Motor speed** px a second, new), **Motor strength** (new, 0..1, default 0.5: how
    hard it drives toward its speed, `gain = 40/s × strength`, at most `strength × 50` rad/s² or px/s² times its
    mass's own). A stopped part keeps no speed past its stop (no bounce).
25. **Free parts meet the course and each other** at 240 Hz: points every cell along each one's outline (its
    shape's zero line) are tested against the still field and every other driven part's shape; a point inside is
    pushed out along the field's slope (position) and its speed into the face is taken away (restitution 0.2,
    friction 0.5). Keyframed parts push driven ones the same way (they are solids that move).
26. **State.** Each driven part's pose, speed, turn speed, last acceleration (for decision 22) go into
    `WaterState`; the clock's reports already carry the pushes in flight. `TAKE_FORMAT` 4.
27. **The print** gains every Driven setting (and Pin at, Travel, Motor speed and strength); `WaterInputs` describes
    the Motor's keys, so a Motor key remakes from the key before; every other Driven change remakes from 0.
28. **Editor**: the pin's dot and the slide's line (with its travel's ends) on the stage for a picked Driven part;
    a Driven part is drawn where the draft has it; the notes `DRIVEN_LATER` goes; `TOO_HEAVY_TO_FLOAT`? No: the
    water shows it.

## Tasks (5b, outline; each with its own failing test first)

1. `parts/solver.rs`: `DrivenSettings`, `DrivenState`, `Solver::step(&mut self, push: &[Push], frame_seconds) ->
   Vec<Pose>`; unit tests: a part in air falls at g (after the added mass's lag settles), a pinned bar with its
   centre off the pin swings and settles under Drag, a Spring brings it back, Stops hold, a Motor reaches its speed,
   a Slide keeps to its line, a Free crate rests on a still floor without sinking or jitter, two crates stack.
2. The push on the card: `push` buffer (binding 32) cleared each frame; the adds in predict, apply, deep push and
   finish (only for drops near a driven part's face: `solid[2n + node] > 0` and that part driven); the deep faces'
   pass; the reduce pass into the `Report`; a GPU test `the_water_pushes_a_held_plate`: a plate held still in a
   stream reads a push along the stream; a held block on a pool's floor under water reads its buoyancy within 25 %
   of `ρ g area` (thin water and the deep water together).
3. The body steps driven parts: `Water::set_driven`, the poses from the solver each frame; GPU behaviour tests:
   `a_float_floats` (Weight 0.5: about half under the water, within 20 % of its height, still within 2 px after 6
   s), `a_heavy_block_sinks`, `a_wheel_turns_in_a_stream`, `a_pinned_flap_lifts_under_a_pour`, `a_gate_on_a_slide_is
   pushed_open`, `driven_parts_repeat` (twice, from a copy, from a restart point).
4. State and take (format 4), kit (settings, print, inputs, notes), editor (pin dot, slide line), drawing (from the
   body / the take).
5. Notes, look check (`water-driven`: a float, a wheel and a gate), timing, review, PR.

---

# Part 5c: buckets that catch liquid and tip

## Decisions (5c, settled when 5c starts, against what 5b measured)

29. **Catches liquid** counts the drops inside the part's cup (a Bucket's or Basin's bowl, its `cut()` in its own
    frame, sampled as a second `MoverShape`), summed on the card with the push. The liquid inside is the part's
    own weight while it is in the cup: its mass at the drops' mean place, added to the part, and those drops' push
    left out of the part's push (so it is not counted twice).
30. **Tips when**: a pinned part that Catches liquid has its pin placed for it, at the height where the cup filled
    to Tips when balances, a little to the side it tips to (right; left when Flipped), so it stands while it fills,
    tips once it is that full, pours, and rights itself by its own weight once it has poured; its Stops are its rest
    and its furthest tip. Another joint: Tips when does nothing (a note says so).
31. Ready parts: a Bucket placed with Moves Driven gets Pin and Catches liquid on, a Float Free and Weight 0.5, a
    Wheel Pin, a Gate Slide (the palette's defaults, one undo step).

## Tasks (5c, outline)

1. The cup's count on the card; the caught liquid's weight in the solver; a GPU test `a_bucket_tips_when_full`
   (fills under a tap, tips between 0.7 and 0.9 full at Tips when 0.8, pours, rights itself, fills again).
2. Pin placement from Tips when; Stops as rest and tip; the editor's pin follows.
3. The palette's Driven defaults; notes; look check (`water-buckets`); review; PR; the hand-over for the next part.

---

## As built

### 5a (moving solids and Keyframed parts)

Built as planned (decisions 1 to 18; `docs/notes/water.md` "Part 5a" has the detail). Where it differs:

- **The take keeps each part's speed as well as its pose** (32 bytes a part, not 16), and a restart point keeps the
  speed each part moved at over its last frame (`Moving.speeds`, `WaterState.speeds`, `Water::resume_movers`): a
  moment captured just after a restart moves its parts on by the same speed as one captured on a body that played
  through, so a final remade from a key is the same byte for byte (`water_final.rs`
  `a_final_with_a_moving_paddle_remakes_from_the_key_before` failed at its second chunk without it).
- **A moving part's texture goes with it** (not in the plan): the look drawing read every material's texture at the
  world point, so a moving rock-textured block slid under its own veins. Each material now has a `texture` map, set
  for a moving part from its start pose (`Mover.start`, `Movers::textures`); `LookDraw::set_movers(movers, poses)` and
  `set_posed(field, textures)` replace the plan's "compose and `set_field`" in each drawing place, and the plan's
  `Live.movers` / `Live::field_now` weren't needed (the body keeps its movers: `Water::movers()`, `poses()`).
- **The stage path's colours**: the design system has no `TEXT_SOFT` or `ACCENT`, so the path is `MUTED` over
  `OUTLINE`, the key dots `TEXT_STRONG`, the part at its place `FAINT` and at the playhead `TEXT_STRONG`.
- **Timing**: `water_moving_speed` makes the one-screen chamber twice with a paddle, still and Keyframed, and prints
  the moving paddle's share of a scene second (its lines start `water final:`, so the card job's summary keeps them).
