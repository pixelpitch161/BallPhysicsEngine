# Particles thing (spec `docs/superpowers/specs/2026-11-06-particles-design.md`, plans `docs/superpowers/plans/particles/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

Part 1 is in (plan `part1.md`): the **engine**. A **Particles** thing (`bpe-kit/src/things/particles/`, kind
`particles`, family "Particles") makes its own particles, separate from the `layer.particles` Look layer (which stays
as it is). Its own settings are the **Source** (`ParticlesSettings`: From Whole screen (Edge, Margin: the camera's shot
bounds) / An area (Rectangle, Circle, Ring) / A point / A line / Riding its parent (Each ball, The middle, Ball number,
The edge, Inside: the tree parent's balls or wall, `RIDES`); Emit Steady (Rate, a ball when riding each ball) / Bursts /
On hits (Who hits, Least speed, Follow impact) / Keep a crowd / Once at the start; each particle's Life, Speed,
Direction, Angle, Spread, Takes the parent's speed, Size, Turn, Spin, their spreads, Seed; Most particles (cap,
`MOST_EVER`)). Its cards are its own (`ModuleKind::fits`; `motion.basic`/`bounce.basic` don't fit it, and the inspector
shows them as cards of their own, not the Physics card): **Motion** `motion.particles` (`forces.rs`, force layers in
the new `LayerSection::Force`: Gravity, Wind, Drag, Swirl, Wander, Rise, Sway and flutter, each with Amount and Only
for, applied top of the list last), **Meets things** `meets.particles` in the Bounce slot (`meets.rs`: balls push
(`BallGrid`, at most `MOST_NEAR` 32 a particle) and Pass / Bounce off / Vanish, shapes Pass / Bounce / Stop / Vanish
through `keep_clear_from` against each wall's outline at the last particle step, the picture's edges Pass / Wrap round /
Bounce / Vanish) and **Look** `look.particles` (`look.rs`: draw layers Sprites (the `sprites.wgsl` shapes) and Streaks
(stretched by speed), common Where, Only (All / Younger than / Older than / A share / The rest), Colour from (Own paint
following life/order/place/speed/time, or the Parent ball's colour), Opacity, Size ×, Size over life, Fade, Twinkle,
Blend; sub-layers `particles.sub.glow/outline/shadow/recolour/flash/hue` with the shape sub-layers' settings). The
state (`state.rs`: `ParticlesState`, `Speck`s oldest first) lives in `World.states`, stepped in `after_step` at 1/120 s
(`step::catch_up`: the particle steps `due` by the world's time × 120, so lives keep time at any Physics rate; hits
since `ParticlesState.world_step` let particles off; `step.rs`: births, then the move in chunks of `CHUNK` 4096 on `std::thread::scope`, the same
for any thread count, then a stable `retain`); first births in `after_build`; every birth's dice from
`seed_of(world seed, thing, birth number, Seed)` (`births.rs`). `World::animate` calls the thing sim's
`settings_changed` for keys on cards with no module sim, so keyed forces work. The draw (`draw.rs`) draws nothing
itself and lists each shown Look layer as an `Around` (`draw_around` makes sprites through
`around_subs::particle_with_extras`); lighting fields stay 0 (part 7). **Decoration only**: `ThingSim::in_main` is
false, so `World.beside` lists it, `main_part(resolved, sims)` leaves it out, fingerprints hash the main part only,
and `beside_fingerprint` is what a particle playback is built from. The editor's main playback, the sound engine
(`Engine::with_patch`) and the aiming thread build from `main_part`; `bpe-studio/src/particle_job.rs` (`ParticleJob`)
runs the whole scene on its own thread, follows the playhead (`follow`) and publishes `Snapshot`s; the preview lends
their states to its world while it draws (`World::lend_states`/`take_back`), reads the latest snapshot once a
frame, steps the playback to its moment (`Studio::sim_target_with`; the job follows `aim_target`) and lends it only
when its step is the playback's (`particles_there`); "Particles… N%" shows past `PARTICLES_BEHIND` (0.25 s), and a
riding thing's handles use `World::anchor_as` (the main world doesn't build it). The status line shows "Particles: N, X ms".
Export (`bpe-cli`) builds the whole world. `looks.rs`: the Add thing menu's Particles heading (Empty, then
`GROUPS`: Rain, Snow, Hail, Blossom petals, Sandstorm; Fire on a ball, Spark fountain, Embers and ash; Fireflies,
Confetti; Rainbow smoke trail, Ink in water), `add_particles` (riding looks ride a Balls/Emitter/Shape parent).
Editor: `look_editor/particles.rs` (the Forces and Look lists; a Motion card's editor state is keyed apart,
`look_editor::pick_key`), `preview/particle_handles.rs` (area corner/edges, line end). Scene format 12. Goldens:
`tests/golden-particles*.txt` (`scenes::particle_scenes`, test-only).

