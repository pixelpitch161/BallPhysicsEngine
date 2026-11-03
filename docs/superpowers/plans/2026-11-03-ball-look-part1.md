# Ball Look editor, part 1 (Layers core): plan

Spec: `docs/superpowers/specs/2026-11-03-ball-look-editor-design.md` (read it first; this plan only adds code
detail). Mockups: canvas https://claude.ai/artifact/C4Nyud3QGE5S15VbR3DwDb, board "D Ball with a layer stack" and
"How you change things". Branch: start a fresh branch from `main`.

Work rules: superpowers workflow (TDD per task, verify before claiming done). Opus at high effort for every step.
No subagent review per task; ONE Opus review of the whole diff at the end, before the PR. No demo videos or
screenshots unless Aedan asks. Never add built-in or example scenes. Messages to Aedan are plain language.

## Outcome

- A new Balls thing gets a `look.layers` card holding one Body layer: a plain white circle, Normal blend, no shine,
  no outline, no glow, no flash.
- The Look card of a Balls thing is the Look editor: ball view (Still / Moving / Hit, Dark / Scene), Layers list,
  Reactions list, selected entry's settings.
- Layers: Body, Pattern, Outline, Shine, Glow, Blend. Reactions: Flash on hit, Colour on hit, Stretch when fast,
  Colour over time, Turning. Any number of drawn layers; Blend and each reaction at most once.
- Every drawn layer but Body and Blend has Opacity and a Blend mode against the layers under it.
- One render path draws balls (`layers.wgsl`). The classic `BallInstance` path and `look.basic` no longer draw
  balls. The arena keeps `look.basic`.
- File format 4. Goldens (physics) byte-identical.

## Decisions (fixed; don't reopen)

1. **Layer params are flat layered keys** `"<id>/<key>"` in `Slot.params`. Chosen so `Address`, tracks, pulses,
   copy/paste value, `Animation::apply` and undo need no change. `/` never appears in a plain setting key (add a
   debug assert in the derive output check, task 1).
2. **Layer list lives on `Slot`** (`Slot.layers: Vec<Layer>`), not on `Thing`, because layers belong to the Look card.
3. **`LayerId(u32)`**, new id = max existing + 1 (Body is 0). Ids are never reused within a card while it lives,
   except that a pasted whole look replaces the list and its ids.
4. **Body is index 0 always.** `move_layer` refuses index 0 and refuses moving anything to index 0. Reactions are kept
   after drawn layers in the Vec (edits keep that invariant: adding a drawn layer inserts it before the first
   reaction; adding a reaction pushes to the end).
5. **Body's Size row edits the thing's own `radius`** (an `Edit::SetOwn("radius", ..)`), shown at the top of the
   Body settings. Body has no `size` key of its own.
6. **Body colour is today's `Paint`** with key prefix `""` (keys `fill`, `fill_*` exactly as in `BasicLook`), default
   one colour white `#ffffff`. Keep the same `follows` list so gradients across the group still work.
7. **Shine replaces Style Shiny.** Body is always flat. Shine's default (strength 1, spot (-0.35, -0.4), size 1)
   must give the same pixels as today's `sphere_light` for a flat white ball within 2/255 (GPU test).
8. **Colour reactions recolour Body colour only** (plus Pattern when its colours are "shade of the body"), exactly as
   today's `hue_shifted` does with `fill`.
