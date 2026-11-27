# Water mode part 4: More liquids (plan)

Spec: `docs/superpowers/specs/2026-11-24-water-mode-design.md`, "Parts" item 4 and section 3 ("Several liquids").
Hand-over: `ballphysics/water-mode/part4-5-4b-handover.md` in the project files (main 086a7fa). Parts 5 (moving parts)
and 4b (forces and ferrofluid) are built side by side in other threads.

What the checkpoint promised Aedan (page https://claude.ai/artifact/BJkTfdZBfoF1xtW3mg834q, items p4-mix, p4-weight,
p4-thick, p4-pours, p4-drains, loop-flow):

- **Several liquids and colour mixing**: more than one Liquid, each with its own colour and feel; they blend where they
  meet (Mix speed), and Un-mix plays the mixing backwards, as the old Liquid thing does.
- **Liquids that float on each other**: a Weight per Liquid, so oil floats on water and a glass can hold layers (honey,
  water, oil) that settle by weight.
- **Thick liquids**: Thickness up to honey, so a pour folds over itself as it lands, sags off a ledge and stretches
  into strands.
- **Rain, drips and bursts**: rain over an area, a drip from a point, a burst that throws a lump of water at once.
- **Drains and leaks**: a drain, a slow leak, the edge of the course.
- **Drains that feed pours**: water that leaves at a drain comes back at a pour, so a course runs as long as the video.

Heat (freezing, melting), drying and stains, reactions and fizz are Later (checkpoint). Forces are part 4b.

## Four pull requests

Each is its own pull request, merged by this thread once its checks are green and its review is done, in this order:

| | What | Mostly in |
| --- | --- | --- |
| **4.1** | Rain, drips and bursts; drains, leaks and the course's edge; drains that feed pours | `bpe-water drop.rs`, new `lumps.rs`, `drops/{mod,clock,state}.rs`, `finish.wgsl`, `report.wgsl`; kit `places.rs`, `draft.rs`; stage markers |
| **4.2** | Weight (one liquid floats on another), Keeps apart (oil and water), colour mixing (Mix speed, Mixes when) | `sweep.wgsl`, `deep.wgsl`, `finish.wgsl`, a mix pass; the take's mix plane; the look's splats |
| **4.3** | Thick liquids (honey, slime) and the ready-made liquids ("Use a liquid") | `finish.wgsl` (the evening of speeds), the clock; kit `liquids.rs` |
| **4.4** | Un-mix | decided before it is built (below) |

Pours and drains come first: they touch the engine least (births on the host, one test in the finish) and give the
most at once (a course that runs as long as the video). Weight and mixing change the pressure and the take, so they go
second, on a settled base. Thick liquids reuse 4.2's pair rules. Un-mix is last because it rests on everything else.

**Each later sub-part's tasks are written into this file (its own "Tasks" section) before its code is touched**, from
the decisions below; the decisions here are what each must hold to.

## Global constraints

