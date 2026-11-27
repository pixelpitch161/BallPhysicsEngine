# Water mode part 8: Make final and export, with slow motion (plan)

> **For agentic workers:** built natively in one thread (Aedan's project rule: one Opus review per part, no
> approval stops after the design). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Aedan presses **Make final** in a Water scene. The water is worked out once at Final detail in the
background and saved as a **take**; the Final view scrubs it, a look or camera edit redraws it at once, a physics
edit marks only the tail after it out of date, and **Export** writes an mp4 that is exactly what was scrubbed, from
the editor and from the command line. A **Speed** setting on the Water card, keyframed into ramps, plays a stretch
in **slow motion** (4 or 8 times the frames kept, so a splash plays slowly and smoothly).

**Architecture:** `bpe-water` gains what a take is made of: a body's state written to and read from bytes
(restart points), frames captured inside a frame's one submission at whole steps (slow motion), a pass that packs
each drop into 8 bytes on the card, the frame codec (planes, lz4) and chunk files, and a plain drawing that can move
drops on by their speed. `bpe-kit` builds the final's setup (`WaterSetup` at the Final cell), describes what the
physics reads so two setups give the earliest time they differ, maps water time to video time (Speed), keeps the
finals folder (find a take by print, or the best prefix of another, hard-linked), and runs the make loop.
`bpe-cli` draws a Water scene's video from a take (or from a draft run), and makes a missing final first from the
command line. `bpe-studio` runs Make final on its own thread and device, shows the Draft | Final switch, the Final
view, the Final lane, the status line, the Export window's water choices and Preview final frame, and plays the
playhead at the Speed.

