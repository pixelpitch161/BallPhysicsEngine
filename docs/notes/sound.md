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
  `PRE_ROLL` (4 s) and renders silently so tails ring. `render` never allocates (a counting-allocator test).
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
  timeline. `Engine::seek` calls `VoiceSource::catch_up` with every hit before the pre-roll so slots and orders
  are exact after a seek (and `swap_patch` does for a voice new mid-clip); `Meters::write_slot`/`shape_slot` report the slot playing. By side uses
  `things::shape::side_on` (no allocation; the outline is made at patch build).
  **Buses** (part 6): the mixer card holds Bus 1..3 (`bus1_…` keys: name, on, fader, pan, mute, solo, send1..3;
  Bus 1 on, 2 and 3 off) and their effects as layers (`bpe-kit/src/sound/fx.rs`, `fx.eq` … `fx.width`, section
  Effect, `Layer.lane` = bus 0..2; `fx.filter`/`fx.distortion` reuse the chain's settings without Key track and Solo). `buses.rs`: `Buses` runs them in send order (post-fader sends; a send that
  would loop is dropped, `would_loop`), solo/mute/on fades, and lines up latency with `Lag` (`lag.rs`); the
  engine takes `latency()` back so hits stay on their samples. A shape's main bus and sends are its
  `ShapeRoute` (`out_bus`, `out_send1..3`). `fx_chain.rs`: `FxChain` per bus (Mix crossfade, bypass fade,
  keyed numbers via `Keyed`, `adopt` on a patch swap). `Meters` has bus, effect and EQ-analyser readings.
  Export extras (`offline.rs` `render_sound` with `Extras`): `stems` (24-bit WAV per bus + Master,
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

**Small links (W12, 2026-11-08).** `ControlSampler` hears every sound event the engine plays (`hits::plays`, so a
Particles thing's moments too), and a Particles thing whose sound a setting follows steps in the main world. The
voice's, Health's and the rest's whose-hits setting is labelled **Hits from**; the Distance and Light modulators'
measuring setting is **Which balls**. Phase units are "°".
