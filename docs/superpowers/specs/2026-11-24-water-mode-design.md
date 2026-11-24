# Water mode: a scene type built only for liquid (design)

Owner: Aedan. Brainstorm thread 2026-11-24. Mockups: https://claude.ai/artifact/5Y9ow3CYMFKGiNhmz1uTtA
(scene chooser; course builder layout **D, all three on one tool rail**, picked by Aedan; six looks, all wanted).
Physics trial brief: `ballphysics/water-mode/physics-trial-brief.md` in the project files (its verdict fills in
"The physics core" below).

Every number marked *target* or *estimate* is unmeasured. Lines marked **(My call)** are choices Aedan was not
asked about; they are cheap to change before the part that builds them.

## Intent

Aedan, 2026-11-24: "When creating a new scene, a choice of simulation type is given. Each simulation type's
editor, features, and systems are tuned to be the best for that kind of simulation. This means the physics,
lighting, and the way you edit the simulation does not have to be 100% consistent between each simulation type.
Let's start with the new liquid mode."

The videos Aedan has in mind: "liquids flowing through corridors, slopes, obstacles, etc, ranging from small
streams to big pools getting filtered through various openings", built with "as much rich detail as possible".

Today's Liquid thing lives inside the ball engine and pays for it: it steps on the balls' clock and must be
bit-exact for them, it is stepped again by every playback that needs the balls (editor, sound, aim, particle job,
export), it is lit through the balls' lighting passes (the surface is redrawn for every pass; depth and caustics
are faked), and its settings are squeezed into ball cards. A Water scene has no balls, so none of that applies.

Success: Aedan picks **Water** for a new scene, picks a look, carves a course and drops in pieces, sets a pour, and
watches the water run through it live at draft detail with lines showing where it will go. Aedan presses **Make
final**, scrubs the full-detail result, changes the look and the camera without waiting again, and exports an mp4
that is exactly what was scrubbed.

## Goals

- **Scene type.** A new scene is Balls or Water. A Water scene has its own things, its own physics, its own
  lighting and its own editor screen. Balls scenes do not change at all.
- **Course builder** with one tool rail: carve rock with a brush, snap in pieces, draw walls with a pen.
- **Courses bigger than the screen** with a travelling camera, and one-screen courses too.
- **Liquid first.** Parts can be fixed, keyframed or driven by the water (wheels, buckets, floats, gates), and
  none of them may limit what the liquid itself can do.
- **Draft then Final.** Draft detail plays live while editing. Make final works the physics out once at full
  detail and saves it; scrubbing and export read the saved result.
- **Flow lines and trouble marks** from the draft, so the path of the water is visible before any final is made.
- **Six starting looks** on one water renderer, every setting still Aedan's to change: real water in lamp-lit
  rock, clean glass lab, neon in the dark, thick paint on paper, molten and glowing, bold cartoon. More later.
- **Everything today's liquid can do without balls** carries over: colour mixing and Un-mix, pours, rain, drips,
  drains and leaks, whirlpools, magnets (spikes), heat, freezing and melting, drying, stains, fizz, foam, bubbles,
  mist, wet walls.
- Shared with Balls scenes: the Camera thing, keyframes and the timeline, undo, copy and paste of values, the
  Effects stack, bloom, the Grade card and Grade mode, export, Video mode.

## Non-goals (for now)

- **Sound.** Aedan, 2026-11-24: "let's not worry about the sound feature for the liquid mode". A Water scene has
  no sound cards, no Mixer and no liquid sounds, and exports an mp4 with no sound of its own. In a video the
  video's song plays over it. Follow sound is therefore not offered in a Water scene yet.
- Balls, Emitters, Shapes, Particles, Rigs, Rules and Health in a Water scene. Floats and other driven parts take
  the place of "things in the water".
- Text in a Water scene (**My call**; asked below).
- Real 3D. The water stays a 2D side view; depth is drawn, not simulated.
- Checking a small spot at full detail without making the final ("spot check"). Later idea.
- Old scene files keep no promises (project rule). The scene format goes from 12 to 13.

## Anti-goals

