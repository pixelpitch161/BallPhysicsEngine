# Leave trails behind when an emitter catches a ball

Aedan (2026-11-05): "Make it so there is an option to not remove the trail of a ball once it is caught."
A follow-up to part 2 (the Emitter). Small and drawing-only.

## Design

- **Where:** a switch on the catching Emitter's Catch tab, **Leave trails** (`leave_trails: bool`, default off,
  group Catch, shown when Catches is Remove). Off keeps today's behaviour: a caught ball and its trail go at
  once. The other catch kinds keep the ball alive, so its trail is never lost there.
- **What a left trail does:** it runs out the way it would if the ball had stopped dead where it was caught.
  Its tail keeps going along the ball's old path at the speeds the ball went and slips into the catch point
  over the trail's own Length; then it is gone. Length by distance: the tail moves at the speed the ball was
  caught at (Length − speed × time since caught). A Line set to stay (Forever) stays for good. Its colours,
  widths and sub-layers carry on as for any trail (the Life gradient, the taper and the fade run on, so the
  head end thins and dims as it runs out).
- Only strip, Dots and Smoke trails. Ghosts (drawn with the ball), particles and wavefronts are unchanged.
  Balls removed for other reasons (Earlier rounds: Remove / Fade out) are unchanged.
- **Drawing-only:** toggling never rebuilds the simulation or the sound. The sim records, for every ball a
  catcher removes, who caught it and when, whatever the switch says; the drawing asks the catcher's (resolved,
  current) settings whether to leave the trail.

## Code

1. `bpe-kit/src/things/balls/rows.rs`: `BallRows.caught: Vec<Option<Caught>>` with
   `pub struct Caught { by: ThingId, step: u64 }`, pushed `None` in `push`, counted in `memory`.
2. `bpe-kit/src/things/emitter/mod.rs`: the `leave_trails` setting (after Things in Catch) and default false.
3. `bpe-kit/src/things/emitter/sim.rs`: `catch`, Remove arm records `rows.caught[row] = Some(Caught { me, step })`
   before `pop`. `affects_physics` says false for `leave_trails`.
4. `bpe-kit/src/modules/look/layers/trail/draw.rs`: `Balls.gone: &dyn Fn(usize) -> Option<f32>` (seconds since a
   removed ball was caught by a catcher that leaves trails; `None` for every other ball). `draw` draws such
   balls too (and counts them for the strip-point share). `Balls::path` takes it: `Cut::after_gone` gives `None`
   when nothing is left (a time cut no longer than the time gone, a distance cut used up by speed × time gone),
   and the still head (the samples since the catch, all on one point) collapses to its last point so no strip
   segment has zero length. Smoke puffs younger than the path's first point are skipped.
5. `bpe-kit/src/things/balls/draw.rs::with_balls`: builds `gone` from `group_of(..).rows.caught`, the catchers in
   `cx.scene` whose `leave_trails` is on, and `cx.time`. Other `Balls` builders pass `&|_| None`.

## Not to do

- Don't touch the physics, the log or `pop` (goldens must not move). Don't zero a removed ball's speed.
- Don't keep trails for balls removed any other way.

## Verify

- Sim test: a catcher with Remove records `caught` (catcher, step of its Enter) for each ball; Send back out and
  Pass through record nothing; `leave_trails` doesn't affect physics.
- Draw tests (shooter and catcher scene, Streak trail on the shooter's look, 0.5 s): just after the catch the
  frame has trail vertices only with Leave trails on; they shrink as time goes on and are gone after the trail's
  Length; a distance trail runs out too; a staying Line stays.
- `cargo test -p bpe-kit`, the goldens unchanged, `cargo clippy --workspace`, `cargo fmt`.

## Review fixes (one Opus review)

- The catch's Enter counted as a hit, so Restart at each bounce cut a left trail (even a staying Line) to nothing:
  `last_hits_but` leaves a left ball's catch out.
- Run-out trails no longer count for the strip-point share (`Balls::shows` asks `Balls::cut`).
- A distance trail of a slowly caught ball ends once the history no longer reaches back to when it moved.
- A staying Line's thinning is counted from the newest sample left after the still head is dropped.
