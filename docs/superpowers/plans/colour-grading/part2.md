# Colour grading part 2: Colour — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add Lightroom's colour panels to the grade: **HSL / Colour**, **Point Colour**, **Colour Grading** (with the
colour wheel widget) and **Calibration**, each an ordinary panel (a sub-layer of the Whole picture) that runs on the
GPU in its place in the panel order, keyframes and follows sound like every other setting, and is edited in Grade
mode with Lightroom's views.

**Spec:** `docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved by Aedan 2026-11-09), part 2 of 6
(sections 1, 2, 4 and 6). **Part 1:** `part1.md` here and `docs/notes/grading.md` ("Part 1 as built"), handover in the
project files `ballphysics/colour-grading/part1-handover.md`. Mockups: project files
`ballphysics/colour-grading/mockups/GradeMode.dc.html` (the Colour Grading panel: view chips 3-way / Shadows /
Midtones / Highlights / Global, three wheels in a row with "hue° · saturation" under each, then Blending and
Balance).

**Tech:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37.

## Decisions made here (the spec leaves them to the plan)

1. **Point Colour is its own panel kind** (`grade.point_colour`, "Point Colour"), placed straight after HSL / Colour
   in `PANEL_ORDER`. Lightroom Classic shows it as a tab of the HSL / Colour panel; here a panel is a processing step,
   so it gets its own place in the order (it can be dragged, switched off, added twice) and the HSL panel stays the
   24 sliders. The spec's table lists it in the HSL / Colour row and its data-model list leaves it out; the part 1
   handover lists it as its own place. This plan follows the handover.
2. **`PANEL_ORDER`** becomes Basic, Tone Curve, HSL / Colour, Point Colour, Colour Grading, Calibration. Part 3 puts
   Detail and Effects between Colour Grading and Calibration. The starting card (and `ensure_grade`) therefore has
   six panels; Basic and Tone Curve keep ids 1 and 2.
3. **HSL / Colour** (`hsl.rs`, `HslPanel`): 24 numbers `<band>_hue`, `<band>_saturation`, `<band>_luminance` for
   bands red, orange, yellow, green, aqua, blue, purple, magenta, each −100..100, `keyframe, pulse`. Groups "Hue",
   "Saturation", "Luminance"; labels are the band names (Lightroom's rows).
   - **Bands in OkLCh.** A band's centre is the Oklab hue of the sRGB colour at Lightroom's band hue (HSV hue 0, 30,
     60, 120, 180, 240, 270, 300 at full saturation and value): about 29.2°, 52.8°, 109.8°, 142.5°, 194.8°, 264.1°,
     293.8°, 328.4°. Worked out once from `maths::oklab`, not typed in.
   - **Windows.** Between two neighbouring centres the two bands share the colour by a smoothstep, so the eight
     weights always add to 1 and a hue between bands is moved by both (the spec's "eight overlapping smooth
     windows").
   - **Hue** ±100 turns the hue towards the next band (+, Lightroom: Red + goes orange) or the one before (−) by up
     to 0.75 of the distance to that neighbour.
   - **Saturation** scales chroma by `1 + s` (−100 grey, +100 twice).
   - **Luminance** scales Oklab L by `2^(0.8 · l · k)`, `k = clamp(C / 0.08, 0, 1)`, so greys (no hue) are never
     brightened or darkened and dull colours only a little.
   - **Baking.** The panel bakes into one lookup row of `HUE_SAMPLES` = 360 samples over the Oklab hue (sample i at
     i°), each `[hue turn (radians), saturation s, luminance l, 0]`; the shader interpolates and wraps. New op
     `OP_HUE` in Oklab. A pixel with chroma under 1e-7 is left alone (no hue to read).
4. **Point Colour** (`point_colour.rs`, `PointColourPanel`): eight picks `pick1_` .. `pick8_`, each a setting group
   (`PointPick`, embedded with `#[setting(paint, prefix = ...)]` like the equalizer's bands) of `on` (bool, "Used"),
   `colour` (`Color`, the picked colour; never keyframed, as the spec says of picks), `hue`, `saturation`,
   `luminance` (−100..100, keyframe, pulse) and `range` (0..100, default 50, keyframe, pulse). Groups "Colour 1" ..
   "Colour 8". Fixed slots instead of a list so every number keeps a fixed address (keys, pulses, copy/paste,
   undo); removing a pick turns its slot off and resets it; a new pick takes the first free slot.
   - **Distance** from a pixel to a pick, in OkLCh with the pick at (Lp, Cp, hp): `d² = (ΔL/wL)² + (ΔC/wC)² +
     (ΔH/wH)²` where `ΔH = 2·√(C·Cp)·sin(Δh/2)` (the usual hue difference, so a grey pick is matched by lightness and
     chroma, not hue). Widths grow with Range r = range/100: `wL = 0.08 + 0.32r`, `wC = 0.04 + 0.16r`,
     `wH = 0.03 + 0.14r`. Weight `w = 1 − smoothstep(0.35, 1, d)`.
   - **Shifts**: Hue ±100 turns the hue by up to ±40°; Saturation scales chroma by `1 + s·w`; Luminance scales L by
     `2^(0.8·l·w)`; the hue turn is `w·h·40°`.
   - All weights are worked out from the panel's input colour, then each used pick's change is applied in slot order.
   - New op `OP_POINT` in Oklab; `extra` = the number of picks; three words per pick: `[L, a, b, 0]`, `[hue turn,
     s, l, 0]`, `[1/wL, 1/wC, 1/wH, 0]`. A pick that is off, or whose three shifts are 0, is left out; a panel with
     none adds no step.
   - The picked colour is a scene colour (sRGB bytes), turned into linear light for the GPU.
