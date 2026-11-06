# Gradients part 2: shapes — implementation plan

> **For agentic workers:** executed natively (the owner's rules: no approval stops after the design, one Opus
> review at the end of the part). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** one shared list of gradient shapes (`GradientForm`) for every gradient that fills an area, replacing the
per-site lists, with the ten new forms (Diamond, Polygon / star, Spiral, Kaleido sweep, Off-centre round, Waves,
Marble, Free points, Ripples from things, Turbulence) on the GPU and the CPU, Free points dragged in the views,
Ripples' distance picture, and the gradient row's Shape tab.

**Architecture:** the form and its numbers are gradient **companions** (`"<k>~form"`, `"<k>~sides"`, ...) like part
1's Bands and Grain. Part 1's once-a-frame pass (`gradient_extras::apply_moves`) copies them onto the gradient value
as a draw-time `Gradient.layout` (never saved), so every site reads `g.layout` instead of its own enum and
direction. On the GPU every form is a pattern code of `paint.wgsl::paint_colour`, its numbers in two new vec4s of
`PaintGpu`. Ripples read a jump-flood field of the frame's balls and shape outlines, built only when some paint in
the frame uses Ripples.

**Tech stack:** Rust, wgpu/WGSL, egui 0.37 + kittest.

**Spec:** `docs/superpowers/specs/2026-11-06-gradients-design.md` (section 3 and the Shape tab of section 5).

## Global constraints

- Drawing only: physics goldens (`crates/bpe-kit/tests/golden*.txt`) and sound goldens must not change.
- Pure in time: a form at a moment depends only on settings, time and what is drawn this frame (Ripples' field is
  built from this frame's balls and shapes), never on earlier frames.
- No limits: numbers get a `span`; a hard `range` only where a value breaks (Sides ≥ 3, Arms ≥ 1, Mirrors ≥ 1,
  Spacing > 0, Size > 0, Softness > 0, Detail 1..8).
- Every default leaves today's picture: a gradient with no companion stored draws exactly as before part 2 (pinned
  picture tests stay green without re-pinning).
- Editor controls use `bpe-studio/src/style/` tokens and IBM Plex; every control is named for kittest.
- Don't keep `GradientShape`, `PaintRuns`, `LineRuns`, `PlaceShape` or the gradient patterns of
  `inside::Pattern` around.
- Don't add built-in or example scenes. CLAUDE.md's crate map is updated.

## Decisions (made here, from the spec)

1. **Where the form lives.** Companions on the gradient's key. Two kinds of area site (a new `Flags.area`):
   - **Fills** (`#[setting(area)]`): ball layers' and sub-layers' own colours (Outline, Glow, Shine, Shading, sub
     Glow, sub Outline, the shape's sub Glow/Shading), shape paint layers (Colour and gradient, Outline, Glow). They
     lose their `gradient`/`runs` and `direction` fields; the companions `~turn` (default 90, the old Direction
     default everywhere), `~centre` (0,0), `~size` (1) and `~repeat` (Clamp) replace them.
   - **Placed** (`area_placed`): the site keeps its own placing settings and gets only `~form` and the form's own
     numbers. `Paint` (the background, a ball Body following Each ball, balls following Place/Start) keeps
     Direction / Centre / Colours end at / Beyond the last colour; the Pattern layer (`InsidePaint`) keeps Angle /
     Size / Spin / Scroll / Beyond / Turns with the ball.
   `Area` is `No | Fills(GradientForm) | Placed(GradientForm)`, the form being the site's default. Every site's
   default is Straight except the Pattern layer's, Stripes (today's default Pattern layer is shade-mode stripes).
2. **The Pattern layer.** `Pattern` becomes `None, Gradient, Dots, Checker, Halves, Slices, Swirl, Face`
   (default Gradient). Gradient lays its colours out by `inside_colours~form` etc. whether they come from its own
   gradient or a shade of the ball's colour; in shade mode the colours row is hidden, so the editor shows the
   Shape tab's rows under Shade (Task 9). `look.basic` test scenes that use Linear/Radial/Sweep/Stripes/Rings are
   converted by `layers_from_basic` into Gradient + a stored `~form`.
3. **Units.** A form works on a point `u` in its site's units: ball units inside a ball (-1..1, y down), shape units
   in a shape (`s.q`), `(place − centre) / span` for a Paint. Then `q = rotate(u − centre, −turn) · k` where `k` is
   `1/size` for Fills sites and the site's own scale for Placed ones (Pattern: Size; Paint: 1). Straight on a Paint
   keeps today's `dot(d, dir)/span + 0.5` (its span is the whole width); every other form takes span as its radius,
   which is today's Round. Form-own positions (Focus, Free points) are in these q units.
