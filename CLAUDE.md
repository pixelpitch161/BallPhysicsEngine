# Ball Physics Engine: guide for Claude

Ball Studio is a Rust + wgpu editor and renderer for vertical 9:16 bouncing-ball videos (mp4) for
TikTok, Reels and Shorts. The owner is a music producer on Windows 11 who is not a programmer. They run a ready-built
`ball-studio.exe` and never run commands themselves.

| | |
| --- | --- |
| Editor crate / binary | `bpe-studio` / `ball-studio-next` |
| Command-line renderer | `bpe-cli` / `bpe-render-next` |
| Scenes | none built in: the editor always opens to a blank project (Scene settings + Camera) |
| Area notes | `docs/notes/` (one file per area, the detail this guide leaves out) |

The shipped `bin\ball-studio.exe` on the owner's PC is built from **`bpe-studio`** (`-p bpe-studio`), even though
its binary is named `ball-studio-next`. On 2026-11-03 everything but the core was removed to rework the program from
a smaller base; the version before is **Version 1** (the `version-1` branch). Documents from before then are in
`docs/superpowers/archive/` and describe removed features. Old scene files don't need to keep loading or looking the
same: there's no backward compatibility until the owner is happy with the software.

## Core ideas

- **Things in a tree** (`bpe-core/src/scene.rs`): Scene settings (the root), Camera, Balls, Emitter, Shape, Light,
  Rig, Particles, Text, Folder. Every thing **owns its settings**: nothing is shared, linked, inherited or locked,
  and there's no Library, presets or randomize. A thing's **cards** live in `Thing.slots` as `Slot { kind, params,
  stash }` (Look, Motion, Bounce, Health, Sound; the scene adds Effects in its Shader slot, Lighting, Air, Mixer).
  A card kind is a `ModuleKind` (the name is historical). A setting's address is `{ thing, slot, key }`.
- **Layers** (`bpe-core/src/layers.rs`): a card can hold layers (`Slot.layers`, sections Drawn, Around, Reaction,
  Group, Sub, Chain, Modulator, Effect, Force); a layer's settings are layered keys `"<layer id>/<key>"`. Sub-layers
  sit right after the layer they change. Effects on a thing are Shader layers of its Look.
- **Settings** come from `#[derive(Settings, Choice)]` (`bpe-settings-derive`): each field's `#[setting(...)]` drives
  the inspector, keyframes and pulses. **Resolve** (`resolve.rs`) lays each card's defaults under the thing's own
  values. Right-clicking a setting opens Copy Value / Paste Value / Create Keyframe; a setting can also Follow sound,
  light or health (`PulseSource`) and, on a Balls group or Emitter, **Vary across balls** (`"<key>~to"`, `~by`,
  `~ease`; `docs/notes/foundations.md`).
- **Plugins**: every thing, card, layer and rule block kind registers in the kit (`bpe-kit/src/lib.rs::kit()`), with
  a sim (`ThingSim`, `ModuleSim`, `BlockSim`) for physics and a draw (`ThingDraw`) for pictures.
- **Physics** is a fixed 240 Hz staged step loop (`bpe-runtime`) and is deterministic: goldens pin it. Things that
  only decorate (most Particles things) run beside the main world (`World.beside`, `main_part`), so an edit to them
  restarts only their own playback. Fingerprints decide what an edit restarts (physics, sound, particles).
- **Drawing**: things draw into a `Frame` (`bpe-gfx/src/frame.rs`) and the renderer draws it: balls through one
  layered uber-shader, shapes and text through paths, trails as strips, particles as sprites, then lighting passes,
  effects, bloom and grade. A frame with no Lighting card is byte-identical to before lighting existed.
- Scene files are format 12 (`save.rs`). Never add built-in or example scenes to the program unless the owner asks;
  test scenes live in `bpe-kit/src/scenes.rs` behind the `test-scenes` feature.

## Area index

Each area's detailed notes are in `docs/notes/`; its design is in `docs/superpowers/specs/` and its plans (with
"As built" sections) in `docs/superpowers/plans/`. Read both before changing an area.

