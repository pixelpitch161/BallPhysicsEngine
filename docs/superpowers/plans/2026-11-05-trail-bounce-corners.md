# Trails follow the real bounce (bounce corners in the history)

Bug (Aedan, 2026-11-05): with a fast ball, a trail sometimes points a different way from how the ball
really bounced.

## Root cause

`bpe-runtime/src/history.rs` keeps each ball's place 60 times a second (every 4th step at 240 Hz) and a
trail (`bpe-kit/.../trail/kinds.rs::path`) is drawn straight from sample to sample. A bounce almost never
lands on a sample, so the trail runs from the last place before the bounce straight to the first place
after it and cuts the corner. At 3600 px/s a sample is 60 px apart, so the cut-off corner is up to ~30 px
deep and the trail's last segment before the bounce points the wrong way. Sampling every step would not
fix it either: a fast ball takes up to 64 substeps a step and its bounce lands inside one.

The wall pass (`World::collide`, `keep_clear_from`) moves a ball back to just touching the wall in the
substep it hits, so the ball's place at the end of that substep is the bounce point itself (pairs: where
the two balls touch). Nothing records it today.

## Fix

1. `History` gets **corners**: per ball a `VecDeque<(f64, Vec2)>` of (moment in steps, fractional; place),
   oldest first. `History::corner(ball, at, place)` adds one; it is ignored for a ball with no sample yet
   (a trail never reaches before its first sample) and for a moment not after the ball's newest point.
   `travelled` now runs through corners too (distance to the newest point, sample or corner), and
   `History::tip(ball)` is that newest point. `record` drops corners older than the ball's oldest sample.
   `memory()` counts them. `History::trace(ball)` gives samples and corners merged, newest first, as
   (moment in steps, place); a corner on a sample's own moment is left out (same place).
   `History::at` is replaced by `History::place(ball, t, head_t, head)`: where the ball was at moment `t`
   (steps), from `head` at `head_t` back through corners and samples, O(1) to find the samples and a
   binary search for corners.
2. `World::step`: when any history is kept, after each substep every new `Hit` event in the log adds a
   corner for each side that is a ball of a thing with a history (`item` < its rows), at
   `(step - 1) + (k + 1) / substeps` steps, at the ball's place then. Physics is untouched (goldens must
   not move).
   Found while testing: the wall pass pushes the ball back along the wall's normal only, so where the
   substep leaves it has slid along the wall from where it touched (up to a substep's move), and the
   trail's way in to that point is a few degrees off. So `collide` also notes, for a wall hit, the moment
   it touched (`(step - 1) + hit_at`) at `start.lerp(moved_to, within)`: on the line it came in on. The
   trail then runs in straight, touches, slides the little way the physics slid it, and goes out.
3. `kinds::path` walks `History::trace` instead of `points`: each point's `ago` is
   `newest_ago + (newest_step - t) * dt` (dt = interval / every). A corner at or after the head
   (`ago <= 1e-6`) or at the same place as the point before it is skipped. Cuts (time, distance, since)
   work as before on every point.
4. `Path` gets `corner: Vec<bool>`. `Path::along(k)` becomes time-based (`ago[k] / reach`, index-based only
   when reach is 0), as points are no longer evenly spaced. `thin` always keeps corners and counts sample
   numbers over sample points only, so kept samples stay kept. `limit` truncates `corner` too.
5. `Balls::rolled` uses `tip` so Dots stay put. `particles::emit::ball_at` uses `History::place`.

## Not doing

- No change to hit times, last-hit ages, Restart-at-bounce timing or the strip mitering (`strip` already
  keeps each segment's own side at a sharp bend).
- No sampling rate change.

## Verification

- New runtime test: a fast ball (≥ 3000 px/s) bouncing in a box; for every hit, the history's trace has a
  point within 1 px of the ball's touching place, and it is missing before the fix (test written first).
- New kit test: the trail path of that ball passes within 1 px of the bounce, and its segment just before
  the bounce points along the ball's incoming velocity (within a few degrees).
- History unit tests: corners merge in order, are dropped with old samples, `travelled`/`tip` include them,
  `place` reads through a corner.
- Existing trail, particle, dots and thinning tests pass; `cargo test --workspace` (debug) passes; physics
  goldens unchanged (no BLESS); clippy and fmt clean.