9. **Per-layer blend** uses W3C compositing with `blended()` from `blend.wgsl` (copy the function into
   `layers.wgsl` via the same include mechanism used by `styled.wgsl`; don't fork the maths):
   `Cs' = (1-αb)·Cs + αb·B(Cb,Cs)`, `co = αs·Cs' + (1-αs)·αb·Cb`, `αo = αs + αb·(1-αs)`, all premultiplied out.
10. **Whole-ball blend** (Blend layer) feeds `build_frame` the same three values look.basic did (`blend`,
    `blend_amount`, `blend_each`). `build_frame` reads them through a new `ThingDraw::blend(&ResolvedThing)` hook (or
    the existing routing fn) that, for `look.layers`, reads the first enabled `layer.blend`'s layered keys.
11. **Uber-shader**, no generated WGSL in part 1.
12. **Ball view** is a second `bpe_gfx::Renderer` drawing a mini scene; same texture pattern as `preview.rs`.
13. **Format 4**; format 3 files refused with the existing `LoadError::OldEditor` message.
14. **Starter**: `starter_kind(SlotId::Look)` returns `look.layers` for balls, `look.basic` for others (`fill_slot`
    already knows the thing kind; pass it through).
15. `Edit` keys change from `&'static str` to `String` everywhere in `inspector.rs` (layered keys are built at run
    time).

## Task 1: core data model (bpe-core)

Files: `crates/bpe-core/src/scene.rs`, `registry.rs`, `address.rs`, `lib.rs`, new `layers.rs`.

- `layers.rs`: `pub struct LayerId(pub u32)` (Copy, Ord, Hash, serde transparent, Display). `pub struct Layer { id:
  LayerId, kind: KindId, name: String, enabled: bool }` (serde default, `enabled` default true). Helpers:
  `pub fn layered_key(id: LayerId, key: &str) -> String`, `pub fn split_layered(key: &str) -> Option<(LayerId,
  &str)>` (None for plain keys or a non-number prefix).
- `scene.rs`: `Slot.layers: Vec<Layer>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  `Slot::layer(id)`, `Slot::layer_index(id)`, `Slot::next_layer_id()`. `Scene::FORMAT = 4`.
- `registry.rs`: `pub enum LayerSection { Drawn, Reaction }`. `pub trait LayerKind: Send + Sync { fn id(&self) ->
  &'static str; fn name(&self) -> &'static str; fn help(&self) -> &'static str; fn settings(&self) -> &'static
  [SettingDef]; fn section(&self) -> LayerSection; fn repeats(&self) -> bool; fn card(&self) -> &'static str; }`.
  `Registry::add_layer(Box<dyn LayerKind>)` (panic on duplicate id, like `add_module`), `layer(&str) ->
  Option<&dyn LayerKind>`, `layers_for(card: &str) -> impl Iterator` in registration order.
- `address.rs`: in `def_at`, when `addr.slot` is Some and `split_layered(&addr.key)` is Some, find the layer in the
  slot, get `reg.layer(kind)`, and look the rest of the key up in its settings. `value_at` / `set_at` follow
  automatically if they go through `def_at` (check; if they look up defs themselves, route them through the same
  helper `slot_def(scene, reg, thing, slot, key)`). Unknown layer id or kind: `None`, same as an unknown key today.
- Debug check: a test in `registry.rs` that no registered card or layer setting key contains `/`.

Tests (`crates/bpe-core`, using a tiny test registry with a fake card kind and two fake layer kinds, as other core
tests do): `split_layered` cases (`"3/width"`, `"width"`, `"x/width"`, `"3/"`); `def_at` finds a layer setting;
`value_at` returns the default when unset; `set_at` stores and a default value removes the key; serde round trip of
a `Slot` with layers and of one without (no `layers` field written).

Verify: `cargo test -p bpe-core` green.

## Task 2: resolve, load, edits (bpe-core)

Files: `resolve.rs`, `save.rs`, `edit.rs`.

- `resolve.rs`: `ResolvedLayer { id, kind, name }`; `ResolvedSlot.layers: Vec<ResolvedLayer>` (enabled, known kinds,
  in order). For each, write the layer kind's defaults then the stored layered values into `params` under layered
  keys. `ResolvedSlot::layer_params(&self, id) -> Params` (plain keys; builds a new `Params`, fine for per-frame use
  at ball counts we have, but cache per thing in draw if profiling says so). New `ProblemKind::UnknownLayerKind(KindId)`;
  the layer is skipped. A disabled layer is not listed and its keys are not written.
- Animation: confirm `Animation::apply` writes `params.set(key)` into the resolved slot params, so a track on
  `"3/width"` animates. A track on a disabled or missing layer writes a key nobody reads: harmless, leave it.
- `save.rs`: when cleaning a card's params, also keep layered keys whose layer exists and whose key the layer kind
  knows (with the value type it expects); drop the rest. Layers of unknown kind are kept (reported at resolve), as
  unknown cards are today. Older format: existing `OldEditor` path (FORMAT bump does it).
- `edit.rs` (on `Scene`, each returns `Result<_, EditError>` like `add_slot`):
  - `add_layer(reg, thing, slot, kind) -> LayerId`: kind must exist and `card()` must equal the slot card's kind;
    refuse a second copy of a non-repeating kind (`EditError::OnlyOne`); drawn layers insert before the first
    reaction, reactions push. Name = kind name, or "Name 2", "Name 3"… for the lowest free number.
  - `remove_layer(thing, slot, id)`: refuse Body (index 0). Removes the layer, every `"<id>/…"` param, and every
    `tracks` / `pulses` entry whose Address is that slot with a key starting `"<id>/"`.
  - `move_layer(thing, slot, id, to_index)`: drawn layers only, `1 ..= last drawn index`; refuses Body and reactions.
  - `duplicate_layer(...) -> LayerId`: copy kind, params (re-keyed), tracks and pulses (re-keyed), name + " copy";
    inserted right above the original. Refuse Body and non-repeating kinds.
  - `rename_layer`, `set_layer_enabled` (refuse disabling Body).
  - `paste_layer(reg, thing, slot, CopiedLayer) -> LayerId` where `pub struct CopiedLayer { kind, name, params:
    BTreeMap<String, Value> (plain keys) }`; same rules as `add_layer`, then values written. A Body paste replaces
    Body's values instead of adding.
  - `copy_layer(thing, slot, id) -> Option<CopiedLayer>` and `copy_look(thing, slot) -> Option<CopiedLook>`
    (`CopiedLook { layers: Vec<CopiedLayer> }`), `paste_look(...)` replaces the whole list and params (tracks and
    pulses of the old layers are removed; the copy carries no keyframes).

Tests: resolve fills defaults under layered keys; disabled layer absent; unknown layer kind reported; `layer_params`
strips prefix; a track on `"1/width"` changes the resolved value at t; load drops a stale layered key and a key of a
removed layer; each edit's happy path and every refusal; remove cleans params, tracks, pulses; duplicate re-keys
tracks; paste_look replaces; format 3 file refused.

Verify: `cargo test -p bpe-core` green; `cargo clippy -p bpe-core` clean.

## Task 3: layer kinds (bpe-kit)

Files: new `crates/bpe-kit/src/modules/look/layers/{mod.rs, body.rs, pattern.rs, outline.rs, shine.rs, glow.rs,
blend.rs, reactions.rs}`, `modules/look/mod.rs`, `lib.rs::kit()`, `starters.rs`.

- `look.layers` card kind (`LayersLook`, `ModuleKind`): id `look.layers`, name "Look", slot Look, `fits` balls only,
  empty settings. Change `look.basic`'s `fits` to exclude balls.
- One `#[derive(Settings)]` struct + `LayerKind` per kind. Reuse existing `Choice` enums (`BallShape`, `Blend`) and
  `Paint` / `InsidePaint`; don't copy them.
  - `layer.body` "Body": `shape: BallShape` (circle), `roundness` (as BasicLook), `fill: Paint` (embedded, prefix
    `""`, follows as BasicLook, default white). Drawn, repeats false.
  - `layer.pattern` "Pattern": `inside: InsidePaint` (embedded, prefix as BasicLook's), `opacity` 1, `blend` Normal.
    Default pattern must not be "None" (otherwise adding it shows nothing): Stripes, shade of the body.
  - `layer.outline` "Outline": `width` px (0..60, default 4), `sits: OutlineSits` (Inside default / Centre / Outside),
    `colour_from: ColourFrom` (SameAsBody default / Own), `colour` (white, `when` Own), `opacity`, `blend`.
    "Same as body" draws the body colour darkened 40 % so a white ball shows a visible grey ring (state it in help).
  - `layer.shine` "Shine": `strength` 0..1 (1), `spot` point (-0.35, -0.4), `size` 0.2..3 (1), `opacity`, `blend`.
  - `layer.glow` "Glow": `strength` 0..2 (0.8), `size` 0.05..4 (0.8, radii), `colour_from`, `colour`, `opacity`,
    `blend`.
  - `layer.blend` "Blend": `blend`, `blend_amount`, `blend_each` with BasicLook's defs. Drawn section but drawn
    nothing; repeats false. The layer list shows it in Layers.
  - Reactions (section Reaction, repeats false): `react.flash` "Flash on hit" (flash 0.4, flash_size, flash_seconds as
    BasicLook), `react.hue` "Colour on hit" (`hue_per_hit`), `react.stretch` "Stretch when fast" (`stretch`, default
    a visible amount, e.g. 0.3), `react.rainbow` "Colour over time" (`rainbow`), `react.turning` "Turning" (`turn`,
    `spin`, `face_motion`).
  - Copy `help` text from BasicLook's attributes where the setting is the same.
- `starters.rs`: `starter_kind` gains the thing kind; balls get `look.layers` and `fill_slot` then calls
  `add_layer(.., "layer.body")` once. Nothing else is added.
- `scenes.rs` (test-only): replace `classic_look()` for balls with `classic_layers()` that builds the equivalent
  stack: Body (colour), Shine (if the old look was Shiny), Outline (if outline > 0), Glow (if glow > 0), Flash (if
  flash > 0) etc. Goldens only pin physics, so test scenes may look different; picture tests that pinned the old ball
  look get re-checked in task 6.

Tests: kit registers every kind; no layer setting key has `/`; a new Balls thing has `look.layers` with exactly one
layer, Body, white circle, and no reaction; the arena still gets `look.basic`; `look.layers` doesn't fit the arena
and `look.basic` doesn't fit balls.

Verify: `cargo test -p bpe-kit --lib` green.

## Task 4: frame records (bpe-gfx frame.rs)

- `#[repr(C)] Pod` `LayerGpu` (multiple of 16 B): `head: [u32;4]` = (code, blend mode, flags, 0); `amount: [f32;4]`
  = (opacity, a, b, c); `p0, p1: [f32;4]` per-kind parameters; `colour, colour2: [f32;4]` linear premultiplied-ready
  RGBA; `paint: PaintGpu` (existing struct; row -1 none). Layer codes: 0 body, 1 pattern, 2 outline, 3 shine, 4
  glow. Document each code's `p0`/`p1` layout in a comment table at the struct.
- `#[repr(C)] Pod` `LayeredInstance`: `center: [f32;2]`, `radius`, `seed`, `stretch: [f32;2]` (direction × amount),
  `angle`, `reach` (how far outside the radius the stack draws: max glow size and outside outline), `first: u32`,
  `count: u32`, `shape: u32`, `roundness: f32`, `body: [f32;4]` (body colour after reactions), `opacity`, `flags`,
  pad. Keep under 128 B.
- `Frame.look_layers: Vec<LayerGpu>`, `Frame.layered: Vec<LayeredInstance>`, plus the same in each `BlendLayer` and
  the overlay, mirroring how `look` instances are routed today. `clear()` empties them.

Tests: size and alignment asserts (`size_of::<LayerGpu>() % 16 == 0`, `LayeredInstance <= 128`).

## Task 5: renderer (bpe-gfx lib.rs, new shaders/layers.wgsl)

- `FrameBuffers`: one more `GrowableBuffer` per instance list for `layered`, and one STORAGE buffer per list for
  `look_layers`. `GrowableBuffer` today is VERTEX | COPY_DST; add a constructor taking usage (`STORAGE | COPY_DST`).
  Grow to the next power of two; recreate the bind group when it grows.
- Bind group layout 2: one read-only storage buffer, FRAGMENT visibility. Pipeline layout for layered pipelines:
  [globals, paint, layers].
- `layers.wgsl`: vertex expands the instance to a quad of half-size `radius * (1 + reach)` plus stretch, like
  `ball_look.wgsl` does. Fragment: local point → undo stretch and angle → `d = ball_shape(p, shape, roundness)`
  (reuse the function; share via the existing include/concat approach). Loop `i in first..first+count` (cap 64):
  - body: coverage from `d` with 1 px AA; colour from `paint_colour` with the layer's `PaintGpu`, or `body` colour.
  - pattern: today's inside-paint code path, coverage = body coverage.
  - outline: band by `sits` (inside `[-w,0]`, centre `[-w/2,w/2]`, outside `[0,w]`) in pixels, AA both edges.
  - shine: `sphere_light`-style highlight at `spot`, coverage = body coverage. Outputs white with alpha = light.
  - glow: outside the body, falloff `strength * exp(-(d/size)^2)`-style matching today's glow formula; inside 0.
  - composite with the layer's blend (decision 9), multiplied by opacity.
  - Output premultiplied; multiply by instance opacity.
- Pipelines: normal, object-picture variant (`fs_layers_object` writing the same as `fs_look_object` does for look
  instances), and an `each.rs` `Kind::Layered` variant for blend-with-each-other. Draw order: layered instances draw
  where look instances drew (same list position), so stacking against trails/particles is unchanged.
- Remove the classic `BallInstance` drawing of balls only if nothing else uses it (check `pictures.rs` and the
  arena); otherwise leave the pipeline and stop pushing balls to it.

Tests (`crates/bpe-gfx`, lavapipe picture tests next to `paint_tests.rs`, new `layers_tests.rs`): a white circle is
white at centre, transparent outside, AA at edge; outline inside vs outside changes where grey pixels are; Multiply
pattern darkens, Screen lightens; shine is brightest near its spot; glow is non-zero just outside the radius and zero
with strength 0; disabled (count 0 after body) equals body alone; same frame twice gives identical bytes; Shine
default ≈ old Shiny within 2/255 (decision 7).

Verify: `cargo test -p bpe-gfx` green under mesa lavapipe.

## Task 6: balls draw through layers (bpe-kit)

Files: `things/balls/draw.rs`, `modules/look/draw.rs`, new `modules/look/layers/gpu.rs`, `modules/trail/draw.rs`.

- `gpu.rs`: `pub fn records(slot: &ResolvedSlot, ...) -> (Vec<LayerGpu>, LayerSummary)` built once per thing per
  frame (layer settings are per thing; only Body colour, seed and hue vary per ball). `LayerSummary { reach, shape,
  roundness, blend: Option<(Blend, f32, bool)>, reactions }`.
- `BallsDraw::draw`: for `look.layers`, push the thing's records once to `frame.look_layers` (or the blend layer's),
  then one `LayeredInstance` per ball with `first/count` pointing at them. Per-ball: body colour from the Body paint
  at the ball's place/order (existing paint evaluation), then reactions: Colour on hit (`hue_shifted` by hits ×
  turn), Colour over time (rainbow), Stretch when fast (`STRETCH_SPEED` = 900 as today), Turning (angle). Flash on
  hit pushes `FlashInstance` exactly as today, only when the reaction exists. Hit marks unchanged.
- Pattern with "shade of the body" needs the per-ball body colour: pass it in the instance `body` field and have the
  shader use it when the pattern record's flag says so (no per-ball records).
- `ball_colors()` (used by trails and particles): body colour with colour reactions applied, reading the layered
  card. Keep the signature; trails and particles keep working.
- Blend routing: `build_frame` (frame.rs ~line 874) gets the whole-ball blend from the summary (decision 10).
- Remove the balls branch that used `look.basic` / `BallInstance`.

Tests (kit, CPU-side, no GPU): new ball → one instance, count 1 (body only), white, no `FlashInstance` on a hit;
adding Flash → a flash on hit; Outline record has the right code and width; a disabled Outline → count 1; Blend layer
Add → the thing routes into an Add blend layer; Colour on hit turns body hue after hits; Stretch when fast stretches
above speed; trails read the body colour. Goldens: `cargo test -p bpe-kit --test golden*` unchanged in debug AND
`--release` (no re-bless; if a golden moves, something touched physics: stop and find it).

## Task 7: inspector edits become String keys (bpe-studio)

- `inspector.rs` `Edit::SetOwn`, `SetSlot`, `NewKey` take `String`. Update every constructor and the apply code;
  `reached()` unchanged. Pure refactor, no behaviour change.
- New edits: `AddLayer(SlotId, &'static str)`, `RemoveLayer(SlotId, LayerId)`, `MoveLayer(SlotId, LayerId, usize)`,
  `DuplicateLayer`, `RenameLayer(SlotId, LayerId, String)`, `LayerEnabled(SlotId, LayerId, bool)`,
  `PasteLayer(SlotId, CopiedLayer)`, `PasteLook(SlotId, CopiedLook)`. Structure edits act on the primary thing only.
  `SetSlot` with a layered key reaches other selected things only if their card has a layer with that id and the same
  kind.
- Every edit goes through the existing history so undo works.

Verify: `cargo test -p bpe-studio` green before moving on (existing kittests prove the refactor).

## Task 8: Look editor UI (bpe-studio look_editor/)

Files: new `crates/bpe-studio/src/look_editor/{mod.rs, layer_list.rs, clipboard.rs}`, `inspector.rs`, `style/`.

- `inspector.rs`: when the slot card kind is `look.layers`, call `look_editor::show(ui, cx, thing, slot)` instead of
  `settings_grid`. `Inspector` gains `selected_layer: BTreeMap<ThingId, LayerId>` (default Body; fall back to Body if
  the stored id is gone).
- `mod.rs`: ball view (task 9) on top; then "Layers" header with `+ Add layer` (`menu_button`, drawn kinds from
  `reg.layers_for("look.layers")`, non-repeating ones already present greyed); rows top to bottom = highest index
  first, Body last; then "Reactions" header with `+ Add reaction` and rows; then a heading with the selected
  entry's name and its settings via `settings_grid` with keys mapped to layered keys (wrap `get` and map
  `changes`/`new_keys` back). Body's settings start with a "Size" row bound to the thing's own `radius` def (reuse
  the def from the Balls kind's settings, edits as `SetOwn("radius")`).
- `layer_list.rs` row: grip (not on Body), eye toggle (not on Body), painted thumbnail (small circle in the layer's
  main colour; Body = body colour; Outline = ring; Glow = soft dot; Shine = white spot; Pattern = two stripes; Blend =
  two overlapping circles), name (double-click → inline `TextEdit`, Enter/blur commits, Esc cancels, empty name
  restores the kind name), summary on the right (e.g. "4 px · inside", "Circle · white", "Add 100 %"). Selected row
  uses the design system's selected row token. Drag to reorder with egui's `dnd_drag_source` / `dnd_drop_zone`
  (drawn rows only; drop above Body only). Right-click menu: Duplicate, Copy layer, Paste layer, Copy whole look,
  Paste whole look, separator, Move up, Move down, separator, Delete. Items that can't apply are disabled (Body:
  only Copy layer, Paste layer onto Body, Copy/Paste whole look).
