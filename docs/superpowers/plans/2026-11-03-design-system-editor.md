# Editor rebuild with the Design System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> This project runs it **native** (one builder, one Opus review at the end, per the owner's token rules).

**Goal:** Make `bpe-studio` look and work like the Ball Studio Design System, with a saved track colour per thing
and a grouped, folded timeline.

**Architecture:** A new `style/` module (tokens, fonts, track colours, drawn controls) replaces `theme.rs`; every
panel draws only through it. `bpe-core` gains `TrackColour` on `Thing`. The timeline is rebuilt around per-thing
groups and a Hits lane.

**Tech Stack:** Rust, egui/eframe 0.37, egui_kittest, serde/ron.

**Spec:** `docs/superpowers/specs/2026-11-03-design-system-editor-design.md` (read it first; values below
are copied from it).

## Global Constraints

- Only `crates/bpe-studio/src/style/` names colours, fonts or sizes (source test enforces it, swatch exceptions
  listed in the spec).
- Track colours never reach physics, resolve, gfx or export. Goldens must not change (debug and `--release`).
- Every control keeps an accessible name (`labelled_by` / `widget_info`); `ComboBox` stays only where it is today.
- `cargo fmt` (width 120) and `cargo clippy --workspace` clean. Windows-friendly code.
- Sentence case for every new string. No new features.

## Review Focus

1. An old scene file with no `colour` fields opens: every thing gets a distinct colour in tree order, the root none.
   (Task 1 test `old_scenes_get_colours_in_tree_order`.)
2. More than 8 things: colours wrap to the least-used, never panic. (Task 1 `the_ninth_thing_takes_the_least_used`.)
3. A scene with keys on a thing that is later deleted or hidden: the timeline skips a missing thing and still
   draws a hidden one's group. (Task 6 `groups_skip_missing_things`.)
4. A very narrow inspector (300px) keeps value boxes inside the panel. (Existing app test
   `the_narrowest_inspector_keeps_its_value_boxes_inside` must still pass after Task 5.)
5. Hits from a thing with no colour (root / walls) draw in `TEXT`, not a panic. (Task 6 `hit_colour_falls_back`.)

---

### Task 1: TrackColour in bpe-core

**Files:**
- Modify: `crates/bpe-core/src/scene.rs` (enum, `Thing.colour`, `next_colour`, `give_colours`)
- Modify: `crates/bpe-core/src/registry.rs` (`ThingKind::track_colour`)
- Modify: `crates/bpe-core/src/edit.rs` (`add_thing` sets the colour)
- Modify: `crates/bpe-core/src/save.rs` (`load` calls `give_colours` after `mend`)
- Modify: `crates/bpe-core/src/lib.rs` (re-export `TrackColour`)
- Modify: `crates/bpe-kit/src/things/arena/mod.rs`, `crates/bpe-kit/src/things/camera/mod.rs`

**Interfaces — Produces:**
- `pub enum TrackColour { Pink, Cyan, Lime, Orange, Violet, Yellow, Red, Teal }` (Copy, Eq, Ord, Debug,
  serde `rename_all = "lowercase"`), `TrackColour::ALL: [TrackColour; 8]`, `fn name(self) -> &'static str`.
- `Thing.colour: Option<TrackColour>` (serde default, skip if none).
- `Scene::next_colour(&self) -> TrackColour`, `Scene::give_colours(&mut self, reg: &Registry)`.
- `ThingKind::track_colour(&self) -> Option<TrackColour>` default `None`; Arena `Orange`, Camera `Violet`.

- [ ] Step 1: tests in `scene.rs`/`edit.rs` test modules: `colours_go_in_order` (add 3 balls → Pink, Cyan, Lime;
  root `None`), `arena_and_camera_take_their_own` (camera Violet, arena Orange, a second arena gets next free),
  `the_ninth_thing_takes_the_least_used` (8 balls then a 9th → Pink; remove the Cyan one's colour use and add →
  Cyan), `old_scenes_get_colours_in_tree_order` (save text with colours stripped, `load` → distinct colours in
  `order`, root None), `a_colour_survives_save_and_load`. Use `bpe-kit`'s registry where kinds are needed (these
  tests go in `crates/bpe-kit/tests/track_colours.rs` if core has no arena/camera kinds).
