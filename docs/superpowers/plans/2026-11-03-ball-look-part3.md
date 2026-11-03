# Ball Look editor, part 3 (Group view): plan

Spec: `docs/superpowers/specs/2026-11-03-ball-look-editor-design.md` (sections Group view, Architecture). Part 1 plan
for how the stack works: `docs/superpowers/plans/2026-11-03-ball-look-part1.md` (merged, PR #72). Mockups: canvas
https://claude.ai/artifact/C4Nyud3QGE5S15VbR3DwDb, boards "Group view" and "Group options".

Work rules: superpowers workflow (TDD per task, verify before claiming done). Opus at high effort for every step.
No subagent review per task; ONE Opus review of the whole diff at the end, before the PR. No demo videos or
screenshots unless Aedan asks. Never add built-in or example scenes. Messages to Aedan are plain language. Branch
fresh from `main`.

**Runs in parallel with part 2** (`docs/superpowers/plans/2026-11-03-ball-look-part2.md`). Part 2 merges first. Tasks
1 to 10 don't need part 2. **Task 11 (group shader) needs part 2's in-ball shader programs**: do it only after
rebasing onto a `main` that has part 2. If part 2 isn't merged when tasks 1 to 10 and the review are done, open the
PR without task 11, say so in the PR and to Aedan, and add task 11 as a follow-up PR from this thread once part 2
merges.

## Shared contract with part 2 (both plans say the same; don't deviate)

- `LayerSection` variants in this order: `Drawn, Reaction, Mask, Group`. Part 3 adds `Group` (and `Mask` stays part
  2's; if it isn't on main yet, add only `Group` after `Reaction` and let the rebase put `Mask` before it).
- `Layer` gains fields only with `#[serde(default, skip_serializing_if = ...)]`: part 2 `parent: Option<LayerId>`,
  part 3 `only: Vec<u32>`.
- `Value` / `ValueType` gain variants at the end: part 2 `Strokes`, part 3 `Points(Vec<[f32; 2]>)`.
- `LayerGpu` grows (part 2) by `extra: [[f32; 4]; 4]`. `LayeredInstance` grows (part 3) by `group: [f32; 4]` after
  `range`; `range.w` = group mode code. Part 2 doesn't use `range.z/w`.
- Record codes: 0 to 4 part 1; 5 shading, 6 shader, 7 mask (part 2); **8 group gradient, 9 group shader (part 3)**.
- Format: part 3 bumps `Scene::FORMAT` by one from whatever main has when it merges (5 if part 2 hasn't merged, 6
  after rebasing onto part 2).

## Outcome

- The inspector header of a Balls thing has a **Ball / Group** switch next to the kind name. Ball is part 1's Look
  editor. Group shows the whole group in the view and the group settings below it.
- Group settings (Balls thing's own settings, because they decide where balls start): **Group** on/off (off = one
  ball), **Shape** (Grid, Line, Ring, Filled circle, Triangle, Honeycomb, Spiral, Heart, Scatter, Word, Custom), **How
  many**, **Space between**, shape-only settings (Columns for Grid, Radius for Ring, Word and Letter height for Word,
  Seed and Area for Scatter), **Turn**. **Custom**: drag balls in the view; + adds, right-click removes.
- **Colour across the group** (look, group layer `group.gradient`): Gradient with **Edit in view** (drag end dots to
  turn and stretch, diamonds to ease), Angle, Easing (Even, Ease in, Ease out, Ease both) and its strength, **Spread
  by** (Place, Order, Centre out, Random, Speed, Hits, Time), **With ball colours** (Off, Replace, Tint, Multiply,
  Screen, Overlay) + Amount, **Colours it changes** (Body, Every layer, Choose…).
- **Variety** (size, launch speed, colour, turn spread, Shuffle), **Release** (all at once, one by one, waves; order),
  **Launch** (same, fan, outward, random), **Odd ones out** (pick balls in the view, give them their own layers).
- **Shader across the group** (task 11, after part 2): group layer `group.shader`, repeats; effect + settings, speed,
  scale, Blend with the balls (Replace, Normal, Add, Screen, Multiply, Overlay, Soft light, Tint, Hue) + Amount,
  Where it shows (Inside the balls only / Also around them), Colours it changes. Group gradient and group shaders
  stack in a small list of their own, drag to order.
- Today's Arrangement (Single / Block / Ring) is gone. **Goldens byte-identical**: Block and Ring test scenes keep
  their exact positions.

## Decisions (fixed)

1. **Placement settings replace `Arrangement`** on `BallsSettings` (physics). Keys: `group: bool` (false), `shape:
   GroupShape` (Grid), `count: u32` "How many" (30, range 1..5000), `columns: u32` (6, Grid and Honeycomb), `space:
   f32` "Space between" (8 px, range -400..400, span 0..60: the gap between neighbours' edges, so centre to centre
   is `2 × Size + space`), `ring_radius` (200, Ring), `word: String` ("BALLS", Word), `letter_height` (240 px, Word),
   `seed: i64` (1, Scatter and Variety), `area: Vec2` (600 × 600, Scatter), `custom: Points` (Custom), `turn` deg
   (0). Off-group = one ball at the thing's position (today's Single). Every shape fills `count` balls except Word
   (as many as the letters need) and Custom (one per point).