5. **Colour Grading** (`colour_grading.rs`, `ColourGradingPanel`): for each of shadows, midtones, highlights, global:
   `<region>_hue` (0..360 °, default 0), `<region>_saturation` (0..100), `<region>_luminance` (−100..100); then
   `blending` (0..100, default 50) and `balance` (−100..100). All `keyframe, pulse`. Groups "Shadows", "Midtones",
   "Highlights", "Global", "Blend". Labels "Hue", "Saturation", "Luminance", "Blending", "Balance".
   - **Hue** is Lightroom's wheel hue (0 red, 120 green, 240 blue): the tint's direction is the Oklab (a, b) direction
     of the sRGB colour at that HSV hue, so the wheel's colours and the tint agree.
   - **Tint**: `Δab = dir · (sat/100) · 0.10 · w_region · min(L / 0.25, 1)` (black stays black).
   - **Region weights** on Oklab lightness `x0 = clamp(L, 0, 1)`: a pivot `p = clamp(L_grey − 0.3·balance/100, 0.15,
     0.85)`, `L_grey` = Oklab L of 18% grey (0.5646); `x = x0 < p ? 0.5·x0/p : 0.5 + 0.5·(x0 − p)/(1 − p)`;
     `h = 0.02 + 0.313·blending/100`; `w_s = 1 − smoothstep(1/3 − h, 1/3 + h, x)`, `w_h = smoothstep(2/3 − h, 2/3 + h,
     x)`, `w_m = max(1 − w_s − w_h, 0)`. Balance + gives the highlights more of the range (Lightroom's sense); Blending
     0 is a near-hard split, 100 the widest overlap.
   - **Luminance**: shadows, midtones and highlights add `0.2 · lum/100 · w_region` to L; Global scales L by
     `2^(0.5·lum/100)`; L is kept ≥ 0 (never clamped above).
   - Global's tint has weight 1.
   - New op `OP_WHEELS` in Oklab; five words: `[p, h, global log2 gain, 0]`, then `[Δa, Δb, ΔL, 0]` per region
     (shadows, midtones, highlights, global; the tint before its L factor). Identity (every saturation and luminance
     0) adds no step; Blending and Balance alone change nothing.
6. **Calibration** (`calibration.rs`, `CalibrationPanel`): `shadows_tint`, `red_hue`, `red_saturation`, `green_hue`,
   `green_saturation`, `blue_hue`, `blue_saturation`, each −100..100, `keyframe, pulse`. Groups "Shadows", "Red
   primary", "Green primary", "Blue primary". Labels "Tint", "Hue", "Saturation".
   - **Primaries matrix** (linear, reuses `OP_MATRIX`): each primary (the linear sRGB unit colours) goes to Oklab, its
     hue turns by `hue/100 · 30°` (+ towards the next primary's side, as Lightroom: Red + goes orange) and its chroma
     scales by `1 + 0.6·sat/100`; back to linear, these are the matrix's columns P. Columns are then scaled by
     k = P⁻¹·(1, 1, 1), so white (and every grey) stays exactly as it was.
   - **Shadows Tint** (new op `OP_DARK_TINT`, linear, one word `[g.r, g.g, g.b, 0]`): `raw = (1 + 0.25t, 1 − 0.35t,
     1 + 0.25t)` (t = tint/100, + magenta, − green) divided by its luminance (Rec. 709 weights), so the tint keeps
     brightness; `c' = c · mix(1, g, w)`, `w = 1 − smoothstep(0, 0.18, Y)` with Y the pixel's luminance. Black
     stays black.
   - Matrix first, then the shadow tint.
7. **Coloured tracks** (spec section 4: HSL and Calibration sliders draw their colour range, the video owns colour).
   `GridCx` gains `rails: Option<&dyn Fn(&str) -> Option<Vec<Color32>>>`: colours drawn as a thin strip under a
   number slider's rail (a shape reserved before the row is drawn, so it sits under the knob). HSL: hue rows go from
   the band turned −100 to +100, saturation rows from grey to twice the band, luminance rows from dark to light.
   Calibration and Colour Grading hue/saturation rows likewise; Basic's Temp (blue → yellow) and Tint (green →
   magenta) too, since the hook is there. Every other grid passes `rails: None`. Colours are made in `style/` (a new
   `style::video` helper from linear light, marked `// scene colour`).
8. **The colour wheel** (`grade_mode/wheel.rs`): a disc whose angle is the hue (0 red at the right, counter-clockwise
   through yellow, green, … as Lightroom's wheel turns) and whose distance from the centre is the saturation (0 at
   the centre, 100 at the rim), painted as a mesh in video colours (white in the middle fading out to full colour);
   a puck at the setting. Dragging the puck (or clicking the disc) sets hue and saturation together (two edits,
   keyed at the playhead when keyed). **Shift** drags finely (a tenth). A double-click puts both back to 0. The
   wheel is named for kittest (`"<Region> wheel"`, role Slider). Under it the "hue° · saturation" numbers.
9. **Editor-only views** (never saved, in `GradeMode`): the HSL panel's chips **Hue / Saturation / Luminance / All**
   (default Hue, as Lightroom); Colour Grading's chips **3-way / Shadows / Midtones / Highlights / Global** (default
   3-way). 3-way shows the shadows, midtones and highlights wheels in a row with their luminance sliders under, then
   Blending and Balance; one region shows a big wheel and its Hue, Saturation and Luminance sliders, then Blending and
   Balance.
