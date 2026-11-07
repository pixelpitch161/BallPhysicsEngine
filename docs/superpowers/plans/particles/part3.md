# Particles part 3: more motion

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 2, 3.2, 3.3, 11 part 3). Approved by the owner
in the brainstorm thread; built straight after this plan (no approval stops), one Opus review at the end, then the PR,
merged by this thread once checks and review pass. No exe install (that waits for every particles part).

Part 1 is merged (e36d189, plan `part1.md` with its "As built"). Parts 2 and 4 to 7 build in parallel, so every shared
file is touched additively and part 3's own work sits in new files.

## What part 3 delivers

- **Sources**: **A word or picture** (text in IBM Plex at a height and weight, or a PNG picture at a width; one place per
  Spacing px of what is filled) and **A grid** (covering the screen or a rectangle; by spacing or columns × rows;
  Jitter; Stagger rows). Each place is a particle's **home**; homes turn with the thing's Turn and a new **Homes turn**
  (°/s). With **Keep a crowd** or **Once at the start**, a word or grid fills **every place** (one particle each, a dead
  one replaced at its own place); the other Emits pick places at random.
- **Emit On the beat**: Count particles on every beat / bar / few bars of the scene's tempo, from an Offset in beats.
- **Direction Round the middle** (a new Heading): flying sideways round what they come from (orbits).
- **Forces**: Pull to, Push away from, Spring home, Flock, Magnetic field, Flow field, Spirograph, Pulse on the beat,
  Ripple from hits.
- Every force's **Only for** gains **A share** and **The rest** (the same split as a Look layer's Only, so a layer can
  draw exactly the particles a force acts on: comet gas and dust from one thing).
- **Looks**: 10 Galaxy round a ball, 12 Flow lines (motion), 13 Form a word, 23 Flock (motion), 25 Shockwaves, 26 Comet
  tail, 27 Black hole, 28 Iron filings, 33 Beat grid, 38 Moiré (two things), 40 Spirograph (motion); 7 Fireflies now
  flee the balls.
- Editor: the new settings show through the generic grids; the grid's rectangle gets the corner handle; the looks show
  in the Add thing menu.

## Decisions made while planning (fill-ins of the spec, stated so review can correct them)

1. **Places ("spots")** are worked out once per change of what they depend on, in the thing's own frame (middle at 0,
   x right, y down), and kept on the state as `Arc<Vec<Vec2>>` so checkpoints and clones share them (up to 1 000 000
   points). Key: a `SpotsKey` of every setting they read (and the video size for a screen grid, and for a picture the
   file's modified time). At most `MOST_SPOTS` = `MOST_EVER` places; past it the list is cut.
2. **Words** are filled by a scanline over the glyph outlines (non-zero winding, crossings sorted per lattice row), not
   by testing every lattice point against every edge (a long text at 2 px would take seconds). The glyph outlines are
   the balls' Word shape's (`things::balls::shapes::letter_outlines`, made `pub(crate)` with a font argument). Weight:
   Regular, Medium, SemiBold (the three IBM Plex Sans files the editor ships). One line of text (no line breaks, as the
   balls' Word). The lattice is centred on the letters like the balls' Word.
3. **Pictures** are PNG only (`png` crate, already in the lock; moved from a dev-dependency to a dependency). A place is
   kept where the picture is filled: **Uses** Solid parts (alpha ≥ 50 %), Bright parts (light × alpha ≥ 50 %) or Dark
   parts. Width sets the size, the height by the picture's shape. A missing or broken file gives no places (no
   particles) and is not an error. Decoded pictures are cached process-wide by path, modified time and size (at most 8).
   The editor's `unit = "picture"` browse button (unused until now) filters PNG only and says so.
4. **Grids**: Covers **The screen** (the video's size at zoom 1, plus Margin, centred on the camera's middle at each
   step, so it stays in shot like Whole screen sources) or **A rectangle** (Width × Height round the thing's place,
   reusing the area's Width/Height settings, so the area's corner handle works on it). **Grid by** Spacing or Columns
   and rows. Points sit at cell middles; **Jitter** (0..1 of a cell) moves each by fixed dice of its index; **Stagger
   rows** shifts every other row by half a cell (a honeycomb).
5. **Homes that move**: a particle born at a place keeps its place's number (`Speck.spot`, place + 1; 0 none). Its home
   is worked out every step: `centre + rotate(angle, place)`, where `centre`/`angle` are the thing's anchor (the camera's
   middle and 0 for a screen grid) and `angle` adds Turn and the **Homes turn** run so far (`ParticlesState.home_turn`,
   stepped by Homes turn × dt every particle step, so a keyed Homes turn integrates exactly). Particles without a place
   keep the place they were born (`Speck.home`).