2. **Grid matches today's Block exactly**: rows = ceil(count / columns); origin = -((columns-1), (rows-1)) × step /
   2; ball i at origin + (i mod columns, i div columns) × step, step = 2 × radius + space; a part-filled last row is
   left-aligned. With count = columns × rows and space = spacing - 2 × radius this is today's Block. **Ring** matches
   today's Ring (clockwise from the top, `ring_radius`); Space between is hidden for Ring. Turn rotates every offset
   about the centre (turn 0 = today).
3. **Test scenes** (`scenes.rs`, `testing.rs`, the `p.set("arrangement"…)` uses in kit tests): `"single"` → group
   off; `"block"` → grid with count = columns × rows and space = spacing - 2 × radius; `"ring"` → ring. Values in the
   test scenes are small integers or halves, so `2r + (s - 2r)` is exact in f32. Goldens must not move; if one does,
   find the arithmetic difference (do not re-bless).
4. **Other shapes** (positions only, deterministic, all centred on the thing): Line (along x, step apart), Filled
   circle (hexagonal rings out from the centre until `count`), Triangle (rows 1, 2, 3… pointing up), Honeycomb (grid
   with odd rows shifted half a step, rows √3/2 step apart), Spiral (golden-angle Vogel spiral, `step × 0.5 × √i`),
   Heart (points along the heart curve, evenly by arc length, scaled so neighbours are `step` apart), Scatter (seeded
   splitmix64 points in `area`, rejecting ones closer than `step` to a placed one, up to 30 tries each; may place
   fewer, the Group view says "Only N fit"), Word (render `word` with the IBM Plex Sans font from
   `crates/bpe-studio/assets/fonts/` via `ab_glyph` — already in Cargo.lock through egui — at `letter_height`, sample
   a square lattice of `step`, keep lattice points inside glyphs), Custom (the points, in px relative to the thing).
   Place the font bytes in bpe-kit with `include_bytes!` of that same file path (no copy).
5. **Group look is group layers** in the same `look.layers` card: `LayerSection::Group`, kinds `group.gradient`
   (repeats false) and `group.shader` (repeats true, task 11). In the `layers` Vec they sit after the reactions;
   their order among themselves is their draw order (first is applied first). The Ball view lists don't show them;
   the Group view's "Group look" list does. `copy_look` leaves them out and `paste_look` keeps the target's own.
   `copy_layer` / `paste_layer` work on them within the Group list.
6. **Group gradient maths**: per ball the CPU works `t` (Place: projection of the ball's spawn offset on the angle,
   over the group's extent along it; Order: i / (n-1); Centre out: distance from the group centre over the max;
   Random: seeded by ball number and `seed`; Speed, Hits, Time: as `paint::Follow` does today, reuse `paint::eval`),
   eases it (`Even` t; `In` t^k; `Out` 1-(1-t)^k; `Both` smoothstep-like `t^k / (t^k + (1-t)^k)`, k = Easing strength
   1..6, default 2), looks the colour up in the gradient. Then by **Colours it changes**:
   - Body: mixed into the body colour on the CPU (before the colour reactions) with the With-ball-colours mode at
     Amount. Patterns that are a shade of the body follow.
   - Every layer: the colour goes in `LayeredInstance.group` with `range.w` = mode code and the group record (code 8)
     at the end of the stack mixes it over the finished ball (inner and outer) by mode and Amount.
   - Choose: a `chosen: Text` setting holding layer ids ("3,5") picked from a checklist; those records get flag bit 2
     and the shader mixes the group colour into that layer's colour before compositing it.
   - Mode maths (straight colours, ball `b`, group `g`): Replace `g` (Body: replaces the body colour; Every layer
     and Choose: `g` over the covered pixels, keeping coverage), Tint (hue and saturation of `g`, lightness of `b`:
     reuse `blended(Colour)` maths), Multiply `b × g`, Screen `1 - (1-b)(1-g)`, Overlay (W3C), all
     through the same `blended()` codes where one exists; then `mix(b, result, amount)`. Off = the layer draws nothing.
