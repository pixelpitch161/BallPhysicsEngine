# The editor (bpe-studio) in detail

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

- `bpe-studio`: the egui 0.37 editor. Its look is the Ball Studio Design System (spec
  `docs/superpowers/specs/2026-11-03-design-system-editor-design.md`): `style/` holds every token, the IBM Plex
  fonts (`assets/fonts/`), track colours and drawn controls (switch, track picker, key diamonds). No other editor
  file may make up a colour (a test checks; a real scene colour line ends in `// scene colour`). Every thing but
  the scene settings has a `Thing.colour` (`TrackColour([u8; 3])`, any colour, saved as `"#rrggbb"` (older files with bare names open with new colours); a new thing gets a random readable one far from the hues in use, `Scene::new_colour`; editor only, never read by physics or export; picked with `style::track_picker` from the tree's tag and row menu, the timeline swatch and the inspector header). Its layout has the tree on the left, the inspector on the right, the preview
  in the middle and the timeline at the bottom. Key files are `app.rs`, `inspector.rs`, `preview.rs`, `timeline/`,
  `widgets.rs` (settings grids and the right-click value menu), `palettes.rs` and `export_window.rs`.
  **Grid snap** (`grid.rs`, toolbar Grid / Snap / spacing / angle step, saved in `grid-next.txt`): Snap snaps the
  preview's drags and, through `setting_snap.rs`, inspector settings while dragged (plan
  `docs/superpowers/plans/2026-11-07-snap-settings.md`): the app `put`s the frame's `SettingSnap` in egui memory,
  `Snapper::for_unit` reads it by the shown unit (`px` a length: whole squares, eighths under one; a `px` pair or
  Position/Offset a place; `°`/`deg` an angle step; not the Video size, nor a slider under four steps across,
  `widgets::setting_snapper`), Alt held is free (so Alt+click resets from the label only), typed numbers stay exact.
  `NumberFeel.snap` drives `span_slider` and `two_ended`; egui sliders and boxes snap after the fact (`after_drag`).
  Motion and Bounce show as one **Physics** card (`Card::Physics`, `PHYSICS` in `inspector.rs`: one header, Remove,
  "+ Add card" entry and jump-bar chip; each card's grid inside); the scene still keeps the two cards.
  A selected shape is moulded in the preview (`shape_handles.rs`: corners, sides, bend handles, a circle's edge,
  Shift mirrors across the shape's middle line, right-click Add point / Create curve / Straighten / Delete point /
  Turn into points); a `look.shape` card shows the Look editor without the ball view or Reactions, and its Body
  shows the shape's `BODY_HEADINGS` settings (changing Sides or Start from removes the path in the same edit).
  A `look.layers` card shows the Look editor (`look_editor/`: the ball view, the Layers and Reactions lists with
  their menus, drag to reorder and rename, a layer's sub-layers in one box under it (`layer_list::piece`: "+
  Sub-layer", fold arrow, drag within the box, row menu; `Edit::AddSubLayer`/`MoveSubLayer`/`PasteSubLayer`,
  `Inspector.folded_pieces`), then the picked layer's settings; the
  copied layer/look live in `Inspector.clipboard`). `layer_rows.rs` draws a cut's From/Layer and a Shader's
  Effect, `code_box.rs` the own-code box, `handles.rs` the handles on the ball view and `brush.rs` painting strokes
  there. A Balls thing's Look card starts with a Ball / Group switch (`Inspector.look_mode`; in the Balls card when there is
  no layered Look card): Group mode
  (`look_editor/group.rs`, `group_view.rs`) shows the whole group where it starts (custom balls, the launch arrow
  and the gradient's line drag there; balls can be picked for layers of their own), the shape chips and group
  settings, Across the group (the group layers: gradient and shaders) and folds for Variety, Release, Launch and
  Odd ones out. Those settings are left off the Balls card itself (`GROUP_HEADINGS`). An emitter's own card is the
  Emitter card (`emitter_card/`, a child module of `inspector.rs`): chips Emit | Catch | Marker, the mini view
  (`mini_view.rs`: rays, drag the square to aim, fan edges, ray ends for speed), the rounds strip (`strip.rs`, from
  `Inspector.rounds`, which the app fills from the playback) and the Sits on menu; its Look has the Ball / Group
  switch, Group mode showing one round laid out (`ViewCx::settings` stands it in as a custom group). In the
  preview a selected emitter shows its catch circle and start ring/line and snaps onto a circle's middle when
  dropped near it (`preview::seat_near`); the timeline ticks its rounds. While an emitter aims Through bounces,
  `aim_job.rs` (`AimJob`, `Studio.aim_job`) runs the scene to its end on its own thread, filling the aim cache; the
  playback never steps past it (`Studio::sim_target`) and the preview shows "Aiming… N%" (`Studio::aiming`) and the
  latest round's found paths. The
  timeline (`timeline/`) shows a Hits lane and a group per thing with keys; only the selected thing's group is open,
  over a bars-and-beats grid from the tempo (`beat_lines`, thinned under 4 px; keys snap to beats when the grid
  snaps). Sound: `audio_out.rs` opens the default device with cpal (`AudioOut::open`, `None` without one: the
  playhead then follows the wall clock) and feeds it from an rtrb ring (`Feeder`, resampling with rubato when the
  device isn't 48 kHz); `sound_job.rs` is the sound thread (`SoundJob`: Play, Pause, Seek, Patch, Rebuild; the
  editor never waits on it) and `SoundJob::clock` is the playhead while playing. A physics edit rebuilds the
  sound, anything else patches it; a seek clears the speakers' ring with a 2 ms fade-in. The transport has the
  speaker button and listening volume (`settings::ListenSettings`, never in the scene). The left panel switches
  Things / Mixer (`LeftPanel`; `mixer/`: the tempo row and `master::strip` with `style::Knob` knobs, the level
  meter, loudness readouts and the -14 LUFS graph from `style/meter.rs`). The Mixer is console strips
  (`mixer/strips.rs`: Bus 1..3 and a compact Master; effect slots with `slot_menu.rs`, drag between buses via
  `cx::move_effect`; `mixer/cx.rs` `MixCx`/`MixEdit` gathers edits like `TabCx`). A slot or "Master" sets
  `Mixer.pick` (`MixerPick`); the editor opens inside the Mixer under the strips, across the three bus columns in place of
  the sends and pan (the Mixer opens at the tree's width each time; `mixer/editors/`: `eq.rs` curve (shared with the chain EQ strip through `eq_curve.rs`), `comp.rs`, `delay.rs`, `reverb.rs`, `master.rs`, the rest from settings).
  Hovering a strip rings the shapes on that bus in the preview (`preview::plays_into`). The shape's Output
  panel picks Bus 1..3, has sends to the others and warns when a bus is off. A selected shape's inspector has
  Settings | Sound chips (`InspectorTab`, `Inspector.tab`); the Sound tab (`sound_tab/`) widens the inspector to
  its own panel id and saved width (760 px, `settings::load_sound_width`). `sound_tab/mod.rs` has `TabCx` (knobs
  with the value menu) and gathers `TabEdit`s applied after drawing; `write.rs` Enable/Disable/Copy/Paste sound
  (paste renumbers layers), `osc.rs` the Osc A/B panels with `wave_view.rs`, `mods/` the modulator row and editors,
  `adsr_view.rs` the draggable envelope and what it modulates, `keys.rs` the keyboard (FL names: C5 = MIDI 60; a
  click toggles the note and auditions it, also while stopped), `playing.rs` and `output.rs` (per-shape meter).
  Each oscillator's layer box is `chain_box.rs` ("+ Add layer" with Paste layer, drag a strip's row to reorder in
  its lane, `paste` keeps a Modulation's source when the card has it), `strip.rs` (a folded row: arrow, on switch,
  name and `layer_summary`, a 76×32 mini view, the Mix mini-knob, the loop mark; open: type, slope or quality chips,
  Solo "S", the 64 px view and the knobs; right-click Rename in place / Duplicate / Copy layer / Paste layer /
  Delete; open strips in `SoundTab.open_strips`) and `strip_views.rs` (filter response with its cutoff/reso
  handle, transfer curve, modulated wave). The copied chain layer is `Clipboard.chain_layer`.
  `notes/` is the Notes panel (part 5): `header.rs` (Key, Chord
  presets, octaves), `grid.rs` (a row per slot whose cells sit over the keys via `keys::KeyGeom`; gutter number and
  chord name, row menu, drag to reorder, out-of-range markers), `presets_menu.rs`, `mod.rs` (`NotesState`, the pure
  `apply_op`, the Move notes / Keep notes modal, Plays and Slots change). Copied notes live in
  `Inspector.clipboard.notes`; `TabEdit::OwnStep` makes an edit its own undo step. The timeline gives a shape whose
  slots take turns a group with slot-change ticks (`lanes::slots_take_turns`, `Timeline.slot_marks`), and hold
  keys have no ease menu.
  `sound_tab/mods/` is the modulator row (part 4): `row.rs` (tiles grouped by kind with a four-arrow handle that
  drags a `ModDrag` onto a knob, right-click Rename/Duplicate/Delete, **+**), `drag.rs` (the ghost and the "Can't
  modulate this" note, `check`, `on_knob`), `routes.rs` (the "<name> modulates" list: range in real units, Amount,
  Inv, ±, ×, and the settings that follow it), and the editors `distance.rs`, `lfo.rs` (the wide LFO page; `lfo_view.rs` draws and edits the drawn shape, `SoundTab.lfo_tool`), `hit.rs` (ADSR is
  `adsr_view.rs`). Knobs show a ring per route (`style::ModRing`, drag its end to set the amount, Alt for both
  ways) and a Modulate by submenu. `playing.rs` has the Plays chips; `preview/zones.rs` draws and drags the
  Distances' Near/Far lines while the Sound tab is open. **Follow sound** in the editor: `follow.rs` (`menu`,
  `FollowCx` on `GridCx.follow`, `now`, `set`, `followers`); a settings row's menu has Follow sound ▸ shape ▸
  modulator | Level, the buses, and Stop following sound; a following slider shows a band ("Follows …"); the
  pulse lane says what it follows. `Studio.follow` holds a `ControlSampler` for `build_frame_with` while any
  setting follows sound.

