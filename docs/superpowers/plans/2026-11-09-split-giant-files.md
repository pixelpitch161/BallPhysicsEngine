# Split the giant files (W17) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the biggest source files and the longest functions into focused modules, and name the light-pass
numbers, with no change at all to what the program does.

**Architecture:** Pure code motion inside each crate. A file `x.rs` becomes a folder `x/` whose `mod.rs` keeps the
type definitions and re-exports every item that used to live in `x.rs` (`pub use` / `pub(crate) use`), so every
path outside the folder (`crate::frame::LightGpu`, `bpe_gfx::Frame`, `crate::inspector::show`, …) keeps working.
Long functions are cut at their existing `// ----------` section comments into helper functions in the same folder.
The lighting shader becomes a folder of `.wgsl` files that `concat!` joins back into **exactly the same text**.

**Tech Stack:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37.

**Spec:** the whole-code review (Claude Doc "Ball Studio code review: what to improve",
https://claude.ai/artifact/RpJEYqdjL2NWTPtUzCArYe), items **C8** and **C9**, work plan line **W17** ("Split the
giant files", last). Aedan approved everything but the Size section.

- C8: "The renderer's central pieces are too big to change safely. The lighting shader is 4,308 lines with 144
  functions in one file. One lighting function is 728 lines long, and the frame file is 3,383 lines. Light-pass
  numbers are compared as bare numbers in 27 places." Fix: "Split lighting into one module per pass, split the frame
  file by topic, and name the pass numbers."
- C9: "Editor god-functions. The inspector's main function is 568 lines. Across the codebase, 41 functions are 150
  lines or longer." Fix: "Give each card its own module, as the Emitter card already has."

The coordinator's brief adds: the other giant files too (the work plan's title is "Split the giant files"), and a
clear file map in the docs, because the colour-grading build (spec `docs/superpowers/specs/2026-11-09-colour-grading-design.md`)
starts next on top of this layout and touches `post.wgsl`/effects, `frame.rs`, the renderer, `app.rs`, the inspector
and `settings.rs`.

## Global Constraints

- **No behaviour change.** Every golden (`crates/bpe-kit/tests/golden*.txt`, `crates/bpe-sound/tests/golden-sound*.txt`)
  passes unchanged in debug and in `--release`; no `BLESS_GOLDEN`. No test is changed except to move it, and no
  test is skipped, `#[ignore]`d or deleted.
- **Pictures byte-identical.** The full-size look check (`full_size_look.rs`, 1080 x 1920) renders the same PNG bytes
  before and after (compared with `cmp`, see Task 10).
- **Shader text:** the joined lighting source is byte-identical to today's `lighting.wgsl` + `common.wgsl`; light-pass
  naming changes shader text only by replacing a bare-number test with a named one that is true for exactly the same
  values (the pass numbers are whole numbers or -1).
- Public paths stay: nothing outside a split folder changes its `use` lines, except where a private item had to
  become `pub(crate)`/`pub(super)` to be reached from a sibling file. No new `pub` (crate-external) items.
- `cargo fmt --all` (width 120) and `cargo clippy --workspace --all-targets -- -D warnings` clean.
- Windows-friendly (no new platform code). No exe is built or installed.
- File names in plain words like the rest of the code base (`run.rs`, `tiles.rs`, not `impl_2.rs`).

## Review Focus

1. **A re-export that silently picks another item.** A glob `pub use x::*` can shadow a same-named item; every moved
   name must still resolve to the same item. Checked by compiling every crate and its tests (the type checker rejects
   ambiguity between glob imports when the name is used).
2. **A moved test that stopped running.** Moving `mod tests` to `tests.rs` must keep it compiled under `#[cfg(test)]`
   and registered. Checked by counting tests before and after (Task 10, Step 2): the total per crate must match.
3. **Shader text changed by the split.** Checked by a script that joins the new files and `cmp`s against the old
   file (Task 1, Step 3) and by the byte-identical pictures (Task 10).
4. **A light-pass test changed meaning** (for example `> 2.5` written as `>= PASS_SURFACE`). Checked by the
   byte-identical full-size pictures (every lighting quality and scene) and the picture tests.
