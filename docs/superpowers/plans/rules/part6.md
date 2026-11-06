# Rules part 6: spectacle actions

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md` (sections 4, 6 "Do", 9). Part 1 (`part1.md`) is merged: this
part only adds block kinds, actions and the places that read them. Design approved by the owner on 2026-11-06 (rules
design thread); no approval stops. An "As built" section at the end records anything that moved.

## Goal

Five Do blocks that make a moment feel big:

| Block | Id | Does |
| --- | --- | --- |
| Camera | `do.camera` | shakes the camera (strength, time, wobble speed) or punches its zoom (amount, time) |
| Slow motion | `do.slow` | the simulation runs at N× (gliding in and out) for N s or for good, while the video clock and keyframes run on |
| Flash | `do.flash` | fires who's Flash on hit and Colour on hit layers as if hit |
| Lighting scene | `do.light_scene` | shows a lighting scene (a number, next, previous, random, the lights' own, or back to the card's Show) with a crossfade |
| Play a note | `do.note` | a shape's voice plays a note (a set note or its own next note as if hit), heard in the editor and in export |

A scene without rules, or with rules that use none of these, simulates, draws and sounds byte-identically (every golden,
picture hash and sound golden unchanged).

## Design decisions

1. **Everything goes through the rules' world state or the log, never the draw's memory.** Camera kicks, slow motion
   and the lighting scene shown live in `RulesState` (cloned with the world, so snapshots, seeks and the sound
   engine's own playback keep them). Flashes and notes are log events, because the things that react to hits (the
   look's reactions and the sound engine) already read the log.
2. **Actions apply at the next step's start**, like part 1's: `Action::Kick`, `Action::Pace`, `Action::Flash`,
   `Action::LightScene`, `Action::Note`.
3. **New event kinds** get tags 20 and 21 (`Flash`, `Note`), leaving 12..19 for parts 3 to 5 building at the same
   time, so the merges don't fight over numbers.
4. **Slow motion is a pace.** Each step the world works out `pace` (1 normally) from the rules' pace changes at the
   video time, and the physics takes `dt × pace`: substeps, forces, moving, collisions, the resting threshold and the
   speed ceiling (`ceiling / pace`), and the walls' scripted spin and shrink (`World::sim_dt`). The video clock
   (`World::time`), keyframes, the camera, emitters' and Release's schedules, freezes, trails' 60-a-second history
   and every draw run on video time. A ball's velocity stays in pixels a (simulated) second, so hit strengths and the
   notes they play are unchanged; hits just come further apart. `RulesState.lag` counts how far the simulated clock
   is behind (`World::sim_time() = time() − lag`), and Motion's and Bounce's `SimCx.time` read the simulated clock
   (exactly `step / rate` while `lag` is 0).
   - Pace changes are part 1's `Change` (How::Set on `Value::Number(1.0)`, its glide, ease, "for a time then back"),
     folded with `fold`, so a second slow motion glides from the first and hands back to it when it ends.
   - Range 0.01× to 8×. Faster than 1 makes each step move further; the ceiling shrinks by the pace so a step never
     moves a ball further than it can check.
   - Aiming (`World::lone`) copies no rules: an aimed round flown during slow motion doesn't land where aimed (the
     aim check's existing "Rules may change these balls' paths" covers it).
5. **Camera kicks** are `Kick { thing, start step, punch: bool, amount, seconds, rate }` in `RulesState.kicks` (at most
   `MOST_KICKS` 64, oldest dropped; over ones pruned at the step's start). The camera sim, after its own shake and
   punch, adds each of its kicks: weight `(1 − age/seconds)²`, shake = smooth noise at `age × rate` (salted by the
   kick's start step, so two kicks never move together) × amount px, punch = zoom × (1 + Σ amount × weight). A Kick
   on a thing that isn't a camera is never made (the block only aims at cameras; a fixed one that isn't is a problem).
6. **Flash** logs `EventKind::Flash` (`a` = the ball's row or (thing, 0) for a thing without balls, `point` the
   ball's place or the thing's pose, `speed` the strength as a hit speed: strength × `FULL_FLASH_SPEED`) and counts
   it in `RulesState.flashes[(thing, item)]`. A whole Balls thing flashes each live ball (one event a ball). Readers:
   - balls: `last_hits_but` takes Flash like a hit (age and strength for Flash on hit / Colour on hit sub-layers,
     trails' and particles' hit subs), the reaction Flash on hit's sprite scan takes it (not a hit for the scene's
     effects), and `GroupCx::hits` (hits so far, for Colour on hit's hue turns) adds the flashes;
   - shapes: `shape_hits` takes Flash events with `a.thing` the shape (age, strength, count).
   - Nothing else reads it: not physics (`Bodies.hits` is untouched), not sound, not particles' "on hits", heat,
     rigs, lights' flare or the scene's effects.
7. **Lighting scene** keeps `ShownScene { step, from: Option<u32>, to: Option<u32>, fade: Option<f32> }` (`None` =
   the card's own Show at that moment; `Some(0)` = the lights' own settings; a fade of `None` = the card's
   Crossfade). The action carries a `SceneOp` (Scene(n) / Next / Previous / Random / LightsOwn / Card) and the scene
   count, and is resolved at apply time against the scene a rule showed last (Next after none is scene 1; Random is
   `dice`d from the seed and step and never the same as now; asking for what already shows changes nothing).
   `light_scenes::now(cx)` then shows the rule's scene from its moment, crossfading from what showed (the card's
   pick at that moment when the rule took over from the card), until a later rule picks another or hands back to
   the card.
8. **Play a note** logs `EventKind::Note`: `a` = the firing's ball (or (shape, 0)), `b` = (shape, pitch 0..127 or
   `AS_IF_HIT`), `point` the firing's point (pan across the shape, Hit modulators), `normal.x` the note's length in
   seconds (0: the envelopes' own gate), `speed` the velocity 0..1. The sound engine's `HitCursor` places Note events
   on their samples like hits; `ShapeVoice::take_hits` plays one for its shape bypassing the hit filter (min strength,
   retrigger, heard from), a set pitch as that note or `AS_IF_HIT` as the slot's next pick (counting as a hit for the
   play order, not for the slot clock); a drone shape plays it on its own voice beside the held notes (as clicked
   keys do). A length > 0 closes every envelope's gate after it. `catch_up` replays `AS_IF_HIT` picks after a seek.
   Follow sound's `ControlSampler` hears Note events on its shape as accepted hits with the velocity as strength.
   MIDI export records them (they go through `start_note`). The test-tone voice plays on them too.

## Files

| Crate | File | What |
| --- | --- | --- |
| bpe-runtime | `src/events.rs` | `EventKind::Flash` (20), `EventKind::Note` (21) with their field use documented |
| bpe-runtime | `src/rules/shows.rs` (new) | `Kick`, `MOST_KICKS`, `kick_weight`, `SceneOp`, `ShownScene`, `AS_IF_HIT`, `FULL_FLASH_SPEED` |
| bpe-runtime | `src/rules/actions.rs` | `Action::{Kick, Pace, Flash, LightScene, Note}` and applying them |
| bpe-runtime | `src/rules/mod.rs` | `RulesState.{kicks, pace, lag, flashes, scene}`, `pace_at`, `kicks`, `flashes`, `light_scene`, pruning |
| bpe-runtime | `src/world.rs` | `World.pace`, `sim_dt`, `sim_time`, `sim_clock`; the step scales dt; limits by pace; lag after the step; `lone` resets |
| bpe-kit | `src/rules/camera.rs`, `slow.rs`, `flash.rs`, `scene.rs`, `note.rs` (new) | the five blocks |
| bpe-kit | `src/rules/mod.rs` | registers them |
| bpe-kit | `src/things/camera/sim.rs` | kicks added after the hits' shake and punch |
| bpe-kit | `src/things/shape/sim.rs` | spin and shrink by `sim_dt` |
| bpe-kit | `src/things/balls/draw.rs`, `things/shape/draw.rs` | Flash read as a hit by the reactions |
| bpe-kit | `src/modules/light_scenes.rs` | `now` shows a rule's scene |
| bpe-kit | `src/scenes/rule_scenes.rs`, `tests/golden-rules*.txt` | a "Slow motion" scene (re-blessed) |
| bpe-sound | `src/hits.rs`, `src/voices/shape.rs`, `src/control.rs`, `src/engine.rs` | notes from rules |

## Blocks (bpe-kit `rules/`)

All settings in group "Rule"; numbers marked * can come from the firing (`Amount`).

- **Camera** (`CameraSettings`): `who` (default `Who::Kind("camera")`), `mode` Shake / Zoom punch, `amount`* (Shake,
  px, span 0..200, default 40), `punch`* (Zoom punch, share of the zoom, range −0.95..20, span −0.5..1, default 0.15),
  `seconds`* (range 0.01..1e9, span 0..3, default 0.4), `rate` (Shake, wobbles a second, range 0.1..200, span 1..60,
  default 18). Prepare: a fixed who that names a thing that isn't a camera is a problem ("<name> isn't a camera").
  Act: each target whose kind is `camera` gets an `Action::Kick`.
- **Slow motion** (`SlowSettings`): `speed`* (range 0.01..8, span 0.05..2, default 0.25), `glide` (s, default 0.3),
  `ease` (`GlideEase`), `lasts` (`Lasts`, default For a time), `seconds` (default 1). Act: `Action::Pace(Change)`.
- **Flash** (`FlashSettings`): `who` (That), `strength`* (range 0..1.5, default 1). Act: per target an
  `Action::Flash` (a Group or whole ball thing flashes every live ball at apply time).
- **Lighting scene** (`SceneShowSettings`): `show` A scene / Next scene / Previous scene / Random scene / The lights'
  own / Back to the card, `scene`* (Int from 1, when A scene), `crossfade` The card's / Its own, `fade` (s, when Its
  own, default 1). Prepare reads the number of scenes on the scene's Lighting card (0 without one: the scene ops then
  do nothing). Act: `Action::LightScene`.
- **Play a note** (`NoteSettings`): `who` (default Other: the shape hit), `pitch_from` A note / The shape's next note,
  `note`* (Int 0..127, default 60 = C5, when A note), `velocity`* (0..1, default 0.8), `length` (s, 0 = as the
  envelopes, span 0..4). Act: per target of kind `shape` one `Action::Note`.

## Tests (what passing looks like)

- runtime (`rules/tests.rs`, tiny test blocks): a pace of 0.5 makes a ball fall a quarter as far per step squared as
  normal over the same steps (exact against a plain world run with gravity halved twice is not needed: compare
  distances fallen, ≈ 1/4 within 1%), `sim_time` lags by the pace, `limits` unchanged at pace 1; kicks are kept and
  pruned; a Flash event per live ball with counts; scene ops resolve (Next wraps, Previous wraps, Random never the
  same, Card → `to: None`, the same scene again changes nothing); a Note logs its event at the next step.
- kit (`rules/tests.rs`): Camera shake moves the camera's view from the plain run and is gone after its time; Zoom
  punch zooms; a non-camera named is a problem; Slow motion slows balls and the shape's spin and hands back; Flash
  gives a ball's layered instance a hit age of 0 and a Colour on hit count of 1 without any hit, and a shape's path
  the same; Lighting scene: `now` shows the scene picked, crossfades, Next/Previous, back to the card; Play a note
  logs a Note on the shape with the pitch.
- sound (`bpe-sound/tests/rule_notes.rs`): a scene whose ball never hits, with a Time rule playing a note on a shape
  with sound, renders sound after the note's moment and none before; the MIDI log holds the note's pitch; a seek
  replays it.
- goldens: `rule_scenes` gains "Slow motion" (every 3rd hit on the shape slows the world to 0.3× for 1 s); bless
  `golden-rules*.txt` in debug and release; every other golden line unchanged.
- `cargo clippy --workspace`, `cargo fmt`, the whole workspace's tests.

## Not doing

- No editor work (part 2 brings the Rules panel; these blocks show through its generic block inspector, and their
  faces use the default summary until part 2 adds summaries).
- Slow motion never slows the video clock, keyframes, the camera, emitters' or releases' schedules, freezes or
  effects; aimed rounds don't know about it.
- Flash doesn't count as a hit for physics, sound, particles' "on hits", heat glow, rigs, lights or effects.
- A rule's lighting scene isn't shown in the editor's Show scene readout (`words_at` has no playback).

## As built

- Built as planned. `FULL_FLASH_SPEED` is one constant (600 px/s, `bpe_runtime::rules`), shared with the balls'
  Flash on hit sprites, so a Flash of strength 1 flashes as a full-strength hit does; Strength runs 0 to 1.5.
- Slow motion: `World.pace` (from `RulesState::pace_at`, a fold of the Pace changes over 1) scales `sim_dt`, the
  speed ceiling (`limits().ceiling / pace`) and the resting threshold; `lag` grows by `dt × (1 − pace)` a step and
  `sim_time` = `time − lag`; `SimCx.time` is exactly `step / rate` while `lag` is 0, so nothing moves without a rule.
  A shape's own Spin and Shrink follow `sim_dt`. Keyframed moves carry no wall velocity (as before), so they run on at
  the video's pace. Emitter aim uses the ceiling at pace 1 (aiming ignores slow motion).
- Lighting scene: `light_scenes::now` reads the card as it stood at a moment (Show scene from its hold keys, hits
  heard up to that moment), so a crossfade from the card never jumps when the card would have moved on mid-fade.
- Play a note: `ShapeVoice::rule_note`; the ControlSampler (Follow sound) takes the note's velocity as its strength
  (and the Hit modulator's), its length as its gate and the engine's seed; the compressor's Hits key takes it by its
  velocity; the MIDI log closes a drone's own-voice note where its gate closes.
- Review follow-ups not done: a rule note on an Always/In range shape counts in Follow sound only as the latest hit
  (the engine gives it a short voice of its own), and rule notes ignore Strum.
