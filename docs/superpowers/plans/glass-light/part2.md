# Glass and light part 2: prism and rainbow beams

Spec: `docs/superpowers/specs/2026-11-07-glass-light-design.md` (part 2), approved by Aedan 2026-11-07 18:34Z. Builds on
part 1 (`part1.md`, merged #149) and lighting part 4's lasers (`bpe-kit/src/things/light/laser.rs`, `draw.rs`,
`laser_beams` in `lighting.wgsl`). Part 3 (bent light through glass shapes, a GPU photon pass) is built in parallel in
another thread: this part keeps to the laser tracer, the light kinds, the Surface presets and one small GPU pass, so the
two meet only in `surface.rs` (`glass_of`) and `lighting.rs`/`lighting.wgsl` (new items appended, nothing reworked).

**Goal.** White light through glass fans into a rainbow: a laser splits into Colours paths where it bends through glass
with Rainbow split, a new **Light beam** (a laser with a width) does the same as a soft band, and a **Prism** chip makes
the glass for it.

## Global constraints (every task)

- Draw-only: physics, sound and every golden unchanged (`cargo test -p bpe-kit --test golden*`, sound goldens).
- A scene without a Lighting card byte-identical (`tests/unlit_pictures.rs`); a lit scene with no laser/beam through
  dispersive glass unchanged (`lit_flat_pinned.rs`, `stage_pictures.rs`, `coloured_shadows.rs`, `see_through_shadows.rs`).
- Rainbow split 0, or Colours 1, gives today's laser path exactly (same points, same single `LightGpu`).
- `cargo fmt` (width 120), `cargo clippy --workspace` clean, bpe-gfx tests with `--test-threads=2`.
- No demo videos or screenshots.

## Decisions

1. **Wavelengths.** A colour is a place `w` on the spectrum, −1 red (700 nm) to +1 violet (400 nm), `λ = 550 − 150 w`.
   With `Colours` = N the colours sit at `w_k = −1 + (2k + 1)/N`, k = 0 (red) … N − 1 (violet).
2. **Dispersion** (`laser::index_at(index, rainbow, w)`): `n(w) = max(index + rainbow · DISPERSION · w / 2, 1)`, i.e. the
   spec's `n + rainbow · D · (λ₀ − λ)/(700 − 400)`. `DISPERSION` = 0.0462: Rainbow split 1 on Bends light 1.5 spreads
   red (w −1) and violet (w +1) by 4° through a 60° prism at the middle's least deviation (pinned by a test, ± 0.25°).
   Red bends least. `w = 0` is the plain index, so a beam that never splits is today's beam exactly.
3. **Colour shares** (`laser::spectrum(n) -> Vec<(f32, [f32; 3])>`): each colour's rgb weight is the renderer's
   `spectral(w)` hat (the same three triangles as `lighting.wgsl::spectral`), normalised per channel so the N weights add
   to exactly (1, 1, 1). A light's colour × weight is that colour's light: white splits into a rainbow; a red laser keeps
   only its red share (its blue colours are black and are dropped, decision 6). N = 1 is one weight of (1, 1, 1).
