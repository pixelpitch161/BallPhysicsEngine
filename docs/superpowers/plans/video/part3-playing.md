# Video arrangement, part 3: Video mode, playing. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Video mode, Space (or the arrangement's Play button) plays the whole video in real time, picture and
sound, as it will export: each clip's scene, the transitions, the song and every clip's sound through the video's
Master. Moving the playhead while playing jumps there; a clip that has to run up to its point shows "Getting
ready…" while it catches up on another thread. Loop plays the video again from the start. The arrangement gets
its **Scene sound** lane (each clip's hits as ticks in its colour). Double-clicking a clip (or **Edit scene** in its
inspector) opens its kept scene in Build mode with a bar "Editing *Ring Drop* in the video *Drop Tape*. Back to
video"; Back to video (or the Video button) puts the edited scene back into the video as one undo step.

**Architecture:** Three background workers, none of which the editor waits on:

1. **Pictures**: the still-frame worker of part 2 (`video_mode/still.rs`, its own `headless_device`) learns a
   second kind of job, *Play from frame F*. It drives a new `bpe_cli::arranged::Player`, which keeps each clip on
   screen as a live run (FrameMaker + Renderer at the cover size), draws frame after frame, composes transitions as
   the export does and reads each frame back as RGBA at `STILL_SCALE`. Frames go to the editor through a small
   bounded channel, so the worker never runs more than a few frames ahead. Clips about to come on screen are run up
   in the worker's idle time, a little at a time.
2. **Sound**: a *take* (`video_mode/take.rs`): the whole video's sound (`bpe_sound::VideoMix`) rendered from 0 on a
   thread into memory, as fast as it goes. The existing sound thread (`sound_job.rs`) learns to play a take instead
   of its scene engine (`Command::Video`), so Play, Pause, Seek, the clock, the speakers' ring, the listening volume
   and the speaker delay all work as they do for a scene.
3. **Hits**: a thread that runs each clip's scene physics (no drawing) and lists its hits in video time, for the
   Scene sound lane (`video_mode/hits.rs`).

The playhead follows the speakers' clock (`SoundJob::clock`), or the wall clock without speakers or without sound.
The picture shows the newest drawn frame not later than the playhead.

**Tech Stack:** Rust (toolchain pinned), egui 0.37 + egui_kittest, wgpu, rtrb/cpal through the existing sound
thread.

