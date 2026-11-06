# Gradients part 4: the big editor and the tools

Spec: `docs/superpowers/specs/2026-11-06-gradients-design.md`, section 5 and "Build parts" item 4. Part 1 (#121) is
in: the value (Mix, gap blends, middles, Loop), companions (`bpe_core::gradient_extras`), the bar with diamonds
(`gradient_bar.rs`) and the tabs (`gradient_tabs.rs`). Parts 2 (shapes) and 3 (movement) build at the same time in
other threads and only add companions and tab content, so this part stays in the editor (`bpe-studio`) and touches
nothing they own. Design approved by the owner; no approval stops.

## What this part adds

1. **The big editor**: a window opened from a button on a gradient bar's row, editing the same setting live: a wide
   bar, Mix chips and Loop, the tools, a table of stops (number, colour, hex, place, gap blend, middle, remove) with
   "Add colour", and small previews of the gradient on a ball and on a ring of balls. Closing it changes nothing.
2. **The tools**, in the Colours tab and in the big editor: Flip, Spread evenly, Random, Pick from picture. (The
   Palette menu already sits beside the bar, in the row and in the window.)
3. **Pick from picture**: an eyedropper over the preview; a click sets the selected stop's colour from the drawn
   picture (the renderer's output, read back), not the scene colour under it.
4. **Copy gradient with its settings**: a gradient row's right-click menu gains it; Paste Value then pastes the
   gradient and every companion the copy carried that the target offers.

## Decisions

1. **Where the window lives.** It is drawn by `gradient_tabs::tab_rows` (only main gradient rows of a settings grid
   have tabs: not a vary To row, not a companion), before the "fewer than two stops" early return, so it stays up if
   the stops drop to one. Drawing it there gives it the row's `get` and `GridOutcome`, so Flip can move a Points
   companion with the stops (decision 6). Its edits go into the row's `value` like the bar's, so the row reports a
   change through the usual path (undo, keys, everything as today).
2. **The button.** `gradient_bar_named` gains a `big: bool`; `widgets::widget` passes `true` only for the main row of
   a gradient that gets tabs (the same test `grid_row` uses), `false` for a To row. With `big`, the bar's row puts a
   small "Editor" button right of the bar (in the room the Palette menu leaves there), named "{label} editor"; a
   click toggles the window's open flag (temp data `bar.with("big editor")`, session memory). One-stop gradients have
   no button (they have no tabs either).
3. **The window.** `egui::Window` with id `bar.with("big editor window")`, title "{label}: gradient", default width
   560, resizable, not collapsible, its close button clearing the open flag. Its controls are named after
   "{label} editor" so they never clash with the row's ("Colours editor bar", "Colours editor stop 2", "Colours
   editor hex 2" …). Its bar uses widget id `bar.with("big")` but keeps the **selection** under `bar`, so the row
   and the window select the same stop: `gradient_bar` gets an inner `bar_in(ui, widgets, memory, g, name, big)`;
   `gradient_bar_named(ui, id, g, name, big)` is `bar_in(ui, id, id, g, name, big)`.
4. **The table** (an `egui::Grid`): a header row (Colour, Hex, At, Gap after, Middle), then a row per stop: its
   number (a click selects it; the selected number is bright), colour well ("… editor colour N"), hex box ("…
   editor hex N", the tab's `hex_box` made `pub(crate)` with its own id base), place % ("… editor at N"; moving it
   re-sorts and keeps the stop selected), gap blend as a `menu_button` with the five blends ("… editor gap N",
   showing the current blend's words), middle % ("… editor middle N"), and "Remove" ("… editor remove N", off when
   only one stop is left). The gap cells are left empty where there is no gap (the last stop of a gradient that
   doesn't loop, or a gap of no width), as the Colours tab does. **Add colour** ("… editor add colour") puts a stop
   halfway along the selected stop's gap (or the gap before it when it has none), coloured as the gradient is there,
   blending as the gap it splits, and selects it.
5. **The previews**: painted with egui (no GPU): a ball 72 px across filled straight across left to right (vertical
   slices clipped to the circle as a mesh), and a ring of 12 balls coloured by Order the way the kit does it (`i/n`
   when looped, `i/(n-1)` otherwise: the seam fix shows). Named "{label} on a ball" / "{label} on a ring" (Image
   role). Bands and other companions are not shown in them (the bar doesn't show them either).
6. **Flip** (`gradient_tools::flipped(g) -> (Gradient, Vec<usize>)`): the colour at `t` becomes the colour at
   `1 − t` (wrapped when looped). Each stop goes to `1 − at` (`rem_euclid(1)` when looped, so a stop at 0 stays at
   0); the gap after each new stop is the mirror of the old gap before that stop: blend Ease in ↔ Ease out (others
   unchanged), middle `1 − m`. Unlooped, the first old stop (now last) keeps the old last stop's unused gap, so Flip
   twice gives the gradient back exactly. The order vector says which old stop each new stop is; a **Points**
   companion (a value per stop, part 2's Free points) whose length matches the stops is permuted by it in the same
   edit (`points_after_flip`). Any `ValueType::Points` companion qualifies, so no part 2 name is assumed.
7. **Spread evenly**: `Gradient::spread_evenly` (part 1), which honours Loop (`i/n` looped, `i/(n-1)` not).
8. **Random** (`gradient_tools::recoloured(g, colours)`): `palette_gen::palette(fresh seed, n)` into the stops kept:
   places, blends, middles, Mix, Loop and each stop's alpha stay; only the colours change. (The Palette menu's
   "Random palette" still replaces the whole gradient, as before.)
9. **Pick from picture** (`eyedropper.rs`, state in egui temp data under one global id):
   - `start(ctx, target)`, `stop(ctx)`, `target(ctx) -> Option<Id>`, `deliver(ctx, colour)` (ends picking, keeps the
     colour for the target with the pass it came in), `take(ctx, target) -> Option<Color>` (a colour for this target
     from at most `STALE_PASSES` 4 passes ago; an older one is thrown away so a pick nobody took never lands later).
   - The tool button "Pick from picture" is lit while picking for this bar; clicking it again stops.
   - `tab_rows` takes a delivered colour first thing and puts it on the selected stop, keeping that stop's alpha.
   - `preview::show`: while picking, after the picture and its pills are drawn and before the playing early return,
     the pointer over the picture is a crosshair and a pill reads "Click the picture to pick a colour (Esc to stop)";
     a primary click reads the renderer's output back (`Renderer::read_output`, sRGB bytes) and delivers the pixel
     under the pointer (`eyedropper::pixel_at(pixels, size, uv)`); a secondary click or Esc stops. Nothing else in
     the preview reacts to that click (no selection, no drag). The read is once per click; its cost (one frame read
     back) is fine.
10. **Copy gradient with its settings**: `Copied` gains `extras: Vec<(&'static str, Value)>` (companion name, value),
    empty for every other copy. A main gradient row's menu (under Copy Value) gains "Copy gradient with its
    settings", which copies the gradient and every companion offered on it, as each reads now (`get` of its full
    key), with words "N colours with its settings". Paste Value on a main gradient row, when the copy has extras,
    also writes each extra the row offers (through `companion_def(def, c).key`, a static key) into
    `GridOutcome.changes`; extras the row doesn't offer are skipped, and companions the copy lacks are left as they
    are. Plain Copy Value is unchanged (the gradient only). Knob value menus never see extras (they only paste
    `value`).

## Files

- `crates/bpe-studio/src/gradient_tools.rs` (new): `flipped`, `points_after_flip`, `recoloured`, `random_colours`,
  `add_colour` (the stop Add colour makes), and `tools(ui, g, memory, name) -> ToolsOutcome { changed, flip:
  Option<Vec<usize>> }` drawing the four buttons in a wrapped line, named "{name} flip", "{name} spread evenly",
  "{name} random", "{name} pick from picture".
- `crates/bpe-studio/src/gradient_editor.rs` (new): `open_button`, `is_open`, `window(ctx, label, bar, g) ->
  ToolsOutcome`, the table, Add colour, the previews.
- `crates/bpe-studio/src/eyedropper.rs` (new): the state functions and `pixel_at`.
- `gradient_bar.rs`: `bar_in` with separate widget and memory ids, the `big` flag and the Editor button.
- `gradient_tabs.rs`: the colour delivery, the window, a "Tools" row at the end of the Colours tab, Flip's points;
  `BLENDS`, `MIXES`, `chip`, `name_it`, `hex_box`, `mix_rows` shared with the window (`pub(crate)`).
- `widgets.rs`: `widget(.., big)`, the menu item, `Copied.extras`, pasting extras in `setting`/`grid_row`.
- `preview.rs`: the eyedropper over the picture.
- `main.rs`: the new modules.
- `CLAUDE.md`: a "Part 4" paragraph under Gradients.

## Tests

- `gradient_tools` unit tests: Flip of an unlooped gradient (places, colours, Ease in ↔ Ease out, middles), sampling
  the flipped gradient at `t` equals the old one at `1 − t` within 1e-4 for gaps of every blend and off-centre
  middles; looped Flip samples the old at `(−t) mod 1`; Flip twice is the identity (looped and not); the order
  vector; `points_after_flip`; `recoloured` keeps everything but colours (and alpha); Add colour's place and blend.
- `eyedropper` unit tests: `pixel_at` corners and middle, out-of-range uv; `take` only for the target, once, and not
  when stale; `deliver` ends picking.
- Kittests in `gradient_tabs.rs`: the Tools row's Flip, Spread evenly (looped and not) and Random; Pick from picture
  lights while picking, a delivered colour lands on the selected stop (alpha kept), clicking again stops.
- Kittests in `gradient_editor.rs` (through a settings grid, like the tabs' tests): the Editor button opens the
  window and closing it leaves the value and the change list alone; a table row per stop; hex, at, gap menu, middle
  and Remove edit the right stop; Add colour adds and selects; Mix and Loop; a stop selected in the window is the one
  the row's Colours tab shows; the previews are there; no Editor button on a one-colour gradient.
- Kittest in `widgets.rs`: Copy gradient with its settings from one gradient row (with `colours~bands` stored), then
  Paste Value on another gradient row writes its gradient and its `~bands`; plain Copy Value pastes no companion.
- Whole workspace: `cargo test --workspace` green bar the known release-only runtime test; goldens untouched (this
  part changes no engine code); `cargo clippy --workspace` clean; `cargo fmt`; the colour-token test green.

## Things not to do

- Don't touch `bpe-core`, `bpe-kit` or `bpe-gfx` (parts 2 and 3 work there); the Points permutation goes through
  values only.
- Don't build Free points dragging, the Shape or Move content, or anything that moves a gradient.
- Don't make up colours in editor code (tokens only; scene colours carry `// scene colour`).
- Don't let the eyedropper's click select or drag anything in the preview.
- Don't add example scenes; don't make demo videos or screenshots.

## Verification

| Check | Passing looks like |
| --- | --- |
| `cargo test -p bpe-studio` | the new unit tests and kittests green, the old ones too |
| `cargo test --workspace` | green (bar `pushing_an_earlier_step_panics_in_debug` in release) |
| `git diff main -- crates/*/tests/golden*` | empty |
| clippy / fmt | no warnings, no diff |
