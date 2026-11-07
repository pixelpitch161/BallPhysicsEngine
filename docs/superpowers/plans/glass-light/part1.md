# Glass and light part 1: coloured shadows and stained glass — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** light that passes through anything see-through takes its colour, pixel by pixel, in Flat and Stage, in the
direct light, the light field and the haze, with Tints light, Deeper when thick and the Lighting card's Coloured light
through glass switch.

**Architecture:** a new light pass (8, the **tint picture**) draws every thing once more as its *optical depth*
(−ln of what it lets through, per channel). The blockers' field pass folds that into the field (which becomes
Rgba16Float: R distance, G/B/A how much the nearest blocker blocks of each channel), and every reader of the field
(the march, the near-miss soft edge, Stage slabs, the light field's cascades) reads colour from it. Stage spheres get
Deeper when thick from their own record. The CPU light probe tints by each see-through thing's colour.

**Tech stack:** Rust, wgpu/WGSL (bpe-gfx), the kit's draws (bpe-kit), the settings derive.

**Spec:** `docs/superpowers/specs/2026-11-07-glass-light-design.md`, section "Part 1". Read it with this plan.

## Global constraints

- Draw-only: physics, sound and every golden (`crates/bpe-kit/tests/golden*.txt`) stay byte-identical.
- No Lighting card: picture byte-identical (`tests/unlit_pictures.rs`).
- Nothing see-through: the lit picture is unchanged (`tests/lit_flat_pinned.rs`, `tests/stage_pictures.rs`,
  `tests/thin_blockers.rs`, `tests/see_through_shadows.rs`, `tests/glass_pictures.rs` pass untouched).
- Coloured light through glass off: exactly #145's colourless picture.
- New numbers are `span(..)` and `keyframe`; no `range` except the hard limits named here.
- The direct and mix passes stay within 16 sampled textures a stage (they are at 16 today).
- Live and baked alike (the bake renders the same passes per sample).

## Decisions (where this plan departs from or pins down the spec)

1. **The tint picture stores optical depth, not a multiplied colour.** `D = −ln(transmittance)` per channel in an
   Rgba16Float picture cleared to 0, written by every existing pipeline with alpha 0. The pipelines blend premultiplied
   "over" (`src + dst·(1 − src.a)`), so alpha 0 adds: depths add, transmittances multiply, exactly the spec's multiply
   blend, with no new pipelines. Alpha is unused; thickness is folded into the depth (Beer's law) instead of a separate
   max-alpha channel.
2. **Each pixel's depth** = `−ln(share) + (−ln(T)) · (1 + THICK · thick)` where
   - `share = 1 − blocks · p` (what passes, #145's maths), `blocks` the record's folded blocking (`light[1]`),
   - `p` the thing's *presence* at the pixel: its alpha with the antialiased edge divided out where that edge covers at
     least `HARD_FROM` (0.1) of the pixel — so its opacity inside, a faint soft edge stays faint,
   - `T = mix(1, chroma(colour), clamp(tints · p, 0, 1))`, `chroma(c) = c / max(c.r, c.g, c.b)` (white for black),
   - `thick` = Deeper when thick × the thing's thickness at the pixel (0..1).
   The spec's "tints × see_through_share" is read as "tints × presence": clear glass (See-through 1, opacity 1) still
   tints fully, while the far soft edge of a sprite doesn't throw a hard ring of full colour.
3. **The colour is read through the blockers' field**, not the tint picture: only `fs_field` reads the tint picture,
   bound in place of `picture` (group 0 binding 1, which the field passes never read). So the direct, mix, light field
   and Stage passes gain no binding. The stained-glass picture is thrown at the field's size (full at Ultra and Bake,
   half at Fast/Good, a quarter at Draft) and blurred bilinearly, as the field already is.
4. **Field channels.** R distance (as now). For a blocked texel, over its blocking pixels (`foot_*`, the same pixels
   `foot_most` reads): `k` = the mean of each pixel's colour depth `D − min(D)` (the colour part; its grey part is the
   share), and `g` = the most any pixel's grey part `min(D)` is. Then per channel
   `b_c = o` where `k_c == 0` (exact), else `1 − (1 − o) · e^(−k_c)`, with `o` = #145's `blocker_opacity` in Flat and,
   on a Stage, `1` unless the tint picture was drawn and `g < TINT_SOLID` (10), when `o = 1 − e^(−g)`. A texel that only
   closes a diagonal gap takes its neighbours' as `blocker_opacity` does. Readers take the blocker's opacity as
   `min(b)` (exactly #145's G when nothing is coloured) and its colour as `(1 − b_c) / (1 − min(b))` (exactly 1 when
   uncoloured).
5. **The light field** keeps #145's rule for how much passes (glass under half opaque is open) and only adds the colour:
   a cascade ray crossing a blocked texel of the blockers' field (it is see-through, or it would be opaque in
   `gi_scene`) multiplies what it carries by that blocker's colour. Only when the tint picture was drawn, so other
   scenes' light fields don't change.
