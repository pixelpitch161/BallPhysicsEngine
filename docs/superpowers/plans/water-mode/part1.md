# Water mode part 1: Water scenes open (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A scene is Balls or Water. The app opens on a chooser (and File > New shows it), a Water scene saves and
loads, holds the Water things as data, and shows its own stage shell with the tool rail. Nothing moves and no
water is drawn yet. Balls scenes are untouched.

**Architecture:** `bpe-core` gains `SimKind` on the `Scene`, ten new `SlotId`s, and one rule used everywhere: a
thing kind and a slot each say which scene kind they belong to (`ThingKind::in_sim`, `SlotId::in_sim`), and
`add_thing`, paste and `resolve` respect it. `bpe-kit` registers the eight Water thing kinds and their fourteen
card kinds as plain `#[derive(Settings)]` data with no sim and no draw, so the ball runtime ignores them. In the
editor a Water scene hands the ball engine an empty stand-in scene, so the ball playback and every job built on
it (aim, particles, sound, Follow sound sampler, light probe) stay empty without a check at each of the roughly
90 places that read the playback; the central panel shows `water_mode::stage` in place of the ball preview.

**Tech stack:** Rust (toolchain pinned), `bpe-core`, `bpe-kit`, `bpe-cli`, `bpe-studio` (egui 0.37, egui_kittest).

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md`, sections 1, 7, 8, 9 and Parts item 1.
Mockups (chooser, layout D): https://claude.ai/artifact/5Y9ow3CYMFKGiNhmz1uTtA and
`ballphysics/water-mode/mockups/` in the project files (`Main.dc.html` is the chooser, `Rail.dc.html` layout D).

## Global constraints

- **Balls scenes do not change**: physics goldens, sound goldens and pinned pictures are byte-identical. No file
  under `crates/*/tests/golden*` may differ from main. No golden is re-blessed in this part.
- A scene file with no `sim` field is a Balls scene. `Scene::FORMAT` is 13, `Scene::OLDEST_FORMAT` stays 12.
- No physics, no water drawing, no `bpe-water` crate in this part.
- No sound, no Text, no Balls, Emitters, Shapes, Particles, Rigs, Rules or Health in a Water scene. Pulses
  (Follow sound, light, health) and Vary across balls are not offered there.
- Editor colours come only from `style/` (a test checks). Controls are named for kittest. No `ComboBox`.
- Never create built-in or example scenes: a new Water scene is Scene settings, Camera and the Course only.
- `bpe-core`, `bpe-runtime` and `bpe-kit` refuse platform maths (`clippy.toml`); nothing here needs any.
- Today's Liquid thing (`things/liquid`, `bpe-gfx/src/liquid_sim/`) is not touched.
- Do not build or install the exe. Tests: only what this part adds or changes; CI runs the rest.

## Review focus

Inputs the spec implies but does not spell out. Each has a test in the task that owns the code.

1. **A Water scene file that holds a ball thing** (hand-edited, or a file from a later version): it opens, the
   thing is reported in the tree as not belonging and is left out, nothing crashes (Task 1, `resolve`).
2. **File > New with unsaved work, then Cancel**: the open scene is kept exactly, and the chooser is gone
   (Task 5).
3. **Opening a Water scene while a Balls scene is playing sound**: the sound is rebuilt on the empty stand-in (no
   notes left ringing), no aim or particle job survives, and opening a Balls scene afterwards starts them again
   (Task 4).
4. **Pasting copied ball things into a Water scene** (the clipboard survives New and Open): nothing is pasted and
   the status line says why (Task 1 core, Task 6 words).
5. **The app started by double-clicking a scene file, or after a crash with work to restore**: no chooser over
   the opened scene; the restore question comes first and the chooser only after Discard (Task 5).

---

## Design decisions (all of them)

1. **`SimKind`** (`bpe-core/src/sim.rs`): `enum SimKind { #[default] Balls, Water }`, `Serialize`/`Deserialize`,
   `Hash`, `Ord`; `SimKind::ALL`, `name()` ("Balls", "Water"). `Scene.sim: SimKind` with
   `#[serde(skip_serializing_if = "is_default")]`, so a Balls scene's file gains no line.
2. **Format 13** for every new scene (the spec's choice: one number, and an older program refuses a Water scene
   with "made with a newer version"). A file opened at 12 keeps 12 until it is a new scene; nothing rewrites it.
   Saved grades take the same number (`saved_grade.rs` reads `Scene::FORMAT`); `Video::FORMAT` is its own and
   stays 1.
3. **New `SlotId`s**, appended after `Grade` so no existing slot's place or order in a saved file moves:
   `Water`, `WaterLight`, `Back`, `Form`, `Material`, `Moves`, `Feel`, `Mixing`, `Heat`, `Main`. Names: "Water",
   "Light", "Back", "Shape", "Material", "Moves", "Feel", "Mixing", "Heat", "Settings". All are in the card list.
   `SlotId::ALL` has 22 entries.
4. **Which scene kind a slot belongs to**: `SlotId::in_sim(self, sim) -> bool`. Both: `Look`, `Shader`, `Grade`.
   Balls only: `Trail`, `Motion`, `Bounce`, `Particles`, `Lighting`, `Sound`, `Mixer`, `Health`, `Air`. Water
   only: the ten new ones.
5. **The order cards show in**: `SlotId::shown_in(sim) -> &'static [SlotId]`. Balls: `ALL` in order (as today).
   Water: `Water, WaterLight, Back, Form, Feel, Look, Mixing, Heat, Material, Moves, Main, Shader, Grade`, which
   is the spec table's order for every thing.
6. **Which scene kind a thing kind belongs to**: `ThingKind::in_sim(&self, sim) -> bool`, default Balls only.
   Scene settings, Camera and Folder say both. The eight Water kinds say Water only. Text stays Balls only.
7. **The rule is enforced in the core**, not in each editor panel: `Scene::add_thing` refuses with
   `EditError::WrongScene(kind, sim)`; `paste_things` leaves such things (and what is under them) out, like a
   second camera; `resolve` reports `ProblemKind::WrongScene(kind)` and leaves the thing and its children out
   (never the root); a card in a slot of the other scene kind is ignored, like a card in a slot the kind does not
   offer. The Add menu and the Rules/Follow target lists only filter what they show.
8. **Hidden settings**: `ThingKind::offered(&self, def: &SettingDef, sim) -> bool`, default true. They are
   hidden in the inspector only; `resolve` still gives every setting, so `SceneSettings::from_params` and export
   read them as today. Scene settings in Water: groups Video, Picture, Sound (tempo drives the timeline's bar grid
   and Video mode); World and Background are hidden (gravity is on the Water card, the Back card replaces the
   background). Camera in Water: groups Shot and World box; Follow, On hits and Stage camera are hidden.
9. **Water thing kinds have no own settings** (like Folder). Everything is on cards. A placed thing's place and
   turn are `Thing.transform`, as for a Shape; keyframed places come with the Moves card (Piece, Wall) in part 5.
   **My call**: Pours, Drains, Forces and Lamps are not keyframed in place yet; a later part can add an offset
   setting without changing files.
10. **Ids**: thing kinds `water_course`, `water_piece`, `water_wall`, `water_liquid`, `water_pour`,
    `water_drain`, `water_force`, `water_lamp` (the old Liquid thing is `liquid` and the ball Light is `light`,
    so plain names would clash). Card kinds `water.world`, `water.light`, `water.back`, `water.material`,
    `water.shape`, `water.line`, `water.moves`, `water.feel`, `water.look`, `water.mixing`, `water.heat`,
    `water.pour`, `water.drain`, `water.force`, `water.lamp`.
11. **Kit layout**: `things/water/mod.rs` (the eight kinds from one table, `new_water_scene`, `StartingLook`) and
    `modules/water/{mod,scene,solids,liquid,places}.rs` (the cards). The spec says `things/water_*` and
    `modules/water_*`; one folder each holds the same code with less repetition, and later parts add a file per
    kind there when a kind gains a sim or a draw.
12. **The Camera's Follow card** (spec section 2) is part 9's; it is not registered here.
13. **The look pick** is recorded as the Water card's own setting `look` (`StartingLook`: Plain, Lamp-lit rock,
    Glass lab, Neon, Paint on paper, Molten, Cartoon). Its help says the looks arrive later. Part 6 turns it into
    the look picker or removes it.
14. **A new Water scene** (`bpe_kit::things::water::new_water_scene`): Scene settings with Water, Light and Back
    cards, a Camera, and the Course with its Rock card. No mixer card, no Liquid, no Pour.
15. **Course size** is `screens_wide` x `screens_tall` on the Water card, each 1 to 8; `WaterWorld::screens()`
    keeps the product at 16 or less by lowering `screens_tall`. One screen is 1080 x 1920 px whatever the video's
    size. The course is centred on the world origin, so the default camera shows a one-screen course exactly.
16. **The ball engine's stand-in**: `Studio.no_balls: (Scene, Resolved)` is `bpe_kit::new_studio_scene` resolved,
    made once. `runs::ball_side(scene, resolved, none)` gives the scene itself for Balls and the stand-in for
    Water. Every place that builds or fingerprints the playback, the aim job, the particle job, the Follow
    sampler, the light probe or the sound goes through it. In a Water scene the fingerprints therefore never
    change and nothing restarts. The playback is stepped only by the ball preview, which a Water scene does not
    show.
17. **`bpe_kit::ensure_held(scene, reg)`**: a camera always, a mixer card only in a Balls scene. It replaces the
    `ensure_camera` + `ensure_mixer` pairs in `with_scene`, `replace_scene`, `autosave::held` and
    `video_mode::scenes::keep`.
18. **The chooser** is one modal with two steps. Step 1: Balls and Water tiles (click selects), a Name box,
    **Open a scene…**, **Cancel** (File > New only) and **Create Balls scene** or **Next** (Water). Step 2 (Water):
    seven look tiles (Plain first and selected), **Back**, **Create Water scene**. Create goes through the usual
    question about unsaved work (`guard(Pending::New(..))`). At start there is no Cancel; the blank Balls project
    sits behind the chooser as today.
19. **Names**: a Balls scene starts as "New scene" (today's), a Water scene as "Water course". The Name box
    follows the picked tile until it is typed in.
20. **Not offered in a Water scene yet, each with one plain sentence where it is switched off**: Grade mode (the
    mode switch's Grade chip is disabled; `D` and "Open in Grade mode" are caught by one guard), Export video
    (button disabled), Lights and Mixer tabs and the Rules view (hidden), adding the scene to a video (refused),
    the command-line renderer (refuses with an error).
21. **The Water stage shell** (layout D): tool rail down the stage's left edge with all nine tools in the spec's
    order; Select, Pour, Drain, Lamp and Force work, Carve, Piece, Pen and Camera path are disabled with a hover
    line until their part lands. A strip above the stage holds the active tool's one-line hint. The stage shows
    the course's outline fitted to the room, the camera's shot as a 9:16 frame, and a labelled marker for each
    Pour, Drain, Force, Lamp, Piece and Wall. Select picks a marker by click and moves it by drag (snapping to the
    grid when Snap is on); the four placing tools add a thing where the stage is clicked and go back to Select.
    The toolbar shows a **Water scene** chip. Draft | Final and Make final are part 8's and are not shown.
22. **Inspector in a Water scene**: cards in `shown_in` order; + Add card lists only Water slots; no Attach row
    (nothing rides); no Placement card on the Course; no own card for a kind with no own settings; no Follow
    entries in a setting's right-click menu.

## Things not to do

- Do not add `if water` checks inside `preview.rs`, `timeline/`, `shape_handles.rs` or the look editor: they are
  not reached, or work as they are on an empty playback.
- Do not give the Water kinds a `ThingSim` or a `ThingDraw`.
- Do not reorder or renumber existing `SlotId`s, and do not change any existing setting's key, default or range.
- Do not make `ModuleKind` carry a scene kind: the slot and `fits(thing_kind)` already decide.
- Do not write a migration: a format 12 file is a Balls scene as it stands.

---

## File structure

| File | Responsibility |
| --- | --- |
| `crates/bpe-core/src/sim.rs` (new) | `SimKind` |
| `crates/bpe-core/src/ids.rs` | the ten slots, `in_sim`, `shown_in` |
| `crates/bpe-core/src/scene.rs` | `Scene.sim`, `FORMAT = 13` |
| `crates/bpe-core/src/registry.rs` | `ThingKind::in_sim`, `ThingKind::offered` |
| `crates/bpe-core/src/edit.rs`, `clip.rs`, `resolve.rs` | the rule: add, paste, resolve |
| `crates/bpe-core/src/settings.rs` | the new sections |
| `crates/bpe-core/src/testkit.rs` | a Water test kind (`pool`), scene and folder in both |
| `crates/bpe-kit/src/modules/water/` (new) | the fourteen card kinds and their settings |
| `crates/bpe-kit/src/things/water/mod.rs` (new) | the eight thing kinds, `StartingLook`, `new_water_scene` |
| `crates/bpe-kit/src/things/{scene,camera,folder}/mod.rs` | `in_sim`, `offered`, the root's new slots |
| `crates/bpe-kit/src/lib.rs` | registration, `ensure_held` |
| `crates/bpe-kit/tests/restarts.rs` | skips kinds that are not Balls kinds |
| `crates/bpe-cli/src/lib.rs`, `arranged.rs`, `main.rs` | refuse a Water scene |
| `crates/bpe-studio/src/app/{mod,runs,files,playing,panels,shortcuts,actions,autosave}.rs` | the stand-in, the chooser's wiring, what a Water scene hides |
| `crates/bpe-studio/src/water_mode/{mod,chooser,rail,course_view}.rs` (new) | the chooser, the stage shell |
| `crates/bpe-studio/src/style/chooser.rs` (new) | the chooser pictures' colours |
| `crates/bpe-studio/src/inspector/*`, `tree.rs`, `widgets/mod.rs`, `transport.rs`, `follow.rs`, `grade_mode/mod.rs`, `video_mode/{scenes,files}.rs` | filters by scene kind |
| `docs/notes/water.md` (new), `CLAUDE.md`, `docs/notes/engine.md` | notes |

---

## Task 1: the core (`bpe-core`)

**Files:** create `sim.rs`; modify `lib.rs`, `ids.rs`, `scene.rs`, `registry.rs`, `edit.rs`, `clip.rs`,
`resolve.rs`, `settings.rs`, `save.rs` (tests), `testkit.rs`, `rules/tests.rs` (the pinned "format: 12").

**Produces:**
- `pub enum SimKind { Balls, Water }`, `SimKind::ALL: [SimKind; 2]`, `SimKind::name(self) -> &'static str`.
- `Scene.sim: SimKind`; `Scene::FORMAT == 13`.
- `SlotId::{Water, WaterLight, Back, Form, Material, Moves, Feel, Mixing, Heat, Main}`;
  `SlotId::in_sim(self, sim: SimKind) -> bool`; `SlotId::shown_in(sim: SimKind) -> &'static [SlotId]`.
- `ThingKind::in_sim(&self, sim: SimKind) -> bool` (default `sim == SimKind::Balls`);
  `ThingKind::offered(&self, def: &SettingDef, sim: SimKind) -> bool` (default `true`).
- `EditError::WrongScene(KindId, SimKind)`, shown as `A Water scene can't have a "balls"`.
- `ProblemKind::WrongScene(KindId)`.
- `SECTION_ORDER` gains, after "Health": "Course", "Look", "Back", "Material", "Joint", "Body", "Glow",
  "Crust", "Drawn", "Extras", "Pour", "Drain", "Force", "Lamp".
- testkit: `PoolKind` (`"pool"`, Water only, slots `[Feel]`), scene and folder kinds in both.

- [ ] **Step 1: failing tests** (in each file's `tests`):

```rust
// sim.rs
#[test] fn a_scene_is_balls_unless_it_says_water() {
    assert_eq!(SimKind::default(), SimKind::Balls);
    assert_eq!(SimKind::ALL.map(SimKind::name), ["Balls", "Water"]);
}
// ids.rs (replacing the 12-slot asserts' length; the old places stay asserted)
assert_eq!(SlotId::ALL.len(), 22);
assert_eq!(SlotId::ALL[12], SlotId::Water);
assert_eq!(SlotId::ALL[21], SlotId::Main);
#[test] fn every_slot_belongs_to_a_scene_kind_and_shows_once() {
    for s in SlotId::ALL {
        assert!(SimKind::ALL.iter().any(|k| s.in_sim(*k)), "{s:?}");
    }
    for sim in SimKind::ALL {
        let shown = SlotId::shown_in(sim);
        let want: Vec<SlotId> = SlotId::ALL.into_iter().filter(|s| s.in_sim(sim)).collect();
        let mut sorted = shown.to_vec(); sorted.sort();
        assert_eq!(sorted, want, "{sim:?} shows each of its slots once");
    }
    assert_eq!(SlotId::shown_in(SimKind::Balls), &SlotId::ALL[..12]);
    assert!(SlotId::Look.in_sim(SimKind::Water) && !SlotId::Mixer.in_sim(SimKind::Water));
    assert!(!SlotId::Feel.in_sim(SimKind::Balls));
}
// save.rs
#[test] fn a_file_with_no_sim_is_a_balls_scene_and_balls_files_gain_no_line() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let text = save(&s);
    assert!(!text.contains("sim"), "{text}");
    let old = text.replacen("format: 13", "format: 12", 1);
    assert_eq!(load(&old, &r).unwrap().sim, SimKind::Balls);
    s.sim = SimKind::Water;
    let back = load(&save(&s), &r).unwrap();
    assert_eq!(back.sim, SimKind::Water);
    assert_eq!(back, s);
}
// newer_files_are_refused now loads "(format: 14)"
// edit.rs
#[test] fn a_thing_goes_only_in_its_own_kind_of_scene() {
    let r = testkit::registry();
    let mut balls = Scene::empty(&r);
    assert_eq!(balls.add_thing(&r, "pool", ThingId::ROOT), Err(EditError::WrongScene("pool".into(), SimKind::Balls)));
    let mut water = Scene { sim: SimKind::Water, ..Scene::empty(&r) };
    let e = water.add_thing(&r, "balls", ThingId::ROOT).unwrap_err();
    assert_eq!(e.to_string(), "A Water scene can't have a \"balls\"");
    assert!(water.add_thing(&r, "pool", ThingId::ROOT).is_ok());
    assert!(water.add_thing(&r, "folder", ThingId::ROOT).is_ok());
}
// clip.rs: copy a "balls" from a Balls scene, paste into a Water scene: Ok(Pasted) with no tops, scene unchanged.
// resolve.rs
#[test] fn a_thing_of_the_other_scene_kind_is_reported_and_left_out_with_its_children() {
    let r = testkit::registry();
    let mut s = Scene::empty(&r);
    let b = testkit::push(&mut s, "balls", ThingId::ROOT);
    let under = testkit::push(&mut s, "folder", b);
    s.sim = SimKind::Water;
    let out = resolve(&s, &r);
    assert!(out.things.contains_key(&ThingId::ROOT));
    assert!(!out.things.contains_key(&b) && !out.things.contains_key(&under));
    assert_eq!(out.problems, [Problem { thing: b, slot: None, kind: ProblemKind::WrongScene("balls".into()) }]);
}
#[test] fn a_card_in_the_other_scene_kinds_slot_is_ignored() {
    // a "pool" in a Water scene with a Feel card resolves it; the same scene marked Balls reports the pool
    // (WrongScene); a root Mixer card in a Water scene is not in `resolved.things[ROOT].slots` and adds no problem.
}
```

- [ ] **Step 2: run** `cargo test -p bpe-core sim ids save edit clip resolve` : the new tests fail to compile.
- [ ] **Step 3: implement.** `add_thing` checks `in_sim` right after the kind is found, before `OnlyOne`.
  `paste_things` adds `!kind.in_sim(self.sim)` to the skip test beside `one_here`. `resolve` checks after the
  unknown-kind check, `id != ThingId::ROOT`, and filters cards with `s.in_sim(scene.sim)`. `mend_loaded` is
  unchanged.
- [ ] **Step 4: run** `cargo test -p bpe-core` : all pass (the whole crate is quick and all of it is touched).
- [ ] **Step 5: commit** "Water mode part 1: scene kind, new slots and the belongs-to rule in the core".

## Task 2: the kit (`bpe-kit`)

**Files:** create `modules/water/{mod,scene,solids,liquid,places}.rs`, `things/water/mod.rs`; modify
`modules/mod.rs`, `things/mod.rs`, `things/scene/mod.rs`, `things/camera/mod.rs`, `things/folder/mod.rs`,
`lib.rs`, `tests/restarts.rs`.

**Consumes:** Task 1's `SimKind`, slots, `in_sim`, `offered`, sections.

**Produces:**
- `bpe_kit::things::water::{COURSE, PIECE, WALL, LIQUID, POUR, DRAIN, FORCE, LAMP}` (the ids in decision 10),
  `KINDS: [&str; 8]`, `is_placed(kind: &str) -> bool` (every one but the Course and the Liquid),
  `StartingLook` (`Choice`; `ALL: [StartingLook; 7]`, `title()`, `words()` one line from the spec's table,
  `swatch() -> (Color, Color)` liquid and solid), `new_water_scene(reg, look: StartingLook, name: &str) -> Scene`,
  `course_of(scene) -> Option<ThingId>`.
- `bpe_kit::modules::water::{WORLD, LIGHT, BACK, MATERIAL, SHAPE, LINE, MOVES, FEEL, LOOK, MIXING, HEAT, POUR,
  DRAIN, FORCE, LAMP}` (card ids), the settings structs `WaterWorld`, `WaterLightCard`, `BackCard`,
  `MaterialCard`, `PieceShape`, `WallLine`, `MovesCard`, `FeelCard`, `WaterLook`, `MixingCard`, `HeatCard`,
  `PourCard`, `DrainCard`, `ForceCard`, `LampCard`, `register(&mut Registry)`,
  `WaterWorld::screens(&self) -> (u32, u32)`, `WaterWorld::course_size(&self) -> Vec2`, `SCREEN: Vec2 = (1080, 1920)`.
- `bpe_kit::ensure_held(scene: &mut Scene, reg: &Registry)`.
- `bpe_kit::things::scene::root_cards(sim: SimKind) -> &'static [&'static str]` (Balls: `ROOT_CARDS`; Water:
  `["Video", "Picture", "Sound"]`).

**The things** (all: no own settings, `can_stick_to_screen` false, `in_sim` Water only):

| id | name | single | family | slots | default slots | slot labels |
| --- | --- | --- | --- | --- | --- | --- |
| `water_course` | Course | yes | Other | Material | Material | Material = "Rock" |
| `water_piece` | Piece | no | Course | Form, Material, Moves | all three | Form = "Shape" |
| `water_wall` | Wall | no | Course | Form, Material, Moves | all three | Form = "Line" |
| `water_liquid` | Liquid | no | Liquids | Feel, Look, Mixing, Heat | Feel, Look, Mixing | |
| `water_pour` | Pour | no | Liquids | Main | Main | Main = "Pour" |
| `water_drain` | Drain | no | Liquids | Main | Main | Main = "Drain" |
| `water_force` | Force | no | Forces | Main | Main | Main = "Force" |
| `water_lamp` | Lamp | no | Lights | Main | Main | Main = "Lamp" |

Scene settings' slots become `[Shader, Lighting, Air, Mixer, Grade, Water, WaterLight, Back]`, with
`WaterLight` labelled "Light".

**The cards** (key: label, type, default; range/span; flags). Carried-over settings keep the old Liquid thing's
label, range and span. No setting has `pulse`. Every group is a section.

- `water.world` (Water, fits scene), "Water":
  Detail: `draft_detail` Draft detail, `DraftDetail {Coarse, Normal*, Fine}` (12, 8, 6 px cells), simple;
  `final_detail` Final detail, `FinalDetail {Coarse, Normal*, Fine}` (3, 2, 1.5 px), simple.
  Course: `gravity` Gravity, Vec2 (0, 4000) px/s², simple, keyframe (today's default liquid falls at 8 x 500);
  `screens_wide` Screens wide, u32 1, range 1..8, simple; `screens_tall` Screens tall, u32 1, range 1..8, simple.
  Look: `look` Starting look, `StartingLook` Plain.
- `water.light` (WaterLight, fits scene), "Light":
  Light: `ambient` Ambient 0.3, range 0..1e9 span 0..2, simple, keyframe; `bounce` Bounce 0.3, span 0..1,
  keyframe; `caustics` Caustics 1, span 0..2, simple, keyframe; `shafts` Shafts 0, span 0..1, keyframe.
  Quality: `quality` Quality, `LightQuality {Draft, Good*, Best}`.
- `water.back` (Back, fits scene), "Back":
  Back: `shows` Shows, `BackShows {DarkerRock*, Colour, Gradient, Picture}`, simple; `darker` Darker 0.5, range
  0..1, when shows=darker_rock; `colour` Colour (0x14, 0x18, 0x1e), when shows=colour, keyframe; `gradient`
  Gradient (dark blue to near black), when shows=gradient, keyframe; `angle` Direction 90 °, span -180..180, when
  shows=gradient; `picture` Picture, String, unit "picture", when shows=picture.
- `water.material` (Material, fits course, piece, wall), "Material":
  Material: `material` Material, `MaterialKind {Rock*, Glass, Paper, Basalt, Flat}`, simple; `colour` Colour
  (0x6b, 0x5e, 0x52), simple, keyframe; `second_colour` Second colour (0x3d, 0x35, 0x2f), keyframe;
  `texture_size` Texture size 120 px, range 1..1e9 span 10..400; `depth` Depth 12 px, range 0..1e9 span 0..40;
  `roughness` Roughness 0.6, range 0..1; `outline` Outline 0 px, range 0..1e9 span 0..10; `outline_colour`
  Outline colour black, when outline; `wet_marks` Wet marks 0.5, range 0..1.
- `water.shape` (Form, fits piece), "Shape":
  Shape: `piece` Piece, `PieceKind {Ramp*, Funnel, Pipe, Sieve, Basin, Step, Pillar, Wheel, Bucket, Gate, Float,
  Custom}`, simple; `size` Size (300, 200) px, simple; `flip` Flip, bool; `cuts` Cuts, bool; `path` Outline,
  `ShapePath`, when piece=custom.
- `water.line` (Form, fits wall), "Line":
  Shape: `points` Points, `Vec<[f32; 2]>` `[[-150, 0], [150, 0]]`; `thickness` Thickness 24 px, range 1..1e9
  span 2..200, simple; `curved` Curved, bool; `ends` Ends, `LineEnds {Round*, Square}`; `cuts` Cuts, bool.
- `water.moves` (Moves, fits piece, wall), "Moves":
  Motion: `moves` Moves, `MovesKind {Fixed*, Keyframed, Driven}`, simple; `offset` Position (0, 0) px, keyframe,
  when moves=keyframed; `turn` Turn 0 °, span -360..360, keyframe, when moves=keyframed; `scale` Scale 1, range
  0.01..100 span 0.1..4, keyframe, when moves=keyframed.
  Joint (each `when` starts `moves=driven`): `joint` Joint, `Joint {Free*, Pin, Slide}`; `weight` Weight 1, range
  0.01..1e9 span 0.1..10; `drag` Drag 0.1, range 0..1e9 span 0..1; `spring` Spring 0, range 0..1e9 span 0..1;
  `stops` Stops, bool; `stop_low` Stop from -90 and `stop_high` Stop to 90, span -360..360, and stops; `motor`
  Motor 0, span -4..4; `catches` Catches liquid, bool; `tips_when` Tips when 0.8, range 0..1, and catches.
- `water.feel` (Feel, fits liquid), "Feel":
  How it flows: `thickness` 0.02, range 0..1e9 span 0..1, simple, keyframe; `surface_pull` Surface pull 0.2,
  range 0..1e9 span 0..1, keyframe; `weight` Weight 1, range 0.05..1e9 span 0.1..10, simple, keyframe;
  `stickiness` Stickiness 0, range 0..1e9 span 0..1, keyframe.
- `water.look` (Look, fits liquid), "Look", struct `WaterLook`:
  Colours: `colour` Colour (0x2f, 0x9b, 0xff), simple, keyframe; `colour_from` Colour from, `ColourFrom {ItsOwn*,
  Speed, Heat, Age, Depth}`, simple; `colour_gradient` Gradient (blue to white), when colour_from!=its_own.
  Body: `clarity` Clarity 200 px, range 0..1e9 span 0..600, simple, keyframe; `deep` Deep colour (0x0b, 0x2a,
  0x4f), when clarity; `solid` Solid 0, range 0..1, keyframe; `bend` Bend 12 px, range 0..1e9 span 0..60,
  keyframe; `split` Rainbow split 0, range 0..1, when bend, keyframe.
  On its surface: `smoothing` Smoothing 0.5, range 0..1e9 span 0..1, keyframe; `streaks` Streaks 1, range 0..1e9
  span 0..4, keyframe; `shine` Shine 0.5, range 0..1, keyframe; `shine_size` Shine size 0.3, range 0..1;
  `edge_sheen` Edge sheen 0.3, range 0..1, keyframe; `ripples` Ripples 0, range 0..1e9 span 0..1, keyframe;
  `thin_film` Thin film 0, range 0..1, keyframe.
  Glow: `glow` Gives off light 0, range 0..1e9 span 0..4, keyframe; `glow_colour` Glow colour white, when glow;
  `glow_from` Glow from, `GlowFrom {Always*, Heat, Speed}`, when glow.
  Crust: `crust` Amount 0, range 0..1, keyframe; `crust_colour` Colour (0x2a, 0x22, 0x20), when crust;
  `crust_forms_in` Forms in 3 s, range 0.05..600 span 0.1..20, when crust; `crust_cracks` Cracks 0.5, range 0..1,
  when crust.
  Drawn: `outline` Outline 0 px, range 0..1e9 span 0..10, keyframe; `outline_colour` Outline colour black, when
  outline; `bands` Bands 0, u32 range 0..16; `band_softness` Band softness 0.2, range 0..1, when bands.
  Extras: for each of `foam`, `bubbles`, `mist`, `sparks`: `<x>` (Foam, Bubbles, Mist, Sparks) 0, range 0..1,
  keyframe; `<x>_size` (Foam size ...) 6 px, range 0.5..1e9 span 1..40, when `<x>`; `<x>_colour` white (sparks
  (0xff, 0xb0, 0x40)), when `<x>`; `<x>_life` 1 s, range 0.05..60 span 0.1..5, when `<x>`.
  Colour mixing: `swirl_detail` Swirl detail 0.5, range 0..1, keyframe; `shows_as` Shows as, `MixShows
  {Shimmer*, Glow, Foam}`.
- `water.mixing` (Mixing, fits liquid), "Mixing":
  Colour mixing: `mix_speed` Mix speed 0.5, range 0..1, simple; `fades` Fades 0 /s, range 0..1e9 span 0..1;
  `fades_to` Fades to white, when fades.
  Un-mix: `remember` Remember for Un-mix, bool; `unmix_seconds` Seconds 5 s, range 0.1..60 span 1..20, when
  remember; `unmixing` Un-mixing, bool, when remember.
- `water.heat` (Heat, fits liquid), "Heat": the old Liquid thing's Heat group as it is (`temperature`, `cools`,
  `warms`, `cold_thickens`, `freezes`, `freezes_below`, `melts_above`, `dries`, `stains`, `stain_strength`), with
  the same labels, defaults, ranges and conditions, `keyframe` kept and `pulse` dropped.
- `water.pour` (Main, fits pour), "Pour":
  Pour: `liquid` Liquid, `Target`, default `Target::Kind("water_liquid")`; `kind` Kind, `PourKind {Tap*, Rain,
  Drip, Burst, FillAtTheStart}`, simple; `rate` Rate 1 x, range 0..1e9 span 0..4, simple, keyframe; `width` Width
  40 px, range 0..1e9 span 1..400, simple, keyframe, when kind!=fill_at_the_start; `speed` Speed 300 px/s, range
  0..1e9 span 0..2000, keyframe, when kind!=fill_at_the_start; `aim` Aim 90 °, span -180..180, keyframe, when
  kind!=fill_at_the_start; `spread` Spread 0 °, range 0..180 span 0..45, keyframe, when kind!=fill_at_the_start;
  `fill_size` Fill size (300, 200) px, when kind=fill_at_the_start; `starts` Starts at 0 s, range 0..600; `stops`
  Stops at 600 s, range 0..600.
- `water.drain` (Main, fits drain), "Drain":
  Drain: `kind` Kind, `DrainKind {Drain*, Leak, EdgeOfTheCourse}`, simple; `radius` Radius 40 px, range 0..1e9
  span 1..200, simple, keyframe, when kind!=edge_of_the_course; `rate` Rate 1 x, range 0..1e9 span 0..4,
  keyframe; `whirlpool` Whirlpool 0, range 0..1e9 span 0..4, keyframe, when kind=drain; `edge` Edge,
  `CourseEdge {Bottom*, Top, Left, Right, All}`, when kind=edge_of_the_course.
- `water.force` (Main, fits force), "Force":
  Force: `kind` Kind, `ForceKind {Magnet*, Whirlpool, Wind, Heater, Cooler}`, simple; `strength` Strength 1,
  span -4..4, simple, keyframe; `reach` Reach 300 px, range 0..1e9 span 10..1500, keyframe, when kind!=wind;
  `aim` Aim 0 °, span -180..180, keyframe, when kind=wind; `spikes` Spikes 0, u32 range 0..64, when kind=magnet;
  `spike_size` Spike size 0.5, range 0..1, when kind=magnet and spikes.
- `water.lamp` (Main, fits lamp), "Lamp":
  Lamp: `kind` Kind, `LampKind {Spot*, Strip, Sun}`, simple; `colour` Colour (0xff, 0xe2, 0xb8), simple,
  keyframe; `brightness` Brightness 1, range 0..1e9 span 0..4, simple, keyframe; `aim` Aim 90 °, span -180..180,
  keyframe; `spread` Spread 60 °, range 1..360, keyframe, when kind=spot; `length` Length 400 px, range 1..1e9
  span 10..2000, keyframe, when kind=strip; `softness` Softness 0.3, range 0..1; `shadows` Shadows, bool true.

- [ ] **Step 1: failing tests** in `things/water/mod.rs` and `modules/water/mod.rs`:

```rust
#[test] fn a_new_water_scene_is_settings_a_camera_and_the_course() {
    let reg = crate::registry();
    let scene = new_water_scene(&reg, StartingLook::Neon, "Cave run");
    assert_eq!((scene.sim, scene.name.as_str()), (SimKind::Water, "Cave run"));
    let kinds: Vec<&str> = scene.order.iter().map(|id| scene.things[id].kind.as_str()).collect();
    assert_eq!(kinds, ["scene", "camera", COURSE]);
    let root: Vec<_> = scene.things[&ThingId::ROOT].slots.iter().map(|(s, c)| (*s, c.kind.as_str())).collect();
    assert_eq!(root, [(SlotId::Water, WORLD), (SlotId::WaterLight, LIGHT), (SlotId::Back, BACK)]);
    let resolved = bpe_core::resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let w = WaterWorld::from_params(&resolved.things[&ThingId::ROOT].slots[&SlotId::Water].params);
    assert_eq!(w.look, StartingLook::Neon);
    assert_eq!(bpe_core::load(&bpe_core::save(&scene), &reg).unwrap(), scene, "it saves and loads");
    assert_eq!(new_water_scene(&reg, StartingLook::Plain, "").name, "Water course");
}
#[test] fn each_water_kind_starts_with_its_own_cards() {
    // table of (kind, [(slot, card id)]) from the things table above; add_thing into a new Water scene,
    // assert the cards, every card's params empty, resolve has no problems, and
    // reg.kinds_fitting(slot, kind).count() == 1 for every slot the kind has.
}
#[test] fn water_kinds_go_only_in_water_scenes_and_ball_kinds_only_in_balls_scenes() {
    let reg = crate::registry();
    let mut balls = crate::new_studio_scene(&reg);
    let mut water = new_water_scene(&reg, StartingLook::Plain, "");
    for kind in reg.things().filter(|k| !["scene", "camera"].contains(&k.id())) {
        let in_water = KINDS.contains(&kind.id()) || kind.id() == "folder";
        let in_balls = !KINDS.contains(&kind.id());
        if kind.id() == COURSE { continue; } // the scene has its one
        assert_eq!(crate::add_thing(&mut water, &reg, kind.id(), ThingId::ROOT).is_ok(), in_water, "{}", kind.id());
        assert_eq!(crate::add_thing(&mut balls, &reg, kind.id(), ThingId::ROOT).is_ok(), in_balls, "{}", kind.id());
    }
    assert_eq!(crate::add_thing(&mut water, &reg, COURSE, ThingId::ROOT), Err(EditError::OnlyOne(COURSE.into())));
}
#[test] fn a_water_scene_builds_an_empty_ball_world() {
    // every Water kind added; World::build(&scene, &resolved, &sims()): bodies empty, advance_to(1.0) logs nothing,
    // fingerprint and beside_fingerprint run without panicking.
}
#[test] fn the_course_is_at_most_sixteen_screens() {
    let w = WaterWorld { screens_wide: 8, screens_tall: 8, ..WaterWorld::default() };
    assert_eq!(w.screens(), (8, 2));
    assert_eq!(WaterWorld::default().course_size(), Vec2::new(1080.0, 1920.0));
    assert_eq!(WaterWorld { screens_wide: 0, ..WaterWorld::default() }.screens(), (1, 1));
}
#[test] fn water_settings_follow_nothing_and_vary_nothing() {
    // every setting of every water.* card: !flags.pulse; every Water thing kind and card: vary(key) is None.
}
#[test] fn scene_settings_and_the_camera_hide_their_ball_settings_in_a_water_scene() {
    // SceneKind.offered(def, Water) is true exactly for groups Video, Picture, Sound; all true for Balls.
    // CameraKind.offered(def, Water) true exactly for groups Shot, World box.
    // root_cards(Water) == ["Video", "Picture", "Sound"]; root_cards(Balls) == ROOT_CARDS.
}
#[test] fn ensure_held_gives_a_water_scene_a_camera_and_no_mixer() { /* and a Balls scene both, idempotent */ }
```

  Existing tests updated in the same step: `scene_settings_keep_their_effects_slot_to_themselves` (the new
  slots), `every_addable_thing_has_a_family` (families gain "Course" and "Forces"),
  `the_look_registry_has_a_look_for_...` (the list gains `water.look`).
- [ ] **Step 2: run** `cargo test -p bpe-kit --lib water` : fails to compile.
- [ ] **Step 3: implement** the cards with a small `card!` macro in `modules/water/mod.rs` (id, name, help, slot,
  settings type, fitting thing kinds) and the things from one `WaterKind` struct of consts. Register after the
  old Liquid's cards in `registry()`. `new_water_scene` uses `Scene::empty`, sets `sim` first, then
  `ensure_camera`, `add_slot` for the three root cards, `add_thing(COURSE)`; a look other than Plain is written
  to the Water card's `look`.
- [ ] **Step 4: gate the kit-wide test.** `tests/restarts.rs`: the kinds loop takes
  `reg.things().filter(|k| k.in_sim(SimKind::Balls))`. (The Water twin of this test is part 2's, when a draft
  exists to restart.)
- [ ] **Step 5: run** `cargo test -p bpe-kit --lib -- water starters scene camera the_blank every_ kinds_fitting
  the_look_registry ensure_` and `cargo test -p bpe-kit --lib every_kit_plugin_passes_the_contract
  every_module_setting_has_a_known_section angles_say` : all pass. `cargo test -p bpe-kit --test restarts
  --no-run` compiles (its run is CI's: about 40,000 short runs).
- [ ] **Step 6: commit** "Water mode part 1: the Water things and cards as data".

## Task 3: the command-line renderer (`bpe-cli`)

**Files:** modify `src/lib.rs`, `src/arranged.rs`.

**Produces:** `pub fn check_exportable(scene: &Scene) -> anyhow::Result<()>`, an error for a Water scene:
"Water scenes can't be exported yet. Export arrives with Make final." Called first in `export_video`,
`final_frame`, `preview_clip` and, for every kept scene, in `export_arranged`.

- [ ] **Step 1: failing test** (`lib.rs` tests, no graphics device needed): `check_exportable` is `Ok` for
  `new_studio_scene` and an `Err` whose text is the sentence above for `new_water_scene`.
- [ ] **Step 2: run** `cargo test -p bpe-cli check_exportable` : fails. **Step 3:** implement. **Step 4:** passes.
- [ ] **Step 5: commit** "Water mode part 1: the renderer refuses a Water scene for now".

## Task 4: the editor holds a Water scene (`bpe-studio`, no new screens yet)

**Files:** modify `app/mod.rs`, `app/runs.rs`, `app/files.rs`, `app/playing.rs`, `app/autosave.rs`,
`app/panels.rs`, `app/shortcuts.rs`, `app/actions.rs`, `transport.rs`, `timeline/mod.rs` (one read of
`studio.bottom`), `grade_mode/mod.rs` (`mode_switch`), `video_mode/scenes.rs`, `video_mode/files.rs`,
`tree.rs` (`problem_words`); tests in `app/tests.rs`.

**Produces:**
- `Studio.no_balls: (Scene, Resolved)`; `runs::ball_side<'a>(scene: &'a Scene, resolved: &'a Resolved, none:
  &'a (Scene, Resolved)) -> (&'a Scene, &'a Resolved)`.
