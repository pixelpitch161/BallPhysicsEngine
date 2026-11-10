# Built-in scenes part 3: in-between scenes 15 to 24

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ten in-between scenes for the Scenes window (Will it escape?, Escape layers, Melody per bounce,
Polyrhythm rings, All meet at once, Hypnotic petals, Red vs Blue, Colour wars, Marble race, Merge and grow), each
using two or three systems together with fuller sound, plus the counter payoff check the library tests left for
this part.

**Architecture:** Scenes are code in a new folder `bpe-kit/src/library/middle/`, one file each with `ENTRY` and
`build`, listed in `library/mod.rs`'s `ENTRIES` after the starters. They use only today's engine (part 2 added the
gaps, Merge balls and Melody they need). `library/build.rs` gains the few helpers they use. The library test's
`PayoffEvent::Counter` check replays the scene and reads the rules' counter.

**Tech Stack:** Rust (pinned toolchain), the workspace crates. No engine, shader or editor changes are planned.

**Spec:** `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md` (the in-between table, "Checks"). Part 2's
plan and its As built: `part2.md`. Handover: `/mnt/project-files/ballphysics/builtin-scenes/part2-handover.md`.

## Global constraints

- **Goldens unchanged**: no engine change, so no golden line may move. `BLESS_GOLDEN` must not be needed.
- Scenes start from `new_studio_scene` through `Lib`, every thing named, related things in a Folder, rules named for
  what they do, no card, layer or rule that does nothing, no files from disk, public-domain melodies only.
- In-between scenes have `SoundLevel::Designed`: more than the starter voice (their own oscillator and envelope
  choices) and a reverb or delay on the Mixer's bus.
- Lengths 25 to 30 s (in-between has no rule in the test, but the window lists the time). Preview 12 to 15 s.
- `cargo fmt --all` (width 120), `cargo clippy --workspace --all-targets -- -D warnings`, tests with
  `--test-threads=2`. Build with `CARGO_INCREMENTAL=0` and line-tables-only debug (set in `~/.cargo/config.toml`
  in the container so every command agrees). Run `cargo test -p bpe-kit --lib` before pushing.
- Unused `Lib` helpers fail clippy: add a helper only when a scene uses it.
- The exe is not built or installed.

## Facts found while planning (the engine as it is)

- **A Do block is a leaf.** The rules runtime (`bpe-runtime/src/rules/mod.rs::deliver`) passes a firing on from
  When, If and Logic blocks only; a Do block queues its actions and passes nothing on. A rule that does several
  things links every Do to the last If (or the When). `Lib::rule` chains blocks one after another, so it only fits
  rules with one Do at the end. New helper `Lib::rule_doing(name, chain, dos)`: the chain linked in a line, each
  Do linked from the chain's last block.
- `when.hit` between two balls fires for both sides (A hits B and B hits A). A rule naming two teams by tag fires
  once for each team's side, so two "takes over" rules in the same step would swap two balls.
- A ball's own tag (`do.tag` with That or Other on a ball) lives in `RulesState::ball_tags` and a Who of Tag finds
  it; a thing's tag can't be taken off one ball. Team membership that changes per ball must be ball tags only.
- `when.fired` (an emitter's balls, each a ball) is the only When whose firing is about each new ball at the start,
  so teams that start tagged come from two emitters.
- A ball's colour is its Look's Body layer `0/colours` (a Gradient); a rule changes it for one ball (per-ball look
  settings are live).
- Emitters can sit on a focus of an ellipse (`sits_on`, `sits_focus`); with the Bounce card's Touch walls with
  Middle, balls from one focus meet at the other within 0.01 px (engine notes).
- Live words: `{counter <name>}`, `{health of <thing>}`, `{balls left of <thing>}`, `{standings down N of
  <thing>}` (the Race leaderboard look). Standings name balls by their nearest colour name.
- The camera follows the middle of every live ball of a followed thing; Stay inside with a 1080-wide box keeps it
  from sliding sideways.
- `do.note` plays on a Shape's voice (a set note, or the shape's own next note as a hit picks it; with Plays All at
  once that is a chord).
