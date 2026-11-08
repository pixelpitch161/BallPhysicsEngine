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
  "+ Add slot" entry and jump-bar chip; each card's grid inside); the scene still keeps the two cards.
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

**Small links (W12, 2026-11-08).** Follow light lists Text things; Follow sound lists shapes, Texts and Particles
with sound, and the buses whenever anything has a voice. `widgets::target_picker` reads like the rules' who picker
(Anything, Itself, Thing ▸, Tagged ▸, Any ▸) and `target_words` says "Ball", "Tagged walls", "Any Shape". A Vec2 in
px/s (a ball's Launch, a Gobo drift) shows as **speed** and **angle** (`speed_and_angle`; written back as x/y only
when changed, a still pair's angle remembered in egui memory), and its words read "500 px/s at -53°". Every angle
unit is "°" or "°/s" (`tests::angles_say_degrees_one_way`). A shape's Look card has a **Moves** list.
