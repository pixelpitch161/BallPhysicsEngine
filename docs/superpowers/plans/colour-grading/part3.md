# Colour grading part 3: Detail and Effects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give the grade **area steps** (steps that look at the pixels around a pixel) by splitting the grade into
passes, and with them add Basic's **Texture, Clarity, Dehaze**, the **Detail** panel (Sharpening) and the
**Effects** panel (post-crop Vignette with Highlights, moving Grain, Gradient map). Every number keyframes and
follows sound like any other setting; preview and export look the same.

**Spec:** `docs/superpowers/specs/2026-11-09-colour-grading-design.md` (approved by Aedan 2026-11-09), part 3 of 6
(sections 1, 2, 6 and 7). **Before:** `part1.md`, `part2.md` here, `docs/notes/grading.md` ("Part 1/2 as built"),
handover `ballphysics/colour-grading/part2-handover.md` in the project files.

**Tech:** Rust 1.97 (pinned), wgpu/WGSL, egui 0.37.

## Decisions made here (the spec leaves them to the plan)

1. **`PANEL_ORDER`** becomes Basic, Tone Curve, HSL / Colour, Point Colour, Colour Grading, **Detail**
   (`grade.detail`), **Effects** (`grade.effects`), Calibration (Lightroom's order). Arrays of 8. The starting card
   has one of each, so Calibration's layer id moves from 6 to 8 (`full_size_look.rs::grading_colour` indexes panels
   by place and moves its Calibration index from 5 to 7, so that scene stays byte-identical).
