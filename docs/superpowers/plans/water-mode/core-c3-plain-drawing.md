# All-drops water core, part C3: the plain drawing on the card (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Water mode's own picture of the water, drawn on the card straight from the all-drops engine's buffers:
a flat back, rock from the solid field with a smoothed edge and a darker rim, and the liquid as a smooth body
with a crisp outline, darker and richer with depth over its Clarity, a thin lighter rim along its surface, and
round droplets; then films, `water_core.rs` and the full-size look check moved onto it, and the bridge removed.

**Architecture:** One render pass splats every drop (an instanced quad read from the drops' storage buffers) into
two half-float pictures with additive blending, in the drops' order, so the sums repeat. A small pass turns the
amount into how much of each coarse texel is liquid and how much is open (not rock), and a ladder of halvings
(a B-spline pyramid) blurs it. One full-screen pass then cuts the outline, reads the depth from the pyramid at the
pixel's Clarity, adds the rims and the rock, and writes 8-bit sRGB with a fixed dither.

**Tech stack:** Rust 2024, wgpu 30 (render pipelines, WGSL), glam, bytemuck. Tests on `bpe_gfx::test_device()`.

**Spec:** `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md`, section 9 ("The plain drawing"),
section 11 (repeats: "the drawing's sums in the drops' order") and the C3 pass marks. C1's plan
(`core-c1-engine.md`, "As built and measured") says what the engine hands over: `Water::drop_buffers()`
(`DropBuffers { xv, nf, id, count }`, 16 bytes a drop each, grid units).

## Global constraints

- wgpu's default limits (the editor's device is made with them): at most 8 storage buffers a stage, a storage
  binding of at most 128 MiB, a 2D texture at most 8192 pixels on a side. The drawing asks for no feature.
- Nothing the drawing does reads the drops back to the processor. It reads `xv` and `id` where the engine left
  them, after the frame that wrote them (queue order).
- The same drops in the same order on the same card give the same picture to the bit (blending adds in the
  drops' order; nothing depends on thread scheduling; the dither is fixed by pixel position).
- Processor-side code in `bpe-water` keeps the crate's `clippy.toml` (no `f32::sin`, `powf` and the like; use
  `bpe_core::maths::Maths`). Colours go to the card as sRGB and are made linear in WGSL.
- No physics, physics number or engine test changes (part C2 runs beside this part on `drops/`, `drop.rs`,
  `scenes.rs`, `probe.rs`, `engine.rs`). Balls scenes, their goldens and their pinned pictures do not move.
