# Lighting part 2b: finishing Stage 3D (advanced surfaces, reflections, glass, mirror walls, depth of field)

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), built on part 2a (`part2a.md`, merged as
#109). Built straight after this plan (no approval stops), one Opus review at the end, then the PR, merged by the
thread itself once tests and the review pass (owner's standing instruction for the overnight lighting run,
2026-11-06). A short video and stills go to project files `ballphysics/lighting/part2b/`. No exe is installed on
the owner's PC until every lighting part is in.

## What 2b delivers

- **Advanced surfaces**: Jelly (and wax), Pearl (and soap bubble), Flake paint, Brushed metal, Frosted glass,
  Velvet, Lava: new chips in the Surface section, each a preset of the same numbers, plus the new numbers
  **Rainbow split**, **Soak**, **Film**, **Flakes** and **Flake size**, **Brushing**, **Brush lines** (Spun /
  Straight) and **Brush angle**, **Frost**, **Rim**, **Inner glow**, and (shapes) **Lens**.
- **Surface sub-layer** (`sub.surface` on ball layers, `shape.sub.surface` on shape layers): gives one layer (and
  its sub-layers) a surface of its own.
- **Mirror bounces** (live reflections): Reflects gains **The scene** (now the default): a shiny surface's mirror
  ray is traced against every ball (sphere), and in Stage the back wall, the floor and the shapes' fronts, up to
  **Reflection depth** bounces (Lighting card, Quality), in Flat and in Stage.
- **Glass refraction and lenses**: a see-through ball or shape shows what is behind it, bent by its Bends light
  through its height (balls: the sphere; shapes: **Lens**), split into colours by Rainbow split, blurred by Frost,
  tinted by its colour.
- **Mirror walls**: a shape's outline can be a mirror (**Mirror wall**: Off / Outside / Inside / Both sides,
  **Mirror strength**, **Squeeze**): the far side shows this side reflected; Squeeze 1 on a circle is an exact
  circle inversion (the "endless mirror world").
- **Live depth of field** (Stage): **Depth of field** on the Stage section with **Focus at** (depth) and **Blur**,
  soft round bokeh in front of and behind the focus.
- **Lasers through glass**: `surface::glass_of` for part 4's laser tracer (`Meets::Bend { index }`).
- **Part 2a's gaps**: shapes' oblique footprint, Stage balls blocking the light field, a glowing background on
  Stage and shiny pixels, wide penumbras, a black Lit 0 layer on a shiny ball.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **Numbers, not new pipelines, for the advanced surfaces.** Every advanced surface is the same shader path with
   more numbers. `SurfaceGpu` grows from two vec4s to five (`c`, `d`, `e`). Presets (roughness, metal, clear coat,
   bends light, see-through, toon steps, rainbow, soak, film, flakes, flake size, brushing, frost, rim, inner glow):
   - Matte, Plastic, Chrome, Metal, Glass, Toon: as 2a, every new number 0 (Glass also Rainbow 0.3; flake size 0.12
     everywhere as the size to use once Flakes is raised).
   - **Jelly** (0.35, 0, 0.3, 1.4, 0.35, 0, 0, 0.8, …): soaks light and lets some through (coloured shadows).
   - **Pearl** (0.2, 0, 0.6, 1.6, 0, 0, 0, 0.1, film 0.55): thin-film colours over a soft sheen. A soap bubble is
     Pearl with See-through raised (stated in the Pearl help).
   - **Flake** (0.4, 0.3, 1.0, 1.5, 0, 0, 0, 0, 0, flakes 0.7): a clear coat over a coloured base with sparkles.
   - **Brushed** (0.3, 1, 0, 1.5, …, brushing 0.8, Spun): a stretched highlight.
   - **Frosted** (0.45, 0, 0, 1.5, 0.85, …, frost 0.5): glass that blurs what is behind it.
   - **Velvet** (1, 0, 0, 1.0, …, rim 1.2): no shine, bright rims.
   - **Lava** (0.6, 0, 0, 1.5, …, soak 0.3, inner glow 1.5): glows from its middle.
   Brush lines and Brush angle are not part of the presets (like Reflects and Bulge).
