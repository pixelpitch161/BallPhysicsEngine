# Water mode

Spec: `docs/superpowers/specs/2026-11-24-water-mode-design.md` (a scene is a Balls scene or a Water scene, picked
when it is made; ten parts). Plans: `docs/superpowers/plans/water-mode/`. The older Liquid thing of Balls scenes is
`liquid.md`; nothing here changes it.

## Part 1: Water scenes open (as built)

A Water scene can be made, saved, opened and laid out. **No water is simulated or drawn yet**, and it can't be
exported. Balls scenes are untouched: no golden or pinned picture changed.

### The rule: a scene is one kind (`bpe-core`)

- `Scene.sim: SimKind { Balls, Water }` (`bpe-core/src/sim.rs`), default Balls and left out of a Balls file, so
  every older file loads as a Balls scene. **Scene files are format 13**; 12 still opens (`OLDEST_FORMAT`), and a
  loaded file becomes 13 when it is saved again (`save::mend_loaded` sets it).
- Ten new `SlotId`s after Grade: `Water`, `WaterLight` ("Light"), `Back`, `Form` ("Shape"), `Material`, `Moves`,
  `Feel`, `Mixing`, `Heat`, `Main` ("Settings"). `SlotId::in_sim(sim)` says which scene kind a card belongs to
  (Look, Shader and Grade belong to both); `SlotId::shown_in(sim)` is the inspector's card order
  (`SlotId::WATER_ORDER` for Water).
