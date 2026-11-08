# Gradients (spec `docs/superpowers/specs/2026-11-06-gradients-design.md`, plans `docs/superpowers/plans/gradients/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

Part 1 is in (plan `part1.md`): the gradient value (`bpe-core/src/color.rs`) has a **Mix** (`Mix`: Light, the old
blend in linear light and the default of every settings struct's gradient; Plain, sRGB numbers; Even, OKLab; Round the
wheel / Long way round, OKLCH hue the short or long way, chroma lowered into sRGB by `oklab::fit_gamut`), each stop a
**gap blend** for the gap after it (`Stop.blend`: `GapBlend` Smooth / Eased / Ease in / Ease out / Steps, and
`Stop.middle`, where the halfway colour or the Steps edge sits, a power bend), and **Loop** (`Gradient.looped`: `t` wraps
with `fract` and the last stop's gap runs back round to the first; `set_looped` respaces a gradient whose last stop sits
on 1). The old `transition` field is read into gap blends (`SavedTransition`, read only). New gradients made in the
editor (the "+", palettes, Add matching, Random) mix Even; the Rainbow palette loops Round the wheel. **Companions**
(`gradient_extras.rs`): every gradient setting `k` (a layer's keeps its prefix) has `"<k>~bands"` (`banded(n)`, 2n
stops, at most `MOST_BANDS`) and `"<k>~grain"` (`Gradient.grain`, `#[serde(skip)]`, carried to the GPU only) beside it,
plain numbers that key, pulse and follow sound; `def_at`/`layer_def` answer them, they can't vary across balls, and
`apply_moves` puts them onto the gradients right after `Animation::apply` (`build_frame_sensing`, the light probe).
The GPU atlas rows are 1024 wide (`ATLAS_WIDTH`, the grade map stays 256), `PaintGpu.grain` dithers in `paint_along`
(`paint_px`, set at the top of every fragment entry that paints). `Follow::OrderThereAndBack` and the group gradient's
`Spread::ThereAndBack`; Order on a looped gradient spreads `i / n` (`eval::order_place`) so a ring has no seam. The
editor (`gradient_bar.rs`) has a diamond per gap (drag: the middle), a selected stop, and the tabs under the bar
(`gradient_tabs.rs`: Colours (stop colour, hex, at, gap blend, middle) | Blend (Mix, Loop, Bands, Grain, Every gap),
Shape and Move once parts 2 and 3 add companions there); companion rows are ordinary grid rows under their full key
(`companion_def`, `widgets::def_in`), and their timeline lanes read "Colours bands".

