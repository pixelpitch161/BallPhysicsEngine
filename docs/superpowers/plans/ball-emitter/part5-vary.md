# Ball Emitter part 5: Vary any setting across balls (plan)

Spec: `docs/superpowers/specs/2026-11-05-ball-emitter-design.md` §8 (approved 2026-11-05). This plan fixes
everything §8 left open. Branch `claude/project-thread-qicigs`. Parts 1 (Ellipse) and 2 (Emitter) build at the
same time and merge first; this part is built against main and rebased on them (task 9).

## 0. What the owner gets

Right-click a setting of a Balls group (or an Emitter) and pick **Vary across balls**. The setting gets a second
value, **To**, and each ball takes its own value between the two, spread by **Number, Round, Launch angle, Place,
Random, Speed** or **Hits**, eased **Even, Ease in, Ease out** or **Ease both**. A number row shows a two-ended
slider (the value and To) with "Varies by Number" under it; **Stop varying** in the same menu takes it away.
Both ends can have keyframes. Works for the ball's size and launch speed, the Physics card (Motion and Bounce),
every drawn layer and sub-layer of the Look, the reactions, and trail and particle layers.

## 1. Decisions

1. **Storage** (spec): beside the value in the same params, `"<key>~to"`, `"<key>~by"`, `"<key>~ease"`; a layer's
   keep their prefix (`"3/width~to"`). A setting **varies exactly when its `~by` key is there**. `~to` missing
   means To = the def's default; `~ease` missing means Even. `~by` and `~ease` are stored even when they equal
   their defaults (`set_at` never drops a vary key), so a vary never vanishes by being set to Number/Even.
2. **Defs**: `def_at`/`layer_def` answer a vary key: `~to` gets the base setting's own def (same type, range,
   unit, keyframe flag), so `set_at`, `set_key`, `value_at`, clamping, pulses and Follow sound all work on To with
   no new code. `~by` and `~ease` get two static choice defs (`VARY_BY_DEF`, `VARY_EASE_DEF`), never keyframed.
   A vary key whose base isn't a def (or whose base can't vary) has no def.
3. **What can vary**: by type, Number, Int, Vec2, Color, Gradient (`vary::can_vary_type`). Points, paths,
   strokes, text, choices, bools, targets, routes, slots never. On top, each kind says which of its settings
   vary and how they are read (`VaryUse`): `ThingKind::vary(key)`, `ModuleKind::vary(key)`,
   `LayerKind::vary(key)`, all default `None` except `LayerKind`, whose default is `Some(Live)` for a can-vary
   type (so every drawn layer and sub-layer is in). `VaryUse::Live` = read while the ball lives, every spread
   offered; `VaryUse::AtLaunch` = read once when the ball is let go, Speed and Hits not offered (a ball at
   launch has no hits, and its speed is what is being set).
   - Balls own: `radius` Live, `velocity` AtLaunch. Nothing else (group layout, Variety, Release are group-wide).
   - `motion.basic`: every setting Live. `bounce.basic`: every number setting Live (`takes_over`, `hit_balls`,
     targets are bools/targets anyway).
   - Look layers: everything of a can-vary type, except: the group layers Gradient and Shader (already spread
     across the group: `None`), the Blend layer (it decides how the whole thing is mixed: `None`), and on trail
     and particle layers the keys that set a frame-wide window or route the whole layer (listed in task 6).
   - Scene, camera, folder, shape things and shape cards: nothing (the menu item is only offered on balls).
4. **Where it is offered**: a Balls thing with Group on, or a thing of kind `emitter` (part 2). A single ball has
   nothing to spread over, so the item isn't shown there; a vary left on a group that is switched off reads
   t = 0 (the value) and stays in the file.
