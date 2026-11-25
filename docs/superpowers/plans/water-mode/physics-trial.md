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
- `crates/bpe-kit/tests/liquid_trial.rs`: the course scene (`course`), the measurements (ignored, run by hand)
  and the side-by-side frames.

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
3. **Solver: multigrid over red-black sweeps, P clamped at 0, warm-started.** Two V-cycles a step: two sweeps
   on the cells, what they still lack summed onto a grid half as fine (2 x 2 cells to one), and so on down to a
   handful of cells, then each coarser answer added back on the way up with two more sweeps. Why not conjugate
   gradients: its dot products need a reduction every iteration, and the clamp (P ≥ 0) breaks it. Why not plain
   sweeps (the first try): a pool needs about as many sweeps as it is cells deep, every step, or it does not
   rest (30 sweeps left a 67-cell pool heaving at 50 px/s; 120 made it dead still; Extreme would need 600).
   Three things the multigrid needed, each found by it failing: a coarse face is half the sum of the fine faces
   that cross it, but the way out to the air is not halved (its middle is only a quarter-cell further from the
   surface), or every level puts the surface too far off and the pressure swings; a coarse cell's answer has a
   floor (it may not ask any cell under it for less than no pressure), or liquid in free fall, whose cells all
   lack "less than nothing", wipes out every pressure; and a cell at its floor that would rather have less hands
   down no lack. The kept pressure is part of the body's state (a copy copies it), or a seek would not repeat.
4. **Walls enter as open shares of faces** (cut cells, after Batty et al. 2007): each face's open share comes from
   the walls band's signed distances at its two end nodes, measured from the plane a quarter cell off the wall's
   face (today's mirror plane, so today's drawing meets the wall the same way). A slope is then a slope to the
   pressure, not a staircase. A hollow wall thinner than a cell, which no node may fall inside, also shuts every
   face whose two cell middles are on opposite sides of its line, and drops do not spread onto faces or cells
   across such a wall, so liquid on both sides of a thin wall does not feel the other side. A band node that
   lies exactly on a wall's line is never trusted for the side of another point: its normal may point either way
   (`Outline::nearest`), which once let a whole pool out through one wall of the 600-wide box.
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

All of it is on the trial's branch only. The test is `crates/bpe-kit/tests/liquid_trial.rs`; every number of the
trial core is a `BPE_TRIAL_*` environment variable, so tuning needs no rebuild.

### In the cloud (software card), Detail Normal unless said

| Measure | Today's core | Trial core |
| --- | --- | --- |
| Still boxes (594/3, 604/3, 600/36, 600/40) after 5 s: rms speed | 5.5, 8.6, 8.4, 5.6 px/s | 0.3, 0.1, 0.7, 3.3 px/s |
| the same: thrown up at most | 3, 15, 15, 2 px | 0 px |
| Deep still pool (Level 0.6), level after 1 s and 20 s | sinks 19 px, then 16 px | sinks 3 px, then 3 px |
| the same: rms speed after 1 s and 20 s | 33 px/s, 40 px/s | 0.1 px/s, under 0.05 px/s |
| Slosh half swing (theory 0.73 s) | 0.78 s | 0.77 s |
| Slosh kept each half swing | 0.86 to 0.96 | 0.80 to 0.93 |
| Course, 6 s: drops outside the outer box | 600 of 11935 (5 %) | 0 |
| Course, 6 s: through the 12 px opening (1.5 cells) | 0 | 499 |
| Course, 6 s: through the 24 px opening | 946 | 875 |
| Course, 6 s: left in the top pool (theory about 15 %) | 40 % | 20 % |
| Course, 30 s | not run | all drained: 4219 through 12 px, 7455 through 24 px, 0 outside |
| Repeats (two runs, played, sought back and played) | the same | the same |
| Wall time, course, 6 s of liquid | 151 s | 34 s |
| Wall time, 361 frames of the course at Fine, 540 x 960 | 759 s | 193 s |
| Wall time, 361 frames of Water Weird at Fine | 498 s | 147 s |

The deep pool at Fine and Extreme on the trial core: a second of it at both took 29 s together; rms 4.5 px/s
(Fine) and 1.1 px/s (Extreme) after 1 s.

Today's core's prints (`liquid_speed`, pour and box at Normal, 1 s) are unchanged by the branch:
`7e100902fef20dc9`, `7cccd7f6eeae0ca6`.

### At full size (the look)

