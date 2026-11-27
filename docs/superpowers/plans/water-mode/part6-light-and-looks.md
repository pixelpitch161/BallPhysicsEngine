# Water mode part 6: Light and the six looks (plan)

> **For agentic workers:** built natively in one thread (Aedan's project rule: one Opus review per part, no
> approval stops after the design). Steps use checkbox (`- [ ]`) syntax for tracking. Part 6 is three pull
> requests in this thread, each merged before the next is opened: **6a** the renderer (light, materials, back, the
> liquid's look), **6b** the extras (foam, bubbles, mist, sparks) and wet marks, **6c** the six looks and the
> shared finish (Effects, bloom, Grade, masks). Each gets its own review; Aedan's 2026-11-27 07:49Z ask (stills and
> a short render of each look, sent without waiting for him) comes after 6c merges.

**Goal:** a Water scene is drawn by its own lit renderer. Lamps (Spot, Strip, Sun) light the course in 2D:
solids throw soft shadows, light dims and takes the liquid's colour as it goes through it, a wavy surface focuses
it into moving caustics, and Shafts show the beams. Glowing liquid lights the rock round it. Every Material is
drawn (Rock, Glass, Paper, Basalt, Flat) with its texture, rounded edge, roughness and outline, the Back shows
darker rock, a colour, a gradient or a picture, and the Liquid's Look card is drawn: colour from its own, speed or
depth, see-through and bent, shine, edge sheen, ripples, thin film, glow, paint, outline and bands. Foam, bubbles,
mist, sparks and wet marks come from what the water does. Picking one of the six looks writes its row into the
scene's own settings. Effects, bloom and Grade (with masks that pick the water, the solids and the back) finish
the picture as they do in a Balls scene. Nothing here needs Make final again, and every Balls picture stays
byte-identical.

