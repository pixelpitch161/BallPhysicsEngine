# Editor layout, Part 3: Scene settings cards and a shorter Sound card — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split Scene settings into six short cards (Video, World, Background, Picture, Music, Mix), give every Sound card the same sections, and move the chord editor out of the card into the bottom panel.

**Architecture:** bpe-kit gives every root setting one of six card groups, and the inspector draws the root as one card per group. Each card has its own fold arrow and dice; the dice use a new `Scope::Group`. bpe-kit gives the sound modules' settings section groups. A new `widgets::settings_grid_sections` draws a grid that lets the caller add its own widgets at the end of a section, or as a section of their own. The Sound card uses it for the instrument and its "Try it" keyboard, the chords Edit button, and Channel. The node panel gains a `PanelTarget::Chords` that shows `chords::show` in place of the node canvas.

**Tech Stack:** Rust 2024 workspace, egui 0.37 + egui_kittest, the `bpe-settings-derive` `Settings` derive.

**Spec:** `docs/superpowers/specs/2026-11-01-editor-layout-design.md`, Part 3 (3.1, 3.2) and "Testing". Parts 1 and 2 are merged (main 1e5173d).

## Global Constraints

- Old saved scenes need not load. The built-in scenes in `scenes/next/*.ron` are regenerated with `BPE_BLESS=1 cargo test -p bpe-kit scenes`. Pictures and sound must not change: this part regroups rows, renames labels and moves widgets.
- Physics must not change: `cargo test -p bpe-sim --test golden` passes without re-blessing.
- Every new or renamed `SettingDef` keeps a help sentence that ends with "." and passes `bpe_core::check_registry`.
- Every new egui control is labelled (`labelled_by`, or `widget_info` for custom ones), because the kittest accessibility check needs it. Pickers are `menu_button`s, not ComboBoxes.
- Never put `.context_menu` on an egui colour button (it shares the colour picker popup id).
- Root cards, exactly, in this order: Video, World, Background, Picture, Music, Mix.
- Sound sections use the Part 2 names: Instrument, Notes, Which hits, Level, Chords, Channel (folded), plus one new section, "On the beat", placed after "Which hits" in `SECTION_ORDER`.
- New labels, exactly: "Try it" (fold over the octave buttons and keyboard); "Edit" (chords button, hover "Open the chord editor under the preview"); bottom panel title "Chords: <progression name>" with a "Close" button; the root's Sound slot card and jump button read "Sound" (it was "Music"); the root paint's first row reads "Colours" (it was "Background").
- Before each push: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Use `CARGO_INCREMENTAL=0` to save disk; disk is tight, so delete stale `target/debug/deps` duplicates if a link fails.

## Rulings made while planning

1. **Root cards come from groups in bpe-kit.** The spec says "a card is a group", so every `SceneSettings` field gets `group = "<card>"`, and `bpe_kit::things::scene::ROOT_CARDS` lists the six names. The old sub-groups (Video, Grade, Master, Sound, Song, Beat, Camera sound) go away. Inside a card, rows follow field order, so fields are reordered in the struct to match the spec's table. The inspector draws each card's defs with their group blanked, so no heading repeats the card title.
2. **The root's Sound slot is labelled "Sound".** The spec's jump bar lists both "Music" (the card) and "Sound" (the slot), and today's slot label "Music" would clash with the card.
3. **Per-card dice use a new `Scope::Group { thing, group }`.** It rolls the thing's own settings whose `group` matches. A card with no `random` setting shows no dice. Only Picture (bloom) and Music (seed) have dice today.
4. **Drums' "On beats" and "Beat volume" go in a new "On the beat" section,** after "Which hits". They aren't hit settings, and no spec section fits them. Melody's "Tune", "Part", "Transpose" and "Play chords together" go in Notes, and "Velocity from hit" goes in Which hits. Drums' "Drum" goes in Instrument.
5. **The chord editor's audition uses the module's own instrument,** not a thing's override of it. The bottom panel knows the module, not the thing. An override of the instrument on one thing is rare, and the cost is hearing the module's instrument in the editor.
6. **The chord editor's per-progression state (`ChordUi`) moves from `Inspector` to `NodePanel`.** Only the panel draws the editor now.
7. **Clicking the timeline chord lane label opens the progression the lane shows,** auditioned through the module of the thing the lane read it from.

