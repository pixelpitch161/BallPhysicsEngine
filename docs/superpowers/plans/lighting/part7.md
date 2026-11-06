# Lighting part 7: light sensing (optional, one-way)

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md`, "6. Light sensing (optional, one-way)": Follow light,
Light you can hear (`sound.light`), light-sensitive balls, shadow switches, Moths and light pressure, all reading a
CPU light probe `LightAt`. Builds on 1a (Light things, `LightDraw::light`, the Lighting card), 1b (Gives off light)
and 6 (rigs and lighting scenes drive the lights). Built straight after this plan (owner's overnight rule: no stops),
one Opus review at the end, then the PR, merged by this thread once tests and the review pass.

**One-way, off by default.** Nothing here runs unless a scene uses it: a Follow light pulse, a setting varied by
Light or Shadow, a Motion card with Moths on, or a `sound.light` modulator. The renderer never reads any of it, the
probe never reads itself, and with none of it in a scene every picture, physics golden and sound golden is
unchanged. The only new goldens are the Moths scenes' (`crates/bpe-kit/tests/golden-moths*.txt`).

## What part 7 delivers

- **The light probe** (`bpe-runtime/src/light.rs` the traits, `bpe-kit/src/sensing/` the probe): how much light
  falls on a point, a ball or a shape at a moment, from every Light thing (as the renderer gets it: riding, rigs,
  lighting scenes, flares) and every ball or shape whose look gives off light, with analytic shadows from balls and
  shapes, and the scene's ambient light. No bounce, no light field, no haze: the probe is the direct light only.
- **Follow light** (`PulseSource::Light`): right-click a setting ▸ Follow light ▸ a thing ▸ Light on it / Brightest
  light on it / Shadow on it / In shadow (a switch), or ▸ a light ▸ Its strength. Beside Follow sound. Works on look
  settings (the picture) and on physics settings (read at the end of the previous step, like Follow sound).
- **Light-sensitive balls and shadow switches, ball by ball**: two new spreads for Vary across balls, **Light** (how
  much light falls on each ball) and **Shadow** (how much of it its shadows take), on look layer settings and on the
  Motion and Bounce cards. With Follow light ▸ In shadow they give the shadow switches.
- **Moths and light pressure**: a **Light sensing** fold on the Motion card (shown in the Physics card): Moths Off /
  Drawn to light / Pushed by light, Pull, Near (a soft core so they circle the lamp), Shadows hide light.
- **Light you can hear**: a sound modulator `sound.light` ("Light") beside LFO, Distance and Hit: how much light falls
  on the balls it hears (Average / Brightest ball / Darkest ball / How many lit / Each note's own ball), with Dark and
  Bright ends, Curve, Smooth, Invert, Reads Light or Shadow.

## Decisions made while planning (each a fill-in of the spec, stated here)

1. **The probe is the renderer's lights, worked out on the CPU.** The kit builds a `LightScene` exactly as a frame
   does (`LightingSettings::gpu()` then `LightDraw::light` for every Light thing, through a `DrawCx` with the probe's
   own resolved scene), so riding, rigs, lighting scenes, flares and Strip outlines all match the picture. It then adds
   up each lamp's light at a point the way `lighting.wgsl::lamps` does on a flat white surface (shade 1: no normal),
   with `falloff`, `source_point`, the cone, the shadow tint `mix(1, shadow colour, (1 − v) × shadow strength)` and
   the Shadows style (Off: no shadows) and Length (how far behind a blocker a shadow reaches).
2. **What casts shadows in the probe**: every alive ball of a thing whose Look's Body blocks light (`blocks_light`
   on the base layer; default on) as a circle, and every shape whose body blocks light as its outline's centre line
   with half its thickness (a filled shape as its outline: a ray across it is blocked where it crosses the edge,
   which is the same for any point outside it). Visibility is analytic: for a circle at distance `t` along the ray
   whose centre misses the ray by `m`, with penumbra half-width `w = soft × t` (`soft` the lamp's size over its
   distance times Softness, as the shader's `angular × softness`; Hard and Toon use 0.5 px), `v = smoothstep(r − w,
   r + w, m)`; a segment band crossed is `v = 0`. A ray starts past the ball being asked about (it never shadows
   itself) and stops `stop` short of the lamp as the shader does (a ridden ball's radius plus the lamp's size).
   Balls are put in a uniform grid (cell twice the median radius, at most 256×256 cells) walked by DDA, so crowds
   with Moths stay quick.
3. **Glowing things are lamps to the probe.** A ball whose look has a shown Gives off light sub-layer on a drawn
   layer (or its Body) is a Point lamp at its middle: colour = the Emit's own colour + its tint × the layer's own
   colour when it has one (`colour` setting, else white), Real falloff, reach the Lighting card's Glow reach, size
   its radius, it casts shadows. A shape that gives off light is a Strip lamp along its outline. The light field's
   gain (`GI_GAIN` 2) is not modelled: a glowing ball of Strength 2 reads like a lamp of Strength 2. Trails,
   particles, the wavefront and the background's glow are left out (they are many small or spread-out sources the
   probe can't see cheaply); the plan says so to the owner in the PR.
4. **A light amount `L`** at a point is the luminance (0.2126 r + 0.7152 g + 0.0722 b) of ambient plus every lamp,
   in the shader's units (1 = the picture as if unlit). Pulses and spreads read a **level** `1 − e^(−L)` (0 in the
   dark, 0.63 at L = 1, 0.95 at L = 3; negative light reads 0): smooth, never stuck at 1 under bright lamps.
   - **Light on** a thing: the mean level over its alive balls (a Balls or Emitter thing), 16 points round a shape's
     outline (the shape itself not blocking), or its place (anything else: a light's own place reads its own light).
   - **Brightest light on** a thing: the level of the single brightest lamp (no ambient), the most over its balls.
   - **Shadow on** a thing: `1 − lit / unshadowed` over its balls (0 where no lamp reaches).
   - **In shadow** (the switch): the share of its balls whose Shadow is over 0.5 (one ball: 0 or 1).
   - **Its strength** (a Light thing): the light's strength as it shines now (rigs, scenes, flare) over its own
     Strength setting, 0..1 (a light set to 0 reads 0). For one riding each ball, the brightest of its lamps.
5. **Spreads** `VaryBy::Light` and `VaryBy::Shadow` (appended to `VaryBy`, so saved names stay): `BallSpread.light`
   and `.shadow` (0..1, the ball's level and its Shadow). They `changes()` (read each step), so they are not offered
   where a value is read once at launch, and **not for a thing's own settings** (Balls Size, Emitter Size: those are
   worked out by the thing's sim, which has no probe): `VaryBy::offered` gets a `lit` flag the editor sets only for
   card and layer settings in a scene with a Lighting card. A ball's spread is worked out with the probe only when
   some varied setting spreads by Light or Shadow (`Rows::needs_light`; the draw's `spread_of` asks the probe only
   when `cx.light` is there).
6. **Physics reads light as it was at the end of the previous step**, like Follow sound: `World::animate` passes
   `Inputs.light` from the probe on the world and log as they are; Moths pull is worked out once a step, after the
   Spawn stage, for every alive ball, and added in every substep's move (`World.pulls`). Exact in editor and export.
7. **Moths** (Motion card, group "Light sensing", key prefix `moth_`): `moths` (Off / Drawn to light / Pushed by
   light), `moth_pull` (px/s² at light amount 1, span 0..4000, default 1200), `moth_near` (px, span 0..300, default
   60: the pull is scaled by `d² / (d² + near²)` so a moth slows into an orbit rather than sticking), `moth_shadows`
   (default on: a lamp hidden by a shadow pulls only as much as its light gets through). The pull of each lamp is
   along the way to its `source_point` (a Sun's: towards it, `-direction`), times that lamp's light amount at the
   ball (falloff × cone × visibility, its colour's luminance). A ball's own glow never pulls it. All three numbers
   keyframe, pulse and vary; no hard ranges.
8. **When a world has the probe**: `World::build` makes it (`Sims.light`'s `LightMaker::make`) when a physics
   setting follows light, a Motion or Bounce card spreads by Light or Shadow, or a Motion card has Moths on. Then the
   physics fingerprint also hashes `LightMaker::fingerprint` (every Light and Rig thing's settings and tracks, the
   Lighting card, the Look cards of balls, emitters and shapes with their tracks: what the probe reads), so a look
   edit restarts the run only in such a scene.
9. **The renderer never reads sensing.** The probe resolves its own copy of the scene with `Inputs.light` = `None`
   (it never reads itself). Follow light is not offered (and `sensing::can_follow_light` refuses it) on what the
   lighting reads: Light and Rig things, the scene's Lighting card, the scene settings' `bg_glow*` keys, Gives off
   light sub-layers and every layer's Lighting keys (`lit`, `blocks_light`, `bulge`). The editor's edit path checks
   it too.
10. **The draw** gets the probe through a new `DrawCx.light` (`None` everywhere but frames built with
    `build_frame_sensing`, which `build_frame_with` calls with no probe). The editor (`Studio.light_probe`) and the
    renderer (`bpe-cli`) make one when `sensing::uses_light(scene)`, like `Studio.follow` for sound.
11. **`sound.light`** is a fourth modulator kind, built like Distance (`MAX_OF_EACH` 8, shared or per note for Each
    note's own ball, knob places `k::light(i, key)`, routes, Follow sound, the Sound tab's row). It reads the probe
    once a block from the world and log the engine already steps (`ModCx.light`, `ModCx.log`); the patch makes the
    probe when the card has a Light modulator that is on. The probe allocates when it works out the lamps for a new
    step (once a step, not per block); the counting-allocator test's patch has no Light modulator.
    Settings: Reads (Light / Shadow), Follows (Average / Brightest ball / Darkest ball / How many lit / Each note's
    own ball), Heard from (default every Balls thing), Dark (level that reads 0, span 0..1, default 0.1), Bright
    (reads 1, default 0.8), Lit at (How many lit: level counted as lit, default 0.5), Full at (How many lit: count,
    default 10), Curve (-100..100), Smooth (ms, default 60), Invert.

## Files and types

### bpe-core
- `animation.rs`: `PulseSource::Light { thing: ThingId, source: LightSource }`; `LightSource { On, Brightest,
  Shadow, InShadow, Strength }` (`name()` words); trait `LightLevels { fn level(&self, thing, &LightSource, time) ->
  f32 }`; `Inputs.light: Option<&dyn LightLevels>`; `pulse_strength` reads it; `PulseSource::follows_light()`.
- `vary.rs`: `VaryBy::Light`, `VaryBy::Shadow` (appended), `BallSpread.light/.shadow`, `t` for them, `changes()`
  true, `VaryBy::reads_light()`, `VaryBy::offered(use_, lit)`.
- `lib.rs` exports.

### bpe-runtime
- `light.rs`: `LightAt` (`level`, `ball`, `pull`), `LightMaker` (`make`, `fingerprint`), `LightLevelsAt` (core's
  `LightLevels` at a world and log), `LightPull { strength, near, shadows }`, `level_of(L)`.
- `plugins.rs`: `Sims.light: Option<Arc<dyn LightMaker>>`; `ModuleSim::light_pull(state) -> Option<LightPull>`.
- `rows.rs`: `Rows::needs_light()`.
- `world.rs`: `World.light`, `World.pulls`; `needs_light`; build makes the probe; `animate` passes light inputs;
  `step` works out the pulls; `move_bodies` adds them; `ball_spread_with(…, light)`; `refresh_rows` asks the probe
  only for rows that need it; `lone` and clone keep the probe (not the pulls); fingerprint hashes the maker's.

### bpe-gfx
- `frame.rs`: `DrawCx.light: Option<&dyn LightAt>`; `build_frame_sensing(…, sound, light, frame)`; `Inputs.light`
  from `LightLevelsAt` when a probe is given.

### bpe-kit
- `sensing/mod.rs`: `KitLight` (the `LightMaker`), `uses_light(scene)`, `can_follow_light(scene, reg, address)`,
  `lighting_reads`, `probe(scene, resolved) -> Arc<dyn LightAt>`.
- `sensing/probe.rs`: `Probe` (scene, resolved, animation; a per-moment cache of `Lamps`), `Lamps` (lamps, blockers,
  ambient, style), `Lamp`, `Blockers` (balls grid, wall segments), `sample`, `visibility`, levels and pull.
- `modules/motion/`: the Light sensing settings and `light_pull`.
- `modules/look/layers/vary.rs::spread_of`: fills light and shadow from `cx.light` when the look needs them.
- `sound/mods.rs`: `LIGHT_MOD` `sound.light`, `LightModSettings`, `LightReads`, `LightFollows`; in `MOD_KINDS`.
- `lib.rs`: `sims()` sets `light`; `scenes.rs`: `moth_scenes()` (test-only).
- `tests/golden.rs`: Moths goldens (`golden-moths.txt`, `-debug`).

### bpe-sound
- `mods/light.rs`: `LightParams`, `light_value`, `LightMod`; `mods/mod.rs` `ModCx.light`/`log`.
- `voices/knobs.rs`: `k::light(i, key)` places; `voices/shape.rs`: `LightSpec`, `Src::Light`, per note and shared;
  `patch.rs` builds them and the probe; `control.rs` (Follow sound) reads them too.

### bpe-studio
- `follow.rs`: the Follow light menu (`FollowMenu.light`), words, colour, `now` for light pulses.
- `widgets.rs`: Follow light ▸ … items under Follow sound, and its Stop entry; the band says "Follows light on …".
- `vary.rs`: Light and Shadow offered by the `lit` rule.
- `app.rs`: `Studio.light_probe` made by `sensing::uses_light`; `preview.rs` passes it to `build_frame_sensing`.
- `sound_tab/mods/`: the Light modulator tile, its page (settings grid) and "+ Light".
- `timeline/`: a pulse lane following light says what it follows.

### bpe-cli
- Frames draw with `build_frame_sensing` and the probe when `uses_light`.

## Tests (what passing looks like)
- core: `pulse_strength` reads light levels; spreads Light/Shadow; `offered` without `lit` leaves them out.
- runtime: a fake `LightAt` drives a physics setting (Follow light) and Moths-like pulls; no probe without use.
- kit probe: a lamp lights a point by its falloff; a ball between lamp and point casts a shadow (Shadow ≈ 1), a ball
  beside the ray doesn't; ambient alone reads its level; a glowing ball lights its neighbour; a rig chase changes
  Its strength; Shadows Off leaves no shadow; the probe matches the picture's light (lit pictures: a pixel's
  lit-only value grows where the probe's level grows). Moths: balls end nearer the lamp than without; Pushed by
  light ends farther; the Moths goldens; the physics goldens and sound goldens unchanged; `unlit_pictures` unchanged.
- sound: a Light modulator reads more when a lamp is near the balls, routes move a knob, Follow sound reads it.
- studio: the Follow light menu lists things and lights only with a Lighting card; a lighting-read setting doesn't
  offer it; Vary offers Light and Shadow on a look layer but not on Size.

## Things not to do
- Never let the renderer or the probe read Follow light, a light spread, or Moths.
- Never make the probe run in a scene that doesn't use it (no world, frame or patch builds one).
- Don't move existing physics or sound goldens, or any unlit picture.
- No hard `range` on new settings; spans only.
- Don't add built-in scenes; Moths scenes are test-only.
