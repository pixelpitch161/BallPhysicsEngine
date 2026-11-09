# Colour grading

Lightroom-style colour grading of the finished picture, edited in the editor's **Grade mode**. Spec:
`docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved 2026-11-09, six parts). Plans:
`docs/superpowers/plans/colour-grading/`. Parts 1 and 2 are built; parts 3 to 6 add area steps, masks, the
remaining panels and scopes.

## Part 1 as built (the grade itself)

### Data (bpe-core, bpe-kit)

- **The Grade card** is `SlotId::Grade` on Scene settings (last in `SlotId::ALL`, not in the card list), kind
  `grade` (`bpe-kit/src/modules/grade/`). Its own setting is `on` (off skips the grade and keeps every setting).
- **Masks are its layers** (`LayerSection::Mask`). The Whole picture (`grade.whole`) is the base layer: first,
  never moved or removed. Masks come in part 4 and go after it (`add_layer_in`), moving among masks with their
  sub-layers (`move_layer`).
- **Panels are a mask's sub-layers** (`LayerSection::Panel`, kinds `grade.basic` and `grade.tone_curve`). A mask
  takes parts (`LayerSection::MaskPart`, part 4) before its panels and up to `MAX_MASK_SUBS` (40) sub-layers.
  Panels repeat ("Tone Curve 2"); their order is the processing order; `move_sub_layer_in` moves a sub-layer among
  its siblings of the same section. `PANEL_ORDER` is Lightroom's order (Reset order sorts by it, copies after
  their first, keeping their own order).
- A panel's setting is a layered key `"<panel id>/<key>"` on the card, keyframed and pulsed like any other. No grade
  key restarts the physics, sound or particles (`bpe-kit/tests/restarts.rs::grade_settings_restart_nothing`).
- **No Grade card until the first edit.** `starting_card(reg)` is the card `ensure_grade` makes (Whole picture 0,
  Basic 1, Tone Curve 2, all defaults); Grade mode shows it until an edit makes the real one, so its addresses
  hold.
- **Basic** (`basic.rs`): Temp, Tint (White balance), Exposure (−5..5 stops), Contrast, Highlights, Shadows,
  Whites, Blacks (Tone), Vibrance, Saturation (Presence), all −100..100. Texture, Clarity and Dehaze come with area
  steps (part 3). **Tone Curve** (`tone_curve.rs`): mode Parametric / Point (which editor shows; both curves
  apply, parametric first), Highlights/Lights/Darks/Shadows, three splits (25/50/75), point curves `rgb`, `red`,
  `green`, `blue` (`Value::Curve`, bend ignored).
- `grade::stack(resolved card)` turns the card at a moment into `bpe_gfx::GradeStack` (shown masks, shown panels,
  in order). The scene's draw puts it on `Frame.grade`.
- The old `grade_*` settings (brightness, contrast, saturation, warmth, tint, vignette, gradient map) and the old
  `grade()` in the composite are gone. Vignette and gradient map return in part 3's Effects panel. Graded sunset
  (test scene) uses the card (Temp, Contrast, a keyed Vibrance).

### GPU (bpe-gfx)

- `frame/grade.rs`: `GradeStack { masks, split }`, `GradeMask { amount, panels }`, `GradePanel::{Basic,
  ToneCurve}`; `is_identity` (no steps) leaves the composite's untouched path, byte-identical to no grade.
- `grade/compile.rs` compiles the stack into words for **one fixed interpreter shader** (`shaders/grade.wgsl`,
  storage buffer at group 2 of the composite): header `[steps, split, 0, 0]`, each step `[op, space, words,
  extra]` then data. Ops: `OP_MATRIX` (white balance and exposure as one 3×3), `OP_CURVE` (1024-sample rows over
  `u = p/(p+1)` of offsets from the straight line, per channel, plus the slope at 0 for values below), `OP_CHROMA`
  (vibrance, saturation in Oklab), `OP_BEGIN`/`OP_END` (a mask's amount). Panels at their defaults add no steps. A
  new order or value never builds a pipeline; `renderer/grade.rs` (`GradeGpu`) re-uploads only a changed stack.
- It runs inside `fs_composite`: `(hdr + bloom) × exposure → clamp → run_grade → clamp → roll_off → sRGB →
  dither`, on linear HDR light, before the roll-off, so glows above white can be brought back.
- **Working spaces**: linear, tone (the sRGB curve continued above 1, mirrored below 0) and Oklab; the shader
  converts only when a step needs another space.
- **Maths** (`grade/maths.rs`, CPU tests in `grade/tests.rs`): Contrast is a Hermite S about tone(0.18); Highlights
  a soft compressor above 0.5 (−100 takes linear 3.0 under white); Shadows below 0.6; Whites a gain above 0.6;
  Blacks below 0.4; all monotonic. Temp/Tint: Bradford from D65 to a white on the Planckian locus (Kim et al.), +100
  = +130 mired, −100 = −100 mired, tint ±0.03 Duv, the target at Y = 1 so a grey keeps its brightness. Vibrance
  weights dull colours and spares skin hues (Oklab hue 55°); Saturation scales chroma by `1 + s`. Parametric:
  PCHIP through region centres (±0.45 of the region's width); points: PCHIP through the points, straight on past
  the ends.
- **Split**: `GradeStack.split` (editor only): pixels left of it skip the grade.
- **Histogram** (`grade/scopes.rs`, `shaders/scopes.wgsl`): a compute pass counts the finished 8-bit picture
  (R, G, B, brightness × 256 bins, workgroup atomics), read back with `map_async` without waiting:
  `Renderer::request_histogram` then `Renderer::histogram` on a later frame. Only the editor asks.
- Picture tests (`grade/pictures_tests.rs`, `test_device`): defaults byte-identical, near-identity within 1/255,
  Exposure +1 doubles light, Temp warms a grey at its brightness, Tint magenta, Highlights −100 under white,
  Saturation −100 grey, order matters, split, mask amount half-way, histogram counts.

### Editor (bpe-studio, `grade_mode/`)

- `EditorMode { Build, Grade }` and `GradeMode` (compare view, split line, folded panels, curve channel, drags,
  histogram counts) live on `Studio` (`mode`, `grade`); none of it is saved.
- **Build | Grade** switch at the right of the toolbar, and **D** (no modifiers, nothing focused). In Grade mode
  **\\** toggles Before and **Y** Split. The status line says so.
- Grade mode replaces the side panels: the left column (`grade_mode::left`/`masks`) lists the Whole picture (its
  eye is the card's `on`, with how many panels are changed) and the **Histogram** (`scopes.rs`, square-root
  heights, brightness filled, channels as lines); the right column (`panels.rs`) shows the panels in order. Each
  header folds, has an on/off switch (the layer's `enabled`), drags to a new place (a line shows where it lands),
  and right-clicks for Reset panel, Move up/down, Remove panel (refused for the last) and Reset order. **Add
  panel** adds one at the end. Settings are the ordinary settings grid: double-click resets, right-click keys or
  follows.
- The Tone Curve's graph (`curve.rs`): Parametric shows the curve and its splits; Point shows the channel chosen
  by the RGB/Red/Green/Blue chips: a click adds a point, a drag moves it (ends move only up and down), a
  double-click takes it away.
- The **preview** in Grade mode is always the Camera view, without handles; its corner switch is After / Before /
  Split. `preview::compose` clears the grade for Before or sets `split` for Split (so the effects' warm-up matches);
  `Drawn` includes the view, and Split's white line drags. The Histogram is asked for after each redraw.
- The **timeline** in Grade mode shows only Scene settings' grade lanes ("Only grade keyframes show in Grade mode").
- Scene settings' inspector has a **Grade** card: on/off, which panels are changed, and Open in Grade mode.
- Edits go through `grade_mode::cx::apply` (`GradeEdit`), which gives the scene its card first; undo works as for any
  edit (the frame's diff).

### Checks

- Full-size look scenes `grading-basic` (lit demo, strong Basic) and `grading-curves` (colour demo through the Tone
  Curve). Every other scene renders byte-identical to before.

## Part 2 as built (Colour)

Plan: `docs/superpowers/plans/colour-grading/part2.md` (every maths choice is stated there).

### Panels (bpe-kit `modules/grade/`)

- `PANEL_ORDER` is Basic, Tone Curve, **HSL / Colour** (`grade.hsl`), **Point Colour** (`grade.point_colour`),
  **Colour Grading** (`grade.colour_grading`), **Calibration** (`grade.calibration`); part 3 puts Detail and
  Effects before Calibration. The starting card has one of each (Basic 1, Tone Curve 2, ...).
- Point Colour is its own panel (Lightroom shows it as a tab of HSL / Colour), so it has its own place in the
  order. Its eight picks are fixed slots `pick1_` .. `pick8_` (`PointPick`, a `paint`-prefixed setting group
  like the equalizer's bands): `on`, `colour` (never keyed), `hue`, `saturation`, `luminance`, `range` (50).
- `hsl.rs`: `<band>_hue|saturation|luminance` for red .. magenta (groups Hue, Saturation, Luminance; labels are the
  band names). `colour_grading.rs`: `<region>_hue` (0..360°), `_saturation` (0..100), `_luminance` for shadows,
  midtones, highlights, global, then `blending` (50) and `balance`. `calibration.rs`: `shadows_tint` and each
  primary's `_hue` and `_saturation`. Every number keyframes and pulses.

### GPU (bpe-gfx)

- `frame/grade.rs`: `GradePanel::{Hsl, PointColour, ColourGrading, Calibration}` with `GradeHsl`,
  `GradePointColour`/`GradePick` (colour in linear light), `GradeColourGrading`/`GradeWheel`, `GradeCalibration`;
  at their defaults they add no steps.
- New ops in the same interpreter (no new pipeline): `OP_HUE` (Oklab: a 360-sample row over the Oklab hue of
  `[turn, saturation, luminance, 0]`), `OP_POINT` (Oklab: `extra` picks, three words each: the pick in Oklab, its
  shifts, one over its widths), `OP_WHEELS` (Oklab: `[pivot, overlap, global log2 gain, 0]` then `[Δa, Δb, ΔL, 0]`
  per region), `OP_DARK_TINT` (linear: the gain on the darks). Calibration's primaries are an `OP_MATRIX`.
- Maths (`grade/maths.rs`, CPU twins of the shader, tested in `grade/tests.rs`):
  - HSL bands centre on the Oklab hues of Lightroom's HSV band hues (29°, 53°, 110°, 143°, 195°, 264°, 294°,
    328°); neighbours share a hue by a smoothstep that leaves each band the hues within 15% of the way to the next
    (`BAND_FLAT`), so the weights add to 1. Hue ±100 turns 0.75 of the way to the neighbour; Saturation scales
    chroma by `1 + s`; Luminance scales L by `2^(0.8·l·min(C/0.08, 1))` (greys never move).
  - Point Colour weight: `1 − smoothstep(0.35, 1, d)` with d the lightness, chroma and hue (`ΔH = 2√(C·Cp)
    sin(Δh/2)`) distances over widths that grow with Range; weights from the panel's input, shifts applied in
    slot order (hue ±40°). A colour brighter than white is compared at white's lightness (`tamed`), since
    the eyedropper picks the finished 8-bit picture.
  - Colour Grading: regions by Oklab lightness around a pivot (18% grey's L, moved by Balance), overlap from
    Blending; tints are Oklab (a, b) offsets in the direction of the wheel hue's colour (0 red, 120 green, 240
    blue), times `min(L/0.25, 1)` so black stays black; region luminance adds to L, Global's scales it.
  - Calibration: each primary's hue (±30°) and chroma (×(1 + 0.6s)) moved in Oklab, the columns scaled so white
    maps to white (greys never move). Shadows Tint is a luminance-keeping magenta/green gain faded out by 18%
    luminance.
- Picture tests (`grade/pictures_tests.rs`): HSL Blue −100 saturation greys a blue patch and keeps a red one; the
  GPU matches the maths on seven colours for every colour panel (within 2/255); defaults byte-identical; order
  matters (Calibration then HSL differs from HSL then Calibration).

### Editor (bpe-studio)

- `grade_mode/colour.rs`: the bodies. HSL / Colour has Lightroom's tabs Hue / Saturation / Luminance / All
  (`GradeMode::hsl_view`, not saved). Point Colour: the used picks as swatches (click to show one), **Pick** (the
  preview becomes the eyedropper, `eyedropper.rs`; the picked colour fills the first free slot), **Remove**
  (`GradeEdit::RemovePick` clears that slot's values, keys and pulses); then the selected pick's sliders.
  Colour Grading: chips 3-way / Shadows / Midtones / Highlights / Global (`GradeMode::wheel_view`); 3-way shows
  three wheels and their luminance sliders, one region a big wheel with its Hue, Saturation and Luminance; then
  Blending and Balance. Calibration: its sliders.
- `grade_mode/wheel.rs`: the colour wheel (a mesh in video colours; angle = hue anticlockwise from red at the
  right, distance = saturation). A drag puts the puck under the pointer; with Shift it moves a tenth as far; a
  click jumps; a double-click resets. kittest name `"<Region> wheel"` (role Slider).
- Coloured rails: `GridCx.rails` (`widgets/mod.rs`) gives a number setting a colour strip under its rail
  (`setting.rs::colour_strip`, drawn into a shape placed before the row so it sits under the knob). Basic's Temp
  and Tint, HSL, Point Colour, Colour Grading and Calibration use it; every other grid passes `None`. Colours come
  from `style::video` / `style::video_bytes` (the video owns colour).
- The preview's eyedropper works in Grade mode too (it picks the picture as shown, so Before picks the ungraded
  colour).
- Timeline lanes name a setting's group when its label appears more than once in its layer ("HSL / Colour ·
  Saturation · Blue"); unique labels are unchanged ("Basic · Vibrance").

### Checks

- Full-size look scene `grading-colour` (the colour demo through all four colour panels). Every other scene renders
  byte-identical to before.
- `restarts.rs::grade_settings_restart_nothing` checks well over 90 grade settings.
