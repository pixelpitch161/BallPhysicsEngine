# Liquid part 1: the liquid itself, with colour and mixing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new **Liquid** thing whose drops are simulated on the graphics card (MLS-MPM), fill shapes or the scene,
pour from spouts, are pushed aside by balls, carry their own colour and mix (Paint, Light, Blend, Marble, Keep apart),
and draw as one smooth surface. Decoration only (beside the main world); balls are pushed by it in part 2.

**Architecture:** `bpe-gfx/src/liquid_sim/` owns the card side (buffers, compute pipelines, one submit per liquid
step, copy-on-the-card clones, a late count read). `bpe-kit/src/things/liquid/` owns the thing kind, its cards, the
settings it reads, births (Fill and Pour), the walls band, the balls list and the shared body (one per world, in the
first Liquid thing's state). The renderer's existing `LiquidPass` gains a second blob source: drops read straight from
the card's buffer with an indirect draw.

**Tech Stack:** Rust, wgpu compute (WGSL), egui 0.37 inspector, the existing `#[derive(Settings, Choice)]`.

**Spec:** `docs/superpowers/specs/2026-11-17-liquid-thing-design.md` (sections 5, 6.1, 6.4, 6.5, 8 and 10 for this
part; section 12 item 1 is the scope).

## Global Constraints

- Every number is a setting with a `span`, past-span typing allowed (`range` only where a value is meaningless, such
  as a negative width); defaults make plain water.
- Deterministic: same scene, same card, same Detail = the same drops. Everything summed on the card with atomics sums
  in fixed point (`i32`); no float atomics; no randomness but `bpe_core::dice` on the CPU.
- A frame with no Liquid thing stays byte-identical; the Particles thing and its Liquid layer draw exactly as before.
- Existing goldens must not move (the liquid is beside the main world in this part).
- No new built-in or example scenes (Aedan 2026-11-17). Test scenes behind `test-scenes` are fine.
- Every editor control named for kittest; colours only from `bpe-studio/src/style/`.
- Plain-language labels and help (the owner is not a programmer).

## Review Focus

1. A Liquid thing with no card named on its thread (the editor's main playback, the sound engine, a test without
   a device): it must hold no drops, never panic, and the status line says "Liquid needs the graphics card".
2. Huge Detail on a huge area (Extreme, 4000 px tall shape): node count capped (`MOST_NODES`), cell grows instead,
   drops capped (`MOST_DROPS`), never an out-of-memory panic or a buffer over the device limit.
3. Seeking back and forth and checkpoints: a cloned state that steps first must copy on the card and re-upload its
   walls band; the result must equal a straight run (pinned by a test).
4. NaN or infinite settings typed in (Thickness, Level, Pour speed): read as their default or clamped, never NaN
   drops (a NaN drop poisons the grid). The shader also kills any non-finite drop.
5. Two Liquid things, one inside a shape and one on its own, plus a second shape: shared area holds both; the shape
   liquid's drops that leave its shape out of a gap are removed at the area's edge, the free liquid's stop at it.

---

## Decisions (what the spec left to the builder)

- **Solver: MLS-MPM, weakly compressible, in grid units** (cell = 1). Drops carry place, speed, the APIC matrix `C`,
  `J` (volume ratio), colour and strength, liquid index, birth serial, a stir "kick" and a material offset. Per
  substep: clear the grid, P2G (mass, momentum, volume as `i32` fixed point: mass and volume × 2^20, momentum ×
  2^13, each drop's contribution clamped to ±2^24 before scaling so nothing wraps), grid update (momentum → speed
  stored as `f32` bits in the same slots; walls band and the area's edges), G2P (APIC gather, then per-drop
  collisions with walls, balls and edges; `J = 1 / max(volume density, 0.05)` measured from the grid).
- **Stress:** `σ = −p·I + μ(C + Cᵀ)`; `p = E·(1 − J)` when `J < 1`; for `1 ≤ J < 1 + tear`, `p = −E·pull·(J − 1)`
  (cohesion, the Surface pull); past `1 + tear` no pull (the liquid breaks into drops, the Spray). `E` per drop =
  `stiff × weight`.
- **Substeps:** gravity is real (scene px/s² ÷ cell px), so stiffness must beat `g·H`. Wave speed
  `c = sqrt(g·H / ε)` with `H` the area's height in cells and `ε` = 0.12 at Springiness 0 down to 0.015 at 1 (linear);
  substeps per liquid step `n = clamp(ceil(c·dt / 0.4), 1, 64)`; `E = c²`. Viscosity `ν = 400·Thickness²·(cell px /
  8)⁻²` cells²/s, capped at `0.2 / dt_sub`. Speeds capped at `0.45 / dt_sub` cells/s (no drop crosses half a cell
  in a substep). Settles = damping `v *= exp(−k·dt_sub)`, `k = 8·Settles²` per second.
- **Detail** (Rough, Normal, Fine, Very fine, Extreme) = cell 12, 8, 6, 4, 3 px; two drops a cell each way (4 a cell),
  so drops per 100 × 100 px = 278, 625, 1 111, 2 500, 4 444 (the number shown under the chips is from
  `drops_per_100px`). Preview and Export values; the export uses Export Detail (part 1 pushes no balls). The export
  names itself with `bpe_kit::things::liquid::exporting_for_now()` (a thread-local guard, like the card naming).
- **Area:** fixed at the build: Shot at the start (the camera's shot at 0 s, grown to every Liquid thing's parent
  shape's bounds), Fit the shapes (the shapes' bounds alone, the shot when none), Custom (as Shot at the start plus
  Margin on every side). Read from the first Liquid thing in tree order (as Detail and Steps a second are: the shared
  body has one grid). At most `MOST_NODES` = 2 000 000 grid nodes: the cell grows until it fits.
- **Edges:** drops of a Liquid thing that isn't in a shape stop at the area's edges (it "fills the scene's edges");
  drops of one in a shape are removed when they leave the area.
- **Shared body:** all Liquid things step in one body held by the first Liquid thing's state (the *host*). Each
  Liquid thing keeps its own state (`LiquidState`) with its own read settings; the host's `body: Option<Body>` holds
  the card buffers. At most `MOST_LIQUIDS` = 16 Liquid things share it; later ones are left out with a problem note.
- **The Look in part 1** is a plain card (no layers yet): Colour (Its own / One colour), the Mixing group, and a
  plain Surface group mapped onto the existing `LiquidStyle` (code Plastic, or Matte when Shine is 0). The whole
  shared body draws as one surface with the host's Look; each drop keeps its own colour. Part 3 brings layers,
  the 15-chip Surface, lighting and gradients by depth/speed/age.
- **Cards and slots:** own settings = **Liquid** (what it's like, Detail, Area); Motion slot = **Fill** card
  (`liquid.fill`: Fill at the start, Level, colour, strength, Most drops; layers **Pour** `liquid.pour` in
  `LayerSection::Force`); Bounce slot = **Meets** card (`liquid.meets`: Balls push it; layers **Other liquid**
  `liquid.other`); Look slot = **Look** (`liquid.look`). The inspector shows a Liquid thing's cards in the order
  Liquid, Fill, Meets, Look (a per-kind slot order).
- **Pour amount** is Width × Speed (px² a second): births a liquid step = width·speed·dt ÷ drop area, the fraction
  carried in the state (`LiquidState.pour_carry`), so the amount doesn't change with Detail.
- **Fill Level** rising adds whole rows of drops between the last filled line and the new one; falling removes this
  liquid's drops above the new line (the shader's per-liquid cut line). The row lattice is fixed from the region's
  bottom, so rises are deterministic.
- **Most drops:** births carry a per-liquid serial; drops with `serial < born − most` are removed (the oldest born).
- **Colour per drop:** paints (`#[setting(paint)]`, follows Time, Place, Order) evaluated on the CPU at each birth.
- **Mixing** runs every `MIX_EVERY` = 4 liquid steps on neighbours found by binning drops into grid cells (count,
  scan, scatter, then each drop reads the 3 × 3 cells); every sum is fixed point, so the bin order (from atomics)
  can't change the result. Pair rule from a 16 × 16 table: Mix (style, speed, when) or Layer (no colour; a
  separating push and a relative-weight lift). Mix styles: **Blend** (OKLab lerp), **Light** (RGB lerp lifted to the
  brighter of the two's peak), **Paint** (Kubelka–Munk over 8 bands from 3 smooth basis curves, back through their
  exact pseudo-inverse so equal colours stay equal), **Marble** (no colour passes; drawn sharp), **Keep apart** (no
  colour passes; between two liquids it also separates like Layer). Strength weights who wins. Mix also blends
  Weight and Thickness through the per-drop material offset.
