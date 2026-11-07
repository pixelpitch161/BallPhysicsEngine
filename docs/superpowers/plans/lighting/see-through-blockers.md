# See-through things let light through (Flat lighting)

Aedan, 2026-11-07, after the thin-blockers fix (`thin-blockers.md`): "Can you make it so things let through more
like the more transparent they are?"

## What happens now

In Flat, a thing that blocks light blocks it whole wherever it covers a tenth of a pixel or more
(`BLOCK_LEAST`). How see-through it is plays no part:

- the surface picture's R is blocks × cover, and cover there is the antialiased edge times the opacity, so the
  two can't be told apart: a pixel half covered by a solid line reads the same as a pixel wholly under a half
  see-through wall. The thin-blockers fix had to treat both as solid so thin lines would stop light;
- a Surface's See-through (glass) never reaches the 2D shadows at all.

## The goal

In Flat, a blocker lets through the share of light it is see-through:

- **opacity**: a layer's or a thing's opacity, a colour's alpha, a fading ball or trail, the soft edge of a
  particle or smoke, all pass `1 − opacity`;
- **See-through**: a Look card's Surface (or a layer's Surface sub-layer) at See-through `s` passes `s` of the
  light that reaches it (so glass casts a light shadow, clear glass none);
- light that crosses several see-through things takes each one's share in turn (two half see-through walls
  pass a quarter);
- the shadow keeps its soft edge; a solid thing (and a thin solid line) still blocks whole, exactly as now;
- Stage is unchanged (its shapes' slab shadows and its spheres have their own maths), and so is the light
  field (glow and bounce light): a thing at least half opaque stops it, as now.

## How

### The surface picture reads opacity, not cover (Flat only)

R becomes "how much the things over this pixel block, by their opacity", taken whole across an antialiased edge
where the thing covers at least `BLOCK_LEAST` of the pixel (below that it stays cover-weighted, so which pixels
block is exactly as now). A and G, B are unchanged.

- Balls (`layers.wgsl::shade_layers`) and shapes (`shape_layers.wgsl::shade_shape_layers`): in the surface pass in
  Flat, where the outline's antialiased cover is at least 0.1, R = max(R, the inside layers' own R) before the
  thing's opacity and its broken-piece fade multiply it.
- Trails (`trail.wgsl`): `shade_trail` leaves its crisp antialiased cover (with the sub-pixel dimming, and a band
  trail's band edge) in a private `AA_COVER`; `scene.wgsl::light_flat`'s surface branch divides it out of R where
  it is at least 0.1. A soft trail's softness stays (it fades as see-through would). Sprites and smoke fields set
  nothing: their alpha is their see-through.
- Because blending is premultiplied "over", R where a see-through thing lies over another becomes
  `1 − (1 − a)(1 − b)`, as it should; at an edge over another blocker it can pass 1, so readers clamp it.
- Readers that want the old cover-weighted value (the light field's scene, `lit_at`'s "blocked here", the Stage
  camera) read `min(R, A)`, which is exactly the old R for a single thing (R = blocks · op, A = cover · op).

### See-through folded into "blocks" (kit)

`surface::Stamps::apply` already stamps every record with its surface index; it also multiplies the record's
`light[1]` (blocks, 0 or 1) by `1 − see_through` of that surface (the thing's, or the layer's own Surface
sub-layer's). In the surface pass `light_piece` keeps `keeps` for exactly non-blocking layers (`blocks <= 0`), and
on a Stage takes any blocking as whole (so Stage stays exactly as it is).

### The march

`lighting.wgsl`:

- `pixel_block` clamps R to 1. A pixel leaves its own blocker first (`march`) when that blocker covers more than
  half of it: `min(R, A) > 0.5 · R` (the same pixels as before for a solid thing; a see-through thing lights its
  own pixels too).
- The field (R16Float → Rg16Float) gains G: **how much the nearest blocker blocks** (`fs_field`: the most any
  blocking pixel under the nearest blocked texel blocks; a texel that only closes a diagonal gap takes its
  neighbours'). `field_both(p)` reads both.
- `march` keeps `lit`, the share the see-through blockers it crossed let through. When `confirm` meets a blocking
  pixel: R at least `SOLID` (0.98) is solid, exactly as now; less is see-through: the ray walks through it
  (half a pixel at a time near its edges, by the field's depth inside it), keeping the most any pixel on the way
  blocks, multiplies `lit` by `1 − that` once out of it and carries on (a solid pixel met inside makes it solid).
- Samples near a see-through blocker darken the soft edge by its opacity only: the ray keeps `e`, the least of
  `1 − o · (1 − smoothstep(v))` over its samples (`o` the field's G), which for solid blockers is exactly the old
  `smoothstep(v)`.
- `visibility`: a middle ray that crosses nothing gives `e`. One that crossed something solid and ends deep in it
  gives 0 (as now). Otherwise the rays across the lamp run as now (a ray across stops at a solid crossing and
  carries on through see-through ones); the share of the lamp the clear rays show is `V`, and the light is
  `1 − ō (1 − V)`, `ō` the mean of `1 − lit` over the rays that crossed something (the middle one too). With
  only solid blockers `ō` is 1 and this is exactly the old union.
- A transparent shadow's soft edge: just outside it the middle ray grazes the blocker (`e` = 1 − o/2); just
  inside it crosses (V ≈ 1/2 as for a solid blocker), so `1 − o/2` on both sides: no step.

### The light field

`fs_gi_scene`'s blocks come from `min(R, A)` (unchanged values) and its "thin" rule (a texel the lighting's field
blocks counts as opaque) applies only where that blocker is at least half opaque (the field's G), or a half
see-through fill would turn opaque in the glow light.

## Tests (`crates/bpe-kit/tests/see_through_shadows.rs`)

Light only, ambient 0, light measured in linear light against the same scene with the blocker not blocking:

1. A thick wall at See-through 0, ½, 1: none / about half / all of the light behind it (Draft, Good, Ultra).
2. A thin hollow ring (2 world units) at See-through ½ round the lamp: about half outside it.
3. Two half see-through walls: about a quarter.
4. A ball whose Body is at opacity ½ in front of the lamp: about half behind it.
5. The soft edge past a see-through wall's end has no step.
6. `thin_blockers.rs` still passes (solid things), and the pinned Flat pictures move only where something
   see-through blocks.

## Not in this change

- Coloured shadows (light tinted by the colour it passes through).
- Stage shapes' slab shadows and the CPU light probe (light sensing) still treat see-through things as solid.
- An opaque line drawn over a see-through fill can be hopped over inside the fill (the walk steps by the field's
  depth there).
