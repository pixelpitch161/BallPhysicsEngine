# Gradients part 1: the core and the tabbed row

Spec: `docs/superpowers/specs/2026-11-06-gradients-design.md` (approved by Aedan 2026-11-06). This part builds the
spec's section 1 entire, section 2's mechanism with Bands and Grain, and section 5's bar, Colours tab and Blend tab
(Shape and Move tabs only as empty slots filled by later parts). Parts 2 (shapes), 3 (movement) and 4 (big editor and
tools) start after this merges and only add companion definitions and tab content.

Rules that hold throughout: physics goldens (`crates/bpe-kit/tests/golden*.txt`) and sound goldens do not change; a
gradient with every new setting at its default draws as today except the atlas precision (≤ 1/255 per channel on
smooth gradients); nothing keeps frame-to-frame state; the design system's tokens only; every new control is named for
kittest.

## Decisions made while planning (beyond the spec)

1. **Gap blend for the last stop**: `Stop.blend`/`middle` of the last stop describe the wrap gap and are used only
   when the gradient loops. The editor shows that stop's gap row only when Loop is on.
2. **Middle bend**: `f' = f^p` with `p = ln 0.5 / ln m`, `m` clamped to `[0.001, 0.999]`; `m == 0.5` skips the
   power so the default is bit-exact. The blend shape (Eased, Ease in, Ease out) is applied to `f'`. Steps is exact:
   first colour while `f < m`, second from `m` on (no power curve).
3. **Plain** blends the sRGB-encoded floats (`c/255`, not rounded) and converts back to linear with the exact
   transfer function, so a Plain bake is smooth, not 8-bit stepped.
4. **OKLCH with a grey end**: when one end's chroma is under `1e-4` its hue is taken from the other end, so red to
   white in Round the wheel doesn't swing through other hues. Both grey: hue 0, chroma stays ~0.
5. **Gamut**: after any OKLab/OKLCH mix the linear result is checked against `[-1e-5, 1 + 1e-5]`; when out, chroma is
   reduced by bisection (20 steps) at the same L and hue until in, then clamped. Light and Plain never go out.
6. **Grain reaches the GPU through the value**: `Gradient` gains `grain: f32`, `#[serde(skip)]`, documented as
   draw-time only. It is never saved, never edited and never set by anything but `gradient_extras::apply_moves`, which
   copies the `~grain` companion into it just before drawing (so Grain stays an ordinary keyframable companion, as the
   spec requires). This is how every `PaintGpu` site (ball layers, sub-layers, shapes, background, recolour) gets it
   without each settings struct naming a companion. It is not hashed into the atlas key (it doesn't change the baked
   row) and `sample`/`bake` ignore it. `PaintGpu.steps` becomes `PaintGpu.grain`; `BackgroundPaint.steps` becomes
   `BackgroundPaint.grain`.
7. **Grain on the GPU**: one uniform value per pixel, the same for r, g and b, `(hash(floor(pixel)) − 0.5) × |grain| /
   255`, added in sRGB-encoded space and turned back to linear (dithering hides 8-bit steps, which are in sRGB). The
   pixel comes from a WGSL private `paint_px` that each fragment entry point that paints sets from its
   `@builtin(position)` first thing. A shader that doesn't set it gets a constant (harmless) offset; every painting
   entry point is listed in Task 3. Grain is applied in `paint_along` (all per-pixel gradient painting goes through it)
   and in the background's exact two-colour path.
8. **Bands as a value transform**: `Gradient::banded(n)` returns 2n stops (`k/n` and `(k+1)/n`, both holding the
   colour sampled at `(k + 0.5)/n`), Mix Light, blends Smooth, `looped` kept, grain kept. Identical neighbours blend to
   themselves, so the bands are flat in every Mix. Bands companion is a Number, read `round()`, hard range `0..=1024`
   (more than the atlas width shows nothing new and would only allocate; this is the "would break" limit), span 0..24.
   0 is off; 1 is one flat band.
