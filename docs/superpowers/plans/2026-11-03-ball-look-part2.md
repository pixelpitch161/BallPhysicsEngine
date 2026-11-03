# Ball Look editor, part 2 (Masks, Shading, Shader layer, handles, brush): plan

Spec: `docs/superpowers/specs/2026-11-03-ball-look-editor-design.md` (sections Layers, Catalogue, Masks, How things
are changed). Part 1 plan (for how the stack works): `docs/superpowers/plans/2026-11-03-ball-look-part1.md`. Part 1
merged as PR #72. Mockups: canvas https://claude.ai/artifact/C4Nyud3QGE5S15VbR3DwDb, boards "Masks", "Layer
catalogue" (Shading, Shader cards), "How you change things" (handles).

Work rules: superpowers workflow (TDD per task, verify before claiming done). Opus at high effort for every step.
No subagent review per task; ONE Opus review of the whole diff at the end, before the PR. No demo videos or
screenshots unless Aedan asks. Never add built-in or example scenes. Messages to Aedan are plain language. Branch
fresh from `main`.

**Runs in parallel with part 3** (`docs/superpowers/plans/2026-11-03-ball-look-part3.md`). Part 2 merges first. Keep
to the shared contract below so part 3's rebase is trivial.

## Shared contract with part 3 (both plans say the same; don't deviate)

- `LayerSection` gains variants in this order: `Drawn, Reaction, Mask, Group`. Part 2 adds `Mask`; part 3 adds
  `Group`. Add yours at the end of the enum as listed; if the other is already on main, keep both.
- `Layer` gains fields only with `#[serde(default, skip_serializing_if = ...)]`. Part 2 adds `parent:
  Option<LayerId>`; part 3 adds `only: Vec<u32>`.
- `Value` / `ValueType` gain variants at the end. Part 2 adds `Strokes(Strokes)`; part 3 adds `Points(Vec<[f32;
  2]>)`.
- `LayerGpu` grows (part 2) by `extra: [[f32; 4]; 4]` after `paint`. `LayeredInstance` grows (part 3) by `group:
  [f32; 4]` after `range`; `range.w` is part 3's group mode. Part 2 doesn't use `range.z/w`.
- Record codes in `LayerGpu::head[0]`: 0 body, 1 pattern, 2 outline, 3 shine, 4 glow (part 1); 5 shading, 6 shader,
  7 mask (part 2); 8 group gradient, 9 group shader (part 3).
- Generated shader programs (task 6 here) are the only way any effect code runs inside a ball; part 3's group shader
  reuses them.
- `Scene::FORMAT` goes 4 → 5 in part 2. Part 3 bumps to 6 if part 2 is already merged, otherwise to 5 and then
  rebases to 6.

## Outcome

- **Shading** layer: Linear, Radial or Brush darkening; colour (black), amount, stays with the ball or the screen;
  opacity, blend.
- **Shader** layer: any of the 13 built-in effects or own code, with its settings, Speed, Scale, stays with the ball
  or the screen; opacity, blend. Drawn inside the ball, in stack order, in the editor's ball view, the preview and the
  export alike.
- **Masks** on any drawn layer except Body and Blend: From Layer below / Another layer / Shape / Brush / Light or dark
  parts; Invert, Softness, Moves with; several masks multiply. Right-click "Mask with layer below"; a mask thumbnail
  on the row; Alt-click the mask thumbnail shows the mask alone in the view.
- **Handles** in the ball view for the selected layer or mask, **click the ball** to select the top layer covering
  that point, **brush** painting on the big ball for Brush shading and Brush masks, saved and undoable.
- Goldens byte-identical (no physics change). Format 5.

## Decisions (fixed)

1. **A mask is a layer** of kind `layer.mask` (section `Mask`, repeats) whose `Layer.parent` is the layer it limits.
   Its settings are layered keys like any layer's, so keyframes, copy/paste value and undo work unchanged. In the
   card's `layers` Vec a mask sits directly after its parent (and after the parent's earlier masks). Edits keep that:
   moving, duplicating, copying, deleting a layer carries its masks; deleting a parent deletes its masks.
