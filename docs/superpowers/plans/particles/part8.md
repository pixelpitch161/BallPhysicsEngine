# Particles part 8: Huge swarm

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (section 9, section 11 part 8). Approved by the owner in the
brainstorm thread ("Both, exact engine first"); built straight after this plan with one Opus review at the end; the
thread merges its own PR once checks and the review pass. The exe is installed only once every particles part is merged.
Parts 1 (engine), 2 (air) and 5 (looks 1) are in; parts 3, 4, 6 and 7 build at the same time, so shared files are
touched additively.

## What part 8 delivers

- A switch on a Particles thing's **Look** card: **Engine: Exact / Huge swarm**. Huge swarm moves the particles on the
  graphics card: up to **4 000 000** a thing (Most particles' hard cap in this mode), decoration only.
- What a swarm does: every Source (where and when, riding, on hits, crowds, bursts) exactly as the exact engine picks
  births; the forces **Gravity, Wind (gusts and gust waves), Drag, Swirl, Wander, Rise, Sway and flutter, Follow the
  air**; meeting **balls** (push, pass / bounce off / vanish), **shapes** (pass / bounce / stop / vanish) and the
  picture's **edges** (pass / wrap / bounce / vanish).
- What it draws: the Look layers **Sprites, Streaks, More shapes and Glitter**, with Colour from (own colours over life,
  speed, order, place, start or time; or the parent ball's colour), Opacity, Twinkle, Size ×, Size over life, Fade,
  Only, Where, Blend and Blend amount, and the sub-layers **Shadow, Glow and Outline**.
- Checkpoints of the particle buffers stay in video memory (their own 2 GB budget) so scrubbing back is quick, the same
  on the same PC every time (no atomics, fixed order); export steps the same way.

## Decisions (fill-ins of the spec, stated so the review can correct them)

1. **The world drives the graphics card.** A swarm's particles live in GPU buffers owned by its `ParticlesState`
   (`swarm: Option<Swarm>`), so they ride along with everything that already carries thing states: playback
   checkpoints, seeks, the particle job's snapshots. Each particle step the thing's `after_step` works out the step's
   inputs on the CPU and submits one command buffer that steps the buffers (decision 4). Cloning a state copies its
   particle buffers on the GPU (`copy_buffer_to_buffer`, queued after every earlier step), so a checkpoint or a
   snapshot is a still copy the world's later steps never touch. Nothing is read back to the CPU while stepping.
2. **Which graphics card.** `bpe_gfx::swarm::SwarmGpu` holds a device, its queue and the compute pipelines (made once
   per device: `SwarmGpu::for_device`, a process-wide cache). The world doesn't know about devices: a thread names the
   card its worlds step swarms on with `swarm::use_on_this_thread(gpu)`, and a swarm state takes `on_this_thread()`
   when it is built. The editor names its own (eframe's) device on the UI thread in `Studio::new`, and `ParticleJob::start_if_wanted`
   hands the UI thread's card on to the job's thread; `bpe_cli::export_with` and `bpe_cli::final_frame` name theirs for
   as long as they run (`use_for_now`, a guard that puts back what was named before); tests use one shared headless device. A world built where no card was named (the
   physics-only worlds never build Particles things; kit unit tests) gets a swarm with no particles, and the Look card
   says "Huge swarm needs the graphics card" only then. The renderer draws a swarm only from buffers of its own device
   (`wgpu::Device` compares by identity); others are skipped.
3. **Births stay on the CPU, with the same dice.** `births::births` is split: `births::plan(st, cx, since, first,
   alive)` picks this step's births (counts, places, dice) exactly as before and gives them as `Speck`s; the exact
   engine pushes them under its cap as before (goldens unchanged), the swarm uploads them. The GPU appends them after
   the living particles, dropping what goes past Most particles (their birth numbers still count, as in the exact
   engine). The CPU doesn't know how many are alive, so:
   - **Keep a crowd** uploads up to `crowd_spares(count, life)` spare births a step (enough for the crowd's deaths
     at its shortest life, at least 64, at most the crowd) and the GPU takes `max(crowd − alive, 0)` of them; a crowd
     that loses more than that in one step (a ball sweeping through with Vanish) fills up over the next steps.
   - At once (Once at the start, a burst, a hit) a swarm may let off up to the swarm cap (`MOST_SWARM_AT_ONCE` =
     4 000 000) instead of the exact engine's `MOST_AT_ONCE` (100 000).
4. **One particle step on the GPU** (dt 1/120, the same `step::due` clock as the exact engine, so it keeps time at any
   Physics rate): five dispatches of 256-wide workgroups in one compute pass, each its own sync scope:
   1. `cs_births`: births `j < used` go to `count + j`, `used = min(n, cap − count)` (crowd: also `≤ crowd −
      count`); writes `count + used` (the step's working count).
   2. `cs_move`: every particle below the working count: dead when its life is over (age ≥ life), else forces in
      list order, the speed ceiling, the move, turn and flutter, balls, shapes, edges, the finite check; written to
      the scratch copy with a keep flag (as `step::move_one`).
   3. `cs_scan_local`: each workgroup's exclusive prefix of the keep flags, and its total.
   4. `cs_scan_sums`: one workgroup scans the totals (each thread a run of them, then a workgroup scan).
   5. `cs_scatter`: kept particles copied back in order (stable: oldest first, as `retain`); the new count, and the
      indirect draw arguments `[4, count, 0, 0]`.
   Integer prefix sums, no atomics, every particle written by one thread: the same result every run on one card.
5. **Particle layout** (`bpe_gfx::swarm::SpeckMain` + `SpeckMore`, 64 bytes, in two buffers so each fits the default 128 MB storage
   binding at 4 000 000): `main` = place, velocity, born (step), life, seed, turn (32 bytes); `more` = home, size,
   spin, flip, ball, two spare (32 bytes). `previous` isn't kept: the move pass has it in hand. Default device limits
   are enough (at most 7 storage buffers a stage, each under 128 MB, buffers under 256 MB).
6. **Per-step inputs** go up in one storage buffer of 32-bit words (`StepInputs`, offsets in the step's uniform): the
   balls (place, velocity, radius) and their uniform grid (`BallGrid::parts`, the exact engine's own grid, at most 32
   balls a particle), the walls as segments (decision 7) and their grid, and the air (decision 8). The births go in
   their own pair of buffers. Buffers grow by doubling and are made again with their bind groups.
7. **Shapes on the GPU are segments.** Each wall that bounces (`!passes`) becomes the closed polyline
   `light::draw::outline_points` gives, now and as it was at the last particle step (`walls_before`, the exact
   engine's own record), each segment with the wall's half thickness (0 for a filled shape). A particle's step from
   where it was to where it got crosses a segment (or ends within half thickness + 0.5 px of it): it is put back on the
   side it came from at that distance, and Bounce / Stop / Vanish as `meet_walls` does, the wall's velocity there being
   (segment now − before) / dt. A uniform grid (cells about 32 px, at most 128 × 128, at most `MOST_SEGMENTS` 65 536
   entries) over the segments' boxes; a particle looks at the cells of where it was and where it got. A free outline
   or an ellipse is as exact as its points (`ROUND_POINTS`), which is plenty for decoration.
8. **Air**: when a force follows the air, its grid (`Air::size`, `origin`, `cell_size`, `velocities`) goes up each
   step after `catch_up_to`, as part 2's plan says. `air_at` in WGSL is `Air::at`: bilinear between cell centres,
   clamped one cell in, and off the grid the ambient wind (the Air card's speed, way, gusts, gust every and waves,
   with `gust_factor`'s noise). A swarm never heats the air (no read back); its Rise's Heats the air does nothing.
9. **WGSL twins.** `hash`, `noise`, `curl`, `gust_factor`, every force, `meet_balls`, `meet_edges` and the walls'
   response are written again in `shaders/swarm_step.wgsl` with the same formulas; they agree with the CPU to float
   rounding, not bit for bit (tests compare them with tolerances).
10. **Drawing straight from the buffers.** `Frame.swarms` / `Frame.swarms_front` hold `SwarmRun { at, source,
    look, ramp, balls }` beside the sprites, as part 5's fields do: `at` is how many sprites of the same list are drawn
    before it (`cull` and `fit_lists` move it with the kept sprites). The renderer draws a run with a pipeline whose
    vertex stage (`vs_swarm`, `shaders/swarm_draw.wgsl` pasted after sprites.wgsl) reads the particle from the
    buffers, works out its sprite as `draw::seen` and `draw::sprite` do (Only, life, size over life, fade, twinkle,
    colour, shape, streak length, flutter, pose, glitter's flash) and hands it to sprites.wgsl's own vertex code
    (`sprite_out`, split out of `vs_sprite`) and fragment, so a swarm looks like the exact engine's sprites in every
    pass (the light passes see Lit 0, as part 1's particles; blend layers with the `each` pipelines). Instances come
    from `draw_indirect` on the step's draw arguments: nothing is counted on the CPU. A layer gives one run per thing
    it draws: shadows, glows, its own sprites, a glitter star, outlines, in the order `particle_with_extras` puts them.
11. **Colours on the GPU.** A run's `ramp` is the layer's Colours sampled at 256 places (with its moves at the time
    drawn); the vertex stage picks the place as `paint::offset` does for Life, Speed, Order (the particle's hashed stop),
    Place and Start (Straight, World or Screen), Time, with scroll and repeat. Other follows draw the colour
    `colour_at` gives with no inputs. Colour variation and the gradient's forms other than Straight are not on the
    GPU. Parent ball: a list of the parent's ball colours by row (`ball_colors`) goes with the run.
12. **Not in Huge swarm** (the Look card says so under the switch): Soft smoke, Flame and Web lines layers, Fine detail,
    the Recolour, Flash on hit and Colour on hit sub-layers; and whatever later parts add that needs the CPU (two-way,
    sound, rules, feeding children, settle, pile, stick, heat into the air). Those parts leave swarm things out.
13. **Checkpoints in video memory.** New `World.gpu_memory` (by thing, like `state_memory`) and `World::gpu_memory()`;
    `Playback` thins its checkpoints when their GPU bytes pass `GPU_CHECKPOINT_BYTES` (2 GB) as it does for the CPU
    budget. A swarm notes `bound × 64` bytes (`bound`: the most it can hold now, `min(cap, births so far)`; the
    buffers grow to it and copies copy only that much).
14. **Status line**: a swarm's count reaches the CPU late, through a small map-read after its steps
    (`Swarm::count_hint`, an atomic updated when the card answers); the particle job's snapshot adds it to its count.
15. **Moving between engines** rebuilds the particle playback (the switch is a Look key, part of `beside_fingerprint`
    through the thing's cards). The switch can't be keyframed.
16. **Bake samples** (`FrameMaker::samples`) step the world between samples before drawing them; a swarm then shows
    the frame's own step in every sample (no motion blur for swarms).

## Files

### `bpe-gfx`
- `src/swarm/mod.rs` (new): `SwarmGpu` (`for_device`, `use_on_this_thread`, `on_this_thread`), `SwarmSpeck`
  (`main`/`more` halves, `pack`/`unpack`), `SwarmBuffers` (the particles, counts and draw arguments; `Clone` copies on
  the GPU; `read` for tests), `StepIn` (uniform + words + births), `SwarmGpu::step`, `count_hint`.
- `src/swarm/draw.rs` (new): `SwarmRun`, `SwarmLook`, `SwarmSource`, the draw pipeline (`SwarmDraw`) and its `each`
  variants.
- `src/shaders/swarm_step.wgsl`, `src/shaders/swarm_draw.wgsl` (new).
- `src/shaders/sprites.wgsl`: `vs_sprite` calls the new `sprite_out(vi, v)` (no change in what it draws).
- `src/frame.rs`: `Frame.swarms`, `swarms_front`; clear, `draws_nothing`, `has_overlay`, `fit_lists_to`, `cull_through`
  (`cull_sprites` also moves swarm runs' `at`).
- `src/lib.rs`: the renderer draws swarm runs among the sprites (`draw_sprites`), with `each` pipelines in blend
  layers; `pub mod swarm`.
- `src/swarm_tests.rs` (new): GPU tests.

### `bpe-runtime`
- `world.rs`: `World.gpu_memory`, `World::gpu_memory()` (copied in clones, cleared in `lone`).
- `playback.rs`: `GPU_CHECKPOINT_BYTES`, thinning by either budget.

### `bpe-kit`
- `things/particles/look.rs`: `Engine` (Exact / HugeSwarm), `LookCardSettings` (the card's own setting `engine`),
  `ParticlesLookKind::settings`, `engine_of`, `in_swarm(kind)` (which layer and sub-layer kinds a swarm draws).
- `things/particles/mod.rs`: Most particles' hard range 1..4 000 000; `MOST_SWARM`; the exact engine clamps to
  `MOST_EVER`.
- `things/particles/births.rs`: `plan` (births worked out, not pushed), `births` pushes them; `crowd_spares`.
- `things/particles/meets.rs`: `BallGrid::parts` (its arrays for the GPU).
- `things/particles/swarm.rs` (new): `Swarm` in the state, `inputs` (forces, meets, balls, walls, air, edges into a
  `StepIn`), `step` (one particle step), `memory`.
- `things/particles/state.rs`: `ParticlesState.swarm`, `engine`.
- `things/particles/sim.rs`: build/after_build/after_step take the swarm path when the engine says so.
- `things/particles/draw.rs`: in a swarm, each shown layer pushes its runs (`swarm_runs`).
- `things/particles/swarm_tests.rs` (new), `tests/swarm.rs` (new, GPU).

### `bpe-studio`
- `app.rs`: `Studio::new` names eframe's device on the UI thread (`use_on_this_thread`).
- `particle_job.rs`: hands the UI thread's card to the job's thread; the snapshot counts a swarm's `count_hint`.
- `look_editor/particles.rs`: the Engine chips at the top of the Look card and the "not in Huge swarm" line; layers a
  swarm doesn't draw are marked in the layer list ("not drawn in Huge swarm").

### `bpe-cli`
- `lib.rs`: `export_with` and `final_frame` name their device on their thread.

## Steps and how each is verified

1. **GPU step** (`bpe-gfx` swarm): `swarm_tests.rs` with the shared test device: births append and the cap drops the
   rest; life ends remove particles and the rest keep their order (a stable compaction over three workgroups and
   more); gravity moves a particle as semi-implicit Euler does (within 1e-3 px over a second); a clone is a still copy
   (stepping the original doesn't change it); the same steps twice give byte-identical buffers; 300 000 particles step
   (more than one scan run).
2. **Runtime**: a world's `gpu_memory` counts; a playback over the GPU budget thins its checkpoints (unit test with a
   fake state noting GPU bytes).
3. **Kit**: `tests/swarm.rs` (GPU): each force against the exact engine's result for one particle (tolerances);
   balls push and touch; a shape bounces and no particle gets through a ring wall in 5 s; edges wrap/bounce/vanish;
   follow the air; Keep a crowd stays at its count; a seek back and forth equals a straight run (read back, hashed);
   main vs whole world balls unchanged; no device → no particles, no panic. Exact-engine goldens unchanged (`cargo
   test -p bpe-kit --test golden_particles`), ball goldens unchanged.
4. **Draw**: pictures test (GPU): a swarm of sprites draws where its particles are; a swarm's Sprites look like the
   exact engine's for a still crowd (mean colour within a tolerance); Glow adds light round them; `cull` keeps runs
   between the same sprites.
5. **Editor** (kittest): the Engine chips switch the card's `engine`; the line about what Huge swarm leaves out shows
   only in Huge swarm; the particle job publishes a swarm with a count.
6. **Bench** (ignored): 1 000 000 particles with gravity, wind and swirl, the step time printed.
7. `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx lib tests with `--test-threads=4`),
   release goldens, CLAUDE.md map, the Windows exe builds.

## Not doing (part 8)

- No two-way, sound, rules, children, settle/pile/stick, heat into the air for swarms (spec section 9).
- No GPU versions of Soft smoke, Flame, Web lines, Fine detail or the colour sub-layers; no colour variation.
- No change to the exact engine's results, the existing `layer.particles`, or any golden; no built-in scenes.
- Part 3 merged first, so part 8 gives its forces swarm codes (see As built); Flock stays exact-only.

## As built

- `bpe-gfx/src/swarm/mod.rs`: `SwarmGpu` (`for_device` cached per device; `use_on_this_thread`, `on_this_thread`,
  `use_for_now` → a guard restoring the card named before), `SpeckMain`/`SpeckMore` (32 bytes each), `ForceGpu`,
  `StepGpu`, `StepIn { step, words, births }`, `SwarmBuffers` (`step`, `ensure_room`, `source`, `count_hint` through a
  late async map of the counts buffer, `read` for tests; `Clone` copies on the card). Room grows by powers of two from
  1024 up to 4 194 304. `STEP_STILL` (32) makes `cs_move` copy without moving: a swarm's first births at build are
  placed, not stepped, as the exact engine's `step::first` does.
- `bpe-gfx/src/swarm/draw.rs` + `shaders/swarm_draw.wgsl`: one uniform `SwarmLook` a run (kinds, timing, draw numbers,
  paint, camera, sub-layer numbers, a 256-sample colour ramp and a 64-sample sub-layer ramp) in group 1 with the two
  particle buffers and the parent's ball colours; roles own, star (glitter), shadow, glow, outline. `vs_swarm` ends in
  sprites.wgsl's `sprite_out`, so a swarm sprite is shaded by the same fragment code as any sprite. `draw_sprites`
  interleaves sprite runs, fields and swarm runs by `at`.
- `bpe-kit/src/things/particles/swarm.rs`: `Swarm`, `first`, `step_in`, the force/wall/ball/air uploads; walls are
  segments on a uniform grid (≤ 128 cells a side, ≤ 65 536 entries, cells at least 32 px).
- In both engines Keep a crowd ends a step below its crowd by that step's deaths (births come before deaths); the tests
  compare the swarm's crowd with the exact engine's rather than with the crowd itself.
- The editor's Look card for a Particles thing starts with Engine chips (Exact / Huge swarm); in Huge swarm a faint
  line says what it leaves out, and a picked layer a swarm doesn't draw says so in red.
- Tests: `bpe-gfx/src/swarm/tests.rs` (7), `bpe-kit/src/things/particles/swarm_tests.rs` (9, against the exact
  engine), `bpe-runtime` playback budget test, the editor's Engine chips test, and an ignored bench
  (`swarm_tests::bench_a_million`).
- Part 3's forces on the card (it merged while this part was building): Pull to / Push away from (every Toward,
  Swallow), Spring home (a particle's `SpeckMore.spot` names its word, picture or grid place; the homes' centre, turn
  and places go up each step), Magnetic field, Flow field (all four patterns), Spirograph, Pulse on the beat (the
  step's kick share worked out on the CPU by `pulse_share`) and Ripple from hits (each force's rings in the words);
  Only for A share / The rest. What they read comes from part 3's own `Ready::new` each step (`swarm::put_ready`,
  `StepGpu.middle/parent/homes/lists` and a second ball grid `fballs`). **Flock** isn't on the card (it needs every
  particle's neighbours) and a swarm skips it; a swarm's word/picture/grid crowd is filled at the start and not
  refilled (the CPU doesn't know which places are free). The editor says both. Tested against the exact engine
  (`part_3_forces_move_a_swarm_as_the_exact_engine_does`, `ripples_ball_pulls_and_magnets_…`,
  `a_swarm_springs_home_to_its_grid_and_skips_a_flock`).
- Review fixes: the Look's Engine is in `beside_fingerprint` (`ThingSim::beside_card_key`), so switching it restarts
  the particle playback; a word/picture/grid crowd isn't capped by Count on the card; swarm births are planned at most
  Most a step; `Swarm.buffers` is an `Arc` shared by clones and copied on the card only when a shared one steps
  (`Arc::make_mut`), so snapshots and the preview's borrowed states cost nothing; the bound comes down with the count
  read back (plus births since); lowering Most never shrinks the bound under the live particles; a failed count read
  frees the reader; the video-memory checkpoint budget is 1 GiB.
- Parts 4 and 7 merged while this part was building. A swarm leaves their CPU-only features out: two-way (`in_main`
  and `step::two_way` say no for a swarm), feeding (a swarm is never fed and feeds nothing), Stick (bounces off), Pop
  (vanishes), Settle and Pile up (stop), the Piles layer (not drawn), moments and their sound, and part 7's lighting
  of particles (swarm sprites draw unlit). `births::plan` is now `births_for(.., For::Swarm)` with the swarm's own
  particles out of `st.specks` (which is empty in a swarm); the exact path is `births` unchanged, and it notes no
  moments for a swarm.
