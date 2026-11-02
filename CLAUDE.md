# Ball Physics Engine: guide for Claude

Ball Studio is a Rust + wgpu editor and renderer for vertical 9:16 bouncing-ball videos (mp4 with sound) for
TikTok, Reels and Shorts. The owner is a music producer on Windows 11 who is not a programmer. They run a ready-built
`ball-studio.exe` and never run commands themselves.

## Two engines live side by side

The repo holds **two complete engines**. Almost all current work happens in the **new modular one**.

| | New modular engine (current) | Old engine (frozen) |
| --- | --- | --- |
| Editor crate / binary | `bpe-studio` / `ball-studio-next` | `bpe-app` / `ball-studio` |
| Command-line renderer | `bpe-cli` / `bpe-render-next` | `bpe-export` / `bpe-render` |
| Scenes | `scenes/next/` | `scenes/*.ron`, `scenes/showcase/` |
| Design spec | `docs/superpowers/specs/2026-10-29-modular-studio-design.md` | none |

The shipped `bin\ball-studio.exe` on the owner's PC is built from **`bpe-studio`** (`-p bpe-studio`), even though
its binary is named `ball-studio-next`. `README.md` still documents the **old** editor's tabs and features, so don't
take it as a description of the current UI.

Old scene files don't need to keep loading or looking the same. The owner has said there's no backward
compatibility until they are happy with the software.

## Crate map (new engine)

Dependencies go bottom to top:

- `bpe-settings-derive`: `#[derive(Settings, Choice)]` turns a plain struct into a settings schema. Each field's
  `#[setting(...)]` attribute (label, range, unit, `simple`, `random`, `keyframe`, `structural`, `when`) drives the
  inspector UI, dice, locks, keyframes and pulses all at once.
- `bpe-core`: the data model. Things sit in a tree (`scene.rs`), modules sit in a shared library (`library.rs`),
  values, colours/gradients, blend modes, progressions, animation (keyframes), save/load (`save.rs`) and
  **resolve** (`resolve.rs`, which follows the order override > linked module > parent's module > Scene settings).
  `registry.rs` holds the plugin registry.
- `bpe-runtime`: the world, the fixed 240 Hz staged step loop, the shared collision pass (`pairs.rs`, `walls.rs`,
  `solids.rs`), the camera, the event log (`events.rs`) and playback/snapshots.
- `bpe-graph`: node-graph wiring, types and validation. Rules and shader graphs share it.
- `bpe-vm`: compiles module and rule graphs and runs them on the CPU.
- `bpe-gfx`: the renderer. Things draw into a frame (`frame.rs`) and the GPU draws it: gradients, blend, grade,
  effects, flow layers (GPU swarms/liquids), particles, text.
- `bpe-synth`: the modular node synth (DSP blocks, patch catalog/compiler, voices).
- `bpe-mix`: instruments, chords, buses, master chain, stems and export of the soundtrack.
- `bpe-music`: song decoding, beats/tempo, band levels, MIDI. The old engine uses it too.
- `bpe-kit`: **every plugin**, with one folder per thing kind in `src/things/` (arena, balls, camera, emitter,
  field, folder, liquid, obstacle, scene, soft, swarm, text) and one per module kind in `src/modules/` (look, trail,
  motion, bounce, sound, particles, rules, shader, drums, melody, and more). A folder usually has `mod.rs`
  (settings + kind), `sim.rs` (physics) and `draw.rs` or `graph.rs`. `lib.rs::kit()` registers them all.
  `starters.rs` holds per-thing starter modules and `scenes.rs` holds the built-in scenes.
- `bpe-cli`: headless export and batch rendering.
- `bpe-studio`: the egui 0.37 editor. Its layout has the tree on the left, the inspector on the right, the node
  panel under the preview and the timeline at the bottom. Key files are `app.rs`, `inspector.rs`, `preview.rs`,
  `nodes/`, `timeline/`, `library.rs`, `palettes.rs`, `export_window.rs` and `sound_job.rs`.

Old engine crates you should rarely need: `bpe-scene`, `bpe-sim`, `bpe-audio`, `bpe-render`, `bpe-export`,
`bpe-app`.

## Working rules

- The project follows the superpowers workflow: brainstorm, then a spec in `docs/superpowers/specs/`, then a plan
  in `docs/superpowers/plans/`, then build, then verify. Read the latest spec/plan for the area you touch.
- The owner wants plain-language messages and a double-click exe, never instructions to run commands.
- Keep code Windows-friendly. Exes are cross-built from Linux with MinGW.

## Building and testing in a cloud container

A fresh container needs:

```sh
apt-get update && apt-get install -y libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers \
  libxkbcommon-x11-0 libxkbcommon0 xvfb xdotool imagemagick gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

- Tests: `cargo test --workspace` takes about 8-10 minutes cold, so run it in the background. Without
  `mesa-vulkan-drivers`, GPU picture tests fail with "no graphics adapter". That's an environment problem, not a
  bug. In `--release`, `bpe-runtime`'s `pushing_an_earlier_step_panics_in_debug` always fails (it is debug-only).
- Lint: `cargo clippy --workspace` and `cargo fmt` (max width 120). `cargo fmt` rewraps lines, so scripted text
  replacements can silently miss. Check that the old text is present first.
- Windows exe: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio` (and `-p bpe-cli` for the
  renderer). Rename `ball-studio-next.exe` to `ball-studio.exe` when delivering it.
- Disk is a fixed allowance and `target/` passes 18 GB. Use `CARGO_INCREMENTAL=0` and delete `target/debug` when the
  disk fills.
- Screenshots: run the release editor under Xvfb at 1440x960 and capture with `import -window root`. When killing
  it, use `pkill -f "[b]all-studio-next"` (a plain pattern also kills your own shell).

## Golden tests (determinism)

The physics is deterministic, and goldens pin it:

- New engine: `crates/bpe-kit/tests/golden*.txt` (plus `-debug` variants).
- Old engine: `crates/bpe-sim/tests/golden*.txt`, and `crates/bpe-audio/tests/old_scenes.rs` pins the soundtrack
  hashes.

Any change that moves balls changes golden lines. Re-bless with `BLESS_GOLDEN=1`, **once in debug and once with
`--release`**. Then diff against main: only the lines you meant to change may differ. Goldens differ slightly on
native Windows (libm trig), which is a known gap.

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

- Module states are `MotionState { settings, program }` and `BounceState { settings, program }` (in
  `bpe-kit/src/modules/{motion,bounce}/sim.rs`). `downcast_ref::<BasicBounce>()` compiles but silently returns
  `None`.
- A custom module's setting blocks ARE its params. `nodes::panel::sync_params` keeps them equal, so never write one
  side alone.
- There are two matchers on purpose. `World::matching(target, me)` is the physics view (Me = the thing itself).
  `World::matches(me, thing, target)` / `things_of` is the rules view (Me = the Rules scope).
- An inherited (linked-from-parent) module card is read-only until "Change here".