**Spec:** `docs/superpowers/specs/2026-11-24-video-arrangement-design.md` ("The editor: Video mode": Playing,
Editing a clip's scene; "Parts" 3). **Earlier parts:** `docs/notes/video.md`, `plans/video/part1-file-and-export.md`,
`plans/video/part2-arranging.md`.

## Global Constraints

- No physics or sound golden line may change. A scene played alone (Build mode, export) is untouched: the sound
  thread's scene path, `export_arranged` and `arranged_frames` behave exactly as before.
- Editor colours only from `style/` tokens; a clip's ticks use its kept scene's `TrackColour` (`// scene colour`).
- kittest: every control named (`labelled_by`), `query_all_by_label(..).next()` where several may match.
- egui pitfalls (CLAUDE.md): `Sense::click_and_drag()` on the clip itself for double-click; no `add_space` in grids;
  never read `press_origin()` inside `clicked()`.
- Plain words for every message. No demo videos or screenshots.
- Tests: only the new ones plus the existing tests of the files touched (`bpe-studio` `video_mode::`,
  `sound_job::tests`, `app::tests`; `bpe-cli --test arranged`). `cargo fmt --all`, `cargo clippy --workspace
  --all-targets -- -D warnings` stay clean.

## Design decisions (made here, stated so the review can check them)

1. **Sound plays from a take, not live.** The spec left "live or background render" to the builder. A take renders
   the video's mix from 0 exactly as the export does (`VideoMix`, the same clip engines, fades, song and Master), so
   what is heard is what is exported, and a jump anywhere already rendered is instant. `VideoMix` renders a scene's
   sound much faster than real time, so the start of a take is ready within moments. A take is 384 KB a second
   (48 kHz stereo `f32`): 23 MB a minute, 690 MB for the longest video (30 min); the editor holds at most two (the
   one playing and the one replacing it).
2. **A changed video gets a new take**, started once the video has rested 0.4 s with no clip drag held
   (`TAKE_REST`). While playing, the old take goes on playing until the new one has rendered past the playhead
   plus 0.25 s (`SWAP_AHEAD`), then the sound thread swaps to it at the same sample: an edit is heard a moment
   later, never as a gap. A take nobody holds any more stops rendering (its thread checks `Arc::strong_count`).
   No take is made while the video has no clips or no sound (`bpe_sound::video_has_sound`); such a video plays to
   the wall clock.
3. **The sound thread plays a take through `Command::Video(Option<Arc<Take>>)`.** `Some` switches its source to
   the take (or queues it as the next take when one is already playing); `None` goes back to the scene engine. With
   a take as its source, `Seek(t)` sets the read position to `t` rounded down to a block (the same rounding as the
   clock's `base`), `Play`/`Pause` work as before but never touch the scene engine (no pre-warm), and filling copies
   the take's samples into the ring. When the take hasn't rendered as far as the read position plus one block, the
   thread writes nothing and reports itself held (`shared.set_playing(false)`), so the clock waits and no dropout is
   counted; it starts again once the take is 0.25 s ahead (or finished). Past the take's end it writes silence.
   Leaving Video mode sends `Video(None)` and seeks the scene engine back to the Build playhead.
4. **Play waits until both are ready.** Pressing Play starts a picture job at the playhead's frame and (with sound)
   tells the sound thread the take, then the session is *starting*: the playhead holds, the preview shows the last
   frame with a spinner and "Getting ready…", and only once the first picture frame has come back and the take has
   rendered 0.25 s past the playhead does the sound start (`SoundJob::play`). Pressing Space again while starting
   cancels it.
5. **The picture follows the sound.** Each editor frame the session takes frames from the channel into a queue of at
   most `QUEUE` = 4 and shows the newest one whose number is at or before the playhead's frame, dropping older ones.
   The worker reads the playhead's frame from a shared `AtomicU32`; when it is more than 2 frames behind it, it steps
   the physics of the frames in between without drawing them (`Player::skip`), so a heavy scene drops frames rather
   than drifting from the sound. Skipped frames aren't drawn, so an Effect that remembers frames looks a little
   different until it has drawn a few: the preview's quick run-up already accepts that, the export never skips.
6. **`bpe_cli::arranged::Player`** (pub, `Player<'a>` borrowing the tidied video, its clip scenes and the kit):
   `new(device, queue, video, scenes, kit, scale)`, `draw(frame, stop) -> Result<Vec<u8>>` (RGBA, frames asked in
   order), `skip(frame)`, `warm_ahead(until: Instant, stop)` and `size()`. It draws exactly what `still_frame` and
   the export draw at that frame (the same `moment`, `look_of`, cover fit, `within` and `Compose`), without bake
   (as the still frame), with each frame's motion-blur samples (`FrameMaker::samples(t, fps, None)`). A lone clip at
   the output size is read back straight from its renderer; anything else through `Compose`; a gap is black. Its
   `Readback` holds one frame (push, then `finish`): the editor wants each frame as soon as it is drawn.
7. **Run-ups are the quick kind and come in small pieces.** A clip's live run (`LiveClip`) starts its scene from 0
   and steps it frame by frame to its first frame: physics only (`FrameMaker::advance_to`, new) until the frames the
   scene's memory needs, which are drawn (the rule of `bpe_cli::run_up`: from `earliest = first - MAX_MEMORY`, the
   memory read at `earliest`). `LiveClip::warm(until, stop)` does as much as fits before `until` and says whether it
   is done. Clips on screen at the first frame are warmed to the end before it is drawn; a clip that starts within
   `LOOKAHEAD` = 4 s after the frame being drawn is made and warmed in the worker's idle time (while the channel is
   full) and finished at once if it comes on screen unready (the picture then catches up by skipping). A clip that
   has ended is dropped, so at most three scenes (two on screen, one coming) are held.
