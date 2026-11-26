# Water mode part 2b: water runs in a Water scene (plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a Water scene's stage plays real water live at draft detail: the Course is carved out of solid rock with a
brush, a Liquid's Feel and Look are read, a Pour pours a tap or fills a box at the start, all drawn in the plain look,
in place of part 1's empty stand-in. Also: time a full screen of draft water on the owner's RX 9070 XT, and make the
physics faster only if that draft cannot play live.

**Architecture:** `bpe-kit` turns a Water scene into the engine's inputs (`water::draft::WaterSetup`: the carved
course as a signed-distance mask, the liquids' `Feel`s, the pours, the keyframed physics read once a frame, and a
fingerprint of everything the physics reads). `bpe-studio` runs the draft on its own thread on the window's graphics
device (`water_mode::draft::DraftJob`), following the playhead with restart copies every 2 s, and the stage draws the
body's drops with the plain drawing straight from the card into an egui texture. The Carve tool paints strokes on the
Course's new **Carving** card, one undo step a stroke, with the rock redrawn live while it paints.

**Tech stack:** Rust, wgpu compute (the all-drops engine `bpe_water::Water`), the plain drawing
(`bpe_water::draw::Plain`), egui 0.37 / egui-wgpu, kittest.

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` (sections 1, 2, 5, 7, 8, 9 and Parts, 2b, as updated
by the water checkpoint, PR #237) and `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md` (sections
3, 8, 9, 10, 11). Notes: `docs/notes/water.md` (parts 1, C1, C2, C3).

## Global Constraints

- Balls scenes do not change: no physics golden, pinned picture or sound may move. `cargo test -p bpe-kit --test
  golden` passes untouched.
- No sound and no text in Water mode (Aedan, 2026-11-24). Water is judged against real water (Aedan, 2026-11-25).
- `bpe-core`, `bpe-runtime`, `bpe-kit` and `bpe-water` never call platform maths (`sin`, `exp`, `powf`, `atan2`,
  `mul_add`, `hypot`...): use `bpe_core::maths::Maths` (`det_*`). `sqrt`, `abs`, `min`, `max` are fine.
- Editor colours only from `style/` tokens. Every control named for kittest (`.labelled_by`, `widget_info`). No
  `ComboBox`: pickers are `menu_button`s or toggle rows.
- The editor's device is made with wgpu's default limits: at most 8 storage buffers a stage, 128 MiB a binding.
- Draft cap 500,000 drops (water spec section 3, Detail). Draft cells 12 / 8 (default) / 6 px.
- A water frame is 1/60 s (`bpe_water::drops::numbers::FRAME`); the playhead's frame is `floor(time × 60)`.
- Windows-friendly code (cross-built with MinGW). Aedan never runs commands; never build or install the exe.
- Test only what this part adds or changes; CI covers the rest. No demo videos or screenshots unless asked.
- Speed numbers told to Aedan are measured on the RX 9070 XT; anything else is called an estimate.

## Decisions

Lines marked **(My call)** are choices Aedan was not asked about; they are cheap to change.

1. **Carving lives on the Course's new Carving card** (`water.carve`, `CarveCard { carved: Strokes }`, slot
   `SlotId::Form`, label "Carving"; the Course's cards become Carving then Rock). The spec says carving is saved as its
   brush strokes (`Value::Strokes`); a card keeps the Course a thing with no settings of its own, like every Water
   kind, and the inspector's Strokes row already counts strokes and has Clear. **(My call)**
2. **A carve stroke is a `bpe_core::Stroke` in world pixels** (the course centred on the world's middle): `size` is the
   brush's radius, `softness` is 1 − Hardness, `erase` means Fill (rock put back), `strength` 1 and `density` 1 unused,
   and a new field `square: bool` (serde default false, left out of the file and of `hash64` when false, so every
   saved stroke and every cached picture keyed by a hash stays as it is) for the square tip.
3. **The mask** (`CarveMask`, spec section 2: "4 px per sample at 1080 wide with the edge stored as a distance"): a
   grid over the course, one sample every 4 world pixels, holding the signed distance to the rock's face (above 0
   open, below 0 rock), clamped to ±48 px (`BAND`: the engine reads the field only within a reach, about two cells, of
   a face; 48 px is four cells at the coarsest draft). Nothing carved is all rock; past the course's edge is rock.
   Strokes apply in order (constructive solid geometry on distances): a stroke's own shape `g` (above 0 inside it) is
   the round brush swept along its path (`radius − distance to the path`) or, with `square`, the axis-aligned square
   of side `2 × radius` swept along each segment (a convex hexagon: its exact signed distance). Dig: `f = smax(f, g,
   k)`; Fill: `f = smin(f, −g, k)`, with `k = softness × radius` (polynomial smooth max, `k = 0` the plain max). So
   Hardness 100 % gives crisp corners where strokes meet and lower Hardness rounds them off, as rock worn by water.
   **(My call: what Hardness means for rock)** A stroke touches only the samples within `radius + k + BAND` of its
   path, which keeps a rebuild of hundreds of strokes well under a second.
4. **The physics' field** at a detail: `SolidField::from_distance(Layout::cover(−size/2, size/2, cell), |p, _| mask
   .at(p))` (bilinear). Thin rock is not thickened: what the physics feels and what the drawing shows come from the
   same field (spec section 2), so a ridge thinner than about 1.5 draft cells is drawn and felt the same way, and at
   draft a fast drop may cross it (said in the notes; trouble marks are Later).
5. **Liquids** are numbered in the scene's order, enabled ones only, at most 16 (`MOST_LIQUIDS`); a 17th is not poured
   and the stage says so. A Feel card maps one to one: `Feel { gravity: 1, surface_pull, thickness, stickiness, weight,
   settles: 0 }`. The plain drawing reads each Liquid's Colour, Deep colour and Clarity by the drop's liquid number, so
   drops are born with the engine's own colour (`bpe_water::WATER`); part 4's mixing gives them their Liquid's colour
   and puts it in the fingerprint (Decision 9) then.
6. **Pours.** A **Tap** is `bpe_water::Pour { place: the Pour's own place, aim, width: Rate × Width, speed, spread,
   from: Starts at, until: Stops at, kind: liquid | STOPS, colour }` (all-drops spec section 8: "half the Rate is a
   stream half as wide"). **Fill at the start** is `fill_stacked` over the box `place ± Fill size / 2` with `rest_off =
   Numbers::wall_off`, kind `liquid | STOPS`; a drop of a later fill within 0.35 cell of an earlier fill's drop is left
   out, so overlapping boxes never double the water **(My call)**. **Rain, Drip and Burst pour nothing in 2b** (part
   4); the stage's strip says so while one is picked. A Pour's Liquid: `Target::Thing(id)` of a Liquid is that one,
   `Target::Kind("water_liquid")` and `Target::Anything` the first Liquid, `Target::Tag(t)` the first Liquid with that
   tag, anything else none (it pours nothing and the strip says "This pour has no Liquid to pour"). A disabled Pour
   pours nothing. Drops keep the `STOPS` bit: water stays in the course (drains are part 4).
7. **Placing a Pour in a scene with no Liquid also adds one** named "Water", in the same undo step, so the first pour
   pours something and its look is one click away in the tree **(My call)**.
8. **Keyframes** on physics settings (Water: Gravity; Feel: all four; Pour: Rate, Width, Speed, Aim, Spread) are read
   once a frame at the frame's start, `k / 60` s (all-drops spec section 3, "(My call)" there). The setup keeps those
   settings' tracks; `WaterSetup::inputs_at(frame)` gives the frame's gravity, feels and pours. `set_pours` keeps each
   pour's progress by its place in the list, so the list's order never changes within a run.
9. **What restarts the draft** (`WaterSetup::print`, a hash of everything the physics reads): the Water card's
   Gravity, Screens wide and tall and Draft detail (and their tracks); the Carving; which Liquids there are, their
   order, whether they are on and their Feel (and tracks); every Pour's card (and tracks), its place, whether it is
   on and which Liquid it pours. **Not**: the Starting look, Final detail, the Light and Back cards, the Rock, a
   Liquid's Look, Mixing and Heat, Drains, Forces, Lamps, Pieces and Walls (none acts on the water before parts 3, 4,
   4b and 6), the camera, names, colours in the tree, the scene's Length. As for the balls, a held slider or a drag
   restarts the draft when it is let go or rests (`hold_runs`, `RUNS_REST`).
10. **The draft job runs on its own thread on the window's device**, not a second device **(My call; the spec says
    "its own thread and device queue")**: the plain drawing reads the drops' buffers straight from the card, and a
    second device would mean reading a stage-sized picture back and uploading it again every frame. The thread steps
    whole frames, each under the body's lock; the stage draws under `try_lock` and so never waits for a frame (it
    takes the lock outright only when its picture is over 250 ms old, so a draft catching up still shows progress).
    The GPU's queue is shared with the window, so a frame's physics and the window's drawing take turns on the card.
11. **Following the playhead.** The job's target is the playhead's frame. Ahead of the body, it steps frame by frame;
    behind, it goes back to the newest restart copy at or before the target (or starts over) and steps on. A copy is
    kept at every 120th frame (2 s) the body reaches for the first time; their memory is estimated as `count × 128 +
    nodes × 64` bytes each, and when copies pass 1 GiB every other one is let go **(My call)**. A copy repeats exactly
    (all-drops spec section 11), so going back never changes the water. While playing, the playhead runs on the wall
    clock and never waits for the draft; the picture shows the newest frame stepped, and the stage says "Working out
    the water…" when it is more than 0.25 s behind.
12. **The draft cap:** once a body holds 500,000 drops its pours are stopped for the rest of the run (`set_pours(&[])`)
    and the stage says "The draft holds at most 500,000 drops: the pours have stopped." (The job takes the cap as a
    number so tests can use a small one.)
13. **The stage picture**: drawn on the UI thread with the live body's `Plain` at the course view's size in real pixels
    (`rect × pixels_per_point`, at most 2048 a side, never above twice the course's own size), registered with egui as
    a native texture (the default `Rgba8Unorm` view, as the ball preview's), drawn under the screen lines, the camera's
    shot and the markers. It is drawn again only when the body's frame, its generation, the picture's size, the look or
    the field changed. With no graphics device (tests) the course shows its flat ground as in part 1.
14. **The plain look from the cards** (`water::draft::plain_look(scene, resolved, reg, time)`): each Liquid's Look
    Colour, Deep colour and Clarity at the playhead (keyframes read there); the Course's Rock Colour; the Back: Darker
    rock (rock × (1 − Darker)), Colour, Gradient (its colour half way along) **(My call)**, Picture (darker rock until
    part 6 draws pictures).
15. **The Carve tool** (rail: Carve becomes ready). Strip: **Dig | Fill**, **Size** (the brush's width, 8 to 600 px,
    default 120), **Hardness** (0 to 100 %, default 100), **Round | Square**, **Straight** (a toggle; holding Shift
    does the same), **Smooth** (a toggle). Alt held while painting does the other of Dig and Fill. The brush is the
    editor's, not saved (as Grade mode's brushes) **(My call: the defaults)**. A press starts a stroke; points are added
    when at least `max(2, radius / 4)` world px from the last; Straight keeps two points (the press and the pointer);
    Smooth is a lazy brush (the brush follows the pointer on a string of `radius / 2`). Letting go puts the stroke on
    the Carving card as one undo step (`separate_edit`); Escape drops it. While painting the stage redraws the rock
    from a working copy of the mask (the stroke's segments added as they come) through the live `Plain`'s field, with
    the old water still playing; the draft starts over when the stroke is let go. A Course with no Carving card (a part
    1 file) gets one at the first stroke. The brush's outline follows the pointer over the course.
16. **The stage's words**: a course with nothing carved says in its middle "The course is solid rock. Pick Carve and
    paint to dig it out." **(My call)** The status line adds "· Water: 52,300 drops (draft 8 px)" while a draft runs,
    and "· Water draft stopped" in the problem colour if its thread stopped.
17. **Markers**: a Pour's marker also shows its aim (a line of 40 screen points from the marker) and, for a Tap, its
    stream's width across it; a Fill at the start shows its box. **(My call)**
18. **Timing** (spec Parts 2b; coordinator's brief): a full screen at draft detail is one screen carved as one chamber
    (24 px of rock round it), two thirds filled at the start, with a Tap of Width 40, Rate 1, Speed 300 pouring into it:
    about 86,000 drops at 8 px. **It plays live** when a frame's physics and one stage-sized drawing (600 × 1067) take
    at most 12 ms together on the RX 9070 XT, leaving 4.7 ms of the 16.7 ms frame to the window. It is timed at 12,
    8 and 6 px. Only if 8 px misses that mark is the physics made faster, in a task planned then from what the
    measuring shows.

## Review Focus

1. **Scrubbing back and forth while it plays** must never show water from a different run or crash: the job goes back
   by a copy and the picture follows. Test `going_back_plays_the_same_water` (Task 4).
2. **Editing while the draft catches up** (an undo, a Feel slider let go, a new stroke) must start the draft over with
   the new physics and never draw the new field over the old body's layout. Test
   `a_new_setup_replaces_the_body_and_its_drawing` (Task 4) and `a_detail_change_gives_a_new_layout` (Task 2).
3. **A course with nothing carved, a Pour inside rock, a Pour with no Liquid, a fill box half in rock**: no drop is
   ever placed in rock and nothing panics. Tests `a_fill_at_the_start_fills_its_box_where_it_is_open` and
   `a_pour_finds_its_liquid` (Task 2), `drops_stay_out_of_the_rock` (Task 3).
4. **Leaving a Water scene for a Balls scene (New, Open, undo past the start is not possible) stops the draft thread**
   and frees its card memory. Test `a_balls_scene_has_no_draft` (Task 5).
5. **A long carve stroke** (hundreds of points, a big brush) must keep the stage responsive while painting and
   rebuild quickly on load. Test `a_long_stroke_is_quick_to_paint_and_to_build` (Task 1: 400 points of radius 150
   built in under 1 s in debug).

---

### Task 1: Carving: the square tip, the Carving card and the mask

**Files:**
- Modify: `crates/bpe-core/src/value.rs` (`Stroke.square`, its serde default and its hash)
- Modify: the full `Stroke { .. }` literals that name every field (`bpe-kit/src/modules/look/layers/gpu.rs`,
  `bpe-kit/src/paint/brush.rs` test `line`, `bpe-kit/tests/full_size_look.rs`, `bpe-studio/src/look_editor/brush.rs`,
  `bpe-studio/src/look_editor/mod.rs`, `bpe-studio/src/grade_mode/brush.rs`, `bpe-gfx/src/grade/brush.rs` test,
  `bpe-gfx/src/grade/pictures_tests.rs`): add `square: false`
- Create: `crates/bpe-kit/src/modules/water/carve.rs` (`CarveCard`, `CarveMask`, `CarvePaint`, `mask_for`)
- Modify: `crates/bpe-kit/src/modules/water/mod.rs` (`CARVE = "water.carve"`, card "Carving", `CARDS` 16)
- Modify: `crates/bpe-kit/src/things/water/mod.rs` (Course slots `[Form, Material]`, labels as a list)

**Interfaces:**
- Produces: `bpe_core::Stroke { .., square: bool }`;
  `bpe_kit::modules::water::{CARVE, CarveCard { carved: Strokes }}`;
  `carve::BAND: f32 = 48.0`, `carve::STEP: f32 = 4.0`;
  `CarveMask { lo: Vec2, cols: u32, rows: u32, open: Vec<f32> }` with `CarveMask::rock(size: Vec2) -> CarveMask`,
  `CarveMask::build(size: Vec2, strokes: &Strokes) -> CarveMask`, `fn at(&self, p: Vec2) -> f32` (bilinear, −BAND past
  the course), `fn apply(&mut self, stroke: &Stroke)`, `fn field(&self, cell: f32) -> bpe_water::SolidField`,
  `fn layout_for(size: Vec2, cell: f32) -> bpe_water::Layout`;
  `CarvePaint::start(base: Arc<CarveMask>, stroke: Stroke) -> CarvePaint`, `fn push(&mut self, p: [f32; 2])`,
  `fn reset(&mut self, stroke: Stroke)` (Straight and Smooth rebuild it), `fn mask(&self) -> &CarveMask`,
  `fn stroke(&self) -> &Stroke`;
  `mask_for(size: Vec2, strokes: &Strokes) -> Arc<CarveMask>` (a process-wide cache of the last two, keyed by
  `strokes.hash64()` and the size);
  `bpe_kit::things::water::carving_of(scene: &Scene) -> Strokes` (the Course's, empty without a card).
- `bpe-kit` gains `bpe-water` as a normal dependency (it stays a dev-dependency with `test-scenes` too).

- [ ] **Step 1: Write the failing tests** (`bpe-core` `value.rs` tests; `carve.rs` tests; the two water things tests)

```rust
#[test]
fn a_square_tip_is_saved_and_hashed_only_when_on() // a round stroke's ron has no "square" and its hash64 is the
    // one from before the field (pin the number printed by the old code for `one`); a square one round-trips and
    // hashes differently.

