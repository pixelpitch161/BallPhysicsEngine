# Marks part 2: skin paint, better splats, clean trails

Date: 2026-11-02. Builds on `2026-11-01-marks-layer-design.md` (part 1 merged as PR #64).
Design doc agreed with Aedan: https://claude.ai/artifact/BZNewr45Zyq8kGhXUzBzqs

## Why

Aedan judged part 1's look weak, the splats above all: a splat on a canvas that rides a bar gets
cut off by the canvas's rectangle and doesn't sit on the thing that was hit. When a ball splats a
wall or obstacle, the paint should go on **that thing**.

## What we agreed

- **Skin paint:** walls (arenas) and obstacles carry their own paint, shaped like them, moving
  and spinning with them. No canvas needed.
- **Per brush, Print or Spread:** each Splat brush picks how a hit paints a skin.
- **Trails:** clean strokes now. Wet, Watercolour, Neon, drips, pickup and pictures move to
  part 3.
- **Approach:** a hidden canvas per thing, reusing part 1's machinery.
- **New part numbering:**
  - Part 2 is this spec.
  - Part 3 is rich paint: the styles, spread, drips, pickup and pictures.
  - Part 4 is the old part 3: Cover, Powder, Fog, Scrape, Claim and numbers.

## Design

### 1. Skins (`bpe-kit/src/things/canvas/skin.rs`, arena and obstacle settings)

**Settings.** Arenas and obstacles get a **Paint skin** settings group in their own Thing card
(not in Look, because one Look module is shared by balls and walls):

| key | type | default | notes |
| --- | --- | --- | --- |
| `skin_on` | bool "Takes paint" | true | |
| `skin_width` | f32 "Skin width" 0..3 | 1.0 | Extra reach past a thin line, in multiples of the line's half width. Solid shapes ignore it. |
| `skin_fade` | f32 s | 0 | 0 means never. |
| `skin_opacity` | f32 0..1 | 1 | Keyframeable. |

**What a skin is.** A skin is a `CanvasState` owned by a thing. It is not a tree entry.

- The skin's world keeps it in a new `World.skins: BTreeMap<ThingId, CanvasState>`.
- It is built at `World::build` for every thing whose kind supports skins (arena, obstacle), when
  `skin_on` is set and the scene has at least one brush with Splat on and Lands on including
  "The thing hit".
- Its area is the thing's local bounding box (all items, e.g. a peg grid), plus the largest
  splat reach (`splat_size × (1 + splat_by_speed × 2)`) as a margin.
- Its pose is the thing's pose every step, so marks are logged in the thing's local frame and
  spin and move with it.
- The grid uses part 1's rules: cell size, tiles and the memory count.
- Its marks go into `Log.marks` with a new `Mark.target: MarkTarget { Canvas(ThingId),
  Skin(ThingId) }`, which replaces `Mark.canvas`. Marks stay unhashed and golden logs stay
  unchanged.
- Fingerprint: skin settings count only when the scene has marks (the same gate as World.looks).

**Mask.** A skin shows only where its thing is. The skin composite multiplies the paint by the
thing's own coverage. It uses the same signed distance the thing's line is drawn with (ring,
polygon, box, capsule, peg, bar), from the thing's `ShapeInstance` data for this frame:

- **Thin line** (arena outline, ring and polygon obstacles): cover where
  `|d| <= half_line × (1 + skin_width)`.
- **Solid** (peg, peg grid, bar): cover where `d <= 0`.

The edge is anti-aliased over one pixel. The mask is evaluated in the composite shader. The
draw part pushes a `SkinLayer { place, textures, shapes: Vec<ShapeInstance>, opacity, fade }`
right after the thing's own shapes, so the skin sits on top of its line and under the balls.

**GPU.** A skin is a `Layer::Canvas` in FlowLayers, keyed by a skin id, with replay, checkpoints
and texture budget exactly as in part 1. Composite uses part 1's Flat style with the mask
added.

### 2. Splats onto skins (`bpe-kit/src/modules/marks`)

The Splat section gains two settings:

| key | type | default |
| --- | --- | --- |
| `splat_onto` | choice "Lands on": Thing hit / Canvases / Both | Thing hit |
| `splat_skin` | choice "Skin look": Print / Spread | Print |

- The **mover's brush** paints the skin of the thing it hit (`e.b`) at each hit.
- A **wall's or obstacle's own brush** paints its own skin wherever something hits it.
- A ball-to-ball hit has no skin to paint and paints canvases only.

Two looks:

- **Print:** the splat shape at the hit point, turned by `splat_turn`, with radius as in part 1.
  Its centre is pushed `0.3 × radius` into the surface along the normal, so the print sits
  across the line and doesn't float beside it.
- **Spread:** colour smeared along the surface on both sides of the hit point.
  - Length is `2 × splat_size × (1 + splat_by_speed × min(speed/1000, 2))`.
  - Width is the skin's line reach for thin lines, and `0.5 × splat_size` for solids.
  - The sim walks the thing's outline from the projected hit point in both directions, in
    pieces of at most 8 px, and logs each piece as a `MarkShape::Smear` capsule. The radius
    tapers to 0 at both ends (radius in `radius`, taper position in `variant` as 0 to 255).
  - Outlines come from a new `ThingKind::outline(thing, world) -> Vec<Polyline>` (local frame)
    implemented for arena and obstacle shapes. Circles and rings use 64 segments.

Colour follows part 1's rules: from the Ball (the mover) or the brush's own Colours.

### 3. Better splat shapes (`canvas_stamp.wgsl`)

- **Splat** is redrawn:
  - a main blob (smooth union of 3 to 5 offset discs);
  - 4 to 9 satellite droplets, thrown toward the ball's direction of travel;
  - a short streak along that direction.
- The direction is passed in `angle`. For Print, `angle` faces the hit and a squash factor
  (in `variant`'s high bits) flattens the blob along the surface for hits faster than 600 px/s.
- `variant` picks the layout (1 to 8 variants as before).
- All shapes use a smooth-union SDF with anti-aliasing scaled to the texel. Shapes must not
  show faceting at detail 1.
- Ring, Star, Crack, Burst and Dots stay as they are.

### 4. Clean trails (`canvas/sim.rs`, `canvas_stamp.wgsl`)

1. **Root cause first.** Find out what makes the jagged teeth along part 1's trail edges (see the
   Paint pour video) and fix that cause with a pixel test that fails today.
2. **Ribbons.** Each painting ball's stroke is a continuous ribbon:
   - Consecutive pieces share their joint exactly, and the joint's coverage isn't drawn twice:
     each piece leaves out the disc at its start when it continues the previous piece, so there
     are no darker overlaps with see-through colours.
   - Colour and radius are interpolated along each piece, from its start values to its end
     values. `Mark` gains `radius_to` and `colour_to`.
3. **Smoothing.** The sim holds one point back and logs the piece from the point before last to
   the last point as a Catmull-Rom curve, split into pieces of at most 6 px. Fast balls draw
   curves. A bounce point is a corner (no smoothing across a hit).
4. **Taper.** A new `paint_taper` setting (0..1, default 0.3) narrows the first and last
   `taper × 40 px` of each ribbon. The last piece is tapered when the stroke ends (ball stops
   painting, dies, or the load runs out).
5. **Speed width** is smoothed with a new `paint_smooth` setting (0..1, default 0.5, an
   exponential average per ball) so it swells and thins rather than jumping.

### 5. Editor

- The Paint skin group is shown in arena and obstacle inspectors, with `when = "skin_on"` on
  its other rows.
- Lands on and Skin look sit in the Splat section. Taper and Smoothing sit in Paint trail.
- No new gizmos.

### 6. Demos, tests, deliverables

**Demo scenes** (bless golden lines for the new ones, keep the others byte-identical):

- `splat-ring` is reworked: the ring arena and spinning bar take Print and Spread splats, and
  there is no canvas.
- `paint-pour` gets clean trails with taper.
- A new `peg-paint`: a peg board whose pegs take Print splats while balls also paint trails on
  a canvas behind them.

**Tests:**

- Same run gives the same skins.
- Seek equals straight for skins.
- Skin marks are logged in the thing's local frame and follow a spinning bar.
- The mask keeps paint on the thing: a pixel test checks nothing outside the line plus its reach.
- Spread follows a circle arena's curve: all pieces lie within the reach of the outline.
- The ribbon has no gaps or double-darkened joints: a pixel test with a half-transparent colour.
- The trail edge has no teeth: the root-cause test from section 4.
- A no-marks scene keeps its fingerprint and golden logs.
- Speed: 20 balls hitting a ring with Spread on stays under 1 ms per step in release.

**Deliverables:** videos of the three demos, editor screenshots, a Windows exe, a REPORT.md and a PR.

## Left for later

- Part 3: Wet paint, Watercolour, Neon ink; spread and drips; pickup; picture splats.
- Skins on soft bodies and balls.
- Paint trails onto skins: "Touching a wall" painting along a wall.