6. **Filling every place**: with a word or grid source, Keep a crowd and Once at the start ignore Count: Once at the
   start makes one particle per place at the build; Keep a crowd, every step, makes one at each place that has no living
   particle (in place order). Most particles still caps them. Steady, Bursts and On the beat pick a place by dice
   (`hash(seed, 8) % places`); On hits still births at the hit.
7. **The beat** comes from the scene's tempo, the same as the rules' Beat block and the rigs
   (`rig::pattern::beat_seconds`). The scene sim now keeps a small `SceneBeat { beat, per_bar }` state at
   `ThingId::ROOT` (its first state); `beats::of(world)` reads it (120 bpm 4/4 without it). Moments are
   `first + k × every` (first = Offset × beat), counted on the particle clock (`step / 120`): a particle step takes the
   moments in `(t − dt, t]`, the build's step the moment at 0. The editor's particle playback must rebuild when the tempo
   changes, so the runtime gains `ThingSim::affects_beside(key)` (default false; the scene sim says yes for `tempo`,
   `time_top`, `time_bottom`) and `beside_fingerprint` hashes those keys of every main thing. The main playback is not
   rebuilt (tempo is not physics).
8. **Per-step preparation** (`ready.rs`, `Ready`): once a particle step, before the chunks, everything the new forces
   read is gathered read-only: the thing's middle, its parent's balls by row and their middle, a ball grid for Balls
   nearby and the magnetic field (reach = the largest any force asks), each Ripple's rings (from the log), each Pulse's
   two latest beat moments, the homes frame, and for Flock a snapshot of every particle's place and velocity in a
   uniform grid. Chunks read only `Ready`, so 1 thread equals many. `Forces::apply` takes `&Ready` and the particle's
   index (Flock skips itself by index).
9. **Pull to / Push away from**: Toward (Its place / A point / Its ball / Ball number / The middle of its parent /
   Balls nearby). Its ball is the ball the particle came from (`Speck.ball`), else the parent's middle, else its place.
   Strength in px/s² at **At** px (Balls nearby: at the ball's surface), Falloff Even / Weaker further (1/d) / Much
   weaker further (1/d²), the factor capped at 100 and the distance softened to at least 4 px (and the Swallow radius);
   **Reach** px (0 everywhere; Balls nearby in ball radii, default 4); **Orbit**: sideways acceleration as a share of
   the pull (negative the other way); **Swallow radius** (Pull to only): a particle closer dies. Balls nearby looks at
   most `MOST_NEAR` balls (the ball grid's cell list).
10. **Spring home**: `v += Stiffness × Amount × (home − p) × dt`, then `v *= e^(−Damping × Amount × dt)`.
11. **Flock**: neighbours within Reach (at most 32, found in a fixed cell order); `a = Separation × Σ(p − o)` over those
    within **Keep apart** × Reach, `+ Cohesion × (mean place − p) + Alignment × (mean velocity − v)`, all × Amount; then
    the speed is held between **Least speed** and **Most speed** (blended by Amount when under 1); the particle turns
    to face the way it flies (so shapes like Shards point along the flight).
12. **Magnetic field**: From Each ball (a dipole along its motion, or one way) or Its place (a dipole one way). Field
    direction `3(m·n)n − m`; strength `min(1, (Size / r)³)` with Size = Field size × the ball's radius (or Field size px
    at a point), looked at out to 4 × Size. **Line up** turns the particle's turn towards the field line (either end,
    a needle) by `min(1, (Line up × strength + 0.1) × dt)` of the gap; **Pull** pulls it towards the field's source by
    Pull × strength px/s².
