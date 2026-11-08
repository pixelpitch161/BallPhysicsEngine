# Glass and light part 4: wild glass and mirrors — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Spec:** `docs/superpowers/specs/2026-11-07-glass-light-design.md`, "Part 4", approved by Aedan 2026-11-07 18:34Z.
Builds on part 3's photon pass (`part3.md` and its "As built": `bpe-gfx/src/bent_light.rs`,
`shaders/bent_light.wgsl`), part 2's beams and part 1's tint.

The spec allows two halves. This part is split, each its own PR, built and merged in turn:

- **4a, mirrors** (this file, tasks below): mirrors and shiny things throw lamp light (the photon pass reflects as
  well as bends), the **Mirror ball** chip (Facets, Facet gaps; tiled shading, spots that turn with the ball, on a
  Stage too) and **light pipes**.
- **4b, colour tricks** (section at the end, planned in full before it is built): Dichroic, Diamond fire, Rainbows in
  rain.

## Global constraints (both halves)

- Draw-only: physics, sound and every golden (`crates/bpe-kit/tests/golden*.txt`, sound goldens) byte-identical.
- No Lighting card: byte-identical (`tests/unlit_pictures.rs`).
- **Nothing that throws, nothing changes.** A lit scene where no thing throws light (every Throws light 0, or Mirror
  light 0, or shadows Off) and with no light pipe draws exactly as before: `lit_flat_pinned`, `stage_pictures`,
  `thin_blockers`, `see_through_shadows`, `coloured_shadows`, `glass_pictures`, `lit_pictures`,
  `stage_camera_pictures`, `particle_light_pictures`, `liquid_pictures`, `bent_light_pictures`, `prism_pictures`,
  `bake_pictures` pass untouched. Test scenes that pick Chrome/Metal/Brushed through `preset_values` will now throw
  light: where such a test pins numbers it sets `throws` 0 (stated in the test) rather than loosening its tolerance.