- `Studio::water(&self) -> bool`; `Studio::bottom_view(&self) -> settings::BottomView` (Timeline in a Water
  scene, without changing the saved choice).
- `Studio::headless_water() -> Studio` (tests): `with_scene` on `new_water_scene(Plain, "")`.
- `Studio::keep_out_of_grade(&mut self)`: in a Water scene, Grade mode goes back to Build with the status
  "Grading a Water scene arrives with its light and looks."; called each frame right after `shortcuts`.
- `grade_mode::mode_switch(ui, mode, grade_ok: bool)`.

- [ ] **Step 1: failing tests** (`app/tests.rs`):

```rust
#[test] fn a_water_scene_starts_no_ball_jobs_and_keeps_no_mixer() {
    let studio = Studio::headless_water();
    assert!(studio.water());
    assert!(studio.scene.card(ThingId::ROOT, SlotId::Mixer).is_none());
    assert!(studio.aim_job.is_none() && studio.particle_job.is_none());
    assert!(studio.follow.is_none() && studio.light_probe.is_none());
    assert!(studio.playback.world().bodies.is_empty());
}
#[test] fn editing_a_water_scene_restarts_nothing_and_undo_works() {
    let mut studio = Studio::headless_water();
    studio.sound = Some(SoundJob::recording());
    let kit = studio.kit.clone();
    let (print, heard) = (studio.fingerprint, studio.sound_fingerprint);
    let before = studio.scene.clone();
    let pour = bpe_kit::add_thing(&mut studio.scene, &kit.registry, bpe_kit::things::water::POUR, ThingId::ROOT).unwrap();
    studio.after_edit(before.clone());
    assert_eq!((studio.fingerprint, studio.sound_fingerprint), (print, heard));
    assert!(studio.sound.as_ref().unwrap().recorded().is_empty(), "the sound is left alone");
    assert!(studio.resolved.things.contains_key(&pour));
    assert!(studio.undo());
    assert_eq!(studio.scene, before);
}
#[test] fn opening_a_water_scene_stops_the_ball_jobs_and_a_balls_scene_starts_them_again() {
    // headless() with an emitter that aims and a Particles thing (as an_air_edit_... builds them), sound recording:
    // replace_scene(new_water_scene) -> aim_job and particle_job None, recorded() == ["rebuild"], playback empty;
    // replace_scene(the first scene) -> particle_job is Some again and the fingerprint equals the first one.
}
#[test] fn a_water_scene_cannot_reach_grade_mode_or_a_video() {
    let mut studio = Studio::headless_water();
    studio.mode = crate::grade_mode::EditorMode::Grade;
    studio.keep_out_of_grade();
    assert_eq!(studio.mode, crate::grade_mode::EditorMode::Build);
    assert!(studio.status.contains("Grading a Water scene"));
    studio.new_video();
    assert!(studio.video.video.scenes.is_empty(), "a new video from a Water scene starts empty");
}
#[test] fn a_water_scene_s_toolbar_and_panels_hide_what_it_has_not_got() {
    // toolbar harness on headless_water(): "Water scene" is there; "Export video…" and "Grade" are disabled;
    // left_panel harness: no "Lights" and no "Mixer" label; transport::controls harness: no "Rules" label.
    // The same three harnesses on Studio::headless(): all of them present and enabled (Balls is unchanged).
}
#[test] fn pasting_ball_things_into_a_water_scene_says_why_not() {
    // copy the ball in headless(), move the clipboard to headless_water(), paste_things(None) is false and
    // status == "Nothing to paste: those things don't go in a Water scene."
}
```