6. **Stage.** Balls draw nothing in the tint pass on a Stage (they are spheres there). Shapes' slabs read the field's
   per-channel blocking. Spheres keep `sphere_block`'s tint (`colour × share`), and with Coloured light through glass on
   it becomes `share · (mix(1, colour, tints))^(1 + THICK · deeper · f)`, `f` how near the middle the ray passes
   (`sqrt(1 − (m/r)²)`), so Tints light 1 and Deeper 0 are exactly today's.
7. **Per-record tints** ride in `LayerGpu.head[2]` bits 16..23 (Tints light) and 24..31 (Deeper when thick), each
   `round(v × 64)` clamped to 0..255 (so 0 to 3.98 in 1/64 steps), stamped by `surface::Stamps::apply`. Bits 0..4 stay
   the flags. A layered thing takes its thickness, blocking and Deeper from its Body record; each layer's Tints light
   is applied to its own colour in the walk (so a Surface sub-layer's Tints light counts).
8. **Pass only when needed.** `LightScene::needs_tint(&Frame)`: Coloured light through glass on, some light or glow,
   and `Frame::see_through_blockers()` — a conservative scan (a false yes only costs the pass): a look record that
   blocks light with `0 < light[1] < 1` or a drawn record's opacity `amount.x < 1`, a layered instance at opacity
   `misc.x < 1`, any trail vertex, particle or field that blocks light (their edges are soft), in the frame, its blend
   layers' frames and its overlay.
9. **Probe** (`sensing/probe.rs`): a blocker carries `passes: [f32; 3]` — its share
   `1 − (1 − See-through) · Body opacity` times, with Coloured light through glass on, `mix(1, chroma(colour), Tints ×
   opacity)` (colour: a ball's Body, a shape's first Colour and gradient layer else its Body, `layer_colour`). Solid
   blockers keep the old maths exactly (`v` and its smoothstep); see-through ones multiply per channel by
   `mix(passes, 1, smooth(v_b))`. Deeper when thick is left out of the probe (it reads luminance only).
10. **Followed colours and the probe.** The probe resolves without light inputs, so a colour that follows light is read
    unfollowed there; no new `lighting_reads` entries (Tints light and Deeper are Surface settings, already covered).

## Review focus

1. A **scene with no see-through record** but Coloured light through glass on must draw byte-identically: every vec3
   path must reduce to the old scalar per channel (test: white glass on vs off, byte-equal).
2. **Opaque thin lines on a Stage** with the tint pass on must still block whole (the Stage opacity takes the grey
   part's *most*, and depth is clamped so solid reads ≥ `TINT_SOLID`) (test: `stage_keeps_solid_shapes_solid`).
3. **Things that don't block light** (Blocks light off, Lit-0 shine layers, liquids, swarms) must write no depth (they
   would tint light they don't touch) (test: a red shape with Blocks light off throws no colour).
4. **Blend layers** (a shape with a Blend layer) are composited into the tint picture additively (`Blend::Add` in pass 8),
   or they would erase the depth under them (test: the red glass test repeated with the shape given a Blend layer).
5. **Overlapping coloured things** multiply (red over blue gives purple), never take the top one's colour only.

---

### Task 1: the tint pass in the draw shaders (bpe-gfx)

**Files:**
- Create: `crates/bpe-gfx/src/shaders/tint.wgsl`
- Modify: `crates/bpe-gfx/src/lib.rs` (`scene_source`, `TINT_PASS`, `light_globals`, render's light-pass loop,
  `draw_layer`'s composite blend), `crates/bpe-gfx/src/sprites.rs`, `fields.rs`, `swarm/draw.rs` (prepend tint.wgsl),
  `shaders/scene.wgsl`, `shaders/sprites.wgsl` (`light_flat`, `light_extra`, `hard_tags`), `shaders/layers.wgsl`
  (`light_piece`, `shade_layers`, `hard_tags` callers), `shaders/layers_walk.wgsl`, `shaders/shape_walk.wgsl`,
  `shaders/shape_layers.wgsl`, `shaders/liquid.wgsl`, `crates/bpe-gfx/src/frame.rs`, `crates/bpe-gfx/src/lighting.rs`
  (`Targets.tint`, `picture(8)`).
- Test: `crates/bpe-gfx/src/frame.rs` tests, `crates/bpe-gfx/src/programs_tests.rs` (sources still parse).

**Interfaces — produces:**
- `tint.wgsl`: `const TINT_PASS: f32 = 8.0; const THICK: f32 = 2.0; const TINT_SOLID: f32 = 10.0;`
  `fn tint_chroma(c: vec3<f32>) -> vec3<f32>`; `fn tint_out(colour: vec3<f32>, presence: f32, blocks: f32, tints: f32,
  thick: f32) -> vec4<f32>` returning `vec4(D, 0)` per decision 2 (0 when `blocks <= 0` or `presence <= 0`; `share` at
  least 1e-6, `T` at least 1/1024).
- `frame.rs`: `LayerGpu::TINT_SHIFT = 16`, `LayerGpu::DEEPER_SHIFT = 24`, `LayerGpu::TINT_BITS: u32 = 0xffff_0000`,
  `pub fn tint_bits(tints: f32, deeper: f32) -> u32`; `LightingGpu::glass_colour(&self) -> bool` (`more[3] > 0.5`);
  `Frame::see_through_blockers(&self) -> bool` (decision 8); `LightScene::needs_tint(&self, frame: &Frame) -> bool`.
  `SphereGpu.b[3]` = Deeper when thick, `SphereGpu.c[1]` = Tints light (docs updated).
- `lib.rs`: `const TINT_PASS: u32 = 8;` the pass drawn into `Lighting::picture(8)` when `needs_tint`.

- [ ] **Step 1: tests for the CPU parts.** In frame.rs tests: `tint_bits(1.0, 0.5) == (64 << 16) | (32 << 24)`;
  `tint_bits(-1.0, 9.0) == 255 << 24`; `tint_bits(..) & 0x1f == 0`. `Frame::see_through_blockers`: false for a default
  frame and for a frame whose only record has `light[1] == 1.0` and `amount.x == 1.0`; true with `light[1] == 0.5`;
  true with one `TrailVertex` whose `light[1] > 0`; true when only a blend layer's frame has such a particle. Run
  `cargo test -p bpe-gfx --lib frame` → fails to compile.
- [ ] **Step 2: implement the Rust side** (`tint_bits`, `see_through_blockers`, `needs_tint` = `settings.glass_colour()
  && (!lights.is_empty() || glows) && frame.see_through_blockers()`, `Targets.tint` an HDR_FORMAT full-size picture
  made with the others, `picture(8)`, `TINT_PASS` added to the modes drawn after mode 6 when `needs_tint`, the light
  globals grown to `2 * TINT_PASS` slots, `draw_layer` composites pass 8 with `Blend::Add`). Prepend tint.wgsl to
  `scene_source` and to the three sprite-family sources.
- [ ] **Step 3: the shaders.**
  - `light_flat` (scene.wgsl and sprites.wgsl): passes 5..7 only go to `light_extra` (`m > 4.5 && m < 7.5`); UV-only
    returns 0 as now; then `m > 7.5` returns `tint_out(c.rgb / c.a, p, blocks, 1.0, 0.0)` (0 for `c.a <= 1e-5`, light
    added with no cover), `p` = `c.a` hardened by `AA_COVER` in scene.wgsl (`c.a / AA_COVER` where `AA_COVER >=
    HARD_FROM`, at most 1), `c.a` in sprites. `hard_tags`/sprites' `light_extra` pass pass 8 through.
  - `layers.wgsl`: private `TINT_BLOCKS`, `TINT_THICK`; `light_piece` handles pass 8 first: a layer that keeps
    (`light.y <= 0`, not the Body) shows nothing (`a_in = a_out = 0`); otherwise its colours become
    `mix(1, tint_chroma(c), clamp(tints, 0, 1))` (tints from `head.z` bits), its light 0. `shade_layers`, after the
    thing's opacity and clip: in pass 8 the output is `tint_out(out.rgb / out.a, p, TINT_BLOCKS, 1.0, TINT_THICK)` with
    `p` = `out.a` over `cover` where `cover >= HARD_FROM` (at most 1), and `vec4(0)` for a ball on a Stage
    (`LIGHT_BALL && g.light.z > 0.5`). Same in `shade_shape_layers` (no Stage exception).
  - `layers_walk.wgsl` where `LIGHT_HEIGHT` is set: `TINT_BLOCKS = body_rec.light.y`, `TINT_THICK = deeper ·
    sqrt(max(1 − e², 0)) · step(d, 0)` (deeper from the Body's bits); the group colour (`case 8u`) is laid over in pass 8
    too. `shape_walk.wgsl`: `TINT_BLOCKS = look_layers[first].light.y`, `TINT_THICK = deeper · clamp(−s.edge /
    max(layered_shape.a.w, 1e-6), 0, 1)`.
  - `liquid.wgsl`: pass 8 returns `vec4(0)` (a liquid blocks no light).
- [ ] **Step 4:** `cargo test -p bpe-gfx --lib -- --test-threads=2` → frame tests pass and every assembled shader still
  validates (the existing source tests).
- [ ] **Step 5: commit** "Glass part 1: the tint pass".

### Task 2: the field carries colour, and every reader takes it (bpe-gfx, lighting.wgsl)

**Files:** `crates/bpe-gfx/src/lighting.rs` (`BLOCKER_FIELD_FORMAT` → `Rgba16Float`, `run(.., tint: Option<&TextureView>)`,
`Uniform.trace` z = glass colour on, w = tint drawn, the field pass's group 0 binding 1 = the tint view or the dummy),
`crates/bpe-gfx/src/lib.rs` (passes the tint view), `crates/bpe-gfx/src/shaders/lighting.wgsl`.

**Interfaces — consumes:** Task 1's tint picture and `TINT_SOLID`/`THICK` (copied into lighting.wgsl as
`TINT_SOLID_DEPTH` and `THICK_DEPTH`, with a Rust test that the two files agree).
**Produces (WGSL):** `fn foot_tint(t: vec2<i32>) -> vec4<f32>` (mean colour depth rgb, most grey), `fn
field_all(p: vec2<f32>) -> vec4<f32>` (replaces `field_both`: distance, blocking rgb), `fn opacity_of(b: vec3<f32>) ->
f32` (`min`), `fn colour_of(b: vec3<f32>) -> vec3<f32>` (decision 4, 1 where `b_c <= min(b)` or `min(b) >= 1`);
`Ray.lit: vec3<f32>`, `Ray.e: vec3<f32>`; `visibility(..) -> vec3<f32>`; `slab_visibility(..) -> vec3<f32>`.

- [ ] **Step 1:** `fs_field` writes `(d, b_r, b_g, b_b)` per decision 4 (`u.trace.w` 0: `b = vec3(o)` with `o` as
  today). `field_opacity` (the light field's thin rule) reads `opacity_of`.
- [ ] **Step 2: the march.** Inside a see-through stretch keep `col = min(col, colour_of(field_all(q).yzw))` from the
  stretch's first sample; leaving it, `lit *= (1 − glass) * col` (so `vec3(1 − glass)` when uncoloured, exactly as
  now); solid crossings set `lit = vec3(0)`. The near-miss edge: `o3 = select(clamp(b, 0, 1), vec3(1), stage ||
  opacity_of(b) >= SOLID)`, `e = min(e, select(1 − o3 · (1 − s), vec3(s), o3 >= vec3(1)))` per channel.
  `visibility` works per channel: `blocked += 1 − r.lit`, `o = blocked / crossings`, result
  `select(1 − o · (1 − share), vec3(share), o >= vec3(1))`; early outs return vec3s. `lamps()` and `shade()` take the
  vec3 (`shade`'s Flat branch drops its `vec3(..)` wrap). The haze (`air`) already multiplies by the vec3 tint.
- [ ] **Step 3: Stage slabs.** `slab_visibility` keeps its scalar `v` over solid samples (`opacity_of(b) >= 1`) exactly
  as now and a `see: vec3` = min over see-through samples of `1 − b · (1 − smooth(raw))`; returns
  `face · min(vec3(smooth(v)), see)`; it stops early only when `v <= 0`. `field_on_slab_all(q)` reads `field_all`.
- [ ] **Step 4: spheres.** `sphere_block` with `u.trace.z > 0.5`: `through = clamp(sp.a.w, 0, 1) · pow(max(mix(1,
  sp.b.rgb, clamp(sp.c.y, 0, 1)), 1/1024), 1 + THICK_DEPTH · max(sp.b.w, 0) · f)` (decision 6); otherwise as today.
- [ ] **Step 5: the light field.** `fs_cascade` with `u.trace.w > 0.5`: at each step read the blockers' field texel under
  `q` (`textureLoad(field, qi)`, same size); inside a blocker (`r < 0`) take `col = min(col, colour_of(..))` and step
  0.75 texels; on leaving it `trans *= col`; steps are at most `max(field_r / texel_world, 0.75)` so thin glass isn't
  skipped. What a hit gives, and what the cascade above adds, are times `trans`.
- [ ] **Step 6:** `cargo test -p bpe-gfx -- --test-threads=2` (all lighting/lens/fields tests) and
  `cargo test -p bpe-kit --test lit_flat_pinned --test thin_blockers --test see_through_shadows --test stage_pictures
  --test glass_pictures --test lit_pictures --test unlit_pictures` → all pass unchanged.
- [ ] **Step 7: commit** "Glass part 1: the field carries colour".

### Task 3: settings and stamps (bpe-kit)

**Files:** `crates/bpe-kit/src/modules/look/surface.rs`, `crates/bpe-kit/src/modules/lighting.rs`,
`crates/bpe-kit/src/things/balls/draw.rs` (`push_ball_light`).

**Interfaces — produces:** `SurfaceSettings.tints` ("Tints light", `span(0, 1)`, keyframe, group Surface, default 1;
help "How much light passing through takes its colour: 0 none, 1 all of it.") and `SurfaceSettings.thick_tint`
("Deeper when thick", `span(0, 1)`, keyframe, group Surface; help "How much deeper the colour gets where it is thick,
like the middle of a glass ball."); `Preset.tints` (1 everywhere) and `Preset.thick_tint` (0.5 for Glass and Jelly, 0
otherwise), both in `preset_values`; `Stamps.thing_tint: u32` and `Stamps::apply` writing `tint_bits` into
`head[2]` (the thing's on every record, each own-surface layer's on its records, other bits kept);
`LightingSettings.glass_colour: bool` ("Coloured light through glass", group Shadows, default true; help "Light through
see-through things takes their colour.") → `gpu().more[3]`; `SphereGpu` gains `b[3] = thick_tint`, `c[1] = tints`.

- [ ] **Step 1: tests.** surface.rs: Glass and Jelly presets have `thick_tint == 0.5`, every other 0, all `tints == 1`;
  `presets_set_every_number…` still passes with both keys in `preset_values`; `Stamps::apply` on records with
  `head[2] = FLAG | UNWORN` leaves those bits and adds `tint_bits(t, d)` from the card (and a Surface sub-layer's own on
  its layer's records). lighting.rs: default `glass_colour` true → `gpu(None).more[3] == 1.0`, off → 0.
- [ ] **Step 2:** implement; `cargo test -p bpe-kit --lib surface lighting` → pass.
- [ ] **Step 3: commit** "Glass part 1: Tints light, Deeper when thick, Coloured light through glass".

### Task 4: pictures (bpe-kit GPU test `tests/coloured_shadows.rs`)

Harness as `see_through_shadows.rs` (270 × 480, Light only, linear light, `BPE_SAVE_RENDERS`), with `light_rgb(pixels,
x, y) -> [f32; 3]` and per-channel shares against the same scene with Blocks light off. The wall: `wall(600)` across
the middle, lamp at (0, −400), ambient 0, Deeper when thick 0 unless said. Colours are set as sRGB; expected
transmittances use their linear values and `chroma`.

- [ ] **Step 1: write the tests** (each fails before Tasks 1–3 are in):
  1. `red_glass_throws_red_light` — See-through 0.85, Body `#ff8080`: behind it (the four `BEHIND` points, Good and
     Ultra) R share 0.85 ± 0.06, G and B shares `0.85 · t` ± 0.06 with `t = lin(0x80)` (chroma of the linear colour).
  2. `red_over_blue_gives_purple` — two overlapping walls 40 apart, `#ff6666` and `#6666ff`, See-through 1: behind both,
     each channel's share within 0.06 of the product of the two transmittances; R and B each more than 3 × G.
  3. `a_two_colour_pane_throws_both_colours` — a Body gradient red→blue Across the wall, See-through 1: behind its left
     third R > 2 × B, behind its right third B > 2 × R.
  4. `tints_light_0_is_colourless` — test 1's wall at Tints light 0: the picture is byte-identical to a white wall's
     (and each share 0.85 ± 0.06).
  5. `glass_colour_off_is_exactly_colourless` — Coloured light through glass off: the red wall's picture equals the
     white wall's byte for byte, and the white wall's picture with it on equals it off (the vec3 paths are exact).
  6. `deeper_when_thick_deepens_the_middle` — a red Glass ball (radius 160, `#ff8080`, Deeper 0.5) between lamp and
     floor: behind its middle the G/R ratio is `t^(1 + 2 · 0.5)` ± 0.06; at Deeper 0, `t` ± 0.06; and behind a ray 0.9 r
     from its middle G/R is above the middle's.
  7. `a_blended_glass_shape_still_tints` — test 1 with a Blend (Multiply) layer on the wall: the same shares ± 0.06.
  8. `a_shape_that_doesnt_block_throws_no_colour` — test 1 with Blocks light off: shares 1 ± 0.03 in every channel.
  9. `stage_glass_slab_lets_coloured_light_through` — Depth Stage, the red wall at See-through 0.85 standing in the
     room: its shadow on the back wall is brighter than with See-through 0 and its R/G ratio over 1.5; a solid wall's
     shadow (See-through 0) is as dark as with Coloured light through glass off (`stage_keeps_solid_shapes_solid`,
     shares within 0.02 of each other).
  10. `glow_through_red_glass_comes_out_red` — no lamps; a white ball with Gives off light (strength 2) at (0, −300)
     behind the red wall at See-through 0.85 (open in the light field), Glow reach 1500: at the four `BEHIND` points
     R > 1.5 × G in the light (and with a white wall R ≈ G within 10%).
- [ ] **Step 2:** `cargo test -p bpe-kit --test coloured_shadows` → all pass; then the whole Global-constraints list.
- [ ] **Step 3: commit** "Glass part 1: coloured shadows pictures".

### Task 5: the light probe (bpe-kit/src/sensing/probe.rs)

**Interfaces:** `LookLight.passes: [f32; 3]` (decision 9; `[0; 3]` for solid), `Ball.passes`, walls
`(ThingId, Vec<[Vec2; 2]>, [f32; 3])`, `Blockers::visibility(..) -> [f32; 3]`; `Lamps.glass_colour: bool` from the
Lighting card.

- [ ] **Step 1: tests** (`sensing/tests.rs`): `a_red_see_through_pane_lowers_the_light_by_its_share_and_colour` — the
  test ball at the middle, a lamp at (0, −400), a filled rectangle shape across between them at See-through 0.6, Body
  `#ff0000` (linear red): the ball's `LightSource::On` light (`Lamps::sample(..).lamps_lit`, via the probe's level and
  `level_of` inverted, ambient 0) is `0.6 · luminance([1, 0, 0])` (± 2%) of the open light; with Coloured light through
  glass off, `0.6` (± 2%); a solid pane (See-through 0) still 0. Existing probe tests unchanged.
- [ ] **Step 2:** implement; `cargo test -p bpe-kit --lib sensing` and the moth/rule goldens
  (`cargo test -p bpe-kit --test golden` etc.) → unchanged.
- [ ] **Step 3: commit** "Glass part 1: the light probe sees coloured light".

### Task 6: finish

- [ ] CLAUDE.md: "Glass and light" says part 1 is in, with the names above (tint pass 8, field channels, bits, settings,
  probe).
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx with `--test-threads=2`), goldens in
  debug and release unchanged.
- [ ] One Opus review of the branch; fix what it finds; PR; merge when checks pass.

## Not in this part

- Light bent by glass (part 3), prisms and beams (part 2), mirrors (part 4).
- The light field doesn't take how much see-through things pass (only their colour), as #145 left it.
- Deeper when thick in the probe; the probe's colour of a thing is one colour (its Body's or first paint's), not its
  picture.
- A layer wearing its own Surface sub-layer takes its own Tints light but its thing's blocking and Deeper when thick.
- Two different colours inside one field texel mix (the field is coarser than the picture below Ultra).

## As built

- **Clear glass seeds the field.** A pixel that blocks under `BLOCK_LEAST` (See-through 1 or 0.95) still counts as a
  blocker when its colour holds back some channel by `TINT_LEAST` (0.1, the tint picture's deepest channel less its
  shallowest; `pixel_tints`). The seed pass reads the tint picture too (bound in `picture`'s place, as the field pass
  does), `foot_most` returns −1 for "no blocker" (clear glass blocks next to nothing), and `march` walks through what
  the field calls `clear_tint` (opacity under `BLOCK_LEAST`, coloured) when `confirm` finds nothing in the picture,
  taking its colour from the field; a pixel on clear coloured glass leaves it first. Flat only: on a Stage the surface
  picture seeds see-through shapes whole already.
- **Paint layers that don't block** colour inside a blocking body: a layer with Blocks light off writes no presence
  of its own, but the colour seen through the thing is the stack's colour there (a Colour and gradient layer over a
  glass Body tints with the paint's colour).
- **The probe** takes a see-through ball's `passes` once even where the grid visits it twice (the least), and walls
  multiply by `passes` per crossing.
- **Stage.** A see-through shape's slab shadow now lets its share through (it was solid): `slab_visibility` returns the
  solid `v` times the least `see` of the see-through slabs crossed. `tests/see_through_shadows.rs`'s Stage test turns
  Coloured light through glass off, the old behaviour.
- **Tests** in `tests/coloured_shadows.rs` (10). A two-colour pane needs its gradient turned to 0 (left to right): the
  default Turn 90 runs through the wall's thickness, and a ray crossing red then blue rightly comes out dark.
- **After review.**
  - The light field holds a ray's steps short only near a *coloured* blocker, and those steps don't count against
    its budget, so where nothing is coloured (white glass, Tints light 0) it is exactly as without the tint picture.
  - The tint picture blocks by the most any of the thing's layers blocks (a Body with Blocks light off under an
    Outline that blocks still shadows on a Stage); `pixel_tints` needs the surface picture to block a little there
    (clear glass blocks 1/1024), so a thing that doesn't block light never colours it.
  - The group colour tints by the Body's Tints light (`tint_group_over`).
  - A thing split at a Blend layer writes its tint from the lower part only (`TINT_SKIP`): the layers above the Blend
    layer don't tint.
  - A Stage glass sphere at Tints light 1 and Deeper when thick 0 is exactly its colour, as before.
  - Probe: a see-through ball lets its share through once however often the grid meets it, and balls in a row
    multiply; a filled shape is passed once going in and out (a band each crossing).
  - Known: on a Stage, a thing at Opacity under 1 (and soft particles) now lets its share through where it used to
    block whole; the probe lets see-through things' share through even with Coloured light through glass off, as the
    Flat picture has since #145.
