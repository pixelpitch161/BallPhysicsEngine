# Plan: sub-layers (part 1)

Spec: `docs/superpowers/specs/2026-11-04-sublayers-trails-physics-design.md`, "Part 1: Sub-layers". Read it first;
this plan doesn't repeat the kinds table or the per-kind GPU maths, it says where each piece goes and how it is
checked. Mockup of the list (board D): https://claude.ai/artifact/HuhqLfZ8W6vGh3SzafLXmW.

Branch: the build thread's own, from `main`. Builds beside part 3 (Physics card); see the spec's merge order.
Part 2 (trail/particle layers) builds on this part, so keep the sub-layer APIs named as below.

## Decisions (all fixed; don't reopen)

1. Sub-layers are layers with `Layer.parent = Some(layer)`, sitting straight after their layer in `Slot.layers`
   (today's mask rule). `LayerSection::Mask` is renamed `LayerSection::Sub`.
2. Ten sub kinds per look card. Balls (`look.layers`): `sub.cut`, `sub.glow`, `sub.outline`, `sub.shadow`,
   `sub.shading`, `sub.shine`, `sub.shader`, `sub.recolour`, `sub.flash`, `sub.hue`. Shapes (`look.shape`):
   `shape.sub.cut` ... `shape.sub.hue` (same ten). A kind belongs to one card, as every layer kind does.
3. Which parent takes which sub: `LayerKind::takes_subs() -> bool` (renamed from `maskable`) on the parent, plus
   new `LayerKind::takes_sub(&self, sub: &str) -> bool` (default: `true`). Part 2's trail/particle layers override
   it. Body, Blend, reactions, group and sound layers: `takes_subs() == false`.
4. At most `MAX_SUBS = 12` sub-layers per layer (`bpe-core`).
5. Order: sub-layers apply in card order (top to bottom in the box). The piece mixes in at its layer's place with
   its layer's Opacity and Blend.
6. One GPU pass: the walk carries a `Piece` with a signed shape distance; no offscreen textures (spec, "How it
   draws").
7. Record codes 20..29 (`LayerGpu::SUB_CUT` = 20 ... `SUB_HUE` = 29). `LayerGpu::MASK` (7) goes. `head.w` of a
   layer's record counts its sub records. `MAX_LAYERS` 64 → 128.
8. Per-ball hit data: `LayeredInstance.hit: [f32; 4]` and `PathInstance.hit: [f32; 4]` (seconds since last hit,
   strength, hits so far, 0).
9. Brush tiles get a distance channel (`Rg8Unorm`).
10. File format 9. A new registry upgrade hook does the `layer.mask` → `sub.cut` rename (below).
11. Ball-wide reactions (Flash on hit, Colour on hit) stay as they are.

## Things not to do

- No sub-layers in sub-layers; no sub-layers on Body, Blend, reactions, group or sound layers.
- No offscreen per-ball rendering. No brush on shapes.
- Don't change physics; goldens must not move (`git diff main -- crates/bpe-kit/tests/` empty).
- No demo renders or screenshots; GPU picture tests and kittests are the checks.
- Don't keep `add_mask`/`Edit::AddMask`/"Add mask" menu items as aliases: rename everywhere.

## Tasks

Each task ends green: `cargo test -p <crates touched>` (debug), `cargo clippy`, `cargo fmt`. Run the full workspace
suite in debug and `--release` at the end (Task 9). GPU tests need `mesa-vulkan-drivers` (CLAUDE.md).

### Task 1: core data and edits (`bpe-core`)

Files: `crates/bpe-core/src/registry.rs`, `layers.rs`, `save.rs`, `scene.rs`, `testkit.rs`, `edit.rs` if it lists
sections.

1. `LayerSection::Mask` → `LayerSection::Sub` (rename every use: `grep -rn "LayerSection::Mask" crates`).
2. `LayerKind::maskable` → `takes_subs`; add `fn takes_sub(&self, _sub: &str) -> bool { true }`.
3. `pub const MAX_SUBS: usize = 12;` in `layers.rs`.
4. `Scene::add_mask(reg, thing, slot, parent)` → `Scene::add_sub_layer(reg, thing, slot, parent, kind: &str) ->
   Result<LayerId, EditError>`. Refuse (`EditError::NotAccepted`) when: `parent` is base, not Drawn, or itself a
   sub; `!pk.takes_subs()`; `kind` isn't a `LayerSection::Sub` kind of this card (`k.card() == card.kind`);
   `!pk.takes_sub(kind)`; the parent already has `MAX_SUBS`. Missing kind → `EditError::UnknownKind`. Name: the
   kind's name, numbered as `add_layer` numbers names. It goes after the parent's last sub (today's position rule).
5. `Scene::move_sub_layer(reg, thing, slot, id, to: usize) -> Result<(), EditError>`: `id` must be a sub; `to`
   counts among its siblings from 0 (clamped); other layers keep their places.
6. `CopiedLayer.masks` → `subs`. `copy_layer`, `copy_look`, `duplicate_layer`, `remove_layer`, `move_layer`,
   `paste_layer`, `paste_look` keep today's mask behaviour under the new names. `paste_layer` of a copied
   sub-layer on its own stays refused (as a mask's is today).
7. New `Scene::paste_sub_layer(reg, thing, slot, parent, copied: &CopiedLayer) -> Result<LayerId, EditError>`: adds
   `copied` (a sub kind) as `parent`'s last sub with its settings, same refusals as `add_sub_layer`.
8. Upgrades: `Registry` gains `upgrades: Vec<(u32, fn(&mut Scene))>` with `pub fn add_upgrade(&mut self, below:
   u32, f: fn(&mut Scene))`; `save::load` runs, in order, every upgrade whose `below` is greater than the file's
   format, after `mend` and before `clean`/`clean_layers`. `Scene::FORMAT` = 9 (`OLDEST_FORMAT` stays 7). Test:
   a registry with an upgrade below 9 runs it on a format 8 text and not on a format 9 one.
9. Tests: port every mask test in `layers.rs` (`masks_go_after...`, `a_moved_layer_takes_its_masks...`,
   `a_duplicate_brings_copies...`, `removing_a_layer_removes_its_masks...`, `copied_layers_and_looks_keep...`,
   `a_mask_whose_layer_is_gone...`, `a_pasted_look_renumbered...`) to sub-layers using `testkit`'s fake kinds
   (rename its fake mask kind to a sub kind, and give `testkit` a second fake sub kind so order tests mean
   something). Add: the 12 limit; `takes_sub` refusing a kind; `move_sub_layer` within siblings and clamping;
   `paste_sub_layer`.

Verify: `cargo test -p bpe-core`.

### Task 2: sub kinds and their settings (`bpe-kit`)

Files: `crates/bpe-kit/src/modules/look/layers/mask.rs` → rename to `cut.rs` (`MaskLayer` → `CutLayer`,
`MaskFrom` → `CutFrom`, `MaskShape` → `CutShape`, `MaskMoves` → `CutMoves`; codes unchanged); new
`crates/bpe-kit/src/modules/look/layers/subs.rs`; `layers/mod.rs`; `crates/bpe-kit/src/modules/look/shape/mod.rs`
and new `shape/subs.rs`; `crates/bpe-kit/src/lib.rs` (registration and the upgrade).

1. `subs.rs` holds the new settings structs (spec table): `SubGlow` (`strength`, `size`, `where_: GlowWhere`
   key `"where"`, `colour_from: SubColourFrom` (SameAsLayer, Own), `colour`, `gradient`, `direction`, `opacity`),
   `SubOutline` (`width`, `sits: OutlineSits`, `colour_from`, `colour`, `gradient`, `direction`, `opacity`),
   `SubShadow` (`angle`, `distance`, `softness`, `colour: Gradient` solid black, `opacity`, `moves: ShadowMoves`
   (Screen, Ball)), `SubRecolour` (`mode: RecolourMode` (Tint, Hue, Gradient, Light), `tint: Color`, `amount`,
   `turn`, `colours: Gradient`, `gradient`, `direction`, `brightness`, `contrast`, `saturation`, `opacity`; `when`s
   by mode), `SubFlash` (`strength`, `length`, `colour: Color`, `follow_impact: bool`), `SubHue`
   (`turn_per_hit`). Defaults and ranges from the spec table; ranges for Recolour: brightness -1..1, contrast
   0..2, saturation 0..2. Shading, Shine and Shader sub kinds reuse `ShadingLayer`, `ShineLayer`, `ShaderLayer`
   (`shader::defs`, `shader::dynamic_defs`).
2. `layers/mod.rs::kinds()`: remove the `MASK` kind; add the ten `sub.*` kinds (section `Sub`, repeats true,
   `takes_subs` false) at the end of the drawn/reaction kinds, before the group kinds, in the spec's order. Drawn
   kinds keep `takes_subs: true` (Body and Blend false). Consts `SUB_CUT` ... `SUB_HUE`; drop `MASK`.
3. `shape/mod.rs`: the same ten as `shape.sub.*` for `look.shape`; shape paint kinds (Colour and gradient,
   Outline, Glow, Shader) `takes_subs: true`. Shape Cut uses a `ShapeCutLayer` (or `CutLayer` with `when`s) whose
   From leaves out Brush; shape Shading leaves out Brush: implement as separate `#[derive(Choice)]` enums
   (`ShapeCutFrom`, `ShapeShadingKind`) so the menus can't offer Brush. Shape sizes (Glow size, Shadow distance)
   are in **world pixels** (shape walk works in pixels): Glow size 1..400 px (40), Shadow distance 0..200 px (12),
   Outline width as today's shape outline.
4. The `Kind` struct field `maskable` → `takes_subs`; `impl LayerKind for Kind` maps it.
5. Upgrade (registered in `lib.rs` where the registry is built): `reg.add_upgrade(9, masks_to_cuts)` where
   `masks_to_cuts` renames every layer of kind `"layer.mask"` to `"sub.cut"` in every card of every thing.
6. Update every use (`grep -rn "MaskLayer\|MaskFrom\|MaskShape\|MaskMoves\|layers::MASK\|maskable"
   crates`), including `bpe-studio`.
7. Tests: the registry test lists the new ids in order; `drawn_layers_but_body_and_blend...` asserts
   `takes_subs` for drawn kinds only and `repeats` for drawn and sub kinds; a format 8 file with a `layer.mask`
   layer loads with a `sub.cut` one and the same params (write the RON by hand in the test, as `save.rs` tests
   do).

Verify: `cargo test -p bpe-kit` (non-GPU parts; GPU tests may fail until Task 4 if records change; keep them
compiling).

### Task 3: renderer data (`bpe-gfx`, Rust side)

Files: `crates/bpe-gfx/src/frame.rs`, `masks.rs`, the pipeline set-up that declares `LayeredInstance` and
`PathInstance` vertex attributes (find with `grep -rn "LayeredInstance" crates/bpe-gfx/src`).

1. `LayerGpu`: remove `MASK`; add `SUB_CUT = 20` ... `SUB_HUE = 29` with a doc line each. `MAX_LAYERS` = 128.
2. `LayeredInstance.hit: [f32; 4]` and `PathInstance.hit: [f32; 4]`, appended last; extend the instance vertex
   attribute lists and the WGSL `LayeredIn`/path input structs to match (new `@location`s after the last used).
   Default: `[1e6, 0.0, 0.0, 0.0]` (never hit) wherever instances are built with `..Default::default()` or by
   hand; grep every construction.
3. `masks.rs`: tiles become two channels: R = cover (as today), G = distance to the cover's 0.5 edge, in tile
   units where 1 tile side = 2 ball radii, mapped `g = 0.5 + clamp(dist / 0.25, -1, 1) * 0.5` (inside > 0.5).
   Compute it in `bpe-kit/src/modules/look/paint/brush.rs` after rasterising (two-pass 3-4 chamfer distance over
   the 128² cover, from the 0.5 threshold), so the atlas stays a byte uploader. Texture format `Rg8Unorm`; tile
   byte length 2 × 128². Update the cache key if it hashes bytes.
4. Tests (`masks.rs`, `brush.rs`): a painted disc's G channel is 0.5 at its edge, above inside, below outside,
   symmetric; R unchanged from before (compare against today's bytes for one fixed stroke set).

### Task 4: the walk (`bpe-gfx`, WGSL)

Files: new `crates/bpe-gfx/src/shaders/sub_layers.wgsl`; `layers_walk.wgsl`; `layers.wgsl`; `shape_walk.wgsl`;
`shape_layers.wgsl`; `layers_shader_case.wgsl`; `shape_shader_case.wgsl`; `effects/in_ball.wgsl`,
`effects/in_shape.wgsl`; `lib.rs::layers_source` (and the shape equivalent) to paste `sub_layers.wgsl` in;
`programs.rs` (in-ball programs gain the sub-shader case).

1. `layers.wgsl`: `shape_mask` → `shape_mask_d(shape, m, size) -> f32` returning the signed distance used today
   inside `soft_inside(...)` (each case's first argument), and `shape_mask(shape, m, size, s) =
   soft_inside(shape_mask_d(...), s)`. `mask_value` → `cut_value` returning `(cover_in, cover_out, dist)`: dist
   from `shape_mask_d` (Shape), the tile's G channel ((g - 0.5) * 0.5 * 2, negated so inside is negative)
   (Brush), `(0.5 - v.x) * 0.1` otherwise; Invert flips cover and distance. `tile_at` reads `.r`; new `tile_d_at`
   reads `.g`. `COV_IN/COV_OUT` arrays 64 → 128.
2. `sub_layers.wgsl` (shared by both walks; no ball- or shape-specific globals: take what it needs as arguments):
   - `struct Piece { c_in: vec3<f32>, a_in: f32, c_out: vec3<f32>, a_out: f32, light_in: vec3<f32>,
     light_out: vec3<f32>, sd: f32 }`
   - `fn piece_over(p: Piece, c: vec3<f32>, b_in: f32, b_out: f32) -> Piece` (paint over, straight colours:
     `a' = b + a(1-b)`, `c' = (c b + c_p a_p (1-b)) / a'`), `fn piece_under(...)` (the same with the piece on top),
   - `fn sub_glow(p, l, unit) `, `fn sub_outline(p, l, aa, unit)`, `fn sub_shadow(p, l, shifted_sd, unit)`,
     `fn sub_shading(p, factor, l)`, `fn sub_shine(p, l, lit_at)`, `fn sub_recolour(p, l)`,
     `fn sub_flash(p, l, hit)`, `fn sub_hue(p, l, hit)`, each per the spec's "Each sub-layer record on the piece".
     `unit` converts the record's size to the walk's distance unit (balls: radii, sizes already in radii except
     Outline width in px → `/ radius`; shapes: world px).
   - Colour helpers it needs (`hue_turn`, luminance) come from what `layers.wgsl`/`paint.wgsl` already define;
     add `hue_turn` if missing (same maths as the CPU `shift_hue` in `bpe-core` colours, so tests can compare).
3. `layers_walk.wgsl`: for each top-level record `k` with `head.w = n` sub records:
   - compute `c, a_in, a_out` as today (cases 0..6), and `sd` per the spec (`d` for body/pattern/shine/shading/
     shader, the band for outline, `(0.5 - a_out_raw) * size` for glow); a top-level glow's light starts in
     `light_out` (cover 0) as today.
   - build `Piece`, loop `j in 1..=n` over `look_layers[first + k + j]` dispatching on `head.x` 20..29 to the
     functions above. Cut's "Layer under it"/"Layer" read `COV_IN/COV_OUT[below]`/`[target]` and the Light/Dark
     cuts read the stack under the layer, exactly as `mask_value`'s callers do today (keep the
     `lower && above` / `whole_in` logic for split balls).
   - Shadow: `shifted_sd` = the layer's own `sd` re-evaluated at `p - offset` (offset from Angle and Distance;
     Moves with Screen uses `screen` space as Shading does), then each **earlier** Shape/Brush cut of this piece
     re-evaluated there (`max`); Light/Dark/Layer cuts skipped. Write this as `fn piece_shape_at(first, k, j, q)`
     in the walk template (it needs the ball's shape function and records).
   - mix the piece in where today `c, a_in, a_out` are mixed (`layer_over`, the glow-light path, `top_over` etc.
     for split balls): covered colour from `c_in/a_in`, `c_out/a_out`, then light as today's glow light.
   - `COV_IN/OUT[k]` = the piece's final covers. The SOLO view (head.z & 4) shows the product of the piece's cut
     covers, as today's masks solo.
   - `k += 1 + n` as today.
4. `shape_walk.wgsl`: the same, with `sd` from `s.edge` (filled: `s.edge`; band layers: their band), sizes in
   world px, hit data from the path instance. Shapes had no masks: add the piece loop fresh. Keep the shape's
   split logic as is, a piece counting as its layer.
5. Sub Shader (26): in `layers_shader_case.wgsl` add `case 26u` used **inside the piece loop** (the template
   pastes a sub-case marker `//SUB_SHADER_CASE` into the piece loop; empty when there are no shader layers).
   `effects/in_ball.wgsl`: `InBall` gains `piece: u32` (0: the stack under the layer, today's; else
   `1 + parent record index`) and `piece_count: u32`; `in_ball_begin_piece(l, first, parent_k, j, q, screen)`
   sets them so `sample()` walks `look_layers[first + parent_k ..]` with count `1 + (j - 1)` (the layer and the
   subs before this one; the walk's `min(head.w, count - k - 1)` already stops at the count) and returns its
   colour over black. The same for `in_shape.wgsl`. `programs.rs`: a program is needed when any top-level or
   sub shader record is present; effect sources include sub shaders' (`Stack::effect_sources`).
6. Tests in `crates/bpe-gfx/src/layers_tests.rs` and `paths_tests.rs` (GPU; build records by hand as those tests
   do), each comparing pixels at named points:
   - `a_glow_on_an_outline_lights_the_body_inside_the_ring`: body white-ish grey, outline cyan, sub Glow: a point
     inside the body near the ring is brighter than with no sub; a point at the centre is not changed.
   - `cut_then_glow_differs_from_glow_then_cut`: half cut + glow on an outline: with the cut first, the glow is
     present round the cut's straight edge; with the glow first, the cut removes the glow on the hidden half.
   - `a_shadow_falls_under_its_layer_at_its_angle`: a shadowed pattern disc darkens the body on the shadow side
     only.
   - `recolour_changes_only_its_layer`, `a_flash_lights_only_its_layer_after_a_hit` (hit data age 0 vs 1e6),
     `colour_on_hit_turns_by_hits`, `a_shader_sub_layer_sees_only_its_layer` (a test effect returning `sample()`
     shows the layer's colour, not the body's, outside the layer's cover), `outline_follows_the_cut_edge`.
   - shapes: a glow sub on a shape's Outline brightens the shape's fill inside the band.
   - every existing mask picture test, ported to Cut records, gives the **same pixels** as before (keep their
     expected values).

### Task 5: records (`bpe-kit` GPU side)

Files: `crates/bpe-kit/src/modules/look/layers/gpu.rs`, `crates/bpe-kit/src/modules/look/shape/gpu.rs`,
`crates/bpe-kit/src/things/balls/draw.rs`, `crates/bpe-kit/src/things/shape/draw.rs`.

1. `DrawnLayer.masks: Vec<MaskLayer>` → `subs: Vec<SubLayer>` with `pub enum SubLayer { Cut(CutLayer),
   Glow(SubGlow), Outline(SubOutline), Shadow(SubShadow), Shading(ShadingLayer), Shine(ShineLayer),
   Shader(Box<ShaderLayer>), Recolour(SubRecolour), Flash(SubFlash), Hue(SubHue) }` (and `id: LayerId` beside it
   for picking/solo). `Stack::of` reads `sub.*` layers into their parent's `subs` (hidden subs skipped, hidden
   parents' subs skipped, as masks are today).
2. `Stack::records`: after a layer's record, one record per sub, codes 20..29, fields per the WGSL above
   (document the packing in a comment above the match, as `LayerGpu`'s field docs do). Cut records carry today's
   mask fields. `head.w` = number of sub records written. A sub Shader whose own code doesn't read writes no
   record (as top-level).
3. `effect_sources`, `program`: include sub shaders. `reach`: a sub Glow adds its size, a sub Outline sitting
   Outside its width, a Shadow its distance plus softness, on top of the layer's own reach.
4. `records_for` (balls that don't wear a layer): mark the layer's record UNWORN as today; its sub records need no
   change (the walk skips `1 + head.w`).
5. `split`: count records with subs (a split index is a record index; it must land on a layer's record, never a
   sub's).
6. Hit data: in `balls/draw.rs`, from the hits the draw already scans for flashes, per ball: seconds since its
   last hit, that hit's strength (`speed / FULL_FLASH_SPEED`, at most 1.5), its hit count since the start (the
   log has every accepted hit; count per ball index with one pass per frame, cached on nothing: it's cheap at
   scene sizes; if profiling says otherwise, keep a running count in `World`). Write `LayeredInstance.hit`.
   Only scan when the stack has a Flash or Colour-on-hit sub (`Stack::needs_hits()`); else `[1e6, 0, 0, 0]`.
   Shapes: the same for hits on the shape (`things/shape/draw.rs`), into `PathInstance.hit`.
7. `ShapeStack` (`shape/gpu.rs`): the same `subs` and records, shape units.
8. Tests (non-GPU, in `gpu.rs` tests): records order and codes for a layer with three subs; hidden sub and hidden
   parent; `head.w`; the sub shader joins `effect_sources` once per distinct effect; reach grows by a sub glow;
   split never points at a sub; `records_for` with subs; `topmost_at` ignores subs. Port
   `masks_stay_with_their_shown_layer`, `masks_follow_their_layer_and_name_their_target_by_record`,
   `a_mask_moving_with_a_pattern_turns_only_as_the_pattern_spins` to cuts. A draw test that a ball hit 3 times
   gets `hit[2] == 3` and age near 0 just after its last hit; a shape the same.

### Task 6: editor list and menus (`bpe-studio`)

Files: `crates/bpe-studio/src/look_editor/layer_list.rs`, `look_editor/mod.rs`, `look_editor/layer_rows.rs`,
`crates/bpe-studio/src/inspector.rs` (`Edit` and `apply_layer_edit`, `Inspector` fields), `look_editor/clipboard.rs`.

1. `Edit::AddMask(SlotId, LayerId, &'static str)` → `Edit::AddSubLayer(SlotId, LayerId, &'static str /* kind */)`;
   `Edit::MoveSubLayer(SlotId, LayerId, usize)`; `Edit::PasteSubLayer(SlotId, LayerId /* parent */, CopiedLayer)`.
   `apply_layer_edit` calls the new core edits and picks the new sub-layer (`inspector.selected_layer`).
2. `Inspector.folded_pieces: BTreeSet<(ThingId, LayerId)>` (editor state, never saved).
3. `layer_list.rs`:
   - `masks_of` → `subs_of`. `top_level` unchanged.
   - `piece(ui, inspector, cx, card, layer, picked, edits) -> Rect`: when `subs_of(card, layer.id)` is empty,
     draw `row(...)` as today. Otherwise draw a frame: 1 px `style::LINE` border, `style::GROUND` fill, 2 px
     corners (mockup D), containing the header row (today's row plus a fold arrow after the grip; folded shows
     "▸ n" muted after the name), then (unless folded) each sub row indented 30 px with a 14 px thumbnail and
     26 px height, then a "+ Sub-layer" row (muted, small) that opens `sub_kind_menu`. Return the whole box rect
     (the drag-and-drop of top-level layers uses it).
   - `sub_kind_menu(ui, cx, card, parent, edits)`: the sub kinds of the card for which the parent's kind
     `takes_sub(kind)`, in registry order, with a separator before Flash on hit; greyed with hover "Up to 12
     sub-layers" at 12.
   - `menu(...)`: replace "Add mask" and "Mask with layer below" with a submenu "Add sub-layer" (`ui.menu_button`)
     holding `sub_kind_menu`, enabled when the kind `takes_subs`. "Paste layer" pastes a copied sub-layer as the
     last sub (`Edit::PasteSubLayer`) and a copied top-level layer as today.
   - sub row menu (today's `mask_menu` grown): Rename, Duplicate, Copy layer, Paste layer (a copied sub-layer:
     after this one, via `PasteSubLayer` then `MoveSubLayer`), Delete.
   - Drag a sub row: payload `DraggedSub(ThingId, LayerId /*parent*/, LayerId)`; a drop target within the box
     draws the gap line and pushes `Edit::MoveSubLayer`; dropping outside its box does nothing.
   - Thumbnails: Cut keeps `paint_mask_thumbnail` (renamed `paint_cut_thumbnail`); add small painted icons for
     the others (halo, ring, offset disc, half-dark disc, shine spot, "fx" text, swatch of the Recolour tint, a
     4-point star, a 3-colour wheel), using `style` colours and the layer's own colour where it has one.
   - Summaries (`settings_summary`): Cut keeps `mask_words`; Glow "0.8 strong"; Outline "6 px inside"; Shadow
     "135°"; Recolour mode word; Flash "0.3 s"; Colour on hit "30° a hit"; Shading/Shine/Shader as their
     top-level summaries.
   - `alt_click_solo` stays for Cut sub rows.
4. `mod.rs::show`: draw `piece(...)` for each top-level drawn layer (top first, as now); `records` counting for
   `DRAWN_LIMIT` counts subs. When a sub-layer is picked, the settings heading reads "<sub name> on <layer
   name>". The shapes' Look editor uses the same `piece` (no ball view).
5. `layer_rows.rs`: `mask_rows` → `cut_rows` (From / Layer rows; Layer lists the layers under the parent, as
   today). Sub Shader uses `shader_rows`. The rest use the settings grid with the kind's defs (dynamic defs for
   the shader), as other layers do.
6. `handles.rs` / `brush.rs`: handles and the brush work on sub kinds as they did on masks/shading: Cut Shape
   (angle/size handles), Cut Brush and Shading Brush (brush target), Shading Linear/Radial (handles), Shadow (one
   handle at the offset, dragging sets Angle and Distance). `brush::target` matches `sub.cut` (From Brush) and
   `sub.shading` (Kind Brush) as it matched masks and shading.
7. `clipboard.rs`: `Clipboard.layer` can hold a sub-layer (its kind tells which); no new field.

### Task 7: editor tests (kittests)

In `look_editor` tests (follow `look_editor/mod.rs` tests' helpers: `add(&mut h, "+ Add layer", "Outline")`):

1. `add_sub_layer_from_the_menu_makes_a_box`: add Outline; right-click its row; "Add sub-layer" ▸ "Glow"; the card
   has a `sub.glow` with parent the outline; the list shows "+ Sub-layer"; the Glow is picked and its heading reads
   "Glow on Outline".
2. `plus_sub_layer_adds_another_at_the_end`.
3. `dragging_a_sub_row_reorders_within_its_box_only` (drop on another box: nothing changes).
4. `folding_hides_the_sub_rows_and_counts_them`.
5. `a_sub_row_menu_duplicates_copies_pastes_and_deletes`; `pasting_a_sub_layer_on_a_layer_adds_it_last`.
6. `the_body_blend_and_reactions_offer_no_sub_layers` (menu item disabled/absent).
7. `a_shape_layer_takes_sub_layers_without_brush` (Cut's From has no Brush).
8. `twelve_is_the_most` (13th greyed).
9. `old_mask_tests_still_hold_as_cuts`: port the existing mask kittests (search `"Add mask"` in studio tests).

Remember the kittest pitfalls in CLAUDE.md (named controls, `query_all_by_label(..).next()`, `scroll_to_me`, idle
steps before a double-click after a menu).

### Task 8: docs

`CLAUDE.md`: replace the mask sentences in the `bpe-core`, `bpe-gfx`, `bpe-kit` and `bpe-studio` paragraphs with
sub-layers (section `Sub`, `add_sub_layer`, `MAX_SUBS`, kinds `sub.*`/`shape.sub.*`, codes 20..29,
`sub_layers.wgsl`, `Piece`, the brush distance channel, `hit` on instances, the box UI, format 9 and
`Registry::add_upgrade`). Keep it as terse as the rest of the file.

### Task 9: verify, review, deliver

1. `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (debug, background) and
   `cargo test --workspace --release` (expect only `pushing_an_earlier_step_panics_in_debug` to fail in release).
   `git diff main -- crates/bpe-kit/tests/` empty (no golden moved).
2. One Opus review of the whole diff (project rule). Fix what it finds, with a failing test first for each bug.
3. PR (attribution block, assignee/reviewer as the project's PR rules say). Build Windows exes
   (`-p bpe-studio`, `-p bpe-cli`, rename `ball-studio-next.exe` to `ball-studio.exe`) into project files
   `ballphysics/sublayers/`.
4. Write `ballphysics/sublayers/decisions.md` with any ruling made while building that a later part (trail and
   particle layers) needs.

## Done when

- Right-click a layer ▸ Add sub-layer works for all ten kinds on balls and shapes, shown as boxes (mockup D), with
  ordering, folding, copy/paste/duplicate/delete carrying them.
- The pictures match the spec's descriptions (tests above), old mask scenes open as cuts and look the same.
- All suites green, goldens untouched, review findings fixed.
