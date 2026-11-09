# File map

Where each part of the code lives, after the 2026-11-09 split of the giant files (W17, plan
`docs/superpowers/plans/2026-11-09-split-giant-files.md`). A big file became a folder: its `mod.rs` keeps the types
and what the parts share, and each child file does one job. Every old path still works (`bpe_gfx::Frame`,
`crate::inspector::show`, `crate::frame::LightGpu`, …) because each `mod.rs` re-exports what its children used to
export.

Each folder's `mod.rs` starts with a short map of its own files; this page is the overview.

## bpe-gfx (the renderer)

| Path | What is there |
| --- | --- |
| `lib.rs` | Module list and re-exports, the shader sources several pipelines share (`LIGHTING_WGSL`, `scene_source`, `layers_source`), the graphics devices for tests (`test_device`, `headless_device`) |
| `frame/` | What to draw for one frame. `mod.rs`: `Frame`, blend layers' records (`Place`, `BlendLayer`), clear and what a frame gives off |
| `frame/build.rs` | `ThingDraw`, `ModuleDraw`, `Draws`, `DrawCx`, `build_frame*`: each thing's drawing part fills the frame |
| `frame/instances.rs` | GPU records: `LayerGpu`, `LayeredInstance`, `PathInstance`/`PathStyle`/`PathChannel`, `FlashInstance`, `HitMark`, `BallMark` |
| `frame/trails.rs`, `particles.rs`, `paths.rs` | Trails and streaks (strips); sprites (`Particle`) and fields; shapes and text as paths and glyphs |
| `frame/layers.rs`, `cull.rs` | Blend layers and the overlay; the view (`set_view`), list limits and culling |
| `frame/light.rs` | What the lighting reads: `LightScene`, `LightGpu`, `LightingGpu`, surfaces, mirrors, spheres, throwers, pipes, benders, `LightShow` |
| `frame/effects.rs` | `EffectCall`, `Stage`, `Takes`, `BackgroundPaint` |
| `frame/grade.rs` | The picture's grade: `GradeStack`, `GradeMask`, `GradePanel`, `GradeBasic`, `GradeToneCurve`, the colour panels (`GradeHsl`, `GradePointColour`, `GradeColourGrading`, `GradeCalibration`), `GradeDetail`, `GradeEffects` |
| `grade/` + `shaders/grade.wgsl`, `shaders/scopes.wgsl` | The grade compiled to a program cut into segments, one a pass (`compile.rs`, `Segment`), its maths (`maths.rs`), the Histogram (`scopes.rs`) |
| `frame/ripples.rs` | Ripple sources |
| `renderer/` | The `Renderer`. `mod.rs`: its fields and small accessors, `Globals`, `FrameBuffers`, `GrowableBuffer`, vertex layouts |
| `renderer/new.rs` | Building it: every pipeline, layout and sampler (the composite/post pipeline too) |
| `renderer/scene.rs` | Rendering a frame: `render`, `render_scene` (the scene pass and lighting), `finish` (effects, bloom, the grade program's sync, the grade's passes and the composite pass) |
| `renderer/draw.rs` | The draw calls inside the passes: backgrounds, balls, things, sprites, liquids, glass, overlay, layers |
| `renderer/targets.rs`, `grade.rs`, `paint.rs` | The renderer's pictures (HDR, bloom levels, object and layer pictures); the grade program's buffer and its passes (`GradeGpu`: the HDR pairs, the blur pyramid, `run_passes`); the paint texture |
| `readback.rs` | `Readback`: frames back to the CPU for export (RGBA or video colours) |
| `light_pass.rs` + `shaders/light_passes.wgsl` | The light-pass numbers by name (`PICTURE`, `LIT`, `SURFACE`, `EMISSION`, `BACKGROUND`, `UV`, `TAGS`, `BEHIND`, `TINT`, `GLASS`); shaders test `light_pass(g.light.x) == PASS_LIT` and so on. A test keeps the two lists equal |
| `lighting/` | The lighting passes. `mod.rs`: `Lighting` and its uniform; `kit.rs`: pipelines and layouts (`LightKit`); `run.rs`: one frame's lighting in order (plan, upload, passes); `passes.rs`: each pass's encoding; `targets.rs`: its pictures and quality tiers; `glass.rs`: dichroic, caustic and stage uniforms, the rainbow, `SphereGrid`, `photons_per_pair`; `binds.rs`: the bind group cache; `tiles.rs`: light tiles |
| `shaders/lighting/` | The lighting shader, one file per pass, joined in this order by `LIGHTING_WGSL`: `bindings`, `field`, `shadows`, `direct`, `light_field`, `haze_bloom`, `stage`, `shade`, `mix`, `dof_floor`, `bake`, `caustics` (then `common.wgsl`) |
| `shaders/post.wgsl` | Bloom, the grade's passes (`vs_grade`, `fs_grade_pass`, the pyramid's `fs_grade_down0`/`fs_grade_down`) and the composite (`fs_composite`: exposure, the grade's last segment (`run_grade`), roll-off, sRGB, dither) |
| `effects/` | The Effects stack (`chain.rs`), lens effects, built-in effects |

## bpe-studio (the editor)

| Path | What is there |
| --- | --- |
| `app/mod.rs` | `Studio` (everything the editor holds), building it, and the frame (`eframe::App::ui`) |
| `app/runs.rs` | Edits and undo steps, and the runs they restart (physics, aim, particles, sound) |
| `app/playing.rs` | The playhead, play/seek/step, the speakers and auditions |
| `app/files.rs`, `actions.rs` | New, open, save and asking before losing work; undo, redo, copy, paste, duplicate, delete |
| `app/autosave.rs`, `problems.rs` | Autosave and crash copies; problems in the status line |
| `app/shortcuts.rs` | Keyboard shortcuts |
| `app/panels.rs` | The toolbar, the left panel (tree, Mixer, Lights), the inspector's panel, grid controls, the status line |
| `app/dialogs.rs` | Stop-export, restore and confirm dialogs |
| `inspector/mod.rs` | The inspector's state, `show` (top to bottom, one call per part), card frames and folding |
| `inspector/header.rs`, `find.rs` | Name, kind, eye, tags; Find a setting |
| `inspector/own.rs` | The kind's own card (the scene's one card per group, a Balls thing's Ball / Group switch) |
| `inspector/placement.rs` | The Placement card |
| `inspector/slots.rs`, `physics.rs`, `surface.rs` | One card per slot (header with Type and Remove, then its body); the Physics card; a Look card's Surface section |
| `inspector/emitter_card/`, `add_card.rs` | The Emitter card; "+ Add card" |
| `inspector/edits.rs` | `Edit` (what cards ask for) and applying it after the drawing; `set_or_key` |
| `widgets/mod.rs` | What setting rows share: `GridCx`, `Row`, `TargetChoices`, `GridOutcome` |
| `widgets/grid.rs`, `setting.rs`, `menu.rs` | The settings grid and group headings; one setting's row and widget; its right-click menu |
| `widgets/numbers.rs`, `pickers.rs`, `words.rs` | Number sliders and their feel; who, target and colour pickers; settings in words |
| `grade_mode/` | Grade mode: `mod.rs` (state, the mode switch, the masks column), `cx.rs` (reading the card, `GradeEdit`, `apply`), `panels.rs` (the panel column), `curve.rs` (the Tone Curve graph), `scopes.rs` (the Histogram), `colour.rs` (the colour panels' bodies and coloured rails), `wheel.rs` (Colour Grading's wheel) |
| `mixer/`, `lights_panel/`, `look_editor/`, `timeline/`, `preview/` + `preview.rs`, `rules_panel/`, `sound_tab/`, `looks_page/`, `style/` | Unchanged by the split: see `editor.md` |

## bpe-runtime (the physics)

| Path | What is there |
| --- | --- |
| `world/mod.rs` | `World`, its cards (`Slotted`), `Between`, `Lent`, `Card`, what it tells the outside |
| `world/build.rs` | Building a world from a scene |
| `world/step.rs` | One step: stages, substeps, moving bodies, collisions and bounces |
| `world/animate.rs` | Keys, pulses and rules worked out before each step (`physics_animation`) |
| `world/lend.rs` | Lone copies; lending states and cards |
| `world/prints.rs` | Fingerprints (`fingerprint`, `sound_fingerprint`, `beside_fingerprint`) and a scene's parts (`main_part`, `heard_part`, `is_beside`) |
| `pairs.rs`, `walls.rs`, `history.rs`, `playback.rs`, `rules/` | Unchanged by the split: see `engine.md` |

## Old names

Notes written before the split name the old files. They are now:

| Old file | Now |
| --- | --- |
| `bpe-gfx/src/lighting.rs` | `lighting/` (`Lighting::run` is `lighting/run.rs`, the pipelines `lighting/kit.rs`) |
| `bpe-gfx/src/shaders/lighting.wgsl` | `shaders/lighting/*.wgsl` |
| `bpe-gfx/src/frame.rs` | `frame/` (`strip` is `frame/trails.rs`) |
| `bpe-gfx/src/lib.rs` (the `Renderer`, `Readback`) | `renderer/`, `readback.rs` |
| bare light-pass numbers (`g.light.x > 2.5`, `BEHIND_PASS`, `TINT_PASS`, `GLASS_PASS`) | `light_pass::*` and `PASS_*` |
| `bpe-studio/src/app.rs`, `inspector.rs`, `widgets.rs` | `app/`, `inspector/`, `widgets/` |
| `bpe-studio/src/emitter_card/` | `inspector/emitter_card/` |
| `bpe-runtime/src/world.rs` | `world/` |

## Where the colour grading is

Part 1 is built (`docs/notes/grading.md`): `SlotId::Grade` and the `Mask`, `MaskPart`, `Panel` sections in
`bpe-core`; the card and panels in `bpe-kit/src/modules/grade/`; `frame/grade.rs`, `grade/` and
`shaders/grade.wgsl` in `bpe-gfx` (run in the composite, `renderer/grade.rs`); Grade mode in
`bpe-studio/src/grade_mode/` (chosen in `app/mod.rs::ui`, the switch in `app/panels.rs`, keys in
`app/shortcuts.rs`, the preview's compare in `preview.rs`, the timeline filter in `timeline/mod.rs`, the Scene
settings summary in `inspector/own.rs::grade_summary`). Part 2 added the colour panels (`hsl.rs`, `point_colour.rs`, `colour_grading.rs`,
`calibration.rs` in the kit; four ops in `grade/compile.rs` and the shader). Part 3 added area steps and passes
(`renderer/grade.rs`, `post.wgsl`), Basic's Texture, Clarity and Dehaze, and the panels `detail.rs` and `effects.rs`.
Later parts add panels to `modules/grade/`, steps to
`grade/compile.rs` and the shader, and masks as more `Mask` layers.

## Still long

The split moved code without rewriting it. Functions over about 250 lines remain, each now in a file of its own
topic: `renderer/new.rs::new` (pipeline descriptors in order), `renderer/scene.rs::render_scene`,
`bpe-studio/src/tree.rs::show`, `preview.rs::show`, `bpe-kit/src/things/text/looks.rs::add_text`,
`bpe-runtime/src/rules/actions.rs::apply_rule_actions`. Cut one up when a change needs it.
