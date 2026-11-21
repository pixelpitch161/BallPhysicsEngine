# Liquid fix: spray and bouncing by itself

Asked by the owner on 2026-11-21 with a project file (`Water Weird`: a hollow 840 × 1680 box, a filled turned
plate and a filled circle in it, a Liquid thing that is no shape's child pouring onto the plate, Detail Fine,
Export detail Extreme, Size Bucket): "the liquid sporadically bounces and gets thrown around by itself.
Sometimes, when set to a pour mode, the liquid will randomly spray everywhere seemingly because it's colliding
with itself right away." The owner suggested more than 240 steps a second, or interpolating. Debugged
systematically (reproduce, measure, one change at a time) before any fix; this plan records what was found.

## What was measured (main 6bc08bb, software Vulkan, a scratch test that is not kept)

- **Reproduced.** In the owner's scene the pour arrives at 1,900 to 3,300 px/s; drops were thrown at 6,000 to
  18,000 px/s. Counting drops that gained more than 800 px/s in one liquid step: 0, 31, 312, 728, 992, 626 per
  half second. After 4 s, 973 of 7,240 drops sat on exactly the same spot as another (groups of up to 13).
- **A still pool bursts by itself.** A closed 600 × 900 box filled to 0.3 with no pour and no ball simmered at
  45 to 50 px/s for good, then at 3.0 s threw drops 236 px up.
