# Video arrangement (several scenes in one video)

Spec: `docs/superpowers/specs/2026-11-24-video-arrangement-design.md` (Aedan picked direction B, an arrangement of
scene clips on a song's bar grid). Plans: `docs/superpowers/plans/video/`.

## Part 1: the Video file and its export (as built)

No editor work yet. A video is made in code or by hand and exported from `bpe-render-next`.

### The file (`bpe-core::video`, `bpe-core::video_save`)

- `Video { format: 1, settings: VideoSettings, song: Option<Song>, scenes: BTreeMap<KeptId, KeptScene>, clips: Vec<Clip> }`,
  saved as pretty RON with the extension `.ballvideo` (`video_save::VIDEO_EXTENSION`). Scenes are **kept inside**
  the video (`KeptScene { name, colour: TrackColour, scene }`); a kept scene no clip plays is left out on save, and
  each kept scene's unused pictures and sounds are left out as a scene file's are.
- `VideoSettings`: 1080 × 1920, 60 fps, 120 BPM, 4/4 (`time_bottom` is a plain 2/4/8/16), `first_beat` 0,
  `snap` Bar, `master: VideoMaster { input, output, ceiling -1, release 60, end_fade 0 }`.
  `beat_seconds()` and `bar_seconds()` count beats as a scene does (bottom note scales the beat).
- `Clip { id, scene, start, length, offset, level_db, follow_video_tempo (on), scene_song (off), transition_in }`,
  times in video seconds. `Transition { kind: TransitionKind, sound: Crossfade | KeepBoth }`; kinds Cut, Crossfade,
  FadeThrough { colour }, Flash { colour }, Wipe { direction, softness }, Push { direction }, ZoomThrough
  (`TransitionKind::FADE_THROUGH`, `FLASH`, `WIPE` are the defaults: black, white, left 0.05).
- `Video::tidy(scene_length)` puts clips in their limits, working in whole frames: sort by start, round, offset
  `0..scene_frames-1`, length `1..scene_frames-offset`, a clip that would overlap the clip two before moves to the
  end of the clip before, overlap with the clip before at most half the shorter clip, nothing past 1800 s, clips
  of missing kept scenes dropped. `video_save::load` does **not** tidy; always open through
  `bpe_kit::video::open_video` (load + tidy with each scene's resolved Length).
- `save::load` was split: the mending and cleaning of a loaded scene is `save::mend_loaded`, used by both files.
- Errors (`VideoLoadError`): Newer, Broken, Scene { name, error }, MissingScene { clip }, all in plain words.

### Playing a clip (`bpe-kit::video`)

- `clip_scene(video, clip, reg)` is the scene copy a clip plays: the video's fps always; with Follow video tempo the
  video's tempo, time signature and `first_beat = (video.first_beat - start + offset) rem_euclid bar`; the Mixer's
  `backing_on` = `scene_song && backing_on`.
- `moment(video, frame) -> Moment { old, new, progress, kind }` in whole frames (`OnScreen { clip, scene_time,
  scene_frame }`); a gap or past the end has neither.
- `cover(scene_size, video_size) -> CoverFit { draw, crop }`: drawn at the scene's shape just big enough to cover
  (even sides), the middle cropped. Same size is untouched. `within(fit, max)` shrinks a draw size past the
  card's largest picture (same shape and crop); the export applies it with the device's limit.

### Sound (`bpe-sound::video_mix`)

- `Master::set_bare` / `Engine::set_bare`: Input × Output only (keys and rules still apply), no limiter, ceiling,
  limiter delay or End fade. A scene played alone never sets it.
- `VideoMix` walks the video in 64-sample blocks; a clip's engine (bare, `set_end(offset + length)`) is made when
  its first sample comes up, renders from 0 and throws away its offset, and is dropped after its last sample.
  Fades: equal power (`sin`/`cos` of a quarter turn) over overlaps unless the incoming clip's sound is KeepBoth;
  a 5 ms linear edge at a hard end, and at a cut-in start only when the clip starts partway (offset > 0). The
  song is a `patch::Backing`. Then the video's Master (its limiter delay discarded) and its End fade.
- `VideoMix::set_end` cuts the sound (and moves the End fade) for an export of the first frames only.
  A KeepBoth clip cut in partway still gets the 5 ms edge.
- `video_samples` (tests) and `render_video_sound(video, kit, path, seconds, progress)` (WAV, cancel as `render_sound`). `video_has_sound(video, kit)`.

### Pictures (`bpe-gfx::compose`)

- `Compose` is one pass (two textures, a linear clamp sampler, one uniform `ComposeParams`, output in
  `OUTPUT_FORMAT`) that draws the new picture cropped (`look` 0) or mixes two by a `Look`. `compose::reference` is
  its CPU twin line for line (pure tests); `tests/compose.rs` checks the card against it within 2/255.
- Flash's colour peaks at the middle of the transition (quick rise from 0.2, slower fall to 1).
- `Readback::push_texture` reads back any output-format texture; `push(renderer)` calls it.

### Export (`bpe-cli::arranged`)

- `arranged_frames(device, queue, video, kit, scale, layout, max_frames, each)` tidies a copy, makes each clip's
  scene up front, and per frame draws the clips on screen. A clip's `ClipDraw` (FrameMaker + Renderer at the cover
  size + its own bake choice) is made when it first shows and first draws every scene frame from 0 to its offset
  without reading back (Effects that remember stay exact), then is dropped once the clip ends. A clip alone at the
  video's size is read back straight from its renderer (same bytes as the scene's export); anything else goes
  through `Compose` (a gap is black). A Cut over an overlap shows the new clip alone.
