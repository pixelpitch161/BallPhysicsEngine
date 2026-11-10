# Built-in scenes part 5: spectacles 34 to 40

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The last seven spectacles for the Scenes window: Fire tornado, Word from chaos, Liquid escape, Letter
smash, Stem battle, Gravity flips every bar and Golden hour. With them the library holds all 40 scenes (14 / 14 /
12). They lean on the Particles thing (flames, embers, a word of specks, liquid), real letters with health and the
text effects, rules on the beat changing gravity, light and the sound, and the Grade card.

**Architecture:** As parts 1 to 4: one file per scene in `bpe-kit/src/library/spectacles/` with `ENTRY` and `build`,
listed in `spectacles/mod.rs` and in `ENTRIES` (`library/mod.rs`) after part 4's, in the spec's order 34 to 40.
`library/build.rs` gains only the helpers these scenes use (clippy fails an unused one). No engine change is planned;
a scene that finds a feature doesn't do what it needs takes the nearest idea that works and says so in As built.

**Tech Stack:** Rust (pinned toolchain), the workspace crates.

**Spec:** `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md` (the tables, "Checks"). Part 4's plan and
its As built: `part4.md`. Handover: `/mnt/project-files/ballphysics/builtin-scenes/part4-handover.md`. Notes:
`docs/notes/library.md` (helpers and every gotcha so far), `particles.md`, `text.md`, `rules.md`, `sound.md`,
`grading.md`.

## Global constraints

- **Goldens unchanged**: no engine change, so no golden line may move and `BLESS_GOLDEN` is not needed.
- Scenes start from `new_studio_scene` through `Lib`; every thing named, related things in a Folder, rules named for
  what they do, no card, layer or rule that does nothing, no files from disk, public-domain or own melodies only.
