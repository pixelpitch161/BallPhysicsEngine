# Built-in scenes part 4: in-between 25 to 28 and spectacles 29 to 33

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Nine more scenes for the Scenes window: the last four in-between scenes (Stained glass, Prism rainbow,
Club night, Splash glass) and the first five spectacles (The simulation is the song, Escape on the drop, One to a
million, Diamond fire, Mirror maze). They lean on lighting and glass, light rigs, liquid particles and the Huge swarm.

**Architecture:** As parts 1 to 3: one file per scene with `ENTRY` and `build`. The in-between ones go in
`bpe-kit/src/library/middle/`, the spectacles in a new `bpe-kit/src/library/spectacles/` (the spec's example path
`library/spectacles/diamond_fire.rs`). `library/mod.rs` lists them in `ENTRIES` after part 3's, in the spec's order.
`library/build.rs` gains the few helpers they use. No engine change is planned; a part that finds a feature
doesn't do what a scene needs swaps in the nearest idea that works and says so here.

**Tech Stack:** Rust (pinned toolchain), the workspace crates.

**Spec:** `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md` (the tables, "Checks"). Part 3's plan and
its As built: `part3.md`. Handover: `/mnt/project-files/ballphysics/builtin-scenes/part3-handover.md`.

## Global constraints

- **Goldens unchanged**: no engine change, so no golden line may move and `BLESS_GOLDEN` is not needed.
- Scenes start from `new_studio_scene` through `Lib`, every thing named, related things in a Folder, rules named for
  what they do, no card, layer or rule that does nothing, no files from disk, public-domain or own melodies only.
- Every scene here is `SoundLevel::Designed` (own oscillators and envelopes from `Lib::tone`, a reverb on the Mixer).
- In-between: 25 to 30 s. Spectacles: 25 to 40 s (the test checks), a hook title, a live number on screen, one
  payoff stated in `SceneInfo.payoff` where it is a moment the physics decides. Preview 12 to 15 s.
- `cargo fmt --all` (width 120), `cargo clippy --workspace --all-targets -- -D warnings`, tests with
  `--test-threads=2`, line-tables-only debug and no incremental (in `~/.cargo/config.toml`). `cargo test -p bpe-kit
  --lib` and `cargo test -p bpe-studio scenes_window` before pushing.
- Unused `Lib` helpers fail clippy: add a helper only when a scene uses it.
- The exe is not built or installed.

## Facts found while planning (the engine as it is)

- A Look card holds `SurfaceSettings` on its own keys (`surface`, `roughness`, `see_through`, `tints`, `bends_light`,
  `rainbow`, `fire`, `dichroic`, `mirror`, `light_pipe`, ...). Picking a chip in the editor writes the chip's preset
  numbers (`surface::preset_values`) in the same edit; a scene does the same through a new `Lib::surface`.
- The `shape.glass` ready look is a Glass chip with a 0.55 Colour layer: coloured glass that tints the light through
  it (Coloured light through glass is on by default on the Lighting card).
- A filled shape is solid glass to the beam tracer; with the Prism chip (Rainbow split 1) a Light beam or a laser
  through it fans into colours. Glass bends lasers and beams; other shapes follow the light's At shapes (Bounce,
  Stop, Pass through). A light's `bounces` caps the turns.
- A light inside a Balls thing rides its ball (Rides on); `add_light(scene, reg, kind, parent)` takes the parent.
- A Rig drives every light under it: Pattern (Chase, Strobe, Sweep, ...), Steps on Beats/Bars/Hits/Seconds at the
  scene's tempo, colours from a list, Brightness. `add_rig` adds four spots.
- Rules: `when.beat` (every beat or N bars, Offset in beats) runs from the scene's tempo and first beat;
  `do.change` reaches any card (Own, Look, Lighting, Mixer, Sound ...) so a rule can change the light or a rig's
  Brightness; `do.flash` on a Light flares it by its Flare; `do.count` Adds or Sets (no multiply); `do.spawn` adds
  balls to a Balls thing; `when.count` fires when a counter reaches a number.
