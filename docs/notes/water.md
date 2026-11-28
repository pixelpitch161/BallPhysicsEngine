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
  restart points, drops, time a frame; the engine print hashes bpe-water's sources and the kit's water
  sources, which each crate's `build.rs` prints, and the take's print holds the step numbers, so another build's
  take is made again), `chunk-000012.bin` (water second 12, `BPECHNK1` with an index at its end),
  `point-000240.bin` (a restart point every 120 frames, lz4 of `WaterState`), `make.lock` while a make writes (written
  again every 20 s, stale after 2 minutes). Every file is written as `.part` and renamed, so a crash keeps every whole chunk.
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
  frame's moments side by side), and stops at the next frame when asked, keeping every whole chunk.

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
- **A make and edits**: `FinalJob` carries its take's print. After an edit the lane says "the scene as it was being
  made" and the status line "· Making the final of the scene as it was: N %"; Make final and export then refuses to
  export it ("the scene changed while it was being made"). Opening another scene stops the make; a stopped make says
  "Stopped making the final. What was made is kept."

### Measured (RX 9070 XT)

`water_final_speed` (`bpe-kit tests/water_final.rs`, run by the card job's timing step): one screen two thirds full
with a tap at Normal final detail (2 px), 10 s, 1.4 million drops: **10.0 wall seconds and 267 MB a scene second**
(2.67 GB in all, 3.15 bytes a drop a moment, a restart point 47 MB), the C2 estimate. A frame reads and unpacks in
31 ms.

## Part 6a: light and the look drawing (as built)

Plan: `docs/superpowers/plans/water-mode/part6-light-and-looks.md` (decisions 1 to 31 and "As built"). A Water
scene is now drawn in its own look everywhere (the stage, the Final view, Make final's export and Preview final
frame): lamps with soft shadows, light through the water and its caustics, glow and bounce, each Material, the Back,
and each liquid's Look card, finished by the same bloom, exposure and composite as a Balls frame.

### The cards and the take's print (`bpe-kit modules/water/look.rs`, `things/water/look.rs`, both `build.rs`)

- The Look, Light, Back and Lamp cards live in `modules/water/look.rs`; `plain_look` and **`look_of(scene, resolved,
  time) -> Look`** in `things/water/look.rs`. The kit's `build.rs` prints every file under its roots but those named
  `look.rs` or `looks.rs`, and bpe-water's prints `src/drops`, `src/take`, `src/drop.rs`, `src/solid.rs` and
  `src/lib.rs` only (and, since 6b, the wet marks a take keeps: `src/draw/wet.rs`, `src/shaders/wet.wgsl`), so a
  change to how water looks never makes a final again.
- `look_of` reads each enabled Liquid's Look (scene order, at most 16; a gradient as 64 linear samples, sent only when
  the colour is read from it), the Course's Rock and each enabled Piece's and Wall's Material (in the order the solid
  field numbers them), the Back, the Light card, every enabled Lamp (at its `transform.position`; Water things have no
  place keyframes yet) and the scene's Bloom and Exposure, all at water time. It reads no file.
- The Light card's **Ambient defaults to 1**, so a new scene is evenly lit as the plain look was.

### The look drawing (`bpe-water draw/look/`, `shaders/look.wgsl`)