- `clipboard.rs`: an app-level `Option<CopiedLayer>` and `Option<CopiedLook>` in the app state (not the OS
  clipboard), surviving selection changes.
- Colours only from `style/` tokens (the existing test enforces it). Accessibility names via `labelled_by` on every
  control (kittest needs it); pickers are `menu_button`s.
- Timeline lane label: `timeline/lanes.rs:66` builds "Card · Setting"; for a layered key use "<layer name> ·
  <setting label>".

Tests (kittest, `crates/bpe-studio/tests/` like the existing ones): new Balls thing → Look editor shows Body only, no
reactions; add Outline → row appears selected and its settings show; change Width; rename; hide (eye); reorder two
layers by Move up; duplicate; copy layer, select another ball, paste; copy whole look, paste on another ball; delete;
Blend greyed in the add menu once added; Create keyframe on Outline Width → lane "Outline · Width"; undo restores
each step; arena Look card still the old grid.

## Task 9: ball view (bpe-studio look_editor/ball_view.rs)

- `BallView { renderer: Option<bpe_gfx::Renderer>, texture: Option<TextureId>, size: u32, mode: ViewMode
  (Still/Moving/Hit), ground: Ground (Dark/Scene), clock: f64, last_key: u64 }`, one per editor (in app state).
- Mini scene: the scene root's settings (for Scene ground) and a copy of this thing alone, resolved; a synthetic
  `World` with one body at the origin, radius from the thing, velocity 0 (Still/Hit) or (700, 0) (Moving), hits added
  to the event log at the loop start in Hit mode (1.2 s loop). Camera: centred, zoom so radius = 35 % of view size.
  The ball view never steps physics; it only builds a frame.
