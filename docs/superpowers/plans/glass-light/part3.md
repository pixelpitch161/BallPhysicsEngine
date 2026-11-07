# Glass and light part 3: every glass shape focuses light — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** lamp light through any glass shape, glass liquid or bending particle really bends: bright lines and dark
gaps behind lenses, rainbow rims with Rainbow split, live and in the Bake. Today only glass *balls* focus light (the
sphere caustics of lighting part 3).

**Architecture:** the direct light stops passing light straight through glass that bends (it counts as a solid
blocker there), and a new **2D photon pass** carries that light instead: a compute pass traces photons from every
light toward every bender's box through the blockers' field and the surface picture, refracting at each glass edge,
and writes each photon's straight pieces ("legs") to a buffer; a draw pass splats the legs as soft lines into two
half-size pictures (light on surfaces, light in the haze); a full-screen pass adds them into the direct light and the
air light before anything reads those. Everything downstream (the mix, shiny surfaces, the Stage, bounce light, the
light views) then sees the bent light exactly as it sees the sphere caustics today.

**Tech stack:** Rust, wgpu 30 / WGSL (bpe-gfx: `lighting.rs`, `shaders/lighting.wgsl`, new `bent_light.rs` and
`shaders/bent_light.wgsl`), the kit's draws and settings (bpe-kit).

**Spec:** `docs/superpowers/specs/2026-11-07-glass-light-design.md`, "Part 3". Part 1's plan and its "As built"
(`part1.md`) describe the tint picture and the per-channel blockers' field this part reads.

## Global constraints

- Draw-only: physics, sound and every golden (`crates/bpe-kit/tests/golden*.txt`) stay byte-identical.
- No Lighting card: byte-identical (`tests/unlit_pictures.rs`).
- **Nothing bending, nothing changes:** a lit scene with no bender (no glass shape, no Glass liquid, no particle with
  Bends light above 1), or with Glass caustics 0, or with no light, runs no photon pass and draws exactly as before
  (`lit_flat_pinned`, `stage_pictures`, `thin_blockers`, `see_through_shadows`, `coloured_shadows`, `glass_pictures`,
  `lit_pictures`, `stage_camera_pictures`, `particle_light_pictures`, `liquid_pictures` and `bake_pictures` pass
  untouched). Every new shader branch is behind `u.bent.x > 0.5`.
- Balls keep the sphere caustics (Flat and Stage) and are never 2D benders: a ball's surface table entry has kind
  `BALL`, and only kit-listed benders and free glass count.
- New numbers are `span(..)` and `keyframe`; no new `range` but the hard ones named here.
- The direct and mix passes stay within 16 sampled textures and 8 storage buffers a stage (both are at their texture
  limit): this part adds no binding to either. Every bit the direct pass needs rides in the shared uniform `u`.

## Decisions (where this plan pins down or departs from the spec)

1. **What bends.** A *bender* is one of:
   - an **indexed** bender: a Shape thing whose Look Surface lets light through (See-through ≥ 0.05) and bends it
     (Bends light > 1.0001), or a shape layer whose own Surface sub-layer does; the kit lists it in
     `LightScene.benders` as `Bender { lo, hi, index }` (its world box and surface index);
   - **free glass**: a particle sprite whose layer's **Bends light** is above 1.0001, and a Particles **Liquid** layer
     with Surface Glass and Bends light above 1.0001. These have no surface table entry: in the surface picture (light
     pass 2) they write `B = −(Bends light) × cover` and `A = cover` (R stays how much they block), so a pixel's
     `B / A < −1.0001` says "free glass, this index of refraction" without any table. `surface_code` already clamps a
     negative code to 0 (matte), so how free glass looks is unchanged. The renderer makes one free bender whose box is
     the union of every free-glass sprite's and blob's extent (`Frame::free_glass_box`).
