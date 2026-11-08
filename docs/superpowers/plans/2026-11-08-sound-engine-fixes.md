# W9: sound engine fixes

Workstream W9 of the 2026-11-08 improvement review ("Ball Studio code review: what to improve", section Sound,
findings A1, A2, A3, A4, A7, A8, A11, A12). Aedan picked everything but the Size section on 2026-11-08, so the
review's suggested fixes are the approved design; this plan states how each is built. W10 (balls and emitters make
sound) waits on this one.

Rule for the whole part: with every new setting at its default, the sound goldens (`crates/bpe-sound/tests/golden-
sound*.txt`) and the physics goldens must not move. Each item below says which tests show it works.

## A4: queue only hits that can sound

- `Engine` keeps `heard: Vec<ThingId>` (sorted): every voice's thing, plus every shape a bus compressor keys on
  (`Sidechain::Hits`), from `SoundPatch::heard()` (new: `BusesSpec::hit_keys()` lists the keyed shapes).
- `HitCursor::read` takes a filter: an event waits only if `a.thing` or `b.thing` is heard. Ball-on-ball hits and
  hits on silent things never enter the queue, so they can't fill the 1,024-a-block limit.
- `swap_patch` with a different heard set re-reads the cursor from the voices' place (`cursor.seek(log,
  voice_pos)`), so a voice turned on mid-play gets the hits already read. `catch_up` already filters by voice.
- Test (`tests/engine.rs`): a scene of many balls hitting each other plus one shape with a test tone: with more
  ball-ball hits in a block than the limit, the shape's hit still sounds on its own sample. Goldens unchanged.

## A12: rebuild the sound patch only when the sound changed

- `bpe_sound::patch_fingerprint(scene, resolved) -> u64` hashes exactly what `SoundPatch::build` reads: the scene's
  Tempo, Beats in a bar, Beat note, First beat at (A11), Width and Duration; the Mixer card (scene and resolved);
  every thing's id, kind, enabled flag, tags and its Sound card (scene and resolved); the shape's own settings when
  its sound plays By side (it reads the outline); every track and pulse on a Sound or Mixer address; and, when a
  Light modulator is on, `sensing::fingerprint` and whether the scene has a Lighting card.
- The editor keeps `patch_print`; `scene_changed` sends a patch only when it moved (a rebuild still sends the whole
  scene). The wavetable bake path keeps sending one.
- Test: a colour drag (ball look, background) keeps the print; a sound knob, a mixer knob, a key on a sound knob, the
  tempo and a lamp (with a Light modulator) each change it. An editor test checks a colour edit sends no `patch`.

## A8: the ceiling holds, float stems, end fade

- `Master::process`: Output's boost goes before the limiter and its cut after it (output gain `o`: `max(o, 1)` before,
  `min(o, 1)` after). A peak limiter is scale-invariant, so boosting then limiting at the ceiling equals limiting
  lower and boosting; the result never passes the ceiling, and Output at 0 dB or below works exactly as before
  (goldens unchanged).
- Stems are 32-bit float WAVs (`WavOut::create` + `write`), so nothing is clamped at full scale. The 24-bit writer
  goes if nothing else uses it. Help text in the export window says float.
- End fade: a Mixer setting `master_end_fade` (seconds, 0..30, default 0, group Master, "Fade out at the end"). The
  engine knows where the clip ends (`Engine::set_end(seconds)`; the editor and `render_sound` set it, the editor
  from the scene's Duration, the export from its length) and scales the mix, and every stem, by a smooth (cosine)
  fall over the last `end_fade` seconds, after the master. 0 is off: nothing changes.
- Tests: `tests/offline.rs` a loud patch with Output +12 dB stays under -1 dBTP (true-peak meter); a stem above full
  scale keeps its level (float); a 1 s end fade ends in silence and the second before it is untouched.

## A11: hit strength curve, first beat

- Two new Playing knobs on the voice card: `play_full_at` ("Full strength at", px/s, 50..10000, span 100..5000,
  default 1500 = today's `STRONG_HIT`) and `play_curve` ("Strength curve", -100..100, default 0 = straight). They
  join `knobs::SHAPE_KEYS` so keys and routes reach them. `StrengthCurve { full_at, curve }` with `of(speed)` =
  `bend(speed / full_at clamped 0..1, curve/100)` (the modulators' `bend`). `HitFilter` holds one (set each block
  with min strength and retrigger), `hit_values` takes it (Hit modulator's Strength), Follow sound (`control.rs`) the
  same, and a bus compressor keyed on hits uses its keyed shape's curve. Defaults give exactly `strength()`.
- `SceneSettings.first_beat` ("First beat at", seconds, 0..600, span 0..10, default 0, group Sound, next to Tempo).
  `TempoMap` carries it (`first()`); `beat_at`/`bar_at` count from it. Every beat-counting place shifts by it: the
  slot clock (By tempo lines), LFO free-run phase when synced, the timeline grid and snapping, the scene's beat
  uniform (`scene::draw::beat_at`), `World.first_beat` (set by the scene sim) for particles born on the beat, Text
  moves, live words and shows, rules When on the beat, light-scene and rig patterns (`clock_in_time`), arcs on the
  beat, gradient Step on the beat and Breathe. Before the first beat the grid runs on backwards (negative beats); a
  count that can't go below 0 (slot index, bar numbers) clamps there.
- Tests: `tempo.rs` maths with an offset; slot By tempo changes land on offset bar lines; scene `beat_at` with an
  offset; the timeline draws its first bar line at the offset; `StrengthCurve` defaults equal `strength`, a lower
  Full at reaches 1 sooner, curves bend. Goldens unchanged at the defaults.

## A2: the Light modulator off the sound path

- `LightTable` (in `mods/light.rs`): the light and shadow on every alive ball a voice's Light modulators may read,
  at one physics moment (`(run, step, logged)`), sorted by `(thing, row)`. `VoiceSource::prepare(world, log)` (new,
  default no-op) fills it; `ShapeVoice::prepare` does when the voice has Light layers and a probe: the balls each
  shared Light hears, and for Each note's own ball the balls the voice hears from (`play_heard_from`).
- `Engine::step_physics` (which may allocate) calls `prepare` on every voice after the physics moves, and `seek`
  after its catch-up, so the probe's heavy work (animating the lights, building lamps, shadow rays) happens there,
  once a step, never inside `render_prepared`.
- `light_value` reads balls through a reader closure; `LightMod::next_block` reads the table when it is for the
  world's moment and has the ball, else the probe (Follow sound's `ControlSampler` keeps reading the probe).
- Test: `tests/alloc.rs` gets a lit scene with a shared and a per-note Light modulator: `render_prepared` allocates
  nothing. `light_mod.rs` tests still pass (same values).

## A3: shorter warm-up, none on resume, built aside

- Warm-up (`Engine::seek`'s silent render before the landing point) is sized to the patch:
  `SoundPatch::warm_up()` = the longest tail: per voice, each envelope's delay + attack + hold + decay + gate
  + release (largest of base and keys), the bus effects' tails (reverb Decay, delay time × repeats to -60 dB from its
  Feedback, capped), plus 0.25 s. At least `MIN_WARM_UP` 0.5 s; a held gate (Until next hit with Sustain) or a drone
  (Plays Always / In range) needs at least `HELD_WARM_UP` 4 s (the old fixed pre-roll); at most `MAX_WARM_UP` 30 s.
  `VoiceSource::warm_up()` lets the test tone report its decay. `PRE_ROLL` goes; `Engine::warm_up()` reports it.
- Resume: after Pause, while nothing else is asked, the sound thread warms the engine at the paused time in advance.
  When Play then comes for that time, the seek is skipped and sound starts at once. An audition, a rebuild or a
  seek elsewhere cancels the warm state (an audition first stops the notes so a clicked key still starts from quiet).
- Helper thread: a Rebuild (a physics edit) builds the new engine and does its warm-up on a "ball-studio sound
  warm-up" thread, so the sound thread keeps answering (pause, keys, newer edits). A newer Rebuild or Seek makes the
  one in flight stale (a generation number; the engine's warm-up loop checks a cancel flag between blocks and stops
  early). The new engine replaces the old when it lands; Patch commands that came meanwhile are applied to it.
- Tests: `warm_up` sizes (short patch ~0.75 s, 10 s release, 20 s reverb, drone floor, cap); a seek with a long reverb
  matches playing straight through (A/B with `render_samples`); `sound_job` tests: Play after Pause at the same time
  clears without a new warm-up; a rebuild lands from the helper and the old engine plays keys meanwhile.

## A1 + A7: speakers on time, dropouts fade and say so

- Feeder output delay line of 96 frames (2 ms): every device frame goes through it, so when the ring runs dry there
  are 2 ms left to fade out over (no click), then silence; when frames come back it fills and fades in over 2 ms.
  A clear (seek) fades out over the delay line too.
- `Shared` gains `playing` (set by the sound thread on Play/Pause), `underruns` (counted when the ring runs dry while
  playing, and on a cpal Xrun), and `delay_frames` (48 kHz frames between handing a frame out and hearing it: cpal's
  `playback - callback` timestamp, half a callback buffer, the delay line and the resampler's delay; smoothed).
- `SoundJob::clock` = base + max(0, played - delay - speaker delay) / 48 kHz. Speaker delay is the editor's own
  listening setting (0..500 ms, default 0, saved in `listening.txt` as a third word), set from a right-click menu
  on the speaker button ("Speaker delay", for Bluetooth headphones).
- The editor shows "Sound can't keep up: try fewer voices or effects" in the status line when `underruns` grew.
- Priority: the sound thread asks for a high priority and the aiming and particle threads for a low one
  (`thread_priority.rs`; Windows `SetThreadPriority` through `windows-sys` 0.61; elsewhere nothing).
- Tests (`audio_out.rs`): a dry ring ramps down over 2 ms, no step; refill ramps up; underruns count only while
  playing; the callback still never allocates; clock subtracts the delay; listening settings round trip with the
  delay (and old two-word files still load).

## Not doing

- No change to the physics, hits on their samples, or what an export sounds like with default settings.
- No new scene file format: new settings are plain keys with defaults (format stays 12).
- No exe build or install.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test --workspace -- --test-threads=2` (background), and the release goldens for bpe-kit and bpe-sound.
- Goldens: none re-blessed (diff against main is empty).
- One Opus review of the whole diff before the PR; CI green before merging.

