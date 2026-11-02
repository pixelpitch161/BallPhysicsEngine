# Marks part 4A (canvas side: covers, scrape, claims, numbers) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Canvases that balls remove (Cover, Powder, Fog), brushes that scrape and chip, colour-war claims on Paint canvases, and the live numbers (covered, revealed, wet, shares) every canvas reports to the world. Plus the Add-menu starting points and five demos.

**Architecture:**
- The simulation's grid stays the truth. A grid now has a *rest* amount (0 for paint, 255 cover and fog, 160 powder), and per-tile counters that make the numbers cheap.
- Paint canvases keep being drawn from their logged marks on the GPU. Scrape and Chip marks are logged for Paint canvases only and replayed as wipes.
- Cover, Powder and Fog are drawn straight from the grid: the canvas's drawing part copies the grid into an RGBA8 image each frame; the composite samples it. Claimed ground is a second small map, drawn the same way, under the paint.
- Every step the canvas hands its numbers to the world with `World::report_canvas` (the shared interface, already on the branch).

**Tech Stack:** Rust workspace: bpe-runtime, bpe-kit, bpe-gfx (wgpu + WGSL), bpe-studio (egui 0.37).

**Spec:** `docs/superpowers/specs/2026-11-02-marks-part4-cover-scrape-claim-design.md`. Parts 1 to 3 (`2026-11-01-marks-layer-design.md`, `2026-11-02-marks-part2-skins-design.md`, `2026-11-02-marks-part3-rich-paint-design.md`) still hold wherever this spec is silent. Read `CLAUDE.md` first, then the whole spec, before Task 1. This plan refers to the spec's sections as "spec §N".

**The other stream.** A second build thread runs `docs/superpowers/plans/2026-11-02-marks-part4b-numbers.md` at the same time: captions, rules, End the video, the synth input and the Meter. **Do not touch** `crates/bpe-vm`, `crates/bpe-synth`, `crates/bpe-kit/src/things/text/`, `crates/bpe-kit/src/modules/rules/`, `crates/bpe-kit/src/graphs.rs`, `crates/bpe-kit/src/music/`, `crates/bpe-cli/`, `crates/bpe-studio/src/timeline/`, `crates/bpe-studio/src/follow_menu.rs`, `crates/bpe-runtime/src/actions.rs`, `crates/bpe-runtime/src/canvas_numbers.rs` or `bpe-gfx/src/shaders/scene.wgsl`. If you think one of them needs a change, stop and say so in your thread instead.

## Global Constraints

- **Branch.** Start from the interface commit:

```sh
git fetch origin claude/project-thread-bv0ih5
git checkout -B <your designated branch> FETCH_HEAD
git log --oneline -3   # the plan commits on top, then 90f729b (the shared canvas numbers interface)
```

- **Golden logs.** Every existing line of `crates/bpe-kit/tests/golden.txt`, `golden-debug.txt`, `golden7.txt`, `golden7-debug.txt`, and the old engine's goldens, stays byte-identical. Only the five new demo lines may be added, at the end of `golden.txt` and `golden-debug.txt` (Task 14). Grids, claims, drops, numbers and teams never enter the log hash or the fingerprint.
- **Existing canvases are unchanged.** A Paint canvas with no Scrape, Chip or Claim brush simulates and draws exactly as before: same marks, same grid hash, same pictures. All existing `canvas_pictures.rs`, `paint_pictures.rs` and `pictures.rs` tests pass unchanged.
- **Deterministic, and seek equals straight play.** The same run gives the same grids, claim maps, teams, drops and numbers. Seeking back and forth gives exactly the straight-run state. No `HashMap`/`HashSet` iteration in simulation code (`BTreeMap`/`Vec` only), no wall clock, no `rand`. Random choices come from `crate::things::canvas::sim::mix(step, item)`.
- **Nothing here touches physics.** The canvas sim reads the world and writes only its own state, `World::report_canvas`, `World::set_teams` and the log's marks and samples.
- **WGSL never names a field or variable `target`, `active` or `move`.**
- **Settings groups.** Every *module* setting group is in `bpe_core::SECTION_ORDER` (test `every_module_setting_has_a_known_section`): add "Scrape" and "Claim" right after "Splat". Thing groups are free: the canvas gains "Underneath" and "Colour war".
- **Build environment.** Every cargo command runs with `export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 BPE_REQUIRE_GPU=1`. After every GPU test run, check the output does not contain "skipping". Every **new** GPU test helper must `panic!("no GPU adapter")` when `BPE_REQUIRE_GPU` is set and no adapter is found.
- **Formatting and lints.** `cargo fmt` (max width 120) and `cargo clippy --workspace --all-targets -- -D warnings` before each commit. `cargo fmt` rewraps lines, so re-read a file after formatting it before any scripted replacement.
- **Commits.** One commit per task with the message given. End every message with the attribution lines your thread is given. Push after every commit (`git push -u origin <branch>`; never force).
- **No review in this stream.** The one Opus review of part 4 runs in stream B after it merges your branch. Do not dispatch reviewers.

## Review Focus (the final reviewer in stream B checks these first)

1. **Paint canvases are byte-identical** without scrape/claim brushes: grid hash and pictures. Tests: Task 3 `paint_grid_is_unchanged_by_the_rest_value`, Task 10 `a_paint_canvas_without_claims_draws_as_before`, plus all existing picture tests.
2. **Seek equals straight** for every new kind, claims with grow, fog drops, and the numbers. Test: Task 8 `every_new_demo_seeks_to_the_straight_run`.
3. **The grid and the picture agree on chips.** Test: Task 9 `rust_chip_shapes_match_the_stamp_shader`.
4. **Counters are exact.** Test: Task 3 `counters_match_a_full_scan` (random writes, compared to a brute-force scan).
5. **Golden diffs are only the five added lines.** Task 14.
6. **Speed.** Removable canvas and claims-with-grow steps stay under 1 ms mean in release (Task 14 `speed.rs`).

## Shared definitions (used by many tasks; do not rename)

```rust
// bpe-runtime/src/events.rs: MarkShape gains two variants at the end.
pub enum MarkShape { Stroke, Splat, Ring, Star, Crack, Burst, Dots, Smear, Drip, Picture, Lift, Bleed, Scrape, Chip }
// bpe-gfx/src/canvas/stamp.rs shape_number: ... Lift 10, Bleed 11, Scrape 12, Chip 13.
// bpe-kit grid ink_of: Scrape 0, Chip 0 (they are not paint).

// Mark fields for the new shapes:
//   Scrape: from→to capsule, radius → radius_to, angle = previous piece's direction, variant bits 1-2 =
//           the ribbon joints exactly as a Stroke (bit 0 always 0), within = the piece's within.
//   Chip:   from == to = centre; radius; angle = turn; variant = layout_and_squash (bits 0-4, as a
//           Splat's) | shape << 5 (0 splat, 1 ring, 2 star, 3 crack, 4 burst, 5 dots).
//   Both:   colour = colour_to = [0.0, 0.0, 0.0, strength] with strength = scrape_strength clamped 0..1.

// bpe-kit/src/things/canvas/grid.rs
pub const REST_COVER: u8 = 255;
pub const REST_POWDER: u8 = 160;
pub const REST_FOG: u8 = 255;

// bpe-gfx/src/canvas/shapes.rs (pure Rust, no GPU): the chip shapes, twin of canvas_stamp.wgsl.
pub fn chip_distance(shape: u32, q: glam::Vec2, r: f32, variant: u32) -> f32;
//   shape 0 splat (variant bits 0-4), 1 ring, 2 star, 3 crack (variant & 7), 4 burst, 5 dots (variant & 7).
//   q is in the chip's own turned frame (the same q fs_stamp computes).
```

---

### Task 0: Environment

**Files:** none.

- [ ] **Step 1:** Install the system packages and the Windows target:

```sh
apt-get update && apt-get install -y libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers \
  libxkbcommon-x11-0 libxkbcommon0 xvfb xdotool imagemagick gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

- [ ] **Step 2:** Create your branch from the interface commit (Global Constraints, "Branch").
- [ ] **Step 3:** In every shell: `export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 BPE_REQUIRE_GPU=1`.
- [ ] **Step 4:** GPU check: `cargo test -p bpe-gfx --lib canvas_marks_stamp_their_shapes 2>&1 | tee /tmp/gpu.txt; grep -c skipping /tmp/gpu.txt` prints `0`. Do not go on until it does.
- [ ] **Step 5:** Baseline: `cargo test -p bpe-kit --test golden` and `cargo test -p bpe-runtime` pass. Any later failure in them is yours.
- [ ] **Step 6:** `df -h /`. Whenever free space is under 8 GB, `rm -rf target/debug`.

---

### Task 1: Runtime: the Scrape and Chip mark shapes

**Files:**
- Modify: `crates/bpe-runtime/src/events.rs` (the `MarkShape` enum only).
- Modify: `crates/bpe-kit/src/things/canvas/grid.rs` (`ink_of`, `stamp`'s match).
- Modify: `crates/bpe-gfx/src/canvas/stamp.rs` (`shape_number`).
- Modify: any other exhaustive `match` on `MarkShape` the compiler names (skins, evolve). New arms do nothing.

- [ ] **Step 1:** Add, after `Bleed`:

```rust
    /// A ball scraping along its track: a capsule like a Stroke's (from → to, radius → radius_to,
    /// the joints in variant bits 1 and 2, angle the previous piece's direction) that takes paint
    /// away. colour[3] is the strength, 0 to 1. Not paint: it leaves no ink.
    Scrape,
    /// A piece knocked out by a hit: a splat shape at `from` of `radius`, turned by `angle`.
    /// variant bits 0-4 are the splat's layout and squash, bits 5-7 the shape (0 splat, 1 ring,
    /// 2 star, 3 crack, 4 burst, 5 dots). colour[3] is the strength. Not paint.
    Chip,
```

- [ ] **Step 2:** `ink_of`: `MarkShape::Lift | MarkShape::Bleed | MarkShape::Scrape | MarkShape::Chip => 0`. `CanvasGrid::stamp`: the same four `=> return`. `shape_number`: `Scrape => 12.0, Chip => 13.0`.
- [ ] **Step 3:** `cargo build --workspace --all-targets`, then add a do-nothing arm wherever the compiler asks. Do **not** add `_ =>` catch-alls.
- [ ] **Step 4:** `cargo test -p bpe-runtime -p bpe-kit --lib` passes.
- [ ] **Step 5:** Commit `feat(marks): scrape and chip mark shapes`.

---

### Task 2: Settings: canvas kinds, underneath, colour war, scrape and claim brushes

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/mod.rs` (enums, `CanvasSettings`, `Default`, tests).
- Modify: `crates/bpe-kit/src/things/canvas/sim.rs` (`affects_physics` only).
- Modify: `crates/bpe-kit/src/modules/marks/mod.rs` (enums, `MarksBrushes`, `Default`).
- Modify: `crates/bpe-core/src/settings.rs` (`SECTION_ORDER`: "Scrape", "Claim" after "Splat").
- Modify: `crates/bpe-kit/src/lib.rs` (re-export the new enums beside `CanvasKind`, `SplatShape` and friends).

- [ ] **Step 1: enums** in `canvas/mod.rs`:

```rust
pub enum CanvasKind { #[default] Paint, /** Balls scrape it off. */ Cover, /** Sand or snow balls plough through. */ Powder, /** Misted glass balls wipe clear. */ Fog }
pub enum CoverLook { #[default] Matte, Foil }
pub enum PowderLook { #[default] Sand, Snow }
pub enum FogLook { #[default] Frost, Steam }
/// What shows where a cover is scraped away.
pub enum Underneath { #[default] Scene, Colour, Gradient, Picture, Text }
```

All derive `Clone, Copy, Debug, Default, PartialEq, Eq, Choice` like `CanvasStyle`, with a `///` doc on each variant. Update the `CanvasKind` doc comment ("What brushes do to the canvas.").

- [ ] **Step 2: new `CanvasSettings` fields**, in this order after `opacity` (exact attributes):

```rust
    #[setting(label = "Cover look", when = "kind=cover", group = "Look")]
    pub cover_look: CoverLook,
    #[setting(label = "Powder", when = "kind=powder", group = "Look")]
    pub powder_look: PowderLook,
    #[setting(label = "Fog", when = "kind=fog", group = "Look")]
    pub fog_look: FogLook,
    /// How high scraped powder piles up along the track.
    #[setting(label = "Ridges", when = "kind=powder", range(0, 1), group = "Look")]
    pub ridges: f32,
    /// How much the fog blurs what is behind it.
    #[setting(label = "Blur", when = "kind=fog", range(0, 40), unit = "px", group = "Look")]
    pub blur: f32,
    /// How thick the fog looks.
    #[setting(label = "Density", when = "kind=fog", range(0, 1), group = "Look", keyframe)]
    pub density: f32,
    #[setting(label = "Underneath", when = "kind=cover", group = "Underneath")]
    pub under: Underneath,
    #[setting(label = "Colour", when = "kind=cover and under=colour|text", group = "Underneath")]
    pub under_colour: Color,
    #[setting(label = "Colours", when = "kind=cover and under=gradient", group = "Underneath")]
    pub under_colours: Gradient,
    #[setting(label = "Picture", when = "kind=cover and under=picture", unit = "picture", group = "Underneath")]
    pub under_picture: String,
    #[setting(label = "Text", when = "kind=cover and under=text", group = "Underneath")]
    pub under_text: String,
    #[setting(label = "Text size", when = "kind=cover and under=text", range(8, 600), unit = "px", group = "Underneath")]
    pub under_text_size: f32,
    #[setting(label = "Text colour", when = "kind=cover and under=text", group = "Underneath")]
    pub under_text_colour: Color,
    /// Seconds a scraped place takes to fill back in; 0 never.
    #[setting(label = "Grows back", when = "kind=cover|powder|fog", range(0, 30), unit = "s", group = "Over time")]
    pub grows_back: f32,
    /// How often steam runs down in drops from what is wiped.
    #[setting(label = "Drops", when = "kind=fog and fog_look=steam", range(0, 1), group = "Over time")]
    pub drops: f32,
    /// The colour war's tiles: 0 is smooth ground.
    #[setting(label = "Tile size", when = "kind=paint", range(0, 200), unit = "px", group = "Colour war")]
    pub tile_size: f32,
    /// How fast claimed ground spreads by itself.
    #[setting(label = "Grow", when = "kind=paint", range(0, 1), group = "Colour war")]
    pub grow: f32,
    /// Whether teams can take each other's ground.
    #[setting(label = "Take over", when = "kind=paint", group = "Colour war")]
    pub take_over: bool,
    /// How dark the lines between teams are.
    #[setting(label = "Border", when = "kind=paint", range(0, 1), group = "Colour war")]
    pub border: f32,
```

  Defaults: `cover_look Matte, powder_look Sand, fog_look Frost, ridges 0.6, blur 14.0, density 0.55, under Scene, under_colour Color::rgb(0x1b,0x10,0x30), under_colours Gradient::even(&[Color::rgb(0xff,0x5c,0xa8), Color::rgb(0x3e,0x9b,0xff)]), under_picture "", under_text "YOU WIN", under_text_size 160.0, under_text_colour Color::rgb(0xff,0xd2,0x3e), grows_back 0.0, drops 0.5, tile_size 0.0, grow 0.0, take_over true, border 0.0`. (`Gradient` and `Color` come from `bpe_core`; check `Gradient::even` exists as used in `scenes.rs`.)

- [ ] **Step 3: existing fields get kind conditions.** Change only the `when` strings:
  - `starts_as`: `when = "kind=paint"`.
  - `start_colour`: `when = "starts_as=colour or kind=cover|powder|fog"`.
  - `style`, `detail`, `dry_time`, `bleed`, `drips`, `fade`: `when = "kind=paint"`.
  - `gloss`: `"kind=paint and style=wet_paint"`. `grain`, `edge_dark`: `"kind=paint and style=watercolour"`. `glow_size`, `glow_strength`: `"kind=paint and style=neon_ink"`.
  - `light_angle`: `"style=wet_paint or kind=powder"`.
  - `layer`, `blend`: `"kind!=fog"`.
  - **First check the grammar.** Read `SettingDef::applies` in `crates/bpe-core/src/settings.rs`: confirm `and`, `or`, `!=` and `=a|b` are parsed and how `and`/`or` mix. If `a and b=c|d` or `a or b=c|d` is not supported as written, extend the parser in the smallest way (with a test in `settings.rs`), never by changing these strings' meaning.
- [ ] **Step 4: brushes** in `modules/marks/mod.rs`:

```rust
/// What a chip knocked out by a hit looks like.
pub enum ChipShape { #[default] Splat, Ring, Star, Crack, Burst, Dots }
/// Who claimed ground belongs to.
pub enum ClaimTeam { /** One team per thing. */ ByThing, /** One team per colour. */ #[default] ByColour }
```

  New `MarksBrushes` fields, after the Splat fields and before `colour_from`, in this order:

```rust
    #[setting(label = "Scrape path", group = "Scrape", simple)]
    pub scrape_on: bool,
    #[setting(label = "Width from", when = "scrape_on", group = "Scrape")]
    pub scrape_width_by: WidthBy,
    /// Size: half the width is this many ball radii. Fixed: this many times 10 px. Speed: px = this × speed / 100.
    #[setting(label = "Width", when = "scrape_on", range(0.1, 4), group = "Scrape", keyframe)]
    pub scrape_width: f32,
    #[setting(label = "Chip at hits", group = "Scrape")]
    pub chip_on: bool,
    #[setting(label = "Chip shape", when = "chip_on", group = "Scrape")]
    pub chip_shape: ChipShape,
    #[setting(label = "Chip size", when = "chip_on", range(2, 400), unit = "px", group = "Scrape", keyframe)]
    pub chip_size: f32,
    #[setting(label = "Size by speed", when = "chip_on", range(0, 1), group = "Scrape")]
    pub chip_by_speed: f32,
    #[setting(label = "Hit speed", when = "chip_on", range(0, 2000), unit = "px/s", group = "Scrape")]
    pub chip_min_speed: f32,
    /// How much one pass takes away.
    #[setting(label = "Strength", when = "scrape_on or chip_on", range(0, 1), group = "Scrape")]
    pub scrape_strength: f32,
    #[setting(label = "Claim path", group = "Claim", simple)]
    pub claim_on: bool,
    #[setting(label = "Width from", when = "claim_on", group = "Claim")]
    pub claim_width_by: WidthBy,
    #[setting(label = "Width", when = "claim_on", range(0.1, 4), group = "Claim", keyframe)]
    pub claim_width: f32,
    #[setting(label = "Claim at hits", group = "Claim")]
    pub claim_hits: bool,
    #[setting(label = "Hit size", when = "claim_hits", range(2, 400), unit = "px", group = "Claim")]
    pub claim_hit_size: f32,
    #[setting(label = "Size by speed", when = "claim_hits", range(0, 1), group = "Claim")]
    pub claim_hit_by_speed: f32,
    #[setting(label = "Hit speed", when = "claim_hits", range(0, 2000), unit = "px/s", group = "Claim")]
    pub claim_min_speed: f32,
    #[setting(label = "Team", when = "claim_on or claim_hits", group = "Claim")]
    pub claim_team: ClaimTeam,
```

  Defaults: `scrape_on false, scrape_width_by Size, scrape_width 1.0, chip_on false, chip_shape Splat, chip_size 30.0, chip_by_speed 0.5, chip_min_speed 80.0, scrape_strength 1.0, claim_on false, claim_width_by Size, claim_width 1.2, claim_hits false, claim_hit_size 40.0, claim_hit_by_speed 0.5, claim_min_speed 50.0, claim_team ByColour`.
- [ ] **Step 5: `affects_physics`** (canvas sim): add to the not-physics list `"cover_look" | "powder_look" | "fog_look" | "blur" | "density" | "under" | "under_colour" | "under_colours" | "under_picture" | "under_text" | "under_text_size" | "under_text_colour" | "border"`. `kind`, `ridges`, `grows_back`, `drops`, `tile_size`, `grow`, `take_over` stay physics (they rebuild the world).
- [ ] **Step 6: tests first** (in `canvas/mod.rs` tests), then make them pass:
  - `each_kind_shows_its_settings`: for each kind (with every other choice at its default), the keys that apply equal exactly:
    - Paint: `kind fit brushes_from layer starts_as style detail dry_time bleed drips fade blend opacity tile_size grow take_over border` (plus `size` only when fit=custom, so not here).
    - Cover: `kind fit brushes_from layer start_colour blend opacity cover_look under grows_back`.
    - Powder: `kind fit brushes_from layer start_colour light_angle blend opacity powder_look ridges grows_back`.
    - Fog: `kind fit brushes_from start_colour opacity fog_look blur density grows_back`.
    - Fog with `fog_look=steam` adds `drops`. Cover with `under=text` adds `under_colour under_text under_text_size under_text_colour`; `under=gradient` adds `under_colours`; `under=picture` adds `under_picture`.
    - Compare as sorted lists. Use the existing `applying` helper pattern with a `get` closure over several keys.
  - Update `canvas_style_knobs_show_for_their_style`: its `get` must also answer `kind` = `paint`.
  - `canvas_defaults_keep_existing_canvases` stays, and add: `CanvasSettings::from_params(&Params::default()) == CanvasSettings::default()` (old scene files load as Paint).
  - In `marks/mod.rs` tests: `brush_defaults_round_trip` (`from_params(to_params())`) and that the defaults have all new switches off.
  - `bpe-core`'s `every_module_setting_has_a_known_section` passes.
- [ ] **Step 7:** `cargo test -p bpe-core -p bpe-kit --lib` passes. Commit `feat(marks): settings for covers, powder, fog, scrape and claims`.

---