Pools, walls and openings look right on the trial core. **Thin streams and films in the air do not**: on Water
Weird the thin pour, once it has struck a shape, breaks into lumps and a rain of small drops, some in level rows,
where today's core keeps clean streams that hug the shapes. On the course the stream out of the corridor and
the ones under the small openings are ragged strands. Today's core has no separate hold-together force to port:
its streams hold because each drop is spread over three cells and its pressure is a spring. The trial core's
cells push only when they are full, so anything thinner than a cell is loose drops.

Two quick tries, neither enough: letting a cell's pressure go a little below nothing (`BPE_TRIAL_TENSION`), which
pulls drops into cell-sized lumps and not into streams; and a weaker push-out (`BPE_TRIAL_PUSH_OUT=0`), which
changed nothing, so the rain is not the density term kicking.

### On the owner's card

AMD Radeon RX 9070 XT, Vulkan, driver 26.11.2 (LLPC), the Windows exe built on the PC from 0755f3c8 (the
tension knob added later is off by default, so the same core). Physics only (no drawing), 5 s of scene time,
run twice with no check running on the PC; both runs gave the same prints and wall times within 0.1 s.
The second run's numbers, as wall seconds for one second of scene (1.00 = real time):

| Scene | Detail | Drops | Today | Trial | Trial is |
| --- | --- | --- | --- | --- | --- |
| Water Weird | Normal | 5 091 | 0.15 | 0.08 | 1.8 x faster |
| Water Weird | Fine | 9 050 | 0.23 | 0.08 | 2.8 x |
| Water Weird | Extreme | 36 203 | 0.45 | 0.11 | 4.0 x |
| Still box | Normal | 19 832 | 0.19 | 0.08 | 2.5 x |
| Still box | Fine | 34 888 | 0.39 | 0.08 | 4.6 x |
| Still box | Extreme | 141 768 | 1.24 | 0.15 | 8.1 x |
| Pour | Normal | 4 772 | 0.14 | 0.08 | 1.7 x |
| Pour | Fine | 8 485 | 0.19 | 0.08 | 2.3 x |
| Pour | Extreme | 33 941 | 0.52 | 0.11 | 4.9 x |
| Course | Normal | 11 935 | 0.23 | 0.08 | 2.9 x |
| Course | Fine | 21 170 | 0.45 | 0.09 | 5.1 x |
| Course | Extreme | 84 680 | 1.19 | 0.15 | 8.2 x |

The trial core's cost hardly grows with Detail (0.08 to 0.15), where today's grows 3 to 6 times; at Normal the
trial's 0.08 is mostly the fixed cost of the passes, not the drops. Still box, mean speed squared after 5 s:
today 2234 / 3134 / 2683 (about 47 to 56 px/s rms), trial 0 / 1 / 0.

The rest of the card's runs (each started after 90 s with no check on the PC and watched for one starting; the
course run's first try had one start and was thrown away, its counts were identical).

Speed over 20 s (the pours have filled more by then), wall seconds a second of scene:

| Scene | Core | Normal | Fine | Extreme |
| --- | --- | --- | --- | --- |
| Water Weird (20 364 / 36 203 / 144 815 drops) | today | 0.24 | 0.44 | 1.26 |
| | trial | 0.08 | 0.09 | 0.15 |
| Course (11 935 / 21 170 / 84 680 drops) | today | 0.29 | 0.51 | 1.40 |
| | trial | 0.08 | 0.09 | 0.15 |

At half rate (`BPE_TRIAL_RATE=120`, 5 s) the trial core ran at 0.04 to 0.08 and the box stayed still (mean
speed squared 0 / 20 / 1); the look at half rate was not checked.

Still boxes (Level 0.3; boxes 594/3, 604/3, 600/36, 600/40) after 5 s, rms px/s:

| Core | Normal | Fine | Extreme |
| --- | --- | --- | --- |
| today | 5.4, 8.3, 7.8, 5.8 | 77.1, 52.5, 4.9, 10.7 | 16.5, 15.5, 5.6, 12.5 |
| trial | 0.4, 0.1, 0.6, 3.3 | 0.1, 0.4, 1.4, 0.2 | 1.2, 1.8, 2.8, 0.9 |

Today's threw drops up 2 to 34 px; the trial 0 px. The trial has more drops sharing a place in these shallow
boxes (crowded 70 to 1182 against 2 to 506).

Deep pool (Level 0.6, about 534 px deep) over 20 s:

