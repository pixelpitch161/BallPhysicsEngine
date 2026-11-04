# Polish batch: editor, balls and shapes (plan)

Aedan's 14-item list (2026-11-04). Small changes across the editor, the Ball Look and the Shape thing, built in one
branch (`claude/project-thread-d87dxs`), one review at the end, one PR. No screenshots or demo renders.

The Ball Look blend fix of PR #75 (`ThingDraw::draw_over`, `Stack::split`, `LayeredInstance.range.z`) is the model
for the shape blend fix (item 14).

## Decisions taken (Aedan didn't weigh in; sensible calls)

- **Fullscreen** = the window opens **maximised** (title bar and taskbar stay, so it can still be closed and
  moved). F11 toggles true borderless fullscreen.
- **Tree width**: the tree opens at its widest (480, its maximum); it can still be dragged narrower.
- **Track colours** become any colour: `TrackColour` is an sRGB triple saved as `"#rrggbb"` (old names like
  `"pink"` still read). A new thing gets a random readable colour (HSL hue random, saturation 0.55-0.85, lightness
  0.60-0.72), the hue picked from 16 random candidates as the one furthest from the hues already in use. Kinds no
  longer have their own colour (camera violet / shape orange go). The picker is egui's HSV picker in a popup,
  opened by clicking the colour tag of a tree row, the colour square of a timeline group, the swatch at the top of
  the inspector, or "Colour" in a tree row's right-click menu.
- **Launch arrow**: drawn (and grabbable) only for the selected Balls thing, or while it is being dragged.
- **Shape handles**: a selected shape no longer gets a thick yellow outline, dots on every side and squares on
  every corner. It shows its faint outline, the middle ring, and only the handle under the pointer (or being
  dragged): hovering a side lights that side and shows its two corners, hovering a corner shows that corner and
  lights its two sides, a circle's edge lights the edge and shows the radius dot. Bend handles of curved sides stay
  shown while the shape is selected (they sit off the shape and could not be found otherwise).
- **Inside outline gap** is the AA conflation of a ball split by its Blend layer: at an edge pixel covering α the
  lower (blended) part shows through the upper part by α(1-α). Fix: the lower part works out how much the upper
  part covers (`t_in`, `t_out`) and draws `(α(1-t_in)·inner + (1-α)(1-t_out)·outer) / (1-u)` with
  `u = α·t_in + (1-α)·t_out`, which composites exactly to the unsplit result (for blends mixed in linearly by
  coverage). Same for shapes.
- **Shine colour**: new `Light` setting (White / Own colour). White is today's shine exactly. Own colour shows
  Colour (gradient), Gradient (Straight / Round) and Direction like Outline/Glow, and tints the light's highlight
  (the spot, sheen, glint and rim), not the body under it.
- **Filled Ring** group shape: rings of balls inside the Ring's Radius, the outer ring at Radius and one every step
  (ball size × 2 + Space between) inward; each ring holds as many balls as fit with no two closer than a step
  (n = ⌊π / asin(step / 2r)⌋), clockwise from the top; a ring smaller than half a step becomes one ball in the middle.
  Numbered from the middle out. Its settings: Radius and Space between; How many is not shown (like Word), the
  Group view's note says how many balls it holds. Filled circle stays.
- **Ball size**: the Balls `radius` setting is relabelled "Ball size" and shown as a row under the Ball / Group
  switch (both modes). The Balls card itself is no longer drawn for a Balls thing with a layered Look card; without
  one it holds only the switch, Ball size and the Group view.