8. **Jumps.** The session remembers the time it set the playhead to last; when the playhead is anywhere else while
   playing (the ruler, Home, End, the arrows, a clip paste), it is a jump: the picture job is stopped and a new one
   started at the new frame, the sound is sought there, and the session is *starting* again (decision 4). Dragging
   on the ruler while playing pauses for the drag and plays again when it is let go (as the scene's scrub bar does).
9. **Edits while playing.** The picture job is restarted at the playhead once the video has rested `TAKE_REST` with
   no drag held, while the clock runs on (the picture catches up by skipping); the sound swaps to the new take as in
   decision 2, so an edit never stops the sound. Undo and redo count as edits. Playing, the take and the hits run
   liquids at Export detail, as the still frame and the export do, so what plays is what is exported.
10. **Stopping.** Space, the Play button, or reaching the end without Loop. The playhead stays on the frame on show
    (so the paused picture is the one just seen), the picture job is stopped (its channel dropped), the sound is
    paused there, and the still frame is told that frame is already on show (`Still::shown_from_play`), so it isn't
    drawn again. At the end with Loop on, the playhead jumps to 0 (decision 8).
11. **Loop** is the video's own (`VideoSession.looping`, the editor's, not saved), a switch in the arrangement's
    header beside a Play button and the time. The header's Play button reads "Play" / "Stop" (plain words; the
    default fonts lack ▶).
12. **Switching modes** stops the video playing. Entering Video mode pauses the Build scene; leaving it sends
    `Video(None)` to the sound thread. The editor notices a mode change once a frame (`Studio::follow_mode`, at the
    end of `ui`), wherever it came from (toolbar, D, opening a scene from Video mode).
13. **Scene sound lane** (below the Song lane): each clip's hits as 2-point ticks in its scene's colour, where they
    land in the video, only within the clip. The hits worker (one thread, `Priority::Low`) runs each clip's scene
    copy (`clip_scene`) with `World::build` / `advance_to` to `offset + length` and keeps its `EventKind::Hit` times
    as video seconds. It keeps each clip's result with the scene copy and the span it ran for, so an edit that leaves
    a clip's scene copy and span alone (moving a clip without Follow video tempo, a level, a transition) reuses it.
    A newer video stops the run in progress between clips and between seconds of a clip. While a clip's hits are
    being found the lane says "Finding hits…" in its strip. Only hits are shown (a clip's scene's notes and its
    sound's loudness are not, my call: hits are what the scene's own timeline shows).
