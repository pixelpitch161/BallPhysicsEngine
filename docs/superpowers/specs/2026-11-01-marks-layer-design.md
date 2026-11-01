# Marks layer: canvases and brushes

Date: 2026-11-01. Requested by Aedan. Path: architectural (spec, then plan per part).
Ideas list: https://claude.ai/artifact/JkHGTW5XrymPXLeT8q7Ykj
Design doc (approved 17:15Z): https://claude.ai/artifact/55RqjdPPRbYvvcEFGTwBsJ

## Why

Walls and balls react to hits only for a moment today: flashes, particles, camera shake,
colours that follow hits. Nothing leaves a mark that stays. Satisfying ball videos often
build a picture over time: paint trails, splats, a scratch card being revealed, a colour war
filling the screen. This adds one shared "marks layer" that all of those sit on.

## What we agreed

- **Scope: all seven marks ideas.** Paint canvas, hit splats, scratch-card reveal, colour war
  fill, colour pickup, drips, and sand/snow/fog.
- **Setup:** a new **Canvas** thing in the tree, plus **brush** settings in a new **Marks**
  module slot on balls, walls and obstacles. Several canvases per scene are allowed.
- **Area:** a canvas covers a fixed rectangle of the world. By default it fits the arena (or
  the starting view when there is no arena). It can be dragged and can ride a thing.
- **Drives the scene:** canvases report live numbers usable in text, rules and the synth.
- **Look:** each canvas has a **Style**.
- **Engine approach 1:** the simulation keeps a coarse, exact grid plus a log of every mark; the
  graphics card redraws the marks sharp and applies the Style.
- Old scene files need not open (standing project rule).

## Design

### 1. The Canvas thing (`bpe-kit/src/things/canvas/`)

Kind id `"canvas"`, name "Canvas". It has no module slots: it is marked, it doesn't
move or bounce. Settings:

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `kind` | choice Paint, Cover, Powder, Fog | Paint | What brushes do to it |
| `size` | Vec2 world px | arena size, else video size | Dragged in the preview |
| `position` | Vec2 | arena centre, else 0,0 | Normal placement; can Ride a thing (existing attaching) |
| `layer` | choice Behind everything, Behind balls, On top | Paint/Powder: Behind balls; Cover/Fog: On top | Draw order |
| `brushes_from` | Target | Anything | Which Marks modules mark it (existing Target picker) |
| `starts_as` | choice Empty, Colour, Gradient, Picture, Text | Paint: Empty; Cover: Colour; Powder/Fog: Colour | Gradient uses the paint system; Picture uses the new picker (part 2); Text uses the text thing's font |
| `start_colour` | colour | per kind | Sand beige, snow white, fog grey-white, cover silver |
| `style` | choice (per kind, section 3) | Flat / Matte / Sand / Frost | Visual only |
| `detail` | 0.25 to 1 | 1 | Texture texels per world pixel; visual only |
| `dry_time` | s | 1 | Paint stays wet this long |
| `fade` | s or Never | Never | Marks fade out after this long |
| `spread` | 0 to 1 | 0.2 | How far wet paint bleeds |
| `drips` | 0 to 1 | 0 | How much wet paint runs down (gravity direction) |
| `refill` | s or Never | Cover: Never; Powder/Fog: 4 | Time to come back fully |
| `blend`, `opacity` | existing blend modes | Normal, 1 | Reuses the colour system's blend settings |

`affects_physics` says no for `style`, `detail`, `blend`, `opacity`, `layer`; editing them keeps
playing. Every setting keyframes and pulses like any other.

Add thing gets four starting points that set `kind` and `style` and add a Marks starter module
on the scene's balls when they have none: **Paint canvas**, **Scratch card**, **Sand**,
**Fog glass**.

### 2. Brushes: the Marks module (`bpe-kit/src/modules/marks/`)

New `SlotId::Marks` (shown as "Marks"), allowed on balls, emitters, soft bodies, walls (arena)
and obstacles. One module kind `marks.brushes` with four sections, each with an `on` switch:

- **Paint trail** (things with bodies): `width` mode Fixed / By size / By speed with value and
  range; `shape` Round / Square / Calligraphy (+ angle); `colour` Ball / Gradient / Set;
  `load` (paint that runs out over distance, refills on each hit; 0 = endless); `when` Always /
  After first hit / Touching a wall / Faster than (+ speed); `pickup` 0 to 1.
