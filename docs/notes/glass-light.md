# Glass and light (spec `docs/superpowers/specs/2026-11-07-glass-light-design.md`, plans `docs/superpowers/plans/glass-light/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

Part 1 is in (plan `part1.md`, "As built" at its end): **coloured shadows and stained glass**. Light pass 8, the
**tint picture** (`TINT_PASS`, `shaders/tint.wgsl`: `tint_out` writes optical depth `−ln(share) + (−ln T)(1 + THICK ·
thick)` with alpha 0, so premultiplied over adds depths and what passes multiplies; `T = mix(1, chroma, tints ·
presence)`), drawn by balls (`layers.wgsl` `light_piece`/`shade_layers`, `TINT_BLOCKS`/`TINT_THICK` from the walk),
shapes (`shape_layers.wgsl`), trails/sprites (`scene.wgsl::light_flat`, `sprites.wgsl`); liquid writes nothing. Made
only when `LightScene::needs_tint` (Coloured light through glass on, lights or glows, `Frame::see_through_blockers`),
`Lighting::make_tint`. Only the seed and field passes read it (bound in `picture`'s place): the blockers' field is
Rgba16Float, R distance, GBA each channel's blocking (`blocker_channels`, `foot_tint`; readers take `opacity_of` = min
and `colour_of`; on a Stage the grey depth gives the opacity under `TINT_SOLID_DEPTH`); clear glass under `BLOCK_LEAST`
still seeds where it colours (`pixel_tints`, `TINT_LEAST`, `clear_tint` in `march`). `Ray.lit`/`e`, `visibility`,
`slab_visibility` (see-through Stage slabs now pass their share) are `vec3`; `sphere_block` tints by a sphere's
`c[1]` Tints and `b[3]` Deeper; `fs_cascade` multiplies by the glass colour on the way (`Uniform.trace` = [bounce,
stage, glass colour, tinted]). Records carry Tints light and Deeper when thick in `LayerGpu.head[2]` bits 16..23/24..31
(64ths, `LayerGpu::tint_bits`, stamped by `surface::Stamps`). Surface gains **Tints light** (`tints`, 1) and **Deeper
when thick** (`thick_tint`, 0.5 on Glass and Jelly); the Lighting card's **Coloured light through glass**
(`glass_colour`, on; `LightingGpu.more[3]`). The light probe tints too (`LookLight.passes`, `Ball.passes`, walls'
passes; `visibility` → `[f32; 3]`). Off, or Tints light 0, the picture is byte-identical. `tests/coloured_shadows.rs`.

Part 2 is in (plan `part2.md`, "As built" at its end): **prism and rainbow beams**, CPU tracing plus one GPU pass.
`laser.rs`: `Meets::Bend { index, rainbow }` (from `surface::glass_of`, now a `Glass { index, rainbow }`), `index_at`
(`n + rainbow · DISPERSION · w/2`, w −1 red to 1 violet, `DISPERSION` 0.0462 = 4° through a 60° prism at Rainbow split
1), `spectrum(n)` (the shader's `spectral` hats, shares adding to white per channel), `trace_split` (white to the first
glass with Rainbow split it bends into, then a path a colour; at most `MOST_BEAM_SEGMENTS` 4096, each colour an even share
of what is left; `trace` never splits), `Path.corners` (`Met`: thing, row, side, `Turned`). Filled walls are solid glass to the tracer
(`Wall.solid`: the bare outline, light inside leaves only through its outer side, `turn_of`), so a filled shape is a real prism. Lights:
**Colours** (`colours`, 1..64, default 12, group "Beam", which the laser's settings moved into) and the **Light beam**
(`LightType::Beam`, code 6, "+ Beam": Width `beam_width`, Edge softness, Rays across): `push_light` pushes one `LightGpu`
a run (white, then each colour, `push_one`), a beam's middle ray lighting as a laser that draws no line, and its band as
`LightScene.ribbons` (`bpe_gfx::RibbonGpu`, `laser::ribbons`: neighbouring rays' segments paired while they meet the same
things, cut where they part; `BEAM_SHOW` 0.35, `MOST_RIBBONS`) drawn by `vs_ribbon`/`fs_ribbon` (group 3 binding 2),
added into the mix's target right after the mix, brighter where the band narrows. The **Prism** Surface chip (14th:
See-through 0.95, Bends light 1.6, Rainbow split 1). Beam width handles (`LightPart::Width`). Ribbons stay out of the
probe. `tests/prism_pictures.rs`.