- **More steps a second does not help** (the owner's idea). The same stiffness at half the time step gave as
  many thrown drops (237 against 158): it is not a time-step instability.

## The three causes

1. **The liquid cannot feel walls** (the main cause). A drop's squeeze J is 1 / the liquid volume the grid holds
   round it (`cs_g2p`). Beside a wall half of that neighbourhood is wall, so the drop reads half the liquid,
   takes itself for the open surface (J > 1) and pushes back at nothing. The liquid behind it then packs into
   the wall until the drops there are dense enough to read J = 1 anyway. Measured in the still box: the half
   cell on the floor's and the walls' drop line held 12 to 14 drops per square cell, the body of the liquid 4
   (three times too many), then an empty layer, then another dense one. When a pour, a corner or a slosh moves
   that packed layer into the open liquid it reads as squeezed to a third and is thrown out: the spray where a
   pour lands on a shape, the jets up the side walls, the still pool's burst.
2. **Nothing damps squeezing.** The pressure is a pure spring (stiff: the speed of sound is about 12,400 px/s
   in the owner's scene). Any pile-up springs back at the speed of sound times how squeezed it was. Stiffness
   × 0.1 cut the thrown drops from 2,689 to 143, which showed the springing is what throws them.
3. **"Holes close" (PR #198) moves drops where it must not.** It moves a drop down the slope of the liquid
   round it. That slope also points into walls (no liquid there) and up out of the open surface. Drops moved
   into a wall are put back on the same line 0.5 cells from it, so drops moved along that line end on exactly
   the same spot and are one drop from then on (same place, same grid velocity, for ever); at the surface it
   pumped the top row. With it off, the still pool's stacked drops went from 1,675 to 0. It still has to
   close real cracks: with it off a pour leaves speckle holes near the surface (rendered "Liquid paint mixing").

## The fix (three changes, each measured alone and together)

All in `crates/bpe-gfx/src/shaders/liquid_sim.wgsl`. No setting, uniform or file format changes.

1. **Walls count as full liquid.** New `weight_below(x)` (how much of a node's quadratic-spline weight lies
   below x), `wall_share(n)` and `edge_share(x, y)`: the share of node n's weight that lies where no drop can
   be. For a wall: from `0.5 − WALL_GAP` cells outside its face (drops are kept 0.5 out; `WALL_GAP` 0.25 is
   half the room a drop at rest takes, so the wall's share starts where the first row's room ends) inwards;
   for a band (two-sided wall, `t.z ≥ 0`) only as far as the same margin on its other side, so liquid on the
   other side of a thin wall is not counted twice. For the picture's edges (drops with `STOPS`): the same
   from `EDGE + 0.05 − WALL_GAP`. It is a flat-wall estimate from each node's nearest face (what the walls
   band holds), so it is a little low in an inside corner and a little high round an outside one.
   - `cs_g2p`: the gathered volume `vd` and its `slope` include the shares (so "holes close" no longer takes
     a wall for the open surface, and the slope no longer points into walls). `p.j` is still stored
     **without** them (`liquid`): `liquid_drops.wgsl` reads it to draw a drop with little liquid round it as a
     droplet, and a thin stream down a wall must keep drawing. Cooling, wind and drying (`open`) also still
     go by the liquid alone, as before: changing what dries is not this fix's business.
   - `cs_p2g`: `j = 1 / (1 / p.j + walls)`, the shares gathered at the drop's place (9 nodes). A drop not
     measured yet (`p.j` 0: just born, or played back by Un-mix) is at rest, as before; it used to be marked
     by `p.j` 1, which the walls' share would have turned into a squeeze on its first substep.
   - Where a wall is at the picture's edge the two shares are joined as 1 − (1 − wall)(1 − edge), not added.
   - Not added to the grid's volume channel itself: the balls' "wet" and "how deep" readings and the
     hole-closing probe read that channel and must see real liquid only.
2. **Squeezing is damped** (`cs_p2g`): a bulk viscosity, stress += `BULK` (0.7) × √stiffness × weight ×
   tr(C) on both diagonal terms, only while `j < 1 + tear`. √stiffness is the speed of sound in cells a
   second, so it is "0.7 × sound speed × one cell": a cell-sized pile-up is damped in about a step of its own
   springing, a slosh (hundreds of cells long) hardly at all, and flow that keeps its volume (tr C = 0) not at
   all. Stability: substeps keep sound-speed × dt_sub ≤ `CFL` 0.15, so ζ × dt_sub ≤ 0.105 always (the
   viscosity cap is 0.2); checked with Thickness 1 as well. Not for `j ≥ 1 + tear`: there it smeared a pour's
   thin stream into a fan.
3. **"Holes close" only closes holes** (`cs_g2p`):
   - only towards liquid on the far side of a gap: the grid's (real) volume `CLOSE_ACROSS` 2.5 cells down
     the slope, `smoothstep(0.3, 0.7)`; nothing across (the open surface) gives no move;
   - never into a wall (the move's part into each wall face of the 4 nodes round the drop is taken off), and
     faded out from 1.6 to 0.6 cells off a face, so drops on a wall's line are not slid along it.

Dropped along the way: jittering drops on a wall's line (noise in still liquid); pushing coincident drops
apart in `cs_mix` (not needed once 1 and 3 are in); laying a fill down already squeezed by its own weight.

## Measured with all three

| | before | after |
| --- | --- | --- |
| Owner's scene, fastest drop | 6,000 to 18,000 px/s | about 3,000 (the pour's own speed) |
| Owner's scene, drops with more energy than a fall gives them | 11 to 25 at a time | 0 |
| Owner's scene, stacked drops at 4 s (Fine / Extreme at 3 s) | 973 / 444+ | 11 / 45 |
| Still box, drops per square cell on the floor | 12 to 14 | 4 to 7 |
| Still box at 2 s | 45 to 50 px/s, bursts at 3 s | 7 to 11 px/s and falling, no burst |
| "Liquid water" (ring, ball drops in) at 4 s | 59 px/s, 562 stacked | 10 px/s, 12 stacked |

## Steps

1. Tests first (`crates/bpe-kit/src/things/liquid/tests.rs`), both failing on main:
   - `still_water_in_a_box_comes_to_rest_without_bursting_or_packing_against_the_floor`: a closed box filled to
     0.3, 3.5 s: no drop thrown 80 px above the level (main: 236), RMS speed under 25 px/s (main: 45 to 50),
     under 0.5 % of drops stacked (main: 6 %), the fullest half-cell layer on the floor under 2.6 × one higher up
     (main: over 3; two rows can share a layer, so 2 is no packing).
   - `water_poured_onto_a_shape_is_never_thrown_faster_than_it_could_fall`: a pour onto a filled circle in a box
     (the liquid no shape's child, as in the owner's scene), checked every frame for 2.5 s: no drop faster than
     1.5 × its spout speed plus a fall to the floor. Main: 18,105 px/s against a limit of 2,713.
2. The shader changes above. Remove every scratch hook; `sim.rs` is unchanged.
3. Run only what this touches: `cargo test -p bpe-kit --lib things::liquid -- --test-threads=2`,
   `cargo test -p bpe-gfx liquid_sim -- --test-threads=2`. Re-bless `golden_liquid` in debug and `--release`
   and check only liquid lines moved.
4. Full-size look check `LOOK_ONLY=liquid` with a new scene `liquid-pour-on-shape` (the second test's scene,
   `scenes::liquid_pour_on_a_shape`); look at the pictures (walls: no gap, no thin line; pours; lone
   drops and streams on walls still drawn).
5. `docs/notes/liquid.md`: "Spray and bouncing fixed, as built". Clippy, fmt. One review. PR, CI, merge.

## Not doing

- No more steps a second and no interpolation: measured, it does not help.
- No change to stiffness, substeps, Springiness, Size, pours or any setting; no new setting for the damping.
- Balls are not counted as full the way walls are (liquid may still pack a little round a sunk ball): its own
  piece of work if it shows.
- Drops that meet where two flows run together along a wall can still end on one spot (about 0.2 % of drops in
  the owner's scene, groups of up to 4): harmless now that nothing springs.
- No exe build or install. No new built-in scene.