2. **Which pixels bend** (`bender_at(ip)` in lighting.wgsl): cover `A ≥ 0.5`, blocks `R < SOLID`, and either free glass
   (`B/A < −1.0001`, id `FREE_BENDER` 4096) or a surface code (rounded `B/A`, within 0.2, modulo 2048 so a Lit 0 glass
   still bends) whose bit is set in `u.bends` (2048 bits, `array<vec4<u32>, 16>`; set from the benders' indices by
   `bend_bits`, indices ≥ 2048 ignored). `bends_at(ip) = bender_at(ip) >= 0`. Everything is off unless `u.bent.x` is 1.
3. **The direct light through glass that bends** (`march`): a pixel on bending glass leaves it first (as a pixel on a
   blocker does: `leaves |= bends_at(ip)`); `confirm` also stops at a bending pixel; a crossing that `confirm` finds on
   a bending pixel, or a see-through stretch with a bending pixel in it, or clear coloured glass that bends, is a
   **solid** crossing (`lit = 0`). So the direct light (and its haze) behind bending glass is the shadow of a solid
   blocker, with the light's Shadow colour and Shadow strength as for any shadow. On a Stage `slab_visibility` takes a
   see-through slab sample whose nearest blocker bends (`nearest_bends`, through the jump flood's seeds) as solid.
4. **Bending glass is in the blockers' field** even when it blocks less than `BLOCK_LEAST` (clear glass): `foot_blocks`,
   `foot_most` and `foot_tint` count bending pixels. Their opacity is their own (tiny for clear glass), so readers of
   the field that don't know about bending (the light field) treat them as before (open glass, part 1's colour).
5. **Photons.** For each (light, bender) pair that can cast shadows (`p5.w > 0.5`, Shadow style not Off) photons leave
   the light:
   - point, spot: from the lamp; area and strip: from the point of the light nearest the bender box's middle
     (`source_point`), a known approximation; each photon's start is moved across the lamp by up to its soft-shadow size
     (`p2.z × Shadow softness × the light's softness`, Soft style only) so the photons' edges soften as the direct
     shadow's do; the photons fan stratified over the angle the box covers (`full turn` when the light is inside it);
     their weight is that angle over the photon count;
   - sun: parallel, from a line across the box before it, spaced over the box's width across the sun's way; weight
     that width over the count;
   - a spot's cone is applied by each photon's direction, a gobo where the photon first enters glass, the UV light's
     Visible share (`p7.z`), Shadow strength (`p2.w`), Glass caustics (a multiplier, as for the sphere caustics).
6. **A photon's way** (`cs_bent`): it steps through the field (jumps by the field distance outside blockers and deep
   inside them, half a picture pixel near edges), classifying each pixel: open, see-through (blocks at least
   `BLOCK_LEAST`), solid (at least `SOLID`, or on a Stage at least 0.3), or bending glass (`bender_at`). It
   - ends at a solid blocker before reaching glass, and at the first bending glass that isn't its pair's bender
     (each pair keeps only the photons whose first glass is its own, so overlapping boxes never count light twice);
   - refracts at every glass edge with the index from the surface table (`surfaces[i].e.w`, free glass from `−B/A`),
     reflects totally inside where it must, the edge's normal from the cover gradient of that bender's pixels over a
     5×5 window (Sobel; the field's gradient where that is flat), frosted glass jittering the new way by up to
     `frost × 0.5` radians;
   - takes a wavelength when it first meets glass with Rainbow split above 0 (stratified, −1 red to 1 violet, index
     `n (1 + rainbow × 0.04 w)` as the sphere caustics, coloured by `spectral(w)`), part 1's tint (`(1 − b) × colour`
     of each glass and see-through blocker crossed, from the picture's blocking and the field's colour);
   - is **drawn from where it first leaves glass**: on surfaces and inside later glass and see-through things, and
     through a solid blocker it then meets (lighting that blocker's own pixels, which leave their own blocker in the
     direct pass too) until it leaves it, where it ends; it also ends past the light's reach, past Shadow length beyond
     its last glass exit (the direct light stops counting glass farther than that), off the picture, or after
     `BENT_LEGS` (6) legs or `8 × steps` steps.