- The direct pass is untouched by 4a (mirrors add light; they take none away), so `u.bent.x` (part 3's "bending is
  on") stays 0 in a frame whose photon pairs are all mirrors.
- New numbers `span(..)` and `keyframe`; a `range` only for hard limits named here.
- Storage buffers: the photon compute stage may see at most 8 (the device's default limit). 4a brings it to exactly
  8 (group 0: lights, segments; group 2: surfaces, spheres, cells, items; group 3: pairs, legs). The thrower table
  rides in the photon uniform, not a new buffer.
- No demo videos or screenshots. bpe-gfx tests with `--test-threads=2`.

## 4a decisions

1. **Who throws light.** A *thrower* is a Shape, or a Balls/Emitter thing (Flat only), whose Look Surface (or a shape
   layer's own Surface sub-layer) has **Throws light** above 0, whose layer blocks light, and which isn't glass that
   bends (part 3's `surface::bends`: glass bends, it doesn't throw). What it gives back of the light that meets it,
   per channel, is `surface::throw_of`:
   - with a Mirror wall on (shapes): Mirror tint × Mirror strength;
   - otherwise `mix(F0, colour, Metal)` (F0 from Bends light, as the shading's), `colour` the thing's colour (a
     shape's first Colour and gradient layer, else its Body's; a ball's Body colour at the moment: the probe's
     `layer_colour` rule, moved to `surface::thing_colour`);
   - times Throws light. A thrower whose brightest channel is under 0.02 is none (Plastic, F0 0.04, at Throws light
     0.5 throws nothing: no photons are spent on it).
2. **Settings** (`SurfaceSettings`, Surface group, every Look card with a Surface and the Surface sub-layer):
   - **Throws light** (`throws`, span 0..1, keyframe): how much lamp light it throws back onto the room, like a mirror.
     Presets: Chrome 1, Metal 0.5, Brushed 0.5, Mirror ball 1, every other 0 (spec). A Mirror wall throws only when
     this is above 0 (its help says so).
   - **Facets** (`facets`, span 0..400, keyframe; 0 smooth): a ball (or shape) made of this many flat mirror tiles
     round its middle. Mirror ball 120, others 0.
   - **Facet gaps** (`facet_gaps`, `when = "facets"`, span 0..1, keyframe): how much of each tile is dark gap between
     tiles. Mirror ball 0.12, others 0.
   - Whole thing only (`WHOLE_THING_ONLY`, shapes, `when` starts `thing=shape`): **Light pipe** (`light_pipe`, bool,
     default off), **Carries** (`pipe_carries`, when light pipe, range(1, 1e9), span(1, 2000), px, keyframe, default
     600), **Pipe glow** (`pipe_glow`, when light pipe, span 0..4, keyframe, default 1).
   - Lighting card, Quality group: **Mirror light** (`mirror_light`, span 0..2, keyframe, default 1): every
     mirror's thrown light times this; 0 sends no mirror photons. Into `LightingGpu.bent[2]`.
3. **Mirror ball chip** (`SurfaceKind::MirrorBall`, the 15th, after Prism): Chrome's numbers (Roughness 0.05, Metal 1,
   Bends light 1.5) with Throws light 1, Facets 120, Facet gaps 0.12. Its swatch is a ball of tiles
   (`style/swatch.rs`, arm 14).
4. **The photon pass reflects.** `LightScene.throwers: Vec<Thrower>` (bpe-gfx `frame.rs`: box `lo`/`hi`, `index`
   (surface index), `colour` (decision 1, rgb), `widen` (Roughness², decision 6), `facets`, `gaps`, `ball`), at most
   `MOST_THROWERS` 64 (`LightScene::push_thrower` drops the rest). `BentPlan::of` makes a pair for every (light,
   thrower) as for benders (`bent_pairs`, same reach rule), its `BentPair.b[2]` = 1 (0 a bender pair), when Mirror
   light isn't 0 and shadows aren't Off. On a Stage ball throwers make no pairs (balls aren't in the 2D picture there:
   decision 8). Bender pairs come first, then thrower pairs; the photon budget is shared by all.
   `BentUniform` gains `throws: [[u32; 4]; 16]` (bit per thrower index, as `bend_bits`) and `table: [ThrowerGpu; 64]`
   (`a` = [index, ball 0/1, facets, gaps], `b` = [colour rgb, widen]); `counts[3]` = how many; a new
   `BentUniform.extra: [f32; 4]` = [Mirror light, 0, 0, 0].
5. **A photon meeting a mirror** (`cs_bent`): `bent_at` checks, after glass and before solid, `thrower_at(ip)`
   (cover ≥ 0.5 and its surface code's bit in `bu.throws`) and returns `BENT_THROW - index` (`BENT_THROW` −16; every
   kind at or under it is a thrower). In the open (not in glass, not in a solid it lights), meeting a thrower:
   - its first glass or mirror is the photon's pair's: a thrower pair's photon that meets other glass or another
     thrower first stops (that pair carries it), as a bender pair's does; a bender pair's photon that meets a mirror
     before its glass stops. After that every mirror reflects (light between two mirrors), within `BENT_LEGS`.
   - The normal: a ball's from its sphere (`ball_at`, exact circle; the cover's slope if no sphere is found), a
     shape's from its cover's slope (`glass_normal`, now for any surface code), faced against the photon.
     With Facets ≥ 3 the normal's angle is rounded to the middle of its tile (`2π / Facets` wide, turned by the ball's
     turn, `SphereGpu.c[2]`; a shape's tiles start at angle 0); a photon landing in a tile's gap (the outer
     `gaps` share of the tile, `|2 frac − 1| > 1 − gaps`) ends there.
   - It reflects (`reflect`), from the last open point, its light × the thrower's colour × Mirror light; its path key
     takes the thrower and the tile (`path_key(key, 300 + tile % 256)`, so neighbours on another tile are not counted
     as neighbours); it is drawn from there on (`drawn`), and carried as far as the light reaches (a mirror's light is
     new light: the direct light took none of it away).
   - A thrower pair's photons start with the light as part 3's do but without Glass caustics (that multiplies bender
     pairs only).
6. **Roughness blurs** (Flat and the picture plane): a leg carries a **blur** at each end (`Leg.d` = [blur at start, at
   end, 0, 0]; `Leg` grows to 64 bytes, at most 2^17 × 8 × 64 = 64 MB). Each reflection adds the thrower's `widen`
   (Roughness², so Chrome 0.0025, Metal 0.12) to how fast the blur grows with path length; `vs_bent` adds the blur to
   the line's half width at each end (its kernel stays normalised, so a rough mirror's spot is wider and dimmer by the
   same light).
7. **Tiled shading** (the mix, `surfaced`): a ball pixel whose surface has Facets ≥ 3 and whose sphere is found
   (`SHADE_SPHERE`) takes its tile's normal (`facet_normal(n, facets, turn)`, a ball seen from its pole: rings
   `2π / Facets` wide from the middle out, each with `max(1, round(Facets × sin θ))` tiles, so the rim ring has Facets
   and matches the photons'); in a tile's gap the pixel is dimmed to 15% of what it was (the grout). Shapes keep their
   own normals (no tile shading on shapes; their photons are faceted).
8. **Stage mirror balls.** The sphere caustics (`caustic_glass`, `vs_caustic`) also take chrome spheres on a Stage:
   `caustic_glass` lists every sphere whose thing is a ball thrower (found by its surface index among
   `LightScene.throwers`), as `CausticGlass { a: [centre, radius, gaps], b: [throw colour rgb, turn], c: [0, 0, 1
   (a mirror), facets] }`. A mirror photon reflects off the (faceted) sphere normal at its first hit, × Mirror light, and lands
   on the wall or floor as glass photons do; one that lands in a gap or goes back toward the camera draws nothing. In
   Flat mirror spheres are not in the caustic pass (the 2D pass throws their light).
9. **Light pipes** (CPU, `bpe-gfx/src/pipes.rs`, after every thing's light hook in `build_frame_sensing`:
   `LightScene::light_pipes`). A shape with Light pipe on pushes a `Pipe { points (its closed outline, world), carries,
   glow, tint (its colour's hue, `c / max channel`), bends (its Bends light, at least 1.0001) }` into
   `LightScene.pipes` (at most `MOST_PIPES` 16). For each pipe:
   - the outline is cut into `n = clamp(perimeter / 8, 16, 256)` equal pieces;
   - light **enters** each piece from every light that isn't a laser or beam: `p1 × falloff(d, reach, kind) × spot
     cone × ds / (2π max(d, size + 4))` (the share of the lamp's light that piece meets, `d` from the lamp to the
     piece's middle); and from every laser or beam segment that ends within 6 px of the outline: the laser's `p1`
     there (all of it), at the nearest piece;
   - it **runs** both ways round the closed outline, half each way: over a piece it gives out
     `T (1 − e^(−ds / Carries))` and keeps the rest; at a corner whose turn `α` is sharper than the critical angle
     (`α > π/2 − asin(1 / Bends light)`) a share `min((α − crit) / (π/2), 1)` of what is travelling leaks out there;
     a run stops after two laps or when under 1e-4 of what entered;
   - what comes out of each piece, times the pipe's tint and Pipe glow × `PIPE_GAIN` (4: a lamp inside a ring pipe of
     radius 200 lights the ring's outside like a quarter of the lamp, pinned by a unit test), is grouped into at most
     `MOST_PIPE_PIECES` 32 runs of neighbouring pieces, each pushed as a strip light (`push_strip`'s fields; reach
     `4 × Carries` at most 1200, width the shape's band at least 4) of their summed light. Nothing comes out: nothing is
     pushed. Pipes never cast shadows (strip lights from them have shadows off) and send no photons.
   - The probe never sees pipes (it builds lights through `LightDraw::light` only).
10. **One-way sensing.** Thrown light and pipes are drawn only: never in the probe, never read by physics or sound.
11. **Known limits** (written into the As built): mirrors throw light on the picture plane (no height); a Stage's
    mirror *shapes* throw on the picture plane as part 3's glass does; text things don't throw; light reflected at
    glass edges is still dropped; a pipe's light enters by distance only (no shadows between lamp and pipe).

## 4a review focus

1. With no thrower and no pipe every new path is off: the pinned picture tests pass byte for byte.
2. A flat mirror's spot lands where the reflected geometry says; outside its cone nothing changes.
3. Photon ownership: no light is counted twice where a mirror and a glass share a lamp.
4. The compute shader never writes outside its `BENT_LEGS` slots; gap photons and photons sent away from the box end.
5. Pipe light is bounded by what entered (a closed lossless pipe gives out what it took in, never more).

## 4a tasks

### Task 1: settings and presets (bpe-kit `surface.rs`, `modules/lighting.rs`, `sensing/probe.rs`)

- [ ] Tests first (`surface.rs` tests): the Mirror ball preset (Throws light 1, Facets 120, gaps 0.12, Metal 1) and 15
  chips; `presets_set_every_number_but_the_ways_and_the_whole_things_own` still holds with the new keys (presets set
  `throws`, `facets`, `facet_gaps`; the pipe keys are whole-thing only); `throw_of`: Chrome white-ish × its colour,
  Matte none, Glass none (it bends), Plastic at 0.5 none, a Mirror wall Matte at Throws light 1 its tint × strength;
  `gpu().f == [throws, facets, gaps, 0]`. Lighting: `mirror_light` default 1 → `gpu().bent[2] == 1`.
- [ ] Implement; `cargo test -p bpe-kit --lib surface lighting probe` passes.

### Task 2: the renderer's data (bpe-gfx `frame.rs`, `bent_light.rs`)

- [ ] `SurfaceGpu.f` (WGSL struct too), `Thrower`, `MOST_THROWERS`, `LightScene.throwers`/`push_thrower`, `Pipe`,
  `MOST_PIPES`, `LightScene.pipes`, `SphereGpu.c[2]` = turn (doc), `Leg.d`, `BentUniform.throws/table/extra`,
  `ThrowerGpu`, `throw_table(&[Thrower]) -> ([[u32; 4]; 16], [ThrowerGpu; 64], u32)`, pair kinds in `bent_pairs`,
  `BentPlan` gains `throwers` and `bending: bool` (any bender pair runs: `u.bent.x`).
- [ ] Unit tests: thrower bits and table (index 0 never, past 2047 ignored, the 65th dropped), pairs (kind flag,
  benders first, Stage drops ball throwers, Mirror light 0 none, shadows Off none), `Leg` is 64 bytes, the WGSL's
  `BENT_THROW` and `MOST_THROWERS` agree.

### Task 3: the shaders (bpe-gfx `bent_light.wgsl`, `lighting.wgsl`, `lighting.rs`)

- [ ] `thrower_at`, `thrower_entry`, `BENT_THROW`, the reflection in `cs_bent` (decision 5), blur (decision 6) in
  `put_leg` and `vs_bent`; `glass_normal` for any code; `facet_normal` and the tile shading in `surfaced` (decision 7);
  the caustic mirror (decision 8: `CausticGlass` docs, `caustic_glass` takes mirror spheres on a Stage).
- [ ] `lighting.rs`: stage layout bindings 2, 3, 4 seen by the compute stage; `u.bent.x` from `plan.bending`; the
  bent uniform's new fields; `caustic_glass(scene)` reads the throw entries.
- [ ] `cargo test -p bpe-gfx --lib -- --test-threads=2` (sources validate, existing tests pass).

### Task 4: the kit lists throwers and pipes (shape draw, balls draw)

- [ ] `ShapeDraw::light`: throwers (the shape's surface and each own-surface layer that throws and whose layer blocks
  light; box as the benders' grown by 2 px); a `Pipe` when Light pipe is on. `push_ball_light`: one thrower per Balls
  (or Emitter) thing round its live balls' discs, and each sphere's turn (the Turning layer's
  `turn + spin × time + own variety`, radians; 0 without one).
- [ ] Unit tests (`things/shape` and `things/balls` tests): a Chrome shape lists one thrower with its index and
  colour, a Matte one none, a glass one none; a chrome Balls group one thrower whose box holds every ball; a Turning
  layer's turn reaches the spheres; Light pipe on lists a pipe with the outline.

### Task 5: pipes (bpe-gfx `pipes.rs`, `frame.rs` call)

- [ ] Unit tests: no pipes → no lights added; a lamp inside a ring pipe lights every run, all about the same; a laser
  ending on a square pipe makes the run where it entered brightest and the light falls with distance along the pipe
  (Carries 200 vs 2000); a square pipe's corners give out more than its sides' middles (Bends light 1.5: 90° corners
  are past the critical angle); total out ≤ total in × `PIPE_GAIN` × glow, and equal within 2% for a closed pipe with a
  long Carries and no corners; `PIPE_GAIN` pinned (decision 9).
- [ ] Implement; `build_frame_sensing` calls `lit.light_pipes()` after the light loop.

### Task 6: pictures (bpe-kit `tests/mirror_light_pictures.rs`, GPU; bpe-cli bake test)

Harness as `bent_light_pictures.rs` (270 × 480, Light only, linear light).

- [ ] `a_flat_mirror_throws_light_where_the_reflection_says` — a lamp at (−300, −400), a Chrome filled wall 300 × 40 at
  (0, 0): points inside the reflected cone (worked out in the test from the lamp's image) are brighter than with
  Throws light 0 by at least 30%; points outside it are within 2%.
- [ ] `nothing_throws_with_throws_light_or_mirror_light_zero` — Throws light 0, and Mirror light 0, each send no
  photons (`bent_photons() == 0`) and are byte-identical to the scene with a Matte wall.
- [ ] `rough_metal_throws_a_softer_spot` — the same wall as Metal (Roughness 0.35) at Throws light 1: its brightest
  thrown point is under Chrome's and its lit stretch across the cone is wider.
- [ ] `a_mirror_ball_throws_spots_that_turn_with_it` — a Mirror ball (Facets 24) lit from the side: round a circle
  of radius 300 on the lamp's side the thrown light has between 6 and 18 separate bright peaks; turning the ball by
  half a tile (a Turning layer) moves the peaks (the brightest point moves by at least 10 px).
- [ ] `a_mirror_ball_shows_its_tiles` — a lit Mirror ball (Picture, not Light only): along a line across it the
  brightness changes in steps (at least 4 gap dips under 50% of their neighbours).
- [ ] `a_stage_mirror_ball_throws_spots_on_the_wall` — on a Stage: the wall's brightest point is at least 2× the
  same wall with Throws light 0.
- [ ] `a_light_pipe_glows_along_its_outline` — a ring pipe (radius 200, band 12) with a lamp inside it and a far
  point outside it: the outside is brighter with Light pipe on than off, by at least 20%; and no lamp: nothing.
- [ ] bpe-cli `a_mirror_throws_light_in_the_bake` — the flat mirror baked at 4 samples: inside the cone ≥ 1.3 × Throws
  light 0.

### Task 7: editor (bpe-studio `style/swatch.rs`, surface chip tests)

- [ ] The Mirror ball swatch (a disc of tiles, `SWATCH_*` tokens only); the chip-count tests 14 → 15. The new numbers
  show under the chips as the others do (no new code); the Lighting card shows Mirror light in Quality.

### Task 8: finish 4a

- [ ] CLAUDE.md "Glass and light": part 4a is in, with the names above. "As built" notes for 4a below.
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx `--test-threads=2`), goldens in debug
  and release unchanged.
- [ ] One Opus review; fix; PR; merge when checks pass. Then plan 4b in full (below) and build it.

## 4b decisions (planned in full after 4a merged)

1. **Dichroic, one rule everywhere** (`bpe_gfx::dichroic`, `bpe-gfx/src/dichroic.rs`, with its WGSL twin
   `dichroic_hue`/`dichroic_through`/`dichroic_bounced` in lighting.wgsl, pinned against each other by a test). A
   surface's dichroic numbers are `[d, shift, hue]` (`SurfaceGpu.g` = [Dichroic, Dichroic shift in turns, Dichroic hue in
   turns, Fire]). Light meeting it at angle θ (cos θ = |dir · normal|, 1 straight on) takes the hue
   `h(θ) = hue + shift × (1 − cos θ)` (the hue wheel `hue_rgb(h)`: `clamp(|fract(h + k) × 6 − 3| − 1, 0, 1)` for
   k = 0, 2/3, 1/3); what goes through is `T = mix(1, hue_rgb(h), d)` and what bounces off is `R = d × (1 − hue_rgb(h))`,
   so T + R = 1 per channel at any d (a filter that splits light, losing none). Dichroic 0 gives T = 1, R = 0 exactly
   and every code path below multiplies by exactly 1 or is skipped, so nothing changes.
2. **Settings** (`SurfaceSettings`, Surface group, any preset, every Look card and the Surface sub-layer): **Dichroic**
   (`dichroic`, span 0..1, keyframe), **Dichroic shift** (`dichroic_shift`, when dichroic, span −360..360 °, keyframe,
   default 120), **Dichroic hue** (`dichroic_hue`, when dichroic, span 0..360 °, keyframe, default 190: cyan-blue through,
   amber off, the classic look), **Fire** (`fire`, span 0..1, keyframe). No new chips (the spec adds none). Presets
   set `dichroic` and `fire` to 0 and leave `dichroic_shift`/`dichroic_hue` alone (like the brush angle).
3. **Where dichroic acts:**
   - **The look** (`shade`): the shine's F0 × `mix(1, 1 − hue_rgb(h(nv)), d)` (so the reflections take the opposite
     hue), the diffuse × `mix(1, 1 − hue_rgb(h(nv)), 0.35 d)` (Pearl's sheen, by the angle to the eye), and the light
     through it (`through`) × T(nv). In Flat the behind picture seen through glass (`transmitted`'s tint) × T(nv).
   - **Part 1's coloured shadows** (Flat, `march`): where a ray goes into a see-through blocker it reads that
     pixel's surface (`surface_of`) and, when its Dichroic is above 0, the blocker's normal from the field's slope
     there (`field_normal`); the stretch's light × T(θ) when it comes out. (Stage slabs and spheres: a known limit.)
   - **Part 2's beams** (`laser.rs`): `Meets::Bend` gains `dichroic: [f32; 3]` (d, shift, hue in turns; from
     `surface::glass_of`). In `trace_split` the first glass a beam bends into with Rainbow split (and Colours > 1) **or**
     Dichroic above 0 is where it splits: the colours (one, white, when it doesn't split by colour) go in with their
     shares × T(θ), and, with Dichroic above 0, one more path, the **reflected beam** (`Colour { w: 0, share: R(θ),
     reflected: true }`), leaves by `reflect` and walks on. `trace` never splits (unchanged). Ribbons pair colours by
     index as before (the reflected one last).
   - **Part 3's photons** (`cs_bent`): going into bending glass whose surface has Dichroic above 0, power × T(θ) (θ
     from the photon's way and the glass normal); free glass has no surface (none).
   - **4a's thrown light**: a mirror whose surface has Dichroic above 0 throws × `mix(1, 1 − hue_rgb(h(θ)), d)` (its
     R normalised to its brightest, so a dichroic mirror throws the opposite hue at the same strength), in `cs_bent`
     and the Stage caustic mirror branch.
4. **Diamond fire, the look** (`shade`): with Fire above 0 (and not Toon), each lamp's highlight is the GGX lobe sampled
   at `3 + round(4 × Fire)` wavelengths (3 to 7), each with its normal turned across the light's plane by
   `w × 0.12 × Fire` (`w` −1 red to 1 violet, the tangent `normalize(cross(n, dir3))`, any other way when
   degenerate) and coloured by `spectral(w)`, normalised by the sum of the colours; the shine becomes
   `mix(plain, spectral × (1 + 2 Fire), Fire)`. One loop, one `ggx` call site.
5. **Diamond fire, star glints** (balls and emitters, drawn on the CPU, `balls/draw.rs::fire_glints`): when the scene
   is lit (`Frame.light` is set) and a ball's surface has Fire above 0, each ball hit within `GLINT_SECONDS` (0.6 s)
   adds `1 + round(2 × Fire)` star sprites (Particle shape 3, Glitter's star: added light, negative alpha) round its
   upper left (where the shine sits), placed by the ball's number and hits so far (so the same hit always glints in the
   same places), half size `radius × 0.45 × Fire × (0.6 + 0.4 strength)`, alpha `Fire × (1 − age / GLINT_SECONDS)²`,
   white times the ball's colour mixed half to white, into `Frame.particles_front`. Shapes: their hits' points
   (`shape_hits`, if they carry a point) glint the same way; if not, shapes have the spectral glints only (a known limit).
6. **Rainbows in rain** (the Lighting card's **Rainbow** group; `LightingGpu.rainbow` = [Rainbow, Rainbow size
   (px, the 42° bow's radius), Second bow, Dark band], `LightingGpu.rainbow_at` = [centre x, centre y, 1 when set by
   hand, 0]):
   - Settings: **Rainbow** (`rainbow`, span 0..2, keyframe, default 0), **Rainbow centre** (`rainbow_from`: Opposite the
     sun / A point), **Centre** (`rainbow_centre`, px pair, when a point, keyframe), **Rainbow size** (`rainbow_size`,
     span 100..2000 px, keyframe, default 700), **Second bow** (`second_bow`, span 0..1, keyframe, default 0.4), **Dark
     band** (`dark_band`, span 0..1, keyframe, default 0.5).
   - Opposite the sun: the renderer works the centre out (`lighting.rs::rainbow_centre`): the brightest Sun light's way
     (`p3.xy`, where its light goes) from the camera's middle, `1.3 × size` along it; with no sun, straight down.
   - Drawn in the mix (`lit_at`, after the haze): `bow(θ)` with θ = 42° × distance to the centre / size, per channel
     as the sum over 16 wavelengths of `spectral(w)` × the bow at that wavelength's angle (primary 40.6° violet to
     42.3° red, a soft peak σ 0.6° and a glow inside it; secondary 50.4° red to 53.6° violet, × Second bow × 0.43;
     between them × (1 − Dark band)), normalised by the summed colours; × Rainbow × the light there (direct + field, its
     luminance, at most 2) × what catches it: the haze (`air.w × density` when there is haze) plus the tags picture's
     B (rain), at most 1.
   - **Makes rainbows** (`AroundLit`, particle layers and the Particles thing's layers: `makes_rainbows`, span 0..1):
     packed with Catches beams in `extra.z` = `catches + 2 × round(makes × 255)` (`layers/lighting.rs::stamp`), so with
     0 it is exactly the old value; sprites.wgsl and trail.wgsl unpack it (`tag_catch`, `tag_rain`) and the tags pass
     writes `B = makes × cover`. `LightScene.rain` (any record that makes rainbows) and the Rainbow above 0 make the
     tags picture (`needs_tags`).
7. **One-way**: none of it reaches the probe, the physics or the sound (the probe's light stays as 4a left it).
8. **Known limits** (into the As built): dichroic shadows only in Flat; a beam splits once (a dichroic glass after the
   split, or after a rainbow split, tints nothing more); the light field passes dichroic glass as plain glass;
   swarm sprites don't make rainbows.

## 4b review focus

1. Dichroic 0, Fire 0 and Rainbow 0 leave every picture byte-identical (pinned tests), the laser paths unchanged
   (`laser.rs` tests), Makes rainbows 0 leaves `extra.z` as it was.
2. T + R = 1 for every θ and d; the WGSL and Rust hue wheels agree.
3. The beam split: segment caps still hold with the reflected path; ribbons still pair.
4. No new storage buffers in passes already at the limit; uniform layouts match.

## 4b tasks

1. **bpe-gfx data**: `SurfaceGpu.g` (+ WGSL), `dichroic.rs` (+ tests), `LightingGpu.rainbow/rainbow_at`,
   `GlowUniform.rainbow/rainbow_at` (+ WGSL `Glow`), `LightScene.rain`, `needs_tags`.
2. **Shaders**: the dichroic helpers; `shade` (look, fire); `transmitted`'s tint; `march`; `cs_bent` (into glass,
   throw); the caustic mirror; the rainbow in `lit_at`; sprites/trail tag unpacking.
3. **bpe-kit**: surface settings and `gpu()`, presets, `glass_of` → `Glass { index, rainbow, dichroic }`, `Meets::Bend`
   dichroic, `trace_split`'s split (+ `Colour.reflected`), `push_light`/ribbons with the reflected path; the Lighting
   card's Rainbow group; `AroundLit` Makes rainbows and the stamp packing, `LightScene.rain`; fire glints.
4. **Tests**: `surface.rs` (presets, gpu g), `laser.rs` (`a_dichroic_glass_splits_light_into_opposite_hues`: T + R = 1,
   hues opposite; Dichroic 0 is the old path), `dichroic.rs` units, `lighting.rs` (rainbow defaults, centre), kit GPU
   `tests/wild_glass_pictures.rs`: Dichroic 0 byte-identical to before for a glass room and a glass shadow; a dichroic
   pane's shadow light takes the hue and turns with the angle; a dichroic mirror throws the opposite hue; Fire 0 is
   byte-identical and Fire 1 splits a highlight into colours; star glints show after a hit and fade; Rainbow 0
   byte-identical; with haze, the bow's red ring lies outside its violet ring at the set size; rain that makes rainbows
   shows the bow where it falls and none where it doesn't.
5. **Editor**: the new numbers fold under the chips; Rainbow group in the Lighting card (no new code expected); swatch
   unchanged.
6. **Finish**: CLAUDE.md, As built (4b), fmt, clippy, tests, one Opus review, PR, merge.

## As built (4a)

- Settings, presets and the 15th chip as decisions 1 to 3. `surface::thing_colour(look)` uses the probe's
  `layer_colour` (now `pub(crate)`). `WHOLE_THING_ONLY` holds the three pipe keys.
- The renderer data as decision 4, with `BentUniform.extra` = [Mirror light, thrower count, 0, 0] and the thrower
  table in the uniform (not a storage buffer: the compute stage already uses all 8). `BentPlan::bending()` says
  whether any bender pair runs; `u.bent.x` follows it, so a frame with only mirrors leaves the direct pass's
  bent-light branches off. Bender pairs need Glass caustics, thrower pairs need Mirror light; both need shadows.
- `cs_bent` as decision 5. Ownership: a thrower pair's photon that meets glass, or another thrower first, stops; a
  bender pair's photon that meets a mirror before its glass stops; after the first, every mirror reflects. A photon
  steps a quarter pixel off the mirror after reflecting so it doesn't meet the same tile again.
- The blur (decision 6) is a private `BLUR` (blur so far, rate) carried along the photon and written per leg end.
- Tile shading (decision 7): `facet_normal` and `facet_gap` in lighting.wgsl, the grout at 15%.
- Stage mirror balls (decision 8) go through `caustic_glass`/`vs_caustic`; Glass caustics now gates only the glass
  entries inside `caustic_glass`, and `CausticU.counts[3]` carries Mirror light.
- Pipes (decision 9): the piece count is rounded up to a multiple of `MOST_PIPE_PIECES` so a ring's strips are alike;
  what is still travelling after `LAPS` (4) times round comes out evenly, so a pipe gives out exactly what it caught
  (× gain × glow). The corner rule is the physical one: a corner turning by `α` leaks when `α > π/2 − asin(1/n)`, so
  an index that hardly bends leaks at any corner and glass holds its light round 45° corners. Suns aren't caught.
- Review fixes: a mirror throws only light coming in (`dot(dir, n) < 0` with `n` outward), so a photon leaving
  through the mirror's soft edge after a throw goes on its way; a leg with no blur keeps part 3's widths exactly
  (`vs_bent` widens only when `leg.d` is above 0); Stage mirror balls need shadows not Off and throw only lights
  that cast shadows (as the 2D pass); a laser puts its light into a pipe once, where it first ends on it; a pipe's
  glow reaches 4 × Carries, at most `PIPE_REACH` 1200; the rim ring of a tiled ball has exactly Facets tiles; a
  test checks lighting.wgsl + bent_light.wgsl parse and validate (`the_light_pass_source_is_valid`).
- The test-only `stage_demo` and `surfaces_demo` scenes use Chrome/Metal/Brushed, so they now throw light (the
  look the spec asks for); their tests check thresholds and still pass.
- Known limits as decision 11, and: a ball's throw colour is its Body's paint at the moment (as its sphere's);
  mirror photons, like glass ones, carry what the shadow dropped (Shadow strength × (1 − Shadow colour)); when the
  photon cap leaves thrower pairs out, glass photons meeting those mirrors first still stop; a Lit 0 layer over a
  mirror still counts as the mirror (as part 3's glass); `ball_at` gives the nearest sphere of any thing.

## As built (4b)

- Data as decisions 1 to 6: `SurfaceGpu.g` = [Dichroic, shift/360, hue/360, Fire]; `bpe_gfx::dichroic` (`Dichroic`,
  `hue_rgb`, `through`, `bounced`, `back`, `SPHERE_COS`) is the Rust twin of lighting.wgsl's `dichroic_hue`,
  `dichroic_through` and `dichroic_back` (a test pins the wheel's text). `SurfaceSettings::dichroic()` is all 0 when
  the surface isn't dichroic; `Glass` (lasers) carries it.
- **Shadows** (`march`, Flat): fs_direct has no group 2, so the main lighting `Uniform` carries the first
  `MOST_DICHROIC` (16) dichroic surfaces as `dichroic` ([index, Dichroic, shift, hue], `lighting::dichroic_list`) with
  their count in `u.bent.y`; `dichroic_entering` reads the surface code under the point where a ray goes into a
  see-through stretch and the normal from the field's slope, and the stretch's light is × T when it comes out (or
  runs out of steps). No dichroic surface: the count is 0 and the function returns 1 at once.
- **Photons** (`cs_bent`): `dichroic_into` × T going into bending glass (and straight from glass into other glass),
  `dichroic_off` × `mix(1, 1 − hue, d)` on a mirror throw. **Stage sphere caustics**: `caustic_glass` folds T (glass) or
  the throw colour (mirror balls) in on the CPU at `SPHERE_COS` (2/3, the mean |cos| over a sphere's face).
- **Lasers**: `Meets::Bend { index, rainbow, dichroic }` (`Meets::bend(index, rainbow)` for plain glass);
  `walk_on(.., split, by_colour, ..)`; `trace_split` splits at the first glass with Rainbow split (Colours > 1) or
  Dichroic: the colours (or one white path) × T, then the reflected `Colour { reflected: true }` with share R; each
  path an even share of the segments left.
- **Fire** as decision 4 in `shade`; **star glints** as decision 5 in `balls/draw.rs::fire_glints` (called from the
  balls' `draw`, so emitters too), placed by a hash of the thing, the row and the hit's step.
- **Rainbow** as decision 6: `rainbow_bow(p)` in lighting.wgsl (16 wavelengths, `BOW_COLOURS`), added in `lit_at`
  after the haze. The haze catches it by its thickness (the strongest channel of Haze × Haze colour, times the
  drifting density), not `air.w` (that is Haze dims, 0 by default). `GlowUniform.rainbow_at.z` is 1 when rain makes
  rainbows and the tags picture is drawn.
- **Makes rainbows** deviates from decision 6's packing: Catches beams' span goes to 2 (and isn't a hard range), so
  `catches + 2 × makes` couldn't be told apart. `bpe_gfx::pack_tags` keeps `extra.z` = Catches beams exactly when
  Makes rainbows is 0, else `−(round(makes × 255) + 256 × round(catches × 64))`; `tag_catch`/`tag_rain` in scene.wgsl
  (used by trails and layers.wgsl, whose mini-ball records carry it too) and `sprite_tag_catch`/`sprite_tag_rain` in
  sprites.wgsl read it back; `around::more_in` sets `LightScene.smoke` and `.rain` from the parts.
- Tests: `bpe-gfx` `dichroic::tests` (wheel, T + R = 1, packing), `lighting::grid_tests` (rainbow centre, dichroic
  list); `bpe-kit` laser tests (dichroic split, a dichroic prism), `diamond_fire_flashes_star_glints_after_a_hit_that_fade`,
  `tests/wild_glass_pictures.rs` (Dichroic 0/Fire 0/Rainbow 0 unchanged, a dichroic wall's blue shadow, bending
  dichroic glass carrying red light, a dichroic ball's red shine, Fire colouring a highlight, the bow red outside
  violet at its size in haze, rain that makes rainbows).
- Review fixes: star glints ask the scene for its Lighting card (a ball drawn into a layer frame for its Z-order or
  Blend has no `frame.light`); `shines` counts Dichroic, so a Matte dichroic surface takes its sheen in Flat; the
  rainbow's outer glow fades out from 60° to the 75° cut; Fire and the plain highlight share one `ggx` call site (the
  plain one the loop's last); `dichroic_list` takes only see-through surfaces that don't bend; the light probe's
  `LightScene.plain_glass` makes lasers pass dichroic glass as plain glass (decision 7); a test checks a dichroic
  mirror throwing the opposite hue; "Rainbow" is a settings section (`SECTION_ORDER`).
- Known limits as decision 8, and: lasers don't take a dichroic mirror's colour, and dichroic glass that doesn't bend
  (Bends light 1) doesn't split a laser (only see-through glass that bends is `Bend`); a rainbow-split glass after a
  dichroic split doesn't split by colour again; the dichroic list holds 16 surfaces (later ones shadow as plain
  see-through); free glass (bubbles, liquid) isn't dichroic; shapes take no star glints (Fire's spectral highlight
  only); with Makes rainbows on, Catches beams is kept to 1/64.
