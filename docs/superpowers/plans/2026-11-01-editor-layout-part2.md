# Editor layout, Part 2: One section order, jump bar, header — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every inspector card read the same way: the same sections in the same order, rarely-used sections folded, a jump bar to reach any card, a two-row header with a Placement card, a steady panel width, and a grouped Add thing menu.

**Architecture:** The section order is a fixed list in bpe-core. bpe-kit gives every module setting a group from that list. bpe-studio's `widgets::rows_for` sorts groups by the list, folds the quiet ones by default, and marks a folded group that holds changes. The inspector gains a jump bar above its scroll area, a compact header and a Placement card. The tree's context menu takes over the Parent picker as "Move into". The app keeps the inspector width the user drags to, and saves it.

**Tech Stack:** Rust 2024 workspace, egui 0.37 + egui_kittest, the `bpe-settings-derive` `Settings` derive.

**Spec:** `docs/superpowers/specs/2026-11-01-editor-layout-design.md`, Part 2 (2.1 to 2.4) and "Testing". Part 1 is merged (main 6a28385). Part 3 (Scene settings cards, Sound card) gets its own plan after this one merges.

## Global Constraints

- Old saved scenes need not load. The built-in scenes in `scenes/next/*.ron` are regenerated with `BPE_BLESS=1 cargo test -p bpe-kit scenes`. Pictures must not change: this part moves rows and renames labels, and draws nothing differently.
- Physics must not change: `cargo test -p bpe-sim --test golden` passes without re-blessing.
- Every new or renamed `SettingDef` keeps a help sentence that ends with "." and passes `bpe_core::check_registry`.
- Every new egui control is labelled (`labelled_by`), because the kittest accessibility check needs it. Pickers are `menu_button`s, not ComboBoxes.
- Never put `.context_menu` on an egui colour button (it shares the colour picker popup id).
- Section names and order, exactly: Basics, Shape, Style, Burst, Each particle, Across the group, Colours, Inside each one, Surface, Light, Motion, Growing, Instrument, Notes, Which hits, Level, Chords, Other balls, Against some things, Variation, Channel, Blending. ("Surface" is added for the Liquid surface look; see Ruling 2.)
- Folded until opened: Variation, Blending, Other balls, Against some things, Channel.
- New labels, exactly: "Gradient" with choices "Straight" / "Round" (was "Place from": Direction / Centre); "Beyond the last colour" (was "Past the end"); "Gravity ×" (was "Gravity" in Motion modules) with help "How strongly the scene's gravity pulls these, as a multiple."; tree menu "Move into"; card title "Placement".
- Before each push: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Use `CARGO_INCREMENTAL=0` to save disk; disk is tight, so delete stale `target/debug/deps` duplicates if a link fails.

## Rulings made while planning

1. `SECTION_ORDER` and `FOLDED_SECTIONS` live in `bpe_core::settings` (bpe-kit's tests need them and can't see bpe-studio). The spec's per-def `folded` flag becomes the fixed `FOLDED_SECTIONS` list: the spec names the folded groups by name, so a flag on every def would only repeat the list. The spec is amended in Task 1's commit.
2. The spec's section table doesn't cover Liquid surface, Glowing dots, Flocking, Flowing or the shader effects. Liquid surface: Colours, Surface (clarity, refraction, highlight, smoothing, glow), Blending. Glowing dots: Shape (size), Colours, Light (brightness), Variation, Blending. Flocking and Flowing motion, and every shader effect: Basics.
3. Sound modules keep their rows ungrouped until Part 3, which designs the Sound card's sections. Rules has no settings. The "every module setting has a known group" test skips the Sound slot.
4. Thing kinds' own cards keep their groups in appearance order, after their ungrouped rows (spec 2.1 allows ungrouped rows there). Part 3 turns the root's groups into cards.
5. "Colours end at" shows the unit "px" when its paint follows Place or Start. Units are fixed per def, so `widgets` picks the unit through one function, `unit_for(def, value_of)`, rather than adding a field to every `SettingDef`.
6. On the Look card, the paint's heading is "Across the group" on every thing. (The spec's "Colours" for single things needed the instance count at registry time, which the defs don't have.) Trail, Particles, Glowing dots and Liquid surface keep "Colours".

## Review Focus