- Health's By strength takes `factor` per px/s of the hit's strength, Hits from a `Target`.

## Decisions made without Aedan (he asked for the parts to run to completion; also in the As built and handover)

1. **Folder `middle/`** for the in-between scenes (the spec's example path `library/middle/will_it_escape.rs`).
2. **Will it escape?** keeps one ball in a spinning ring with one gap. Each escape adds 1 to the counter
   "Escapes", plays a bright note, and a new ball drops into the middle (`do.spawn` into the same Balls thing) so
   the count keeps climbing. Escaped balls fall off the bottom of the screen and are taken away (a zone under the
   screen breaks them, Vanish). Payoff: the counter reaching 1 at the first escape's moment.
3. **Escape layers**: three nested rings with one gap each, spinning different ways; when the ball leaves a ring,
   that ring shatters, the camera punches in and the next ring out plays a note a step up (a `do.note`, then that
   ring's hits are an octave higher). A title "FREE!" when the last ring breaks. Payoff: the outer ring breaking.
4. **Melody per bounce** plays Ode to Joy (Beethoven, public domain), a ball bouncing in a box, with "GUESS THE
   SONG" at the top; near the end the title changes to the answer ("ODE TO JOY") through Set text.
5. **Polyrhythm rings** puts three rings in a column; each ball goes straight across its ring with no gravity at a
   speed that makes it hit 3, 4 and 5 times a bar (120 bpm, a bar 2 s). They start at the left wall so every bar
   begins with all three together. The rings turn (Spin) only as a look; a circle's spin doesn't move a ball.
   The spec's "Three rings spin at 3:4:5" is read as three rings beating 3:4:5.
6. **All meet at once**: an emitter sits on the ellipse's upper focus and fires a round of balls all round, with
   Touch walls with Middle and bounciness 1, so they meet at the lower focus, then back at the upper one, again and
   again. A small pass-through zone at each focus plays a chord (`do.note`, the zone's own next note, Plays All at
   once) when the balls arrive, held to one chord a meeting by a cooldown.
7. **Hypnotic petals**: a six-petal flower (a moulded polygon outline with bent sides) and an emitter in the middle
   firing a few hundred small balls all round, no gravity, bounciness 1, a colour spread round the round and a
   background gradient that shifts through keyframes. Hits play soft notes with a limit on notes a second.
8. **Red vs Blue**: two big balls in a box with a few spinning bumpers, each ball a Health card taking damage By
   strength from anything it hits, so the ball that hits harder and more often loses: the physics decides, not the
   starting health. At zero it shatters; a rule then sets the title to the winner and punches the camera. Live
   health numbers at the top. Payoff: the loser breaking (whichever the tuned scene gives).
9. **Colour wars**: two emitters fire the teams at the start (a when.fired rule tags each ball and counts it). When
   a red ball hits a blue one, the blue ball turns red (tag, colour, both counters) with a 50% chance, and the same
   the other way round, so each meeting can go either way (both going at once swaps the two balls, which keeps the
   counts right). The first team to take every ball wins (when.count on the counter, the title says who). Payoff:
   that counter reaching the whole count.
10. **Marble race**: six marbles drop through a tall course (pegs, two spinners, slanted ramps) to a finish zone,
    the camera following the pack inside a 1080-wide box, a Race leaderboard of the six ranked furthest down, and
    "WINNER!" with a flash when the first marble crosses the line (Only once). Payoff: that rule firing. The spec's
    "camera on the leader" becomes the camera on the pack (the camera follows a thing's middle; it can't pick out
    the leading ball).
11. **Merge and grow**: 32 small balls of one size in a ring, no gravity, bounciness 1, Hit other balls on; two
    same-size balls that touch merge (Add areas), with Pitch follows size on the balls' sound so bigger balls play
    lower. A live ball count. No payoff (merges depend on chance meetings).
12. **Counter payoffs** are checked by replaying the scene in 1/60 s steps and finding the first moment the counter
    reaches the amount (the log has no counter events).

## Review focus

1. Each scene's idea shows: escapes happen, payoffs land on time, the polyrhythm lines up every bar, the balls really
   meet at the focus, the winner is decided before the end.
2. The rules are tidy and do what their names say (every Do linked to the right block; no rule left doing nothing).
3. Sound is designed, not clipping, and not a wall of notes (limits where hundreds of balls hit).
4. Full-size pictures: no flat frames, nothing off screen that should be seen, text readable.

---

### Task 1: Helpers and the counter payoff check

**Files:** modify `crates/bpe-kit/src/library/build.rs`, `crates/bpe-kit/tests/library.rs`.

- [ ] **Step 1:** In `library.rs`, replace the Counter panic with a replay: build a fresh `World`, advance in 1/60 s
  steps to the scene's end, and give the first time `world.rules().counter(name) >= amount` (for an amount below the
  counter's start, falling to it: `<=`; the start is the counter at 0 s). Bump `levels_so_far` as scenes land.
- [ ] **Step 2:** `build.rs` helpers, added as scenes need them:
  - `rule_doing(name, chain: &[(kind, settings)], dos: &[(kind, settings)]) -> RuleId`.
  - `melody(id, steps: &[&[u8]])`: the Sound card's Plays Melody with these steps (made if missing).
  - `reverb(decay, mix)`: the scene's Mixer (`ensure_mixer`) gets a Reverb on Bus 1's lane.
  - `emitter(name, at, f)`: an emitter named and set by `f`.
- [ ] **Step 3:** `cargo test -p bpe-kit --test library -- --test-threads=2`: PASS. Commit with the first scene.

### Task 2: The ten scenes

**Files:** create `crates/bpe-kit/src/library/middle/{mod.rs, will_it_escape.rs, escape_layers.rs,
melody_per_bounce.rs, polyrhythm_rings.rs, all_meet_at_once.rs, hypnotic_petals.rs, red_vs_blue.rs,
colour_wars.rs, marble_race.rs, merge_and_grow.rs}`; modify `library/mod.rs` (`mod middle;`, `ENTRIES` 15..24 in
the spec's order), `tests/library.rs` (`levels_so_far` (14, 10, 0)).

| id | s | Things | Rules and sound |
| --- | --- | --- | --- |
| will-it-escape | 30 | "Ring" (r 430, one gap 0.07, spin 45°/s, glow look), "Balls" (one ball r 28, gravity 1, bounciness 0.97), "Title" (Hook title "WILL IT ESCAPE?"), "Escape counter" (Hit counter look, "{counter Escapes} ESCAPED"), "Drain" (wide pass-through box below the screen) | "A ball gets out": when.enter Leaves Ring → count Escapes +1, spawn a ball at the ring's middle, play a note on the ring. "Gone off screen": when.enter Enters Drain → break That (Vanish). Ring: Plays Up minor pentatonic, triangle wave, reverb. Payoff Counter Escapes 1. |
| escape-layers | 30 | Folder "Rings": "Inner ring" (r 200), "Middle ring" (r 330), "Outer ring" (r 460), each one gap, spins ±; "Ball" (no gravity, 700 px/s, bounciness 1, trail) | Per ring: when.enter Leaves → break the ring (Shatter), camera Zoom punch, play a note on the next ring. Last: Set text "FREE!". Each ring's own notes, a step up from the one inside. Payoff Breaks "Outer ring". |
| melody-per-bounce | 28 | "Box" (rectangle), "Ball" (gravity, bounciness 1, one-way launch), "Title" | Box Plays Melody: Ode to Joy (60 steps); "The answer" when.time 24 → Set text "ODE TO JOY". Bell-like voice, reverb. |
| polyrhythm-rings | 24 | Folder "Rings" ×3 (r 250 at y −620, 0, 620, spin as a look), Folder "Balls" ×3 (r 26, no gravity, speeds for 3/4/5 a bar, trail) | Each ring its own note (C4, G4, E5) and voice; tempo 120; "{bar}" counter. |
| all-meet-at-once | 25 | "Ellipse" (900 × 1500), "Emitter" (sits on focus 1, 72 balls all round, Touch Middle, trails), Folder "Foci": "Top focus", "Bottom focus" (pass-through r 24) | "Chord when they meet": when.enter Enters a focus → cooldown 0.5 s → note on that focus (its next note, Plays All at once). Ellipse hits play quiet shimmer notes, most notes a second limited. |
| hypnotic-petals | 30 | "Flower" (6 petals), "Emitter" (300 balls r 6, all round, colour spread), background keyed | Flower hits play a pentatonic pad, Most notes a second 10, reverb. |
| red-vs-blue | 30 | "Arena" (box), Folder "Bumpers" (3 spinning filled shapes), "Red", "Blue" (r 70, Health 100 By strength), "Title", "Red health", "Blue health" | "Red is out" / "Blue is out": when.health Reaches zero → Set text title, camera punch. Each ball its own voice. Payoff Breaks the tuned loser. |
| colour-wars | 30 | "Arena" (ring), "Red cannon", "Blue cannon" (emitters, 20 balls each), "Red count", "Blue count", "Title" | Tag on fire + count; "Red takes a blue ball" / "Blue takes a red ball" (hit → chance 50 → tag remove/add, colour, counts); "Red wins" / "Blue wins" (when.count reaches 40 → Set text). Payoff Counter of the tuned winner. |
| marble-race | 30 | "Course" walls, Folder "Pegs", Folder "Spinners", Folder "Ramps", "Finish line" (pass-through), "Marbles" (6), "Standings" (Race leaderboard), "Title" | "First across the line": when.enter Enters Finish line → only once → Set text "WINNER!", flash That, camera punch. Marbles play notes on pegs. Payoff RuleFires. |
| merge-and-grow | 30 | "Ring", "Balls" (32, r 22, random launch 500 px/s, Hit other balls), "Ball count" | "Same sizes merge": when.hit ball on ball → merge (Add areas). Balls' Sound with Pitch follows size 12. |

- [ ] **Step 1:** Write each scene; use a throwaway probe test (`tests/zz_probe.rs`, deleted before commit) that
  plays a scene and prints hits, rule firings, breaks and counters over time to tune it.
- [ ] **Step 2:** `cargo test -p bpe-kit --test library -- --test-threads=2`, `cargo test -p bpe-sound --test
  library_sound`: PASS. Window tests `cargo test -p bpe-studio scenes_window`: PASS (24 tiles).
- [ ] **Step 3:** Full-size look check: `LOOK_OUT=<dir> LOOK_ONLY=library-<id> cargo test -p bpe-kit --test
  full_size_look -- --ignored --nocapture` for each new scene; open the pictures and crops and fix what looks wrong.
- [ ] **Step 4: Commit** in two or three batches.

### Task 3: Notes, checks, review, PR

- [ ] Notes: `docs/notes/library.md` (part 3 As built, new helpers, gotchas: Do blocks are leaves, both sides of a
  ball hit fire); this plan's As built.
- [ ] `cargo fmt --all`, clippy, `cargo test -p bpe-kit --lib`, the whole suite once in the background (goldens
  unchanged).
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Push, draft PR, subscribe; when green and reviewed, merge.
- [ ] Handover `/mnt/project-files/ballphysics/builtin-scenes/part3-handover.md`, memory, message the coordinator.

## As built

Landed 2026-11-10. Ten scenes in `bpe-kit/src/library/middle/`, every one `SoundLevel::Designed`; no engine change,
goldens untouched. Where the build differs from the decisions above, the build wins:

| # | Scene | Length | What it ended up as | Payoff (measured) |
| --- | --- | --- | --- | --- |
| 15 | Will it escape? | 30 s | Ring r 360 with one 0.15 gap spinning 60°/s, a ball r 28 at 900/-500, Mallet ring. Each escape (once a ball) counts "Escapes", spawns a new ball and plays a high note; a Drain zone under the screen breaks fallen balls. | Escapes reaches 1 at 2.1 s (escapes at 2.08, 9.17, 14.45, 21.92, 24.72, 27.38; before Only once a ball, balls that bounced back in counted twice) |
| 16 | Escape layers | 26 s | Rings r 190/320/450, gaps 0.12/0.09/0.08, spins 70/-50/25, green/cyan/pink Neon tubes, Pluck. Leaving a ring shatters it (14 pieces), punches the camera and plays the next ring's top note an octave up; the last says "FREE!". | Outer ring breaks at 20.75 s (inner 3.54, middle 9.28) |
| 17 | Melody per bounce | 28 s | Ode to Joy in 62 steps (some with a low chord note) on a Bell box 720 x 940; a ball r 30 at 830/-1000, about 2.4 hits a second. Title turns to "ODE TO JOY" at 25.5 s. | none |
| 18 | Polyrhythm rings | 25 s | 120 bpm, bar 2 s. Three rings r 250 (red Bass 3 a bar, yellow Mallet 4, cyan Bell 5), rings at y -500, 60, 620; each ball starts against the left side moving left, at across x hits / bar, so it hits at once and every bar starts with all three. "BAR {bar}". | none (checked: all three hit together every 2 s, from 0 s) |
| 19 | All meet at once | 25 s | Ellipse 900 x 1500, an emitter of 72 balls r 7 on the upper focus (aim -87.5°, speed 560, Touch Middle). Focus zones r 4 play Pad chords (Am7 / Fmaj7, All at once) with a 1 s cooldown. | none (chords at 2.67, 5.35, 8.03 s ...) |
| 20 | Hypnotic petals | 30 s | A 144-point six-petal polygon (Aurora, spin 6), 300 balls r 6 at 420 from the middle, a rainbow by launch angle whose angle is keyed -180 to 180 over the scene (in place of a moving background). Pad notes, retrigger 125 ms. | none |
| 21 | Red vs Blue | 30 s | Box 940 x 1300, three spinning white bumpers tagged "hurts"; balls r 70 also "hurts"; Health 100 By strength x 0.006 from "hurts" only. KO rule: title, the loser's number set to 0, zoom punch, slow motion 1.5 s, and the winner's Health takes Nothing. | Red breaks at 18.77 s |
| 22 | Colour wars | 30 s | Seed 8. Ring r 480, two emitters of 20 (After 0.1 s). Cannons fire from a ring of 125. Fired balls are tagged and counted; a hit turns That, the other team's ball (50% chance each side, 0.25 s per-ball cooldown): tags, Body colour, counters. 40 wins. | Blue reaches 40 at 23.63 s |
| 23 | Marble race | 25 s | Six marbles through 46 pegs (rows of 7 and 6), four ramps and two spinners to a finish at y 3900; the camera follows the pack inside a 1080 x 5400 box; leaderboard size 30 at (-190, -470). "WINNER!", a camera punch and the Finish line (Hit ripple look) flashing. | First across at 15.92 s |
| 24 | Merge and grow | 30 s | 64 pearls r 14 in a ring r 470, speed gain 1.08 capped at 650; same size within 35% merge; Pitch follows size (12 semitones, at 14). | none (64 down to 3 by about 19 s) |

New in `build.rs`: `Block`, `rule_doing`, `melody`, `Tone` + `tone`, `reverb`, `emitter`, `keys`, `colour_in`, and
`look_in` now puts the look on a second time with Keep mine, so the colour reaches the tube, glow and halo (found at
the full-size look check: every ring had come out the look's own pink). `tests/library.rs` gained the Counter payoff
replay (`counter_reaches`). Other fixes from the full-size check: Escape layers' title moved down to -640, the race
board made smaller and moved on screen, the loser's health shows 0 instead of "-".
