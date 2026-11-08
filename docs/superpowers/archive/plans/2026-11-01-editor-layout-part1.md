# Editor layout, Part 1: Type switch, one Look, Across and Inside colours — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a module change type in place, merge the Classic and Styled looks into one Look, and split colours into "Across the group" and "Inside each one", including the per-ball linear gradient Aedan asked for.

**Architecture:** In bpe-core, a new `Scene::set_module_kind` edit stashes the settings the new type lacks and restores them on the way back. In bpe-kit, `look.classic` and `look.styled` become one `look.basic`, and a new `InsidePaint` setting group replaces Paint's inside rows. In bpe-gfx, every inside pattern moves into `paint.wgsl`, with a "shade of own colour" mode. In bpe-studio, the card header gets a Type menu next to the Module menu.

**Tech Stack:** Rust 2024 workspace, wgpu/WGSL, egui 0.37 + egui_kittest, the `bpe-settings-derive` `Settings` derive.

**Spec:** `docs/superpowers/specs/2026-11-01-editor-layout-design.md` (Part 1, "Built-in scenes", "Testing"). Parts 2 and 3 get their own plans after this one merges.

## Global Constraints

- Old saved scenes need not load. The built-in scenes in `scenes/next/*.ron` are regenerated with `BPE_BLESS=1 cargo test -p bpe-kit scenes` (the test in `crates/bpe-kit/src/scenes.rs` near line 1742).
- Physics must not change: `cargo test -p bpe-sim --test golden` passes without re-blessing.
- Every new or renamed `SettingDef` has a help sentence that ends with "." and passes `bpe_core::check_registry` (the existing contract test).
- Every new egui control is labelled (`labelled_by`), because the kittest accessibility check needs it. Pickers are `menu_button`s, not ComboBoxes.
- Never put `.context_menu` on an egui colour button (it shares the colour picker popup id).
- User-visible names are exactly: type `look.basic` named "Look"; style choices "Glow", "Flat", "Shiny"; headings "Across the group" (many instances) or "Colours" (single thing), and "Inside each one"; Inside rows "Pattern", "Inside colours" ("Shade of own colour" / "Own gradient"), "Shade", "Colours", "Beyond the last colour", "Angle", "Spin", "Size", "Scroll", "Turns with the ball".
- Before each push: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Use `CARGO_INCREMENTAL=0` to save disk.

## Review Focus

1. **Switching type on a module shared by several things, with keyframes on one thing's override.** Expect every user to switch, and the override's keyframes to come back on the return trip. Test in Task 1 (`switching_a_shared_module_stashes_each_things_overrides_and_their_keys`).
2. **Switching type twice without going back (A → B → C → A).** Expect A's stash to survive the trip through C. Test in Task 1 (`a_stash_survives_a_switch_to_a_third_kind`).
3. **Inside Own gradient with a one-colour gradient, or Size near 0.1.** Expect a flat colour and no NaNs or black pixels. Test in Task 4 (`inside_linear_with_one_colour_is_flat`).
4. **Undo right after a type switch.** Expect the module, its graph and its keyframes back exactly. The editor's history snapshots the whole scene, so this holds if `set_module_kind` changes only the scene. Test in Task 6 (`undo_after_a_type_switch_restores_the_module`).
5. **A Look on a thing other than balls (arena, obstacle, text, soft body) after the merge.** Expect it to draw as before. The Glow/Flat/Shiny and Shape rows are hidden there. Test in Task 5 (`an_arena_card_hides_the_ball_only_look_rows`).

---

### Task 1: Switch a module's type, with a stash (bpe-core)

**Files:**
- Modify: `crates/bpe-core/src/library.rs` (add `Stash`, `Module.stash`)
- Modify: `crates/bpe-core/src/edit.rs` (add `set_module_kind`, `EditError::WrongSlot`)
- Test: `crates/bpe-core/src/edit.rs` test module (use `testkit` registries; add two small test module kinds sharing one key there if none fit)

