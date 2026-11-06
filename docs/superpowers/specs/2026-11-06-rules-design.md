# Rules: design

Date: 2026-11-06. Brainstorm thread "Design the Rules event system". Mockups: the "Rules Editor Options" page
(https://claude.ai/artifact/4EzofzUTkkGHJrWi1gk5HA; file `ballphysics/rules/rules-options.html` in the project files).
Every decision below was made or approved by the owner in that thread.

## 1. What this is

Rules are an optional, independent section of a scene that controls what happens in the simulation. A rule listens
for something that happens (a hit, a time, a beat), checks whatever it needs to, then changes the simulation (breaks a
shape, adds speed to a ball, changes any setting). A new project has **no rules**, and a scene without rules simulates
and draws exactly as it does today (goldens and pictures byte-identical).

This is not Version 1's system. Version 1 gave every card a "Nodes depth" (a node graph per module, `bpe-graph` and
`bpe-vm`, removed on 2026-11-03). Rules here are one section for the whole scene, never inside a card, and nothing is
compiled from a free graph.

The owner's example, used throughout:

> When Ball 1 hits any shape tagged "breakable" → break that shape → Ball 1 speed +100.

### What the owner said, and what is assumed

Said: an independent section; no rules by default; nodes that receive events and nodes that act when criteria are met;
chain lanes (option D); several When cards joining into a Logic card; every Logic mode in one card; the Do-card rules
in section 5; Rules swap with the timeline; the Break looks; the timeline Rules lane; the safety limits; a Health card.
The owner wants as much customisation as possible with few limits on how far settings go.

Assumed (stated so they can be corrected at review): rules never change the saved scene (they change the running
simulation only); the full catalogue in section 6 is built over several parts, part 1 having the core blocks; a rule's
actions apply at the start of the next step (1/240 s later).

## 2. The look: chain lanes

The Rules section is a list of **lanes**, one per rule. A lane reads left to right, like an effect chain on a mixer
track:

```
[ Breakable bricks ]  [WHEN Hit: Ball 1 hits tag breakable] → [IF hit strength > 200] → [DO Break: the shape it hit, Shatter] → [DO Change: Ball 1 Speed +100] [+]
   on · fired 14×                                                                       ↘ [DO Camera shake 0.2 s] [+]
```

- A lane's **header column** (150 px) shows an on switch, the rule's name (rename in place) and "fired N times" up to
  the playhead. Right-click: Rename, Duplicate, Copy rule, Paste rule, Delete. Drag the header to reorder rules (order
  matters only for what is applied first in the same step, section 4.4).
- **Blocks** (cards) sit in the lane. There are four sections of block, each with a small label on top: **WHEN**
  (events, a 2 px `text` top edge), **IF** (checks), **LOGIC** (joins, a 2 px `muted` top edge) and **DO** (actions).
- A block's face shows its kind name and two or three of its settings as small fields ("Ball 1", "hits breakable").
  Clicking a block selects it and its full settings show in the **inspector**, like a layer's do, with the normal
  setting rows, sliders and the right-click value menu.
- **Branching**: any block can lead to more than one next block. A second next block sits on a row below (the "also"
  row in the mockup). The `+` after a block opens the block menu (WHEN / IF / LOGIC / DO submenus, then the kinds).
- **Joining**: only a **Logic** block takes more than one input. Several When blocks (or branches) stack in a column
  and lines join them into the Logic block, which then carries on along the lane. A lane may hold several When blocks;
  "+ When" at the lane's start adds one stacked under the others.
- **Linking by hand**: each block has a small out-dot on its right edge. Dragging from it onto another block links
  them (onto a non-Logic block that already has an input, the link replaces it; onto a Logic block, it adds an input).
  Right-click a link line: Remove link. Right-click a block: Duplicate, Copy block, Paste block after, Delete (its
  links close up: its input goes on to its outputs where that makes a valid chain).
- **Layout** is worked out, never placed by hand: a block's column is the longest path to it from a When block; rows
  follow the order the blocks were added within their column. Lines are drawn between columns. A long lane scrolls
  sideways inside the panel.
