# Colour grading part 1: the grade itself — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the old basic grade with the start of the Lightroom-style grade: a Grade card on Scene settings
whose Whole picture holds Basic and Tone Curve panels in a real processing order, run on the GPU on the linear HDR
picture before the roll-off; and Grade mode in the editor (Build | Grade switch and D, the masks list with only
Whole picture, the panel column with collapse, on/off, drag to reorder, Add panel, Reset panel, Remove, Reset order,
After / Before / Split, the Histogram, double-click reset).

**Spec:** `docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved by Aedan 2026-11-09), part 1 of 6.
Mockups: project files `ballphysics/colour-grading/mockups/` (layout B, Grade mode).

**Tech:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37. Code map: `docs/notes/file-map.md` ("Where the colour grading
goes").

## Decisions made here (the spec leaves them to the plan)

1. **One fixed grade shader that reads the stack as a program.** The composite pass reads a storage buffer holding
   the compiled stack (a list of steps, each a kind, a working space and its numbers, curves as lookup rows) and runs
   it in a loop. Nothing is compiled per grade, so a slider never rebuilds a shader and a new panel order never
   does either (the spec's "pipelines cached by shape, not numbers" holds in the strongest form: there is one
   pipeline). The loop's branches are the same for every pixel, so it costs little. Part 3 splits the stack into
   several passes at area steps; each pass runs a range of the same program.
2. **The fused pass is the composite.** With only point steps (all of part 1), the grade runs inside `fs_composite`:
   `(hdr + bloom) × exposure → clamp 0..65504 → grade program → clamp 0..65504 → roll_off → sRGB → dither`. The old
   `grade()` and its uniforms go. With no grade the shader takes exactly today's path (byte-identical pictures).
3. **Curves are baked in `bpe-gfx`, not the kit.** The frame's `GradeStack` carries each panel's resolved numbers
   and point lists; `bpe-gfx::grade` turns them into steps and lookup rows (`compile`). The picture tests in
   `bpe-gfx` can then build stacks from plain numbers. The compiled program is kept and re-uploaded only when the
   stack changed.
4. **Working spaces** (spec section 2): linear (the renderer's linear sRGB), tone (the sRGB transfer curve, which
   for x > 1 is the same formula continued: same slope and curvature at 1, monotonic, finite to the half-float limit
   and past it; negative values mirrored), colour (Oklab). The shader keeps track of the space it is in and converts
   only when a step needs another.
5. **Lookup rows**: 1024 samples per curve over `u = p / (p + 1)` (p the tone value; half the samples cover 0..1,
   the display range), sample i at `u = i / 1024`. Each sample stores the curve's *offset from the straight line*
   per channel (r, g, b), so an identity curve is exactly zero. Below 0 a curve continues straight with its slope at
   0 (stored in the step's header); past the last sample the offset is held.
6. **Tone maths** (all in tone space, each monotonic, composed in Lightroom's order Contrast → Highlights → Shadows
   → Whites → Blacks, baked into one row per Basic panel):
   - Contrast c = Contrast/100: a cubic Hermite through (0,0), (m,m), (1,1), m = tone(0.18) = 0.4614, slope at m
     `1 + 0.7c`, at 0 and 1 `1 − 0.45c`; straight on past 1 (and below 0) with the end slope.
   - Highlights h: identity below 0.5; above, x = p − 0.5: h < 0 → `0.5 + x / (1 + 1.3|h|x)` (a soft compressor
     that brings a linear 3.0 below white at −100); h > 0 → `0.5 + x(1 + 0.8h·x)` up to 1, straight on past 1.
   - Shadows s: below 0.6, `p + s·A·p(1 − p/0.6)²`, A = 1.7 lifting, 0.9 darkening.
   - Whites w: above 0.6, `0.6 + x(1 + 0.45w·r(x))`, x = p − 0.6, r a smoothstep over 0.6..1 then 1: a gain on the
     top end that carries on above white (−100 brings white to about 0.78 and glows down with it).
   - Blacks b: below 0.4, `p + b·K(1 − p/0.4)²`, K = 0.08 lifting, 0.10 crushing (values pushed below 0 are clamped
     at the end as crushed black).
   Unit tests check every function is monotonic at the slider ends and identity at 0.
7. **Parametric curve**: nodes at 0, the centre of each of the four regions (bounded by the three splits, kept in
   order with a gap of at least 0.02 inside 0.05..0.95) and 1; each region's node moves up or down by up to 0.45 of
   its region's width; a monotone cubic (Fritsch–Carlson PCHIP) through the nodes. Zero everywhere gives exactly
   the straight line.
8. **Point curves**: the panel's `Value::Curve` points (x 0..1 in order, ends at 0 and 1, `bend` ignored) through a
   PCHIP; past 1 straight on with the end slope, below 0 with the start slope. Lightroom applies parametric then
   point: each channel's row is `channel(rgb(parametric(p)))`.
9. **White balance** (Temp, Tint): Bradford adaptation from D65 to a target white. Temp ±100 moves the target along
   the Planckian locus (Kim et al. 2002 cubic spline) in mireds from D65's 153.8: +100 is +130 mired (about 3500 K,
   warmer), −100 is −100 mired (about 18,600 K, bluer). Tint ±100 moves it ±0.03 Duv off the locus (+ magenta, −
   green). The target is scaled to Y = 1, so a grey keeps its luminance. Exposure (×2^stops) folds into the same
   3×3 matrix.
10. **Vibrance and Saturation** in OkLCh: Saturation scales chroma by `1 + s` (−100 grey, +100 twice); Vibrance by
    `1 + v·w`, w = (1 − (C/0.25)²) clamped to 0..1 and lessened by half around skin hues (Oklab hue ≈ 55°), so
    dull colours move most. Vibrance runs before Saturation.
11. **Skipped at defaults.** A panel at its defaults adds no steps; a stack with no steps sets the composite's grade
    flag off. A disabled panel is not in the resolved card, so it adds nothing.
12. **The Whole picture** is the Grade card's base layer (`grade.whole`, section `Mask`): never moved, removed or
    hidden. The grade's on/off is a card setting `on` (the Whole picture row's eye and the Build-mode summary
    switch); off, the draw gives an empty stack.
13. **No Grade card until the first edit.** Opening Grade mode does not change the scene. The panel column shows the
    starting card (`starting_card`: Whole picture with Basic and Tone Curve, ids 0, 1, 2) from a template; the
    first edit calls `ensure_grade`, which builds exactly that card, so the addresses the column used are right.
14. **Panels in part 1**: Basic (no Texture, Clarity, Dehaze until part 3) and Tone Curve. Lightroom's order list
    (`PANEL_ORDER`) has only these two; later parts add theirs in place.
15. **Editor-only state** (never saved): the mode (`settings::EditorMode`, starts in Build), After / Before / Split
    and the split line, folded panels, the Tone Curve's channel chip. The Tone Curve's Parametric / Point *mode* is a
    saved setting (it says which editor the panel shows; both curves always apply).
16. **Split** is done by the composite: the stack carries an editor-only `split` (0..1 across the picture); pixels
    left of it skip the grade. Before clears the stack. Export never sets either.
17. **Histogram**: a compute pass over the finished 8-bit picture counts R, G, B and luminance into 4 × 256 bins
    (workgroup atomics, then the buffer), read back without waiting (`map_async`, polled next frame). Only the
    preview asks for it, only in Grade mode.
18. **Gradient map and vignette** leave with the old grade and come back in part 3 (Effects panel). The Graded sunset
    test scene moves onto the new card (Temp, Contrast, a keyed Vibrance) and loses its map and vignette until then.

## Files

### bpe-core
- `ids.rs`: `SlotId::Grade` appended last (`ALL` has 12), name "Grade", `in_card_list` false.
- `registry.rs`: `LayerSection::{Mask, MaskPart, Panel}` appended.
- `layers.rs`:
  - `add_layer_in`: `Mask` goes at the end of the card (after the base mask); `MaskPart` / `Panel` refused (they are
    sub-layers).
  - `check_sub`: a parent of section `Mask` takes subs of section `MaskPart` or `Panel` (its kind's `takes_sub`
    still asked); the old rule (Drawn/Around parents, Sub subs) is unchanged. A mask takes at most `MAX_MASK_SUBS`
    (40).
  - `add_sub_layer`: a `MaskPart` goes after the mask's last part (before its panels); anything else at the end of
    the block, as now.
  - `move_sub_layer` gains the registry and moves a sub-layer among its siblings **of the same section** (so a panel
    stays among panels and a part among parts). For Sub-only parents this is exactly today's behaviour. Callers
    (`inspector/edits.rs`, tests) pass `reg`.
  - `move_layer`: a `Mask` layer moves among the masks, never below the base (Whole picture).
  - `duplicate_layer` of a Panel uses the mask limit.
- Tests: Grade slot last and hidden; panels under the whole picture in order; a part goes before panels; moving a
  panel among panels; the base mask can't move, go or hide; a mask's block moves with its subs.

### bpe-gfx
- `frame/grade.rs` (new): `GradeStack { masks: Vec<GradeMask>, split: Option<f32> }`, `GradeMask { amount, panels:
  Vec<GradePanel> }`, `GradePanel::{Basic(Basic), ToneCurve(ToneCurve)}`, `Basic { temp, tint, exposure, contrast,
  highlights, shadows, whites, blacks, vibrance, saturation }`, `ToneCurve { highlights, lights, darks, shadows,
  splits: [f32; 3], rgb, red, green, blue: Vec<CurvePoint> }`, defaults, `GradeStack::is_identity()`. `Frame.grade:
  GradeStack` replaces `Grade` (`frame/effects.rs` loses `Grade`). `Frame::clear` resets it.
- `grade/` (new folder): `mod.rs` (map, constants: `CURVE_SAMPLES` 1024, op and space codes), `maths.rs` (tone
  encode/decode, Oklab, white balance matrix, tone functions, PCHIP, curve baking), `compile.rs` (`Program`: the
  stack as `Vec<[f32; 4]>`; `compile(&GradeStack) -> Program`; `Program::is_empty`), `scopes.rs` (`Histogram`:
  compute pipeline, bins, non-blocking readback), `tests.rs` (CPU maths), `pictures_tests.rs` (GPU, `test_device`).
- `shaders/grade.wgsl` (new): the program loop, spaces, ops (`OP_MATRIX`, `OP_CURVE`, `OP_CHROMA`), Oklab.
  `shaders/scopes.wgsl` (new): the histogram compute shader.
- `shaders/post.wgsl`: `grade()`, `grade_map` binding and the `grade`/`grade_more` globals go; `fs_composite` runs
  `run_grade` when `g.post.z` is 1 (after the input clamp, before `roll_off`); grade program at group 2.
- `renderer/mod.rs`: `Globals` loses `grade`, `grade_more` (and the four shaders that declare `Globals`: `post`,
  `scene`, `sprites`, `liquid`); `post.z` = 1 when the compiled program has steps.
- `renderer/grade.rs`: `GradeMap` becomes `GradeGpu` (layout, storage buffer grown as needed, bind group, the last
  compiled stack to skip work). `renderer/new.rs`: composite layout drops binding 3, pipeline layout adds group 2.
  `renderer/targets.rs`: no grade map. `renderer/scene.rs::finish`: sync the program, bind group 2.
- `Renderer::request_histogram` / `Renderer::histogram` (latest bins, if read).
- `grade_tests.rs` replaced by the new picture tests.

### bpe-kit
- `modules/grade/` (new): `mod.rs` (ids `GRADE`, `WHOLE`, `BASIC`, `TONE_CURVE`; `GradeCardKind` with setting `on`;
  `WholeKind` (base, `Mask`); `PanelKind` for each panel; `register`; `PANEL_ORDER`; `starting_card`,
  `ensure_grade`, `add_panel`, `reset_order`, `can_remove_panel`; `stack(resolved card, time) -> GradeStack`),
  `basic.rs` (`BasicPanel` settings), `tone_curve.rs` (`ToneCurvePanel` settings, `CurveMode`), `tests.rs`.
- `lib.rs::registry`: `modules::grade::register(&mut r)`.
- `things/scene/mod.rs`: the nine `grade_*` settings and `grade()` go; `slots()` gains `SlotId::Grade`.
- `things/scene/draw.rs`: `frame.grade = modules::grade::stack(...)` from `cx.resolved.slots[Grade]`.
- `scenes.rs::graded_sunset`: onto the new card.
- `tests/restarts.rs`: a second test, every grade setting changed one at a time leaves `fingerprint`,
  `sound_fingerprint` and `beside_fingerprint` unchanged.
- `tests/full_size_look.rs`: a `grading` scene (lit, glows above white, Basic with Highlights and Whites down, a
  Tone Curve with points and parametric moves); the Graded sunset stays.

### bpe-studio
- `settings.rs`: `EditorMode { Build, Grade }` (not saved to disk).
- `grade_mode/` (new): `mod.rs` (`GradeView` state: after/before/split, split x, folded, channel chips, dragging),
  `cx.rs` (`GradeEdit` and `apply`), `masks.rs` (the left list: Whole picture row with eye), `panels.rs` (the
  right column: headers, bodies, Add panel, right-click menu, drag to reorder), `curve.rs` (the tone curve graph),
  `scopes.rs` (the histogram strip), `tests.rs` (kittest).
- `app/mod.rs`: `Studio.mode`, `Studio.grade_view`; `ui` draws Grade mode's left and right panels instead of the
  tree and inspector.
- `app/panels.rs`: the Build | Grade switch in the toolbar; `left_panel` / `inspector_panel` branch on the mode.
- `app/shortcuts.rs`: D (no modifiers, not typing, nothing focused) toggles the mode; `\` toggles Before; Y Split.
- `preview.rs`: in Grade mode the corner switch is After / Before / Split, the picture is the Camera view, the
  frame's grade is cleared for Before or split for Split (in `compose`, so the warm-up matches), `Drawn` knows the
  view, a draggable split line, no thing handles; the Histogram strip under the picture.
- `timeline/mod.rs`: in Grade mode only Scene settings' Grade lanes.
- `inspector/own.rs`: Scene settings' Grade summary (on/off, panels changed, "Open in Grade mode").
- `inspector/tests.rs`: the old "picture card shows the grade" test goes; a summary test.
- `style/tokens.rs`: histogram channel colours (video colours, allowed: "the video owns colour").

### Docs
- `docs/notes/grading.md` (new area) with the as-built summary; one index line in `CLAUDE.md`.
- `docs/notes/file-map.md`: the grade files.

## Things not to do
- Don't clamp to 0..1 inside the stack; don't grade after the roll-off.
- Don't rebuild a pipeline when a number or the order changes.
- Don't let any grade key restart physics, sound or particles.
- Don't show the histogram in an export; don't save the mode, the view or the split.
- Don't add built-in grades or example scenes.
- Don't keep upgrade code for the old `grade_*` keys.

## Steps and checks

1. **Core** (ids, sections, layers) with unit tests. `cargo test -p bpe-core`.
2. **gfx maths + compile** with CPU tests: monotonic tone functions at extremes; identity at defaults (no steps);
   WB keeps grey's luminance and warms it; parametric and point identity; PCHIP passes through its points; Highlights
   −100 takes tone(3.0) below 1.
3. **gfx shader + renderer** with GPU tests (`test_device`): no grade byte-identical to the frame without one;
   Exposure +1 doubles linear light (within 1/255 after encode); Temp 0 / Tint 0 skipped; Temp + warms grey and
   keeps its luminance; near-identity panels within 1/255 of none; Highlights −100 brings a 3.0 background under
   white; swapping Basic (Saturation −100) and a red Tone Curve changes the result; split grades only the right;
   histogram counts a flat picture into one bin per channel.
4. **kit**: card, kinds, ensure/starting card, add panel names ("Tone Curve 2"), reset order, the draw reaches
   the frame (keys at a time, disabled panel skipped, order kept, `on` off empties it), restarts test, save/load
   round trip, the test scene.
5. **studio**: the mode, panels, curve, preview, histogram, timeline, summary; kittest: the switch and D, Before and
   Split, Add panel copy, panel order change by drag and by the menu, Reset panel, Remove refused on the last panel,
   double-click reset, the summary's button opens Grade mode.
6. **Whole checks**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
   --workspace -- --test-threads=2`; goldens unchanged (debug and release, no bless); full-size look check: every
   scene but graded-sunset and the new grading scene byte-identical to main (`cmp`), and the grading scene's full
   pictures and crops looked at (banding, edges, clipping); one Opus review; PR; CI green; merge.

## As built

Built as planned (summary in `docs/notes/grading.md`). Where it differs:

- `move_sub_layer` was kept as it was; the new `Scene::move_sub_layer_in(reg, …)` moves a sub-layer among its
  siblings of the same section (the grade uses it; the Look editor's callers are unchanged).
- The contract (`bpe-core/src/contract.rs`) allows a base layer of section `Mask` as well as `Drawn`.
- `EditorMode` and Grade mode's state (`GradeMode`) live in `bpe-studio/src/grade_mode/mod.rs`, not `settings.rs`
  (nothing of them is saved).
- The Histogram sits in Grade mode's left column under the masks, as in the approved mockup (layout B), not under
  the picture. The preview's corner switch is After / Before / Split.
- A panel's header has Move up and Move down in its right-click menu besides dragging.
- The full-size look check has two grading scenes, `grading-basic` (lit demo through a strong Basic) and
  `grading-curves` (colour demo through the Tone Curve).
- The Graded sunset test scene's `track` and `own` builder helpers went (nothing else used them).