7. **Edit in view**: the gradient tool shows a line across the group with an end dot at each end (drag: angle and
   extent; the extent becomes `span` on the gradient paint) and one diamond in the middle (drag along the line: Easing
   strength; double-click cycles Easing). Changes are normal `Edit::SetSlot` edits on the group.gradient layer's keys
   (one undo step per drag).
8. **Variety** (Balls settings, physics-safe): `vary_size` 0..1 (0), `vary_speed` 0..1 (0) change physics but default
   0 so goldens hold; `vary_colour` 0..1 (hue spread), `vary_turn` 0..180 deg are look-only and read by `BallsDraw`
   from the thing's resolved params. All seeded by `seed` and the ball number (splitmix64). **Shuffle** button adds 1
   to `seed`. Per-ball size factor is stored in `BallsState` and reapplied in `settings_changed` (so a Size keyframe
   keeps each ball's factor).
9. **Release** (`release: Release` AllAtOnce default, OneByOne, Waves; `release_gap` s 0.1; `wave_size` 10;
   `release_order: ReleaseOrder` FirstToLast, CentreOut, Random): `build` pushes the balls released at step 0;
   `ThingSim::spawn` pushes each later ball at step `round(k × gap × 240)`. The ball's number (for colours, Order and
   Odd ones out) is its place in the shape, not its push order: keep a `numbers: Vec<u32>` in `BallsState` indexed by
   body row. Check that snapshots/seek replay `spawn` deterministically (emitters used this in Version 1; read
   `bpe-runtime` playback before relying on it).
10. **Launch** (`launch: Launch` Same default, Fan, Outward, Random; `fan_angle` 60 deg): Same = today's velocity;
    Fan spreads the velocity's direction over `fan_angle` by ball number; Outward points it away from the group
    centre at the same speed; Random seeded. The Group view shows a drag arrow from the group centre for the launch
    velocity (sets `velocity`).
11. **Odd ones out**: `Layer.only: Vec<u32>` (ball numbers; empty = every ball). In the Group view, **Pick balls**
    toggles a picking mode: click balls to add/remove them from the view's picked set (view state). With some
    picked, **Give picked balls a layer** opens the drawn-layer add menu and adds that layer with `only` = picked; the
    Ball view's layer row shows "only 3 balls". `BallsDraw` groups balls by the set of layers that apply to them and
    writes one record range per distinct set (most scenes: one range).
12. **Group view** draws with the same second `Renderer` as the Ball view: a mini scene with the thing and its real
    group placement (`offsets` at step 0, no physics), camera fitted to the group's bounds plus 10 %, ground Dark or
    Scene. Moving and Hit modes aren't offered in Group view (Still only, plus the gradient tool, custom dragging,
    picking and the launch arrow as overlays).
13. **Where the group settings show**: the Balls thing's own settings grid in the inspector hides the placement,
    variety, release and launch keys (mark them `group = "Group"` in the derive and skip that group in the thing's own
    grid); the Group view shows them. Size stays in the Body layer (part 1) and in the own grid as today.
14. **Group shader** (task 11): an in-ball shader record (code 9) at its place in the group list, using part 2's
    generated programs, with uv = the ball's point in **group space** (ball centre's offset in the group's bounds +
    local point × radius, over the bounds, × 1/Scale), so one effect spreads across the group. Blend with the balls by
    mode and Amount. Inside only: covers the body. Also around: also drawn past the edge in a soft ring of width
    `around` (radii, default 0.6, falling off like a glow), which raises the stack's `reach`.

## Task 1: core: Group section, Points value, layer `only` (bpe-core)

- `LayerSection::Group`; `Layer.only: Vec<u32>`; `Value::Points(Vec<[f32; 2]>)`, `ValueType::Points` (no keyframes,
  copy/paste value works); every match handles them.
