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
