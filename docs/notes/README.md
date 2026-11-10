# Notes by area

The detailed "as built" notes for each part of Ball Studio. `CLAUDE.md` keeps only the map, the rules and the
pitfalls, with one line per area pointing here. Read the file for the area you touch before you change it.

| File | Area |
| --- | --- |
| `foundations.md` | The 2026-11-03 strip-down, every thing owning its settings, Vary across balls |
| `engine.md` | The crates in detail: settings derive, core (things, cards, layers, resolve, save), runtime (world, steps, collisions, history), gfx (frames, layered balls, shapes, paths), kit (every plugin: balls, shapes, emitter and aim), cli |
| `lighting.md` | Lighting parts 1a to 8: lights, Flat and Stage, surfaces, glass, light field, UV and tags, sensing, lens and Stage camera, bake and caustics, light shows, Effects stack |
| `glass-light.md` | Coloured shadows, prism beams, bent lamp light, mirrors and light pipes, dichroic glass, fire, rainbows |
| `rules.md` | Rules engine and editor, break looks, more events and actions, Health, spectacle actions |
| `gradients.md` | Gradient mixes and gap blends, companions, gradient shapes, movement, the big editor |
| `particles.md` | The Particles thing: engine, air, meeting things, looks, trails/marks/liquid, moving and forming, moments and sound, Huge swarm |
| `text.md` | The Text thing: fonts and layout, live words, letters as walls, letter moves, experimental effects |
| `sound.md` | bpe-dsp and bpe-sound: voices, chains, modulators, notes, buses, Follow sound, export audio |
| `file-map.md` | Where each file is: the folders the giant files were split into (W17), old file names, where colour grading goes |
| `grading.md` | Colour grading: the Grade card, its panels as layers, the GPU grade program, Grade mode, the Histogram |
| `library.md` | The built-in scenes: the library in bpe-kit, its checks, the Scenes window |
| `editor.md` | bpe-studio: design system, panels, inspector cards, Look editor, emitter card, timeline, Mixer, Sound tab |

## Keeping notes

- When a part lands, add its summary (what was built, the names a later thread needs, tests and goldens) to its
  area's file, in the same pull request.
- A new area gets a new file here, a row in this table and one line in `CLAUDE.md`'s area index.
- `CLAUDE.md` changes only for a new area, a new working rule or a new pitfall, so parallel threads don't collide on it.
