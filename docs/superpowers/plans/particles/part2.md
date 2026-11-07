# Particles part 2: the air

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 3.3 Wind/Rise/Follow the air, 5, 8, 11 part 2).
Part 1 (`part1.md`, merged at e36d189) is the base. Approved design; built straight after this plan with one Opus review
at the end; the thread merges its own PR once checks and the review pass. The exe is installed only once every
particles part is merged. Parts 3 to 7 build in parallel: every shared file is touched additively.

## What part 2 delivers

- An **Air card** on the scene settings (`SlotId::Air`, card kind `air`): Wind (speed, direction, gusts, gust waves),
  Calms down, Resolution (Coarse / Normal / Fine: 32 / 16 / 8 px cells), Balls stir and Stir reach, Heat rises, Heat
  fades, Keeps its swirls, Shapes block the air, Margin.
- **The air**: a 2D grid fluid over the camera's view plus a margin (`bpe-kit/src/air.rs`, `Air`), stepped at
  120 Hz in lock step with the particle steps. Balls stir it, shapes are solid cells (moving with their wall), heat
  rises, it calms down towards the card's wind. Semi-Lagrangian advection, 20 Jacobi pressure iterations,
  deterministic, the same with 1 thread or many.
- **Follow the air** (`force.air`): particles are carried by the air (Amount, Grip).
- **Rise** gains **Heats the air**: rising particles warm the air where they are, so the air itself rises (smoke
  plumes that drag their neighbours up).
- **Wind** gains **Gust waves**: gusts travel across the picture as waves instead of the whole picture at once.
- Editor: the Air card in the scene settings (added by itself when a Follow the air force is added), the **Air**
  toolbar toggle (Show air: faint arrows of the air at the playhead, never in the video).
- Looks: **Dust clouds** (5), **Thick fog** (15, the part 2 form: an area crowd carried by the air; Spring home and
  two-way drag come with parts 3 and 4), and **Sandstorm** (21) made full: gust waves and a haze layer.

## Decisions (fill-ins of the spec, stated so the review can correct them)

1. **Where the air lives.** One `Air` per world, kept in `World.states` under `ThingId::ROOT` (the scene's state), so
   checkpoints, copies, `lend_states` and the particle job's snapshots carry it with no new machinery. The scene sim
   puts a *sleeping* `Air` there (its settings only, no grid) when the scene has an Air card, and re-reads the card in
   `settings_changed` (Air keys). A Particles thing **wakes** it in `after_build` (allocates the grid, creating it with
   the default settings when there is no card) when the thing follows the air or heats it, or when the scene has an
   Air card (so Show air has something to show). The editor's main world builds no Particles things, so it never has a
   grid: the air costs nothing there.
2. **Lock step.** Nothing steps the air on its own. Before each particle step *k*, the Particles thing brings the air
   to step *k* (`Air::catch_up_to(k, world)`): the first thing in tree order does the work, the others find it there.
   The air at step *k* reads the world as it stands (balls, walls, camera) and the heat put in during step *k − 1*;
   heat a thing puts in during step *k* waits for step *k + 1*. So every thing reads the same air, whatever their
   number or order, and it keeps time at any Physics rate (it follows `step::due`).
3. **The grid** is a lattice anchored in the world (cell (i, j) covers `[i·c, (i+1)·c) × [j·c, (j+1)·c)`), so it
   never resamples: each air step it covers the camera's shot bounds (`births::screen`'s box) grown by Margin; when the
   camera moves or zooms, overlapping cells are kept and new ones start as the ambient wind with no heat. At most
   `MOST_CELLS` 160 000 cells (400 × 400); a bigger view takes the middle and the rest reads the ambient wind.
