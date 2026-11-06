# Rules part 4: Health

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md`, section 7 (and the Health rows of section 6). Part 1
(the engine core, `part1.md`) is merged; parts 2 (editor), 3 (break looks), 5 and 6 build beside this one. The
"As built" section at the end records anything that moved.

## Goal

A ball, a Balls group, an emitter (its balls) or a shape can get a **Health card** from "+ Add slot". Each ball (by
body row) or shape keeps its own health in the world's `RulesState`; hits take health away in the Rules stage before
any rule runs; at zero the thing (or the ball) breaks, or nothing happens and rules decide. Rules gain When **Health**
(drops / reaches 0 / goes up / changes), If **Health is** and Do **Change health**. Any keyframable setting can
**Follow health** (a pulse whose source is a thing's health share), and a group's card and look settings (and its
balls' Size) can **Vary across balls by Health**. A scene without a Health card simulates, draws and fingerprints
exactly as before (every golden and picture hash unchanged).

## Decisions (beyond the spec)

1. **Health lives by body row**, not ball number: an emitter's balls share numbers across rounds, rows never. A thing
   with no balls (a shape) has one health. The spec's "by thing and ball number" is read as "each ball its own".
2. **Only hits from** is a `Target` (the physics' own: Anything / Me / a thing / Tag / Kind) with its existing editor
   picker, not a rules `Who`: it needs nothing from part 2's who picker and reads with `World::names`. A hit with no
   other side counts only for Anything.
3. **Hits take**: Each hit (an amount, default 1), By strength (strength × factor, default 0.01: a 300 px/s hit takes
   3), Nothing. Plus **Least strength** (softer hits take nothing, default 0). Amounts may be negative (a hit that
   heals): few limits. Health never goes below 0 and has no top: healing past Starting health is allowed. A share
   (health ÷ starting health) reads 0 to 1 wherever it is read (Follow health, the Health spread), clamped.
4. **At zero**: Break / Nothing. The Break is an `Action::Break` queued in the Rules stage (applied at the next
   step's start, like any rule's), built from a template the kit fills (`HealthCard.at_zero`), so part 3's break
   looks reach it by filling the template's look without a runtime change. Health reaching 0 from a rule's Change
   health breaks in the same step's action pass (the Break is appended to the actions being applied).
5. **Starting health** may vary across balls, read once when the ball's health is first worked out
   (`VaryUse::AtLaunch`: Number, Round, Launch angle, Place, Random). It isn't keyframable, and Health settings
   can't be keyframed, pulsed or changed by Change setting in this part (the card isn't in Change setting's card
   list).
6. **Health spread** stores `BallSpread.hurt` = 1 − share (0 at full health, 1 at none), so a spread made without
   health (the editor's group view, a ball not hit yet) sits at full health. "Varies by Health": the setting's own
   value at full health, its To as health runs out. Offered only on a thing with a Health card, and only for
   settings read live (`VaryBy::changes`).
7. **Follow health** reads `PulseSource::Health { thing }`: the share of a thing without balls, else the mean share
   of its live balls (0 when none is alive), 0 for a thing without a Health card. Physics reads it in `animate`
   (after the step-start actions, so health as the last Rules stage and its actions left it); the picture reads it
   at the drawn step. The light probe passes it too. Sound knobs don't follow pulses at all today, so nothing there.
8. Health has no events in the log (no new event tag, so parts 3/5/6 keep theirs): the stage's health changes are
   `HealthMoment`s in `RulesState`, read by When Health and cleared at the end of each stage.
9. Health scenes get their own goldens, `tests/golden_health.rs` → `golden-rules-health*.txt` (inside the spec's
   `golden-rules*` family), so parts 3/5/6 adding rule scenes don't collide with them.
10. A scene with Health cards and no enabled rule still gets a `RulesState` (health only, no rules).
11. `SlotId::Health` is appended at the end of `SlotId::ALL` (after Mixer) so no existing slot's index moves.

## Layers and files

| Crate | File | What |
| --- | --- | --- |
| bpe-core | `ids.rs` | `SlotId::Health` (last; name "Health"; in the card list) |
| bpe-core | `animation.rs` | `PulseSource::Health { thing }`, `HealthLevels` trait, `Inputs.health`, `pulse_strength`, `set_pulse` allows it on keyframe settings |
| bpe-core | `edit.rs` | removing a thing drops pulses following its health |
| bpe-core | `vary.rs` | `VaryBy::Health`, `BallSpread.hurt`, `reads_health`, `offered(use_, lit, health)` |
| bpe-runtime | `rules/health.rs` (new) | `HealthSim` trait, `HealthCard`, `Takes`, `HealthChange`, `Hp`, `HealthMoment`, `Health` (the state), damage, applying changes |
| bpe-runtime | `plugins.rs` | `Sims::add_health`, `Sims::health` |
| bpe-runtime | `rules/mod.rs` | `RulesState.health`, `build` takes the world and returns `Some` for health alone, the stage works damage out first and clears moments last, `StageCx.health` |
| bpe-runtime | `rules/actions.rs` | `Action::Health { thing, row, change }`; `apply_rule_actions(sims, log)` handles it and appends zero breaks |
| bpe-runtime | `rules/sim.rs` | `StageCx.health: &Health` |
| bpe-runtime | `world.rs` | `World::health`, `hurt`, `hurts`, `health_levels`, `impl HealthLevels for World`; `ball_spread` sets `hurt`; `animate` passes health; fingerprint hashes Health cards |
| bpe-gfx | `frame.rs` | `build_frame_sensing` passes `health` |
| bpe-kit | `modules/health/mod.rs` (new) | `HealthSettings`, `HealthKind` (`health`), `HealthCardSim` (the `HealthSim`), `HEALTH` |
| bpe-kit | `rules/health.rs` (new) | `WhenHealth` (`when.health`), `HealthIs` (`if.health`), `ChangeHealth` (`do.health`), `health_values` |
| bpe-kit | `rules/mod.rs`, `rules/when.rs`, `rules/checks.rs` | register the three; Hit firings and Compare's thing values carry `health`/`share` |
| bpe-kit | `things/{balls,emitter,shape}/mod.rs` | `SlotId::Health` in `SLOTS` (an emitter's default slots no longer equal `SLOTS`) |
| bpe-kit | `things/balls/sim.rs`, `things/emitter/sim.rs` | Size varied by Health follows each ball's hurt |
| bpe-kit | `modules/look/layers/vary.rs` | `spread_of` sets `hurt` |
| bpe-kit | `sensing/probe.rs` | passes `health` |
| bpe-kit | `lib.rs` | registers the card kind and its sim |
| bpe-kit | `scenes/rule_scenes.rs`, `scenes.rs` | `health_scenes` (test-only) |
| bpe-kit | `tests/golden_health.rs`, `tests/golden-rules-health*.txt` | goldens |
| bpe-studio | `follow.rs`, `widgets.rs` | Follow health ▸ and Stop following health |
| bpe-studio | `vary.rs`, `widgets.rs` | "Health" spread, offered with a Health card |

## 1. Core

- `SlotId::Health` after `Mixer`; `ALL` gains it (length 10); `name()` "Health"; `in_card_list` true.
- `PulseSource::Health { thing: ThingId }`; `name()` "Health"; `follows_health()`.
- `pub trait HealthLevels { fn share(&self, thing: ThingId) -> f32; }` and `Inputs.health: Option<&dyn
  HealthLevels>` (`NONE` none, `Debug` shows it). `pulse_strength`: `inputs.health.map_or(0.0, |h| h.share(thing))`.
- `set_pulse`: Health like Sound and Light (any keyframe setting, or a pulse one).
- `edit.rs` remove-thing pulse clean-up: `PulseSource::Health { thing }` goes with its thing.
- `VaryBy::Health` (last, so saved names stay), doc "How much of a ball's health is gone (with a Health card)".
  `changes()` includes it; `reads_health()`; `offered(use_, lit, health)` filters it out without `health`.
  `BallSpread.hurt: f32` (0 full health); `t(Health) = hurt`.

## 2. Runtime (`rules/health.rs`)

```rust
pub enum Takes { EachHit(f64), ByStrength(f64), Nothing }
pub enum HealthChange { Damage(f64), Heal(f64), Set(f64), Refill }
pub struct HealthCard {
    pub start: f64,
    /// Starting health varied across the balls: its other end and how.
    pub start_vary: Option<(f64, bpe_core::vary::Vary)>,
    pub takes: Takes,
    pub least: f32,
    pub from: Target,
    /// What happens at zero: an action (a Break) whose thing, row, point, normal and strength are
    /// filled in when it is used (`aimed`); `None` for Nothing.
    pub at_zero: Option<Action>,
}
pub trait HealthSim: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn prepare(&self, params: &Params) -> HealthCard;
}
pub struct Hp { pub now: f64, pub start: f64 }
pub struct HealthMoment { pub thing, pub row: Option<u32>, pub before: f64, pub after: f64, pub start: f64,
    pub other: Option<Party>, pub point: Vec2, pub normal: Vec2, pub strength: f32 }
