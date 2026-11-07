# Particles part 1: the engine

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 3, 4, 8, 10, 11 part 1). Approved by the owner
in the brainstorm thread; built straight after this plan (no approval stops), one Opus review at the end, then the PR.
Merged only when the owner says "merge"; then the exe is built from main and installed.

## What part 1 delivers

- A new thing kind **Particles** (`particles`, family "Particles") with its own **Source** settings and three cards:
  **Motion** (`motion.particles`, force layers), **Meets things** (`meets.particles`, in the Bounce slot) and **Look**
  (`look.particles`, draw layers with the around sub-layers).
- An exact, deterministic CPU particle engine stepped with the physics at 120 Hz, in chunks of 4096 particles on
  threads, its state in `World.states` so playback checkpoints and seeks replay it exactly.
- Two-way is not in part 1: every Particles thing is decoration. It never touches balls, walls, the log or the sound,
  and the editor steps it on its own **particle playback** thread, so a particle edit never restarts the balls or the
  sound.
- Sources: Whole screen (Everywhere/Top/Bottom/Left/Right, Margin, follows the camera), An area (Rectangle/Circle/Ring),
  A point, A line, Riding its parent (Each ball / The middle / Ball number / The edge / Inside). Emit: Steady, Bursts,
  On hits, Keep a crowd, Once at the start. (Word/picture, Grid, Its parent's particles and On the beat are later
  parts.)
- Forces: Gravity, Wind (with gusts, no travelling waves), Drag, Swirl, Wander, Rise, Sway and Flutter.
- Meets: balls (Push and Reach; On touch Pass / Bounce off / Vanish), shapes (Pass / Bounce / Stop / Vanish), the
  picture's edges (Pass / Wrap round / Bounce / Vanish).
- Look layers: **Sprites** and **Streaks**, with Colour (own paint over life / speed / place / order / time, or the
  parent ball's colour), Opacity, Size over life, Fade, Twinkle, Where (behind or in front of the balls), Only (all /
  younger / older / a share), Blend and Blend amount, and the six around sub-layers (Glow, Outline, Shadow, Recolour,
  Flash on hit, Colour on hit) as `particles.sub.*` kinds.
- The Add thing menu's **Particles** heading: Empty, then the part 1 looks in four groups.
- Editor: the cards' lists, handles in the preview, the "Particles: N, X ms" status line.
- Tests: particle goldens, determinism (seeks, threads, main vs particle playback), pictures, kittest, bench.

## Decisions made while planning (fill-ins of the spec, stated so review can correct them)

1. **Particles step on even world steps** (`world.step % 2 == 0`, in `after_step`), not odd ones as the spec guessed.
   Frames at 30, 60 and 120 fps fall on even steps (steps 8, 4, 2 apart), so they show the particles exactly where
   they are; a frame on an odd step (24, 50 fps) shows the last particle step, at most 1/240 s old. So no lerp is
   needed at draw time (`previous` is still kept for the shape tests and streak direction).
