# Video arrangement, part 1: the Video file and its export. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `.ballvideo` file (song, kept scenes, clips with transitions) that `bpe-cli` exports to one mp4 with
sound. No editor work in this part.

**Architecture:** Data and limits in `bpe-core::video`; the per-clip scene copy (tempo follow, scene song) and the
"what is on screen at t" timing in `bpe-kit::video`; the mix (clips without their limiter, the song, the video's
Master) in `bpe-sound::video_mix`; cover fit and transitions as one GPU pass in `bpe-gfx::compose`; the frame loop
and `export_arranged` in `bpe-cli::arranged`.

**Tech Stack:** Rust (toolchain pinned), wgpu, RON/serde, ffmpeg.

**Spec:** `docs/superpowers/specs/2026-11-24-video-arrangement-design.md` (read it first, sections "The Video
file", "How a clip plays its scene", "Sound", "Pictures and transitions", "Export").

## Global Constraints

- Physics code (bpe-core, bpe-runtime, bpe-kit) never calls `f32::sin`, `exp`, `powf`, `atan2`, `mul_add`; use
  `bpe_core::maths::Maths`. Sound (`bpe-sound`) and shaders may use the platform's maths.
- No physics or sound golden line may change. Don't re-bless anything.
- Picture tests use `bpe_gfx::test_device()`, never `headless_device()`. Run tests with `--test-threads=2`.
- `cargo fmt --all` (width 120) and `cargo clippy --workspace --all-targets -- -D warnings` stay clean.
- Video format 1, extension `.ballvideo`. Video length at most 1800 s. Clip `length >= 1 frame`,
  `offset >= 0`, `offset + length <= scene Length`, overlap `<= min(length of either clip) / 2`, overlaps only with the
  neighbouring clip. Start, length and offset are whole video frames.
- Defaults: 1080 × 1920, 60 fps, 120 BPM, 4/4, first beat 0, snap Bar, Master as the Mixer's defaults
  (input 0 dB, output 0 dB, ceiling -1 dB, release 60 ms, end fade 0 s), clip level 0 dB, follow video tempo on.
- Sound fades: equal power, out = cos(p·π/2), in = sin(p·π/2), p linear over the overlap; 5 ms fade at a cut or a
  hard clip end.
- Plain-language error messages (the owner reads them in the editor later).
- Test run only what the task touches (project rule); CI runs the rest.

## Review Focus

1. **A gap between two clips**: the frames there are black (0,0,0) and the clip sound is silent, while the song
   plays on. Test in Task 5 (`a_gap_is_black`) and Task 3 (`a_gap_is_silent_but_the_song_plays`).
2. **A video with no sound anywhere** (no song, scenes without sound): exports through the one-pass silent track,
   like a silent scene. Test in Task 5 (`a_silent_video_has_a_silent_track`).
3. **A clip that starts at an offset in a scene whose Effects remember earlier frames**: still the same bytes as
   the scene's own export. Test in Task 5 uses a scene with a trail-memory effect (`same_bytes_with_an_offset_and_memory`).
4. **A clip that ends exactly at its scene's Length**: no end fade from the scene, no click. Test in Task 3
   (`a_clip_to_the_scene_end_has_no_scene_end_fade`).
5. **A file whose clip names a kept scene that isn't there, or an empty video**: a plain error, never a panic.
   Tests in Task 1 (`a_clip_with_a_missing_scene_is_refused`) and Task 5 (`an_empty_video_says_so`).

---

### Task 1: The Video data, its limits, save and load (`bpe-core`)

**Files:**
- Create: `crates/bpe-core/src/video.rs` (types, `Video::tidy`, timing helpers), `crates/bpe-core/src/video_save.rs`
  (save and load)
- Modify: `crates/bpe-core/src/lib.rs` (add `pub mod video; pub mod video_save;` and re-exports `Video`, `Clip`)

**Interfaces:**
- Produces:
  - `pub struct Video { pub format: u32, pub settings: VideoSettings, pub song: Option<Song>, pub scenes: BTreeMap<KeptId, KeptScene>, pub clips: Vec<Clip> }`,
    `Video::FORMAT = 1`, `Video::MAX_SECONDS = 1800.0`, `Video::new() -> Video` (defaults above, no clips).
  - `pub struct VideoSettings { width: u32, height: u32, fps: u32, tempo: f32, time_top: u32, time_bottom: u32 /* 2, 4, 8 or 16 */, first_beat: f64, snap: Snap, master: VideoMaster }`
  - `pub struct VideoMaster { input: f32, output: f32, ceiling: f32, release: f32, end_fade: f32 }` (dB, dB, dB, ms, s)
  - `pub enum Snap { Bar, Beat, Off }`
  - `pub struct Song { name: String, audio: crate::scene_audio::SceneAudio, starts_at: f64, level_db: f32, on: bool }`
    (`SceneAudio` already serialises as base64)
  - `pub struct KeptId(pub u32)`, `pub struct KeptScene { name: String, colour: TrackColour, scene: Scene }`
  - `pub struct ClipId(pub u32)`, `pub struct Clip { id, scene: KeptId, start: f64, length: f64, offset: f64, level_db: f32, follow_video_tempo: bool, scene_song: bool, transition_in: Transition }`
  - `pub struct Transition { kind: TransitionKind, sound: TransitionSound }`,
    `pub enum TransitionKind { Cut, Crossfade, FadeThrough { colour: [u8; 3] }, Flash { colour: [u8; 3] }, Wipe { direction: Direction, softness: f32 /* 0..=0.3 */ }, Push { direction: Direction }, ZoomThrough }`,
    `pub enum Direction { Up, Down, Left, Right }`, `pub enum TransitionSound { Crossfade, KeepBoth }`.
    Defaults: `Transition { kind: Cut, sound: Crossfade }`; FadeThrough black, Flash white, Wipe Left 0.05.
  - `Video::tidy(&mut self, scene_length: impl Fn(&Scene) -> f64)`: sorts clips by start, rounds start/length/offset
    to whole frames, clamps every limit in Global Constraints (in that order: offset, length to scene, overlap with
    the clip before, then with the clip after; a clip that would overlap a clip two away is moved to start at the
    end of the clip before), drops clips whose kept scene is missing. `scene_length` is passed in because the
    Length setting is resolved by bpe-kit.
  - `Video::end(&self) -> f64` (end of the last clip, 0 when empty), `Video::frame_count(&self) -> u32`
    (`round(end * fps)`), `Video::overlap(&self, i: usize) -> f64` (overlap of clip `i` with clip `i - 1`, 0 for the
    first), `Video::unused_scenes_removed(&self) -> Video`.
  - `video_save::save(video: &Video) -> String` (pretty RON, unused kept scenes left out, each kept scene's own
    unused pictures and audio left out as `save::save` does) and
    `video_save::load(text: &str, reg: &Registry) -> Result<Video, VideoLoadError>`: probes the format like
    `save::load`, mends each kept scene as `save::load` does (`mend`, `give_colours`, clean cards; factor the shared
    part out of `save::load` into `pub(crate) fn mend_loaded(scene: &mut Scene, reg: &Registry) -> Result<(), LoadError>`
    rather than copying it). It does **not** call `tidy` (bpe-kit does, it knows the Length).
  - `pub enum VideoLoadError { Newer(u32), Broken(String), Scene { name: String, error: LoadError }, MissingScene { clip: u32 } }`
    with `Display`: "This video was made with a newer version of Ball Studio", "This video file is damaged: …",
    "The scene “{name}” in this video didn't open: {error}", "A clip in this video names a scene that isn't in it".

- [ ] **Step 1: Write failing tests** in `video.rs`'s `mod tests` and `video_save.rs`'s `mod tests`:
  - `new_video_has_the_defaults`: every default in Global Constraints.
  - `tidy_rounds_to_whole_frames`: fps 60, start 1.004 becomes 1.0, length 2.009 becomes 2.0167 (121 frames).
  - `tidy_keeps_clips_inside_their_scene`: scene Length 10, offset 4, length 9 becomes length 6; offset -1 becomes 0.
  - `tidy_limits_overlap_to_half_the_shorter_clip`: clips 0..4 and 3..5 (length 2) become overlap 1.0 (start 3.0
    kept, the earlier clip unchanged; the later clip's start moves to 4 - 1 = 3.0 → stays). Then clips 0..4 and
    2.5..4.5: overlap 1.5 > 1.0, the later clip's start moves to 3.0.
  - `tidy_moves_a_clip_that_would_overlap_two_away`: clips A 0..4, B 3..8, C 3.5..9 become C starting at 7.5 or
    later (C's overlap with B limited by the half rule) and never overlapping A.
  - `a_clip_with_a_missing_scene_is_refused`: `load` of a file whose clip names KeptId 9 with no kept scene 9 gives
    `VideoLoadError::MissingScene { clip }`.
  - `save_then_load_is_the_same_video`: a video with a song (8 bytes), two kept scenes, three clips with Wipe and
    Flash transitions round-trips equal.
  - `a_newer_video_says_so`: format 2 gives `Newer(2)`.
  - `unused_kept_scenes_are_left_out_of_the_file`.
- [ ] **Step 2: Run** `cargo test -p bpe-core video -- --test-threads=2`. Expected: fail to compile (no `video`).
- [ ] **Step 3: Implement** the interfaces above. Derive `Clone, Debug, PartialEq, Serialize, Deserialize` with
  `#[serde(default)]` on structs, as `Scene` does. Rounding: `(x * fps).round() / fps`.
- [ ] **Step 4: Run** the same command. Expected: all pass. Run `cargo test -p bpe-core save` too (the refactor of
  `save::load`). Expected: pass.
- [ ] **Step 5: Commit** "Video: the file (clips, transitions, song, kept scenes) and its limits".

### Task 2: Playing a clip's scene, and what's on screen when (`bpe-kit`)

**Files:**
- Create: `crates/bpe-kit/src/video.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub mod video;`)

**Interfaces:**
- Consumes: Task 1's types.
- Produces:
  - `pub fn scene_length(scene: &Scene, reg: &Registry) -> f64` (resolved Scene settings `duration`).
  - `pub fn open_video(text: &str, reg: &Registry) -> Result<Video, VideoLoadError>`: `video_save::load` then
    `tidy(|s| scene_length(s, reg))`. Every caller opens videos through this.
  - `pub fn clip_scene(video: &Video, clip: &Clip, reg: &Registry) -> Scene`: a copy of the kept scene with, when
    `follow_video_tempo`, root `tempo`, `time_top`, `time_bottom` (as `TimeBottom`) set to the video's and
    `first_beat` = `video.first_beat - clip.start + clip.offset` wrapped into `[0, bar)` (`bar` = seconds in one
    video bar; use `rem_euclid`, it's exact); and the root Mixer's `backing_on` = `clip.scene_song && backing_on`.
    Nothing else changes.
  - `pub struct OnScreen { pub clip: usize, pub scene_time: f64 }` and
    `pub struct Moment { pub old: Option<OnScreen>, pub new: Option<OnScreen>, pub progress: f32, pub kind: TransitionKind }`:
    `pub fn moment(video: &Video, frame: u32) -> Moment`. Outside a transition only `new` is set (or neither, in a
    gap or past the end). During clip `i`'s incoming overlap, `old` is clip `i - 1`, `new` is clip `i`,
    `progress = (t - clip_i.start) / overlap` in `[0, 1)`, `kind` clip `i`'s. Times are frame-exact:
    `t = frame / fps`, `scene_time = t - start + offset`, computed in whole frames to avoid drift
    (`(frame - start_frame + offset_frame) / fps`).
  - `pub fn cover(scene_size: (u32, u32), video_size: (u32, u32)) -> CoverFit` with
    `pub struct CoverFit { pub draw: (u32, u32), pub crop: [f32; 4] /* x, y, w, h in the drawn picture, 0..1 */ }`:
    `draw` is the scene's shape scaled so both sides are `>=` the video's (rounded up to even numbers), `crop` the
    middle part with the video's shape. Same size gives `draw == video` and crop `[0, 0, 1, 1]`.
- [ ] **Step 1: Write failing tests** in `video.rs`'s `mod tests`:
  - `following_the_tempo_sets_the_scene_beat_grid`: video 128 BPM 4/4 first beat 0.5, clip start 3.0 offset 2.0:
    the clip scene's resolved tempo 128, time_top 4, Four, first_beat = (0.5 - 3 + 2) rem_euclid 1.875 = 1.375.
    And for every scene time `s` in a few samples, the beat phase `((s - 1.375) / beat).rem_euclid(4)` equals the
    video's `((s - offset + start - 0.5) / beat).rem_euclid(4)` to 1e-9.
  - `not_following_keeps_the_scene_tempo`.
  - `scene_song_off_mutes_the_backing_track`.
  - `moment_finds_old_and_new_in_a_transition`: clips 0..2 s and 1.5..4 s at 60 fps: frame 30 is only clip 0;
    frame 90 has old 0 and new 1 with progress 0.0; frame 105 progress 0.5; frame 120 only clip 1 with scene_time
    `0.5 + offset`.
  - `moment_is_empty_in_a_gap_and_after_the_end`.
  - `cover_fits_a_square_scene`: (1080, 1080) into (1080, 1920) draws (1920, 1920), crop `[0.21875, 0, 0.5625, 1]`.
  - `cover_of_the_same_size_is_untouched`.
- [ ] **Step 2: Run** `cargo test -p bpe-kit --lib video -- --test-threads=2`. Expected: fail to compile.
- [ ] **Step 3: Implement.** Use `bpe_core::maths` if any trig is needed (none should be).
- [ ] **Step 4: Run** the same. Expected: pass.
- [ ] **Step 5: Commit** "Video: a clip's scene follows the video's tempo; what's on screen at each frame".

### Task 3: The video's sound (`bpe-sound`)

**Files:**
- Modify: `crates/bpe-sound/src/master.rs` (`Master::set_bare(bool)`), `crates/bpe-sound/src/engine.rs`
  (`Engine::set_bare(bool)`: the master skips its limiter and ceiling, and `fade_end` does nothing)
- Create: `crates/bpe-sound/src/video_mix.rs`; modify `lib.rs` (`pub mod video_mix;` and re-export
  `render_video_sound`, `video_has_sound`)

**Interfaces:**
- Consumes: Tasks 1-2 (`clip_scene`, `Video`).
- Produces:
  - `Master::set_bare(&mut self, bare: bool)`: when bare, `process` applies `input × output` gain only (both still
    smoothed, keyed and ruled), no limiter, no ceiling. Default false, so nothing changes for a scene.
  - `Engine::set_bare(&mut self, bare: bool)`.
  - `pub fn video_has_sound(video: &Video) -> bool`: the song is on, or any clip's `clip_scene` has sound
    (`has_sound`).
  - `pub fn video_samples(video: &Video, kit: &Kit) -> Vec<f32>` (interleaved stereo, `round(end × 48000)` frames;
    for tests) and `pub fn render_video_sound(video: &Video, kit: &Kit, path: &Path, progress: impl FnMut(f64) -> bool) -> anyhow::Result<()>`
    (the same in chunks to a WAV with `WavOut`, cancel like `render_sound`: "sound render cancelled", file removed).
    Both share one streaming mixer `struct VideoMix` that walks the video in 64-frame blocks and keeps an engine
    only for clips heard now: a clip's `Engine::offline(clip_scene)` with `set_bare(true)` and `set_end(offset + length)`
    is made when its first sample comes up, renders and throws away its first `offset` seconds (from 0, not
    `seek`), then feeds its samples through a small queue so clip starts need not fall on a block edge; it is
    dropped after its last sample.
  - Per sample: `clip × db_to_gain(level_db) × fade`, fade = equal power over the clip's incoming and outgoing
    overlaps (skip a fade when that transition's `sound` is `KeepBoth`), 5 ms linear fade at a Cut (no overlap)
    edge and at the clip's own end. Song: `patch::Backing` built from `audio::decode(&song.audio.bytes)`,
    `start = round(starts_at × 48000)`, gain `db_to_gain(level_db)`, added when `on`. Then the video's `Master`
    (made from a `MixerSettings` holding `VideoMaster`'s values; not bare), then its end fade
    (`end_fade_gain`, reused) over the video's end.
- [ ] **Step 1: Write failing tests** in `video_mix.rs`'s `mod tests` (use the `test-tone` feature's scenes as
  `bpe-cli/tests/export.rs` does: scenes tagged `test-tone…` sound):
  - `a_bare_master_is_input_times_output`: Master with input +6 dB and a ceiling of -20 dB, bare, lets a 0.5
    amplitude block out at 0.5 × 2 (to 1e-3) once smoothed.
  - `one_clip_is_its_scene_without_its_limiter`: a video of one clip (offset 0, length 1 s, level 0 dB, Master at
    input 0 output 0 ceiling 0 dB) equals the scene's own bare render, sample for sample (the 5 ms end fade aside:
    compare the first 0.99 s).
  - `an_offset_clip_is_the_scene_from_there`: offset 0.5 s equals the bare scene render from 0.5 s on (same
    comparison window).
  - `a_crossfade_keeps_constant_power`: two clips of the same steady tone overlapping 0.5 s: the RMS over each
    10 ms window inside the overlap is within 0.5 dB of the RMS outside it.
  - `keep_both_does_not_fade`.
  - `a_gap_is_silent_but_the_song_plays`: clips 0..1 and 2..3, a song of a steady tone at 0 dB: 1.2..1.8 s is the
    song alone (no clip sound); with the song off, it is exact zeros.
  - `a_clip_to_the_scene_end_has_no_scene_end_fade`: a scene with end fade 1 s, clip to its full Length: the clip's
    last 0.5 s isn't faded by the scene (only the 5 ms edge).
  - `level_moves_the_clip`: -6 dB halves the amplitude (to 1e-2).
- [ ] **Step 2: Run** `cargo test -p bpe-sound --features test-tone video_mix -- --test-threads=2`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, then `cargo test -p bpe-sound --test golden_sound -- --test-threads=2` (or the
  sound golden test's real name: `ls crates/bpe-sound/tests`). Expected: pass, goldens unchanged.
- [ ] **Step 5: Commit** "Video: the sound (each clip without its limiter, the song, one Master)".

### Task 4: Reading back any texture, cover fit and transitions on the card (`bpe-gfx`)

**Files:**
- Modify: `crates/bpe-gfx/src/readback.rs` (`push_texture`; `push` calls it)
- Create: `crates/bpe-gfx/src/compose.rs`, `crates/bpe-gfx/src/shaders/compose.wgsl`; modify `lib.rs`
  (`pub mod compose;`)

**Interfaces:**
- Consumes: nothing from earlier tasks (bpe-gfx doesn't depend on bpe-kit; it gets plain numbers).
- Produces:
  - `Readback::push_texture(&mut self, device, queue, texture: &wgpu::Texture, view: &wgpu::TextureView) -> anyhow::Result<Option<Vec<u8>>>`,
    checks `texture` size; `push(renderer)` becomes `push_texture(renderer.output_texture(), renderer.output_view())`.
  - `pub enum Look { Crossfade, FadeThrough([f32; 3]), Flash([f32; 3]), Wipe { direction: u32 /* 0 up 1 down 2 left 3 right */, softness: f32 }, Push { direction: u32 }, ZoomThrough }`
    (bpe-cli maps `TransitionKind` to it).
  - `pub struct ComposeParams` (the uniform, `bytemuck::Pod`) and
    `pub fn params(look: Option<Look>, progress: f32, old_crop: [f32; 4], new_crop: [f32; 4]) -> ComposeParams`,
    pure. `look: None` is one picture (the new one), cropped.
  - `pub fn reference(params: &ComposeParams, uv: [f32; 2], old: impl Fn([f32; 2]) -> [f32; 4], new: impl Fn([f32; 2]) -> [f32; 4]) -> [f32; 4]`:
    the CPU twin of the shader, for tests.
  - `pub struct Compose`, `Compose::new(device, width, height) -> Compose`,
    `Compose::draw(&mut self, device, queue, old: Option<&wgpu::TextureView>, new: Option<&wgpu::TextureView>, params: &ComposeParams)`
    (a `None` side is black), `Compose::output_texture()`, `Compose::output_view()`. Output format = the
    renderer's output format (`Rgba8Unorm`, check `renderer/new.rs`), linear sampler, clamp to edge.
  - The looks, with `e = smoothstep(0, 1, progress)` (Push uses `e` for motion; Wipe uses `progress` linear):
    Crossfade `mix(old, new, e)`; FadeThrough `mix(old, c, 2e)` for `e < ½` else `mix(c, new, 2e − 1)`; Flash
    `mix(old, new, e) + c × f` with `f = e/0.35` rising to 1 at 0.35 then `(1 − e)/0.65` falling, clamped to 1;
    Wipe edge at `progress` across the screen in its direction, soft over `softness` of the screen; Push the new
    picture offset by `(1 − e)` of the screen from its side and the old by `−e`; ZoomThrough old scaled about the
    centre by `1 + 2e` and faded by `1 − e`, new scaled `0.6 + 0.4e` (pixels outside it black) and faded by `e`,
    summed.
- [ ] **Step 1: Write failing tests:**
  - `compose.rs` `mod tests` (pure, no card): `crossfade_is_old_then_half_then_new` (p 0, ½, 1 with old red and new
    blue: red, (0.5, 0, 0.5), blue), the same three for FadeThrough (½ is black), Flash (½ is clamped white-ish:
    every channel `>= 0.9`), Wipe Left (at p ½ the left quarter is new, the right quarter old; direction Left means
    the edge moves leftward from the right, so pin it in the test name `wipe_left_moves_right_to_left`), Push,
    ZoomThrough (at p 0 the middle pixel is old, at p 1 new), `one_picture_is_cropped` (crop `[0.25, 0, 0.5, 1]`
    samples the old texture's x 0.25 at the screen's left edge).
  - `crates/bpe-gfx/tests/compose.rs` (needs the card via `test_device()`): `the_shader_matches_the_reference`:
    two 16 × 16 flat textures (red, blue), every look at p 0.25, 0.5, 0.75 at 32 × 64 output; sampled pixels equal
    `reference` within 2/255. `push_texture_reads_back_a_compose`: RGBA readback of a crossfade at ½.
- [ ] **Step 2: Run** `cargo test -p bpe-gfx --lib compose -- --test-threads=2` and
  `cargo test -p bpe-gfx --test compose -- --test-threads=2`. Expected: fail to compile.
- [ ] **Step 3: Implement.** One render pass, a full-screen triangle, two textures and one uniform. The WGSL repeats
  `reference`'s maths line for line.
- [ ] **Step 4: Run** both, plus `cargo test -p bpe-gfx --lib readback -- --test-threads=2`. Expected: pass.
- [ ] **Step 5: Commit** "Video: cover fit and transitions on the graphics card; read back any texture".

### Task 5: Drawing and exporting a video (`bpe-cli`)

**Files:**
- Create: `crates/bpe-cli/src/arranged.rs`; modify `crates/bpe-cli/src/lib.rs` (`pub mod arranged;`, re-export
  `export_arranged`), `crates/bpe-cli/src/main.rs` (`.ballvideo` input)
- Test: `crates/bpe-cli/tests/arranged.rs`

**Interfaces:**
- Consumes: Tasks 1-4 (`open_video`, `clip_scene`, `moment`, `cover`, `render_video_sound`, `video_has_sound`,
  `Compose`, `params`, `Readback::push_texture`), and in `lib.rs` `FrameMaker`, `bake_for`, `Picture`, `spawn`,
  `mux`, `resolve_encoder`, `retry_on_cpu`, `CancelOnDrop`, `AUDIO_ARGS` (make what's needed `pub(crate)`; don't
  copy them).
- Produces:
  - `pub fn arranged_frames(device, queue, video: &Video, kit: &Kit, layout: PixelLayout, max_frames: Option<u32>, each: impl FnMut(u32, Vec<u8>) -> bool) -> anyhow::Result<()>`:
    the frame loop, used by the export and the tests. For each frame: `moment`; each clip on screen has a
    `ClipDraw { scene: Scene /* clip_scene */, maker: FrameMaker, renderer: Renderer /* at cover.draw */, bake, next: u32 /* next scene frame to draw */ }`
    made the first time the clip is on screen and dropped once the loop passes its end. Before its first frame a
    `ClipDraw` draws every scene frame `0..offset_frames` with `maker.samples` + `renderer.render_samples` and
    no readback (exact run-up). Each frame then draws the scene frame `scene_time × fps`. One clip, same size: read
    back `renderer` directly (same bytes). Otherwise `Compose::draw` and `push_texture`. Nothing on screen:
    `Compose::draw(None, None)` (black). Scenes keep their own bake choice (`bake_for(scene, kit, None)`).
    `FrameMaker` borrows its scene: keep the `Scene` boxed in the `ClipDraw` and build the maker from it
    (or own the scenes in a `Vec<Scene>` made up front; the builder's choice).
  - `pub fn export_arranged(device, queue, video: &Video, kit: &Kit, options: &ExportOptions, progress: impl FnMut(u32, u32) -> bool) -> anyhow::Result<Encoder>`:
    as `export_video`: refuses `options.stems`, `options.midi` or `options.bake == Some(_)` with
    "Stems, MIDI and baking aren't made for a video yet", refuses an empty video with "This video has no clips";
    same encoder choice and CPU retry, temp folder, sound thread with `render_video_sound` when
    `video_has_sound`, else one pass with the silent track; same 90/10 bar, cancel and clean-up, then `mux`.
    The draw loop of `draw_video` (ffmpeg child, writer thread) should be shared, not copied: split
    `draw_video` so its ffmpeg part takes a frame source closure, and use it from both.
  - `main.rs`: a path ending `.ballvideo` opens with `open_video` and runs `export_arranged`; output defaults to the
    video's name with `.mp4`. Usage text gains `bpe-render-next <video.ballvideo> [out.mp4] [options]`.
- [ ] **Step 1: Write failing tests** in `tests/arranged.rs` (helpers as in `tests/export.rs`: `ready()`,
  `temp_dir`; small scenes at 270 × 480 made by `bpe_kit::new_studio_scene` plus a ball, Length 2 s, video
  270 × 480 at 30 fps):
  - `same_bytes_outside_transitions`: clips A (offset 0, 0..1.5 s) and B (offset 0.5 s, 1.0..2.5 s,
    Crossfade): frames 0..29 equal scene A's own frames 0..29, frames 45..74 equal scene B's own frames 30..59
    (own frames from a test loop of `FrameMaker::samples` + `Renderer` + RGBA `Readback`, as `draw_video` draws).
  - `same_bytes_with_an_offset_and_memory`: the same with B's scene given an Effects layer that remembers frames
    (pick one with `Frame::memory() > 0`, e.g. a trail or echo effect from `bpe-kit/src/modules/effects/`).
  - `a_transition_mixes_the_two`: at the middle frame of the crossfade every sampled pixel is within 2/255 of the
    average of the two clips' own frames at those scene times.
  - `a_square_scene_covers_the_video`: a 480 × 480 scene in a 270 × 480 video: no black column at the sides and
    the scene's centre pixel lands at the frame's centre.
  - `a_gap_is_black`.
  - `exports_a_two_clip_video_with_sound` (ffmpeg): `export_arranged` with `Encoder::Cpu`: ffprobe shows video
    270 × 480, an audio stream, and duration within 0.1 s of `video.end()`.
  - `a_silent_video_has_a_silent_track` (ffmpeg): scenes without sound, no song: an audio stream exists.
  - `an_empty_video_says_so` and `stems_are_refused_for_a_video`: error text as above, no file left.
  - `a_cancelled_video_export_leaves_no_file`.
- [ ] **Step 2: Run** `cargo test -p bpe-cli --test arranged -- --test-threads=2`. Expected: fail to compile.
- [ ] **Step 3: Implement** `arranged.rs`, the `draw_video` split and `main.rs`.
- [ ] **Step 4: Run** the same, then `cargo test -p bpe-cli --test export -- --test-threads=2` (the split of
  `draw_video` mustn't change a scene's export). Expected: pass.
- [ ] **Step 5: Commit** "Video: draw and export a video; bpe-render-next takes a .ballvideo".

### Task 6: Notes, checks, review, pull request

**Files:**
- Create: `docs/notes/video.md` (the area's notes: the file, limits, timing, sound, compose, export; what part 2
  builds on)
- Modify: `CLAUDE.md` (one Area index line: "Video arrangement (several scenes in one video, transitions, the
  mix) | `video.md` | `specs/2026-11-24-video-arrangement-design.md`, `plans/video/`"), this plan (an "As built"
  section at the end)

- [ ] **Step 1:** Write the notes and the As built section (what changed from this plan and why).
- [ ] **Step 2: Run** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and the debug
  physics goldens `cargo test -p bpe-kit --test golden -- --test-threads=2` (confirm none moved). Expected: clean,
  pass.
- [ ] **Step 3:** One review of the whole branch by an Opus subagent (project rule), fix what it finds.
- [ ] **Step 4:** Commit, push, open the draft PR, watch CI, merge when green (project rule: threads merge their
  own PRs after checks and review). Don't build or install the exe.
