# Staying Lines that really stay (many balls)

Asked 2026-11-06 by the owner: with 1000+ balls whose trails are set to stay forever, the trails start to disappear
when a lot is drawn. They want staying trails to stay, however many balls.

## Cause

`trail/draw.rs::draw` shares `MOST_STRIP_POINTS` (200 000) out evenly across the balls wearing the layer
(`points_each`), and `kinds::limit` keeps each ball's **newest** points. With 1000 balls a Line keeps 200 points,
about 3.3 s at 60 samples a second, so a "Stays" Line loses everything older than that. `kinds::thin` (one sample in
4/8/16 plus corners) doesn't help: it keeps the newest 240 points whole, already more than 200.

A second, quieter cap: `Frame::fit_lists` cuts the frame's trail vertices to `MOST_TRAIL_VERTICES` (3 M, six a
segment). A layer's Glow, Outline and Shadow sub-layers each draw the strip again, so 200 000 points with two of
them is 3.6 M vertices: the frame cuts the end of the list and whole trails vanish.

## Design (no new option)

The Line's **Stays** switch keeps doing what it says; nothing new in the inspector.

1. **Simplify instead of cut.** A staying Line's path (`kinds::path` with no cut, after `drop_still_head`) goes
   through `kinds::simplify` instead of `thin` + `limit`: a greedy sleeve fit from the oldest point to the ball,
   keeping a point only where the line leaves a band `tol` wide around a straight run from the last kept point (and
   where it starts coming back towards it, as a ball bouncing straight back). Straight runs between bounces become
   two points, so a crowd bouncing in a circle costs about one point a bounce. Because the fit goes from the oldest
   point forwards and restarts at fixed points, what it keeps of the old part never changes as the line grows (no
   shimmer).
2. **Fixed points.** The oldest point and the ball are always kept. So are samples whose number (counted from the
   start of the clip, `step / every`) is a multiple of `grid`, the smallest power of two giving at most
   `GRID_POINTS` (64) such points along the ball's line (more with wobble: `waves × 16`), so colours, fades and
   wobble that change along the line still have points to change at. A power of two keeps the set nested: when
   the line doubles in age, every other fixed point drops out and the rest stay.
3. **Tolerance.** `tol` starts at `LINE_TOLERANCE` (0.15 output pixels, in world units through the camera zoom;
   as is when the trail draws in screen space). Invisible.
4. **Budget and levels.** The layer's budget is `MOST_STRIP_POINTS / (1 + strip sub-layers)` (Shadow, Glow, Outline
   each draw the strip again), so the frame's vertex guard never cuts a trail. A layer's Lines are drawn at a
   **level**: at level `l` with `2^l` times the tolerance and the grid spacing, up to `MOST_LEVEL` (8). Each frame
   picks the finest level whose points fit (`draw.rs::pick_level`), looking from the level the last frame used
   (`line::level_of`/`set_level`) and checking the one finer, so which level is drawn never hangs on what was drawn
   before. Only past the coarsest level, as before, is each trail cut to its newest share (`cut_to`).
5. **Remembered between frames** (`trail/line.rs`). A staying Line (stays, not restarting at bounces) is worked out
   straight from the history, oldest first (`History::trace_after`), with a streaming `Sleeve`; what it settles
   (points kept for good) and its state after the newest sample are kept in a `Memo` per (thing, layer, ball row,
   level) in a process-wide map. Each frame carries on with only the new samples and redoes the short tail to the
   ball. A memo is checked against the history every time (same tolerance, grid and first sample, and its last
   place still in the history); after scrubbing back it rewinds to its newest grid sample the history still has
   (the fit restarts at grid samples, so everything before is still right), else it starts afresh. A remembered
   line is the same as one worked out afresh (`along` is summed in the same order in f64; ago and arc come from
   moments and distances along the way), checked by tests.
   Where a ball stops (a caught ball, a held one), the sample after it got there is kept (`stops`), so
   `drop_still_head` still ends a caught ball's line where it stopped (one sample later than before: invisible).
6. **Other trails** (Streak, Ribbon, Dots, Smoke) keep their per-ball share as before when over the budget. Lines
   that don't stay or restart at bounces are simplified per frame from their (bounded) path with `kinds::simplify`.

Measured (release, 1200 balls launched outward, 1 minute in, `time_a_crowd_of_staying_lines`): the frame took about
580 ms with the first, unremembered version; now about 50 ms while playing, about 110 ms after scrubbing back a few
seconds, about 0.4 s for a first frame. Without gravity the lines stay at level 0 (full detail); with gravity
(curved arcs) a minute in they are at level 5 (4.8 px), every line still reaching its start
(`levels_of_a_crowd_a_minute_in`).

### Not done (and why)

- **Painting old parts into a kept picture** (bake). Fast whatever the length, but it breaks "nothing is kept
  between frames" (scrubbing back means repainting from 0), goes wrong when the camera moves or zooms, and freezes
  anything that changes along the line (fade, colours by age, wobble, glow). Simplifying keeps every one of those.

## Steps (as first planned; see the design above for what was built)

1. `kinds.rs`: `pub fn simplify(path: &Path, tol: f32, grid: u64, newest: u64, newest_ago: f32, interval: f32) ->
   Path` (newest-first in and out; fixed points by sample number `newest - round((ago - newest_ago) / interval)`,
   corners and the head never on the grid) and `pub fn grid_for(samples: usize, wobble_waves: Option<f32>) -> u64`.
   The core `fn sleeve(points: &[Vec2], fixed: impl Fn(usize) -> bool, tol: f32) -> Vec<usize>` works oldest first
   over indices and returns kept indices oldest first.
   Tests: a straight run keeps its two ends; a right angle keeps the corner; a ball going out and straight back keeps
   the far end; points within `tol` of the drawn line everywhere (random walk check); fixed grid points kept;
   simplifying a longer copy of the same line keeps the same old points (stable).
2. `draw.rs`: split `draw` into a first pass (`fn gather`) that picks each ball's layer/trail copy and builds its
   path (staying lines simplified at `tol`), a fitting step (`fn fit`: escalation rounds for staying lines, then the
   old per-ball share for the rest), and the drawing pass (unchanged code). Remove `thin` (and `KEEP_WHOLE`) if
   nothing else uses it.
   Budget helper `fn strip_budget(subs: &[SubLayer]) -> usize`.
3. Tests in `draw.rs`: `a_crowd_of_lines_that_stay_all_reach_their_start` (1200 balls, 12 s: every ball's first
   sample has a drawn vertex near it; fails before the fix with 1200 missing), a Line with Glow+Outline on a crowd
   stays under `MOST_TRAIL_VERTICES`, the existing left-trail tests still pass. Physics goldens can't move (drawing
   only).
4. Timing check (release): 1200 balls at 60 s, frame build time before/after.
5. `cargo test --workspace`, clippy, fmt; one Opus review; PR.