- A Balls scene must be byte-for-byte what it is today: same physics goldens, same pinned pictures, same sound.
- An export must never differ from the Final that was scrubbed. Export reads the saved final; it never re-runs the
  physics on its own.
- A moving part must never make the liquid coarser, slower to step or less correct. When the two compete, the
  part gives way.
- The editor must not stall: draft stepping, Make final, flow lines and light are all off the frame.
- Speed and size numbers in messages to Aedan are measured on the RX 9070 XT, never guessed.

## Constraints from the project

- Editor colours come only from `style/` tokens. Controls are named for kittest. No `ComboBox`.
- Windows-friendly code, cross-built with MinGW. Aedan never runs commands.
- wgpu without a window goes through `bpe_gfx::headless_device`; picture tests use `bpe_gfx::test_device()`.
- Tests that draw run on Aedan's card in CI; pinned pictures refresh only through "Refresh GPU pictures".
- `bpe-core`, `bpe-runtime` and `bpe-kit` keep refusing platform maths. The water engine is a new crate with no
  such rule: it never feeds the balls' physics.

## Approaches considered

1. **One app, a new water engine inside** (**picked by Aedan**, 2026-11-24). Ball Studio stays one program. A
   Water scene runs on its own physics, lighting and editor screen and shares the timeline, camera, grading,
   export and Video mode.
2. **Grow today's Liquid thing.** Quicker to a first result, but keeps the ball clock, the ball lighting and the
   ball card layout, which are the limits being lifted.
3. **A separate Water program.** Most freedom, but the timeline, grading and export would be built twice, and a
   water scene could not share a video with a balls scene.

## The design

### 1. Scene type and the Water scene's things

- `Scene` gains `kind: SceneKind { Balls, Water }` (format 13). **File > New** opens the chooser from the mockup:
  two tiles, Balls and Water; Water then offers the six looks and **Plain**. The app still opens straight to a
  blank Balls scene (**My call**; asked below).
- A Water scene is still a tree of things that own their settings, so the inspector, right-click Copy Value /
  Paste Value / Create Keyframe, undo and save all work unchanged. Each thing kind in the kit says which scene
  kind it belongs to; the Add menu, paste and the Scenes window respect it.
- Things in a Water scene:

| Thing | What it is | Cards |
| --- | --- | --- |
| Scene settings (root) | The world and the picture as a whole | **Water** (detail, gravity, course size), **Light** (ambient, bounce, caustics, shafts, quality), **Back** (what is behind the course), Effects, Grade |
| Camera | Shared with Balls scenes | Camera, plus **Follow** (below) |
| Course | The rock: one per scene | **Rock** (material and its settings), holds the carved mask |
| Piece | A snapped-in part: ramp, funnel, pipe, sieve, basin, step, pillar, wheel, bucket, gate, float, or a custom drawn shape | **Shape**, **Material**, **Moves** (Fixed, Keyframed, Driven) |
| Wall | A pen-drawn line with a thickness | **Line**, **Material**, **Moves** |
| Liquid | One kind of liquid; a scene can hold several that mix | **Feel**, **Look**, **Mixing**, **Heat** |
| Pour | Where a liquid comes in: tap, rain, drip, burst, fill at the start | **Pour** (which Liquid, rate, width, speed, aim, when) |
| Drain | Where it leaves: drain, leak, edge of the course | **Drain** |
| Force | Magnet, whirlpool, wind, heater, cooler | **Force** |
| Lamp | A light: Spot, Strip, Sun | **Lamp** |
| Folder | Grouping | none |

- Carried-over settings keep today's names where the meaning is the same (Thickness, Surface pull, Clarity, Deep
  colour, Bend, Shine, Streaks, Mix speed, Remember for Un-mix and so on), so nothing has to be relearned.
- A setting's address stays `{ thing, slot, key }`. Keyframes work on every setting that has them today. Pulses
  (Follow sound, light, health) and Vary across balls are not offered in a Water scene yet.

### 2. The course

- **One solid field.** Carved rock, pieces and walls are all turned into one field that says, for every point,
  how far the nearest solid edge is and what material it is. The physics and the drawing both read that one
  field, so what blocks the water is exactly what is drawn.