- `add_layer` puts Group layers after reactions; `move_layer` accepts group layers within the group range only.
- `copy_look` skips Group layers; `paste_look` removes only the target's non-group layers and keeps its group ones.
- `set_layer_only(thing, slot, id, Vec<u32>)` edit (refuses Body: Body is every ball's).
- Format bump (contract).

Tests: group layer order and moves; copy/paste look leaves group layers alone; Points serde; `only` round trip; Body
refuses `only`.

## Task 2: placement settings and shapes (bpe-kit things/balls)

Files: `things/balls/{mod.rs, sim.rs}`, new `things/balls/shapes.rs`, `scenes.rs`, `testing.rs`, kit tests that set
`arrangement`.

- `BallsSettings` per decision 1 with `when` conditions (`"group and shape=grid|honeycomb"` style: check
  `bpe_core::applies` syntax: `or`, `and`, `|`). Remove `Arrangement`.
- `shapes.rs`: `pub fn offsets(s: &BallsSettings) -> Vec<Vec2>` dispatching to one fn per shape (decisions 2, 4),
  then turn. Word rasterising cached by (word, height, step) in a `OnceLock<Mutex<HashMap>>`.
- `sim.rs` uses `shapes::offsets`; `testing::scene_with_balls(arrangement)` maps the old words (decision 3).

Tests: grid of 2 × 3 equals today's Block positions bit for bit (compute today's formula in the test); ring equals
today's Ring; count respected per shape; scatter deterministic and no two closer than step; word "I" gives a column;
turn 90° rotates. **Goldens**: `cargo test -p bpe-kit --test golden*` debug and `--release` unchanged.

## Task 3: Variety, Release, Launch (bpe-kit things/balls/sim.rs)

- Decisions 8, 9, 10. `BallsState { settings, numbers, size_factor }`. `spawn` hook for Release.
- Look-only variety read by `BallsDraw` (task 6).

Tests: defaults leave positions, velocities, radii identical to task 2 (goldens again); OneByOne with gap 0.1 adds
one ball every 24 steps in order; CentreOut order; Waves of 10; Fan spreads directions symmetric about the launch
velocity; Outward points away from centre; vary_size changes radii deterministically and a Size keyframe keeps the
factors; seek back and replay gives identical bodies (snapshot test).

## Task 4: group gradient layer kind (bpe-kit)

- `GroupGradient` settings: `colours: Gradient` (two stops, a blue to pink default so adding it shows something),
  `angle` deg, `span` (group fraction 0.1..4, 1), `ease: Easing` (Even), `ease_strength` 1..6 (2), `spread: Spread`
  (Place, Order, CentreOut, Random, Speed, Hits, Time), `mix: GroupMix` (Replace default, Tint, Multiply, Screen,
  Overlay, Off), `amount` 0..1 (0.8), `changes: Changes` (Body default, EveryLayer, Choose), `chosen: Text`.
- `group/` module in `modules/look/layers/group.rs` with `pub fn group_t(...)` and `pub fn group_colour(...)`
  (decision 6), pure.

Tests: Order gives 0 for the first ball and 1 for the last; Ease in bunches colours toward the start; Centre out is 0
in the middle; Random deterministic per seed; Replace on Body gives the gradient colour; Multiply on white gives the
gradient colour; amount 0 leaves the ball.

## Task 5: renderer: group colour on the instance (bpe-gfx)

- `LayeredInstance.group: [f32; 4]` (contract), `range.w` = mode code (0 none). Size check ≤ 128 B.
- `layers.wgsl`: group record code 8 at the end mixes `in.group` over inner/outer by `range.w` and amount
  (`amount.x`); flag bit 2 on any record mixes the group colour into that record's colour first.

Tests (lavapipe): Every layer Multiply turns a white striped ball's white parts blue and keeps dark stripes dark;
Choose on an Outline colours only the outline; mode 0 changes nothing.

## Task 6: draw: group colour, look variety, odd ones out (bpe-kit balls/draw.rs, gpu.rs)

- Per ball: group `t` and colour (task 4), body colour mix for Changes = Body, instance `group` for Every layer /
  Choose; look variety (hue spread, turn spread).
- Records per distinct set of applying layers (decision 11): build a key per ball from the ids of `only` layers that
  name it; write one record range per key; instances point at theirs.

Tests: ten balls with Order spread get ten different body colours in order; Every layer sets instance group colour
and records include code 8; an `only` layer on ball 3 gives ball 3 a longer range than ball 2; vary_turn changes
angles deterministically.

## Task 7: editor: Ball / Group switch and Group view frame (bpe-studio)

Files: `inspector.rs` (header switch), new `look_editor/group_view.rs`, `look_editor/ball_view.rs` (shared renderer).

