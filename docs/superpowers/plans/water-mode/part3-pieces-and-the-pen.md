# Water mode part 3: pieces and the pen (plan)

> **For agentic workers:** built natively in one thread (Aedan's project rule: one Opus review per part, no
> approval stops after the design). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aedan drops ready-made pieces and pen-drawn walls into a Water scene's course; they snap, turn, flip
and scale, can cut tunnels out of the rock, each wears its own Material (its Wetting in the water, its Colour in the
plain look), and the draft's water runs over and through them at once.

**Architecture:** the kit turns every enabled Piece and Wall into `bpe_water::Solid`s in world pixels
(`modules/water/pieces.rs`); the draft's setup lays them over the carved rock in scene order, adding or cutting,
into the one `SolidField` the physics and the drawing read (`SolidField::add_part`, which also records each node's
owner and Wetting). The editor's Piece and Pen tools and the Select tool's handles edit the cards and the
thing's place; the draft restarts through `WaterSetup::print` as for carving.

**Tech Stack:** Rust, wgpu/WGSL (`bpe-water`), egui 0.37 + kittest (`bpe-studio`).

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` (sections 2, 4 "Liquid first", 5 "Trouble
marks", 7, Parts 3). Hand-over: `/mnt/project-files/ballphysics/water-mode/part3-handover.md` (items 0 to 6).

## Global Constraints

- Balls scenes byte-for-byte unchanged: no Balls golden or pinned picture moves.
- Editor colours only from `style/` tokens; every control named for kittest (`labelled_by`); no `ComboBox`.
- `bpe-kit` refuses platform maths: `det_sin_cos`, `det_atan2` and the rest of `bpe_core::maths::Maths`.
- Windows-friendly; the owner never runs commands; no exe built or installed.
- No sound or text in Water mode. No demo videos or screenshots unless asked; the full-size look check is for what
  is drawn (the plain look's solid colours).
- Picture tests through `bpe_gfx::test_device()`; pinned pictures only through "Refresh GPU pictures".
- Test only what this part adds or changes; CI runs the rest.

## Item 0: why Wetting reached less at 3 px than at 6 px (found before planning)

Measured on the software card with `tests/scratch_wet.rs` (removed before the pull request), the 2c engine:

| Blob, Wetting 1 | 6 px | 3 px |
| --- | --- | --- |
| 24 px, full gravity, 0.5 to 6 s | a one-drop film at 0.5 s (117 x 3 px) | a cap, 54 x 12.6 px, 50°, the same from 1 s to 6 s |
| 24 px, full gravity, Surface pull's bend off (`curv` 0) | film | film (249 x 1.5) |
| 24 px, full gravity, 16 or 32 steps a frame, no landing loss, no drag, no deep part | | 50° every time |
| 12 / 48 px, full gravity | 80° / 9° | 60° / 13° |
| 36 px, a tenth of gravity (the edge's true angle), Wetting 0 / 0.5 / 1 | 117° / 100° / 71° | 115° / 99° / 74° |

1. **The edge angle is the same at both details** once gravity does not flatten the blob (a tenth of it, so the
   cap's shape is the contact angle's): within 3° at every Wetting. Adhesion does not reach less at finer cells.
2. **What 2c measured** was a 24 px blob, 8 drops across at 6 px. Under full gravity it slumps, and a one-drop film
   on a wet face is a dead end at that detail: every drop sees the face below and its row beside it, so the skin's
   bend term (`bend`, the share `curv`) has nothing to work on, and nothing pulls the film back up. At 3 px the
   same blob is 16 drops across and settles as the cap its true angle makes. So the 6 px film was the coarse
   detail's, and the 3 px cap the truth. (The tilted plate's film share also counted "two spacings deep", which is
   6 px at 6 px and 3 px at 3 px.)
3. **Wetting 1 was never a film**: about 72° at every detail, where the card promises "water spreads over it in a
   film". The cause is the bend term. A wetted face is liquid to the skin's pull (`S`) and to the way the liquid
   lies (`L`), but not to the bend, and the bend gives the water's own surface more pull than its pull between
   drops alone; with the bend off, Wetting 1 spreads (12 to 21° and still going at 2 s) and Wetting 0 drops from
   116° to 78°. So a soaked face does not match the water's own surface, and the edge stands up.
4. **The fix** (Task 1) makes a wetted face count the bend's share too, measured so the edge follows Young's law
   (cos θ = 2 × Wetting − 1, where the model reaches) at 3 px and 2 px under weak gravity, and keeps Wetting 0's
   beads as they are. The candidates and their numbers are in "As built and measured", decision 1.

## Decisions (my calls; cheap to change before they are built)

1. **Wetting's fix** (item 0): see Task 1; whichever of the two candidates measures closest to Young's law at 3 px
   and 2 px with Wetting 0 unchanged: (a) the face's pull counted `wet_pull` times, (b) the face as resting liquid
   to the bend too (its lattice's drops counted in the bend sum as neighbours with no surface of their own).
2. **Order and Cuts.** Rock first (the Course's carving), then each enabled Piece and Wall in `scene.order` (the
   tree's order). One that adds lays its parts' solid over what is there (`min` of distances); one that cuts
   takes its **hull** away from everything before it (`max(d, −d_hull)`); a later one adds back over a cut.
3. **A cut's faces wear the cutting thing's Material** (its Wetting now, its Colour in the plain look): a glass
   pipe cut through rock is lined with glass. A node's owner is the part whose face is nearest; inside a part that
   adds, the part's (later ones are drawn on top).
4. **What a piece is.** Each `PieceKind` is parts in its own frame (its box `Size`, centred on its place, x mirrored
   by Flip, turned by the thing's Rotation): filled polygons for blocks, thick lines for thin walls (so they are
   thickened for the physics), and a hull for cutting. Table in Task 3. Moving kinds (Wheel, Bucket, Gate, Float)
   are fixed in part 3 (part 5 moves them); a Piece whose Moves is not Fixed stays put and says so.
5. **A piece is a path.** A Custom piece's Outline (`ShapePath`, shape units ±1 times half its Size) is edited on
   the stage. A ready-made piece of one closed outline (Ramp, Step, Pillar, Basin, Bucket, Gate, Float) has
   **Make custom** in the strip: it becomes a Custom piece with that outline, one undo step. Funnel, Pipe, Sieve
   and Wheel are several parts and keep their kind.
6. **Thin parts.** Lines (pen walls, a piece's thin walls) are at least `LEAST_WALL` (1.5) cells thick for the
   physics and the draft's drawing, so a wetted face always has a reach of solid behind it and the wet table's
   half-plane holds (hand-over item 3). A Piece or Wall with a part under two draft cells thick gets the trouble
   note `THIN` when picked. Filled blocks are not thickened (their tips may leak at draft, as thin rock does).
7. **Pieces' own settings.** The Shape card gains **Thickness** (a hollow piece's walls, a Wheel's paddles, a
   Sieve's bars; px, default 16) and **Opening** (a Funnel's gap and a Sieve's gaps; px, default 40). Each kind
   placed from the palette gets its own Size (table in Task 3).
8. **The field stays cheap.** A part changes only the nodes within `BAND` (48 px) of its box; past that the field
   keeps its distance (only its sign matters there). Measured in Task 4 (a one-screen course, 40 pieces, at Fine).
9. **Air under pieces** (hand-over item 4): a piece's or wall's underside is the course's, as rock's is: water
   poured over an upturned cup fills it; no diving bell in part 3. Air already trapped and risen under a submerged
   piece stays while it is deep (2c's rule). Written into the notes and handed to part 5 (buckets, floats).
10. **Snapping.** Placing or dragging a piece snaps it so a corner of its hull meets another solid's corner (or a
    wall's point) within 10 screen points; failing that, its place snaps to the grid (as now). A pen point snaps to
    another solid's corner or point within 10 screen points, else to the grid. Alt turns all snapping off (the
    grid's rule). The turn handle snaps to 15° steps unless Alt is held.
11. **The plain look shows each solid in its Material's Colour.** The field's second half on the card holds each
    node's owner; the drawing mixes the four nodes' colours (the Rock's for owner 0, so a field with no parts draws
    exactly as before and the pinned pictures stay). At most 64 parts get their own colour (`MOST_COLOURED`); past
    that they draw in the Rock's.
12. **Tools.** Piece: a palette in the strip (an icon of each kind's own outline, named for kittest), Turn (°),
    Flip, Cuts; a ghost follows the pointer; a click places it (one undo step), picks it and keeps the tool.
    Pen: Thickness, Curved, Round | Square, Cuts; clicks add points with a rubber band to the pointer; a
    double-click or Enter finishes (two points at least), Escape or another tool drops it; the wall's place is its
    points' middle. Select: a piece or wall is picked by its body as well as its marker and dragged with snapping;
    a picked one shows its outline, a picked wall or Custom piece its points (drag to move, double-click the line
    to add one, double-click a point to remove it, keeping at least two on a wall and three on a piece), and a
    picked piece a turn dot above it and a size dot at its box's corner.
13. **While a piece is dragged or a point moved** the draft is held (as for any drag) and the stage's picture shows
    the newest setup's rock over the water still playing (carving's preview, generalised); the draft starts over
    when it is let go.

## Review Focus

1. **A cut piece reordered in the tree** (above or below a piece it overlaps): the field must follow the order, and
   the draft must restart. Test: `order_decides_what_a_cut_takes` (Task 4).
2. **A degenerate shape**: a Size of 0 or negative, a wall of one point or all its points on one spot, a Custom
   outline of fewer than three points, NaN in a point. Nothing may panic, and such a part adds nothing. Test:
   `broken_shapes_add_nothing` (Task 3).
3. **A piece over a tap's spout**: the tap must say it can't pour (in a solid) and pour nothing. Test:
   `a_tap_inside_a_piece_pours_nothing` (Task 4).
4. **Undo across tools**: a piece placed, a wall finished, a point dragged and a Make custom are each exactly one
   undo step, and an Escape mid-pen leaves no step. Tests in Task 6 and 7.
5. **A piece far past the course's edge or huge** (Size 1e6): the field build is bounded by the layout and stays
   fast. Test: `a_huge_piece_is_kept_to_the_layout` (Task 4).

---

### Task 1: Wetting reaches the same at every detail, and Wetting 1 is a film (item 0)

**Files:** Modify `crates/bpe-water/src/drops/numbers.rs`, `src/drops/mod.rs`, `src/drops/shaders/finish.wgsl`
(only for candidate b), `src/scenes.rs`; Test `crates/bpe-water/tests/wetting.rs`, `tests/shots.rs` (`wetting`).

**Interfaces:** Produces `scenes::cap(cell: f32, side: f32, wetting: f32) -> Scene` (a `blob` under a tenth of the
gravity, so its edge shows the true angle) and the chosen number in `Numbers` (`wet_pull: f32` or
`wet_bend: f32`), with `from_env_for` and its doc saying what was measured.

- [ ] Step 1: tests in `tests/wetting.rs`:
  - `the_edge_angle_is_the_same_at_every_detail`: `cap(cell, 36.0, 0.75)` at 6 and 3 px after 2 s; angle
    `2 atan(2h / w)` from `probe::puddle`; `|a6 − a3| ≤ 8°` and both in 45..80°.
  - `a_soaked_floor_spreads_a_blob_into_a_film_at_every_detail`: `blob(cell, 24.0, 1.0)` at 6 and 3 px after
    3 s: height under two spacings at each (2c's failing case at 3 px: 12.6 px).
  - Keep `a_blob_spreads_on_a_wet_floor_and_stands_up_on_a_waxy_one` and `wetting_reaches_only_water_near_a_face`.
- [ ] Step 2: run them on the 2c engine: the film test fails at 3 px; the angle test passes (item 0, point 1).
- [ ] Step 3: build both candidates behind numbers, measure `cap` at Wetting 0, 0.25, 0.5, 0.75, 1 at 6, 3 and 2 px,
  keep the one closer to Young's law at 3 and 2 px with Wetting 0's angle unchanged; the other comes out. Its
  default goes in `Numbers::default()`; `wet_table`'s unit test checks the lattice sum with the number at 1 and the
  default's scaling.
- [ ] Step 4: `wetting` in `tests/shots.rs` measures caps under a tenth of gravity (36 px) beside the 2c blobs.
- [ ] Step 5: the course's shelves and openings at 6 px (software card) before and after: `DEFAULT_WETTING` stays 1
  if the shelves' tallest puddle is still at most 7 px with nothing standing; any change of more than 5 points in
  the openings' shares goes in "As built" with its reason.
- [ ] Step 6: `cargo test -p bpe-water --test wetting --test engine --test air` and the lib tests pass.
- [ ] Step 7: commit "bpe-water: a wetted face counts the skin's bend too, so Wetting 1 is a film at every detail".

### Task 2: lines with square ends, parts laid over the field, owners (`bpe-water`)

**Files:** Modify `crates/bpe-water/src/solid.rs`, `src/lib.rs` (exports).

**Interfaces:** Produces
- `Solid::Polyline { points: Vec<Vec2>, thickness: f32, square: bool }`: a thick line through `points`, round
  joins, round or square (SVG "square": half the thickness past the end) ends, at least `least` thick.
- `Solid::bounds(&self, least: f32) -> Option<(Vec2, Vec2)>` (None for a solid with no finite points).
- `pub struct Part { pub solids: Vec<Solid>, pub cuts: bool, pub wetting: f32 }`.
- `SolidField.owner: Vec<u16>` (0 the field it was built as; every constructor fills 0).
- `SolidField::add_part(&mut self, part: &Part, owner: u16)` with `pub const BAND: f32 = 48.0` (pixels).

- [ ] Step 1: unit tests in `solid.rs`: `a_polyline_is_round_or_square_at_its_ends` (distances at the ends, the
  join and the middle; square end flat half a thickness past the point), `a_part_adds_its_solid_and_its_wetting`
  (a block part at Wetting 0 over an open field: inside negative, owner 1 there and at open nodes nearer it than any
  other face, Wetting 0 there and the base's elsewhere), `a_cutting_part_opens_the_solid_and_lines_it` (a cut
  through a filled block: open inside, the cut's face owner and Wetting the cutter's), `a_part_touches_only_its_band`
  (nodes past `BAND` of its box unchanged, bit for bit), `a_part_with_nothing_in_it_changes_nothing`.
- [ ] Step 2: implement; `polygon`'s even-odd test stays; a part's distance at a node is the min over its solids;
  NaN or empty solids are skipped; the wetting clamped as `with_wetting` does.
- [ ] Step 3: `cargo test -p bpe-water --lib solid` passes; commit "bpe-water: thick lines, and parts laid over the
  solid field".

### Task 3: what each piece and wall is (`bpe-kit modules/water/pieces.rs`)

**Files:** Create `crates/bpe-kit/src/modules/water/pieces.rs`; Modify `solids.rs` (Thickness, Opening, Cuts'
words), `modules/water/mod.rs` (exports).

**Interfaces:** Produces
- `pub const PALETTE: [PieceKind; 12]` (the strip's order: Ramp, Step, Pillar, Basin, Funnel, Pipe, Sieve, Bucket,
  Wheel, Gate, Float, Custom) and `pub fn default_size(kind: PieceKind) -> Vec2`.
- `PieceShape::parts(&self) -> Vec<Solid>` and `PieceShape::hull(&self) -> Vec<Solid>` in the piece's own frame;
  `PieceShape::outline(&self) -> Option<ShapePath>` (one closed outline in shape units, for Make custom; None for
  Funnel, Pipe, Sieve, Wheel); `PieceShape::thinnest(&self) -> f32` (its thinnest part, px).
- `WallLine::solid(&self) -> Option<Solid>` (a `Polyline`; Curved runs a centripetal Catmull–Rom through the
  points, 8 pieces a span) and `WallLine::path(&self) -> Vec<Vec2>` (the line drawn).
- `pub fn placed(solid: &Solid, at: Vec2, turn: f32) -> Solid` (turned clockwise by `turn` radians, by
  `det_sin_cos`, then moved to `at`).

Kinds (w, h the Size; t Thickness; g Opening; y down, the box centred on the place):

| Kind | Size | Parts | Hull |
| --- | --- | --- | --- |
| Ramp | 300 x 150 | filled triangle (−w/2, h/2), (w/2, h/2), (w/2, −h/2) | the same |
| Step | 300 x 200 | filled staircase of three steps rising to the right | the same |
| Pillar | 60 x 300 | filled rectangle | the same |
| Basin | 300 x 160 | a U line (square ends at the top edge), thickness t | its box |
| Funnel | 300 x 200 | two lines from the top corners to (±g/2, h/2), thickness t | trapezoid round both |
| Pipe | 400 x 80 | two lines along its length at y = ±(h/2 − t/2), thickness t | its box |
| Sieve | 320 x 20 | bars t wide and h tall with gaps g, spread evenly across w | its box |
| Bucket | 160 x 180 | a U line narrowing to 0.7 w at the bottom, thickness t | its trapezoid |
| Wheel | 200 x 200 | a filled hub (0.15 of the smaller side) and 8 paddle lines to its rim, thickness t | its circle |
| Gate | 20 x 200 | filled rectangle | the same |
| Float | 160 x 80 | filled rounded rectangle (corners a quarter of the smaller side) | the same |
| Custom | 300 x 200 | its Outline flattened (`ShapePath::flatten(FLATTEN)`), each point times half the Size | the same |

Flip mirrors x before the turn. Thickness and Opening are shown only for the kinds that use them (`when`).

- [ ] Step 1: tests: `every_piece_fills_its_box` (each kind's parts lie within its box plus half a thickness, and
  reach its four sides but for Funnel's and Sieve's gaps), `flip_mirrors_a_piece`, `the_hull_holds_the_parts`,
  `a_piece_turns_about_its_place` (`placed` at 90°), `a_curved_wall_passes_through_its_points`,
  `make_custom_keeps_the_outline` (`outline()` × half the Size gives the parts' polygon), `broken_shapes_add_nothing`
  (Size 0 or NaN, a one-point wall, a two-point Custom outline: `parts()` empty or `solid()` None, no panic).
- [ ] Step 2: implement; commit "bpe-kit: the pieces and walls as solids".

### Task 4: the course is rock, pieces and walls (`bpe-kit things/water/course.rs`, `draft.rs`)

**Files:** Create `crates/bpe-kit/src/things/water/course.rs`; Modify `things/water/draft.rs`, `things/water/mod.rs`,
`draft/tests.rs`.

**Interfaces:** Consumes Task 2 and 3. Produces
- `pub struct SolidSetup { pub id: ThingId, pub kind: &'static str, pub at: Vec2, pub turn: f32, form: Keyed,
  material: Keyed }` with `part(&self) -> Option<Part>` (world solids, Cuts, Wetting at 0 s), `outline(&self) ->
  Vec<Vec<Vec2>>` (world outlines to draw: polygons closed, lines as their centre path), `corners(&self) ->
  Vec<Vec2>` (snap points: the hull's corners, a wall's points), `thinnest(&self) -> f32`, `distance(&self, p) -> f32`.
- `WaterSetup.solids: Vec<SolidSetup>` (enabled Pieces and Walls in `scene.order`), `WaterSetup::field_with(&self,
  mask: &CarveMask) -> SolidField` (`field()` = `field_with(&self.mask())`), `solid_at(&self, p: Vec2, reach: f32)
  -> Option<ThingId>` (the top one), `snap_points(&self, except: Option<ThingId>) -> Vec<Vec2>`.
- Notes: `THIN` ("Thinner than two draft cells: the draft makes it thicker for the water, so the final will be a
  little different.") for a Piece or Wall whose thinnest part is under `2 × cell`; `MOVES_LATER` ("Moving pieces
  arrive in a later part of Water mode: this one stays where it is for now.") for Moves not Fixed. `IN_ROCK`
  becomes "This tap's spout is inside a solid: carve the rock away round it, or move it, to pour."
- `print` adds, per solid: id, kind, place and turn (bits), the whole Form card and its keyframes, and the
  Material's `wetting` (and its keys). A Piece's or Wall's colour, Moves, and the rest of its Material do not
  restart.
- `plain_look` fills `PlainLook.solids` with each solid's Material Colour at `time`, in `solids` order.

- [ ] Step 1: tests in `draft/tests.rs`: `a_piece_adds_solid_and_its_own_wetting` (a Wetting-0 block in Wetting-1
  rock: the field's Wetting 0 at its face, 1 at the rock's), `a_cutting_pipe_opens_a_tunnel_lined_with_its_material`,
  `order_decides_what_a_cut_takes`, `a_tap_inside_a_piece_pours_nothing`, `a_huge_piece_is_kept_to_the_layout`
  (Size 1e6 builds in under a second), `thin_and_moving_pieces_say_so`, `a_piece_is_picked_by_its_body`,
  `snap_points_leave_the_piece_itself_out`, `the_plain_look_colours_each_solid`, and `what_restarts_the_draft`
  flipped: Form cards all restart, Material's `wetting` on Course, Piece and Wall, moving and turning a Piece or
  Wall, turning one off; colour and Moves do not. Remove the "not yet" comment.
- [ ] Step 2: `a_field_with_forty_pieces_builds_quickly`: one screen at Fine (6 px), carved, with 40 pieces of every
  kind: `field()` under 150 ms in release, printed (run once by hand in release; asserted under 2 s in debug).
- [ ] Step 3: implement; `cargo test -p bpe-kit things::water modules::water`; commit "bpe-kit: the course is rock,
  pieces and walls".

### Task 5: each solid drawn in its own colour (`bpe-water draw/plain.rs`, `shaders/plain.wgsl`)

**Files:** Modify `crates/bpe-water/src/draw/plain.rs`, `src/shaders/plain.wgsl`; Test `tests/plain.rs`.

**Interfaces:** Produces `PlainLook.solids: Vec<[f32; 3]>` (sRGB, owner k ≥ 1 is `solids[k − 1]`) and
`pub const MOST_COLOURED: usize = 64`. The field buffer is the distances then the owners (as floats); `PlainU`
gains `solids: [[f32; 4]; 64]`.

- [ ] Step 1: test `each_solid_is_drawn_in_its_colour` (a field with a block part owner 1 and `solids` red: a pixel
  well inside it red, a pixel inside the rock the rock's colour, the back the back's); `plain_pinned` unchanged.
- [ ] Step 2: implement (the rock's colour at a pixel mixes the four nodes' owners' colours bilinearly; owner 0 and
  owners past the palette are the Rock's). `cargo test -p bpe-water --test plain --test plain_pinned`.
- [ ] Step 3: full-size look check of a course with pieces (`LOOK_ONLY` is not needed: a by-hand still from
  `tests/shots.rs` at 1080 x 1920 with three coloured pieces over rock, looked at with its 1:1 crops for seams and
  jagged edges). Commit "bpe-water: the plain look draws each solid in its own colour".

### Task 6: the Piece tool (`bpe-studio water_mode/piece_tool.rs`)

**Files:** Create `crates/bpe-studio/src/water_mode/piece_tool.rs`, `water_mode/solid_tests.rs`; Modify `rail.rs`
(Piece and Pen ready, hints), `stage.rs`, `mod.rs`.

**Interfaces:** Produces `PieceBrush { kind: PieceKind, turn_deg: f32, flip: bool, cuts: bool }` (in
`WaterStage.piece`), `piece_tool::strip(ui, studio)`, `piece_tool::place(ui, response, studio, view)`,
`piece_tool::ghost(painter, studio, view, at)`, `snap_piece(setup, shape, at, turn, reach) -> Vec2` (the place
after snapping to other solids, else `None` and the grid's), `make_custom(studio, id) -> bool`.

- [ ] Step 1: tests: `the_palette_places_each_kind_with_its_size` (pick "Funnel" in the strip, click: one Piece,
  kind Funnel, Size `default_size(Funnel)`, at the snapped point, picked, the tool still Piece, one undo step),
  `a_piece_snaps_to_another_ones_corner` (a ramp placed 4 world px off a pillar's corner lands on it; with Alt it
  does not), `turn_flip_and_cuts_go_on_the_piece`, `make_custom_turns_a_ramp_into_its_outline` (one undo step;
  Funnel has no Make custom), and the rail's ready list (only Camera path greyed).
- [ ] Step 2: implement; commit "bpe-studio: the Piece tool".

### Task 7: the Pen tool (`water_mode/pen_tool.rs`)

**Files:** Create `crates/bpe-studio/src/water_mode/pen_tool.rs`; Modify `stage.rs`; Test `solid_tests.rs`.

**Interfaces:** Produces `PenBrush { thickness: f32, curved: bool, square: bool, cuts: bool }` and
`PenLine { points: Vec<Vec2> }` (in `WaterStage.pen`, `WaterStage.line`), `pen_tool::strip`, `pen_tool::draw`
(clicks, double-click, Enter, Escape), `pen_tool::finish(studio) -> Option<ThingId>`.

- [ ] Step 1: tests: `three_clicks_and_enter_make_a_wall` (one Wall, three points relative to its place at their
  middle, the strip's thickness, one undo step, the tool still Pen), `a_double_click_finishes_a_wall`,
  `escape_drops_the_line_and_leaves_no_undo_step`, `a_pen_point_snaps_to_a_pieces_corner`,
  `one_point_makes_no_wall`.
- [ ] Step 2: implement; commit "bpe-studio: the Pen tool".

### Task 8: Select moves, turns, sizes and edits points (`water_mode/solid_edit.rs`, `picture.rs`)

**Files:** Create `crates/bpe-studio/src/water_mode/solid_edit.rs`; Modify `stage.rs`, `picture.rs` (`Rock`'s
preview is any field: `preview: Option<SolidField>` built only when its version changed), `carve_tool.rs` (its
preview is `setup.field_with(painting mask)`); Test `solid_tests.rs`.

**Interfaces:** Produces `solid_edit::pick(studio, view, at) -> Option<ThingId>` (marker first, then body),
`solid_edit::handles(painter, studio, view)`, `solid_edit::drag(ui, response, studio, view) -> bool` (true when
it took the drag: a point, the turn dot, the size dot, or the body).

- [ ] Step 1: tests: `a_click_on_a_pieces_body_picks_it`, `dragging_a_piece_moves_it_with_snapping` (one undo step),
  `dragging_a_walls_point_moves_only_that_point`, `double_clicks_add_and_remove_points` (a wall keeps two at
  least; a Custom piece three), `the_turn_dot_turns_in_fifteen_degree_steps`, `the_size_dot_resizes_in_the_pieces_frame`,
  `a_picked_thin_wall_says_so` (the strip shows `THIN`).
- [ ] Step 2: implement; `app/water_tests.rs`: `a_piece_edit_restarts_the_draft_and_a_colour_edit_does_not`.
- [ ] Step 3: `cargo test -p bpe-studio water_mode app::water_tests`; clippy; commit "bpe-studio: picking, moving,
  turning and moulding pieces and walls".

### Task 9: notes, review, pull request, merge, hand-over

- [ ] `docs/notes/water.md` "Part 3: pieces and the pen (as built)"; this plan's "As built and measured"; the spec's
  Parts line 3 says built; `CLAUDE.md` unchanged (no new area).
- [ ] One Opus review of the branch; fixes; `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D
  warnings`, the touched crates' tests.
- [ ] Draft pull request, assigned and review requested; ready when green; merge when every check is green.
- [ ] Hand-over for part 8 (Make final and export, with slow motion) in
  `/mnt/project-files/ballphysics/water-mode/part8-handover.md`; project memory `water-3`; message the coordinator.

## Things not to do

- Don't touch `bpe-gfx/src/liquid_sim/` or the old Liquid thing. Don't change a Balls scene's behaviour.
- Don't thicken filled blocks by guessing their thickness, and don't clamp Size in the card (a small piece is the
  owner's to make; it gets `THIN`).
- Don't step Keyframed or Driven pieces (part 5), or start the final (part 8).
- Don't make air under pieces (decision 9) or change 2c's air rules.
- Don't let a colour edit or a Moves edit restart the draft.
- Don't loosen a test bound to pass; a moved number goes in "As built" with its reason.

## As built and measured

(Filled in as the tasks land.)
