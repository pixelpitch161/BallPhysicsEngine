# Lighting part 6: light shows (the Rig and Lighting scenes)

Spec: `docs/superpowers/specs/2026-11-06-lighting-design.md` ("Light shows": a **Rig** thing, a folder for lights,
with patterns that drive its lights on the tempo (Chase, Strobe, Sweep, Alternate colours, Random; on beats, bars or
hits; colour lists; direction), and **Lighting scenes** on the Lighting card: named snapshots of every light's
settings, switched or crossfaded on bars, hits or timeline marks). Builds on part 1a (`things/light/`,
`modules/lighting.rs`, `ThingDraw::light`). Built straight after this plan (owner's overnight rule: no stops), one
Opus review at the end, then the PR, merged by this thread once tests and the review pass.

Everything here is **draw-only**: it changes the `LightGpu`s a frame hands the renderer and nothing else. No
physics, sound or no-card picture changes; goldens never move.

## What part 6 delivers

- **The Rig thing** (`rig`, family Lights, `bpe-kit/src/things/rig/`): a folder for lights. Every light under it (in
  tree order, nested rigs included) is one of its **fixtures**. Its pattern works out, for each fixture at the moment
  drawn, a brightness, maybe a colour from its list, and a turn and sway, and the light's draw applies them.
- **Patterns**: Still, Chase, Strobe, Sweep, Alternate colours, Random. **Steps on** Beats, Bars, Hits or Seconds.
  **Order** Forward, Backward, Back and forth, From the middle out, To the middle, Shuffled. **Colours** from the
  lights or from the rig's colour list.
- **Lighting scenes** on the Lighting card: a list of named snapshots of every Light's and Rig's settings, shown
  **On the timeline** (a hold-keyed "Show scene" setting: the timeline's keys are the marks), **In turn** every N bars,
  or **On hits**, with a **Crossfade** time. While a scene shows, its saved settings stand in for the lights' own.
- Editor: Rig in "+ Add thing ▸ Lights" (a new rig comes with four spots), its icon, lights added into a selected rig,
  the Rig's settings in the inspector, and the **Scenes** list on the Lighting card (save the lights as a scene,
  rename, update, put back on the lights, duplicate, move, delete) with a "Showing" readout.

## Decisions made while planning (each a fill-in of the spec, stated here)

1. **Fixtures are found from the tree.** A rig drives every enabled Light thing under it, in tree order. A light
   inside a nested rig is a fixture of both; the rigs' effects stack (brightness multiplies, turns and sways add, the
   nearest rig whose colours come from its list sets the colour). A rig has no Placement card (like lights) and no sim:
   it never moves, and its settings never touch physics.
2. **Riding through a rig.** A light rides the nearest ancestor that is not a rig: a rig inside a Balls group makes
   its lights ride the balls (`places` walks up past rigs). Adding a light while a rig is selected puts it inside the
   rig; when that rig sits in a Balls/Emitter/Shape thing the light starts at offset 0 like any rider.
3. **Fixtures or lamps.** "Steps across" picks what the pattern counts: **Lights** (each Light thing is one fixture;
   a light riding each ball lights all its lamps alike) or **Every lamp** (each ball's lamp is its own fixture, so a
   chase runs ball to ball). Lamp counts come from `light::draw::places` of each earlier fixture.