2. **Particles are an array of structs** (`Vec<Speck>`, about 72 bytes each), not a struct of arrays: one
   `chunks_mut(4096)` hands each thread whole particles, and every per-particle pass reads one cache line. The spec's
   fields are all there (`place`, `previous`, `velocity`, `born`, `life`, `size`, `turn`, `spin`, `seed`, `home`,
   `flags`, plus `ball` (the parent ball it came from, row + 1, 0 none) and `flip` (Flutter's phase)).
3. **Seeds**: particle *i*'s seed is `hash(world seed, thing id, its birth number, Seed setting)`, its birth number
   counting every birth the thing ever tried (dropped ones at the cap too), so a particle's dice never depend on the
   others alive or on any other thing. Each draw from the dice uses `hash(seed, k)` for a fixed `k` per use.
4. **Keeping particles out of the main world.** New `ThingSim::in_main(&ResolvedThing) -> bool` (default true; the
   Particles sim says false). `bpe_runtime::main_part(resolved, sims)` is the resolved scene without those things.
   `fingerprint` and `sound_fingerprint` hash `main_part` only, so particle settings, keys, places and tags never
   change them. The editor's main playback, the sound engine's playback (`bpe-sound/src/engine.rs`) and the aiming
   thread build from `main_part`; export (`bpe-cli`), tests and the particle playback build from the whole scene.
   `World.beside` notes the built things that aren't in main; `World::lone` drops them (aiming never steps
   decoration). Because a Particles thing writes nothing but its own state (no bodies, walls, poses, events), a world
   with it steps every ball exactly as one without it: a test checks this step for step.
5. **The thing reads its own cards** (as built: this replaced a first idea of `ModuleSim`s for the two cards, whose
   `prepare` can't see the card's layers). `motion.particles` and `meets.particles` have no module sims; the
   particle sim reads them from its `ResolvedThing` into `Forces` / `MeetsSettings` at build and in
   `settings_changed`. `World::animate` now calls the thing sim's `settings_changed` for a keyed card setting whose
   card has no module sim, so keyframed forces and meets work. The Source settings are the thing's own:
   `affects_physics` says true for every key (only so keys reach `settings_changed`; the fingerprint skips the thing
   anyway), and `settings_changed` copies the new settings into the state. `beside_fingerprint` hashes what the
   particle playback is built from (the physics, the beside things' Source and their Motion and Meets cards whole,
   and their keys), so a particle edit restarts only the particle playback.
6. **The draw reads the state from `cx.world.states`.** In the editor the main world has no particle states, so the
   preview **lends** the particle playback's latest states to the main world for the frame (`World::lend_states` /
   `World::take_back`, the `set_between` pattern: draw-only, the world never steps while they are lent). A lent
   state is ignored by the main world's stepping (the thing isn't in its order).
7. **The particle playback thread** (`bpe-studio/src/particle_job.rs`, `ParticleJob`): owns a whole-scene `Playback`
   (checkpoint budget 512 MB, `Playback::with_budget`), takes a target time and new scenes through a mutex and a
   condvar, steps in 10 ms chunks (`seek_within`), and after each chunk publishes a `Snapshot { step, states, count,
   step_ms }` (only the Particles things' states, cloned). It is started only when the scene has a Particles thing.
   `Studio::sim_target` holds the main playback at the snapshot's step while the job is caught up with it (so balls
   and particles show the same step while playing); while the job is behind the main playback (just rebuilt), the
   main playback runs on and the preview shows the latest snapshot with a "Particles… N%" pill.
8. **The look is drawn as around layers.** `ParticlesDraw::draw` draws nothing; `around()` lists one entry per shown
   Look layer (its Blend, amount, and behind the balls unless Where is In front), and `draw_around(i)` draws layer
   *i*'s sprites through `around_subs::particle_with_extras`, so other blends route through the frame's layers as
   trails' do. Particle lighting fields are left 0 (part 7). `Thing.z_order` doesn't reorder the sprite lists (as
   for trails and particle layers today).
9. **Particle sub-layer kinds** are new ids on the `look.particles` card (`particles.sub.glow`, `.outline`, `.shadow`,
   `.recolour`, `.flash`, `.hue`), with the shape sub-layers' settings (sizes in pixels: `ShapeSubGlow`,
   `ShapeSubShadow`, and `SubOutline`, `SubRecolour`, `SubFlash`, `SubHue`). Flash on hit and Colour on hit read the
   parent thing's hits (the last hit of any of its balls, or the shape being hit); with no parent, no hits.
10. **Force layers** sit in a new `LayerSection::Force`, ordered among themselves like modulators (no lanes),
    applied top of the list first (the list shows the first-applied on top, like the Motion order reads).
11. **Whole screen** covers the camera's shot bounds (`CameraView::shot_bounds_half`, the axis box of a turned shot)
    grown by Margin; Top/Bottom/Left/Right births sit on that edge, Margin outside the shot. With no camera thing it
    is the video's rectangle round the origin.