- Every scene is `SoundLevel::Designed` (`Lib::tone` voices, a reverb on the Mixer), 25 to 40 s (the test checks),
  opens with a hook title, keeps a live number on screen and builds to one payoff. A payoff the physics decides is
  stated in `SceneInfo.payoff` and checked; preview 12 to 15 s; poster after the moment it shows (posters are drawn
  on Aedan's PC, where chaotic moments can shift slightly).
- Every scene still needs at least one Hit (the test); scenes about particles or words keep balls that bounce.
- The physics test builds the whole world, beside things included, in debug: a scene's full-length run should take
  well under a minute there (keep exact-engine particle counts in the low thousands).
- `cargo fmt --all` (width 120), `cargo clippy --workspace --all-targets -- -D warnings`, tests with
  `--test-threads=2`, line-tables-only debug and no incremental (`~/.cargo/config.toml`). `cargo test -p bpe-kit
  --lib` and `cargo test -p bpe-studio scenes_window` before pushing.
- `levels_so_far` becomes (14, 14, 12): the spec's final count.
- The exe is not built or installed.

## Facts found while planning (the engine as it is)

- Particles forces (`things/particles/forces.rs`, `more_forces.rs`) all carry **Amount** (0..1, keyframed and
  pulsed), so a force can be faded in and out by keys or follow a sound. Swirl is curl noise (Strength, Size, Drift);
  Rise lifts and can heat the Air card's air; Follow the air (`force.air`) moves particles with it; Pull to /
  Push away from take A point, Orbit and Falloff; Spring home pulls each speck to its place in a word; Flow field has
  Noise / Spiral / Waves / Vortex.
- The Text look **Particle word** (`text_saying`) is a Text that only spells a Form a word Particles thing (Spring
  home); **Letters break apart** is a walled Text (Bounce off) with Health per letter, shatter at zero, and falling
  balls beside it; text effects `text.fx.glitch` / `text.fx.melt` / `text.fx.split` (Glitch and Split hit harder
  after a hit) are layers on its Look.
- `do.note` plays on a **shape** only (a set note, or The shape's next note, which with Plays Melody walks a melody a
  firing). `when.beat` takes Every Beat / Bar / Bars and an Offset in beats (a fraction works: 0.5 is the off-beat).
- The Mixer has three buses; a rule's Change setting reaches a Sound card's Volume (`out_volume`, dB) and a bus's
  Fader. A voice's Slots change By tempo (every N bars) walks a chord progression in `notes_slots`.
- Scene gravity is the Scene settings' `gravity` (a point, px/s²; down is +y), keyframed and reachable by a rule's
  Change setting. A rule's Multiply compounds per firing.
- Follow sound is `scene.pulses[Address] = Pulse { source: PulseSource::Sound { shape, source: Level }, .. }`; the
  kit alone hears no sound (tests read 0), the editor and export follow it through `bpe_sound::kit()`.
- The Grade card is made by `modules::grade::ensure_grade` (Whole picture, Basic, Tone Curve); other panels are its
  sub-layers (`grade.colour_grading`, `grade.effects`: vignette and grain), layered keys on the card.

## Decisions made without Aedan (he asked for the parts to run to completion; also in As built and the handover)

1. **Fire tornado** (32 s, 100 bpm): a night scene, a fire pit at the bottom middle. Flames (Flame field, Fire and
   smoke look) and embers rise from it, Rise lifting them and a Pull to a point above the pit with Orbit spinning
   them round a column, plus Swirl for turbulence: a twisting funnel of fire. A kick ball lands on a drum pad on
   every beat (`club_night::on_the_beat`); the flames' Rise and the embers' rate **follow the kick's sound**, so the
   fire pulses with the bass. Live "HEAT {n}°" climbs a step on each landing; at the top number a rule lights the
   **firestorm** (stronger orbit and rise, flash, zoom punch, title). Payoff: the Heat counter reaching its top.
2. **Word from chaos** (30 s, 120 bpm): a Particle word "ORDER" of a few thousand specks. They start blown apart
   (born at their places, then a Flow field and Swirl tear them round the screen with Spring home at Amount 0). A
   live countdown of beats to the drop. On the drop (a bar line) Spring home's Amount is keyed up and the chaos
   forces down, so the specks snap into the word with a flash and a chord; after a held bar they are blown apart
   again. Beat balls bounce through the specks pushing them about (the hits). Payoff: RuleFires "The drop" (a
   `when.time` rule on the bar).
3. **Liquid escape** (32 s): a ring with one gap high on its side; an emitter in the middle pours small water drops
   (balls with a liquid look, hitting each other) that pile up in the ring. The level climbs until the drops pour
   out through the gap and fall to the floor. Live "{n}% FULL" (a counter added to on each drop fired). Payoff:
   Counter "Spilled" reaching 1 (the first drop out, a `when.enter` Leaves rule) with flash and title.
4. **Letter smash** (30 s): the Letters break apart look saying "SMASH", every letter with more health; Glitch and
   Colour split hit harder after hits, and a Melt that drips more as the word breaks. Balls fall and bounce into it
   (an emitter keeps a few coming). Each letter plays its spelled note; each break shatters with a zoom punch and a
   step down the counter "LETTERS LEFT". Payoff: RuleFires "The last letter breaks" (counter falls to 0).
5. **Stem battle** (36 s, 110 bpm): four balls in a ring (Drums, Bass, Chords, Melody), each with Health and a label
   riding it. Four speaker shapes under the ring play one part each from beat rules (`do.note` with The shape's next
   note through a Melody list, kick and hat on the drum speaker). When a ball runs out of health it breaks and a
   rule fades its speaker's Volume to silence and shatters the speaker, so its part goes quiet; the last ball wins
   ("MELODY WINS"). Four stems but three buses, so each stem's own Volume is what the rule turns down (the same kind
   of change, a Sound card's setting). Live "{n} PARTS LEFT". Payoff: RuleFires "The winner" (counter falls to 1).
6. **Gravity flips every bar** (32 s, 120 bpm): balls in a box; a rule on every bar multiplies the scene's gravity by
   -1, so the balls fall to the ceiling and back on the bar; the same rule steps the lights (a rig stepping on bars
   with a colour list) and the balls' notes follow a four-chord progression a bar each (Slots change By tempo). Live
   "FLIPS {n}". Payoff: RuleFires "Zero gravity" at the last bars: gravity off, everything floats, slow motion.
7. **Golden hour** (36 s, 72 bpm): a slow, filmic scene. A low warm sun from the right (a Point light far off, warm
   colour, keyframed lower and redder over the scene), long soft shadows; golden balls drop one by one onto gentle
   slopes and roll; each hit plays the next note of Satie's Gymnopédie No. 1 (public domain) on a soft piano tone,
   over a pad. Full colour grading: warm white balance, gentle S curve, orange highlights and teal shadows, vignette
   and grain. Live "SUNSET IN {m:ss}" (a countdown). Payoff: Counter "Notes" reaching the melody's length before the
   end (each hit counts), so the tune always finishes.
8. New `Lib` helpers only as needed: likely `follow_sound(address, shape)` (a pulse on a setting) and a `Tone::Piano`
   (a soft, quickly decaying triangle and sine with a long release). `on_the_beat` moves from `middle/club_night.rs`
   to `build.rs` if two scenes use it.

## Review focus

1. Each idea shows in the pictures: a twisting fire column that pulses, specks snapping into a word, liquid spilling
   through the gap, letters glitching, melting and shattering, stems going silent, gravity flipping, warm graded
   light with long shadows.
2. The rules do what their names say; every Do hangs off the right block; nothing is left doing nothing.
3. Sound is designed, not clipping, and not a wall of notes; beat rules and beat balls are in time.
4. Full-size pictures: no flat frames, nothing off screen that should be seen, text readable, no light leaks.

---

### Task 1: Helpers

**Files:** modify `crates/bpe-kit/src/library/build.rs` (and `middle/club_night.rs` if `on_the_beat` moves).

- [ ] Only what a scene uses (clippy): `follow_sound`, `Tone::Piano`, `on_the_beat` shared.

### Task 2: The seven scenes

**Files:** create `library/spectacles/{fire_tornado, word_from_chaos, liquid_escape, letter_smash, stem_battle,
gravity_flips_every_bar, golden_hour}.rs`; modify `library/spectacles/mod.rs`, `library/mod.rs` (`ENTRIES` 34 to
40), `tests/library.rs` (`levels_so_far` (14, 14, 12)).

- [ ] **Step 1:** Write each scene. Tune with a throwaway probe test (`tests/zz_probe.rs`, deleted before commit)
  that plays a scene, prints hits, rule firings, breaks and counters every half second and draws pictures at chosen
  moments (with the swarm's graphics card on the thread).
- [ ] **Step 2:** Beat scenes: check from the probe's hit times that beat balls land on the beat all the way through.
- [ ] **Step 3:** `cargo test -p bpe-kit --test library -- --test-threads=2`, `cargo test -p bpe-sound --test
  library_sound`, `cargo test -p bpe-studio scenes_window`: PASS.
- [ ] **Step 4:** Full-size look check for each new scene (`LOOK_ONLY=library-<id>`), open the pictures and crops,
  fix what looks wrong.
- [ ] **Step 5: Commit** in batches.

### Task 3: Notes, checks, review, PR

- [ ] Notes: `docs/notes/library.md` (part 5 As built, new helpers, gotchas); this plan's As built.
- [ ] `cargo fmt --all`, clippy, `cargo test -p bpe-kit --lib`, the whole suite once in the background.
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Push, draft PR, subscribe; when green and reviewed, merge.
- [ ] Final handover `/mnt/project-files/ballphysics/builtin-scenes/part5-handover.md`, memory, tell the coordinator
  the built-in scenes work is finished.
