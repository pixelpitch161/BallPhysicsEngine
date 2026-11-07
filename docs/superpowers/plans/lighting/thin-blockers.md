# Thin blockers block light fully (Flat lighting fix)

Aedan, 2026-11-07: "In 2D lighting mode, thin lines leak a lot of light through them (a thin hollow shape, or
wavefronts set to block light)."

## What was measured (root cause)

`crates/bpe-kit/tests/thin_blockers.rs` (270 × 480, 4 world units a pixel; a point light inside a hollow circle of
radius 300, ambient 0, light measured outside the ring against the same scene with the ring not blocking):

| Quality | 1 / 2 / 4 world thick | 8 thick | 16 thick | 40 thick |
| --- | --- | --- | --- | --- |
| Draft | all the light | ~75–100 % | ~65–80 % | 0 |
| Good, High | all the light | ~70 % (40 % on diagonals) | ~30 % | 0 |
| Ultra | 1, 2: all; 4: ~75 % | ~55 % | ~0–15 % | 0 |

Three causes, all in `crates/bpe-gfx/src/shaders/lighting.wgsl`:

1. **Seeding drops thin lines.** `fs_seed` takes one bilinear sample of the surface picture at the centre of each
   field texel (the field is 1/2 or 1/4 of the picture below Ultra) and marks it blocked only above 0.5. A line
   thinner than about half a texel never gets into the field, so it casts no shadow at all.
2. **The soft-shadow estimate needs depth.** `visibility` keeps `min(0.5 + 0.5·d / (t·angle))` along the ray (the
   "full penumbra" signed-distance estimate). Inside a blocker `d` only gets as negative as half the blocker's
   thickness, so anything thinner than the penumbra at that distance lets about half the light through, however
   long the wall.
3. **The march can step over a wall.** It steps at least one field texel against a bilinearly sampled field, so a
   one-texel wall can be stepped across without a sample landing inside it (and a diagonal line of single texels,
   touching only at corners, has a gap where the bilinear field is 0).

## The fix

- `fs_seed` (conservative seeding): a field texel blocks when **any** picture pixel under it blocks at least
  `BLOCK_LEAST` (0.25 of its cover), via `foot_blocks(texel)`; a **diagonal gap** (two blocked texels touching only at
  a corner, both other texels open) is closed by blocking one of the open pair, so every line is 4-connected in the
  field. Thick shapes are unchanged but for their edge texels (at most one texel bigger).
