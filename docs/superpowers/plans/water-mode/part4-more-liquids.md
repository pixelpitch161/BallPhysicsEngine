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

## Five pull requests

Each is its own pull request, merged by this thread once its checks are green and its review is done, in this order:

| | What | Mostly in |
| --- | --- | --- |
| **4.1** | Rain, drips and bursts; drains, leaks and the course's edge; drains that feed pours | `bpe-water drop.rs`, new `lumps.rs`, `drops/{mod,clock,state}.rs`, `finish.wgsl`, `report.wgsl`; kit `places.rs`, `draft.rs`; stage markers |
| **4.2** | Weight (one liquid floats on another), Keeps apart (oil and water), colour mixing (Mix speed, Mixes when) | `sweep.wgsl`, `deep.wgsl`, `finish.wgsl`, a mix pass; the take's mix plane; the look's splats |
| **4.3a** | The ready-made liquids ("Use a liquid") | kit `modules/water/liquids.rs`; the inspector's Liquid |
| **4.3b** | Thick liquids (honey, slime, paste) | `finish.wgsl` and a new thick pass (the evening of speeds), `numbers.rs` |
| **4.4** | Un-mix | decided before it is built (below) |

Since 2026-11-27 23:59Z (Aedan: no merge and no full check run until a part's work is done), 4.3a, 4.3b and 4.4
are built on the branch one after another and go to main as one pull request, with one review.

Pours and drains come first: they touch the engine least (births on the host, one test in the finish) and give the
most at once (a course that runs as long as the video). Weight and mixing change the pressure and the take, so they go
second, on a settled base. 4.3 is split in two (found when it started): the ready-made liquids touch only the kit and the editor and 4b's
Ferrofluid row waits on their file, so they go first; thick liquids change the engine's step and are judged on films.
Thick liquids reuse 4.2's pair rules. Un-mix is last because it rests on everything else.

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

    **Chosen (2026-11-28): (b), a backwards stretch.** Aedan picked it on a decision card (00:48Z, recommended
    there). Why: the un-mixing looks the same either way (it is the take's own frames played backwards);
    (b) needs no new physics, no ring of frames and no restoring a body from a frame, so the take, its restart points,
    finding an older take and Make final all stay as they are; it is a stretch of the video's clock, which slow motion
    (part 8's `Speeds`) already is, so the export, the final view and the editor's playhead get it in one place.
    What (a) adds is the water going on afresh from where it was played back to, with the pours where the timeline is;
    that can come later on top of (b)'s clock if wanted.

    - **Where**: the Water card, group *Un-mix* (it is what the video does, not what a liquid is): **Un-mix** (on or
      off), **From** and **To** (water seconds). The Mixing card's old *Remember for Un-mix*, *Seconds* and
      *Un-mixing* go (in Water mode they never did anything).
    - **The clock** (`Speeds`): the video plays the water to To, then backwards from To to From, then forwards from
      From again, on to the end: `water_time(t)` is part 8's up to To's video time, then mirrored, then shifted by
      the two stretches. Each stretch plays at the Speed there, so a slowed stretch un-mixes slowly too. The video is
      `2 (video_time(To) − video_time(From))` longer. `video_time(τ)` is where the video first shows τ. A stretch
      with From ≥ To, or Un-mix off, changes nothing.
    - **No physics change**: the take's print never hashes Un-mix (as it never hashes Speed), so turning it on, or
      moving From and To, makes nothing again.
    - **The editor**: while the Final shows, a Water scene's playhead runs on the video's clock
      (`Studio.water_clock`): the water time is worked out from it each tick, and a scrub sets the clock where the
      video first shows that water time, so playing shows the water going back over the stretch. The draft plays
      forwards as before (going back in the draft plays again from its newest restart copy, so a stretch played
      backwards there would play every frame again from a copy up to 120 frames back).

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

## 4.2: Weight, Keeps apart and colour mixing

Decisions 19 to 21, made exact here. Words: a drop's **inverse mass** `w = 1 / Weight` (air 1); a drop's **mix**
is the share of one other liquid it carries (its colour, the dye in it; its motion stays its own liquid's).

### What is decided for 4.2 (beyond 19 to 21)

- **One flag says the weights differ** (`Fr.mixf.x` bit 0, set by the host when two of the body's liquids'
  Weights, clamped to 0.1 to 10, are not equal). Off, every mass factor below is the literal 1.0 (`select`), the deep
  gather's mass is the volume (so a lone liquid of any Weight moves to the bit as at Weight 1, where today a lone
  Weight 3 liquid gathers three times the face mass), and the deep push is not scaled: a scene of Weight 1 liquids
  runs the arithmetic it runs today. On, the pressure, the pair terms and the deep push share by mass.
- **The pressure by mass** (`sweep.wgsl`): `lambda_at` sums `w_i |sum|^2 + sum_j w_j |g_j|^2` for the stiffness and
  softens by `fr.k.z * w_i`; `apply_at` multiplies the drop's shift by `w_i` (before the cap and the solids). The
  neighbours' `w_j` are read from `velb[j].z` only when the flag is on. Air keeps its own rules (pushed only by air
  and the solids) with `w` 1.
- **Pair terms by mass** (`finish.wgsl`): each pair's evening, closing loss, skin pull, lies and bend are scaled by
  `2 w_i / (w_i + w_j)` (that is `2 m_j / (m_i + m_j)`: momentum kept, exactly 1 for two drops alike). A wetted face's
  and the grip's terms are not (a face has no mass, and a liquid alone must move the same at any Weight).
- **The deep push by mass**: `SUMS` becomes 7 (word 6: mass at the cell's middle, gathered only with the flag);
  `cs_deep_across` writes each cell's (mass, volume) at its middle into a new per-cell buffer `dmass` (binding 31)
  before it empties the sums; `cs_deep_push` scales the drop's push by `m_mean / m_i`, `m_mean` the bilinear mass
  over the bilinear volume round it (1 where there is none), before the push's cap. Momentum per cell is kept (the
  cell's drops share the face's change by mass).
- **Keeps apart** (`MixingCard::keeps_apart`, 0 to 1, default 0; engine `Feel::keeps_apart`): between two liquid
  drops of different liquids, the pair's skin pull, lies and bend are scaled by `1 - max(keep_i, keep_j)`. The pressure,
  evening and closing loss are untouched (oil and water still fill a glass together). Between drops of one liquid,
  nothing changes.
- **The mix word** (`id.w`, and `Drop::mix` in place of the unused `e`): bits 0 to 3 the other liquid, bits 16 to 31
  its share of 65535, rounded; 0 unmixed (a share under 1/65535 is unmixed). Born drops, air and drops a test adds
  without one are unmixed. `Water::add` uploads a drop's mix.
- **The mix passes** (`cs_mix`, `cs_mix_keep` in a new `mix.wgsl`): after the frame's last `cs_finish` (before the
  drains are counted and air is born), when the flag's bit 1 is on (the body has two or more liquids and one has a
  Mix speed above 0), `p` times over: `cs_mix` reads every drop's mix from the sorted ids (binding 5) and writes its
  new one to `mixb` (binding 30), then `cs_mix_keep` copies `mixb` back into the ids' 4th word. Each drop sums, over
  its liquid neighbours (the last step's bins, `xv`'s places), a 16-entry array of how much of each liquid they hold
  (their own liquid `1 - s`, their other `s`), each by `coef_ij = gate_ij (1 - keep_ij) r_i r_j L_ij`, with
  `L_ij = (2 / rho0) 30 / (pi h^3) t^2 / max(d, 0.05 h)` (the spiky kernel's Laplacian weight, `t = 1 - d / h`),
  `keep_ij` as above between different liquids (0 between alike). Its new holding is
  `c_i (1 - S) + acc` (`S` the coefficients' sum, all scaled by `0.9 / S` where `S > 0.9`, so a crowded spot stays
  calm); its share is `1 - c_i[own]`, its other the liquid it holds most of but its own (the lower number on a tie),
  clamped and rounded into the word. With three or more liquids round a drop the dye of the weaker others goes to the
  strongest (own versus other is kept). Air and drops that left take no part and keep their word.
- **Mix speed in px² a second**: `D = 400 s^2` px²/s (`s` the Mix speed, 0 to 1: about 20 px of blending in a
  second at the default 0.5, 40 px at 1), between two liquids `D_ij = 400 s_i s_j`. `r_l` (`Feel.c.z` on the card) is
  `s_l sqrt(400 FRAME / p) / cell`. The passes a frame: `p = ceil(max_l(400 s_l^2 FRAME / cell^2) L_rest / 0.45)`,
  1 to 64 (`Numbers::mix_passes`), `L_rest` the sum of `L` over the resting lattice (`Numbers::mix_at_rest`, as
  `at_rest`). At 2 px a Mix speed of 1 takes about 40 passes; at the cap (finer than 1.5 px at Mix speed 1) it mixes
  as fast as 64 passes allow (an edge case, said in the notes).
- **Mixes when** (`Feel.c.w`: 0 Always, 1 Only when stirred, 2 Only on hits), from the pair's speeds at the frame's
  end: stirred `min(|v_i - v_j| / STIR, 1)`, `STIR` 200 px/s; hits `smoothstep(HIT / 2, HIT, closing)`, closing the
  speed at which they close, `HIT` 150 px/s (`Fr.mixs.xy`, in cells a second); Always 1. The pair's gate is the
  smaller of what each one's rule gives.
- **Fades and Fades to leave the Mixing card** (they need drops that carry an age: Later). The card's colour
  settings form the group "Meeting other liquids": Keeps apart, Mix speed, Mixes when.
- **The take** (`TAKE_FORMAT` 3): `cs_pack` packs 3 words a drop (`packed: array<u32>`, the third the mix word);
  `FrameCodec` keeps two more planes, the share as a byte (`round(share16 / 257)`) and the other liquid; `TakeFrame`
  gains `mixes` (decoded back to the mix word, share byte times 257); `StoredDrops::set` takes the mixes.
- **The look** (`look.wgsl`): `vs_look_splat` passes the drop's other liquid and share (flat); `fs_look_splat` and
  `fs_look_weights` put `k (1 - s)` in the own liquid's channel and `k s` in the other's (only when `s > 0`: an
  unmixed drop writes what it writes today), so every per-liquid look setting blends by the weights; `vs_look_nodes`
  mixes the drop's sigma, glow and colour between its two liquids by `s` (again only when `s > 0`). Mist and Sparks
  stay the drop's own liquid's. **The plain drawing** (`plain.wgsl` `vs_splat`) mixes colour and deep by `s` the
  same way.
- **The probe** colours a drop by its share instead of the stretch (`e` is gone).
- `Fr` gains, at its end after the drains block, `mixf: vec4<u32>` (bit 0 the weights differ, bit 1 colours mix;
  the passes; 0; 0) and `mixs: vec4<f32>` (STIR, HIT in cells a second; 0; 0); `Feel` gains `c` (inverse mass, keeps
  apart, `r`, mixes when).

### Edge cases (4.2)

- Weight outside 0.1 to 10 is clamped (the card's range is 0.1 to 10 too).
- Two liquids of the same Weight: the flag is off (they move as one liquid).
- A heavy liquid poured onto a light pool sinks through it in deep water (the push by mass) and in thin water (the
  pressure by mass); a light one poured under a heavy one rises.
- Keeps apart 1 between two liquids with Mix speed above 0: no colour crosses (`1 - keep` is 0).
- Mix speed 0 on one of two liquids: no colour crosses between them (`s_i s_j` is 0); each still mixes with liquids
  of its own kind that carry dye.
- A drop carrying a share of a liquid the scene no longer has: the look clamps the liquid number as it does a
  drop's own.
- Only when stirred with still water: no mixing. Only on hits at rest: none.
- A restart point and a copy carry the mixes (they are the ids' 4th word, which both keep whole).

### Not to do (4.2)

- No new binding on `cs_finish_predict` (it is at 8) or `cs_finish`.
- No mixing inside the steps (once a frame, after the last step).
- No change to how drops are born, sorted or drained; no change to `fall()`.
- No change to a drop's motion by its mix (it keeps its own Feel).
- No random numbers: ties go to the lower liquid number, sums run in the sorted order.

### Task 1: `bpe-water`: masses (`numbers.rs`, `common.wgsl`, `sweep.wgsl`, `finish.wgsl`, `deep.wgsl`, `deep.rs`, `drops/mod.rs`, `passes.rs`)

- `Feel` gains `keeps_apart: f32`, `mix_speed: f32`, `mixes_when: MixWhen` (`Always`, `Stirred`, `Hits`; defaults
  0, 0, Always); `FeelGpu` gains `c: [f32; 4]`; `feel_gpu` fills `c.x = 1 / clamp(weight, 0.1, 10)`, `c.y = keep`
  clamped 0 to 1, `c.w` the code (`c.z` is filled by `frame_gpu`, which knows the passes). `b.w` becomes the clamped
  weight 0.1 to 10.
- `common.wgsl`: `Feel.c`, `Fr.mixf`, `Fr.mixs`; `fn weighted() -> bool`; `fn inv_mass(kind) -> f32` (1 for air);
  `fn pair_mass(wi, wj) -> f32` (`2 wi / (wi + wj)`); `fn keep_of(ki, kj) -> f32` (0 unless both liquid and different).
- `sweep.wgsl` as decided; `finish.wgsl` as decided (the factors multiply `wj`'s products, so with the flag off and
  keep 0 each is `x * 1.0`).
- `deep.wgsl`: `SUMS` 7, the gather's mass `select(1, weight, weighted) * VOL` (air 1), word 6 only when weighted;
  `dmass` (binding 31) written by `cs_deep_across` when weighted; `cs_deep_push` scales by the mean mass over its own
  when weighted. `deep.rs` `SUMS` 7 and the `dmass` buffer (8 bytes a cell). `passes.rs`: binding 31 on
  `cs_deep_across` and `cs_deep_push` (7 storage each; the test that no pass binds more than 8 still passes).
- `drops/mod.rs` `frame_gpu`: `mixf`, `mixs`, `c.z`; `weights_differ()`.
- Tests (`tests/mixing.rs`, new, on the card): `a_liquid_alone_moves_the_same_at_any_weight` (a pool and a pour at
  Weight 1 and at Weight 4, one liquid: the drops equal to the bit after 2 s); `two_liquids_alike_move_as_one` (half
  the drops liquid 1, both Weight 1: the same drops as one liquid, to the bit); `heavy_liquid_sinks_and_light_rises`
  (a box with a layer of Weight 3 over a layer of Weight 1, each 30 px deep, at 6 px and at 3 px: after 4 s the heavy
  liquid's mean height is below the light's by at least half a layer); `a_deep_pool_settles_by_weight` (the same in a
  pool deep enough for the deep part, 300 px, checked that the deep part ran); `oil_floats` (a blob of Weight 0.8
  under a Weight 1 pool rises to the top within 4 s).

### Task 2: `bpe-water`: Keeps apart (`finish.wgsl`)

- As decided. Test `kept_apart_liquids_pull_into_round_drops`: a 60 x 60 px square of liquid 1 (Keeps apart 1,
  Surface pull 1) in a still pool of liquid 0 (Weight equal, gravity 0): after 2 s its drops' furthest from their
  middle shrinks by at least 8 % (a square rounding up), where with Keeps apart 0 it changes by under 3 %.

### Task 3: `bpe-water`: colour mixing (`mix.wgsl`, `drops/mod.rs`, `passes.rs`, `drop.rs`, `numbers.rs`, `probe.rs`)

- `Drop::mix` (replacing `e`), `Drop::with_mix(other, share)`, `Drop::mix_share()`, `Drop::mix_other()`;
  `DropOut.mix`; `cs_export` writes `id_cur[i].w`; `Water::add` uploads `d.mix`.
- `mix.wgsl` (in the engine's source list, so the engine's print sees it): `cs_mix`, `cs_mix_keep` as decided.
  `passes.rs`: `P::Mix` and `P::MixKeep` (after `Drained`), bindings `[0, 1, 2, 5, 8, 30]` and `[0, 1, 5, 30]`; binding 30 is `mixb` (4 bytes a drop, grown with the drops).
- The frame loop: after the last step's `Finish` (`j + 1 == n`), when mixing, `p` pairs of `Mix`, `MixKeep` on that
  step's group.
- `Numbers::mix_at_rest()`, `Numbers::mix_passes(feels, cell) -> u32`, `MIX_MOST` 400 px²/s, `MOST_MIX_PASSES` 64.
- Tests (`tests/mixing.rs`): `colours_run_together_where_liquids_meet` (two still liquids side by side, gravity 0,
  Mix speed 1, at 6 px: after 1 s drops within 10 px of the line carry shares between 0.15 and 0.85 of the other,
  drops beyond 80 px carry under 0.02, and the amount of each liquid, own plus carried, is kept within 1 %);
  `mixing_is_the_same_at_every_detail` (the blend's width, where shares are between 0.1 and 0.9, at 6 px and 3 px
  agree within 30 %); `no_mix_speed_no_mixing` (Mix speed 0: every word stays 0); `kept_apart_colours_dont_cross`
  (Keeps apart 1: every word stays 0); `stirred_mixing_waits_for_a_stir` (Only when stirred, still: words stay 0);
  `a_restart_point_keeps_the_mixes` (state at 1 s, from_state, both on to 2 s: the same drops and words).
  `numbers.rs` unit tests: the passes grow as the cell shrinks and stay within 1 to 64; `mix_at_rest` is positive.

### Task 4: `bpe-water`: the take and the drawings (`pack.wgsl`, `capture.rs`, `take/{mod,frame}.rs`, `draw/stored.rs`, `look.wgsl`, `plain.wgsl`, `draw/sets.rs`)

- As decided. `frame.rs` unit test: a moment with mixed drops reads back its mixes (the share within 1/255).
  `tests/look.rs` `a_mixed_drop_draws_between_its_liquids` and `tests/plain.rs` `a_mixed_pool_draws_between_its_liquids`
  (a red liquid and a blue one; a pool of the red carrying half the blue draws a colour between the two pools', with
  both its red and blue within 25 % of the halfway). `draw/sets.rs` `mixed(cell)`: a pool whose shares run from 0 to
  1 across it, for the full-size look check (`water-look-mixed-c3`, `water-mixed-c3` in `full_size_look.rs`).
- `bpe-cli` and `bpe-studio` pass `frame.mixes` to `StoredDrops::set`.

### Task 5: `bpe-kit`: the Mixing card and the setup (`modules/water/liquid.rs`, `things/water/draft.rs`, `draft/inputs.rs`, tests)

- `MixingCard`: `keeps_apart` (Keeps apart, 0 to 1, default 0), Fades and Fades to removed, the group "Meeting other
  liquids"; `FeelCard` Weight's range 0.1 to 10.
- `LiquidSetup` gains `mixing: Keyed` (its Mixing card); `print` hashes its `keeps_apart`, `mix_speed`, `mixes_when`;
  `inputs()`' `liquid:{k}` part describes them too; `inputs_at` fills the Feel's mixing fields.
- `what_restarts_the_draft`: Mixing's `keeps_apart`, `mix_speed`, `mixes_when` restart, its Un-mix settings don't
  (yet). A test that two Liquids with Mix speed 0.5 poured into one glass give drops carrying shares after 2 s
  (`two_poured_liquids_mix`, kit draft tests, on the card).

### Task 6: check, notes, review, merge

- Focused tests: `cargo test -p bpe-water --test mixing --test look --test plain --test take -- --test-threads=2`,
  the engine's unit tests, `cargo test -p bpe-kit --lib things::water`, `cargo test -p bpe-studio water_mode`,
  clippy, fmt; the full-size look check `LOOK_ONLY=water-look-mixed,water-mixed`, looked at in full and in its 1:1
  crops.
- `docs/notes/water.md` "Part 4.2 (as built)"; this file's "As built: 4.2". One review, fix, merge when green, then
  4.3's tasks.

## 4.3a: ready-made liquids

Decision 23, built before 4.3b because it touches no physics and 4b adds its Ferrofluid row to the same table. Its
Thickness values are today's; 4.3b moves Thickness onto a new scale and retunes the thick rows (Honey, Syrup, Slime,
Paint) on films.

### What is decided for 4.3a

- **One table, one row a liquid** (`bpe-kit/src/modules/water/liquids.rs`, like `looks.rs`): `ReadyLiquid` is Water,
  Oil, Honey, Syrup, Slime, Milk, Paint, Mercury (`ALL`), each with `title()`, `words()` (one line for the menu's
  hover) and `row() -> LiquidRow { feel: FeelCard, mixing: MixingCard, look: WaterLook }`. 4b adds Ferrofluid as a
  row (and whatever card of its own it needs, written by its own row code). Water's row is the cards' defaults.
- **What a liquid writes** (`apply_liquid(scene, reg, id, liquid) -> bool`, false and nothing changed when `id` is
  not a Water Liquid):
  - its **Feel** card: all four settings (Thickness, Surface pull, Weight, Stickiness);
  - its **Mixing** card's *Meeting other liquids* group (Keeps apart, Mix speed, Mixes when), never the *Un-mix*
    group (Remember, Seconds, Un-mixing: that is what the video does, not what the liquid is);
  - its **Look** card's *Its colour*, *Its body* and *Its surface* groups (what the stuff is: colour, clarity, deep
    colour, solid, bend, rainbow split, smoothing, streaks, shine, edge sheen, ripples, thin film), never *Glow*,
    *Drawn* or *Extras* (how the scene's ready-made look draws every liquid: outlines and bands in Cartoon, glow in
    Neon, foam and bubbles), so a Cartoon scene's oil keeps its outline;
  - never its Heat card (heat is Later and stays as the person set it).
  A setting is written as the row's value, or removed where the row's value is the card's default (as a look does),
  so one liquid after another never keeps what the first set. A card the Liquid lacks is added (it can't be removed
  today, but a hand-made file may lack one). Keyframes on a setting stay (as with a look).
- **The name**: a Liquid still called what it was given (Liquid, Liquid 2, Water, a ready look's liquid name, or
  another ready liquid's title, each with or without a number after it) takes the liquid's title; one the person
  named keeps its name.
- **Groups by name**: the groups to write are matched on `SettingDef::group` (`"Meeting other liquids"`, `"Its
  colour"`, `"Its body"`, `"Its surface"`), and a test fails if a card's group is renamed (every setting of the three
  cards is either written or named as left).
- **The writer**: `looks.rs`'s `write` and `write_card` take a filter (`Fn(&SettingDef) -> bool`, the settings to
  write) instead of `keep_colours`; the looks pass theirs, so looks are unchanged. Both become `pub(super)`.