2. **One mask kind with a `from` choice** (`MaskFrom`: `LayerBelow` default, `Layer`, `Shape`, `Brush`, `Light`,
   `Dark`), its other settings shown with `when` conditions. `Layer` names its target as `target: i64` (the target
   layer id; -1 none). The menu lists only drawn layers below the parent. A target that is missing, hidden, above the
   parent or not drawn makes the mask pass nothing (the layer disappears) and the row summary says "masked by a
   missing layer". Chosen over "ignore the mask" so that hiding the target behaves like clipping does in paint apps.
3. **Brush strokes are a value**: `bpe_core::Strokes { list: Vec<Stroke> }`, `Stroke { points: Vec<[f32; 2]>, size:
   f32, softness: f32, strength: f32, erase: bool }`, points in ball space (-1 to 1, y down, before the ball turns).
   `ValueType::Strokes`, never keyframed or pulsed (`Flags::default()`), default empty. Settings grid shows it as a
   summary row "3 strokes" with a **Clear** button; painting happens in the ball view only.
4. **Brushes become textures on the CPU**: each Strokes value is rasterised to a 128 × 128 R8 tile (deterministic:
   distance from each pixel centre to each segment, `smoothstep(size, size*(1-softness), d)` × strength, erase
   multiplies by `1 - that`). Tiles live in a new `MaskAtlas` in bpe-gfx (one 2D-array texture, 128² × N layers,
   grown in powers of two, cached by a hash of the Strokes value). Bound as group 2 binding 1 (texture) and 2
   (sampler) of the layered pipelines. Records point at a tile index (`p1.w`, -1 none).
5. **Shader layers run inside the ball's fragment**, not as a picture pass. Every built-in effect reads the picture
   through `sample(uv)`; inside a ball, `sample` returns *the stack under the shader layer* worked out again at that
   point (a loop over the records below it). So a Kaleidoscope layer mirrors the ball's own pattern, a Lava layer
   paints lava, and layers above the shader paint over it normally. Rules that follow:
   - A shader layer's `sample` sees the layers under it **without** other shader layers (WGSL has no recursion).
     State it in the Shader layer's help: "Sees the layers under it, leaving out other Shader layers."
   - `prev()` (history) returns `sample(uv)` inside a ball (no built-in uses history today; own code that does gets
     no feedback, said in the help).
   - `object_at(uv)` returns the ball's own position and code at that point.
   - `fx.time` is the frame time × Speed; `fx.pulse`, `fx.level`, `fx.hits` are the frame's, as in picture effects.
   - Stays with the ball: `uv = (q * 0.5 + 0.5)` scaled about 0.5 by `1 / Scale`, `q` the ball-space point (turns
     with the ball). Stays with the screen: `uv` = the pixel's screen uv scaled about the ball's screen centre.
     `sample(uv2)` maps `uv2` back to a ball-space point the same way and evaluates the lower stack there; points
     outside the shape are the lower stack's outside (glow) value.
6. **Generated programs**: a stack with shader layers draws with a pipeline built from `layers.wgsl` plus each
   distinct effect source once, its top-level names prefixed (`fx3_effect`, `fx3_Params`, `fx3_hash`…) by
   `effects::prefixed(wgsl, n)`. A generated `fn run_effect(slot: u32, uv: vec2<f32>, l: LayerGpu) -> vec3<f32>`
   switches on the slot. Programs are cached in the renderer by a hash of the ordered list of sources; kept like the
   effect chain's compiled effects (`KEEP_COMPILED`). A stack without shader layers uses the plain part 1 pipeline
   (no change for it).
7. **Own code** reuses today's effect file format (header lines + `fn effect(uv, p)`), stored as the layer's
   `code: Text` setting (shown when Effect = Own code). A file that doesn't parse or compile draws the layer as
   nothing and shows the error under the code box (the chain already reports compile errors; reuse its message
   path). Built-ins are picked by `effect: Choice` from `BUILTINS` ids plus `own`.
