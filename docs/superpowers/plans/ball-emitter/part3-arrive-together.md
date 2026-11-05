# Ball Emitter part 3: Arrive together — plan

Date: 2026-11-05. Spec: `docs/superpowers/specs/2026-11-05-ball-emitter-design.md`, section 6 (design approved).
Branch `claude/project-thread-0uv6bi`, from main `c3d3a1d` (parts 1 Ellipse #94, 2 Emitter #96 and 5 Vary #95 in).
Built straight through, one Opus review at the end, one PR. No screenshots or videos.

## 0. What it does

An emitter's **Aim** gains **Straight in** and **Keep my directions**. Every ball of a round is aimed so it reaches
the **Target** **Arrive after** seconds after the round fires, all together. Straight in works out each ball's
velocity (Directions and Speed don't steer it); Keep my directions keeps each ball's own direction and works out its
speed and when it leaves. Balls that can't make it are skipped or fired anyway (**Can't reach**).

## 1. Decisions this plan makes (spec gaps)

1. **Exact in the world's own steps, not in continuous time.** The world integrates semi-implicit Euler at 240 Hz
   (`v += a·dt`, then `x += v·dt`), so the spec's continuous formulas would miss by `a·dt·t/2` (4 px for a 2 s throw
   under 1000 px/s²). The aim solves the stepped motion instead:
   - A ball launched in the Spawn stage of step `L` first moves in step `L`, so it sets off from the moment
     `now(L) = max(L, 1) − 1` (a launch while building, step 0, moves first in step 1: same moment 0). After `j`
     whole steps it is at `p0 + j·dt·v0 + a·dt²·j(j+1)/2`; inside a step it moves in a straight line, so part-way
     `e = k + f` steps along it is at `P(e) = p0 + e·dt·v0 + a·dt²·G(e)` with `G(e) = k(k+1)/2 + f(k+1)`.
   - **Straight in**: `N = A − now(L)` steps from setting off to the arrival moment `A`:
     `v0 = Δ/(N·dt) − a·dt·(N+1)/2`. Exact at the end of step `A`.
   - **Keep my directions** (direction `u`, `n = u.perp()`): across `u`, `n·Δ = (n·a)·dt²·G(e)`, so
     `G(e) = Q = n·Δ / ((n·a)·dt²)`; `k = ⌊(√(8Q+1) − 1)/2⌋` (nudged so `k(k+1)/2 ≤ Q < (k+1)(k+2)/2`),
     `f = (Q − k(k+1)/2)/(k+1)`; along `u`, speed `s = (u·Δ − (u·a)·dt²·G(e)) / (e·dt)`. Needs `Q > 0` and `s > 0`.
     The ball's path passes exactly through the target, `e` steps after it sets off; it leaves at the step that puts
     that moment nearest `A` (start moment `round(A − e)`), so it arrives within half a step of the others.
   - Without a pull across `u` (`|n·a| < 1e-3`): only a direction through the target reaches (`|n·Δ| ≤ 0.5 px`,
     `u·Δ > 0`), at **Speed**: `e` is where `u·P(e)` first reaches `u·Δ` (walked step by step, then linear inside
     the step; with no pull at all `e = u·Δ/(Speed·dt)`), as the spec's `t = |Δ|/Speed`.
   - The fixed `dt` assumes one substep a step. Steps split only when some ball in the scene goes faster than
     3,840 px/s (`SAFE_MOVE` 16 px a step); then a ball under a pull lands up to `a·dt·t/2` off. Noted in the
     code, not warned (the speeds are rare and the miss small).