1. **A card where every row of a section is hidden by `when`.** Expect no empty heading and no fold arrow for it. Test in Task 2 (`a_section_with_no_shown_rows_has_no_heading`).
2. **A keyframe or pulse inside a folded section, with every value at its default.** Expect the dot, because the video still changes there. Test in Task 2 (`a_folded_section_with_a_keyframe_shows_the_dot`).
3. **Jumping to a card that is folded, or below the bottom of a long inspector.** Expect it to unfold and its header to land at the top of the view on the next frame. Test in Task 4 (`the_jump_bar_unfolds_and_scrolls_to_sound`).
4. **Move into, on a folder, offering itself or its own children.** Expect them left out of the list, as the Parent picker does today. Test in Task 3 (`move_into_leaves_out_the_thing_and_whats_under_it`).
5. **A settings file holding a width outside the panel's range, or garbage.** Expect the width clamped to 320 to 640, or the 420 default for garbage, and no crash. Test in Task 5 (`a_saved_width_is_clamped_and_garbage_ignored`).

---

### Task 1: Every module setting in a known section (bpe-core, bpe-kit)

**Files:**
- Modify: `crates/bpe-core/src/settings.rs` (add `SECTION_ORDER`, `FOLDED_SECTIONS`, `section_rank`)
- Modify: `crates/bpe-kit/src/modules/**` (groups on every module setting; `look/basic.rs`, `look/liquid.rs`, `look/swarm.rs`, `trail/mod.rs`, `particles/*.rs`, `bounce/*.rs`, `motion/*.rs`, `shader/*.rs`)
- Modify: `crates/bpe-kit/src/paint/mod.rs` (labels "Gradient" / "Beyond the last colour"; choices `straight` / `round`)
- Modify: `docs/superpowers/specs/2026-11-01-editor-layout-design.md` (Ruling 1: the folded list replaces the `folded` flag)
- Test: `crates/bpe-kit/src/lib.rs` tests (or the existing registry test module)

**Interfaces:**
- Produces: `pub const SECTION_ORDER: &[&str]` and `pub const FOLDED_SECTIONS: &[&str]` (values in Global Constraints). `pub fn section_rank(group: &str) -> Option<usize>`, where a group ending in " variation" ranks with "Variation".
- Produces: the paint's `shape` choice values `straight` and `round` (were `direction` and `centre`). Every `when` that names them is updated (`shape=straight`, `outline_shape=round`, ...).

- [ ] **Step 1: Failing tests**
  - `every_module_setting_has_a_known_section`: for every module kind not in `SlotId::Sound`, every def's `group` is non-empty and `section_rank(group).is_some()`. The failure message names kind, key and group.
  - `the_look_card_reads_in_section_order`: for `look.basic`, the first appearance of each group, in def order, follows `SECTION_ORDER`: Shape, Style, Across the group, Inside each one, Light, Motion, Variation, Blending. `glow` and `flash*` are in Light. `turn` and `spin` are in Motion. `ball_shape`, `outline` and `outline_*` are in Shape.
  - `bounce_sections`: Basics holds bounciness, speed gain, top speed, friction, takes over. Growing holds grow, biggest size, pops after. Other balls holds hits other balls and which balls.
  - `the_place_choice_reads_straight_or_round`: the paint's `shape` def is labelled "Gradient", with choice names `["straight", "round"]`. `repeat` is labelled "Beyond the last colour".
  - `motion_gravity_is_a_multiple`: `gravity_scale` in every motion kind is labelled "Gravity ×", with the help from Global Constraints.

- [ ] **Step 2: Run, expect FAIL** (`cargo test -p bpe-kit section`, `... straight`, `... gravity`).

- [ ] **Step 3: Implement.**
  - Set groups through `#[setting(group = "...")]`, and through the paint embed's group for paint rows.
  - Particles: Burst holds preset, count, speed, spread, least speed and follow impact. Each particle holds size, life, gravity and drag. Colours holds colour from plus the paint.
  - Trail: Shape holds length, width and taper. Colours holds colour from plus the paint. Light holds brightness.
  - Liquid surface, Glowing dots, Flocking, Flowing and shaders: Ruling 2.
  - Order the fields so the defs come out in section order (the derive emits fields in order).