Part 3 is in (plan `part3.md`, "As built" at its end): **lamp light bent by glass shapes, liquid and bubbles**. A
*bender* is a Shape whose Surface (or a layer's own) lets through ≥ 0.05 and has Bends light > `BENDS_FROM` 1.0001
while its layer blocks light (`surface::bends`, pushed by `ShapeDraw::light` as `Bender { lo, hi, index }` into
`LightScene.benders`), or **free glass**: particle sprites whose Lighting fold's **Bends light** (`ParticlesLit`,
`Particle.light[2]`; the Bubbles look 1.33) or a Glass Liquid layer's Bends light (`LiquidStyle.c[2]`) bends
(`Frame::free_glass_box`, id `FREE_BENDER` 4096): only when photons go through it, light pass 9 (`GLASS_PASS`,
`Renderer::draw_glass`: sprites and liquids alone) draws it into the glass picture (index × cover, cover;
`Lighting::make_glass`), and `shaders/free_glass.wgsl` marks it over a copy of the surface picture where nothing is solid,
keeping R/G/A and writing B = −(1 + 16 q_ior + q_cover) (`free_glass_of` in lighting.wgsl; index in 64ths, cover in
15ths). Balls keep the sphere caustics. `bpe-gfx/src/bent_light.rs`: `BentPlan::of` (lib.rs and `Lighting::run`
agree on it), `bent_pairs` (light × bender, lights that cast shadows, no lasers, within reach), `bent_budget`/
`bent_photons` (Draft 4096 … Ultra/bake 32768 a frame × Bent light rays, at most `MOST_BENT_PHOTONS` 2^17, ≥ 256 a pair
while the cap allows, so every pair is carried), `bend_bits` into `Uniform.bends` (2048 bits; `u.bent.x` turns every new
branch on, so without benders pictures are byte-identical; `bent_carried` keeps glass see-through for lasers). lighting.wgsl: `bender_at`/`bender_covering`, `bends_at`,
`nearest_bends` (Stage, through the seeds); bending glass seeds the field and counts as **solid** in `march`/`confirm`/
`slab_visibility` (a pixel on it leaves it first). `shaders/bent_light.wgsl` (pasted after lighting.wgsl): `cs_bent`
(a compute pass, group 3: 4 `BentU`, 5 pairs, 6 legs written) sends each pair's photons in fans from up to 16 points
across the lamp (`lamp_copies`; a sun's from a line before the box), steps by the field, classifies pixels
(`bent_at`: open, see-through, solid, bender; a glass's soft edge counts as open), refracts with the surface table's
index (free glass from the picture), Sobel normals over 5 × 5 (`glass_normal`), total internal reflection, exact
Fresnel loss (`passes_edge`), Frost, Rainbow split (`spectral`, `BENT_COLOURS` 16 wavelengths in turn in a fan), part
1's tint, gobo at the first glass, steps at most a pixel in glass, and keeps up to `BENT_LEGS` 8 legs (48 bytes: ends,
path lengths, haze, light, power, path key, + `RAINBOW_KEY` once split) from where it first leaves its pair's glass (a
photon whose first glass is another bender's is that pair's; with one leg left it bends no more), carrying the light
only as far past its last glass as the direct light counts it solid (`carried_past`: Shadow length, on a Stage the
slab's reach). `vs_bent`/`fs_bent` splat each leg into the direct light and the air (before the background-light
copy; `fs_direct` then loads both): a signed width from how far its fan neighbours' same leg lies (`spread_from`, ×1.5,
of its own colour once split, at least Bent light softness, at most `MOST_SPREAD` 48 ×; a focus inside a leg is sharp), kernel
15/(16h)(1 − y²)², × falloff(s) × s for a lamp. Photon power is what the shadow dropped (Shadow strength × (1 − Shadow
colour)) × Glass caustics. Lighting card: **Bent light rays**, **Bent light softness**. `Renderer::bent_photons()`.
Tests `bpe-kit/tests/bent_light_pictures.rs`, bake `a_glass_lens_focuses_in_the_bake`. Known limits: no Lights up,
toon bands or normal turning on bent light; area/strip lights send from one point; the light field and the probe pass
glass straight.