Part 2 is in (plan `part2.md`, "As built" at its end): **the air**. The scene's **Air card** (`SlotId::Air`, last in
`SlotId::ALL`; kind `air`, `bpe-kit/src/modules/air.rs`: Wind, Wind direction, Gusts, Gust every, Gust waves; Calms
down, Keeps its swirls, Resolution Coarse/Normal/Fine 32/16/8 px, Margin; Balls stir, Stir reach, Shapes block the
air; Heat rises, Heat fades). The solver is `bpe-kit/src/air.rs` (`Air`, `AirSpec`, `gust_factor`): a world-anchored
lattice over the camera's shot plus Margin (cells kept when the camera moves, at most `MOST_CELLS`), each step calm
to the ambient wind, balls stir, heat lifts, vorticity confinement, semi-Lagrangian advection, solid shape cells
moving with their wall, 20 Jacobi pressure iterations; rows split in fixed bands on threads past `THREADED_CELLS`,
the same for any thread count. One `Air` per world in `World.states[ThingId::ROOT]` (`air_of`, `take_air`/`put_air`):
the scene sim puts a sleeping one there with an Air card and re-reads it on Air keys; a Particles thing that follows
or heats it (or any, with a card) wakes it in `after_build` (a late wake starts still at that step); the first
Particles thing due a step takes every Particles thing's steps together, one particle step at a time: the air to
that step (`Air::catch_up_to`), each thing's `step::step_in`, then its heat (`step::put_heat`), so at any Physics
rate every thing reads the same air. Air keys following sound or light reach only `beside_fingerprint`.
Off the grid the air is the ambient wind. Forces: **Follow the air** (`force.air`, Grip), Rise's **Heats the air**,
Wind's **Gust waves** (0 = part 1's maths). `world::BESIDE_SLOTS` (`[Air]`): Air keys are animated, left out of
the physics fingerprint and hashed by `beside_fingerprint` (an Air edit restarts only the particle playback).
`things/particles/air.rs::ensure_air_card` (by `add_particles` for looks that follow the air and the editor's Add
layer of `force.air`). Editor: the toolbar's **Air** toggle (`Studio.show_air`, shown with a particle job) draws
`preview/air_view.rs` arrows from the particle job's snapshot (which carries the ROOT air). Looks Dust clouds,
Thick fog; Sandstorm with gust waves and a haze layer. Goldens gain "Dust in the air", "Smoke heating the air",
"Sandstorm". What part 8 needs is listed in the plan.