2. **What each number does** (lighting.wgsl `shade`/`mixed`):
   - **Soak**: wrapped diffuse `(n·l + s)/(1 + s)`, plus light that soaks through from behind (`max(−n·l, 0)`,
     unshadowed by the ball itself) tinted by the colour; in the mix `soak × through × colour` is added.
   - **Film**: thin-film interference as a colour on the Fresnel: `0.5 + 0.5 cos(2π(film·4·(1.2 − n·v) + (0, ⅓,
     ⅔)))`, multiplying F0 and the rim of the sky/scene reflection (strength rises with Film from 0).
   - **Flakes**: the surface is cut into cells of Flake size (a ball's radius × size, in the ball's own frame from its
     sphere; shapes in world cells of 24 px × size × 8); a share of cells (Flakes) get a tilted normal (hash) and a
     sharp highlight (roughness 0.08) mixed toward the colour by half.
   - **Brushing**: anisotropic GGX (Burley's) with the tangent round the ball's middle (Spun) or along Brush angle
     (Straight), the highlight stretched by `1 + 6 × brushing` along the lines; faint lines (a hash of the distance
     from the middle or across the lines) shade the highlight by ±15 % × brushing.
   - **Frost**: the refracted picture is blurred over `frost × (16 px + the ball's radius)` with 12 taps (Vogel
     disc), and roughness under it is unchanged.
   - **Rim**: `rim × (1 − n·v)³ × through` added to the diffuse (velvet sheen from every light, unshadowed by the
     surface's turning).
   - **Inner glow**: `colour × inner glow × (0.35 + 0.65 (n·v)²)` added on top (a lamp's own glow, not light for
     others: give the layer Gives off light for that, stated in the help).
   - **Rainbow split**: refraction samples red, green and blue with Bends light × (1 − r·0.04, 1, 1 + r·0.04).
3. **Surface sub-layer.** A sub-layer kind with the Surface settings (all but Bulge and Lens: a layer has no height
   of its own). It writes no record. Its layer's record and the layer's sub-layer records carry its own surface
   index. Indices: things keep 2a's (1-based by Look-card order, `surface_index`); sub-layer surfaces follow, in
   thing order then layer order (`layer_surface_index(scene, thing, layer)`), so the draw and the light hook agree
   without shared state. Several Surface sub-layers on one layer: the last one shown wins. Picking a Surface on the
   sub-layer writes its preset numbers in the same edit, like the section (`Edit::SetLayer` with the presets).
   Not on the Body (the Body's surface is the card's) and not on around layers (sheets).
4. **Reflects: The scene** (code 2, new default; codes: 0 Nothing, 1 A sky gradient, 2 The scene). The mirror ray
   from the shaded point is traced (`trace_scene`): spheres (every ball, Flat and Stage; walking the sphere grid),
   in Stage also the back wall (z = −Back wall), the floor and the shapes' fronts (z = front, where the blockers'
   field is inside). What it meets:
   - a ball: its colour is read from the picture where the hit point shows (x, y), or the ball's Body colour for a
     point on its back half (n.z < 0), lit by `shade` at the hit point with the ball's own surface (without
     shadows: `NO_SHADOWS`, so a reflection costs one lighting loop) and mixed as that ball's pixel would be. A ball
     whose surface also reflects the scene sends the ray on (Reflection depth, default 3, `range(1, …)`, span 1..6),
     the colour carried multiplied by each mirror's Fresnel colour; the last bounce takes the sky;
   - the wall, floor or a shape front: the picture where that point shows (wall: (x, y − D·k)), lit as the stage is
     lit there (`shade` with the plane's surface, no shadows);
   - nothing: the sky, as 2a.
   The reflection replaces 2a's sky term by what was found, with the same Fresnel and roughness blur (roughness >
   0.3 keeps the sky: a blurred mirror of balls would need many rays; stated). Balls are pushed as spheres in Flat
   too (the Flat light pass still never shadows by them).
