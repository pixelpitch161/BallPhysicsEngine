# Video arrangement, part 4: the On top lane. Implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task by task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A video gets an **On top lane** between the Scenes lane and the Song lane. Its clips draw over the main
lane's picture inside a frame (a rectangle placed on the screen, rounded corners, a border, an opacity, a fade in
and out), so a video can show picture in picture and split screens. A clip's inspector has layout shortcuts (Full,
Top half, Bottom half, Left half, Right half, the four corners, the four 2 × 2 slots). Its sound joins the video's
mix like a main clip, with a level, a pan and its fades. It works the same in the export (`export_arranged`,
`bpe-render-next`), the still frame and playing in Video mode.

**Architecture:**

- `bpe-core::video`: `Video.on_top: Vec<OnTopClip>`, `OnTopClip { clip: Clip, row: u32, place: Placement }`,
  `Placement { x, y, width, height, corner, border, border_colour, opacity, fade_in, fade_out }`, `Layout` and
  `Placement::with_layout`, `Clip.pan`. `Video::tidy` tidies the on-top clips too. Helpers that see every clip
  (`all_clips`, `clip`, `clip_mut`, `has_clips`), and `end`, `next_clip_id`, `unused_scenes_removed` count both lanes.
- `bpe-kit::video`: `on_top_at(video, frame) -> Vec<TopOnScreen>` (what the On top lane shows at a frame: which clip,
  its scene frame, its alpha after opacity and fades, in drawing order) and `top_size(place, out)` (the frame's size
  in pixels at an output size).
- `bpe-gfx::compose`: a second pass, `Compose::overlay`, that draws one finished picture into the composed output
  inside a rounded rectangle with a border and an alpha (blended over what is there), with its CPU twin
  `overlay_reference`; shader `shaders/compose_overlay.wgsl`.