8. **Shader-layer settings**: the effect's own settings (from its header) are not fixed per kind, so the Shader layer
   kind's `settings()` lists the common ones (effect, code, speed, scale, space, opacity, blend) plus, generated, the
   union of every built-in's header params under keys `"<effect id>.<param>"` with `when = "effect=<id>"`. Own code's
   params use keys `"own.<param>"`; `def_at` for a layered key of a shader layer with Own code looks them up from the
   layer's parsed code (add `LayerKind::dynamic_def(&self, params: &Params, key: &str) -> Option<SettingDef>`, default
   `None`; the shader kind parses the `code` param and leaks defs once per distinct code hash, as `settings_of` does).
   `.` is allowed in keys; `/` still isn't.
9. **Packing**: up to 16 floats of effect params go in `LayerGpu.extra` (header order, colours three floats, exactly
   `shader::draw::pack_params`). Common values: `p0 = (speed, scale, space, effect slot)`.
10. **Masks on the GPU**: a mask record (code 7) follows its parent's record; the parent's `head[3]` = how many mask
    records follow. The fragment keeps each record's inner coverage in a private `array<f32, 64>`; a mask's value is
    computed from it (Layer below = coverage of the previous drawn record; Layer = coverage of the target's record
    index, resolved on the CPU, -1 = nothing), from shape maths, a brush tile, or the luminance of the stack under
    the parent (Light: `smoothstep(t - soft, t + soft, lum)`; Dark: `1 -` that). Invert, then softness widens the
    edge. Masks multiply into the parent's coverage before it is composited.
11. **Handles and brush live only in the ball view** (not the main preview). Handles are drawn with egui on top of the
    view texture; their positions come from ball-space ↔ view-pixel maps in `ball_view.rs`. A drag is one undo step
    (history snapshot on drag start, values set while dragging, as the settings grid's slider does).
12. **Click to select**: the CPU works out which record covers the clicked ball-space point with a pure function
    `look_layers::topmost_at(stack, q) -> Option<LayerId>` (shape SDF, outline band, pattern always covers, shading
    and shader cover the body, glow only outside). No GPU read-back.

## Task 1: core: masks as child layers, Strokes value, format 5 (bpe-core)

Files: `layers.rs`, `registry.rs`, `value.rs` (or wherever `Value` lives), `settings.rs`, `scene.rs`, `save.rs`.

- `Layer.parent: Option<LayerId>` (serde default, skip if None). `LayerSection::Mask`.
- `add_mask(reg, thing, slot, parent) -> LayerId`: parent must be a drawn layer that isn't the base (Body) or `layer.blend`
  (`LayerKind::maskable()` default true; Body's and Blend's return false). Inserts after the parent's last mask.
  Name "Mask", "Mask 2"…
- Update `move_layer` (moves the parent with its masks; indices count parents only), `duplicate_layer` (copies masks,
  re-keyed, parents pointed at the copy), `remove_layer` (removes masks and their keys/tracks/pulses), `copy_layer`
  (`CopiedLayer.masks: Vec<CopiedLayer>`), `paste_layer` (adds them), `copy_look`/`paste_look` (keep masks).
  `set_layer_enabled` on a mask turns that mask off only.
- Masks can't be moved on their own in part 2 (no reordering of masks; their order doesn't matter, they multiply).
- `Strokes`, `Stroke` types (serde, PartialEq, Hash via bit patterns for caching). `Value::Strokes`,
  `ValueType::Strokes`. Every `match` on Value / ValueType handles it (interpolation: no keyframes, so `Track` never
  sees it; copy/paste value works).
- `LayerKind::dynamic_def` (decision 8); `def_at` falls back to it when the static settings don't have the key.
- `Scene::FORMAT = 5`.

Tests: add_mask happy path and refusals (Body, Blend, a mask, a reaction); move carries masks; duplicate re-keys mask
params and points `parent` at the new layer; remove parent removes masks, keys, tracks, pulses; copy/paste layer and
whole look keep masks; Strokes serde round trip; `def_at` uses `dynamic_def`; format 4 refused.

Verify: `cargo test -p bpe-core`.

## Task 2: kit: Shading, Mask and Shader layer kinds (bpe-kit)

Files: new `modules/look/layers/{shading.rs, mask.rs, shader.rs}`, `layers/mod.rs` (`kinds()`, constants
`SHADING = "layer.shading"`, `SHADER = "layer.shader"`, `MASK = "layer.mask"`), `paint/brush.rs` (rasteriser).

