# Lighting part 8: the Effects stack and the psychedelic effects

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), sections "No limits", "The editor" (Effects
stack) and "Psychedelic and experimental effects". Part 1a (PR #106) is on main. Parts 1b, 1c, 2a and 6 are built at
the same time; this part merges after them (part-number order) and rebases onto whatever has landed. Built straight
after the plan (no approval stops), one Opus review at the end, then the PR, merged by this thread once tests and
the review pass (owner's standing instruction, 2026-11-06 06:41Z).

## What part 8 delivers

1. **The Effects stack.** The scene's Shader card becomes an **Effects** card (`effects`, slot `Shader`, scene only)
   holding effect layers, bottom first: each a built-in effect (or own code) with **Mix** (how much shows),
   **Blend** (how its result mixes onto what it took: Normal, Add, Screen, …) and **Takes** (Whole picture / Light
   only / Glow only). Drag to reorder, hide, rename, duplicate, copy and paste, like the Look's layers.
2. **Effect memory and warm-up.** Effects that read their last frame declare how long they remember. After a seek
   the editor shows the frame at once, then replays the remembered seconds in the background on a second renderer
   ("Warming up… N%") and swaps it in, so a paused frame matches the export (which always runs from the start).
3. **Fifteen picture effects** (the spec's list): Feedback tunnel (Echo grown: drift and a centre), Kaleidoscope
   (more folds: rings and grid), Hue storm, Liquid light, Light ripples, Ball worlds, Polar tunnel, Breathing space,
   Thermal vision, Op-art lens, Fractal light, Rainbow echoes, Melting picture, Solarized neon, Gravity lens.
4. **Electric arcs** (`layer.arcs`, a ball around layer): lightning between balls that come near, which lights a lit
   scene (strip lights) and takes the around sub-layers (so 1b's Gives off light applies once it lands).
5. **Aura fields** (`layer.aura`, a ball group layer): one flowing field round the group's balls, drawn as glowing
   contour lines (CPU marching squares) and a soft fill.
6. **Negative light**: a Negative switch on Light things (gives darkness), and the light sum clamped at 0 per channel
   so darkness never pushes a pixel below black. (Negative on Gives off light is part 1b's; told to that thread.)
7. **Effect header spans**: `[a, b]` in an effect header is now the slider's usual span; `hard [a, b]` is a real
   limit. The 13 existing effects' limits are reviewed (table below). New header lines `choice` and `memory`.

Together: the spec's 18 psychedelic items (15 effects + Negative light + Electric arcs + Aura fields).

## Decisions made while planning (each a fill-in of the spec, stated here)

1. **One layer kind per built-in** (`picture.<id>`, e.g. `picture.kaleidoscope`, and `picture.own` for own code)
   in the card `effects`, section `LayerSection::Effect`, lane 0, repeats. Each kind's settings are `mix`, `blend`,
   `takes`, `glow_above` (when takes=glow), then the effect's own header settings under plain keys. Materials
   (stage Things: Lava, Rainbow flow, Hit glow) have no Takes (they paint things, not a picture). The add menu lists
   "Over the picture" then "Materials". The old one-effect `shader.<id>` module kinds are retired (no Type menu).
2. **Old scene files**: an upgrade (`add_upgrade(12, single_effect_to_stack)`, format stays 11 so other parts'
   format bumps don't collide; the step is a no-op on stacks) turns a scene card `shader.<id>` into an Effects card
   with one `picture.<id>` layer, its `blend` number becoming `mix`. Test scenes go through `put_old_card`, which
   does the same; a `shader.*` card on a ball is dropped as before.
3. **Takes**, at the Picture stage on the main picture only (materials and blend layers always take the whole
   picture):
   - **Whole picture**: as today.
   - **Light only**: the effect works on the light that falls on lit things. With a Lighting card the lit picture
     is `P = U + (L − 1)·W` (U the picture before lighting, W the lit colour, 1a's mix), so the shader derives
     `L = 1 + (P − U)/W` per channel (where W > 1e-4, else L = 1 there), the effect's `sample()` sees L, and the
     result goes back as `P + (L' − L)·W`. U is `targets.behind` (copied before the light pass), W is the lighting's
     lit target; both are bound only for a lit frame. No Lighting card: Light only changes nothing (the editor says
     "Light only needs a Lighting card"). This needs nothing from 1b; when 1b lands its glow light is part of P − U,
     so it bends with the light.
   - **Glow only**: the effect works on the part brighter than **Glow above** (default 0.9, span 0–2):
     `G = max(P − T, 0)`, result `P − G + G'`. It works in any scene, lit or not, and catches glows, flashes, hit
     light and hot spots.
   - Mix and Blend apply in the taken picture (`mix(X, blended(blend, X, X'), mix)`).
4. **Mix past 0..1 is allowed** (span [0, 1], no clamp): above 1 pushes the effect further, below 0 pulls the
   other way. The shader's final clamp to [0, 64] and its NaN guard stay. A Mix of exactly 0 skips the effect.
5. **Memory pictures**: an effect that reads `prev()` (or `kept()`) renders a second output, its memory, into the
   history (MRT; location 1, then copied). For Whole picture the memory is exactly today's output (goldens). For
   Light/Glow it is the taken picture after the effect (L' or G'), so feedback on the light feeds back light.
   New prelude `keep(v)` stores a number per pixel in the memory's alpha (main picture only) and `kept(uv)` reads it
   back (Thermal vision's heat). Effects without memory keep one output.
6. **Memory length** (header `// memory: <seconds>` or `// memory: fade <key>`): `fade <key>` reads that setting as
   a per-1/60 s keep factor, seconds = ln(1/256)/ln(fade)/60, at most `MAX_MEMORY` 10 s (a fade ≥ 1 never forgets:
   10 s). An effect with `prev(` and no memory line remembers 2 s. `EffectCall.memory` carries the seconds.
7. **Warm-up** (editor only, `preview/warm.rs`): when the shown time jumps (backwards, or forward more than the
   chain's `MAX_STEP` 0.25 s) and the frame has effects with memory, a `Warmup` starts: a lightweight copy of the
   playback (`Playback::rewound(time)`: the latest checkpoint at or before it, no others), seeked to
   `t − memory` (at most 10 s, never before 0), then stepped frame by frame at the scene's video fps, each frame
   built and drawn on a second `Renderer` (same size, shared programs), a budget of 6 frames per editor frame, then
   the target frame is drawn there and the two renderers swap. A new seek, a revision change or Play cancels it.
   The pill reads "Warming up… N%". Export needs nothing: it renders every frame from 0.
8. **What the prelude gains**: the ball list (storage buffer, `MAX_FX_BALLS` 1024 a frame: `ball_count()`,
   `ball_uv(i)`, `ball_radius(i)` in screen heights, `ball_color(i)`, `ball_hit(i)` seconds since its last hit,
   `ball_seed(i)`), filled from `Frame.ball_marks` (the kit's balls and emitter draws push a `BallMark` per live
   ball when the scene has an Effects card), gathered from the frame and its blend layers' frames; the beat
   (`fx.beat`: beat phase, bar phase, beats per second; `beat()`, `bar()`, `kick()` = the stronger of the beat's
   pulse and the hit pulse), from `Frame.beat` set by the scene's draw from the Mixer card's tempo at the moment
   (constant-tempo phase from 0, like the timeline grid); the light picture through Takes. The in-ball prelude gets
   the same names (no balls, no beat but the hits' pulse), so every built-in still runs in a ball's Shader layer.
9. **New header line kinds**: `// choice key = default [a, b, c] help` (packed as the index, read as
   `p.key == 0.0`…; shows as a menu), `// memory: …` (above). Reserved keys gain `mix`, `takes`, `glow_above`.
10. **Arcs**: pairs of the thing's live balls whose edges are within **Reach** (O(n) per ball with a grid of cells
    the size of Reach; at most **Most arcs** per frame, nearest first, default 64, span 0–500, no hard top), each
    arc from edge to edge through **Detail** points offset across by two octaves of value noise (**Jagged**),
    seeded by the two ball numbers and the flicker tick `floor(time × Flicker)`; an arc shows on a tick when its
    seeded chance is under **Chance**, closer pairs brighter (fade with gap/Reach). **Fire**: Always / When they hit
    (either ball hit within Fire length) / On the beat (within Fire length after each beat). Optional **Branches**
    (a share of arcs fork once). Drawn as two strips (core and glow) through `strip_with_subs`, additive by default.
    In a lit scene each arc adds a strip light along its points (`ThingDraw::light` on balls and emitters), its
    colour × **Lights the scene**, reach **Light reach**, no shadows (`p5.w` 0). Lit default 0, Blocks light off.
11. **Aura**: a group layer drawn by the balls' draw as an around piece (behind the balls by default). The field is
    `f(p) = Σ (r_i / |p − c_i|)²` over the group's live balls (balls further than Reach × r skip: grid binning);
    contour lines where `log2(f) × Lines − time × Flow` crosses a half-integer, found by marching squares on a grid
    of **Detail** cells across the group's box grown by Reach (linked into polylines), coloured by the
    field-weighted ball colours (or its own gradient across the levels), drawn as soft additive strips; the fill is
    one soft additive sprite per ball (`Particle` shape smoke) sized Reach × r × 0.6, at **Fill**. Lit default 0.
12. **Negative**: `negative` (bool, group "Special") on the Light settings flips the strength's sign when the light
    is pushed. `fs_light` clamps the summed light at 0 per channel before contact shading and the mix.
13. **Electric arcs and Aura are draw-only**: nothing physical, no history, worked out from the world at the moment
    drawn, so scrubbing and export agree.
14. **Deferred** (stated): Lens flares, Anamorphic streaks and Colour fringes (part 5's); Effects stack entries for
    them come with part 5. The Light Lab mixes "Arcs on the beat" and "Painted mandala" are scenes of what is here.

## The 13 existing effects' limits (spans unless a hard limit is named, with why)

| Effect | Setting | Was | Now |
| --- | --- | --- | --- |
| Colour cycle | speed, kick, saturation, contrast, pulse | ranges | spans (negative contrast inverts; the final clamp catches infinities) |
| Colour split | amount, kick, radial | ranges | spans (negative swaps red and blue) |
| Echo (Feedback tunnel) | fade | [0, 0.99] | span [0, 0.99], hard [0, 1e9]: a negative keep has no meaning |
| | zoom | [0.9, 1.1] | span, hard [0.01, 1e9]: the code can't zoom by 0 or less |
| | spin, hue, pulse | ranges | spans |
| Film | vignette, grain, scanlines | ranges | spans |
| Glitch | amount, react, rate, pulse | ranges | spans |
| | bands | int [4, 120] | span, hard [1, 1e9]: zero bands divides by zero |
| Hit glow | punch | [0, 10] | span (negative darkens on hits) |
| Kaleidoscope | slices | int [2, 24] | span, hard [1, 1e9]: fewer than one slice means nothing |
| | spin, zoom, angle, pulse | ranges | spans |
| Lava | speed, scale, heat | ranges | spans |
| Liquid | amount, scale, speed, pulse | ranges | spans |
| Pixels | size | int [2, 64] | span, hard [1, 1e9]: a pixel under one video pixel means nothing |
| | gap | [0, 0.6] | span |
| Rainbow flow | bands, speed, glow | ranges | spans |
| Shockwaves | width | [0.005, 0.2] | span, hard [0.0005, 1e9]: a ring of no width divides by zero |
| | strength, speed, glow, life, pulse | ranges | spans |

The new effects' hard limits: Ball worlds levels hard [0, 16] and Fractal light steps hard [1, 1024] (loops per pixel:
more hangs the GPU), Polar tunnel arms and Solarized neon bands hard [1, 1e9] (division), Fractal ball hard [1, 1e9].

## Files and types

### bpe-core
- No format bump. Nothing else (layers, sections and edits are reused).

### bpe-gfx
- `effects/mod.rs`: `ParamKind::Number { default, min, max, int, hard: Option<(f32, f32)> }` (min/max the span),
  `ParamKind::Choice { default: usize, names: Vec<String> }`, `Header.memory: Memory` (`None`, `Seconds(f32)`,
  `Fade(String)`), `memory_seconds(header, params) -> f32`, `MAX_MEMORY`. Parser for `hard`, `choice`, `memory`.
  `assemble` adds blend maths, the Takes plumbing and, for memory effects, the second output. `BUILTINS` grows to 26
  (13 old + 13 new files), materials last. `PRELUDE_NAMES` gains the new prelude names.
- `effects/prelude.wgsl`: `Fx` gains `takes: f32`, `glow_above: f32`, `blend_code: u32`, `ball_count: u32`,
  `beat: vec4<f32>`; bindings 5 (unlit picture), 6 (lit colour), 7 (balls storage); `sample` reads the taken
  picture; ball, beat and `keep`/`kept` helpers.
- `effects/in_ball.wgsl`: the same helper names (no balls, the hits' pulse as the kick, `kept` 0, `keep` nothing).
- `effects/builtin/*.wgsl`: 13 new files; echo and kaleidoscope extended (defaults draw the same picture); every
  header's ranges reviewed (table above).
- `effects/chain.rs`: `Fx` mirrors; `run` binds the unlit and lit pictures (or black) and the balls buffer, skips
  Light-only calls on unlit frames, renders memory effects with two targets, copies the memory into the history;
  `base_uniform` adds the beat and ball count; `fx_balls(frame)` gathers marks through the camera.
- `frame.rs`: `EffectCall` gains `takes: Takes` (`Whole`, `Light`, `Glow`), `glow_above: f32`, `blend: Blend`
  (mode) beside `amount` (the old `blend: f32` renamed `amount`), `memory: f32`; `BallMark { center, radius, color,
  code, seed, hit_age }`, `Frame.ball_marks`, `Frame.beat: [f32; 4]`; `Frame::memory()` (longest memory).
- `lib.rs`: passes the unlit picture (`targets.behind`) and the lit colour to the Picture-stage chain run on a lit
  frame. Nothing changes for frames with no effects.
- `shaders/lighting.wgsl`: the per-channel clamp at 0 (decision 12).

### bpe-kit
- `modules/effects/` (new; replaces `modules/shader/`): `mod.rs` (`EffectsCard` module kind `effects`, the layer
  kinds `PictureEffect(&Builtin)` and `OwnEffect`, `Takes` choice, `param_defs` moved here, `ids`), `draw.rs`
  (`EffectsDraw`: one `EffectCall` per shown layer, in order; `pack_params` moved here), `upgrade.rs`
  (`single_effect_to_stack`, used by the registry upgrade and `put_old_card`).
- `modules/look/layers/shader.rs`: `EFFECTS` lists every built-in (26) then own; imports move.
- `modules/look/layers/arcs.rs` (new, `layer.arcs`): settings, `arcs()` (pairs, points), draw with subs, `lights()`.
- `modules/look/layers/aura.rs` (new, `layer.aura`, section Group): settings, `field_lines()` (marching squares),
  draw.
- `around.rs`: `AroundKind::Arcs`, `AroundKind::Aura`; `around_layers` reads them.
- `things/balls/draw.rs`, `things/emitter/draw.rs`: draw the new around kinds, push `BallMark`s when the scene has
  an Effects card, `light` hook for arcs.
- `things/scene/draw.rs`: `frame.beat` from the Mixer card's tempo.
- `things/light/mod.rs` + `draw.rs`: `negative`.
- `lib.rs`: register the card, 27 effect layer kinds, arcs, aura, the upgrade; drop the `shader.*` kinds.
- `scenes.rs` / `scenes_stage5.rs`: `put_old_card` turns a root `shader.*` into a stack layer.
- `starters.rs`: a new Effects card starts empty.

### bpe-studio
- `look_editor/effects.rs` (new): the Effects card: "Effects" list (top first) with "+ Add effect" (two headings),
  rows from `layer_list::row` with drag to reorder, the picked effect's settings (an own code effect's code box), the
  Light-only hint. `inspector.rs` routes an `effects` card there.
- `preview/warm.rs` (new): `Warmup`, the second renderer, the pill; `preview.rs` starts/advances/cancels it.
- `bpe-runtime/src/playback.rs`: `Playback::rewound(&self, time) -> Playback`.
- Tests that used the Shader card's Type menu move to the Effects card.

## Tests (what passing looks like)

- **Header** (`bpe-gfx` effects): spans and `hard` parse (default inside hard, outside span allowed), `choice`
  packs the index, `memory` both forms and the 2 s default, `memory_seconds` of a fade, reserved keys; every
  built-in parses, assembles and validates (naga) over the picture, alone in a ball and three at once.
- **Chain** (GPU, lavapipe): Glow only leaves dim pixels exactly as they were and changes bright ones; Light only on
  an unlit frame changes nothing; Light only on a lit frame keeps a Lit-0 glow as it was while a lit ball changes;
  Blend Add adds; Mix 2 pushes past the effect; an effect reading `ball_count()` sees the frame's balls; `keep`/`kept`
  carry a value to the next frame; every built-in renders the two-balls frame with no NaN and (but Mirror on a
  symmetric frame) a changed picture.
- **Kit**: the Effects card makes one call per shown layer in order with Mix, Blend, Takes and memory; hidden layers
  draw nothing; an old `shader.kaleidoscope` card upgrades into a one-layer stack with its settings; arcs: two balls
  within Reach make strips, far ones none, the same moment draws the same arcs, a lit scene gets one strip light per
  arc; aura: two balls give closed contour loops round them; a negative light pushes a negative colour.
- **Studio** (kittest): the scene's Effects card lists its effects and "+ Add effect" adds one; the Takes row shows
  for a picture effect and not for a material; warm-up: a seek on a scene with Echo starts a warm-up that finishes
  and leaves the renderer's history warm (the frame equals a straight run's frame).
- **No-card identical** (`unlit_pictures`): unchanged hashes (echo and kaleidoscope test scenes included). Physics
  and sound goldens unchanged.
- `cargo clippy --workspace`, `cargo fmt`, design-system colour test.

## Demo for the owner (overnight rule)

A test-only scene builder renders a short video and stills with the release renderer into project files
`ballphysics/lighting/part8/`: Feedback tunnel + Kaleidoscope rings, Hue storm, Ball worlds, Gravity lens, Thermal,
Solarized neon, Electric arcs, Aura fields, and a negative light. No scene is added to the program.

## Things not to do

- No change to frames with no effects (and no change to any pipeline when nothing new is used).
- No hard limits for taste; no built-in scenes in the program; no colours outside `style/`.
- Don't keep picture state between frames beyond declared effect memory (the warm-up replays it).
- Don't move physics or sound goldens.

## What changed while building

- **Upgrade step**: `add_upgrade(u32::MAX, single_effect_to_stack)` rather than 12, so it runs on every file
  whatever format later parts bump to; on a stack it does nothing.
- **Aura** is an around layer (`AroundKind::Aura`, with Electric arcs), not a group layer: it takes the around
  sub-layers and sits with trails and particles. It is drawn by the balls' draw from the thing's own live balls.
- **The beat** comes from the scene settings' Tempo / Beats in a bar / Beat note (`things::scene::draw::beat_at`),
  where the timeline grid reads it, not from the Mixer card.
- **Memory length** counts frames until what is kept is under 1/4096 (not 1/256): the cut-off is in linear light,
  and 1/256 still shows as about 13 steps of the 8-bit picture, so a warmed-up frame came out visibly short.
- Negative sits in the Light group of the Light settings.
