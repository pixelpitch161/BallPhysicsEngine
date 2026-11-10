# Double-click a slider to reset it (2026-11-10)

Aedan: "Bring back the double-click to reset shortcut."

## What happened (root cause)

- Since PR #3 (2026-10-28) a double-click (or Alt+click) on a setting's name **or its slider** put the setting back
  to its default.
- W5 (PR #168, commit `db6dbde`, 2026-11-08) took the widget's double-click away, keeping only the name's, because a
  double-click on a **number box** (egui `DragValue`) starts typing into it, which made accidental resets
  (`widgets/setting.rs`: "Only the label resets"). Sliders lost it too, though nothing on a slider's rail uses a
  double-click.
- Grade mode's notes (`docs/notes/grading.md`, "double-click resets") and its test
  `a_double_click_puts_a_setting_back_and_the_switch_turns_a_panel_off` assumed the slider still reset. The test
  passed only because it double-clicks Temp's rail at its middle, where the click alone already lands on 0.

## Design decisions

1. A double-click on a **slider's rail** (anywhere on the bar, left of its number box) resets the setting to its
   default, in every settings grid: inspector cards, the Look editor, Scene settings, Grade mode panels. Both slider
   kinds: `number_slider` (egui `Slider`, a hard range) and `span_slider` (a usual span that can go past).
2. A double-click on the **number box** keeps starting typing (W5's reason stays true; `a_typed_length_stays_exact`
   keeps passing). Plain number boxes with no slider (unbounded numbers, x / y places) keep typing too.
3. The name's double-click and Alt+click, and the menu's Reset to default, stay as they are.
4. Nothing on a rail resets while Alt is held (Alt on a rail drags freely past the grid).
5. Other sliders that aren't grid rows but have a clear starting value get the same rail double-click:
   - the Lights strip's Strength fader (`lights_panel/strip.rs`) → the Strength setting's default, through
     `set_or_key` (keys at the playhead when keyed), in one undo step with the click before it (no `own_step`, so the history merges them);
   - Grade mode's Brush sliders (Size, Feather, Flow, Density) → that brush's starting value
     (`Brushes::default().settings[tool]`);
   - the Look editor's brush sliders (size, softness, strength) → `Brush::default()`;
   - a gradient gap's Middle slider (`gradient_tabs.rs`) → 50 %.
6. Not changed: the two-ended Vary slider (a double-click there would have to pick an end; its name still resets),
   switches, dropdowns, colour swatches, gradient bars (their double-click adds a stop), knobs (they already reset on
   a double-click), the export window's quality slider and the listen volume (not settings).

## Code

- `widgets/setting.rs`:
  - `pub fn slider_rail(ui: &Ui, r: &Response) -> egui::Rect`: an egui `Slider`'s rail from its response (its left
    `ui.spacing().slider_width`, the number box to the right). `number_slider` uses it where it now builds the rect.
  - `pub fn rail_double_clicked(response: &Response, rail: egui::Rect) -> bool`: `reset_clicked(response, false)` and
    the pointer is inside `rail`.
  - In `setting()`, after the name's `resettable`: unless `own_clicks`, read the rail rect kept at
    `response.id.with("rail")` (both slider kinds keep it already, for Follow bands); when there is one and
    `rail_double_clicked`, put `def.default` back (`changed = true` only when it differs). Rows without a slider keep
    no rail rect, so nothing else resets.
  - Comment updated: the rail and the name reset; the number box types.
- `widgets/mod.rs`: re-export `slider_rail` / `rail_double_clicked` for the callers outside the module.
- `widgets/menu.rs`: Reset to default's hover "(also: double-click its name or its slider)".
- `lights_panel/strip.rs::fader`, `grade_mode/brush.rs::bar`, `look_editor/brush.rs::bar`,
  `gradient_tabs.rs` Middle: the rail double-click as in decision 5.

## Tests (written first, failing before the fix)

- `widgets/tests.rs`:
  - `double_clicking_a_slider_puts_its_default_back`: `balls_grid`, set Size (`radius`) away from its default,
    double-click near its rail's right end (not where the default sits) → `radius` is the default.
  - `double_clicking_a_span_slider_puts_its_default_back`: the same on a span-slider row (a `span` setting).
  - `a_typed_length_stays_exact` (exists) still passes: the number box's double-click types.
- `grade_mode/tests.rs`: the existing Temp test double-clicks near the rail's right end, so the click alone can't
  land on the default.
- `grade_mode` brush and the lights fader: one test each that a rail double-click restores the starting value.

Double-clicks in kittest need steps under egui's 0.3 s (`with_step_dt(1.0 / 60.0)` or a longer
`max_double_click_delay`), and a press + release on a drag-only egui `Slider` shows as `drag_stopped` (which
`reset_clicked` handles).

## Verify

- `cargo test -p bpe-studio -- --test-threads=2` (the new and changed tests, then the crate), `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo fmt --all`. Nothing drawn changes, so no goldens or look check.
- `docs/notes/editor.md` (Setting menu bullet) and `docs/notes/grading.md` say what resets.

## As built

As planned, plus from the review: the rail double-click never resets while Alt is held (not only Alt+click), and
the Lights fader's reset doesn't start its own undo step, so one Undo takes back the whole double-click (its test
checks). The number-box test passes partly because egui focuses a `DragValue` on its first click and
`reset_clicked` refuses while text editing has focus; the rail rect is what keeps an unfocused box from resetting.