## Review Focus

1. **A root card whose every row is hidden by `when`.** Example: a `when` turns off every row, though none does today. Expect the card still drawn with its title, and no empty grid. Test in Task 2 (`a_root_card_with_no_shown_rows_still_has_its_title`).
2. **The chords picker set to the built-in starter chords** (no progression picked). Expect the muted "Plays the built-in starter chords…" line, and the Edit button disabled with hover "Pick or make a progression to edit it". Test in Task 4 (`edit_is_off_for_the_starter_chords`).
3. **A progression deleted, or undone away, while its editor is open.** Expect the panel to close on the next frame with no panic. Test in Task 5 (`the_chord_panel_closes_when_its_progression_goes`).
4. **Opening Nodes while Chords is open, and the other way round.** Expect only one panel; the new one replaces the old. Test in Task 5 (`nodes_and_chords_replace_each_other`).
5. **The Channel section, folded by default, on a sound module that has inserts.** Expect the "Channel (changed)" dot when inserts or a send are set. Test in Task 4 (`channel_shows_the_dot_when_it_has_inserts`).

---

### Task 1: Root card groups, the group dice and the Sound sections (bpe-core, bpe-kit)

**Files:**
- Modify: `crates/bpe-kit/src/things/scene/mod.rs` (groups, field order, `ROOT_CARDS`, slot label, paint label)
- Modify: `crates/bpe-kit/src/random.rs` (`Scope::Group`)
- Modify: `crates/bpe-core/src/settings.rs` (`SECTION_ORDER` gains "On the beat")
- Modify: `crates/bpe-kit/src/modules/sound/mod.rs`, `melody/mod.rs`, `drums/mod.rs` (groups, field order)
- Modify: `crates/bpe-kit/src/lib.rs` (the section-coverage test stops skipping the Sound slot)
- Modify: `scenes/next/*.ron` (rebless)

**Interfaces:**
- Produces: `pub const ROOT_CARDS: [&str; 6] = ["Video", "World", "Background", "Picture", "Music", "Mix"]` in `bpe_kit::things::scene`, re-exported as `bpe_kit::ROOT_CARDS`.
- Produces: `Scope::Group { thing: ThingId, group: &'static str }` in `bpe_kit::random`.
- Produces: `SECTION_ORDER` with "On the beat" between "Which hits" and "Level".