5. **Glass refraction.** A **behind** picture: when any surface in the frame is see-through (`LightScene.refracts`),
   the scene is drawn once more like the plain picture (`Globals.light.x` 0) with `Globals.light.w` = 1: records
   whose surface index is flagged see-through (`light[3] ≥ GLASS_FLAG` 4096: the kit adds the flag; the shaders
   read `index % 4096` everywhere else) draw nothing, so the behind picture is the scene without its glass. Effects
   don't run in it (as in every light pass). In the mix, a see-through pixel: the view ray is bent by the surface's
   normal (`refract(−v, n, 1/ior)`), and travels the thing's thickness (twice its height plus 8 px) to the plane
   behind it: `q = p + d.xy/max(−d.z, 0.15) × thickness`. The behind picture at q (three taps with Rainbow split,
   blurred by Frost) is lit as an uncovered pixel there is (Flat: ambient + the direct light picture + the light
   field at q; Stage: `shade` of the wall or floor point q shows, no shadows), tinted by the thing's colour
   normalised to its brightest channel (a white glass is clear), then mixed over by See-through:
   `out = mixed_opaque × (1 − see) + see × transmitted + highlights` (the opaque part loses its see-through
   handling; 2a's tinted lighter shadows stay). Without a behind picture (no see-through surface) nothing changes.
6. **Lens** (shapes): a shape's body record carries Lens in `light[2]`; its walk gives the surface picture a height
   `lens × scale × √(1 − (1 − t)²)`, t = depth inside the outline in shape units (0 at the edge, 1 a unit in), so a
   circle with Lens 1 is a half-sphere lens (its normals bend what is behind it and catch highlights); Lens 0 keeps
   shapes flat (2a's pictures). The inspector shows Lens only on shapes and Bulge only on balls.
7. **Mirror walls** (shapes, Flat and Stage, needs a Lighting card): `ShapeMirror` settings in the shape's Surface
   section: Mirror wall (Off / Outside / Inside / Both sides), Mirror strength (default 0.85), Squeeze (default 1),
   Mirror tint (colour, default white). The shape's light hook pushes a `MirrorGpu` (its outline as segments in
   `LightScene.segments` (shared with strip lights and lasers: each names its own range), which side, strength,
   squeeze, tint, its half-size S). The mix: a pixel on a mirrored side of a mirror within S × 4 of it finds the
   nearest point q on the outline (and inside/outside by crossings), d = |p − q|, the depth inside
   `d' = d / (1 + squeeze × d / S)` (squeeze 1 on a circle of radius S is exactly circle inversion), and the mirrored
   point `p' = q + (q − p)/d × d'`; it then lights pixel p' instead (the whole mix at p'), dimmed by strength ×
   tint over its own light (strength 1 shows only the mirror). Mirrors don't reflect each other (one mirror per
   pixel: the nearest). At most 16 mirrors a frame (`MOST_MIRRORS`), 512 segments each.
8. **Depth of field** (Stage only): Stage section rows **Depth of field** (switch), **Focus at** (z, px, default 0:
   the balls' middles; span −480..480), **Blur** (how many px of blur a px of depth away from the focus gives,
   default 0.04, span 0..0.2). A pass after the glossy floor (`fs_dof`): each pixel's depth (a ball's height, a
   shape's front, or the wall/floor point it shows), its blur circle `min(|z − focus| × blur, 32 px)`, and a
   24-tap gather that takes a tap only where the tap's own circle reaches this pixel (so a sharp thing in front
   isn't smeared by a blurred wall behind it), bright taps weighted up (bokeh).
9. **Gaps from 2a:**
   - **Oblique footprint**: the shapes' slab is sheared so its outline at depth z sits at `y − (z − front)·k`; the
     back of a shape then shows exactly under its front, and its shadow and contact shading on the wall start at its
     drawn foot. `field_on_slab(xy, z)` reads the field there.
   - **Stage balls in the light field**: in Stage a ball's height counts as blocking in the light field's scene
     (`fs_gi_scene`) and at the pixel (`blocks_here`), so glowing things' light stops at balls (`GlowUniform.on.w` =
     Stage).
   - **Glowing background**: the background's own light is drawn into a picture of its own (`bg_glow`), the direct
     light pass draws the lamps alone, and the mix adds `bg_glow` where 1b added it inside the direct light (Flat,
     uncovered pixels) and on the stage's wall and floor and shiny pixels (where 2a left it out). Flat pictures stay
     the same (the same sum).
   - **Wide penumbras**: with at most 128 balls the shadow ray tests every ball (no grid clipping at all); past that
     the grid lists balls out to 3 radii (`SPHERE_MARGIN` 2 → 3).
   - **Black Lit 0 layers**: a layer with Lit 0 writes surface index 0 (a plain sheet) in the surface picture, so a
     black Lit 0 layer over a shiny ball takes no highlight.