- A voice's Osc A can be Noise; routes from an ADSR to a pitch knob (`a_semi`) move it by a share of its travel
  (48 semitones), so a fast second envelope on `a_semi` makes a kick's pitch drop.
- Particles: `looks::add_particles` makes a Particles thing from a ready look; a Liquid layer (`particles.liquid`)
  with the Glass surface and Bends light bends lamp light through it (free glass, bent light); particles meet
  balls and shapes through their Bounce card. The Huge swarm (Particles Look card Engine) runs on the graphics card
  up to 4 000 000 particles; without a card (the physics tests) it has none, and export and the editor give it one.
- A ball bouncing straight up and down under gravity with bounciness 1 lands every `T = 2 v / g`; a ball started
  at its top lands after `sqrt(2 h / g)`. Part 3's Polyrhythm rings found that straight motion with no gravity keeps
  time over 25 s; the probe checks the same for bounces under gravity before the music scenes rely on it.

## Decisions made without Aedan (he asked for the parts to run to completion; also in the As built and handover)

1. **Folder `spectacles/`** for 29 to 33; 25 to 28 go in `middle/` (they are in-between scenes in the spec).
2. **Stained glass**: a rose window of coloured glass panes (diamonds round a middle pane) in a dark stone room.
   A glowing **lamp ball** carries a point light and bounces among the panes with a few glass marbles, so the
   coloured shadows the panes throw sweep round the walls as it moves. Each pane rings its own bell note.
3. **Prism rainbow**: a white Light beam from the left through a filled glass prism (Prism chip) in the middle
   that spins slowly, so the rainbow fan sweeps across the room. White marbles drop from an emitter, bounce off
   the prism and pass through the colours. The spec's "bouncing glass prism" is read as balls bouncing on the
   prism: a shape can't move about on its own, and a spinning prism makes the rainbow move.
4. **Club night**: 120 bpm. Balls bounce on a dance floor in time: the kick ball lands on every beat, the clap ball
   on beats 2 and 4, the hat ball on the off-beats, each ball's own drum voice on its hits. Their start heights and
   speeds are worked out so the landings fall on the beat (`on_the_beat` in the scene file). A rig of spots chases
   on the beat in club colours, a second rig strobes on each bar, haze shows the beams, and a mirror ball hangs at
   the top.
5. **Splash glass**: a tall drinking glass (a Glass box with its top side open) holding a liquid (particles drawn
   as a Glass Liquid that bends light), lit by a lamp from the side, with balls dropped in from above every couple
   of seconds that splash the water about. The balls ring the glass as they hit it.
6. **The simulation is the song** (spectacle, 120 bpm): random plinks first (three balls in a ring playing random
   notes of A minor pentatonic), then a ball arrives every two bars, each one a part of the beat (kick, hats, clap,
   bass, chords), fired by its own emitter at the top of its bounce at the moment that puts its landings on the
   grid. A live "PLAYERS" count. Payoff: the drop at bar 13 (a `when.beat` rule: title, flash, zoom punch, the
   lights change), checked as RuleFires.