**Interfaces:**
- Produces: `pub struct Stash { pub params: Params, pub tracks: BTreeMap<String, Track>, pub pulses: BTreeMap<String, Pulse>, pub locks: BTreeSet<String>, pub overrides: BTreeMap<ThingId, Params>, pub thing_tracks: BTreeMap<(ThingId, String), Track>, pub thing_pulses: BTreeMap<(ThingId, String), Pulse>, pub thing_locks: BTreeSet<(ThingId, String)> }` (serde default, `Default`, `PartialEq`). `Module.stash: BTreeMap<KindId, Stash>`, skipped when empty.
- Produces: `impl Scene { pub fn set_module_kind(&mut self, reg: &Registry, m: ModuleId, kind: &str) -> Result<(), EditError> }`. `EditError::WrongSlot { module: ModuleId, kind: String }`, plus `UnknownKind` if no variant exists for that yet.

- [ ] **Step 1: Write the failing tests**
  - `set_module_kind_keeps_shared_keys_and_stashes_the_rest`: the module of kind A has `{shared: 1, only_a: 2}`. After switching to B, `params == {shared: 1}`, `kind == B`, and `stash[A].params == {only_a: 2}`.
  - `switching_back_restores_the_stash_but_newer_shared_values_win`: A → B, set `shared = 5`, B → A. Expect `{shared: 5, only_a: 2}` and `stash` without A.
  - `keyframes_pulses_and_locks_on_dropped_keys_are_stashed`: a track and a pulse at `Owner::Module(m)/only_a`, and a lock. After A → B, none are left in `scene.tracks/pulses/locks`. After B → A, all three are back, equal.
  - `switching_a_shared_module_stashes_each_things_overrides_and_their_keys`: two things linked. Thing 1 overrides `only_a` and has a track at `Owner::Thing(t1), slot Some(slot)/only_a`. A → B → A restores both.
  - `a_stash_survives_a_switch_to_a_third_kind`: A → B → C → A restores `only_a`.
  - `a_custom_graph_is_dropped_and_a_template_one_replaced`: `graph.custom = true` → `None` after the switch. A non-custom graph → B's `template()` wrapped as `ModuleGraph { template: B, .. }` or `None`.
  - `a_kind_of_another_slot_is_refused`: `Err(EditError::WrongSlot{..})`, and the scene is unchanged.

- [ ] **Step 2: Run them, expect FAIL** (`cargo test -p bpe-core set_module_kind` and the others). Compile error: no such method.

- [ ] **Step 3: Implement** in `edit.rs`, following spec 1.2 steps 1 to 6. Use `reg.carry_over(&params, kind)` to decide which keys stay. A key "fits" B when `carry_over` keeps it. Apply the same test to tracks, pulses, locks and overrides by key. When restoring from `stash[B]`, only insert keys that are not already present. `channel` and `macros` stay untouched.

- [ ] **Step 4: Run, expect PASS.** Then run `cargo test -p bpe-core`, all green.

- [ ] **Step 5: Commit** `core: switch a module's type in place, stashing what the new type lacks`.

### Task 2: Which module types fit which thing (bpe-core + bpe-kit)

**Files:**
- Modify: `crates/bpe-core/src/registry.rs` (`ModuleKind::fits`, `Registry::kinds_fitting`)
- Modify: `crates/bpe-kit/src/modules/{look_liquid,look_swarm,motion_liquid,motion_flock}/mod.rs` (override `fits`)
- Modify: `crates/bpe-studio/src/library.rs` ("+ New" list unchanged: the Library has no thing; leave it showing every kind)
- Test: `crates/bpe-kit/src/lib.rs` tests

**Interfaces:**
- Produces: `fn fits(&self, thing_kind: &str) -> bool { true }` on `ModuleKind`. `pub fn kinds_fitting<'a>(&'a self, slot: SlotId, thing: &'a dyn ThingKind) -> impl Iterator<Item = &'a dyn ModuleKind>`, filtering by `m.slot() == slot && thing.accepts(slot, m.id()) && m.fits(thing.id())`, in registry order.

- [ ] **Step 1: Failing test** `kinds_fitting_limits_looks_and_motions_by_thing`:
  - balls Look gives only `["look.basic"]`. For now, before Task 5, assert it gives `["look.classic", "look.styled"]`. Task 5 updates the assertion.
  - liquid Look gives `["look.liquid"]`; swarm Motion gives `["motion.flock"]`.
  - balls Shader gives all 13. Scene Sound gives `["sound.drums", "sound.melody"]`.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** `look.liquid` and `motion.liquid` fit only `liquid`. `look.swarm` and `motion.flock` fit only `swarm`. `look.classic`/`look.styled` (later `look.basic`) and `motion.basic` fit everything except `liquid` and `swarm`.