7. **Splatting.** Each leg is a quad `h` either side (`h = max(3 × Bent light softness, 2) × world pixel`), drawn
   additively into the half-size `bent` and `bent_air` pictures (Rgba16Float) with the kernel
   `15/(16h) (1 − y²)²` across (integral 1) times the leg's power times, for a lamp, `falloff(s) × s` (`s` the path
   length from the light at that point), so for unbent glass the photons give exactly the light the direct pass
   dropped: a fan of `N` photons over angle `Δθ` is `N / (s Δθ)` photons per unit length at distance `s`. A sun's
   legs take its power as it is. The haze picture takes the same times the light's Beam in haze.
8. **Adding it in.** `fs_bent_add` (full screen) samples both half-size pictures and adds them into the direct light
   and the air light, before the background-light copy (so shiny surfaces and the Stage see it, as the sphere
   caustics) and before `fs_direct` (which then loads both pictures instead of clearing the air).
9. **Budgets.** Photons a frame: Draft 16384, Fast 32768, Good 65536, Ultra 131072, a baked sample 131072, times
   Bent light rays, at most `MOST_BENT_PHOTONS` 2^18 (the legs buffer is then 2^18 × 6 × 48 bytes, 75 MB, under the
   128 MB binding limit), shared out per pair, at least 256 a pair (pairs past that budget are dropped, farthest bender
   first is not attempted: they are in light and bender order, and the count covers all of them up to the cap). A
   baked sample turns the photon pattern (`frac(index × 0.618…)` added to every stratum), so samples average to a
   smooth picture.
10. **Settings.** Lighting card, Quality: **Bent light rays** (`bent_rays`, span 0..4, default 1, keyframe) and **Bent
    light softness** (`bent_softness`, span 0..4, default 1, keyframe), both shown when Glass caustics isn't 0; Glass
    caustics' help now says balls, shapes, liquid and bubbles. `LightingGpu.bent = [rays, softness, 0, 0]`.
    Particle layers' Lighting fold (`layer.particles`, `shape.particles`, and the Particles thing's Sprites, Streaks,
    More shapes and Glitter): **Bends light** (`bends_light`, span 1..2, default 1 = off, keyframe), read by
    `lighting::of` into `light[2]` and stamped on the sprites (`Particle.light[2]`). Liquid layer: **Bends light**
    (`bends_light`, Liquid group, when Glass, span 1..2, default 1.33, keyframe) into `LiquidStyle.c[2]`. The Bubbles
    look sets Bends light 1.33.
11. **The light probe** is unchanged (spec rule 5): glass keeps letting its share through there.
12. **Known limits** (written into the As built): light bent by glass doesn't take Lights up (linking), toon bands or
    the surface's normal turning; area and strip lights send their photons from one point; the light field (bounce and
    glow light) still passes glass straight; a ball behind glass is lit by photons only where they reach its outline
    pixels; free glass sprites count as covering in the surface picture (on a Stage they are then shaded as things).

## Review focus

1. With no bender every new branch is off (`u.bent.x == 0`): the pinned-picture tests must pass byte for byte.
2. Energy: glass with Bends light 1.001 behind a lamp must give the light part 1 gives (the photons replace exactly
   what the march drops), for a point lamp and a sun.
3. Overlapping benders (one glass behind another, boxes overlapping) must not double the light.
4. The compute shader must never write outside its own `BENT_LEGS` slots, and unused slots must be cleared (empty legs
   draw nothing), so the picture is deterministic.
5. The direct pass's march must not darken the inside of clear bending glass (a pixel on it leaves it first).

---

### Task 1: plan and settings plumbing (bpe-gfx frame.rs, bpe-kit settings)