**Architecture:** `bpe-water` gains a second drawing, `draw::look` (`LookGpu`, `LookDraw`, inputs `Look`),
built on the plain drawing's splats and depth ladder (shared, not copied) and adding world-space passes over the
course (the liquid's light density and glow, the lamp light map, the spread light) and a picture-space compose
that writes linear HDR into a `bpe_gfx::Renderer`, whose finish (exposure, bloom, Effects, Grade, the roll-off and
dither) makes the output. `bpe-gfx` gains one entry point that finishes a picture drawn elsewhere. `bpe-kit` reads
the cards into a `Look` (`things/water/look.rs`) and holds the looks table (`modules/water/looks.rs`). The three
places that draw water today (the stage's draft, the Final view, the export and Preview final frame) switch from
`Plain` to `LookDraw` with the same `(drops, view, look, ahead)` entry point. `Plain` stays as it is (its tests,
the look check's plain group and `golden-plain.txt` keep drawing it).

**Tech Stack:** Rust, wgpu/WGSL (`bpe-water`, `bpe-gfx`), egui 0.37 + kittest (`bpe-studio`).

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` section 6 (Light and look), section 7 (Grade
mode on a Water scene, Shot only), Parts 6, "The water checkpoint". Hand-over:
`/mnt/project-files/ballphysics/water-mode/part6-handover.md`.

## Global Constraints

- Balls scenes byte-for-byte unchanged: no Balls golden or pinned picture moves. `Renderer::finish` is split so
  the Balls path runs exactly the passes it runs now; the new entry point only adds.
- The plain drawing is unchanged: `plain.wgsl`'s text and `Plain`'s passes stay, `golden-plain.txt` does not
  move. The look drawing compiles `plain.wgsl` followed by its own `look.wgsl` into one module and reuses the plain
  pipelines for the ladder, so the two can't drift.
- **Nothing here needs Make final again**: no look setting goes into `WaterSetup.print` or the take's print, and
  every look input is read from the drops of the moment (and, for wet marks in 6b, from a field the make keeps next
  to the take). `what_restarts_the_draft` gains every new setting as "does not restart".
- **A frame is the same every time it is drawn**: no random numbers, no atomics that add floats, no state carried
  from the frame drawn before. Rays go out in fixed patterns, noise is a hash of the world point, the fixed number
  of passes never depends on timing. Scrubbing and the export give the same picture to the bit on one card.
- **The light doesn't depend on the view**: every world-space pass (density, light map, spread light, wet marks)
  is laid on a grid fixed to the world and the course, and liquid and solids are read over the whole course, so
  the same world point gets the same light whatever the camera shows (at one Quality).
- Animated looks (ripples, fizz, sparks' trails) run on **water time**, so they slow with slow motion.
- Editor colours only from `style/` tokens; controls named for kittest; no `ComboBox`; no glyphs the default fonts
  lack.
- Physics crates refuse platform maths; `bpe-water`'s drawing code is not physics but uses `bpe_core::maths` where
  the CPU builds numbers anyway (gradients, colours), so the clippy rule needn't care.
- Windows-friendly; cross-built with MinGW; no exe built or installed; Aedan never runs commands.
- Tests: only what part 6 adds or changes. Picture tests through `bpe_gfx::test_device()`, at 270 x 480 or less.
  New pinned pictures only through the "Refresh GPU pictures" workflow (6c pins the looks).
- Speed numbers told to Aedan as facts are measured on the RX 9070 XT (the card job's timing step); an estimate is
  called one.

## What exists (read before building)

- `bpe-water/src/draw/plain.rs` and `shaders/plain.wgsl` (C3, parts 2b, 3, 8): splats of every drop into two
  `Rgba16Float` amount pictures, the depth ladder over the course (`Depth`, `ladder_for`, `fs_world`, `fs_base`,
  `fs_down`), `fs_show` (back, liquid by depth, rim, rock with a darker rim, dither, sRGB into `Rgba8Unorm`).
  `View { lo, scale, width, height }`, `PlainLook`, `draw_at(drops, view, look, ahead)`.
- The take holds per drop its cell, kind's low byte (`AIR` 128 kept), place to 1/256 and speed as a signed log
  byte; `StoredDrops::set` puts a moment on the card as `xv` (grid units, speed in grid units a second) and `id.x`
  (kind). Not stored: age, heat, foam, hits. `nf` is always "full" for stored drops.
- The three drawing places: `bpe-studio/src/water_mode/picture.rs` (`StagePicture::show`, whole course at its size
  on screen, at most 2048 a side), `final_view.rs` (`FinalPicture`), `bpe-cli/src/water.rs` (`Drawing`,
  `draw_moment`, `export`, `final_frame`). Each builds its look with `bpe_kit::things::water::draft::plain_look`.
- The cards (part 1): `modules/water/liquid.rs` `WaterLook` (groups Its colour, Its body, Its surface, Glow,
  Crust, Drawn, Extras, Mixing shows), `solids.rs` `MaterialCard` (Material, Colour, Second colour, Texture size,
  Depth, Roughness, Outline, Wet marks, Wetting), `scene.rs` `WaterLightCard` (Ambient 0.3, Bounce, Caustics,
  Shafts, Quality) and `BackCard` (Darker rock, Colour, Gradient + Direction, Picture), `places.rs` `LampCard`
  (Spot, Strip, Sun; Colour, Brightness, Aim, Spread, Length, Softness, Shadows). A Lamp is a placed thing
  (`transform.position`, keyframed as `position`).
- The scene's Picture card (Bloom, Exposure) is shown in Water scenes but nothing reads it yet.
- `bpe_gfx::Renderer::finish` (`renderer/scene.rs`): picture effects (`chain.run`, light pictures from
  `light_pictures`), bloom, silhouettes for Things masks, grade passes, composite into `output_view`
  (`Rgba8Unorm`). `render_scene` writes the globals first.
- The take's engine print hashes all of `bpe-water/src` and the kit's `things/water` and `modules/water` (each
  crate's `build.rs`, `ROOTS`).

## Decisions (my calls; cheap to change before they are built)

### The renderer as a whole

1. **`LookDraw` is a new drawing beside `Plain`, not a change to it.** It owns a `Plain` (for its field buffer,
   its depth ladder and its uniform, which the look passes bind) and a `bpe_gfx::Renderer` (the finish). Its module
   is `plain.wgsl` + `look.wgsl` (string concatenation at build time, `concat!(include_str!(..), include_str!(..))`);
   `look.wgsl` adds bindings from 10 up and its own entry points, and calls plain's functions (`place`, `drawn`,
   `field_at`, `face_at`, `amount_filled`, `absorbed`, `ladder_at`, `side`, `srgb_of` is not used). Plain's uniform
   `U` is written by `Plain::numbers` as today (with `PlainLook::default()`; only its view, grid, splat, widths and
   depth parts are read by the look passes). `plain.rs` makes `Depth`, `texture`, `numbers`, the ladder half of
   `record` (`record_ladder`) and the field buffer `pub(crate)`; nothing in plain's own passes changes.
2. **One entry point, as `Plain`'s:** `LookDraw::draw_at(&mut self, drops: &DropBuffers, view: &View, look: &Look,
   ahead: f32)`, `draw` (ahead 0), `set_field`, `output()`, `output_view()`, `read()` (rgba8 sRGB, rows packed), so
   the three places swap one type for the other. `output()` is the Renderer's output texture (`Rgba8Unorm`).
3. **HDR in, the shared finish out.** The compose pass writes linear light into the Renderer's HDR target
   (`Renderer::hdr_target()`, new, the `Rgba16Float` picture `render_scene` draws into), then
   `Renderer::finish_picture(device, queue, encoder, frame, given)` (new) writes the globals for `frame` and runs
   the same finish as a Balls frame: picture effects, bloom, Things silhouettes, grade, composite. `frame` is a
   `bpe_gfx::Frame` with only `width`, `height`, `video`, `time`, `bloom`, `exposure`, `effects` and `grade` filled
   (the rest empty), built by `Look::frame()`. `given: GivenPicture { light: Option<(view, view)>, things:
   Option<..> }` replaces what `finish` reads from the ball lighting and the ball silhouettes; in 6a both are `None`
   (Effects and Grade stay greyed until 6c). `finish` becomes `finish_with(.., inputs)` where the Balls call passes
   exactly what it computes today (`light_pictures(..)` and `draw_silhouettes(..)`), so Balls pictures are unchanged.
4. **Units of light.** Light is linear, 1 is "white light": a surface of albedo `a` under light 1 shows `a`.
   Ambient 1 with no lamps draws every albedo as itself, which is the plain look's flat light. **The Light card's
   Ambient defaults to 1** (was 0.3), so a Plain scene stays evenly lit; each look sets its own.
5. **Quality** (Light card): Draft, Good, Best set the light map's texel (8, 4, 2 world px), the spread light's
   base texel (32, 16, 8 px) and the march's finest step (the light texel). **The editor draws Draft while a slider
   or a drag is held** (`studio.hold_runs`), and its Quality otherwise; the export draws the card's Quality.
6. **World grids.** The light map covers the view's world box clipped to the course, one texel out each side,
   snapped to its texel grid from the course's top left, at most 2048 texels a side (the texel doubles until it
   fits). The density, glow and owner fields are on the layout's nodes (one a cell, as the ladder). The spread light
   covers the whole course. So the light at a world point is the same in the stage's whole-course picture and in
   the export's shot when both are drawn at one Quality; the stage at a coarse zoom may use a bigger texel only when
   the course is too large for 2048 texels a side (16 screens at Best).

### The liquid's own pictures (picture space)

7. **Look splats** (`vs_look_splat`, `fs_look_splat`), every drop in the buffers' order with additive blending,
   into six `Rgba16Float` pictures: `A0 = (Σk·|v|, Σk·|v|², Σk_air, Σk)` (liquid only in `.a`, so plain's
   `amount_filled` reads it as plain's `amount0.a`), `A1 = (Σk·vx, Σk·vy, Σk_lone, 0)` and `W0..W3` (liquid `n`'s
   weight in channel `n % 4` of `W[n / 4]`). Speeds in world px a second. Air drops add only to `Σk_air` (they are
   not liquid; 6b draws bubbles from it). Per-pixel look parameters are mixed from the 16 weights against a uniform
   array of liquids, so a seam between liquids blends every setting, not only colour.
8. **Smoothing** sets the splat's radius per liquid: `RADIUS × (0.6 + 0.8 × Smoothing)` (Smoothing 0.5 is plain's
   0.9 cells; 0 shows every drop, 1 is soft blobs), its peak scaled so a body at rest still sums to 1, and never
   under `LEAST_R` picture px. The face fill in the compose uses the pixel's mixed radius.
9. **Streaks** stretch each splat along its drop's speed by `Streaks × |v| × (1/120 s)` (half a 1/60 s frame: a
   film camera's half shutter), at most 4 cells, its peak divided by the stretch so a stream still sums to about 1.
   This closes the faint lines inside a tap's stream that part 8 saw half a frame ahead, and makes fast thin water
   read as a stream, not beads. Streaks 0 is round splats.

### The course's fields (world space, on the layout's nodes)

10. **Density and glow** (`fs_look_nodes`, the node splat as plain's `fs_world` but with values): two node
    pictures, `D = (Σk σr, Σk σg, Σk σb, Σk)` and `G = (Σk glow rgb, 0)`, each with a mip chain made by 2 x 2
    averages (`fs_half`). A liquid's light extinction per world px is `σc = (1 + 2 (1 − colourc)) × (1 + 20 ×
    Solid) / max(Clarity, 1)` (`colourc` its linear Colour): light through Clarity px of it keeps about a third of
    its own colour and less of the others, so lamp light under water goes the water's colour and paint stops it.
    Its glow emission is `Gives off light × Glow colour × glow factor`, the factor 1 for Always and `smoothstep(0,
    1500 px/s, speed)` for Speed, taken per drop.
11. **Owners and materials** are read from plain's field buffer (distances then owners). A uniform array gives each
    owner (0 the Rock, 1 to 64 pieces and walls) its material kind and base colour; past 64 the Rock's (as
    `MOST_COLOURED`).

### Lamp light (world space)

12. **The light map** (`cs_light`, one invocation a texel, compute): for each texel middle `p` and each Lamp, the
    lamp's light at `p` and the light-weighted mean direction to the lamps, in two `Rgba16Float` textures
    `L = (rgb, 0)` and `Ldir = (x, y, 0, 0)`. Up to 16 lamps (`MOST_LAMPS`); past that the stage says so.
    - **Spot**: from its place, cone round its Aim of half-angle Spread / 2 with an edge `max(2°, Softness ×
      half)` wide; 360° is all round.
    - **Strip**: a line of its Length through its place, across its Aim, shining to the Aim's side only (a cosine
      to the strip's facing); its light at `p` is the light of the nearest point of the line.
    - **Sun**: everywhere, from its Aim (its place is not used), no falloff; it is marched from `p` back against
      the Aim to the course's edge.
    - **Falloff** for Spot and Strip: `REACH / (REACH + d)` with `REACH` 600 px (a 2D lamp's gentle spread).
    - Light = `Brightness × Colour (linear) × falloff × cone × visibility × transmittance × caustics`.
13. **Shadows**: from `p` toward the lamp the solid field is sphere-traced (step `max(|d|, light texel)`, at most
    48 px since the field holds ±48 px) with the soft-shadow estimate `vis = min(vis, k × d / t)` (`k` from
    Softness: `mix(32, 2, Softness)`, widened for a strip by its seen length). A step inside a **Glass** solid
    (by the nearest node's owner) doesn't stop the light: it is tinted by the glass's colour, `colour^(len / 64
    px)`. With Shadows off, solids don't stop its light (the liquid still dims it). A texel inside a solid gets no
    light (nothing reads it: decision 21).
14. **Through the liquid**: along the same march, `τ += D.rgb × step` read from the density's mip level whose
    texel is nearest the step; in air and rock the march skips ahead by the field and by a coarse level of the
    liquid's amount (a level-4 texel of no liquid is skipped whole). Transmittance `exp(−τ)`.
15. **Caustics**: along the march, the surface crossing nearest `p` (where the amount passes the drawing's level)
    is kept with the length `t` from it to `p`. There the surface's normal and curvature `κ` come from the amount
    at the density's level 1 (central and second differences, so the draft's grain doesn't speckle it), plus the
    Ripples' curvature (decision 26). The light is multiplied by `C = 1 / max(|1 − t κ (η − 1) / η|, 1 / 6)` with
    `η` 1.33 (a curved surface focusing light below it, as a lens of focal length `η R / (η − 1)`), then by
    `mix(1, C, Caustics)` clamped to 0 to 8. Rays never bend in the march itself (the light goes straight to the
    lamp): what bunching does to brightness is the caustic, and where light goes after it bends is not followed.
    That keeps one march a texel, no flicker and no splatting.
16. **Shafts**: the light map is the light in the air and the water, so the compose adds
    `Shafts × L × (0.12 in air, 0.6 × the liquid's colour in liquid)` to the picture: beams cut by the rock and
    bright streaks under a wavy surface.

### Spread light: glow and bounce (world space, the whole course)

17. **One pyramid for both.** At the spread light's base texel over the whole course, the light emitted is the
    liquid's glow (`G` averaged into the texel) plus `Bounce × (lamp light × base colour)` of the solids' faces and
    of the liquid's body in it; the lamp light for bounce is the light map worked out again at the base texel over
    the whole course (cheap: one screen at 16 px is 68 x 120 texels), so bounce from off-shot rock is the same in
    every view. Openness `O` is 1 in open space, 0 in a solid (Glass 0.7). Both are halved to a texel or two across
    the course's narrow side. From the coarsest level down, each level's spread light is the coarser one
    upsampled plus its own emission, then two 3 x 3 passes that never cross a closed texel (`S = O × (E + w ×
    Σ neighbours S)`), and the levels' shares are summed. Thin rock under a level's texel leaks a little light at
    the coarse levels (known, small); near the rock, where it shows, the fine levels block it.

### What is drawn (picture space, `fs_look`)

18. **Order** (back to front): the back, the paper's cut-out shadow on it, the liquid (see-through over the bent
    back), shafts, the solids, then outlines; 6b adds the extras over the liquid. Everything is lit by
    `Ambient × ao + L + S` at its point (L the light map, S the spread light, both bilinear).
19. **The Back**: Darker rock is the Rock's own material albedo (its texture) times `1 − Darker`, with no rounded
    edge; Colour; Gradient across the course along Direction (world-anchored); Picture is the PNG cover-fitted to
    the course and centred (read once per path by `LookDraw`, `png` crate, kept on the card; a missing or unreadable
    file shows Darker rock and the stage says so). Ambient occlusion: the back darkens by up to 35 % within 24 px
    of a solid (the rock in front casting a soft contact shadow), at every Ambient.
20. **Materials** (albedo, all noise a hash of the world point, scaled by Texture size):
    - **Rock**: Colour and Second colour mixed by layered value noise (veins and patches) with a fine grain.
    - **Glass**: frosted: the back seen through it (four fixed offsets of Texture size / 20 averaged, so it is
      blurred) tinted by Colour, mixed 55 % with Colour lit; its edge picks up a bright line from the lamps. It lets
      tinted light through (decision 13).
    - **Paper**: Colour with a fine fibre grain toward Second colour, and a **cut-out shadow**: the back (and the
      liquid in front of the back) darkens where the field moved by `(0.5, 0.7) × Depth` px is inside the paper,
      softened over Depth / 2.
    - **Basalt**: Colour, with a network of cracks (cell edges of a hashed grid) in Second colour, and glossy
      (its Roughness default stays the card's; the looks set it low).
    - **Flat**: Colour only: no texture, no rounded edge.
21. **Solids are lit at their face.** A solid pixel takes the light of the open point just outside its nearest face
    (`p + n̂ (|d| + 1.5 light texels)`, `n̂` the field's slope), fading into the solid as `exp(−|d| / spill)` with
    `spill = 2 × max(Depth, 8 px)`, so rock is lit round the caves the lamps light and dark deep in the mass; plus
    Ambient everywhere. **Depth** rounds the edge: within Depth px of the face the normal tilts from facing you to
    facing out along `n̂`; lamps light it by `N · L3` (`L3` the mean lamp direction tilted 0.6 toward you) and
    **Roughness** gives a highlight `(1 − r) × (N · H)^mix(80, 4, r)`. With no lamps the tilted edge is darker
    under Ambient (the plain look's darker rim, now from the shape). **Outline**: a line of Outline px just inside
    the face in Outline colour.
22. **The liquid's body** at a pixel (weights mix every liquid's settings):
    - **Colour from**: Its own (Colour); Speed (the Gradient at `smoothstep(0, 1500 px/s, mean speed)`); Depth (the
      Gradient at the ladder's absorbed share). Then `mix(own, Deep colour, absorbed)` as plain, except for Depth.
    - **Lit**: albedo × (Ambient × the ladder's light left + L + S). Deep water is dark under a lamp because L
      already lost its light on the way down.
    - **See-through**: clear liquid shows what is behind: `trans = (1 − Solid) × exp(−TANK / Clarity) ×
      (1 − absorbed / 2)` with `TANK` 100 px (the tank's depth into the screen), the back seen through it tinted by
      `Colour^(TANK / Clarity)`. Clarity 0 is opaque.
    - **Bend**: the back seen through it is moved by `−n̂ × Bend × edge` picture px (`n̂` the outward slope of the
      amount smoothed over 2 px, `edge` 1 at the surface fading over 40 world px inside), plus the Ripples' slope.
      **Rainbow split** moves red by `1 − 0.15 × split` and blue by `1 + 0.15 × split` of that.
    - **Solid** (paint): opaque, its albedo lit by `N · L3` with the rounded normal (decision 23), so paint is a
      glossy body.
23. **The surface**: a 3D normal from the amount's slope, rounded over `ROUND` (6 world px, at least 1.5 picture px)
    in from the outline: `N = normalize(n̂ × tilt, 1 − tilt + 0.15)`. **Shine**: `Shine × L × (N · H)^p` with `p =
    mix(400, 8, Shine size)` and `H` from `L3` and the eye `(0, 0, 1)` (plus a faint sky term `0.2 × Ambient` from
    above, so Shine shows without lamps). **Edge sheen**: `Edge sheen × (1 − N.z)² × (Ambient + L)`, which
    replaces plain's fixed lighter rim. **Thin film**: where the surface turns (`1 − N.z`) and in a slow hash noise,
    an iridescent tint `0.5 + 0.5 cos(2π (t + (0, 1/3, 2/3)))` mixed by Thin film.
24. **Glow**: the body adds its emission (decision 10) on top, in HDR, so bloom spreads it; the spread light lights
    the rock and back round it (decision 17).
25. **Drawn**: **Bands** quantise the body's light (its luminance over its albedo's) into `Bands` steps, each step
    edge softened by Band softness; **Outline** is a line of Outline px centred on the liquid's edge in Outline
    colour (the liquid's alpha reaches Outline / 2 past its edge for it).
26. **Ripples** are a fixed sum of five travelling waves along the world's x (wavelengths 18 to 70 px, speeds from
    the water-wave relation, phases from a hash), their height fading over 40 px down from the air; their slope tilts
    `N` and adds to Bend, their curvature adds to the caustics' `κ` (decision 15), all on water time. Ripples 0 is
    the water's own surface only.
27. **Removed until their parts**: the Crust group (its heat and the time it takes to form come with heat, Later),
    Colour from Heat and Age and Glow from Heat (no heat; a drop's age isn't kept), and the Mixing shows group
    (colour mixing is part 4). Their settings are taken off the Look card now and come back with their parts (each
    hand-over says so); a saved scene with them still opens (an unknown key is dropped, as for every card).

### Cards, prints, the editor (6a)

28. **Look cards move to their own file** so a look change never remakes a final: `WaterLook` and its enums, the
    Light card, the Back card and the Lamp card move to `bpe-kit/src/modules/water/look.rs` (re-exported from
    `modules::water` as today); `plain_look` and the new `look_of` live in `bpe-kit/src/things/water/look.rs`. The
    kit's `build.rs` prints every file under its two roots **except** files named `look.rs` or `looks.rs`;
    `bpe-water`'s prints `src/drops`, `src/take`, `src/drop.rs`, `src/solid.rs` and `src/lib.rs` (not `src/draw`,
    `src/shaders`, `src/probe.rs`, `src/scenes.rs`). The Material card stays in `solids.rs` (Wetting is physics).
    This build still remakes every final once (the prints change).
29. **`look_of(scene, resolved, time) -> Look`** (bpe-kit) reads at `time` (water seconds): each enabled Liquid's
    Look (scene order, at most 16), the Course's Rock and each enabled Piece's and Wall's Material (in the order the
    field numbers them, at most 64), the Back, the Light card, each enabled Lamp (its place from `position`'s
    keyframes, its card at `time`), the Picture card's Bloom and Exposure, and in 6c the Effects and Grade. It is
    cheap (no file reading: the Back picture is a path). `Look` is `Clone + PartialEq`, so the stage redraws only
    when it changes.
30. **Lamp markers** on the stage: a Spot's cone (its Spread, 120 screen points long), a Strip's line (its Length)
    with a tick toward its Aim, a Sun's arrow at the course's top edge; picked and dragged as any marker (Select).
    No aim handle yet (Aim in the inspector).
31. **Speed**: the compose is one full-picture pass; the light map is one compute invocation a texel. Budget
    (estimate): Good with two lamps at 1080 x 1920, 1.4 million drops, within 10 ms on the RX 9070 XT. Measured by
    `water_look_speed` in `bpe-kit tests/water_final.rs` (run by the card job's timing step; it prints lines
    starting "water final: look" so the step's filter keeps them).

### Extras and wet marks (6b)

32. **Extras are drawn from the moment**, never remembered: what the physics reports at a moment is in the drops
    (speeds, air drops, lone drops), so the extras are a function of the drops and the water time, and stay the
    same every time a moment is drawn. Settings whose meaning needs memory are taken off: **Foam lasts** and **Mist
    lasts** go; **Bubbles last** is the fizz's life (below) and **Sparks last** the length of a spark's trail in
    seconds of its flight.
33. **Bubbles**: (a) the physics' trapped air (`Σk_air`) is drawn as bubbles: its outline a bright thin rim of
    Bubble colour, its inside the body lightened a little, small pockets round; with Bubbles 0 trapped air stays the
    hole it is today. (b) **Fizz**: inside liquid, small round bubbles of Bubble size rise at 120 px/s from a
    world-fixed hashed lattice (one spot per 3 Bubble sizes), each living Bubbles last, fading in and out, only
    where the liquid is deep enough to hold them (amount over the level for its whole size), their count by
    Bubbles.
34. **Foam**: where the liquid churns (the spread of its speeds, `E|v|² − |Ev|²`, over 30 px), where trapped air
    meets it, and along a fast front, a cellular white texture of Foam size in Foam colour that moves with the
    water by a two-phase flow map (the pattern pushed by the mean speed over a 1 s cycle, two copies half a cycle
    apart blended), near the surface more than deep down.
35. **Mist**: lone drops (little liquid round them, `Σk_lone`) moving over 600 px/s draw as soft dots of Mist size
    and a faint haze round splashes, in Mist colour.
36. **Sparks**: lone or edge drops over 900 px/s draw as thin streaks behind them along their speed, `Sparks last ×
    |v|` long (at most 160 px), bright (Spark colour × 3, HDR) at the head and fading to the tail. A sprite pass
    over the drops (`vs_extra`, discarding drops that don't qualify).
37. **Wet marks** need memory, so the water keeps it: a **wet field** on the layout's nodes holding the frame each
    node was first touched by liquid (a `u32` a node; `u32::MAX` never). `cs_wet` marks the four nodes round each
    liquid drop with `atomicMin` (whole numbers, so the order doesn't matter: the same field every time).
    - The **draft** marks it after every frame it steps (`DraftJob`); going back keeps it (a node first wet after
      the playhead reads dry there), a restart clears it.
    - The **final**: the make marks it after every frame on its own card and writes it next to each chunk
      (`wet-000012.bin`: lz4 of one `u16` a node, frames since the take's start, `0xFFFF` never), so the field at
      any moment of chunk `c` is that file read against the moment's frame. A partial remake starts from the old
      take's file of the chunk before its restart point with every entry at or past the restart frame cleared.
      `TAKE_FORMAT` goes up by one (every older take is made again).
    - **Drawn**: a solid within 6 px of its face where its node is wet darkens by `Wet marks × 45 %` and turns a
      little glossier (Roughness down by half the mark).

### The six looks and the shared finish (6c)

38. **`looks.rs`** (bpe-kit `modules/water/looks.rs`): one table, one row a look: settings for every Liquid's Look
    card, for the Rock and every Material (never Wetting), the Back, the Light card, the Lamps (kind, place as a
    share of the camera's shot, card), the Picture card (Bloom, Exposure), the Effects stack and the Grade card.
    **`apply_look(scene, reg, look, keep: Keep { colours, lamps })`** writes the row as one edit: Keep my colours
    leaves every colour, gradient and Glow colour setting as it is; Keep my lamps leaves the Lamps (else every Lamp
    is removed and the row's added). Plain is a row too (the cards' defaults). Adding a seventh look is adding a row.
39. **A new Water scene with a look** gets the row and a Liquid named for it (Water, Neon, Paint, Lava, ...), so the
    first Pour pours it. The Water card's Starting look setting goes (nothing is remembered); the chooser's tiles say
    what each look is now (Molten: "Lava that glows and throws sparks, lighting the dark rock round it."; Paint:
    "Thick glossy paint on paper cut-outs.").
40. **Use a look…**: a menu button on the Water stage's strip with the seven looks and the two switches; one undo
    step; the status line names the look.
41. **The rows** (values tuned with the full-size look check; these are the starting points):

| Look | Liquid | Solids and back | Light | Finish |
| --- | --- | --- | --- | --- |
| Lamp-lit rock | `#4fb4e8`, Deep `#06223a`, Clarity 320, Bend 16, Shine 0.6, Edge sheen 0.4, Ripples 0.5, Foam 0.25, Bubbles 0.2 | Rock `#7a5c44` / `#4a3626`, Texture 160, Depth 14, Roughness 0.7, Wet marks 0.6; Back Darker rock 0.65 | two warm Spots `#ffd2a0` 1.6 from the shot's upper left and right, Spread 70, Softness 0.4; Ambient 0.12, Bounce 0.4, Caustics 1.2, Shafts 0.3 | Bloom 0.25 |
| Glass lab | `#35d0a8`, Deep `#0c5a6a`, Clarity 450, Shine 0.9, Shine size 0.12, Edge sheen 0.6, Bubbles 0.6 | Glass `#c9d6e2`, Depth 8, Roughness 0.1; Back Colour `#eef3f7` | Sun from above `#ffffff` 0.6, Softness 0.8; Ambient 0.9, Caustics 0.6 | Bloom 0.1 |
| Neon in the dark | Colour from Speed `#6a1bff` → `#ff3dc8` → white, Clarity 120, Gives off light 2.5, Glow colour `#ff3dc8`, Glow from Speed | Flat `#12101e`; Back Colour `#07060c` | no lamps; Ambient 0.05, Bounce 0.5 | Bloom 0.8 |
| Paint on paper | `#e84a3a`, Solid 1, Clarity 0, Shine 0.7, Shine size 0.35, Streaks 0.5 | Paper `#efe7d6` / `#d9cdb4`, Texture 40, Depth 10; Back Darker rock 0.08 | Sun `#fff4e0` 0.5 from the upper left, Softness 1; Ambient 0.85, Caustics 0 | none |
| Molten | Colour from Speed `#6e0e00` → `#ff5a10` → `#ffd060`, Deep `#3a0800`, Clarity 40, Gives off light 2, Glow colour `#ff8a30`, Sparks 0.5 | Basalt `#241c1a` / `#0e0a09`, Roughness 0.25; Back Darker rock 0.7 | no lamps; Ambient 0.03, Bounce 0.6 | Bloom 0.6 |
| Cartoon | `#3aa0ff`, Clarity 300, Bands 3, Band softness 0.1, Outline 4 `#101018`, Shine 0.5, Foam 0.6 | Flat `#ffd24a`, Outline 4 `#101018`; Back Colour `#8fd3ff` | no lamps; Ambient 1, Caustics 0, Shafts 0 | none |

42. **The shared finish in Water scenes**: Effects and the Grade card are offered on the Water scene's root, and
    `look_of` reads them into `Look.finish` (`EffectLayer::read` over the Shader slot's layers, as `EffectsDraw`;
    `modules::grade::stack(slot, None)`). Every built-in Effect is marked (`effects::water_ok`): the ones that work
    on the whole picture are offered; the ones that need balls, hits or things (Ball worlds, Fractal light, Gravity
    lens, Hue storm, Op-art lens, Shockwaves, Light ripples, Hit glow, Lava, Rainbow flow) are hidden from a Water
    scene's Add effect menu. **Takes** Light only and Glow only read the water's light pictures: the picture before
    light (albedo under Ambient 1) and the lit colour, written by the compose to two more HDR targets only when an
    effect takes them.
43. **Grade masks**: Linear, Radial, Brush, Colour Range and Luminance Range work on any picture. **Things** in a
    Water scene offers Only: **Liquids**, **Solids**, **Back**, **Extras**, **One thing** (a Liquid, Piece, Wall or
    the Course) in place of Balls, Shapes, Text, Particles, Background. The compose writes a coverage picture per
    selection (red the chosen, alpha anything but the back) in the same `Rgba8Unorm` layout the ball silhouettes
    use, and the renderer's jump flood (`Silhouettes::flood_given`, new) makes the things field from it, so Expand
    and Feather work as in Balls.
44. **Grade mode and Shot only**: Grade mode opens on a Water scene. Its picture is the camera's shot drawn by
    `LookDraw` with the finish (as the export), so scopes and masks see what the video shows. The Water stage gains
    a **Whole course | Shot** switch: Shot shows the camera's shot with the whole finish; Whole course shows the
    course with bloom and exposure but no Effects or Grade (they are made for the shot's shape).

## Edge cases

- No Liquid, no drops: the back and the solids are drawn and lit; the liquid passes draw nothing.
- No lamps: the light map passes are skipped (L is 0); Shafts and Caustics then do nothing.
- A Lamp inside rock with Shadows on lights nothing; with Shadows off it lights round it.
- A Spot of Spread 360: no cone. Brightness 0: skipped.
- Over 16 Lamps: the first 16 in the tree's order; the stage says "Only 16 lamps light a Water scene".
- Clarity 0: opaque, depth-flat (as plain); σ uses Clarity at least 1 px.
- Bands 0: smooth. Outline 0: none. Smoothing 0 or 1, Streaks 0: as decisions 8, 9.
- The Back's Picture missing or unreadable: Darker rock, and the stage's strip says "The back's picture can't be
  read". A huge picture is scaled down to 4096 a side when read.
- A picture over the card's largest texture: clamped (as plain), told once.
- Course bigger than 2048 light texels a side: the texel doubles (decision 6).
- Draft while held: the light jumps from Draft to the Quality when the slider is let go (one redraw).
- A take from before this part: made again (prints and `TAKE_FORMAT`, 6b); no old take is read.

## Not to do

- Don't change `plain.wgsl`, `Plain`'s passes or `golden-plain.txt`.
- Don't follow bent rays (no photon splatting, no atomics on floats); caustics come from the surface's curvature.
- Don't keep anything from the frame drawn before (no temporal smoothing, no particle state); wet marks live with
  the water (draft job, make), not the drawing.
- Don't put any look setting into a print.
- Don't build heat, crust, colour mixing or drying here.
- Don't run unrelated tests; don't render demo videos before 6c (then only the stills and renders Aedan asked for).

## 6a: the renderer (light, materials, back, the liquid's look)

### Task 1: prints and the cards' files

**Files:** `crates/bpe-water/build.rs`, `crates/bpe-kit/build.rs`, `crates/bpe-kit/src/modules/water/{look.rs
(new), liquid.rs, scene.rs, places.rs, mod.rs}`, `crates/bpe-kit/src/things/water/{look.rs (new), draft.rs,
mod.rs}`.

- [x] `bpe-water/build.rs`: `ROOTS` = `src/drops`, `src/take`, `src/drop.rs`, `src/solid.rs`, `src/lib.rs` (a
  file root prints the file). `bpe-kit/build.rs`: skip files named `look.rs` or `looks.rs`.
- [x] Move `WaterLook`, `ColourFrom`, `GlowFrom` (and drop `MixShows`), `WaterLightCard`, `LightQuality`,
  `BackCard`, `BackShows`, `LampCard`, `LampKind` into `modules/water/look.rs`; re-export in `mod.rs` as before.
  Remove the Crust group, Mixing shows group, `ColourFrom::{Heat, Age}`, `GlowFrom::Heat` (decision 27); Light
  card Ambient default 1 (decision 4). Remove Foam lasts and Mist lasts now (decision 32), so 6b doesn't touch the
  card again. Fix the tests that name them.
- [x] Move `plain_look` to `things/water/look.rs` (re-exported from `draft` for its callers).
- [x] Verify: `cargo test -p bpe-kit --lib things::water modules::water` pass; a print test: change a comment in
  `look.rs` in a scratch build and the engine print is unchanged (by hand once, not a test).

### Task 2: `bpe-gfx`: finish a picture drawn elsewhere

**Files:** `crates/bpe-gfx/src/renderer/{scene.rs, mod.rs}`, test in `crates/bpe-gfx/src/output_tests.rs`.

- [x] `Renderer::hdr_target(&self) -> &wgpu::TextureView` (the HDR picture; `HDR_FORMAT`).
- [x] Split `finish` into `finish_with(device, queue, encoder, frame, scened, inputs: FinishInputs)` with
  `FinishInputs { light: Option<(TextureView, TextureView)>, things: Things }` (`Things::Frame` draws the frame's
  silhouettes as today, `Things::None`, and in 6c `Things::Given(..)`); `finish` computes the Balls inputs exactly
  as today and calls it.
- [x] `pub struct GivenPicture { pub light: Option<(TextureView, TextureView)> }` and `Renderer::finish_picture(
  device, queue, encoder, frame, given)`: writes the globals for `frame` (`Globals::new(frame, &frame.camera,
  width)`), forgets effect history when the frame's time goes back (as `render` does through the chain), runs
  `finish_with(.., Scened::default(), ..)`.
- [x] Test: a flat HDR picture of linear 0.5 grey drawn into `hdr_target` and finished with no effects, Bloom 0,
  Exposure 1 reads back as the same grey a Balls frame of that background reads (the composite's roll-off), and
  Bloom 1 spreads a bright dot. The Balls goldens run in CI unchanged.

### Task 3: `bpe-water`: the look drawing's inputs and passes

**Files:** `crates/bpe-water/src/draw/{mod.rs, plain.rs, look/mod.rs (new), look/inputs.rs (new),
look/passes.rs (new)}`, `crates/bpe-water/src/shaders/look.wgsl (new)`.

- [x] `look/inputs.rs`: `Look { liquids: Vec<LiquidLook>, rock: MaterialLook, solids: Vec<MaterialLook>, back:
  BackLook, light: LightLook, lamps: Vec<Lamp>, time: f32, finish: FinishLook }` and the parts (`LiquidLook` every
  Look card setting the drawing reads, colours sRGB 0..1 and gradients as 64 linear samples; `MaterialLook { kind:
  MaterialKind (own copy: Rock, Glass, Paper, Basalt, Flat), colour, second, texture, depth, roughness, outline,
  outline_colour, wet_marks }`; `BackLook { shows: Back::{DarkerRock(f32), Colour, Gradient(samples, angle),
  Picture(path)} }`; `LightLook { ambient, bounce, caustics, shafts, quality: Quality }`; `Lamp { kind, place,
  aim, colour, brightness, spread, length, softness, shadows }`; `FinishLook { bloom, exposure, effects:
  Vec<EffectCall>, grade: GradeStack }`), all `Clone + PartialEq + Debug`, `Default` the cards' defaults with Ambient
  1. `Look::plain(&PlainLook)` maps the plain inputs (for tests).
- [x] `look/mod.rs`: `LookGpu::new(device, queue) -> Arc<LookGpu>` (its `PlainGpu`, the module `plain.wgsl +
  look.wgsl`, pipelines: look splats, look nodes, half, light, spread (inject, down, up, smooth), compose; one
  gradient texture 64 x 16 `Rgba16Float`, one owner-material uniform), `LookDraw::new(gpu, field, rest)`,
  `set_field`, `draw`, `draw_at`, `output`, `output_view`, `read`, `draw_many` (timing). It holds its `Plain`, a
  `bpe_gfx::Renderer` sized to the last picture (made again on a size change), the six look pictures, the node
  pictures and their mips, the light maps (shot and whole course), the spread pyramid, the back picture's texture
  and path.
- [x] `shaders/look.wgsl`: `LU` (binding 10: liquids' look rows, lamps, light, materials by owner, back, the light
  map's box and texel, time), bindings 11 to 30 as the passes need; entry points `vs_look_splat`/`fs_look_splat`,
  `vs_look_nodes`/`fs_look_nodes`, `fs_half`, `cs_light`, `fs_spread_*`, `fs_look`. Hash noise (`hash2`, `vnoise`,
  `fbm`, `cells`) world-anchored.
- [x] Verify (`crates/bpe-water/tests/look.rs`, new, GPU, 270 x 480 at `View::screen(0.25)` unless said):
  1. the same drops draw the same picture to the bit, twice and from a second `LookDraw`;
  2. with the plain inputs (`Look::plain`) a pool is drawn where its drops are and the back and rock where they are
     (as plain's own test: pixel colours by region);
  3. a Spot lights open space in its cone and not outside it, and a wall between it and a point shadows the point
     (dark with Shadows on, lit with Shadows off);
  4. a lamp's light under a deep pool is dimmer than over it and bluer for blue water;
  5. caustics: under a pool with a hand-made wavy surface (`sets::wavy_pool`) the floor's brightest light is over
     1.5 times its mean with Caustics 2, and even (within 10 %) with Caustics 0;
  6. glow: glowing liquid lights rock beside it; rock behind a closed wall stays at its ambient;
  7. each Material draws: Flat is one colour across its inside, Rock and Paper vary, Glass shows the back's gradient
     through it, Outline draws its colour along the face;
  8. the Back: Colour, Gradient (left and right differ along Direction 0), Picture (a 2 x 2 PNG written to a temp
     file shows its four colours in the four quarters), a missing Picture draws Darker rock;
  9. Bend moves a striped back seen through liquid (a stripe's edge shifts) and Bend 0 doesn't;
  10. Shine: a lamp gives the surface facing it a highlight brighter than the body below it;
  11. Bands 3 gives at most 6 distinct lightness levels in a lit pool's body (3 bands, softened edges excluded);
  12. the light doesn't depend on the view: one world point drawn at `View::screen(0.25)` and in a zoomed view of
      its quarter has the same light map value (read back from the light map, `LookDraw::light_at(world)` test
      helper);
  13. Draft and Good differ little (mean pixel difference under 6/255) and both differ from no lamps.
  `cargo test -p bpe-water --test look -- --test-threads=2`.

### Task 4: `bpe-kit`: the cards into a `Look`

**Files:** `crates/bpe-kit/src/things/water/look.rs`, tests in `things/water/draft/tests.rs` (or `look.rs`'s own
`mod tests`).

- [x] `look_of(scene, resolved, time) -> Look` (decision 29); materials in the field's owner order (the same
  `scene.order` filter `SolidSetup` uses); lamps from `position` and `rotation`-free cards; gradients sampled 64
  times (`Gradient::sample`).
- [x] Tests: the Look reads every card (a scene with two liquids, a piece, a lamp, keyed Brightness: values at 0 and
  at 2 s); a lamp's keyed place; materials follow the solids' order; the Back's picture path; no look setting
  restarts the draft (`what_restarts_the_draft` extended with the Lamp, Light, Back and Look settings, which it
  already walks: check its counts); Ambient default 1.

### Task 5: the three places draw with the look

**Files:** `crates/bpe-studio/src/water_mode/{picture.rs, draft.rs, final_view.rs, stage.rs}`, `crates/bpe-cli/
src/water.rs`, `crates/bpe-kit/tests/water_draft.rs`, `water_final.rs`.

- [x] `Live.plain: Plain` becomes `Live.look: LookDraw` (draft job builds it with a `LookGpu`); `StagePicture::show`
  takes `&Look`; `Drawn.look: Look`; `FinalPicture` the same; `stage.rs` builds `look_of` (with Draft quality while
  `studio.hold_runs`).
- [x] `bpe-cli`: `Drawing { look: LookDraw, stored }`; `draw_moment` builds `look_of(scene, resolved, tau)`; export
  reads `drawing.look.output()`; `final_frame` reads `drawing.look.read()`.
- [x] Lamp markers (decision 30) in `stage.rs`'s `paint`.
- [x] `water_final_speed`'s neighbour `water_look_speed` (`#[ignore]`, release): one screen at 2 px, 1.4M drops
  from the take, Good, two lamps, prints `water final: look N ms a frame at 1080 x 1920 (Draft A ms, Best B ms)`.
- [x] Verify: `cargo test -p bpe-studio water_mode`, `cargo test -p bpe-cli water`, `cargo test -p bpe-kit --test
  water_draft --test water_final` (the tests that compare the take's drawing with the live body's draw both with
  the look now).

### Task 6: look at it, notes, review, merge

- [x] Full-size look check: `water-look-*` scenes in `full_size_look.rs` (the bench and the pieces set at 3 px
  under two lamps; each Material; the Back's four kinds; caustics in a wavy pool; glow in the dark), run with
  `LOOK_ONLY=water-look`, open every picture and its 1:1 crops: no jagged edges, light leaks, banding, seams, or
  anything that doesn't make sense. Fix and look again.
- [x] `docs/notes/water.md` "Part 6a (as built)"; this plan's "As built" for 6a.
- [x] fmt, clippy, the focused tests; PR (draft), CI, one Opus review, fixes, merge.

## 6b: extras and wet marks

### Task 7: extras

**Files:** `crates/bpe-water/src/draw/look/{mod.rs, inputs.rs}`, `shaders/look.wgsl` (`vs_extra`/`fs_extra`, the
compose's bubbles and foam), `crates/bpe-kit/src/modules/water/look.rs` (Bubbles last's and Sparks last's help),
`things/water/look.rs`.

- [x] Bubbles (decision 33), foam (34), mist (35), sparks (36).
- [x] Verify (`tests/look.rs`): trapped air draws a rim with Bubbles on and a hole with Bubbles 0; fizz shows inside a
  still pool and none in the air; churning water (a set with mixed speeds) gets foam and still water none; fast lone
  drops draw sparks and mist, slow ones don't; the same moment draws the same extras twice; fizz moves up between two
  water times.

### Task 8: wet marks

**Files:** `crates/bpe-water/src/draw/wet.rs` (new: `WetField { buffer, layout }`, `mark(encoder, drops, frame)`,
`clear`, `to_bytes`/`from_bytes`, `upload`), `shaders/wet.wgsl` (new, `cs_wet`), `crates/bpe-water/src/take/
{mod.rs, chunk.rs}` (`TAKE_FORMAT` + 1, wet files), `crates/bpe-kit/src/things/water/{make.rs, finals.rs}`,
`crates/bpe-studio/src/water_mode/{draft.rs, final_view.rs}`, `crates/bpe-cli/src/water.rs` (`TakeReader::wet`).

- [x] Decision 37 for the draft, the make, partial remakes and drawing.
- [x] Verify: a drop marks its four nodes with its frame and a later frame never moves them; the field is the same
  whatever order the drops are in; a take's wet file reads back as written; a partial remake's wet field equals a
  whole make's (with the make's existing "same as one go" test); the draft after going back draws marks only where
  the water had been by then; a wet solid is darker than a dry one with Wet marks 1 and the same with 0.

### Task 9: look at it, notes, review, merge

- [x] Full-size look check `water-extras-*` (a tap into a pool with foam, bubbles and mist; sparks on fast streams;
  wet marks on a course the water has run down), notes "Part 6b (as built)", PR, CI, review, merge.

## 6c: the six looks and the shared finish

### Task 10: the looks table

**Files:** `crates/bpe-kit/src/modules/water/looks.rs` (new), `things/water/mod.rs` (`new_water_scene`, Starting
look removed from `WaterWorld`), `crates/bpe-studio/src/water_mode/{chooser.rs, stage.rs}`.

- [ ] Decisions 38 to 41. Verify: each look writes its row (every setting it names, read back); Keep my colours and
  Keep my lamps; applying is one undo step; a new scene with a look has its Liquid and lamps; Wetting is never
  written; the chooser's words.

### Task 11: Effects, Grade and masks in Water scenes

**Files:** `crates/bpe-gfx/src/effects/mod.rs` (`water_ok`), `renderer/silhouettes.rs` (`flood_given`),
`renderer/scene.rs`, `crates/bpe-water/src/draw/look/*` (light pictures, coverage), `crates/bpe-kit/src/things/
water/look.rs`, `modules/grade/masks.rs` (Water Only kinds), `crates/bpe-studio/src/app/*` (Effects and Grade
offered, Grade mode opens, `a_water_scenes_toolbar_says_so_and_greys_grade` turned round), `water_mode/stage.rs`
(Whole course | Shot).

- [ ] Decisions 42 to 44. Verify: a water scene's Kaleidoscope mirrors the picture; a Grade's Whole picture panel
  changes it; a Things mask on Liquids changes only the liquid (and Expand grows it); Light only reads the water's
  light; the hidden effects aren't in a Water scene's Add effect menu; Grade mode opens and its scopes read the
  shot; the Balls goldens don't move (CI).

### Task 12: pinned looks, look at them, the stills and renders

- [ ] `crates/bpe-kit/tests/water_looks_pinned.rs`: one hand-built course (`sets`-style, no physics: a carved cave,
  a pool, a stream, two pieces) drawn in each of the seven looks at 270 x 480, tiles pinned to the owner's card
  (`golden-water-looks.txt`) through the "Refresh GPU pictures" workflow.
- [ ] Full-size look check `water-looks-*` (each look on a made final), looked at closely.
- [ ] Notes "Part 6c (as built)", this plan's "As built", PR, CI, review, merge.
- [ ] Aedan's ask: a still and a short render of each look (one demo course, a final, each look applied), into
  `/mnt/project-files/ballphysics/water-mode/part6-looks/`, sent in one message without waiting for him.
- [ ] Hand-over for the next part (4, 5 and 4b side by side: which comes first is the coordinator's), memory, message
  the coordinator.

## As built

### 6a

Built as planned (decisions 1 to 31), with these changes found while building against the tests:

- **Spread light is cone traced, not the pyramid of decision 17.** The 3 x 3 passes either spread too little or,
  with growth, leaked through thin rock into sealed chambers. Now the emission (glow, plus Bounce of the lit faces
  and body) is put on a grid over the whole course at 4 times the light texel with 6 mips, and each texel gathers
  it along 24 cones (48 steps, tan 0.13), reading coarser mips further out, each step stopped by the solid field
  (Glass passes 70 %). Gain 2. A liquid's glow doesn't light itself (it is added once, in the body).
- **The light map has three targets**: the light through the liquid, the mean lamp way, and the light with no
  liquid in the way. Paint and the faces in front of the water are lit by the third (paint under a lamp was black).
- **The body is lit by `albedo × (Ambient + L + S)`** where the plan said `Ambient × light left`: Ambient 1 keeps
  every colour as it is however deep, and depth darkens through Deep colour and the light map.
- **Bend moves the back by world px** (`(−slope × edge + ripple slope) × Bend`) along the slope of a surface smoothed
  from the depth ladder over 24 px; the amount's own slope was too grainy to bend anything visible.
- **Sun**: marched to the course's edge, so a closed course's rock ceiling stops it unless Shadows is off (6c's
  Glass lab turns Shadows off).
- **Lamps have no place keyframes** (Water things have no place settings yet): `look_of` reads `transform.position`.
- **Speed**: `water_look_speed` is folded into `water_final_speed` (the same take; it prints `water final: look ..`
  for Good, Draft and Best under two Spots), so the card job's timing step measures it.
- `LookDraw::renew` keeps the renderer and pictures across bodies (a new `Renderer` per physics edit would rebuild
  every Balls pipeline); `bpe-water/tests/look.rs` holds the 13 tests.
- **What the full-size look check found** (all in `look.wgsl`):
  - *The cloud's card reads a texture's size wrong when the level differs from pixel to pixel*
    (`textureDimensions(t, k)` with `k` worked out per pixel returned another level's size for some pixels, so
    `load0` read nothing there and the underwater light had dark ticks). Every level's size is now worked out from
    level 0's (`size_at`), and every read across levels is a loop over the level numbers with each one's share
    (`level_share`), as plain's ladder does.
  - *Caustics speckled and beaded.* The curvature is read at the level a deep point's blurred view spans (its depth
    times `0.02 + 0.2 × Softness`, never under 12 px), mixed between levels, the ripples' curvature blurred per wave;
    extinction is read at level 1 at least; the surface is the first point out of the liquid at about 12 px
    (`surface_level`), found by bisection, and a march that starts in a bubble deep down is still lit through the
    surface (`been_in`). A focus line much thinner than a light texel beaded where it slanted across the texels, so
    each texel takes the focusing's average over its own span of `1 − t κ (η − 1)/η` (the closed form of
    `1/max(|x|, 1/6)`'s sum), from the curvature one texel along the surface and its own depth.
  - *A grain in brightly lit water*: the surface rounding read how far inside the outline a pixel is from the
    amount's slope, which between the drops makes up an outline at every pixel; past two splats the smooth surface's
    distance is used.
  - *A soft lamp's shadow edge ended hard* where the solid field stops saying distances (48 px from a face): a
    penumbra never reaches past 44 px (`SHADOW_REACH`). Glass casts no soft edge (the march near glass took it for
    rock and left a grainy band beside it).
  - *Lines out from a solid's middle*: the light on a face, carried in from the nearest face, changes sharply where the
    solid's own shadow starts; it now falls off as `exp(−(depth in / spill)²)`, so it lights the rim and fades before
    the lines show. (Taking it along the face over the depth was tried: it drew dotted lines where the nearest face
    changes.)
- **What the review found** (fixed): a march to a lamp gave up after 160 steps of at most 8 cells, so at Final detail a
  lamp over 1,300 to 2,500 px away cast no shadows (now 320 steps, 16 cells at a time where level 5 is empty too:
  `a_far_lamp_still_casts_shadows`); the Back's Picture read only a file, not the picture the scene keeps (the
  inspector's "…" now puts it on the Back card, and `BackLook::Picture` carries the kept bytes); its note stayed after
  the Back stopped being a picture; a picture thinner than its scale-down step made a texture with no rows; the shot's
  light map now reaches past the view as far as a solid's face light and the bent back are read (`light_reach`); the
  stage's lamp markers read the lamp at the playhead.
- **Known limits, left as they are**: Paper's cut-out shadow is offset by the largest Paper Depth in the scene, and the
  caustics' ripples use the largest Ripples of any liquid (the light map can't tell liquids apart).

### 6b

Built as planned (decisions 32 to 37), with these changes:

- **A fourth node field, motion** (`(Σk vx, Σk vy, Σk|v|², Σk)` with mips), so the churn and the mean speed are read
  over any reach through the level shares. The look pictures' own speed sums stay as they were.
- **Foam's flow map is pushed by the mean over 120 px**, not over 30 px: in churning water the mean over 30 px
  changes so much from pixel to pixel that a cycle's push tore the pattern into streaks. The churn (where foam
  shows) is still read over 30 px.
- **Trapped air's bubble is the liquid's body drawn as if deep in** (flat, unbent) and lightened, which keeps a
  pocket's colour the body's own. A pocket's middle where no liquid splat reaches takes the first liquid's look.
- **Sparks and mist are one sprite pass, two sprites a drop**, premultiplied (mist over, sparks added); lone is read
  from the density field's level 2, as planned, with soft edges in both speed and crowding so a drop fades in and
  out of them.
- **Wet marks are marked on the card by the make itself** (after every frame) and read back once a chunk; the
  marks for a chunk go to the writer before any moment of the next one. The last frame's end can be a chunk of its
  own (a 2 s final has chunks 0, 1 and 2), so it gets the marks too.
- **The frame a final's moment reads its marks against is ⌊60τ⌋**, its chunk's, so a moment taken from the next
  frame's start reads one frame early.
- Tests: `tests/look.rs` 14 to 19, `tests/wet.rs`, `take::chunk`'s wet test, `water_final`'s wet checks and the
  draft's `the_draft_keeps_its_wet_marks_going_back`; the full-size look check `water-extras` (a fast tap thrown
  at a pool's wall, from a made final; `LOOK_TAKES` keeps the final between runs).
- **What the full-size check found, and the changes it made:**
  - Fizz at Bubbles 0.8 lit nearly every spot, so it read as dots printed on the water, and rose through the
    fast stream. Now 0.3 of the spots have a bubble, each its own size (half to all of Bubble size), and it fades
    out where the mean speed is 250 to 600 px/s.
  - Every edge drop sparked, so a fast stream's edges were a bright outline, and drops arriving at a wall drew a
    burst there and a line down it. Now lone drops spark, and edge drops only when leaving the body (moving out
    the way its liquid thins); near a solid, only drops leaving it.
  - Wet marks 6 px deep and 45 % darker hardly showed: now 12 px and 50 %.
  - A wide cavity the pour opens in the pool stays a hole with Bubbles on: the physics finds trapped air only
    within 48 px of the water round it (part 2c's `air_reach`), so a cavity much wider than that holds none (the
    probe picture of the moment showed air in every small pocket and none in the big ones). Left to the physics.
- **What the review found:** the take's print left out the wet marks' code (`draw/wet.rs`, `shaders/wet.wgsl`),
  so a change to how they are marked or packed would have kept old finals; they are printed now. And the motion
  sums' half-float rounding reads a false spread on an even fast stream (about 6.5 % of its speed), so foam now
  starts at no less than 7 % of the mean speed.