- [ ] **Step 1: Write the failing tests.**
  - In `scene/mod.rs`:
    - `every_root_setting_is_on_a_card`: every def of `SceneSettings::defs()` has a `group` in `ROOT_CARDS`.
    - `root_cards_hold_the_spec_rows`: for each card, the labels of the defs with that group, in order, equal the spec's table, as below.
      - Video: `["Width", "Height", "Frame rate", "Length"]`.
      - World: `["Gravity", "Physics rate"]`.
      - Background: the paint's rows (first one labelled "Colours"), then `"Background stays on screen"`.
      - Picture: `["Bloom", "Exposure", "Brightness", "Contrast", "Saturation", "Warm / cool", "Tint", "Vignette", "Vignette size", "Gradient map", "Gradient map amount"]`.
      - Music: `["Tempo", "Beat sync", "Beats split into", "Stretch", "Song", "Song starts at", "Song volume", "Ducks under hits", "Seed"]`.
      - Mix: `["Loudness", "Clipper", "Clipper drive", "Ceiling", "Auto release", "Release", "Off-shot fade"]`.
    - `the_root_sound_slot_reads_sound`: `SceneKind.slot_label(SlotId::Sound) == "Sound"`.
  - In `random.rs`, `a_group_roll_changes_only_that_group`: on a scene, `roll` with `Scope::Group { thing: ThingId::ROOT, group: "Music" }`. Only `seed` changes, never `bloom`. Rolling "Video" changes nothing.
  - In `lib.rs`, extend the existing "every module setting has a known section" test so it no longer skips the Sound slot.
  - In `lib.rs`, add `sound_cards_use_the_spec_sections`. The group sequence (first appearance order, after `rows_for`-style sorting by `section_rank`) must be:
    - `sound.notes`: `["Instrument", "Notes", "Which hits", "Level", "Chords"]`;
    - `sound.melody`: `["Instrument", "Notes", "Which hits", "Level"]`;
    - `sound.drums`: `["Instrument", "Which hits", "On the beat", "Level"]`.
  - In `lib.rs`, `sound_notes_rows_follow_the_spec`: the Notes labels of `sound.notes` start `["Lowest note", "Highest note", "Picks from", "Order", "Plays"]`, then the two `when = "plays=full_chord"` rows. Which hits is `["Plays when", "Only hits against", "Shortest gap", "Quietest hit", "Loudest hit"]`. Level is `["Volume", "Pan", "Stereo width"]`.
- [ ] **Step 2:** `cargo test -p bpe-kit -p bpe-core` and see the new tests fail.
- [ ] **Step 3: Implement.**
  - Set the groups and reorder the fields to match. Keep each field's help. Change the paint attribute's `label` to "Colours" and `slot_label(Sound)` to "Sound".
  - Add "On the beat" to `SECTION_ORDER`.
  - Add `Scope::Group`, handled beside `Scope::Card { slot: None }`, keeping only defs with `def.group == group`.
  - Keep the scene's `grade()` and every reader of these keys unchanged: keys don't change, only groups, order and two labels.
- [ ] **Step 4:** `BPE_BLESS=1 cargo test -p bpe-kit scenes`, then `cargo test --workspace` and `cargo test -p bpe-sim --test golden`. All pass. bpe-studio tests that look for the old "Music" slot label or old group headings may fail here; Task 2 and Task 4 update them. Note any you leave failing in the report.
- [ ] **Step 5: Commit** `kit: root settings in six card groups, sound modules in sections`.

### Task 2: Scene settings drawn as six cards (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs` (root cards, `Card::Section`, jump bar, Mix holds Buses and Master effects, `Edit::RollGroup`)
- Test: `crates/bpe-studio/src/inspector.rs` tests module

**Interfaces:**
- Consumes: `bpe_kit::ROOT_CARDS`, `Scope::Group` (Task 1).
- Produces: `Card::Section(&'static str)`, the fold key `(id, None, name)`, and `Edit::RollGroup(&'static str)`.

- [ ] **Step 1: Write the failing tests** (kittest, `harness(Some(ThingId::ROOT))`):
  - `scene_settings_shows_six_cards`: headings "Video", "World", "Background", "Picture", "Music", "Mix" exist (each has "Hide <name>"), with tops in that order. "Hide Scene settings" is gone.
  - `the_root_jump_bar_lists_the_cards_then_the_slots`: `jump_bar(&h)` labels start with `["Video", "World", "Background", "Picture", "Music", "Mix"]`, followed by the root's slot labels ("Effects", "Sound", "Rules" for the slots it has).
  - `master_effects_sit_in_the_mix_card`: the "Master effects" label's top lies below the "Mix" heading, with no other card heading between them.
  - `jumping_to_mix_scrolls_it_to_the_top`: click the bar's "Mix", run, and the Mix top in `card_tops` is within 1 px of 0.
  - `a_root_card_with_no_shown_rows_still_has_its_title`: draw a root card with an `applies` that hides everything, through the same function the inspector uses. Its "Hide <name>" arrow exists and no grid row is drawn.
  - `music_card_dice_rolls_only_music`: click the Music card's "Randomize these settings" and only `seed` changes. The Video card has no dice.
  - Update the existing tests that used "Scene settings" as the root's bar button, the "Music" slot label or "Hide Scene settings". Leave the tree's "Scene settings" row unchanged; the thing is still named that.