| Area | Notes | Spec / plans |
| --- | --- | --- |
| Strip-down, own settings, Vary across balls | `foundations.md` | `plans/2026-11-03-*`, `plans/ball-emitter/part5-vary.md` |
| Crates in detail (core, runtime, gfx, kit incl. balls, shapes, emitter and aim, cli) | `engine.md` | `specs/2026-11-04-shape-thing-design.md`, `specs/2026-11-05-ball-emitter-design.md`, `plans/ball-emitter/` |
| Lighting (lights, Flat/Stage, surfaces, sensing, lens, Stage camera, bake, light shows, Effects stack) | `lighting.md` | `specs/2026-11-06-lighting-design.md`, `plans/lighting/` |
| Glass and light (coloured shadows, prisms, bent light, mirrors, pipes, dichroic, rainbows) | `glass-light.md` | `specs/2026-11-07-glass-light-design.md`, `plans/glass-light/` |
| Rules (engine, editor, break looks, events, Health, spectacle actions) | `rules.md` | `specs/2026-11-06-rules-design.md`, `plans/rules/` |
| Gradients (mixes, companions, shapes, movement, big editor) | `gradients.md` | `specs/2026-11-06-gradients-design.md`, `plans/gradients/` |
| Particles thing (engine, air, meeting, looks, liquid, forming, sound, Huge swarm) | `particles.md` | `specs/2026-11-06-particles-design.md`, `plans/particles/` |
| Text thing (fonts, live words, letter walls, letter moves, effects) | `text.md` | `specs/2026-11-07-text-thing-design.md`, `plans/text/` |
| Sound (bpe-dsp, bpe-sound: voices, chains, modulators, notes, buses, Follow sound) | `sound.md` | `specs/2026-11-04-sound-system-design.md`, `plans/sound-system/` (decisions log there) |
| Editor (design system, inspector, Look editor, timeline, Mixer, Sound tab) | `editor.md` | `specs/2026-11-03-design-system-editor-design.md`, `specs/2026-11-03-ball-look-editor-design.md` |
| Trails and the staying Line | `engine.md` (bpe-kit) | `plans/trails/staying-lines.md`, `plans/2026-11-05-trail-bounce-corners.md` |
| Checks, picture tests, this guide | this file | `plans/2026-11-08-safety-nets-lean-guide.md` |

## Crate map

Dependencies go bottom to top. `docs/notes/engine.md`, `sound.md` and `editor.md` have the long version.

- `bpe-settings-derive`: the `Settings`/`Choice` derives (label, range, `span`, unit, `simple`, `keyframe`, `pulse`,
  `when`).
- `bpe-core`: the data model: scene tree, cards, layers and sub-layers, values (colours, gradients, paths, strokes,
  points, curves, routes, light scenes), animation (keyframes, pulses), edits, save/load, resolve, the plugin
  registry, rules data.
