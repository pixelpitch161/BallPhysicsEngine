# Built-in scenes part 1: the Scenes window and the first 8 starters

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Scenes button opens a window that lists the built-in scenes with posters, plays a preview with sound,
and opens one as a new project; the first 8 starters ship in it.

**Architecture:** Scenes are code in a new always-compiled `bpe-kit/src/library/` (`SceneInfo` plus a `build`
function per scene). The editor's `scenes_window/` module filters them (pure logic), draws posters and preview clips on
a background thread with its own graphics device through `bpe_cli` (the export's drawing), and plays preview sound
through a second `SoundJob`. Opening goes through the existing unsaved-work guard and `replace_scene(scene, None)`.

**Tech Stack:** Rust (pinned toolchain), egui 0.37 / eframe, wgpu, kittest, the workspace's crates.

**Spec:** `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md` (read it first; this plan only adds the
decisions the spec leaves to the build).

## Global Constraints

- The editor still opens to a blank project (`new_studio_scene`); nothing is added unless a built-in scene is opened.
- Opening makes an ordinary scene with `Studio.path = None`; nothing in the saved file marks it as built-in.
- Scenes are built from `bpe_kit::new_studio_scene(reg)` and `starters::add_thing`; never the test-only `classic_*`
  helpers or `scenes::Builder`.
- Scenes use no files from disk (no samples, pictures, MIDI); fonts only from the bundled set.
- Starters have "one clean, simple sound that is easy to swap": one Sound card in the scene (on the thing that is hit
  or the ball), the starter voice with only its note settings changed.
- Every thing named for what it is; no two things with the same name; rules named for what they do; no unused cards.
- Design system: no editor file outside `style/` makes up a colour (a test checks). Use `style::` tokens and widgets.
- egui pitfalls in `CLAUDE.md` (no `add_space` in a Grid, `labelled_by` for kittest, `menu_button` not ComboBox).
- Format with `cargo fmt --all` (width 120); clippy with `-D warnings`; tests with `--test-threads=2`.

## Review Focus

1. **Unsaved work when opening a built-in scene.** Open scene with unsaved changes must show the save-changes
   dialog, and Cancel there must keep the project as it was. Test in Task 6.
2. **Closing the window while a preview is being drawn or playing.** The drawing thread must stop and the preview
   sound must go quiet; the project's playing state comes back. Test in Task 7 (window closed mid-preview drops the
   sound job and sets the stop flag).
3. **No graphics device or no speakers.** Tiles fall back to swatches and the preview says so; nothing panics. Test in
   Task 7 (window built with no device shows swatches and "No preview on this computer").
4. **Filters that match nothing.** Find words that match nothing show "No scenes match" and a Clear button that
   brings everything back. Test in Task 5 (pure filter) and Task 7 (the Clear button).
5. **A scene that breaks after a physics change.** The library test runs each scene its full length and fails on
   rule problems, non-finite balls or no hits. Test in Task 1.

---

### Task 1: The library core, its checks and "First bounce"

