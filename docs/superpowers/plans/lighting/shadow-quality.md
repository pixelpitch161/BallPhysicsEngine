# Shadow quality (review W2: V1 to V5)

Aedan, 2026-11-08 (code review): "shadows often look aliased or don't make sense". The full-size visual review
(report "Ball Studio code review: what to improve", findings V1 to V5; before/after pictures and trial patches in the
project files `ballphysics/visual-review/lighting/`, made against bb129c0 and untested) found five causes. Aedan
picked every finding but the Size ones. This plan fixes them in `crates/bpe-gfx/src/shaders/lighting.wgsl` and
`crates/bpe-gfx/src/lighting.rs`. No settings, no scene format, no physics or sound changes; a scene with no Lighting
card is untouched (the unlit pictures stay byte-identical).

## V1: bright jagged lines inside shadows, fans through thin walls (Flat and Stage)

Cause: a shadow ray that skims a ball or runs along a thin wall takes tiny steps (`max(|d|, 0.5 texel)`), uses up
its step budget (16 Draft, 32 Good, 64 High, 96 Ultra) and counts the rest of its way as open.

Fix (trial patch `fix-shadow-leaks.patch`, taken as is, then checked by eye):

- `march`: a step only counts toward the budget when the ray is at least 2 texels from every blocker
  (`abs(d) >= 2 texel`); the loop's own cap goes from 4× to 12× the steps so a ray can't loop forever.
- The soft edge estimate uses the nearest the ray came to a blocker between this sample and the last
  (Aaltonen's improved soft shadows: the two samples' clear circles meet `y` back along the ray, `near =
  sqrt(d² − y²)` at `t − y`), so where the samples fall doesn't show as stripes in the penumbra.
