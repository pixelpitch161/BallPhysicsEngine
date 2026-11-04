# The Shape thing (replaces the Arena)

Date: 2026-11-04. Status: design approved by the owner (Aedan) in the "Design the new Shape thing" thread.
Mockups: https://claude.ai/artifact/BbFQLkEkiCmGJcd4gBeP7b (scene-view editing board, inspector layout A chosen).

## Goal

Remove the Arena and add one generic **Shape** thing whose outline the owner moulds by hand in the video, with a
layered Look like the balls' (without the big ball view). Shapes are walls: balls bounce off them.

## What the owner decided

| Question | Decision |
| --- | --- |
| What a shape does to balls | Balls bounce off its edges **from both sides**. Hollow = a band: balls inside stay inside (like the Arena), balls outside bounce off it. Filled = solid: balls bounce off the outside only. |
| Spin and Shrink | Kept, as the Shape's own settings. |
| Dragging a polygon | **Free shape.** Drag a corner = only that corner moves. Drag a side = both its corners move together. Changing Sides afterwards rebuilds a fresh even polygon (one undo step brings the moulded shape back). |
| Mirror | Holding **Shift** while dragging mirrors the move across the shape's own up-down middle line (it turns with the shape). |
| Circle | Its radius is Size, also set by dragging its edge. Right-click the edge: **Turn into points** (becomes a mouldable shape of 8 curved sides that looks the same). |
| Colour | **Body is plain white.** Colour comes from stacked **Colour and gradient** layers above it (part 2). |
| When handles show | **Whenever the Shape is selected.** Dragging the shape elsewhere moves it. |
| Right-click menus | Side: Add point, Create curve (Straighten when curved). Corner: Delete point (a shape keeps at least 3). Circle edge: Turn into points. A curved side shows two bend handles. |
| Inspector layout | **A: Body is the bottom layer** of the Look's layer list, like a ball's Body. Picking it shows the shape settings under the list. |

Small call made while designing (tell the owner): there is no separate "Turn" setting. The Placement section's
Rotation turns the shape and Spin animates it, as with the Arena.

## The thing