- **Carving.** The Course starts as solid rock. The carve brush digs or fills (size, hardness, round or square
  tip, straight-line and smooth modes). The carved mask is kept inside the scene file, compressed, at 4 px per
  sample at 1080 wide (**my call**; the edge is stored as a distance, so it draws smooth at any zoom).
- **Pieces** come from a palette on the rail, snap to the grid and to each other, and can be scaled, turned and
  flipped. A piece is a path, so its points can be edited like a Shape's. **Walls** are pen lines (straight or
  curved, with a thickness and round or square ends).
- **Order.** Rock first, then each Piece and Wall adds solid or, with **Cuts** on, removes it. So a pipe piece can
  cut a tunnel through rock.
- **Size.** A course is measured in screens. One screen is 1080 x 1920. Up to 8 screens on a side and 16 screens
  in all (**my call**). Only the tiles that hold liquid are stepped and stored, so an empty stretch of a long
  course costs nothing.
- **Camera.** The stage shows the whole course beside the camera's shot, as in the mockups. The Camera's position
  and zoom are keyframed; the **Camera path** tool drops those keyframes by clicking on the course. **Follow**
  can instead keep the shot on the water's front, on a Piece, or on the busiest water, with Lead and Smoothness.

### 3. The physics core

Drops carried on a grid stay (it is what gives streams, splashes and colour carried drop by drop). What changes:

- **Its own clock.** The water picks its own step from how fast it is moving. Nothing reads it back for balls,
  nothing else replays it, and it does not have to match bit-for-bit across machines.
- **Same card, same result.** A run must repeat exactly on the same graphics card at the same detail, so that a
  final redone from the middle joins the part before it with no seam, and so tests can pin it.
- **Which pressure method** is the trial's question. Candidate A solves the pressure on the grid each step so the
  liquid cannot squash (no springy pools, no patches for packing at walls). Candidate B is today's method (the
  liquid is slightly squashy and pushes back). The trial thread compares them side by side on Aedan's card for
  speed, thin streams and openings 2, 4 and 8 cells wide, pools at rest, volume kept over 20 seconds, and feel.
  **Its verdict is copied here before part 1 is planned.** Everything else in this design holds for either.
- **Detail.** Cell size at 1080 wide: Draft 12, 8 (default) or 6 px; Final 3, 2 (default) or 1.5 px. About four
  drops a cell. A one-screen course two thirds full is about 86,000 drops in draft and 1.4 million in the default
  final. Caps: 500,000 draft drops, 8 million final drops (**my call**, from memory on a 16 GB card).
