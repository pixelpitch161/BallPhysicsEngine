# W5: editor workflow

Workstream W5 of the 2026-11-08 improvement review (report "Ball Studio code review: what to improve", findings E7 to
E14). Aedan picked it on 2026-11-08 ("everything besides Size"), so the design is approved; this plan records every
decision. Editor-only apart from two data additions in `bpe-core` (copying things, pictures kept in the scene) and the
Particles picture reading those bytes: no physics, picture or sound changes, so goldens and picture hashes must not
move. W4 (never lose work) is merged; W6 (editor speed, PR #161) touches `app.rs` and `timeline/mod.rs` nearby, so
rebase onto it if it merges first.

## Goal

| Finding | What changes for Aedan |
| --- | --- |
| E7 | Ctrl+D duplicates, Ctrl+C / Ctrl+V copy and paste things (or the timeline's selected keys), Left / Right step one frame (Shift: one beat), End goes to the end, Delete works from any tab. |
| E8 | Copy thing / Paste thing in the tree's row menu, bringing children, cards, keys, pulses and the fonts and pictures they use, also into another scene opened in the same session. Duplicate acts on the whole selection and copies children too. |
| E9 | Timeline keys: Shift- or Ctrl-click to select several, drag a box over empty lane space to select, drag any selected key to move them all, Ctrl+C / Ctrl+V to paste them at the playhead, Delete removes them, right-click "Repeat every bar". |
| E10 | A setting's right-click menu has "Reset to default" and "Remove all keyframes". Double-clicking the slider itself no longer resets (only the label does, and Alt+click on it). |
| E11 | A Particles picture is kept inside the scene, like fonts, so moving the PNG no longer loses it. A picture that can't be found says "Picture not found" on its row. |
| E12 | The empty preview shows a hint; "+ Add thing" sits at the top of the Things panel; scenes, fonts and PNG pictures can be dropped onto the window. |
| E13 | A "Find a setting" box at the top of the inspector lists every matching setting of the selected thing, ready to edit. |
| E14 | "+ Add slot" says "+ Add card". The window title is "Scene name • – Ball Studio" (the dot while unsaved). |

Not done (and why): ready-made looks for Balls and Shapes (E12's optional extra) are presets, which the project rules
rule out unless Aedan asks. Pasting into a second, separately started editor window (system clipboard) is left out:
the in-memory clipboard already reaches every scene opened in the session.

## Decisions

### Copying things (E8), new `bpe-core/src/clip.rs`

- `pub struct ThingClip { things: Vec<Thing>, tops: Vec<ThingId>, tracks: BTreeMap<Address, Track>, pulses:
  BTreeMap<Address, Pulse>, fonts: BTreeMap<String, SceneFont>, pictures: BTreeMap<String, ScenePicture>, outside:
  BTreeMap<ThingId, (KindId, String)> }`: things in tree order with their source ids; `outside` names every thing a
  copied setting points at that isn't copied (kind and name, to check it on paste).
- `Scene::copy_things(&self, reg, ids) -> Option<ThingClip>`: the tops are `ids` minus the root, anything inside another
  listed thing and one-per-scene things (the camera; its subtree is skipped). Each top brings all its descendants, their
  tracks and pulses (by `Address.thing`), and the fonts and pictures named by a `Value::Text` of theirs whose key is
  `font` or `picture` (own, slot or layered key `…/font`). `None` when nothing is left.
- `Scene::paste_things(&mut self, reg, clip, parent, after: Option<ThingId>) -> Result<Pasted, EditError>` with
  `Pasted { tops: Vec<ThingId>, unlinked: usize }`:
  - New ids from `next_thing`; parents remapped (a top hangs off `parent`, keeping its `attach` only when `parent` is
    its old parent, else `Attach::Group`); one-per-scene kinds already present are skipped with their subtree.
  - Each pasted thing gets a new track colour (`new_colour`) and keeps its name unless a thing outside the paste has it,
    then `free_name("<name> copy")`.
  - References inside values (`Target::Thing`, `Who::Thing`, `SceneLight.thing`) and pulse sources (`Sound.shape`,
    `Light.thing`, `Health.thing`) to a copied thing point at its copy. One to an outside thing stays when this scene
    has a thing of that id with the same kind and name (pasting into the same scene); otherwise the target becomes
    `Target::Anything` / `Who::Anything`, a pulse following it is dropped, and `unlinked` counts it.
  - Fonts and pictures: a name this scene lacks is added as it is; the same bytes under another name use that name; a
    name taken by other bytes goes in as `"name (2)"` (`add_font` / `add_picture`) and the pasted values are renamed.
  - Placed after `after` when it is a child of `parent`, else last under `parent`.
- Rules (scene-wide) are not copied.
- Studio: `crate::clipboard::Clip { Things(ThingClip), Keys(KeyClip) }` in `Studio.clipboard` (last copy wins,
  survives New and Open). `things.rs` (new, studio) holds `copy`, `paste`, `duplicate`, `delete` on a `Studio`:
  - paste parent: a selected folder, else the primary's parent (placed after the primary), else the root, as Add does;
    selects the pasted tops; status "Pasted Balls." / "Pasted 3 things."; with `unlinked > 0` it adds "2 settings
    pointed at things that aren't in this scene and now point at anything."
  - duplicate: each selected top (the selection's tops, as Delete uses) is copied and pasted under its own parent right
    after itself; the copies end up selected. Replaces `tree::duplicate`; keys and pulses come along as before.
  - each is its own undo step (`recorded` via `after_separate_edit`).
- Tree row menu: "Copy" and "Paste" (greyed without copied things, hover "Copy a thing first") next to Duplicate; the
  menu returns them in `TreeOutcome` (`copy: Option<Vec<ThingId>>`, `paste: bool`) for the app to apply.

### Keyboard (E7), `app.rs`

- `shortcuts` (before the panels) keeps Save/Open/New/F11/undo/Space/Home. New there, when no text field has the
  keyboard and no widget has focus: Left / Right one frame (`(round(t·fps) ± 1) / fps`), Shift+Left / Shift+Right one
  beat (`(round(t / beat) ± 1) · beat`), End to the end; each pauses and seeks.
- New `late_shortcuts` after the panels (so the timeline's own Delete, which consumes the key, wins) when no text field
  has the keyboard: Ctrl+D duplicate; Ctrl+C copy (keys when the timeline has selected keys, else the selected things;
  also on `egui::Event::Copy`, which egui-winit sends for Ctrl+C); Ctrl+V paste (the clipboard's keys or things; also
  `Event::Paste`); Delete deletes the selection from any left tab. The tree's own Delete check is removed.
- The tree's hint and tooltips name the keys (Duplicate "Ctrl+D", Copy "Ctrl+C", Paste "Ctrl+V").

### Timeline keys (E9), `timeline/mod.rs`, `bpe-core/src/animation.rs`

- `Timeline.selected: BTreeSet<(Address, usize)>` (was `Option`), `Timeline::single()` for the curve editor.
- Click a key: it alone; Shift/Ctrl-click toggles it in the selection. Dragging a key not selected selects it alone
  first (with Shift/Ctrl, adds it). Click on empty lane space clears; Escape clears.
- Box: a drag that starts on empty lane space (lanes become `click_and_drag`) draws a box (fill `style::SELECTED`,
  edge `style::TEXT`) over the open group's lanes; on release every key whose diamond centre is inside is selected
  (added to the selection with Shift/Ctrl). Key centres are collected while drawing.
- Group drag: on drag start the touched tracks are kept (`GroupDrag { grab: (Address, f64), before: BTreeMap<Address,
  Track>, picked: Vec<(Address, usize)> }`). Each frame `delta = snap(x) - grab time`, clamped so no key goes below 0,
  then `Scene::move_keys(before, picked, delta, settle: false)` rebuilds those tracks from `before` with the picked
  keys moved and returns their new indices (the selection). On release the same with `settle: true`, which drops any
  unpicked key within `SAME_TIME` of a moved one (a key put down on another takes its place, as today).
- Copy (Ctrl+C or "Copy N keys" in a key's menu): `KeyClip { thing, keys: Vec<(Address, f64 offset, Key)> }`, offsets
  from the earliest. Paste (Ctrl+V, or "Paste keys at the playhead" in a lane's right-click menu): each key at
  `playhead + offset` with `set_key` and its ease; keys copied from one thing go onto the open thing's same settings
  when another thing is open (settings it doesn't have are skipped, status says how many landed). The pasted keys are
  selected.
- "Repeat every bar" in a key's menu (selection non-empty): period = bar × max(1, ceil(span / bar − 1e-6)) where span
  is last minus first selected time (the label says "every 2 bars" when it is 2); copies at `t + n·period` while
  inside the video length, replacing keys at the same times. Own undo step.
- Delete removes every selected key. A key's ease menu sets the ease of every selected key on hold-free tracks.
- `Scene::move_keys` and `Track` stay index-based; unit tests in `animation.rs` (`move_keys_moves_together`,
  `settling_replaces_keys_underneath`, `no_key_goes_below_zero` is the caller's clamp, tested in the timeline).

### Setting menu (E10), `widgets.rs`

- `row_menu_following` gets "Reset to default (<value words>)" under Paste Value, enabled when the value differs
  from `def.default` (returns `MenuPick::Paste(default)`), and "Remove all keyframes" under Create Keyframe, enabled
  when `mark.keyed` (`MenuPick::RemoveKeys`).
- `RowAction::RemoveKeys`, `GridOutcome.cleared: Vec<&'static str>`, `ValueMenu::RemoveKeys`. Every grid caller adds
  `Edit::ClearKeys(slot, key)` beside its `Edit::NewKey`; inspector applies it to every reached thing
  (`scene.tracks.remove`). Knob callers (mixer `cx.rs`, `master.rs`, sound tab, lights strip) remove the track the
  same way as their `NewKey`.
- The widget's own double-click reset goes (a DragValue's double-click starts typing, which made accidental resets);
  the label keeps double-click and Alt+click.

### Pictures kept in the scene (E11), `bpe-core/src/scene_pictures.rs`, `bpe-kit` particles

- `Scene.pictures: BTreeMap<String, ScenePicture>` (bytes, base64 in the file, left out when empty, like `fonts`).
  `add_picture(name, bytes) -> String` dedups by bytes and numbers names like `add_font`. `save` leaves out pictures
  no `picture` value names (own params, slot params, layered keys, tracks), so a picked-over picture doesn't grow the
  file forever. Format stays 12 (an older file has no field and reads as none).
- Particles `key_of`: when the scene (`tree`) has a picture named `s.picture`, `SpotsKey::Picture` carries its bytes
  (`PictureBytes(Arc<[u8]>)`, equal and hashed by pointer and length, so comparing keys never walks the bytes) and
  `picture_spots` decodes them; else the old path behaviour (a path still works).
- Editor: the picture row's "…" reads the PNG and asks (as Load font does) for `Edit::LoadPicture(key, name, bytes)`,
  which adds it and sets the setting to the name for every reached thing. Under a non-empty value that is neither a
  scene picture nor an existing file the row shows "Picture not found" in `style::PROBLEM` (hover: "Choose the PNG
  again with …; it is then kept inside the scene"). `TargetChoices.pictures` lists the scene's picture names.

### First start and dropping files (E12)

- Tree: "+ Add thing" is the first thing in the Things panel, the rows scroll under it, the drag hint stays at the
  bottom.
- Preview: while the scene has nothing but the scene settings and the camera, a hint in the middle of the stage:
  "Nothing here yet" and "Click + Add thing at the top left and pick Balls, then add a Shape for them to bounce in. You
  can also drop a scene, a font or a PNG picture onto the window." (`style::TEXT` / `style::MUTED`). While files hover
  over the window: "Drop to open it" over the stage.
- `Studio::take_drops(ctx)` (in `ui`, before the panels): the first dropped file decides: `.ron` opens it through the
  usual unsaved-changes guard (`Pending::OpenPath`); `.ttf`/`.otf` loads the font into the scene and, when Text things
  are selected, sets their Font; `.png` keeps the picture in the scene and sets it on selected Particles things (made of
  a picture), or else adds a "Form a word" Particles thing made of the picture. Anything else: a File problem "Ball
  Studio can't open <name>: drop a scene (.ron), a font (.ttf, .otf) or a PNG picture."

### Find a setting (E13), `inspector.rs`

- `Inspector.find: String`, a search field (hint "Find a setting", × to clear) above the jump bar on the Settings
  tab. While it holds text, the cards give way to the matches: for the kind's own settings, each plain card (by its
  title) and each layer of a layered card ("Look › Glow"), the settings whose label or group contains the words (case
  ignored, every word must match), that apply now, as grids under a small heading naming where they live, with groups
  blanked so nothing is folded. Edits are the usual `SetOwn` / `SetSlot` (layered keys) through `own_grid`,
  `slot_grid`'s grid and `look_editor::layer_settings`, so menus, keys and follow all work. "No settings match" when
  none do. The text is kept per editor (not per thing), cleared by ×.

### Wording (E14)

- "+ Add slot" → "+ Add card", its disabled hover "Every card this kind can use is already shown"; docs updated.
- `Studio::window_title()` = `"<scene name or Untitled> • – Ball Studio"` while unsaved, else without the dot;
  sent with `ViewportCommand::Title` only when it changes; `main.rs` starts with "Ball Studio" (the app id given to
  `run_native` stays, it names egui's storage).

## Tests (what passing looks like)

- `bpe-core`: `clip.rs` tests: copy brings children/keys/pulses; paste into the same scene keeps outside links and
  remaps inside ones; paste into another scene unlinks outside ones and counts them; the camera is skipped; fonts and
  pictures come along and are renamed on a clash. `scene_pictures.rs`: save/load round trip, unused pictures left
  out. `animation.rs`: `move_keys`.
- `bpe-kit`: a Particles thing made of a scene picture finds the same spots as from the file.
- `bpe-studio` kittest: Ctrl+D duplicates the selection with children; Ctrl+C/Ctrl+V pastes into a New scene; Delete
  from the Mixer tab deletes; Left/Right/Shift/End move the playhead; Shift-click and box select keys, group drag moves
  both, Ctrl+C/V pastes keys at the playhead, Repeat every bar fills to the end; Reset to default and Remove all
  keyframes; picture "not found" note and LoadPicture; dropping a .ron/.ttf/.png; Find a setting lists Lighting's
  matches and an edit there changes the card; the window title.
- The usual: `cargo fmt`, clippy `-D warnings`, `cargo test --workspace -- --test-threads=2`, goldens untouched.

## As built

(filled in when the work lands)