- [ ] **Step 4: Run, expect PASS.** Then `cargo test -p bpe-kit`.
- [ ] **Step 5: Commit** `registry: module kinds say which things they fit`.

### Task 3: The InsidePaint setting group (bpe-kit, settings only)

**Files:**
- Create: `crates/bpe-kit/src/paint/inside.rs`
- Modify: `crates/bpe-kit/src/paint/mod.rs` (`pub mod inside; pub use inside::{InsidePaint, Pattern, InsideFrom};`)
- Test: in `inside.rs`

**Interfaces:**
- Produces: `pub enum Pattern { None, Linear, Radial, Sweep, Stripes, Rings, Dots, Checker, Halves, Slices, Swirl, Face }` (derive `Choice`, default None). `pub enum InsideFrom { Shade, Own }` (Choice names `shade`, `own`; labels via the derive's usual snake-to-words). `pub struct InsidePaint { pub pattern: Pattern, pub from: InsideFrom, pub shade: f32, pub colours: Gradient, pub repeat: Repeat, pub angle: f32, pub spin: f32, pub size: f32, pub scroll: f32, pub turns: bool }`, with `impl SettingGroup for InsidePaint`, embedded with the existing `paint` attribute.
- Defaults: `pattern None, from Shade, shade -0.5, colours = two stops white → black, repeat Clamp, angle 90, spin 0, size 1, scroll 0, turns false`.

- [ ] **Step 1: Failing tests:**
  - `inside_rows_and_conditions` asserts these keys and conditions, with prefix `in_`: `in_pattern` (always), `in_from` (`in_pattern!=none`), `in_shade` (`in_pattern!=none and in_from=shade`), `in_colours` and `in_repeat` (`in_pattern!=none and in_from=own`), `in_angle`/`in_spin`/`in_size`/`in_scroll`/`in_turns` (`in_pattern!=none`).
    - The labels are exactly the Global Constraints names. Ranges: shade (-1, 1), angle (-180, 180) deg, spin (-720, 720) deg/s, size (0.1, 24), scroll (-4, 4) cycles/s. angle, spin, size, scroll and shade are keyframed. The field's own `when` is ANDed in, as Paint's `also()` does.
  - `inside_round_trips_and_saves_only_changes`.
  - `every_inside_condition_names_a_setting`, the same check as Paint's test.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement** `group_defs`/`group_read`/`group_write`, mirroring `Paint`'s implementation in `paint/mod.rs` (`leak`, `also`). The group heading is `o.group`, and the main row's label is "Pattern" (ignore `o.label`, which names the heading).
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** `paint: InsidePaint settings group`.

### Task 4: One inside shader for every pattern, plus shade mode (bpe-gfx + bpe-kit gpu)

**Files:**
- Modify: `crates/bpe-gfx/src/shaders/paint.wgsl`. Move `bands`, `bands_over`, `inside` (rename to `paint_inside_mask`) and `pattern` (rename to `paint_mask`) here from `ball_look.wgsl`, so the glyph, soft and particle modules that only include `paint.wgsl` have them. Delete them from `ball_look.wgsl`.
- Modify: `crates/bpe-gfx/src/gradients.rs` (`PaintGpu` docs: new pattern codes, shade mode)
- Modify: `crates/bpe-gfx/src/shaders/{scene.wgsl,styled.wgsl,glyph.wgsl}` and any soft shader calling `paint_colour`, to pass the instance's base colour
- Modify: `crates/bpe-kit/src/paint/gpu.rs` (new `inside_gpu`; `paint_gpu` keeps only the Path case)
- Test: `crates/bpe-gfx/src/paint_tests.rs`, `crates/bpe-kit/src/paint/gpu.rs` tests

**Interfaces:**
- Pattern codes in `PaintGpu.pattern`: 0 none, 1 linear, 2 radial, 3 sweep, 4 path (unchanged), 5 stripes, 6 rings, 7 dots, 8 checker, 9 halves, 10 slices, 11 swirl, 12 face.
- Shade mode: `row == -2.0`, `t0` = the shade amount (-1 to 1). The gradient runs from `base` at 0 to `shaded(base, shade)` at 1. `shaded` mixes toward black, `base * (1 + s)`, for s < 0, and toward white, `mix(base, 1, s)`, for s > 0, in linear light, keeping alpha. `row == -1.0` still means "paint nothing".
- WGSL: `fn paint_colour(p: PaintGpu, local: vec2<f32>, time: f32, base: vec4<f32>) -> vec4<f32>`. How `along` is found:
  - Linear: `dot(local, dir(angle)) * 0.5 * size + 0.5`, with 0..1 across the shape at size 1.
  - Radial: `length(local) * size`.
  - Sweep: as today, but from 0.
  - Path: as today.
  - Stripes and Rings: today's maths, keeping the "Clamp mirrors" rule (`paint_pattern`).
  - Dots to Face: `paint_mask(rotate(local, -angle), code, size*2)` as the 0..1 `along`.
  - Linear, Radial and Sweep honour `repeat` as given (no forced mirror).
  - Shade mode skips the atlas and variation, and blends `base` to `shaded` by the wrapped `along`.
- Rust: `pub fn inside_gpu(i: &InsidePaint, vary: [f32; 4], atlas: &mut GradientAtlas, time: f32) -> PaintGpu`, where the angle is `i.angle + i.spin * time`. It returns `PaintGpu::default()` (row -1) for `Pattern::None`. In Own mode it uses `atlas.row(&i.colours)` and `repeat_code(i.repeat)`. In Shade mode it uses `row: -2.0, t0: i.shade`.

- [ ] **Step 1: Failing tests:**
  - (gfx) Draw one styled-look instance and render with `pictures::save`:
    - `inside_linear_shade_runs_from_base_to_darker_top_to_bottom`: angle 90, shade -1, base red. The top pixel ≈ red and the bottom pixel ≈ black (tolerance 0.05 linear).
    - `inside_linear_own_gradient_uses_its_stops`: blue → green.
    - `inside_linear_with_one_colour_is_flat`: no channel differs > 0.02 across the ball, and no NaN.
    - `inside_dots_mask_matches_the_old_fill_dots`: compare against a render made with the old `fill` code 6 before the change. Capture it in this step, before editing the shader.
  - (kit) `inside_gpu_codes`: Linear gives pattern 1.0; Face gives 12.0; Shade gives row -2.0 and t0 = shade; None gives the default.
- [ ] **Step 2: Run, expect FAIL** (`cargo test -p bpe-gfx inside_`, `cargo test -p bpe-kit inside_gpu`).
- [ ] **Step 3: Implement** the WGSL and `inside_gpu`. Every existing caller of `paint_colour` passes its instance colour as `base`. The old `paint_gpu` inside branch is deleted, and the Path branch stays.
- [ ] **Step 4: Run, expect PASS.** Then run `cargo test -p bpe-gfx -p bpe-render`. Existing picture tests that used the old Inside codes (stripes 1, rings 2) are updated to the new codes (5, 6), not re-blessed blindly. Their pictures must be unchanged.
- [ ] **Step 5: Commit** `gfx: one inside shader for every pattern, with shade of own colour`.

### Task 5: Merge Classic and Styled into one Look (bpe-kit)

**Files:**
- Modify: `crates/bpe-kit/src/modules/look/mod.rs` (remove `ClassicLook`, `ClassicLookKind`), `look/styled.rs` (rename to `look/basic.rs`: `BasicLook`, `BasicLookKind`, id `look.basic`, name "Look")
- Modify: `crates/bpe-kit/src/paint/mod.rs` (Paint loses `inside*` fields and rows; `GroupOpts.inside` is removed in `crates/bpe-core/src/settings.rs`, along with the `inside` attribute in `crates/bpe-settings-derive/src/lib.rs`)
- Modify: every reference found by `grep -rn "look.classic\|look.styled\|ClassicLook\|StyledLook\|Inside::\|\.inside\b" crates`:
  - `lib.rs` (`flow_source`, registry, tests)
  - `random.rs`, `starters.rs`, `graphs.rs`, `music/physics.rs`
  - `things/{balls,arena,obstacle,liquid,soft,text}/draw.rs`
  - `bpe-studio` (`inspector.rs`, `tree.rs`, `widgets.rs`)
- Test: `look/basic.rs` tests, `balls/draw.rs` tests, `text/draw.rs` tests, `soft/draw.rs` tests

**Interfaces:**
- Consumes: `InsidePaint`, `inside_gpu` (Tasks 3 and 4), and `fits` (Task 2).
- Produces: `pub struct BasicLook`, with StyledLook's fields minus `finish` and `pattern`, plus `pub style: Style` (enum `Glow` default, `Flat`, `Shiny`; `#[setting(label = "Style", when = "thing=balls", group = "Style", simple, random)]`) and `pub inside: InsidePaint` (`#[setting(paint, label = "Inside each one", prefix = "in_", group = "Inside each one", when = "thing=balls|text|soft")]`).
  - The `fill` Paint's group/label is "Across the group" when `thing=balls|text|particles|swarm`, else "Colours". Use two embedded rows gated by `when` only if the derive can't vary a label. Otherwise keep the one label "Colours" and let Part 2's section code rename the heading. Pick the simpler one and note the ruling in the ledger.
  - `pub fn look_of(cx: &DrawCx) -> Option<BasicLook>` replaces both readers.

- [ ] **Step 1: Failing tests:**
  - `the_look_registry_has_one_ball_look`: the Look kinds are exactly `["look.basic", "look.liquid", "look.swarm"]`. Update Task 2's assertion to `["look.basic"]` for balls.
  - `glow_circle_draws_through_the_classic_ball_path`: a default look gives `frame.balls` non-empty and `frame.looks` empty.
  - `flat_or_shiny_or_another_shape_draws_a_styled_instance`: Flat, then Circle with Shiny, then Star with Glow each give `frame.looks` non-empty.
  - `inside_linear_on_a_ball_sets_its_paint`: `in_pattern=linear` on Glow Circle gives a ball instance whose paint pattern is 1.0.
  - `an_arena_card_hides_the_ball_only_look_rows`: for `thing=arena`, `style`, `ball_shape`, `in_pattern` and `outline` don't apply. `colours` and `glow` do.
  - Text and soft: the existing `text_outline_and_inside_pattern_paint_too` and `an_inside_pattern_paints_across_the_body_s_box` are rewritten to set `in_pattern` = `stripes` and assert pattern code 5.0.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
  - In `balls/draw.rs`: `style == Glow && ball_shape == Circle` goes through the classic path, with `paint: inside_gpu(...)`. Anything else goes through `styled::instance`, where `form.y` = 0 for Flat or Glow and 1 for Shiny.
  - `styled.wgsl` drops the `fill > 1.5` pattern branch and `color2`. It always calls `paint_colour(paint, p, time, color)` when `paint.row != -1`.
  - Keep `LookInstance` layout. Set `color2` to `color`, or remove it and fix its vertex layout. Remove it if under ~30 lines.
- [ ] **Step 4: Run, expect PASS.** Then `cargo test -p bpe-kit -p bpe-gfx -p bpe-render -p bpe-studio` and `cargo test -p bpe-sim --test golden` (unchanged).
- [ ] **Step 5: Commit** `kit: one Look for every thing, with Style and Inside each one`.

### Task 6: Type and Module menus on each card (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs` (header around the existing `ui.menu_button(state, ...)`, plus the `Edit` enum and its dispatch)
- Test: `crates/bpe-studio/src/inspector.rs` tests

**Interfaces:**
- Consumes: `Scene::set_module_kind` (Task 1) and `Registry::kinds_fitting` (Task 2).
- Produces: `Edit::SetKind(SlotId, &'static str)`. The Type button is labelled with the kind's `name()` and `labelled_by` a hidden-or-small "Type" label. It is drawn only when `kinds_fitting(slot, kind).count() > 1` and the card shows a module. It is disabled for `Write::Inherited`.
  - Its menu lists the kinds (selected = current). For a shared module (`users_of > 1`) it adds a muted first line, "Changes all N things using <name>".
  - The New-module submenu uses `kinds_fitting` too. Everything else in the old picker stays in the Module menu, unchanged.
  - Multi-select: `SetKind` applies once per distinct module among the selected things whose slot shows the same kind.

- [ ] **Step 1: Failing kittest tests:**
  - `an_effects_card_has_a_type_menu_and_a_balls_look_has_none`.
  - `picking_a_type_switches_the_module_and_back_restores_it`: on a Sound card, Notes → Drum machine → Notes keeps `lowest note` as set.
  - `undo_after_a_type_switch_restores_the_module`: drive through `Studio`'s history the way existing undo tests do, or `history.rs` directly if no Studio test harness exists.
  - `a_shared_modules_type_menu_says_how_many_change`.
- [ ] **Step 2: Run, expect FAIL** (`cargo test -p bpe-studio type_`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS.** Then `cargo test -p bpe-studio`.
- [ ] **Step 5: Commit** `editor: Type menu beside the Module menu on every card`.

### Task 7: Rebuild the built-in scenes and check every picture

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs`, `crates/bpe-kit/src/scenes_stage5.rs` (map old looks as in the spec's "Built-in scenes"), `scenes/next/*.ron` (blessed)
- Create (scratch, not committed): `$SCRATCH/compare.sh`
- Output: `/mnt/project-files/ballphysics/editor-layout/part1/` (screenshots, `build-ledger.md`, Windows exes)

- [ ] **Step 1: Before changing the builders, render references from main.** Build `bpe-render-next` at `origin/main`. Run `bpe-render-next scenes/next/<name>.ron $REF/<name>.mp4 --frames 120 --scale 0.5` for all 34 scenes.
- [ ] **Step 2: Map the builders.**
  - Classic becomes Glow.
  - Styled Solid becomes Flat. Shiny becomes Shiny.
  - A Styled pattern becomes Flat plus Inside `<pattern>`, with Own gradient = [old fill colour, old pattern colours…], Repeat Clamp, Size = old `pattern_size`, Angle = old `turn`, Spin = old `spin`, Turns = old `face_motion`.
  - Colour-paints' classic Inside Rings becomes Inside Rings with Own gradient = its old fill gradient, Mirror, Size = old `inside_scale`, Scroll = old `inside_scroll`.
- [ ] **Step 3: Bless and test.** Run `BPE_BLESS=1 cargo test -p bpe-kit scenes`, then `cargo test --workspace`.
- [ ] **Step 4: Render the branch the same way and compare** frame by frame with `ffmpeg -lavfi psnr`. The expected result is PSNR ≥ 45 dB for every scene except the listed expected changes:
  - colour-paints block balls: rings no longer shift colour with each ball's place.
  - Any styled pattern scene whose old pattern used the paint row.
  Write the per-scene PSNR table and each ruling into `build-ledger.md`.
- [ ] **Step 5: Screenshots under Xvfb** (`setsid target/release/ball-studio-next scenes/next/neon-ring.ron`, 1440x2400, `xdotool`, `import -window root`):
  - a ball's Look card showing Style, Across the group and Inside each one;
  - the Effects card's Type menu open;
  - a block of balls with Across Order and Inside Linear 90° Shade -0.6 (the "self" gradient).
  Save them to the part1 folder.
- [ ] **Step 6: Windows exe.** Run `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli` (needs `rustup target add x86_64-pc-windows-gnu` and `gcc-mingw-w64-x86-64`). Copy `ball-studio-next.exe` as `ball-studio.exe`, and `bpe-render-next.exe`, into the part1 folder.
- [ ] **Step 7: Commit** `scenes: rebuild built-in scenes on the merged Look` and push. Open the PR for Part 1.

---

## Self-review notes

- Spec coverage for Part 1:
  - 1.1 header: Task 6. 1.2 switch: Task 1. 1.3 fits: Task 2. 1.4 one Look: Task 5. 1.5 Across: Task 5 (Paint loses inside). 1.6 Inside: Tasks 3 and 4.
  - 1.7 other switches: Tasks 2 and 6 (Effects and Sound tests). Built-in scenes: Task 7. Testing: in each task.
- The spec says colour-paints "looks the same". It can't exactly, because the old inside rings were centred on the across-group colour. This is listed as an expected change in Task 7, and the spec is amended to match.
