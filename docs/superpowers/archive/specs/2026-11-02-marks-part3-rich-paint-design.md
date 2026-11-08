# Marks part 3: rich paint

Date: 2026-11-02. Builds on `2026-11-01-marks-layer-design.md` (part 1, PR #64) and
`2026-11-02-marks-part2-skins-design.md` (part 2, PR #65). Requested by Aedan; the design below
was agreed with him before this spec was written.

## Why

Parts 1 and 2 lay paint down and leave it as it landed: flat colour that never changes. Real paint
is wet for a while. It shines, it runs, it bleeds into the paint beside it, and a ball that rolls
through it comes out the other side wearing some of it. Part 3 makes the paint feel like paint:
three new looks, paint that changes while it dries, colour that balls pick up, and splats that can
be any picture.

## What we agreed

1. **Styles.** Three new styles beside Flat, which stays as it is:
   - **Wet paint:** glossy fresh paint with a highlight from a light angle. It goes matte as it
     dries. Thicker paint, where marks overlap, catches more light. Settings: Light angle, Gloss.
   - **Watercolour:** soft see-through washes with darker pooled edges and paper grain. Wet paint
     feathers outward. Settings: Grain, Edge darkening.
   - **Neon ink:** bright lines with a coloured glow that fades as the ink dries. Settings: Glow
     size, Glow strength.
   - Every style works on canvases and on skins. A skin's style is picked in its Paint skin group.
2. **Wet paint over time**, on canvases and skins:
   - **Dry time** (seconds, default 1).
   - **Bleed** 0 to 1: wet paint spreads and blends into its neighbours.
   - **Drips** 0 to 1: thick wet paint runs. On a canvas it runs the way gravity pulls. On a skin
     it runs downhill along the thing's surface: it slides down a ring or round a peg and drips
     off the lowest point instead of leaving the line.
   - Drips leave thin tapering streaks that dry like everything else.
   - Scrubbing stays exact.
3. **Pickup.** The Paint trail section gets **Pickup** 0 to 1.
   - A ball rolling through wet paint takes on some of its colour, and that paint thins.
   - The ball's own look shows the picked-up tint, fading back over a new **Pickup fade** time.
   - Its later trail paints the mixed colour.
   - Only wet paint can be picked up.
   - Pickup never changes how anything moves (golden logs stay identical).
4. **Picture splats.**
   - Splat Shape gets **Picture**, with a PNG/JPG file picker.
   - The file is copied into `<scene name> pictures/` next to the scene and stored as a relative
     path.
   - **Picture colours**: Own, or Tinted (the paint colour, keeping the picture's light and dark).
   - Pictures are turned and sized like other splats, on canvases and skins.
   - A missing file shows a status-line note and draws a checker pattern.
5. **How it's built.**
   - The simulation owns everything that changes the picture over time. At 30 Hz, on tiles with
     wet paint, it handles drying, bleed (averaging wet neighbours), drips (moving paint downhill
     and logging drip marks) and pickup (mixing into the ball's tint).
   - The graphics card replays the logged marks, drip and bleed marks included.
   - Each style is a composite shader that reads the time painted and the thickness of each texel,
     so drying, gloss and glow cost nothing per frame.
   - Pictures are decoded once and kept on the graphics card.
6. **Demos:** Wet pour, Watercolour garden, Neon splat ring, Colour mixing.
   - **Tests:** determinism, scrub equals play, drips run downhill on skins, pickup only from wet
     paint, picture load and missing picture, and each style tells wet from dry.
   - Then one final review, the exe and a PR. No videos or screenshots this time.

## Design

### 1. How a grid cell remembers wet paint (`bpe-kit/src/things/canvas/grid.rs`)

A cell stays 8 bytes. Its fields change to:

| field | type | meaning |
| --- | --- | --- |
| `rgb` | `[u8; 3]` | paint colour (sRGB) |
| `amount` | `u8` | how much paint is here: its thickness; 0 means untouched |
| `painted` | `u16` | the 30 Hz tick it was last painted at, wrapping |
| `team` | `u8` | kept for part 4's Claim |
| `by` | `u8` | which ball laid it: a tag from the ball's thing and number; 0 for drips and bleed |

Decisions:

- **Wetness is worked out, never counted down.** A cell is wet while
  `now_tick − painted < dry_ticks` (u16 wrapping maths). Drying therefore writes nothing. Tiles
  that only dry stay shared between playback checkpoints, and the graphics card dates its texels
  the same way (the time painted, part 1's `aux.r`), so the two agree without the GPU ever reading
  the grid. Wrapping happens after 36 minutes at 30 Hz, far past any video.
- **Thickness adds up.** Each mark adds its own *ink* to `amount` (saturating at 255): a stroke
  96, a Spread smear 160, a splat or print 200, a picture 96 and a drip 40. Parts 1 and 2 stored
  "the larger amount". A ribbon's pieces must not count their shared joints twice, so the grid
  now cuts pieces exactly like the stamp shader: the start cap is left out when a piece
  continues the one before, and the end cap when the next one continues it. Only real overlaps
  thicken paint: a trail crossing a trail, a splat on a trail.
- **New paint covers old paint.** It mixes by its colour's alpha, as the graphics card blends it.
  Wet colours mix with each other only through Bleed.
- **Tiles know when they were last painted.** `tile_tick` is one u32 per tile. A tile is active
  while it has wet paint, and only active tiles and the tiles touching them are evolved.

### 2. The 30 Hz tick (`bpe-kit/src/things/canvas/evolve.rs`, new)

- Every `max(1, steps_per_second / 30)` steps, each canvas and each skin evolves once. This
  happens at the end of its own Events-stage work, after that step's brush marks.
- A tick happens only when there is something to do: Bleed > 0, Drips > 0, or (canvases only)
  some brush that marks the canvas has Pickup > 0. Every existing scene does no tick work, so it
  costs exactly what it did.
- The order inside a tick is fixed: **drips** (move the runners, then start new ones), then
  **pickup** (canvases), then **bleed**.
- The tick only reads and writes the canvas's own grid and runners, `World.picked` and the log.
  It never touches bodies, walls or events.

### 3. Bleed

- **Simulation.** For each cell in the active tiles and their four neighbour tiles, reading a
  copy of the grid from before the tick (so the order never matters):
  - A wet painted cell moves its colour toward the average colour of its wet painted 4-neighbours
    by `bleed / 4` per tick.
  - An empty cell next to wet cells holding at least 48 paint gains
    `(their amounts added) × bleed / 4`, capped at 255. It takes their amount-weighted colour and
    the newest of their paint times, so it is wet until they dry. Its `by` is 0.
  - Thin paint therefore spreads only a cell or two, and nothing spreads once dry.
- **On the graphics card: a bleed pass, driven by bleed marks.** For every tile it processed,
  the tick logs one `Bleed` mark. Its `from`/`to` hold the tile's rectangle (grown by one cell) in
  canvas pixels, its `radius` holds the dry time in seconds and its `radius_to` the Bleed amount.
  After stamping that step's marks, the graphics card copies the paint and time textures under
  those rectangles to a scratch pair. It then runs one blending pass over the rectangles:
  - Each texel that is wet at the mark's time blends with its wet neighbours `1 + 2·bleed` texels
    away.
  - An empty or dry texel beside wet ones gains some of their paint and their paint time.
- **Why not one mark per cell?** That would be hundreds of thousands of marks a second. Why not a
  blur every frame? That would depend on the frame rate and not be seek-exact. The pass runs only
  at the layer steps that carry bleed marks. It reads only logged data and the textures, and its
  state is checkpointed with the textures, so seeking back replays it texel for texel. The
  picture's bleed is finer than the grid's (texels against 4-pixel cells). The grid's version
  only decides what pickup and drips see.

### 4. Drips

A drip is a **runner**: a small blob of paint the simulation moves each tick.

- **Starting.** In active tiles, a cell starts a runner when all of these hold:
  - it is wet;
  - its amount is at least `255 − 150·drips`;
  - the next cell downhill (along gravity's larger axis, in the canvas's own frame) is below that
    amount, so it is the low edge of a thick patch;
  - a hash of (canvas, tick, cell) falls under `0.08 + 0.25·drips`.

  At most 2 runners start per tile per tick, and at most 96 run per canvas or skin. The cell
  gives up 48 of its paint. The runner takes the cell's colour and a load of
  `clamp((amount − threshold/2)/255, 0.2, 1)`.
- **On a canvas** a runner moves `(30 + 150·load) px/s` along gravity in the canvas's frame. It
  wobbles by up to ±0.15 rad from a hash. Each pixel travelled costs `1/(60 + 200·drips)` of the
  load.
  - Each tick logs one `Drip` mark: a capsule from where the runner was to where it is, its
    radius going from `r(load before)` to `r(load after)`, with
    `r(load) = (1 + 2.5·drips)·√load` px.
  - The mark is stamped into the grid as wet paint with ink 40 and `by` = 0, so streaks dry,
    bleed and can be picked up like any paint.
  - When the load runs out, the last piece tapers to 0. A runner that leaves the canvas is
    dropped.
- **On a skin** a runner follows the surface. It keeps which outline line it is on and its offset
  across the line. Each tick, against gravity turned into the thing's own frame:
  - It moves along the line the way gravity's pull along the line points, at
    `(30 + 150·load)·max(|pull|, 0.25)` px/s, turning corners and wrapping round closed lines.
  - It stops at the **lowest point**: where the pull along the line changes sign at a corner,
    where the line is flat (|pull| < 0.05), or at the end of an open line. There it leaves a drop,
    a round Drip mark 1.6× its streak radius, and is gone.
  - So paint on a ring arena gathers at the bottom of the ring, paint on a peg runs round to the
    peg's underside, and paint on a spinning bar slides toward whichever end is lower right now.
  - Outlines come from part 2's `outline_local`. Only the runner's own line is searched, so peg
    grids stay cheap.
- **Joins.** A runner's pieces join without darker beads. Each piece after the first leaves out
  the disc of its start radius round its start, and the half-plane behind its start along the
  previous piece's direction. This is a new variant bit, 8, read by the stamp shader and the grid
  alike.
- **No gravity, no drips.** With zero gravity nothing runs.

### 5. Pickup

- **Settings** (Marks, Paint trail section):
  - `paint_pickup` "Pickup" 0..1, default 0. It shows even when Paint trail is off, because a ball
    that doesn't paint can still pick up.
  - `paint_pickup_fade` "Pickup fade" 0..30 s, default 3. It shows when Pickup is above 0; 0 means
    the tint never fades.
- **Where the tint lives.** It lives in a new `World.picked: BTreeMap<ThingId, Vec<Picked>>`, one
  entry per ball row, grown when first written. It is cloned with the world, so playback and GPU
  checkpoints rewind it. It is not in the fingerprint and not in the log hash.
  - `Picked { rgb: [f32; 3] (linear), amount: f32, step: u64, fade: f32 }`.
  - The shown amount is worked out when it is read: `amount − (seconds since step) / fade`,
    floored at 0. So fading costs nothing and needs no per-step update.
- **On a canvas**, at each tick, for every brush that marks the canvas and has Pickup > 0, in
  world order, for each living ball:
  - The cells within half the ball's radius of its centre (at least the centre's cell) are read.
    Only wet cells with paint whose `by` is not the ball's own tag count: a ball never "picks up"
    its own trail.
  - If any count, the ball's share is `s = min(1, pickup × (wet cells / cells read) × 0.35)`. The
    ball mixes their amount-weighted colour into its tint by `s`. Each of those cells loses
    `s × 0.5` of its amount (at least 1).
  - A `Lift` mark is logged: a disc of that radius at the ball's centre, whose `radius_to` is the
    share removed. On the graphics card it multiplies paint and thickness by `1 − share`. Lift
    marks never re-date texels.
- **On a skin**, the same happens at each hit of the ball on that skin's thing, around the
  contact point. A ball rolling along a wall without logging hits picks nothing up from it.
- **What shows the tint.** It goes over the look colour and over a rule's tint, as
  `rgb = lerp(colour, picked.rgb, amount)`, in one shared function used by:
  - the ball's drawn colour (`ball_colors`, so trails and particles that follow the ball follow
    too);
  - the simulation's colour for marks (`SimLook::colour`);
  - marks whose brush says Colour from Own (`Colours::of`).

  So a ball's later trail paints the mixed colour.
- **Never changes motion.** Pickup reads the grid and writes only `World.picked`, cells and marks.
  The golden logs of every scene, the new Colour mixing demo with Pickup on or off included, are
  identical.

### 6. Picture splats

- **Settings** (Marks, Splat section): `SplatShape::Picture`; `splat_picture` "Picture" (text,
  `unit = "picture"`: the inspector shows a browse button that filters PNG and JPG); and
  `splat_picture_colours` "Picture colours" with the choices Own and Tinted. Both show only for the
  Picture shape.
- **The picture table.** At build, the kit puts every distinct non-empty picture path of the
  brushes into `World.pictures` (sorted, at most 16). A Picture mark names its picture by slot:
  `variant` bits 0 to 4 hold the slot (31 means "not in the table"), and bit 5 means Tinted.
  `Mark` gains no fields.
- **Simulation.** A picture splat is stamped into the grid as a disc of 0.7 × radius in the paint
  colour. The simulation never reads image files, so Own-colour pictures count as the paint colour
  for pickup and drips. A brush whose Picture is empty draws an ordinary Splat.
- **Turn and size** are as for other splats. The picture fits a square of side `2 × radius`. Facing
  the hit stands the picture up away from the surface (a hit on the floor shows it upright);
  Random turns it at random. On a skin, Print pushes it into the surface as part 2 does.
- **Graphics card** (`bpe-gfx/src/canvas/pictures.rs`, new):
  - A `PictureAtlas` holds a 2D array texture of 512×512 Rgba8UnormSrgb layers, one per slot,
    with a full mip chain built on the processor.
  - Each picture is decoded once with the `image` crate (PNG, JPG) and scaled to fit 512 with its
    shape kept, letterboxed with transparent edges.
  - A file that is missing or can't be read gets a checker layer: 32 px squares of pink `#ff4fd8`
    and near-black `#202020`. Slot 31 draws the same checker from the shader.
  - The atlas is loaded when the flow layers restart, from the private world's `World.pictures`.
    Paths resolve against `FlowSource.picture_folder`.
  - Own uses the picture's colours with its alpha (times the paint's alpha). Tinted uses
    `paint rgb × lightness × 1.25`, where lightness is the picture's sRGB luma from 0 to 1.
- **Files** (`bpe-kit/src/pictures.rs`, new):
  - A relative path resolves against the scene file's folder.
  - When the scene has a file, any picture outside `<scene file name> pictures/` is copied in
    right after the edit that picked it, and the setting is rewritten to the relative path, with
    `/` separators.
  - A name clash with a different file gets " 2", " 3" and so on; an identical file is reused.
  - A scene that has never been saved keeps the full path until its first save, which copies the
    pictures in. Save As to another folder copies them again.
  - A batch copies the scene with full paths, so it renders from any folder.
- **Status line.** "Picture not found: star.png (a checker shows instead)". It also says when a
  scene names more than 16 pictures.

### 7. Styles on the graphics card (`canvas.wgsl`, `canvas_stamp.wgsl`)

- **A third texture per canvas and skin.** `body` is R16Float: thickness. Each mark adds
  `ink/255 × coverage` to it with additive blending. Lift marks multiply it down. Checkpoints copy
  it with the other two.
- **Time painted** stays in `aux.r`. Wetness in the composite is
  `clamp(1 − (now − painted) / dry_time, 0, 1)`; a dry time of 0 is always dry.
- **`CanvasLayer.look: PaintLook`** carries style, dry time, light angle, gloss, grain, edge
  darkening, glow size and glow strength into the composite uniform.
- **The styles:**
  - **Flat** is unchanged and ignores wetness, so existing pictures stay the same.
  - **Wet paint.** The height is thickness plus 0.35 × coverage. Its slope, measured 1.5 texels
    each way, bends a normal. Light comes from the Light angle: degrees clockwise from straight up,
    default 315 (top left), held fixed in the world so a spinning bar's highlight doesn't spin. The
    highlight is the specular gain over a flat surface: only slopes that face the light shine.
    Overlaps and edges catch it, and flat insides don't. Shininess runs from 10 to 60 by Gloss. Dry
    paint keeps 12% of the gloss, so it goes matte. Wet paint is a touch darker.
  - **Watercolour.**
    - Coverage is read through a soft disc 1 texel wide when dry and 4 when wet (eight taps), so
      wet paint feathers.
    - The wash is see-through (alpha × 0.75).
    - Edges darken by `edge darkening × clamp((coverage − blurred coverage) × 3, 0, 1) × 0.6`.
    - Value-noise paper grain, fixed to the canvas, thins and darkens the paint by Grain. It also
      shows faintly on a Starts as Colour canvas.
  - **Neon ink.**
    - The line is drawn bright: `colour × (1.5 + 1.5·wet)`.
    - Glow comes from 16 taps in a disc of Glow size, each weighted by its own wetness and its
      distance. It is added (premultiplied with alpha 0) times Glow strength, so a dry line keeps
      its brightness and loses its glow.
    - On a skin, the line is masked as before and the glow is masked by a widened mask that fades
      out over Glow size past the thing's edge.
    - Glow stops at the canvas's or skin's own rectangle.
- **The stamp pipeline gains:**
  - shape numbers Drip 8, Picture 9, Lift 10, Bleed 11;
  - the `body` target;
  - the picture array and its sampler;
  - a second pipeline for Lift: in the same pass, with a multiplying blend and no depth write;
  - a third pass for Bleed, with its own small shader `canvas_bleed.wgsl`.
- **Order in each layer step:** stamps in log order (drips and pictures included), then lifts,
  then bleed. This is also the simulation's order, because a tick is the last world step of its
  layer step whenever the physics rate is a multiple of 120 (the default 240 is). At odd rates
  the order can differ slightly, which stays deterministic and seek-exact.

### 8. Settings, keys and groups

**Canvas** (`CanvasSettings`, thing card). The new rows go after Style in Look, and before Fade in
Over time:

| key | label | type / range | default | when | group |
| --- | --- | --- | --- | --- | --- |
| `style` | Style | Flat, Wet paint, Watercolour, Neon ink | Flat | | Look |
| `light_angle` | Light angle | 0..360 ° | 315 | `style=wet_paint` | Look |
| `gloss` | Gloss | 0..1, keyframe | 0.6 | `style=wet_paint` | Look |
| `grain` | Grain | 0..1 | 0.5 | `style=watercolour` | Look |
| `edge_dark` | Edge darkening | 0..1 | 0.5 | `style=watercolour` | Look |
| `glow_size` | Glow size | 2..80 px | 18 | `style=neon_ink` | Look |
| `glow_strength` | Glow strength | 0..3, keyframe | 1 | `style=neon_ink` | Look |
| `dry_time` | Dry time | 0..30 s | 1 | | Over time |
| `bleed` | Bleed | 0..1 | 0 | | Over time |
| `drips` | Drips | 0..1 | 0 | | Over time |

- `affects_physics` is false for the visual keys: `light_angle`, `gloss`, `grain`, `edge_dark`,
  `glow_size`, `glow_strength`, plus part 1's list.
- `dry_time`, `bleed` and `drips` change the marks, so editing them restarts the playback.
- None of the three is keyframed.

**Skin** (`SkinSettings`, group "Paint skin"). Each key is the canvas's with `skin_` in front:
`skin_style`, `skin_light_angle`, `skin_gloss`, `skin_grain`, `skin_edge_dark`, `skin_glow_size`,
`skin_glow_strength`, `skin_dry_time`, `skin_bleed`, `skin_drips`.

- They have the same labels, ranges and defaults. Each `when` gains `skin_on and …`.
- They come after Opacity, in the order Style, the style's knobs, Dry time, Bleed, Drips.
- `affects_marks` adds `skin_dry_time`, `skin_bleed` and `skin_drips`. The rest are visual.

**Marks brush** (module; groups are already in `SECTION_ORDER`):

| key | label | type / range | default | when | group |
| --- | --- | --- | --- | --- | --- |
| `paint_pickup` | Pickup | 0..1 | 0 | | Paint trail |
| `paint_pickup_fade` | Pickup fade | 0..30 s | 3 | `paint_pickup` | Paint trail |
| `splat_shape` | Shape | + Picture | Splat | | Splat |
| `splat_picture` | Picture | text, unit `picture` | "" | `splat_on and splat_shape=picture` | Splat |
| `splat_picture_colours` | Picture colours | Own, Tinted | Own | `splat_on and splat_shape=picture` | Splat |

### 9. Performance and memory

- **Processor.**
  - The tick touches only active tiles and their neighbours. Writes happen only where a cell
    really changes, so copy-on-write keeps untouched tiles shared.
  - Runners are capped at 96 per canvas or skin. Drip marks are at most 96 × 30 a second; lift
    marks one per ball per tick while picking up; bleed marks one per processed tile per tick.
  - Budget: a busy portrait canvas with 20 painting balls, Bleed 0.5, Drips 0.5, Pickup 0.5 and
    Dry time 2 s must keep its mean canvas step under 1 ms in release (speed test). Part 2's skin
    speed test stays under 1 ms with Skin drips 0.5 added.
- **Memory.**
  - The grid stays 8 bytes a cell (about 2.4 MB for a portrait canvas at 4 px cells).
  - Runners are under 8 KB per canvas, and pickup is 32 bytes a ball.
  - The graphics card adds 2 bytes a texel for `body` (4 MB for a full portrait canvas). It also
    holds, only while bleed is on, 16 bytes a texel of scratch that is never checkpointed. The
    picture atlas is up to 16 × 1.4 MB.
  - Checkpoints keep part 1's 256 MB budget and simply space out sooner.
  - The log's marks grow by at most about 3,000 + 30 × balls + 30 × active tiles a second.

### 10. Editor

- **Canvas card:** Look gains Style's knobs; Over time gains Dry time, Bleed and Drips.
- **Arena and obstacle cards:** the Paint skin group gains Style, the knobs, Dry time, Bleed and
  Drips.
- **Marks card:** Paint trail gains Pickup and Pickup fade. Splat gains the Picture shape, the
  Picture row (with its browse button) and Picture colours.
- The status line names missing pictures. The CLI renderer resolves pictures against the scene
  file's folder. Studio exports use the scene's folder.

## Demos

Built-in scenes are added to `scenes::marks`, and their files are written to `scenes/next/`.
New golden lines are added; existing lines are byte-identical.

- **Wet pour:** Paint pour in Wet paint style. The balls also splat the canvas (Lands on
  Canvases), so splats drip. Dry time 2.5, Drips 0.6, Bleed 0.15, Gloss 0.7.
- **Watercolour garden:**
  - A cream paper canvas (Starts as Colour `#f4efe4`) in Watercolour style: Dry time 3, Bleed
    0.5, Grain 0.6, Edge darkening 0.6.
  - Twelve soft-coloured balls fall through a staggered peg grid. Their wide trails and their
    splats land on the paper.
- **Neon splat ring:** Splat ring's ring and bar in Neon ink. The ring has Skin drips 0.5, so
  paint slides down the ring to its bottom. Dry time 2.5 on both. Neon pink, cyan and lime balls.
- **Colour mixing:**
  - Red, yellow and blue balls paint thick Wet paint trails (Dry time 4, Bleed 0.25) with
    Pickup 0.6 and Pickup fade 4.
  - Where they cross each other's wet trails they take on each other's colour and paint the mix.

## Tests

- **Determinism:** the same run twice gives identical grids, runners, pickup and marks, on a
  canvas and on a skin, with bleed, drips and pickup on.
- **Scrub equals play:**
  - On the processor, the grid hash, marks and log after seeking 8 s then 5 s equal a straight run
    to 5 s.
  - On the graphics card, the textures after seeking back with bleed on equal a straight run, bit
    for bit.
- **Drips:**
  - On a canvas they run along gravity, never with zero gravity, never from dry or thin paint, and
    end tapered.
  - On a ring arena's skin every drip piece stays within the line's reach, and drips end at the
    ring's lowest point.
  - On a peg they run round to its underside. On a spinning bar they stay on the bar.
- **Pickup:**
  - Only wet paint is picked up, never the ball's own.
  - The tint fades over Pickup fade, and the later trail paints the mixed colour.
  - The drawn colour equals the simulation's colour.
  - Lift marks thin the paint, and the golden logs are identical with Pickup on or off.
- **Pictures:**
  - A generated PNG (red left half, blue right half) stamps red on the left and blue on the right,
    turned when the splat turns.
  - Tinted keeps its light and dark.
  - A missing or broken file draws the checker and is named in the status line.
  - A slot past 16 draws the checker.
  - Picking a file copies it into `<scene> pictures/` and stores the relative path; saving
    elsewhere copies it again.
- **Styles:**
  - Each new style tells wet from dry on the same marks: Wet paint's highlight fades, Watercolour's
    footprint shrinks, and Neon's glow goes.
  - Flat is identical wet or dry.
  - Overlaps shine more in Wet paint, the light angle moves the highlight, and neon glow shows
    past a skin's line but not past its glow size.
- **No change where nothing changed:**
  - A scene without Marks keeps its fingerprint, `looks`, skins and pickup empty.
  - Existing golden lines are byte-identical in debug and release.
- **Speed:** the busy canvas and skin tests above, in release.

## Left for later

- Drops that fall off a skin landing on a canvas or another thing below.
- Pickup while rolling along a wall without hits. Picture colours read by the simulation, so
  Own-colour pictures can be picked up in their own colours.
- Re-reading a picture file that changed on disk without picking it again.
- Paint that changes physics (sticky or slippery areas).
- Part 4: Cover, Powder, Fog, Scrape, Claim and canvas numbers.
