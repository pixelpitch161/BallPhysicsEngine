# Colour features, part 1 (Gradients) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace every thing's single colour / flat colour list with one shared Paint (a gradient with movable stops and transition styles, a Follow mode, Repeat, Scroll and an optional inside pattern), drawn the same way by every thing kind.

**Architecture:** `bpe-core` gets a `Gradient` value that replaces `Value::Colors`; `bpe-settings-derive` learns to expand a `Paint` field into prefixed settings; `bpe-kit/src/paint/` owns `Paint`, its enums and the one function that turns a paint plus per-instance inputs into a colour (or a gradient row and offset for the GPU). `bpe-gfx` gets a gradient atlas texture that every pipeline with an inside pattern samples. Each thing kind's draw code is then switched over, one task per family, and the editor gets a gradient bar.

**Tech Stack:** Rust 2024 workspace, wgpu + WGSL, egui/eframe (+ egui_kittest), serde/RON, lavapipe for GPU tests.

**Spec:** `docs/superpowers/specs/2026-10-30-colour-features-design.md` (sections 1-5, 10, 11). Parts 2 (blend modes, variation) and 3 (palettes, harmony, grade) get their own plans later.

## Global Constraints

- Clean break: old scene files need not load. Do not write migration code. Rewrite the built-in scenes and the `.ron` files under `scenes/` to the new keys.
- Merge latest `origin/main` before Task 1, and again before the PR. If the camera/world branch (`claude/project-thread-ivn0s6`) has landed, Place/Screen uses its world→screen mapping; if not, Screen = World and `Inputs::place_screen` is filled with the world position.
- `BTreeMap` everywhere, never `HashMap` (determinism rule).
- Golden **physics** logs (`tests/golden*.txt`) must not change. Golden picture hashes may change only where a scene's settings changed; record new ones in the same commit as the change.
- Two-clock rule: hits/keys/Time on the simulation clock (`cx.time`), beats on the video clock (`cx.video_time`).
- Every new setting has a `when` so it only shows where it applies (`check_registry` must pass).
- WGSL field names must not be `target`, `active` or `move`.
- Never put `.context_menu` on an egui colour button; use `Popup::context_menu(..).id(...)`.
- The colour picker's Additive option stays in this part (it is removed in part 2).
- GPU tests need `apt install mesa-vulkan-drivers libasound2-dev`; without lavapipe they silently skip, so check they ran.

## Review Focus

1. A gradient with one stop everywhere (default for every thing) must look exactly like today's single colour, including additive (alpha 0) colours. Test: Task 1 `one_stop_samples_its_colour_everywhere` and Task 5 picture test `solid_paint_matches_plain_colour`.
2. Two stops at the same `at` (user drags one onto another) must not divide by zero or produce NaN. Test: Task 1 `stops_at_the_same_place_make_a_hard_edge`.
3. Keyframing between gradients with different stop counts must not panic and must jump at halfway. Test: Task 1 `lerp_with_different_counts_jumps_at_half`.
4. Spread of 0, Colours end at of 0, or a ball group of one ball must give t = 0 and not NaN. Test: Task 3 `zero_spans_and_single_balls_give_the_first_colour`.
5. More distinct inside-pattern gradients in one frame than the atlas's starting height must grow the atlas, not overwrite rows. Test: Task 4 `atlas_grows_past_its_first_height`.

---

### Task 1: The Gradient value

**Files:**
- Modify: `crates/bpe-core/src/color.rs` (add `Stop`, `Transition`, `Gradient`; delete `Color::sample_gradient` once nothing calls it, at the end of Task 12)
- Modify: `crates/bpe-core/src/value.rs` (`Value::Colors` → `Value::Gradient(Gradient)`, `ValueType::Colors` → `ValueType::Gradient`)
- Modify: `crates/bpe-core/src/settings.rs` (`SettingValue for Gradient`, remove the `Vec<Color>` impl; `is_on(Gradient)` = more than one stop)
- Modify: `crates/bpe-core/src/animation.rs:239` (lerp), `crates/bpe-core/src/contract.rs:76`, `crates/bpe-runtime/src/world.rs:811` (fingerprint bytes: tag 5, then per stop `at` as f32 LE + rgba, then transition index)
- Test: in-file `#[cfg(test)]` modules of `color.rs`, `settings.rs`, `animation.rs`

**Interfaces:**
- Produces:
  - `pub struct Stop { pub at: f32, pub color: Color }`
  - `#[derive(Choice)] pub enum Transition { #[default] Smooth, Eased, Steps, Hue }`
  - `pub struct Gradient { pub stops: Vec<Stop>, pub transition: Transition }` (Clone, Debug, PartialEq, Serialize, Deserialize)
  - `Gradient::solid(c: Color) -> Gradient`, `Gradient::even(colors: &[Color]) -> Gradient`, `Gradient::sample(&self, t: f32) -> [f32; 4]` (linear light; t clamped to 0..=1), `Gradient::bake(&self, n: usize) -> Vec<[f32; 4]>`, `Gradient::first(&self) -> Color`, `Gradient::sorted(self) -> Gradient` (sort by `at`, clamp `at` to 0..=1, empty → solid white)
  - `Value::Gradient(Gradient)`, `ValueType::Gradient`

