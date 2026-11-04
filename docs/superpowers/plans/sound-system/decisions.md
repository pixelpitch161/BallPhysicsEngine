# Sound system: decisions made without the owner

The owner (Aedan) approved the whole design on 2026-11-04 and then asked (06:49Z) for every part to carry on
without waiting for his approval. Calls made after that are logged here, newest last, so he can check them in
the morning. Spec: `docs/superpowers/specs/2026-11-04-sound-system-design.md`.

| When | Part | Decision | Why |
| --- | --- | --- | --- |
| 06:50Z | all | Sound settings use the existing card + layer machinery: `sound.voice` card on shapes, `sound.bus` x3 and `sound.master` on the scene settings thing; chain layers and modulators are `Slot.layers` with new sections. Scene format 7 -> 8. | Copy/paste, keyframes, undo and save come for free. |
| 06:50Z | all | Follow sound is a new `PulseSource::Sound`; physics-moving settings read the previous step's value. | Reuses pulses; no feedback loop through physics. |
| 06:50Z | 1 | `Event.at` (fraction of the step) is not part of `Log::hash`. | Physics goldens stay put. |
| 06:50Z | 1 | Export renders sound to a temp float WAV first, then muxes (AAC 320k). | Simple and Windows-safe (no named pipes). |
| 06:50Z | 6 | Bus knobs take keyframes, not modulators; EQ starts with a 30 Hz low cut. | A bus has no notes or hits of its own. |
| 06:50Z | 3 | FM/PD A/B clips are rendered and attached to the part 3 PR; the build does not wait on them. | Owner asked for no approval stops tonight. |
| 07:20Z | 1 | One `sound.mixer` card (in a new `SlotId::Mixer`) holds the master and all three buses; plain keys use `_` prefixes (`a_gain`, `bus1_fader`), never `/`, which marks layered keys. | Cards are one per slot id; `/` would be misread as a layer id. |
| 07:20Z | 1 | All shared core changes (slots, layer sections, `Layer.lane`, routes, note slots, Follow sound source, format 8) land in part 1. | Parts 3, 5 and 6 can then build in parallel without core conflicts. |
| 07:20Z | 2 | Wavetables for a (wave, Warp mode) are baked on first use on the sound thread, cached (LRU of 12 sets, ~2 MB each). | Baking all of them up front would take ~500 MB. |
| 07:20Z | 2 | Hit strength = speed / 1500 px/s, capped at 1. | A full-strength hit should be a hard bounce, not a nudge. |
| 07:20Z | 2–3 | Alias tests sweep to 4186 Hz (scientific C8), stricter than FL's C8 (2093 Hz). | The spec's "C8" read the strict way. |
| 07:20Z | 4 | When a setting follows a per-note modulator, it uses that modulator's largest value among the shape's sounding notes. | One setting can only take one value. |
| 07:20Z | 4 | Following a bus's level uses an estimate from its shapes' levels and the fader (no audio needed). | Keeps Follow sound physics-only, so picture export matches. |
| 07:20Z | 5 | Changing a shape's key with notes in place offers "Move notes" (shift by the root interval the nearest way, then snap to the scale) or "Keep notes". | Never drops notes. |
| 07:20Z | 6 | A compressor whose Hits sidechain shape is deleted falls back to Self. | No silent dead compressor. |
| 08:05Z | 1 | Old (format 7) scene files still open and are saved as format 8. | Nothing is lost; no backward compatibility is promised, but it costs nothing here. |
| 08:05Z | 1 | The Master's limiter is two lookahead stages (3.4 ms delay, taken back inside the engine so hits stay on their exact sample). | One stage let peaks through by about 0.2 dB on very hot sound; the ceiling must hold. |
| 08:05Z | 1 | The export's audio asks ffmpeg for AAC 320 kbps at 48 kHz; the test checks the codec, rate and the 320k request rather than the measured bit rate. | AAC spends fewer bits on simple sound (a test tone measures ~69 kbps even at 320k). |
| 08:05Z | 1 | A scene with no sound still exports a silent track, without rendering silence through the engine. | Faster export, same file. |
| 08:05Z | 1 | Keyframes on the Master knobs can be set in part 1 but only take effect from part 2 (when every sound knob takes keyframes). | Keeps part 1's engine simple; part 2 evaluates keyed sound settings. |
| 08:05Z | 1 | The playhead follows the sound the speakers have played; with no sound device (or if it stops working) it follows the wall clock and the editor carries on. | Picture and sound never drift apart, and a PC without speakers still works. |
| 08:05Z | 1 | Turning the speaker off (or the listening volume) only changes what you hear: the clock keeps running and the export is not changed. | As the spec says: listening is not mixing. |
| 08:05Z | 1 | The editor never waits on the sound thread: commands queue without a limit and the thread skips work a later command makes pointless (a drag sends one a frame). | No stutter while dragging physics settings. |
| 08:05Z | 1 | Bars-and-beats lines thin out below 4 px; bar numbers show only where they are 28 px apart; keys snap to beats only while the grid's Snap is on. | Readable at any length and zoom. |
| 08:05Z | 1 | The left panel's Things and Mixer faces each keep their own width (the Mixer opens 420 wide and can be dragged wider for part 6's buses). | Going back restores the tree exactly. |
| 10:10Z | 2 | Every sound knob's keyframes are read once per 64-sample block (1.3 ms), before the envelopes and routes move it; the Master's Input and Output keys now take effect too (Ceiling and Release take no keys). | Keys stay exact to the ear and the sound path never allocates. |
| 10:10Z | 2 | Wavetable copies are 16384 down to 1024 samples (16 to 64 per harmonic), read with 4-point interpolation, and each copy is baked from an 8192-sample drawing. | Shorter copies folded back at -52 dB on narrow pulses and full Sync; the spec asks for -90 dB. Costs about 12 MB of memory per wave. |
| 10:10Z | 2 | The alias test plays steady tones at 17 pitches from 20 Hz to 4186 Hz rather than one sweep, measured against the loudest harmonic. | In a sweep "folded-back sound" can't be told apart from the sweep itself. |
| 10:10Z | 2 | Each pitch reads one wavetable copy, crossfading to the next only over the top quarter of an octave (3 semitones). | Halves the work for most notes and stays alias-free; a glide changes brightness over 3 semitones instead of 12. |
| 10:10Z | 2 | The sub's square reads the band-limited Square table, not a PolyBLEP square. | PolyBLEP folds back well above -90 dB. |
| 10:10Z | 2 | Unison loudness is power-balanced: the middle copy or copies share 1 - Blend, the side copies share Blend. | Turning Unison from 1 to 16 keeps about the same level. |
| 10:10Z | 2 | The oscillator was rewritten for speed: copy by copy, four samples at a time, fixed-point phase within a block. A heavy patch (16+16 unison, sub, both oscillators) runs about 11x real time per note on a 2.1 GHz server core. | The first version ran only 1.9x, too slow for many notes. |
| 10:10Z | 2 | Envelopes on Gain, Level, Sub level and Volume scale the sound every sample, not once a block. | A 2 ms attack is only 1.5 blocks long. |
| 10:10Z | 2 | Pressing Stop stops every note. While stopped, clicking a key on the Sound tab still plays it (no hits, no physics, keys read at the playhead), for about 50 ms after the note goes quiet. | You can try notes without pressing Play; a held tail is cut by Stop (as in part 1). |
| 10:10Z | 2 | The Sound tab edits the first selected shape only, and the wide inspector keeps its own width, separate from the Settings tab's. | Each tab opens at the width you left it. |
| 10:10Z | 2 | Paste sound replaces the whole sound card on the target shape and drops its old sound keyframes. | A pasted sound is the copied sound, exactly. |
| 10:10Z | 2 | Oscillator knobs come in rows (Gain, Level, Warp, Octave, Semi, Cents / Unison, Detune, Spread, Blend, Random phase, Pan / Phase, Restart or Free, Sub), 54 px wide so six fit. Osc B's panel is dimmed while it's off but stays editable. | Everything fits the 760 px wide inspector without scrolling sideways. |
| 10:10Z | 2 | The modulator row shows ADSR 1 and ADSR 2 (Off while ADSR 2's layer is off), then Distance 1 and 2 and LFO 1 and 2 as Off tiles until part 4. | The row has its final shape from the start. |
| 10:10Z | 2 | The Gate has Fixed or Until next hit, in ms or Beats; a beats gate is drawn at the scene's tempo. | As the spec, with the length shown in the unit you picked. |
| 10:10Z | 2 | The keyboard's − and + change how many octaves it shows (it always starts at the same octave). Clicking a key plays it whether it was turned on or off. Mono hides Voices and shows Glide. | Plays what you clicked; Mono has one note at a time. |
| 10:10Z | 2 | A sound keyframe's timeline lane names its part of the voice: "Osc A · Gain", "ADSR 1 · Attack". | Osc A and Osc B both have a Gain. |
| 11:45Z | 6 | The EQ's band shapes are Low cut, Low shelf, Bell, Notch, High shelf and High cut. | The spec said "Cut"; one at each end is what the EQ needs. |
| 11:45Z | 6 | Mix starts at 30 % on the Delay, 25 % on the Reverb and 50 % on Chorus, Phaser and Flanger; the EQ, Compressor, Limiter and Width start at 100 %. Time effects crossfade at equal power, the rest at equal gain. | A new delay or reverb on a bus sounds right straight away; a full-wet reverb on a whole bus rarely does. |
| 11:45Z | 6 | The Multi-tap delay's tap times are percentages of the main time. | Syncing the main time moves every tap with the tempo. |
| 11:45Z | 6 | The bus Limiter reuses the Master's true-peak limiter (lookahead about 1.5 ms). | One limiter to trust; its lookahead is in the plan's range. |
| 11:45Z | 6 | Buses are lined up for latency: a bus with a limiter makes the others wait for it, and the engine takes the total back, so hits land on the same sample with or without effects. | The plan didn't say; without it a limiter on one bus would smear a shape sent to two buses. |
| 11:45Z | 6 | A Hits sidechain hears every hit on its shape (no Min strength or Retrigger), each as hard as its speed. A Bus sidechain hears the other bus's previous block (64 samples late). | Ducking follows the physics, not the shape's note filter; one block late keeps the buses in one fixed order. |
| 11:45Z | 6 | A shape's sends go through the engine's route for the shape, not the voice's knobs. | Keeps part 6 out of the voice code that parts 3 and 5 change. |
| 11:45Z | 6 | The Low cut is off at the bottom of its range and the High cut at the top (Width's Mono below likewise). | No separate on switches. |
| 11:45Z | 6 | A synced "1 bar" uses the scene's real bar length (3/4 is three beats). | What a musician expects. |
| 11:45Z | 6 | A Solo on a bus that is off counts for nothing. | Otherwise turning a bus off could silence the others. |
| 11:45Z | 6 | Stems are 24-bit and clip at full scale; the Master stem is the mix after its limiter. The bus stems add up to the mix before the limiter. | As the plan; 24-bit can't hold more than full scale. |
| 11:45Z | 6 | A change that alters the buses' latency while playing skips or pads a few samples of output. | Rare (adding or removing a limiter); the alternative is rebuilding the whole engine. |
| 11:45Z | 6 | The strips are 4 equal columns at the 420 px Mixer: send knobs read "To 2", "To 3"; each bus's effect list is always five rows tall (it scrolls past five) so the strips line up; the Master column keeps Input, Output, Ceiling and Release, and the loudness readouts and graph sit under all four. Click "Master" for its editor. | Fits 420 px with no sideways scroll. |
| 11:45Z | 6 | The fader runs straight in dB from -60 to +6. Double-click puts it back at 0 dB. | Easy to read and to keyframe. |
| 11:45Z | 6 | The Delay and Reverb editors are their settings plus a small picture (the echoes; the tail); Limiter, Chorus, Phaser, Flanger and Width are their settings, with a correlation or turned-down readout on Width and Limiter. | The views that help most; more can come. |
| 11:45Z | 6 | The EQ curve right-click "Delete" turns the band off and puts it back as it started. Double-click on the curve turns on the off band whose frequency is nearest. | Bands are fixed slots (1..8). |
| 11:45Z | 6 | MIDI note lengths: a Fixed gate gives the note its gate length; Until next hit lasts until the shape's next note (or the clip's end). Velocity is the hit strength scaled to 1..127. The MIDI tempo counts quarter notes as the scene's tempo does. | Matches what the voice plays. |
