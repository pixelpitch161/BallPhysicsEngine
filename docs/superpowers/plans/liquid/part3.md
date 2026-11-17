# Liquid part 3: looking real (plan)

Spec: `docs/superpowers/specs/2026-11-17-liquid-thing-design.md`, sections 6.3 (Wet balls), 6.5 (Wet walls), 7
(Look) and 12 part 3. Builds on parts 1 and 2 (`docs/notes/liquid.md`). Aedan asked for the whole liquid feature to be
finished and merged without approval stops (2026-11-17), so every decision is made here from the spec.

Parts 4 (stranger liquids) and 5 (sound) are built at the same time. Part 4 grows the solver's `Drop` from 20 to 24
floats and adds `fades`/`fades_to` to the Look card's Colour mixing group. This part does **not** change the solver's
step kernels or the `Drop` layout; every shader that reads drops reads them through one constant `DROP_FLOATS`, so
whoever merges second changes one number.

## What the owner sees

The Liquid thing's **Look** card, top to bottom:

1. **Its colour** (unchanged) and **Colour mixing** (unchanged, plus **Mixing shows** and **Shows as**).
2. **Surface**: the 15 Surface chips the balls, shapes and the Particles Liquid layer wear (painted balls; picking
   one sets the numbers under it), then those numbers (folded), as the Particles Liquid layer has them (no Throws
   light, Facets, Facet gaps).
3. **Shape of the surface** group: Edge, Edge softness, Roundness, **Smoothing**, Opacity, In front of the balls.
4. **Inside** group: **Clarity** (how far you see in before the colour takes over), **Deep colour** (what it turns
   to deep down), **Light soaks in** (soft bright edges, milk).
