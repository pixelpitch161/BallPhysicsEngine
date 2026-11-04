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