**Files:** `crates/bpe-gfx/src/frame.rs` (`LightingGpu.bent`, `Bender`, `LightScene.benders`, `Particle` docs,
`LiquidStyle` docs, `Frame::free_glass_box`), `crates/bpe-kit/src/modules/lighting.rs`,
`crates/bpe-kit/src/modules/look/layers/lighting.rs` (`ParticlesLit`, `of`), `.../layers/mod.rs` (`light_defs`),
`.../layers/around.rs` (`Drew::stamp` writes `light[2]`), `crates/bpe-kit/src/things/particles/liquid.rs`,
`crates/bpe-kit/src/things/particles/looks.rs` (Bubbles).

- [ ] Tests first: `Frame::free_glass_box` none for a default frame, the union of a bending sprite's extent and a Glass
  liquid's blobs, ignoring sprites at Bends light ≤ 1.0001 and non-Glass liquids, and reading blend layers' frames;
  `LightingSettings` default `bent_rays`/`bent_softness` 1 → `gpu().bent == [1, 1, 0, 0]`; `lighting::of` reads
  `bends_light` into `[2]`; the Particles thing's Sprites layer offers `bends_light`, Soft smoke doesn't; a Glass
  liquid's style has `c[2] == 1.33`.
- [ ] Implement; `cargo test -p bpe-gfx --lib frame` and `cargo test -p bpe-kit --lib lighting liquid` pass.

### Task 2: the shaders know which pixels bend (lighting.wgsl, sprites.wgsl, liquid.wgsl)

- [ ] `Uniform` gains `bent: [f32; 4]` (on, 0, 0, 0) and `bends: [[u32; 4]; 16]`; `bend_bits(&[Bender]) -> [[u32; 4]; 16]`
  (unit test: index 0 never set, 5 → word 0 bit 5, 2047 → last bit, 2048 ignored).
- [ ] lighting.wgsl: `FREE_BENDER`, `bender_at`, `bends_at`, `nearest_bends`; seeding (`foot_blocks`, `foot_most`,
  `foot_tint`); `confirm`, `march` and `slab_visibility` per decision 3.
- [ ] sprites.wgsl: the sprite's `light` goes to the fragment as `vec3`; in pass 2 a sprite with `light.z > 1.0001`
  writes `(blocks × a, 0, −light.z × a, a)` with `a` its alpha. liquid.wgsl: in pass 2 Glass with `c[2] > 1.0001`
  writes `(shown / 1024, 0, −c[2] × shown, shown)`.
- [ ] `cargo test -p bpe-gfx --lib -- --test-threads=2` (sources validate).

### Task 3: the photon pass (bpe-gfx `bent_light.rs`, `shaders/bent_light.wgsl`, `lighting.rs`)

**Produces:** `pub struct Bender { pub lo: [f32; 2], pub hi: [f32; 2], pub index: u32 }` (frame.rs, `FREE_BENDER`),
`BenderGpu`, `Leg` (48 bytes), `BentUniform`, `pub fn bent_photons(quality: f32, pairs: usize, rays: f32) -> u32`,
`pub const MOST_BENT_PHOTONS: u32 = 1 << 18`, `BENT_LEGS = 6` (matched in WGSL by a test), the compute pipeline
`cs_bent`, the splat pipeline `vs_bent`/`fs_bent`, the add pipeline `fs_bent_add`, `Targets.bent`,
`Lighting::bent_photons()` and `Renderer::bent_photons()` (the last frame's photon count, 0 when the pass didn't run).

- [ ] bent_light.wgsl is pasted after lighting.wgsl (one module, so it shares `u`, the field, the surface picture, the
  lights and the surface table); its own bindings sit in group 3 at 4 (uniform), 5 (benders), 6 (legs, read-write,
  compute), 7 (legs, read, vertex), 8 and 9 (the two half-size pictures, add pass). The shared group 0 layout's
  bindings 0, 3, 4, 5, 6, 7 and the stage layout's 0 and 1 gain `COMPUTE` visibility.
