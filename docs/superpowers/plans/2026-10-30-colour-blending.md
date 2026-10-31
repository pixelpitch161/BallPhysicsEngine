# Colour blending and variation (colour features part 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every visible thing gets a Blend mode and Blend amount, and every Paint gets per-instance colour variation (hue, saturation, brightness) with an optional slow wobble.

**Architecture:** A `Blend` choice lives in `bpe-core`. Draw modules carry `blend` + `blend_amount`. `build_frame_at` routes each thing (by its Look slot) and each module slot (by its own settings) either into the main `Frame` or into a `BlendLayer` sub-frame; the renderer draws each layer into its own HDR texture and a composite pass mixes it over a copy of the picture with the mode's formula. Variation is computed per instance from a seed in `paint::colour_at` (CPU) and passed as a per-instance HSV offset in `PaintGpu` for inside patterns (GPU).

**Tech Stack:** Rust workspace, wgpu + WGSL (bpe-gfx), egui 0.37 (bpe-studio), lavapipe for picture tests.

**Spec:** `docs/superpowers/specs/2026-10-30-colour-features-design.md`, sections 6 and 7 (and the Paint table in section 2).

## Global Constraints

- Golden **physics** logs must not change (colours never touch physics).
- With every new setting at its default (Blend Normal, amount 1, all vary 0, wobble 0) every existing scene renders **byte-identical** to main.
- Clean break is allowed: old scene files need not load; built-in `scenes/next/*.ron` are re-blessed with `BPE_BLESS=1`.
- Maps are `BTreeMap` only. WGSL identifiers must not be `target`, `active` or `move`.
- Two-clock rule: wobble runs on the simulation clock (`cx.time`), beats on the video clock.
- Editor default font lacks some glyphs: labels use plain words.
- No `.context_menu` on an egui colour button (rule from #43).
- Release `cargo test` always fails `bpe-runtime pushing_an_earlier_step_panics_in_debug`; run tests in debug.
- Commit after each task with the Co-Authored-By/Claude-Session trailer used on this branch.

## Rulings made while planning (deviations from the spec, with reasons)

- **P2-R1 One layered path for every non-trivial mode.** The spec puts Screen/Multiply/Lighten/Darken/Subtract on fixed-function blend states. With premultiplied HDR draws, Darken (Min) turns uncovered pixels black and Screen needs clamping, so every mode other than "Normal at amount 1" and "Add handled by trails/particles" goes through the layer + composite shader. Cost: one layer clear + one picture copy per (mode, amount) group per frame, zero when nothing uses blending.
- **P2-R2 Blended things draw above normal things.** Layers are composited after the main frame's flashes/particles/glyphs pass and before the stuck-to-screen overlay (whose own layers composite after it). Within one layer, the normal per-kind order applies.
- **P2-R3 Where the setting lives.** `blend`/`blend_amount` are module settings on `look.classic`, `look.styled`, `look.liquid`, `look.swarm`, `trail.streak` and the particles module. A thing's own draw (ThingDraw) follows its Look slot's blend. The scene background has no blend.
- **P2-R4 Trails and particles keep their cheap additive path.** `ModuleDraw::draws_add_itself() -> bool` (default false, true for trail and particles): for those, Add (any amount, colour scaled by amount) and Normal (alpha scaled by amount) stay in the main frame. The old `TrailBlend` and `BurstBlend` enums are removed; their "Cover" is `Normal`, particles' "Auto" is gone (default Add; built-in scenes that relied on Auto choosing cover write `Normal`).
- **P2-R5 Variation math.** Per instance, three values r1..r3 in [-1, 1] from `seed` (fixed hash, or smooth value noise over `time * wobble` when wobble > 0). Hue += `vary_hue * r1` degrees; saturation = clamp(s + `vary_sat * r2`, 0, 1); value *= max(0, 1 + `vary_bright * r3`). Alpha is untouched; additive colours vary their light the same way.
- **P2-R6 No variation on liquid and background** (one fluid / one picture). New `GroupOpts.vary: bool` (derive option `no_vary` sets false) hides the four keys there. Swarm varies per point in its shader; particles vary per burst.
- **P2-R7 The colour picker's "Additive" checkbox is removed** (`Alpha::OnlyBlend`). `Color::additive` stays as the internal encoding. Built-in scenes with additive colours switch to opaque colours + Blend Add.

## Review Focus

1. A thing with Blend ≠ Normal that is also stuck to the screen: it must blend into the screen overlay, not the world layer, and not vanish. (Test in Task 2.)
2. Blend amount keyframed to 0: the thing must look exactly as if absent, not black or doubled. (Test in Task 3: amount 0 equals no draw.)
3. Darken / Multiply on a thing with transparent surroundings: pixels the thing does not cover must be unchanged. (Test in Task 3.)
4. Variation with many balls and wobble on while scrubbing backwards: colours are a pure function of (seed, time), never accumulated state. (Test in Task 5: same inputs, same colour, any call order.)
5. An additive colour inside a layered blend (alpha encodes light): no NaN/inf from un-premultiplying. (Test in Task 3.)

---

### Task 1: Blend setting on every draw module

**Files:**
- Create: `crates/bpe-core/src/blend.rs` (export from `lib.rs`)
- Modify: `crates/bpe-kit/src/modules/look/mod.rs`, `look/styled.rs`, `look_liquid/mod.rs`, `look_swarm/mod.rs`, `trail/mod.rs`, `trail/draw.rs`, `particles/mod.rs`, `particles/draw.rs`, `crates/bpe-kit/src/scenes.rs` (+ `scenes/` submodules) for any `TrailBlend`/`BurstBlend` use
- Test: in the same files' `#[cfg(test)]` modules

**Interfaces:**
- Produces: `bpe_core::Blend` — `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Choice)] enum Blend { #[default] Normal, Add, Screen, Multiply, Lighten, Darken, Subtract, Overlay, SoftLight, Difference, Hue, Colour }`, with `Blend::from_params(params: &Params) -> (Blend, f32)` reading keys `"blend"` (Choice) and `"blend_amount"` (number, default 1.0, clamped 0..1), and `Blend::shader_code(self) -> u32` (Normal 0, Add 1, Screen 2, Multiply 3, Lighten 4, Darken 5, Subtract 6, Overlay 7, SoftLight 8, Difference 9, Hue 10, Colour 11).
- Every listed module struct gains `blend: Blend` (`#[setting(label = "Blend", group = "Blending")]`) and `blend_amount: f32` (`#[setting(label = "Blend amount", range = 0.0..=1.0, group = "Blending")]`, default 1.0, keyframeable like other numbers). Trail default `Blend::Add`; particles default `Blend::Add`; looks default `Normal`.
- Choice labels shown in the editor: Normal, Add, Screen, Multiply, Lighten, Darken, Subtract, Overlay, Soft light, Difference, Hue, Colour.

- [ ] **Step 1: Failing tests**
  - `bpe-core::blend::tests::from_params_reads_mode_and_clamps_amount`: params `{"blend": Choice("multiply"), "blend_amount": 1.7}` → `(Blend::Multiply, 1.0)`; empty params → `(Blend::Normal, 1.0)`.
  - `bpe-kit` look tests: `ClassicLook::default().blend == Blend::Normal`, and its defs contain keys `blend` and `blend_amount`; round trip through params keeps `Blend::Screen, 0.25`.
  - Trail: `StreakTrail::default().blend == Blend::Add`; `shade()` with `Normal, 0.5` gives alpha halved; with `Add, 0.5` gives RGB halved and alpha −1 (today's Add encoding).
  - Particles: default `Add`; `Normal` gives `Burst.additive == false`; `Add` gives `true` with colour scaled by amount.
- [ ] **Step 2:** `cargo test -p bpe-core blend && cargo test -p bpe-kit look trail particles` → fail.
- [ ] **Step 3:** Implement. Remove `TrailBlend`, `BurstBlend`, `HitBurst::additive`'s Auto/preset logic; update every use. Built-in scenes that had particles on Auto with a non-glowing preset get `blend: Blend::Normal` so their pictures do not change.
- [ ] **Step 4:** `cargo test --workspace` passes after `BPE_BLESS=1 cargo test -p bpe-kit scenes`; golden physics unchanged.
- [ ] **Step 5:** Commit `feat(colour): Blend and Blend amount settings on draw modules`.

### Task 2: Route blended things into layers (kit/frame side)

**Files:**
- Modify: `crates/bpe-gfx/src/frame.rs` (`Frame`, `ModuleDraw`, `build_frame_at`, `clear`, `lift_overlay_extras`)
- Modify: `crates/bpe-kit/src/modules/trail/draw.rs`, `particles/draw.rs` (implement `draws_add_itself`)
- Test: `crates/bpe-gfx/src/frame.rs` tests and a kit-level test building a frame from a small scene (follow the existing kit frame tests)

**Interfaces:**
- Consumes: `Blend::from_params` (Task 1).
- Produces:
  - `pub struct BlendLayer { pub blend: Blend, pub amount: f32, pub frame: Frame }`; `Frame.layers: Vec<BlendLayer>` (cleared by `clear()`, kept in first-use order).
  - `Frame::layer_mut(&mut self, blend: Blend, amount: f32) -> &mut Frame` — returns `self` when `blend == Normal && amount >= 1.0`, else the layer with equal `(blend, amount.to_bits())`, created with this frame's width/height/time/camera/video/gradients settings copied as `overlay_mut` does.
  - `ModuleDraw::draws_add_itself(&self) -> bool { false }`.
  - Routing in `build_frame_at`: target = overlay if `r.screen` else main; then for ThingDraw, `(b, a)` from the thing's `SlotId::Look` params (Normal/1 if no look); for each module slot, from that slot's params; if the module `draws_add_itself()` and `b` is Normal or Add, draw into the target directly; otherwise into `target.layer_mut(b, a)`.
  - Layer frames share the main frame's `GradientAtlas` rows: layers are drawn by the same renderer call, so `paint_gpu` rows must index the atlas that will be uploaded. Use the same approach as overlay bursts (`GradientAtlas::adopt`) when lifting, or have `layer_mut` frames write into the parent's atlas — pick one and test it.
- [ ] **Step 1: Failing tests**
  - `a_multiply_look_puts_the_ball_in_a_layer`: scene with one ball, look blend Multiply 1.0 → `frame.balls` (or `looks`) empty, `frame.layers.len() == 1`, layer has the ball, `layer.blend == Multiply`.
  - `normal_at_full_amount_stays_in_the_main_frame` (no layers).
  - `a_stuck_blended_thing_goes_into_the_overlays_layer`: `screen: Some(true)` + Screen blend → `frame.overlay.unwrap().layers.len() == 1`, `frame.layers.is_empty()`.
  - `trail_add_stays_in_the_main_frame_but_trail_screen_uses_a_layer`.
  - `two_things_with_the_same_mode_and_amount_share_one_layer`.
  - `a_layered_inside_pattern_keeps_a_valid_atlas_row`: layered ball with Stripes → its `paint.row` refers to a row present in the atlas that the renderer will upload.
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace` passes; all scene pictures unchanged (nothing uses blending yet). **Step 5:** commit `feat(colour): route blended things into layers`.

### Task 3: Draw and composite blend layers on the GPU

**Files:**
- Create: `crates/bpe-gfx/src/shaders/blend.wgsl`, `crates/bpe-gfx/src/blend.rs` (pipeline + bind group + `composite` call)
- Modify: `crates/bpe-gfx/src/lib.rs` (`Targets`: a `layer` HDR texture with RENDER_ATTACHMENT|TEXTURE_BINDING, and a `under` HDR copy with TEXTURE_BINDING|COPY_DST — or reuse `behind` if its contents are not needed afterwards; `Renderer::render`, `draw_overlay`)
- Test: `crates/bpe-gfx/src/blend_tests.rs` (lavapipe; follow `paint_tests.rs` helpers `gpu()`, `render()`)

**Interfaces:**
- Consumes: `Frame.layers`, `Blend::shader_code`.
- Produces: `Renderer` draws, for each layer of the main frame after the "flashes" pass and before `draw_overlay`, and for each overlay layer after the overlay: clear `layer` to transparent; draw the layer frame's trails, shapes, soft, swarms, liquids (their composite targets `layer`), balls, looks, flashes, particles and glyphs into it (refactor the existing pass bodies into one `draw_things(&mut encoder, frame, view, globals)` used by the scene/balls/flashes passes, the overlay and layers); copy `hdr` → `under`; full-screen composite into `hdr` with blend None.
- Composite formula (in `blend.wgsl`, premultiplied layer colour `c`, coverage `a`, picture `d`):
  - Additive-encoded light (a ≈ 0 with light): use `a' = clamp(max(c.r, c.g, c.b), 0, 1)`, `s = c / max(a', 1e-4)`; otherwise `s = c / max(a, 1e-4)`. Never divide by zero.
  - `B(d, s)`: Normal `s`; Add `d + s`; Screen `1 − (1 − clamp(d))(1 − clamp(s))`; Multiply `d·s`; Lighten `max`; Darken `min`; Subtract `max(d − s, 0)`; Overlay, Soft light (W3C compositing spec formulas on clamped 0..1 inputs); Difference `|d − s|`; Hue and Colour (W3C non-separable SetLum/SetSat/ClipColor on clamped inputs).
  - Result `mix(d, B(d, s), a · amount)`; Add uses `d + c · amount` directly (no coverage mix) so additive light stays additive.
- [ ] **Step 1: Failing picture tests** (frame 64×64, background a flat colour `d`, one layered full-screen-covering shape or large ball of colour `s`; read the centre pixel; compare to the formula computed in Rust with tolerance 2/255; sample a corner pixel the thing does not cover and assert it equals the background exactly):
  - `each_blend_mode_matches_its_formula` (loop all 12 modes; d = (0.6, 0.3, 0.2), s = (0.2, 0.5, 0.9)).
  - `blend_amount_zero_leaves_the_picture_unchanged` (Multiply, amount 0 → whole picture equals a frame without the thing, byte for byte).
  - `normal_at_half_amount_is_half_way`.
  - `darken_does_not_touch_uncovered_pixels`.
  - `an_additive_colour_in_a_layer_stays_finite` (alpha-0 light colour with Overlay; no NaN; pixel finite and plausible).
  - `a_stuck_layer_blends_over_the_overlay`.
- [ ] **Step 2:** run (`cargo test -p bpe-gfx blend`) → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace` passes; all built-in scene pictures byte-identical (render each and compare with main, as part 1 did). **Step 5:** commit `feat(colour): blend layers and composite pass`.

### Task 4: Remove the Additive checkbox and switch scenes to Blend Add

**Files:**
- Modify: `crates/bpe-studio/src/widgets.rs`, `crates/bpe-studio/src/gradient_bar.rs` (`Alpha::OnlyBlend`; update tests at widgets.rs ~953/975 that look for the "Additive" label to look for the colour picker's alpha slider or another picker label instead)
- Modify: built-in scenes (`crates/bpe-kit/src/scenes.rs`, `scenes/*.rs`) and the randomize dice (`crates/bpe-kit/src/random.rs`) wherever they produce additive (alpha 0 with light) colours
- Test: studio widget tests; a kit test that no built-in scene contains an additive colour

**Interfaces:**
- Consumes: Blend settings (Task 1), layers (Tasks 2-3).
- Produces: `bpe_kit::scenes::tests::no_built_in_scene_uses_additive_colours` (walk every `Value::Color` and `Value::Gradient` stop of every built-in scene; none `is_additive()`).
- [ ] **Step 1:** failing tests (the scene walk; the studio test asserting the picker has no "Additive" text).
- [ ] **Step 2:** run → fail. **Step 3:** replace each additive colour with its opaque light colour and set the owning module's `blend: Add`; for a gradient that mixes additive and covering stops, keep covering stops and pick the blend the majority used, and note it in the commit message. Dice must not roll additive colours. **Step 4:** re-bless scenes; render every built-in scene and compare with main; differences allowed only where an additive look moved into a layer (max 2/255 per channel); list them in the task report. `cargo test --workspace` passes. **Step 5:** commit `feat(colour): Blend Add replaces the additive colour checkbox`.

### Task 5: Variation settings and CPU colour variation

**Files:**
- Modify: `crates/bpe-core/src/settings.rs` (`GroupOpts.vary: bool`, default true), `crates/bpe-settings-derive/src/lib.rs` (option `no_vary`)
- Modify: `crates/bpe-kit/src/paint/mod.rs` (fields, defaults, `group_defs`/`group_read`/`group_write`), `paint/eval.rs` (`Inputs.seed`, variation in `colour_at`), new `paint/vary.rs`
- Modify: `look_liquid/mod.rs` and `things/scene/mod.rs` paint fields to use `no_vary`
- Test: `paint/vary.rs` and `paint/mod.rs` tests

**Interfaces:**
- Produces:
  - `Paint` gains `vary_hue: f32` (degrees, range 0..=180, label "Vary hue"), `vary_sat: f32` (0..=1, "Vary saturation"), `vary_bright: f32` (0..=1, "Vary brightness"), `wobble: f32` (Hz, 0..=5, "Wobble", shown when any vary > 0 is not expressible, so always shown with the other three). Keys are `prefix + "vary_hue"` etc., all keyframeable, none `simple`, inspector group "Variation". All default 0.
  - `Inputs.seed: u32` (default 0).
  - `pub fn seed_of(thing: ThingId, index: usize) -> u32` in `paint/vary.rs` (stable hash; never the same for index 0 and 1 of one thing).
  - `pub fn vary_offsets(p: &Paint, seed: u32, time: f32) -> Option<[f32; 3]>` → `None` when all three amounts are 0; else `[hue_degrees, sat_add, bright_mul]` per ruling P2-R5.
  - `pub fn apply_vary(rgba: [f32; 4], v: [f32; 3]) -> [f32; 4]` (HSV via `bpe_core::Color`'s to_hsv/from_hsv maths on f32; HDR values above 1 keep their scale through V).
  - `colour_at` applies `apply_vary` after Follow when `vary_offsets` is `Some`, using `Inputs.time` (simulation clock).
- [ ] **Step 1: Failing tests**
  - `zero_variation_is_an_exact_no_op` (`vary_offsets` None; `colour_at` bit-identical to before for a few paints).
  - `different_seeds_give_different_hues_within_the_range` (vary_hue 30: 50 seeds, every hue offset in [−30, 30], at least 40 distinct).
  - `fixed_offsets_do_not_change_with_time_when_wobble_is_zero`.
  - `wobble_changes_smoothly_and_is_a_pure_function_of_time` (t and t+0.001 differ by < 0.05 of range; evaluating t=5 then t=1 then t=5 gives identical results).
  - `saturation_and_brightness_are_clamped` (no negative channels, sat in 0..1).
  - Paint round trip includes the four keys; `no_vary` group has none of them in its defs.
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace` passes; pictures unchanged. **Step 5:** commit `feat(colour): per-instance colour variation`.

### Task 6: Variation for every thing, on CPU and GPU

**Files:**
- Modify: callers that build `Inputs` (`look/draw.rs` BallInputs/WallInputs, `things/soft/draw.rs`, `things/text/draw.rs`, `modules/trail/draw.rs`, `modules/particles/draw.rs`) to set `seed` via `seed_of(thing, index)` (trail points use their ball's seed; particles use `seed_of(thing, burst index)` or the existing `seed_of(thing, mark)`)
- Modify: `crates/bpe-gfx/src/gradients.rs` (`PaintGpu.vary: [f32; 4]` = hue turns, sat add, bright mul, unused; default `[0, 0, 1, 0]`), its vertex attribute layout(s), every WGSL struct receiving paint, `shaders/paint.wgsl` (apply after sampling the row), `crates/bpe-kit/src/paint/gpu.rs` (`paint_gpu` fills `vary` from `vary_offsets(p, inputs.seed, inputs.time)`)
- Modify: swarm (`look_swarm`, `bpe-gfx` `SwarmLook` + `flow/swarm_draw.wgsl`): `SwarmLook.vary: [f32; 4]` = (vary_hue degrees, vary_sat, vary_bright, wobble Hz), applied per point with the shader's `draw_rand` and frame time
- Test: kit tests per kind; `paint_tests.rs` GPU tests

**Interfaces:**
- Consumes: Task 5 functions.
- [ ] **Step 1: Failing tests**
  - Kit: `a_ball_group_with_vary_hue_has_different_colours` (8 balls, one colour, vary_hue 40 → at least 6 distinct instance colours; same frame built twice is identical).
  - Kit: soft points, text letters and wall pieces each get distinct colours with variation on.
  - GPU: `inside_pattern_varies_per_ball` (two balls with Stripes and vary_hue 90 → centre pixels differ; with vary 0 identical to main).
  - GPU: `swarm_points_vary` (vary_bright 1 → pixel brightness spread across points > 0).
- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`; every built-in scene picture byte-identical to the end of Task 4. **Step 5:** commit `feat(colour): variation for every thing`.

### Task 7: Demo scene, editor check and deliverables

**Files:**
- Modify: `crates/bpe-kit/src/scenes.rs` (new built-in "Blend modes": a dark-to-light background gradient, four rows of overlapping balls using Multiply, Screen, Overlay and Difference, one wall with Subtract, a trail with Add, a styled-ball group with vary_hue 25 + wobble 0.5); add to the scene list; `scenes/next/blend-modes.ron` via bless
- Test: the existing scene tests; an inspector kittest that a look card shows "Blend" and "Blend amount" and a Paint shows "Vary hue"

- [ ] **Step 1:** failing kittest `look_card_shows_blending_and_variation`. **Step 2:** run → fail. **Step 3:** implement scene and anything the kittest needs (Blending/Variation groups fold like other groups). **Step 4:** full `cargo test --workspace`, clippy; render `blend-modes` to mp4 with `bpe-render-next` and save a frame PNG and an editor screenshot (as part 1 did) into `/mnt/project-files/ballphysics/colour-features/part2/`. **Step 5:** commit `feat(colour): Blend modes demo scene`.
