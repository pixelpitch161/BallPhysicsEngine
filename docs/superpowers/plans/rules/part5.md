# Rules part 5: more events and actions

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md` (sections 3.3, 4.1, 4.5, 6 and 9.5). Builds on part 1
(`docs/superpowers/plans/rules/part1.md`, "As built"). Headless: engine and kit only. The editor's Rules panel is
part 2; every new block's settings come from its `#[derive(Settings)]` schema, so part 2's inspector shows them with
no more work. A scene without enabled rules simulates and draws byte-identically (goldens, picture hashes).

## Goal

New blocks:

| Section | Id | Name | Does |
| --- | --- | --- | --- |
| When | `when.enter` | Enters / Leaves | a ball comes into or leaves a shape (or comes into an emitter's catcher) |
| When | `when.fired` | Fired | an emitter fires a ball, or a round |
| When | `when.removed` | Removed / Broken | a ball is removed (for any reason), or a ball or thing is broken by a rule |
| When | `when.count` | Count reached | a thing's hits so far, its ball count, or a counter rises to / falls to N |
| When | `when.level` | Crosses a level | a ball's speed, size, light or shadow, or a shape's or bus's sound goes over / under a level |
| When | `when.signal` | Signal | another rule sent this signal |
| If | `if.counter` | Counter is | a counter compares to a number |
| Do | `do.split` | Split | the ball into N balls (sizes shared by area, a fan, keeping its look, cards and rule changes) |
| Do | `do.spawn` | Spawn balls | N balls into a Balls thing or an emitter, at the firing's point or a thing's place, with a launch |
| Do | `do.fire` | Fire a round | an emitter fires a round now (its Fire when gains "By a rule") |
| Do | `do.tag` | Tag | adds or removes a tag on a ball or a thing |
| Do | `do.count` | Count | adds to, sets or resets a named counter |
| Do | `do.signal` | Send signal | sends a named signal with this firing's values |

Plus the safety limits that arrive with them: the 4096-ball cap a thing (`MAX_BALLS`) for splits and spawns, and the
8 signal rounds a stage.

Also a shape gets **Balls pass through** (a switch, off by default): its wall stays (lights, lasers, foci and the
probe still see the shape) but balls aren't bounced by it, so a shape can be a zone for Enters / Leaves. Without it
a ball can never come into a shape (walls are two-sided: a ball always stays on the side it started), so the block
would be useless.

## Files

| Crate | File | What |
| --- | --- | --- |
| bpe-runtime | `walls.rs` | `Wall.passes: bool` (balls go through); every literal gains `passes: false` |
| bpe-runtime | `world.rs` | `collide` and the Middle fix-up skip walls that let balls pass (and the Edge "carried across another wall" check); `RulesState::build` gets `&World`; the fingerprint hashes the sound/light makers' fingerprints when a rule block hears sound / sees light; `step` hands `sims` to `apply_rule_actions` |
| bpe-runtime | `plugins.rs` | `ThingSim::add_ball(thing, world, &NewBall) -> Option<u32>` and `ThingSim::fire_now(thing, world) -> bool` (defaults: can't) ; `NewBall` |
| bpe-runtime | `rules/sim.rs` | `PrepareCx.world`; `StageCx.log`, `.counters`, `.ball_tags`; `BlockSim::hear` (When blocks hearing signals), `hears_sound`, `sees_light` |
| bpe-runtime | `rules/mod.rs` | signal rounds (`MOST_ROUNDS` 8, carried signals), `Signal`, counters, ball tags, `MAX_ROWS`, `copy_row`, readers `counter`, `counters`, `ball_tags`, `has_ball_tag` |
| bpe-runtime | `rules/actions.rs` | `Action::Split`, `Spawn`, `Fire`, `Tag`, `Count`, `Signal` and applying them |
| bpe-kit | `rules/who.rs` | `matches(cx, ..)` takes the stage (a ball's own tags count), `targets` adds balls tagged on their own; whole-thing parties use item `WHOLE` |
| bpe-kit | `rules/events.rs` (new) | `when.enter`, `when.fired`, `when.removed` |
| bpe-kit | `rules/counts.rs` (new) | `when.count`, `if.counter`, `do.count` |
| bpe-kit | `rules/level.rs` (new) | `when.level` |
| bpe-kit | `rules/signals.rs` (new) | `when.signal`, `do.signal` |
| bpe-kit | `rules/balls.rs` (new) | `do.split`, `do.spawn`, `do.fire` |
| bpe-kit | `rules/tags.rs` (new) | `do.tag` |
| bpe-kit | `rules/mod.rs` | registers the 13 kinds |
| bpe-kit | `things/balls/sim.rs`, `things/emitter/sim.rs` | `add_ball` (rows kept in step with the bodies); emitter `fire_now`, `FireWhen::ByARule`, `EmitterState.asked` |
| bpe-kit | `things/emitter/mod.rs` | `FireWhen::ByARule` ("By a rule") |
| bpe-kit | `things/shape/{mod,sim}.rs` | `ShapeSettings.passes` ("Balls pass through"), carried into `Wall.passes` |
| bpe-kit | `modules/look/layers/particles/draw.rs` | particles don't bounce off walls balls pass through |
| bpe-kit | `rules/tests_part5.rs` (new), `scenes/rule_scenes.rs`, `tests/golden-rules*.txt` | unit tests and two more golden scenes |

## 1. Runtime

### 1.1 Walls balls pass through

`Wall { .., passes: bool }`. In `World::collide`: the wall loop `continue`s for `wall.passes`; the Edge carry's
`crosses` check ignores them; the Middle fix-up loop ignores them. Nothing else reads it in the runtime. Particles
(draw-only) skip them too. The emitter's aim (`aim.rs`) uses `world.walls` for foci and "walls move" only, so it is
unchanged.

### 1.2 Hooks for balls a rule makes

```rust
/// A ball a rule adds to a thing (Split, Spawn balls).
pub struct NewBall {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Its size; `None`: the thing's own (its Size, as a new ball of it would be).
    pub radius: Option<f32>,
    /// The body row it is split from: it takes that ball's number and facts.
    pub like: Option<u32>,
}
trait ThingSim {
    /// Adds `ball` to the thing's bodies and whatever it keeps per row, returning its row; `None` when
    /// the thing has no balls to add to.
    fn add_ball(&self, _thing: ThingId, _world: &mut World, _ball: &NewBall) -> Option<u32> { None }
    /// A rule asks the thing to fire now (an emitter's round); false when it can't.
    fn fire_now(&self, _thing: ThingId, _world: &mut World) -> bool { false }
}
```

- Balls: `add_ball` pushes the row; `like` copies its number, size share, launch, place and in-round and its own
  size (so a later Size key scales the smaller child in proportion, as growth); otherwise the next number
  (`max(offsets.len(), largest number + 1)`), size share 1, the launch angle of its velocity, place 0, own size =
  its radius (`radius` or the thing's Size at that number).
- Emitter: the same; a new number is `next_number` (then +1), round `rounds_fired`, launch its angle. `fire_now`
  adds one to `asked`; `rounds_due` adds `asked` (then the state forgets it when the round fires); By a rule
  starts no round of its own; `round_marks` shows no rounds still to come for By a rule.

### 1.3 New actions (applied in `apply_rule_actions`, which now takes `sims`)

```rust
Action::Split { thing, row, into: u32, spread: f32 /* radians */, speed: f32 /* factor */, share: bool }
Action::Spawn { thing, balls: Vec<(Vec2, Vec2, Option<f32>)> }   // place, velocity, size
Action::Fire { thing }
Action::Tag { thing, row: Option<u32>, tag: String, add: bool }
Action::Count { name: String, how: CountHow /* Add(f64) | Set(f64) */ }
Action::Signal { name: String, firing: Firing }                     // never queued: the stage takes it
```

- **Split** of a live row (skipped when broken, dead or past the caps): the ball's velocity (a frozen ball's held
  velocity), direction `d` (straight up when still), speed `|v| × speed`. Child `k` of `n` (k = 0..n) goes the way
  `d` turned by `spread × (k/(n−1) − 0.5)` and sits `perp(d) × (k/(n−1) − 0.5) × 2W` from the centre, `W =
  max(r − r', 0.05 r)` (inside the old ball, so no child starts past a wall the ball was clear of); `r' = r/√n` when
  sizes are shared, else `r`. The ball itself becomes child `(n−1)/2` (keeps its row, trail and history); the others
  are added through `add_ball(like: row)` and take its hits so far; the ball's own rule changes are copied onto
  them (`copy_row`). Children past `MAX_BALLS` live balls or `MAX_ROWS` rows are dropped (the ball still shrinks
  only by the children made: `r' = r/√made`).
- **Spawn**: each ball through `add_ball(like: None)`, until the caps; non-finite skipped.
- **Fire**: `fire_now`. A broken thing is skipped.
- **Tag**: a whole thing's tags (`world.tags`, which the physics' targets read too) or one ball's
  (`RulesState.ball_tags`, read by rules only). Removing a tag a ball doesn't have alone does nothing.
- **Count**: `counters[name]` added to or set; a non-finite result is dropped. Reset is Set 0.
- Breaking a ball forgets its own tags; breaking a thing forgets its tags' rule-made ones? No: a broken thing is out
  of the order and `targets` leaves broken things out, so its tags no longer matter.

Caps: `MAX_BALLS` (4096) live balls a thing; `MAX_ROWS` (65 536) rows a thing (rows are never removed, so splits
and breaks that go on and on stop adding there).

### 1.4 Signals and the stage

`Signal { name: String, firing: Firing }`. The stage (`RulesState::run`):

1. Round 0: per rule (list order, skipping problems): Logic ticks, When blocks fire, then every When block `hear`s
   the signals carried from the last step. A Do block's `Action::Signal` goes to the next round's list instead of
   the queue (it still counts toward the rule's budget).
2. Rounds 1..=8 (`MOST_ROUNDS`) while signals were sent: per rule, every When block `hear`s that round's signals.
3. Signals sent in round 8 are carried to the next step's round 0 (`RulesState.carried`).

Per rule the noted blocks (one `Rule` event per block per step), the budget and "busy" last across the rounds; the
events are logged after the last round, rule by rule. Signals a rule sends are heard by every rule, itself too.

`StageCx` gains `log: &Log` (the whole log up to now, for sound and light), `counters: &BTreeMap<String, f64>` and
`ball_tags: &BTreeMap<(ThingId, u32), BTreeSet<String>>`; `PrepareCx` gains `world: &World` (built, before any step).

### 1.5 Sound and light read by rules

`BlockSim::hears_sound(params) -> bool` and `sees_light(params) -> bool` (default false). `fingerprint_of`, for every
enabled rule's block, hashes `sims.sound`'s fingerprint once when any hears sound and `sims.light`'s once when any
sees light, so editing the voice or a lamp restarts a run whose rules read them. The level block makes its own
`SoundAt`/`LightAt` from the makers at prepare (`None` without a maker: it then never fires).

## 2. Kit blocks

Every kind is a `#[derive(Settings)]` struct in group "Rule", a `RuleBlockKind` (`block_kind!`) and a `BlockSim`.
Numbers marked "from" can come from the firing (`Amount`). Whole-thing parties carry item `WHOLE` (`u32::MAX`), so
`who::side` reads them as the whole thing, never a ball row.

### 2.1 Who (`who.rs`)

- `matches(cx: &StageCx, who, party)`: `Tag` also counts a ball's own tags (`cx.ball_tags`).
- `targets`: for `Tag`, things with the tag whole, then balls tagged on their own (their thing not tagged), in
  (thing, row) order, alive only.

### 2.2 Enters / Leaves (`events.rs`)

Settings: `who` (balls, default Anything), `zone` (Who, default Anything), `way` Enters / Leaves / Either.
- Zones: every thing with a wall (shapes) that `zone` matches and isn't broken; a ball is in it when its centre is
  inside the outline (`Outline::inside`). The state keeps the set of (zone, ball thing, row) inside, worked out at
  prepare from the built world (balls inside at the start never "enter"). Each stage: the set now (live balls that
  `who` matches), compared with the last: new → Enters, gone → Leaves (but not when the ball died or its zone is
  gone: then it is just dropped). In (zone, thing, row) order.
- Emitters: Enters also fires on the step's `Enter` events (a ball coming into a catcher that catches), whose
  catcher `zone` matches.
- Firing: subject the ball, other (zone, `WHOLE`), point the ball's place, values the ball's (speed, size, hits,
  ball) and `way` (1 entered, 0 left).

### 2.3 Fired (`events.rs`)

`who` (emitters, default Anything), `each` A ball / A round.
- A ball: each `Spawn` event whose emitter matches: subject the ball, other (emitter, `WHOLE`), point where it
  starts, values ball values + `round`, `speed` (its launch speed).
- A round: each round in `EmitterState.fired` made on this step: subject (emitter, `WHOLE`), values `round`,
  `balls` (its size), `count` (rounds fired).

### 2.4 Removed / Broken (`events.rs`)

`who` (default Anything), `what` Removed (`Remove` events: a ball gone for any reason, a broken one too) / Broken
(`Break` events: a ball or a whole thing broken by a rule). Subject the ball (or the thing, `WHOLE`), point the
event's, values the ball's as it was, `strength` for Broken.

### 2.5 Count reached, Counter is, Count (`counts.rs`)

- **Count reached**: `of` Hits so far / Ball count / A counter; `who` (Thing, Tag, Kind, Anything: each thing
  counted on its own; That/Other/Group count as Anything here), `counter` (name), `test` Rises to / Falls to,
  `amount` N. Fires when the value crosses N: rises from below N to N or more, falls from above N to N or less.
  Hits so far counts the `Hit` events a thing takes part in (once an event), from the start; ball count is its live
  balls; a counter starts at 0. Starting values from the built world. Subject (thing, `WHOLE`) (none for a
  counter); values `count`.
- **Counter is**: `counter`, `test` (Over / Under / Between / Equal, as Compare), `amount`, `to` (from). A counter
  never set is 0.
- **Count**: `counter`, `how` Add / Set / Reset, `amount` (from). Applied at the next step's start, like every
  action.

Counter names are trimmed; an empty name is a problem ("Count needs a counter name").

### 2.6 Crosses a level (`level.rs`)

`value` Speed / Size / Light / Shadow / Sound; `who` (balls, for the first four), `shape` (Who, for Sound; must name
a thing), `sound` The shape's level / Bus 1 / Bus 2 / Bus 3; `way` Goes over / Goes under / Either; `level`.
- Per ball (first four): the value now, against the value at the last stage (a ball's first stage only notes it).
  Light and Shadow are the probe's `ball(..)` levels (0..1); speed in px/s, size in px.
- Sound: `SoundAt::level(world, log, shape, Level | Bus(n), time)` (0..1), one memory.
- Fires on the crossing: over is `last ≤ level < now`, under is `last ≥ level > now`. Subject the ball (none for
  sound), values the ball's and `value`.

### 2.7 Signals (`signals.rs`)

- **Send signal** (Do): `name`. Acts `Action::Signal { name, firing }` (a copy of the firing that reached it).
- **Signal** (When): `name`. `hear`: every signal of that name becomes a firing with the sender's subject, other,
  point, normal and values, made this step.

### 2.8 Split, Spawn balls, Fire a round (`balls.rs`)

- **Split**: `who` (default That), `into` (2..1024, span 2..8, default 2, from), `spread` (degrees, span 0..180,
  default 60, from), `speed` (×, default 1, from), `sizes` Shared by area / Keep size. A whole thing target splits
  each of its live balls.
- **Spawn balls**: `into` (Who, default Group), `count` (1..4096, default 1, from), `at` The firing's point / A
  thing's place, `place` (Who), `way` Up / Angle / Away from the hit / Like that ball / All round / Random, `angle`
  (from), `spread` (degrees, a fan across the balls), `speed` (px/s, default 300, from), `size` Its own size / Set
  size, `radius` (from). Places: the point plus each ball's way × its size when there are several (they fan out
  from it). Random ways are dice of (rule, block, step, subject, ball index).
- **Fire a round**: `who` (default Anything, emitters only).

### 2.9 Tag (`tags.rs`)

`who` (default That), `tag` (text, trimmed; empty is a problem), `how` Add / Remove. A ball target tags the ball;
a whole thing target tags the thing.

## 3. Shapes and emitters

- `ShapeSettings.passes: bool` ("Balls pass through", after Fill, not keyframed); `ShapeState::wall` sets
  `Wall.passes`.
- `FireWhen::ByARule` ("By a rule"). `EmitterState.asked: u32`. `rounds_due`: `starts` is false for By a rule;
  `due += asked`; `fire_and_launch` sets `asked = 0` after firing. An emitter that doesn't emit ignores it.

## 4. Tests (what passing looks like)

- runtime: signals go round (a test block pair: a rule sending a signal each round reaches 8 rounds and then carries
  to the next step); a split without `add_ball` is skipped; `Wall.passes` lets a ball through a ring.
- kit (`rules/tests_part5.rs`), exact steps where it matters:
  - Enters/Leaves: a ball crossing a pass-through ring fires Enters on the step its centre crosses and Leaves on the
    step it comes out; a ball starting inside never "enters"; Either fires both.
  - Fired: an emitter's balls and rounds; Fire a round with By a rule fires only when asked, a round a time.
  - Removed / Broken: a Break of a ball fires both Removed and Broken; a ball removed by its Bounce fires Removed.
  - Count reached: hits so far rising to 3 fires once; ball count falling to 0 after breaks; a counter set by Count
    (one step later).
  - Counter is: Over / Equal.
  - Crosses a level: speed going over a level as a ball falls; size going over with a Change setting glide.
  - Signals: Send → Signal in another rule fires the same step, carries the values; a loop of two rules stops after
    8 rounds a step.
  - Split: 1 → 3 balls, areas add up, the fan's ways, children keep the number, their changes copied; a split at
    the cap makes no balls.
  - Spawn: N balls into a Balls thing at the firing's point with All round ways; into an emitter.
  - Tag: a ball's own tag is matched by a Hit's Who Tag; a thing's tag changes what a Bounce card's special target
    sees.
  - Determinism: two runs equal; seek back (snapshot) and replay equal, for a scene using every new block.
- goldens: two more rule scenes in `scenes::rule_scenes` ("Split chain" — balls splitting on hits up to the cap —
  and "Zones and signals" — pass-through zones, counters, signals, spawns, tags); `golden-rules*.txt` re-blessed in
  debug and `--release`; the existing lines of `golden-rules*.txt` and every other golden unchanged.
- `cargo clippy --workspace` clean; `cargo fmt`.

## Not doing in part 5

- The editor (part 2 shows the blocks through their settings schemas; a counter list in the Rules panel can read
  `RulesState::counters`).
- Leaves for an emitter's catcher (only Enters, from its `Enter` events).
- Per-ball tags in the physics (a Bounce card's special target reads whole things' tags only).
- Rule-made balls log no `Spawn` event (that event is an emitter's firing).

## As built

- Every block in sections 1–3 is in, under the names above; `bpe-kit/src/rules/` gained `events.rs`, `counts.rs`,
  `level.rs`, `signals.rs`, `balls.rs` and `tags.rs`, registered after part 1's 14 in `register` and `add_sims`.
- "Balls pass through" sits after Smallest size (outside the Body headings, so it shows in the Shape card's main
  grid and the Look editor's Body leaves it out).
- `PrepareCx` gained `world` (Enters' starting inside sets, Count reached's starting ball counts) and `StageCx`
  gained `log`, `counters` and `ball_tags`. `RulesState::build` takes the world.
- `World::fingerprint` hashes the sound maker's fingerprint when an enabled rule's block `hears_sound`, and the light
  maker's when one `sees_light`.
- Tests are in the kit (`rules/tests_part5.rs`, 20 tests: pass-through, Enters/Leaves/Either and a ball starting
  inside, Fired balls and rounds, Fire a round with By a rule, Removed/Broken, Count reached on hits, ball count and a
  counter, Count's Set/Reset, Crosses a level on speed, signals the same step and a signal loop going round 8 times
  a step, Split's areas, fan and copied changes, the cap, Spawn All round and into an emitter, ball and thing tags,
  determinism through seeks). The runtime-only cases in section 4 are covered there through the kit's blocks.
- Goldens: "Split chain" and "Zones and signals" added to `scenes::rule_scenes`; `golden-rules*.txt` gained their two
  lines in debug and `--release`, the four part 1 lines unchanged.
- After the review: a Spawn at the firing's point fans out from the firing's ball's middle (a hit's point is on the
  wall's line), and Spawn and Split put a ball that would land across a wall at the middle instead
  (`World::clear_way`); Enters keeps every live ball's place in watched zones and applies Who when it fires (a tag
  given inside a zone is no coming in), a zone that starts being watched only notes who is in it, and a ball made
  since the last stage (fired, released, spawned, split off) inside a zone was born there; `Bodies::row_of` gives
  the first live row with a number (split balls share it); a Spawn queues no more balls than there is room for;
  the stage lends its counters, ball tags and broken things out instead of copying them, and drops the tags of
  balls gone.
- Count reached on Hits so far only counts the hits of the stages its rule runs in (a rule too busy for a step
  misses that step's hits); left as it is.