- `ThingKind::in_sim(sim)` (default Balls only) and `ThingKind::offered(def, sim)` (default true: whether one of
  the kind's own settings is shown in that kind of scene).
- The core holds the rule, so no editor path can break it: `add_thing` gives `EditError::WrongScene(kind, sim)`
  ("A Water scene can't have a "balls""), `add_slot` refuses a card of the other kind, `paste_things` skips things
  of the other kind (and what is under them), and `resolve` leaves them out with `ProblemKind::WrongScene` (a
  hand-edited file). Cards of the other kind are dropped from the resolved thing.

### The Water things and cards (`bpe-kit`, data only)

`things/water/mod.rs` (`water::KINDS`, one `WaterKind` struct for all eight) and `modules/water/` (`water::CARDS`,
one `WaterCard`; settings in `scene.rs`, `solids.rs`, `liquid.rs`, `places.rs`). None has a `ThingSim` or a
`ThingDraw`: they are settings a later part reads.

| Thing (`water_*`) | Cards (slot: card id) | Notes |
| --- | --- | --- |
| Scene settings (root) | Water: `water.world` (`WaterWorld`), WaterLight: `water.light`, Back: `water.back` | plus Effects and Grade as before |
| `water_course` (one per scene) | Material: `water.material` ("Rock") | the solid the water runs through |
| `water_piece`, `water_wall` | Form: `water.shape` / `water.line`, Material, Moves: `water.moves` | family "Course" |
| `water_liquid` | Feel: `water.feel`, Look: `water.look`, Mixing: `water.mixing`; Heat: `water.heat` is added with + Add card | not at a place |
| `water_pour`, `water_drain`, `water_force`, `water_lamp` | Main: `water.pour` / `.drain` / `.force` / `.lamp` | a Pour's and Drain's Liquid is a `Target` (default every Liquid) |

- `WaterWorld`: gravity (0, 4000), `screens_wide` and `screens_tall` (1 to 8 each, at most 16 screens in all:
  `screens()`, `course_size()` in world pixels, the course centred on the world's middle), `look: StartingLook`,
  Draft and Final detail. `StartingLook` is Plain and the six looks Aedan picked (Lamp-lit rock, Glass lab, Neon in
  the dark, Paint on paper, Molten, Cartoon). **Part 1 only records the pick**; the looks arrive in part 6.
- `water::new_water_scene(reg, look, name)`: Scene settings with the three cards, a Camera and the Course.
  `water::is_placed(kind)`: everything but the Course and a Liquid. `water::course_of(scene)`.
- The scene, camera and folder kinds belong to both scene kinds. In a Water scene the scene shows only its Video,
  Picture and Sound cards (`things::scene::root_cards(sim)`) and the camera only its Shot and World box
  (`camera::WATER_GROUPS`); both `offered` hooks look the setting up **by key**, because the inspector blanks a
  def's group on a root card.
- `ensure_mixer` does nothing in a Water scene (no sound there yet). `tests/restarts.rs` and the kit-wide tests go
  over Balls kinds only; the settings contract (`simple` count, ranges, sections in `SECTION_ORDER`) covers the
  Water cards too. New section names sit after "Health" in `SECTION_ORDER`, and **a Water section must be a name
  no ball setting groups under** (ranking "Colour" or "Body" moved a ball card's sections, so the Water Look's are
  "Its colour" and "Its body"; `a_water_section_is_no_ball_settings_group` checks).
- Text is left out of Water scenes for now (Aedan, 2026-11-24).

### The editor (`bpe-studio`)

- **The ball engine runs an empty stand-in.** `app/runs.rs::ball_side(scene, resolved, none)` hands the physics,
  sound, particle and bake jobs an empty Balls scene (`Studio.no_balls`) when the scene is a Water scene, so no
  ball job starts or restarts on an edit. Every place that feeds those jobs goes through it.
- **The chooser** (`water_mode/chooser.rs`, a modal): Balls or Water, a Name, "Open a scene…"; Water goes on to the
  seven look tiles. It shows when the app starts with no file (`Studio::new`; a file given on the command line, a
  dropped file or restored work closes it) and on New / Ctrl+N (`ask_new_scene`; the unsaved-work question comes
  after Create). `Studio::new_scene(NewScene { sim, look, name })`. While it is up the keyboard shortcuts are off
  (`no_dialog()`).
- **What a Water scene hasn't got yet says so** (the sentences are consts in `app/mod.rs`): Grade mode's chip is
  greyed (`NO_WATER_GRADE`, and `keep_out_of_grade()` catches the D key and "Open in Grade mode"), Export video is
  greyed (`NO_WATER_EXPORT`), it can't be kept in a video (`NO_WATER_VIDEO`), the Lights and Mixer tabs and the
  Rules view are hidden (the remembered tab is kept for Balls scenes), and the toolbar shows a "Water scene" chip.
  `bpe-cli` refuses with `NO_WATER_EXPORT` (`check_exportable`, at the start of every export). A picture, song or
  font dropped on a Water scene, or behind the chooser, is let go with a sentence and keeps nothing in the scene
  (`drops.rs::lets_go_of_drop`, `NO_WATER_DROPS`, `DROP_AFTER_CHOOSING`).
- **Inspector**: cards in `SlotId::shown_in` order; + Add card and Find go by the scene kind; a Water thing has no
  own card (`inspector::has_own_card`), the Course and a Liquid no Placement (`has_placement`), no Attach row
  (nothing rides), no Follow sound / light / health (`follow::offered`), and the cards a thing is made of have no
  Remove (`water_keeps`; a Liquid's Heat does). The Add thing menu and every Target list offer only the scene
  kind's things (`k.in_sim(scene.sim)`); families "Course" and "Forces" are new.
- **The Water stage** (`water_mode/stage.rs`, shown by `Studio::centre_shows() == Centre::Water` in place of the
  ball preview): a strip with the tool's hint, the rail (`rail.rs`, `Tool::ALL`: Select, Carve, Piece, Pen, Pour,
  Drain, Lamp, Force, Camera path; `ready()` is Select, Pour, Drain, Lamp and Force, and Carve since part 2b; the rest are greyed
  with "Arrives in a later part of Water mode"), and the course fitted to the room (`CourseView::fit`, `to_screen`,
  `to_world`) with a line between its screens, the camera's 9:16 shot, and a marker with its name for every placed
  thing. Select picks a marker and drags it (`transform.position`, on the grid when Snap is on, one undo step); a
  placing tool adds its thing at the click and goes back to Select. `Studio.water_stage: WaterStage`.

### Tests

`bpe-core` (sim, save, edit, clip, resolve), `bpe-kit` `things::water` and `modules::water`, `bpe-cli`
`a_water_scene_is_refused_in_plain_words`, `bpe-studio` `app/water_tests.rs`, `water_mode/chooser.rs`,
`water_mode/tests.rs`, and the Water tests at the end of `inspector/tests.rs` and in `tree.rs`.

### What the next parts pick up

- Part 2 (the physics, after the trial's verdict) reads `WaterWorld`, `FeelCard`, `MixingCard`, `HeatCard`, the
  Pour, Drain and Force cards and the solids; it gives the kinds their sims and replaces the stand-in for Water.
- The rail's Piece, Pen and Camera path tools, the looks behind `StartingLook`, Draft | Final and Make final,
  grading and export are later parts (the spec's "Parts"); Carve, the draft and pours came in part 2b (below).

## Part 2a: the water core (as built)

Plan and measurements: `docs/superpowers/plans/water-mode/part2a.md` (its "As built and measured"). Nothing in the
editor uses it yet (part 2b): the crate is reached only by its own tests and by `bpe-kit/tests/water_core.rs`.

- **`bpe-water`** (after `bpe-gfx`, before `bpe-kit`): the liquid's physics on the card. `Water::new(gpu, &SolidField,
  gravity, Numbers, &[Feel])`, `add(&[Drop])`, `step()` (one liquid step, 240 a second), `copy()`, `read()`.
  `Numbers` holds every solver number (`Numbers::from_env` lets the measuring tests replace any by `BPE_WATER_*`);
  `Feel` is a liquid in the owner's terms (gravity, Surface pull, Spray, Weight, Settles).
- **The step** (`src/shaders/`, joined in `sim/passes.rs`): drops (48 bytes: place, speed, APIC matrix, stretch,
  kind, serial, colour) spread to a staggered grid with whole-number sums (`cs_p2g`); each cell works out its faces'
  speeds, how full it is and what must flow out (`cs_grid`); the pressure is solved one-sided (never below nothing)
  by red-black sweeps on a ladder of coarser grids (`cs_sor`, `cs_down`, `cs_up`), kept from step to step; the
  faces are corrected (`cs_proj`, `cs_slip`); the drops gather their speed back and move in short sub-moves, kept
  off the solids (`cs_g2p`). This is the physics trial's core (plan `physics-trial.md`), ported.
- **Solids are a distance field** (`solid.rs`): `SolidField` holds each node's distance to the nearest face
  (`build` from `Solid`s, `from_distance` from any measure). A hollow wall or a line thinner than a cell is a cell
  thick to the physics, so a face is always between nodes; the trial's thin-wall cases are gone.
- **What keeps streams whole** (all in the plan's "As built"): a face no drop reaches counts as air, not as still
  liquid (the trial's fault: fast lone drops were braked and a thin pour fell as slow flat clusters); thin moving
  liquid evens out its own spacing along its flight (`Numbers::even`, in `cs_g2p`); crowded drops in thin liquid
  part gently (`most_out_thin`); a cell pushes back only when it is full (`fill` 1: resisted earlier, water
  landing on a shape left it in bursts); "thin" is the drops' share of the room the solids leave round a cell
  (`near` in `cs_grid`), so a film on a slope is thin too; and a shut face gives the drops the liquid's speed less
  only its part *into* the solid (`cs_slip`: with the part out of it taken off as well, a film left a corner as a
  fan). A drop's carried stretch (`Drop::e`) pulls through the hold nodes (`hold.wgsl`): weak at the default
  Surface pull.
- **Where it stands (2026-11-25):** not yet as clean as today's core on the owner's own Water Weird scene (its thread breaks
  at Fine and Extreme on their card): a film one or two cells thick never fills its cells, so the one-sided
  pressure does not keep its drops evenly spaced, and the film gathers into humps that leave the disc as
  pieces. The likely cure is a soft crowding push between neighbouring drops in thin liquid (the plan's "Why
  the owner's thread breaks at Extreme"). `water_core.rs` measures it: `WATER_ACROSS=1`, `WATER_DOTS`.
  Part 2a-2 (below) added that push: the thread is whole and clean at Fine on the owner's card, whole but
  knotted at Extreme.
- **Not built yet: tiles.** The grid is dense (every cell of the course stored and stepped). The plan's tiles are
  their own part, before courses bigger than a screen.
- **The bridge** (`bridge.rs`, `bridge.wgsl`; removed in C3, which draws the drops plainly): the drops rewritten in today's 112-byte layout with a wet grid made
  from the solid field, as a `bpe_gfx::liquid_sim::DropsSource`, so today's liquid drawing draws the new core.
  No wetness through it (Wet walls shows nothing), and a thin wall's water stops at the cell-thick face.
- **Test scenes and measures**: `scenes.rs` (feature `test-scenes`: still boxes, the course, the Water Weird
  layout, a thin pour, a sheet off a ledge, a pour on a disc) and `probe.rs` (`Stats`, `Stream`, dot pictures).
- **Pitfall**: a new physics number goes in `Numbers`, its default, `from_env` and `StepGpu` (matching `Step` in
  `common.wgsl`). A pass that binds a new buffer lists it in `passes.rs::ENTRIES`.

### Tests

`bpe-water/tests/core.rs` (still water rests, a pool keeps its height, two runs and a copy are the same to the
bit, a thin wall holds a fast pour, a full box stays shut, a two-cell opening flows, the air does not brake a
pour), `tests/streams.rs` (a thin pour is one narrow stream at 8 and 12 px cells, a sheet stays a sheet, water
leaves a corner as one arc, a lone drop is left alone), `bpe-kit/tests/water_core.rs` (`the_bridge_draws_every_drop`, now `the_plain_drawing_draws_every_drop`). By hand, in release:
`bpe-water/tests/measure.rs` (`look`, `bands`, `strands`) and `water_core.rs::side_by_side` (both cores drawn
from one scene, joined left and right; its header lists the `WATER_*` variables).

## Part 2a-2: thin water keeps its spacing (as built)

Plan and measurements: `docs/superpowers/plans/water-mode/part2a2-thin-spacing.md` ("What the measuring found",
"As built and measured"). Still only `bpe-water` and `water_core.rs`: nothing in the editor uses it yet.

- **Elbow room** (`elbow_room` in `drops.wgsl`, called from `cs_g2p`): a drop on the move that is nearer other
  drops than `Numbers::apart_reach` (0.45 cell; liquid at rest is half a cell apart) moves away from them, its
  place only, never its speed. Neighbours are found through **spots** (`spots`, binding 12): squares half a cell
  wide, each holding how many drops are in it and the whole-number sums of their places within it
  (`cs_hold_p2g` adds them atomically, `cs_clear` clears them), so a drop reads the nine spots round it and
  takes each spot's drops together at their middle (its own spot without itself). No lists, nothing that
  depends on the order drops are handled in. The move is scaled by the drop's own speed (`apart_pace`: none under
  100 px/s, in full from 300), so a pool at rest is left exactly as it lies; it is capped a step (`apart_most`)
  and goes through `keep_off`. Drops on the very same point part each its own way (an angle from its serial).
  A course too big for the spots buffer (over about 2.8 million cells on a card with wgpu's default limits)
  goes without (`sim::spots_bytes`, `Step.r.x` 0).
- **Why places and not speeds**: where a bunch fills a cell the pressure parts it by giving the drops speed,
  and in the air nothing takes that speed back, so each bunch flew on as a short row across the stream (the
  "hairs" of part 2a). Moved apart before they are ever crowded, they are never thrown.
- **A pour is a stream** (`Pour::fanned`): a Pour's Spread fans the stream smoothly across the spout (the drop
  at the spout's edge leans `spread` degrees, the middle none); `Pour::default().spread` is 0, like the Pour
  card's. Today's Liquid thing gives each drop a random way within its Spread (4 degrees by default): on this
  core that is sideways speed in every drop, a stream three times wider and a quarter as dense by the time it
  lands, landing in pulses. `water_core.rs` hands the new core today's births with Water mode's ways (Spread 0;
  `WATER_FAN=1` keeps the scene's Spread, fanned smoothly).
- **Steps by detail** (`Numbers::for_cell`): the liquid steps often enough that water at 2000 px/s crosses no
  more than three quarters of a cell a step, in whole multiples of 240: 12 px cells 240 steps a second, 8 and
  6 px 480, 3 px 960 (never over 1920). A film that hits a shape head-on at over a cell a step is stopped in
  jolts and leaves in ribs. `Numbers::default()` is still 240 (the behaviour tests' grids are coarse); whoever
  makes a body for a detail calls `for_cell`. This stands in for "the water picks its own step" (spec section
  3), which needs the fastest drop read back: part 2b's.
- **`fill` is 0.5** (was 1): a cell pushes back once half its room would fill in a step. At 1 a film thinner
  than a cell met a shape with no pressure until a cell was full, and whether it then left in ripples hung on
  where the cells lay under it (the owner's file showed teeth at Fine that the hand-built layout did not, until
  its grid was moved a quarter of a cell: `WATER_SHIFT`). **Check anything about thin water on several grid
  shifts**, not one.
- **Not cured: barbs at Extreme.** A film one to three cells thick that meets a shape head-on piles up where
  it lands and is let go in pulses, each a barb across the film a little further on (the plan's "Not cured in
  this part"). No number changes it; it needs its own design. On the owner's card the barbs ride down the
  thread as knots (five at 6 s), so the Extreme half of the pass mark is not reached; Fine's is.
- **Measures** (`probe.rs`): `Spacing` (each drop's distance to its nearest other: the median, and the share
  within a quarter of a cell) and `along` (drops by stretch along a line: how even a film is). `measure.rs`:
  `look` prints the spacing, how ragged the Water Weird layout's film and thread are (`probe::ragged`), and
  writes the drops themselves with `WATER_DUMP`; `pace` times scenes with no read-back (the core's speed).
- **Pitfall**: `SPOT` and `SPOT_ONE` in `common.wgsl` and `SPOT_WORDS` in `sim/mod.rs` go together. A drop's
  place within its spot must be worked out the same way where it is added (`cs_hold_p2g`) and where it is taken
  off again (`elbow_room`): both call `spot_of`.

### Tests

`core.rs`: a settled pool plays on the same to the bit with elbow room and without, a heap of 400 drops on one
point spreads without a burst, drops at the grid's edge are safe, the core runs with elbow room off.
`streams.rs`: drops on the move keep apart, two drops on one point part and still fall freely, a film leaves
the block evenly. Unit tests: `finer_cells_step_more_often`, `a_pour_fans_out_smoothly`,
`spots_fit_or_are_left_out`.

## Part C1: the all-drops engine (as built)

Spec: `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md` (parts C1 the engine, C2 deep water and
landings, C3 plain drawing, then 2b the editor). Plan and measurements: `docs/superpowers/plans/water-mode/
core-c1-engine.md` ("As built and measured"). The owner chose this engine on 2026-11-25 after the all-drops trial
(draft PR #228, never merged). Still only `bpe-water` and `water_core.rs`: nothing in the editor uses it yet.

- **Two engines in the crate.** `bpe_water::Water` (and `WaterGpu`, `Numbers`, `Feel`) is the all-drops engine
  in `src/drops/`. The grid core of parts 2a and 2a-2 moved unchanged to `bpe_water::grid` (`src/grid/`, its
  shaders in `src/grid/shaders/`, its environment numbers now `BPE_GRID_*`); C2 retires it but for its pressure
  solve. `Drop`, `Pour`, `fill`, the solid field, the bridge (removed in C3) and the probes are shared.
- **The step** (`src/drops/shaders/`, joined in `drops/passes.rs`): every drop falls and moves to where it would
  be in short sub-moves kept off the solids (`cs_predict`, which also copies in the step's births); the drops
  are counted into bins one reach wide and sorted by bin and then by serial (`cs_sum_a/b/c`, `cs_scatter`,
  `cs_order`, no cap on drops a bin, the bin of drops that left last and unranked: they take no part, so
  their order needn't repeat, and `read()` returns only drops in the course); four sweeps of position-based pressure,
  one-sided (a multiplier never above nothing), the solids counted by their share from a table
  (`cs_lambda`, `cs_apply`); then the speeds, their evening by Thickness, the skin (a pull between drops and a
  bend term), grip and drag (`cs_finish`). The spiky kernel for both density and push, softening `eps` 4.
- **Drops stored by field**, 16 bytes a drop in each buffer (`drops/buffers.rs`): kept from step to step `xv`
  (place, speed), `nf` (which way the liquid lies, how full) and `id` (kind, serial, colour, in two buffers the
  sort moves between); the step's own working buffers besides. `MOST_DROPS` is 8,388,608 (128 MiB a buffer).
  **No pass binds more than 8 storage buffers** (`every_pass_has_its_entry_and_fits_the_limits`).
- **Frames, and the water picks its steps** (`drops/clock.rs`): `frame()` moves on 1/60 s in `n` equal steps,
  all sent as one submission with each step's numbers at its own uniform offset. `n` is the most of the floor
  (8 a frame at 12 px cells, 16 at 8 and 6, 32 finer), enough for the fastest drop to move at most
  `move_most` spacings a step, and enough for the skin, but **never more than `most_steps` = 32** (the trial's
  1920 a second): the skin's bend term is only calm up to about 40 steps a frame (above that it throws a pour's
  drops back out of the spout and sideways), so a skin that wants more steps is capped instead, as in the trial.
  At 3 px and finer every frame is 32 steps. The fastest speed comes from frame `k - 2`'s report
  (read back without waiting), the drops added since and the pours running, plus two frames of fall. Drops
  that left the course are let go from the end at a frame boundary, two frames late.
- **Births** (`Pour::rows`, `Water::set_pours`): a pour is born as rows across its spout at exactly the resting
  spacing, a row each time the last has run that far, each placed where a row born at that moment is at the
  step's end; a width that is not a whole number of drops puts one more drop in some rows, so the amount is
  width × speed within 2 % at every detail. Under one drop wide it is single drops with gaps. The body keeps its
  pours (copies carry them) and builds every step's births on the host. A spout with no room (judged a reach
  behind it, counting the new drop itself) owes at most `held_rows` rows. `Pour::width` is the stream's width:
  Rate × Width is applied by whoever builds the pour.
- **Fills** (`fill_stacked`): the box's resting lattice, each column set down on a face within `FILL_SNAP`
  (16 px, or two cells) below the box's foot, a quarter cell off it, keeping the box's amount; a column over an
  opening or in mid-air stays in the box, one whose foot is in a solid starts on its face.
- **Thin walls are 1.5 cells thick for this engine** (`drops::LEAST_WALL`, `SolidField::build_least`): a one-cell
  wall can have a hole in its gradient where its middle falls between node rows, and fast drops crossed there.
- **A step's end joined with the next fall** (`cs_finish_predict`, births by `cs_born`): one pass fewer a step,
  the same water to the bit; `Numbers::join` 0 keeps the plain order.
- **Measuring runs** (`tests/shots.rs`, by hand, in release): `shots` (films and stills in the plain drawing,
  `plain.rs`), `pace` (wall seconds a scene second, steps a frame, ms a step), `pools`, `openings`, `repeats`; the
  trial's shots are `scenes::shot`. `water_core.rs` plays `old`, `grid` and `drops` side by side through the
  bridge (`Bridge::new(device, field)`, `update(&body.feed())`, `source()`); its all-drops body pours its own rows
  from today's spouts, gets walls `LEAST_WALL` cells thick, and leaves out today's starting drops inside that
  thicker band (they would be thrown out of it). Through the bridge a drop's fullness counts the solids' share,
  so films on walls draw a little fuller than the grid core's (C3 removes the bridge). The card run's script
  is the plan's Task 8 (`card/all-drops-core-c1/run.ps1`, kept with the results on the card-run branches of
  draft pull requests #232 and #233; C2's run starts from it).
- **Handed to C2** (measured here): a pool still squeezes a few per cent under its weight and bounces for about
  0.2 s after the start, its speed peaking near 100 px/s; during that bounce a drop by a side wall is thrown up
  the wall, the faster the wider the gap between the wall and the fill's nearest column (0.67 cell: 600 px/s).
  That gap is the fill's (its columns are the global lattice, not snapped to side faces): C2's fill should sit
  snugly against side faces as well as on the floor. A restart point must keep `nf` too (the bend term reads
  the step before's `n.xy`). Also C2's: the pace target (0.28 to 0.39 s a scene second at Fine against 0.25),
  the 2 px pool not at rest, a pour in zero gravity bursting at the spout, and a skin that stays calm at any
  step count (so `most_steps` can rise above 32 and Surface pull 1 isn't capped at 2 px). Measurements: the
  plan's "As built and measured".
- **Pitfall**: a new physics number goes in `drops::numbers::Numbers`, its default, `from_env_for` and, if a
  shader reads it, `FrameGpu` (matching `Fr` in `common.wgsl`). A pass lists its bindings in
  `passes.rs::ENTRIES`, at most 8 storage buffers.

### Tests

`tests/engine.rs`: a pool stays in its box and keeps its height; two runs and a copy are the same (and with a
pour, copied at frame 7); a heap on one point spreads and repeats; a lone drop falls freely; a thin wall holds a
2000 px/s pour; an empty body moves on; the clock takes more steps for faster water; drops that leave are
counted out; a tap pours its amount, is born even, and is narrower turned down; a 2 px pour with the strongest
skin stays calm at the most steps a frame (it burst at the old ceiling of 128); a submerged spout holds back; a
fill is stacked on the floor; the joined pass changes nothing; a run whose steps a frame change while drops
leave the course repeats (twice and from a copy). Unit tests in `drop.rs` (rows pour their amount,
a thin tap pours single drops, a pour starts at its time, rows are even, a fill is set down on a floor but not
through an opening, a fill in mid-air stays in its box), `drops/numbers.rs` and
`drops/passes.rs`. `bpe-kit/tests/water_core.rs::the_bridge_draws_every_drop` runs for both engines (C3: now
`the_plain_drawing_draws_every_drop`, all-drops only).

## Part C3: the plain drawing on the card (as built)

Spec: `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md` (section 9). Plan, decisions and
measurements: `docs/superpowers/plans/water-mode/core-c3-plain-drawing.md`. Still only `bpe-water`, its tests and
`water_core.rs`: part 2b puts it in the editor and the export.

- **What it is** (`src/draw/plain.rs`, `src/shaders/plain.wgsl`): `Plain::new(gpu, field, rest)` and
  `draw(&body.drop_buffers(), &View, &PlainLook)` draw straight from the engine's drop buffers, never read back,
  at any picture size and zoom (`View { lo, scale, width, height }`, `View::screen(scale)`; a side over the card's
  most is clamped with a message). `output()` is the picture on the card (`Rgba8Unorm` holding sRGB, an
  `Rgba8UnormSrgb` view allowed), `read()` reads it back for tests. One `PlainGpu` (pipelines) per device.
- **Passes**: (1) each drop a splat `(1 − q²)²` of radius 0.9 cell (at least 1.25 picture px, its peak then kept
  visible), summed with `One + One` into two `Rgba16Float` pictures: amount, colour, Deep colour and Clarity, so
  liquids mix where they meet; (2) the depth ladder over the **course**, not the picture (one texel a layout node,
  so the same water has the same depth wherever the camera is and however near): the same splats summed at each
  node, then an `Rg16Float` level of each node's air share and open (not rock) share, padded past the layout to a
  whole number of top texels so every halving (`[1 3 3 1] / 8`, texel by texel) is exact, down to a texel about
  the course's narrow side; past the course is closed (the air's share, not the liquid's: a half float keeps small
  shares exactly, which deep water needs, else its colour is blotchy); (3) the picture: the amount filled up to
  faces the drops rest `rest` cells off, the outline where it passes 0.2, smoothed over one picture pixel by its
  slope; depth from **air's share of the open part** round the pixel, blurred by Clarity (`x = −σ √2 erfinv(2o −
  1)`, light left `exp(−x / Clarity)`), so a pool darkens with depth and light enters only through the air (a film
  on rock or a stream by a wall stays its Colour) while a wall beside the water neither lights it nor darkens it;
  a thin lighter rim of the liquid's own Colour along the surface but not along rock; the rock on top from the
  solid field (smoothed edge, 25 % darker within 3 px of its face); a fixed 4 × 4 dither under half a step, so
  ramps never band.
- **The course's edge is a wall** to the drawing as to the physics (drops stop 2.05 cells in from the grid's edge):
  no light comes in past it, a body resting on it is filled up to it and has no rim there; only the field's rock
  is drawn.
- **Inputs**: `PlainLook { back, rock, liquids: [PlainLiquid { colour, deep, clarity }] }` (sRGB; a drop's liquid
  is `kind & 15`, a kind past the list takes the last; defaults are the Water Look card's: Colour `#2f9bff`, Deep
  `#0b2a4f`, Clarity 200, rock `#6b5e52`, back the rock at half).
- **Courses are carved out of rock.** Light comes in only through the air, so a thin wall with air behind it would
  let a pool's side glow. Water mode's courses are carved out of solid rock (2b), and the test courses are drawn
  that way: `draw::sets::carved` turns a hollow box into slabs, `water_core.rs` treats everything outside the
  closed hollow wall round the course's middle as rock (`drawn_distance`). The physics' own field is unchanged.
- **Pitfall: one ladder level for all pixels at once.** Where liquids of different Clarity meet, neighbouring
  pixels want different ladder levels; the cloud's software card takes one level for a block of pixels (sampled
  or loaded), which drew a bright line at the seam. Each level is read in a loop over the level number.
- **Drops not on the engine**: `StoredDrops::new(device, queue, &[Drop])` puts any list on the card in the
  engine's layout; `stored::save_drops` / `load_drops` write and read drops files (`shots` with `WATER_DUMP=1`
  writes one per still). Hand-built sets (`draw::sets`, feature `test-scenes`): `bench(cell)` (a pool of two
  liquids, a stream, a film on a slope, a blob, lone drops, close pairs, drops that left), `screen_pool(cell,
  share)`, `lattice`, `carved`, `look()`.