- Lines at most 120 wide (`cargo fmt --all`); `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Tests that draw call `bpe_gfx::test_device()`. `cargo test` with `--test-threads=2`.
- No exe is built or installed. Films only from the card run (Task 6), which the spec asks for.

## Decisions (My calls; the spec leaves the drawing's details to the builder)

| What | Decision | Why |
| --- | --- | --- |
| Splat | `(1 - q²)²` inside radius `R = 0.9` cell, peak `W = 3 / (4π R²)` (a body at rest sums to 1); `LEVEL = 0.2` | C1's processor drawing: a film one drop thick sums to 0.57 along its line (one piece); drops part visibly past about 2.3 spacings; same in cells at every detail, so draft and final outlines agree within a spacing |
| Least splat | radius at least `LEAST_R = 1.25` picture px; when enlarged the peak is `max(W (r_true / r)², 2.5 LEVEL)` (tuned: 1.5 lost a drop at a pixel's corner at scale 0.25) | a lone drop stays at least about a pixel across at any zoom; at full size (scale 1) no detail is enlarged (1.5 px cells give 1.35 px) |
| Amount pictures | two `Rgba16Float` targets: `(Σk·colour, Σk)` and `(Σk·deep, Σk·clarity / 1024)`, colours linear, cleared to 0, blend `One + One` | per-liquid colour, Deep colour and Clarity mix where liquids meet; half floats are blendable on every card |
| Face fill | the amount is divided by `1 - missing`, `missing` the share of a splat beyond the face (drops rest `rest` cells off it), at most ×5 | C1's processor drawing: liquid resting on a face is drawn up to it, with no line of back between |
| Outline | `alpha = clamp(0.5 + (a - LEVEL) / |∇a|, 0, 1)`, `∇a` by central differences of the face-filled amount | one picture pixel of smoothing at any size |
| Depth (as built) | `b = blur(closed)`, closed = liquid cover or rock cover (the larger), blur σ = Clarity; depth `x = σ √2 erfinv(2b − 1)` softly floored at 0, light left `exp(−x / Clarity)`, colour `mix(Colour, Deep colour, 1 − that)`; Clarity 0: no depth | light comes in only through the air: a pool is deep at its floor and walls, and liquid at the edge of liquid and rock together (a film on a slope, a stream by a wall) has b about a half and stays its Colour. (Planned `blur(C·O) / blur(O)` lit pools through thin walls with air behind them; Water courses are carved out of solid rock, so the drawing field of a test scene is carved too, `sets::carved`) |
| Blur (as built) | an `R16Float` ladder: half-size base level, then halvings with the `[1 3 3 1] / 8` kernel (four bilinear taps); read with a cubic B-spline texel by texel (16 reads), level `log2(σ_px / 1.53)` blended between its two neighbours, each level read in a loop over the level number | any Clarity at any zoom from one ladder; smooth. The loop: where liquids of different Clarity meet, neighbouring pixels want different levels, and the cloud's software card takes one level for a block of pixels, which drew a bright line at the seam (test `liquids_of_different_clarity_meet_without_a_line`) |
| Liquid rim (as built) | the liquid's own Colour a little lightened (`colour + (1 - colour) × 0.22`) within `max(3 × scale, 1.25)` px of the outline, gone within that of rock | "a thin lighter rim along the surface", not along the rock it rests on; 0.35 towards white read as a pale cartoon outline, and lightening the deep body made holes deep in a pool ringed in grey |
| Rock | `Rock` colour, cover `clamp(0.5 - d × scale, 0, 1)` from the solid field (bilinear), 25 % darker within `max(3 × scale, 1.5)` px of its face | "an edge smoothed against the background and a slightly darker rim" |
| Order | back, then liquid, then rock on top | splats that spill into rock are hidden; the liquid is drawn up to the face |
| Output | `Rgba8Unorm` holding sRGB (view format `Rgba8UnormSrgb` allowed for 2b), ±0.5/255 ordered dither (4×4 Bayer) before rounding | no banding in long depth ramps; fixed, so a frame is the same every time |
| Drops not drawn | not finite, or outside the layout's nodes (the engine's `alive`) | drops that left the course wait to be let go two frames later |
| Inputs | `PlainLook { back, rock, liquids: Vec<PlainLiquid { colour, deep, clarity }> }`, sRGB `[f32; 3]`; a drop's liquid is `kind & 15`; a kind past the list takes the list's last (none: `PlainLiquid::default()`, the Water Look card's defaults); default rock `#6b5e52`, back the rock at half | what 2b reads from the cards; the drops' own colours are left for part 4's mixing; a light grey rock outshone the water |
| Stored drops | `StoredDrops::new(device, queue, &[Drop])`, the engine's buffer layout; the pinned and test pictures use hand-built drop sets (`draw::sets`, feature `test-scenes`), never a physics run; drops files (`save_drops` / `load_drops`) for dumps of real runs looked at by hand | "never from live physics"; hand-built sets need no binary file in the repository and still cover pools, a stream, a film, spray, two liquids, drops that left |

Every number above lives as a `const` in `draw/plain.rs` (and the matching WGSL `const`); tuning changes only
those. Each tuned value and the pictures behind it go in "As built".

## Review focus

1. **An empty body, and a body whose buffers grew.** `draw` with `count` 0 still draws the back and the rock;
   after `Water::add` replaced the buffers, the next draw reads the new ones (bind groups are made each draw).
   Test `an_empty_body_draws_the_rock_and_the_back` (Task 2) and `a_grown_body_draws_its_new_drops` (Task 3).
2. **Zoom far out and far in.** At scale 0.05 a lone drop still shows (a pixel or more); at scale 8 nothing
   breaks and the depth level is clamped to the ladder's top. Test `a_lone_drop_shows_at_any_zoom` (Task 2) and
   `zooming_in_far_keeps_the_body_whole` (Task 3).