5. **Light** group (its own pretend light, used without a Lighting card or at Lit 0): Shine, Light from, **Rim light**,
   **Reflects** (how much sky it mirrors), Sky top / Sky bottom, **Bend** (how far it bends what's behind it) and
   **Rainbow split**.
6. **Detail on top** group: **Ripples** with **Ripple size**, **Thin film** with **Film size**, **Outline** with
   **Outline colour**, **Gives off light**.
7. **With lighting** group: **Lit** (0 keeps its own light) and **Blocks light**.
8. A **Layers** list (`+ Add layer`): **White water and foam**, **Bubbles**, **Things inside**, **Mist**, **Caustics**,
   **Wet walls**, **Wet balls**. Each at most once, each can be turned off, each with its own settings.

With a Lighting card the real lights shade it through its Surface (shadows, highlights, glass bending lamp light,
coloured shadows), its glow lights the scene, and its foam, bubbles and flecks are lit with it.

## Design decisions

1. **One pass, more numbers.** All surface looks are worked out in the existing surface pass (`liquid.wgsl`
   `fs_surface`) from the field. `LiquidStyle` gets eight new `vec4`s (`h` to `o`) after `g`; `STRIDE` goes from 256 to
   512 bytes (dynamic offsets stay 256-aligned). A zero in each new number means "as before", so the Particles
   Liquid layer (which leaves them at default) draws byte-identically, and every unlit picture test and golden
   stays as is.
2. **Second field target becomes `Rgba16Float`** (was `R16Float`): r the colour weight (as before), g foam
   (`w × foaminess`), b how fast the drop moves (`w × |v|` in px/s / 1000, for the caustics' and ripples' motion and
   Mixing shows), a the drop's kick (`w × kick`, set by balls, fading: splashes foam). `liquid_drops.wgsl` writes all
   four; the Particles blob path never writes the second target (it isn't bound there) and `colour_of` reads only r.
3. **Smoothing** is the drop reach in cells: `reach = 1 + 2 × smoothing` (default 0.25 → 1.5, today's reach), and the
   inside level `INSIDE = π reach²` follows (part 1's maths). Edge level is still `level × INSIDE`.
4. **Depth.** `inside` (px from the edge, from the field slope) is the depth. **Clarity** `c` px: cover becomes
   `shown × (1 − exp(−inside / c))` blended towards 1 by opacity of the deep colour, i.e.
   `alpha = shown × mix(1, 1 − exp(−depth/c), see)`, with `see = 1` when Clarity > 0. Colour goes from the colour at
   the edge to **Deep colour** × colour by `1 − exp(−depth / c)`. Clarity 0 = solid (today). Default for a new Liquid:
   Clarity 160 px, Deep colour a dark blue-green `#0b2a3a`; water now reads as water.
5. **Bend** (unlit): when Bend > 0 and Clarity > 0 (or the Surface is a glass kind), the surface pass reads the
   picture behind it (the renderer copies it first, as for pretend Glass): `LiquidStyle::reads_behind()` replaces
   `glass()` in `draw_liquids`. The behind picture is offset by `normal.xy × bend` (the normal is the rounded rim's
   plus ripples). **Rainbow split** samples r, g, b at `bend × (1 ± split)`.
6. **Surface kinds unlit**: the 15 kinds map onto the six pretend codes (`particles::liquid::pretend_code`), as the
   Particles Liquid layer does. Glass kinds read behind with Tint 0.3 (as today's Glass) and their Clarity.
7. **Ripples**: a value-noise height (two octaves, moving with time and the drops' mean speed b-channel) at Ripple
   size px adds to the normal's xy by `ripples × 0.6`, only inside (fading in over the rim). Lit: added to the height
   written in pass 2 (`ripples × size × 0.15`), so the lamps catch them.
8. **Thin film**: a rainbow (cosine palette) by `depth / film_size + noise + time × 0.1`, mixed over the colour by
   `film × fresnel-ish (0.3 + 0.7 (1 − n.z))`. Same colour goes to pass 1 in a lit scene.
9. **Light soaks in**: `colour × soak × (1 − x)` added near the rim (x the rim fraction), and in a lit scene also
   to the emission pass (it reads as light coming through).
10. **Reflects** scales the pretend sky term (`sky × fresnel × reflects`), default 0.3 (today's Plastic number).
    **Rim light** is the pretend rim (today 0).
11. **Outline**: a band `outline` px wide just inside the edge in Outline colour, mixed by the band's cover.
12. **Gives off light**: `glow × colour` added to the picture; in a lit scene it is written to the emission pass
    (3), which lights the things near it, as a ball's glow does.
13. **Mixing shows**: where neighbouring colours differ (the colour 3 px either side, `|Δ|`), a shimmer (Shimmer:
    animated sparkle noise × white), a soft glow (Glow: the mixed colour brighter, into emission when lit) or foam
    (Foam: white froth). Draw-only, no solver change.
14. **Layers** are layer kinds on the Liquid's Look card (`section: Around`, `repeats: false`, card `liquid.look`),
    read by the draw in `cards::LookLayers::read`. Off or missing = its numbers 0 in the style. Shader layers
    (picture effects) are not offered here: the Liquid is drawn by its own pass, not in a layer frame (known gap).
    - **White water and foam** (`liquid.foam`): Amount 0.8, From speed 450 px/s (drops faster than this foam, full at
      twice it), Splashes foam 1 (from kick), On top 24 px (foam shows within this depth of the edge, plus wherever
      a drop is stretched), Froth size 3 px, Colour white. Foaminess per drop in the splat:
      `smoothstep(from, 2 from, |v|) + splashes × kick`, clamped to 1.
    - **Bubbles** (`liquid.bubbles`): Amount 0.4, Size 5 px, Size variety 0.6, Rise speed 70 px/s, Wobble 0.5, Shine
      0.8. Procedural in world space (cells 4 × size, a bubble where `hash < amount`), rising with time, only deeper
      than their size, popping (a ring that fades) at the top. Deterministic in time, so seeking is fine.
    - **Things inside** (`liquid.inside`): Share 0.03 of the drops, Size 1.6 px, Sparkle 0.7, Colour from (Their own
      liquid / One colour), Colour (gold). Drawn by a new dots pass (`liquid_dots.wgsl`) from the drops themselves
      (a drop is a fleck when `hash(serial) < share`), so they ride the flow; drawn only where the field says inside.
      Sparkle flickers each fleck's brightness by `hash(serial, floor(time × 8))`.
    - **Mist** (`liquid.mist`): Amount 0.7, Size 1.2 px. The same dots pass draws each drop that is outside the
      surface (the field at its middle under the Edge level) as a soft dot of its colour: spray as fine dots.
    - **Caustics** (`liquid.caustics`): Amount 0.5, Size 70 px, Speed 0.6, Deep 400 px (how far down they fade),
      Colour white. A procedural caustic net (the min of two moving cellular-noise distances, sharpened) inside the
      liquid, strongest near the top. Unlit: added light. Lit: into the emission pass too, so they light the scene.
    - **Wet walls** (`liquid.wet_walls`): Amount 0.5, Dries in 4 s, Streaks 0.6. Needs memory of where the liquid
      was: the body keeps a **wet grid** on the card (decision 15). Drawn by a full-screen pass before the surface
      (`fs_wet` in `liquid.wgsl` reading the wet grid as a storage buffer, its own bind group), in the liquid's
      colour, broken into vertical streaks by a column noise.
    - **Wet balls** (`liquid.wet_balls`): Shine 0.8, Tint 0.4, Dries in 3 s, Drips 0.6. Draw-only. A ball's wetness
      comes from the **event log** (decision 16): 1 while in the liquid, `exp(−t / dries)` after its last Out. Drawn as
      particles in front of the balls (`frame.particles_front`): a tint disc of the liquid's colour (cover
      `tint × wet × 0.5`), a highlight dot up-left (added light, `shine × wet`) and a hanging drip under the ball
      (Drips, shrinking as it dries). Needs Pushes balls (the events are logged only in the main world): known gap.
15. **Wet grid** (`bpe-gfx liquid_sim`): `Body.wet`, a buffer of `2 × cols × rows` f32 (ping-pong halves), zero at
    birth, copied by `Clone`, counted in `bytes()`. A new pipeline `cs_wet` in its own shader file
    (`liquid_wet.wgsl`, its own bind group: a uniform `[cols, rows, parity, 0]` + `[keep, drip, 0, 0]`, grid read,
    solid read, wet read-write) is dispatched once at the end of each `Body::step`, after the last substep: a node in
    the walls band (`solid[2n].w > 0.5`) with mass on it (`grid[4n] > 0`) is wet 1; else it keeps `keep` of its last
    value and takes `drip` of the node above it (y − 1, world y is down), so wet runs down in streaks. `keep =
    exp(−dt / 6 s)`, `drip = 0.995` a step at 240 Hz (scaled by dt). The look's Dries in remaps the value:
    `shown = clamp((wet − e) / (1 − e))`, `e = exp(−dries / 6)` (so any Dries in up to about 20 s works). Always
    computed (cheap: one thread a node; nodes ≤ 2 M, typically 30k–250k), so it needs no fingerprint and a look
    edit restarts nothing. `DropsSource` gains `wet: wgpu::Buffer`, `cols`, `rows`, `wet_half` (which half was last
    written, from `steps % 2`).
16. **Wet balls from the log**: `things::liquid::wet::wetness(log, rate, time, liquid) -> BTreeMap<(ThingId, u32),
    f32>` walks the Liquid events of that Liquid thing (`EventKind::Liquid`, item 0 in, 1 out, ball in `a`) up to
    `time`. In without a later Out → 1; Out at `t₀` → `exp(−(time − t₀)/dries)`; under 0.01 dropped.
17. **Real lighting** (Lit > 0 with a Lighting card), following W15 (`real_lit`):
    - `style.d = [lit, stamp, blocks, link]` as the Particles Liquid does. Surface index: after every thing's and
      Surface sub-layer's (`surface::taken`) and every Particles Liquid layer (`particles::liquid::layers_taken`),
      the Liquid thing's place among Liquid things in tree order (`things::liquid::surface_index`). The draw and the
      new `LiquidDraw::light` hook both work it out from the scene alone.
    - `LiquidDraw::light`: `scene.set_surface(index, s.gpu(SurfaceGpu::SHEET))`, `refracts |= s.see_through()`, and
      when it blocks light and its Surface bends light, a `Bender` box round the liquid's Area (the host layout:
      origin to origin + cols × cell), its index.
    - Pass 1 (lit colour): the composed colour (depth, foam, bubbles, film, outline) × Lit; pass 2: the rounded
      height plus ripples; pass 3 (emission): glow + caustics + soak + Mixing shows (Glow) + the colour's glow; the
      picture: the composed colour plus that light. Blocks light: `surface::keeps_of(surface::passes(s))` when on.
    - Dots (flecks, mist) and wet walls in a lit scene: drawn in the picture and pass 1 (× Lit), nothing elsewhere.
18. **Frames without a liquid** stay byte-identical: nothing is drawn or copied when no liquid is in the frame. The
    second target's format change only affects frames with a liquid.
19. **Restarts**: every new Look key and layer is draw-only, so nothing restarts; `restarts.rs` must still pass (its
    rule: a key whose change doesn't change any fingerprint is fine when it doesn't change the run).

## Files and what changes

**bpe-gfx**
- `src/liquid.rs`: `LiquidStyle` fields `h..o` (documented), `reads_behind()`, `STRIDE` 512, `WEIGHTS_FORMAT`
  Rgba16Float, a dots pipeline (`vs_dot`/`fs_dot` in `liquid_dots.wgsl`, instanced over the drops, drawn after the
  surface into the same target, premultiplied), a wet pipeline (`fs_wet`, full screen, drawn before the surface, its
  own group: style + wet buffer). `Placed.drops` keeps the dots' and wet's groups.
- `src/shaders/liquid.wgsl`: Style grows; `world_of(px)`; noise helpers; the new looks in `fs_surface`; `real_lit`
  takes the composed colour, the glow and the ripple height; `fs_wet`.
- `src/shaders/liquid_drops.wgsl`: Style grows; `DROP_FLOATS`; reach from `e.w`; writes the four weights.
- `src/shaders/liquid_dots.wgsl` (new): flecks and mist.
- `src/shaders/liquid_wet.wgsl` (new) and `liquid_sim/mod.rs`: the wet grid (decision 15).
- `src/renderer/draw.rs`: `reads_behind()`.

**bpe-kit**
- `things/liquid/cards.rs`: Look settings grow; `look_defs()` (the card's settings with the Surface numbers after
  `surface`, Plastic's values); `LookLayers` (the seven layer kinds' settings, read from the card); `kinds()` lists
  them.
- `things/liquid/draw.rs`: `style_of` fills every new number; the `light` hook; Wet balls particles.
- `things/liquid/wet.rs` (new): wetness from the log.
- `things/liquid/mod.rs`: `surface_index`.
- `things/particles/liquid.rs`: `layers_taken(scene)`.
- `scenes.rs`: the water test scene keeps its look; a `liquid_looks` test scene list ("Liquid lit pool": lit room,
  glass water with foam, bubbles, caustics, flecks, wet walls, wet balls; "Liquid glow": neon liquid giving off
  light in the dark).
- `tests/full_size_look.rs`: shots `liquid-lit` and `liquid-looks` (unlit, every layer on) and `liquid-glow`.

**bpe-studio**
- `inspector/slots.rs`: the Liquid Look card shows the Surface chips (`surface_section` with the liquid's keys)
  then its grid without the surface keys, then the Layers list.
- `inspector/surface.rs`: `surface_section` takes which keys to show.
- `look_editor/liquid.rs`: `show_layers` (section Around), and `list_of` takes the section.

## Tests (what passing looks like)

- `bpe-gfx` `liquid_sim::tests::wet_grid_marks_walls_and_dries`: a body with a walls band and drops: after steps the
  wet grid is 1 on band nodes under the liquid, 0 away from walls; after the drops are gone it decays; a clone holds
  the same words.
- `bpe-gfx` picture tests (`liquid_look_tests.rs`, `test_device()`): a drops liquid on a grid drawn (a) plain with the
  new numbers at zero gives exactly the old picture (Particles liquid regression: blob path byte-identical), (b)
  Clarity makes the middle of a shallow pool show the background, (c) Foam on fast drops whitens, (d) Gives off
  light makes the emission pass non-zero, (e) Bend shifts what's behind.
- `bpe-kit` `things/liquid/tests_look.rs`: `look_defs` holds the Surface numbers minus `NOT_ON_A_LIQUID`; layer kinds
  register on the Look card; `style_of` fills foam/bubbles/etc. only when their layer is on; `surface_index` comes
  after a Particles Liquid layer's; the `light` hook puts the surface in the table and a bender when glass blocks;
  `wetness` from a made-up log (in → 1, out → decays, Dries in); a lit scene with a liquid draws real numbers
  (`d.x > 0`).
- `bpe-kit/tests/lit_liquid_pictures.rs` gains a lit Liquid-thing picture: the lamp's side is brighter than the far
  side and the liquid casts a shadow.
- Existing: `unlit_pictures.rs`, `lit_pictures.rs`, `restarts.rs`, `golden_liquid.rs`, goldens: unchanged.
- Studio: the Liquid Look card shows the Surface chips and `+ Add layer` adds Foam (kittest).
- Full-size look check: `LOOK_ONLY=liquid-water,liquid-mixing,liquid-lit,liquid-looks,liquid-glow`, look at full
  pictures and crops for grain, seams, flicker, light leaks.

## Not doing (here)

- Shader layers (picture effects) on the Liquid's Look.
- Foam that lasts after the drop slows (would need per-drop memory in the solver, part 4's territory).
- Wet balls for a liquid beside the physics (no events there).
- Changing the solver's step or the `Drop` layout.
