# Particles thing: design

Date: 2026-11-06. Brainstorm thread "Design a Particles thing". Mockups: "Particle Looks"
(https://claude.ai/artifact/3RcRuGJuCuzWwqa9DAQocW, looks 1 to 16; file `ballphysics/particles-thing/particle-looks-mockup.html`
in the project files) and "More Particle Looks" (https://claude.ai/artifact/SfMX7pbMtQ4sHdNm8Xnz5U, looks 17 to 40; file
`ballphysics/particles-thing/more-particle-looks-mockup.html`). The mockups are rough canvas sketches; their code is a
reference for behaviour only. Every decision below was made or approved by the owner in that thread.

## 1. What this is

A new thing kind, **Particles** (`particles`), that makes its own particles: rain over the whole picture, fire riding a
ball, dust thrown up where balls land, a flock, a word made of specks, liquid metal, a kaleidoscope. One general system
makes all 40 looks from the mockups; the looks are starting points in the Add thing menu, never a separate code path.

Today particles exist only as a Look layer on balls and shapes (`layer.particles`, `shape.particles`,
`bpe-kit/src/modules/look/layers/particles/`): stateless (worked out again every frame from the log), at most 4000 a
layer, decoration only. **That layer stays exactly as it is.** The Particles thing is new and separate.

### What the owner said

- How particles relate to the world (asked as A decoration / B feel the world / C two-way): **"B with C as an
  option"**. Balls push particles (wakes, splashes, parting smoke, bending flames); particles never push balls. A
  **two-way switch**, off by default, makes particles change the physics (weight, piles balls land on, drag in thick
  fog, a waterfall pushing a ball down, sticky dust making a ball heavier).
- Looks: **all 16** of the first page and **all 24** of the second (section 2).
- Engine: **"Both, exact engine first"**: an exact CPU engine stepped with the physics for all 40 looks, then a later
  graphics-card **Huge swarm** mode for millions of decorative particles that the air still pushes.
- Design section 1 (the building blocks, section 3 here), section 2 (how it is worked out and how it connects,
  sections 4 to 8) and section 3 (the eight build parts, section 11) were each approved ("Looks good", "Looks good",
  "Yes").
- Standing wishes that apply: as much customisation as possible and few limits on how far settings go; psychedelic and
  experimental looks beside high-quality ones; anything that changes the physics is an optional switch, off by default;
  no built-in or example scenes; no backward compatibility work.

### Assumed (stated so they can be corrected at review)

- Particles are 2D, in world pixels, like everything else; in a Stage-lit scene they are flat sprites on the picture
  plane (as the existing particle layer is).
- Particles step at **120 Hz** (every second world step), not 240: that is enough for anything a particle does and
  halves the cost. Drawing between steps lerps each particle from its previous place.
- "Whole screen" sources follow the camera: they fill what the camera sees (plus a margin), so rain keeps falling in
  shot when the camera moves.
- The default most-particles cap is 20 000 a thing, adjustable up to 1 000 000 (hard cap, section 4.6).

## 2. The 40 looks and what each needs

Every look is a starting point: picking it fills in settings, cards and layers (and adds a fed child thing where noted,
section 3.5). Everything stays editable afterwards. "P" is the build part (section 11) that makes the look possible; a
look appears in the Add thing menu once its part is in.

