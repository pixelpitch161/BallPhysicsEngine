# Video arrangement: several scenes in one video (design)

Owner: Aedan. Brainstorm thread 2026-11-24. Mockups: https://claude.ai/artifact/8op18vCUDH4M93eddkE5nu
(direction **B, Arrangement**, picked by Aedan).

## Intent

Today one scene is one file and one export. Aedan wants to make one video out of several scenes. Aedan is a music
producer, so the cut from one scene to the next should land on the beat, the way a drop lands in a DAW
arrangement. The new thing is a **Video**: a song and scene clips laid out on a bar grid, exported as one 9:16 mp4
with sound.

Success: Aedan drags three scenes onto a song, slides them so each change lands on a drop, picks a crossfade or a
flash between two of them, presses Export, and gets one mp4. Each scene in it looks and sounds the same as it does
on its own.

## Goals

- A **Video** file (`.ballvideo`) that holds its song, its scenes and the clips laid out in time.
- **Clips** on a main lane, one after another. A clip plays a scene from some point in it (its *offset*) for its
  length. It can be trimmed at either end and moved. Two clips that overlap make a **transition** whose length is
  the overlap.
- **Transitions**: Cut (no overlap), Crossfade, Fade through colour (black by default), Flash (a colour peaking at the
  middle), Wipe (direction, soft edge), Push (the new scene pushes the old one out, direction), Zoom through (the
  old picture grows and fades, the new one grows in from small).
- **Sound**: one song (optional) across the whole video; each clip's own scene sound with a level; one Master
  (input, true-peak limiter, ceiling, output) over everything. Under a transition the two clips' sounds crossfade
  (equal power).
- **Beat grid**: the video has a tempo and time signature. The timeline counts bars and snaps to bar, beat or off.
- **Video mode** in the editor (Build | Grade | Video): the arrangement timeline, a preview that plays the whole
  video, an inspector for the video, a clip or a transition, and Export.
- **Edit a clip's scene** from the video (double-click) in Build mode, then go back to the video.
- **Export** the video from the editor's Export window and from the command line renderer.
- **Later part: On top lane** for picture in picture and split screens (placement, frame shape, border, its own
  clips).

## Non-goals (for now)

- Physics carrying from one scene into the next (balls flying from scene 1 into scene 2). Each clip's scene starts
  from its own start, which keeps every export the same.
- Effects, grading or lighting across the whole video. Each scene keeps its own Effects and Grade.
- A scene's lighting or Effects changing across a transition (the "lighting change" transition from the mockup).
- Stems and the MIDI file for a video. A video export saves the mp4 only.
- Speeding a clip up or slowing it down, reversing a clip, keyframes on the video.
- Tempo changes within a video.
- More than one main lane.

## Anti-goals

- A scene played inside a video must not look or sound different from that scene exported on its own (apart from
  the size it's fitted to and the tempo, when the clip follows the video's tempo; see below). Outside transitions,
  the frames are the same bytes as the scene's own export.
- Opening a video must never change the scene files it was made from, and editing a scene file must never change a
  video behind Aedan's back (see "Scenes are kept inside the video").
- The editor must not stall while a video plays or while it's arranged. Slow work (running a scene up to a clip's
  offset, working out the sound) happens off the frame.

## Constraints from the project

- Physics is deterministic and pinned by goldens. Nothing in this feature may change a scene's physics or sound
  when it's played on its own. Physics code still uses `bpe_core::maths`.
- Old scene files don't need to keep loading (no backward compatibility yet). The Video file starts at format 1.
- Editor colours come only from `style/` tokens. Clips use each kept scene's track colour (`TrackColour`).
- Exports are cross-built for Windows; ffmpeg as today (`find_ffmpeg`, encoders, BT.709 4:2:0 readback).
- The owner never runs commands: everything is reachable in the editor. The command line gets the same export
  for tests and for CI.

## Approaches considered

