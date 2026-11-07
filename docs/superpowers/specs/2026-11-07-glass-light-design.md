# Glass and light: coloured shadows, prisms, focusing glass, wild glass

Date: 2026-11-07. Status: design approved by the owner (Aedan) in the "Explore coloured glass and prism lighting"
thread ("Looks good", 18:34Z), after he picked **all four parts, in this order**. Each part gets its own plan in
`docs/superpowers/plans/glass-light/` and its own thread.

- Audit and live sketches (A coloured shadows, B prism, C focusing glass, D wild glass):
  https://claude.ai/artifact/9cPaDZPP3AVPH9v4NmG6an. The sketches are web mock-ups of the look, not engine renders.

Builds on the lighting system (spec `2026-11-06-lighting-design.md`, plans `plans/lighting/`), in particular part 2b
(glass, Rainbow split, the behind picture), part 3 (glass-ball caustics, the bake), part 4 (lasers and their `Meets::Bend`
hook) and the see-through fix (`plans/lighting/see-through-blockers.md`, #145), whose known limit "no coloured shadows"
part 1 removes.

## What exists today (audit on main 645a8e7)

| Feature | Today | Gap |
| --- | --- | --- |
| Glass surface (`look/surface.rs`) | Bends light (IOR), See-through, Rainbow split (Glass preset 0.3), Frost; the behind picture bent by `transmitted` | Only what is *seen through* the glass splits |
| Glass caustics (Lighting card › Quality, default 1) | `lighting::caustic_glass`: see-through **spheres** focus photons onto wall/floor (Stage) or the plane (Flat), a wavelength each with Rainbow split | Shapes, liquid and particles never focus light |
| Lasers (`things/light/laser.rs`) | `Meets::Bend { index }` through glass balls and shapes, total internal reflection, `MOST_BOUNCES` 256 | One index, one colour: no dispersion |
| Shadows through glass | Flat: `march` multiplies `Ray.lit` by what each see-through blocker lets through, colourless. Stage: `sphere_shadow` tints by a glass ball's colour | Flat is grey; Stage shapes (`slab_visibility`) block as solid; the light probe treats see-through as solid |
| Light's Shadow colour | One colour for all of a light's shadows | Not from the glass |
| Mirror walls, Chrome | Show a reflection of the picture (`fs_mirror`, `trace_scene`) | Don't redirect lamp light |

## Rules every part follows

1. **Draw-only.** Physics, sound and the physics goldens never move. A scene without a Lighting card stays byte-identical
   (`bpe-kit/tests/unlit_pictures.rs`).
2. **Nothing see-through, nothing changes.** A lit scene with no see-through record (no opacity under 1, no See-through
   over 0) draws exactly as before (`tests/lit_flat_pinned.rs`, `tests/stage_pictures.rs` unchanged).
3. **Live and baked.** Every look works live and in the Bake (`Renderer::render_samples`); the bake may use more rays.
4. **Spans, not limits.** New numbers use `span(..)` and only hard impossibilities get a `range(..)`.
5. **One-way sensing.** The light probe (lighting part 7) sees part 1's coloured light as brightness (luminance). Parts
   2 to 4's beams, photons and spots stay out of the probe, as lasers' beam glow is today. Nothing the lighting reads may
   sense light.
6. **Keyframes and Follow sound.** Every new number is `keyframe` (so it can be keyed, pulsed and follow sound) unless
   stated otherwise.

## Part 1: coloured shadows and stained glass

Light that passes through anything see-through takes its colour, pixel by pixel, in Flat and Stage, for balls, shapes,
trails and particles, in the direct light, the light field (bounce and glow light) and the haze.

### Look

- Red glass throws a red pool; red over blue throws purple (colours multiply). A gradient, pattern or brush strokes on a
  see-through thing are thrown as a picture onto what is behind it (stained glass).
- How much passes is unchanged from #145 (opacity and See-through); part 1 only adds the colour.
- **Deeper when thick:** the colour strengthens with how far the light travels inside (Beer's law), so a glass ball's
  middle tints more than its rim and a thick bar more than a thin line.

### Settings

- Surface section (`look/surface.rs::SurfaceSettings`, every Look card with a Surface, and the Surface sub-layer):
  - **Tints light** (`tints`, span 0..1, default 1 on every preset): 0 lets light through in the lamp's own colour
    (today), 1 takes the full colour.
  - **Deeper when thick** (`thick_tint`, span 0..1, default 0 on every preset but Glass/Jelly 0.5): how much the
    thickness deepens the colour.
- Particle and trail layers have no Surface section: they tint by their colour at full strength whenever they are
  see-through (opacity under 1). (Their Lighting fold's Blocks light already decides whether they block.)
- Lighting card (`modules/lighting.rs`), Shadows group: **Coloured light through glass** (`glass_colour`, switch, default
  on). Off gives #145's colourless picture exactly.

### How it is drawn

- A new light pass, the **tint picture** (`Globals.light.x` = 8, a new mode beside 1..7): Rgba8Unorm, cleared to white,
  every record drawn with a **multiply** blend (`dst × src`), writing `mix(1, chroma(colour), tints × see_through_share)`
  where `chroma(c) = c / max(c.r, c.g, c.b)` (so dark glass tints but doesn't darken twice: darkness is already in how much
  passes), and in alpha the thickness (a ball: its height, the G of the surface picture; a shape: distance inside its
  outline from `shape_at`, normalised by Thickness or the shape's size) times Deeper when thick. Multiplying means overlapping separate
  things combine (fixing #145's "the top thing's blocking only" for colour). Records with nothing see-through write white.
  Every draw path takes the mode: `layers.wgsl`, `paths.wgsl`/`shape_layers.wgsl`, `sprites.wgsl`, trails, fields,
  liquid, swarm draws. Made only when the frame has a see-through record and `glass_colour` is on
  (`LightScene::needs_tint`).
- **Flat direct light:** `march` keeps `Ray.lit` as a colour (`vec3`): leaving a see-through stretch multiplies it by
  `tint^(1 + THICK·a)`, with `tint` the tint picture's colour and `a` its alpha along the stretch (the most saturated
  sample and the largest alpha, matching how `glass` keeps the most blocking pixel; `THICK` a constant pinned by a test).
  The tint picture's alpha is thickness × Deeper when thick, blended with a separate **max** alpha component (the colour
  multiplies, the thickness keeps the deepest). `visibility` returns a `vec3`; `fs_direct`'s `tint` (the
  shadow-colour mix) already takes a `vec3`. The direct pass is at its 16-texture limit (`DIRECT_BINDINGS`): the plan
  frees a slot (preferred: the direct pass gets its own group-0 layout without the `picture` binding it never reads, the
  tint picture taking that slot).
- **Light field and haze:** `fs_gi_scene` multiplies what it gathers through see-through texels by the tint, so bounce and
  glow light come out coloured; `air` (haze) uses the coloured visibility, so beams through red glass glow red in the air.
- **Stage:** `slab_visibility` stops treating see-through shapes as solid: where the ray crosses a slab it reads the
  shape's share (field G) and its tint at the crossing's picture-plane point. Spheres keep `sphere_shadow`'s tint, gaining
  Deeper when thick (chord length through the sphere).
- **Light probe** (`bpe-kit/src/sensing/probe.rs`): see-through things let their share through, coloured by their
  thing-level colour (the Body's, or a shape's first paint), and levels read the luminance (`level_of`).

### Tests

- White or grey glass, and Tints light 0, give the #145 picture (`tests/see_through_shadows.rs` unchanged; a new test
  sweeps Tints light 0).
- Red over blue gives purple behind both; a two-colour gradient pane throws both colours where expected
  (`tests/coloured_shadows.rs`, GPU, tile averages).
- `glass_colour` off is byte-identical to before.
- Stage: a see-through shape's slab shadow lets its share through, coloured.
- Probe: a red see-through pane between a lamp and a ball lowers the ball's level by the pane's share and luminance.

## Part 2: prism and rainbow beams

White light through glass fans into a rainbow, for lasers and a new wide beam.

### Settings

- Laser (`things/light/mod.rs`, Laser group): **Colours** (`colours`, range 1.., span 1..32, default 12, not keyframed):
  how many wavelengths a beam splits into where it bends. 1 = today.
- **Light beam**, a new light kind (`LightType::Beam`, code 6, Add thing › Lights and the Lights panel "+ Beam"): a laser
  with a width, like sunlight through a slot. Its Beam group: Width (span 0..200 px), Softness (edge falloff), Rays across
  (range 2.., span 2..64, default 16), Colours (as the laser), plus the laser's Beam length, Beam glow, Hit glow, Bounces,
  At shapes, At balls. Handles: the laser's aim, plus two width handles.
- Surface: a **Prism** preset chip (14th): clear (See-through 0.95, Roughness 0), Bends light 1.6, Rainbow split 1.
- Rainbow split (existing) is the dispersion: the index for a wavelength λ is
  `n(λ) = n + rainbow × D × (λ₀ − λ)/(λ₁ − λ₀)` with λ from 400 to 700 nm, λ₀ = 550, and D chosen so that Rainbow split 1
  on Bends light 1.5 spreads the colours about 4° through a 60° prism (`DISPERSION` in `laser.rs`, pinned by a test).

### How it is drawn

- `laser::trace` gains a wavelength: `Meets::Bend { index }` is bent with `n(λ)` (a `Dispersion` passed in from the
  surface's Rainbow split via `surface::glass_of`, which returns index and rainbow). The beam is traced once white; at the
  first bend whose dispersion is above 0 it splits into `colours` paths (`trace_split`), each carrying its spectral colour
  (sums of all colours = white; a coloured light keeps only its own share of each). Shared prefixes are drawn once.
  At most `MOST_BEAM_SEGMENTS` (4096) segments a light; past it the farthest-traced colours are dropped first.
- Each path is lit and drawn as strip segments exactly as a laser's today (`push_strip`, `laser_beams` in the mix), so
  the colours light surfaces near them, glow in haze and leave hit glows (a rainbow stripe where they land).
- A Beam traces `rays_across` parallel rays per colour and draws each neighbouring pair as a **ribbon** (a soft quad strip
  between the two rays' paths, split where the two paths take different turns, e.g. one bounces and one passes), lighting
  the strip segments of its middle ray at the ribbon's strength. Ribbons go in the laser beam list so haze and blooms
  catch them.
- Balls that are glass bend and split beams too (the tracer already meets balls as circles).

### Tests

- Rainbow split 0 or Colours 1 gives today's laser path exactly (`laser.rs` unit tests).
- A 60° glass prism splits a white beam into `colours` paths ordered red (least bent) to violet, spread within tolerance
  of the formula; total internal reflection still happens per colour.
- `MOST_BEAM_SEGMENTS` cap holds with Bounces 256 between two mirrors.
- GPU: a beam through a prism leaves a red-to-violet stripe on the wall (`tests/prism_pictures.rs`).

## Part 3: every glass shape focuses light

Lamp light through any glass shape really bends: bright lines and darker gaps behind lenses, moving ripples under water,
rainbow rims with Rainbow split. Today only balls do this.

### Settings

- Lighting card Quality: the existing **Glass caustics** now covers shapes, liquid and particles too. New: **Bent light
  rays** (`bent_rays`, span 0..4, a multiplier on the photon count) and **Bent light softness** (splat size).
- Particle Look layers (Sprites, the Particles thing's layers and `layer.particles`) gain **Bends light** in their
  Lighting fold (default 1 = off), so bubbles bend light. The Particles **Liquid** layer with Surface Glass writes the
  surface picture (today it draws as covering black there) so it bends light too.

### How it is drawn

- **Glass in the light:** a pixel whose way to a lamp crosses glass that bends light (IOR above 1, see-through) no longer
  gets that lamp's light straight through it: `march` reports the share that went through bending glass, and the direct
  light keeps only the rest (plus the part reflected or blocked as now). The bent share is drawn by photons instead.
- **2D photon pass** (Flat, and the picture plane in Stage): `bpe-gfx/src/bent_light.rs`, `shaders/bent_light.wgsl`.
  The kit lists each frame's bending glass (`LightScene.benders`: a box and a surface index per thing, from the same
  stamps as the surface table). For each (light, bender) pair, photons leave the lamp toward the bender's box (a
  stratified fan over the angle it covers, `photons_per_pair` sized by Quality and Bent light rays), march through the
  blocker field and surface picture, refract at each crossing with the IOR from the surface table (the normal from the
  field's gradient), reflect totally inside where they must, take a wavelength each with Rainbow split, pick up part 1's
  tint, and stop at a solid blocker or the light's reach. Each photon's path is splatted as soft line segments into a
  half-size Rgba16Float **bent light** picture with the lamp's falloff, normalised so that glass with Bends light 1 gives
  the same light as the direct light it replaced (the key test). The direct light and the air add it (`fs_mix`).
- **Stage:** glass spheres keep part 3's sphere caustics; glass shapes send their photons on the picture plane, so their
  focused light lands on the wall behind them.
- **Bake:** more photons, a new photon pattern turn each sample (fixing part 3's "only rotates per sample" note).

### Tests

- Bends light 1: the bent light picture equals the direct light it replaces within tolerance (tile averages).
- A convex lens shape focuses: the brightest tile behind it is several times the unbent level; a concave one spreads.
- Rainbow split on a lens gives coloured rims (red outside violet, or the reverse past the focus).
- Nothing bending: no photon pass runs (timings show zero), pictures unchanged.

## Part 4: wild glass and mirrors

May be planned as 4a (mirrors and light pipes, which reuse part 3's photons) and 4b (dichroic, diamond fire, rainbows),
each its own thread.

- **Mirrors throw light.** Photons (part 3's pass) also leave lamps toward mirror walls and shiny blockers (Metal ≥ 0.5,
  Roughness under 0.5: Chrome, Metal, Brushed) and reflect, spread by Roughness; in Stage, toward chrome spheres, landing on
  the wall and floor. Setting on the Surface: **Throws light** (span 0..1, default 1 for Chrome, 0.5 Metal, 0 others).
- **Mirror ball** surface chip: Chrome with **Facets** (span 4..400, default 120) and **Facet gaps**. Its shading shows the
  tiles; photons hitting it reflect off the facet normal (quantised from the ball's turn, so spinning balls sweep their
  spots), landing as soft spots; in haze they show as thin rays.
- **Dichroic** (Surface numbers, any preset): **Dichroic** (span 0..1) and **Dichroic shift** (degrees of hue over the
  angle). Light through it takes a hue that turns with the angle it meets the surface; light bounced off takes the
  opposite hue. Applies to part 1's tint (angle from the field's gradient against the lamp's way), part 2's beams (a
  Bend also sends a reflected beam in the opposite hue), part 3's photons, and the thing's own look (an angle-shifting
  sheen like Pearl's).
- **Diamond fire** (Surface number **Fire**, span 0..1): highlights split into a few spectral glints (the specular lobe
  sampled at three to seven wavelengths with slightly different bends), plus star glints (Glitter's sprite) on hits,
  their strength following the hit's age.
- **Light pipes** (Surface switch **Light pipe**, shapes with a band): light entering the band (from a lamp near or
  inside it, or a laser or beam) runs along the outline and glows out of it, fading over **Carries** (span 0..2000 px);
  drawn as strip lights along the outline with the travelled strength, brightest where the light entered and at
  corners sharper than the critical angle.
- **Rainbows in rain:** the Lighting card's **Rainbow** group: Rainbow (strength), Rainbow centre (the anti-sun point; by
  default worked out from the brightest Sun light's direction), Rainbow size (the 42° bow's radius), Second bow, Dark band.
  Drawn in `fs_mix` where haze density or rain catches it: particle layers' Lighting fold gains **Makes rainbows**
  (writes the tags picture's free B channel). Colours by the real bow's angular spread (red outside, violet inside; the
  second bow reversed, fainter, at 51°; Alexander's dark band between).

### Tests

- A flat mirror wall throws a lamp's light into a spot where the reflected geometry says.
- A mirror ball with N facets throws about N/2 spots facing the lamp; turning it moves them.
- Dichroic 0 changes nothing; at 1 transmitted and reflected hues are opposite.
- Rainbow strength 0 changes nothing; at 1 the bow's red ring lies outside its violet ring at the set size.

## Editor

- Surface section: the new numbers fold under the chips as the others do; the Prism and Mirror ball chips are painted in
  `style/swatch.rs` (a split-light prism, a tiled ball).
- Lighting card: Coloured light through glass (Shadows), Bent light rays / softness (Quality), the Rainbow group.
- Lights panel and Add thing menu: "+ Beam". Beam handles in `preview/light_handles.rs` (aim and two width handles).
- No new windows.

## Order and threads

Part 1 first (parts 2 to 4 reuse the tint). Then parts 2 and 3 can run side by side (part 2 touches the laser tracer and
light kinds, part 3 the GPU photon pass). Part 4 after part 3 (mirrors and light pipes use its photons). Each part: its
plan in `docs/superpowers/plans/glass-light/partN.md`, one Opus review before its PR, merges itself when checks and the
review pass. The exe is installed once, after all parts are merged.

## Not in scope

- Hardware ray tracing (the lighting spec's decision stands).
- Light through glass in the physics or the sound (one-way rule).
- Polarisation, fluorescence beyond the existing UV glow, and volumetric glass (glass is a surface on 2D things and
  Stage spheres/slabs).
