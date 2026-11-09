# Editor responsiveness (review workstream W6: S1 to S9)

Source: the 2026-11-08 code review ("Ball Studio code review: what to improve", Speed, Editor feel), items S1 to S9,
approved by Aedan with everything but the Size section. One pull request, one review at the end. Nothing here changes
what the export draws or how the balls move: goldens must not move.

## Goal

The editor stays smooth while you drag sliders, scrub, pause on effects, add lights, type effect code or sit idle on
the Look card. Each item below states the fix, the exact place, what not to do and how it is checked.

## S1: the warm-up's picture is the preview's picture

Problem: `preview/warm.rs::draw_at` builds frames with `build_frame_with` (no light probe), without `show_light`
(Light only / Heat map), without Solo (`lights_panel::soloed`) and without the particle playback's particles, then
swaps that renderer in.

Fix:
- New `preview::compose(studio, world, log, time, look, frame)` in `preview.rs`: `build_frame_sensing` with the soloed
  copy of `resolved` (or `studio.resolved`), `studio.follow`, `studio.light_probe`; then `world_look` when `look`;
  then `show_light(frame, studio.light_show)`. `draw_picture` and `warm::draw_at` both call it. `draw_picture` keeps
  its own lending of particles and `stage_view` bookkeeping around it.
- The preview keeps the particle snapshot it last drew (`Preview::particles: Option<Arc<Snapshot>>`). The warm-up's
  last frame (the playhead's own, the one swapped in) lends it when its step is the warm playback's step, exactly as
  `draw_picture` does. Replayed frames before it have no particles (the particle playback only has the playhead's
  moment); noted in `docs/notes/editor.md`.
- The light show and the soloed lights join what a warm-up was started for (`Seen`, `Warmup::light`): changing either
  restarts it.
- Not: no second particle playback for the warm-up (it would replay particles from the start).

Check: new GPU test in `warm.rs`: a lit scene with an echo effect shown as Light only; after the warm-up swaps in, the
picture equals a straight run drawn with Light only (and differs from the Picture show). Existing warm-up test passes.

## S2: the warm-up has a time budget and lets go of its spare renderer

Fix in `warm.rs`:
- `FRAMES_PER_UPDATE` becomes `WARM_BUDGET = 8 ms`: replay frames until the budget is spent (always at least one).
- After the swap the spare renderer (the old cold one) is kept for `SPARE_KEEP = 2 s` of no warm-up, then dropped
  (`Preview::spare_since`). `update` asks for one repaint after `SPARE_KEEP` so it is dropped even when idle. A scene
  with nothing that remembers drops it at once (as now).

Check: warm-up test still swaps (it loops `update` until it does); new unit check that the spare goes once idle past
the keep time (time injected through a `now` argument to a small helper, `spare_expired`).

## S3: physics edits rebuild the runs when you let go or pause

Problem: `Studio::scene_changed` rebuilds the aiming, the main playback, the particle playback and the sound on every
frame of a slider drag.

Fix in `app.rs`:
- New field `Studio::hold_runs: bool`, set at the start of each `ui` frame to "a pointer button is down and the
  preview is not being dragged" (`Preview::dragging()`), false otherwise and in tests.
- `scene_changed` resolves, follows and probes as now, then calls `restart_runs()`. `restart_runs` compares the
  physics, particle and sound fingerprints with the ones the runs were built from. While `hold_runs` is on and one of
  them changed, it only notes `runs_due = Some(now)` and keeps the old runs (the preview shows the old run drawn with
  the new looks). Otherwise it rebuilds what changed exactly as today.
- `settle_runs()` runs at the end of each frame (after `record_frame`): when `runs_due` is set and either the pointer
  is up or nothing changed for `RUNS_REST = 100 ms`, it calls `restart_runs()` with the hold off. While due it asks
  for a repaint after `RUNS_REST`.
- Preview handle drags (moving a ball, moulding a shape) still rebuild every frame, so the thing follows the pointer.
- Not: no change to the fingerprints themselves.

Check: unit tests: a held physics edit keeps the playback (`fingerprint` unchanged, `runs_due` set); letting go
rebuilds once; a held rest of 100 ms rebuilds; a look edit while held still only bumps the revision.

## S4: the Look card's views rebuild only when something changed

Problem: `look_editor/ball_view.rs::show` and `group_view.rs::picture` resolve the mini scene and `format!("{:?}")`
the resolved thing on every frame; the ball view's Moving and Hit build a new `World` each 33 ms, also when the
window isn't in front.

Fix:
- Both views keep `Cached { mini: Scene, resolved: Resolved, content: u64 }`: `mini_scene` is still made each frame
  (one thing's copy), compared with the cached one (`PartialEq`), and only on a difference resolved and hashed again.
  The picture key is `content` plus the per-frame numbers (radius, time, clock, pixels, mode, ground, solo, number).
- The ball view keeps the `World` it built for `content` and reuses it: `build_view_frame` takes `&mut World` and
  resets every field it sets (position, radius, velocity, alive, hits) each time. `World::build` runs only when the
  content changes.
- Moving and Hit repaint every 33 ms while the window has focus, every 250 ms when it doesn't
  (`ui.input(|i| i.focused)`).

Check: existing view tests pass (they call `build_view_frame`, now through a small wrapper that builds the world);
new test: two `show`-equivalent key calls on an unchanged scene don't rebuild (`Cached` hit counter in tests).

## S5: the timeline adds hit ticks as hits arrive

Fix in `timeline/mod.rs`: `Timeline::hit_ticks: HitTicks { made_for: (revision, track left/width bits, duration
bits, rate), read: usize, ticks: BTreeSet<(i32, ThingId)> }`. Each frame: same key and the log at least `read` long
reads only `events[read..]`; otherwise starts over. The slot marks (`slot_change_times`, `round_marks`) are worked
out again at most every 200 ms while playing and the log grows (immediately on a new revision, a new duration, or
while paused).

Check: unit test on `HitTicks::update`: growing logs read only the new part (same ticks as a full read); a shorter log
or another revision starts over.

## S6: no whole-scene copy every frame; undo capped by size

Fix in `app.rs` / `history.rs`:
- `Studio::snapshot: Option<Scene>`: the scene as last recorded. `ui` takes it (`take()`, or a clone the first time)
  instead of `self.scene.clone()`; `record_frame` compares once and, only when the scene changed, records and stores a
  fresh copy. `after_edit`/`after_separate_edit`/`replace_scene`/`undo`/`redo` keep working (the next frame's compare
  sees the scene they left and stores it).
- Frames with no input at all while playing (no events, no button down, no flags, same revision) skip the compare.
- `unsaved()` is cached by `revision` (and cleared on save and on a new scene) so the toolbar doesn't compare the
  scene with the saved one every frame.
- `History` keeps `Step { scene, bytes }`; `bytes` is the scene's RON length, counted without building the text
  (`ron::ser::to_writer` into a counting `fmt::Write`). Past `UNDO_BYTES = 256 MiB` in undo and redo together, the
  oldest undo steps go (the newest step always stays). `UNDO_LIMIT = 200` stays.

Check: history tests for the byte cap; app tests for recording (existing ones pass); a test that an idle playing frame
doesn't compare (counter in tests).

## S7: lighting and effect shaders build in the background and are shared

Fix in `bpe-gfx`:
- `lighting.rs`: the pipelines, layouts, sampler and empty group move to `#[derive(Clone)] LightKit` built by
  `LightKit::build(device)`; `Lighting::with_kit(device, &kit)` makes the per-renderer buffers. The Stage camera's
  pipelines likewise move to `StageKit` inside `LightKit` (`stage_camera_pass.rs`, `StageCameraPass::with_kit`).
- `programs.rs`: `Programs` also holds the light kit (`LightSlot::{None, Building(Receiver), Ready(LightKit)}`) and
  the picture effects (`effects: HashMap<u64, Result<Arc<Compiled>, String>>`, `effects_pending`). In a background
  store both build on their own thread; otherwise at once, as now (exports, tests, the CLI).
- `Renderer::render_scene`: a lit frame whose light kit isn't ready yet is drawn unlit and counts as waiting
  (`programs_waiting()`), so the editor draws it again in 50 ms (`redraw_when_built`).
- `effects/chain.rs`: `Chain` gets the shared store (`share_programs` hands it on); `compile` asks the store and skips
  an effect still building (marking the renderer waiting). Its own map keeps `Arc<Compiled>`.
- Not: no change to any shader text or what a ready frame draws.

Check: `programs_tests.rs`: a background store hands out the light kit and an effect only once built, and two
renderers sharing it get the same kit (`Arc::ptr_eq` on a marker). Every existing lighting and effect picture test
passes (they use the default, immediate store).

## S8: scrubbing catches up once per frame

Fix: `Studio::seek` only moves the playhead (and tells a playing sound); the preview's `draw_picture` does the catching
up, one budget a frame. A test-only `Studio::catch_up()` seeks the playback all the way for tests that need it.

Check: the scrub test still holds; tests that read `playback.time()` right after `seek` call `catch_up()`.

## S9: the particle playback's particles are shared, not copied, for each picture

Fix in `particle_job.rs`: `Snapshot::states` becomes `Vec<(ThingId, Arc<dyn ThingState>)>`; the preview lends
`Box::new(arc.clone())` (the runtime already treats a boxed `Arc<dyn ThingState>` as its state), so a drawn frame no
longer deep-copies every particle. `air_view` reads through the same type.

Check: the particle job test passes; preview particle tests pass.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo test -p bpe-studio -p bpe-gfx -- --test-threads=2` locally, then CI's full run.
- Goldens: none should move (no physics change); CI's release goldens confirm.
- Full-size look check: nothing drawn changes for a ready frame, so only the lighting/effects scenes are rendered once
  as a spot check (`LOOK_ONLY=room,glass,text`).
- One Opus review of the whole diff before the PR.

## As built

Built as planned, with these differences:

- S1: the warm-up looks up the particle playback's latest snapshot itself (`draw_at`), instead of the preview keeping
  the one it drew. The new picture test solos one of two lights (Light only and Heat map leave the picture effects out,
  so they never need a warm-up).
- S3: a held edit that adds or removes a thing never waits (`Studio::runs_things`), so the runs never hold a thing the
  scene no longer has. `settle_runs` takes the pointer state and returns how long to wait.
- S4: the cache is `ball_view::MiniCache`, shared by both views; the Group view still builds its world when its
  picture key changes (it lays out rounds in it), the ball view keeps its world.
- S6: `record_frame` keeps its `before` argument (tests drive it); `ui` passes the kept snapshot. The quiet-frame skip
  is the pure `quiet_frame`.
- S7: the Stage camera's pipelines are part of `LightKit` (`StageKit`), so the first moved Stage camera doesn't
  compile either. `Lighting::layouts` went (the kit hands the layouts over). The store keeps at most 64 picture effects
  (`KEEP_EFFECTS`), oldest built first out.
- The as-built summary for the editor is in `docs/notes/editor.md`, "Responsiveness".