- Render at `size = min(card width, 300) * pixels_per_point`, square. Same `register_native_texture` /
  `update_egui_texture_from_wgpu_texture` pattern as `preview.rs`. Dark ground clears to `#151515` (`// scene colour`).
- Redraw rule: Still redraws when a hash of (resolved thing, playhead, size, ground) changes; Moving/Hit request a
  repaint after 33 ms while the card is visible.
- Buttons Still / Moving / Hit and Dark / Scene as segmented buttons over the top edge of the view, design-system
  styled.

Tests: unit test that the mini scene puts one ball at the view centre with the expected radius; kittest that the
buttons change `mode`/`ground`; no GPU picture test of the editor.

## Task 10: whole-workspace check and cleanup

- Remove now-dead code: the balls branch in `look/draw.rs`, `is_classic_ball` if unused, classic ball pipeline if
  unused (task 5 note). `cargo clippy --workspace` clean, `cargo fmt`.
- Update `CLAUDE.md` crate map: `look.layers`, `LayerKind`, layered keys, `look_editor/`, `layers.wgsl`, format 4.
- `cargo test --workspace` (debug, background, several minutes) and `cargo test --workspace --release` except the
  known debug-only test. Goldens unchanged in both.

## Task 11: final review, PR, exe

- One Opus review subagent of the whole diff against the spec and this plan; fix what it finds.
- Open the PR (attribution lines as the session reminders say), subscribe to PR activity.
- Build `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli` and keep the exes ready.
  After Aedan says "merge": build from main, copy to `F:\!Ai Stuff\BallPhysicsEngine\bin\ball-studio.exe` keeping
  `bin\ball-studio-backup.exe`, and `bpe-render.exe` likewise.

