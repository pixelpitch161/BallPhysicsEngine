# Colour grading

Lightroom-style colour grading of the finished picture, edited in the editor's **Grade mode**. Spec:
`docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved 2026-11-09, six parts). Plans:
`docs/superpowers/plans/colour-grading/`. Part 1 is built; parts 2 to 6 add panels, area steps, masks, the
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
