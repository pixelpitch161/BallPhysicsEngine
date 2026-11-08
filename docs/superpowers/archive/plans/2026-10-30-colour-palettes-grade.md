# Palettes, harmony and scene grade (colour features part 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Palette menu on every Colours bar (built-in palettes, saved palettes, save, random, "Add matching colours"), and a Grade card on the Scene that colour-grades the finished picture.

**Architecture:** Palettes and harmony are pure functions in `bpe-kit/src/paint/` (`palettes.rs`, `harmony.rs`); saved palettes are a small RON file handled in `bpe-studio/src/palettes.rs`; the gradient bar gains a Palette menu that edits the `Gradient` in place. The grade is a group of Scene settings copied into `Frame.grade`, uploaded in the post globals, and applied in `post.wgsl`'s `fs_composite` after tonemapping on sRGB-encoded values, with a 256×1 gradient-map texture.

**Tech Stack:** Rust workspace, egui 0.37 + egui_kittest (bpe-studio), wgpu/WGSL (bpe-gfx), serde + ron, lavapipe picture tests.

**Spec:** `docs/superpowers/specs/2026-10-30-colour-features-design.md`, sections 8, 9 and 10.

## Global Constraints

- Scenes copy palette colours in; they never link to a palette.
- Saved palettes live in `palettes.ron` beside the app executable; if that folder is not writable, in the settings folder (`bpe_studio::settings::settings_dir()`, which is `%APPDATA%\Ball Studio` on Windows).
- All Grade settings at defaults is an exact no-op: rendered output byte-identical to before.
- The grade runs in screen space, after the shader effect chain, bloom and tonemapping, on display-ready colours.
- All Grade sliders are keyframeable and pulse-able (`keyframe, pulse`).
- Golden physics logs must not change. Maps are `BTreeMap` only. WGSL identifiers must not be `target`, `active` or `move`.
- No `.context_menu` on an egui colour button; use `Popup::context_menu(..).id(...)` / `egui::menu` style menus with explicit ids.
- Editor default font lacks ♭ ♯ ▶ and similar glyphs: plain words only ("Palette", "Random palette").
- Release `cargo test` always fails `bpe-runtime pushing_an_earlier_step_panics_in_debug`; test in debug.
- Commit after each task with the Co-Authored-By/Claude-Session trailer used on this branch.

## Rulings made while planning

- **P3-R1 Harmony result.** "Add matching colours" replaces the gradient's stops with `[first stop's colour, matches…]` evenly spaced (0..1), keeping the transition. Analogous is ordered `[hue−30°, base, hue+30°]` (so the base sits in the middle); all others start with the base.
- **P3-R2 Grade formulas** (on sRGB-encoded `c` in 0..1, in this order): gradient map `c = mix(c, map(luma(c)), map_amount)` with `luma = dot(c, (0.2126, 0.7152, 0.0722))`; brightness `c *= 1 + brightness`; contrast `c = (c − 0.5)·(1 + contrast) + 0.5`; saturation `c = mix(vec3(luma(c)), c, 1 + saturation)`; warmth `c.r += 0.1·warmth; c.b −= 0.1·warmth`; tint `c.g −= 0.1·tint` (positive = magenta); vignette `d = length((uv − 0.5)·2)/√2`, `c *= 1 − vignette·smoothstep(vignette_size, 1, d)`; finally clamp 0..1, then dither as today. Ranges: brightness, contrast, saturation, warmth, tint −1..1 default 0; vignette 0..1 default 0; vignette size 0..1 default 0.5; map amount 0..1 default 0.
- **P3-R3 No-op guarantee by branch.** `fs_composite` applies the grade only when a uniform flag is set; the Rust side sets it only when any grade value differs from default. The unflagged path is today's code unchanged.
- **P3-R4 Palette store reach.** The gradient bar finds the saved palettes through egui context data (`palettes::store(ctx) -> Arc<Mutex<PaletteStore>>`), inserted once by the app at start-up; tests insert a store pointing at a temp dir. Without a store in the context the menu shows built-ins only and hides "Save as palette".

## Review Focus

1. A missing, empty or corrupt `palettes.ron`: the editor must start and show built-ins; a corrupt file is not overwritten until the user saves, and then a backup `palettes.ron.bak` keeps the old text. (Tests in Task 2.)
2. Applying a palette to a one-stop Colours (single swatch): it must become a multi-stop gradient, not keep only the first colour. (Test in Task 3.)
3. Harmony on an additive or transparent first stop: matches keep the base's alpha/additive-ness; nothing turns black. (Test in Task 1.)
4. Grade with keyframed values passing exactly through defaults mid-video: no flicker beyond the tolerance of the graded path; at the exact default frame the output equals the ungraded output. (Test in Task 4: `grade_at_defaults_is_byte_identical`.)
5. Vignette size 1 (edge of range) and 0: no NaN from `smoothstep` with equal edges. (Test in Task 4.)

---

### Task 1: Built-in palettes, harmony helper, palette dice

**Files:**
- Create: `crates/bpe-kit/src/paint/palettes.rs`, `crates/bpe-kit/src/paint/harmony.rs` (export from `paint/mod.rs`)
- Modify: `crates/bpe-kit/src/random.rs` (`palette()`)
- Test: in those files

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Palette { pub name: String, pub colours: Gradient }`
  - `pub fn built_in() -> Vec<Palette>` — at least these 20, in this order, each 3-6 stops, transition Smooth unless noted: Sunset, Neon, Pastel, Ocean, Forest, Candy, Fire, Ice, Mono (black to white), Retro, Vapor, Earth, Aurora, Berry, Citrus, Desert, Lagoon, Midnight, Rainbow (transition Hue), Gold. Colours are the implementer's choice; names are fixed.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Harmony { Complementary, Triad, Analogous, SplitComplementary, Shades, Tints }` with `Harmony::ALL` and `label()` ("Complementary", "Triad", "Analogous", "Split complementary", "Shades", "Tints").
  - `pub fn matching(base: Color, h: Harmony) -> Vec<Color>` (the new colours only): Complementary +180°; Triad +120°, +240°; Analogous −30°, +30°; SplitComplementary +150°, +210°; Shades value ×0.75, ×0.5, ×0.25; Tints mixed toward white by 0.25, 0.5, 0.75. Each keeps `base`'s alpha (and so its additive-ness).
  - `pub fn add_matching(g: &Gradient, h: Harmony) -> Gradient` per ruling P3-R1.
  - `random::palette(dice, n)`: with probability 0.5 picks a random built-in palette and samples it at `n` evenly spaced points (n = 1 → its middle), otherwise today's procedural palette.