1. **Play list** (mockup A): a window listing scenes in order with a join between each. Smallest, but you can't
   see the timing against the music, and you type lengths as numbers.
2. **Arrangement** (mockup B, **picked**): clips on a song timeline with a bar grid. It's the DAW model Aedan
   already knows. It covers the play list, and a second lane gives picture in picture later.
3. **Split screen** (mockup C): scenes at the same time in panes. A different job (races, comparisons). It comes
   back as the On top lane of approach 2.
4. **Screen thing** (mockup D): a thing that plays a scene inside a frame in another scene. Most flexible, but
   every transition would be keyframed by hand. It could come later as its own feature.

## The design

### The Video file

A new data type in `bpe-core` (`video.rs`), saved as RON with the extension `.ballvideo` (format 1), written
through the same safe write the scene uses (temp file, then rename; a backup on save).

```
Video {
  format: 1,
  settings: VideoSettings {
    width: 1080, height: 1920, fps: 60,        // the video's own; scenes are fitted to it
    tempo: 120.0, time_top: 4, time_bottom: Four,
    first_beat: 0.0,                           // seconds into the video where bar 1 starts
    snap: Bar | Beat | Off,                    // editor only
    master: the Mixer's master settings (input, ceiling, release, output, end fade)
  },
  song: Option<Song { name, bytes /* kept */, starts_at: f64, level_db: f32, on: bool }>,
  scenes: BTreeMap<KeptId, KeptScene { name, colour: TrackColour, scene: Scene }>,
  clips: Vec<Clip {
    id: ClipId, scene: KeptId, lane: Main,     // OnTop comes in the later part
    start: f64,                                // video seconds where the clip begins
    length: f64,                               // seconds it plays
    offset: f64,                               // seconds into the scene where it begins
    level_db: f32,                             // the scene's sound in the mix, 0 dB = as it is
    follow_video_tempo: bool,                  // default true
    scene_song: bool,                          // play the scene's own backing track; default false when the video has a song
    transition_in: Transition,                 // how the clip before turns into this one
  }>,
}
Transition { kind: Cut | Crossfade | FadeThrough { colour } | Flash { colour } | Wipe { direction, softness }
             | Push { direction } | ZoomThrough, sound: Crossfade | KeepBoth }
```

- **Time is kept in seconds**, not beats. The editor snaps to the beat grid when you drag. Changing the tempo
  later moves the grid, not the clips. (My call: seconds keep the file simple and match the scene's own times. A
  tempo change after laying clips out is rare, and "Move clips with tempo" can be added later.)
