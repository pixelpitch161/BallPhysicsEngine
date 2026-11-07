# Particles part 6: looks 2

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 2, 3.6, 10, 11 part 6). Part 1 (the engine) is in
(`docs/superpowers/plans/particles/part1.md`, read its "As built"). Parts 2 to 7 are built in parallel, so every shared
file is touched additively and new work sits in new files. Built straight after this plan (no approval stops), one
Opus review at the end, then the PR, which this thread merges itself once checks and the review pass. The exe is not
installed by this part (that happens once every particles part is merged).

## What part 6 delivers

- Three new Look layer kinds on `look.particles`:
  - **Trails** (`particles.trails`): each particle's recent path drawn as a line, up to 64 points, tapering and
    fading towards the tail.
  - **Marks that stay** (`particles.marks`): particles that touch a shape (or an edge of the picture, or die) leave a
    mark that stays where it was made, with splats round it and a drip running down from it over time.
  - **Liquid** (`particles.liquid`): the particles splatted into a smooth field on the graphics card, cut at a level
    into a blobby surface and shaded with normals worked out from the field, with a **Surface**: Matte, Plastic,
    Chrome, Metal, Glass (bends what is behind it), Toon.
- Three **modifiers** every drawn layer has (new "Experimental" fold): **Mirror** (2, 3, 4, 6, 8 or 12 ways round the
  middle of the picture, the thing the particles ride, or a point; turned copies or mirrored ones, a kaleidoscope),
  **Colour split** (red, green and blue copies pulled apart along the way each particle goes, by its speed) and
  **Glitch** (bands of the picture torn sideways at random moments, with a colour split in the torn bands).
- New starting looks in the Add thing menu: **Liquid splashes** (Weather and nature), **Flow lines**, **Kaleidoscope**,
  **Colour split dust**, **Liquid metal**, **Paint drips**, **Pixel glitch** (Experimental and psychedelic). Flow lines,
  Liquid splashes and Pixel glitch are their simple forms (part 3's Flow field, part 4's pooling and part 5's Pixels
  finish them when those parts land). Spirograph (look 40) needs part 3's Spirograph force and is left to part 3.

## Decisions made while planning (stated so review can correct them)