5. **Spread maths** (`bpe_core::vary`), all t in 0..1, clamped, NaN → 0, the same as the group gradient's at
   Stretch 1:
   - Number: `number / (count − 1)` (one ball: 0). Count = how many balls the group has in all (released or
     not); an emitter's is part 2's business (it supplies `BallSpread`).
   - Round: the ball's round, first round 0 to last 1 (`round / (rounds − 1)`); a Balls thing: 0.
   - Launch angle: the direction it was launched in, clockwise from +x (y down), `deg.rem_euclid(360) / 360`.
     Balls: the angle of its own launch velocity (`launch_velocity`, before what it rides adds).
   - Place: where the ball starts in the group from left to right in the group's own frame (offset x, from the
     leftmost to the rightmost place; all on one x: 0).
   - Random: `hash(seed, number, salt(key)) / 2³²` — seeded by the group's Seed and the ball's number, and salted
     by the setting's own key (FNV-1a of the key string), so two settings varied by Random are independent.
   - Speed: `|v| / 1500` (group gradient `FULL_SPEED`). Hits: `hits / 10` (`FULL_HITS`).
   - Easing: Even t; Ease in t²; Ease out 1−(1−t)²; Ease both t²/(t²+(1−t)²) (the group gradient's `eased`
     at strength 2).
   - The ball's value = `value.lerp(to, eased(t))` (`Value::lerp`: numbers straight, Int rounded, colours in
     linear light, gradients stop by stop or switching at half when their stop counts differ).
6. **Physics per ball** (runtime): a varied Motion or Bounce card gets a prepared state per *bucket*: t is
   quantised to `VARY_LEVELS` = 1024 steps (`q = round(t·1023)`), a ball's bucket is the tuple of its q's (one per
   varied key of the card), and `prepare` runs once per distinct bucket on a copy of the card's params with the
   varied keys mixed. Rows point at their bucket's state (`Arc`). The cache lives in the world's `Slotted`
   (cleared when `animate` re-prepares the card), so it is a pure function of the params and the bucket:
   deterministic, and a scrub equals a play. Rows are refreshed after the Spawn stage of every step (new rows, and
   every row when a varied key spreads by Speed or Hits) and at the end of `World::build`. 1024 steps of a
   0.5..1 bounciness is 0.0005 apart: far under anything visible; the thing's own Size and Launch speed are exact
   per ball (no buckets).
7. **Which module calls go per ball**: `forces` (a new `ModuleSim::forces_one`, called per alive row instead of
   `forces_at` when the card varies and the module says `per_ball()`), the wall pass's `respond_at`, `effects`,
   the press (`pull`), and in the pair pass each ball's bounciness, friction, press, `respond_pair` and `effects`.
   What decides structure stays per thing (`pairs()` on/off and its target, `special_target`, `takes_over`):
   those are bools/targets and never vary. A module that doesn't say `per_ball()` ignores varies (none such
   today; the test modules in `bpe-runtime/src/testing.rs`).
8. **Thing settings per ball** (kit, Balls): Size = `mix(radius, radius~to, t) × size factor` when the ball is
   pushed. `BallsState.own_size` keeps each row's own size (no growth); `settings_changed` (keys on either end)
   and, when Size spreads by Speed or Hits, `scripted` every step re-work own sizes and move each radius by the
   existing rule (a ball at its own size takes the new one exactly, a grown one keeps its growth in proportion).
   Launch speed: the launch logic (`launch_velocity`) runs from `mix(velocity, velocity~to, t)`; Fan/Outward/
   Random turn it as today, Speed spread on top.
9. **Look per ball** (kit): `layers/vary.rs` `VariedLook` holds the base `Stack`, the Look's varied keys (base key,
   to, by, ease) and a per-frame cache from bucket (same 1024 quantising, tuple over the Look's varied keys) to
   a `Stack` built by `Stack::of` on a copy of the resolved Look slot with those keys mixed. Every place the Balls
   draw reads the stack for one ball reads that ball's stack: the instance (`Stack::instance`: stretch, turning,
   shape), its records (each bucket's records are written once a frame: the existing `range_for` map gains the
   bucket in its key, alongside the "wears" list), the body colour (`colours`), the flashes, ghosts and hit marks.
   Records are what the GPU reads, so no shader changes: a varied stack just names its own record range — the
   cost is only paid by varied stacks, and at most one record set per bucket a frame (≤ 1024 per varied key in
   use, usually far fewer). The program (shader layers) comes from the base stack: own code is text and can't
   vary, so every bucket's program key is the same (debug-asserted). Nothing is kept between frames.
10. **Trails and particles per ball**: the trail draw and the particle emit/draw read the layer's settings per
    ball through a `layer_for(row) -> &AroundLayer` built the same way (bucket cache, per frame). Keys that set a
    window shared by the whole layer stay unvaried (task 6 lists them).
11. **Keyframes on To**: a track at `"radius~to"` (or `"3/width~to"`) is an ordinary track (def from decision 2).
    `physics_animation` and the fingerprint judge a vary key by its base key (`vary::base_key`), so keys on a
    physics To rebuild like keys on the value. Timeline lanes name a To track "<label> (to)".
