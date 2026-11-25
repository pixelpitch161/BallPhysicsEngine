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
  Drain, Lamp, Force, Camera path; `ready()` is Select, Pour, Drain, Lamp and Force, the rest are greyed with
  "Arrives in a later part of Water mode"), and the course fitted to the room (`CourseView::fit`, `to_screen`,
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
- The rail's Carve, Piece, Pen and Camera path tools, the looks behind `StartingLook`, Draft | Final and Make
  final, grading, sound and export are later parts (the spec's "Parts").

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
- **Not built yet: tiles.** The grid is dense (every cell of the course stored and stepped). The plan's tiles are
  their own part, before courses bigger than a screen.
- **The bridge** (`bridge.rs`, `bridge.wgsl`): the drops rewritten in today's 112-byte layout with a wet grid made
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
leaves a corner as one arc, a lone drop is left alone), `bpe-kit/tests/water_core.rs` (`the_bridge_draws_every_drop`). By hand, in release:
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
  this part"). No number changes it; it needs its own design.
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
