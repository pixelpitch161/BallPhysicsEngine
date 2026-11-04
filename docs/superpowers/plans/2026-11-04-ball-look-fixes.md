# Ball Look editor: fixes and small changes (2026-11-04)

Aedan's list after parts 1 to 3 (PRs #72, #74, #73). Twelve items; each is below with what was found, the decision
taken, the files touched, and how it is verified. One branch, one PR, one Opus review at the end.

Goldens: nothing here moves balls, so `crates/bpe-kit/tests/golden*.txt` must stay byte-identical (checked in debug
and release). Picture tests that pin the classic shiny ball must stay within their tolerances (Shine at its defaults
is unchanged).

## 1. "How many" always types a 1

**Found.** Reproduced in a kittest: typing 5, 0, 7 into How many gave 1, 11, 121. The setting spans 1 to 5000, so
`widgets::number_feel` gives a step of 10, and egui's slider snaps every value it stores to `start + k·step`. A typed
5 becomes 1, a typed 10 becomes 11, and so on. Every setting whose span is 2000 or more has the same fault.

**Fix.** `number_feel` never gives a step above 1 (item 3). Typed values then land on whole numbers. The kittest
`typing_how_many_sets_it` (group.rs) types 2 then 5 and expects 25, and a widgets unit test pins the steps.

## 2. Remove the launch arrow from the Group view

`group_view.rs`: drop `Drag::Arrow`, `arrow_tip`, `ARROW_SECONDS`, the arrow's drawing and grabbing, and the arrow's
room in the view's fit. Launch speed and direction stay in the Launch fold (the Balls settings). Tests that drag the
arrow are removed; a test checks a press where the tip was grabs nothing.

## 3. Slider steps too large

**Decision.** The step is the power of ten nearest a two-hundredth of the span, **capped at 1**: 0 to 1 still moves
by 0.001 and 0 to 40 by 0.1, but 0 to 2000 and 1 to 5000 move by 1. The number box's drag speed is capped at 1 per
pixel for spans of 200 or more (Shift stays ten times finer), so dragging the box also counts in ones. Doc comment
and `number_feel` tests updated.

## 4 and 5. Gradients per ball (Body and each layer)

**Body.** `Follow` gains `EachBall` ("Each ball"), offered only by the Body (first in its `follows` list) and the
Body's new default. With Each ball and more than one colour, the gradient is painted across each ball: Gradient
Straight (by Direction) or Round (centre to edge). The paint's Direction and Gradient rows show for Each ball too;
Centre, Positions in, Span and Repeat don't. The ball's single "body colour" (for Same as body layers, trails,
particles, hit marks) is the gradient's middle colour.

- GPU: the body record carries a `PaintGpu` (`paint.row` ≥ 0, pattern 1 linear or 2 radial, angle, scale 1, clamp).
  The walk works out the body colour at the point once (`body_c`) and every "same as body" use reads it: the Body
  itself, an outline or glow Same as body, Shine's fallback, a pattern that is a shade of the body.
- Reactions: Colour over time turns the gradient's stops on the CPU (as patterns' own gradients already do). Colour on
  hit and the colour Variety turn each ball's gradient through the instance's `vary.w` (turns), which the walk adds
  to the body paint's hue only. The body paint's own Variation (vary hue/sat/bright) uses `vary.xyz` as patterns do.
- Group gradient changing the Body with a per-ball body gradient: mixed on the GPU into the body record (flag
  GROUPED), since the CPU mix can't reach a gradient.

**Each layer.** Outline, Glow and Shading's `colour` becomes a gradient (`Value::Gradient`, one stop = a plain
colour, still keyframeable). When it has more than one colour (`when = "... and colour"`, a gradient with several
stops counts as on), two rows show: **Gradient** Straight / Round and **Direction**. Straight runs across each ball
at Direction; Round runs: Outline from the ring's inner edge to its outer, Glow from the ball's edge out to the
glow's end, Shading from the ball's centre to its edge. Pattern already has its own gradient. Shine has no colour.

## 6. Shine material, and 12. Shine softness

`ShineLayer` gains **Material** (`ShineMaterial`: Glossy, Matte, Metal, Glass; Glossy default) and **Softness**
(0 to 1, default 0.5, keyframe). Record: `p0.w` material code, `p1.x` softness.

- Softness scales the highlight's exponent by 4^(1-2s) (0.5 = today's exactly, 0 sharper, 1 softer) and its peak by
  that factor^0.35, so a softer spot is wider and a little dimmer.
