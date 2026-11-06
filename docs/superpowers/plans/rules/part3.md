# Rules part 3: break looks (plan)

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md` (sections 5.4, 6 "Break looks", 8 "Shatter pieces and the
other Break looks: picture tests", 9 part 3). Part 1 (`part1.md`, PR #124) made `do.break` with Vanish only. This part
adds the looks **Fade out**, **Pop** and **Shatter**. They are drawn only: the physics, the log and every golden stay
exactly as they are. An "As built" section at the end records anything that moved.

## Goal

A Break block picks how the broken thing goes:

| Look | What it does | Settings (key, default) |
| --- | --- | --- |
| Vanish | gone at once (part 1) | none |
| Fade out | fades to nothing | Time `fade_time` 0.4 s |
| Pop | swells, then fades | Time `pop_time` 0.3 s, Swell `swell` 1.5× |
| Shatter | cut into pieces that fly apart from the hit, fall with the scene's gravity, bounce off other shapes, spin and fade | Pieces `pieces` 12, Spread `spread` 300 px/s, Spin `spin` 360°/s, Gravity `gravity` 1×, Bounce `bounce` 0.4, Hit shapes `hit_shapes` on, Fade after `fade_after` 0.8 s, Fade over `fade_over` 0.6 s |

Gravity, Hit shapes and Fade over are additions to the spec's list (the owner wants every look adjustable): Gravity
scales the scene's gravity for the pieces, Hit shapes off lets them fall through everything, Fade over is how long
the fade after Fade after takes.

The thing stops simulating at once, as in part 1 (its wall is gone, its balls are removed with `Remove` events, it
leaves the order). Only its picture plays out. Scrubbing back before the break shows it whole again (the record lives
in the world, which snapshots clone).

What shows a look:

- **Shapes**: all three. Shatter pieces wear the shape's own look: each piece is the whole shape drawn again, moved
  and turned with the piece and **clipped** to that piece's cell, so every layer (paint, gradient, outline, glow,
  shaders, sub-layers) shows exactly the part of the picture that piece was.
- **Balls** (one ball, the Group's balls, or every ball of a broken Balls or Emitter thing): all three, the same way
  (each piece is the ball's own layered instance, clipped).
- **Lights**: Fade out fades its strength; Pop flares it by Swell while fading; Shatter holds it until Fade after then
  fades over Fade over (a light has no pieces).
- Anything else (camera, folder, rig, scene settings): the look is Vanish whatever is picked.

## Files

| Crate | File | What |
| --- | --- | --- |
| bpe-runtime | `src/rules/remains.rs` (new) | `BreakShow`, `Remains`, `RemainsOf` |
| bpe-runtime | `src/rules/actions.rs` | `Action::Break` gains `show: BreakShow`; applying a break records the remains |
| bpe-runtime | `src/rules/mod.rs` | `RulesState.remains`, `remains_of`, `showing`, `changes_picture`, pruning |
| bpe-gfx | `src/frame.rs` | `PathInstance.clip`, `LayeredInstance.clip`; `build_frame_sensing` keeps broken things whose look still shows |
| bpe-gfx | `src/lib.rs` | `PATH_ATTRIBUTES` and `LAYERED_ATTRIBUTES` gain the clip attribute |
| bpe-gfx | `shaders/paths.wgsl`, `shaders/layers.wgsl`, `shaders/shape_layers.wgsl` | the clip: quad fitted to the clip polygon, cover × clip × (1 − fade) |
| bpe-kit | `src/rules/breaks.rs` | the looks, `LookSettings`, `show_for`, `lasts` |
| bpe-kit | `src/rules/remains/` (new) | `mod.rs` (how a look shows at an age), `shatter.rs` (cells, pieces, their motion) |
| bpe-kit | `things/shape/draw.rs` | a broken shape draws its remains |
| bpe-kit | `things/balls/draw.rs` | a broken ball row draws its remains |
| bpe-kit | `things/emitter/draw.rs` | a broken emitter's marker is gone |
| bpe-kit | `things/light/draw.rs` | a broken light's strength follows its look |
| bpe-kit | `sensing/probe.rs` | keeps showing lights as the frame does |
| bpe-kit | `tests/break_pictures.rs` (new) | GPU picture tests of the looks |

## 1. Runtime: the remains (bpe-runtime `rules/remains.rs`)

```rust
/// How a break's look plays out: the look's settings (the kit's drawing reads them; the runtime
/// never does) and how long it shows after the break, in seconds (0: gone at once, Vanish).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakShow { pub look: Params, pub lasts: f64 }