5. **Order-dependent code moved out of order.** Inside a function cut into helpers, statements must keep their order
   and every `&mut` borrow must end where it did. Checked by goldens (physics), picture tests (renderer) and the
   kittest editor tests (inspector, app).

---

### Task 1: Lighting shader as one file per pass

**Files:**
- Delete: `crates/bpe-gfx/src/shaders/lighting.wgsl`
- Create in `crates/bpe-gfx/src/shaders/lighting/` (cut at today's `// ---------- … ----------` lines, in this order;
  each file starts exactly at a section line and ends just before the next):
  - `bindings.wgsl`: header comment, `Lighting`/`LightGpu` structs, bindings, `FsIn`, `vs_full` (lines 1-122)
  - `field.wgsl`: where things are + the jump flood, blockers' field, `fs_seed`/`fs_jump`/`fs_field` (123-616)
  - `shadows.wgsl`: the light: falloff, source points, extended lights, `march`, `visibility`, `height_at` (617-1226)
  - `direct.wgsl`: part 1b reads (`Glow`), lamps, noise/gobos/links, `lamps`, `fs_direct` (1227-1647)
  - `light_field.wgsl`: the light field (radiance cascades) (1648-1970)
  - `haze_bloom.wgsl`: moving haze, memory, the lights' bloom, part 4 in the mix (1971-2160)
  - `stage.wgsl`: surfaces and the stage: structs, spheres, slab march (2161-2863)
  - `shade.wgsl`: GGX, films, dichroic, `shade`, `trace_scene`, transmitted light (2864-3563)
  - `mix.wgsl`: the mix, rainbow, `lit_at`, mirror walls, `fs_mix`, `fs_mirror` (3564-4007)
  - `dof_floor.wgsl`: depth of field and the glossy floor (4008-4195)
  - `bake.wgsl`: the bake's tracing and clean-up (4196-4495)
  - `caustics.wgsl`: caustics and the light beams' bands (4496-end)
  (Exact cut lines are the section comments; line numbers are today's and only a guide.)
- Modify: `crates/bpe-gfx/src/lib.rs` (`LIGHTING_WGSL`)

- [ ] Step 1: Cut the file with a script at the section lines (no hand edits), so the pieces join to the original.
- [ ] Step 2: `LIGHTING_WGSL = concat!(include_str!("shaders/lighting/bindings.wgsl"), …, include_str!("shaders/common.wgsl"))`
  in the order above; doc comment lists what each file holds.
- [ ] Step 3: Verify: `cat` the pieces in order and `cmp` with `git show HEAD:crates/bpe-gfx/src/shaders/lighting.wgsl`
  → no output. Fix any comment in Rust that says "lighting.wgsl's `x`" to name the new file.
- [ ] Step 4: `cargo test -p bpe-gfx --lib -- --test-threads=2 lighting` passes (shader compiles).
- [ ] Step 5: Commit "Split the lighting shader into one file per pass".

### Task 2: Name the light-pass numbers

**Files:** Create `crates/bpe-gfx/src/shaders/light_passes.wgsl`; modify `lib.rs`, the scene, paths, sprites,
fields, liquid and layer shaders that test `g.light.x`, and `lighting/` Rust code that uses pass numbers.

**Interfaces — Produces:**
- Rust `crate::light_pass` module (in `lib.rs` or `light_pass.rs`): `pub(crate) const LIT: u32 = 1; SURFACE = 2;
  EMISSION = 3; BACKGROUND = 4; UV = 5; TAGS = 6; BEHIND = 7; TINT = 8; GLASS = 9;` replacing `BEHIND_PASS`,
  `TINT_PASS`, `GLASS_PASS` and the bare `4`/`2`/`1..=8` in `render_scene`, `light_globals`, `draw_layer`,
  `Lighting::picture`.
- WGSL (`light_passes.wgsl`): `fn light_pass(x: f32) -> i32 { return i32(round(x)); }` and
  `const PASS_PLAIN: i32 = 0; PASS_BEHIND: i32 = -1;` (the plain picture without see-through things, `light.x` -1),
  `PASS_LIT = 1 … PASS_GLASS = 9` (no `PASS_BEHIND` 7 in WGSL: the shaders see -1 for it).
- Every bare test on the pass (`g.light.x`, or the local `m`/`mode` read from it) becomes a test on
  `light_pass(g.light.x)` against the names: `x > 2.5` → `>= PASS_EMISSION`; `x < 1.5` → `<= PASS_LIT`;
  `x > 4.5 && x < 5.5` → `== PASS_UV`; `x < -0.5` → `== PASS_BEHIND`; `x > 0.5` → `>= PASS_LIT` (true for 1..9,
  false for 0 and -1, as before), and so on: each must be true for exactly the same set of {-1, 0, 1..9}.
- `light_passes.wgsl` is added once to each shader module that uses it (scene source, sprites, fields, liquid,
  and any other module that reads `g.light.x`); `layers_source`'s `plain` replace (`g.light.x` → `plain_mode()`)
  still works because the text `g.light.x` is kept inside `light_pass(...)`.
- Not touched: `lights_up`'s `mode` in lighting (a light's link mode), swarm `mode`, layer group `mode`: they are not
  light passes.