- **Firing**: during playback and scrubbing, a block that fired within the last 0.15 s of the playhead gets a 1.5 px
  `text-strong` ring. Nothing else is coloured; the only hues in the panel are things' track colours on the "who"
  fields (a dot or diamond in the thing's colour), as everywhere else in Ball Studio.
- A rule with a **problem** (a block points at a thing that was deleted, a setting that no longer exists, a Logic
  block with fewer than two inputs, a block with no input that isn't a When) shows a red mark on its header; hovering
  says what is wrong. That rule is skipped; every other rule keeps running.

### 2.1 Where it sits

The editor's bottom area switches between **Timeline** and **Rules** (two chips at the left of the bottom panel's
header row). The Rules view gets the full width under the preview, so blocks can be watched firing while the preview
plays. The transport stays where it is. The choice is remembered in the editor's own settings (not the scene).

### 2.2 Timeline

The timeline gets a **Rules lane** right under the Hits lane, shown only when the scene has rules. Each firing of a
rule (its last block's actions running) draws a tick in `text`. Hovering a tick names the rule and the moment.

## 3. Data model

### 3.1 Saved

`Scene.rules: Rules` (serde: skipped when empty), beside `things`, `tracks` and `pulses`. Rules are not a thing and
not a card. Scene files become **format 12** with no upgrade (an older file simply has no rules).

```rust
pub struct Rules { pub list: Vec<Rule>, pub next_rule: u32 }
pub struct Rule {
    pub id: RuleId,             // stable, never reused in a scene (like layer ids)
    pub name: String,
    pub enabled: bool,
    pub blocks: Vec<Block>,     // in the order added (layout reads it)
    pub next_block: u32,
}
pub struct Block {
    pub id: BlockId,            // unique within its rule
    pub kind: KindId,           // "when.hit", "if.compare", "logic", "do.change", ...
    pub inputs: Vec<BlockId>,   // empty for When; exactly one for If/Do; two or more for Logic, in stacked order
    pub params: Params,         // the block's settings, from its kind's #[derive(Settings)] schema
}
```

- Block kinds are plugins, registered in the registry like thing and card kinds (`RuleBlockKind`: id, label,
  section When/If/Logic/Do, settings schema, `dynamic_defs` for settings that depend on others, and a `summary` for the
  block's face). They live in `bpe-kit/src/rules/` (one file per kind or a few small kinds a file).
- A rule is valid when its blocks form a graph with no cycles, every non-When block has its inputs, and every target
  and setting it names exists. Edits (`bpe-core/src/rules.rs`): `add_rule`, `remove_rule`, `move_rule`,
  `duplicate_rule`, `rename_rule`, `set_rule_enabled`, `add_block(after)`, `add_when`, `remove_block`, `link`,
  `unlink`, `copy_rule`/`paste_rule`, `copy_block`/`paste_block`. Every one is an undo step.
- Keyframes may go on block settings (`Address` gains a rule-block form: `{ rule, block, key }` beside the thing
  form), so a rule's amount can change over the clip. Pulses may not go on block settings.

### 3.2 Who a block points at: `Who`

Every block that points at things has a **who** picker (`Who`, saved in params as a value):

| Who | Means |
| --- | --- |
| That ball | the ball the firing is about (its subject) |
| The thing it hit | the other side of the firing (a hit's other party: a shape, or the other ball) |
| That ball's group | every ball of the subject's Balls or Emitter thing |
| A named thing | one thing by id (Ball 1, Bricks); for a group, optionally one ball number |
| Anything tagged … | every thing with the tag |
| Any … | every thing of a kind (any shape, any ball) |
| The scene / camera / a light | the scene settings, the Camera thing, a Light |

"That ball" changes only that one ball, even inside a group (section 5.1). A named thing that is later deleted makes
the rule show a problem (section 2).

### 3.3 Values a firing carries: `Firing`

When a When block fires it makes a **firing**: who it is about and what it knows.

```rust
pub struct Firing {
    pub step: u64,
    pub subject: Option<Party>,   // "that ball" (or that thing)
    pub other: Option<Party>,     // "the thing it hit"
    pub point: Vec2, pub normal: Vec2,
    pub values: Values,           // named numbers: strength, speed, size, hits so far, ball number, health, count…
}
```

A firing flows along the lane. An If block lets it through or stops it. A Logic block collects firings (section 4.3).
A Do block acts on it. Any number setting on an If or Do block can be a fixed amount or **from the firing**: a value
it carries × a factor + an offset ("the hit's strength × 0.5"). The block's number field shows a small "from" switch;
on, the field picks the value and shows the factor and offset under it.

## 4. How rules run

### 4.1 The Rules stage

`World` gains a `rules: Option<RulesState>` built from `Scene.rules` at `World::build` (`None` when the scene has no
enabled rule, so nothing changes for a scene without rules). Each step:

1. **Start of the step** (before `animate` and Spawn): apply the actions queued at the end of the previous step, in
   the order queued (section 4.4).
2. The normal stages run (Spawn, Scripted, substeps, Events).
3. **Rules stage** (after Events and the history record): read this step's new log events, plus time and beat
   moments that fall in this step; make firings for every When block that matches; run each through its lane; queue
   the actions the Do blocks produce. Health damage from this step's hits is worked out first (part 4), so "Health
   drops" fires in the same stage.

Nothing reads the future: a rule sees this step's events and the world as it is after this step. Because actions
apply at the next step's start, everything a rule does is part of the simulation, cloned in snapshots and identical
on every replay. Scrubbing back, exporting and the sound engine's own playback all give the same result.

### 4.2 Order

Rules run in list order; within a rule, When blocks in their stacked order; events in log order; firings fan out
through the lane depth first in block order. Every collection is a `BTreeMap`/`Vec` in id order. Anything random
(Chance, a Random value) hashes `World.seed` with (rule, block, step, subject), never a shared random stream, so
adding a rule never changes another rule's dice.

### 4.3 The Logic block

One block kind with a **Mode** and a **Same ball** switch. Two events almost never fall on the same step, so And
needs a time:

| Mode | Fires when | Settings |
| --- | --- | --- |
| **And, within a time** (default) | every input fired within *Within* of each other | Within (default 0.5 s) |
| **And, since last time** | each input has fired at least once since this block last fired (collect them all) | — |
| **Then** | the inputs fired in their stacked order, each within *Within* of the one before | Within |
| **Or** | any input fires | — |
| **Not** | the first input fired and none of the others fired within *Within* after it (fires when the time runs out) | Within |
| **Any N** | at least N different inputs fired within *Within* | N, Within |

- **Same ball**: on, inputs only count together when their firings have the same subject; the block keeps its memory
  per subject (bounded: at most 4096 subjects remembered, oldest forgotten first). Off, the block has one memory.
- The firing a Logic block passes on is the one that completed it (for Not, the first input's), with the values of
  the other inputs' latest firings added under their block names when they don't clash.
- The memory (which inputs fired when) is part of `RulesState`, so it replays exactly.
- The same block joins If branches too: two If blocks after one When, both into "And, within a time", is "speed over
  300 and size under 20" (both pass on the same step).

### 4.4 Actions

A Do block produces **actions** (`Action` in `bpe-runtime/src/rules/actions.rs`), queued and applied at the start of
the next step in order. Applying an action never runs rules (rules only see what the stage saw). A target that has
gone (a removed ball, a broken shape) is skipped silently.

### 4.5 Safety limits

- At most **4096 balls** a thing (`MAX_BALLS`); a split or spawn past it is dropped.
- At most **4096 actions per rule per step**; past it the rule stops for that step and shows "too busy at 3.2 s" on
  its header (not a problem mark; it still runs next step).
- Signals (part 5) that make firings in the same stage go round at most **8 rounds**; later rounds wait for the next
  step.
- Every number an action writes is checked finite; a NaN or infinity is dropped. Speeds still clamp to the world's
  ceiling.
- A rule with a problem (section 2) is skipped whole.

### 4.6 What a rule changes, as the world keeps it

`RulesState` holds:

- `changes: BTreeMap<ChangeKey, Change>`: every running setting change (whole thing, or one ball), each with how it
  combines (Set/Add/Multiply), its value, when it started, its glide and how long it lasts. A change's value at any
  moment is worked out from these, so drawing between two steps is exact (section 5.3).
- the Logic blocks' memories, Every Nth / Only N times / Cooldown counters, counters and health (parts 4 and 5).
- the broken things and their break records.

The fingerprint (`fingerprint_of`) hashes `Scene.rules`; a scene with no enabled rules hashes as today.

### 4.7 Things that run on copies of the world

- `World::lone` (emitter test flights for aiming) copies the world **without** rules: aiming finds ways as if no rule
  acted. An aimed round can therefore miss when a rule changes the physics during its flight; the emitter's aim check
  says so when the scene has rules that change physics settings or motion ("Rules may change these balls' paths").
- The sound engine runs its own playback of the physics; it builds the same `RulesState`, so balls split by a rule
  make sounds and rules replay the same there.
- Follow sound's `ControlSampler` reads hits from the log as before; balls made by rules hit and are heard.

## 5. Do blocks: how they change things (approved)

### 5.1 Who it changes

The Do block's who picker (section 3.2). "That ball" changes only that ball. That is possible for the settings that
may **vary across balls** (`ThingKind::vary`, `ModuleKind::vary`, `LayerKind::vary`: Size, Motion and Bounce
settings, ball look layer settings); a per-ball change reuses the vary machinery (each ball's own value, per-ball
Motion/Bounce states `ModuleSim::per_ball`/`Slotted.rows`, `BallsState` own sizes, the draw's per-ball stacks). For
a setting that can't vary per ball, the setting picker marks it "whole group" and a change aimed at that ball changes
its whole group.

### 5.2 What it can change

Two kinds of Do block:

- **Change motion** (`do.motion`): the ball's motion right now. Modes: Add speed (along its direction), Set speed,
  Multiply speed, Push (a velocity added in a direction: Up, Away from the hit, Toward a thing, an angle), Stop,
  Aim at (a thing or a point, keeping its speed), Freeze for N s (held, like `Bodies.held`), Teleport to (a thing's
  place, a point). Acts on balls only.
- **Change setting** (`do.change`): **any setting the inspector shows**, on any card or thing: Bounciness, Size, Glow
  colour, Spin, the scene's Gravity, a light's Strength, a block setting of another rule. Picked through a setting
  menu (thing → card → layer → setting, the same structure as Follow sound's menu). Numbers, colours (Set and a glide
  between colours), switches and choices (Set only) all work.

### 5.3 How, and how long

- **How**: Set, Add or Multiply (numbers); Set for everything else. An optional **Glide** (seconds, any curve from the
  ease list) moves from where the setting is to the new value.
- **How long**: For good, or **For N seconds then back** (gliding back over the same Glide).
- **On top of keyframes**: a change sits on top of whatever the keyframes and pulses give the setting at each moment.
  Add and Multiply work from the keyframed value; Set wins until it runs out. Changes on one setting stack in the
  order they were applied (a later Set replaces earlier changes on that setting for that ball or thing).
- Ranges: a change may push a setting past its slider span (the owner wants few limits), but never past its hard
  `range`.
- **Physics settings** reach the simulation through `animate` (the world keeps an `animated` copy when rules can
  change physics settings, even with no keyframes; a changed card is prepared again). **Look settings** reach the
  picture through the draw: `build_frame_sensing` applies the world's changes to its resolved copy at the drawn
  moment, after keyframes; per-ball look changes go through the per-ball draw path.

### 5.4 Break (approved)

`do.break` removes a shape, a ball, a group or any thing (a light goes dark, a camera stops following), with a
**Look**:

| Look | What it does | Settings |
| --- | --- | --- |
| Vanish | gone at once | — |
| Fade out | fades to nothing | Time |
| Pop | swells, then fades | Time, Swell |
| Shatter | cut into pieces that fly apart from the hit, fall with the scene's gravity, bounce off other shapes, spin and fade | Pieces, Spread, Spin, Fade after, Bounce |

- A broken thing stops simulating at once (a broken shape's wall is gone, a broken ball is removed with a `Remove`
  event) and only its look plays out.
- Shatter pieces wear the shape's (or ball's) own look, cut into polygon shards (a seeded Voronoi of its outline).
  They are **drawn only**, worked out from the Break event at the moment drawn (closed form, or 1/120 s steps for the
  bounces, like particles), so they never push balls and can't change the physics.
- The log gains `EventKind::Break` (tag 11: `a` the broken thing or ball, `point`/`normal` the hit, `speed` its
  strength); a removed ball's `Remove` follows. Scrubbing back before the break shows the thing again.
- Several hits before breaking need no health setting (an If "hits so far is 3", or Every Nth); the Health card
  (section 7) does it properly.

## 6. The blocks

Parts are in section 9. "P1" means part 1.

### When (events)

| Block | Fires | Firing's values | Part |
| --- | --- | --- | --- |
| Hit | Who hits Whom (each a who: a ball, a group, a tag, any shape…) | strength, speed, ball number, hits so far | P1 |
| Time | At a time, or Every N s from a time, optionally until a time | time, count | P1 |
| Beat | Every beat / bar / N bars (the scene's tempo), with an offset | bar, beat | P1 |
| Enters / Leaves | a ball comes into or leaves a shape (or an emitter's catcher) | speed | P5 |
| Fired | an emitter fires a ball or a round | round, speed | P5 |
| Removed / Broken | a thing or ball is removed or broken | — | P5 |
| Count reached | a thing's hits so far, its ball count, or a counter reaches N | count | P5 |
| Crosses a level | a ball's speed or size, a sound level, a light level goes over or under a level | value | P5 |
| Signal | another rule sent this signal | the sender's values | P5 |
| Health drops / reaches 0 | a thing's or ball's health falls / reaches 0 | health, damage | P4 |

### If (checks)

| Block | Passes when | Part |
| --- | --- | --- |
| Compare | a value of the firing or of a thing (speed, size, hits so far, ball number, strength, time) is over / under / between / equal | P1 |
| Is | the subject (or other) has a tag, is a kind, is a named thing, is ball number N | P1 |
| Chance | N% of the time (seeded) | P1 |
| Every Nth time | the Nth, 2Nth… firing (per rule, or per subject) | P1 |
| Cooldown | not more often than every N s (per rule, or per subject) | P1 |
| Only N times | the first N firings (per rule, or per subject) | P1 |
| Between times | only from one time to another | P1 |
| Counter is | a counter compares to a number | P5 |
| Health is | health is below / above | P4 |

### Logic

One block, section 4.3. P1.

### Do (actions)

| Block | Does | Part |
| --- | --- | --- |
| Change motion | section 5.2 | P1 |
| Change setting | section 5.2, 5.3 | P1 |
| Break (Vanish) | section 5.4 | P1 |
| Break looks Fade out, Pop, Shatter | section 5.4 | P3 |
| Change health | Set or Add (heal or damage) | P4 |
| Split | the ball into N balls (sizes shared by area, spread in a fan, keeping its look and cards) | P5 |
| Spawn balls | N balls into a Balls thing at the firing's point or a thing's place, with a launch | P5 |
| Fire a round | an emitter fires a round now (its Fire when gains "By a rule") | P5 |
| Tag | add or remove a tag on who | P5 |
| Count | add to, set or reset a named counter (a scene-wide number, shown in the Rules panel) | P5 |
| Send signal | sends a named signal (When Signal) with this firing's values | P5 |
| Camera | shake (strength, time) or zoom punch (amount, time) | P6 |
| Slow motion | the simulation runs at N× for N s (gliding in and out): the world's step moves balls and walls by dt × N while the video clock and keyframes run on | P6 |
| Flash | fires who's Flash on hit and Colour on hit layers as if hit | P6 |
| Lighting scene | shows a lighting scene (the Lighting card's scenes) with its crossfade | P6 |
| Play a note | a shape's voice plays a note (pitch, velocity), heard in export | P6 |

## 7. Health (approved)

- A **Health card** (`SlotId::Health`, kind `health`, `bpe-kit/src/modules/health/`) that can be added from "+ Add
  card" on a ball, a Balls group, an emitter (its balls) or a shape. A thing has no health unless it has the card.
- Settings: **Starting health** (default 3; can vary across balls), **Hits take** (Each hit: an amount, default 1 /
  By strength: strength × factor / Nothing: only rules change it), **Only hits from** (a who: anything, a tag, a
  thing), **At zero** (Break, with the Break looks and their settings / Nothing: rules decide).
- Each ball in a group keeps its own health (in `RulesState`, by thing and ball number); damage is worked out in the
  Rules stage from the step's hits before rules run. A scene with Health cards but no rules still gets a
  `RulesState` (health needs it).
- Rules: When "Health drops" / "Health reaches 0", If "Health is", Do "Change health".
- **Follow health**: any keyframable setting can follow health, the way settings follow sound
  (`PulseSource::Health { thing }`, reading health ÷ starting health, 0 to 1, as at the end of the previous step for
  physics settings). On a group, **Vary across balls** gains **Health** (`VaryBy::Health`, each ball's own share) so
  each brick or ball can dim or redden as it is hit. The settings row menu gets "Follow health ▸".

## 8. Testing

- No rules: goldens (`bpe-kit/tests/golden*.txt`), unlit/lit picture hashes and sound goldens unchanged.
- New goldens `golden-rules*.txt` from test-only scenes (`scenes::rule_scenes`, behind `test-scenes`): breakable
  bricks, Logic modes (each mode pinned with exact firing steps), a split chain hitting the ball cap, health bricks.
  Blessed once in debug and once in `--release`.
- Unit tests per block kind (fires / passes / does what the table says), Logic modes with hand-made firings,
  determinism (two runs equal, a seek back and replay equal: snapshot holds `RulesState`), safety limits (ball cap,
  action budget, signal rounds, NaN dropped), problems (deleted target, missing setting, cycle) skip only that rule.
- Save/load round trip of rules (format 12); a format 11 file opens with no rules.
- Editor (kittest): Timeline/Rules switch, add rule, add blocks from the menu, link into a Logic block, select a block
  and edit it in the inspector, the problem mark, the Rules timeline lane.
- Shatter pieces and the other Break looks: picture tests at fixed moments (part 3).

## 9. Parts and order

Each part gets its own thread, plan (`docs/superpowers/plans/rules/partN.md`), one Opus review at the end, and a PR.

1. **Part 1, engine core** (first, alone): `Scene.rules` and edits (format 12), block-kind registry, `RulesState`
   and the Rules stage, firings, `Who`, values "from the firing", Logic (every mode), action queue, safety limits,
   Rule and Break log events, fingerprint, snapshots, sound playback; blocks When Hit/Time/Beat, every P1 If, Do
   Change motion, Change setting (whole thing and per ball, physics and look, keyframes underneath), Break (Vanish);
   tests and rule goldens. Headless: no editor change except loading and saving.
2. **Part 2, editor**: the Timeline/Rules switch, lanes, blocks, the block menu, linking and the worked-out layout,
   the inspector for a block (who picker, setting menu, "from" values), copy/paste, firing rings, the problem mark,
   the timeline Rules lane, kittest tests.
3. **Part 3, break looks**: Fade out, Pop, Shatter (draw-only pieces).
4. **Part 4, health**: the Health card, damage, its blocks, Follow health, Vary by Health.
5. **Part 5, more events and actions**: Enters/Leaves, Fired, Removed/Broken, Count reached, Crosses a level,
   Signals, Counters, Split, Spawn balls, Fire a round, Tag.
6. **Part 6, spectacle actions**: Camera, Slow motion, Flash, Lighting scene, Play a note.

Part 1 merges first. Parts 2 to 6 then run in parallel: each adds block kinds through the registry and its own
folders, and touches shared files (`world.rs`, `inspector.rs`, `app.rs`, `kit/lib.rs`, the block menu) only
additively. Merge order after part 1: 2, then 3 to 6 in any order (each rebases on main before its PR). Parts 3 to 6
need no part 2 code to build their engine side; their editor side (a new block's face summary, any special field)
lands after part 2.

## 10. Not doing

- No free node canvas and no compiled graph VM (option A was turned down for this).
- Rules never edit the saved scene; there is no "bake rule into keyframes".
- No rules inside cards or per thing; no built-in or example rules or scenes in the program.
- No hardware-specific or GPU-side rule evaluation: rules run on the CPU in the simulation.
- No backward compatibility work: old files just have no rules.
