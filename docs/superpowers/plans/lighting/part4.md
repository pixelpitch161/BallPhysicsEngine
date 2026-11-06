# Lighting part 4: air and extras

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved). Builds on 1a (`part1a.md`, #106), 1b
(`part1b.md`, #111), 1c (`part1c.md`, #110) and 6 (`part6.md`, #108). Built straight after this plan (no approval
stops, the owner's overnight instruction of 2026-11-06), one Opus review at the end, then the PR, merged by this
thread once tests and the review pass. Parts 2a, 5, 7 and 8 are built at the same time; whichever lands later
rebases and runs every test again. A short video and stills go to project files `ballphysics/lighting/part4/`.

## What part 4 delivers

- **Swirling haze**: the Lighting card's Air fold gains Haze moves (Still / Drifting / Swirling), Haze speed, Haze
  size, Patchiness, Drift direction and **Balls stir it**: the haze becomes a moving, uneven volume; beams through
  it go streaky, and balls leave swirling wakes in it.
- **Smoke that catches light**: a **Catches beams** setting in the Lighting fold of trails, particles and the
  wavefront. Where such a layer covers, the lamps' light in the air (each lamp's own shadows, its Beam in haze)
  lights it up in its own colour, so a beam crossing a smoke trail shows as a bright shaft through the smoke.
- **Underwater light**: the Air fold gains **Underwater** (0..1), Water colour, Water top, Depth fade, Caustics,
  Caustic size, Caustic speed and Sun rays: moving caustic light on everything, colour that fades into the water
  colour with depth, and shafts of light from the surface.
- **Lasers**: a new light kind **Laser** (Add thing ▸ Light ▸ Laser, the Lights tab's "+ Laser"): a thin beam from
  the lamp along its Direction (Fixed, Where it's going, Spins), traced on the CPU through the world: it bounces
  off shapes and balls (each Bounce / Stop / Pass through), up to Bounces times and Beam length. It lights what is
  round its path (like a strip, Reach and Falloff), glows as a visible line with a bright spot where it hits
  (Laser glow, Hit glow), shows in haze and Light blooms. **Glass hook**: the tracer already bends a beam through
  a surface that answers `Meets::Bend { index }` (Snell, total internal reflection); part 2b only has to say which
  things are glass.
- **UV reveal**: a light can be **Ultraviolet** (it lights nothing but a faint violet, its Visible light share) and
  every drawn and around layer gains **Under UV** (Normal / Glows / Only under UV) and **UV glow**: where UV light
  falls (with its shadows) such a layer glows in its own colours; Only under UV layers are hidden elsewhere.
- **Glow in the dark**: the Lighting card's new **Memory** fold: Glow in the dark, Remembers (The background /
  Everything), Charge speed, Fade time, Glow colour from (The light's colour / Own colour), Glow colour. Where light
  falls the picture charges, then glows it back as the light moves away, fading over Fade time. Exact in exports
  (rendered from the start); the editor warms the memory up after a seek.
- **Heat glow**: a new sub-layer **Heat glow** (`sub.heat` on ball layers and the Body, `shape.sub.heat` on shape
  layers and the shape's Body): hits heat the ball (or the shape) up, and it glows red, then orange, yellow and
  white as it gets hotter, cooling down over time. Settings: Heat per hit, Cool down, Hottest, Glow, Lights the
  room (how much of its glow it gives off as light).
- **Gobos and flicker** on every light, in a new fold **Gobo and flicker**: Gobo (None, Stripes, Dots, Window,
  Leaves, Water caustics, Rings, Clouds), Gobo size, Gobo turn, Gobo spin, Gobo drift, Sharpness, Gobo depth;
  Flicker, Flicker speed, Flicker style (Candle / Faulty / Pulse), Flicker seed. Gobos show on surfaces and in
  haze (a striped beam), flicker on everything the light does.
- **Live caustics**: the moving Water caustics gobo and the underwater caustics (both worked out live from time,
  the same in the editor and the video). Light focused by glass balls and lenses needs glass (2b) and comes with
  the bake (part 3); the laser tracer's `Bend` is the hook.
- **Lights up (light linking)**, deferred from 1c: a light's **Lights up** (Everything / Only these / Everything but
  these), **Things** (a thing, a tag or a kind, the existing target picker) and **The background too**.
- **Picked colour**, deferred from 1c: a light's Colour from gains **Picked**: the colour of a thing's layer
  (**Pick from** a thing, **Layer** number), as it is at the moment drawn; riding each ball of a group, each ball's
  light takes that ball's own colour from the layer.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **Two new light passes, only when used.** Mode 5 (**UV**): every layer writes, with its cover, its colour × UV
   glow when it glows under UV and black otherwise (so what is in front hides what is behind); drawn only when a
   light is ultraviolet *and* a layer glows under UV (`LightScene.uv`). Mode 6 (**tags**): every layer writes R =
   Catches beams, G = its thing's link number, times its cover (premultiplied, Normal blend, A = cover); drawn only
   when something catches beams or a light uses Lights up (`LightScene.smoke`, `LightScene.links` not empty).
   Shine, Shading, Shader and group Shader layers that don't glow leave what is under them (as in mode 3). Every
   `g.light.x` comparison in the WGSL is checked for the new modes (mode 4 stays the background's own light).
2. **Only under UV layers** are hidden in modes 0 to 3 and 6 (their piece's cover set to 0) and shown in mode 5.
   The kit sets the flag only when the scene has a Lighting card, so without the card such a layer shows normally
   (nothing is lit, nothing is UV). The record field is 0 in every older scene: the mode 0 path is a branch on a
   value that is 0, and the no-card hashes stay the same.
3. **Records**: `LayerGpu.more: [f32; 4]` = UV glow (0 for Normal), Only under UV (0/1), Catches beams, link
   number (size 240 → 256). `TrailVertex.extra` and `Particle.extra` (`[f32; 4]`, the same meaning), with vertex
   attributes. The background's link number is `Globals.light.z`. Part 2a uses `LayerGpu.light[3]` and the surface
   picture's B; part 4 uses neither.
4. **Link numbers**: a thing's link number is 1 + its place among the resolved scene's things (`links::number_of`),
   the same in the draw and the light hook; past 2047 (the half-float picture's exact whole numbers) a thing gets
   0 and Only these never lights it (stated limit of the picture, not of the data). At an edge between two things
   one pixel can read a third number: accepted, as 2a's surface index (a 1 px seam).
5. **Lights up**: `LightGpu.p10` = mode (0 everything, 1 only, 2 all but), first number in the `links` buffer,
   how many, the background too (1/0). `lamps()` reads the pixel's number from the tags picture (rounded G / A,
   0 where nothing covers: the background) and skips the light's surface light for a pixel it doesn't light up
   (the light in the air, haze and beams, is never linked). **Things** is a `Target` (a thing, its tag, its kind;
   a group of balls by its Balls thing, a shape's layers with the shape); the matched things are the scene tree's
   (`links::matching`), a thing matching by Thing also takes its children.
6. **Picked colour**: `LightColour::Picked`, `pick_from: Target` (default the Balls kind), `pick_layer: u32`
   (1 = the base layer). The colour is that layer's main colour (its `colour` setting: a colour, or a paint's
   gradient at its middle, or for the Body the group's per-ball colour when the light rides each ball of that very
   group and the Body follows each ball, `balls::draw::ball_colour`). No such layer: the light's own colour.
7. **Lasers** (`things/light/laser.rs`): `trace(world, from, dir, s, skip) -> Vec<Vec2>` marches a ray against
   every wall (`outline_points`, each side a capsule of the band's half thickness, `Wall.solid` too) and every
   live ball (circles). A hit is answered by `meets(thing) -> Meets { Bounce, Stop, Pass, Bend { index } }`:
   shapes by **At shapes**, balls by **At balls**; `Bend` refracts (Snell from outside in and inside out, total
   internal reflection) and is never returned in part 4 (2b returns it for glass). A laser riding a ball skips
   that ball. The path's segments go in `LightScene.segments`; the GPU lights from it as from a strip (kind 5 is
   handled by the strip branch of `source_point`), and the mix adds the beam: `colour × (glow × core(d / width) +
   hit glow at each bounce point)`. Defaults for a new Laser: Strength 3, colour `#FF2F4E`, Reach 160 px, Size 2,
   Height 20, Width 3 px, Bounces 8, Beam length 8000 px, glow 1, hit glow 1.
8. **Gobos** are worked out in `lamps()` in the light's own frame: `q = rotate(p − lamp, −(turn + spin·t)) / size
   − drift·t / size` (a sun's in the world's frame), pattern value 0..1 sharpened by Sharpness, the light times
   `1 − depth × (1 − pattern)`. Same on the haze. Patterns: Stripes, Dots, Window (four panes with bars), Leaves
   (thresholded value noise, moving), Water caustics (moving cell edges), Rings, Clouds (soft noise). `p8` = kind,
   size, sharpness, depth; `p9` = cos, sin of the turn, the drift's offset xy. "Custom picture" and "Drawn" gobos
   need a picture import and a drawing tool: not in this part (stated).
9. **Flicker** is CPU-side in `push_light` (`flicker(t, speed, style, seed)`): Candle (smooth noise, 1 − amount ×
   n), Faulty (mostly on, short dropouts), Pulse (a smooth sine-ish throb); pure in time and seed, so the editor and
   export agree.
10. **Haze moves**: a field-size **extras** pass (`fs_extras`) writes A = the haze's density: value-noise fbm of
    the world point drifted along Drift direction × speed × t (Drifting) or domain-warped by a turning field
    (Swirling), mixed to 1 by 1 − Patchiness; plus a **stir** picture: every live ball of every Balls/Emitter thing
    (`LightScene.stirs`, pushed by their light hooks when Haze and Balls stir it are on) splatted as an instanced
    quad (3 radii) adding its velocity × falloff; the extras pass moves its noise point by the stir and swirls it.
    Still = density 1 everywhere (1b's haze exactly). The mix multiplies the haze's light and veil by the density.
11. **Memory**: the extras pass's RGB is the remembered light: `m = old × e^(−dt/fade) ` then mixed toward the
    light falling here (direct + field + 2 × UV, no ambient) by `1 − e^(−charge × dt)` where it is brighter
    (charging never lowers it). Two field-size pictures ping-pong; the old one is read **reprojected** through the
    last frame's camera. `dt` is the frame time step; 0 (the same frame drawn again) changes nothing; backwards or
    more than 0.5 s forwards clears it (a seek): `Renderer::memory_cold()` then says so. The mix adds `m × glow ×
    colour`, only where the background shows past what blocks light (The background) or everywhere (Everything).
    Export draws every frame from 0 s, so it is exact. **Warm-up in the editor**: after a seek while paused, when
    the memory is cold, the preview re-runs the light (not the picture) for frames from up to 3 × Fade time before
    (at most 90 frames) with `Renderer::warm_light`, then draws the frame, so a paused frame matches the video.
12. **Heat glow** writes a Flash on hit record (code 28) with `amount.w = 1` (heat): `amount.x` glow, `.y`
    hottest (0..1 up the ramp), `.z` lights the room. Each ball's heat (`hit[3]`, the instance's spare) is worked
    out on the CPU: Σ over its hits of Heat per hit × hit strength × e^(−age / cool down) (`heat_of`); a shape's
    from the hits against it. The colour runs dark red → orange → yellow → white with heat (`heat_colour`). It is
    added as light over where its layer covers (unlit by the lamps, like a lamp), and in the emission pass gives
    off light × Lights the room (`LightScene.glows` is set). Several Heat glow sub-layers on one thing share the
    first one's Heat per hit and Cool down (one heat a ball; stated).
13. **UV light** is added up in `fs_direct` into the direct picture's alpha (one number: how much UV falls here,
    with shadows and gobo), its visible share added as ordinary light. Mix: `out += uv × uv picture`.
14. **Smoke**: `out += air light × Catches beams cover × the picture's colour there` (the smoke glows its own colour
    by the light in the air), after the haze. The air light is worked out when there is haze or smoke.
15. **Underwater** is all in the mix: depth below Water top `d = max(y − top, 0)`; `fade = 1 − e^(−d / depth fade)`;
    light × (1 + caustics × underwater × (pattern − 0.35) × (1 − fade)) and × mix(1, water colour, underwater ×
    fade); sun rays (slanted, moving bands) add `water colour × rays × underwater × (1 − fade)`. Above Water top
    nothing changes. Caustic pattern shared with the gobo (`caustics(q, t)`).
16. **Bindings**: wgpu's default limit of 16 sampled textures a stage. Group 1 keeps 1b's 10 and gains binding 11
    (tags), 12 (UV picture), 13 (extras), 14 (stir). Each pass's group 1 layout lists only what it reads: the 1b
    passes as before; `direct` + tags (16 in all); `mix` without emission, gi_scene, gi_dist, above (14);
    `extras` with direct, above (= the last memory), the field ways and stir (12). The stir pass has its own group
    0 (the uniform, seen by the vertex stage, and the stirrers buffer). Group 0 gains binding 9 (`links`, storage).
17. **No limits**: spans only, hard limits only where the maths breaks: Haze size, Caustic size, Gobo size
    `range(0.001, …)` (divides by it), Fade time and Cool down `range(0.001, …)`, Depth fade `range(0, …)`, Laser
    width, Beam length `range(0, …)`, Bounces `range(0, …)` (a count), Flicker seed and Pick layer `range(…)` as
    counts (Pick layer from 1). Everything else unbounded, keyframes and can Follow sound.
18. **Scene format** stays 11 (new keys only).
19. **Not in this part** (stated so nothing is silently dropped): custom picture and drawn gobos; light focused by
    glass (2b/3); per-layer heat spots where a shape was hit (the whole shape heats); haze stirred by trails or
    shapes (balls only); glow-in-the-dark per layer (the background or everything).

## Files and types

### bpe-gfx
- `frame.rs`: `LayerGpu.more`, `TrailVertex.extra`, `Particle.extra`; `LightGpu.p8..p11`; `LightingGpu.haze`
  (moves code, speed, size, patchiness), `haze_more` (drift xy, stir, 0), `water` (underwater, caustics, caustic
  size, caustic speed), `water_more` (colour rgb, sun rays), `water_depth` (top, depth fade, 0, 0), `memory` (glow,
  fade, charge, remembers), `memory_colour` (rgb, from light 1/0); `LightScene.uv`, `.smoke`, `.links: Vec<f32>`,
  `.stirs: Vec<[f32; 4]>` (position xy, velocity xy) + `stir_radius` per stir in a second vec4 → `[[f32; 4]; 2]`.
- `lib.rs`: modes 5 and 6 drawn into `uv` and `tags` pictures when asked; `Globals.light.z` the background's link
  number; trail and sprite vertex layouts gain `extra`; `light_globals` sized for modes 1–6; `memory_cold`,
  `warm_light`.
- `lighting.rs`: new targets (`uv`, `tags`, `extras[2]`, `stir`), layouts per pass (decision 16), pipelines
  `stir`, `extras`; memory state (`last_time`, `last_camera`, which picture is current, cold flag).
- `shaders/lighting.wgsl`: `fs_stir`/`vs_stir`, `fs_extras`, noise and caustics helpers, gobos, links and UV in
  `lamps()`, laser beams, underwater, memory, smoke and density in `fs_mix`.
- WGSL elsewhere: `light_piece` modes 5/6 and `uv_hide` (layers.wgsl, used by layers_walk and shape_walk),
  `light_flat(c, lit, blocks, emit, extra)` (scene.wgsl, sprites.wgsl and every caller), `sub_flash` heat mode
  with `HEAT_*` privates (sub_layers.wgsl), trail.wgsl and sprites.wgsl pass `extra`.

### bpe-kit
- `things/light/mod.rs`: `LightType::Laser`; `LightColour::Picked`; new settings (Laser fold, Gobo and flicker,
  Special: Ultraviolet, Visible light; Lights up; Picked); `Gobo`, `FlickerStyle`, `LightsUp`, `AtThings`.
- `things/light/laser.rs` (new): the tracer and `Meets`.
- `things/light/draw.rs`: lasers, gobos, flicker, UV, links (`LightScene.links`), picked colour.
- `things/light/links.rs` (new): `number_of`, `matching`.
- `modules/lighting.rs`: Air (haze moves, underwater), Memory fold; `gpu()` fills the new fields.
- `modules/look/layers/lighting.rs`: Under UV, UV glow on every Lighting fold; Catches beams on around layers
  (`AroundLit`); `extra_of`.
- `modules/look/layers/heat.rs` (new): `HeatGlow` settings, `SUB_HEAT`, `SHAPE_SUB_HEAT`, `heat_of`, record.
- `layers/gpu.rs`, `shape/gpu.rs`: `SubLayer::Heat`, records carry `more`; `needs_hits` with heat; `glows` with heat.
- `balls/draw.rs`, `things/shape/draw.rs`: heat in `hit[3]`; link numbers; stirs pushed in the light hook.
- `layers/around.rs` and the particle/trail draws: `extra` stamped.
- `scenes.rs`: `air_demo` (test-only) for the video; tests use `lit_room`/`glow_room` and switch things on.

### bpe-studio
- `preview.rs`: memory warm-up after a seek while paused (`warm_memory`).
- `preview/light_handles.rs`, `lights_panel/`: the Laser kind (aim like a spot).

## Tests (what passing looks like)

`bpe-kit/tests/lit_pictures.rs` gains, each on small renders, comparing pixels:
- Swirling haze is uneven across a beam (pixels along the beam differ by more than at Still) and a ball moving
  through Balls stir it changes the haze next to it.
- A trail that Catches beams is brighter inside a spot's cone than the same trail outside it, and without the
  setting the beam doesn't brighten it beyond its lit colour.
- Underwater: light at the same height varies with the caustics; deep down is tinted the water colour.
- A laser lights a bright line along its Direction and not beside it; aimed at a shape it bounces (lit past the
  bounce point on the reflected side); At shapes Stop ends it.
- UV: an Only under UV ball is invisible with no UV light and glows its colour under a UV light; a Glows ball
  shows normally and brighter under UV; a ball between the UV lamp and another throws a UV shadow.
- Glow in the dark: after a lamp is switched off (frames drawn in order), the background still glows where it
  was lit, and less after Fade time; a cold memory (a seek) shows nothing until warmed.
- Heat glow: a ball that was just hit glows reddish brighter than one never hit; it lights a neighbour in the dark.
- Gobo Stripes makes the lit floor alternate light and dark across; flicker changes the light from frame to frame
  but is the same at the same time.
- Lights up Only these: a ball not in the list is unlit by that light while the listed one is lit.
- Picked colour: a light picking a red ball's Body lights a white ball red.
- Units: laser tracer (reflection off a wall, stop, pass, ball bounce, a `Bend` refracts and totally reflects),
  link numbers, heat sums, flicker determinism, new settings have only maths-breaking hard limits.
- No-card identical: `unlit_pictures.rs` unchanged and passing. Physics and sound goldens unchanged.
- `cargo clippy --workspace`, `cargo fmt`, the editor's kittest tests.

## Things not to do

- No new pass without its feature in use (modes 5/6, extras, stir only when asked); nothing at all without a
  Lighting card. Nothing physical reads any of it. No state kept between frames except the declared memory. No
  hard ranges for taste. No colours outside `style/`. No built-in scenes in the program.
