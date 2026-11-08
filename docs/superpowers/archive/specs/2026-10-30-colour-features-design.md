# Colour features: design

Date: 2026-10-30. Status: approved in conversation, section by section; awaiting review of this document.

## Goal

Today a ball group's colours are a flat list spread evenly and blended smoothly, and each ball
is one solid colour picked by its starting row, order, speed or hits. Other things have one
colour each, and the only blending choice is Additive. This change gives every thing one shared,
much richer colour system: gradients with movable stops and transition styles, colours spread
across a group or painted inside each thing, colours that react to place, hits, the beat and
time, per-thing variation, blend modes, palettes, a harmony helper and a scene colour grade.

## Decisions (Aedan's picks)

1. Multiple colours can appear **across the group** (each ball one colour) and **inside each
   thing** (a gradient painted on it), as separate options that can be used together.
2. Blending means both: **transition styles** between gradient colours, and **blend modes** for
   how a thing draws over what is behind it.
3. All eight extra ideas are in: palettes, colour by live place, colour on hit, variation drift,
   colour by music, scene colour grade, harmony helper, animated gradients.
4. Build approach A: **one shared Colours setting** used everywhere. (Rejected: a separate Colour
   module slot beside Look, which splits colour settings across two places; shader-node templates,
   which means wiring nodes instead of moving sliders.)
5. **Every thing** gets the colour features, adapted to what makes sense for it.
6. **Clean break:** old scene files do not have to load. Old single `Colour` and list `Colours`
   settings merge into one setting.

## Scope and build order

One spec, built in three parts, each its own plan, PR and Windows build:

1. **Gradients:** the `Gradient` value and editor, Follow modes (place, order, speed, hits, life,
   step on hit, beat, time), inside patterns, transition styles, all things converted.
2. **Blending and variation:** blend modes (cheap and layered), Blend amount, variation sliders
   and wobble.
3. **Palettes, harmony, grade:** palette library and file, harmony helper, Scene Grade card.

Before part 1 starts, merge latest main; the camera/world thread
(`docs/superpowers/specs/2026-10-30-camera-world-design.md`) changes the renderer (world vs
screen space, stuck-to-screen things, liquid/swarm drawing). Whichever lands second adapts.

## 1. The Gradient value

`bpe-core/src/color.rs` gains:

```rust
pub struct Stop { pub at: f32, pub color: Color }          // at in 0..=1
pub enum Transition { Smooth, Eased, Steps, Hue }
pub struct Gradient { pub stops: Vec<Stop>, pub transition: Transition }
impl Gradient {
    pub fn solid(c: Color) -> Self;
    pub fn even(colors: &[Color]) -> Self;                  // stops spaced evenly
    pub fn sample(&self, t: f32) -> [f32; 4];               // linear light
    pub fn bake(&self, n: usize) -> Vec<[f32; 4]>;          // for the GPU
}
```

