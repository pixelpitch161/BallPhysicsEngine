# Plan: one way to ride a parent (W14)

Spec: `docs/superpowers/specs/2026-11-09-one-way-to-ride-design.md`. Branch `claude/project-thread-0mdbom`.

## Steps

### 1. One crossing test (`bpe-core/src/shape_path.rs`)

- Add `pub fn crosses(a: Vec2, b: Vec2, p: Vec2) -> bool`: `(a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) /
  (b.y - a.y) * (b.x - a.x)` (the exact float order every copy used, so results are bit-identical).
- `inside_polygon` uses it. Add `inside_contours(contours: &[Vec<Vec2>], p)` (even-odd over all).
- Callers: `bpe-runtime/src/letters.rs::Letter::inside` (keep its box check, then `inside_contours`),
  `bpe-kit/src/things/light/laser.rs::in_polygon` (pairs: fold with `crosses`), `bpe-kit/src/text/tiles.rs` (the
  inline test inside its distance loop calls `crosses`), `particles/births.rs::inside` (removed; outline toolbox).
  `rules/remains/shatter.rs::inside` is a half-plane clip, not this test: leave it.
- Verify: `cargo test -p bpe-core shape_path`, a unit test that `crosses` agrees with the old formula on a grid.

### 2. Outline toolbox (`bpe-kit/src/outline.rs`, `pub mod outline` in `lib.rs`)

- Move `outline_centre` → `centre`, `outline_points` → `points` (with `ROUND_POINTS` 96), `outline_contours` →
  `contours` from `light/draw.rs`. Keep thin `pub use` names in `light::draw`? No: update every caller (light draw,
  strip outline, particles births, studio `light_handles` and any others found with grep).
- `pub fn round(contours: &[&[Vec2]], share: f32) -> Option<(Vec2, Vec2)>`: share wraps (`rem_euclid(1.0)`; a share
  already in [0, 1) is unchanged, which keeps particles bit-identical), walks the contours in order by length, and in
  the last side clamps (the particles' old `round_points` algorithm: `left <= l || last`, `lerp((left / l).clamp)`,
  `l > 0` else 0). The way out is from that contour's point average (`try_normalize`, else `NEG_Y`). `None` with no
  points. `round_one(points, share)` for one contour.
- `pub fn inside(points, p)` = `shape_path::inside_polygon`; `inside_any(contours, p)` = `inside_contours`.
- Delete `light::draw::point_round`, `births::{outline_points, round_points, inside}`. Move the
  `a_point_goes_round_by_its_share` test to `outline.rs` (1.25 → wraps, as before).

### 3. The riding module (`bpe-kit/src/riding.rs`)

- `#[derive(Choice)] pub enum RidesOn { EachBall (default), TheMiddle, BallNumber, TheEdge, Inside, EachLetter,
  ThePen }`.
- `pub const RIDDEN_BY_TEXT: [&str; 3] = ["balls", "emitter", "shape"]`, `pub const RIDDEN: [&str; 4]` adding
  `"text"`. `pub fn can_ride(rider: &str, parent: &str) -> bool` (text riders use `RIDDEN_BY_TEXT`).
- `pub fn offered(rider: &str, parent: &str) -> &'static [RidesOn]`:
  balls/emitter: EachBall, TheMiddle, BallNumber, TheEdge (+ Inside for particles);
  shape: TheMiddle, TheEdge (+ Inside for particles);
  text (lights, particles): TheMiddle, EachLetter, TheEdge, ThePen (+ Inside for particles); anything else: all.
- `pub struct Ride { pub on: RidesOn, pub ball_number: u32, pub edge_at: f32 }`.
- `pub struct Spot { pub at, pub velocity, pub out: Option<Vec2>, pub ball: Option<(ThingId, usize)>, pub radius }`.
- `pub struct Letters` (a Text parent now, in world places): built by `Letters::of(at: &TextAt)` from `text_now`;
  holds the `TextNow`, `screen`, the camera (`Camera` copy) and lazily (`OnceCell`) the shown letters' middles and
  world contours, and the pen (`moves::pen_of(now, screen, camera)`, factored out of `moves::pen`). Middle = middle of
  `now.bounds()` (as lights used); velocity = the text's pose velocity.
- `pub enum Ridden<'a> { Nothing, Balls { id, bodies: &'a Bodies }, Shape { wall: &'a Wall, velocity: Vec2 },
  Text(Letters) }`. `Ridden::of(world, parent: Option<ThingId>, text: impl FnOnce(ThingId) -> Option<Letters>)`:
  ROOT/None → Nothing; bodies → Balls; a Text (by `world.kind_of`) → Text (from the closure; None → Nothing); walls →
  Shape (velocity `wall.velocity`, else the pose's, as lights had); else Nothing.
  `pub fn letters_in_world(world, id) -> Option<Letters>`: the sim's way (params from the `TextState`, else the
  saved thing; `world.tree`).