- **Splat** (any): `shape` Splat / Ring / Star / Crack / Burst / Dots / Picture; `size` with
  By hit speed amount; `turn` Face the hit / Random; `variants` 1 to 8; `colour` Ball / Thing
  hit / Set; `min_speed`. On a body-owning thing it marks its own hits; on a wall or obstacle it
  marks whatever hits it.
- **Scrape** (things with bodies): `width`, `strength` (share removed per pass, 1 = one pass
  clears it), `ridges` (powder pushed aside into raised edges).
- **Claim** (things with bodies): `width`, `team` By thing / By ball group / By colour,
  `take_over` (can claim others' ground).

A brush marks every canvas whose `brushes_from` matches the brush's thing, and that has the
right kind: Paint trail, Splat and Claim mark Paint canvases; Scrape marks Cover, Powder and
Fog. A section with no matching canvas does nothing.

### 3. Styles (graphics card only)

| Kind | Styles | Own knobs |
| --- | --- | --- |
| Paint | Flat, Wet paint, Watercolour, Neon ink | light angle, gloss / grain, glow |
| Cover | Matte, Foil | light angle, roughness |
| Powder | Sand, Snow | light angle, grain size |
| Fog | Frost, Steam | blur, drops |

Part 1 ships Flat only (plus a plain look for each kind in part 3); the rest arrive in parts 2
and 3.

### 4. Numbers

Each canvas computes, 60 times a second, from its grid:

| Name | Means | Kinds |
| --- | --- | --- |
| `covered` | share of cells with paint, 0 to 1 | Paint |
| `revealed` | share of start material removed | Cover, Powder, Fog |
| `share <team>` | a team's share of claimed cells | Paint with Claim |
| `leader` | the team with the largest share (name) | Paint with Claim |
| `wet` | share of painted cells still wet | Paint |

- **Text tokens** (`things/text/fill.rs`): `{<canvas name>.covered}`, `.revealed`, `.wet` as
  whole percent; `{<canvas name>.share <team name>}`; `{<canvas name>.leader}`. A team name is
  the claiming thing's name (By thing), its group name, or its colour name. Unknown tokens stay
  as written (existing rule).
- **Rules:** When block **When a canvas reaches** (canvas, number, team when needed, level,
  Rising / Falling): fires once per crossing, comparing the canvas's previous and current
  value. Do block **End the video** (`after` s, default 2): new `Action::EndVideo { after }`
  sets `World.end_step` once (the first one wins). Export and the timeline use
  `min(length, end time + after)`; the timeline shows a marker.
- **Synth:** `SCENE_INPUTS` gains `canvas` "Canvas" with extra fields canvas, number and team;
  reads the 60 Hz canvas table in the log. Listed in the Follow… menu. Sound never feeds back.

### 5. Simulation (`bpe-runtime`, `bpe-kit`)

- **Grid.** `CanvasGrid { cell: f32, w, h, tiles: Vec<Arc<Tile>> }`, tiles of 32×32 cells,
  copy-on-write (`Arc::make_mut`) so playback checkpoints share unchanged tiles. A cell is 8
  bytes: `rgb: [u8; 3], amount: u8, wet: u8, team: u8, depth: u8, flags: u8`. Cell size starts
  at 4 world px and doubles until the grid has at most 300,000 cells.
- **State.** `CanvasState { grid, last_positions per (thing, item), paint_load, numbers,
  prev_numbers, active_tiles }` stored in `World.states` under the canvas's id.
- **Order.** The canvas sim's `after_step` runs once per step in the Events stage. For each
  brush thing (in tree order) and each living item: Paint / Scrape / Claim stamp a capsule from
  the last position to the current one; Splat stamps at this step's `Hit` events. Stamps use
  integer cell maths (fixed-point 16.16 for positions and radii) so results are the same on
  every machine.
- **Evolution** at 30 Hz (every `steps_per_second / 30` steps), active tiles only: wet → dry
  countdown, spread (neighbour averaging of wet cells), drips (a wet cell with more than a
  threshold amount moves part of it one cell along gravity, logging a drip mark), refill
  (amount back towards full), fade (amount down). A tile leaves the active set when nothing in
  it is changing.
- **Pickup.** For each ball with `pickup > 0`, the wet paint under its centre is mixed into
  `Bodies.tint` by `pickup × wetness`, then that cell's paint is reduced by the same share.
- **Nothing else touches physics.** Balls never bounce off marks in this feature.
- **Log.** `Log.marks: Vec<Mark>` with `Mark { step, canvas, kind (Stroke, Stamp, Scrape,
  Claim, Drip), from, to, radius, shape, angle, colour, team }`, and `Log.canvas:
  Vec<CanvasSample>` at 60 Hz with every canvas's numbers. Neither is part of the log hash, so
  existing golden logs stay identical.

### 6. Graphics (`bpe-gfx/src/canvas/`)

- `Frame.canvases: Vec<CanvasLayer>`: canvas id, world rectangle and pose, layer, style and
  knobs, blend, the marks to draw since the last frame, and the time.
- Per canvas two textures at `size × detail`, long side capped at 4096: `paint`
  (Rgba16Float: colour × amount) and `aux` (Rgba16Float: time painted, thickness, team, wet
  start). Marks are drawn as instanced capsules and stamps; splat shapes come from a small
  built-in atlas plus loaded pictures.
- Drying, fading and refilling are computed in the style shader from the time channel, so they
  cost nothing per frame. Spread and watercolour bleed use a blur pass on the 30 Hz clock.
- Scrubbing: texture checkpoints every 2 s within the existing GPU flow checkpoint budget;
  a seek redraws marks from the nearest checkpoint. Export draws straight through.
- Composited in its layer: Behind everything after the background, Behind balls with the
  walls pass, On top before text.

### 7. Pictures (part 2)

A picture picker (PNG or JPG) for Cover `starts_as = Picture` and Splat `shape = Picture`.
The file is copied next to the scene file into `<scene name> pictures/` and referenced by
relative path, so a scene folder stays portable. Uses the `image` crate. A missing file shows
a status-line problem and draws a checker pattern.

### 8. Editor (`bpe-app`)

- Add thing → Canvas submenu with the four starting points.
- Preview: dashed outline with handles for the canvas rectangle (snaps to grid); marks drawn
  live; scrubbing exact.
- Inspector: Canvas card sections Kind, Area, Look (Style, Layer, Blend, Detail), Over time
  (Dry time, Fade, Spread, Drips, Refill). Marks card: one section per brush with its switch.
- Rules menu: the new When and Do blocks. Text card token help lists canvas tokens. Synth
  catalog and Follow… menu list the Canvas input. Timeline shows the End the video marker.
- Demo scenes: Paint pour, Splat ring (part 1); Scratch to reveal, Colour war, Sand garden
  (part 3).

## Build in three parts

1. **Paint canvas.** Canvas thing (Paint kind only), grid, tiles, checkpoints, Marks slot with
   Paint trail and Splat (built-in shapes), mark log, GPU canvas with Flat, dry, fade, layers,
   blend, editor parts, Paint pour and Splat ring.
2. **Rich paint.** Wet paint, Watercolour, Neon ink; spread, drips, pickup; picture picker and
   Picture splats.
3. **Reveal and colour war.** Cover, Powder, Fog kinds with Scrape and their styles; Claim;
   numbers in text, rules (with End the video) and synth; Scratch to reveal, Colour war, Sand
   garden.

Each part: its own plan, PR, short video, screenshots and Windows exe.

## Testing

- Determinism: the same scene run twice gives byte-identical grids and mark logs.
- Seek: jumping to a time gives the same grid as playing straight there; the GPU frame after a
  seek equals the straight-run frame.
- Golden logs of existing scenes are unchanged.
- Stamping unit tests: a capsule covers the expected cells; splats at hit points; drips move
  downwards; refill reaches full in `refill` seconds.
- Numbers: in test scenes `revealed` reaches 1, the When block fires exactly once per crossing,
  End the video trims the export length, and text tokens fill.
- Stills of every style saved with `BPE_SAVE_RENDERS` for a visual check.
- Speed: canvas step under 1 ms for a full portrait screen with 20 painting balls (release).

## Left for later

- Paint that changes physics (sticky, bouncy or slippery areas).
- Canvases in 3D mode.
- Other ideas-list groups: reactive surfaces, breaking things, sound walls.
