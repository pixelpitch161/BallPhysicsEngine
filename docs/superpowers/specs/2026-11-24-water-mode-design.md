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
- Text in a Water scene (Aedan, 2026-11-24: "We can leave text out of water for now").
- Real 3D. The water stays a 2D side view; depth is drawn, not simulated.
- Checking a small spot at full detail without making the final ("spot check"). Later idea.
- Stems, MIDI and bake for a Water scene (the Export window hides them).

## Anti-goals

- A Balls scene must be byte-for-byte what it is today: same physics goldens, same pinned pictures, same sound.
- An export must never differ from the Final that was scrubbed. Export reads the saved final; it never re-runs the
  physics on its own.
- A moving part must never make the liquid coarser, slower to step or less correct. When the two compete, the
  part gives way.
- The editor must not stall: draft stepping, Make final, flow lines and light are all off the frame.
- Speed and size numbers given to Aedan as facts are measured on the RX 9070 XT. An estimate is always called one.

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

- `Scene` gains `sim: SimKind { Balls, Water }` (the kit already has a `SceneKind`, so not that name). The field
  defaults to Balls when a file has none, so every saved scene, saved grade and scene kept inside a video keeps
  loading. `Scene::FORMAT` goes to 13 only so that an older program refuses a Water scene cleanly;
  `OLDEST_FORMAT` stays 12.