10. **Point Colour's editor**: a row of eight swatch slots (used ones filled with their colour; the selected one
    ringed), a **Pick** button that turns the preview into the eyedropper (the existing `eyedropper.rs`: the click
    picks the colour shown, so Before (\\) picks the ungraded colour) and fills the first free slot, selecting it;
    **Remove** turns the selected pick off and resets its settings. The selected pick's settings show below (its
    colour with the ordinary colour picker, Hue, Saturation, Luminance, Range). With no picks a line says how to
    start. The preview lets the eyedropper work in Grade mode.
11. **Timeline lane names**: a layered setting whose label is shared by another setting of its layer (the HSL's
    "Red" ×3, Colour Grading's "Hue" ×4, Point Colour's ×8, Calibration's "Hue" ×3) names its group too: "HSL /
    Colour · Saturation · Blue". Unique labels stay as they are ("Basic · Vibrance").
12. **No pipeline changes.** Every new op is a branch in the same interpreter shader (`shaders/grade.wgsl`); the
    program buffer grows as it already does. A panel at its defaults adds no step.

## Files

### bpe-gfx
- `frame/grade.rs`: `GradePanel::{Hsl, PointColour, ColourGrading, Calibration}`; structs `GradeHsl { hue, saturation,
  luminance: [f32; 8] }`, `GradePointColour { picks: Vec<GradePick> }`, `GradePick { colour: [f32; 3] (linear),
  hue, saturation, luminance, range }`, `GradeColourGrading { shadows, midtones, highlights, global: GradeWheel,
  blending, balance }`, `GradeWheel { hue, saturation, luminance }`, `GradeCalibration { shadows_tint, red_hue,
  red_saturation, green_hue, green_saturation, blue_hue, blue_saturation }`, each with `is_identity`. Exported from
  `lib.rs`.
- `grade/mod.rs`: `OP_HUE` 6, `OP_POINT` 7, `OP_WHEELS` 8, `OP_DARK_TINT` 9, `HUE_SAMPLES` 360.
- `grade/maths.rs`: `band_hues()`, `hsl_row(&GradeHsl) -> Vec<[f64; 4]>` and `hsl(lab, row)` (CPU version of the
  shader), `PickGpu` / `point_picks`, `point_colour(lab, picks)`, `wheels(&GradeColourGrading) -> WheelsGpu`,
  `colour_wheels(lab, w)`, `hsv_hue_direction(deg)`, `calibration_matrix`, `dark_tint_gain`, `dark_tint(c, g)`.
- `grade/compile.rs`: the four panels as steps.
- `shaders/grade.wgsl`: the four ops.
- `grade/tests.rs`: CPU tests (below). `grade/pictures_tests.rs`: GPU tests (below).

### bpe-kit
- `modules/grade/hsl.rs`, `point_colour.rs`, `colour_grading.rs`, `calibration.rs` (settings + `gpu()`).
- `modules/grade/mod.rs`: ids `HSL`, `POINT_COLOUR`, `COLOUR_GRADING`, `CALIBRATION`; `PANEL_ORDER` of six;
  `panel_kinds()` of six; `stack` arms; `free_pick(card params, panel)`, helpers for Point Colour editing.
- `modules/grade/tests.rs`: six starting panels; each new panel reaches the stack.
- `tests/restarts.rs`: the minimum raised (the grade now has well over 100 settings).
- `tests/full_size_look.rs`: `grading-colour` (the colour demo through HSL, a Point Colour pick, Colour Grading's
  split tone and Calibration).

### bpe-studio
- `widgets/mod.rs` (`GridCx.rails`), `widgets/setting.rs` (the strip under the rail), every `GridCx { .. }` gains
  `rails: None` except the grade's.
- `style/` : `video(lin rgb) -> Color32` and `video_srgb`; nothing else.
- `grade_mode/panels.rs`: bodies for the new kinds; `grade_mode/colour.rs` (new: HSL chips, Point Colour swatches,
  Colour Grading views, the rails); `grade_mode/wheel.rs` (new: the wheel widget); `grade_mode/mod.rs`: the view
  state; `grade_mode/cx.rs`: `GradeEdit::AddPick(panel, Color)` and `RemovePick(panel, slot)`.
- `preview.rs`: in Grade mode the eyedropper picks (before the "only looks" return).
- `timeline/lanes.rs`: group in a lane's name when its label is shared in its layer.
- `grade_mode/tests.rs`: kittest for the new panels.

### Docs
- `docs/notes/grading.md`: "Part 2 as built". `docs/notes/file-map.md`: the new files. This plan's "As built".

## Things not to do

- Don't build a pipeline per panel or value; don't clamp to 0..1 inside the stack (L ≥ 0 only).
- Don't save the view chips or the selected pick.
- Don't keyframe the picked colours (they're values like curve points).
- Don't let any grade key restart physics, sound or particles.
- Don't change any picture without a Grade card (every other look scene byte-identical).
- Don't add built-in grades or example scenes.