- **The menu** (bpe-studio): a **Use a liquid…** menu button at the top of a Water Liquid's settings, under its
  name and tags (`inspector/use_liquid.rs`, drawn from `inspector::show` after `header::show` for a thing of kind
  `water::LIQUID`), listing the titles with their words on hover. Picking one pushes `Edit::UseLiquid(ReadyLiquid)`,
  which `edits::apply` runs on every selected Liquid (`reached(.., None)`, so a selected Pour is left alone), sets
  `inspector.own_step` (one undo step of its own) and returns true. The scene's physics print changes with the Feel
  and Mixing cards, so the draft restarts on its own.
- **The rows** (Thickness on today's scale; Weight 1 is water; Mercury is as heavy as the card allows):

| | Thickness | Surface pull | Weight | Stickiness | Keeps apart | Mix speed | Look |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Water | 0.02 | 0.5 | 1 | 0 | 0 | 0.5 | the Look card's defaults |
| Oil | 0.08 | 0.35 | 0.9 | 0.2 | 1 | 0 | clear gold, shiny, a thin film's rainbow |
| Honey | 0.6 | 0.6 | 1.4 | 0.6 | 0.2 | 0.15 | deep amber, glossy, a little cloudy |
| Syrup | 0.35 | 0.55 | 1.3 | 0.5 | 0.1 | 0.25 | dark red-brown, glossy |
| Slime | 0.45 | 0.85 | 1.1 | 0.7 | 0.6 | 0.1 | bright green, half solid, very smooth |
| Milk | 0.03 | 0.5 | 1.03 | 0.05 | 0 | 0.6 | white and solid, soft shine |
| Paint | 0.25 | 0.45 | 1.2 | 0.4 | 0 | 0.8 | red, solid, glossy (Paint on paper's) |
| Mercury | 0.01 | 1 | 10 | 0 | 1 | 0 | silver: solid, darkening with depth, the most shine and sheen |

  The look values are tuned on one full-size still (below); the table above is where they start.

### Edge cases (4.3a)

- `id` of another kind (a Pour), or gone: `apply_liquid` returns false and changes nothing.
- A selection of two Liquids and a Pour: both Liquids change, the Pour doesn't.
- The same liquid twice: the second changes nothing but is still an undo step (harmless; the menu was used).
- A Liquid with keyframes on Colour: the base colour is the row's, the keys still lead (as with a look).
- A ready look after a ready liquid writes every Liquid's whole Look (6c), as before.

### Not to do (4.3a)

- No physics change, no new setting, no change to how anything is drawn; goldens and the take don't move.
- No built-in or example scene (the look check's still is a test scene).
- No linking: the liquid's settings are plain values it owns from then on.

### Task 1: `bpe-kit`: the table and `apply_liquid` (`modules/water/liquids.rs`, `looks.rs`, `mod.rs`)

`ReadyLiquid`, `LiquidRow`, `apply_liquid`, the name rule (`takes_title(name) -> bool`), exported from
`modules::water`. `looks.rs`'s writer takes a filter. Tests in `liquids.rs`:
`each_liquid_writes_its_row` (after another liquid first; resolve the scene and compare each card's written
settings with the row's), `a_liquid_leaves_un_mix_heat_and_the_scenes_drawing` (set Remember, Seconds, a Heat card,
Outline, Glow and Foam first; they stay), `every_setting_is_written_or_left_on_purpose` (the groups exist),
`a_liquid_takes_its_title_unless_it_was_named` and `only_a_liquid_takes_one`. Passing: these and the existing
`looks.rs` tests (`cargo test -p bpe-kit --lib modules::water`).

### Task 2: `bpe-studio`: the menu (`inspector/use_liquid.rs`, `inspector/mod.rs`, `inspector/edits.rs`, tests)

kittest in `inspector/tests.rs`: `use_a_liquid_sets_every_selected_liquid` (two Liquids and a Pour selected,
Honey picked: both Liquids are named Honey with Honey's Thickness and colour, the Pour unchanged, `own_step` set) and
`only_a_liquid_offers_use_a_liquid` (a Pour and the scene's settings don't show it). Passing: these and
`a_liquid_shows_feel_look_and_mixing…` (`cargo test -p bpe-studio --lib inspector::tests`).

### Task 3: one still (`full_size_look.rs`, `LOOK_ONLY=water-liquids`)

A course of eight glass cells (two rows of four, Walls between them), each filled at the start with one ready
liquid, made at Coarse detail for 2 s and drawn at 2 s in the Plain look (`water-liquids`). Looked at full size and
in its crops; the rows' look values are tuned until each reads as what it is. Not a golden and not sent to Aedan.

### Task 4: check, notes, review, merge

fmt, clippy, the tests above; notes (`docs/notes/water.md` "Part 4.3a (as built)") and "### 4.3a" under "As
built" here; one review; draft PR, ready, merge on green; message 4b's thread that `liquids.rs` is on main.

## 4.3b: thick liquids

Decision 22. Built on the branch after 4.3a; with Aedan's rule of 2026-11-27 (one full check run and one merge at
the end of the part), 4.3a, 4.3b and 4.4 go to main in one pull request.

### What was found (measured before planning)

Thickness does nothing past about 0.1 today. The evening of speeds (`finish.wgsl`, an XSPH share of the neighbours'
speeds) takes `ν dt 20 / reach²` a step, capped at 0.5 so it stays calm: at 3 px cells (reach 4.5 px, steps of
about 1/480 s) the cap is about 240 px² a second, so water (150) is under it and everything from oil up spreads as
one liquid of 240 does; Thickness 1, 5 and 30 spread alike at 6 and 3 px, barely slower than water. Honey that folds
as it lands at the streams the videos draw (30 to 60 px wide) needs a gravity-viscous length (ν²/g)^(1/3) near the
stream's width: with this scale's gravity (about 4,300 px a second squared), ν of 10⁴ to 10⁵ px² a second.

A grid solve of the viscosity (Batty and Bridson's, the best for coiling) would take a new grid of the speeds, its
own boundary at every surface and many more passes; the plain Laplacian form, solved implicitly on the drops, damps
a blob's spin at its surface (its neighbourhood is one-sided there), and a falling thread then cannot buckle and
coil. So the thick liquids take Peer, Cornelis and Teschner's way (2015, "An Implicit Viscosity Formulation for SPH
Fluids"): each step, a target velocity gradient keeps each drop's spin and its spreading or squeezing and drops its
shear, and the speeds are rebuilt from it, a few sweeps a step.

### What is decided for 4.3b

- **Thickness's scale** (`Numbers::evening`): ν = `visc_base` + `visc_thick` t + `visc_cube` (t − 0.1)³ for t past
  0.1 (100 + 2,500 t + 140,000 (t − 0.1)³ px² a second): unchanged up to 0.1 (water is 0.02), syrup 0.35 about
  3,200, slime 0.45 about 7,200, honey 0.6 about 19,000, paste 1 about 100,000. `visc_cube` is an engine number
  with an override (`VISC_CUBE`), like the others.
- **Who takes the thick passes**: a liquid whose ν is past what the evening's cap gives at this step's length
  (`Numbers::thick_extra(feel, cell, dt)` = ν − 0.5 reach² / (20 dt), when positive). A frame with no such liquid
  takes no thick pass and joins its steps as now, so water and every liquid of Thickness 0.1 or less move exactly
  as before (the goldens and pinned pictures don't move).
- **The passes**, after each step's Finish (`thick.wgsl`, new; on a thick frame the steps are not joined, so each
  step's Finish keeps its drops in `xv` and the next step's Predict falls from there, as at a cut):
  1. `cs_thick_grad`: each drop of a thick liquid works out its velocity gradient from its own liquid's neighbours
     (no air, no other liquid), corrected so a straight-line field is read exactly even at a surface: G = A M⁻¹,
     A = Σ (v_j − v_i) ⊗ ∇W_ij, M = Σ (x_j − x_i) ⊗ ∇W_ij (spiky kernel's gradient), M regularised by 5 % of its
     trace where a thread is one drop wide. It keeps G's spin and its spreading and drops its shear: G* = R + D,
     R = (G − Gᵀ)/2, D = tr(G)/2 I. G* is stored a drop (`thick_g`, binding 32, a vec4).
  2. `cs_thick_a` and `cs_thick_b`, in pairs (`thick` sweeps a frame step, even): each thick drop's speed moves a
     share α of the way to the rebuilt one, v' = Σ_j W_ij (v_j + ½ (G*_i + G*_j)(x_i − x_j)) / Σ_j W_ij over its
     own liquid's neighbours and itself; `a` reads `xv`'s speeds and writes `thick_v` (binding 33), `b` reads
     `thick_v` and writes `xv`'s speeds (only the z and w of its own drop: places are only read).
  Inside the liquid this is the Laplacian's smoothing (a straight-line field, a turn included, is kept); at a
  surface it keeps a turn and lets go of shear, which is what a free surface does.
- **How many sweeps and how big a share** (`Numbers::thick_sweeps(feels, cell, dt)`): one sweep with share 1 smooths
  as ν_sweep = σ² / (2 dt), σ² the kernel's second moment on the resting lattice (worked out like
  `mix_at_rest`, `Numbers::thick_moment`); the sweeps are the most any liquid needs, ceil(ν_extra / ν_sweep), rounded
  up to even, at most `MOST_THICK_SWEEPS` (32); each liquid's share α = ν_extra / (sweeps ν_sweep) ≤ 1 (FeelGpu's new
  `d.x`). So ν is in px² a second, the same at every detail.
- **FeelGpu** gains `d: [α, 0, 0, 0]` (WGSL `Feel.d`); the frame's `mixf.z` is the sweeps (0: none).
- Air keeps its own motion (the evening's share couples it as now); a thick drop next to another liquid feels only
  its own; drag at faces and Stickiness are as before.
- **The rows** (4.3a's Honey, Syrup, Slime, Paint) are retuned on stills once this is built.

### Edge cases (4.3b)

- A lone thick drop (no neighbours of its liquid): G = 0, v' = v; nothing changes.
- A thread one drop wide: M is near singular across it; the regularised inverse reads the gradient along it.
- Thickness of a liquid keyframed across 0.1 mid-video: the frame it crosses starts or stops the thick passes and
  the joining (the motion goes on smoothly; the take is still the same each time it is made).
- A frame with thick liquid but no thick drops in the course (all drained or not poured yet): the passes run over
  nothing (each drop checks its kind); cheap.
- Restart points and copies: `thick_g` and `thick_v` live only inside a step; nothing is kept.

### Not to do (4.3b)

- No change to a frame with no thick liquid (joined steps, the evening, the passes all as now).
- No grid viscosity, no new step count rule (the sweeps are stable at any share: each is an average).
- No new binding on `cs_finish_predict`.

### Task 1: `bpe-water`: the scale and the numbers (`numbers.rs`)

`visc_cube`, `evening` with the cube, `thick_extra`, `thick_moment`, `thick_sweeps`, `FeelGpu.d`. Unit tests:
`thickness_sets_the_evening` (extended: unchanged to 0.1, honey about 19,000), `only_thick_liquids_take_sweeps`
(water, oil at 6 px none; honey some; more at 3 px than 6 px; the shares at most 1; paste capped).

### Task 2: `bpe-water`: the passes (`thick.wgsl`, `common.wgsl`, `passes.rs`, `buffers.rs`, `mod.rs`)

The three pipelines and bindings 32 and 33; the frame unjoined and the sweeps after each Finish on a thick frame.
Tests (`tests/thick.rs`, on the card): `thicker_liquids_spread_slower` (a 120 px square lump on a floor, its width
at 0.5 s for Thickness 0.02, 0.35, 0.6, 1: each narrower than the last, honey under 60 % of water's spread);
`thickness_spreads_alike_at_6_and_3_px` (honey's width at 0.5 s within 15 %); `a_thick_blob_keeps_its_spin` (no
gravity, a disc of honey turning rigidly keeps 80 % of its turn after a quarter second); `a_thick_blob_loses_its_
shear` (no gravity, a disc sheared: honey's shear falls under 30 % in a tenth of a second where water's stays over
70 %); `water_takes_no_thick_pass` (a water frame's passes unchanged: `steps_last_frame` and a copy's drops equal to
a body of Thickness 0.1).

### Task 3: judge and tune (`full_size_look.rs` `water-thick`)

Stills in strips (a frame every tenth of a second) of honey poured onto a floor from a tap, a slab of honey off a
ledge, and two blobs pulled apart, at Coarse detail in the container; tuned until honey folds as it lands, sags and
stretches into strands. Then the 4.3a rows' Thickness retuned and the `water-liquids` still made again.

### Task 4: notes

`docs/notes/water.md` "Part 4.3b (as built)" and "### 4.3b" under "As built" here. (The full checks, the review and
the merge come once, after 4.4.)

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

### 4.2

Built as planned, with these changes (the detail is in `docs/notes/water.md`, "Part 4.2"):

- `P::Mix` and `P::MixKeep` sit after `AirBorn` and before `Report` in the pass list (the passes after `Report` have
  no step bind groups); they are dispatched once a frame on the last step's group right after its `Finish`, before
  `cs_drained` and trapped air's passes, as decided.
- The review: **air weighs as the lightest liquid** (`Fr.mixs.zw`) instead of 1, so masses count only against each
  other and air in light liquids is no heavier than in water (`air_rises_in_light_liquids_as_in_water`); where the
  liquid lies is not shared by mass (a way, not a force); a Liquid with no Mixing card mixes nothing; drops files
  keep the mix (`BPEDROP2`); `Water::add` clears a mix word naming the drop's own liquid. The group "Meeting other
  liquids" was added to `bpe-core`'s `SECTION_ORDER` (the inspector refuses groups that are not sections).
- The deep gather's mass is `select(1, the liquid's clamped Weight, weighted)` (air 1) rather than
  `1 / inv_mass`, which is not exact; so a lone liquid is the same at any Weight to the bit.
- **Only when stirred** is `smoothstep(STIR / 4, STIR, |v_i - v_j|)`, not `min(|v_i - v_j| / STIR, 1)`: measured,
  a pool resting under gravity has neighbours moving against each other at up to 34 px/s at 6 px cells and 35 px/s
  at 3 px (the 99.9th percentile 19 and 27), so nothing under 50 px/s counts as a stir. (A body in zero gravity is
  not at rest: its skin rounds it up and drops meet at over 100 px/s, so the still tests rest under gravity.)
- Tests: `no_mix_speed_no_mixing` and `kept_apart_colours_dont_cross` are one test,
  `colours_cross_only_when_they_may`; `stirred_mixing_waits_for_a_stir` became `colours_wait_for_a_stir_or_a_hit`
  (two liquids resting on a floor, settled 2 s with no Mix speed, then Only when stirred and Only on hits: no word
  changes in 1 s; a lump of one falling into the other mixes more than 20 drops where it lands);
  `heavy_liquid_sinks_and_light_rises` is `a_heavy_liquid_sinks_through_a_light_one` (a block of Weight 3 over a
  pool of Weight 1, 6 and 3 px). `kept_apart_liquids_pull_into_round_drops` measures from the start: the square's
  furthest drop (40.3 px) is at 33.0 px within a quarter of a second at Keeps apart 1 (a round drop of its area
  reaches 33.9 px) and at 42.2 px at Keeps apart 0 (the pool's jiggle), so the test asks under 88 % and over 95 %.
- The kit's `two_poured_liquids_mix` (on the card) became a unit test, `the_mixing_card_reaches_the_water` (the
  card's settings reach each liquid's Feel); the engine's tests show the mixing itself.
- `src/lumps.rs` joined the engine's print roots in `build.rs` (4.1 left it out, so an edit to it would not have
  changed the engine's print).
- The probe's `picture` lost its tear colouring (`Drop.e`, the stretch, is now `Drop.mix`).

### 4.3a

Built as Tasks 1 to 4 say. Differences and findings:

- The look check's first still dropped each lump about 100 px into its basin: the lumps threw jets at about four
  times their landing speed (3,600 px a second against 900), up the walls and out of 300 px basins, onto the
  ceiling and into the other basins, where they dyed the liquids (a dark lens of water on the paint, drawn in
  water's deep colour under the paint's surface). Eight Water rows do the same, so it is the engine's flat-impact
  jet, not a row's; the still stands its lumps on the floors instead. Worth judging against real water in a later
  realism pass (a slab landing flat does throw fast jets, but these may be too fast).
- Look values tuned on the still: Oil's thin film 0.35 to 0.12 (it read as dirty), Paint got a dark red deep colour
  (its row had the default navy, which a blend with a clear liquid would show), Mercury went from a pale silver
  (it read as milk) to a mid silver that darkens with depth (Clarity 40).
- Ferrofluid's row added here (part 4b merges first; its thread gave the values): the Feel card's defaults, the
  Look black, solid and glossy, and Keeps apart 1 (oil-based). The still is three rows of three.

### 4.3b

Built as Tasks 1 to 4 say, with these differences (each found on the card in the container, `tests/thick.rs` and
scatter pictures of honey on 6 px cells):

- **Who is thick, and the plain evening**: a liquid past Thickness 0.1 (`THICK_FROM`), not one past the evening's
  cap (`thick_extra` would have given water stages on 1 px cells, where the cap is under 150). A thick liquid has no
  plain evening at all, the stages take all of its ν (`thick_evening`): with the plain evening capped at 0.5 a step
  and the stages on top, a spinning disc of honey lost half its spin in a quarter second (the plain evening pulls a
  surface drop toward its neighbours' speeds, which wears a turn away; the stages then spread that through the
  blob). Without it the disc keeps its spin (3 to 3.01 radians a second).
- **Stages, not sweeps**: plain sweeps would have taken 59 a step for honey at 3 px (and hundreds for paste), so the
  sweeps are RKL1 stages (super time stepping: `s` stages smooth as (s² + s) / 2 sweeps, each a sweep at the
  liquid's share times `(2j − 1) / j` plus `(1 − j) / j` of the stage before last). They are a multiple of three
  and rotate through `xv.zw`, `xp.xy` and `xp.zw`, the stage's number in `blocks[0]` (`cs_thick_tick`), the turn in
  `lamb`: no bindings 32 and 33.
- **Once a frame**, after the last step's Finish (as the colours mix), not after every step: the steps stay joined
  and it costs far less (the stages grow as the square root of what a step wants). `thick_stages(feels, cell)`, at
  most 180 (`MOST_THICK_STAGES`).
- **Only the turn is kept**, not the spreading: kept as a spreading, a tap's stream (its rows parting as they speed
  up) fanned out sideways into spray. M is held up only where it is near singular (its smaller eigenvalue to a tenth
  of its trace): 5 % of the trace everywhere read every turn short, and the disc lost a quarter of its spin.
- **Solid faces hold a thick liquid** (not in the plan): a face within reach counts in each stage as liquid at rest
  behind it, so honey clings, sags off a ledge (before, it rolled off whole) and runs slowly down a wall (68 px in
  a quarter second where water falls 125).
- **The tests** measure at a quarter second (not half); `water_takes_no_thick_pass` is `only_thick_liquids_take_
  stages` (no stages at any detail for Thickness 0.1 or less) and the unchanged goldens; `a_thick_liquid_clings_to_
  a_wall` is new.
- **Not reached**: strands, and one unbroken rope from a tap. The density solve only pushes, so a stretching thread
  parts once it is a few drops thin; a tap's stream parts into lumps a little below the spout (water's does too at
  slow taps), and the lumps pile and fold on the floor. Two blobs thrown apart part at once. Written up as what a
  later part could add (a stream held at its spout's speed for its first reach; a pull inside a thick liquid).
- **Rows**: Honey 0.6 to 0.8 (it read as runny at this scale's gravity: a 60 px layer of 0.6 slides off a wall at
  about 270 px a second), Slime 0.45 to 0.6.

### 4.4

Built as decision 24's choice says. The draft plays forwards only (going back in it plays again from a restart
copy), so the playhead runs on the video's clock only while the Final shows, and an exported draft leaves Un-mix
out. The Mixing card's old Un-mix group went with it, and 4.3a's `LEFT` lost "Un-mix".