- **The app opens on the chooser** from the mockup (Aedan, 2026-11-24: "The app should open with the choice
  between ball or water"): two tiles, Balls and Water, and **Open a scene**. Balls gives today's blank project.
  Water then offers the six looks and **Plain**. **File > New** shows the same chooser.
- The root is the same Scene settings thing with the same `SceneSettings` (Width, Height, fps, Length, tempo),
  because export and Video mode read them from there. Settings that mean nothing without balls are hidden in a
  Water scene; the water's gravity is on the Water card. The Camera's ball-only settings are hidden the same way.
- A Water scene is still a tree of things that own their settings, so the inspector, right-click Copy Value /
  Paste Value / Create Keyframe, undo and save all work unchanged. Each thing kind in the kit says which scene
  kind it belongs to; the Add menu, paste and the Scenes window respect it.
- Things in a Water scene:

| Thing | What it is | Cards |
| --- | --- | --- |
| Scene settings (root) | The world and the picture as a whole | its own settings as today (size, fps, Length), **Water** (detail, gravity, course size), **Light** (ambient, bounce, caustics, shafts, quality), **Back** (what is behind the course), Effects, Grade |
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
- A setting's address stays `{ thing, slot, key }`. `SlotId` is a fixed list with one card per slot, so the Water
  cards need new entries. Reused: `Look` (a Liquid's Look), `Shader` (Effects), the Grade slot. New: `Water`,
  `WaterLight`, `Back` (root); `Form` (a Piece's Shape, a Wall's Line); `Material` (Course, Piece, Wall); `Moves`;
  `Feel`, `Mixing`, `Heat` (Liquid); `Main` (the one card of a Pour, Drain, Force or Lamp).
- Keyframes work on every setting that has them today. Pulses (Follow sound, light, health) and Vary across balls
  are not offered in a Water scene yet.
- Names to avoid clashes: the saved final is a `WaterTake` (Video mode already has a sound "take"), and the
  Liquid's Look card type is `WaterLook`.

### 2. The course

- **One solid field.** Carved rock, pieces and walls are all turned into one field that says, for every point,
  how far the nearest solid edge is and what material it is. The physics and the drawing both read that one
  field, so what blocks the water is exactly what is drawn.
- **Carving.** The Course starts as solid rock. The carve brush digs or fills (size, hardness, round or square
  tip, straight-line and smooth modes). Carving is saved as its brush strokes (`Value::Strokes`), and the mask is
  rebuilt from them and kept only in memory, at 4 px per sample at 1080 wide with the edge stored as a distance so
  it draws smooth at any zoom (**my call**). That keeps the file and each undo step small on a big course.
- **Pieces** come from a palette on the rail, snap to the grid and to each other, and can be scaled, turned and
  flipped. A piece is a path, so its points can be edited like a Shape's. **Walls** are pen lines (straight or
  curved, with a thickness and round or square ends).
- **Order.** Rock first, then each Piece and Wall adds solid or, with **Cuts** on, removes it. So a pipe piece can
  cut a tunnel through rock.
- **Size.** A course is measured in screens. One screen is 1080 x 1920. Up to 8 screens on a side and 16 screens
  in all (**my call**). Only the tiles that hold liquid are stepped and stored, so an empty stretch of a long
  course costs nothing. Tiles are part of the physics from its first part, since the grid and the saved final
  both depend on them; the editor's tools for courses over one screen come in a later part.
- **Camera.** The stage shows the whole course beside the camera's shot, as in the mockups. The Camera's position
  and zoom are keyframed; the **Camera path** tool drops those keyframes by clicking on the course. **Follow**
  can instead keep the shot on the water's front, on a Piece, or on the busiest water, with Lead and Smoothness.

### 3. The physics core

**Replaced on 2026-11-25** by `2026-11-25-all-drops-water-core-design.md`. After the all-drops trial Aedan chose
"All-drops engine. Middle, slider reaches both.", so Water mode's liquid is drops that feel their neighbours
directly, with the grid's pressure solve working only underneath deep water. Below is the history that led there.
"Same card, same result", Detail, the caps, "Same water at every detail", several liquids and moving solids still
hold as written.

Drops carried on a grid stay (it is what gives streams, splashes and colour carried drop by drop). What changes:

- **Its own clock.** The water picks its own step from how fast it is moving. Nothing reads it back for balls,
  nothing else replays it, and it does not have to match bit-for-bit across machines.
- **Same card, same result.** A run must repeat exactly on the same graphics card, driver and program version at
  the same detail, so that a final redone from the middle joins the part before it with no seam. On the RX 9070 XT
  that only holds today because sums are done in whole numbers (`liquid.md`), so the new core keeps whole-number
  or fixed-order sums everywhere, the pressure solve included. A take records the program version, card and
  driver it was made with and counts as out of date when any of them changes.
- **The pressure method: the trial's verdict** (2026-11-25, record in
  `docs/superpowers/plans/water-mode/physics-trial.md`, results page
  https://claude.ai/artifact/6sgerUCgwjRnvsX1ZWRjBR): **adopt the pressure-solving core, with named gaps.** The
  trial core is drops on a staggered grid, the pressure solved each step so the liquid cannot squash (one-sided:
  it pushes but never pulls), solved by multigrid with no substeps, with walls as part-open cell faces. Measured
  on the RX 9070 XT, physics only, at Normal / Fine / Extreme:

  | | Today's core | Trial core |
  | --- | --- | --- |
  | Seconds to work out one second (the course) | 0.29 / 0.51 / 1.40 | 0.08 / 0.09 / 0.15 |
  | Still deep pool after 20 s, movement left | 32 / 63 / 29 px/s | 0 / 1.8 / 0.6 px/s |
  | Pool height kept | 95 to 97 % | 98 to 99.7 % |
  | Share through a 12 px opening in 30 s | 0.7 / 2 / 23 % | 36 / 33 / 33 % |
  | Drops lost through walls | 25 / 0.5 / 0.4 % | 0 / 0.16 / 0 % |

  Both repeat exactly on one machine. The trial core's answers hardly change with Detail, which is what makes a
  draft a fair preview of a final.
- **The gap that decides it: thin streams and films.** The trial core breaks a thin pour into lumps and a rain
  of drops; today's core keeps clean streams (that comes from its wide 3-cell reach and springy pressure, so
  there is no separate hold-together force to copy over). Two quick tries in the trial did not fix it.
