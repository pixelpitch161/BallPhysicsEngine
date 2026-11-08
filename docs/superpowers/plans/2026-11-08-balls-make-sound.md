# W10: balls and emitters make sound

Workstream W10 of the 2026-11-08 improvement review ("Ball Studio code review: what to improve", findings G1 and
A5, the same thing seen from Gaps and from Sound). Aedan picked everything but the Size section on 2026-11-08, so
the review's suggestion is the approved design: "Let Balls and Emitters take a Sound card, using the same voice as
shapes, and let them hear their own hits plus fire, catch and break moments." This file is the design and the plan.
W9 (sound engine fixes) is merged; W11 waits on this one.

Rule for the whole part: nothing changes for a scene without a Sound card on a Balls or Emitter thing. The physics
goldens and the sound goldens (`crates/bpe-sound/tests/golden-sound*.txt`) must not move.

## Design

A **Balls** thing (one ball or a group) and an **Emitter** take the same Sound card a shape has (`sound.voice`, the
whole voice: oscillators, chain layers, envelopes, modulators, notes, slots, Output, buses). The card is added from
the Sound tab ("Enable sound"), as on a shape. Its balls are what sound:

| Moment | Event in the log | Setting | Default | Who |
| --- | --- | --- | --- | --- |
| Hit a shape | `Hit`: one of its balls (`a` or `b`) met a thing without balls (a shape, a Text, a wall) | `moments_hit_shape` (exists, Particles') | on | Balls, Emitter |
| Hit a ball | `Hit`: one of its balls met a ball (its own or another thing's) | `moments_hit_ball` (exists) | on | Balls, Emitter |
| Fired | `Spawn` with `b.thing` = this emitter | `moments_fire` (new) | off | Emitter |
| Caught | `Enter`: one of its balls came into a catcher, or (an Emitter) a ball came into it | `moments_catch` (new) | off | Balls, Emitter |
| Broken | `Break` with `a.thing` = this thing (a rule broke one of its balls, or the thing) | `moments_break` (new) | off | Balls, Emitter |

Decisions:

- **One event, one note per voice.** A hit of ball A (thing 1) on ball B (thing 2) plays on thing 1's voice and on
  thing 2's voice (each hears its own ball); a hit between two balls of the same thing plays once. A ball hitting a
  shape that has its own sound plays on both, as two instruments would.
- **The voice sees the event from its own ball.** Before a ball voice reads a `Hit`, the event is turned round so
  that `a` is its own ball and `b` what that ball met (`HitFilter::seen`). Everything downstream then works
  unchanged: the retrigger guard is per own ball, By ball plays its own ball's number, Each note's ball follows its
  own ball, Spell notes / By letter read the letter its ball hit (`b` is the Text). `Spawn`, `Enter`, `Break` are
  already from the ball (`a`).
- **Heard from** on a ball voice names what its balls must hit to play (default Anything). It applies to hits only;
  Fired, Caught and Broken are its own moments. The Sound tab labels it "Hits on".
- **Strength.** A hit's strength is its speed through the voice's strength curve (Full strength at, Strength curve)
  as on a shape. Fired uses the launch speed, Caught the speed it came in at, Broken the strength of the hit that
  broke it (the `Break` event's `speed`), all through the same curve, so Full strength at shapes them all.
- **Hit modulator.** Place on edge and Side read the shape the ball hit (`b`) instead of the voice's own thing; on a
  shape's voice that is the same thing (a hit's `b` is the shape), so shapes don't change.
- **Distance** on a ball voice measures from the thing's own place (it has no outline: Centre works, Edge reads
  nothing), as on a Text. In range works with it. Not changed in this part.
- **By side** has no outline on a ball voice: it plays side 0, as it already does on a Text (the Notes panel offers
  every Plays choice on every thing; that stays).
- No rate limit of its own (Particles have Most notes a second because a cloud logs thousands of moments): balls
  hits are already in the log, and Voices, the retrigger guard and Min strength limit them as on a shape.
- **Shapes, Text and Particles hear exactly what they heard before**: the shape filter now names the kinds it takes
  (`Hit`, `Moment`), so the new event kinds that reach the voices never play on them, and a Hits sidechain keys
  only on hits, notes and moments as before.

## Plan

### bpe-kit

1. `sound/voice.rs`:
   - `VoiceSettings`: add `moments_fire` ("Being fired"), `moments_catch` ("Being caught"), `moments_break`
     ("Breaking"), all `bool`, group Playing, default `false`. Doc comments say Balls/Emitter.
   - `VoiceKind::fits`: also `things::balls::KIND`/`"balls"` and `things::emitter::KIND`. Help: "The thing's
     sound: ...".
   - `pub fn hears_own_balls(thing_kind: &str) -> bool`: Balls or Emitter (the one place the kinds are named).
2. `things/balls/mod.rs` and `things/emitter/mod.rs`: `SLOTS` gain `SlotId::Sound` (not `DEFAULT_SLOTS`).

### bpe-sound

3. `hits.rs`: `plays` also takes `Spawn`, `Enter`, `Break` (they reach the voices). New `touches(kind)`: `Hit`,
   `Note`, `Moment` (the old set), used by the Hits sidechain (`fx_chain.rs`) and the test tone (`voice.rs`) so they
   behave as before.
4. `voices/hit_filter.rs`:
   - `pub struct BallMoments { hit_ball, hit_shape, fire, catch, broken: bool }` with `of(&VoiceSettings)`.
   - `HitFilter.balls: Option<BallMoments>`; `HitFilter::hearing(self, Option<BallMoments>) -> Self`.
   - `HitFilter::seen(&self, &Event) -> Event`: for a ball voice, a `Hit` whose `b` is its thing and `a` isn't
     comes back with `a`/`b` swapped and the normal turned round; anything else as it is. Allocates nothing.
   - `accept` (on a seen event): a shape voice takes only `Hit` and `Moment` with `b` = its thing (as before); a
     ball voice takes, per `BallMoments`: `Hit` with `a` its thing and a `b` (Hit a ball when `b.thing` has ball
     rows, `world.bodies`, else Hit a shape) named by Heard from; `Spawn` with `b` its thing; `Enter` with `a` or `b`
     its thing; `Break` with `a` its thing. Then strength, Min strength and the retrigger guard as now (keyed by
     `a`, the own ball).
5. `patch.rs`: `ShapeSpec.balls: Option<BallMoments>`, set in `spec_with` when the scene thing's kind
   `hears_own_balls`. `ShapeVoice::new`, `ControlSampler::new` build their filters `.hearing(spec.balls)`;
   `voices/slots.rs::slot_changes` builds it from the scene thing's kind and the card.
6. `voices/shape.rs`: `take_hits` and `catch_up` read `self.filter.seen(&hit.event)` (a stack copy) for every
   event but a rule's `Note`. `mods/hit.rs::hit_values`: Place and Side on `b`'s thing for a `Hit` (`shape`
   otherwise).
7. `control.rs` (Follow sound): read every event that `plays`; a `Note` counts only when its `b` is the voice's
   thing (as now), anything else goes through the filter (seen, then accept) and the seen event is kept. A shape's
   Follow sound now also hears its moments (Particles), as the engine does.
8. `voices/slots.rs::slot_change_times`: read every event that `plays` but `Note` (the filter sorts them), so a By
   hits slot change on a ball voice counts its fires and catches as the engine does.

### bpe-studio

9. `inspector.rs`: `sounds(kind)` also true for Balls and Emitter, with chips "Settings" / "Sound" ("The sound its
   ball makes when it hits things", emitter: "The sound its balls make when they are fired, hit things or are
   caught"). `+ Add card` lists only slots that are in the card list (`SlotId::in_card_list`): Sound and Mixer are
   added from their own tabs.
10. `sound_tab`: `moments::is_balls(cx)`; the line under Enable sound: "Turn it on and its balls play a note when they
    hit things." `moments::show_balls`: "Plays when its balls" with switches Hit a shape, Hit a ball, Are fired
    (Emitter only), Are caught, Break. `playing.rs`: "Heard from" reads "Hits on" on a ball voice with its own hover
    text.

### Not in this part

- Balls released one by one log no `Spawn` (only an Emitter fires), so Fired is the Emitter's.
- A rule's Play a note on a ball voice, a Hits sidechain keyed on a ball thing, Distance from each ball: later, if
  wanted.

## Verification

- `crates/bpe-sound/tests/ball_sound.rs` (new):
  - A ball with the starter sound falling on a shape plays notes; the same with Hit a shape off plays none.
  - Two Balls things meeting: each voice plays once per hit; a shape voice under them still plays its own hits only.
  - An emitter with Fired on plays one note per ball fired (count = `Spawn` events); off, its balls in empty space
    play nothing.
  - Shooter and catcher: Caught on the catcher plays on each catch; on the shooter (its balls caught) too.
  - Heard from naming another shape: hits on the first shape play nothing.
  - By ball: each own ball plays its own note even when the ball is `b` of the hit.
  - Follow sound (`ControlSampler`) agrees with the engine on a ball voice's hits (level above zero after a hit).
- `hit_filter.rs` unit tests: `seen` turns a hit round only for a ball voice; a shape voice refuses `Spawn`,
  `Enter`, `Break`.
- Existing tests unchanged: `bpe-sound` (goldens, follow, particle_sound, text_sound, alloc), `bpe-kit` restarts and
  goldens, `bpe-studio` inspector (`add_slot_lists_only_slots_the_kind_lacks...`).
- Editor tests (`sound_tab`): a Balls thing shows the Sound chip; Enable sound adds the card; the ball moments panel
  shows (Are fired only on an Emitter).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace -- --test-threads=2`.

## As built

Built as planned, plus one thing found while testing: an emitter's round at 0 s is fired while the world is built,
where no log exists, so no `Spawn` is ever logged for it. Logging it would move the physics goldens and change what
the rules' "When fired" counts, so the sound makes those events itself: `hits::start_fires(scene, world)` (from
the emitter rows of the world as built) and `HitCursor::start_with` keeps them, waiting on sample 0 at the start and
after a seek back to 0; `Engine::catch_up` counts them first. Follow sound and the timeline's slot marks read only the
log, so they don't see that first round's fires (noted in `docs/notes/sound.md`).

Other details as built: `bpe_kit::things::balls::KIND` is new (the kind's id was a literal); `hits::touches` keeps the
Hits sidechain and the test tone on hits, notes and moments; Follow sound now reads a Particles voice's moments too
(it read only hits and notes before, unlike the engine). Tests: `crates/bpe-sound/tests/ball_sound.rs` (9 tests),
`hit_filter.rs::a_balls_sound_sees_a_hit_from_its_own_ball`, the Sound tab's
`balls_and_emitters_have_a_sound_tab_with_their_balls_moments`.
