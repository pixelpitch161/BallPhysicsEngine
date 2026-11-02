# Marks part 4B (reading the numbers: captions, rules, end of video, synth, Meter) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything that reads a canvas's live numbers: caption tokens like `{Card.revealed}`, a rule block that fires when a canvas reaches a level, a rule that ends the video (export and the timeline stop there), a Canvas number input for the synth, and a new on-screen Meter thing (Split bar, Ring, Ranking). Then merge stream A, finish the demos, run the one review of part 4, and deliver.

**Architecture:**
- The numbers come from the shared interface already on the branch (`crates/bpe-runtime/src/canvas_numbers.rs`, commit `90f729b`): `World::canvas_numbers`/`teams`/`names`, `World::canvas_named`, `Log::canvas_at`, `Log::teams_of`, `CanvasRef`. Stream A makes canvases report them; this stream only reads them. **Your tests write numbers by hand** with `World::report_canvas`, `Log::push_canvas` and `set_teams`, so you never wait for stream A.
- Captions and the Meter read the log at the drawn step. Rules read the world in the same step (rules run after every thing's `after_step`). The synth reads a 60 Hz series built from the log after the full run.
- The Meter draws two new shape kinds the renderer gains in `scene.wgsl`: a filled rounded box clipped to an x range (kind 3) and a filled arc band (kind 4).

**Tech Stack:** Rust workspace: bpe-runtime, bpe-vm, bpe-synth, bpe-kit, bpe-gfx (wgpu + WGSL), bpe-cli, bpe-studio (egui 0.37, kittest).

**Spec:** `docs/superpowers/specs/2026-11-02-marks-part4-cover-scrape-claim-design.md` (§1, §7 to §10 are yours; read all of it). Read `CLAUDE.md` first.

**The other stream.** Stream A (`docs/superpowers/plans/2026-11-02-marks-part4a-canvas.md`) builds the canvas side at the same time. **Until Task 7, do not touch** `crates/bpe-kit/src/things/canvas/`, `crates/bpe-kit/src/modules/marks/`, `crates/bpe-kit/src/scenes.rs`, `crates/bpe-kit/src/starters.rs`, `crates/bpe-kit/src/pictures.rs`, `crates/bpe-gfx/src/canvas/`, `crates/bpe-gfx/src/shaders/canvas*.wgsl`, `crates/bpe-gfx/src/flow/`, `crates/bpe-studio/src/tree.rs`, `crates/bpe-runtime/src/canvas_numbers.rs` or the golden files. In `crates/bpe-runtime/src/events.rs` touch only the `Log` struct's new `end` field and `truncate_motion`; in `crates/bpe-gfx/src/frame.rs` only `ShapeInstance` (stream A edits `CanvasLayer` there). If you think one of the others needs a change, stop and say so in your thread.

## Global Constraints

- **Branch.** Start from the interface commit:

```sh
git fetch origin claude/project-thread-bv0ih5
git checkout -B <your designated branch> FETCH_HEAD
git log --oneline -1   # 90f729b Marks part 4: shared canvas numbers interface (world and log)
```

- **Golden logs are byte-identical, all of them, in this stream.** You add no golden lines. Rule blocks and End the video change no physics and log no events. After Task 7 the only golden lines that differ from main are stream A's five added lines.
- **Nothing here touches physics.** No new events, nothing in `Log::hash`, nothing in `World::fingerprint`. `World::end_step` and `Log::end` are not hashed.
- **Deterministic, and seek equals straight play.** Rule levels live in the `Vm` (cloned with the world). `Log::end` is forgotten on rewind past the step it was asked at. No `HashMap`/`HashSet` iteration in simulation code.
- **WGSL never names a field or variable `target`, `active` or `move`.**
- **egui/kittest pitfalls** (CLAUDE.md): name every control with `.labelled_by(label.id)`; pickers are `menu_button`s; `query_all_by_label(..).next()`; `scroll_to_me()` for rows below the fold; never `ui.add_space` inside a `Grid`.
- **Build environment.** `export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 BPE_REQUIRE_GPU=1`. After GPU test runs, the output must not contain "skipping". New GPU test helpers `panic!("no GPU adapter")` when `BPE_REQUIRE_GPU` is set and there is no adapter.
- **Formatting and lints.** `cargo fmt` (width 120) and `cargo clippy --workspace --all-targets -- -D warnings` before each commit.
- **Commits.** One per task, message as given, ending with your thread's attribution lines. Push after each (`git push -u origin <branch>`, never force; after Task 7's merge a normal push still works).

## Review Focus (Task 8's reviewer checks these first)

1. **Crossings fire once.** `when.canvas` fires on the crossing step only, never on the first sample, again only after crossing back. Test: Task 2 `canvas_level_fires_once_per_crossing`.
2. **The end survives seeking.** Seek past the end and back before it: `Log::end` is cleared and set again identically. Test: Task 3 `the_end_is_forgotten_on_rewind_and_found_again`.
3. **Export stops at the end and keeps sound in step.** Test: Task 3 `export_stops_at_the_end`.
4. **Captions read the drawn step, not the live world.** Test: Task 1 `caption_reads_the_log_at_the_drawn_step`.
5. **Meter shapes are exact.** Tests: Task 5 `filled_box_clips_to_its_range`, `arc_band_covers_its_sweep`.
6. **Goldens unchanged** in this stream; stream A's five lines are the only golden difference from main after the merge.

---

### Task 0: Environment

**Files:** none.