- `Ridden::spots(&self, ride) -> Option<Vec<Spot>>` (None = Nothing: the rider keeps its own place). Balls: EachBall
  every alive ball; BallNumber `row_of(n - 1)` alive; TheEdge each alive ball at `ball + from_angle(-π/2 + τ·share) ·
  r`; anything else the middle (fold sum / n of positions and velocities; empty → no spots). Shape: TheEdge
  `round_one(points(outline), share)`, else `centre`. Text: EachLetter each shown letter's middle; TheEdge `round` over
  every shown contour; ThePen the pen (none → no spots); else the middle.
- `Ridden::pick(&self, ride, dice: &Dice) -> Option<Spot>` with `Dice { pick: usize, unit: &dyn Fn(u32) -> f32 }`,
  replicating `place_for`'s riding branch exactly for balls and shapes (pick `% len`, u1 = unit(1), u2 = unit(2),
  inside tries `unit(20 + 2k)`, `unit(21 + 2k)`, 8 tries, centre fallback, middle out). Text: EachLetter random
  letter (out from the middle); TheEdge `round(all, u1)`; Inside 16 tries in the letters' box with `inside_any`, else
  a random letter's middle; ThePen the pen (None when none); else the middle.
- `Ridden::count(&self, ride) -> usize`: balls one by one (EachBall, TheEdge, Inside) → alive balls; EachLetter on a
  Text → shown letters; else 1.

### 4. Lights (`things/light/mod.rs`, `draw.rs`)

- Remove `light::RidesOn`; `rides_on: crate::riding::RidesOn`; `edge_at` `when = "rides_on=the_edge"`; `RIDES` →
  `riding::RIDDEN` (rig `fixtures::rides_anything`, studio `lights_panel`, text `sim.rs`/`draw.rs` use
  `RIDDEN_BY_TEXT`).
- `places_in(world, parent, s)` → `Ridden::of(world, parent, |id| letters_in_world(world, id))` then spots → `Place`
  (`skip` = the ball's radius, `ball` = (id, row)). `places(cx, s)`: for a Text parent build a `TextAt` with scene,
  log, sound and look (as `text_places` did) and `Letters::of`; Points At the pen still aims at the pen. Remove
  `text_places`.
- Light `offers` (`ThingKind` impl) → `riding::offered`.

### 5. Particles (`things/particles/mod.rs`, `births.rs`, `sim.rs`)

- Remove `particles::RidesOn`; `RIDES` → `riding::RIDDEN`; `looks.rs::can_ride` uses it.
- `BirthCx` gains `ridden_text: OnceCell<Option<Letters>>` (all four builders) so a Text parent is laid out once a
  step. `place_for`'s RidingItsParent → `Ridden::of(...)`, `pick`, `Spot` → `Place` (`ball` = row + 1, impact 1).
  `riding_count` → `Ridden::count`.
- `physics_print_extra`: riding a Text parent → `letters_hash(scene, parent)` mixed with its transform, so an edit
  to the Text restarts the particles.
- Particles `offers`.

### 6. Text (`things/text/mod.rs`, `draw.rs`, `sim.rs`)

- Remove `TextRides`; `rides_on: riding::RidesOn`; new `edge_at` ("Round the edge", `when = "rides_on=the_edge"`,
  `span(0, 1)`, keyframe, group Riding). Update the settings-order test.
- `riding_places` → `Ridden::of(world, parent filtered by RIDDEN_BY_TEXT, |_| None)` → spots → poses (screen
  conversion and Position offset as before). `fast_letters` Automatic counts EachBall or TheEdge on balls.
- Text `offers`.

### 7. The editor (`bpe-core/src/registry.rs`, `bpe-studio`)

- `ThingKind::offers(&self, scene: &Scene, thing: ThingId, key: &str, choice: &str) -> bool { true }`.
- `GridCx.offers: Option<&dyn Fn(&SettingDef, &str) -> bool>` (None everywhere but the thing's own settings grid in
  `inspector.rs`); `widget` takes it and skips choices not offered (the current one still shows as selected).
- Placement card: Item → "Rides on" (The whole group / One ball) + "Ball number" (1-based) when One ball.
- `tree.rs::particles_parent` and `lights_panel::light_parent` use `RIDDEN`.

### 8. Tests

- `riding.rs` unit tests: each choice on balls, a shape and a Text gives the spots in the table; fallback to the
  middle; `offered` per rider and parent; `pick` on balls equals the old code's numbers for every choice (copy the
  old formulas into the test); `count`.
- Particles: a Particles thing riding a Text's Each letter / The edge / The pen is born on the letters (positions
  inside the text's box; pen spots follow Draw on). Editing the Text's words changes the fingerprint.
- Lights: a light on Each letter gives one lamp a letter; The edge on balls one a ball on its rim.
- Text: a Text on a shape's edge sits on the outline at its share.
- Studio kittest: a light inside a shape offers only The middle and The edge; the Placement card shows Ball number
  counted from 1.
- `restarts.rs` covers the new Text `edge_at` (draw only).
- Full-size look check: a new `riding` shot (lights on each letter of a Text, particles from its edge, a light on a
  ball's rim, a label on a shape's edge).
- Goldens: run all `golden*` tests in debug and release; they must pass unchanged (no re-bless).

## Not to do

- Don't change `Attach::Ride`'s stored `item` meaning (the runtime and goldens read it).
- Don't touch shatter's half-plane `inside`.
- Don't let a Text ride a Text.