- [ ] **Step 1: Failing tests**
  - `there_are_twenty_named_palettes` (names exactly the list above, all unique, each 3..=6 stops, stops sorted).
  - `complementary_is_opposite_hue` (red → one colour with hue 180° ± 1°).
  - `analogous_puts_the_base_in_the_middle` (`add_matching` of a 1-stop red gives 3 stops at 0, 0.5, 1 with the middle red).
  - `shades_get_darker_and_tints_lighter`.
  - `matching_keeps_alpha_and_additive` (base `Color::additive(..)` → every match `is_additive()`; base alpha 128 → all alpha 128).
  - `add_matching_keeps_the_transition`.
  - `palette_dice_sometimes_returns_a_built_in_palette` (over 200 seeds, at least one result equals a built-in sampled at n=3; same seed → same result).
- [ ] **Step 2:** `cargo test -p bpe-kit palettes harmony random` → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace` passes (re-bless scenes only if a built-in scene uses the dice at build time; golden physics unchanged). **Step 5:** commit `feat(colour): built-in palettes and harmony helper`.

### Task 2: Saved palettes file

**Files:**
- Create: `crates/bpe-studio/src/palettes.rs` (register in `main.rs`/`lib` module list like other studio modules)
- Modify: `crates/bpe-studio/src/app.rs` (create the store at start-up and insert it into the egui context per P3-R4)
- Test: `crates/bpe-studio/src/palettes.rs` tests with a temp dir

**Interfaces:**
- Consumes: `bpe_kit::paint::palettes::Palette`.
- Produces:
  - `pub struct PaletteStore { path: PathBuf, pub saved: Vec<Palette>, broken: bool }`
  - `PaletteStore::load_from(path: PathBuf) -> PaletteStore` (missing → empty; unreadable/corrupt → empty with `broken = true`)
  - `PaletteStore::open() -> PaletteStore`: path `palettes.ron` next to `std::env::current_exe()`, if that folder is writable (probe by creating and removing a temp file); else `settings_dir()/palettes.ron`.
  - `PaletteStore::save_as(&mut self, name: &str, colours: &Gradient) -> std::io::Result<()>`: trims the name; empty → "My palette"; an existing name is replaced (same position); writes atomically (write `palettes.ron.tmp`, rename); if `broken`, first copies the old file to `palettes.ron.bak`, then clears `broken`.
  - `PaletteStore::remove(&mut self, name: &str) -> std::io::Result<()>`.
  - `pub fn store(ctx: &egui::Context) -> Option<Arc<Mutex<PaletteStore>>>` and `pub fn install(ctx: &egui::Context, store: PaletteStore)`.
  - File format: RON list of `Palette` (`Vec<Palette>`), pretty-printed.
- [ ] **Step 1: Failing tests**: `missing_file_loads_empty`, `save_then_load_round_trips`, `saving_an_existing_name_replaces_it`, `a_corrupt_file_is_kept_as_bak_on_first_save` (write "nonsense(" → load → `saved` empty → `save_as` → `.bak` holds "nonsense(" and the file now parses), `empty_name_becomes_my_palette`, `remove_deletes_by_name`.
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** `cargo test -p bpe-studio palettes` and workspace pass. **Step 5:** commit `feat(colour): saved palettes file`.

### Task 3: Palette menu on the Colours bar

**Files:**
- Modify: `crates/bpe-studio/src/gradient_bar.rs` (a "Palette" menu button beside the transition menu, and beside the one-colour swatch's "+")
- Test: kittests in `gradient_bar.rs` (follow its existing tests) and one inspector kittest

**Interfaces:**
- Consumes: Task 1 (`built_in`, `Harmony`, `add_matching`, `random::palette` not required — the menu's random item uses `built_in()` plus a seed from `random::fresh_seed()` mapped to `palette(dice, max(stops, 3))`), Task 2 (`palettes::store`, `PaletteStore::save_as`/`remove`).
- Produces: menu button with accessible name `"<name> palette"` (e.g. "Colours palette"), items:
  - one per built-in palette (a small gradient swatch + its name); clicking replaces stops and transition with the palette's;
  - a "Saved" section listing the store's palettes (same behaviour; each has a small "Remove" item in a sub-menu);
  - "Save as palette..." opening a name field with "Save" / "Cancel" (hidden without a store);
  - "Random palette";
  - "Add matching colours" sub-menu with the six `Harmony` labels, applying `add_matching`.
  - Every change marks the bar's response changed, so the inspector writes the setting (and keyframes it if keyed) exactly as dragging a stop does.
- [ ] **Step 1: Failing kittests**: `picking_a_palette_replaces_the_stops` (choose "Ocean" → gradient equals `built_in()` Ocean), `a_one_colour_bar_takes_a_whole_palette` (1 stop → Ocean's stop count), `add_matching_triad_gives_three_stops`, `save_as_palette_adds_it_to_the_saved_list` (store in temp dir; type a name; Save; store contains it and the file exists), `no_store_hides_save_as`, `random_palette_changes_the_colours`.
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** studio tests and workspace pass; clippy clean. **Step 5:** commit `feat(colour): Palette menu on every Colours bar`.

### Task 4: Scene colour grade

**Files:**
- Modify: `crates/bpe-kit/src/things/scene/mod.rs` (Grade settings group), `things/scene/draw.rs` (fill `frame.grade`)
- Modify: `crates/bpe-gfx/src/frame.rs` (`Frame.grade: Grade`; `clear()` resets; overlay keeps none), `crates/bpe-gfx/src/lib.rs` (post globals gain grade values + flag; 256×1 `Rgba16Float` map texture bound to the composite bind group, written when `map_amount > 0`), `crates/bpe-gfx/src/shaders/post.wgsl` (`fs_composite`)
- Test: `crates/bpe-gfx/src/grade_tests.rs` (lavapipe), kit scene settings tests

**Interfaces:**
- Produces:
  - `#[derive(Clone, Debug, PartialEq)] pub struct Grade { pub brightness: f32, pub contrast: f32, pub saturation: f32, pub warmth: f32, pub tint: f32, pub vignette: f32, pub vignette_size: f32, pub map: Gradient, pub map_amount: f32 }` in `bpe-gfx` with `Default` (values per P3-R2; `map` = black→white) and `fn is_default(&self) -> bool` (ignores `map` when `map_amount == 0`).
  - Scene settings keys (group "Grade", all `keyframe, pulse` except the gradient which is `keyframe`): `grade_brightness` "Brightness", `grade_contrast` "Contrast", `grade_saturation` "Saturation", `grade_warmth` "Warm / cool", `grade_tint` "Tint", `grade_vignette` "Vignette", `grade_vignette_size` "Vignette size", `grade_map` "Gradient map" (a `Gradient`), `grade_map_amount` "Gradient map amount".
  - Shader per ruling P3-R2 and P3-R3.