- **Speed on the owner's card** (RX 9070 XT, 2026-11-26): 1.4 million drops at 1080 × 1920 in 4.3 ms a picture,
  86,000 in 0.7 ms (targets 8 and 2 ms). The card run's films and stills are in the project files, `water-core/c3/`.
- **Gone**: the bridge to today's liquid drawing (`bridge.rs`, `bridge.wgsl`, both engines' `feed()`) and the
  processor plain drawing (`src/plain.rs`). `shots` films and stills, `water_core.rs` and the look check all use
  this drawing; `water_core.rs` plays `old` and `drops` only (`WATER_CORES`).

### Tests

`tests/plain.rs`: a pool is drawn where its drops are; the same drops draw the same picture to the bit; an empty
body draws the rock and the back; a lone drop shows at scale 1, 0.25 and 0.05; drops that left are not drawn; odd,
tiny and too-big pictures draw; deep water is darker than thin water; the rim is lighter than the body under it;
rock has a smooth edge and a darker rim; a pool is drawn up to its floor at 8, 3 and 2 px; a film one drop thick is
one piece; draft and final outlines agree; a kind without a look takes the last; Clarity 0 is flat; zooming in far
keeps the body whole; liquids of different Clarity meet without a line; a grown body draws its new drops. Unit
tests in `draw/` (the splat's least size, the ladder's levels, sets, drops files). `tests/plain_pinned.rs`: the
bench and a screen pool at 8 px drawn at 270 × 480, pinned to the owner's card (`golden-plain.txt`, one file for
debug and release). By hand: `bench_pictures` (the bench, and any drops file on its shot's course:
`PLAIN_OUT`, `PLAIN_CELLS`, `PLAIN_DROPS`, `PLAIN_DROPS_CELL`, `PLAIN_SHOT`) and `plain_pace` (ms a picture) in
`tests/plain.rs`; the `water` group of the full-size look check. `bpe-kit/tests/water_core.rs::
the_plain_drawing_draws_every_drop`.