- [ ] **Step 1:** System packages and the Windows target:

```sh
apt-get update && apt-get install -y libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers \
  libxkbcommon-x11-0 libxkbcommon0 xvfb xdotool imagemagick gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

- [ ] **Step 2:** Create your branch (Global Constraints, "Branch"). In every shell, export the build variables.
- [ ] **Step 3:** GPU check: `cargo test -p bpe-gfx --lib canvas_marks_stamp_their_shapes 2>&1 | tee /tmp/gpu.txt; grep -c skipping /tmp/gpu.txt` prints `0`.
- [ ] **Step 4:** Baseline: `cargo test -p bpe-runtime -p bpe-vm -p bpe-synth` and `cargo test -p bpe-kit --test golden` pass.
- [ ] **Step 5:** Read `crates/bpe-runtime/src/canvas_numbers.rs` in full: every later task uses it.

---

### Task 1: Caption tokens

**Files:**
- Modify: `crates/bpe-kit/src/things/text/fill.rs` (`fill_with`).
- Modify: `crates/bpe-kit/src/things/text/draw.rs` (the lookup).
- Modify: `crates/bpe-kit/src/things/text/mod.rs` (the help text only).

- [ ] **Step 1: `fill_with`.** Keep `fill`'s signature and behaviour:

```rust
/// `fill`, with `extra` asked about any other token (the text between the braces, as written):
/// `Some` puts its answer in, `None` leaves the token as written.
pub fn fill_with(template: &str, v: &Values, extra: &dyn Fn(&str) -> Option<String>) -> String;
pub fn fill(template: &str, v: &Values) -> String { fill_with(template, v, &|_| None) }
```

  In the token match, the `_` arm becomes `match extra(token) { Some(s) => out.push_str(&s), None => out.push_str(&after[..=end]) }`. The five built-in tokens win over `extra`.
- [ ] **Step 2: the lookup** in `TextDraw::draw`, replacing the `fill(&s.text, ...)` call:

```rust
let lookup = |token: &str| -> Option<String> {
    let r = CanvasRef::parse(token)?;
    let canvas = cx.world.canvas_named(&r.canvas)?;          // unknown canvas: token stays as written
    let teams = cx.log.teams_of(canvas);
    let numbers = cx.log.canvas_at(canvas, last_step).cloned().unwrap_or_default();
    Some(r.text(&numbers, teams).unwrap_or_else(|| "0".into()))   // a team that hasn't claimed yet reads 0
};
let text = fill_with(&s.text, &Values { time, duration, hits, balls }, &lookup);
```

  `last_step` is the variable `draw` already computes for hits. Read the **log**, never `cx.world.canvas_numbers` (Review Focus 4: the live world can be ahead of the drawn moment).
- [ ] **Step 3:** Help text: `"A caption. {time}, {seconds}, {countdown}, {hits} and {balls} fill in as the video plays, and so do a canvas's numbers: {Canvas.revealed}, {Canvas.covered}, {Canvas.wet}, {Canvas.share Red}, {Canvas.leader}."`
- [ ] **Step 4: tests first** (`fill.rs` and `draw.rs` tests):
  - `extra_fills_unknown_tokens`: `fill_with("{a} {time} {b}", .., |t| (t == "a").then(|| "A".into()))` → `"A 0:00 {b}"`.
  - `fill_is_unchanged`: the existing tests pass as they are.
  - `canvas_tokens_fill_from_the_log`: a scene with a canvas thing named "Card" and a text `"{Card.revealed}% {Card.leader}|{Card.share Red}|{Nope.revealed}"`. Build the world; push into a `Log` by hand a `CanvasSample { step: 0, canvas, numbers: CanvasNumbers { revealed: 0.374, shares: vec![0.2, 0.5], .. } }` and `log.set_teams(canvas, vec![Team { name: "Red".into(), .. }, Team { name: "Blue".into(), .. }])`. Draw at time 0 with `build_frame_at` and collect the glyphs' text another way: easier, unit-test the `lookup` by moving it into a `pub(crate) fn canvas_token(world: &World, log: &Log, step: u64, token: &str) -> Option<String>` and test that: `"Card.revealed"` → `"37"`, `"Card.leader"` → `"Blue"`, `"Card.share Red"` → `"20"`, `"Card.share Green"` → `"0"`, `"Nope.revealed"` → `None`, `"card.REVEALED"` → `"37"`.
  - `caption_reads_the_log_at_the_drawn_step` (Review Focus 4): samples at steps 0 (revealed 0.1) and 240 (0.9); `canvas_token(.., step 120, ..)` → `"10"`, step 240 → `"90"`.
  - Before any sample: `"Card.revealed"` → `"0"`, `"Card.leader"` → `""`.
- [ ] **Step 5:** Fail, implement, pass (`cargo test -p bpe-kit --lib text`). Commit `feat(captions): canvas number tokens`.

---

### Task 2: Rules: When a canvas reaches

**Files:**
- Modify: `crates/bpe-vm/src/host.rs` (`Host::canvas_number`).
- Modify: `crates/bpe-vm/src/catalog.rs` (`when.canvas`).
- Modify: `crates/bpe-vm/src/run.rs` (`Vm.levels`, the trigger).
- Modify: `crates/bpe-kit/src/graphs.rs` (`WorldHost::canvas_number`).

- [ ] **Step 1: host.**

```rust
    /// A number of the first canvas `canvas` names (Covered, Revealed, Wet, or Share of `team`),
    /// 0 to 1, as it stands this step; `None` when there is no such canvas, it has not reported
    /// yet, or the team is unknown.
    fn canvas_number(&self, _canvas: &Target, _number: &str, _team: &str) -> Option<f32> {
        None
    }
