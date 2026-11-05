# Ball Emitter, Ellipse and arrive-together aiming: design

Date: 2026-11-05. Owner: Aedan. Status: design approved in chat (design doc
https://claude.ai/artifact/Dkwx5Ssc5APCyWH8SrwSAS, "looks good"), Emitter card layout C picked
(mockups https://claude.ai/artifact/RMBBjFkUKjbcSRxjur2xwc).

## 1. Why

Two reference reels (instagram.com/reel/De2UFHPxTkI and instagram.com/reel/DfFaE7fNPAy) show balls fired
from one point that all arrive at another point at the same moment, wave after wave, with ever more balls.
Both use the **ellipse focus property**: every path from one focus to the wall and on to the other focus has
the same length (twice the semi-major axis), so balls fired at one speed in every direction from a focus all
reach the other focus together, with no aiming. Then they carry on and meet again at the first focus, and
over many bounces their paths gather onto the long axis.

- Reel 1 (36 s, upright ellipse): markers on both foci; one ball first; each time a wave lands on a focus,
  that focus flashes and fires more (1, 2, 4 … hundreds); colour by launch angle (rainbow); thin line
  trails that stay; wavefronts circle → lens → circle; ripple rings at the end.
- Reel 2 (35 s, tilted ellipse): "RAYS: 10 / 100 / 1000" counter; each round a full circle of balls from
  one focus; white dot heads joined by a white wavefront line; flash ring at the other focus when it lands;
  rounds fade out and the next has ten times as many.

Aedan's idea: a "Ball Emitter" thing that emits (a ball designed like a normal ball, how many, how, variation,
delay between balls), receives (deletes balls that enter) or returns (sends the same balls back out), and can
aim so all balls arrive at a chosen receiver at the same time.

## 2. Decisions (Aedan's)

1. Aiming: both, in order. First exact maths for straight and thrown shots (plus the ellipse trick, which
   needs no aim). Later, aiming through bounces off any shape by test-firing.
2. One **Emitter** thing that can emit, catch, or both (not separate Emitter/Receiver things, not a Balls
   mode, not a link to a Balls design). It owns its ball design: the full ball Look editor (Group mode) and a
   Physics card, as a Balls thing has.
3. Emitter card layout **C**: Emit | Catch | Marker chips; the Emit tab has a mini view (drag aim and speed)
   and a rounds strip over time, then the setting rows.
4. Standing rules: no built-in or example scenes; no backward compatibility needed; plain-language UI.

## 3. Parts and build order

| Part | Adds | Depends on | Can run in parallel with |
| --- | --- | --- | --- |
| 1 Ellipse | Exact ellipse outline, Width/Height/Turn, foci, Touch walls with middle | nothing | 2, 5 |
| 2 Emitter | The thing: Emit, Catch, rounds, triggers, marker, ball design, Wavefront layer, gradient spreads, editor card C | nothing (Sits on a focus needs part 1's `foci`; build it against a small trait and wire the ellipse when 1 is in) | 1, 5 |
| 3 Arrive together | Aim: Straight in / Keep my directions | 2 | 1, 5 |
| 4 Test-fired aim | Aim: Through bounces | 3 | 5 |
| 5 Vary any setting | Right-click > Vary across balls, for Balls groups and emitters | nothing | 1, 2, 3 |

Merge order for shared code: 1 before 2 (2 rebases to wire Sits on); 2 before 3; 5 rebases on whatever is in.
Each part is its own plan, thread, review and PR.

Out of scope: on-screen text such as the reel's "RAYS: 10" counter (no Text thing since the strip-down), and a
sound for catches. Shape sounds still play when emitted balls hit shapes.

## 4. Part 1: Ellipse shape

### 4.1 Settings (`things/shape/mod.rs`)

- `ShapeStart` gains `Ellipse` (after Polygon and Circle; Circle stays the default).
- New settings, shown only when `start=ellipse`, in the Body group (so they show in the Look card's Body like
  the other `BODY_HEADINGS` settings):
  - `width`: "Width", px, span(20, 2000), range(1, 20000), default 900, simple, keyframe.
  - `height`: "Height", px, same ranges, default 1300, simple, keyframe.
  - `turn`: "Turn", °, span(-180, 180), default 0, keyframe. Clockwise, like every angle in the program.
- `Size` (`radius` today) is hidden for ellipses (`when = "start!=ellipse"`); Width/Height take its place.
  Shrink scales both (the same factor as a circle's radius), Spin turns it on top of Turn.
- Changing Start from to Ellipse removes the path in the same edit (as Sides does today).

### 4.2 Physics (`bpe-runtime/src/walls.rs`)

- New `Outline::Ellipse { center, half: Vec2 /* semi-axes */, angle, thickness }`.
- Closest point on an ellipse: work in the ellipse's own frame, use the robust distance-to-ellipse
  method (Eberly's bisection on the parametric root; f64 inside, at most 64 iterations, exact for the axis
  cases a=b and points on an axis). Normal = the gradient of the implicit function at the closest point.
  Two-sided like `Ring`: a ball keeps to the side its centre was on at the start of the step
  (`Bodies.previous`), hollow by default, `Wall.solid` for Filled.
- `base_outline` (`things/shape/sim.rs`) returns `Outline::Ellipse` for `start=ellipse` and `Ring` when width
  equals height. Spin and Shrink act on it as on a Ring (`angle` turns, `half` scales with `growth`).
- Every `match` on `Outline` handles the new arm: walls, `testing.rs`, particles `emit.rs`/`sim.rs`
  (particles bounce off it), shape `draw.rs` (`wall_form`), `bpe-gfx/src/frame.rs`, studio
  `shape_handles.rs` and `preview/zones.rs`, `bpe-core/src/shape_path.rs` (Turn into points samples it).
- **Touch walls with** (new setting on the Bounce card, so on Balls and Emitter Physics cards): "Edge"
  (default, today's behaviour) or "Middle". With Middle the wall test uses radius 0 for that ball, so its
  centre bounces exactly on the outline. This is what makes balls from one focus meet *exactly* at the
  other; with Edge they meet within about one ball size. Ball-to-ball pairs still use the real radius.

### 4.3 Foci

- `bpe-kit/src/things/shape/foci.rs`: `pub fn foci(world_or_resolved…) -> Option<[Vec2; 2]>`: for an
  ellipse, centre ± c·(cos turn, sin turn) along the long axis with c = sqrt(a² − b²) (a the larger
  semi-axis); a circle's foci are both its middle; polygons and moulded paths have none. Computed from the
  shape's current pose (spin, shrink and keyframed Width/Height at that moment), so a focus moves with it.
- The editor draws the two foci of a selected ellipse (and circle) as small `text`-coloured ring marks.
- Drawing: shape paint layers draw an ellipse through `paths.wgsl` as an exact form (an ellipse code in
  `PathInstance` analogous to the circle-with-no-points case; `shape_at` gives the exact distance with the
  same method as physics, f32, a few iterations), so Outline, Glow and Along-the-edge gradients follow it.
- Turn into points (right-click) samples the ellipse into a moulded path of 16 curve corners (cubic
  approximation); the inspector then notes "No longer an exact ellipse: balls from a focus will no longer
  meet perfectly."
- Shape sound "By side" treats an ellipse as one side.

### 4.4 Tests

- walls: closest point and normal against a brute-force sample (10⁵ points) for several a/b, inside and
  outside points, on-axis points, a=b reduces to Ring results bit for bit.
- Focus property: 360 balls (radius 1, Touch walls with Middle, no gravity, no pairs) fired from focus 1
  at one speed all pass within 0.5 px of focus 2 within the same step ± 1.
- Goldens unchanged (no existing scene uses an ellipse); add one golden test scene (test-scenes feature
  only, never in the program) `ellipse-focus`.
- GPU picture test: an ellipse shape with Outline layer draws within 1 px of the true outline.

## 5. Part 2: The Emitter

### 5.1 The thing

- Kind id `emitter`, family "Bouncers", name "Emitter", icon a ring with a dot, help "Fires balls it
  designs itself, catches balls that reach it, or both."
- Cards: its own settings (shown as the **Emitter** card with Emit | Catch | Marker chips), **Look**
  (`look.layers`, which now fits `balls` and `emitter`), **Physics** (Motion + Bounce, as balls).
  `starters.rs` gives a new emitter the same starting Look/Motion/Bounce as a new Balls thing, except
  Motion gravity follows the scene (as balls) and Bounce pairs off.
- Its balls are body rows of the emitter thing (`world.bodies[emitter]`). They go through exactly the same
  drawing as a Balls group: the Balls draw code is generalised over "a thing with ball rows" (numbers, size
  factors, settings the look needs) so Balls and Emitter share it; nothing ball-specific is copied.
  Everything keyed on kind `balls` for ball looks accepts `emitter` too: `look.layers` `fits`, starters,
  the Look editor's Ball/Group switch and Group view, trails/particles history wants, camera "follow balls"
  target, sound modulators that read ball numbers (Hit modulator "ball number", Distance "each note's
  ball"), Follow sound.
- `EmitterState` holds what `BallsState` holds (numbers, size factors) plus: each row's round, launch
  angle (radians, the direction it was fired in, before speed spread), born step, fade-out start step (or
  none); the round counter; the ball count of the next round; pending launches `(step, …)` soonest last;
  held balls for Send back out; waves seen per catcher. `ball_number` and `numbers()` work for it.

### 5.2 Emit settings (keys and defaults)

| Key | Label | Type / range | Default | Notes |
| --- | --- | --- | --- | --- |
| `emits` | Emits | bool | true | |
| `radius` | Size | 0.5..200 px | 6 | Ball size, as Balls; shown beside the Look card's Ball/Group switch |
| `sits_on` | Sits on | Free / a shape's Focus 1 / Focus 2 | Free | Picker lists ellipses and circles; part 1 needed for foci |
| `start_from` | Start from | Point, Ring, Line | Point | |
| `start_radius` | Radius | 1..4000 px | 60 | when Ring |
| `start_length` | Length | 1..4000 px | 300 | when Line; the line is across the aim direction |
| `directions` | Directions | All round, Fan, One way, Random | All round | |
| `aim_angle` | Aim direction | -180..180 ° | -90 (up) | Middle of a fan, the one way, where "Around" order starts |
| `fan_angle` | Fan angle | 0..360 ° | 90 | when Fan |
| `speed` | Speed | px/s, span 50..2000 | 600 | |
| `vary_speed` | Speed spread | 0..1 | 0 | |
| `count` | How many | 1..20000 | 10 | First round's balls |
| `gap` | Gap | 0..30 s, span 0..1 | 0 | Between balls in a round |
| `order` | Order | Around, Random, Middle out | Around | when gap > 0 |
| `rounds` | Rounds | 1..1000, plus Endless | 1 | |
| `grow_times` | Each round × | 0.1..100 | 1 | Next count = round(count × times + plus) |
| `grow_plus` | Each round + | -10000..10000 | 0 | |
| `fire_when` | Fire when | At start, Every, When a wave lands on | At start | |
| `start_delay` | After | 0..600 s | 0 | when At start: first round's time |
| `every` | Every | 0.05..600 s | 4 | when Every; first round at `start_delay` |
| `wave_from` | On | an emitter (itself included) | itself | when When a wave lands on; each landing fires one round |
| `earlier` | Earlier rounds | Keep, Fade out, Remove | Keep | What happens to earlier rounds' balls when a new round fires |
| `fade_time` | Fade time | 0.05..30 s | 1 | when Fade out; ball and its trails fade, then the ball is removed |
| `most_balls` | Most balls | 1..100000 | 20000 | Live balls of this emitter; a round is cut short at the cap |
| `aim` | Aim | Off (part 3 adds Straight in, Keep my directions; part 4 Through bounces) | Off | |

Rules:

- The k-th ball of a round of n gets direction: All round `aim + 360°·k/n`; Fan evenly across
  `aim ± fan/2` (one ball: `aim`); One way `aim`; Random seeded by (scene seed, thing, round, k).
  Speed × (1 ± spread·r) with r seeded likewise. Start place: Point = the emitter; Ring = on the ring at
  the ball's own direction; Line = evenly along the line across `aim` (one ball: the middle).
- Order with gap > 0: Around (k = 0..n), Random (seeded shuffle), Middle out (from the middle of the fan or
  line outwards, alternating sides).
- Ball numbers count up across rounds in firing order (round 1 is 0..n-1, round 2 continues), so group
  gradients and "only these balls" layers work across rounds. Each ball also keeps its round and launch
  angle (5.5).
- A round's launches are pending entries released in `ThingSim::spawn` at their step, logged as `Spawn`
  events (as Balls' Release does). The first ball of a round also marks the round start for the timeline.
- `most_balls` counts live rows; balls past the cap in a round are not fired (the round is cut short).
- Endless rounds with "When a wave lands" can chain two emitters forever; the cap stops growth.
- An emitter that Sits on a focus moves with it every step (its pose is set in `scripted` from the shape's
  pose in the same step, so a spinning ellipse carries it).

### 5.3 Catch settings

| Key | Label | Choices / range | Default |
| --- | --- | --- | --- |
| `catches` | Catches | Nothing, Remove, Send back out, Pass through | Nothing |
| `catch_size` | Catch size | 1..2000 px | 40 |
| `catch_from` | From | Any balls, Its own, Chosen things (target picker) | Any balls |
| `return_way` | Which way | Same way back, Out my way | Same way back |
| `return_delay` | Delay | 0..30 s | 0 |

- A ball **arrives** when its centre enters the catch circle (it was outside at the step's start and is
  inside after it, or crosses it within the step: swept segment test against the circle). A ball that starts
  inside (just fired) does not arrive until it has left and come back. Arrivals are logged as `Enter` events
  (`a` the ball, `b` the emitter) in the emitter's `after_step`, in tree order, so the log stays deterministic.
- Remove: the ball's `alive` goes false with a `Remove` event (the existing `pop`), on whichever thing owns it.
- Send back out: the same row keeps its owner, look, trail and number. Same way back reverses its velocity;
  Out my way gives it this emitter's direction for its place in the wave (its arrival order k of the wave's
  n) and Speed (and, from part 3, this emitter's aiming). With a Delay it is held still at the catch point
  (velocity zero, ignored by walls and pairs) until released.
- Pass through: nothing happens to the ball; it still arrives.
- **Wave**: the arrivals at one catcher of balls from one (owner, round). A wave **lands** at its first
  arrival; arrivals of the same (owner, round) within the next 0.5 s join it; a later one starts a new wave.
  Balls of a Balls thing count as round 0 of that thing. Landings drive: "Fire when a wave lands on" (one
  round per landing), the marker's flash and ripple, and the balls' own Flash on hit / Colour on hit
  reactions (an arrival counts as a hit for those reactions, not for sound or physics).

### 5.4 Marker settings (visual only, `affects_physics` false)

| Key | Label | Default |
| --- | --- | --- |
| `marker` | Marker: None, Ring with dot, Dot, Ring | Ring with dot |
| `marker_size` | Size, px | 12 |
| `marker_colour` | Colour | warm gold `#ffc46b` |
| `marker_glow` | Glow, 0..1 | 0.4 |
| `flash_fire` | Flash when it fires, 0..1 | 0.5 |
| `flash_land` | Flash when a wave lands, 0..1 | 0.9 |
| `flash_fade` | Fade, s | 0.4 |
| `ripple` | Ripple, bool | true |
| `ripple_rings` | Rings, 1..10 | 3 |
| `ripple_far` | How far, px | 300 |
| `ripple_fade` | Fade, s | 1.2 |

The marker draws in the emitter's own `draw` (rings and dot as sprites or path instances), the flash as a
soft glow growing with strength and fading over `flash_fade`, ripples as thin rings expanding to
`ripple_far` while fading. Everything is worked out from the log at the moment drawn (no state between
frames), so scrubbing and export match. It never collides.

### 5.5 New look spreads and the Wavefront layer

- Group gradient `Spread` gains **Round** (a ball's round, first to last) and **Launch angle** (the
  direction it was fired in, around the colour wheel: 0..360° maps across the gradient). For a Balls thing
  Round is 0 for every ball and Launch angle is the angle of its launch velocity.
- New around-layer **Wavefront** (`layer.wavefront`, `LayerSection::Around`, on Balls and Emitter):
  draws a line through the live balls of each round (Balls: the whole group) in number order, closed when
  the round's Directions are All round (setting "Closed": Auto / Yes / No). Settings: Width (px), Colour /
  gradient (as a trail), Softness, Opacity, "Break when apart" (px: skip a segment longer than this; default
  200, so the line doesn't cross the screen when neighbours split). Takes the around sub-layers
  (`AROUND_SUBS`). Drawn as a trail strip (`TrailVertex`) through `Frame.trails`.
- Fade out: a ball whose round is fading draws (body, layers, trails, particles, wavefront) multiplied by
  its fade (1 → 0 over `fade_time` from the step the next round fired), then is removed with a `Remove`.

### 5.6 Editor (layout C)

- Add menu: Emitter under Bouncers. Tree icon: ring with dot.
- Emitter card: chips **Emit | Catch | Marker** (state kept per inspector like `InspectorTab`).
- Emit tab, top to bottom: **mini view** (170 px, `ground` well): the emitter, one round's directions as
  rays coloured by each ball's look colour, the Start ring or line, the catch circle dashed, a square aim
  handle (drag to set Aim direction), fan edge handles (drag to set Fan angle), ray ends (drag to set Speed:
  ray length = distance flown in 0.5 s). Then the **rounds strip**: a lane over the clip length showing each
  round as a block with its ball count, read from the playback log's round starts (exact for every Fire
  when). Then rows: Fire (Emits, Sits on, Start from, Directions, Aim direction, Fan angle, Speed, Speed
  spread, How many, Gap, Order), Rounds (Rounds, Each round × and +, Fire when, After/Every/On, Earlier
  rounds, Fade time, Most balls), Aim. The mini view and strip use the design system tokens only; scene
  colours on them are marked `// scene colour`.
- When the emitter sits on a focus of an ellipse with Aim Off, a hint under Sits on reads "Balls from here
  meet at the other focus." and, when gravity or Edge touching would spoil it, "…if gravity is off and
  balls touch walls with their middle."
- Catch tab and Marker tab: plain rows as in the mockup's fourth board.
- Look card: the existing Look editor with Ball / Group switch. Group mode for an emitter shows one round
  laid out a short way along its directions (so Launch angle gradients read), with Variety and Odd ones
  out folds; Release/Launch folds are not shown (the Emit tab has them).
- Preview: a selected emitter shows its catch circle, Start ring/line and (part 3) aimed paths; it can be
  dragged; near a focus of an ellipse or circle it snaps (sets Sits on); dragging it away sets Free.
- Timeline: the emitter's group shows a tick per round start; Hits lane as usual.
- Right-click menus on every setting as everywhere (Copy / Paste / Create Keyframe). Keyframable: Speed,
  Aim direction, Fan angle, How many, Gap, Every, Catch size, marker settings.

### 5.7 Tests

- Firing: directions/start places for each Directions × Start from; round sizes with ×/+; Gap and Order
  timing on exact steps; seeded Random stable.
- Fire when: At start, Every, When a wave lands (chain of two emitters, fires one round per landing);
  Endless stops at Most balls.
- Catch: arrival only on entry (fired balls inside don't count); swept entry at high speed; Remove logs
  Remove; Send back out keeps row and look, reverses or re-aims; Delay holds; Pass through untouched;
  waves group by (owner, round) within 0.5 s.
- Earlier rounds: Keep, Fade out (fade value and removal step), Remove.
- Sits on: follows a spinning, shrinking ellipse focus.
- Drawing: GPU picture tests for marker, flash, ripple, Wavefront, fading round; Launch angle gradient.
- Studio (kittest): chips switch tabs; mini view aim drag changes `aim_angle`; strip shows round blocks;
  snapping sets Sits on.
- Goldens: existing goldens unchanged; new test scenes `emitter-rounds`, `emitter-chain` (test-scenes only).
- Determinism: export twice, same frames and audio hash; scrub to a time = play to it.

## 6. Part 3: Arrive together

- `aim` gains **Straight in** and **Keep my directions**, with `aim_target` (an emitter or any thing's
  middle; default the other focus when Sits on a focus, else none) and `arrive_after` (s, 0.1..60,
  default 2) and `cant_reach` (Skip / Fire anyway, default Skip).
- Let Δ be the target's position at the moment of arrival minus the ball's start (a target that moves by
  keyframes or riding is evaluated at that moment through the world's pose at that step; spin of a parent
  included), g the constant acceleration on the ball (scene gravity + the ball's own Motion pull), T the
  arrival time after the round fires.
- Straight in: each ball launches at the round's fire time (plus its Gap slot) with
  `v = Δ/t − g·t/2`, t = T − its slot delay. Directions and Speed are ignored (shown greyed).
- Keep my directions: each ball keeps its direction u. With n ⟂ u: flight time t² = 2(n·Δ)/(n·g) (needs
  t² > 0), speed s = (u·Δ − (u·g)t²/2)/t (needs s > 0 and s ≤ the speed ceiling). Without gravity only
  u ∥ Δ reaches (t = |Δ|/Speed). Each ball launches at T − t after the round fires; a ball with t > T, or no
  solution, follows `cant_reach`.
- Exact only when nothing else acts in flight. The inspector warns when the Physics card has drag, wind,
  speed limits, pairs on, or the ball's path (sampled) crosses a wall: "Something in the way: balls may miss."
- Preview: faint aimed paths for every ball of the next round (at most 400 drawn).
- Tests: arrival within one step and 0.5 px of the target with and without gravity for both styles, a
  moving target, can't-reach skipping, ceiling.

## 7. Part 4: Through bounces (test-fired)

- `aim` gains **Through bounces**: for each ball direction, find the speed (and launch delay) so the ball
  passes through the target at arrival time T, bouncing off any walls and moved by any Motion, by firing a
  lone copy of the ball in a stripped world (walls, the ball's Motion/Bounce, no other balls) and searching
  speed (bracket + bisection on the signed miss across the target, at most 40 test flights per ball).
- Results are cached per (scene physics fingerprint, emitter settings) and computed on a background thread in
  the editor (the preview shows "Aiming… 37%"); export waits for them. Balls with no solution follow
  `cant_reach`.
- Detailed design happens in that part's plan; this spec fixes only the setting, the cache key and the
  "lone ball in a stripped world" method.

## 8. Part 5: Vary any setting across balls

- Right-click on any ball setting or layer setting of a Balls group or Emitter (numbers, colours, angles,
  points excluded) adds **Vary across balls**: the setting gets a second value ("To") and a **Spread by**
  (Number, Round, Launch angle, Place, Random, Speed, Hits) with an Easing (Even, Ease in, Ease out, Ease
  both), the same spreads as the group gradient. Each ball's value is mixed between the setting's value and
  To by its spread position. "Stop varying" removes it.
- Stored beside the value in the card's params under `"<key>~to"`, `"<key>~by"`, `"<key>~ease"` (layered
  keys keep their layer prefix); keyframes may act on both ends.
- CPU-read settings (size, speed, physics) vary per ball in the sim; GPU-read layer settings carry per-ball
  values: each `LayeredInstance` names a small per-ball override block when its stack has varied settings
  (the renderer cost is only paid by varied stacks).
- A row with a varied setting shows a two-ended slider (value and To) and "Varies by Number" under it.
- Needs its own detailed plan; this spec fixes the storage keys, the spreads and the UI entry point.

## 9. Things not to do

- Don't make example or built-in scenes; test scenes live only behind the `test-scenes` feature.
- Don't approximate the ellipse with a polygon or cubic path for physics or drawing.
- Don't copy the Balls draw or look code into the emitter; generalise it.
- Don't store per-frame state for markers, waves, fades or wavefronts: work them out from the log and the
  world at the moment drawn.
- Don't let catching change ball ownership; a caught ball stays its owner's row.
- Don't re-bless existing goldens; only new test scenes add golden lines.
- No colours made up in editor code (the design system test); scene colours end in `// scene colour`.