4. **`DrawCx.tree: &Scene`.** The draw context gains the scene tree (the light needs its rig's other fixtures and the
   Lighting card's keyframe track). Six constructions change; nothing else in the renderer does.
5. **Pure in time.** A pattern is worked out from the moment drawn (and, on hits, the log up to it): step `k` and the
   phase `φ` inside it. Beats/Bars/Seconds: `x = t / step_length`, `k = floor(x)`, `φ = x − k`. Hits: every
   `Every (hits)`-th heard hit at least `Least gap` after the last one counted starts a step; `φ` = time since it /
   `Step lasts`; before the first hit the step is 0 at phase ∞ (a chase head is lit, a strobe dark). Scrubbing and
   export draw the same. The tempo is the scene's Tempo and Beats in a bar / Beat note, the same maths as
   `bpe_sound::TempoMap` (kit's `rig::beat_seconds`, same clamps; a test keeps them equal).
6. **Fixture order.** Order maps fixture `i` of `n` to a place `p` among `m` places: Forward `p = i`; Backward
   `p = n−1−i`; From the middle out `p = |2i − (n−1)| / 2` rounded down (`m = ⌈n/2⌉`, the middle pair lights together);
   To the middle the reverse; Shuffled a seeded permutation; Back and forth uses Forward places with a head that bounces
   `0..m−1..1` (period `2(m−1)`).
7. **Chase**: the head is at place `k mod m` (or the bouncing head). A fixture is lit while its place is within
   **Lit at once** `W` places behind the head (`W` ≥ 1, fractional W rounds up). After it goes out it fades over
   **Tail** steps: `level = 1 − ((j − 1) + φ) / tail` where `j` steps ago it was last lit (searched back one period).
   Its colour from the list is the step it was last lit: `colours[(k − j) mod len]`.
8. **Strobe**: every fixture flashes at the start of each step for **Flash length** (share of a step), then fades over
   Tail; colour `colours[k mod len]`.
9. **Sweep**: brightness 1; the direction turns by `½·Sweep angle·sin(π·x + 2π·p·Offset)` and the place sways
   sideways by `½·Sway·sin(…)` (one swing there in one step, back in the next); colour `colours[p mod len]`.
10. **Alternate colours**: brightness 1; colour `colours[(p + k) mod len]`, the list shifting by one place each step.
11. **Random**: each step each fixture is lit with chance **Lit share** (`hash(seed, k, i)`), fading over Tail, with
   colour `colours[hash mod len]`.
12. **Still**: brightness 1, colour `colours[p mod len]` when the colours come from the list (a way to colour a rig).
13. **Brightness** (×, keyframes, pulses with hits) multiplies every fixture; **Off level** is what an unlit fixture
   keeps: `strength × Brightness × (Off + (1 − Off)·level)`.
14. **Colour list** is a `Gradient` setting (`colours`), its stops in order being the list, edited with the palette
   bar every colour list in the program uses. Colours from: **The lights** (default) / **The list**.
15. **Lighting scenes are a new value**, `Value::LightScenes(Vec<LightingScene>)` in core
   (`light_scenes.rs`: `LightingScene { name, lights: Vec<SceneLight { thing, params }> }`), the Lighting card's
   `scenes` setting (never keyframed). A scene holds, for every Light and Rig thing when saved, its settings as they
   stood at the playhead (keyframes applied), minus what is structure, not look: a light's Kind, Runs along, Shape,
   Area shape, Rides on, Ball number, Heard from (`STRUCTURE`). Lights added later aren't in older scenes and keep
   their own settings in them.
16. **Showing a scene.** `Scenes play`: **Off** (default: nothing changes), **On the timeline**, **In turn**, **On
   hits**. On the timeline, **Show scene** (Int, `hold`, keyframes; 0 = the lights' own settings) picks it; its keys
   are the timeline marks. In turn: a new scene every **Every (bars)** bars, in **Order** (In turn / Back and forth /
   Random, seeded), scene 1 from the start. On hits: each heard hit at least Least gap apart moves to the next, in the
   same Order. A change crossfades over **Crossfade** seconds from the scene before (0 snaps): numbers, points and
   colours blend (colours in linear light), whole numbers round, everything else (choices, switches) switches at the
   halfway point. While a scene shows, its saved settings replace the light's own, keyframes and Follow sound
   included; a key it doesn't hold is the light's own. A change that comes while an earlier crossfade is still running starts its own fade from
   the scene before it (not from the blend on screen), so it can jump; set keys at least a Crossfade apart for
   smooth chains. Back and forth chases light the places behind the head the way it is going.
17. **Order of application** for one light: its resolved settings (keyframes, pulses, Follow sound) → the showing
   Lighting scene → its rigs' patterns (rigs' settings taken through the scene too) → `LightGpu`.
18. **Editing scenes** (kit functions in `modules/light_scenes.rs`, used by the editor, each one undo step):
   `capture` (every light and rig at a time), `add_scene` (named "Scene N"), `update_scene`, `put_on_lights` (writes
   the scene's values back: as keys at the playhead where the setting has keys, else plain), `rename_scene`,
   `duplicate_scene`, `move_scene`, `remove_scene`. Moving or removing renumbers the Show scene value and its keys so
   they keep naming the same scenes (a removed scene's keys become 0, the lights' own).
19. **A new rig** comes with four Spot lights in a row across the top (x −360, −120, 120, 360; y −760; cone 40°;
   pointing down; warm white) so the pattern shows at once; default pattern Chase on beats, Lit at once 1, Tail 1,
   Off level 0.1, colours a sunset list (from the lights until switched). Adding a rig also adds the Lighting card
   when there is none (as the first light does).
20. **No limits**: every new number is a `span`. Hard limits only where the maths breaks: Every (beats/bars/seconds)
   `range(0.001, …)` (zero divides by zero), Every (hits) `range(1, …)`, Step lasts `range(0.001, …)`, Lit at once
   `range(0, …)` (a negative count means nothing), Tail `range(0, …)`, Crossfade `range(0, …)`, Least gap `range(0, …)`,
   Show scene `range(0, …)`. Brightness, Off level, Flash length, Lit share, Sweep angle, Sway, Offset are spans only
   (an Off level past 1 makes lit fixtures the dim ones, which is allowed).

## Files and types

### bpe-core
- `light_scenes.rs` (new): `LightingScene`, `SceneLight`, `LIGHT_SCENES_KEY = "scenes"`; `is_valid` (finite values).
- `value.rs`: `Value::LightScenes`, `ValueType::LightScenes` (never keyframed; accepts valid lists).
- `settings.rs`: `SettingValue for Vec<LightingScene>`; `is_set`-style match arm.
- `lib.rs`: exports.
- `save.rs` test: scenes round-trip.

### bpe-runtime
- `world.rs`: the value hasher learns `Value::LightScenes` (tag 14; debug text like Routes/Slots).

### bpe-gfx
- `frame.rs`: `DrawCx.tree: &'a Scene`; `build_frame_with` passes it. Tests updated.

### bpe-kit
- `things/rig/mod.rs` (new): `RigKind` (`rig`, family Lights, icon "rig"), `RigSettings` (Pattern, Steps on, Every
  (beats/bars/seconds/hits), Step lasts, Least gap, Heard from, Order, Steps across, Lit at once, Tail, Flash
  length, Lit share, Sweep angle, Sway, Offset, Colours from, Colours, Brightness, Off level, Seed), choices
  `Pattern`, `StepsOn`, `Order`, `Across`, `RigColours`; `add_rig` (rig + four spots).
- `things/rig/pattern.rs` (new): `Clock` (step, phase), `clock_at` (beats/bars/seconds/hits), `place_of`,
  `Drive { level, colour, turn, sway }`, `drive(settings, clock, i, n) -> Drive`, `beat_seconds`, `hits_heard`.
- `things/rig/fixtures.rs` (new): `rigs_of(tree, light)`, `fixtures(tree, resolved, rig)`, `Applied` (the stacked
  drive of every rig over one lamp) and `apply_rigs(cx, light_id, lamps) -> Vec<Drive>`.
- `modules/light_scenes.rs` (new): `ScenesPlay`, `SceneOrder`; `Showing { from, to, mix }`, `showing(cx)`,
  `params_with_scene(cx, thing, own) -> Cow<Params>`, `blend_params`, `blend_value`; the edit functions of decision
  18; `STRUCTURE`.
- `modules/lighting.rs`: Scenes group settings (`scenes_play`, `show_scene` hold, `scene_every`, `scene_order`,
  `scene_heard`, `scene_gap`, `scene_fade`, `scenes`, `scene_seed`).
- `things/light/draw.rs`: `light` reads `params_with_scene`, then `places`, then `apply_rigs`, then pushes each lamp
  with its drive (strength × level, colour, direction + turn, place + sway ⟂ to x). `places` rides the nearest
  non-rig ancestor. `add_light` rides likewise.
- `starters.rs`: adding a `rig` adds the Lighting card when there is none.
- `lib.rs`: register `RigKind`; `scenes.rs`: `light_show` test scene (a rig of spots over balls, three lighting
  scenes) used by tests and the demo render.

### bpe-studio
- `tree.rs`: Rig icon (`Icon::Rig`: a bar with three hanging cones); "Rig" listed in the Lights family (by the
  generic entry, which calls `add_rig`); AddLight goes inside a selected rig.
- `inspector.rs`: no Placement card for rigs; the Lighting card shows the Scenes list under its grid
  (`light_scenes_panel.rs`).
- `light_scenes_panel.rs` (new): "Showing: Scene 2 → Scene 3 · 40%" readout, the scene rows (name, menu: Rename /
  Update from the lights / Put on the lights / Duplicate / Move up / Move down / Delete), "+ Save the lights as a
  scene", and "Show here" on a row when Scenes play On the timeline (sets Show scene, a key when keyed).
- `widgets.rs`: `Value::LightScenes` words ("3 scenes") and its grid row.

Review fixes (one Opus review): a rig's colour list and places are prepared once per rig a frame (`Prepared`), a
plain chase finds how long ago a light was lit directly, Back and forth lights behind the head only, a rig can't
be grabbed in the preview, Every (hits/bars) and Least gap keyframe, the beat maths has a test against `TempoMap`.

## Tests (what passing looks like)

- **Pattern units** (`rig/pattern.rs`): a forward chase lights fixture k at beat k and only it; its tail fades
  linearly; backward and from-the-middle orders; back-and-forth bounces; strobe flashes for Flash length; alternate
  colours shift by one each step; random is the same for the same seed and moment and differs with the seed; hits
  steps count only heard hits a gap apart; `beat_seconds` equals `TempoMap` for a few tempos (in bpe-sound's tests).
- **Frame tests** (kit, CPU only, through `build_frame_with`): a rig of four spots with a chase gives one bright light
  per beat moving along; a rig in a Balls group with Every lamp chases ball to ball; nested rigs multiply; Colours from
  the list colour the light; Sweep turns a spot's direction; a light outside any rig is unchanged.
- **Lighting scenes**: capture/put back round-trip; a scene on the timeline replaces strength and colour; a crossfade
  halfway blends numbers and colours and switches choices at half; In turn changes every N bars; On hits advances on
  heard hits; Off changes nothing; removing a scene renumbers Show scene keys; save/load keeps scenes.
- **Untouched**: physics goldens, sound goldens and `golden-unlit.txt` (no-card pictures) pass unchanged.
- Editor kittests: Rig in the Add thing menu adds a rig with four lights; a light added with a rig selected goes
  inside it; the Lighting card's "Save the lights as a scene" adds a scene; Delete removes it; the design-system
  colour test.
- `cargo clippy --workspace`, `cargo fmt`.

## Things not to do

- No state kept between frames; nothing physical reads rigs or scenes; no physics/sound/no-card picture changes.
- No hard ranges for taste; no built-in scenes in the program (`light_show` is test-only); no colours outside
  `style/`.
- Don't touch the Lights tab or handles (part 1c) beyond what this part needs.