**Files:**
- Create: `crates/bpe-kit/src/library/mod.rs`, `crates/bpe-kit/src/library/build.rs`,
  `crates/bpe-kit/src/library/starters/mod.rs`, `crates/bpe-kit/src/library/starters/first_bounce.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (add `pub mod library;`)
- Test: `crates/bpe-kit/tests/library.rs`

**Interfaces:**
- Produces (in `bpe_kit::library`):
  - `pub enum Level { Starter, InBetween, Spectacle }` with `pub fn label(self) -> &'static str` ("Starter",
    "In-between", "Spectacle") and `pub fn plural(self) -> &'static str` ("Starters", "In-between", "Spectacles");
    `Level::ALL: [Level; 3]`.
  - `pub enum Kind { BounceAndDrop, Escape, Music, BattlesAndRaces, Hypnotic, LightAndGlass, LiquidSmokeSparks, Words }`
    with `label()` ("Bounce and drop", "Escape", "Music", "Battles and races", "Hypnotic", "Light and glass", "Liquid,
    smoke and sparks", "Words") and `Kind::ALL: [Kind; 8]`.
  - `pub enum SoundLevel { None, Simple, Designed }` with `label()` ("no sound", "one simple sound", "sound designed").
  - `pub struct Payoff { pub what: PayoffEvent, pub at_seconds: f64, pub within_seconds: f64 }`,
    `pub enum PayoffEvent { RuleFires(&'static str) /* rule name */, Breaks(&'static str) /* thing name */, Counter(&'static str, f64) }`
    (defined now, used from part 3; the test in this task checks payoffs only where present).
  - `pub struct SceneInfo { pub id, pub name, pub level: Level, pub kinds: &'static [Kind], pub about, pub uses:
    &'static [&'static str], pub change_first, pub sound: SoundLevel, pub seconds: f64, pub poster_seconds: f64,
    pub preview_seconds: f64, pub swatch: [u32; 2] /* 0xRRGGBB */, pub payoff: Option<Payoff> }` (all text fields
    `&'static str`).
  - `pub struct Entry { pub info: SceneInfo, pub build: fn(&Registry) -> Scene }` and
    `impl Entry { pub fn scene(&self, reg: &Registry) -> Scene }` (calls `build`, then sets `scene.name =
    info.name` and checks in debug that the Scene settings length equals `info.seconds`).
  - `pub fn all() -> &'static [Entry]` (a `static` array in list order) and `pub fn find(id: &str) -> Option<&'static
    Entry>`.
  - `impl SceneInfo { pub fn matches(&self, words: &str) -> bool }`: every whitespace-separated word, ignoring case,
    appears in the name, a kind label or `about`.
- Produces (in `library::build`, `pub(crate)`): `Lib<'a> { reg, scene }` with `new(reg, seconds: f64) -> Lib`
  (`new_studio_scene`, sets duration), `add(&mut self, kind, parent, name) -> ThingId` (`add_thing` + rename),
  `at(&mut self, id, Vec2)`, `params<S: Settings>(&mut self, id, f: impl FnOnce(&mut S))` (reads the thing's own
  params into `S`, changes, writes back), `card<S: Settings>(&mut self, id, slot, f)`, `layer(&mut self, id, slot,
  kind) -> LayerId` and `layer_set(&mut self, id, slot, layer, key, Value)`, `look(&mut self, id, look_id: &str)`
  (applies a ready-made look with `Colours::Look`, `How::Replace`), `sound(&mut self, id) -> &mut Slot` (inserts
  `sound::starter_voice` on the Sound slot and returns it), `rule(&mut self, name, blocks: &[(&str, Vec<(&str,
  Value)>)]) -> RuleId` (blocks linked one after another, as `rule_scenes::lane`), `text(&mut self, name, words,
  at: Vec2, height: f32) -> ThingId`, `scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings))`,
  `finish(self) -> Scene`.

- [ ] **Step 1: Write the failing tests** in `crates/bpe-kit/tests/library.rs`:
  - `ids_names_and_info_are_tidy`: ids unique, match `^[a-z0-9]+(-[a-z0-9]+)*$`, `find(id)` returns the entry; names
    unique; `about`, `change_first` non-empty; `uses` non-empty; `kinds.len()` is 1 or 2; `0 < poster_seconds <
    seconds`; `0 < preview_seconds <= 15.0` and `<= seconds`; starters have `SoundLevel::Simple`.
  - `levels_so_far`: counts per level are `(8, 0, 0)` (each later part bumps this).
  - `every_scene_builds_tidily`: for each entry, `scene(reg)`: `resolve` gives no problems for unknown kinds (every
    thing kind, card kind and layer kind is in the registry), Scene settings `duration == info.seconds`, thing names
    unique, no rule named `"Rule <n>"`, and `bpe_core::load(&bpe_core::save(&scene), reg) == scene` (use the
    existing save/load functions; compare with `assert_eq!`).
  - `every_scene_plays_its_full_length`: `bpe_kit::testing::run`-style world built from the scene, advanced to
    `info.seconds`: no rule has a problem or `busy_at`, every live ball's position and velocity are finite, and the
    log has at least one Hit event.
  - `every_scene_draws_a_picture`: through `bpe_gfx::test_device()`, render at 270 x 480 at `poster_seconds`,
    `seconds * 0.25` and `seconds * 0.75` (the same Playback/build_frame path as `full_size_look.rs::render`); each
    picture has at least 2% of pixels differing from the first pixel by more than 8 in any channel.
  - `every_scene_with_sound_sounds`: for `sound != None`, `bpe_sound::render_samples(&scene, &kit, 4.0)` has a peak
    above 0.001 and below 1.0.
  - `payoffs_land_on_time`: for entries with a payoff, the event happens within `at_seconds ± within_seconds` (none
    yet; the test iterates and passes vacuously).
- [ ] **Step 2: Run** `cargo test -p bpe-kit --test library -- --test-threads=2`. Expected: fails to compile
  (`bpe_kit::library` missing).
- [ ] **Step 3: Implement** `library/mod.rs`, `library/build.rs` and `starters/first_bounce.rs`:
  - **First bounce** (`first-bounce`, Starter, [BounceAndDrop], 12 s, poster 1.2, preview 8, swatch `[0xf2f2f2,
    0x2a2a2a]`): background dark grey `0x202020`; a Shape "Floor" (a wide flat box: Polygon 4 sides, Width 900 Height
    40 at y 650, Fill on) and one Ball "Ball" (radius 40, at (0, -500), small sideways speed 120) wearing ready look
    `"candy"`; bounce 0.85; the Floor has the Sound card (starter voice, note C5, Plays a pluck: leave the starter
    patch). about: "One ball, one floor, a soft pluck on every hit. The simplest scene to pull apart." uses:
    ["Balls", "Shape", "Sound"]. change_first: "The ball's look, how bouncy it is, and the note the floor plays."
  - Exact setting names come from each settings struct (`BallsSettings`, `ShapeSettings`, `SceneSettings`,
    `BasicBounce`); read the struct, don't guess a key.
- [ ] **Step 4: Run** the same command. Expected: all 7 tests pass (`levels_so_far` temporarily expects `(1, 0, 0)`
  until Task 2 lands).
- [ ] **Step 5: Commit** `git commit -m "Built-in scenes: the library, its checks and First bounce"`.

### Task 2: Starters 2 to 8

**Files:**
- Create: `crates/bpe-kit/src/library/starters/{ball_in_a_circle,grow_every_bounce,shrinking_box,every_bounce_makes_two,rainbow_drop,fountain,string_art}.rs`
- Modify: `crates/bpe-kit/src/library/starters/mod.rs`, `crates/bpe-kit/src/library/mod.rs` (`all()` order 1..8),
  `crates/bpe-kit/tests/library.rs` (`levels_so_far` → `(8, 0, 0)`)

**Interfaces:** Consumes Task 1's `Lib` helpers and `Entry`.

Each scene: the spec's row for the idea; 9:16; seconds 15 to 25; poster at a moment where the idea shows; one Sound
card. Decisions per scene:

| id | Seconds | Things (names) | How |
| --- | --- | --- | --- |
| ball-in-a-circle | 20 | "Ring" (circle, size 450, thickness 10, look `neon_tube` or a clean shape look), "Ball" (radius 28) | Ring has the Sound card; Notes: a major scale (C major), Plays Up. |
| grow-every-bounce | 20 | "Ring", "Ball", "Hits" (Text "Bounces: {hits of Ring}") | Rule "Ball grows on every bounce": `when.hit` (who: the Ball, other: the Ring) → `do.change` Size ×1.06 for good, up to the ring's inside (stop at radius 380 with an `if.compare` on size if the block exists; otherwise cap through the change's own limit). Ball's sound: notes going down as it grows is part 2; here a single note. |
| shrinking-box | 20 | "Box" (square size 480, Shrink set so it reaches ~150 at 18 s, Smallest size 140), "Ball" | Box has the Sound card; Plays Up over a minor pentatonic so speed-up reads as rising notes. |
| every-bounce-makes-two | 15 | "Ring", "Ball", "Ball count" (Text "{balls left}") | Rule "Every bounce makes two": `when.hit` on the Ring (min strength so resting balls don't split) → `if.only` n = 7 → `do.split` into 2, spread 30, sizes shared by area. Ring has the Sound card. |
| rainbow-drop | 15 | "Bowl" (a U-shaped Path shape or a half ellipse; use the Corners path), "Rainbow balls" (group Grid 8 x 8, radius 14) | Group gradient layer across the group (rainbow palette); Bowl has the Sound card, Plays Random over a pentatonic. |
| fountain | 20 | "Fountain" (Emitter at the bottom, Fan upward, Every 0.5 s, Endless, Earlier rounds Fade out), "Floor" | Emitter balls wear `comet`; Emitter has the Sound card (it plays its own balls' hits, W10). |
| string-art | 25 | "Ring", "Ball A", "Ball B" | Each ball has a Trail layer drawing a staying Line (the "forever" Line of `plans/trails/staying-lines.md`) in its own colour; Ring has the Sound card with a soft bell (starter patch, longer release only if it's one setting). |

- [ ] **Step 1:** Bump `levels_so_far` to `(8, 0, 0)`; run the library tests; expected FAIL on the count.
- [ ] **Step 2:** Write the 7 scenes, one file each, registering them in `all()` in the order above.
- [ ] **Step 3:** Run `cargo test -p bpe-kit --test library -- --test-threads=2`. Expected: PASS. A scene that fails
  "plays its full length" is retuned, never excused.
- [ ] **Step 4: Commit** `git commit -m "Built-in scenes: starters 2 to 8"`.

### Task 3: Full-size look check for the library

**Files:** Modify `crates/bpe-kit/tests/full_size_look.rs`.

- [ ] **Step 1:** Add `fn library_shots(reg) -> Vec<Shot>`: one shot per entry named `library-<id>` at times
  `[poster_seconds, seconds * 0.4, seconds * 0.8]`, and include it in the shots list so `LOOK_ONLY=library` picks them.
- [ ] **Step 2:** Run `LOOK_OUT=$SCRATCH/look LOOK_ONLY=library cargo test --release -p bpe-kit --test full_size_look
  -- --ignored --nocapture`. Expected: 24 PNGs plus crops and a contact sheet.
- [ ] **Step 3:** Open the contact sheet and each full picture and crops; fix jagged edges, light leaks, banding,
  things off screen, unreadable text, empty-looking frames. Re-run until they look right.
- [ ] **Step 4: Commit** `git commit -m "Built-in scenes: full-size look check set"`.

### Task 4: Preview clips in bpe-cli

**Files:** Modify `crates/bpe-cli/src/lib.rs`. Test: in its `#[cfg(test)]` module (or `crates/bpe-cli/tests/` if the
crate keeps tests there).

**Interfaces:**
- Produces: `pub fn preview_clip(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, kit: &Kit, seconds: f64,
  fps: u32, scale: f32, stop: &AtomicBool, each: impl FnMut(u32, Vec<u8>, (u32, u32))) -> anyhow::Result<u32>`:
  draws frames `0..ceil(seconds * fps)` unbaked with `FrameMaker` and one `Renderer` at `output_size(video, scale)`,
  calling `each(index, rgba, size)` per frame; returns the count; `stop` set → `bail!("stopped")` between frames.
  Holds the swarm card like `final_frame`. Draws each frame again while `renderer.programs_waiting()`.
- [ ] **Step 1: Test** `preview_clip_draws_every_frame_and_stops`: a tiny scene (one ball), `seconds 0.5, fps 10,
  scale 0.1` gives 5 calls with indices 0..5 and the same size; with `stop` already set it returns `Err` and calls
  nothing. Uses `bpe_gfx::test_device()`.
- [ ] **Step 2:** Run `cargo test -p bpe-cli preview_clip -- --test-threads=2`; expected FAIL (missing function).
- [ ] **Step 3:** Implement. **Step 4:** Run; expected PASS. **Step 5: Commit.**

### Task 5: Filtering (pure) and opening a built-in scene

**Files:**
- Create: `crates/bpe-studio/src/scenes_window/mod.rs`, `crates/bpe-studio/src/scenes_window/filter.rs`
- Modify: `crates/bpe-studio/src/main.rs` (module), `crates/bpe-studio/src/app/mod.rs` (`Pending::OpenBuiltIn(&'static
  str)`, `ToolbarClick::Scenes`, `Panels.scenes: ScenesWindow`), `crates/bpe-studio/src/app/files.rs` (`perform`,
  `open_built_in`), `crates/bpe-studio/src/app/panels.rs` (Scenes button after Recent)

**Interfaces:**
- Produces in `filter.rs`: `#[derive(Clone, Debug, Default, PartialEq)] pub struct Filter { pub level: Option<Level>,
  pub kind: Option<Kind>, pub words: String }`; `pub fn shown(&self, all: &'static [Entry]) -> Vec<&'static Entry>`
  (in list order); `pub fn level_count(&self, all, level: Option<Level>) -> usize` (with this filter's kind and words,
  that level); `pub fn kind_count(&self, all, kind: Kind) -> usize` (with this filter's level and words, that kind);
  `pub fn is_clear(&self) -> bool`.
- Produces in `files.rs`: `pub fn open_built_in(&mut self, id: &str) -> bool`: `library::find(id)` → `entry.scene(reg)` →
  `replace_scene(scene, None)`, status "Opened the built-in scene <name>. Save it to keep your changes.", clears the
  File problem; an unknown id reports a File problem and returns false.
- [ ] **Step 1: Tests** in `filter.rs`: `no_filter_shows_all_in_order`, `level_and_kind_narrow`,
  `words_must_all_match_ignoring_case` ("BOUNCE circle" → only ball-in-a-circle), `counts_follow_the_other_filters`
  (with words "ring": `level_count(None)` equals shown().len() with level None; a kind with no match counts 0),
  `nothing_matches_shows_nothing_and_clear_resets`. In `app/tests.rs` (existing studio test module):
  `opening_a_built_in_scene_makes_an_unsaved_free_new_project` (path `None`, name "Ball in a circle", `unsaved()`
  false, things include "Ring" and "Ball"), `opening_with_unsaved_work_asks_first` (edit something, `guard(Pending::
  OpenBuiltIn("first-bounce"))` sets `confirm` and leaves the scene), `an_unknown_built_in_id_keeps_the_scene`.
- [ ] **Step 2:** Run `cargo test -p bpe-studio filter open_built_in opening_ -- --test-threads=2` (or the module
  paths); expected FAIL.
- [ ] **Step 3:** Implement; the toolbar button `style::toolbar_button(ui, "Scenes")` with hover "Open one of the
  built-in scenes, from simple starters to full spectacles" sets `ToolbarClick::Scenes`, which opens the window.
- [ ] **Step 4:** Run; expected PASS. **Step 5: Commit.**

### Task 6: Posters, clips and preview sound (the background side)

**Files:** Create `crates/bpe-studio/src/scenes_window/pictures.rs`, `crates/bpe-studio/src/scenes_window/sound.rs`.

**Interfaces:**
- `pictures.rs`: `pub struct Pictures` owning one worker thread (spawned on first ask) with its own
  `bpe_gfx::headless_device()`; `pub fn ask_poster(&mut self, id)`, `pub fn ask_clip(&mut self, id)` (a newer clip
  ask goes first; posters for shown tiles before the rest), `pub fn poll(&mut self, ctx) ` (turns arrived pixels into
  `egui::TextureHandle`s), `pub fn poster(&self, id) -> Option<&TextureHandle>`, `pub fn clip(&self, id) -> Clip`
  where `pub enum Clip { NotAsked, Making { done: u32, of: u32 }, Ready(Vec<TextureHandle>) /* 30 fps */,
  Failed(String) }`, `pub fn stop(&mut self)` (sets the worker's stop flag; the worker finishes nothing more and ends).
  Posters: `bpe_cli::final_frame(device, queue, scene, kit, poster_seconds, 1.0 / 6.0, Some(false), stop)`; clips:
  `bpe_cli::preview_clip(.., preview_seconds, 30, 0.2, ..)`. Keep the 4 most recently asked clips. `Pictures::none()`
  for "no graphics device" (every ask is ignored; `device_missing()` true) — used when `headless_device` fails and in
  tests.
- `sound.rs`: `pub struct PreviewSound { job: Option<SoundJob>, id: Option<&'static str>, seconds: f64 }`;
  `pub fn play(&mut self, entry, kit)` (builds the scene, `AudioOut::open().and_then(|o| SoundJob::start(..))`, plays
  from 0), `pub fn clock(&self) -> Option<f64>` (looped: when the job's clock passes `seconds`, seek 0), `pub fn
  stop(&mut self)` (drops the job), `pub fn set_on(&mut self, on: bool)` (mute through the job's shared volume).
- [ ] **Step 1: Tests** (`pictures.rs`): `with_no_device_nothing_is_drawn_and_it_says_so`; a GPU test
  `a_poster_arrives` (uses the worker with `test_device()` injected through `Pictures::with_device(device, queue)`;
  ask_poster("first-bounce"), poll until it arrives within 60 s, size 180 x 320). (`sound.rs`):
  `stop_drops_the_job` with `SoundJob::recording()`-style construction if available, else test only the loop
  arithmetic in a `fn looped(clock, seconds) -> f64` helper.
- [ ] **Step 2:** Run; expected FAIL. **Step 3:** Implement. **Step 4:** Run; expected PASS. **Step 5: Commit.**

### Task 7: The window itself

**Files:** Modify `crates/bpe-studio/src/scenes_window/mod.rs`; create `crates/bpe-studio/src/scenes_window/tests.rs`;
modify `crates/bpe-studio/src/app/mod.rs` (show the window each frame, pause/restore playing).

**Interfaces:**
- `pub struct ScenesWindow { pub open: bool, filter: Filter, picked: Option<&'static str>, hovered: Option<(&'static
  str, f64 /* since */)>, pictures: Pictures, sound: PreviewSound, sound_on: bool, playing: bool, was_playing: bool }`;
  `pub fn open(&mut self, was_playing: bool)`, `pub fn show(&mut self, ctx, kit) -> Option<WindowOutcome>` where
  `pub enum WindowOutcome { Open(&'static str), Closed { was_playing: bool } }`.
- Layout and copy exactly as the spec's "The Scenes window" section: header, filters (Level then Kind, counts in
  `style::value` mono), grid of tiles (poster or swatch gradient, level badge, name, kinds joined with " · "), preview
  column (clip at the sound clock, else the poster with "Getting the preview ready…" and a progress bar from `Clip::
  Making`), Playing/Paused, Sound on/off, time "m:ss / m:ss", name, "Level · Kinds · N s · <sound label>", about,
  "Uses: a, b, c", "Good things to change first", the note line, Open scene (`style::primary_button`) and Cancel.
  Hover 0.3 s before asking a clip. Double-click a tile opens it.
- Names for kittest: filter buttons labelled "Level <name>" / "Kind <name>", tiles "Scene <name>", "Open scene",
  "Cancel", "Close scenes", "Find a scene", "Clear the filters".
- [ ] **Step 1: Tests** (kittest, `tests.rs`, with `Pictures::none()`): `scenes_button_opens_the_window`,
  `level_filter_narrows_the_tiles_and_counts_show`, `find_box_filters_and_clear_brings_them_back`,
  `clicking_a_tile_shows_its_info` (name, about and "Good things to change first" text present),
  `open_scene_replaces_the_project` (via the Studio: click Open scene → scene name is the picked one, path `None`),
  `cancel_and_escape_close_and_restore_playing` (opened while playing → closed → playing again),
  `with_no_device_tiles_show_swatches_and_the_preview_says_so`, `closing_mid_preview_stops_the_sound_and_drawing`.
- [ ] **Step 2:** Run `cargo test -p bpe-studio scenes_window -- --test-threads=2`; expected FAIL.
- [ ] **Step 3:** Implement. **Step 4:** Run; expected PASS.
- [ ] **Step 5:** Screenshot the release editor under Xvfb at 1440 x 960 with the window open (for my own check
  only, not sent): tiles readable, nothing clipped, the design system's greys. Fix what looks wrong.
- [ ] **Step 6: Commit.**

### Task 8: Notes, guide line, handover, full checks, review, pull request

**Files:** Create `docs/notes/library.md`; modify `docs/notes/README.md` (row), `CLAUDE.md` (one area-index line:
"Built-in scenes (library, Scenes window)"), this plan (an "As built" section at the end).

- [ ] **Step 1:** Write the notes: the module layout, `SceneInfo`, the `Lib` helpers, how to add a scene (file,
  `all()` order, bump `levels_so_far`, look-check), the window's pieces, test names.
- [ ] **Step 2:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, then the whole suite
  once in the background: `cargo test --workspace -- --test-threads=2`. Expected: all pass (goldens unchanged:
  nothing here moves physics).
- [ ] **Step 3:** One Opus review of the branch (subagent, `superpowers:requesting-code-review`); fix what it finds.
- [ ] **Step 4:** Push, open the PR (draft → ready once green), subscribe, merge when checks and review pass.
- [ ] **Step 5:** Write `/mnt/project-files/ballphysics/builtin-scenes/part1-handover.md` (what landed, decisions
  made, gotchas, what part 2 starts with) and message the coordinator to start part 2 in a fresh thread.