2. **Basic** gains `texture`, `clarity`, `dehaze` (−100..100, keyframe, pulse) in the Presence group, before
   Vibrance (Lightroom's order: Texture, Clarity, Dehaze, Vibrance, Saturation). They run in the panel's reading
   order: white balance and exposure, the tone sliders, **then Texture, Clarity and Dehaze together as one area
   group**, then Vibrance and Saturation.
3. **Detail** (`detail.rs`, `DetailPanel`), group "Sharpening": `amount` 0..150 (0), `radius` 0.5..3 px (1.0),
   `detail` 0..100 (25), all `keyframe, pulse`.
4. **Effects** (`effects.rs`, `EffectsPanel`):
   - group "Post-crop vignette": `vignette_amount` −100..100 (0), `vignette_midpoint` 0..100 (50),
     `vignette_roundness` −100..100 (0), `vignette_feather` 0..100 (50), `vignette_highlights` 0..100 (0);
   - group "Grain": `grain_amount` 0..100 (0), `grain_size` 0..100 (25), `grain_roughness` 0..100 (50);
   - group "Gradient map": `map` (`Gradient`, default black → white, `keyframe`, no pulse: a value like the old
     `grade_map`), `map_amount` 0..100 (0).
   - Order inside the panel: Gradient map, then Vignette, then Grain (grain sits on top, as film's does).
   - Labels are Lightroom's ("Amount", "Midpoint", …); the timeline adds the group when a label repeats (part 2).
5. **Program format.** Word 0 becomes `[segments, split, 0, 0]`; words `1..=segments` are the **segment table**
   `[first word, steps, needs pyramid (0/1), 0]`; the steps follow, still `[op, space, words, extra]` + data. A
   segment is what one GPU pass runs. `Program` gains `segments: Vec<Segment { first, steps, pyramid }>`.
   - **Area steps** (ops below marked *area*) always start a segment. Area steps that follow each other with no
     point step between share one segment (and one pyramid): they all read the segment's input picture and each
     adds its change onto the colour so far. Segment 0 may hold no steps (the grade starts with an area step):
     its pass copies the picture in.
   - A grade with no area step is one segment and runs, as now, inside the composite: **byte-identical** to
     part 2 for every stack without area steps.
6. **Passes on the GPU** (`renderer/grade.rs`):
   - With n > 1 segments, passes 0..n−2 run `fs_grade_pass` into an HDR pair (colour and *kept*, the colour a
     weaker-than-full mask started from, so `OP_BEGIN`/`OP_END` survive a pass split: two `Rgba16Float` targets,
     ping-ponged, made the first time a frame has area steps and remade on resize). The last segment runs in
     `fs_composite`, which reads the last pass's pair, then roll-off, sRGB and dither as before.
   - The pass number reaches the fragment as the draw's instance (`draw(0..3, s..s + 1)`, `vs_grade` passes
     `instance_index` flat), so no extra buffer or pipeline per segment.
   - Pass 0 (and the composite when there is one segment) reads `(hdr + bloom) × exposure` exactly as the
     composite does today (`composite_input`); later passes `textureLoad` the previous pair.
   - Between passes values are kept in −65504..65504 (f16), NaN becomes 0; nothing is clipped to 0..1.
   - Group 2 grows from the program alone to: program, input colour, input kept, pyramid. A grade with one
     segment binds 1×1 stand-ins.
   - The three pipelines (pass, first downsample, downsample) and the targets are made the first time a frame
     has area steps; a renderer that never grades with them builds nothing new.
7. **The pyramid.** Before a segment that needs it, its input colour is downsampled into one `Rgba16Float` texture
   of half size with up to 8 mips (`PYRAMID_LEVELS`), the 13-tap filter bloom uses (`fs_grade_down0` from the
   input, `fs_grade_down` per further mip). It holds the **tone** space colour (rgb, the sRGB curve carried above
   1, linear input clamped to 64 first like bloom) and the tone value of the **least channel** in alpha (Dehaze's
   dark channel). Blurs are read with a cubic B-spline (four bilinear taps), so no blocks or diamonds show.
   - **Blur size in video pixels.** An op names a blur's σ in pixels of the scene's video (Scene settings' size).
     The shader turns it into output pixels with `scale = output height / video height`, and mip `k` stands for
     σ ≈ 1.2 · 2^k output pixels: a σ is read at the fractional level `log2(σ / 1.2)` (between two mips), and
     below mip 0 as a mix of the pixel itself and mip 0. So a preview at a smaller size looks like the export
     scaled down.
8. **New ops** (all in `shaders/grade.wgsl`'s one interpreter; numbers in `grade/mod.rs`):
   - `OP_SHARPEN = 10` (tone, *area*, no pyramid): `[gain, limit, σ vpx, 0]`. Lightroom sharpens brightness only:
     the input's tone luma minus a Gaussian blur of it (σ = Radius, taps out to `ceil(2σ)` output pixels, at most
     6, read with `textureLoad`), soft-limited `d' = L·tanh(d / L)` with `L = 0.02 + 0.5·(detail/100)²` (Detail 0
     holds halos back most), added as `gain · d'` with `gain = amount / 100`.
   - `OP_TEXTURE = 11` (tone, *area*): `[gain, limit, σ fine, σ coarse]`: the band between blurs of σ 2 and 8 video
     pixels (medium detail, not the finest edges), `gain = 1.2 · texture/100`, `L = 0.1`. Minus smooths that band.
   - `OP_CLARITY = 12` (tone, *area*): `[gain, limit, σ, 0]`: the pixel's luma minus a blur of σ 40 video pixels,
     `gain = 0.9 · clarity/100`, `L = 0.15`, weighted to the midtones `w = clamp(1 − 0.75·(2Y − 1)², 0.25, 1)` (Y
     the tone luma; above white 0.25). Minus softens (a dreamy look), as Lightroom.
   - `OP_DEHAZE = 13` (linear, *area*): `[d, σ, 0, 0]`, `d = dehaze/100`, σ 80 video pixels. Plus: the veil is the
     least of the pixel's own least channel and the blurred dark channel (so a dark thing in haze isn't made
     negative and a lone white thing isn't taken for haze), capped at 0.8; `v = 0.75·d·veil` (at most 0.6, so no
     channel gains more than 2.5×); `c' = max(c − v, 0) / (1 − v)` (white stays white, the veil comes off and
     colour comes back by itself). Minus: a light fog `A = 0.55 + 0.45·clamp(Y_local, 0, 1)` (grey, brighter where
     the scene is, Y_local the blurred linear luma) mixed in by `0.6·|d|`. The spec's "Dehaze's colour part" is the
     colour the veil hid coming back, so no extra Oklab step.
   - `OP_VIGNETTE = 14` (linear; *area* only when Highlights > 0 and Amount < 0): `[a, mid, width, roundness]`,
     `[highlights, σ 6 vpx, 0, 0]`. Shape: `p = (uv − 0.5)·2`; roundness `r ≥ 0` blends the frame's ellipse to a
     circle on the short side (`q = mix(p, p · size / min(size), r)`), `r < 0` makes a rounder rectangle
     (superellipse exponent `n = 2 − 6r`); `d = (|q.x|ⁿ + |q.y|ⁿ)^(1/n)`. `mid = 0.35 + 0.9·midpoint/100`,
     `width = 0.02 + feather/100`, `v = smoothstep(mid − width/2, mid + width/2, d)`. Amount < 0 darkens like
     exposure, `c · (1 + a·v·(1 − h·b))`, b = `smoothstep(0.3, 1, Y_local)` (Highlights keeps lamps and glows
     bright, Lightroom's Highlight Priority); Amount > 0 lightens towards white in tone space.
   - `OP_GRAIN = 15` (linear): `[amount, cell vpx, roughness, 0]`. Monochrome value noise (quintic, lattice values
     from the composite's PCG hash `dither_noise`, seeded by the frame's time bits, so the same frame always has the
     same grain and every frame differs). Cell `1 + 3·size/100` video pixels. Roughness r adds a finer octave
     (`×2.1`) and clumping (a coarse octave `×0.37` modulating the amplitude):
     `n = (n1 + 0.6r·n2)·(1 + 0.8r·n3)/(1 + 0.6r)`. Applied in linear, scaled by brightness as film is:
     `c' = max(c·2^(1.2·a·n) + 0.0015·a·n, 0)` (a = amount/100; the small added part keeps a faint grain in black).
   - `OP_GRADIENT_MAP = 16` (tone): `extra` = amount (0..1), then `MAP_SAMPLES` = 256 rows of the gradient in tone
     space (`Gradient::bake`, linear, then toned). The pixel's tone luma (clamped 0..1) picks the colour, which is
     multiplied by the luma's excess over 1 when brighter than white (a glow stays bright); `mix(c, map, amount)`.
   - Every panel at its defaults adds no step (Detail Amount 0; Effects with Vignette, Grain and map amounts 0;
     Basic's three at 0).
9. **CPU twins** (`grade/maths.rs`) for the parts tests can check exactly: `lift` (a tone-luma change that keeps
   colour: `(c + ε)·(Y + Δ + ε)/(Y + ε) − ε`, ε 0.05, so greys move by exactly Δ and black can still be lifted),
   `soft_limit`, `clarity_weight`, `dehaze` (given a veil), `vignette` (weight at a place), `sharpen_limit`,
   `gradient_row`. The area ops' neighbourhood reads are tested on the GPU.
10. **Editor**: Detail and Effects bodies are the ordinary grid (`panels.rs::grid`, no rails). The Gradient map row
    is the ordinary gradient setting. Nothing else new.
11. **Moving grain** needs no per-frame program upload: the seed is the frame's time (`Globals.view.w`), so the
    stack (and the uploaded program) stays the same while the playhead moves.

## Files

### bpe-gfx
- `frame/grade.rs`: `GradeBasic` + `texture`, `clarity`, `dehaze` (`area_identity`); `GradePanel::{Detail,
  Effects}`; `GradeDetail { amount, radius, detail }`, `GradeEffects { vignette_*, grain_*, map: Gradient,
  map_amount }` with `is_identity`; exports in `lib.rs`.
- `grade/mod.rs`: ops 10..16, `MAP_SAMPLES`, `PYRAMID_LEVELS`, the blur sizes.
- `grade/compile.rs`: segments, the new steps, `Segment`.
- `grade/maths.rs`: the twins (decision 9). `grade/tests.rs`: CPU tests; `grade/pictures_tests.rs`: GPU tests.
- `shaders/grade.wgsl`: group 2 bindings, `run_grade` per segment (returns colour and kept), the ops, the blur
  reader. `shaders/post.wgsl`: `vs_grade`, `GradeIn`, `composite_input`, `fs_grade_pass`, `fs_grade_down0`,
  `fs_grade_down`, `fs_composite` reading the segment.
- `renderer/grade.rs`: `GradeGpu` (program, stand-ins, `GradeAreas`: pipelines, targets, pyramid, bind groups;
  `run_passes`, `composite_group`, `last_segment`). `renderer/new.rs`, `renderer/scene.rs::finish`: wiring.

### bpe-kit
- `modules/grade/basic.rs` (+3 settings), `detail.rs`, `effects.rs` (new), `mod.rs` (`DETAIL`, `EFFECTS`, arrays of
  8, stack arms), `tests.rs` (eight starting panels, new panels reach the stack).
- `tests/restarts.rs`: minimum raised. `tests/full_size_look.rs`: `grading_colour`'s Calibration index;
  new `grading-detail` scene (lit demo: Clarity, Dehaze, Texture, Sharpening, Vignette with Highlights, Grain, a
  light gradient map).

### bpe-studio
- `grade_mode/panels.rs`: bodies for Detail and Effects. `grade_mode/tests.rs`: kittest.

### Docs
- `docs/notes/grading.md` "Part 3 as built"; `docs/notes/file-map.md`; this plan's "As built".

## Things not to do

- Don't build a pipeline per panel, value or order; the pass count comes from the program, not new shaders.
- Don't clip to 0..1 between passes or inside a step (only f16's limits); don't change any picture without an area
  step (every existing look scene byte-identical).
- Don't seed grain from the clock or random state; don't let any grade key restart physics, sound or particles.
- Don't add Noise Reduction, lens or other panels the spec left out; no built-in grades or example scenes.

## Steps and checks

1. **gfx data, maths, compile, shaders, passes.** CPU tests first:
   - segments: Clarity alone → 2 segments, the second starting with it and needing the pyramid; Texture, Clarity
     and Dehaze → one area segment; Detail straight after them → the same segment; Sharpening first in the stack →
     an empty segment 0; a vignette without Highlights → one segment; a stack without area steps → one segment
     whose words match part 2's after the table;
   - `lift` moves a grey by exactly Δ and keeps a colour's channel ratios when bright; `soft_limit` is odd,
     monotonic, bounded by L; `dehaze` keeps white and black, darkens a hazy grey, `−` lifts black towards fog;
     vignette weight 0 at the centre, 1 in the corner at Amount −100 midpoint 0, rounder with roundness +100;
     the gradient row runs from the first colour to the last.
   - GPU (`test_device`): defaults byte-identical (Basic's three, Detail, Effects); Sharpening makes a ball's edge
     overshoot on both sides; Clarity + widens the ball/background difference and − narrows it; Texture + brightens
     a small dot against its background; Dehaze + darkens a flat hazy grey and keeps white; Vignette −100 darkens
     corners not the centre, +100 lightens them, Highlights keeps a bright picture's corners; Grain: the same frame
     twice is the same bytes, another time differs, the mean stays within 2 levels; Gradient map at 100 recolours
     a grey by its brightness; a pass split leaves point steps as they were (Exposure → Sharpen on a flat picture →
     Saturation equals the stack without Sharpen within 1/255); a half-amount mask across a split; the split line
     with area steps.
   `cargo test -p bpe-gfx --lib grade -- --test-threads=2`.
2. **kit**: settings, kinds, eight panels, stack arms, restarts minimum, look scenes; `cargo test -p bpe-kit`.
3. **studio**: bodies; kittest: the Detail panel shows Amount, Radius and Detail and a change makes the card with a
   Detail panel in the stack; the Effects panel shows the Gradient row.
4. **Whole checks**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
   --workspace -- --test-threads=2` (background); goldens unchanged in debug and release (no bless); full-size look
   check: every scene but `grading-detail` byte-identical to main (`cmp`), and `grading-detail`'s pictures and 1:1
   crops looked at (halos at edges, blockiness in the blurs, grain look, vignette banding); one Opus review; PR; CI
   green; merge.
5. Handover for part 4 in the project files (`ballphysics/colour-grading/part3-handover.md`).

## As built

Built as planned (summary in `docs/notes/grading.md`, "Part 3 as built"). Where it differs, mostly from the review:

- The pyramid's blur per mip, measured on an impulse through the 13-tap downsample and the B-spline, is σ ≈
  1.9 · 2^k output pixels (mip 0 1.76), not 1.2 · 2^k: `PYR_SIGMA` is 1.9, so the blur sizes are the stated ones.
- Sharpening's σ is kept to 3 output pixels (its taps reach 2σ), and it reads the tone luma each pass writes in its
  colour's alpha instead of toning 169 neighbours.
- `soft_limit` clamps its argument to ±10 before `tanh` (NaN on back ends that work tanh out from exp); the pixel's
  own tone colour is clamped to ±64 like the pyramid's first level.
- The lightening vignette leaves light past white where it is (it pulled it down to white).
- Grain smaller than an output pixel is weakened by its size in output pixels, so a small preview isn't grainier
  than the export scaled down.
- Look check: grain speckled the balls' highlights (`exp2` grain on values past white). It now fades by the
  pixel's peak channel (smoothstep 0.6 to 1.5); a fade by luma didn't stop it on saturated pink. Faint wavy
  lines in the lit demo's haze show only under a strong contrast stretch and appear with every part 3 setting
  off, so they come from the scene and parts 1-2's strong Shadows/Highlights, not part 3.
- Left of the Split line, with area steps, the picture passes through the f16 pairs (within a level of Before).