## Steps and checks

1. **gfx data + maths + compile + shader**, CPU tests first (TDD):
   - band weights add to 1 at every hue; a hue on a band's centre is that band's alone;
   - HSL at defaults bakes to zeros and adds no step; Blue −100 saturation greys pure blue (C → 0) and leaves pure red
     unchanged (within 1e-6); Red hue + turns red's hue up (towards orange); a grey is never moved by any HSL slider;
   - Point Colour: a pick at red with Saturation −100 greys red and leaves blue; Range 0 is narrower than 100 (a
     colour 20° away is moved less); an off pick changes nothing;
   - Colour Grading: weights add to 1 and favour shadows at L 0.1 and highlights at L 0.95; a shadows tint warms a
     dark grey and leaves a white nearly alone; balance + gives a midtone more highlight weight; black stays black;
   - Calibration: any slider keeps white and greys exactly (matrix · 1 = 1); Red hue + turns pure red's hue up;
     Red saturation − lessens red's chroma; Shadows Tint + makes a dark grey magenta (G lowest) at its luminance,
     leaves white alone.
   - GPU (`test_device`): HSL Blue −100 saturation greys a blue patch and leaves a red one (the spec's check); GPU
     matches CPU on a colour ramp within 2/255 for each new panel; each new panel at defaults is byte-identical to no
     grade; order matters (Calibration then HSL vs HSL then Calibration differ).
   `cargo test -p bpe-gfx --lib grade -- --test-threads=2`.
2. **kit**: settings, kinds, starting card of six, stack arms, picks; restarts; `cargo test -p bpe-kit`.
3. **studio**: rails hook, wheel widget, views, Point Colour editor, eyedropper in Grade mode, lane names; kittest:
   the HSL chips change the rows shown (Hue shows "Red" sliders, Saturation too, All shows 24); a wheel drag sets hue
   and saturation and makes the card; Shift moves less; double-click resets; 3-way shows three wheels and Global one;
   Point Colour: Pick then a delivered colour fills slot 1 and shows its sliders, Remove clears it.
4. **Whole checks**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace
   -- --test-threads=2` (background); goldens unchanged in debug and release (no bless); full-size look check: every
   scene but the new `grading-colour` byte-identical to main (`cmp`), and `grading-colour`'s pictures and crops
   looked at (banding in hue transitions, odd hue flips near greys, clipping); one Opus review; PR; CI green; merge.
5. Handover note for part 3 in the project files (`ballphysics/colour-grading/part2-handover.md`).
