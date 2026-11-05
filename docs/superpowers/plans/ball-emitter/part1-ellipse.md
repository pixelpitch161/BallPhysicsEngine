# Ball Emitter part 1: Ellipse shape — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native, per the project's rules:
> one Opus review at the end of the part, no approval stops). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Shape can start from an exact **Ellipse** (Width, Height, Turn) that balls bounce off exactly, that draws
exactly, whose two foci are known (and marked in the editor), plus a Bounce setting **Touch walls with: Edge / Middle**
so balls fired from one focus meet exactly at the other.

**Architecture:** A new `Outline::Ellipse` in `bpe-runtime/src/walls.rs` with an exact nearest point (Eberly's
bisection in f64) goes through the generic two-sided `keep_on_side` pass. `ShapeSettings` gains `start=ellipse` with
`width`/`height`/`turn`; `base_outline` makes the new outline (a `Ring` when width equals height). The GPU draws it as
a third `PathInstance` kind (ellipse, semi-axes in `b.zw`) with an iterative nearest point in WGSL, so every paint
layer follows the true curve. `bpe-kit/src/things/shape/foci.rs` reads foci off the wall as it stands.

**Tech Stack:** Rust, glam, wgpu/WGSL, egui 0.37.

**Spec:** `docs/superpowers/specs/2026-11-05-ball-emitter-design.md` §4 (and §9 "Things not to do").

## Global Constraints

