# Liquid thing: design

Date: 2026-11-17. Asked for by Aedan in thread "Choose the next big step". Brainstorm page with live sketches of every
liquid and mixing style: https://claude.ai/artifact/15dHFAPZoFER8UtR6TgQHo (copy in the project files,
`ballphysics/liquid-thing/liquid-lab.html`). The tanks there are a small processor sketch (double density relaxation),
not the method this design builds.

## 1. Why

Today's "liquid" is a Look layer on a Particles thing (`particles.liquid`, `bpe-gfx/src/liquid.rs`): ordinary
particles drawn as blobs that run together. There is nothing underneath: no pressure, no waves, no weight. Aedan:
"The liquids in the current shader system look very cheap and don't interact with the balls very interestingly. A
new sophisticated liquid thing built from the ground up would be a great addition."

What he asked for, in his words or confirmed by him:

| Question | Answer |
| --- | --- |
| What is it? | A new **Liquid** thing with its own physics: "water fills a container, sloshes, splashes, carries balls and gets pushed by them". |
| Look | "As much visual quality as possible and work with the lighting system, to where it looks like real liquid." |
| Settings | "A large amount of customization for both the physics and visuals." Every setting on the Liquid Lab page was approved ("All of this looks good"). |
| Scale | **C: both** big bodies (a third of the screen or more, floods, waves) and small detailed amounts (pours, streams, a cup), with a Detail setting. |
| Favourite part | Colour mixing: "the colors of the liquid (or multiple liquids) blend and mix with each other". The mixing section was approved too. |
| Mixing comes when? | In the first part, not as a later add-on (proposed, approved). |
| How it's built | **A**: a graphics-card liquid that reports its push on each ball back every step. |
| Export of a ball-pushing liquid | **Match the preview, with a finer-export switch** (section 5, Detail). |

## 2. Goals, non-goals, anti-goals

**Goals**

- A liquid that behaves like one: fills any shape and settles level, sloshes when its container moves, splashes,
  sprays and breaks into droplets, runs thick or thin, stretches, layers by weight.
- Real two-way contact with balls: balls float, bob, sink, get carried by currents and slowed by thickness; balls push
  the liquid aside, make splashes, wakes and waves.
- Colour that lives in the liquid: every bit carries its colour wherever it flows, several liquids share one body of
  liquid and mix (Paint, Light, Blend, Marble) or keep apart, with streaks that stay sharp.
- A look that reads as real liquid under the Lighting card: refraction, reflections, depth colour, shine, foam,
  caustics, glow, thin film.
- Scales from a few thousand drops to hundreds of thousands, set by Detail (separately for preview and export).
- Everything the rest of Ball Studio has: own settings, keyframes, pulses, Follow sound/light/health, Rules, sound,
  goldens, the restart test.

**Non-goals**

- Replacing or changing the Particles thing or its Liquid look layer. They stay as they are (they're cheap and fine for
  decoration). No migration from one to the other.
- 3D liquid. The engine is 2D; on a Stage camera the liquid draws as a flat layer, like today's Liquid layer does.
- Making old scenes look or play the same: there are no old Liquid things.

**Anti-goals** (would make this a failure even if it works)

- Blobby, lumpy or grainy surfaces: the particle grain must never show at export size.
- A liquid the balls ignore, or that ignores the balls.
- Preview and export disagreeing, or two plays of the same scene disagreeing (on one machine).
- Colours that turn to mud instantly: mixing must be slow and controllable, and Marble must keep streaks.
- An editor that freezes: heavy liquid must lower its preview Detail or fall behind smoothly, never lock the window.

## 3. Constraints found in the code

- Ball physics runs on the processor in a fixed 240 Hz staged step loop (`bpe-runtime`), and goldens pin it.
- The only crowd engine on the graphics card, the Huge swarm (`bpe-gfx/src/swarm/`, `things/particles/swarm.rs`),
  **cannot push balls** (two-way is processor-only, `things/particles/two_way.rs`). The liquid is the first thing on
  the graphics card that changes the main world.