7. **Escape on the drop** (spectacle): nested spinning rings with gaps, the build-up played by the rings' hits and
   a soft beat; the outer escape *is* the drop: the rule on it shatters the ring, slows time, flashes, swaps the
   light from cold blue to hot magenta, turns the rig on and starts the full beat (the beat rules wait on a counter
   "Dropped"). The rings are tuned so the outer escape lands on a bar line (the tempo's first beat set to match).
   Payoff: Breaks of the outer ring.
8. **One to a million** (spectacle): a ring with a gap; every ball that gets out is replaced by two new balls in
   the middle, so the balls double and double. A live count of the balls. When the count reaches its top number
   the ring shatters, the counter text reads 1,000,000 and a Huge swarm of a million particles pours out of the
   middle. Payoff: the counter reaching that number. The doubling is chance (balls launched each their own random
   way), so the Seed is tuned for the payoff time.
9. **Diamond fire** (spectacle): a big filled diamond (Glass chip with Fire and some Dichroic, Rainbow split) spins
   in the dark, a white Light beam through it fans rainbow rays round the room, and diamond balls (Fire, so they
   throw star glints when hit) bounce off it. A live hit count "{n}/100"; at 100 hits the diamond shatters with slow
   motion and a flash. Payoff: the counter reaching 100.
10. **Mirror maze** (spectacle): a laser zig-zags round a room of chrome mirrors (At shapes Bounce); glowing balls
    with no gravity bounce round and every mirror has Health, so after a few hits it shatters and the beam finds a
    new path. Light pipes glow where the beam meets them. Live "MIRRORS LEFT". When the last mirror breaks, the beam
    reaches a crystal prism in the middle and fans into a rainbow (the prism sits behind the mirrors from the
    laser's view until then). Payoff: the counter of mirrors left reaching 0.
11. New drum voices for the music scenes: `Tone::Kick` (sine with a fast pitch drop), `Tone::Hat` (short noise),
    `Tone::Clap` (noise and a triangle, a little longer).

## Review focus

1. Each idea shows in the pictures: coloured shadows that move, a rainbow fan that sweeps, the beat lands on the
   grid, the drop comes with the escape, the swarm pours out, the diamond's fire, the beam changing path.
2. The rules do what their names say; every Do hangs off the right block; nothing is left doing nothing.
3. Sound is designed, not clipping, and not a wall of notes; drums are in time.
4. Full-size pictures: no flat frames, nothing off screen that should be seen, text readable, no light leaks.

---

### Task 1: Helpers

**Files:** modify `crates/bpe-kit/src/library/build.rs`.

- [ ] `surface(id, SurfaceKind, f)`: the Look card's Surface set to the chip with its preset numbers, then `f` on
  `SurfaceSettings`.
- [ ] `light` gains the parent (`light_in(parent, ..)`), so a lamp can ride a ball; the old `light` calls it with
  the root.
- [ ] `particles(look, parent, name)`: `looks::add_particles` named.
- [ ] `rig(name, f)` if the club scenes use more than the starter spots' defaults.
- [ ] `Tone::Kick`, `Tone::Hat`, `Tone::Clap` in `tone`.
- [ ] Only what a scene uses (clippy).

### Task 2: The nine scenes

**Files:** create `library/middle/{stained_glass, prism_rainbow, club_night, splash_glass}.rs`,
`library/spectacles/{mod, the_simulation_is_the_song, escape_on_the_drop, one_to_a_million, diamond_fire,
mirror_maze}.rs`; modify `library/middle/mod.rs`, `library/mod.rs` (`mod spectacles;`, `ENTRIES` 25 to 33),
`tests/library.rs` (`levels_so_far` (14, 14, 5)).

- [ ] **Step 1:** Write each scene. Tune with a throwaway probe test (`tests/zz_probe.rs`, deleted before commit)
  that plays a scene, prints hits, rule firings, breaks and counters every quarter second and draws pictures at
  chosen moments (with the swarm's graphics card on the thread).
- [ ] **Step 2:** Music scenes: check from the probe's hit times that the beat balls land within a step or two of
  the beat for the whole scene.
- [ ] **Step 3:** `cargo test -p bpe-kit --test library -- --test-threads=2`, `cargo test -p bpe-sound --test
  library_sound`, `cargo test -p bpe-studio scenes_window`: PASS.
- [ ] **Step 4:** Full-size look check for each new scene (`LOOK_ONLY=library-<id>`), open the pictures and crops,
  fix what looks wrong.
- [ ] **Step 5: Commit** in batches.

### Task 3: Notes, checks, review, PR

- [ ] Notes: `docs/notes/library.md` (part 4 As built, new helpers, gotchas); this plan's As built.
- [ ] `cargo fmt --all`, clippy, `cargo test -p bpe-kit --lib`, the whole suite once in the background.
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Push, draft PR, subscribe; when green and reviewed, merge.
- [ ] Handover `/mnt/project-files/ballphysics/builtin-scenes/part4-handover.md`, memory, message the coordinator.

---

## As built (2026-11-10)

Nine scenes, no engine change. Helpers in `library/build.rs`: `surface`, `light_in` (`light` calls it with the
root), `particles`, `unlit`, and `Tone::Kick`, `Tone::Hat`, `Tone::Clap`. No `rig` helper: the scenes use `add_rig`
and set the spots themselves. Gotchas are in `docs/notes/library.md`.

| # | Scene | Length | What it does | Payoff |
| --- | --- | --- | --- | --- |
| 25 | Stained glass | 28 s | A lamp ball (a point light inside a glowing ball) bounces round a rose window of eight coloured panes and a middle pane; their flat coloured shadows sweep the walls. | none |
| 26 | Prism rainbow | 28 s | A white beam through a glass prism (rainbow split 2.5) that rocks to and fro by keyframed spin; pearls drop through the colours onto a floor. | none |
| 27 | Club night | 30 s | Kick, clap, hats and bass balls land on a 120 bpm grid (`on_the_beat`); a chase rig on the beat, a strobe rig each bar, a mirror ball, haze. | none |
| 28 | Splash glass | 26 s | A Glass box open at the top holding a spring-home particle crowd drawn as a Glass liquid; glass balls drop in three at a time and splash it; a spot lamp from the side. | none |
| 29 | The simulation is the song | 36 s | Random plinks in a ring, then each player drops onto the stage every two bars from the top of its bounce; "N OF 5 PLAYING"; the drop rule at bar 13. | RuleFires "The drop" at 24.125 s |
| 30 | Escape on the drop | 32 s | Three spinning rings; each escape shatters a ring. The last escape (20.34 s) shatters in slow motion and the drop waits for the next bar (21 s): text, zoom, lamp blue to pink, the rig on, the full beat. A room keeps the ball on screen afterwards. | Counter "Dropped" 1 at 21 s |
| 31 | One to a million | 30 s | Each escape spawns two balls in the middle (balls double); at 256 the rule sets the count to 1,000,000, shatters the ring and fires an emitter whose ball carries a Huge swarm fountain. | Counter "Balls" 1,000,000 at 16.8 s |
| 32 | Diamond fire | 30 s | A spinning five-sided glass diamond (Prism chip with Fire 1, Dichroic 0.35, Rainbow 2) splits a white beam into rainbow rays; 18 diamond balls under gravity hit it; shatters at 100 hits. | Counter "Hits" 100 at 18.23 s |
| 33 | Mirror maze | 30 s | A red laser bounces round six neon-edged chrome mirrors (Health 7, shatter), two light pipes and a spinning prism; four glowing balls break the mirrors; at the last one the laser turns white and the prism fans it into a rainbow. | RuleFires "The last mirror breaks" at 22.0 s |

Changes from the decisions above, and why:

- **Escape on the drop**: the drop is a `when.beat` (every bar) rule waiting on the counter the last escape sets,
  not the escape itself. The escape timing is chaotic (a 0.03 px/s change moved it by 20 s), so a drop hung on it
  could land off the beat on another computer; the bar never does. Payoff is the Dropped counter.
- **One to a million**: no Seed tuning was needed; the spawn's Random way and the ball-to-ball hits give the
  doubling, and the swarm starts from a rule (an emitter fired By a rule carrying the swarm), not by keyframes.
- **Diamond fire**: a pentagon, not a square (parallel sides cancel the split); gravity on so the balls keep hitting.
- **Mirror maze**: the laser is red and turns white at the end, so the rainbow is the reveal; the mirrors are
  neon-edged (plain chrome read as faint grey lines). Payoff is the last rule, since a counter falling to 0 from a
  start of 0 is met at once.
- **Club night / The simulation is the song**: the scene's Gravity is doubled and the Gravity × values halved,
  because Gravity × only goes to 5 and the save clamps it.