- `ShapeStart` gains `Ellipse` after Polygon and Circle; Circle stays the default.
- `width`: "Width", px, span(20, 2000), range(1, 20000), default 900, simple, keyframe, group Body, `when = "start=ellipse"`.
- `height`: "Height", px, same ranges, default 1300, simple, keyframe, group Body, `when = "start=ellipse"`.
- `turn`: "Turn", °, span(-180, 180), default 0, keyframe, group Body, `when = "start=ellipse"`. Clockwise.
- `size` gets `when = "start!=ellipse"`. Shrink scales both semi-axes by the same factor as a circle's radius; Spin turns on top of Turn.
- Changing Start from removes the path in the same edit (already true: `inspector::shape_afresh` on key `start`).
- Physics: closest point by Eberly's bisection, f64 inside, at most 64 iterations; exact for a=b and on-axis points. Normal = gradient of the implicit function at the closest point. Two-sided like `Ring`.
- `base_outline` → `Outline::Ellipse` for `start=ellipse`, `Ring` when width == height.
- Touch walls with: new Bounce setting "Edge" (default, today's behaviour) or "Middle"; Middle uses radius 0 for that ball's wall test (and its centre bounces on the outline's centre line). Pairs still use the real radius.
- `foci(..) -> Option<[Vec2; 2]>`: centre ± c·(cos θ, sin θ) along the long axis, c = sqrt(a² − b²); circle: both its middle; polygons/paths: none. From the shape's current pose.
- Editor draws the two foci of a selected ellipse (and circle) as small `style::TEXT` ring marks.
- Drawing is exact (no polygon or cubic approximation for physics or drawing, §9).
- Turn into points: 16 curved corners (cubic). Shape sound By side: an ellipse is one side.
- Goldens unchanged; one new test scene `Ellipse focus` (test-scenes feature only) adds one golden line in each file.
- No colours made up in editor code. Plain-language UI words. No built-in/example scenes.

## Decisions this plan makes (not in the spec)

1. **Shape unit of an ellipse** = its larger semi-axis, `max(width, height) / 2` (`ShapeSettings::unit()`; for other
   starts it is `size`). Smallest size (`min_size`) and the editor frame use it, so Shrink stops at the same "size"
   as a circle of that radius would.
2. **Middle ignores the band too**: with Middle the ball's centre bounces on the outline's centre line, so a thick
   band doesn't spoil the focus property ("its centre bounces exactly on the outline", spec 4.2).
3. **Middle bounces carry on**: after a Middle ball's real hit, it continues for the rest of the substep from where its
   centre crossed the line along its new velocity (instead of staying on the projected point). Without this the
   bounce point is off by up to one step's travel along the normal (2.5 px at 600 px/s) and the spec's 0.5 px focus
   test can't pass. Edge balls are untouched, so goldens don't move.
4. **Along the edge / Place** on an ellipse are measured by the parametric angle of the nearest point, clockwise
   from the ellipse's own top (GPU `along` and `place_on_edge` agree). "From the middle" gradients on an ellipse run
   by the ellipse's own radius `length(q / axes)` so they follow its shape.
5. **Editor handles**: an ellipse has a Width dot (end of its width axis) and a Height dot (end of its height axis),
   `Handle::Axis(id, 0|1)`, each dragging that full axis to twice the pointer's distance along it; dragging the edge
   (`Handle::Radius`) scales both by the same factor; right-click on the edge: Turn into points. Width/Height/Turn
   keyed → the drag sets a key at the playhead, as Size does.
6. **"No longer an exact ellipse" note**: shown as the Turn into points menu item's hover text ("Makes the ellipse 16
   curved sides you can mould. Balls fired from a focus will no longer meet perfectly.") rather than a lasting
   inspector note, because once it's a path nothing marks it as having been an ellipse. Called out in the PR.
7. Turn is baked into the ellipse's `outline_path()` (and into the Turn into points path), so shape units stay the
   unturned shape frame everywhere (`in_shape_units`, editor `ShapeFrame`).

## Review Focus

1. Width == height (an ellipse that is a circle) and very flat ellipses (1 × 2000): no NaN, balls stay on their side.
   → Task 1 tests `a_circle_ellipse_matches_the_ring` and `a_very_flat_ellipse_keeps_balls_inside`.
2. A ball exactly at the centre or on an axis of a hollow ellipse (Eberly's degenerate branches). → Task 1 `nearest_on_an_axis_is_exact`.
3. Keyframed Width/Height/Turn while running: the wall follows, foci move. → Task 3 `width_keys_resize_the_ellipse_and_move_its_foci`.
4. A Middle ball in a corner/tight spot (carry-on landing on the wrong side) must not escape. → Task 2 `a_middle_ball_never_leaves_a_box`.
5. Shrink and Spin on an ellipse (scale both, turn on top of Turn). → Task 3 `an_ellipse_spins_and_shrinks_like_a_ring`.

---

### Task 1: `Outline::Ellipse` in the runtime

**Files:**
- Modify: `crates/bpe-runtime/src/walls.rs`
- Modify: `crates/bpe-runtime/src/testing.rs` only if a match needs it (it uses `if let`, so likely not)

**Interfaces:**
- Produces: `Outline::Ellipse { center: Vec2, half: Vec2 /* semi-axes before turning */, angle: f32, thickness: f32 }`;
  `pub fn nearest_on_ellipse(half: Vec2, p: Vec2) -> Vec2` (own frame, exact, f64 inside); every `Outline` method
  handles the arm: `point(u)` (ray from the centre at `u` radians clockwise from `angle`), `center`, `thickness`,
  `angle`, `reach` (= larger semi-axis), `corners` (empty), `inside` ((x/a)²+(y/b)² < 1), `nearest_and_line`
  (closest point; line normal = normalised (x/a², y/b²), turned).

- [ ] **Step 1: Tests** (in `walls.rs` tests):
  - `nearest_on_an_ellipse_agrees_with_brute_force`: for half ∈ {(300,150), (150,300), (500,40), (200,199)}, angles
    {0, 0.7}, 200 seeded points inside and outside: |q−p| within 0.05 px of the minimum over 10⁵ samples of
    `center + rot(angle)(a cos t, b sin t)`; q lies on the ellipse (|(x/a)²+(y/b)²−1| < 1e-4 in its frame); normal unit
    and `(p−q)·n ≥ −1e-3`. Add the ellipse to `nearest_agrees_with_brute_force`'s list too.
  - `nearest_on_an_axis_is_exact`: (0,0) → (0, ±b) end of the minor axis at distance b; (x,0) inside beyond the
    evolute (x > a − b²/a) → (a,0); (0, y) → (0, b); points on axes outside → axis ends.
  - `a_circle_ellipse_matches_the_ring`: `nearest_on_ellipse((r,r), p)` equals the Ring's nearest bit for bit
    (`assert_eq!`) for several p, including p = 0.
  - Add the ellipse to `a_ball_that_was_inside_is_pulled_back_in` and `inside_and_point_agree_on_the_outline`.
  - `a_ball_bounces_inside_and_outside_an_ellipse`: hollow (300,150) thickness 4; ball r 5 at (0,148) moving down from
    previous (0,0) → pushed to y = 150−2−5 = 143, normal (0,−1); from outside previous (0,200) at (0,152) → y = 157.
    Solid: ball starting inside pushed out of the nearest edge.
  - `a_very_flat_ellipse_keeps_balls_inside`: half (1000, 1); 1000 seeded random previous-inside balls r 0.3 moved by
    up to 5 px: every result finite and inside.

- [ ] **Step 2:** `cargo test -p bpe-runtime walls` → fails to compile (no variant).

- [ ] **Step 3: Implement.** `nearest_on_ellipse`: a == b → the Ring's exact maths in f32 (centre + normalised p·r,
  `dir(0)` for p = 0 — the caller handles angle). Otherwise fold to the first quadrant with the larger axis first
  (e0 ≥ e1, y0 = |along e0|, y1 = |along e1|), in f64:

  ```text
  if y1 > 0:
    if y0 > 0: z0=y0/e0, z1=y1/e1, g=z0²+z1²−1
               if g == 0 → (y0, y1) else r0=(e0/e1)², s=root(r0,z0,z1,g); x0=r0·y0/(s+r0), x1=y1/(s+1)
    else      → (0, e1)
  else: n0=e0·y0, d0=e0²−e1²
        if n0 < d0 → xd=n0/d0; (e0·xd, e1·sqrt(1−xd²)) else (e0, 0)
  root: n0=r0·z0; s0=z1−1; s1 = g<0 ? 0 : hypot(n0, z1)−1; ≤64 bisections of s=(s0+s1)/2,
        stop when s equals an end; g(s)=(n0/(s+r0))²+(z1/(s+1))²−1 → g>0: s0=s, g<0: s1=s, else stop.
  ```
  Restore signs and the axis swap. `keep_clear_from` sends a hollow-inside Ellipse through `keep_on_side(.., true, ..)`
  like `Path`. In `keep_on_side`, after the reach check, skip work when the Lipschitz bound proves the ball clear:
  `Outline::clear_by(&self, p, limit, inside) -> bool` — for an ellipse, g = sqrt((x/a)²+(y/b)²), `(g<1) == inside &&
  |1−g|·min(a,b) ≥ limit` (g is 1/min(a,b)-Lipschitz); false for the other forms. `keep_inside_exact`'s Ellipse arm
  is unreachable (`None`, commented like `Path`).

- [ ] **Step 4:** `cargo test -p bpe-runtime` → all pass.

- [ ] **Step 5: Commit** "Runtime: exact ellipse outline".

### Task 2: Touch walls with: Edge / Middle

**Files:** `crates/bpe-runtime/src/walls.rs`, `plugins.rs` (ModuleSim), `world.rs` (collide), `lib.rs` (export),
`crates/bpe-kit/src/modules/bounce/{mod.rs,sim.rs}`, `crates/bpe-kit/src/modules/look/layers/particles/sim.rs`
(passes `Touch::Edge`).

**Interfaces:**
- Produces: `pub enum Touch { Edge, Middle }` in walls.rs (exported); `keep_clear(.., radius, touch: Touch, wall)` and
  `keep_clear_from(.., radius, touch: Touch, wall)`; with `Middle` the wall test uses radius 0 and no band (limit 0,
  ring/plane maths with thickness 0); the contact point is the centre.
  `ModuleSim::touch(&self, state) -> Touch { Touch::Edge }`; `BasicBounce.touch: WallTouch` (`#[derive(Choice)] enum
  WallTouch { #[default] Edge, Middle }`), setting `#[setting(label = "Touch walls with", group = "Bounce", when =
  "thing!=shape")]`, doc "Edge: balls bounce when their edge meets a wall. Middle: when their centre does, so they
  bounce exactly on the outline." Placed after `friction`.

- [ ] **Step 1: Tests.** walls.rs `a_middle_ball_touches_with_its_centre`: hollow ring r 100 thickness 10, ball r 20
  at (101,0) from (0,0), Middle → at (100,0), contact point (100,0), normal (−1,0). world.rs (or a kit test)
  `a_middle_ball_carries_on_after_its_bounce`: one ball r 5, no gravity, moving +x at 240 px/s from x = 99.5 inside a
  ring r 100 hollow 4, Middle: after one step (1 px travel, crossing at 0.5) its x is 99.5 (reflected), velocity −240.
  kit `a_middle_ball_never_leaves_a_box` (bounce tests): 50 Middle balls at random speeds ≤ 3000 in a hollow
  rectangle and a hollow ellipse for 10 s: every ball stays inside. Default bounce test: `touch` defaults to Edge.

- [ ] **Step 2:** run → fails to compile.

- [ ] **Step 3: Implement.** `collide`: `let touch = own.map_or(Touch::Edge, |(sim, st)| sim.touch(st));` per mover.
  After `respond`, when `touch == Middle` and it was a real hit (`contact.speed ≥ resting`): `let crossed =
  start.lerp(moved_to, within); let carried = crossed + bodies.velocity[i] * ((1.0 - within) * dt);` keep it only when
  it is on the ball's own side (`wall.solid ? !inside : inside == before.inside(previous)` — re-use the same rule as
  `keep_clear_from`), else leave the pushed position. Edge path byte-for-byte unchanged.

- [ ] **Step 4:** `cargo test -p bpe-runtime -p bpe-kit bounce walls` pass; goldens (`cargo test -p bpe-kit --test golden`) unchanged.

- [ ] **Step 5: Commit** "Bounce: Touch walls with Edge or Middle".

### Task 3: Shape settings, sim and foci

**Files:** `crates/bpe-kit/src/things/shape/{mod.rs,sim.rs,foci.rs (new)}`, `crates/bpe-kit/src/lib.rs` (re-export
`foci`), `crates/bpe-core/src/shape_path.rs` (`ellipse`).

**Interfaces:**
- Consumes: Task 1 `Outline::Ellipse`.
- Produces: `ShapeStart::Ellipse`; `ShapeSettings { width, height, turn }`; `ShapeSettings::ellipse(width, height,
  thickness) -> Self`; `ShapeSettings::unit(&self) -> f32` (max(w,h)/2 for an ellipse, else `size`);
  `outline_path()` for an ellipse = `ShapePath::ellipse(Vec2::new(w, h) / 2 / unit, turn_radians)`;
  `ShapePath::ellipse(half: Vec2, turn: f32) -> ShapePath` (16 corners from the top, each side a cubic with handle
  4/3·tan(TAU/64), the circle's construction mapped through the axis scale and turn; `circle()` stays 8 sides);
  `pub fn foci(world: &World, shape: ThingId) -> Option<[Vec2; 2]>` (first focus is the one at −c along the long
  axis's direction (cos θ, sin θ) where θ is the outline angle, plus a quarter turn when the height is the longer one).

- [ ] **Step 1: Tests.**
  - mod.rs: body-settings test includes `width`, `height`, `turn`; `an_ellipse_outline_path_is_16_curves_through_its_axes`
    (points 0, 4, 8, 12 at top/right/bottom/left of the turned ellipse; every flattened point within 1e-3 units of the true ellipse).
  - sim.rs `each_form_of_outline_is_picked`: ellipse 900×1300 → `Ellipse { half: (450,650), angle: 0 }`; 600×600 →
    `Ring { radius: 300, sides: 0 }`; turn 30 → angle 30° in radians plus the thing's rotation.
  - sim.rs `width_keys_resize_the_ellipse_and_move_its_foci`: keys width 900→500 over 1 s: half.x 450 then 250; foci
    distance from the centre goes from sqrt(650²−450²) to sqrt(650²−250²).
  - sim.rs `an_ellipse_spins_and_shrinks_like_a_ring`: spin 90, shrink 0.5, min_size 325 (= half the unit): after 10 s
    half = (225, 325) and the angle has turned 90°/s.
  - foci.rs: an upright 900×1300 at (0,0) → foci (0, ∓sqrt(650²−450²)); 1300×900 → on x; a circle → both centre;
    a polygon → None; a turned ellipse (turn 30) → foci on the turned long axis; a shape riding a moving parent → foci
    move with it.
  - sim.rs `side_on_and_place_treat_an_ellipse_as_one_side`: side 0 for points all round; `place_on_edge` → (~0 at the
    top, ~0.25 at the right, 0.5 bottom, 0.75 left, side 0, sides 1).
  - **Focus property** (kit test in `foci.rs` or `tests/ellipse_focus.rs`): no gravity, shape ellipse 900×1300 hollow
    thickness 8 at (0,0); balls: group Ring of 360, ring_radius 1, radius 1, at focus 1, launch Outward at 600 px/s,
    Touch walls with Middle, motion drag 0. Step 1/240 s at a time to past the arrival time T = (1300 − 1)/600 s,
    tracking each ball's closest approach to focus 2 on the segment between successive positions: every ball within
    0.5 px, and every ball's closest step within ±1 of the median.

- [ ] **Step 2:** run → fail.

- [ ] **Step 3: Implement.** `base_outline` arm `(ShapeStart::Ellipse, _)`: half = (w/2, h/2), angle = turn + turn_deg.to_radians();
  Ring when w == h. `ShapeState::outline` Ellipse arm scales half by `self.scale`, adds the turn. `floor()` uses `unit()`.
  `in_shape_units` divides by `unit()·scale`. `settings_changed` recomputes `st.path` when (start, sides, path) changed
  (as now; an ellipse doesn't read it for sides). `side_on` returns 0 for `start=ellipse`; `place_on_edge` for an
  ellipse reads the wall (Ellipse or Ring form): nearest point in the outline's own frame → `fract(atan2(x/a, −y/b)/TAU)`,
  side 0, sides 1 — no allocation.

- [ ] **Step 4:** `cargo test -p bpe-kit shape` and `-p bpe-core shape_path` pass.

- [ ] **Step 5: Commit** "Shape: exact ellipse with foci".

### Task 4: Drawing the ellipse exactly

**Files:** `crates/bpe-gfx/src/frame.rs` (`push_ellipse`, `ShapeInstance::outline` arm), `shaders/paths.wgsl`,
`shaders/shape_layers.wgsl`, `crates/bpe-gfx/src/paths_tests.rs`, `crates/bpe-kit/src/things/shape/draw.rs`
(`WallForm.ellipse`), `crates/bpe-kit/src/modules/look/shape/gpu.rs` (`Geometry.ellipse`).

**Interfaces:**
- Produces: `Frame::push_ellipse(center, angle, scale, half_thickness, axes: Vec2 /* shape units, max 1 */, color, code) -> usize`
  writing a `PathInstance` with `b = [half, 2.0 (kind ellipse), axes.x, axes.y]`, `c = [code, 1, 0, 1]`;
  `WallForm { .., ellipse: Option<Vec2> }` and `Geometry { .., ellipse: Option<Vec2> }` (axes in shape units; `extent`
  along dir = ±sqrt((a·dx)²+(b·dy)²)); WGSL `fn ellipse_nearest(q: vec2<f32>, ab: vec2<f32>) -> vec2<f32>` (iterative
  trig-free nearest point, 5 iterations, on |q| with signs restored) in paths.wgsl used by `path_signed` and `shape_at`.

- [ ] **Step 1: Tests.** paths_tests `an_ellipse_draws_on_its_true_outline`: full size frame; a hollow ellipse at
  (0,−400) scale 400, axes (1, 0.5), angle 0.4, half band 1.5; a filled one at (0,500) scale 300, axes (0.6,1). For 64
  points round each true outline: the hollow band's pixel there is bright and 4 px either side along the normal is
  dark (within 1 px of the true line); the filled one bright 2 px inside, dark 2 px outside. Same hollow ellipse
  wearing body + an Outline record (inside line 6 px) draws the line within the band's inner 6 px (shape_at path).
  `shape_layers` along: red-to-blue Along gradient on an ellipse: top reddish, left bluish. draw.rs:
  `an_ellipse_draws_one_exact_shape` (kind 2, axes (450/650, 1), scale 650, no points). Shader problems empty.

- [ ] **Step 2:** run (`cargo test -p bpe-gfx paths`, `-p bpe-kit draw`) → fail.

- [ ] **Step 3: Implement.** `path_signed` / `shape_at`: kind ≥ 1.5 → nearest c = `ellipse_nearest(q, b.zw)`; sd =
  |q−c|·scale, negative when (q/ab)·(q/ab) < 1; along = `fract(atan2(c.x/ab.x, −c.y/ab.y)/TAU + 1)`. `shape_run` case
  1 with no band on an ellipse: `length(s.q / layered_shape.b.zw)`. `wall_form` Ellipse arm: scale = max(half), axes =
  half/scale, turn = angle; `draw_part` calls `push_ellipse` for it.

- [ ] **Step 4:** tests pass, `cargo test -p bpe-kit --test pictures` passes with no shader problems.

- [ ] **Step 5: Commit** "Draw: exact ellipse shapes".

### Task 5: Editor: Ellipse in the Body, handles, foci marks, Turn into points

**Files:** `crates/bpe-studio/src/shape_handles.rs`, `preview.rs` (`Handle::Axis`), `preview/zones.rs`
(`own_frame` arm), `look_editor/layer_list.rs` (`shape_words` → "Ellipse"), others the compiler names.

**Interfaces:**
- Consumes: Task 3 `foci`, `ShapeSettings::unit`, `ShapePath::ellipse`.
- Produces: `Handle::Axis(ThingId, u8)` (0 width, 1 height); `Seen.ellipse: Option<Vec2>` (full width, height as seen);
  `MenuTarget::Circle` covers an ellipse's edge.

- [ ] **Step 1: Tests** (shape_handles tests, an ellipse studio via `studio_with`):
  - `an_ellipse_shows_width_and_height_dots`: hit at the width end → `Axis(id,0)`, height end → `Axis(id,1)`, edge
    elsewhere → `Radius`, middle → `Body`.
  - `dragging_an_axis_dot_sets_width_or_height`: width dot dragged 50 px outward → width 1000; keyed width gets a key
    at the playhead instead.
  - `dragging_an_ellipse_edge_scales_both`: edge drag from 650 to 715 px along the height axis → width 990, height 1430.
  - `turn_into_points_makes_16_curved_sides`: act on an ellipse → start polygon, path = `ShapePath::ellipse(..)` with 16
    sides and curves, size = unit.
  - `foci_marks_are_drawn_for_a_selected_ellipse`: `focus_marks(world, id)` (pure helper used by `draw`) gives 2
    points for an ellipse, 2 equal points for a circle, none for a polygon.
  - layer_list: `shape_words` of an ellipse is "Ellipse".

- [ ] **Step 2:** run → fail.

- [ ] **Step 3: Implement.** `seen`: frame angle = wall angle − turn for an ellipse (and its Ring form), size =
  `unit()·scale`, path = `outline_path()`. `hit_handle`: ellipse → dots at shape points `rot(turn)(±a', 0)`/`(0, −b')`
  (the right and top ends), then edge (|distance to the true outline| ≤ EDGE_GRAB on screen, via the wall's `nearest`)
  → Radius. `apply`: Axis → `2·|(pointer − centre)·axis_dir| / scale`, snapped, min 20, key-aware like `size`; Radius on
  an ellipse → factor = |pointer−centre| / |offset| times the widths stored in `Mould` at start. Menu: Turn into points
  for an ellipse with the decision-6 hover text; `act` sets start polygon, `size` = unit, path = outline_path().
  `draw`: ellipse outline from `wall.outline` points (exact `point(u)` sampled 96 times), the two dots, focus rings
  (radius 4, 1.2 px `style::TEXT`) for a selected ellipse or circle.

- [ ] **Step 4:** `cargo test -p bpe-studio` passes (incl. the design-system colour test).

- [ ] **Step 5: Commit** "Editor: ellipse handles, foci and Turn into points".

### Task 6: Test scene, goldens, docs

**Files:** `crates/bpe-kit/src/scenes.rs` (`ellipse_focus`, appended last in `all`), `tests/golden*.txt` (one new
line each), `CLAUDE.md` (Shape and walls lines), `docs/superpowers/plans/ball-emitter/part1-ellipse.md` (ticks).

- [ ] **Step 1:** `ellipse_focus`: "Ellipse focus", no gravity, duration 20, ellipse 900×1300 thickness 3, a Ring of
  48 balls radius 4 ring_radius 1 at focus 1 (0, −sqrt(650²−450²)), launch Outward 500 px/s, Touch walls with Middle.
- [ ] **Step 2:** `BLESS_GOLDEN=1 cargo test -p bpe-kit --test golden` in debug and `--release`; `git diff` shows only
  the appended "Ellipse focus" line in each file.
- [ ] **Step 3:** `cargo test --workspace` (background), `cargo clippy --workspace`, `cargo fmt`; all green except the
  known release-only debug test.
- [ ] **Step 4:** CLAUDE.md: walls gain `Outline::Ellipse` and Touch; shape gains Ellipse, `unit()`, `foci.rs`;
  paths kind 2.
- [ ] **Step 5: Commit** "Test scene: Ellipse focus"; one Opus review of the branch; fix; PR.
