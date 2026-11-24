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
  (even sides), the middle cropped. Same size is untouched.

### Sound (`bpe-sound::video_mix`)

- `Master::set_bare` / `Engine::set_bare`: Input × Output only (keys and rules still apply), no limiter, ceiling,
  limiter delay or End fade. A scene played alone never sets it.
- `VideoMix` walks the video in 64-sample blocks; a clip's engine (bare, `set_end(offset + length)`) is made when
  its first sample comes up, renders from 0 and throws away its offset, and is dropped after its last sample.
  Fades: equal power (`sin`/`cos` of a quarter turn) over overlaps unless the incoming clip's sound is KeepBoth;
  a 5 ms linear edge at a hard end, and at a cut-in start only when the clip starts partway (offset > 0). The
  song is a `patch::Backing`. Then the video's Master (its limiter delay discarded) and its End fade.
- `video_samples` (tests) and `render_video_sound` (WAV, cancel as `render_sound`). `video_has_sound(video, kit)`.

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

### For part 2 (Video mode: arranging)

Build on `open_video`, `Video::tidy`, `video_save::save`, `moment` and `cover`; the preview's still frame can use
`arranged_frames` with `max_frames` or a `ClipDraw`-like run-up. The editor has nothing for videos yet.