4. **Site measures kept.** Round on an outline runs across its ring and on a glow out from its edge (ball and
   shape), Round on a shape's Colour and gradient runs from the middle (across the band of a hollow shape),
   Straight on a shape runs edge to edge at Turn (today's `p1` extents), and Along the edge on a shape is today's
   `s.along`. These ignore Centre but take Size. Along the edge elsewhere is a sweep from the top, clockwise.
5. **Repeat.** Straight, Round, Sweep, Diamond, Polygon, Spiral, Kaleido, Off-centre, Waves, Marble and Turbulence
   honour Repeat; Stripes, Rings, Along the edge and Ripples mirror on Clamp (as Stripes/Rings do today). A shape
   layer with Flow and Clamp still wraps (today's rule).
6. **Free points.** Stop i sits at `~points[i]` (`Value::Points`, q units); missing points default to
   `0.6·(cos, sin)(−90° + 360°·i/n)`; at most 16 (stops past 16 are left out). Colour = Σ wᵢcᵢ / Σ wᵢ,
   `wᵢ = 1/(dᵢ^softness + 1e-6)`, mixed in OKLab when the gradient mixes Even / Round the wheel / Long way round,
   else in linear light. The points and colours go into an atlas row (`GradientAtlas::points_row`: texel 2i the
   point, 2i+1 the colour), so no new binding. CPU and GPU agree. Adding or removing a stop in the bar adds or
   removes its point when points are stored.
7. **Ripples.** Sources are circles (each ball: centre, radius) and capsules (each shape's outline as segments,
   radius half its band, ≤ 256 segments a shape), each with its owner's centre. A 2-best jump flood keeps, per
   texel, the two nearest sources of distinct owners for each of Everything / Balls / Shapes (`Rgba32Uint` +
   `Rg32Uint`, source index + 1). Paint reads the field at its world point, skips its own owner (a ball's centre,
   a shape's centre), and gets the exact distance to the source; along = distance / Spacing − Flow·time, mirrored on
   Clamp. The field is half the picture's size, built when `Frame.ripples` is set (the frame builder sets it when a
   stored `~form` is ripples). On the CPU (balls following Place) Ripples falls back to Round.
8. **Noise.** Value noise on a PCG-hashed lattice with smoothstep, fBm of `detail` octaves (lacunarity 2, gain
   0.5), output −0.5..0.5; the same code in `bpe-core` and WGSL. Marble: `q.x·0.5 + 0.5 + bend·fbm(1.5q)`.
   Turbulence: domain-warped fBm drifting at `speed`.
9. **Form maths** (q as in 3, angle = `atan2(q.y, q.x)/τ`):
   Straight `q.x/2 + ½`; Round `|q|`; Sweep `fract(angle)·k` (as today); Stripes `q.x/2`, Rings `|q|`;
   Diamond `|q.x| + |q.y|`; Polygon / star: 2n corners from the top alternating radius 1 and
   `cos(π/n)(1 − depth)` (≥ 0.02), `along = dot(q, n̂)/h` of the edge of q's sector (1 on the outline);
   Spiral `fract(angle·round(arms) + |q|·twist)`; Kaleido `1 − |2·fract(angle·round(mirrors)) − 1|`;
   Off-centre: q squashed by Stretch along Stretch turn, then the focal radial from Focus (kept inside 0.99) to the
   unit circle; Waves `q.x/2 + ½ + height/2 · sin(τ(count(q.y/2 + ½) + flow·time))`; Marble, Turbulence as 8.
10. **Shape tab.** Form chips painted as 28×20 previews of the gradient in that form (CPU, cached per gradient and
    form), then Turn, Centre, Size, Beyond (Fills only), then the picked form's own rows. Companion `when` terms
    starting with `~` are relative to the gradient's key (`~form=spiral` on `3/colour` reads `3/colour~form`).
11. **Movement parts.** Waves' and Ripples' Flow and Turbulence's Speed are form settings here (the spec's table);
    spiral turning and marble drift are part 3's "Shape moves".

## Review focus

1. A gradient with no companions on every site draws exactly as before (pinned pictures green): Task 4/6/7 tests.
2. Free points with more than 16 stops, fewer points than stops, and a one-stop gradient: Task 1 tests.
3. Ripples with no balls or shapes in the frame (field empty → along 0, the first colour) and a ball inside a filled
   shape (measures from the other things): Task 5 test.
4. Degenerate numbers (Sides 3 at depth 1, Size 0 typed, Spacing tiny, non-finite) never give NaN: Task 1 property
   test over every form.
5. Changing the form in the editor of a layer whose gradient has one stop shows no Shape tab (part 1 hides tabs):
   Task 9 kittest.

---

### Task 1: the form in core (`bpe-core/src/gradient_form.rs`)

**Files:** create `crates/bpe-core/src/gradient_form.rs`; modify `lib.rs`, `color.rs` (Gradient gains
`layout: Layout`, draw-time like `grain`, not saved, ignored by `sample`), `settings.rs` (`Flags.area: Area`).

**Produces:**
- `enum GradientForm { Straight, Round, Sweep, Stripes, Rings, AlongTheEdge, Diamond, Polygon, Spiral, Kaleido,
  OffCentre, Waves, Marble, FreePoints, Ripples, Turbulence }` (Choice, default Straight), `fn code(self) -> f32`
  (1, 2, 3, 5, 6, 4, 13..22 in that order of the new ones).
- `enum Repeat { Clamp, Repeat, Mirror }` moved here from `bpe-kit::paint` (re-exported there), `code()`.
- `enum RippleFrom { Everything, Balls, Shapes }`.
- `enum Area { No, Fills(GradientForm), Placed(GradientForm) }` with `is_area()`, `fills()`, `default_form()`.
- `struct Layout { form: Option<GradientForm>, turn: Option<f32>, centre: Option<[f32; 2]>, size: Option<f32>,
  repeat: Option<Repeat>, sides, star, arms, twist, mirrors, focus: [f32; 2], stretch, stretch_turn, count, height,
  flow, detail, bend, seed, softness, points: Vec<[f32; 2]>, spacing, from: RippleFrom, speed }` with the
  companions' defaults; `Layout::form_or(default)`.
- `fn along(form, layout: &Layout, q: Vec2, k: f32, time: f32) -> f32` (every along-form; Ripples → Round;
  FreePoints → 0), `fn free_colour(g: &Gradient, layout, q) -> [f32; 4]` (linear), `fn noise(q, seed)`,
  `fn fbm(q, octaves, seed)`, `fn free_points(g, layout) -> Vec<([f32;2], [f32;4])>` (≤ 16).
- `Flags.area` defaults to `Area::No`; every `Flags { .. }` literal gains `..Flags::default()` or `area`.

- [ ] Tests: each form's along at chosen points (Straight 0/0.5/1 across, Diamond 1 on the corner (1,0), Polygon
  1 on the outline at every corner and edge middle for n = 3, 5, 6 and star depth 0 and 0.5, Spiral seamless across
  the angle wrap for integer arms, Kaleido symmetric, Off-centre 0 at Focus and 1 on the unit circle, Waves equal to
  Straight at height 0); noise in −0.5..0.5 and equal for equal inputs; free points: the colour at a point is its
  stop's, 17 stops use 16, missing points get defaults; no NaN over a grid of q × degenerate numbers for every form.