- `visibility` is split into `march` (one ray; returns its visibility and whether it **crossed** a blocker):
  - the step is at least half a field texel, so a sample always lands inside a one-texel wall;
  - wherever the field says "inside a blocker" and the pixel there is not itself a blocker (the field is coarser
    than the picture and now conservative), `confirm` walks the **picture itself** half a pixel at a time out of that
    stretch: if a blocking pixel is met the ray crossed something, else the stretch was only the field's coarseness
    (a pixel beside a thin line, on its far side from the line's shadow) and counts as open;
  - a pixel that is itself a blocker leaves its blocker first, as before (a thing doesn't shadow itself).
- When the central ray crossed a blocker and wasn't already fully dark, the light is worked out from
  `LAMP_RAYS` (5) rays to points across the lamp, edge to edge (the middle one is the ray already marched, so four
  more marches): a ray that crosses anything shows nothing; one that gets past shows the lamp from its point less
  how clear it stayed (`clear` = least `d / t` along it, an angle) to its point plus that, within the lamp; the light
  is the length of the union of those stretches over the lamp's width. For one straight blocker edge this is exact
  and continuous (the clear ray's stretch reaches the edge), and where the middle ray grazes a blocker it is 0.5,
  meeting the outside estimate, so the penumbra has no step. (Tried first and dropped: slices each with their own
  outer penumbra, which left a flat step at a quarter, and slices centred inside the lamp, which started the soft
  edge late.) Rays that don't cross anything keep today's estimate exactly; deep crossings (the old estimate already
  0) stay 0 without the extra rays.
- Just past a stretch only the field's coarseness blocked, samples nearer than a texel to it don't count toward
  the grazing estimate (`beside`): the field can't tell within a texel, and counting them half-shadowed pixels on
  the lit side of a thin line.
- `BLOCK_LEAST` is 0.1 (a line a fifth of a pixel thin still blocks). A pixel is "on a blocker" (and leaves it first)
  only when it blocks more than half; at a blocker's soft edge it doesn't, and `confirm` skips the pixel's own
  pixel, so the way to the light decides: on the lit side it is lit, on the far side dark (no bright rim behind
  shapes, no dark rim in front).
- The light field (`fs_gi_scene`): a texel is also opaque where the lighting's own (now conservative) field marks it
  blocked (`texel_blocked`), so glowing things' light stops at thin lines too. Such a texel bounces as the blocking
  taps alone do (their lit colour and light weighted by how much they block), or a dim ring of them round a lit
  thing would hide its bounce.

## After the review

- **Stage balls stay out of the field.** A Stage ball marks the surface with a quarter (`layers.wgsl`, so the old
  "more than a half" seed left it out); the seed now takes more than 0.3 on a Stage (`Lighting.trace.y` = 1 on a
  Stage, set in `lighting.rs`), `BLOCK_LEAST` elsewhere. Stage pictures were compared before and after: the same
  but for the thin ring's edge.
- **`confirm` starts at the ray's last sample outside a blocker** (`open`, at most a texel back), not at the first
  inside one: the field's edge lies up to a step before it, and a line in the near half of its texel lay there too.
- **Skips don't use up the steps**: coarseness skips and `beside` samples don't count toward the Quality's steps
  (a separate cap of four times as many loop rounds), so a ray along a thin line doesn't run out and leak.
- **Cost bounds**: no extra rays when the soft edge where the middle ray crossed is under half a pixel (it is dark);
  Draft uses the lamp's two edges only (3 rays, two extra marches).
- Known trade-off: anything blocking at least a tenth of a pixel's cover blocks fully (before: half), so a fading
  thing keeps its shadow until nearly gone, and soft-edged blockers' shadows grow by up to a field texel. 0.2 was
  tried and let half-pixel lines through at Draft on diagonals.
- Tests added: a thin wall at four slants at every Quality (points all along behind it), and a lamp 40 world units
  beside a thin wall (its side lit, the far side dark).

## As built: pictures that moved on purpose

- `golden-lit-flat*.txt` re-blessed: `lit_room`'s ring is 3 world units thick (0.75 px at the test's size) and used
  to let the lamp's light out whole; outside it is now dark (tiles up to 80/255). `lit_demo`'s 10-thick ring leaked
  about a third; the rest moved by a few levels (inner penumbras now integrated over the lamp).
- `lit_pictures::bounce_light_spills_a_lit_balls_colour`: the room's thin white ring now blocks and bounces the
  lamp's white light, so the test turns its blocking off; the ball's edge texels now stop the field too, a hair less
  red (+8 against +11), so the check is > +6.

## Not changed

Stage's sphere and slab shadows, the CPU light probe (`bpe-kit/src/sensing/`, which reads outlines exactly), lasers.
No settings, no scene format change, no physics or sound goldens. A scene with no Lighting card is untouched.

## Verification

- `tests/thin_blockers.rs` (GPU): the ring at 2, 4, 8 and 16 world units thick blocks the lamp fully outside it at
  Draft, Good and Ultra (light outside ≤ 3 of 765), the open ring still lets the light through; a lamp outside a
  thin wall's end still lights round it (the penumbra is there: a pixel past the wall's end is lit, one behind its
  middle dark); a thin line's lit side next to it is still lit (no dark band from the coarse field).
- All existing lighting picture tests (`lit_pictures`, `lit_flat_pinned`, `lit_part4`, `stage_pictures`,
  `glass_pictures`, `bake_pictures`, `unlit_pictures`) still pass.
