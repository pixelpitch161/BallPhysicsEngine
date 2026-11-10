# Colour grading part 6: grading tools (plan)

Spec: `docs/superpowers/specs/2026-11-09-colour-grading-design.md`, sections 4 (scopes strip, Snapshots under the masks,
Save / Load grade, J and Alt clipping, the targeted adjustment tool), 5 (Snapshots value, `.grade` files) and 6 part 6.
Handover: `ballphysics/colour-grading/part5-handover.md` (project files). Approved design; this is the last part.

What part 6 adds: **Waveform, RGB Parade, Vectorscope** next to the Histogram in a scopes strip under the preview;
**J** clipping and **Alt-drag** clipping views; the **targeted adjustment tool** on Tone Curve and HSL / Colour;
**Snapshots** (save, restore with undo, hover to preview, rename, update, delete); **Save grade… / Load grade…**
(`.grade` files). Nothing here changes an exported picture: every frame renders byte-identical to before.

## Decisions

1. **The scopes strip sits under the preview** (spec section 4 and mockup B's middle column), as an
   `egui::Panel::bottom("grade scopes")` drawn after Grade mode's side columns, so it spans only the middle (between
   the masks and the panels, above the timeline). Resizable, default 180 px, 120..360. Its first row holds chips
   **Histogram / Waveform / RGB Parade / Vectorscope** (`GradeMode.scope: ScopeKind`, default Histogram, not saved);
   under them the chosen scope fills the rest. The Histogram leaves the left column (part 1 put it there), which now
   holds the masks, the overlay switch, **Snapshots** and **Save grade… / Load grade…**.
2. **One GPU type for all four scopes.** `bpe-gfx/src/grade/scopes.rs`: `Histogram` becomes `Scopes` (same
   non-blocking read back: `encode` / `after_submit` / `take`, one reading at a time). `ScopeKind { Histogram, Waveform,
   Parade, Vectorscope }`; `ScopeCounts { kind, counts: Vec<u32>, size: (u32, u32) }`. Bins (`u32` each), by kind:
   - Histogram: `4 × 256` (red, green, blue, brightness), as before (`ScopeCounts::histogram()` gives
     `HistogramBins`).
   - Waveform: `256 levels × 256 columns`, index `level × 256 + column`; column `x × 256 / width`; level is the
     brightness `dot(c, (0.2126, 0.7152, 0.0722))` of the 8-bit sRGB-encoded picture, as the Histogram's.
   - RGB Parade: three waveforms, red, green then blue, index `channel × 65536 + level × 256 + column`.
   - Vectorscope: `256 × 256`, index `v × 256 + u`, from BT.709 on the encoded values: `Cb = (B − Y) / 1.8556`, `Cr =
     (R − Y) / 1.5748`, `u = bin(Cb + 0.5)`, `v = bin(Cr + 0.5)` (`bin(x) = clamp(round(x · 255), 0, 255)`).
   The buffers are sized for the largest (`3 × 65536` words); a reading clears and copies only its own words. One
   shader module `scopes.wgsl`, entry points `cs_histogram` (workgroup counts, as before), `cs_waveform`, `cs_parade`,
   `cs_vectorscope` (global atomics; at most ~2 M pixels in the editor's preview). One bind group layout (picture,
   bins with no minimum size).
3. **Renderer API**: `Renderer::request_scope(device, queue, kind)`, `scope_pending()`, `scope(device) ->
   Option<ScopeCounts>`, replacing `request_histogram` / `histogram_pending` / `histogram`. The editor ignores counts of
   a kind it isn't showing any more and asks again. Only the editor asks; an export never counts.
4. **Scope pictures** (`bpe-studio/src/grade_mode/scopes.rs`): when counts come in, the editor makes an egui texture
   (`ctx.load_texture`, linear filtering) from them: Waveform 256 × 256, Parade 768 × 256 (three side by side),
   Vectorscope 256 × 256 (Cr up). A bin's strength is `min(1, ln(1 + n) / ln(1 + r))` with `r` the 99th percentile of
   the non-empty bins (at least 1), so one big flat background doesn't wash the rest out. Inks (`style/video.rs`):
   Waveform `SCOPE_TRACE` (a light neutral token), Parade `CHANNEL_RED/GREEN/BLUE`, Vectorscope the video colour at
   that chroma (`style::video`, brightness 0.6), each times the strength (premultiplied, `gamma_multiply`). Kept in
   `GradeMode.scope_image` with its kind; drawn stretched over the strip (Waveform, Parade) or as the biggest centred
   square (Vectorscope). The Histogram draws as part 1 does.
