# Colour grading (Lightroom-style): design

Approved by the owner on 2026-11-09 (thread "Design Lightroom-style colour grading"), section by section. Built in
six parts, each its own thread, plan and PR: `docs/superpowers/plans/colour-grading/part1.md` … `part6.md`.

**When:** the owner asked for this to be built **after all the code review workstreams (W1–W17) are merged**. W17
splits the giant files, so each part's plan is written when its build starts, against the code as it is then. This
spec names areas and types, not line numbers.

## What the owner asked for and chose

- "A colour grading / post processing suite like Lightroom. I am proficient in Adobe Lightroom, so I would like
  colour grading features similar to it."
- **Q1, how close to Lightroom: A.** Lightroom Classic's names, its −100…+100 slider ranges (as it shows them for a
  JPEG) and its panel order, so the owner's habits carry over. **No** loading of Lightroom `.xmp` presets.
- **Q2, where a grade applies: A.** One grade for the whole video plus masks, as Lightroom works. Each mask has its
  own full set of panels. No per-thing grade cards and no Effects-style free stack of grades.
- **Layout: B, Grade mode** (mockups "Colour grading layouts", https://claude.ai/artifact/FoXoYeebDE9Xem9oGHhGHT,
  source copy in the project files `ballphysics/colour-grading/mockups/`): a Build | Grade switch at the top like
  Lightroom's Develop module.
- **Panel order is real (A).** Lightroom only reorders panels on screen; here the order on screen *is* the processing
  order. It starts in Lightroom's order, has Reset order, and a panel can be **added more than once** (a second Tone
  Curve at the end). Each mask keeps its own order.
- Design sections approved as written: 1 contents, 2 where the grade sits in the picture, 3 masks, 4 Grade mode,
  5 parts and checks. This spec is those sections made exact.

Breadth first and everything mixes with everything (standing rule): every grade number is an ordinary setting, so it
keyframes, Follows sound, light or health, copies and pastes, and undoes like any other.

## 1. What is in it

Panel names and order follow Lightroom Classic. Spelling follows the rest of the editor ("Colour"). Default order:

| # | Panel | Settings (range, default 0 unless said) |
| --- | --- | --- |
| 1 | **Basic** | *White balance:* Temp −100…100, Tint −100…100. *Tone:* Exposure −5…+5 stops, Contrast, Highlights, Shadows, Whites, Blacks (each −100…100). *Presence:* Texture, Clarity, Dehaze, Vibrance, Saturation (each −100…100) |
| 2 | **Tone Curve** | Mode Parametric / Point. *Parametric:* Highlights, Lights, Darks, Shadows (−100…100) and the three split points (Shadows/Darks 25, Darks/Lights 50, Lights/Highlights 75; 0…100, kept in order). *Point:* a curve for RGB, Red, Green, Blue (each `Value::Curve`, default the straight line). Both apply when both are set (Lightroom does the same) |
| 3 | **HSL / Colour** | Hue, Saturation, Luminance for Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta (24 numbers, −100…100). View chips Hue / Saturation / Luminance / All are editor only. **Point Colour**: up to 8 picked colours, each Hue shift, Saturation shift, Luminance shift (−100…100) and Range (0…100, default 50) |
| 4 | **Colour Grading** | Shadows, Midtones, Highlights, Global: each Hue 0…360°, Saturation 0…100, Luminance −100…100. Blending 0…100 (default 50), Balance −100…100 |
| 5 | **Detail** | Sharpening: Amount 0…150 (default 0), Radius 0.5…3.0 px (default 1.0), Detail 0…100 (default 25) |
| 6 | **Effects** | Post-crop Vignette: Amount −100…100, Midpoint 0…100 (50), Roundness −100…100, Feather 0…100 (50), Highlights 0…100. Grain: Amount 0…100, Size 0…100 (25), Roughness 0…100 (50). **Gradient map** (moved here from today's grade): Gradient (default black→white), Amount 0…100 (0) |
| 7 | **Calibration** | Shadows Tint −100…100. Red, Green, Blue Primary: Hue −100…100, Saturation −100…100 |

Left out on purpose: Noise Reduction, Lens Corrections, Transform (a rendered video has no lens or sensor noise; lens
looks are Effects layers already), Profiles, Auto, and `.xmp` import.

Extras because this is video:
- Every number keyframes and can Follow sound / light / health (`keyframe, pulse` on every number). Point curves,
  Point Colour picks and brush strokes are values that are never keyframed (like other curve and stroke values).
- **Grain moves**: a new grain pattern each video frame, the same pattern for the same frame in every render.
- **Snapshots** save named versions of the whole grade inside the scene; **Save grade… / Load grade…** write and read
  a grade file to reuse it in another project.
- The old basic grade goes away: Scene settings' Picture card loses Brightness, Contrast, Saturation, Warmth, Tint,
  Vignette, Vignette size, Gradient map and its Amount (`grade_*` keys). No upgrade code (no backward compatibility).
  The Picture card keeps Bloom and Exposure.

## 2. Where the grade sits in the picture

Today: `fs_composite` (post.wgsl) does `(hdr + bloom) × exposure → roll_off → sRGB encode → grade() → dither`. The grade
runs on clipped, display-ready values, so light above white is gone before it is graded.

New order: `(hdr + bloom) × exposure → **grade stack** → roll_off → sRGB encode → dither`.

- **Input** is the full-precision linear HDR picture after Effects, bloom and the Picture card's Exposure (the
  renderer's linear sRGB primaries, values above 1 kept). With bake (accumulated samples) the grade runs once on the
  average, as the effects, bloom and grade do today.
- **Working spaces.** Each panel step declares the space it works in, and the stack converts between them only when
  the space changes:
  - *Linear*: white balance (Temp/Tint as a gain in a cone-response space, Bradford-style, so a neutral grey stays
    neutral at 0 and stays neutral in luminance when moved), Exposure (×2^stops), Calibration (a 3×3 primaries
    matrix built from the six sliders, plus Shadows Tint weighted to the darks), Grain (added in linear, scaled by
    local brightness like film).
  - *Perceptual tone*: a brightness encoding equal to the sRGB transfer curve up to 1 and continued smoothly above 1
    (same slope and curvature at 1, monotonic, defined up to the half-float limit). Contrast, Highlights, Shadows,
    Whites, Blacks, the tone curves and Colour Grading's luminance work here. The curves' 0…1 range is the display
    range; above 1 a curve continues with its last segment's slope, so nothing above white is clipped by a curve.
    Highlights and Whites reach into the range above 1, so they can bring hot glows and lamps back into range.
  - *Perceptual colour*: Oklab / OkLCh for everything that changes hue or saturation (HSL / Colour, Point Colour,
    Vibrance, Saturation, Colour Grading's tints, Dehaze's colour part). Hue bands for HSL are eight overlapping
    smooth windows centred on Lightroom's band hues, so a colour between two bands is moved by both.
- **Order of steps.** The Whole picture's panels run in their list order. Then each mask, in list order, runs its own
  panels on the result so far, and the result is mixed back by the mask: `out = mix(prev, mask_panels(prev),
  amount × coverage)`. This is Lightroom's "masks sit on top of the main edit", with a real order.
- **Point steps and area steps.** Most steps change each pixel on its own (point steps). Texture, Clarity, Dehaze,
  Sharpening and the Vignette's Highlights need the pixels around them (area steps). The stack is compiled into
  passes: a run of point steps is one fused GPU pass; an area step ends a pass, builds the blur it needs from that
  pass's output (a downsample pyramid like bloom's, reused across area steps of the same pass), then continues. A
  grade with no area steps is one pass.
- **Video, not photos.** Grain is seeded by the frame number and pixel, never by time of day or random state.
  Preview and export run the same passes at the same resolution scale, so they match.
- **No grade, no change.** A scene without a Grade card, or whose Grade is all defaults with no masks, skips the
  stack completely; its frames are byte-identical to today's frames with the old grade at its defaults.
- Lighting, effects, bloom and the rest of the renderer are unchanged; the grade is draw-only and never touches
  physics, sound or particles.

## 3. Masks