- **Scenes are kept inside the video** (`Video.scenes`), the way a scene keeps its pictures and sounds. Adding a
  scene copies it in. Two clips can play the same kept scene. Editing a clip's scene edits the kept copy, not the
  file it came from. (My call: the video is one file that can't break when a scene file moves, and it never
  changes behind Aedan's back. Ball Studio already keeps pictures, fonts and sounds inside the file.) A kept scene
  no clip uses is removed on save.
- **The main lane** has its clips sorted by `start`. A clip may overlap only the clip before and the clip after
  it, and never a clip two away. Overlap length = transition length. A clip's own `transition_in` describes the
  transition with the clip before it, and is ignored (treated as Cut) when the two don't overlap. Gaps between
  clips are allowed and show black and silence (the song keeps playing).
- **Limits**: `offset >= 0`; `offset + length <= scene Length` (a clip can't play past its scene's end; to play
  longer, raise the scene's Length); `length >= 1 frame`; overlap `<= min(length of either clip) / 2`, so a
  transition never covers half of a clip or more. The video's end is the end of its last clip.
- **Video length**: up to 30 minutes (my call, three times a scene's 10).

### How a clip plays its scene

- A scene the same size as the video is drawn as it is. A scene of another shape is **fitted to cover**: drawn at
  its own shape, just big enough to cover the video (a 1:1 scene in a 1080 × 1920 video is drawn at 1920 × 1920),
  then the middle is cut out. (My call: its blur, glow and lighting then look as they do at its own shape. It costs
  memory, at most two such pictures at once.) Fitting never changes the scene, so physics never changes.
- **Whole frames**: a clip's start, length and offset are always whole frames of the video (rounded when edited
  and on load), so its frames fall on the scene's own frames and transitions are sampled evenly. (My call.)
- Scene time at video time `t` = `t - clip.start + clip.offset`. Frames are drawn at the video's fps, with the
  scene's motion blur samples worked out at that fps.
- A clip with an offset runs its scene from 0 up to the offset first, so it's the same as the scene played from
  the start. In an **export** the run-up draws every frame from 0 exactly as the scene's own export would (the
  scene's Effects and lighting remember earlier frames, `effects/mod.rs`, `memory_cold`), and only the reading back
  is skipped, so the clip's frames are the same bytes. Its sound renders from 0 too (not `Engine::seek`), and the
  part before the offset is thrown away. This costs time for big offsets. (My call: exact over fast in an export,
  because the export is what gets posted.) The **editor** may use the quicker run-up `final_frame` uses (physics
  from 0, the last frames drawn), off the frame.
- **Follow video tempo** (on by default): the clip plays a copy of its scene with the scene's Tempo, time
  signature and First beat set to the video's, the First beat moved so the scene's beats land on the video's
  beat grid (`scene first beat = video first_beat - clip.start + clip.offset`, wrapped to within one bar and kept
  >= 0). Things that move or sound on the beat (rigs, light shows, things born on the beat, music) then land on
  the song's beat. This can change a scene's physics compared with the scene alone, when something in it reads the
  tempo; that is intended, and it's shown in the clip's inspector ("This scene plays at the video's 128 BPM").
  Off: the scene keeps its own tempo.
- **Scene song**: a scene's own backing track plays as part of its sound only when `scene_song` is on. It
  defaults to off when the video has a song, on when it has none. With Scene song and Follow video tempo both on,
  the recording still plays at its own speed; the clip's inspector says so ("The scene's song keeps its own
  speed"). Nothing is refused. (My call.)
- Each scene keeps its own Lighting, Effects, Grade, bake choice and cameras.

### Sound

The video's sound at a moment is:
`Master( song × song level + Σ clips (clip sound before its master × clip level × fade) )`.

- **Before its limiter**: a clip's sound skips its own scene's true-peak limiter, ceiling and end fade, so a
  scene's limiter doesn't pump on its own and then again on the video's. The rest of its Master stays: Input and
  Output gains with their keyframes and the rules that change them, so a scene that ducks or swells its Master
  still does. The scene's buses, faders and bus effects apply as usual. `bpe-sound` gets a switch to render a
  scene that way. (My call.)
- **Fades**: a clip's sound fades in over its incoming transition and out over its outgoing one, equal power
  (`cos`/`sin` of a quarter turn, sound maths, not physics). With `sound: KeepBoth` neither fades over that
  transition. At a Cut, or at the clip's hard end with no transition, the sound gets a 5 ms fade so nothing clicks.
- **Song**: its kept bytes are decoded as a scene's backing track is (same decoder and sample bank); it starts at
  `starts_at` (video seconds, may be negative to skip its intro) with its level. Off mutes it.
- **Master**: the video's Master settings, the same knobs as a scene's Master, in the video inspector.

### Pictures and transitions

Each frame: draw each clip on screen at that moment (one, or two during a transition) into its own finished
picture at the video's size (the scene's whole renderer: lighting, effects, bloom, grade), then put them together
in one new GPU pass in `bpe-gfx` (`compose.rs` + `compose.wgsl`) and read the result back as today.

- Outside a transition the compose pass is skipped and the clip's picture is read back as it is (`Readback`
  learns to read back a texture as well as a `Renderer`, for the composed picture). That keeps
  those frames the same bytes as the scene's own export.