- [ ] Step 2: run `cargo test -p bpe-core -p bpe-kit track` → fail (no type).
- [ ] Step 3: implement as in the Interfaces block and spec §2.
- [ ] Step 4: `cargo test -p bpe-core -p bpe-kit` → pass; goldens unchanged.
- [ ] Step 5: commit "Track colours on things".

### Task 2: The style module (tokens, fonts, apply, controls)

**Files:**
- Create: `crates/bpe-studio/src/style/{mod.rs,tokens.rs,track.rs,fonts.rs,controls.rs}`
- Create: `crates/bpe-studio/assets/fonts/{IBMPlexSans-Regular.ttf,IBMPlexSans-Medium.ttf,IBMPlexSans-SemiBold.ttf,IBMPlexMono-Regular.ttf,OFL.txt}`
  (from github.com/IBM/plex releases or Google Fonts).
- Delete: `crates/bpe-studio/src/theme.rs`; Modify every `theme::` user to `style::` (mechanical rename for now,
  mapping ACCENT/ACCENT_TEXT → TEXT, WARNING/ERROR → TEXT, CARD → PANEL, BACKGROUND → GROUND).

**Interfaces — Produces:** tokens per spec §1; `style::apply(&egui::Context)`; `style::track_color(TrackColour)
-> Color32`; `style::clip_fill(TrackColour) -> Color32`; `style::colour_of(scene: &Scene, id: ThingId) -> Color32`
(track colour, `TEXT` when none); `style::semibold(size: f32) -> FontId`; `style::mono(size) -> FontId`;
controls `primary_button`, `toggle_button`, `toolbar_button`, `switch`, `section_header`, `changed_dot`,
`paint_diamond`, `divider`, `track_picker(ui, current: Option<TrackColour>) -> Option<TrackColour>`.

- [ ] Step 1: tests in `style/mod.rs`: `clip_fill_mixes_22_percent` (Pink → rgb(0x55,0x37,0x43): 0x28+(0xff-0x28)*.22
  = 0x55 rounded, etc. — compute exact bytes in the test from the formula), `a_switch_toggles_on_click` (kittest:
  click by label flips a bool), `the_track_picker_picks` (click "Track colour Teal" → Some(Teal)),
  `no_panel_names_its_own_colour` (source scan, spec §1 exceptions).
- [ ] Step 2: run → fail.
- [ ] Step 3: implement; `apply` exactly per spec §1; kittest harnesses switch `.with_context_setup(crate::style::apply)`.
- [ ] Step 4: `cargo test -p bpe-studio` → all pass (existing tests too).
- [ ] Step 5: commit "Design System style layer for the editor".

### Task 3: Toolbar, status bar, panel frames

**Files:** Modify `crates/bpe-studio/src/app.rs` (`toolbar`, `toolbar_left`, `grid_controls`, `status_line`,
`bar_frame`, `ui` panel frames and default widths `TREE_W`/`INSPECTOR_W`, confirm dialog).
- [ ] Step 1: update/keep app tests (`a_long_save_status_does_not_reach_export`, inspector width tests with the new
  default 370).
- [ ] Step 2: implement per spec §3 Toolbar / Status bar; panel frames with 1px `LINE_SOFT` dividers.
- [ ] Step 3: `cargo test -p bpe-studio app::` → pass. Commit "Toolbar and status bar in the new look".

### Task 4: Tree rows and track colour menu