- **Aedan's choice** (2026-11-25): the new core, streams fixed first. "The pools do look much much better now.
  The goal should be for the streams to be even better than todays. The liquid should be as good as you can
  possibly make it." So the first physics part (2a below) builds a real hold-together for streams and films on
  the new core, with two marks, judged on full-size frames and side-by-side clips on the RX 9070 XT with the
  table above still holding:
  - **Pass mark** (the least that keeps the new core): Water Weird's pour looks as clean as on today's core.
    **If 2a cannot reach it, Water mode is built on today's core** (moved into `bpe-water`, off the ball
    clock), and the rest of this design is unchanged.
  - **Goal: better than today's.** A pour two cells wide stays one unbroken stream down a whole screen. A sheet
    running off an edge stays a sheet until it meets something. A stream keeps a steady width, without the
    jitter and beading today's shows, and stays clean at draft detail as well as final. A stream breaks into
    drops only when the liquid's Surface pull says it should (low for spray, high for ropes), so breaking up is
    a look Aedan chooses, never an accident of the method. Each of these gets its own test scene and clip.
  - 2a keeps working past the pass mark toward the goal, and says plainly in its report which of the goal's
    lines it reached and which it did not.
- **Smaller gaps to close in 2a or note:** a slosh dies down about a third sooner than it should; 34 of 21,170
  drops crossed walls at Fine; one drop went missing in a still pool. **Not built in the trial:** moving walls
  (part 5), thick liquids, slime and ice (part 4), forces (part 4).
- **Detail.** Cell size at 1080 wide: Draft 12, 8 (default) or 6 px; Final 3, 2 (default) or 1.5 px. About four
  drops a cell. A one-screen course two thirds full is about 86,000 drops in draft and 1.4 million in the default
  final. Caps: 500,000 draft drops, 8 million final drops (**my call**, from memory on a 16 GB card).