- `export_arranged` refuses stems, MIDI and baking ("Stems, MIDI and baking aren't made for a video yet") and an
  empty video ("This video has no clips"). It shares `export_parts` (the sound thread, the 90/10 bar, mux, cancel
  and clean-up) and `encode_frames` (the ffmpeg child and writer thread) with `export_video`.
- `bpe-render-next <video.ballvideo> [out.mp4]` exports a video (scale, frames, crf, preset, ffmpeg, encoder).

### Tests

`bpe-core` video/video_save, `bpe-kit --lib video::`, `bpe-sound --lib video_mix`, `bpe-gfx --lib compose`,
`bpe-gfx --test compose`, `bpe-cli --test arranged` (same bytes with and without an echo memory, a crossfade mixes,
cover fit, gap black, export with sound and silent, refusals, cancel, the command line). No golden moved.

### Known small gaps (from the part 1 review, left for later)

Not byte-identical at an inexact export scale (goes through compose); scene Length rounded to frames, not
floored; the 1800 s cut can leave a clip all transition; NaN times sort last; the video's own settings (size,
fps, tempo, time signature) aren't clamped; `VideoMix` expects tidied clips; clip ids aren't checked; a song that
won't decode is dropped silently.


## Part 2: Video mode, arranging (as built)

Plan: `docs/superpowers/plans/video/part2-arranging.md` (its "Design decisions" list every call made). All in
`bpe-studio/src/video_mode/` plus `bpe_cli::arranged::still_frame`.

- **Mode**: `EditorMode::Video` (Build | Grade | Video in the toolbar; D still toggles Build/Grade only). In Video
  mode `Studio::ui` draws `video_mode::{bottom, left, right, centre}` instead of the scene's panels, and the Export
  window gets `export_window::Subject::Video`.
- **Session** (`mod.rs`): `Studio.video: VideoSession` always exists (an empty untitled video equals its `saved`,
  so it isn't unsaved). It holds the video, `saved`, `path`, its own `VideoHistory` (whole videos, at most 64 steps;
  `change(merge, f)` records one step, merged while the pointer is held or a text box is focused; `commit(before)`
  for a drag), the pick (`VideoPick` Video / Clip / Transition / Scene; `pick()` falls back to Video when its target
  is gone), the playhead `time`, the arrangement view, the clip clipboard, the still frame and a `revision`.