```

  `WorldHost`: `self.world.things_of(self.me, canvas)` filtered to `self.world.kind_of(id) == Some("canvas")`, first one (id order); then `CanvasRef { canvas: String::new(), number: number.to_lowercase(), team: team.trim().into() }.value(self.world.canvas_numbers_of(id)?, self.world.teams_of(id))`.
- [ ] **Step 2: block.** In `rules()`, after `when.counter`:

```rust
        when("when.canvas", "When a canvas reaches", "Fires when a canvas's number crosses a level: covered, revealed, wet, or a team's share.")
            .param(target("canvas", "Canvas", "anything"))
            .param(choice("number", "Number", &["Covered", "Revealed", "Wet", "Share"]))
            .param(text("team", "Team", "").help("For Share: the team's name, as captions write it"))
            .param(number("level", "Level (%)", 50.0, 0.0, 100.0))
            .param(choice("way", "Way", &["Up", "Down"])),
```

  (Use `ParamSpec`'s real help method if `text(..)` returns something else; see how `target()` adds help.)
- [ ] **Step 3: run.** `Vm` gains `levels: BTreeMap<u32, f32>` (initialised empty in `new`). In the triggers loop, beside `when.counter`:

```rust
"when.canvas" => {
    let canvas = target_from_text(b.text("canvas"));
    if let Some(v) = cx.host.canvas_number(&canvas, b.text("number"), b.text("team")) {
        let level = b.num("level") / 100.0;
        if let Some(was) = self.levels.insert(b.id, v) {
            let up = b.text("way") != "Down";
            let crossed = if up { was < level && v >= level } else { was > level && v <= level };
            if crossed {
                self.fire(&cx, t, &mut walk);
            }
        }
    }
}
```

  Check the real accessor names (`b.text`, `b.num`) against the `when.counter` arm and use those. A `None` leaves `levels` as it was.
- [ ] **Step 4: tests first** (`run.rs` tests use a test host; add `canvas_number` to it backed by a `Cell<f32>`):
  - `canvas_level_fires_once_per_crossing` (Review Focus 1): values 0.2, 0.4, 0.6, 0.7, 0.4, 0.55 with level 50 % Up → fires on the third and sixth runs only. The first run never fires even when its value is already above (start at 0.7: no fire; then 0.3, 0.6 → one fire).
  - `canvas_level_down`: 0.8, 0.6, 0.4 at 50 % Down → fires once, on the third.
  - `unknown_canvas_never_fires`: the host returns `None` → nothing.
  - kit (`graphs.rs` tests): `world_host_reads_canvas_numbers`: a scene with a canvas, `world.report_canvas(id, CanvasNumbers { revealed: 0.3, .. }, &mut log)`, then `WorldHost { world: &world, me: ThingId::ROOT }.canvas_number(&Target::Anything, "Revealed", "")` → `Some(0.3)`; Share of a team set with `set_teams` reads its share; an unknown team → `None`.
  - The catalog test that lists Rules blocks (if any) includes `when.canvas`.
- [ ] **Step 5:** Fail, implement, pass (`cargo test -p bpe-vm -p bpe-kit --lib`). Commit `feat(rules): when a canvas reaches`.

---

### Task 3: End the video

**Files:**
- Modify: `crates/bpe-vm/src/catalog.rs` and `run.rs` (`do.end`, `Act::EndVideo`).
- Modify: `crates/bpe-kit/src/graphs.rs` (`action`).
- Modify: `crates/bpe-runtime/src/actions.rs` (`Action::EndVideo`), `world.rs` (`end_step`), `events.rs` (`Log::end`).
- Modify: `crates/bpe-cli/src/lib.rs` (`export_with`).
- Modify: `crates/bpe-studio/src/app.rs` (stop at the end), `crates/bpe-studio/src/timeline/mod.rs` (the marker).

- [ ] **Step 1: the block.** `act("do.end", "End the video", "Ends the video a little after this: export and the timeline stop there.", &[]).param(number("after", "After (s)", 2.0, 0.0, 30.0))`. In `reach`: `"end" => walk.push(Act::EndVideo { after: b.num("after").max(0.0) })`. `Act::EndVideo { after: f32 }`. `graphs::action`: `Act::EndVideo { after } => Action::EndVideo { after }`.
- [ ] **Step 2: the runtime.**
  - `Action::EndVideo { after: f32 }` (doc: "End the video `after` seconds from now; the first one counts.").
  - `World` gains `pub end_step: Option<u64>` (doc: "The step the video ends at, when a rule ended it. Cloned with the world; never in the fingerprint."), `None` in `build`.
  - `apply_one`: `Action::EndVideo { after } => if self.end_step.is_none() && after.is_finite() { let at = step + (after.max(0.0) as f64 * self.steps_per_second as f64).round() as u64; self.end_step = Some(at); log.set_end(step, at); }`.
  - `Log` gains `pub(crate) end: Option<(u64, u64)>` (asked at, ends at), **not** in `hash()` or `len()`. `pub fn set_end(&mut self, asked: u64, at: u64)`, `pub fn end(&self) -> Option<u64>` (the end step), `pub fn end_time(&self, steps_per_second: u32) -> Option<f64>`. In `truncate_motion(step)`: `if self.end.is_some_and(|(asked, _)| asked > step) { self.end = None; }`. `memory()` unchanged.
- [ ] **Step 3: export.** In `export_with`, right after `let warp = bpe_kit::warp_for(..)`:

```rust
    // A rule may end the video early: draw up to the first frame at or after the end.
    let total = match log.end_time(run.steps_per_second) {
        Some(end) => (0..total).find(|&k| warp.sim_time(k as f64 / fps as f64) >= end).map_or(total, |k| k + 1),
        None => total,
    };