- **Same water at every detail** stays a rule: a setting means the same thing in draft and final (today's "the
  same water at every Detail" work carries over), so the draft is a fair preview of everything bigger than a
  couple of draft cells.
- **Speed.** The trial core works out the physics faster than real time at every Detail it was run at. A final
  has about ten times the drops of the trial's Extreme, which nobody has timed, so how long Make final takes is
  still measured in part 8 before it is promised. The firm target stays: the draft plays at full speed with the
  default course. The trial also found that export time is mostly drawing, so the renderer (part 6) is where
  export speed is won or lost, and it is timed there.
- **Several liquids.** Each drop carries its Liquid, its colour and its heat. Feel settings (Thickness, Surface
  pull, Weight, Stickiness) are per Liquid and blend where liquids mix. Weight lets one liquid float on another
  if the chosen core supports it; otherwise it is left for later and said so in the notes.
- **Moving solids.** Each step the solid field carries how fast each solid point is moving, so a moving part
  pushes the water properly. Liquid that cannot be squashed has nowhere to go when it is shut in completely, so:
  a Driven part stops against trapped liquid; a Keyframed part keeps to its keyframes and the trapped drops are
  moved to the nearest open space (with a trouble mark).

### 4. Parts that move

- **Moves** card on a Piece or Wall: **Fixed**; **Keyframed** (position, turn and scale on the timeline, with a
  path preview); **Driven** (the water and gravity move it).
- A Driven part has a **Joint**: **Free** (floats, crates), **Pin** (wheels, buckets, flaps, see-saws) or
  **Slide** (gates, pistons). Settings: Weight, Drag, Spring (pulls back to rest), Stops (turn or travel limits),
  Motor (turns or slides on its own at a speed), Catches liquid (a bucket) and Tips when (how full).
- **How it works.** Driven parts are stepped on the processor by a small rigid-body solver at a fixed 240 steps
  a second, however small the liquid's own steps are. The push of the water on each part is added up on the
  graphics card and read back at that rate without making the card wait. The solver counts the weight of the
  water a part has to shove aside as part of the part (added mass), which is what keeps a light float steady when
  it learns about the water a moment late. Free parts collide with the course and with each other. Up to 64
  driven parts (**my call**).
- **Liquid first, concretely.** The liquid's grid, step and method never change because a part is there. Parts
  are sampled onto the liquid's grid. A part thinner than two cells is thickened for the physics only, and gets a
  trouble mark at draft detail. Any steadying is done on the part's side; the liquid is not touched.

### 5. Draft, Final, flow lines and trouble marks

- **Draft** steps live in the editor on its own thread and device queue. An edit that changes the physics
  restarts it, as today; an edit to a look, lamp, camera, Effects or Grade never does.
- **Make final** runs the physics at final detail in the background and writes a **take** (`WaterTake`).
- **What a take holds.** Sixty frames for every second, whatever the scene's or a video's fps (the water's clock
  does not depend on fps; a frame at another rate is drawn from the nearest stored frame with its drops moved on
  by their speed). Each frame holds the drops themselves, not finished pictures, because Smoothing, Streaks and
  the rest of the Look must still work afterwards. A drop near a surface or where colours meet is about 5 bytes
  (place within its cell, colour, heat, age, which Liquid). A cell deep inside a pool, full and of one colour, is
  about 3 bytes for all its drops. Speed is stored once per cell. Every 2 seconds there is also a full restart
  point (about 48 bytes a drop).
- **Size** (*estimate*, not measured): 4 to 8 GB for a 30 second one-screen final at 2 px cells, restart points
  included. The Make final part measures it on real courses and reports it to Aedan. Levers if it is too much:
  store every other frame, a coarser default final, or fewer restart points. Before making, the window says how
  much room is needed and refuses if the disk cannot hold it.
- **Where takes live.** One finals folder (in app data by default, movable in Settings; **my call**), with each
  take found by a fingerprint of the scene's physics. A scene file, a recovered copy, a Save As copy and the same
  scene kept inside a video therefore all find the same take, and nothing is made twice. It is a cache: deleting
  it loses nothing but time. **Clear finals** empties it, and takes unused for 30 days are removed. A take is
  written in chunks, so a crash keeps everything up to the last whole chunk.
- **Draft | Final switch** in the toolbar. In Final view the timeline's **Final** lane shows what is made, what
  is being made, and what is out of date (hatched). Frames can be scrubbed as soon as they are made, and Make
  final can be stopped at any time.
- **What an edit costs.** Look, lamp, camera, Effects, Grade: nothing; the final redraws at once. A physics
  change is out of date from the restart point before the **earliest time any value the physics reads becomes
  different**: for a keyframe that is added, moved or removed, the time of the keyframe before it on that setting
  (the curve between them changes too); for a Pour's start, the earlier of the old and new start; for a setting
  with no keyframes, the course, a Liquid's Feel or gravity, the beginning. Only that tail is remade. The final is
  remade only when Aedan asks or on export (**my call**).
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
   today's noise, and visible shafts if Shafts is on. Rays go out in a fixed fan, never at random, so a frame is
   the same every time it is drawn and nothing flickers or needs smoothing over time. Solids block light along
   the whole way from the lamp, even off screen; liquid is only counted inside the shot plus a margin. The bent
   light code in `bpe-gfx/src/lighting` (glass and light) is reused where it fits.
4. **Liquid as a light.** A liquid that gives off light (Glow, Heat) spreads it to the solids and the back,
   blocked by solids, so neon and lava light the rock around them.
5. **The liquid itself**, from its Look card: body colour by depth, what is behind it bent through it, edge
   sheen, shine from the lamps, a solid glossy body for paint, a crust, flat bands and an outline for cartoon.
6. **Extras.** Foam, bubbles, mist and sparks are small sprites born from what the physics reports (hard hits,
   trapped air, fast surfaces), and wet marks stay on solids the liquid has touched.
7. **Shared finish.** Effects, bloom and Grade, the same code as a Balls scene. Today `Renderer::finish` reads
   more than the finished picture: Effects read the things' object and light pictures, and the grade's Things
   masks read silhouettes. So the water renderer hands the finish the same kinds of input: the colour picture, a
   light picture (lamp and glow light), and silhouettes for each Liquid, the solids, the back and the extras. The
   grade's Things masks then pick those. The light and looks part goes through every Effect and Grade mask and
   marks it offered, offered from the water's inputs, or hidden in a Water scene; changing `finish` to take these
   inputs must leave every Balls picture byte-identical.

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
- Build | Grade | Video modes stay. Grade mode works on a Water scene with the masks listed in section 6.
- **A Water scene has its own stage in the editor.** Today the preview steps the ball `Playback` that the whole
  editor holds and composes the picture with handles; that path is left alone. In a Water scene the water stage
  runs instead, the ball playback stays empty and unstepped, and the jobs built from every scene today (sound,
  aim, particles, the Follow sound sampler, the light probe) are not started. Recovery and autosave, Save As, the
  Scenes window and Preview final frame each get a Water path in the part that first needs it.
- **Export** uses the final. If any of the range is missing or out of date, the Export window offers **Make final
  and export** as one button with the time it expects, and **Export the draft** for a quick look. Export, Preview
  final frame, the Scenes window's clips and Video mode all get their frames through `bpe-cli`'s `FrameMaker` and
  `LiveClip`; those gain a clip source with a ball kind and a water kind.
- **Video mode.** A Water scene can be a clip like any other. It plays from its take when there is one and from
  the draft otherwise (the clip shows a small Draft tag). Exporting a video makes any missing finals first. The
  take is the same one the scene itself uses, found by fingerprint in the finals folder. A water clip has no
  scene sound.

### 8. Where the code goes

- `bpe-core`: `SimKind`, the new `SlotId`s, the Water things' data (`water/`: carve strokes, piece and wall
  shapes, joints). No physics.
