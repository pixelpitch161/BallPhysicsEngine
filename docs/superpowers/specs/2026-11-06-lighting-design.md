# Lighting, surfaces and mind-bending picture effects

Date: 2026-11-06. Status: design approved by the owner (Aedan), in the "New visual effects/customization"
brainstorm thread. The owner approved the system shape, how it is drawn, all 33 ideas, the Light Lab and the editor
layout; the setting lists below fill those in. Each build part gets its own plan.

- Lighting Looks (12 live sketches, the six building blocks): https://claude.ai/artifact/QeRgpq4dcqrEgKmhjxaaTG
- Advanced lighting ideas (33 ideas, owner: "all of it"): https://claude.ai/artifact/DyqbrwJv8JmCADoHPuLFpc
- Light Lab (60 live sketches: the 33 ideas, 18 psychedelic effects, 9 mixes, 8 rules):
  https://claude.ai/artifact/9Zj2hxUWzYo37ENcJWNNaG (source: project files `ballphysics/lighting/light-lab-src/`)
- Editor layouts (chosen: A + B + C's handles): https://claude.ai/artifact/5erCSnfj1FQYX6cJ4DDAUD (copy:
  project files `ballphysics/lighting/editor-layouts-mockup.html`)

The sketches are web mock-ups that show the look, not engine renders. Match their look, not their code.

## Goal

Film-quality and wildly experimental light in the same scenes. Any number of lights; anything that already exists
(ball layers, shape layers, trails, particles, the wavefront, the background) can give off light; every ball and
shape has a surface that answers every light; a scene can be flat 2D or a 3D stage; a live quality for editing and
a baked cinema quality on export. On top, a stack of psychedelic picture effects that can work on the whole
picture or on the light alone. Everything mixes with everything, and every setting can be pushed far past its
usual range.

## What the owner decided

| Question | Decision |
| --- | --- |
| Focus | Impact moments, ball materials, world and lighting; **advanced lighting is the headline**. |
| Live or baked | **Both**: live while editing, a heavier bake on export. |
| Styles | All four first styles (Lit 2D, Glow room, Studio 3D, Ray-traced materials) as **one mixable system**. |
| Priority | None. **Breadth**: a very wide range of possibilities. |
| Emitting light | **Anything that already exists** can give off its own light. |
| System shape | Light things; the Gives off light sub-layer; the scene Lighting card; Surfaces with a per-layer Lit amount. |
| How it's worked out | Draw-only; live shadows and a shared light field; Stage traces spheres live; Bake on export; Preview final. |
| Advanced ideas | **All 33.** |
| Light affecting physics or settings | Allowed, but **only as an optional switch**, and it must **never change how lighting works or limit it**. |
| Psychedelic | Besides the high-quality side, **very psychedelic, experimental, mind-bending** effects too. |
| Limits | **As much customization as possible, little or no limit** on how far settings go. |
| Editor layout | **A + B + C's handles**: lights in the tree, a Lights tab, handles in the preview whenever a light is picked. |
| Hardware | The owner's GPU is an AMD Radeon RX 9070 XT (RDNA 4, 16 GB, hardware ray tracing). Push live quality high. |

Small calls made while designing (stated to the owner in the brainstorm):
- No Lighting card on the scene = the picture of today, byte for byte. Adding the first Light thing adds the card.
- Light never touches the physics, the sound or the goldens unless the owner turns on a Light sensing feature.
- Sliders keep a usual span; dragging past the end or typing goes beyond. Only true impossibilities are blocked.

## The rules everything follows

1. **Anything that glows lights the rest.** Balls, shapes, trails, particles, arcs, lasers and hit flashes light the
   scene, cast shadows and show in reflections.
2. **Every surface answers every light.** Chrome reflects it, glass bends and splits it, jelly lets it through, toon
   turns it into bands.
3. **Hits feed light.** Flashes, heat glow, ripples, arcs and flares start from hits.
4. **Sound drives light.** Any light setting can Follow sound; the rig and lighting scenes run on the tempo.
5. **Light can drive things back, optionally.** Light sensing sends how lit a thing is to sound, looks or physics. Off
   by default, one-way: lighting never reads it, so it can never limit the lighting.
