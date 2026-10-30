# Big worlds and a camera: design

Date: 2026-10-30. Status: approved in conversation, section by section; awaiting review of this document.

## Goal

Today the world is the video: world units are video pixels, `0,0` is the centre of a 1080 x 1920
frame, and the renderer, the preview, the scores and the dice all assume that box. This change
makes a scene an endless world, and adds a Camera thing that decides what the video shows. The
camera can follow things, be keyframed, and react to hits. The editor can show the exact shot or
a free view of the whole world.

## Decisions (Aedan's picks)

1. The camera does everything: follows things, takes keyframes on the timeline, and shakes or
   punches its zoom on hits.
2. The world is endless by default, with an optional world box the camera can't leave.
3. The editor preview has both views behind a switch: Camera (the exact shot) and World (a free
   canvas with the camera drawn as a draggable 9:16 frame).
4. One camera for now. The design leaves room for several cameras with cuts later, without
   changing scene files.
5. Every thing gets a "Stick to screen" switch. Text and the background start stuck. Everything
   else starts in the world.
6. Sound follows the camera: hits outside the shot fade with distance and pan by position. A
   Scene setting of 0 gives the old behaviour.
7. Build approach A, "the camera is a lens": physics and things stay in world positions, and the
   renderer looks through the camera. (Rejected: every draw part shifting its own shapes; drawing
   the whole world into one huge picture and cropping it.)

## Assumption (not asked, stated in conversation and not corrected)

A scene saved before this change opens with the camera at `0,0`, Zoom 1, Turn 0, no follow,
nothing on hits, the background stuck and the sound settings at 0. Its pictures and sound come
out exactly as before.

## Units

- World units stay as they are: one unit is one pixel of a 1080-wide video at Zoom 1, `y` down.
- **Screen space** is today's world space. It is 1080 units across the video, with `0,0` at the
  centre and the height following the video's aspect ratio. Stuck things are placed in it.
- The camera maps world to screen: `screen = rotate(world - position, -turn) * zoom`. At Zoom 1,
  the shot is 1080 world units across, whatever the export size. A 720p export frames the same
  shot as a 1080p one.

## 1. The Camera thing

There is a new thing kind, `camera`, in `bpe-kit/src/things/camera/`, with Settings derive, sim
and draw parts, registered like every other kind. `bpe_kit::ensure_camera` adds one, named
"Camera", directly under the root, whenever the editor or the CLI opens, creates or loads a scene
that has none. It is always the newest thing, so no other thing's id moves. A scene with no camera
(for example the built-in scene builders the golden tests use) behaves as an identity camera. The tree won't let you delete the live camera, and the add menu offers no second
camera for now.

The settings use foldable cards. Each setting has a `when` condition, so a setting only shows when
it applies. There are no Simple/More groups.

| Card | Setting | Notes |
|---|---|---|
| Shot | Position (Vec2), Zoom (0.05 to 20, log slider), Turn (degrees) | All keyframeable. Position is the thing's own transform position, so it drags like any thing. |
| Follow | Follow: None / Thing / Tag / All balls | A target picker, reusing the existing Target type. |
| | Follow amount 0 to 1 | Keyframeable. Blends the keyed shot (0) and the followed point (1). |
| | Smoothness 0 to 1 | How lazily the camera catches up (critically damped spring). |
| | Look ahead (seconds) | Leads the target by its velocity times this. |
| | Slack (share of the shot) | A middle area where the target can move without the camera moving. |
| | Fit all, plus Fit margin | Zooms out so every followed ball stays in shot. It never zooms in past the Zoom setting. |
| On hits | Shake strength, Shake fade (s) | Trauma model: each hit adds `strength * hit speed / reference`, which decays over the fade, and the offset is smooth noise from the seed. |
| | Zoom punch amount, Punch recovery (s) | Each hit adds a punch, multiplying the zoom by `1 + amount`, which eases back over the recovery. |
| | Hits from (Target) | Defaults to all balls. |
| World box | Stay inside (bool), Box size (Vec2), Box centre (Vec2) | When it's on, the shot is clamped so no part of it leaves the box. If the box is smaller than the shot, the shot is centred on the box. |

**Runtime.** The camera's sim keeps a `CameraState` (smoothed position, velocity, zoom, shake
trauma, punch level, shake noise phase) as its `ThingState` in `World.states`. The sim advances
it every physics step from the step's keyed settings, the target's live positions, and the hit
events of that step. It writes the resulting `CameraPose { center, zoom, turn }` to
`World.poses`. Because it lives in the world, checkpoints and scrubbing restore it, so straight
playback, scrubbing and export always give the same shot. The camera is not a body and nothing
collides with it.

The final pose is computed in this order: keyed shot, then blend with follow by Follow amount,
then Fit all, then shake and punch, then the world box clamp.

**Several cameras later.** The camera is an ordinary thing kind marked "one per scene", so
allowing more later is just lifting that limit and adding a "live camera" setting that defaults
to the only one. Existing files need no change.

## 2. Drawing through the camera

- `Frame` gets `camera: CameraView { center, zoom, turn, screen_width: 1080.0 }`.
  `build_frame_at` fills it from the live camera's pose (identity when there is no camera).