- **New crate `bpe-water`** (after `bpe-gfx`, before `bpe-kit`): the physics (`sim/`, WGSL compute), driven parts
  (`parts/`), takes (`take/`), flow lines and trouble marks (`preview/`), the renderer (`draw/`, `shaders/`). It
  uses `bpe-gfx` for the device helpers and for the shared finish (section 6, step 7).
- `bpe-kit`: the Water thing kinds, cards and settings (`things/water_*`, `modules/water_*`) and the looks table.
  Each thing kind says which `SimKind` it belongs to, and the tests that walk every thing kind and build a ball
  `World` (`restarts.rs` and the "every kind" tests) skip the Water kinds.
- `bpe-studio`: `water_mode/` (the water stage, tool rail, stage tools, course view, Final lane, the make-final
  job).
- `bpe-cli`: the clip source in `FrameMaker` and `LiveClip`, so export of a Water scene, and of a video holding
  one, makes the final and then draws from it.
- Today's Liquid thing and `bpe-gfx/src/liquid_sim/` are not touched until the last part.

### 9. Checks

- **Physics tests** check behaviour, not exact numbers: volume kept over 20 seconds, a pool comes to rest, water
  passes an opening of 2, 4 and 8 cells at the expected rate, the same result twice on the same device, a final
  redone from a restart point equals a final made in one go.
- **Pinned pictures are drawn from a small stored take**, not from live physics, so a driver update that moves the
  physics' last bits does not move them.
- **Edits**: a test changes every setting of every Water thing kind and checks it restarts the draft and marks the
  final out of date exactly when it should (the Water twin of `restarts.rs`).
- **Pictures**: picture tests at 270 x 480 through `test_device()`; one full-size look scene per starting look in
  `full_size_look.rs`; pinned pictures on Aedan's card for each look.
- **Speed and size** are timed on the RX 9070 XT by the Remote Control recipe and written into the notes.
- Balls goldens and pinned pictures must not change in any part.

## Parts

Each part is its own thread and pull request, ends with something Aedan can open, and gets one Opus review. Each
part adds its own tools to the rail.