Part 4 is in (plan `part4.md`, "As built" at its end): **particles meeting things**. **Feeding**: Born from **Its
parent's particles** (`BornFrom::ItsParentsParticles`, the tree parent a Particles thing) When they (`FedWhen`) Die /
Land / Hit a ball / Hit a shape / Hit anything / Pop / Stop rising (Count each) or Every step (Rate a parent particle);
the parent keeps that step's `feeds` (`feeds.rs`: `Feed`, `FeedKind`, only the kinds its children `wants`,
`LAND_SPEED`), and in part 2's lock-step particle loop a fed thing steps right after its parent (tree order),
reading the feeds of that very step, so it sees every one at any Physics rate (`step::wants_of`; fed things are
always decoration, at most `MOST_FED` births a moment); `births::fed_places`, `Speck.kin_seed/kin_life`, Colour from **Its parent particle** (`ColourFrom::ParentParticle`).
**Meets things** gains On touching a ball Pop / **Stick** (`STUCK`, `Speck.on/on_row/offset`, rides the ball, falls off
on a hard landing at Fall off when it lands, `SHAKE_SPEED`, `LOOSE` until it leaves the ball) and At shapes **Settle**
(`RESTING`, rides a moving shape, `age` stands still, kicked up by a ball faster than `KICK_SPEED`, Fade after
`SETTLE_FADE`, Most kept) / **Pile up** (`piles.rs`: `Piles`/`Pile` height columns `COLUMN` 2 px wide on still up-facing
walls and the floor (edges Bounce: `MeetsSettings::floor`), `Surfaces::of`, `add_sand` slides to the Angle of repose
(`MOST_SLIDES`, `SLIDE_LEAST`), pours off ledges (`spill`), crumbles when its shape moves (`crumble_moved`), cut by balls
(`cut_by_ball`, sand Thrown up, `throw_sand`, `MOST_THROWN` in one go); touching piles on one surface are joined (`merge_touching`); layer **Piles** `particles.piles` draws one upright
streak a column). **Two-way** (`two_way.rs`, Meets things' Two-way group: Weight, Pushes / Slows / Holds up / Weighs
balls): `in_main` is then true, so the thing runs in the main world and counts in the fingerprint (`ThingSim::steps_own_cards`
makes `fingerprint_of` hash its own Motion/Bounce cards, and then the Air card and its keys too); `TwoWayOut` sums each particle step's momentum per ball in
chunk order, given once in the thing's `forces`, stuck weight every substep, and `hold_up` (the `settle` hook) rests
balls on piles (digging in past `DIG_SPEED`). Looks: Fire and smoke (Flame; its Smoke child Soft smoke), Bubbles, Sand that
piles up, Falling leaves (part 5's Leaves), Waterfall and mist, Liquid splashes, Fireworks, Sticky dust; Thick fog is two-way (slows the balls); part 3's Flock draws part 5's Birds; Rain splashes (`Recipe.children`,
`add_recipe`).
Editor: the Add thing menu's "Empty, fed by <name>" while a Particles thing is selected; the status line also counts
the main world's (two-way) particles. Goldens `tests/golden-particles-meeting*.txt` (`scenes::particle_scenes_meeting`).