10. **Lasers through glass**: `surface::glass_of(look) -> Option<f32>`: Some(Bends light) when the card's Surface
    has See-through ≥ 0.5 and Bends light > 1. Part 4's tracer answers a ball or shape whose Look is glass with
    `Meets::Bend { index }` (wired after part 4 is on main; if part 4 is not yet on main when 2b merges, the
    function is there for it and part 4 wires it).
11. **Defaults and names.** Surface chips are named Matte, Plastic, Chrome, Metal, Glass, Toon, Jelly, Pearl, Flake,
    Brushed, Frosted, Velvet, Lava. New Surface defaults: Rainbow split 0, Soak 0, Film 0, Flakes 0, Flake size 0.12,
    Brushing 0, Brush lines Spun, Brush angle 0°, Frost 0, Rim 0, Inner glow 0, Lens 0, Reflects The scene. All
    are `span`s (no limits), but Flake size `range(0.005, …)` (a cell must have size) and Reflection depth
    `range(1, …)`.
12. **No card, Flat Matte**: untouched. `unlit_pictures` and `lit_flat_pinned` must pass unchanged.
13. **Deferred** (named so nothing is dropped): any Surface on the floor and wall (still Matte / Glossy / Mirror),
    a mirror back wall, blurred mirror reflections of balls (rough > 0.3 keeps the sky), "Shows in reflections" per
    layer (reflections read the picture), refraction of glass seen in reflections (shown with its picture colour),
    lit sheets (trails, particles) on the picture plane in Stage, rotating flakes with a ball's turn (balls have no
    drawn turn in the light pass), mirror walls reflecting each other, light focused by glass (caustics: part 3).

## Files and types

### bpe-gfx
- `frame.rs`: `SurfaceGpu` gains `c: [rainbow, soak, film, flakes]`, `d: [flake size, brushing, brush lines (0 spun
  / 1 straight), brush angle (rad)]`, `e: [frost, rim, inner glow, bends light]`; `SurfaceGpu::GLASS_FLAG` (4096.0);
  `SphereGpu` gains `c: [surface index, 0, 0, 0]`; `MirrorGpu { a: [first segment, count, side (1 outside, 2 inside,
  3 both), strength], b: [squeeze, size S, 0, 0], c: [tint rgb, 0] }`; `LightScene.mirrors: Vec<MirrorGpu>`,
  `LightScene.refracts: bool`; `LightingGpu.dof: [on, focus, blur, reflection depth]`.
- `lib.rs`: the behind pass (`light_globals` slot for "mode 0 behind" with `light.w` 1, `self.light_mode` 7 so no
  effects run) into `Lighting::behind()` when `scene.refracts`; the background glow into `Lighting::bg_glow()`.
- `lighting.rs`: stage layout gains binding 5 (mirrors), 6 (behind picture), 7 (background glow); the `behind`,
  `bg_glow` targets (made when first needed); `fs_dof` pipeline and a second stage target; the post chain (mix →
  floor? → dof? → out); spheres uploaded in Flat too; `SPHERE_MARGIN` 3; `GlowUniform.on.w` Stage; `StageUniform`
  gains `dof` and `counts` (mirrors).
