# Sub-layers, trail and particle layers, and one Physics card

Date: 2026-11-04. Brainstorm thread "Design sub-layers, trail and particle layers, Physics card". Approved by the
owner (Aedan) at 16:13Z after these answers:

- Sub-layers replace masks; every proposed kind is in ("all", 15:58Z).
- Layout **D**: a layer and its sub-layers are drawn together in one box in the Layers list (16:02Z). Mockups:
  https://claude.ai/artifact/HuhqLfZ8W6vGh3SzafLXmW (board D, "The layer and its sub-layers in one box").
- Trails and particles become Look layers with every proposed new option ("all", 16:08Z).
- Physics is a straight merge of Motion and Bounce; new physics ideas wait for their own brainstorm (16:09Z).

The work is three parts, each its own plan and PR:

| Part | What | Plan | Depends on |
| --- | --- | --- | --- |
| 1 | Sub-layers (balls and shapes) | `docs/superpowers/plans/2026-11-04-sublayers-part1.md` | nothing |
| 3 | Physics card | `docs/superpowers/plans/2026-11-04-physics-card.md` | nothing (builds beside part 1) |
| 2 | Trail and Particles layers | `docs/superpowers/plans/2026-11-04-trail-particle-layers.md` | part 1 merged |

**Merge order:** part 3 and part 1 in either order (they touch different files apart from `inspector.rs`, where part 3 changes only the per-slot card loop), then part 2. Part 3 changes no saved data. Part 1 bumps the scene file format to **9** and part 2 to **10**. Old files keep opening: each bump has an upgrade
step in `bpe-core/src/save.rs` (the kit registers it with `Registry::add_upgrade`, added in part 1), even though the owner does not require
backward compatibility, because the upgrades are cheap and the owner was told old scenes would open.

Words used here: a **layer** is a top-level row of a Look's Layers list (Body, Pattern, Outline, ...). A
**sub-layer** is a layer whose `Layer.parent` is set: it belongs to that layer. A layer and its sub-layers together
are its **piece**.

---

## Part 1: Sub-layers

### What the owner sees

- Right-clicking a drawn layer's row (in a ball's or a shape's Look) shows **Add sub-layer ▸** with the kinds that
  fit that layer. Picking one adds it at the end of that layer's sub-layers and picks it.
- A layer with at least one sub-layer is drawn as **one box** (mockup D): the layer's row is the box's header (grip,
  fold arrow, thumbnail, name, summary, eye), its sub-layers are indented rows inside the box (thumbnail, name,
  summary, eye), and the box ends with a small **+ Sub-layer** row that opens the same kind menu. A layer with no
  sub-layers is a plain row as today. The fold arrow hides the sub-rows (editor state only, never saved); the box
  header still shows how many are folded ("▸ 2").
- Sub-layers work **top to bottom in their box**: the first changes the layer, the next changes that result, and so
  on. "Glow, then Cut half" gives half of a glowing outline; "Cut half, then Glow" glows round the half outline.
  Sub-rows drag to reorder **within their box** only.
- The piece then mixes into the stack exactly where the layer sits, with the layer's own Opacity and Blend, as one
  picture. So a Glow sub-layer on the Outline spreads over the Body under it (the owner's example).
- A sub-layer row's right-click menu: Rename, Duplicate, Copy layer, Paste layer (a copied sub-layer pastes after
  this one; a copied top-level layer is refused here), Delete. A top-level layer's menu keeps Rename, Duplicate,
  Copy layer, Paste layer, Copy whole look, Paste whole look, Delete, and gains Add sub-layer ▸ in place of today's
  "Add mask" and "Mask with layer below". Pasting a copied sub-layer from a top-level layer's menu adds it as that
  layer's last sub-layer.
- Duplicate, Copy layer, Paste layer, Copy/Paste whole look and Delete on a layer carry its sub-layers (as masks do
  today). Moving a layer moves its box.
