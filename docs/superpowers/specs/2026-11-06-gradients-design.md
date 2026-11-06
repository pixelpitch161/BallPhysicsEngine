# Better gradients: blending, loops, shapes, movement and a real editor

Date: 2026-11-06. Status: design approved by the owner (Aedan) in the "Improve gradient options" brainstorm thread.
He asked for every option shown in every area ("all" each time) and approved both design sections. Each build part
gets its own plan in `docs/superpowers/plans/gradients/`.

Mock-ups (pictures made for the brainstorm, not engine renders; match the look, not the code). They are copied in
project files `ballphysics/gradients/`:

- `loop-options.png`: the rainbow seam on a ring and the two fixes (owner: both).
- `blending-options.png`: Mix colours in, gap blends with a middle handle, Bands, Grain (owner: all).
- `shape-options.png`: the ten new gradient shapes (owner: all, one shared list).
- `movement-options.png`: the eight ways a gradient moves (owner: all).
- `editor-options.png`: the editor layouts (owner: **C, tabs, plus B, the big editor as a pop-out**).

## Goal

The owner's trigger: a ring of balls coloured by Order with the Rainbow palette has a sharp cut between purple and
red, because Order gives the first ball the start of the gradient and the last ball its very end
(`index / (count - 1)` in `bpe-kit/src/paint/eval.rs::offset` and `layers/group.rs::group_t`). Fix that, and make
every gradient in the program richer at the same time: how colours mix, gradients that loop, new shapes for gradients
that fill an area, gradients that move, and an editor that makes all of it quick to use. Push settings far (the
owner wants as much customisation as possible and psychedelic looks beside clean ones).

## What the owner decided

| Question | Decision |
| --- | --- |
| Which areas | All four: blending, shapes and layouts, movement, editing. |
| The ring seam | **Both**: a Loop switch on any gradient, and There and back as an extra Order choice. |
| Blending | All four: Mix colours in (five ways), a blend and middle per gap, Bands, Grain. New gradients mix Even. |
| Shapes | All ten, one shared list for every gradient that fills an area. |
| Movement | All eight, each a plain number setting (keyframes, hits, follow sound and light all work on it). |
| Editor | Tabs (Colours, Blend, Shape, Move) under a compact bar, plus a button that opens a big editor window. |
| Old scenes | No backward compatibility needed; they may look slightly different. |

## The rules everything follows

1. **One gradient, everywhere.** Every `Gradient` value in the program gets the new blending and Loop: ball layers,
   sub-layers, outlines, glows, shines, recolours, group gradients, trails, particles, the wavefront, aura, shape
   layers, the background, light colours and rig colours. No site keeps a private gradient type.
2. **The value holds what shapes the colours; settings beside it hold what moves or lays them out.** Mix, Loop and
   each gap's blend and middle are part of the `Gradient` value (they copy, paste, save in palettes and keyframe with
   it). Bands, Grain, the shape and its numbers, and the movements are **companion settings** stored beside the
   gradient's own key (see below), so each is an ordinary keyframable number that hits, sound and light can drive.
3. **Drawing only.** Nothing here moves a ball. Physics goldens (`crates/bpe-kit/tests/golden*.txt`) must not change
   in any part. Sound goldens must not change.
4. **Pure in time.** A gradient at a moment depends only on the settings, the time, the tempo and the log, never on
   what was drawn before, so scrubbing and export agree.
5. **No limits.** Numbers get a usual `span` and only a hard `range` where a value would break (a negative band
   count, a sides count below 3). Speeds, amounts and sizes go as far as the owner drags them.
6. **The design system.** Every new editor control uses the tokens in `bpe-studio/src/style/` (no made-up colours; the
   colour test must stay green) and IBM Plex. Kittest needs every control named.

## 1. The gradient value (`bpe-core/src/color.rs`)

`Gradient` becomes:

- `stops: Vec<Stop>`, where each `Stop` gains `blend: GapBlend` and `middle: f32` (0..1, default 0.5): how the gap
  **after** this stop blends to the next. The last stop's gap is used only when the gradient loops.