Each mask is a list of **parts** combined in order with **Add / Subtract / Intersect** (Lightroom's mask components),
then its own panels. Kinds of part:

- **Linear Gradient**: two lines (start and end) dragged on the preview; Feather is the distance between them.
- **Radial Gradient**: centre, width, height, angle, Roundness −100…100, Feather 0…100; Invert.
- **Brush**: painted strokes with Size, Feather, Flow, Density, Erase, brush A and B settings (editor only, like
  Lightroom). Strokes are a `Value::Strokes` in the frame's own units (0…1 across, 0…1 down) for a screen mask, or
  world units for a world mask.
- **Colour Range**: up to 5 picked colours (eyedropper on the preview) and Refine 0…100. Coverage from the OkLab
  distance to the nearest pick.
- **Luminance Range**: low and high ends (0…100) and Smoothness 0…100, on perceptual brightness.
- **Things**: Only balls, Only shapes, Only text, Only particles, Only background, or one named thing (a `Target`).
  Coverage is exactly what those things draw (their silhouette, with soft edges), plus **Expand** (px) and
  **Feather** (px) to take in some of their glow. Background = everything no thing covers.

Every part has Invert. Every mask has: a name, an eye toggle (off = skipped, settings kept), **Amount** 0…100 (how
strong its panels are, default 100), and the red overlay (O). Its panels start as one Basic panel; "+ Add panel" adds
any panel, including a second copy.

Moving with the video:
- A Linear, Radial or Brush part can **follow a thing**: the thing itself, one ball of a group or emitter (by number),
  or **every ball** (one copy of the shape round each ball, combined with Add). Options: Turns with it, Grows with it
  (size scales with the ball's radius).
- By default a part is fixed on the **screen**. **Stick to the world** places it in the world, so it moves with the
  camera.
- Every number on a mask and its parts keyframes and can Follow sound (a radial glides across, its Amount pulses on
  the beat).

How coverage is made on the GPU:
- Gradients, ranges and brushes are worked out in the grade pass itself (strokes in a mask texture like
  `masks.rs`'s brush tiles).
- **Things** parts need a silhouette: the frame draws the chosen things a second time into an 8-bit coverage target
  (shape only: no lighting, effects or glow), once per distinct thing selection used by any visible mask, then
  Expand (dilate) and Feather (blur). A scene with no Things part draws no silhouettes.
- **Every ball** follows read the balls' screen positions and radii from a storage buffer (at most 4096 balls;
  further balls are left out and the mask row says so).
- A mask that points at a thing that no longer exists covers nothing and shows "Thing not found" on its row.

## 4. Grade mode in the editor

Follows mockup B.

- **Mode switch.** A Build | Grade switch in the middle of the toolbar, and the **D** key (free today; Ctrl+D stays
  Duplicate). The mode is editor state (`settings.rs`), never saved in the scene. In Build mode the Scene settings
  inspector shows a small **Grade** summary (on/off, number of masks, "Open in Grade mode").
- **Left panel.** The **masks** list: *Whole picture* on top (cannot be moved or deleted), then the masks in order
  (drag to reorder; tree-row style; kind icon, name, eye). Right-click: Rename, Duplicate, Invert, Delete. "+ Add
  mask" menu with the kinds. Under it: **Snapshots** (Save snapshot; click restores, with undo; hover shows it in the
  preview; right-click Rename / Update / Delete) and **Save grade… / Load grade…**.
- **Middle.** The preview with **After / Before (\\) / Split (Y)**; Split has a draggable line. Mask handles are drawn
  and dragged on the picture (gradient lines, radial ellipse and pin, brush cursor). Under the preview a scopes
  strip: **Histogram, Waveform, RGB Parade, Vectorscope** (one shown at a time, picked by chips), worked out on the
  GPU from the graded picture, preview only.
- **Right.** The selected mask's header (name, Amount) and its panels. Each panel: a collapse arrow, its name, an
  on/off switch (off = skipped, settings kept), and a drag handle; dragging changes the processing order. "+ Add
  panel" adds any panel (a copy is named "Tone Curve 2"). Right-click a panel: Reset panel, Remove (copies and any
  panel but the mask's last), Reset order (puts the panels in Lightroom's order, copies after their first).
