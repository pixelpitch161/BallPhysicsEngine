# W7 plan: physics, playback and export speed

Design: `docs/superpowers/specs/2026-11-08-physics-playback-speed-design.md`. One pull request, one commit per step.
Every step keeps the goldens (debug and release) and the picture tests exactly as they are, except S16 where only a
scene with a very big ball may move. Timings: `cargo test --release -p bpe-kit --test speed_check -- --ignored
--nocapture --test-threads=1` before and after each step (release with `CARGO_PROFILE_RELEASE_LTO=false`).

## Step 0: the speed checks

- `crates/bpe-kit/tests/speed_check.rs`: ignored tests that print timings (trails and jumping back, the sound
  engine's physics, an endless emitter, fine air, one big ball among many, moths). Not run by CI's test job (ignored)
  but compiled, so they can't rot.

## Step 1: S15, a lasting thread pool

- `crates/bpe-kit/Cargo.toml`: `rayon = "1"` (lock already has 1.12).
- `crates/bpe-kit/src/air.rs`: `by_rows`, `by_rows2` use `rayon::scope` + `s.spawn` per band (same `bands`).
  Calm, heat, swirls (curl by rows into a buffer, then the confinement by rows reading only the curl) and the final
  clamp loop go through `by_rows`/`by_rows2` with the step's `threads`. Each closure writes only its own row and reads
  only buffers from before the loop.
- `crates/bpe-kit/src/things/particles/step.rs`: `run_chunks_indexed` and `run_chunks_with` use `rayon::scope`; the
  grouping (`per = chunks.len().div_ceil(threads)`) and the in-order collection stay.
- Verify: `air::tests` (any thread count gives the same air), particle goldens, `particles` unit tests, fine air timing.
- Don't: change band sizes, `THREADED_CELLS`, `MOST_THREADS`, or anything numeric.

## Step 2: S10 + S14 (paths), `Track`

- New `crates/bpe-runtime/src/track.rs`: `pub(crate) struct Track<T: Copy + PartialEq>` with `chunks:
  VecDeque<Arc<[T]>>` (each exactly `CHUNK` = 64), `tail: Vec<T>` (< 64), `skip: usize` (items dropped from the
  front of the stored items), `run: Option<(T, usize)>` (count ≥ 1 of the same item after the stored ones). API:
  `new`, `len`, `is_empty`, `get(i)`, `front`, `push_back(x)` (equal to the last item: grows the run; else pushes the
  run out first when it is longer than 1... see code), `pop_front`, `iter()` (double-ended), `iter_from(i)`,
  `partition_point(f)`, `is_one_run()` (every item is the run), `memory()` (tail + unshared/shared share of chunks).
  `Clone` (shares chunks), `PartialEq` by items, `Debug`.
- `history.rs`: `balls: Vec<Track<Vec2>>`, `corners: Vec<Track<(f64, Vec2)>>`; every `VecDeque` use becomes the
  `Track` one (`range(first..)` → `iter_from`, `ring[i]` → `get`). `record(step, positions, alive)`: a ball not alive
  whose track `is_one_run()` at the cap and the same place is skipped (tip moment still set). `memory()` uses
  `Track::memory`. Callers of `record` pass the alive column (world.rs).
- Unit tests in `track.rs`: matches a `VecDeque` model under a long random mix of pushes (with repeats) and pops,
  clones share chunks, memory falls when shared. Existing `history.rs` tests unchanged and passing.
- Verify: runtime tests, kit trail and particle tests (`trail::`, `line::`, `particles::`), goldens, trail picture
  tests; speed checks (jump points no longer thinned; emitter world memory down).

## Step 3: S14 (rows), `Bodies.gone_below`

- `bodies.rs`: `pub gone_below: usize` (default 0), `fn note_gone(&mut self)` advances it over removed rows,
  `fn live_rows(&self) -> Range<usize>` (`gone_below..len`). Doc: rows below are removed for good.
- `world.rs`: `note_gone` for every thing at the start of `step`; the per-row loops in `step`, `substep`/the wall
  stage, `work_out_pulls` and `collide_pairs`' gather use `live_rows()`; each loop still checks `alive` as before.
- Kit: emitter loops over `st.rows` in `fire_round`/fades and `arrivals` start at `gone_below`.
- Verify: goldens, emitter tests, speed check (the emitter second at 120 s).

## Step 4: S11 and S13

- `world.rs`: `build` calls a private `build_with(scene, resolved, sims, drawn: bool)`; `pub fn build_unseen`. When
  not drawn, `sim.history(r)` is not asked. Test: an unseen world has no trail history, keeps Text standings', and
  steps the same as a drawn one (positions and log hash after 5 s of a trails scene).
- `playback.rs`: `Playback::unseen(scene, resolved, sims, budget)`, `fn forward_only(mut self) -> Self` (no
  checkpoints after the first). Test: forward-only seek gives the same world as a normal one; seeking back still works
  (from the start).
- `bpe-sound/src/engine.rs`: physics via `Playback::unseen`; `Engine::offline(...)`, the same with `forward_only`;
  `offline.rs`'s renders use it.
- `bpe-studio/src/particle_job.rs` and `aim_job.rs`: unseen.
- `bpe-cli/src/lib.rs` `export_with`: sound on a scoped thread into the temp WAV (progress through an atomic, cancel
  through an atomic flag its progress closure reads); video to `video.mp4` in the temp folder (no audio input, no
  faststart); then `mux` (`-i video -i sound -map 0:v -map 1:a -c:v copy` + `AUDIO_ARGS` + `+faststart -shortest`).
  A scene without sound keeps today's one pass. Progress: `done = frames × 0.9 + sound × 0.1` of `total`. Errors and
  cancelling remove every output as now.
- Verify: `bpe-cli` export tests (they run ffmpeg), sound goldens, `bpe-sound` tests, studio particle/aim job tests.

## Step 5: S16, big balls apart

- `pairs.rs`: `Grid::runs_in(cx0, cx1, cy0, cy1)` (one run per column), `for_each_pair` unchanged. In
  `collide_pairs`: after gathering, `bigs` = balls with radius > `BIG_TIMES` (4) × median, used only when
  `0 < bigs.len()` and `bigs.len() * BIG_SHARE (8) <= balls.len()`. Then the grid holds the small balls (mapped back
  to ball indices, which keep their order), cell `2 × largest small`; contacts from small pairs in grid order, then
  every big ball's pairs (cells its reach covers, and the other big balls) sorted and deduplicated, in that order.
  The contact maths moves into one function used by both.
- Tests: a big ball among small ones finds the same set of contacts as a plain all-pairs check (several random
  layouts); with no big ball the contact list is the same as before (the goldens).
- Verify: goldens (name any line that moves, and why), pairs tests, speed check.

## Step 6: S17, the light probe

- `sensing/probe.rs`: `Probe.now: Mutex<Option<Resolved>>` (the animated copy kept between steps; taken with
  `try_lock`, a fresh clone when busy or first); rules or gradient extras: a fresh clone of it, as today.
  `Probe.looks: Mutex<BTreeMap<(ThingId, bool), Arc<LookLight>>>` for things whose Look no key writes, used only on
  the plain path. `Lamps::of` takes the looks through a closure.
- Verify: `sensing::tests`, moths goldens (`golden-moths*.txt`), lit goldens, speed check.

## Step 7: finish

- `docs/notes/engine.md`: as built (Track, gone_below, unseen worlds, forward-only playback, rayon pool, big balls,
  probe reuse) with the before/after timings.
- Full `cargo test --workspace -- --test-threads=2`, clippy, fmt, release goldens; one Opus review; PR; CI green;
  merge.

## As built

(filled in at the end)