12. **Editor**:
    - The row menu gains, under Create Keyframe: **Vary across balls** (when offered and not varying) or **Stop
      varying** (when varying). Vary starts with Spread by Number, Even, and To = `vary::starting_to(def, value)`:
      a number goes a quarter of its slider span (range, else span, else ×2 / +1) towards the far end, clamped; an
      Int the same, rounded, never equal; a colour turns a third round the hue wheel; a gradient every stop a third
      round; a Vec2 ×1.5 (zero: (100, 0)). Stop varying removes the three keys and every track and pulse on `~to`
      (one undo step, like every inspector edit).
    - A varied **Number/Int** row's widget is a two-ended slider (`widgets::two_ended`): one rail over the
      slider's span, a knob for the value and one for To (the nearer knob takes a press), the band between them in
      the thing's track colour, then two number boxes "from – to". Accessible names: the row's label for the value
      knob/box, "<label> to" for the To ones. Right-click on the To box opens the value menu for `~to` (Copy, Paste,
      Create Keyframe on To).
    - A varied **Color, Gradient or Vec2** row keeps its normal widget for the value and gets a second grid row
      "↳ To" (painted, no glyph: plain text "To") with the same widget for To.
    - Under every varied row: a small line "Varies by [Number ▾] [Even ▾]" (two `menu_button`s named "Varies by"
      and "Easing", their items the spreads/easings; AtLaunch settings leave out Speed and Hits).
    - All colours from `style` tokens; nothing made up.
13. **Things not to do**: no shader changes; no new state kept between frames for drawing; no format bump (vary
    keys are ordinary params; files without them load as before); don't re-bless goldens (nothing moves without
    a vary); don't vary group layers, the Blend layer, choices or text; don't quantise the thing's own Size and
    Launch speed; don't add example scenes.

## 2. Tasks

### Task 1 — core: `bpe-core/src/vary.rs`

New module, re-exported from `lib.rs` as `pub mod vary` plus the main names.

```rust
pub const TO: &str = "~to"; pub const BY: &str = "~by"; pub const EASE: &str = "~ease";
pub const VARY_LEVELS: u32 = 1024;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum VaryBy { #[default] Number, Round, LaunchAngle, Place, Random, Speed, Hits }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum VaryEase { #[default] Even, EaseIn, EaseOut, EaseBoth }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum VaryUse { Live, AtLaunch }
pub enum VaryPart { To, By, Ease }
pub fn to_key(key) -> String; by_key; ease_key;
pub fn split(key: &str) -> Option<(&str /*base*/, VaryPart)>;  // "3/width~to" -> ("3/width", To)
pub fn base_key(key: &str) -> &str;                             // the key itself when not a vary key
pub fn is_vary_key(key) -> bool;
pub fn can_vary_type(ty: &ValueType) -> bool;
pub static VARY_BY_DEF: LazyLock<SettingDef>; VARY_EASE_DEF  // or consts built with &'static NAMES
pub fn def_for(base: Option<&'static SettingDef>, part) -> Option<&'static SettingDef>; // To: base if can vary
#[derive(Clone, Debug, PartialEq)] pub struct Vary { pub key: String, pub to: Option<Value>, pub by: VaryBy, pub ease: VaryEase }
pub fn varies_in(params: &Params) -> Vec<Vary>;   // every key with a ~by, in key order
impl VaryBy { pub fn live_only(self) -> bool /* Speed|Hits */; pub fn dynamic(self) -> bool /* same */ }
impl VaryEase { pub fn apply(self, t: f32) -> f32 }
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallSpread { pub number: u32, pub count: u32, pub round: f32, pub launch: f32, pub place: f32,
                        pub speed: f32, pub hits: u32, pub seed: u32 }
impl BallSpread { pub fn t(&self, by: VaryBy, salt: u32) -> f32 }
pub fn salt(key: &str) -> u32;                         // FNV-1a
pub fn level(t: f32) -> u16;                           // round(t * 1023)
pub fn at_level(q: u16) -> f32;                        // q / 1023
pub fn mixed(value: &Value, to: &Value, t: f32) -> Value;   // value.lerp(to, t)
pub fn with_varies(params: &Params, varies: &[Vary], ts: &[f32], defaults: &dyn Fn(&str)->Option<Value>) -> Params;
pub fn starting_to(def: &SettingDef, value: &Value) -> Value;
```