- [ ] Step 1: List every test with `grep -n "light\.x\|let m = g.light" crates/bpe-gfx/src/shaders/*.wgsl` and
  write the table old → new in the commit message.
- [ ] Step 2: Make the replacements; `cargo test -p bpe-gfx --lib -- --test-threads=2` passes (all picture tests).
- [ ] Step 3: Commit "Name the light-pass numbers".

### Task 3: `lighting.rs` → `lighting/`, and `Lighting::run` cut into passes

**Files:** `crates/bpe-gfx/src/lighting.rs` → `crates/bpe-gfx/src/lighting/`:
- `mod.rs`: module doc, formats/limits consts, `Uniform`, `Lighting` struct, `with_kit`, `ensure`, the picture
  accessors, `texture`, `uniform_buffer`, re-exports.
- `binds.rs`: `BindCache`, `BindKey`, `Bound`.
- `tiles.rs`: `LIGHT_TILE`, `TILED_FROM`, `LightTiles` (+ `tile_tests` as `#[cfg(test)] mod tests`).
- `glass.rs`: `dichroic_list`, `StageUniform`, `CausticUniform`, `CausticGlass`, `caustic_glass`, `rainbow_*`,
  `photons_per_pair`, `SphereGrid` (+ `grid_tests`).
- `quality.rs`: `GlowUniform`, binding lists, `Memory`, `cascade_count`, `gi_steps`, `quality_tier`,
  `field_divisor`, `Targets`, `GlowInputs`.
- `kit.rs`: `LightKit` and `LightKit::build` (473 lines; cut by pipeline family into private helpers in the same
  file: field passes, light passes, glow passes, stage passes, bake passes).
- `run.rs`: `Lighting::run`, cut into `fn plan(&self, frame, scene, background_glows) -> RunPlan` (the on/off
  decisions: baking, traced, caustics, bent, into_bg, reflects, blurs, mirror walls, haze, field, blooms, extras,
  memory…), `fn ensure_extra_targets(&mut self, device, &RunPlan)`, `fn upload(&mut self, device, queue, frame,
  scene, &RunPlan, output_width) -> Uploaded` (uniforms and buffers), and the encode steps in order (`field_passes`,
  `direct_passes`, `glow_passes`, `mix_passes`) taking the bind-group helpers they need. `RunPlan` and `Uploaded`
  are private structs in `run.rs`. Statement order inside the encoder is unchanged.

- [ ] Step 1: Move items (no edits but visibility); `cargo check -p bpe-gfx --tests`.
- [ ] Step 2: Cut `run` and `build`; no function in `lighting/` over ~200 lines.
- [ ] Step 3: `cargo test -p bpe-gfx --lib -- --test-threads=2` passes; commit.

### Task 4: `frame.rs` → `frame/` by topic

