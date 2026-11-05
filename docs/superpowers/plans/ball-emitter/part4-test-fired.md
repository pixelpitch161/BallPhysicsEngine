# Ball Emitter part 4: Through bounces (test-fired aiming) — plan

Date: 2026-11-05. Spec: `docs/superpowers/specs/2026-11-05-ball-emitter-design.md`, section 7 (design approved).
Branch `claude/project-thread-tvbkg6`, from main `8bbcf39` (parts 1, 2, 3, 5 and Leave trails in).
Built straight through, one Opus review at the end, one PR. No screenshots or videos.

## 0. What it does

An emitter's **Aim** gains **Through bounces**. Each ball keeps its own direction (as Keep my directions does), and
its speed and leaving time are found by **test-firing**: a lone copy of the ball is fired in a stripped copy of the
world (the walls, the ball's own Motion and Bounce cards, nothing else) at trial speeds until one passes through the
target, bouncing off whatever walls are in the way and moved by whatever Motion does. The ball then leaves so it gets
there **Arrive after** seconds after the round fired, with the others. Balls with no way through follow **Can't
reach**.

The spec fixes the setting, the "lone ball in a stripped world" method, the search (bracket + bisection on the signed
miss across the target, at most 40 test flights a ball), the cache key (the scene's physics fingerprint and the
emitter's settings) and the editor's background aiming ("Aiming… 37%"; export waits). Everything else is decided
here.

## 1. Decisions this plan makes (spec gaps)

1. **Exact by construction.** A test flight *is* the world's own stepping (`World::step` on a stripped copy), from
   the very moment the real ball sets off, so a found way is the way the real ball goes, step for step, as long as
   nothing that was stripped acts on it. What can differ (noted in code, warned where cheap):
   - its balls bump into other balls (Bounce `hit_balls`): warned, as in part 3;
   - its Motion card varies across the balls: the lone ball uses the card's own values: warned, as in part 3;
   - steps split into substeps by the fastest ball in the *whole* scene; the lone copy splits by the lone ball only
     (only when some other ball passes 3,840 px/s): noted, not warned;
   - a physics setting following sound hears the lone copy's (empty) hits: noted, not warned;
   - a wall riding a ball (or riding a thing that rides one) stays where it is in the lone copy: noted.
2. **Which ways count.** Direction is kept; only speed is searched (spec). A *pass* is the ball going by the target:
   the flight's step polyline, taken relative to the target (each end minus the target's place at that moment, so a
   moving target is followed exactly as part 3's `Track` says), has a segment the target is abreast of (its foot on
   the segment). Its **signed miss** is the cross distance there (left of the way positive). Passes are grouped by
   **arc**: how many wall hits the ball had before it (segments in a step with a hit are skipped). Within one arc the
   miss moves smoothly with speed, so a sign change between two speeds on the same arc brackets a way through.
3. **Bounces at least** (new setting `aim_bounces`, Int 0..20, default 1, shown under Through bounces): only passes
   on arc ≥ N count, so Through bounces really bounces by default (0 allows a straight shot).
4. **Which way wins**, when several do: the fewest bounces, then the speed nearest **Speed** (in ratio). So with no
   pull at all (where the path's shape doesn't depend on speed), a direction that passes through goes at exactly
   Speed when that gets there in time — as Keep my directions does. Speed stays shown (it is the first try and the
   tie-break); Speed spread, Gap and Order are hidden (each ball sets its own leaving time, as under Keep).
5. **The search, per ball** (budget `MOST_FLIGHTS` = 40 flights):
   1. Fly at Speed (when > 0).
   2. Scan: `SCAN` = 12 speeds evenly in ratio from `lo = 0.5·min(quickest, Speed)` to
      `hi = min(ceiling, 4·max(quickest, Speed))`, `quickest` = D/Tb, D the distance to the target where it will be at
      arrival, Tb the longest flight that can still arrive in time (arrival moment − the earliest set-off moment).
   3. A pass within `GOOD` (0.45 px) on an allowed arc is a way (costs nothing more).
   4. Brackets: neighbouring speeds (sorted, Speed's flight included) whose passes on the same allowed arc have
      opposite signs, both near the target (|miss| ≤ `max(0.3·D, 40 px)`: far-abreast passes of a long flight are not
      real crossings). Tried best first (fewest bounces, nearest Speed), each by the Illinois method (false position,
      halving the stale end) towards `TOL` (0.25 px), stopping when the bracket stalls (relative width 1e-6); a trial
      speed whose flight has no pass on that arc means the arc broke there (a different wall), so that bracket is
      dropped and the next tried. With nothing yet, the speed between the two nearest neighbouring flights is tried.
   5. **Edge bounces quantise**: with Touch walls with Edge a bounce lands on whole steps, so the miss against speed
      is a sawtooth (jumps of 2–3 px when a bounce moves to the next step) and may never come within `TOL`. So the
      nearest flight is taken when within `GOOD`; otherwise the direction is turned by at most `MOST_TWEAK` (0.3°)
      around the nearest speed, by the same Illinois search on the angle (a turn of 0.3° is never seen).
   6. Each flight flies at most Tb steps; a pass after Tb doesn't count (too slow to arrive in time).
6. **When it leaves.** A way passes the target `e` steps after setting off; it sets off from `start = round(A − e)`
   (A the arrival moment), leaving on step `start + 1` (or on the firing step when `start` is the earliest moment),
   so it arrives within half a step of the others. Walls or the target may move, and the emitter itself may move, so
   when `start` isn't the earliest moment the way is **checked from `start`**: the stripped copy is stepped (no ball)
   to that moment and the ball fired again from the emitter's place then. If it no longer passes within `GOOD`, the
   speed is found again close by: the same speed, then secant steps on that arc (each clamped to ±6 %), then a fresh
   search around it. When the new way wants another start, `start` is moved by a secant on
   `g(start) = start + e − A`, at most `SETTLE_TRIES` (6) times. **Deviation from the spec's 40 flights:** the
   settling gets its own `SETTLE_FLIGHTS` (24) on top of the 40 the search may use, because a spinning or shrinking
   shape moves every bounce when `start` moves; a still scene costs one extra flight.
7. **Can't reach**: no way in the budget → Skip (not fired; takes no number) or Fire anyway (goes at once, at Speed,
   unaimed). A way that would have to set off before the round fired (`start` < earliest) → Skip, or Fire anyway: goes
   at once at the found speed (late).
8. **Out my way** (catch, Send back out) aims through bounces too: the caught ball is test-fired as a round of one
   firing when it would leave (catch step + 1, or its Delay's end), with **its own thing's** Motion and Bounce cards
   lent to the lone copy (`World::lend_cards`) and its own size. Can't reach: Skip removes it (a Remove event), Fire
   anyway sends it unaimed. Same way back is never aimed.
9. **The stripped world** (`World::lone`): a copy of the world taken during the firing step's Spawn stage, set back to
   the moment before the step (`step = now(fire)`), so stepping it runs that very step again with only:
   - every thing without balls (shapes, folders, camera, scene) and the emitter itself; other emitters and Balls
     things are left out (not stepped; their poses stay where they are);
   - the emitter's state swapped for a **quiet** copy (fires nothing, catches nothing; `EmitterState.quiet`, which
     `settings_changed` keeps quiet) and its bodies emptied;
   - no history; keys and pulses only on kept things (so keyed walls and keyed Motion move as they will);
   - its Motion and Bounce cards as the world has them (a varied card without per-ball rows).
   One lone ball is pushed into the emitter's bodies at the set-off moment with the emitter's ball size (Size ×
   size variety of the number it will get is *not* known at firing: the card's Size is used; noted).
10. **Cache** (`aim_cache.rs`): one per process, keyed by `(fingerprint, emitter, what)` where `what` is
    `Round { round, fire }` or `Catch { step, thing, row }`. The fingerprint is the scene's physics fingerprint (the
    no-history one, `sound_fingerprint`'s), which the world now keeps (`World.fingerprint: Option<u64>`, worked out in
    `World::build` only when a sim asks: `ThingSim::needs_fingerprint`, true for an emitter aiming Through bounces).
    The physics is deterministic, so the same key always gives the same answer: a hit changes nothing but the time it
    takes. An entry being worked out by one thread is waited for by another (no work twice). At most 4 fingerprints
    and about 200,000 aimed balls are kept (oldest fingerprint goes first). A world with no fingerprint works aims out
    without caching.
11. **Background aiming in the editor** (`aim_job.rs`, `AimJob`): when the playback is rebuilt and some emitter aims
    Through bounces, a thread runs its own copy of the world from 0 s to the scene's end, filling the cache as rounds
    fire (stopped when superseded or the editor closes). The editor's own playback never steps past where that thread
    has got to while it runs, so the UI never works an aim out itself; while it is held back the preview shows
    "Aiming… 37%" (how far the thread has got towards the playhead) in its top-left corner, and repaints. The sound
    thread, the export and tests work aims out themselves (or wait on the thread's entry): export waits for them.
12. **Rounds aim in parallel**: a round's balls are searched on up to `available_parallelism` (at most 8) threads with
    `std::thread::scope`, each ball on its own clones; results are put back in ball order, so the answer doesn't
    depend on the threads.
13. **What the sim keeps**: `EmitterState.aimed: Vec<Aimed>` (`Aimed { round, aims: Arc<RoundAim> }`), the rounds
    still launching plus the latest; `launch_ball` reads its ball's velocity from there (shared by `Arc`, so world
    snapshots stay cheap). `RoundAim { fire, arrive, meet, balls: Vec<Option<BallAim>> }`,
    `BallAim { leave, velocity, reaches, path }` (`path`: at most 96 places along the found flight, for the preview).
14. **Preview and checks**: a selected emitter aiming Through bounces draws the **latest round's** found paths (faint,
    at most 400 balls) and a ring where they meet (the next round's can't be worked out every frame). The card's lines:
    "Pick what to aim at."; "N of M balls can't reach it in time: they are skipped / go anyway." from the latest
    round; "Something in the way: balls may miss." for `hit_balls`, a varied Motion card or a varied Size (the test
    flights use the card's Size; walls, drag and wind are inside the test flights); "The walls move: fewer balls find
    a way through." when a wall spins, shrinks or moves (a fast spin breaks most ways between scan speeds); the
    moving-target line as in part 3. Before any round has fired there are no counts.
15. **Defaults**: `aim` Off (unchanged), `aim_bounces` 1. Aim Off and the other aims change nothing: goldens don't move.

## 2. Files

New:
- `crates/bpe-kit/src/things/emitter/test_fire.rs`: the lone world, flights, passes, the search, `aim_round`,
  `aim_catch`.
- `crates/bpe-kit/src/things/emitter/aim_cache.rs`: the shared cache.
- `crates/bpe-kit/src/things/emitter/test_fire_tests.rs`: its tests.
- `crates/bpe-studio/src/aim_job.rs`: the background aiming thread.

Changed:
- `bpe-runtime/src/plugins.rs`: `ThingSim::needs_fingerprint(&self, resolved) -> bool { false }`.
- `bpe-runtime/src/world.rs`: `pub fingerprint: Option<u64>`; `World::lone(&self, keep: &dyn Fn(ThingId) -> bool)
  -> World`; `World::lend_cards(&mut self, from: &World, owner: ThingId, to: ThingId)`.
- `bpe-kit/src/things/emitter/mod.rs`: `Aim::ThroughBounces`, `aim_bounces`, the `when`s (Gap, Order hidden under
  Through bounces), `pub mod test_fire; pub mod aim_cache;`.
- `bpe-kit/src/things/emitter/sim.rs`: `EmitterState.{quiet, aimed}`, `Moment.through`, aims worked out before
  `fire_and_launch` borrows the state (spawn, after_build), `launch_ball` reads them, Out my way through bounces,
  `needs_fingerprint`.
- `bpe-kit/src/things/emitter/aim.rs`: `next_round` empty under Through bounces; `check` from the latest round.
- `bpe-kit/src/lib.rs`: `lone_sims()` (a process-wide `Sims` the test flights step with; the sim hooks get no `Sims`).
- `bpe-studio`: `app.rs` (start/stop the job with the playback, held catch-up), `preview.rs` (held seek, "Aiming…",
  latest round's paths), `main.rs`/`lib` module list.
- `CLAUDE.md`: Through bounces in the emitter paragraph, the runtime's `lone`/`fingerprint`, the editor's job.

## 3. Types and functions

```rust
// bpe-runtime
pub trait ThingSim { fn needs_fingerprint(&self, _r: &ResolvedThing) -> bool { false } }
pub struct World { pub fingerprint: Option<u64>, .. }
impl World {
    pub fn lone(&self, keep: &dyn Fn(ThingId) -> bool) -> World;
    pub fn lend_cards(&mut self, from: &World, owner: ThingId, to: ThingId);
}

// test_fire.rs
pub const MOST_FLIGHTS: u32 = 40;
pub const SETTLE_FLIGHTS: u32 = 24;
pub const SCAN: usize = 12;
pub const TOL: f64 = 0.25;
pub const GOOD: f64 = 0.45;
pub const MOST_TWEAK: f64 = 0.3; // degrees
pub const PATH_POINTS: usize = 96;
pub const SETTLE_TRIES: u32 = 6;
pub struct BallAim { pub leave: u64, pub velocity: Vec2, pub reaches: bool, pub path: Vec<Vec2> }
pub struct RoundAim { pub fire: u64, pub arrive: u64, pub meet: Vec2, pub balls: Vec<Option<BallAim>> }
pub struct Shot { pub start: Vec2, pub way: Vec2 }        // one ball's place (at firing) and direction
struct Lone { world: World, me: ThingId, radius: f32 }     // the stripped copy at some moment
pub fn lone_at(world: &World, me: ThingId, owner: ThingId, radius: f32, moment: u64) -> Lone
struct Flight { passes: Vec<Option<Pass>> /* per arc */, path: Vec<Vec2>, flights_used }
struct Pass { miss: f64, e: f64 }
fn fly(lone: &Lone, p0, v, aim: &Aimer, setoff: u64, most: u64) -> Flight
fn search(lone, shot, aim, s, earliest, budget) -> Option<Found>     // decision 5
fn settle(base, shot, aim, s, found, fire, budget) -> Option<BallAim> // decision 6
pub fn aim_round(world: &World, me: ThingId, s: &EmitterSettings, aim: &Aimer, fire: u64, round: u32, n: u32,
                 seed: u64) -> Arc<RoundAim>
pub fn aim_catch(world: &World, me, s, aim, owner: ThingId, radius: f32, shot: Shot, fire: u64) -> Option<BallAim>

// aim_cache.rs
pub enum What { Round { round: u32, fire: u64 }, Catch { step: u64, thing: ThingId, row: u32 } }
pub fn cached<T: Clone + Send + Sync + 'static>(fp: Option<u64>, me: ThingId, what: What, work: impl FnOnce() -> T) -> T
```

## 4. Steps

1. **Runtime** (`World::lone`, `lend_cards`, `fingerprint`, `needs_fingerprint`). Tests in `world.rs`: a lone copy
   keeps walls and the kept thing, drops other bodies and history; stepping a lone copy with one ball pushed gives
   exactly the positions the full world gives that ball when nothing else touches it (`testing` scene).
2. **Settings**: `Aim::ThroughBounces`, `aim_bounces`, `when`s; settings test: Gap/Order hidden, Speed shown,
   Bounces at least shown only under Through bounces.
3. **test_fire.rs** (TDD): passes and signed misses on hand-made polylines (moving target included); the search
   finds a speed through a target behind one bounce in a box with gravity (flight re-run passes within GOOD, 0.45 px);
   picks the fewest bounces and nearest Speed; no pull + a direction through the focus of an ellipse (Touch Middle)
   goes at Speed in one flight; no way → None; never more than 40 + 24 flights (counted).
4. **Cache**: same key computes once (counter), waiting thread gets the same value, eviction past 4 fingerprints.
5. **Sim**: aims before `fire_and_launch`; `launch_ball` reads `aimed`; Out my way. World tests (`test_fire_tests.rs`):
   - a box (polygon shape, gravity), emitter in one corner firing a fan of 6 up and away, target emitter in the other
     corner behind a bounce: every reaching ball passes within 0.5 px of the target within one step of the arrival
     moment, with at least one hit before (from the log);
   - an ellipse (Touch walls with Middle, no gravity) emitter on Focus 1, 16 balls All round, target = other focus:
     all reach, all at Speed, leave together;
   - a slowly spinning shape (2°/s): still within 0.5 px (the check from `start` follows the walls); fast spins are
     only warned about (walls_move);
   - Skip vs Fire anyway counts (`Fired.launched`), Out my way aimed through a bounce;
   - the cache: building the world twice gives identical logs; a second build doesn't fly again (cache counter);
   - Aim Off and the other aims: existing tests and goldens untouched.
6. **Editor**: `AimJob` (start on rebuild when wanted, stop on drop, `reached`), held catch-up and "Aiming… N%",
   latest round's paths, card lines (Bounces at least row comes with the settings). kittest: Through bounces shows
   Bounces at least and Speed, hides Gap; `AimJob::wanted` only for Through bounces.
7. CLAUDE.md, `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (debug), goldens untouched in debug
   and release, Windows exe builds.
8. One Opus review, fixes, PR.

## 5. Things not to do

- Don't change anything when Aim isn't Through bounces (goldens, and part 3's behaviour).
- Don't write a second physics for the test flights: they step a real (stripped) `World`.
- Don't let the editor's own playback work an aim out (it waits for the job), and don't make the physics depend on
  whether the cache had an entry.
- Don't keep per-frame preview state; the paths come from the latest round's `RoundAim` in the world.
- Don't make up colours in editor code; the label and paths use design-system tokens.
- No built-in scenes; don't re-bless existing goldens.