2. **The pull** `a` is the ball's steady acceleration: scene gravity × the Motion card's Gravity × plus Wind (both
   steady, so exact; the spec's warning on wind is not needed). Read from the Motion card as the world has it
   (`World::card` below; keys move it). The world prepares cards after building things, so an aimed round at 0 s
   fires in `after_build` (noting its balls in the history at step 0), not in `build`. Air drag isn't steady: warned.
   A Motion card that varies across balls uses the card's own values for the aim: warned.
3. **The ceiling**: a ball whose speed would pass `Limits::ceiling` at either end of its flight can't reach.
4. **Target** (`aim_target`, an Int thing id like `sits_on`): 0 means "the other focus" when the emitter sits on a
   focus (Focus 1 ↔ Focus 2; a circle's other focus is its middle), else nothing. Any other id is that thing's
   middle: an emitter that sits on a focus is aimed at as that focus. A thing that isn't there, or 0 with no seat,
   means no target: the round fires as with Aim Off and the card says "Pick what to aim at".
5. **Where the target will be** at the arrival moment, from how it moves now (no world copy is run):
   - a focus of a shape: exactly as the shape's own Spin and Shrink will carry it (its angle grows by `spin·dt` a
     step, its size by `(1 − shrink·dt)` a step down to its Smallest size), its middle carried on at its pose's
     velocity (`FocusTrack`, from `ShapeState` and the wall);
   - a Balls thing: its first ball still there, carried on and pulled exactly as the world moves it (until it
     bounces);
   - anything else: its pose's place plus its velocity × the time ahead (a rider on a ball or a moving parent).
   A target that goes its own way (the last two) adds a line to the card: "The target moves on its own: balls aim
   where it is heading and miss if it turns or bounces."
   Keyed Spin/Width/Height or a turning parent are followed only as they stand at firing.
6. **Gap and Order** apply to Straight in (each ball's own time, `t = T − its slot`); Keep my directions sets every
   ball's own leaving time, so Gap and Order are hidden then. **Speed** and **Speed spread** are hidden for Straight
   in; Keep my directions shows Speed (used when nothing pulls across, decision 1) and hides Speed spread.
   **Directions**/**Aim direction**/**Fan angle** still place the balls on a Ring (and Aim direction turns a Line),
   so under Straight in they show only for a Ring (Aim direction also for a Line). Rows that don't apply are
   **hidden, not greyed**, as every other card in the program does (the spec said greyed).
7. **Can't reach** — Skip: the ball isn't fired (its round is cut short by it; it takes no number). Fire anyway: a
   Keep ball that would have to leave before the round fired leaves at once with its worked-out speed (late); a ball
   with no way at all goes unaimed (its direction at Speed, no delay). A Straight in ball whose slot comes too late
   (`N < 1`) or would pass the ceiling: Skip, or unaimed.
8. **Launches are re-aimed when they go.** `fire_round` plans every ball (leaving step, for Keep) from the
   emitter's pose at firing; `launch_ball` works the velocity out again from where the emitter is at the leaving
   step (it may ride or sit on a moving focus), aiming at the target at the planned arrival moment (Keep: refined
   once at its own arrival moment, decision 1). A plan that no longer solves at launch follows Can't reach.
   `Launch` gains `arrive: Option<u64>` (the arrival moment; `None` unaimed).
9. **Send back out · Out my way** aims too: the ball's own "round" fires when it would go (the catch step + 1, or
   its Delay's end); Straight in sends it off then; Keep my directions holds it (`Bodies.held`, the existing
   `Held`) until its leaving step. A caught ball is aimed under its own thing's pull, not the catcher's. Can't reach: Skip removes the ball (a `Remove` event), Fire anyway sends it
   unaimed. Same way back is never aimed.
10. **Launch angle** of an aimed ball (the gradient's Launch angle spread, `BallRows.launch`, the Spawn event's
    `normal`) is the direction it actually leaves in.
11. **Warnings** (the card, under the Aim rows, Aim not Off): "Pick what to aim at." with no target; "Something in
    the way: balls may miss." when the Motion card has air drag, the Motion card varies across balls, its balls bump
    into other balls (Bounce `hit_balls`), or a planned path of the next round (sampled, 24 points a ball) comes
    within a ball's size of a wall; "N of M balls can't reach in time: they are skipped" / "…: they go anyway."
    The app works these out from the playback world like the rounds strip (`aim::check`, `Inspector.aim_checks`).
12. **Preview**: a selected emitter with Aim on draws the next round's aimed paths (as if fired at the playhead),
    faint, at most 400 balls (evenly picked), 24 points each, and a small ring at the target.
13. **Defaults**: `aim` Off, `aim_target` 0, `arrive_after` 2 s (0.1..60, span 0.1..10, keyframable),
    `cant_reach` Skip. Aim Off changes nothing: goldens don't move.

## 2. Files

New: `crates/bpe-kit/src/things/emitter/aim.rs` (all the aim maths and the `Aimer`), its tests in
`crates/bpe-kit/src/things/emitter/aim_tests.rs`.

Changed:
- `bpe-runtime/src/world.rs`: `pub fn card(&self, thing, slot) -> Option<Card<'_>>` with
  `pub struct Card<'a> { pub kind: &'a str, pub state: &'a dyn ThingState, pub varies: bool }`; re-export `Card`
  from `lib.rs`.
- `bpe-kit/src/things/shape/sim.rs`: `ShapeState::floor` public.
- `bpe-kit/src/things/emitter/mod.rs`: `Aim::{StraightIn, KeepMyDirections}`, `CantReach`, the three settings,
  the `when`s of decision 6, `EmitterSettings::aims()`.
- `bpe-kit/src/things/emitter/sim.rs`: `Launch.arrive`, `Moment.aim: Option<Aimer>`, planning in `fire_round`,
  aiming in `launch_ball`, Out my way in `catch`, `Aimer` built in `build`/`spawn`/`catch`.
- `bpe-studio/src/emitter_card/mod.rs`: Aim rows split round a Target menu row; warning lines.
- `bpe-studio/src/emitter_card/mini_view.rs`: no Speed ray drags under Straight in.
- `bpe-studio/src/inspector.rs`: `aim_checks: BTreeMap<ThingId, AimCheck>`.
- `bpe-studio/src/app.rs`: fill `aim_checks` beside `fill_rounds`.
- `bpe-studio/src/preview.rs`: aimed paths in `emitter_overlay`.
- `CLAUDE.md`: the aim in the emitter paragraph.

## 3. `aim.rs`

```rust
pub struct Pull { pub accel: Vec2, pub drag: f32, pub varies: bool }
pub fn pull_in(world: &World, thing: ThingId) -> Pull            // from World::card(Motion) → MotionState

pub enum Track { Still(Vec2), Moving { at: Vec2, velocity: Vec2 }, Focus(FocusTrack) }
pub struct FocusTrack { centre, velocity, offset, spin, scale, keep, floor }   // per second / per step
impl Track { pub fn at(&self, ahead_steps: f64, dt: f32) -> Vec2 }
pub fn target_track(world: &World, me: ThingId, s: &EmitterSettings) -> Option<Track>

pub fn now(step: u64) -> u64                                   // max(step, 1) − 1
pub fn straight_in(delta: Vec2, accel: Vec2, n: u64, dt: f32) -> Vec2
pub struct Keep { pub steps: f64, pub speed: f32 }
pub fn keep_way(delta: Vec2, accel: Vec2, u: Vec2, speed: f32, dt: f32, most_steps: u64) -> Option<Keep>
pub fn path_at(p0: Vec2, v0: Vec2, accel: Vec2, e: f64, dt: f32) -> Vec2

pub struct Aimer { style: Aim, track: Track, accel: Vec2, dt: f32, now: u64, arrive: u64, ceiling: f32,
                   fire_anyway: bool }
impl Aimer {
    pub fn of(world, me, s, pull, now) -> Option<Aimer>        // None: Aim Off or no target
    pub fn target_at(&self, moment: f64) -> Vec2
    pub fn plan_keep(&self, p0, u, speed, fire: u64) -> Plan    // leaving step for a Keep ball
    pub fn velocity(&self, p0, u, speed, leave: u64, arrive: u64) -> Option<Vec2>  // at launch
}
pub enum Plan { Go { leave: u64, arrive: Option<u64> }, Skip }

/// The next round as if fired at the playhead: every ball's start, velocity, leaving and arrival moments.
pub struct AimedBall { pub start: Vec2, pub velocity: Vec2, pub leave: u64, pub arrive: u64, pub reaches: bool }
pub fn next_round(world: &World, me: ThingId) -> Vec<AimedBall>
pub fn path_points(world: &World, ball: &AimedBall, accel: Vec2, points: usize) -> Vec<Vec2>
pub struct AimCheck { pub no_target: bool, pub in_the_way: bool, pub cant_reach: u32, pub of: u32, pub skip: bool }
pub fn check(world: &World, me: ThingId) -> Option<AimCheck>    // None: not an emitter, or Aim Off
```

## 4. Steps

1. Runtime `World::card`, `ShapeState::floor` pub. Test: `card` answers an emitter's Motion state.
2. Settings: enums, keys, `when`s; settings tests (`when` of Gap/Speed/Directions under each Aim).
3. `aim.rs` pure maths with unit tests first (TDD): `straight_in` lands on the target after N steps of the world's
   integration (simulated in the test with the same loop), with and without a pull; `keep_way` passes through the
   target (polyline distance < 1e-3 px) with a pull, `None` when the pull pushes it away (`Q ≤ 0`) or speed ≤ 0;
   no pull: through-direction only, at Speed; `FocusTrack::at` matches stepping a spinning, shrinking ellipse.
4. Sim: `Aimer` in build/spawn, `fire_round` plans, `launch_ball` aims, Out my way aims. World tests
   (`aim_tests.rs`, `testing` helpers, a blank scene with an emitter and a target emitter):
   - Straight in, no gravity, Ring start, 12 balls: every ball within 0.5 px of the target at the arrival step.
   - Straight in with gravity and Wind, Gap 0.05: all within 0.5 px at the same step.
   - Keep my directions with gravity, Fan: every reaching ball's path passes within 0.5 px of the target within one
     step of the arrival moment; unreachable ones skipped (`Fired.launched < size`), Fire anyway fires them all.
   - Keep without gravity: only the ball pointing at the target goes (Skip), at Speed.
   - Moving target: the other focus of a spinning, shrinking ellipse; and a target riding at a steady speed.
   - Ceiling: a target too near for a heavy pull at a tiny Arrive after → can't reach.
   - Out my way aimed: a caught ball reaches the target T after it goes.
   - Aim Off unchanged: the existing emitter tests and goldens pass untouched.
5. Editor: Aim rows and Target menu, warnings from `aim_checks`, mini view, preview paths. kittest: picking Straight
   in shows Arrive after and hides Speed; the Target menu lists emitters and sets `aim_target`; the warning line shows
   with drag.
6. CLAUDE.md, `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (debug), goldens untouched in debug
   and release (`cargo test -p bpe-kit --release --test golden*`), Windows exe builds.
7. One Opus review, fixes, PR.

## 5. Things not to do

- Don't change the integrator or anything when Aim is Off (goldens).
- Don't run a copy of the world to predict the target (that is part 4's test-firing, and costly).
- Don't store per-frame preview state; the preview and checks are worked out from the world each frame.
- Don't make up colours in editor code; the paths use design-system tokens.
- No built-in scenes.

## 6. Review (one Opus pass) and what changed

- Out my way aimed a caught ball under the catcher's pull: now the ball's own thing's (test covers it).
- A Balls thing as target was its start place: now its first ball's own path; moving targets get a card line.
- The next round's checks and paths showed for an emitter that won't fire again (Emits off, rounds used up): none now.
- A target that has gone shows "Nothing" in the menu. The wall check looks at at most 100 balls.
- Kept as is: rows hidden under an Aim still act when a ball goes unaimed (Fire anyway, or no target); Earlier
  rounds Remove with Every shorter than Arrive after cuts rounds before they land (as it would unaimed).
