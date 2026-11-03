# Editor rebuild with the Ball Studio Design System

Date: 2026-11-03. Approved design (Claude Doc): https://claude.ai/artifact/2p8ChnYZFMgYkeXAj1mWJ5.
Design System: https://claude.ai/artifact/SPfnA1YGCJr8SHFstvjxji (copy in project files
`ballphysics/design-system/system/`: `README.md`, `tokens.json`, `components/*`). Reference mockup:
`ballphysics/design-system/look-directions/GreyClips.dc.html` (C2 "Grey with clip lanes", tree rows from B).

## Goal

The editor (`bpe-studio`) looks and works like the Design System: neutral greys, IBM Plex Sans/Mono, 2px
corners, inverted on/primary buttons, and one track colour per thing that follows it through the tree, the
preview selection, the timeline and the inspector. Physics, export and the video are untouched; goldens must not
move.

## Owner's decisions

1. Full Design System in one go (not split into parts).
2. Timeline: every thing with keyframes or pulses gets a group; only the selected thing's group is open. Every
   other group is one thin coloured bar showing its keyframe diamonds.
3. A setting with keyframes or a pulse shows its label in its thing's track colour (instead of the old violet).

## 1. The look layer: `crates/bpe-studio/src/style/`

Replaces `theme.rs` (deleted). It is the only place in the editor that names a colour, font or size.

- `style/tokens.rs`: one `pub const` per Design System token, named like the token: `GROUND #1e1e1e`,
  `PANEL #282828`, `STAGE #2b2b2b`, `RAISED #333333`, `SELECTED #383838`, `HOVER #3a3a3a`, `LINE_SOFT #383838`,
  `LINE #444444`, `FAINT #777777`, `MUTED #a0a0a0`, `TEXT #e2e2e2`, `TEXT_STRONG #ffffff`, `ON_LIGHT #1e1e1e`,
  slider fill `SLIDER_FILL #bdbdbd`; sizes `CONTROL_H 26`, `VALUE_H 22`, `TREE_ROW_H 30`, `LANE_H 23`,
  `GROUP_H 22`, `TOOLBAR_H 40`, `STATUS_H 24`, `TREE_W 230`, `INSPECTOR_W 370`, `LABEL_W 108`, `VALUE_W 62`,
  `LANE_NAME_W 120`, `RADIUS 2`, `RADIUS_ROW 8`. Old `ACCENT`, `ACCENT_TEXT`, `WARNING`, `ERROR`, `CARD`,
  `BACKGROUND` are gone: warnings and errors use `TEXT` with words; nothing in the chrome is coloured except
  track colours.
- `style/track.rs`: `pub fn track_color(c: TrackColour) -> Color32` (the 8 hexes from tokens.json), `pub fn
  clip_fill(c) -> Color32` (track colour mixed 22% over `PANEL`, in sRGB bytes:
  `panel + (track - panel) * 0.22` per channel, rounded).
- `style/fonts.rs`: IBM Plex Sans Regular / Medium / SemiBold and IBM Plex Mono Regular TTFs, `include_bytes!`
  from `crates/bpe-studio/assets/fonts/` (with `OFL.txt`). Proportional = Plex Sans Regular (egui's default fonts
  stay as fallbacks after it, for glyphs Plex lacks); Monospace = Plex Mono; a named family `"semibold"` =
  Plex Sans SemiBold, `"medium"` = Plex Sans Medium. `style::semibold(size) -> FontId`.
- `style/mod.rs::apply(ctx)`: fonts, then text styles Small 12, Body 13, Button 13, Heading 18 (semibold
  family), Monospace 12; visuals per the README's "Building it in egui" section:
  `panel_fill = window_fill = PANEL`, `extreme_bg_color = GROUND` (text field and value wells),
  `faint_bg_color = RAISED`, `window_stroke 1 LINE`, `window_corner_radius = menu_corner_radius = 2`,
  `popup_shadow`/`window_shadow` = offset (0,8) blur 20 black 0xaa, `selection.bg_fill = SELECTED` and
  `selection.stroke 1 TEXT` (a deliberate change from the README's `text` fill: egui uses selection for text
  selection and selectable labels, and a light fill there would hide light text), `slider_trailing_fill = true`
  with the trailing fill colour taken from selection, so sliders get their own drawing (below),
  `handle_shape = HandleShape::Rect { aspect_ratio: 0.2 }`, `slider_rail_height = 2`. Widget states: all corner
  radius 2; `noninteractive` stroke `LINE_SOFT`, fg `TEXT`; `inactive` weak_bg_fill transparent, bg_fill `LINE`
  (slider rails), bg_stroke 1 `LINE`, fg `TEXT`; `hovered` fill `HOVER`, stroke 1 `LINE`, fg `TEXT_STRONG`;
  `active`/`open` fill `TEXT`, fg `ON_LIGHT`. Spacing: item 8 x 6, button_padding 10 x 5, interact_size.y 26,
  menu_margin 2.