- [ ] **Step 1: Merge `origin/main`; write failing tests in `color.rs`**

```rust
#[test] fn one_stop_samples_its_colour_everywhere() {
    let c = Color::rgb(200, 30, 90);
    let g = Gradient::solid(c);
    for t in [-1.0, 0.0, 0.37, 1.0, 2.0] { assert_eq!(g.sample(t), c.to_linear()); }
    let add = Color { r: 255, g: 0, b: 0, a: 0 };            // additive keeps alpha 0
    assert_eq!(Gradient::solid(add).sample(0.5)[3], 0.0);
}
#[test] fn even_matches_the_old_sampler() {
    let cs = [Color::rgb(255,0,0), Color::rgb(0,255,0), Color::rgb(0,0,255)];
    let g = Gradient::even(&cs);
    for t in [0.0, 0.25, 0.5, 0.9, 1.0] { assert_eq!(g.sample(t), Color::sample_gradient(&cs, t)); }
}
#[test] fn stops_move_where_colours_sit() {
    let g = Gradient { stops: vec![Stop{at:0.0,color:RED}, Stop{at:0.8,color:BLUE}], transition: Transition::Smooth };
    assert_eq!(g.sample(0.8), BLUE.to_linear());
    assert_eq!(g.sample(1.0), BLUE.to_linear());
    assert!((g.sample(0.4)[0] - RED.to_linear()[0] * 0.5).abs() < 1e-5);
}
#[test] fn steps_split_halfway_between_stops() {
    let g = Gradient { stops: vec![Stop{at:0.0,color:RED}, Stop{at:0.6,color:BLUE}], transition: Transition::Steps };
    assert_eq!(g.sample(0.29), RED.to_linear());
    assert_eq!(g.sample(0.31), BLUE.to_linear());
}
#[test] fn eased_is_smoothstep_and_hue_goes_the_short_way() {
    let e = Gradient { transition: Transition::Eased, ..Gradient::even(&[BLACK, WHITE]) };
    assert!((e.sample(0.25)[0] - 0.15625).abs() < 1e-5);    // smoothstep(0.25) of linear 0..1
    let h = Gradient { transition: Transition::Hue, ..Gradient::even(&[Color::rgb(255,0,0), Color::rgb(255,0,255)]) };
    let mid = Color::from_linear(h.sample(0.5));
    assert!(mid.r > 200 && mid.b > 100 && mid.g < 10, "red→magenta passes through rose, not green: {mid:?}");
}
#[test] fn stops_at_the_same_place_make_a_hard_edge() {
    let g = Gradient { stops: vec![Stop{at:0.5,color:RED}, Stop{at:0.5,color:BLUE}], transition: Transition::Smooth };
    assert_eq!(g.sample(0.49), RED.to_linear());
    assert_eq!(g.sample(0.51), BLUE.to_linear());
    assert!(g.sample(0.5).iter().all(|x| x.is_finite()));
}
#[test] fn bake_has_n_texels_and_both_ends() {
    let g = Gradient::even(&[RED, BLUE]);
    let b = g.bake(256);
    assert_eq!(b.len(), 256);
    assert_eq!(b[0], RED.to_linear()); assert_eq!(b[255], BLUE.to_linear());
}
#[test] fn saves_as_stops_and_reads_a_hex_string_as_solid() {
    let g = Gradient::even(&[RED, BLUE]);
    let text = ron::to_string(&g).unwrap();
    assert_eq!(ron::from_str::<Gradient>(&text).unwrap(), g);
    assert_eq!(ron::from_str::<Gradient>("\"#ff0000\"").unwrap(), Gradient::solid(RED));
}
```

In `animation.rs` tests:

```rust
#[test] fn lerp_blends_gradients_stop_by_stop() { /* a=[(0,RED),(1,BLUE)], b=[(0.2,BLUE),(1,RED)]; at t=0.5 stop 0 has at 0.1 and colour Color::mix(RED,BLUE,0.5) */ }
#[test] fn lerp_with_different_counts_jumps_at_half() { /* 2 stops vs 3 stops: t=0.49 → a, t=0.5 → b */ }
```

In `settings.rs` tests, change the `"none"`/`"two"` fixtures to `Value::Gradient(Gradient::solid(WHITE))` / `Value::Gradient(Gradient::even(&[WHITE, WHITE]))`; the existing assertions `!holds("none")` and `holds("two")` stay.

- [ ] **Step 2: Run** `cargo test -p bpe-core` — Expected: FAIL (Gradient not defined).