12. **Riding**: read from the tree parent (as lights do) when Where is Riding its parent; the thing's own Position
    (Placement) is an offset from the place ridden. Each ball rides alive balls; The edge on a ball is each ball's rim
    (a random angle), on a shape its outline (points spread by length from `light::draw::outline_points`); Inside is a
    shape's fill (tries up to 8 random points in its bounds, even-odd inside the outline points; else the middle) or,
    on balls, inside each ball. Riding Each ball, Steady's Rate is per ball.
13. **Balls push particles** with a falloff `w = Push × (1 − d / (Reach × r))²` inside `Reach × r` of a ball's middle:
    the particle's velocity moves towards the ball's by `1 − e^(−12 w dt)`, and inside the ball's radius it is also
    pushed straight out at the ball's speed (so a ball parts smoke and carries rain). On touch (centre within the
    radius plus the particle's half size): Pass, Bounce off (put on the surface, the velocity relative to the ball
    reflected with Bounciness), Vanish. Balls go into a uniform grid (cell = the largest reach, at least 8 px, at most
    256 × 256 cells over their bounds) built once a particle step; at most 32 balls are looked at per particle.
14. **Shapes** use `keep_clear_from` against `World.walls` with the particle as a 0.5 px circle moving from
    `previous` to `place` (two-sided like balls), skipping walls whose bounds (the outline's points' box, grown by the
    wall's band and 2 px) the particle's step doesn't touch. Bounce reflects with Bounciness and rubs Friction off
    the sideways speed; Stop sets the velocity to 0 (it slides no more; gravity then pulls it along the wall); Vanish
    removes it.
15. **Limits**: Most particles (default 20 000, span 1..200 000, hard 1 000 000); births past it are dropped (their
    birth numbers still count). Every number written is checked finite; a particle with a non-finite place or
    velocity is removed. Speeds are clamped to the world's ceiling.
16. **Scene format 12** (no upgrade): files with Particles things don't open in older editors.

## Files

### `bpe-core`
- `registry.rs`: `LayerSection::Force`. `layers.rs`: `add_layer_in`, `sound_end`'s rank (Force after Effect),
  `move_layer` (Force moves among Force layers). `scene.rs`: `FORMAT = 12`.

### `bpe-runtime`
- `plugins.rs`: `ThingSim::in_main`.
- `world.rs`: `World.beside`, filled in `build`; `lone` drops `beside` things; `lend_states`/`take_back` (`Lent`);
  `main_part`; `fingerprint_of` hashes `main_part`.
- `playback.rs`: `Playback::with_budget(scene, resolved, sims, bytes)`; `lend_states`/`take_back` passing through.

### `bpe-kit`
- `things/particles/mod.rs`: `KIND`, `ParticlesKind`, `ParticlesSettings` (the Source: folds Where, When, Each
  particle, Limits), its enums (`From`, `Edge`, `AreaShape`, `RidesOn`, `Emit`, `Heading`).
- `things/particles/state.rs`: `Speck`, `ParticlesState` (specks, birth counter, rate carry, next burst step, the
  settings, parent, last step's time in ms), `flags`.
- `things/particles/births.rs`: `places` (where a birth can be, with the parent's velocity and ball), `births_now`
  (how many this step and from where), `spawn` (one particle's dice).
- `things/particles/forces.rs`: the `motion.particles` card, the force layer kinds `force.gravity`, `.wind`, `.drag`,
  `.swirl`, `.wander`, `.rise`, `.sway`; `Forces` (prepared), `apply` on one particle.
- `things/particles/meets.rs`: the `meets.particles` card, `Meets` (prepared), `BallGrid`, `meet_balls`, `meet_walls`,
  `meet_edges`.
- `things/particles/step.rs`: `step` (births, the chunked move, deaths and compaction), `run_chunks` with a thread
  count (tests force 1 and many).
- `things/particles/sim.rs`: `ParticlesSim` (`build`, `after_build`, `after_step`, `settings_changed`, `in_main`),
  (no module sims: see decision 5).
- `things/particles/look.rs`: the `look.particles` card, `SpritesLayer`, `StreaksLayer`, the sub-layer kinds.
- `things/particles/draw.rs`: `ParticlesDraw`.
- `things/particles/looks.rs`: `Look` (Empty and the part 1 looks), `GROUPS`, `add_particles`.
- `things/particles/tests.rs` (unit tests), `scenes.rs`: `particle_scenes` (test-only).
- `lib.rs`, `starters.rs` (the three starter cards), `modules/motion`, `modules/bounce` (their cards don't fit
  particles), `things/mod.rs`.
- `tests/golden_particles.rs` + `tests/golden-particles.txt` / `-debug.txt`; `tests/particle_pictures.rs`.

### `bpe-sound`
- `engine.rs`: the physics playback built from `main_part`.

### `bpe-studio`
- `particle_job.rs` (new), `app.rs` (`Studio.particles`, rebuilds, `sim_target`, status line), `preview.rs` (lend
  the states, the "Particles… N%" pill, handles), `preview/particle_handles.rs` (new), `tree.rs` (the Particles
  heading and `Action::AddParticles`), `inspector.rs` (no Physics merge for particles; the Motion and Look cards'
  editors), `look_editor/forces.rs` (new, the force list), `look_editor/mod.rs` (the particles Look without ball view,
  trails or reactions), `aim_job.rs` (`main_part`).

## Settings (labels as shown; spans, not hard ranges, except where the maths breaks)

Source (`ParticlesSettings`):
- Where: From (Whole screen); Edge (Everywhere); Margin 40 px; Area shape (Rectangle); Width 400, Height 300 px;
  Radius 200, Inner radius 100 px; Length 400 px; Line turn 0°; Rides on (Each ball); Ball number 1.
- When: Emit (Steady); Rate 60 per s; Every 1 s; Count 30; Who hits (anything, a `Target`); Least speed 0 px/s;
  Follow impact (on).
- Each particle: Life 2 s (hard > 0.001), Life spread 0.3; Speed 100 px/s, Speed spread 0.3; Direction (A way:
  `Heading` Off the bounce / All round / Along the path / Behind / A way), Angle −90° (up), Spread 30°; Takes the
  parent's speed 0; Size 3 px (hard ≥ 0), Size spread 0.3; Turn 0°, Turn spread 1 (0..1 of a whole turn); Spin 0°/s,
  Spin spread 0.5; Seed 0.
- Limits: Most particles 20 000 (hard 1..1 000 000).

Forces (each with Amount 1 and Only for All / Younger than / Older than, Age 1 s):
- Gravity: Strength 400 px/s², Direction 90°, Scene's gravity (off).
- Wind: Speed 200 px/s, Direction 0°, Grip 2 per s, Gusts 0 (0..1), Gust every 2 s.
- Drag: Drag 1 per s.
- Swirl: Strength 300 px/s², Size 150 px, Drift 30 px/s.
- Wander: Strength 200 px/s², Turns 1 per s.
- Rise: Strength 300 px/s², Cools over 0 s (0 = never).
- Sway and flutter: Sway 20 px, Sway speed 1 per s, Flutter 0 per s, Flutter drift 30 px/s.

Meets things: Balls push 0.6, Reach 2 radii, At balls (Pass), Ball bounciness 0.5; At shapes (Pass), Bounciness 0.4,
Friction 0.1; At edges (Pass), Edge margin 40 px.

Look layers (common): Where (Behind the balls), Only (All), Age 1 s, Share 0.5, Colour from (Own), Colours (paint,
follows life/speed/place/order/time, default white), Opacity 1, Size × 1, Size over life (Even), Fade (Out), Twinkle
0, Twinkle speed 8, Blend (Normal), Blend amount 1. Sprites: Made of (Dots), Softness 0.3. Streaks: Stretch 0.03 s,
Least length 4 px, Width 1.

## Steps and how each is verified

1. Core: `LayerSection::Force`, FORMAT 12. `cargo test -p bpe-core` (a test adds and moves force-section layers in a
   test card).
2. Runtime: `in_main`, `beside`, `main_part`, `lone`, lend/take back, `Playback::with_budget`. Runtime tests: a lent
   state is visible and gone after take back; `main_part` drops a thing whose sim says so; fingerprints ignore it.
3. Kit engine (state, births, forces, meets, step, sim): unit tests: each Emit's counts; each From's places inside
   their region; each force's effect on one particle; balls push and touch outcomes; shapes bounce/stop/vanish and
   never let a particle through; edges wrap/bounce/vanish; cap drops births; NaN removal; 1 thread equals 8; a seek
   back and forth equals a straight run; a world with particles steps every ball exactly as one without
   (`main_part`), the log hashes equal.
4. Look and draw: unit tests on frames (counts, colours by life, Only, blends routed); pictures test (GPU) for
   Sprites and Streaks.
5. Looks and `add_particles`: each look adds a thing that resolves without problems and makes particles in 2 s.
6. Goldens: `golden_particles.rs` writes an FNV hash of every particle's place, velocity and life once a second for
   `particle_scenes` (rain, snow riding, sparks bouncing in a ring, crowd wandering, hits bursting); blessed in debug
   and release. The ball goldens must not move (`cargo test -p bpe-kit --test golden` unchanged).
7. Sound and aim: built from `main_part`; existing sound goldens unchanged.
8. Editor: particle job, lending, sim target, status line, tree menu, inspector cards, force list, particle Look,
   handles. Kittest: adding each look from the menu; the Motion list adds/reorders a force; the Look list adds a
   Streaks layer and a Glow sub-layer; the status line shows the count; the particle job reaches the playhead.
9. Bench (ignored): 100 000 rain particles, step time printed; target < 4 ms a particle step in release on 8 cores.
10. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace` (debug), release goldens, CLAUDE.md map,
    Windows exes build and start under Xvfb.

## Not doing (part 1)

- No two-way, Pop, Stick, Settle, Pile, Splash into, feeding children (part 4); no air (part 2); no Word/Grid
  sources, On the beat, the other forces (part 3); no other layer kinds, Mirror, Colour split, Glitch, Fine detail
  (parts 5, 6); no lighting fold or Gives off light on particle layers (part 7); no Huge swarm (part 8).
- No change to `layer.particles` / `shape.particles`; no built-in or example scenes; no demo videos.

## As built

- Particle steps keep time at any Physics rate: `step::due(world step, rate)` = world time × 120, and `catch_up` takes
  none, one or several steps after a world step (at 240 exactly one every second step, so the goldens are those of
  the even-step plan). On hits reads the log since the last particle step (`ParticlesState.world_step`).
- The preview reads the particle job's latest snapshot once a frame, steps the main playback to its moment and lends
  it only when the steps match; the job follows `Studio::aim_target` so an aiming hold doesn't run it ahead.
  "Particles… N%" shows only when it is more than `PARTICLES_BEHIND` (0.25 s) behind (while playing it is a frame
  late, which is no wait). The particle playback has its own 512 MB checkpoint budget.
- A riding Particles thing's handles sit where it is born: `World::anchor_as` works its place out from its parent in
  the main world, which doesn't build it.
- States are read through the box (`(**state).as_any()`): `Box<dyn ThingState>` is a `ThingState` itself, so
  `as_any` on a `&Box` gives the box.
- Scene format stays 12 (Rules part 1 took 12 first; particles need no upgrade).
- Left for later parts: effect warm-up after a jump replays the main playback without particles (`preview/warm.rs`);
  pulses on hits don't reach particle settings (`World::animate` passes no hits, as for the physics); a snapshot is a
  full clone of the states (fine to the cap, costly near 1 000 000 particles).
