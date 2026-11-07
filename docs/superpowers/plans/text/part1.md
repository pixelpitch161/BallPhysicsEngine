# Text thing, part 1: the Text thing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Project rule: one Opus review of the whole part before the PR, no review after every task.

**Goal:** A new **Text** thing: words in any of 52 built-in fonts or a loaded one, laid out straight, round a circle, on a wave or along a shape's edge, drawn letter by letter through the Shape's paths pipeline with the full Shape Look card, on the screen or in the world, edited in the inspector and typed straight into the preview, with six starting looks.

**Architecture:** `bpe-kit/src/text/` is a text engine (fonts → `TextLayout` → `arrange` → placed contours) shared by the draw and the editor. Each letter is one `PathInstance` (all sharing the text's centre, turn and layer records) whose point list holds several contours, flagged in `path_points[k].z/w`, and whose quad is held to the letter's box by the existing 4-corner `clip`. The Text's Look card is `look.shape`. No physics yet: the Text has no wall (part 3).

**Tech Stack:** Rust, wgpu/WGSL, egui 0.37 + egui_kittest, skrifa 0.44 (outlines, already used), **rustybuzz 0.20** (shaping, new), **i_overlay 9** (glyph overlap removal, new), **base64 0.22** (scene fonts, new).

**Spec:** `docs/superpowers/specs/2026-11-07-text-thing-design.md` (read it first; sections "The thing", "Fonts", "Layout", "Drawing (part 1)", "Editing", "Looks by part" are this part).

## Global Constraints

- Thing kind id `text`, name "Text", family "Text"; `sticks_to_screen()` true. Slots in part 1: Look only (`SLOTS = DEFAULT_SLOTS = [SlotId::Look]`).
- Default font `Anton`; fallback font IBM Plex Sans SemiBold (`things::balls::shapes::FONT`, built in).
- Fonts folder lookup order: `BPE_FONTS` env var, `<exe dir>/fonts/`, `<workspace>/assets/fonts/text/`.
- Size = capital-letter height in px. Curves flattened within 0.2 px at the drawn size; at most 2048 points a letter; at most `MOST_LETTERS` = 4000 letters a Text (the rest are left out).
- Scenes stay format 12: `Scene.fonts` is `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`, so files without fonts save byte-identically.
- Existing shapes must draw exactly as before (their points keep `z = w = 0`); every existing picture test and golden passes unchanged. No golden is re-blessed in this part.
- Never add built-in or example scenes; the looks only fill settings when picked from the Add thing menu.
- Editor: no colour literals outside `style/` (a test checks); every control named for kittest (`labelled_by`), pickers are `menu_button`s; follow CLAUDE.md's egui pitfalls.
- Plain-language UI words (the owner is not a programmer). `cargo fmt` (width 120) and `cargo clippy --workspace` clean.
- Run bpe-gfx lib tests with `--test-threads=2` in the cloud container (memory).

## Review Focus

1. **Characters the font lacks** (emoji, Cyrillic in Bebas Neue, a tab): left out with no crash and no box; the space after them still counts. Test in Task 3.
2. **Variable-font overlaps** (Inter, Montserrat at heavy weights overlap contours): no seam, no outline drawn through a letter's inside. Test in Task 3 (no two contour edges of a letter cross after layout).
3. **Missing fonts folder or a removed loaded font**: the text still draws in the fallback and the Font row says why. Tests in Tasks 1 and 7.
4. **A file that isn't a font given to Load font** (a renamed .txt, an empty file): refused with a plain message, scene unchanged. Test in Task 7.
5. **Degenerate layout input**: empty words, only spaces/newlines, Size 0 or negative, a wrap narrower than one letter, one word longer than the wrap, 20 000 characters: no panic, no hang, a sensible result (nothing, or one word per line, or capped at `MOST_LETTERS`). Tests in Task 3.

---

## File structure

| File | Responsibility |
| --- | --- |
| `assets/fonts/text/*.ttf`, `assets/fonts/text/licences/*.txt` | The 52 fonts (55 files) and their licences |
| `crates/bpe-core/src/scene_fonts.rs` | `SceneFont` (bytes, base64 serde), `Scene::add_font` / `remove_font` |
| `crates/bpe-kit/src/text/mod.rs` | The text engine's public face |
| `crates/bpe-kit/src/text/fonts.rs` | The built-in list, folder lookup, loading and caching fonts, fallback |
| `crates/bpe-kit/src/text/layout.rs` | Shaping and outlines → `TextLayout` (cached) |
| `crates/bpe-kit/src/text/arrange.rs` | Path placement of letters at a moment |
| `crates/bpe-kit/src/things/text/mod.rs` | `TextKind`, `TextSettings` and its choices |
| `crates/bpe-kit/src/things/text/sim.rs` | `TextSim`: the pose (spin) |
| `crates/bpe-kit/src/things/text/draw.rs` | `TextDraw`, `text_now` |
| `crates/bpe-kit/src/things/text/looks.rs` | The six starting looks, `add_text` |
| `crates/bpe-gfx/src/frame.rs`, `shaders/paths.wgsl`, `shaders/shape_layers.wgsl` | Several contours a shape; `Frame::push_letter` |
| `crates/bpe-studio/src/font_picker.rs` | The font picker and Load font |
| `crates/bpe-studio/src/style/fonts.rs` | Text fonts as egui families for the picker |
| `crates/bpe-studio/src/preview/text_handles.rs` | Grab, size, wrap and radius handles |
| `crates/bpe-studio/src/preview/text_typing.rs` | Double-click to type in the preview |
| `crates/bpe-studio/src/tree.rs`, `inspector.rs`, `widgets.rs`, `look_editor/mod.rs`, `preview.rs`, `app.rs` | Wiring |

---

### Task 1: The fonts on disk and finding them

**Files:**
- Create: `assets/fonts/text/` (55 font files, renamed), `assets/fonts/text/licences/`, `assets/fonts/text/README.md`
- Create: `crates/bpe-kit/src/text/mod.rs`, `crates/bpe-kit/src/text/fonts.rs`
- Modify: `crates/bpe-kit/src/lib.rs` (`pub mod text;`)

**Interfaces:**
- Produces:
  - `pub enum FontGroup { BigAndBold, RoundAndFriendly, Clean, Elegant, TechAndRetro, HandDrawn, Wild }` with `fn name(self) -> &'static str` ("Big and bold" … "Wild") and `pub const ALL: [FontGroup; 7]`.
  - `pub struct BuiltIn { pub name: &'static str, pub group: FontGroup, pub files: &'static [(u16, &'static str)], pub weights: Option<(u16, u16)> }` — `files` is weight → file name (one entry for single and variable fonts, four for Poppins); `weights` is the variable axis range or the statics' span.
  - `pub const BUILT_IN: [BuiltIn; 52]`, `pub const DEFAULT_FONT: &str = "Anton"`.
  - `pub struct FontData { pub name: String, pub bytes: std::sync::Arc<[u8]>, pub id: u64, pub weight_axis: Option<(f32, f32)> }` (`id` = FNV-1a of the bytes, the cache key).
  - `pub enum Found { Font(Arc<FontData>), Fallback { wanted: String, font: Arc<FontData> } }` with `fn font(&self) -> &Arc<FontData>` and `fn problem(&self) -> Option<String>`.
  - `pub fn fonts_dir() -> Option<PathBuf>`; `pub fn find_in(dir: Option<&Path>, scene_fonts: &BTreeMap<String, bpe_core::SceneFont>, name: &str, weight: f32) -> Found`; `pub fn find(scene_fonts: &…, name: &str, weight: f32) -> Found` (= `find_in(fonts_dir().as_deref(), …)`); `pub fn fallback() -> Arc<FontData>`; `pub fn family_name(bytes: &[u8]) -> Option<String>`; `pub fn is_font(bytes: &[u8]) -> bool`.
  - (`SceneFont` comes from Task 2; do Task 2's bpe-core type first or stub `BTreeMap<String, Arc<[u8]>>` and switch over in Task 2.)

- [ ] **Step 1: Download the fonts and licences**

From `https://raw.githubusercontent.com/google/fonts/main/<path>`, saved under the name on the right (no brackets in file names):

```
ofl/anton/Anton-Regular.ttf                      Anton.ttf
ofl/bebasneue/BebasNeue-Regular.ttf              BebasNeue.ttf
ofl/oswald/Oswald[wght].ttf                      Oswald.ttf
ofl/archivoblack/ArchivoBlack-Regular.ttf        ArchivoBlack.ttf
ofl/bowlbyone/BowlbyOne-Regular.ttf              BowlbyOne.ttf
ofl/russoone/RussoOne-Regular.ttf                RussoOne.ttf
ofl/blackopsone/BlackOpsOne-Regular.ttf          BlackOpsOne.ttf
ofl/bungee/Bungee-Regular.ttf                    Bungee.ttf
ofl/sigmar/Sigmar-Regular.ttf                    Sigmar.ttf
ofl/rubikmonoone/RubikMonoOne-Regular.ttf        RubikMonoOne.ttf
ofl/varelaround/VarelaRound-Regular.ttf          VarelaRound.ttf
ofl/fredoka/Fredoka[wdth,wght].ttf               Fredoka.ttf
ofl/baloo2/Baloo2[wght].ttf                      Baloo2.ttf
ofl/nunito/Nunito[wght].ttf                      Nunito.ttf
ofl/lilitaone/LilitaOne-Regular.ttf              LilitaOne.ttf
ofl/titanone/TitanOne-Regular.ttf                TitanOne.ttf
apache/chewy/Chewy-Regular.ttf                   Chewy.ttf
apache/luckiestguy/LuckiestGuy-Regular.ttf       LuckiestGuy.ttf
ofl/ibmplexsans/IBMPlexSans[wdth,wght].ttf       IBMPlexSans.ttf
ofl/inter/Inter[opsz,wght].ttf                   Inter.ttf
ofl/montserrat/Montserrat[wght].ttf              Montserrat.ttf
ofl/poppins/Poppins-Regular.ttf                  Poppins-400.ttf
ofl/poppins/Poppins-SemiBold.ttf                 Poppins-600.ttf
ofl/poppins/Poppins-Bold.ttf                     Poppins-700.ttf
ofl/poppins/Poppins-Black.ttf                    Poppins-900.ttf
ofl/outfit/Outfit[wght].ttf                      Outfit.ttf
ofl/spacegrotesk/SpaceGrotesk[wght].ttf          SpaceGrotesk.ttf
ofl/playfairdisplay/PlayfairDisplay[wght].ttf    PlayfairDisplay.ttf
ofl/dmserifdisplay/DMSerifDisplay-Regular.ttf    DMSerifDisplay.ttf
ofl/abrilfatface/AbrilFatface-Regular.ttf        AbrilFatface.ttf
ofl/cinzel/Cinzel[wght].ttf                      Cinzel.ttf
ofl/shrikhand/Shrikhand-Regular.ttf              Shrikhand.ttf
ofl/spacemono/SpaceMono-Regular.ttf              SpaceMono.ttf
ofl/jetbrainsmono/JetBrainsMono[wght].ttf        JetBrainsMono.ttf
ofl/orbitron/Orbitron[wght].ttf                  Orbitron.ttf
ofl/audiowide/Audiowide-Regular.ttf              Audiowide.ttf
ofl/pressstart2p/PressStart2P-Regular.ttf        PressStart2P.ttf
ofl/vt323/VT323-Regular.ttf                      VT323.ttf
ofl/silkscreen/Silkscreen-Regular.ttf            Silkscreen.ttf
ofl/righteous/Righteous-Regular.ttf              Righteous.ttf
apache/permanentmarker/PermanentMarker-Regular.ttf PermanentMarker.ttf
ofl/caveat/Caveat[wght].ttf                      Caveat.ttf
ofl/pacifico/Pacifico-Regular.ttf                Pacifico.ttf
ofl/lobster/Lobster-Regular.ttf                  Lobster.ttf
apache/satisfy/Satisfy-Regular.ttf               Satisfy.ttf
ofl/bangers/Bangers-Regular.ttf                  Bangers.ttf
ofl/monoton/Monoton-Regular.ttf                  Monoton.ttf
ofl/bungeeshade/BungeeShade-Regular.ttf          BungeeShade.ttf
ofl/bungeeoutline/BungeeOutline-Regular.ttf      BungeeOutline.ttf
ofl/rubikglitch/RubikGlitch-Regular.ttf          RubikGlitch.ttf
ofl/rubikwetpaint/RubikWetPaint-Regular.ttf      RubikWetPaint.ttf
ofl/rubikpuddles/RubikPuddles-Regular.ttf        RubikPuddles.ttf
ofl/nosifer/Nosifer-Regular.ttf                  Nosifer.ttf
ofl/creepster/Creepster-Regular.ttf              Creepster.ttf
ofl/fascinate/Fascinate-Regular.ttf              Fascinate.ttf
```

(URL-encode `[`/`]` as `%5B`/`%5D`.) Each family's `OFL.txt` (ofl/) or `LICENSE.txt` (apache/) goes to `licences/<FileStem>.txt` (Poppins once). `README.md` lists the families, their licences and that the folder ships as `fonts\` beside the exes. Expected total about 11 MB, 55 font files. Check every file starts with `00 01 00 00` or `OTTO`.

- [ ] **Step 2: Write the failing tests** in `text/fonts.rs`:
  - `every_built_in_font_is_on_disk_and_loads`: for each `BUILT_IN` entry and each of its files, `find_in(Some(repo_dir), &empty, name, w)` is `Found::Font` and skrifa reads its cap height. `BUILT_IN.len() == 52`; names unique; every group has at least one font; the group order and members match the spec's table.
  - `variable_fonts_report_their_weight_axis`: Inter's `weight_axis` is `Some((100.0, 900.0))`; Anton's is `None`.
  - `poppins_picks_the_nearest_weight_file`: weights 300 → `Poppins-400.ttf`, 650 → 600 or 700 (nearest, ties up), 1000 → 900 (compare `FontData.bytes` with the files).
  - `a_missing_folder_falls_back_and_says_so`: `find_in(None, …, "Anton", 400.0)` is `Fallback`, its `problem()` is `Some` and mentions the fonts folder; an unknown name in a good folder is also `Fallback` with a "not found" problem.
  - `scene_fonts_win_and_are_found_by_name`: a scene font named "Mine" (bytes of `Anton.ttf`) is found as `Found::Font` with name "Mine" even with no folder.
  - `family_name_and_is_font`: `family_name(Anton bytes) == Some("Anton")`; `is_font(b"hello")` false; `is_font(&[])` false.
- [ ] **Step 3: Run** `cargo test -p bpe-kit text::fonts` — FAIL (module missing).
- [ ] **Step 4: Implement `text/fonts.rs`.** Loaded files are cached process-wide (`OnceLock<Mutex<HashMap<PathBuf, Arc<FontData>>>>`), scene fonts by `id`. `fonts_dir()` returns the first of the three places that holds `Anton.ttf`; the workspace place is `concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/text")`. `weight_axis` from skrifa's `axes()` tag `wght`. Problem words: "Font file not found: put the fonts folder beside ball-studio.exe" / "No font called \"<name>\"; showing IBM Plex Sans".
- [ ] **Step 5: Run** the tests — PASS. **Commit** `feat(text): the 52 built-in fonts and finding them`.

### Task 2: Fonts saved inside the scene

**Files:**
- Create: `crates/bpe-core/src/scene_fonts.rs`
- Modify: `crates/bpe-core/src/scene.rs` (field `fonts`), `crates/bpe-core/src/lib.rs` (export), `Cargo.toml` + `crates/bpe-core/Cargo.toml` (`base64 = "0.22"`)

**Interfaces:**
- Produces: `#[derive(Clone, Debug, PartialEq)] pub struct SceneFont { pub bytes: Arc<[u8]> }` (serde: a base64 string); `Scene.fonts: BTreeMap<String, SceneFont>`; `impl Scene { pub fn add_font(&mut self, name: &str, bytes: Arc<[u8]>) -> String; pub fn remove_font(&mut self, name: &str) -> bool }` — `add_font` returns the name used ("Name", else "Name (2)", "Name (3)" …); adding bytes identical to an existing font returns that font's name without a copy.

- [ ] **Step 1: Failing tests** in `scene_fonts.rs`: `a_scene_with_a_font_saves_and_loads_it` (save → load round trip through `save.rs` keeps the bytes; the saved text contains no long numeric array); `a_scene_without_fonts_saves_as_before` (a new scene's saved text has no `fonts` key); `adding_a_font_twice_names_it_once_and_a_new_one_gets_a_number`; `remove_font_says_whether_it_was_there`.
- [ ] **Step 2: Run** `cargo test -p bpe-core scene_fonts` — FAIL.
- [ ] **Step 3: Implement.** Switch Task 1's `find_in` to `&BTreeMap<String, SceneFont>`.
- [ ] **Step 4: Run** `cargo test -p bpe-core && cargo test -p bpe-kit text::fonts` — PASS. **Commit** `feat(core): fonts saved inside the scene`.

### Task 3: Laying text out

**Files:**
- Create: `crates/bpe-kit/src/text/layout.rs`
- Modify: `crates/bpe-kit/Cargo.toml` (`rustybuzz = "0.20"`, `i_overlay = "9"`)

**Interfaces:**
- Consumes: `FontData` (Task 1).
- Produces:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)] pub enum Align { Left, #[default] Centre, Right }`, `pub enum Case { #[default] AsTyped, Capitals, Lower, Title }` (labels "As typed", "CAPITALS", "lower case", "Title Case") — defined here, used by `TextSettings`.
  - `pub struct LayoutSpec<'a> { pub words: &'a str, pub size: f32, pub weight: f32, pub slant: f32, pub letter_spacing: f32, pub line_spacing: f32, pub align: Align, pub wrap: f32, pub case: Case }`.
  - `pub struct Letter { pub ch: char, pub index: u32, pub line: u32, pub word: u32, pub origin: Vec2, pub advance: f32, pub contours: Vec<Vec<Vec2>>, pub min: Vec2, pub max: Vec2 }` — contours relative to `origin` (the glyph's pen position on its baseline), px, y down; `min`/`max` the contour bounds in block space.
  - `pub struct TextLayout { pub letters: Vec<Letter>, pub min: Vec2, pub max: Vec2, pub line_step: f32, pub lines: u32 }`; `pub const MOST_LETTERS: usize = 4000; pub const MOST_POINTS: usize = 2048;`
  - `pub fn layout(font: &FontData, spec: &LayoutSpec) -> Arc<TextLayout>` (cached by font id + every spec field's bits + words; at most 256 entries, cleared when full, as `balls::shapes::word` does).

- [ ] **Step 1: Failing tests** (all with `fonts::find_in(repo_dir, …)`):
  - `size_is_the_capital_height`: "H" at size 100 in Anton: `max.y - min.y` within 1 px of 100.
  - `the_block_is_centred_on_the_middle`: "HI" centre aligned: `(min + max) / 2` within 1 px of the origin on x; on y, halfway between the cap top of line 1 and the last baseline is 0.
  - `kerning_is_applied`: "AV" in Inter is narrower than "A" + "V" laid out separately (rustybuzz kerning works).
  - `letters_count_skips_spaces_and_newlines`: "A B\nC" gives 3 letters with lines 0, 0, 1, words 0, 1, 2, indexes 0, 1, 2.
  - `letter_spacing_and_line_spacing_move_letters`: spacing 20 moves the second letter's origin 20 px right; line spacing 2 doubles `line_step`.
  - `align_left_and_right`: on "WIDE\nI", the second line's letter sits left / centred / right of the block.
  - `wrap_breaks_between_words_and_keeps_long_words_whole`: "one two three" with a wrap that fits one word gives 3 lines; a 1 px wrap gives one word per line, nothing lost; wrap 0 is one line.
  - `case_changes_the_letters`: Capitals of "abc" gives 'A','B','C'; Title of "hello world" gives 'H' and 'W' capitals.
  - `slant_leans_letters`: slant 20°: the top of "I" is further right than its bottom by about `cap × tan 20°`.
  - `weight_changes_a_variable_font`: Inter "I" at 900 is wider than at 100.
  - `holes_are_contours`: "O" in Anton has 2 contours; "B" has 3.
  - `overlaps_are_merged`: for "B8&@" in Inter at 700 and Montserrat at 900, no two edges of different contours of one letter cross (an O(n²) segment test).
  - `missing_glyphs_are_left_out`: "A😀B" in Bebas Neue gives 2 letters; "A\tB" has 2 letters.
  - `nothing_to_draw`: "", "   ", "\n\n" give 0 letters; size 0 and size −5 give 0 letters; no panic.
  - `a_huge_text_is_capped`: 20 000 'A's gives `MOST_LETTERS` letters in under 2 s (debug).
  - `points_per_letter_are_capped_and_fine`: no letter has more than `MOST_POINTS` points; at size 1000 every flattened curve point is within 0.2 px of the true curve (check one quadratic of "O" by sampling).
- [ ] **Step 2: Run** `cargo test -p bpe-kit text::layout` — FAIL.
- [ ] **Step 3: Implement `layout`.** Order: case → split lines on `\n` → per line, shape with rustybuzz (`Face::from_slice`, `set_variations(&[Variation { tag: wght, value }])` when the font has the axis, `UnicodeBuffer`, `shape`) → greedy wrap by words on shaped widths (a word wider than the wrap gets its own line) → outlines through skrifa `outline_glyphs()` at `LocationRef` with the same weight, scale `size / cap_height_units` (cap height from metrics, else 0.7 em, as `letter_outlines_in`) → flatten quads/cubics adaptively to 0.2 px (subdivide until the control polygon is within tolerance; cap at `MOST_POINTS`, raising the tolerance if needed) → merge contours with `i_overlay` (float API, non-zero fill rule; check the crate's docs for the exact trait, e.g. `SimplifyShape::simplify_shape(FillRule::NonZero)`) → shear by slant about the baseline → place by align and line step (`line_spacing × (ascender − descender + line gap)` at that scale) → centre the block. glyph id 0 (`.notdef`) and glyphs with no outline are not letters but keep their advance.
- [ ] **Step 4: Run** the tests — PASS. **Commit** `feat(text): laying text out`.

### Task 4: Paths: circle, wave, a shape's edge

**Files:**
- Create: `crates/bpe-kit/src/text/arrange.rs`

**Interfaces:**
- Consumes: `TextLayout`, `Letter` (Task 3).
- Produces: `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Placed { pub at: Vec2, pub turn: f32 }` (a letter's origin in the text's frame and its turn, radians clockwise); `pub enum PathAt { Straight, Circle { radius: f32, start: f32, inside: bool }, Wave { height: f32, wavelength: f32, shift: f32, follow: bool }, Edge { points: Vec<Vec2>, start: f32, gap: f32 } }` (`start` radians for a circle, 0..1 for an edge; edge points closed, in the text's frame, going clockwise from the shape's first corner); `pub fn arrange(layout: &TextLayout, path: &PathAt) -> Vec<Placed>`; `pub fn placed(letter: &Letter, p: Placed) -> Vec<Vec<Vec2>>` (contours turned by `p.turn` about the origin, then moved to `p.at`).

- [ ] **Step 1: Failing tests:** `straight_keeps_the_layout` (`at == origin`, turn 0); `a_circle_puts_letters_on_the_radius_facing_out` (every letter's baseline middle is `radius` from 0 within 0.5 px; the text's middle letter sits at `start`; turns increase clockwise; with `inside` the reading direction flips and letters face in); `later_lines_go_in_by_a_line_step` on a circle; `a_wave_follows_the_curve` (y = height × sin(2π x / wavelength + shift) at each letter's middle; with `follow` the turn matches the slope's angle, without it 0); `an_edge_runs_along_the_outline` (on a square of side 400 with gap 0 every baseline middle is on the square within 0.5 px; `start` 0.5 moves the text's middle to the far side; a positive gap moves letters outward); `degenerate_paths_do_not_panic` (radius 0, wavelength 0, an edge with fewer than 3 points: letters stay where Straight puts them).
- [ ] **Step 2: Run** `cargo test -p bpe-kit text::arrange` — FAIL.
- [ ] **Step 3: Implement.** A letter is placed by its baseline middle (`origin + advance/2` along the line); distances along the path are the straight layout's x; lines after the first are offset by `line_step` toward the circle's/shape's middle.
- [ ] **Step 4: Run** — PASS. **Commit** `feat(text): circle, wave and edge paths`.

### Task 5: Several contours in one shape, and `Frame::push_letter`

**Files:**
- Modify: `crates/bpe-gfx/src/frame.rs` (near `push_path`, ~line 1705), `crates/bpe-gfx/src/shaders/paths.wgsl` (`path_signed`, ~line 160), `crates/bpe-gfx/src/shaders/shape_layers.wgsl` (`shape_at`, ~line 36)
- Test: `crates/bpe-gfx/src/paths_tests.rs`

**Interfaces:**
- Produces: `pub const CONTOUR_END: f32 = 1.0;` and
  `pub fn push_letter(&mut self, center: Vec2, angle: f32, contours: &[Vec<Vec2>], margin: f32, color: [f32; 4], code: f32) -> Option<usize>` on `Frame`: pushes every contour's points (each contour's last point `[x, y, CONTOUR_END, (len − 1) as f32]`, others `[x, y, 0, 0]`), then the letter's box grown by `margin` (px) as 4 clip corners going round the same way `rules/remains/draw.rs` writes cells, and a `PathInstance` with `a = [center, angle, 1.0]`, `b = [0.0, 1.0, first, count]` (filled, kind 1), `c = [code, reach, 0, reach]` (reach = the farthest point from 0), `clip = [box_first, 4, 0, 0]`. Returns its index; a letter with no contours pushes nothing and returns `None`.

- [ ] **Step 1: Failing GPU tests** in `paths_tests.rs` (follow its existing harness):
  - `a_letter_with_a_hole_leaves_the_hole_empty`: a square ring made of an outer square and an inner square (two contours) through `push_letter`: a pixel in the hole is background, a pixel in the band is the colour.
  - `two_contours_do_not_join`: two separate squares in one letter: the pixel between them is background (no edge from the first contour's end to the second's start).
  - `a_letter_draws_only_in_its_box`: a letter's pixels outside its box plus margin are background, and with a Glow-like `c[2]` reach inside the margin the glow is kept.
  - `shapes_draw_as_before`: an existing polygon via `push_path` renders identical bytes to before the change (render once on main's code path and compare a stored hash, or compare against a reference drawn with the same frame minus the change — use the test file's existing reference style).
- [ ] **Step 2: Run** `cargo test -p bpe-gfx --lib paths_tests -- --test-threads=2` — FAIL.
- [ ] **Step 3: Implement.** In both WGSL loops, iterate edges from point `k` to `next(k)`: `next = k - u32(p.w)` when `p.z > 0.5`, else `k + 1`, and the list's last point wraps to the first as today when its `z` is 0 (so old shapes are unchanged). The `along` total in `shape_at` sums every contour (a letter's edge runs contour after contour).
- [ ] **Step 4: Run** the bpe-gfx tests (all, `--test-threads=2`) and `cargo test -p bpe-kit --test lit_pictures --test unlit_pictures --test glass_pictures` — PASS, no picture changed. **Commit** `feat(gfx): several contours in one shape, push_letter`.

### Task 6: The Text thing: settings, pose, drawing

**Files:**
- Create: `crates/bpe-kit/src/things/text/{mod.rs, sim.rs, draw.rs}`
- Modify: `crates/bpe-kit/src/things/mod.rs`, `crates/bpe-kit/src/lib.rs` (register kind, sim, draw; `pub use`), `crates/bpe-kit/src/modules/look/shape/mod.rs` (`fits`: shape or text), `crates/bpe-kit/src/starters.rs` (`SlotId::Look if thing == text::KIND => look::shape::CARD`)
- Test: in the new files, and `crates/bpe-kit/tests/text_pictures.rs` (GPU)

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - `pub const KIND: &str = "text";` `pub struct TextKind;` `pub enum TextPath { #[default] Straight, Circle, Wave, Edge }` (labels "Straight", "Round a circle", "Along a wave", "Along a shape's edge").
  - `#[derive(Clone, Debug, PartialEq, Settings)] pub struct TextSettings` with, in this order (key: label, group, attributes, default):
    `words`: "Words", "Words", unit `lines`, simple, "TEXT" · `font`: "Font", "Words", unit `font`, simple, "Anton" · `weight`: "Weight", "Words", range(100, 1000), keyframe, 400 · `slant`: "Slant", "Words", range(−30, 30), unit "°", keyframe, 0 · `size`: "Size", "Words", span(10, 1000), range(1, 20000), unit "px", simple, keyframe, 160 · `letter_spacing`: "Letter spacing", "Words", span(−50, 200), unit "px", keyframe, 0 · `line_spacing`: "Line spacing", "Words", range(0.3, 5), keyframe, 1.0 · `align`: "Align", "Words", Centre · `wrap`: "Wrap width", "Words", span(0, 2000), range(0, 20000), unit "px", keyframe, 0 · `case`: "Case", "Words", AsTyped · `spin`: "Spin", "Words", range(−360, 360), unit "deg/s", keyframe, 0 · `path`: "Path", "Path", simple, Straight · `radius`: "Radius", "Path", when `path=circle`, span(20, 2000), unit "px", keyframe, 300 · `start_angle`: "Starts at", "Path", when `path=circle`, span(−180, 180), unit "°", keyframe, 0 · `inside`: "Reads from inside", "Path", when `path=circle`, false · `wave_height`: "Height", "Path", when `path=wave`, span(0, 500), unit "px", keyframe, 60 · `wavelength`: "Wavelength", "Path", when `path=wave`, span(20, 3000), unit "px", keyframe, 600 · `wave_shift`: "Wave shift", "Path", when `path=wave`, span(0, 1), keyframe, 0 · `wave_speed`: "Wave speed", "Path", when `path=wave`, span(−5, 5), unit "per s", keyframe, 0 · `follow_slope`: "Letters follow the slope", "Path", when `path=wave`, true · `along`: "Shape", "Path", when `path=edge`, a `Target` (none by default) · `along_start`: "Starts at", "Path", when `path=edge`, range(0, 1), keyframe, 0 · `gap`: "Gap", "Path", when `path=edge`, span(−200, 200), unit "px", keyframe, 20.
    Plus `impl TextSettings { pub fn layout_spec(&self) -> LayoutSpec<'_> }`.
  - `TextKind`: id `text`, family "Text", name "Text", icon "text", help "Words on the screen or in the world, in any font, with the Look layers shapes have.", `sticks_to_screen` true, slots `[Look]`.
  - `pub struct TextNow { pub pose: Pose, pub layout: Arc<TextLayout>, pub placed: Vec<Placed>, pub font_problem: Option<String> }` and `pub fn text_now(tree: &Scene, params: &Params, world: &World, id: ThingId, time: f64) -> Option<TextNow>` in `draw.rs` — the one way the draw and the editor see a Text at a moment (`Edge` reads the target's wall through `shape::draw::wall_form` and moves its outline into the text's frame; `wave_shift + wave_speed × time` is the shift).
  - `TextSim`: `build` and `scripted` write `world.poses` like `ShapeSim` (anchor, plus `spin × sim_dt` accumulated in a `TextState { angle }`). No wall.
  - `TextDraw` (`behind` true): `draw` / `draw_over` / `blend` / `light` as `ShapeDraw` does for a non-mirror shape: `ShapeStack::of(look)`, records once from `Geometry { points: all placed points, ellipse: None, half: 0 }`, lighting `stamp`, `surface::Stamps` / body Lens, program, hits (`shape_hits` logic: Flash events and hits naming the Text), then one `push_letter` per letter with `margin = stack.reach_past() + 2.0`, `layers = [first, count, program, part]`, `c[2] = reach_past`, `hit`. Move `shape_hits` to `pub(crate)` and reuse it rather than copying. Around layers are not drawn for a Text.
- [ ] **Step 1: Failing tests:**
  - in `mod.rs`: `the_settings_are_in_their_groups_and_defaults_match_the_spec` (every key above, its group, its default); `a_new_text_starts_with_a_shape_look_card_and_on_the_screen` (`add_thing(.., "text", ROOT)`: one slot, `look.shape`, `resolve` has no problems, `ResolvedThing.screen` true).
  - in `draw.rs`: `a_text_draws_one_shape_per_letter` ("HI" → 2 paths sharing `a`, `layers`; 3 letters for "A B C"); `an_empty_text_draws_nothing`; `a_text_with_no_look_draws_nothing`; `the_records_are_shared_and_the_gradient_spans_the_word` (a Colour and gradient layer Across: its record's extent covers all letters); `a_screen_text_draws_into_the_overlay` (with `screen` true the paths are in `frame.overlay`, with false in the frame); `spin_turns_the_pose` (spin 90 deg/s, after 1 s of steps the pose angle is π/2 within 1e-3); `edge_path_follows_a_shape` (a Text along a circle Shape of radius 300 with gap 0: every letter's middle within 1 px of the circle).
  - in `tests/text_pictures.rs`: `a_letter_hole_is_empty_on_screen` (render "O" in white, the middle pixel is the background, a pixel on the stroke is white); `a_screen_text_does_not_move_with_the_camera` (camera moved 300 px: the screen text's pixels are unchanged, a world text's moved); `a_lit_text_casts_a_shadow` (Lighting card + Point light left of an "I" in the world: a pixel right of the letter is darker than one left of it); `glass_letters_bend_what_is_behind` (Surface Glass on "O" over a striped background: pixels inside the stroke differ from the same pixels with Surface Matte).
- [ ] **Step 2: Run** `cargo test -p bpe-kit things::text` and `cargo test -p bpe-kit --test text_pictures` — FAIL.
- [ ] **Step 3: Implement** the three files, registration and `fits`.
- [ ] **Step 4: Run** `cargo test -p bpe-kit` (goldens included: none move) — PASS. **Commit** `feat(text): the Text thing draws its words`.

### Task 7: The Words card in the editor: box, font picker, Load font

**Files:**
- Create: `crates/bpe-studio/src/font_picker.rs`
- Modify: `crates/bpe-studio/src/widgets.rs` (~line 1639, the `ValueType::Text` arms; `TargetChoices` gains `fonts: Vec<String>`), `crates/bpe-studio/src/style/fonts.rs` (`install` split so text fonts can be added), `crates/bpe-studio/src/inspector.rs` (~line 415 title: "Words" for a Text; edits from the picker), `crates/bpe-studio/src/look_editor/mod.rs` (the Body of a Text shows the hint "The words, font and path are in the Words card above."; the Add layer menu leaves out `shape.particles` for a Text), `crates/bpe-studio/src/tree.rs` (`FAMILIES = ["Bouncers", "Shapes", "Text", "Lights", "Particles"]`; the Text heading: "Empty text" then a "Looks" submenu, Task 9 fills it), the edit enum where `Edit::SetOwn` lives (new `Edit::LoadFont(ThingId, Arc<[u8]>)`, `Edit::RemoveFont(String)`).

**Interfaces:**
- Consumes: `fonts::{BUILT_IN, FontGroup, find, family_name, is_font}`, `Scene::add_font/remove_font`.
- Produces: `pub fn font_row(ui: &mut Ui, value: &mut String, cx: &FontCx) -> Response` in `font_picker.rs` (`FontCx { scene_fonts: &[String], problem: Option<String> }`); `pub enum FontRequest { Load(Vec<u8>), Remove(String) }` with `pub fn take_request(ctx: &egui::Context) -> Option<FontRequest>` (the picker leaves a request in egui temp memory; the inspector takes it after drawing and turns it into an edit); `pub fn text_family(name: &str) -> egui::FontFamily` (`FontFamily::Name("text:<name>")`) and `pub fn ensure_text_fonts(ctx: &egui::Context, scene: &Scene)` in `style/fonts.rs` (installs every built-in found on disk and every scene font as egui families on first need and whenever `scene.fonts` changes, keeping the four style families; missing files are skipped).
- [ ] **Step 1: Failing kittest tests** (in the files above, following `look_editor/mod.rs`'s harness style):
  - `the_add_menu_has_a_text_heading_with_empty_text`: clicking "+ Add thing" → "Empty text" adds a `text` thing, selected.
  - `typing_in_the_words_box_changes_the_words`: a Text's "Words" box takes "HELLO\nWORLD" (two lines kept).
  - `the_font_picker_lists_every_group_and_picks_a_font`: open "Font", see the seven group headings and "Bebas Neue"; typing "beb" in the search leaves Bebas Neue and hides Anton; clicking it sets `font` to "Bebas Neue".
  - `load_font_adds_it_to_the_scene_and_uses_it`: a `FontRequest::Load(Anton bytes)` taken by the inspector adds `scene.fonts["Anton"]` (named by its family; scene fonts are looked up before built-ins, so the name may match one) and sets the selected Text's font to it; one undo removes both.
  - `load_font_refuses_a_file_that_is_not_a_font`: `FontRequest::Load(b"not a font")` leaves the scene unchanged and shows "That file isn't a font the program can read (.ttf or .otf)."
  - `a_missing_font_says_so_on_the_row`: a Text with font "Nope" shows the fallback words under the Font row.
  - `the_body_of_a_text_points_at_the_words_card` and `a_text_cannot_add_a_particles_layer`.
- [ ] **Step 2: Run** `cargo test -p bpe-studio font_picker` (and the others) — FAIL.
- [ ] **Step 3: Implement.** The `lines` box is `TextEdit::multiline(..).desired_rows(3).desired_width(f32::INFINITY)`. The picker is a `menu_button` labelled with the current font name drawn in its own family (fallback: plex-sans plus "(missing)"); inside: a search box (`TextEdit::singleline`, named "Search fonts"), "Loaded" (scene fonts, each with a Remove button) then the groups in `FontGroup::ALL` order, each row the font's name in its own family at 18 px, then "Load font…" (rfd filter "Fonts" `ttf`, `otf`). Load names the font by `family_name`, else the file stem.
- [ ] **Step 4: Run** `cargo test -p bpe-studio` — PASS (the colour-literal test too). **Commit** `feat(studio): the Words card, font picker and Load font`.

### Task 8: In the preview: grab, size, wrap, radius, and typing

**Files:**
- Create: `crates/bpe-studio/src/preview/text_handles.rs`, `crates/bpe-studio/src/preview/text_typing.rs`
- Modify: `crates/bpe-studio/src/preview.rs` (`Handle` gains `TextSize(ThingId)`, `TextWrap(ThingId)`, `TextRadius(ThingId)`; `hit_test` (~line 865): text handles right after `shape_handles::hit_handle`, and text bodies with the walls at the end; `apply_drag`; `draw_handles`; double-click starts typing), `crates/bpe-studio/src/app.rs` (`Studio.typing: Option<text_typing::Typing>`)

**Interfaces:**
- Consumes: `bpe_kit::things::text::text_now` (Task 6).
- Produces: `text_handles::{hit(studio, view, pos) -> Option<Handle>, on_body(studio, view, id, pos) -> bool, draw(ui, studio, view, id), drag(studio, handle, world_pos)}`; `text_typing::{Typing { id, before: Scene, buffer: String }, start(studio, id), show(ui, studio, view)}`.
- [ ] **Step 1: Failing tests** (`preview.rs` test module style):
  - `clicking_a_text_selects_it_and_dragging_moves_it` (a world Text and a screen Text, each).
  - `dragging_the_corner_dot_scales_size` (corner at the layout's bottom-right; dragging it to twice the distance from the middle doubles Size; keyed Size writes a key at the playhead, as `set_or_key`).
  - `dragging_the_edge_bar_sets_the_wrap_width` (bar at the right edge; wrap = 2 × its distance from the middle on the text's own x; dragging it inside the narrowest word still keeps every word: Task 3's rule).
  - `the_radius_ring_shows_only_on_a_circle_path_and_sets_radius`.
  - `double_clicking_a_text_types_into_it`: double-click, type "NEW", press Escape: `words` is "NEW", and one undo brings back the old words (one undo step for the whole session); typing a newline with Enter keeps it (Escape or a click outside ends).
- [ ] **Step 2: Run** `cargo test -p bpe-studio text_handles text_typing` — FAIL.
- [ ] **Step 3: Implement.** Handles go through each thing's own view (`view.of(studio, id)`) so screen texts are grabbed where they show. Typing: an `egui::Area` at the text's screen bounds with a multi-line `TextEdit` (named "Type the words"), focused; each change writes `words` with the scene marked as recorded; ending calls `after_separate_edit(before)` once.
- [ ] **Step 4: Run** `cargo test -p bpe-studio` — PASS. **Commit** `feat(studio): move, size, wrap and type a Text in the preview`.

### Task 9: The six starting looks

**Files:**
- Create: `crates/bpe-kit/src/things/text/looks.rs`
- Modify: `crates/bpe-studio/src/tree.rs` (the "Looks" submenu, `Action::AddText(Look)`)

**Interfaces:**
- Produces: `pub enum Look { Empty, HookTitle, SpinningRing, NeonSign, GlassLetters, ShadowLettering, FlowingGradient }` with `name()` ("Empty text", "Hook title", "Spinning word ring", "Neon sign", "Glass letters", "Shadow lettering", "Flowing gradient") and `help()` (one plain sentence each); `pub const LOOKS: [Look; 6]` (all but Empty); `pub fn add_text(scene: &mut Scene, reg: &Registry, look: Look, parent: ThingId) -> Result<ThingId, EditError>`.

Each look (settings not named keep their defaults; layer settings are written through the layers' own keys, as `particles/looks.rs` does):
- **Hook title**: words "WAIT FOR\nTHE LAST\nONE…", Anton, size 120, line spacing 0.95, on the screen, position (0, −620). Look: Body white; an Outline layer black 6 px outside; a Shadow sub-layer on the Body black at 60 %, 8 px down, soft 6 px.
- **Spinning word ring**: words "ROUND AND ROUND • ROUND AND ROUND • ", Bebas Neue, size 70, letter spacing 6, path Round a circle radius 330, spin 30 deg/s, in the world. Look: a Colour and gradient layer of solid #FFD84A.
- **Neon sign**: "NEON", Monoton, size 200, in the world. Look: Body at opacity 0; an Outline layer #FF3DF0 6 px centred with a Gives off light sub-layer (strength 2); a Glow layer #FF3DF0. Gives the scene its Lighting card if it has none (as `add_thing` does for lights).
- **Glass letters**: "GLASS", Bowlby One, size 220, in the world. Look: Surface Glass, written the way the editor writes a Surface chip's preset in one edit. Lighting card as above.
- **Shadow lettering**: "SUN", Anton, size 260, in the world. Look: a Colour and gradient layer #FFE7B0 → #C47F34 Straight top to bottom. Adds a Point light (`light::add_light`) as a sibling, 300 px left of and 700 px above the text, warm white; Lighting card as above.
- **Flowing gradient**: "FLOW", Anton, size 220, on the screen. Look: a Colour and gradient layer with the Rainbow palette (looped Round the wheel), Straight, Cycle 0.3, Hit wave on.
- [ ] **Step 1: Failing tests:** `every_look_adds_a_text_that_resolves_cleanly` (each `LOOKS` entry: resolves with no problems, draws at least one path); `the_lit_looks_light_the_scene` (Neon, Glass and Shadow leave a Lighting card on the root; Shadow adds exactly one Light sibling); `each_look_sets_its_words_and_font` (table of the values above); kittest `the_looks_submenu_adds_a_hook_title`.
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test -p bpe-kit things::text::looks && cargo test -p bpe-studio` — PASS. **Commit** `feat(text): six starting looks`.

### Task 10: Export, docs and the whole check

**Files:**
- Modify: `crates/bpe-cli` (a test only), `CLAUDE.md`

- [ ] **Step 1: Failing test** in bpe-cli's tests: `a_scene_with_text_exports_a_frame` (a scene with an Empty text renders one frame through the export path with non-background pixels where the letters are).
- [ ] **Step 2: Run** — FAIL if anything is missing (fonts are found through `fonts_dir()`, so it should pass once Tasks 1–6 are in; fix what it shows).
- [ ] **Step 3: CLAUDE.md**: a "Text thing" section (spec and plans paths; `text/` engine: fonts lookup and `Scene.fonts`, `layout` (rustybuzz, skrifa, i_overlay, `MOST_LETTERS`), `arrange`; the Text kind and `TextSettings`; letters as `push_letter` instances with contour flags `z`/`w` and the 4-corner clip; `look.shape` fits text; `text_now`; the editor's font picker, handles and typing; the looks), and the crate map's `bpe-kit` line mentions `text/` and `things/text/`. Building: the exe needs `assets/fonts/text/` copied to a `fonts` folder beside it.
- [ ] **Step 4: Run the whole check:** `cargo fmt --check`, `cargo clippy --workspace`, `cargo test --workspace` (in the background; bpe-gfx lib with `--test-threads=2`), and the release goldens `cargo test --release -p bpe-kit golden`. Expected: all pass, no golden or picture hash changed (`git diff --stat` shows no `tests/golden*` change). Then the Windows exes: `cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli`.
- [ ] **Step 5: Commit** `docs: the Text thing in CLAUDE.md`.

## Delivering

- One Opus review of the whole branch before the PR (project rule); fix what it finds.
- The PR merges itself once checks and review pass (project rule). Installing waits until **all five parts** are merged (project rule), but whoever installs must also copy `assets/fonts/text/` (55 fonts + `licences/`) to `F:\!Ai Stuff\BallPhysicsEngine\bin\fonts\`: `device_commit_files` takes at most 50 files a call, so two calls; strip the exes first (25 MB limit).
- Update the plan with an "As built" section at its end for whatever differed.

## As built

Everything above is in. Where the build differs from the plan, or settles what it left open:

- **Contour flags.** A contour's first point has z = `CONTOUR_START` (2) and its last z = `CONTOUR_END` (1), both
  with w = the contour's length − 1, rather than flags on the first point only: `path_signed` walks backwards (it
  needs the start to find the end) and `shape_at` forwards (it needs the end to find the start). Old shapes keep
  z = w = 0; `paths_tests::shapes_draw_as_before` pins their picture by a hash made on the old shaders.
- **Choice labels** come from the variant names: Path is Straight / Round a circle / Along a wave / Along a shape
  (`when` strings `path=round_a_circle` …); Case is As typed / Capitals / Lower case / Title case.
- **Wave shift** is in waves (y = h·sin(2π(x/λ + shift))), so 1 is a whole wave and Wave speed is waves a second.
- **Along a shape** reads the first wall the Target names other than the text itself (default Anything = the first
  shape); circles and ellipses are cut into 256 points from the top, clockwise. Gap is measured from the outline's
  centre line. A Text gives no ripple sources (Ripples gradients ignore it) and no particles layers: the Look editor
  leaves the Trails and particles list out for a Text.
- **Looks.** A Text's Body has no settings and takes only the light and heat sub-layers, so Hook title's drop shadow
  sits on its white Paint layer, and Neon sign's dark tube is a dark Paint layer under the pink Outline (which gives
  off light) and the Glow, rather than a Body at opacity 0.
- **Load font.** `FontRequest::Load(bytes, file stem)`: the stem names a font whose file gives no family name. A
  refusal (`font_picker::NOT_A_FONT`) shows under the Font row until a font is picked or loaded. The inspector test
  checks that one frame both keeps the font and sets the text in it, which is one undo step in the studio.
  `fonts::problem_of(in_scene, name)` gives the Font row's warning without reading the file.
- **Editor fonts.** `style::ensure_text_fonts` installs every built-in found on disk and every scene font as egui
  families once a Text is selected, again when the scene's fonts change; `family_ready` keeps a name in the plain
  font until its family is there. The font menu stays open while the search box is typed in (CloseOnClickOutside).
- **Typing.** egui leaves an Area disabled on its first frame (it only sizes it), so the box takes the keyboard on
  the first frame it can, then ends on Escape or when it loses the keyboard. Each key re-resolves the scene with
  `Studio::scene_changed` and marks the frame recorded; ending records the session once with
  `after_separate_edit`.
- **Handles.** The Size dot and Wrap bar show on Straight (the dot on Along a wave too), the Radius ring on Round a
  circle; Size scales by the corner's distance from the middle; Wrap is twice the pointer's distance on the text's
  own x. Any Text is grabbed by the box round its letters, after the shapes' own handles and before the shapes' walls.
- The tree has a T icon for a Text (the icon test asks every kind for one).