5. **Graticules**, painted over the picture in `style::LINE_SOFT`: Waveform and Parade lines at 0, 25, 50, 75, 100%
   (labels "0", "50", "100" at the left in `FAINT`), Parade lines between its thirds; Vectorscope a circle at the
   edge, a cross through the middle, small squares at the 75% red, yellow, green, cyan, blue and magenta points
   (worked out with the same Cb / Cr maths, `vectorscope_point`) with their initials, and the skin-tone line from the
   middle at 123° (anticlockwise from +Cb). Each scope is a `Role::Image` named after it ("Waveform") for kittest.
6. **Clipping is drawn over the picture by the editor, not in the composite**, so the export, the scopes, the
   eyedropper and the targeted tool always read the clean picture. `bpe-gfx/src/grade/scopes.rs::Clipping`: a compute
   pass (`cs_clipping` in `scopes.wgsl`) reads the finished 8-bit picture and writes its own `Rgba8Unorm` storage
   texture of the same size, which the preview registers with egui once (re-pointed on resize) and paints over the
   picture with the picture's rectangle. A channel counts as at white when ≥ 254.5/255 and at black when ≤ 0.5/255
   (the composite keeps pure black and white exact, `post.wgsl`). `ClipView`:
   - `Marks` (**J**): red `(1, 0, 0)` where every channel is at white, blue `(0, 0.25, 1)` where every channel is at
     black, see-through elsewhere (Lightroom's J).
   - `Whites` (Alt): opaque; each channel 1 where it is at white, 0 otherwise (black where nothing clips, red where
     only red does, white where all do), as Lightroom's Alt view of Whites.
   - `Blacks` (Alt): opaque; each channel 0 where it is at black, 1 otherwise (white where nothing clips, black where
     all do).
   The pass runs after a redraw (or when the view or size changes) only while a view is on; mode in a 16-byte uniform.
   `GradeMode.clipping` (J's switch, toggled by **J** in Grade mode with nothing focused) and `GradeMode.alt_clip`
   (the Alt view, while held) decide it: the Alt view wins. The preview's corner gets a small "Clipping (J)" pill
   while J is on.
7. **Alt-drag**: Lightroom shows its clipping views while Alt drags Exposure, Highlights or Whites (whites view) and
   Shadows or Blacks (blacks view). In any Basic panel (`panels.rs::basic`), a change to one of those settings this
   frame while Alt and the primary button are held sets `alt_clip`; it stays while both are held and ends when either
   is let go (checked at the start of the column each frame), so a still pointer doesn't flicker it off. The status
   line says "Alt shows clipping". (Alt-click still resets a setting: a click isn't a drag.)
8. **The targeted adjustment tool** (`grade_mode/target.rs`). A **Target** toggle (painted target icon, kittest name
   "Targeted adjustment") in the Tone Curve's chip row and in HSL / Colour's tab row turns it on for that panel
   (`GradeMode.targeted: Option<LayerId>`); pressing it again, Esc, or the panel going away turns it off; only one panel
   at a time. While on, the preview's Grade branch gives presses to it first (after the Split line, before handles
   and brushes): the cursor is a vertical-resize arrow and a pill says "Drag up or down on the picture (Esc to
   stop)". A press reads the picture as shown at the press (`Renderer::read_output`, as the eyedropper) once; the drag
   then moves values from what they were at the press by how far the pointer went up (`dy` points; Shift a tenth as
   far). Each drag is one undo step (`own_step`); values go through `GradeEdit::Set` (keyed at the playhead when
   keyed). The colour under the pointer is the picture as shown (graded, or ungraded in Before), as the eyedropper
   reads it.
   - **Tone Curve, Parametric**: the pixel's brightness `x` (Rec. 709 weights on the encoded bytes) picks the region
     by the panel's splits (`maths::splits`): Shadows below the first, Darks, Lights, Highlights above the last; that
     slider moves `0.5 × dy`, clamped to −100..100.
   - **Tone Curve, Point**: the chip's channel curve (RGB uses the brightness, Red / Green / Blue their own byte). A
     point within 0.04 of `x` is taken (the nearest), else one is added at `(x, curve(x))` (`maths::point_curve`) when
     there is room (`MAX_CURVE_POINTS`, at least 0.01 from its neighbours; else the nearest is taken). Its y moves
     `dy / 300`, clamped to 0..1; x stays.
   - **HSL / Colour**: the pixel (decoded to linear, `maths::oklab`) gives an Oklab hue; `maths::band_weights` gives
     each band's share; the tab shown picks the setting (Hue, Saturation, Luminance; All moves Saturation, as
     Lightroom's default). Each band with weight > 0.01 moves `0.5 × weight × dy` from its own start, clamped.
9. **Snapshots are a value on the Grade card**: `bpe-core/src/saved_grade.rs` (new): `SavedGrade { params, layers,
   used_ids, tracks: BTreeMap<String, Track>, pulses: BTreeMap<String, Pulse> }` (the card's settings but `snapshots`,
   its masks, parts and panels, and the keyframes and pulses of its settings, by key on the card), `GradeSnapshot {
   name, grade }`, `GRADE_SNAPSHOTS_KEY = "snapshots"`, `Value::Snapshots(Vec<GradeSnapshot>)`,
   `ValueType::Snapshots` (valid when every number in it is finite and no snapshot holds snapshots). The Grade card's
   settings gain `snapshots: Vec<GradeSnapshot>` (label "Snapshots", never keyframed or pulsed, not shown in any
   grid). Keyframes and pulses belong to the whole grade, so a snapshot keeps them: restoring a snapshot of a keyed
   grade brings its keys back.
10. **Snapshot edits** (`bpe-kit/src/modules/grade/snapshots.rs`): `capture(scene) -> SavedGrade` (the card as it is,
    or the starting card), `put_grade(scene, reg, &SavedGrade)` (gives the scene its card, replaces its params but
    `snapshots`, its layers and `used_ids`, and every Grade key and pulse), `snapshots_of`, `add_snapshot` (named
    "Snapshot 1", "Snapshot 2"... the first free), `update_snapshot(i)`, `rename_snapshot(i, name)` (blank keeps the
    old name), `remove_snapshot(i)`, `restore_snapshot(i)`. `GradeEdit::{SaveSnapshot, RestoreSnapshot(i),
    UpdateSnapshot(i), RenameSnapshot(i, name), RemoveSnapshot(i), LoadGrade(SavedGrade)}`. A restore or load is its
    own undo step (`separate_edit`), and clears the editor's passing state (`painting`, `handle`, `renaming`, the
    targeted tool, curve and header drags); the selected mask stays when it is still there.
