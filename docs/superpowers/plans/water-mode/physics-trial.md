# Water mode: physics core trial (throwaway, never merged)

Brief: `/mnt/project-files/ballphysics/water-mode/physics-trial-brief.md` (2026-11-24). This is the plan and the
record of what was picked. Results go in "As built and measured" at the end.

## The question

Is a core that solves the pressure so the liquid cannot squash clearly better than today's spring-pressure core
for corridors, slopes, obstacles and openings, on the owner's RX 9070 XT? Deliver a table, side-by-side clips and
a verdict (adopt / adopt with gaps / keep today's).

## What is built

A second core, "the trial core", beside today's in `bpe-gfx/src/liquid_sim/`, chosen by a process-wide switch
(`liquid_sim::set_trial`, read when a `Body` is made). Today's core is not touched: its passes, numbers and
prints stay bit for bit as on main, so any difference seen is the new physics.

- `crates/bpe-gfx/src/shaders/liquid_trial.wgsl`: the trial core's passes.
- `crates/bpe-gfx/src/liquid_sim/trial.rs`: `Trial` (the switch and its numbers), the trial pipelines, the
  trial's own buffers (`TrialScratch`), `Body::trial_passes`.
- `crates/bpe-gfx/src/liquid_sim/mod.rs`: `Body::step` runs the trial passes in place of the substep loop when the
  body's core is the trial one; `Clone` copies the kept pressure.
- `crates/bpe-kit/src/things/liquid/sim.rs::step_settings`: one substep and a higher speed cap for the trial core.
- `crates/bpe-kit/src/scenes.rs::liquid_course`: the course scene (test scenes feature).
- `crates/bpe-kit/tests/liquid_trial.rs`: the measurements (ignored, run by hand) and the side-by-side frames.

Births, the dead's removal, mixing, the walls band, the wet grid, drawing: all today's, unchanged. The drops stay
in today's 112-byte layout, so today's drawing draws both cores.

## The method, and why each choice

1. **Drops still carry everything (APIC), the grid is staggered.** Speeds across live on cell faces (u on the
   left face, v on the top face), pressure in cell middles; a drop spreads to the 4 nearest faces of each kind
   (bilinear) and to the 4 nearest cell middles for density. Why staggered: on a grid with speed and pressure at
   the same points, a projection has checkerboard pressure modes it cannot see. Why bilinear and not today's
   quadratic spline: a drop then touches only its own cell's faces, so a stream one or two cells wide stays a
   stream (today anything under about three cells is smeared, which is the owner's main use case). 20 atomic adds
   a drop a step instead of 36 a substep. Sums stay fixed-point integer atomics rounded to nearest (`fixed()`), so
   a step is the same whatever order the card runs it in.
2. **The pressure is solved, one-sided.** For every cell that holds liquid: find P ≥ 0 so the flow out of the
   cell is at least `s`, where `s` lets a cell that is not full yet fill up (`(ρ − 1) / (ρ dt)`, below 0) and
   makes an over-full one empty out (`k (ρ − 1) / dt`, above 0, capped). Why one-sided ("it cannot squash" but it
   can come apart): the usual rule "a cell with any drop in it is full liquid" makes spray and thin streams keep
   their puffed-up volume for good when they land, and a pressure that may go negative pulls sparse liquid
   together into clumps. With P ≥ 0, loose drops fall freely, pile up, and push back only once a cell is full.
   The density term is also what keeps the volume (the known weak spot: drops drifting together); it is measured,
   not assumed.
3. **Solver: red-black over-relaxed Gauss-Seidel with P clamped at 0, warm-started.** `iters` sweeps a step
   (each two passes), `omega` about 1.8. Why not conjugate gradients: its dot products need a reduction and a
   read-back or many tiny passes every iteration, and the clamp (P ≥ 0) breaks it. Why not multigrid: it is the
   right answer for big pools at Extreme and is listed as a full-build item; the trial measures how far the plain
   sweep gets (the slosh period against theory shows whether pressure reaches across the pool in time). The kept
   pressure is part of the body's state (a copy copies it), or a seek would not repeat.
4. **Walls enter as open shares of faces** (cut cells, after Batty et al. 2007): each face's open share comes from
   the walls band's signed distances at its two end nodes, measured from the plane a quarter cell off the wall's
   face (today's mirror plane, so today's drawing meets the wall the same way). A slope is then a slope to the
   pressure, not a staircase. A hollow wall thinner than a cell, which no node may fall inside, also shuts every
   face whose two cell middles are on opposite sides of its line, and drops do not spread onto faces or cells
   across such a wall, so liquid on both sides of a thin wall does not feel the other side.
5. **Free slip at walls.** Shut faces are left out when a drop gathers its speed (the rest are re-weighted), then
   the drop itself loses its speed into a wall it touches and is put back half a cell off the face, exactly as
   today. Faces with too little liquid on them (under 2 % of a drop) are left out the same way.
6. **Walls count as full liquid in the density** (today's lesson): each cell adds the share of its kernel that
   lies inside a wall or past the picture's edge.
7. **Stepping**: one projection per liquid step (1/240 s, or 1/120 s by the Liquid's Steps a second), no
   substeps. A drop's move is split into sub-moves of at most 0.4 cell with the wall check at each, so a fast
   drop cannot jump a thin wall. No read-back.
8. **Left out on purpose** (the brief): balls, forces, heat, ice, stretch, thickness, wet-wall physics. Mixing and
   the removal of the dead run as today (they only read the drops).

## Things not to do

- Do not change today's core, its shaders' arithmetic or any golden.
- Do not merge, do not ask for a review, do not build or install the app.
- Do not calm the trial core by damping or by taking speed off nodes; if still water moves, find what is out of
  balance.

## How each claim is checked (`crates/bpe-kit/tests/liquid_trial.rs`, release, by hand)

| Claim | Measure | Scene |
| --- | --- | --- |
| Speed | wall-clock seconds for a second of liquid, both cores, on the RX 9070 XT (Remote Control recipe) | Water Weird, box, course at Normal / Fine / Extreme |
| Still water rests | rms speed and the fastest drop after 5 s; stacked drops | boxes 594 / 600 / 604 wide, walls 3 / 36 / 40 px |
| Slosh | half-swing time against 0.73 s; share kept each swing | box, Level 0.3 |
| Volume | pool level (95th percentile height) and mean drops a full cell at 0, 5, 20 s | box, Level 0.6 |
| Openings | drops through each opening after 10 s; the narrowest that passes | course |
| Repeats | two runs and a seek give the same print | pour, box |
| Feel | side-by-side clips at 1080 x 1920 a side | course, Water Weird |

## As built and measured

(filled in at the end)