**Files:** Modify `crates/bpe-studio/src/tree.rs` (`show`, `row`, `name_button`, `context_menu`, `paint_icon`,
`Action::Colour(ThingId, TrackColour)` handled with the other actions through the edit path).
- [ ] Step 1: test `a_row_menu_changes_the_track_colour` (right-click "Ball", click "Track colour Teal" →
  colour Teal; `studio`'s undo restores it if the tree harness has history, else assert the scene edit only).
- [ ] Step 2: run → fail. Step 3: implement per spec §3 Tree. Step 4: `cargo test -p bpe-studio tree::` → pass.
- [ ] Step 5: commit "Tree rows with track colours".

### Task 5: Inspector and settings rows

**Files:** Modify `crates/bpe-studio/src/inspector.rs` (header with colour button `Edit::Colour`, `card`,
card headers → `section_header` with summary, jump bar look), `crates/bpe-studio/src/widgets.rs` (`label_width`
→ fixed 108/96, `label` colour via `label_colour`, changed dot, `group_heading`, bools → `switch`, value wells),
`gradient_bar.rs`, `palettes.rs`, `export_window.rs` (tokens only, switches, primary Export).

**Interfaces — Produces:** `widgets::label_colour(mark: Option<KeyMark>, track: Color32) -> Color32` (track when
keyed or pulsed, else `MUTED`); `GridCx.track: Color32`.
- [ ] Step 1: tests `a_keyed_label_takes_the_track_colour` (unit on `label_colour`), `the_header_colour_button_sets_the_colour`
  (kittest: click "Track colour" button then "Track colour Lime" → thing colour Lime), update
  `the_label_column_is_a_share_of_the_panel_but_never_narrow` to the fixed widths (108 at ≥330, 96 below).
- [ ] Step 2: fail. Step 3: implement per spec §3 Inspector. Step 4: `cargo test -p bpe-studio` → pass (incl.
  `the_narrowest_inspector_keeps_its_value_boxes_inside`).
- [ ] Step 5: commit "Inspector sections and rows in the new look".

### Task 6: Timeline groups, Hits lane, preview selection

**Files:** Modify `crates/bpe-studio/src/timeline/{mod.rs,lanes.rs,ease.rs}`, `crates/bpe-studio/src/transport.rs`,
`crates/bpe-studio/src/preview.rs`, `crates/bpe-studio/src/preview/world_view.rs`.

**Interfaces — Produces:** `lanes::groups_for(scene: &Scene, kit: &Kit) -> Vec<Group>` with
`Group { thing: ThingId, name: String, lanes: Vec<Lane> }` in `scene.order`, only things with tracks or pulses,
skipping missing things; `lanes_for(scene, kit, thing: ThingId) -> Vec<Lane>` (one thing);
`transport::hit_colour(scene: &Scene, thing: ThingId) -> Color32`.
- [ ] Step 1: tests: replace `the_timeline_shows_lanes_for_the_selection_then_scene_settings` with
  `only_the_selected_group_is_open` (ball selected: "Ball · Size" lane present, "Scene settings · Gravity" lane
  absent, group headers "Ball" and "Scene settings" present, key "Key at 0:01.00 of Scene settings · Gravity"
  absent); `clicking_a_group_header_selects_it` (click "Scene settings" header → selection primary ROOT, its key
  now present); `groups_skip_missing_things` (track on ThingId(99) → no group, no panic);
  `hit_colour_falls_back` (ROOT → TEXT; ball → its track colour). Keep the drag/delete/ease/double-click tests
  (they select the ball, so its group is open).
- [ ] Step 2: fail. Step 3: implement per spec §3 Timeline and Preview. Step 4: `cargo test -p bpe-studio` → pass.
- [ ] Step 5: commit "Grouped timeline with clips and a Hits lane; preview selection in track colour".

### Task 7: Verify, review, ship

- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (debug) and `cargo test -p bpe-kit --release`
  goldens: no golden diff vs main.
- [ ] Release build under Xvfb 1440x960, add Balls + Arena, key a setting, screenshot; compare with the mockup by eye
  and fix what differs.
- [ ] Update `CLAUDE.md` (style module replaces theme.rs; track colours on things).
- [ ] One Opus review subagent of the whole branch; fix findings.
- [ ] Push, open PR, build the Windows exe for after the merge.