## Edge cases

- A ball with 0 radius or a stack whose reach is huge (glow size 4 + outside outline 60 px): quad still bounded by
  `radius * (1 + reach)`; clamp reach to 6 radii.
- Many layers: no limit in data; the shader loop caps at 64 records per ball and the editor says "Only the first 64
  layers are drawn" under the list if exceeded.
- Old keyframes on a deleted layer are removed with it; keyframes on a hidden layer stay and work again when shown.
- Multi-select of balls with different stacks: layer list shows the primary thing's; setting edits reach others only
  with same id + kind (decision in task 7).
- Paste layer of a non-repeating kind already present: replace that layer's values instead of refusing.
- Paste whole look onto a ball whose card isn't `look.layers` (can't happen in part 1; guard anyway: disabled item).
- Body cannot be hidden, deleted, moved, duplicated; its menu disables those.
- Pattern "shade of the body" with Colour on hit: shade follows the shifted body colour.
- Ball view with Scene ground and a scene gradient background: draw the scene's background pass, not the arena.
- Selected layer deleted or undone away: selection falls back to Body.

## Things not to do

- Don't change physics or golden files. Don't re-bless goldens.
- Don't keep two ways of drawing balls.
- Don't add presets, Library, randomize, or built-in scenes.
- Don't change the arena's Look card or `look.basic`'s settings.
- Don't generate WGSL per stack (part 2 only, own-code shaders).
- Don't start masks, shading, shader layers, handles, brush, or group view (parts 2 and 3).
- Don't keep backward compatibility with format 3 files.
- Don't read `pointer.press_origin()` in a `clicked()` handler; don't use `ui.add_space` inside a Grid; use
  `query_all_by_label(..).next()` in kittests (see CLAUDE.md pitfalls).