| # | Look | From | Needs | P |
| --- | --- | --- | --- | --- |
| 1 | Rain | Whole screen, top | gravity, wind, balls push, streaks; child Splashes (on landing / hitting a ball) | 1 (splashes 4) |
| 2 | Snow | Whole screen | gravity, wander, sway, balls push | 1 |
| 3 | Fire on a ball | Riding each ball | rise, swirl, flame look, embers child | 1 (flame look 5) |
| 4 | Fire and smoke | A point | rise, swirl; child Smoke (fed when flames die) | 2 (air), 4 (feeding), 5 (looks) |
| 5 | Dust clouds | Ball hits | drag, air, soft smoke look | 2, 5 |
| 6 | Spark fountain | A point | gravity, bounce off shapes and balls, streaks | 1 |
| 7 | Fireflies | An area, fixed crowd | wander, flee balls, glow pulse | 1 (flee 3) |
| 8 | Bubbles | Bottom edge | rise, sway, pop when touched | 4 |
| 9 | Embers and ash | Whole screen | rise, swirl, two layers | 1 |
| 10 | Galaxy round a ball | Riding a ball | pull to / orbit the ball | 3 |
| 11 | Confetti | Top edge | gravity, flutter, flip, balls push | 1 |
| 12 | Flow lines | Whole screen | follow a flow field, trails | 3 (field), 6 (trails) |
| 13 | Form a word | A word or picture | spring home, balls blast them | 3 |
| 14 | Sand that piles up | A point (pouring) | pile up; two-way: balls land on piles | 4 |
| 15 | Thick fog | An area, fixed crowd | spring home, air; two-way: drag on balls | 2, 3, 4 |
| 16 | Rainbow smoke trail | Riding a ball | swirl, colour over time, smoke look | 1 (smoke look 5) |
| 17 | Falling leaves | Top edge | flutter, settle, kicked up again | 4, 5 (leaf shape) |
| 18 | Hail | Whole screen | gravity, bounce, melt (life) | 1 |
| 19 | Waterfall and mist | Top edge, a span | gravity, split round balls; mist child; two-way pushes balls down | 4 |
| 20 | Blossom petals | An area (gust) | swirl, flutter, petal shape | 1 (petal shape 5) |
| 21 | Sandstorm | Left edge | strong wind with gust waves, haze layer | 1 (gust waves 2) |
| 22 | Liquid splashes | Ball hits | gravity, pool, liquid look | 4, 6 |
| 23 | Flock | An area, fixed crowd | flocking, avoid balls, bird/fish shape | 3, 5 |
| 24 | Fireworks | Ball hits and bottom edge | rockets; child Bursts fed when rockets stop rising | 4 |
| 25 | Shockwaves | Ball hits (rings) and a fixed crowd | ring wave pushes specks, spring home | 3 |
| 26 | Comet tail | Riding a ball | push away from a far point (two strengths, two layers) | 3 |
| 27 | Black hole | A point (orbiting crowd) | pull to a point, orbit, swallow, streaks | 3 |
| 28 | Iron filings | A grid | line up with the balls' magnetic field | 3 |
| 29 | Sticky dust | An area, fixed crowd | stick to balls, shaken off on landing; two-way: weight | 4 |
| 30 | Glitter | Whole screen | spin, facets that flash | 5 |
| 31 | Kaleidoscope | Riding a ball | mirror copies round the middle | 6 |
| 32 | Colour split dust | Whole screen | swirl, colour split by speed | 6 |
| 33 | Beat grid | A grid | spring home, pulse on the beat, ripples from hits | 3 |
| 34 | Web of light | An area, fixed crowd | wander, lines between near neighbours (and to balls) | 5 |
| 35 | Liquid metal | Top edge and ball hits | gravity, roll, liquid look with Chrome | 6 |
| 36 | Ink in water | Riding a ball | swirl, growing soft blots, multiply blend | 1 (soft look 5) |
| 37 | Paint drips | Riding a ball and hits | gravity, marks that stay where they land | 6 |
| 38 | Moiré | Two grids | spring home, balls push one grid | 3 |
| 39 | Pixel glitch | Riding a ball | pixel shape snapped to a grid, glitch bands | 5 (pixels), 6 (glitch) |
| 40 | Spirograph | Riding a ball | spirograph paths round the parent, trails | 3, 6 |

## 3. The building blocks

### 3.1 The thing

`bpe-kit/src/things/particles/` (`ParticlesKind`, kind id `particles`, family **"Particles"**, icon `particles`). It
has its own settings (the **Source**, section 3.2) and three cards:

| Card | Slot | Card kind | Holds |
| --- | --- | --- | --- |
| Motion | `SlotId::Motion` | `motion.particles` | force layers (section 3.3) |
| Meets things | `SlotId::Bounce` (`slot_label` "Meets things") | `meets.particles` | what happens when particles meet balls, shapes and the picture's edges, and the two-way switch (section 3.4) |
| Look | `SlotId::Look` | `look.particles` | draw layers and their sub-layers (section 3.6) |

`ModuleKind::fits` keeps these card kinds on the Particles thing only, and keeps the ball/shape cards off it. Adding
one through `add_thing` gives all three cards (`starters.rs`). The Placement card is shown (Position is the source's
place for Point/Area/Line/Word sources; Z-order sorts its drawing like any thing).

### 3.2 Source (the thing's own settings)

Folds **Where**, **When**, **Each particle**, **Limits**.

**Where** (`From`):

