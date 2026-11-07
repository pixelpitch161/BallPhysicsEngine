# Text thing, part 2: live words — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use checkbox (`- [ ]`)
> syntax. Project rule: one Opus review of the whole part before the PR, no review after every task.

**Goal:** words that change while the video plays. Values typed into the words as tokens (`{hits}`, `{countdown
30|m:ss}`, standings of the balls …), words shown letter by letter, word by word or line by line with appear and
disappear moves timed by seconds, beats or hits, Words keys that hold, a rule block that sets a Text's words, Texts
that ride balls (one copy per ball), and a fast lane that draws crowds of small letters from distance tiles. Five new
looks (Hit counter, Countdown timer, Lyrics on the beat, Labels on balls, Race leaderboard) and Hook title's pop-in.

**Architecture:** everything stays pure in time: a Text's words, its show and its riding are worked out at the moment
drawn from the world, the log and the rules state, then laid out (the layout cache keys the resulting string) and
placed. `bpe-kit/src/text/live.rs` parses and fills tokens; `things/text/show.rs` works out each letter's appear and
disappear; `things/text/draw.rs` gains `TextAt` (what a Text is read with) and `text_copies` (one `TextNow` a ridden
ball). The fast lane is a fourth path kind in the existing paths pipeline: a letter's quad reads a signed-distance
tile (the brush-tile atlas, `Frame::mask_tiles`) instead of walking its outline, so every shape layer, the blend split,
z-order, the overlay and the light passes keep working. The rule block's state sits in `RulesState.texts`.

