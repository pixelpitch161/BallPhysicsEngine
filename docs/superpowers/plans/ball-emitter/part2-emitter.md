# Ball Emitter part 2: the Emitter thing — plan

Date: 2026-11-05. Spec: `docs/superpowers/specs/2026-11-05-ball-emitter-design.md`, section 5 (design approved,
Emitter card layout C). Branch `claude/project-thread-rejn7j`. Part 1 (Ellipse) builds in parallel and merges
first; this part works on circles from day one and wires ellipse foci after rebasing on part 1 (task 12).

The design is approved, so this plan is built straight through, reviewed once by an Opus subagent at the end, and
sent as one PR. No screenshots or videos.

## 0. Decisions this plan makes (spec gaps)

1. **When the first round fires.** "After" (`start_delay`) is the first round's time for every Fire when. At
   start fires only that one round (Rounds, Each round ×/+ and Earlier rounds are hidden then). Every fires the
   first round at After, then one every `every` seconds. When a wave lands on fires the first round at After only
   when **Also at start** (`wave_first`, default on) is on, then one round per landing. Without it, an emitter
   that waits on itself would never start.
2. **Sits on** is two keys, not one: `sits_on` (an Int thing id, 0 = Free) and `sits_focus` (Focus 1 / Focus 2).
   The Emit tab draws them as one menu ("Free", "<shape name> · Focus 1", "· Focus 2"). Until part 1 is in, only
   circles are listed (both foci are the middle). A shape that is gone, or isn't one we can sit on, means Free.
3. **Rounds** is `rounds` (1..1000) plus a switch `endless` ("Endless"), so the slider stays a plain number.
4. **Catch size** is a radius (like ball Size): how far from the emitter's middle a ball's centre is caught.
5. **Send back out** — Same way back: the ball stays where it crossed the catch circle and its velocity is
   reversed. Out my way: the ball is moved to this emitter's start place for its arrival order k (of the round's
   size n) and fired with this emitter's direction and Speed (with Speed spread), exactly as `fire` would. With a
   Delay it waits at that place, held still (`Bodies.held`), and leaves with that velocity when the time comes.
6. **Held balls** get a new `Bodies.held: Vec<bool>` column in `bpe-runtime`: a held ball is not integrated,
   not collided with walls, not paired, and is never caught. Nothing in today's scenes holds a ball, so goldens
   don't move.
7. **Directions are absolute angles plus the emitter's own turn** (its pose angle: transform rotation, a parent it
   rides with Turn with parent). Launched balls don't take the pose's velocity (so a riding emitter fires at its
   own Speed), unlike Balls.
8. **Launches use the emitter's pose at the Spawn stage** (where the scripted stage left it the step before). An
   emitter is built and scripted after every shape (`needs_walls() = true`), so Sits on follows a spinning or
   shrinking shape within the same step's scripted stage.
9. **Speed spread** works as the Balls' one: × (1 + spread·(u − 0.5)), 0.5..1.5 at full spread. **Size spread,
   Colour spread, Turn spread and Seed** are the emitter's Variety fold (Group mode), as on Balls; every ball's
   rolls are by its ball number.
10. **Ball numbers** count up across rounds in launch order. Balls cut by Most balls take no number.
11. **Round spread** (group gradient): `t = (round − 1) / ((rounds − 1) · stretch)` with finite rounds; with
   Endless, `t = frac((round − 1) / (10 · stretch))`, so endless rounds go round the gradient every 10 rounds
   (× Stretch). Balls things: round 0 → t = 0. **Launch angle**: `t = frac((launch° − angle°) / 360)` — the
   gradient's Angle is where the wheel starts. Angle shows for Place and Launch angle; Stretch for Round too.