- [ ] **Step 2:** run them and see them fail.
- [ ] **Step 3: Implement.**
  - For `id == ThingId::ROOT`, replace the single kind card with a loop over `ROOT_CARDS`. Each card takes the defs whose group is its name, cloned with `group: ""`, and draws them with the same `GridCx` code the kind card uses today (factor it into one function both paths call). The title comes from `fold_arrow` + `RichText::strong`. The dice show only when a def in the card has `flags.random`.
  - `card_header(ui, inspector, Card::Section(name), …)`.
  - Move the Buses and Master effects cards inside the Mix card, under its grid, as two sub-sections with the same strong labels as today.
  - In `jump_bar`, for the root, emit `Card::Section(name)` for each of `ROOT_CARDS` in place of `Card::Thing`.
  - `card_at_top` falls back to the first card drawn rather than `Card::Thing` when nothing is at the top yet. Use the first entry of `card_tops`, or `Card::Thing` when there is none.
  - `Card::fold_key` for `Section(name)` is `(id, None, name)`.
- [ ] **Step 4:** `cargo test -p bpe-studio` passes.
- [ ] **Step 5: Commit** `studio: Scene settings as six cards with a jump bar`.

### Task 3: A settings grid with the caller's own section contents (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/widgets.rs`
- Test: `crates/bpe-studio/src/widgets.rs` tests module

**Interfaces:**
- Produces:

```rust
/// Widgets a caller draws at the end of one section of a settings grid, below its rows. A
/// section named only here (no setting has its group) gets its own heading, ranked by
/// `section_rank`, and folds like any other.
pub struct SectionExtra<'a> {
    pub group: &'static str,
    /// Whether the section holds changes, for the folded-with-changes dot.
    pub changed: bool,
    pub draw: &'a mut dyn FnMut(&mut Ui),
}

pub fn settings_grid_sections(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Clone,
    defs: &[SettingDef],
    get: impl Fn(&str) -> Value,
    cx: &GridCx,
    extras: &mut [SectionExtra<'_>],
) -> GridOutcome
```

- `settings_grid_marked` stays, as `settings_grid_sections` with no extras.

- [ ] **Step 1: Write the failing tests** (kittest on a small `SettingDef` list, as the existing widget tests do):
  - `an_extra_draws_under_its_sections_rows`: defs in groups "Notes" and "Level". An extra on "Notes" adds a button "Extra". The button's top is below the last Notes row and above the "Level" heading.
  - `an_extra_only_section_gets_a_heading_in_rank`: an extra on "Channel" with no defs gets a "Channel" heading after "Level". It is folded by default (`FOLDED_SECTIONS`), so the button is hidden until its heading is clicked and the fold is fed back through `folds`.
  - `a_shut_extra_section_with_changes_shows_the_dot`: `changed: true` and folded, so "Channel (changed)" exists.
  - `labels_line_up_across_the_split`: a row's label left edge in Notes equals one in Level.
- [ ] **Step 2:** run them and see them fail.
- [ ] **Step 3: Implement.** Walk the rows from `rows_for`, with the extra-only sections merged in by rank. Close the current `settings_like_grid` after a section that has an extra, call the extra's `draw` (when the section isn't shut), and open a new grid with id `(id, group)` for what follows. Every grid uses `cx.label_width` and the same `right` edge, so columns line up.
- [ ] **Step 4:** `cargo test -p bpe-studio widgets` passes.
- [ ] **Step 5: Commit** `widgets: settings grid with caller-drawn section contents`.