- **Launch speed** moves to the Placement card (a settings row under Placement's own rows), with keyframes and the
  right-click menu as before.
- **Shader card** removed from Balls and Shape (`SlotId::Shader` leaves their slot lists). The scene settings keep
  their Effects card; the effect kinds stay registered for it.
- **Z-order**: `Thing.z_order: i32`, default 0, range -100 to 100, a Placement row "Z-order" for things that draw
  (not the camera, not folders, not the scene). Higher draws over lower. At equal z-order nothing changes (shapes
  under balls, then id order). Things stuck to the screen ignore it (they draw over the world anyway).

## Z-order in the renderer

`BlendLayer` gets `z: i32` and `place: Place` (replacing `behind: bool`):
`Place::{Back, Behind, BehindOver, Front, Blend, Over}` (in drawing order within one z):
- `Back`/`Front`: plain (Normal at full) layers holding things at z ≠ 0: shapes (`ThingDraw::behind`) go Back,
  everything else (balls, every module's trail/particles) Front. Drawn straight into the HDR picture
  (`draw_things`), no composite.
- `Behind`: a blended shape's layer (was `behind: true`).
- `BehindOver`: what a split shape paints over its blended part (shapes' `draw_over`).
- `Blend`: other blended things.
- `Over`: what a split ball paints over its blended part.

`build_frame` routes each thing by `(z, blend)`; at z 0 it is as today. After drawing, layers are sorted stably by
`(z, place)` (replacing `over_layer_last`). The renderer draws: background, layers with z < 0, the frame's back
lists, z-0 Behind/BehindOver layers, the frame's balls, objects, the Things effect stage, flashes, z-0 Blend/Over
layers, layers with z > 0, overlay. `Frame::layer_mut` keeps its signature (z 0, Blend).

## Shape blend split (item 14)

- `ShapeStack` keeps `blend_at` (the Blend layer's place among `drawn`) and gets `split() -> Option<u32>`: `None`
  without a Blend layer, for Normal at full amount, or when nothing that writes a record sits above it; else 1 +
  the records of the drawn layers under it (a Shader whose code doesn't read writes none).
- `PathInstance.layers[3]` becomes the split (same encoding as balls: low 16 bits the first record above the
  Blend layer, bit 16 the upper part). `ShapeDraw::draw` writes `part`, `draw_over` pushes the shape again with
  `cut | OVER`.
- `shape_walk.wgsl` takes `split`; lower part skips records ≥ cut (but measures their cover for the conflation
  fix); upper part accumulates them with `top_over` as balls do and returns them. `in_shape.wgsl` passes 0.
- `build_frame` routes `draw_over` of a `behind()` thing into a `BehindOver` layer.

## Tasks

1. Window and tree: `main.rs` `.with_maximized(true)`, F11 toggles `ViewportCommand::Fullscreen`; `TREE_W` 480.
2. Track colours: core `TrackColour` struct + serde + random pick (`Scene::new_colour`), remove
   `ThingKind::track_colour` and the kinds' impls; `style::track_color` reads the triple; picker popup
   `style::colour_popup`; tree tag click, tree menu, timeline square, inspector swatch. Tests: serde round trip
   and legacy names, random colours readable and spread, tree/inspector picker edits.
3. Launch arrow only when selected (`draw_handles`, `hit_test`). Test: unselected balls' tip isn't grabbed.
4. Shape handles on hover (`shape_handles::draw`). Test: hit-testing unchanged (existing tests).
5. Inspector: Ball size row by the Ball/Group switch, Balls card not drawn with a layered Look, Launch speed and
   Z-order in Placement. Tests: kittests for the rows' places.
6. Shader slot off Balls and Shape; fix tests that used it on a ball.
7. Filled Ring: `GroupShape::FilledRing`, `shapes::filled_ring`, `when` lists, chips, note. Tests: spacing ≥ step,
   outer ring at Radius, middle ball, deterministic.
8. Shine light colour: `ShineLight` choice, colour/gradient/direction settings, record flag + paint, shader
   tint. Test: default record unchanged; own colour record carries colour and paint; GPU test light tint.
9. Shape blend split (above). Tests: split positions, two path instances in Behind/BehindOver layers, GPU test
   that a layer above the Blend layer paints normally.
10. Conflation fix in both walks. GPU test: an Inside outline above a Blend layer covers the edge pixel like the
    unsplit ball (no ring of the blended body).
11. Z-order (core field, Placement row, gfx routing, renderer). Tests: build_frame layer order and places, GPU test
    that a raised shape covers a ball.
12. CLAUDE.md map updates; fmt, clippy, full test suite debug and release; goldens must not move (drawing only,
    except none touch physics).

## Not doing

- No change to physics (goldens untouched). No new built-in scenes. No hit-test ordering by z-order.
- The scene's Effects card stays.

## Verification

`cargo test --workspace` (debug and `--release`), `cargo clippy --workspace`, `cargo fmt --check`; goldens unchanged
(`git diff main -- crates/bpe-kit/tests/golden*` empty). One Opus review of the whole branch before the PR.