12. **Order spread on an emitter** counts against the planned total (`planned`: the sum of all rounds' sizes
    when rounds are finite and Fire when isn't a wave, capped at Most balls; otherwise the balls fired so far),
    so early balls keep their colour as later rounds come.
13. **Group places of an emitter** (what the group gradient's Place and Centre out read, and what Group mode
    shows) are one round's layout: ball k's start place plus `LAYOUT` along its direction, with `LAYOUT =
    max(6 · Size, 48)` px. Each row keeps its own layout place (`BallRows.place`).
14. **Arrivals count as hits for the ball's reactions only**: Flash on hit (reaction and sub-layer), Colour on
    hit (reaction and sub-layer; `BallRows.arrivals` adds to the hit count the look reads). Never for sound,
    physics, `Bodies.hits`, Grow per hit or Pops after.
15. **Mini view rays** take each ball's colour from the Body's first colour mixed with the group gradient
    (`emitter::round_colours`), not a full look evaluation. Marked `// scene colour` in the editor.
16. **Rounds strip** shows the rounds fired so far (from the playback's world, `EmitterState.fired`), and, for
    At start and Every with finite rounds, the rounds still to come as outlined blocks at their planned times.
17. **Camera Follow "Balls"** (`Target::Kind("balls")`, the default) also follows emitters' balls. Other targets
    are as before.
18. **Spawn and Enter events**: two new `EventKind`s with tags 8 and 9 (`Spawn`, `Enter`). Spawn: `a` = the ball
    (emitter, row), `b` = `(emitter, round)`, `point` = where it starts, `normal` = its direction, `speed` = its
    speed. Enter: `a` = the ball (owner, row), `b` = `(catcher, round of the ball)`, `point` = where its centre
    crossed the circle, `speed` = its speed, `at` = 0. Every reader of the log already filters by kind (checked:
    sound hits, control sampler, slots, camera, timeline, frame hits, paint eval, particles, shape draw).

## 1. Files

New:
- `crates/bpe-kit/src/things/emitter/mod.rs` — `EmitterKind`, `EmitterSettings` and its enums, headings, `KIND`.
- `crates/bpe-kit/src/things/emitter/fire.rs` — pure maths: directions, start places, order, round sizes,
  layout, seeds. No world.
- `crates/bpe-kit/src/things/emitter/sim.rs` — `EmitterSim`, `EmitterState`, firing, catching, fades, holds.
- `crates/bpe-kit/src/things/emitter/draw.rs` — `EmitterDraw` (balls through the shared Balls drawing, then the
  marker, flashes and ripples).
- `crates/bpe-kit/src/things/emitter/foci.rs` — `focus_points(&Outline)`, `can_sit_on(&Thing)`, `sits_on_pose`.
- `crates/bpe-kit/src/things/balls/rows.rs` — `BallRows` and `GroupOf` (what drawing and sound read about any
  thing with ball rows).
- `crates/bpe-kit/src/modules/look/layers/wavefront.rs` — `WAVEFRONT`, `WavefrontLayer`, its draw.
- `crates/bpe-studio/src/emitter_card/mod.rs` — the Emitter card: chips, tabs, rows, hints.
- `crates/bpe-studio/src/emitter_card/mini_view.rs` — the 170 px aim/fan/speed view.
- `crates/bpe-studio/src/emitter_card/strip.rs` — the rounds strip.
- `crates/bpe-kit/tests/emitter.rs` — the sim tests that need the whole kit (or keep them in `sim.rs`).

Changed: `bpe-runtime` (`bodies.rs`, `events.rs`, `pairs.rs`, `world.rs`, `lib.rs`), `bpe-kit` (`lib.rs`,
`things/mod.rs`, `things/balls/{sim,draw}.rs`, `modules/look/layers/{mod,group,around,trail/draw}.rs`,
`modules/look/layers/particles/draw.rs` if it reads `Balls`, `starters.rs`, `things/camera/sim.rs`,
`scenes.rs`), `bpe-sound` (only through `balls::sim::numbers`/`ball_number`, which now read `BallRows`),
`bpe-studio` (`inspector.rs`, `look_editor/{group,group_view,ball_view,mod}.rs`, `preview.rs`, `tree.rs`,
`timeline/{lanes,mod}.rs`, `app.rs`), `CLAUDE.md`.

## 2. Runtime (bpe-runtime)

### 2.1 `Bodies.held`
- `pub held: Vec<bool>` after `number`: "Held still by something (a catcher waiting to send it back out): not
  moved, not bounced off walls or other balls". `push` pushes `false`; `memory` counts it.
- `world.rs::move_bodies`: integrate only `alive[i] && !held[i]`.
- `world.rs::collide`: skip `held[i]` beside `!alive[i]`.
- `pairs.rs` gathering (line ~235: `if b.alive[i]`) → `if b.alive[i] && !b.held[i]`.
- Not part of the fingerprint or the log. Test: `a_held_ball_stays_put_through_gravity_walls_and_pairs`.

### 2.2 Events
- `EventKind::Spawn` (tag 8) and `EventKind::Enter` (tag 9), doc comments as in decision 18. Update the tag test.
- `pairs::pop` becomes `pub` and is re-exported from `lib.rs` (catching and fades remove balls with it).

## 3. Shared ball rows (bpe-kit `things/balls/rows.rs`)

```rust
/// What every thing with ball rows (Balls, Emitter) keeps per row beyond `Bodies`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BallRows {
    pub numbers: Vec<u32>,       // ball number of each row
    pub size_factor: Vec<f32>,   // share of Size (Variety)
    pub round: Vec<u32>,         // emitter round, 1-based; 0 for Balls
    pub launch: Vec<f32>,        // direction fired in, radians (Balls: its launch velocity's angle)
    pub place: Vec<Vec2>,        // its place in the group (Balls: offsets[number]; Emitter: layout place)
    pub arrivals: Vec<u32>,      // times caught (counts as hits for its look's hit reactions)
    pub fade_from: Vec<Option<u64>>, // step its fade out began (emitter Fade out)
}
impl BallRows { pub fn push(&mut self, number, factor, round, launch, place); pub fn number(&self, row) -> u32; pub fn len() }
```
- `BallsState` swaps `numbers` and `size_factor` for `rows: BallRows`; `BallsState::number(row)` stays.
  `push_ball` pushes the row with round 0, launch = `velocity.to_angle()` of the launched velocity (0 for a still
  ball), place = `offsets[number]`.
- `pub fn rows(world, thing) -> Option<&BallRows>` and `rows_mut` downcast `BallsState` or `EmitterState`.
- `balls::sim::numbers` and `ball_number` read `rows()` (so sound's By ball, Hit modulator ball number, riding an
  item and the camera work for emitters unchanged).
- `GroupOf<'a> { rows: &'a BallRows, places: &'a [Vec2], count: usize, seed: i32, rounds: RoundSpan }` from
  `group_of(world, thing)`: Balls → places = offsets, count = offsets.len(), rounds = one; Emitter → places =
  `EmitterState.layout`, count = `planned` (decision 12), rounds = finite n or endless.
- `pub fn variety(kind, params) -> BallsSettings`: a Balls thing's own settings; for an emitter a stand-in with
  `group: true`, `radius`, `seed`, `vary_size/speed/colour/turn` from its settings (what `colour_turn`,
  `look_turn`, `size_factor` read).

## 4. Look changes (bpe-kit)

### 4.1 `look.layers` fits emitters
- `LayersLookKind::fits`: `"balls" | "emitter"`. `starters::starter_kind`: Look for `"balls" | "emitter"`.
- `look/mod.rs:53` and `paint/inside.rs:331` apply settings as for `Some("balls")`: leave (they are per-card
  `when` checks with the thing kind; an emitter must read like balls) — change both to pass `"balls"` when the
  thing is an emitter. Check `def.applies` users in `look_editor` that pass the thing kind: map `emitter` to
  `balls` through one helper `bpe_kit::look_kind(kind) -> &str` so `thing=balls` conditions hold on emitters.

### 4.2 Group gradient spreads
- `Spread` gains `Round` and `LaunchAngle` (after `Time`, so saved choices keep their names). `BallInGroup` gains
  `round: u32`, `rounds: RoundSpan` (`Finite(u32)` / `Endless`), `launch: f32` (radians). `group_t` per decision
  11. Settings' `when`: Angle `spread=place|launch_angle`, Stretch `spread!=order|random|launch_angle`.
- Tests: `round_runs_first_to_last_and_endless_wraps_every_ten`, `launch_angle_goes_round_the_gradient_from_the_angle`.

### 4.3 Balls drawing generalised (`things/balls/draw.rs`)
- `GroupCx` holds `Option<GroupOf>` instead of `Option<&BallsState>`; `look` from `variety()`. `ball()` fills
  round, rounds and launch from the rows.
- `last_hits`: also reads `EventKind::Enter` with `e.a.thing == id` (the ball side only).
- Hit counts the look reads (`recoloured(..)`, `ball.hit[2]`, `Balls::hit`) are `bodies.hits[i] + arrivals[i]`.
- Flash on hit reaction: Enter events of its balls flash too (at `e.point`, strength from `e.speed`).
- **Fade**: `fade(i)` = 1 − (now − fade_from·dt)/fade_time, clamped; 1 without one. A fading ball's instance
  `misc[0] *= fade`; `colours(..)` (and so trails, particles, wavefront) scale alpha by it (light-adding colours,
  alpha < 0, scale their rgb). The emitter's fade_time reaches the draw through `GroupOf.fade_steps`.
- `BallsDraw`'s `blend`, `draw`, `draw_over`, `around`, `draw_around` work for any thing with ball rows; the
  emitter's `EmitterDraw` calls them, then draws its marker.

### 4.4 The Wavefront layer (`layers/wavefront.rs`)
- `WAVEFRONT = "layer.wavefront"`, an `around_kind` ("Wavefront", "A line through each round's balls, in
  order: a wavefront.") registered after Particles.
- `WavefrontLayer` settings: `place: AroundPlace` (Where, default Behind), `width` (px, 0.5..40, default 2,
  keyframe), `source: TrailSource` (Ball / Own colour), `colour: Gradient` (when own; runs along the line by
  number), `softness` 0..1 (0.5), `opacity` 0..1 (1, keyframe), `closed: Closed { Auto, Yes, No }`, `break_apart`
  px (0..4000, 200: a gap longer than this breaks the line; 0 never breaks), `blend: Blend` + `blend_amount`
  as the trail's.
- `AroundKind::Wavefront(Box<WavefrontLayer>)`; `around_layers` reads it (balls only, not shapes); `place()` and
  `blend()` cover it; `around_history` needs nothing for it.
- Draw (`draw_around` in balls draw): group live rows wearing it by round (Balls: one group), sort each by number,
  split where neighbours are further apart than `break_apart`, close Auto when the emitter's Directions is All
  round (Balls: when its shape is Ring or Filled ring), push strips through the trail's strip-with-subs helper
  (`trail::draw::strip_with_subs`, split out of `BallCx::strips`), colour per point = the ball's seen colour (or
  the gradient at its place along the line) × opacity × its fade. At most `MOST_STRIP_POINTS` points.
- Tests: `a_wavefront_draws_one_closed_strip_per_round_in_number_order`, `a_gap_wider_than_break_apart_breaks_it`.

## 5. The Emitter (bpe-kit `things/emitter/`)

### 5.1 Kind and settings
`EmitterKind`: id `emitter`, family "Bouncers", name "Emitter", icon "emitter", help "Fires balls it designs
itself, catches balls that reach it, or both.", slots and default slots Look, Motion, Bounce.

Headings (each setting's `group`): `"Fire"`, `"Rounds"`, `"Aim"`, `"Catch"`, `"Send back out"`, `"Marker"`,
`"Flash"`, `"Ripple"`, `"Variety"`; `EMIT_HEADINGS = [Fire, Rounds, Aim]`, `CATCH_HEADINGS`, `MARKER_HEADINGS`.

`EmitterSettings` (key, label, type/range, default, `when`):
- Fire: `emits` Emits bool true; `radius` Size 0.5..200 px 6 simple keyframe (shown by the Look card's switch, not
  in the Emit tab); `sits_on` Sits on Int 0 (custom row); `sits_focus` Focus1/Focus2 (custom row);
  `start_from` Point/Ring/Line; `start_radius` Radius 1..4000 px 60 `start_from=ring`; `start_length` Length
  1..4000 px 300 `start_from=line`; `directions` AllRound/Fan/OneWay/Random; `aim_angle` Aim direction
  -180..180 ° -90 keyframe; `fan_angle` Fan angle 0..360 ° 90 `directions=fan` keyframe; `speed` Speed px/s
  span(50, 2000) range(0, 20000) 600 keyframe; `vary_speed` Speed spread 0..1 0; `count` How many 1..20000 10
  keyframe; `gap` Gap range(0,30) span(0,1) s 0 keyframe; `order` Around/Random/MiddleOut `gap>0`.
- Rounds: `fire_when` AtStart/Every/WhenAWaveLands; `start_delay` After 0..600 s 0; `every` Every 0.05..600 s 4
  `fire_when=every` keyframe; `wave_from` On Target Me `fire_when=when_a_wave_lands`; `wave_first` Also at start
  bool true `fire_when=when_a_wave_lands`; `rounds` Rounds 1..1000 1 `fire_when!=at_start and !endless`;
  `endless` Endless false `fire_when!=at_start`; `grow_times` Each round × 0.1..100 1 `fire_when!=at_start`;
  `grow_plus` Each round + -10000..10000 0 `fire_when!=at_start`; `earlier` Earlier rounds Keep/FadeOut/Remove
  `fire_when!=at_start`; `fade_time` Fade time 0.05..30 s 1 `earlier=fade_out and fire_when!=at_start`;
  `most_balls` Most balls 1..100000 20000.
- Aim: `aim` Aim: Off (one choice now; part 3 adds more).
- Catch: `catches` Nothing/Remove/SendBackOut/PassThrough; `catch_size` Catch size 1..2000 px 40 keyframe
  `catches!=nothing`; `catch_from` AnyBalls/ItsOwn/Chosen `catches!=nothing`; `catch_things` Things Target
  Anything `catch_from=chosen and catches!=nothing`; `return_way` SameWayBack/OutMyWay
  `catches=send_back_out`; `return_delay` Delay 0..30 s 0 `catches=send_back_out`.
- Marker (all `affects_physics` false): `marker` None/RingWithDot/Dot/Ring; `marker_size` 1..200 px 12
  keyframe; `marker_colour` Color #ffc46b; `marker_glow` 0..1 0.4 keyframe; `flash_fire` 0..1 0.5; `flash_land`
  0..1 0.9; `flash_fade` 0.05..10 s 0.4; `ripple` bool true; `ripple_rings` 1..10 3; `ripple_far` 10..4000 px
  300; `ripple_fade` 0.05..10 s 1.2.
- Variety: `vary_size`, `vary_colour`, `vary_turn` (as Balls), `seed` i32 1. `vary_colour`, `vary_turn` and every
  Marker key are look-only (`affects_physics` false).

### 5.2 Pure maths (`fire.rs`)
- `direction(s, k, n, round, scene_seed, thing) -> f32` (radians, before the pose turn) per spec 5.2 rules.
- `start_offset(s, k, n, dir) -> Vec2`: Point 0; Ring `start_radius · dir`; Line across `aim`: perp(aim) ·
  (k/(n−1) − 0.5) · length (one ball: 0).
- `launch_order(s, n, round, seed) -> Vec<u32>`: Around 0..n; Random seeded shuffle; Middle out by |k − (n−1)/2|
  with the lower k first on a tie.
- `slot_step(s, j, rate) -> u64`: `round(j · gap · rate)` for the j-th launch of a round.
- `next_count(s, n) -> u32`: `round(n · times + plus)` clamped to 0..=most_balls.
- `round_sizes(s) -> Vec<u32>` (finite rounds) and `planned(s)` (decision 12).
- `layout(s) -> Vec<Vec2>`: round 1's group places (decision 13), by k.
- `speed_factor(s, number)`, `size_factor` via `variety()`.
- Seeds: `Rng::new(scene_seed as u32 ^ thing.0.wrapping_mul(0x9e37_79b9), round << 20 ^ k)` with a roll tag.
- Unit tests for every Directions × Start from, order, sizes, layout, seeds.

### 5.3 State and sim (`sim.rs`)
```rust
pub struct EmitterState {
    pub settings: EmitterSettings,
    pub rows: BallRows,
    pub layout: Vec<Vec2>,
    pub planned: usize,
    pub rounds_fired: u32,
    pub next_count: u32,
    pub next_number: u32,
    pub next_every: Option<u64>,
    pub pending: Vec<Launch>,           // soonest last: { step, round, k, n }
    pub fired: Vec<Fired>,              // { step, round, launched } per round, in order
    pub held: Vec<Held>,                // { thing, row, release, velocity } balls this catcher holds
    pub waves: Vec<Wave>,               // open waves: { owner, round, first, arrivals }
    pub landings: Vec<Landing>,         // { step, owner, round, wave size so far }
}
```
- `build`: settings from params; pose = `sits_on_pose` or `world.anchor`; `world.poses.insert`; empty Bodies;
  `fire_due(step 0)` and `launch_due(step 0)` (a round at 0 s is there on the first frame).
- `needs_walls() -> true`, `history()` → `around_history`.
- `spawn(step)`: release held balls due; read landings at step−1 of every emitter `wave_from` names (a target of
  kind emitter, Me = itself) → rounds to fire; `fire_due`; `launch_due` (counting live rows once, cut at Most
  balls; each launch pushes a body row, its `BallRows` row and a Spawn event).
- `scripted`: pose from Sits on (focus of the shape's wall as moved this step) or `world.anchor`; writes
  `world.poses`.
- `after_step`: fades due (pop + Remove), then catching (5.3.1).
- `settings_changed`: keep settings; resize rows when Size keyed (as Balls); keep `next_every` until it fires.
- `affects_physics`: false for Marker keys, `vary_colour`, `vary_turn`.
- Firing a round r ≥ 2 first applies Earlier rounds to live rows of rounds < r: Fade out sets `fade_from = step`
  on those without one; Remove pops them.

#### 5.3.1 Catching
- Skip when `catches == Nothing`. Centre = this step's pose; r = catch size.
- Candidates: every body thing (BTreeMap order) matching From: Any balls (all), Its own (self), Chosen
  (`world.names(target, me, other)`). Rows alive, not held.
- Arrival: `|previous − c| > r` and the segment previous → position comes within r of c. Entry point by the
  segment–circle quadratic.
- For each arrival (in order): Enter event; wave grouping per (owner, round) within 120 steps of the wave's first
  arrival, else a new wave and a Landing; owner's `rows.arrivals[row] += 1`; then Remove (pop), Send back out
  (decision 5; held when Delay > 0), or nothing (Pass through).
- Open waves older than 120 steps are dropped.

### 5.4 Marker drawing (`draw.rs`)
- After the balls (`BallsDraw` methods), sprites in `frame.particles_front`: Ring (`RINGS`, half size =
  marker_size), dot (`DOTS`, size · 0.3), glow (soft `SMOKE`-style sprite with softness 1, adding light:
  negative alpha, size × 3, strength `marker_glow`).
- Flash: for each `fired` (flash_fire) and `landings` (flash_land) entry younger than `flash_fade`: an adding
  glow `size · (2 + 4 · strength)`, intensity `strength · (1 − age/fade)²`.
- Ripple (landings, when on): ring j of n starts `j · 0.15 · fade` late, grows from size to `ripple_far` (ease
  out) over the rest, alpha `(1 − p)²`, `RINGS` sprites in marker colour.
- Nothing kept between frames; everything from the state and time.

### 5.5 Sits on (`foci.rs`)
- `focus_points(&Outline) -> Option<[Vec2; 2]>`: `Ring { sides: 0 }` → both its centre; others `None`. Part 1
  adds `Outline::Ellipse` (task 12).
- `can_sit_on(thing: &Thing) -> bool`: a shape whose Start from is Circle (part 1: or Ellipse).
- `sits_on_pose(world, s) -> Option<Pose>`.

## 6. Other kit changes
- `lib.rs`: register `EmitterKind`, `EmitterSim`, `EmitterDraw`; re-exports.
- `camera/sim.rs` follow: `Target::Kind("balls")` also matches emitters (decision 17).
- `scenes.rs`: test scenes `Emitter rounds` (an emitter at the middle of a circle, 3 rounds × 10 every 1 s, Fade
  out, Wavefront and a group gradient by Launch angle) and `Emitter chain` (two emitters in a circle, each
  firing when a wave lands on the other, Pass through catches, Endless with Most balls 400), appended to `all`.
  Bless their golden lines (debug and release); no other line may change.

## 7. Editor (bpe-studio)
- `tree.rs`: icon `Icon::Emitter` (a ring with a dot).
- `inspector.rs`: an emitter's own card is the Emitter card (`emitter_card::show`) with chips Emit | Catch |
  Marker (`Inspector.emitter_tab: BTreeMap<ThingId, EmitterTab>`). `BALLS` checks that mean "a thing with ball
  rows" (Look switch, group mode, Size row) become `has_ball_look(kind)`: balls or emitter. An emitter has no
  Launch speed row under Placement.
- Emit tab: mini view, rounds strip, then rows Fire (Emits, Sits on menu + hint, Start from, Radius/Length,
  Directions, Aim direction, Fan angle, Speed, Speed spread, How many, Gap, Order), Rounds (Fire when, After,
  Every/On/Also at start, Rounds, Endless, Each round × and +, Earlier rounds, Fade time, Most balls), Aim.
- Hint under Sits on, when Aim is Off and it sits on a focus: "Balls from here meet at the other focus." and, when
  the Physics card has gravity or (after part 1) balls touch walls with their edge, "…if gravity is off and balls
  touch walls with their middle."
- Catch and Marker tabs: plain grids of their headings, with the "Only when Catches is Send back out" rows shown
  by their `when`.
- Mini view (`mini_view.rs`): 170 px tall, `style::GROUND` well; the emitter at the middle; one round's rays
  (length = Speed × 0.5 s, fitted to the view) in `round_colours` (scene colours); the Start ring or line; the
  catch circle dashed; a square aim handle (drag sets `aim_angle`), fan edge handles (drag sets `fan_angle`,
  Fan only), ray ends (drag sets `speed`). A drag is its own undo step (`own_step`).
- Rounds strip (`strip.rs`): a 26 px lane over the clip length; a block per round fired (from
  `Inspector.rounds`, filled by the app each frame from the playback world) labelled with its ball count, and
  outlined blocks for rounds still to come (decision 16).
- Look card: Ball / Group switch for emitters; Group mode for an emitter shows the group view of one round's
  layout (no shape chips, no custom-ball dragging, no + tool), Across the group, and the Variety and Odd ones out
  folds (Variety holds Size spread, Colour spread, Turn spread, Seed + Shuffle).
- `group_view.rs`: `ViewCx::settings()` for an emitter returns a stand-in `BallsSettings` (group on, shape
  Custom, `custom` = the layout, its size and variety) so fitting, picking and the gradient line work;
  `group_scene` sets the emitter to fire one round at 0 s; `build_group_frame` moves an emitter's rows to the
  layout after building.
- `ball_view.rs` `mini_scene`: an emitter fires its first round at 0 s with no gap (so the view has a ball).
- Preview: a selected emitter shows its catch circle (dashed) and Start ring or line; it can be dragged (it is a
  "spot" thing when it has no balls); dropped within 24 screen px of a focus of a circle (part 1: an ellipse) it
  sits on that focus; dragging a sitting emitter away sets Free and puts its position where it was dropped.
- Timeline: an emitter's group shows even without keys, with a tick per round fired (reusing the slot ticks,
  labelled "Round n").

## 8. Verification
- `cargo test --workspace` (in the background; GPU picture tests need mesa), `cargo clippy --workspace`,
  `cargo fmt`.
- New tests (names indicative):
  - runtime: held ball; event tags.
  - fire.rs: `all_round_spreads_evenly_from_the_aim`, `a_fan_of_one_goes_the_aim_way`, `ring_and_line_start_places`,
    `middle_out_order`, `random_is_seeded_and_stable`, `round_sizes_grow_by_times_and_plus`.
  - sim: `at_start_fires_one_round_at_after`, `every_fires_rounds_on_their_steps`,
    `gap_and_order_launch_on_exact_steps`, `a_wave_landing_fires_the_other_emitter`,
    `endless_stops_at_most_balls`, `fired_balls_inside_do_not_arrive_until_they_come_back`,
    `a_fast_ball_is_caught_by_the_swept_test`, `remove_logs_a_remove`, `send_back_out_keeps_row_and_number`,
    `out_my_way_refires_from_the_start_place`, `delay_holds_the_ball_still`, `pass_through_leaves_it_alone`,
    `waves_group_by_owner_and_round_within_half_a_second`, `fade_out_fades_then_removes`,
    `remove_earlier_rounds`, `sits_on_follows_a_spinning_shrinking_circle_middle`,
    `seek_back_replays_the_same` (scrub = play).
  - draw (frame-level, no GPU): marker sprites, flash after a fire and a landing, ripple rings, a fading ball's
    opacity, launch-angle gradient colours differ round the wheel, arrivals flash the ball's Flash on hit.
  - one GPU picture test: an emitter's marker and a wavefront put light where expected.
  - studio (kittest): chips switch tabs; a mini view aim drag changes `aim_angle`; the strip shows a block per
    round fired; dropping an emitter on a circle's middle sets Sits on.
  - goldens: only the two new lines appear (diff `tests/golden*.txt` against main).

## 9. Things not to do
- Don't copy the Balls draw or look code; generalise it.
- Don't keep per-frame state for markers, waves, fades or wavefronts.
- Don't change ball ownership on a catch.
- Don't re-bless existing golden lines; don't add built-in or example scenes.
- No made-up colours in editor code (the design-system test); scene colours end in `// scene colour`.
- Don't let arrivals change `Bodies.hits` (physics and sound must not hear them).

## 10. Tasks, in order
1. Runtime: `Bodies.held`, events, `pop` public. Tests.
2. `BallRows`, `group_of`, `variety`; Balls sim on rows. All existing tests green, goldens unchanged.
3. Emitter kind, settings, `fire.rs` + tests.
4. Emitter sim: firing, rounds, fades, catching, holds, Sits on (circles). Tests.
5. Group gradient spreads; Balls draw generalised (fade, arrivals); EmitterDraw + marker. Tests.
6. Wavefront layer. Tests.
7. Starters, look fits, camera, registration, test scenes, goldens.
8. Studio: tree icon, inspector wiring, Emitter card with tabs.
9. Studio: mini view and rounds strip.
10. Studio: Look card Group mode for emitters, ball view, preview drag/snap/overlays, timeline ticks.
11. Full test run, clippy, fmt; CLAUDE.md map; one Opus review; fixes; PR.
12. After part 1 merges: rebase, `Outline::Ellipse` in `focus_points`, ellipses in `can_sit_on`, the hint's
    Touch walls check, a test that balls from one focus meet at the other.