### Task 4: The Sound card in sections (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/inspector.rs` (the slot card's three `Write` arms, `instrument_section`, `chord_editor`, `channel_section`, `Inspector`)

**Interfaces:**
- Consumes: `settings_grid_sections`, `SectionExtra` (Task 3); the sound groups (Task 1).
- Produces: `Inspector.try_open: bool` (default false); `Inspector.chords_request: Option<(ProgressionId, ModuleId)>`, taken by the app in Task 5.

- [ ] **Step 1: Write the failing tests** (kittest on a ball with a `sound.notes` Sound card, and one with `sound.drums`):
  - `sound_card_sections_in_order`: headings Instrument, Notes, Which hits, Level, Chords, Channel, with tops in that order.
  - `the_instrument_macros_sit_in_instrument`: the instrument section's first widget (e.g. the "Plays the built-in Pluck…" line, or a macro knob on a picked instrument) is between the Instrument and Notes headings.
  - `try_it_is_folded_until_opened`: "Oct −" is absent. Click "Show Try it" and run: "Oct −" and the keyboard keys show, under Instrument.
  - `edit_opens_the_chord_editor_request`: with a progression picked, click "Edit" and `inspector.chords_request == Some((pid, module))`. No "Chord lane" widget is drawn in the inspector.
  - `edit_is_off_for_the_starter_chords`: with no progression picked, the "Edit" button is disabled and the starter-chords line shows.
  - `channel_shows_the_dot_when_it_has_inserts`: give the module a non-empty insert graph or one send above `SEND_OFF`. "Channel (changed)" shows while folded.
  - `drums_card_has_on_the_beat`: a `sound.drums` card shows "On the beat", and no "Notes" or "Chords".
- [ ] **Step 2:** run them and see them fail.
- [ ] **Step 3: Implement.**
  - In each `Write` arm that draws a Sound slot (Own, Override), build three extras and draw through `settings_grid_sections`:
    - **Instrument:** today's `instrument_section` body without the keyboard, then a `fold_toggle` named "Try it" over the octave buttons and `crate::keyboard::show`.
    - **Chords:** only when the defs have a `ValueType::Progression`. Either today's starter-chords line or an "Edit" button, enabled when a library progression is picked.
    - **Channel:** today's `channel_section` when `slot == SlotId::Sound`; `changed` is true when the module has inserts or any send above `SEND_OFF`.
  - Remove the inline `chord_editor` call and move `chord_ui` out of `Inspector` (Task 5 takes it). The Inherited arm keeps its disabled plain grid.
  - Remove `chord_editor` if nothing calls it. Keep `chord_clip`, but move it to `crate::chords` as `pub fn clip_for(chords: Vec<Chord>, bpm: f32, scene: &Scene, module: Option<ModuleId>, instrument: Option<InstrumentId>) -> ChordClip`, since Task 5 uses it too.
  - Non-sound slots keep `settings_grid_marked`.
- [ ] **Step 4:** `cargo test -p bpe-studio` passes.
- [ ] **Step 5: Commit** `studio: the Sound card in sections, chords Edit, Try it`.