- **Same water at every detail** stays a rule: a setting means the same thing in draft and final (today's "the
  same water at every Detail" work carries over), so the draft is a fair preview of everything bigger than a
  couple of draft cells.
- **Speed targets**, to be measured in part 1: draft plays at full speed on the RX 9070 XT with the default
  course; a 30 second one-screen final at 2 px is made in under 5 minutes.
- **Several liquids.** Each drop carries its Liquid, its colour and its heat. Feel settings (Thickness, Surface
  pull, Weight, Stickiness) are per Liquid and blend where liquids mix. Weight lets one liquid float on another
  if the chosen core supports it; otherwise it is left for later and said so in the notes.
- **Moving solids.** Each step the solid field carries how fast each solid point is moving, so a moving part
  pushes the water properly and a closing gate squeezes it out.

### 4. Parts that move

- **Moves** card on a Piece or Wall: **Fixed**; **Keyframed** (position, turn and scale on the timeline, with a
  path preview); **Driven** (the water and gravity move it).
- A Driven part has a **Joint**: **Free** (floats, crates), **Pin** (wheels, buckets, flaps, see-saws) or
  **Slide** (gates, pistons). Settings: Weight, Drag, Spring (pulls back to rest), Stops (turn or travel limits),
  Motor (turns or slides on its own at a speed), Catches liquid (a bucket) and Tips when (how full).
- **How it works.** Driven parts are stepped on the processor by a small rigid-body solver. The push of the water
  on each part is added up on the graphics card and read back once a step (one step late, as today's ball "feel"
  is). Free parts collide with the course and with each other. Up to 64 driven parts (**my call**).
- **Liquid first, concretely.** The liquid's grid, step and method never change because a part is there. Parts
  are sampled onto the liquid's grid. A part thinner than two cells is thickened for the physics only, and gets a
  trouble mark at draft detail. If light parts jitter, the part is damped; the liquid is not touched.

### 5. Draft, Final, flow lines and trouble marks

- **Draft** steps live in the editor on its own thread and device queue. An edit that changes the physics
  restarts it, as today; an edit to a look, lamp, camera, Effects or Grade never does.
- **Make final** runs the physics at final detail in the background and writes a **take**: every frame's drops,
  packed small, plus a full restart point every 2 seconds. The take is a folder beside the scene file (app data
  for an unsaved scene). It is a cache: deleting it loses nothing but time. The status bar shows its size, and
  **Clear finals** removes it.
- **Size** (*estimate*): about 3 bytes a drop near surfaces, far less deep inside pools (stored as filled cells
  with a colour). Target at most 4 GB for a 30 second one-screen final at default detail. If part 6 measures more,
  the fallback is to store every other frame and place the in-between drops from their speed. Before making, the
  window says how much room it needs and refuses if the disk cannot hold it.
- **Draft | Final switch** in the toolbar. In Final view the timeline's **Final** lane shows what is made, what
  is being made, and what is out of date (hatched). Frames can be scrubbed as soon as they are made, and Make
  final can be stopped at any time.
- **What an edit costs.** Look, lamp, camera, Effects, Grade: nothing; the final redraws at once. A change that
  starts at a time (a keyframe, a pour's start, a gate opening): the final is out of date from the restart point
  before that time, and only that tail is remade. A change to the course, a Liquid's Feel or gravity: all of it.
  The final is remade only when Aedan asks or on export (**my call**).
- **Flow lines.** A second draft run goes ahead of the playhead in the background and traces where the water
  travels over the whole clip. The lines are drawn on the course: thicker where more water goes, coloured by
  time. Options: whole clip, or the next few seconds from the playhead. After a final is made its own lines can be
  shown beside the draft's in a second colour, which shows how far the draft can be trusted.
- **Trouble marks** sit where the draft cannot be trusted: an opening narrower than three draft cells that water
  reaches, water thinner than two draft cells, a part thickened for the physics, and water faster than the draft
  can step cleanly. Each mark says in a sentence what may differ in the final.
- **Honest limit**, already told to Aedan: what full detail does can only be known by running it. The lines and
  marks narrow the surprise; they do not remove it.

### 6. Light and look

Its own renderer, drawn once per frame from the drops (draft: live drops; final: the take). Nothing here needs
Make final again.

1. **Solids and back.** The Back, then the solid field with each solid's material, lit by the lamps.
2. **Liquid fields.** Drops are gathered into pictures of amount, colour, speed, heat, foam and age. A smoothed
   surface with a slope comes from the amount (Smoothing and Streaks as today).
3. **Lamp light.** For each Lamp, light is followed through the scene in 2D: stopped by solids, bent where it
   enters the liquid (Bend), tinted and dimmed by how far it has travelled through liquid (Clarity, Deep colour).
   Where bent light bunches up it is brighter, which gives real light patterns under the surface instead of
   today's noise, and visible shafts if Shafts is on. Worked out only for the camera's shot plus a margin.
4. **Liquid as a light.** A liquid that gives off light (Glow, Heat) spreads it to the solids and the back,
   blocked by solids, so neon and lava light the rock around them.
5. **The liquid itself**, from its Look card: body colour by depth, what is behind it bent through it, edge
   sheen, shine from the lamps, a solid glossy body for paint, a crust, flat bands and an outline for cartoon.
6. **Extras.** Foam, bubbles, mist and sparks are small sprites born from what the physics reports (hard hits,
   trapped air, fast surfaces), and wet marks stay on solids the liquid has touched.
7. **Shared finish.** Effects, bloom and Grade, the same code as a Balls scene.

**Liquid > Look card** (groups): *Colour* (Colour, Colour from: its own / speed / heat / age / depth, as a
gradient), *Body* (Clarity, Deep colour, Solid: 0 clear to 1 paint, Bend, Rainbow split), *Surface* (Smoothing,
Streaks, Shine, Shine size, Edge sheen, Ripples, Thin film), *Glow* (Gives off light, Glow colour, Glow from heat
or speed), *Crust* (Amount, Colour, Forms in, Cracks), *Drawn* (Outline, Outline colour, Bands, Band softness),
*Extras* (Foam, Bubbles, Mist, Sparks, each with amount, size, colour, life), *Mixing shows* (Swirl detail, Shows
as).

**Materials** (Course > Rock, Piece and Wall > Material): **Rock**, **Glass** (frosted, lets tinted light
through), **Paper** (flat with grain and a cut-out shadow), **Basalt** (dark, glossy cracks), **Flat** (one
colour). Each has Colour, Second colour, Texture size, Depth (how rounded the edge looks), Roughness, Outline, Wet
marks. **Back** card: Same as rock but darker, Colour, Gradient or Picture.

**Lamps**: Spot (position, aim, spread), Strip (a line of light), Sun (a direction over the whole course), each
with Colour, Brightness, Softness and Shadows. Scene **Light** card: Ambient, Bounce, Caustics, Shafts, Quality
(Draft, Good, Best; the editor uses Draft while a slider is dragged).

**The six starting looks** are one table in the kit (`looks.rs`): each row is plain setting values for the
Liquid Look cards, the materials, the Back, a set of Lamps, Effects and Grade. Picking one writes those values
into the scene's own settings (one undo step, with **Keep my colours** and **Keep my lamps** switches). Nothing
stays linked to the look afterwards. Adding a seventh look later is adding a row.

| Look | Liquid | Solids and back | Light |
| --- | --- | --- | --- |
| Real water in lamp-lit rock | Clear, deep colour, bend, caustics, light foam | Warm rock, darker rock behind | Two warm spots |
| Clean glass lab | Bright blue, crisp shine, bubbles | Frosted glass on white | Soft sun, high ambient |
| Neon in the dark | Gives off light, colour by speed, white core | Near-black flat | No lamps; the liquid lights the scene |
| Thick paint on paper | Solid, glossy, sharp swirls where colours meet | Paper cut-out with shadow | Flat soft light |
| Molten and glowing | Glow from heat, crust, sparks | Basalt | No lamps; glow only |
| Bold cartoon | Flat bands, thick outline, foam blobs | Flat colour with outline | None (drawn flat) |

### 7. The editor

- **Layout D** from the mockups. Tool rail down the stage's left edge: Select, Carve, Piece, Pen, Pour, Drain,
  Lamp, Force, Camera path. The active tool's options sit in a strip above the stage (brush size, piece palette,
  line thickness). Left panel: the things tree. Right: the inspector. Bottom: the timeline with the Final lane.
- Toolbar: a **Water scene** chip, **Draft | Final**, **Make final**, **Export**. Status bar: draft drops, and the
  final's drops, time and disk room as measured on the last make (estimates before the first).
- **View switches** on the stage: Flow lines, Trouble marks, Whole course / Shot only, Final lines.
- Build | Grade | Video modes stay. Grade mode works on a Water scene unchanged, since it grades the finished
  picture.
- **Export** uses the final. If any of the range is missing or out of date, the Export window offers **Make final
  and export** as one button with the time it expects, and **Export the draft** for a quick look.
- **Video mode.** A Water scene can be a clip like any other. It plays from its take when there is one and from
  the draft otherwise (the clip shows a small Draft tag). Exporting a video makes any missing finals first. The
  take is found by a fingerprint of the kept scene's physics and lives in a folder beside the video file. A water
  clip has no scene sound.

### 8. Where the code goes

- `bpe-core`: `SceneKind`, the Water things' data (`water/`: course mask, piece and wall shapes, joints), format
  13. No physics.
- **New crate `bpe-water`** (after `bpe-gfx`, before `bpe-kit`): the physics (`sim/`, WGSL compute), driven parts
  (`parts/`), takes (`take/`), flow lines and trouble marks (`preview/`), the renderer (`draw/`, `shaders/`). It
  uses `bpe-gfx` for the device helpers and for the shared finish (Effects, bloom, Grade), which `bpe-gfx` exposes
  as one call on a finished picture.
- `bpe-kit`: the Water thing kinds, cards and settings (`things/water_*`, `modules/water_*`), the looks table, and
  one **player** that both scene kinds sit behind ("the frame at time t") so the editor preview, export and
  `bpe-kit::video` do not care which kind a scene is.
- `bpe-studio`: `water_mode/` (tool rail, stage tools, course view, Final lane, the make-final job).
- `bpe-cli`: export of a Water scene and of a video holding one (makes the final, then draws from it).
- Today's Liquid thing and `bpe-gfx/src/liquid_sim/` are not touched until the last part.

### 9. Checks

- **Physics tests** check behaviour, not exact numbers: volume kept over 20 seconds, a pool comes to rest, water
  passes an opening of 2, 4 and 8 cells at the expected rate, the same result twice on the same device, a final
  redone from a restart point equals a final made in one go.
- **Edits**: a test changes every setting of every Water thing kind and checks it restarts the draft and marks the
  final out of date exactly when it should (the Water twin of `restarts.rs`).
- **Pictures**: picture tests at 270 x 480 through `test_device()`; one full-size look scene per starting look in
  `full_size_look.rs`; pinned pictures on Aedan's card for each look.
- **Speed and size** are timed on the RX 9070 XT by the Remote Control recipe and written into the notes.
- Balls goldens and pinned pictures must not change in any part.

## Parts

Each part is its own thread and pull request, ends with something Aedan can open, and gets one Opus review.

1. **Water scenes exist.** Scene kind and chooser, `bpe-water` with the chosen physics core at draft detail,
   Course with carving, Liquid (Feel), Pour (tap and fill at the start), a plain look, the shared player, save.
2. **Light and the six looks.** The whole renderer of section 6, materials, Back, Lamps, the looks table.
3. **Pieces and the pen.** Piece and Wall, the palette, snapping, Cuts, the complete tool rail.
4. **Everything today's liquid can do.** Mixing and Un-mix, rain, drips, bursts, drains, leaks, Forces, heat,
   freezing, drying, stains, fizz, wet marks.
5. **Moving parts.** Keyframed and Driven, joints, floats, wheels, buckets, gates.
6. **Make final.** Final detail, takes, the Final lane, partial remakes, flow lines, trouble marks.
7. **Big courses.** Courses over one screen, tiles, the camera path tool and Follow.
8. **Export, Video mode, clean-up.** Export from takes, water clips in a video, command-line export, and, once
   Aedan agrees Water mode is at least as good, removing the old Liquid thing from Balls scenes.

Parts 2 to 5 do not depend on each other and can run side by side once part 1 is merged.

## Risks

- **The pressure method's speed** at 1.4 million drops is unknown until the trial reports. If Candidate A loses,
  part 1 builds on Candidate B and the design is otherwise unchanged.
- **Take size.** If packing cannot reach the target, finals store every other frame.
- **Driven parts read the water one step late**, which can make light floats jitter. The part is damped, never
  the liquid.
- **2D light at full size** may be too slow at Best on a long shot. Quality steps and the shot-only margin are
  the levers, and the editor already drops to Draft while dragging.
- **Two scene kinds behind one player** touches the editor preview, export and Video mode in part 1. That part
  must show Balls goldens and pictures unchanged before anything else lands.

## Questions for Aedan

1. Finals use real disk room while a scene is being worked on, around 4 GB for a 30 second video (estimate), in a
   folder that is safe to delete. Is that all right?
2. The app still opens to a blank Balls scene, and File > New shows the Balls or Water chooser. Or should the
   chooser be the first thing shown?
3. Text is left out of Water scenes for now. Is that all right, or is text over the water wanted from the start?