- `mix: Mix`, how colours between two stops are worked out:
  - **Light** (today's): straight blend in linear light.
  - **Plain**: straight blend of the sRGB numbers, like most paint apps.
  - **Even**: straight blend in OKLab, so brightness changes evenly and the middle never goes muddy.
  - **Round the wheel**: OKLCH, lightness and chroma blended straight, hue the short way round. Replaces the old
    whole-gradient `Transition::Hue` (which was HSV).
  - **Long way round**: OKLCH, hue the long way round.
  Alpha always blends straight. Out-of-gamut OKLab/OKLCH results are brought into sRGB by reducing chroma at the same
  lightness and hue (not by clipping each channel), so Round the wheel stays smooth.
- `looped: bool` (**Loop**): the gradient is a circle. Between the last stop and the first (through 1 = 0) it blends
  with the last stop's gap. `sample(t)` wraps `t` with `fract` when looped, so `sample(1.0) == sample(0.0)`.
- `GapBlend`: **Smooth**, **Eased** (smoothstep), **Ease in**, **Ease out**, **Steps** (holds the first colour to the
  middle, then the second: a hard edge at the middle). `Transition` is removed.
- The **middle** moves where the halfway colour sits: the gap's own 0..1 is bent by a power curve so that
  `f(middle) = 0.5` (for Steps it is simply where the edge falls).
- Saving: the new fields are written; reading accepts the old `transition` key and turns it into gap blends (Smooth,
  Eased, Steps) or Mix (Hue → Round the wheel). A gradient saved without `mix` reads as **Light** and without
  `looped` as false, so every existing scene and test scene keeps its exact colours. A solid colour still saves as a
  bare hex string. No format bump is needed.
- **New gradients mix Even.** Code that makes a gradient for the owner (the gradient bar's "+", Palette picks,
  Random, harmony, starters that start with more than one colour) sets `Mix::Even`. `Gradient::solid`, `even` and
  `Default` keep Light, so test scenes and pinned pictures do not move. A one-stop gradient is unaffected by mix.
- `bake(n)` keeps its meaning. **Atlas rows grow from 256 to 1024 samples** (`bpe-gfx/src/gradients.rs::ATLAS_WIDTH`;
  `PAINT_WIDTH` in `paint.wgsl` follows; the grade map row keeps its own width constant) so Steps edges and Bands stay
  crisp across a full-height background. The old whole-gradient `steps` flag in `PaintGpu` goes: hard edges are baked.
  Picture tests whose hashes move from this must be shown to differ by at most 1/255 per channel before they are
  re-pinned; anything bigger is a bug.
- `Value::lerp` for two gradients (keyframes) also blends `middle`; `mix`, `looped` and `blend` switch at the halfway
  point. Lerping gradients with different stop counts keeps today's behaviour.

**Loop and things that count balls.** Wherever a place along the gradient comes from counting (`index / (count-1)`),
a looped gradient uses `index / count` instead, so the last ball sits one step before the start and the ring closes:
paint `Follow::Order`, group `Spread::Order` and `Spread::Round`. Launch angle, Time, Scroll and Repeat already go
round and become seamless with a looped gradient. When Loop is switched on in the editor and the last stop sits at 1.0,
the stops are re-spaced evenly to `i / n` in the same edit so there is room for the blend back. The **Rainbow**
palette ships looped (stops at `i/6`, Mix Round the wheel).

**There and back.** A new choice beside Order in paint `Follow` (`OrderThereAndBack`) and group `Spread`
(`ThereAndBack`): `t = 1 - |2·index/count - 1|`, so ball 0 is at the start, the middle ball at the end and the last
ball one step short of the start again. One ball takes the first colour.

## 2. Companion settings

A gradient setting with key `k` (whatever its prefix, e.g. `colours`, `bg_colours`, `outline_colour`, `3/colours` in a
layer) may have companion keys `k~<name>` in the same params, in the style of the vary keys (`<key>~to`). They are
defined once, centrally (`bpe-core`, beside `vary.rs`: a `gradient_extras` module), and every gradient setting gets
them without each struct naming them; `def_at` answers their definitions like any setting (so keyframes, pulses,
Follow sound, Follow light, Copy/Paste Value and the timeline all work on them). Each has a **tab** tag that the editor
uses (Blend, Shape or Move). A companion that is absent reads as its default, and every default leaves the gradient
exactly as it is.

- Blend tab: `~bands` (0 off; N flat bands, each the colour at its band's middle; looped bands wrap seamlessly),
  `~grain` (0 off; per-pixel noise, still from frame to frame, strength in 1/255 steps; only where a gradient is
  painted per pixel).
- Shape tab: see section 3. Offered only on gradients that fill an area.
- Move tab: see section 4. Offered on every gradient.

Which gradient settings fill an area is the kinds' say (a flag on the setting def, `area`), not a guess from the key.

## 3. Shapes (gradients that fill an area)

One shared list, `GradientForm`, used by every gradient that fills an area: a ball's Body (when it follows Each
ball), every ball layer and sub-layer that has its own colours (Pattern's own gradient, Outline, Glow, Shine, Shading
where it has a gradient), shape paint layers (Colour and gradient, Outline, Glow) and the background. It replaces the per-site lists `GradientShape` (Straight/Round),
`PaintRuns`/`LineRuns` (Across / From the middle / Along the edge), `PlaceShape` (Straight/Round) and the gradient
patterns of `inside::Pattern` (Linear, Radial, Sweep, Stripes, Rings). The Pattern layer keeps its mask patterns
(Dots, Checker, Halves, Slices, Swirl, Face) as its own choice; when it uses a gradient it lays it out with the shared
list.

Every form has `~form`, `~turn` (degrees), `~centre` (in the site's own units: ball units inside a ball, shape units
in a shape, world or screen in the background), `~size` and `~repeat` (Clamp / Repeat / Mirror). Forms and their own
settings:

| Form | Own settings | Notes |
| --- | --- | --- |
| Straight | | today's Linear / Across |
| Round | | today's Radial / Outward / From the middle |
| Sweep | | round the centre |
| Stripes, Rings | | today's repeating ones |
| Along the edge | | shapes only (today's), and outlines along their ring |
| **Diamond** | | `|x| + |y|` |
| **Polygon / star** | Sides (3+), Star depth (0 = polygon) | |
| **Spiral** | Arms, Twist | |
| **Kaleido sweep** | Mirrors | a sweep folded back on itself |
| **Off-centre round** | Focus (x, y), Stretch, Stretch turn | round with the bright point moved off the middle |
| **Waves** | Count, Height, Flow | straight, bent by a sine; Flow moves it |
| **Marble** | Detail, Bend, Seed | straight, bent by noise |
| **Free points** | Softness | each stop is a dot placed in the area; colours blend by distance |
| **Ripples from things** | Spacing, Flow, From (Everything / Balls / Shapes) | rings out from every ball and shape |
| **Turbulence** | Detail, Speed, Seed | noise that drifts over time |

- **Free points:** stop `i`'s place is `~points[i]` (`Value::Points`, kept the same length as the stops: adding or
  removing a stop adds or removes its point), dragged on the preview and in the ball view. At most 16 points; more
  stops than that fall back to the first 16. Blending is inverse-distance with Softness as the power.
- **Ripples from things:** distance from each pixel to the nearest ball or shape outline in the picture, divided by
  Spacing, moved by Flow. It needs a distance picture of the things, built only when some gradient in the frame uses
  Ripples (the jump-flood code the lighting uses is the model). Inside a ball or shape it measures from the other
  things.
- Seams: Sweep, Spiral and Kaleido wrap at their angle; with a looped gradient they are seamless.
- On the GPU every form is a pattern code in `bpe-gfx/src/shaders/paint.wgsl::paint_colour` (shared by balls, shapes
  and the background), with the form's own numbers in the paint record. Noise is the same hash on CPU and GPU so
  probes and thumbnails agree.

## 4. Movement

Every gradient gets these companions (Move tab). Each is a number with its own speed where it has one; all are
keyframable, take hit pulses and can follow sound or light. Defaults are 0 (still).

| Move | Settings | What it does |
| --- | --- | --- |
| **Cycle** | Speed (cycles/s), Shift | slides the colours along; with Loop it is seamless, without Loop the end cuts |
| **Step on the beat** | Every (beats; a bar = beats in a bar), Glide (s) | jumps one stop along per step, from the scene's tempo (`things::scene::draw::beat_at`) |
| **Breathe** | Amount, Speed or On the beat | squeezes and widens the stops around the middle |
| **Hue spin** | Speed (turns/s), Turn | turns every colour's hue together (OKLCH) |
| **Stops wander** | Amount, Speed, Seed | each stop drifts on its own, smooth seeded noise, never crossing a neighbour |
| **Hit wave** | Strength, Width, Speed, Colour (White / a colour) | a hit sends a bright band along the gradient from its start |
| **Morph** | Amount (0..1), To (a second gradient) | blends every point of the gradient toward the second one: `mix(A(t), B(t), amount)` in the gradient's own Mix |
| **Shape moves** | the form's own Flow / Speed / Turn speeds | spiral turns, waves flow, marble and turbulence drift |

- Cycle, Step, Breathe, Hue spin, Wander and Morph are applied to the gradient value **once per frame, before
  anything draws**: a pass (`bpe-core`, `gradient_extras::apply_moves(resolved, time, tempo)`) runs right after
  `Animation::apply` wherever drawing builds its settings (`bpe-gfx/src/frame.rs::build_frame_sensing`, the light
  probe `bpe-kit/src/sensing/probe.rs`, and any other draw-time resolve). Every kit then sees the moved gradient for
  free. Bands are applied in the same pass, after the moves.
- **Hit wave** is per ball or shape, so it is not in that pass: the paint record carries its numbers and the GPU uses
  the instance's own hit age (`LayeredInstance.hit` / `PathInstance.hit`; a stack with a hit wave `needs_hits()`).
  Colours worked out on the CPU per instance (`paint::colour_at`) apply it there from the ball's last hit.
- The physics never reads gradients, so the world does not run the pass.

## 5. The editor (`bpe-studio`)

The gradient row (`gradient_bar.rs`, every place a gradient setting is drawn) becomes:

- **The bar**, a little taller: stops as triangles under it (click the bar to add, drag to move, drag off to remove,
  click to select, right-click Remove), and a **diamond** in each gap that drags the gap's middle. A looped gradient
  draws its wrap part after the last stop.
- **Tabs** under the bar: **Colours | Blend | Shape | Move**. A tab with nothing to show is left out (Shape on
  gradients that do not fill an area). The tab picked is remembered per setting for the session.
  - Colours: the selected stop's colour swatch, hex box and place (%), its gap's blend chips (Smooth, Eased, Ease in,
    Ease out, Steps) and middle slider, then the tools Flip, Spread evenly, Random, Pick from picture, Palette.
  - Blend: Mix chips (Light, Plain, Even, Round the wheel, Long way round), the Loop switch, Bands, Grain, and "Set
    every gap" (a blend for all gaps at once).
  - Shape: the form chips (painted as small previews, like the surface chips) and the form's settings.
  - Move: each move as a fold with its settings; a move that is on shows a dot on its fold.
- **The big editor** (a button on the bar's row): a window with a wide bar, a table of stops (number, colour, hex,
  place, gap blend, middle, remove), the Mix chips and Loop, the tools, and small previews of the gradient on a ball
  and a ring of balls. It edits the same setting; closing it changes nothing.
- **Pick from picture**: an eyedropper over the preview; the click sets the selected stop's colour from the drawn
  picture (not the scene colour under it).
- **Spread evenly** honours Loop (`i/n` looped, `i/(n-1)` not). **Flip** reverses the stops (and their points).
  **Random** rolls a palette (the existing `palette_gen`) into the stops kept. Copy/Paste Value on a gradient carries
  its companions too ("Copy gradient with its settings").
- Free points are dragged in the preview and the ball view while the Shape tab shows Free points.
- Every new control is named for kittest; the inspector's existing right-click menu works on every companion.

## Build parts and order

Each part: plan in `docs/superpowers/plans/gradients/partN.md`, one Opus review at the end, then the PR. Part 1 goes
first; parts 2, 3 and 4 then build at the same time, in their own threads, each merging main before its PR.

1. **Core and the tabbed row.** Section 1 entire (value, Mix, gap blends and middles, Loop, saving, atlas 1024,
   Order `i/count` when looped, There and back, Rainbow looped, every `Transition` user moved over), the companion
   mechanism (section 2) with Bands and Grain, and the editor's bar with diamonds, the Colours and Blend tabs and the
   empty Shape/Move tab slots that list companions by their tab tag. Tests: OKLab/OKLCH against reference values,
   gap blends and middles, loop wrap, `index/count`, There and back, bands, saving old `transition` files, picture
   tests for Mix, Loop on a ring and Bands, kittest for the bar, diamonds and tabs.
2. **Shapes.** Section 3: `GradientForm` replacing the per-site lists at every area site, the ten new forms in
   `paint.wgsl`, Free points (points value, preview dragging), Ripples (the distance picture), the Shape tab's chips.
   Tests: a picture per form on a ball, a shape and the background; Ripples only builds its picture when used.
3. **Movement.** Section 4: the move companions, `apply_moves` and its call sites, Hit wave on GPU and CPU, Morph's
   second gradient, the Move tab. Tests: each move pure in time (same frame twice, scrub back and forth), Step on the
   beat against the tempo, Hit wave on a hit, follow sound on Cycle Speed, export equals preview.
4. **The big editor and tools.** Section 5's window, stop table, Flip, Spread evenly, Random, Pick from picture,
   copying a gradient with its settings. Tests: kittest for the window and each tool.

Parts 2 and 3 each add companions; part 1 defines the mechanism so they only add definitions and their tab content.
Part 4 touches only the editor.

## Quality bars (all parts)

- `cargo test --workspace` green (bar the known release-only runtime test), `cargo clippy --workspace` clean,
  `cargo fmt`.
- Physics and sound goldens unchanged.
- No gradient with every new setting at its default draws differently from today, except the atlas precision
  (≤ 1/255 per channel, part 1).
- CLAUDE.md's crate map updated in each part (the gradient value, companions, forms, moves, the editor row).
- Windows exes built from main after each merge go to the owner as usual.

## Things not to do

- Don't add built-in or example scenes.
- Don't keep `Transition` or any private per-site gradient shape list around "for compatibility".
- Don't make a move or shape read anything drawn earlier (no frame-to-frame state).
- Don't cap speeds, counts or amounts beyond the hard ranges named here.
- Don't put Bands or Grain inside the gradient value: they are companions so they can be keyframed and followed.