Part 5 is in (plan `part5.md`): **looks 1**, drawing only. New `look.particles` layer kinds: **Soft smoke**
(`particles.smoke`) and **Flame** (`particles.flame`) are soft **fields**: `bpe_gfx::Field { at, splats, look }` in
`Frame.fields`/`fields_front` (`Splat`, `FieldLook`: mode `FIELD_SMOKE`/`FIELD_FLAME`, adds, amount, strength, detail,
shade, light/emit/extra, 32 baked heat stops), splatted additively into a half-size Rgba16Float picture each right before
the pass that draws its list, then shaded by a full-screen triangle where `at` puts it among the list's sprites
(`bpe-gfx/src/fields.rs`, `shaders/fields.wgsl` pasted after sprites.wgsl + each.wgsl; `Renderer::draw_sprites` splits
the sprite runs, `Sprites::draw_range`; `cull` keeps `at` between the same kept sprites). Smoke: density × Thickness ×
drifting fbm detail, cover 1 − e^(−d), shaded by the slope against Light from, Dark inside; Flame: read through rising noise
(Bend), heat × detail × flicker → Heat colours, tinted by the layer's Colours. A field layer's Shadow/Glow/Outline subs
draw per particle (as Smoke sprites, own sprites dropped; `Pieces::into_frame_with_field`). **More shapes**
(`particles.shapes`, `MoreShape`: sprite codes 10 leaf, 11 petal, 12 flake, 13 needle, 14 bird, 15 fish in sprites.wgsl,
`Particle.shape[1]` the pose; Pixels are Confetti boxes snapped to Pixel grid; Point along Own turn / Way it goes),
**Glitter** (`particles.glitter`: code 16 diamond facets, `draw::glint`, a Stars flash), **Web lines** (`particles.web`,
`things/particles/web.rs`: `links` nearest within Reach, each pair once, `ball_links`; lines are Streaks capsules), and
every layer's **Fine detail** fold (`copies`, `copy_spread`, `copy_size`, `copy_opacity`, `copy_shimmer`; `FineDetail` in
`Common`, `draw::copy_offset`). Looks: Glitter, Web of light, Pixel glitch added; Fire on a ball (Flame + embers), Rainbow
smoke trail, Ink in water (Soft smoke) and Blossom petals (Petals) upgraded. `tests/particle_pictures.rs`,
`bpe-gfx/src/fields_tests.rs`.
Part 6 is in (plan `part6.md`, "As built" at its end): **trails, marks, liquid and the experimental modifiers**, all
drawn only (goldens unchanged). Look layers `particles.trails` (`trail.rs`: `TrailsLayer` Length, Every, Width,
Taper, Fade along, Softness; the state's `TrailRing` keeps each particle's last places, slot = particle step %
points, `grow` on births, `keep` with the retain, `write` after the move; `recent` reads newest first; strips with
Shadow/Glow/Outline subs, cut at jumps), `particles.marks` ("Marks that stay", `marks.rs`: `Mark`s in
`ParticlesState.marks`, made in `step::end_step` from the touch flags `meets.rs` sets in `Speck.flags` bits 16..19
(once per particle a reason: On shapes, On the edges, When they die), `prune` caps at Most marks and drops faded ones;
drawn as dots with splats and a drip) and `particles.liquid` (`liquid.rs`: `LiquidLayer` Blob size, Level, Edge
softness, Roundness, Surface Matte/Plastic/Chrome/Metal/Glass/Toon, Light angle, Shine, Bend, Tint, Sky, Rim; no
sub-layers, no Colour split). The stepping reads only `wants.rs::Wants` (trail points, marks kept) from the Look, and
`ThingSim::beside_print_extra` (hashed by `beside_fingerprint`, never the main part's) rebuilds the particle playback
only when that changes. A liquid is `Frame.liquids` (`bpe_gfx::Liquid { blobs, style, front }`): `bpe-gfx/src/liquid.rs`
`LiquidPass` splats `LiquidBlob`s additively into a full-size Rgba16Float field (its own bind group: style only) and
`shaders/liquid.wgsl` `fs_surface` cuts it at Level and shades it (Glass samples the copied-under picture bent by the
normal), after the scene pass (behind) and after the flashes (front), and in layer frames; light passes draw it as
covering black (nothing in the surface picture). Every draw layer has the **Experimental** fold (`modifiers.rs`
through `Kind.dynamic`: Mirror 2..12 ways round The middle of the picture / The thing they ride / A point, Mirrored
copies; Colour split (three added channel copies along the velocity; not on Liquid, Soft smoke, Flame or Web lines,
`defs_without_split`), Glitch (bands torn sideways, `Tears`, pure in time)); applied on the CPU to `Pieces`, blobs and
field splats (`draw::finish`). Part 5's Fine detail copies trails, marks and blobs too. Five new looks (Light
streams, Kaleidoscope, Colour split dust, Liquid metal, Paint drips), part 4's Liquid splashes drawn as a Liquid, and part 5's Pixel glitch torn by
Glitch. Tests: `particles/tests_part6.rs`,
`tests/liquid_pictures.rs` (GPU).

