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
  part gently (`most_out_thin`). A drop's carried stretch (`Drop::e`) pulls through the hold nodes (`hold.wgsl`):
  weak at the default Surface pull.
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
pour), `tests/streams.rs` (a thin pour is one narrow stream at 8 and 12 px cells, a sheet stays a sheet, a lone
drop is left alone), `bpe-kit/tests/water_core.rs` (`the_bridge_draws_every_drop`). By hand, in release:
`bpe-water/tests/measure.rs` (`look`, `bands`, `strands`) and `water_core.rs::side_by_side` (both cores drawn
from one scene, joined left and right; its header lists the `WATER_*` variables).

