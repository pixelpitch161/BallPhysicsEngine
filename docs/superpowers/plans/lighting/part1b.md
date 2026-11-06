# Lighting part 1b: Gives off light, the light field, haze with beams, Light blooms

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved). Builds on part 1a (plan `part1a.md`, PR #106):
`SlotId::Lighting`, `things/light/`, `layers/lighting.rs`, `bpe-gfx/src/lighting.rs` + `shaders/lighting.wgsl`,
`Frame.light`, `ThingDraw::light`, the light passes (`Globals.light.x`: 0 picture, 1 lit colour, 2 surface). Built
straight after this plan, one Opus review at the end, then the PR, merged by this thread once tests and the review
pass (owner's overnight instruction, 2026-11-06). Parts 1c, 2a, 6 and 8 run at the same time and rebase on this one.

## What 1b delivers

- **Gives off light**, a sub-layer on every drawn layer of balls (`sub.light`) and shapes (`shape.sub.light`), the
  Bodies included, and on trails, particles and the wavefront (both kinds added to `AROUND_SUBS`). The light comes
  from where the layer actually covers, after its cuts. Settings: Colour from (the layer's colours / own colour),
  Colour, Strength, Negative (gives darkness: flips the strength, as part 8's Negative on lights), Flare on hit and
  Flare length (ball and shape layers).
- **The background gives off light**: a "Gives off light" fold on the scene settings (Strength, Colour from the
  background's colours / own colour, Colour).
- **The light field** (radiance cascades, 2D): every glowing thing lights the rest with soft shadows from what blocks
  light and colour spill; **Bounce light** (lit things that block light bounce it onto others, one bounce, and a
  second pass so glow light bounces too); **Glow reach**.
- **Haze with beams**: the Lighting card's Air fold (Haze, Haze colour, Haze dims) and each light's Air fold (Beam in
  haze). Lamps' light shows in the air with their shadows (shafts through gaps, spot cones); glowing things' light
  shows in the haze as halos.
- **Light blooms** (Room fold): lamps show as glowing bulbs and glowing things spill a soft bloom of their light.
- All of it costs nothing without a Lighting card (the no-card test still pins the picture), and nothing beyond 1a's
  passes when none of it is used.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **Emission is folded into its layer's record**, not a record of its own. `LayerGpu` gains `emit: [[f32; 4]; 2]`:
   `emit[0]` = own colour × strength (rgb, linear) and the flare amount (w); `emit[1]` = the tint the layer's own
   colours are multiplied by (rgb: strength, or 0) and the flare length (w). Several Gives off light sub-layers on one
   layer add up. `SubLayer::Light` writes no record (`writes()` false), so record numbers, cuts and `COV_IN` are as
   before. The emission of a piece is `emit[0].rgb × cover + emit[1].rgb × colour` (premultiplied), times the flare.
2. **A new light pass, mode 3 (emission)**: the frame is drawn a fourth time, only when something gives off light
   (`LightScene.glows`), with `Globals.light.x = 3`. Every layer writes its emission (0 for layers that give none)
   with its cover, so things in front hide the glow behind them. Blend layers composite as Normal in modes 2 and 3
   (1a: Lighten in mode 2 stays). Light that sub-layers add (a Glow sub-layer's halo) gives none; a Glow layer's own
   light gives its emission by its light (`max` channel) when it has the sub-layer.
3. **A layer that gives off light shows its own colours, unlit** (it is a lamp): its Lit counts as 0 in the lit
   colour pass. A Gives off light with Strength 0 still marks it (the switch is having the sub-layer shown).
4. **Body sub-layers**: the ball Body and the shape Body take sub-layers, but only Gives off light
   (`takes_subs` true, `takes_sub` only the light kind). Core's `check_sub` no longer refuses base layers by being
   base (it asks `takes_subs`); the editor's sub-layer box and menu show for the Body when it takes subs.
5. **Around layers** (trails, particles, wavefront, and what their sub-layers draw): the emission is worked out on
   the CPU per vertex/particle (`Drew::light` stamps it): `TrailVertex.emit` and `Particle.emit` (`[f32; 4]`, rgb
   straight colour, w 0) = own + tint × the vertex's straight colour. Mini balls and ghosts get it on their records.
   No flare on around layers (they have no hits of their own).
6. **The background's emission is not in the light field** (as a surface everywhere it would stop every glow ray);
   it is light added where it is: drawn once into a `bg_glow` picture (the background pass alone, mode 3) and added
   to the light at each pixel, like an ambient light that follows the background's colours. The background then
   shows unlit (it is the lamp).
7. **The light field**: radiance cascades over the field picture (1a's tier size). Scene picture (`gi_scene`,
   field size, Rgba16Float): rgb = emission + bounce source, a = opaque (1 where something blocks light or gives
   light). A second jump flood (over the same seed textures) gives the distance to the nearest opaque texel
   (`gi_dist`, R16Float, texels). Cascade i: probe spacing 2^i texels, 4^(i+1) directions (a 2^(i+1) square),
   interval from `r0·(4^i − 1)/3` to `r0·(4^(i+1) − 1)/3` (r0 = 1 texel); stored direction-first in a
   `2·W × 2·H` picture (W, H the field size rounded up to a multiple of 2^(N−1)); two pictures ping-pong, top cascade
   first, each merging with the four nearest probes of the one above (manual bilinear, 16 loads) where its ray is
   unblocked. Rays march by the distance field (at least one texel a step), stop at the first opaque texel (its
   emission × the Glow reach window, `(1 − (t/R)²)²` with t the distance from the probe in world units), or off the
   picture. N = the fewest cascades whose last interval ends past the picture's diagonal (or the Glow reach), at most
   9. Steps a ray: Draft 8, Good 12, High 16, Ultra 24. The resolve averages cascade 0's four directions:
   `field = 2 × mean radiance` (gain 2, so a white glowing ball lights a neighbour a ball's width away about as a
   lamp of Strength 1 does).
8. **Bounce light**: source = Bounce × lit colour × direct light × blocks (only things that block light bounce; the
   background never does). With Bounce > 0 and glowing things, the field runs twice: the second time its source adds
   Bounce × lit colour × the first field (glow light bounces too). Without glowing things it runs once.
9. **Passes, with a Lighting card**: (1a) modes 1, 2 → the blocker field → (1b) `fs_direct` (full size, two
   targets: the light from Light things on surfaces, and the light in the air) → when glows or bounce: `fs_gi_scene`,
   seed/jump/`fs_gi_dist`, cascades, `fs_gi_resolve` (once or twice) → when Light blooms: `fs_glow_source` (lamp
   bulbs + emission) and three tent downsamples → `fs_mix` (full size) writes the HDR picture. 1a's `fs_light` is
   split into `fs_direct` + `fs_mix`; with no 1b setting in use the picture is 1a's (the direct light passes through
   a half-float picture, a rounding difference only; no test pins 1a's lit hashes).
10. **The mix**: `light = max(ambient + direct + field + bg_glow, 0) × contact` (clamped per channel at 0, as part
    8 does for negative lamps); `out = c + (light − 1) × w` (1a's), then
    `out = out × (1 − veil) + haze_colour × haze × (air + field)` with `veil = clamp(dims, 0, 1) × min(haze, 1)`,
    then `+ blooms × bloom picture`; clamped to [0, 6e4] (negative light can't push a pixel below black).
11. **Haze** uses each light's own shadow march (the same visibility as the surface at that pixel), its falloff and
    cone, times its **Beam in haze** (`LightGpu.p7.x`, default 1); no normal shading. Ambient doesn't light the haze.
12. **Light blooms**: the bloom source (field size) is every lamp's bulb `colour × strength / (1 + (d/size)²)²`
    (d to its source point; suns none) plus the emission picture; three levels made by tent downsampling; the mix adds
    `amount × (0.35 l0 + 0.3 l1 + 0.2 l2 + 0.15 l3)`. Lamps stay invisible at Light blooms 0 (1a's look).
13. **Settings and spans** (no limits: spans, hard limits only where maths breaks):
    - Gives off light (`GivesLight`, keys `colour_from`, `colour`, `strength`, `negative`, `flare`, `flare_length`):
      Negative a switch, default off; Strength
      span(0, 8) default 2 (negative allowed: darkness); Flare span(0, 8) default 0; Flare length `range(0.001, …)`
      (a zero length divides by zero) span(0.02, 2) default 0.4 s; Colour from Layer's colours / Own colour; Colour
      default warm white `#FFE6C0`. All numbers and the colour keyframe.
    - Lighting card: Room adds **Bounce light** span(0, 2) default 0, **Glow reach** `range(0, …)` span(0, 4000)
      default 2000 px (0 = no glow reaches anything; a negative reach means nothing), **Light blooms** span(0, 2)
      default 0; new **Air** fold: **Haze** span(0, 2) default 0, **Haze colour** default white, **Haze dims**
      span(0, 1) default 0.
    - Light thing: new **Air** fold, **Beam in haze** span(0, 4) default 1.
    - Scene settings: new fold **Gives off light**: Strength `bg_glow` span(0, 4) default 0, Colour from
      (Background's colours / Own colour) `bg_glow_from`, Colour `bg_glow_colour`.
14. **Deferred** (stated so nothing is silently dropped): per-emitter Reach and Falloff (one field carries every
    glow; the scene's Glow reach instead), per-emitter Casts shadows (needs a second, unshadowed field: part 4),
    Lights up (linking: 1c's list), Flicker (part 4 with gobos), Fade along for trails (a trail's own fade already
    fades its glow), Shows in reflections (2b). Swirling haze, smoke and haze that moves are part 4.
15. **Scene format** stays 11 (new keys only, defaults fill in).
16. **Memory**: nothing kept between frames. The cascade pictures are remade only when the size or tier changes.

## Files and types

### bpe-core
- `layers.rs`: `check_sub` refuses a base layer only when it doesn't take subs. Test: a Body takes a Gives off light
  and keeps it right after itself when drawn layers move.

### bpe-gfx
- `frame.rs`: `LayerGpu.emit: [[f32; 4]; 2]` (size 240), `TrailVertex.emit`, `Particle.emit` (`[f32; 4]`);
  `LightGpu.p7` (beam in haze, 3 spare); `LightingGpu.glow` (bounce, glow reach, light blooms, 0), `LightingGpu.air`
  (haze colour × haze rgb, dims), `LightingGpu.background` (`[[f32; 4]; 2]`: own, tint); `LightScene.glows: bool`.
- `lib.rs`: `Globals.light_emit`, `Globals.light_tint` (background emission, read in mode 3); the trail and sprite
  vertex layouts gain `emit`; `render` draws mode 3 into the emission picture when `glows`, the background alone in
  mode 3 into `bg_glow` when the background gives light; `draw_layer` composites as Normal in mode 3; passes the
  pictures to `Lighting::run`.
- `lighting.rs`: new targets (`emission`, `bg_glow`, `direct`, `air`, `gi_scene`, `gi_dist`, `cascades[2]`,
  `fields[2]`, `glow[4]`), a second bind group layout (group 1), pipelines `direct` (two targets), `gi_scene`,
  `seed_gi`, `gi_dist`, `cascade`, `resolve`, `glow_source`, `glow_down`, `mix`; `cascade_count`, `gi_steps`.
- `shaders/lighting.wgsl`: `fs_light` split into `fs_direct` + `fs_mix`; the passes above.
- WGSL: `light_piece` mode 3 (layers.wgsl), `light_flat(c, lit, blocks, emit)` mode 3 (scene.wgsl, sprites.wgsl),
  trail.wgsl and sprites.wgsl pass `emit`, the background's mode 3, shape walk; flare from the instance's hit.

### bpe-kit
- `modules/look/layers/gives_light.rs` (new): `GivesLight` settings, `EmitFrom`, `Emit { own, tint, flare,
  length }` (`of`, `add`, `is_on`, `around`), the kinds' constants `SUB_LIGHT = "sub.light"`.
- `layers/mod.rs`: `sub_kinds` adds Gives off light; Body kind takes subs (`takes_sub`: only `sub.light`);
  `AROUND_SUBS` gains both light kinds.
- `layers/gpu.rs`: `SubLayer::Light(GivesLight)` (writes no record; not `reads_hits` but `needs_hits` when a flare is
  set); `DrawnLayer.emit`, `Stack.body_emit`; records carry `emit`; Lit 0 on emitting layers.
- `shape/mod.rs`, `shape/gpu.rs`: `shape.sub.light`, ShapeBody takes it, `ShapeStack` emission per layer.
- `layers/around.rs`: `AroundLayer.emit`; `Drew::light` stamps `emit` (and Lit 0 when it gives light).
- `particles/draw.rs`, `balls/draw.rs` (ghosts, mini balls): records/particles take the layer's emission.
- `modules/lighting.rs`: the new settings and `gpu()`.
- `things/light/`: `beam` setting, `p7`.
- `things/scene/`: background Gives off light settings; `draw.rs` fills `LightingGpu.background`, sets `glows`.
- Balls, emitter and shape draws set `LightScene.glows` when a shown layer gives off light (`ThingDraw::light`).
- `scenes.rs`: `glow_room` test scene (glowing balls, a shape, haze, bounce, a spot) for tests and the demo.

### bpe-studio
- `look_editor/layer_list.rs`: the Body's sub-layer box and menu when its kind takes subs.

## Tests (what passing looks like)

- `bpe-kit/tests/lit_pictures.rs` gains: a glowing red ball lights a white neighbour red on the side facing it, and
  leaves it black without the sub-layer (ambient 0); a blocking ball between them darkens the neighbour; a glowing
  ball in the dark still shows its colour; negative strength darkens a lamp-lit ball; Bounce light tints a shadowed
  white ball by a lit red one (Bounce 0 doesn't); haze shows a spot's cone in empty space (brighter inside than
  outside) and nothing at Haze 0, and a blocker in the cone leaves a darker shaft; Light blooms brightens the air at
  a lamp; the background giving off blue light lights a white ball blue; a glowing trail lights a ball near it; a
  shape's glowing layer lights a ball inside it.
- No-card identical: `unlit_pictures.rs` unchanged and passing.
- Kit units: Gives off light defaults and keys; emission folds into records (no extra record, cut targets unchanged);
  around emission stamps; `glows` set only with a shown sub-layer; Lighting card new keys.
- Core: Body takes only Gives off light; it stays after the Body when drawn layers move.
- Physics and sound goldens unchanged; `cargo clippy --workspace`, `cargo fmt`, the editor's kittest tests.

## Things not to do

- No new pass runs without a Lighting card; no mode 3 draw without a glowing layer; no field without glows or
  bounce; no bloom passes at Light blooms 0.
- Nothing physical reads any of it. No state kept between frames. No hard ranges for taste. No colours outside
  `style/`. No built-in scenes in the program.