pub struct Health { things: BTreeMap<ThingId, HealthOf>, pub(crate) moments: Vec<HealthMoment> }
struct HealthOf { card: HealthCard, balls: bool, hp: Vec<Hp> }
```

- `Health::build(scene, resolved, world, sims)`: every built thing (`world.order`) whose resolved Health slot's kind
  has a `HealthSim`; `balls` = the world has bodies for it. A thing without balls gets its one `Hp` at once.
- `fill(world, sims)`: rows new since last time get `Hp { start, now: start }` with `start` from the card at the
  row's `world.ball_spread` (vary mixed exactly, not stepped), never below 0. Called at the start of every stage.
- Readers: `of(thing, row: Option<u32>) -> Option<Hp>` (a ball thing needs a row; a row not filled yet reads full
  at its card's plain start), `share(thing, row)`, `thing_share(world, thing)` (mean over live rows), `has(thing)`,
  `moments()`.
- `damage(world, events, broken, out)`: each Hit in log order, each side (a, and b when it is a party) whose thing
  has health, isn't broken, whose row is alive (balls), takes the card's damage when `strength ≥ least` and the
  card's `from` names the other side (`World::names(from, me, other)`; no other: Anything only). `change` the hp;
  a moment for a real change; at zero (before > 0, after ≤ 0) push `at_zero` aimed at it.
- `change(thing, row, HealthChange, point, other, strength) -> Option<(HealthMoment, bool zero)>`: the one place hp
  moves (clamped at 0, non-finite dropped, no moment when nothing moved).
- `RulesState.health: Health`; `build(scene, resolved, sims, world)` returns `Some` when a rule is enabled **or** a
  Health card is there. `run`: `health.fill`, `health.damage` (into `pending`), the rules (with `StageCx.health`),
  then `health.moments.clear()`. `RulesState::health()` for readers.
- `Action::Health { thing, row: Option<u32>, change: HealthChange }`. `apply_rule_actions(sims, log)`: a queue
  walked by index so a zero Break can be appended; `row: None` on a ball thing changes every live ball; each change
  makes a moment (point: the ball's place, or the thing's pose) for this step's stage.
- World: `health(thing, row) -> Option<Hp>`, `hurt(thing, row) -> f32`, `hurts(thing) -> Vec<f32>` (by row),
  `health_levels() -> Option<&dyn HealthLevels>` (some when health is kept), `impl HealthLevels for World`.
  `ball_spread` sets `hurt`; `animate` passes `health`; the fingerprint hashes each Health card's kind and params
  (only scenes with one change). `World::lone` keeps no rules, so no health (as part 1's test flights).

## 3. Kit

- `modules/health/mod.rs`: `HEALTH = "health"`; `HealthSettings` (group "Health"): `start` (Starting health, f64,
  range(0, 1e12), span(1, 20), default 3), `takes` (`HitsTake` Each hit / By strength / Nothing), `amount` (Each hit
  takes, default 1, span(0, 10), when takes=each_hit), `factor` (Per px/s, default 0.01, span(0, 0.1), when
  takes=by_strength), `least` (Least strength, px/s, range(0, 1e9), span(0, 2000)), `from` (Only hits from,
  Target, default Anything), `at_zero` (`AtZero` Break / Nothing). `HealthKind`: slot Health, fits balls, emitter
  and shape, `vary("start") = AtLaunch`. `HealthCardSim::prepare` reads them into a `HealthCard`
  (`at_zero` = `Action::Break` template for Break).
- `rules/health.rs`:
  - **When Health** (`when.health`; `who` Who (default Anything; That/Other/Group count as Anything), `event`
    Drops / Reaches zero / Goes up / Changes): a firing per matching moment of the stage: subject the ball (thing,
    row) or the thing (item 0), other the moment's, point/normal, values `health`, `damage` (before − after),
    `share`, `start`, `strength` and the ball's values.
  - **Health is** (`if.health`; `of` That ball / The thing it hit / A thing + `who`, `test` Below / Above / Between /
    At zero / Full, `measure` Points / Share (%), `amount`, `to`, both may come from the firing): no health → fails.
    Full is `now ≥ start`. Share compares `share × 100`.
  - **Change health** (`do.health`; `who`, `how` Damage / Heal / Set / Refill, `amount` from the firing): targets
    as `who::targets`; Group on a ball thing changes each live ball; a target without a Health card is skipped. A
    fixed who naming a thing without a Health card (or the scene) is a problem.
  - `health_values(cx, party, values)`: `health`, `share`, `start` of a party with health; called by Hit and
    Compare's thing values.
- Balls/Emitter `own_sizes` take each row's hurt (`World::hurts`, read before the state is borrowed); their
  `scripted` already resizes every step for a spread that changes.
- Draw `vary::spread_of` sets `hurt` from `cx.world.hurt`.

## 4. Editor

- `follow.rs`: `FollowMenu.health: Vec<(ThingId, String)>` (things with a Health card, tree order); `words`
  "<name> · Health"; `colour` the thing's; `now` includes Health pulses. `widgets.rs::follow_items`: "Follow health"
  submenu (things), "Stop following health" when following; "Stop following sound" only for sound.
- `vary.rs`: `by_words`/`by_help` for Health; `VaryCx::healthy()` (the thing has a Health card); `varies_line` takes
  it.

## 5. Tests (what passing looks like)

- core: `VaryBy::Health` offered only with health and only live; `t(Health)` reads hurt; pulse strength reads
  `share`; `set_pulse` takes Health on a keyframe setting; removing a thing drops its health pulse.
- runtime (`rules/tests.rs`, test health sim over the dots world): damage by each hit / strength / least / from;
  clamped at 0; zero queues a Break applied next step (ball removed); Nothing leaves it; Change health set/heal/
  damage/refill with moments; health without rules builds a `RulesState`; snapshot clone equal; no Health card =
  no state.
- kit (`rules/tests.rs` additions): Health card on a shape breaks after 3 hits; per-ball health in a group (one
  ball breaks, others don't); varied starting health; When Health drops/reaches 0 fire with values; Health is;
  Change health heals; Follow health moves a physics setting (bounciness) and a look colour in the frame copy;
  Size varied by Health shrinks a hit ball; seek replays equal.
- goldens: `tests/golden_health.rs` over `scenes::health_scenes` (Health bricks, Health group); blessed once in debug
  and once `--release`. Every existing golden (`golden*.txt`, `golden-rules*.txt`, sound goldens) unchanged.
- studio: Follow health menu offered with a Health card, sets the pulse and words; Health spread offered with a
  card.
- `cargo clippy --workspace` clean, `cargo fmt`.

## Not doing

- The break looks of part 3 on the Health card (the `at_zero` template gets its look when part 3 merges).
- Rules panel faces/summaries for the new blocks (part 2's editor gives every block a face from its settings).
- Health in the log, keyframes on Health settings, health on sound knobs, a top (most) health.

## As built

- Built as planned. Health lives in `bpe-runtime/src/rules/health.rs` (`Health`, `HealthOf`, `Hp`, `HealthCard`,
  `HealthSim`, `aimed`); the kit's card is `bpe-kit/src/modules/health/` and its blocks `bpe-kit/src/rules/health.rs`.
- `World::apply_rule_actions` walks its queue by index so a Health action that reaches zero appends its Break and
  that Break applies in the same pass (the "breaks at once" test).
- Sizes, looks and physics that follow or vary by health read it as the previous step's Rules stage left it (the
  scripted sizes run before the stage), the same one-step lag as Follow sound and Follow light.
- The emitter offers the Health card from "+ Add slot" but doesn't start with one (`DEFAULT_SLOTS`).
- Tests: `bpe-kit/src/rules/health_tests.rs` (16), runtime unit tests in `rules/health.rs`, core vary tests, the
  studio's `follow_health_is_offered_for_things_with_a_health_card`; goldens `tests/golden-rules-health*.txt`.
- From the review: a test flight's copy (`World::lone`) keeps the health as it is, held (`RulesState::lone`,
  `Health::held`: hits there take nothing), so aiming sees what Follow health reads; the world is built at full
  health (`FullHealth` for the build's 0 s keyframes); a group before its first ball reads full (1), and 0 only once
  every ball is gone; a starting health of 0 counts as `LEAST_START` so the first hit still reaches zero; endless
  amounts clamp (−∞ empties, +∞ caps at `f64::MAX`), only NaN is dropped; taking a Health card away drops Follow
  health of that thing (`drop_slot_animation`). At zero Break breaks at the next step's start, so a rule hearing it
  reach zero can't save it (the setting's help says so); pick Nothing and break it from a rule for that.