| Measure | Core | Normal | Fine | Extreme |
| --- | --- | --- | --- | --- |
| Top sinks by, after 20 s | today | 16.4 px | 18.8 px | 25.6 px |
| | trial | 2.6 px | 9.4 px | 1.7 px |
| Drops a full cell (4.00 at the start) | today | 4.11 | 4.08 | 4.19 |
| | trial | 4.01 | 4.00 | 4.00 |
| rms speed at 20 s, px/s | today | 31.6 | 62.6 | 28.7 |
| | trial | 0.0 | 1.8 | 0.6 |
| Crowded at 20 s | today | 1452 | 4100 | 7923 |
| | trial | 45 | 443 | 363 |

The trial lost one drop at Fine and one at Extreme (34 888 to 34 887, 141 768 to 141 767); where it goes is not
known.

Slosh (theory 0.73 s a half swing): today 0.78 / 0.82 / 0.80 s, trial 0.77 / 0.78 / 0.78 s. Swing left after
four half swings: today 69 / 70 / 71 %, trial 53 / 50 / 52 %. **The trial loses a slosh about a third sooner.**

Course after 30 s (drops; "held up" is the top pool, the way down and the bottom pool together):

| Measure | Core | Normal | Fine | Extreme |
| --- | --- | --- | --- | --- |
| Through the 12 px opening | today | 85 | 452 | 19 209 |
| | trial | 4 270 | 7 021 | 28 206 |
| Through the 24 px opening | today | 7 058 | 9 725 | 55 219 |
| | trial | 7 459 | 13 374 | 53 987 |
| Held up | today | 1 764 | 10 890 | 9 881 |
| | trial | 206 | 741 | 2 486 |
| Outside the outer box | today | 3 028 | 103 | 371 |
| | trial | 0 | 34 | 1 |
| Wall seconds for the 30 s | today | 8.9 | 15.3 | 45.9 |
| | trial | 2.6 | 2.9 | 14.2 |

The trial's shares hardly change with Detail (12 px: 36 / 33 / 33 %; 24 px: 62 / 63 / 64 %); today's do
(12 px: 0.7 / 2 / 23 %). One timing does not fit: the trial at Extreme took 14.2 s here (4.6 s of it between
10 s and 15 s of scene, 4.1 s between 15 s and 20 s) where the straight 20 s speed run of the same scene took
2.99 s. The trial core's work a step is fixed, so the likely cause is something else using the card then (the
owner was at the PC); not confirmed.

Repeats at Fine (pour and course): both cores gave the same print for two runs, a played run, and a run sought
back and played again.

Frames with drawing at 1080 x 1920, Extreme, made while checks ran on the PC: course 481 frames, today 364 s,
trial 412 s; Water Weird 601 frames, today 443 s, trial 352 s. Drawing and writing the pictures, not the
physics, is most of that time.

The PC's scratch build and temporary folder were deleted afterwards; nothing was installed.

### Verdict

Adopt, with named gaps. The gap that matters is thin streams (above): a full build needs a real hold-together
and stream part before it replaces today's core, with "Water Weird's pour looks as clean as today's" as its
test. If that part fails, keep today's core.

The card's numbers confirm it: 2 to 8 times faster and faster than real time at every Detail, still pools at
rest, pools that keep their height, walls that hold, a 12 px opening that flows at every Detail, and answers
that hardly change with Detail. Smaller gaps: a slosh dies about a third sooner; 34 of 21 170 drops crossed
walls on the course at Fine; one drop went missing in the still pool at Fine and Extreme; drops share places
more in shallow still pools. The owner's page: https://claude.ai/artifact/6sgerUCgwjRnvsX1ZWRjBR

### What a full build still needs (rough sizes)

| Item | Size | Note |
| --- | --- | --- |
| Hold-together, thin streams and films, hugging walls | large, the main risk | not a port; needs its own design (a surface force, or drops that smooth their own spacing) |
| Moving walls (keyframed and water-driven) | medium | wall speeds are already in the band; the faces' open shares change each step; two-way push needs the pressure on a part summed |
| Thick liquids (honey) | medium | a second solve for the speeds (viscosity); not tried |
| Slime and ice | medium to large | slime wants stretch, which today's core has and this one does not; ice as today (drops that stop) |
| A livelier slosh | small to medium | the bilinear hand-over between drops and grid loses swing; a wider kernel or a share of the drop's own speed |
| Repeating exactly on other machines | small to medium | same on one machine now; across cards it needs the same care as today's sums (fixed-point is kept) and no fused arithmetic |
| Skipping empty tiles, half-rate steps | small | speed only |
| Balls, forces, heat, mixing, wet walls | as today's, each small to medium | they read or push the drops; the grid passes differ |
