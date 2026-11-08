# One wind, and rules on more cards (design)

Workstream W13 of the 2026-11-08 improvement review ("Ball Studio code review: what to improve"), findings G4 and G8.
Aedan picked every finding outside the Size section on 2026-11-08 15:18Z, which approves the review's suggested fixes;
this spec turns them into a design. The plan is `docs/superpowers/plans/2026-11-08-one-wind-rule-cards.md`.

## The problems

- **G4, four winds.** The Motion card's Wind (a steady acceleration), the Air card's Wind (the scene's air: a speed
  and direction with gusts, swirled by balls and blocked by shapes), the Particles Wind force (a private wind for one
  Particles thing) and the Lighting card's haze Drift direction all move things "with the wind", and none of them
  knows about the others. Only Particles read the Air card. Balls stir the air but the air never pushes them.
- **G8, rules can't reach four cards.** Change setting offers the thing's own settings, Look, Motion, Bounce, Effects
  and Lighting. It can't change the Health, Air, Sound or Mixer cards, so a rule can't start a gust, make a thing
  tougher, open a filter or pull a bus fader down.

## What changes for the owner

1. **Balls follow the air.** The balls' Motion card gains **Follow the air** (group Air, "per s", 0 = off, keyframe,
   pulse, varies across balls). Above 0 each ball is carried towards the air's speed where it is, by that grip, the
   same maths as the Particles Follow the air force: `v += (air(p) − v) · (1 − e^(−grip·dt))`. The air is the real
   Air card air: the wind with its gusts, swirled by the balls (so balls now push each other through the air), blocked
   by shapes (a ball behind a shape is sheltered) and lifted by heat from particles. Without an Air card the air is
   still: balls only stir it and feel each other's wake.
2. **The Motion card's Wind is renamed Steady push** (same key, same maths), with help that points to Follow the air
   for the scene's wind. The Particles Wind force keeps its name (it is that thing's own wind) and its help points to
   Follow the air too. So "Wind" as a setting name means the Air card's wind everywhere a thing shares it.
3. **Haze moves With the air.** The Lighting card's Haze moves gains **With the air**: the haze's clouds drift with
   the Air card's wind, speed, direction and gusts as keyed, Speed × scaling it. Without an Air card it stands still.
4. **Rules change four more cards.** Change setting's Card gains **Health**, **Air**, **Sound** and **Mixer**.
   - Health: every setting the card has (Starting health applies to balls born after the change; balls keep the
     health they have).
   - Air: every setting (a gust on a hit: Wind × 3 for 1 s with a glide).
   - Sound and Mixer: every number the sound engine reads once a block, the same ones timeline keys reach on the
     voice (the oscillators, the chain and modulator layers, Playing's Voices, Glide, Min hit strength, Retrigger
     guard, Output's Volume, Pan, Follow hit and Sends) and on the mixer (each bus's Fader, Pan and Sends, the
     Master's Input and Output, and every number of the bus effects). The note slots' timing (Strum, Every, Jump to
     slot) and switches stay as set. The menu only lists what a rule reaches.

## Design decisions

- **The air steps with the physics when balls follow it.** A scene's balls *feel the air* when any Motion card's
  Follow the air is above 0, varied above 0, keyed, pulsed or changed by an enabled rule (`bpe_runtime::air_felt`,
  read from the saved scene, so every world agrees). Then every world built from the scene (the editor's main
  playback, the particle playback, the sound engine's physics, the export) wakes the air at build and steps it at the
  start of every physics step, before the forces; balls read it once a step at their place (like Moths read the
  light). Scenes whose balls don't feel the air are unchanged: no golden moves.
- **A Particles thing that heats the air steps with the physics while balls feel it**, as a two-way one does.
  Otherwise the particle playback (which has the heat) and the main playback (which hasn't) would move the balls
  differently. Following the air without heating it changes nothing for the balls, so such a thing stays beside.
- **The fingerprint** hashes the Air card and its keys whenever balls feel the air (today only for two-way
  particles), and the main world senses Air keys that follow sound or light.
- **The runtime asks the kit for the air through `Sims.air`** (`AirSim`: wake, catch up, velocities at points), the
  way `Sims.light` gives light: the air solver stays in the kit.
- **Rule changes on Air and Health are physics**: `World::is_physics` counts the Air and Health slots, so a change is
  worked out every step in every world. An Air change reaches the scene sim's `settings_changed` (the air re-reads
  its card, as for keys); a Health change prepares the thing's Health card again (`Health::set_card`).
- **Sound and Mixer changes are read by the sound engine**, never by the physics: each block, every number knob is
  as set, then its timeline key, then the rules' changes on it in the engine's own physics world (which runs the same
  rules), folded at the block's time with the same `Change` maths. Looking a change up never allocates (the keys are
  made when the patch is built). The control sampler behind Follow sound folds the same changes, so physics that
  follows a shape's level hears a rule's Volume change too.
- **Haze drift is an integral the world keeps**: the scene sim adds the air's ambient wind × the step to
  `Air.drift` every step (cheap, deterministic, carried by checkpoints), and the scene draw hands the haze that
  offset. The shader takes the drift as an offset in pixels for every mode (Drifting and Swirling work it out from
  Speed and the time as before), so it needs no history.

## Not done (and why)

- Rules can't change the Sound card's switches and choices (wave, filter type, notes) or the note slot timing: the
  patch is built from them, and changing them mid-sound needs a patch swap per change. Timeline keys can't reach
  them either.
- A Particles thing whose Rise "Heats the air" is 0 but keyed above 0 is not counted as heating (the same limit the
  particles' own air waking has).
- Haze gusts don't travel as waves (the haze drifts as one sheet); the balls' and particles' air does.
