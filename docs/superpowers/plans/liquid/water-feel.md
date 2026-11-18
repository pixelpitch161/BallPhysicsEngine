# Liquid fixes, part A: water that feels like water

Design: the liquid review (2026-11-17, report https://claude.ai/artifact/HpzbSCF5chBk44hy2rVxrs, handover
`/mnt/project-files/ballphysics/liquid-thing/review/handover.md`). Aedan approved "Both, feel first" on 2026-11-17,
so this plan is built without approval stops. Part B (lighting and look) follows in its own thread.

## What was wrong (from the review's renders on main b850811)

- **W1 time scale.** The liquid falls at the scene's gravity (500 px/s² by default) in grid cells. On screen a
  ring 800 px across then acts like ~20 m of water: one slosh takes ~6 s, which reads as thick and slow. Liquid
  Gravity × 8 looked right in the review's pour test.
- **W2 voids never close.** `cs_p2g` gives a drop no pressure at all once its volume ratio J (= 1 / the grid's
  volume around it) passes `1 + tear`, so a cavity behind a pour or a ball, or a crack, has walls that never
  pull back together.
- **W3 lone drops are never drawn.** A drop's splat peaks at 1 but the Edge level inside a liquid is π reach² ×
  Edge (≈ 3.5), so a drop on its own, or a thin stream at Very fine, is invisible.
- **W4 the settings barely matter.** Thickness 0 / Settles 0 / Springiness 1 look about like the defaults.

## Design decisions

1. **Size** (new `LiquidSettings::size`, Choice `LiquidSize`: Glass, Bucket (default), Bath, Pool; group "How it
   flows", `simple`, no keyframe). How big the liquid is in real life: smaller liquids move faster. Each size
   multiplies the liquid's own gravity: Glass × 16, Bucket × 8, Bath × 3, Pool × 1 (Pool is how liquids moved before).
   - `sim.rs::step_settings`: the liquid's gravity factor (`lq.b.z`) and its stiffness use `gravity × size`.
   - The push on balls (`lq.e.x`) is divided by the size factor, so a ball's float/sink by Ball weight stays
     as before (the liquid's pressure grows with its gravity).
   - Pours: speed × √size (and the births per second with it), so a pour's arc keeps its shape on screen
     (an arc's shape goes with speed² / gravity) and the stream stays as dense.
   - Cost: substeps go with √(stiffness) so Bucket is ~2.8× the steps of Pool; `MOST_SUBSTEPS` (64) still caps
     it (the stiffness is then capped as today).
2. **Closing voids (W2).** Pressure for J > 1 depends on how open the drop is (`exposure(vd)`): a drop with
   liquid nearly all round it (low exposure) gets the full equation of state (it pulls back like it pushes),
   up to a small cap, so cracks and holes inside close; at the open surface it keeps today's gentle Surface
   pull and tear. The exact ramp is chosen by renders (pour into still water and a ball dropped in: holes and
   cracks gone within ~0.5 s, the surface not lumpy or beaded). Exposure needs the drop's vd in `cs_p2g`: it is
   `1 / p.j` (p.j is set from vd in `cs_g2p`).
3. **Lone drops drawn (W3).** `liquid_drops.wgsl vs_drop` reads the drop's `j` (float 16) and gives a drop
   with little liquid round it (vd = 1/j under ~0.35) a stronger splat, so its own peak passes the Edge level
   (`st.a.x`) by ~1.6× and it draws as a small round droplet. Field and colour weights are scaled together, so
   colours are unchanged. Drops inside the liquid are unchanged.
4. **Water defaults (W4).** After 1–3, set water-like defaults (Thickness lower, Surface pull lower, Spray
   higher, Settles lower) by renders, and check with renders that Thickness, Settles and Springiness each
   change what is seen.

## Steps

1. Size: the enum, the setting, `factor()`, default Bucket; `step_settings`, push; `births::pour`.
2. W2 in `liquid_sim.wgsl::cs_p2g`; renders of pour (6-pour scene) and ball drop.
3. W3 in `liquid_drops.wgsl`; render of a pour at Very fine and of spray.
4. W4 defaults; renders of each setting at two values.
5. Tests: `cargo test -p bpe-kit liquid`, `-p bpe-gfx liquid_sim -- --test-threads=2`, `restarts`, re-bless
   `golden_liquid` (debug and release). Floats/sinks tests must still pass unchanged (push scaling).
6. Full-size look check `LOOK_ONLY=liquid` and look at the pictures.
7. `docs/notes/liquid.md` "Water feel, as built". Clippy, fmt. One Opus review. PR, CI, merge.

## Not doing

- No lighting or look changes (Part B). No new built-in scenes. No exe build.
- Not changing scene gravity's default or the ball physics.

## Verify

- Renders: pour into still water (holes close, no cracks), slosh period of a Bucket ring ~1–1.5 s, spray
  droplets visible, Very fine pour visible as a stream, each feel setting visibly different.
- `tests_balls` floats/sinks pass untouched. Goldens: only liquid lines move.