`crates/bpe-gfx/src/frame/`: `mod.rs` (`Frame`, its `Default`, `Stage`, `Place`, `BlendLayer`, the small `impl Frame`
for layers/overlay/culling, re-exports), `instances.rs` (`LayerGpu`, `LayeredInstance`, path instances/channels/
styles, `thing_code`, `FlashInstance`), `trails.rs` (`TrailVertex`, `StreakStyle`, `push_trail`, streaks, `strip`,
`Corner`), `particles.rs` (`Particle`, field looks, `Splat`, `Field`, ripple sources and `Frame`'s ripple methods),
`marks.rs` (`HitMark`, `EffectCall`, `Takes`, `BallMark`, `blend_alpha`, `BackgroundPaint`, `Grade`), `light.rs`
(`RibbonGpu`, `LightGpu`, `LightingGpu`, `SurfaceGpu`, `MirrorGpu`, `SphereGpu`, `LightScene`, throwers, pipes,
benders, tags, `LightShow`), `paths.rs` (`Frame::push_path/ellipse/letter/glyph`, `mask_tile`), `build.rs`
(`DrawCx`, `ThingDraw`, `ModuleDraw`, `Around`, `Draws`, `build_frame*`, culling helpers), `tests.rs`,
`tint_tests.rs`. `crate::frame::X` and `bpe_gfx::X` paths unchanged.

- [ ] Move, `cargo test -p bpe-gfx --lib -- --test-threads=2` and `cargo check --workspace --tests`; commit.

### Task 5: The renderer out of `lib.rs`

`crates/bpe-gfx/src/lib.rs` keeps the module list, re-exports, formats, vertex attributes, `LIGHTING_WGSL`,
`scene_source`, `layers_source`, `GrowableBuffer`, devices (`headless_device`, `test_device`, `NoGpu`). New:
- `renderer/mod.rs`: `Renderer` struct, `Globals`, `Through`, `FrameBuffers`, `Scened`, small accessors, `render`,
  `render_samples`, `read_output`.
- `renderer/new.rs`: `Renderer::new` (445 lines), cut into helpers by pipeline family (scene pipelines, post/bloom,
  paint, effects).
- `renderer/scene.rs`: `render_scene` (cut into the light-pass drawing, `draw_light_passes`, and the rest), `finish`,
  `light_globals`, `lighting_ready`, `light_pictures`.
- `renderer/draw.rs`: `draw_scene`, `draw_glass`, `draw_overlay`, `draw_back/balls/front/sprites/objects/things/
  liquids/layer…`, `pipeline`, `copy_under`, `begin_pass`.
- `renderer/targets.rs`: `Targets` and its `impl`.
- `renderer/grade.rs`: `GradeMap`, `GRADE_WIDTH`, `PaintTexture` (the colour-grading build replaces `GradeMap`).
- `readback.rs`: `Readback`, `ReadbackSlot`.
`pub use renderer::Renderer; pub use readback::Readback;`. Fields other modules read become `pub(crate)`.

- [ ] Move, test bpe-gfx lib, check workspace; commit.

### Task 6: `world.rs` → `world/`

`crates/bpe-runtime/src/world/`: `mod.rs` (`World`, `Slotted`, `Prepared`, `RunId`, `Between`, `Lent`, `Card`,
`build*`, accessors), `animate.rs` (`Animated`, `physics_animation`, `animate`, `rule_keys`, `refresh_ball_changes`,
`refresh_rows`, `work_out_pulls`), `step.rs` (`advance_to`, `step`, `substep`, `move_bodies`, `collide`, `matches`,
`is_special`, `decide`, `respond`, `step_at`, `press_of`, `note_corners`), `lend.rs` (`lone`, `lend_*`, `take_back`),
`prints.rs` (`needs_light`, fingerprints, `is_beside`, `main_part`, `heard_part`, `beside_*`, `Hasher`, `FullHealth`),
`tests.rs`. `pub use` keeps `bpe_runtime::{World, fingerprint, step_at, …}`.

- [ ] Move; `cargo test -p bpe-runtime` and `cargo test -p bpe-kit --test golden` (debug) pass; commit.

### Task 7: Inspector: one module per card, `show` cut up

`crates/bpe-studio/src/inspector.rs` → `inspector/mod.rs` (`Inspector`, `InspectorTab`, `Card`, `show`, tabs, fold
helpers, `card`, `card_header`, jump bar) with `show` cut at its section comments into:
- `inspector/header.rs`: the name/kind/eye/tags header.
- `inspector/own.rs`: the kind's own settings (`own_grid`, `root_card`, ball look mode switch, `layered_look`,
  `apply_layer_edit`, `group_mode`).