### Task 3: The grid's rest value, counters, scraping and growing back

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/grid.rs`.

**Interfaces produced:**

```rust
impl CanvasGrid {
    pub fn new(size: Vec2) -> Self;                       // == surface(size, 0, 4.0, 300_000): unchanged behaviour
    pub fn surface(size: Vec2, rest: u8, start_cell: f32, max_cells: u64) -> Self;
    pub fn rest(&self) -> u8;
    pub fn real_cells(&self) -> u64;                      // width * height
    pub fn off_cells(&self) -> u64;                       // cells whose amount != rest (running total)
    pub fn deficit(&self) -> u64;                         // Σ max(0, rest - amount) (running total)
    pub fn revealed(&self) -> f32;                        // rest == 0: 0.0; else deficit / (rest * real_cells)
    pub fn covered(&self) -> f32;                         // rest == 0: off_cells / real_cells; else 1 - revealed
    pub fn scrape(&mut self, m: &Mark, ridges: f32);      // Scrape or Chip marks; others do nothing
    pub fn grow_back(&mut self, step_amount: u8);         // every tile with off > 0, toward rest
    pub fn image(&self) -> Vec<u8>;                       // RGBA8, width*height*4: [amount, 0, 0, 255] per real cell, row-major
}
```

- [ ] **Step 1: tests first** (`grid.rs` tests):
  - `paint_grid_is_unchanged_by_the_rest_value`: a `CanvasGrid::new(1080×1920)` stamped with the marks of the existing `capsule_covers_its_cells` and splat tests gives the same `hash()` as before. Write the expected hashes down **before** changing any code (run the test body on the unchanged code and paste the numbers in).
  - `surface_starts_at_rest`: `surface(400×400, 255, 2.0, 600_000)`: cell size 2, every cell amount 255, `off_cells 0`, `deficit 0`, `revealed 0`, `covered 1`.
  - `surface_cell_size_doubles_past_its_cap`: `surface(1080×1920, 255, 2.0, 600_000)` has cell 2 (518,400 cells); `surface(1080×4000, 255, 2.0, 600_000)` has cell 4.
  - `scrape_takes_cover_away`: a Scrape mark (−100,0)→(100,0) radius 10, strength 1 on a 255 cover: cells at (0,0) and (90,0) are 0, (0,20) is 255; revealed > 0 and equals (cells changed × 255) / (255 × real cells).
  - `half_strength_takes_half`: strength 0.5 (`colour[3] = 0.5`, so s = 128) takes `round(128/255 × 255) = 128` from a 255 cell: 127 left; twice → 0 (never below 0).
  - `scrape_wipes_paint`: on a `new` (rest 0) grid stamped red at amount 200, a strength-1 scrape leaves amount 0 under it; strength 0.5 (s = 128) leaves `200 × 127 / 255 = 99`.
  - `ridges_rise_beside_the_track`: powder (rest 160), ridges 1.0, strength 1 scrape radius 20 along x: the cell at (0, 26) (inside 1.6 r = 32, outside r) is `160 + round(1 × 255/255 × 160 × 0.35) = 216`; (0, 40) stays 160; (0, 0) is 0. Ridges 0 leaves (0, 26) at 160. A ridge never exceeds 255.
  - `ridges_respect_the_ribbon_cut`: a piece with variant bit 1 (2) and angle 0 (cut behind the start): a cell at (−26, 0) behind the start, in the band, stays 160.
  - `chip_covers_its_shape`: a Chip mark of shape 1 (ring, `variant = 1 << 5`), radius 40, at the origin: a cell at (34, 0) (on the ring) is 0 and the centre cell stays 255.
  - `grow_back_returns_to_rest`: scrape a cover, then `grow_back(ceil(255 / (2 s × 30)))` 60 times → every cell 255 and `off_cells 0`, `deficit 0`. A powder ridge cell at 216 comes back down to 160.
  - `counters_match_a_full_scan`: on a 300×300 powder grid, 500 scrapes and chips from a SplitMix sequence (`mix(k, 0)` for positions, radii 2..40, strengths 0..1, ridges 0.7) interleaved with 20 `grow_back(9)` calls; after each operation `off_cells()` and `deficit()` equal a brute-force scan over `get(x, y)` for all real cells.
  - `image_has_one_pixel_per_cell`: `image().len() == w*h*4`; a scraped cell's pixel is `[0, 0, 0, 255]`, an untouched one `[255, 0, 0, 255]`.
- [ ] **Step 2:** Run them, see them fail (they don't compile yet).
- [ ] **Step 3: implement.**
  - Fields: `rest: u8`, `tile_off: Vec<u16>`, `tile_deficit: Vec<u32>`, `off_total: u64`, `deficit_total: u64`. None of these are hashed: **leave `hash()` exactly as it is** (it hashes width, height and every cell, padding included).
  - `surface` is today's `new` with the start cell and cap as parameters and the blank tile `Arc::new([Cell { amount: rest, ..Cell::default() }; TILE_CELLS])` shared by every tile. `new(size)` calls `surface(size, 0, START_CELL_SIZE, MAX_CELLS)`.
  - **One private write helper**, used by `paint`, `set` and the new code. Nothing else may call `Arc::make_mut` on tiles:

```rust
    /// Writes `new` into `slot` of `tile`, keeping the tile's and the grid's counters, and only
    /// un-sharing the tile when the cell changes.
    fn write(&mut self, tile: usize, slot: usize, new: Cell) {
        let old = self.tiles[tile][slot];
        if old == new {
            return;
        }
        let rest = self.rest;
        let off = |c: Cell| (c.amount != rest) as i64;
        let short = |c: Cell| (rest as i64 - c.amount as i64).max(0);
        let d_off = off(new) - off(old);
        let d_def = short(new) - short(old);
        self.tile_off[tile] = (self.tile_off[tile] as i64 + d_off) as u16;
        self.tile_deficit[tile] = (self.tile_deficit[tile] as i64 + d_def) as u32;
        self.off_total = (self.off_total as i64 + d_off) as u64;
        self.deficit_total = (self.deficit_total as i64 + d_def) as u64;
        Arc::make_mut(&mut self.tiles[tile])[slot] = new;
    }
```

  - `covered()` keeps its meaning for paint grids but now returns `off_total / real_cells` (no scan). The `bytes()` count adds the counters.
  - **Footprints.** Refactor the cell loop of `stamp` into a private visitor, so stamp and scrape share the cuts:

```rust
    /// Visits every grid cell whose centre is within `reach` (canvas px) of the segment
    /// m.from → to, with `stamp`'s ribbon and drip cuts applied, passing the cell, whether it is
    /// inside `radius`, and its squared distance in 16.16 units.
    fn capsule_cells(&self, m: &Mark, to: Vec2, radius: f32, reach: f32, f: &mut dyn FnMut(u32, u32, bool));
```

    `stamp` calls it with `reach == radius` and paints the cells where `inside` is true; the cut logic (`cut_before`, `joined`, `cut_after`) moves into the visitor unchanged and is applied to every visited cell. Scrape uses `ribbon = true` cuts (same bits as a Stroke). After the refactor the hashes in `paint_grid_is_unchanged_by_the_rest_value` must still match.
  - `scrape(m, ridges)`:
    - `s = (m.colour[3].clamp(0.0, 1.0) * 255.0).round() as u32`. If `s == 0` return.
    - Material grids (`rest > 0`): `take = ((s * rest + 127) / 255) as u8` (that is `round(s/255 × rest)`); `ridge = if self.rest == REST_POWDER { (ridges.clamp(0.0, 1.0) * s as f32 / 255.0 * self.rest as f32 * 0.35).round() as u8 } else { 0 }`: ridges only on powder grids.
    - Scrape: radius `(m.radius + m.radius_to) * 0.5`; visit with `reach = radius * 1.6` when `ridge > 0`, else `reach = radius`. Inside: material `amount = amount.saturating_sub(take)`; paint grid `amount = (amount as u32 * (255 - s) / 255) as u8`. Outside but visited (the band): `amount = amount.saturating_add(ridge)`.
    - Chip: for every cell in the box `from ± (1.8 + 0.6) × radius`, `q = rotate(centre − from, −angle)` (the same turned frame `fs_stamp` uses: `q = (c·o.x + s·o.y, −s·o.x + c·o.y)` with `c, s` = cos and sin of the angle), `d = bpe_gfx::chip_distance(variant >> 5 & 7, q, radius, variant & 31)`. Inside when `d <= 0`; band when `0 < d <= 0.6 × radius`.
    - Colour, `painted`, `team` and `by` of a cell are left as they are. Every write goes through `write`. Scraping never wakes a tile (`tile_tick` untouched).
  - `grow_back(g)`: for every tile with `tile_off > 0` (in index order), for every real cell of `tile_cells(t)` with `amount != rest`: move `amount` toward `rest` by at most `g` (up or down). `g == 0` does nothing.
  - `image()`: row-major over real cells, `[amount, 0, 0, 255]`.
- [ ] **Step 4:** Note `chip_distance` comes in Task 4. Do Task 4 Step 3 (the Rust shapes) first if you prefer, or stub `chip_distance` in `bpe-gfx/src/canvas/shapes.rs` to a disc (`q.length() - r`) for this task and leave `chip_covers_its_shape` `#[ignore]` with a `// Task 4` note until Task 4 replaces the stub and removes the ignore.
- [ ] **Step 5:** `cargo test -p bpe-kit --lib canvas` passes, plus every existing grid, evolve and skin test. Commit `feat(marks): grid rest value, counters, scraping and growing back`.

---

### Task 4: The chip shapes in Rust

**Files:**
- Create: `crates/bpe-gfx/src/canvas/shapes.rs` (pure Rust, `pub mod shapes;` in `canvas/mod.rs`, `pub use canvas::shapes::chip_distance;` in `bpe-gfx/src/lib.rs`).

- [ ] **Step 1:** Port from `crates/bpe-gfx/src/shaders/canvas_stamp.wgsl`, line for line, in `f32`:
  - `hash(variant, k, salt)` (PCG; use `wrapping_mul`/`wrapping_add`, `>>` on `u32`; the division by `4294967295.0` in `f32`).
  - `segment`, `smin`, `splat_paint` (as `splat`), `ring`, `star`, `crack`, `burst`, `dots`.
  - `pub fn chip_distance(shape: u32, q: Vec2, r: f32, variant: u32) -> f32`: `0 => splat(q, r, variant & 31)`, `1 => ring(q, r)`, `2 => star(q, r)`, `3 => crack(q, r, variant & 7)`, `4 => burst(q, r)`, `5 => dots(q, r, variant & 7)`, anything else `q.length() - r`.
  - Copy each constant exactly as written in the WGSL (`1.7453293`, `0.25`, `2.999`, `5.999` and so on). Keep the WGSL's order of operations.
- [ ] **Step 2: unit tests** in `shapes.rs`: the hash of `(3, 5, 12)` matches a value you compute by hand from the WGSL formula and paste in; `ring` is negative at `(0.85 r, 0)` and positive at the centre; `splat` is negative at the centre for every variant 0..32; every shape is positive beyond `1.9 r` for 64 directions and all variants.
- [ ] **Step 3:** Remove the Task 3 stub and the `#[ignore]` on `chip_covers_its_shape`. `cargo test -p bpe-gfx --lib shapes` and `cargo test -p bpe-kit --lib grid` pass.
- [ ] **Step 4:** Commit `feat(marks): chip shapes in Rust`. (The GPU comparison is Task 9.)

---