Wire-up in core:
- `address.rs` `def_at`: a vary key resolves through `def_for` against its base key's def (thing, card and
  layer paths). `set_at`: a vary key is never removed for equalling the default.
- `registry.rs` `layer_def`: same for layered vary keys (so `save.rs clean_layers` keeps them). `save.rs clean`:
  vary keys of thing/card params are kept when `def_for` gives a def that accepts the value, dropped otherwise.
- `resolve.rs`: a layer's vary keys are copied into the resolved slot params under their layered key (the loop
  over defs only copies defs).
- `registry.rs`: `ThingKind::vary`, `ModuleKind::vary` (default `None`), `LayerKind::vary` (default
  `Some(Live)` when the def's type can vary, `None` otherwise). Helper `Registry::vary_use(scene thing, address)`.
- `edit.rs`: `Scene::start_varying(reg, a) -> Result<()>` (writes `~by`=number, `~ease`=even, `~to`=starting_to),
  `Scene::stop_varying(a) -> bool` (removes the three keys, tracks and pulses on all three addresses).
- Tests (in `vary.rs` and the touched files): split/base/keys; each spread's t at ends and middle; ease ends and
  midpoints; Random independent per salt and stable; Int lerp rounds; `def_at` of `~to` is the base def and of
  `~by` the choice def; `set_at` keeps `~by`=Number; layer vary keys survive save/load and resolve; keyframes on
  `~to` (`set_key` accepted, `Animation` writes them into the resolved params); `start_varying`/`stop_varying`
  round trip leaves params and tracks as before; `starting_to` differs from the value for every type.

### Task 2 — runtime: per-ball module states

- `plugins.rs`: `ThingSim::ball_spread(&self, thing, world, row) -> Option<BallSpread>` (default `None`: the
  runtime fills number from `Bodies.number`, count from the rows, the rest 0). `ModuleSim::per_ball() -> bool`
  (false) and `ModuleSim::forces_one(state, bodies, item, gravity, dt, cx)` (default nothing).
- `world.rs`: `Slotted` gains `rows: Option<Rows>`; `Rows { varies: Vec<Vary>, salts, dynamic: bool,
  states: Vec<Arc<dyn ThingState>> /* by row */, buckets: Vec<Vec<u16>> /* by row */, cache: HashMap<Vec<u16>,
  Arc<dyn ThingState>> }`. `Slotted::state_for(row) -> &dyn ThingState` (the row's, else the card's).
  `World::refresh_rows(sims)` after Spawn every step and at the end of `build`; `animate` re-prepares give a fresh
  `Rows` (empty cache). The spread of each row: `sims.thing(kind).ball_spread(..)` or the default, speed/hits from
  the bodies.
- Use the row state in `move_bodies` (per alive row `forces_one`), `collide` (decider, effects, press per row),
  `pairs.rs` (`Ball` gains `bounciness`, `friction`, `press`; `respond_pair`/`effects` with the row state; holds'
  friction per ball), `press_of` → `press_of_row`.
- `physics_animation` and `fingerprint_of`: `affects_physics(vary::base_key(key))`.
- Not varied stays bit-for-bit as today (goldens).
- Tests (`world.rs` with the runtime's test sims extended by a `per_ball` motion): a varied pull gives each row
  its own fall; buckets are shared (≤ 1024 prepares for 3000 rows); a ball spawned late gets its state; Speed
  spread re-buckets as balls speed up; a world cloned mid-run steps the same as the original.

### Task 3 — kit physics

- `modules/motion`: `per_ball` true, `forces_one` (the same maths as `forces` on one row: so `forces` calls it
  in its loop), `ModuleKind::vary` Live for every key. `modules/bounce`: `per_ball` true, `vary` Live for the
  number keys.
- `things/balls/mod.rs`: `ThingKind::vary`: `radius` Live, `velocity` AtLaunch.
- `things/balls/sim.rs`: `BallsState` gains `varies: Vec<Vary>`, `place: Vec<f32>` (by number), `launch: Vec<f32>`
  (by number), `own_size: Vec<f32>` (by row). `pub fn spread_of(st, bodies, row) -> BallSpread`;
  `BallsSim::ball_spread` uses it. `push_ball` mixes radius and velocity; `settings_changed` and `scripted`
  (dynamic Size only) resize via one `resize_to(own_sizes)`.
- Tests: Size by Number from 10 to 40 over 4 balls gives 10, 20, 30, 40 (exact, × size factor 1); Launch speed by
  Number; keys on Size To move sizes; Bounciness by Number: ball 0 keeps all its speed, ball 3 none (row states);
  Gravity × by Number makes later balls fall faster; Hits-spread Size grows on hits; goldens unchanged.

### Task 4 — kit look

- `layers/vary.rs`: `VariedLook::of(slot: &ResolvedSlot, base: Stack) -> VariedLook`; `fn key(&self, spread) ->
  Vec<u16>`; `fn stack(&self, bucket) -> Rc<Stack>` (RefCell cache); `fn is_plain()`.
- `LayerKind::vary`: `None` on the group Gradient and Shader layers and on Blend.
- `things/balls/draw.rs`: `draw_part`, `colours`, `with_balls`, the flash/hit loop and ghosts read the ball's own
  stack; `range_for` keyed by (wears, bucket). `spread_of` shared with the sim (`sim::spread_of`).
- Tests: a varied Outline width by Number gives each ball a record range whose outline width record differs, from
  the value to To; an unvaried scene's frame is byte-identical to before (records, instances); 3000 balls by
  Number write ≤ 1024 record sets; a varied Body colour turns the instance body colour per ball; a varied Stretch
  amount stretches per ball; Hits spread follows hits.

### Task 5 — kit around layers (trails, particles)

- Trails: `trail::draw::draw` and the ghosts read `layer_for(row)`; `Balls` gains `layer: &dyn Fn(usize) ->
  Rc<AroundLayer>` (or the draw gets it alongside). Kept unvaried (`LayerKind::vary` → `None`): the trail's
  length/seconds (history window), `restart`, `blend`, `blend_amount`, `place`; particles' `life` (memory
  window), `blend`, `blend_amount`, `place`, the cap. Every other number/colour/gradient varies.
- Particles: `ball_emissions` and `simulate`/`shade`/`sprite` read the emission's ball's layer.
- Tests: trail width by Number differs per ball's strip; particle size by Number differs per ball's sprites;
  unvaried frames unchanged.

### Task 6 — editor

- `widgets.rs`: `GridCx.vary: Option<&VaryCx>`; `VaryCx { now: &dyn Fn(&str) -> Option<VaryNow>, offered: &dyn
  Fn(&SettingDef) -> Option<VaryUse> }`; `VaryNow { to: Value, by, ease, to_mark: KeyMark }`; `GridOutcome.varies:
  Vec<(&'static str, VaryEdit)>`; `VaryEdit { Start, Stop, To(Value), By(VaryBy), Ease(VaryEase), KeyTo }`;
  `two_ended` slider; the "To" row; the "Varies by" line; menu items.
- `vary.rs` (studio): `VaryCx` builder from (scene, reg, thing, address fn); `apply` for `Edit::Vary(slot, key,
  edit)` (To through `set_or_key` on the `~to` address; KeyTo through `new_key`).
- Wire `GridCx.vary` in `inspector.rs` `own_grid`, `slot_grid`, `look_editor/mod.rs` `size_row`, `layer_settings`,
  `look_editor/group.rs` (offered set filters out what doesn't vary); other grids pass `None`.
- `timeline/`: a To track's lane label "<label> (to)".
- kittest tests: Vary across balls appears on a group's Size and not on a single ball's; picking it writes `~by`
  and `~to`; the two knobs/boxes are found by name and editing To writes `radius~to`; Varies by menu changes `~by`;
  Stop varying removes the keys; launch speed's menu lacks Speed/Hits; design system colour test still passes.

### Task 7 — docs, lint, full tests

CLAUDE.md (bpe-core vary keys, runtime per-ball rows, kit VariedLook, editor two-ended slider), `cargo fmt`,
`cargo clippy --workspace`, `cargo test --workspace` (debug) and the goldens in release, all unchanged.

### Task 8 — rebase on parts 1 and 2

When they merge: rebase; give the emitter `ThingKind::vary` (its Size Live, Speed AtLaunch), `ThingSim::ball_spread`
from `EmitterState` (number, count so far, round, launch angle, place along the start line/ring), and route the
generalised ball draw through `VariedLook`. If part 2 is still open when everything else is done, open the PR
against main and say so; the emitter wiring follows as a small PR.

### Task 9 — one Opus review, PR

## 3. Verify

- Every new test passes; `cargo test --workspace` green except the known release-only debug test.
- Goldens unchanged in debug and release (no vary in any test scene).
- Frame-identity test for unvaried scenes (task 4) passes.