1. **Trails need the past, so the state keeps it, only when asked.** A draw is worked out from the state alone (no
   memory between frames), so a particle's recent places live in `ParticlesState` as a ring (`trail.rs::TrailRing`):
   `points` places per particle in one `Vec<Vec2>` (particle *i*'s at `i × points ..`), one written every particle
   step at slot `step % points` (one head for every particle). A particle born later has fewer valid points (its age
   in steps, `valid`), so no per-particle head is kept. The ring is only kept when the Look has a Trails layer:
   `points` = the most any shown Trails layer needs (its Length × Every, at most `MOST_TRAIL_POINTS` 256); Length is 2
   to 64 points, Every 1 to 4 particle steps between points. Both are structure (no keyframes). New particles start
   their ring filled with their birth place; a change of `points` starts every ring again from where each particle is.
2. **Marks are kept in the state too** (`marks.rs::Mark { place, velocity, size, seed, ball, at, why }`), made after
   each particle step from flags the move sets: `TOUCHED_SHAPE` (`meet_walls`, any outcome but Pass, at the touch
   point), `TOUCHED_EDGE` (`meet_edges`: Vanish, Wrap round or Bounce acting), and `DEAD` (died or vanished). Each
   particle leaves at most one shape mark and one edge mark (`MARKED_SHAPE`, `MARKED_EDGE`), so a particle sliding
   along a floor (At shapes: Stop) doesn't flood the list. A Marks layer picks which it draws (On shapes, On edges,
   Where they die: three switches); the state makes the union of what the shown Marks layers want. Most marks (default
   5000, span to 100 000, hard 1..1 000 000) is the largest any layer asks; past it the oldest go. A mark that every
   layer has finished fading (each layer's Fade after + Fade over, when every layer fades) is dropped, so marks that
   fade don't fill the list. Flags are high bits (16..19) so part 4's moment flags can sit in the low ones.
3. **What the sim reads from the Look** is gathered in `wants.rs::Wants` (`trail_points`, `marks_why`, `most_marks`,
   `marks_last`), read from the resolved Look card in `read_into` (build and `settings_changed`). A change of Wants
   must restart the particle playback, but a colour change must not: new `ThingSim::more_fingerprint(&ResolvedThing)
   -> u64` (default 0) is hashed into `beside_fingerprint` for beside things; the particle sim returns `Wants`' hash.
   The physics fingerprint is untouched (particles are still left out of `main_part`).
4. **Modifiers are settings every drawn layer gets** through the layer kind's `dynamic` defs (`modifiers.rs::
   Modifiers`, `#[derive(Settings)]`, group "Experimental"): the `drawn` maker in `look.rs::kinds` sets
   `dynamic: modifiers::defs`, so part 5's layer kinds made the same way get them too. `layers_of` reads
   `Modifiers::from_params` into `LookLayer.modifiers`. Resolve already fills dynamic defs' defaults.
5. **Modifiers work on what a layer drew, on the CPU**, so they work for every layer kind and every sub-layer:
   - Colour split, per particle: the particle's pieces (its sprite and its sub-layers' extras, or its trail strip)
     are drawn into a scratch `Pieces`, then copied three times into the layer's, red moved ahead along the way the
     particle goes by `Split × speed + Split offset`, green in place, blue behind; each copy is that channel alone as
     added light (covering paint turns into its light: the three copies add back up to the colour where they
     overlap). Off at 0 (the scratch is skipped). Liquid has no Colour split (hidden by `when`).
   - Mirror, on the layer's whole `Pieces` (and Liquid's blobs) at the end: `ways − 1` turned copies round the centre
     (turn steps of a whole turn ÷ ways); with **Mirrored copies** on, every second copy is also flipped across the
     line through the centre at its turn (a kaleidoscope). Strip vertices only move (their `edge` is a distance, so a
     flip draws the same), sprites move and turn (`place[2]`; flipped: `2a − turn`). Centre: The middle of the
     picture (the camera's centre), The thing they ride (its balls' middle in the drawn world, or its pose), A point
     (Mirror centre, world px). Off draws one copy (nothing changes).
   - Glitch, on the layer's whole `Pieces` (and blobs) after the mirror: the picture is cut into bands `Band height`
     px tall on the screen; each band, in each slot of `1 / Changes a second`, is torn with chance `Glitch` by up to
     `Tear` px sideways (on the screen: through `CameraView::to_screen`/`to_world`), the dice a hash of (layer id,
     band, slot, Glitch seed). In a torn band sprites also get a red/blue split of `Tear colour split` px (two added
     copies, the covering one kept). A strip vertex moves by its own band, so torn trails kink. Off at 0.