- Transition progress `p` runs 0 to 1 over the overlap, eased smooth (smoothstep) except where noted.
  - **Crossfade**: mix of old and new by `p`.
  - **Fade through colour**: old to colour over the first half, colour to new over the second.
  - **Flash**: crossfade, plus the colour added with a peak at the middle (a quick rise, a slower fall).
  - **Wipe**: a straight edge sweeps across in its direction (up, down, left, right), softness 0 to 30 % of the
    screen.
  - **Push**: the new picture slides in from its direction and pushes the old one out (linear motion, eased).
  - **Zoom through**: the old picture scales up to 3× and fades; the new one scales from 0.6× to 1× and fades in.
- The mixing happens on the finished pictures in the video's colours, the same pictures that would be read back.

### Export

- `bpe-cli` gets `export_arranged(device, queue, video, kit, options, progress)`, the video's partner to
  `export_video`, with the same encoders, CPU fallback, temp folder beside the output, cancel and clean-up, and the
  bar split 90 % frames and 10 % sound.
- The sound renders on its own thread into one WAV (every clip, the song and the Master), while the frames are
  drawn into a video-only mp4. Then the same `mux`.
- Frames: a clip's `FrameMaker` and `Renderer` are made when the clip first comes on screen (after its run-up to the
  offset) and dropped after it ends, so at most two scenes are in memory at once.
- `bpe-render-next` takes a `.ballvideo` as well as a scene (`bpe-render-next video.ballvideo -o out.mp4`), with the
  options that apply (scale, max frames, encoder, crf, preset). `--stems`, `--midi` and `--bake` are refused for a
  video with a plain message.

### The editor: Video mode

- **Build | Grade | Video** in the toolbar. A new project, or a project with only a scene, shows Video mode empty
  with "Drop a scene here, or + Add scene" and a **New video** button that makes a video with the open scene as its
  first clip.
- **File**: Open accepts `.ballvideo` too (and dropping one on the window). Save in Video mode saves the video.
  The title bar names the video. Autosave and recovery cover the video as they cover a scene.
