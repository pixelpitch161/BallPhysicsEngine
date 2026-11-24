# Video arrangement, part 2: Video mode, arranging. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A third face of the editor, **Video** (Build | Grade | Video), where Aedan lays scene clips out on a
song's bar grid: add scenes to the video, drag them onto the Scenes lane, move and trim clips, overlap two for a
transition and pick its kind, snap to bar or beat, undo, and set the video's, a clip's and a transition's settings
in the inspector. New video, Open, Save, autosave and recovery cover the video, and the Export window exports it.
The preview shows the still frame at the playhead, drawn off the editor's thread. Playing (picture and sound in
real time), Edit scene / Back to video and the Scene sound lane come in part 3; the On top lane in part 4.

**Architecture:** Everything new is in `bpe-studio` (a `video_mode/` folder) plus one function in `bpe-cli`
(`arranged::still_frame`). The editor keeps one `VideoSession` beside its scene (`Studio.video`): the video, the
video as last saved, its file, its own undo history, what is picked and the arrangement's view. The scene the
Build mode edits is untouched by Video mode. Every change to the video goes through pure functions in
`video_mode/arrange.rs` that end with `Video::tidy`, so the limits of part 1 hold after every edit.

**Tech Stack:** Rust (toolchain pinned), egui 0.37 + egui_kittest, wgpu, RON.

**Spec:** `docs/superpowers/specs/2026-11-24-video-arrangement-design.md` ("The editor: Video mode", "Parts" 2).
**Part 1 notes:** `docs/notes/video.md` (what `open_video`, `tidy`, `moment`, `cover`, `arranged_frames` do).

## Global Constraints

- No physics or sound golden line may change; nothing outside `bpe-studio` and `bpe-cli::arranged` changes
  behaviour. A scene played alone is untouched.
- Editor colours only from `style/` tokens. A clip's fill is its kept scene's `TrackColour` (a real scene colour,
  the line ends `// scene colour`), as the timeline does for things.
- kittest: every control named (`labelled_by`), pickers are `menu_button`s, `query_all_by_label(..).next()` where
  several may match.
- egui pitfalls from CLAUDE.md: no `ui.add_space` inside `egui::Grid`; a drag-only widget over a click widget
  swallows clicks (use `Sense::click_and_drag()` on the clip itself); never read `press_origin()` inside
  `clicked()`.
- Plain words for every message. No new colours, no demo videos, no screenshots unless asked.
- Tests: only the new ones and the editor tests this touches (`bpe-studio` `video_mode::`, `app::tests` that
  touch files/autosave/modes, `export_window`, `recovery`), and `bpe-cli --test arranged` for `still_frame`.
- `cargo fmt --all` (width 120) and `cargo clippy --workspace --all-targets -- -D warnings` stay clean.

## Design decisions (made here, stated so the review can check them)

1. **One `VideoSession` always exists** (`Studio.video`, never `None`): a fresh editor holds an empty, untitled
   video that equals its `saved` copy, so it is never "unsaved" until something is added. New video replaces it.
   This keeps every `Option` check out of the panels.
2. **The video's undo is its own.** `VideoHistory { undo: Vec<Video>, redo: Vec<Video> }`, at most
   `VIDEO_UNDO_LIMIT` = 64 steps (a video holds whole scenes, so steps are kept few). Every committed edit pushes
   the video as it was before. A drag pushes once, when it ends, and only if the video changed. In Video mode
   Undo/Redo (toolbar, Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z) act on the video's history; in Build and Grade on the
   scene's, as today.
3. **Every edit is applied to a copy taken when the gesture began**, then tidied (like the timeline's
   `GroupDrag`): a drag computes `start_video + delta`, so rounding never drifts and a clip pushed by `tidy` comes
   back when the pointer returns.
4. **Snapping** (`arrange::snap`): Bar snaps to `first_beat + k·bar`, Beat to `first_beat + k·beat`, Off to whole
   frames. Holding Alt drags free (whole frames still, from `tidy`). A move snaps the clip's **start**; if its
   **end** is closer to a grid line than its start is (in pixels, within 8 px), the end snaps instead, so a clip can
   be lined up by its tail to make a transition land on a bar. A trim snaps the edge being dragged.
