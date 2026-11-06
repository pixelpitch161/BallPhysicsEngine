# Rules part 1: engine core

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md` (sections 3, 4, 5, 6 and 9.1). Headless: no editor change
but loading and saving (and one `value_words` arm for the new value). Every decision below is fixed for the build; the
"As built" section at the end records anything that moved.

## Goal

A scene can hold rules (format 12). The world runs them in a Rules stage after every step, queues their actions and
applies them at the next step's start, so every rule's effect is part of the simulation: cloned in snapshots,
identical on replay, in the sound engine's own playback and in export. Blocks in this part: When Hit / Time / Beat;
If Compare / Is / Chance / Every Nth / Cooldown / Only N times / Between times; Logic (every mode); Do Change motion,
Change setting (whole thing and per ball, physics and look, over keyframes) and Break (Vanish). A scene without an
enabled rule simulates and draws byte-identically (goldens and picture hashes unchanged).

## Layers and files

| Crate | File | What |
| --- | --- | --- |
| bpe-core | `src/rules.rs` (new) | `RuleId`, `BlockId`, `Rules`, `Rule`, `Block`, `BlockSection`, `Who`, `CopiedRule`, `CopiedBlock`, structure checks (`Rule::problems`), the edits, "from the firing" companion keys |
| bpe-core | `src/registry.rs` | `RuleBlockKind` trait; `Registry::add_block`, `block`, `blocks` |
| bpe-core | `src/value.rs`, `settings.rs` | `Value::Who`, `ValueType::Who`, `SettingValue for Who` |
| bpe-core | `src/scene.rs` | `Scene.rules` (skipped when empty), `FORMAT` 12 |
| bpe-core | `src/save.rs` | `clean_rules`: one block per id, inputs to missing blocks dropped, params cleaned against the kind, next ids mended |
| bpe-core | `src/contract.rs` | block kinds pass the same settings contract |
| bpe-runtime | `src/rules/mod.rs` (new) | `RulesState`: prepared rules, the stage, the queue, `Problem`s, fired counts |
| bpe-runtime | `src/rules/firing.rs` | `Firing`, `Values` |
| bpe-runtime | `src/rules/sim.rs` | `BlockSim` trait, `PrepareCx`, `StageCx` |
| bpe-runtime | `src/rules/actions.rs` | `Action`, `MotionChange`, applying them |
| bpe-runtime | `src/rules/changes.rs` | `ChangeKey`, `Change`, `How`, `Lasts`, folding changes onto values |
| bpe-runtime | `src/events.rs` | `EventKind::Rule` (tag 10), `EventKind::Break` (tag 11) |
| bpe-runtime | `src/plugins.rs` | `Sims::add_block`, `Sims::block` |
| bpe-runtime | `src/world.rs` | `World.rules`, the two hooks in `step`, `animate` over rule changes, per-ball card states (`Slotted.own`), `fingerprint_of` hashes the rules, `lone` drops them, `World::rules()`, `World::broken`, `World::ruled` |
| bpe-runtime | `src/rows.rs` | `Rows::params_at` (a row's varied params) |
| bpe-gfx | `src/frame.rs` | `build_frame_sensing`: broken things are left out; whole-thing changes applied after keyframes |
| bpe-kit | `src/rules/` (new) | `mod.rs` (register, `kit_registry`), `who.rs`, `amount.rs`, `when.rs`, `checks.rs`, `logic.rs`, `motion.rs`, `change.rs`, `breaks.rs`, `tests.rs` |
| bpe-kit | `src/lib.rs` | registers block kinds and block sims |
| bpe-kit | `things/balls/sim.rs`, `things/emitter/sim.rs` | per-ball Size changes through `World::ruled` |
| bpe-kit | `things/balls/draw.rs`, `modules/look/layers/vary.rs` | per-ball Look changes: a ball with changes wears its own stack and trails |
| bpe-kit | `sensing/probe.rs` | the light probe sees rule changes and broken lights |
| bpe-kit | `src/scenes_rules.rs` (test-only), `tests/golden_rules.rs`, `tests/golden-rules*.txt` | rule scenes and their goldens |
| bpe-studio | `widgets.rs` | `value_words` for `Value::Who` (the only editor line) |

## 1. Data (bpe-core `rules.rs`)

```rust
#[serde(transparent)] pub struct RuleId(pub u32);
#[serde(transparent)] pub struct BlockId(pub u32);
pub struct Rules { pub list: Vec<Rule>, pub next_rule: u32 }          // #[serde(default)]
pub struct Rule { pub id: RuleId, pub name: String, pub enabled: bool, pub blocks: Vec<Block>, pub next_block: u32 }
pub struct Block { pub id: BlockId, pub kind: KindId, pub inputs: Vec<BlockId>, pub params: Params }
pub enum BlockSection { When, If, Logic, Do }
```

- `Rules::is_empty` (no rules) drives `skip_serializing_if`; `Rules::any_enabled`.
- `Scene.rules: Rules` beside `pulses`. `Scene::FORMAT` = 12, no upgrade: an older file simply has none.
- **`Who`** (a setting value, `Value::Who`, `ValueType::Who`):
  `That` (default), `Other`, `Group`, `Thing { thing: ThingId, ball: Option<u32> }` (ball numbers are 1-based in rules,
  as the editor says "Ball 1"), `Tag(String)`, `Kind(KindId)`, `Anything`. "The scene / camera / a light" are
  `Thing` with that id. `Who::names_thing()` gives the fixed thing it needs (for the deleted-target problem).
- **From the firing**: a number setting `k` of an If or Do block reads `"<k>~from"` (Text: a value's name, empty or
  missing = the fixed number), `"<k>~times"` (default 1) and `"<k>~plus"` (default 0): `value × times + plus`.
  `from_key`, `times_key`, `plus_key`, `split_from(key)`, `is_from_key`; `block_def(kind, key)` answers them (Text,
  Number, Number). They are plain params, cleaned on load like vary keys.
- **Structure** (`Rule::problems(&self, reg) -> Vec<RuleProblem>`): unknown kind; When with inputs; If/Do with not
  exactly one input; Logic with fewer than two; an input that isn't in the rule; a cycle (Kahn's algorithm in block
  order); a block naming a thing that isn't in the scene (`Scene::rule_problems` adds this one: any `Value::Who`
  param whose `names_thing()` is missing). `RuleProblem { block: Option<BlockId>, what: RuleProblemKind }` with
  `words()` for the editor.
- **Edits** on `Scene` (each an undo step in the editor; `EditError` on bad ids): `add_rule(name) -> RuleId`,
  `remove_rule`, `move_rule(id, to_index)`, `duplicate_rule` (new rule id, same block ids), `rename_rule`,
  `set_rule_enabled`, `add_block(reg, rule, kind, after: Option<BlockId>) -> BlockId` (a When ignores `after`; any
  other block takes `after` as its input; a Logic block added after a block takes it as its first input),
  `add_when(reg, rule, kind)`, `remove_block` (its outputs that had it as an input take its inputs where the result
  is a valid count, else just drop it), `link(rule, from, to)` (onto a Logic: adds an input; onto an If/Do: replaces;
  onto a When: `EditError::WrongType`; a link that would close a cycle: `EditError::Loop`), `unlink(rule, from, to)`,
  `copy_rule`/`paste_rule` (`CopiedRule`, pasted with a fresh id, named "<name> copy" when that name is taken), `copy_block`/
  `paste_block(rule, after)` (`CopiedBlock`, fresh id, input `after`). Block params are set with
  `set_block_param(reg, rule, block, key, value)` (typed and clamped like `set_at`; default removed).
- `Registry::add_block(impl RuleBlockKind)`; trait: `id`, `name`, `help`, `section`, `settings`, `dynamic_defs(get)`
  (default none), `summary(get) -> String` (default the name). Ids share the one id space (`check_new`).
- Load (`save.rs::clean_rules`): one rule per id, one block per id in a rule, inputs that name no block of the rule
  dropped, `next_rule`/`next_block` at least one past the largest id, known kinds' params cleaned like a card's (plus
  their from-companions). Unknown kinds keep everything for the problem mark.

## 2. Runtime (bpe-runtime `rules/`)

### 2.1 Plugins

```rust
pub trait BlockSim: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn section(&self) -> BlockSection;
    /// Reads the settings once; Err(words) is a problem that skips the rule.
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String>;
    fn fire(&self, _state: &mut dyn ThingState, _cx: &StageCx, _out: &mut Vec<Firing>) {}              // When
    fn pass(&self, _state: &mut dyn ThingState, _firing: &Firing, _cx: &StageCx) -> bool { true }        // If
    fn join(&self, _state: &mut dyn ThingState, _input: usize, _inputs: usize, _firing: &Firing, _cx: &StageCx,
            _out: &mut Vec<Firing>) {}                                                                    // Logic
    fn tick(&self, _state: &mut dyn ThingState, _inputs: usize, _cx: &StageCx, _out: &mut Vec<Firing>) {} // Logic
    fn act(&self, _state: &mut dyn ThingState, _firing: &Firing, _cx: &StageCx, _out: &mut Vec<Action>) {} // Do
    fn changes_settings(&self) -> bool { false }  // do.change: the world keeps an animated copy
}
```

`PrepareCx { scene, resolved, rule, block }`; `StageCx { world, events (this step's new log events), step, time,
rule, block, sims }`. A block's state is one `Box<dyn ThingState>` (its settings and its memory), cloned with the
world.

### 2.2 `Firing`

`Firing { step, subject: Option<Party>, other: Option<Party>, point: Vec2, normal: Vec2, values: Values }`,
`Values(Vec<(&'static str, f64)>)` with `get`, `set`, `merge_missing`. Names used in part 1: `strength`, `speed`,
`size`, `hits`, `ball` (1-based), `time`, `count`, `bar`, `beat`. A Party's `item` is the body row for balls.

### 2.3 `RulesState`

Built in `World::build` after everything else (so it sees the built world), `None` when no rule is enabled and no
block exists. Holds: `rules: Vec<RunRule>` (`RunRule { id, blocks: Vec<RunBlock>, outputs: Vec<Vec<(usize, usize)>>
(per block: (next block index, input index)), problem: Option<String>, busy_at: Option<u64> }`, `RunBlock { id,
section, kind: &'static str?, state }`), `pending: Vec<Action>`, `changes: BTreeMap<ChangeKey, Vec<Change>>`,
`ended: BTreeSet<ChangeKey>` (keys that lost their last change since the world last animated), `dirty:
BTreeSet<ChangeKey>`, `broken: BTreeSet<ThingId>`, `frozen: Vec<Frozen>` (thing, row, until step, velocity),
`changes_settings: bool`.

Problems: structure (`Rule::problems`), a block kind with no sim, `prepare` errors. A rule with a problem is skipped
whole; `RulesState::problem(rule)` reads it.

### 2.4 The stage (`World::run_rules`, after the history record)

For each rule in list order, enabled and without problem, with an action budget of `MOST_ACTIONS` (4096):
1. Logic blocks' `tick` (in block order), each output delivered.
2. When blocks in block order: `fire` → firings, each delivered.
3. `deliver(block, firing)`: for each output (next, input) in block order: If → `pass`; Logic → `join`; Do → `act`
   (actions into the queue while the budget lasts; past it the rule stops for this step and notes `busy_at`).
   Every block that fires (When fires, If passes, Logic completes, Do acts) is noted once per step with its
   firing's subject.
4. After the rule: one `EventKind::Rule` event per noted block (block order): `a` the subject (or `(ROOT, 0)`),
   `b = Party { thing: ROOT, item: rule × 65536 + block }`, point the firing's point.

Nothing random but hashed: `rules::dice(seed, rule, block, step, subject) -> f32`.

### 2.5 Actions (applied first thing in `World::step`, after `step += 1`, before `animate`)

```rust
pub enum Action {
    Motion { thing: ThingId, row: Option<u32>, motion: MotionChange },   // None: every live ball of it
    Change { key: ChangeKey, change: Change },
    Break { thing: ThingId, row: Option<u32>, point: Vec2, normal: Vec2, strength: f32 },
}
pub enum MotionChange { AddSpeed(f32), SetSpeed(f32), MulSpeed(f32), Push(Vec2), Stop, AimAt(Vec2), Freeze(f32), Teleport(Vec2) }
```

- Skipped silently: a broken thing, a dead or missing row, anything non-finite (checked again here).
- Add/Set speed act along the ball's direction (straight up when it is still). Freeze holds the ball
  (`Bodies.held`) and keeps its velocity, given back when it lets go (frozen list, released at the step's start; a
  ball frozen again keeps the later end). Teleport moves `position`, `previous` and `start` together.
- Change: pushed onto its key's list (a Set with no glide that lasts for good drops the earlier changes on that key).
  Marks the key dirty.
- Break of a row: `alive = false`, `Break` event then `Remove` event (both at this step). Break of a whole thing:
  `broken.insert`, its wall removed, every live ball removed (a `Remove` each), dropped from `order` (its sim stops: a
  camera stays where it last looked, an emitter stops firing), its per-ball and whole changes dropped. `Break` event:
  `a = (thing, row or u32::MAX)`, point/normal/speed from the hit. Riders of it keep their last pose.
- Releasing frozen balls happens before the actions, so a Freeze queued this step holds from now.

### 2.6 Changes (`changes.rs`)

`ChangeKey { thing, slot: Option<SlotId>, row: Option<u32>, key: String }` (ord: thing, slot, row, key).
`Change { how: How (Set | Add | Multiply), value: Value, start: f64, glide: f32, ease: Ease, lasts: Option<f32>,
range: Option<(f64, f64)> }`.
- Weight at `t`: 0 before `start`; up `ease((t − start)/glide)` (1 with no glide); when it lasts `n`, from
  `start + n` the weight falls `1 − ease((t − start − n)/glide)`; done (removed) after `start + n + glide`.
- Fold onto a base, in order: Set `base.lerp(value, w)`; Add `base + value × w` (numbers, ints, pairs; a non-number
  Add is a Set); Multiply `base × (1 + (m − 1) w)` (numbers, ints, pairs); then clamped to `range` (hard range only;
  never the slider's span). Non-finite results keep the base.
- `RulesState::apply_whole(resolved, time, keep: impl Fn(thing, slot, key) -> bool)`,
  `apply_ball(thing, slot, row, params, time)`, `ball_rows(thing, slot)`, `moving(key, time)`, `prune(time)`
  (drops finished changes, noting `ended`).

### 2.7 Physics settings (`animate`)

- The world keeps `Animated { animation, resolved, pristine: Option<Arc<Resolved>> }` when there is animation **or**
  some prepared block `changes_settings()`. `pristine` (the built-from settings) only in the second case.
- Each step: `prune(time)`; every whole-thing key that changed or ended is reset from `pristine`; the animation
  applies; `apply_whole` (physics keys only: Motion/Bounce cards, or a thing's own keys its sim says
  `affects_physics`) writes the changes; the cards and things to refresh are the animation's destinations plus
  those keys (the existing "same params" check keeps unchanged cards).
- **Per ball, Motion and Bounce**: `Slotted.own: BTreeMap<u32, Arc<dyn ThingState>>` (rows with changes);
  `state_for(row)` reads `own` first; `varies()` is true when `own` is not empty. Rows are prepared again when their
  changes are dirty, moving (in a glide), ended, or the card was prepared afresh: params = the card's, varied at the
  row's spread (`Rows::params_at`) when the card varies, then `apply_ball`. Only for a sim with `per_ball()`;
  otherwise the change is turned into a whole-thing change when it is made (the kit decides; section 3.6).
- **Per ball, a thing's own keys** (Size): things with per-ball changes that are dirty/moving/ended hear
  `settings_changed`; `World::ruled(thing, None, key) -> Option<Ruled>` (a copy of the rows' changes and the time)
  lets the sim fold them over each ball's own value. Balls and Emitter use it for `radius`.
- `World::lone` copies no rules and no pristine (what rules did so far stays; nothing more happens).

### 2.8 Fingerprint, snapshots, sound

- `fingerprint_of` hashes `format!("{:?}", scene.rules)` when any rule is enabled; nothing otherwise.
- Snapshots and playback checkpoints clone `World`, rules included.
- The sound engine builds its worlds with the same sims (`bpe_sound::kit()` takes `bpe_kit::sims()`), so rules run
  there too; rules don't change the sound patch in part 1 (`Sound`/`Mixer` cards are refused by Change setting).

## 3. Kit blocks (bpe-kit `rules/`)

Every kind is a `#[derive(Settings)]` struct, a `RuleBlockKind` (registered in `registry()`) and a `BlockSim`
(registered in `sims()`). Ids: `when.hit`, `when.time`, `when.beat`, `if.compare`, `if.is`, `if.chance`, `if.every`,
`if.cooldown`, `if.only`, `if.between`, `logic`, `do.motion`, `do.change`, `do.break`. Settings group "Rule" (added
to `SECTION_ORDER`).

`kit_registry()` (a process-wide `OnceLock<Registry>`) answers setting definitions for `do.change`.

### 3.1 Who (`who.rs`)

- In a When: `matches(world, who, party)`: `Thing { ball }` also checks the row's ball number; `Tag`, `Kind`,
  `Anything` by the world's tags and kinds; `That`/`Other`/`Group` count as Anything there.
- In an If/Do: `targets(world, who, firing) -> Vec<(ThingId, Option<u32>)>`: That = the subject (its row when it is a
  ball), Other = the other side, Group = the subject's thing, `Thing { ball: Some(n) }` = that ball's row (none when
  the group has no such ball yet), Tag/Kind/Anything = every built, unbroken thing that matches, whole, in id order.

### 3.2 When

- **Hit** (`who`, `whom`, `min_strength`): every `Hit` event of the step in log order where `a` matches who and `b`
  whom; a ball-on-ball hit is also tried the other way round (b as the subject) so either side can be "who".
  Firing: subject the matching side, other the other, point/normal (normal flipped for the mirrored one),
  values strength (the event's speed), speed (the subject's speed now), size (its radius), hits (its hits so far), ball
  (its number + 1).
- **Time** (`at`, `repeat` switch, `every` (≥ 1/240 s), `until` switch + `until_at`): moments `at + k·every`. The stage
  of step `s` takes the moments with `step_at(m) == s` (step 1 also takes those of step 0). Values time, count.
- **Beat** (`every`: Beat / Bar / Bars, `bars`, `offset` in beats): the tempo from the scene settings at build
  (`pattern::beat_seconds`), moments `offset·beat + k·period`, as Time. Values bar, beat (1-based), count.

### 3.3 If

- **Compare** (`value` name, `test` Over / Under / Between / Equal, `amount`, `to`): the firing's value (missing:
  doesn't pass); Equal within 1e-6 (for whole numbers like hits, exact); both numbers may come from the firing.
- **Is** (`side` That ball / The thing it hit, `test` Matches / Ball number, `who`, `number`): Matches uses the When
  rule of who; Ball number compares the side's ball number (1-based).
- **Chance** (`percent`): `dice(seed, rule, block, step, subject) < percent / 100`.
- **Every Nth** (`n`, `per` Rule / Each ball), **Cooldown** (`seconds`, `per`), **Only N times** (`n`, `per`): counts
  firings reaching the block; per ball keyed by the subject, at most `MOST_REMEMBERED` (4096) kept, the oldest
  forgotten first.
- **Between times** (`from`, `to`): passes while `from ≤ time ≤ to`.

### 3.4 Logic (`logic.rs`)

`mode` (And within / And since last / Then / Or / Not / Any N), `within` (s, default 0.5), `n` (2), `same_ball`.
Memory per key (the subject when Same ball, else one), at most 4096 keys:
- And within: last step per input; fires when every input's last is within `within` of now; then forgets.
- And since last: seen per input; fires when all are seen; then forgets.
- Then: progress index and last step; input 0 starts (again), the next input in order within `within` moves on; the
  last completes; an input out of turn is ignored; a late one starts over at 0 (or sets progress 1 if it is input 0).
- Or: passes every firing on.
- Not: input 0 opens a waiting firing; any other input at the same step or within `within` after closes every open
  one; `tick` passes on those whose time ran out (`now − start > within`); at most 4096 waiting.
- Any N: last step per input; fires when at least `n` inputs are within `within` of now; then forgets.
The firing passed on is the completing one (Not: the first input's) with the other inputs' latest values merged
where the names are free.

### 3.5 Do: Change motion (`motion.rs`)

`who`, `mode` (Add speed / Set speed / Multiply speed / Push / Stop / Aim at / Freeze / Teleport), `amount`, `way` (Up
/ Away from the hit / Toward a thing / Angle), `toward` (Who), `angle`, `to` (The firing's point / A thing's place),
`place` (Who). Balls only (targets without bodies are skipped). Amounts can come from the firing. Places are worked
out at the stage (a thing's place: its first live ball for a ball thing, else its pose, else its anchor).

### 3.6 Do: Change setting (`change.rs`)

`who`, `card` (Own / Look / Motion / Bounce / Shader / Lighting), `setting` (Text key, layered keys allowed),
`how` (Set / Add / Multiply), `to_number`, `to_pair`, `to_colour`, `to_gradient`, `to_switch`, `to_choice`
(the one matching the setting's type is used), `times` (Multiply), `glide`, `ease` (the ease names), `lasts`
(For good / For a time), `seconds`.
- Prepare: for every thing in the scene that has the setting (`Scene::def_at` on the kit registry) keep its def's
  type and hard range and whether a single ball can change alone (`Scene::vary_use == Live` and, for Motion/Bounce,
  the card's sim says `per_ball`). A fixed who naming a thing without it is a problem ("Ball 1 has no Bounciness").
  The Sound and Mixer cards are refused (part 6 brings Play a note; sound settings don't follow rules yet).
- Act: per target, a `ChangeKey` with the row only when the ball can change alone (else the whole thing); the value
  from the setting's type (numbers may come from the firing); Int rounds; Bool/Choice/Colour/Gradient are Set only.

### 3.7 Do: Break (`breaks.rs`)

`who`, `look` (Vanish only in part 1; part 3 adds the rest). Targets as 3.1: a ball row breaks one ball, a whole
thing breaks the thing.

### 3.8 Per-ball looks and sizes

- `Worn::of` (balls draw): rows with Look changes (`world.rules()?.ball_rows(thing, Look)`) wear a stack made from the
  card's params varied at their spread then `apply_ball` at the drawn time (`VariedLook::own_stack`), each its own
  entry in `stacks`; their trails and particles likewise (`own_around`), so `spreads` is filled when any row has
  around-layer changes.
- Balls and Emitter `settings_changed` fold `World::ruled(thing, None, "radius")` over each row's own size and resize
  as they do for keys; a state flag (`ruled_size`) makes the call after the last change ends put sizes back.

## 4. Drawing (`build_frame_sensing`)

When `world.rules()` has whole changes or broken things, the frame takes the copy path: after `animation.apply`,
`apply_whole(.., all keys)` at the drawn time, then broken things are removed from the copy (and from the frame's
light: lights are things). The light probe's `work_out` does the same.

## 5. Tests (what passing looks like)

- core: round trip with rules; format 11 opens with none; format 13 is refused; every edit (ids, links, cycle refused,
  remove closes up, copy/paste fresh ids); problems (missing input, cycle, Logic < 2, unknown kind, deleted thing);
  from-keys defs.
- runtime: `changes.rs` weights and folds (Set/Add/Mul, glide, back, range, NaN); Rule/Break tags 10/11; the stage
  with tiny test block sims (`testing.rs`): order, one event per block per step, budget (`busy_at`), a problem rule
  skipped while others run, actions apply next step, snapshot/seek replay equal, no rules = identical log and
  fingerprint.
- kit (`rules/tests.rs`): each block kind does what section 3 says, with exact steps; Logic every mode with
  hand-made firings; Change setting whole (physics: bounciness changes the next bounce; keyed value underneath with
  Add), per ball (one ball's bounciness / size / look colour, the others unchanged), glide and back, range clamp;
  Break a shape (no more hits on it, not drawn), a ball (Remove follows Break); the frame of a broken shape has no
  path; determinism (two runs equal; seek back equal); sound engine playback has the rule's events.
- goldens: `tests/golden_rules.rs` over `scenes::rule_scenes` (Breakable bricks, Logic modes, Glide and back, Per-ball
  bounce): `golden-rules.txt` and `-debug`, blessed once in debug and once with `--release`. Every existing golden and
  picture test unchanged.
- `cargo clippy --workspace` clean, `cargo fmt`.

## Not doing in part 1

Editor (part 2), Fade/Pop/Shatter (part 3), Health (part 4), the P5 events and actions (Enters/Leaves, Signals,
Split, Spawn, counters: the 4096-ball cap and the 8 signal rounds arrive with them), P6 actions; keyframes on block
settings; rules changing sound.

## As built

- core `bpe-core/src/rules/` (`mod.rs`, `tests.rs`): as section 1. `Value::Who`/`ValueType::Who`, `"Rule"` in
  `SECTION_ORDER`, `Scene.rules` (skipped when empty), `FORMAT` 12, `save::clean_rules` mends ids and links on load.
  The test registry has four test blocks (`testkit::TEST_BLOCKS`).
- runtime `bpe-runtime/src/rules/`: `mod.rs` (`RulesState`, the stage, `dice`, `rule_item`, `Ruled`), `sim.rs`
  (`BlockSim`, `PrepareCx`, `StageCx`), `firing.rs`, `actions.rs` (`MotionChange`, `Action`, `Frozen`,
  `World::apply_rule_actions`, `WHOLE`), `changes.rs`, `tests.rs` (tiny test blocks over `testing.rs`'s dots).
  `World` gains `rules`, `Animated.pristine`, `Slotted.own` (per-ball Motion/Bounce states made in
  `refresh_ball_changes`), `is_physics`, `rules()`, `broken()`, `ruled()` and `card_for(thing, slot, row)` (the state a
  row uses, for tests and later the editor). Every log reader already filtered by kind; the new events change nothing
  for a scene without rules.
- kit `bpe-kit/src/rules/`: `mod.rs` (the `block_kind!` macro gives each struct its `RuleBlockKind` side, `register`,
  `add_sims`, `kit_registry`), `who.rs`, `amount.rs` (from-the-firing numbers), `memory.rs` (`Remembered`, per-ball
  memory capped at 4096, least lately used forgotten), `when.rs`, `checks.rs`, `logic.rs`, `motion.rs`, `change.rs`
  (`setting_infos`), `breaks.rs`, `tests.rs` (19 tests). Times and seconds (Time's At/Every/Until, Beat's Offset,
  Cooldown, Between, Logic's Within) are `f64` settings so 0.65 s lands on step 156, not 155.
- Per-ball size: `BallsState`/`EmitterState.ruled_size`; `own_sizes` takes the `Ruled` Size. Per-ball looks:
  `VariedLook::own_slot`/`own_stack`/`own_around`, `Worn.own_around`; balls changed alike share one stack (keyed by
  their params' text). A whole-thing Look change goes through `build_frame_sensing` (and the light probe's
  `work_out`), which also drops broken things from the frame's copy.
- Scenes: `bpe-kit/src/scenes/rule_scenes.rs` (a child of `scenes.rs`, for its `Builder`): Breakable bricks, Logic
  lanes, Glide and back, Per-ball bounce; `tests/golden_rules.rs`, `tests/golden-rules*.txt`.
- Editor: only `widgets::value_words` learnt `Value::Who` (part 2 brings the Rules panel).
- After the Opus review: per-ball changes are worked out again on the step after their glide ends (so a
  glide that isn't a whole number of steps, or is shorter than one, still gets there: `Change::moving(since,
  time)`); the stage hands blocks the broken things (`StageCx.broken`, the world's own answer is empty while the
  stage runs); a setting keeps at most `MOST_CHANGES` (64) changes (settled Adds or Multiplies merge, else the
  oldest goes) and a broken ball's own changes are forgotten; per-ball lookups are key ranges; change glides and
  lengths are `f64`; a freeze never takes a ball something else holds; Chance is salted by the firing's point;
  Break on the Group breaks each ball and leaves the thing; Compare can read a thing's values (Of: A thing).
- Not done in part 1: a ball's own Motion/Bounce state made for a rule change takes its other varied settings at its
  spread without its light and shadow (only `refresh_rows` reads the probe for Light/Shadow spreads).
- Also not done: rules don't change the sound patch (the Sound and Mixer cards can't be picked); a ball's own
  Look stack is made afresh every frame (shared between balls changed alike), fine for a few hundred balls.