13. **Flow field**: Pattern Noise (angle = noise × 2 turns, drifting over time), Spiral (round the middle, turned in by
    Pull in °), Waves (a way, swayed by a sine across it), Vortex (round the middle, fastest at Size). Particles take on
    the flow's velocity by Grip (as Wind does).
14. **Spirograph**: round Its place / A point / Its ball / Ball number / The middle of its parent; each particle's pen
    follows the hypotrochoid `(R − r)(cos θ, sin θ) + d(cos kθ, −sin kθ)`, `k = (R − r)/r`, `θ = 2π(Speed × age +
    phase)`, its R, r and d each spread by **Pens differ** from its dice. The velocity is set to reach the next point
    in one step, blended by Amount (0..1).
15. **Pulse on the beat**: on each moment (Every beat / bar / few bars, Offset) a kick of Strength px/s, Outwards from
    the thing's middle / Up / A way / All round (each particle its own way), spread over Decay seconds
    (`S × (F(τ) − F(τ − dt))`, `F(τ) = 1 − e^(−τ/decay)`, a step at 0) from the two latest moments.
16. **Ripple from hits**: each hit (Who hits, Least speed, Follow impact) within the last Lasts seconds is a ring
    `r = Speed × age` round the hit point; a particle within Width of the ring is pushed outwards by
    `Strength × (1 − age/Lasts) × (1 − x²)²`, `x = |d − r| / Width`. The newest `MOST_RINGS` (64) only. Worked out from
    the log, so nothing is kept.
17. **Looks** are recipes in a new `looks_motion.rs`; `looks.rs` gains the enum variants, names, help and their group
    entries, and `add_particles` calls `looks_motion::also(look)` for extra sibling things (Moiré's second grid), so the
    `Recipe` struct other parts fill doesn't change. Moiré adds "Moiré" (a still grid) and "Moiré 2" (a turning grid the
    balls push), returning the first.
18. **Goldens**: the part 3 scenes are a new golden file (`tests/golden-particles-motion*.txt`,
    `scenes::particle_motion_scenes`) so parallel parts don't collide; the part 1 file changes only in its Fireflies
    lines (they flee now). Ball goldens don't move.
19. **Scene format** stays 12 (new settings and kinds only).

## Settings (labels as shown; spans, not hard ranges, except where the maths breaks)

Source additions (`ParticlesSettings`, folds as before):
- From gains **A word or picture** (`a_word`) and **A grid** (`a_grid`).
- Word: Made of (Text / Picture), Text ("BOUNCE"), Letter height 240 px (hard > 1), Weight (SemiBold; Regular, Medium),
  Picture (a PNG path), Picture width 600 px, Uses (Solid parts / Bright parts / Dark parts).
- Grid: Covers (The screen / A rectangle), Grid by (Spacing / Columns and rows), Columns 24, Rows 40 (hard ≥ 1), Jitter
  0, Stagger rows (off). Width/Height shared with the area (when A rectangle).
- Spacing 12 px (hard ≥ 1) for words, pictures and grids by spacing.
- Turn (`line_turn`) also for words and grids; Margin also for a screen grid. Homes turn 0 °/s.
- Emit gains **On the beat**: Every (Beat / Bar / Bars), Bars 2, Offset 0 beats; Count as for bursts.
- Direction gains **Round the middle**.

Every force (common): Amount 1, Only for (All / Younger than / Older than / **A share** / **The rest**), Age 1 s,
**Share 0.5**.

- Pull to: Toward (Its place), Point (0, 0) px, Ball number 1, Strength 600 px/s², At 100 px, Falloff (Weaker further),
  Reach 0 px, Reach 4 × (balls), Orbit 0, Swallow radius 0 px.
- Push away from: the same without Swallow radius (Toward Balls nearby by default).
- Spring home: Stiffness 30 per s², Damping 4 per s.
- Flock: Reach 120 px, Keep apart 0.3, Separation 6 per s², Alignment 1.6 per s, Cohesion 1.2 per s², Least speed 200,
  Most speed 440 px/s.