- [ ] **Step 4: Run, expect PASS**, then run `cargo test -p bpe-kit`. Expect only the three stale-scene tests to fail: `the_shipped_scene_files_match_the_code`, `stage5_scenes_resolve_round_trip_and_match_their_files` and `every_bundled_scene_loads_with_known_keys`. The `straight`/`round` rename changes saved choice names. Bless with `BPE_BLESS=1 cargo test -p bpe-kit scenes` and check that the diff only renames.

- [ ] **Step 5: Commit** `kit: every module setting sits in a fixed section`.

### Task 2: Cards follow the section order, fold the quiet sections and mark changes (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/widgets.rs` (`rows_for`, the group heading row, `unit_for`)
- Modify: `crates/bpe-studio/src/inspector.rs` (`Inspector.collapsed` plus a new `opened` set; `fold_groups`; the slot-card calls)
- Test: `crates/bpe-studio/src/widgets.rs` and `inspector.rs` tests

**Interfaces:**
- Consumes: `bpe_core::settings::{SECTION_ORDER, FOLDED_SECTIONS, section_rank}`.
- Produces: `rows_for` keeps its signature. Ungrouped rows come first, then groups sorted by `section_rank`. Unknown groups go last, in appearance order, with a `debug_assert!` naming them only when `defs` belong to a module kind (pass a `module: bool` through the caller, or check at the call site).
- Produces: `pub struct Inspector { .., pub opened: BTreeSet<(ThingId, Option<SlotId>, &'static str)> }`. A group in `FOLDED_SECTIONS` is folded unless it is in `opened`. Any other group is folded only when it is in `collapsed`.
- Produces: `pub fn unit_for(def: &SettingDef, value_of: &dyn Fn(&str) -> Option<Value>) -> &'static str`. It returns "px" for a key ending in `span` whose sibling `follow` (same prefix) is `place` or `start`. Otherwise it returns `def.unit`.

- [ ] **Step 1: Failing tests**
  - `rows_follow_the_section_order`: defs given in the order Blending, Light, Shape come out as Shape, Light, Blending.
  - `a_section_with_no_shown_rows_has_no_heading` (Review Focus 1).
  - kittest `variation_and_blending_start_folded_and_open_when_clicked`: on a ball's Look card, the Variation and Blending headings show, and their rows don't. After a click on Variation, "Vary hue" shows, and stays shown after selecting another thing and coming back.
  - kittest `a_folded_section_with_changes_shows_the_dot`: set `vary_hue` to 20 on the module. The Variation heading's accessible label ends with " (changed)", and the dot is drawn. With defaults, it doesn't.
  - kittest `a_folded_section_with_a_keyframe_shows_the_dot` (Review Focus 2).
  - `span_reads_px_when_following_place`.

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement.**
  - The dot is a small accent-coloured circle after the heading text. The heading's widget info appends " (changed)" so tests and screen readers can see it.
  - "Changed" means any row of the group whose value differs from `def.default`, or that has a track or a pulse at its address. Use the card's existing `get` and `mark_for`.

- [ ] **Step 4: Run, expect PASS.** Then `cargo test -p bpe-studio`. Existing tests that click rows now inside a folded Variation or Blending section must first click the heading open. Update them that way, and don't change what they assert.

- [ ] **Step 5: Commit** `editor: cards follow one section order, quiet sections start folded`.

### Task 3: A two-row header, a Placement card, and Move into (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs` (header block in `show`, `tree_rows` split into a `placement_card`, kind card title)
- Modify: `crates/bpe-studio/src/tree.rs` (`context_menu`: a "Move into" submenu; `Action::MoveInto(ThingId, ThingId)`, applied with the same `set_parent` edit the Parent picker makes today)
- Test: `inspector.rs` and `tree.rs` tests

