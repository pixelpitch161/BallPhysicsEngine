# Ball Physics Engine: guide for Claude

Ball Studio is a Rust + wgpu editor and renderer for vertical 9:16 bouncing-ball videos (mp4) for
TikTok, Reels and Shorts. The owner is a music producer on Windows 11 who is not a programmer. They run a ready-built
`ball-studio.exe` and never run commands themselves.

## One engine, stripped to its core

On 2026-11-03 the owner had everything except the core removed, to rework the program from a smaller base. The
version before that is **Version 1**, kept on the `version-1` branch (with its exes in the owner's
`bin\version-1\` folder). Removed in that pass: the old engine (`bpe-app`, `bpe-scene`, `bpe-sim`, `bpe-audio`,
`bpe-render`, `bpe-export`, `bpe-music`), node graphs and rules (`bpe-graph`, `bpe-vm`), the Explore and Batch
windows, the canvas and paint marks, the whole sound and chords system (`bpe-synth`, `bpe-mix`), and the Obstacle,
Liquid, Soft body, Swarm, Field, Meter, Emitter and Text things. Exports carry a silent audio track.

Rework step 2 (same day) dropped shared modules: every thing owns its settings. A thing's cards (Look, Trail,
Motion, Bounce, Particles, Shader) live in `Thing.slots` as `Slot { kind, params, stash }` (balls have no Shader card and shapes none of Trail, Motion or Shader: effects on a thing are Shader layers of its Look; only the scene settings keep a Shader card, the picture's Effects); nothing is shared,
linked, inherited from a folder or locked, and there is no Library, presets or randomize (dice). Right-clicking a
setting opens Copy Value / Paste Value (…) / Create Keyframe; there are no keyframe diamonds in the inspector.

| | |
| --- | --- |
| Editor crate / binary | `bpe-studio` / `ball-studio-next` |
| Command-line renderer | `bpe-cli` / `bpe-render-next` |
| Scenes | none built in: the editor always opens to a blank project (Scene settings + Camera) |
| Design spec | `docs/superpowers/specs/2026-10-29-modular-studio-design.md` (written before the strip-down) |

The shipped `bin\ball-studio.exe` on the owner's PC is built from **`bpe-studio`** (`-p bpe-studio`), even though
its binary is named `ball-studio-next`. Older specs and plans in `docs/superpowers/` describe removed features.

Old scene files don't need to keep loading or looking the same. The owner has said there's no backward
compatibility until they are happy with the software.

## Crate map

Dependencies go bottom to top:

- `bpe-settings-derive`: `#[derive(Settings, Choice)]` turns a plain struct into a settings schema. Each field's
  `#[setting(...)]` attribute (label, range, `span`, unit, `simple`, `keyframe`, `pulse`, `when`) drives the
  inspector UI, keyframes and pulses at once. `span(a, b)` is a slider's usual span when there is no hard `range`.
- `bpe-core`: the data model. Things sit in a tree (`scene.rs`), each with its own params and its own cards in
  `slots`. Also values, colours/gradients, blend modes, animation (keyframes and pulses on hits), save/load
  (`save.rs`), edits (`edit.rs`: `add_slot`, `remove_slot`, `set_slot_kind`) and **resolve** (`resolve.rs`: each
  card's defaults under the thing's own values). A setting's `Address` is `{ thing, slot, key }`.
  `registry.rs` holds the plugin registry. `ModuleKind` is the trait for a card kind (the name is historical).
  A card can hold **layers** (`layers.rs`, `Slot.layers`): each `Layer { id, kind, name, enabled, only }` is of a
  `LayerKind` whose `LayerSection` is Drawn, Reaction or Group, kept in that order in the Vec (index 0 is the base
  layer, which can't move, hide or go; group layers sit at the end and move only among themselves). `only` lists
  the ball numbers of a group that wear a drawn layer (empty = every ball; `set_layer_only`). `copy_look` leaves
  group layers out and `paste_look` keeps the target's own. `Value::Points` holds a list of points. A layer's settings
  live in the card's params under layered keys `"<layer id>/<key>"` (`layered_key`, `split_layered`); edits are
  `add_layer`, `remove_layer`, `move_layer`, `duplicate_layer`, `rename_layer`, `set_layer_enabled`,
  `copy_layer`/`paste_layer` and `copy_look`/`paste_look`. A **mask** is a layer too (kind `layer.mask`, section
  `LayerSection::Mask`) whose `Layer.parent` is the drawn layer it limits; it sits right after its parent in
  `Slot.layers`, and moving, duplicating, copying or deleting a layer carries its masks (`add_mask`). A layer kind can
  add settings from its own values (`LayerKind::dynamic_defs`, read through `layer_def`): a Shader layer's own code.
  Brush strokes are a value, `Value::Strokes` (points in ball space, never keyframed). A shape's moulded outline is
  `Value::Path(ShapePath)` (`shape_path.rs`: corners in shape units, x right, y down, middle at 0, size 1, each side
  straight or a cubic with two handles; `even(n)`, `circle()`, `add_point`, `delete_point`, `create_curve`, mirror
  partners). Scene files are format 7.
- `bpe-runtime`: the world, the fixed 240 Hz staged step loop, the shared collision pass (`pairs.rs`, `walls.rs`),
  the camera, the event log (`events.rs`) and playback/snapshots. The generic `ThingSim` hooks (spawn, forces,
  settle, after_step) stay for new thing kinds even where no current kind uses them. Walls are **two-sided**:
  `keep_clear` keeps a ball on the side its centre was on at the start of the step (`Bodies.previous`); a hollow
  `Outline::Ring`/`Box` with the ball inside keeps the exact old maths (goldens), `Outline::Path` is a free outline
  (even-odd inside), and `Wall.solid` makes a filled shape that balls bounce off from outside only.
- `bpe-gfx`: the renderer. Things draw into a frame (`frame.rs`) and the GPU draws it: gradients, blend, grade,
  shader effects, particles and trails. Shapes draw through `shaders/paths.wgsl` (`Frame.paths`, `PathInstance`,
  `Frame.push_path`: a filled or banded outline from `Frame.path_points`, group 2 binding 4 beside the layer records,
  or a circle with no points). A shape with paint layers names its records in `PathInstance.layers` (first, count,
  program); `shaders/shape_layers.wgsl` works out where a pixel is against the outline (`shape_at`: distance, edge,
  position along the edge) and the `shape_walk.wgsl` template walks the shape records (table on `LayerGpu`: body 0,
  colour and gradient 10, outline 2, glow 4, shader 6). Shader layers in shapes run in the same in-ball programs as
  balls' (`Made.shape`, `effects/in_shape.wgsl`; `sample` dispatches on `in_ball.shape`). Paths and layered balls
  share one pipeline layout.
  Balls draw through `shaders/layers.wgsl`, one uber-shader: a thing writes
  its `LayerGpu` records once a frame (`frame.look_layers`) and one `LayeredInstance` per ball names them. The
  older `BallInstance`/`LookInstance` pipelines are still in the renderer but nothing in the kit draws with them.
  The stack walk is `shaders/layers_walk.wgsl`, a template pasted in by `layers_source`. Records cover Shading (5),
  Shader (6) and Mask (7) layers; a mask record follows its layer's (whose `head[3]` counts them). Brush strokes reach
  the GPU as 128² tiles in a `MaskAtlas` (`masks.rs`, `Frame.mask_tiles`). Shader layers run **inside** the ball: a
  stack with them draws with a generated program (`programs.rs`, `LookProgram`, `Frame.programs` and
  `Frame.layered_runs`) made of `layers.wgsl` plus each effect once, renamed by `effects::prefixed` and given the
  `effects/in_ball.wgsl` prelude, whose `sample()` walks the layers under the shader layer. `in_ball_problem(code)`
  checks own code without a GPU. The `SOLO` record bit makes a layer show its masks alone (the editor's view).
  Group records sit after a ball's own: 8 the group gradient's colour, 9 a group shader, which runs in the same
  program with uv in the group's square (`LayeredInstance.place`: the ball's start offset and the group's side);
  `layers_group_shaded.wgsl` mixes a group shader into the layers that name it (`amount.z/w` of their records).
  A ball with a Blend layer under other drawn layers draws in two parts (`LayeredInstance.range.z`: the first record
  above the Blend layer, bit 16 for the upper part): the layers under it go through the blend, the ones above paint
  normally over them (`ThingDraw::draw_over`, `Stack::split`). Shapes split the same way (`PathInstance.layers[3]`, `ShapeStack::split`), and the lower part rescales its soft edge by the upper part's cover (`TOP_COVER`, `split_lower_mixed`) so the two parts add up to the whole at the edge. **Z-order**: `Thing.z_order` (-100..100, default 0, in the Placement card) sorts a thing's drawing: every `BlendLayer` has a `z` and a `Place` (Back, Behind, BehindOver, Front, Blend, Over) and `Frame::sort_layers` orders them; z < 0 draws under the scene's back, z > 0 over the balls. A record whose own colours are a gradient carries a
  `paint` (pattern 1 straight across the ball, 2 outward), the Body's too when its colours follow Each ball
  (`Follow::EachBall`, the Body's default). In-ball programs live in a `ProgramStore`; the editor shares one between
  its renderers and builds programs on a background thread (`ProgramStore::in_background`,
  `preview::shared_programs`), exports build them while drawing.
- `bpe-kit`: **every plugin**, with one folder per thing kind in `src/things/` (balls, camera, folder, scene, shape)
  and one per card kind in `src/modules/` (look, trail, motion, bounce, particles, shader). A ball's Look is
  `look.layers` (`modules/look/layers/`: Body, Pattern, Outline, Shine (its Light is white or its own colours/gradient, `ShineLight`), Glow, Shading, Shader, Blend, the reactions
  Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning, Mask, and the group layers Gradient
  (`layers/group.rs`: a colour per ball across the group, mixed into the body on the CPU or into every/chosen
  layer by the shader's group record, code 8) and Shader (`layers/group_shader.rs`: an effect spread across the
  group, record code 9, over the whole ball and optionally a ring around it, or into the Body/chosen layers);
  `gpu.rs` turns a stack into records (a ball wearing only some layers gets them at opacity 0, `records_for`) and has `topmost_at` for clicks on the ball; `paint/brush.rs`
  rasterises strokes to tiles). The **Shape** (`things/shape/`, replacing the old Arena) is an outline balls bounce
  off from both sides: `ShapeSettings` (the `BODY_HEADINGS` settings Start from, Sides, Size, Fill, Thickness and
  the Corners `path`, plus Spin, Shrink, Smallest size); `sim.rs::base_outline` picks the exact Ring or Box form for
  circles, even polygons and axis rectangles and an `Outline::Path` otherwise. Its Look is `look.shape`
  (`modules/look/shape/`): the base `shape.body` (plain white) and the paint layers Colour and gradient (Across /
  From the middle / Along the edge, Flow), Outline, Glow, Shader and Blend (whole-shape); `shape/gpu.rs::ShapeStack`
  turns them into records and `things/shape/draw.rs::wall_form` draws every wall form in the shape's own frame. `look.basic`
  fits no thing kind now. Test scenes are still written with `look.basic` on balls and `classic_cards` turns it
  into layers (`layers_from_basic`). Balls (`things/balls/`) are one ball or, with `group` on, a group in a
  `shape` (`shapes.rs`: Grid, Line, Ring, Filled ring (rings of balls from the middle out, `filled_ring`), Filled circle, Triangle, Honeycomb, Spiral, Heart, Scatter, Word from
  the IBM Plex font, Custom points), with Variety (size, speed, colour, turn spread by `seed`), Release (all at
  once, one by one, waves, via `ThingSim::spawn`) and Launch (same, fan, outward, random). A ball's number in the
  group (`sim::numbers`) is its place in the shape, not its row. Test scenes use `BallsSettings::block`/`ring`,
  which keep the old Block/Ring positions exactly. A folder usually has
  `mod.rs` (settings + kind), `sim.rs` (physics) and `draw.rs`. `lib.rs::kit()` registers them all. `paint/` is the
  colour and gradient system (not the removed canvas). `starters.rs` gives a new thing its starting cards (`add_thing`, `fill_slot`) and
  `scenes.rs` holds the test scenes, compiled only for tests (the `test-scenes` feature, a dev-dependency). They pin
  the old look/motion values with `classic_*` helpers so goldens don't move. Never add built-in or example scenes
  to the program unless the owner asks.
- `bpe-cli`: headless export.
- `bpe-studio`: the egui 0.37 editor. Its look is the Ball Studio Design System (spec
  `docs/superpowers/specs/2026-11-03-design-system-editor-design.md`): `style/` holds every token, the IBM Plex
  fonts (`assets/fonts/`), track colours and drawn controls (switch, track picker, key diamonds). No other editor
  file may make up a colour (a test checks; a real scene colour line ends in `// scene colour`). Every thing but
  the scene settings has a `Thing.colour` (`TrackColour([u8; 3])`, any colour, saved as `"#rrggbb"` (old names still read); a new thing gets a random readable one far from the hues in use, `Scene::new_colour`; editor only, never read by physics or export; picked with `style::track_picker` from the tree's tag and row menu, the timeline swatch and the inspector header). Its layout has the tree on the left, the inspector on the right, the preview
  in the middle and the timeline at the bottom. Key files are `app.rs`, `inspector.rs`, `preview.rs`, `timeline/`,
  `widgets.rs` (settings grids and the right-click value menu), `palettes.rs` and `export_window.rs`.
  A selected shape is moulded in the preview (`shape_handles.rs`: corners, sides, bend handles, a circle's edge,
  Shift mirrors across the shape's middle line, right-click Add point / Create curve / Straighten / Delete point /
  Turn into points); a `look.shape` card shows the Look editor without the ball view or Reactions, and its Body
  shows the shape's `BODY_HEADINGS` settings (changing Sides or Start from removes the path in the same edit).
  A `look.layers` card shows the Look editor (`look_editor/`: the ball view, the Layers and Reactions lists with
  their menus, drag to reorder and rename, masks indented under their layer, then the picked layer's settings; the
  copied layer/look live in `Inspector.clipboard`). `layer_rows.rs` draws a mask's From/Layer and a Shader's
  Effect, `code_box.rs` the own-code box, `handles.rs` the handles on the ball view and `brush.rs` painting strokes
  there. A Balls thing's Look card starts with a Ball / Group switch (`Inspector.look_mode`; in the Balls card when there is
  no layered Look card): Group mode
  (`look_editor/group.rs`, `group_view.rs`) shows the whole group where it starts (custom balls, the launch arrow
  and the gradient's line drag there; balls can be picked for layers of their own), the shape chips and group
  settings, Across the group (the group layers: gradient and shaders) and folds for Variety, Release, Launch and
  Odd ones out. Those settings are left off the Balls card itself (`GROUP_HEADINGS`). The
  timeline (`timeline/`) shows a Hits lane and a group per thing with keys; only the selected thing's group is open.

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

- Tests: `cargo test --workspace` takes several minutes cold, so run it in the background. Without
  `mesa-vulkan-drivers`, GPU picture tests fail with "no graphics adapter". That's an environment problem, not a
  bug. In `--release`, `bpe-runtime`'s `pushing_an_earlier_step_panics_in_debug` always fails (it is debug-only).
- Lint: `cargo clippy --workspace` and `cargo fmt` (max width 120). `cargo fmt` rewraps lines, so scripted text
  replacements can silently miss. Check that the old text is present first.
- Windows exe: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio` (and `-p bpe-cli` for the
  renderer). Rename `ball-studio-next.exe` to `ball-studio.exe` when delivering it.
- Disk is a fixed allowance and `target/` grows large. Use `CARGO_INCREMENTAL=0` and delete `target/debug` when the
  disk fills.
- Screenshots: run the release editor under Xvfb at 1440x960 and capture with `import -window root`. When killing
  it, use `pkill -f "[b]all-studio-next"` (a plain pattern also kills your own shell).

## Golden tests (determinism)

The physics is deterministic, and goldens pin it: `crates/bpe-kit/tests/golden*.txt` (plus `-debug` variants).

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

- Card sim states are `MotionState { settings }` and `BounceState { settings }` (in
  `bpe-kit/src/modules/{motion,bounce}/sim.rs`). `downcast_ref::<BasicBounce>()` compiles but silently returns
  `None`.
- `World::matching(target, me)` lists the things a target points at (Me = the thing itself); `World::names(target,
  me, other)` asks whether it points at one thing (the camera's hit trigger uses it).
- A thing with no Look card draws nothing, and a card of an unknown kind is reported and ignored.