#[test]
fn nothing_carved_is_all_rock() // CarveMask::build(SCREEN, &empty): at(0,0) == -BAND; field(8.0) all < 0.
#[test]
fn a_dig_opens_a_round_hole_with_its_distance() // one dot of radius 100 at (0,0): at(0,0) ≈ 100 (±0.5), at(150,0)
    // ≈ -50 (±0.5), at(0,-100) ≈ 0 (±0.6).
#[test]
fn a_fill_after_a_dig_puts_rock_back() // dig r 100 then fill r 40 at (0,0): at(0,0) ≈ -40, at(70,0) ≈ 30.
#[test]
fn strokes_count_in_order() // fill then dig at the same place: open; dig then fill: rock.
#[test]
fn a_square_tip_digs_a_square() // square dot r 50 at (0,0): at(45,45) > 0, at(0,60) ≈ -10, at(60,60) ≈ -14.1 (±0.6).
#[test]
fn hardness_rounds_the_corner_where_strokes_meet() // two crossing lines (horizontal, vertical, r 30): at the rock
    // point (40,40) a softness-1 second stroke gives a larger value than softness 0.
#[test]
fn past_the_course_is_rock() // a dig over the whole screen: at(600, 0) (outside 1080 wide) == -BAND; at(530,0) > 0.
#[test]
fn painting_point_by_point_is_the_same_as_building_whole() // CarvePaint fed a 30-point stroke equals build() of the
    // strokes with it, every sample within 1e-4.
