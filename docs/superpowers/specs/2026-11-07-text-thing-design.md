# The Text thing: design

Approved by the owner on 2026-11-07 (thread "Design the new Text thing"). Built in five parts, each its own
thread, plan and PR: `docs/superpowers/plans/text/part1.md` … `part5.md`.

## What the owner asked for and chose

- "A Text thing." Asked what text is for, the owner chose **all three** in one thing: words on the screen (hook
  titles, counters, captions, lyrics), letters as real objects balls hit, and words made of the engine's own stuff
  (balls, particles, light, glass).
- **All 24 looks** of the mockup page "Text Looks" (https://claude.ai/artifact/3WPFpxmH9tPFchNyEvHbTC, copy in the
  project files `ballphysics/text-thing/text-looks-mockup.html`) are in. Numbers below (#1 … #24) are that page's.
- **Engine approach C**: every letter is a real outline drawn and (later) bounced like a Shape, plus a fast lane for
  crowds of small labels.
- A **larger built-in font set**: all **52 fonts** of the "Font Shelf" page
  (https://claude.ai/artifact/S9GRxjQ9FYqFysaFRsrohS, copy `ballphysics/text-thing/font-shelf.html`), plus Load font.
- The four design sections (the thing and its settings; looks; text in the world; fast lane, editing and parts) were
  each approved as written in the thread. This spec is those sections made exact.

Breadth first and everything mixes with everything (the owner's standing rule): text uses the Shape's Look card,
lighting, surfaces, rules, sound and health rather than growing look systems of its own.

## The thing

A new thing kind **`text`** (`bpe-kit/src/things/text/`), name "Text", family **"Text"** (a new Add thing heading
between Shapes and Lights). `ThingKind::sticks_to_screen` is **true**: a new Text sits on the screen; the existing
"stuck to the screen" switch (`Thing.screen`) puts it in the world. Things stuck to the screen are drawn after the
Stage camera move and unlit (as today), so the lit looks (#13 Neon, #14 Glass, #18 Shadow) are set in the world.

Its cards: **Look** (part 1), then **Bounce**, **Sound** and **Health** (part 3). It has the Placement card
(Position, Rotation, Z order).

### Its own settings (`TextSettings`)

Shown in the inspector's own card, titled **Words**, in these groups.

**Words** (part 1)
- **Words**: the text, any number of lines (`Value::Text`, unit `lines`: a multi-line box). Default `TEXT`.
- **Font**: a font name (`Value::Text`, unit `font`: the font picker). Default `Anton`.
- **Weight** 100 … 900 (fonts with a weight axis or several weight files use it; others ignore it), **Slant** −30° …
  30° (a lean every font can take, made by shearing), **Size** (the height of a capital letter, px), **Letter
  spacing** (px), **Line spacing** (× the font's own line height), **Align** Left / Centre / Right, **Wrap width**
  (px, 0 = no wrapping), **Case** As typed / CAPITALS / lower case / Title Case. Size, spacings, wrap, weight and
  slant keyframe.
- **Spin** (deg/s), as a Shape has.

**Path** (part 1)
- **Path**: Straight / Round a circle / Along a wave / Along a shape's edge.
- Round a circle: **Radius**, **Starts at** (the angle the middle of the text sits at, 0 = top, clockwise),
  **Reads from inside**. Along a wave: **Height**, **Wavelength**, **Wave shift** (keyframe it, or **Wave speed** in
  cycles/s), **Letters follow the slope**. Along a shape's edge: **Shape** (a `Target` naming a Shape thing),
  **Starts at** (0 … 1 round its edge from its first corner), **Gap** (px off the edge, negative inside). Multi-line
  text on a circle or edge puts later lines a line step further in.

**Show** (part 2)
- **Show**: All at once / Letter by letter / Word by word / Line by line; **Appear** None / Pop / Fade / Slide /
  Drop / Typewriter; **Disappear** the same; **Timed by** Seconds (Starts at, Each takes) / Beats (one per beat from
  Starts at) / Hits (one per hit of a `Target`); **Stays for** (seconds, 0 = stays).

**Balls** (part 3)
- **Balls**: Pass through (default) / Bounce off / Bounce inside, with **Thickness** for Bounce inside.

## Fonts

### The 52 built-in fonts

Shipped as files in a **`fonts` folder beside the exe** (the program file has a 25 MB install limit and the fonts
are about 11 MB). In the repository they live in `assets/fonts/text/` with each family's licence in
`assets/fonts/text/licences/`. 48 are SIL Open Font Licence, four (Chewy, Luckiest Guy, Permanent Marker, Satisfy)
Apache 2.0; all may be shipped. Source: `https://raw.githubusercontent.com/google/fonts/main/<path>`.

| Group | Fonts (variable weight in brackets) |
| --- | --- |
| Big and bold | Anton, Bebas Neue, Oswald [200–700], Archivo Black, Bowlby One, Russo One, Black Ops One, Bungee, Sigmar, Rubik Mono One |
| Round and friendly | Varela Round, Fredoka [300–700], Baloo 2 [400–800], Nunito [200–1000], Lilita One, Titan One, Chewy, Luckiest Guy |
| Clean | IBM Plex Sans [100–700], Inter [100–900], Montserrat [100–900], Poppins (400, 600, 700, 900 as four files), Outfit [100–900], Space Grotesk [300–700] |
| Elegant | Playfair Display [400–900], DM Serif Display, Abril Fatface, Cinzel [400–900], Shrikhand |
| Tech and retro | Space Mono, JetBrains Mono [100–800], Orbitron [400–900], Audiowide, Press Start 2P, VT323, Silkscreen, Righteous |
| Hand drawn | Permanent Marker, Caveat [400–700], Pacifico, Lobster, Satisfy, Bangers |
| Wild | Monoton, Bungee Shade, Bungee Outline, Rubik Glitch, Rubik Wet Paint, Rubik Puddles, Nosifer, Creepster, Fascinate |

The exact source paths and file names are in part 1's plan.

### Finding and loading fonts

- The fonts folder is looked for in order: the `BPE_FONTS` environment variable, `fonts/` beside the running exe,
  then the repository's `assets/fonts/text/` (development and tests).
- **Load font…** reads any `.ttf` or `.otf` and saves its bytes **inside the scene file** (`Scene.fonts`, base64 in
  the RON), so a scene opens the same on any PC. Its name is the file's family name, made unique ("Name (2)").
  Loaded fonts are listed first in the picker and can be removed (a Text using a removed font falls back).
- A font that can't be found (a missing folder, a removed font) draws in **IBM Plex Sans SemiBold**, which is built
  into the program, and the Font row says so in plain words ("Font file not found: put the fonts folder beside
  ball-studio.exe").
- Shaping is **rustybuzz** (kerning and ligatures, the font's weight axis); outlines are **skrifa** (already used).

## Layout

`bpe-kit/src/text/` is the text engine, shared by the draw, the preview's handles and (later) the physics:

- **`layout`**: words, case, wrap, align, spacing, weight and slant → a `TextLayout` of straight lines of
  `Letter`s (every drawn glyph: its character, line, word, index among drawn letters, contours in px with y down
  relative to its own origin on the baseline, its bounds and its origin in the block). Spaces take room but are not
  letters. Size is the capital height (as the Balls "Word" shape's letter height). The block's middle (the middle of
  the widest line across, and halfway between the first line's capital top and the last line's baseline down) sits
  at the thing's place. Curves are flattened to within 0.2 px at the drawn size (at most 2048 points a letter), and each glyph's contours are
  merged under the non-zero rule (`i_overlay`), because variable fonts overlap their contours: what is drawn and
  bounced is a clean outline with real holes and no seams inside.
  Layouts are cached process-wide by everything that makes them.
- **`arrange`**: a layout and the Path settings at a moment → each letter's place and turn in the text's own frame
  (straight: unchanged; circle: round the arc, the text's middle at Starts at; wave: on the wave, turned by its slope;
  shape's edge: along the target's outline at this moment, read from the world's wall). Not cached (paths move).

## Drawing (part 1)

- The Text's Look card is the Shape's **`look.shape`** (its `fits` takes `text` too): Body, Colour and gradient,
  Outline, Glow, Shader, Blend, every shape sub-layer (Gives off light and Heat glow included) and the Surface section.
  The Body's row says the words are set in the Words card. The `shape.particles` around layer is not offered on a
  Text until part 3 gives it a wall.
- Each letter draws as one shape instance (`PathInstance`), all sharing the text's centre, turn and scale (1: shape
  units are pixels) and the text's one set of layer records, so a gradient runs across the whole text. A letter's
  points are its contours, placed by `arrange`; holes work because the outline test is even-odd over every contour of
  the letter. The paths shaders learn **several contours** in one point list: a point whose `z` is 1 is the last of
  its contour and `w` says how many points back its first is (`Frame::push_letter`; existing shapes write 0 and draw
  exactly as before). Each letter's quad covers only its own box: the box (grown by its band, everything its layers
  reach past the outline and 2 px) is written as the instance's 4-corner `clip`, the convex clip shattered pieces
  already use.
- The Blend split, Surface (the shape's surface index and stamps), lighting stamps, hit data (Flash rules), screen
  overlay and z-order all work as they do for a Shape.

## Live words (part 2)

- **Insert value ▸** puts a token in the words: `{hits}`, `{hits of <thing>}`, `{balls left}` (of a thing or all),
  `{time}`, `{countdown <seconds>}`, `{beat}`, `{bar}`, `{counter <name>}` (a rule counter), `{health}` /
  `{health of <thing>}`, `{ball number}` and `{ball name}` (for text riding a ball), each with a number format
  (decimals, leading zeros). The editor shows tokens as coloured chips in the box.
- **Standings** `{standings <by> <rows>}`: one line per ball, ranked by Furthest down / Most hits / Health / Size /
  Speed, each line `<place> <name> <value>` ("1st RED 72m"). Lines move to their new place over Re-sort time.
- Values are read at the moment drawn from the world, the log and the rules state (pure in time, so scrubbing and
  export agree). A layout is cached per distinct resulting string.
- **Show** (above) works per letter, word or line, by seconds, beats (the scene's tempo, `beat_at`) or hits.
- The **Words** setting keyframes with Hold keys (`Flags.hold`).
- Rules: Do block **`do.text`** (Set text: Replace / Add a line / Show the next line / Clear; To words may hold
  tokens) acting on a Text thing at the next step, kept in `RulesState` like counters.
- **Riding**: a Text inside a Balls, Emitter or Shape rides it (Each ball, The middle, Ball number; `RIDES` like
  lights and particles). Riding every ball uses the fast lane.
- **The fast lane** (`Fast letters`, automatic for riding every ball and for Standings): each glyph is drawn once
  into a signed-distance atlas on the CPU (64 px cells, 8 px spread) and the letters are drawn as sprites that read
  it, with the Body colour or gradient, Outline, Glow and Shadow; other layers, lighting and walls are left out.

## Real letters (part 3)

- **Walls**: a Text that bounces is one wall, `Outline::Letters` (every contour of every letter, in the text's frame,
  with a box per letter to skip far letters quickly), two-sided like every wall, so balls that start outside bounce
  off (#8) and balls that start inside a letter stay inside it (#7, #9). Bounce inside draws the contours as bands of
  Thickness. Pass through keeps the wall (`Wall.passes`) so rules' Enters / Leaves see letters as zones.
- **Each letter is a target**: a contact names its letter (`Contact.part`), the Hit event carries it in `b.item`,
  rules' Who can name "letter N", and Break, Health and Flash act on one letter (letters are the thing's rows, as
  balls are a group's). A broken letter shatters with the Break looks shapes have (#10).
- **Sound**: the Sound card plays on hits like a shape's; notes **By letter** (letter N plays slot or note N) and
  **Spell notes** (the letters A to G play their own note names, in the voice's octave; other letters are silent).
- **Light**: letters already block light through the drawing (part 1); in Stage they stand out of the wall like
  shapes (the extruded field), glass letters bend lasers (`surface::glass_of`), and a Text can be a Lights up target.

## Letter moves (part 4)

- Gradients and layers per letter: a layer's **Across** the whole text / Each letter / Each line (an instance's
  centre becomes the letter's or line's middle); layers worn by only some letters (`Layer.only` reads letter numbers,
  as it reads ball numbers).
- **Letter moves**, a list beside the layers (`LayerSection::Reaction` on `look.shape` for Text only): Wave, Jump on
  the beat, Jump on hit, Flicker (neon-style, seeded, #13 gains it), **Follow sound** (each letter its own level from a modulator, Level or Bus), Shake, Spin each
  letter, Scatter and come back, and **Draw on** (the outline traces in from 0 to 1 over time; the pen's place is
  readable by others). Moves work on each letter's place and turn after `arrange`, pure in time.
- Others point at a Text: the Balls group's Word shape and the Particles "A word" source gain **From a Text thing**
  (its font, layout and path, not just the IBM Plex word), and a Laser can **ride a Text's pen** (#17).

## Experimental (part 5)

Each a fold on the Text's Look card, pure in time: **Echo** (copies toward a moving point, each smaller and shifted
in hue, #23), **Mirror** (2 to 12 ways round the text's middle, the picture's middle or a point, #21), **Colour
split** and **Glitch** (torn horizontal bands, harder on hits, #19), **Melt** (each column of the letters drips and
stretches, Drip amount, Speed, Restart, #20).

## Editing

- The Add thing menu's **Text** heading: **Empty text**, then the looks as starting points, added in the part that
  makes them possible.
- The Words card: the multi-line box (tokens as chips from part 2), the **font picker** (each name drawn in its own
  font, a search box, the seven groups, loaded fonts first, **Load font…**, Remove on a loaded font), and the rest as
  ordinary rows.
- **In the preview** (selected Text): drag the text to move it, drag the corner dot to resize (Size), the right edge
  bar to set the Wrap width, the circle to set the Radius on a circle path. **Double-click the text to type** into a
  box over it; the picture updates as you type; one undo step per typing session; Esc or a click elsewhere ends.

## Looks by part

1. Hook title (#1), Spinning word ring (#12, the ring of text alone), Neon sign (#13), Glass letters (#14), Shadow
   lettering (#18, adds a light), Flowing gradient (#24).
2. Hit counter (#2), Countdown timer (#3), Lyrics on the beat (#4), Labels on balls (#5), Race leaderboard (#6), and
   Hook title gains its pop-in.
3. Balls inside a word (#7), Bounce off letters (#8), Pour in and fill (#9), Letters break apart (#10), Letters that
   play notes (#11).
4. Word made of balls (#15), Particle word (#16), Laser writing (#17), Text follows sound (#22), and Neon sign gains
   its flicker.
5. Glitch (#19), Melting text (#20), Kaleidoscope words (#21), Echo stack (#23).

A look only fills settings when the owner picks it; there are no built-in scenes.

## Testing

- The physics goldens never move for scenes without Text; part 3 adds goldens for balls against letters.
- Unit tests for layout (wrap, align, case, spacing, weight, slant, holes, paths), fonts (lookup order, fallback,
  scene fonts round trip), tokens and standings (part 2).
- Picture tests (`bpe-kit/tests/text_pictures.rs`): a letter's hole is empty, contours don't leak, a screen text
  doesn't move with the camera, lit looks light and shadow, the fast lane matches the full draw closely.
- Editor tests (kittest): Add Empty text, typing in the Words box and in the preview, the font picker, Load font.

## Not doing

- Right-to-left and vertical scripts, emoji and colour fonts (their outlines draw as plain letters).
- Editing a font's outlines by hand (a Text can't be moulded like a Shape; turn it into a Shape later if asked).
- Hardware text rendering libraries or GPU glyph caches beyond the fast lane's atlas.
