# Small links between features (workstream W12): design

Date: 2026-11-08. Source: the whole-codebase review's "Gaps and inconsistencies between features" section (findings G2,
G3, G5, G6, G7, G9, G12, G13, G14), which Aedan approved with everything except the Size section. Each finding names
its fix; this spec settles the details the review left open. Plan: `docs/superpowers/plans/2026-11-08-feature-links.md`.

Nothing here moves a ball: goldens must not change. Old scene files don't need to load the same.

## G2. A light can point at a thing

- `Points` gains **At a thing** (`Points::AtAThing`). Two new light settings, shown only for it (and never for a Point
  light): **Aims at** (`aim_at: Target`, default every Balls thing) and **Aims at ball** (`aim_ball: u32`, default 1;
  0 = the middle of the group's live balls).
- The aim point is the point of the first thing `aim_at` names (in id order, the light itself left out) that has one:
  a thing with balls gives ball `aim_ball` (by its number, when alive) or the middle of its live balls; a wall (shape,
  walled Text) its outline's middle; anything else its pose. Nothing there: the light keeps its Direction.
- Every lamp of the light aims on its own (a spot riding each ball points each one at the target). A laser or beam
  aimed this way is not cut short (unlike At the pen); it meets the thing like anything else.
- The light probe (Follow light) and the editor's handles see the same aim. The aim handle can't be dragged while the
  light points at a thing or the pen.

## G3. Shapes take the letter moves

- Wave, Jump on the beat, Jump on hit, Flicker, Follow sound, Shake, Spin and Scatter and come back now fit a Shape's
  Look card as well as a Text's (Draw on and the experimental effects stay Text-only). A shape wears them as one letter
  (letter number 0): its "Only some letters" list is ignored.
- Drawing only, as on a Text: the wall the balls hit doesn't move. The move's offset is in the picture's own frame (up
  is up, even on a spinning shape), its turn adds to the shape's turn, its size and stretch scale the drawn outline
  about its middle (stretch in the shape's own frame; a circle becomes an ellipse), and its opacity fades the whole
  shape (with a Break look's fade on top).
- Jump on hit's "Hits of: Itself" means hits on the shape.
- The editor shows a **Moves** list on a shape's Look card (the add menu offers the eight moves).

## G5. The camera follows more

- Follow what may name Particles, Lights and Rigs too:
  - a Particles thing (Exact engine) counts once, at the middle of its live particles, and Fit all keeps every
    particle in the shot (its size included). A Huge swarm thing gives no point (its particles live on the graphics
    card). A followed Particles thing steps with the physics, as one a rule reads does (new `ThingSim::keeps` hook,
    read by `is_beside`).
  - a Light counts once per lamp, where it shines from (its Position, keyed or not, or what it rides); a Rig counts
    each lamp of its lights. Light scenes don't move them. The camera's fingerprint hashes the followed lights, so an
    edit to one restarts the camera's run.
- Fit all uses the real outline of a shape or a walled Text (the box round its outline, its band included) rather than
  its middle alone. The followed middle doesn't change.

## G6. Follow light offers Text

- Texts appear in the Follow light menu with Light on it, Brightest light on it, Shadow on it and In shadow (the probe
  already senses each letter round its outline).

## G7. Follow sound hears Particles

- The Follow sound menu lists Particles things whose sound is on (their modulators and Level), and buses show whenever
  anything has a voice.
- A Particles thing whose sound a setting follows (by its own Level or modulator, or through a bus, when it is heard)
  steps with the physics, so its moments are in the log Follow sound reads. `ControlSampler` reads moments as the
  engine plays them (`hits::plays`).

## G9. Particles fade when they break, and lights flash

- A Particles thing can show the Break looks Fade out and Pop (Pop fades it; Shatter fades it over the Shatter's time).
  A Break rule naming a Particles thing keeps it in the main world, so the editor and the export see the same look.
- A Flash rule aimed at a Light (or a Rig: each of its lights) spikes its strength as a flare would: a flash of
  Strength f adds f × (1 − age)² over the light's Flare length, times its Flare (or × 1 when Flare is 0). Follow light's
  "Strength as it shines" sees it too.

## G12. Effects and Ripples see letters

- Each shown letter of a Text is a Ripples source (its contours, one owner a letter, counted as a shape).
- Each shown letter joins the effects' ball list (`ball_count`, `nearest_ball`, …) as a round mark at its middle, as
  big as its box, in the Text's colour, with its last hit.
- Particles are left out (too many), as before.

## G13. One name and one picker for "whose hits"

- Every setting that picks whose hits count is labelled **Hits from** (lighting scenes, lights, rigs, the voice, Health,
  Particles, Text's Show, letter moves and effects, the camera). The Distance and Light modulators' "Heard from" is
  about which balls they measure, so it becomes **Which balls**.
- The setting picker and the rules' who picker read alike: Anything, Itself, Thing, Tagged, Any (kind).

## G14. One way to set launch, units and word fonts

- A pair in px/s (a ball's Launch speed, a light's Gobo drift) shows as **speed** and **angle** (degrees, 0 to the
  right, 90 down), as the Emitter and Particles set them. It is still saved, keyed and varied as x/y.
- Units say "°" and "°/s" everywhere (no "deg").
- The Balls' typed Word and Particles' typed word take a **Font** (the Text fonts, with the same picker). The default
  is IBM Plex Sans, which draws exactly as before (the Particles' Weight picks its weight; other fonts take the weight
  file nearest it).