- [ ] Run `cargo test -p bpe-core gradient_form` — fail, implement, pass. Commit.

### Task 2: shape companions and the pass (`gradient_extras.rs`, derive)

**Files:** modify `crates/bpe-core/src/gradient_extras.rs`, `crates/bpe-settings-derive/src/lib.rs`.

- Companions (tab Shape): `form` (Choice, keyframe off), `turn` (deg, default 90, span −180..180), `centre` (Vec2),
  `size` (default 1, range (1e-3, ∞), span 0.1..4), `repeat` (Choice) — these five offered on Fills only, except
  `form` on both; then `sides` (5, range 3.., span 3..12, when `~form=polygon`), `star` (0, span 0..1), `arms` (1,
  range 1.., span 1..8), `twist` (1, span −4..4), `mirrors` (6, range 1.., span 1..16), `focus` (Vec2 (−0.35,
  −0.35)), `stretch` (1, range (0.01, ∞), span 0.25..4), `stretch_turn` (0 deg), `count` (3, span 0..12), `height`
  (0.15, span 0..1), `flow` (0, span −2..2, waves|ripples), `detail` (3, range 1..8), `bend` (0.5, span 0..2),
  `seed` (0, span 0..100, marble|turbulence), `softness` (2, range (0.1, 64), span 0.5..8), `points` (Points,
  never keyframed), `spacing` (60 px, range (0.5, ∞), span 4..400), `from` (Choice RippleFrom), `speed` (0.2, span
  0..2). Numbers keyframe and pulse like Bands.