- Picking a sub-layer shows its settings under the lists, headed "<Sub-layer name> on <layer name>".
- Alt-click on a **Cut** sub-layer's thumbnail shows the layer's cuts alone in the ball view (today's mask solo).
- Clicks on the ball view still pick top-level layers (`topmost_at`); sub-layers are picked from the list.
- One level only: a sub-layer has no sub-layers, and its menu has no Add sub-layer.
- At most **12 sub-layers per layer** (the menu item greys out at 12).
- The Body, Blend, reactions, group layers, and sound layers take no sub-layers. Every other drawn layer does:
  Pattern, Outline, Shine, Glow, Shading and Shader on balls; Colour and gradient, Outline, Glow and Shader on shapes.

### The kinds

All ten are sub-layer-only kinds (section `LayerSection::Sub`, which replaces `LayerSection::Mask`). Ids are new;
settings structs are reused where the wording fits. Balls' kinds are `sub.*` (card `look.layers`); shapes get the
same ten as `shape.sub.*` (card `look.shape`), with sizes in world pixels.

| Kind id | Name | Settings | What it does to the piece |
| --- | --- | --- | --- |
| `sub.cut` | Cut | today's `MaskLayer`: From (Layer under it, Layer, Shape, Brush, Light, Dark), Layer, Shape, Angle, Size, Strokes, Threshold, Invert, Softness, Moves with | Limits where the piece shows, and its shape (for later Glow/Outline/Shadow) |
| `sub.glow` | Glow | `SubGlow`: Strength, Size, Where (Outside the layer, Inside it, Both), Colour from (Same as the layer, Own), Colour, Gradient, Direction, Opacity | Soft light round the piece's current shape |
| `sub.outline` | Outline | `SubOutline`: Width (px), Sits (Inside, Centre, Outside the layer's edge), Colour from (Same as the layer: darker, Own), Colour, Gradient, Direction, Opacity | A band along the piece's current edge, painted over it |
| `sub.shadow` | Shadow | `SubShadow` (new): Angle (°, default 135), Distance (radii, 0.15), Softness (0.3), Colour (black), Opacity (0.6), Moves with (Screen default, Ball) | A soft offset copy of the piece's shape under it |
| `sub.shading` | Shading | today's `ShadingLayer` | Darkens the piece's colour only |
| `sub.shine` | Shine | today's `ShineLayer` | Lights the piece's colour only |
| `sub.shader` | Shader | today's `ShaderLayer` (effect, own code) | An effect whose picture is the piece only |
| `sub.recolour` | Recolour | `SubRecolour` (new): Mode (Tint, Hue, Gradient, Light), Colour + Amount (Tint), Turn ° (Hue), Colours + Gradient + Direction + Amount (Gradient: replaces the piece's colours by their lightness), Brightness, Contrast, Saturation (Light), Opacity | Changes the piece's colours only |
| `sub.flash` | Flash on hit | `SubFlash` (new): Strength (0.8), Length (s, 0.3), Colour (white), Follow impact (on) | The piece lights up when this ball (or shape) is hit |
| `sub.hue` | Colour on hit | `SubHue` (new): Turn per hit (°, 30) | The piece's colours turn round the wheel on every hit |

Every setting with `keyframe` today keeps it; new number settings are `keyframe`, strength-like ones also `pulse`.
Strength, Opacity, Amount and Turn are `simple`.

**On shapes** the menu leaves out: Cut From Brush (no brush view for shapes), Shading Brush (its Kind chips leave
Brush out), and nothing else. Shapes' Flash on hit and Colour on hit use the shape's own hits (balls hitting it).

**Ball-wide reactions stay.** The Reactions list keeps Flash on hit (the hit-spot flash sprite) and Colour on hit
(the whole body). The sub-layer versions are additional, per piece.

### How it draws (GPU)

Drawing stays one pass per ball (and per shape): the existing analytic stack walk in
`bpe-gfx/src/shaders/layers_walk.wgsl` (and `shape_walk.wgsl`) is extended. No offscreen per-ball textures.

**The piece.** When the walk reaches a top-level drawn record, it works out the layer's colour and cover inside and
outside the shape as today (`c`, `a_in`, `a_out`), plus its **shape distance** `sd`: signed, in ball radii (shape
units on shapes), negative inside the layer's shape. Then it applies the sub-layer records that follow the layer's
record, in order, to the piece state:

```
struct Piece { c_in: vec3, a_in: f32, c_out: vec3, a_out: f32, light_in: vec3, light_out: vec3, sd: f32 }
```

and finally mixes the piece into the stack: `inner = layer_over(inner, c_in, a_in, mode)`,
`outer = layer_over(outer, c_out, a_out, mode)`, then adds `light_in`/`light_out` the way a glow's light is added
today (mixed with what is under it by the blend when the mode is not Normal). `COV_IN/COV_OUT[k]` record the
piece's final cover, so a Cut "Layer under it" on a later layer reads the whole piece.

**Shape distance of a layer** (`sd` before sub-layers):

- Body, Pattern, Shine, Shading, Shader (top-level): the ball's shape distance `d` (`ball_shape`). Shapes: the
  shape's distance from `shape_at` (filled: inside negative; band: `abs(d) - half_thickness`).
- Outline: its band, `max(lo - d, d - hi)`.
- Glow: from its cover, `sd = (0.5 - cover) * size` (a glow has no hard edge).

**Each sub-layer record on the piece:**

- Cut: cover `v` (in, out) worked out exactly as `mask_value` does today; `a *= v`, `light *= v.x/v.y`. Its own
  distance: Shape cuts compute it analytically (the `shape_mask` cases are rewritten to return a distance first,
  then `soft_inside`), Brush cuts read a **distance channel** baked beside the brush cover (see Masks atlas below),
  Light/Dark/Layer cuts estimate it from their cover, `(0.5 - v) * 0.1`. Then `sd = max(sd, cut_d)` (`-cut_d` when
  inverted).
- Glow: `h = strength * exp(-(t / size)^2)` where `t = max(sd, 0)` (Outside), `max(-sd, 0)` (Inside, an inner
  glow), or `abs(sd)` (Both); faded at the quad's edge as today's glow. Added to `light_in`/`light_out` in its
  colour (Same as the layer: the piece's colour where it covers, else the layer's own colour) times opacity.
- Outline: band cover `b = band(sd, lo, hi)` (lo/hi from Width and Sits, in radii); painted over the piece:
  `(c, a) = over((c, a), outline_colour, b * opacity)` for both channels.
- Shadow: `s = opacity * soft_inside(sd_at(q - offset), softness)`, where `sd_at` re-evaluates the layer's own shape
  distance and its Shape and Brush cuts that come before the shadow at the shifted point (Light/Dark/Layer cuts are
  skipped there). The shadow goes **under** the piece: `(c, a) = over((shadow_colour, s), (c, a))`.
- Shading: `c = mix(c, shade_colour, factor * amount * opacity)` (cover unchanged; `shading_factor` as today).
- Shine: `c = mix(c, shine_material(..., c, ...), strength * opacity)` (as today's shine, with the piece's colour
  as "under").
- Shader: runs its effect with `sample()` returning the piece (the layer and the sub-layers before this one) at
  the sampled point; `c = mix(c, effect, opacity)`. The in-ball program machinery (`programs.rs`,
  `effects/in_ball.wgsl`) gains a piece-sampling begin, so the generated program's second walk (the one shader
  layers sample) can walk one piece.
- Recolour: `c = recolour(c)` per mode; mixed by opacity.
- Flash on hit: `c += flash_colour * level`, `level = strength * exp(-3 * age / length) * impact`, from the ball's
  hit data (below).
- Colour on hit: `c = hue_turn(c, turn_per_hit * hits)`.

Records: sub-layer records follow their layer's record (as mask records do today); `head.w` counts them. Kind codes
**20..29** in the record's `head.x` (20 cut, 21 glow, 22 outline, 23 shadow, 24 shading, 25 shine, 26 shader, 27
recolour, 28 flash, 29 colour on hit); 10 is taken by the shapes' Colour and gradient. Mask code 7 goes.
`MAX_LAYERS` (frame.rs and layers.wgsl) rises from 64 to 128, and `COV_IN/COV_OUT` with it.

**Hit data per ball.** `LayeredInstance` gains `hit: [f32; 4]`: seconds since this ball's last hit (1e6 when none),
that hit's strength (speed / `FULL_FLASH_SPEED`, at most 1.5), how many hits this ball has had, and 0. `PathInstance`
gains the same for a shape (hits on the shape). They are filled from the event log the draw already scans for
flashes.

**Masks atlas.** Brush tiles (`masks.rs`, `MaskAtlas`) gain a distance channel: when strokes are rasterised to a
128² tile, a second 128² channel holds the distance to the painted edge (two-pass chamfer distance on the CPU,
clamped to ±0.25 radii, stored 0..1). The tile texture becomes two channels (`Rg8Unorm`); Shading Brush keeps
reading the cover channel.

**Shapes.** The shape walk (`shape_walk.wgsl`) applies the same sub-layer code: the sub-layer functions live in one
new WGSL file, `bpe-gfx/src/shaders/sub_layers.wgsl`, pasted into both walks, taking the piece, the point, the
record and the hit data. Shapes' records come from `ShapeStack` in `bpe-kit/src/modules/look/shape/gpu.rs`.

**Blend splits** (`Stack::split`, `ShapeStack::split`) count a piece as its layer: the sub-records ride with the
layer above or below the Blend layer.

### Data and edits (bpe-core)

- `LayerSection::Mask` is renamed `LayerSection::Sub`. `Layer.parent` keeps meaning "the layer this one belongs
  to"; sub-layers sit straight after their layer in `Slot.layers` (today's mask rule).
- `Scene::add_mask` becomes `Scene::add_sub_layer(reg, thing, slot, parent, kind) -> Result<LayerId, EditError>`:
  refuses a parent that is the base, not Drawn, itself a sub-layer, of a kind whose `takes_subs()` is false, a
  sub kind of another card, one the parent's kind refuses (`LayerKind::takes_sub(sub)`, default yes; part 2's
  trail and particle layers use it), or a 13th sub-layer (`EditError::NotAccepted`).
