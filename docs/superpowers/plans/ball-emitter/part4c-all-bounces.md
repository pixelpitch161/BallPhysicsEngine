# Ball Emitter part 4c: Through bounces finds every ball's way — plan

Follow-up to part 4b (`part4b-together.md`, PR #101). Aedan, 2026-11-06 03:37Z:

> In a test with 500 balls, it says 494 of them can't reach the target. But when I put it so it fires all balls
> anyways, many of the balls reach the target, and a lot of them reach it within the 50 max bounces. Even if the
> catch range is set to 3 pixels.

## 1. What was wrong (measured)

Test: a circle of 508, an emitter at (-200, 0) firing 500 balls all round at 600 px/s, the target at (150, 120),
Bounces 1..50, Leave together, As soon as all can.

| Case | Aimed (part 4b) | Unaimed within 3 px in 60 s |
| --- | --- | --- |
| Keep mine, no gravity | **6** | 253 |
| Keep mine, gravity | 1 | 197 |
| Adjust 30°, no gravity | 370 | 253 |
| Adjust 30°, gravity | 83 | 197 |
| Adjust 10°, no gravity | 121 | 253 |

Causes:
1. Phase 1 looks for ways on only `WAYS` (3) arcs from Bounces at least, and its flights stop past them: the ways with
   4..50 bounces that the unaimed balls take are never seen (Keep mine: 6 of 500).
2. Adjust's phase 1 scans 17 turns 3.75° apart: on later arcs the miss swings too far between them to bracket.
3. Phase 2 (`solve_turn`) moves the speed at most 15% a try, 7 tries: a ball whose soonest way is much sooner than
   the round's moment `T` (the latest ball's) can't slow down enough, even with no gravity, where slowing down by
   `e/T` keeps the very same path and is exact.
4. Under gravity a ball's way changes when it slows, so the way to start from should be one whose time at Speed is
   already near `T` (more bounces): only the first 3 arcs were known.

## 2. The new search (test_fire.rs), leaving together

Keep mine becomes Adjust each ball with the turn limited to `MOST_TWEAK` (0.3°, unseen); otherwise both are one search.

**Groups.** Balls that set off from the same place (within 1e-3 px: Start from Point, or a ball of one) share their
test flights: a flight depends only on the start, the absolute direction and the speed.

**Phase 1, scan (parallel over all flights of the round).** Each ball adds directions evenly over its own window
(±Turn at most, or ±0.3° keeping its direction): an odd count `n = clamp(SCAN_BUDGET / (group size × speeds), 5, 17)`
(one ball gets 17; 500 from a point get 9 each, 4500 together, 0.08° apart all round). Speeds: Speed; keeping its
direction where gravity, wind or drag bend the paths and there's no time to arrive by, also 0.8, 0.64 and 0.5 ×
Speed (`KEEP_SPEEDS`); keeping its direction with a time to arrive by, `TIMED_SPEEDS` (8) speeds in ratio round what
gets there in time, as part 4 did. Each flight flies to Bounces at most (or the horizon, `MOST_STEPS` untimed, 2T
with Arrive after) and keeps only its passes per arc (no places).

**Leads.** Per group and speed, the flights sorted by direction (all round: the last and the first are neighbours):
each neighbouring pair whose misses on an arc (≥ Bounces at least) change side, both within `near`, is a *bracket
lead*; so is one direction at two neighbouring speeds; a flight passing within `ROUGH` on an arc is a *through lead*.
Its guessed moment is the ends' pass times interpolated by the misses.