- Glossy: today's light. Matte: wider, flatter diffuse and a faint broad sheen, no sharp spot. Metal: high contrast,
  a reflected horizon band and a highlight tinted by the colour under it. Glass: darker middle, a bright rim (fresnel),
  a sharp white highlight and a faint second glint opposite it.
- At Glossy and softness 0.5 the maths is today's, so the classic ball's picture tests hold.

## 7. The Blend layer blends layers above it

**Found.** The Blend layer sets the whole thing's blend (`ThingDraw::blend`), so every layer mixes with the scene.

**Decision.** The Blend layer blends the layers **under** it with what is behind the ball; layers above it paint
normally on top. A Blend layer at the top of the list behaves as today.

- Kit: `Stack.blend_at` (how many drawn layers sit under the Blend layer); `Stack::split()` gives the record index
  where the layers above start (body, plus each writing layer under it with its masks), `None` when nothing writing
  sits above, or the blend is Normal at full amount.
- `ThingDraw` gains `draw_over(&self, cx, frame)` (default nothing). `build_frame` calls it after the routed draw, into
  the thing's normal target. `BallsDraw::draw` draws the lower part (instance `range.z = split`) and `draw_over` the
  upper (`range.z = split | 0x10000`), both through one shared `draw_part`. Hit flashes go with the lower part only.
- Walk (`layers_walk.wgsl`, new argument `split`): the lower part skips records at or above `split` (except the group
  records 8 and 9, which still change it). The upper part walks everything as now and also keeps `top_in`/`top_out`:
  each layer at or above `split` is composited over them with its blend worked out against the full stack under it
  (`top_over`), glows add their light, the group record 8 recolours them too; it returns the top. In-ball shaders'
  `sample` walks pass split 0.
- Odd ones out keep working (unworn layers keep their records, so indices hold).

## 8. Adding or editing a shader lags for seconds

**Found.** Timed on the software GPU: building one in-ball program takes about 1.3 s, on the UI thread, once per
renderer. The editor has three (preview, ball view, group view), and the ball and group views make a new renderer
whenever their size changes, which builds again.

**Fix.**
- Programs build on a background thread (`wgpu::Device` is `Clone + Send`, error scopes are per thread). Until one is
  ready its balls draw with the plain pipeline, shader layers left out, exactly as a program that failed. Export and
  tests build synchronously as now (`Renderer::set_background_programs(bool)`, default off; the editor turns it on).
- One program store for every renderer of the editor: `ProgramStore` (`Arc<Mutex<Programs>>`), passed with
  `Renderer::share_programs`. Resizing a view keeps its programs.
- `Renderer::programs_pending()`; the editor repaints every 50 ms while one is building, so the effect appears without
  moving the mouse.
- Test: a background renderer's first frame has the program pending and draws the plain ball; after waiting it draws
  the effect. Existing program picture tests stay synchronous.

## 9. Group shader: how much of the group it covers

**Decision.** Group shaders gain **Covers** (0 to 1, default 1, keyframe and pulse), **From** (an angle, default 0:
from the left) and **Edge** (softness 0 to 1, default 0.2). Covers 1 is the whole group, 0.5 half of it from the
From side, with a soft edge Edge wide; animating Covers wipes the effect across the group. Worked per pixel in the
group's square (the shader's group uv), so a ball on the edge can be half covered. It scales the group shader's
coverage everywhere it mixes: the whole ball, the ring around it, and the layers it changes. Record: `p1.y` covers,
`p1.z` from (radians), `p1.w` edge.

## 10. Body mixing

**Decision.** The Body gains a **Mixing** section with **Opacity** (0 to 1, keyframe, pulse): a see-through body, so
outlines and glows can stand alone. There is nothing under the Body inside a ball, so it has no Blend of its own:
mixing it with the scene is the Blend layer right above it, which now (item 7) blends only the layers under it. The
kit test that says Body has no opacity is updated (Body has opacity, not blend).

## 11. Ball / Group switch inside the Look section

The switch leaves the thing's header (inspector.rs row 3) and becomes the first row inside the Look card, above the
ball or group view. Without a Look card the Group mode is unreachable as now; the switch shows only on a
`look.layers` card. Tests that click "Group" keep working (same label); one checks the header no longer has the
"Edit" row.

## Not doing

- No new layer kinds or materials beyond the four named. No change to the Blend layer's settings.
- No changes to physics, goldens, or the save format number (old files needn't load; `colour` keys of Outline, Glow
  and Shading now hold gradients, and a saved plain colour is dropped to the default by `load`'s cleaning).
- No screenshots or videos.