- `style/controls.rs`, the drawn pieces every panel uses:
  - `primary_button(text) -> Button` and `toggle_button(text, on) -> Button`: inverted (`TEXT` fill,
    `ON_LIGHT` semibold/regular label) when primary or on, else plain.
  - `toolbar_button(ui, text) -> Response`: a frameless button that fills `HOVER` on hover.
  - `switch(ui, &mut bool) -> Response`: a 30 x 16 square outline with a square 10px knob, `TEXT` when on
    (knob right), `FAINT` when off (knob left). Its `widget_info` is a `Role::CheckBox` with the selected state,
    so `labelled_by` and kittest `click()` keep working. Replaces every `ui.checkbox` in the editor.
  - `section_header(ui, open, title, summary) -> Response`: 32px row, chevron (`MUTED`), title in semibold 13,
    and when closed the summary in `MUTED` small. Named "Hide {title}" / "Show {title}" like the old
    `fold_toggle` so tests keep their labels.
  - `changed_dot(painter, pos)`: a 2.5px-radius ring in `TEXT`, 1px.
  - `paint_diamond(painter, center, size, fill)`: a key diamond in `TEXT_STRONG` with a 1px `GROUND` outline.
  - `divider(ui)`: a 1px `LINE_SOFT` full-width line.
- Sliders keep egui's `Slider` (tests drag them; no reimplementation). Its look comes from the visuals above:
  2px `LINE` rail, `SLIDER_FILL` trailing fill (set via `selection.bg_fill` only inside `number_slider`'s
  scope: `ui.visuals_mut().selection.bg_fill = SLIDER_FILL`), a thin rect handle in `TEXT_STRONG`
  (`widgets.*.bg_fill` for the handle is set in the same scope). The value box beside it is egui's DragValue on
  `GROUND` (`extreme_bg_color` is not used by DragValue, so `number_slider`/value fields set
  `widgets.inactive.weak_bg_fill = GROUND` in their scope) with mono 12 text, right-aligned.
- A source test (`style::tests::no_panel_names_its_own_colour`) reads every `.rs` file under
  `crates/bpe-studio/src` outside `style/` and fails on `Color32::from_rgb(`, `Color32::from_gray(`,
  `Color32::WHITE`, `Color32::BLACK` or `from_white_alpha(`. Allowed exceptions are listed by file and line text in
  the test: the colour and gradient swatches (`widgets.rs picker_color`, `gradient_bar.rs` checkerboard and
  stop colours, `palettes.rs` palette swatches), which show real scene colours.

## 2. Track colours

- `bpe-core/src/scene.rs`: `pub enum TrackColour { Pink, Cyan, Lime, Orange, Violet, Yellow, Red, Teal }`
  (serde as lower-case names, `ALL: [TrackColour; 8]` in that order, `name()` "Pink"…). `Thing` gains
  `#[serde(default, skip_serializing_if = "Option::is_none")] pub colour: Option<TrackColour>`; `None` only for
  the root (scene settings).
- `Scene::next_colour(&self) -> TrackColour`: the first of `ALL` no thing uses; when all 8 are used, the one used
  by the fewest things, first in `ALL` order on a tie.
- `ThingKind::track_colour(&self) -> Option<TrackColour>` (default `None`): the kind's own starting colour.
  Arena returns `Orange`, Camera returns `Violet`.
- `Scene::add_thing` sets `colour` to the kind's own colour when the kind has one and no thing uses it yet,
  else `next_colour()`. A duplicate (`tree::duplicate` goes through `add_thing`) so gets the next free colour,
  not the original's. A folder made from a selection is added through `add_thing` so it gets one too.
- `Scene::mend` (run on every load): every non-root thing with `colour == None` gets one, walking `order`:
  kind colour if the kind has one and no thing yet has that colour, else `next_colour()`. The root's colour is
  forced to `None`. `mend` takes no registry today; it gets the kind colour through a new
  `Scene::give_colours(&mut self, reg: &Registry)` called from `save::load` after `mend`, and from
  `Scene::empty`'s callers that add things (they go through `add_thing` anyway).