```

  `run` is the full-run world built just above. The progress callback and the frame loop use the new `total`. The audio stays the full mix; ffmpeg's `-shortest` (already there) cuts it to the frames.
- [ ] **Step 4: the editor.**
  - `Studio::end_time(&self) -> Option<f64>`: `self.playback.log().end_time(sps)` turned into video time. The warp maps video → run (`sim_time`); find the video time by stepping the frame times like export does (frame step `1 / fps`), first one at or after the end; `None` when no end.
  - Where playback stops at the clip's end (`if playing && self.time >= self.duration()` and the tick that advances `self.time`), also stop at `end_time()`; `set_playing(true)` at or past the end restarts from 0 as it does at the duration.
  - Timeline: when `end_time()` is `Some(t)`, paint a vertical line in the theme's accent colour at `t` across the ruler and tracks, with a hover text "The video ends here (a rule ended it)", and give it `widget_info` with label "Video ends here" so tests can find it.
- [ ] **Step 5: tests first:**
  - vm: `end_block_asks_to_end`.
  - runtime: `the_first_end_counts` (two EndVideo actions at different steps → the first one's step); `end_is_not_in_the_hash` (a log with and without an end has the same `hash()` and `len()`).
  - kit (`crates/bpe-kit/tests/`, a new `end_video.rs`): a scene whose Rules module is `when.at` (time 1) → `do.end` (after 2): straight run to 5 s has `log.end() == Some(240 × 3)` at 240 Hz; `the_end_is_forgotten_on_rewind_and_found_again` (Review Focus 2): seek 5 → 0.5 → `end()` is `None`; → 5 → the same `Some(..)`. Build the rule graph the way existing kit rule tests do (find one with `grep -rn "when.at" crates/bpe-kit/src`).
  - cli: `export_stops_at_the_end` (Review Focus 3): export that scene with `max_frames: None` and a short `duration` (6 s) at a low scale with the test ffmpeg setup the existing export tests use; the video has `(3 s × fps) + 1` frames (or within one frame of it) and the audio stream is not longer than the video by more than one frame. If the existing export tests are `#[ignore]`d for needing ffmpeg, follow the same pattern and also add a unit test of the frame-count calculation (pull it into `fn frames_until_end(total, fps, end, warp) -> u32`).
  - studio (kittest): with that scene loaded and played past 3 s, a widget labelled "Video ends here" exists, and playing stops at the end time (the play head ≤ end + one frame).
- [ ] **Step 6:** Fail, implement, pass. Commit `feat(rules): end the video`.

---

### Task 4: The synth's Canvas number input

**Files:**
- Modify: `crates/bpe-synth/src/inputs.rs` (`SCENE_INPUTS`, `SceneSignals.canvas`, `value`, `key`, `Uses.canvas`, `uses`).
- Modify: `crates/bpe-synth/src/catalog.rs` (the `reads` param on `scene_input`).
- Modify: `crates/bpe-synth/src/blocks/physics.rs` (pass `reads`).
- Modify: `crates/bpe-synth/src/follow.rs` (`reads` for canvas, the label).
- Modify: `crates/bpe-kit/src/music/physics.rs` (`scene_series` builds the canvas series).
- Modify: `crates/bpe-studio/src/follow_menu.rs` and its two callers (`fx_list.rs`, `instruments.rs`).

- [ ] **Step 1: the input.** Append `def("canvas", "Canvas number", 0.0, 1.0)` to `SCENE_INPUTS` (last, so existing indices stay). `SceneSignals` gains `pub canvas: BTreeMap<String, Arc<Vec<f32>>>` (doc: "Each canvas number a Canvas number block reads, by its Reads text as written, sampled at `rate` on the video clock.") with `value`: `"canvas" => self.canvas.get(channel.trim()).map_or(0.0, |s| blend(s, self.rate, time))` (the `channel` argument carries the Reads text for this input). `key()` hashes the map like `levels`. `Uses` gains `pub canvas: BTreeSet<String>`; `uses()` inserts the trimmed `reads` text of every switched-on `scene_input` block whose input is `canvas`.
- [ ] **Step 2: the param.** `scene_input` gains `ParamSpec::new("reads", "Reads", Param::Text(String::new())).help("For Canvas number: the canvas and number, as captions write them, like Arena.revealed")`. `PhysicsIn` keeps a `reads: String` from `k.text("reads")` and, for `Reads::Scene`, passes `if self.input == "canvas" { &self.reads } else { &self.channel }` as `value`'s `channel`.
- [ ] **Step 3: follow.** In `follow()`, when `with.input == "canvas"`, insert the text into `"reads"` instead of `"channel"`. In `followed()`, the label is `format!("{} ({reads})", def.label)` for the canvas input when `reads` is not empty.
- [ ] **Step 4: the series.** `scene_series` gains a `scene: &Scene` parameter (pass it from `physics_inputs`). For every Reads text used anywhere a Scene block can run (instrument patches via `patches(scene)` and every effect graph: find where effect chains' graphs live by following how Channel level's channels are collected, and gather `uses(g).canvas` from all of them):
  - `r = CanvasRef::parse(text)`; skip when `None`.
  - The canvas: the first thing in `scene.order` with kind `"canvas"` whose name matches `r.canvas` (trimmed, ignoring case); skip when none.
  - Samples: `log.canvas_samples_of(id)` mapped to `(video_time(step / sps), [r.value(&numbers, log.teams_of(id)).unwrap_or(0.0), 0, 0, 0])`, resampled to `n` points at `rate` with the same `resample` and hold-last rule `balls` uses.
  - Insert under the trimmed text.
