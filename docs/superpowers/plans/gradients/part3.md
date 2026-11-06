# Gradients part 3: movement

Spec: `docs/superpowers/specs/2026-11-06-gradients-design.md` section 4 (approved by Aedan 2026-11-06). Part 1 (#121)
built the gradient value, the companion mechanism (`bpe-core/src/gradient_extras.rs`, keys `<key>~<name>`, the
`apply_moves` pass after `Animation::apply` in `bpe-gfx/src/frame.rs::build_frame_sensing` and the light probe), Bands,
Grain and the tabbed row (`bpe-studio/src/gradient_tabs.rs`). This part adds the eight moves as Move-tab companions,
the pass that applies them, Hit wave on the GPU and the CPU, Morph's second gradient and the Move tab. Parts 2 (shapes)
and 4 (big editor) build at the same time: keep to this scope and expect to merge main before the PR.

Rules that hold throughout: physics and sound goldens do not change; a gradient with every move at its default draws
exactly as today (the moves are skipped, nothing is re-hashed); every move is pure in time (settings, time, tempo and
the log only, no frame-to-frame state); no caps beyond the hard ranges named here; design-system tokens only; every new
control named for kittest.

## Decisions made while planning (beyond the spec)

1. **Moves are carried on the value, applied when it is sampled.** Like part 1's grain, `Gradient` gains one draw-time
   field `moves: Moves` (`#[serde(skip)]`, never saved or edited, set only by `gradient_extras::apply_moves`). It
   holds `shift` (Cycle and Step, in gradient lengths), `breathe` (a stretch around the middle, as a natural log: the
   stops spread by `e^breathe`), `hue` (turns, Hue spin), `morph: Option<Box<Morph { to, amount }>>` and
   `wave: Option<HitWave>`. `Gradient::sample` honours all but the wave, so `bake` (the GPU atlas), `banded`, CPU
   colours, light colours and the probe see the moved gradient with no change at any draw site, exactly in every Mix
   and gap blend. Resampling the moved gradient into a new stop list was rejected: it can't keep Eased/Steps gaps and
   OKLCH mixes exact. Moving the stops themselves was rejected for Cycle on an unlooped gradient (its cut would need a
   looped value, which changes Order's spacing).
2. **Sampling order** (`sample(t)`): `v = t − shift` (an unlooped gradient that cycles wraps with `fract`: "without
   Loop the end cuts"); `w = 0.5 + (v − 0.5)·e^(−breathe)`; colour `A(w)` (the gradient's own clamp or wrap); Morph
   mixes `A(w)` with `To(w)` by `amount` in A's Mix (`mix_linear`); Hue spin turns the result's OKLCH hue by `hue`
   turns (greys stay grey; gamut fitted as part 1 does). `Moves::default()` skips all of it, so still gradients sample
   bit-exactly as before.
3. **Stops wander** moves the stops themselves in the pass (it is about each stop): stop `i` moves by
   `amount·n_i(time)` (smooth seeded value noise in −1..1, `wander_noise(seed, i, time·speed)`) of half the gap to the
   neighbour on that side (cyclic when looped; an end stop of an unlooped gradient moves only inward). Every stop moves
   from its own place by at most half of each gap, so none ever crosses a neighbour. Amounts past 1 act as 1 (the
   spec's "never crossing a neighbour" is the limit); negative amounts flip the drift.
4. **Step on the beat** shifts by whole stop spacings: `spacing = 1/n` looped, `1/(n−1)` not (one stop along when the
   stops are even). At beat-step `k = floor(time / (every·beat))` the shift is `k − 1 + ease(g)` spacings with `g =
   (time − k·every·beat) / glide` clamped (smoothstep), 0 for `k = 0`; glide 0 jumps. It adds to Cycle's shift.
   "Every" is in beats; a bar is "beats in a bar" beats, typed as that number.
5. **The beat** is read inside `bpe-core` from the resolved root's `tempo`, `time_top` and `time_bottom` (the scene
   settings, keys as `SceneSettings` has them), the same sums as `bpe-kit/src/things/scene/draw.rs::beat_at`
   (`gradient_extras::Beat::of(resolved)`), so `apply_moves(resolved, time)` keeps its signature and both call sites
   stay as they are. A kit test checks it against `beat_at`. Missing or bad values read as 120 BPM, 4/4.
6. **Breathe**: `breathe = amount · sin(2π·phase)`; phase = `time · speed` or, with "Breathe every" > 0 beats, `time
   / (every·beat)`. Amount has no range (big amounts squeeze to a line and spread past the ends).
7. **Hue spin**: `hue = turn/360 + speed·time` turns, reduced mod 1 in f64 before f32 so long clips keep precision.
   The same for Cycle's shift.
8. **A one-colour gradient** with moves is resolved in the pass to its moved colour (`sample(0)` into its one stop,
   moves cleared but the wave): many draw sites read a solid colour with `first()` and would otherwise miss Hue spin and
   Morph. The editor still shows no tabs on one colour (part 1's decision 14 stands: the tab row on every solid colour
   would crowd the inspector); its companions keep working when keyed, pasted or copied.
9. **`step_stops`** (Step colours, light show colour lists) gives each stop its moved colour (`sample(at)`) when the
   gradient moves, so stepping and light shows follow Hue spin and Morph.
10. **Bands come after the moves** (spec): `banded` samples the moved gradient and returns a still one (moves cleared,
    wave, grain and Loop kept).
11. **Hit wave on the GPU through the atlas**: the wave is a per-gradient fact, so it travels with the gradient's atlas
    row, not in `PaintGpu` (which part 2 is widening for the forms; keeping out of it avoids a clash and touches no
    draw site). `GradientAtlas` keeps a wave per row (`waves()`, two texels: strength, width, speed, 1; and the colour
    in linear light); the renderer uploads them to a second texture in the paint bind group (binding 2, `Rgba32Float`,
    2 × rows, read with `textureLoad`). The atlas hash eats the moves and the wave only when they are not still, so
    every existing row hash stays. `paint.wgsl` gains `var<private> paint_hit: vec4<f32>` (the instance's
    `[age, strength, hits so far, heat]`), set beside `paint_px` in `shade_layers` and `shade_path` (the only entry
    points with an instance hit); elsewhere it stays 0 and no wave shows.
12. **The wave's shape**: front at `speed·age` along the gradient (from 1 back when Speed < 0), a smooth bump
    `(1 − (d/width)²)²` for `|d| < width` measured on the gradient's own place `t` (after the paint's start, scroll and
    repeat); `i = strength·bump`; colour `mix(c, wave, min(i, 1)) + wave·max(i − 1, 0)` (past 1 it adds light, no
    cap), applied after the instance's variation and before grain. It doesn't scale with the hit's strength, so the CPU
    (which has no strength for a hit) and the GPU agree. A ball never hit (age `NO_HIT`) shows none.
13. **Which hits**: a ball's last hit as its layers' Flash on hit reads it (`last_hits`, within a memory long enough
    for the wave to pass: `(1 + width)/|speed|`, 10 000 s when Speed is 0); a shape's last hit by a ball
    (`shape_hits`). `Stack::needs_hits`/`ShapeStack::needs_hits` also answer true when a gradient in the card's
    params has a wave (`Stack.wave_memory`, `ShapeStack.wave_memory`). The background, trails, particles and lights
    have no instance hit: Hit wave does nothing there (its help says "on balls and shapes").
14. **The CPU wave**: `paint::Inputs` gains `hit_age: Option<f32>`; `colour_at` applies the wave at `offset(p, i)`
    when it is set. `BallInputs` and `WallInputs` look up the last hits when a paint steps on hits **or** has a wave,
    and set `hit_age` only for a wave on a piece that has been hit.
15. **Morph's To** is a Gradient companion `~morph_to`, default cyan to magenta (Mix Even). A companion gradient
    has no companions of its own (`split` refuses a base that is itself a companion) and no tabs. Its row shows in
    the Move tab's Morph fold as an ordinary gradient row (keys, paste, palettes work).
16. **Hit wave colour** is a Color companion `~wave_colour`, default white.
17. **Shape moves**: the forms' own Flow and Speed are part 2's settings (they move by themselves). This part adds the
    one move every form shares, `~spin` ("Shape spin", degrees a second), which the pass adds onto the `~turn`
    companion (part 2's, read as stored or 0) so any form turns. Until part 2 lands it writes `~turn` and nothing
    reads it.
18. **Move tab folds**: each move's companions share their `SettingDef.group` (Cycle, Step on the beat, Breathe, Hue
    spin, Stops wander, Hit wave, Morph, Shape moves). The Move tab draws a fold heading per group (part 1's
    `widgets::group_heading`, its dot when shut and something inside is changed, keyed or pulsing) with its open state
    as session memory under the bar's id; all start shut. Companion labels carry the move's name ("Cycle speed") so the
    timeline lane reads "Colours cycle speed".
19. **Pass order**: Wander (stops), then shift (Cycle + Step), breathe, hue, morph set on `moves`; Shape spin; then
    the one-colour resolve (decision 8); then Bands; then Grain and the wave. The `~to` of a varied gradient gets the
    same (as part 1 does for Bands).

## The companions (`bpe-core/src/gradient_extras.rs`, Move tab)

| Fold | Key | Label | Type, default | Span (hard range) | Unit |
| --- | --- | --- | --- | --- | --- |
| Cycle | `~cycle` | Cycle speed | Number 0 | −2..2 | /s (cycles) |
| Cycle | `~cycle_shift` | Cycle shift | Number 0 | 0..1 | |
| Step on the beat | `~step_every` | Step every | Number 0 (off) | 0..8 (0..) | beats |
| Step on the beat | `~step_glide` | Step glide | Number 0.15 | 0..1 (0..) | s |
| Breathe | `~breathe` | Breathe amount | Number 0 | 0..1 | |
| Breathe | `~breathe_speed` | Breathe speed | Number 0.5 | 0..4 | /s |
| Breathe | `~breathe_every` | Breathe every | Number 0 (use speed) | 0..8 (0..) | beats |
| Hue spin | `~hue_speed` | Hue spin speed | Number 0 | −1..1 | turns/s |
| Hue spin | `~hue_turn` | Hue turn | Number 0 | −180..180 | ° |
| Stops wander | `~wander` | Wander amount | Number 0 | 0..1 | |
| Stops wander | `~wander_speed` | Wander speed | Number 0.5 | 0..4 | /s |
| Stops wander | `~wander_seed` | Wander seed | Number 0 | 0..100 | |
| Hit wave | `~wave` | Hit wave strength | Number 0 | 0..2 | |
| Hit wave | `~wave_width` | Hit wave width | Number 0.15 | 0.01..1 (0..) | |
| Hit wave | `~wave_speed` | Hit wave speed | Number 2 | −8..8 | /s |
| Hit wave | `~wave_colour` | Hit wave colour | Color white | | |
| Morph | `~morph` | Morph amount | Number 0 | 0..1 | |
| Morph | `~morph_to` | Morph to | Gradient cyan→magenta | | |
| Shape moves | `~spin` | Shape spin | Number 0 | −180..180 | °/s |

Numbers keyframe and pulse; the colour and gradient keyframe. Each has a plain help sentence.

## Task 1: the value (`crates/bpe-core/src/color.rs`)

- `pub struct Moves { pub shift: f32, pub breathe: f32, pub hue: f32, pub morph: Option<Box<Morph>>, pub wave:
  Option<HitWave> }` (Clone, Debug, Default, PartialEq); `pub struct Morph { pub to: Gradient, pub amount: f32 }`;
  `#[derive(Copy)] pub struct HitWave { pub strength, width, speed: f32, pub colour: Color }`. `Moves::is_still()`
  (no shift, breathe, hue or morph; the wave doesn't change sampling).
- `Gradient.moves: Moves` (doc: draw-time only). Every `Gradient { .. }` literal gains `moves: Moves::default()` (or
  `..`). Serialize unchanged; Deserialize gives the default.
- `pub fn mix_linear(a: [f32;4], b: [f32;4], u, mix) -> [f32;4]`; `mix_colours` calls it with `to_linear()`.
- `sample` as decision 2 (the old body becomes `sample_still`). `banded` samples the moved gradient and clears the
  moves but the wave. `step_stops` as decision 9. `pub fn hue_turned(c: [f32;4], turns) -> [f32;4]` (OKLCH).
- `pub fn wandered(&self, amount, speed, seed, time: f64) -> Gradient` and `fn wander_noise(seed: u32, i: usize, x:
  f64) -> f32` (PCG hash of lattice points, smoothstep between, −1..1).
- `animation.rs` `Value::lerp` keeps `a`'s moves.
- Tests: still gradients sample exactly as `sample_still`; shift on a looped gradient equals `sample(t − d)`; shift on
  an unlooped one wraps with a cut; breathe puts stop `x` at `0.5 + (x − 0.5)·e^b`; hue turns red's OKLCH hue and
  leaves grey; morph 0/1/0.5; banded after moves is flat, still and carries the moved colours; wander never crosses
  (1000 times × 50 random gradients), is the same at the same time, moves with time, changes with seed, ends of an
  unlooped gradient stay inside; step_stops gives moved colours.

## Task 2: the pass (`crates/bpe-core/src/gradient_extras.rs`)

- The companions of the table, `group` set to the fold; `split` refuses a base that is a companion.
- `pub struct Beat { pub seconds: f64, pub bar: u32 }` with `Beat::of(resolved)` (decision 5).
- `apply_moves(resolved, time)`: gathers every companion of each base (a `Found` with a value per name, absent =
  default), then decision 19 for the base and its `~to`. Pure: reads only params, time and the beat.
- `pub fn wave_memory(params: &Params) -> f32`: the longest any gradient in `params` with a wave needs its last hit
  (decision 13), 0 when none. (Reads the gradients' `moves.wave`, so call it on moved params.)
- Tests: each move at a few times (Cycle shift = speed·t + shift mod 1; Step at 120 BPM every beat: 0 before 0.5 s,
  half way through the glide, one spacing after; Breathe on the beat; Hue; Morph sets `to`; Wander moves stops; Spin
  adds to `~turn`); the pass twice at one time gives equal params and scrubbing (3 s, 1 s, 3 s) gives equal results;
  everything at its default leaves the gradient equal to the input; a one-colour gradient with Hue spin comes out one
  turned stop; `~morph_to~bands` is not a key; `Beat::of` reads tempo/time signature; a Follow sound pulse on
  `colours~cycle` (a fake `SoundLevels`) moves the shift.

## Task 3: the renderer (`bpe-gfx`)

- `gradients.rs`: `GradientAtlas.waves: Vec<[[f32; 4]; 2]>` filled by `row`, `pub fn waves()`; `adopt` adds a still
  wave; `hash` eats `moves` (shift, breathe, hue, morph's `to` hashed recursively and amount) and the wave only when
  not still/none. `pub const NO_WAVE`. `paint_layout` gains binding 2 (`Float { filterable: false }`).
- `lib.rs` `PaintTexture`: a second texture "gradient waves" (`Rgba32Float`, 2 × rows), in the bind group at 2,
  written in `sync`.
- `paint.wgsl`: `@binding(2) var paint_waves: texture_2d<f32>;`, `var<private> paint_hit`, `paint_t(p, along,
  time)` (the place `paint_sample` uses), `paint_wave(p, t, c)` (decision 12), `paint_along` = sample → vary → wave →
  grain.
- `layers.wgsl::shade_layers` and `paths.wgsl::shade_path` set `paint_hit = in.hit;`.
- Tests: atlas: a still gradient's row hash and bake unchanged; moves and waves give new rows; `waves()` holds the
  numbers. GPU (`paint_tests.rs` style): a layered ball painted with a wave gradient, hit age so the front sits in the
  middle, is brighter there than with `NO_HIT`, and identical to the no-wave picture with `NO_HIT`.

## Task 4: the kit (`bpe-kit`)

- `paint/eval.rs`: `Inputs.hit_age`, `colour_at` applies the wave (shared `pub fn wave_colour(c, w: &HitWave, t,
  age)` in core so CPU and GPU sums are written once each).
- `modules/look/draw.rs`: `BallInputs::new`/`get`, `WallInputs::new`/`get` (decision 14).
- `modules/look/layers/gpu.rs` `Stack.wave_memory` from `gradient_extras::wave_memory(&slot.params)`;
  `needs_hits` and `flash_memory` use it. `modules/look/shape/gpu.rs` `ShapeStack` the same.
- `paint/gpu.rs::background_gpu`: the exact two-colour path also needs `moves.is_still()`.
- Tests: `Beat::of` equals `beat_at` for a few tempos and signatures; `colour_at` with a wave and a hit age shows the
  wave colour at the front; a stack with a wave needs hits; picture test (`tests/gradient_pictures.rs`): a background
  with Cycle speed at 1 s and at 1 s again after drawing 3 s gives the same pixels and differs from 0 s (pure in time;
  export and preview both draw through `build_frame`); Hue spin half a turn turns red to cyan-ish; Step on the beat
  at 0.4 s equals 0 s and differs at 0.75 s; a ball whose Body (Each ball) has a Hit wave, drawn just after its first
  hit, differs from the same ball with no wave and equals it before the hit.

## Task 5: the editor (`bpe-studio/src/gradient_tabs.rs`)

- Move tab: `companion_rows` for `ExtraTab::Move` draws a fold heading per group (decision 18; `group_heading` made
  `pub(crate)`, open state temp data `bar.with(("move fold", group))`), then the group's rows when open. A heading's
  dot: shut and a companion inside is stored (not default) or keyed/pulsed (`cx.marks`).
- Tests (kittest): the Move tab shows on a gradient; its folds show shut; opening Cycle shows Cycle speed and Cycle
  shift; dragging Cycle speed writes `colours~cycle`; opening Morph shows a "Morph to" gradient row with no tab row
  of its own; a fold with a stored companion shows "Cycle (changed)" while shut.

## Task 6: docs and checks

- CLAUDE.md: the moves (`Moves`, sampling order, wander, the beat, Hit wave through the atlas's wave texture and
  `paint_hit`, `wave_memory`), the Move tab.
- `cargo fmt`, `cargo clippy --workspace` clean, `cargo test --workspace` green (bar the known release-only runtime
  test); `git diff origin/main -- crates/*/tests/golden*` empty in debug and release.

## Things not to do

- Don't put moves or the wave in saved files or in `PaintGpu`.
- Don't let a move read anything drawn earlier (no frame-to-frame state, no caches that change what is drawn).
- Don't build part 2's forms or Shape tab, or part 4's window and tools.
- Don't cap speeds or amounts beyond the hard ranges in the table.
- Don't add example scenes.

## Verification summary

| Check | Passing looks like |
| --- | --- |
| `cargo test -p bpe-core` | moves on the value, the pass, purity, beat, follow sound on Cycle speed green |
| `cargo test -p bpe-gfx` | atlas hashes and waves, GPU hit wave green |
| `cargo test -p bpe-kit` | beat agrees with `beat_at`, CPU wave, needs_hits, move pictures green; goldens unchanged |
| `cargo test -p bpe-studio` | Move tab kittests green |
| `git diff origin/main -- crates/*/tests/golden*` | empty |
| clippy / fmt | no warnings, no diff |