- **Edits** (`arrange.rs`): pure functions on `Video`, each ending in `Video::tidy`: `snap` (Bar/Beat from the first
  beat, Off or Alt free), `snap_move` (whichever end of the clip is nearer a grid line), `grid_lines`, `bar_beat`,
  `keep_scene` (name or "Scene N", first free of `TRACK_COLOURS`), `add_clip` (whole scene, Crossfade in),
  `add_at_end`, `move_clip`, `trim_start` (start and offset together, end kept, stops at offset 0 and one frame),
  `trim_end`, `set_overlap`, `delete_clip`, `paste_clip`, `duplicate_clip`, `remove_scene`, `new_video_from`
  (the scene's size, fps, tempo, time signature and first beat).
- **Files** (`files.rs`): Open takes `.ron` or `.ballvideo` (`ask_open` → `ask_open_path` → guard by kind);
  `open_video_path` uses `open_video` and switches to Video mode; `save_video(_to)` through `write_safely` with a
  backup; Recent lists both; the title names the video in Video mode. `Pending::NewVideo` / `OpenVideoPath`;
  `must_ask` / `asking_about_video` pick whose work the question is about; closing asks about the scene, then the
  video (`close_scene_answered`). `Pending::Open` is gone (Open picks the file first, then asks).
- **Panels**: `scenes.rs` (kept scenes: swatch, name, length, clip count or "Not in the video yet: left out when
  saved"; a row drags an egui `DragAndDrop` payload `KeptId`, double-click or its menu adds it at the end; + Add
  scene: the Build scene, a file, a built-in); `inspect.rs` (Video: size, fps chips, tempo, time signature, first
  beat, snap, Master, Song with Load song…; Clip: scene menu, start, length, offset, level, follow tempo and scene
  song with their notes, Duplicate, Delete; Transition: kind chips, colour, direction, soft edge, length in beats,
  sound; Scene: name, colour, Add to the end, Remove); `lanes.rs` (header with Snap chips, Fit, playhead; the ruler
  seeks; the Scenes lane draws clips in their colour, overlaps striped with the kind's name; `hit` finds edges
  (6 px, the side the pointer is on where two clips meet), then transitions, then bodies; a drag starts from the
  video as it was (`ClipDrag.before`) every frame and is one undo step; dropping a kept scene's payload makes a clip;
  wheel scrolls, Ctrl+wheel zooms; the Song lane draws the waveform).
- **Keys** (Video mode): Ctrl+S/Shift+S/O/N act on the video, Ctrl+Z/Y on its history, Home/End/Left/Right
  (Shift: a beat) move its playhead, Delete / Ctrl+C / Ctrl+V (at the playhead, snapped) / Ctrl+D on the picked
  clip; Space says playing comes next. A `.ballvideo` dropped on the window opens; a `.ron` dropped in Video mode is
  kept in the video.
- **Still frame** (`still.rs`): one worker thread with its own `headless_device` (made on the first ask, kept),
  newest ask wins, a newer ask sets the old one's stop flag; the last frame stays with a spinner while the next is
  drawn. `bpe_cli::arranged::still_frame` runs each clip on screen with `bpe_cli::run_up` (the Preview final frame's
  quick run-up, now shared with `final_frame`) then composes as the export does, at `STILL_SCALE` 0.5. Off in
  headless studios unless a test sets `still.enabled`.
- **Export**: `Subject::{Scene, Video}`; a video hides Lighting, stems and MIDI, names the mp4 after the video,
  remembers its own Change… path (`video_path`), greys Export with "Add a clip first" when empty, and runs
  `export_arranged`.
- **Recovery**: the session also writes `session-….ballvideo` + `.vpath` (`Session::write_video` / `clear_video`);
  `Orphan.kind` (`Work::Scene` / `Work::Video`); each is offered on its own; discarding one keeps the other and the
  lock goes with the last; `tidy` keeps a lock while a video copy is there; `VideoCrashSlot` beside the scene's.
  `Offer.video` restores into the session and switches to Video mode.
- Part 3 (below) added playing, the Scene sound lane and Edit scene / Back to video.
- Tests: `bpe-studio` `video_mode::` (arrange pure tests; session, files, drags, trims, overlap, inspector,
  keys, closing, autosave and restore, the Export window, the still frame), `recovery::tests::a_video_copy_is_an_orphan_of_its_own`,
  `bpe-cli --test arranged` `still_frame_*`.


## Part 3: Video mode, playing (as built)

Plan: `docs/superpowers/plans/video/part3-playing.md` (its "Design decisions" list every call made).

- **Picture** (`bpe_cli::arranged::Player`): draws a tidied video's frames in order from any frame, as the still
  frame does (quick run-up, no bake), reading each back at once as RGBA. `LiveClip` runs a clip's scene up to its
  first frame a piece at a time (`warm(until)`: physics only via `FrameMaker::advance_to` until the frames the
  scene's memory needs, the rule of `run_up`); `warm_ahead(until)` makes the next clip within `LOOKAHEAD` (4 s) and
  runs it up; `skip(frame)` steps the physics without drawing; ended clips are dropped. `clip_scenes` is shared
  with `arranged_frames`.
- **The worker** (`video_mode/still.rs`): the still-frame thread also takes `Work::Play { from, frames, clock }`;
  frames go through a `sync_channel(PLAYED_AHEAD = 3)`; while it is full the next clip is run up (3 ms at a time);
  more than `BEHIND` (2) frames behind the playhead (`clock`, an `AtomicU32`) it skips. Playing, the take and the
  hits all run liquids at Export detail (`exporting_for_now`), as the still frame and the export do. `Still::play` gives a `PlayFeed` (dropping it
  stops the job); `show_frame`, `shown_from_play` (the stop frame isn't drawn again); older still results never
  overwrite a newer frame.
- **Sound** (`video_mode/take.rs`, `sound_job.rs`): a `Take` renders the whole `VideoMix` from 0 on a low-priority
  thread into memory, in fixed 4 800-frame chunks so growing never copies under the lock the sound thread reads
  through (stops when nobody holds it; `Take::silent` for a video without sound). Takes are made only while
  playing (at Play, and once an edit rests) and let go when leaving Video mode. The sound thread's
  `Command::Video(Some(take))` makes it the source (a second one while playing waits in `next` and takes over once
  `TAKE_AHEAD` (0.25 s) past the playhead); `Seek` moves the read position, `Pause` never touches the engine, filling
  copies blocks and holds (no dropout counted, the clock waits) while the take isn't that far ahead; `Video(None)`
  gives the engine back (the editor seeks it to the Build playhead).
- **Playing** (`video_mode/play.rs`, `VideoSession.playing`): Space / the header's Play–Stop, Loop switch.
  `video_play` → `start_video_at` (picture job, take to the sound thread, *starting*: the preview shows "Getting
  ready…" until the first frame is back and the take is `SOUND_AHEAD` past the playhead; then `SoundJob::play`).
  `video_tick` (from `Studio::tick` in Video mode): a new take once the video rests `TAKE_REST` (0.4 s, no clip drag),
  a jump (playhead ≠ `expected`) starts again there (a jump to the end stops on the last frame, or loops), an edit while playing restarts only the picture (the sound swaps
  takes), the clock is `SoundJob::clock` or the wall clock, the end stops (or loops). Frames are queued (`QUEUE` 4)
  and the newest at or before the playhead is shown. `video_stop` leaves the playhead on the frame on show (its revision kept, so a frame from before an edit is drawn
  again); stopping while getting ready leaves it where it was going; Play on the last frame starts from 0. A picture
  job whose channel closes counts as ready, so an error or the end never leaves it "getting ready". A sound
  thread that appears later is told the take. Dragging the ruler while playing pauses for the drag.
- **Modes**: `Studio::follow_mode` (end of `ui`) follows any change of mode: leaving Video mode stops the video and
  sends `Video(None)` (`leave_video_mode`); entering it pauses the Build scene.
- **Scene sound lane** (`video_mode/hits.rs`, `lanes::scene_sound_lane`): a low-priority thread runs each clip's
  `clip_scene` (physics only, a second at a time, stopping for a newer revision) to `offset + length` and sends its
  `Hit` times shifted into video time, inside the clip; runs are kept by (scene copy, span) for reuse, up to 16.
  Ticks in the clip's colour; "Finding hits…" while busy. `Hits.enabled` (on in the windowed editor).
- **Edit scene** (`video_mode/edit.rs`): double-click a clip or the clip inspector's Edit scene →
  `edit_clip_scene` parks the Build scene (`ParkedScene`: scene, saved, path, history), opens the kept scene with no
  path in Build mode, and `Studio.clip_edit` holds it; the bar "Editing X in the video Y." with Back to video.
  `back_to_video` (button, Video button, any switch to Video mode) puts the scene into the video as one undo step
  (`put_back_clip_scene`, tidied, nothing when the scene wasn't changed) and restores the parked scene (still saved
  if it was) and its playhead. A new video's session starts with no hits. While editing: Save saves the
  video with the edit; New/Open/built-in/New video/Open video and closing go Back to video first; the scene's
  autosave is skipped and the video's autosave/crash copy use `video_with_edit`; `video_unsaved` counts the edit.
- Tests: `bpe-cli --test arranged` `player_*`; `bpe-studio` `sound_job::tests::{a_take_*, a_new_take_*,
  going_back_*}`, `video_mode::tests` (Space, end and Loop, jumps, mode switch, takes after rest, the picture on the
  card, hits, Edit scene and Back, saving and opening while editing, the Video button).