- [ ] **Step 3: Implement** the types and methods in `color.rs`, the `Value`/`ValueType` change, `SettingValue for Gradient`, `is_on`, lerp (stop-by-stop `at` lerp + `Color::mix`, transition from `a` when t < 0.5 else `b`; different counts → `if t < 0.5 { a } else { b }`), contract and fingerprint. `sample` finds the pair of stops around t on the sorted list; when two stops share `at`, the later one wins for t ≥ at. Steps: colour of the nearest stop by `at` (ties → later). Hue: lerp in HSV on the short arc (use the HSV helpers behind `Color::shift_hue`), then convert to linear. Alpha lerps linearly in every mode. `Deserialize` accepts either the struct or a hex string (untagged helper enum). Fix every compile error in other crates by the smallest change that keeps behaviour: `Vec<Color>` gradient fields stay compiling by temporarily typing them `Gradient` and calling `Gradient::even` where a list was built — later tasks replace them properly.

- [ ] **Step 4: Run** `cargo test -p bpe-core && cargo build --workspace` — Expected: PASS / builds.

- [ ] **Step 5: Commit** `git commit -am "Core: Gradient value with stops and transitions replaces colour lists"`

---

### Task 2: Paint settings block and the derive

**Files:**
- Create: `crates/bpe-kit/src/paint/mod.rs` (Paint, Follow, Repeat, Inside, `impl SettingGroup for Paint`)
- Modify: `crates/bpe-core/src/settings.rs` (add `SettingGroup` trait)
- Modify: `crates/bpe-settings-derive/src/lib.rs` (`paint`/`group` field option)
- Modify: `crates/bpe-kit/src/lib.rs` (`pub mod paint;`)
- Test: `crates/bpe-kit/src/paint/mod.rs` tests; `crates/bpe-core` derive tests

**Interfaces:**
- Produces in `bpe-core`:
  ```rust
  pub struct GroupOpts { pub prefix: &'static str, pub label: &'static str, pub group: &'static str,
                         pub when: &'static str, pub follows: &'static [&'static str], pub inside: bool,
                         pub screen_only: bool }   // screen_only: no `space` def, Place always reads place_screen
  pub trait SettingGroup: Sized + Default {
      fn group_defs(o: &GroupOpts) -> Vec<SettingDef>;          // keys/whens leaked once per outer type
      fn group_read(o: &GroupOpts, p: &Params) -> Self;
      fn group_write(&self, o: &GroupOpts, p: &mut Params);    // only fields differing from default
  }
  ```
- Derive (options `inside` and `screen_only` are optional flags): a field `#[setting(paint, label = "Colours", prefix = "fill_", when = "...", follows = "place|start|order", inside)] pub fill: Paint` calls the three methods with a `GroupOpts`. `prefix` may be `""`.
- Produces in `bpe-kit::paint`:
  ```rust
  #[derive(Choice)] pub enum Follow { #[default] Place, Start, Order, Speed, Hits, Life, Step, Beat, Time, NearestBall, Path }
  #[derive(Choice)] pub enum Space { #[default] World, Screen }
  #[derive(Choice)] pub enum Repeat { #[default] Clamp, Repeat, Mirror }
  #[derive(Choice)] pub enum Inside { #[default] None, Stripes, Rings, Sweep }
  #[derive(Choice)] pub enum PlaceShape { #[default] Direction, Centre }
  pub struct Paint {
      pub colours: Gradient, pub follow: Follow, pub shape: PlaceShape, pub angle: f32, pub centre: Vec2,
      pub space: Space, pub span: f32, pub repeat: Repeat, pub scroll: f32, pub take_hit_colour: bool,
      pub step_time: f32, pub inside: Inside, pub inside_angle: f32, pub inside_spin: f32,
      pub inside_scale: f32, pub inside_scroll: f32,
  }
  ```
  Defaults: `colours = Gradient::solid(WHITE)`, `follow = Place`, `shape = Direction`, `angle = 90.0` (top → bottom, y down), `centre = Vec2::ZERO`, `span = 1920.0`, `repeat = Clamp`, `scroll = 0.0`, `take_hit_colour = false`, `step_time = 0.15`, `inside = None`, `inside_angle = 0.0`, `inside_spin = 0.0`, `inside_scale = 1.0`, `inside_scroll = 0.0`.