Part 3 is in (plan `part3.md`): **moving and forming**. Sources From **A word or picture** (Text with Letter height and
Weight, or a PNG Picture with Picture width and Uses Solid / Bright / Dark parts) and **A grid** (Covers The screen or
A rectangle, by Spacing or Columns and rows, Jitter, Stagger); their places are `spots.rs` (`SpotsKey`, `spots_for`, a
process-wide cache of `Arc<Vec<Vec2>>`; words by a scanline fill of `balls::shapes::letter_outlines_in`), each
particle's `Speck.spot` (1-based) names its **home**, which follows the anchor and turns by **Homes turn**
(`ParticlesState.home_turn`, `HomeFrame`); Keep a crowd / Once at the start fill every free place. Emit **On the beat**
(`beats.rs::Moments`, Every Beat / Bar / Bars, Offset) reads the tempo from `World.beat_seconds`/`beats_in_bar` (set by
the scene sim; `things::scene::sim::beat_of`); `ThingSim::key_restarts_beside` puts the tempo keys into `beside_fingerprint` so
a tempo edit restarts the particles (not the physics). Heading **Round the middle**. Nine force layers
(`more_forces.rs`): Pull to / Push away from (Toward Its place, A point, Its ball, Ball number, The middle of its parent,
Balls nearby; Falloff, Orbit, Swallow), Spring home, Flock (start-of-step `ready::Flockmates` grid, at most
`MOST_MATES`), Magnetic field (balls as dipoles; turns needles), Flow field (Noise / Waves / Spiral / Vortex),
Spirograph, Pulse on the beat, Ripple from hits (rings from the log, `MOST_RINGS`); what they read is gathered once a
step in `ready.rs::Ready` so any thread count gives the same. Every force's Only for gains A share / The rest (the same
dice as the Look's Only). Eleven looks (`looks_motion.rs`): Galaxy round a ball, Flow lines, Form a word, Flock,
Shockwaves, Comet tail, Black hole, Iron filings, Beat grid, Moiré (two grids, `looks_motion::also`), Spirograph.
Goldens `tests/golden-particles-motion*.txt` (`scenes::particle_motion_scenes`); the preview shows a rectangle grid's
corner handle.

Since the 2026-11-08 clean-up every look's recipe is a small function of its own (`looks.rs::rain()`,
`looks_motion.rs::galaxy()`, ...), and `recipe()` is one line per look; shared bits are `riding`, `top`, `vanish`,
`push` and `screen_grid`. The particles' dice are `bpe_core::dice` (`hash`, and `Stream` for the burst and shatter
pieces).

Part 7 is in (plan `part7.md`, "As built" at its end): **moments, sound, rules, lighting**. Meeting code marks a
`Speck` (`met`/`with`/`impact`) and the serial retain turns marks into `Moment`s (`moments.rs`: Hit a ball, Hit a shape,
Land (once, `LANDED`), Born, Burst; `ParticlesState.moments`/`moment_counts` for world step `moments_step`, at most
`MOST_MOMENTS`). A Particles thing takes a **Sound card** (`slots()` has `SlotId::Sound`, `VoiceKind::fits`): the
Moments group of `VoiceSettings` (`moments_*`, `MomentStrength`) picks which play; `log_notes` logs the strongest as
`EventKind::Moment` (tag 22, `a` = (thing, seed), `b` = (thing, kind code)) within Most a second; `bpe-sound` plays
them like hits (`hits::plays`, no Heard from for own moments). The sound engine's physics is `heard_part` (main plus
beside things whose `ThingSim::heard` is some) and `sound_fingerprint` mixes them in. Rules: `when.particles`
(`particles/rules.rs`, Who / Moment / At least); `BlockSim::keeps` pulls a Particles thing a rule reads into the main
world (`is_beside(scene, ..)`, `main_part(scene, ..)`), and `ThingSim::steps_own_cards` hashes its cards whole then. Lighting:
its drawn layers have `AroundLit`'s fold (`Kind::light_defs`), the `particles.sub.light` Gives off light sub-layer
(listed in the Lights panel), stamped by `Drew::stamp`. Hits pulses on its settings: `ThingSim::pulses_on_hits`,
`Animation::apply_where` with `bpe_runtime::hit_strength`. Editor: `inspector::sounds`, `sound_tab/moments.rs`.
Tests: `particles/tests_part7.rs`, `bpe-sound/tests/particle_sound.rs`, `bpe-kit/tests/particle_light_pictures.rs`.

