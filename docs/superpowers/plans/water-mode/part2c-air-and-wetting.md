# Water mode part 2c: air and wetting (plan)

Spec: `docs/superpowers/specs/2026-11-24-water-mode-design.md`, "Parts", 2c (added at the water checkpoint,
2026-11-26, PR #237): "Air trapped under a plunging pour rises and breaks up instead of lingering as big pockets,
and each material gets a **Wetting** setting: on a waxy surface water beads up and rolls, on a wet one it spreads
into a film. Wetting, not a weaker skin bend, is what settles C2's tall shelf puddles." Engine: the all-drops core
(`2026-11-25-all-drops-water-core-design.md`, built in C1 #231, C2 #235, C3 #234), in the editor since 2b (#239).
Aedan's checkpoint picks: https://claude.ai/artifact/BJkTfdZBfoF1xtW3mg834q (items `air`, `puddles`, `wetting`,
all Keep). The design was approved there; lines marked **(My call)** are choices Aedan was not asked about.

Goal: real water. A jet plunging into a pool drags air under; real air breaks into bubbles that rise fast and
burst at the top, so a second after the pour stops the pool is clear. Real water on wax stands in round beads that
roll; on wet stone or clean glass it spreads into a thin film and low puddles.

## What is wrong today, and why (from C2's measuring and the code)

- **Air.** A pocket under a plunging pour is a hole with nothing in it. The drops round it rest at their own
  density, so nothing pushes into the hole and nothing holds it up either; and the deep part treats the hole as
  the liquid's edge (`cs_deep_cells`: an open cell no drop reaches beside one they do is an edge), so the water
  round it within 24 px gets no deep push. Only the roof's own weight closes it, slowly: at Fine one pocket was
  still there at 5.8 s, 2.8 s after the pour stopped (C2 Task 10). Real air in water is at the water's pressure
  and is light, so it rises.
- **Puddles.** The skin's normals (`lies` in `finish.wgsl`) are worked out from the liquid's drops only, so to the
  skin a solid is air. A puddle's bottom row then has a "surface" against the floor, and the bend term pushes a
  surface's drops outward: at the floor that pushes down on the rock and holds the rest of the puddle up. That is
  why the bend holds C2's shelf puddles 7 to 11 px tall (with `curv` 0 they fall to 2 to 5 px), and why every
  surface is a waxy one today.

## The design

### Air: drops that fall upward

**(My call)** Trapped air is made of **air drops**: drops like the water's, with the `AIR` bit (128) in their
kind, that take up room exactly as water does (they count in every drop's density and in the deep cells' volume)
but fall upward. So a pocket keeps its volume, carries the water's pressure at its depth, and rises; the water
closes in under it and round it; the pocket's skin is the water's own; when it reaches open air it bursts.

- **Falling upward.** An air drop's gravity is `−air_lift` times the course's (`air_lift` 1, the Water card's
  **Bubbles rise**). With every drop the same mass, an air drop feels the water's pressure push as a water drop
  does (one gravity up) plus its own lift, so a pocket's start matches real air in 2D: buoyancy one gravity on
  the water it displaces, against that water's inertia.
- **No skin of its own.** An air drop has no skin pull, no bend and no grip. A water drop leaves air drops out of
  its skin's sums (`lies`, the pull, the bend), so a pocket's wall is a surface to the water's skin: bubbles stay
  round and hold together. The evening of speeds (Thickness) and the landing's closing loss (`meet_even`) act
  between all drops, which is the bubble's drag.
- **Bursting.** An air drop whose neighbourhood is less than `air_pop` (0.75) full has open air beside it: it is let
  go (moved off the course in `finish`, so it sorts last and is counted out two frames later, as a drop that left
  the course is). Inside a pocket air drops are about 1 full; at a flat open surface a drop is about 0.67 full.
  So a bubble opens from the top a layer a step once its roof breaks.
- **Not drawn.** The plain drawing skips air drops (`vs_splat`, `vs_world`): a pocket still shows as a hole, as
  today, and air spread thin among the water is not seen. Bubbles as bright rings, fizz and foam are part 6's
  extras, which will draw them from these drops.
- **In the deep part** an air drop is gathered with water's weight (whatever its liquid), so the grid sees a
  pocket as full and moving: a pocket is no longer the liquid's edge, and the water round it gets the deep push.

**Where air drops come from (My call): the card finds trapped air each frame.** On the last step of every frame
that measures the deep cells (every frame with drops, as today), after `cs_deep_column` and before
`cs_deep_across` (which empties the cells' sums):

1. `cs_air_find`, a thread a cell. A cell is **trapped air** when it is open (under half solid), no drop reaches it
   (its volume sum is 0: no drop within a cell of its middle either way), and, looking out from it along its column
   and its row up to `air_reach` (48 px, at least 3 cells) each way, past empty and part-full cells:
   - **up** it meets a full cell of liquid (not rock, not the grid's edge): water closes over it;
   - **down, left and right** it meets a full cell of liquid or a solid (the course's edge counts as solid when
     the edges are shut).
   "Full" is the deep part's own test (the drops in it at least `deep_full` full, and they and the solids covering
   at least half of it). Spray, streams, sheets and films in open air are never full on every side, so air is never
   made in the open; a hole bigger than twice `air_reach` across is left as a hole (**My call**: real pockets under
   a pour are smaller, and the cap keeps a cave of air under a curtain of water from filling with drops).
   Each of the cell's four lattice spots (a quarter cell in from its corners, the resting lattice) that is at
   least `wall_off` from the rock and inside the course is marked. The cell also notes the liquid's speed round it
   (the mass-weighted mean of the face speeds of the 3 x 3 cells round it, from the sums).
2. `cs_air_sum_a`, `cs_air_sum_b`: the marked spots' running sums over the cells, in blocks of 512 then over the
   blocks (as the bins' sums), whole numbers, so every spot gets the same place in the births on every run. The
   total goes to the frame's report (`Report.spare`, now `air`).
3. After the frame's last `cs_finish`, the births: the host reserves `M` slots after the frame's drops (`M` from
   the trapped air the report two frames back found, plus a quarter and 64, at most `air_most` 65,536, none when it
   found none). `cs_air_dead` fills the slots past the births with a drop off the course; `cs_air_born` writes each
   marked spot, by its running sum, into its slot: its place, the speed round it, kind `AIR | STOPS`, and the
   serial the host reserved for that slot. Spots past `M` wait for the next frame (the cell is still empty).
4. The report counts the births as live (`live` = the last sort's live drops plus the births), so the slots not
   used are let go at the boundary two frames on with the drops that left. The host's count, room and serials take
   `M` more.

So everything a frame finds is born in the same frame's submission: a copy made at a frame boundary needs nothing
new, and the births repeat exactly (places from the cell, slots and serials from whole-number running sums). A
new hole is filled from the third frame after it opens (the report two frames back must have seen trapped air for
slots to be reserved), in a frame's time (1/60 s) per step of `air_most`.

Air needs the deep part's cells (`Numbers::deep` 1); with `deep` 0 there is no trapped air. **Trapped air** on the
Water card turns it off (`Numbers::air` 0): the frame then runs none of the passes and pools behave as in C2 to
the bit.

### Wetting: the surface acts like water to the water's skin

**(My call)** **Wetting** is how much a surface counts as water to the water's skin, 0 to 1:

- **Its normals.** `lies` adds `wetting × L(d)` toward the face, where `L(d)` is what a resting half-plane of drops
  behind the face adds to the smooth-kernel sum of a drop `d` from it (a host table, as `wall_table`). At Wetting
  1 a puddle's bottom row sees liquid all round: no surface at the floor, so the bend no longer holds it up, and
  it settles as low as its skin allows. At 0 the solid is air to the skin, as today.
- **Its pull (adhesion).** The skin's pull adds `wetting × S(d)` toward the face, by the pair's own `thin` number
  taken with a full neighbour (`2 / (full + 1)`), where `S(d)` is the skin's pull a resting half-plane of drops
  behind the face gives (a host table). At Wetting 1 the rock pulls the water as the water pulls itself (the
  water spreads over it: a film); at 0.5 about half (a contact about square); at 0 not at all (round beads).
  This is the Young–Dupré picture: the work of adhesion is Wetting times the work of cohesion.
- **Its drag, measured (Task 6):** a waxy face may keep only part of the drag every liquid has (`wet_drag`), so
  beads roll. Kept only if the slope measuring shows beads sticking at Wetting 0.
- Air drops take none of it. Stickiness (the liquid's own grip and drag) is unchanged and adds to it.

Wetting acts only within a reach (1.5 cells) of a face, so water away from the rock (streams in the air, the open
surface of a pool) is the same to the bit at any Wetting.

**Where it lives.** Each node of the solid field carries the wetting of the solid whose face is nearest
(`SolidField::wetting`, one per node). In 2c every node takes the Course's Rock card's Wetting; part 3 gives each
Piece and Wall its own. On the card the wetting is the second half of the `solid` buffer (`solid[nodes + node]`,
read between nodes as the distance is), so no pass binds another buffer (`cs_finish_predict` already binds 8).

**Defaults (My call, settled by measuring in Task 6).** The Material card's Wetting default is the one at which
the course's top shelf meets the spec's puddle mark (at most twice the skin length plus a spacing: 7 px at Fine,
5.5 px at Extreme, at Surface pull 0.2) without standing water, expected about 0.6 to 0.8 ("wet stone").
`bpe_water::DEFAULT_WETTING` holds it, and the field, the test scenes and the Material card all start from it.

### Settings

- **Water card** (`WaterWorld`), new group **Trapped air** (in `SECTION_ORDER` after "Carving": "Air" is a ball
  section): **Trapped air** (on/off, default on: "Air caught under the water rises as bubbles and bursts at the
  top"), **Bubbles rise** (0 to 4 times, default 1, keyframed: "How fast trapped air rises. 1 is real air.").
- **Material card** (`MaterialCard`): **Wetting** (0 to 1, simple: "0 is waxy: water beads up and rolls off. 1 is
  soaked: water spreads into a film."). In 2c it is read on the Course's Rock card only (part 3 reads a Piece's and
  a Wall's).
- `WaterSetup::print` gains the Water card's `trapped_air` and `bubble_rise` and the Course's Material `wetting`;
  `inputs_at` reads Bubbles rise from its keyframes once a frame (`FrameInputs::air_lift`), and the draft sets it
  on the body each frame (`Water::set_air_lift`). The draft's field takes the Rock's Wetting
  (`SolidField::with_wetting`).

## Files

| File | What |
| --- | --- |
| `crates/bpe-water/src/drop.rs` | `AIR`, `Drop::is_air` |
| `crates/bpe-water/src/solid.rs` | `SolidField::wetting`, `DEFAULT_WETTING`, `with_wetting`, `wetting_at`, `for_card` (cells then wetting) |
| `crates/bpe-water/src/drops/numbers.rs` | `air`, `air_lift`, `air_pop`, `air_reach`, `air_most`, `wet_drag`; `air_window`; `wet_table`; env |
| `crates/bpe-water/src/drops/mod.rs` | `FrameGpu.air`, `.wet`, `dgrid.z/w`; the air passes in `play_frame`; `M`; `set_air_lift`; the solid upload |
| `crates/bpe-water/src/drops/air.rs` (new) | the air buffers (`airc`, `airb`) and `slots_for` (M from the report) |
| `crates/bpe-water/src/drops/clock.rs` | `Report.air` (was `spare`), `Clock::air` |
| `crates/bpe-water/src/drops/passes.rs` | `AirFind`, `AirSumA`, `AirSumB`, `AirDead`, `AirBorn`; bindings 27, 28 |
| `crates/bpe-water/src/drops/shaders/air.wgsl` (new) | the five passes |
| `crates/bpe-water/src/drops/shaders/{common,sort,finish,deep,report}.wgsl` | `AIR`, `Fr.air`, `Fr.wet`; air's fall; skin, pop and wetting in `finish_at`; air's mass; the report |
| `crates/bpe-water/src/shaders/plain.wgsl`, `src/draw/plain.rs` | air drops not drawn (the world pass binds the kinds) |
| `crates/bpe-water/src/probe.rs` | `Stats`, `pieces`, `Stream`, `ragged`, `along`, `inside` leave air out; `pockets` (new) |
| `crates/bpe-water/src/scenes.rs` | `pool_with_hole`, `blobs` (new test scenes) |
| `crates/bpe-water/tests/air.rs`, `tests/wetting.rs` (new) | behaviour tests |
| `crates/bpe-water/tests/shots.rs` | measuring runs `air`, `wetting`; `shelves` takes `WATER_WETTING` |
| `crates/bpe-core/src/settings.rs` | "Trapped air" in `SECTION_ORDER` |
| `crates/bpe-kit/src/modules/water/{scene,solids}.rs` | the new settings |
| `crates/bpe-kit/src/things/water/draft.rs` | print, inputs, field |
| `crates/bpe-studio/src/water_mode/draft.rs` | sets the air lift each frame |
| `docs/notes/water.md` | "Part 2c (as built)" |

## Tasks

Run only the tests this part touches (`cargo test -p bpe-water --features test-scenes --test air` and so on,
`--test-threads=2`); CI runs the rest, the engine tests on the owner's card.

### Task 1: air drops (no finding yet)

- `drop.rs`: `pub const AIR: u32 = 128`; `Drop::is_air`. Unit test.
- `numbers.rs`: `air` 1, `air_lift` 1.0, `air_pop` 0.75, `air_reach` 48.0, `air_most` 65,536, `wet_drag` 1.0
  (no change until Task 6), in `Default`, `from_env_for` (`BPE_WATER_AIR`, `_AIR_LIFT`, ...). `air_window(cell)`:
  the reach in cells, at least 3, at most 64. Unit test.
- `common.wgsl`: `const AIR: u32 = 128u;` `fn is_air(kind: u32) -> bool`; `Fr.air: vec4<f32>` (lift, or 0 when
  air is off; pop; reach in cells; 0) after `dgrid`; `FrameGpu` to match.
- `sort.wgsl` `fall`: an air drop's gravity factor is `−fr.air.x` (its liquid's damping and the cap as water).
- `finish.wgsl` `finish_at`: the neighbour's kind from `velb[j].z`; an air drop skips its skin, bend, grip; a water
  drop leaves air neighbours out of `lies`, `pull`, `bend`; both keep `even`, `meet` and drag. An air drop under
  `fr.air.y` full is moved to `(−1e6, −1e6)` with no speed (`nf` 0).
- `deep.wgsl` `cs_deep_gather`: an air drop's mass is a water drop's.
- `probe.rs`: every measure of the liquid leaves air out (`Stats::of`, `inside`, `along`, `ragged`, `Stream::of`,
  `pieces`, `picture`); `print_of` keeps all drops.
- `tests/air.rs` (12 px, the software card):
  - `air_rises_and_bursts`: `scenes::pool_with_hole` (a still box 0.6 full with a round hole 72 px across 120 px
    above the floor) with the hole filled by hand with air drops (`fill_stacked` on the hole, `AIR | STOPS`) and
    `Numbers { air: 0 }` (so nothing else is born): the air's middle rises every 0.25 s; by 2.5 s no air drop is
    left; the liquid's drops are the same in number throughout; the pool's top ends lower than it was with the
    air in (the pocket's room is gone) and within 2 % of where a pool without the hole would rest.
  - `water_round_a_bubble_keeps_it_round`: at 0.25 s the air drops' spread across and down are within 40 % of each
    other (a bubble, not a smear).
  - (That air changes nothing where there is none is Task 2's `trapped_air_off_and_on_alike_without_air`.)
- **Passing:** the three tests above, `engine.rs` unchanged, `every_pass_has_its_entry_and_fits_the_limits`.
- Commit "bpe-water: air drops fall upward and burst at the top".

### Task 2: the card finds trapped air

- `shaders/air.wgsl` (joined after `deep.wgsl`): `cs_air_find`, `cs_air_sum_a`, `cs_air_sum_b`, `cs_air_dead`,
  `cs_air_born` as in the design. Bindings: 27 `airc` (per cell `vec4<u32>`: spots' mask, running sum, speed x and
  y as bits), 28 `airb` (per block of 512 cells, then one more: the total). `dgrid.z` the serial the frame's first
  air slot takes, `dgrid.w` the slots `M`. The births pass writes the kept buffers (`xv`, `nf`, and the kinds the
  frame ends in: binding 4 in the group of the body's `now` after the frame).
- `passes.rs`: the five passes and their bindings (each at most 8 storage buffers; `cs_report` adds 28).
- `drops/air.rs`: `Air { cells: wgpu::Buffer, blocks: wgpu::Buffer }` (per layout node, and per block plus 2);
  `slots_for(found, room_left, most) -> u32` (`found + found / 4 + 64`, none when `found` is 0, at most `most`
  and `room_left`). Unit test.
- `mod.rs` `play_frame`: on the last step, when the cells are measured and `numbers.air` is 1, `AirFind`,
  `AirSumA`, `AirSumB` after `DeepColumn`; after the last finish, when `M > 0`, `AirDead` (M threads) and `AirBorn`
  (a thread a cell), both at the last step's numbers; the air blocks' buffer cleared at the frame's start; `count`
  and `clock.sent` take `end + M`; `grow_to(end + M)`; the serial moves on by `M`. `Report::spare` becomes `air`,
  read in `Clock::settle` into `Clock::air`.
- `report.wgsl`: `live` adds `min(total, M)`; `air` the total.
- `scenes.rs`: `pool_with_hole(cell)` (above, the hole left empty).
- `tests/air.rs`:
  - `a_hole_in_a_pool_fills_with_air`: by frame 4 there are air drops, all within the hole (and a cell of its
    edge); no air drop above the pool's top; by 3 s none is left; the liquid's count never changes.
  - `open_water_gets_no_air`: the slope shot and a thin pour at 12 px for 1 s: no air drop is ever read, and the
    drops are the same to the bit as with `air` 0.
  - `trapped_air_off_and_on_alike_without_air`: the still box: `air` 1 and 0 the same to the bit for 1 s.
  - `air_repeats`: `pool_with_hole` twice from the start, and from a copy made at frame 3 (births in flight):
    the same to the bit at 1 s (`print_of` with the air).
  - `trapped_air_off_leaves_the_hole`: with `air` 0 no air drop is ever made.
  - `a_pour_into_a_pool_loses_its_air`: the pool shot at 12 px for 4 s (the pour stops at 3): air is found while it
    pours; at 4.5 s there is less than a tenth of the most there was.
- **Passing:** these, Task 1's, `engine.rs`, `plain.rs` (the drawing is still unchanged), the passes' limits test.
- Commit "bpe-water: the card finds trapped air and fills it with air drops".

### Task 3: air is not drawn

- `plain.wgsl`: `vs_splat` and `vs_world` put an air drop off the picture, as a drop that left. `plain.rs`: the
  world pass's layout and group bind the kinds (binding 2).
- `tests/plain.rs` `air_is_not_drawn`: a pool with air drops in a hole draws the hole as a hole (the amount there
  as with no drops), and the same pool with the air left out draws the same picture to the bit.
- Commit "bpe-water: the plain drawing leaves air out".

### Task 4: measuring the air (software card first, the owner's card in Task 8)

- `tests/shots.rs` `air` (`#[ignore]`, by hand): the pool shot at `WATER_CELLS` (6, 3, 2 px); every 0.25 s from
  1 to 6 s: air drops, trapped room (`probe::pockets`: 4 px squares between the walls with no liquid drop within
  4 px, with liquid above them in their column and below them; their area, the biggest connected piece and its
  middle's height); and the wall seconds. `WATER_AIR=0` for C2's behaviour beside it.
- Tune `air_lift` (no: it is the physics' own 1), `air_pop`, `air_reach` only as the numbers say, one at a time,
  each change in "As built". The marks are Task 8's.
- Commit "bpe-water: the air measuring run".

### Task 5: Wetting in the engine

- `solid.rs`: `SolidField { layout, distance, wetting }`, `wetting` one per node; `DEFAULT_WETTING` (Task 6 sets
  it; until then 0, so Task 5 changes no water); `build*` and `from_distance` fill it; `with_wetting(w)` sets every
  node; `for_card()` the distances in cells then the wettings (2 × nodes floats). `copy()` keeps it (the buffer is
  copied whole).
- `numbers.rs` `wet_table() -> [[f32; 4]; 17]`: for 33 distances from a flat face (0 to a reach), `S(d)` and
  `L(d)` as in the design, from the resting density (four drops a cell) over the half-plane behind the face, by the
  same kernels and constants the shader uses (`skin`, the smooth kernel `24 / (π h⁴) r (1 − r²/h²)²`). Unit test:
  `L` is positive near the face and 0 at a reach; `S` at a resting drop's distance (`wall_off`) is a pull toward
  the face, about what a surface drop feels from the water under it (within 30 %).
- `common.wgsl`: `Fr.wet: array<vec4<f32>, 17>` (after `wall`), `wet_table(q)`, `wet_at(q)` (bilinear from
  `solid[nodes + ...]`).
- `finish.wgsl`: for a water drop within a reach of a face: `pull += wet · thin_w · S · n̂`, `lies += wet · L ·
  (−n̂)` (`n̂` the face's outward way); the base drag times `mix(wet_drag, 1, wet)`.
- `mod.rs`: the solid buffer from `for_card()`; `FrameGpu.wet`.
- `scenes.rs` `blobs(cell, wetting)`: five square blobs (12 to 64 px) on a level plate and five on a plate tilted
  15°, in a box, Surface pull 0.5 (`beads` with its field at `wetting`).
- `tests/wetting.rs` (6 px, a small course, the software card):
  - `a_blob_spreads_on_a_wet_floor_and_stays_round_on_a_waxy_one`: one 48 px blob on a floor after 1.5 s, at
    Wetting 0, 0.5, 1: wider and lower as Wetting grows (each step at least a spacing wider), and at 0 its height
    at least half its width.
  - `wetting_reaches_only_water_near_a_face`: a blob falling in the middle of a big box for 10 frames (never within
    a reach of a face): Wetting 0 and 1 the same to the bit.
  - `air_takes_no_wetting`: air drops under a ceiling at Wetting 0 and 1 move the same.
- Commit "bpe-water: Wetting, how much a surface counts as water to the water's skin".

### Task 6: measuring Wetting and the defaults

- `tests/shots.rs` `wetting` (`#[ignore]`): `blobs` at Wetting 0, 0.25, 0.5, 0.75, 1 at `WATER_CELLS` (6, 3 px)
  after 3 s: each level blob's width, height and the angle its edge makes with the floor (`2 atan(2h / w)`, a
  circle's cap); each tilted blob's run down the plate, and the share of its drops left behind as a film. `shelves`
  takes `WATER_WETTING`.
- Decide, with each change and its numbers in "As built":
  - whether `wet_drag` stays 1 (beads on a waxy slope roll off without a film behind) or comes down;
  - the strength of each term (only if the angles are not monotone, or Wetting 1 does not spread to a film one or two
    drops thick, or Wetting 0 does not keep beads at least as round as today);
  - `DEFAULT_WETTING`: the least Wetting at which `shelves` meets the mark (tallest at most 7 px at 6 px and
    5.5 px at 3 px, no standing pool), rounded up to a tenth.
- Then the engine tests and the film shots at the default (`engine.rs`, `streams` in `shots`): any bound that moved
  because thin water now clings to rock is looked at, and kept only with a reason in "As built" (never loosened to
  pass a fault).
- Commit "bpe-water: Wetting's default, from the shelves".

### Task 7: the cards and the editor

- `bpe-core/src/settings.rs`: "Trapped air" after "Carving".
- `scene.rs` `WaterWorld`: `trapped_air: bool` (default true), `bubble_rise: f32` (range 0 to 1e9, span 0 to 4,
  unit ×, keyframe, default 1), both in group "Trapped air".
- `solids.rs` `MaterialCard`: `wetting: f32` (range 0 to 1, span 0 to 1, simple, group "Material"), default
  `bpe_water::DEFAULT_WETTING`.
- `draft.rs`: `WaterSetup` keeps the Course's Material card (`Keyed`); `print` hashes `trapped_air`, `bubble_rise`
  (and its keys) and the Course's Material `wetting`; `numbers()` sets `air` from Trapped air; `field()` takes the
  Rock's Wetting; `FrameInputs.air_lift` (Bubbles rise at the frame's start).
- `bpe-studio/src/water_mode/draft.rs`: each frame's inputs set the body's air lift (`Water::set_air_lift`).
- Tests: `bpe-kit` `things::water::draft` (a Wetting change, Trapped air and Bubbles rise restart the draft; a
  Piece's Material does not yet; Bubbles rise is read from its keyframes; the field carries the Rock's Wetting;
  Trapped air off sets `air` 0); `what_restarts_the_draft` lists the new keys; the settings contract tests
  (`simple` count, sections, `a_water_section_is_no_ball_settings_group`).
- Commit "Water: Trapped air and Bubbles rise on the Water card, Wetting on a material".

### Task 8: the card run and the verdict

Through Remote Control on `F:\!Ai Stuff\BallPhysicsEngine` (the recipe in project memory `liquid-speed` and C1's
and C2's runs: a Windows test exe built in the WSL `bpe-ci` box, run natively; results to the run's own branch,
`[skip ci]`; end the session when it reports). Groups: `air` (6, 3, 2 px, air on and off), `wetting` (6, 3 px),
`shelves` (6, 3 px at the default), the films (pool at 6, 3, 2 px; beads, slope, ledge, course, weird at 6 and
3 px), `openings`, `landing` (6 and 3 px at Surface pull 0.2 and 0.5), `pace` (pool and course at 6 and 2 px, air
on and off), `repeats`.

| Mark | How |
| --- | --- |
| A plunging pour's air rises and leaves: trapped room 1.5 s after the pour stops under a tenth of its most, and none (under 1 %) at 2.5 s, at Fine, Extreme and 2 px (C2: a pocket at 5.8 s) | `air` |
| Pockets rise: the biggest pocket's middle moves up while it lasts; big pockets break up or rise out (films, by eye, against real jets: bubbles rise and burst within a second or two) | `air`, films |
| No air in the open: slope, ledge, thin pour, still pools on and off the same to the bit | `tests/air.rs` |
| The water is kept: the liquid's drops never change in number through the air | `tests/air.rs`, `air` |
| Repeats with air: twice, and from a copy with births in flight | `tests/air.rs`, `repeats` |
| Wetting is monotone and alike at Fine and Extreme: blobs wider and lower as it grows; Wetting 0 beads (angle at least 120°), Wetting 1 a film one or two drops thick (angle at most 30°); the same Wetting's angles within 20° across the two details | `wetting` |
| Shelves at the default: tallest at most 7 px (Fine) and 5.5 px (Extreme), no standing pool | `shelves` |
| Away from the rock nothing changes: `wetting_reaches_only_water_near_a_face`; Water Weird's landing still one body, small share within 5 points across details | tests, `landing` |
| The course's shares within 5 points of C2's (wet rock holds a little more on slopes: said so) | `openings` |
| Air costs at most a tenth of a frame at Fine on the pool shot | `pace` |

The draft in the editor is not timed again unless `pace` shows air costing more than a tenth.

### Task 9: notes, review, pull request, merge, hand-over

- `docs/notes/water.md` "Part 2c (as built)" (what it is, the numbers, the card's verdict, the tests); this plan's
  "As built and measured"; the spec's Parts line for 2c says it is built.
- One Opus review of the whole branch; fix what it finds; the PR's checks green; merge (squash).
- A hand-over for part 3 (pieces and the pen) in the project files, `ballphysics/water-mode/part3-handover.md`:
  what 2c leaves for it (a Piece's and a Wall's Material Wetting read into the field's per-node wetting, nearest
  face's solid; air under pieces), then message the coordinator to start part 3 (Aedan, 2026-11-26 19:43Z: water
  parts roll on without asking).

## Review focus

1. **Births and the count.** A slot reserved but not used must be let go, never a live drop: `live` counts the
   births, `count` the slots. Test: `a_hole_in_a_pool_fills_with_air` checks the liquid's count every frame, and
   `air_repeats` the whole body.
2. **A copy at a boundary with births in flight** plays on the same (`air_repeats` from frame 3).
3. **Air in the open** never: the finding needs full liquid above (not rock) and full liquid or rock on the other
   three sides within the reach. `open_water_gets_no_air`.
4. **Air off is C2 to the bit** (the passes don't run, the report's air is 0, no slot is reserved).
5. **Wetting 0 is today's skin**: every new term is `wet ×` something, added; `wetting_reaches_only_water_near_a_face`
   and the blob at 0 against C2's beads.
6. **Pair symmetry** (C2's review lesson): the skin's pairs leave air out on both sides alike (a water drop skips an
   air neighbour; an air drop has no skin), so no pair pulls one way only.

## Things not to do

- Don't seed air from the host or from a report two frames late: the births happen in the frame that finds them.
- Don't let an air drop's place come from a random number, or its slot from an atomic counter.
- Don't draw air (part 6 draws bubbles); don't give air a skin of its own.
- Don't weaken the skin's bend to flatten puddles: Wetting is the fix (the checkpoint's choice).
- Don't change anything for water further than a reach from a face for Wetting, or anywhere there is no trapped
  air for Air.
- Don't touch today's Liquid thing, Balls scenes, or the pinned pictures (`golden-plain.txt` is drawn from stored
  drops and has no air).
- Don't build or install the exe. No demo films: the card run's films are for judging, kept in the project files.

## As built and measured

### What changed from the design, and why (measured on the software card at 12 and 6 px)

1. **One-way pressure.** With air pushed by the water as water is, air that broke off a pocket sat in the water's
   lattice and rode its flow down. Now the liquid counts air in its fullness and makes way for it, but air is pushed
   only by air and the solids (`lambda_at` leaves liquid out of an air drop's density, `apply_at` its push).
2. **Bursting where the water's top or a film is over it.** Air thrown out of the water against the box's lid stayed
   (the wall's share made it full): out of deep water a solid no longer counts toward an air drop's fullness
   (`bursts`). And air that reached the top spread under it as a raft held by a film of water: an air drop now also
   bursts when liquid over it (within 60° of straight up) is less than `air_pop` full (`finish_at`), as a real
   bubble's film drains and breaks.
3. **The finding fills a pocket out to its water.** Air born only in the cells no drop reaches stopped about a cell
   short of the water round it, so it had open room beside it and burst at once (at 12 px a 72 px hole's 16 air drops
   were gone the frame after they were born). Now any cell neither full nor solid that the water shuts in is trapped,
   and its spots clear of every drop by 0.4 cell (`AIR_CLEAR`) are born: the air reaches the water and stays; a pocket
   part filled is filled the rest of the way the next frame. (Filling cells beside empty ones only was tried first: it
   left the rest of a pocket that had begun to close.)
4. **Room for new pockets.** The slots now add a quarter of the deep cells (64 to 2048) to the air found two frames
   back and a quarter more, so a pocket that closes while air is being found is filled in the frame it is found (at
   Fine a 72 px hole got 144 of its 201 spots at first, all 201 with the room).
5. **`air_drag` (0.2).** At the liquid's own evening and closing loss, air that broke into small bubbles rode the
   water's flow: the 72 px hole's air was spread through the top of the pool 3 s on (42 drops left). Air and liquid
   now even out and close on each other by a fifth of the share liquid has with liquid: the hole rises at about
   400 px/s and its air is gone by 2.25 s at Fine. (The interior of a big pocket is air against air, so its rise is
   the same; 2D theory for a 72 px cap is about 200 to 280 px/s, so Bubbles rise below 1 is there for slower.)
6. **The marks' tests.** At 12 px an empty hole falls in for the two frames the count takes to come back, so
   `a_hole_in_a_pool_fills_with_air` asks for 30 air drops, not 60 (47 come). The pool shot keeps sloshing for
   seconds after its 3 s pour and its breaking waves trap air all along, so `a_pour_into_a_pool_loses_its_air`
   pours for one second (with the fixes below: most air 86 at 1.25 s, 1 at 4 s). A 48 px blob is too big to stand up (the skin of C2's
   water holds about 20 px up), so the blob test is 24 px: at Wetting 0 its edge is near square to the floor (36 px
   wide, 17 px tall), at 0.5 lower (42 x 15), at 1 a film (117 x 3). `a_stronger_skin_holds_a_bubble_whole_longer`
   looks at 0.25 s, since bubbles rise faster.
7. **`wet_drag` stays 1.** On the tilted plate after 1 s at Fine, beads at Wetting 0 already run 334 px down it with
   none of it a film (56 % still on it), and at Wetting 1, 256 px with 58 % of what is on it a film: waxy beads roll
   without taking drag away.

### Measured on the software card (lavapipe), Fine (6 px), before the review's fixes

(These numbers are from before point 8 below: born air was being cut at the boundaries, so they flatter the air.)

- The pool shot (a 70 px pour plunging for 3 s), trapped room (`probe::pockets`, px²) every 0.25 s from 3.25 s:

  | | 3.25 | 3.5 | 3.75 | 4.0 | 4.25 | 4.5 | 4.75 | 5.0 | 5.25 | 5.5 | 5.75 | 6.0 | mean |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | air off (C2) | 34,209 | 31,833 | 84,735 | 111,258 | 85,014 | 17,082 | 16,236 | 32,193 | 30,861 | 5,544 | 5,940 | 36,621 | 40,960 |
  | air on | 84,195 | 69,876 | 29,601 | 14,742 | 30,555 | 59,706 | 43,362 | 3,438 | 3,600 | 4,023 | 8,937 | 261 | 29,358 |

  The air's own room counts as trapped room (air is not drawn: a pocket full of air is a hole), so while the pool
  churns (about 1,000 px/s rms) the two are alike; from 2 s after the pour the pockets are about a tenth of air
  off's. The plan's mark (under a tenth of the most 1.5 s after the pour, under 1 % at 2.5 s) is missed on this shot
  while it sloshes. Air's reach: 160 px found about ten times the air of 48 px but left the pockets as they were, so
  it stays 48 px.
- Wetting 0 is C2's water: the course's top shelf after 10 s is 5.6 % of the water, its tallest puddle 13.5 px
  (C2's card: 5.6 %, 13.4 px).

### The review's fixes (one Opus review of the branch)

8. **New air was cut at frame boundaries.** Air was born after the drops that left, but a boundary lets go from the
   end: whenever more drops had left two frames back than the last frame kept as unused room, the cut ran into air
   born the frame before (at 12 px, 2,723 live air drops cut in a one-second pour). Air is now born over the head of
   the drops that left (`starts[cells]`), and the room past the births holds drops that left, so the body always ends
   in drops that left. Test `a_boundary_lets_go_of_no_air` (it failed with 2,160 cut before the fix).
9. **The draft's cap counted air.** `Water::count()` holds air and the room kept for it, so near the draft's cap the
   pours could stop and start as air came and went. The report now counts the liquid's drops in the course
   (`Report.liquid`, `cs_report` binds the kinds the last step sorted into) and `Water::liquid()` adds the liquid
   poured since; the editor's draft caps its pours and shows its count by it. Test
   `the_liquid_is_counted_without_the_air`.
10. **A copy did not take the depth share** (`deep.dfa`), which a frame that only measures the deep part reads on its
    steps before the last (where air bursts): `Deep::copy_kept` copies it.
11. **`bursts` was looked at for every drop** (`select` takes both sides): now for air drops only, the same numbers.
12. Left as they are, with a note in part 3's hand-over: air takes liquid 0's feel (part 4, several liquids); air born
    against a drop the last step moved (rare, by under 0.4 cell); the step count leaves Bubbles rise out of its
    headroom (the speed cap holds it; the report catches up two frames on).

### The card (RX 9070 XT), Task 8

Three runs through Remote Control (results `card/water-2c/results` on the branch until the merge, then project files
`water-core/2c/`): at c7cfd3f (before the review, shelves only kept for their ceiling finding), at ca85aa3 (the
review's fixes; shelves, Wetting, pools kept), and at 33d8ed9 (the band below; air, films, pace, landing, course,
repeats).

13. **The shelf measure counted drops hanging from the box's lid** (the top pool starts 24 px under it, and on wet
    rock a few drops stay up there): any Wetting above 0 read 254 to 256 px. `shelves` now counts only water joined
    to the shelf, each drop within a cell of the one under it, and reports the rest as "hanging above" (0.0 % in
    every row at 30 s on the second run).
14. **Shelves at 30 s, Stickiness 0** (tallest px at 6 px / 3 px): Wetting 0 7.1 / 7.9 (C2 7.0 / 7.9), 0.3 6.9 /
    7.6, 0.5 6.2 / 6.9, 0.7 5.5 / 6.6, 0.9 5.3 / 6.2, **1 6.6 / 5.2**. Only Wetting 1 meets both marks (7 and 5.5),
    so **`DEFAULT_WETTING` is 1**. The engine tests (35 lib, 28 engine, 20 plain, air, wetting) and the kit's 46
    Water tests pass at it; no bound moved.
15. **A foam under the water's top.** With point 8 fixed, the pour's air lived on: at 3 px 53,925 air drops at
    4 s and 32,328 at 6 s, much of it a foam of small pockets under the top that never burst. The liquid rests on
    air (it makes way for it), so a cap or a foam's films stand. A real bubble's cap drains and breaks within a
    moment, so **air now bursts out of the deep water**, in the band under the water's open top that the drops alone
    hold (`deep_d`, 24 px at every detail; the deep part counts air as water, so a foam under the top is in the band
    and a pocket deep in the pool is not). This replaces the earlier "out of the deep water, if the drops alone are
    not full" rule. Air held deep under a solid stays (`air_held_deep_under_a_bell_stays`); air on a floor with no
    water round it bursts at once, so `air_takes_no_wetting` (which watched such a block burst from its sides in)
    is now `air_on_a_wet_floor_bursts_as_on_a_dry_one`.
16. **The pool shot at 33d8ed9** (air drops; pockets px², air on / air off):

    | | most air | air at 4.5 s | air at 6 s | pockets mean on / off | pockets most on / off |
    | --- | --- | --- | --- | --- | --- |
    | 6 px | 1,191 | 214 | 188 | 36,599 / 33,641 | 91,305 / 90,531 |
    | 3 px | 11,523 | 4,094 | 4,027 | 79,924 / 60,507 | 140,260 / 119,500 |
    | 2 px | 22,418 | 15,467 | 11,626 | 87,117 / 110,983 | 165,644 / 178,493 |

    (At ca85aa3, before the band: most 5,415, 53,925 and 94,426; at 6 s 2,024, 32,328 and 70,412.) **The mark is
    missed as written**: the pool sloshes (rms about 1,000 px/s) for seconds after the pour and its breaking waves
    keep trapping air, as they make empty pockets with air off. By the stills (3.5, 4.5 and 5.5 s) the foam under the
    top is gone and the air is small pockets in the churning water; what is left at 6 s is new air from the slosh and
    the rims of cavities wider than about twice `air_reach`, which stay empty inside (as with air off). (The table's
    "biggest pocket" is a different pocket from row to row, so it does not show one pocket rising.)
17. **Pace** (wall s a scene second, quiet PC): pool 0.450 / 0.417 at 6 px, 2.905 / 2.845 at 2 px; course 0.407 /
    0.403 and 2.056 / 2.008; weird 0.379 / 0.380 and 1.196 / 1.169. Air costs at most 8 % (pool, 6 px): the mark (a
    tenth) is met. (At ca85aa3, before the band, the pool at 2 px cost 18 %.)
18. **Wetting on the card** (blobs after 3 s, edge angle °): monotone at both details. At 6 px a 24 px blob is 92,
    84, 70, 47, 6 at Wetting 0, 0.25, 0.5, 0.75, 1; at 3 px 89, 79, 73, 61, 51. **Missed**: Wetting 0 beads stand
    at 113° (12 px blobs) but 90° (24 px), not 120° (bigger blobs sag: the skin holds about 20 px up); and Wetting
    is **weaker at 3 px than at 6 px** (at Wetting 1 the 24 px blob is 6° against 51°; the tilted plate keeps 60 %
    of its water as a film at 6 px, 3 % at 3 px), more than the 20° the plan allowed. Handed to part 3 (it gives
    each piece its own Wetting): find why adhesion reaches less at finer cells (the table's reach is a cell; the
    skin's is too) before a final at 2 px is judged against a draft.
19. **The course** at 30 s (Wetting 1 against C2 at 0): top pool 5.3 % / 5.7 % (C2 7.5 / 7.3), through 12 px 32.8 /
    32.3 (C2 34.8 / 38.2), through 24 px 57.2 / 55.9 (C2 52.0 / 51.1): wet rock holds less on the shelves, so more
    goes on; within 5 points but for the 12 px opening at 3 px (5.9). Landing: Water Weird's block stays one body
    (2 to 10 pieces, its biggest holds nearly all). Pools at rest 99.7 % of their height; repeats all yes.
20. **`run.ps1`**: a list of groups needs `powershell -Command` (`-File` passes it as one string). Another project's
    CI runner on the PC shares it; pace was re-run when it was quiet.

### Verdict

Built and kept: trapped air that rises, breaks up and bursts at the top (and stays deep under a solid), Wetting per
node with Wetting 1 as the default (the shelves' marks met), the liquid count for the draft's cap. Missed and said:
the pour shot's pockets mark (the slosh keeps trapping air, as with air off), Wetting 0's 120° beads, and Wetting's
reach at Extreme against Fine (to part 3).