5. **Moving a clip** sets its start; `tidy` then sorts, keeps overlaps to the neighbour and to half the shorter clip.
   A clip dropped so it would overlap the clip two away lands at the end of the clip before (part 1's rule).
6. **Trimming**: the right edge changes `length` only (at most `scene length - offset`, at least 1 frame). The left
   edge moves `start` and `offset` together and `length` the other way, so the picture at the clip's end stays
   where it is; it stops at `offset = 0` and at 1 frame long.
7. **A transition is an overlap.** Dropping or trimming a clip over the end of the one before makes one (its
   `transition_in` keeps its kind; a new clip's kind is **Crossfade**, so an overlap made by dragging shows a
   transition straight away; a Cut over an overlap still shows the new clip alone as in part 1). The overlap is
   drawn as a striped block over the two clips; a click picks the transition. The transition inspector's **Length
   in beats** sets the overlap by moving the incoming clip's start (clamped by `tidy`).
8. **Kept scenes** (`Video.scenes`) are listed on the left. **+ Add scene** offers: *The scene in Build mode*
   (a copy), *From a file…* (a `.ron`, loaded with `bpe_core::load` and `held` like Open), and *Built-in ▸* (the
   Scenes window's library, one submenu entry per scene). A kept scene gets the scene's name (else "Scene N") and
   the first of the eight track colours no other kept scene uses (cycling when all are used). A kept scene no clip
   plays is left out on save (part 1); its row says so ("Not in the video yet: left out when saved").
9. **Making a clip**: drag a kept scene's row onto the Scenes lane (egui drag-and-drop payload `KeptId`; dropped at
   the pointer's time, snapped), or its row's **Add to the end** (starts where the last clip ends). A new clip plays
   the scene from 0 for its whole Length (cut short at 1800 s by `tidy`).
10. **New video** (the button on the empty arrangement, and the toolbar's New in Video mode) makes a video with the
    open scene kept and played as its first clip at 0. The video takes the scene's size and frame rate, and its
    tempo and time signature, so the first clip looks as it does alone.
11. **Files**: Open accepts `.ron` and `.ballvideo`; a video opens through `bpe_kit::video::open_video` into the
    session and switches to Video mode. Save / Save as in Video mode write `video_save::save` through
    `files::write_safely` (backup `<name> (backup).ballvideo`). The Recent list holds videos too. The title bar in
    Video mode reads "<video name> • – Ball Studio", the name being the file's stem or "Untitled video". Dropping a
    `.ballvideo` on the window opens it.
12. **Asking before losing work**: `Pending` gains `NewVideo`, `OpenVideoPath(PathBuf)` (and Open's dialog routes a
    picked `.ballvideo` there). The confirm dialog asks about the video for those, about the scene for the scene's
    own actions. Closing asks about the scene first (if unsaved), then about the video (if unsaved).
13. **Autosave and recovery**: the session's worker also writes `session-<pid>-<ms>.ballvideo` (+ `.vpath`, the
    video's own file) while the video is unsaved, on the same 60 s rhythm, and clears them once it is saved. A crash
    copy of the video sits beside the scene's (`CrashCopy.video`). An orphaned `.ballvideo` is offered back like a
    scene ("Restore your last work on the video "X"?"), restoring into the session and switching to Video mode.
14. **The preview's still frame** is drawn on a worker thread with its own graphics device (made once, kept), by
    `bpe_cli::arranged::still_frame(device, queue, video, kit, frame, scale, stop)`: each clip on screen runs its
    scene with the quick run-up `final_frame` uses (physics from 0, only the frames the scene's memory needs drawn),
    then the same compose as the export. Asked at scale 0.5; a newer ask stops the one being drawn. The preview
    shows the last frame that came back with a small spinner while a newer one is drawn, "Drop a scene here, or
    + Add scene" over an empty video, and black in a gap.
15. **Playhead**: `VideoSession.time` (video seconds). Click or drag on the ruler to move it; Home / End; Left /
    Right a frame, Shift a beat. Space does nothing in Video mode yet (the status line says "Playing a video comes
    in the next update.").
16. **Clipboard**: Ctrl+C copies the picked clip (`VideoSession.clip_board: Option<Clip>`, the editor's own), Ctrl+V
    pastes it at the playhead (snapped), Ctrl+D duplicates it to just after itself, Delete removes it. Each is one
    undo step. Deleting a kept scene's last clip keeps the scene in the list.
17. **The Export window** gets an `ExportSubject` (`Scene { scene, settings }` or `Video { video, name }`). For a
    video it shows the video's size, fps and length, hides Lighting, stems and MIDI, names the file after the video,
    and runs `bpe_cli::export_arranged`. An empty video greys Export with "Add a clip first".
18. **Not in part 2**: playing, Scene sound lane (hit ticks need each clip's run), Edit scene and Back to video,
    the On top lane. The arrangement shows the ruler, the Scenes lane and the Song lane (waveform).

## File structure

- Create `crates/bpe-studio/src/video_mode/mod.rs`: `VideoSession`, `VideoPick`, `VideoHistory`, `ArrangeView`,
  the mode's three panels' entry points (`left`, `right`, `bottom`, `centre`).
- Create `crates/bpe-studio/src/video_mode/arrange.rs`: the pure edits (Task 1).
- Create `crates/bpe-studio/src/video_mode/files.rs`: new, open, save of the video on `Studio`.
- Create `crates/bpe-studio/src/video_mode/scenes.rs`: the kept scenes list and + Add scene.
- Create `crates/bpe-studio/src/video_mode/inspect.rs`: the Video, Clip, Transition and Scene inspectors.
- Create `crates/bpe-studio/src/video_mode/lanes.rs`: the arrangement (ruler, Scenes lane, Song lane, drags).
- Create `crates/bpe-studio/src/video_mode/still.rs`: the still-frame worker and the centre panel.
- Create `crates/bpe-studio/src/video_mode/tests.rs`: kittest and session tests.
- Modify `crates/bpe-studio/src/grade_mode/mod.rs` (`EditorMode::Video`, the switch's third button),
  `app/mod.rs` (the field, the frame's panels by mode), `app/panels.rs` (toolbar in Video mode),
  `app/shortcuts.rs`, `app/files.rs` (Open's filter, `open_path` by extension, `perform`), `app/dialogs.rs`,
  `app/autosave.rs`, `recovery.rs`, `drops.rs`, `export_window.rs`, `main.rs` (mod), `preview.rs` not touched
  (Video mode's centre is its own).
- Modify `crates/bpe-cli/src/arranged.rs` (`still_frame`) and `crates/bpe-cli/tests/arranged.rs` (a test).
- Modify `docs/notes/video.md` (part 2 as built), this plan's "As built".

## Review Focus

1. **A drag that crosses another clip**: moving a clip past its neighbour reorders them; the transition each clip
   carries stays with its clip; tidy never leaves a clip overlapping a clip two away. Test
   `moving_a_clip_past_its_neighbour_reorders_them`.
2. **Undo after a drag** gives back the video as it was before the drag in one step, not one step per frame. Test
   `a_drag_is_one_undo_step`.
3. **Closing with both a scene and a video unsaved** asks twice, and Cancel on either stops the close. Test
   `closing_asks_about_the_scene_then_the_video`.
4. **Recovery of a video and a scene from the same crashed session**: both are offered (one per start); discarding
   one does not delete the other's files. Test `a_video_copy_is_an_orphan_of_its_own`.
5. **The still frame on an empty video, in a gap, past the end**: black or the empty hint, never a panic. Test
   `still_frame_in_a_gap_is_black`.

---

### Task 1: The pure arranging edits (`video_mode/arrange.rs`)

**Interfaces (all take `&mut Video` and the kit's registry for scene lengths, and end with
`video.tidy(|s| bpe_kit::video::scene_length(s, reg))`):**

- `pub fn snap(time: f64, settings: &VideoSettings, free: bool) -> f64`: Bar/Beat grid from `first_beat`, Off or
  `free` returns `time`; never below 0.
- `pub fn grid_lines(settings, from, to) -> Vec<(f64, bool)>`: the beat times in a range, `true` on a bar.
- `pub fn keep_scene(video, scene: Scene, name: &str) -> KeptId`: next id, the name (else "Scene N"), the first
  free track colour (`TRACK_COLOURS`, the eight `TrackColour` consts).
- `pub fn add_clip(video, reg, kept, start) -> Option<ClipId>`: a clip of the whole scene at `start`, Crossfade in.
- `pub fn add_at_end(video, reg, kept) -> Option<ClipId>`: at `video.end()`.
- `pub fn move_clip(video, reg, id, start)`, `trim_start(video, reg, id, start)`, `trim_end(video, reg, id, end)`.
- `pub fn set_overlap(video, reg, incoming: ClipId, seconds)`.
- `pub fn delete_clip(video, id) -> bool`, `pub fn duplicate_clip(video, reg, id) -> Option<ClipId>`,
  `pub fn paste_clip(video, reg, clip: &Clip, start) -> Option<ClipId>` (new id; refused if its kept scene is gone).
- `pub fn remove_scene(video, kept)`: the scene and its clips.
- `pub fn new_video_from(scene: &Scene, reg) -> Video` (decision 10).

**Steps**
- [x] Write tests first in the same file: snap to bar/beat/off/free and with a first beat; keep_scene names and
  colours (ninth wraps); add_clip plays the whole scene; move then tidy (`moving_a_clip_past_its_neighbour_reorders_them`);
  trim_start keeps the end picture (`end - offset` unchanged) and stops at offset 0; trim_end stops at the scene's
  length; set_overlap clamps to half the shorter clip; duplicate lands after; paste of a clip whose scene is gone is
  refused; new_video_from takes size/fps/tempo.
- [x] Implement. Run `cargo test -p bpe-studio --lib video_mode::arrange`. Expected: all pass.

### Task 2: The session, its history and files (`video_mode/mod.rs`, `video_mode/files.rs`)

- `pub struct VideoSession { pub video: Video, pub saved: Video, pub path: Option<PathBuf>, pub history: VideoHistory,
  pub pick: VideoPick, pub time: f64, pub view: ArrangeView, pub drag: Option<ClipDrag>, pub clip_board: Option<Clip>,
  pub still: still::Still }`.
- `VideoPick { Video (default), Clip(ClipId), Transition(ClipId), Scene(KeptId) }`; a pick whose clip or scene is
  gone reads as Video (`VideoSession::pick()` checks).
- `VideoHistory::{record(before), undo(&mut Video) -> bool, redo, can_undo, can_redo, clear}`.
- `VideoSession::commit(&mut self, before: Video)`: records `before` if it differs, clears redo.
- `Studio::video_unsaved()`, `video_name()`, `new_video()`, `open_video_path(path) -> bool`,
  `save_video(choose) -> bool`, `save_video_to(path) -> bool`, `video_undo()`, `video_redo()`.

**Steps**
- [x] Tests: `a_fresh_video_is_not_unsaved`, `new_video_keeps_the_open_scene_as_its_first_clip`,
  `save_then_open_gives_the_same_video` (temp folder), `undo_and_redo_walk_the_video_steps`,
  `a_broken_video_file_is_a_file_problem`.
- [x] Implement. Run `cargo test -p bpe-studio --lib video_mode::`.

### Task 3: The mode, the toolbar, keys, dialogs and drops (`app/*`, `grade_mode::mode_switch`)

- `EditorMode::Video`; the switch has Build | Grade | Video (hover text "Arrange scenes into one video on a
  song's beat"). D toggles Build/Grade only (in Video mode it does nothing).
- `Studio::ui`: in Video mode the side and bottom panels are `video_mode::left/right/bottom` and the central panel
  is `video_mode::centre`; the Export window gets the video.
- Toolbar in Video mode: New ("Start a new video with the open scene as its first clip"), Open…, Recent, Scenes,
  Save, Save as…, Undo, Redo (the video's), the video's name as a label with its save status; no grid buttons, no
  name box.
- Shortcuts in Video mode: Ctrl+S/Shift+S/O/N, Ctrl+Z/Y, Home, End, Left/Right (frame/beat), Delete, Ctrl+C, Ctrl+V,
  Ctrl+D; Space says the status line words of decision 15.
- `Pending::NewVideo`, `Pending::OpenVideoPath`; confirm dialog by whose work (decision 12); `drops::Dropped::Video`.

**Steps**
- [x] Tests: `the_toolbar_switches_to_video_mode`, `ctrl_z_in_video_mode_undoes_the_video_not_the_scene`,
  `closing_asks_about_the_scene_then_the_video`, `dropping_a_video_opens_it_in_video_mode`,
  `opening_a_video_switches_to_video_mode`.
- [x] Implement, run those tests.

### Task 4: The panels: scenes list, inspector, arrangement (`scenes.rs`, `inspect.rs`, `lanes.rs`)

- Left panel "video scenes" (width `style::TREE_W`): header "SCENES IN THIS VIDEO", one row per kept scene (colour
  swatch, name, length "0:12", "2 clips" / "Not in the video yet: left out when saved"), a click picks it, a drag
  carries its `KeptId`, row menu Add to the end / Remove from the video. **+ Add scene** menu (decision 8).
- Right panel "video inspector" (`settings::INSPECTOR_DEFAULT` wide), by pick:
  - Video: Size W × H (number boxes 16..=4096, even), Frame rate (24, 25, 30, 50, 60 chips), Tempo (20..=400),
    Time signature (top 1..=16, bottom 2/4/8/16 menu), First beat (s), Snap (Bar | Beat | Off), Song (Load song…,
    name and length, ×, On, Starts at, Level dB), Master (Input, Output, Ceiling, Release, End fade).
    A number edit commits when the drag ends (one undo step a drag).
  - Clip: its scene (menu of kept scenes), Start, Length, Offset (s, with "bar 3.2" beside Start), Level (dB),
    Follow video tempo (switch; with it on: "This scene plays at the video's 128 BPM."), Scene song (switch; with it
    and follow both on and the scene having a song: "The scene's song keeps its own speed."), Delete clip.
  - Transition: Kind (Cut, Crossfade, Fade through colour, Flash, Wipe, Push, Zoom through chips), its colour
    (`egui::color_edit_button_srgb`), direction chips, softness 0..30 %, Sound Crossfade | Keep both, Length in beats.
  - Scene: name box, colour (`style::track_picker`), its length, its clips, Add to the end, Remove from the video.
- Bottom panel "arrangement" (resizable, 220 px): a header row (Snap chips, Fit, the playhead as m:ss.ff and bar.beat),
  the ruler (bar numbers, click/drag seeks), the Scenes lane (clips drawn as rounded blocks in their colour with the
  name and "from 0:04.00" when offset > 0; the playhead line), the Song lane. Wheel scrolls, Ctrl+wheel zooms about
  the pointer; Fit shows the whole video. A clip's middle drags to move, its edges (6 px) trim; a click picks it, a
  double-click picks it too (Edit scene comes in part 3). Overlaps are striped and click to pick the transition.
  Dropping a kept scene's payload on the lane adds a clip. The lane's empty state says "Drop a scene here, or + Add
  scene" with a **New video** button when the video has no clips.
- `ArrangeView { px_per_second, left: f64, lane_rect: Option<Rect> }` kept on the session so tests can find where a
  time is (`x_of`, `time_of`).

**Steps**
- [x] Tests (kittest, the whole `Studio::ui` in Video mode at 1440 × 900): `adding_the_build_scene_lists_it`,
  `add_to_the_end_makes_a_clip`, `dragging_a_clip_moves_it_by_whole_bars`, `alt_drags_free`,
  `dragging_the_right_edge_trims`, `dragging_the_left_edge_keeps_the_end_picture`, `a_drag_is_one_undo_step`,
  `overlapping_makes_a_transition_and_clicking_it_picks_it`, `the_transition_inspector_changes_the_kind`,
  `the_video_inspector_changes_the_tempo`, `delete_copy_paste_duplicate_a_clip`.
- [x] Implement, run `cargo test -p bpe-studio --lib video_mode::`.

### Task 5: The still frame (`bpe-cli::arranged::still_frame`, `video_mode/still.rs`)

- `pub fn still_frame(device, queue, video: &Video, kit: &Kit, frame: u32, scale: f32, stop: &AtomicBool)
  -> anyhow::Result<(Vec<u8>, (u32, u32))>`: tidied copy; `moment(frame)`; for each clip on screen a `FrameMaker`
  and `Renderer` at its cover size, the quick run-up (`final_frame`'s memory rule), the samples at its scene frame;
  then a lone same-size clip is read back straight, else `Compose` (a gap is black). Stops with "stopped" when asked.
  `final_frame`'s run-up is moved into a shared helper `run_up(maker, renderer, time, fps, bake, stop)` so both use
  the same code.
- `Still` worker: one thread with its own `headless_device` made on its first job; jobs carry `(Video, frame, gen)`;
  it drains to the newest; the `stop` flag of a job is set when a newer one is sent. Results come back as
  `(gen, pixels, size)` and become an egui texture. The centre panel asks again when the video or the frame changed
  (compared by a cheap key: the video's revision counter on the session and the frame).

**Steps**
- [x] Test in `bpe-cli/tests/arranged.rs`: `still_frame_matches_the_export_frame_outside_a_transition` (a scene with
  no memory: same bytes as `arranged_frames`' frame), `still_frame_in_a_gap_is_black`.
- [x] Test in `video_mode/tests.rs`: `the_still_frame_comes_back` (worker returns a texture within 60 s with a test
  device; skipped like `a_final_frame_comes_back_and_shows` without one).
- [x] Implement, run them.

### Task 6: Export window and recovery for the video

- `export_window::ExportSubject`; `show(ctx, subject, base_dir, kit)`; `start` runs `export_arranged` for a video.
- `recovery`: `Job::WriteVideo`, `Job::ClearVideo`, `Session::{write_video, clear_video}`, `SessionFiles.video,
  vpath`; `Orphan.kind: Work` (`Scene` or `Video`, by extension); `orphans` lists both; `tidy` keeps a lock while a
  `.ballvideo` is there; `Orphan::discard` removes the lock only if the other copy is gone. `CrashCopy.video`.
- `app/autosave.rs`: the video autosaves beside the scene (`autosaved_video`, `video_was_unsaved`); `offer_back`
  reads a video orphan with `open_video`; `restore` puts it in the session (still unsaved, `saved` = what its file
  holds or an empty video) and switches to Video mode.

**Steps**
- [x] Tests: `export_window` `a_video_hides_stems_midi_and_lighting` (kittest on the window), recovery
  `a_video_copy_is_an_orphan_of_its_own`, app `an_unsaved_video_is_autosaved_and_offered_back`.
- [x] Implement, run them.

### Task 7: Notes, checks, review, pull request

- [x] `docs/notes/video.md`: "Part 2: Video mode, arranging (as built)". `docs/notes/editor.md`: one line pointing
  to it. This plan's "As built".
- [x] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, the tests above.
- [x] One Opus review of the whole part; fix what it finds.
- [x] Draft PR, CI green, merge; project memory `multi-scene-video`.

## As built

Built as planned in `crates/bpe-studio/src/video_mode/` (mod, arrange, files, scenes, inspect, lanes, still,
tests) with `bpe_cli::arranged::still_frame` and `bpe_cli::run_up`. The summary for later parts is in
`docs/notes/video.md` ("Part 2 as built"). Changes from the plan, mostly from the review:

- Dragging a clip snaps its head, or its tail when the tail is nearer a line and within about 8 points
  (`snap_move`'s `reach`), so a far tail never pulls the clip.
- A frame rate change tidies every clip onto whole frames in the same undo step.
- New, Open and built-in scenes picked while in Video mode switch to Build, where the scene shows.
- The video's autosave and crash copy compare the session's revision, not the whole video (it holds whole
  scenes); the restore offer names the bare file name.
- The still frame uses the Export liquid detail, as the export does.
- Closing the window asks about the scene, then about the video when it is unsaved too.
- Deferred to part 3: real-time playing, the Scene sound lane, Edit scene / Back to video.