- `bpe-cli::arranged`: the three drawing paths (`arranged_frames`, `still_frame`, `Player`) draw the on-top clips
  on screen with their own renderers (cover fit to the frame's pixel size), and put the frame together through one
  shared function `put_together` (main picture, then each overlay in order).
- `bpe-sound::video_mix`: on-top clips become plans like main clips (their fades are their own Fade in / Fade out,
  equal power; else the 5 ms edges), and every clip has a pan (a balance: `left × min(1, 1 − pan)`,
  `right × min(1, 1 + pan)`, so 0 is exactly as before).
- `bpe-studio::video_mode`: the On top lane in the arrangement (rows, drags, trims, drops), the on-top part of the
  clip inspector (row, layout chips, place, corner, border, opacity, fades, pan), "Move to On top" / "Move to the
  main lane", copy, paste, duplicate and delete on both lanes, hits and the Scene sound lane for both.

**Spec:** `docs/superpowers/specs/2026-11-24-video-arrangement-design.md` ("Later part: On top lane", "Parts" 4).
**Earlier parts:** `docs/notes/video.md`, `plans/video/part1-file-and-export.md`, `part2-arranging.md`,
`part3-playing.md`.

## Global constraints

- No physics or sound golden line may change. A video without on-top clips draws and sounds exactly as before:
  `arranged_frames` keeps reading a lone main clip straight from its renderer (same bytes as the scene's export);
  a pan of 0 multiplies by exactly 1.0.
- Physics code still uses `bpe_core::maths`. The only new maths in bpe-core/bpe-kit is arithmetic and `round`.
- Editor colours only from `style/` tokens; on-top clips use their kept scene's `TrackColour` (`// scene colour`).
  The border colour is the user's, picked with `color_edit_button_srgb` as the transition colours are.
- kittest: every control named (`labelled_by`), `query_all_by_label(..).next()` where several may match.
- Plain words. No demo videos or screenshots (project rule); the full-size look check is not run because no new
  look of a scene is drawn, only a frame put together from pictures the scenes already draw (the compose card test
  checks the pixels).
- Tests: only the new ones plus the existing tests of the files touched (below). `cargo fmt --all` and
  `cargo clippy --workspace --all-targets -- -D warnings` stay clean.

## Design decisions (made here, stated so the review can check them)

1. **On-top clips live in their own list, `Video.on_top`**, not in `Video.clips` with a lane field (the spec's
   sketch). Everything about the main lane leans on `clips` being one sorted lane whose neighbours make
   transitions (`overlap(i)`, `moment`, the mix's fades, the transition inspector, `set_overlap`); a second list
   leaves all of that untouched. An `OnTopClip` wraps an ordinary `Clip` (its `transition_in` is unused), so clip
   ids, the scene, start, length, offset, level, follow tempo and scene song work the same on both lanes, and the
   editor's id-based edits (`move_clip`, `trim_start`, `trim_end`, `delete_clip`) find a clip in either list.
   Clip ids are unique across both lists (`next_clip_id`).
2. **Rows.** The On top lane has up to three rows (`Video::ON_TOP_ROWS` = 3): enough for a 2 × 2 split (the main
   clip plus three on top). Clips on one row never overlap (tidy moves a later clip to where the one before it on
   its row ends); clips on different rows may. Higher rows draw above lower ones. The arrangement shows the rows in
   use plus one empty row to drop onto (at most three). `row` is clamped into 0..=2 on tidy.
3. **Placement** is in shares of the video's screen: `x`, `y` (top-left corner) and `width`, `height`, each 0 to 1,
   y down. Tidy clamps `width` and `height` to 0.05..=1 and `x` to `0..=1 − width`, `y` to `0..=1 − height`, so a
   frame is always wholly on screen. `corner` (radius) and `border` (width) are in pixels of the video at its own
   size (scaled with the export scale), clamped 0..=960 and 0..=200; the card clamps the corner to half the
   frame's shorter side. `border_colour` is sRGB bytes, white by default. `opacity` 0..=1. `fade_in`, `fade_out`
   in seconds, whole frames, each ≥ 0, and together at most the clip's length (both shrunk in proportion when
   longer).
4. **The default placement** of a new on-top clip is the bottom-right corner (`Layout::Corner(BottomRight)`), a
   border of 6 px, corner 24 px, opacity 1, no fades: picture in picture is the commonest use.
5. **Layouts** (`Layout`, pure, tested): `Full` (0, 0, 1, 1), `TopHalf` (0, 0, 1, ½), `BottomHalf` (0, ½, 1, ½),
   `LeftHalf` (0, 0, ½, 1), `RightHalf` (½, 0, ½, 1), `Corner(c)`: width and height 0.4 (the video's shape) with a
   margin of 4 % of the width on both sides (the y margin is the same in pixels: `0.04 × width ÷ height` of the
   height), `Quarter(q)`: the four 2 × 2 cells (top-left, top-right, bottom-left, bottom-right). Choosing a layout
   sets x, y, width, height only; corner, border, opacity and fades stay.
6. **How an on-top clip is drawn.** Its scene is fitted to cover the frame's own pixel size (`cover(scene size,
   frame size)`, the frame's size = `round(width × out width)` × `round(height × out height)`, each at least 2 and
   even), exactly as a main clip covers the video, and drawn with its whole renderer. The compose output starts
   as the main picture (the transition as before, or a lone clip through the plain crop, or black in a gap); then
   each on-top clip on screen, lowest row first, is drawn into it by the overlay pass.
7. **The overlay pass** (`compose_overlay.wgsl`, CPU twin `overlay_reference`): a full-screen triangle; for a
   pixel at `pos` (pixel centre): `half = rect size ÷ 2` in pixels, `p = pos − rect centre`, rounded-box distance
   `d = length(max(|p| − half + r, 0)) + min(max(|p|.x − half.x + r, |p|.y − half.y + r), 0) − r` with
   `r = min(corner, min(half))`. Coverage `a = clamp(0.5 − d, 0, 1)` (a one-pixel soft edge). Inside, the picture is
   sampled at `crop.xy + q × crop.zw` (`q` = the point's place in the frame, 0 to 1); the border mixes in with
   `b = clamp(0.5 − (−d − border), 0, 1)` when `border > 0` (border colour where `−d < border`). The colour is written
   with straight alpha `a × alpha`, blended `src × α + dst × (1 − α)` on colour and keeping the destination's alpha.
   Pixels outside are discarded (no write). Values are the output's bytes / 255 (the output is `Rgba8Unorm`), so the
   mix is in the video's colours, as the transitions are.
8. **Alpha** of an on-top clip at video frame `f` (`bpe_kit::video::on_top_alpha`): `opacity × in × out`,
   `in = min(1, (f − start + 1) ÷ (fade_in frames + 1))` when `fade_in > 0` else 1, `out = min(1, (end − f) ÷
   (fade_out frames + 1))` when `fade_out > 0` else 1, in whole frames. A clip whose alpha is 0 is still run (its
   physics goes on) but not drawn.
9. **Its sound.** An on-top clip is a plan in `VideoMix` like a main clip: its engine renders from 0 and throws
   its offset away; its fade in is equal power over `fade_in` (else the 5 ms edge when it starts partway, else
   none), its fade out equal power over `fade_out` (else the 5 ms edge). Opacity does not change the sound (the
   Level does). Plans are sorted by start across both lanes, so engines are made when their first sample comes up.
10. **Pan on every clip** (`Clip.pan`, −1 left to 1 right, default 0): a balance, `L × min(1, 1 − pan)`,
    `R × min(1, 1 + pan)`, applied with the clip's gain. At 0 both are exactly 1.0, so every existing mix is the
    same samples. The main clip inspector shows Pan too (it costs nothing and Aedan mixes).
11. **The lone-clip byte path stays.** `put_together` reads a lone main clip straight from its renderer only when no
    on-top clip is drawn at that frame; with any on-top clip drawn the frame goes through compose and overlays.
12. **Memory**: at most two main scenes plus one per row (five) are drawn at once. Each on-top renderer is only
    the frame's size (a quarter of the screen for a corner), so this is less than three more full pictures.
13. **The editor lane.** Each row is a strip 30 points high named "On top 1" (and 2, 3) with the clips in their
    scene colour and their name; a clip's middle drags to move it (and onto another row when the pointer goes over
    one), its edges trim as on the Scenes lane, double-click edits its scene, a click picks it. Snapping, Alt and
    the one-undo-step drag work as on the Scenes lane (`lanes::ClipDrag` gains nothing new: the row follows the
    pointer's row each frame from `before`). A kept scene dropped on a row makes an on-top clip there
    (`arrange::add_on_top`). With no on-top clips the lane shows one empty row reading "Drop a scene here for
    picture in picture".
14. **Moving between lanes** (`arrange::to_on_top(video, reg, id, row)`, `arrange::to_main(video, reg, id)`): the
    clip keeps its id, scene, start, length, offset, level, pan, tempo and song switches; to On top it gets the
    default placement on the first row it fits on at its time (else row 1, where tidy moves it along); to the
    main lane it gets a Crossfade in (as a new main clip does).
15. **Copy and paste** keep the lane: the clipboard is `Copied { clip, top: Option<(row, Placement)> }`; paste
    puts it at the playhead on the same lane (and row); Ctrl+D duplicates on the same lane just after it.
16. **Everything that counts clips counts both**: `Video::has_clips` (Play, the still frame, Export's "Add a clip
    first" and `NO_CLIPS`), `video_has_sound`, `clips_of`, `remove_scene`, the kept-scene list's counts, the
    video inspector's "N clips", `video_save::load`'s MissingScene check, the hits thread (each clip's hits in
    video time; ticks drawn for both lanes in the one Scene sound lane, in each clip's colour), and Back to video's
    pick.

## Files

- `crates/bpe-core/src/video.rs`: types, tidy, helpers, layouts, tests.
- `crates/bpe-core/src/video_save.rs`: MissingScene over both lists; save/load round trip test gains an on-top clip.
- `crates/bpe-kit/src/video.rs`: `TopOnScreen`, `on_top_at`, `on_top_alpha`, `top_size`, tests.
- `crates/bpe-gfx/src/compose.rs`, `crates/bpe-gfx/src/shaders/compose_overlay.wgsl`: `OverlayParams`,
  `overlay_params`, `overlay_reference`, `Compose::overlay`; `crates/bpe-gfx/tests/compose.rs`: card vs reference.
- `crates/bpe-cli/src/arranged.rs`: `put_together`, on-top drawing in the three paths; `crates/bpe-cli/tests/arranged.rs`:
  on-top tests.
- `crates/bpe-sound/src/video_mix.rs`: plans across both lanes, pan, on-top fades; tests.
- `crates/bpe-studio/src/video_mode/{arrange, lanes, inspect, mod, hits, play, still, edit, scenes, tests}.rs`.
- `docs/notes/video.md` (Part 4 as built), this plan's As built.

## Tasks

### Task 1: the data (`bpe-core`)

- [ ] `Clip.pan: f32` (default 0). `Placement` (defaults per decision 4), `Layout { Full, TopHalf, BottomHalf,
  LeftHalf, RightHalf, Corner(Corner), Quarter(Corner) }`, `Corner { TopLeft, TopRight, BottomLeft, BottomRight }`,
  `Placement::with_layout(self, layout, (w, h)) -> Placement`, `OnTopClip { clip, row, place }`,
  `Video.on_top` (serde default), `Video::ON_TOP_ROWS`.
- [ ] `Video::tidy`: main lane as before (the code is untouched apart from moving into `tidy_main`), then
  `tidy_on_top`: drop clips of missing scenes, clamp rows, sort by (row, start), per clip the same whole-frame
  limits as the main lane, no overlap on a row, inside `MAX_SECONDS`, placement clamps (decision 3), fades
  (decision 3), pan clamped to −1..=1 on both lanes.
- [ ] `all_clips()`, `clip(id)`, `clip_mut(id)`, `has_clips()`; `end`, `next_clip_id`, `unused_scenes_removed` over
  both lists. `video_save::load` checks both lists for a missing scene.
- [ ] Tests: on-top clips tidy (rounded, inside their scene, no overlap on a row, rows clamped, may overlap across
  rows, placement inside the screen, fades fit the clip), `end` and ids count both, layouts (each layout's
  rectangle; a corner keeps the video's shape; layout keeps the rest), save/load gives the same video with an
  on-top clip, a missing scene on the On top lane is refused.
  Verify: `cargo test -p bpe-core video` passes.

### Task 2: what is on top at a frame (`bpe-kit`)

- [ ] `TopOnScreen { top: usize, scene_frame: u32, scene_time: f64, alpha: f32 }`; `on_top_at(video, frame)` gives the
  on-top clips covering `frame`, in drawing order (by row, then start), alpha per decision 8 (those at alpha 0 are
  still listed: the drawers keep them running and skip the overlay); `on_top_alpha(video, top, frame)`.
  `top_size(place, out) -> (u32, u32)` per decision 6.
- [ ] Tests: on_top_at finds clips per row and frame with the right scene frame (offset counted); fades give 1/(n+1)
  steps and opacity scales; top_size of a corner at 270 × 480; order is by row.
  Verify: `cargo test -p bpe-kit --lib video::`.

### Task 3: the overlay pass (`bpe-gfx`)

- [ ] `OverlayParams` (uniform, whole vec4s: `rect` in pixels x, y, w, h; `crop`; `border_colour` rgba;
  `corner`, `border`, `alpha`, pad; `size`) and `overlay_params(rect_px, crop, corner, border, colour, alpha)`.
- [ ] `overlay_reference(params, pos_px, under, picture) -> [f32; 4]`, the CPU twin (decision 7).
- [ ] `compose_overlay.wgsl` and `Compose::overlay(device, queue, picture, &OverlayParams)` (its own pipeline, bind
  group layout and uniform, made with the rest in `Compose::new`; blending per decision 7; LoadOp::Load).
- [ ] Pure tests: inside is the picture, outside is the under colour, the border is its colour, a corner pixel is
  outside, alpha ½ is half way, crop picks the picture's part. Card test (`tests/compose.rs`): red under, blue
  picture, a rounded bordered half-alpha rectangle, every third pixel within 2/255 of the reference (pixels on the
  soft edges are compared too: both use the same distance).
  Verify: `cargo test -p bpe-gfx --lib compose` and `cargo test -p bpe-gfx --test compose`.

### Task 4: drawing on-top clips (`bpe-cli::arranged`)

- [ ] `fn top_scenes(video, kit) -> Vec<Scene>` (each on-top clip's `clip_scene`).
- [ ] `put_together(device, queue, compose, readback, out, main, tops)`: `main` = `Main { old, new, look, progress,
  lone }` (pictures as `(view, crop)`), `tops` = `[(view, OverlayParams)]`. Lone and no tops → `readback.push(renderer)`;
  else `compose.draw(...)` then `compose.overlay` for each top, then `push_texture`.
- [ ] `arranged_frames`: `top_draws: BTreeMap<usize, ClipDraw>` made on first show with the frame's size (exact
  run-up, as main clips), drawn every frame they're on (also at alpha 0), dropped after their end.
- [ ] `still_frame`: each on-top clip at the frame run up with `run_up` and drawn, then `put_together`.
- [ ] `Player`: `tops: BTreeMap<usize, LiveClip>`; `draw`, `skip`, `passed`, `warm_ahead` (the next clip of either
  lane within `LOOKAHEAD`), `all_warm` cover them.
- [ ] Tests (`tests/arranged.rs`): a video of a red-backed main scene and a blue-backed on-top clip in the top-left
  quarter: pixels inside the quarter are the on-top scene's, outside the main's, and the border colour shows; a
  frame before the on-top clip starts is byte-identical to the main scene's own frame; a fade halves the mix
  part-way; `still_frame` and `Player` give the same frame as `arranged_frames` within 2/255 per channel at a frame
  with an on-top clip (the quick run-up only differs for scenes that remember frames; these don't).
  Verify: `cargo test -p bpe-cli --test arranged`.

### Task 5: their sound (`bpe-sound::video_mix`)

- [ ] `ClipPlan` holds `clip: Clip` (a copy, no scene) instead of an index, and `pan: [f32; 2]`; `VideoMix::new`
  makes plans for both lists, sorted by start (stable, main first at the same start); `start` uses the plan's
  clip. On-top fades per decision 9. `video_has_sound` counts on-top clips.
- [ ] Tests: an on-top clip alone sounds like its scene (bare) times its level; a pan of 1 silences the left and keeps
  the right exactly; a pan of 0 changes nothing (the existing tests stay sample-exact); an on-top fade in of 0.5 s
  is equal power (gain at the middle ≈ sin(π/4)).
  Verify: `cargo test -p bpe-sound --lib video_mix`.

### Task 6: the editor

- [ ] `arrange.rs`: `clip`/`clip_mut` search both lists (`Video::clip`); `add_on_top(video, reg, kept, row, start)`;
  `to_on_top`, `to_main`; `delete_clip`, `remove_scene`, `clips_of` over both; `paste_clip(video, reg, copied,
  start)` and `duplicate_clip` keep the lane; `set_place(video, reg, id, place)`; `set_row`. Tests for each.
- [ ] `mod.rs`: `Copied` clipboard; `ArrangeView.top_rows: Vec<Rect>` (the On top rows as last drawn).
- [ ] `lanes.rs`: `on_top_lane` between Scenes and Song (decision 13), `hit_row` (edges then bodies on one row),
  drags reuse the Scenes lane's code path (one function `drag_clip` shared), a Body drag of an on-top clip follows
  the row under the pointer, a kept scene dropped on a row makes an on-top clip. The Scene sound lane draws ticks
  for on-top clips too.
- [ ] `inspect.rs`: the clip inspector reads the clip from either list; Pan on both; for an on-top clip an
  "ON TOP" section: Row chips (1, 2, 3), Layout chips (Full, Top, Bottom, Left, Right; the four corners; the four
  quarters), X, Y, Width, Height (in %), Corner, Border (px), Border colour, Opacity (%), Fade in, Fade out (s), and
  "Move to the main lane"; a main clip gets "Move to On top". The video inspector's count reads "N clips" over
  both lanes.
- [ ] `hits.rs`, `play.rs`, `still.rs`, `edit.rs`, `scenes.rs`, export window: `has_clips` / both lists as
  decision 16.
- [ ] Tests (`video_mode::tests`): dropping a kept scene on the On top lane makes an on-top clip (picked); dragging
  it moves it along and onto row 2; the inspector's Top half chip sets its place as one undo step; Move to On top
  and back keep the id; copy and paste of an on-top clip keeps its lane and row; Delete removes it; Play is allowed
  with only an on-top clip.
  Verify: `cargo test -p bpe-studio video_mode::`.

### Task 7: notes, review, PR

- [ ] `docs/notes/video.md`: "Part 4: the On top lane (as built)". This plan's "As built". CLAUDE.md unchanged
  (no new area or rule).
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] One Opus review of the whole part; fix what it finds.
- [ ] Draft PR, CI green (the tests that draw run on Aedan's card there), merge.

## Things not to do

- Don't touch `moment`, `Video::overlap` or the main lane's tidy rules.
- Don't add a lane field to `Clip`, or put on-top clips into `Video.clips`.
- Don't change the lone-clip byte path for frames without on-top clips.
- Don't add keyframes to the placement (a later idea: moving PiP).
- Don't build or install the exe, render demo videos or take screenshots.

## As built

Built as planned (notes in `docs/notes/video.md`, "Part 4"), with these differences:

- Decision 6: the frame's rectangle is `bpe_kit::video::top_rect`, whose edges are rounded to whole pixels so halves
  and quarters meet without a gap; its sides are not forced even (the on-top renderer is never read back on its
  own, and `cover` makes the drawn size even).
- Decision 14: a clip moved to On top with no free row at its time goes on the last row (row 3), where tidy moves
  it along.
- The On top lane keeps the rows it showed while a clip is dragged (`lanes::on_top_lane`), so the bottom panel
  doesn't grow under the pointer and move the rows.
- The inspector's Fade in and Fade out each stop at the room the other leaves; tidy's proportional shrink is for
  files only. Huge fades in a file are capped at the clip before they are summed.
- `arrange::set_row` was not needed (the drag and the inspector set the row and tidy).
- Review (one Opus pass): no serious findings; the fade drag, the overflow and the order of the inspector's two
  edits were fixed. Known and kept: on a row, a clip dragged or pasted in front of another pushes the later one
  along (the "later one moves" rule). The full-size look check wasn't run: it renders scenes, not videos; the
  overlay's edges are checked pixel by pixel against the reference on the card (`tests/compose.rs`).
- Tests run: `bpe-core` video, `bpe-kit --lib video::`, `bpe-gfx --lib compose` and `--test compose`,
  `bpe-sound --lib video_mix`, `bpe-cli --test arranged` (all 20), `bpe-studio video_mode::`. No golden changed.