9. **Grain companion**: Number, no hard range, span 0..8, keyframe and pulse; the shader takes `abs`.
10. **Animation of a companion that isn't stored**: `Animation::new` takes a track's base from the resolved params; a
    companion at its default isn't stored, so it falls back to `gradient_extras::default_of(key)` when the base gradient
    is there. Otherwise keying Bands from 0 would do nothing.
11. **Companions can't vary across balls**: `Scene::vary_use` returns `None` for a companion key (its per-ball mixing
    would happen after the pass that applies it).
12. **Loop switch off**: when Loop is switched off in the editor and the stops sit at `i/n` (within 1e-4), they are
    re-spaced to `i/(n−1)`, the mirror of switching it on. Otherwise they stay.
13. **Click on a stop** keeps today's behaviour (opens its colour picker) and also selects it; the Colours tab shows
    the selected stop. Dragging a stop selects it. The selection is session memory per row.
14. **Tabs only on two or more stops**: a one-colour gradient keeps today's swatch, "+" and Palette. (Part 3 may show
    Move on a single colour for Hue spin.) A tab with no rows is left out, so in this part the row shows Colours and
    Blend only.
15. **New gradients mix Even**: the bar's "+", built-in palettes (Rainbow: looped, stops `i/6`, Round the wheel),
    Random palette and Add matching colours set `Mix::Even`. Settings-struct defaults (`GroupGradient::default` etc.)
    and `Gradient::solid`/`even`/`Default` stay Light so test scenes and pinned pictures don't move. Starters
    (`starters.rs`) write no multi-colour gradient today, so nothing changes there.