- **`LookDraw`** owns a `Plain` (its field buffer, depth ladder and uniform) and a `bpe_gfx::Renderer` for the
  finish; `LookGpu` holds its pipelines (one module, `plain.wgsl` then `look.wgsl`). API as `Plain`'s: `draw`,
  `draw_at(.., ahead)`, `draw_many`, `set_field`, `renew(field, rest)` (another body, keeping the pictures and the
  renderer), `output()` (the renderer's `Rgba8Unorm` output), `read()`, `notes()` ("The back's picture can't be
  read", "Only 16 lamps light a Water scene"), and `light_at(p)` for tests.
- **Passes**, each a full-picture or splat render pass: look splats into `A0 = (k|v|, k|v|², k_air, k)`, `A1 = (k vx,
  k vy, k_lone, 0)` and the liquids' weights (`W0`, `W1..W3` only past 4 liquids; speeds in 1000 px/s), each liquid's
  radius from Smoothing and stretched by Streaks; plain's ladder; node fields D (extinction), G (glow) and C (colour)
  with 6 mips; the **light map** (`fs_light`, three targets: light and openness, mean lamp way and weight, and the
  light with no liquid in the way, which lights paint and the faces in front) on a world grid fixed to the course
  (8, 4, 2 px for Draft, Good, Best, at most 2048 a side); the **spread light** (glow plus Bounce of the lit faces
  and body) over the whole course at 4 times the light texel, **cone traced** (24 cones, 48 steps, from the
  emission's mips, stopped by the solid field, Glass letting 70 % through); then `fs_look` writes linear light into
  `Renderer::hdr_target()` and `Renderer::finish_picture` finishes it.
- **Lamps**: Spot (cone of Spread, edge from Softness), Strip (the nearest point of a line of its Length across its
  Aim, shining to the Aim's side), Sun (marched against its Aim to the course's edge, so a closed course's rock
  ceiling stops it unless Shadows is off). Falloff `600 / (600 + d)`. Shadows are a soft sphere-traced march of the
  solid field; Glass tints; liquid dims by its extinction; the first surface crossed focuses the light into
  **caustics** by its curvature (level-1 amount plus Ripples), `mix(1, C, Caustics)` clamped to 0 to 8.
- **Caustics in detail**: the curvature is read where the march first comes out of the liquid at about 12 px
  (`surface_level`, the crossing bisected), at the level a deep point's blurred view spans (depth times `0.02 + 0.2 ×
  Softness`, at least 12 px, mixed between levels), plus the Ripples' curvature blurred per wave; each light texel
  takes the focusing's average over its own span (`focus_sum`), so a focus line thinner than a texel neither beads nor
  vanishes. A march that starts in a bubble deep down is lit through the surface above it (`been_in`). A soft shadow's
  edge reaches at most 44 px (`SHADOW_REACH`), where the field stops saying distances, and glass casts none.
- **Solids lit at their face**: each solid pixel takes the light just outside its nearest face, falling off as
  `exp(−(depth in / spill)²)` (spill twice Depth, at least 16 px), so the rim is lit and the light is gone before the
  lines where the nearest face changes would show.
- **Pitfall: a level worked out per pixel.** The cloud's software card (lavapipe) reads `textureDimensions(t, k)` and
  `textureLoad(t, .., k)` with a per-pixel `k` as one level for a block of pixels, so `look.wgsl` works out a level's
  size from level 0's (`size_at`) and reads across levels as a loop over the level numbers (`level_share`).
- **Marches** take at most 320 steps, through open air half a level-4 texel at a time (a whole one where level 5 is
  empty too), so a lamp several screens away still shadows at Final detail. The shot's light map reaches past the
  view as far as a solid's face light and the bent back are read (`light_reach`), so the picture's edge is lit as the
  stage's whole-course picture is.
- **The Back's Picture** is the PNG the scene keeps under the card's name (the inspector's "…" keeps it and names it on
  the Back card), or else the name read as a file's path. Paper's cut-out shadow uses the largest Paper Depth, and the
  caustics' ripples the largest Ripples, of the scene.
- **The surface rounding** (Shine, Edge sheen, Ripples' tilt) takes how far inside the outline a pixel is from the
  amount's slope within a splat of the outline, and from the smooth surface past two splats: deep in a body the
  amount's slope between the drops would make a grain under bright light.
- **Nothing remembered**: no float atomics, no frame-to-frame state, noise hashed from the world point, so a moment
  draws the same picture every time and the light at a world point doesn't depend on the view.
- **Drawn** back to front: the back (Darker rock, Colour, Gradient, Picture cover-fitted, with a soft contact shadow
  near solids and paper's cut-out shadow), the liquid (see-through over the back bent by Bend in world px along the
  smoothed surface's slope, Rainbow split, Deep colour, Solid paint lit from the front, Shine, Edge sheen, Thin film,
  Bands, Glow, Outline), shafts, then the solids lit at their face (Rock, Glass frosted over the back, Paper, Basalt,
  Flat; Depth's bevel, Roughness's highlight, Outline).
- Tests: `bpe-water tests/look.rs` (13, on lavapipe at 270 x 480; `LOOK_DEBUG_DIR` writes their pictures); the
  plain drawing's own tests are unchanged.

### The three places (`bpe-studio water_mode/{draft,picture,final_view,stage}.rs`, `bpe-cli water.rs`)

- The draft's `Live.look: LookDraw` is kept from body to body (`renew`); the stage draws `look_of` at the playhead,
  with **Draft light while a slider or drag is held** (`studio.hold_runs`), and shows the drawing's notes in a pill.
  The Final view and the export draw the same way; the export reads the renderer's output.
- **Lamp markers**: a Spot's cone (120 points long), a Strip's line with a tick toward its Aim, a Sun's three arrows
  down from the course's top edge.

## Part 6b: extras and wet marks (as built)

The plan's decisions 32 to 37 (`docs/superpowers/plans/water-mode/part6-light-and-looks.md`, its "As built / 6b").
Every extra is a function of the drops, the cards and the water time, never remembered: the same moment draws the
same extras. Bubbles last is fizz's life and Sparks last a spark's trail in seconds of its flight. Wet marks are
the one part of the look with a memory, so the water keeps them.

### Foam, bubbles and fizz (`bpe-water draw/look/`, `shaders/look.wgsl`)

- **A motion node field**, a fourth beside density, glow and colour: `(Σk vx, Σk vy, Σk|v|², Σk)` (speeds in
  thousands of px/s), with mips (`fs_look_nodes`, `fs_half`). The nodes pass writes four rgba16f targets, which is
  32 bytes a sample, the default limit. `churn_at(p, size)` reads the mean speed and the spread of the speeds
  (`sqrt(E|v|² − |Ev|²)`) over `size` world px, through the level shares.
- **Foam** shows where:
  - the speeds spread by 60 to 260 px/s over 30 px (starting at no less than 7 % of the mean speed: the
    motion sums are half floats, and an even stream at 3000 px/s reads a spread of up to about 195);
  - trapped air's splats meet the liquid;
  - a front moves out through the surface at 300 to 900 px/s (within 16 px of it).

  It is weighted 0.35 deep down, rising to 1 over the top 40 px. Its pattern is the cell walls of a hashed grid of
  Foam size, clumped by a slower noise. It is moved by a flow map: two copies on a 1 s cycle, half a cycle apart,
  each pushed by the mean speed over 120 px and started again somewhere else. (The mean over 30 px tore it into
  streaks in churning water.) It covers where the pattern tops `1 − amount`, lit as the surface is.
- **Trapped air as bubbles**: where the air's splats (A0's third channel) reach the liquid's level, the hole is
  filled with the body. It is drawn as if deep in (flat, not bent), 15 % lighter, with a bright rim of Bubble
  colour on the outline (a tenth of Bubble size, at least 1 px). With Bubbles 0 the hole is drawn as before. The
  liquid block now also runs where only air splats reach.
- **Fizz**: a lattice of cells 3 Bubble sizes across that rises at 120 px/s with its bubbles.
  - In each life a spot is lit with chance 0.3 × Bubbles (`FIZZ_SHARE`; at Bubbles 0.8 every spot read as
    dots printed on the water), at a hashed place inside its cell, half to all of Bubble size. It lives
    Bubbles last, fading in over its first fifth and out over its last 30 %.
  - It shows only where the liquid is deeper than the bubble, not in a pocket, and where the water's mean
    speed over 30 px is under 250 px/s, gone by 600 (`FIZZ_STILL`: a still lattice rising through a fast
    stream read as printed on it).
  - It is drawn as a ring and a lighter inside, fading out when smaller than a pixel.

### Mist and sparks (`vs_extra`, `fs_extra`)

- **A sprite pass** over the drops after the compose, two sprites a drop. Blending is premultiplied: mist is
  drawn over, sparks are added (alpha 0). It is never drawn over a solid, and it runs only when a liquid has Mist
  or Sparks.
- **Lone and edge drops**: a drop is lone where the density field's level 2 holds under about 0.1 (a body at rest
  is 1, a lone drop about 1/64), and at an edge under about 0.5. An edge drop is **leaving** when it moves out
  along the way the liquid thins (the level's slope over 4 cells, within about 50°). Within 1.5 to 3 cells of a solid
  neither is drawn unless the drop is leaving the solid (moving out from its face, within about 60°): a thin sheet
  running along a wall is lone drops to the density, and drew a bright line down the wall.
- **Mist**: lone drops over 600 px/s (fading in over 200). Each draws a soft dot of Mist size and a haze 4 times as
  wide, lit by Ambient, the shot's light and the spread light at the drop.
- **Sparks**: lone drops, and edge drops leaving the body, over 900 px/s. (Every edge drop sparked at first,
  and a fast stream's drops move along its edges, so its edges were drawn as a bright outline.) Each draws a streak back along its speed, `Sparks last × speed`
  long (at most 160 px) and half Spark size wide (at least 0.75 px, dimmer when thinner). It is Spark colour × 3
  at its head, fading to the tail.

### Wet marks (`bpe-water draw/wet.rs`, `shaders/wet.wgsl`)

- **`WetField`** holds a `u32` a node: the water frame liquid first touched it (`DRY` is `u32::MAX`). `cs_wet`
  marks the four nodes round each liquid drop (not air) with `atomicMin`, so the drops' order doesn't matter. It has
  `upload`, `read` and `clear`. `HeldWet` puts marks read from a take on the card for a drawing, sending them again
  only when they change.
- **Drawn** (`LookDraw::set_wet(field, frame)`; marks on another layout draw none; the frame rides in `LU.time.w`,
  below 0 for none). A solid within 12 px of its face is drawn wet where the nodes half a cell outside the face are
  wet by the frame drawn. It is `Wet marks × 50 %` darker, its Roughness down by half the mark, fading over its
  last 6 px. (At first 6 px and 45 %, which hardly showed at full size.)
- **The draft** (`bpe-studio water_mode/draft.rs`): `Live.wet` is marked at a new body's start and after every
  frame the thread plays. Going back keeps it, as the stage reads it against the playhead's frame. A new body starts
  dry. The draft export marks its own (`bpe-cli water.rs`).
- **The final** (`bpe-kit things/water/make.rs`):
  - The make marks after every frame on its own card.
  - At each chunk's end, and at the last frame (whose end may be a chunk of its own), it reads the marks back. It
    sends them to the writer before any moment of the next chunk.
  - The writer writes `wet-NNNNNN.bin` before the chunk is whole and counts its bytes with the chunk's.
    `take::chunk::write_wet` writes a magic, then lz4 of a `u16` a node: frames since the take's start, 0xFFFF dry.
  - A remake from a restart point starts from the marks of the chunk before it, with every node first wet there or
    later made dry again (`dry_from`). `Finals::begin` shares the wet files along with the chunks.
  - `TAKE_FORMAT` is 2, so every older take is made again.
- **Reading** (`bpe-cli water.rs`, `TakeReader::wet`): the marks kept with the chunk of water time τ, read against
  frame ⌊60τ⌋. A moment chosen from the next frame's start reads one frame early. The Final view's worker reads the
  marks with the moment; the export and Preview final frame hand them to the drawing.

### Tests

- `bpe-water/tests/look.rs`:
  - 14: trapped air draws a bubble, or a hole with Bubbles 0;
  - 15: fizz rises inside still water, and never shows in the air or in a pool running fast;
  - 16: churning water foams and still water doesn't;
  - 17: fast lone drops draw mist and sparks, slow ones neither, and nor do a fast band's edges;
  - 18: every extra draws the same twice;
  - 19: wet marks darken the rock near its face, not with Wet marks 0, and not before the frame.
- `tests/wet.rs`: the nodes round a drop, a later frame never moves a mark, air marks none, the drops' order doesn't
  matter, upload.
- `take::chunk`: wet marks read back as written.
- `bpe-kit/tests/water_final.rs`: every chunk has its marks, they only grow, and a remake's marks equal those of a
  make in one go.
- `bpe-studio` draft tests: `the_draft_keeps_its_wet_marks_going_back`.

### Known, left for later

- A big air pocket's middle, where no liquid splat reaches, takes the first liquid's look (air drops carry no
  liquid).
- A cavity much wider than the physics' air reach (48 px) holds no air drops, so it stays a hole with Bubbles on
  (the full-size check's pool: air in every small pocket, none in the two big ones).
- Wet marks are on the course's grid, so a piece that moves would leave its marks where it was (pieces don't move
  yet).
- Mist and sparks are drawn over the liquid in front, and over each other in the drops' order.

## Part 6c: the ready-made looks and the Effects (as built)

The plan's decisions 38 to 42 and 44 (`docs/superpowers/plans/water-mode/part6-light-and-looks.md`, its "As built /
6c"). The Grade card, Grade mode, the Things masks for water (decision 43) and the Light only and Glow only takes
went to 6d.

### The looks table (`bpe-kit modules/water/looks.rs`)

- **`ReadyLook`**: Plain, Lamp-lit rock, Glass lab, Neon in the dark, Paint on paper, Molten and Cartoon
  (`ReadyLook::ALL`, in the chooser's and the menu's order). Each has a title, one line of words, a name for a new
  scene's Liquid and a swatch for its chooser tile. `row()` is its row of the table (`Row`):
  - the Liquid's Look and the solids' Material (one for the Course's Rock and every Piece and Wall);
  - the Back and the Light card;
  - its lamps (`LampRow`: a Lamp card, and where it goes as a share of the camera's shot from its middle);
  - Bloom and Exposure.

  Plain is the cards' defaults, so adding a look is adding a row.
- **`apply_look(scene, reg, look, keep)`** writes a row as one edit:
  - Each card it names is set to the card's defaults with the row's values on top, so one look after another never
    keeps what the first set. A missing card is added.
  - It never writes Wetting (physics: a look never changes the take) or Quality (`NEVER`). A setting with keyframes
    keeps them.
  - `Keep { colours, lamps }`: Keep my colours skips every Color and Gradient setting, and Colour from
    (`COLOUR_WAYS`: Neon and Molten colour theirs by speed, which replaced a one-colour liquid's colour with the
    default gradient). Keep my lamps leaves the lamps
    as they are; otherwise every Lamp goes, and the look's are added ("Lamp", or "Lamp 1", "Lamp 2"…) in the camera's
    shot at the start.
- **New scenes start in a look**: the Water card's Starting look setting is gone. `new_water_scene` is the bare
  scene, and `start_water_scene(reg, look, name)` adds a Liquid named for the look and applies it. The chooser's
  tiles and the editor's New Water scene use it.

### Use a look (`bpe-studio water_mode/stage.rs`)

- A menu button at the right end of the stage's strip (`LOOK_MENU_W`, so the tool's row never wraps for it): the
  seven looks (each with its words on hover), then the Keep my colours and Keep my lamps switches
  (`WaterStage.keep`). `use_look` applies the look, drops lamps that went from the selection, makes it its own undo
  step (`separate_edit`) and says "Using the … look.".
- The strip's height isn't changed while a button is held: picking a piece adds to the strip, and the course moving
  under a drag moved the piece with it.

### Effects on the shot (decision 42)

- **`FinishLook`** (`bpe-water draw/look/inputs.rs`) carries the effects and the grade beside Bloom and Exposure.
  `LookDraw` puts them in the frame it hands `Renderer::finish_picture`, which runs them as a Balls frame's.
- **`finish_of`** (`bpe-kit things/water/look.rs`) reads the root's Effects card with its keyframes at the water
  time (`modules::effects::draw::calls`, shared with `EffectsDraw`), keeping only calls at `Stage::Picture`, and the
  Grade card (`grade::stack(card, None)`; offered since 6d).
- **What a Water scene is offered** (`Builtin::water_ok`, `modules::effects::water_ok`): own code, and every
  built-in over the picture but Ball worlds, Fractal light, Gravity lens, Hue storm, Op-art lens, Shockwaves,
  Light ripples, Glitch and Breathing space, which are made of the balls, their hits, the beat or the things
  (Glitch waits for hits at its defaults, and Breathing space for beats or hits). The materials paint things, so the
  "Materials" part of "+ Add effect" is left out. An effect pasted in anyway draws as it can: the ball list is
  empty and no hits come.
- **Light only** changes nothing in a Water scene until 6d gives it the water's light, and its row says so.
- The effects' programs build at once on `LookDraw`'s own renderer (it shares no background store), so a new effect
  costs one short stall the first time.

### Whole course | Shot (decision 44)

- **`Seen`** (`WaterStage.seen`), switched in the toolbar beside Draft | Final (`final_ui::toolbar`). It is there,
  not in the strip, because the strip's first row got too narrow and wrapped the Select hint.
- **`CourseView::fit_shot(room, course, centre, shot)`**: the camera's shot at the playhead
  (`bpe_cli::water::shot_at`, shared with the export's `shot_view`) fitted into the room. `rect` is still the
  course's outline (larger than the room), so every tool maps the same way; `shot` holds the part shown,
  `seen_rect()` is it on screen, `holds()` takes clicks only there (every tool, Select too), and `clip(pad)` keeps
  the tools' drawing to it.
- **The pictures are drawn again when the part shown moves** (`picture::place_of` in both `Drawn` keys): in the
  Shot view a camera moved or zoomed gives a picture of the same size over another part of the world.
- **The picture** (`picture_view`) is drawn for the part shown (`seen()`), so the Shot view is drawn at the
  stage's own pixels over the shot. The camera's outline isn't drawn in it.
- **Whole course** draws with Bloom and Exposure but no Effects or Grade (`FinishLook::without_shot_finish`): they
  are made for the shot's shape.
- **Known, left for later**: an effect that remembers its frames (Feedback tunnel, Rainbow echoes, Thermal vision)
  starts fresh when the Shot view is moved to another moment, where the export has built it up from the start
  (Balls has `preview/warm.rs` for this). While the draft starts over, the stage keeps the look it last drew, so
  for that moment a switch to Whole course still shows the Effects.

### Pinned looks (`bpe-kit/tests/water_looks_pinned.rs`)

Part 3's pieces course (`sets::pieces(8.0)`, hand-laid drops) in a scene of two Liquids, in each of the seven looks
at water time 0.5 s, at 270 x 480. The tile averages are in `golden-water-looks.txt`, pinned to the owner's card, one
file for debug and release. "Refresh GPU pictures" makes it. On another card the test checks that each look draws
and that no two draw the same.

### Tests

- `bpe-kit`: `modules::water::looks` (5: each look writes its row over another look's, lamps in the shot; Keep my
  colours and Keep my lamps; never Wetting or Quality; a scene started in a look; the words), and
  `the_finish_reads_the_scenes_effects_over_the_picture_and_its_grade`.
- `bpe-water/tests/look.rs` 20: Mirror in the finish copies the water picture's left half onto its right.
- `bpe-gfx`: `a_water_scene_is_offered_the_effects_that_need_no_balls`.
- `bpe-studio`: `a_water_scene_adds_effects_but_only_the_ones_its_picture_can_feed`,
  `the_shot_view_fits_the_shot_and_maps_both_ways`, `the_shot_view_shows_the_finish_and_the_whole_course_none`,
  the toolbar's switch (`the_toolbar_switches_the_view_and_export_is_on`),
  `use_a_look_sets_the_scene_to_it_as_one_undo_step`.
- The full-size look check `water-looks` (`LOOK_ONLY=water-looks`): a demo course made once (a chamber, a pool a
  quarter full, a tap on a Ramp spilling into a Basin), drawn at 2 s in each look. `LOOK_SCENES=<folder>` saves
  each look's scene file, to render with `bpe-render-next --finals <LOOK_TAKES folder>`. It changed four rows
  (the plan's "As built / 6c"): Glass lab was washed out, Neon's still water was dark, Molten's pool blown out, and
  Paint's lamp shaded everything under a line of paint too hard.
- **Fixed in 6d** (6a's light, seen in that check): a Spot beside a falling stream drew a wide striped,
  stair-stepped caustic pattern on the rock behind, and a small drop in the air cast a dashed shadow.

## Part 6d: Grade mode on a Water scene, and two light fixes (as built)

The fourth of part 6's pull requests: decision 44's first half and the Grade card of decision 42. The Things masks
for water (decision 43) and the Light only and Glow only takes (decision 42's last sentence) are 6e.

### Two light fixes (`bpe-water shaders/look.wgsl`, `lamp_light`)

- **A lone drop's whole shadow**: the march through the air skipped a coarse texel (level 4, and level 5 round it)
  whose amount was under a thousandth. A lone drop is a few thousandths of a coarse texel, so on some rays it was
  stepped over and on others not, and its shadow came out dashed. Now only a texel with none in it is skipped. The
  shadow is deep near the drop and softens further down, as the coarser levels average the drop over more room.
- **No caustics behind a stream**: a point in the air was lit through the liquid's lens wherever its lamp's light
  had gone through liquid. Light that came out of the liquid again (through a falling stream, onto the rock behind)
  isn't focused by one surface, so the march drew the stream's two sides as one wide striped lens. Now the lens
  counts in full when the light went into the liquid within 16 world px of the point and fades out by
  `LENS_REACH` (32 px, a bubble's size; `t_in`, where the march first went in), so no line is drawn where it ends.
  Lamp-lit rock has its Caustics 1.2 back.

### The finish measures in the video's pixels (`FinishLook.video`)

- `finish_of` gives the finish the scene's Width and Height; `LookDraw` makes the frame it hands
  `finish_picture` the video's width wide and the picture's shape tall (`(0, 0)`: the picture's own, as Whole
  course and the tests use). Across, because the editor's shot is always 9:16 while the video may not be, and the
  masks' handles measure across too; the export's picture is the video's shape, so there it is the video's size.
  The Effects (`base.scale`) and the Grade (`GradeAt.scale`) measure in the video's pixels, as in a Balls frame, so
  the editor's Shot view (drawn at the stage's own pixels) and an export at a lower `--scale` look like the video
  scaled down. Before this the Shot view's blurs and masks were sized for its smaller picture.

### Grade mode (`bpe-studio water_mode/graded.rs`)

- **It opens on a Water scene** (the D key, the toolbar, "Open in Grade mode"): `keep_out_of_grade` and
  `NO_WATER_GRADE` are gone, and the scene settings show the Grade card's summary as a Balls scene's do.
- **The stage in Grade mode is the shot**: `water_mode::stage` hands over to `graded::stage`, which fits the
  camera's shot at the playhead into the room (`graded::shot_view`) and draws it with the same `stage::water` as
  the Shot view (the draft, or the final's moment in the Final view). No tool rail or strip: the course is built in
  Build mode. The toolbar's Whole course | Shot is greyed (`GRADES_THE_SHOT`).
- **`look_now`** shows the grade as Grade mode does (`GradeMode::show_on`: Before, Split, the overlay, a stroke
  being painted) and draws a hovered snapshot from its own copy of the scene (`preview::peeked`). A held press
  doesn't drop the light to Draft in Grade mode (`hold_runs` is any press for a Water scene): a grade edit never
  moves the water, and the scopes, clipping and picks must read the picture as the video shows it. While the draft
  starts over, the stage keeps the last look's liquids but takes the finish as it is now.
- **What Grade mode's tools read**: each picture keeps the texture it last drew in a `picture::Shown` (a clone of
  `LookDraw::output()`, its size, and a drawing number that differs for every drawing, the draft's and the
  final's). `Graded` keeps its own `Scopes` and `Clipping` and runs them on it when the drawing changes (or the
  scope or clipping view does); the eyedropper and the targeted tool read it back (`bpe_gfx::read_rgba8`, now
  public). The clipping picture's egui texture is let go when another scene opens (`Graded::free`, before the
  stage is reset). The Split line, the compare switch, the corner pill and the colour pick are the ball preview's
  (`preview::{compare_switch, split_line, corner_pill, pick_colour}`; `pick_colour` now takes a reader). The
  masks' handles and brush get screen points per video pixel, as in the ball preview.
- **No Things part in a Water scene yet** (`masks::offered`): Add mask and Add part left it out until 6e gave
  it the water's own kinds (6e put both menus back as they were).
- **Speed**: the zero thresholds cost nothing measurable on the owner's card (`water_final_speed`'s look a frame
  at 1080 x 1920 under two Spots, a tap's spray in it: 18.1 ms before, 17.6 ms after). Known: near many lone drops
  the march steps finely through each coarse texel holding one, so on a very tall course at Fine detail a ray
  through much spray could reach `MOST_STEPS` (320) before its lamp, and stop taking in the liquid beyond.

### Tests

- `bpe-water/tests/look.rs` 21 (a lone drop's shadow is deep under it and has no dashes), 22 (a Spot beside a
  stream lights the rock behind it the same with Caustics 2 as with none, and close behind it the lens fades out
  with no line), 23 (a Radial mask in the finish grades the same block at a quarter and at half size, and for a
  video of another shape). 21 and 22 fail on the old shader (22's close band on the lens cut off at 32 px), 23
  without the video's size.
- `bpe-kit`: `the_look_reads_every_card` (the finish's video size).
- `bpe-studio`: `grade_mode_shows_the_shot_with_its_grade_as_grade_mode_compares_it`,
  `a_water_scenes_masks_offer_no_things_part` (6e turned it round),
  `a_water_scenes_toolbar_says_so_and_grade_mode_grades_the_shot`,
  `a_water_scenes_settings_show_its_own_cards_and_none_of_the_balls` (the Grade card now shows),
  `a_water_scene_cannot_go_in_a_video`.

## Part 6e: Things masks on a Water scene, and the Light only take (as built)

The last of part 6 (decision 43, decision 42's takes). A Water scene's grade has Things parts again, choosing
what the water's drawing draws, and an effect on the Light only reads the water's light.

### Things parts (`bpe-kit modules/grade/masks.rs`)

- **Only** gains **Liquids**, **Solids** (the Course's rock and every Piece and Wall), **Back** and **Extras**
  (foam, bubbles, mist and sparks). A Water scene's Only offers those and **One thing** (a Liquid, the Course, a
  Piece or a Wall, or a tag or a kind of them); a Balls scene's offers Balls to Background and One thing as
  before (`only_offered`, used by Grade mode's settings rows). A Things part added in a Water scene starts on
  Liquids (`water_start`, in `add_mask` and `add_part`).
- **`ThingsPart::water_things(scene)`** names them from the scene by thing codes (`bpe_gfx::thing_code`, as a
  Balls part does from the world); Extras is `bpe_water::draw::look::EXTRAS` (`u32::MAX`); Back is Background's
  "every thing, turned over". `grade::stack_in(card, world, water)` passes the scene to `masks::area`;
  `finish_of` uses it. `masks::follow_note(.., water)` likewise, so a mask row's "Thing not found" looks in the
  scene, not in the blank Balls world a Water scene plays beside.
- **`FinishLook.things`** (`ThingCodes`) tells the drawing which code drew what: each liquid's by kind, the
  course's (owner 0) and each Piece's and Wall's (owner k from 1, in the scene's order, as the draft's field
  numbers them; `things_of` in `things/water/look.rs`).

### The drawing (`bpe-water draw/look`, `shaders/look.wgsl` section 8)

- The compose is now `composed()`, behind two entry points: `fs_look` (the colour alone, as before) and
  `fs_look_all`, which also writes the **picture before light** (each part's colour under a light of 1: the back's
  colour, the liquid's mixed with what shows through it, foam, bubbles and outline, the solid's, glass's frosted
  back) and **who is seen** (`Rgba32Uint`: the liquid's and the solid's cover, the extras' share of the liquid's,
  the two liquids seen most and the first's share, the solid's owner). The mist and sparks pass likewise has
  `fs_extra_all`, which also lays their cover over a picture (sparks by their brightness).
- **`fs_cover`** turns who is seen into one choice's coverage (`CoverU`: which liquid kinds, owners and the extras
  it chooses, or every thing), red the chosen as seen and alpha every thing but the back, the layout the Balls
  silhouettes use. `LookDraw` asks the renderer which choices the grade makes (`Renderer::selections`), runs
  `fs_look_all` and the coverage passes only when there are some or an effect takes the light, and hands them over
  in `GivenPicture { light, things }`.
- **`Silhouettes::flood_given`** (bpe-gfx) floods each given coverage into the things field as a Balls
  silhouette is (`Things::Given`), so Expand and Feather work in the video's pixels as in Balls.
- **Light only**: `GivenPicture.light` is the picture before light as both the unlit picture and the lit share,
  so the take reads the light as the picture over it (`bpe_light_of`: c = unlit · light) and puts what the effect
  made back the same way. Glow only needed nothing. The effect row's "Light only doesn't reach a Water scene's
  light yet" is gone.

### Tests

- `bpe-water/tests/look.rs` 24 (a Things mask grades only the Liquids, with Expand round them, the Solids, the
  Back, or one block), 25 (one on the Extras grades a spark's trail and not the back), 26 (an effect making the
  light 2 on the Light only keeps the back red and the pool blue, where on the whole picture it makes them white).
  All three fail without the coverage and the picture before light.
- `bpe-kit`: `a_water_scenes_things_parts_name_what_the_water_draws` (the codes in the draft's order, what each
  Only covers, a new part on Liquids reaching the finish), `the_look_reads_every_card` (the course's code).
- `bpe-studio`: `a_water_scenes_things_part_starts_on_liquids_and_offers_the_waters_things` (Grade mode's Only
  menu), `a_water_scenes_things_part_offers_the_waters_things`.

## Part 4.1: rain, drips, bursts, drains and fed pours (as built)

Plan `docs/superpowers/plans/water-mode/part4-more-liquids.md` (decisions 1 to 18). Rain, Drip and Burst pour, the
three Drain kinds take liquid away, and a tap can pour back what the drains took, so a course runs as long as the
video does.

### Lumps (`bpe-water lumps.rs`)

- **`Lumps`**: a stream of lumps, each a disc of drops on the resting lattice (half a cell apart) born at its own
  moment: lump `k` at `from + k × every` while before `until` (`every` 0 or not a number above 0: one at `from`).
  Rain's lumps sit across its width at right angles to its Aim, each turned by a share of its Spread and sized
  `size × (0.75 + 0.5 h)`; a drip's and a burst's are at the pour's place. `hash01(seed, k, j)` (splitmix64) gives
  every share, so the drops born in any stretch are worked out from the times alone (no state; splitting a stretch
  gives the same drops). A burst's drop at `d` from the middle sets off at `Speed × (way × (1 - s) + s × d / r)`,
  `s` = Spread / 180.
- The lattice is **offset by the lump's own share of a spacing** (`hash01(.., 3)`, `(.., 4)`): a disc at a random
  offset holds its area over the spacing squared on average whatever its size, so a fine drizzle pours its amount
  at every detail (this replaced the plan's floor rule for lumps under one drop). Drops within `clear` of the
  solids or past `EDGE_CLEAR` (2.1 cells) of the grid's edge are left out. `amount()` counts the area (past
  100,000 lumps, `count()` from the times alone times a lump's mean area, so heavy rain over a long video is
  counted whole).

### Timed births (`bpe-water drops/mod.rs`)

- **`Water::set_timed(&[Timed])`**: the next frame's drops born at their own moments (used once; at most
  `MOST_BIRTHS`). `births()` puts each into the step its moment falls in (one before the frame into step 0, one at
  or past its end into the last), after that step's tap rows, moved on to the step's end by `x + v t + g t² / 2`
  (`g` the course's gravity times its liquid's `Feel::gravity`; air has none). Their speeds count in `steps_now`.
  Not through `add()`, which primes the deep water.
- The step's number now rides in **bits 8 and up of `St.n.w`** (bits 0 and 1 stay the last and first step; the
  only readers, in deep.wgsl, mask them).

### Drains (`bpe-water drops/drains.rs`, `common.wgsl`, `finish.wgsl`, `report.wgsl`)

- **`Drain { place, reach, pull, edges, feeds }`** (`Drain::disc`, `Drain::edge`; edges LEFT 1, RIGHT 2, TOP 4,
  BOTTOM 8), at most `MOST_DRAINS` 32, `Water::set_drains` from the next frame. On the card: `Fr` gains, at its
  end, `drains` (middle and reach in grid units, chance a step `1 - exp(-pull dt)`, 1 when infinite, 0 when not a
  number or not above 0), `dedges` (four to an entry) and `dmeta` (count, the frame's code).
- **`finish_at(i, step)`**, before the neighbour loop, asks `drain_taking`: one share, `share_of(i, frame code,
  step)`, is rolled for the drop, and the first drain whose reach holds it (a disc's within its reach; an edge's
  within `EDGE + 0.8` cells of a named edge) and whose chance is above the share takes it. So where drains overlap
  the strongest decides whatever their order (a Leak over a pool doesn't slow the hole in it), and a Rate of 0 is
  to the bit the same as no drain, overlapping or not. Liquid is sent to `(-2e6 - k, -1e6 - code)` and dies; **air
  bursts** there by the same chance (to `(-1e6, -1e6)`) and is never counted. `cs_finish_predict` passes the step
  before its `st`'s, `cs_finish` its own.
- **`cs_drained`** (bindings 0, 1, 2, 18; its own bind group per report slot) runs after the frame's last step and
  before trapped air is born over the drops that left (air is born at the head of the dead drops and would hide
  them): it adds each dead drop whose place is this frame's code to `Report.drained[k]`. The frame code wraps at
  2^23 frames, so the place stays a whole number a float holds. No step pass gained a binding
  (`cs_finish_predict` is at 8).
- The clock keeps each frame's **feeding drains** (a bit mask, as the drains were when it ran) with its entry;
  `settle()` returns what they took with the drops to let go, and the body adds it to its **tank**.
  `Water::drained()` is what each drain took in the frame the last boundary read (two frames back);
  `Water::tank()` the tank.

### Fed taps (`bpe-water drop.rs`, `drops/mod.rs`)

- **`Pour.fed`**: such a tap pours only from the tank. Each frame (`fed_shares`), in the pours' order, it may pour
  `min(full, max(tank left / 6, min(a row, tank left)))` drops, `full` its unfed amount for the frame
  (`Pour::amount`), at most what the tank still holds; its stream is narrowed to that share (`width × share`, so it
  stays one stream), and its rows stop at its allowance. What the fed taps poured comes out of the tank. Liquid is
  kept to the drop round a loop (the test drains a pool into a tap above it for 10 s, then lets the tank run dry).

### Restart points (`bpe-water drops/state.rs`)

- `BPESTAT2`: each pour's `fed`, the timed drops still to come, the drains, the tank, the clock's entries with their
  feeding drains and the reports with `drained`, and the clock's last drained counts and tank feed. `BPESTAT1` no
  longer reads (no backward compatibility yet).

### The cards and the setup (`bpe-kit modules/water/places.rs`, `things/water/draft.rs`, `draft/inputs.rs`)

- **Pour card**: Rate for taps only; **Drops a second** and **Drop size** (rain and drips), **Burst size** and
  **Every** (bursts; Every 0 is once at Starts), Width for taps and rain, Spread for taps, rain and bursts, and
  **Fed by drains** (taps). Drops a second, Drop size, Burst size, Every, Starts and Stops are read at the start
  (a keyed rate would need the schedule's history); Width at the frame; Aim, Speed and Spread at each lump's moment.
  Drops a second 0 pours nothing.
- **Drain card**: **Feeds pours** (default on). A Drain pulls `DRAIN_PULL` (30 a second) × Rate in its Radius, a
  Leak `LEAK_PULL` (1) × Rate, an Edge of the course `DRAIN_PULL` × Rate along its Edge (or every edge). Whirlpool
  is left for part 4b's forces.
- `WaterSetup.drains` (enabled Drains in order, past 32 noted `TOO_MANY_DRAINS`); `FrameInputs.drains` (from
  `inputs_at`) and `FrameInputs.timed` (from `inputs_on`, `timed_on`: each pour's lumps for the frame on the
  field, its seed from its thing id); `set_inputs` sets both (no timed drops while the body is full). A fed tap
  with no drain that feeds pours says `NO_FEEDING_DRAIN`. `LATER_KINDS` is gone. The print hashes the drains;
  `inputs()` has a `drains` part (from 0) and a `lumps:<id>` part for each rain, drip and burst (from its Starts
  less `TAP_LEAD`, as a tap). `drops_about` counts the lumps. `start()` sets frame 0's timed drops and drains too.
- The editor's draft thread (`bpe-studio water_mode/draft.rs`) plays each frame through `set_inputs`, as the final
  and the export do (it used to set the inputs by hand, and would have left the rain and the drains out).

### The editor (`bpe-studio water_mode/stage.rs`)

- Markers: rain shows its aim and its width across, a drip its aim, a burst its aim and its lump's circle; a Drain
  or Leak its reach as a circle; an Edge drain a thick line along its edge(s).

### Tests

- `bpe-water/tests/pours.rs` (on the card): a timed drop falls as one added at its moment (within 0.1 cell after 10
  frames, and the list is used once); rain pours its amount (within 5 %) from its Starts and repeats twice and from
  a copy; a leak at pull 1 keeps 0.37 of a pool in a second at 12 and 6 px (measured 0.379, 0.375); a drain in a
  pool's floor leaves under 5 % after 3 s (measured 3.0 % and 2.7 %: a few drops cling to the walls and corners)
  and its count is exactly what it took; two drains are told apart and air is not counted; an edge drain takes all
  of a tap's water (what it took plus what is left is what was poured); a drain with no pull is to the bit no
  drain; a loop keeps every drop (14,541 taken from a pool of 4,266 in 10 s); a fed tap with no feeding drain pours
  nothing; the tank survives a restart point and a copy to the bit; drains that take nothing change nothing where
  they overlap another, and overlapping drains take the same whatever their order. Unit tests in `lumps.rs`
  (heavy long rain is counted whole) and `drops/drains.rs`.
- `bpe-kit`: `rain_drips_and_bursts_pour_at_their_moments`, `drains_are_read_each_frame`,
  `a_fed_tap_says_when_no_drain_feeds_it`; `what_restarts_the_draft` now counts the Drain card, moving a Drain and
  turning one off; `the_earliest_change_is_where_the_water_can_first_differ` (rain from just before its Starts, a
  drain from 0).
- `bpe-studio`: `a_picked_pour_says_what_it_cannot_do` (rain pours; a fed tap with no drain says so),
  `the_draft_plays_rain_and_drains`.

## Part 4.2: Weight, Keeps apart and colour mixing (as built)

Plan `docs/superpowers/plans/water-mode/part4-more-liquids.md` ("4.2"). Several liquids in one course: a heavy one
sinks through a light one and oil floats, a liquid that keeps apart pulls into round drops in another, and colours
run together where liquids meet, by Mix speed, the same in pixels at every detail.

### Weight (`bpe-water drops/numbers.rs`, `common.wgsl`, `sweep.wgsl`, `finish.wgsl`, `deep.wgsl`)

- `Feel::mass()` is the Weight clamped to `LIGHTEST` 0.1 to `HEAVIEST` 10; `FeelGpu.b.w` holds it and the new
  `FeelGpu.c` `(1 / mass, keeps apart, the mixing root, Mixes when's code)`. `Fr.mixf.x` bit 0 (`weighted()`) is set
  only when the liquids' masses differ, and **every mass term is behind it**: with one Weight (or one liquid) every
  pass runs its old arithmetic, so a scene's drops are the same to the bit at any Weight
  (`a_liquid_alone_moves_the_same_at_any_weight`, `two_liquids_alike_move_as_one`).
- Weighted, the pressure is Macklin and Mueller's with masses: the multiplier's stiffness is
  `w_i |sum g|² + sum w_j |g|²` and the softening `k.z w_i` (`w` one over the mass), and the shift is scaled by `w_i`,
  so a heavy drop yields less and sinks. In `finish_at` each pair's evening, closing loss, skin pull and bend are
  shared by `pair_mass = 2 w_i / (w_i + w_j)` (where the liquid lies is not: it is a way, not a force). **Air weighs
  as the lightest liquid** (`Fr.mixs.zw`, its mass and one over it): masses count only against each other, so with
  the flag on air behaves as it does with it off, and a bubble rises in two light liquids as in water (with air
  weighing 1 it rose 15 % slower in liquids of 0.3). The deep part (`deep.wgsl`, `SUMS` 7) gathers each cell's mass (word 6;
  the liquid's mass, air 1), keeps it in `dmass` (binding 31, 8 bytes a cell, written by `cs_deep_across`) and
  scales a drop's push by the cell's mean mass over its own.

### Keeps apart (`finish.wgsl`)

- **Keeps apart** (0 to 1, the Mixing card): between two different liquids the larger of their two keeps counts.
  To the skin such a neighbour is only `1 - keep` liquid: where the liquid lies, the skin's pull and the bend count
  it by `1 - keep`, so each liquid has a surface against the other and a lump of one pulls into a round drop in the
  other. Pressure and the evening of speeds still act between them (they don't pass through each other). Keeps apart
  1 also stops colour crossing. Liquids that keep nothing apart run the old expressions (a branch, not a `× 1`).

### Colour mixing (`bpe-water drops/shaders/mix.wgsl`, `drop.rs`, `drops/mod.rs`)

- A drop's **mix word** is its id's 4th word (`Drop.mix`, which replaces the stretch `e`): bits 0 to 3 the other
  liquid it carries, bits 16 to 31 its share of 65535, 0 unmixed (`mix_word`, `Drop::with_mix`, `mix_share`,
  `mix_other`). Its motion stays its own liquid's; only its colour (and look) blends. Air keeps 0.
- Once a frame, right after the last step's `Finish` (before `cs_drained` and trapped air's passes; `P::Mix` and
  `P::MixKeep` sit before `Report` in the pass list so they get step bind groups), `Fr.mixf.y` passes of `cs_mix` (bindings 0, 1, 2, 5, 8, 30) and `cs_mix_keep` (0, 1, 5, 30) diffuse
  every liquid drop's holding of each liquid toward its liquid neighbours' (a 16-entry array a drop): a pair's
  share a pass is `gate (1 - keep) r_i r_j L`, `L = (2 / rho0) 30 / (pi h³) t² / max(d, 0.05 h)` (the spiky kernel's
  Laplacian weight), scaled by `0.9 / S` where the shares' sum `S` passes 0.9. What isn't the drop's own liquid is
  carried as the one it holds most of (ties to the lower number), so with three liquids the weaker's dye goes to the
  stronger. `mixb` (binding 30, 4 bytes a drop) holds a pass's result. Sums run in sorted order, no random numbers.
- **Mix speed** `s` is `D = 400 s²` px² a second (`MIX_MOST`; between two liquids `400 s_i s_j`): about 20 px of
  blend in a second at 0.5, 40 px at 1. `Numbers::mix_passes` picks enough passes that one moves at most 0.45 of a
  resting drop's colour (`MIX_STEP`, from `mix_at_rest`), 1 to 64 (`MOST_MIX_PASSES`; finer than about 1.5 px at Mix
  speed 1 it mixes as fast as 64 passes allow); `mix_root` gives each liquid's `r` for that many. So the blend's
  width is the same in pixels at 6 and 3 px cells (within 30 %, `mixing_is_the_same_at_every_detail`).
- **Mixes when**: Always; **Only when stirred**, `smoothstep(STIR / 4, STIR, |v_i - v_j|)` with `STIR` 200 px/s (a
  pool resting under gravity has neighbours moving against each other at up to about 35 px/s at 6 and 3 px, so its
  jiggle is no stir); **Only on hits**, `smoothstep(HIT / 2, HIT, closing)`, `HIT` 150 px/s. The pair's gate is the
  smaller of the two liquids' (`Fr.mixs`, in cells a second).
- A restart point and a copy keep the mix words (the ids are kept whole). `Water::add` clears a word that names the
  drop's own liquid (`cs_mix` would drop it), and air carries none.

### The take and the drawings (`pack.wgsl`, `capture.rs`, `take/`, `draw/stored.rs`, `look.wgsl`, `plain.wgsl`)

- **`TAKE_FORMAT` 3**: `cs_pack` packs 12 bytes a drop (the third word the mix word, air 0); a moment
  (`FrameCodec`, magic `BPF2`) keeps 7 planes, the share as a byte (`round(share16 / 257)`) and the other liquid, so
  a share under a 255th reads back unmixed. `TakeFrame.mixes`; `StoredDrops::set` takes them (the cli, the final
  view and the kit's tests pass `frame.mixes`).
- **The look**: `vs_look_splat` passes the other liquid and share (flat); `fs_look_splat` and `fs_look_weights` put
  `k (1 - s)` in the own liquid's channel and `k s` in the other's, so every per-liquid look setting blends by the
  weights; `vs_look_nodes` mixes sigma, glow and colour by `s`. Mist and sparks stay the drop's own liquid's.
  **The plain drawing** (`vs_splat`, `mix_of`) mixes colour and deep by `s`. Both only when `s > 0`: an unmixed drop
  draws what it drew before.
- `draw/sets.rs` `mixed(cell)`: a pool of two liquids whose shares ramp to a half across its middle third, and a
  half-mixed blob; the full-size look check's `water-mixed-c3` and `water-look-mixed-c3`.
- The probe's `picture` no longer colours by the stretch (gone with `e`). Drops files (`stored::save_drops`) are
  `BPEDROP2`, 28 bytes a drop with the mix word.

### The cards and the setup (`bpe-kit modules/water/liquid.rs`, `things/water/draft.rs`, `draft/inputs.rs`)

- **Mixing card**: the group "Meeting other liquids" (a section in `bpe-core` `SECTION_ORDER`) holds **Keeps apart** (new, 0 to 1), Mix speed and Mixes when;
  Fades and Fades to are gone (they need drops that carry an age). Un-mix stays for part 4.4. The Feel card's
  **Weight** is 0.1 to 10.
- `LiquidSetup.mixing` (its Mixing card); `MIXING_KEYS` (`keeps_apart`, `mix_speed`, `mixes_when`) are in the print
  and in each liquid's `inputs()` part and restart the draft; `inputs_at` fills the Feel's mixing fields. A Liquid
  with no Mixing card mixes nothing and keeps nothing apart (taking the card off restarts the water).

### Tests

- `bpe-water/tests/mixing.rs` (on the card): a lone liquid at Weight 1, 4 and 0.3 and two alike liquids move the
  same to the bit; a Weight 3 block sinks through a Weight 1 pool at 6 and 3 px, and in a pool deep enough for the
  deep part; oil (0.8) rises from under water; a kept-apart square pulls round (40.3 to 33.0 px in a quarter of a
  second, a round drop's reach 33.9) where a not-kept one stays (42.2); colours run together by the line, stay
  their own 80 px off and keep each liquid's amount within 1 %; the blend is as wide at 6 and 3 px; Mix speed 0 and
  Keeps apart 1 cross no colour; resting liquids that mix only when stirred or on hits stay apart, and a lump
  falling in mixes them; a restart point and a copy keep the mixes; a bubble rises as fast in liquids of Weight 0.3
  and 0.31 as in water (within 5 %). Unit tests: the pass count
  (`numbers.rs`), a moment keeps each drop's mix (`take/frame.rs`).
- `tests/look.rs` `a_mixed_drop_draws_between_its_liquids`, `tests/plain.rs` `a_mixed_pool_draws_between_its_liquids`.
- `bpe-kit`: `the_mixing_card_reaches_the_water` (and none with no Mixing card); `what_restarts_the_draft` counts the
  Mixing card's three.

## Part 4.3a: ready-made liquids (as built)

Plan `docs/superpowers/plans/water-mode/part4-more-liquids.md` ("4.3a"), decision 23. No physics, setting or drawing
changed; goldens and the take don't move.

- **The table** (`bpe-kit modules/water/liquids.rs`): `ReadyLiquid` (Water, Oil, Honey, Syrup, Slime, Milk, Paint,
  Mercury, Ferrofluid; `ALL`, `title()`, `words()`, `row() -> LiquidRow { feel, mixing, look }`), like the looks'
  table. Water's row is the cards' defaults. Adding a liquid is adding a row. Ferrofluid's row is part 4b's (water's feel, which
  the Magnet's spikes are tuned with; black, solid, glossy), kept apart from water as an oil-based liquid is.
- **`apply_liquid(scene, reg, id, liquid) -> bool`** writes the Feel card whole, the Mixing card's *Meeting other
  liquids* and the Look card's *Its colour*, *Its body* and *Its surface* (`WRITTEN`), never the Look's *Glow*,
  *Drawn* and *Extras* (`LEFT`: how the scene's ready-made look draws every liquid, so a Cartoon scene's oil keeps
  its outline; the Mixing card's old *Un-mix* group went in 4.4), never Heat. A setting at the card's default is removed (as a look
  does), keyframes stay. A Liquid still called what it was given (Liquid, Water, a look's or a liquid's name, with
  or without a number, `takes_title`) takes the liquid's title. False, nothing changed, for anything but a Liquid.
- **The writer**: `looks.rs`'s `write` and `write_card` take a filter (`Fn(&SettingDef) -> bool`) instead of
  `keep_colours` (`a_look_writes` is the looks' own), and are `pub(super)`.
- **The editor** (`bpe-studio inspector/use_liquid.rs`): a **Use a liquid…** menu under a Water Liquid's name and
  tags; a pick is `Edit::UseLiquid`, put on every selected Liquid (`reached(.., None)`: a selected Pour is left
  alone) as an undo step of its own (`own_step`). The Feel and Mixing cards are in the physics print, so the draft
  restarts on its own.
- **Rows** (as 4.3a made them; 4.3b moved Thickness to a new scale and made Honey 0.8 and Slime 0.6):

| | Thickness | Surface pull | Weight | Stickiness | Keeps apart | Mix speed |
| --- | --- | --- | --- | --- | --- | --- |
| Water | 0.02 | 0.5 | 1 | 0 | 0 | 0.5 |
| Oil | 0.08 | 0.35 | 0.9 | 0.2 | 1 | 0 |
| Honey | 0.6 | 0.6 | 1.4 | 0.6 | 0.2 | 0.15 |
| Syrup | 0.35 | 0.55 | 1.3 | 0.5 | 0.1 | 0.25 |
| Slime | 0.45 | 0.85 | 1.1 | 0.7 | 0.6 | 0.1 |
| Milk | 0.03 | 0.5 | 1.03 | 0.05 | 0 | 0.6 |
| Paint | 0.25 | 0.45 | 1.2 | 0.4 | 0 | 0.8 |
| Mercury | 0.01 | 1 | 10 | 0 | 1 | 0 |
| Ferrofluid | 0.02 | 0.5 | 1 | 0 | 1 | 0 |

  Looks: Oil clear gold with a faint thin film; Honey deep amber, glossy; Syrup dark red-brown; Slime bright green,
  half solid, smooth; Milk cream white and solid; Paint red and solid (a dark red deep colour, so a blend with a
  clear liquid darkens to red, not to the default navy); Mercury mid silver, solid, darkening to near black with
  depth (Clarity 40) for a metal's gradient, the most shine and edge sheen (a pale silver read as milk).
- **Look check**: `LOOK_ONLY=water-liquids`, nine Basins with one liquid each, a lump standing on each floor that
  slumps to the walls, made at Coarse (3 px) and drawn at 2 s. Found with it: a lump dropped ~100 px into a basin
  throws jets at about four times its landing speed (a flat lump meeting a flat floor; eight Water rows do the
  same, so it is the engine's, not a row's), which leave 300 px basins and dye the other liquids where they land.
  The still stands its lumps on the floors instead.
- **Tests**: `liquids.rs` (`each_liquid_writes_its_row`, `a_liquid_leaves_heat_and_the_scenes_drawing`,
  `every_setting_is_written_or_left_on_purpose`, `a_liquid_takes_its_title_unless_it_was_named`,
  `only_a_liquid_takes_one`, `the_liquids_say_what_they_are`); `bpe-studio` inspector
  `use_a_liquid_sets_every_selected_liquid`, `only_a_liquid_offers_use_a_liquid`.

## Part 4.3b: thick liquids (as built)

Plan `docs/superpowers/plans/water-mode/part4-more-liquids.md` ("4.3b" and its "As built"), decision 22. Water and
every liquid of Thickness 0.1 or less move exactly as before (no thick pass, the plain evening, joined steps), so the
goldens and the pinned pictures don't move.

- **Thickness's scale** (`Numbers::evening`): ν = 100 + 2,500 t, plus 140,000 (t − 0.1)³ past `THICK_FROM` (0.1),
  t at most 4 (px² a second; `visc_cube`, override `VISC_CUBE`). Syrup 0.35 about 3,200, Slime 0.6 (the old Honey)
  about 19,000, Honey 0.8 about 50,000, paste 1 about 105,000.
- **Who is thick**: Thickness past 0.1. A thick liquid has **no plain evening** (`feel_gpu`'s `b.z` is 0): the
  stages take all of it (`Numbers::thick_evening`). The plain evening pulls a surface drop toward its neighbours'
  speeds, which wears a turn away at a surface, and the stages then carry that loss through the blob (a spinning
  disc of honey lost half its spin in a quarter second).
- **The passes** (`shaders/thick.wgsl`), **once a frame** after its last step's Finish, as the colours mix (steps
  stay joined): `cs_thick_grad` reads each thick drop's velocity gradient from its own liquid's neighbours, corrected
  so a straight-line field reads exactly even at a surface (G = A M⁻¹, spiky gradient; M's smaller eigenvalue held
  up to a tenth of its trace, only where a thread is about one drop wide), and keeps **only its turn** (Peer,
  Cornelis and Teschner keep the spreading too; kept, a tap's stream, whose rows part as they speed up, fanned out
  sideways into spray). Then `s` RKL1 stages (`cs_thick_stage`, each with `cs_thick_tick`, Meyer, Balsara and
  Aslam's super time stepping: `s` stages smooth as (s² + s) / 2 plain sweeps): each moves a drop's speed toward
  Σ W (v_j + turn × (x_i − x_j)) / Σ W over its own liquid's neighbours and itself. A **solid face** within reach
  counts as liquid at rest behind it (`wall_table`'s share of the rest density added to Σ W), so a thick liquid
  clings to what it touches and sags off a ledge instead of sliding off it. (The course's own edge is not a solid
  and holds nothing.) Buffers reused, free once Finish has run: the turn in `lamb` (13), the stages' speeds rotate
  through `xv.zw`, `xp.xy`, `xp.zw` (6; the stages are a multiple of three, so the last lands in `xv`), the stage
  number in `blocks[0]` (9); `cs_thick_stage` reads the solids (15). No new binding.
- **How many** (`Numbers::thick_stages(feels, cell)`): a plain sweep smooths by `thick_moment` (the kernel's second
  moment on the resting lattice, about 0.148 cells²) × cell² / (2 × frame); the fewest stages that give every
  liquid its ν at a share of at most 0.9 (`THICK_SHARE`), at most `MOST_THICK_STAGES` (180). Thickness 0.6 takes 66,
  51, 33, 18, 12 and 9 stages at 1.5, 2, 3, 6, 8 and 12 px, Honey's 0.8 about 105 at 1.5 px and 54 at 3 px; paste on 1 px cells and Thickness 4 at 3 px take all
  180 and are thinner there than they ask (paste about 62 % of its thickness, Thickness 4 about 7 %). `FeelGpu.d.x`
  is each liquid's share, `d.y` the plain evening's share air takes when the liquid is liquid 0 (so bubbles in a
  honey-only scene still feel the honey; `b.z` for a liquid that is not thick); `mixf.z` the stages.
- **Measured** (`tests/thick.rs`, lavapipe, 6 px): a 120 px lump on a floor is 356, 354, 351, 321, 242 and 148 px
  wide after a quarter second at Thickness 0.02, 0.1, 0.11, 0.35, 0.6 and 1 (smooth across 0.1); honey 0.6 is 242
  px wide at 6 px and 246 at 3 px; a disc of honey turning at 3 radians a second still turns at 3.01 after a
  quarter second; a sheared disc keeps 15 % of its shear in a tenth of a second (water 93 %); a 60 px block on a
  wall slides 68 px in a quarter second (water falls 125).
- **What it can't do yet**: the drops have no pull holding a stretched thread together (the density solve only
  pushes), so a thread thinner than a few drops parts. A slow, narrow tap's stream at 6 px pulls apart into lumps a
  little below the spout as it speeds up (water's does too at slow taps), so it falls in lumps that pile up and
  fold rather than one unbroken rope (an 80 px tap at 400 px a second at 3 px holds together); two blobs pulled
  apart part without a strand. Honey costs: at 1.5 px it takes about 105 stages a frame. Candidates: a thick
  stream held at its spout's speed for its first reach; a pull in the density solve inside a thick liquid.
- **Rows**: Honey 0.6 → 0.8, Slime 0.45 → 0.6 (Syrup 0.35 and Paint 0.25 as they were).
- **Tests**: `numbers.rs` `thickness_sets_the_evening`, `only_thick_liquids_take_stages`; `passes.rs` (the
  entries); `tests/thick.rs` `thicker_liquids_spread_slower`, `thickness_spreads_alike_at_6_and_3_px`,
  `a_thick_blob_keeps_its_spin`, `a_thick_blob_loses_its_shear`, `a_thick_liquid_clings_to_a_wall`.
- **Look check**: `LOOK_ONLY=water-thick` (`full_size_look.rs` `water_thick_pictures`: honey in three columns, a
  tap at the left, a lump on a ledge in the middle, a burst thrown at a wall on the right; made at 3 px, 38,036
  drops, 509 s on lavapipe; frames at 0.5, 1, 1.5 and 2 s). The tap's 80 px stream at 400 px a second holds
  together at 3 px, lands and folds into a pile by 0.5 s; the lump sags over the ledge's end into a hanging drip by
  2 s; the burst clings to the wall and creeps down it. Seen: once the pile reaches the spout a slot opens in the
  stream there, and the stream's tail spatters a little as the tap stops. No jagged edges, seams or leaks in the
  1:1 crops.
- **Not checked**: slow motion of a thick liquid. The moments inside a frame are packed before the frame's thick
  stages run, so in slow motion a thick liquid's in-between pictures may step slightly at each frame; look at a
  slow-motion final of honey before relying on it.

## Part 4.4: Un-mix (as built)

Plan `docs/superpowers/plans/water-mode/part4-more-liquids.md` ("4.4 Un-mix", decision 24 and its choice). Aedan
chose (b), a backwards stretch of the video, on 2026-11-28. No physics changed.

- **The Water card**, group *Un-mix*: **Un-mix**, **From** and **To** (water seconds; defaults 2 and 5). The Mixing
  card's *Remember for Un-mix*, *Seconds* and *Un-mixing* are gone (in Water mode they never did anything).
- **The clock** (`bpe-kit things/water/draft/slow.rs`, `Speeds`): when the video reaches To it plays the water
  backwards to From at the Speed there, then forwards from From again and on to the end. `unmix()` is the stretch
  when it is on, From < To and To before the Length's end (else nothing changes); `water_time(t)` mirrors slow motion's
  clock over the stretch; `video_time(τ)` is where the video first shows τ (past To, after both passes); the video
  is twice the stretch's video seconds longer (`video_length`). `without_unmix()` is the same clock without it.
  `longer_words()` says "in slow motion", "with Un-mix" or both (the status line and the Export window).
- **The take**: its print never reads Un-mix (as it never reads Speed), so turning it on or moving the stretch makes
  nothing again; `bpe-cli` exports the final over the new clock. An exported draft leaves Un-mix out (the draft only
  plays forwards), so it is shorter than the Length the Export window shows, which is the final's.
- **The editor**: while the Final shows a scene with Un-mix, the playhead runs on the video's clock
  (`Studio::water_next`, `WaterFinal.clock`: the video time and the water time it gave; a playhead set anywhere
  else starts it again where the video first shows that time), so playing goes back over the stretch and forwards
  again, where the final is made through To. The draft plays forwards (going back in the draft plays again from its newest restart copy, up to 120
  frames, for every frame).
- **Tests**: `draft/tests.rs` `un_mix_plays_its_stretch_backwards_then_forwards`; `bpe-studio`
  `un_mix_takes_the_finals_playhead_back_over_its_stretch`.

## Part 4b: forces and ferrofluid (as built)

The plan is `docs/superpowers/plans/water-mode/part4b-forces-and-ferrofluid.md` (its decisions and "As built and
measured"). Wind, whirlpools and magnets push the water; a magnet stands its liquid up in spikes, and the eighth
ready-made look, **Ferrofluid**, makes that liquid black and glossy. Heaters and Coolers wait for heat.

### Forces in the engine (`bpe-water drops/forces.rs`, `shaders/forces.wgsl`)

- **A force is an input, as gravity is**: `Water::set_forces(&[Force])` once a frame (at most `MOST_FORCES` 16,
  those that can't be felt left out: not finite, no reach, no liquid, a magnet's pull 0, a rate 0), kept in
  `Fr.forces` (48 bytes each: place, reach and the most push in cells; the kind's numbers; kind, the liquids'
  bits, a magnet's spikes). Nothing of it is kept from frame to frame, so the take's format is as it was; a copy
  carries the list. Every liquid drop the force names (not trapped air) takes its push in `fall()`, after gravity
  and damping and before the speed cap: the magnets' pushes are added, then each whirlpool's and wind's draw on the
  speed in the list's order. A push that isn't finite is left out, the drop kept.
- **Magnet**: the slope of `Φ = −A·R·Ψ(u/s)`, `u = r/R`, `Ψ(v) = (1 − v²)³`, `A` = Strength × 4000 / 1.7173
  (`PSI_SLOPE_MOST`: Strength gravities at most with no spikes). Its reach's share `s = 1 − depth·c(u)·(1 −
  T(x))` dips between `spikes` teeth spread over `spread` round `aim`: `T = (1 − |x|)³` over a tooth's half width
  (`SPIKE_WIDTH` 0.5 of the turn between two, so neighbouring teeth meet), its point rounded off within
  `SPIKE_ROUND` 0.1 so the pull doesn't flip across a spike's middle, `c(u) = smoothstep(0.2, 0.45, u)` (round and
  calm inside), the valleys' reach past the spread. Every level past `u` 0.45 is the same star, so the liquid's
  skin stands in cones with sharp tips and round valleys, the same at every detail, and along every line out of
  the middle the potential only rises, so the middle is the deepest place. The depth is held to `MOST_DEPTH` 0.9
  and to `MOST_STEEP` 3.9 × a tooth's half width (radians), so narrow teeth are no steeper than 9 over 160° at
  Spike size 1: uncapped, 9 over 30° at Strength 4 stirred at 205 to 260 px/s and 9 over 60° at 165 to 200,
  capped 7 to 12 and 26 to 32 (20 over 360°: 40 to 55; 9 over 160°, not capped: 36 to 49). A push is held to
  `MOST_PUSH` 256 gravities (a guard: held pushes are no longer down a slope, and the first magnet, which met its
  guard, churned); its size and way are worked out apart, so a pull too big for a float still pushes at the most.
  It pulls through rock (the field isn't blocked; the drops are still kept off the solids).
- **Whirlpool**: the speed round its middle is drawn toward `speed·2x/(1 + x²)`, `x = r/(0.3R)`, at `rate` a
  second faded over the last 30 % of its reach, by the exact `1 − e^(−rate·dt)`, so every step count gives the same
  change a second. **Wind**: a drop's speed is drawn toward the wind's at `rate` times how much of it meets the air
  (`1 − smoothstep(0.35, 0.75, fullness)`: a lone drop all of it, a flat open top a little, inside nothing). Wind
  blows everywhere; solids don't shelter (Known).
- **Inside a magnet's reach its liquids hold themselves up** (`in_magnet` in deep.wgsl's cells pass: no cell
  there that holds one of the magnet's liquids is full, so depth is measured from it; the gather pass ORs each
  cell's liquids into an eighth sum): the deep water rests no fuller than the drops just above it (C2), a magnet
  presses them fuller, and the deep pressure stirred the mound at about 100 px/s. Without it the mound comes to
  rest. The water there is a little more pressed than elsewhere (Known). Other liquids keep their deep part: with
  the whole reach the drops' own, a deep tank of water over a magnet for another liquid sank 10 px and stirred at
  13 to 29 px/s against 3 to 5 (`a_magnet_leaves_other_liquids_deep_water_alone`).
- **Steps**: `steps_now` adds each force's push over two frames, as for gravity (`Force::headroom`: a magnet's pull
  without its spikes, Strength gravities; a whirlpool's and a wind's draw on a drop at rest). Counting a spike's
  sharper slopes took a 6 px frame to 32 steps, where the skin's bend (calm only at the trial's pace, C1) stirred a
  Strength 4 mound; at the floor's steps it holds still. **Strong magnets** (`strong_spikes_measured`, Strength 4,
  Spike size 1): at 6 px 16 steps, tips moving 0.65 px a second; at 3 px 32 steps, 0.27; no drop thrown past the
  reach. So the skin calm at any step count was not needed here (still Known for fast water at fine detail).

### Force cards (`bpe-kit modules/water/places.rs`, `things/water/draft/forces.rs`)

- The **Force** card: Kind, **Liquid** (Any Liquid by default; a Liquid, a tag), Strength (keyed), Reach (not
  Wind), Aim and **Gusts** (Wind), Spikes (9), **Spike size** (0.75, keyed), **Spikes point** (−90, up) and **Spike
  spread** (180°) for a Magnet.
- `ForceSetup::all_of` reads the enabled Forces and the Drains whose Whirlpool is above 0 (at the start or on a
  key above 0) in the scene's order; a Drain past the 32 the water drains spins nothing either. Per frame,
  `force_at(time)`: a Magnet's pull `S·FORCE_UNIT/1.7173`, depth `SPIKE_DEPTH` 0.6 × Spike size (0 with no spikes); a Whirlpool `S·SPIN_SPEED` (600 px/s), rate `SPIN_RATE` 4 ×
  min(|S|, 1); a Wind `S·WIND_SPEED` (1000 px/s) × `1 + Gusts·gust(t)` along Aim, rate `WIND_RATE` 3 × min(|S|, 1),
  so Strength 0 is no force; a Drain's whirlpool `Whirlpool·600`, rate 4 × min(Whirlpool, 1), reaching twice its
  Radius, every liquid. `gust(t) = (sin 1.3t + sin(2.9t + 1) + sin(4.7t + 2))/3` (det maths). A force's place is
  its position, read through `place_at(time)` for a later place track.
- Notes: `HEAT_LATER` (Heater, Cooler), `NO_LIQUID_TO_MOVE` (its Liquid off, gone, past 16, or none),
  `TOO_MANY_FORCES` (past 16), `TOO_MANY_SPINS` (a spinning Drain past 16 forces: it still drains, its whirlpool
  doesn't spin).
- **What restarts**: every Force card setting, a Force's place and turning it off restart the draft and mark the
  final out of date (the print, and `force:{k}` parts of `WaterInputs` from 0, by place in the list: whirlpools and
  winds draw in its order); a Drain's whirlpool is in its drain's print and parts already (part 4.1).
  `set_inputs`, `start` and `resume` set the forces.
- The stage's held setup takes a note that changes with no physics (`WaterRun::renote`): a Force turned into a
  Heater with no Liquid in the scene said nothing before.

### The stage (`bpe-studio water_mode/stage.rs`)

- `force_marks`: a Magnet's and a Whirlpool's ring at its Reach (`LINE_SOFT`), a Magnet's spikes as ticks across
  it, a Whirlpool's spin as an arrow head at the ring's top (to the right above 0), a Wind's three arrows along its
  Aim, all at the playhead with keyframes.

### The Ferrofluid look (`bpe-kit modules/water/looks.rs`)

- The eighth `ReadyLook` (title "Ferrofluid", Liquid "Ferrofluid", "Black glossy ferrofluid on a pale plate, in soft
  even light."): ink black (`#08080b`), Solid 1, Clarity 0, no Bend or Streaks, Smoothing 0.15 (spikes keep their
  points), Shine 1 of size 0.6 (its gloss is the sky's highlight along the rims that face up), Edge sheen 0.12; a
  pale Flat plate (`#e6e3dd`) on a pale Back (`#d4d1cb`); Ambient 1, Bounce 0.2, no caustics or shafts, no lamps;
  Bloom 0.1.
- Why no lamps: an opaque liquid stops a lamp's light, so it throws its shadow on the back. Spots placed by their
  share of the shot sat in the rock above a carved chamber and lit nothing; two shadowless Suns from 55° and 125°
  filled the valleys between the spikes with grey; from 72° and 108° the spikes were fine, but in the shared looks
  scene every drop stuck on the ceiling, the tap's stream and the slope's film threw long stripes across the pale
  back, with the light map's steps on their edges. With no lamps nothing throws a shadow. Why little sheen: a thin
  spike is all edge to the surface rounding, and at Edge sheen 0.55 the spikes read grey.
- Full-size look check: `LOOK_ONLY=water-ferrofluid` (a carved chamber 700 wide, a 380 x 70 pool on its floor, a
  Magnet 40 px under the floor in the rock, Strength keyed 0 to 3 over 1.5 s, Spikes 9 over 160°; Normal detail,
  4 s, about 33 000 drops, made in about 400 s on the cloud's software card): the whole screen at 3 s, the frame
  after it, and the spikes at twice the size. Its final is kept with `LOOK_TAKES`.

### Known (part 4b)

- Wind blows everywhere in the course: solids don't shelter the water behind them. Forces don't move air drops,
  and nothing moves a force (part 5's place keyframes are on Pieces and Walls only).
- Inside a magnet's reach there is no deep part, so a deep pool there is a little more pressed than elsewhere.
- A Magnet's two outer teeth, near the floor, fill to the edge of its Reach.
- Heaters and Coolers wait for heat.
- The look drawing's, seen in the Ferrofluid look check: a spike under about 50 px across is drawn tilted across
  its whole width (the surface rounding reads the smooth surface at 24 px, which calls the spike all edge), with a
  faint band and a dashed one-pixel highlight where it meets the body; the liquid's own shadow on the back has the
  light map's steps on its edge at Good light (4 px), finer at Best.
- The skin calm at any step count is still not built (C1, C2): not needed by forces, as measured above.