- Magnetic field: From (Each ball), Points (Along its motion; A way), Angle −90°, Field size 3 × (balls) / 100 px
  (point), Line up 10 per s, Pull 60 px/s².
- Flow field: Pattern (Noise), Speed 160 px/s, Size 300 px, Grip 2 per s, Changes 0.05 per s, Angle 0°, Pull in 15°,
  Sway 30°.
- Spirograph: Round (Its place), Point, Ball number, Ring radius 240, Wheel radius 90, Pen distance 110 px, Speed 0.4
  turns per s, Pens differ 0.4.
- Pulse on the beat: Every (Beat), Bars 2, Offset 0, Strength 300 px/s, Decay 0.08 s, Way (Outwards), Angle −90°.
- Ripple from hits: Who hits (anything), Least speed 0, Follow impact (on), Speed 900 px/s, Width 60 px, Strength
  4000 px/s², Lasts 1.4 s.

## Files

### `bpe-runtime`
- `plugins.rs`: `ThingSim::affects_beside(&self, key) -> bool` (default false).
- `world.rs`: `beside_fingerprint` hashes main things' `affects_beside` params; a test that a tempo-like key changes it
  and the main fingerprint doesn't.

### `bpe-kit`
- `Cargo.toml`: `png` a dependency.
- `things/scene/sim.rs`: `SceneBeat` state at ROOT (built and on settings change), `affects_beside`.
- `things/balls/shapes.rs`: `letter_outlines(font, text, height)` `pub(crate)`, the font bytes per weight.
- `things/particles/mod.rs`: the new `BornFrom`, `Emit`, `Heading` variants; `WordFrom`, `Weight`, `Uses`,
  `GridCovers`, `GridBy`, `BeatEvery`; the new settings; `when`s widened.
- `things/particles/spots.rs` (new): `SpotsKey`, `spots_for`, `word_spots` (scanline), `picture_spots`, `grid_spots`,
  `MOST_SPOTS`, the picture cache.
- `things/particles/beats.rs` (new): `beat_of(world)`, `Moments` (first, every), `count_in(t0, t1)`, `latest(t, n)`.
- `things/particles/state.rs`: `Speck.spot`, `ParticlesState.spots`, `spots_key`, `home_turn`; `home_of`.
- `things/particles/births.rs`: places for words and grids, `Place.spot`, On the beat, filling every place, Round the
  middle.
- `things/particles/forces.rs`: Only for A share / The rest (+ Share), the new `Force` variants, `read`, `common`,
  `apply(s, index, age, cx, ready)` dispatching to `more_forces`, `kinds()` appended, `Forces::wants()`.
- `things/particles/more_forces.rs` (new): the nine settings structs and enums, each force's per-particle maths.
- `things/particles/ready.rs` (new): `Ready`, `Flockmates` (the neighbour grid), `Ring`, built by `Ready::new`.
- `things/particles/step.rs`: builds `Ready`, steps `home_turn`, passes the index (`run_chunks_indexed`).
- `things/particles/sim.rs`: `read_into` with the world (spots), unchanged otherwise.
- `things/particles/looks.rs` + `looks_motion.rs` (new): the part 3 looks; Fireflies flee.
- `things/particles/tests.rs` (+ `tests_motion.rs`, new): unit tests below.
- `scenes.rs`: `particle_motion_scenes`; `tests/golden_particles_motion.rs` + its two golden files.

### `bpe-studio`
- `widgets.rs`: the picture browse button filters PNG.
- `preview/particle_handles.rs`: a rectangle grid's corner handle and outline; a word's or grid's place shows.
- Kittests: adding Black hole and Form a word from the Add thing menu; the Motion list adds a Pull to force.

## Steps and how each is verified

1. Runtime `affects_beside` + scene `SceneBeat`: runtime test (beside fingerprint moves with an `affects_beside` key,
   the physics fingerprint doesn't); kit test (`beat_of` reads the scene's tempo; default without).