- [ ] **Step 2: run** `cargo test -p bpe-studio --bin ball-studio-next water` : fails to compile.
- [ ] **Step 3: implement.**
  - `with_scene`: `ensure_held`; build `no_balls`; take `(ball, ball_resolved)` from `ball_side` and build the
    fingerprints, aim job, playback, particle print and job, follow and light probe from them. `self.resolved`
    stays the real scene's.
  - `replace_scene`, `scene_changed`, `restart_runs`, `follow_bakes`, `keep_speakers` and `Studio::new` (the two
    `SoundJob::start`s): the same substitution. `runs_things` stays the real scene's keys.
  - `autosave::held` and `video_mode::scenes::keep`: `ensure_held`. `keep` first refuses a Water scene: status
    "Water scenes can't go in a video yet." and nothing is kept (it returns `Option<KeptId>`; its three callers
    take the `Some`). `new_video`: a Water scene gives `Video::new()` and the status "Started a new video. Water
    scenes can't go in a video yet."
  - `left_panel`: in a Water scene `self.left` is Things and `left_switch` is not drawn.
  - `transport::controls`, `bottom_panel`, `timeline::show`: read `studio.bottom_view()`; the Timeline | Rules
    chips are not drawn in a Water scene.
  - `toolbar`: Export is `add_enabled(!water || video mode)` with the disabled hover "Exporting a Water scene
    arrives with Make final."; `mode_switch(.., !self.water())` disables Grade with "Grading a Water scene
    arrives with its light and looks."; in a Water scene (not Video mode) a **Water scene** chip
    (`style::chip`-like label, named for kittest) sits before the scene's name.
  - `paste_things`: when nothing was pasted and some copied thing's kind is not `in_sim` here, the status is the
    sentence in the test; otherwise today's words.
  - `tree::problem_words`: `WrongScene(k)` reads "<kind name> doesn't go in a <Water|Balls> scene" (it takes
    the scene kind from a new `sim` argument, or falls back to "this kind of scene" where it has no scene).
