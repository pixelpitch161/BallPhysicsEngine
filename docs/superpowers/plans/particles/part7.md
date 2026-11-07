# Particles part 7: lighting, sound and rules

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` sections 6 (beyond part 1) and 7, and section 11 part 7.
Approved by the owner in the brainstorm thread; built straight after this plan (no approval stops), one Opus review at
the end, then the PR, merged by this thread once checks and the review pass. The exe is not installed by this part:
that happens once every particles part is merged.

Part 1 (`part1.md`, "As built") is the base: a Particles thing is decoration stepped beside the physics
(`ThingSim::in_main` false, `World.beside`, `main_part`), drawn as around layers (`ParticlesDraw::around`/
`draw_around`), its state `ParticlesState` with `Speck`s in `World.states`.

## What part 7 delivers

1. **Lighting** (section 6). Every drawn layer of a Particles thing's Look (`look.particles`: Sprites, Streaks, and
   whatever drawn kinds parts 5 and 6 add) gets the around layers' **Lighting** fold (`AroundLit`: Lit, Blocks light,
   Catches beams, Under UV, UV glow). A new sub-layer **Gives off light** (`particles.sub.light`, the `GivesLight`
   settings) makes a layer's particles lamps through the light field (fire lights the room). What a layer draws is
   stamped with its lighting exactly as a ball's around layers are (`Particle.light`, `.emit`, `.extra`). The Lights
   panel lists each Gives off light sub-layer of a Particles thing with its fader, M and S. Stage draws them flat on the
   picture plane as it does every around layer (nothing new). Without a Lighting card nothing changes.
2. **Moments** (section 7). A Particles thing notes what its particles did this step: **Hit a ball**, **Hit a shape**,
   **Land** (stopped on a shape), **Born** (in a burst) and **Burst** (a burst went off). They are kept on the state, not
   logged. Parts 4 (Pop, Splash, settling) append their own kinds.
3. **Sound** (section 7). A Particles thing can have a Sound tab like a shape's (the same `sound.voice` card, played by
   its moments). The Sound tab adds a **Moments** panel for Particles things: which moments play notes, **Most notes a
   second** (thinning keeps the strongest) and **Strength from** (speed, size, both, or the same for all). Follow hit
   pans by place, as on a shape. The sound engine's playback keeps a Particles thing that sounds (it is "heard"), the
   balls and the editor's main playback don't.
4. **Rules** (section 7). A When block **Particles** (`when.particles`): Who (a Particles thing), Moment (Any or one
   kind), At least (N in a step); it fires once a step for each thing with that many, with values `count`, `speed`
   (the fastest) and `size`, at the place of the strongest moment. A Particles thing a rule reads steps with the
   physics (the rule changes the balls), so the editor's main playback builds it and its edits run the scene again.
5. **Pulses on hits** reach particle settings (part 1 left them still): the world gives a Particles thing's
   pulses the hits, as the picture does. Follow sound already works on them (part 1's `World::animate` passes the
   sound); a test pins it.
6. **Looks**: Fire on a ball, Spark fountain, Embers and ash and Fireflies get a Gives off light sub-layer on their
   glowing layer (it lights a scene only with a Lighting card; nothing changes without one).

## Decisions made while planning (fill-ins of the spec, stated so review can correct them)

1. **Lighting fold for any drawn particle layer.** `Kind::light_defs` returns `AroundLit::defs()` for a drawn layer
   (`LayerSection::Drawn`) of the `look.particles` card, so the drawn kinds parts 5 and 6 add get it without touching
   `lighting::defs_of`. Particles are smoke-like by default: Lit 1, Blocks light off, Catches beams 0.
2. **The draw stamps like a ball's around layer.** `look::LookLayer` gains `light: [f32; 4]` (`lighting::of`),
   `more: [f32; 4]` (`lighting::extra_of(.., 0, true)`) and `emit: Option<Emit>` (`Emit::of_subs` over the layer's
   subs; a lamp is unlit, `unlit_if_lamp`). `draw_around` records `Drew::from(frame)` before it draws and stamps after
   (`Drew::stamp(frame, light, more, cx)`, the body of today's `Drew::light`, which now calls it), and the layer's own
   pieces get `Pieces::give_light(emit)` before they go into the frame. `SubLayer::Light` is read by `sub_of`; it adds no
   piece of its own and doesn't change colours. Flare on hit doesn't apply to particles (as for trails, its help says).
3. **Light probe**: particles are not lamps for the CPU light probe (Follow light, moths, Light spreads), as ball
   trails and particle layers aren't today. The probe is built from the main world, which has no beside things.
4. **Moments live on the state.** `ParticlesState.moments: Vec<Moment>` (at most `MOST_MOMENTS` 4096 kept a world step,
   the rest only counted), `moment_counts: [u32; MOMENT_KINDS]` (every one counted), `moments_step: u64` (the world step
   they belong to). `after_step` clears them when a new world step starts (so with several particle steps in one world
   step, at a Physics rate under 120, they add up; at 240 half the world steps have none). The first births, made when
   the world is built, are the moments of world step 1. `Moment { kind, place, velocity, speed, size, with, seed }`:
   `speed` is how fast it went into what it met (the speed along the normal against a ball or shape; a birth's own
   speed), `with` the ball's or shape's thing (`None` for births).
5. **Moments are found deterministically after the parallel move.** `move_one` (on the chunk threads) only marks a
   particle: `Speck.met` (`MET_BALL`, `MET_SHAPE`, `MET_LAND` bits in a new `u8`) plus `Speck.impact` (the speed in) and
   `Speck.with` (the thing id, `u32`). The serial `retain_mut` pass that removes the dead then turns marks into
   moments in particle order, and clears them. **Land** is once per particle (`LANDED` in `flags`, never cleared): a
   particle stopping against a shape (At shapes: Stop) lands the first time and is quiet after, though it touches the
   wall every step. **Hit a shape** is Bounce or Vanish at a shape; **Hit a ball** is Bounce off or Vanish at a ball
   (Pass makes no moment). `BallAt` gains `thing` (the ball's thing) for `with`.
6. **Born and Burst.** `births::births` notes a **Burst** for each burst that goes off (Bursts, Once at the start, each
   hit of On hits; place: the source's place, or the hit's; `speed` the hit's speed or 0; `size` the count born) and a
   **Born** for each particle born in one. Steady and Keep a crowd make neither (the spec's "bursts only").
7. **Sound events are logged, thinned.** A Particles thing whose Sound card is on and plays on some moment turns its
   moments into log events of a new kind, `EventKind::Moment` (tag 22): `a` = (the particles thing, the particle's seed),
   `b` = (the particles thing, the moment kind's code), `point` the place, `normal` the particle's direction, `speed` the
   strength × `STRONG_HIT` (1500, so the voice's `strength()` gives it back), `at` 1 (the end of the step). At most
   **Most notes a second** go into the log: a budget (`ParticlesState.note_budget`, starting at 1, growing by Most ×
   the world step's seconds, capped at max(1, Most × 0.1)) takes the strongest of the step's moments (ties: earlier
   first), whole notes only. So rain never floods the log. Only the playing moment kinds count. Strength from: Speed
   (`speed / 1500`), Size (the half size over 6 px), Speed and size (their product), The same (1). All clamped to 0..1.
8. **The voice is the shape voice.** `VoiceKind::fits` takes `particles` too. `VoiceSettings` gains the Moments
   settings (`moments_hit_ball` on, `moments_hit_shape` on, `moments_land` on, `moments_born` off, `moments_burst` off,
   `moments_most` 30 per s (range 1..2000, not keyed), `moments_strength` Speed); shapes ignore them. `HitCursor::read`
   and `Engine::catch_up_these` take `Moment` events as they take hits; `HitFilter::accept` takes a Moment on its own
   thing without Heard from (the moments are its own particles'); everything after (min strength, retrigger per
   particle, notes, slots, modulators, Follow hit pan) is a hit's. Distance modulators and In range read nothing for a
   Particles thing (they measure to a shape's outline); By side and the Hit modulators' place read 0, as `unwrap_or`
   already gives.
9. **Who hears the particles.** `ThingSim::heard(&ResolvedThing) -> Option<u64>`: a beside thing whose events the sound
   hears, with a hash of what of its sound card changes them (Particles: the card on, the five moment switches, Most and
   Strength from). `bpe_runtime::heard_part(scene, resolved, sims)` is `main_part` plus the heard beside things: the sound
   engine (`Engine::with_patch`) builds its physics from it. `sound_fingerprint` mixes in each heard beside thing's
   beside hash (its settings, Motion, Meets, keys, as `beside_fingerprint` hashes them) and its `heard` hash, so a
   particle edit rebuilds the sound (not the balls) and a volume change only patches it.
10. **Rules read particles in the main world.** `BlockSim::keeps(&self, params, thing, kind, tags) -> bool` (default
    false): a block that reads a thing stepped beside the physics, which then steps with it. `when.particles` keeps the
    thing its Who names (`Who::Thing`), or every Particles thing for any other Who (tags can change at run time).
    `bpe_runtime::is_beside(scene, sims, id, r)` = `!in_main(r)` and no enabled rule's block keeps it; `main_part`,
    `has_beside`, `beside_fingerprint` and `World::build` (filling `World.beside`) use it, so `main_part` and
    `has_beside` take the scene now. `ThingSim::own_cards() -> bool` (Particles: true): its Motion and Bounce cards have
    no module sims, so when it steps with the physics the fingerprint hashes them whole.
11. **The Particles block.** `when.particles`, section When, settings Who (`Who`, default Anything), Moment
    (`MomentPick`: Any, Hit a ball, Hit a shape, Land, Born, Burst), At least (Int 1, range 1..1 000 000). Fires once
    a stage per matching, unbroken Particles thing whose count of that moment this world step (from `moment_counts`,
    so past the 4096 kept too) is at least At least: subject (the thing, `WHOLE`), other the strongest moment's `with`
    (whole) when it met something, point its place, normal its direction, values `count`, `speed` (the fastest kept) and
    `size` (the mean half size of those kept). Values are named in `block_kind!` (`values = ["count", "speed", "size"]`).
12. **Pulses on hits.** `ThingSim::pulses_on_hits() -> bool` (Particles: true). `World::animate` works out the hits
    (`bpe_runtime::hit_strength`, moved from `bpe-gfx` and re-exported there, same window, fade and full hit) only when
    such a thing is animated, and applies them to those things alone (`Animation::apply_where`); every other thing keeps
    0 hits, so the physics is as before (goldens).
13. **Follow sound menus** list shapes only: a Particles thing's own sound is heard in the sound engine's world, never in
    the editor's main world, so its Level can't be followed.
14. **No scene format change**: the new settings have defaults, and old files open as before.

## Files

### `bpe-core`
- `animation.rs`: `Animation::apply_where(resolved, time, inputs, keep: impl Fn(ThingId) -> bool)`; `apply` calls it.

### `bpe-runtime`
- `events.rs`: `EventKind::Moment` (tag 22), `hit_strength` (moved from `bpe-gfx::frame`, with `HIT_WINDOW`, `HIT_FADE`,
  `FULL_HIT`).
- `plugins.rs`: `ThingSim::heard`, `own_cards`, `pulses_on_hits`.
- `rules/sim.rs`: `BlockSim::keeps`.
- `world.rs`: `is_beside`, `heard_part`, `main_part(scene, ..)`, `has_beside(scene, ..)`, `beside_fingerprint` and
  `sound_fingerprint` as above, `fingerprint_of` hashing own cards, `World::build` filling `beside` through
  `is_beside`, `animate` with hits for `pulses_on_hits` things.
- `lib.rs`: exports.

### `bpe-gfx`
- `frame.rs`: `hit_strength` re-exported from the runtime.

### `bpe-kit`
- `things/particles/moments.rs` (new): `MomentKind` (with `code`, `ALL`), `Moment`, `MOST_MOMENTS`, `MOMENT_KINDS`,
  `collect` (marks to moments), `note` (push one, counting), `MomentSound` (the Sound card's moment settings, read
  from a resolved thing), `strength_of`, `thin` (the budget and the strongest), `to_events`.
- `things/particles/state.rs`: `Speck.met/with/impact`, `LANDED`; state `moments`, `moment_counts`, `moments_step`,
  `note_budget`, `sound: Option<MomentSound>`.
- `things/particles/meets.rs`: marks in `meet_balls`/`meet_walls`; `BallAt.thing`.
- `things/particles/step.rs`: the collecting `retain_mut`; `balls_in` fills `thing`.
- `things/particles/births.rs`: Burst and Born moments.
- `things/particles/sim.rs`: clearing moments each world step, the sound events, `heard`, `own_cards`,
  `pulses_on_hits`; `read_into` reads the Sound card.
- `things/particles/look.rs`: `SUB_LIGHT` kind, `LookLayer.light/more/emit`, `sub_of` reads Gives off light.
- `things/particles/draw.rs`: the stamping.
- `things/particles/looks.rs`: Gives off light on the four glowing looks.
- `things/particles/rules.rs` (new): `ParticlesWhen` (`when.particles`), `MomentPick`, its settings.
- `modules/look/layers/mod.rs`: `Kind::light_defs` for drawn `look.particles` layers.
- `modules/look/layers/around.rs`: `Drew::stamp`.
- `rules/mod.rs` (registration of the block, or `lib.rs`), `sound/voice.rs` (`fits`, the Moments settings,
  `MomentStrength`).

### `bpe-sound`
- `engine.rs`: physics from `heard_part`; `catch_up_these` takes Moments. `hits.rs`: `read` takes Moments.
  `voices/hit_filter.rs`: a Moment on its own thing skips Heard from.

### `bpe-studio`
- `inspector.rs`: the Settings | Sound chips and `sound_open` for Particles things too.
- `sound_tab/mod.rs` and a new `sound_tab/moments.rs`: the Moments panel (Particles things only), Heard from and In
  range left out for them; the empty card's line says "plays a note when its particles hit things".
- `lights_panel/mod.rs`: `GIVES_OFF_LIGHT` gains `particles.sub.light`.
- `follow.rs`: shapes only.
- `app.rs`, `particle_job.rs`, `aim_job.rs`: the new `main_part`/`has_beside` signatures.

### `bpe-cli`
- nothing new (export builds the whole world; the sound engine uses `heard_part`).

## Steps and how each is verified

1. Runtime and core plumbing (events, hooks, `is_beside`, `heard_part`, fingerprints, `apply_where`, `hit_strength`).
   Runtime tests: `is_beside` honours a block's `keeps`; `heard_part` keeps a heard beside thing; the physics goldens and
   sound goldens don't move.
2. Moments: unit tests in `things/particles/tests.rs`: rain on a shape that Stops makes Land once per particle; Bounce
   at a shape makes Hit a shape with the speed in; Bounce off a ball makes Hit a ball with `with` the ball's thing;
   bursts make one Burst and Count Born; moments clear on the next world step; 1 thread equals 8 for moments too; the
   particle goldens don't move (the moments change no particle).
3. Sound: kit tests: a sounding thing logs at most Most a second, the strongest first; strength from size; nothing logged
   without a Sound card. `bpe-sound` test (`tests/particle_sound.rs`): rain with a voice on a shape renders sound
   (non-silent) only when its sound is on, the same twice (determinism), and after a seek the same as straight through;
   the engine's physics keeps the heard thing (`heard_part`), and a ball run with it hits the same as without.
4. Rules: kit tests: `when.particles` fires when rain hits a shape (At least 1 and a high N that never fires), a Do
   Break on it breaks a ball; the thing is in the main part only while an enabled rule reads it; the fingerprint
   changes when its Meets card changes while it is in main.
5. Lighting: kit tests: a Gives off light sub-layer stamps `emit` on the layer's own sprites only, a lamp is unlit,
   Lit/Blocks light stamp `light`, UV and Catches beams stamp `extra` only with a Lighting card; the fold's settings are
   offered on Sprites and Streaks. GPU test in `tests/particle_light_pictures.rs`: a dark lit room with particles that give
   off light is brighter near them than without the sub-layer.
6. Pulses: a kit test: a Particles thing's Rate pulsed by hits makes more particles after a hit than without the pulse;
   a ball scene with a hits pulse on a physics setting runs as before.
7. Editor (kittest): a Particles thing shows Settings | Sound; Enable sound adds the card; the Moments panel's switches
   write their keys; the Lights panel lists a particle Gives off light sub-layer; the Look's Lighting fold shows Lit.
8. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace` (debug), release goldens, CLAUDE.md map, the
   Windows exes build.