#[test]
fn a_long_stroke_is_quick_to_paint_and_to_build() // 400 points across the screen, r 150: build in under 1 s (debug).
#[test]
fn the_physics_field_follows_the_mask() // field(8.0).at(p) ≈ mask.at(p) within 1 px at 20 points near a face.

// things/water/mod.rs: each_water_kind_starts_with_its_own_cards now wants the Course with
// [(Form, CARVE), (Material, MATERIAL)]; the_cards_are_named_for_what_they_are_on adds
// label(COURSE, SlotId::Form) == "Carving".
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p bpe-core stroke -- --test-threads=2` and `cargo test -p bpe-kit --lib water -- --test-threads=2`
Expected: compile errors (no `square`, no `carve`), then FAIL on the old Course cards.

- [ ] **Step 3: Implement** `Stroke.square`; `carve.rs` as Decision 3 (a stroke's touched box is its points' box grown
  by `radius + k + BAND`; per stroke a working buffer of its own shape `g` over that box, the max over segments, then
  combined into `open` once; `apply` is the same for one stroke; `CarvePaint` keeps `before` (the base mask), the
  stroke's `g` over the whole course (−∞ where untouched) and the combined mask, and each new segment updates `g` and
  the combined value only in that segment's box, so painting is exact and incremental); the card; the Course's slots
  and labels (`WaterKind.label` becomes `&'static [(SlotId, &'static str)]`); `carving_of`; `mask_for` cache
  (`Mutex<Vec<(u64, Vec2, Arc<CarveMask>)>>`, two kept).

- [ ] **Step 4: Run the tests; they pass**

Run: the same two commands. Expected: PASS. Also `cargo test -p bpe-kit --lib -- --test-threads=2 settings` (the
settings contract covers the new card: `simple` count, sections in `SECTION_ORDER`; the Carving's section is named
"Carving", which no ball setting groups under).

- [ ] **Step 5: Commit** `Water 2b: the Carving card and the carved rock as a distance mask`

---

### Task 2: The scene as the engine's inputs (`WaterSetup`)

**Files:**
- Create: `crates/bpe-kit/src/things/water/draft.rs`
- Modify: `crates/bpe-kit/src/things/water/mod.rs` (`pub mod draft;`)

**Interfaces:**
- Consumes: Task 1's `mask_for`, `CarveMask::field`, `layout_for`, `carving_of`.
- Produces:
  - `pub const DRAFT_MOST: u32 = 500_000;`
  - `pub struct WaterSetup { pub size: Vec2, pub cell: f32, pub strokes: Arc<Strokes>, pub liquids: Vec<LiquidSetup>,
    pub pours: Vec<PourSetup>, pub gravity: Vec2, pub notes: Vec<&'static str>, pub print: u64, /* tracks */ }`
  - `pub struct LiquidSetup { pub id: ThingId, pub feel: Feel, pub colour: u32 }`
  - `pub enum PourKindSetup { Tap(bpe_water::Pour), Fill { lo: Vec2, hi: Vec2, kind: u32, colour: u32 }, Later }`,
    `pub struct PourSetup { pub id: ThingId, pub liquid: Option<usize>, pub kind: PourKindSetup }`
  - `WaterSetup::of(scene: &Scene, resolved: &Resolved) -> WaterSetup` (draft detail)
  - `fn layout(&self) -> Layout`, `fn mask(&self) -> Arc<CarveMask>`, `fn field(&self) -> SolidField`,
    `fn numbers(&self) -> Numbers` (`Numbers::for_cell(cell)`),
    `fn start(&self, gpu: Arc<WaterGpu>, field: &SolidField) -> Water` (body with the fills added and the frame-0
    inputs set), `fn inputs_at(&self, frame: u32) -> FrameInputs`,
    `pub struct FrameInputs { pub gravity: Vec2, pub feels: Vec<Feel>, pub pours: Vec<bpe_water::Pour> }` (pours: the
    Taps only, in the setup's order, each with its liquid, at that moment)
  - `fn fills(&self, layout: &Layout, field: &SolidField) -> Vec<Drop>` (Decision 6, overlaps left out)
  - `pub fn plain_look(scene: &Scene, resolved: &Resolved, reg: &Registry, time: f64) -> bpe_water::draw::PlainLook`
  - `pub fn liquid_of(scene: &Scene, target: &Target) -> Option<ThingId>` (Decision 6's rule)

- [ ] **Step 1: Write the failing tests** (in `draft.rs`, no card needed)

```rust
#[test]
fn a_new_water_scene_sets_up_an_all_rock_course_with_nothing_poured() // cell 8, size SCREEN, no liquids, no pours,
    // field all < 0.
#[test]
fn a_tap_pours_its_liquid_at_its_place() // a Liquid and a Pour at (100,-500), Rate 0.5, Width 60, Speed 400,
    // Aim 80, Spread 5, Starts 1, Stops 3: inputs_at(0).pours == [Pour { place, width: 30, speed: 400, aim: 80,
    // spread: 5, from: 1, until: 3, kind: STOPS, colour: Look colour as linear rgba8 }].
#[test]
fn a_fill_at_the_start_fills_its_box_where_it_is_open() // a box carved 400 x 300, a fill box 600 x 200 over its floor
    // half in rock: every drop's field value > 0, the count within 5 % of the open part's area / (cell/2)^2.
#[test]
fn overlapping_fills_do_not_double_up() // two equal fill boxes at the same place: as many drops as one.
#[test]
fn a_pour_finds_its_liquid() // Thing(second liquid) -> 1; Kind(LIQUID) and Anything -> 0; Tag("hot") -> the tagged
    // one; a disabled liquid is skipped (the next one counts as 0); no Liquid -> pours nothing and the note
    // "This pour has no Liquid to pour".
#[test]
fn later_pour_kinds_pour_nothing_yet() // Rain, Drip, Burst -> PourKindSetup::Later and the note
    // "Rain, drips and bursts arrive in a later part of Water mode."
#[test]
fn keyframed_physics_is_read_each_frame() // gravity keyed (0,4000) at 0 and (0,1000) at 1 s: inputs_at(30).gravity.y
    // == 2500 (linear); a Rate keyed 1 -> 0 over 1 s: inputs_at(60) pours width 0.
#[test]
fn a_detail_change_gives_a_new_layout() // Draft detail Fine: cell 6, layout().cell == 6, print differs.
#[test]
fn what_restarts_the_draft() // for every setting of every card on a scene holding one of each Water kind (Course,
    // Piece, Wall, Liquid with Heat, Pour, Drain, Force, Lamp; scene cards), and each placed thing's position: change
    // it (the `changed` rule of bpe-kit/tests/restarts.rs, plus a stroke added for Strokes and a new Target for a
    // Target) and check the print changes exactly for Decision 9's list. A tally line prints how many were checked.
#[test]
fn the_plain_look_reads_the_cards() // Look Colour #ff0000, Deep #000040, Clarity 50; Rock #102030; Back Colour
    // #010203: PlainLook { back: [1,2,3]/255, rock: [16,32,48]/255, liquids: [{[1,0,0],[0,0,64/255],50}] }; Darker
    // rock 0.25 -> back = rock * 0.75.
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p bpe-kit --lib things::water::draft -- --test-threads=2`. Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement** `draft.rs` as Decisions 5, 6, 8, 9 and 14. The print: a `DefaultHasher` over
  `format!("{:?}")` of the physics' base values (Water card's gravity, screens and draft detail; each liquid's id, on
  and Feel card; each pour's id, on, place, card params and liquid number), `strokes.hash64()`, the course size, and
  the tracks of those settings (`scene.tracks` filtered to their addresses). Keyframed values come from
  `Track::value_at(time)` over the resolved base. `start`: `Water::new(gpu, field, gravity, numbers, &feels)`, then
  `add(&fills)`, then `set_pours`. `fills` dedupe by a hash of half-cell bins checking the 3 × 3 bins round a drop.

- [ ] **Step 4: Run the tests; they pass.** Same command. Expected: PASS.

- [ ] **Step 5: Commit** `Water 2b: a Water scene as the engine's inputs`

---

### Task 3: The draft on the card, and its pace

**Files:**
- Create: `crates/bpe-kit/tests/water_draft.rs`

**Interfaces:**
- Consumes: `WaterSetup::{of, field, start, inputs_at}`, `plain_look`, `bpe_water::draw::{Plain, PlainGpu, View}`.
- Produces: `fn screen_course(reg: &Registry, cell: DraftDetail) -> Scene` (the timing scene of Decision 18: one
  carved chamber, a fill two thirds high, a Tap at the top), used by both tests.

- [ ] **Step 1: Write the tests**

```rust
#[test]
fn drops_stay_out_of_the_rock() // test_device(); screen_course at 12 px played 2 s: count > 0, every drop read back
    // has field.at(world point) > -0.5 cell, and the tap's drops reached the pool (some drop below the fill's top
    // has a serial past the fill's count).
#[test]
#[ignore = "a measuring run, by hand on the owner's card"]
fn draft_pace() // for 12, 8 and 6 px: drops, steps a frame (mean and most), physics ms a frame over frames 60..360
    // (one submission a frame, waiting for the card only at the end of each 60), plain drawing ms at 600 x 1067 and
    // at 1080 x 1920 (Plain::draw_many), and "plays live: yes/no" by Decision 18's 12 ms mark. First line names the
    // card (adapter name, backend, driver). Tab-separated, one row a detail.
```

- [ ] **Step 2: Run** `BPE_SKIP_GPU_TESTS= cargo test -p bpe-kit --test water_draft -- --test-threads=2` on lavapipe.
  Expected: `drops_stay_out_of_the_rock` PASS. Then `cargo test --release -p bpe-kit --test water_draft -- --ignored
  --nocapture draft_pace` once on lavapipe (numbers only to check it runs; not told to Aedan).

- [ ] **Step 3: Commit, push, and time it on the RX 9070 XT** by a Remote Control session on
  `F:\!Ai Stuff\BallPhysicsEngine` (project memory `liquid-speed` recipe: build the test exe for
  `x86_64-pc-windows-gnu` inside WSL `bpe-ci` as `runner` in a scratch folder, run it natively from `%TEMP%`, never
  run cargo on Windows in the project folder, wait for CI jobs on the PC to finish first, stop at once on any screen
  freeze or driver reset (check the Windows logs for display driver resets), push the printed table to branch
  `claude/water-2b-pace` with `[skip ci]`, then end). End the session when it reports. If Remote Control cannot
  start, use the CI probe recipe (project memory `card-shadow-edge-fix`).

- [ ] **Step 4: Decide.** 8 px plays live (Decision 18): no physics change; write the table into "As built and
  measured". It does not: add a Task 3b here, planned from the measurement (which passes take the time, by the
  engine's own `pace` numbers), and do it before Task 8.

---

### Task 4: The draft job

**Files:**
- Create: `crates/bpe-studio/src/water_mode/draft.rs` (`DraftJob`, `Live`, `Shared`)
- Modify: `crates/bpe-studio/src/water_mode/mod.rs`
- Modify: `crates/bpe-studio/Cargo.toml` (`bpe-water.workspace = true`)

**Interfaces:**
- Consumes: Task 2's `WaterSetup`, `DRAFT_MOST`.
- Produces:
  - `pub struct Live { pub water: Water, pub field: SolidField, pub plain: Plain, pub generation: u64, pub print: u64 }`
  - `DraftJob::start(device: &wgpu::Device, queue: &wgpu::Queue, setup: Arc<WaterSetup>, most: u32) -> Option<DraftJob>`
  - `fn restart(&self, setup: Arc<WaterSetup>)` (newest wins), `fn follow(&self, frame: u32)`,
    `fn live(&self) -> &Mutex<Option<Live>>`, `fn reached(&self) -> Option<u32>` (the body's frame),
    `fn count(&self) -> u32`, `fn full(&self) -> bool` (stopped by the cap), `fn failed(&self) -> bool`,
    `fn busy(&self) -> bool` (a target or a setup still to reach)
  - `pub const COPY_EVERY: u32 = 120;` `pub const COPIES_MOST: u64 = 1 << 30;`
  - `pub fn frame_of(time: f64) -> u32` (`floor(time × 60)`, never below 0)
- The thread: named "water draft", low priority (`thread_priority`), parked up to 100 ms when there; a drop guard sets
  `failed` if it ends by a panic; `Drop` stops it.

- [ ] **Step 1: Write the failing tests** (in `draft.rs`; `bpe_gfx::test_device()`; Task 3's kind of scene built in
  the test, 12 px, a small fill and a tap)

```rust
#[test]
fn the_draft_follows_the_playhead_and_goes_back() // follow(90): reached Some(90); follow(30): reached Some(30) and
    // the body's frames() == 30.
#[test]
fn going_back_plays_the_same_water() // follow(150) then read(); follow(300); follow(150): read() equal to the first.
#[test]
fn a_new_setup_replaces_the_body_and_its_drawing() // restart with Draft detail Fine: live.generation goes up,
    // live.field.layout.cell == 6 and live.water.layout() == live.field.layout.
#[test]
fn the_draft_stops_pouring_at_its_most() // most 400 with a tap: count stays within one frame's births of 400 and
    // full() is true.
#[test]
fn copies_are_kept_every_two_seconds_within_their_memory() // a unit test of the keep/let-go rule on frame numbers
    // and estimated sizes (no card): 10 copies of 200 MiB keep the newest and every other one under 1 GiB.
```

- [ ] **Step 2: Run them to see them fail.** `cargo test -p bpe-studio water_mode::draft -- --test-threads=2`.

- [ ] **Step 3: Implement** as Decisions 10, 11 and 12. Each frame: `set_gravity`, `set_feels`, `set_pours` from
  `setup.inputs_at(frames)` (no pours once full), `frame()`, keep a copy when `frames % COPY_EVERY == 0` and none is
  kept there. Going back: drop the copies after the target, `copy()` the newest at or before it into the body. A new
  setup: build its field (`setup.field()`), body (`setup.start`) and `Plain::new(plain_gpu, &field, numbers.wall_off)`
  off the lock, then swap them in under it (`generation + 1`), copies cleared. `WaterGpu` and `PlainGpu` are made once
  on the thread.

- [ ] **Step 4: Run the tests; they pass.** Same command.

- [ ] **Step 5: Commit** `Water 2b: the draft runs on its own thread and follows the playhead`

---

### Task 5: The stage plays the water

**Files:**
- Modify: `crates/bpe-studio/src/water_mode/stage.rs` (picture under the course lines; middle words; markers per
  Decision 17)
- Create: `crates/bpe-studio/src/water_mode/picture.rs` (`StagePicture`: the egui texture, what was drawn)
- Modify: `crates/bpe-studio/src/app/mod.rs` (`Studio.water_draft: Option<DraftJob>`, `water_setup:
  Option<Arc<WaterSetup>>`, `water_print: u64`; the stage call gets `frame.wgpu_render_state()`)
- Modify: `crates/bpe-studio/src/app/runs.rs` (restart_runs: the water print, held like the balls')
- Modify: `crates/bpe-studio/src/app/panels.rs` (status line readout)
- Modify: `crates/bpe-studio/src/water_mode/tests.rs`, `crates/bpe-studio/src/app/water_tests.rs`

**Interfaces:**
- Consumes: Task 4's `DraftJob`, `Live`, `frame_of`; Task 2's `plain_look`, `WaterSetup::of`.
- Produces: `pub fn stage(ui: &mut Ui, render: Option<&egui_wgpu::RenderState>, studio: &mut Studio)`;
  `StagePicture::show(&mut self, ui, render, studio, view: &CourseView)`;
  `fn picture_view(view: &CourseView, pixels_per_point: f32) -> bpe_water::draw::View` (Decision 13's size rule:
  `lo = view.to_world(view.rect.min)`, `scale = picture width / course width`);
  `pub(crate) fn water_readout(count: u32, cell: f32) -> String` ("· Water: 52,300 drops (draft 8 px)").
- `Studio::scene_changed` / `restart_runs`: in a Water scene build `WaterSetup::of`; if its print differs from
  `water_print` and runs are not held, `job.restart(setup)`; a Balls scene drops the job.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_picture_covers_the_course_view() // a CourseView of SCREEN in a 600 x 900 room at 2 px a point: View.lo ==
    // (-540, -960), width == round(rect.width * 2), scale == width / 1080; a 4-screen course in a huge room caps at
    // 2048 a side.
#[test]
fn a_physics_edit_restarts_the_draft_and_a_look_edit_does_not() // headless_water studio: a Feel change moves
    // water_print, a Look colour does not (no device needed: the print is kept even without a job).
#[test]
fn a_balls_scene_has_no_draft() // after new_scene(Balls) studio.water_draft is None and water_setup None.
#[test]
fn the_status_line_counts_the_draft_drops() // water_readout(52_300, 8.0) == "· Water: 52,300 drops (draft 8 px)".
#[test]
fn a_course_with_nothing_carved_says_how_to_start() // kittest: the stage shows "The course is solid rock. Pick
    // Carve and paint to dig it out." and not after a stroke is on the Carving card.
```

- [ ] **Step 2: Run them to see them fail.** `cargo test -p bpe-studio water -- --test-threads=2`.

- [ ] **Step 3: Implement** Decisions 9, 13, 16 and 17. The job starts in `stage()` the first frame a render state is
  there and the scene is a Water scene. Each stage frame: `job.follow(frame_of(studio.time))`; `StagePicture` draws
  (Decision 10's lock rule) and registers or updates its texture; `request_repaint_after(16 ms)` while `job.busy()` or
  playing; "Working out the water…" pill when `reached` is over 15 frames behind the target.

- [ ] **Step 4: Run the tests; they pass.** Same command, plus the existing `water_mode::tests` and
  `app::water_tests` (they must still pass without a device).

- [ ] **Step 5: Commit** `Water 2b: the stage plays the draft in the plain look`

---

### Task 6: The Carve tool

**Files:**
- Create: `crates/bpe-studio/src/water_mode/carve_tool.rs` (`CarveBrush`, the strip, painting)
- Modify: `crates/bpe-studio/src/water_mode/rail.rs` (Carve ready; its hint "Paint to dig the course out of the rock.
  Alt fills it back in. Shift draws a straight line.")
- Modify: `crates/bpe-studio/src/water_mode/stage.rs` (the strip shows the brush when Carve is picked; painting)
- Modify: `crates/bpe-studio/src/water_mode/tests.rs`

**Interfaces:**
- Consumes: Task 1's `CarvePaint`, `mask_for`, `CarveCard`; Task 4's `Live` (its `plain.set_field`).
- Produces: `pub struct CarveBrush { pub fill: bool, pub size: f32, pub hardness: f32, pub square: bool, pub
  straight: bool, pub smooth: bool }` (Default: Dig, 120, 1.0, Round, off, off);
  `fn stroke(&self, points: Vec<[f32; 2]>, alt: bool) -> Stroke`;
  `WaterStage.brush: CarveBrush`, `WaterStage.painting: Option<Painting>` where `Painting { before: Strokes, paint:
  CarvePaint, lazy: Vec2, press: Vec2 }`;
  `pub fn strip(ui: &mut Ui, brush: &mut CarveBrush)` (controls named "Dig", "Fill", "Brush size", "Hardness",
  "Round", "Square", "Straight", "Smooth").

- [ ] **Step 1: Write the failing tests** (kittest, `water_studio()`, no device)

```rust
#[test]
fn carving_paints_one_stroke_as_one_undo_step() // Carve picked, a drag from screen point A to B: the Course's Carving
    // has one stroke, its first point == view.to_world(A) (within 1 px), radius 60, not erase, not square; Undo
    // leaves no stroke; Redo brings it back.
#[test]
fn alt_fills_and_the_strip_sets_the_brush() // clicking "Fill", "Square", setting "Brush size" to 40: the next stroke
    // is erase, square, radius 20; a drag with Alt held while Fill is on digs (erase false).
#[test]
fn a_straight_line_keeps_two_points() // "Straight" on (or Shift held): a drag through 8 points stores 2 points, the
    // press and the release.
#[test]
fn smooth_lags_behind_the_pointer() // "Smooth" on: a drag's last point is within radius / 2 of the release, not on it,
    // and the path has no point farther than radius / 2 from the pointer's path.
#[test]
fn escape_drops_the_stroke() // press, move, Escape, release: no stroke, nothing in the history.
#[test]
fn a_course_without_a_carving_card_gets_one() // remove the card from the scene, paint: the card is back with one
    // stroke, in the same undo step.
#[test]
fn carve_is_ready_on_the_rail() // Tool::Carve.ready() and its button is enabled.
```

- [ ] **Step 2: Run them to see them fail.** `cargo test -p bpe-studio water_mode -- --test-threads=2`.

- [ ] **Step 3: Implement** Decision 15. The strip sits in the strip row after the tool's name (the hint moves to the
  button's hover text while Carve is picked). Painting starts from `mask_for(size, &carving_of(scene))`; each frame
  with a new point, when the live body is there and its field's layout is the draft's, rebuild that field from
  `paint.mask()` and `set_field` it (under `try_lock`; a missed frame is caught next frame). Release: `studio.scene`
  gets `before + stroke` on the Carving card (added with `scene.add_slot` when missing) and `separate_edit = true`;
  Escape restores the live field from `live.field`.

- [ ] **Step 4: Run the tests; they pass.** Same command.

- [ ] **Step 5: Commit** `Water 2b: the Carve tool`

---

### Task 7: Pours that pour

**Files:**
- Modify: `crates/bpe-studio/src/water_mode/stage.rs` (`place`: a Pour adds "Water" when there is no Liquid; notes in
  the strip for a picked Pour)
- Modify: `crates/bpe-studio/src/water_mode/tests.rs`

**Interfaces:**
- Consumes: Task 2's `WaterSetup.notes`, `liquid_of`, `PourKindSetup::Later`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn placing_a_pour_without_a_liquid_adds_water() // Pour tool click: a Pour and a Liquid named "Water"; one Undo
    // removes both; a second Pour adds no second Liquid.
#[test]
fn a_picked_pour_says_what_it_cannot_do_yet() // a picked Pour of kind Rain shows "Rain, drips and bursts arrive in a
    // later part of Water mode." in the strip; a Tap shows none.
```

- [ ] **Step 2: Run them to see them fail.** `cargo test -p bpe-studio water_mode -- --test-threads=2`.
- [ ] **Step 3: Implement** Decisions 6 (the strip's words), 7 and 17.
- [ ] **Step 4: Run the tests; they pass.**
- [ ] **Step 5: Commit** `Water 2b: a pour pours, and the first brings a Liquid`

---

### Task 8: Notes, review, pull request

**Files:**
- Modify: `docs/notes/water.md` ("Part 2b: water runs in a Water scene (as built)", and part 1's "What a Water
  scene hasn't got yet" brought up to date), `CLAUDE.md` (the crate map's `bpe-water` line: the editor uses it now),
  this plan ("As built and measured").

- [ ] **Step 1:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 2:** the tests this part touches: `cargo test -p bpe-core value`, `cargo test -p bpe-kit --lib water`,
  `cargo test -p bpe-kit --test water_draft`, `cargo test -p bpe-studio water`, all `-- --test-threads=2`; and
  `cargo test -p bpe-kit --test golden` to show the balls unchanged.
- [ ] **Step 3:** one Opus review of the whole branch; fix what it finds (a test for each real fault).
- [ ] **Step 4:** push, open the draft PR (attribution lines first), watch CI; mark it ready and merge it (squash)
  when every check is green.
- [ ] **Step 5:** project memory: a `water-2b` file and the index line; tell the coordinator the part is merged.

## As built and measured

(Filled in as the tasks land.)
