# Marks part 4: covers, scraping, colour wars and live numbers

Date: 2026-11-02. Builds on `2026-11-01-marks-layer-design.md` (part 1, PR #64),
`2026-11-02-marks-part2-skins-design.md` (part 2, PR #65) and
`2026-11-02-marks-part3-rich-paint-design.md` (part 3, PR #66). Aedan agreed the design below
(mockup page: https://claude.ai/artifact/CQ7ePV1xcuNL9UXeH8nCo5, answers Q1 = C, Q2 = D,
Q3 = B, then "Approve").

Two build threads work at once from the plans:

- `docs/superpowers/plans/2026-11-02-marks-part4a-canvas.md`: stream **A**, the canvas side.
- `docs/superpowers/plans/2026-11-02-marks-part4b-numbers.md`: stream **B**, everything that
  reads the numbers.

Both start from branch `claude/project-thread-bv0ih5`, whose commit `90f729b` holds the shared
interface (`crates/bpe-runtime/src/canvas_numbers.rs`). Stream B merges stream A's branch at the
end, runs the one review of the part, and opens the one PR.

## Why

Parts 1 to 3 only ever add paint. The videos Aedan wants next take something away or fight over
ground: a ball scratching a lottery card open, balls raking a sand garden, a finger-on-glass fog
wipe, and colour wars where each ball's team grabs territory while a bar on screen shows who is
winning. Part 4 adds surfaces that balls remove, a way to claim ground for a team, and live numbers
(how much is revealed, each team's share) that captions, rules, the synth and a new Meter read.

## What we agreed

1. **Three new canvas kinds** beside Paint:
   - **Cover**: a layer that balls scrape off. Look **Matte** or **Foil** (a scratch-card
     shine). **Underneath**: the scene behind, a colour, a gradient, a picture, or text. **Grows
     back** off by default.
   - **Powder**: **Sand** or **Snow**. Scraping pushes **Ridges** (0 to 1) up along the edges of
     the track. Grows back, 4 s by default in the Add-menu starting points.
   - **Fog**: **Frost** or **Steam**, drawn on top of everything, blurring what is behind it.
     **Blur** sets how much. Fogs back, 4 s by default in the starting points. Steam has
     **Drops**: drips that run down from wiped edges, leaving clear tracks.
   - The Add menu's Canvas entry becomes a small menu of starting points: Paint canvas, Scratch
     card, Sand, Snow, Fog glass, Colour war.
2. **Scrape** (a new section on the Brushes module):
   - **Scrape path**: the ball wipes along its track. Width from size, fixed, or speed, like the
     paint trail.
   - **Chip at hits**: a hit knocks out a splat-shaped piece (the six splat shapes), bigger for
     harder hits, above a slowest hit speed.
   - **Strength** 0 to 1: how much one pass takes away.
   - It works on Cover, Sand, Snow and Fog. On a Paint canvas it wipes paint away.
3. **Claim** (a new section on the Brushes module, and a **Colour war** group on Paint canvases):
   - Brushes: **Claim path** (ground along the track), **Claim at hits** (a disc at each hit),
     **Team** By thing or By colour.
   - The canvas: **Tile size** (0 is smooth ground, more is a grid of tiles), **Grow** (claimed
     ground spreads into its neighbours over time), **Take over** (teams can steal each other's
     ground), **Border** (darker lines between teams).
   - All of these combine. Claimed ground is drawn under the paint.
4. **Numbers**, worked out 60 times a second: **covered**, **revealed**, **wet**, **share** (per
   team) and **leader**.
   - Captions: `{Arena.revealed}`, `{Arena.share Red}`, `{Arena.leader}` (`Arena` is the
     canvas's name).
   - Rules: **When a canvas reaches** (a level, going up or down, firing once per crossing) and
     **End the video** (after a few seconds). Export and the timeline stop at the end.
   - Synth: a **Canvas number** scene input.
5. **Meter** (a new thing, under On screen next to Text):
   - Reads a canvas and a number: each team's share, or revealed, covered or wet.
   - **Look**: Split bar, Ring or Ranking. Size, place, **Smooth**, **Show percent**, **Show
     names**. Sticks to the screen like Text and keyframes like any thing.
   - A single number (revealed, covered, wet) shows as a progress bar or a ring.
6. **How it is built.** Like parts 1 to 3, the simulation's grid is the truth. Paint stays marks
   replayed on the graphics card. The new kinds and claimed ground are drawn straight from the
   grid. Nothing moves the balls, so every existing golden line stays identical.
   - Demos: Scratch to reveal, Colour war (tiles), Territory (grow), Sand garden, Fog glass.
   - Two build threads at once, one review at the end, then the exe and a PR. No videos or
     screenshots.

## Design

### 1. The shared interface (done, commit `90f729b`)

`crates/bpe-runtime/src/canvas_numbers.rs`, re-exported from `bpe_runtime`:

- `CanvasNumbers { covered, revealed, wet, shares: Vec<f32> }` (all 0 to 1; a share is the team's
  part of the whole canvas), with `share(i)`, `unclaimed()`, `leader()`.
- `Team { name, colour }` (colour in linear light).
- `CanvasSample { step, canvas, numbers }`, kept in the log 60 times a second.
- `CanvasRef { canvas, number, team }` with `parse("Arena.share Red")`, `value(..)` and
  `text(..)`. `CANVAS_NUMBERS = ["covered", "revealed", "wet", "share", "leader"]`.
- `World`: `canvas_numbers`, `teams`, `names` (every thing's name), `canvas_named(name)`,
  `canvas_numbers_of(id)`, `teams_of(id)`, `canvas_value(&ref)`, `canvas_text(&ref)`,
  `report_canvas(id, numbers, log)` (every step; it logs a sample on steps that are multiples of
  `steps_per_second / 60`), `set_teams(id, teams, log)`.
- `Log`: `push_canvas`, `canvas_samples()`, `canvas_samples_of(id)`, `canvas_at(id, step)`,
  `set_teams`, `teams_of(id)`. Samples are cut on rewind (`truncate_motion`). Neither samples nor
  teams are in the log hash.

Stream A writes the numbers; stream B only reads them. B's tests write numbers by hand through
the same calls, so B never waits for A.

### 2. Canvas kinds and their grid (stream A)

`CanvasKind` gains `Cover`, `Powder` and `Fog`.

**The grid's rest value.** A `CanvasGrid` gains `rest: u8`: the amount an untouched cell holds.
Paint is 0 (as today), Cover and Fog are 255, Powder is 160 (so ridges can rise above it to 255).
`CanvasGrid::surface(size, rest, start_cell, max_cells)` builds a grid whose every cell, padding
included, is `Cell { amount: rest, ..zero }`. All tiles start as one shared `Arc`. Paint keeps
`CanvasGrid::new(size)`, which is `surface(size, 0, 4.0, 300_000)`.

| kind | rest | start cell | most cells |
| --- | --- | --- | --- |
| Paint | 0 | 4 px | 300,000 |
| Cover | 255 | 2 px | 600,000 |
| Powder | 160 | 4 px | 300,000 |
| Fog | 255 | 4 px | 300,000 |

**Counters.** Every write to a cell goes through one private helper that keeps, per tile,
`off: u16` (cells whose amount is not `rest`) and `deficit: u32` (the sum of `max(0, rest −
amount)`), and running totals of both. Padding cells are never written. So:

- Paint `covered` = total `off` / real cells (the same as today's `covered()`, without the scan).
- Cover, Powder, Fog: `revealed` = total `deficit` / (`rest` × real cells), and `covered` =
  1 − `revealed`. Powder pushed above `rest` by ridges counts as not revealed.

**What brushes do to each kind.**

- Paint trail and Splat paint only Paint canvases. On the other kinds they do nothing and log
  nothing.
- Scrape path and Chip at hits work on every kind (see §3).
- Claims work only on Paint canvases (see §4).

**Grows back.** A canvas setting **Grows back** (seconds, 0 to 30, default 0; called "Fogs back"
in the Fog starting point's wording but the same setting) applies to Cover, Powder and Fog. On
each 30 Hz tick, every tile with `off > 0` moves each real cell's amount toward `rest` by
`ceil(rest / (grows_back × 30))`, down as well as up (ridges settle). 0 never grows back.

**Fog drops (Steam only).** Each scrape piece on a Steam fog may start a drop at either side of
its end (a chance of `drops × 0.15` each, from a hash of the step and the ball). A drop runs the
way gravity pulls in the canvas's own frame, 40 to 100 px/s (from its hash), for 60 to 200 px,
wiping a disc of radius `max(2.5, cell / 2)` along its way. At most 64 run at once. No drops
without gravity. Drops are in the canvas's state, so seeking replays them.

**Drawing (§5).** Paint canvases are still drawn from their marks. Cover, Powder and Fog are
drawn from the grid: each frame the canvas's drawing part copies the grid into an RGBA8 image (the
amount in red), and the renderer uploads it only when it changed.

### 3. Scrape and chip (stream A)

Brushes gain a **Scrape** section (after Splat in the inspector):

| setting | label | shown when | default |
| --- | --- | --- | --- |
| `scrape_on` | Scrape path | always | off |
| `scrape_width_by` | Width from | `scrape_on` | Size |
| `scrape_width` | Width | `scrape_on` | 1.0 |
| `chip_on` | Chip at hits | always | off |
| `chip_shape` | Chip shape | `chip_on` | Splat |
| `chip_size` | Chip size (px) | `chip_on` | 30 |
| `chip_by_speed` | Size by speed | `chip_on` | 0.5 |
| `chip_min_speed` | Hit speed (px/s) | `chip_on` | 80 |
| `scrape_strength` | Strength | `scrape_on or chip_on` | 1.0 |

Width works exactly like the paint trail's width (`WidthBy`). The scrape path is a second ribbon
per ball (its own map, taper 0, no load), cut into the same smooth pieces 60 times a second. A
chip uses the splat machinery (`splat_look`) with the chip settings, and the six splat shapes.

**Two new mark shapes**: `MarkShape::Scrape` (a capsule with the ribbon cuts, like a stroke) and
`MarkShape::Chip` (a splat shape: the variant's bits 0 to 4 are the splat layout and squash, bits
5 to 7 the shape: 0 splat, 1 ring, 2 star, 3 crack, 4 burst, 5 dots). The strength is the mark's
`colour[3]`. They are logged **only** for Paint canvases, where the graphics card replays them as
wipes. On the other kinds the grid is the picture, so nothing is logged.

**On the grid** a scrape or chip covers the cells whose centres are inside its shape (the
capsule, or the shape's signed distance ≤ 0 at the centre). Strength `s` is 0 to 255:

- Paint cells: `amount = amount × (255 − s) / 255` (rounded down). Colour and date stay.
- Cover, Powder, Fog cells: `amount −= round(s / 255 × rest)`, never below 0.
- Powder ridges: cells in the band just outside the shape (capsule: `r < d ≤ 1.6 r`, behind the
  ribbon cut excluded; chip: `0 < sdf ≤ 0.6 r`) gain `round(ridges × s / 255 × rest × 0.35)`, up
  to 255.

The chip shapes are signed distance functions in WGSL (`canvas_stamp.wgsl`). Stream A ports the
six to Rust (`canvas/shapes.rs`), with the same PCG hash, so the grid and the picture agree.

**On the graphics card** a new `fs_wipe` pipeline scales paint and thickness down by
`1 − strength × cover`, like part 3's lift (the time painted is left alone). Marks are drawn in
runs of stamps, lifts and wipes, in log order.

### 4. Claims (stream A)

Brushes gain a **Claim** section (after Scrape):

| setting | label | shown when | default |
| --- | --- | --- | --- |
| `claim_on` | Claim path | always | off |
| `claim_width_by` | Width from | `claim_on` | Size |
| `claim_width` | Width | `claim_on` | 1.2 |
| `claim_hits` | Claim at hits | always | off |
| `claim_hit_size` | Hit size (px) | `claim_hits` | 40 |
| `claim_hit_by_speed` | Size by speed | `claim_hits` | 0.5 |
| `claim_min_speed` | Hit speed (px/s) | `claim_hits` | 50 |
| `claim_team` | Team | `claim_on or claim_hits` | By colour |

Paint canvases gain a **Colour war** group (shown when `kind=paint`): `tile_size` (Tile size,
0 to 200 px, default 0), `grow` (Grow, 0 to 1, default 0), `take_over` (Take over, default on),
`border` (Border, 0 to 1, default 0).

**The claim map** sits beside the paint grid in the canvas state, only when some claiming brush
marks the canvas. Its cell is `tile_size` when that is above 0, else the paint grid's cell. Each
cell holds a team number (0 none, else team index + 1) in one `Arc<Vec<u8>>`, with a count per
team.

- **Claim path**: 60 times a second each ball claims a capsule from its last claim point to where
  it is now, with the radius the width settings give, plus the cell the ball is in.
- **Claim at hits**: a disc at each hit of radius `claim_hit_size × (1 + claim_hit_by_speed ×
  min(speed / 1000, 2))`, for hits at or above the slowest speed, for the moving ball's team.
- A cell is claimed when its centre is within the radius (smooth ground) or within the radius
  plus half a cell (tiles, so a tile touched anywhere near flips).
- **Take over** off: only unclaimed cells can be taken.
- **Teams.** By thing: one team per claiming thing, named after the thing, coloured by the first
  ball that claimed. By colour: one team per colour (sRGB rounded to 5 bits a channel), named by
  a colour-name table (Red, Orange, Brown, Yellow, Lime, Green, Teal, Cyan, Blue, Purple, Pink,
  White, Grey, Black), with " 2", " 3" for repeats. Teams are added in the order they first
  claim, at most 255. A new team is handed to the world with `set_teams`.
- **Grow**, on the 30 Hz tick, over a copy of the map (so the order of cells doesn't matter):
  an unclaimed cell with claimed neighbours (4-neighbourhood) joins the most common neighbouring
  team with chance `grow × 0.5 × count / 4`. With Take over on, a claimed cell whose neighbours
  hold more cells of another team than of its own joins that team with chance `grow × 0.25 ×
  (theirs − its own) / 4`. Ties go to the lower team number. The chance comes from the SplitMix
  hash of the tick and the cell. Only 32×32 blocks with claimed cells (and their neighbours) are
  visited.
- **Shares** are each team's count over the map's cells.

**Drawing.** The claim map is copied into an sRGB RGBA8 image (team colour, alpha 255 where
claimed). The composite draws claimed ground in place of the canvas's start colour in all four
styles, so paint lies on top. Tiles are sampled nearest (crisp squares), smooth ground linear.
**Border** darkens texels whose neighbour (one claim cell away) belongs to another team.

### 5. Drawing the new kinds (stream A)

`CanvasLayer` gains `surface: Surface` (Paint, or Cover, Powder or Fog with their looks and
settings), `grid: Option<GridImage>` and `under_glyphs: Vec<GlyphInstance>`.

- The flow layers make no textures for non-Paint canvases (their marks are not logged anyway).
- The composite gains a grid texture (binding 6) and the picture atlas (binding 7, with its
  sampler at 8), and uniform fields for the surface, the underneath and a 16-stop gradient.
- **Cover**: the amount, filtered, is smoothed through `smoothstep(0.35, 0.65)` so edges are
  clean at 2 px cells. Matte: the cover colour with fine grain. Foil: diagonal bands with a moving
  shine and grain. A thin shadow on the cover's side of each scratched edge, toward the scene's
  light (315°). Underneath shows where the cover is gone: Scene (nothing drawn), Colour, Gradient
  (top to bottom), Picture (stretched over the canvas), or Text (the colour, with the text laid
  out at the canvas's centre). Text glyphs are drawn by the renderer between an "underneath"
  draw and a "cover" draw of the same canvas.
- **Powder**: height = amount / 255. Lit from the light angle by the height's slope; Sand has
  warm grain, Snow has blue shadows and a little sparkle. Where the powder is gone the scene
  shows through.
- **Fog**: always drawn on top of everything (its Layer setting is hidden). The renderer copies
  the picture so far, then the fog pass blurs it (12 taps, Blur px) by the fog's amount and lays
  the fog colour over it by Density. Frost adds grain; Steam is smoother and lighter.
- **Paint**: claims as above. A Paint canvas without claims draws exactly as before.

### 6. The Add menu and the demos (stream A)

`bpe_kit::add_canvas(scene, reg, start, parent)` adds a canvas set up as a starting point and turns
on the matching brush on the balls (scrape for Scratch card, Sand, Snow and Fog glass; claim for
Colour war). The tree's Add menu shows Canvas as a submenu with the six starting points.

Demos, appended to `scenes::marks` after part 3's: **Scratch to reveal** (a foil card named
"Card" over "YOU WIN" text), **Colour war** (tiles, canvas "War"), **Territory** (smooth ground
that grows, canvas "Land"), **Sand garden** (canvas "Sand"), **Fog glass** (Steam with drops,
canvas "Fog"). Five golden lines are added at the end; no old line changes.

### 7. Captions (stream B)

`{<canvas>.<number>}` tokens fill from the log: the latest sample of that canvas at the drawn
step. A number shows as a whole percent without the `%` sign; `leader` shows the leading team's
name (empty while nothing is claimed). A canvas name that matches no canvas leaves the token as
written; a team that hasn't claimed yet reads 0. `fill` keeps its signature; a new `fill_with`
takes a lookup for unknown tokens.

### 8. Rules (stream B)

- **When a canvas reaches** (`when.canvas`): Canvas (a target; the first canvas it names),
  Number (Covered, Revealed, Wet, Share), Team (for Share), Level (0 to 100 %), Way (Up, Down).
  It fires on the step the number crosses the level the chosen way (up: from below to at or
  above; down: from above to at or below), never on the first value it sees. It can fire again
  after crossing back.
- **End the video** (`do.end`): After (seconds, default 2). The first one wins. The world notes
  the end step (`World::end_step`, `Log::end`); the simulation keeps running so nothing else
  changes. The log keeps it out of its hash and forgets it on rewind past the step it was asked
  at.
- Export draws frames up to the first frame at or after the end (the soundtrack is cut by
  ffmpeg's `-shortest`). The editor's timeline shows the end as a marker and playback stops there.

### 9. Synth (stream B)

`SCENE_INPUTS` gains `canvas` ("Canvas number", 0 to 1). A Scene block reading it has a text
param **Reads** in caption syntax (`Arena.revealed`, `War.share Red`). The scene series gains each
read's values at 60 Hz on the video clock. The Follow menu offers each canvas's covered, revealed
and wet.

### 10. Meter (stream B)

A new thing kind `meter` (family On screen, sticks to the screen, no slots, no sim):

| setting | label | default |
| --- | --- | --- |
| `canvas` | Canvas | anything (the first canvas) |
| `reads` | Reads | Shares (or Revealed, Covered, Wet) |
| `look` | Look | Split bar (or Ring, Ranking) |
| `width` | Width (px) | 800 (not for Ring) |
| `thickness` | Thickness (px) | 56 |
| `ring_size` | Ring size (px) | 160 (Ring) |
| `smooth` | Smooth | 0.5 (mean over `smooth × 0.5` s) |
| `show_percent` | Show percent | on |
| `show_names` | Show names | on |
| `font` | Font | Anton |
| `text_size` | Text size (px) | 36 |
| `text_colour` | Text colour | white |
| `empty_colour` | Empty colour | white at 15 % |
| `fill_colour` | Fill colour | #ffd23e (single numbers) |
| `max_rows` | Rows | 4 (Ranking) |

It draws two new shape kinds the renderer gains: a filled rounded box clipped to an x range
(kind 3) and a filled arc band (kind 4).

### 11. What does not change

- Golden logs: every existing line stays identical. Stream A adds five lines.
- Skins: no scraping, no claims, no new kinds.
- A Paint canvas with no claim brush and no scrape brush draws and simulates exactly as before.
- Numbers, teams and the end never enter the fingerprint or the log hash.

### 12. Known gaps

- On Windows, trig in the chip shapes can differ in the last bit from Linux, so a chip may cover
  a different border cell and numbers may differ by a hair (the same gap as goldens).
- Under-text does not turn with a turned canvas.
- Fade on a Paint canvas is not seen by `covered` (it counts painted cells).