3. **Picture sizes that are odd, tiny or too big.** 1 × 1, 3 × 7, 1081 × 1921 draw; a side over 8192 is clamped
   with a message, never a wgpu panic. Test `odd_and_tiny_pictures_draw` (Task 2).
4. **A liquid's kind past the looks given, and Clarity 0.** No panic, no black: the last look given (or the
   default) is used; Clarity 0 draws the Colour flat. Test `a_kind_without_a_look_takes_the_last` and
   `clarity_zero_is_flat` (Task 3).
5. **Liquid that rests on rock, and a film one drop thick on a slope.** No line of back between a resting pool
   and its floor; the film is one unbroken piece and stays the Colour (not deep). Test
   `a_pool_is_drawn_up_to_its_floor` and `a_film_one_drop_thick_is_one_piece` (Task 3).

---

## File structure

| Path | What it is |
| --- | --- |
| `crates/bpe-water/src/draw/mod.rs` | `pub mod plain; pub mod stored; #[cfg(feature = "test-scenes")] pub mod sets;` and the re-exports |
| `crates/bpe-water/src/draw/plain.rs` | `PlainGpu` (pipelines, one a device), `Plain` (one picture's textures and the field), `View`, `PlainLook`, `PlainLiquid` |
| `crates/bpe-water/src/draw/stored.rs` | `StoredDrops`: `&[Drop]` uploaded in the engine's buffer layout |
| `crates/bpe-water/src/draw/sets.rs` | Hand-built drop sets for tests, pinned pictures, the look check and pace (`bench`, `screen_pool`) |
| `crates/bpe-water/src/shaders/plain.wgsl` | `vs_splat`/`fs_splat`, `vs_full`, `fs_base`, `fs_down`, `fs_show` |
| `crates/bpe-water/tests/plain.rs` | The drawing's behaviour tests |
| `crates/bpe-water/tests/plain_pinned.rs`, `tests/golden-plain.txt` | The pinned picture (270 × 480, tiles, the owner's card) |
| `crates/bpe-water/tests/shots.rs` | Films and stills drawn by `Plain`; `plain_pace` (by hand) |
| `crates/bpe-kit/tests/water_core.rs` | Today's core (today's drawing) beside the all-drops engine (plain drawing); no grid option |
| `crates/bpe-kit/tests/full_size_look.rs` | A `water` group: the bench sets at full size |
| Removed | `src/bridge.rs`, `src/shaders/bridge.wgsl`, `src/plain.rs` (the processor drawing), `Water::feed`, `grid::Water::feed` |

---

### Task 1: Stored drops and the hand-built sets

**Files:** Create `src/draw/mod.rs`, `src/draw/stored.rs`, `src/draw/sets.rs`; modify `src/lib.rs`.

**Interfaces (produces):**
- `pub struct StoredDrops { xv: wgpu::Buffer, nf: wgpu::Buffer, id: wgpu::Buffer, count: u32 }`,
  `StoredDrops::new(device: &wgpu::Device, queue: &wgpu::Queue, drops: &[Drop]) -> StoredDrops` (xv = place and
  speed, nf = (0, 0, 1, 0), id = (kind, serial, colour, 0); at least 16 bytes so an empty set binds),
  `StoredDrops::buffers(&self) -> DropBuffers<'_>`.
- `sets::Bench { pub field: SolidField, pub drops: Vec<Drop> }`; `sets::bench(cell: f32) -> Bench`: a 1080 × 1920
  screen round (0, 0) with the course box (hollow, 24 px walls), a slope block and a disc; a pool on the floor
  whose top is a gentle wave (`det_sin`), a stream 20 px wide falling into it, a film one drop thick resting on
  the slope `wall_off` (0.25 cell) off it, a second liquid (kind 1) as a blob touching the pool, spray (lone drops
  at sub-pixel offsets, pairs 1, 2 and 3 spacings apart), one drop outside the layout and one at NaN. Every place
  is on the resting lattice plus a fixed hash wobble of at most 0.1 spacing. `sets::screen_pool(cell: f32, share:
  f32) -> Bench`: the screen's box filled to `share` of its height (two thirds at 2 px is about 1.4 million
  drops, at 8 px about 86,000: the spec's speed targets).
- `sets::look() -> PlainLook`: the Water Look and Rock defaults, a second liquid in a contrasting colour.

- [ ] **Step 1:** Unit tests in `sets.rs`: `the_bench_has_every_part` (drops of kind 0 and 1, at least one drop
  out of the layout, the stream's drops are 20 px across ±1 spacing), `screen_pool_counts_match_the_targets`
  (2 px, 2/3: 1.3 to 1.5 million; 8 px: 80,000 to 92,000), `sets_are_the_same_every_time` (two calls give equal
  drops, bit for bit).
- [ ] **Step 2:** Build the sets and `StoredDrops`. Run `cargo test -p bpe-water --lib draw`; expected: pass.
- [ ] **Step 3:** Commit "bpe-water: stored drops and hand-built drop sets for the drawing".

### Task 2: The plain drawing

**Files:** Create `src/draw/plain.rs`, `src/shaders/plain.wgsl`, `tests/plain.rs`.

**Interfaces (produces):**
- `pub struct View { pub lo: Vec2, pub scale: f32, pub width: u32, pub height: u32 }` (world point at the top left,
  picture pixels a world pixel); `View::screen(scale: f32) -> View` (the 1080 × 1920 screen round (0, 0)).
- `pub struct PlainLiquid { pub colour: [f32; 3], pub deep: [f32; 3], pub clarity: f32 }` (sRGB 0..1, world px),
  `Default` = Water Look's `#2f9bff`, `#0b2a4f`, 200. `pub struct PlainLook { pub back: [f32; 3], pub rock:
  [f32; 3], pub liquids: Vec<PlainLiquid> }`, `Default` = back `#1e2028`-like dark, rock `#c8c8cc`-like light.
- `PlainGpu::new(device: &wgpu::Device, queue: &wgpu::Queue) -> Arc<PlainGpu>`.
- `Plain::new(gpu: Arc<PlainGpu>, field: &SolidField, rest: f32) -> Plain` (`rest`: how far off a face drops rest,
  in cells: the engine's `Numbers::wall_off`); `set_field(&mut self, field: &SolidField)`;
  `draw(&mut self, drops: &DropBuffers, view: &View, look: &PlainLook)` (records and submits; makes or remakes its
  textures when the picture size changes); `output(&self) -> &wgpu::Texture`; `read(&self) -> Vec<u8>` (rgba8,
  sRGB, rows packed); `draw_many(&mut self, drops, view, look, times: u32) -> std::time::Duration` (for pace:
  `times` draws in one submission, waited for).

- [ ] **Step 1:** Failing tests in `tests/plain.rs` (all on `test_device()`, drawing `StoredDrops`):
  `a_pool_is_drawn_where_its_drops_are` (a lattice block: inside its box shrunk by a spacing every pixel is
  liquid-coloured, outside grown by two spacings every pixel is back or rock),
  `the_same_drops_draw_the_same_picture_twice` (bytes equal),
  `an_empty_body_draws_the_rock_and_the_back` (count 0: the disc's middle is rock, a corner is back),
  `a_lone_drop_shows_at_any_zoom` (scale 1, 0.25 and 0.05: some pixel within 2 px of it is not back),
  `drops_that_left_are_not_drawn` (a NaN drop and one past the layout: the picture equals the empty one),
  `odd_and_tiny_pictures_draw` (1 × 1, 3 × 7, 1081 × 1921: reads the right number of bytes).
- [ ] **Step 2:** Run `cargo test -p bpe-water --test plain`; expected: fails to compile (no `Plain`).
- [ ] **Step 3:** The shader and the passes, as in "Decisions": `vs_splat` (instance = drop; six corners of a
  quad of radius `r` picture px round the drop, or a quad off the picture for drops not drawn), `fs_splat` (two
  targets), `fs_base` (half size: `C·O` and `O` into mip 0 of an `Rg16Float` ladder), `fs_down` (mip k from mip
  k − 1), `fs_show` (full size, into `Rgba8Unorm`). The field goes to the card as a storage buffer of node
  distances in world px clamped to ±64 cells, read bilinearly. The uniform holds the view, the layout, `rest`,
  the scale-derived widths and 16 liquids. Ladder levels: until a side reaches 1.
- [ ] **Step 4:** Run `cargo test -p bpe-water --test plain`; expected: pass.
- [ ] **Step 5:** Commit "bpe-water: the plain drawing on the card".

### Task 3: How it looks: depth, rims, rock, and tuning by eye

**Files:** `src/draw/plain.rs`, `src/shaders/plain.wgsl`, `tests/plain.rs`.

- [ ] **Step 1:** Tests: `deep_water_is_darker_than_thin_water` (the bench at 3 px, scale 1: the pool 300 px below
  its top is nearer Deep colour than the stream's middle, and luma falls going down the pool's first 200 px),
  `the_rim_is_lighter_than_the_body_under_it` (the pool's top, 1 px inside the outline against 12 px inside),
  `rock_has_a_smooth_edge_and_a_darker_rim` (across the disc's edge there are in-between pixels; 1 px inside the
  face is darker than 20 px inside), `a_pool_is_drawn_up_to_its_floor` (no back-coloured pixel between a resting
  pool and its floor at 8, 3 and 2 px), `a_film_one_drop_thick_is_one_piece` (along the bench film no gap of
  back), `draft_and_final_outlines_agree` (`screen_pool` at 8 and 2 px: the cover masks differ only within one
  draft spacing, 4 px, of the outline), `a_kind_without_a_look_takes_the_last`, `clarity_zero_is_flat` (pool
  colour the same at 20 and 400 px deep), `a_grown_body_draws_its_new_drops` (a real `Water` with a fill, drawn,
  then `add` past its room, drawn: the new drops show), `zooming_in_far_keeps_the_body_whole` (scale 8 on the
  pool's middle: all liquid, no back).
- [ ] **Step 2:** Run, fix until they pass.
- [ ] **Step 3:** Tune by eye on the cloud's software card: the bench at 8, 3 and 2 px at full size and its 1:1
  crops, and dumps of real runs (`shots` with `WATER_DUMP`, Task 5) of weird, slope, ledge and pool at 6 and
  3 px. Look for jagged edges, banding, seams in the depth, speckled holes in moving water, a rim that reads as
  an outline drawn round the water, and a row of dots for a film. Write each change of a number and why in "As
  built".
- [ ] **Step 4:** Commit "bpe-water: the plain drawing's depth, rims and rock".

### Task 4: The pinned picture

**Files:** Create `tests/plain_pinned.rs`, `tests/golden-plain.txt`; modify `.github/workflows/gpu-pictures.yml`
(bless and check `cargo test -p bpe-water --test plain_pinned`, and add the file to the commit list),
`docs/notes/checks.md` (the pinned list).

- [ ] **Step 1:** `the_plain_drawing_draws_as_pinned`: `sets::bench(8.0)` and `sets::screen_pool(8.0, 0.4)`
  drawn at 270 × 480 (`View::screen(0.25)`), 10 × 10 tile averages hex-encoded as `lit_flat_pinned.rs` does,
  compared through `bpe_gfx::card_golden` with each tile allowed 1/255; on another card it checks the pictures
  are not all back and prints "not compared". One file for debug and release (no physics is involved).
- [ ] **Step 2:** Bless locally once so the file exists (it names lavapipe), push, then start "Refresh GPU
  pictures" on the branch so the owner's card makes it. Expected: the workflow commits `golden-plain.txt` made on
  the RX 9070 XT, and CI's card job passes it.
- [ ] **Step 3:** Commit "bpe-water: the plain drawing's pinned picture".

### Task 5: Films, water_core.rs and the look check on the plain drawing; the bridge removed

**Files:** Modify `tests/shots.rs` (only `screen()`, where stills and film frames are drawn, a `WATER_DUMP`
option, and a new `plain_pace`), `crates/bpe-kit/tests/water_core.rs`, `crates/bpe-kit/tests/full_size_look.rs`,
`src/lib.rs`, `src/drops/mod.rs` and `src/grid/mod.rs` (remove `feed`); delete `src/bridge.rs`,
`src/shaders/bridge.wgsl`, `src/plain.rs`.

**Interfaces:**
- `shots`: frames and stills drawn by `Plain` from `water.drop_buffers()` (no read-back for the picture; the
  stats still read back on whole seconds), `View::screen(WATER_FILM_SCALE)` and `View::screen(1.0)`. With
  `WATER_DUMP=1` each still also writes `<label>-<t>s.drops` (`probe`-free: a little-endian file of
  `x, y, vx, vy: f32, kind, colour: u32` per drop, read by `stored::load_drops(path) -> Vec<Drop>`).
- `plain_pace` (ignored): `screen_pool(2.0, 2/3)` at 1080 × 1920 and `screen_pool(8.0, 2/3)` at 1080 × 1920 and
  540 × 960; prints the card, drops, picture size and ms a draw (20 draws in one submission after 3 to warm up).
- `water_core.rs`: `WATER_CORES` takes `old` and `drops` (default both). `old` is drawn by today's renderer as
  before; `drops` by `Plain` with walls `LEAST_WALL` cells thick, `PlainLook` from the Water Look card's defaults.
  `the_bridge_draws_every_drop` becomes `the_plain_drawing_draws_every_drop` (the same pool, the same coverage
  bounds, drawn by `Plain`).
- `full_size_look.rs`: group `water`: `water-bench-c8`, `-c3`, `-c2` and `water-pool-c2`, plus each `.drops` file
  named in `LOOK_WATER` (comma list) drawn on the bench's course (`LOOK_WATER_CELL`, default 6).

- [ ] **Step 1:** Make the changes; `cargo test -p bpe-kit --test water_core -- --test-threads=2` and
  `cargo test -p bpe-water` pass; `cargo build -p bpe-kit --tests` builds `full_size_look`.
- [ ] **Step 2:** Run `LOOK_OUT=<scratch> LOOK_ONLY=water cargo test --release -p bpe-kit --test full_size_look --
  --ignored --nocapture` and look at the pictures and crops.
- [ ] **Step 3:** Notes: `docs/notes/water.md` (a "Part C3" section at the end), `CLAUDE.md` (the end of the
  `bpe-water` line: "and the plain drawing on the card (`draw/`)"), `crates/bpe-water/Cargo.toml` description.
- [ ] **Step 4:** Commit "Water: films, water_core and the look check on the plain drawing; the bridge goes".

### Task 6: The card run, the review and the merge

**Files:** Create `card/all-drops-core-c3/run.ps1` (from C1's on branch `claude/project-thread-zl1ff7`), removed
before merging; modify this plan ("As built and measured").

- [ ] **Step 1:** Push, open the pull request as a draft, CI runs.
- [ ] **Step 2:** Card run by Remote Control on `F:\!Ai Stuff\BallPhysicsEngine` (project memory `liquid-speed`,
  `all-drops-trial`, `all-drops-core`): build in WSL `bpe-ci`, run natively: `shots` for every reference shot
  (slope, ledge, pool, course, beads, weird) at Fine (6 px), Extreme (3 px) and 2 px, `WATER_FILM_SCALE=1`
  (full-size films), stills at the trial's moments, Surface pull 0.2 and 1 on slope, ledge and beads at 3 px; Water
  Weird from Aedan's file through `water_core.rs`; `plain_pace`; the look check's `water` group. Films joined with
  the PC's ffmpeg; results pushed to its own branch as `[skip ci]`. Films to project files `water-core/c3/`. End
  the session when it reports.
- [ ] **Step 3:** Judge: films side by side with C1's (`water-core/c1/`) and the trial's
  (`water-core/all-drops-trial/`) at the same shots and details, and against real water (round droplets, smooth
  body, no streaks or bands, depth reads as depth); full-size stills and 1:1 crops for jagged edges, banding,
  seams; flicker between film frames; the speed targets (8 ms at 1.4 million drops at 1080 × 1920; 2 ms at
  86,000). Write the verdict in "As built and measured".
- [ ] **Step 4:** One Opus review of the branch (superpowers:requesting-code-review), findings fixed.
- [ ] **Step 5:** Remove `card/`, merge when every check is green, tell Aedan with the films.

## Things not to do

- Don't read the drops back to draw them (only the measuring runs read back, for their tables).
- Don't change any physics, the engine's numbers, `scenes.rs`, `probe.rs` or `engine.rs` (C2's).
- Don't add lights, shine, foam, bubbles, mist or the six looks: those are part 6, built on this drawing's amount
  picture.
- Don't use the drops' own colours for the tint (part 4's mixing will), nor a random number anywhere.
- Don't draw from live physics in the pinned picture.
- Don't keep the bridge, or anything that only it used.

## As built and measured

(Filled in as the part is built.)