- [ ] `Lighting::run`: benders = `scene.benders` + the free bender; `bent_on` = Glass caustics ≠ 0, a light casts
  shadows, Shadow style not Off, benders; base uniform `bent`/`bends`; after the field: compute, splat, add (into the
  direct picture: loading when the background glows or caustics drew, else clearing; the air picture cleared then
  added); `into_bg` and `glows_in_direct` include it; `fs_direct` loads the air picture when it ran.
- [ ] Unit tests: `bent_photons` budgets, cap and floor; `BENT_LEGS` and `FREE_BENDER` agree with the WGSL.

### Task 4: the kit lists benders (bpe-kit shape draw)

- [ ] `ShapeDraw::light`: a shape whose Surface lets through ≥ 0.05 and bends (> 1.0001) pushes `Bender` with its box
  (outline points ± half its band, + 2 px) and surface index; each own-surface layer that bends pushes one with its
  index and the same box. Unit test (`things/shape` tests): a Glass shape lists one bender with its index, a Matte
  see-through one none, a Glass ball none.

### Task 5: pictures (bpe-kit `tests/bent_light_pictures.rs`, bpe-gfx `bent_light_tests.rs`, bpe-cli bake test)

Harness as `see_through_shadows.rs` (270 × 480, Light only, linear light). Each test also checks
`Renderer::bent_photons()` where it matters.