- Physics, resolve, gfx and export never read `colour`. A save of a scene now writes `colour: Some(pink)` etc.
  per thing; old files load fine (serde default), and `Scene::FORMAT` stays 3.
- Edits: `Edit::Colour(TrackColour)` in the inspector header; `Action::Colour(id, TrackColour)` from the tree
  row's right-click menu. Both go through the normal edit path so Undo covers them.
- UI: `style::track_picker(ui, current) -> Option<TrackColour>`: a row of 8 square 18px swatches; the current
  one gets a 1.5px `TEXT` outline 2px out; each swatch is a button named "Track colour {name}".

## 3. Panels

The layout stays: toolbar, tree left, preview centre, inspector right, timeline and status bar at the bottom.
Panels touch with 1px `LINE_SOFT` dividers; no gaps, shadows or rounded panels.

- **Toolbar** (`app.rs::toolbar`, `bar_frame`): height 40, `PANEL`, margin 8 x 7. Same buttons in the same order
  drawn as `toolbar_button`s; separators 1px `LINE` 18px tall. Grid / Snap toggles use `toggle_button`. Scene
  name field `GROUND` well, 170 wide. Save status small `MUTED`; "Unsaved changes" in `TEXT` with the "•".
  Export video… is `primary_button` (semibold).
- **Tree** (`tree.rs`): panel default width `TREE_W`, padding 8 x 10. Filter field on top (hint "Filter", full
  width), rows below in a scroll area, then at the bottom "+ Add thing" (a plain button, full width) and the hint.
  Row (`row`): 30px tall, `RADIUS_ROW` corners, padding 10 left; children indent 14 per depth. Contents left to
  right: fold triangle (only with children; its 14px place kept otherwise), a 4 x 16 rounded tag in the track
  colour (empty for the root), the kind icon 10px in `MUTED` (the selected row's ball and disc icons take the
  track colour), the ride hook if it rides, the name (body; `TEXT_STRONG` and medium weight when selected,
  `MUTED` when hidden), tags small `MUTED`, problems as a `TEXT` "!" with the problems on hover, and the eye in
  `FAINT` (`MUTED` when selected) at the right. Selected row: `SELECTED` fill + 1.5px `TEXT` inner ring; hovered
  row: `HOVER` fill. The whole row is the click/drag target (name button keeps its label for tests, but is
  frameless). Right-click menu: Rename, Track colour (the swatch row; not for the root), Duplicate, Delete, …
  as today.
- **Preview** (`preview.rs`): the central panel fills `STAGE`; the 9:16 frame has no border. Selection colour =
  the selected thing's track colour (`TEXT` for the root): wall outline, body circle, aim arrow and the folder's
  dashed ring use it; the active drag handle uses `TEXT_STRONG`; unselected marks use `TEXT` at 45% alpha
  (`style::FAINT_MARK`). The World / Video switch becomes a two-segment toggle (2px corners, 1px `LINE` border,
  the on segment inverted) at the top-left of the stage.
- **Inspector** (`inspector.rs`, `widgets.rs`): default width `INSPECTOR_W` (still draggable, saved as today).
  Header: the name field (26px `GROUND` well) and a 26px colour button showing the track colour that opens the
  track picker (not for the root), then the kind in `MUTED` small and the eye. The jump bar stays, as small flat
  buttons (the marked card inverted). Cards become flat sections: `card()` draws no fill, horizontal padding 10,
  a `LINE_SOFT` divider under each; the card header is `section_header` with a summary when folded ("Off" for a
  card that has an on switch that is off; otherwise the count of changed settings, "2 changed", or nothing).
  Setting rows: label column `LABEL_W` (the old proportional `label_width` is replaced by a fixed 108, shrinking
  to 96 below a 330 panel), labels `MUTED`, a changed setting (value != default) gets `changed_dot` drawn just
  left of its label text, a keyed or pulsing setting's label is in the thing's track colour (`GridCx.track:
  Color32`, `TEXT` for the root). Group headings inside a card are semibold `MUTED` small with the changed dot
  in `TEXT`. Choices use the dropdown look (`RAISED`, `LINE` border, 22px). Bools use `switch`. Vec2 fields,
  text fields and number boxes are `GROUND` wells.
