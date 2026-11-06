# Lighting part 3: the cinema render (Bake on export, Preview final frame)

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` (approved), the last build part. Built on every other
part (1a #106, 1b #111, 1c #110, 2a #109, 2b #116, 4 #114, 5 #115, 6 #108, 7 #113, 8 #112). Built straight after
this plan (no approval stops, owner's standing instruction for the overnight lighting run), one Opus review at the
end, then the PR, merged by the thread itself once tests and the review pass. A short video and stills go to project
files `ballphysics/lighting/part3/`. After merging, the exes are built from main and installed on the owner's PC
(the last part).

## What part 3 delivers

- **Bake on export**: the Lighting card's Quality fold gains **On export** (Same as live / **Bake**) and the bake's
  settings: **Samples** (rays a pixel), **Light bounces**, **Bake reflections**, **Motion blur** with **Shutter**,
  **Noise clean-up**. A baked frame is the scene drawn many times and averaged: each sample a little moved within the
  pixel (smooth edges), a little earlier within the shutter (true motion blur from the physics' own steps), with its
  own random rays (soft glossy reflections, smooth rainbows, path-traced bounce light, more caustic photons).
- **The bake's light**: the light field at full size with more steps and as many bounces as Light bounces asks
  (Flat); in Stage a **path tracer** for bounce light (cosine rays from every pixel against the balls, the wall, the
  floor and the shapes' fronts, each hit lit by the lamps with the balls' shadows, carried on for Light bounces),
  cleaned up by an edge-aware filter guided by the surface picture; **deep reflections** (Bake reflections, up to
  16) and **blurred reflections** of the scene on rough surfaces (2b kept the sky there); **rainbows** in glass from
  one wavelength a sample.
- **Caustics** (live and baked): **Glass caustics** on the card. Light from every lamp is traced through every
  see-through ball (two refractions through the sphere, each photon its own wavelength when the glass has Rainbow
  split), and lands on the back wall or floor (Stage) or on the picture under the ball (Flat) as a focused bright
  spot with rainbow edges. Live uses fewer photons, the bake many more a sample.
- **Preview final frame**: a button on the Lighting card (and **Test a frame** in the export window) renders the
  frame at the playhead exactly as the export will, in a window, with how long it took and about how long the whole
  video would take.
- **Export window**: a Lighting row (Live / Bake) when the scene has a Lighting card, the bake's settings in one
  line, and Test a frame. The command-line renderer takes `--bake` / `--live`.

## Decisions made while planning (fill-ins of the spec, each stated)

1. **A bake is an average of samples, not a second renderer.** The spec's path tracer "over the same LightScene"
   is built as extra stochastic work inside the existing light passes plus accumulation, so Live and Bake share
   every pass, light, surface and effect, and "Bake differs only by rays, bounces and clean-up" holds by
   construction. `Renderer::render_samples(&[Frame])` draws each sample's scene and lighting (`render_scene`), keeps
   a running average in a 32-bit float picture (`accumulate` pass, ping-pong: `a + (x − a)/(i + 1)`, no float
   blending needed), puts the average back in the HDR picture, then runs the picture effects, bloom and grade once
   (`finish`) with the last sample's frame. One sample is exactly `render`.
2. **Samples.** Sample `i` of `n`: sub-pixel offset from the Halton (2, 3) sequence (`bake::jitter(i)`, in output
   pixels, −0.5..0.5, sample 0 at the pixel's middle), applied by moving the frame's camera centre by the offset times
   the world size of a pixel (turned with the camera). Things stuck to the screen are not offset.
3. **Motion blur from the physics' steps.** The bake's moments for a frame at `t` (fps `f`): `t_i = t − s/f ·
   (n − 1 − i)/n` (`s` = Shutter, 0 with Motion blur off), so the last sample is the frame itself and the samples go
   forward in time (the world only runs forward; that is why Shutter has the hard range 0..1: a longer shutter would
   need moments before the last frame drawn). A moment between two physics steps `k` and `k + 1` draws the world at
   step `k + 1` with every ball moved back to `previous + (position − previous) · a` (`World::set_between`, undone by
   `World::restore_between` before the next step, so the physics and the goldens never see it); a moment on a step
   draws that step. Keyframes, pulses and Follow sound are read at the sample's own time, so animated settings
   blur too. Trails read the history as before.
4. **Effects with memory see one frame, not n.** The chain takes an `effect_time` during samples: every sample of a
   frame counts as the same moment for an effect's history (the first swaps, the rest redo from the same `before`),
   so a feedback trail is as long baked as live. Glow in the dark's memory steps by each sample's real time gap,
   which adds up to the frame's (monotonic), so it is the same too.
5. **The bake's quality.** Samples carry `Frame.bake = Some(BakeSample { index, count })`. `render_samples` sets
   each sample's `light.settings.more[2]` to 4 (Bake: `quality_tier(4)` = field at full size, 128 shadow steps;
   `gi_steps(4)` = 32) and `dof[3]` to Bake reflections. The lighting uniform gains `sample: [index, count, baking,
   light bounces]`; the shaders' random numbers (`seed_rng`, `rand`: PCG of pixel, sample and a salt) are only used
   when `baking`, so live pictures are untouched.
6. **Flat bounces.** The light field's runs: live as today (2 with glowing things and Bounce light, else 1); baking,
   `1 + bounces` with glowing things, `bounces` without (at least 1), each run bouncing the last (run ≥ 1 already reads
   the previous run's field).
7. **Stage path tracer** (`fs_trace`, baking, Stage, Bounce light ≠ 0, Light bounces ≥ 1): for each pixel the point
   and normal as `surfaced` finds them (a ball's height slope, a shape's front, the wall or floor by `stage_spot`), 2
   cosine-weighted rays a sample (`TRACE_RAYS`); each walks up to Light bounces hits (`scene_hit`: balls by the
   grid, wall, floor, shape fronts), at each adding `carried × bounce_light(hit)` (the lamps on it, Lambert, with the
   balls' shadows (`sphere_shadow`), times its colour from the picture, its Body colour on a ball's far side) and
   carrying its colour (at most 0.95) onward; escaping rays add nothing (the ambient term already is light from
   everywhere). The mean, times Bounce light, is light arriving. Then **Noise clean-up**: three à-trous passes
   (steps 1, 2, 4; 5×5 B-spline) whose weights fall with differences in cover, surface index and height, and in the
   light itself; off, the raw rays. The result is added into the background-light picture (`bg_glow`), which the
   Stage mix adds to every pixel's light already. In such a bake the light field leaves bounce out (it would count
   it twice); it still carries glowing things' light.
8. **Caustics.** `Glass caustics` (span 0..2, default 1, keyframe). Glass = a sphere whose See-through ≥ 0.05 and
   Bends light > 1.0001. Photons per (light, glass ball) pair: Draft 1024, Good 4096, High 16384, Ultra 32768,
   Bake 65536 a sample; at most 2^20 photons a frame live (2^23 baking: export still honours every ball, with fewer
   photons each), at least 64 a pair. Each photon (`vs_caustic`): a point on the ball's disc as the lamp sees it
   (Fibonacci spiral by index, turned by the sample's random turn when baking), from the lamp's place (Point, Spot,
   Area/Strip nearest point; Sun parallel; lasers left out: their beams already bend), entering and leaving the
   sphere by Snell (total internal reflection drops it), with η × (1 + Rainbow split · 0.04 · s) for a wavelength
   `s` ∈ [−1, 1] (red to violet) carrying a spectral colour that averages to white. It lands on the nearest of the
   back wall (z = −Back wall) and floor (Stage), or the plane under the ball (z = −radius, Flat); off-plane or
   upward photons are dropped. Its power is the light falling on the ball (strength × falloff × cone, as
   `hit_light`) × πr² / photons × See-through × 0.92 (what the two surfaces let through) × the glass's colour ×
   Glass caustics, spread as a normalised soft disc (radius `2r√(π/N)`, at least 1.5 px, wider by Frost) over the
   receiver, drawn on screen where that point shows (`y + z·k`), skipped where a ball covers the pixel (the surface
   picture's height). Photons are added into the direct-light picture before the background-light copy, so Flat
   pixels get them through the direct light and Stage/shiny pixels through `bg_glow`. Photon places depend only on
   the index (and the sample), so a still scene's caustics stand still. Not done (stated): photons blocked by other
   balls on the way, caustics from glass shapes (Lens) and from mirrors.
9. **Rainbows and blurred reflections when baking.** `transmitted` with Rainbow split takes one wavelength a sample
   (spectral colour × the bent picture) instead of three taps; `shade`'s scene reflection turns the mirror ray by a
   random amount growing with roughness² and traces it for any roughness (2b's sky above 0.35 is live only). The
   trace's bounce cap goes from 8 to 16.
10. **Settings** (Lighting card, Quality group, after Reflection depth): On export (`OnExport` Same as live / Bake,
    default Same as live: nothing changes for old scenes), then with Bake: Samples (`bake_samples`, range(1, 1e9):
    zero samples draw nothing, span 1..64, default 16), Light bounces (`light_bounces`, range(0, 1e9), span 0..6,
    default 2), Bake reflections (`bake_reflections`, range(1, 1e9), span 1..16, default 8), Motion blur (default
    on), Shutter (range(0, 1), reason above, default 0.5, "of a frame"), Noise clean-up (default on); and for live
    and bake Glass caustics. The bake settings are not keyframes (a video bakes one way). `LightingGpu` gains
    `bake: [samples, light bounces, bake reflections, shutter (0 without motion blur)]` and `bake_more: [1 with Bake,
    1 with clean-up, glass caustics, 0]`. `bpe_kit::modules::lighting::bake_of(scene, registry) -> Option<Bake>` reads
    the card for the exporter.
11. **Hardware ray tracing: not used (stated).** wgpu's ray queries trace triangles; this scene is analytic spheres,
    planes and a distance field, which the sphere grid already traces exactly; a hardware path would need triangle
    proxies and could not be tested in the cloud (no ray-query adapter). The spec makes it optional with the picture
    the same either way, so nothing is lost but speed; no setting is added for it.
12. **Depth of field when baking** is 2b's Stage depth of field (it already blurs by depth), not a lens sampled per
    sample (stated): the stage is an orthographic front view with no lens to sample.
13. **Export**: `ExportOptions.bake: Option<bool>` (None = the card's On export); a scene with no Lighting card
    always renders live (nothing to bake; `render`, byte-identical). `bpe_cli::FrameMaker` (world, log, follow,
    light probe) gives a frame's sample frames at a moment for export, Preview final frame and Test a frame alike.
    The progress bar counts frames as before.
14. **Editor**: the Lighting card shows "Preview final frame" under its grid; the Inspector sets
    `final_frame_wanted`, the app starts `final_frame::FinalFrame` (a thread with a headless device and its own
    renderer, like the export) at the playhead with the export's size, and a window shows the picture, "Worked out in
    N s", and "The whole video (F frames) would take about M". The export window's Lighting row picks Live / Bake
    (starting from the card's choice), says the bake's settings, and Test a frame opens the same window.

## Files and types

### bpe-runtime
- `world.rs`: `World::set_between(&mut self, a: f32) -> Between` (saves positions, sets each live ball to the lerp)
  and `World::restore_between(&mut self, Between)`.

### bpe-gfx
- `bake.rs` (new): `BakeSample { index, count }`, `Bake { samples, shutter, bounces, reflections }`,
  `moments(time, fps, rate, &Bake) -> Vec<Moment { time, step, between }>`, `jitter(i) -> [f32; 2]`, Halton.
- `frame.rs`: `Frame.bake: Option<BakeSample>`; `LightingGpu.bake`, `bake_more`.
- `lib.rs`: `render` split into `render_scene` + `finish`; `render_samples`; `accumulate` pipelines and the
  `accum` pair (Rgba32Float); `effect_time` on the chain during samples.
- `effects/chain.rs`: `effect_time: Option<f32>`.
- `lighting.rs`: Uniform `sample`; quality code 4; `runs` from bounces; caustics (layout group 3, `CausticUniform`,
  `CausticGlass`, `vs_caustic`/`fs_caustic` pipeline, additive into direct); trace (`fs_trace` into `traced[0]`,
  `fs_denoise` ping-pong, `fs_trace_add` into `bg_glow`).
- `shaders/lighting.wgsl`: `sample` in `Lighting`; RNG; spectral colour; `scene_hit`, `bounce_light`, `fs_trace`,
  `fs_denoise`, `fs_trace_add`; caustic vertex/fragment; bake branches in `shade` and `transmitted`; trace cap 16.

### bpe-kit
- `modules/lighting.rs`: the settings, `OnExport`, `gpu()` fills `bake`/`bake_more`, `bake_of`.
- `scenes.rs` (test-only): `bake_demo` (Stage: glass, chrome and red balls, a spot lamp, a fast ball) for the tests
  and the video.

### bpe-cli
- `lib.rs`: `ExportOptions.bake`, `FrameMaker`, `bake_frame` (one frame's pixels and seconds), the export loop
  through `render_samples`. `main.rs`: `--bake`, `--live`.

### bpe-studio
- `final_frame.rs` (new), `inspector.rs` (the button), `export_window.rs` (Lighting row, Test a frame), `app.rs`
  (wiring).

## Tests (what passing looks like)

- `bpe-gfx` unit: `moments` (n moments, increasing, last = the frame, first `s/f·(n−1)/n` earlier; shutter 0 all
  at `t`), `jitter` (in −0.5..0.5, first at 0, spread), quality code 4.
- `bpe-runtime`: `set_between` moves a falling ball to the lerp and `restore_between` puts it back bit for bit; the
  world then steps exactly as one never touched (fingerprint equal).
- `bpe-kit/tests/bake_pictures.rs` (GPU):
  - one sample equals `render` byte for byte (lit scene);
  - a still lit scene baked with 8 samples (no blur) keeps its mean brightness within 2 % and its edges get more
    in-between pixels (anti-aliasing);
  - motion blur: a fast ball baked with Shutter 0.5 covers a longer streak along its motion than across, Shutter 0
    a round ball;
  - caustics: a Stage glass ball under a lamp makes the wall brighter in a spot than Glass caustics 0 does; with
    Rainbow split the spot's red and blue peaks sit apart;
  - bounce: Stage, a red ball by a white wall, Bounce light 1, baked: the wall near the ball redder than with Bounce
    light 0;
  - baking the same frame twice gives the same bytes;
  - an effect with memory (Echo) baked looks as it does live after the same frames (within a small difference).
- `unlit_pictures`, `lit_flat_pinned`, `stage_pictures`, `glass_pictures` and the physics and sound goldens are
  unchanged (Glass caustics adds light only round glass balls; if a glass test moves, its expectation is checked and
  the test scene says Glass caustics 0 where caustics aren't its subject).
- `bpe-cli`: options parse `--bake`/`--live`.
- Editor kittest: the Lighting card shows Preview final frame and the bake rows with On export Bake.

## Things not to do

- No change to any picture without Bake or glass caustics (live shaders' random numbers only when baking).
- No physics change: `set_between` is undone before the next step; goldens untouched.
- No hard limits but Samples ≥ 1, Light bounces ≥ 0, Bake reflections ≥ 1 and Shutter 0..1 (reasons above).
- No built-in scenes; `bake_demo` is test-only.
- No new colours outside `style/`.

## As built (changes after review and the demo)

- **Bounced light** (`bake_bounce`: Like live / Traced, `bake_more[3]`), default **Like live**. The demo showed a
  Stage bake with the path tracer replacing the light field's bounce coming out much darker than the live picture
  (a flat wall can't light itself, so the traced bounce is far weaker than the field's in-plane spread). Like live
  keeps the field's bounce exactly as live, so the bake is the live look smoothed; Traced is decisions 6 and 7 as
  planned, and Light bounces shows only with it.
- The bake also averages the unlit picture and lit colour (effects that take Light only), Preview final frame warms
  up only as long as the scene remembers (at the bake's quality) and can be stopped, and a ball drawn between steps
  leaves out the trail ahead of it (`Balls.ahead`).