2. Spots: unit tests: a grid by columns × rows has exactly that many points inside the rectangle; by spacing covers it
   at that spacing; Stagger shifts odd rows half a cell; Jitter stays inside its cell and is the same every time; a
   word's spots equal the old point-in-letter test on a coarse lattice (the scanline is exact); an empty word has none;
   a 4 × 4 PNG written to a temp folder gives the expected solid / bright / dark places; a missing file gives none.
3. Source and births: On the beat at 120 bpm makes Count at 0 s, 0.5 s, 1 s… (Bar every 2 s); Keep a crowd on a grid
   fills every place once and replaces a killed particle at its own place; Once at the start fills every place; homes
   follow a moved anchor and Homes turn; Round the middle flies sideways.
4. Forces: one particle each: Pull moves it towards its target and Swallow kills; 1/d² weaker than Even far away; Orbit
   adds sideways speed; Its ball follows the particle's own ball; Push the other way; Balls nearby only within Reach;
   Spring home returns and settles; Flock aligns two neighbours' velocities and holds speed in range, and skips itself;
   Magnetic field turns a needle to the dipole's field line; Flow field noise is followed at Speed; Spirograph puts the
   particle on its curve; Pulse gives Strength over a beat; Ripple pushes only particles near the ring; A share / The
   rest split like a layer's Only.
5. Determinism: a Flock + Pull + Ripple scene over 9000 particles gives the same with 1 thread and 8; a seek back and
   forth equals a straight run (the goldens test does it per scene).
6. Looks: every look (old and new) resolves and makes particles within 2 s (the existing test covers new variants);
   Moiré adds two things.
7. Goldens: `golden_particles_motion.rs` over `particle_motion_scenes` (Galaxy riding, Form a word, Black hole, Flock
   with balls, Shockwaves, Iron filings, Beat grid, Spirograph riding, Comet tail riding, Flow lines), blessed in debug
   and release; part 1's file re-blessed (Fireflies lines only differ); ball goldens unchanged.
8. Editor: grid corner handle drags Width/Height; kittests for the menu and the force list.
9. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace` (debug), release goldens, CLAUDE.md map,
   Windows exe builds.

## Not doing (part 3)

- No air (part 2), feeding children, Pop/Stick/Settle/Pile, two-way (part 4), new layer kinds (Needles, Birds, Web
  lines: part 5; Trails: part 6), lighting/sound/rules (part 7).
- No JPEG pictures (PNG only), no multi-line text, no handle for a force's Point (typed in the force's settings).
- No change to `layer.particles` / `shape.particles`; no built-in or example scenes; no demo videos.

## As built

- Step 1's runtime test became a kit test (`tests_motion::the_tempo_reaches_the_world_and_restarts_only_the_particles`):
  the physics fingerprint stays and `beside_fingerprint` moves when the tempo changes.
- **Pulse on the beat** reads every beat so far (`beats::Beats`: since the latest, gap, count) and sums the older kicks'
  tails as a geometric series (`more_forces::pulse_share`), so a Decay longer than the gap still gives each kick its
  whole Strength.
- A word's or picture's lattice is widened to at most `4 × MOST_SPOTS` cells (`spots::lattice_step`) so a huge Picture
  width or Letter height can't hang the particles; PNGs decode with a limit that fits `MOST_PIXELS`.
- A picture's modified time is read when a run starts; a refresh partway through keeps it, so a picture changed on
  disk shows on the next restart of the particles (any setting change rebuilds them), never partway through a run.
- Pull / Push toward **Balls nearby** with Reach 0 does nothing (0 "everywhere" is for a single centre).
- Known and left as they are: a keyed Width/Height on a Spacing grid changes the column count, so place numbers move
  to other places while it changes; the preview's rectangle-grid outline turns with the thing and Turn but not with
  Homes turn (the editor's world doesn't step the particles).
- Kittests: the Motion list adds Pull to / Spring home / Flock; a picture source shows the PNG browse button and a grid
  its rows; Moiré from the Experimental group adds its two grids (in place of Black hole / Form a word, which the kit
  tests add and resolve).