- `ShadingLayer`: `kind: ShadingKind` (Linear default, Radial, Brush); Linear `from: Vec2` (-0.7, -0.7) dark end,
  `to: Vec2` (0.7, 0.7); Radial `centre` (0.3, 0.3), `size` 0.1..2 (0.9), Radial darkens *outside* the size (vignette)
  unless `invert`; Brush `strokes: Strokes`; `colour` black; `amount` 0..1 (0.6); `space: Moves` (Ball default,
  Screen); `softness` 0..1 (0.5); `opacity`, `blend` (Normal). Add-menu order after Glow: Shading, Shader.
- `MaskLayer`: `from: MaskFrom`, `target: i64` (when from=layer), `shape: MaskShape` (Half default, Wedge, Quarters,
  Ring, Circle, Band), `angle` deg, `size` 0..1 (0.5), `strokes` (when brush), `threshold` 0..1 (0.5, when light or
  dark), `invert` bool, `softness` 0..1 (0.05), `moves: MaskMoves` (Layer default, Ball, Screen). Kind `maskable()`
  false, section Mask.
- `ShaderLayer` (hand-written `settings()` because of decision 8; build the defs once in a `OnceLock`): `effect`
  choice (13 built-in ids, then `own`; default `lava`), `code: Text` (when effect=own; default the shortest
  built-in, rainbow_flow's source, so own code starts working), `speed` 0..4 (1), `scale` 0.1..8 (1), `space` (Ball,
  Screen), `opacity`, `blend`, then the generated `"<id>.<param>"` defs.
- `paint/brush.rs`: `pub fn rasterise(strokes: &Strokes) -> [u8; 128 * 128]` (decision 4), pure and deterministic.
- `Stack::of` (gpu.rs) reads the new kinds into `Drawn::Shading`, `Drawn::Shader`, and masks into
  `DrawnLayer { layer: Drawn, id: LayerId, masks: Vec<MaskLayer> }` (refactor `drawn: Vec<Drawn>` into that; keep
  Body unmaskable).

Tests: every new kind registered in menu order; mask kind isn't maskable; shader defs have one `"lava.heat"`-style key
per built-in param, each with `when`; `rasterise` of one stroke is bright on the stroke and 0 far away, erase removes
it, same input gives the same bytes; Stack::of keeps masks with their parent.

Verify: `cargo test -p bpe-kit --lib`.

## Task 3: renderer: records, mask atlas, shading and masks in layers.wgsl (bpe-gfx)

Files: `frame.rs`, `lib.rs`, new `masks.rs` (MaskAtlas), `shaders/layers.wgsl`.

- `LayerGpu.extra: [[f32; 4]; 4]`; update `ZERO`; size stays a multiple of 16. Codes `SHADING = 5`, `SHADER = 6`,
  `MASK = 7` as consts. Field-use table in the doc comment for every code:
  - shading: `p0 = (kind 0/1/2, softness, space, amount)`, `p1 = (from.xy, to.xy)` for linear or `(centre.xy, size,
    tile)` for radial/brush (`tile` in `p1.w` for brush), `colour`.
  - mask: `p0 = (from code, invert, softness, moves)`, `p1 = (angle rad, size, threshold, tile)`, `amount.y` =
    target record index (-1 none), `amount.z` = the parent's angle for Moves with layer.
  - shader: decision 9.