**Closing a lead** (shared: `OnceLock` per lead, so a lead several balls want is flown once): a through lead is flown
again for its places (and brought within `TOL` by a turn of at most `HAIR`, 0.5°, when it can be); a bracket is
closed in on the direction (or speed) by Illinois (`close_in`, flights stopping past the lead's arc), at most
`CLOSE_FLIGHTS` (16). The result is a `Way` (absolute direction, speed, arc, e, places) or nothing (the arc broke
between the ends). Every ball that wants the lead pays its share of the steps (`Group.sharers`) whoever flew it, so
what a ball tries next never depends on which thread got there first (the answers stay the same run to run).

**Phase 2, soonest (parallel over balls).** A ball's leads are those with both ends in its window. Its soonest way:
its leads by guessed moment, closed in turn until one holds (at most `SOONEST_TRIES` 8, `BALL_STEPS`). Keeping its
direction where the paths bend, a ball the scan found nothing for searches its own speeds closer (`own_ways`: part 4's
`Search::ways`, up to `OWN_WAYS` 3 arcs) and what it finds joins the group's leads.
`T` = the latest soonest (As soon as all can) or Arrive after (as before). A ball with no lead in its window can't
reach and follows Can't reach.

**Phase 3, meet (parallel over balls).** Be at the target at `T` (`off_at` ≤ `REACH`, else the nearest ≤ `ROUGH`).
Its leads in order: where nothing bends the paths, the fewest bounces first, then the least turn (slowed or sped,
the way stays the same, and the fewer the bounces the less the steps' rounding moves it); where the paths bend, the
least change of speed first (|ln e/T|). Then:
1. **check** up to `CHECK_TRIES` (12) distinct ways (ways on one arc closer than `SAME_WAY` 0.2° are one): each
   slowed or sped by `e/T` on its own direction, one flight; there within `REACH`: done. Half the steps are kept for
   step 2.
2. **solve** the nearest `MEET_TRIES` (4) with `solve_turn`: the secant on speed along the ways through the target on
   that arc. Its first step is the whole `e/T` where nothing bends the paths (15% where they do); a direction that
   would need more than the ceiling is skipped; a pass whose time is far (`BRANCH`, 15% of `T`) from the nearest
   speed tried is another run of ways and counts as none.
Steps: `max(BALL_STEPS, MEET_FLIGHTS × T)` (`MEET_FLIGHTS` 30), flights `SOLVE_FLIGHTS` (150). Then `borrow_ways`
(same start, within the window, another ball's exact answer) as before. The second pass of part 4b (the others'
answers as first guesses) is dropped: the shared leads already give every ball the others' ways.

**Leave when needed**: each ball's way nearest Speed that is there by `T` (else its soonest), then part 4's settle.

**Other fixes.**
- A flight ends after `QUIET_DISTANCE` (6000 px) without touching a wall instead of ten seconds: a ball slowed a lot
  (to meet a late moment) still gets as far.
- No ball goes faster than the speed that splits the world's steps (`SAFE_MOVE` / dt = 3840 px/s): a faster one
  makes the world substep and every other ball's bounces change, so aimed balls missed.

**Found on the way, not fixed here.** An Edge bounce is not quite the same at every speed: the wall's normal is taken
where the ball was slid back to, not where it touched. A way slowed 2–8× drifts 0.5–15 px after 8 s (measured). Taking
the normal at the touch point roughly halves that (tried), but the crossing is still worked out on a straight line,
and it moves every golden; Keep mine, which can only turn 0.3°, is what it costs most. Left for a physics pass.

## 3. Files

- `emitter/test_fire.rs`: groups, `Lead`, `Way`, `scan`, `close_lead`, `soonest`, `meet` reworked; `first_ways`,
  `together_balls`' second pass and `WAYS`/`TURN_SCAN`/`SCAN` go; `solve_turn` takes a first guess.
- `emitter/aim.rs`: `Aimer.drag` (gravity, wind or drag bend the paths: `Aimer::bends`).
- `emitter/sim.rs`: the catch-aim `Aimer` keeps `drag`.
- `emitter/test_fire_tests.rs`: the 500-ball tests below; the existing ones kept.
- CLAUDE.md (Through bounces paragraph), this plan.

## 4. Tests

- `a_big_round_turning_each_ball_finds_every_ball_a_way`: 150 balls, Adjust 30°, no gravity, Bounces 1..50: ≥ 147
  aimed, all leave together, each really within `ROUGH` + 0.5 px of the target at the moment, after ≥ 1 bounce.
- `keeping_their_directions_ways_with_many_bounces_count_too`: 120 balls, Keep mine, After 10 s: ≥ 40 aimed, each
  really there, some after more than 4 bounces.
- `five_hundred_balls` (ignored, by hand): Adjust 500 of 500 (21 s release on 4 cores); Keep mine about 100 of
  500 (As soon as all can waits about 80 s for the slowest ball; 51 s).
- All part 4b tests pass; the When needed test needed the timed speed scan and nearest-Speed choice.

Measured after (500 balls, same scene; part 4b in brackets): Adjust 30° no gravity 500 (370); Adjust 10° 483 (121);
Adjust 30° gravity 251 (83); Keep mine no gravity about 100 (6), Keep mine After 10 s 233; Keep mine gravity 1 (1).

## 5. Not doing

- No change to the other aims, the cache, the background aiming or the preview.

## 6. Review (Opus) and what changed from it

- A ball's own answer is now held to its window: closing a lead may turn a hair past its ends (`HAIR`, `turn`), so
  `soonest`, `meet` and When needed skip ways outside ±Turn at most (±0.3° keeping its direction), and `solve_turn`
  counts a way outside as none. The big-round tests check every ball's turn.
- `BRANCH` compares a pass with when the nearest speed tried would pass going the same way at the new speed
  (`(late + T)·s0/s`), not with that speed's own time, so slowing a lot is no longer taken for another run of ways.
- Neighbouring flights are at most one ball's direction spacing apart, so with Turn at most ≥ 90° a lead never spans
  the back of a window.
- Flights also end after `QUIET_STEPS` (9600, 40 s) without touching a wall: one sliding in a bowl.
- A closed way keeps only its thinned places (the preview's): a ball meeting the others flies its own again.
- Left as is: per-ball budgets split shared closes between the balls that can turn to them, so they bound each
  ball's share, not the total (the count limits bound that); `ceiling_of` ignores gravity's share of a step.