- `def_for(base, c)`: None unless the base is a gradient and the companion is offered by its `Area`; `form` answers
  the per-default-form def (`FORM_DEFS`, one per form).
- `companions_for(base)` filters the same way.
- `apply_in` collects every companion of a base into a `Layout` and sets `g.layout` on the base and its `~to`
  (alongside part 1's bands and grain). `pub fn any_ripples(resolved) -> bool`.
- Derive: `#[setting(area)]`, `#[setting(area = "stripes")]`, `#[setting(area_placed)]`.

- [ ] Tests: Shape companions offered on a Fills gradient and not on a plain one; Placed offers `form` and the own
  numbers but not turn/centre/size/repeat; the Pattern-like default reads Stripes; the pass builds the layout from
  stored keys and leaves unstored ones `None`; `any_ripples`. Commit.

### Task 3: GPU paint forms (`bpe-gfx`)

**Files:** `gradients.rs` (`PaintGpu.form: [f32; 4]`, `PaintGpu.place: [f32; 4]` = centre xy, spare; `points_row`),
`shaders/paint.wgsl`, `frame.rs` (`LayerGpu::ZERO`), `shaders/layers.wgsl` (`own_colour`), `shape_walk.wgsl`,
`shape_layers.wgsl` (`shape_colour` on pattern codes, `shape_run` gone), `scene.wgsl` + `lib.rs` Globals
(`bg_form`, `bg_shape`: code, free row), `BackgroundPaint.form/shape`.

- `paint.wgsl`: `fn paint_q(p, local) -> vec2` (centre, turn, scale), `fn paint_form_along(p, local, time) -> f32`
  (a switch on the code, the maths of decision 9), noise/fbm, `fn paint_free(p, q) -> vec4` (reads the row with
  `textureLoad`), `paint_colour` rewritten as one switch (masks unchanged); `paint_from*` fill the new fields.
- `own_colour(l, p, along, plain, site_round)`: code 2 on outline/glow kinds uses `along·scale`, code 4 a sweep from
  the top, free points `paint_free`, else `paint_colour`.

- [ ] Tests (`paint_tests.rs`, GPU): a ball record per new form against the CPU `along` at sampled pixels (within
  2/255); a default record draws the same bytes as before (hash pinned before the change). Commit.

### Task 4: ball and shape sites (`bpe-kit`)

**Files:** `modules/look/layers/{mod,outline,glow,shine,shading,subs,pattern,gpu}.rs`, `modules/look/shape/{layers,
subs,gpu}.rs`, `paint/{inside,gpu}.rs`, `modules/look/basic.rs` (+ `layers_from_basic`), `scenes*.rs`, tests.

- Remove `GradientShape`, `PaintRuns`, `LineRuns`; colour fields get `area`; `direction`/`gradient`/`runs` go.
- `paint::gpu::form_gpu(g: &Gradient, default: GradientForm, place: Placing, atlas) -> PaintGpu` shared by every
  site (`Placing { turn, centre, k, repeat, scroll }` from the layout or the Placed site's own settings).
- Pattern: `Pattern::Gradient` + masks; `inside_gpu` lays Gradient out by the form (default Stripes).
- Shape records: `p1` extents at the layout's turn; outline/glow `p0.z` gone.

- [ ] Tests: records of each site at defaults equal the pre-change records (pattern, angle 90, scale 1, repeat 0);
  a stored `~form=spiral` reaches `paint.pattern`; layers_from_basic converts Rings to Gradient + `~form=rings`. All
  pinned picture tests still pass. Commit.

### Task 5: Ripples' field

**Files:** create `crates/bpe-gfx/src/ripples.rs`, `shaders/ripples.wgsl`; modify `frame.rs` (`RippleSource`,
`Frame.ripple_sources`, `Frame.ripples`), `lib.rs` (paint layout bindings 2 field A, 3 field B, 4 sources storage
whose element 0 is the world→field mapping; built before the scene draws), `paint.wgsl` (`paint_world`, `paint_own`
private vars, `paint_ripple_distance(from)`), `layers.wgsl` (world varying, own = centre), `paths.wgsl`,
`scene.wgsl`; kit: balls and shapes push sources when `frame.ripples`; `frame.rs::build_frame_sensing` sets
`frame.ripples` from `gradient_extras::any_ripples`.

- [ ] Tests: no ripples form → no field built (`Renderer::ripples_built()` false); a background with Ripples and one
  ball draws rings (the colour at 1.5 spacings out matches the gradient at 0.5 mirrored) and a ball with Ripples
  inside a filled shape reads its distance from the shape's edge, not 0. Commit.

### Task 6: Paint sites (background, Each ball, Place)

**Files:** `paint/{mod,eval,gpu}.rs`, `things/scene/draw.rs`, `modules/look/layers/gpu.rs` (body),
`tests/background_pictures.rs`.

- `Paint.shape` goes; `colours` is `area_placed` when its follows include place, start or each_ball; Direction shows
  for each_ball|place|start, Centre for place|start.
- `eval::offset` for Place/Start uses `gradient_form::along` (Ripples → Round, Free points → its colour in
  `colour_at`).
- `background_gpu` hands the form (mode 3 = any form other than Straight/Round, which keep modes 1/2).

- [ ] Tests: `a_screen_place_in_the_world_paints_what_offset_gives` for every along-form; a background picture per
  form (hash pinned) and the old round/straight backgrounds unchanged. Commit.

### Task 7: picture tests per form

`crates/bpe-kit/tests/gradient_form_pictures.rs`: for every form a ball layer, a shape Colour and gradient layer
and the background, drawn at 270×480, pinned as tile averages (like `lit_flat_pinned.rs`) and checked to differ
from Straight. Commit.

### Task 8: Shape tab (`bpe-studio`)

**Files:** `gradient_tabs.rs` (Shape tab: `form_chips`, companion rows with relative `when` rewritten in
`companion_def`, defaults from the def), `style/swatch.rs` or a new `gradient_form_chips.rs` (CPU previews),
`widgets.rs` (`get` falls back to the companion def's default).

- [ ] Kittest: the Shape tab shows on an outline's gradient and not on a Recolour's; picking "Spiral" sets
  `colour~form`; Spiral shows Arms and Twist and not Sides; Turn edits `~turn`; a Paint gradient shows the chips but
  no Turn. Commit.

### Task 9: Free points dragging, points follow stops, shade-mode Pattern

**Files:** `look_editor/handles.rs` (ball view dots for the picked layer's gradient when its form is Free points),
`preview.rs` / new `preview/free_points.rs` (dots for a selected shape's layers and the background), `widgets.rs`
(stop added/removed → points), `look_editor` layer grid (Pattern in shade mode shows the Shape rows).

- [ ] Kittest/unit: dragging a dot writes `~points` with the full list; removing stop 1 of 3 with stored points
  removes point 1. Commit.

### Task 10: docs, lint, review, PR

- CLAUDE.md "Gradients" section: part 2 as built. This plan's "As built" notes.
- `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (background), goldens unchanged.
- One Opus review of the branch; fix; PR.

## As built

- Every area gradient takes one form list (`GradientForm`, 16 forms) through its `~form` companion; `Area` on the
  setting says whether it is placed by its own Turn/Centre/Size/Beyond (`Fills`) or by its paint (`Placed`). The old
  per-site choices (Paint's Gradient, Pattern's Stripes/Rings/Linear, the layers' `GradientShape`) were removed, not
  upgraded: there is no backward compatibility for scene files yet. The Pattern's Gradient defaults to Stripes.
- `apply_moves` fills `Gradient.layout` once a frame; code that reads a resolved scene without it (unit tests that
  build stacks straight from `resolve`) must call it to see companions. A Paint writes its colours without the layout
  (`group_write`), so scenes round-trip.
- The shaders keep every costly paint function at one call site: Marble and Turbulence share one `paint_fbm` call,
  and the Pattern's colour goes through the same `own_colour` call as the other layers. Without that the in-ball
  program tests ran out of memory compiling on llvmpipe.
- Ripples: a half-size field of source indices (Rgba32Uint every/balls, Rg32Uint shapes), seeded by instanced quads
  with write masks and spread by a 2-best jump flood keeping distinct owners. With no source it reads 0 (the first
  colour). The CPU (`area_colour`, the chips) draws Ripples as rings from the middle.
- A shade-mode Pattern (Inside colours: shade of own colour) hides the Colours row, so it has a plain "Shape" choice
  row under the same `in_colours~form` key; the form's own numbers are only reachable with its own gradient.
- Free point dots: the ball view for the picked layer, and the preview for a selected shape's Look layers and for
  the background (on the screen unless "Background stays on screen" is off). Dots are not shown for a paint measured
  on the screen inside a ball's paint (`space` = screen).
- Picture tests: `bpe-kit/tests/gradient_form_pictures.rs` checks every form on the background against the CPU's
  `area_colour` at a grid of points, and every form inside a ball and on a shape against Straight, rather than
  pinning tile averages (that holds on any GPU).