- [ ] **Step 4: run** `cargo test -p bpe-studio --bin ball-studio-next -- water a_physics_edit an_air_edit
  no_device` : pass (the last three show the Balls path is as it was).
- [ ] **Step 5: commit** "Water mode part 1: the editor holds a Water scene with the ball engine empty".

## Task 5: the chooser

**Files:** create `water_mode/mod.rs`, `water_mode/chooser.rs`, `style/chooser.rs`; modify `main.rs` (`mod
water_mode`), `style/mod.rs`, `app/mod.rs`, `app/files.rs`, `app/dialogs.rs`, `app/shortcuts.rs`,
`app/panels.rs` (New's hover text), `app/autosave.rs` (`restore`).

**Produces:**
- `pub struct NewScene { pub sim: SimKind, pub look: StartingLook, pub name: String }`.
- `pub struct Chooser { pub step: Step, pub sim: SimKind, pub look: StartingLook, pub name: String, pub
  named: bool, pub at_start: bool }`, `Chooser::at_start()`, `Chooser::for_new()`,
  `enum Step { Kind, Look }`, `enum ChooserOutcome { Create(NewScene), Open, Cancel }`,
  `chooser::show(ctx, &mut Chooser) -> Option<ChooserOutcome>`.
- `Studio.chooser: Option<Chooser>`; `Pending::New(NewScene)`; `Studio::new_scene(&mut self, new: NewScene)`;
  `Studio::ask_new_scene(&mut self)` (opens the chooser; what New and Ctrl+N call outside Video mode);
  `Studio::chooser_dialog(&mut self, ctx)`.

**Behaviour:**
- `Studio::new` opens the chooser when no scene path was given. `blank()` and `headless()` never do.
- `chooser_dialog` draws nothing while `self.offer` (restore) or `self.confirm` is up. `restore()` clears the
  chooser; Discard leaves it. `replace_scene` clears it (a scene dropped on the window, a recent file).
- Create: the chooser closes and `guard(Pending::New(new), ctx)` runs. If the save question is then cancelled
  the scene stays and the chooser stays closed.
- Open a scene…: `ask_open(ctx)`; the chooser closes only when not at start (at start it stays until a scene
  opens).
- While the chooser is up, `shortcuts`, `late_shortcuts` and `take_drops` are skipped as for the other dialogs,
  except that drops still work at start.
- `new_scene`: Balls gives `new_studio_scene` with the name when it is not empty; Water gives
  `new_water_scene`. Status "Started a new scene." or "Started a new Water scene."
- Tile pictures are painted with the painter from `style/chooser.rs` tokens (the mockup's colours); a look's
  tile is its two swatch colours through `style::video_bytes`.

- [ ] **Step 1: failing tests** (`water_mode/chooser.rs` tests, a harness that draws `chooser::show` and stores
  the outcome; and `app/tests.rs` for the wiring):

```rust
#[test] fn balls_is_one_click_and_create() {
    // harness on Chooser::for_new(): click "Balls", click "Create Balls scene":
    // outcome == Create(NewScene { sim: Balls, look: Plain, name: "New scene" })
}
#[test] fn water_offers_plain_and_the_six_looks_then_creates() {
    // click "Water": the Name box reads "Water course"; click "Next"; the seven titles of StartingLook::ALL are
    // there and "Plain" is selected; click "Neon in the dark", click "Create Water scene":
    // outcome == Create(NewScene { sim: Water, look: Neon, name: "Water course" }). "Back" returns to step 1.
}
#[test] fn a_typed_name_is_kept_when_the_kind_changes() { /* type "Cave", click Water: name still "Cave" */ }
#[test] fn cancel_is_only_there_for_file_new() {
    // for_new(): "Cancel" exists and gives Cancel; at_start(): no "Cancel"; both have "Open a scene…".
}
// app/tests.rs
#[test] fn file_new_with_unsaved_work_asks_first_and_cancel_keeps_the_scene() {
    let mut studio = Studio::headless();
    studio.scene.name = "Mine".into(); // unsaved
    let kept = studio.scene.clone();
    studio.ask_new_scene();
    assert!(studio.chooser.is_some());
    // run the dialog with the outcome Create(Water, Plain): confirm is Some(Pending::New(_)), chooser None,
    // scene == kept; then confirm = None (Cancel): scene == kept, chooser None.
    // then perform(Pending::New(water)): studio.water(), name "Water course", path None, history empty.
}
#[test] fn the_chooser_waits_behind_the_restore_question_and_goes_when_a_scene_opens() {
    // chooser Some + offer Some: chooser_dialog draws no "New scene" heading; restore() -> chooser None.
    // chooser Some: replace_scene(any) -> chooser None.
}
```

- [ ] **Step 2: run** `cargo test -p bpe-studio --bin ball-studio-next chooser` : fails to compile.
- [ ] **Step 3: implement.** Modal id `"new-scene"`, 940 wide as the mockup. Heading "New scene" and the
  mockup's line "What does this scene simulate? Each kind has its own editor, physics and lighting, built for
  that kind alone." Tiles are `Sense::click()` rects with `widget_info` labelled by their title and `selected`.
- [ ] **Step 4: run** the two test groups: pass.
- [ ] **Step 5: commit** "Water mode part 1: the Balls or Water chooser at start and on New".

## Task 6: the inspector, the tree and the lists in a Water scene

**Files:** modify `inspector/mod.rs`, `inspector/own.rs`, `inspector/slots.rs`, `inspector/add_card.rs`,
`inspector/find.rs`, `inspector/placement.rs`, `follow.rs`, `tree.rs`, `widgets/mod.rs`; tests in
`inspector/tests.rs` and `tree.rs`.

**Consumes:** `SlotId::shown_in`, `SlotId::in_sim`, `ThingKind::in_sim`, `ThingKind::offered`,
`bpe_kit::things::scene::root_cards`, `bpe_kit::things::water::{COURSE, KINDS}`.

**Changes:**
- The three `SlotId::ALL.into_iter().filter(|s| s.in_card_list())` loops (jump bar, `slots::cards`,
  `find`) iterate `SlotId::shown_in(scene.sim).iter().copied().filter(..)`.
- `add_card::menu` also filters `s.in_sim(scene.sim)`.
- `show`'s `applies` closure also asks `kind.offered(d, scene.sim)`; `own::kind_card` and the jump bar take the
  root's cards from `root_cards(scene.sim)`.
- A thing whose kind has no own settings shows no own card in a Water scene, and its jump bar starts at
  Placement (as a Balls thing with a layered Look does). A Folder in a Balls scene is as today.
- `placement::show` skips `water_course`; `position_rows` draws the Attach row and what follows it only in a
  Balls scene.
- `follow::offered(scene) -> bool` (Balls only); `slots::slot_grid_of` and `own::own_grid` pass `follow: None`
  when it is false.
- `tree.rs`: the Add menu's kinds also filter `k.in_sim(scene.sim)`; `FAMILIES` gains "Course" and "Forces"
  (after "Liquids"); `icon_shape` gives each Water kind an icon (Course `Square`, Piece and Wall `Shape`, Liquid
  and Pour `Drop`, Drain `Emitter`, Force `Specks`, Lamp `Lamp`).
- `widgets::TargetChoices::from_scene`: `kinds` also filters `k.in_sim(scene.sim)`.

- [ ] **Step 1: failing tests:**

```rust
// tree.rs
#[test] fn a_water_scene_s_add_menu_lists_only_water_things() {
    // water harness: click "+ Add thing": "Piece", "Wall", "Liquid", "Pour", "Drain", "Force", "Lamp", "Folder"
    // are there; "Balls", "Shape", "Emitter", "Course" (the scene has it) are not. Click "Pour": a water_pour
    // is added with its Pour card.
    // the existing Balls harness: none of "Piece", "Wall", "Pour", "Drain", "Force", "Lamp" is listed.
}
// inspector/tests.rs
#[test] fn a_water_scene_s_root_shows_its_own_cards_and_hides_the_ball_ones() {
    // water harness, root selected: headings "Video", "Picture", "Sound", "Water", "Light", "Back" exist;
    // "World", "Background", "Lighting" do not; "+ Add card" lists "Effects" only (Grade is not in the list).
}
#[test] fn a_liquid_s_cards_read_feel_look_mixing_and_heat_can_be_added() {
    // add a water_liquid, select it: card titles in y order are Feel, Look, Mixing; no "Liquid" own card;
    // "+ Add card" offers "Heat" and nothing else; a slider of the Feel card (Thickness) changes the scene.
}
#[test] fn a_pour_has_a_place_and_no_attach_row_and_the_course_has_no_placement() {
    // Pour selected: "Position" and "Rotation" exist, "Attach" does not. Course selected: no "Placement".
    // A ball in the Balls harness still has "Attach".
}
```

- [ ] **Step 2: run** `cargo test -p bpe-studio --bin ball-studio-next -- a_water_scene a_liquid a_pour` : fail.
- [ ] **Step 3: implement.** **Step 4: run** the same filter plus `icons_exist_for_every_kit_kind
  the_add_menu` and the inspector's `the_root` tests: pass.
- [ ] **Step 5: commit** "Water mode part 1: the inspector and the tree in a Water scene".

## Task 7: the Water stage shell

**Files:** create `water_mode/rail.rs`, `water_mode/course_view.rs`; modify `water_mode/mod.rs`, `app/mod.rs`
(the central panel, `Studio.water_stage: WaterStage`), `style/mod.rs` (rail sizes if a token is missing).

**Produces:**
- `pub enum Tool { Select, Carve, Piece, Pen, Pour, Drain, Lamp, Force, CameraPath }`, `Tool::ALL` (that
  order), `name()`, `hint()` (one line for the strip), `ready()` (Select, Pour, Drain, Lamp, Force),
  `places() -> Option<&'static str>` (the thing kind a click adds).
- `pub struct CourseView { pub rect: Rect, pub size: Vec2 }` with `fit(room: Rect, course: Vec2) -> CourseView`
  (centred, whole course visible, 16 px margin), `to_screen(world: Vec2) -> Pos2`, `to_world(Pos2) -> Vec2`,
  `scale() -> f32`.
- `pub struct WaterStage { pub tool: Tool, drag: Option<(ThingId, Vec2)> }`.
- `pub fn stage(ui: &mut Ui, studio: &mut Studio)`: the strip, the rail, the course.

**Behaviour:** as decision 21. A click on empty course with Select clears the selection. A drag starts only on a
marker and moves `transform.position` by the pointer's world movement, snapped with `studio.grid.step(alt)`; the
frame's own recording makes it one undo step per drag (the history merges a held drag). A placing tool's click
adds the thing with `bpe_kit::add_thing` under the root at the clicked world point (snapped), selects it, sets
`separate_edit`, and puts the tool back to Select; a click outside the course's outline does nothing. Markers are
drawn for kinds where `water::is_placed`, in the thing's track colour, with its name; the selected one has the
selection stroke. The camera's shot is the 9:16 frame at the Camera's Position and Zoom (resolved at the
playhead), in `style::MUTED`.

- [ ] **Step 1: failing tests** (`water_mode/mod.rs` tests):

```rust
#[test] fn the_view_fits_the_course_and_maps_both_ways() {
    let v = CourseView::fit(Rect::from_min_size(Pos2::ZERO, vec2(832.0, 1032.0)), Vec2::new(1080.0, 1920.0));
    assert!((v.rect.height() - 1000.0).abs() < 0.01 && (v.rect.width() - 562.5).abs() < 0.01);
    assert_eq!(v.to_screen(Vec2::ZERO), v.rect.center());
    let p = Vec2::new(-300.0, 500.0);
    assert!((v.to_world(v.to_screen(p)) - p).length() < 1e-3);
    // a wide course (2 screens wide, 1 tall) in the same room is limited by width instead
}
#[test] fn the_rail_has_the_nine_tools_and_only_the_built_ones_work() {
    // stage harness on headless_water(): every Tool::ALL name is a labelled button, in y order;
    // "Carve", "Piece", "Pen", "Camera path" are disabled; clicking "Pour" selects it (state.water_stage.tool).
}
#[test] fn the_pour_tool_adds_a_pour_where_the_stage_is_clicked_and_select_moves_it() {
    // tool = Pour; click at view.to_screen((120, -400)) with the grid's snap off: one water_pour at (120, -400),
    // selected, tool back to Select, undo removes it. Drag its marker by (+50, +30) screen points: its position
    // moved by that over view.scale(); one undo puts it back.
}
#[test] fn a_click_outside_the_course_adds_nothing() { /* Pour tool, click in the stage's margin */ }
#[test] fn the_stage_shows_in_a_water_scene_and_the_ball_preview_does_not() {
    // unit: Studio::headless_water().water() chooses the stage (the central panel's branch is a function
    // `centre_shows(&Studio) -> Centre { Video, Water, Preview }` so it can be asserted without a window).
}
```

- [ ] **Step 2: run** `cargo test -p bpe-studio --bin ball-studio-next water_mode` : fail.
- [ ] **Step 3: implement.** The rail is 44 px wide, buttons are `style::toggle_button`s with `Sense::click()`
  and `add_enabled(tool.ready())`, disabled hover "Arrives in a later part of Water mode". The strip is one
  `ui.horizontal` 32 px tall with the tool's name and hint.
- [ ] **Step 4: run** the same filter, then `cargo test -p bpe-studio --bin ball-studio-next style` (the test
  that no editor file outside `style/` makes up a colour): pass.
- [ ] **Step 5: commit** "Water mode part 1: the Water stage shell with the tool rail".

## Task 8: notes, the guide, and the proof that Balls did not change

**Files:** create `docs/notes/water.md`; modify `CLAUDE.md` (the Scenes row, the format number, one index line),
`docs/notes/engine.md` (the format sentence), `docs/notes/README.md` if it lists the notes, this plan ("As
built").

- [ ] **Step 1:** `cargo fmt --all` then `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- [ ] **Step 2: Balls goldens.** `git diff --stat origin/main -- 'crates/*/tests/golden*' 'crates/*/tests/data'`
  prints nothing. `cargo test -p bpe-kit --test golden --test golden_rules --test golden_health --test
  golden_text` (debug; the physics goldens that build the most kinds) pass against the unchanged files. The
  pinned pictures and the release goldens are CI's (they run on the owner's card).
- [ ] **Step 3:** write `docs/notes/water.md` ("as built": the rule, the kinds and cards, the stand-in, what a
  Water scene hides and where each sentence is, what part 2 picks up) and the three `CLAUDE.md` edits.
- [ ] **Step 4:** push, open the draft pull request, one Opus review of the whole branch, fix what it finds,
  merge when every check is green.

## Self-review (done while writing)

- Spec section 1: SimKind, format, chooser, shared root with hidden settings, the kinds, the slots, keyframes
  only: Tasks 1, 2, 5, 6. Section 7's part-1 share (own stage, ball jobs not started, layout D's rail): Tasks 4
  and 7. Section 8: core, kit, studio, cli each have a task; `bpe-water` is part 2's. Section 9: "Balls goldens
  and pinned pictures must not change": Task 8. Parts item 1 word for word: covered; the `CLAUDE.md` line: Task 8.
- Left for later parts on purpose: the Course's carving (`Value::Strokes`) and the mask (part 2); the Camera's
  Follow card and Camera path (part 9); Draft | Final and Make final in the toolbar, the status bar's drops
  (parts 2 and 8); the Water twin of `restarts.rs` (part 2); Grade mode, export, Video mode and the Scenes window
  for a Water scene (parts 6, 8, 10); recovery and autosave need nothing (they save and load any scene).
- Names used across tasks were checked against each task's Produces block.

## As built

Built as planned in eight tasks; the notes are `docs/notes/water.md`. Where the build went another way than the
plan above:

- **Format.** A loaded file's format becomes 13 in `save::mend_loaded` (the plan kept a 12 file at 12), so a Water
  scene is never saved as 12. `add_slot` also refuses a card of the other scene kind.
- **Sections.** The Water Look's sections are new names (Colour, Body, Its surface, Glow, Crust, Drawn, Extras,
  Mixing shows) so they read in the spec's order; `mixes_when` is on the Mixing card only; the look pick sits in
  the Water card's "Course" section.
- **Export check.** One check at the start of `video_of` (every export, final frame and preview clip) and
  `check_scenes` for a video, instead of one per entry point.
- **Mixer.** `ensure_mixer` itself does nothing in a Water scene, so its callers did not change.
- **`offered` by key.** The scene's and the camera's `offered` look a setting up by key: the inspector blanks a
  def's group on a root card, and going by `def.group` hid every Balls scene setting (caught by the existing
  inspector tests).
- **Cards that stay.** The cards a Water thing starts with, and the scene's Water, Light and Back, have no Remove
  button; a card added with + Add card (a Liquid's Heat) comes off again. A Water scene's settings show no Grade
  summary card.
- **The rail** draws a picture per tool (the thing's own tree icon for a placing tool) with the name on hover and
  for screen readers: nine words don't fit 44 px. The stage is `water_mode/stage.rs` with tests in
  `water_mode/tests.rs`; opening another scene puts the tool back to Select; the course shows a faint line between
  its screens.
- **Tests.** Tests and code were written together per task. The editor's tests are `app/water_tests.rs`,
  `water_mode/chooser.rs`, `water_mode/tests.rs`, and the Water tests in `inspector/tests.rs` and `tree.rs`. Open
  from the chooser is not driven through `answer_chooser` in a test (it opens the system's file dialog).
- **Checked.** `cargo clippy --workspace --all-targets -- -D warnings` clean; the editor's 908 tests, `bpe-core`'s
  and the kit's Water tests pass; no golden or pinned picture file changed, and the kit's physics goldens pass in
  debug against the unchanged files.