11. **Snapshots in the column**: heading "Snapshots", **Save snapshot**, then a row per snapshot (newest last). A
    click restores it; hovering shows it in the preview (`GradeMode.peek: Option<usize>`, set each frame by the
    column; the preview builds that frame from a copy of the scene with the snapshot put on, cached by (index,
    revision) in `GradeMode.peeked`); the preview's `Drawn` key holds the peek. Right-click: Rename (in place, like a
    mask's), Update ("Put the grade as it is now into this snapshot"), Delete. With none: "Save the grade as it is to
    come back to it."
12. **Grade files** (`.grade`): `GradeFile { format, grade: SavedGrade }` in RON, `format` the scene file's
    (`Scene::FORMAT`); `bpe_core::grade_file_text` / `read_grade_file` (plain-word errors: not a grade, newer, too
    old). Snapshots are left out of the file, and loading keeps the scene's own. Loading (`grade::load_grade_text`)
    cleans the card as a scene load does (`save::clean_card`, new pub: settings and layers), refuses a file without a
    Whole picture first, keeps only keys and pulses of settings the cleaned card has, then puts it on (one undo step).
    A Things part naming a thing this scene lacks shows "Thing not found" (part 5's note). **Save grade…** and **Load
    grade…** buttons open the system file picker (rfd, filter "Ball Studio grade"); a line under them says what
    happened ("Saved Warm dusk.grade.", or the problem in the problem colour).
13. **Fingerprints**: `snapshots` restarts nothing (no grade key does); `prints.rs` gets a `Snapshots` arm like
    `LightScenes`. `clip.rs` needs nothing (the Grade card is never copied with things).

## Files

- `bpe-core`: `saved_grade.rs` (new; types, validity, file text), `value.rs` (variant and type), `settings.rs`
  (`SettingValue for Vec<GradeSnapshot>`, `is_set`), `save.rs` (`pub fn clean_card`), `lib.rs` (exports).
- `bpe-runtime/src/world/prints.rs`: the `Snapshots` arm.
- `bpe-gfx`: `grade/scopes.rs` (`Scopes`, `ScopeKind`, `ScopeCounts`, `Clipping`, `ClipView`, `vectorscope_point`),
  `shaders/scopes.wgsl` (four counts and the clipping pass), `grade/mod.rs` (exports), `renderer/mod.rs` and
  `new.rs` (the scope calls), `grade/pictures_tests.rs` (tests).
- `bpe-kit/src/modules/grade/`: `mod.rs` (`snapshots` setting), `snapshots.rs` (new), `tests.rs`; `tests/restarts.rs`.
- `bpe-studio`: `grade_mode/mod.rs` (state, `scopes_strip`), `scopes.rs` (strip, chips, pictures, graticules),
  `target.rs` (new), `snapshots.rs` (new: the column's Snapshots and grade files), `masks.rs` (column without the
  Histogram), `panels.rs` (Alt views, Target toggle on Tone Curve), `colour.rs` (Target toggle on HSL), `cx.rs`
  (edits), `tests.rs`; `preview.rs` (scope counting, clipping pass and overlay, the targeted tool's press, the peek,
  `Drawn`); `app/mod.rs` (the strip), `app/shortcuts.rs` (J, Esc for the tool); `style/video.rs` (`SCOPE_TRACE`,
  scope inks); `widgets/words.rs`, `widgets/setting.rs` (the new value type, never shown).
- Docs: `docs/notes/grading.md` ("Part 6 as built"), this plan's "As built".

## Steps and how each is checked

1. **Core**: types, value, file text, `clean_card`. Tests (`saved_grade.rs`): a scene with snapshots saves and opens
   the same; NaN refused; a snapshot inside a snapshot refused; grade file text round-trips; a newer format and
   broken text give their words.
2. **GPU**: `Scopes` and `Clipping`. Picture tests (`test_device`): a half black / half white picture's waveform has
   all its counts at level 0 in the left columns and 255 in the right; a red picture's parade has red at 255 and
   green and blue at 0; a red picture's vectorscope lands on `vectorscope_point(red)`; the histogram test still counts
   a flat picture into one bin; clipping Marks paints white red, black blue and grey not at all; Whites and Blacks
   views as decided.
3. **Kit**: snapshots and grade files. Tests: capture and restore bring back layers, params, keys and pulses and keep
   the snapshot list; add / rename / update / remove; a grade file loads into another scene, drops unknown settings,
   refuses one without a Whole picture; `restarts.rs` checks the `snapshots` setting restarts nothing.
4. **Editor**: strip, clipping, Alt, targeted tool, snapshots, files. Tests (kittest and plain): the chips switch the
   scope; the scope picture's strength maths; J toggles the clipping view and Alt-drag on Whites shows the whites view
   until let go; the targeted tool's maths (a dark pixel moves Shadows, a point is added under the pointer and moved,
   a blue pixel moves Blue's saturation most); Save snapshot adds a row and a click restores it as one undo step;
   hovering a row peeks; Rename / Update / Delete from the menu.
5. **Whole**: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --
   --test-threads=2`, release goldens unchanged (nothing moves balls). No exported picture changes, so no full-size
   look scene is added; the grading look scenes render as before. One Opus review; PR; CI green; merge.

## Things not to do

- Don't draw scopes or clipping into the composite or any export; don't save the scope choice, J, the peek or the
  tool in the scene.
- Don't rebuild any grade pipeline for the tools; the scopes and clipping passes are made once per renderer.
- Don't keyframe snapshots or show them in a settings grid; don't put snapshots in `.grade` files.
- Don't block the editor waiting for scope counts (map without waiting, as part 1).
- Don't add built-in grades or example snapshots.

## Known limits (go in the notes)

- The mask overlay (O) is drawn into the picture, so the scopes count its red while it shows.
- A loaded grade's One thing part names a thing by its id in the scene it came from; in another scene that id may be
  a different thing (or none: "Thing not found").
- The targeted tool reads the picture as shown (after the grade), not the colour each panel sees.