6. **Liquid is a graphics card pass of its own** (`bpe-gfx/src/liquid.rs`, `shaders/liquid.wgsl`), not sprites:
   - `Frame.liquids: Vec<Liquid>`, `Liquid { blobs: Vec<LiquidBlob>, style: LiquidStyle, front: bool }`,
     `LiquidBlob { place: [x, y, radius, weight], colour: [r, g, b, a] }` (linear). The kit pushes one per shown
     Liquid layer (into the blend layer's frame when its blend isn't Normal, as other around layers route).
   - At the start of a render the renderer uploads every liquid of the frame, its overlay and its blend layers (one
     blob buffer, one 256-byte-stride uniform buffer with dynamic offsets), keyed by the `Liquid`'s address for the
     frame's draw (the frame isn't touched between upload and draw).
   - Drawing a liquid: clear the field (Rgba16Float, the picture's size), splat each blob as an instanced quad with
     additive blending (rgb = colour × w, a = w, w = weight × (1 − d²/r²)³), then a full-screen pass into the target
     with premultiplied blending: density F = field.a over the Level, colour = field.rgb / F, the surface's cover a
     smoothstep across the level as wide as Edge softness px (the field's screen gradient tells how far a pixel is
     from the cut), normal from the field's gradient (`normalize(-∂F/∂x · k, -∂F/∂y · k, 1)`, k = Roundness), shaded
     by Surface: Matte (diffuse from the Light angle), Plastic (diffuse + a white highlight), Chrome (a reflection of a
     sky made of Sky top / Sky bottom along the reflected ray, tinted lightly by the colour, plus a highlight), Metal
     (Chrome tinted by the colour), Glass (the picture behind, bent along the normal by Bend px and tinted by Tint,
     with a highlight and a rim), Toon (diffuse in three bands, a hard highlight). Rim lights the edge.
   - Glass reads the picture behind: the renderer copies the HDR picture into `behind` (`copy_under`) right before a
     Glass liquid's surface pass.
   - Where: behind the balls after the frame's trails and sprites (`draw_scene` after the "scene" pass) and in front
     after the flashes pass; in a blend layer's frame and the overlay after each half of `draw_things`. In the light
     passes a liquid draws as covering black (mode 1 lit colour 0, so lighting leaves it as drawn; 3, 5, 6 the same),
     nothing in mode 2 (it blocks no light), and as itself in the behind pass. Its field and pipelines are made the
     first time a frame has a liquid; a frame without one costs nothing.
   - Blobs per liquid are cut to `MOST_BLOBS` (200 000) as a guard. Each blob's radius is the particle's size × Blob
     size, its weight its opacity × fade (so fading particles shrink out of the surface).
7. **Trails draw as strips** through `push_trail` (newest first): width the particle's size × Width at the head,
   × (1 − Taper) at the tail; alpha × (1 − Fade along × along²); colour the layer's colour for the particle (as Sprites
   picks it); softness. A segment longer than a quarter of the picture's shorter side is a wrap or a jump: the trail
   stops there. Strips are capped as every frame's are (`fit_lists`).
8. **Marks draw as sprites**: a mark is one sprite of Made of (default Dots) at its place, size × Mark size, its
   colour the layer's (Colours follow: life = the mark's age ÷ (Fade after + Fade over) when it fades, else 0; speed =
   its speed when made; order, place, time as Sprites; or the parent ball's colour); **Splats** (0 to 12 small dots
   round it, sized and spread by Splat spread, dice from its seed, flung along its speed when made); **Drip** (a
   streak hanging down the screen's down from it, growing to Drip length over Drip time, ease-out, narrowing to the
   tip); **Fade after** (0 never) then Fade over. Twinkle and Size over life read the mark's life. Sub-layers work on
   marks as on sprites.
9. **No new goldens.** The particle goldens hash particles only and stay as they are (a trail ring or marks list never
   changes how a particle moves); unit tests pin the rings and marks (threads, seeks, caps).

## Files

### `bpe-runtime`
- `plugins.rs`: `ThingSim::more_fingerprint` (default 0). `world.rs`: `beside_fingerprint` hashes it per beside thing.

### `bpe-gfx`
- `liquid.rs` (new): `Liquid`, `LiquidBlob`, `LiquidStyle` (Pod uniform: level, softness, roundness, surface code,
  light angle, shine, bend, tint, rim, opacity, sky top/bottom), `LiquidPass` (field texture, splat and surface
  pipelines, buffers, `prepare`, `draw`). `shaders/liquid.wgsl` (new). `frame.rs`: `Frame.liquids` (also in
  `has_overlay`, `draws_nothing`, `set_view` untouched, `cull` leaves them). `lib.rs`: the pass in `Renderer`, prepare
  in `render_scene`, draws in `draw_scene`/`draw_things_part`. `lib.rs` re-exports.

### `bpe-kit`
- `things/particles/trail.rs` (new): `TrailRing`, `MOST_TRAIL_POINTS`, `TrailsLayer` settings, `draw_trails`.
- `things/particles/marks.rs` (new): flags, `Mark`, `collect` (after a step), `prune`, `MarksLayer`, `draw_marks`.
- `things/particles/liquid.rs` (new): `LiquidLayer` settings, `Surface`, `push_liquid`.
- `things/particles/modifiers.rs` (new): `Modifiers`, `Mirror`, `MirrorRound`, `split_into`, `mirror`, `glitch`,
  `mirror_blobs`, `glitch_blobs`, `defs`.