- [ ] **Step 5: the Follow menu.** `menu(ui, patch, channels, canvases: &[String])` where `canvases` are the scene's canvas names in tree order (a new `canvas_names(scene)` beside `channel_names`). In `list`, `def.name == "canvas"` shows a submenu: one submenu per canvas name with "Covered", "Revealed" and "Wet"; a click picks `FollowInput { kind, input: "canvas", channel: format!("{name}.{number}") }` (lowercase number). With no canvases it shows "No canvases" (weak). Update both callers.
- [ ] **Step 6: tests first:**
  - synth: `canvas_input_reads_its_series` (a `SceneSignals` with `canvas["Card.revealed"] = [0, 0.5, 1]` at rate 2 → `value("canvas", "Card.revealed", 0.5) == 0.5`; unknown text → 0); `uses_collects_canvas_reads`; `follow_puts_canvas_reads_in_reads` and `followed_names_the_canvas_read`.
  - kit: `scene_series_has_canvas_numbers`: a scene with a canvas "Card" and a patch with a Canvas number block reading `Card.revealed`; a hand-made log with samples (0.0 at step 0, 1.0 at step 240) → the series at 1 s is 1.0 and at 0 s is 0.0.
  - studio (kittest): the Follow menu on a knob of an instrument patch shows "Scene" → "Canvas number" → "Card" → "Revealed", and clicking it wires a Scene block with `reads = "Card.revealed"`.
- [ ] **Step 7:** Fail, implement, pass. Commit `feat(synth): canvas number input`.

---

### Task 5: Two new shape kinds for meters

**Files:**
- Modify: `crates/bpe-gfx/src/frame.rs` (`ShapeInstance::fill_box`, `ShapeInstance::arc_band`; the field docs).
- Modify: `crates/bpe-gfx/src/shaders/scene.wgsl` (`vs_shape` extent, `shade_shape` branch).

- [ ] **Step 1: constructors.**

```rust
    /// A filled box of half size `half`, corners rounded by `corner`, shown only where its own x
    /// (from the centre, before the turn) is within `clip` (use [-1e9, 1e9] for all of it).
    pub fn fill_box(centre: Vec2, angle: f32, half: Vec2, corner: f32, clip: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            a: [centre.x, centre.y, angle, clip[0]],
            b: [half.x, half.y, corner.min(half.x).min(half.y).max(0.0), clip[1]],
            c: [3.0, 0.0, 0.0, 0.0],
            color: blend_alpha(color),
            paint: PaintGpu::default(),
        }
    }
    /// A filled band of a ring: radius `radius`, `half_width` either side of it, centred on the
    /// direction `mid` (radians, as `angle` turns everything else) and reaching `half_sweep` either way
    /// (π or more: the whole ring).
    pub fn arc_band(centre: Vec2, radius: f32, half_width: f32, mid: f32, half_sweep: f32, color: [f32; 4]) -> Self {
        Self {
            a: [centre.x, centre.y, mid, 0.0],
            b: [radius, half_width, half_sweep, 0.0],
            c: [4.0, 0.0, 0.0, 0.0],
            color: blend_alpha(color),
            paint: PaintGpu::default(),
        }
    }
```

  Update the doc comments of `a`, `b`, `c` for kinds 3 and 4.
- [ ] **Step 2: shader.** In `vs_shape`, the extent: kind 3 keeps `length(s.b.xy)`; kind 4 is `s.b.x + s.b.y` (check the `c.x > 0.5` branch: make it `if s.c.x > 3.5 { extent = s.b.x + s.b.y; } else if s.c.x > 0.5 { extent = length(s.b.xy); }`). In `shade_shape`, first line: `if in.c.x > 2.5 { return shade_fill(in); }`:

```wgsl
// Kinds 3 and 4: filled shapes with no line and no glow, edges a pixel soft.
fn shade_fill(in: ShapeOut) -> vec4<f32> {
    let px = g.view.z;
    let local = rotate(in.world - in.a.xy, -in.a.z);
    var d = 0.0;
    if in.c.x < 3.5 {
        let corner = in.b.z;
        let q = abs(local) - (in.b.xy - vec2<f32>(corner));
        d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - corner;
        d = max(d, max(in.a.w - local.x, local.x - in.b.w));           // the x clip
    } else {
        d = abs(length(local) - in.b.x) - in.b.y;
        let half = in.b.z;
        if half < 3.14159 {
            d = max(d, dot(vec2<f32>(local.x, abs(local.y)), vec2<f32>(-sin(half), cos(half))));
        }
    }
    let cover = clamp(0.5 - d / px, 0.0, 1.0) * abs(in.color.a);
    if cover <= 0.0 {
        discard;
    }
    return vec4<f32>(in.color.rgb * cover, covering(cover, in.color.a));
}
```

  `fs_shape_each` goes through `shade_shape`, so blend layers work too. The objects pass (`draw_objects`, `shape_object`) uses the same vertex shader; check its fragment shader handles kinds 3 and 4 (treat them as filled the same way, or exclude them if the objects pass only needs outlines; say which in a comment).