- `Frame` gets an `overlay: Option<Box<Frame>>`. `build_frame_at` draws every thing whose
  "Stick to screen" is on into the overlay instead of the main frame, so draw parts don't change.
  The renderer draws the main frame's scene passes through the camera, then the overlay's scene
  passes with no camera, before bloom and the screen effects. The switch is a new
  `screen: Option<bool>` on `Thing` (`None` means the kind's default, from a new
  `ThingKind::sticks_to_screen()` that is true for Text). It is shown on the thing's first card,
  and hidden for Liquid, Swarm, the Camera and Scene defaults.
- The shader `Globals` gets a world-to-clip matrix and a screen-to-clip matrix. `to_clip` takes
  the layer into account. The background gradient uses screen space when stuck, and world space
  (spanning the world box, or a 1080 x 1920 area around the camera's start when there is no box)
  when not.
- **Culling.** Draw parts skip items whose bounds are outside the shot plus a margin. The
  physics still runs everywhere.
- **Liquid and swarms (bpe-gfx flow).** Their simulation area stays in world space and must
  never depend on the camera, so that physics is the same whatever the shot. `arena_size`
  stops falling back to the video size: with no wall it uses the thing's own area setting.
  Only their drawing (the surface pass and the swarm draw) goes through the world matrix. The
  liquid's screen-sized surface texture is rendered in camera space.
- **Effects.** Screen effects (bloom, kaleidoscope, echo, film, pixels, and so on) stay
  screen-wide. Anything that turns a hit's world position into a pixel (`pictures.rs`,
  `effects/chain.rs`) first maps it through the camera.
- **Trails** stay in the world. Text keeps its current behaviour while stuck. Unsticking it places
  it at the same spot in the world.

## 3. The editor (bpe-studio)

- A **Camera / World** switch above the preview, remembered per scene (editor state, not in the
  scene file).
- **Camera view** shows the exact shot. The preview `View` gains the camera: `to_screen` and
  `to_world` go through it, so dragging, hit testing and handles keep working when the camera
  has moved, zoomed or turned.
- **World view** is a free canvas. You scroll to zoom around the pointer, middle-drag or
  space-drag to pan, and press F to frame the selected thing (or everything if nothing is
  selected). The camera is drawn as a 9:16 frame with a centre handle (move), corner handles
  (zoom) and a turn handle. Dragging it while a camera setting is keyed adds a key, the same as
  other keyed edits. During playback the frame moves and leaves a faint path of the next two
  seconds. The world box is a dashed outline. Things outside the shot are drawn normally, and
  the area outside the shot is dimmed slightly.
- **Grid and snap** (being built in another thread) must work in both views. The World view
  grid covers the whole canvas. This plan follows whatever API that work lands with.
- **Adding a thing** places it at the centre of the current view.
- Inspector text that says "the video is 1080 pixels wide" changes to "world units: 1080 is the
  width of the shot at Zoom 1".

## 4. Sound, scores and dice

- **Sound.** The mixer gets the camera pose at every hit (from the run's log of poses, sampled at
  the physics rate). Sound modules already pan hits by their horizontal position with their own
  Stereo width setting. That pan now uses the hit's position on screen, through the camera,
  clamped to full left or right off-screen, so no separate Scene spread setting is needed. There
  is one new Scene setting in a "Camera sound" group:
  - **Off-shot fade:** the distance past the shot edge, in shot widths, where a hit reaches
    silence. 0 means off.

  Old scenes open with it at 0. New scenes start with 1.5. This applies to hit sounds only.
  Beats, drums on the beat, and the song are unaffected.
- **Scores.** `score::sample` counts a ball as on screen when it is inside the camera's shot at
  that moment, not inside the fixed box.
- **Dice.** `random.rs` sizes arenas to fit the camera's starting shot instead of the fixed 520
  radius.

## Keeping things safe

- All 30 scenes in `scenes::all()` must keep their exact golden hashes (pictures and sound). The
  default camera must therefore be an exact identity, with no float rounding in the matrix path
  at Zoom 1 and Turn 0.
- Old scene files load. A missing camera is added on open with the defaults above. A missing
  `screen` uses the kind's default: on for Text, off for everything else. The background is a
  Scene setting, on by default.
- New tests:
  - Follow, smoothing, shake and punch give the same pose when scrubbed as when played straight
    through.
  - World-to-screen and screen-to-world round-trip at various zoom and turn values.
  - Stuck and world Text draw at the right place after the camera moves.
  - Liquid and swarm physics are unchanged when only the camera changes.
  - The score's on-screen count follows the camera.
  - Off-shot fade and pan behave as set, and 0 matches the old mix byte for byte.
  - The world box clamp never shows outside the box.
  - Preview hit testing works under a moved, zoomed and turned camera.
- A demo scene, **Long drop**: a peg course several screens tall, with the camera following the
  lead ball down, a zoom punch on hits, and a world box around the course. It is added to the
  starters, rendered to mp4, and copied to the project files.

## Out of scope

Several cameras and cuts, 3D cameras, parallax layers, camera-driven rules or nodes (for example
"when off-screen, remove"), and camera motion blur.