6. **Air catches everything.** Haze and smoke show beams from lights, lasers and glowing things alike.
7. **Picture effects bend the lit scene.** Feedback, kaleidoscope, warps and lenses run after lighting, so shadows and
   glows fold with them. Each effect can take the whole picture or the light alone.
8. **Memory remembers any light.** Glow-in-the-dark, echoes and thermal trails work with whatever lit the scene.

## No limits

The owner wants to be able to do "really weird stuff". So:

- Every new number setting uses `span(a, b)` (the slider's usual span), never `range`, unless a value outside the
  range has no meaning or breaks the maths (negative reach, a cone wider than 360°, zero samples). Those few hard
  limits are listed per setting in the part plans and each says why.
- Negative values are allowed wherever they mean something: negative strength gives darkness (Negative light),
  negative falloff grows with distance, negative roughness is clamped at the shader but stored as typed.
- The inspector shows a value past its span with the value field in `text-strong` with a `text` border and a small
  arrow at the slider's end (the mockup's "past" style). Dragging past the end keeps going at the same rate.
- Built-in effect headers (`// simple name = default [a, b]`) become spans too. A header may add `hard [a, b]` for a
  real limit. Today's clamps on the existing 13 effects are reviewed in part 8.
- Counts (lights, samples, bounces, reflections, rays) have no cap in the data. The renderer may warn ("this will be
  slow") and the live view may cap what it draws per frame for speed, but export honours what is typed.

## The parts of the system

### 1. Light things (`light`)

A new thing kind, family Lights, in `bpe-kit/src/things/light/` (`mod.rs` settings + kind, `draw.rs` its handles'
data and its `LightGpu` records, `sim.rs` nothing physical). A light is a thing like a ball or a shape: it sits in the
tree, has a track colour and a timeline group, every number keyframes, and every setting can Follow sound.

Kinds (`LightKindChoice`): **Point**, **Spot**, **Sun** (parallel light from a direction), **Area** (a lit rectangle
or disc with a size, for soft light), **Strip** (a neon tube: a line or the outline of a chosen shape), **Laser** (a
thin beam that bounces off mirror surfaces and bends through glass), **Sky** (light from all round, a gradient top to
bottom, for the base of cinema light).

Settings (one Light card, `SlotId::Light`, folds in this order):
- **Light**: Kind; Colour (Own colour / Gradient over time / Temperature in kelvin / Picked from a thing's layer);
  Strength; Reach; Falloff (Real 1/d², Soft, Linear, None, Drawn curve); Size (the lamp's own size: bigger is softer
  shadows); Height (above the picture in Flat, its depth in Stage); Cone width and Cone edge (Spot); Direction
  (Sun, Spot, Laser); Points (Fixed angle / Where it's going / At a thing / Spins); Width and Length (Area, Strip);
  Follows shape (Strip: a shape's outline).
- **Shadows**: Casts shadows; Softness (over Size); Shadow colour; Shadow strength.
- **Lights up** (light linking): Everything / Only these / Everything but these, with a thing list.
- **On hits**: Flare (strength boost on hits of chosen things), Flare length, Heard from.
- **Gobo and flicker**: a pattern the light shines through (Stripes, Dots, Window, Water caustics, Leaves, Custom
  picture, Drawn), its scale, turn and drift; Flicker amount and speed (seeded).
- **Air**: Beam in haze (how much its beam shows in the scene's haze).
- **Special**: Ultraviolet (lights only layers marked Shows under UV, see Advanced surfaces), Negative (gives
  darkness), Laser bounces and Laser width.

**Riding**: a light whose parent in the tree is a Balls, Emitter or Shape thing rides on it. Rides on: **Each ball**
(one light per ball; no cap, the renderer warns past a few hundred), **The group's middle**, **Ball number N**, or
**The shape's middle / a point on its edge**. A light at the root or in a folder sits where its Placement says.
Riding changes nothing about the parent; core already lets any thing hold children, the kit reads the parent.

### 2. Gives off light (sub-layer)

A sub-layer kind added to every layer kind that takes sub-layers: ball drawn layers (`sub.light`), shape layers
(`shape.sub.light`), the around layers trails, particles and wavefront (added to `AROUND_SUBS`), and a **Gives off
light** fold on the scene's Background settings. Settings: Colour (the layer's own colours / own colour / gradient);
Strength; Reach; Falloff; Casts shadows (All / From balls only / None); Fade along (trails); Flare on hit; Flicker;
Negative (gives darkness); Lights up (linking); Shows in reflections. The light comes from where the layer actually
covers, so a ring outline lights like a ring and a trail like a ribbon.

### 3. The scene Lighting card (`scene.lighting`)

A new card on the scene in a new slot `SlotId::Lighting` (scene only). **No card = today's picture exactly.**
Folds:
- **Room**: Depth (**Flat 2D** / **Stage 3D**); Ambient colour and strength; Sky light; Bounce light (how much light
  bounces off lit things onto others); Light blooms (lights spill into the bloom).
- **Stage** (Stage only): Back wall depth; Floor (None / At the bottom / Under the lowest shape / At a height);
  Floor and wall surfaces (Matte / Glossy / Mirror / any Surface); Shape depth (how far shapes stand out from the
  wall); Camera lens (orthographic front by default, same framing as Flat).
- **Shadows**: Style (Soft / Hard / Toon / Off); Softness; Length (Flat: how far shadows reach); Contact shading.
- **Air**: Haze amount and colour; Haze moves (still / drifting / swirling, speed, scale, balls stir it); Underwater
  (water caustics pattern from above).
- **Memory**: Glow-in-the-dark floor (how much the floor and wall remember light, fade time).
- **Quality**: While editing (Draft / Good / High / Ultra); On export (Same as live / **Bake**); Rays a pixel; Light
  bounces; Reflection depth; Caustics; Motion blur; Depth of field (focus, aperture); Noise clean-up; Use ray tracing
  hardware (Auto / Off). A **Preview final frame** button renders the current frame at export quality.

Adding the first Light thing to a scene adds the card with Ambient 0.25, so the light reads; the owner can raise it.

### 4. Surfaces

How a thing takes light. A **Surface** section at the top of the Look card of balls and shapes (whole thing), plus a
**Surface** sub-layer that gives one layer its own. Surfaces: **Matte**, **Plastic**, **Chrome**, **Metal**,
**Glass**, **Toon**, and the advanced ones: **Jelly and wax** (light soaks in and glows out the far side),
**Pearl and soap bubble** (thin-film colours), **Flake paint** (sparkling flakes under a clear coat), **Brushed /
spun metal** (stretched highlight that turns with the ball), **Frosted glass** (blurs what's behind), **Velvet**
(bright rims), **Lava / heat** (glows from inside). Each is a preset of the same numbers, all editable:
Roughness, Metal, Clear coat, Bends light (index of refraction), Rainbow split (dispersion), See-through, Soak
(subsurface), Film thickness, Flakes (amount, size), Brushing (amount, direction), Frost, Rim, Toon steps, Reflects
(The scene / A sky gradient / Nothing).

Every drawn layer (ball, shape, around) gets a **Lighting** fold: **Lit** (0 = ignores lights and looks exactly as
today, 1 = fully lit; default by kind, `LayerKind::lit_default`: Body, Pattern, Outline, Shading, trails, particles,
wavefront, shape body and Colour and gradient 1; Glow, Shine and shape Glow 0), **Blocks light** (casts shadows,
default on for bodies, off for glows), **Shows in reflections**, **Shows under UV only**. In Flat, a ball's shading
normal comes from **Roundness** on the Surface (0 a flat disc, 1 a full sphere).

### 5. Stage 3D

The physics stays 2D. Stage puts the 2D world on a stage: balls become spheres centred on the picture plane (z = 0),
the background sits on a back wall behind them, an optional floor runs under them, shapes stand out from the wall
by Shape depth with their paint on the front face, and trails, particles and the wavefront are lit sheets on the
picture plane. Balls are drawn as camera-facing impostor spheres, so every existing ball look keeps working, shaded
by the sphere's normal. The default camera looks straight at the stage with the same framing as Flat; **Stage camera
moves** (part 5) add tilt, orbit, height and lens on the Camera thing for Stage only.

### 6. Light sensing (optional, one-way)

Nothing here is on unless the owner turns it on, and the renderer never reads any of it.
- **Follow light**: a right-click entry beside Follow sound on any keyframe setting: Light on this ball, Light on this
  shape, Brightest light nearby, A light's strength, Shadow on this ball. Built like Follow sound: a new
  `PulseSource::Light`.
- **Light you can hear**: a sound modulator kind `sound.light` (how much light falls on the shape's balls).
- **Light-sensitive balls** and **Shadow switches** come from Follow light on look settings.
- **Moths and light pressure**: a Light sensing fold in the Physics card (balls drawn to or pushed from lights), off by
  default.

All of it reads a CPU **light probe** (`bpe-runtime/src/light.rs`, `LightAt`): direct light at a point from Light
things and Gives off light layers, with analytic sphere and wall shadows, no bounce. It is worked out from the world
at a step, like the sound's `ControlSampler`, so physics stays deterministic and identical in editor and export.
When anything physical follows light, the light settings join the physics fingerprint (as Follow sound does).

## How it is drawn (`bpe-gfx`)

Lighting is **draw-only**. With no Lighting card nothing below runs and every pipeline is today's.

With a card, each frame:
1. **The light scene.** The kit's draw code reports, through a new `ThingDraw::light` hook, the frame's
   `LightScene`: lights (`LightGpu`), spheres (every ball: centre, radius, surface, average colour, emission),
   walls (each shape's outline as segments with thickness and depth) and the stage planes. It is uploaded once a
   frame. A uniform grid over the spheres (built on the GPU) serves shadow and reflection rays in crowds.
2. **Material pictures.** Things draw as today, but the lighting pipelines write several targets: the colour as today
   (used as the base colour), the emission (Gives off light), and surface data (normal, surface id, Lit, roughness,
   blocks-light cover). The no-lighting pipelines are untouched.
3. **Direct light.** Each pixel adds every Light thing it can see, with soft shadows: analytic cone-against-sphere
   occlusion for balls and a distance field of the walls (jump flood) for shapes; in Stage, real 3D rays.
4. **The light field.** The emission picture and the blocking cover feed a 2D global-illumination field (radiance
   cascades), so every glowing thing lights the rest with soft shadows and colour spill, and lit things bounce
   light (Bounce light). Shadowed and unshadowed emitters use two fields when both are in use.
5. **Stage tracing.** In Stage, reflections, glass refraction and dispersion trace against the spheres, walls and
   planes, live at a lighter depth (Reflection depth), full on export.
6. **Air.** Haze is marched along the view with each light's shadows, so beams show; swirling haze is a moving noise
   volume that balls stir.
7. **Mix.** `colour × (Lit × light + (1 − Lit)) + emission`, in linear HDR, then the existing effects chain, bloom,
   tonemapping and grade. Lens effects (flares, streaks, fringes) run after bloom.

**Bake on export.** A compute path tracer over the same `LightScene`: many rays a pixel, Light bounces deep, real
caustics (light traced from the lights through glass), spectral rainbows, deep reflections, true motion blur from
the physics' own steps between frames, depth of field, then a noise clean-up guided by the colour and normal
pictures. In Flat 2D the bake runs the light field at full resolution with many more rays. **Hardware ray tracing**
(wgpu's ray query on Vulkan/DX12) is an optional speed-up for the bake's visibility tests, detected at start, with
the compute tracer as the fallback; the picture is the same either way.

**Preview final frame** renders the current frame through the bake into a window. Live and Bake share the scene, the
lights and the surfaces; only the number of rays, bounces and the clean-up differ.

**Memory and determinism.** Nothing physical is kept between frames. Effects with memory (feedback, echoes,
glow-in-the-dark, thermal) declare how long they remember; export runs from the start, so it is exact. After a seek
the editor shows the frame at once, then warms the memory up in the background from that many seconds back and
redraws (like "Aiming… N%"), so a paused frame matches the export.

## The editor (`bpe-studio`)

- **A. Lights in the tree**: Light things show in the tree with a kind icon (sun, spot cone, strip bar…); "+ Add
  thing ▸ Light ▸ Point / Spot / Sun / Area / Strip / Laser / Sky / Rig". Their inspector has the Light card and
  Placement; the timeline gives them a group like any thing.
- **B. The Lights tab**: the left panel switches **Things / Lights / Mixer** (`LeftPanel::Lights`, `lights_panel/`).
  At the top the scene's Depth and Quality chips. Then a strip per light source, grouped by rig: Light things, every
  Gives off light sub-layer anywhere in the scene ("Drifters › Trail"), and Ambient and bounce. A strip has its track
  tag, colour dot, a Strength fader (the real setting, at the playhead, undoable), **M** (turns the light off: saved)
  and **S** (solo: editor view only, never saved or exported). Clicking a strip selects the thing and opens the layer.
- **C's handles**: whenever a light is selected or the Lights tab is open, the preview draws its handles
  (`preview/light_handles.rs`): the lamp, the reach ring, spot cone edges, a sun's direction arrow, an area's size,
  a strip's ends, a laser's path; all drag. A **Show** menu under the preview switches Picture / Light only / Heat
  map (editor only).
- **Surface** section and per-layer **Lighting** fold in the Look editor; the Surface picker shows sphere swatches.
- **Follow light** in the setting menu (beside Follow sound), only offering what exists.
- **Effects stack**: the scene's Shader card becomes an **Effects** card holding effect layers (drag to reorder,
  each with Mix, Blend and **Takes**: Whole picture / Light only / Glow only).
- **Export window**: Lighting Live / Bake, the bake's settings and a rough time measured from a test frame.
- Past-span values look as in the mockup. Every control is named for kittest.

## Psychedelic and experimental effects

New built-in effects (`bpe-gfx/src/effects/builtin/`), each a layer in the Effects stack that can take the whole
picture or the light alone: **Feedback tunnel** (Echo grows zoom, turn, hue, drift and a centre), **Kaleidoscope**
(more folds, radial and grid folds), **Hue storm**, **Liquid light**, **Light ripples** (hit rings that interfere),
**Ball worlds** (each ball a window into the scene, several levels), **Polar tunnel**, **Breathing space** (beat
warp and colour split), **Thermal vision** (heat palette with memory), **Op-art lens** (stripes bent by the balls),
**Fractal light** (a fractal steered by a thing's position), **Rainbow echoes**, **Melting picture** (drip and
smear), **Solarized neon**, **Gravity lens** (balls bend the picture behind them). Plus things that are not picture
effects: **Negative light** (the Negative switch on lights and Gives off light), **Electric arcs** (a new around layer
`layer.arcs`: lightning between balls that come near, which gives off light) and **Aura fields** (a group layer:
flowing fields around the balls with glowing contour lines). The effects prelude gains the ball list (positions,
radii, colours), the hit list it already has, the sound levels it already has, and the light picture.

Lens and camera: **Lens flares**, **Anamorphic streaks** and **Colour fringes** are Lighting card or Effects stack
entries; **Stage camera moves** extend the Camera thing in Stage.

Light shows: a **Rig** thing (a folder for lights) with patterns that drive its lights on the tempo (Chase, Strobe,
Sweep, Alternate colours, Random; on beats, bars or hits; colour lists; direction), and **Lighting scenes** on the
Lighting card: named snapshots of every light's settings, switched or crossfaded on bars, hits or timeline marks.

## All 33 ideas, and where each lands

| Idea | Part | | Idea | Part |
| --- | --- | --- | --- | --- |
| Cinema render | 3 | | Swirling haze | 4 |
| Caustics | 3 (live light: 4) | | Coloured shadows | 1a (falls out of lights + shadows) |
| Rainbows | 3 | | Light through gaps | 1a (shadows) + 1b (haze) |
| Mirror bounces | 2b (live), 3 (deep) | | Smoke that catches light | 4 |
| True motion blur | 3 | | Underwater light | 4 |
| Depth of field | 2b (live), 3 | | Light you can hear | 7 |
| Jelly and wax | 2b | | Light-sensitive balls | 7 |
| Pearl and soap bubble | 2b | | Lasers | 4 |
| Flake paint | 2b | | UV reveal | 4 |
| Brushed metal | 2b | | Glow-in-the-dark floor | 4 |
| Frosted glass | 2b | | Heat glow | 4 |
| Mirror walls | 2b | | Moths and light pressure | 7 |
| Glass shapes and lenses | 2b | | Shadow switches | 7 |
| Glossy floor | 2a | | Lens flares | 5 |
| Anamorphic streaks | 5 | | Colour fringes | 5 |
| Stage camera moves | 5 | | Lighting rig | 6 |
| Lighting scenes | 6 | | | |

The 18 psychedelic effects, Electric arcs, Aura fields and Negative light are part 8. The 9 mixes in the Light Lab
need nothing of their own: each is two or three of the above together, and is a test scene for the part that
finishes its last piece.

## Build parts and order

Each part is a fresh thread on Opus at high effort with its own plan in `docs/superpowers/plans/lighting/`, one Opus
review at the end, then a PR. Every part integrates with everything built before it from day one.

| Part | What | Needs |
| --- | --- | --- |
| **1a** | Light things (Point, Spot, Sun, Area, Strip; riding; Light card in the inspector, A), scene Lighting card (Flat 2D: Room, Shadows, Quality live), `LightScene` and the `ThingDraw::light` hook, material pictures, direct light with soft shadows from balls and shapes, Lit and Blocks light on every layer, Roundness, scene format 11, the no-card-identical test. | – |
| **1b** | Gives off light on every layer kind and the background, the light field (glow room, bounce light, coloured spill), haze with beams, Light blooms. | 1a |
| **1c** | The Lights tab (B), light handles (C), Show: Light only / Heat map, past-span value look across the inspector. | 1a |
| **2a** | Stage 3D: impostor spheres, back wall, floor (Glossy floor), shape depth, 3D shadows, base surfaces (Matte, Plastic, Chrome, Metal, Glass, Toon) and the Surface section. | 1a |
| **2b** | Advanced surfaces (Jelly, Pearl, Flake, Brushed, Frosted, Velvet, Lava), Surface sub-layer, live reflections (Mirror bounces), glass refraction and lenses, Mirror walls, live depth of field. | 2a |
| **3** | Bake: path tracer, cinema light, caustics, rainbows, deep reflections, true motion blur, depth of field, clean-up, Preview final frame, Export window, optional hardware ray tracing. | 2b, 1b |
| **4** | Air and extras: swirling haze, smoke that catches light, underwater light, lasers, UV reveal, glow-in-the-dark, heat glow, gobos and flicker, live caustics. | 1b (lasers through glass: 2b) |
| **5** | Lens and camera: lens flares, anamorphic streaks, colour fringes, Stage camera moves. | 1a (camera moves: 2a) |
| **6** | Light shows: Rig thing with patterns, Lighting scenes. | 1a |
| **7** | Light sensing (optional): `LightAt`, Follow light, `sound.light`, light-sensitive balls, shadow switches, Moths. | 1b |
| **8** | Effects stack (Takes, memory warm-up), the 18 psychedelic effects, Electric arcs, Aura fields, Negative light, effect header spans. | – (Takes light only: 1b) |

Order: 1a first. Then 1b, 1c, 2a, 6 and 8 in parallel. Then 2b, 4, 5, 7. Then 3. Merge order for shared files
(`bpe-gfx/src/lib.rs`, `frame.rs`, `bpe-kit/src/lib.rs`, the inspector): in part number order within a wave; a part
that lands second rebases and runs every test again.

## Quality bars (all parts)

- No Lighting card: the picture is byte-identical to main for every test scene (a test pins hashes made before the
  lighting work).
- Physics goldens and sound goldens do not move in any part, except new goldens for Moths in part 7.
- Live and export draw the same frame at the same quality; Bake differs only by rays, bounces and clean-up.
- No flicker between frames from noise: live noise is seeded by pixel and frame in a way that stays still when the
  scene is still, and the clean-up keeps a still frame still.
- Every new setting keyframes and can Follow sound (hold settings excepted, as today).
- Picture tests check what a lighting feature does by pixel values on small renders (lit side brighter, shadow
  falls away from the light, colour spill takes the emitter's hue, a mirror shows the neighbour). They run headless
  on lavapipe in the cloud.
- Live speed: the owner's GPU is fast, but nothing can be measured in the cloud. Each part adds a frame-time readout in
  the status bar for the lighting passes so the owner can report numbers, and Quality tiers scale every heavy pass.

## Things not to do

- Do not let any lighting code run, or any pipeline change, when the scene has no Lighting card.
- Do not make lighting read Light sensing, Follow light or anything physical that light sensing changes.
- Do not move the physics or sound goldens (outside part 7's new ones).
- Do not add hard `range` limits for taste. Spans only, hard limits only where the maths breaks, each with a reason.
- Do not keep picture state between frames except declared effect memory with warm-up.
- Do not add built-in or example scenes to the program. Test scenes stay test-only.
- Do not invent colours in the editor outside `style/` (the design-system test).
- Do not require hardware ray tracing: it is a speed-up with a full fallback.