## Part C2: deep water and landings (as built)

Spec: `docs/superpowers/specs/2026-11-25-all-drops-water-core-design.md` (sections 6 and 7). Plan, decisions and
measurements: `docs/superpowers/plans/water-mode/core-c2-deep-water.md` ("As built and measured"). Still only
`bpe-water` and its tests: part 2b puts the engine in the editor.

- **The deep part** (`src/drops/deep.rs`, `src/drops/shaders/deep.wgsl`): on a frame that runs it, each step,
  after the sort and before the sweeps, the drops are gathered onto the layout's cells (mass and momentum on the
  faces, volume and fullness at the middles, as whole numbers added atomically, so in any order the same);
  each cell works out its faces' speeds and how full it is (the drops' own fullness, not their share of the
  cell: a share beside a face never reads full); its depth under the liquid's own edge is found down its column
  and across its row (an air cell no drop reaches past a solid is not an edge); the depth share `w` is a
  smoothstep from `deep_d` (24 px) to `deep_d + deep_b` (40 px); what must flow out of each cell is set
  (`cs_deep_want`, below); the one-sided pressure is solved by the grid core's multigrid from the pressure the
  step before kept (`cycles` V-cycles a step, `prime` on the first step after a frame without it); and every drop
  with `w > 0` takes `w` times the pressure's change of speed on the place it would reach, at most `deep_most`
  cells a step. A drop with `w = 0` is not written: thin water is the drops' own, and on or off the deep part
  gives the same drops to the bit where nothing is deep. The pressure (`cellp`) is part of the body's state,
  so copies carry it.
- **Deep water rests as full as the water on it.** The drops-only band above `deep_d` is squeezed a little by the
  water over it; deep water held at exactly 1 was lighter than the water resting on it, and the pool's top
  turned over and over (12 px/s at Fine on the owner's card). The across pass sums the liquid and its fullness in
  that band (binding 26, `rest`, fixed point), and the want pass asks every deep cell to rest that full, at least
  1 and at most `1 + rest_most` (0.02). An over-full cell pushes out `push_out` (0.02) of its excess a step, a
  cell with room fills `fill_rate` (0.5) of it before it pushes back.
- **Deep frames:** frame `k` runs the deep part when frame `k − 2`'s report counted deep cells, or on the two
  frames after `add()`; a pour with nothing deep never runs it (`Numbers::deep` 0 turns it off).
- **Numbers by detail** (`Numbers::for_cell`): steps a frame as C1 (8 at 12 px, 16 at 8 and 6, 32 finer, never
  over `most_steps` 32), `iters` 4, and two V-cycles a step under 2.5 px cells (one at 3 px and coarser). None of
  them could come down without the pool sagging or moving more.
- **Snug fills:** `fill_stacked` finds each side's end along the box's middle row; a side ending in a solid has
  its column exactly `rest_off` off the face, and the columns are spread evenly between two such sides (a gap
  by a side wall threw a drop up the wall during the start's bounce in C1).
- **Landings** (`meet_even` 0.3, `finish.wgsl`): a share of the speed at which a drop and its neighbours close
  on each other (along the line between them, and only while they close) is taken away. Each of a pair loses the
  same, by the pair's own weight over the neighbours' weight a drop deep in the liquid has, so momentum is kept
  (`water_that_meets_keeps_its_momentum`: by each drop's own neighbours, a surface drop took more, and a drop
  fired into a resting blob drove it backwards). The share is for the step that crosses the same share of a
  spacing as Fine's 16 steps a frame (`Numbers::meet_share`), so a 2 px step (32 a frame where the reference
  is 48) or a frame whose fast water took more steps loses as much a second. Drops that meet stop bouncing
  apart, so a hard landing at Fine stays a sheet as it does at Extreme, while streams still stretch and
  sloshes still swing.
- **The bins' running sums** are a parallel scan (`cs_sum_a`: a workgroup of 256 sums a block of 512 bins); the
  same water to the bit.
- **The grid core is gone** (`src/grid/` and its tests): its solve lives on in `deep.wgsl`. `Pour::drops` and
  `Dice` (its random births) went with it.
- **Measured on the owner's card** (RX 9070 XT, 2026-11-26, the plan's Task 10 has the verdict mark by mark):
  the deep pool at rest for 20 s keeps 99.7 % of its height at Fine and Extreme and 99.0 % at 2 px, moving at
  1.3, 2.3 and 6.1 px/s rms, not growing, its top within half a pixel (the spec's 1 px/s is missed: what is left
  is where the push fades in and beside the walls); the slosh keeps 77 % (Fine), 82 % (Extreme) and 76 % (2 px)
  after four half swings. Water Weird's landing has 26, 26 and 22 % of the water above the floor's pool in small
  pieces at 6, 3 and 2 px at Surface pull 0.2, and 30, 29 and 27 % at 0.5 (both within the 5 points). The
  course's top shelf after 30 s keeps 2.4 to 4.0 % of the water (trial and C1: 7.3 and 7.4 %), but its puddles
  stand 7 to 11 px tall where the spec asks at most 7 at Fine and 5.5 at Extreme: the skin's bend (`curv`)
  holds them up (with `curv` 0 they are 2 to 5 px). The course's shares are within a point of C1's, runs
  repeat, and the owner's Water Weird file breaks into fewer pieces than C1's (190 against 320 at Fine), nearer
  today's liquid. Pace: Fine 0.31 to 0.54 s a scene second (the grid core's 0.15 to 0.18 was the target),
  Extreme 0.69 to 1.47, 2 px 1.15 to 2.73; a one-screen course at 2 px (1.4 million drops) is not measured
  (about 10 s a scene second by the big pool's ms a step). A pour plunging into a pool carries air pockets down
  that are a little bigger and last a little longer than C1's. The card run's films and tables are in the
  project files, `water-core/c2/`, and on draft pull request #236's branch.
- **Handed on:** the shelves' puddles and the skin's bend (a look question for the owner), the pace at Fine, the
  pool's last few px/s, the air a plunging pour carries down, the 1.4-million-drop pace, and `most_steps` (a pour
  bursts at 128 steps a frame; a bend taken on fewer steps was worse; a bend from smoothed directions was not tried).
- **Pitfall: judge pools on the owner's card at Fine and finer.** The software card's 12 px box looked calm while
  the card's 6 px pool rolled; probes by depth (fullness, unrest) found the cause where the totals could not.

### Tests

`tests/engine.rs`: a deep pool keeps its height (98.5 % at 12 px); the deep part leaves thin water alone (on and off the
same to the bit); a copy of a deep pool plays on the same; an open edge is not deep; a small course still solves; a fill
starts at rest; no drop climbs a side wall (none above 200 px/s); a pool filled by a pour gets its push without a jolt;
deep frames follow the water; a hard landing stays whole (under 30 % above the pool at 6 px and Surface pull 0.2). Unit
tests in `drops/deep.rs` (the ladder's levels), `drops/numbers.rs` (floors and cycles follow the detail, the meeting
share a second is the same at any step), `drops/passes.rs` (every pass fits the limits), `probe.rs` (pieces tell a sheet
from spray). Also: water that meets keeps its momentum; thin water is left alone through the deep frames after an
`add()`. By hand in `tests/shots.rs`: `landing` and `shelves` (new), and `pools`, `pace`, `openings`, `repeats` as in
C1.

## Part 2b: water runs in a Water scene (as built)

Plan: `docs/superpowers/plans/water-mode/part2b-water-in-the-editor.md` (decisions 1 to 18 there). A Water scene's
stage plays real water at draft detail on the all-drops engine (C1, C2) in the plain drawing (C3): the Course is carved
out of solid rock with a brush, a Liquid's Feel and Look are read, and a Pour pours a tap or fills a box at the start.
Balls scenes are untouched.