- [ ] **Step 1: Failing tests**
  - `grade_at_defaults_is_byte_identical` (render a frame with background gradient + a ball at default grade vs. a renderer with no grade field touched; bytes equal).
  - `brightness_up_brightens_and_contrast_spreads` (mid-grey background: brightness 0.5 → pixel brighter; contrast 0.5 on a two-tone background → difference grows).
  - `saturation_minus_one_is_grey` (r = g = b ± 1/255).
  - `warm_adds_red_takes_blue`; `tint_positive_is_magenta` (green drops).
  - `vignette_darkens_corners_not_centre` (centre ± 1/255 of ungraded; corner darker).
  - `vignette_size_edges_are_finite` (size 0 and 1, vignette 1; no NaN, corners ≤ centre).
  - `gradient_map_full_maps_black_to_first_stop` (black background, map red→blue amount 1 → pixel red).
  - Kit: scene defs contain all nine keys in group "Grade"; scene with defaults gives `frame.grade.is_default()`.
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace`, clippy; every built-in scene picture byte-identical (grade off). **Step 5:** commit `feat(colour): scene colour grade`.

### Task 5: Demo scene, editor check and deliverables

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (new built-in "Graded sunset": balls painted with the Sunset palette, a Triad-matched wall, grade warmth 0.3, contrast 0.2, vignette 0.4, gradient map Sunset at 0.25 with its amount keyframed 0→0.5 over the video); bless `scenes/next/graded-sunset.ron`
- Test: inspector kittest `scene_card_shows_the_grade_group` (Scene card shows "Brightness", "Vignette", "Gradient map")
- [ ] **Step 1:** failing kittest. **Step 2:** run → fail. **Step 3:** implement scene and any inspector tweak needed (Grade group folds like other groups). **Step 4:** full tests + clippy; render `graded-sunset` to mp4 and a frame PNG, and an editor screenshot with the Palette menu open, into `/mnt/project-files/ballphysics/colour-features/part3/`. **Step 5:** commit `feat(colour): Graded sunset demo scene`.

### Task 6: Blend with each other (added at Aedan's request, 2026-10-31)

Aedan asked (thread, 2026-10-31 00:18Z): balls of one group should be able to blend with each other, not only with what is behind them. Today a blended thing's instances are drawn into one layer where they cover each other, and the layer is composited once (part 2 ruling P2-R1/P2-R2).

**Ruling P3-R5 (how):** a new switch `blend_each` ("Blend with each other", group "Blending", default off, shown when `blend` is Add, Screen, Multiply, Lighten, Darken or Subtract) on `look.classic`, `look.styled` and `look.swarm`. When on, the thing's layer is keyed separately (`BlendLayer.each: bool`), cleared to the mode's identity, and its instances are drawn into it with a fixed-function blend state for that mode so overlapping instances combine with each other by the mode; the composite then combines the layer with the picture by the same mode and amount, treating the whole layer as covering wherever any instance drew (coverage tracked in alpha with Max). Identities and in-layer blend states: Add/Screen/Lighten clear 0 (Add One+One; Screen One, OneMinusSrc on clamped src; Lighten Max), Multiply clear 1 (Dst, Zero), Darken clear a large value (Min), Subtract clear 0 and accumulate the subtracted light with One+One, composite as d − layer. Modes Overlay, Soft light, Difference, Hue and Colour keep today's behaviour and hide the switch. Pipelines per (instance kind, mode) are created lazily and cached.

**Files:**
- Modify: `crates/bpe-kit/src/modules/look/mod.rs`, `look/styled.rs`, `look_swarm/mod.rs` (setting); `crates/bpe-core/src/blend.rs` (`Blend::combines_with_itself(self) -> bool` true for the six modes; `from_params` gains reading `"blend_each"` → keep signature, add `Blend::each_from_params(&Params) -> bool`); `crates/bpe-gfx/src/frame.rs` (`BlendLayer.each`, `Frame::layer_mut(blend, amount, each)` — `each` only kept when `blend.combines_with_itself()`); `crates/bpe-gfx/src/lib.rs`, `blend.rs`, `shaders/blend.wgsl` (identity clear, per-mode in-layer pipelines, composite of an `each` layer)
- Test: `crates/bpe-gfx/src/blend_tests.rs`, kit layer tests, look settings tests

- [ ] **Step 1: Failing tests**
  - `two_overlapping_screen_balls_blending_with_each_other_are_brighter_where_they_overlap` (switch on: overlap pixel brighter than either single-ball pixel; switch off: overlap equals single).
  - `multiply_balls_with_each_other_darken_their_overlap`.
  - `add_with_each_other_matches_adding_both` (overlap ≈ d + s1 + s2 within 2/255 before tonemap reasoning — use a dark background and low intensities so nothing clips).
  - `darken_each_leaves_uncovered_pixels_untouched` (byte-equal outside the balls).
  - `each_layers_and_plain_layers_do_not_share` (same mode/amount, one with the switch and one without → two layers).
  - Settings: `blend_each` key exists on the three looks, default false, hidden (its `when`) for Overlay.
  - Every built-in scene renders byte-identically (switch defaults off).
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** full tests, clippy, fmt; byte-identity check of all scenes/next/*.ron. **Step 5:** commit `feat(colour): blend with each other`.