| From | Settings |
| --- | --- |
| Whole screen (default) | Edge: Everywhere / Top / Bottom / Left / Right; Margin. Follows the camera's view at the step. |
| An area | Shape: Rectangle / Circle / Ring; Width, Height (or Radius, Inner radius); around Position |
| A point | Position (Placement) |
| A line | Length, Turn, around Position |
| Riding its parent | only when inside a Balls, Emitter or Shape (or another Particles thing, section 3.5): Each ball / The middle / Ball number / The edge (a shape's outline, or each ball's rim) / Inside (a shape's fill). The same places lights ride (`light::draw::places` is the model; the sim reads `World.bodies` and walls) |
| A word or picture | Text (IBM Plex, Size, Weight) or an image file; Spacing: one particle per N px of the filled area; each particle's **home** is its place in it |
| A grid | Columns × Rows or Spacing, Jitter, around Position and Turn; each particle's home is its cell |
| Its parent's particles | only inside another Particles thing (section 3.5) |

**When** (`Emit`): Steady (Rate per second, per ball when riding Each ball) / Bursts (Every, Count) / On hits (Who
hits: a `Target` as the camera and emitter use, Count, Least speed, Follow impact) / On the beat (Every beat / bar / N
bars, Offset, Count; the scene's `TempoMap`) / Keep a crowd (Count: a fixed population, born at 0 s and replaced when
one dies) / Once at the start (Count). Rate and Count keyframe, pulse and follow sound.

**Each particle**: Life (s) and Life spread; Speed, Speed spread, Direction (Off the bounce / All round / Along the
path / Behind / A way: angle), Spread (°); Takes the parent's speed (0..1); Size, Size spread; Turn, Spin, Spin
spread; Seed. Hard ranges only where maths breaks (life > 0, size ≥ 0); everything else is a span with no hard limit
(`span(a, b)`), as in the lighting work.

**Limits**: Most particles (default 20 000; span 1..200 000; hard 1 000 000). When the cap is reached new births are
dropped (oldest are never killed early, so nothing pops).

### 3.3 Motion: force layers (`motion.particles`)

Layers in a new section `LayerSection::Force` (kept in list order like the Effects stack's layers), applied top to
bottom each step, each with **Amount** (0..1, unclamped span) and an **Only for** filter (all / younger than / older than, for layered
behaviour like a rocket that stops pushing). Kinds (`force.*`):

| Force | Settings | P |
| --- | --- | --- |
| Gravity | Strength (px/s²), Direction (default down; uses the scene's gravity when "Scene's") | 1 |
| Wind | Speed, Direction, Gusts (strength, how often, Waves: gusts travel across the picture) | 1 (waves 2) |
| Drag | Amount per second | 1 |
| Swirl | Strength, Size, Speed (curl noise, as `sim::curl` today) | 1 |
| Wander | Strength, How often it turns | 1 |
| Rise (heat) | Strength, Cools over (s) | 1 (heat into the air 2) |
| Sway / Flutter | Sway width, Speed; Flutter (flip rate, drift as it flips) | 1 |
| Follow the air | Amount (0 none, 1 moves with the air) | 2 |
| Pull to | Target: a point / its parent / a ball / balls nearby; Strength, Falloff (none, 1/d, 1/d²), Orbit (sideways share), Swallow radius (die there) | 3 |
| Push away from | the same, outwards (comet tails, fleeing balls) | 3 |
| Spring home | Stiffness, Damping (needs a home: Word/Grid source, or "where it was born") | 3 |
| Flock | Separation, Alignment, Cohesion, Reach, Speed range | 3 |
| Magnetic field | Field from: each ball (a dipole along its motion or up) / a point; Line up (turn particles to the field), Pull | 3 |
| Flow field | Pattern: Noise / Spiral / Waves / Vortex; Scale, Speed; particles move along it | 3 |
| Spirograph | Ring radius, Wheel radius, Pen distance, Speed, around its parent | 3 |
| Pulse on the beat | Kick outwards or upwards on each beat (strength, decay) | 3 |
| Ripple from hits | each hit of a Target sends a ring that pushes particles as it passes (speed, strength, how long) | 3 |

### 3.4 Meets things (`meets.particles`)

**Balls**: Push (how much a passing ball drags particles along, Reach in ball radii; part 1 works it per particle from
nearby balls; with the air (part 2) balls also push the air) and then **On touch**: Pass / Bounce off (Bounciness) /
Vanish / Pop (makes a pop moment) / Stick (stay on the ball's surface, Fall off when it lands: share) — Pop and Stick
part 4.

**Shapes**: Pass / Bounce (Bounciness, Friction) / Stop / Vanish / Settle (stay where they land; Fade after; most kept)
/ Pile up (section 4.5). Bounce and Vanish part 1; Settle, Pile up part 4. Uses the world's walls (`World.walls`,
`keep_clear`-style side test against each wall's outline, a broad-phase grid over the walls' bounds).

**Picture edges** (the camera's view): Pass / Wrap round / Bounce / Vanish.

**Splash into** (part 4): pick a child Particles thing fed by particles that land, hit a ball or pop (section 3.5);
that is how rain splashes and waterfalls spray.

**Two-way** (part 4; switch, off by default; section 4.4): Weight per particle, and what it does: Push balls, Slow
balls inside (drag by density), Hold balls up (piles), Add weight when stuck.

### 3.5 Particles make particles

A Particles thing **inside** another Particles thing is fed by it: its Where is "Its parent's particles" with **When
they**: Die / Land (stop on a shape or settle) / Hit a ball / Hit a shape / Pop / Stop rising (vertical speed crosses
0: rockets) / Every step (a trail of particles, rate per particle). Count per moment, and Takes their speed / colour
(colour from the parent particle's colour at that moment). The parent's moments of a step are kept on the parent's
state (`moments`, cleared each step) and read by the child in the same step, so the parent always steps first (things
step in tree order). Chains can be any depth; a thing can't be fed by itself (the tree forbids it).

### 3.6 Look: draw layers (`look.particles`)

Layers like a ball's Look: a list of drawn layers, each with sub-layers, mixed in order. Each layer draws every
particle of the thing (or only some: **Only** by age, by a share (every Nth), or by kind when a look has two, e.g.
embers and ash use one thing with two layers split by **Share**).

Layer kinds (`particles.*`):

| Layer | What | P |
| --- | --- | --- |
| Sprites | the existing sprite shapes (Sparks, Smoke, Confetti, Stars, Shards, Rings, Dots, Hearts, Bubbles, Streaks) through `sprites.wgsl` | 1 |
| Streaks | stretched along motion by speed (rain, sparks, hail lines) | 1 |
| More shapes | Leaves, Petals, Flakes, Needles (point along their turn or a field), Birds/Fish, Pixels (snapped to a grid) | 5 |
| Soft smoke | soft volumes splatted at half size and shaded (density, noise detail, light from the scene) | 5 |
| Flame | density and heat splatted, shaded through a heat gradient with flicker and noise | 5 |
| Glitter | facets that flash when turned to the light (or a fixed light direction without a Lighting card) | 5 |
| Web lines | lines between particles within Reach (and to balls), fading with distance | 5 |
| Liquid | metaball surface: particles splatted into a field, cut at a level, shaded with normals; Surface (any lighting surface: Chrome makes liquid metal, Glass bends what is behind) | 6 |
| Trails | each particle's recent path as a line (Length in steps, up to 64), fading | 6 |
| Marks that stay | particles that land or settle stamp a mark into a picture kept with the thing (paint drips) | 6 |

Every layer has **Colour** (Own / gradient over life / over speed / over place / the parent ball's colour / picked
from the parent particle), Opacity, Size over life (curve), Fade (In / Out / Both / None / curve), Twinkle, Blend and
Blend amount, and the experimental modifiers: **Mirror** (Off / 2 / 4 / 6 / 8 / 12 ways round the middle of the
picture or round the parent; part 6), **Colour split** (red/green/blue pulled apart by speed; part 6), **Glitch**
(bands torn sideways, chance and size; part 6), **Fine detail** (draw N extra small copies round each particle,
jittered by a hash, no sim cost; part 5). Sub-layers: the around-layer ones (`AROUND_SUBS`: Glow, Outline, Shadow,
Recolour, Flash on hit, Colour on hit) and Gives off light (part 7). Lit, Blocks light and Catches beams sit in the
layer's Lighting fold (part 7).

## 4. How particles are worked out

### 4.1 Where the state lives

A Particles thing is a `ThingSim` (`things/particles/sim.rs`, `ParticlesSim`) whose `ThingState` (`ParticlesState`)
lives in `World.states` like an emitter's, so playback checkpoints and clones carry it and every replay is identical.
The state is a struct of arrays: `place`, `previous`, `velocity`, `born` (step), `life`, `size`, `turn`, `spin`,
`seed`, `home`, `flags` (stuck, settled, popped…), `stuck_to`, and per-thing extras (`moments` for children, `piles`,
`marks`, trail rings). Removal is a stable compaction (order kept), so particle *i*'s seed never depends on others.

### 4.2 The step

The world runs at 240 Hz; particle things step every second world step (`World.step % 2 == 1`) in the Events stage
(`after_step`), in tree order, after the balls have moved and the log has this step's hits. Each particle step:

1. **Births** from the Source (rate accumulators in the state; counts hashed from `World.seed`, thing id, step and
   birth index; never a shared random stream, so adding one thing never changes another's dice).
2. **Forces** in layer order, then integrate (semi-implicit Euler, dt = 1/120 s).
3. **Meets**: balls (push, touch outcomes), shapes, edges.
4. **Ages and deaths**, moments for children, compaction.

The work is split into fixed chunks of 4096 particles run on `std::thread::scope` threads (as the emitter's
`test_fire` does); each chunk reads only shared, read-only inputs (the balls as they stand, the walls, the air grid,
neighbour grids built before the chunks run from the previous positions) and writes only its own particles. Anything
summed across particles (two-way pushes, densities) is summed per chunk and then across chunks in chunk order. So the
result is the same with 1 thread or 32, every run (a test checks this).

### 4.3 Editing stays quick: two playbacks

- **Two-way off** (the normal case): particle things don't touch the physics. `ParticlesSim::affects_physics` is false
  for every particle setting, the physics fingerprint and the sound fingerprint leave them out, and the editor's main
  playback (balls, sound, aiming, light probe) builds its world **without** them (`World::build` skips things whose
  sim says `in_main(resolved) == false`). A second **particle playback** (`bpe-studio/src/particle_job.rs`, its own
  thread, like `aim_job.rs`) builds the whole world including the particle things and steps it to the playhead; the
  preview draws balls from the main playback and particles from the particle playback (both deterministic, so they
  agree step for step). Changing a particle setting rebuilds only the particle playback; the balls and the sound never
  restart.
- **Two-way on** for any Particles thing: that thing is part of the main world, its settings affect the physics, and
  edits rebuild the main playback as any physics edit does.
- **Export** (`bpe-cli`) and tests use one playback with everything.
- Copies of the world: `World::lone` (emitter aiming) drops two-way-off particle things (aiming is unaffected by
  decoration) and keeps two-way ones. The sound engine's playback keeps particle things that have a sound (section 7)
  or are two-way, and drops the rest.
- The particle playback has its own checkpoint budget (512 MB, then thinned as `Playback` does today).

### 4.4 Two-way (part 4)

With the switch on, each particle has a Weight. After the particle step, the summed effect on each ball is applied as
a velocity change at the next substep's Forces stage (`ThingSim::forces`): pushes (momentum from particles that hit
it), drag (by particle density in the cells it covers), extra pull of gravity and lower bounce while particles are
stuck to it (Sticky dust), and support from piles (section 4.5). Every amount is clamped and finite-checked, and speeds
still clamp to the world's ceiling.

### 4.5 Piles (part 4)

A thing that piles keeps a height field per surface it piles on: a column every 2 px along each shape edge facing up
(and along the picture's bottom edge when that is a wall). A landing particle adds to its column and slides to a lower
neighbour while the slope is steeper than the **Angle of repose**. Piles draw as a filled band in the particle colour
(a Look layer option, "Pile" drawn by the Sprites layer as a strip). Two-way on: balls rest on and bounce off the pile
surface (a settle step pushes a ball out of the pile and reflects its velocity with the pile's Bounciness) and a hard
landing digs the pile and throws the sand back up as particles.

### 4.6 Limits

- Most particles per thing: setting, hard cap 1 000 000; all Particles things together are not capped, but the status
  line warns past 2 000 000.
- Children: most births per parent moment 1000.
- Neighbour forces (Flock, Web lines) look at most 32 neighbours each.
- Every number written is checked finite; a NaN particle is removed.

## 5. The air (part 2)

The scene gets an **Air** card (scene settings, new `SlotId::Air`, card kind `air`, added the first time a Particles
thing with Follow the air or a Rise force is added, like the Lighting card): Wind (speed, direction, gusts), Calms down
(how fast the air settles), Resolution (Coarse / Normal / Fine: 32 / 16 / 8 px cells), Heat rises (strength), Shapes
block the air (on).

It is a 2D grid fluid over the camera's view plus a margin (`bpe-runtime/src/air.rs`, `Air` on the world, stepped at
120 Hz before the particle things): balls write their velocity into the cells they cover (stirring), heat sources
(Rise forces with "Heats the air") add heat that rises, shapes are solid cells; semi-Lagrangian advection, a fixed
number of Jacobi pressure iterations (20), velocity decay by Calms down. Deterministic and chunked by rows like the
particles. Built only when something reads it. The editor's **Show air** toggle (preview toolbar) draws faint arrows
from the air at the playhead.

## 6. Lighting (parts 1 and 7)

Sprites carry the existing `Particle.light/emit/extra` fields. Part 1 writes 0s (no light). Part 7 adds the Lighting
fold to every particle layer (Lit, Blocks light: writes to the surface pass so thick smoke and sand cast 2D shadows),
the **Gives off light** sub-layer (fire lights the room; through the light field, so thousands of tiny emitters cost
the same as one), Catches beams and UV glow, and Stage behaviour (flat on the picture plane, as around layers are).

## 7. Sound and rules (part 7)

- **Moments**: Land, Hit a ball, Hit a shape, Pop, Splash, Burst, Born (bursts only). A thing keeps this step's
  moments (place, speed, size, which ball/shape). They are not log events (rain would flood the log).
- **Sound**: a Particles thing can have a Sound tab like a shape's (`sound.voice` card, same oscillators, chain,
  modulators and notes), played by the moments it picks: Which moments, Most notes a second (thinning picks the
  strongest), Strength from speed and size, Pan from place. The sound engine's playback then keeps that thing (4.3).
  Rain can patter, bubbles can blip.
- **Rules**: a When block **Particles** (Who: a Particles thing; Moment; At least N in a step) firing with values
  count, place, speed. Needs the rules engine (rules part 1) to be in; this block lands with part 7.
- **Follow sound** and pulses work on particle settings like any setting; On the beat uses the scene's tempo.

## 8. Editor (`bpe-studio`)

- **Add thing ▸ Particles**: Empty, then the starting looks grouped as in the mockups (Weather and nature, Fire and
  energy, Space and sparkle, Experimental and psychedelic), each with a one-line hover help. Only looks whose part is
  in are listed. `bpe_kit::things::particles::looks::add_particles(scene, reg, look, parent)` writes everything,
  including fed child things, in one edit.
- The inspector shows the Source settings in the thing's own grid, then the three cards. Motion and Look show their
  layer lists with the same list widgets as the Look editor and the Effects card (add, drag to reorder, rename, on/off,
  duplicate, copy/paste, delete).
- **Handles** in the preview for the selected Particles thing: area corners/edges and radius, point, line ends and turn,
  grid corner, a direction arrow for Direction and Spread, wind arrows on Wind forces and the Air card. Handles drag
  through `set_or_key` like light handles.
- **Status line**: "Particles: 48 210, 2.4 ms" (count at the playhead and the last step's time), and the warning past
  2 000 000.
- Riding: "Riding its parent" is offered only where it makes sense; a Particles thing dropped into a ball group gets
  Riding Each ball by default.
- Timeline, keyframes, pulses, Follow sound, Copy/Paste value: as for any setting.

## 9. Huge swarm (part 8)

A Look-card switch **Engine: Exact / Huge swarm** on a Particles thing. Huge swarm runs on the graphics card:
decoration only (no two-way, sound, rules, settle, pile, stick or feeding children), up to 4 000 000 particles a
thing. Forces: Gravity, Wind, Drag, Swirl, Wander, Rise, Follow the air, Pull to, Push away, Spring home, Flow field.
Meets: balls push, shapes bounce/vanish, edges. Each step, the balls, walls and air grid go up to the GPU; a compute
pass steps the particles at 120 Hz; checkpoints of the particle buffers every 0.5 s stay in video memory (budget 2 GB)
for scrubbing. The same PC always gives the same result (no atomics in motion, fixed order); a different graphics card
may differ slightly, which is accepted for decoration. Export steps the same way.

## 10. Testing

- **Particle goldens** (`bpe-kit/tests/golden-particles*.txt`, debug and release): for test scenes
  (`scenes::particle_scenes`, test-only) an FNV hash of every particle's place, velocity and life once a second,
  blessed like the physics goldens. Ball goldens must not move in any part (two-way-off particles never touch them).
- **Determinism**: seek back and forth equals a straight run; 1 thread equals many threads; the particle playback and
  the main playback agree on every ball at every step.
- **Pictures** (`bpe-kit/tests/particle_pictures.rs`): one per layer kind, plus Mirror, Colour split and Glitch.
- **Editor** (kittest): adding each listed look, the cards' layer lists, handles, the status line.
- **Bench** (ignored test): 100 000 rain particles with Part 1 forces; the build ledger records the step time. Target:
  under 4 ms a particle step in release on an 8-core machine.
- Exes are cross-built and checked to start under Xvfb, as for every part.

## 11. Parts and order

Each part gets its own thread, plan (`docs/superpowers/plans/particles/partN.md`), one Opus review at the end, and a
PR. Each part adds the looks it makes possible to the Add thing menu (section 2) with their own kittest check.

1. **Part 1, engine** (first, alone): the thing kind and cards, Source (every From except Word/picture, Grid and Its
   parent's particles; every When except On the beat), the state, the 120 Hz chunked step, checkpoints, the particle
   playback in the editor (4.3), forces Gravity / Wind / Drag / Swirl / Wander / Rise / Sway and Flutter, Meets balls
   (push, pass, bounce, vanish) / shapes (pass, bounce, stop, vanish) / edges, riding, Look layers Sprites and Streaks
   with colour over life/speed and the common layer settings, the around sub-layers, handles, status line, goldens,
   determinism tests, bench. Looks 1 (without splashes), 2, 6, 9, 11, 18, 20, 21 (without gust waves) and the simple
   forms of 3, 7, 16, 36.
2. **Part 2, air**: the Air card and grid, balls stirring it, heat, shapes blocking, Follow the air, gust waves, Show
   air. Looks 5, 21 (full), 15 (with part 3/4 pieces as they land).
3. **Part 3, more motion**: Word/picture and Grid sources, On the beat, Pull to, Push away, Spring home, Flock,
   Magnetic field, Flow field, Spirograph, Pulse on the beat, Ripple from hits. Looks 10, 12 (motion), 13, 23 (motion),
   25, 26, 27, 28, 33, 38, 40 (motion).
4. **Part 4, meeting things**: feeding children (3.5), Pop, Stick, Settle, Pile up, Splash into, and the two-way
   switch (4.4, 4.5). Looks 1 (splashes), 4 (feeding), 8, 14, 15, 17, 19, 22 (motion), 24, 29.
5. **Part 5, looks 1**: Soft smoke, Flame, More shapes, Glitter, Web lines, Fine detail. Completes 3, 4, 5, 16, 17,
   20, 23, 30, 34, 36, 39 (pixels).
6. **Part 6, looks 2**: Liquid, Trails, Marks that stay, Mirror, Colour split, Glitch. Completes 12, 22, 31, 32, 35,
   37, 39, 40.
7. **Part 7, lighting, sound and rules**: section 6 beyond part 1, section 7. Rules' Particles block waits for rules
   part 1 to be merged.
8. **Part 8, Huge swarm** (last): section 9.

Part 1 merges first. Parts 2 to 7 then run in parallel: each adds its kinds through the registry (`force.*`,
`particles.*`, new Source and Meets options behind their own files in `things/particles/`) and touches shared files
(`world.rs`, `inspector.rs`, `app.rs`, `kit/lib.rs`, the Source and Meets settings structs, the looks list) only
additively. Merge order after part 1: any order, each rebasing on main before its PR; the looks list
(`things/particles/looks.rs`) is a table each part appends to. Part 8 starts after parts 1 and 2 (it reads the air).

Scene format: part 1 bumps to the next free format number (no upgrade: older files simply have no Particles things);
part 2's `SlotId::Air` needs no further bump unless another feature took the number in between.

## 12. Not doing

- No change to the existing `layer.particles` / `shape.particles` layers.
- No built-in or example scenes; the starting looks only fill settings when the owner adds one.
- No 3D particles (depth, perspective): Stage scenes draw them flat on the picture plane.
- No hardware ray tracing or GPU simulation outside part 8; the exact engine runs on the CPU.
- No backward compatibility work.
- Not for now: particles colliding with each other (beyond Flock, Liquid and piles), fluids beyond the air grid, and
  keyframing individual particles.