- Several playbacks step the main world on their own: the editor's (`app/runs.rs`), the particle job
  (`particle_job.rs`, it names the card with `bpe_gfx::swarm::use_on_this_thread`), the **sound engine**
  (`bpe-sound/src/engine.rs`, `Playback::unseen`, no card today), the aim job and the exporter (`bpe-cli`, which
  names the card with `use_for_now`). They must all see the same liquid.
- Two-way particles already reach the balls one world step late (`TwoWayOut`: worked out in a step, given at the
  first substep of the next). That one-step lag is the accepted pattern.
- Checkpoints copy world state; the swarm keeps its particles on the card and copies them on the card, thinned by
  `GPU_CHECKPOINT_BYTES` (`playback.rs`). The liquid follows that.
- A frame with no Lighting card must stay byte-identical; lit liquids write the light passes the way
  `liquid.wgsl::real_lit` does (W15, `docs/notes/lighting.md`).
- The direct light pass is at its 16-texture limit (lighting notes); any new texture there must replace or pack one.
- The editor runs on egui 0.37 with the Ball Studio Design System (`bpe-studio/src/style/`), and every control must be
  named for kittest.

## 4. Approaches considered

**A. Graphics-card liquid that reports back every step (picked by Aedan).** One method on the card covers water,
honey, slime that stretches and tears, layered and mixing liquids: a hybrid particle-and-grid method (MLS-MPM is the
reference: particles carry mass, speed, colour and material; a grid in between does pressure and contact). After each
liquid step it sends back one small buffer: the push on each ball and how much of each ball is under. Scales with
Detail. Cost: the biggest build, and every main-world playback needs the card.

**B. Liquid on the processor.** Talking to balls is free and simple, but it tops out near 10,000 to 20,000 drops,
which rules out the big bodies Aedan wants (C). Dropped.

**C. Processor liquid near the balls, graphics card elsewhere.** Seams where the two meet and two engines to keep in
step. Dropped.

## 5. What the person sees

### The thing

**+ Add thing ▸ Liquid** adds a Liquid thing (tree icon: a drop). Like every thing it **owns its settings**. Its cards
(each a `ModuleKind` in the kit, settings from `#[derive(Settings)]`):

| Card | Slot | Holds |
| --- | --- | --- |
| Liquid | Motion | what it's like (section 6.1), heat and change (6.2) |
| Fill | (new slot, or Placement) | where it comes from and goes (6.4) |
| Meets | Bounce | balls (6.3), shapes, other liquids (6.5) |
| Look | Look | surface, colour and mixing look (section 7); layers like other Looks |
| Sound | Sound | splash voices (section 9) |

The builder picks the exact slot names; the person must see these five groups as cards in this order.

**Several Liquid things share one body of liquid.** All Liquid things in a scene step in one shared simulation, so two
liquids poured together actually meet: they mix or layer by their own settings. Each thing keeps its own settings and
its own drops; the sharing is invisible except that they touch.