### Task 5: Brushes scrape and chip; paint skips the new kinds

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/sim.rs`.
- Modify: `crates/bpe-kit/src/testing.rs` if a helper is needed (for example a `cover_scene(kind)` builder).

**Behaviour:**
- `CanvasSim::build` picks the grid by kind: Paint `CanvasGrid::new(size)`, Cover `surface(size, REST_COVER, 2.0, 600_000)`, Powder `surface(size, REST_POWDER, 4.0, 300_000)`, Fog `surface(size, REST_FOG, 4.0, 300_000)`. Non-Paint canvases get `Evolve::new(0.0, 0.0, 0.0, sps)` (evolution off) and never pick up.
- `CanvasState` gains `pub scrapes: BTreeMap<(ThingId, u32), Ribbon>` (built empty).
- In `marks()`, a brush counts when `b.paint_on || b.splat_on || b.scrape_on || b.chip_on || b.claim_on || b.claim_hits` (claims arrive in Task 7; leave them doing nothing here). Then:
  - `paint_on && paint_now && kind == Paint` → `strokes` (as now). `splat_on && kind == Paint` → `splats` (as now). **On other kinds neither runs.**
  - `scrape_on && paint_now` → new `scrape_strokes(st, thing, b)`.
  - `chip_on` → new `chips(st, thing, b)`.
- `scrape_strokes` mirrors `strokes` without load, pickup colour or Paint-when:
  - `tune = Tune { taper: 0.0, follow: 1.0 }`, `endless = true`, `Ribbon::new(0.0, hits)` from `st.scrapes`.
  - Radius as `strokes` computes it but from `scrape_width_by`/`scrape_width`.
  - A dead ball finishes its ribbon. Pieces shorter than `MIN_STROKE` wait.
  - Each piece that `reaches` the canvas becomes a `Mark { shape: Scrape, variant: p.joints, angle: p.joins_at, within: p.within, colour: [0, 0, 0, strength], colour_to: same, .. }`.
  - `st.grid.scrape(&mark, st.settings.ridges)`; the mark goes to `self.out` **only when the canvas kind is Paint**.
  - Steam fog only: for each piece, call `st.note_edge(p.to, p.radius, normal)` (Task 6) for later drops; leave a no-op until then.
- `chips` mirrors `splats`: the same hit filter (`ours`, `e.kind == Hit`, `e.speed >= b.chip_min_speed`), but **no** `splat_onto` skin check (chips land on canvases only).
  - Stamp: a new `chip_stamp(world, e, b, pose_angle) -> Stamp` beside `hit_stamp`: `splat_look(e, mover, b.chip_size, b.chip_by_speed, 8, Turn::FaceHit, pose_angle)` gives `(radius, angle, layout)`; for shapes other than Splat the angle is `e.normal.y.atan2(e.normal.x) - pose_angle` and the layout is `layout & 7`, exactly like `hit_stamp`; `variant = layout | (chip_shape_number << 5)` with Splat 0, Ring 1, Star 2, Crack 3, Burst 4, Dots 5.
  - Mark `Chip`, `from == to == at`, `radius_to == radius`, colour as above. `st.grid.scrape(&mark, ridges)`; logged only on Paint canvases.
- [ ] **Step 1: tests first** (sim.rs tests, using the existing `testing` helpers `run`, `canvas_state`, `brushes` and friends):
  - `paint_trails_do_not_paint_covers`: a Cover canvas and balls with Paint trail and Splat on: after 2 s the grid's `off_cells()` is 0 and the log has no marks for the canvas.
  - `scraping_reveals_a_cover`: Cover, balls with Scrape path: after 2 s `revealed() > 0`, the log has **no** marks for the canvas, and the cell under each ball's position is 0.
  - `scraping_logs_wipes_on_paint`: a Paint canvas with paint trail and scrape path both on: the log has Scrape marks; each Scrape mark comes after the Stroke marks of the same step for the same ball (brush order: strokes first, then scrapes, then splats, then chips).
  - `chips_follow_hits`: Cover, chip on, `chip_min_speed 0`: every Hit event of the balls in the first 3 s has a chipped cell within `chip_size × 3` of its point (`get` == 0 somewhere in that box).
  - `slow_hits_do_not_chip`: `chip_min_speed 100000` → `revealed() == 0` with scrape off.
  - `golden_scenes_keep_their_grids`: for every scene in `scenes::marks(reg)` (the seven existing ones), the canvas grid hash at 5 s equals the value on the unchanged code. Record the seven hashes first on the base commit, then paste them in.
- [ ] **Step 2:** Fail, implement, pass. `cargo test -p bpe-kit --lib` and `cargo test -p bpe-kit --test golden` pass (goldens unchanged).
- [ ] **Step 3:** Commit `feat(marks): brushes scrape and chip`.

---

### Task 6: Growing back and steam drops in the 30 Hz tick

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/sim.rs` (`after_step`, `CanvasState`).
- Create: `crates/bpe-kit/src/things/canvas/drops.rs` (`pub mod drops;` in `canvas/mod.rs`).

**Interfaces:**

```rust
// drops.rs
#[derive(Clone, Debug, PartialEq)]
pub struct FogDrop { pub at: Vec2, pub left: f32, pub speed: f32 }   // canvas px, px of run left, px/s
pub const MAX_DROPS: usize = 64;
/// Starts drops from the edges noted this tick, moves every drop, and wipes its path into `grid`.
pub fn tick(grid: &mut CanvasGrid, drops: &mut Vec<FogDrop>, edges: &[(Vec2, u64)], chance: f32, gravity: Vec2, per_second: f32, half: Vec2);
```