**Tech stack:** Rust, wgpu/WGSL, egui 0.37 + egui_kittest. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-07-text-thing-design.md`, section "Live words (part 2)" and "Looks by
part" (2). Read part 1's plan's "As built" first.

## Global constraints

- Nothing moves for scenes without a Text: physics goldens, sound goldens, `unlit_pictures` hashes,
  `paths_tests::shapes_draw_as_before` all stay as they are.
- A Text without tokens, Show All at once with Appear None, no rule text and not riding draws exactly as in part 1
  (the same `PathInstance`s byte for byte; a test pins it).
- Pure in time: nothing is kept between frames (the only caches are keyed by inputs: layouts, glyph outlines, glyph
  tiles), so scrubbing, the preview and export agree.
- Plain words in every label and help text; no new colours made up in the editor (the style test).
- Scene format stays 12; new settings are plain settings with defaults (old scenes read them as defaults).

## Decisions (each settles something the spec left open)

1. **Token grammar.** `{name args|format}` with case-insensitive names: `hits`, `hits of <thing>`, `balls left`,
   `balls left of <thing>`, `time`, `countdown <seconds>`, `beat`, `bar`, `counter <name>`, `health`, `health of
   <thing>`, `health percent`, `health percent of <thing>`, `ball number`, `ball name`, `standings <by> <rows>` and
   `standings <by> <rows> of <thing>`. `<by>` is `down`, `hits`, `health`, `size` or `speed` (the long forms
   "furthest down", "most hits" are read too). `<thing>` is a thing's name (case-insensitive, the first in tree order).
   Anything else in braces is shown as typed, braces and all. Names are used rather than ids because the owner reads
   and types the words; Insert value writes the current name.
2. **Formats.** After `|`: `0`s before an optional `.` give the least whole digits (leading zeros), `0`s after it the
   decimals (`|000`, `|0.00`); a format with `:` is a clock: `m:ss`, `m:ss.0`, `m:ss.00`, `h:mm:ss`. Defaults: `time`
   one decimal, `countdown` whole seconds rounded up (so 30 shows at the start and 1 until it ends), everything else
   whole numbers. A countdown with decimals or a clock rounds up at its last digit too.
3. **What each value reads** (at the moment drawn, `cx.time` the video clock):
   - `hits`: on a copy riding a ball, that ball's hits (`Bodies.hits`); otherwise every Hit event in the log up to now.
     `hits of X`: Hit events with X on either side (a group counts its balls' hits).
   - `balls left`: live balls of every thing with balls; `of X` of X only.
   - `time`: seconds; `countdown N`: `max(0, N − time)`.
   - `beat`: the beat within its bar, 1 … beats in a bar; `bar`: the bar, from 1 (`World.beat_seconds`,
     `beats_in_bar`, the same clock as `beat_at`). No tempo: 0.
   - `counter N`: the rules' counter (`RulesState::counter`), 0 without rules.
   - `health`: on a riding copy the ball's health points, else the Text's tree parent's (a thing with a Health
     card); `health of X`: X's points (a group: the sum over its live balls); `health percent`: the share × 100. No
     Health card: `-`.
   - `ball number` (1-based `Bodies.number + 1`) and `ball name` only on a riding copy; elsewhere `-`.
4. **Ball names** are the nearest of 13 colour names (Red, Orange, Yellow, Lime, Green, Teal, Cyan, Blue, Purple,
   Pink, White, Grey, Black; by OKLCH hue, chroma and lightness) of the ball's own colour (`balls::draw::ball_colors`
   through a `DrawCx` for the ball's thing), capitals as typed by the Case setting. Two live balls of a thing with the
   same name get their numbers after it ("RED 2"). A ball thing with no Look card names its balls "Ball N".
5. **Standings** expand to one line per ball, best first, at most `<rows>` lines (1 … 50): `<place> <name> <value>`
   ("1st RED 7.2m"). Values: down = how far below its start in metres (100 px = 1 m, one decimal, "m"), hits (whole),
   health (points), size (radius px), speed (m/s, one decimal). Ties keep thing-then-row order. **Re-sort:** rankings
   are taken on a grid every Re-sort time R (a Text setting, 0.1 … 1 s, default 0.4); between grid moment g and g + R
   each line glides from its place at g − R to its place at g (smoothstep). A ball new to the top rows comes up from
   below the list; one that leaves it goes at once. Values at a past moment: hits exact from the log; positions from
   the ball thing's history when it has one covering the moment, else estimated as `p − v·Δ`; health and size as now.
   A Text whose words hold a standings token asks for 2 s of history on the ball things it reads
   (`TextSim::history` returns Some so the world fingerprint changes and the editor rebuilds; `TextSim::build` calls
   `World::keep_history` on those things). The lines move per paragraph, so wrapping doesn't break it
   (`Letter.paragraph`, new).
6. **Show** (settings group "Show"): Show All at once / Letter by letter / Word by word / Line by line; Appear and
   Disappear None / Pop / Fade / Slide / Drop / Typewriter; Takes (s, how long a move lasts, default 0.35); Timed by
   Seconds / Beats / Hits; Starts at (s); Each takes (s, Seconds); Every (beats, Beats, default 1); Hits of (a
   `Target`, Hits); Stays for (s, 0 stays). Unit k appears at: Seconds `start + k·each`; Beats the
   `(first + k·every)`-th beat where first is the first beat at or after Starts at; Hits the moment of the (k+1)-th
   Hit event at or after Starts at with the target on either side. All at once is one unit. A unit not yet appeared
   is not drawn; one past `appear + takes + stays` is gone after its Disappear move. Moves (t from 0 to 1 over Takes):
   Pop scale `back_out(t)` about the letter's middle; Fade opacity t; Slide up from 0.6 × size below, with fade;
   Drop from 1.2 × size above, `bounce_out(t)`, fade in over the first 30 %; Typewriter none (it shows whole at
   once). Disappear plays the same move backwards (Drop falls on down). Opacity is the instance's clip fade
   (`PathInstance.clip[2]`), scale and offset move the letter's points on the CPU.
7. **Rule text** (`do.text`, "Set text"): Who (default every Text: `Who::Kind("text")`), Do Replace / Add a line /
   Show the next line / Clear, To words (Replace, Add a line; may hold tokens). Applied at the next step's start into
   `RulesState.texts[thing]` = `RuleText { words: Option<String>, added: Vec<String>, nexts: Vec<f64> }`: Replace sets
   `words`, clears `added` and `nexts`; Add a line pushes onto `added`; Show the next line pushes the video time it
   applied at onto `nexts`; Clear sets `words` to "" and clears the rest. The words drawn are `words` (or the Text's
   own) with each added line after a line break. While `nexts` is not empty the Text shows one line at a time: line
   n (n = how many nexts, from 1) appeared at `nexts[n−1]` with the Appear move, and line n − 1 leaves with the
   Disappear move at the same moment, whatever the Show setting (past the last line nothing shows). Rule text is
   drawn only: it changes nothing physical, and nothing reads it but the draw.
8. **Riding** (settings group "Riding", read only when the Text's tree parent is a Balls, Emitter or Shape thing):
   Rides on Each ball (default) / The middle / Ball number, and Ball number. The Text's Position is the offset from what
   it rides; its Rotation and Spin turn it as before (it doesn't turn with the ball). Each ball gives one copy per live
   ball with its own tokens (`ball number`, `ball name`, `hits`, `health`). The middle is the mean of live balls' places
   (a shape's: its pose). A screen Text riding things is put where the camera shows them. Attach (Placement) is
   ignored while a Text rides this way (it would move it twice). A Text that rides nothing draws as before.
9. **Fast letters** (Words group, Automatic / Always / Never): Automatic uses the fast lane for a Text riding each
   ball or showing standings. A fast letter is `PathInstance` kind 3 (`b.y` = 3): `b.z` indexes a record in
   `path_points` = [tile centre x, y, turn, tile half size] in the text's frame, `b.w` the tile in
   `Frame::mask_tiles`; its clip is the tile square's four corners. Tiles are 128² `Rg8` like brush tiles: G the
   signed distance (`masks::tile_distance`, ± `TILE_DISTANCE_REACH` tile half-widths), R the cover; the glyph's
   outline at a canonical cap height of 100 px fills the middle 3/4 of the tile, so the distance reaches a sixth of
   the glyph's size past its edge. The shaders (`path_signed`, `shape_at`) read the distance there (`along` is 0), so
   all layers work; layers reaching further than the tile are faded out within it (`c.z` = min(past, reach)).
   Tiles are worked out on the CPU once a glyph (font id, glyph id, weight, slant) and cached; a frame names at most
   `MAX_MASK_TILES`, letters past that fall back to their outlines.
10. **Words keys hold:** `words` gets `keyframe, hold` (text values already hold between keys in `Value::lerp`). The
    preview's typing box starts from the words at the playhead and writes through `set_or_key`.
11. **Glyph outline cache:** a process-wide cache of merged glyph outlines keyed by (font id, glyph, weight, slant,
    scale bits), so a timer that changes every frame lays out only the shaping, not the outlines.
12. **The editor's words box** shows tokens with a tinted background (`style::TOKEN_BG`, a new token in `style/`)
    through a `TextEdit` layouter, and an **Insert value ▸** menu under it (Hits ▸ Everything / each thing; Balls left
    ▸ All / each thing with balls; Time; Time as a clock; Countdown; Countdown as a clock; Beat; Bar; Counter ▸ each
    counter the rules name; Health ▸ This ball / each thing; Ball number; Ball name; Standings ▸ Furthest down / Most
    hits / Health / Size / Speed). It inserts at the box's last cursor place (the end when it never had one).

## Files

| File | What |
| --- | --- |
| `crates/bpe-kit/src/text/live.rs` (new) | `Piece`, `Token`, `Format`, `RankBy`, `parse`, `has_tokens`, `reads_standings`, `format_value`, `LiveCx`, `fill` → `Filled { words, moves }`, standings, `colour_name` |
| `crates/bpe-kit/src/text/tiles.rs` (new) | `GlyphTile { hash, bytes, centre, half }`, `tile_for(font, glyph, weight, slant)` cached, the CPU distance field |
| `crates/bpe-kit/src/text/layout.rs` | `Letter.glyph`, `Letter.paragraph`; glyph outline cache; `glyph_outline` |
| `crates/bpe-kit/src/text/mod.rs` | exports |
| `crates/bpe-kit/src/things/text/mod.rs` | new settings (Show, Values, Riding, Fast letters), enums, `words` hold |
| `crates/bpe-kit/src/things/text/show.rs` (new) | `Showing`, unit moments, `letter_move` |
| `crates/bpe-kit/src/things/text/draw.rs` | `TextAt`, `text_now`, `text_copies`, riding places, fast lane draw, show moves |
| `crates/bpe-kit/src/things/text/sim.rs` | `history`, `build` keeps ball history for standings |
| `crates/bpe-kit/src/things/text/looks.rs` | five looks, Hook title pop-in |
| `crates/bpe-kit/src/rules/text.rs` (new) | `do.text` block |
| `crates/bpe-runtime/src/rules/{actions,mod}.rs` | `Action::Text`, `TextOp`, `RuleText`, `RulesState::text` |
| `crates/bpe-gfx/src/frame.rs` | `Frame::push_glyph`, `Frame::mask_tile`, `PATH_GLYPH` |
| `crates/bpe-gfx/src/shaders/paths.wgsl`, `shape_layers.wgsl` | kind 3 |
| `crates/bpe-studio/src/words_box.rs` (new) | the words box with token chips and Insert value |
| `crates/bpe-studio/src/widgets.rs` | `lines` unit → `words_box`; `TargetChoices.counters` |
| `crates/bpe-studio/src/preview/{text_handles,text_typing}.rs` | copies, typing keys |
| `crates/bpe-studio/src/style/` | `TOKEN_BG` |
| `CLAUDE.md` | Text part 2 paragraph |

## Tasks

### Task 1: layout additions
- [ ] `Letter` gains `glyph: u16` and `paragraph: u32` (filled in `lay_out`).
- [ ] Glyph outline cache in `layout.rs` (`OUTLINES: OnceLock<Mutex<HashMap<(u64,u16,u32,u32,u32), Option<Arc<..>>>>>`,
  cleared past 4096 entries); `outline` callers read through it.
- [ ] `pub fn glyph_outline(font, glyph, weight, slant, size) -> Option<Vec<Vec<Vec2>>>` (one glyph at a cap height).
- Verify: existing `text::layout` tests pass; a new test checks paragraphs and glyph ids.

### Task 2: tokens (`live.rs`)
- [ ] `parse`, `Format::parse`, `format_value(v, fmt, default)`; unknown tokens stay as typed.
- [ ] `LiveCx { tree, world, log, time, me, ball: Option<(ThingId, usize)>, names }` and `fill`.
- [ ] Values per decision 3; standings per decision 5 (`rank_at`, `standings_lines`, `moves`).
- [ ] `colour_name` per decision 4.
- Verify: unit tests for parsing (every token, formats, unknown braces), formatting (leading zeros, decimals, clocks,
  countdown rounding up), values in a small world (hits from a log, balls left, counter, beat/bar), standings order and
  the glide (place at g − R and g, smoothstep), colour names.

### Task 3: Show (`show.rs`)
- [ ] Settings and enums (decision 6), `unit_of(letter)`, `appear_times` (seconds, beats, hits), `letter_move(t)`
  → `Move { scale, offset, opacity }`, rule lines (decision 7).
- Verify: unit tests for each timing, each move's ends (t = 0 hidden or at its start, t = 1 whole), Stays for, rule
  next lines.

### Task 4: rule text (runtime + kit)
- [ ] `Action::Text { thing, op: TextOp }`, `TextOp::{Replace(String), AddLine(String), NextLine, Clear}`,
  `RuleText`, `RulesState.texts`, `RulesState::text(thing)`, applied in `apply_rule_actions` (video time from
  `World::time`). `lone` keeps them.
- [ ] `bpe-kit/src/rules/text.rs`: `SetText` block (`block_kind!`, `do.text`, "Set text"), registered in both lists.
- Verify: a rule scene (When Time 0.1 → Set text Replace "HI") reads `RuleText` after the step; Add a line, Show the
  next line, Clear; no golden moves.

### Task 5: riding, copies and the draw
- [ ] `TextAt` and `text_copies(at) -> Vec<TextNow>`; `text_now` = the first copy (None with none).
- [ ] Riding places (decision 8), screen mapping, per-copy `LiveCx.ball`.
- [ ] Show moves applied per letter (contours moved, clip fade), standings line moves.
- [ ] `TextSim::history` / `build` (decision 5).
- Verify: drawing tests: a riding Text draws one copy per live ball with its number; The middle; Ball number; a
  Text with no tokens draws exactly as before (instances equal to a part 1 build); Show letter by letter at moments;
  a Hits-timed show; a rule's Replace reaches the frame.

### Task 6: fast lane
- [ ] `tiles.rs` distance field (exact segment distance, even-odd inside) and cache.
- [ ] `Frame::mask_tile(hash, bytes) -> Option<u32>` (dedupes by hash, `None` past `MAX_MASK_TILES`),
  `Frame::push_glyph(center, angle, record, tile, margin, color, code) -> usize`.
- [ ] Shaders: kind 3 in `path_signed` and `shape_at` before the ellipse branch; `glyph_signed`.
- [ ] Draw: fast letters per decision 9.
- Verify: `bpe-kit/tests/text_pictures.rs` gains "the fast lane matches the full draw closely" (mean channel
  difference under 3 % and the cover within 2 % at 40 px letters) and a fast letter's hole is empty;
  `paths_tests::shapes_draw_as_before` unchanged.

### Task 7: looks
- [ ] Hit counter (`{hits}`, big, Pop on a hit? no: plain, Anton 260 px, top), Countdown timer (`{countdown
  30|m:ss}`, Space Mono), Lyrics on the beat (four lines, Show Line by line, Beats, every 4, Pop in, Fade out,
  Stays for 1.8 s), Labels on balls (`{ball number}`, rides each ball, in the world, 36 px, black outline; inside the
  selected or first Balls/Emitter thing; where there is none, under the parent like any text), Race leaderboard
  (`{standings down 5}`, Space Mono, left aligned, top left); Hook title: Word by word, Pop, 0.12 s apart.
- Verify: `every_look_adds_a_text_that_resolves_cleanly` covers them; a test per new look's words and settings.

### Task 8: editor
- [ ] `words_box.rs` (decision 12), `TargetChoices.counters` (every `counter` param of the rule blocks), wired from
  `widgets.rs` for unit `lines`.
- [ ] Typing box starts from the words at the playhead and keys through `set_or_key`.
- [ ] Handles: `on_body` checks every copy.
- Verify: kittest: Insert value ▸ Time puts `{time}` in the words; the Show rows appear; typing into a keyed Text
  keys it.

### Task 9: finish
- [ ] `cargo fmt`, `cargo clippy --workspace`, `cargo test --workspace` (bpe-gfx with `--test-threads=2`), goldens
  unchanged in debug and release.
- [ ] CLAUDE.md paragraph; this plan's "As built".
- [ ] One Opus review, fixes, PR, merge.

## Not doing (this part)

- Walls, sound, health on a Text (part 3), letter moves and Across each letter (part 4), the experimental folds
  (part 5).
- A separate sprite pipeline for the fast lane (the paths pipeline already has the tile atlas bound).
- Keeping anything between frames for the standings glide (it is worked out from the grid each frame).

## As built

- **Tokens** (`text/live.rs`) as decisions 1 to 5. `has_tokens` and `standings_in` are cheap checks the draw, the sim
  and the editor share. Words without a `{` skip the parse entirely, so part 1's Texts lay out exactly as before.
- **Standings history:** `TextSim::history` returns 2 s when the words hold standings (so the world's fingerprint
  changes and the editor rebuilds), and `TextSim::after_build` (not `build`: the ball things may be built later in the
  tree) calls `World::keep_history` on every thing with bodies and notes their rows. Without a history covering the
  moment, `place_ago` works back along the velocity.
- **Show:** `shows_whole` also needs a still Appear, so a Pop on All at once still pops. The Typewriter move is the
  same as None per unit (the letter-by-letter timing gives the typing).
- **Rule text:** Show the next line lays out line n alone (`split('\n').nth(n − 1)`), appearing with the Appear move
  at `nexts[n − 1]`. The line before goes at once rather than leaving with the Disappear move: both lines laid out at
  once would sit one above the other, not in the same place. Past the last line nothing shows.
- **Draw:** `TextAt { tree, scene, params, world, log, id, time }` (`TextAt::new` without the resolved scene and log),
  `text_copies`, `text_now` (first copy). `TextNow.shown` holds each letter's `LetterMove`; `contours(k)` applies it,
  `bounds()` covers every placed letter shown or not (so the editor can grab a Text before it appears). One set of
  layer records covers every copy: drawn letters' outline points, or fast letters' box corners. A fast letter's layers
  reach at most its tile's reach (`c[2] = min(past, half × TILE_DISTANCE_REACH)`). A glyph with no tile (a space, the
  atlas full) falls back to its outline.
- **Overlay atlas:** a screen Text's tiles go into the overlay frame's own `mask_tiles`, which `FrameBuffers::upload`
  already uploads per frame, so nothing new was needed in the renderer.
- **Looks:** as Task 7. Labels on balls goes inside the picked thing when it is a Balls group or an Emitter, else the
  first in the tree, else under what was picked.
- **Editor:** `words_box.rs` (the layouter marks each `{…}` that reads as a value; the menu is "Insert value", plain
  text, since the default fonts lack ▸); the Riding group always shows (it only matters inside a ball thing).
- **Tests:** `text/live.rs`, `text/tiles.rs`, `things/text/{show,draw,looks}.rs` units, `rules/text.rs`;
  `tests/text_pictures.rs` gains `fast_letters_look_like_drawn_ones` (under 5% of lit pixels differ, at the edges) and
  `labels_ride_their_balls`; the studio's `insert_value_puts_a_value_in_the_words`,
  `a_text_shows_its_show_and_riding_rows`, `typing_into_keyed_words_keys_them_at_the_playhead`.
