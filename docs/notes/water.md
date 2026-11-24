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
  Water cards too. New section names sit after "Health" in `SECTION_ORDER`.
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
  `bpe-cli` refuses with `NO_WATER_EXPORT` (`check_exportable`, at the start of every export).
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
