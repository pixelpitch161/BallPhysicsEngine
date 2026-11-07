# Snap inspector settings to the grid: plan

Asked by Aedan on 2026-11-06: "Have an option for settings that involve distance to snap to the grid while
adjusting in the inspector (size, thickness, position and rotation, etc.)". Mock-ups of four places for the switch:
https://claude.ai/artifact/1yQ43QWY1r15MFdLHuamew. Option picked: **A, the toolbar's Snap button does both**.

Single part, editor only (`bpe-studio`). Nothing in a scene file, the physics, the renderer or export changes, so
no golden moves.

## What the user sees

- The toolbar's **Snap** button (`Grid.snap`, already there) now also snaps distance and angle settings while they
  are dragged in the inspector. Its hover text says so.
- A new **angle step** box sits after the grid size box in the toolbar: `15°` to start, 1° to 90°, whole degrees,
  remembered with the grid in `grid-next.txt` (`angle=15`). Hover: "The angle step that angle settings snap to".
- **Holding Alt** while dragging turns snapping off for that moment, exactly like the preview's drags.
- What snaps (the shown unit decides, see `Measure`):
  - **Length**: a number setting whose unit is exactly `px` (Size, Thickness, Width, Height, Reach, Smallest size,
    Letter height, Area…). At least one grid square away from 0 it lands on whole grid squares (60 px grid: 60,
    120, 180…). Under one square it lands on eighths of a square (7.5, 15, 22.5 … 52.5, 60) so small settings stay
    usable. 0 is a step (it is a multiple of an eighth); the hard `range` then clamps (Thickness can't go under 0.5).
  - **Place**: a pair (`Vec2`) in `px` and the Placement card's Position and Offset. Each of x and y lands on whole
    grid squares, 0 included, the same as dragging the thing in the preview (`grid::snap_point`).
  - **Angle**: a number in `°` or `deg`, and the Placement card's Rotation: whole angle steps.
  - Never: `px/s`, `px/s²`, `deg/s`, `°/s`, `%`, seconds and every other unit; colours, choices, text.
- **Only dragging and arrow keys snap.** Typing a number into a box is always exact. A value that is off the grid
  stays as it is until it is dragged.
- A snapped slider shows small ticks under its rail where the steps are (only when they are at least 4 screen
  points apart, and never on a log-scale slider). The ticks go away while Alt is held.
- Keyframes, pulses, follow sound and vary work as before: a snapped value is just the value written.

## Design decisions

1. **How the widgets know.** `widgets::widget` and friends are called from dozens of places (inspector, look
   editor, rules inspector, gradient tabs, emitter card). Threading a new argument through all of them is churn
   for no gain, so the app puts the frame's snap into egui's context memory once a frame
   (`setting_snap::put(ctx, ..)`), and the number widgets read it (`setting_snap::now(ui)`). It is temp data under
   one fixed `Id`. Without a `put` (unit tests, kittest harnesses that don't run the app) `now` is `None`: nothing
   snaps, so every existing widget test is unchanged.
2. **Alt is read when used**, in `now(ui)`: `ui.input(|i| i.modifiers.alt)` gives `None`. This is the preview's rule
   too (`Grid::step(held_off)`).
3. **The step comes with the feel.** `NumberFeel` gains `snap: Option<Snapper>`. `number_feel` sets it to `None`.
   `widgets::widget` builds `setting_feel(def).with_snap(Snapper::for_unit(ui, unit))`. `span_slider` (used for
   spanned settings and by the Lights panel's faders) snaps through `feel.snap` in its drag, click and arrow-key
   paths, never in the typed path. The Lights panel builds its feel with `number_feel`, so its faders (Strength:
   no length unit) don't snap.
4. **Post-snapping egui widgets.** egui's `Slider` (settings with a hard range but no span) and `DragValue` (no
   range, pairs, Placement) keep their own unrounded drag value in memory (`drag_value.rs` stores the precise value;
   `Slider` re-reads the pointer each frame), so the value can be snapped after the widget returns without sticking.
   `setting_snap::after_drag(ui, &response, value, snapper) -> bool` snaps `value` when the response
   `dragged()` or `clicked()` or has focus with an arrow key pressed this frame, and returns whether it changed it.
5. **Rounding rules** (pure functions in `setting_snap.rs`, unit tested):
   - `length(v, s)`: `if v.abs() < s { (v / (s/8)).round() * (s/8) } else { (v / s).round() * s }`.
   - `place(v, s)`: `(v / s).round() * s`.
   - `angle(v, a)`: `(v / a).round() * a`.
   - Integer settings round the result (`.round()`), then the hard range clamps (`range` order-free, as now).
6. **Which unit.** `Measure::of(unit: &str) -> Option<Measure>` on the unit **as shown** (`unit_for`'s result, so a
   setting whose unit changes with another setting is read as it shows): `"px"` → `Length` for numbers / `Place` for
   `Vec2`; `"°" | "deg"` → `Angle`; else `None`. Trimmed, exact match (so `px/s` never matches).
7. **Vary's two-ended slider** (`vary::two_ended`) snaps its rail drags through `feel.snap` (the feel it is handed
   from `setting`, which now carries the snap) and its two boxes through `after_drag`.
8. **Placement card** (`inspector::position_rows`): Position and Offset x/y through `after_drag` with `Place`;
   Rotation through `after_drag` with `Angle`. They are not `SettingDef`s, so they take the snapper directly
   (`Snapper::place(ui)`, `Snapper::angle(ui)`).
9. **Ticks.** `span_slider` draws ticks on its own rail (`left..=right`), mapping each step `lo + k·step` in the
   span. `number_slider` draws them over egui's rail using egui's geometry: rail from `rect.left() + r` to
   `rect.left() + slider_width - r`, `r = rect.height() / 2.5` (times the aspect ratio when the style's
   `handle_shape` is `Rect`), and only when the slider isn't logarithmic. At most 200 ticks; none when closer than
   4 points. Colour `style::LINE` brighter variant: use the existing `style::FAINT` (no new colour is made up).
10. **The toolbar.** `grid_controls` adds the angle box after the spacing box: `DragValue` over `grid::ANGLES`
    (1.0..=90.0), speed 0.2, 0 decimals, suffix `°`, labelled by the Snap button. The toolbar must still fit at
    1440 px wide: check once by running the release editor under Xvfb and looking at the toolbar (for myself, not
    sent to Aedan). If it doesn't fit, shrink the scene name box further.
11. **Grid file.** `Grid { show, snap, spacing, angle }`, default angle 15. `load_from` reads `angle=` (clamped to
    `ANGLES`); a file without it gives 15. `save_to` writes the four lines.
12. **What is not touched:** preview drags (already snap), the gradient bar's stop positions (0..1, no unit), the
    Sound tab's knobs, the Mixer, LFO curves, shape handles (already snap), keyframe values in the timeline.

## Files

- `crates/bpe-studio/src/setting_snap.rs` (new): `SettingSnap { length: f64, angle: f64 }`, `put(ctx, Option)`,
  `now(ui) -> Option<SettingSnap>`; `Measure { Length, Place, Angle }` with `of(unit, pair: bool)`; `Snapper {
  measure, step }` with `for_unit(ui, unit, pair)`, `place(ui)`, `angle(ui)`, `snap(v) -> f64`, `ticks(lo, hi) ->
  Option<f64>` (the step to draw ticks at, `None` past 200); `after_drag(ui, &Response, &mut f64, Option<Snapper>,
  range) -> bool`; the pure `length`, `place`, `angle`. Unit tests.
- `crates/bpe-studio/src/main.rs` / `lib.rs`: `mod setting_snap;`.
- `crates/bpe-studio/src/grid.rs`: `angle` field, `ANGLES`, load/save, tests.
- `crates/bpe-studio/src/app.rs`: `setting_snap::put` at the top of `ui()`; `grid_controls` angle box and new
  Snap hover text.
- `crates/bpe-studio/src/widgets.rs`: `NumberFeel.snap` + `with_snap`; `widget` builds the snapped feel and
  post-snaps `number_slider`, plain `DragValue` and pair boxes; `span_slider` snaps drags/clicks/keys and draws
  ticks; `number_slider` draws ticks; `setting` hands the snapped feel to `two_ended`. Tests.
- `crates/bpe-studio/src/vary.rs`: `two_ended` rail and boxes snap.
- `crates/bpe-studio/src/inspector.rs`: `position_rows` snaps.
- `CLAUDE.md`: one paragraph under the bpe-studio map.

## Tests (what passing looks like)

1. `setting_snap` unit tests: `length(40, 60) == 37.5`, `length(89, 60) == 60`, `length(91, 60) == 120`,
   `length(-100, 60) == -120`, `length(4, 60) == 7.5`, `length(1, 60) == 0`; `place(29, 60) == 0`,
   `place(-31, 60) == -60`; `angle(44, 15) == 45`, `angle(-8, 15) == -15`; `Measure::of`: `px` Length, `px` pair
   Place, `°`/`deg` Angle, `px/s`, `deg/s`, `%`, `` None.
2. `now` is `None` without a `put`, `None` with Alt held, `Some` otherwise (a kittest harness that `put`s).
3. Grid: round trip keeps `angle`; a file without `angle` loads 15; `angle=500` clamps to 90.
4. Widget tests (kittest, `build_ui_state`, `put` in the harness's ui closure):
   - span slider over 0..400 px with snap 60: a rail drag lands on a multiple of 60 (or an eighth under 60).
   - the same drag with Alt held lands off the grid.
   - typing `123` into the box keeps 123.
   - an angle span slider (`°`) lands on multiples of 15.
   - a `px/s` slider doesn't snap.
   - a ranged (no span) `px` setting through `widget` (egui `Slider`) lands on the grid.
   - a `Vec2` px pair dragged lands on whole squares.
5. All existing `bpe-studio` tests pass unchanged (no `put` in their harnesses, so nothing snaps there). An
   inspector-level kittest with a Studio: Snap on, drag a Shape's Size, the value is a multiple of 60.
6. `cargo clippy --workspace`, `cargo fmt`, `cargo test -p bpe-studio` (and the workspace in the background).
   Goldens untouched (`git diff --stat` shows no golden file).

## As built (after the review)

- The number box beside a span slider snaps when dragged too (`after_drag` on the box), like every other box.
- A setting doesn't snap when its slider span couldn't stop at four steps (`Snapper::fits`: a 0.25 to 10 px Aura
  width or a 0 to 20° sun size stays free), nor the scene's Video Width/Height (`setting_snapper`, group "Video").
- Alt+click on a setting's widget no longer resets it (Alt now means "drag freely"); Alt+click on the label and a
  double-click on either still put the default back.
- Up/Down arrows in a focused egui number box are used by egui itself and move by its own speed; Left/Right on a
  slider rail snap. Accepted.

## Things not to do

- Don't snap typed numbers, values drawn but not dragged, or values loaded from a file.
- Don't add a colour to the editor (the style test fails): use `style::FAINT`.
- Don't change `number_feel`'s steps; the snap only acts when a snapper is present.
- Don't put the snap state into the scene file or `Studio`'s undo history.

## Decision

Aedan picked option A on 2026-11-07 00:01Z. (For the record, the others were: B a separate
settings toggle; C a magnet per row; D Ctrl held to snap.)