- **Timeline** (`timeline/`): background `PANEL`, top divider. Rows:
  1. Transport (40px): a 28px square inverted play/pause button, a frameless restart button, the time
     `0:04.20 / 0:20.00` in mono 14, Loop as a `switch` with its label, and the hint right-aligned in small
     `MUTED`: "Lanes are grouped under the thing they belong to."
  2. Ruler (`scrub_bar`, 18px): a `GROUND` strip from `LANE_NAME_W + 8` with second marks in `LINE`; click or
     drag scrubs exactly as today (same fn name, returns the track rect).
  3. Hits lane (`LANE_H`): name "Hits" in `MUTED` small; one 2px tick per hit at its time, coloured by the track
     colour of `event.a.thing` (`TEXT` if that thing has none). Replaces the old histogram in the scrub bar.
  4. Groups, in `scene.order`, root first: each thing with at least one track or pulse. Group header (`GROUP_H`):
     a 10px swatch in the track colour (`MUTED` square for the root), the thing's name in semibold small, and
     across the track a 3px line in the track colour at 55% alpha. Clicking the header selects the thing
     (which opens its group). Open group (the selection's primary thing): one lane per setting, as today
     (`lanes_for` now takes one thing), the name indented 17px in `MUTED` small, the strip drawn as a clip
     (`clip_fill` + 1px top edge in the full track colour; the root's clip is `RAISED` with a `MUTED` edge),
     keys as `paint_diamond` (selected key: 1.5px `TEXT_STRONG` ring around it; hovered: grown 1px).
     Pulse lanes keep their controls, drawn on the clip. Folded group: no lanes; the header's line becomes a
     clip bar `GROUP_H - 6` tall holding a small diamond per key of every lane of that thing (display only).
  5. The playhead: one 1px `TEXT_STRONG` line from the ruler down through every lane.
  With no lanes at all, the Hits lane still shows and the hint under it reads: "Right-click a setting and pick
  Create keyframe. Its lane shows up here under its thing." Key click / drag / snap / delete / ease menu /
  double-click add / curve window behave exactly as today.
- **Status bar** (`status_line`): 24px, `GROUND` fill, small `MUTED` text, top divider `LINE_SOFT`.
- **Other windows** (export window, gradient bar, curve editor, palettes, confirm dialog, add-thing menu): same
  layout, only colours from tokens, switches instead of checkboxes, the export window's Export button primary.

## As built (differences from the plan above)

- Section headers keep the inspector's existing fold arrow (`widgets::fold_toggle`, now drawn with
  `style::paint_chevron`) and a semibold title; the folded summary is "N changed" (values this thing set).
  There is no separate `section_header`.
- `clip_fill` is `style::track::mix_over_panel(track_color(c))`; `style::clip_colours` gives a clip's fill and
  edge (raised grey and muted for the scene settings).
- The source colour test allows a line that ends in `// scene colour` (swatches and the gradient checkerboard),
  instead of a list of exceptions in the test.
- Sliders: the trailing fill is turned on per slider in `widgets::number_slider`, with `selection.bg_fill` set to
  `SLIDER_FILL` only in that scope. Value fields are wells through `style::wells`; dropdowns in rows through
  `style::raised`.
- Group headings inside a card stay in the label column and wrap (a heading that overflows widens the column).
- The folded-card summary is only "N changed"; there is no "Off" summary. The slider handle aspect is 0.25.
- In a tree row only the name takes clicks and drags (as before); the rest of the row shows hover and selection.
- Timeline group headers are named "{thing} lanes" for screen readers and tests (the tree row is "{thing}").
- The saved inspector width moved to a new file (`inspector-width-v2.txt`) so everyone starts at 370.

## 4. Testing

- Existing editor tests keep passing; tests whose labels or drawing change are updated to the new labels (only
  where this spec changes them).
- New tests: `TrackColour` order and wrap (`next_colour` after 8 picks the least used); Arena Orange and Camera
  Violet; `add_thing` gives colours; a loaded old scene gets colours in tree order and the root gets none; save →
  load keeps a colour; duplicate takes the next free colour; tree right-click colour change + undo; inspector
  header colour pick; timeline: with a ball and the root keyed and the ball selected, the ball's lanes show and
  the root's group is one folded bar (its lane labels absent); clicking the root's header selects it; a hit tick
  colour helper returns the hitter's track colour; a keyed label uses the track colour (unit test on the colour
  chooser `widgets::label_colour(mark, track)`); `switch` toggles on click; the source colour test.
- Goldens unchanged (`cargo test -p bpe-kit golden`, debug and release); `cargo clippy --workspace`,
  `cargo fmt --check`.
- One release-build screenshot under Xvfb compared by eye with the mockup (for the builder only, not sent unless
  asked).

## Not doing

No new settings, kinds or features; no change to what a button does; no light theme; track colours never reach
the video, export or physics; no layout moves beyond the list above.