**Tech Stack:** Rust, wgpu/WGSL (`bpe-water`), `lz4_flex` (new, pure Rust), egui 0.37 + kittest (`bpe-studio`).

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` (sections 3 "Same card, same result", 5 "Draft,
Final", 7 the editor's toolbar, status bar and Export, 8 the clip source, 9 Checks; Parts 8; "The water
checkpoint"). The checkpoint's slow motion item: "Make a stretch of the final with 4 or 8 times as many frames, so a
splash or a landing plays slowly and smoothly, and ramp the speed on the timeline. The water already picks its own
steps, so only the frames it keeps change." Hand-over: `/mnt/project-files/ballphysics/water-mode/
part8-handover.md`.

## Global Constraints

- Balls scenes byte-for-byte unchanged: no Balls golden or pinned picture moves; the plain drawing with nothing
  moved on (`ahead` 0) draws exactly as before (`golden-plain.txt` does not move).
- The physics is unchanged to the bit: capturing frames, saving restart points and slow motion change only what is
  kept, never the water. A final redone from a restart point equals one made in one go (spec section 9).
- An export of the final reads the saved take; it never re-runs the physics on its own. Only **Export the draft**
  (asked for by name) runs a draft.
- Editor colours only from `style/` tokens; every control named for kittest; no `ComboBox`; no glyphs the default
  fonts lack.
- `bpe-kit` refuses platform maths (`bpe_core::maths::Maths`).
- Windows-friendly (paths, hard links on NTFS, free space through the Windows call); cross-built with MinGW; the
  owner never runs commands; no exe built or installed.
- No sound in a Water scene (its export has a silent track as every export does). No demo videos or screenshots
  unless asked; the full-size look check is for what is drawn (the final drawn from a take).
- Picture tests through `bpe_gfx::test_device()`; no new pinned picture (the take's drawing is checked against the
  live body's, which needs no golden).
- Test only what this part adds or changes; CI runs the rest.
- Speed and size numbers told to Aedan as facts are measured on the RX 9070 XT (the card job's timing step); an
  estimate is called one.

## Decisions (my calls; cheap to change before they are built)

1. **Slow motion is the Water card's Speed** (group "Slow motion", after "Trapped air"): 1 is real time, 0.25 plays
   four times slower, 0.125 eight times; up to 4 plays faster (frames are skipped, nothing more is kept).
   Keyframed, so a ramp is two keyframes with an ease. It is not physics: it never restarts the draft, and the
   print does not hash it.
2. **The scene's timeline is the water's time; slow motion makes the video longer.** Every keyframe (physics, look,
   camera) and every Pour time stays in water seconds, so a pour still starts where its keyframe sits on the
   timeline; Length is how much water there is. A water second at Speed `s` plays for `1/s` video seconds, so the
   video is `V = ∫₀^Length dτ / s(τ)` long. The Export window, the status line and the Speed setting's hover say the
   video's length. Playing in the editor moves the playhead at the Speed (slow stretches play slowly, the draft too,
   one draft frame held over several screen frames). Why: a Balls-scene-like video timeline would make every
   physics keyframe move whenever the Speed changed, and Speed would become physics. Song sync happens in Video
   mode (part 10), whose timeline is the video's.
3. **What is kept.** A take keeps every water frame (the state at `k/60` s), and in a frame whose Speed is under 1
   it keeps `f − 1` more, evenly inside it: `f` is the smallest of 1, 2, 4, 8 at least `1 / s` (the least Speed at
   the frame's start, middle and end), capped at 8. At Final detail every frame is 32 steps (`Numbers::for_cell`:
   the floor at 3 px and finer is 32, never more than `most_steps` 32), so the in-between frames land on whole
   steps (every 16, 8 or 4 steps). A frame that takes another count puts in-between frame `i` at step
   `round(i · n / f)` and records its exact time.
4. **In-between frames come from the plain order.** The joined pass (`cs_finish_predict`) never writes the places
   mid-frame. At a step whose end is kept, the step ends with `cs_finish` and the next starts with `cs_predict`
   (the plain order, `Numbers::join` 0's), which C1 showed gives the same water to the bit
   (`the_joined_pass_changes_nothing`).
5. **A drop on disk is 5 bytes**, frames grouped by cell (the layout's cells, `floor` of the drop's grid place): a
   byte a cell for how many drops it holds (255 then two more bytes for 255 or more), then for each drop in cell
   order its place in its cell (x and y, a byte each: 1/256 of a cell, 0.008 px at 2 px), its speed (x and y, a
   signed byte each on a log scale: `±round(15 · ln(1 + |v| / 4 px/s))`, 7 % steps, 0 to about 20,000 px/s) and its
   kind's low byte (liquid and air). Stored as planes (counts, x, y, speed x, speed y, kind) and the frame
   compressed with lz4. Air drops are kept (part 6 draws bubbles from them). Colour, heat and age are not kept yet:
   the plain look colours by liquid; parts 4 and 6 add planes (the format number goes up, older takes count as out
   of date).
6. **The pack pass** (`cs_pack`, binding 29) writes each drop as two words on the card (its cell and kind, its
   quantised place and speed; a drop that left is marked gone); the host reads 8 bytes a drop back (not 48), sorts
   by cell (a counting sort) and writes the planes. Reading back waits for the card only a frame later (a ring of
   two), so the card steps the next frame while the host packs.
7. **Restart points every 2 s** (every 120 frames, frame 0 is rebuilt from the setup): a body's whole state as
   `Water::state()` gives it (the kept drops' three buffers, the deep pressure, walls and depth share, the clock's
   reports read back, the pours' progress, the counters), lz4-compressed. `Water::from_state` builds a body that
   plays on exactly as the copy `Water::copy` would.
8. **A take is a folder** in the finals folder named by the take's print (16 hex digits): `head.ron` (written to a
   `.part` file and renamed), `chunk-000012.bin` (water second 12: its 60 frames and their in-between frames, each
   with its time; written to `.part` and renamed, so a crash keeps every whole chunk), `point-000240.bin` (restart
   point at frame 240), `make.lock` while a make writes it (stale after 2 minutes without a chunk).
9. **The take's print** hashes the physics description (decision 10), the Final cell, the engine's print (the WGSL
   source, `TAKE_FORMAT` and `ENGINE_VERSION`, bumped by hand when the physics' host code changes) and the program's
   version. The head records the card and driver it was made on: a take made on another card or driver is not used
   (Make final makes a new one).
10. **What the physics reads, described** (`WaterInputs`, kept in the head): one part per thing the physics reads
    (the Water card's physics keys at Final detail, the course's strokes and solids, the Rock's Wetting, each
    Liquid's Feel, the taps by position, the fills), each with the time it can first change the water (`from`: 0,
    or a tap's Starts less three frames), a hash of its settings without keyframes, and each keyframed setting's
    keyframes as (time, hash of value and ease). **The earliest change** between two descriptions: a part that
    differs in its fixed hash, or is there in one and not the other, changes from its `from` (the earlier of the
    two); a keyframed setting whose keyframes differ changes from the keyframe before the first one that differs
    (from its part's `from` if the first differs, as the curve before it holds that key's value). Taps are compared
    by position in the list, so a removed tap changes everything from the earlier start of what moves into its
    place (the pours' progress is kept by position).
11. **Finding a take:** the take with this print (every made frame valid). Else the take with the same cell, card,
    engine and the latest earliest change against this description, if that is past its first restart point: its
    frames are valid up to the earliest change, and Make final starts a new take of this print whose chunks and
    points before the restart point at or before that change are **hard links** to the old take's (copies where the
    disk can't link), so nothing is made twice and the old take stays whole. Every find reads only the heads.
12. **Where the finals folder is:** `BPE_FINALS` if set, else `%LOCALAPPDATA%\Ball Studio\Finals` on Windows (local,
    not roaming: takes are big), `$XDG_CACHE_HOME/ball-studio/finals` or `~/.cache/ball-studio/finals` elsewhere.
    There is no Settings window yet, so moving it is Later. **Clear finals** (the toolbar's Finals menu) empties it;
    takes whose head was last used more than 30 days ago are removed when the editor starts (on a thread).
13. **Disk room:** before a make, the room it needs is estimated (frames still to make × the drops × the bytes a
    drop took on the last make, 3.5 before the first, plus a restart point of 48 bytes a drop every 2 s); a make
    whose estimate is more than the free space (`GetDiskFreeSpaceExW`, `statvfs`) less 2 GB is refused in plain
    words. A write that fails anyway stops the make with the disk's own words.
14. **The most drops a final holds is 8 million** (`FINAL_MOST`, spec "Caps"): past it its pours stop, as the
    draft's do at 500,000.
15. **One maker at a time.** The editor's **Make final and export** starts the editor's own make (if it isn't
    running) and the export once it is done; the export then only reads. The command line makes the final itself
    (with `make.lock`; a take locked by another maker is refused with words).
16. **The make runs on its own device** (`bpe_gfx::headless_device`), not the window's, so the editor's frames never
    queue behind a 1.4-million-drop frame; the Final view reads the take from disk on the window's device.
17. **The Final view** shows the take's frame nearest the playhead's water time, its drops moved on by their speed
    to the exact moment (`ahead`, at most half the spacing of the frames kept there); a moment not made yet shows
    the draft with a pill "Not made yet: showing the draft"; a made moment that is out of date shows the old final
    with a pill "Out of date: Make final remakes from 4 s". Frames are read and unpacked on a worker thread (the
    newest asked for wins); the Final view's field (the course at the Final cell) is built on a thread too.
18. **The Final lane** (Water scenes, on the timeline above the keyframe lanes, both views): made (solid), out of
    date (hatched), being made (a lighter stretch up to where the make is), not made (empty), and stretches kept in
    slow motion marked along its bottom edge. A click on it scrubs, as the ruler does.
19. **Export of a Water scene** draws the camera's shot at each video frame's water time from the take in the
    plain look (until part 6) at the video's size, 9:16 of the course at the Camera's Position and Zoom (the shot
    the stage shows), and puts a silent track on it. The Export window shows **Export** when the final covers the
    video, else **Make final and export** (with the time and room it expects) and **Export the draft**. The
    command line makes a missing final itself (`--finals DIR` picks the folder) or, with `--draft`, exports the
    draft.
20. **Preview final frame** of a Water scene draws the take's frame at the playhead at the export's size; a moment
    not made yet says so ("This moment of the final isn't made yet: press Make final, or see the draft on the
    stage.").
21. **Status line:** "· Final: 1.4M drops, 10 s of 10 made, 6.2 GB" (measured on the last make), "· Making the final:
    40 % · about 3 min left", or before any make "· Final: about 1.4M drops, about 7 GB (estimate)". The Video card's
    Length row adds "(video 34.2 s with slow motion)" when Speed is keyed.
22. **Video mode stays as it is** (part 10): a video holding a Water scene is still refused.

## Review Focus

- The physics must not move: the capture path (decision 4) and the restart points (decision 7) are checked bit for
  bit against a plain run and against `Water::copy`.
- A take is a cache: any doubt (another card, format, engine, a broken file) means "not made", never a wrong frame.
- What the Final lane calls valid must be what the physics would make again (decision 10's taps by position, the
  three-frame margin before a tap starts, a keyframe moved changes from the key before it).
- The editor never waits on the make or on the disk on its own thread.

## Tasks

### Task 1: restart points (`bpe-water drops/state.rs`)

**Files:** `crates/bpe-water/src/drops/state.rs` (new), `drops/mod.rs`, `drops/clock.rs`, `drops/deep.rs`,
`drop.rs`, tests in `tests/take.rs` (new).

- [ ] `pub struct WaterState` (bytes; `to_bytes()` / `from_bytes()`; a magic `BPESTAT1`): the counters (`count`,
  `room`, `serial`, `poured`, `steps`, `frames`, `now`, `edges_shut`, `deep_last`, `deep_dt`, `prime_next`,
  `deep_hold`, `deep_frames`), the clock's host side (`Clock::state()` / `Clock::set_state()`: every report read
  back, so its `recent` entries are all Known; `let_go`, `added`, `settled`, `reported`, `spouts`, `deep`, `air`,
  `liquid`, `last_steps`), the pours' `Poured` by position, and the buffers `xv`, `nf`, `id[now]` (`count × 16`
  each) and the deep part's `cellp`, `wallc`, `dfa` (whole).
- [ ] `Water::state(&self) -> WaterState` (waits for the card, as `copy` does) and `Water::from_state(gpu, field,
  gravity, numbers, feels, &state) -> anyhow::Result<Water>` (`Water::new` on the field, then as `copy`: the room,
  the buffers written, the clock set, the counters; refuses a state whose layout or buffer sizes don't match).
- [ ] Test `a_body_from_its_state_plays_on_as_its_copy`: a pool with a tap and trapped air (`scenes::pool`-like,
  12 px), 40 frames, state to bytes and back, then 30 more frames on the original, on its copy and on the restored
  body: `read()` equal to the bit for all three (and the drop count, `liquid()`, `frames()`).

### Task 2: frames captured inside a frame (`bpe-water drops/mod.rs`, `drops/shaders/finish.wgsl`, `passes.rs`)

- [ ] `P::Pack` (`cs_pack`, bindings 0, 1, 2, 5, 29): drop `i` of the step's drops (`st.n.x` of them) written to
  `pack[base + i]` as `vec2<u32>`: word 0 the cell `floor(x) + floor(y) × (cols − 1)` in its low 24 bits and the
  kind's low byte in its top 8 (`0xFFFFFFFF` for a drop not alive), word 1 the place in the cell (x, y: `u8` of
  `fract × 256`) and the speed (x, y: the log byte of decision 5, computed in px/s from grid units × cell). The
  output's base is `st.n.w` (the step record's last word, free) times the drops' count at that capture.
- [ ] `pub struct Captured { times: Vec<f64>, counts: Vec<u32>, buffer }` and `Water::frame_keeping(keep: u32,
  out: &mut Capture)`: as `frame()`, with the frame's steps cut at `round(i · n / keep)` for `i` in 1..=keep; at
  each cut (but the last) the step ends with `Finish` and the next starts with `Predict`; after each cut's finish a
  `Pack` pass writes that moment's drops into the capture buffer at its own base. The frame's last cut packs after
  the frame's own `Finish`. `Capture` grows its buffer as needed (`keep × count × 8` bytes) and keeps a ring of two
  so the host reads one while the card fills the other (`Capture::take(device) -> Option<CapturedFrames>`, waits
  for a frame sent before, never the one just sent). `Water::capture_now(out)`: the body's state as it is (frame 0
  and a restart's first frame).
- [ ] `keep` 1 with nothing cut is `frame()` exactly (same passes, same order); the kinds buffer packed at a cut is
  the one that step sorted into (`1 − (now ^ (j & 1))`), as the report's.
- [ ] Tests: `keeping_frames_changes_no_drop` (a pool with a tap, 30 frames with `keep` 8 on one body and plain
  `frame()` on another: `read()` equal to the bit); `a_kept_frame_is_the_body_at_its_moment` (the last capture of a
  frame unpacked equals `read()` within the quantisation: places within 1/256 cell, speeds within the log step);
  `kept_frames_are_in_order_of_time` (times of a `keep` 4 frame are `k + i/4`, a falling drop's place grows).

### Task 3: the frame codec and chunk files (`bpe-water take/`)

**Files:** `crates/bpe-water/src/take/mod.rs`, `take/frame.rs`, `take/chunk.rs`; `Cargo.toml` (lz4_flex).

- [ ] `TAKE_FORMAT: u32 = 1`, `ENGINE_VERSION: u32 = 1`, `engine_print() -> u64` (DefaultHasher over the engine's
  WGSL source, the plain drawing's is not included, `TAKE_FORMAT`, `ENGINE_VERSION`).
- [ ] `frame::encode(packed: &[[u32; 2]], cells: u32) -> Vec<u8>` (counting sort by cell, the planes, lz4
  `compress_prepend_size`), `frame::decode(bytes, layout) -> anyhow::Result<TakeFrame>` with `TakeFrame { xv:
  Vec<[f32; 4]>, kinds: Vec<u32> }` (place at the quantum's middle, speed from the log byte, grid units), bounds
  checked (a broken frame is an error, never a panic). Speed byte helpers `speed_byte(px_s)` / `byte_speed(b)` on
  the host, matching `cs_pack`'s.
- [ ] `chunk::ChunkWriter` (`new(dir, second)`, `push(time, keep, bytes)`, `finish() -> ChunkInfo`: the `.part`
  written whole then renamed; `ChunkInfo { second, frames, keep: Vec<u8> (60), bytes }`) and `chunk::Chunk::open(path)
  -> anyhow::Result<Chunk>` (`times()`, `frame(i) -> &[u8]`), and `point` files: `write_point(dir, frame,
  &WaterState)` (lz4, `.part` then rename), `read_point(dir, frame)`.
- [ ] Unit tests: a frame round trip (drops in every cell corner, at the course's far edge, a cell of 300 drops,
  air, a gone drop dropped), speed bytes round trip within 7 %, a cut short or garbled frame is an error, a chunk's
  `.part` is never read as a chunk, a point round trip.

### Task 4: the plain drawing moves drops on (`bpe-water draw/plain.rs`, `shaders/plain.wgsl`, `draw/stored.rs`)

- [ ] `Plain::draw_at(&buffers, &view, &look, ahead: f32)`: `draw` is `draw_at(.., 0.0)`. `U.view.w` carries
  `ahead` (seconds); the shader reads a drop's place through `place(i)` = `xv[i].xy`, plus `xv[i].zw × ahead` only
  when `ahead != 0`, in `drawn`, the splats and the ladder's splats alike.
- [ ] `StoredDrops::set(&mut self, device, queue, xv: &[[f32; 4]], kinds: &[u32])`: writes into its buffers (grown
  only when too small; `nf` stays full), so the Final view and the export reuse one set.
- [ ] Tests (in `tests/plain.rs`): `moving_on_by_nothing_draws_the_same` (ahead 0 against `draw`, to the bit) and
  `a_drop_moved_on_draws_where_it_will_be` (a lone drop at 100 px/s, ahead 0.1 s, drawn 10 px on).

### Task 5: the final's setup, the physics described, the Speed (`bpe-kit things/water/`, `modules/water/scene.rs`)

**Files:** `things/water/draft.rs`, `things/water/inputs.rs` (new), `things/water/slow.rs` (new),
`modules/water/scene.rs`, `things/water/draft/tests.rs`.

- [ ] `WaterWorld.speed: f32` (label "Speed", group "Slow motion", `range(0.125, 4)`, `span(0.125, 2)`, unit "×",
  keyframe, simple; default 1; hover "How fast the water plays: 0.25 is four times slower (the final keeps four
  times the frames there), 2 twice as fast. Slow motion makes the video longer."). "Slow motion" after "Trapped
  air" in `SECTION_ORDER`.
- [ ] `WaterSetup::final_of(scene, resolved)`: as `of` at `final_detail.cell_px()`, the print hashing
  `final_detail` in place of `draft_detail` (a shared `of_detail(scene, resolved, Detail)`); `pub fn detail()`;
  `WaterSetup::with_cell(cell)` for tests. The draft's print is unchanged.
- [ ] `inputs.rs`: `WaterInputs { parts: Vec<InputPart> }` (serde), `InputPart { name, from, fixed: u64, keys:
  Vec<(String, Vec<(f64, u64)>)> }`; `WaterSetup::inputs()` (decision 10: `world` (gravity, screens, the setup's
  detail key, trapped air, bubble rise), `course` (strokes' hash, the solids in order with place, turn, form,
  Material Wetting), `rock` (Wetting), `liquid:<n>` (id and Feel), `tap:<n>` (id, liquid, place, card; `from` its
  Starts − 3/60, 0 if keyed), `fills` (every Fill at the start), `numbers`); `earliest_change(a, b) -> Option<f64>`
  (`None`: the same water).
- [ ] `slow.rs`: `Speeds::of(scene, resolved)` (the Water card's Speed track or value; `speed_at(τ)`, clamped to
  0.125..4), `video_length(water_length)`, `water_time(video_t)` (a table of the video time at every 1/480 water
  second, binary search and straight between), `keep_for(frame) -> u32` (decision 3), `keyed()`.
- [ ] Tests: the final's print follows Final detail and not Draft detail (and the draft's the other way round);
  Speed is no physics (neither print moves); `earliest_change`: the same scene `None`; a Feel value 0; a keyframe
  added at 3 s after one at 1 s gives 1 s, one before every key gives 0; a tap's rate with Starts 5 gives 5 − 3/60;
  a tap removed from the front gives the earlier of the two starts; a carve stroke 0; a colour `None`; slow motion:
  a 1 s stretch at 0.25 in a 10 s scene is a 13 s video, `water_time` inverts `video_time`, `keep_for` gives 4 there
  and 1 elsewhere, a ramp keeps 2 then 4 then 8.

### Task 6: the finals folder (`bpe-kit things/water/finals.rs`)

- [ ] `Finals { dir }`: `Finals::default_dir()` (decision 12), `Finals::at(dir)`.
- [ ] `TakeHead` (serde RON): `format`, `engine`, `program`, `card`, `print`, `cell`, `layout` (origin, cols,
  rows), `inputs: WaterInputs`, `made: u32` (frames whose chunks are whole), `chunks: Vec<ChunkInfo>`, `points:
  Vec<u32>`, `drops: u32` (most), `seconds_making: f64` (wall time of the makes that wrote its chunks),
  `last_used` (unix seconds), `length_frames` (asked for).
- [ ] `Finals::find(&self, inputs, cell, card, print) -> Found { None, Exact(TakeHead), Prefix(TakeHead, valid:
  u32 frames, from_point: u32) }` (decision 11; heads that don't read are skipped).
- [ ] `Finals::begin(&self, print, head, from: Option<(&TakeHead, u32)>) -> anyhow::Result<TakeDir>`: makes or
  opens the folder, takes `make.lock`, links (or copies) the chunks and points before `from`'s restart point, writes
  the head; `TakeDir::{chunk_done, point_done, set_head, release}`.
- [ ] `Finals::clear_all()`, `clear(print)`, `sweep(days)`, `size() -> u64`, `free_space(dir) -> Option<u64>`
  (Windows `GetDiskFreeSpaceExW` through `windows-sys`; unix `libc::statvfs`), `room_needed(frames, drops,
  bytes_a_drop)`.
- [ ] Tests (temporary folders): a take found by its print; a prefix found with its valid frames and linked chunks
  (the new take's chunk files are the old's, the old take still whole); another card's take is not found; a broken
  head is skipped; a `.part` chunk is not counted; a stale lock is taken, a fresh one refused; sweep removes only
  takes unused for 30 days; clear empties; free space reads a number for the temp folder.

### Task 7: the make (`bpe-kit things/water/make.rs`)

- [ ] `MakeFinal { setup: Arc<WaterSetup> (final), speeds: Speeds, frames: u32 (ceil(Length × 60)), card }` and
  `make(&self, gpu, finals, progress: &MakeProgress, stop: &AtomicBool) -> anyhow::Result<TakeHead>`:
  1. find (Task 6); `Exact` and complete with the kept frames the speeds need: done. Else the frame to start from:
     the last restart point at or before the first frame that is missing, out of date or kept too thinly.
  2. the field at the final cell (timed into the progress: `field_ms`), the body from that point (`from_state`) or
     the setup's start; pours stop at `FINAL_MOST`.
  3. frame by frame: the inputs (as the draft), `frame_keeping(keep_for(k))`, the restart point every 120 frames
     (before stepping), the captures handed to a writer thread (a bounded channel of 2: encode, chunk, head after
     each chunk); `stop` ends it at the next frame (the chunk then being written is dropped).
  4. the head's `made`, `drops`, `seconds_making`, and the lock let go.
  `MakeProgress` (atomics): frames made, frames to make, the frame started from, bytes written, drops, phase.
- [ ] Tests (a small Water scene: one screen, a 160 × 160 fill and a tap, made at 8 px with `with_cell`):
  `a_final_is_made_and_reads_back` (2 s: every frame there, times in order, frame 60 unpacked equals a draft body's
  frame 60 within the quantisation); `a_final_remade_from_a_restart_point_equals_one_made_in_one_go` (make 4 s;
  change a tap's rate keyed after 2.5 s; make again: it starts from frame 120, links chunks 0 and 1, and its chunks
  2 and 3 equal byte for byte those of a take of the changed scene made from the start in another folder);
  `a_stopped_make_keeps_its_whole_chunks`; `slow_motion_keeps_more_frames_there` (Speed 0.25 from 1 to 1.5 s: chunk
  1 keeps 4 a frame in its first 30 frames and 1 after); `a_make_that_needs_more_room_than_the_disk_has_is_refused`
  (a `room` override).

### Task 8: export and the final frame (`bpe-cli water.rs`, `lib.rs`, `main.rs`)

- [ ] `check_exportable` lets Water scenes through (`NO_WATER_EXPORT` goes); `arranged::check_scenes` keeps refusing
  a video with one (`NO_WATER_VIDEO_EXPORT`, "Water scenes can't go in a video yet.").
- [ ] `water::WaterSource { Final { finals: Finals, make: bool }, Draft }` in `ExportOptions.water`; `export_video`
  sends a Water scene to `water::export_water`: video length from the speeds; for the final, `make` makes what is
  missing first (progress through a `WaterProgress` callback: making then drawing), else refuses with
  `FINAL_MISSING` ("The final isn't made for the whole video yet: make it first, or export the draft."); a draft
  runs `WaterSetup::of` frame by frame to each video frame's water time. Each video frame: its water time, the take
  frame nearest it (`TakeReader`, a chunk kept open, read and unpacked one frame ahead on a thread), `StoredDrops::set`,
  `Plain::draw_at` with the camera's shot (`water::shot_view(scene, resolved, τ, width, height)`) and `plain_look`
  at τ, `Readback::push_texture` (the plain picture's sRGB view), a silent track.
- [ ] `final_frame` for a Water scene: the take's frame at the time, or `FRAME_NOT_MADE`.
- [ ] `main.rs`: `--draft`, `--finals DIR`; a Water scene's final made first, its progress printed.
- [ ] Tests: `a_water_scene_exports_from_its_final` (make 1 s at 8 px into a temp folder, export at 0.25 scale:
  an mp4 of 60 frames, ffprobe-free: the file exists and the progress reached the end; skipped without ffmpeg as
  the other export tests are); `a_water_scene_exports_its_draft`; `an_export_without_the_final_says_so`;
  `the_final_frame_is_the_takes`; the old refusal test turned round; `a_video_with_a_water_scene_is_still_refused`.

### Task 9: the editor (`bpe-studio water_mode/{final_job,final_view}.rs`, toolbar, lane, status, export window)

- [ ] `water_mode/final_job.rs`: `FinalJob::start(scene, resolved, kit, finals) -> FinalJob` (a "water final" thread
  at low priority with its own `headless_device`; `progress()`, `stop()`, `done() -> Option<Result<TakeHead>>`);
  `Studio.water_final: WaterFinal { job, found: Found (re-found when the final's print changes), view: WaterView,
  picture: FinalPicture, export_after: bool }`.
- [ ] `water_mode/final_view.rs`: `FinalPicture` (a worker that opens the take, reads and unpacks the newest frame
  asked for; the field at the Final cell built on a thread; `StoredDrops` and a `Plain` on the window's device;
  `show(painter, render, view, look, at)` like `StagePicture::show`, with the pills of decision 17).
- [ ] Toolbar (Water scenes): **Draft | Final** (`toggle_button`s, hover words), **Make final** (or **Stop making**
  while it runs; greyed with words when there is nothing to make), a **Finals** menu (Clear this scene's final, Clear
  every final (its size), Show the finals folder).
- [ ] Timeline: `final_lane` (decision 18) in a Water scene, named "Final" for kittest, its hover saying what each
  part is.
- [ ] Status line (decision 21); the Video card's Length row's video length.
- [ ] Playhead: a Water scene's `tick` moves `time += dt × speed(time)`; `duration()` stays Length.
- [ ] Export window: a Water scene's subject shows a FINAL section and the buttons of decision 19; Make final and
  export starts the job and the export after it; its length row is the video's.
- [ ] Preview final frame in a Water scene goes through `bpe_cli::final_frame` (decision 20).
- [ ] Startup: `Finals::sweep(30)` on a thread.
- [ ] Tests (`water_mode/final_tests.rs`, `app/water_tests.rs`): the switch shows and swaps the view; Make final
  starts a job and the lane shows it being made (a fake progress); a physics edit marks the lane out of date from
  its earliest change and a colour edit doesn't; the export window shows Make final and export and Export the draft
  without a final and Export with one; the status line's words; slow motion moves the playhead at a quarter speed;
  the Finals menu clears.

### Task 10: measure, look, notes, review, PR, merge, hand-over

- [ ] `bpe-kit/tests/water_final_speed.rs` (`#[ignore]`, by hand and in the card job's timing step): a one-screen
  course carved as one chamber two thirds full with a tap (as `draft_pace`), made at 2 px for `WATER_SECONDS` (10)
  into a temp finals folder: wall seconds a scene second (making, of which packing and writing), MB a scene second,
  bytes a drop, restart point MB, the field's ms at 2 px, and drawing a frame from the take at 1080 × 1920 (read,
  unpack, draw); the take deleted after. The CI card job's timing step runs it and its lines join the summary.
- [ ] Full-size look check: `water-final` scene in `full_size_look.rs` (a take made at 3 px, its frame drawn at
  1080 × 1920; and the same moment moved on half a frame), looked at in full and 1:1 crops.
- [ ] Notes: `docs/notes/water.md` "Part 8 (as built)"; this plan's "As built and measured"; the spec's Parts line.
- [ ] One Opus review; fixes; PR; checks green; merge; part 6 hand-over in `ballphysics/water-mode/`; coordinator
  told; memory line.

## Things not to do

- Don't change the physics to make capturing easier (no new step counts, no frame length that follows Speed).
- Don't share a `ProgramStore`, a `WaterGpu` or buffers across the make's device and the window's.
- Don't read a take on the editor's thread (open, read and unpack on the worker).
- Don't keep Speed in the print, nor Final detail in the draft's print.
- Don't make Video mode take Water scenes (part 10), nor draw anything but the plain look (part 6).
- Don't write a chunk or point in place: always a `.part` file and a rename (a hard-linked file is shared).
- Don't add a pinned picture: the take's drawing is checked against the live body's.

## As built and measured

(Filled in as the part lands.)