/// What is left of a broken ball or thing while its look plays out.
#[derive(Clone, Debug, PartialEq)]
pub struct Remains {
    pub step: u64,            // the step it broke on
    pub time: f64,            // that step's time
    pub point: Vec2, pub normal: Vec2, pub strength: f32,   // the hit, as the Break event has them
    pub show: Arc<BreakShow>, // shared by every ball of one break
    pub gravity: Vec2,        // the world's gravity when it broke (pieces fall by it)
    pub wall: Option<Wall>,   // a shape's wall as it stood
    pub ball: Option<BallAt>, // a ball as it was
}
pub struct BallAt { pub position: Vec2, pub velocity: Vec2, pub radius: f32 }
```

- `RulesState.remains: BTreeMap<(ThingId, u32), Remains>`, keyed by thing and body row (`WHOLE` for the thing).
  Only breaks with `lasts > 0` are kept.
- Applying `Action::Break` (`actions.rs`):
  - a row: before `pop`, `remains.insert((thing, r), Remains { ball: Some(BallAt { .. }), .. })`.
  - a whole thing: the wall is **taken** out of `walls` (not dropped) into the thing's record; every live ball gets
    its own row record (before its `pop`); the thing's own record (`WHOLE`) is made too, so the frame knows the
    thing still shows.
- `RulesState::prune_remains(time)` drops records whose `time + lasts < now` (at the step's start, beside the other
  pruning in `apply_rule_actions`). Nothing reads them in the physics.
- `RulesState::remains_of(thing, row: Option<u32>) -> Option<&Remains>`, `remains() -> impl Iterator`,
  `showing(thing, time) -> bool` (its `WHOLE` record with `time < r.time + lasts`).
- `changes_picture()` stays true while anything is broken (part 1 already).
- Fingerprint, goldens: unchanged (the remains never touch the physics; the Break event is as it was).
- `World::lone` drops rules (part 1), so remains too.

Not doing: putting the look in the `Break` event (`b` stays `None`), so every log reader and golden is untouched.

## 2. Kit: Break's looks (`rules/breaks.rs`)

- `BreakLook { Vanish, FadeOut, Pop, Shatter }` (choice names `vanish`, `fade_out`, `pop`, `shatter`).
- `BreakSettings` keeps `who` and `look`, plus the look settings with `when = "look=…"` so only the picked look's
  show: `fade_time` (Time, `range(0, 1e6)`, `span(0, 3)`, s), `pop_time`, `swell` (`range(0, 1e3)`, `span(1, 4)`, ×),
  `pieces` (Int `range(2, 256)`, span 2..64), `spread` (`range(0, 1e6)`, `span(0, 2000)`, px/s), `spin`
  (`range(-1e6, 1e6)`, `span(0, 1440)`, °/s), `gravity` (`range(-1e3, 1e3)`, `span(0, 3)`, ×), `bounce`
  (`range(0, 1)`), `hit_shapes` (switch), `fade_after` (`span(0, 5)`, s), `fade_over` (`span(0, 3)`, s).
  Times are `f64` like part 1's.
- `LookSettings` (the same look keys without `who`, `#[derive(Settings)]`) is what the drawing reads from
  `BreakShow.look`; Health (part 4) can hand its own card's params the same way.
- `show_for(settings, kind) -> BreakShow`: Vanish, or a kind without looks (anything but `shape`, `balls`,
  `emitter`, `light`), is `BreakShow::default()` (lasts 0). Otherwise `look` = `LookSettings` as params and
  `lasts` = Fade `fade_time`, Pop `pop_time`, Shatter `fade_after + fade_over` (each at least 0; a non-finite one 0).
- `act` puts `show` on each `Action::Break`, one `Arc` a target kind.

## 3. Kit: how a look shows (`rules/remains/mod.rs`)

```rust
pub struct Showing { pub alpha: f32, pub scale: f32 }   // at an age after the break
pub fn showing(look: &LookSettings, age: f64) -> Option<Showing>   // None once it is over (or before it began)
```

- Fade out: `u = age / fade_time`; alpha `1 − smoothstep(u)`; scale 1.
- Pop: `u = age / pop_time`; scale `1 + (swell − 1)(1 − (1 − u)²)` (eases out); alpha `1 − u²`.
- Shatter (the whole, for lights): alpha 1 until `fade_after`, then `1 − smoothstep((age − fade_after)/fade_over)`.
- A zero time is over at once.

## 4. Kit: Shatter (`rules/remains/shatter.rs`)

