# W7: physics, playback and export speed (design)

Workstream W7 of the 2026-11-08 improvement review ("Ball Studio code review: what to improve", findings S10, S11 and
S13 to S17; S12's longer-term half, letting the particle thread reuse the main run's balls, stays for later). Aedan
picked everything but the Size section on 2026-11-08. Nothing here changes what a scene does or how it looks: every
change is the same result reached with less work, so the goldens and picture tests must not move (S16 is the one
place a result may move, and only in a scene with a very big ball; see below).

## What was measured first

`crates/bpe-kit/tests/speed_check.rs` (ignored tests, release, printed timings) on the 4-core container, before:

| Check | Before |
| --- | --- |
| Fine air (dust, 186 x 290 cells), 2 s of air | 1.8 s (slower than real time); 80% of it the pressure solve, which starts 4 threads 22 times an air step |
| 1600 small balls hitting each other, 3 s | 0.33 s; with one ball of radius 200 added: 3.0 s (9x) |
| Endless emitter, 200 balls a second, earlier rounds removed | the second up to 10 s: 2 ms; up to 120 s: 42 ms (24,000 rows, 20 alive); half of it recording the paths of removed balls; the world is 28 MB, almost all paths of removed balls |
| 120 balls, Line that stays, 120 s | 142 MB of jump points, thinned from 241 to 31 |
| The sound engine's copy of that scene, 60 s | 134 MB, all of it ball paths nothing reads |
| Moths, 5 s | 110 ms; about half rebuilding the light probe's settings (cloning the whole resolved scene and re-reading every look each step) |

## Decisions

### S10 and S14 (paths): shared, run-compressed path storage

`History` keeps each ball's samples and corners in a `Track` (`bpe-runtime/src/track.rs`) instead of a `VecDeque`:

- Full chunks of 64 items are frozen into `Arc<[T]>` and shared by every copy of the world (a jump point copies the
  pointers, not the samples); only the open tail (under 64 items) is copied.
- A run of identical items at the end is kept as one item and a count. A ball that has gone (or rests) records the
  same place every sample: its path stops growing in memory, and a 2 s trail of a removed ball becomes one item.
- Dropping the oldest item moves a start offset; a chunk is let go once every item in it has been dropped.
- Reads (`len`, `get`, iterate both ways, iterate from an index, binary search) give exactly what the `VecDeque` gave,
  so every reader (trails, staying Lines and their memos, particle layers, Text standings) sees the same items.
  `Track`'s `PartialEq` compares items, not how they are stored.
- `History::memory` counts a shared chunk's bytes divided by how many copies share it, so the jump points' budget
  sees what is really used and stops thinning them.
- A removed ball whose whole kept path is one run is skipped when sampling (pushing the same place and dropping the
  oldest would change nothing), and its newest moment still moves on, as before.

S14's other half: `Bodies.gone_below` (rows below it are all removed). Removed balls never come back (nothing sets
`alive` back to true; checked), so `World::step` advances it once a step and the runtime's per-ball loops in the step,
the walls stage and the ball-to-ball gather start there. An endless emitter whose oldest rounds go has its removed
balls below it. The finding's other idea, reusing a removed ball's row, is not done: rows are what the log, the trail
memos, particle emissions, rules and riders name a ball by, so reusing one would change event logs and risk old events
landing on a new ball. Leaving rows in place and not paying for them gets the speed without that.

### S11: worlds nobody draws keep no trail paths

`World::build_unseen` and `Playback::unseen` build a world that never calls `ThingSim::history` (the trail and
particle layers' want). Text standings still ask for paths through `World::keep_history` in `after_build`, so a
scene whose physics reads them (letter walls showing live standings) runs the same. Used by the sound engine
(`Engine`), the editor's particle thread and its aiming thread. Paths never change the physics (a test pins that
already: "the same scene without the history steps the same").

### S13: export sound beside the video

- The sound engine used for the export keeps no jump points (`Playback::forward_only`): it only ever moves forward.
- `bpe_cli::export_with` renders the sound on a second thread (`std::thread::scope`) while the video is drawn. The
  video goes to a temporary video-only mp4; when both are done, a second ffmpeg pass copies the video stream and
  encodes the sound into the final file (`-c:v copy`, the same AAC settings, `+faststart`, `-shortest`). A scene with
  no sound keeps the single pass with a silent track. Stems and the MIDI file are written by the sound thread as now.
- The progress bar is the frames' share (90%) plus the sound's (10%), both moving at once; cancelling stops both and
  removes every file the export made. If the sound fails, the video is stopped and the error reported.

### S15: one lasting pool of threads

The air step and the particle step use `rayon::scope` (rayon is already in the build through other crates) instead of
`std::thread::scope`: the same fixed bands and chunks, run on a pool that stays up, so no thread is started per pass.
Band edges still depend only on the row count and the thread count, so the air and the particles are the same for
any number of threads (the existing tests check that). The air step's loops that ran on one thread (calm, heat,
swirls and their curl, the final clamp) run by rows on the pool too: every cell reads only the buffers from before,
so the result is the same.

### S16: big balls handled apart

In the ball-to-ball stage, when a few balls are much bigger than the rest (radius over 4x the median, and no more than
one ball in 8), the grid is sized for the largest small ball and holds only the small balls. Each big ball finds its
neighbours by looking up the grid cells its reach covers, plus every other big ball. Contacts are then the small
pairs in grid order followed by the big ball's pairs in index order. With no big ball the stage runs exactly as
before, so scenes of similar balls keep their logs. A scene with a very big ball may solve its contacts in a different
order (same physics, different rounding), so its golden line may move; that is the only allowed golden change, and
it must be named in the PR.

### S17: the light probe re-reads only what changed

- The probe keeps its animated copy of the settings between steps and re-applies the keys to it (applying is
  idempotent and writes only the keyed settings), instead of cloning the whole resolved scene every step. When rules
  or gradient movement change the picture it works on a fresh copy, as before.
- How each thing's look blocks or gives light (`LookLight`) is worked out once per thing and reused while that
  thing's Look isn't keyed and nothing else changed it.
- Ball positions still feed the blockers every step, as they must.

## Not done (and why)

- S12's longer term (particle thread reusing the main run's balls): left for later, as the work plan says.
- Reusing removed balls' rows (see S14).