- `bpe-runtime`: the world, the 240 Hz staged step loop, collisions (`pairs.rs`, `walls.rs`, two-sided walls), the
  camera, the event log, history (trails' past), playback and snapshots, sound/light/health inputs, the rules stage.
- `bpe-gfx`: the renderer: frames, the layered ball shader and in-ball programs, paths for shapes and text,
  trails, sprites, fields, liquid, swarm, lighting (`lighting.rs`, `shaders/lighting.wgsl`), effects, bake.
- `bpe-kit`: every plugin: one folder per thing kind in `src/things/`, per card kind in `src/modules/`, rule blocks
  in `src/rules/`, the text engine in `src/text/`, paint and gradients in `src/paint/`, starting cards in
  `starters.rs`, test scenes in `scenes.rs`.
- `bpe-dsp`: sound building blocks at 48 kHz in 64-frame blocks (oscillators, wavetables, filters, shapers, bus
  effects, true-peak limiter, loudness meter).
- `bpe-sound`: the sound engine (its own physics playback, voices, chains, modulators, notes, buses, Master), offline
  WAV, stems and MIDI, `ControlSampler` for Follow sound.
- `bpe-cli`: headless export (frames, sound muxed as AAC, stems, MIDI, bake).
- `bpe-studio`: the egui 0.37 editor in the Ball Studio Design System (`style/` holds every token; no other editor
  file may make up a colour, a test checks).

## Working rules

- The project follows the superpowers workflow: brainstorm, then a spec in `docs/superpowers/specs/`, then a plan
  in `docs/superpowers/plans/`, then build, then verify. Read the latest spec/plan for the area you touch.
- The owner wants plain-language messages and a double-click exe, never instructions to run commands.
- Keep code Windows-friendly. Exes are cross-built from Linux with MinGW.
- **Notes, not this file**: when a part lands, add its "as built" summary to its area's file in `docs/notes/` in the
  same pull request. Change this file only for a new area (one index line), a new working rule or a new pitfall.
- **Checks**: every pull request runs `.github/workflows/ci.yml` (format, clippy with warnings as errors, all tests on
  a software Vulkan driver, release goldens, the Windows cross-build). Merge only when every check is green; never
  skip, disable or `#[ignore]` a test to get there.
- **Look at full-size pictures**: after any change to what is drawn, run the full-size look check (below), open the
  pictures and their 1:1 crops, and look for jagged edges, light leaks, banding, seams, flicker between frames and
  anything that doesn't make sense, before the PR. Picture tests run at 270 x 480 and check numbers, so they miss
  these. Add a scene to `full_size_look.rs` when a feature needs one.

## Building and testing in a cloud container

A fresh container needs:

```sh
apt-get update && apt-get install -y libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers ffmpeg \
  libxkbcommon-x11-0 libxkbcommon0 xvfb xdotool imagemagick gcc-mingw-w64-x86-64
```

- Rust is pinned in `rust-toolchain.toml` (with clippy, rustfmt and the Windows target), so CI and every container
  build with the same compiler. Moving it forward is its own pull request that fixes any new clippy warnings.

- Tests: `cargo test --workspace` takes several minutes cold, so run it in the background. Run `bpe-gfx`'s own tests
  with `--test-threads=2` or they run out of memory. In `--release`, `bpe-runtime`'s
  `pushing_an_earlier_step_panics_in_debug` always fails (it is debug-only).
- **Picture tests need a graphics device.** Every test that draws gets it from `bpe_gfx::test_device()`, which fails
  the test when there is none (install `mesa-vulkan-drivers`). Set `BPE_SKIP_GPU_TESTS=1` only to skip them on
  purpose. New picture tests use `test_device()`, never `headless_device()`.
- **Full-size look check**: `LOOK_OUT=<folder> cargo test --release -p bpe-kit --test full_size_look -- --ignored
  --nocapture` renders the look scenes at 1080 x 1920 (full PNGs, four 1:1 quarter crops each, a contact sheet).
  `LOOK_ONLY=room,text` picks scenes by name; `LOOK_QUALITIES=good` picks lighting qualities.
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` (max width 120). `cargo fmt`
  rewraps lines, so scripted text replacements can silently miss. Check that the old text is present first.
- Windows exe: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio` (and `-p bpe-cli` for the
  renderer). Rename `ball-studio-next.exe` to `ball-studio.exe` when delivering it, and put `assets/fonts/text/`
  beside it as `fonts\` (the Text thing's fonts). Build and install it only when the owner asks.
- Disk is a fixed allowance and `target/` grows large. Use `CARGO_INCREMENTAL=0` and delete `target/debug` when the
  disk fills.
- Screenshots: run the release editor under Xvfb at 1440x960 and capture with `import -window root`. When killing
  it, use `pkill -f "[b]all-studio-next"` (a plain pattern also kills your own shell).

## Golden tests (determinism)

The physics is deterministic, and goldens pin it: `crates/bpe-kit/tests/golden*.txt` (plus `-debug` variants) and
`crates/bpe-sound/tests/golden-sound*.txt`.

Any change that moves balls changes golden lines. Re-bless with `BLESS_GOLDEN=1`, **once in debug and once with
`--release`**. Then diff against main: only the lines you meant to change may differ. Goldens differ slightly on
native Windows (libm trig) and on newer Linux (glibc after 2.39), known gaps: CI runs on Ubuntu 24.04 to match the
containers.

## egui / kittest pitfalls

- A drag-only widget on top of a click widget swallows clicks. Use `Sense::click_and_drag()` on the button itself.
- There is no `egui::SidePanel`; use `egui::Panel::left(id)`. Side panels shrink to their content on the first
  frame, so call `ui.set_min_width(ui.available_width())` first.
- `ui.add_space` inside `egui::Grid` panics in debug.
- kittest's accessibility check needs every control named via `.labelled_by(label.id)`. ComboBox can't be named,
  so pickers are `menu_button`s. `get_by_*` panics on several matches; use `query_all_by_label(..).next()`.
  Rows below the fold need `scroll_to_me()` in tests.
- Never read `pointer.press_origin()` inside a `clicked()` handler, because egui clears it on release.
- The default fonts lack glyphs like ◉ ○ ♭ ♯ ▶. Paint icons or use plain words.

## Engine pitfalls

- Card sim states are `MotionState { settings }` and `BounceState { settings }` (in
  `bpe-kit/src/modules/{motion,bounce}/sim.rs`). `downcast_ref::<BasicBounce>()` compiles but silently returns
  `None`.
- `World::matching(target, me)` lists the things a target points at (Me = the thing itself); `World::names(target,
  me, other)` asks whether it points at one thing (the camera's hit trigger uses it).
- A thing with no Look card draws nothing, and a card of an unknown kind is reported and ignored.
- An edit that should restart the physics, sound or particles only does so if the right fingerprint hook sees it
  (`ThingSim::affects_physics`, `own_cards`, `fingerprint_more`, `more_fingerprint`, `affects_beside`); a missing one
  leaves the preview stale.