- A Balls scene is untouched: no ball golden, pinned picture or sound changes.
- A scene with none of the new things (no rain, drip, burst, drain or fed pour; one liquid, or liquids of Weight 1 with
  Keeps apart 0 and Mix speed 0; Thickness under today's top) plays the water it plays today, to the bit where that can
  be checked (on and off the same in one build). Where a new number multiplies an old one, it is 1.0 exactly for
  today's water, so the product is the old number to the bit.
- "Same water at every detail": every new setting is in pixels and seconds (a drain's reach in px, its pull a share a
  second, rain's drop size in px, mixing in px² a second), never in cells or steps.
- "Same card, same result": every count is a whole number, every chance a hash of the drop's sorted place and the
  step, never a clock or a random generator; a body made from a restart point plays on to the bit.
- The engine stays within its limits: no pass binds more than 8 storage buffers (`cs_finish_predict` is at 8 already),
  and `Fr` stays one uniform buffer (64 KiB).
- Physics edits restart the draft and mark the final out of date from the right time: every new physics setting goes
  into `WaterSetup::print` and `WaterSetup::inputs` (`what_restarts_the_draft` covers the cards' settings).
- A look-only file stays out of the take's print roots (`build.rs`); physics settings stay out of `look_of`.
- Coordination with 4b (agreed 2026-11-27): 4b appends a forces block at the end of `Fr`/`FrameGpu` and one call in
  `sort.wgsl` `fall()`. Part 4 never touches `fall()`; it appends its drains block at the end of `Fr` (after whichever
  block is there when it merges), adds a third vec4 `c` to `Feel`/`FeelGpu` in 4.2, uses the id buffer's 4th word for
  the colour mix, and raises `TAKE_FORMAT` in 4.2.
- Plain words in the editor; no `ComboBox`; colours only from `style/`; controls named for kittest.
- No demo videos or screenshots unless Aedan asks; tests and the full-size look check where something is drawn.

## What exists (read before building)

- `bpe-water/src/drop.rs`: `Drop` (48 bytes, `kind` bits 0 to 3 its liquid, `AIR` 128, `STOPS` 256), `Pour` (a tap:
  place, aim, width, speed, spread, from, until, kind, colour) with `rows()` (rows across its spout the resting spacing
  apart, a row each time the last has run that far) and `hold()`, `Poured` (owed, born, extra), `fill_stacked`.
- `bpe-water/src/drops/mod.rs`: `Water::set_pours` (at most `MOST_POURS` 64, each judging the room at its spout),
  `births(n)` (each step's births at its end, into the births buffer), `add()` between frames (primes the deep part),
  `settle()` at a boundary (lets go the drops that left), `liquid()`.
- `finish.wgsl` `finish_at`: the step's last word on a drop; an air drop that bursts is sent to (-1e6, -1e6) and is let
  go with the drops that left. `cs_finish` binds 7 storage buffers, `cs_finish_predict` 8.
- `report.wgsl` `Report` (fastest, live, deep, air, liquid, spout[64]), read back two frames late (`clock.rs`), kept
  whole in a restart point (`ClockState`).
- `state.rs` `WaterState` (`BPESTAT1`): everything `Water::copy` carries, the pours with their progress.
- `bpe-kit things/water/draft.rs`: `WaterSetup` (liquids, pours, solids, notes, print), `inputs_at`/`inputs_on`/
  `set_inputs` (taps only; Rain, Drip and Burst get `LATER_KINDS`), `fills`, `start`, `resume`, `drops_about`;
  `draft/inputs.rs` `WaterInputs` (parts with the time each can first change the water).
- `bpe-kit modules/water/places.rs`: `PourCard` (Liquid, Kind, Rate, Width, Speed, Aim, Spread, Fill size, Starts,
  Stops), `DrainCard` (Kind: Drain, Leak, Edge of the course; Radius, Rate, Whirlpool, Edge), `ForceCard` (4b's).
- `bpe-studio water_mode/stage.rs`: markers (`pour_marks`, `lamp_marks`); the Drain tool places a Drain.

## Decisions (my calls; cheap to change before they are built)

### 4.1 Rain, drips and bursts

1. **Lumps, born at their own moment.** Rain, a drip and a burst are each a stream of **lumps**: a disc of drops on the
   resting lattice (half a cell apart, centred on the lump's middle), born at the moment the schedule says. Lump `k` of
   a pour is born at `starts + k × every` while that is before `stops` (`every`: 1 / Drops a second for rain and drips;
   the burst's Every, or once at Starts when Every is 0). The schedule needs no memory: the drops born in any stretch of
   time are worked out from the times alone, so a restart point holds nothing for them.
2. **Where.** A drip and a burst are at the pour's place. Rain's lump `k` is at a place across the pour's Width
   (perpendicular to its Aim), `(h(k, 0) - 0.5) × Width` from the middle, its way turned by `(h(k, 1) - 0.5) × Spread`,
   its size `Drop size × (0.75 + 0.5 h(k, 2))`. `h(k, j)` is a splitmix64 hash of the pour's seed, `k` and `j` to
   [0, 1); the seed is a hash of the pour's thing id (so moving a pour doesn't move its raindrops' pattern, and two
   rains differ).
3. **Speeds.** Rain and a drip move at Speed along their way. A burst's drop at `p` in a lump of radius `r` round `c`
   moves at `Speed × (way × (1 - s) + s × (p - c) / r)`, `s` = Spread / 180: Spread 0 throws the lump whole along Aim,
   180 bursts it outward like a water balloon, between the two it is thrown and comes apart.
4. **Lumps under a drop.** A lump smaller than one drop's room (area under the spacing squared, `a < 1`) is one drop
   when `floor((k + 1) a) > floor(k a)`, so a fine drizzle pours its amount without memory. A bigger lump is the lattice
   points within its radius (its amount within a drop or two of its area).
5. **Born in the engine at the step they fall in.** The setup hands the body the frame's timed drops before the frame
   (`Water::set_timed(&[Timed])`, `Timed { at: f64 seconds, drop: Drop }`); `births()` puts each into the step whose
   time it falls in, moved on from its moment to the step's end by its speed and its liquid's fall
   (`x + v τ + ½ g τ²`, `v + g τ`), as a tap's rows are. The list is used once: a frame with no `set_timed` gives none.
   Not through `add()`: that primes the deep water, which rain on every frame would upset.
6. **Never in a solid.** A lump's drop nearer the solids than a drop rests (`wall_off` cells) is left out (the setup
   checks the field), so rain falling from inside rock or a burst against a wall pours only its open part.
7. **The card.** Pour card: Rate and Fill size as now; **Drops a second** (rain and drips, 0 to 10,000, span 0 to 60,
   default 8, simple); **Drop size** (rain and drips, px, 1 to 400, span 2 to 60, default 12); **Burst size** (px, 1 to
   4000, span 10 to 600, default 160, simple); **Every** (bursts, s, 0 to 600, span 0 to 10, default 0: once); Width
   for taps and rain; Speed and Aim for all but Fill at the start; Spread for taps, rain and bursts; Starts and Stops
   for all but Fill at the start; Rate for taps only. Aim, Spread and Speed are read at each lump's moment (keyframes
   work); Drops a second, Drop size, Burst size and Every are not keyframed (a keyed rate would need the schedule's
   history).
8. **The cap.** While the body holds the most liquid (`DRAFT_MOST`, `FINAL_MOST`) no lumps are born, as no rows are.
9. **What it costs.** `drops_about` counts each lump's area over the spacing squared.

### 4.1 Drains, leaks and the course's edge

10. **A drain takes the liquid that reaches it.** Each step, in `finish_at`, a liquid drop inside a drain's reach leaves
    with chance `p = 1 - exp(-pull × dt)` (worked out on the host per frame, so the same share a second at any step):
    it is sent off the course to a place that says which drain took it and when, and is let go with the drops that
    left. The chance is a hash of the drop's place in the sorted order (the same every run), the frame and the step.
    Air in a drain's reach bursts (it is not counted).
11. **Kinds.** **Drain**: a disc of Radius round its place, pull `DRAIN_PULL` (30 a second) × Rate: a hole, most of what
    is in it is gone within a few frames. **Leak**: the same disc, pull `LEAK_PULL` (1 a second) × Rate: a slow seep.
    **Edge of the course**: a strip along the chosen edge (or every edge) where drops stop (`EDGE` + 0.8 cells from the
    grid's edge), pull `DRAIN_PULL` × Rate: liquid that reaches the edge leaves, as if it ran off the screen. The edges
    stay shut to the engine (the strip takes the drops that stop there), so nothing else about edges changes.
12. **At most 32 drains** (`MOST_DRAINS`, in `Fr` as two vec4s each: middle, reach and chance; kind and edges), the rest
    noted (`TOO_MANY_DRAINS`).
13. **Counted.** The report counts each drain's liquid taken in that frame (`Report.drained[32]`, a whole number each):
    a taken drop's place is `(-2e6 - k, -1e6 - frame)`, and `cs_report` counts the dead drops whose code is this frame's
    (a drop taken earlier is not counted again; air's burst code stays (-1e6, -1e6)). The step's number rides in bits 8
    and up of `St.n.w` (bits 0 and 1 stay the last and first step).
14. **Whirlpool on a Drain** spins liquid round it: that is a force in `fall()`, so 4b builds it with its Whirlpool
    (asked 2026-11-27); 4.1 leaves the setting as it is.

### 4.1 Drains that feed pours

15. **One tank.** Every drain whose **Feeds pours** is on (Drain card, default on) adds what it took to the body's
    **tank** when its report is read (two frames late, at the boundary, so the same every run). A Tap whose **Fed by
    drains** is on (Pour card, taps only) pours only from the tank: each frame it may pour at most a sixth of the tank
    (at least one row while the tank holds one), never more than its Rate × Width; its stream is narrowed to what it
    may pour that frame (its columns scaled, so it stays one stream rather than coming out in dashes), and what it
    poured is taken from the tank. Fed taps take from the tank in the scene's order. The tank is part of the body's
    state (copies and restart points carry it).
16. **Its liquid.** A fed tap pours its own Liquid, whatever went down the drain.
17. **The words.** A fed tap with no drain feeding pours says `NO_FEEDING_DRAIN` on the stage.

### 4.1 The editor

18. Markers: rain shows its width across and its aim; a drip its aim; a burst its disc and aim; a Drain or Leak its
    reach as a circle; an Edge drain a thick line along its edge(s). Rain, Drip and Burst lose `LATER_KINDS`.

### 4.2 Weight, Keeps apart and colour mixing (detailed in its own Tasks before it is built)

19. **Weight by mass.** A drop's mass is its liquid's Weight (clamped to 0.1 to 10; air 1). The density stays a count
    (each drop is the same room), and the pressure's moves are shared out by inverse mass (Macklin and Mueller's
    position based fluids with masses: the multiplier's sum of squared slopes and its softening take each drop's
    inverse mass, and a drop's move is divided by its own), so a heavy drop yields less and sinks, a light one rises,
    and a liquid alone moves the same at any Weight. Deep water: the grid's push on a drop is scaled by the mean mass
    round it over its own (the face's change of speed shared out by mass), so layers settle in deep pools too. Pair
    pulls (evening, closing, skin) are shared by mass so momentum is kept. Weight 1 everywhere multiplies by exactly 1.
20. **Keeps apart** (Mixing card, 0 to 1): between two drops of different liquids the skin's pull and the bend count
    only by `1 - keep` (the larger of the two), so oil keeps to round drops and a sharp layer on water, and colours
    don't run across a kept-apart line.
21. **Colour mixing.** A drop carries a share of one other liquid (the id's 4th word: the other liquid, 4 bits; its
    share, 16 bits; 0 unmixed). Once a frame a mix pass moves each drop's share toward its neighbours' (a diffusion in
    px² a second from Mix speed, enough passes that it is the same at every detail), weighted by `1 - keep`; Mixes when
    Always, Only when stirred (by how fast neighbours move against each other), Only on hits (closing faster than a hit
    speed). A drop keeps its own liquid's Feel (mixing is colour, the dye in it). The take gains a mix plane
    (`TAKE_FORMAT` 3), the look's splats share a drop between its two liquids, so every look setting mixes. Fades and
    Fades to leave the card until drops carry an age (Later).

### 4.3 Thick liquids and ready-made liquids

22. Thickness reaches honey and paste: the evening of speeds is solved implicitly enough to stay calm at any
    Thickness (several evening sweeps a step for thick drops, each a share the step can hold), in px² a second as now,
    so a thick pour folds as it lands, sags off a ledge and stretches into strands. Judged on full-size films on the
    owner's card (honey onto a floor, off a ledge, a strand between two blobs).
23. **Use a liquid** (a picker on a Liquid, like Use a look): Water, Oil, Honey, Syrup, Slime, Milk, Paint, Mercury
    (and 4b's Ferrofluid), each a row of plain Feel and Look values written in one undo step, nothing kept linked
    (`bpe-kit modules/water/liquids.rs`, `ReadyLiquid`).

### 4.4 Un-mix

24. Decided before it is built, between: (a) the old Liquid's way (Un-mixing on, keyframed: the water plays its
    remembered frames backwards and then goes on from there; the draft keeps a ring of recent frames, the final reads its
    own take's frames), or (b) a backwards stretch in the video (the take played backwards, no new physics). (a) is what
    was promised; (b) is far cheaper. The choice is written here with its reasons when 4.4 starts.

## Edge cases

- A lump straddling the course's edge: drops past the edge are left out like those in rock.
- Every = 0 and Starts past Stops: one burst at Starts only if Starts < Stops.
- Drops a second 0, Drop size 0, Burst size 0: nothing is born. A huge rate is capped by `MOST_BIRTHS` a frame.
- A drain whose reach is wholly in rock takes nothing. Two drains overlapping: the first in the list takes a drop.
- A drain at a tap's spout takes the new drops (allowed; the stage shows both markers).
- Fed taps with an empty tank pour nothing; a fed tap's Starts and Stops still hold.
- A Drain's Rate 0 takes nothing (its pull is 0).
- Restart point mid-burst, mid-rain: the schedule is times only, so the remade take is the same.
- A drop taken by a drain on the frame's last step is counted in that frame's report.

## Not to do

- No new storage binding on `cs_finish_predict` (it is at 8): the drains' counts go through the drops' places and the
  report.
- No random generator or clock time anywhere in the physics.
- No change to `fall()` (4b's), to the course's edges' shutting, or to how a tap's rows are born.
- No `add()` for rain or bursts (it primes the deep water).
- No drawing of drains or rain of their own in the look.

## 4.1: rain, drips, bursts, drains and fed pours

### Task 1: `bpe-water`: lumps (`src/lumps.rs`, `drop.rs`)

- New `src/lumps.rs`: `LumpKind { Rain, Drip, Burst }`; `Lumps { kind, place, aim, width, size, speed, spread, every:
  f64, from: f64, until: f64, seed: u64, kind_bits: u32 }`; `Timed { at: f64, drop: Drop }`;
  `Lumps::born(&self, layout, field, clear_px, t0, t1, at: impl Fn(f64) -> (aim, speed, spread)) -> Vec<Timed>`: every
  lump with `t0 <= time < t1` (k from `ceil((t0 - from) / every)`), its disc on the resting lattice (`disc(layout, c,
  r)`, the points `c + s (i, j)` with `|s (i, j)| <= r`, `s` the spacing, plus the stateless single drop of decision
  4), each drop's speed by decision 3, those in a solid (`field.at(p) <= clear_px`) or past the course left out.
  `hash01(seed, k, j)` (splitmix64). `Lumps::amount_between(t0, t1, layout)` (drops, for `drops_about`).
- Unit tests: lump times (Starts, Every, Stops, once), the schedule is the same split any way (`born(0, 1) ==
  born(0, 0.4) + born(0.4, 1)`), rain's places stay within its width and differ by seed, a disc's count is within 2 % of
  its area over `s²` for `r >= 4 s`, a drizzle pours its amount over 1000 lumps (within one drop), a burst's speeds
  (Spread 0 all along Aim; 180 outward, the middle still), drops in a solid are left out.

### Task 2: `bpe-water`: timed births (`drops/mod.rs`)

- `Water.timed: Vec<Timed>`; `set_timed(&[Timed])` (kept to `MOST_BIRTHS`); `births(n)` takes the list: each timed
  drop into the step `j` with `from_j <= at < from_j + dt` (one before the frame's start into step 0, one at or past the
  frame's end into the last), moved to the step's end by decision 5 (gravity: the course's, times its liquid's
  `Feel::gravity`, in grid units), given the body's next serial, counted in `poured` and in the clock's `added` speed.
- Test (`tests/pours.rs`, on the card): a timed drop is born at its step and falls as a lone drop born by `add()` at the
  same moment would (its place after 10 frames within 0.1 cell); a frame with no `set_timed` after one with births
  births none; timed births repeat (twice, and from a copy made with births on their way).

### Task 3: `bpe-water`: drains (`drops/mod.rs`, `clock.rs`, `common.wgsl`, `finish.wgsl`, `report.wgsl`)

- `pub struct Drain { place: Vec2 (world px), reach: f32 (px), pull: f32 (a second, may be infinite), edges: u8 (0 a
  disc; bits 1 left, 2 right, 4 top, 8 bottom), feeds: bool }`, `MOST_DRAINS` 32; `Water::set_drains(&[Drain])` (from
  the next frame), `drains()`.
- `Fr` gains, at its end: `drains: array<vec4<f32>, 64>` (per drain: middle x, y (grid), reach (cells), chance a step;
  kind (0 disc, 1 edges), edges bits, 0, 0) and `dmeta: vec4<u32>` (drains, frame, 0, 0). `FrameGpu` matches; the
  chance is `1 - exp(-pull × dt)` (1 when infinite, 0 when not finite or 0).
- `St.n.w`: bits 8 and up the step's number in its frame (`births()` writes them; every reader masks bits 0 and 1).
- `finish_at`: after the burst test, for a live liquid drop (`!air`), the first drain whose reach holds it (disc: within
  the reach of its middle; edges: within `EDGE + 0.8` cells of a named grid edge) takes it when
  `hash(i, frame, step) < chance`: `nf[i] = 0`, returns `(-2e6 - k, -1e6 - frame)`. An air drop in a drain's reach
  bursts as at the top.
- `Report.drained: array<atomic<u32>, 32>` after `spout`; `cs_report` adds 1 to `drained[k]` for each dead drop whose
  place is drain `k`'s code for this frame. `clock.rs` `Report` matches; `settle()` keeps the report's `drained` for the
  body (`Clock::drained`).
- Tests (`tests/pours.rs`): a drain under a pool empties it (the liquid in the course under 2 % of the start in 3 s at
  12 px and 6 px, never coming back); a leak takes a share a second (a pool keeps between 30 and 80 % after 1 s at
  Rate 1, the same within 10 points at 12 and 6 px); the report counts what each drain took (the sum of `drained` over
  the frames equals the liquid lost, two drains told apart); an edge drain at the bottom takes a tap's water that reaches
  it (no pool forms on the floor); a drain with Rate 0 takes nothing (to the bit the same as no drain); air in a drain
  bursts and is not counted; repeats (twice, from a copy).

### Task 4: `bpe-water`: the tank and fed taps (`drop.rs`, `drops/{mod,state}.rs`)

- `Pour.fed: bool` (default false). `Water.tank: u64`: at each boundary's settle, the drained counts of the drains with
  `feeds` on are added (`settle` returns them; the body keeps the drains as they were when that report's frame ran, so a
  drain list changed since doesn't move the count to another drain). In `births()`, before the steps: each fed pour's
  allowance for the frame (decision 15) and its columns' share `f = allowance / full`, applied as a width
  `width × f` for this frame's rows only (`Pour::rows` with a narrowed copy); after the frame's births, the tank less
  what the fed pours poured. A fed pour is never held at its spout for more than its allowance.
- `WaterState` (`BPESTAT2`): each pour's `fed`, the tank, the drains (`Drain`), the clock's reports with `drained`.
- Tests: a fed tap with no feeding drain pours nothing; a loop (a fill over a drain that feeds a tap above it) keeps its
  liquid within 3 % of the start over 10 s at 12 px and its tap running; the tank survives a restart point (a body from
  `state()` plays on to the bit with a fed tap); a fed tap narrower than full when the drains take little (its drops a
  frame within a row of the allowance).

### Task 5: `bpe-kit`: the cards and the setup (`modules/water/places.rs`, `things/water/draft.rs`, `draft/inputs.rs`)

- `PourCard`: the new settings and `when`s of decision 7, `fed_by_drains`. `DrainCard`: `feeds_pours` (default on).
- `WaterSetup`: `drains: Vec<DrainSetup>` (enabled Drains in order, at most `MOST_DRAINS`, else `TOO_MANY_DRAINS`),
  rain, drips and bursts as `PourSetup`s with their liquid (`LATER_KINDS` gone); `FrameInputs.timed` (the frame's lumps
  from every Rain, Drip and Burst, `inputs_on` with the field, the seed a hash of the pour's id) and `FrameInputs.drains`
  (Edge of the course's strip; Drain and Leak pulls by decision 11; Rate × pull); `set_inputs` sets both (no timed
  drops while full); fed taps carry `fed`; `NO_FEEDING_DRAIN`; `print` hashes the drains (id, place, card); `inputs()`
  gains `drains` (from 0) and `lumps:k` per rain, drip and burst (from its Starts less `TAP_LEAD`, as a tap);
  `drops_about` adds the lumps.
- Tests (`draft/tests.rs`): rain, drip and burst pour (a frame's timed drops at their moments, none before Starts, none
  in the rock, Burst once and Every); the drains' inputs (kinds, Rate, reach, pull, edges); a fed tap and its note;
  every new setting restarts the draft (`what_restarts_the_draft` grows by itself; check it covers Drain); a rain's
  Starts moves the earliest change to there; `drops_about` counts rain.

### Task 6: `bpe-studio`: markers and notes (`water_mode/stage.rs`, `tests.rs`)

- Decision 18's markers. The pour-note test now checks a Rain pours (no note) and a fed tap with no drain says
  `NO_FEEDING_DRAIN`.

### Task 7: check, notes, review, merge

- Focused tests: `cargo test -p bpe-water --test pours -- --test-threads=2`, the drop and lumps unit tests,
  `cargo test -p bpe-kit --lib things::water`, `cargo test -p bpe-studio water_mode`, clippy and fmt. CI covers the rest
  (the card job runs the drawing and engine tests on the owner's card).
- `docs/notes/water.md` "Part 4.1 (as built)"; this file's "As built: 4.1".
- One review (a reviewer agent on the diff), fix, merge when green, then 4.2's tasks.

## As built

### 4.1

Built as planned, with these changes (the detail is in `docs/notes/water.md`, "Part 4.1"):

- Decision 4: lumps of any size are the lattice points within their radius, **the lattice offset by the lump's own
  hashed share of a spacing**, so a lump holds its area over the spacing squared on average at any size (a drizzle
  pours its amount at every detail); the floor rule for small lumps was not needed. `Lumps` has no Aim, Speed or
  Spread of its own: `born` takes a `throw(moment)` that reads them, and the stream carries a colour.
- Decision 10: the drain test is at the start of `finish_at` (before the neighbour loop, so a taken drop costs
  nothing); air in a drain's reach bursts by the drain's chance, not at once, so a slow Leak doesn't pop every
  bubble. Overlapping drains (the review): one share is rolled for the drop and the first drain in reach whose
  chance is above it takes it, so the strongest decides whatever the order and a Rate 0 drain never shadows
  another (the edge case "the first in the list takes a drop" now holds only for which drain is credited).
- The review also found the editor's draft set its inputs by hand and so left rain and drains out: it now plays
  each frame through `WaterSetup::set_inputs`, as the final does, and `start()` sets frame 0's timed drops and
  drains.
- Decision 13: the counting is its own pass, `cs_drained`, after the frame's last step and before trapped air is
  born: air is born over the head of the drops that left and would hide this frame's taken drops from
  `cs_report`. The frame code wraps at 2^23 frames.
- Decision 15: the clock keeps, with each frame's entry, which drains fed the tank then (a bit mask), so changing
  the drains doesn't move a count to another drain. Fed taps' allowances also cap their rows, so a fed tap never
  pours a drop the tank doesn't hold, and liquid is kept to the drop round a loop.
- Restart points are `BPESTAT2` (timed drops, drains, tank, fed, the reports' drained counts).
- The "a fed tap narrower than full" test was folded into the loop test (the loop keeps every drop, which needs the
  allowance to hold).
