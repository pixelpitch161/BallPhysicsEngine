# Light mode: a scene type built only for light (design)

Owner: Aedan. Brainstorm thread 2026-11-26 ("Shape a Light Table mode"). Idea pages, each with live sketches:
all light ideas https://claude.ai/artifact/KKxkeDk62xp9VHPKeHJerz, the Light Table
https://claude.ai/artifact/SeWYB621hhg3U9PCMjJF1e, the Lighthouse and Slow Light
https://claude.ai/artifact/XZC7g6MDvcz2gkJypcUDZb. Their sources are in the project files under
`ballphysics/new-modes/`. The sketches are small JavaScript sims; they show the behaviour, not the engine.
The editor mockups (2026-11-26/27, with Aedan's picks) are https://claude.ai/artifact/T2QcxmJC6uwnLupfXjKNE9,
source in the project files under `ballphysics/light-mode/`.

**Changed 2026-11-27:** slow motion is its own mode, **Slow light**, beside Light, at Aedan's request. Sections,
ramps and the two clocks are gone; section 7 describes the new shape.

Every number marked *target* or *estimate* is unmeasured. Lines marked **(My call)** are choices Aedan was not
asked about; they are cheap to change before the part that builds them.

## Intent

Aedan, 2026-11-26, in order:

- "Brainstorm what a 'light' or 'laser' simulation type could be."
- "I specifically like the light table. Expand upon that as the main premise."
- "I would make a video most similar to the 'rainbow lighthouse' example. I'm also thinking about a potential
  'slow mo' idea where the video is slowed down to show the light actually traveling. I imagine this could lead
  to some extremely high fidelity visuals."
- "My goal is to have pieces you can place like you had in the light table mode. Any of them can move or rotate
  during the video. Tools to make rigs (object X orbits object Y). Real time and both ways of slow motion you showed
  here. The most important thing to get right is the quality of the light simulation itself, and the different
  things it refracts/reflects off of through."
- (2026-11-27, after the editor mockups) "I'd rather have a separate mode for the slow motion stuff, because I
  can't think of a situation where I'd want both real time and slow motion lights in the same video. That way the
  slow motion mode could have slightly different settings for the lights and be more intuitive rather than having
  it be in the timeline."

Standing rules that apply (project memory): it must feel like a real simulation, not a scripted video; simulation
quality comes before music timing; as much customisation as possible, psychedelic and experimental looks welcome.

Success: Aedan picks **Light** for a new scene, drops a white beam and a ring of six prisms on the table, makes the
ring spin with one rig tool, and sees the rainbows sweep round live. Aedan starts a **Slow light** scene with the
same pieces, sets the beam to one flash, and watches it cross the wheel, slow down in the glass and split into
colours; then gives the wheel a spin and watches the crawling beams curl. Aedan exports a 1080 x 1920 mp4 that is exactly what the preview showed, with no grain.

## Goals

- **Scene types.** A new scene is Balls, Water, **Light** or **Slow light**. Light and Slow light share their
  things, engine and editor stage; Slow light runs the whole video slowed down (section 7). Balls and Water scenes
  do not change.
- **A light table.** A flat table seen from above, in a dark room with a little haze, so light shows everywhere it
  travels. Pieces are placed, dragged and turned on it like the Light Table sketch.
- **Light quality first.** Light is simulated, never painted on: every beam, rainbow, focus point and shadow is
  light that got there by the rules of optics, in real colours, clean enough at Final quality that no grain shows.
- **A wide set of materials** light passes through, bends in, splits in, bounces off, is coloured by and lands on
  (section 3), each with its real behaviour.
- **Anything moves.** Any piece, light included, can move, turn and scale during the video.
- **Rig tools**: spin, orbit ("object X orbits object Y"), swing, slide along a path, attach, gear links, aim at,
  copy around a centre, and chains of these.
- **Two modes for time**: **Light** runs in real time. **Slow light** slows the whole video until light visibly
  travels; pieces that stand still give true slow motion (light flying through a frozen table), and pieces given a
  move meet the crawling light where they are, so beams curl (the "what if").
- **Camera and looks**: pan and zoom, exposure, long exposure, film glow, haze, and looks from lab bench to full
  psychedelic. Shared with other scenes: keyframes and the timeline, undo, copy and paste of values, Effects, Grade
  and Grade mode, export.

## Non-goals (for now)

- **Real 3D.** The table is flat and light travels in its plane (approach 1 below). A 3D look painted on the pieces
  may come later on top.
- **Pieces that fall and collide** (marbles, beads). Pieces move by rigs and keyframes only. Physics-driven pieces
  are a later idea and the engine must not rule them out.
- **Sound.** Like Water mode: a Light scene has no sound cards and exports an mp4 with no sound of its own
  **(My call)**.
- Text, Balls things and Water things in a Light scene.
- Light as waves (ripples and interference stripes, page 1's Light Waves). Later, as a separate way to run.
- Polarising filters, crystals that split a beam in two, and stress rainbows. Later; the engine carries what they
  need from part 1 (section 2).
- Glow paint that stores light and fades. Later; it needs light from earlier frames.
- Water from Water mode as glass. Later, once both modes exist.
- Light scenes in Video mode. Later, with the same clip source Water mode needs.

## Anti-goals

- A Balls or Water scene must be byte-for-byte what it is today: same goldens, same pinned pictures.
- **Nothing faked in the light.** No painted rainbows, flares or glows pretending to be light paths. Effects and
  Grade may sit on top of the finished picture, as in every scene, and are labelled as effects.
- An export must never differ from the preview at the same quality and time.
- Smooth glass must never show facets or steps in its light: curves bend light as curves.
- The editor must not stall: the light is worked out off the frame, and dragging never waits on it.
- Speed numbers given to Aedan as facts are measured on the RX 9070 XT. An estimate is always called one.

## Constraints from the project

- Editor colours come only from `style/` tokens. Controls are named for kittest. No `ComboBox`.
- Windows-friendly code, cross-built with MinGW. Aedan never runs commands.
- wgpu without a window goes through `bpe_gfx::headless_device`; picture tests use `bpe_gfx::test_device()`.
- Tests that draw run on Aedan's card in CI; pinned pictures refresh only through "Refresh GPU pictures".
- `bpe-core`, `bpe-runtime` and `bpe-kit` keep refusing platform maths. The light engine is a new crate with no
  such rule: it never feeds the balls' physics. Rig motion is worked out in `bpe-kit` or the new crate from
  `bpe_core::maths`, so a rig's positions are the same on every machine **(My call)**.

## Approaches considered

1. **A flat table, light travelling across it, on the graphics card** (**my pick**, told to Aedan 2026-11-26
   with the invitation to choose 3D instead). Light is traced in the table's plane. This is what the sketches
   show and what Aedan asked for, it lets every path be exact and clean, and real time and both slow motions come
   from one engine.
2. **The flat table with a 3D look painted on the pieces.** Not a starting point; a later layer on top of 1.
3. **A full 3D scene with a camera.** Most photographic, but 3D light takes many times longer to come out clean,
   Slow light becomes very costly, and the editor would have to be 3D.

Inside approach 1 there are two ways to turn traced light into a picture, and part 1 builds both and keeps the
better (section 2, "Drawing the light").

## The design

### 1. Scene type and the Light scene's things

- `SimKind` gains `Light` and `SlowLight` (after `Water`; the field already defaults to Balls and is left out of a
  Balls file). Both allow the same Light thing kinds; they differ in the root's cards, each light's timing card and
  the timeline (sections 4, 7, 8) **(My call: two kinds rather than one kind with a switch, so the chooser, the
  toolbar chip and the scene-kind rule treat Slow light as its own mode, as Aedan asked)**.
  `Scene::FORMAT` moves on only if an older program would otherwise misread a Light scene **(My call: move it to
  14 so 13-only programs refuse it cleanly)**.
- **The chooser** gains two tiles, Light and Slow light, which share the starting looks (section 6). File > New
  is the same chooser.
- The root is the same Scene settings thing with the same `SceneSettings` (Width, Height, fps, Length), shown as
  Water mode shows them. The Camera thing is shared, with its ball-only settings hidden.
- A Light scene is a tree of things that own their settings, so the inspector, right-click Copy Value / Paste
  Value / Create Keyframe, undo and save all work unchanged, and the scene-kind rule in `bpe-core` (`in_sim`,
  `WrongScene`) keeps the kinds apart.
- Things in a Light scene:

| Thing | What it is | Cards |
| --- | --- | --- |
| Scene settings (root) | The table, the room and the picture | its own settings as today, **Table** (size in metres, walls, floor), **Air** (haze, haze patches), **Engine** (quality, bounces, colour split), in Slow light also **Light speed** (section 7), Effects, Grade |
| Camera | Shared | Camera, **Exposure** (exposure, long exposure, film glow, tone) |
| Light | A source: white beam, laser, lamp, spotlight, strip, sun, panel | **Source** (type and shape), **Spectrum**, **Switching** in Light or **Fires** in Slow light (section 4), Moves |
| Piece | Anything light meets: glass, mirror, filter, grating, stop, screen | **Shape**, **Material**, Moves |
| Rig | An empty centre that pieces can be attached to and that can itself move: the hub of a wheel, a pendulum's pivot | Moves |
| Volume | A patch of something light passes through: smoke, milky water, fog | **Shape**, **Medium**, Moves |
| Folder | Grouping | none |

- Kind ids start with `light_` (`light_source`, `light_piece`, `light_rig`, `light_volume`), as
  Water's start with `water_`, so they never clash with the Balls mode's own Light and Rig kinds.
- New `SlotId`s for the new cards; `Form`, `Material`, `Moves` and `Main` already exist from Water mode and are
  reused where the meaning matches. Light's cards must use section names no ball setting groups under (the Water
  lesson: `a_water_section_is_no_ball_settings_group`).

### 2. The light engine

A new crate, **`bpe-light`** (after `bpe-gfx`, before `bpe-kit`), on the graphics card in WGSL compute. It takes a
frozen description of the table at a moment (or across a shutter) and returns a picture of the light.

**What is simulated.** Light is a great many paths. Each path starts at a light, carries one wavelength, a
strength, its share in each polarisation (in the table's plane and across it) and the time it has been travelling.
It goes in straight lines through air and uniform materials and is changed at every surface it meets by that
surface's material (section 3), until it is used up, leaves the room, or reaches the bounce limit. Weak paths are
ended at random with their strength raised to match (so the picture stays right on average), never simply cut.

**Real colour.**

- Wavelengths from 380 to 780 nm. Each light's paths take wavelengths in proportion to its real spectrum
  (section 4), spread evenly over the spectrum rather than picked at random, so white stays white and only real
  splitting shows colour (the Light Table sketch's lesson).
- Each material's index is a curve over wavelength from published data (Sellmeier formulas for named glasses,
  water, diamond and the rest). The Colour split setting scales how far the curve departs from its middle, from 0
  (no splitting) through 1 (the real material) to larger for show.
- Light is added up per pixel as XYZ (the CIE 1931 2° curves), not as RGB, and turned into the screen's colours
  only at the end. Laser lines and pure rainbow colours lie outside what a screen can show, so the turn into RGB
  maps them inside while keeping their hue, and very bright light goes towards white the way it does on a real
  camera, instead of clipping into flat blobs.

**What the camera sees.** The camera looks straight down at the table. Light shows where:

- **Haze** in the air sends a little of every path towards the camera, in proportion to the haze's thickness and
  the length of path through it. This is what makes beams visible. Haze can thin a beam over a long way when it
  is thick (on by default at a gentle amount).
- **Surfaces that scatter** (white walls, screens, paper, frosted glass, smoke) glow where light lands on them, in
  proportion to the light they scatter.
- **The pieces themselves** are drawn on top as a separate layer from their Look settings (outline, glass tint,
  edge sheen, or nothing), so a piece can be invisible and show only through what it does to the light.

**Drawing the light.** Part 1 builds both of these and keeps the one that is cleaner at full size for the same
time on the RX 9070 XT, or a mix of both:

- **Lines.** Every path segment is added to the picture as a line, spread over the pixels it crosses by exactly
  how much of each pixel it covers (the sketches' fix for seams). Simple and exact; clean only with very many
  paths.
- **Fans.** Paths are sent in fans of neighbours. Where two neighbours meet the same surfaces in the same order,
  the strip between them is filled, brighter where the strip narrows and dimmer where it widens, so beams and
  focus lines come out smooth from far fewer paths. Where neighbours part (one hits an edge the other misses), the
  fan splits and falls back to lines. Balls mode's bent light already spreads by neighbours (`bent_light.rs`).

**Sums.** Light is added up on the card in whole numbers, so a frame is the same every time it is drawn on the
same card, driver and program version, and the order paths finish in never matters (Water mode's lesson). Each
frame's paths are chosen from the frame number and the scene, never from the clock, so a frame is drawn the same
in the editor and in export.

**Quality and time.**

- **Quality**: Draft, Good, Best and Final, each a number of paths a frame (the numbers come from part 1's
  measurements). The editor plays at Draft and drops lower while something is dragged.
- **Clean when still.** When nothing on the table moves, the editor keeps adding paths to the same picture until
  it is spotless, and says so.
- **Moving pieces and the shutter.** Each path picks its own moment inside the frame's shutter, and the table is
  posed at that moment for that path, so a turning wheel blurs exactly as a camera would see it. Shutter length is
  a Camera setting (0 = frozen frames).
- **Long exposure** keeps the shutter open over many frames, adding their light, for light-painting patterns.
- A frame never needs the frame before it (except slowed light, section 7, and long exposure, which adds earlier
  frames' light), so scrubbing is instant and export can draw frames in any order.

**Size targets** (*targets*, measured in part 1 on the RX 9070 XT): Draft plays at 30 frames a second or better
with the Lighthouse scene at the stage's size; Final draws a 1080 x 1920 frame of that scene, clean, in under 5
seconds.

**Geometry.** Pieces are made of exact pieces of outline: straight lines, circular arcs and pen curves. Pen curves
are split finely into arcs, so their normals turn smoothly and no facets show in the light. Lenses are two exact
arcs. Each frame the outlines are posed (section 5) and gathered into a search structure on the card so a path
finds the next surface quickly with hundreds of pieces on the table.

**Bounces.** Up to 64 surface events a path by default, settable to 512 **(My call)**.

### 3. Materials

Each material is a set of real behaviours with settings in the owner's terms. A Piece has one Material; a Volume
has one Medium. Every behaviour below works with every light and every kind of motion.

**Glass and clear things** (light bends going in and out, by the exact Fresnel equations for both polarisations,
with total internal reflection and the Brewster angle coming out on their own):

- Named kinds with real index curves: **Crown glass**, **Flint glass** (strong splitting), **Fused quartz**,
  **Acrylic**, **Water**, **Ice**, **Sapphire**, **Diamond**, **Cubic zirconia** (stronger splitting than diamond),
  and **Custom** (index and splitting set by hand, up to well past diamond for show).
- **Coloured glass**: soaks up light by wavelength as it travels through (deeper colour where it is thicker), from
  real filter curves (red, amber, yellow, green, cyan, cobalt blue, violet, magenta) or a drawn curve.
- **Frosted glass**: rough surfaces that spread the light that passes, from lightly hazy to fully milky, on one
  side or both.
- **Opal / milky glass**: glass that scatters inside, blue scattered more than red, so it looks blue from the side
  and passes orange light.
- Light pipes and fibres need no material of their own: a bent glass rod carries light by total internal
  reflection.

**Mirrors and metals**:

- **Silver**, **Aluminium**, **Gold**, **Copper**, **Chrome**, each reflecting with its real colour curve (gold
  reflects warm, copper warmer), smooth or **brushed** (rough, spreads the reflection).
- **Half mirror** (beam splitter): reflects a set share and passes the rest.
- **Corner reflector**: sends light back the way it came.
- The room's **walls** can be open (light leaves), black, white (scatter) or mirror (section 6).

**Things that sort colours**:

- **Dichroic glass**: reflects one band of colours and passes the rest, with the band moving towards blue as the
  light comes in more slantwise, as real coated glass does (worked out from a thin stack of layers).
- **Thin film** (soap film, oil sheen): the colours of interference from a film of a set thickness, which can vary
  along the piece, so colour shifts with angle and thickness.
- **Diffraction grating** (and a CD-like surface): fans light into rainbows by the grating equation, in orders on
  both sides, reflecting or passing, with lines per millimetre as the setting.
- **Colour filters** are coloured glass in a thin slab; a **stop** passes nothing.

**Things light lands on**:

- **Screen / white card / paper / white wall**: scatters what lands on it, so it shows where light falls, with its
  own colour.
- **Black velvet**: soaks up everything (beam dumps, blades, gobo cut-outs).
- **Fluorescent** (highlighter ink, uranium glass): soaks up violet and ultraviolet and gives it back as a longer
  colour, so it glows under an ultraviolet light or a blue laser. Ultraviolet lights show nothing in the air on
  their own, only on fluorescent things.

**Volumes** (Mediums): **Smoke** and **Fog** patches (thicker haze, so beams show brighter there), **Milky water**
(scatters blue, passes orange: a sunset in a tank), **Clear water** with soft blue-green soaking.

Materials as built in part 3: the four glass rows, mirrors and metals, half mirror, screens and black velvet,
coloured glass, frosted glass, haze and smoke. The rest in part 8.

### 4. Lights

- **Types**: **White beam** (parallel, a set width and edge softness), **Laser** (a thin beam of one or more real
  lines), **Lamp** (a point or small disc sending everywhere), **Spotlight** (a cone), **Strip** (a line of light,
  like a tube), **Sun** (parallel light over the whole table from a direction), **Panel** (a line sending one way).
- **Spectrum** presets with real shapes: Daylight (D65), Sunlight, Candle (1900 K), Warm bulb (2800 K), Halogen
  (3200 K), white LED (blue plus phosphor), Fluorescent tube (mercury lines and phosphors), Sodium street lamp,
  Neon, Mercury lamp, Red, green and blue lasers (638, 532, 450 nm), any laser line, Ultraviolet (365 nm),
  Blackbody at any temperature, and a drawn custom spectrum.
- **Brightness** in one number per light, with the haze and exposure doing the rest, so lights mix as real light
  does.
- **Switching** (Light mode): Steady, Blink (on time, off time), Off then on. Brightness is also keyframed.
- **Fires** (Slow light mode, in place of Switching) **(My call, shown on the mockup page)**: **Steady** (already
  shining everywhere when the video starts), **One flash** (at, pulse length), **Flashes** (first flash, pulse
  length, every), **Switch on** (at), **Switch off** (at). The pulse length is set in video seconds and also shown
  as how long the streak of light is on the table and how long the flash would be in real life ("a streak 7.5 cm
  long: a 0.25 ns flash"). Each light's lane on the timeline draws what it sends over time, and its flashes drag
  there too.

### 5. Placing, motion and rigs

- **Every Piece, Light, Volume and Rig has a pose**: position, turn and scale. Each pose setting can be
  keyframed.
- **Moves card**: a list of motions, applied in order on top of the pose. Each is a few settings in plain terms,
  and each can be keyframed (so a spin can speed up):

| Motion | Settings |
| --- | --- |
| **Spin** | Turns a minute, direction, speeding up and slowing down |
| **Orbit** | Around (a thing or a point), radius, turns a minute, start angle, oval shape, keep facing the centre or not |
| **Swing** | A real pendulum: pivot, length, starting angle, how fast it dies down |
| **Slide along a path** | A pen-drawn path, speed, there and back or loop |
| **Wobble** | Smooth wandering in position and turn: how far, how fast |
| **Attach to** | Rides on another thing, taking every motion it has (chains: X orbits Y, which orbits Z) |
| **Gear link** | Turns at a set ratio of another thing's turning (3 to 2 makes long exposures close up) |
| **Aim at** | Keeps pointing at another thing (a laser that tracks a moving mirror) |

- **Rig tools that make things** **(My call)**, so every thing keeps owning its own settings (the project's rule:
  nothing shared, linked or inherited):
  - **Copy around**: makes a Rig at a centre and N separate copies of the chosen piece spaced round it, each
    attached to the Rig. A prism wheel in one step: spin the Rig and the wheel turns. **Re-space** on the Rig lays
    its pieces out evenly again after some are added or removed.
  - **Mirror copy**: makes a separate mirrored copy across a line.
  - To change all six prisms at once, select them and edit: in a Light scene the inspector applies a change to
    every selected thing that has that setting (each still its own value afterwards).

- **Motion is worked out from time alone.** Every rig gives the pose of every thing for any moment directly, with
  no history (a pendulum dying down has a formula, or is worked out once on the processor and kept as a table).
  That is what lets the light engine pose the table at any moment inside a frame, lets Slow light meet each
  piece where it is when the light arrives, and lets export draw frames in any order. Attach chains are worked out from the
  root down; a loop (X attached to Y attached to X) is refused with a sentence.
- **Nothing is scripted.** The light is never told where to go; rigs only move pieces, and the light does the
  rest. Switching is a setting of each light.

### 6. The table, the room and the camera

- **Table**: its size in metres (default 1 m tall, the width from the scene's shape), used for Slow light's
  timings. **Walls**: Open, Black, White or Mirror, each side on its own. **Floor**: dark, a colour, or a picture.
  Tables bigger than one screen with a travelling camera come later; the engine works in table units from the
  start so that is only an editor part **(My call)**.
- **Air**: haze thickness (how strongly beams show), haze patches, haze that thins long beams.
- **Camera**: position, zoom and turn, keyframed; follow a thing.
- **Exposure card**: Exposure, Long exposure (how many seconds of light add up), Shutter, Film glow (amount, size,
  colour), Tone (filmic curve and how highlights go to white), and Colour maps for psychedelic looks laid over the
  real light (hue shift, false colour by wavelength, by brightness).
- **Looks** are a table in the kit like Water's `looks.rs`: plain setting values for the Table, Air, Exposure,
  Effects and Grade, and a few pieces' Looks. Picking one writes the values; nothing stays linked. Starting looks
  **(My call)**: Lab bench (dark grey table, light haze, crisp), Night haze (thick haze, film glow), Neon club
  (black, strong glow, saturated), Blueprint (blue table, white outlines), Stained glass (warm, soft), Full
  psychedelic (colour maps on), and Plain.

### 7. Time: Light is real time, Slow light is its own mode

Aedan asked (2026-11-27) for slow motion to be its own mode rather than sections on the timeline: a video is either
real time or slow, never both, and a slow mode can give lights settings that suit it. So there are no sections,
ramps or second clock. (If a video ever needs both, Video mode can put a Light scene and a Slow light scene back to
back with a transition, `docs/notes/video.md`.)

**Light** (real time): light is everywhere at once each frame (it crosses a 1 m table in about 3.3 ns). Pieces,
rigs and keyframes all run on the one timeline.

**Slow light**: the whole video is slow.

- **Light speed** (a card on Scene settings): **Crosses the table in**, from 0.5 to 30 seconds (default 4 s),
  shown beside "about 1,200,000,000 times slower than real light". One setting for the whole video **(My call)**.
- **How light looks** (same card): **Afterglow** (how long a place keeps glowing after light has passed, 0 to 2
  s) and **Front glow** (how bright the leading edge of a flash or switch-on is). Both are drawn from the light's
  own timing, not painted.
- **Pieces stand still unless given a move** **(My call: one mode holds both kinds Aedan asked for)**:
  - A table where nothing moves is **true slow motion**: light at its real relative speed in each material (three
    quarters in water, two thirds in crown glass, about two fifths in diamond, blue slower than red, from the group
    index), flying through a frozen table. All paths are traced once with the time light reaches every point and
    drawn exactly for any moment, so scrubbing is instant and the path count can be far higher than a real-time
    frame's. Echoes are exact shares, so faint ones come out clean. This is where the highest fidelity comes from
    (*target*: the Final frame of a flash in the Lighthouse is clean at full size; numbers from the part that
    builds it).
  - Giving any piece a move (spin, orbit, keyframes) makes it meet the crawling light where it is when the light
    arrives, so a turning wheel curls beams into spirals and a spinning mirror throws galaxy arms (the "what if",
    once called slowed light). Here light needs its history: it is stepped forward on the card, with checkpoints
    every second; scrubbing goes back to the nearest checkpoint and steps forward. Each bit of light lives at its
    own moment inside its frame, so beams stay continuous (the sketch's lesson). The mockup steps about 4,000 bits
    of light and rebuilds a moment in about 0.1 s on a laptop processor *(estimate for the real engine: faster, on
    the card)*.
  - **Doppler colours** (off by default): light reflected off a mirror moving at a fair share of the light speed
    shifts colour, as real light would.
- **Each light's Fires** (section 4) says when it sends light. A light left Steady fills the table from the start.
- **Camera delay** (off by default): light from far parts of the table reaches a camera above it later, which bends
  the fronts as real high-speed footage of light shows; the camera's height sets how much.
- Export turns each frame number into a video time; a still table needs nothing else, a moving one steps from the
  nearest checkpoint, so any frame can still be drawn on its own.

### 8. The editor

Aedan's picks on the mockup page (2026-11-27): **layout A**, the **piece pictures**, the **material list** (not
tiles), all three **rig tools** (drag to link, Copy around and Mirror copy, handles with "where things go"), the
**light probe**, the **status readout** and the **keys**. The New scene window was a maybe; layouts B and C were
not picked.

- **Layout A, like Water mode** (Aedan picked it there too): the tree on the left, the tool rail down the stage's
  left edge, the tool's options in a strip above the stage, the inspector on the right, the timeline below. The
  Light stage is a sibling of the Water stage (`water_mode/stage.rs`) and reuses its view and rail code where it
  fits: the table fitted to the room, the camera's 9:16 shot as a frame, the live light under the pieces' outlines.
  Light and Slow light use the same stage.
- **Rail**: Select, Light, Glass, Mirror, Filter, Screen, Volume, Pen (draw any glass or mirror shape), Rig,
  Camera. Picking Light, Glass, Mirror, Filter or Screen fills the options strip with that group's pieces, each
  with a **picture made by the real engine** of what it does to light (a prism splitting a beam, a lens focusing,
  a grating fanning rainbows); later pieces show greyed.
- **Palette** of ready pieces: Prisms (equilateral, right angle, any angle), Lenses (convex, concave, flat on one
  side, meniscus, a Fresnel lens, a rod), Ball, Half ball, Block, Rod and bent rod, Gem (a cut diamond outline),
  Glass ring, Flat, curved, parabolic and oval mirrors, Mirror drum (any number of sides), Corner reflector, Half
  mirror, Grating, CD, Soap film, Slit, Blades, Screen, Black stop, Smoke patch, Tank of milky water.
- **Material list**: the Material card opens a list grouped by family (Liquids and ice, Glass and crystal,
  Coloured and frosted, Mirrors and metal, Coatings and gratings, Where light lands), each row with an engine-made
  picture and plain facts ("Bends 1.517 · Splits colour a little · Light slows to 65%"). Several pieces on one rig
  can be changed together with a tick box.
- **Rig tool** with options Orbit, Ride on, Aim at, Spin, then Copy around (with a count) and Mirror copy: drag
  from the thing that moves to what it goes round, rides on or points at (or to an empty spot for a point); a
  dashed line and a label ("go round Mirror") show what the drag will do. The inspector's **Moves** section lists
  each move with Stop / Let go and has + Spin, + Orbit, + Ride on, + Aim at (more later).
- **Handles**: drag to move, a round handle to turn, corners to scale; a picked moving thing shows its own handles
  (a spin's arrow you drag to change turns a minute, an orbit's ring and a square to change its size). **Show where
  things go** draws faint ghosts of where the picked thing will be over the next seconds.
- **Keyboard**: arrows move (Shift further), `[` `]` turn, N picks the next, Delete removes, Space plays, Esc goes
  back to Select.
- **Status**: Draft while moving (it plays live), then "Gathering light" and "Clean" as the still picture builds.
  In Slow light it says whether the table stands still or pieces move, and how many times slower than real light.
- **Light probe** **(My call)**: hover a beam to see its colours as a small spectrum and its strength.
- **Timeline**: in Light, keyframes as today. In Slow light: the seconds ruler with **light time in nanoseconds**
  under it, each light's lane drawing what it sends over time (flashes as pulses, drag them), a **Light reaches**
  lane marking when the first flash reaches each piece or rig, and a lane for each moving rig.
- Toolbar: a **Light scene** or **Slow light scene** chip, **Quality**, **Export**. Build | Grade modes; Grade works
  on the light picture with the masks the Light renderer hands the shared finish (section 9).

### 9. Where the code goes

- `bpe-core`: `SimKind::Light` and `SimKind::SlowLight`, the new `SlotId`s, the Light things' data (outlines,
  motion lists, Fires).
- **`bpe-light`** (new crate): the engine (`trace/`, WGSL compute), materials and spectra (`optics/`: index curves,
  metal colour curves, filter curves, CIE curves, light spectra), drawing (`draw/`: lines, fans, tone and colour
  mapping), time (`time/`: arrival times, still-table flight frames, moving-table stepping and checkpoints). It hands the
  shared finish a colour picture, a light picture and silhouettes for each piece, as Water mode's renderer does
  (`2026-11-24-water-mode-design.md`, section 6, step 7).
- `bpe-kit`: the Light thing kinds, cards and settings (`things/light_*`, `modules/light_*`), rig motion (worked
  out with `bpe_core::maths`), the looks table, the palette's ready pieces.
- `bpe-studio`: `light_mode/` (stage, rail, options strip, handles, palette, material list, probe, Slow light
  timeline lanes).
- `bpe-cli`: a Light clip source in `FrameMaker`, so a Light scene exports. Water mode needs the same clip source
  (its part 8); whichever mode gets there first builds it for both **(My call)**.

### 10. Checks

- **Optics tests** check the numbers against the textbook: Snell's law and total internal reflection angles,
  Fresnel shares at several angles for both polarisations, the Brewster angle, a prism's deviation against the
  formula at several wavelengths, a lens's focus against the lens formula, the index curves against their data,
  the time through a slab against its group index, the grating's angles, energy kept (light in = out + soaked
  up), and metal colours.
- **The same frame twice** on the same device is identical.
- **Pictures**: picture tests at 270 x 480 through `test_device()`; full-size look scenes in `full_size_look.rs`
  (the Lighthouse, a prism, a lens focus, a mirror room, a flash in flight); pinned pictures on Aedan's card.
- **Edits**: every setting of every Light thing kind changes the picture when it should (the Light twin of
  `restarts.rs`).
- Speed is timed on the RX 9070 XT by the Remote Control recipe and written into the notes.
- Balls and Water goldens and pinned pictures must not change in any part.

## Parts

Each part is its own thread and pull request, ends with something Aedan can open or watch, and gets one Opus
review.

1. **The light engine.** `bpe-light` on its own: paths with real colour, exact Fresnel, index curves, total
   internal reflection, crown glass, a mirror, black and white walls, haze, white beams, lasers and lamps; both
   ways of drawing (lines and fans) compared at full size for cleanness and speed on the RX 9070 XT, and the
   better kept; whole-number sums; quality levels set from measurements; test scenes (prism, lens focus, glass
   ball, a six-prism lighthouse posed at several moments, mirror room). Ends with full-size pictures and a short
   film for Aedan from a test harness, and a verdict on the drawing method.
2. **Light scenes open.** `SimKind::Light`, the Light tile, the Light things and cards as data, the Light stage in
   layout A with the rail and options strip, the engine's light live under the pieces, Draft and Clean when still.
3. **Pieces and materials, first set.** The palette with engine-made pictures, handles and pen shapes; the
   materials listed at the end of section 3 and the material list; real light spectra; the light probe.
4. **Motion and rigs.** Keyframed poses, every motion in the table of section 5, attach chains, Copy around,
   Mirror copy and Re-space, editing several selected pieces at once, motion trails, the shutter and motion blur.
5. **Camera, looks and export.** Camera moves, the Exposure card, film glow, colour mapping and tone, the looks,
   the shared finish with Effects and Grade, the Light clip source and mp4 export. **Aedan can export the Rainbow
   Lighthouse video here.**
6. **Slow light, still table.** `SimKind::SlowLight` and its tile, the Light speed card, each light's Fires,
   arrival times and group speeds, exact echoes, still-table tracing at high path counts, the Slow light timeline
   lanes (light time, Fires lanes, Light reaches), camera delay. **Aedan can export a flash crossing the
   Lighthouse here.**
7. **Slow light, moving pieces.** Stepping with checkpoints, light meeting moving pieces, Doppler colours.
8. **Materials, second set.** Dichroic glass, thin films, gratings and CD, fluorescence and ultraviolet lights,
   corner reflectors, opal, milky water, the remaining glass kinds and metals.

Order: 1, 2, 3, 4, 5 in a row (5 gives the first video). Then 6, 7 and 8 side by side.

**How it sits with Water mode** **(My call)**: part 1 touches only the new crate, so it can start whenever Aedan
says without disturbing Water mode's checkpoint after C2. Part 2 changes the chooser and `SimKind`, which Water's
part 2b also touches, so the two are not run at the same time.

## Risks

- **Clean enough at full size.** Grain-free light at 1080 x 1920 is the whole point. Fans are the main lever,
  more paths the second; part 1 measures both before anything else is built on them.
- **Whole-number sums can overflow** in a focus point thousands of times brighter than the haze. Part 1 picks the
  sizes and scale so a frame cannot overflow, and tests a laser focused to a point.
- **Screen colours for laser light.** Pure colours lie outside what a screen shows; mapping them badly makes
  lasers look flat or wrong. Part 1 shows Aedan red, green and blue lasers and a rainbow at full size.
- **Slow light with moving pieces is the one part with history**, so scrubbing it is slower than the rest.
  Checkpoints are the cure; if stepping is too slow at Final it draws at Good while editing.
- **Many pieces.** Hundreds of pieces with long bounce chains may be slow. The search structure and the bounce
  limit are the levers, measured in part 1 with a mirror room.

## Answered by Aedan (2026-11-26)

- The light table is the main premise of the mode.
- First video: most like the Rainbow Lighthouse; slow motion that shows light travelling.
- Goal: placeable pieces, any of which move or turn; rig tools ("object X orbits object Y"); real time and both
  slow motions; the quality of the light simulation and the range of things it meets matter most.
- (2026-11-27) Slow motion is its own mode, not timeline sections. Editor picks: layout A, piece pictures, the
  material list, all three rig tools, the probe, the status readout and the keys (section 8).

Told to Aedan as my calls, open to change: a flat table rather than 3D, pieces move by rigs only (no falling and
colliding yet), no sound at first, Light and Slow light as choices beside Balls and Water. Settled after the builder
check, also open to change: Copy around makes separate pieces on a Rig, edited
together by selecting them, rather than live copies.