Part 4a is in (plan `part4.md`, "As built (4a)" at its end): **mirrors throw light** and **light pipes**. A *thrower*
is a Shape, or a Balls/Emitter thing (Flat only), whose Surface (or a shape layer's own) has **Throws light** > 0
(`throws`; Chrome 1, Metal/Brushed 0.5, the new 15th chip **Mirror ball** 1 with **Facets** 120 and **Facet gaps**
0.12: `facets`, `facet_gaps`, `SurfaceGpu.f` = [throws, facets, gaps, 0]), whose layer blocks light and which doesn't
bend (`surface::throw_of`: a Mirror wall's tint × strength, else mix(F0, colour, Metal), × Throws light, none under
0.02; `thing_colour`). Shapes and balls push `bpe_gfx::Thrower`s (box, surface index, colour, `widen` = Roughness²,
facets, gaps, ball) into `LightScene.throwers` (`MOST_THROWERS` 64); a ball's sphere carries its turn in `SphereGpu.c[2]`.
The Lighting card's **Mirror light** (`mirror_light`, `LightingGpu.bent[2]`) scales it; 0 sends nothing. `BentPlan`
makes thrower pairs after bender pairs (`BentPair.b[2]` 1, `thrower_pairs`; Stage drops ball throwers), the bent
uniform carries `throws` bits and the `ThrowerGpu` table, and `cs_bent` reflects a photon meeting one (`thrower_at`,
`BENT_THROW` −16 − index; the normal from the sphere or the cover, faceted by Facets, ending in a gap; each photon is
its first glass's or mirror's pair's), × colour × Mirror light, its blur growing by `widen` along the path
(`Leg.d`, 64-byte legs, `vs_bent` widens). `u.bent.x` is set only when a bender pair runs (`BentPlan::bending`), so
mirror-only frames leave the direct pass alone. A faceted ball shows its tiles (`facet_normal`, `facet_gap` in
`surfaced`: rings from the middle out, gaps dimmed to 15%). On a Stage, mirror balls throw through the sphere caustic
pass (`caustic_glass` lists them, `CausticGlass.c[2]` 1, `vs_caustic` reflects). **Light pipes** (shapes, whole thing
only: **Light pipe** `light_pipe`, **Carries** `pipe_carries`, **Pipe glow** `pipe_glow`) push a `Pipe` into
`LightScene.pipes` (`MOST_PIPES` 16); `bpe-gfx/src/pipes.rs::LightScene::light_pipes` (called in `build_frame_sensing`
after the light hooks) catches lamps by falloff × cone and lasers ending on the outline, runs the light both ways
round, fading over Carries and leaking at corners past the critical angle, and pushes at most `MOST_PIPE_PIECES` 32
shadowless strip lights (`PIPE_GAIN` 4); never in the probe. Tests `bpe-kit/tests/mirror_light_pictures.rs`, bake
`a_mirror_throws_light_in_the_bake`.

Part 4b is in (plan `part4.md`, "As built (4b)" at its end): **dichroic glass, diamond fire, rainbows**. Surface
gains **Dichroic** (`dichroic`), **Dichroic shift**/**hue** (degrees, default 120/190) and **Fire** (`fire`); presets set
Dichroic and Fire to 0 (`SurfaceGpu.g` = [d, shift/360, hue/360, fire]; `bpe_gfx::dichroic::Dichroic` the Rust twin of
lighting.wgsl's `dichroic_hue/through/back`: hue `hue + shift (1 − |cos|)`, T = mix(1, hue, d), R = d (1 − hue)). It
acts in `shade` (F0 and a 0.35 share of the diffuse × the opposite hue, `through` × T), `transmitted`'s tint, Flat
shadows (`march` × T per see-through stretch through `dichroic_entering`, reading the main `Uniform.dichroic` list of
16, count `u.bent.y`, since fs_direct has no group 2), photons (`cs_bent` `dichroic_into`/`dichroic_off`), Stage sphere
caustics (`caustic_glass`, folded at `SPHERE_COS`) and lasers (`Meets::Bend { .., dichroic }`, `trace_split` splits at
dichroic glass into the colours × T plus `Colour { reflected: true }` × R). Fire: a spectral highlight in `shade` (3 to
7 wavelengths, tilted normals) and star glints after a hit (`balls/draw.rs::fire_glints`, `GLINT_SECONDS` 0.6, lit
frames only). The Lighting card's **Rainbow** group (Rainbow, Rainbow centre Opposite the sun / A point, Centre,
Rainbow size, Second bow, Dark band; `LightingGpu.rainbow/rainbow_at`, `GlowUniform` the same, `lighting::rainbow_centre`)
draws `rainbow_bow` in `lit_at` × the light there × what catches it: haze thickness plus the tags picture's B, written
by layers whose Lighting fold has **Makes rainbows** (`makes_rainbows` on AroundLit/ParticlesLit; packed into
`extra.z` by `bpe_gfx::pack_tags`, read by `tag_catch`/`tag_rain`; `LightScene.rain`). All off: byte-identical.
`tests/wild_glass_pictures.rs`.