**Interfaces:**
- Produces:
  - Header row 1: the name field (labelled "Name"), a muted kind badge whose hover is `kind.help()`, and an eye toggle labelled "Shown". The badge and the eye don't appear on the root.
  - Header row 2: Tags.
  - The kind's card is titled `kind.name()`. The root keeps "Scene settings".
  - The Placement card (fold arrow, open by default; fold key `(id, None, "placement")`) holds Position, Rotation, Attach (and the rider's Offset, Turns with it, Item) and Stick to screen. It is absent for the root and the camera. Stick to screen stays hidden for liquid and swarm, as today.
- The Parent row is removed from the inspector.

- [ ] **Step 1: Failing tests**
  - kittest `the_header_is_name_kind_and_shown_then_tags`: for a ball thing, the labels "Name", "Shown" and "Tags" exist, and "Parent" and "Kind" don't. A "Placement" heading exists, with "Position" under it.
  - kittest `the_camera_and_scene_settings_have_no_placement_card`.
  - kittest `move_into_puts_a_thing_in_a_folder`: right-click a ball row, open Move into, click the folder's name. The ball's parent becomes the folder, and one undo step brings it back.
  - `move_into_leaves_out_the_thing_and_whats_under_it` (Review Focus 4): a folder's menu doesn't list itself or its children. It lists "Scene settings" (the root) for a thing that is in a folder.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS**, then `cargo test -p bpe-studio`. Tests that read "Parent" or "Kind" move to the new places. Keep their assertions.
- [ ] **Step 5: Commit** `editor: two-row header, Placement card, Move into from the tree`.

### Task 4: The jump bar (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs` (new `fn jump_bar`, `Inspector.jump_to` and `Inspector.card_tops`, and a call before the `ScrollArea` in `show`)
- Test: `inspector.rs` tests

**Interfaces:**
- Produces:
  - `pub enum Card { Thing, Placement, Slot(SlotId) }`.
  - `Inspector.jump_to: Option<Card>`, the target waiting for the next frame.
  - `Inspector.card_tops: Vec<(Card, f32)>`, each card header's top from the last frame, relative to the scroll area's top.
- Buttons are "Thing" first, then one per slot the thing has, in `SlotId::ALL` order, labelled with `kind.slot_label(slot)`. Placement has no button: it sits just under Thing. For the root, the slots it has are listed after "Scene settings" (Part 3 replaces this list with its cards). Each button is a `selectable_label`, whose accessible name is the label.
- Click: remove the card's fold key from `collapsed` and set `jump_to`. Next frame, when that card's header is drawn, call `response.scroll_to_me(Some(Align::TOP))` and clear `jump_to`.
- The selected button is the last card whose stored top is ≤ 1 px. If none is, it is the first card.
- The bar sits above the `ScrollArea`, outside it, in a `horizontal_wrapped`.

- [ ] **Step 1: Failing tests**
  - kittest `the_jump_bar_lists_thing_then_the_slots_in_order`: a ball shows Thing, Look, Trail, Motion, Bounce, Sound, Particles, Rules, in the order its kind has them.
  - kittest `the_jump_bar_unfolds_and_scrolls_to_sound` (Review Focus 3):
    - Setup: fold the Sound card, and make the panel short (a harness height of 600).
    - Click "Sound" in the bar, then run two frames.
    - Expect: the Sound card is open, its header's rect top is within 2 px of the scroll area's top, and the "Sound" button is selected.
  - kittest `the_jump_bar_marks_the_card_at_the_top_while_scrolling`: scroll down past Look. Expect "Look" to be selected.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS**, then `cargo test -p bpe-studio`.
- [ ] **Step 5: Commit** `editor: a jump bar above the inspector`.

### Task 5: Steady inspector width, one label column, a clipped save status (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/app.rs` (the right panel: start from the saved width, save it when the drag ends; the toolbar's save label)
- Modify: `crates/bpe-studio/src/settings.rs` (`load_inspector_width() -> f32`, `save_inspector_width(f32)`, and `_in(dir)` variants for tests; file `inspector-width-next.txt`)
- Modify: `crates/bpe-studio/src/widgets.rs` and `inspector.rs` (`pub fn label_width(panel: f32) -> f32 = max(120, 0.38 * panel)`; every settings grid uses it as `min_col_width` for column 0 and wraps its labels at that width)
- Test: `settings.rs`, `widgets.rs` and app/inspector kittests

**Interfaces:**
- Produces: `settings::load_inspector_width_in(dir: &Path) -> f32`, which returns 420 when the file is missing or unreadable and clamps to 320 to 640 otherwise. `settings::save_inspector_width_in(dir: &Path, w: f32)`.
- The inspector's contents call `ui.set_max_width(panel_width)`. Labels wrap, so they never widen the panel.
- The save status is drawn in a fixed 180 px slot, elided with "…" (`egui::Label::new(..).truncate()`). Its hover shows the full text.

- [ ] **Step 1: Failing tests**
  - `a_saved_width_is_clamped_and_garbage_ignored` (Review Focus 5): "1000" loads as 640, "abc" as 420, a missing file as 420, and "500" round-trips.
  - kittest `the_inspector_width_holds_when_selecting_scene_settings`: the panel width is measured with a ball selected, then with Scene settings selected. The two are within 1 px.
  - kittest `slider_columns_line_up_across_cards`: the left edge of the "Glow" slider (Look card) and of the "Length" slider (Trail card) are within 1 px.
  - kittest `a_long_save_status_does_not_reach_library`: with a 120-character file name, the status label's rect right edge is left of the Library button's left edge.

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.** Read the width at startup and pass it as the panel's `default_size`. Save on the frame the panel's width changes and the mouse is released. Write only when the value differs from the saved one.
- [ ] **Step 4: Run, expect PASS**, then `cargo test -p bpe-studio`.
- [ ] **Step 5: Commit** `editor: steady inspector width, one label column, clipped save status`.

### Task 6: A grouped Add thing menu (bpe-core, bpe-kit, bpe-studio)

**Files:**
- Modify: `crates/bpe-core/src/registry.rs` (`ThingKind::family(&self) -> &'static str`, default "Other")
- Modify: `crates/bpe-kit/src/things/*.rs` (Bouncers: Balls, Emitter. Walls: Arena, Obstacle. Flowing: Liquid, Swarm, Soft body. Forces: Field. On screen: Text. Folder and Camera stay "Other".)
- Modify: `crates/bpe-studio/src/tree.rs` (the "+ Add thing" menu)
- Test: kit registry test and a tree kittest

**Interfaces:**
- Produces: `const FAMILIES: [&str; 5] = ["Bouncers", "Walls", "Flowing", "Forces", "On screen"]` in tree.rs.
  - The menu draws each family as a small muted heading, then its kinds, in registry order. Folder comes last, after a separator.
  - The camera stays out of the menu while one exists, as today.
  - A kind whose family is "Other" and isn't Folder goes after Folder, so new kinds still appear.

- [ ] **Step 1: Failing tests**
  - `every_addable_thing_has_a_family`: every kit thing kind except scene, folder and camera has a family in `FAMILIES`.
  - kittest `add_thing_groups_kinds_under_headings`: open "+ Add thing". The labels "Bouncers", "Walls", "Flowing", "Forces" and "On screen" exist. "Balls" is drawn below "Bouncers" and above "Walls".

- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS**, then `cargo test --workspace`.
- [ ] **Step 5: Commit** `editor: Add thing grouped by family`.

### Task 7: Check pictures, screenshots, Windows exe, push

**Files:**
- Modify: `scenes/next/*.ron` (blessed again if any task left them stale)
- Output: `/mnt/project-files/ballphysics/editor-layout/part2/` (screenshots, `build-ledger.md`, Windows exes)

- [ ] **Step 1:** `BPE_BLESS=1 cargo test -p bpe-kit scenes`, then `cargo test --workspace` and the golden test.
- [ ] **Step 2: Pictures.**
  - Render all 34 scenes on main and on the branch with `bpe-render-next scenes/next/<name>.ron <out>.mp4 --frames 120 --scale 0.5`. Build main in a separate `git worktree` with its own target dir.
  - Compare with `ffmpeg -lavfi psnr`. Expect every scene bit-identical (this part draws nothing new).
  - Debug any difference to its root cause before going on. Write the table to `build-ledger.md`.
- [ ] **Step 3: Screenshots under Xvfb**, using the method in the Part 1 plan's Task 7:
  - a ball's inspector showing the jump bar, header, Placement card, and Look card sections with Variation folded and its dot;
  - the Add thing menu open;
  - the tree's Move into menu open.
- [ ] **Step 4: Windows exe:** `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`. Copy `ball-studio-next.exe` as `ball-studio.exe`, and `bpe-render-next.exe`, into the part2 folder.
- [ ] **Step 5: Commit** any blessed scenes as `scenes: rebless after section changes`, and push. The controller opens the PR after the final review.