**A Liquid thing in a Shape** (tree child of a Shape) fills that shape by default, the way a Particles thing rides its
parent (W14's one way to ride a parent, `engine.md`). On its own it fills the scene's edges.

**The liquid's area** (designer's call): the simulation covers a fixed rectangle in the world, set at 0 s: the camera's
shot at the start, grown to hold every Liquid thing's parent shape. Camera moves never slosh it and never restart it.
Drops that leave the area (out of a gap, off the edge) are removed. An **Area** setting on the scene-level liquid
(first Liquid thing's Liquid card, "Area" fold: Shot at the start / Fit the shapes / Custom with a margin) lets a
person make it bigger.

### Detail

One **Detail** setting on the Liquid card picks how many drops: Rough, Normal, Fine, Very fine, Extreme, with a number
underneath (drops per 100 x 100 px). It has a **Preview** and an **Export** value (export defaults one step finer). The
status line shows the drop count. The builder sets the numbers so Normal runs a third-of-the-screen pool at full
speed on Aedan's RX 9070 XT and Extreme reaches several hundred thousand drops for export.

Changing Preview Detail changes how the preview moves (it's a different simulation). When no liquid pushes balls,
export uses Export Detail and only the liquid's look differs. **When a liquid pushes balls** (Aedan picked "Match, with a
finer-export switch", 2026-11-17): export uses the **Preview** Detail by default so ball paths, hits and sound
come out exactly as tuned, and a **Finer export** switch on the Liquid card exports at Export Detail with the warning
"Balls will take different paths from the preview." The sound engine always steps at the Detail the picture it
belongs to uses (Preview in the editor, the export's in `bpe-cli`), so hits line up.

## 6. Physics settings

All numbers are settings with `span`s (no hard limits, past-span typing allowed, per the project's "no limits" rule),
keyframable and pulsable unless noted. Defaults make plain water.

### 6.1 What it's like (Liquid card)

| Setting | Does |
| --- | --- |
| Thickness | Viscosity, water through honey to tar. |
| Weight | Density. Decides what floats in it and which liquid sits on top. |
| Surface pull | Surface tension: low spreads into sheets, high pulls into beads and round drops. |
| Stretch | Holds together in strands and springs back (elastic part, for slime and jelly). |
| Tears at | How far a stretchy liquid pulls before it snaps (plastic yield). |
| Springiness | How firm it is: stiff and still, or bouncy with small waves (stiffness of its pressure). |
| Spray | How easily it breaks into droplets and mist at speed. |
| Settles | Damping: how long sloshing lasts. |
| Detail | Section 5. Not keyframable. |
| Steps a second | 120 or 240 liquid steps a second (default 240). Not keyframable. |

### 6.2 Heat and change (Liquid card, "Heat" fold)

| Setting | Does |
| --- | --- |
| Temperature | Starting heat, 0 to 1. |
| Cools / Warms | How fast it loses heat at its surface and gains it near heat sources. |
| Cold thickens it | How much Thickness rises as it cools (lava). |
| Freezes below / Melts above | Turns solid (drops stop moving relative to each other, balls skid on it) and back. |
| Dries up | Drops slowly vanish; with Stains on, they leave a mark where they were (as the Particles "Marks that stay"). |

Heat sources: balls with a Heat glow sub-layer, Light things with heat, and a "Heat at" point list on the card.

### 6.3 With the balls (Meets card)

| Setting | Does |
| --- | --- |
| Pushes balls | Whether this liquid's drops push balls. Balls always push every liquid aside (one way), on or off. |
| Floats or sinks | By each ball's own weight (radius², as `TwoWayOut`), or forced: Always floats, Always sinks, Bobs. |
| Ball weight | Multiplier on how heavy balls are to the liquid. |
| Drag | How much it slows balls moving through it. |
| Splash size | Scales what a landing ball throws up. |
| Carries balls | How much currents and waves move balls along. |
| Sticks to balls | Liquid clings and drips off as the ball leaves. |
| Wet balls | Balls come out shiny and tinted by the liquid, fading as they dry (draw-only, a ball Look effect fed by the liquid). |
| Wakes | Extra ripples behind balls moving along the surface. |
| Changes the ball | Off / Dissolves (shrinks to nothing) / Shrinks / Grows / Takes its colour, at a rate. |
| Target | Which balls it meets (the shared Target picker; default every ball). |

**Events** for Rules and sound: `Enter liquid` (a ball's centre goes under), `Leave liquid`, `Splash` (entry speed
over a threshold, with the speed as the event's strength). They join the event log as new tags beside Hit, Spawn and
Enter, worked out on the processor from the read-back buffer so they are deterministic.

### 6.4 Where it comes from and goes (Fill card)

| Source | Does |
| --- | --- |
| Fill | Fill the parent shape (or the scene) to a Level (0 to 1), at the start. Keyframing Level adds or removes liquid at the top (a rising flood). |
| Pour | A spout: place, aim, width, rate, speed, on/off. Several pours allowed (a list). |
| Rain | Drops from the top edge at a rate. |
| From balls | Balls leak liquid as they move, or burst into it when they break or are removed. |
| Drains | Holes that remove liquid, with a whirlpool strength. |
| Currents | Pumps (a push along a line), whirlpools (spin round a point), wind across the surface (from the scene's Air card if it has wind). |
| Magnets | Points that pull the liquid, with Spikes (ferrofluid lobes). A ball can be the magnet. |
| Most drops | A cap; the oldest drops are removed past it. |

Moving or spinning the container (a Shape's own motion, keyframes or rules) sloshes the liquid through the walls'
speed, no setting needed. Gravity is the scene's, with a per-liquid **Gravity** multiplier (negative pours upwards).

### 6.5 Shapes and other liquids (Meets card)

- **Shapes**: every shape blocks it (two-sided, like balls), with **Sticks to walls** (adhesion) and **Wet walls**
  (draw-only streaks). Gaps in shapes let it pour out.
- **Other liquids**: a list of rules, each naming a Liquid thing (Target picker) and picking **Mix**, **Layer** or
  **React**. Mix blends colour (section 8) and also blends Thickness and Weight in the meeting drops. Layer keeps them
  apart (a separating push between the two, plus their Weights sort them). React turns meeting drops of both into a
  third liquid (its own Liquid thing, named in the rule) at a rate, with optional fizz (bubbles), heat and a sound.
  With no rule, two liquids **Mix** (Aedan's favourite part is liquids blending).

## 7. Look

A new Look card for the Liquid thing, with layers in the existing layer system (`bpe-core/src/layers.rs`). The surface
is one layer ("Surface"); others sit above or below it.

**Surface** layer settings:

| Setting | Does |
| --- | --- |
| Surface | The shared 15-chip `SurfaceKind` with its sub-layer numbers, as the Particles Liquid layer uses since W15. |
| Clarity | How far you see in before its colour takes over (absorption by depth). |
| Bend | Refraction strength, with Rainbow split. |
| Light soaks in | Soft bright edges for milky liquids (subsurface). |
| Shine | Highlight strength (also from Surface). |
| Reflects | The scene mirrored in it (uses the lighting system's reflections when a Lighting card is there). |
| Ripples | Fine surface detail, smooth to choppy. |
| Edge | Soft to crisp, optional Outline (as other layers' Outline sub-layer). |
| Gives off light | Brightness of light it gives the scene (the Lighting card's light field, as "Gives off light" sub-layers). |
| Thin film | Rainbow colours that slide over it with its movement (soap, oil). |
| Smoothing | How hard the surface hides the drop grain (default high enough that none shows at 1080 x 1920). |

Other layers: **White water and foam** (where it's fast and on top, Foam lasts), **Bubbles** (rising, popping at the
top), **Things inside** (glitter, flecks), **Mist** (spray as fine dots), **Wet walls**, **Caustics** (bright ripples it
throws below, see lighting), plus the shared Shader layers (effects) every Look can hold.

**Drawing.** The liquid is drawn from its drops on the card without reading them back: splat them into a smooth
field (anisotropic kernels or a narrow-band distance field, the builder picks), take the surface, its normal, and a
"depth" from the distance to the edge for a rounded meniscus look. The shading is a new pass beside today's
`LiquidPass`, drawn in the same places (behind and front) and in layer frames.

**With a Lighting card** (Lit > 0) it writes the light passes the way `liquid.wgsl::real_lit` does for the Particles
Liquid layer: colour, blocks, height, surface index; see-through kinds join the glass picture so lamps bend through it
(the bent-light pass of the glass-light parts), and Caustics use that pass. Without one it draws its own pretend light,
like the Particles Liquid layer. A frame with no Liquid thing stays byte-identical.

## 8. Colour and mixing

Colour lives on every drop (three channels plus a mix weight) from part 1.

| Setting | Where | Does |
| --- | --- | --- |
| Colour | Look | One colour or a Gradient by Depth, Speed, Age, Temperature, or Its own (each drop keeps the colour it was made with). |
| Pour colour | Fill (each source) | The colour new drops get: one colour, a Gradient over time, cycle, or From the ball. |
| Mix style | Look ▸ Mixing | **Paint** (mixed in pigment space: blue + yellow = green), **Light** (adds up and brightens: red + green = yellow), **Blend** (straight average, in the project's Even/OKLab mix), **Marble** (never blends, only streaks), **Keep apart** (no colour passes between drops). |
| Mix speed | Mixing | Instant to never. |
| Mixes when | Mixing | Always, Only when stirred (gated by how differently neighbours move), Only on hits. |
| Swirl detail | Mixing | Resolution of a fine colour layer carried on the card above the drops (a dye grid moved by the liquid's own grid speed), so streaks stay sharp with fewer drops. Off = colour only on drops. |
| Colour strength | each source | A strong colour takes over a mix, a weak one tints. |
| Fades | Mixing | Ink fades or drifts towards one colour over time. |
| Balls drop ink / take colour | Meets | A ball leaves a cloud of its colour where it lands or moves; a ball comes out tinted (the Wet balls effect). |
| Shapes paint it | Meets | A shape's edge tints the liquid it touches. |
| Picture in the liquid | Fill | Start a Fill as a picture or a word (PNG or text, like Particles' "A word or picture" source). |
| Mixing shows | Look | A shimmer, foam or glow where colours are actively mixing. |
| Un-mix | Rules action and a Liquid card switch | Plays the liquid's own last N seconds backwards (the drops retrace their steps; balls are not reversed). While it rewinds the liquid pushes no balls (balls still push it). History is kept 60 times a second (not every step) for at most N seconds, played back smoothly between samples, and its size counts towards `GPU_CHECKPOINT_BYTES`; the editor shows its memory beside N. |

Mixing between different Liquid things uses the **Other liquids** rule (6.5); Mix style and speed come from the rule's
first named liquid unless the rule sets its own.

## 9. Sound

A Sound card on the Liquid thing, built on the existing sound engine (`bpe-sound`: voices, chains, modulators):

- **Splash voices**: Splash, Plop, Drip, Glug, Pour, Bubbles, each a voice with its own chain, fired by the liquid's
  events (Splash, Enter, Leave, pours running, bubbles popping).
- **Pitch from**: ball size, depth reached, splash speed, or the colour under the ball (a note per colour).
- **Underwater**: hits of balls under the surface are muffled and filtered by depth (a per-hit low-pass whose cutoff
  follows depth).
- **Modulators**: Slosh (how much the liquid moves), Mix amount (how mixed the pool is), Level, as modulator sources
  any sound setting can use.
- **Colour as notes**: each liquid's colour plays a note; where colours mix, their notes sound together.
- **Follow sound** works on liquid settings like any other (waves, level, colour pulsing with the music).

The sound engine steps its own physics playback, so it runs the liquid too (section 10).

## 10. How it runs

- **Where** (designer's call): all Liquid things in a scene share **one** simulation, so they can always meet. If any
  of them has Pushes balls on, the whole shared liquid runs in the main world (`ThingSim::in_main` true,
  `steps_own_cards`) and its physics settings count in the physics fingerprint; if none does, it runs beside the main
  world and edits to it restart only the liquid. Balls push the liquid either way (uploaded every liquid step), so a
  beside liquid follows the main world's ball movement, as the Huge swarm does.
- **The card**: every playback that steps a world holding a Liquid thing must have the graphics card named on its
  thread (as the swarm does). The sound engine and the aim job gain it. With no card, a Liquid thing holds no drops and
  the editor shows "Liquid needs the graphics card" on the thing; tests use `bpe_gfx::test_device()`.
- **Step**: at each liquid step (240 or 120 a second), upload the balls that the liquid meets (place, speed, radius,
  row) and the shapes' outlines (reuse the swarm's segments on a grid, `BallGrid::parts`), run the compute passes,
  and write a small "what the balls feel" buffer: per ball, momentum change and how much is under. Read it back.
- **One step late**: the balls get that push at the first substep of the next world step, as `TwoWayOut` does. This
  lets the read-back overlap the next step's work. Events (Enter/Leave/Splash) are worked out on the processor from
  the same buffer.
- **Deterministic**: anything summed on the card with atomics sums in fixed point (integers), as the old GPU liquid
  and swarm did; drops are processed in a stable order; no random numbers but the shared dice (`bpe_core::dice`). Same
  scene, same card, same Detail = the same drops, every play. Different cards may differ slightly (as goldens already
  differ on Windows); CI pins the software driver.
- **Checkpoints and seeking**: the liquid's state (drops, dye grid, history for Un-mix) is copied on the card for each
  checkpoint and counts towards `GPU_CHECKPOINT_BYTES`, thinned like the swarm's.
- **Fingerprints and restarts**: liquid physics settings restart the physics when in the main world and only the
  liquid when beside; look settings restart nothing. Wire every hook so `bpe-kit/tests/restarts.rs` passes.
- **Cost guard**: the preview never blocks the window: like the old GPU budget (`gpu-layers` memory), the editor caps
  catch-up work per frame and shows the preview falling behind instead of freezing.
- **Scene files**: the new thing and cards save like every other thing. Bump the format only if the builder needs to.

## 11. Testing

- Goldens: new `crates/bpe-kit/tests/golden-liquid*.txt` (debug and release) for a few liquid scenes on the software
  driver: ball paths and liquid summary lines (drop count, centre of mass, level) at set steps. Existing goldens must
  not move.
- Determinism: the same scene stepped twice, and stepped through a checkpoint and a seek, gives byte-identical drops.
- Behaviour tests (small Detail): water fills a box and settles level; a light ball floats, a heavy one sinks; a ball
  dropped in makes a Splash event; honey slows a ball more than water; two liquids with Layer end with the light one
  on top; Paint mix of blue and yellow gives green; Marble keeps distinct colours after stirring; Un-mix returns the
  drops to their places.
- Picture tests (`test_device()`): lit and unlit surface, glass bend, foam, no grain at a set Smoothing.
- Full-size look check: add `liquid-water`, `liquid-honey`, `liquid-mixing` and `liquid-lit` scenes to
  `full_size_look.rs` and look at them for grain, seams, light leaks and flicker.
- The restart test covers every new setting.

## 12. Parts and order

Each part is a fresh thread with its own plan in `docs/superpowers/plans/liquid/`, one Opus review at the end, merged
by the thread when CI is green.

1. **The liquid itself, with colour and mixing.** The Liquid thing and cards, the shared simulation on the card
   (water through honey: Thickness, Weight, Springiness, Settles, Spray, Surface pull, Gravity), Fill and Pour, the
   Area, shapes blocking it, balls pushing it aside (one way), Detail, checkpoints, every playback given the card,
   beside the main world only (Pushes balls arrives in part 2), and a **speed readout** in the status line (drops and
   milliseconds a liquid step) so Detail's numbers can be set from Aedan's own card the next time he installs. Colour per
   drop, several Liquid things sharing one body, Other liquids Mix / Layer, Mix styles Paint, Light, Blend, Marble,
   Keep apart, Mix speed, Mixes when, Swirl detail. A plain lit-free surface draw (smooth, no grain) so it can be seen.
2. **Balls.** Pushes balls on (main world, read-back, one step late), Floats or sinks, Drag, Carries, Splash size,
   Sticks to balls, Wakes, the three events and Rules support, balls drop ink and take colour.
3. **Looking real.** The full Surface layer, Lighting card integration (passes, glass bend, caustics, reflections,
   gives off light), White water and foam, Bubbles, Mist, Things inside, Thin film, Wet walls, Wet balls, Mixing shows.
4. **Stranger liquids.** Stretch and Tears at, Heat (temperature, cooling, freezing, melting, drying, stains), React,
   Rain, From balls, Drains, Currents, Magnets, Changes the ball, Shapes paint it, Picture in the liquid, Fades,
   Un-mix.
5. **Sound.** The Sound card, splash voices, underwater, modulators, colour as notes; plus a few built-in liquid
   scenes in the Scenes window (Aedan asked for built-in scenes; these extend that library).

Parts 3 and 4 can run side by side after 2; 5 after 2.

## 13. Not doing

- No 3D liquid, no liquid on the Stage camera beyond a flat layer.
- No changes to the Particles thing or its Liquid layer, and no tool to convert one to the other.
- No hardware ray tracing (the lighting system decided against it).
- No backward compatibility work: there are no old Liquid things.
- No built-in or example scenes outside part 5's additions to the Scenes window library, and the editor still opens
  blank.

## 14. Left to the builder

- The exact solver (MLS-MPM is the reference; anything with the same behaviour and determinism will do), grid size,
  and how elastic and plastic parts are modelled.
- How surface tension is approximated (MPM has none built in; a cohesion term or a surface-based force).
- The splatting and surface method, and how the fine colour layer is stored and moved.
- Detail's numbers per step, and the read-back mechanics (staging buffers, polling) within the one-step-late rule.
- Card and slot names in code, keys, the event tags' numbers, and whether the scene format needs a bump.
- How the sound engine and aim job get the card on their threads.
