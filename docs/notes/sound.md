# Sound crates in detail (bpe-dsp, bpe-sound)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.
The editor's sound panels are in `editor.md`; the design is `docs/superpowers/specs/2026-11-04-sound-system-design.md`.

- `bpe-dsp`: sound building blocks at 48 kHz in 64-frame blocks (`SAMPLE_RATE`, `BLOCK`): dB helpers, a
  `Smoother`, `FlushGuard` (denormals off), polyphase IIR halfband oversamplers (2/4/8×), a 4× true-peak detector,
  the two-stage lookahead `TruePeakLimiter` (3.4 ms) and an EBU R128 `LoudnessMeter`. For the synth: `warp.rs` (the
  five waves and ten Warp modes, drawn plain; the editor's wave view uses `warp::draw`), `tables.rs` (band-limited
  wavetables: a `TableSet` per wave and Warp mode, `WARP_STEPS` steps × `MIPS` copies of 16384..1024 samples, baked on
  first use into the shared `WaveBank`; `octaves`/`mips_at` pick the copies, crossfading only over the top quarter
  of an octave, `XFADE_FROM`), `osc.rs` (the unison oscillator: up to 16 copies, Detune/Spread/Blend, f64 phase
  between blocks and u32 fixed-point inside one, 4-point Lagrange four samples at a time), `noise.rs` (seeded
  white and pink) and `envelope.rs` (DAHDSR with a curve per segment). For the chain layers: `filter/` (`StereoFilter`:
  Cytomic SVF low/high/band pass, notch, peak and shelves at 12 or 24 dB, a ZDF ladder, a comb and a three-band
  formant, coefficients glided over a block; `response_db` draws the real curve), `shaper.rs` (`Shaper`: the ten
  distortion curves through a `StereoOversampler`, four sub-samples at a time in `f32x4`, first-order antiderivative
  antialiasing on the cornered curves at 2× and up, bias taken back out, a 5 Hz DC blocker and the Tone tilt;
  `transfer` draws the curve) and `pm_delay.rs` (`PmDelay`: FM and PD as a moving Hermite delay of at most 2 ms,
  `am`, `ring`). Bus effects in `fx/`: `eq.rs` (`Eq8`, 8
  bands, `band_response_db` for the editor's curve), `compressor.rs` (`Key::SelfInput/External/Hits`),
  `delay.rs` (Standard/Ping-pong/Multi-tap, Digital/Analog/Tape), `reverb_plate.rs`, `reverb_hall.rs`,
  `reverb_spring.rs` (shared parts in `reverb_common.rs`), `modfx.rs` (Chorus, Phaser, Flanger), `width.rs` and
  `analyser.rs` (256 log bins).
- `bpe-sound`: the sound engine. `Engine` runs its own `Playback` of the physics and renders 64-frame blocks with
  every hit on its exact sample (`Event.at` is where in the step a hit happened, never hashed; `hits.rs`
  `HitCursor`), the voices (`VoiceSource`, built per scene by `SoundPatch::build`; `swap_patch` keeps ringing notes
  via `adopt`) and the `Master` (input, output, true-peak ceiling -1 dBTP, release) from the scene's `sound.mixer`
  card (`bpe-kit/src/sound/mixer.rs`; `ensure_mixer` puts it on every scene). `Engine::seek` goes back
  `Engine::warm_up` seconds (sized to the patch's longest tail, below) and renders silently so tails ring. `render` never allocates (a counting-allocator test).
  `TempoMap` reads the scene's Tempo / Beats in a bar / Beat note. `offline.rs`: `render_to_wav` (streamed float
  WAV) and `has_sound`. The `test-tone` feature gives a thing tagged `test-tone` (or `test-tone-decay=<s>`) a test
  blip voice; tests use it, the program never does. **Shape voice** (part 2): a shape's `sound.voice` card
  (`bpe-kit/src/sound/voice.rs`: `VoiceSettings` with Osc A/B as prefixed `OscSettings` (`a_`/`b_` keys), Playing,
  Notes, Output and `routes`; ADSR layers `sound.adsr` in `adsr.rs`; `starter_voice` is what Enable sound adds;
  the chain layers `chain.modulation`, `chain.filter`, `chain.distortion` and `chain.eq` (8 bands like the bus EQ,
  run per sample with `Eq8::tick`) are in `bpe-kit/src/sound/chain.rs`,
  a Modulation's `source` text being "" (the other osc), "a", "b" or a layer id) becomes a `ShapeVoice`
  (`voices/shape.rs`, built by `patch::shape_voice`): hits pass the `HitFilter` (min strength, retrigger, Heard from),
  `NotePicker` picks a note of slot 0, `Voices` (`alloc.rs`) gives it a slot (Poly up to 16, the oldest fading in 5 ms;
  Mono glides). Every number knob has a place in `knobs::layout` (keys read once a block from `ShapeSpec.keyed`),
  `ModMatrix` (`modmatrix.rs`) applies routes as Serum does: an Amount is a share of the knob's own travel from where it is set, clamped at its ends (Hz knobs turn in ratios, `turns_in_ratios`, the same in the engine and the editor's knob and ring); the one exception is an ADSR on a level knob (Gain, Level, Sub level, Volume, `combine_of(key, from_envelope)`), a VCA per sample (`scale_by_routes`), other routes on level knobs adding dB per sample (`offset_by_routes`), the
  oscillator layer chains (part 3) are `chain_graph.rs` (`ChainGraph`: which layer reads which, by lane; a Modulation
  layer's source is an oscillator raw or the sound after any layer; a tap that would close a circle is a loop and
  reads one sample late, the first layer in card order hearing on time; `loops_of` for the editor's loop mark) and
  `chain.rs` (`ChainSpec` built per patch, `VoiceChain` per note: Osc → Gain → layers → Level → Pan, Solo cuts a lane
  at its layer, a tapped oscillator that is off still runs unheard, `adopt` keeps ringing filters by layer id). Chain
  knobs have fixed places (`knobs::chain(place, key)`, 38 keys: 14 shared then 24 EQ band numbers from `EQ_BANDS_AT`, `MAX_CHAIN` 32 layers a card) so keys and routes reach
  them. The Master's Input/Output keys are read every block too (`MasterKeys`). `Engine::audition`, `stop_notes` and
  `render_paused` (stopped: no hits or physics) play
  keys clicked on the Sound tab; `Meters` also carries a level per shape (`write_shape`/`take_shape` by thing id) and the notes it
  sounds as a 128-bit mask (`write_notes`/`take_notes`, from `VoiceSource::sounding_notes`; the piano roll lights them).
  Sound goldens: `tests/golden-sound*.txt` (an FNV hash of the starter patch; `golden-sound-chain*` a patch with
  layers on both oscillators), blessed like the physics goldens. `tests/ab_clips.rs` (ignored) renders the FM/PD A/B
  clips.
  **Notes** (part 5): the key/plays/slot-change settings and their enums are in `bpe-kit/src/sound/notes.rs`
  (`Root`, `Scale`, `Plays`, `Change`, `SlotOrder`, `MAX_SLOTS` 16); `notes_jump` ("Jump to slot") is the one
  `hold` setting (`Flags.hold`: `Scene::set_key` makes its keys `Ease::Hold`). `bpe-sound/src/music/` is pure
  theory: `scale.rs` (`in_scale`, `move_into_key`, never drops a note), `chord_name.rs` (FL names, slash chords),
  `voicing.rs` (`voice_progression`, least movement) and `presets.rs` (`PRESETS`, 40 in 8 `Mood`s, `realise`).
  `voices/pick.rs` holds the play orders (`AnyPick`, pure in the hit step except Random walk; `ChordPick` strums),
  `voices/slots.rs` the `SlotClock` (`begin_block`/`until`/`hit`/`end_block`; tempo and time lines from the
  absolute sample; a change on a hit's own sample comes first) and `slot_changes`/`slot_change_times` for the
  timeline. `Engine::seek` calls `VoiceSource::catch_up` with every hit before the warm-up so slots and orders
  are exact after a seek (and `swap_patch` does for a voice new mid-clip); `Meters::write_slot`/`shape_slot` report the slot playing. By side uses
  `things::shape::side_on` (no allocation; the outline is made at patch build).
  **Buses** (part 6): the mixer card holds Bus 1..3 (`bus1_…` keys: name, on, fader, pan, mute, solo, send1..3;
  Bus 1 on, 2 and 3 off) and their effects as layers (`bpe-kit/src/sound/fx.rs`, `fx.eq` … `fx.width`, section
  Effect, `Layer.lane` = bus 0..2; `fx.filter`/`fx.distortion` reuse the chain's settings without Key track and Solo). `buses.rs`: `Buses` runs them in send order (post-fader sends; a send that
  would loop is dropped, `would_loop`), solo/mute/on fades, and lines up latency with `Lag` (`lag.rs`); the
  engine takes `latency()` back so hits stay on their samples. A shape's main bus and sends are its
  `ShapeRoute` (`out_bus`, `out_send1..3`). `fx_chain.rs`: `FxChain` per bus (Mix crossfade, bypass fade,
  keyed numbers via `Keyed`, `adopt` on a patch swap). `Meters` has bus, effect and EQ-analyser readings.
  Export extras (`offline.rs` `render_sound` with `Extras`): `stems` (32-bit float WAV per bus + Master,
  `stem_paths`) and `midi` (`midi.rs`: `NoteLog` from `Engine::record_notes`, `write_midi`, format 1, PPQ
  960).
  **Modulators** (part 4): kit layers `sound.distance`, `sound.lfo`, `sound.hit` (`bpe-kit/src/sound/mods.rs`,
  `MOD_KINDS`, `add_modulator`, at most `MAX_OF_EACH` 8 of a kind, `turn_on_distance`) beside the ADSRs;
  `mods/` holds `LfoMod`, `DistanceMod` (`distance_value`, Centre/Edge, Nearest/Each note's ball/Average/How
  many) and `HitMod` (`hit_values`: strength, place on edge, side, ball number, random). Shared ones run once a
  block in `ModsSpec.order` (`mods/order.rs`), per-note ones live in each note; every number knob of the card is a
  route target (`patch::routable`). An LFO's shape is a drawn curve (`LfoShape::Drawn`, `curve`, `LfoPreset`;
  `bpe_dsp::lfo::curve_at` with `bend_at`; `lfo_curve.rs` edits it: add/move/delete points, bends, Step/Ramp paint,
  flips) or Random/Smooth random; `once` holds the end; `Meters::mod_position` gives the editor's playhead. `Meters` carries each shape's modulator values and last hits
  (`mod_value`, `recent_hits`). `VoiceSettings.play_when` (`PlayWhen`: On hit / Always / In range of
  `play_zone`) makes drones (`ShapeVoice::hold`: held notes are never retriggered by hits; only notes in
  `ShapeVoice.held` count as drone notes; `BlockCx.playing` is false while stopped, when drones wait and clicked
  keys (gate at most 0.4 s, their own voice beside a drone) sound alone). Switched-off modulators' routes are
  left out at patch build. **Follow sound**: `control.rs` `ControlSampler` (built from
  `patch::shape_specs`, no wavetables) works modulators, `Level` and `Bus(n)` out from the log's accepted hits
  and the world at a moment (`bpe_dsp::level_at` closed-form envelopes) with no audio; `FollowSound` is its
  `SoundMaker` and `bpe_sound::kit()` the kit with it (the editor and the renderer use it, so physics follows
  sound everywhere). `tests/follow.rs` checks it against the engine (`Engine::voice_time`).

**Rule changes (W13)**: `ruled.rs` folds a rule's Change setting on a Sound or Mixer number at the block's time with
the physics' `Change` maths (`ruled::number`, `at`), from the engine's own physics world (`world.rules()`), with no
allocation: `ShapeSpec.knob_keys` (sorted at patch build) maps keys to knob places for `voices/shape.rs::rule_knobs`
(each block, after the keys); `ShapeRoute.ruled` (Sends), `BusKeys.ruled` (Fader, Pan, Sends), `Master.ruled`
(Input, Output) and `fx_chain::Keyed.ruled` (every keyframed number of a bus effect, `Keyed::due` when a change is
on) hold their `ChangeKey`s. `ControlSampler` folds the same (`knobs_at`, a bus's fader), so Follow sound hears a
rule's Volume. `voice_rule_keys`/`mixer_rule_keys` list what a rule reaches (the menu shows only these). Switches,
choices and the note slots' timing stay as set (the patch is built from them). Tests: `tests/rule_changes.rs`, a
rules section in `tests/alloc.rs`.

## Sound engine fixes (W9, 2026-11-08)

Plan: `docs/superpowers/plans/2026-11-08-sound-engine-fixes.md`. None of it moves a golden: every new setting is
off at its default.

- **Only heard hits queue** (A4): `SoundPatch::heard()` (voice things plus `BusesSpec::hit_keys`, the shapes a Hits
  sidechain keys on) feeds `HitCursor::read`/`seek`, which drop hits on anything else. `swap_patch` re-seeks the
  cursor when the heard set changes.
- **Rebuild only for sound edits** (A12): `patch_fingerprint(scene, resolved)` hashes the scene keys a patch reads
  (`PATCH_SCENE_KEYS`: tempo, time signature, First beat at, width, duration), the Mixer card, every Sound card and
  the things that have one, keys and pulses on them, a By side shape's outline and, when a Light modulator listens,
  the lights (`sensing::fingerprint`). The editor's `patch_print` sends a Patch only when it changes
  (`tests/patch_print.rs`).
- **Master** (A8): `Master::process` applies Output's boost before the true-peak limiter and its cut after, so
  turning Output up never passes the ceiling. Stems are 32-bit float (`WavOut`). `master_end_fade` ("End fade",
  0..30 s, Master knob) fades the mix, the buses and the stems to silence at the clip's end with a half-cosine
  (`end_fade_gain`; `Engine::set_end` is the export's length, else the scene's Duration).
- **Hit strength** (A11): `play_full_at` ("Full strength at", px/s, default 1500 = `STRONG_HIT`) and `play_curve`
  ("Strength curve", -100..100) on Playing make a `StrengthCurve` (`voices/hit_filter.rs`); the default is bit for
  bit the old straight line. The filter, the Hit modulator, Follow sound, slot changes and a Hits sidechain all use
  the hit's own curve.
- **First beat at** (A11): `SceneSettings.first_beat` (seconds, Sound group, also a field beside Tempo in the Mixer)
  moves bar 1. `TempoMap::first`/`beat_at`/`bar_at` count from it; so do the timeline grid and snap, By tempo slot
  changes (`slots.rs`, nothing changes before it), synced free LFOs (`lfo_origin`; played once, they wait at the
  start), `World.first_beat` and everything that reads beats there (scene beat pulses, rules' beat moments, rig
  patterns, light scenes In turn, particle beats, Text live words, show and moves, gradient Step/Breathe on beat).
- **Light modulator off the sound path** (A2): `VoiceSource::prepare(world, log)`, called by
  `Engine::step_physics` after the physics moves, fills each shape voice's `LightTable` (the light and shadow on
  the balls its Light layers read, once a step); `LightMod` reads it through `light_value_from` and asks the probe
  only for a ball it lacks. Follow sound still reads the probe (`tests/light_mod.rs` checks they agree).
- **Warm-up** (A3): `SoundPatch::warm_up()` is the longest voice note (`VoiceSource::warm_up`: each envelope's delay,
  the longer of its gate and attack + hold + decay, then release, at the largest its keys set, plus 0.2 s of
  strum) plus the buses' tails (`FxChain::tail`: reverb pre-delay + Decay, a delay's time × repeats to -60 dB; a
  frozen reverb or full feedback is endless) plus 0.25 s, from `MIN_WARM_UP` 0.5 s (`HELD_WARM_UP` 4 s when a voice
  `holds()`: a drone or a gate until the next hit) to `MAX_WARM_UP` 30 s. `Engine::seek_until(time, stop)` asks
  `stop` between warm-up blocks. In the editor (`sound_job.rs`): a Rebuild builds and warms the new engine on a
  "ball-studio sound warm-up" thread (`Build`: a cancel flag for a newer rebuild, the latest seek as its target;
  patches meanwhile wait in `pending_patch`), so keys and pause still answer; after Pause the thread warms up at the
  paused time once idle (`prewarm`, giving up at the next command), and a seek to where the engine already is and
  warm (`warm_at`) is skipped, so Play starts at once.
- **Speakers** (A1, A7; `audio_out.rs`): every device frame waits in a 96-frame (2 ms) line in the `Feeder`; when the
  ring runs dry the line drains, fading out, then it is silent until it fills and fades in again. A clear fades out
  over the line too. `Shared` has `playing` (set by the sound thread, not while it waits on purpose), `underruns`
  (dry while playing, or a cpal Xrun) and the delay to the ears (cpal's playback - callback time, half the buffer,
  the line, the resampler; smoothed) plus the listener's speaker delay. `SoundJob::clock` subtracts both. The speaker
  delay (0..500 ms) is set from the speaker button's right-click menu and saved as a third word in `listening.txt`
  (older files still load). The status line says "Sound can't keep up: try fewer voices or effects." when dropouts
  grow. `thread_priority.rs` raises the sound thread and lowers the aiming and particle threads on Windows
  (`windows-sys` `SetThreadPriority`; elsewhere nothing).

## Balls and Emitters make sound (W10, 2026-11-08)

Plan: `docs/superpowers/plans/2026-11-08-balls-make-sound.md`. No golden moves: a scene without a Sound card on a
Balls or Emitter thing hears exactly what it heard before.

- **The card**: `VoiceKind::fits` takes Balls (`things::balls::KIND`, new) and Emitter too
  (`bpe_kit::sound::hears_own_balls(kind)` names the two); both kinds list `SlotId::Sound` (not in their default
  slots). Its balls are what sound. `VoiceSettings` reuses the Particles' `moments_hit_ball` / `moments_hit_shape`
  (on) and adds `moments_fire`, `moments_catch`, `moments_break` (off; "Balls fired", "Balls caught", "Balls broken").
- **Events**: `hits::plays` now also takes `Spawn`, `Enter`, `Break`; `hits::touches` is the old set (Hit, Note,
  Moment), used by a Hits sidechain and the test tone so they don't change. Only heard things' events queue, as
  before.
- **Filter** (`voices/hit_filter.rs`): `BallMoments` (`of`, `of_thing`) on `ShapeSpec.balls` and
  `HitFilter.balls` (`hearing`). `HitFilter::seen` turns a hit round for a ball voice so `a` is its own ball and `b`
  what that ball met (the normal flipped); `take_hits`, `catch_up`, Follow sound and `slot_changes` read the seen
  event, so the retrigger guard, By ball, Each note's ball and By letter (`b` a Text) work from its own ball. `accept`
  (via `hears`): a shape takes only `Hit` and `Moment` with `b` = it (as before); a ball voice takes a `Hit` from its
  own ball (Hit a ball when `b.thing` is in `world.bodies`, else Hit a shape; Heard from names `b`), `Spawn` with `b`
  = it, `Enter` with it on either side, `Break` with `a` = it. Fires, catches and breaks go through the same strength
  curve (launch speed, catch speed, the breaking hit's strength).
- **Hit modulator**: Place and Side read `b`'s thing for a `Hit` (`mods/hit.rs::hit_values`); on a shape that is
  the shape, so nothing changes there.
- **The round at 0 s**: an emitter's first round is there from the first frame and no step logs it, so
  `hits::start_fires(scene, world)` makes `Spawn` events (step 0, from the built world's emitter rows) that
  `HitCursor::start_with` keeps: they wait first on sample 0 (round 1) at the start and after any seek back to 0, and
  `Engine::catch_up` counts them first. Not seen by Follow sound or the timeline's slot marks (they read the log).
- **Follow sound** (`control.rs`) reads every event that `plays` on the voice's thing (a rule's Note only on `b`),
  through the filter; a Particles voice's moments now count there too, as in the engine. The editor's Follow sound
  menu (`bpe-studio/src/follow.rs::menu`) lists Balls and Emitter sounds beside shapes and Texts.
- A Break a rule makes without a hit behind it (When time, say) carries speed 0, so it plays at strength 0: heard
  at the defaults, silent once Follow hit is up (it still takes a voice and moves the play order).
- Not yet: hovering a bus in the Mixer lights only the shapes and Texts that play into it (`preview.rs`, walls), not
  a Balls or Emitter thing; a Hits sidechain and a rule's Play a note still name shapes and Texts only.
- `slot_change_times` reads every playing event but Note (the filter keeps the thing's own).
- **Editor**: `inspector::sounds` takes Balls and Emitter (Settings / Sound chips); `+ Add card` lists only card-list
  slots (Sound and Mixer come from their own tabs). Sound tab: `moments::is_balls`, `show_balls` ("Plays when its
  balls": Hit a shape, Hit a ball, Are fired (Emitter), Catch / Are caught, Break); Heard from reads "Hits on".
  Distance on a ball voice measures from the thing's place (Centre; Edge reads nothing), By side plays side 0, as on
  a Text.
- Tests: `bpe-sound/tests/ball_sound.rs`, `hit_filter.rs` (`a_balls_sound_sees_a_hit_from_its_own_ball`), the Sound
  tab's `balls_and_emitters_have_a_sound_tab_with_their_balls_moments`.

## Samples, backing track, saved sounds, MIDI import (W11, 2026-11-08)

Plan and design: `docs/superpowers/plans/2026-11-08-producer-features.md`. No golden moves: a scene that keeps no
sound files hears exactly what it heard before.

- **Sound files in the scene** (`bpe-core/src/scene_audio.rs`): `Scene.audio` (name → `SceneAudio`, base64 bytes,
  left out when empty), `add_audio` (same bytes keep one name; a clash gets "name (2)"), `bring_audio` (another
  scene's files in, returning the renames), `sound_clip(card)`, and save prunes audio no `a_sample` / `b_sample` /
  `backing` names (`AUDIO_KEYS`, `is_audio_key`). The W5 things clipboard (`clip.rs`) carries the audio its things
  name, as it carries fonts and pictures.
- **Reading** (`bpe-sound/src/audio.rs`): `probe` (fast, the editor's check), `decode` (symphonia: WAV, FLAC, MP3,
  OGG, AIFF; gapless; rubato `FftFixedIn` to 48 kHz with its delay trimmed; mono doubled; 200 MB cap),
  `guess_root("Piano C4.wav")` (C5 = 60), `sine_wav` (tests). `SampleBank`: `get` decodes there and then (export,
  tests), `ready` hands back a finished decode or starts one on a "sound file decode" thread, keyed by the bytes'
  `Arc` through a `Weak`; `decoded()` counts finished decodes and `busy()` says one is running (the editor's
  `follow_bakes` rebuilds the patch on either, as for `WaveBank::baked`).
- **Sampler** (`bpe-dsp/src/sampler.rs`): `SampleData` (both channels, peaks per 64 frames, mip levels by a 65-tap
  halfband for samples up to 30 s, picked so a step stays at most 2^0.6, about a fifth), `SamplerVoice` (4-point Hermite, Start,
  Loop Off / Forward / Ping-pong between Loop start and end, an equal-power Loop fade on Forward, Reverse, a wait
  before the note's first sample). `Wave::Sample` on Osc A or B (`OscSettings` `sample`, `sample_root`,
  `sample_follow`, `sample_start`, `sample_loop`, `sample_loop_start/end`, `sample_loop_fade`, `sample_reverse`;
  none keyframed: read when the patch is built). `patch.rs::osc_spec` fills `OscSpec.sample` from `Tables::sample`
  (Bake: `get`; Ready: `ready`, the patch pending while it decodes; a missing or unreadable file is silence, never
  pending). `voices/shape.rs::render_note` steps the sample at 2^((pitch − root)/12) with Follow notes on, else
  2^((pitch − note)/12) (Octave, Semi, Cents, pitch modulation still move it). Warp, Unison, Sub and Phase don't
  apply to a sample (their `when` says so).
- **Backing track** (Mixer `backing`, `backing_on`, `backing_at` −600..600 s, `backing_level` dB): `patch.rs::Backing`
  (start in samples, gain), added by `Engine::render_voices` into the master input after the voices and buses, at
  sound time, so it goes through the Master (Input, limiter, ceiling, end fade), seeks and exports with the mix; in
  the Master stem, not the bus stems. `has_sound` is true for a scene whose only sound is its backing track.
- **Saved sounds** (`.ballsound`): RON `(format: 1, card, audio)` via `sound_file_text` / `read_sound_file` (plain
  errors for a file that isn't one, or one from a newer version).
- **MIDI import** (`music/midi_import.rs`, midly): notes starting within a 32nd note are a chord; distinct chords fill
  up to 16 slots in first-played order (the rest counted in `left_out`; drums on channel 10 skipped); the first tempo
  (clamped 20..300) and time signature come across; evenly spaced chords on whole beats or bars give By tempo (Every
  N bars or beats, Forward), anything else Only on keyframes with a Jump to slot key per chord (seconds through the
  file's tempo map); the keyboard range is widened to show every note.
- Tests: `bpe-dsp` sampler unit tests; `bpe-sound/tests/sampler.rs` (root, follow off, missing, loop, backing at,
  negative start, level, off, seek), the FLAC/MP3/OGG/AIFF fixtures in `tests/data/`, `midi_import` unit tests,
  `alloc.rs` (sample voices and a backing track allocate nothing).

**Small links (W12, 2026-11-08).** `ControlSampler` hears every sound event the engine plays (`hits::plays`, so a
Particles thing's moments too), and a Particles thing whose sound a setting follows steps in the main world. The
voice's, Health's and the rest's whose-hits setting is labelled **Hits from**; the Distance and Light modulators'
measuring setting is **Which balls**. Phase units are "°".