### Carving (`bpe-core`, `bpe-kit modules/water/carve.rs`)

- **The Course's Carving card** (`water.carve`, `CarveCard { carved: Strokes }`, slot `Form`, labelled "Carving"; the
  Course's cards are Carving then Rock, `WaterKind.label` is a list now). `things::water::carving_of(scene)` reads it
  (empty without the card: a part-1 Course is all rock). Strokes are in world pixels, the course centred on the world's
  middle: `size` is the brush's radius, `softness` 1 - Hardness, `erase` is Fill, and the new `Stroke.square` (saved
  and hashed only when on, so every older stroke and cached picture keeps its hash) is the square tip.
- **`CarveMask`**: the rock's signed distance at one sample every 4 px (`STEP`) over the course, clamped to ±48 px
  (`BAND`: the engine reads the field only near a face), above 0 open. Nothing carved is all rock and past the course's
  edge is rock (`at` takes the edge as a face). Strokes apply in order on distances: a round tip is `radius - distance
  to the path`, a square tip the exact distance to the square swept along each segment (a convex hull of four to six
  corners); Dig is a smooth max, Fill a smooth min, over `k = softness × radius` (Hardness 100 % is a crisp corner where
  strokes meet). A stroke touches only the samples within `radius + k + BAND` of its path. `field(cell)` is the
  `SolidField` both the physics and the drawing read; thin rock is not thickened (a ridge under about 1.5 draft cells
  can be crossed by a fast drop at draft).
- **`CarvePaint`** keeps a stroke's own shape over the course and the mask with it carved in, a segment at a time:
  painting point by point gives exactly what `apply` gives the whole stroke. **`mask_for(size, strokes)`** caches the
  last four masks and builds on one whose strokes start the same way (a stroke added on the end costs that stroke).

### The scene as the engine's inputs (`bpe-kit things/water/draft.rs`)

- **`WaterSetup::of(scene, resolved)`** (cheap; built on every edit): the course size and Draft detail cell, the
  strokes, the enabled Liquids in the scene's order (at most 16) with their Feel cards, every enabled Pour with its card,
  place, kind and Liquid number, notes for pours that can't pour (`NO_LIQUID`, `LATER_KINDS` for Rain, Drip and Burst,
  `TOO_MANY_LIQUIDS`, `TOO_MANY_TAPS`), and `print`. `inputs_at(frame)` reads gravity, Feels and Taps from their
  keyframes at the frame's start (`frame / 60` s); `fills(layout, field)` fills each Fill at the start's box
  (`fill_stacked`, rest off `Numbers::wall_off`) leaving out a later box's drops within 0.35 cell of an earlier box's;
  `start(gpu, field, most)` is a new body with the fills and frame 0's inputs. A Fill's box is kept to the course.
  `inputs_on(frame, field)` is what the draft plays: each tap's stream only as wide as its spout has room for (every
  drop born more than `wall_off` from the rock, walked out from the spout's middle half a cell at a time, never wider
  than the field), and none for a spout in the rock (`taps_in_rock(field)`; the stage then says `IN_ROCK` for it). Without
  that, drops born in the rock fall through it (the mask is flat inside the rock) and leak into caves below. A Tap is
  `Pour { width: Rate × Width, .. }`; drops keep `STOPS` and are born with the engine's own colour (the plain drawing colours by liquid number).
  A Pour's Liquid: a Liquid named by id, the first Liquid for its kind or Anything, the first with a tag.
- **`print`** hashes exactly what the physics reads: the Water card's Gravity, Screens and Draft detail (and their
  keys), the strokes, each Liquid's id and Feel (and keys), each Pour's id, place, Liquid and card (and keys). Not:
  a look, the Rock, Back, Light, Final detail, Starting look, Mixing, Heat, Pieces, Walls, Drains, Forces, Lamps, the
  camera. `what_restarts_the_draft` checks every setting of every Water card against that list (192 changes).