- `things/particles/wants.rs` (new): `Wants`, `read`, `hash`.
- `look.rs`: the three kinds in `kinds()`, `Drawn::Trails/Marks/Liquid`, `LookLayer.modifiers`, `dynamic` on drawn.
- `draw.rs`: `draw_layer` dispatches Trails/Marks/Liquid, the split per particle, mirror and glitch at the end.
- `state.rs`: `ParticlesState.trail`, `.marks`, `.wants`; `memory` counts them.
- `step.rs`: births extend the ring, the ring written after the move, marks collected, dead removed with their rows.
- `meets.rs`: the touch flags (two lines). `sim.rs`: `read_into` reads Wants; `more_fingerprint`.
- `looks.rs`: the seven looks. `tests.rs` / new `tests_part6.rs`: unit tests. `tests/particle_pictures.rs`: pictures.

### `bpe-studio`
- Nothing new is needed: the Look list's add menu and settings grid read the kinds from the registry. Kittest: the new
  looks add from the menu; the Look list adds a Liquid layer and shows its Surface; the Experimental fold is there.

## Settings (labels as shown; spans, not hard ranges, except where the maths breaks)

Trails (common settings, then Trails): Length 16 points (hard 2..64), Every 2 steps (hard 1..4), Width 1 (× size),
Taper 0.7 (0..1), Fade along 1 (0..1), Softness 0.3 (0..1).

Marks that stay (common, then Marks): On shapes (on), On edges (off), Where they die (off), Made of (Dots), Softness
0.4, Mark size 1.5 (×), Splats 4 (0..12), Splat spread 3 (× size), Drip 0 px (span 0..200), Drip time 2 s, Fade after
0 s (0 never), Fade over 1 s, Most marks 5000 (span 100..100 000, hard 1..1 000 000).

Liquid (common, then Liquid): Blob size 4 (× size, span 0..12), Level 0.5 (span 0.05..2), Edge softness 1.5 px,
Roundness 1 (span 0..4), Surface (Plastic), Light angle −45°, Shine 0.6 (0..1), Bend 12 px (Glass), Tint 0.3 (0..1,
Glass), Sky top (pale blue-white) and Sky bottom (dark grey) (Chrome/Metal), Rim 0.3 (0..1).

Experimental (every drawn layer): Mirror (Off; Two/Three/Four/Six/Eight/Twelve ways), Mirror round (The middle of the
picture / The thing they ride / A point), Mirror centre (0, 0 px, A point only), Mirrored copies (off), Colour split
0 s (span 0..0.1, hidden on Liquid), Split offset 0 px (span 0..20), Glitch 0 (0..1), Band height 24 px, Tear 40 px,
Changes a second 8, Tear colour split 4 px, Glitch seed 0.

## Steps and how each is verified

1. Runtime hook: `more_fingerprint`, hashed in `beside_fingerprint`. A runtime test: a beside thing whose sim's extra
   fingerprint changes changes `beside_fingerprint`; the physics fingerprint doesn't change.
2. Kit state: Wants, trail ring, marks (flags in meets, collect, prune, compaction with rows). Unit tests: the ring
   holds each particle's last places in order (straight fall: points on the line, newest = place); new particles
   have `valid` 0 then grow; a removed particle's ring row goes with it (the next particle's row is its own); marks
   come once per particle on a shape (Stop), at the touch point; edges and deaths when asked; the cap drops the
   oldest; faded marks are pruned; 1 thread equals 8 for rings and marks; a seek back and forth (a playback) equals a
   straight run; no ring and no marks without such layers (memory 0 for them).
3. Modifiers: unit tests on `Pieces`: Mirror Four makes 4 copies turned by a quarter; Mirrored copies flip every
   second; Colour split makes 3 added copies of channels that sum to the colour, red ahead along the velocity;
   Glitch 0 changes nothing, 1 moves whole bands by at most Tear, the same at the same moment, differently later.
4. Draw: Trails, Marks, Liquid layers on frames (counts, colours, drip grows, splats in the radius, liquid blobs one a
   particle with size × Blob size, mirror on blobs). `Frame.liquids` routed into a blend layer for a non-Normal blend.