Generated keys (prefix + name), labels, `when` terms (each also ANDed with the field's own `when`):

| key | label | when (beyond the field's) | flags / range |
|---|---|---|---|
| `colours` | the field label | — | keyframe, random |
| `follow` | Colours follow | `colours` | random; Choice limited to `follows` |
| `shape` | Place from | `colours and follow=place\|start` | |
| `angle` | Direction | `colours and follow=place\|start and shape=direction` | range(-180,180) deg, keyframe, random |
| `centre` | Centre | `colours and follow=place\|start and shape=centre` | keyframe |
| `space` | Positions in | `colours and follow=place` | |
| `span` | Spread / Colours end at | `colours and follow=place\|start\|speed\|hits` | range(1, 20000), keyframe, random(1,2000) |
| `repeat` | Past the end | `colours` | random |
| `scroll` | Scroll | `colours` | range(-4,4) cycles/s, keyframe, random |
| `take_hit_colour` | Take hit colour | `follow=step` | (shows for one stop too) |
| `step_time` | Step time | `colours and follow=step` | range(0,2) s |
| `inside` | Inside pattern | only generated when `inside`; `colours` | random |
| `inside_angle`, `inside_spin`, `inside_scale`, `inside_scroll` | Pattern angle / Pattern spin / Pattern size / Pattern scroll | `colours and inside!=none` | ranges (-180,180) deg, (-720,720) deg/s, (0.1,8), (-4,4) cycles/s; keyframe |

Key names in conditions are prefixed too (the generator rewrites `colours` → `fill_colours`, etc.). Group heading = the field's `group`.

- [ ] **Step 1: Write failing tests** in `paint/mod.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Default, Settings)]
struct Two {
    #[setting(paint, label = "Colours", prefix = "", follows = "place|order|speed")] fill: Paint,
    #[setting(paint, label = "Outline colours", prefix = "outline_", follows = "place", inside)] outline: Paint,
}
#[test] fn a_paint_field_expands_to_prefixed_settings() {
    let keys: Vec<_> = Two::defs().iter().map(|d| d.key).collect();
    assert!(keys.contains(&"colours") && keys.contains(&"outline_colours") && keys.contains(&"outline_inside"));
    assert!(!keys.contains(&"inside"), "no inside pattern unless asked");
    let f = Two::defs().iter().find(|d| d.key == "follow").unwrap();
    assert_eq!(f.ty, ValueType::Choice(&["place", "order", "speed"]));
    let a = Two::defs().iter().find(|d| d.key == "outline_angle").unwrap();
    assert!(a.when.contains("outline_colours") && a.when.contains("outline_follow=place|start"));
}
#[test] fn paint_round_trips_through_params() {
    let mut t = Two::default();
    t.outline.colours = Gradient::even(&[RED, BLUE]); t.outline.angle = 30.0;
    let p = t.to_params();
    assert_eq!(p.get("outline_angle"), Some(&Value::Number(30.0)));
    assert!(p.get("angle").is_none(), "defaults are not saved");
    assert_eq!(Two::from_params(&p), t);
}
#[test] fn a_follow_outside_the_list_reads_as_default() {
    let mut p = Params::default(); p.set("follow", Value::Choice("beat".into()));
    assert_eq!(Two::from_params(&p).fill.follow, Follow::Place);
}
```

- [ ] **Step 2: Run** `cargo test -p bpe-kit paint::` — Expected: FAIL.
- [ ] **Step 3: Implement** `SettingGroup`, the derive option (parse `paint`, `prefix`, `follows`, `inside`; `defs()` then becomes `let mut v = vec![...]; v.extend(Paint::group_defs(&opts)); ...` in field order, so build the vec with pushes rather than one `vec![]` literal), and `impl SettingGroup for Paint` (hand-written; leak each prefixed key/when string with `Box::leak`, which happens once per outer type because `defs()` is in a `OnceLock`). Follow choice names come from `follows.split('|')` and are leaked as a `&'static [&'static str]`.
- [ ] **Step 4: Run** `cargo test -p bpe-kit paint:: && cargo test -p bpe-core` — Expected: PASS.
- [ ] **Step 5: Commit** `"Paint settings block: one shared set of colour settings any thing can embed"`

---

### Task 3: Turning a paint into colours

**Files:**
- Create: `crates/bpe-kit/src/paint/eval.rs`
- Modify: `crates/bpe-runtime/src/bodies.rs` (add `pub spawn: Vec<Vec2>` set in `push`, kept in step by every push/remove; not hashed into the physics fingerprint)
- Test: in-file tests

**Interfaces:**
- Consumes: `Paint`, `Gradient` (Tasks 1-2).
- Produces:
  ```rust
  #[derive(Clone, Copy, Default)]
  pub struct Inputs { pub index: usize, pub count: usize, pub place: Vec2, pub place_screen: Vec2, pub start: Vec2,
      pub speed: f32, pub hits: u32, pub since_hit: f32, pub life: f32, pub path: f32,
      pub nearest: Option<[f32; 4]>, pub hit_colour: Option<[f32; 4]>, pub beats: f32, pub time: f32 }
  pub fn offset(p: &Paint, i: &Inputs) -> f32;              // t after Follow, Scroll and Repeat, in 0..=1
  pub fn colour_at(p: &Paint, i: &Inputs) -> [f32; 4];      // linear light
  pub fn beats_at(log: &bpe_runtime::Log, video_time: f64) -> f32;   // whole beats so far + fraction to next
  pub fn last_hit(log: &Log, thing: ThingId, index: usize, time: f64) -> Option<(f64, Option<ThingId>)>;
  ```

Follow → raw t (spec section 3), with `s = span.max(1e-3)`:
- Place/Start: Direction `dot(pos - centre, dir(angle)) / s + 0.5`; Centre `|pos - centre| / s`. Place uses `place` or `place_screen` per `space`; Start uses `start`.
- Order: `index / (count - 1)`, 0 when count ≤ 1. Speed `speed / s`. Hits `hits / s`. Life `life`. Path `path`.
- Step: `k = hits mod n_stops`; colour = blend of stop k-1 → stop k over `step_time` using `since_hit` (with the gradient's transition; Steps = instant). With `take_hit_colour` and `hit_colour` Some, return that colour instead.
- Beat: `beats / (n_stops)`, with Repeat forced to Repeat. Time: `time * scroll` (Scroll is its speed; when 0 use 0.25 cycles/s).
- NearestBall: return `nearest` when Some, else the first stop.
- Then `t += time * scroll` (not for Time/Beat), then Repeat: Clamp `clamp(0,1)`, Repeat `fract`, Mirror `1 - |fract(t/2)*2 - 1|`.

- [ ] **Step 1: Failing tests**: `place_direction_runs_along_the_angle` (angle 0, span 1000: x=-500 → t 0, x=+500 → t 1), `screen_space_reads_place_screen`, `start_uses_spawn_position`, `order_spreads_first_to_last`, `zero_spans_and_single_balls_give_the_first_colour` (span 0 → finite; count 1 → t 0; all outputs finite), `repeat_and_mirror_wrap` (t 1.25 → 0.25 / 0.75), `scroll_moves_the_offset_over_time`, `step_on_hit_walks_the_stops` (3 stops, hits 4 → stop 1 after step_time), `take_hit_colour_wins_when_known`, `beat_steps_one_stop_per_beat`, `beats_at_counts_beat_events_on_the_video_clock` (log with Beat events at 0.5, 1.0: `beats_at(log, 0.75) == 1.5`).
- [ ] **Step 2: Run** `cargo test -p bpe-kit paint::eval` — FAIL.
- [ ] **Step 3: Implement** `eval.rs` and `Bodies.spawn`.
- [ ] **Step 4: Run** `cargo test -p bpe-kit paint:: && cargo test -p bpe-runtime && cargo test --test golden` — PASS, golden physics unchanged.
- [ ] **Step 5: Commit** `"Paint: follow modes, repeat and scroll turn a paint into colours"`

---

### Task 4: Gradient atlas on the GPU

**Files:**
- Create: `crates/bpe-gfx/src/gradients.rs`, `crates/bpe-gfx/src/shaders/paint.wgsl` (shared WGSL, prepended like `ball_look.wgsl` is)
- Modify: `crates/bpe-gfx/src/frame.rs` (`Frame.gradients: GradientAtlas`, cleared in `clear`), `crates/bpe-gfx/src/lib.rs` (texture, sampler pair, bind group layout `paint_layout` at the next free group index for scene/styled/shape/soft/glyph/trail/particle pipelines)
- Test: `crates/bpe-gfx/src/gradients.rs` tests; a GPU test in `crates/bpe-gfx/src/paint_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const ATLAS_WIDTH: usize = 256;
  pub struct GradientAtlas { rows: Vec<[[f32; 4]; ATLAS_WIDTH]>, index: BTreeMap<u64, u32> }
  impl GradientAtlas { pub fn row(&mut self, g: &Gradient) -> u32; pub fn len(&self) -> usize; pub fn clear(&mut self); }
  #[repr(C)] #[derive(Pod, Zeroable, Clone, Copy, Default, Debug, PartialEq)]
  pub struct PaintGpu { pub row: f32, pub t0: f32, pub pattern: f32, pub steps: f32,   // row −1 = no pattern; steps 1 = nearest
                        pub angle: f32, pub scale: f32, pub scroll: f32, pub repeat: f32 }
  ```
  WGSL in `paint.wgsl`: `fn paint_colour(p: PaintGpu, local: vec2<f32>, time: f32) -> vec4<f32>` where `local` is the point in the instance's own unit space (-1..1). Pattern t: Stripes `dot(local, dir(angle)) * 0.5 * scale + 0.5`; Rings `length(local) * scale`; Sweep `atan2(local.y, local.x) / TAU + 0.5` (scale = repeats); then `+ t0 + time * scroll`, Repeat as in Task 3; sample row with the linear sampler, or `textureLoad` at `floor(t * 255.0 + 0.5)` when `steps == 1`.
- Texture: `Rgba16Float`, width 256, height `max(8, next_power_of_two(len))`, re-created only when it must grow; rows hashed by FNV-1a of the gradient's stops + transition.

- [ ] **Step 1: Failing tests**: `same_gradient_gets_the_same_row`, `atlas_grows_past_its_first_height` (20 distinct gradients → 20 rows, row 0 still the first gradient's bake), GPU `pattern_stripes_paint_two_halves` (a styled ball with Stripes angle 0 over [RED, BLUE] Steps: left half red, right half blue within ±2 px of centre line).
- [ ] **Step 2: Run** `cargo test -p bpe-gfx gradients paint_tests` — FAIL.
- [ ] **Step 3: Implement** atlas, upload in the renderer before the scene pass, bind group, `paint.wgsl`.
- [ ] **Step 4: Run** same — PASS (check lavapipe actually ran: test prints adapter name).
- [ ] **Step 5: Commit** `"Renderer: gradient atlas so shapes can paint gradients inside themselves"`

---

### Task 5: Balls (classic and styled looks)

**Files:**
- Modify: `crates/bpe-kit/src/modules/look/mod.rs`, `look/styled.rs`, `look/draw.rs`, `crates/bpe-kit/src/things/balls/draw.rs`
- Modify: `crates/bpe-gfx/src/frame.rs` (`BallInstance.paint: PaintGpu`, `LookInstance.paint: PaintGpu`, `LookInstance.paint2: PaintGpu`, `LookInstance.paint_outline: PaintGpu`), `shaders/scene.wgsl`, `shaders/styled.wgsl`, `shaders/ball_look.wgsl`
- Test: `crates/bpe-kit/src/things/balls/draw.rs` tests, `crates/bpe-gfx/src/style_tests.rs`

**Interfaces:**
- Consumes: `Paint`, `colour_at`, `offset`, `GradientAtlas::row`, `PaintGpu`.
- Produces: `ClassicLook.fill: Paint` (`#[setting(paint, label = "Colours", prefix = "", when = "thing!=liquid", follows = "place|start|order|speed|hits|step|beat|time", inside)]`), replacing `color`, `gradient`, `gradient_by`, `gradient_full`. `StyledLook.fill` (same), `StyledLook.pattern: Paint` (prefix `pattern_`, label "Pattern colours", replaces `color2`), `StyledLook.outline_paint: Paint` (prefix `outline_`, replaces `outline_color`). `hue_per_hit`, `rainbow` stay. `look::draw::colors(look, cx, bodies) -> impl Fn(usize) -> [f32; 4]` builds `Inputs` per ball (place = position, start = `bodies.spawn[i]`, beats via `beats_at`, hit colour via `last_hit` + the other thing's `look_of(..).fill` at the hit point) and stays the single source for `ball_colors` (so trails and particles keep working). A helper `paint_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas, time: f32) -> PaintGpu` (in `paint/gpu.rs`) gives `row = -1` when `inside == None`.
- `registry.carry_over` between classic and styled must keep the `colours`… keys (they share the fill prefix "").

- [ ] **Step 1: Failing tests**: update `switching_look_kind_keeps_the_shared_colour_and_glow` to `colours`; `solid_paint_matches_plain_colour` (GPU: a one-stop fill draws the same pixels as the pre-change golden still for scene "one-ball" — compare against a PNG rendered from `origin/main` and committed under `crates/bpe-gfx/tests/data/`); `row_follow_is_place_from_spawn_at_90_degrees` (a 3-row block with fill Follow Start, angle 90, span = block height: top row first colour, bottom row last); `inside_rings_draw_on_classic_balls` (GPU).
- [ ] **Step 2: Run** `cargo test -p bpe-kit balls && cargo test -p bpe-gfx style_tests` — FAIL.
- [ ] **Step 3: Implement**; delete `GradientBy` and its uses; `random.rs` rolls a Gradient (keep stop count, roll colours and positions; Task 11 finishes the dice).
- [ ] **Step 4: Run** `cargo test --workspace` — PASS except golden picture hashes for scenes whose colour keys changed; rewrite those scenes' builders in `scenes*.rs` (`l.gradient = vec![..]` → `l.fill.colours = Gradient::even(&[..])`, `gradient_by = Hits` → `fill.follow = Follow::Hits`, `gradient_full` → `fill.span`, Row → Start with angle 90 and span = the block's height) so they look the same, and confirm golden picture hashes are unchanged where the look is meant to be identical.
- [ ] **Step 5: Commit** `"Balls: one Colours paint per look part, with follow modes and inside patterns"`

---

### Task 6: Walls and obstacles

**Files:** Modify `crates/bpe-kit/src/things/arena/draw.rs`, `things/obstacle/draw.rs`, `crates/bpe-gfx/src/frame.rs` (`ShapeInstance.paint: PaintGpu`, `ShapeInstance::outline(outline, paint: PaintGpu, color, glow)`), `shaders/scene.wgsl` shape fragment. Test: `crates/bpe-gfx/src/paint_tests.rs`, obstacle draw tests.

**Interfaces:** Walls read the thing's look `fill` (Classic or Styled). Follows for walls/obstacles: `place|order|hits|step|beat|time|path`. `Path`: the shader computes `t` along the outline (ring/polygon: angle around centre / TAU from the shape's angle; box: perimeter fraction clockwise from top-left; capsule: along its length), so Path paints a gradient along the wall line; CPU sets `PaintGpu.row` even when `inside == None` for Path. Order: index of the peg in a peg grid / bar in a set. Inside pattern fills across the shape's bounding box. Hits/Step use hits counted from the log for that obstacle.

- [ ] **Step 1: Failing tests**: `ring_path_gradient_goes_round` (GPU: ring with [RED, BLUE] Path: pixel at angle 0 red, angle 180° mid colour), `peg_grid_order_spreads_colours`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `"Walls and obstacles: paint along their outline or across their shape"`

---

### Task 7: Soft bodies and text

**Files:** Modify `crates/bpe-kit/src/things/soft/draw.rs`, `things/text/{mod,draw}.rs`, `crates/bpe-gfx/src/frame.rs` (`SoftVertex.paint: PaintGpu`, `SoftVertex.local: [f32; 2]`; `GlyphInstance.paint: PaintGpu`, `GlyphInstance.paint_outline: PaintGpu`, `GlyphInstance.local: [f32; 4]` = glyph rect in line space), `shaders/scene.wgsl` soft fragment, `shaders/glyph.wgsl`. Test: soft and text draw tests, GPU test.

**Interfaces:** Soft: per-vertex colour from `colour_at` with `index` = point index, `count` = points, `place` = point position; follows `place|order|hits|step|beat|time`; inside pattern over the body's bounding box (`local`). Text: `TextSettings.outline_paint: Paint` (prefix `outline_`, replaces `outline_color`); fill from the look. New text setting `paint_per_letter: bool` (label "Colours per letter", when `colours`): on → each glyph's `local` is its own rect, `index` = letter index; off → `local` spans the whole line. Follows `place|order|beat|time`.

- [ ] **Step 1: Failing tests**: `soft_body_order_spreads_along_points`, `text_per_letter_order_colours_each_letter` (glyph colours of "ABC" with [RED, GREEN, BLUE] Order = the three stops).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `"Soft bodies and text: shared colour paints"`

---

### Task 8: Trails and particles

**Files:** Modify `crates/bpe-kit/src/modules/trail/{mod,draw}.rs`, `modules/particles/{mod,draw}.rs`, `crates/bpe-gfx/src/frame.rs` (`TrailVertex.life: f32`, `TrailVertex.row: f32`; `Burst.paint: PaintGpu`, `Burst.follow: u32` 0 = fixed colour, 1 = life, 2 = random stop), `shaders/trail.wgsl`, `shaders/particles.wgsl` (+ paint bind group). Test: trail/particle module tests, GPU test.

**Interfaces:** Trail `colour: Paint` (prefix "", replaces `color`; `source` keeps Ball/Own): Own follows `life|place|speed|time|beat`, Life = 0 at the ball, 1 at the tail end. Particles `colour: Paint` (replaces `color`): Own follows `life|order|place|speed|time|beat`; Life samples in the shader by particle age/life; Order picks a random stop per particle from its seed (`stop = floor(hash(seed) * n)`, baked as Steps). Ball source unchanged.

- [ ] **Step 1: Failing tests**: `trail_life_runs_head_to_tail` (vertex 0 colour = first stop, last vertex = last stop), `particles_life_fades_through_colours` (GPU: one burst, sample at 10% and 90% life).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `"Trails and particles: colours over their life"`

---

### Task 9: Liquid and swarm

**Files:** Modify `crates/bpe-kit/src/modules/look_liquid/mod.rs`, `things/liquid/draw.rs`, `modules/look_swarm/mod.rs`, `things/swarm/draw.rs`, `crates/bpe-gfx/src/frame.rs` (`LiquidLayer.paint: PaintGpu` + `paint_place: [f32; 4]` = angle, span, centre; `SwarmLook.paint: PaintGpu`, `follow: u32`), `crates/bpe-gfx/src/flow/{liquid,surface,swarm}.rs` and their shaders. Test: flow tests (GPU).

**Interfaces:** Liquid `colour: Paint` (replaces `color`); follows `place|speed|time|beat`. Place (depth/height via angle): the surface pass computes t from the pixel's world position. Speed: the thickness pass also accumulates `speed × weight` into a second `R16Float` target at the same resolution, and the surface pass divides by thickness to get mean speed, then `t = speed / span`. Swarm `colours: Paint` (replaces `colors`/`colors_by`); follows `speed|nearest_ball|place|order|time|beat`; the swarm shader samples the atlas row with t from its existing speed / index maths, Place from the dot's position.

- [ ] **Step 1: Failing tests**: `liquid_place_paints_top_and_bottom` (GPU: pool with [BLUE, RED] Place angle 90: top surface pixel bluer than bottom), `swarm_speed_uses_the_gradient`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `"Liquid and swarms: shared colour paints"`

---

### Task 10: Scene background

**Files:** Modify `crates/bpe-kit/src/things/scene/{mod,draw}.rs`, `crates/bpe-gfx/src/frame.rs` (`Frame.background: BackgroundPaint` replaces `background_top`/`background_bottom`), background code in `shaders/scene.wgsl` / `post.wgsl` (wherever the top/bottom lerp lives now). Test: scene draw test, GPU test.

**Interfaces:** `SceneSettings.background: Paint` (prefix `background_`, label "Background", follows `place|time|beat`, derive option `screen_only`, so there is no `space` setting and Place reads screen positions). Default `Gradient { stops: [(0, #02022a), (1, #010006)], Smooth }`, angle 90, span = video height. `BackgroundPaint { row: u32, place: [f32; 4] /* angle, span, centre */, t0: f32, scroll: f32, repeat: f32 }`.

- [ ] **Step 1: Failing test**: `default_background_matches_old_top_bottom` (GPU: rendered default background equals the PNG from `origin/main` within 1/255).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` — PASS.
- [ ] **Step 5: Commit** `"Background: one paint at any angle or from the centre"`

---

### Task 11: Editor gradient bar, Paint rows and dice

**Files:**
- Create: `crates/bpe-studio/src/gradient_bar.rs`
- Modify: `crates/bpe-studio/src/widgets.rs` (`Row::Colours` is now one `Gradient` setting; delete the COLOUR/COLOURS merge and `colours_row`'s list logic), `crates/bpe-studio/src/inspector.rs` (delete `edit_colours`'s colour↔colours key moving; keyed edits write whole gradients), `crates/bpe-studio/src/library.rs:174`, `crates/bpe-kit/src/random.rs`
- Test: `widgets.rs`/`inspector.rs` kittest modules, `random.rs` tests

**Interfaces:**
- `pub fn gradient_bar(ui: &mut egui::Ui, id: egui::Id, g: &mut Gradient) -> egui::Response` — draws the baked gradient 256 px × 18 px (full row width), a handle per stop under it. Click on the bar adds a stop at that `at` with the sampled colour; dragging a handle moves `at` (clamped 0..1, list re-sorted, the dragged handle keeps focus); dragging a handle more than 30 px below the bar or right-click → "Remove" (via `Popup::context_menu(..).id(...)`) removes it unless it is the last; clicking a handle opens the egui colour picker (Alpha::BlendOrAdditive, through the existing `picker_color`/`scene_color` helpers). Beside the bar: a Transition combo. A one-stop gradient shows as a single swatch plus a "+" button that adds a second stop (white at 1.0).
- The rest of a Paint's settings render as normal rows (their `when`s do the showing); the card groups them under the paint's `group` heading with a fold arrow (existing `Inspector.collapsed`).
- Dice: `random.rs` rolls a Gradient by keeping the stop count, rolling colours from `palette(dice, n)` and `at`s evenly with ±0.1 jitter (first 0, last 1); Follow/angle/span/scroll roll within their dice ranges.

- [ ] **Step 1: Failing tests** (kittest, harness 420×2600): `clicking_the_bar_adds_a_stop`, `dragging_a_handle_moves_its_stop`, `right_click_remove_keeps_at_least_one`, `one_stop_shows_a_swatch_and_plus`, `keyed_gradient_edit_writes_a_key` (edit at t=1 with keyframes on → a key with the new gradient), `colour_handle_opens_on_left_click` (the #43 rule), and in `random.rs` `gradient_dice_keep_stop_count_and_sorted_stops`.
- [ ] **Step 2: Run** `cargo test -p bpe-studio && cargo test -p bpe-kit random` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same — PASS. Then screenshot the editor under Xvfb with `scenes/next/peg-board.ron` open (see memory `studio-next-gotchas`) and look at the ball card's Colours bar.
- [ ] **Step 5: Commit** `"Editor: gradient bar with draggable stops for every colour setting"`

---

### Task 12: Scenes, cleanup, full check and Windows build

**Files:** `crates/bpe-kit/src/scenes*.rs`, `scenes/**/*.ron`, `crates/bpe-studio/src/{starters,presets}.rs`, `crates/bpe-core/src/color.rs` (delete `sample_gradient`), golden picture hash files.

- [ ] **Step 1:** `grep -rn "gradient_by\|gradient_full\|background_top\|outline_color\|color2\|sample_gradient\|Value::Colors" crates scenes` returns nothing outside comments; rewrite every remaining `.ron` scene to the new keys by loading it in a small throwaway test that rewrites keys, or by hand. Each `.ron` under `scenes/next/` must load without warnings: add test `every_bundled_scene_loads_with_known_keys` in `crates/bpe-kit/tests/`.
- [ ] **Step 2:** One new scene `scenes::colour_demo` ("Colour paints"): a ball block with Place-angle gradient + inside Rings, a ring arena with Path gradient, a trail with Life gradient, particles with Life, a background from Centre. Add it to `scenes::all()` and record its golden lines.
- [ ] **Step 3: Run** `cargo test --workspace && cargo clippy --workspace -- -D warnings` — all PASS; golden physics files unchanged (`git diff --stat tests/golden*.txt` shows only the new scene's lines).
- [ ] **Step 4:** Render `colour_demo` to mp4 with `bpe-render`, cross-build Windows exes (`cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`), copy exes, mp4 and two editor screenshots to `/mnt/project-files/ballphysics/colour-features/part1/`.
- [ ] **Step 5:** Commit, push `claude/project-thread-rlnct1`, then a fresh reviewer over `git diff origin/main...HEAD` before the PR.