- [ ] **Step 3: tests first** (GPU, in `bpe-gfx/src/lib.rs` tests next to the shape tests):
  - `filled_box_clips_to_its_range` (Review Focus 5): a white box half (100, 20) corner 20 with clip [−100, 0] on black: a pixel at local (−50, 0) is white, (50, 0) black, (−99, 18) black (the rounded corner), the pixel at (0, 0) about half covered.
  - `arc_band_covers_its_sweep`: radius 100, half width 10, mid −π/2 (straight up), half sweep π/4: a pixel at angle −π/2 radius 100 is white, at angle 0 black, at angle −π/2 radius 70 black; half sweep π covers the whole ring.
- [ ] **Step 4:** Fail, implement, pass. Commit `feat(gfx): filled box and arc band shapes`.

---

### Task 6: The Meter thing

**Files:**
- Create: `crates/bpe-kit/src/things/meter/mod.rs` (settings and kind) and `draw.rs` (`MeterDraw`).
- Modify: `crates/bpe-kit/src/things/mod.rs` (`pub mod meter;`), `crates/bpe-kit/src/lib.rs` (register `MeterKind` and `MeterDraw` the way Text is; re-export `MeterSettings`).
- Modify: the studio's icon table if kinds map icons by name (find how `"text"` becomes an icon; add `"meter"`, or use `"text"` if there is no table).

- [ ] **Step 1: settings.**

```rust
pub enum MeterReads { #[default] Shares, Revealed, Covered, Wet }
pub enum MeterLook { #[default] SplitBar, Ring, Ranking }

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MeterSettings {
    #[setting(label = "Canvas", simple)]
    pub canvas: Target,
    #[setting(label = "Reads", simple)]
    pub reads: MeterReads,
    #[setting(label = "Look", simple)]
    pub look: MeterLook,
    #[setting(label = "Width", when = "look!=ring", range(100, 1080), unit = "px", keyframe)]
    pub width: f32,
    #[setting(label = "Thickness", range(4, 200), unit = "px", keyframe)]
    pub thickness: f32,
    #[setting(label = "Ring size", when = "look=ring", range(40, 540), unit = "px", keyframe)]
    pub ring_size: f32,
    /// How calmly it moves: 0 follows every change, 1 averages over half a second.
    #[setting(label = "Smooth", range(0, 1))]
    pub smooth: f32,
    #[setting(label = "Show percent")]
    pub show_percent: bool,
    #[setting(label = "Show names", when = "reads=shares")]
    pub show_names: bool,
    #[setting(label = "Font")]
    pub font: Font,
    #[setting(label = "Text size", range(8, 200), unit = "px", keyframe)]
    pub text_size: f32,
    #[setting(label = "Text colour")]
    pub text_colour: Color,
    #[setting(label = "Empty colour")]
    pub empty_colour: Color,
    #[setting(label = "Fill colour", when = "reads!=shares")]
    pub fill_colour: Color,
    #[setting(label = "Rows", when = "look=ranking", range(1, 8))]
    pub max_rows: u32,
}
```

  Defaults: `canvas Target::Anything, reads Shares, look SplitBar, width 800, thickness 56, ring_size 160, smooth 0.5, show_percent true, show_names true, font Font::Anton, text_size 36, text_colour Color::WHITE, empty_colour Color { a: 38, ..Color::WHITE }, fill_colour Color::rgb(0xff, 0xd2, 0x3e), max_rows 4`. Check `Color`'s field names before writing the struct literal.
- [ ] **Step 2: kind.** `MeterKind`: id `"meter"`, family `"On screen"`, name `"Meter"`, icon `"meter"`, help `"A bar, ring or ranking of a canvas's numbers: each team's share in a colour war, or how much is revealed."`, no slots, `sticks_to_screen() -> true`. No `ThingSim`.
- [ ] **Step 3: what it shows** (`draw.rs`):
  - The canvas: the first id in `cx.world.order()` with kind `"canvas"` that `cx.world.matches(cx.thing.id, id, &s.canvas)`. None → draw nothing.
  - The step: `last_step` as Text computes it. The window: `w = (s.smooth.clamp(0, 1) × 0.5 × sps).round() as u64` steps. Average the log samples of the canvas with `step` in `(last_step − w, last_step]` (just the latest at or before `last_step` when `w == 0` or the window holds none). Shares are averaged per team index (a missing index counts 0). No sample at all → every value 0.
  - Values: Shares → one entry per team in `cx.log.teams_of(canvas)` (in team order) with the averaged share and the team's colour; Revealed/Covered/Wet → a single value with `fill_colour`.
  - Labels: percent text `format!("{}%", (v × 100).round())`; names from the teams; for a single value the name is "Revealed", "Covered" or "Wet".
