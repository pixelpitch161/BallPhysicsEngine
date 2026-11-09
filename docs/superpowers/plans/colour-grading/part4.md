# Colour grading part 4: Masks, first half — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give the grade **masks**: masks after the Whole picture, each a list of **parts** (Linear Gradient,
Radial Gradient, Colour Range, Luminance Range) combined with **Add / Subtract / Intersect**, each part with
**Invert**, each mask with **Amount**, an eye, **Invert**, its own panels and the red **overlay (O)**. Gradients
are placed on the screen, **in the world**, or **follow a thing, one ball or every ball** (Turns with it, Grows
with it). Linear and Radial parts have **handles on the preview**. Every number keyframes and follows sound.

**Spec:** `docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved by Aedan 2026-11-09), part 4 of 6
(sections 3, 4, 5, 6 and 7). **Before:** `part1.md`..`part3.md` here, `docs/notes/grading.md`, handover
`ballphysics/colour-grading/part3-handover.md` in the project files.

**Tech:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37.

## Decisions made here (the spec leaves them to the plan)

1. **Kinds** (bpe-kit `modules/grade/masks.rs`): mask `grade.mask` (`LayerSection::Mask`, repeats, named "Mask",
   "Mask 2", ...); parts `grade.part.linear`, `grade.part.radial`, `grade.part.colour_range`,
   `grade.part.luminance_range` (`LayerSection::MaskPart`, repeat). A mask takes parts and panels; the Whole picture
   still takes only panels. Things and Brush parts are part 5.
2. **Mask settings** (`MaskSettings`): `amount` 0..100 (100, keyframe, pulse) "Amount", `invert` (bool) "Invert".
   The eye is `Layer.enabled` (core already skips a hidden layer and so its parts and panels).
3. **A new mask** (+ Add mask, a kind) is a `grade.mask` with one part of that kind and one **Basic** panel, put
   last (masks work in list order). The editor selects it.
4. **Parts' common settings**: `combine` (`Combine` Add / Subtract / Intersect, "Combine", group "Part"), `invert`
   (bool). The **first** shown part's Combine is ignored (it always counts as Add) and the editor hides it. A mask
   with no shown part covers nothing.
5. **Combining** (all coverage is 0..1): part `v` (with Invert, `1 − v`); first: `cov = v`; Add `max(cov, v)`;
   Subtract `cov·(1 − v)`; Intersect `cov·v`; the mask's Invert `1 − cov`; the mask mixes
   `mix(prev, panels(prev), amount/100 · cov)` in linear light (the existing `OP_BEGIN` / `OP_END`).
6. **Where a gradient is** (`place`, `Placed`): *On the screen* (default), *In the world* (moves with the camera:
   the spec's Stick to the world), *On a thing*, *On one ball*, *On every ball*. One choice instead of a Stick to
   world switch plus a follow switch, because following always moves with the world. With a thing or ball the
   gradient's X / Y are an **offset from it** in world units.
   - `target` (`Target`, default every Balls group) when following; `ball` (u32, 1.., `Ball number`) on one ball,
     looked up with `Bodies::row_of(number − 1)` as particles do; `turns`, `grows` (bools) when following.
   - **On a thing**: the middle of what the target names (the camera's `follow_point`, so a group is its balls'
     middle); Turns with it reads the first named thing's pose angle; Grows with it reads its balls' mean radius.
   - **A ball's turn** is its direction of travel (`atan2(v)`; 0 when slower than 1 px/s); balls carry no angle.
   - **Grows with it**: sizes and offsets scale by `radius / 24` (24 px is a new group's radius), so they are as set
     for a ball that size.
   - **Every ball**: one copy per live ball of the named things (emitters' too when the target is Balls, as the
     camera does), at most `MAX_BALLS` = 4096; the rest are left out and the mask row says so.
   - A target that names nothing, or a ball number not there, covers nothing; the mask row says "Thing not found".
7. **A place** (`GradePlace { at, turn, scale }`, screen units: video pixels from the picture's middle, y down, as
   `CameraView::to_screen`): a part's centre is `at + rotate((x, y)·scale, turn)`, its sizes `·scale`, its angle
   `+ turn`. On the screen `{0, 0, 1}`; in the world `{to_screen(0), −camera turn, zoom}`; on a ball
   `{to_screen(p), −camera turn + ball turn, zoom·grow}`. The kit works places out at the scene's draw
   (`World.camera`), so the GPU only ever sees the screen.
8. **Linear Gradient** (`LinearPart`): `x` 0, `y` −400 (px, span ±1500), `angle` 90 (°, −360..360: the way the
   effect fades out, clockwise from right, so 90 fades downwards), `feather` 400 (px, 1..1e6, span 10..2000: the
   distance between the full-effect line and the no-effect line). Coverage `1 − smoothstep(0, 1, t)`,
   `t = dot(p − centre, dir)/feather + 0.5`, the feather at least one output pixel.
9. **Radial Gradient** (`RadialPart`): `x` 0, `y` 0, `width` 500, `height` 500 (px, 1..1e6, span 10..3000, the
   whole ellipse), `angle` 0 (°), `roundness` 0 (−100..100: up blends to a circle on the short side, down to a
   rounder rectangle, as the vignette), `feather` 50 (0..100). `q` the point in the ellipse's own frame over its half
   sizes, then roundness as the vignette (`n = 2 − 6·min(r, 0)`), `d` the superellipse distance; coverage
   `1 − smoothstep(inner, 1, d)`, `inner = min(1 − feather/100, 1 − aa)`, aa one output pixel. Inside is covered;
   Invert covers outside.
10. **Colour Range** (`ColourRangePart`): five slots `used1..5` (bool) and `colour1..5` (Color, never keyed), and
    `refine` 0..100 (50). Picks are taken with the eyedropper from the shown picture, like Point Colour. Distance
    `d` = the least, over picks, of `|(0.5·ΔL, Δa, Δb)|` in Oklab between the pixel (past white `tamed`, as Point
    Colour) and the pick; coverage `1 − smoothstep(0.35·o, o, d)`, `o = 0.03 + 0.25·(refine/100)^1.3`.
11. **Luminance Range** (`LuminanceRangePart`): `low` 50, `high` 100 (0..100, kept in order), `smoothness` 50
    (0..100). `y` = the tone luma of the pixel as shown (`roll_off`, so past white reads as white), 0..1;
    coverage `smoothstep(lo − f, lo, y)·(1 − smoothstep(hi, hi + f, y))`, `f = max(0.3·smoothness/100, 1/512)`.
12. **Range parts read the colour the mask starts from** (`OP_BEGIN`'s saved colour, which rides through area
    passes in `kept`): the picture as graded so far, as the spec says.
13. **Program**: `OP_BEGIN` / `OP_END` as before; a mask with an area always gets them. `OP_END`'s data, for a mask
    with an area: `[parts, invert, overlay, 0]`, then each part `[kind, combine, invert, words]` + its words:
    - linear (kind 1): `[x, y, angle rad, feather]`, then copies `[count, reach, 0, 0]` and count words
      `[at.x, at.y, turn, scale]`, sorted by `at.y`; reach −1 (a linear reaches everywhere);
    - radial (2): `[x, y, half w, half h]`, `[angle rad, roundness, feather, 0]`, copies (reach = the furthest a
      copy's ellipse gets from its place);
    - colour range (3): `[picks, inner, outer, 0]`, then a word per pick `[L, a, b, 0]`;
    - luminance range (4): `[low, high, fade, 0]`.
    The Whole picture (no area) keeps the old wordless `OP_END`, so parts 1–3 grades compile to the same words.
    **No new op**: the next free op stays 17.
14. **Ball copies live in the program** (the storage buffer the passes read), inline in `OP_END`: no new binding,
    no stand-in. A grade following moving balls is compiled and uploaded each frame (a few KB; the stack is compared
    as now). The shader finds the copies within `reach` of the pixel's row by a binary search on the sorted `at.y`,
    so a few hundred balls cost a few hundred tests per pixel at most.
15. **The overlay (O)**: `GradeMask.overlay` (the editor sets it on the selected mask): `OP_END` keeps that mask's
    coverage, `run_grade` returns it, `fs_grade_pass` carries it in `kept`'s alpha (unused until now), and the
    composite mixes `mix(c, (1, 0.02, 0.02), 0.5·cov)` in linear light. A mask with the overlay is compiled even
    when its panels do nothing or its Amount is 0. Never set for an export.
16. **gfx data** (`frame/grade.rs`): `GradeMask { amount, area: Option<GradeArea>, panels, overlay, id }`
    (`None` = the Whole picture, all covered), `GradeArea { parts, invert }`, `GradePart { combine, invert, shape,
    id }`, `GradeCombine`, `GradeShape::{Linear(GradeLinear), Radial(GradeRadial), ColourRange(GradeColourRange),
    LuminanceRange(GradeLuminanceRange)}`, `GradePlace`. `id`s are the layers' ids, for the editor (overlay,
    handles). `is_identity` counts an overlay as doing something.
17. **CPU twin**: `maths::part_cover`, `maths::area_cover(area, colour, p, output px)` (the same formulas; tests
    compare the GPU against them).
18. **Editor** (`grade_mode/`):
    - `GradeMode` gains `mask: Option<LayerId>` (selected; `None` the Whole picture), `part: BTreeMap<mask, part>`
      (the part whose handles show), `overlay` (O), `renaming`, mask row drag state, handle drag state and `shown`
      (the last drawn stack, before Before/Split, for the handles).
    - **Left column** (`masks.rs`, new): Whole picture, then each mask row: a kind icon (painted from its first
      part), the name, a note ("Thing not found", "First 4096 balls only"), the eye. Click selects; drag reorders
      (a line shows where it lands, as panels); double-click or right-click Rename edits the name in place;
      right-click Duplicate, Invert, Delete, Move up, Move down. **+ Add mask** menu: Linear Gradient, Radial
      Gradient, Colour Range, Luminance Range. A **Show overlay (O)** switch under the list. Then the Histogram.
    - **Right column** (`panels.rs`): for a mask, its name and Add panel, its Amount and Invert, then **Parts**
      (`parts.rs`, new): each part a box (fold, name, Combine chip menu except the first, on/off switch, right-click
      Remove part / Move up / Move down), its settings (`grid`), a Colour Range's swatches with Pick / Remove and
      Refine; **+ Add part** with submenus Add / Subtract / Intersect of the four kinds. Then its panels as now.
    - **Handles** (`handles.rs`, new): in Grade mode, for the selected mask's selected (or first) Linear/Radial part,
      on its first place: Linear the three lines, a pin (move) and a turn dot; Radial the outer and inner ellipses, a
      pin, four size dots, a turn dot. Each drag is one undo step (`own_step`); values go through `GradeEdit::Set`
      (keyed at the playhead when keyed). With Before shown the handles still show.
    - **O** (no modifiers, nothing focused) toggles the overlay in Grade mode; the status line names it.
    - `GradeEdit` generalised to masks: `AddPanel(mask, kind)`, `ResetOrder(mask)`, `Enable(layer, on)`; new
      `AddMask(kind)`, `RemoveMask`, `MoveMask(id, to)`, `DuplicateMask`, `Rename(id, name)`, `AddPart(mask, kind,
      Combine)`, `RemovePart`, `MovePart(id, to)`, `RemoveRangePick(part, slot)`. `Applied.added` returns a new
      mask's id (the column selects it).
    - The preview's `Drawn` key includes the overlay and the selected mask, so turning O on redraws.
19. **Restarts**: `grade_settings_restart_nothing` adds a mask with every part kind; checked count rises to ≥ 140.

## Files

### bpe-gfx
- `frame/grade.rs`: the types in decision 16; `GradeMask` gets `Default` (amount 1, no area); exports in `lib.rs`.
- `grade/mod.rs`: `PART_LINEAR..PART_LUMINANCE`, `COMBINE_*`, `MAX_BALLS`, `OVERLAY_RED`.
- `grade/compile.rs`: `mask()` emits BEGIN/END for an area or an overlay; `cover_words(area, overlay)`.
- `grade/maths.rs`: `linear_cover`, `radial_cover`, `colour_range_cover`, `luminance_cover`, `combine`,
  `area_cover`, `range_widths(refine)`.
- `shaders/grade.wgsl`: `grade_cover` and the four parts; `GradeOut.overlay`; `run_grade` starting overlay.
- `shaders/post.wgsl`: `graded` passes kept alpha in; `fs_grade_pass` writes overlay into kept alpha;
  `fs_composite` mixes the overlay.
- `grade/pictures_tests.rs`: tests in "Checks".

### bpe-kit
- `modules/grade/masks.rs` (new): kinds, settings, `places`, `area_of`, `follow_note`, `add_mask`, `add_part`.
- `modules/grade/mod.rs`: register the kinds; `stack(card, world, video)`; mask/part helpers (`masks_of`,
  `parts_of`); `panels_of` unchanged.
- `things/scene/draw.rs`: passes `cx.world` and the video size to `stack`.
- `modules/grade/tests.rs`: kit tests.
- `tests/restarts.rs`: masks and parts.
- `tests/full_size_look.rs`: `grading-masks` scene.

### bpe-studio
- `grade_mode/mod.rs`, `cx.rs`, `panels.rs`, `colour.rs` (range swatches share Point Colour's widget),
  new `masks.rs`, `parts.rs`, `handles.rs`; `preview.rs` (handles in Grade mode, `Drawn`, `shown`);
  `app/shortcuts.rs` (O); status line; `grade_mode/tests.rs`.

### docs
- `docs/notes/grading.md`: "Part 4 as built". This plan's "As built".

## Steps

1. **gfx data and maths** (TDD): types, `maths` cover functions with CPU tests (linear half-way at the centre line,
   radial 1 inside / 0 outside / feather, roundness, combine table, invert, luminance ends, colour range nearest
   pick, places with turn and scale). `cargo test -p bpe-gfx grade::tests`.
2. **Compile and shader**: END words, overlay, copies search. Picture tests (decision 20 list below), then
   `cargo test -p bpe-gfx -- --test-threads=2 grade`.
3. **Kit**: kinds, settings, `stack` with places; kit tests (a radial on one ball sits on the ball; every ball gives
   a copy per ball; world place moves with the camera; a missing target covers nothing; a hidden mask is left out;
   new mask has a part and Basic). Restarts test.
4. **Editor**: edits, masks list, parts, handles, O, tests. `cargo test -p bpe-studio grade_mode`.
5. **Full-size look scene** `grading-masks` and the look check (open the pictures, look at mask edges, banding in
   feathered edges, the overlay is not in exports).
6. **Lint, all tests, goldens unchanged** (`cargo fmt --all`, clippy `-D warnings`, `cargo test --workspace --
   --test-threads=2`), notes, one Opus review, PR, merge on green.

## Checks

- Picture tests (`test_device`): a grade of parts 1–3 compiles to the same words (byte-identical frames); a radial
  mask grades its inside and not its outside, the GPU matching `maths::area_cover` within 2/255 along a row; linear
  half-way at its middle; Add / Subtract / Intersect of two radials give the expected coverage at sample pixels;
  Invert; mask Amount 0 changes nothing (and with the overlay shows only the red); a luminance range brightens
  only the bright half of a ramp; a colour range picks the red patch and not the blue; every-ball copies grade
  round each of three places; a mask with Clarity inside (area pass split) still covers only its part; overlay
  red where covered.
- Kit tests (decision list in step 3); `restarts.rs` sees no mask or part key restart anything; goldens unchanged.
- Editor tests (kittest): Add mask makes a mask with a part and a Basic panel and selects it; the panel column
  shows the selected mask's panels; rename; delete; duplicate; move down; eye; Add part Subtract; the first part
  shows no Combine; O sets the overlay on the selected mask's stack only; a handle drag moves a radial's centre
  (the drag maths on its own).
- Full-size look scene `grading-masks`.

## Things not to do

- Don't add Things parts, Brush, scopes, snapshots or grade files (parts 5 and 6).
- Don't carry coverage across passes (only the overlay needs it, in `kept`'s alpha).
- Don't build a shader per mask; every mask runs in the one interpreter.
- Don't let a grade key restart physics, sound or particles; don't show the overlay or handles in an export.
- Don't change the words of a grade without masks (byte-identical frames for parts 1–3 grades).

## As built

Built as planned; the summary is "Part 4 as built" in `docs/notes/grading.md`. Where it differs or adds:

- **Names count per mask**: a mask's panels and parts are named after their kind and numbered among that mask's
  own (`grade::name_among_siblings`), so a new mask's panel is "Basic", not "Basic 2".
- **`GradeMode.shown` is a `RefCell`**: the picture is built through `preview::compose`, which only reads the
  studio, so it keeps the last stack there for the handles.
- **Handles**: a drag works from the part as it was at the press (the picture catching up a frame later never
  moves the handles under the pointer). A Linear's middle line also moves it. A Radial's size dot puts the edge
  under the pointer for a plain ellipse and scales in proportion when rounder.
- **Renaming** asks for focus only when it opens (asking every frame would take it back from Enter), with a small
  "Name" label for the field.
- The overlay is never set for an export: only `GradeMode::show_on` sets it, on the editor's preview.