- **Swirl detail** is a sharper colour kernel in the draw (left to the builder by spec 14): colours are weighted by
  `w^(1 + 12·swirl)` in a second splat target and divided by their own sum, so streaks stay crisp without grain.
  Marble draws with Swirl detail at least 0.8.
- **Mixes when:** Always; Only when stirred (each pair's gate = relative speed ÷ 30 cells/s, capped at 1); Only on
  hits (gate = the drop's kick, set to 1 when a ball touches it, fading at 2 a second).
- **Readout:** "· Liquid: 43,210 drops, 1.2 ms" (count and time a step from the late read: wall clock between a
  step's submit and its count's map). "· Liquid needs the graphics card" when a body has no card.
- **Throttle:** at most 3 liquid steps in flight; the stepping thread waits on the oldest (`PollType::Wait` with its
  submission index). Never the UI thread: the main playback holds no Liquid (beside only).

## Files

- Create `crates/bpe-gfx/src/liquid_sim/mod.rs` (types, `LiquidGpu`, `Body` buffers, `step`, `read`, clone),
  `crates/bpe-gfx/src/liquid_sim/tests.rs`, `crates/bpe-gfx/src/shaders/liquid_sim.wgsl`.
- Modify `crates/bpe-gfx/src/liquid.rs`, `shaders/liquid.wgsl` (drops source, second target), `lib.rs` (exports).
- Create `crates/bpe-kit/src/things/liquid/{mod.rs, cards.rs, state.rs, sim.rs, births.rs, walls.rs, draw.rs,
  tests.rs}`.
- Modify `crates/bpe-kit/src/lib.rs` (register), `starters.rs`, `scenes.rs` (liquid test scenes),
  `crates/bpe-core/src/settings.rs` (new sections), `crates/bpe-kit/tests/restarts.rs`, create
  `crates/bpe-kit/tests/golden_liquid.rs` + `golden-liquid*.txt`, `crates/bpe-kit/tests/full_size_look.rs` (scenes
  `liquid-water`, `liquid-mixing`).
- Modify `crates/bpe-cli/src/lib.rs` (export detail guard), `crates/bpe-studio/src/{tree.rs (drop icon),
  inspector/slots.rs (order, Fill and Meets lists), look_editor/particles.rs (shared list helpers made `pub(crate)`),
  particle_job.rs + app/panels.rs (readout)}`.
- Docs: `docs/notes/liquid.md` (new area notes, "as built"), one index line in `CLAUDE.md`.

---

### Task 1: The card side — buffers, pipelines and one liquid step

**Files:** `bpe-gfx/src/liquid_sim/mod.rs`, `shaders/liquid_sim.wgsl`, `liquid_sim/tests.rs`, `lib.rs`.

**Interfaces (Produces):**

```rust
pub const MOST_DROPS: u32 = 4_000_000;
pub const MOST_LIQUIDS: usize = 16;
pub const MOST_NODES: u32 = 2_000_000;
pub const MIX_EVERY: u32 = 4;
#[repr(C)] #[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Drop { pub x: [f32; 2], pub v: [f32; 2], pub c: [f32; 4], pub colour: [f32; 4] /* rgb, strength */,
    pub j: f32, pub kind: u32 /* liquid index; bit 8 = stops at edges */, pub serial: u32, pub kick: f32,
    pub mat: [f32; 2] /* weight, thickness offsets */, pub spare: [f32; 2] }            // 80 bytes
#[repr(C)] pub struct LiquidGpu { pub a: [f32; 4] /* stiff, pull, tear, damping */,
    pub b: [f32; 4] /* viscosity, weight, gravity ×, balls push (0/1) */,
    pub c: [f32; 4] /* mix style, mix rate (0..1 per mixing), mixes when, cut line (cells, −inf none) */,
    pub d: [u32; 4] /* serial floor, thickness→viscosity scale bits, 0, 0 */ }
#[repr(C)] pub struct StepGpu { pub grid: [u32; 4] /* cols, rows, substeps, liquids */,
    pub time: [f32; 4] /* dt_sub, gravity x, gravity y (cells/s²), speed cap */, pub counts: [u32; 4],
    pub flags: [u32; 4] /* mixing this step, walls changed, balls, step number */, pub balls: [f32; 4],
    pub ball_at: [u32; 4], pub paint: [[f32; 4]; 8] /* Paint's 8 bands × rgb basis */, pub back: [[f32; 4]; 8],
    pub liquids: [LiquidGpu; MOST_LIQUIDS], pub pairs: [[u32; 4]; 64] /* 16 × 16 rules, 4 a row */ }
pub struct WallNode { pub index: u32, pub n: [f32; 2], pub dist: f32, pub v: [f32; 2] }   // band entries
pub struct BallGpu { pub x: [f32; 2], pub v: [f32; 2], pub r: f32, pub spare: [f32; 3] }
pub struct StepIn<'a> { pub step: StepGpu, pub births: &'a [Drop], pub walls: Option<&'a [WallNode]>,
    pub balls: &'a [BallGpu], pub ball_cells: &'a [u32] /* starts then items */ }
pub struct LiquidCard; impl LiquidCard { pub fn for_device(&wgpu::Device, &wgpu::Queue) -> Arc<LiquidCard> }
pub struct Body { /* drops, counts, grid, solid, scratch, reader */ }
impl Body { pub fn new(gpu: Arc<LiquidCard>, cols: u32, rows: u32) -> Body; pub fn step(&mut self, input: &StepIn);
    pub fn read(&self) -> Vec<Drop>; pub fn bound(&self) -> u32; pub fn count_hint(&self) -> u32;
    pub fn step_ms(&self) -> f32; pub fn bytes(&self) -> usize; pub fn source(&self) -> DropsSource }
impl Clone for Body  // copy on the card, solid band marked stale
```

The card: `liquid_sim::LiquidCard::for_device` reuses swarm's naming: the body takes its device from
`bpe_gfx::swarm::on_this_thread()` (one card naming for every GPU thing); `LiquidCard::of(&SwarmGpu)`.

- [ ] **Step 1: Failing tests in `liquid_sim/tests.rs`** (each gets its device from `crate::test_device()`):
  - `a_box_of_water_settles_level`: 40 × 40 grid, 1600 drops seeded in the lower half, walls on the edges, 600 steps
    of 2 substeps: every drop inside the box, the top 10% of drops' heights spread less than 3 cells, no NaN.
  - `two_runs_are_identical` and `a_copy_steps_like_the_original`: the same inputs twice give byte-equal `read()`;
    `clone()` after 50 steps then 50 more equals 100 straight.
  - `paint_mix_of_blue_and_yellow_is_green`: interleaved blue `(0.1,0.2,0.9)` and yellow `(0.95,0.85,0.1)` drops,
    Mix style Paint, rate 1, gravity 0, 40 steps: the mean colour has `g > r` and `g > b`.
  - `light_mix_of_red_and_green_is_yellow` (mean `r > 0.6`, `g > 0.6`, `b < 0.2`), `marble_keeps_its_colours`
    (after stirring every drop is still within 0.01 of red or blue), `keep_apart_passes_no_colour`.
  - `drops_leave_past_the_area_and_past_most`: a drop moving off an open edge is gone; serial floor removes the oldest.
  - `nan_settings_give_no_nan_drops`: a NaN gravity and stiffness: every drop finite or removed.
- [ ] **Step 2:** `cargo test -p bpe-gfx --lib liquid_sim -- --test-threads=2`: fails to compile (no module).
- [ ] **Step 3: Implement.** WGSL entry points, all `@workgroup_size(256)`: `cs_births` (append at `counts[1] + i`,
  one thread then adds the births), `cs_clear_solid`/`cs_put_solid` (only when `flags[1]`), per substep
  `cs_clear_grid`, `cs_p2g`, `cs_grid`, `cs_g2p`; every `MIX_EVERY`th step `cs_bin_clear`, `cs_bin_count`,
  `cs_scan_local`, `cs_scan_sums`, `cs_bin_place`, `cs_mix`, `cs_mix_apply`; then `cs_flag` (alive: finite, inside the
  area or stopped at it, `serial ≥ floor`, below its cut line), the swarm's integer prefix compaction
  (`cs_scan_local`/`cs_scan_sums`/`cs_scatter`, reused for both bins and drops through a `which` word). One command
  buffer a liquid step; the count copied to a map-read staging buffer when none is waiting (as `CountReader`). Keep
  each pipeline at ≤ 8 storage bindings. Ball lookup: a coarse grid (8 cells) of ball indices built on the CPU.
  Paint basis (blue, green, red by band 410…690 nm): `[0.9,1,0.5,0.1,0.03,0.02,0.02,0.03]`,
  `[0.08,0,0.5,0.9,0.85,0.25,0.05,0.03]`, `[0.02,0,0,0,0.12,0.73,0.93,0.94]`; `back` = `(BᵀB)⁻¹Bᵀ` worked out in Rust
  once (`paint_back()`), test `paint_back_undoes_the_basis` (B·rgb then back = rgb within 1e-5).
- [ ] **Step 4:** the same command: all pass.
- [ ] **Step 5:** `git commit -m "Liquid part 1: drops simulated on the graphics card"`.

### Task 2: The thing, its cards and the settings it reads

**Files:** `bpe-kit/src/things/liquid/{mod.rs, cards.rs}`, `bpe-core/src/settings.rs`, `bpe-kit/src/lib.rs`,
`starters.rs`.

**Interfaces (Produces):** `liquid::KIND = "liquid"`, `LiquidKind` (name "Liquid", icon "drop", family "Liquid",
slots `[Motion, Bounce, Look]`, slot labels Fill / Meets / Look, `can_stick_to_screen` false), `LiquidSettings`
(own: `thickness 0.05, weight 1, surface_pull 0.3, springiness 0.5, spray 0.3, settles 0.2, gravity 1, detail
Normal, export_detail Fine, steps_a_second Steps240, area ShotAtTheStart, margin 100`), `Detail` (Rough … Extreme,
`cell_px()`, `drops_per_100px()`), `Steps { Steps120, Steps240 }`, `Area { ShotAtTheStart, FitTheShapes, Custom }`;
`FillSettings` (`fill true, level 0.35, colour` paint default one stop `#2f7fe0`, `strength 1, most 400000`),
`PourLayer` (`on true, place (0,0), aim 90, width 40, speed 300, spread 4, colour` paint, `strength 1`),
`MeetsSettings` (`balls_push true`), `OtherLiquid` (`liquid: Target, meeting: Meeting {Mix, Layer}, style:
RuleStyle {AsThisLiquids, Paint, Light, Blend, Marble, KeepApart}, own_speed false, speed 0.5`), `LookSettings`
(`colour_from: ColourFrom {ItsOwn, OneColour}, colour #2f7fe0, mix_style MixStyle {Paint, Light, Blend, Marble,
KeepApart} = Blend, mix_speed 0.3, mixes_when MixesWhen {Always, OnlyWhenStirred, OnlyOnHits}, swirl_detail 0.5,
level 0.5, softness 1.5, roundness 6, shine 0.6, light_angle −45, opacity 1, in_front false`). Card ids
`liquid.fill`, `liquid.pour`, `liquid.meets`, `liquid.other`, `liquid.look`. Sections added to `SECTION_ORDER`:
"Liquid", "Detail", "Area", "Fill", "Pour", "With the balls", "Other liquids", "Colour", "Mixing" (Surface exists).
Detail, Export detail, Steps a second and Area aren't keyframable; the physics numbers and Level, Pour on / width /
speed / aim are keyframable and pulsable.

- [ ] **Step 1: Failing tests** in `liquid/tests.rs`: `a_new_liquid_has_its_three_cards` (add_thing gives Fill,
  Meets, Look of the liquid kinds, no Physics card kinds), `liquid_cards_fit_only_liquids`,
  `nan_settings_read_as_defaults` (each number set to NaN reads as its default through `read`).
- [ ] **Step 2–4:** implement, register in `registry()` and `sims()`/`draws()`, `starter_kind` maps the three slots;
  run `cargo test -p bpe-kit --lib things::liquid`.
- [ ] **Step 5:** commit.

### Task 3: The shared body in the world — build, births, walls, balls, stepping

**Files:** `liquid/{state.rs, sim.rs, births.rs, walls.rs}`, `liquid/tests.rs`.

**Interfaces:** `LiquidState { id, parent_shape: Option<ThingId>, settings, fill, pours, meets, others, look,
filled_to: f32, pour_carry: Vec<f32>, born: u32, host: bool, body: Option<Arc<Body>>, area: Area rect (px), cell,
walls_key: u64, step: u32 }` (Clone; `Arc::make_mut` before a step copies on the card). `pub fn state_of(world, id)
-> Option<&LiquidState>`, `pub fn hash_of(world) -> u64` (drops read back: count, places, colours; tests only),
`pub fn inputs_hash(st) -> u64` (what the sim reads), `pub fn exporting_for_now() -> ExportGuard`. `LiquidSim`:
`in_main` false, `steps_own_cards` true, `card_key_restarts_beside(Look, mix_style | mix_speed | mixes_when)` true,
`settings_changed` re-reads.

- Build: every Liquid thing's state; `after_build` of the first in order (the host) works out the area and cell,
  makes the body (when a card is named), seeds every liquid's Fill (two-by-two lattice per cell from the region's
  bottom up to Level, inside the parent shape's contours when it has one (`outline::inside_any`), otherwise the area,
  jittered ±0.15 cell by `dice::hash([seed, liquid, row, col])`), and steps nothing.
- `after_step` of the host: liquid steps due `= world time × rate` (as particles' `due`); each step gathers births
  (Fill rises, every on Pour), the walls band (`walls.rs::band`: every non-passing wall's contours, two-sided, nodes
  within 2 cells; solid walls' insides too; wall speed from the outline as it was last step; uploaded only when the
  outlines' hash changes), balls (`balls_in`, the Meets switch), the liquids table and the pairs table (no rule: Mix
  with the earlier liquid's style and speed), and calls `Body::step`. Notes `gpu_memory`.
- [ ] **Step 1: Failing tests** (GPU, card named with `use_for_now(test card)`): `water_fills_a_box_and_settles`
  (scene `liquid_in_a_box`, 3 s: drops inside the box, surface flat within 2 cells), `a_pour_adds_drops`,
  `rising_level_adds_and_falling_level_removes`, `a_ball_pushes_the_liquid_aside` (no drop centre inside the ball after
  1 s), `balls_push_it_off_lets_balls_through`, `two_liquids_layer_light_on_top` (heavy over light, Layer, 6 s: light
  liquid's mean height above the heavy's), `seeking_replays_the_liquid` (seek 3 → 1 → 3 equals a straight 3),
  `no_card_no_drops` (nothing named: `body` is `None`, no panic, stepping runs).
- [ ] **Step 2–4:** implement; `cargo test -p bpe-kit --lib things::liquid -- --test-threads=2`.
- [ ] **Step 5:** commit.

### Task 4: Drawing the body

**Files:** `bpe-gfx/src/liquid.rs`, `shaders/liquid.wgsl`, `liquid/draw.rs`, `bpe-kit/tests/liquid_pictures.rs`.

**Interfaces:** `bpe_gfx::DropsSource { device, drops, counts, origin: [f32; 2], cell: f32 }` and
`Liquid.drops: Option<DropsSource>`; `LiquidStyle.c[3]` = Swirl sharpness (0 = the old colour). New pipeline
`splat_drops` (vertex reads `drops` storage, `draw_indirect` on `counts`), both splats write a second R16Float target
(the colour weights); the surface divides by it when sharp. Blob radius `2.2 × spacing`, weight `1 / 3.8`.

- [ ] **Step 1: Failing tests:** `a_liquid_in_a_box_draws_a_smooth_surface` (270 × 480, the box scene at 2 s: the
  liquid's colour covers the lower box, the top edge one run of rows, no holes), `old_liquid_layer_pictures_are_
  unchanged` (the existing `tests/liquid_pictures.rs` stays green — run it), `swirl_detail_keeps_streaks_sharp`.
- [ ] **Step 2–4:** implement; run `cargo test -p bpe-kit --test liquid_pictures --test unlit_pictures`.
- [ ] **Step 5:** commit.

### Task 5: Editor, export, readout

**Files:** `bpe-studio` (tree icon, slot order, Fill and Meets lists, readout), `bpe-cli` (export guard).

- The Fill card: its own grid then the Pour list ("Pours", "+ Add pour", empty words "No pours: the liquid only
  fills. Add a pour for a spout."); the Meets card: grid then the Other liquids list ("+ Add rule", empty words "No
  rules: liquids that meet mix."). Reuse `look_editor::{list_header, picked_settings, layer_list}` through a small
  `look_editor::liquid::show_list`.
- Kittest: `liquid_cards_read_liquid_fill_meets_look` (labels in that order down the inspector),
  `add_pour_adds_a_pour_layer`.
- `particles_readout`'s sibling `liquid_readout(drops, ms)`; the particle job's `Snapshot` carries the host's
  `(count_hint, step_ms, has_card)`.
- [ ] Run `cargo test -p bpe-studio liquid` and `cargo test -p bpe-cli`; commit.

### Task 6: Goldens, restarts, look check, notes

- `tests/golden_liquid.rs`: scenes `scenes::liquid_scenes` (water in a box, a pour into a cup shape, two liquids
  mixing Paint, two layering): each second 1…4 the drop count and `hash_of`; seeking gives the same; the balls hit as
  without the liquid. Bless in debug and release.
- `restarts.rs`: beside outcome also hashes each `LiquidState` with `inputs_hash`.
- `full_size_look.rs`: `liquid-water`, `liquid-mixing`; run it, open the PNGs and crops, look for grain and seams.
- `docs/notes/liquid.md` "as built", CLAUDE.md one index line.
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, the relevant tests; commit.

## Not doing in part 1

Pushes balls, Floats or sinks, the three events, drops of ink (part 2); the full Surface, lighting, foam, gradients
by depth/speed/age (part 3); Stretch, Heat, React, Rain, Drains, Currents, Magnets, Un-mix, Picture in the liquid,
Fades, Shapes paint it (part 4); Sound (part 5). No changes to the Particles thing.

## As built

Built as planned, with these changes (the long version is `docs/notes/liquid.md`):

- **Walls band**: each node carries the wall's line distance (signed, out of the outline), half its thickness (−1
  for a filled wall) and its normal; a drop is kept on its own side of a hollow wall's line from whichever node is
  nearest. The first version (distance from the nearest node's own face) let drops through a thin ring when the
  nearest node was on the far side. The balls push drops before the walls do, so a ball never pushes one through.
- **Copies**: the host's body is copied on the card before its band is checked, so a copied world puts its walls on
  again (it used to step one copy without walls).
- **Sections**: the Liquid's groups are "How it flows", "Detail", "Area", "Fill", "With the balls", "Its colour",
  "Colour mixing" (plain "Liquid", "Colour" and "Mixing" are other layers' groups and would have moved them).
- **Family** "Liquids" (a heading "Liquid" would clash with the kind's own entry in the add menu).
- **Goldens** pin the births and the drop counts; the drops' places are pinned on the machine only (seek and a second
  run), as a graphics driver's arithmetic can differ between machines.
- **Test scenes**: `scenes::liquid_scenes` has two ("Liquid water", "Liquid paint mixing"), in the new scene's ring.
