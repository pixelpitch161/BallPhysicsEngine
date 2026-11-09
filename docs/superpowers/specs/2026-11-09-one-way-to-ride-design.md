# One way to ride a parent (workstream W14: review findings G10 and C7)

Approved as part of the whole-code review work plan (Aedan, 8 November 2026: everything but the Size section).
Plan: `docs/superpowers/plans/2026-11-09-one-way-to-ride.md`.

## The problem

Riding was set four different ways:

- **Lights** had their own Rides on list: Each ball, The middle, Ball number, A point on the edge, The pen. A light
  inside a Text could ride its middle or its pen, but not each letter.
- **Particles** had another: Each ball, The middle, Ball number, The edge, Inside. A Particles thing inside a Text
  rode nothing (it was born at its own place).
- **Text** had a third: Each ball, The middle, Ball number. Inside a shape it sat at the middle and could not sit on
  the edge.
- **The Placement card** rides one ball by an "Item" number counted from 0 (-1 for all), where every other ball
  number counts from 1.

In the code (C7) each kind wrote its own "where is the parent now" (three copies of the middle of the balls, three of
the ball by number), two edge-point functions that differ (one wraps the share, one clamps it and also gives the way
out), and the even-odd "is this point inside the outline" test written five times.

## The design

### One choice, `bpe_kit::riding::RidesOn`

One enum, used by the Light, Particles and Text settings (saved names in brackets):

| Choice | Balls or Emitter parent | Shape parent | Text parent |
| --- | --- | --- | --- |
| Each ball (`each_ball`) | every ball | (the middle) | (the middle) |
| The middle (`the_middle`) | the middle of the balls | the shape's middle | the middle of the letters |
| Ball number (`ball_number`) | that ball (counted from 1) | (the middle) | (the middle) |
| The edge (`the_edge`) | each ball's rim | the shape's outline | the letters' outlines |
| Inside (`inside`) | inside each ball | inside the shape | inside the letters |
| Each letter (`each_letter`) | (the middle) | (the middle) | every letter showing |
| The pen (`the_pen`) | (the middle) | (the middle) | where Draw on is tracing |

Bracketed cells are not offered for that parent; a saved value that no longer fits (the thing was moved to another
parent) falls back to The middle, so nothing vanishes. Light's old `a_point_on_the_edge` is now `the_edge`.

What each rider makes of a spot:

- A **light** is one lamp a spot (Each ball, Each letter and The edge on balls give many). On The edge its
  **Round the edge** share (keyframable, 0 at the top, going clockwise) picks the point: round each ball's rim, round
  the shape's outline, or round all the letters' outlines in order (so a keyframed share runs a light along every
  letter). Inside is not offered (a lamp is one point).
- **Particles** are born at a random spot: a random ball, letter or point of the edge or inside. Steady's rate is a
  ball when it rides balls one by one (Each ball, The edge, Inside), and now a letter when it rides Each letter.
- A **Text** is one copy a spot. It gains **Round the edge** like a light (a label can sit on a shape's outline or a
  ball's rim, and travel round it with keys). A Text still can't ride a Text (Each letter, Inside and The pen are not
  offered for it; Inside because a label is one point).

The editor's Rides on dropdown offers only the choices that fit what the thing is inside (and its own kind), through a
new `ThingKind::offers` hook (default: every choice).

### What a Text parent gives

A Text parent is read as it shows: its letters laid out, placed on their path, moved by their Show move (Pop in, Draw
on...). Lights also see the Look card's letter moves, as they are drawn. Particles are stepped without the resolved
Look card, so they see the letters without its letter moves (noted in the notes as a known gap). A screen Text's
letters are turned into world places through the camera, as the pen already was. A Particles thing riding a Text
restarts when the Text's saved settings or its font change (the same hash a Particles thing made of a Text uses).

### The Placement card

Its Item row becomes **Rides on**: The whole group / One ball, with **Ball number** counted from 1 (stored as before,
so the runtime is unchanged).

### One riding module and one outline toolbox (C7)

- `bpe-kit/src/riding.rs`: `RidesOn`, which kinds each rider can ride (`RIDDEN_BY_TEXT` balls/emitter/shape and
  `RIDDEN` adding text), `offered`, `Ride` (the choice, ball number, edge share), `Ridden` (what a parent is now:
  balls, a shape's wall, a Text's letters, or nothing the world moves), `Ridden::spots` (every fixed spot, for lights
  and texts) and `Ridden::pick` (one random spot from dice, for particles), and `Spot` (where, how fast, the way out,
  the ball, its radius).
- `bpe-kit/src/outline.rs`: `centre`, `points`, `contours` (moved from `light/draw.rs`), one `round` (a share of the
  way round one or several closed contours by length, with the way out), and `inside`/`inside_any` (even-odd), which
  every copy of the inside test now calls. The crossing test itself is one function, `bpe_core::shape_path::crosses`,
  which `inside_polygon`, the runtime's letters, the laser, the glyph tiles and the riding module all use.

### What must not change

- Goldens: no ball moves differently, no particle is born differently for the choices that existed (the new code
  keeps every float operation in the same order). `golden*.txt` must not change.
- A light, particle or Text riding balls, a shape or nothing is drawn exactly as before, except the edge share now
  always wraps (1 is the top again, as 0 is).
- Old scene files: no compatibility needed (owner's rule), but the renamed `a_point_on_the_edge` simply reads as the
  default.

## Not in this workstream

- A Text riding a Text, and riding the Look card's letter moves from the particles' step.
- Particles or lights riding a Rig's lamps or another Particles thing (Fed by its parent's particles already covers
  particles from particles).