- Kind id `shape`, name "Shape", family "Shapes", icon a polygon outline, track colour Orange (the Arena's).
- Help: "A shape the balls bounce off: hollow or filled, any outline you mould in the video."
- Cards: Look, Bounce, Particles, Shader allowed; a new Shape starts with Look and Bounce.
- A new Shape is a **plain white hollow circle**: Size 400 px, Thickness 8 px, no spin, no shrink.
- Several shapes can be in one scene. Every one is a wall for every ball.

### Its settings (`ShapeSettings`, the thing's own params)

Grouped "Body" (shown under the Body layer, left off the thing's own settings grid):

| Key | Label | Type | Default | Notes |
| --- | --- | --- | --- | --- |
| `start` | Start from | Choice Polygon / Circle | Circle | |
| `sides` | Sides | Int 3..64 | 6 | `when = "start=polygon"` |
| `size` | Size | px, span 20..2000, range 1..20000 | 400 | Circle radius, or a polygon's corner radius. Keyframe. Scales moulded points. |
| `fill` | Fill | Choice Filled / Hollow | Hollow | |
| `thickness` | Thickness | px, range 0.5..400 | 8 | `when = "fill=hollow"`. Keyframe. |
| `path` | (no row) | `Value::Path` | absent | The moulded outline. Absent = the even polygon from Sides, or the circle. |

Not grouped (the thing's own settings, in the "Shape settings" fold above the cards): `spin` (deg/s, keyframe),
`shrink` (per s, keyframe), `min_size` ("Smallest size", px, `when = "shrink"`). Same meaning as the Arena's.

The structure lives in the thing's params (not in the Look card) because it is physics: the runtime already
re-reads a thing's own params on keyframes and hashes them for "does this change the simulation". The Body layer
row in the Look card is the place the inspector shows them. Copying or pasting a look never changes a shape's outline.

### The path value

`Value::Path(ShapePath)` in `bpe-core`, never keyframed:

```rust
pub struct ShapePath {
    /// Corners in shape units: the even polygon's corners lie on the unit circle; Size multiplies.
    pub points: Vec<[f32; 2]>,
    /// One per side, the side from point i to point i+1 (the last closes to point 0):
    /// None = straight, Some([c1, c2]) = a cubic Bezier with those two bend handles (shape units).
    pub bends: Vec<Option<[[f32; 2]; 2]>>,
}
```

Valid when it has at least 3 points, `bends.len() == points.len()` and every number is finite. Shape units are
the shape's own frame: x right, y the same way as world y, origin at the shape's middle, not turned. Scene file
format goes from 6 to 7.

- Even polygon of n sides: corner k at angle `-90° + k·360°/n` from +x on the unit circle (a corner at the top,
  the same corners as the Arena's polygon, which a test pins).
- Turn into points: 8 corners on the unit circle starting at the top; each side a cubic whose handles are tangent
  at the corners with length `4/3·tan(45°/4) ≈ 0.2652` (so it traces the circle to within 0.01%).
- Create curve: handles at 1/3 and 2/3 along the side, pushed outward (away from the shape's middle side of the line)
  by 0.25 × the side's length, so the curve is visible at once.
- Straighten: the side's bend becomes None.
- Add point: a new corner at the point on the side nearest the right-click. On a curved side the curve is split at
  that t (de Casteljau) so the outline does not change.
- Delete point (only with more than 3): the new side keeps the outer handles of the two sides it replaces if
  either was curved (the previous side's c1 and the next side's c2; a straight one contributes its end point),
  else it is straight.
- Reset (Body layer button) and any change to Sides or Start from remove `path`.

## Physics

### Walls

`bpe-runtime/src/walls.rs`:

- `Wall` gains `solid: bool` (filled) and stops being `Copy` (it stays `Clone`).
- `Outline` keeps `Ring` and `Box`, loses `Rounded` (nothing makes one now), and gains
  `Path { center, angle, scale, points: Arc<[Vec2]>, thickness }`: the outline flattened once in shape units
  (curves as 16 equal-t segments each), placed by centre, angle and scale each step without re-flattening.
- The shape sim picks the exact form when it can, so old physics stays bit-for-bit:
  circle → `Ring { sides: 0 }`; even polygon with no path → `Ring { sides }` with the Arena's angle;
  a path of 4 straight sides that is an axis-aligned rectangle in shape units → `Box`; anything else → `Path`.
- `keep_inside` becomes `keep_clear(position, previous, velocity, radius, wall) -> Option<Contact>`. A ball's side
  of the wall is the side its centre was on at the start of the step (`Bodies.previous`, set before the move), so a
  fast ball cannot tunnel through a thin band:
  - Hollow, previous centre inside: exactly today's inside maths for Ring and Box (goldens unchanged, including
    "a ball too big for the wall sits at the centre"); for Path, keep the centre at least `radius + thickness/2`
    inside the centre line.
  - Hollow, previous centre outside: keep it at least `radius + thickness/2` outside the centre line.
  - Solid: keep it at least `radius` outside the outline, whichever side it came from (a ball that starts inside a
    solid shape is pushed out of the nearest edge).
  - Contact normal points from the wall into the ball's side; `wall_velocity` as today (spin, growth, riding).
- Inside/outside of a Path is even-odd over the flattened points, so a self-crossing outline still works.
- Hits log as today: `Party { thing: wall_id, item: 0 }`.

Known limit, accepted: a shape that shrinks or spins faster than a ball can follow may sweep past it in one step;
the ball then counts as being on the other side.

### The sim (`bpe-kit/src/things/shape/sim.rs`)

The Arena sim's turning, shrinking, riding and keyframe handling, unchanged in meaning, with the outline built from
`ShapeSettings` as above. Keyframed Size and Thickness rebuild the outline around the same middle in
`settings_changed`. Shrink's floor uses Size.

## Drawing

### Part 1: plain white

A new GPU pipeline in `bpe-gfx` draws shapes: one `PathInstance` per shape (centre, angle, scale, kind circle or
path, solid or hollow, half thickness, colour, thing code, and the range of its points in a shared storage buffer of
flattened points). The fragment shader works out the distance to the outline (exact for a circle, nearest segment
otherwise) and inside/outside (even-odd), and covers the fill or the band with one-pixel soft edges at any zoom.
Shapes draw where the Arena's wall drew (before the balls) and honour the thing's blend like other things.

### Part 2: the paint layers

The Look card of a Shape is `look.shape`, a stack of layers like the balls' `look.layers`:

- **Body** (base, always at the bottom, no settings of its own: its row shows the shape settings).
- **Colour and gradient** (repeats): one colour, or a gradient that runs **Across** (with an angle), **From the
  middle** (radial) or **Along the edge** (around the outline); Opacity; Blend.
- **Outline** (repeats): a line along the edge (both edges of a hollow band), Width, colour (same as body or own).
- **Glow** (repeats): soft light outside the edges, Amount, Size, colour.
- **Blend** (one): how the whole finished shape mixes with what is behind it.
- **Shader** (repeats): an effect painted over the layers under it, with own code, like the ball's Shader layer.

The part 2 pipeline turns the path shader into a layered program the way balls' `layers.wgsl` is, with the
distance-to-edge, position-along-edge and inside/outside the layers need. Part 2 writes its own plan from this spec.

## The editor

### Inspector (layout A)

- Header: name and kind, as for every thing. A "Shape settings" fold shows Spin, Shrink, Smallest size.
- Look card: the Look editor's Layers list, **without the ball view and without a Reactions list**. Body is the
  bottom row (no eye, no menu actions that would move, hide, copy or delete it).
- Body picked: under the list, Start from, Sides, Size, Fill, Thickness, then a readout ("6 corners", "7 points,
  1 curve") with a **Reset** button (enabled only when there is a path), then the hint "Drag corners and sides in the
  video to mould it." Changing Sides or Start from also removes the path in the same undo step.
- Part 1 lists only the Body; "+ Add layer" offers what is registered for `look.shape` (nothing in part 1, so it is
  greyed with the hover text "More layers are coming").

### Scene view (preview)

When a Shape is selected:

- Its outline is traced in its track colour.
- Corners: white squares with a track-colour border. Drag = move that corner.
- Sides: a track-colour dot at each side's middle; grabbing anywhere along a side (within the edge grab distance)
  drags the side = moves both its corners by the same amount.
- Bend handles (curved sides only): two hollow diamonds joined to their corners by dashed lines. Drag = move it.
- Circle: a dot on the right of the edge; dragging it or anywhere on the edge sets Size to the pointer's distance.
- Elsewhere on the shape (inside a filled shape, on a hollow band): drag = move the whole shape, as Body drags do.
- **Shift** while dragging a corner, side or bend handle also moves its mirror partner across the shape's vertical
  middle line. The partner is the other point (or handle) nearest the mirror image of the dragged one when the drag
  starts. A dragged point that sits on the line (|x| < 0.5% of Size) stays on the line.
- Right-click opens the menus above (on a side, a corner, or a circle's edge). Each menu action and each drag is one
  undo step.
- Drags snap to the grid like other drags (Alt places freely). Corner positions are kept in shape units.
- First drag on an even polygon or a menu action creates the `path` from its corners.

An unselected shape: clicking its edge (or a filled shape's inside) selects it and drags it.

Hint line: "Drag things to move them · drag the arrow tip to aim · drag a shape's corners and sides to mould it,
Shift mirrors · right-click a side for more".

## Removed

The `arena` thing kind (`things/arena/`), `ArenaKind`, `ArenaSettings`, `ArenaShape`, `ArenaSim`, `ArenaDraw`, the
preview's `Handle::Edge` resize, `Outline::Rounded`. `look.basic` stays (test scenes still use it on balls through
`classic_cards`) but fits no thing kind.

## Tests and goldens

- Test scenes switch from the arena to a Shape at the same `ThingId(1)`: circles, even polygons and boxes give the
  same walls, so every golden line stays the same **except "Ball pit"**, whose capsule becomes a 700 × 1400 hollow
  rectangle (its line is re-blessed, debug and release).
- New unit tests: path geometry (even polygon corners match the Arena's, Turn into points traces the circle, Add
  point on a curve keeps the outline, Delete point), wall forms picked (circle/polygon/box/path), two-sided collision
  (inside a hollow path, outside a hollow ring, solid from outside, starting inside a solid), no tunnelling through a
  thin band at speed, keyframed Size, save/load of `Value::Path`, GPU pictures of a filled and a hollow path shape,
  the inspector's Body row, Reset, Sides removing the path, preview drags (corner, side, bend, radius, Shift mirror)
  and every right-click action.

## Not in scope

Rounded corners, holes in a shape, per-corner keyframes (points are not keyframed), shapes as part of a ball group,
importing outlines from files.