## Responsiveness (W6, 2026-11-08, plan `plans/2026-11-08-editor-responsiveness.md`)

- **One picture recipe.** `preview::compose` builds every preview frame: soloed lights (`lights_panel::soloed`),
  `follow` and `light_probe`, the World view's camera (`world_look`) and the Show bar (`show_light`). `draw_picture`
  and the effects' warm-up (`preview/warm.rs::draw_at`) both use it, and the warm-up lends the particle playback's
  particles to any replayed frame of the same step (in practice the playhead's own, the one swapped in; the replayed
  frames before it have no particles, since the particle playback only holds the playhead's moment). A warm-up
  restarts when the light show or the solo changes (`Seen`, `Warmup::light`).
- **Warm-up budget.** It replays frames for `WARM_BUDGET` (8 ms) per editor frame (at least one) and keeps the spare
  renderer `SPARE_KEEP` (2 s) after its last use, then drops it (`Shown::spare_used`, `warm::spare_due` asks for the
  repaint that drops it).
- **Held edits.** `Studio::hold_runs` is on while a pointer button is held anywhere but on a preview handle
  (`Preview::dragging`). `scene_changed` always re-resolves and bumps the revision, then `restart_runs` compares the
  physics, particle and sound fingerprints; held, a change only sets `runs_due` and the preview draws the old run with
  the new looks. `settle_runs` (end of each frame) restarts them when the button is let go or after `RUNS_REST`
  (100 ms) without a change. Preview drags restart every frame, so a dragged thing follows the pointer.
- **Seeking** (`Studio::seek`) only moves the playhead; the preview's `draw_picture` catches the playback up one
  `CATCH_UP_BUDGET` a frame. Tests use `Studio::catch_up`.
- **Recording edits.** `Studio::snapshot` is the scene as last recorded: the frame compares the scene with it once
  (`record_frame`) and copies the scene only when it changed. `after_edit`, undo, redo and `replace_scene` clear it
  (the next frame copies afresh). A playing frame with no input, nothing flagged and the same revision skips even the
  compare (`quiet_frame`). `unsaved()` is worked out once per revision (`unsaved_at`).
- **Undo size.** `History` keeps `Step { scene, bytes }` (`bytes` = the scene's RON length, counted by
  `history::scene_bytes` without writing the text); past `UNDO_BYTES` (256 MiB) in undo and redo, or `UNDO_LIMIT`
  steps, the oldest undo steps go, never the newest.
- **Look card views.** The ball view and the Group view keep a `ball_view::MiniCache`: the small scene is made each
  frame and compared; it is resolved and hashed (`content_key`) only when it changed, and the ball view keeps the
  `World` built from it (`build_view_frame_in` sets every ball's place, size, speed, life and hits afresh). Moving
  and Hit repaint every 33 ms with the window in front, 250 ms behind other windows.
- **Timeline.** The Hits lane's ticks are `timeline::HitTicks`, read once per hit as the log grows and again from the
  start only for a new revision, ruler, length or a shorter log. The slot marks follow a growing log at most every
  200 ms while playing (`SLOT_MARKS_EVERY`).
- **Particles** are lent to the preview's world without copying: `particle_job::Snapshot::states` holds
  `Arc<dyn ThingState>` and `Snapshot::lent` boxes the pointer (the runtime reads a boxed `Arc` as the state).
- **Shaders off the editor's thread.** `bpe_gfx::ProgramStore` also holds the lighting's pipelines
  (`lighting::LightKit`, with the Stage camera's `stage_camera_pass::StageKit`; `Lighting::with_kit` and
  `StageCameraPass::with_kit` make the per-renderer buffers and pictures) and the picture effects
  (`Programs::effect`, `effects::chain::Compiled` behind an `Arc`; `Chain::share`). A background store (the editor's
  `preview::shared_programs`) builds them on their own threads: a lit frame draws unlit and an effect is left out
  until ready, and `Renderer::programs_waiting` makes the views draw again in 50 ms. Exports and tests use the
  default store, which builds at once, so their pictures never change. Every editor view shares one store, so each
  shader is built once. While typing an effect's code, the call keeps drawing its last text that built until the new
  one is ready (`Chain::last_built`); finished builds of text no longer asked for are collected each time
  (`Programs::collect_effects`). A warm-up waits for the preview's shaders and starts over if the spare renderer
  meets one still building (it shares the preview's own store). A store's pipelines belong to one device: never
  share a store between devices (test devices give equal ids, so a mix-up isn't caught).
- **Known limit.** While a slider is held, the preview draws the old run's places with the new looks, so a look that
  depends on a physics setting (say a size) can look slightly off until you let go.

## Never lose work, exports and errors (W4, 2026-11-08)

Plan `docs/superpowers/plans/2026-11-08-never-lose-work.md` (findings E1 to E6 and C4 of the improvement review).

- **Status line problems** (`app.rs`): `Studio.status` is the grey info line; `Studio.problem: Option<Problem>`
  (`ProblemKind` Speakers, Graphics, File, Export, Recovery, Autosave) shows first in `style::PROBLEM` with a painted dot and a
  "Dismiss" ×, and stays until dismissed or its own source calls `clear_problem(kind)` (the speakers coming back
  clears only a Speakers problem; a good save or open clears File). Use `report(kind, text)` for any failure, never
  `status`. The shader-skipped readout is in the problem colour too.
- **Safe saves** (`files.rs`): `write_safely(path, text, backup)` writes `<name>.tmp`, `sync_all`s it, copies the old
  file to `backup_path` (`My scene (backup).ron`, opens from the Open dialog) and renames the temp file over. Used by
  `Studio::save` and the autosave.
- **Autosave and recovery** (`recovery.rs`): each windowed editor owns `session-<pid>-<millis>.{ron,path,lock}` in
  `settings_dir()/recovery/`, holding `.lock` with `File::try_lock` for its life (an unreadable lock counts as
  held; `tidy` skips locks under 30 s old). `Studio::autosave` (from `tick`, which wakes the window with
  `request_repaint_after` only while a copy is still owed) sends a clone to the session's worker thread on the first
  change and then at most every `AUTOSAVE_EVERY` (60 s) while unsaved; the worker reports each write (`outcome`), so
  an Autosave problem appears on failure and clears on the next success. The files are always cleared when the scene
  goes from unsaved to saved/opened/new (`was_unsaved`; the crash hook may have written a copy the autosave didn't),
  and on a clean exit (`on_exit`: when not unsaved or after "Don't save"; it empties the crash slot first). `recovery::orphans` lists
  sessions whose lock can be taken, newest first; `start_recovery` tidies dead locks and offers the newest orphan
  in the "Ball Studio closed unexpectedly" modal (Restore / Discard; when the scene's file is newer than the copy it
  warns and leads with Discard, `Offer.file_newer`); `restore` keeps the original file name, sets `saved` to what
  that file holds so the unsaved mark is honest, and deletes the old copy only after its own is written and flushed. A copy that won't load is renamed `.ron.broken` and
  reported. Older orphans wait for later starts. Headless studios (tests) never autosave unless a test calls
  `start_recovery` with a temporary folder.
- **Crashes and the graphics card** (C4): `catch_gpu_errors` sets the window device's `on_uncaptured_error` (into
  `Studio.gpu_errors`; `tell_gpu_errors` reports the first as a Graphics problem and afterwards only counts up in
  place, so a per-frame complaint can be dismissed and never hides another problem) and a device-lost callback
  (Destroyed ignored; triggers an immediate autosave). `recovery::install_crash_hook` wraps the panic hook: on the
  editor's own thread only, `crash_save` `try_lock`s `Studio.crash_slot` (refreshed at most every `CRASH_COPY_EVERY`
  = 1 s while unsaved), writes the session copy through its own `.crash.tmp` files and `last-crash.txt` in the
  settings folder, then the default hook runs. `files::write_via` retries the final rename a few times on
  PermissionDenied (OneDrive or an antivirus holding the file on Windows). The export and Preview final frame
  threads set their own device's error handler (a complaint stops the job with an error) and a job whose thread
  disconnects ends as "stopped unexpectedly" instead of looking busy forever.
- **Export** (`export_window.rs`): `progress_percent`, `take_news` (Saved / Failed / Cancelled, told by
  `tell_export_news`: "Video saved: x.mp4 (1:02 min)." or an Export problem). The status line shows "· Exporting 40%"
  (click opens the window). Closing while exporting asks "A video is still exporting (40%). Stop it and close?"
  (`Studio.ask_stop_export`, `stop_export_dialog`; Stop runs `shutdown` then the usual unsaved question).
  `explain_export_error` turns raw errors into a plain sentence (file in use, disk full, ffmpeg won't start, no H.264,
  out of memory, graphics card stopped, no adapter, stopped unexpectedly); the window shows it in the problem colour
  with a collapsed Details fold (scrolling, selectable log, Copy details). A target file that can't be opened for
  writing fails before the sound render. Choices (size, compression, encoder by `Encoder::key`, stems, MIDI, ffmpeg)
  are remembered in `export-next.txt` (`ExportWindow::load_in`, written when they change); not the file or Bake.
  `pretend_busy` fakes a running job for tests.

## Editor workflow (W5, 2026-11-08)

Plan `docs/superpowers/plans/2026-11-08-editor-workflow.md` (findings E7 to E14 of the improvement review).

- **Shortcuts** (`app.rs`): `shortcuts` (first in the frame) adds End (to the end) and Left/Right (a video frame;
  with Shift a beat, from `TempoMap`; `Studio::step`, pauses). `late_shortcuts` runs after the central panel, so a
  focused text box or the timeline has had the keys first: Ctrl+D (`duplicate_selection`), Ctrl+C / Ctrl+V (also
  egui's `Event::Copy` / `Event::Paste` / `Event::PasteImage`, which egui-winit sends instead of the keys): the
  timeline's picked keys first, else things. egui-winit sends Ctrl+V only when the system clipboard holds text or a
  picture, so every copy here also puts "Ball Studio: Copied …" there (`Studio.copied_note`, sent by
  `late_shortcuts`); Delete when the left panel isn't Things (the tree takes it itself there). Each is an own undo step
  (`separate_edit`). The window title is `window_title()` ("Name • – Ball Studio"; Untitled when blank), sent with
  `ViewportCommand::Title` only when it changes.
- **Copy, paste, duplicate things** (`bpe-core/src/clip.rs`, `bpe-studio/src/things.rs`): `Scene::copy_things` makes a
  `ThingClip` of the tops of the selection with everything under them, their tracks, pulses and the fonts and
  pictures they name; the scene settings and cameras are left out. `paste_things(clip, parent, after)` gives new ids
  (kept attachments only under the same parent), "X copy" names when taken, new track colours, relinks Target /
  Who / LightScenes / pulse sources (inside the clip to the copies; outside kept only if the same thing is there
  by id, kind and name, else Anything, counted in `Pasted.unlinked` and said in the status line) and renames
  clashing fonts and pictures. `Studio.clipboard` (`Clipboard::{Empty, Things, Keys}`) lasts through New and Open.
  The tree's row menu has Duplicate, Copy, Paste <what> and Delete with their shortcuts (`Button::shortcut_text`;
  kittest labels them "Paste Ball Ctrl+V"). Paste goes into a selected folder, beside another selected thing, else
  under the scene. "+ Add thing" sits at the top of the tree.
- **Timeline keys** (`timeline/mod.rs`): `Timeline.selected` is a set of `(Address, index)`; click picks one,
  Shift/Ctrl+click adds or removes, dragging on a lane's empty part draws a box (Shift adds), Escape clears.
  `picked_at` keeps each picked key's time as the timeline last drew; a key whose index now holds another time (a
  key added in front of it elsewhere) drops out, and undo/redo clear the selection. Dragging
  a picked key moves them all (`GroupDrag` keeps the tracks as the drag began; `Scene::move_keys` lays them out
  again each frame and settles at the end, keys put on others replacing them; never before 0). The key menu's Ease
  applies to every picked key, plus Copy N keys, Repeat every bar / every 2 bars (copies of the picked keys every
  bar, or every `floor(span / bar) + 1` bars so the first key's copy never lands on the last key, to the end of the
  video, at most `MOST_REPEATS`), Delete N keys. Copied keys (`KeyClip`, times from the first)
  paste at the playhead onto the open thing (the lane menu's "Paste N keys at the playhead", or Ctrl+V), onto the
  same settings where it has them.
- **Setting menu** (`widgets.rs::row_menu_following`): Reset to default (<value>) (greyed at the default) and Remove
  all keyframes (greyed with no keys; `Scene::clear_keys` keeps the value at the playhead). `GridOutcome.cleared` and
  `ValueMenu::RemoveKeys` carry it; every grid caller adds `Edit::ClearKeys` beside `Edit::NewKey`, and the Mixer,
  Master, Sound tab and Lights strip knobs call `inspector::clear_keys`. A widget's own double-click no longer
  resets (it starts typing in a number box); the label's double-click or Alt+click still does.
- **Pictures in the scene** (`bpe-core/src/scene_pictures.rs`, `picture_picker.rs`): `Scene.pictures` holds PNG bytes
  by name (base64 in the file, left out when empty; pictures no `picture` setting names are left out on save). The
  Picture row's "…" reads the file into a `PictureRequest`; the inspector checks the PNG signature, keeps it
  (`add_picture`, same bytes → same name) and sets every selected thing's Picture. Particles' `SpotsKey::Picture`
  reads the kept bytes first (an older scene's file path still works); `reads_tree` is true for a picture
  source so the world keeps the scene for it. A name that is neither kept nor a file
  shows "Picture not found" in the problem colour.
- **Empty preview and drops** (`preview.rs`, `drops.rs`): a scene with only its settings and cameras shows a hint in
  the middle of the preview; files held over the window show "Drop to open it". `Studio::take_drops` (after
  `late_shortcuts`): a `.ron` opens (asking to save first; the first scene only), a `.ttf` / `.otf` is kept with
  `add_font` and the selected Texts are set in it, a `.png` is kept and the selected Particles things (else a new
  one) form it (`from = a_word_or_picture`, `word_from = picture`); anything else is a File problem. egui 0.37's
  dropped files are `Arc<dyn DroppedFile>` (`path()`, `bytes()`); tests push their own.
- **Find a setting** (`inspector/find.rs`): a box above the jump bar. While it holds text, the cards and jump bar give
  way to every setting whose name or section holds every word typed, under its card's title (the thing's own, each plain card's,
  each layer's "Look · Glow"), with sections blanked so nothing folds away; rows edit, key and follow as in their
  card. Escape or Clear empties it. "+ Add slot" is now "+ Add card".

## Ready-made looks (E12, 2026-11-08)

Spec `docs/superpowers/specs/2026-11-08-ready-made-looks-design.md`, plan `plans/2026-11-08-ready-made-looks.md`. The
looks themselves are in `bpe-kit/src/ready_looks/` (`engine.md`).

- **Looks tab** (`looks_page/`): Balls and Emitters get Settings | Looks tabs, Shapes Settings | Looks | Sound
  (`inspector.rs::tabs`, `InspectorTab::Looks`). `Inspector::wide_open` (was `sound_open`) is true on a Sound or Looks
  tab, so the inspector takes the Sound tab's wide width. `LooksPage` (in `Inspector.looks`) keeps the family shown,
  the Find a look words, the look picked per wearer kind (`Wears`), Colours (The look's / Keep mine) and the line under
  the buttons. Three columns from 600 points wide (families, tiles, preview), else stacked.
- **Tiles** (`grid.rs`, `thumbs.rs`): a click picks a look; hovering shows it in the preview. Each tile is the look
  worn by the selected ball itself (a `ball_view::mini_scene` with the look applied, its trail moving when it has one)
  or, for a shape look, by a sample solid ellipse holding the shape's own Look first. One shared 168 px `Renderer`
  draws them, at most `PER_FRAME` (4) a frame, and each is copied into a texture of its own; a tile is drawn again
  only when its key (the thing's kind, Look card, Look keys and pulses, radius, the look and colours) changes, or its
  shaders were still building. Without a GPU (and until drawn) a tile paints the look's two swatch colours
  (`grid::swatch`, also used by the add menu).
- **Preview** (`preview.rs`, `shape_view.rs`): a ball look is the Look editor's ball view
  (`ball_view::show_dressed`, the small scene dressed with the look) with Still / Moving / Hit; a shape look is
  `ShapeView`, the shape alone on the dark ground with Still / Playing / Hit, framed by the path's `reach`. Use this
  look (Replace) and Add its layers on top (OnTop) put the *picked* look on the primary and every selected thing that
  can wear it (`ready_looks::fits`: the kind and a Look card), each with its own colours under Keep mine; each press
  sets `own_step`, so it is one Ctrl+Z of its own however soon it follows another edit. Keep mine is greyed when no
  thing reached has colours to keep (looks with colours of their own, shapes with no Colour layer); the buttons are
  greyed for a thing with no Look card. The page's scroll area has its own id (`"looks page"`, and the Sound tab's
  `"sound tab"`), so the tabs keep their own scroll places. A tile that can't be drawn is remembered by its key and
  not tried again every frame.
- **Add menu** (`tree.rs::with_looks`): Balls, Emitter and Shape are submenus: Plain, the seven favourites with their
  swatch (`Action::AddWithLook` → `ready_looks::add_with_look`), More looks… (`Action::AddForLooks`: a plain one,
  `TreeOutcome.looks`, and the app opens its Looks tab). Tree tests click the kind, then Plain.