- `inspector/placement.rs`: `placement_rows`, `position_rows`, `snap_place`, `eye_toggle`.
- `inspector/slots.rs`: the one-card-per-slot loop and `physics_card`, `slot_grid`, `slot_grid_of`,
  `surface_section`, `surface_chip`.
- `inspector/add_card.rs`: "+ Add card".
- `inspector/edits.rs`: `Edit`, `no_vary_here`, `reached*`, `set_or_key`, `clear_keys`, `new_key`, `mark_for`,
  `set_thing`, `shape_afresh`, and `apply_edits` (today's tail of `show`).
- `inspector/tests.rs`. `emitter_card` stays a child (`inspector/mod.rs` keeps `pub(crate) mod emitter_card;` with
  `#[path]` unchanged) and `find.rs` stays.
Shared state passed as today's `OwnCx` plus a `ShowCx` struct (id, thing, kind, selected, choices, lw, time,
finding) built once in `show`. `show` itself ends under ~120 lines.

- [ ] Move and cut; `cargo test -p bpe-studio` passes (kittest inspector tests); commit.

### Task 8: `app.rs` → `app/`

`crates/bpe-studio/src/app/`: `mod.rs` (`Studio` struct, `Problem*`, `Offer`, `Pending`, `Panels`, `LeftPanel`,
`new`, `blank`, `headless`, `with_scene`, `impl eframe::App` with `ui`), `recovery.rs` (autosave, restore, offers,
GPU errors, problems), `runs.rs` (restart/settle runs, timing getters, seek, step, fill_rounds, play, listen,
speakers, tick, readouts and `main_playback`/`aim_job_for`/`follow_of`/`light_probe_of`/`main_particles`), `edits.rs`
(after_edit, history, copy/paste/duplicate/delete, undo/redo, replace/new scene), `files.rs` (open, save, guard,
perform, window title, file label), `shortcuts.rs`, `toolbar.rs` (toolbar, left switch, grid controls, status line),
`panels.rs` (`left_panel`, `inspector_panel`), `dialogs.rs` (stop export, restore, confirm, close after export),
`tests.rs`.

- [ ] Move; `cargo test -p bpe-studio`; commit.

### Task 9: `widgets.rs` → `widgets/`

`crates/bpe-studio/src/widgets/`: `mod.rs` (re-exports, `GridCx`, `KeyMark`, small helpers), `grid.rs` (grid layout,
rows, sections, `settings_grid`, `grid_row*`, group headings, fold toggle), `setting.rs` (`setting`, `widget`,
`spanned`, follow band), `menu.rs` (clipboard, `row_menu*`, `follow_items`, value menus, `pasted`), `numbers.rs`
(`NumberFeel`, sliders, snap ticks, speed/angle), `words.rs` (`target_words`, `value_words`, `who_words`, path and
strokes words, `scenes_words`), `pickers.rs` (`TargetChoices`, `who_picker`, `target_picker`, colour buttons),
`tests.rs`.

- [ ] Move; `cargo test -p bpe-studio`; commit.

### Task 10: Docs, then the whole check

- [ ] Step 1: `docs/notes/file-map.md`: one table per crate touched: folder → file → what it holds, and "where to add
  X" lines for the colour-grading build (grade passes, masks' silhouettes in the frame, Grade mode in `app/`, the
  Grade summary in `inspector/`). Link it from `docs/notes/README.md`, `engine.md`, `editor.md`, `lighting.md`; add
  one index line in `CLAUDE.md` and fix the paths in its crate map (`lighting/`, `shaders/lighting/`, `frame/`).
- [ ] Step 2: Test counts: `cargo test --workspace -- --list` count per crate before (on main) and after are equal.
- [ ] Step 3: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] Step 4: `cargo test --workspace -- --test-threads=2` (debug, background) all pass; release goldens
  `cargo test --release -p bpe-kit --test golden` and `-p bpe-sound --test golden` pass.
- [ ] Step 5: Pictures: full-size look check on main (in a separate worktree) and on the branch, same `LOOK_*` env;
  `cmp` every PNG pair → no differences.
- [ ] Step 6: `cargo check --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`.
- [ ] Step 7: One Opus review of the whole branch; fix what it finds; PR; merge when CI is green.

## As built

(filled in when done)