- New `Scene::move_sub_layer(reg, thing, slot, id, to)` moves a sub-layer to place `to` among its siblings.
- `LayerKind::maskable()` is renamed `takes_subs()`. `CopiedLayer.masks` becomes `subs`.
- `remove_layer`, `move_layer`, `duplicate_layer`, `copy_layer`, `paste_layer`, `copy_look`, `paste_look` keep
  today's mask behaviour for sub-layers. `paste_layer` of a copied sub-layer onto a top-level layer adds it as the
  layer's last sub-layer (new `Scene::paste_sub_layer(reg, thing, slot, parent, copied)`).
- A Cut naming another layer keeps naming it by id (today's `target`).
- Format **9** upgrade: `Registry::add_upgrade(below, f)` lets the kit register scene fixes that `save::load` runs
  on files older than `below`; part 1 registers one that renames every `layer.mask` layer to `sub.cut` (params
  unchanged). A sub-layer
  whose parent is gone is dropped on load (today's rule).

### Editor (bpe-studio)

- `look_editor/layer_list.rs`: the box (header row, sub-rows, "+ Sub-layer" row, fold arrow, count), drag of
  sub-rows within the box, the menus above. `Inspector.folded_pieces: HashSet<(ThingId, LayerId)>` holds folds.
- `look_editor/layer_rows.rs`: Cut's From/Layer rows (today's mask rows), each sub kind's settings via the usual
  settings grid, Shader's Effect row, the heading "X on Y".
- The ball view's handles (`handles.rs`) for Cut Shape, Shadow (drag the offset), Shading Linear/Radial sub-layers,
  and the brush (`brush.rs`) for Cut Brush and Shading Brush sub-layers, as for masks and shading today.
- Shapes' Look editor (no ball view) shows the same list and box; their sub-layers are edited by settings only.
- Thumbnails: Cut keeps the mask picture; others get a small painted icon (glow halo, ring, offset disc, half-dark
  disc, shine spot, "fx", colour swatch, flash star, hue wheel).

### Testing

- bpe-core: sub-layer edits (add, refuse, order, move within box, duplicate/copy/paste carrying, delete,
  12 limit, paste a sub-layer onto a layer), the format 9 upgrade (a format 8 file with masks loads with cuts).
- bpe-kit: `Stack::records` writes sub records after their layer with the right codes and counts; shapes too;
  `topmost_at` unchanged by sub-layers; hit data reaches instances.
- bpe-gfx pictures (GPU tests, `layers_tests.rs`, `paths_tests.rs`): Glow on an Outline lights the body inside the
  ring; Cut then Glow differs from Glow then Cut; Shadow falls under its layer and offset by its angle; Recolour
  changes only its layer; Flash on hit lights only its layer after a hit; a Shader sub-layer sees only its layer;
  the same on a shape. Existing mask picture tests are ported to Cut and must pass unchanged in output.
- Goldens (physics) do not move: this part is drawing only.
- Studio kittests: right-click Add sub-layer ▸ Glow on Outline makes a box with one sub-row; "+ Sub-layer" adds
  another; drag reorders within the box; fold hides rows; menus carry/paste as above.

---

## Part 2: Trail and Particles layers

### What the owner sees

- The Look editor gains a third list, **Trails and particles**, under Layers and above Reactions, with
  "+ Add trail or particles" (Trail, Particles). Shapes' Look editor gets the list too, with Particles only.
- The Trail and Particles cards are gone from the inspector.
- Each Trail or Particles layer has **Where: Behind the ball / In front** (default Behind), its own Blend and
  amount (as today's cards), and can carry sub-layers in a box (layout D): **Glow, Outline, Shadow, Recolour, Flash
  on hit, Colour on hit**. Cut, Shading, Shine and Shader are not offered on them.
- Several of each are allowed (a short bright streak plus a long faint line, sparks plus smoke).

### Trail layer (`layer.trail`)

| Setting | Values (default) |
| --- | --- |
| Kind | Streak (default), Ribbon, Ghosts, Dots, Line, Smoke |
| Where | Behind the ball, In front |
| Length by | Time (default), Distance |
| Length | 0.05–2 s (0.5) or 10–4000 px (300) |
| Stays forever | Line only: the whole path since the start stays |
| Longer when fast | 0–2 (0): length × (1 + amount × speed / 1000 px/s) |
| Shows above speed | 0–3000 px/s (0) |
| Width | 0.05–2 ball radii (0.5); Line: Thickness 0.5–20 px (2) |
| Width at the tail | 0–1 of Width (0 = today's Taper 1) |
| Width curve | Even, In, Out, Both (Even) |
| Colour from | Ball, Own (Ball) |
| Colours | Paint following Life, Place, Speed or Time (today's) |
| Brightness | 0–2 (0.5) |
| Fade | Even, In, Out, Both, None (Even) |
| Wobble | Amount 0–100 px (0), Waves along it 0.5–20 (3), Speed 0–10 per s (1) |
| Restart at each bounce | on/off (off) |
| Ribbon: Twist | Follows the ball's turning (default), Spins (turns per s 0–10) |
| Ghosts | How many 1–30 (6), Every 0.02–0.5 s (0.06): copies of the whole ball look, fading back |
| Dots | Every 2–200 px (20), Dot size 0.05–1 radii (0.3) |
| Smoke | Puffs per second 5–120 (40), Spreads 0–4 (1.5), Rises 0–400 px/s (40), Softness 0–1 (0.7) |
| Blend, Blend amount | as today (Add, 1) |

Drawing: Streak and Line use today's `push_streak*` with the width curve and wobble applied on the CPU; Ribbon is a
streak whose width at each point is `|cos(twist angle)|` of Width (with a darker back face); Dots and Smoke push
round sprites through the particle renderer (`Frame.bursts` instances with life-based fade); Ghosts push extra
`LayeredInstance`s of the ball at past positions with fading opacity, using the ball's own records. Restart at each
bounce cuts the point list at this ball's hit times from the event log.

History: `bpe-runtime/src/history.rs` keeps as many seconds as the thing's longest trail asks for
(`World::keep_history(thing, seconds)`; Line with Stays forever keeps every sample since the start, at 30 samples a
second). The world fingerprint hashes whether a history is kept and how long, never how it looks.

### Particles layer (`layer.particles`)

| Setting | Values (default) |
| --- | --- |
| When | On hit (default), While moving, Only when fast, When it pops, Steady |
| Count per hit / Rate | 1–200 per hit (12); 1–500 per s (60) for While moving, Only when fast, Steady |
| Least speed | 0–2000 px/s (0): hits (or, Only when fast, ball speeds) under this let off nothing |
| Follow impact | on: harder hits let off more and faster particles |
| Made of | Sparks, Smoke, Confetti, Stars, Shards, Rings (today's), Dots, Hearts, Bubbles, Streaks, Mini balls |
| Direction | Off the bounce (default), All round, Along the path, Behind the ball, Up |
| Spread | 0–360° (140) |
| Speed | 0–2000 px/s (380) |
| Takes the ball's speed | 0–1 (0) |
| Size | 0.5–200 px (2.5) |
| Size over life | Even, Grow, Shrink (default), Pop (grows then bursts) |
| Spin | 0–1440 °/s (0), Spin spread 0–1 (0.5) |
| Life | 0.05–3 s (0.6) |
| Fade | In, Out (default), Both, None |
| Colour from / Colours | today's (Ball, Own; follows Life, Order, Place, Speed, Time) |
| Twinkle | Amount 0–1 (0), Speed 0–30 per s (8) |
| Gravity | -2000–2000 px/s² (300) |
| Drag | 0–10 (2) |
| Swirl | Strength 0–2000 px/s² (0), Size 20–1000 px (200) |
| Pull back to the ball | 0–50 per s (0) |
| Bounce off shapes | on/off (off), Bounciness 0–1 (0.5) |
| Where | Behind the ball, In front |
| Blend, Blend amount | today's |

Shapes: When is On hit (the shape is hit) or Steady (from random points along its edge); Direction "Off the bounce"
means away from the shape's edge; Mini balls is not offered.

Simulation: particles move to a **deterministic CPU simulation** in the draw (`bpe-kit/src/modules/look/layers/
particles/sim.rs`): every emission in the last `life` seconds is re-simulated from its birth to the frame's time in
fixed 1/120 s steps, seeded by thing, ball, emission time and index, so scrubbing and export give the same picture.
Swirl is curl noise; Pull back reads the ball's past positions from its history; Bounce off shapes reflects off the
world's walls (a query added to `bpe-runtime/src/walls.rs`). At most 4000 live particles per layer (oldest dropped).
The GPU draws the resulting instances (`Frame.bursts` becomes a list of particle instances; `particles.wgsl` gains
the new shapes). Mini balls push small `LayeredInstance`s with the ball's records.

Where: Behind draws before the balls (today's place); In front draws in a new list after the balls in the same
place (`Frame.trails_front`, `Frame.bursts_front`).

### Data

- `SlotId::Trail` and `SlotId::Particles` go. New layer section `LayerSection::Around` for the list.
- Format **10** upgrade: a thing's Trail card becomes a Streak `layer.trail` at the end of its Look's Around list
  with the same values (`taper` → Width at the tail = 1 - taper, Width curve Even); a Particles card becomes an On
  hit `layer.particles` (`preset` → Made of; other values carried). Keyframes, pulses and Follow sound on those
  settings move to the layered keys.

### Testing

- Kit draw tests per kind (point counts, widths, colours along, restart at bounce, ghosts count), particle sim
  determinism (same frame twice and after a seek gives the same instances), swirl/pull/bounce sanity, the 4000 cap,
  the upgrade, Behind/In front order in the frame, sub-layers on a trail (Recolour changes vertex colours, Glow adds
  a soft halo streak).
- History: length follows the longest trail; Stays forever keeps the start.
- Goldens do not move (trails and particles never touch physics; the history change must not change stepping).

---

## Part 3: Physics card

The owner sees one **Physics** card where Motion and Bounce were. Underneath, the scene keeps its two cards
(`SlotId::Motion`, `motion.basic` and `SlotId::Bounce`, `bounce.basic`): the merge is in the inspector only. That
keeps the physics, the save format, keyframe addresses, Follow sound and the runtime untouched, so goldens cannot
move, and it leaves room for later physics cards to show inside Physics too.

- The inspector draws one card titled **Physics** for a thing that has a Motion card, a Bounce card or both, in
  the place Motion's card had. Inside it, folds in this order: **Motion** (Gravity ×, Air drag, Wind: Motion's
  `Basics` group, renamed `Motion`), **Bounce** (Bounce's `Basics` group renamed `Bounce`: Bounciness, Speed gain,
  Top speed, Friction, Takes over), **Growing**, **Other balls**, **Against some things**. Each fold is the
  settings grid of the card that owns those settings, so edits, the value menu, keyframes, pulses and Follow sound
  work exactly as today.
- The header's fold arrow folds the whole card; the folded summary counts both cards' settings. **Remove** takes
  both cards off. The "+ Add" menu lists **Physics** once (adding whichever of Motion/Bounce the thing lacks and
  fits). The jump bar shows one **Physics** chip. Problems on either card show under the header.
- Shapes have only a Bounce card, so their Physics card shows only Bounce's folds (with today's `when`s).
- The timeline groups lanes by thing, not card, so it needs no change; its "physics setting" test
  (`lanes.rs`) stays.
- Testing: kittests that a ball shows one Physics card holding both grids (Gravity × and Bounciness rows under
  it, no separate Motion/Bounce headers), that Remove takes both, that "+ Add" Physics brings both back, that a
  shape's Physics shows Bounce settings only, and that the jump bar has one Physics chip. Goldens untouched.
- No format bump.

---

## Not doing

- Sub-layers inside sub-layers; sub-layers on the Body, Blend, reactions, group or sound layers.
- Offscreen per-ball rendering for sub-layers; soft layers' shape distance stays an estimate.
- Brush painting on shapes.
- New physics options (weight, spin, magnets): a later brainstorm.
- Trails on shapes.
- Any built-in or example scene.
