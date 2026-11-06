# Rules part 2: the editor

Spec: `docs/superpowers/specs/2026-11-06-rules-design.md` (sections 2, 3.2, 3.3, 5.2, 8, 9). Part 1 (the engine
core, `docs/superpowers/plans/rules/part1.md`) is merged. This part is the editor only: no physics, picture or sound
changes, no golden moves. An "As built" section at the end records anything that moved.

## Goal

The bottom area switches between **Timeline** and **Rules**. The Rules view lists one lane per rule: a header column
(on switch, name, "fired N times", problem mark, busy note) and the rule's blocks laid out in columns worked out from
the links, with lines between them. Blocks are added from a block menu ("+" after a block, "+ When" at the lane's
start), linked by dragging a block's out-dot onto another block, unlinked from a line's menu, and copied, pasted,
duplicated and deleted from their menus. Clicking a block shows its settings in the inspector with the normal setting
rows, a who picker, the Change setting's setting menu and "from the firing" numbers. Blocks that fired in the last
0.15 s of the playhead get a ring. The timeline gets a Rules lane under the Hits lane when the scene has rules.

## Files

| Crate | File | What |
| --- | --- | --- |
| bpe-core | `src/rules/mod.rs` | `RuleBlockKind::values()` (names a When's firings carry) and `firing_keys()` (number settings that may come from the firing), both defaulted to none, so kinds from parts 3 to 6 need nothing |
| bpe-kit | `src/rules/mod.rs` | `block_kind!` gains an optional `values = [..], from = [..]` tail (old calls unchanged) |
| bpe-kit | `src/rules/{when,checks,motion,change}.rs` | the kinds name their values and from-keys |
| bpe-studio | `src/style/tokens.rs` | `PROBLEM`, the only red: a rule's problem mark |
| bpe-studio | `src/settings.rs` | `BottomView` (Timeline / Rules) saved in `bottom-view.txt` |
| bpe-studio | `src/app.rs` | `Studio.bottom`, the bottom panel draws `timeline::show` or the Rules view, the inspector shows a picked block, a thing selected drops the block |
| bpe-studio | `src/transport.rs` | the Timeline / Rules chips at the left of the header row |
| bpe-studio | `src/timeline/mod.rs` | the Rules lane under the Hits lane |
| bpe-studio | `src/widgets.rs` | `Value::Who` gets a widget (`who_picker`), `who_words` names things, `TargetChoices.ball_things`/`relative`, `grid_row_with` (a trailing control in the widget cell) |
| bpe-studio | `src/rules_panel/mod.rs` (new) | `RulesPanel` state, `show`, the toolbar row, the lanes, every edit through `Scene`'s rule edits |
| bpe-studio | `src/rules_panel/layout.rs` (new) | the worked-out layout: columns, rows, links (pure, unit tested) |
| bpe-studio | `src/rules_panel/fired.rs` (new) | which blocks fired when, read from the log once per log length (`Fired`) |
| bpe-studio | `src/rules_panel/face.rs` (new) | a block's face: section, name, two or three fields |
| bpe-studio | `src/rules_panel/menu.rs` (new) | the block menu (When / If / Logic / Do ▸ kinds) |
| bpe-studio | `src/rules_panel/inspect.rs` (new) | the inspector for a block: heading, problem, settings, who picker, setting menu, from-the-firing |
| bpe-studio | `src/rules_panel/setting_menu.rs` (new) | Change setting's setting picker: thing ▸ card ▸ (layer ▸) setting |
| bpe-studio | `src/rules_panel/tests.rs` (new) | kittest tests |

## 1. Core and kit hooks

```rust
pub trait RuleBlockKind {
    ...
    /// The values a When block's firings carry, by name ("strength", "speed"…), for the editor's
    /// "from the firing" menus. None for other sections.
    fn values(&self) -> &'static [&'static str] { &[] }
    /// The number settings that can come from the firing ("<key>~from" is read).
    fn firing_keys(&self) -> &'static [&'static str] { &[] }
}
```

`block_kind!` keeps its six-argument form and gains `$(, values = [$($v:literal),*])? $(, from = [$($f:literal),*])?`.
Values: Hit `strength, speed, size, hits, ball`; Time `time, count`; Beat `time, count, bar, beat`. From-keys:
Compare `amount, to`; Is `number`; Chance `percent`; Change motion `amount, angle`; Change setting `to_number, times`
(exactly the settings `Amount::read` reads). A test checks every kind's from-keys are number settings of it.

## 2. Bottom view

- `settings::BottomView { Timeline (default), Rules }`, `load_bottom_view`/`save_bottom_view` (`_in(dir)` forms
  tested), file `bottom-view.txt` holding `timeline` or `rules`. Loaded in `Studio::new`, saved on switch when
  `persist_settings`.
- `transport::controls` starts with two `style::toggle_button` chips "Timeline" and "Rules" (hover: "Keys and hits
  over time" / "The scene's rules"). Then the transport as before.
- `timeline::show` draws the controls and the scrub bar, then the lanes (Timeline) or `rules_panel::show` (Rules).
  The scrub bar stays in both, so blocks can be watched firing while scrubbing.

## 3. Layout (`layout.rs`, pure)

`lay_out(rule: &Rule) -> LaneLayout { places: Vec<Place { block, column, row }>, columns, rows, links: Vec<Link {
from, to, input }> }`:
- column = the longest path from a block without (present) inputs: relaxed `blocks.len()` times over the inputs, so a
  rule whose links go round in a circle still gets finite columns (capped at the block count).
- row = the block's place among the blocks of its column, in the order they were added (`Rule.blocks` order).
- links: every input that names a block of the rule, `input` its place in the block's inputs (a Logic block's lines
  meet its left edge spread out in stacked order).
- Tests: a chain, a branch (two outputs on rows 0 and 1), two Whens into a Logic (column 1), a cycle stays finite,
  a missing input is left out.

## 4. The Rules view (`mod.rs`)

State `RulesPanel { picked: Option<(RuleId, BlockId)>, renaming: Option<(RuleId, String)>, link_drag: Option<(RuleId,
BlockId)>, rule_drag: Option<RuleId>, clipboard_rule: Option<CopiedRule>, clipboard_block: Option<CopiedBlock>,
link_menu: Option<(RuleId, BlockId, BlockId)>, fired: Fired }` in `Panels.rules`.

`show(ui, studio) -> Outcome { separate: bool, message: Option<String> }`. Every rule edit goes through `Scene`'s
edits (`add_rule` … `set_block_param`); a structural edit (add/remove/move/duplicate/paste/link/unlink/enable) is its
own undo step (`after_separate_edit`), renames and block settings merge like other edits.

- **Top row**: "+ Add rule" (adds "Rule N", N one past the count, then shows "+ When"), "Paste rule" (when a rule is
  copied), and a muted hint ("Rules run top first. Click a block to see its settings."). No rules: a muted sentence
  saying what a rule is.
- **Lane** (`LANE_PAD` 8, block 156 × 62, column gap 40, row gap 10; lane height = rows × (62 + 10) + pad, at least
  one row): a horizontal `ScrollArea` per panel holding every lane, so long lanes scroll sideways.
  - **Header column** (150 px, `RAISED` fill): `style::switch` (named "<rule> on"), the name (a label; double-click
    or menu Rename puts a `TextEdit` in its place, Enter or focus lost sets it), "fired N times" (muted, from `Fired`
    up to the playhead), a `PROBLEM` dot when the rule has a problem (hover: each problem's words; the runtime's
    `RulesState::problem` first, then `Scene::rule_problems`), and "too busy at 3.2 s" when `busy_at` is set.
    Right-click: Rename, Duplicate, Copy rule, Paste rule, Delete. Dragging the header moves the rule: a line marks
    where it would land, release calls `move_rule`.
  - **Blocks** at their layout places. A block is a `Sense::click` rect named "<Section> <kind name>" ("When Hit"),
    `RAISED` fill with a `LINE` border; selected: `SELECTED` fill and a `TEXT` border. Its face (`face.rs`): the
    section word small and muted on top, with a 2 px top edge in `TEXT` for When and `MUTED` for Logic; the kind's
    name in semibold; then up to three fields (see 4.1). Fired within 0.15 s before the playhead: a 1.5 px
    `TEXT_STRONG` ring. Click: picks it (inspector), clears the thing selection. Right-click: Duplicate (a copy with
    the same inputs, so it sits under it as a branch), Copy block, Paste block after, Delete (`remove_block`).
  - **Out-dot** (radius 4, right edge middle, named "Link from <name>"): dragging draws a line to the pointer;
    releasing over a block calls `link(rule, from, to)`; `WrongType`/`RuleLoop` give a status message ("A When block
    can't have anything leading into it", "That link would go round in a circle").
  - **"+"** (16 px, in the gap right of each block, named "Add after <name>"): the block menu; a When picked there is
    added with `add_when` (stacked), anything else with `add_block(after)`.
  - **"+ When"** under the lane's When column (named "Add When to <rule>"): the block menu's When kinds only.
  - **Lines**: cubic curves in `MUTED` from an out-point to an in-point (a Logic block's in-points spread over its
    left edge). A right-click within 6 px of a line opens its menu: Remove link (`unlink`).
- **Block menu** (`menu.rs`): submenus When / If / Logic / Do listing `reg.blocks()` of that section in registry
  order, each with its help on hover. Logic has one kind, so it is a button.

### 4.1 Faces (`face.rs`)

- Fields: the kind's `summary(get)` when it differs from its name (parts 3 to 6 may give one); else the first three
  `simple` settings that apply (their `when`), each "<label> <words>". A from-the-firing number reads "<label> <value
  words> ×t +p". Do Change setting is special: "Who …", the setting's label ("Bounce · Bounciness", or "No setting
  yet"), and "Set 0.9" / "Add 100" / "Multiply ×2".
- Who words with the scene: That ball, The thing it hit, Its group, the thing's name (", ball N" for one ball),
  "tagged <tag>", "any <kind name>", Anything. A thing's field has a dot in its track colour (the only hue in the
  panel).

## 5. Fired (`fired.rs`)

`Fired { made_for: (u64 revision, usize log length), times: BTreeMap<(RuleId, BlockId), Vec<f64>> }` from the log's
`EventKind::Rule` events (`b.thing == ROOT`, `b.item = rule × 65536 + block`, time = step / steps per second), filled
when the revision or the log length changes. `count(rule, ends, time)`: the number of distinct times up to `time` at
which an end block of the rule (one with no outputs) fired; `recent(rule, block, time)`: fired within
`RING_SECONDS` (0.15) before `time`. `ticks(rule, ends)`: the end blocks' times, for the timeline lane.

## 6. Timeline Rules lane

Under the Hits lane, only when `scene.rules` isn't empty: a lane named "Rules" with a 2 px `TEXT` tick per firing (end
blocks' times, one per pixel column and rule). Hovering the lane names the nearest tick within 4 px: "<rule> at
0:03.25".

## 7. Inspector for a block (`inspect.rs`)

`Studio::inspector_panel` shows `rules_panel::inspect::show` instead of the thing inspector when
`selection.primary` is none and `panels.rules.picked` names a block that exists (the app drops `picked` once a thing is
selected, or when the block is gone).

- Heading card: "<Section> · <kind name>" semibold, the kind's help muted, "In <rule name>", and each problem of this
  block (or the rule's runtime problem) in `PROBLEM` words.
- Settings: the kind's settings (and `dynamic_defs`) that apply, through `widgets::grid_row_with` in one
  `settings_like_grid`, `GridCx { marks: none, follow: none, vary: none }`, changes through `set_block_param`.
  - **Who** rows: `widgets::who_picker` (also used for every `Value::Who` row): a menu button with the who's words;
    items That ball, The thing it hit, That ball's group (only when `TargetChoices.relative`, which the inspector sets
    for If and Do blocks), Anything, Thing ▸ (each thing in tree order), Tagged ▸ (each tag), Any ▸ (each kind). A
    thing with balls (`TargetChoices.ball_things`) gets a "ball" number box beside it (0 = the whole thing).
  - **From the firing**: each of the kind's `firing_keys` gets a "from" toggle chip at the end of its widget cell
    (`grid_row_with`'s trailing control, named "<label> from the firing"). On, its row is drawn by the inspector: the
    label and a menu of value names (the values of the When blocks upstream of this block, walked through inputs,
    or every known name when none is known) plus the chip, then "Times" and "Plus" rows (`from_def`'s settings,
    their keys mapped to "<key>~times"/"<key>~plus"). Turning it on writes the first offered name; off writes "".
  - **Compare's Value**: a menu of value names (upstream values; with Of: A thing, speed, size, hits, ball, time).
  - **Change setting**: the Card and Setting rows are one "Setting" row with a menu (`setting_menu.rs`): Thing ▸ (each
    thing that has changeable settings) ▸ card (the thing's own settings under its kind's name, Look, Motion, Bounce,
    Effects, Lighting, only those it has) ▸ (a layered card: layer by name ▸) settings by label, grouped under their
    group headings. Changeable types: number, whole number, pair, switch, colour, gradient, choice. On a thing with
    balls, a setting that can't change for one ball is marked "(whole group)" (vary use not Live, or a Motion/Bounce
    card whose sim isn't `per_ball`). Picking writes `card` and `setting` in one edit. Then only the To row of the
    setting's type shows, as the setting itself shows it (its span, range and unit; a choice as its choices), How only
    for numbers, Times only for Multiply. The setting's description is found on the who's named thing, else the first
    thing in tree order that has it; none: "No setting picked" and no To row.

## 8. Tests (what passing looks like)

- core/kit: `values`/`firing_keys` defaults; every kit kind's from-keys are number settings of it.
- `layout.rs`: the five cases in section 3.
- `fired.rs`: counts and rings from a hand-made log (distinct times, end blocks only, the 0.15 s window).
- `settings.rs`: bottom view round trip; a missing or odd file is Timeline.
- `setting_menu.rs`: a ball's Bounce card lists Bounciness; a Balls group marks a Bounce setting per its sim; the
  scene settings list Gravity under Motion; a Gradient is offered, Text is not.
- kittest (`rules_panel/tests.rs`, a harness drawing the bottom panel and the inspector like the app):
  - the Timeline/Rules chips switch the bottom view;
  - "+ Add rule" adds a rule; "+ When" ▸ When ▸ Hit adds a When block;
  - "+" after it ▸ Do ▸ Change motion adds a Do block linked to it;
  - two Whens and a Logic: dragging the second When's out-dot onto the Logic block links it (two inputs);
  - clicking a block shows its settings in the inspector; changing a number there sets the block's param; the
    who picker picks a thing;
  - a block pointing at a deleted thing shows the problem mark ("Problem" named, hover words);
  - Change setting: picking Bounciness from the setting menu writes card and setting, and the To row is a number;
  - the from toggle on Change motion's Amount writes `amount~from`;
  - the Rules lane appears on the timeline once the scene has a rule, and has ticks after a firing;
  - right-click a rule ▸ Delete removes it; undo brings it back.
- Everything else unchanged: `cargo test --workspace` green (no golden or picture changes), clippy clean, fmt.

## Not doing

- No free placing of blocks; no zoom of the Rules view.
- No keyframes or pulses on block settings (spec section 10).
- No editor sides for parts 3 to 6's blocks (they add faces and special fields after this merges).
- The emitter's "Rules may change these balls' paths" note (spec 4.7) is not in this part's list.

## As built

- `RuleBlockKind::values()` names what a When's firings carry and `firing_keys()` (planned as `from_keys`, renamed
  for clippy's `wrong_self_convention`) the number settings that can take a value from the firing; both come from
  `block_kind!`'s new `values = [..], from = [..]` tail.
- The lane's own controls (switch, rename field, "+", "+ When") are placed with `Ui::place`, not `Ui::put`, which
  would move the cursor and let the next lane overlap a lane with more rows than its first column
  (`each_lane_sits_below_the_one_before`).
- Picking a setting in Change setting writes its default choice into `to_choice` and sets How to Set for a setting
  that isn't a number, so the To shown is the To saved and the change happens
  (`picking_a_choice_setting_sets_it_to_its_default`). Times shows only for numbers; To hides under Multiply.
- Compare's Value is reset to the first value the new Of offers when Of changes.
- `Fired` reads only the new events while the log grows under the same revision and keeps each rule's end-block
  firings (`firings` gives an `Rc<[f64]>`) until the times change; the timeline's Rules lane skips firings in the
  same pixel column.
- Renaming: Escape keeps the old name, starting another rename saves the first, and the field takes focus back
  after the bottom view was switched away. A link or rule drag left over from a frame the view wasn't drawn is
  dropped (`RulesPanel.drawn_on`).
- New blocks from Duplicate, Paste block after and the lane's Add When menu are picked (and the thing selection
  cleared) so the inspector shows them.
- The who picker's ball number box parses typed text ("ball 3", "every ball", "3").
- The setting menu offers what the runtime's `setting_infos` accepts, so a setting whose `when` hides it for the
  thing kind (Bounce's Takes over on a ball) is still listed, as the runtime would change it.
