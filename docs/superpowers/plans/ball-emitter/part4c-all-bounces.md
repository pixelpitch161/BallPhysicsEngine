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
(±Turn at most, or ±0.3° keeping its direction): an odd count `n = clamp(SCAN_BUDGET / group size, 5, 17)` (so one
ball gets 17, 500 from a point get 9 each, 4500 together, 0.08° apart all round). Speeds: Speed; keeping its direction
when gravity, wind or drag bend the paths, also 0.8, 0.64 and 0.5 × Speed. Each flight flies to Bounces at most
(or the horizon, `MOST_STEPS` untimed, 2T with Arrive after) and keeps only its passes per arc (no places).

**Leads.** Per group and speed, the flights sorted by direction (all round: the last and the first are neighbours):
each neighbouring pair whose misses on an arc (≥ Bounces at least) change side, both within `near`, is a *bracket
lead*; a flight passing within `ROUGH` on an arc is a *through lead*. Its guessed moment is the ends' pass times
interpolated by the misses.

**Closing a lead** (shared: `OnceLock` per lead, so a lead several balls want is flown once): a through lead is flown
again for its places; a bracket is closed in on the direction by Illinois (`close_in`, flights stopping past the lead's
arc), at most `CLOSE_FLIGHTS` (16). The result is a `Way` (absolute direction, speed, arc, e, places) or nothing (the
arc broke between the ends).

**Phase 2, soonest (parallel over balls).** A ball's leads are those with both ends in its window. Its soonest way:
its leads by guessed moment, closed in turn until one holds (at most `SOONEST_TRIES` 8).
`T` = the latest soonest (As soon as all can) or Arrive after (as before). A ball with no lead in its window can't
reach and follows Can't reach.

**Phase 3, meet (parallel over balls).** Be at the target at `T` (`off_at` ≤ `REACH`, else the nearest ≤ `ROUGH`).
Its leads, those with `e ≤ T` (or that may speed up within the ceiling, with Arrive after) nearest `T` first, then
the rest by `e`; for each (closed if needed), turned to its own direction:
1. already there at `T`: done;
2. slowed (or sped) by `e/T` on the same direction: done when there (no gravity: always, the same path);
3. `solve_turn` from there: the secant on speed along the ways through the target on that arc, the first step now
   the whole `e/T` (no 15% clamp on the first step).
At most `SOLVE_FLIGHTS` (60) flights of its own and `MEET_TRIES` (6) leads. Then `borrow_ways` (same start, within
the window, another ball's exact answer) as before. The second pass of part 4b (the others' answers as first guesses)
is dropped: the shared leads already give every ball the others' ways.

**Leave when needed** keeps part 4's settle, from each ball's soonest way (phase 2) of a Keep-mine scan.

**Budgets.** `SCAN_BUDGET` 4096 directions a group (`n` ≥ 5 a ball). One flight is cut at `MOST_STEPS`, Bounces at
most, at rest a second or `QUIET_STEPS`. A search's step budget: `BALL_STEPS` per ball (phase 3), per close
`CLOSE_FLIGHTS × horizon`.

## 3. Files

- `emitter/test_fire.rs`: groups, `Lead`, `Way`, `scan`, `close_lead`, `soonest`, `meet` reworked; `first_ways`,
  `together_balls`' second pass and `WAYS`/`TURN_SCAN`/`SCAN` go; `solve_turn` takes a first guess.
- `emitter/aim.rs`: `Aimer.drag` (gravity, wind or drag bend the paths: `Aimer::bends`).
- `emitter/sim.rs`: the catch-aim `Aimer` keeps `drag`.
- `emitter/test_fire_tests.rs`: the 500-ball tests below; the existing ones kept.
- CLAUDE.md (Through bounces paragraph), this plan.

## 4. Tests

- 500 balls all round from a point, no gravity, Keep mine, Bounces 1..50: at least as many aimed as reach unaimed
  within 3 px (253), all leave on the firing step, a sample of them within `ROUGH` of the target at the moment.
- The same with Adjust each ball 30°, and with gravity (Adjust): far more than part 4b (370 / 83), checked in the
  real run for a sample of balls.
- Time: each of those under 30 s in release on the test machine (8 threads).
- All part 4b tests still pass (goldens don't move: no other aim changes).

## 5. Not doing

- No change to the other aims, the cache, the background aiming or the preview.