Part 8 is in (plan `part8.md`, "As built" at its end): the **Huge swarm**. The Particles Look card's **Engine** (Exact /
Huge swarm, `look.rs::Engine`, `LookCardSettings`, `engine_of`; not keyframable) moves a thing's particles on the
graphics card, up to `MOST_SWARM` 4 000 000, decoration only. The world drives the card: `ParticlesState.swarm`
(`things/particles/swarm.rs`: `Swarm`, `first`, `step_in`) owns `bpe_gfx::swarm::SwarmBuffers` (two 32-byte halves a
particle, `SpeckMain`/`SpeckMore`; `Clone` is a copy on the card, so checkpoints and snapshots are still copies). Which
card: `bpe_gfx::swarm::use_on_this_thread`/`on_this_thread`/`use_for_now` (`SwarmGpu::for_device`, cached); the editor
names eframe's device in `Studio::new` and `ParticleJob` passes it to its thread, `bpe_cli::export_with`/`final_frame`
name theirs; no card named = a swarm with no particles. Births are planned on the CPU with the exact engine's dice
(`births::plan`, `For::Exact`/`Swarm`, `crowd_spares`, `MOST_SWARM_AT_ONCE`); a step is five compute passes in
`shaders/swarm_step.wgsl` (births, move: forces, balls through `BallGrid::parts`, shapes as segments on a grid, edges,
air; then a stable integer-prefix compaction, no atomics; `STEP_STILL` places the first births). Drawing reads the
buffers (`swarm/draw.rs`, `shaders/swarm_draw.wgsl` ending in sprites.wgsl's `sprite_out`): `Frame.swarms`/
`swarms_front` hold `SwarmRun { at, source, look, balls }` among the sprites (`cull` moves `at`), each an indirect draw
on the step's count; roles own, glitter star, Shadow, Glow, Outline. Not in a swarm: Soft smoke, Flame, Web lines, Fine
detail, Recolour/Flash/Colour on hit sub-layers (`draw::swarm_draws`, `swarm_draws_sub`), heating the air, Flock,
refilling a word/picture/grid crowd (`swarm_moves_by`), part 4's two-way, feeding, Stick/Settle/Pile up (a swarm's
bounce off / stop) and Piles layer, and part 7's moments, sound and particle lighting (swarm sprites are unlit). Part 3's other forces
are on the card (`swarm::put_ready` from `Ready::new`; `StepGpu.middle/parent/homes/lists/fballs`, `SpeckMore.spot`). `World.gpu_memory` + `Playback`'s `GPU_CHECKPOINT_BYTES` (2 GB) thin checkpoints by video
memory. The editor's Look card has the Engine chips (`look_editor/particles.rs::engine_row`). Tests:
`bpe-gfx/src/swarm/tests.rs`, `bpe-kit/src/things/particles/swarm_tests.rs` (ignored `bench_a_million`).

**W13**: while some ball follows the air (`bpe_runtime::air_felt`), a Particles thing whose Rise heats the air
(`ParticlesSim::heats_air`: Exact engine, a Rise with Heats the air not 0) steps with the physics, not beside, so
every playback moves the balls alike (`air_tests.rs::heating_particles_step_with_the_balls_that_follow_the_air`).
Following the air alone never moves a ball, so such a thing stays beside. The Air card's edits then restart the
physics too. A thing fed by its parent's particles puts no heat in while balls follow the air (its parent may step
beside; `catch_up_all` skips its `put_heat` when `World.air_felt`). Known gap: a Rise whose Heats the air is 0 but
keyed above 0 isn't counted.