- Ball / Group segmented switch in the inspector header for Balls things; per-thing view choice in `Inspector`.
- Group view frame (decision 12): mini scene with the thing, its offsets, camera fitted; Dark / Scene ground.
- Below the view: sections as on the "Group view" board: **Group** (switch, shape chips with a small dot picture
  each, How many, shape-only settings, Space between, Turn), **Colour across the group** (the group look list with
  + Add: Gradient, Shader (greyed with "comes with Shader layers" until task 11), then the selected one's settings),
  folds **Variety**, **Release**, **Launch**, **Odd ones out**. Settings grids via `settings_grid` (own keys via
  `SetOwn`, layer keys via layered keys). Hide those own keys from the thing's own grid (decision 13).
- "Only N fit" note for Scatter; ball count shown beside How many for Word and Custom.

Tests (kittest): switch to Group shows group settings; Group off shows only the switch and a note "One ball";
choosing Ring shows Radius and hides Columns and Space between; adding a gradient shows its settings; own-settings
grid no longer lists How many; undo works for each.

## Task 8: Custom shape dragging and launch arrow (bpe-studio group_view.rs)

- View ↔ world maps for the group camera. Custom: each point drawn as a handle; drag moves it (one undo step per
  drag), **+** adds a point at the view centre (or where the next click lands, while + is held down: pick one, state
  it: + adds at the centre, offset 1 step right of the last point if one is there), right-click a handle removes it.
  Choosing Custom when there are no points fills `custom` with the current shape's positions (so switching to Custom
  starts from what you saw).
- Launch arrow from the group centre (drag the tip: `velocity`).

Tests: dragging a custom point changes `custom` once per drag; right-click removes; switching Grid → Custom copies the
grid's positions; the launch arrow sets velocity.

## Task 9: gradient tool (bpe-studio group_view.rs)

- Edit in view toggle (decision 7) on the group gradient's settings; the line, dots and diamond overlay; a tag shows
  "−14° · ease out" as on the board.

Tests: dragging an end dot changes angle; dragging the diamond changes ease strength; one undo step each.

## Task 10: Odd ones out picking (bpe-studio group_view.rs, look_editor/layer_list.rs)

- Pick balls mode, picked set, Give picked balls a layer (decision 11); "only N balls" summary on rows; the Ball view
  shows ball number 0's look unless the selected layer has `only`, then the first picked ball's.
- Row menu on an `only` layer: "Apply to every ball" (clears `only`).

Tests: picking two balls and adding an Outline gives `only = [a, b]`; summary text; Apply to every ball clears it.

## Task 11: group shader (after part 2 is on main)

- Rebase onto main with part 2. `group.shader` kind: effect, code, speed, scale, the generated effect params (reuse
  part 2's Shader layer defs builder), `mix: GroupShaderBlend` (Replace, Normal, Add, Screen, Multiply, Overlay,
  SoftLight, Tint, Hue), `amount`, `where_: Where` (Inside default, Around), `around` 0.1..2 (0.6), `changes` /
  `chosen` as the gradient's.
- Record code 9 per decision 14 through part 2's program machinery (`Stack::program` includes group shader sources).
- Enable + Add → Shader in the group look list.

Tests (lavapipe): a Rainbow flow group shader gives neighbouring balls different colours (one effect across the
group) where a ball Shader layer would give them the same; Around draws past the edge and raises reach; amount 0 is
no change.

## Task 12: check, docs, review, PR, exe

- `cargo clippy --workspace`, `cargo fmt`, `cargo test --workspace` debug (background) and `--release` (except the
  known debug-only test). Goldens unchanged, never re-blessed.
- CLAUDE.md: Balls placement settings and `shapes.rs`, group layers (`LayerSection::Group`), `Layer.only`,
  `Value::Points`, group view, format.
- One Opus review of the whole diff against the spec and this plan; fix findings.
- PR with the attribution lines from the session reminders; subscribe to PR activity. After Aedan says "merge":
  build from main, copy `ball-studio.exe` to `F:\!Ai Stuff\BallPhysicsEngine\bin\` keeping `ball-studio-backup.exe`,
  and `bpe-render.exe`.

## Edge cases

- How many 0 isn't possible (range 1); Group off ignores How many.
- Word with no glyph pixels (spaces only): one ball at the centre, note "No letters to place".
- Custom with no points after removing all: one ball at the centre and the + button highlighted.
- 5000 balls in the Group view: draw them all (one instance each); the gradient tool still works.
- Release with Waves larger than count: one wave.
- A Size keyframe with vary_size: factors kept (decision 8).
- Group gradient Spread by Hits/Speed/Time: the Group view shows the step-0 state (all at their start).
- Odd ones out numbers beyond the current count (count lowered): those numbers do nothing; keep them so raising the
  count brings them back.
- Paste whole look onto a ball: the target keeps its group look and placement.
- Choose with a chosen id that no longer exists: ignored.

## Things not to do

- Don't change ball positions for Grid (Block) or Ring with turn 0, or any physics default. Never re-bless goldens.
- Don't move Size out of Body or the own grid.
- Don't put placement in the Look card (it's physics).
- Don't add masks to group layers here (needs both parts; a follow-up after both merge).
- Don't build group shaders as picture passes.
- Don't add presets, Library, randomize or built-in scenes.