- Stops are kept sorted by `at`; at least one stop. One stop means a solid colour.
- **Transitions:** Smooth = linear in linear light (today's maths). Eased = smoothstep between
  neighbouring stops. Steps = hard edge halfway between stops' positions, so each colour owns a
  band (a stop's band is set by dragging stops). Hue = travel the short way round the colour
  wheel in HSV, lerping saturation and value.
- `Value::Colors(Vec<Color>)` and `Value::Color(Color)` for paint settings are replaced by
  `Value::Gradient(Gradient)` and `ValueType::Gradient`. `Value::Color` stays for colours that
  are never gradients (3D lights and similar). `Value::lerp` blends two gradients with the same
  stop count stop by stop (colour and position); different counts jump at the halfway point.
- Save format: `stops: [(0.0, "#ff3ec8"), (1.0, "#3e9bff")], transition: Smooth`. A one-stop
  gradient may be written as a plain hex string and reads back as `Gradient::solid`.

## 2. The Paint block

Every colourable part of a thing uses one shared block of settings, a `Paint`:

| Key (prefix + ) | Setting | Notes |
|---|---|---|
| `colours` | Colours (Gradient) | keyframe, random |
| `follow` | Follow (choice, per thing kind) | see section 3; shows when more than one stop |
| `angle` | Direction (degrees) | Place: direction |
| `centre` | Centre (Vec2) | Place: from centre |
| `space` | World / Screen | Place modes; World default |
| `span` | Spread (px) or "Colours end at" (speed/hits/seconds) | depends on Follow |
| `repeat` | Repeat (Clamp / Repeat / Mirror) | what happens past the end |
| `scroll` | Scroll (cycles per second) | animates any Follow; Time uses it as its speed |
| `inside` | Inside pattern (None / Stripes / Rings / Sweep) | things with an area |
| `inside_angle`, `inside_spin`, `inside_scale`, `inside_scroll` | Inside settings | |
| `vary_hue`, `vary_bright`, `vary_sat` | Variation (part 2) | per-instance random offset |
| `wobble` | Wobble speed (part 2) | 0 = fixed offsets |

- `bpe-settings-derive` gains `#[setting(paint, prefix = "...")]` on a field of type `Paint`,
  which expands into the keys above with that prefix and with `when` conditions joined to the
  field's own. `Paint::defs_for(kind)` hides Follow choices that a kind does not support (see
  the table in section 4), so `check_registry` still validates every condition.
- `bpe-kit/src/paint/` (new) holds `Paint`, `Follow`, and `paint::colour_at(paint, Inputs) ->
  [f32; 4]` where `Inputs { index, count, place_world, place_screen, start_place, speed, hits,
  life, last_hit_colour, beat, time, seed }`. Every thing's draw code calls this one function;
  nothing samples a gradient on its own.
- Inside patterns are drawn on the GPU (section 5). The CPU computes each instance's base offset
  `t0` from Follow; the shader adds the pattern's position and samples the baked gradient.

## 3. Follow modes

| Follow | t (before Repeat) |
|---|---|
| Place: direction | dot(position − centre, dir(angle)) / Spread + 0.5 |
| Place: from centre | distance(position, centre) / Spread |
| Place: along path | progress along the arena outline / obstacle path (walls, trails) |
| Starting place | as Place: direction, but with the position where the instance was spawned (today's "Row" is angle 90° on this) |
| Order | index / (count − 1) |
| Speed | speed / Colours end at |
| Hits | hits / Colours end at |
| Life | age / lifetime (particles, trails, emitter spawns) |
| Step on hit | (hits mod stops) picks a stop; Transition then blends over Step time (seconds) |
| Take hit colour | the colour of the thing it last hit (a switch beside Step on hit) |
| Beat | beats since start on the **video clock**; Steps steps one stop per beat, others slide; pulse shape from the Beat setting group |
| Time | time × Scroll on the simulation clock |

- Place uses `space`: **World** reads world positions (colours stay fixed in the world as the
  camera moves); **Screen** maps through the camera to screen space (colours stay fixed in the
  shot). Before the camera thread lands the two are the same.
- Hits and Beat follow the two-clock rule from stage 7: hits on the simulation clock, beats on
  the video clock.
- Scroll adds `time × scroll` to t for every mode; with Repeat it gives animated gradients.
- "Hue shift per hit" and Rainbow stay as they are and apply after Paint.

## 4. Things and their paints

| Thing / module | Paints | Follow choices | Inside |
|---|---|---|---|
| Balls (classic and styled look) | Fill; styled also Pattern colour, Outline | all | Stripes, Rings, Sweep on the ball, turning with spin |
| Soft bodies | Fill | Place, Order (per point), Time, Beat, Hits, Step on hit | yes, over the body's shape |
| Arena walls, obstacles | Fill | Place (incl. along path), Order (pegs in a grid), Hits, Step on hit, Time, Beat | yes, across the shape |
| Trails | Colour (Source: ball's own / own Paint) | Life (head → tail), Place, Speed, Time, Beat | no |
| Particles | Colour (Source: ball / own Paint) | Life, Order (random in list), Place, Speed, Time, Beat | no |
| Liquid | Colour | Place (depth/height via angle), Speed, Time, Beat | no |
| Swarm | Colours | Speed, Nearest ball, Place, Order, Time, Beat | no |
| Text | Fill, Outline | Place, Order (per letter), Time, Beat | yes, per letter or whole line (switch) |
| Scene background | Background | Place (direction / from centre), Time, Beat; always Screen | no |

- Secondary paints on a thing (Pattern colour, Outline) sample at the main paint's t by default,
  with their own Follow available; each is a full Paint block.
- The Scene's `background_top`/`background_bottom` become one Background paint (angle 90°
  reproduces today's look).
- Swarm `colors_by` and trail/particle `source` choices fold into Follow/Source as listed.

## 5. Rendering

- **Gradient atlas:** each frame, `bpe-gfx` collects every distinct gradient used by an inside
  pattern, bakes it to a 256-texel row of an `Rgba16Float` texture (rows reused by hash), and
  passes the row index in the instance. `LookInstance` gains `grad_row`, `t0`, `pattern`,
  `pattern_angle`, `pattern_scale`. Wall/obstacle/soft/text instances gain the same fields.
- Instances without an inside pattern keep today's path: one linear colour per instance from
  `paint::colour_at`.
- Transitions are baked into the atlas, so the shader only does `textureSample(row, fract/clamp(t))`
  with nearest filtering for Steps and linear for the rest.

## 6. Blend modes (part 2)

Every thing gets `blend` (choice) and `blend_amount` (0..1, keyframe):

- **Cheap modes**, fixed-function blend states: Normal, Add, Screen, Multiply, Lighten, Darken,
  Subtract. Draw lists are grouped by (layer order, blend mode); pipelines are created per mode.
  Blend amount scales towards Normal in the shader (premultiplied maths). Screen clamps inputs
  to 0..1 since the picture is HDR.
- **Layered modes**: Overlay, Soft light, Difference, Hue, Colour. The thing draws into its own
  `Rgba16Float` layer at picture size, then a composite pass reads a copy of the picture under it
  and mixes. Cost is one extra layer and copy per thing that uses one; zero otherwise.
- Liquid and swarm blend their whole layer. Existing trail/particle "Add / Cover" choices are
  replaced by the shared Blend setting. The colour picker's "Additive" checkbox is removed: Blend
  = Add replaces it, and the negative-alpha trick stays only as the shader's internal encoding.

## 7. Variation (part 2)

`vary_hue` (degrees), `vary_bright` and `vary_sat` (0..1): each instance gets a random offset
from its seed, applied in HSV after Follow. `wobble` (Hz) slowly drifts each offset with smooth
seeded noise; 0 keeps them fixed. Instances are stable across scrubbing (seeded, no state).

## 8. Palettes and harmony (part 3)

- A **Palette** button on every Colours editor opens a menu: about 20 built-in palettes
  (`bpe-kit/src/paint/palettes.rs`: sunset, neon, pastel, ocean, forest, candy, fire, ice, mono,
  retro, vapor, earth, …), the user's saved palettes, "Save as palette…", and a dice for a random
  palette. Picking one replaces the stops and transition.
- Saved palettes go in `palettes.ron` beside the app executable (`%APPDATA%`-style fallback if it
  is read-only), shared by every scene. Scenes copy colours in; they never link to a palette.
- **Harmony helper**, in the same menu: "Add matching colours" → Complementary, Triad,
  Analogous, Split complementary, Shades, Tints, built from the first stop and added as evenly
  spaced stops.
- The randomize dice (`random.rs`) uses the palette list as one of its sources.

## 9. Scene colour grade (part 3)

A **Grade** card on the Scene: Brightness, Contrast, Saturation, Warm/Cool, Tint (green/magenta),
Vignette, Vignette size, and Gradient map (a Paint-style Colours with an Amount slider, mapping
the picture's brightness through the gradient). All keyframeable and pulse-able. It runs in
`post.wgsl` in **screen space**, after the shader effect chain, bloom and tonemapping, on display-ready colours so the sliders behave like a photo editor.
All sliders at their defaults is an exact no-op (the pass is skipped).

## 10. Editor

- One **Colours row** (`Row::Colours`, `widgets.rs`) for every paint: a gradient bar with
  draggable stop handles (click the bar to add, drag off or right-click to remove, click a
  handle to open the colour picker), a transition dropdown, the Palette button, and a fold arrow
  for the rest of the Paint settings, each shown only when it applies (`when`).
- A one-stop gradient shows as a single swatch, like today's Colour row.
- Keyframing a Colours row keys the whole gradient (as today).
- Rule kept from #43: no `.context_menu` on an egui colour button; stop handles use
  `Popup::context_menu(..).id(...)`.

## 11. Testing

- Unit tests in bpe-core for `Gradient::sample` per transition, stop sorting, lerp, save/load.
- Unit tests in bpe-kit for every Follow mode in `paint::colour_at`, including world vs screen.
- Picture tests (lavapipe) in bpe-gfx: each inside pattern, each cheap and layered blend mode,
  Blend amount 0 equals Normal, and the grade (defaults = bit-identical skip).
- Inspector kittest: gradient bar adds/moves/removes stops, palette apply, harmony add.
- Golden **physics** logs must not change (colours never touch physics). Golden picture hashes
  and the built-in scenes are updated to the new settings; scenes are rewritten to use Gradient
  where they used `gradient` lists.

## Out of scope

3D-mode inside patterns and layered blends (3D keeps per-instance colours and cheap blends);
per-stop keyframes (the whole gradient is keyed); colour nodes in the node editor beyond what
already exists.
