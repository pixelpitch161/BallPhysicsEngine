# Ball Look editor: design

Date: 2026-11-03. Approved by Aedan 18:09Z.

- Mockups: Ball Look editor canvas, https://claude.ai/artifact/C4Nyud3QGE5S15VbR3DwDb (boards "D Ball with a layer
  stack (chosen)", "Layer catalogue", "How you change things", "Masks", "Group view", "Group options").
- Plain-language design doc: "Ball Look editor design", https://claude.ai/artifact/2KPqFgQwiPKrqsVutQZEoV.
- Part 1 plan: `docs/superpowers/plans/2026-11-03-ball-look-part1.md`.

## Goal

The Balls thing's Look card stops being a long list of sliders and becomes a visual designer: a big view of one ball
on top of a stack of layers, plus a Group view for what spreads across many balls. A new ball always starts as a plain
white circle. What the ball view shows is exactly what the export shows: it is drawn by the same renderer.

Success: every look in the catalogue below can be built by adding layers, clicking choices and dragging on the big
ball; a new Balls thing is one white circle with no glow, shine, outline or flash; physics never changes because of a
look.

## Scope and parts

| Part | Contents | Depends on |
| --- | --- | --- |
| 1 Layers core | Layer data model; `look.layers` card for balls; Look editor (ball view with Still / Moving / Hit and Dark / Scene, layer list, add / reorder / eye / rename / duplicate / copy / paste / copy whole look / paste whole look / delete); per-layer blend; GPU drawing of any stack; layers Body (with Size), Pattern, Outline, Shine, Glow, Blend; reactions Flash on hit, Colour on hit, Stretch when fast, Colour over time, Turning; white-circle default; file format 4 | nothing |
| 2 Masks, shading, shaders | Masks (layer below, another layer, shape, brush, light/dark); Shading layer (linear, radial, brush); Shader layer (built-in effects and own code); on-ball handles and click-to-select; brush painting | part 1 |
| 3 Group view | Ball / Group switch; group switch, shapes (Grid, Line, Ring, Filled circle, Triangle, Honeycomb, Spiral, Heart, Scatter, Word, Custom), how many, space between, turn; group gradient with Edit in view (angle, stretch, easing), Spread by, With ball colours + Amount, Colours it changes; group shader; Variety, Release, Launch, Odd ones out | part 1; merges after part 2 when both are ready |
| 4 Extras | Face, Label, Shadow layers; more body shapes, patterns, outline styles, Shine materials; Glow inner / hard edge; Squash on hit, Size on hit, Fade reactions | parts 1 and 2 |

Parts 2 and 3 get their own plans after part 1 merges, written against the code part 1 leaves. This spec fixes their
behaviour; their plans fix their code.

## The Look editor (all parts)

Layout D. The inspector keeps its width; nothing else in the editor moves.

- The Balls thing's Look card shows the Look editor instead of a settings grid. Other things (the arena) keep their
  current Look card.