- [ ] **Step 4: how it draws.** Centre `p = cx.thing.transform.position` (screen or world as the frame is; the frame handed to `draw` is already the overlay for a stuck thing). `t = thickness`, `half = (width / 2, t / 2)`, `corner = t / 2`. Colours to linear with `.to_linear()`.
  - **Split bar**: a background `fill_box(p, 0, half, corner, [-1e9, 1e9], empty)`. Then pieces left to right: for Shares each team in order with share `v`, for a single value one piece of `fill_colour`; a piece spans local x `[x0, x0 + v × width]` from `x0 = -width/2`, drawn as `fill_box(p, 0, half, corner, [x0, x1], colour)`. Unclaimed ground is the background showing. Text: for Shares with names or percent, a label `"Red 37%"` (whichever parts are on) centred in each piece wider than 90 px, at `text_size` (shrunk to fit the piece, `max_width = piece width − 12`); single values: the percent centred on the bar.
  - **Ring**: background `arc_band(p, ring_size / 2, t / 2, 0, PI, empty)`. Pieces clockwise from the top: start `s = 0`, a piece of sweep `w = v × TAU` is `arc_band(p, ring_size / 2, t / 2, -FRAC_PI_2 + s + w / 2, w / 2, colour)`, then `s += w`. Skip pieces with `w < 1e-4`. Centre text: Shares → the leader's name (if names on) over its percent (if percent on); single value → the percent. Text centred at `p`, the name `text_size × 0.6` above, the percent `text_size × 0.6` below when both show.
  - **Ranking**: rows sorted by value, largest first, ties by team order, at most `max_rows`. Row `k` centre `p + (0, k × t × 1.4)`; each row a background box and a piece `[x0, x0 + v × width]` in the team's colour (the same clipped `fill_box`); its name left-aligned inside (centre the text block at `x0 + 16 + text_width / 2`, using `bpe_gfx::text::measure_text`) and its percent right-aligned inside (`x1_bar − 16 − w/2`). A single value draws one row named by the reading.
  - Text: `layout_text(&label, s.font.face(), s.text_size, anchor, TextAlign::Center (the real name), max_width, usize::MAX, &style, &mut frame.glyphs)`, then set the new glyphs' `color`/`outline_color` to `blend_alpha(text_colour linear)`, as `text/draw.rs` does.
