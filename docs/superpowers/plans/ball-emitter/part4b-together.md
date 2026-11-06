# Ball Emitter part 4b: Through bounces, all together — plan

Follow-up to part 4 (`part4-test-fired.md`, PR #100). Aedan, 2026-11-06, after trying it:

> When aiming at something through bounces, it should fire all balls no matter what unless it is genuinely impossible
> for it to reach the target eventually. It shouldn't be limited to reach the target in a certain amount of time.
> Maybe have an option where it fires the balls in the selected direction, and it simply adjusts the speed of each
> ball so that they all hit at the same time, whatever the earliest case would be. If a ball will never reach the
> target, then that would be okay for it to not fire. And/or an option for the aiming function to adjust the aim of
> each ball individually so that it can get all of them to hit the target within a certain time/# of bounces.
> Either way, I don't want the balls to be fired at different times unless specified.

He picked **both** direction modes on the decision card ("Both: keep or adjust directions").

## 0. What changes

Under Aim = Through bounces:

| Row | Choices | Default |
| --- | --- | --- |
| Arrive | As soon as all can / After (then **Arrive after** shows) | As soon as all can |
| Bounces at least | 0..20 (as now) | 1 |
| Bounces at most | 0..50 (never fewer than at least) | 10 |
| Leave | Together / When needed | Together |
| Ball directions (Leave = Together) | Adjust each ball / Keep mine | Adjust each ball |
| Turn at most (Adjust each ball) | 0..180° | 30° |
| Can't reach | Skip / Fire anyway (as now) | Skip |

- **Together**: every ball leaves on the round's firing step. A ball that can't find a way within the limits follows
  Can't reach (Fire anyway: it goes at Speed its own way, unaimed).
- **When needed**: part 4 as it was (own directions, speed nearest Speed, each leaves so it arrives with the others).
- **As soon as all can**: the arrival moment is the earliest at which every ball that has a way can be there going no
  faster than Speed: `T = max over balls of the first way's time` (see 2). No time limit: flights run until the ball
  has bounced more than Bounces at most (or two minutes, `aim::MOST_STEPS`).
- **After**: the arrival moment is Arrive after from the firing step, as before.
- Speed is the fastest a ball goes when finding T; balls slow down (or turn) to meet at T.

Other aims (Off, Straight in, Keep my directions) don't change; goldens don't move.

## 1. Why both direction modes (what the card says)

With a fixed direction, a ball passes through the target only at certain speeds when anything bends its path
(gravity, wind, drag, a bounce that isn't perfectly springy): each such speed arrives at its own moment, so
**Keep mine** can only meet when nothing bends the paths (an ellipse with Touch walls with Middle and no gravity,
where all speeds share one path and time scales as 1/speed). **Adjust each ball** turns each ball as little as it
needs (two unknowns, speed and direction, for two conditions, being at the target at time T), so all meet under
gravity too. The card says so when Keep mine is picked and something bends the paths:
"Gravity, wind or drag bend the paths: with Keep mine few balls can meet. Try Adjust each ball."

## 2. The search (test_fire.rs)

Leave together, per round (all balls set off from `earliest = now(fire)`, from one base lone copy):

**Phase 1, per ball (parallel), at most `MOST_FLIGHTS` (40):** its first ways, one per arc (bounces before), from the
fewest allowed bounces up, at most `WAYS` (3) arcs:
- Keep mine: the speed search as before (`Search::ways` with `Knob::Speed`: Speed first, then `SCAN` speeds evenly in
  ratio over `[Speed/8, min(ceiling, 8·Speed)]` when there's no time limit, or the part 4 range with one), best per arc
  = nearest Speed in ratio.
- Adjust each ball: at Speed exactly, the turn search (`Knob::Tweak(speed)`): no turn first, then `TURN_SCAN` (16)
  turns evenly over ±Turn at most; brackets of the miss on one arc closed in by Illinois; best per arc = smallest turn.
  A rough way (within `ROUGH` 3 px, the Edge sawtooth) counts: phase 2 makes it exact.

**T:** As soon as all can: `T = max over balls of their soonest way's e` (steps after setting off). After: `T = arrive -
earliest`, and phase 1 looks for ways up to `2T` (they may speed up).

**Phase 2, per ball (parallel), at most `SOLVE_FLIGHTS` (60):** be **at the target at time T** (not merely pass it):
the distance from the target at T (`off_at`, the place interpolated between the two steps around T) within `REACH`
(1 px), else the nearest found within `ROUGH` (3 px). Where the paths bunch up after a curved wall (a focus), some
places can't be reached closer than a few pixels by any speed or turn; a ball 3 px off is not visible.
- Keep mine: its own ways, those with `e ≤ T` nearest T first (they slow down), then the rest; 1-D on speed, the
  along-the-way part of the offset to zero (secant from `s1 = s0 − along/(T·dt)`).
- Adjust each ball: first, any way of the round (its own or another ball's, turned to its direction, within Turn at
  most) from the same start that is already at the target at T, turned least. Else, from the ways nearest T in time,
  `solve_turn`: a secant on speed along the ways through the target on that way's arc (at each speed, `through`
  finds the turn by secant steps then Illinois), the turn at a new speed predicted from the two nearest found; first
  slowed or sped as if the way stayed the same, then the other way (on a curved path, slower can be sooner); it
  stops where every way through on that arc takes about as long (a fold). (A 2-D Newton on speed and turn was tried
  first: near a focus its slopes are nearly parallel and the Edge sawtooth fools them, so it was dropped.)
- Then the balls without a way look again from the ways the others found, and any still without one borrow
  another's from the same start within Turn at most (`borrow_ways`). From one place, with both the moment and the
  bounces fixed, the ways that get there are few, so balls often share a path: variety comes from different bounce
  counts, Start from Ring or Line, or an ellipse's foci (every direction meets).

`RoundAim.arrive` = `earliest + round(T)`, `meet` = the target at `earliest + T`. `fire_round` gives each reaching ball
`arrive: Some(r.arrive)`.

Leave when needed: phase 1 as Keep mine; T as above (As soon as all can: the latest first way, all leaving at once);
then each ball's part 4 settle loop towards `A = earliest + T`.

Flights stop early once the ball has bounced more than Bounces at most (`fly(.., most_arc)`): no time limit costs
nothing more than the bounces asked for.

## 3. Files

- `emitter/mod.rs`: `AimArrive`, `AimLeave`, `AimWay` choices; `aim_arrive`, `aim_bounces_most`, `aim_leave`,
  `aim_way`, `aim_turn`; `when`s (Arrive after: `aim=straight_in or aim=keep_my_directions or aim=through_bounces and
  aim_arrive=after`).
- `emitter/test_fire.rs`: `Flight` keeps `rel` and the arc at each step and where the target caught it; `fly` takes
  `most_arc`; `Search::ways` (generalised `best`), `Search::solve_keep`, `Search::solve_turn`; `aim_round` two-phase;
  `aim_catch` the same for its round of one; `aim_ball` split into `first_ways` and `settle` (When needed).
- `emitter/sim.rs`: `fire_round` arrive from the round.
- `emitter/aim.rs`: `AimCheck.bends_keep`; through-bounces counts say "can't reach it" (no "in time" without a limit).
- `bpe-studio/src/emitter_card/mod.rs`: the new line; kittest for the rows.
- CLAUDE.md, this plan.

## 4. Tests (test_fire_tests.rs)

- Together, Adjust each ball, gravity, fan in a circle: every ball fires on the firing step, all within 1 px of the
  target at the round's arrival (± one step), at least one bounce each; none skipped.
- Together, Keep mine, ellipse foci (Touch Middle, no gravity), 16 balls: all fire together at their own speeds,
  arrive together, the fastest at Speed.
- Keep mine under gravity: fires only the balls that genuinely can meet (and says so through the check).
- After: arrival at Arrive after.
- Bounces at most: no way with more bounces; flights stop early (counted).
- When needed: part 4's staggered leaving, as before.
- The existing cache, catch-circle, caught-ball and spinning-shape tests, updated.

## 5. Not doing

- No change to the other aims, the cache, the background aiming or the preview.
- No per-ball varied Speed in the search (Speed is the card's).