- [ ] `nearly_straight_glass_gives_the_light_it_replaced` — a wall at See-through 0.85, Bends light 1.001: behind it the
  light with the photon pass is within 10% of the light with Glass caustics 0 (part 1's picture), at Fast and Good, a
  point lamp and a sun.
- [ ] `one_glass_behind_another_isnt_counted_twice` — two such walls, one behind the other: within 10% of Glass
  caustics 0.
- [ ] `a_convex_lens_focuses_and_a_concave_one_spreads` — a glass circle (radius 90, Bends light 1.5): the brightest
  tile behind it is at least 2.5 × the open light there; a biconcave lens (a path): the light behind its middle is
  under 0.8 × the open light.
- [ ] `rainbow_split_gives_coloured_rims` — the circle with Rainbow split 1: behind it some tile is red over blue by
  1.3× and another blue over red by 1.3×.
- [ ] `nothing_bending_runs_no_photons` — a Matte see-through wall, a glass ball, and a glass shape with Glass caustics
  0 each run 0 photons; the glass circle runs some.
- [ ] `a_stage_lens_focuses_on_the_wall` — the circle on a Stage: brightest tile behind ≥ 1.8 × open.
- [ ] gfx: `bubbles_bend_light` (hand-made frame: bending sprites in a lit frame run photons and focus a bright line
  behind), `a_glass_liquid_bends_light` (a Glass liquid blob does too; Plastic doesn't run any).
- [ ] bpe-cli: `a_glass_lens_focuses_in_the_bake` — the circle baked at 4 samples: brightest tile ≥ 2 × open.

### Task 6: finish

- [ ] CLAUDE.md "Glass and light": part 3 is in, with the names above.
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx `--test-threads=2`), goldens in debug
  and release unchanged.
- [ ] One Opus review; fix; PR; merge when checks pass; tell the coordinator.

## Not in this part

- Mirrors and shiny things throwing light (part 4 reuses this pass), prisms and beams (part 2).
- Glass balls as 2D benders (they keep the sphere caustics).
- Bent light in the probe, the light field, Lights up and the UV light.

## As built

Built as planned, with these changes found while building:

- **No half-size pictures, no add pass.** `vs_bent`/`fs_bent` splat the legs straight into the full-size direct light
  and air pictures (two attachments, added) before the background-light copy; `fs_direct` loads both. Simpler, and the
  photon counts below keep the fill small.
- **Fewer photons.** Each leg's line is as wide as its neighbours are apart, so far fewer photons give a smooth
  picture: Draft 4096, Fast 8192, Good 16384, Ultra and a baked sample 32768 a frame (× Bent light rays), at most
  `MOST_BENT_PHOTONS` 2^17, at least 256 a pair; `BENT_LEGS` is 8 (a pane behind a see-through thing behind glass
  needs more than 6).
- **Fans, and widths from neighbours.** With soft shadows a pair's photons leave from up to 16 points across the lamp
  (`lamp_copies`), each point a fan of at least 256; a leg's half width at each end is 1.5 × how far the same leg of
  its neighbours in the fan lies from it (`spread_from`; the neighbour counts only when its light and *path key*,
  a number for the order of things it met, are the same), at least Bent light softness's width and at most 48 times
  that. Without this, light spread out by a lens showed as separate streaks.
- **Fresnel.** Each glass edge lets through `1 − F` (exact unpolarised Fresnel, `passes_edge`), so light grazing a
  lens's rim mostly goes, as it does in life (it showed as bright streaks before), and an edge that hardly changes
  the index reflects nothing (Schlick's lost light there at a slant; found in review).
- **A glass's soft edge is open** to a photon (`bender_covering(ip, 0.02)`): its antialiased edge pixels block a
  little and would otherwise tint the photons that crossed them twice, in stripes.
- **Shadow colour.** A photon carries Shadow strength × (1 − Shadow colour) of the light, so with a coloured shadow
  the total is still what part 1 gave.
- **Sun photons** start a Shadow length (at most 4096) before the box, so what shades the glass on the way is met.
- **What doesn't block doesn't bend.** A shape (or own-surface layer) whose layer doesn't block light lists no bender.
- Photons that are outside the box and going away from it stop at once.
- **Free glass has a picture of its own** (found in review). Marking it in the surface pass wiped out what was under
  it (a bubble over a wall cut a hole in its shadow) and changed lit pictures with Glass caustics 0. Now light pass 9
  draws only bending sprites and Glass liquids, only when photons go through free glass (`BentPlan::free_glass`), and
  `free_glass.wgsl` marks it over a copy of the surface picture where nothing solid is, keeping blocking, height and
  cover and writing a code into B (`free_glass_of`). The overlay's glass bends nothing.
- **Review fixes to the photons:** signed neighbour widths, so a focus inside a leg is sharp; the fan's photons take
  16 wavelengths in turn and a split leg measures its width against its own colour's neighbours, so rainbow rims stay
  coloured; a photon carries light only as far past its last glass as the direct light counts the glass solid
  (`carried_past`: Shadow length, or a Stage slab's reach from the lamp's height), its last step clamped to it; with one
  leg left it bends no more (a cloud of bubbles); in glass or see-through it steps at most a pixel (a field step passed
  a wall under a bubble); lasers, whose light isn't carried, see bending glass as see-through (`bent_carried`); every
  pair is carried while the cap allows (fewer photons a pair rather than pairs left out).

Tests: `bpe-kit/tests/bent_light_pictures.rs` (straight glass matches part 1 at Fast and Good for a lamp and a sun,
a pane lit from the side at up to 76° matches too, free glass leaves a wall's shadow whole and a bending glass liquid
draws as a plain one with Glass caustics 0, two panes aren't counted twice, a disc focuses ≥ 2.5×, a gentle biconcave lens spreads, rainbow rims, nothing bending
sends nothing, a Stage lens, a bending sprite focuses, a Glass liquid sends photons and a Plastic one doesn't), the
bake's `a_glass_lens_focuses_in_the_bake`, unit tests in `bent_light.rs`, `frame.rs` (`free_glass_box`), the shape
draw (benders) and the layers' Lighting (Sprites bend, Soft smoke doesn't; the liquid's style).

Known limits (decision 12) stand; reflected light at glass edges is dropped (part 4's mirrors can pick it up). Also
known from review: the legs add into the half-float direct picture (a great many faint legs on one pixel lose a little
precision); on a Stage the bent light is traced in the picture plane, so with Looking down it doesn't shift with the
slab's shadow, and it passes Stage balls; Shadow strength above 1 makes the photons a little brighter than the light
the shadow took; clear bending glass now gives contact shading round it; glass thinner than about 4 pixels takes its
normals from the field, and a thin outline bends only where its cover passes half (so more at export size).