- **Ball view**: a square well at the top of the card, as wide as the card (capped at 300 px tall), dark ground
  (`#151515`, a `// scene colour` line in the editor), drawn by `bpe_gfx::Renderer` into its own texture. It shows
  one ball of this thing, centred, its radius 35 % of the view's size. Two button groups on the view: **Still /
  Moving / Hit** and **Dark / Scene** (Scene draws the scene's own background behind the ball).
  - Still: the ball at rest at the playhead time.
  - Moving: as if moving right at 700 px/s, so Stretch when fast and Turning show. The view's clock loops 0 to 2 s.
  - Hit: as Still, with a hit at the start of every 1.2 s loop, so Flash on hit and Colour on hit show.
  - While Moving or Hit is on and the card is open, the view redraws at up to 30 frames a second; Still redraws
    only when the look, the playhead or the view size changes.
- Part 3 adds a **Ball / Group** switch to the inspector header beside the kind name. Group shows the whole group in
  the view and the group settings below it.
- Under the view (Ball mode): the **Layers** list with **+ Add layer**, then a **Reactions** list with **+ Add
  reaction**, then the selected entry's settings.

## Layers (all parts)

A ball is a stack of drawn layers, painted bottom to top, plus a set of reactions. There is no limit on the number of
layers.

- Every layer has an id (unique in its card), a kind, a name (default: the kind's name, with " 2", " 3" for repeats),
  an on/off eye, and its own settings.
- **Body** is always the bottom layer, exactly one, can't be removed, moved, duplicated or hidden.
- Other drawn kinds may be added any number of times. **Blend** and each reaction kind may be added at most once (the
  add menu greys out ones already there).
- Every drawn layer except Body and Blend has **Opacity** (0 to 1) and **Blend** (all 12 `bpe_core::Blend` modes)
  for how it mixes with the layers under it *in the same ball*. The scene behind the ball is not part of that mix.
- The **Blend layer** sets how the whole finished ball mixes with the scene (Blend, Blend amount, Blend with each
  other: today's look settings). Without it the ball paints over the scene normally.
- Reactions change the whole ball when something happens. They never change physics.
- Colour reactions (Colour on hit, Colour over time) turn the **Body colour** (and pattern colours that are a shade of
  the body). Layers with their own colours keep them. This matches today's Hue shift per hit and Rainbow.

### Catalogue

Part 1 kinds are marked; the rest arrive in the part named.

| Kind | Settings | Part |
| --- | --- | --- |
| Body | Size (the Balls thing's own Size setting, shown here), Shape (circle, square, triangle, diamond, hexagon, star, heart, donut), Roundness (not circle), Colours (today's full Paint: gradient plus what it follows, so colours can still spread across the group) | 1; more shapes, star points / depth, hole size in 4 |
| Pattern | Today's "Inside each one" paint: pattern, colours from a shade of the body or its own gradient, size, angle, spin, scroll, turns with the ball; plus Opacity, Blend | 1; zigzag, waves, hearts, stars, marble in 4 |
| Outline | Width (px), Sits (inside / centre / outside the edge), Colour (Same as body, or its own), Opacity, Blend | 1; dashed / dotted / double, dash count / spin, own gradient in 4 |
| Shine | Strength (0 to 1), Spot (x, y on the ball, -1 to 1), Size; Opacity, Blend. At Strength 1 and the default spot it looks like today's Shiny style | 1; sharpness, edge shadow, materials glossy / matte / metal / glass in 4 |
| Glow | Strength (0 to 2), Size (radii, 0.05 to 4), Colour (Same as body, or its own); Opacity, Blend | 1; soft / hard edge, inner glow in 4 |
| Blend | Blend, Blend amount, Blend with each other (whole ball against the scene) | 1 |
| Shading | Kind (Linear: a line with dark end and length; Radial: centre and size; Brush: strokes), Colour (black), Amount, Stays with the ball or the screen; Opacity, Blend | 2 |
| Shader | Effect (any built-in effect, or own code), its settings, Speed, Scale, Stays with the ball or the screen; Opacity, Blend | 2 |
| Face | Eyes, mouth, expression, eyes look where it goes, blinks, surprised on hit | 4 |
| Label | Word, its number in the group, or its hit count; font, size, colour; upright or turning | 4 |
| Shadow | Offset, softness, darkness | 4 |
| Flash on hit (reaction) | Brightness, Size, Time (today's Flash, Flash size, Flash time) | 1; colour, ring in 4 |
| Colour on hit (reaction) | Turn per hit (today's Hue shift per hit) | 1; take what it hit, step through group colours, fade back in 4 |
| Stretch when fast (reaction) | Amount (today's Stretch) | 1; start speed, limit in 4 |
| Colour over time (reaction) | Rainbow, turns per second | 1; own colour cycle, pulse in 4 |
| Turning (reaction) | Turn, Spin, Face motion (today's) | 1; roll from speed, spin kick on hit in 4 |
| Squash on hit, Size on hit, Fade (reactions) | as in the design doc | 4 |

## Masks (part 2)

Any drawn layer (not Body, not Blend) and, in part 3, the group gradient and group shader can carry masks.

| From | Shows where |
| --- | --- |
| Layer below | the layer directly under it covers (clipping) |
| Another layer | a named layer of the same ball covers |
| Shape | half, wedge, quarters, ring, circle, band; angle, size; edges dragged on the ball |
| Brush | painted strokes (erase removes); size, softness, strength |
| Light or dark parts | the layers under it are lighter (or darker) than a threshold |

Each mask: Invert, Softness, Moves with (the layer, the ball, the screen). Several masks multiply (all must allow it).
A masked layer's row shows a mask thumbnail; Alt-click shows the mask alone in the view; right-click "Mask with layer
below".

## How things are changed (parts 1 and 2)

- Part 1: layer rows (drag to reorder above Body; eye; double-click to rename; right-click Duplicate, Copy layer,
  Paste layer, Copy whole look, Paste whole look, Move up, Move down, Delete). Settings rows are today's settings grid
  (slider + value, right-click Copy value / Paste value / Create keyframe). Keyframes on any layer setting show on the
  timeline as "<layer name> · <setting>".
- Part 2: the selected layer's handles in the ball view (outline width, glow size, shine spot, pattern angle, shading
  line, mask edges, turn); clicking the ball selects the top drawn layer covering that point; brush tools paint on the
  big ball; strokes are stored on the layer, saved and undoable.
- Undo covers every change, as today (the editor history snapshots the scene).

## Group view (part 3)

As in the design doc and the Group boards. Group placement settings (on/off, shape, count, spacing, turn, custom
positions, release, launch) stay on the Balls thing's own settings, because they decide where balls start; the Group
view only shows them. Today's Arrangement choice is replaced by the Group switch plus shapes, and today's Block and
Ring keep their exact positions so their goldens don't move. The group gradient and group shaders form a small stack
of their own (drag to order, each with its own blend, amount and masks). Copy whole look never copies group settings.

## Architecture

### Data (bpe-core)

- `LayerId(u32)`: a layer's id within one card. New ids are one more than the largest in the card.
- `Slot.layers: Vec<Layer>`, saved, skipped when empty. `Layer { id: LayerId, kind: KindId, name: String, enabled:
  bool }`. Order in the Vec is draw order (index 0 = Body = bottom); reactions sit in the same Vec, after the drawn
  layers, and their order means nothing.
- A layer's settings live in its card's `params` under **layered keys** `"<id>/<key>"`, for example `"3/width"`.
  Because the key is still one string, `Address`, keyframes, pulses, Copy / Paste value, animation and undo work
  unchanged.
- New trait `LayerKind` in `registry.rs` (id, name, help, settings, section Drawn / Reaction, `repeats`, `card`: the
  module kind it belongs to). `Registry::add_layer`, `layer(id)`, `layers_for(card)`.
- `Scene::def_at` (and so `value_at`, `set_at`, keyframes) understands layered keys: it splits off the id, finds the
  layer in the card, and looks the key up in that layer kind's settings.
- `resolve` puts every enabled layer's defaults, then its own values, into the resolved card's flat `params` under
  layered keys, and lists the layers (`ResolvedSlot.layers: Vec<ResolvedLayer>`, enabled ones only, in order).
  `ResolvedSlot::layer_params(id)` gives one layer's settings with plain keys. A layer of an unknown kind is reported
  (`ProblemKind::UnknownLayerKind`) and skipped.
- `load` cleans layered params like card params and drops values of layers that aren't in the card.
- Edits: `add_layer`, `remove_layer` (also removes its params, tracks and pulses), `move_layer`, `duplicate_layer`,
  `rename_layer`, `set_layer_enabled`, `paste_layer`, `paste_look`.
- `Scene::FORMAT` goes from 3 to 4. Older files are refused with the existing "made with an older Ball Studio"
  message, as the owner allows no backward compatibility yet.

### Kit (bpe-kit)

- New card kind `look.layers` (name "Look", slot Look, fits balls only, no settings of its own). `look.basic` stops
  fitting balls and keeps serving the arena unchanged.
- New balls get `look.layers` with one Body layer at its defaults: white, circle. No other layer, no reaction.
- One file per layer kind under `modules/look/layers/`, each a `#[derive(Settings)]` struct plus its `LayerKind`, and
  a `gpu.rs` that turns resolved layers into GPU records.
- `things/balls/draw.rs` draws every ball through the layered path. `ball_colors` (trails, particles, hit marks)
  reads the Body colour with the colour reactions applied.

### Renderer (bpe-gfx)

- One uber-shader `layers.wgsl` draws a ball's whole stack in one fragment pass: a storage buffer of layer records
  (`LayerGpu`), and per ball an instance (`LayeredInstance`) with its record range, shape, stretch, angle, body colour
  and how far it reaches. The fragment walks the records bottom to top, works out each layer's colour and coverage,
  and composites it over the layers under it with that layer's blend (W3C separable / non-separable formulas, the
  same `blended()` maths as `blend.wgsl`). Output is premultiplied, so the ball then goes through the existing Normal
  draw or the thing's blend layer exactly like any other instance.
- Records are per frame (`Frame::look_layers`, uploaded with the frame's other lists, bound as group 2 of the layered
  pipelines). Object-picture and blend-with-each-other variants exist like the other instance kinds.
- Part 2's own-code shader layers are the one case that needs generated WGSL: a stack containing one gets its own
  pipeline built from the uber-shader plus that code, cached by a hash of the code. Part 1 has no generated code.

### Editor (bpe-studio)

- `look_editor/` folder: `mod.rs` (the card body), `ball_view.rs` (second `Renderer`, texture, preview world),
  `layer_list.rs` (rows, menus, drag), `clipboard.rs` (copied layer / look). `inspector.rs` calls it for a
  `look.layers` card instead of `settings_grid`.
- The selected layer per thing lives in `Inspector`. Settings of the selected layer are drawn with the existing
  `settings_grid`, its keys mapped to layered keys.
- Layer structure edits act on the primary thing; setting edits reach every selected thing whose card has a layer with
  the same id and kind (today's rule for card settings).

## Testing

- Core: layered keys through `def_at` / `value_at` / `set_at` / `set_key`; resolve fills layer defaults; animation
  writes a layered key; save / load round trip with layers; unknown layer kinds reported; removing a layer removes its
  keys, tracks and pulses; format 3 files refused.
- Kit: a new Balls thing is `look.layers` with exactly one Body layer and no reaction; drawing it gives one white
  circle instance and no flash; each layer kind's record; reactions (stretch, colour on hit, rainbow, turning, flash);
  the Blend layer routes the ball into a blend layer; disabled layers draw nothing.
- Renderer (GPU picture tests under lavapipe): white circle; outline inside / centre / outside; pattern; shine is
  brightest at its spot; glow outside the edge; per-layer Multiply / Screen; same frame twice is identical.
- Editor (kittest): the Look editor shows Body only for a new ball; add, select, edit, rename, hide, reorder, delete,
  duplicate, copy / paste layer and whole look; Create keyframe on a layer setting gives a timeline lane named
  "Outline · Width"; the ball view buttons switch modes.
- Goldens: physics doesn't change, so `golden*.txt` stay byte-identical (checked in debug and release).

## Things not to do

- Don't move any physics setting into the Look card. Body's Size is the Balls thing's own `radius` setting, shown in
  the Body layer.
- Don't keep the old `look.basic` path for balls, or the classic `BallInstance` path for balls. One path draws balls.
- Don't add presets, a Library or randomize (removed on purpose on 2026-11-03).
- Don't add built-in or example scenes.
- Don't change the arena's Look card.
