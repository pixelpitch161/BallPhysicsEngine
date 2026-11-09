# Colour grading part 5: Masks, second half (Things and Brush) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give masks their last two part kinds. **Things** covers exactly what chosen things draw (Only balls,
Only shapes, Only text, Only particles, Only background, or one named thing), with **Expand** and **Feather** in
pixels to take in some of their glow. **Brush** covers painted strokes (Size, Feather, Flow, Density, Erase, brushes
A and B, editor only), fixed on the screen, in the world, or following a thing, one ball or every ball, like the
gradients. Also fix part 4's known gap: under a moved Stage camera, following gradients (and now brushes) drift off
their balls.

**Spec:** `docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved by Aedan 2026-11-09), part 5 of 6
(sections 3, 5, 6, 7). **Before:** `part1.md`..`part4.md` here, `docs/notes/grading.md`, handover
`ballphysics/colour-grading/part4-handover.md` in the project files.

**Tech:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37.

## Decisions made here (the spec leaves them to the plan)

### Things

1. **Kind** `grade.part.things` (`ThingsPart`): `combine`, `invert` (as every part), `only` (`ThingsOnly`: Balls,
   Shapes, Text, Particles, Background, One thing; default Balls), `target` (`Target`, "Which thing", shown for One
   thing; default every Balls group), `expand` (px, 0..1e4, span 0..200, default 0, keyframe, pulse), `feather` (px,
   0..1e4, span 0..200, default 0, keyframe, pulse). Both are pixels of the video, so a small preview matches the
   export.
2. **What counts as a thing's silhouette**: its body only, as the object picture already does: a ball's disc (its
   shape and cuts; balls of a Balls group or an Emitter), a shape's or text's outline fill, a Particles thing's own
   sprites (not their shadows, glows or outlines) and its Huge swarm's particles (body role). Trails, sparks, glows
   and other look decorations are not; Expand and Feather take their glow in. A Particles thing's soft fields and
   liquids are not in its silhouette (stated in the notes as a known gap).
3. **Which things**: Only balls = every thing of kind `balls` and `emitter`; Only shapes `shape`; Only text `text`;
   Only particles `particles`; One thing = `World::matching(target)` (a thing, a tag or a kind). Background = where
   no thing's silhouette covers. Codes are `thing_code(id)` (id + 1) as whole numbers.
4. **Only what shows**: silhouettes are drawn in drawing order with premultiplied "over" blending: a chosen thing
   adds its coverage, any other thing covers what is under it. So a shape in front of a ball hides the ball's mask
   there. Red = the chosen things as seen, alpha = every thing (so Background = 1 − every thing, through the same
   path with the "every thing" selection).
5. **A named thing that isn't there** (One thing naming nothing) is left out like a hidden part (covers nothing
   even inverted) and its row says "Thing not found". A kind with no things now (an emitter before its first ball)
   covers nothing; inverted it covers everything (the picture has none of them).
6. **gfx data**: `GradeShape::Things(GradeThings { things: Option<Vec<u32>>, background, expand, feather })`;
   `None` = every thing (Background uses it), `Some(empty)` = none.
7. **Selections** (`Program.selections: Vec<Selection { codes: Option<Vec<u32>>, reach }>`): compile dedupes the
   Things parts of the masks it emits by their thing list, at most `MAX_SELECTIONS` = 8; past that a part covers
   nothing (layer −1, noted in the notes). `reach` is the most `expand + feather` (video px) any part on it asks.
   `Some(empty)` never gets a selection (covers nothing at no cost).
8. **The silhouette pass** (`bpe-gfx/src/silhouettes.rs`, `shaders/silhouette.wgsl`), run by `Renderer::finish`
   before the grade passes, only when the program has selections. Per selection:
   - clear an `Rgba8Unorm` picture (output size) and draw the frame's lists in drawing order with silhouette
     pipelines: layers below 0; own sprites behind (+ swarms), shapes and text; layers behind the balls; own balls;
     own sprites in front (+ swarms); layers above; then the stuck-to-screen overlay the same way through the
     screen globals. Each fragment outputs `(picked · cover, 0, 0, cover)`, blend One / OneMinusSrcAlpha. A list
     item with code 0 (not a thing's body) is discarded.
   - The fragment shaders look the code up in a sorted code list (storage buffer) by binary search; the
     selection's range comes from a uniform at a dynamic offset (256 bytes a selection).
   - **Distance by jump flooding**: an init pass seeds pixels whose chosen coverage is at least 0.5 (reading the
     silhouette through the Stage camera's per-pixel uv when the moved Stage camera is on, as `see_objects` does);
     then steps of `2^k` down to 1 from `nextpow2(reach · scale + 2)` (`scale` = output height / video height; no
     steps when reach is 0); a last pass writes layer `i` of the **things field** (`Rgba16Float` 2D array): r the
     chosen coverage, g the distance in output pixels to the nearest seed (`1e4` with none), a every thing's
     coverage.
   - Seeds are pixel coordinates in an `Rgba32Float` pair (so 4K exports stay exact).
9. **Program words**: `PART_THINGS` = 5: `[layer, expand, feather, background]` (layer −1 covers nothing). The
   grade group gains binding 4, `grade_things: texture_2d_array<f32>` (a 1×1×1 stand-in when no Things part runs).
   **Coverage**: `s` = field.r, `d` = field.g, `E = expand · scale`, `F = feather · scale` (output px);
   `v = s` when `E + F = 0`, else `v = max(s, 1 − smoothstep(E − 0.5, E + F + 0.5, d − 0.5))`; Background
   `v = 1 − v`. CPU twin `maths::things_cover`.
10. **Particles carry their thing**: `Particle.shape[2]` (spare) is the thing code of a Particles thing's own
    sprites (`Pieces::mark_thing`, OWN only); `SpriteOut.code` passes it on. A Huge swarm's look gets the code in
    `t2.w` (spare); `vs_swarm` puts it in `shape.z` for the body role only. Nothing reads them but the silhouette
    pass, so every picture stays byte-identical.

### Brush

11. **Kind** `grade.part.brush` (`BrushPart`): `combine`, `invert`, `strokes` (`Strokes`, never keyed, not in the
    grid), `place`, `target`, `ball`, `turns`, `grows` (as the gradients: `Placed`). Points and sizes are in the
    place's units (video px from its place, y down, as the gradients' X/Y), not the spec's 0..1: the same units as
    part 4's gradients and their handles.
12. **`Stroke.density`** (bpe-core, new, default 1, saved only when not 1, hashed only when not 1): the most paint
    a stroke builds up to. Painting a stroke onto paint `l` with coverage `v`: `l' = max(l, min(l + v(1 − l),
    density))`, erasing `l(1 − v)`. At density 1 this is the old sum, so the ball brush is unchanged (its tiles
    stay byte-identical).
13. **Lightroom's brush → a stroke**: Size (px, its radius) → `size` (in place units: px / place scale); Feather
    0..100 → `softness`; Flow 0..100 → `strength`; Density 0..100 → `density`; Erase (the Erase brush, or Alt held)
    → `erase`. Brushes A, B and Erase each keep their own Size, Feather, Flow, Density (editor state, not saved;
    A 80 px / 50 / 100 / 100, B 25 px / 0 / 100 / 100, Erase 60 px / 50 / 100 / 100).
14. **The brush grid** (`bpe-gfx/src/grade/brush.rs`): strokes rasterised on the CPU into a grid of 8-bit paint
    over the strokes' box (points ± size, snapped out to 64 px), one texel per `max(2, longest side / 1024)` px.
    Each stroke paints its own coverage (the smoothstep of the ball brush: full inside `size(1 − softness)`, none at
    `size`, times `strength`, the nearest of its segments) and is then laid onto the paint by decision 12. Grids
    are cached by the strokes' hash (the last 32 sets), and a set that only adds a stroke to a cached one with the
    same box starts from it.
15. **Program words**: `PART_BRUSH` = 6: `[x0, y0, 1 / texel, 0]`, `[w, h, grid word, 0]`, the copies (as the
    gradients: reach = the furthest grid corner from the place origin), then the grid, **three bytes in each f32
    lane** as a whole number (`b0 + 256 b1 + 65536 b2`, exact in f32; never bit-cast, which could make NaNs), so
    twelve texels a word. The shader samples it bilinearly at the pixel's point in the place's units, 0 outside;
    copies join by Add (the most). CPU twin `maths::brush_cover`.
16. **Painting in the editor** (`grade_mode/brush.rs`): when the selected mask's picked (or first) part with
    handles is a Brush, the preview shows the brush circle (its size and where its feather starts) and paints:
    a press starts a stroke at the pointer (in the part's first place's units, through `unplaced`), dragging adds
    points (spaced at least 1 px of the video apart), letting go keeps it as **one edit** (one undo step) of the
    part's `strokes`. While painting, `GradeMode::show_on` swaps the stroke into the preview's stack (no scene edit
    per frame); `Drawn` includes the stroke's length. The part's box has brush chips **A / B / Erase**, sliders
    Size, Feather, Flow, Density for the chosen brush, the stroke count and **Clear** (an edit to no strokes).
    A brush with nothing to follow (no place) shows no circle and doesn't paint.

### Stage camera

17. **Following under the moved Stage camera**: `GradePlace` gains `world: bool` (true for In the world and every
    following place). `build_frame` ends with `frame.grade.see_through(&view)` when `frame.stage_view()` has
    `motion > 0` (the same test the renderer uses to run the camera pass). Each world place goes back to the world
    through the flat camera (`to_world`), then to the screen through `StageView::screen_of`; its scale becomes
    `scale_at(p) · scale / zoom`; its turn is measured along a 10 px step of its own x way. A place behind the
    camera is dropped. A big gradient stuck to the world is placed by its middle (it doesn't bend with the
    perspective). The editor's handles read the shown stack, so they sit on the ball too.

### Editor and rest

18. **Add mask** and **Add part** offer the six kinds (Linear, Radial, Brush, Colour Range, Luminance Range,
    Things). A new Brush mask has an empty brush (covers nothing until painted; the part's box says "Paint on the
    picture"). Icons: a brush dab for Brush, a ball over a square for Things.
19. `follow_note`: a Brush follows as the gradients; a Things part on One thing naming nothing says "Thing not
    found".
20. **Restarts**: `grade_settings_restart_nothing` adds a Brush and a Things part; the count rises to ≥ 150.

## Files

### bpe-core
- `value.rs`: `Stroke.density` (manual `Default`, serde default 1 and skip when 1, hash when not 1).

### bpe-gfx
- `frame/grade.rs`: `GradeThings`, `GradeBrush`, `GradeShape::{Things, Brush}`, `GradePlace.world`,
  `GradeStack::see_through`.
- `frame/build.rs`: the Stage remap at the end.
- `grade/mod.rs`: `PART_THINGS`, `PART_BRUSH`, `MAX_SELECTIONS`; `grade/brush.rs` (new): the grid.
- `grade/compile.rs`: words, `Program.selections`.
- `grade/maths.rs`: `things_cover`, `brush_cover`, `brush_sample`.
- `shaders/grade.wgsl`: binding 4, `grade_things_part`, `grade_brush_one`, the brush in `grade_copies`.
- `silhouettes.rs` (new), `shaders/silhouette.wgsl` (new, scene entries), `shaders/silhouette_jfa.wgsl` (new).
- `shaders/sprites.wgsl` (`SpriteOut.code`), `shaders/swarm_draw.wgsl` (code), `swarm/draw.rs` (silhouette
  pipeline), `sprites.rs` (silhouette pipeline).
- `renderer/grade.rs` (binding 4, `set_things`), `renderer/scene.rs` (run silhouettes in `finish`),
  `renderer/new.rs`, `renderer/mod.rs` (the pipelines), `stage_camera_pass.rs` (`uv()`).
- `grade/pictures_tests.rs`, `grade/tests.rs`.

### bpe-kit
- `modules/grade/masks.rs`: `ThingsPart`, `ThingsOnly`, `BrushPart`, kinds, `area` branches, `follow_note`.
- `modules/look/layers/around_subs.rs`: `Pieces::mark_thing`; `things/particles/draw.rs`: marks; swarm code.
- `paint/brush.rs`: density.
- `modules/grade/tests.rs`, `tests/restarts.rs`, `tests/full_size_look.rs` (`grading-things`).

### bpe-studio
- `grade_mode/brush.rs` (new), `mod.rs` (state, `show_on`), `parts.rs` (brush box, Things), `masks.rs` (names,
  icons, Add mask), `handles.rs` (handle part includes Brush), `preview.rs` (paint, `Drawn`), `tests.rs`.

### docs
- `docs/notes/grading.md` "Part 5 as built"; this plan's "As built".

## Steps

1. **Core and gfx data** (TDD): `Stroke.density`; gfx types; `brush.rs` grid with CPU tests (a dot's middle full,
   its edge none; erase; density cap; incremental equals whole; texel size); `maths::things_cover`,
   `brush_cover`; `see_through` CPU test. `cargo test -p bpe-core value`, `cargo test -p bpe-gfx grade::tests`.
2. **Compile and shader** (brush and things words, binding 4 with a stand-in). Picture tests: brush GPU vs maths
   along a row; a brush on every ball.
3. **Silhouettes**: pipelines, pass, JFA, field; picture tests (decision list in Checks).
4. **Kit**: kinds, settings, areas, marks; kit tests; restarts.
5. **Editor**: Add mask/part kinds, brush box, painting, Things in the grid; tests.
6. **Full-size look scene** `grading-things` and the look check.
7. **Lint, all tests, goldens unchanged**, notes, one Opus review, PR, merge on green.

## Checks

- gfx picture tests (`test_device`): a frame of two balls and a shape. Only ball A (exposure +2) brightens A and
  leaves B, the shape and the background; Expand 10 px reaches a pixel 6 px outside A and not 14 px; Feather fades
  between; Background grades the background, not the balls; a shape over a ball hides the ball's mask (only what
  shows); a stand-alone grade without Things parts compiles to the same words and makes no silhouettes; a brush
  stroke matches `maths::brush_cover` within 2/255 along a row; erase and density; a brush on every ball.
- Every existing picture stays byte-identical (particles' code is read by nothing else); goldens unchanged.
- Kit tests: Only balls lists a group and an emitter; One thing naming nothing is left out and noted; Background
  is every thing; a Particles thing's own sprites carry its code and its glows don't; a brush on a ball; restarts.
- Editor tests: Add mask Brush / Things; a stroke from a drag (points in place units, brush settings → stroke
  fields, Alt erases); Clear; the brush circle only for a Brush part.
- `see_through`: a world place lands where `StageView::screen_of` puts its world point; a screen place doesn't
  move.

## Things not to do

- Don't draw silhouettes for a frame whose grade has no Things part, or in the light passes.
- Don't change any existing frame's pixels (particles' codes, sprite codes, the new binding).
- Don't bit-cast brush bytes into floats.
- Don't keyframe strokes; don't save brush A/B/Erase settings in the scene.
- Don't add scopes, the targeted adjustment tool, snapshots or grade files (part 6).
- Don't let a grade key restart physics, sound or particles.

## As built

Built as planned, with these differences (`docs/notes/grading.md` "Part 5 as built" has the whole of it):

- The silhouette pass lives in `bpe-gfx/src/renderer/silhouettes.rs` (shaders `silhouette_select.wgsl`,
  `silhouette_scene.wgsl`, `silhouette_sprite.wgsl`, `silhouette_jfa.wgsl`), not `silhouettes.rs` at the crate root.
- The stuck-to-screen overlay's things draw into a picture of their own, laid over the world's in the seed pass,
  so under the moved Stage camera only the world's silhouette is read through the camera's uv (the overlay is
  never swung by it, as in the picture itself).
- Swarms draw into the silhouette between the sprites where they sit, as the picture draws them.
- The brush grid's lanes are read as the whole numbers they are (`u32(x)`): adding a half first rounded a lane of
  three full bytes (2^24 − 1) up to 2^24, which read as no paint (caught by the picture test).
- The brush grid cache keeps 64 grids (paint levels 32).