- `MaskAtlas` (`masks.rs`): `tile(&Strokes) -> i32` (rasterise via a callback the kit passes, or take the 128² bytes
  directly: `tile_for(hash: u64, bytes: &[u8]) -> i32`; prefer bytes so bpe-gfx doesn't depend on bpe-kit), upload
  dirty tiles in `FrameBuffers::upload`, a 1-layer black array when empty. `Frame.mask_tiles: Vec<(u64, Arc<[u8]>)>`
  carries this frame's tiles; records hold indices into it.
- Group 2 layout: binding 0 storage (records), 1 `texture_2d_array<f32>`, 2 sampler (linear, clamp).
- `layers.wgsl`:
  - Refactor the loop body into `fn stack_at(q, screen_q, first, count, skip_shaders: bool) -> Stack2` returning
    `inner`, `outer` (so shader sampling can reuse it). The main fragment calls it once with `skip_shaders = false`.
  - Coverage memory `var<private> cov: array<f32, 64>` filled per record.
  - Shading (code 5): factor = linear `smoothstep` along `from→to` (1 at `from`), radial distance outside `size`
    (soft), or brush tile at `q * 0.5 + 0.5`; space Screen uses `screen_q` (unturned); composite `colour` at
    `factor * amount * opacity * mask`.
  - Masks: after a parent's own coverage `c` is known and before compositing, walk its `head[3]` following mask
    records and multiply `c` by each mask value (decision 10); skip those records in the main loop.
  - Shape masks in mask space (`moves`: layer = q turned by `amount.z`, ball = q, screen = screen_q), all with
    `softness` AA: Half (`dot(q, dir) > 0`), Wedge (angle span `size * TAU` from `angle`), Quarters (`sign(x)*sign(y)`
    after turning), Ring (`|length(q) - 0.7| < size * 0.5`), Circle (`length(q) < size`), Band (`|dot(q, n)| < size *
    0.5`).

Tests (`layers_tests.rs`, lavapipe): linear shading darker at `from` than at `to`; radial darker at the rim; brush
shading dark only where a stroke went; Half mask on a red Pattern shows red on one side only, Invert flips it; Layer
below mask clips a pattern to an inside outline; Light mask over a Shine shows only in the highlight; two masks
multiply; a missing target hides the layer; same frame twice identical.

Verify: `cargo test -p bpe-gfx`.

## Task 4: kit: records for shading and masks (bpe-kit gpu.rs)

- `Stack::records` writes shading and mask records (decision 10 layout), resolves `target` layer ids to record
  indices (or -1 per decision 2), adds brush tiles to `frame.mask_tiles` (hash = Strokes hash), and keeps `reach`
  unchanged (shading and masks never reach past the shape).
- `BallsDraw` passes the frame so records can add tiles.

Tests: record order body, pattern, mask, outline…; `head[3]` counts masks; a mask on a hidden target gets -1; brush
tile index points at a tile with the stroke; a hidden mask is left out and `head[3]` drops.

## Task 5: effects: prefixing and in-ball prelude (bpe-gfx effects)

Files: `effects/mod.rs`, new `effects/in_ball.wgsl`.

- `pub fn prefixed(wgsl: &str, n: usize) -> Result<String, String>`: finds top-level `fn`, `const`, `struct`, `var`,
  `alias` names (line-start declarations outside comments) and renames whole-word uses to `fx{n}_<name>`. `effect`
  → `fx{n}_effect`, `Params` → `fx{n}_Params`, the generated `unpack` → `fx{n}_unpack(l)` reading `l.extra` instead of
  `fx.params`. Names from the prelude (`sample`, `prev`, `object_at`, `fx`, `PI`, `TAU`, `fold_uv`, the hit helpers)
  are not renamed.
- `in_ball.wgsl`: the in-ball versions of the prelude functions effects call (`sample`, `prev`, `object_at`, hit
  helpers, `fx` as `var<private> fx: Fx` filled from globals at fragment start). `sample` calls `stack_at(..,
  skip_shaders = true)` at the mapped point (decision 5) using a private "current shader layer" (first record and
  index) set before each `run_effect` call.
- Check every built-in assembles and validates (naga) through `prefixed` + `in_ball.wgsl` + `layers.wgsl`, alone and
  three at once (lava, kaleidoscope, glitch).

Tests: `prefixed` renames declarations and their uses but not prelude names or substrings (`effect_size` stays);
two prefixed copies of the same built-in in one module validate; every built-in validates in-ball.

## Task 6: renderer: generated programs (bpe-gfx lib.rs)

- `Frame.programs: Vec<LookProgram { key: u64, sources: Vec<Cow<'static, str>> }>` and `Frame.layered_runs:
  Vec<LayeredRun { program: Option<u32>, start: u32, end: u32 }>` (runs of `layered` instances in draw order; `None`
  is the plain pipeline). The same in each `BlendLayer.frame`. Part 1's plain drawing becomes one run.
- `Renderer` caches `HashMap<u64, ProgramPipelines { normal, each_variants }>` built from `layers.wgsl` +
  `in_ball.wgsl` + each `prefixed(source, n)` + generated `run_effect`. Object-picture drawing always uses the plain
  object pipeline (shape only). Compile errors: the run draws with the plain pipeline minus shader records (they are
  skipped when `run_effect` doesn't exist: plain `layers.wgsl` has a stub `run_effect` returning the lower stack),
  and the error is kept in `Renderer::program_errors()` keyed by program key, for the editor.
- Program compile happens on first sight; the ball view and preview share nothing, each compiles its own.

Tests (lavapipe): a Lava shader layer over a white body changes pixels inside the ball and none outside; a Pattern
above the shader still shows on top; a Kaleidoscope layer over a striped pattern is symmetric; opacity 0 equals no
layer; a broken own code draws like no shader layer and reports an error; two balls with different effects in one
frame draw with two programs; the same frame twice identical.

## Task 7: kit: shader layer records and runs (bpe-kit)

- `Stack::records` writes shader records (decision 9) with `p0.w` = slot index into the stack's distinct effect
  sources; `Stack::program(&self) -> Option<LookProgram>` gives the sources (built-in `wgsl` borrowed, own code
  owned) and key (hash of sources).
- `BallsDraw` pushes the program (deduplicated by key per frame) and a `LayeredRun` per thing.
- Own code that doesn't parse (`effects::parse`): the layer writes no record and the editor shows the parse error.

Tests: two Lava layers share one source slot; Lava + Glitch give two slots; a parse error writes no record; runs
cover every instance in order.

## Task 8: editor: Shading, Shader and mask rows and settings (bpe-studio)

Files: `look_editor/{mod.rs, layer_list.rs}`, new `look_editor/code_box.rs`.

- Masks show indented under their parent row with a mask thumbnail (white/black disc of the mask's idea); the parent
  row shows a small mask picture after its thumbnail. Clicking a mask row selects it; its settings show below.
- Row menu gains **Add mask** and **Mask with layer below** (adds a mask with From = Layer below) on maskable layers;
  on a mask row: Delete, Copy/Paste value via settings only. Masks' eye works.
- From shown as segmented buttons (None is "Delete mask"); `target` shown as a menu of the layers below by name.
- Shader settings: Effect as a menu button (built-ins by name, then "Own code"), then that effect's own settings
  (the `when` filter does it), Speed, Scale, Stays with, Opacity, Blend. Own code: `code_box.rs`, a monospace
  multi-line `TextEdit` (design-system mono font), edits commit on focus loss or Ctrl+Enter, error text under it
  (from parse, then from `program_errors()` of the ball view's renderer).
- Strokes settings row: "N strokes" + Clear button (Clear is one undo step).
- Summaries: Shading "Linear · 60 %", Shader "Lava", mask "Layer below", "Top half", "Brush · 3 strokes",
  "masked by a missing layer".
- Alt-click a mask thumbnail: the ball view shows the parent's mask alone (white shows, black hides) until Alt-click
  again or selection changes (a `BallView.solo_mask: Option<(ThingId, LayerId)>`; the view frame draws a single
  synthetic Body record white with that mask's records and black ground).

Tests (kittest): add Shading, switch to Radial; add Shader, pick Kaleidoscope, its settings change; Own code with a
syntax error shows an error line; Mask with layer below on a Pattern adds an indented mask row; deleting the parent
removes the mask row; mask target menu lists only layers below; Clear strokes; undo restores each.

## Task 9: ball view handles, click to select (bpe-studio ball_view.rs, new handles.rs)

- `ball_view.rs`: `to_view(q) -> Pos2` and `to_ball(Pos2) -> Vec2` maps (ball radius = 35 % of view, ball turned by
  its Turning angle at the view's time, unstretched in Still and Hit; handles hidden in Moving).
- `handles.rs`: for the selected layer: Outline width (dot on the right edge, drag in/out), Glow size (dot at the
  glow's edge), Shine spot (ring at the spot), Pattern angle (dot on a line from centre), Shading linear from/to (two
  dots and a line) or radial centre and size, Shape mask angle and size, Turning turn (dot above the ball). Each
  handle sets the layered key through the usual `Edit::SetSlot`; drag start snapshots history once. Values shown in a
  small tag beside the handle (design-system mono). Handles use `style/` tokens only.
- Click without drag on the ball (no handle hit): select `topmost_at` (decision 12) in `look_layers` (new pure fn in
  bpe-kit gpu.rs next to `Stack`), else Body.

Tests: `to_view(to_ball(p)) == p`; `topmost_at` picks Outline on the rim, Pattern in the middle, Body when no
covering layer; kittest: dragging the outline handle changes width and one undo restores it.

## Task 10: brush painting (bpe-studio ball_view.rs, brush.rs)

- When the selected entry has a Strokes setting showing (Brush shading, Brush mask), the view shows a brush bar:
  **Paint / Erase**, Size (0.02..0.5 of the radius, default 0.15), Softness, Strength. These brush settings are view
  state (`BallView.brush`), not saved, except that each stroke stores the values it was painted with.
- Press-drag on the ball adds a stroke: points appended while dragging (skip points closer than 0.01), committed as
  one `Edit::SetSlot(key, Value::Strokes(..))` on release (one undo step); while dragging, the view shows the stroke
  live by drawing the frame with the in-progress Strokes value substituted (no edit per mouse move).
- Brush cursor: a circle of the brush size following the pointer over the view.

Tests: a drag across the ball gives one stroke with several points in ball space and one undo step; Erase stores
`erase: true`; points outside the ball are clamped to the ball's square (-1 to 1).

## Task 11: check, docs, cleanup

- `cargo clippy --workspace`, `cargo fmt`, `cargo test --workspace` in debug (background) and `--release` (except the
  known debug-only test). Goldens unchanged in both; never re-bless.
- CLAUDE.md: masks are child layers (`Layer.parent`), `Value::Strokes`, `MaskAtlas`, in-ball shader programs
  (`effects::prefixed`, `in_ball.wgsl`, `Frame.programs`/`layered_runs`), format 5.

## Task 12: review, PR, exe

- One Opus review of the whole diff against the spec and this plan; fix findings.
- PR with the attribution lines from the session reminders; subscribe to PR activity.
- Build Windows exes (`-p bpe-studio -p bpe-cli`). After Aedan says "merge": build from main, copy to
  `F:\!Ai Stuff\BallPhysicsEngine\bin\ball-studio.exe` (keep `ball-studio-backup.exe`) and `bpe-render.exe`.
- Tell the part 3 thread (via the coordinator) once merged, so it can rebase and add the group shader.

## Edge cases

- A mask on a layer whose target is reordered above it: passes nothing; summary warns.
- Masks whose parent is hidden: the masks don't matter; keep them.
- Masks on Glow: glow coverage is outside the shape; Layer below / Layer masks use `outer` coverage of the target
  for glow parents. Shape and brush masks work outside the shape (mask space extends past ±1 for glow; brush tile
  clamps to 0 outside).
- More than 64 records in a stack (masks count): drawn up to 64; the editor notice from part 1 counts masks too.
- Shader layer in a stack with Pattern "turns with the ball": in-ball uv in Ball space turns with the ball too.
- Many balls with a heavy own shader: no limit; it is the user's code. The ball view still redraws at most 30 fps.
- Strokes with thousands of points: rasterise cost is per change (cached by hash), not per frame.
- An effect whose header changes (own code edited): old `"own.<param>"` values for params that no longer exist are
  dropped on load by the usual param cleaning (they don't resolve to a def).
- Copy layer of a Shader with own code: the code goes with it (it is a param).

## Things not to do

- Don't run Shader layers as picture passes or add render passes per ball.
- Don't change physics or goldens; don't re-bless.
- Don't add handles to the main preview.
- Don't let masks be reordered or moved between layers in part 2.
- Don't add group anything (part 3) or part 4 extras.
- Don't add presets, Library, randomize or built-in scenes.
- Don't keep format 4 loading.
