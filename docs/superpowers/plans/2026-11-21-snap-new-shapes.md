# Snap new shapes to the grid (2026-11-21)

Aedan: "Make it so that when adding a new shape or changing the thing's shape, it automatically snaps its points to
the grid (when possible, and when grid snap is enabled)."

## Design decisions

1. **When.** Only while the toolbar's **Snap** is on, and not while Alt is held (Alt is "free" everywhere else).
   It happens at two moments:
   - a **new Shape** is added from the tree's Add menu (plain or with a ready look);
   - a shape's **Start from** or **Sides** is changed in the inspector (the two settings that already start the
     outline afresh, `inspector::edits::shape_afresh`). It happens in the same edit, so one undo takes back both.
   Nothing else re-snaps: Size, Turn, Placement, dragging corners (already snapped by the preview drag), undo/redo,
   loading or opening a built-in scene, pasting or duplicating.
2. **A new shape's middle** lands on the nearest grid point (after `place_added` centres it in the view). An
   existing shape whose Start from or Sides changes is **not moved**: only its outline is fitted where it stands.
3. **Even polygon** (Start from Polygon, no moulded corners): each corner (as the world places it, through the
   shape's middle, Placement rotation and Size) moves to its nearest grid point, and the result is kept as the
   shape's own corners (`path`, shape units at the same Size). **Only where possible**: if two corners land on the
   same grid point, or the snapped outline is no longer a proper convex outline turning the same way (a corner
   collapses into a straight line or folds over), nothing is snapped and the even polygon stays as it was. This
   happens for small shapes with many sides on a big grid. The same is skipped when Size is keyframed (the corners
   would scale off the grid at once).
4. **Circle** (no corners to snap): its Size (radius) rounds to whole grid squares (at least one), so with its middle
   on a grid point its top, bottom, left and right touch grid lines. Skipped when Size is keyframed.
5. **Ellipse**: Width and Height round so each half is a whole number of squares (at least one), so its four ends
   reach grid lines when it is not turned. Each skipped when keyframed. Turn is left alone.
6. **Riding shapes** (riding on another thing) are skipped: their position is relative to the thing they ride, so
   the grid can't be known from the scene alone.
7. A shape stuck to the screen snaps like any other (its place is measured from the video's middle, as the grid is).

## Code

- New `crates/bpe-studio/src/shape_snap.rs`:
  - `pub fn fit(scene: &mut Scene, id: ThingId, step: f32) -> bool`: points 3 to 5 above for shape `id` (a no-op
    for any other kind, a rider, or a non-positive / non-finite step). True when it changed the scene.
  - `pub fn place_new(scene: &mut Scene, id: ThingId, step: f32) -> bool`: snaps a new shape's middle
    (`transform.position`, `grid::snap_point`) then calls `fit`.
  - `fn snapped_corners(s: &ShapeSettings, centre: Vec2, turn: f32, step: f32) -> Option<ShapePath>`: the even
    polygon's corners (`ShapePath::even(sides)`), to the world (`centre + rotate(turn) * p * size`), snapped, back to
    shape units; `None` when two coincide or any turn's cross product is not strictly the original sign (convexity
    and winding kept). The path's bends are all `None`.
- `inspector/edits.rs::apply` gains `grid: Option<f32>`; in `Edit::SetOwn`, after `shape_afresh`, a key of
  `"sides"` or `"start"` calls `shape_snap::fit` with it. `inspector/mod.rs` passes
  `setting_snap::now(ui).map(|s| s.length as f32)` (None when Snap is off or Alt is held).
- `app/panels.rs`: where `tree.added` is placed, `shape_snap::place_new` with `self.grid.step(alt)`
  (alt read from the context's modifiers).

## Not doing

- No snapping of a polygon's Size, of curves (bends), or of a moulded outline (Turn into points etc.).
- No change to the kit, runtime or goldens: this is editor-only; built-in scenes and exports are untouched.

## Verification

- Unit tests in `shape_snap.rs`: a hexagon of Size 400 at a grid point snaps every corner onto 60 px grid points and
  keeps 6 corners; a 64-sided polygon of Size 100 on a 60 grid is left alone; a circle's Size 400 → 420; an
  ellipse 900 × 1300 → 960 × 1320; a rider and a keyframed Size are left alone; the world (`shape_handles::seen`)
  places the snapped corners on grid points (checks the shape-units conversion against the runtime).
- Inspector test: with the grid snapping, setting Sides to 4 in the inspector gives a moulded path on grid points;
  without snap the path stays empty (existing test `changing_sides_starts_afresh...` still passes).
- App test: adding a Shape from the tree with Snap on puts its middle on a grid point.
- `cargo test -p bpe-studio`, clippy, fmt. CI runs the rest.