- **Layout in Video mode**:
  - The left panel lists the video's kept scenes (name, colour, length, clips using it), with **+ Add scene**
    (from a file, from the Scenes window's built-in scenes, or the scene open in Build mode).
  - The preview plays the video as it will export, fitted to the panel.
  - The right panel is the inspector: the Video (size, fps, tempo, time signature, first beat, song, Master), a
    clip (scene, start, length, offset, level, follow tempo, scene song) or a transition (kind and its settings,
    sound crossfade or keep both, length in beats).
  - The bottom panel is the **arrangement**: a bar ruler, a Scenes lane, a Song lane (its waveform) and a Scene
    sound lane (each clip's hit ticks in its colour). Later the On top lane goes between Scenes and Song.
- **Arranging**: drag a kept scene onto the Scenes lane to make a clip. Drag a clip's middle to move it, its edges
  to trim (the left edge trims the offset with it, so the picture at the end stays where it is). Drop a clip over
  the end of the one before to make a transition; the overlap shows as a striped block, click it to choose its kind.
  Snapping follows `snap` (Bar, Beat, Off; holding Alt drags free). Delete, copy, paste, duplicate and undo work on
  clips. A clip shows its scene's name, colour and the offset it starts at.
- **Playing**: Space plays and stops, the playhead scrubs, Loop works as it does for a scene. The preview runs each
  clip's scene in its own playback; when the playhead jumps, a clip that has to run up to its point shows a
  "Getting ready…" spinner over the preview while its playback catches up on another thread. Sound in Video mode is
  the video's sound mix, worked out on its own thread ahead of the playhead the way the scene's sound is today, or
  rendered fully in the background and played back, whichever the build finds smooth enough on Aedan's card (the
  builder's call, measured on the PC).
- **Editing a clip's scene**: double-click a clip (or "Edit scene" in its inspector). The editor switches to Build
  mode with that kept scene open and a bar across the top: "Editing *Ring Drop* in the video *Drop Tape*. Back to
  video". Build and Grade work as usual. Back to video (or the Video button) puts the edited scene back into the
  video; undo in the video can take it back as one step. Saving while editing a clip's scene saves the video.
- **Export**: the Export window exports the video when Video mode is on. Its stems, MIDI and bake rows are hidden
  for a video.

### Later part: On top lane (picture in picture, split screen)

- A second lane above Song. Its clips draw over the main lane's picture, inside a frame: a rectangle (x, y, width,
  height as shares of the screen, corner radius), border width and colour, opacity, and a fade in and out.
- Layout shortcuts in a clip's inspector: Top half, Bottom half, Corner (four corners), Side by side left/right,
  2 × 2 slot. A split screen is a main clip and an On top clip set to half each.
- Its sound joins the mix like a main clip, with a pan.

## Parts

1. **Video data and export**: `bpe-core::video` (types, limits, save and load, the transition maths as a pure
   function), `bpe-sound` scene sound without its master and the video mix, `bpe-gfx` cover fit and compose pass,
   `bpe-cli` `export_arranged` and the `.ballvideo` input. Tests below. Nothing in the editor yet.
2. **Video mode: arranging**: the mode switch, the kept scenes list, the arrangement timeline (drag, trim,
   overlap, snap, undo), the inspectors, New video, Open, Save, autosave and recovery, Export from the window. The
   preview shows the still frame at the playhead.
3. **Video mode: playing**: real-time playback of picture and sound, scrubbing with run-ups off the frame, Edit
   scene and Back to video.
4. **On top lane**: picture in picture and split screens.

Each part is its own pull request with an "As built" section in its plan, and its notes go in
`docs/notes/video.md` (new area file, one index line in CLAUDE.md).

## How it's checked

- **Same bytes outside transitions**: a video of two small test scenes (one with an offset, one at offset 0); the
  frames outside the transition equal the frames that scene's own export draws at that scene time (exported at
  the video's size and fps; `Readback` bytes compared).
- **Cover fit**: a 1:1 test scene in a 9:16 video fills the frame, centred (a picture test at 270 × 480).
- **Transitions**: the compose maths is a pure function tested at p = 0, ½, 1 for every kind; one picture test per
  kind at the middle of a transition between two flat-colour scenes, checking sampled pixel colours.
- **Sound**: a video's mix equals the sum of each clip's own limiter-less render times its fades and level, through
  the Master (sample-exact with a Master that does nothing). Equal-power fades sum to constant power. A clip with
  an offset sounds the same as the scene from that point (after its run-up).
- **Follow video tempo**: the scene copy's tempo and First beat are as the formula says, and a rig's beat lands on
  the video's grid.
- **Limits**: every limit above is refused or clamped by `Video` itself (pure tests), and save then load gives the
  same video.
- **Command line**: `bpe-render-next` exports a two-clip video with ffmpeg and the mp4 has the right length and a
  sound track (the test that already does this for a scene, extended).
- **Nothing else moves**: no physics or sound golden line changes in part 1.
- **Editor** (parts 2, 3): kittest tests for dragging, trimming, overlapping, snapping, undo, the inspectors, New
  video, Save and Open, and Edit scene and back. The full-size look check gets a `video-transitions` scene that
  renders the middle of each transition at 1080 × 1920.

## Left to the builder

- Exact module and function names, file splits, and how the compose pass binds two textures.
- How the editor's Video mode shares or doesn't share `Studio`'s one-scene state (a `VideoSession` beside it is
  likely).
- Whether Video mode's sound plays live or from a background render (measure on the PC, part 3).
- The waveform and hit ticks drawing in the arrangement (reuse `timeline::song_lane` and `HitTicks` where they fit).