5. GPU: `liquid.rs` + `liquid.wgsl`. Pictures (`tests/particle_pictures.rs`, GPU): a Liquid layer draws a filled
   blob where particles crowd and nothing far away; Chrome, Glass (bends a striped background: pixels inside differ
   from the plain picture), Toon each draw without error and differ from Matte; Trails draw lines behind falling
   particles; Marks stay on a floor after the particles are gone; Mirror Four puts the picture's quarters alike
   (rotated tile averages match); Colour split shows red and blue fringes; a lit scene with a liquid renders.
6. Looks: the seven looks resolve without problems and make particles (and, for Paint drips, marks; for Liquid
   splashes and Liquid metal a liquid) within their first seconds (`looks` unit test over `Look::all()`).
7. Editor kittest: the looks from the Add thing menu; "+ Add layer" offers Trails, Marks that stay and Liquid; the
   Experimental fold shows Mirror.
8. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace` (debug), the particle and ball goldens
   unchanged (debug and release), CLAUDE.md map, the Windows exes build.

## Not doing (part 6)

- No change to how particles move (goldens unchanged), to `layer.particles` / `shape.particles`, or to two-way.
- Liquid isn't lit by the scene's lights (its own Light angle shades it; part 7 may light it) and doesn't refract the
  balls in the light passes. Liquid has no Colour split; layers whose things blend with each other draw a liquid as
  Normal inside the layer.
- No Spirograph look (part 3), no pooling (part 4), no Pixels sprite shape (part 5): their looks use simple forms.
- No built-in scenes, no demo videos.

## As built

- Built as planned. Trails, marks and the modifiers are as in decisions 1 to 5, 7 and 8; the liquid is decision 6.
- The liquid's splat pass has a bind group of its own holding only the style (the picture tests found that binding
  the field texture in the group while drawing into it is a wgpu validation error).
- The GPU checks are in `crates/bpe-kit/tests/liquid_pictures.rs` (named so it can't clash with another part's
  `particle_pictures.rs`): the liquid fills where particles crowd and leaves the background, every surface differs
  from Matte, Glass bends stripes seen through it (Bend 0 vs 40), Trails show behind a falling particle and not ahead,
  Mirror Four puts copies at the quarter turns, and a lit scene with a Glass liquid renders. Marks on a floor and the
  colour split fringes are checked on the CPU side (`tests_part6.rs`: marks counted and drawn, split copies sum to the
  colour) rather than in pictures.
- The editor kittest (`look_editor/particles.rs`) adds Trails, Marks that stay and Liquid through "+ Add layer" and
  checks the Liquid's settings and Experimental fold show, without Colour split.
- `tests.rs`' every-look test allows an On hits look to make nothing in a blank project (it has no balls to hit).
- After the review: `Frame::clear` empties `liquids` (the editor reuses one frame; liquids piled up); the trail ring
  keeps at most `MOST_RING_PLACES` (4 million places, 32 MB) over Most particles, so a huge crowd keeps shorter
  trails; a Liquid caps its blobs at `MOST_BLOBS / ways` before Mirror so every copy shows whole; a particle dropped
  for a place that isn't a number leaves no mark; Mirror and Glitch work in screen places for a thing stuck to the
  screen; the Look's wants are read at build only (nothing they read can be keyed, and a Look edit rebuilds through
  `more_fingerprint`), so a keyed Look setting no longer re-reads the whole Look every step. Marks' Made of is no
  longer Simple (at most 6 a layer kind). With Wrap round, an edge mark goes where the particle comes back in.
- The tree's kittest window is 1200 px tall so the Add thing menu's Rig entry is on screen.
- Rebased onto parts 2 (the air) and 5 (looks 1): every part 5 kind (Soft smoke, Flame, More shapes, Glitter, Web
  lines) takes the Experimental fold; Soft smoke and Flame have their field's splats mirrored and torn
  (`modifiers::mirror_splats`/`glitch_splats`) and, with Web lines, no Colour split. Part 5's Fine detail (in every
  layer's `Common`) also draws copies of trails (the whole trail moved), marks and liquid blobs. Both parts added a
  "Pixel glitch" look: part 5's pixels on a grid are kept and given part 6's Glitch.
- Merged with part 3 (motion), which has its own "Flow lines": part 6's look of that name is **Light streams**.