Part 2 is in (plan `part2.md`, "As built" at its end): **gradient shapes**. One form list for every area gradient
(`bpe-core/src/gradient_form.rs`: `GradientForm` Straight, Round, Sweep, Stripes, Rings, Along the edge, Diamond,
Polygon, Spiral, Kaleido, Off-centre, Waves, Marble, Free points, Ripples, Turbulence; GPU codes 1..6 and 13..22,
`along` the CPU twin of `paint.wgsl`'s `paint_form`, `area_colour` what the editor's previews show, `free_points`/
`free_colour`, `points_after` keeps a point per stop). A gradient setting's `Flags.area` (`Area::No`, `Fills(form)`:
placed by its own Turn/Centre/Size/Beyond (`~turn ~centre ~size ~repeat`), `Placed(form)`: placed by its paint's
Angle/Size/Centre/Span) offers the Shape companions `~form` and each form's numbers (`~sides ~star ~arms ~twist
~mirrors ~focus ~stretch ~stretch_turn ~count ~height ~flow ~detail ~bend ~seed ~softness ~points ~spacing ~from
~speed`, `when` written relative, `when_of`); `apply_moves` puts them into `Gradient.layout` (`Layout`, never
saved) and the kit's `paint::form_gpu(g, default, Placing, atlas)` turns them into `PaintGpu.pattern`/`form` (80 bytes).
The old per-site gradient choices are gone (the Paint's Gradient straight/round, the Pattern's Stripes/Rings/Linear,
now Pattern Gradient + `in_colours~form`, Stripes by default; a shade-mode Pattern has its own Shape row).
**Ripples** read a field of the frame's balls and shapes (`bpe-gfx/src/ripples.rs`, `shaders/ripples.wgsl`,
`ripple_common.wgsl`): built only when `Frame.ripples` (`gradient_extras::any_ripples`), half the video size, two
nearest sources of different owners among every / balls / shapes by a jump flood; things push `RippleSource`s
(`push_ripple_ball`, `push_ripple_outline`), and paint skips its own owner (`paint_world`, `paint_own`). The
editor's Shape tab (`gradient_tabs.rs::shape_chips`, CPU preview chips in `gradient_form_chips.rs`) then the form's
rows; Free points show as dots in the stop colours on the ball view (`look_editor/free_points.rs`, `handles.rs`) and
in the preview for a selected shape's layers and the background (`preview/free_points.rs`, `Handle::FreePoint`),
a drag writing the whole `~points` list. Heavy paint maths has one call site per program (llvmpipe inlines it all).

Part 3 is in (plan `part3.md`): **movement**, the Move tab's companions (`gradient_extras::moves()`, each with its
fold in `SettingDef.group`): Cycle (`~cycle`, `~cycle_shift`), Step on the beat (`~step_every` beats, `~step_glide`),
Breathe (`~breathe`, `~breathe_speed`, `~breathe_every`), Hue spin (`~hue_speed`, `~hue_turn`), Stops wander
(`~wander`, `~wander_speed`, `~wander_seed`), Hit wave (`~wave`, `~wave_width`, `~wave_speed`, `~wave_colour` a
Color), Morph (`~morph`, `~morph_to` a Gradient companion, which has no companions of its own: `split` refuses a base
that is a companion) and Shape spin (`~spin`, added onto part 2's `~turn`). `apply_moves(resolved, time)` reads the
beat from the root's tempo (`gradient_extras::Beat::of`, the same as `beat_at`), moves the stops for Wander
(`Gradient::wandered`, never past a neighbour) and puts the rest on the draw-time `Gradient.moves` (`Moves`: shift,
breathe, hue, morph, wave; `#[serde(skip)]`), which `Gradient::sample` honours (slide, an unlooped one wrapping with a
cut; spread round the middle; `mix_linear` toward Morph's; `hue_turned` in OKLCH), so bakes, bands, CPU colours,
lights and the probe all see it; `sample_still` is the plain sampling; a one-colour gradient takes its moved colour
into its stop; `step_stops` gives moved colours; bands bake the moves in. **Hit wave** (`HitWave`: `at`, `apply`,
`lasts`) travels with the gradient's atlas row: `GradientAtlas::waves` (two texels a row, `wave_texels`, `NO_WAVE`)
uploaded to the paint bind group's binding 2 (`Rgba32Float`), read by `paint.wgsl::paint_wave` with
`var<private> paint_hit` (set beside `paint_px` in `shade_layers` and `shade_path` from the instance's hit) in
`paint_along` (sample → vary → wave → grain); the atlas hash eats moves and waves only when there are any. Stacks
(`Stack`/`ShapeStack.wave_memory`, `gradient_extras::wave_memory`) ask for hits while a gradient has a wave; on the
CPU `paint::Inputs.hit_age` (filled by `BallInputs`/`WallInputs`) runs it in `colour_at`. The background, trails,
particles and lights have no hit, so no wave. The Move tab folds each move (`widgets::group_heading`, open state under
the bar's id); one-colour gradients still show no tabs.

Part 4 is in (plan `part4.md`, editor only): the **big editor** (`gradient_editor.rs`), a window opened by the
"Editor" button beside a main gradient row's bar (`gradient_bar_named(.., big)`; drawn by `gradient_tabs::tab_rows`,
so it lives while its row is drawn): a wide bar (`gradient_bar::bar_in`: its widgets under `bar.with("big")`, the
selected stop under the row's `bar`, so both select the same stop), Mix chips and Loop, the tools, a table of stops
(row, colour, hex, at, gap blend menu, gap middle, Remove) with Add colour (`gradient_tools::add_colour`), and
painted previews on a ball and a ring of 12 by Order; controls are named "{label} editor …". The **tools**
(`gradient_tools.rs`, a "Tools" row ending the Colours tab and in the window): Flip (`flipped`: `1 − at`, wrapped
when looped, each new gap the old gap before it mirrored, Ease in ↔ Ease out, middle `1 − m`; exact for middles at
0.5, an involution; its order vector permutes any `ValueType::Points` companion through `points_after_flip`),
Spread evenly, Random (`recoloured`: new `palette_gen` colours, places/gaps/alpha kept) and **Pick from picture**
(`eyedropper.rs`: `start`/`stop`/`target`/`deliver`/`take`, state in egui memory; `preview::pick_colour` turns the
preview into an eyedropper, a click reads `Renderer::read_output` and delivers `pixel_at`, Esc or right-click stops,
nothing else in the preview reacts; `tab_rows` puts the colour on the selected stop keeping its alpha; a colour
untaken after `STALE_PASSES` is dropped). A main gradient row's menu has **Copy gradient with its settings**
(`widgets::copy_with`, `Copied.extras` by companion name); Paste Value then also writes each carried companion the
target offers (`companions_pasted`).

**Seams (workstream W3, 2026-11-08).** Sweep, Spiral and Along the edge away from a shape wrap round where the last
colour meets the first: `paint.wgsl::paint_seam` works out how much of the pixel lies across the wrap (the pixel's
width in turns from `fwidth(q)` and the radius, as `paint_mask` does, since the wrapped value's `fwidth` jumps there)
and `paint_colour` mixes the far side's colour in through a two-pass loop (one call site of the paint maths). A Loop
gradient is unchanged (its ends match). The CPU twin `gradient_form::along` keeps the hard seam (editor chips are
small). Test `gradient_form_pictures::the_sweep_and_spiral_seams_are_smoothed_like_an_edge`.