- **Bottom.** The timeline, showing only grade lanes (Scene settings' Grade card) while in Grade mode.
- **Lightroom habits:** double-click a slider's label resets it; Alt-drag Whites or Blacks shows clipped pixels;
  **J** shows clipping on the picture (blue below black, red above white); **O** shows the mask overlay; click the
  tone curve to add a point, drag a point off the graph to remove it; drag a wheel's puck, Shift for fine moves;
  the **targeted adjustment tool** (a target button on Tone Curve and HSL / Colour): drag up or down on the picture to
  move the curve point or the HSL slider(s) for the colour under the mouse.
- Temp, Tint, the HSL sliders and Calibration's sliders draw their colour range in the slider track, as Lightroom
  does. This is the one place editor controls show a hue that is not a track colour: it shows the video's colours,
  which the design system allows ("the video owns colour"). Wheels are colour wheels for the same reason.

## 5. Data model

- A new card slot **`SlotId::Grade`**, only on Scene settings, appended last in `SlotId::ALL` so no slot moves. Not
  shown in the Build inspector's card list (like the Mixer), only through Grade mode. Card kind `grade` registered
  in the kit.
- The Grade card's **layers** hold the masks and panels, reusing layers and sub-layers:
  - Layer section **`Mask`** (new `LayerSection`): one layer per mask. The first is kind `grade.whole` (Whole
    picture), always present and first. Others are kind `grade.mask`.
  - Sub-layer section **`MaskPart`** (new): a mask's parts, sub-layers of their mask, kinds `grade.part.linear`,
    `.radial`, `.brush`, `.colour_range`, `.luminance_range`, `.things`. Parts sit straight after their mask.
  - Sub-layer section **`Panel`** (new): a mask's panels, sub-layers after its parts, kinds `grade.basic`,
    `.tone_curve`, `.hsl`, `.colour_grading`, `.detail`, `.effects`, `.calibration`. List order is processing order;
    `Layer.enabled` is the panel's on/off switch and the mask's eye.
  - Every setting is a layered key (`"<layer id>/<key>"`), so addresses, keyframes, pulses, copy/paste and undo work
    unchanged. Each panel and part kind is a `#[derive(Settings)]` struct.
- **Snapshots** are a `Value` on the Grade card (`snapshots`: name plus the card's layers and params), never
  keyframed. **Grade files** (`.grade`) hold the Grade card alone in the scene file's format and version; loading
  replaces the card (one undo step); Things parts naming things the scene lacks become "Thing not found".
- **Draw side**: the scene's draw turns the resolved Grade card into a `bpe_gfx::GradeStack` on the `Frame`
  (replacing `Frame.grade: Grade`): masks, parts, panels with resolved numbers, curves baked to lookup rows, follow
  positions resolved through the camera. `bpe-gfx` compiles it into passes (section 2) and caches pipelines by the
  stack's shape (kinds and order), not its numbers, so dragging a slider never rebuilds a shader.
- **Fingerprints**: no grade key restarts physics, sound or particles (`restarts.rs` covers it).

## 6. Parts

1. **The grade itself.** `SlotId::Grade`, the card, layer sections, the GPU stack and working spaces, **Basic**
   (all but Texture/Clarity/Dehaze), **Tone Curve** (both modes), real panel order, Add panel copies, Reset order.
   Grade mode's frame: switch and D, the masks list showing only Whole picture, the panel column, After / Before /
   Split, the Histogram, double-click reset. The old `grade_*` settings and `fs_composite`'s `grade()` removed;
   test scenes moved onto the new card.
2. **Colour.** HSL / Colour, Point Colour, Colour Grading with the wheel widget, Calibration.
3. **Detail and effects.** Area steps and pass splitting; Texture, Clarity, Dehaze, Sharpening, post-crop Vignette,
   moving Grain, Gradient map.
4. **Masks, first half.** Mask layers and parts, Linear, Radial, Colour Range, Luminance Range, Add / Subtract /
   Intersect, Invert, Amount, the overlay (O), handles on the preview, follow a thing (one thing, one ball, every
   ball), stick to the world.
5. **Masks, second half.** Things parts (silhouette pass, Expand, Feather) and Brush.
6. **Grading tools.** Waveform, RGB Parade, Vectorscope, J and Alt clipping, the targeted adjustment tool, Snapshots,
   Save grade / Load grade.

Each part: its own fresh thread, plan in `docs/superpowers/plans/colour-grading/`, one Opus review at the end, PR
merged by the thread when checks and review pass, notes added to `docs/notes/grading.md` (a new area: one index line
in CLAUDE.md in part 1). No exe build or install unless the owner asks.

## 7. Checks

- **Picture tests with known numbers** (`bpe-gfx`, `test_device()`): Exposure +1 doubles linear light; Temp 0 and
  Tint 0 leave a grey ramp grey and every test colour unchanged; Temp + keeps grey neutral in luminance and warms it;
  an identity curve and every panel at defaults change nothing (within 1/255 before dither); Highlights −100 brings a
  value of 3.0 below 1.0; swapping two panels changes the result where it should (Saturation −100 then Colour
  Grading vs the other way round); a mask with Amount 0 or eye off changes nothing; Add / Subtract / Intersect give
  the expected coverage; HSL Blue −100 saturation greys a blue patch and leaves a red one.
- **No grade, no change:** frames of the existing test scenes with no Grade card are byte-identical before and after
  part 1 (the old grade at defaults was skipped, so this holds).
- **Grain:** the same frame rendered twice is identical; two frames differ.
- **Restarts:** `restarts.rs` sees no grade key restart physics, sound or particles; goldens unchanged.
- **Editor tests** (kittest): mode switch and D; Before / Split; panel drag changes the stored order; Add panel copy;
  double-click reset; mask add, reorder, rename, delete; snapshots restore with undo.
- **Full-size look check:** a `grading` scene in `full_size_look.rs` (a lit scene with glows above white, a radial
  mask following a ball, grain on), looked at for banding in the curves and gradients, mask edges, grain look and
  clipping before each PR.

## Things not to do

- Don't promise or try to copy Adobe's exact maths; match the feel and the names.
- Don't grade after the roll-off any more, and don't clip to 0…1 anywhere inside the stack.
- Don't rebuild shaders when a number changes; only when the stack's shape changes.
- Don't add per-thing grade cards, `.xmp` import, LUT import, or a free Effects-style grade stack (not chosen).
- Don't let the grade restart physics, sound or particles, or show scopes in an export.
- Don't add built-in grades or example scenes to the program (standing rule); grading looks for the Looks tab are a
  separate request.