## Not doing (part 7)

- No particle lamps in the CPU light probe; no haze stirring by particles; no Flare on hit for particles.
- No Pop, Splash, Settle moments (part 4 adds them with their kinds); no On the beat (part 3).
- No Follow sound of a Particles thing's own sound.
- No change to `layer.particles` / `shape.particles`; no built-in scenes; no demo videos.

## As built

- **Moments** (`things/particles/moments.rs`): `MomentKind` Hit a ball / Hit a shape / Land / Born / Burst, kept on
  `ParticlesState.moments` for world step `moments_step` (at most `MOST_MOMENTS` 4096 kept, every one counted in
  `moment_counts`; several particle steps in one world step add up, `start_moments`). Meeting code marks a `Speck`
  (`met`, `with`, `impact`; `MET_BALL`/`MET_SHAPE`/`MET_LAND`; the first meeting of a step names `with`) and the serial
  retain pass in `step.rs` turns the marks into moments (`collect`), so they are the same on any thread count. A Stop
  lands once (`LANDED` flag). `births.rs::note_burst` notes one Burst (size = how many, speed = the hitting ball's) and
  a Born per particle for Bursts, On hits and Once at the start. The first births (in `after_build`) count as step 1's.
- **Sound**: `VoiceKind::fits` takes particles, and the Particles kind's `slots()` gains `SlotId::Sound` (the tests
  found it missing). `VoiceSettings` gains the Moments group (`moments_hit_ball`, `moments_hit_shape`, `moments_land`
  on; `moments_born`, `moments_burst` off; `moments_most` 30/s; `moments_strength` `MomentStrength` Speed / Size /
  Speed and size / All the same). `MomentSound::of` reads it; `log_notes` logs the strongest playing moments as
  `EventKind::Moment` (tag 22: `a` = (thing, seed), `b` = (thing, kind code), speed = strength × 1500) within a budget
  that grows by Most a second and holds at most max(1, Most/10). `bpe-sound` plays Moment events like hits
  (`hits::plays`; `HitFilter::accept` skips Heard from for the thing's own moments). The engine's physics is
  `heard_part` (main plus beside things whose `ThingSim::heard` is some); `sound_fingerprint` mixes them in.
- **Rules**: `when.particles` (`things/particles/rules.rs`: Who, Moment `MomentPick`, At least; values count, speed,
  size; subject the thing, other the strongest moment's `with`, point its place). `BlockSim::keeps` lets the block pull
  the Particles things it may read into the main world (`is_beside` false while an enabled rule keeps it); then
  `ThingSim::own_cards` makes the fingerprint hash its Motion and Meets cards whole.
- **Lighting**: every drawn layer of `look.particles` takes `AroundLit`'s Lighting fold (`Kind::light_defs`); the
  `particles.sub.light` Gives off light sub-layer; `layers_of` works out `light`/`more`/`emit` (a lamp is unlit,
  `unlit_if_lamp`); `draw_around` stamps through `Drew::stamp` and `Pieces::give_light`. Fire on a ball, Spark
  fountain, Embers and ash and Fireflies give off light. The Lights panel lists the sub-layer.
- **Pulses**: `ThingSim::pulses_on_hits` (true for particles) lets a hits pulse move a beside thing's settings:
  `World::animate` runs `Animation::apply_where` for those things with the log's `hit_strength` (moved from `bpe-gfx`
  to `bpe-runtime`). Keys and Follow sound already worked.
- **Editor**: `inspector::sounds(kind)`; the Sound tab's Moments panel (`sound_tab/moments.rs`, "Plays when its
  particles"); Playing chips On each moment / Always (no In range, no Heard from); Follow sound ▸ lists shapes only.
- **Tests**: `things/particles/tests_part7.rs` (moments, the budget, `heard_part`, rules, fingerprint, pulses,
  lighting stamps, the fold), `bpe-sound/tests/particle_sound.rs` (plays, silent without the card, deterministic, the
  same after a seek), `bpe-kit/tests/particle_light_pictures.rs` (the light round glowing particles), kittests in
  `sound_tab/mod.rs` and `lights_panel/tests.rs`. Goldens unchanged.
- **After the review**: `sound_fingerprint` hashes every heard thing's sound print, beside or kept in the physics by
  a rule, and the Follow sound fingerprint when a heard beside thing's setting follows sound; `World.decoration` (every
  thing whose sim isn't `in_main`, kept by a rule or not) is always left out of `World::lone`, so aiming never steps
  particles; a Speck keeps its first ball and its first shape apart (`with`/`impact` are `[_; 2]`, `MET_AT_BALL`,
  `MET_AT_SHAPE`); a burst whose particles the cap all refused notes nothing; `animate` finds the pulsed things without
  building a set. The Moments settings sit in the "Playing" section, and Most notes takes keys.
- **Gaps**: particles are no lamps for the light probe; no Follow sound of a Particles thing's own sound; Distance
  and In range modulators read nothing for particles; the timeline's slot-change ticks count shape hits only (the
  editor's log has no particles); a shape's Sound card carries the (unused) Moments settings too, so generic setting
  lists such as a rule's Change setting menu show them on shapes; `when.particles` with any Who but one thing keeps
  every Particles thing in the physics.