- `slab_visibility` (Stage shapes): the same budget rule (`abs(d) >= texel`, cap 8×), the same estimate, and steps
  of at most half a texel near a shape (it stepped a whole one and missed thin walls' slabs).

Cost lands only on pixels whose rays skim an edge.

## V2: area lights throw the wrong shape of shadow; Strip lights the same

Cause: `lamps`/`shade` trace an Area or Strip light's shadow to its **nearest point** as if it were a bulb of the
light's Size (24 world units by default), stopping Size short of it. A ball under a wide panel casts a straight band
instead of a shadow that narrows to a point; a ring wall near the panel lets light out in fans (the march stopped
24 short of the nearest point, inside the wall).

Fix (generalising trial patch `fix-area-light-shadows.patch` to Strip lights and to `shade`):

- New `struct Extended { way: vec2, dist: f32, ang: f32 }` and `fn extended_seen(l, p) -> Extended`: how an Area or
  Strip light looks from `p`: the direction of the middle of what it covers (`way`), how far that middle point is
  (`dist`) and its half width seen from `p` as an angle (`ang`, the tangent of the half angle, at most tan 1.3).
  - Area (rectangle or disc): from the corners (rectangle) or the support function (disc), measured across the way
    to the light's middle, as the trial patch did; `ang` = half width seen / distance.
  - Strip: the angles of every segment's ends (and the nearest point) round the way to the nearest point, their
    least and most give the way to the middle of the arc it covers and `ang`; `dist` the distance to the strip's
    point nearest the middle ray's end. (A strip that wraps more than half round `p` is capped at the cap above.)
  - Under the light (`p` on the panel or within a texel of the strip): `ang` 0, no shadow work (as before: the
    light's own place doesn't shadow itself).
- `visibility(p, toward, dist, angular, reach, stop)` gains `src: i32`, the light's index for an Area or Strip
  light (−1 for a point, spot, sun). For an extended light the rays across the lamp each go to the light's point
  nearest where that ray would have ended (`source_point(l, p + toward·dist + side·off·dist)`), ending `stop`
  short of it, and Draft keeps the 5 rays (3 would leave a wide panel's middle unsampled).
- `lamps` and `shade` (the Flat path, used for shiny surfaces) call `extended_seen` for kinds 3 (Area) and 4 (Strip):
  the shadow aims at `way`, marches `dist`, the soft angle is `max(ang, Size / dist) × Shadow softness × the light's
  Softness` (Hard style: half a pixel, as before), and the march stops at the light's edge: `stop` = ridden skip +
  half a pixel, never the light's Size, but at least how deep the light's nearest point sits in a blocker (as now).
  The light's **amount** (falloff) still comes from its nearest point, as now.
- Stage (`shade` with `stage`): the shadow aims at the middle of what the light covers at the light's height, and
  the soft angle is its half width seen over the 3D distance, for both `sphere_shadow` and `slab_visibility`.
- Not changed: the bake's traced bounce (`lamps_on`) and the light probe (CPU) still use the nearest point.

## V3: stripes inside thin shapes' shadows on the Stage back wall

Cause: `slab_visibility` has no crossing test. A ray through a thin shape's slab only gets as deep as half the wall,
so the soft estimate says about half the light gets through, and rays that land a sample in it and rays that step
past alternate: stripes.

Fix: Flat's approach, in 3D.

- Split `slab_visibility` into `slab_march(p, l, dist, ang, stop) -> SlabRay { v, clear, crossed, see }` (one
  ray: `crossed` when a sample of a **solid** shape is inside it, `d < 0`; `clear` the least `d / (t − start)`) and
  `slab_visibility`, which marches the middle ray and, when it crossed and isn't already dark, `LAMP_RAYS` rays
  across the lamp sideways in the picture plane (`side` ⟂ the light's way across, in xy, kept unit in 3D) and takes
  the union of their clear stretches, exactly as `visibility` does (the union moves to a shared `lamp_share`).
  See-through shapes keep their `see` as now. The front face (`face`) is unchanged.
- No picture `confirm` on a Stage (its surface picture is the oblique view): a ray within a texel of a thin shape's
  side counts as crossing, so a thin shape's Stage shadow can be up to one field texel wider. Acceptable on the back
  wall (the shadow there is the shape's, much farther than a texel from it).

## V4: shadow edges wobble by about a pixel as a ball moves

Cause: the field is seeded per texel, all or nothing, at the texel's middle, so a blocker's edge in the field moves
in whole-texel jumps (2 pixels on Good, 4 on Draft) as a ball moves smoothly; projected onto the shadow's far end,
that becomes a crawling edge. The review tried centroid seeds and a second idea; neither helped.

Fix, timeboxed (dropped and written down as still open if it doesn't measurably help): sub-texel edge distance in
the field (the antialiased distance transform idea): `fs_seed` keeps its conservative "does it block" (thin lines
must still block) but also stores how much of the texel the blockers cover (`foot_cover`, from the surface picture's
smooth cover `cover_block`), and `fs_field` adds, outside blockers, the nearest seed's edge offset `(0.5 − cover) ×
texel` (clamped to ±half a texel) to the distance, so a ball's field edge slides smoothly inside its edge texels.
Measured with a new test (below): the shadow edge position against the ball position over many small moves.

## V5: the editor preview's shadows look rougher than the export

Cause: the field's size is the preview's size divided by the Quality's divisor, so at a 540-wide preview a Good
shadow has the coarseness of a 270-wide video.

Fix: `Lighting::ensure` takes the frame's world width (`Frame.width`, the video's width in world units) and sizes
the field so a field texel covers the same world size as in the video: divisor = `clamp(round(q × width /
frame.width), 1, q)` with `q` the Quality's divisor (never coarser than now). An export at video size is unchanged.
The preview costs about what the video's lighting costs per frame at the same Quality.

## Tests

- New `crates/bpe-kit/tests/shadow_quality.rs` (GPU, 270 × 480 via `bpe_gfx::test_device()`):
  - V1: many balls in a block under a low lamp, Draft and Good: no pixel inside the block's shadow region (the
    middle of the gaps behind the balls, picked where Ultra is dark) is brighter than Ultra's there plus a margin.
  - V2: a ball under a wide Area panel: the shadow right below the ball is dark and far below it is lit again (the
    shadow narrows to a point); a ring wall near the panel lets no light out above the ring (outside = dark).
    The same for a Strip light along the top.
  - V3: a thin hollow shape on a Stage: across its shadow on the back wall the light varies smoothly (no row whose
    brightness jumps against its neighbours by more than a margin).
  - V4 (if kept): a ball moved in 16 steps of a quarter pixel: the shadow edge's position moves monotonically with
    steps no bigger than a margin.
  - V5: the same scene drawn at 270 and 540 wide (video 1080) gives the same field size in world units
    (`Renderer` reports its lighting field size), and pictures that match after shrinking.
- All existing lighting tests pass; goldens `golden-lit-flat*.txt` re-blessed only if their lit lines move (debug and
  release), the diff checked to be lighting tiles only.
- Full-size look check: new shots `area-ring` (V2 leak), `strip` (V2 strip), `stage-thin` (V3), `moving` already
  there (V4); run at draft, good, ultra; open the crops.

## Not doing

- Exports forced off Draft (V5's "consider"): Aedan chooses the Quality; with V5 the preview now shows what Draft
  gives in the video.
- The CPU light probe and the bake's traced bounce keep the nearest point for Area and Strip lights.

## As built

- **V1** as planned, plus: a step also has to move at least an even share of the way (`fair` = way / steps) to count
  toward the steps. A ray down a lane between rows of balls stays a few texels from them the whole way and used up
  Draft's 16 steps halfway. The loop's cap is `max(12 × steps, 400)` (a low sun along a long thin wall needed more
  than 12 × 16). `slab_march` follows the same rules (a texel, `max(8 × steps, 300)`). With the even share, the
  steps no longer end a march early: the cap does. That costs Draft more where rays creep, which was the price of
  no leaks.
- **V2** as planned. Extra: `Extended.stop` (the march ends at the light's edge, or as deep as the light sits in a
  blocker), 9 rays across the lamp for Area and Strip lights (`MOST_LAMP_RAYS`; 5 left streaks in wide penumbras),
  `clear` taken from the same nearest-between estimate (for these lights only: for a lamp it put a step at the start of a thin wall's soft edge), and a `wide` march stops once a sample is clearer than the
  rest of the way is long (otherwise walls behind a wide light drew contour lines). The full-size `area` shot's light
  now has Direction 0 (with the default 90° its 360-wide panel stood upright and poked out of the ring: not a leak).
- **V3** as planned, without any picture check on a Stage: a `slab_confirm` was tried and let dotted lines of light
  through thin walls on small pictures, so it was taken out. `SlabRay.over` lets light over the shapes' front when the
  ray rises. Still open: the texel steps of a thin shape's contact shadow right beside it.
- **V4** kept, as `edge_inset` in `fs_field` (`1 − foot_cover` of the seed, from 0 to a texel, where the texel behind
  the seed is wholly covered, so a thin wall's end isn't cut), not the plan's `(0.5 − cover)` stored by `fs_seed`. Only for solid blockers, and not on
  Draft (gated by its 16 steps, so the preview and the video agree): edge steps are 2.5× more even on Ultra and 1.7×
  on Good; on Draft it made them worse.
- **V5** is opt-in: `Renderer::preview_field_like_video(true)`, set by the editor's preview and its warm-up spare.
  `field_divisor` rounds down. Exports draw at the video's size anyway, and the other picture tests keep the
  Quality's share of their small size, which keeps the thin-line code (`confirm`, `foot_blocks`) covered.
- Stops: the middle ray of an Area or Strip light ends half a pixel short of the light, as the rays across it do; on
  a Stage it still stops as deep as the light sits in a blocker.
- Tests: `bpe-kit/tests/shadow_quality.rs` has V1 (Draft and Good against Ultra), V2 (Area and Strip narrowing, ring
  leak), V3 (row brightness goes down and up about once: under 3 × its range; with the stripes it was 4.6×) and V4
  (16 small moves, even steps), all drawn with the preview's field. V5 is a unit test in `lighting.rs`
  (`field_divisor`). Full-size look shots
  `area-wide`, `strip`, `stage-thin`. `golden-lit-flat*.txt` re-blessed (only the lit scenes' tiles move).