4. **Ambient wind** at a place and time: the card's wind × the gust factor `1 + gusts × noise(time / every − along /
   waves, 3.7)` (along = place · wind direction; waves 0 = no travel, the whole picture gusts at once, exactly part 1's
   Wind maths). Outside the grid the air *is* the ambient wind. The Wind force uses the same helper
   (`gust_factor`), so part 1's goldens don't move.
5. **One air step** (dt 1/120): (a) calm towards the ambient wind, `u = w + (u − w)·e^(−calm·dt)`; (b) balls stir: in
   every cell whose centre is within Stir reach × r of a ball, `u += (v_ball − u)·(1 − e^(−24·stir·f·dt))`, f =
   (1 − d/(reach·r))² (balls in body-row order); (c) heat: pending heat added, buoyancy `u.y −= rises·T·dt`, `T ·=
   e^(−fades·dt)`; (d) swirls kept (vorticity confinement, strength Keeps its swirls); (e) advect `u` and `T`
   semi-Lagrangian (back-trace by `u·dt`, bilinear; outside the grid: ambient, no heat); (f) solid cells take their
   wall's velocity; (g) project: divergence, 20 Jacobi iterations of pressure (solid neighbours mirror the cell's
   own, the grid's border is open: pressure 0 outside), subtract the gradient on open cells. Collocated cell-centred
   grid; a little checkerboard is acceptable for decoration. Every value written is checked finite (else ambient) and
   speeds clamp to the world's ceiling; heat clamps to `[0, MOST_HEAT]` (1000).
6. **Solid cells**: with Shapes block the air on, a cell is solid when a wall that doesn't let balls pass covers its
   centre: inside a filled wall, or within half the wall's thickness plus half a cell of its centre line. Its
   velocity is the wall's at the cell (`Wall.velocity + wall_velocity(…)`). The mask is worked out again only when an
   outline or the grid's place changes (kept as the outlines it was made from).
7. **Threads**: advection and the Jacobi iterations split the rows into fixed bands on `std::thread::scope` when the
   grid has at least `THREADED_CELLS` 32 768 cells; every cell's new value reads only the previous buffers, so any
   thread count gives the same air (a test runs 1 and 4). Balls stir in a fixed order on one thread.
8. **Sampling**: `Air::at(p, time)` is the bilinear velocity at `p` (cell centres; outside the grid the ambient wind),
   `Air::heat_at(p)` likewise. Heat goes in with `Air::add_heat(p, amount)` into the nearest cell's pending heat.
9. **Follow the air** (`force.air`, `FollowAirForce`: Amount, Only for, Age, Grip 4 per s): `v += (air(p) − v)·(1 −
   e^(−grip·amount·dt))`. With no air (the thing was built in a world without one) it does nothing.
10. **Heats the air**: a Rise setting (`heats`, 0 by default, span 0..1, unit "per s"): after the move, in particle
    order on one thread, every live particle a heating Rise acts on adds `heats × cool(age) × amount × dt` to its
    cell's pending heat (so "1" is one unit of heat a second a particle). Heat units are arbitrary; Heat rises is
    px/s² per unit.
11. **Gust waves** on Wind: `waves` ("Gust waves", px, 0 = all at once, span 0..2000, shown when Gusts ≠ 0): the
    distance between gusts as they travel along the wind.
12. **Fingerprints**: the Air card is read only by things stepped beside the physics. `world::BESIDE_SLOTS = [Air]`:
    `physics_animation` keeps Air keys (so the particle world animates them), `fingerprint_of` leaves Air addresses
    out, and `beside_fingerprint` hashes the root's Air card (params) and its keys. An Air edit restarts only the
    particle playback; the balls and the sound never restart (a test).
13. **Air card added by itself**: `things::particles::air::ensure_air_card(scene, reg)` (no-op when there is one).
    `add_particles` calls it for looks that follow the air; the editor's Add layer edit calls it when the layer added
    is `force.air`. A Rise that heats doesn't add the card (the air is woken anyway; the card can be added from "+ Add
    slot" to tune it).
14. **Show air** (`Studio.show_air`, editor only, not saved; an "Air" toggle in the toolbar beside Grid, shown while the
    scene has a Particles thing): the preview draws faint arrows (`preview/air_view.rs`) from the air in the particle
    job's latest snapshot (the ROOT state is put in the snapshot), on a lattice about 36 screen points apart inside the
    view, each from its point along the air's own velocity, length `speed × 0.08 s` in video px capped at 30 screen
    points, opacity by speed. Arrows use `style` tokens only.
    With no awake air nothing is drawn (the toggle's hover says what it is for). Arrows are drawn only when the
    snapshot is the playback's step (as particles are).
15. **SlotId::Air** goes last in `SlotId::ALL` (after Health), so no slot's place moves; the scene kind's `slots()` gains
    it. Scene format stays 12 (spec: no bump).

## Files

### `bpe-core`
- `ids.rs`: `SlotId::Air` (last), `name` "Air", ALL 11; test updated.

### `bpe-runtime`
- `world.rs`: `BESIDE_SLOTS`; `physics_animation` keeps them; `fingerprint_of` skips their addresses;
  `beside_fingerprint` hashes them.

### `bpe-kit`
- `air.rs` (new; in the kit, not the runtime as the spec guessed: it needs the kit's noise for gusts, and only kit
  things read it): `AirSpec`, `Air` (`new`, `wake`, `awake`, `catch_up_to`, `at`, `heat_at`, `add_heat`, `size`,
  `origin`, `cell_size`, `velocities`, `cells`), `gust_factor`, `air_of`/`air_mut`/`take_air`/`put_air`, the solver;
  `air/tests.rs`.
- `modules/air.rs` (new): `AIR` ("air"), `AirSettings` (the card), `Resolution`, `AirKind` (`ModuleKind`, fits
  `scene`), `AirSettings::spec()`, `spec_of(resolved root)`.
- `modules/mod.rs`, `lib.rs` (register `AirKind`).
- `things/scene/mod.rs` (`slots()` gains Air), `things/scene/sim.rs` (sleeping air at build, re-read in
  `settings_changed`).
- `things/particles/air.rs` (new): `ensure_air_card`, `wants_air_card`.
- `things/particles/forces.rs`: `force.air` (`FollowAirForce`, `Force::Air`), Rise `heats` (`heat_of`,
  `heats_air`), Wind `waves` (through `crate::air::gust_factor`), `follows_air`, `ForceCx.air`, `OnlyFor::applies`.
- `things/particles/step.rs`: `catch_up`/`step` bring the air along and put heat in; `MoveCx` carries it.
- `things/particles/sim.rs`: `after_build` wakes the air, `after_step` takes it out of the world for the step.
- `things/particles/looks.rs`: Dust clouds, Thick fog; Sandstorm with gust waves and haze; `add_particles` adds the
  Air card for looks that follow the air.
- `scenes.rs`: two more particle scenes ("Dust in the air", "Smoke heating the air"); `tests/golden-particles*.txt`
  gain their lines (no other line moves).
- `things/particles/air_tests.rs` (new): the things in the air.
- `bpe-core/src/settings.rs`: sections "Wind", "Balls and shapes", "Heat" round the existing "Air" (the card's groups).

### `bpe-studio`
- `particle_job.rs`: the snapshot also carries the ROOT air.
- `app.rs`: `show_air`, the toolbar toggle.
- `preview.rs`, `preview/air_view.rs` (new): the arrows.
- `inspector.rs`: `apply_layer_edit` adds the Air card when a `force.air` layer is added.

## Steps and how each is verified

1. Core slot: `cargo test -p bpe-core` (ids test).
2. Runtime air: unit tests: calm air equals the ambient wind (and outside the grid too); a ball moving through still
   air drags the cells it covers its way and the projected air flows round; a solid shape's cells move with it and the
   air beside a wall doesn't flow into it; heat makes the air above rise; gust waves travel (two places along the wind
   gust at different times, waves 0 at the same time); scrolling the camera keeps the overlapping cells; 1 thread
   equals 4 bit for bit; a NaN ball velocity doesn't spoil the air.
3. Fingerprints: an Air card edit or Air key changes `beside_fingerprint` and not `fingerprint` / `sound_fingerprint`.
4. Kit: Follow the air carries a particle with the card's wind; particles near a passing ball with Balls push 0 still
   move (through the air); heat from a Rise that heats lifts a following particle; part 1's Wind with waves 0 is
   unchanged (the goldens); a world with the air steps every ball exactly as one without particles; seek back and
   forth with the air equals a straight run; two things share one air (the second sees the air the first brought).
5. Goldens: `cargo test -p bpe-kit --test golden_particles` with the two new scenes blessed (debug and release); the
   old lines unchanged (diff shows only added lines); the ball goldens unchanged.
6. Looks: each new look adds a thing that resolves without problems and makes particles in 2 s; Dust clouds and Thick
   fog add the Air card once.
7. Editor (kittest): the Air card shows in the scene settings after adding Dust clouds; adding a Follow the air force
   from the Motion card's menu adds the Air card; the Air toggle shows with a Particles thing and turns Show air on;
   `air_view::arrows` gives arrows for a stirred air and none for an empty view.
8. Bench (ignored test): the air step at 1080×1920 Normal and Fine, printed; target under 1 ms (Normal) in release.
9. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace`, release goldens, CLAUDE.md map, Windows
   exe builds.