4. **Splitting.** `Meets::Bend` gains `rainbow` (`Meets::Bend { index, rainbow }`); `surface::glass_of` returns a
   `Glass { index, rainbow }` (rainbow = the Surface's Rainbow split, finite, at least 0). The beam is traced once white
   (`w` 0). At the first bend *into* glass whose `rainbow > 0`, when Colours > 1 and the beam still has a bounce left
   there, the white path ends at that point (`ended_on_hit` true, so its spot shows at the bend as today) and N colour
   paths start from it, each bent in with its own `n(w_k)` and carried on (its own index inside, its own bends at any
   later glass, its own total internal reflection) with the bounces and length left. Nothing splits twice: colour paths
   use `n(w_k)` at every later dispersive glass.
5. **The segment cap** (`MOST_BEAM_SEGMENTS` 4096 a light). The white path has at most `MOST_BOUNCES` + 1 segments.
   Each colour path may have at most `max(1, (4096 − white) / N)` segments: tracing stops there (`ended_on_hit` false),
   so the farthest-traced segments of every colour are what is dropped, evenly, and tracing never does more work than
   the cap. Beams (decision 9) cap each across-ray at `MOST_RIBBON_SEGMENTS / rays` (32768 / rays) the same way.
6. **One light a colour.** `draw.rs::push_light` pushes, for a laser, one `LightGpu` for the white path and one per
   colour path whose colour × strength isn't black (all channels under 1e-6 are skipped), each with its own segment run
   (`p4.zw`), colour (`p1`), and `p11.z` (ended on a hit); everything else (reach, size, shadows, gobo, flicker, flare,
   links, rigs' drive) is the same for all. The shader needs no change for lasers: each colour is a laser line and a strip
   light as today (`laser_beams`, `source_point`, `fs_direct`), so colours light surfaces near them, glow, and leave hit
   glows (the rainbow stripe where they land). Costs: a split laser is 1 + N lights for every lit pixel.
7. **Colours setting** (`colours: u32`, label "Colours", range(1, 64), span(1, 32), default 12, not keyframed, group
   "Beam", when `kind=laser or kind=beam`). 64 is the hard ceiling (`MOST_COLOURS`), as Bounces has 256: every colour is
   a light every lit pixel looks at.
8. **Light beam kind** `LightType::Beam`, code 6, menu name "Light beam", strip word "beam", button "+ Beam". In
   `lighting.wgsl` every `kind > 4.5` test already treats it as a laser (no `light_toward`, a strip in `source_point`, a
   line in `laser_beams`). Its own settings (group "Beam"): **Width** (`beam_width`, range(0, 1e9), span(0, 200), px,
   keyframe, default 60), **Edge softness** (`beam_softness`, span(0, 1), keyframe, default 0.35), **Rays across**
   (`rays_across`, range(2, 64), span(2, 64), default 16, not keyframed), plus Colours and the laser's Beam glow, Hit glow,
   Bounces, Beam length, At shapes, At balls (their `when` becomes `kind=laser or kind=beam`). The laser's Beam width
   stays laser-only, and Beam in haze becomes `kind!=laser and kind!=beam`. The whole group is renamed "Laser" → "Beam"
   (it is the beam's settings for both kinds). `of_kind(Beam)`: colour #fff8f0, strength 2, reach 240, size 2, height 20,
   Beam glow 0.6, Hit glow 0.6.
9. **A beam's light and its band.** A beam traces its **middle ray** exactly as a laser (decisions 4–6), pushed as
   kind-6 `LightGpu`s that light surfaces (size = Size + Width / 2, as a strip's tube) but draw no line (`p7.w` 0) and
   whose hit spots are half the beam wide (`p11.x` = Width / 6; `laser_beams` spots are 3 × `p11.x`). Its band is
   `rays_across` rays, ray j at `(j/(R − 1) − ½) · Width` across the aim from the lamp, each traced with
   `trace_split`. Width under 0.5 px: no band.
10. **Ribbons** (`laser::ribbons`, `bpe_gfx::RibbonGpu { a, b, c, d }` in `LightScene.ribbons`): for neighbouring rays
    j, j + 1, segment m of one paired with segment m of the other makes a quad (a = ray j's segment, b = ray j + 1's),
    walked while both have segment m and the corner at its start met the same thing (`Path.corners`: what each point
    met, `None` at the lamp and where it ran out). Over the white parts the quad takes the light's whole colour; when
    both white parts ended at the same split point thing, each colour k's two paths are walked the same way with
    colour × weight k. Where the rays part (one bounces off a shape the other misses, one passes the prism's corner) the
    ribbon ends. `c` = colour × strength × Beam glow × `BEAM_SHOW` (0.35) and the edge softness, `d` = (place across of
    ray j, of ray j + 1, the gap between them at the lamp, 0). At most `MOST_RIBBONS` (16384) a light.
11. **Drawing ribbons**: one GPU pass (`vs_ribbon`/`fs_ribbon` in `lighting.wgsl`, `Lighting.ribbon`), an instanced
    4-vertex triangle strip a ribbon, added (colour One/One, alpha kept) into the mix's own target right after the mix
    step, before mirror walls, the glossy floor and depth of field (so those see it, and the Stage camera re-views it as
    it re-views laser lines). Brightness at each end is `gap / width there` (clamped to 8): a fan spreading dims, a lens
    focusing brightens. Across, `smoothstep(0, s, u) · smoothstep(0, s, 1 − u)` with `s` = softness / 2 (`s` 0: hard).
    Not drawn in the editor's light-only views (`u.ambient.w > 0.5`), matching `laser_beams`, which `fs_mix` skips there.
    Bind group 3 holds only the ribbons (binding 2, vertex), groups 1 and 2 are empty; group 0 is the mix's `main`.
    No ribbons, no pass.
12. **Prism** surface chip, the 14th (`SurfaceKind::Prism`, after Lava): Roughness 0, Metal 0, Clear coat 0, Bends light
    1.6, See-through 0.95, Rainbow split 1, Tints light 1, Deeper when thick 0. Its swatch is a triangle with a white
    line in and a fan of six spectral lines out (new `SWATCH_*` tokens in `style/swatch.rs`).
13. **One-way sensing.** The light probe builds its lights through `LightDraw::light`, so it sees a split laser's or
    a beam's strip lights as it sees a laser's today (luminance); ribbons (`LightScene.ribbons`) are never read by the
    probe. Rainbow split and the light's settings are already lighting-only (`sensing::lighting_reads`).
14. **Handles.** A beam has the laser's aim arrow and two **width** dots (`LightPart::Width(bool)`) at ± Width / 2 across
    the aim from the first lamp; dragging one sets Width to twice the pointer's distance across the aim (whole px, keyed
    through `set_or_key` like the others). Selected, the beam's two edges show as thin lines along the aim.

## Not in this part

- Light bent through glass shapes onto the room (part 3), mirrors throwing light, dichroic (part 4).
- Ribbons don't cast or read shadows and don't light anything themselves (the middle ray's strip lights do the lighting).
- A beam's band isn't in haze as air light (its ribbons are its glow, as a laser's line is).
- Per-ball glass (varied Surface across balls): a beam meets a Balls thing's card-level Surface, as lasers do today.

## Tasks

### Task 1: dispersion in the tracer (`bpe-kit/src/things/light/laser.rs`)

**Interfaces.** `Meets::Bend { index: f32, rainbow: f32 }`; `Path { points, corners: Vec<Option<(ThingId, Option<u32>)>>,
ended_on_hit }`; `pub const DISPERSION, MOST_BEAM_SEGMENTS, MOST_COLOURS`; `pub fn index_at(index, rainbow, w) -> f32`;
`pub fn spectrum(n: u32) -> Vec<(f32, [f32; 3])>`; `pub fn trace_wavelength(.., w) -> Path` (no splitting; `trace` is it
at `w` 0); `pub struct Split { white: Path, colours: Vec<(f32, Path)> }`;
`pub fn trace_split(scenery, from, dir, length, bounces, skip, meets, colours: u32, most_segments: usize) -> Split`.
The walk is one private function over a `Walk` state (at, dir, left, turns, inside, points, corners) so the white walk,
the colour walks and the plain trace share one body.

- [ ] Step 1: tests (in `laser.rs`):
  - `rainbow_split_off_or_one_colour_is_the_plain_path`: a glass ball (Bend index 1.5) and a shape; `trace_split` with
    rainbow 0 / colours 12, and rainbow 1 / colours 1, gives `white == trace(..)` and no colours.
  - `a_prism_splits_white_light_red_least_bent`: a 60° triangle (three walls, half-thickness 0) with Bend 1.5/1; a beam
    at the least-deviation incidence; 12 colours; every colour path leaves the prism; deviation rises monotonically from
    colour 0 to 11; `trace_wavelength` at w −1 and +1 differ by 4° ± 0.25°.
  - `each_colour_can_be_all_reflected_inside`: a prism at an angle where violet is totally reflected inside and red
    leaves (index 1.5, rainbow 4): colour 0 leaves through the far face, colour 11 meets it and turns back inside.
  - `the_segment_cap_holds_between_two_mirrors`: a glass ball between two mirror walls, Bounces 256, colours 32: total
    segments ≤ 4096, every colour has at least one segment, and the shares are within one of each other.
  - `the_spectrum_adds_to_white`: for n in 1..=64 the weights add to (1, 1, 1) within 1e-5 and colour 0 is the reddest.
  - Existing tests updated for `Bend { index, rainbow: 0.0 }`.
- [ ] Step 2: implement; `cargo test -p bpe-kit --lib things::light` passes.

### Task 2: lasers split in the picture (`surface.rs`, `light/mod.rs`, `light/draw.rs`)

- [ ] `surface::Glass { index, rainbow }`, `glass_of -> Option<Glass>` (its test updated). `LightSettings.colours` (decision
  7), the group rename, the `when`s. `push_light` for a laser uses `trace_split(.., s.colours.clamp(1, MOST_COLOURS),
  MOST_BEAM_SEGMENTS)` and pushes the lights of decision 6.
- [ ] Tests: `tests/glass_pictures.rs::a_laser_bends_through_a_glass_ball` sets Colours 1 (today's path) and a new
  `a_laser_through_rainbow_glass_splits_into_colours` (frame-level, no GPU): Glass ball, Colours 12 → 1 + 12 lights of
  kind 5, the colour lights' `p1` add up to the white light's within 1e-4, their first points equal the white one's last.
  `light/mod.rs`'s hard-range list gains `colours`, `rays_across`, `beam_width`.

### Task 3: the Light beam (`light/mod.rs`, `light/laser.rs`, `light/draw.rs`, `bpe-gfx`)

- [ ] `LightType::Beam` (code 6, `ALL` has 7), its settings and `of_kind` (decision 8). `laser::ribbons(rays: &[Split],
  across: &[f32], gap, colour, shares, softness, most) -> Vec<RibbonGpu>` (decision 10). `push_light` for a beam
  (decision 9). `bpe_gfx::RibbonGpu` (64 bytes, `Pod`) in `frame.rs`, `LightScene.ribbons`, exported from `lib.rs`.
- [ ] `lighting.wgsl`: `RibbonGpu`, `@group(3) @binding(2) ribbons`, `vs_ribbon`, `fs_ribbon` (decision 11).
  `lighting.rs`: `ribbon_layout`, `ribbon` pipeline (HDR target, colour add, alpha `src Zero, dst One`), a
  `GrowableBuffer` `ribbons`, drawn after the mix step when `scene.ribbons` isn't empty.
- [ ] Tests (kit unit): `a_beam_band_is_rays_across_wide` (no glass: R − 1 ribbons a segment, the outer rays Width
  apart); `a_beam_ribbon_ends_where_its_rays_part` (a short wall only half the band meets: ribbons beyond the wall exist
  only between rays that passed it, and the pair straddling the wall's end has no quad past it);
  `a_beam_through_a_prism_makes_coloured_ribbons` (each colour's ribbons carry colour × weight).

### Task 4: the Prism chip (`surface.rs`, `bpe-studio/src/style/swatch.rs`)

- [ ] `SurfaceKind::Prism` and its preset (decision 12); `surface_swatch` arm 13; the Lava arm becomes `12`.
- [ ] Tests: `surface.rs` preset test (Prism: see-through 0.95, rainbow 1, index 1.6, `glass_of` Some); the studio's
  existing surface-chip tests (whatever counts 13 chips) updated to 14.

### Task 5: editor (`lights_panel/mod.rs`, `preview/light_handles.rs`)

- [ ] `kind_word` "beam", "+ Beam" button (the tree's Add thing menu lists `LightType::ALL` already).
- [ ] `LightPart::Width(bool)`: dots, drawing (edges + aim arrow), `apply` (decision 14). Test in
  `preview/light_handles/tests.rs`: dragging a beam's width dot 50 px across the aim sets Width 100.

### Task 6: pictures (`bpe-kit/tests/prism_pictures.rs`, GPU)

Scene: Lighting card, ambient low, a back wall (a solid wide rectangle shape far right, Matte white), a filled
triangle shape (Polygon, Sides 3) with the Prism preset in the middle, a white laser (Colours 12, Bounces 8, At shapes
Stop on the back wall: the wall isn't glass so the laser stops there). Expected landing points come from
`laser::trace_split` on the same world (CPU), so the test checks the GPU against the tracer.

- [ ] `a_laser_through_a_prism_leaves_a_rainbow_on_the_wall`: at the red colour's landing point R > B + 20, at violet's B
  > R + 20, and red and violet land at least 4 output pixels apart.
- [ ] `a_beam_through_a_prism_fans_into_colours`: a Light beam (Width 80) instead: before the prism the band is lit
  (brightness over the empty background by 30) and roughly white (|R − B| < 25); behind it, on the band's red edge R > B,
  on its violet edge B > R.
- [ ] `rainbow_split_off_draws_todays_laser`: with Rainbow split 0 the picture is byte-identical to Colours 1.

### Task 7: finish

- [ ] CLAUDE.md "Glass and light": part 2 is in (names above). Plan "As built" notes.
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx `--test-threads=2`), goldens in debug and
  release unchanged.
- [ ] One Opus review of the branch; fix; PR; merge when checks pass; tell the coordinator.

## As built

- **Filled shapes are solid glass to the tracer.** Before, the tracer met every outline as a band (a capsule a side), so a
  filled glass triangle bent light in and straight back out of each band and never acted as a prism. Now a filled wall
  (`Wall.solid`) is its bare outline (no band, as balls and the picture see it): light inside it leaves only through the
  outline's outer side (`band`'s `fill`, `turn_of` for the outward normal). Lasers through filled glass shapes change
  path (they now really refract through the whole shape); hollow shapes are as before. A lamp inside a filled non-glass
  shape now escapes it (it used to bounce inside the band).
- **Corners say how the beam turned.** `Path.corners` holds a `Met { thing, row, side, how: Turned }` (Ended, Reflected,
  BentIn, BentOut), so ribbons part where neighbouring rays meet different sides of one shape (a band split by a prism's
  corner) or one colour is all reflected inside while its neighbour leaves.
- **Ribbon brightness** is the gap at the lamp over the band's width measured across the way the ribbon runs (not the
  distance between the two end points, which grows where the band meets a wall at a slant).
- **Ribbon budget**: `MOST_RIBBONS` is shared evenly between the pairs of rays, so a band too busy for all of them keeps
  its whole width.
- **Glass lookups** are read once a lamp (a map of the scenery's things to `glass_of`), not on every hit.
- The planned "no ribbons, no pass" picture test was dropped: the pass is skipped by `scene.ribbons.is_empty()`, and the
  unchanged-picture tests (`lit_flat_pinned`, `stage_pictures`, `glass_pictures`) cover lasers.
- **Review notes kept as they are.** Colours defaults to 12 as the spec says, so a laser through the Glass preset
  (Rainbow split 0.3) now splits into 13 lights (a fan of about 1.2°); set Colours 1 to keep one light. Every colour is a
  strip light every lit pixel looks at, so a beam riding many balls with many colours is costly. Ribbons are drawn after
  the light mix, so the lighting's own Light blooms don't catch them (picture effects' bloom does), as with laser lines.
