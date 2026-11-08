# Foundations: the strip-down, own settings, Vary across balls

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

## One engine, stripped to its core

On 2026-11-03 the owner had everything except the core removed, to rework the program from a smaller base. The
version before that is **Version 1**, kept on the `version-1` branch (with its exes in the owner's
`bin\version-1\` folder). Removed in that pass: the old engine (`bpe-app`, `bpe-scene`, `bpe-sim`, `bpe-audio`,
`bpe-render`, `bpe-export`, `bpe-music`), node graphs and rules (`bpe-graph`, `bpe-vm`), the Explore and Batch
windows, the canvas and paint marks, the whole sound and chords system (`bpe-synth`, `bpe-mix`), and the Obstacle,
Liquid, Soft body, Swarm, Field, Meter, Emitter and Text things. A new sound system is being built in six parts
(spec `docs/superpowers/specs/2026-11-04-sound-system-design.md`, plans in `docs/superpowers/plans/sound-system/`,
decisions log `decisions.md` there); all six parts are in: 1 (engine, playback, export audio, Master), 2 (shape
voice, Sound tab), 3 (oscillator layer chains), 4 (modulators, drones, Follow sound), 5 (notes, scales, slots) and
6 (buses, console-strip Mixer, bus effects, stems and MIDI export).

Rework step 2 (same day) dropped shared modules: every thing owns its settings. A thing's cards (Look,
Motion, Bounce, Shader) live in `Thing.slots` as `Slot { kind, params, stash }` (balls have no Shader card and shapes none of Motion or Shader: effects on a thing are Shader layers of its Look; only the scene settings keep a Shader card, the picture's Effects; trails and particles are Look layers, and `SlotId::Trail`/`Particles` are kept only for the test scenes' old cards, which `cards_to_layers` turns into Look layers); nothing is shared,
linked, inherited from a folder or locked, and there is no Library, presets or randomize (dice). Right-clicking a
setting opens Copy Value / Paste Value (…) / Create Keyframe; there are no keyframe diamonds in the inspector.

**Vary across balls** (Ball Emitter part 5, plan `docs/superpowers/plans/ball-emitter/part5-vary.md`): on a Balls
group (or an Emitter) the row menu adds Vary across balls / Stop varying. A varied setting stores `"<key>~to"`,
`"<key>~by"` (`VaryBy`: Number, Round, Launch angle, Place, Random, Speed, Hits) and `"<key>~ease"` beside it, layered
keys keeping their prefix (`bpe-core/src/vary.rs`: `vary_of`, `with_varies`, `BallSpread::t`, `Scene::start_varying`/
`stop_varying`/`vary_use`; `def_at` answers the parts, To keys like any setting). Which settings may vary is the
kinds' say: `ThingKind::vary` (balls: Size live, Launch speed at launch, so no Speed/Hits; an Emitter: Size
live, Speed at launch, `EmitterState::varies`/`own_size`, unaimed balls only), `ModuleKind::vary`
(Motion, Bounce), `LayerKind::vary` (ball layers but group layers, Blend and the trail/particle settings the whole
layer shares, `layers/vary.rs::layer_vary`). Places in a spread are quantised to `VARY_LEVELS` (1024) steps, and
everything is shared per step: the runtime's `rows.rs` keeps a prepared Motion/Bounce state per ball
(`ModuleSim::per_ball`, `forces_one`; `Slotted::state_for(row)`), `BallsState` keeps each ball's own size and launch
(`spread_of`), and the draw keeps a stack per step (`layers/vary.rs::VariedLook`, `balls/draw.rs::Worn`; records
keyed by stack and worn layers; trails pick each ball's copy through `Balls.own`, particles work out each copy's
emissions then cap them together). Nothing changes when nothing varies (goldens). The editor's `vary.rs` has
`VaryCx` (on `GridCx.vary`), `VaryEdit`, the two-ended slider and the "Varies by [spread] [easing]" line; a pair,
colour or gradient gets a "to" row; `Edit::Vary` starts/stops; timeline lanes of To read "(to)".