14. **Edit scene** (double-click a clip's body, or **Edit scene** in the clip inspector): the editor parks the Build
    scene as it is (`ParkedScene { scene, saved, path, history }`), opens a copy of the clip's kept scene in Build
    mode (`replace_scene`, no path, so Ctrl+S doesn't write a scene file), and remembers `ClipEdit { kept, name }`.
    A bar across the top of the window (under the toolbar) reads "Editing <scene> in the video <video>." with a
    **Back to video** button. Build and Grade work as usual, with their own undo.
15. **Back to video** (the bar's button, or the Video button / any switch into Video mode): if the scene changed,
    `video.change(false, |v| v.scenes[kept].scene = scene)` then `arrange::tidy` (its Length may have changed), one
    undo step in the video; then the parked scene comes back (`replace_scene(parked.scene, parked.path)`, then its
    `saved` and `history` restored) and the mode is Video. A kept scene removed meanwhile (impossible from Build mode,
    but undo can't reach it either) just drops the edit with a status line.
16. **While editing a clip's scene**: Ctrl+S / Save puts the scene back into the video (as Back does, staying in
    Build mode, the parked scene untouched) and saves the video; Save as likewise for the video. New, Open (a scene)
    and opening a built-in first go Back to video (the edit is kept in the video), then ask about the parked scene as
    usual. Closing the window first goes Back to video, then asks as usual (scene, then video). Opening a video or New
    video goes Back to video first. The scene's autosave is skipped while editing (the parked scene's own autosave
    copy, if it had one, stays on disk untouched), and the video's autosave and crash copy hold the video with the
    edit put in (`Studio::video_with_edit`), refreshed whenever the scene's revision or the video's changes.
    `video_unsaved()` counts the edit (the edited scene differs from the kept copy).
17. **Not in part 3**: the On top lane (part 4); playing at a size other than `STILL_SCALE`; the scene's notes in
    the Scene sound lane; meters for the video's Master.

## File structure

- Modify `crates/bpe-cli/src/lib.rs`: `FrameMaker::advance_to(time)` (physics only).
- Modify `crates/bpe-cli/src/arranged.rs`: `Player`, `LiveClip`, `clip_scenes`.
- Create `crates/bpe-studio/src/video_mode/take.rs`: `Take` and its render thread.
- Modify `crates/bpe-studio/src/sound_job.rs`: `Command::Video`, the take source in `Worker`, `SoundJob::set_video`.
- Create `crates/bpe-studio/src/video_mode/play.rs`: the session's playing state (`Playing`), `Studio::video_play`,
  `video_stop`, `video_tick`, jumps, takes, starting.
- Modify `crates/bpe-studio/src/video_mode/still.rs`: the worker's `Job::Play`, `Still::shown_from_play`, the
  preview's "Getting ready…".
- Create `crates/bpe-studio/src/video_mode/hits.rs`: the hits worker and the Scene sound lane's data.
- Modify `crates/bpe-studio/src/video_mode/lanes.rs`: header Play / Loop, the Scene sound lane, double-click Edit
  scene, the ruler's pause while dragged.
- Create `crates/bpe-studio/src/video_mode/edit.rs`: `ParkedScene`, `ClipEdit`, `Studio::edit_clip_scene`,
  `back_to_video`, `put_back_clip_scene`, the bar.
- Modify `crates/bpe-studio/src/video_mode/{mod.rs, inspect.rs, files.rs}`, `app/{mod.rs, shortcuts.rs,
  autosave.rs, files.rs}`: wiring.
- Modify `crates/bpe-studio/src/video_mode/tests.rs`: the new checks.
- Modify `docs/notes/video.md`: Part 3 as built.

## Task 1: `Player` in bpe-cli

- [ ] `FrameMaker::advance_to(&mut self, time)`: `self.world.advance_to(time, &self.kit.sims, &mut self.log)`.
- [ ] `pub fn clip_scenes(video, kit) -> Vec<Scene>` (each clip's `clip_scene`), used by `arranged_frames` too.
- [ ] `LiveClip<'a> { maker, renderer, fit, first, at, earliest, from: Option<u32>, drawn_to: Option<u32> }`:
      `new(device, scene, kit, out, fps, first)`; `warm(device, queue, until, stop) -> Result<bool>` steps scene
      frames `at..first`: `< earliest` advance only; `== earliest` reads `maker.frame(..).memory()` and sets `from`
      (`ceil((first_t - memory) × fps)`, at least `earliest`; no memory: `from = first`) and renders it if
      `>= from`; otherwise render when `>= from`, else advance. Returns true at `at == first`. Stopping gives
      "stopped" as `run_up` does.
- [ ] `Player { video, scenes, kit, out, fps, live: BTreeMap<usize, LiveClip>, compose, readback(1) }`.
      `draw(frame, stop)`: `moment`, `look_of`, the old clip filtered as the export does; each on-screen clip is
      made if missing, warmed to the end, then its scene frame drawn with `samples(t, fps, None)` (frames in between
      advanced if it is behind, e.g. after `skip`); then lone readback or compose as `still_frame`; then
      `retain` drops clips that have ended. `skip(frame)` advances each on-screen live clip's physics to that frame's
      scene time (making none). `warm_ahead(until, stop)` makes the first clip starting in `(last, last + 4 s]` and
      warms it. `size()`.
- [ ] Test `bpe-cli --test arranged`: `player_draws_what_the_still_frame_draws` (two flat test scenes with a
      crossfade; the player's frames at a lone frame, mid-transition and after equal `still_frame`'s within 2/255;
      `player_skips_and_still_draws_the_right_frame` (skip to a later frame then draw equals the still frame there
      for a scene without memory); `player_makes_the_next_clip_ahead` (after `warm_ahead` with a long deadline the
      next clip is live and ready).

## Task 2: takes and the sound thread

- [ ] `take.rs`: `Take { samples: Mutex<Vec<f32>>, ready: AtomicUsize, done: AtomicBool, total: usize, revision }`,
      `Take::start(video, kit, revision) -> Arc<Take>` (thread "ball-studio video take", `Priority::Low`, the editor's
      card for swarms; renders `VideoMix::fill` 4 800 frames at a time, appends, stores `ready`; stops when
      `Arc::strong_count == 1`), `ready_frames()`, `is_done()`, `read(from, out: &mut [f32]) -> usize` frames,
      `ready_to(seconds) -> bool` (done, or ready ≥ `seconds` × rate).
- [ ] `sound_job.rs`: `Command::Video(Option<Arc<Take>>)` (name "video"), `SoundJob::set_video`. `Worker` gains
      `video: Option<TakeSource { take, next: Option<Arc<Take>>, pos: u64, waiting: bool }>`. `handle`:
      `Video(Some)` → source or `next`; `Video(None)` → back to the engine, `warm_at = None`. `Seek` with a take →
      `pos = seek_sample`, clear, no engine seek, `waiting = true`. `Pause` with a take: no prewarm. `fill` with a take:
      swap to `next` when it is ready to `pos + SWAP_AHEAD` (or done, or nothing is playing); copy a block at a time
      while the take holds `pos + BLOCK`, write silence past its end; else `waiting`. `run` reports
      `set_playing(playing && !held && !waiting)` and skips pre-warm and paused rendering with a take.
- [ ] Tests (`sound_job::tests`): `a_take_plays_from_where_it_is_sought` (a take of known samples written by a test
      helper `Take::from_samples`; seek, play, the speakers get exactly those samples from that block);
      `a_take_not_ready_holds_the_clock` (a take that is not done and has nothing at the position: the clock stays,
      no underrun counted); `a_new_take_takes_over_at_the_same_place` (playing take A, send take B: the samples move to
      B's at the same position, no gap); `going_back_to_the_scene_plays_the_engine` (after `Video(None)` and a seek, a
      tone scene sounds again).

## Task 3: playing in the session

- [ ] `play.rs`: `Playing { on, starting, expected, frames: Option<Receiver<(u32, Vec<u8>)>>, queue: VecDeque,
      clock: Arc<AtomicU32>, stop: Arc<AtomicBool>, shown: Option<u32>, take: Option<Arc<Take>>, take_revision,
      changed_at, picture_revision, last_tick }` on `VideoSession.playing`; `VideoSession.looping`.
- [ ] `Studio::video_play()`, `video_stop()`, `video_toggle_play()`, `video_tick(now)` (called from `Studio::tick`
      in Video mode): starting → playing → stop at end / loop; jumps (decision 8); edits (decision 9); takes
      (decision 2); the clock (SoundJob or wall); `still.enabled` false (tests) still plays the clock and takes, with
      the picture job off.
- [ ] `still.rs`: `Job::Play { video, from, frames: SyncSender, clock, stop }`; the worker's play loop (try_send; while
      full `warm_ahead` 3 ms or sleep 1 ms; skip when more than 2 behind); `Still::show_frame(ctx, pixels, size)` and
      `shown_from_play(revision, frame)`; the preview's spinner and "Getting ready…" while starting.
- [ ] Space in Video mode toggles playing (replacing part 2's status line).
- [ ] Tests (`video_mode::tests`): `space_plays_and_stops_the_video` (wall clock: playing moves the playhead, Space
      stops it on a whole frame); `playing_stops_at_the_end_or_loops`; `a_jump_while_playing_starts_again_there`;
      `switching_to_build_stops_the_video`; `a_take_is_made_once_the_video_rests` (a video whose scene has sound; an
      edit makes a new take only after `TAKE_REST`); `the_picture_plays_on_the_card` (still enabled, a tiny video:
      Play, frames come back and the shown frame follows the playhead; GPU test via `test_device`'s presence).

## Task 4: Scene sound lane

- [ ] `hits.rs`: `Hits { worker, asked_revision, clips: BTreeMap<ClipId, Vec<f32>>, busy }`, `ask(video, revision)`,
      `take()`; the worker's cache `(Scene, span) → hits` per clip id; stops between clips and every simulated second
      when a newer ask has come.
- [ ] `lanes.rs`: `scene_sound_lane` (ticks per pixel column per clip, the clip's colour, clipped to the clip,
      "Finding hits…" while busy; widget info "Scene sound lane").
- [ ] Test `hits_land_where_the_clip_plays` (a scene with a ball that hits the floor: its hits appear shifted by the
      clip's start minus offset and only inside the clip; a second clip of the same scene reuses the first result).

## Task 5: Edit scene and Back to video

- [ ] `edit.rs`: `ParkedScene`, `ClipEdit`, `Studio.clip_edit: Option<(ClipEdit, ParkedScene)>`;
      `edit_clip_scene(clip)`, `back_to_video()`, `put_back_clip_scene()`, `video_with_edit()`, `edit_bar(ui)`.
- [ ] Wiring: double-click in `lane_input`, Edit scene button in the clip inspector, `follow_mode` (Video switch →
      Back), `save_work`, `guard` (decision 16), the close request, `autosave_scene` skip and `autosave_video` with the
      edit, `video_unsaved`, the bar under the toolbar in Build and Grade modes.
- [ ] Tests: `double_click_edits_the_clip_scene_and_back_puts_it_in` (scene Length changed in Build, Back: the kept
      scene has it, one undo step brings it back, the parked scene and its path came back);
      `saving_while_editing_saves_the_video`; `opening_a_scene_while_editing_keeps_the_edit_in_the_video`;
      `the_video_button_goes_back_to_video`.

## Task 6: notes, checks, review, PR

- [ ] `docs/notes/video.md` "Part 3 (as built)"; this plan's "As built".
- [ ] `cargo fmt --all`, clippy, the tests above (`bpe-studio video_mode:: sound_job::`, `bpe-cli --test arranged`).
- [ ] One Opus review of the whole part; fix what it finds; PR; CI green; merge.

## Things not to do

- Don't change `arranged_frames`' or `export_arranged`'s output (the same-bytes tests pin it).
- Don't seek the scene engine while a take is the source, and don't let a take touch the scene engine's state.
- Don't block the editor's thread on the picture worker, the take or the hits (every receive is `try_recv`).
- Don't open a second audio device: the one sound thread plays both.

## As built

Built as planned, with these differences:

- Edits while playing restart only the picture, not a *starting* phase (decision 9 as now written), so the sound
  never stops for an edit.
- `Take` has no revision of its own; the session remembers which revision its take was made for.
- Stopping with no picture played (tests, or before the first frame) puts the playhead on its whole frame.
- Back to video keeps the parked scene "saved" when it was saved before (the editor adds a camera and Master to
  every scene it holds, which would otherwise make an old headless scene look changed).
- `Studio::edit_frame_for_tests` draws Build mode's bar for the kittest of the Video button.
- After the review: takes are made only while playing (and let go when leaving Video mode) and are stored in fixed
  chunks; liquids run at Export detail in the picture, the take and the hits; a jump to the end stops on the last
  frame (or loops); a closed picture channel counts as ready; the stop frame keeps the revision it was drawn from;
  a new video's session clears the hits; Back to video records nothing when the scene wasn't changed and keeps the
  Build playhead.
- Not measured on Aedan's card yet: how heavy scenes keep up. The player drops frames (skips) rather than letting
  the picture drift from the sound, and the take renders ahead of the playhead, so a slow scene shows as a lower
  frame rate, not as lag.