- `shaders/lighting.wgsl`: `field_on_slab`, sheared `slab_visibility`/contact; `sphere_shadow` brute force for few
  balls; `shade` (soak, film, flakes, brushing, rim, `NO_SHADOWS`), `ggx_aniso`, `trace_scene`, `refracted`,
  `mirrored`, `lit_pixel` (fs_mix's body), `fs_dof`; background glow added in both paths.
- `shaders/layers.wgsl`, `layers_walk.wgsl`, `shape_walk.wgsl`: `light[3] % 4096` as the index; Lit 0 → index 0;
  behind pass skips glass records; shapes' Lens height.

### bpe-kit
- `modules/look/surface.rs`: new kinds and numbers, `Preset`, `BrushLines`, `Reflects::TheScene`, `ShapeMirror`
  (`MirrorSide`), `glass_of`, `layer_surface_index`, `sub_defs`, `SUB_SURFACE`/`SHAPE_SUB_SURFACE`, `stamp_surfaces`
  (sets each record's index and glass flag by layer).
- `modules/look/layers/mod.rs`, `gpu.rs`, `shape/mod.rs`, `shape/gpu.rs`: the sub-layer kinds (no record), the
  shape body record's `light[2]` = Lens.
- `things/balls/draw.rs`, `things/emitter/draw.rs`, `things/shape/draw.rs`: indices with flags, sub-layer surfaces
  in the table, spheres in Flat with their surface index, `refracts`, mirrors.
- `modules/lighting.rs`: Reflection depth (Quality), Depth of field / Focus at / Blur (Stage), `gpu()` fills `dof`.
- `scenes.rs` (test-only): `glass_room`, `mirror_room`, `surfaces_demo` for tests and the video.

### bpe-studio
- `inspector.rs`: the new chips (13), Lens only on shapes, the Mirror wall rows on shapes; a `sub.surface` layer's
  Surface picker writes the preset.
- `style/swatch.rs`: swatches for the seven new surfaces (own tokens).

## Tests (what passing looks like)

- `unlit_pictures`, `lit_flat_pinned` unchanged.
- `tests/stage_pictures.rs` (2a's) pass; new `tests/glass_pictures.rs`:
  - a glass ball over a striped background shows the stripes inside it (bright and dark pixels inside, not one dark
    disc), and a red ball behind a clear glass ball shows red through it;
  - a chrome ball beside a red ball is redder on the side facing it than on the far side (Flat and Stage);
  - a mirror circle (Outside, strength 1) shows a ball inside it outside its edge (the red found outside);
  - depth of field: the wall's edge contrast drops with Blur on, a ball at the focus stays sharp;
  - a black Lit 0 layer on a chrome ball: no highlight there;
  - Stage: a glowing ball's light stops at a ball between it and the wall (darker behind it than beside it);
  - wide penumbra: a big lamp's shadow of a ball is soft past two radii;
  - advanced surfaces: Velvet's rim brighter than its middle; Lava glows in a dark room; Pearl's highlight
    coloured (channels differ); Flake has sparkles (many local maxima); Brushed's highlight longer than wide.
- Kit units: presets set every number but Reflects, Bulge, Lens, Brush lines/angle; Surface sub-layer indices
  follow the things', are stamped on its layer's records and match the table; `glass_of`; the glass flag.
- Physics and sound goldens unchanged (run both); clippy, fmt, editor kittests (chips named; colour test).

## Things not to do

- No lighting code runs and no pipeline changes when the scene has no Lighting card.
- Don't change Flat Matte pictures. Don't touch physics, sound or their goldens.
- No hard ranges for taste; no built-in scenes in the program; no colours outside `style/`; nothing kept between
  frames.

## As built (differences from the plan above)

- **Background glow**: not drawn twice. The background's own light is still drawn into the direct-light picture
  (so Flat is unchanged), then copied into `Targets.bg_glow` before the lamps' light is added over it. A fragment
  stage takes at most 16 pictures, so the mix reads it through group 1's `above` binding (only the cascades read
  `above` otherwise); the stage group gained bindings 5 (mirrors) and 6 (behind picture) only.
- **Behind picture**: drawn as light pass `BEHIND_PASS` (7) in the renderer: `Globals.light = [0, lit, stage, 1]`, so
  the walk's `left_behind` drops records carrying `SurfaceGpu::GLASS_FLAG`; no effects, plain blending.
- **The pass chain**: mix → glossy floor → depth of field, each into `Targets.stage`/`stage_more` and the last
  into the output; depth of field is `fs_dof` (24 Vogel taps, a tap counts where its own blur reaches the pixel,
  bright taps weigh more).
- **Reflections off the picture**: a reflected ray meeting the wall or floor off the picture takes the colour at
  the picture's edge (the wall goes on as it is there).
- **Brushed**: the highlight stretches across the lines (as on real brushed metal), not along them.
- **Velvet**: dims where it faces the eye as well as brightening its rim.
- **Flat speckle fix** (not planned): `visibility` starts 1.5 texels past the edge of the pixel's own blocker;
  just out of it the field is about 0 and half-shadowed big Flat balls' surfaces in speckles. This moves Flat
  lit pictures by up to 3/255 in a tile, so `golden-lit-flat*.txt` were re-blessed (release and debug).
- **Tests**: `tests/glass_pictures.rs` builds its rooms from `scenes::lit_room`/`glow_room` in the test (no
  `glass_room`/`mirror_room` in `scenes.rs`); `scenes::surfaces_demo` (test-only) is the video's scene. Not
  pictured: the black Lit 0 layer and the wide penumbra (both changed in the shader: Lit 0 layers write surface
  index 0; ≤ 128 balls test every sphere and the grid margin is 3 radii).