- `CanvasState` gains `pub drops: Vec<FogDrop>` and `pub edges: Vec<(Vec2, u64)>` (a point and its hash seed, cleared every tick).
- `note_edge(at, radius, dir)` (from Task 5's scrape pieces, Steam fog with `drops > 0` only): pushes the two side points `at ± perp(dir) × radius` with seeds `mix(step, painter(thing, item) as u32 * 2 + side)`. `dir` is the piece's direction (`(p.to − p.from).normalize_or_zero()`); skip when zero.
- The 30 Hz tick in `after_step` now runs when `world.step.is_multiple_of(st.tick_every)` and any of: `st.evolve.on()` (Paint, as now), or the kind is not Paint and (`grows_back > 0` or (Steam and `drops > 0`)), or claims grow (Task 7). In order:
  1. Paint: `evolve::tick` exactly as now (unchanged call).
  2. Not Paint, `grows_back > 0`: `grid.grow_back(ceil(rest / (grows_back × 30)))` (as `u8`, at least 1).
  3. Steam with `drops > 0` and gravity (canvas frame, as `Tick.gravity` is computed) not zero: `drops::tick(...)` with `chance = drops × 0.15`.
- `drops::tick`:
  - For each edge in order, `u = (seed >> 40) as f32 / (1 << 24) as f32`; when `u < chance` and fewer than `MAX_DROPS` run, add `FogDrop { at, left: 60.0 + 140.0 × frac(seed >> 8), speed: 40.0 + 60.0 × frac(seed >> 16) }` where `frac(x) = (x & 0xffff) as f32 / 65535.0`.
  - Each drop moves `speed / per_second` along `gravity.normalize()`, wiping the segment old → new with a strength-1 Scrape mark of radius `max(2.5, cell / 2)` through `grid.scrape(&mark, 0.0)` (variant 0, no cuts).
  - `left` shrinks by the distance; a drop is removed when `left <= 0` or it leaves the canvas (`|at.x| > half.x || |at.y| > half.y`). Keep order with `retain`.
- [ ] **Step 1: tests first:**
  - `a_scraped_cover_grows_back`: a Cover with `grows_back 1` and **no** scraping brush (balls with the default Brushes, paint only, which do nothing on covers). Build the world, scrape a square into its grid by hand (`state_mut::<CanvasState>` then `grid.scrape(..)`), then step 1.1 s → `revealed() == 0`.
  - `powder_grows_back_both_ways`: ridge cells above rest settle to rest within `grows_back` seconds.
  - `steam_drops_run_down`: Fog Steam, `drops 1`, gravity (0, 500), balls scraping: after 3 s some cell below every ball's track is wiped further than `radius × 2` under the lowest scraped point of its column (pick one column through a ball's start and check a wiped cell exists more than 40 px below the track's lowest point). `st.drops.len() <= 64` sampled every 0.1 s.
  - `frost_has_no_drops` and `no_gravity_no_drops`: `st.drops` stays empty.
  - `drops_are_deterministic`: two runs to 4 s give the same `drops` and grid hash.
- [ ] **Step 2:** Fail, implement, pass. Commit `feat(marks): covers grow back, steam runs in drops`.

---

### Task 7: Claims

**Files:**
- Create: `crates/bpe-kit/src/things/canvas/claims.rs` (`pub mod claims;` in `canvas/mod.rs`; re-export `ClaimMap` and `colour_name` from `canvas`).
- Modify: `crates/bpe-kit/src/things/canvas/sim.rs`.

**Interfaces:**

```rust
// claims.rs
#[derive(Clone)]
pub struct ClaimMap { cell: f32, w: u32, h: u32, cells: Arc<Vec<u8>>, counts: Vec<u32>, blocks: Vec<u32> }
impl ClaimMap {
    pub fn new(size: Vec2, cell: f32) -> Self;   // w = ceil(size.x / cell).max(1), h likewise; blocks are 32×32 claim cells
    pub fn dims(&self) -> (u32, u32);
    pub fn cell_size(&self) -> f32;
    pub fn get(&self, x: u32, y: u32) -> u8;      // 0 none, else team index + 1
    pub fn cell_at(&self, p: Vec2) -> Option<(u32, u32)>;   // same centring as CanvasGrid::cell_at
    pub fn centre_of(&self, x: u32, y: u32) -> Vec2;        // same as CanvasGrid::centre_of
    /// Claims for `team` (1-based) every cell whose centre is within `r` (+ half a cell when
    /// `tiles`) of the segment a→b. Without `take_over`, only unclaimed cells.
    pub fn claim(&mut self, a: Vec2, b: Vec2, r: f32, team: u8, take_over: bool, tiles: bool);
    pub fn claim_cell(&mut self, x: u32, y: u32, team: u8, take_over: bool);
    pub fn grow(&mut self, grow: f32, take_over: bool, tick: u32);
    pub fn shares(&self, teams: usize) -> Vec<f32>;          // counts[i] / (w*h) for i in 0..teams
    pub fn image(&self, teams: &[bpe_runtime::Team]) -> Vec<u8>;   // RGBA8 sRGB, see below
    pub fn hash(&self) -> u64;                                // FNV-1a over w, h and cells
}
pub fn colour_name(srgb: [u8; 3]) -> &'static str;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TeamKey { Thing(ThingId), Colour(u16) }   // Colour: r5 << 10 | g5 << 5 | b5 of the sRGB colour

#[derive(Clone)]
pub struct Claims { pub map: ClaimMap, pub teams: Vec<bpe_runtime::Team>, pub keys: Vec<TeamKey>, pub last: BTreeMap<(ThingId, u32), Vec2>, pub changed: bool }
```

- One private `set(x, y, team)` keeps `counts` (grow the vec as needed) and `blocks` (claimed cells per block). Every change goes through it, and through `Arc::make_mut(&mut self.cells)` only when the value changes.
- `claim`: box of cells around the segment grown by `r + cell` (clipped); for each cell, `d = distance(centre, segment)`; claim when `d <= r + if tiles { cell × 0.5 } else { 0.0 }`. Then also claim the cell containing `b` (the ball's own cell). `r` below 0 is 0.
- `grow(grow, take_over, tick)` exactly as spec §4:

```rust
        if grow <= 0.0 { return; }
        let old = self.cells.clone();            // read from the copy, write into self
        let active = /* every block with blocks[i] > 0, plus its 4 neighbour blocks, sorted, deduplicated */;
        for block in active {                    // block index order
            for each cell (x, y) of the block, row by row (clipped to w, h) {
                let me = old[y * w + x];
                let mut n = [0u8; 4]; // tally neighbours left, right, up, down that exist and are claimed
                // best: the team with the largest neighbour count (excluding `me` when me != 0); ties → lower team number
                let p = if me == 0 { grow * 0.5 * best_count as f32 / 4.0 }
                        else if take_over && best_count > my_count { grow * 0.25 * (best_count - my_count) as f32 / 4.0 }
                        else { continue };
                let u = (mix(tick as u64, y * w + x) >> 40) as f32 / (1u64 << 24) as f32;
                if u < p { self.set(x, y, best) }
            }
        }
```

- `colour_name` (sRGB 0..255 → 0..1; `v = max`, `s = (max − min) / max` or 0, hue in degrees from the usual max-channel formula):
  - `v < 0.15` Black. `s < 0.18`: White when `v > 0.85`, else Grey.
  - Hue `< 15` or `>= 345` Red; `< 45` Brown when `v < 0.6` else Orange; `< 70` Yellow; `< 100` Lime; `< 160` Green; `< 185` Teal; `< 200` Cyan; `< 255` Blue; `< 290` Purple; else Pink.
- `image(teams)`: row-major `w × h`, claimed cell → `[r, g, b, 255]` from `Color::from_linear(teams[i].colour)` (sRGB bytes), unclaimed `[0, 0, 0, 0]`. A team number past `teams.len()` draws as unclaimed.

**Sim wiring:**
- `CanvasState` gains `pub claims: Option<Claims>`. In `after_build`, when the kind is Paint and some brush with `claim_on || claim_hits` matches the canvas (`world.matches(canvas, owner, &brushes_from)`, as the pickup check does), create `Claims` with `ClaimMap::new(size, if tile_size > 0 { tile_size } else { grid.cell_size() })`.
- In `marks()`, for brushes with claims and a canvas with `claims`:
  - `claim_on && paint_now`: per living ball, `a = last.get(key).unwrap_or(here)`, radius from `claim_width_by`/`claim_width` (same formulas as the paint width), `map.claim(a, here, radius, team, take_over, tile_size > 0)`, then `last.insert(key, here)`. A dead ball removes its `last` entry.
  - `claim_hits`: for each Hit of this thing (the same `ours` filter) with `speed >= claim_min_speed`: radius `claim_hit_size × (1 + claim_hit_by_speed × (speed / 1000).min(2))`, `claim(at, at, radius, team_of(mover))`.
  - **Team of a ball** `team_of(b, thing, i)`: colour `c = colours.of(b, thing, i)` (linear); key `Thing(thing)` or `Colour(quantised sRGB of c)`; find the key in `keys`; when missing and fewer than 255 teams, push the key and a `Team { name, colour: [c[0], c[1], c[2], 1.0] }` and set `changed = true`; when 255 teams exist already, the claim is skipped. Name: `world.names[thing]` (By thing) or `colour_name(srgb)` (By colour); when the name is taken (case-insensitive), append `" 2"`, `" 3"`, … until free.
- In the 30 Hz tick: `claims.map.grow(settings.grow, settings.take_over, tick)` where `tick = (world.step / st.tick_every) as u32`, after the paint evolution.
- After the step's work, when `claims.changed`: `world.set_teams(canvas, claims.teams.clone(), log)` and `changed = false`.
- `Cx::marks` needs `&mut` access to the claims; it already has `st: &mut CanvasState`. Keep the brush order: strokes, scrapes, claim path, splats, chips, claim hits.
- [ ] **Step 1: tests first** (`claims.rs` and `sim.rs`):
  - `claim_takes_cells_near_the_path` (smooth and tiles: a tile 40 px whose centre is 25 px from a 10 px path is claimed with tiles, not without).
  - `take_over_off_keeps_ground`: team 2 cannot take team 1's cells; with take over on it can; counts stay exact (`counts` equals a scan).
  - `grow_spreads_and_is_order_free`: a single claimed cell with `grow 1` after 30 ticks has more than 1 cell; the result equals running `grow` on a map built by claiming the same cells in a different order.
  - `grow_takes_over_where_outnumbered`: a team-1 cell surrounded by four team-2 cells flips within 40 ticks at grow 1 with take over; never without.
  - `colour_names`: `#ff3e5c` Red, `#3e9bff` Blue, `#3ee07a` Green, `#ffd23e` Yellow, `#ff5ca8` Pink, `#3ee0d0` Teal, `#b6ff3b` Lime, `#ffffff` White, `#808080` Grey, `#000000` Black, `#8b4513` Brown, `#ff8c1a` Orange, `#a04dff` Purple, `#29f0ff` Cyan.
  - `teams_by_colour_get_names_and_repeats`: two balls of colours that quantise differently but both name Red → "Red" and "Red 2".
  - `teams_by_thing_use_the_things_names`.
  - `a_canvas_without_claim_brushes_has_no_claims` (`st.claims.is_none()`), and claims never on Cover/Powder/Fog.
  - `world_gets_the_teams`: after the first claim, `world.teams_of(canvas)` equals `claims.teams` and `log.teams_of(canvas)` too.
- [ ] **Step 2:** Fail, implement, pass. Commit `feat(marks): colour-war claims`.

---

### Task 8: The numbers

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/sim.rs`.
- Modify: `crates/bpe-kit/tests/` (a new `canvas_numbers.rs` integration test file).

- `CanvasState` gains `pub wet: f32`.
  - On each 30 Hz tick of a Paint canvas whose `dry_time > 0`, count the cells `wet(c, now, dry_ticks)` over `grid.wet_tiles(now, dry_ticks)` (real cells only: iterate `tile_cells(t)`), and set `wet = count / off_cells()`, or 0 when nothing is painted.
  - This runs on tick steps even when evolution is otherwise off. Keep the tick condition of Task 6 and add "Paint with `dry_time > 0`" to it. Inside the tick, `evolve::tick` is still called **only** when `st.evolve.on()`, exactly as now, and the wet count does **not** add to `ticks_run`. **The wet count must not change any mark or cell** (it only reads), so goldens and grids stay identical.
- At the very end of `after_step`, every step:

```rust
let numbers = CanvasNumbers {
    covered: st.grid.covered(),
    revealed: st.grid.revealed(),
    wet: if kind == CanvasKind::Paint { st.wet } else { 0.0 },
    shares: st.claims.as_ref().map_or_else(Vec::new, |c| c.map.shares(c.teams.len())),
};
world.report_canvas(canvas, numbers, log);
```

  It must run after the state is put back and after `set_teams`.
- [ ] **Step 1: tests first** (`crates/bpe-kit/tests/canvas_numbers.rs`, built from `scenes::marks` and small scenes):
  - `numbers_arrive_60_times_a_second`: a 2 s Paint pour → `log.canvas_samples_of(canvas)` has 121 samples (steps 0, 4, 8 … at 240 Hz), strictly increasing steps; `covered` never decreases without a scrape brush.
  - `revealed_climbs_as_balls_scrape`: Cover with scrape → revealed at 5 s > revealed at 1 s > 0; covered + revealed = 1 within 1e-6.
  - `shares_add_up`: a colour-war scene → shares sum ≤ 1 + 1e-6, `leader()` is `Some`, and each share equals the claim map's count / cells.
  - `wet_falls_as_paint_dries`: Paint, dry 1 s, balls removed at 1 s (or a canvas brush filter that stops matching): wet is > 0 at 1 s and 0 by 2.5 s.
  - `every_new_demo_seeks_to_the_straight_run` (Review Focus 2): for each of the five new demos (after Task 14; write the test now over the scenes you have and extend it in Task 14), a straight `Playback` seeked to 12 s against one seeked 5 → 2 → 12: equal grid hash, equal claim hash, equal `drops`, equal `log.canvas_samples()` and `log.teams_of`.
- [ ] **Step 2:** Fail, implement, pass. `cargo test -p bpe-kit --test golden` passes unchanged. Commit `feat(marks): canvases report their numbers`.

---

### Task 9: GPU: wipes on paint canvases

**Files:**
- Modify: `crates/bpe-gfx/src/shaders/canvas_stamp.wgsl` (`fs_wipe`).
- Modify: `crates/bpe-gfx/src/canvas/stamp.rs` (the wipe pipeline, runs of three kinds).
- Modify: `crates/bpe-gfx/src/flow/mod.rs` tests (or `stamp.rs` tests, wherever `canvas_marks_stamp_their_shapes` lives).

- [ ] **Step 1: shader.** Add after `fs_lift`:

```wgsl
// A scrape or chip takes paint away: the paint and its thickness under the shape are scaled
// down by the strength (colour.a), which the blend multiplies into what is there.
@fragment
fn fs_wipe(in: StampOut) -> LiftTargets {
    let start = in.ends.xy;
    let r = in.look.x;
    let variant = u32(round(in.look.z));
    let shape = u32(round(in.look.w));
    var d = 0.0;
    if shape == 12u {
        // The same ribbon cuts as a stroke's (variant bits 1 and 2), then the capsule.
        /* copy the `ribbon && (variant & 6u) != 0u` block of fs_stamp here, unchanged */
        d = stroke(in.local, start, in.ends.zw, r, in.clock.w, false);
    } else {
        let c = cos(in.look.y);
        let s = sin(in.look.y);
        let o = in.local - start;
        let q = vec2<f32>(c * o.x + s * o.y, -s * o.x + c * o.y);
        switch (variant >> 5u) & 7u {
            case 0u: { d = splat(q, r, variant & 31u); }
            case 1u: { d = ring(q, r); }
            case 2u: { d = star(q, r); }
            case 3u: { d = crack(q, r, variant & 7u); }
            case 4u: { d = burst(q, r); }
            case 5u: { d = dots(q, r, variant & 7u); }
            default: { d = length(q) - r; }
        }
    }
    let cover = clamp(0.5 - d / texel_size(), 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let k = 1.0 - clamp(in.colour.a, 0.0, 1.0) * cover;
    var out: LiftTargets;
    out.paint = vec4<f32>(k);
    out.aux = vec4<f32>(0.0);
    out.body = vec4<f32>(k);
    return out;
}
```

  Check the field names against `StampOut` (`in.colour`, `in.clock`, `in.ends`, `in.look`, `in.local`) and fix this sketch to the real ones; keep the maths.
- [ ] **Step 2: pipeline.** In `StampPipeline::new` make `wipe` exactly like `lift` (same layout, blend, targets, depth state, write masks) but entry point `fs_wipe`. In the body draw loop, group runs by a kind function `Stamp | Lift | Wipe` (`Scrape | Chip` → Wipe, `Lift` → Lift, everything else but `Bleed` → Stamp) and pick the pipeline per run. Bleed handling is unchanged.
- [ ] **Step 3: tests** (new GPU tests; the helper panics without an adapter under `BPE_REQUIRE_GPU`):
  - `rust_chip_shapes_match_the_stamp_shader` (Review Focus 3): for each of the six shapes and variants `{0, 5, 13, 26}` (for shapes other than splat use the low 3 bits), stamp one opaque Splat-family mark (shape numbers 1..6 with the same variant and turn 0.7 rad, radius 60) onto a blank 256×256-texel canvas of 256×256 px, read back the paint texture, and for every texel centre compute `chip_distance` in the mark's frame: where it is below −1.5 px the texel's coverage (`paint.a`) is above 0.9; above +1.5 px it is below 0.1. (The stamp shader and the wipe shader call the same functions, so this pins the Rust port.)
  - `a_wipe_takes_paint_away`: a full-canvas opaque stroke, then a Chip (ring) at strength 1 and a Scrape at strength 0.5: texels inside the ring line have `paint.a < 0.05`; texels inside the scrape have `paint.a` within 0.02 of 0.5; texels elsewhere are unchanged; `aux.r` (the time painted) is unchanged everywhere.
  - `seeking_back_over_wipes_restores_bit_for_bit`: the part 3 checkpoint test pattern, with Scrape and Chip marks in the log.
- [ ] **Step 4:** `cargo test -p bpe-gfx --lib` passes (no "skipping"). Commit `feat(marks): the graphics card replays wipes`.

---

### Task 10: GPU: drawing covers, powder and claimed ground

**Files:**
- Modify: `crates/bpe-gfx/src/frame.rs` (`Surface`, `Under`, `GridImage`, `CanvasLayer` fields).
- Modify: `crates/bpe-gfx/src/canvas/composite.rs` (bindings 6 to 9, uniform, grid textures).
- Modify: `crates/bpe-gfx/src/shaders/canvas.wgsl`.
- Modify: `crates/bpe-gfx/src/flow/mod.rs` (`picture_atlas()` accessor only).
- Modify: `crates/bpe-gfx/src/lib.rs` (`draw_canvases` passes the atlas; re-exports).

**Interfaces:**

```rust
// frame.rs
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Surface {
    #[default]
    Paint,
    Cover { foil: bool, under: Under },
    Powder { snow: bool, ridges: f32, light_angle: f32 },
    Fog { steam: bool, blur: f32, density: f32 },
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Under {
    #[default]
    Scene,
    /// Linear light.
    Colour([f32; 4]),
    /// 16 stops, top to bottom, linear light.
    Gradient([[f32; 4]; 16]),
    /// A slot of the scene's picture table (`World::pictures`); 31 = missing.
    Picture(u32),
    /// The colour behind the text; the glyphs are the layer's `under_glyphs`.
    Text([f32; 4]),
}
/// A canvas grid copied for drawing: RGBA8, one texel per cell.
#[derive(Clone, Debug, PartialEq)]
pub struct GridImage {
    pub size: [u32; 2],
    pub pixels: std::sync::Arc<Vec<u8>>,
    /// The texels are sRGB colours (claimed ground), not amounts.
    pub srgb: bool,
    /// Sampled nearest (tiles), not filtered.
    pub nearest: bool,
    /// World pixels per texel.
    pub cell: f32,
}
// CanvasLayer gains:
    pub surface: Surface,
    /// Covers, powder and fog: the amounts. Paint: claimed ground. None: nothing.
    pub grid: Option<GridImage>,
    /// How dark the lines between teams are, 0 to 1.
    pub border: f32,
    /// The text under a cover, laid out in world pixels.
    pub under_glyphs: Vec<GlyphInstance>,
```

  Every place that builds a `CanvasLayer` (skins, tests, `canvas_layer()` helpers) adds `surface: Surface::Paint, grid: None, border: 0.0, under_glyphs: Vec::new()`.

- **Composite layout** gains: binding 6 `texture_2d<f32>` (the grid), 7 a filtering sampler (the grid's: nearest or linear), 8 `texture_2d_array<f32>` (the picture atlas), 9 a filtering sampler (the atlas's). Blank stand-ins, made once: a 1×1 `Rgba8Unorm` texture holding `[0, 0, 0, 0]` and a 1×1×1 array texture.
- **Grid textures per slot**: `grids: Vec<Option<GridTexture { texture, view, size, srgb, last: Arc<Vec<u8>> }>>`. Recreate when size or `srgb` differs (`Rgba8UnormSrgb` for claims, `Rgba8Unorm` for amounts); upload with `queue.write_texture` only when `!Arc::ptr_eq(&last, &pixels) && *last != *pixels`.
- **Bind groups**: a canvas with `grid == None` and no picture keeps today's cached path (with the blank stand-ins in 6 to 9). Any other canvas makes its bind group fresh each frame (one or two per frame, cheap) and does not touch the cache.
- **Uniform** (`CanvasUniform`, laid out as `Canvas` in `canvas.wgsl`) gains, after `look2`:

```rust
    /// x: surface (0 paint, 1 cover, 2 powder, 3 fog); y: its look (0 matte/sand/frost, 1 foil/snow/steam);
    /// z: ridges (powder) or blur in world px (fog); w: density (fog) or the light angle in radians (powder).
    surface: [f32; 4],
    /// x: underneath (0 scene, 1 colour, 2 gradient, 3 picture, 4 text); y: picture slot; z: the part (0 all,
    /// 1 underneath only, 2 cover only); w: 1 when there is a grid image.
    under: [f32; 4],
    /// Linear light: the underneath colour.
    under_colour: [f32; 4],
    /// x, y: the grid image's size in texels; z: world px per texel; w: border.
    grid: [f32; 4],
    stops: [[f32; 4]; 16],
```

  `draw` gets a `part: u32` argument (0 for every caller except the cover-with-text path of Task 11).
- **Shader: the paint styles.** Add `fn ground(local: vec2<f32>) -> vec4<f32>`: when `canvas.under.w > 0.5` and the surface is paint, sample the grid image at `uv_g = (local / canvas.grid.z + canvas.grid.xy * 0.5) / canvas.grid.xy`; when its alpha is above 0, return it (straight colour, alpha 1) darkened by the border term; else return `canvas.start`. Replace every use of `canvas.start` in `flat`, `wet_paint`, `watercolour` and `neon_ink` with one `let ground = ground(in.local);` at the top of each. With no grid, `ground` returns `canvas.start` exactly, so the output is unchanged.
  - Border: only when `canvas.grid.w > 0`. Take the nearest texel `t = floor(uv_g × size)`; load the four neighbours with `textureLoad`; for each neighbour whose colour differs from this texel's (any channel by more than 1/255, or one claimed and the other not), let `e` be the distance from the local point to that shared edge in world px; darken by `canvas.grid.w × 0.6 × (1 − smoothstep(0, max(2 px, 0.08 × cell), e))`. Smooth ground uses the same test on the nearest texel.
- **Shader: cover** (`fs_canvas` branches on `canvas.surface.x` first: 0 goes to today's styles unchanged):

```wgsl
fn grid_amount(local: vec2<f32>) -> f32 {
    let uv = (local / canvas.grid.z + canvas.grid.xy * 0.5) / canvas.grid.xy;
    return textureSampleLevel(grid_tex, grid_sampler, uv, 0.0).r;
}
fn under_at(in: CanvasOut) -> vec4<f32>;   // straight colour + alpha of the underneath at this point:
    // 0 scene → (0,0,0,0); 1 colour and 4 text → under_colour; 2 gradient → stops mixed by in.uv.y
    // (16 stops evenly from 0 to 1); 3 picture → textureSampleLevel(atlas, atlas_sampler, in.uv, slot, 0.0),
    // or under_colour when the slot is 31.
fn cover(in: CanvasOut) -> vec4<f32> {
    let a = grid_amount(in.local);
    let k = smoothstep(0.35, 0.65, a);
    var base = canvas.start.rgb;
    let grain = hash2(floor(in.local * 0.7)) - 0.5;          // write hash2: a vec2 → 0..1 PCG-style hash
    if canvas.surface.y < 0.5 {
        base = base * (0.94 + 0.12 * grain);                  // matte
    } else {
        let t = dot(in.local, vec2<f32>(0.7071, 0.7071)) / max(canvas.size.x, 1.0);
        let shine = 0.5 + 0.5 * sin(t * TAU * 3.0 + g.view.w * 0.8);
        base = base * (0.7 + 0.5 * shine) + vec3<f32>(0.06 * grain);   // foil
    }
    // A darker rim on the cover's side of a scratched edge.
    let rim = k * (1.0 - smoothstep(0.65, 0.95, a));
    base = base * (1.0 - 0.35 * rim);
    let part = canvas.under.z;
    let u = under_at(in);
    let o = clamp(canvas.size.w, 0.0, 1.0);
    if part > 1.5 { return vec4<f32>(base * k * o, k * o); }                   // cover only
    if part > 0.5 { return vec4<f32>(u.rgb * u.a * o, u.a * o); }              // underneath only
    let ua = u.a * (1.0 - k);
    return vec4<f32>((base * k + u.rgb * ua) * o, (k + ua) * o);
}
```

- **Shader: powder:**

```wgsl
fn powder(in: CanvasOut) -> vec4<f32> {
    let cell = canvas.grid.z;
    let h = grid_amount(in.local);
    let k = smoothstep(0.02, 0.12, h);
    let dx = grid_amount(in.local + vec2<f32>(cell, 0.0)) - grid_amount(in.local - vec2<f32>(cell, 0.0));
    let dy = grid_amount(in.local + vec2<f32>(0.0, cell)) - grid_amount(in.local - vec2<f32>(0.0, cell));
    let n = normalize(vec3<f32>(-dx * 6.0, -dy * 6.0, 1.0));
    let ang = canvas.surface.w;                                         // clockwise from up, radians
    let l = normalize(vec3<f32>(sin(ang), -cos(ang), 0.8));
    let lit = 0.55 + 0.45 * max(dot(n, l), 0.0);
    let grain = hash2(floor(in.local * 0.5)) - 0.5;
    var c = canvas.start.rgb * lit;
    if canvas.surface.y < 0.5 {
        c = c * (0.92 + 0.16 * grain);                                  // sand
    } else {
        c = mix(canvas.start.rgb * vec3<f32>(0.72, 0.8, 1.0), canvas.start.rgb, lit);   // snow: blue shadows
        if hash2(floor(in.local * 0.5) + vec2<f32>(17.0, 3.0)) > 0.996 { c = c + vec3<f32>(0.6 * lit); }
    }
    let o = clamp(canvas.size.w, 0.0, 1.0);
    return vec4<f32>(c * k * o, k * o);
}
```

- **Rust side**: `CanvasUniform::new` fills the new fields from `surface`, `grid`, `border`; `Under::Gradient(stops)` copies the stops; `Picture(slot)` puts the slot in `under.y`. Fog (`surface.x == 3`) is drawn by Task 11; until then `draw_canvases` skips fog canvases.
- **Atlas**: add `pub(crate) fn picture_atlas(&self) -> Option<&PictureAtlas>` to the flow layers (`self.atlas.as_ref().map(|(_, a)| a)`); `draw_canvases` passes it to `Composite::draw`.
- [ ] **Step 1: tests first** (composite tests, GPU):
  - `a_paint_canvas_without_claims_draws_as_before` (Review Focus 1): render the existing `canvas_textures_are_sampled_and_fade` setup with each of the four styles, before your change (record the pixel bytes into the test as a hash) and after: identical.
  - `claimed_ground_shows_under_paint`: a Paint canvas, `starts_as` dark grey, a 4×4 claims image with the left half team red (sRGB 255,0,0) and the right half clear, nearest: a pixel in the left half is red, in the right half dark grey; with an opaque blue paint texel on the left, that pixel is blue.
  - `borders_darken_team_edges`: a claims image with red | blue halves and `border 1`: the pixel right at the boundary is darker (sum of channels) than a pixel in the middle of the red half by at least 30 %.
  - `a_cover_hides_and_reveals`: Cover matte, start colour silver, Under Colour green, an amount image 255 everywhere except a scraped square of 0: inside the square the pixel is green, outside silver, and the edge is not a hard jump (a pixel half a cell in shows a blend).
  - `foil_shines_where_matte_does_not`: same image, foil: the spread of brightness over a row of untouched pixels is at least 3× the matte one's.
  - `cover_parts_add_up`: drawing part 1 then part 2 over the same background gives the same pixels (within 2/255) as part 0, for Under Colour.
  - `powder_ridges_catch_light`: a powder image with a ridge (amount 255) beside a scraped track (0) on a rest field (160): a ridge pixel on the lit side is brighter than the rest field; track pixels show the clear colour behind.
  - `under_picture_shows_the_atlas`: with a 2-layer test atlas (one red, one blue) and slot 1, the revealed square is blue.
- [ ] **Step 2:** Fail, implement, pass; plus every existing composite and picture test. Commit `feat(marks): covers, powder and claimed ground on the graphics card`.

---

### Task 11: GPU: fog, and text under a cover

**Files:**
- Modify: `crates/bpe-gfx/src/canvas/composite.rs` (the fog pipeline).
- Modify: `crates/bpe-gfx/src/shaders/canvas.wgsl` (`fs_fog` and its group 2 bindings).
- Modify: `crates/bpe-gfx/src/lib.rs` (`draw_canvases`: fog and under text; the glyph-atlas check; an `under_glyphs` buffer).

- **Fog pipeline**: its own pipeline layout `[globals, canvas layout, fog layout]`, where the fog layout (group 2) is `binding 0: texture_2d<f32>` (the picture so far) and `binding 1: filtering sampler`. Entry point `fs_fog`. Blend: **none** (the shader writes the final colour, mixing in the copy of what was there), write mask ALL, target `HDR_FORMAT`. Vertex `vs_canvas`.

```wgsl
@group(2) @binding(0) var behind: texture_2d<f32>;
@group(2) @binding(1) var behind_sampler: sampler;

@fragment
fn fs_fog(in: CanvasOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(behind));
    let uv = in.pos.xy / dims;
    let sharp = textureSampleLevel(behind, behind_sampler, uv, 0.0);
    let f = grid_amount(in.local);
    let radius = canvas.surface.z * f / max(g.view.z, 1e-4);           // world px → output px
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < 12u; i++) {
        let ring = select(1.0, 0.5, i < 6u);
        let a = f32(i % 6u) / 6.0 * TAU + select(0.0, 0.5236, i >= 6u);
        let off = vec2<f32>(cos(a), sin(a)) * radius * ring / dims;
        sum += textureSampleLevel(behind, behind_sampler, uv + off, 0.0).rgb;
    }
    let blurred = sum / 12.0;
    let density = canvas.surface.w;
    let grain = hash2(floor(in.local * 0.6)) - 0.5;
    var fogged = mix(blurred, canvas.start.rgb, 0.35 * density) + vec3<f32>(0.04 * grain);   // frost
    if canvas.surface.y > 0.5 {
        fogged = mix(blurred, canvas.start.rgb, 0.25 * density) + vec3<f32>(0.05);           // steam
    }
    let amount = clamp(f * clamp(canvas.size.w, 0.0, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(sharp.rgb, fogged, amount), sharp.a);
}
```

  `in.pos` is the `@builtin(position)` of `CanvasOut`; check its name. Do not use `target` anywhere.
- **Drawing fog** in `draw_canvases`: a canvas with `Surface::Fog` is always drawn in the `OnTop` place, whatever its `place` says (the kit sets OnTop anyway): `self.copy_under(encoder)`, then a pass on `targets.hdr` with the fog pipeline, group 2 bound to `targets.behind_view` (make that bind group per frame). Fog ignores Blend (always this path).
- **Text under a cover**: the kit fills `under_glyphs` (Task 12). In `draw_canvases`, for a cover canvas whose `under_glyphs` is not empty, record into the same pass: `draw(part 1)`, then the glyphs with the glyph pipeline exactly as `draw_front` draws glyphs (world `Through`, the glyph atlas group at 1, the paint group at 2, an instance buffer), then `draw(part 2)`. The two draws use **different uniform slots** (`2 × i` and `2 × i + 1`; every other canvas uses `2 × i`): uniforms are written before the frame is submitted, so one slot per draw. This holds in the blend-layer path too.
  - The glyph instances: a renderer-level `under_glyphs: GrowableBuffer` uploaded once per frame with every canvas's `under_glyphs` concatenated in canvas order; remember each canvas's range.
  - `draw_canvases` needs the world `Through`; pass it in from the caller.
  - The glyph-atlas check (`let glyphs = |f: &Frame| ...` near "sync_atlas") must also be true when any `frame.canvases[..].under_glyphs` is non-empty.
- [ ] **Step 1: tests first** (GPU, renderer level, `bpe-gfx/src/lib.rs` tests or composite tests):
  - `fog_blurs_what_is_behind`: a frame with a sharp black/white vertical edge (a big white box shape on black), a fog canvas over it with amount 255 everywhere, blur 20, density 0: the brightness step across the edge spreads over more than 10 px (versus 2 px without the fog); a fog whose amount image is 0 changes nothing (pixels equal).
  - `fog_is_drawn_over_balls`: a fog canvas with `place` BehindBalls still blurs a ball (a ball's edge is soft under it).
  - `text_shows_under_a_cover_only_where_revealed`: a cover over Under Text with one glyph run: in a scraped square the glyph colour appears where the glyph is; outside the square only the cover colour shows.
- [ ] **Step 2:** Fail, implement, pass. Commit `feat(marks): fog and text under covers`.

---

### Task 12: Kit: the canvas draws its surface; pictures under covers

**Files:**
- Modify: `crates/bpe-kit/src/things/canvas/draw.rs`.
- Modify: `crates/bpe-kit/src/pictures.rs` (collect and visit `under_picture`).

- `CanvasDraw::draw` fills, from the live settings and `CanvasState`:
  - Paint: `surface: Surface::Paint`; `grid`: the claims image when `st.claims` is `Some` (`GridImage { size: map dims, pixels: Arc::new(map.image(&claims.teams)), srgb: true, nearest: settings.tile_size > 0, cell: map.cell_size() }`), else `None`; `border: settings.border.clamp(0, 1)`. Everything else as today.
  - Cover, Powder, Fog: `texels: [1, 1]` (their paint textures are never used; this keeps the flow layers' memory tiny); `start: Some(start_colour.to_linear())`; `grid: Some(GridImage { size: grid dims, pixels: Arc::new(st.grid.image()), srgb: false, nearest: false, cell: grid.cell_size() })`; `look: PaintLook::default()`.
    - Cover: `Surface::Cover { foil, under }` with `under` from `settings.under`: Scene; Colour(under_colour linear); Gradient(16 stops: sample `under_colours` at `k / 15` for k in 0..16 with the Gradient's own sampling method, to linear); Picture(`pictures::slot_of(world, &under_picture) as u32`, 31 when empty); Text(under_colour linear).
    - Text under: lay out `under_text` with `bpe_gfx::text::layout_text` in `Font::Anton`'s face, size `under_text_size`, centred on the canvas's centre (`pose.position`), alignment centre, fit width `size.x × 0.9`, every letter visible, style `TextStyle { color: [1.0; 4], outline: 0.0, outline_color: [1.0; 4], glow: 0.0 }`, then set each glyph's `color` and `outline_color` to `blend_alpha(under_text_colour linear)` (see how `text/draw.rs` calls these). Push into `under_glyphs`. The text does not turn with a turned canvas (spec §12).
    - Powder: `Surface::Powder { snow, ridges, light_angle }`.
    - Fog: `Surface::Fog { steam, blur, density }`, `place: CanvasPlace::OnTop`, `blend: Blend::Normal`.
- `pictures::collect`: also every canvas state with kind Cover, `under == Picture` and a non-empty `under_picture` (reach it through `world.states` and `CanvasState`, in `world.order()`). Still sorted, still at most `MAX_PICTURES`. `visit_params` visits both `"splat_picture"` and `"under_picture"`, and `visit` also visits every thing's own `params` (canvas settings live on the thing), so adopting copies under-pictures into the pictures folder and `picture_paths` lists them.
- [ ] **Step 1: tests first** (`draw.rs` tests and `pictures.rs` tests):
  - `cover_frames_carry_their_grid`: a Cover canvas's `CanvasLayer` has `texels == [1, 1]`, `grid.size == grid dims`, and a scraped cell shows as amount 0 in `grid.pixels`.
  - `fog_is_always_on_top`.
  - `claims_reach_the_frame`: a colour-war canvas after 2 s has `grid.srgb` true and some pixel with alpha 255.
  - `under_text_lays_out_glyphs`: Under Text "AB" gives 2 glyphs centred within 1 px of the canvas centre (the mean of glyph rect centres).
  - `under_pictures_join_the_table` and `adopting_copies_under_pictures` (mirror part 3's splat-picture tests).
- [ ] **Step 2:** Fail, implement, pass. `cargo test -p bpe-kit` (including `canvas_pictures`, `paint_pictures`, `pictures`) passes. Commit `feat(marks): canvases draw their surfaces`.

---

### Task 13: Starting points in the Add menu

**Files:**
- Modify: `crates/bpe-kit/src/starters.rs` (`CanvasStart`, `add_canvas`); re-export both from `bpe-kit/src/lib.rs`.
- Modify: `crates/bpe-studio/src/tree.rs` (the Canvas submenu, `Action::AddCanvas`).
- Modify: the studio test that covers the Add menu (find it with `grep -rn "Add thing" crates/bpe-studio`).

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasStart { Paint, ScratchCard, Sand, Snow, FogGlass, ColourWar }
impl CanvasStart {
    pub const ALL: [CanvasStart; 6] = [Self::Paint, Self::ScratchCard, Self::Sand, Self::Snow, Self::FogGlass, Self::ColourWar];
    pub fn label(self) -> &'static str;   // "Paint canvas", "Scratch card", "Sand", "Snow", "Fog glass", "Colour war"
    pub fn help(self) -> &'static str;    // one plain sentence each
    pub fn settings(self) -> CanvasSettings;
}
pub fn add_canvas(scene: &mut Scene, reg: &Registry, start: CanvasStart, parent: ThingId) -> Result<ThingId, EditError>;
```

- `settings()` (everything else default):
  - Paint: the default.
  - ScratchCard: `kind Cover, cover_look Foil, start_colour #b8bcc6, under Text, under_text "YOU WIN", under_text_size 200, under_colour #1b1030, under_text_colour #ffd23e`.
  - Sand: `kind Powder, powder_look Sand, start_colour #d9b77e, ridges 0.6, grows_back 4`.
  - Snow: `kind Powder, powder_look Snow, start_colour #f4f7fb, ridges 0.8, grows_back 4`.
  - FogGlass: `kind Fog, fog_look Steam, start_colour #e8eef5, density 0.7, blur 14, grows_back 4, drops 0.5`.
  - ColourWar: `kind Paint, starts_as Colour, start_colour #14141c, tile_size 40, take_over true, border 0.4`.
- `add_canvas`:
  1. Note which balls things already have a Marks module (`resolve_slot(scene, reg, id, SlotId::Marks, ..)`, as `give_balls_brushes` does).
  2. `let id = add_thing(scene, reg, "canvas", parent)?;` (which links fresh Brushes to bare balls). Then write every key of `start.settings().to_params()` into the canvas thing's params.
  3. For every balls thing (scene order) whose Marks slot is `SlotLink::Linked { module, .. }`: read `MarksBrushes::from_params(&scene.library.modules[&module].params)`, apply the start's brush changes, write it back with `to_params()`. **Fresh** means the thing had no Marks module in step 1.
     - ScratchCard, Sand, Snow, FogGlass: `scrape_on = true`, `scrape_width_by Size`, `scrape_width` 1.0 (FogGlass 1.4); fresh: `paint_on = false`.
     - ColourWar: `claim_on = true`, `claim_hits = true`, `claim_team ByColour`; fresh: `paint_on = false`.
     - Paint: no change.
  4. Return `id`.
- `tree.rs`: `Action::AddCanvas(bpe_kit::CanvasStart)`. In the Add menu's `button` closure, for `kind.id() == "canvas"` show `ui.menu_button(kind.name(), |ui| ...)` listing `CanvasStart::ALL` (label, hover help); a click sets the action and closes the menu. Handle `AddCanvas` like `Add`, with the same parent rule, calling `bpe_kit::add_canvas`.
- [ ] **Step 1: tests first:**
  - kit: `each_start_adds_a_canvas_of_its_kind`; `scratch_card_turns_scrape_on_and_paint_off_for_fresh_brushes`; `existing_brushes_keep_their_paint` (balls that had a Marks module with paint on keep `paint_on` and gain `scrape_on`); `colour_war_turns_claims_on`.
  - studio (kittest): opening "+ Add thing" → "Canvas" → "Sand" adds a canvas whose kind is Powder and selects it. Name controls with `labelled_by` where needed; menus are `menu_button`s (CLAUDE.md pitfalls).
- [ ] **Step 2:** Fail, implement, pass (`cargo test -p bpe-kit --lib starters`, `cargo test -p bpe-studio`). Commit `feat(marks): canvas starting points in the Add menu`.

---

### Task 14: Demos, goldens, scene files, speed

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (five demos; the names test).
- Modify: `crates/bpe-kit/tests/golden.txt`, `golden-debug.txt` (blessed).
- Create: `scenes/next/scratch-to-reveal.ron`, `colour-war.ron`, `territory.ron`, `sand-garden.ron`, `fog-glass.ron` (blessed).
- Modify: `crates/bpe-kit/tests/speed.rs`, `crates/bpe-kit/tests/canvas_numbers.rs`.

- `scenes::marks(reg)` returns `[paint_pour, splat_ring, peg_paint, wet_pour, watercolour_garden, neon_splat_ring, colour_mixing, scratch_to_reveal, colour_war, territory, sand_garden, fog_glass]`, in that order. Update the doc comment ("The twelve Marks scenes …").
- Write one builder beside `pour`, used by all five (do not change `pour`):

```rust
/// A box arena 900×1500 under `gravity`, 20 s, balls "`balls_name`" in a ring at (0, −200), and a canvas
/// called `canvas_name` with `canvas`, the balls' brushes set to `brush`.
#[allow(clippy::too_many_arguments)]
fn surface_demo(reg: &Registry, name: &str, canvas_name: &str, gravity: Vec2, colours: &[Color],
    balls: impl FnOnce(&mut BallsSettings), canvas: CanvasSettings, brush: MarksBrushes) -> Scene
```

  It follows `pour` step by step (scene settings, arena, balls, bounce 1.0, ball look with `Gradient::even(colours)` and `Follow::Order`, the canvas added last at `Vec2::ZERO`, then the brushes module's params). Use the real name of the balls settings type (`pour`'s closure argument).
- The demos (every unnamed setting is its default):
  - **Scratch to reveal**: canvas "Card"; gravity (0, 600); balls Ring of 5, radius 30, ring radius 160, velocity (300, −150); colours `#ffffff #ffd23e #ff5c8a #3ee0d0 #a04dff`; canvas = `CanvasStart::ScratchCard.settings()`; brush: `paint_on false, scrape_on true, scrape_width_by Size, scrape_width 1.0, chip_on true, chip_shape Splat, chip_size 30, chip_min_speed 80`.
  - **Colour war**: canvas "War"; gravity (0, 0); balls Ring of 4, radius 34, ring radius 250, velocity (420, −260); colours `#ff3e5c #3e9bff #3ee07a #ffd23e`; canvas = `CanvasStart::ColourWar.settings()` (tiles 40, border 0.4); brush: `paint_on false, claim_on true, claim_width_by Size, claim_width 1.2, claim_hits true, claim_hit_size 60, claim_team ByColour`.
  - **Territory**: canvas "Land"; gravity (0, 400); balls Ring of 3, radius 26, ring radius 200, velocity (320, −200); colours `#ff5ca8 #3ee0d0 #b6ff3b`; canvas: `kind Paint, starts_as Colour, start_colour #101018, tile_size 0, grow 0.6, take_over true, border 0.3`; brush: `paint_on false, claim_on true, claim_width 1.0, claim_hits true, claim_hit_size 80, claim_team ByColour`.
  - **Sand garden**: canvas "Sand"; gravity (0, 300); balls Ring of 6, radius 22, ring radius 200, velocity (260, −120); colours `#5b3a29 #2f4858 #33658a #86bbd8 #f26419 #f6ae2d`; canvas: `CanvasStart::Sand.settings()` with `ridges 0.7, grows_back 6`; brush: `paint_on false, scrape_on true, scrape_width 1.0`.
  - **Fog glass**: canvas "Fog"; gravity (0, 500); balls Ring of 5, radius 30, ring radius 180, velocity (300, −200); colours `#ff3e5c #ffb82e #3ee07a #3e9bff #a04dff`; canvas: `CanvasStart::FogGlass.settings()` with `density 0.75, grows_back 5, drops 0.6`; brush: `paint_on false, scrape_on true, scrape_width 1.4`.
- **Goldens:**

```sh
BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden every_stage_scene_keeps_its_event_log
BLESS_GOLDEN=1 cargo test -p bpe-kit --release --test golden every_stage_scene_keeps_its_event_log
git diff crates/bpe-kit/tests/golden.txt crates/bpe-kit/tests/golden-debug.txt
```

  Expected: each file gains exactly five lines at the end (`Scratch to reveal`, `Colour war`, `Territory`, `Sand garden`, `Fog glass`) and **no other line changes**. If an old line changed, stop and find out why with superpowers:systematic-debugging; do not re-bless. `golden7*.txt` shows no diff.
- **Scene files:** `BPE_BLESS=1 cargo test -p bpe-kit --lib the_shipped_scene_files_match_the_code`, then `git status scenes/next`: five new files and **no modified** old ones.
- [ ] **Step 1: tests first:**
  - Extend the names test with the five names in order. `marks_scenes_resolve_cleanly` covers them.
  - `scratch_to_reveal_reveals`: at 15 s, `revealed` of "Card" is between 0.2 and 0.98.
  - `colour_war_has_four_teams`: at 10 s, `world.teams_of(War)` names are exactly `Red, Blue, Green, Yellow` in some order, and every share is above 0.02.
  - `territory_grows`: the claimed share at 15 s is more than 1.5× the share at 5 s, or above 0.9.
  - `sand_garden_ridges`: at 10 s some cell of "Sand" is above 160.
  - `fog_glass_runs_drops`: within the first 10 s, `drops` is non-empty at some sampled step (every 0.25 s).
  - Extend `every_new_demo_seeks_to_the_straight_run` to all five.
  - `speed.rs` (ignored tests, release):
    - `surface_step_is_quick`: Sand garden and Fog glass, the mean `after_step` time of the canvas stays under 1 ms (copy `canvas_step_is_quick`'s method).
    - `claims_step_is_quick`: Territory, under 1 ms.
    - Run `cargo test -p bpe-kit --release --test speed -- --ignored --nocapture` and put the means in your final message.
- [ ] **Step 2:** Fail, implement the demos, bless.
- [ ] **Step 3: full run** (in the background, 8 to 10 minutes):

```sh
cargo test --workspace 2>&1 | tee /tmp/all.txt; grep -c skipping /tmp/all.txt   # 0
cargo test --workspace --release 2>&1 | tee /tmp/all-release.txt
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

  Everything passes, except in `--release` `bpe-runtime`'s `pushing_an_earlier_step_panics_in_debug` (debug-only, CLAUDE.md).
- [ ] **Step 4:** Commit `feat(marks): cover, colour war, territory, sand and fog demos` and push.

---

### Task 15: Hand over to stream B

**Files:** none.

- [ ] **Step 1:** `git status` is clean and `git log origin/<your branch> -1` equals `HEAD`.
- [ ] **Step 2:** Post one plain message in your thread for the owner: what the canvas side now does, in two or three sentences, and that the review, the exe and the PR come with the second half.
- [ ] **Step 3:** Message the coordinator (`message_coordinator`): "Part 4A is done on branch `<your branch>` at `<sha>`. Tests, goldens (+5 lines) and scene files are in. Stream B can merge it now." Include the speed means and anything you could not do.
- [ ] **Step 4:** Do **not** open a PR, build an exe, or run a review. Resolve your thread once the coordinator confirms.

---

## Likely misreadings (read before starting)

- **"Scrape works on paint" does not mean the GPU draws scrapes on covers.** Only Paint canvases log Scrape/Chip marks. Covers, powder and fog are drawn from `grid.image()`.
- **Do not change `CanvasGrid::hash()`.** Old grids must hash the same. The counters are not hashed.
- **Do not make `new()` start cells at 2 px.** Only Cover uses 2 px (via `surface`).
- **`covered` means different things.** Paint: share painted. Others: share still there. Do not "fix" one to match the other.
- **Claims are on Paint canvases only**, and the claim map is a separate small map, not the cells' `team` byte (leave `Cell::team` at 0).
- **Ridges are powder only**, and only for scrapes and chips, never for drops.
- **Fog ignores Layer and Blend.** The settings are hidden for fog; the kit forces OnTop and Normal anyway.
- **Do not add `_ =>` arms** to `MarkShape` matches. Every match names every shape.
- **Teams only grow during a run.** Never remove a team when its share drops to 0.
- **`wet` counting only reads.** It must never wake tiles or change cells.
- **The two draws of a cover with text need two uniform slots.** One slot would make both draws use the last-written uniform.