16. **The background's exact two-colour path** (`BackgroundPaint.ends`) is taken only for Light, not looped, both
    stops Smooth with middle 0.5 (exactly today's Smooth two-stop case).
17. **Atlas 1024 and Steps**: the old `steps` flag sampled the nearest of 256 texels; now hard edges are baked and the
    sampler blends across one 1/1023 texel. Smooth gradients move ≤ 1/255. A hard edge can move by under 1/512 of
    the gradient's length, which can change edge pixels by more; such tests are re-pinned only after checking that
    the only pixels over 1/255 lie on a hard edge (reported in the PR).
18. **Timeline lane name** for a companion: "<gradient label> <companion label lowercased>", e.g. "Ball · Body ·
    Colours bands".

## Task 1: the gradient value (`crates/bpe-core/src/color.rs`, new `crates/bpe-core/src/oklab.rs`)

`oklab.rs` (pub mod, re-exported functions):
- `pub fn linear_to_oklab(c: [f32; 3]) -> [f32; 3]` and `pub fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3]`
  (Ottosson's matrices, f32, cbrt via `f32::cbrt`).
- `pub fn oklab_to_lch(lab) -> [f32; 3]` (L, C, h in turns 0..1) and `pub fn lch_to_oklab(lch) -> [f32; 3]`.
- `pub fn in_gamut(c: [f32; 3]) -> bool` (each in `[-1e-5, 1 + 1e-5]`) and `pub fn fit_gamut(lch) -> [f32; 3]`
  (decision 5; returns linear RGB clamped to 0..1).

`color.rs`:
- Remove `Transition`. Add a private `SavedTransition { Smooth, Eased, Steps, Hue }` (Deserialize only) for reading.
- `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Choice)] pub enum Mix { #[default]
  Light, Plain, Even, RoundTheWheel, LongWayRound }` with doc lines as the spec words them.
- `pub enum GapBlend { #[default] Smooth, Eased, EaseIn, EaseOut, Steps }` (same derives).
- `Stop { at, color, #[serde(default, skip_serializing_if)] blend: GapBlend, #[serde(default = "half",
  skip_serializing_if = "is_half")] middle: f32 }`; `impl Stop { pub fn new(at, color) -> Self }` (Smooth, 0.5). Every
  `Stop { at, color }` literal in the workspace becomes `Stop::new(at, color)` (≈25 sites, listed by
  `grep -rn "Stop {" crates`).
- `Gradient { stops, mix: Mix, looped: bool, grain: f32 }` (grain `#[serde(skip)]`, decision 6). `solid`, `even`,
  `Default` keep Light, not looped, grain 0.
- Custom `Serialize` through a borrowed helper struct: `stops`, then `mix` (skipped when Light), `looped` (skipped when
  false). Deserialize: untagged `Solid(Color)` or `Stops { stops, #[serde(default)] transition: Option<SavedTransition>,
  #[serde(default)] mix: Mix, #[serde(default)] looped: bool }`; an old `transition` turns into gap blends (Eased/Steps on
  every stop) or `mix = RoundTheWheel` (Hue) when `mix` wasn't given. Then `.sorted()`.
- `sorted()` also keeps `middle` finite and in 0..1 (NaN → 0.5).
- `pub fn sample(&self, t) -> [f32; 4]`: as today when not looped (with the gap after stop `i` worked by `gap`);
  looped: `t = t − floor(t)` (NaN → 0), and `t < first.at || t >= last.at` is the wrap gap from the last stop to the
  first, width `first.at + 1 − last.at`; a zero-width wrap can't be reached (decision in the code comment).
- `fn gap(&self, a: &Stop, b: &Stop, f: f32) -> [f32; 4]`: bend by middle, shape by blend (decision 2), then
  `mix_colours(a.color, b.color, u, self.mix)`.
- `pub fn mix_colours(a: Color, b: Color, u: f32, mix: Mix) -> [f32; 4]` (linear out; alpha straight; u ≤ 0 → a,
  u ≥ 1 → b exactly).
- `pub fn spread_evenly(&mut self)`: `i/n` when looped, `i/(n−1)` otherwise (one stop: at 0).
- `pub fn set_looped(&mut self, on: bool)`: the editor's Loop switch with decisions 12 and the spec's respacing.
- `pub fn banded(&self, n: u32) -> Gradient` (decision 8; `n == 0` returns a clone).
- `pub fn spread(colors: &[Color], mix: Mix, looped: bool) -> Gradient`: evenly spaced for palettes and harmony.
- `bake(n)` unchanged.

Tests (in `color.rs` / `oklab.rs`):
- OKLab reference: white → (1, 0, 0) ±1e-4; red → (0.627955, 0.224863, 0.125846); green → (0.866440, −0.233888,
  0.179498); blue → (0.452014, −0.032457, −0.311528), each ±1e-4; round trip of 100 colours ±1e-4.
- Light mix equals today's numbers (existing `even_spaces_the_stops…` test kept).
- Plain: black→white at 0.5 is sRGB 0.5 (linear ≈ 0.214).
- Even: black→white at 0.5 has OKLab L 0.5 ±1e-3.
- Round the wheel red→blue at 0.5 has hue between theirs the short way (through magenta, h ≈ midpoint); Long way
  passes green-ish (hue on the other side); results in gamut.
- Gap blends: Eased equals smoothstep; Ease in at 0.5 is 0.25 of the way; Steps holds until the middle (0.3 → edge at
  0.3); middle 0.25 puts the halfway colour at 0.25 for Smooth.
- Loop: `sample(1.0) == sample(0.0)`, `sample(1.25) == sample(0.25)`, wrap gap blends last→first with the last stop's
  blend; first stop above 0 wraps correctly.
- `banded(5)`: 10 stops, flat inside each band, colour at band middles; banded looped wraps.
- `set_looped(true)` respaces when the last stop is at 1, not otherwise; `set_looped(false)` respaces back.
- Saving: a new gradient round-trips through RON; old text with `transition: Steps`/`Eased`/`Hue` loads as the gap
  blends / Round the wheel; text without `mix` reads Light, without `looped` false; a stop without blend/middle reads
  Smooth/0.5; grain is never written.

## Task 2: values, companions and the pass (`bpe-core`)

- `animation.rs::Value::lerp`: gradients with equal stop counts lerp `at`, colour and `middle`; `blend`, `mix`,
  `looped` switch at 0.5; grain from `a`. Test updated.
- `vary.rs::starting_to`: turned colours keep each stop's blend/middle and the gradient's mix/looped.
- `lib.rs`: export `Mix, GapBlend, Stop, Gradient` (no `Transition`), `pub mod oklab`, `pub mod gradient_extras`.
- New `crates/bpe-core/src/gradient_extras.rs`:
  - `pub enum ExtraTab { Blend, Shape, Move }` (with `pub const ALL` and `label()`).
  - `pub struct Companion { pub name: &'static str, pub tab: ExtraTab, pub def: SettingDef }` and
    `pub static COMPANIONS: LazyLock<Vec<Companion>>` holding `bands` (decision 8) and `grain` (decision 9), keys
    `"~bands"`/`"~grain"`, labels "Bands"/"Grain", plain help sentences, keyframe + pulse, depth More, group "".
  - `pub const SEP: char = '~'`, `pub fn key(base, name) -> String`, `pub fn split(key) -> Option<(&str, &Companion)>`
    (last `~`, a known name, non-empty base), `pub fn is_companion_key(key) -> bool`,
    `pub fn def_for(base: Option<&'static SettingDef>, c: &Companion) -> Option<&'static SettingDef>` (only on a
    Gradient setting), `pub fn companions_for(base: &SettingDef) -> impl Iterator<Item = &'static Companion>` (every
    companion offered on that gradient: all of them in this part; part 2 filters Shape ones by an `area` flag),
    `pub fn default_of(key) -> Option<Value>`.
  - `pub fn apply_moves(resolved: &mut Resolved, time: f64)` (time unused in this part; part 3 adds the moves before
    Bands): for every thing and card params, for each gradient key with a stored companion, Bands then Grain applied
    to the gradient and to its `~to` when that is a gradient. `pub fn any_in(resolved: &Resolved) -> bool` (any
    companion key stored anywhere: the frame builder skips the clone otherwise).
- `address.rs::def_at` and `registry.rs::layer_def`: after the vary split, a companion key answers
  `def_for(<base def>, c)`.
- `resolve.rs`: a layer's companion keys are copied with its values like vary keys (when `layer_def` finds them).
- `save.rs::clean`: companion keys of a card or thing are kept (clamped) when their base is a gradient setting, dropped
  otherwise (as vary keys). Layers already go through `layer_def`.
- `animation.rs::Animation::new`: decision 10.
- `vary.rs::Scene::vary_use`: `None` for a companion key (decision 11).
- `bpe-runtime/src/world.rs` fingerprint: hash mix, looped, and each stop's blend and middle instead of `transition`.

Tests (`gradient_extras.rs`): split/key round trip and refusal of unknown names; `def_at` answers
`colours~bands` on a card and `3/colours~grain` on a layer, and nothing on a number setting; `set_at` stores and
removes at default; `resolve` copies a layer's companions; `save`/`load` keeps them and drops ones on a non-gradient;
`apply_moves` bands a gradient and its To and copies grain, leaves everything else alone, and `any_in` is false with no
companions; keying Bands from an unstored 0 to 4 animates (decision 10); `vary_use` refuses a companion.

## Task 3: the renderer (`bpe-gfx`)

- `gradients.rs`: `ATLAS_WIDTH = 1024`; `hash` eats mix, looped and each stop's blend and middle (not grain).
  `PaintGpu.steps` → `grain` (docs: strength in 1/255 steps, 0 none). Atlas tests updated (Steps vs Smooth rows differ
  by their stops' blend now).
- `lib.rs`: the grade map gets its own `GRADE_WIDTH = 256` (post.wgsl's `256.0` unchanged); Globals `bg.z` carries the
  background's grain.
- `frame.rs`: `BackgroundPaint.steps` → `grain`. `build_frame_sensing`: the resolved copy is made when the animation
  is non-empty **or** `gradient_extras::any_in`, then `animation.apply` then `gradient_extras::apply_moves(&mut copy,
  time)`.
- `shaders/paint.wgsl`: `PAINT_WIDTH = 1024.0`; `steps` field renamed `grain`; the nearest-texel branch removed;
  `var<private> paint_px: vec2<f32>;`, `fn paint_grain(c: vec4<f32>, grain: f32) -> vec4<f32>` (decision 7, PCG hash
  of the pixel), used at the end of `paint_along` when `grain != 0`.
- Entry points set `paint_px = in.<position>.xy;`: `layers.wgsl` `fs_layers`, `fs_layers_object`,
  `fs_layers_each`; `paths.wgsl` `fs_path`, `fs_path_each`, `fs_path_object`; `scene.wgsl` `fs_background`,
  `fs_shape`, `fs_shape_each`, `fs_ball`, `fs_ball_each`; `styled.wgsl`'s fragment entry. (Where an Out struct lacks a
  position builtin name, read its existing `@builtin(position)` field.)
- `scene.wgsl::background_colour`: the exact two-colour path applies grain too; the atlas path passes `g.bg.z` as grain.
- Tests: `paint_tests.rs`/`layers_tests.rs` lose `steps: 1.0` (hard edges are baked; their Steps gradients now carry
  Steps gap blends); a new GPU test draws a background with Grain 4 over a flat gradient and checks neighbouring pixels
  differ and the average stays within 1/255 of the grain-free picture; one with Grain 0 is byte-identical to before.

## Task 4: the kit (`bpe-kit`)

- `paint/gpu.rs`, `look/layers/gpu.rs` (recolour and `own_paint`), `look/shape/gpu.rs::run_paint`: `steps:` lines
  become `grain: colours.grain`; `background_gpu` sets `grain` and takes the `ends` path per decision 16.
- `paint/eval.rs`: `step_colour` builds its pair from stop `prev` (its blend and middle) to stop `k` with the paint's
  Mix; instant when stop `prev`'s blend is Steps (today's Steps meaning). `offset`: Order uses `index / count` when
  `p.colours.looped`, `index / (count − 1)` otherwise; new `Follow::OrderThereAndBack` (after `Order`): `t = 1 −
  |2·index/count − 1|` (0 for one ball).
- `paint/mod.rs` `Follow` gains `OrderThereAndBack`; every `follows = "…order…"` list (`look/layers/body.rs`,
  `look/basic.rs` ×2, `look/layers/particles/mod.rs` ×2, `paint/mod.rs` test structs) gets `order_there_and_back` right
  after `order`; `paint/mod.rs`'s `span`/`when` terms untouched (Order never used them).
- `look/layers/group.rs`: `Spread::ThereAndBack` after `Order`; `group_t` uses `number / count` for Order and for
  Round's finite rounds (`from_first / (n · span)`) when `gr.colours.looped`; There and back as above; the
  `stretch` `when` term adds `there_and_back` to its exclusions (like Order).
- `paint/palettes.rs`: every built-in is `Gradient::spread(colours, Mix::Even, false)`; Rainbow
  `Gradient::spread(colours, Mix::RoundTheWheel, true)`. Test: Rainbow looped with stops at `i/6`, others Even.
- `paint/harmony.rs::add_matching`: `Gradient::spread(colours, Mix::Even, g.looped)`. Test updated.
- `sensing/probe.rs::work_out`: apply the pass after the animation (and clone when only companions exist).
- `scenes.rs`/tests that build `Stop {…}` or set `transition` move to the new fields (Steps transition → Steps on
  each stop, which samples identically).
- Tests: `offset` Order looped (4 balls → 0, .25, .5, .75) and not looped (0, 1/3, 2/3, 1); There and back for 1, 4
  and 5 balls; `group_t` Order/Round looped and There and back; a ring of 12 balls by Order with looped Rainbow has
  ball 11's colour close to ball 0's (CPU colours, the owner's case); `step_colour` with a Steps stop is instant.

## Task 5: the editor (`bpe-studio`)

`gradient_bar.rs`:
- `BAR_HEIGHT` 22; the transition menu goes. The handle strip draws a diamond (`style::paint_diamond`) per gap at
  `a.at + middle·(b.at − a.at)` (the wrap gap's diamond when looped, placed mod 1), named "{name} middle {i+1}",
  dragged to set that stop's `middle` (clamped 0.01..0.99; the wrap gap measured over its wrapped span). Stops are
  interacted after diamonds so a stop on top wins.
- Selection: `pub fn selected_stop(ctx, id) -> usize` / `select_stop`, temp data `id.with("selected")`; a click or drag
  on a stop selects it (and the click still opens the picker, decision 13). Removing or re-sorting keeps the
  selection on the same stop where it can.
- `one_colour`'s "+" makes the two-stop gradient `Mix::Even`; Random palette keeps the stop count and Loop, Mix Even.
- `pub fn bar_id(label_id: egui::Id) -> egui::Id` so the bar and the tabs share memory.

New `crates/bpe-studio/src/gradient_tabs.rs` (keeps `widgets.rs` from growing):
- `pub fn tab_rows(ui, def, value: &mut Value, bar: egui::Id, cx: &GridCx, right: f32, get: &dyn Fn(&str) -> Value,
  out: &mut GridOutcome) -> bool` (true when the gradient value changed), drawn as grid rows right after the
  gradient's row, only for a gradient (not a companion, not a To) with ≥ 2 stops:
  - The tab row: chips (`style::toggle_button`) for each tab that has something to show — Colours and Blend always,
    Shape/Move when `companions_for(def)` lists any with that tab — named "{label} tab {Tab}". The pick is temp data
    per bar id (session memory), default Colours.
  - Colours: "Stop N" label row with the selected stop's colour well (named "{label} stop colour"), hex box
    ("{label} hex", accepts `#rrggbb`/`#rrggbbaa`, leaves the value on bad text), "at" percent drag ("{label} at",
    moving the stop re-sorts and keeps it selected); "Gap after" chips Smooth/Eased/Ease in/Ease out/Steps ("{label}
    gap {Blend}"), and "Middle" slider 0..100% ("{label} middle"); the gap rows are left out on the last stop of a
    gradient that doesn't loop (decision 1).
  - Blend: "Mix" chips ("{label} mix {Mix}"), "Loop" switch (`style::switch`, "{label} loop", through `set_looped`),
    then each Blend companion as an ordinary settings row (through `widgets::grid_row` with the companion's def under a
    full key, so its label menu, keys, pulses and Follow sound work), then "Every gap" chips ("{label} every gap
    {Blend}") setting every stop's blend at once.
  - Shape and Move: each companion of that tab as a settings row (none in this part).
- Full keys for companion rows: `fn companion_def(base: &SettingDef, c: &Companion) -> &'static SettingDef`, interned
  (a `Mutex<HashMap<(&'static str, &'static str), &'static SettingDef>>`, leaking each once, like paint's `leak`), with
  `key` = `gradient_extras::key(base.key, c.name)`.

`widgets.rs`:
- `setting()` takes `tabs: Option<&mut dyn FnMut(&mut Ui, egui::Id, &mut Value) -> bool>` called right after the main
  row's `end_row` (before a vary To row), its `true` marking the row changed; `grid_row` passes a closure calling
  `gradient_tabs::tab_rows` for gradient defs. `widget()` gives the bar `gradient_bar::bar_id(name)`.
- `pub fn def_in<'a>(defs: &'a [SettingDef], key: &str) -> Option<&'a SettingDef>`: a def by key, answering a
  companion key through its gradient's def. Every `marks` closure that does `defs.iter().find(|d| d.key == key)`
  (`inspector.rs` ×3, `look_editor/mod.rs` ×2, `look_editor/group.rs`, `widgets.rs` test) uses it.
- `grid_row` made `pub(crate)` for `gradient_tabs`.

`timeline/lanes.rs::label_of`: decision 18.

Tests (kittest, in `gradient_tabs.rs` and `gradient_bar.rs`):
- The bar shows a diamond per gap; dragging diamond 1 right raises stop 1's middle; a looped gradient has one more
  diamond.
- Tabs: Colours and Blend show, Shape and Move don't; picking Blend shows Mix, Loop, Bands, Grain, Every gap; the
  pick survives a frame.
- Colours tab: clicking stop 2 selects it (its hex shows); typing a hex sets its colour; "at" moves it; Steps chip
  sets its blend; Middle slider sets middle; the last stop of an unlooped gradient has no gap rows.
- Blend tab: Mix chip sets mix; Loop switch loops and respaces a last-at-1 gradient; Bands row writes
  `colours~bands` through the grid outcome (key and value); Every gap Steps sets every stop.
- Inspector level: on a ball's Body, setting Bands from the tab stores `0/colours~bands` on the Look card and the
  timeline lane is named "… Colours bands" after Create Keyframe.
- Existing gradient bar/widgets tests updated for the removed transition menu.

## Task 6: picture tests (`crates/bpe-kit/tests/gradient_pictures.rs`, GPU, skipped without an adapter)

- **Mix**: a background (Place, straight, across the screen) red→blue drawn in each Mix; the middle column's colour
  matches `Gradient::sample(0.5)` for that Mix within 2/255, and Even's middle is brighter than Light's luma.
- **Loop on a ring**: a Balls ring of 12 by Order with the looped Rainbow palette: the drawn colour of ball 0 and ball
  11 differ by less than the difference between ball 0 and ball 1 plus a margin; the same ring unlooped shows the
  cut (ball 11 near the last stop).
- **Bands**: a background with `bg_colours~bands = 5`: exactly 5 distinct flat colours across, each matching the band
  middle's sample within 2/255.
- **Grain**: covered in Task 3.
- Pinned pictures whose hashes move from the atlas change: run the test before and after, compare every pixel, and
  re-pin only when the worst is ≤ 1/255 (or, per decision 17, the larger ones all sit on a hard edge). Write what moved
  in the PR.

## Task 7: docs and checks

- CLAUDE.md: the gradient value (Mix, gap blends, Loop, oklab), companions (`gradient_extras`, keys `<key>~<name>`,
  tabs, `apply_moves` call sites), atlas 1024 and grain, There and back, the editor row (`gradient_tabs.rs`).
- `cargo fmt`, `cargo clippy --workspace` clean, `cargo test --workspace` green (bar the known release-only runtime
  test), goldens unchanged in debug and release (`git diff` of `crates/bpe-kit/tests/golden*.txt` and sound goldens is
  empty).

## Things not to do

- Don't keep `Transition` (only the private read-side `SavedTransition`).
- Don't put Bands or Grain in the saved value or the editor's value (grain is a draw-time carrier only).
- Don't change any settings-struct default gradient's Mix (pinned pictures).
- Don't let the pass read anything drawn earlier; it is pure in its inputs.
- Don't build the Shape or Move content, Flip/Spread/Random-in-tab/Pick from picture, the big window, or copying a
  gradient with its companions (parts 2–4).
- Don't add example scenes.

## Verification summary

| Check | Passing looks like |
| --- | --- |
| `cargo test -p bpe-core` | OKLab references, gap blends, loop, bands, saving, companions all green |
| `cargo test -p bpe-kit` | Order/There and back/Round, palettes, picture tests, goldens unchanged |
| `cargo test -p bpe-gfx` | atlas, grain and paint tests green |
| `cargo test -p bpe-studio` | kittests for bar, diamonds, tabs, Colours and Blend green |
| `git diff main -- crates/*/tests/golden*` | empty |
| clippy / fmt | no warnings, no diff |