- **`plain_look(scene, resolved, time)`**: each Liquid's Colour, Deep colour and Clarity, the Rock colour, the Back
  (Darker rock, Colour, a Gradient's middle; a Picture shows as darker rock until part 6).

### The draft in the editor (`bpe-studio water_mode/{draft,picture,carve_tool}.rs`)

- **`DraftJob`**: a "water draft" thread on the window's own device (the drawing reads the drops' buffers on the card),
  low priority. It steps a whole frame at a time under the body's lock (`Live { water, field, plain, generation }`) and
  follows the playhead's frame (`frame_of(time)`); going back plays from the newest restart copy at or before the
  playhead (one at the start and one every 120 frames reached, the copies kept under about 1 GiB by letting every other
  one go; a copy is counted at its buffers' room, which is what `Water::copy` takes). A new setup (`restart`) builds
  the body off the lock and swaps it in. Past 500,000 drops (`DRAFT_MOST`) its pours stop. `busy()` counts setups asked
  for against built, so a test or the stage never sees an old body as done. A panic marks it failed ("· Water draft
  stopped" in the status line); the next physics edit starts a fresh thread. Letting the job go waits for the thread to
  finish its frame, so it never outlives the window's card.
- **Taking turns** (`DraftJob::body(wait)`): the stage takes the body when the thread is between frames; when it
  hasn't for 100 ms it waits for the frame the thread is on, and the thread, seeing `wanted`, yields until the stage
  has it (at most 50 ms) before its next frame. A system's lock need not hand over to a waiter, so without this a
  draft catching up (a Feel slider let go at 10 s: 600 frames) could hold the stage still until it got there.
- **`WaterRun`** (`Studio.water_draft`): the newest setup, the job and the stage's picture. `restart_runs` builds a
  setup on every edit and restarts the draft only when `print` changed, held like the balls' while a slider or a drag
  is held (`hold_runs`); `replace_scene` starts it over (and stops it for a Balls scene). The stage starts the thread
  once the window's graphics card is there.
- **The picture** (`StagePicture`): the whole course at its size on screen in real pixels (at most 2048 a side, never
  above two a world pixel, `picture_view`), drawn only when the body's frame, generation, size, look or rock changed,
  registered with egui as a native texture. The rock it shows (`Rock { version, painting }`) is set under the body's
  lock: the stroke being painted (rebuilt only when the stroke gets a point), or the body's own again once it is let
  go or dropped. While the draft starts over it keeps the look it last drew in, so a reordered Liquid list never shows
  the old body in the new colours. A pill says
  "Working out the water… X s" when the draft is more than 15 frames behind, "Starting the water over…" while a new
  setup builds. With nothing carved the course says `ALL_ROCK`. The status line adds "· Water: N drops (draft 8 px)".
- **The Carve tool**: the strip has Dig | Fill, Brush size (8 to 600 px, 120), Hardness (100 %), Round | Square,
  Straight (or Shift) and Smooth (a lazy brush on a string of half the radius); Alt does the other of Dig and Fill.
  Points come every quarter radius (at least 2 px). While a stroke is painted the picture shows the rock from
  `CarvePaint` over the water still playing; letting go puts it on the Carving card as it is then (an undo made while
  painting is kept) as one undo step (adding the card to a part-1 Course), Escape drops it, a click digs one dab. The
  brush's outline follows the pointer. The strip goes onto more rows when the stage is too narrow for it.
- **Pours**: placing the first Pour also adds a Liquid named "Water" in the same undo step. A Pour's marker shows its
  aim and stream width (a Tap) or its box (a Fill at the start); a picked pour that can't pour says why in the strip.

### Measured on the RX 9070 XT

`bpe-kit tests/water_draft.rs::draft_pace` (one screen carved as one chamber, two thirds filled, a tap; 5 s; Vulkan,
driver 26.11.2, branch at 50b1595, 2026-11-26):

| Draft detail | Cell | Drops | Steps a frame (mean, most) | Physics ms a frame | Draw ms 600 x 1067 | Draw ms 1080 x 1920 | Plays live (12 ms) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Coarse | 12 px | 37,780 | 8.0, 8 | 3.56 | 1.16 | 0.48 | yes |
| Normal | 8 px | 85,004 | 16.0, 16 | 8.39 | 0.53 | 0.54 | yes |
| Fine | 6 px | 151,111 | 16.0, 21 | 10.47 | 0.51 | 0.62 | yes |

A full screen of draft water plays live at every detail, so no speed work was done (plan Task 3b not needed). The
first draw (Coarse at 600 x 1067) includes the card warming up. On lavapipe in a container the same draft runs far
slower than real time, and the editor's frames queue behind the water's on the one queue: judge pace on the card only.

### Tests

`bpe-core` `a_square_tip_is_saved_and_hashed_only_when_on`; `bpe-kit` `modules::water::carve` (the mask: all rock,
round and square holes and their distances, fill after dig, order, hardness, past the course, painting equals building,
a long stroke under a second, the field follows the mask, the cache), `things::water` (the Course's cards and labels,
`carving_of`), `things::water::draft` (taps, fills where open and not doubled, a pour's Liquid, later kinds, keyframes,
detail, what restarts, the plain look, a tap in the rock, a tap wider than its hole, huge pours kept to the course); `tests/water_draft.rs` `drops_stay_out_of_the_rock` (on the card) and
`draft_pace` (by hand); `bpe-studio` `water_mode/draft` (follows and goes back, the same water from a copy and from the
start, a new setup, stops at its most, the copies' memory rule, the stage gets the body between frames while the draft
catches up, a tap in the rock is named and pours nothing), `water_mode/picture` (the picture covers the course),
`water_mode/tests.rs` (Carve: one stroke one undo step, an undo while painting kept, Alt and the strip, Straight,
Smooth, Escape, a Course without the card; the all-rock words; a first Pour brings Water; a pour's note), `app/water_tests.rs` (a physics edit restarts
and a look edit doesn't, a Balls scene has no draft, the readout).

## Part 2c: trapped air and Wetting (as built)

Plan: `docs/superpowers/plans/water-mode/part2c-air-and-wetting.md` ("As built and measured" there has every number).
Air caught under the water rises out as bubbles and bursts at the top, and each solid has a **Wetting**: water beads
up and rolls on a waxy one and spreads into a film on a soaked one.

### Trapped air (`bpe-water drops/air.rs`, `shaders/air.wgsl`, `sort`, `sweep`, `finish`, `deep`, `report`)

- **Air drops**: drops with the `AIR` bit (128) in their kind. They fall upward at `air_lift` times the course's
  gravity (the Water card's Bubbles rise, `Water::set_air_lift`). **One-way pressure**: the liquid counts air in its
  fullness and makes way for it, but air is pushed only by air and the solids, so a pocket rises through the water and
  keeps its room (with the pressure both ways, air that broke off sat in the water's lattice and rode its flow down).
  The liquid's skin leaves air out (a pocket's wall is a surface to it, so bubbles keep round); air has no skin, grip or
  Wetting. Air and liquid even out their speeds and close on each other by `air_drag` (0.2) of the share liquid has
  with liquid (`finish_at`): small bubbles slip up through the water instead of riding its flow (at 1 the air of a
  72 px hole was still spread through the pool 3 s on). The deep part gathers air as full water, so a pocket is no
  longer the liquid's edge.
- **Bursting** (`sweep.wgsl` `bursts`, `finish_at`): an air drop is let go (moved off the course, counted out two
  frames on like a drop that left) when it is less than `air_pop` (0.75) full; when it is out of the deep water, in
  the band under the water's open top that the drops alone hold (`deep_d`, 24 px at every detail; the deep part counts
  air as water, so a pocket is deep and a foam under the top is not); and when liquid over it (within 60° of straight
  up) is less than `air_pop` full. The liquid rests on air (it makes way for it), so without the band a cap or a
  foam's films stood: on the owner's card a plunging pour left about 40,000 air drops as a foam under the top at 3 px
  3 s after it stopped. A real bubble's cap drains and breaks within a moment. Air thrown out against a ceiling
  bursts, and air held deep under a solid stays (`air_held_deep_under_a_bell_stays`).
- **Finding** (`cs_air_find`, on the last step of every frame with deep cells, from the deep sums before
  `cs_deep_across` empties them): a cell neither full nor solid, with full liquid over it (not rock: air under a ledge
  is the course's) and full liquid or a solid under and beside it within `air_reach` (48 px, 3 to 64 cells), is trapped
  air. Its lattice spots clear of the solids and of every drop by 0.4 cell (`AIR_CLEAR`, under a spacing) are marked,
  so the air fills the pocket out to its water (air that stopped short of the water had open room beside it and
  burst at once); a pocket part filled is filled the rest of the way the next frame. Whole-number running sums over
  the cells and blocks (`cs_air_sum_a`, `_b`) give each spot its slot.
- **Births** (`cs_air_dead`, `cs_air_born`, after the frame's last finish): the host keeps `slots_for(found, deep)`
  slots of room after the frame's drops (the spots the report two frames back found and a quarter more, plus room for
  pockets that closed since: a quarter of the deep cells, 64 to 2048; none when it found none). Each spot is born at
  the liquid's speed round it, in its slot counted from the **head of the drops that left** (`starts[cells]`, where
  the last sort put them), and the room past the births holds drops that left: the body always ends in drops that
  left, as every boundary lets go from the end. (Born after the drops that left, the next boundary cut live air born
  the frame before: `a_boundary_lets_go_of_no_air`.) The report counts the births as live and reports the spots
  found (`Report.air`, `Water::air_found`) and the liquid's drops in the course (`Report.liquid`, a whole-number
  count). **`Water::liquid()`** is that count from two frames back plus the liquid poured since: the editor's draft
  caps its pours and shows its count by it (`count()` also holds air and the room kept for it). Everything a frame finds is born in its own submission,
  so a copy at a frame boundary needs nothing and runs repeat. An empty hole falls in for the two frames the count
  takes to come back (at 12 px a 72 px hole keeps about 47 air drops of the 113 it would hold).
- **Off** (`Numbers::air` 0, the Water card's Trapped air): none of the passes run and no slot is kept: C2's water to
  the bit. Water in the open (slopes, thin pours, still pools) never gets air and plays the same with it on.
- **Not drawn**: the plain drawing skips air drops (`plain.wgsl` `drawn`), so a pocket shows as a hole. `probe`'s
  measures leave air out (`probe::liquid`, `probe::air`); `probe::pockets` measures trapped room; `probe::picture`
  draws air as small pale yellow dots.

### Wetting (`bpe-water solid.rs`, `numbers.rs` `wet_table`, `finish.wgsl`)

- A wetted face is resting liquid to the liquid's skin, as much as it is wet (0 to 1): within a reach of a face a water
  drop's skin pull gains `wet × thin × S(d)` toward the face (adhesion, `thin` the pair number with a full neighbour)
  and the way the liquid lies gains `wet × L(d)` toward it, so a puddle's foot is no surface for the bend to hold up.
  `S` and `L` are what a resting lattice filling the solid behind a flat face would add (`Numbers::wet_table`, 33
  distances, sent in `Fr.wet`). Wetting 0 is C2's water to the bit; water further than a reach from every face plays
  the same at any Wetting. `wet_drag` (1) is the share of the drag along a face a waxy face keeps: beads already roll
  off a waxy slope at 1, so it stays.
- `SolidField.wetting` is one value per node (`with_wetting`, `wetting_at`, read between nodes on the card from the
  second half of the solid buffer). In 2c every node takes the Course's Rock card's Wetting; part 3 gives Pieces and
  Walls their own (the nearest face's solid). **`DEFAULT_WETTING` is 1** (stone and clean glass are fully wet): on the
  owner's card it is the least Wetting at which the course's top shelf drains to puddles of at most 7 px at 6 px and
  5.5 px at 3 px. It is the Material card's default and every `bpe-water` scene's.

### Cards and the editor (`bpe-kit modules/water/{scene,solids}.rs`, `things/water/draft.rs`, `bpe-studio`)

- **Water card**, group **Trapped air** (after "Carving" in `SECTION_ORDER`): **Trapped air** (on) and **Bubbles rise**
  (0 to 4 times, 1, keyframed). **Material card**: **Wetting** (0 to 1, simple). `WaterSetup` keeps the Course's Rock
  card: `print` hashes Trapped air, Bubbles rise and the Rock's Wetting (a Piece's or Wall's Material does not restart
  the draft yet); `numbers()` sets `air` from Trapped air; `field()` takes the Rock's Wetting;
  `FrameInputs::air_lift` is Bubbles rise at the frame's start, which the draft sets on the body each frame.

### Measured on the RX 9070 XT (Wetting 1, Surface pull 0.5, Stickiness 0)

Tables, stills and films: project files `water-core/2c/`; every number in the plan's "As built and measured".

- **Shelves** (the course's top shelf after 30 s, tallest puddle joined to the shelf; Stickiness 0): Wetting 0 gives
  7.1 px at 6 px and 7.9 px at 3 px (C2's 7.0 and 7.9); 0.9 gives 5.3 and 6.2; **1 gives 6.6 and 5.2**, the only
  Wetting under both marks. Nothing hangs from the box's lid at 30 s.
- **Trapped air** (the pool shot, a 70 px pour plunging for 3 s): the most air drops are 1,191 at 6 px, 11,523 at
  3 px and 22,418 at 2 px; at 6 s, 188, 4,027 and 11,626. The pool sloshes for seconds after the pour and its
  breaking waves keep trapping air, so the plan's mark (trapped room under a tenth of its most 1.5 s after the pour)
  is missed on this shot, as it is with air off; the pockets' mean with air on and off is 36,599 and 33,641 px² at
  6 px, 79,924 and 60,507 at 3 px, 87,117 and 110,983 at 2 px (the air's own room counts as a pocket: air is not
  drawn). Cavities wider than about twice `air_reach` (48 px) stay empty, with air only round their rims.
- **Pace** (wall seconds a scene second, physics alone): air costs 8 % on the pool shot at 6 px (0.450 against 0.417)
  and 2 % at 2 px (2.905 against 2.845); course and weird 0 to 2 %.
- **Wetting** (blobs after 3 s): monotone at both details (a 24 px blob's edge 92°, 84°, 70°, 47°, 6° at Wetting 0
  to 1 at 6 px; 89°, 79°, 73°, 61°, 51° at 3 px). **Wetting is weaker at 3 px than at 6 px**: at Wetting 1 a 24 px
  blob is a film at 6 px but a low cap at 3 px, and the tilted plate keeps 60 % of its water as a film at 6 px but 3 %
  at 3 px. Wetting 0 beads stand at 113° (12 px) and 90° (24 px): bigger blobs sag, as the skin of this water holds
  about 20 px up. Handed on (part 3's hand-over).
- **The course at Wetting 1** drains its top pool further than C2's at Wetting 0 (left in it at 30 s: 5.3 % and 5.7 %
  at 6 and 3 px, C2 7.5 % and 7.3 %), so a little more goes through the 24 px opening (57.2 % and 55.9 %, C2 52.0 %
  and 51.1 %) and less through the 12 px one (32.8 % and 32.3 %, C2 34.8 % and 38.2 %): within the plan's 5 points
  but for the 12 px opening at 3 px (5.9).
- **Unchanged**: the deep pool at rest (99.7 % of its height, rms 1.5 to 3.7 px/s), the slosh, Water Weird's landing
  (one body), and every run repeats (twice, and from a copy).

### Tests

- `bpe-water/tests/air.rs`: air rises and bursts; a stronger skin holds a bubble longer; a hole in a pool fills with
  air (the liquid kept every frame); open water gets none; off leaves the hole; repeats (twice, from a copy with
  births in flight); a pour into a pool loses its air; **a boundary lets go of no air**; **the liquid is counted
  without the air**; **air held deep under a bell stays**.
- `bpe-water/tests/wetting.rs`: a blob spreads on a wet floor and stands up on a waxy one; Wetting reaches only water
  near a face (bit for bit); air on a wet floor bursts as on a dry one.
- `bpe-water/tests/engine.rs`: the heap test counts the liquid's drops (a bursting heap traps a little air).
- `bpe-kit` `things::water::draft` tests: Trapped air, Bubbles rise and the Rock's Wetting restart the draft; the field
  carries the Rock's Wetting.
- Measuring runs (`#[ignore]`, `tests/shots.rs`): `air`, `wetting`, and `shelves` (with its "hanging above" column).


## Part 3: pieces and the pen (as built)

Plan: `docs/superpowers/plans/water-mode/part3-pieces-and-the-pen.md` ("As built and measured" there has every
number). A course is now carved rock with ready-made **Pieces** and pen-drawn **Walls** laid over it, each with its
own Material (Wetting and colour), and the editor places, draws, picks, moves, turns, sizes and moulds them.

### Wetting at every detail (`bpe-water numbers.rs` `wet_bend`, `finish.wgsl`; hand-over item 0)

- 2c's "Wetting is weaker at 3 px" was two things. Under a tenth of gravity (`scenes::cap`, where the edge shows the
  true angle) the angle was the same at 6 and 3 px (117°, 100°, 71° at Wetting 0, 0.5, 1 at 6 px; 115°, 99°, 74° at
  3 px): the 6 px "film" at full gravity was a one-drop-thick layer, the coarseness of a 6 px draft. But no Wetting
  ever made a film: a wetted face was liquid to the skin's pull and to the way the liquid lies, not to the **bend**,
  and the bend held a soaked floor's edge up at about 72°.
- **`Numbers::wet_bend`** (1): a wetted face's resting lattice counts in the skin's bend as neighbours with no
  surface of their own, `wet × thin × 4 × seg(q)` per face (`seg` the lens of the reach circle past the face). Under
  a tenth of gravity a cap's edge now follows Young's law (cos θ = 2 Wetting − 1) from Wetting 0.5 on: 86°, 55°, 2° at
  Wetting 0.5, 0.75, 1 at 6 px, 85°, 63°, 5° at 3 px, 82°, 55°, 6° at 2 px. Below 0.5 it can't reach: Wetting 0.25 stands
  at about 100° (Young 120°) and Wetting 0 at 111 to 117° as before (its bend term is 0, so Wetting 0 is 2c's water
  to the bit). At full gravity a 24 px blob at Wetting 1 is a film at every detail (`a_soaked_floor_spreads_a_blob_
  into_a_film_at_every_detail`).

### Solids on the course (`bpe-water solid.rs`: `Part`, `add_part`, owners, square-ended lines)

- `Solid::Polyline { points, thickness, square }` is a pen wall (square ends run a half thickness past the end points
  and stop flat). `Solid::bounds` and `is_sound` (a polygon of three finite points, a circle of a radius, a line of
  two points): an unsound solid adds nothing anywhere.
- **`Part { solids, cut, wetting }`**, laid over the field by **`add_part(part, owner, least)`** in order: the cut
  first takes the solid away (`max(d, −cut)`), then the solids add (`min`). A node's owner (`SolidField.owner`, one
  `u16` per node, 0 the Rock) and Wetting become the part's where its face is now the nearest: inside what it adds,
  in open water nearer it, and in solid within `LINING` (2 cells) of a face it cut, so a cut is lined with the
  cutting thing's Material. Only nodes within `BAND` (48 px) of the part's box change, so a huge or far piece costs
  only its box clipped to the layout.

### Pieces and Walls as the water sees them (`bpe-kit modules/water/pieces.rs`, `things/water/draft/course.rs`)

- **`PieceShape`** gains **Thickness** (a hollow piece's walls, a wheel's paddles, a sieve's bars; 16 px) and
  **Opening** (a funnel's gap, a sieve's gaps; 40 px). Each kind is `parts()` in its own frame (its box `Size`
  centred on its place, Flip mirrors x, the thing's Rotation turns it clockwise): Ramp, Step, Pillar, Gate, Float
  (rounded corners) and Custom (its Outline, shape units ±1 times half its Size) are filled outlines; Basin, Funnel,
  Pipe, Sieve, Bucket (round-ended rim) and Wheel (a rim, 8 paddles and a hub) are walls `Thickness` thick. Its
  **cut** (`cut()`) is what Cuts takes away: a filled piece's own shape, a hollow piece's inside (a pipe's bore, a
  basin's or bucket's bowl, a funnel's mouth, a sieve's gaps, a wheel's disc), and its walls then add as usual, so a
  cutting pipe is a tunnel lined with pipe and nothing opens beside its walls. Its **hull** (its outline) is only for
  snapping corners. Walls are at most half the box's smaller side thick (a sieve's bars half its width, so a short
  wide sieve keeps its bars). A Wall's Line is a polyline through its points (Curved: a
  centripetal Catmull-Rom curve), Round or Square ends; with Cuts it is a channel. The palette's sizes are
  `default_size` (a 300 x 150 ramp, a 60 x 300 pillar, a 400 x 80 pipe, ...).
- **`SolidSetup`** (one per enabled Piece and Wall, in `scene.order`) is its place, turn and cards as resolved;
  `WaterSetup.solids` holds them. `field_with(mask)` is the carved rock (the Rock's Wetting) with each laid over it
  as owner `k + 1`, thin walls at least `LEAST_WALL` (1.5) cells thick. `solid_at(p, reach)` picks the top-most body
  under a point; `snap_points(except)` lists every other solid's hull corners and wall points.
- **What restarts the draft**: a piece's or wall's place, turn, every Form setting, its Material's Wetting, turning
  it on or off, and the tree's order. Its colour and its Moves card do not. A tap whose spout is inside a piece says
  so (`IN_ROCK`, now "inside a solid") and pours nothing.
- **Notes** when picked: `THIN` (a part under two draft cells thick: the draft thickens it, so the final will be a
  little different) and `MOVES_LATER` (Moves not Fixed: moving pieces are part 5's; it stays put).
- **Not yet**: air under pieces is the course's, as under rock (no diving bell: part 5); Keyframed and Driven pieces
  stay at their place.

### Drawn in their colours (`bpe-water draw/plain.rs`, `shaders/plain.wgsl`)

The plain look's field buffer holds the distances, then the owners. `PlainLook.solids` gives the first
`MOST_COLOURED` (64) owners their Material's colour; the rock colour at a pixel mixes the owners' colours of the
four nodes around it that are inside a solid (open water nodes are owned by the nearest solid too, so counting them
bled a solid's colour across a touching one), and a field with no parts draws exactly as before (the pinned
pictures did not move).

### The editor (`bpe-studio water_mode/{piece_tool,pen_tool,solid_edit}.rs`, `stage.rs`, `picture.rs`)

- **Piece**: the strip has the palette (each kind's own outline, named for kittest), Turn (°), Flip and Cuts; a ghost
  follows the pointer; a click places one (its palette size, named after its kind), picks it and keeps the tool; one
  undo step each. **Make custom** (strip, a picked Ramp, Step, Pillar, Basin, Bucket, Gate or Float) turns it into a
  Custom piece of its own outline, one undo step.
- **Pen**: Thickness, Curved, Round | Square, Cuts. Clicks add points with a line on to the pointer; a double-click
  or Enter finishes a Wall of two points or more (its place the points' middle, one undo step); Escape or another
  tool drops the line and leaves no step. Enter and Escape are left alone while a text box has the keys.
- **Snapping** (Alt turns it all off): a placed or dragged piece moves so its hull corner nearest another solid's
  corner meets it, within 10 screen points (`SNAP_REACH`); a pen point or a dragged point lands on another solid's
  corner (a pen point on the line's first point too). This works whether or not the grid snaps; when it snaps and no
  corner is near, a piece's place lands on the grid, and a dragged wall's first point does (so all its points stay
  on the grid).
- **Select**: a piece or wall is picked by its marker or its body (4 screen points) and dragged with snapping. A picked
  one shows its outline; a wall or Custom piece its points (drag one; double-click its line to add one, a point to
  remove it, keeping two on a wall and three on a piece); a piece a turn dot above it (15° steps, Alt free) and a size
  dot at its box's corner (sized in its own frame, on the grid's spacing).
- **The preview**: while a carve stroke is painted or a piece or wall moved or moulded, the draft's restart waits for
  the drag to rest (as any drag) and the stage's picture shows the course as it will be
  (`WaterSetup::field_with(stroke)` or `field()`, made again only when the stroke or the scene changes) over the water
  still playing. `picture::Rock` carries any `SolidField` as the preview. After a piece's drag or mould the preview
  stays up until the draft plays the new course (so the picture doesn't jump back for the moment the restart waits).

### Measured (in the container; the plan's "As built and measured" has the tables)

- Caps under a tenth of gravity at Wetting 0.5, 0.75, 1: 86°, 55°, 2° at 6 px; 85°, 63°, 5° at 3 px; 82°, 55°, 6° at 2 px
  (Young 90°, 60°, 0°). The edge-angle test takes the mean over three grids: at 0.75 the 6 px cap stays a little
  wider (on the owner's card 50° against 62° on one grid), so the details agree within 8° at 0.5 and 12° at 0.75.
- The course's shelves at 6 px with the fix: 2.1 % left, tallest puddle 5.1 px at 30 s (5.4 px before), nothing
  hanging: `DEFAULT_WETTING` stays 1. Openings at 30 s: 35.6 % through 12 px and 60.8 % through 24 px (35.8 % and
  60.6 % before), so the fix doesn't hold water in narrow gaps.
- Forty pieces over a carved screen at 6 px: 3.2 ms for the carving, 17.7 ms for the pieces (release). The stage's
  preview builds it again for each change while a solid is dragged.

### Tests

- `bpe-water/tests/wetting.rs`: **the edge angle is the same at every detail** (Wetting 0.5 and 0.75, three grids
  each), **a soaked floor spreads a blob into a film at every detail**. `src/solid.rs`: a polyline is round or square at
  its ends, a part adds its solid and its Wetting, a cutting part opens the solid and lines it, a part touches only its
  band, a part with nothing in it changes nothing. `tests/plain.rs`: **each solid is drawn in its colour** (and a
  field with no parts as before).
- `bpe-kit` `modules::water::pieces`: every piece fills its box, the hull holds the parts, Flip mirrors and Rotation
  turns a piece about its place, a curved wall passes through its points, Make custom keeps the outline, broken shapes
  add nothing, the Shape card shows Thickness and Opening only where they count. `things::water::draft`: what restarts
  the draft (flipped for pieces and walls; colour and Moves do not), a piece adds solid and its own Wetting, a cutting
  pipe is a lined tunnel, the tree's order decides what a cut takes, a tap inside a piece pours nothing, a huge piece
  is kept to the layout, THIN and MOVES_LATER, picking by body, snap points, colours in the plain look, forty pieces
  build quickly.
- `bpe-studio` `water_mode::solid_tests`: the palette places each kind at its size; snapping to a corner (not with
  Alt; the grid otherwise); Turn, Flip and Cuts; Make custom (one undo step; none for a funnel); three clicks and Enter
  make a wall; a double-click finishes one; Escape and another tool drop the line; one point makes no wall; a pen
  point snaps; a click on a body picks; a drag snaps and is one undo step; a point drag moves only that point;
  double-clicks add and remove points (two kept on a wall, three on a piece); the turn dot's 15° steps; the size dot
  in the piece's frame; a thin wall says so. `app::water_tests`: a piece edit restarts the draft and a colour or Moves
  edit does not. The rail test: only Camera path is greyed.
- Full-size look check: `LOOK_ONLY=water-pieces` (`water-pieces-c8`, `-c3`).
- From the review: a cutting hollow piece leaves no room beside its walls, a sieve's bars take their thickness
  (kit); a solid's colour doesn't bleed onto one it touches (plain); corners snap with the grid's snapping off, a
  dragged wall keeps its points on the grid (editor).

### Known, left for later

- `LINING` is 2 cells, so a cut's lining is 16 px deep at an 8 px draft and 6 px at 3 px: the lining looks a
  little thicker in the draft.
- Adding a point to a Curved wall measures the click against the straight lines between its points, not the curve.
- Make custom on a cutting Basin or Bucket changes what it cuts: the Custom piece is its walls' one outline, so it
  cuts only where its walls are and leaves its inside as it was.
- A Custom piece is one filled outline, so the kinds made of several parts (Funnel, Pipe, Sieve, Wheel) have no Make
  custom yet.
- A very large Sieve (thousands of bars) is slow to build; the bars are not merged.
- The preview builds every part again for each change while dragging (about 20 ms for forty pieces at 6 px); only
  the moved part's band would need it.

## Part 8: Make final and export, with slow motion (as built)

Plan: `docs/superpowers/plans/water-mode/part8-make-final-and-export.md` (its decisions 1 to 22 and "As built and
measured"). A Water scene's water is now worked out once at **Final detail** with **Make final** into a **take** on
the disk, and its video is drawn from the take, from the editor and the command line. **Speed** on the Water card
plays it in slow motion.

### Restart points and moments inside a frame (`bpe-water drops/{state,capture}.rs`, `shaders/pack.wgsl`)

- **`WaterState`** (`Water::state`, `Water::from_state`, `to_bytes`/`from_bytes`, magic `BPESTAT1`) is exactly what
  `Water::copy` carries: the drops' three buffers, the deep pressure, walls and depth share, the clock's host side
  with every report read back, the pours and their progress, the counters. A body built from it plays on to the bit
  as the copy and the original do (`tests/take.rs`).
- **`Water::frame_keeping(keep, &mut Capture)`** keeps `keep` moments of a frame (1, 2, 4 or 8): the frame's end and
  `keep − 1` cuts after whole steps (`cuts(n, keep)`, every 16, 8 or 4 steps of 32). At a cut the step ends with
  `cs_finish` and the next starts with `cs_predict` (the plain order), which changes no drop: mixed keep counts play
  the same water to the bit.
- **`cs_pack`** (binding 29) writes 8 bytes a drop on the card (its cell and kind's low byte, its place in the cell to
  1/256 and its speed as a signed log byte, `round(15 ln(1 + |v|/4))`); `Capture` reads two frames' slots in turn, so
  the card steps the next frame while the host packs.

### Takes (`bpe-water take/`, `bpe-kit things/water/{finals,make}.rs`)

- **Frame codec** (`FrameCodec`, magic `BPF1`): a counting sort by cell, then planes (counts, x, y, speed x, speed y,
  kind) compressed with lz4. A drop is 5 bytes before lz4; decoded drops are back in their cells, `| STOPS`.
- **A take is a folder** in the finals folder named by its print (16 hex digits): `head.ron` (`TakeHead`: format,
  engine print, program, card, Final cell and layout, `WaterInputs`, chunks with the moments each frame kept,
  restart points, drops, time a frame), `chunk-000012.bin` (water second 12, `BPECHNK1` with an index at its end),
  `point-000240.bin` (a restart point every 120 frames, lz4 of `WaterState`), `make.lock` while a make writes (stale
  after 2 minutes). Every file is written as `.part` and renamed, so a crash keeps every whole chunk.
- **The finals folder** (`Finals::default_dir`): `BPE_FINALS`, else `%LOCALAPPDATA%\Ball Studio\Finals` on Windows,
  else `$XDG_CACHE_HOME/ball-studio/finals` or `~/.cache/ball-studio/finals`. `clear`, `clear_all`, `sweep(30)` (the
  editor sweeps at start, on a thread), `size`.
- **Finding a take** (`Finals::find`): this print's own take, else the take of the scene as it was (same cell, card,
  engine) right the longest, if that reaches past its first restart point: its frames are right up to the
  **earliest change** (`WaterInputs::earliest_change`: each part the physics reads with the time it can first change
  the water: 0, a tap's Starts less three frames, a keyframed setting from the key before the first that differs;
  taps by their place in the list). A make then starts a new take from the restart point at or before it, its
  chunks and points before that hard-linked (copied where the disk can't link).
- **`MakeFinal`** (`of`, `print`, `status`, `make`): refuses a make the disk has no room for (its estimate plus
  2 GB), holds at most 8 million drops (`FINAL_MOST`), packs and writes on its own thread (rayon encodes a
  second's frames side by side), and stops at the next frame when asked, keeping every whole chunk.

### Final detail and slow motion (`bpe-kit things/water/draft/{inputs,slow}.rs`, `modules/water/scene.rs`)

- `WaterSetup::final_of` is the scene at its Final detail (Coarse 3, Normal 2, Fine 1.5 px); the draft's print never
  hashes Final detail and the take's print never hashes Speed.
- **Speed** (Water card, group Slow motion, 0.125 to 4, keyframed): the timeline stays the water's time, so every
  keyframe and pour time stays where it is; a water second at Speed `s` plays for `1/s` video seconds (`Speeds`:
  `video_time`, `water_time`, `video_length`, exact for one Speed, a table every 1/480 s when keyed). A frame keeps
  the smallest of 1, 2, 4, 8 moments at least `0.8 / s` (the least Speed at its start, middle and end).
- The plain drawing moves each drop on by its speed (`Plain::draw_at(.., ahead)`, `view.w`), so a moment drawn a
  little after the one kept is in its place.

### Export and Preview final frame (`bpe-cli water.rs`, `lib.rs`, `main.rs`)

- `bpe_cli::water::export` draws the camera's 9:16 shot at each video frame's water time from the take
  (`TakeReader::near`: the kept moment nearest, and how far past it) in the plain look, with a silent track
  (`WaterSource::Final`; `MakeFinal` makes a missing final first; `Draft` plays the draft). `export_video` sends a
  Water scene there; a video holding a Water scene is still refused (part 10).
- The command line makes a missing final itself (`--finals DIR` picks the folder, "making the final: N %"), or with
  `--draft` exports the draft.
- `water::final_frame`: the take's moment at the playhead, or `FRAME_NOT_MADE` in plain words.

### The editor (`bpe-studio water_mode/{final_job,final_view,final_ui}.rs`, toolbar, timeline, export window)

- **`WaterFinal`** (`Studio.water_final`): what the scene's final is (`Found`: the make, its `Status`, the water and
  video lengths, the drops estimate, the slow stretches), found again at most every 250 ms while the scene changes and
  in the finals folder only when what it needs changes (the print, the frames, each frame's moments).
- **`FinalJob`**: the make on a "water final" thread at low priority with its own `headless_device`; Stop making
  keeps the whole chunks.
- **Toolbar** (Water scenes): **Draft | Final**, **Make final** (greyed once made) or **Stop making**, and a
  **Finals** menu (Clear this scene's final, Clear every final (its size), Show the finals folder, Preview final
  frame).
- **Final view** (`FinalPicture`): a worker opens the take, reads and unpacks the newest moment asked for and lays
  the course out at the Final cell; the window's thread hands the drops to the card and draws them moved on to the
  playhead. Not made: the draft shows, with "Not made yet: showing the draft". Out of date: the old final, with
  "Out of date: Make final remakes from N s".
- **Final lane** (above Hits, named "Final lane, …"): filled where made, hatched where out of date, fainter where it
  is being made, a line along its foot where slow motion keeps more moments; a click or drag scrubs.
- **Status line**: "· Final: 1.4M drops, 10 s of 10 made, 6.2 GB", "· Making the final: 40 % · about 3:00 min left",
  or "· Final: about 1.4M drops, about 7.0 GB (estimate)", with "· video 34.2 s in slow motion" when slowed.
- **Playhead**: in a Water scene it moves at the Speed (`Studio::water_speed`).
- **Export window**: a Water scene shows its FINAL rows, its video length with slow motion, and **Export** when the
  final covers the video, else **Make final and export** (with the time and room it expects; a progress bar while it
  makes, then the export reads the take) and **Export the draft**.
