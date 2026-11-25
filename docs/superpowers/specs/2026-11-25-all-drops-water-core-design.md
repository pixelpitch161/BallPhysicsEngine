# Water mode: the all-drops water core (design)

Owner: Aedan. Thread "Build the all-drops water engine", 2026-11-25. This replaces section 3, "The physics core",
of the Water mode spec (`2026-11-24-water-mode-design.md`). The rest of that spec stands, apart from the changes
listed at the end of this file.

The trial this design builds on was a throwaway engine on branch `claude/project-thread-awquwz` (draft PR #228,
never merged). Its plan and every number it measured are in `docs/superpowers/plans/water-mode/all-drops-trial.md`
on that branch. Films and stills are in the project files under `water-core/all-drops-trial/`, and the results
page Aedan chose from is https://claude.ai/artifact/EuVFgnGuuBRmZw6AfYD2kP. The earlier review that led to the
trial, "Water Reality Check", is https://claude.ai/artifact/G31C5yXbcabsgGkwR1rHdN.

Numbers marked *target* are unmeasured goals. Lines marked **(My call)** are choices Aedan was not asked about.
They are cheap to change before the part that builds them.

## Intent

In Aedan's words:

- 2026-11-25 00:32Z: "The liquid should be as good as you can possibly make it."
- 07:54Z: "It doesn't necessarily have to look similar to the previous liquid iteration. I want it to look better
  and to look as much like actual liquid would behave as possible."
- 16:21Z, after the review: "Option B sounds good, but I want to see option C first before committing to B".
- 19:21Z, after the trial: "All-drops engine. Middle, slider reaches both."

So Water mode's liquid is made of drops that feel their neighbours directly, with no grid holding the body
together. The everyday default sits in the middle of Surface pull (0.5: tap-scale water that holds and then
beads), and the slider still reaches spray at 0 and ropes at 1.

The trial showed that this way is clearly closer to real water on thin water: a tap stream on a slope, a sheet
off a ledge, a pool draining through gaps, beads, and Water Weird. It also gets better at finer detail, where the
grid core gets worse. It also found weak spots, and this design exists to close them:

1. Deep pools squash by 3 to 4 % (the grid core keeps 99.2 to 99.8 % of a pool's height).
2. Aedan's own Water Weird file (Surface pull 0.2) turns to rain on the block at Fine, but holds at Extreme. The
   same pour lands differently at two details, so a draft would not be a fair preview of the final.
3. A pour has to be born as even rows at exactly the resting spacing, or it sprays from the spout.
4. It is 2.2 to 2.8 times slower than the grid core.
5. Water may cling to flat shelves more than it should: 7.3 % of the course's water was left on its top shelf
   after 30 s at Extreme, against 0.4 % on the grid core, both at Stickiness 0.5. (The grid core is not the bar
   here: it had no grip at all.)

Success means the water in a Water scene looks and moves like real water on every reference shot, at draft and
at final detail, on Aedan's RX 9070 XT. It also means part 2b (water in the editor) can be built on this engine
and this drawing without coming back to the physics.

## Goals

- **The engine proper**: the trial's method rebuilt as Water mode's real engine in `bpe-water`, with every
  lesson the trial learnt kept.
- **Firm deep water.** The grid's pressure solve works underneath deep water, so pools keep their height as well
  as the grid core's did. Thin water is left to the drops.
- **Landings the same at every detail.** A hard landing spreads into one body that throws droplets from its
  edges, and it does so at draft, Fine and Extreme alike. The body never turns to rain.
- **Even births.** Pours come out as an even stream at every detail and pour exactly the amount their card asks.
- **The water picks its own step** from how fast it is moving (water spec section 3, "Its own clock").
- **Speed**: at least as fast as the grid core at draft detail, so the draft plays live with room for drawing.
- **A plain drawing on the card**: smooth, round, unstreaked water, drawn live from the drops, which the editor
  (part 2b) and the Plain look use.
- **Same card, same result**, as the water spec requires: a run repeats exactly, from the start and from a copy.

## Non-goals (for this design)

- The editor. That is part 2b, which comes after this design and builds on it.
- Several liquids mixing, Weight (one liquid floating on another), thick liquids such as honey and slime, heat,
  rain, drips, bursts, drains, leaks and forces. Those are part 4. The engine leaves room for them (each drop
  carries its Liquid, and the step has a place for outside pushes), and nothing here builds them.
- Moving parts (part 5). The engine leaves room for a solid that moves; it does not build one.
- Light and the six looks (part 6). The plain drawing is the base that part 6's renderer adds to.
- Foam, bubbles, mist and froth (part 6's extras). The engine keeps what they will need (each drop's fullness,
  its surface direction, its speed), and draws none of them.
- Tiles for courses bigger than a screen (part 2c). The drops need no tiles. The deep-water grid stays dense, as
  the grid core's is, until part 2c.
- Splitting or merging drops to follow thin water more finely. That is a bigger change, and it is only taken up
  if the landing mark below cannot be reached without it.

## Anti-goals

- Thin water must not get worse than the trial's, shot by shot, at any detail. The trial's films are the bar.
- The deep-water push must never touch thin water (anything within `D` pixels of air: a stream, a film, a sheet,
  spray, a bead), and must touch the same water at every detail.
- Breaking into drops must be the liquid's Surface pull's doing, never the detail's or the grid's.
- Balls scenes do not change. No golden, pinned picture or sound may move.
- No speed or size number goes to Aedan as a fact unless it was measured on the RX 9070 XT.

## Constraints

- Everything runs in wgpu compute. It has to work within wgpu's default limits, because the editor's device is
  made with them (`bpe-studio/src/main.rs::wgpu_options`). That means a storage binding of at most 128 MiB and
  8 storage buffers a stage.
- The processor side of `bpe-water` uses `bpe_core::maths` (the crate's `clippy.toml`). Shaders may use WGSL's
  own maths.
- Tests that use the card go through `bpe_gfx::test_device()`, and devices made without a window go through
  `headless_device`. Tests that draw run on Aedan's card in CI. Card runs for films and timings use the Remote
  Control recipe in project memory `liquid-speed` (results as a `[skip ci]` commit, removed before merging).
  Aedan asked for tests to run on his own card whenever possible.
- Detail: a draft cell is 12, 8 (the default) or 6 px; a final cell is 3, 2 (the default) or 1.5 px
  (`DraftDetail`, `FinalDetail`). Drops rest half a cell apart, four to a cell. Caps (water spec): 500,000 draft
  drops and 8 million final drops.
- The water's gravity is 4000 px/s², eight times the scene's. Fast water crosses one or two drop spacings a step.

## Approaches considered

### The method (settled by the trial)

The trial picked **position-based drops** (position based fluids, Macklin and Müller 2013) over the
pressure-solving particle methods (IISPH, DFSPH). At this gravity those need four to six times more steps to stay
stable, while a position-based drop cannot blow up at any step: its place is corrected directly, and its speed
follows from where it ended up. Walls are exact. Aedan chose this engine over Option B (the grid body with drops
only for thin water), which would have needed this same engine plus a hand-over wherever a stream meets a pool.

### Deep water (the open question, decided here)

1. **The grid's pressure underneath deep water. Picked, and it is what Aedan was offered.** Each step the drops
   are gathered onto the course grid, and the grid core's one-sided multigrid solve works out the pressure.
   Drops well below the surface take the pressure's push on their speed, and the drops' own sweeps then even out
   their spacing everywhere. This ends the squash at any depth, works under overhangs, in pipes and in moving
   water, and leaves thin water alone. The cost is the grid solve each step. The grid core's whole step, solve
   included, ran in 0.15 to 0.18 s for each second of scene at Fine, and this design pays for the solve by taking fewer
   steps and fewer sweeps, because the pools no longer need them.
2. **A starting pressure from depth alone** (each drop's push from how much water sits straight above it).
   Cheap, and exact for a still open pool. Wrong under rock overhangs, in pipes and U-bends, and in water that
   moves. Courses have all of those, so this is ruled out.
3. **Simply more sweeps.** The trial needed 8 sweeps for 98 % at Extreme and 10 to settle at 2 px. The cost grows
   with depth and with detail, and it still sags about 2 %. Ruled out.

The trial also tried keeping each drop's pressure from one step to the next. With few sweeps that is unstable (a
pool boils), so it is not used. A kept pressure needs a solve that has spread across the whole pool, which is what
the grid's multigrid provides.

### Building it

**(My call)** The engine is written afresh in the crate's own layout from the trial's lessons, with the trial's
shader as a reference that is known to work. The trial's code is not merged as it is: it caps sorting at 192 drops
a bin, sends the card one job per step, and carries knobs that were tried and dropped.

## The design

### 1. Drops

- Grid units as today: the layout cell is 1, and the drops rest half a cell apart. The spacing `s` is
  `cell / 2` in pixels: 6, 4 and 3 px in draft, 1.5, 1 and 0.75 px in final.
- Each drop carries its place, its speed, its Liquid (`kind`, up to `MOST_LIQUIDS` = 16), a serial that is unique
  in its body and given in birth order, and a colour. The working values of a step (the place it would reach, its
  multiplier, how full its neighbourhood is, which way the liquid lies from it) live beside them.
- **Stored by field (My call).** Each per-drop buffer holds at most 16 bytes a drop, so 8 million drops fit the
  128 MiB binding limit, and no pass binds more than 8 storage buffers. (The trial's 64-byte drop capped a body
  at 2 million.)
- Drops come and go as `Drop`s in the layout used today (`sim::drop::Drop`), so the probes, the fills and the
  bridge keep working while they are still needed.

### 2. A step

The order of the trial's step stays the same. Changes from the trial are in **bold**.

1. **Predict**: gravity, Settles, the speed cap, and room for outside pushes (part 4's forces). The place a drop
   would reach is found in sub-moves of 0.4 cell, each kept off the solids, so no thin wall is jumped.
2. **Sort** each drop into its bin (one reach wide over the whole course), and within a bin by serial. **The order
   is the same on every run, with no cap on how many drops a bin may hold.** The trial ranked by serial only up to
   192 drops a bin and above that kept the order the drops arrived in, so a heap of drops could break a repeat.
3. **Deep water** (section 6): the grid's pressure push for drops below the surface. Added by part C2.
4. **Sweeps**: the density with the steep (spiky) kernel, used for the push as well, with the solids counting as
   liquid by their share (the trial's wall table). The multiplier never goes above nothing, so the liquid pushes
   and never pulls. Then the drop is moved by its own and its neighbours' changes and kept off the solids. **4
   sweeps** a step in C1, as in the trial. C2 lowers this once the deep push carries the pools' weight, but only
   as far as the pool marks still hold.
5. **Finish**: the speed is how far the drop really went. Then neighbouring speeds are evened (the liquid's
   thickness), the skin pulls (a pull between near drops that turns into a push when they touch, plus the pull
   along the surface's bend, worked out from smooth-kernel directions), and grip and drag act at the solids. No
   speed is kept into a face.
6. **Births** (section 8) put a step's new drops in.

These starting numbers carry over from the trial: a reach of 1.5 cells, softening 4, a drop resting 0.25 cell
off a face, the full solids' share, sub-moves of 0.4 cell, a sweep moving a drop at most 0.25 cell, evening 150
px²/s, skin 4.0e5 px³/s² at Surface pull 1, pull 2, bend share 0.25, the skin's step limit of 0.005 cell, grip 2
gravities, drag 2 /s plus 30 /s at Stickiness 1, reaching 0.75 cell. Every number lives in `Numbers`, and each can
be replaced by a `BPE_WATER_<NAME>` variable in the measuring tests, as today.

**The Feel card's settings**:

- **Surface pull** sets the skin, on exactly the trial's curve: its strength goes with Surface pull squared (4.0e5
  px³/s² at 1, a quarter of that at 0.5, 0.04 of it at 0.2), so every value Aedan saw in the trial means the same
  here, his Water Weird file's 0.2 included. 0 has no skin at all: the spray end. The skin is set in pixels, so it
  is the same at every detail; the clock gives it the steps it needs (section 3). The trial's own runs capped it a
  little at 2 px and at Surface pull 1 at Extreme (to about 74 % and 62 %), so the bar there is the formula's
  strength, judged by eye against the trial's films, not the films' slightly weaker skin. The default changes from
  0.2 to **0.5** in `FeelCard` and in `bpe_water::Feel`.
- **Thickness** sets how strongly neighbouring speeds are evened, in px²/s, so it means the same at every
  detail. 0.02, the default, is the trial's water. Truly thick liquids are part 4.
- **Stickiness** sets grip and drag (section 5).
- **Weight** is stored for each Liquid for part 4, and has no effect with one Liquid.
- The grid core's `spray` goes. Break-up belongs to the skin.

### 3. The clock: the water picks its own step

- The water moves on in **frames of 1/60 s**, the take's frame rate (water spec section 5). Each frame is cut into
  `n` equal steps. Settings that have keyframes (gravity, Feel, a Pour's Rate) are read once a frame **(My call)**.
- `n` is the largest of three counts, capped at `n_most`:
  - **speed**: the fastest drop moves at most `move_most` drop spacings a step. The trial's fixed rates gave up
    to about 1.3 spacings a step for its fastest drops, and `move_most` starts there and is tuned on the shots.
  - **skin**: the skin's pull moves a drop at most the trial's 0.005 cell a step. So a strong skin at fine detail
    gets the steps it needs, and its stability cap never weakens it (a weaker skin at finer detail would break
    "same water at every detail").
  - **floor**: a lowest count for each detail. In C1 the floor is the trial's fixed rate (480 a second at 12 px
    cells, 960 at 8 and 6 px, 1920 at 3 px and finer), because without the deep push the pools need it. C2
    lowers the floors as far as the pool marks allow, and that is where most of the speed is won.
  - `n_most` is 128 steps a frame (7680 a second) **(My call)**. The skin alone needs about 37 a frame at the
    default (0.5) at 2 px, about the same as fast water needs there, and about 115 at Surface pull 1 at 1.5 px. So
    a very strong skin at the finest final is slow, but it is right. Only past `n_most` is the skin capped, and
    that water is marked later (part 7's trouble marks).
- **The fastest speed comes from the card without waiting.** The last step of each frame writes the fastest
  drop's speed (a whole-number maximum, so the same every run) into a small read-back ring. Frame `k` uses the
  value from the end of frame `k − 2`, plus headroom for how much faster water can get in two frames (gravity ×
  2/60 s, and any pour's own speed). The value depends only on the water, so the choice of steps repeats exactly.
  A copy and a restart point carry the last two values.
- A Pour's births go by distance travelled (section 8), so the step size never changes what is poured.

### 4. Sending work to the card

- **A frame's steps go to the card as one submission**: one command buffer with all `n` steps in order, the
  step's numbers written once a frame, and no poll per step. The trial sent one submission per step (960 a
  second at Fine, 1,920 at Extreme) and polled after each. Small scenes cost about a third of a millisecond a
  step whatever their drops, on both engines, and that cost was most of the trial's time.
- **Fewer passes a step.** The trial used 15 dispatches a step. The builder joins passes where the data allows
  (for example clearing the counts inside the running sums, or the finish with the next predict). The one-thread
  block sum becomes a parallel one. This is measured as time per step on a small scene.
- **Re-sorting less often** (bins a little wider than the reach, rebuilt every few steps while no drop has moved
  more than the margin) is a lever to measure. It may change only how often the order is rebuilt, never which
  neighbours a drop feels.
- Births and the step's numbers for a whole frame are uploaded once at the start of the frame. Drops that leave
  (off the course's edge, and later drains) take no part straight away, and are dropped from the count at a frame
  boundary using the count read back two frames later, the same way the clock reads.

### 5. Solids, grip and drag

- The solid field (`SolidField`, a distance for each node) stays as it is. Drops rest a quarter cell off a face,
  and the solids count toward the density by their share.
- **Grip** is a pull toward a near face, from Stickiness. **Drag** acts along a face: a little for every liquid,
  plus more from Stickiness.
- **Shelves (C2).** Real water on a flat shelf runs off its edge until what is left is held by its skin: a wet
  film and flat puddles no taller than about twice the liquid's skin length. That length is 10 px × Surface pull
  under the water's gravity (5 px at 0.5, so puddles of about 10 px), and with no skin nothing but the grip's film
  stays. Stickiness adds clinging films on slopes and undersides, not standing water on a level shelf.
- **Room for moving solids (part 5).** When the solid field carries a speed for each node, a drop keeps none of
  its speed into a face relative to the face's own speed. Nothing here builds that.

### 6. Deep water: the grid's pressure underneath (part C2)

- After the predict step and before the sweeps, the drops' speeds and how full each cell is are gathered onto
  the course grid with whole-number sums, as the grid core does today. The grid core's one-sided pressure solve
  runs (red-black sweeps on a ladder of coarser grids, with the pressure kept from step to step), and the faces
  are corrected (`cs_proj`, `cs_slip`).
- **Each drop takes only the pressure's change** in speed at its place (the corrected speeds less the gathered
  ones, never the grid's smoothing of the drops' own speeds), multiplied by its **depth share** `w`. The place it
  would reach moves by that change times the step. The sweeps then even out the spacing everywhere.
- **The depth share.** A cell counts as full when its fill is at least about 0.9. Solid counts as full, so a pool
  against a wall is still deep at the wall. Depth is the distance to the nearest cell that is not full, found by a
  few short passes, and **measured in pixels**, so the push reaches the same water at every detail. `w` is 0
  within `D` of that and 1 deeper than `D + B`, smooth in between. `D` starts at 24 px and `B` at 16 px, each never
  less than 2 cells **(My call)**, and both are tuned. So a stream narrower than about 48 px (Water Weird's is
  19 px, a Pour's default 40 px), a sheet, a film, beads and spray never get the push, at any detail, and the
  drops alone decide what they do. A thicker pour's core may get it where it lands, and then the same way at every
  detail. At fine detail the layer above `D` is many cells deep with no push under it: see Risks.
- **Why speeds, not the drops' multipliers.** Starting each drop's multiplier from the grid's pressure would
  make two solvers chase two slightly different resting states, and they would fight. The grid removes the
  squeeze as speed (the large, slow part: the weight of the column), and the drops fix their own spacing (the
  fine part). This is how hybrid grid and particle methods split the work (Raveendran, Wojtan and Turk 2011).
- The grid is the layout cell at first. A grid twice as coarse is a speed lever, used only if the pool marks
  still hold with it.
- The grid's kept pressure is part of the water's state: copies and restart points carry it.
- **Then fewer steps and sweeps.** With the pools held up from below, C2 lowers the clock's floors (section 3)
  and the sweeps a step, each only as far as the pool marks and the landing mark still hold.
- **A fill at the start begins at rest (My call).** Before the first frame, the grid's pressure is worked out
  with the drops held still, so a pool that is there when the video starts does not settle and bounce.
- The grid core as a whole engine is retired in C2. Its pressure solve, its fill and its solid faces stay; its
  drops' passes (spreading to the grid, gathering back, hold, elbow room, spots) go, along with the tests that
  only test them.

### 7. Hard landings, and the same water at every detail (part C2)

**What real water does.** A stream that lands hard on a block spreads into a sheet that runs away from the
impact and thins as it goes. Its rim throws droplets. The sheet stays a sheet until it meets something or gets
thin enough to tear. A weak skin (the spray end) throws more and finer droplets and tears sooner. A strong skin
(the ropes end) keeps thick rims and pulls into beads. In no case does the whole pour turn to rain where it lands.

**The rule.** Anything bigger than a couple of draft cells lands the same way at every detail. Spray finer than
the draft's drops shows in the draft as the draft's own drops. This is the water spec's "same water at every
detail", applied to break-up.

**First find out why** (systematic debugging, before any fix). Make dot pictures of the landing on Aedan's Water
Weird file at Fine and at Extreme, frame by frame, on the C1 engine. These are the likely causes, each with the
fix it would point to:

1. **The sweeps' push turns into speed.** At an impact the drops are squeezed and the sweeps part them, and a
   drop's speed is how far it moved, so the parting becomes speed that nothing takes back in the air. Part 2a-2
   met the same failure on the grid core and cured it by moving places, never speeds. The fix: a sweep's push
   adds no speed beyond the neighbours' mean, or squeezed liquid evens its speeds more strongly.
2. **A sheet one drop thick has only the skin holding it.** It has no pressure across it, so it tears when it
   becomes one drop thick, and at Fine that happens sooner than at Extreme. The fix: the pull between drops in
   thin liquid grows with the drop spacing, so a sheet tears at the same thinness in pixels at every detail. That
   thinness comes from Surface pull, not from the detail.
3. **The landing is too fast for the step.** Drops cross a spacing into the face in one step. The clock
   (section 3) is the fix, and its `move_most` is tuned on this shot.

The measuring may find other causes. The fix follows what it finds, and the plan for C2 says which was found.

### 8. Pours and fills: born evenly

- **A tap** is born as rows across the spout at exactly the resting spacing `s`. A new row is born each time the
  last row has travelled `s` (a clock of distance that keeps its remainder), and each row is placed where a row
  born at that exact moment would be by the end of the step. A pour starts and stops exactly at its times.
- **The amount is exact without squeezing.** Rate × Width is the stream's width **(My call: half the Rate is a
  stream half as wide, as a tap turned down is)**. That width is `columns × s`, with `columns` a whole number of
  drops, so where the width is not a whole number of drops, some rows have one more drop than others. Those extra
  drops are spread evenly through time by a running remainder, so the amount poured matches Rate × Width × Speed
  within 2 % at every detail. The trial's 6 % squeeze threw bits sideways, so the spacing is never squeezed.
  Below one drop the stream becomes a row of single drops with gaps between them.
- **Spread** fans the columns smoothly, as `Pour::fanned` does today.
- **Only where there is room (My call).** A drop is not born where the liquid is already full (a spout under the
  surface). What it owes is kept, up to a few rows so a freed spout never bursts, and born when there is room,
  judged from the water as it was two frames before, the same way the clock reads.
- **A fill at the start** is the fill box laid with the resting lattice, its lowest row a quarter cell off the
  floor. Every birth (later rain, drips and bursts in part 4 as well) is a patch of the lattice the resting
  density is worked out from. A drop's place never comes from a random number.

### 9. The plain drawing (part C3)

- **On the card** in `bpe-water` (`draw/plain.rs`, `shaders/plain.wgsl`), drawn straight from the drops' buffers
  without reading them back. It draws any picture size and zoom: the stage's whole-course view, and the camera's
  1080 × 1920 shot. It is what the chooser's **Plain** look shows, and what the editor (2b) and the films show
  until part 6's light arrives.
- **What it shows.** The Back is a flat colour. The solids come from the solid field, in a flat colour with an
  edge smoothed against the background and a slightly darker rim. The liquid has a smooth body with a crisp,
  smoothed outline, in its Look's Colour, getting darker and richer with depth over its Clarity in pixels, with
  a thin lighter rim along the surface. Spray shows as round droplets. Nothing is streaked, combed, noisy or
  banded, and a frame looks the same every time it is drawn.
- **How (My call, details left to the builder).** Each drop is a soft round splat summed into an amount picture
  (additive blending, which the card adds in the drops' order, so the picture repeats). The outline is the amount
  cut at a level and smoothed by its slope, one picture pixel wide at any size. Depth comes from a blurred copy
  of the amount at a coarser size, and the rim from the amount's slope. A lone drop is drawn at least about one
  picture pixel across. The splat's size and weight scale with the spacing, so a body drawn from draft drops and
  from final drops has the same outline within a spacing, and a film one drop thick reads as a film, not a row
  of dots.
- **Inputs** are plain values that 2b reads from the cards: each Liquid's colour and Clarity, the rock's colour
  and the back's colour.
- *Targets* on the RX 9070 XT: a 1080 × 1920 picture of 1.4 million drops in 8 ms or less, and a draft course
  (86,000 drops) in 2 ms or less.
- Films and `bpe-kit/tests/water_core.rs` move to this drawing. The trial's drawing on the processor is ported in
  C1 only as a test helper, for films until this one exists. The bridge to today's Liquid drawing
  (`bridge.rs`, `bridge.wgsl`) is removed in C3 once nothing uses it **(My call)**.

### 10. What part 2b sees

The new engine takes over the name `bpe_water::Water`, in the shape 2b expects: `Water::new(gpu, &SolidField,
gravity, Numbers, &[Feel])`, `add`, **`frame()`** (one 1/60 s frame, however many steps it takes; it replaces
`step()`), `copy()`, `read()`, `count()`, the drops' buffers for the drawing, and the Pour and fill births
(`Pour`, `fill`). `Numbers::for_cell(cell)` gives a body's numbers for a detail. In C1 the grid core moves aside to
`bpe_water::grid` (still built and tested, used for comparisons and for its pressure solve) until C2 retires it.

### 11. Repeats

The same card, driver and program version give the same water: twice from the start, from a copy made at any
frame boundary, and between two runs of the program, with the clock and births included. A copy is only made
between frames **(My call)**. Sums go in a fixed order everywhere: neighbours by bin and then serial, the grid's
sums in whole numbers, the fastest speed as a whole-number maximum, and the drawing's sums in the drops' order.
A restart point (part 8) holds the drops (place, speed, kind, serial, colour), the grid's kept pressure, the
clock's last two read-back values, and each Pour's clock and remainder.

## Reference shots and how they are judged

These are the trial's shots, each judged against what real water does, never against the other engine or the old
liquid. They are run at Draft Normal (8 px), Fine (6 px), Extreme (3 px) and the default final (2 px), in the
plain drawing, on the RX 9070 XT, with Surface pull 0.5 and Stickiness 0.5 unless a shot says otherwise, as the
trial ran them, so the trial's films are a fair bar. Surface pull 0.2 and 1 are run on the slope, the ledge and the
beads, and the course is also run at Stickiness 0 (the Feel card's default).

| Shot | What real water does |
| --- | --- |
| Tap stream onto a slope (`slope`) | Slows where it lands, spreads into a layer, runs down, wraps round the lower edge and falls from it, breaking into drops |
| Sheet off a ledge (`ledge`) | A sheet that thins as it falls and breaks into ropes and drops |
| Thick pour into a pool (`pool_pour`) | A splash that throws droplets, churning that dies down into smooth swells, the pool level rising evenly |
| Pool draining through gaps (`course`, openings of 48, 36, 24 and 12 px) | Each opening passes water in proportion to its size; shelves drain and keep only a wet film |
| Beads on a slope (`beads`) | Drops stay round, creep or run, and join into bigger beads |
| Water Weird (Aedan's file, `!Projects\Water Weird.ron`, at its own Surface pull 0.2 and at 0.5) | Lands on the block and leaves it as a sheet and a stream that wraps the disc, throwing some droplets; never a shower |
| Deep pool at rest, and a slosh (`still_box` 0.6) | Comes to rest and keeps its height; a slosh swings for a long time |

## Pass marks

Each part must reach its own marks, measured on the RX 9070 XT. A missed *target* is reported plainly, but it
does not stop the part.

**Part C1, the engine proper**

- Every thin-water shot is the same as the trial's or better, judged by eye side by side with the trial's films
  at Fine, Extreme and 2 px. The course's shares through each opening are within 3 points of the trial's.
- Repeats as in section 11, with the clock and births.
- Births: a tap pours within 2 % of Rate × Width × Speed at every detail, and no drop has sideways speed above 5 %
  of the pour's Speed within 10 spacings of the spout.
- No drop crosses a solid. A lone drop falls freely.
- *Target*: physics alone at Fine takes at most 0.25 s for each second of scene on the trial's shots (the trial
  took 0.35 to 0.43).

**Part C2, deep water and landings**

- A deep pool at rest keeps at least 99 % of its height at Fine, Extreme and 2 px. After 20 s its drops move at
  most 1 px/s (root mean square, as the trial measured; the trial already reached 0.65 and 0.76), and that is not
  growing over the last 5 s. This holds with the default sweeps (no extra sweeps at 2 px).
- Water Weird, at Surface pull 0.2 and at 0.5, at Fine, Extreme and 2 px: the pour leaves the block as one body,
  with no rain over the block. The share of the pour in pieces smaller than two draft cells across agrees between
  those details to within 5 points. A new `probe::pieces` measures it: drops nearer than 1.6 spacings are one
  piece. **At Draft Normal this is a *target*, not a pass mark (My call)**: the 19 px pour is only five draft
  drops wide there. If the draft misses it, C2 says so to Aedan, and part 7's trouble marks show where the draft
  lands differently (see Risks).
- In a scene with no deep water, the deep push on or off gives the same drops to the bit.
- Shelves: after 30 s at Stickiness 0 and 0.5, the course's top shelf holds only a film and puddles no taller than
  twice the skin length (section 5); never a standing pool. The share left is reported beside the trial's 7.3 %.
- C1's marks still hold.
- *Goal line*: a slosh keeps at least 75 % of its swing after four half swings at Fine and 85 % at Extreme. (The
  grid core kept 78 and 95 %, the trial 61 and 62 %. Real water loses very little in four swings.)
- *Targets* (physics alone, seconds for each second of scene): no slower than the grid core at Fine and at Draft
  Normal (0.15 to 0.18 at Fine); within 1.5 times the grid core at Extreme (0.6, or 0.8 for the 166,000-drop
  pool); at 2 px at most 1.5 (2.0 for the 373,000-drop pool). A one-screen course two thirds full at 2 px (about
  1.4 million drops) is measured and the time reported, so Make final's time can be estimated honestly.

**Part C3, the plain drawing**

- Full-size pictures (the full-size look check, `full_size_look.rs` with a Water scene drawn from stored drops)
  are looked at, with their 1:1 crops, for jagged edges, banding, seams and flicker between frames.
- Pinned pictures at 270 × 480 are drawn from a small set of stored drops, never from live physics, so a driver
  update that moves the physics' last bits does not move them.
- The drawing's speed targets (section 9) are measured.
- Every reference shot is filmed at full size in the plain drawing for Aedan.

## Parts

Each part is its own thread and pull request with one Opus review, and ends with films on Aedan's card in the
project files (`water-core/<part>/`). Plans go in `docs/superpowers/plans/water-mode/`, and the as-built notes in
`docs/notes/water.md`.

1. **C1. The engine proper.** The new `Water` in `bpe-water` (section 10), with the grid core moved aside to
   `grid`. Drops stored by field. The step (section 2) with a sort that repeats and has no cap. The clock (section
   3) with the trial's rates as its floor. A frame sent as one submission, and fewer passes (section 4). Births
   (section 8). The Feel card's mapping, with Surface pull defaulting to 0.5. Tests and measures ported from the
   trial (`alldrops.rs`, the shots in `trial.rs`), and the trial's drawing on the processor as a test helper.
   `CLAUDE.md`'s crate map line for `bpe-water` is updated.
2. **C2. Deep water and landings.** The grid's pressure underneath (section 6), the clock's floors and the sweeps
   lowered, hard landings (section 7), shelves, and the grid core retired apart from its solve.
3. **C3. The plain drawing** (section 9), `water_core.rs` moved to it, the bridge removed, and the full film set.

C2 and C3 can run side by side once C1 is merged, because the drawing needs only the drops' buffers. Part 2b
(water in a Water scene) comes after C2 and C3, as the water spec describes, on this engine and this drawing.

## What this changes in the Water mode spec

- Section 3, "The physics core", is replaced by this design. "Same card, same result", Detail, the caps, "Same
  water at every detail", and the rules for several liquids and moving solids stay as they are written there.
- Parts 2a and 2a-2 were built, and are superseded by C1 to C3. Tiles (part 2c) now apply only to the deep-water
  grid and the solid field. The drops need none.
- Part 2b is unchanged apart from sitting on this engine (`Water::frame`) and the plain drawing.
- Part 6's renderer builds on the plain drawing's amount picture.
- Part 8's take holds a frame's drops as section 5 of the water spec says. Its restart points hold what section 11
  here lists.

## Risks

- **A seam where deep water meets shallow.** A band or ripple at the depth where `w` blends in. This is watched
  on the pool shots, and the fix is a wider blend (`B`).
- **The landing mark may not be reachable at Draft Normal** without splitting drops in thin water. If so, the
  draft shows a trouble mark ("spray finer than the draft") in part 7, and splitting comes back as its own design.
- **One fast stream makes the whole course take small steps.** Different steps in different places are not in
  this design.
- **Make final at 2 px is unmeasured** for 1.4 million drops. If it is too slow, the levers are those in sections
  4 and 6, and failing those a coarser default final (the water spec's risk).
- **Lowering the floors can squash or shake the surface layer**, the top `D` (24 px) that the deep push does not
  reach. At 2 px that is 12 cells of drops-only water. C2 lowers the floors and the sweeps only as far as the pool
  marks hold, measuring the layer's own spacing as well as the pool's height.

## Left to the builder

The numbers, tuned on the card. How the passes are joined, how the sort is done, how depth is measured, which of
the landing fixes the measuring points to, and the drawing's splat size, level and blur. Each choice is written in
its part's plan with the measurement behind it.

## Answered by Aedan

- 2026-11-25 19:21Z: the all-drops engine; the default in the middle; "slider reaches both".
