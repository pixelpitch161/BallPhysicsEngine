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