- [ ] **Step 5: tests first** (`meter/draw.rs` tests, with `build_frame_at` like `text/draw.rs` tests; the frame's overlay holds the meter's shapes):
  - `split_bar_pieces_follow_the_shares`: a canvas, teams Red and Blue, a sample with shares `[0.25, 0.5]`: the overlay has 3 kind-3 shapes (background + 2); piece 1's clip is `[-400, -200]`, piece 2's `[-200, 200]` (width 800).
  - `smooth_averages_the_window`: samples 0.0 at step 0 and 1.0 at steps 4…240 with smooth 1 (0.5 s = 120 steps at 240 Hz) drawn at step 240 → the single value is 1.0; drawn at step 60 → between 0.3 and 0.7.
  - `ring_starts_at_the_top`: one piece of 0.25 → `a.z` is `-π/2 + π/4` and `b.z` is `π/4`.
  - `ranking_sorts_and_limits`: four teams with shares 0.1, 0.4, 0.2, 0.3 and `max_rows 3` → rows for the 0.4, 0.3, 0.2 teams in that order (check the pieces' clip right ends).
  - `single_number_uses_the_fill_colour`: Reads Revealed → one piece in `fill_colour`.
  - `no_canvas_draws_nothing`.
  - `meter_settings_round_trip` and the Add menu lists "Meter" under "On screen" (studio kittest, or a registry test that `kind.family() == "On screen"`).
- [ ] **Step 6:** Fail, implement, pass. Commit `feat(meter): split bar, ring and ranking meters`.

---

### Task 7: Merge stream A and finish the demos

**Starts only when stream A is done.** Stream A's thread tells the coordinator its branch and commit; the coordinator passes it on to you. If you reach this task first, post in your thread that tasks 1 to 6 are done and you are waiting for the canvas side, then end your turn. Do not poll.

**Files:**
- Merge: stream A's branch.
- Modify: `crates/bpe-kit/src/scenes.rs` (two demos gain on-screen numbers and a rule).
- Modify: `scenes/next/scratch-to-reveal.ron`, `colour-war.ron` (re-blessed).
- Modify: `CLAUDE.md` (crate map: `meter` in bpe-kit's thing folders; the canvas kinds in one line).

- [ ] **Step 1: merge.** `git fetch origin <A's branch> && git merge --no-ff FETCH_HEAD -m "Merge part 4A (canvas side)"`. Conflicts are expected only in `bpe-kit/src/lib.rs` (both add re-exports and registrations: keep both), `bpe-runtime/src/events.rs` (A's `MarkShape` variants, your `Log.end`: keep both), `bpe-gfx/src/frame.rs` (A's `CanvasLayer` fields, your `ShapeInstance` constructors: keep both), and maybe `bpe-gfx/src/lib.rs`. Then `cargo build --workspace --all-targets` and `cargo test -p bpe-kit --test golden` (A's goldens) pass.
- [ ] **Step 2: demos.** Change only the two demos, after their canvas is added, never their physics:
  - **Scratch to reveal**: a Text thing "Revealed" (stuck to the screen) at (0, −800), text `"{Card.revealed}% revealed"`, size 72; a Rules module on the scene root (or the way existing demos add rules: find one with `grep -n "rules" crates/bpe-kit/src/scenes.rs`) with `when.canvas` (canvas `anything`, Revealed, level 90, Up) → `do.end` (after 2).
  - **Colour war**: a Meter at (0, −820), Split bar, Shares, width 900, thickness 48; a Text at (0, −740), `"{War.leader} leads"`, size 56.
  - **Goldens must not change at all:** `cargo test -p bpe-kit --test golden` (debug) and `--release` pass **without** blessing. If either fails, a thing you added moved something: find it (superpowers:systematic-debugging), do not bless.
  - Scene files: `BPE_BLESS=1 cargo test -p bpe-kit --lib the_shipped_scene_files_match_the_code`; `git status scenes/next` shows exactly the two files modified.
- [ ] **Step 3: tests:** `scratch_to_reveal_ends_itself`: running Scratch to reveal to 20 s, either `log.end()` is `Some` and its step is 2 s after `revealed` first reached 0.9, or `revealed` stayed under 0.9 the whole time (print which; if it never reaches 0.9 in 20 s, lower the rule's level to the highest whole multiple of 10 it does reach by 15 s and note that in the PR). `colour_war_meter_draws`: at 10 s the frame's overlay has a kind-3 shape per team plus the background.
- [ ] **Step 4: CLAUDE.md**: add `meter` to bpe-kit's list of thing folders, and one line under "Engine pitfalls": "A canvas's grid is the truth for Cover, Powder and Fog (drawn from `grid.image()`); only Paint canvases log marks. Numbers go through `World::report_canvas` and are read from the log (`Log::canvas_at`) by captions and the Meter."
- [ ] **Step 5: full run** (background, 8 to 10 minutes):

```sh
cargo test --workspace 2>&1 | tee /tmp/all.txt; grep -c skipping /tmp/all.txt   # 0
cargo test --workspace --release 2>&1 | tee /tmp/all-release.txt
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
git diff origin/main -- crates/bpe-kit/tests/golden.txt crates/bpe-kit/tests/golden-debug.txt   # only A's 5 added lines
```

  Everything passes, except in `--release` `bpe-runtime`'s `pushing_an_earlier_step_panics_in_debug` (debug-only).
- [ ] **Step 6:** Commit `feat(marks): numbers on screen in the demos` and push.

---

### Task 8: The one review of part 4 (Opus subagent)

- [ ] **Step 1:** Dispatch **one** Opus subagent with superpowers:requesting-code-review over `git diff origin/main...HEAD` (both streams). Give it: the spec path; both plan paths; both plans' Global Constraints and Review Focus sections, verbatim. Ask it to check first each Review Focus item of both plans and its named test, then: determinism (no HashMap iteration, no wall clock), WGSL names (`target`, `active`, `move`), that Paint canvases without the new brushes are byte-identical, that the golden diff is only A's five added lines, Windows path handling for under-pictures, and the merge (nothing from either stream lost).
- [ ] **Step 2:** Fix every Critical and Important finding with superpowers:receiving-code-review (verify each before changing code). Re-run the affected tests, then Task 7 Step 5.
- [ ] **Step 3:** Commit `fix(marks): review fixes` (no commit if nothing changed) and push.

---

### Task 9: Deliver

- [ ] **Step 1:** Build the Windows exes:

```sh
cargo build --release --target x86_64-pc-windows-gnu -p bpe-studio -p bpe-cli
mkdir -p /mnt/project-files/ballphysics/marks-layer/part4
cp target/x86_64-pc-windows-gnu/release/ball-studio-next.exe /mnt/project-files/ballphysics/marks-layer/part4/ball-studio.exe
cp target/x86_64-pc-windows-gnu/release/bpe-render-next.exe /mnt/project-files/ballphysics/marks-layer/part4/
ls -la /mnt/project-files/ballphysics/marks-layer/part4/
```

  No videos and no screenshots.
- [ ] **Step 2:** Open one PR to `main` from your branch. Title: `Marks part 4: covers, scraping, colour wars and live numbers`. The body is a plain-language summary for the owner (what's new, where it is in the editor, the five demos), then a short technical section (golden lines added: 5, none changed; the speed means from stream A; what the review fixed), ending with the attribution lines your thread is given. Subscribe to its activity.
- [ ] **Step 3:** After the PR is merged, and only then, through your thread's folder access to `F:\!Ai Stuff\BallPhysicsEngine\bin\`: rename `ball-studio.exe` to `ball-studio-backup.exe` (replacing an older backup), copy the new `ball-studio.exe` in, and copy `bpe-render-next.exe` beside it if a renderer exe is there. Confirm both files and sizes.
- [ ] **Step 4:** Tell the owner in plain words: what changed, which demo scenes to open (Scratch to reveal, Colour war, Territory, Sand garden, Fog glass), that the Canvas entry of "+ Add thing" now offers starting points, and that the old exe is kept as `ball-studio-backup.exe`. No commands.

---

## Likely misreadings (read before starting)

- **Don't compute numbers.** Canvases report them (stream A). This stream only reads them. In tests, write them by hand.
- **Captions and the Meter read the log, rules read the world.** The log is what was true at the drawn moment; the world can be ahead in the preview. Rules run inside the step, so the world is exactly right there.
- **`share` is a share of the whole canvas**, not of the claimed part. The rest is unclaimed and the Meter shows it as the empty colour.
- **Teams appear over time.** `log.teams_of` lists every team the run has made so far; a sample's `shares` may be shorter than the team list (a team that claimed later). Treat a missing share as 0.
- **End the video does not stop the simulation.** It only notes the end; the world keeps stepping so nothing else changes.
- **Do not add a `Log` event** for the end. Events are hashed.
- **`when.canvas` must not fire on the first value**, even if it is already past the level.
- **The Meter has no sim and no slots.** Its colours are its own settings and the teams' colours.
- **Keep `fill`'s signature.** Other code calls it.
- **New `SCENE_INPUTS` entry goes last** so saved patches keep their inputs.