**Cells.** `cells(material, bound, n, seed) -> Vec<Piece>` with `Piece { site: Vec2, cell: Vec<Vec2> }` in the thing's
own units (shape units; for a ball, radii in its quad's frame):

- Sites: `n` seeded points on the drawn material (`Material`): `Filled(points)` (rejection in the bounding box with
  even-odd inside, 32 tries, else the centroid side of a random corner), `FilledEllipse(axes)` (area-uniform),
  `Band { points | ellipse, half }` (a random place along the outline, pushed across it by up to the band's half),
  `Disc` (a ball). Seeded by `hash(thing, row, step)`.
- Each cell is the square `[−bound, bound]²` clipped (Sutherland–Hodgman against one half-plane at a time, convex)
  by the perpendicular bisector toward every other site: the Voronoi cell, convex. `bound` covers everything drawn
  (the outline's reach plus how far its layers reach past it, or a ball's stack reach), so the cells tile it and the
  pieces together are exactly the whole picture. Two sites at the same place: the later one is skipped. A cell with
  fewer than 3 corners is dropped.

**Motion.** `piece_at(piece, k, start: PieceStart, look, age, walls) -> PieceNow { at: Vec2, turn: f32, alpha }`:

- Its site's world place at the break `p0` (the thing's pose turned and scaled), its radius for bouncing
  `sqrt(material area / (n π))` (world pixels), its velocity `v0` = the thing's own velocity + the way away from where it
  was hit (the hit point when it lies within twice the reach of the middle, else the middle; a piece on that point
  goes a seeded way) turned by up to ±25° (seeded) × `spread × (0.6 + 0.8u)`; spin `spin × (0.5 + u)` with a seeded
  sign; gravity `start.gravity × look.gravity`.
- No walls (Hit shapes off, or none): exact, `p0 + v0 t + ½ g t²`. Otherwise particles' stepping
  (`particles::sim::steps_for`: 1/120 s, at most 120 steps) with `keep_clear` and `reflect(.., bounce)` against the
  world's walls at the moment drawn (the broken one is gone from them), and the particles' `on_its_side` first.
- alpha as Shatter's whole (section 3).
- Pure in time: the same at any frame, whatever came before.

**Drawing a piece.** Each piece is the thing's instance again, its `clip = [first point, count, fade, 0]` naming its
cell in `Frame.path_points` (`[x, y, 0, 0]` each), its pose moved so that its site lands at `at` turned by `turn`:

- shape: `turn' = θ0 + turn`, `centre' = at − R(turn')·site·scale`.
- ball: the quad's direction `R(turn)·(1, 0)`, `centre' = at − R(turn)·site·radius`, velocity 0 (no stretch).

Records, outline points and programs are shared by every piece (written once).

## 5. GPU: the clip (bpe-gfx)

- `PathInstance.clip: [f32; 4]` and `LayeredInstance.clip: [f32; 4]`: first clip point in `path_points`, how many
  (0: no clip), how far it has faded (0 shows it whole), spare. Zeroed by default: nothing else changes.
  `LayeredInstance` grows to 176 bytes (the size test's bound moves from 160 to 176).
- Attributes: `PATH_ATTRIBUTES` gets `6 => Float32x4`, `LAYERED_ATTRIBUTES` `12 => Float32x4` (13 of 16).
- `paths.wgsl`: `clip_box(first, count) -> vec4` (the clip polygon's bounds) and `clip_cover(q, first, count,
  px_units) -> f32` (convex: the largest signed distance to its edges, `clamp(0.5 − d / px, 0, 1)`). Both loops run
  only when `count > 0`.
  - `vs_path`: with a clip, the quad is the clip's bounds (shape units, turned and scaled, two pixels' margin), not
    the whole reach, so a shattered shape costs about what the shape does.
  - `shade_path`: cover × clip × (1 − fade), before `hard_tags` (`shade_shape_layers` takes the factor);
    `fs_path_object` multiplies its cover.
- `layers.wgsl`: `vs_layers` fits the quad to the clip's bounds the same way (in the quad's own frame: `local` is
  the corner); `shade_layers` multiplies `out` by clip × (1 − fade) where it already applies `misc.x`;
  `fs_layers_object` too.
- No clip and no fade: the same instructions' results as today (× 1.0), so every picture hash stays.

## 6. Drawing the remains

- `build_frame_sensing`: a broken thing is left in the frame's copy while `rules.showing(id, time)`; the probe's
  `work_out` does the same.
- **Shape** (`ShapeDraw::draw_part`): with no wall, `remains_of(id, None)` gives the wall as it stood; with none
  (or the look over) it draws nothing. Fade/Pop: one instance, `scale × s`, `half × s`, `clip[2] = 1 − alpha`.
  Shatter: the instance is pushed once to get its outline and records, then copied per piece with its pose and clip
  (the first copy replaces it). `shape_hits` and `light` work as they do.
- **Balls** (`balls/draw.rs::draw_part`): a dead row with remains showing at this moment draws from `BallAt`
  (velocity 0): Fade/Pop one instance (`radius × s`, `misc[0] × alpha`), Shatter one per piece. These go after the
  ghosts, in the same run (same records, same program), never into ghosts, ball marks or the Stage spheres.
- **Emitter**: its marker isn't drawn once it is broken.
- **Light**: `light()` reads `remains_of(id, None)`; strength × alpha × scale (Pop flares as it swells).

## 7. Tests (what passing looks like)

- runtime (`rules/tests.rs`): a break with a show keeps remains (the ball's place and size, a shape's wall) and the
  thing `showing` until `time + lasts`, then pruned; a Vanish keeps none; a seek back before the break has none and
  replays to the same remains; a scene without rules has none.
- kit unit (`rules/remains/` tests): `showing` per look (Fade half at half time, Pop larger then gone, Shatter holds
  then fades); cells tile the square (areas add up within 1e-3, each site inside its own cell, convex), are the same
  for the same seed and differ for another; pieces fall by gravity exactly with no walls; with a floor they stay
  above it; pure in time (worked out twice equal).
- kit draw (`rules/tests.rs`): a shape broken with Fade out draws one path with `clip[2]` about 0.5 at half time and
  nothing after; Pop draws it bigger; Shatter draws `pieces`-ish paths sharing one records range, each with a clip;
  a ball broken with Shatter draws clipped layered instances in the thing's run; Vanish draws nothing (part 1's test
  still passes); a light broken with Fade out shines at about half strength at half time.
- GPU pictures (`tests/break_pictures.rs`, skipped without an adapter): Fade out at half time is about half as bright
  where the shape was; Pop at mid time covers pixels outside the shape's old outline; Shatter shortly after the break
  shows coloured pixels in the pieces and gaps between them, later the pieces are below where the shape was (they
  fell); every piece shows the shape's own colour (a two-colour Across gradient: its left pieces stay the left
  colour). A frame with no break is byte-identical with and without the clip code (the existing hash tests:
  `unlit_pictures`, `lit_flat_pinned`, `pictures`).
- Every golden unchanged (`golden*.txt`, `golden-rules*.txt`, sound goldens); `cargo clippy --workspace` clean;
  `cargo fmt`.

## Not doing

- No physics for pieces (they never push balls or change anything simulated); no piece-on-piece bouncing.
- No editor work: part 2 builds the block inspector from the settings schema, so the look's settings show there
  with their `when`s. The block's face summary (e.g. "Shatter, 12 pieces") lands after part 2.
- No look for cameras, folders, rigs.
- No new log event and no golden change.

## As built

- As planned. `BreakSettings` holds every look setting with its `when`; `LookSettings` reads the same keys.
- `RulesState::prune_remains` runs at the start of `apply_rule_actions`, so a world only holds remains still showing.
- `Remains::shows_at` has no lower bound (a frame drawn a hair before the break step's time counts as the break) and
  `age` never goes below 0.
- The clip lives in `paths.wgsl` (`clip_bounds`, `clip_cover`, `clip_keep`, `clips`, `clip_corner`), which comes before
  `layers.wgsl` in the scene module, so balls use the same functions. Group 2 binding 4 (`path_points`) is visible to
  the vertex stage now.
- `particles::sim::on_its_side` is `pub(crate)` for the pieces.
- Tests: runtime `a_break_with_a_look_keeps_its_remains_until_the_look_is_over`; kit `rules/remains/tests.rs` (looks,
  cells, sites, motion, bounces) and five draw tests in `rules/tests.rs`; `tests/break_pictures.rs` (Fade, Pop,
  Shatter of a two-colour square, Shatter of a ball, the same picture however it got there).
- After the Opus review: pieces start where they are (no `on_its_side`: each site is on its own side of every wall,
  and `keep_clear` keeps it there); a broken ball's instance is made as it was at the break (its Turning stops, it
  faces the way it was going, no stretch); the light probe drops every broken thing at once (it is also the physics'
  probe, and looks are drawn only); `showing` is true while any record of the thing shows (a ball shattered earlier
  outlives a later whole Vanish); a shape with no layered Look card still fades and shatters (`wear_layers`); cells
  reach 4 px past what is drawn (`MARGIN`); a break's balls share one cut (`Cuts`), each turned its own seeded way.
- Known: pieces that bounce are flown afresh each frame against the walls as they stand then (like particles), so
  with moving walls a bounce can shift between frames; every scene now reads `path_points` in the vertex stage
  (fine on DX12, Vulkan and Metal).