## What part 8 (Huge swarm) will need from this part

- The air at each particle step: `Air::size()` (cols, rows), `origin()` (the lattice cell of column 0, row 0),
  `cell_size()`, and `velocities()` (row-major `Vec2`s, cell centres): upload as an `Rg32Float` texture (or storage
  buffer) per step. The GPU sampler must copy `Air::at`: bilinear between cell centres, clamped one cell inside, and
  outside the grid the ambient wind (`AirSpec` + `gust_factor`, which is pure in place and time).
- The air is brought to step *k* before particle step *k* (`catch_up_to`), so the swarm must step the air first and
  read it after, the same as a CPU thing.
- Huge swarm particles don't heat the air (no readback); a swarm thing's Follow the air reads it like any other.

## Not doing (part 2)

- No two-way air on balls (part 4's drag is by particle density); particles don't push the air (only heat goes in).
- No wind arrow handles in the preview (the spec's section 8 list); the Air card is set from the inspector.
- No change to the existing `layer.particles` / `shape.particles`; no built-in scenes; no demo videos.

## As built

- The solver lives in `bpe-kit/src/air.rs` (decision 1 above; the spec said `bpe-runtime`).
- The first Particles thing due a step (`ParticlesSim::after_step`) takes every Particles thing's due steps, all
  together one particle step at a time in tree order: the air brought to that step, each thing's `step::step_in`
  with it, then its `step::put_heat`. So at any Physics rate (two particle steps a world step at 60 Hz) every thing
  reads the air as the others left it at that step; the rest then find themselves up to date. `step::step` (no
  air) is kept for tests.
- An air woken mid-run (a key turns on Heats the air) starts still at the thing's step, never run up to it.
- Keys on the Air card are left out of what the main world senses: `world::main_animation` drops `BESIDE_SLOTS`
  keys from the physics fingerprint's keys and from its `follows_sound`/`needs_light`, and a world with nothing
  beside the physics builds no sound or light maker for them; `beside_fingerprint` hashes the sound's or light's
  fingerprint when an Air key follows it.
- The grid stops at ±1e8 px (`FARTHEST`) and every ball and wall loop is held to the grid (`Air::on_grid`), so far
  off or broken places loop over nothing; a cell a solid shape covers keeps no heat.
- The emitter's test-fired lone copies drop the air (`test_fire::Lone::new`).
- Found on the way: part 1's `settings_changed` downcast the box, not the state, so keyed forces and settings never
  reached a Particles thing; fixed (`(**s).as_any_mut()`). The same slip in the emitter's `fire_now` and the
  shape's `side_of`/`side_on` is left to its own fix.
- The Air card's groups are new sections in `bpe_core::SECTION_ORDER`: Wind, Air, Balls and shapes, Heat.
- Part 1's "every look makes particles in 2 s" test lets looks emitted On hits off in the blank project (no balls).