## As built

- Built as planned; the summary is in `docs/notes/sound.md` ("Sound engine fixes (W9)").
- A2: the probe never allocated in the lit allocation test even before the change; the gain is time, not memory (the
  shadow rays and lamp building now run once a step beside the physics, not inside a block). A Light following each
  note's ball fills the table for every ball, since any ball can hit.
- A3: a reverb's Modulation wobble runs from when the reverb starts, so a seek into a modulated reverb is close but
  not sample-exact (it never was); the A/B test turns Modulation off. A synced LFO played once waits at its start
  until the first beat.
- A1/A7: underruns are not counted while the sound thread holds the ring empty on purpose (a new engine on its way).
- Review fixes: a warm-up that gives up still lands at its time, cold (`Engine::seek_until`), so a key clicked
  meanwhile plays at the playhead; a clicked key first stops tails a warm-up left (`unheard_tails`); a seek, a
  rebuild or Play ends the wish to warm up at the pause, and a sound edit while stopped warms up again; Pause sends
  the editor's own playhead so Play lands on the warmed spot; the LFO phase keeps its old rounding at First beat 0;
  a synced bus Chorus/Phaser/Flanger counts its sweep from the first beat; the sound thread asks for Above normal
  (not Highest) so warm-ups can't starve the window.
- A4's test is at the cursor (`hits.rs`): an engine scene with over a thousand ball-on-ball hits in one step is too
  slow for the test run.
- A11: the timeline grid starts at the first beat (nothing is drawn before it). Known gaps, as for Tempo before:
  First beat at doesn't restart the physics, so a rule on the beat keeps the old beat until the next restart; a Text
  show on the beat counts back to beats before the first one.