### Task 5: The chord editor in the bottom panel (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/nodes/panel.rs` (`PanelTarget::Chords`, `open_chords`, `show_chords`, `chord_ui`, `clip`)
- Modify: `crates/bpe-studio/src/app.rs` (take `chords_request`, play the panel's clip)
- Modify: `crates/bpe-studio/src/timeline/chords.rs`, `timeline/mod.rs` (the lane label opens the editor)

**Interfaces:**
- Consumes: `Inspector.chords_request` (Task 4); `chords::{show, ChordUi, Audition, clip_for}`.
- Produces:
  - `PanelTarget::Chords(ProgressionId, ModuleId)`;
  - `NodePanel::open_chords(&mut self, p: ProgressionId, module: ModuleId)`;
  - `NodePanel.clip: Option<ChordClip>`;
  - `timeline::chords::show` returning `Option<(ProgressionId, ModuleId)>` when its label is clicked.

- [ ] **Step 1: Write the failing tests** (app-level kittest, as the existing node panel tests in `nodes/tests.rs` or `app.rs` do):
  - `edit_opens_chords_in_the_bottom_panel`: select a ball whose sound picks a progression and click "Edit". A label "Chords: <name>" and the "Chord lane" widget show, and no node canvas.
  - `close_hides_the_chord_panel`: click "Close", and "Chord lane" is gone.
  - `nodes_and_chords_replace_each_other`: open Chords, then press the Look card's Nodes. The chord lane is gone and the node panel shows. Then press Edit again, and the node panel is gone.
  - `the_chord_panel_closes_when_its_progression_goes`: open it, remove the progression from `scene.library.progressions`, run two frames, and expect `panels.nodes.open == None` with no panic.
  - `a_chord_edit_in_the_panel_reaches_the_scene`: click an in-key chord button and the progression's chord count goes up by one in `scene.library`.
  - `the_timeline_chord_label_opens_the_editor`: click the "Chords (<name>)" label in the timeline, and "Chords: <name>" shows.
- [ ] **Step 2:** run them and see them fail.
- [ ] **Step 3: Implement.**
  - `open_chords` replaces whatever is open, as `open_patch` does.
  - `show` routes `Chords` to `show_chords`, which works like `show_patch`:
    - it draws the same `egui::Panel::bottom("nodes")`, with a header row of the title, then "Close" on the right;
    - below that, a vertical `ScrollArea` with `chords::show` on a clone of the progression;
    - it calls `scene.set_progression` on change;
    - it closes when the progression or module is gone.
  - An audition sets `self.clip` from `chords::clip_for`, with bpm from `bpe_kit::music::root_settings(resolved).bpm` and the module's own `instrument` param (Ruling 5).
  - In `app.rs`, take `inspector.chords_request` beside `fx_request`, and play `panels.nodes.clip` the way `inspector.clip` is played.
  - The timeline label becomes clickable (`Sense::click`), with hover "Edit these chords". `shown` returns the module along with the progression.
  - The tree's Delete guard still uses `chords::lane_focused`.
- [ ] **Step 4:** `cargo test -p bpe-studio` passes.
- [ ] **Step 5: Commit** `studio: chord editor in the bottom panel`.

### Task 6: Check pictures and sound, screenshots, Windows exe, push

**Files:**
- Modify: `scenes/next/*.ron` (blessed again if stale)
- Output: `/mnt/project-files/ballphysics/editor-layout/part3/` (screenshots, `build-ledger.md`, Windows exes)

- [ ] **Step 1:** `BPE_BLESS=1 cargo test -p bpe-kit scenes`, then `cargo test --workspace` and the golden test.
- [ ] **Step 2: Pictures and sound.**
  - Render every scene on main and on the branch with `bpe-render-next scenes/next/<name>.ron <out>.mp4 --frames 120 --scale 0.5`. Build main in a separate `git worktree` with its own target dir.
  - Compare the video with `ffmpeg -lavfi psnr`. Compare the audio by md5 of `ffmpeg -i <out>.mp4 -map 0:a -f s16le -`.
  - Expect every scene bit-identical. Debug any difference to its root cause before going on. Write the table to `build-ledger.md`.
- [ ] **Step 3: Screenshots under Xvfb**, using the method in the Part 1 plan's Task 7:
  - Scene settings with the jump bar and the Video and World cards;
  - the Mix card with Buses and Master effects;
  - a ball's Sound card with Try it open;
  - the chord editor open in the bottom panel.
- [ ] **Step 4: Windows exe:** `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`. Copy `ball-studio-next.exe` as `ball-studio.exe`, and `bpe-render-next.exe`, into the part3 folder.
- [ ] **Step 5: Commit** any blessed scenes as `scenes: rebless after part 3`, and push. The controller opens the PR after the final review.