1. **Water scenes open.** `SimKind`, the new slots, save and load, the chooser at start and on File > New (and
   the one line in `CLAUDE.md` that says the editor always opens to a blank project), the Water things as data, the
   water stage as an empty shell with the tool rail, ball-only jobs and tests gated by kind. Balls goldens and
   pictures shown unchanged.
2. In two halves, because the stream question has to be settled before anything is built on the core:
   - **2a. The water core, with streams better than today's.** `bpe-water` with the pressure-solving core from
     the trial (tiles from the start, whole-number or fixed-order sums) and a hold-together for thin streams
     and films. Pass mark and goal as in section 3, with the trial's table still holding, on the RX 9070 XT.
     Ends with side-by-side clips for Aedan. If the pass mark cannot be reached, 2a instead moves today's core
     into `bpe-water`.
   - **2a and 2a-2 were built** (PRs #225, #227) and are superseded by the all-drops core: **C1** the engine
     proper, **C2** deep water and landings, **C3** the plain drawing
     (`2026-11-25-all-drops-water-core-design.md`, "Parts"). C2 and C3 run side by side after C1.
   - **2b. Water runs in a Water scene** (after C2 and C3, on the all-drops engine and the plain drawing). The
     Course with carving, Liquid (Feel), Pour (tap and fill at the start), a plain look, in place of part 1's
     empty stand-in. 2b also times a full screen of draft water (about 86,000 drops at 8 px) on the RX 9070 XT;
     the physics is made faster only if that draft cannot play live.
   - **2c. Air and wetting** (added at the checkpoint, below; the tiles that used to be called 2c are now in part
     9). Air trapped under a plunging pour rises and breaks up instead of lingering as big pockets, and each
     material gets a **Wetting** setting: on a waxy surface water beads up and rolls, on a wet one it spreads
     into a film. Wetting, not a weaker skin bend, is what settles C2's tall shelf puddles. **Built** (PR #240):
     air bursts once it comes up into the band under the water's top, and Wetting 1 is the default (the least that
     settles the shelves on the RX 9070 XT); Wetting reaching less at Extreme than at Fine is handed to part 3
     (`plans/water-mode/part2c-air-and-wetting.md`, "As built and measured").
3. **Pieces and the pen.** Piece and Wall, the palette, snapping, Cuts, Materials as data.
4. **More liquids.** Several liquids with mixing and Un-mix; **Weight** (one liquid floats on another, layers
   settle by weight); thick liquids (honey, slime); rain, drips and bursts; drains and leaks; and **drains that feed
   pours**, so water that leaves comes back and a course runs as long as the video. Heat (freezing, melting),
   drying and stains, and reactions and fizz are **Later** (checkpoint). Forces moved to part 4b.
   - **4b. Forces and ferrofluid.** Wind, whirlpool and magnet (heater and cooler wait for heat). **Ferrofluid**:
     a black glossy liquid that a magnet pulls into rows of spikes, the magnet's strength on keyframes (water has
     no sound). If strong forces need more than 32 steps a frame, the skin that stays calm at any step count
     (C2's `most_steps` hand-on) is built here.
5. **Moving parts.** Keyframed and Driven, joints, floats, wheels, buckets, gates. Part 8 comes first, so part 5
   adds the driven parts' state to the take's restart points.
6. **Light and the six looks.** The whole renderer of section 6, materials drawn, Back, Lamps, the looks table,
   the shared finish and its masks, foam, bubbles, mist, sparks and wet marks. With heat Later, the Molten look
   glows by its colour and has no crust from heat yet.
7. **Flow lines and trouble marks**, from the draft: **Later** (checkpoint). The all-drops draft plays live and
   lands within a few points of the final, so they are needed much less.
8. **Make final and export**, **moved up to follow part 3** (checkpoint). Final detail, takes, the Final lane,
   partial remakes, the clip source in `bpe-cli`, export of a Water scene from the editor and the command line,
   Preview final frame, the time and disk room of a one-screen final at 2 px measured and reported (about 10 s a
   scene second is the C2 estimate; it is made faster only if it is well over that). **Slow motion**: a stretch
   of the final kept at 4 or 8 times the frames, so a splash plays slowly and smoothly, with speed ramps on the
   timeline. Until part 6 lands, Make final and export draw with the plain look. The take's format leaves room for
   what parts 4, 4b and 5 add (more liquids, heat later, driven parts).
9. **Big courses.** The editor's tools for courses over one screen: whole-course view, Camera path, Follow, and
   tiles for the deep-water grid and the solid field (drops need none).
10. **Video mode and clean-up.** Water clips in a video and the Scenes window. Removing the old Liquid thing from
    Balls scenes is **Later**: Aedan decides once Water mode is finished.

Order (after the checkpoint): 1, 2a, the all-drops core (C1, then C2 and C3 side by side), all built. Then 2b,
2c, 3 and 8 in a row, so a finished water video can be exported in the plain look after part 8. Then 6, 4, 5 and
4b side by side. Then 9 and 10.

## The water checkpoint (2026-11-26)

Aedan asked for a progress check of all remaining water work before part 2b ("Just for the water") and marked every
item Keep, Later or Cut on the page https://claude.ai/artifact/BJkTfdZBfoF1xtW3mg834q (his picks are in its
`picks` store). The parts above already follow it. Besides those:

- **Kept, from C2's hand-on:** air pockets (2c), the draft's speed (timed in 2b), the final's speed (measured in 8).
- **Later:** C2's shelf puddles by the skin's bend (Wetting in 2c is the fix), ropes at the top of the Surface pull
  slider at 2 px (`most_steps` above 32, unless 4b needs it), rain on a window, siphons and the greedy cup (the
  pressure only pushes; a siphon needs a pull in a full pipe, untried), liquids whose thickness changes as they
  move (oobleck, ketchup, stretchy slime), words and logos carved as courses, a spot check at full detail.
- **Cut:** a still pool's last px/s of drift (it cannot be seen), the slosh's last few percent at Extreme, the
  draft's extra spray at Surface pull 0.2, splitting drops where water is thin, and sounds in Water scenes (sound
  stays a non-goal).
- No longer needed (all settled by the all-drops engine): the grid core, grid tiles for speed, knots at Extreme,
  matching the old liquid, the bridge to the old drawing, random births, splitting drops to fix landings.

## Risks

- **Thin streams on the new core** were the main risk of the whole build. The all-drops trial settled the method
  (2026-11-25); the core's own risks are in `2026-11-25-all-drops-water-core-design.md`.
- **Make final may be slow.** Nobody has timed 1.4 million drops yet. If it is too slow to be pleasant, the
  default final becomes coarser and the finest setting stays for the last pass before a post.
- **Take size.** If packing cannot get the size down, finals store every other frame.
- **Driven parts learn about the water a moment late.** Added mass on the part's side is the cure; if a very
  light part still jitters it is made heavier with a note, never by touching the liquid.
- **2D light at full size** may be too slow at Best on a long shot. Quality steps and the shot-only margin are
  the levers, and the editor already drops to Draft while dragging.
- **The editor assumes a ball playback everywhere** (about 90 uses). Part 1 leaves that path alone and gates the
  ball-only jobs by kind; it must show Balls goldens and pictures unchanged before anything else lands.

## Answered by Aedan (2026-11-24)

- The app opens on the Balls or Water chooser.
- Text is left out of Water scenes for now.

- 2026-11-25, after the physics trial: the new pressure-solving core, with streams fixed first and the goal
  "even better than todays"; "the liquid should be as good as you can possibly make it".

- 2026-11-26, at the water checkpoint: the picks and order in "The water checkpoint" above.

Still to be measured and reported before anything is built on them: how much disk room a final uses (several GB
is the estimate) and how long Make final takes.
