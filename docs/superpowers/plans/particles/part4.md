# Particles part 4: meeting things

Spec: `docs/superpowers/specs/2026-11-06-particles-design.md` (sections 3.4, 3.5, 4.4, 4.5, 11 part 4). Approved by the
owner in the brainstorm thread; built straight after this plan (no approval stops), one Opus review at the end, then the
PR, merged by this thread once checks and the review pass. The exe is installed only once every particles part is in.
Part 1 (`part1.md`, read its "As built") is the base. Parts 2, 3, 5, 6 and 7 build at the same time: everything here is
added in its own files or as new enum options, fields and match arms, so their merges stay small.

## What part 4 delivers

- **Particles make particles** (3.5): a Particles thing inside another one can be born from its parent's particles
  (Where: **Its parent's particles**, **When they**: Die / Land / Hit a ball / Hit a shape / Hit anything / Pop / Stop
  rising / Every step), Count per moment (at most 1000), Takes the parent's speed, and the Look's **Colour from: Its
  parent particle**. Chains can be any depth.
- **Meets things** gains: balls **Pop** and **Stick** (Fall off when it lands), shapes **Settle** (Fade after, Most
  kept) and **Pile up** (Angle of repose, Each adds, Thrown up), the picture's bottom edge counting as a floor for
  Stop / Settle / Pile up when the edges Bounce. Settled particles ride a moving shape and are kicked up again by balls.
- **Piles** (4.5): height fields of 2 px columns on still shapes' up-facing surfaces and on the bottom edge, sand
  sliding down to the Angle of repose and pouring off ledges; balls cut through them and throw sand back up; a new Look
  layer **Piles** draws them.
- **Two-way** (4.4): a switch on the Meets things card, off by default, with Weight and four effects: Pushes balls,
  Slows balls, Holds balls up (piles), Weighs balls down (stuck particles). A two-way thing steps with the physics
  (in the editor's main playback, the sound's, aiming, export) and its settings are physics settings.
- Looks 1 (Rain with Splashes), 4 Fire and smoke, 8 Bubbles, 14 Sand that piles up, 15 Thick fog (its part 4 form), 17
  Falling leaves, 19 Waterfall and mist, 22 Liquid splashes (motion), 24 Fireworks, 29 Sticky dust; and "Empty, fed by
  …" in the Add thing menu when a Particles thing is selected.
- Tests: unit tests for each new piece, a second particle goldens file for the part 4 scenes (balls hashed too where
  two-way), determinism (threads, seeks), editor tests for the new menu entries and the status line.

## Decisions made while planning (fill-ins of the spec, stated so review can correct them)

1. **Families step together.** A thing fed by its parent's particles never steps on its own: the topmost Particles
   thing of a feeding chain (the **head**) takes, for every particle step, its own step and then each fed descendant's
   step, in tree order. So a child always reads the moments of the very same particle step its parent
   just took, whatever the Physics rate (part 1's `catch_up` can take several particle steps after one world step). The
   moments are cleared at the start of each of the parent's steps (`ParticlesState.moments`). A fed thing whose parent
   is not a Particles thing in this world steps on its own and is born from nothing.
2. **Moments** are `Moment { kind, place, velocity, normal, seed, life, ball }` (`things/particles/moments.rs`), kinds
   Die (its life ran out, or it faded away after settling), Land (it came to rest on a shape or the floor: Stop, Settle,
   joining a pile; only when it came in faster than `LAND_SPEED` 20 px/s, so a particle resting on a wall doesn't land
   every step), HitBall (touched a ball while On touching a ball isn't Pass), HitShape (met a shape coming in faster than
   `LAND_SPEED`: Bounce, Stop, Settle, Vanish, Pile up), Pop, StopRising (its upward speed crossed 0 this step). A
   parent records only the kinds its fed children ask for (`ParticlesState.wants`, a bit set worked out each family
   step), so a thing nobody feeds from keeps none. Each chunk of the move keeps its own moments and they are joined in
   chunk order (the result is the same for any thread count). Every step is not a moment: a child fed Every step reads
   the parent's particles as they stand after its step, each giving `floor(Rate × DT + its dice)` births.
3. **Count per moment** reuses the Source's Count (at most `MOST_FED` 1000 a moment), Every step reuses Rate (a parent
   particle a second). The new From option is `BornFrom::ItsParentsParticles`; its When they is a new setting
   `fed_when` (`FedWhen`). Emit is hidden for it. A fed birth's place is the moment's place, its parent velocity the
   particle's (Takes the parent's speed), its way out the moment's normal (Off the bounce: a splash flies off the
   surface), its `ball` the parent particle's, and it keeps the parent particle's seed and life fraction
   (`Speck.kin_seed`, `kin_life`) for Colour from: Its parent particle, which colours it with the parent thing's first
   Look layer that has its own colours, at that seed and life (`look::ColourFrom::ParentParticle`).
4. **Fed things are decoration.** A fed thing's two-way switch does nothing (`in_main` is false for it), because its
   parent may be decoration and the editor's main world would then have nothing to feed it. A two-way parent with
   decoration children: the parent steps in both the main world and the particle playback (identically: moments never
   change the parent's particles); the children only in the particle playback.
5. **Resting particles.** `Speck` gains `rest` (the particle step it came to rest on, 0 while moving), `fade_from`
   (the step its resting fade began, 0 none), `on` (the thing it rests on: a ball's thing or a shape, its id) and
   `on_row` (the ball's row), `offset` (Stick: where on the ball, in ball radii from its middle), `kin_seed`, `kin_life`.
   A resting particle's age stands still (`Speck::age` counts to `rest`), so its colour and fade over life hold while
   it sits; when it is kicked up its `born` moves on by the time it rested, so its life runs on from where it stopped.
   No force acts on a resting particle. Resting particles count against Most particles as any.
6. **Stick** (At balls): a touching particle sticks where it touched (`offset` = the way out × 1, on the surface) and
   from then on sits at `ball place + offset × radius`, moving with the ball (its velocity the ball's). Each Hit event
   of that ball since the last particle step (`World` log, as On hits reads it) of at least `SHAKE_SPEED` 60 px/s shakes
   each stuck particle off with chance **Fall off when it lands** (its dice and the event's step); it keeps the velocity
   it had (the ball's before the hit: it flies on as dust does) plus `out × 40 px/s`, and is flagged `LOOSE` until it is
   clear of the ball (so it doesn't stick again at once). A removed ball lets its particles go.
7. **Settle** (At shapes): a particle meeting a shape (or the floor) rests there; it moves with the shape: each step it
   is carried by the shape's move from the last particle step's outline to this one's (centre moved, turned by its
   Spin × DT, scaled by its reach's ratio). A ball's Push wakes it (a ball within its reach moving at least
   `KICK_SPEED` 40 px/s relative to it), and the push then carries it off. **Fade after** (s, 0 never) starts its
   resting fade (`SETTLE_FADE` 0.5 s, then it dies with a Die moment); **Most kept** (default 3000): past it the oldest
   settled particles (list order) start their fade.
8. **Piles** (`things/particles/piles.rs`): a `Pile` is a run of 2 px columns (`COLUMN`) in world x on one surface:
   `key` (the shape's id, or `FLOOR` for the bottom edge), `first` column index, and per column `base` (the surface's y
   there) and `height`. Particles join a pile when they land on an up-facing surface (contact normal within 60° of up)
   of a **still** shape (no Spin, no growth, not moving: piles on moving shapes would float; there they Settle instead),
   or on the floor. A pile is found by key and a base within `SAME_SURFACE` 6 px of the new column's, else a new one is
   made. A neighbour column's base is found by a vertical ray down from the pile's top against the shape's outline
   points (nearest crossing below, within 12 px of the column it came from); with none (the end of a ledge) sand
   pouring there falls off as particles. Each grain adds **Each adds** px of height; then the column slides into lower
   neighbours while it stands more than `tan(Angle of repose) × COLUMN` above one (half the excess moved, at most 256
   moves a grain, columns at most `MOST_HEIGHT` 4000 px), left neighbour first. Grains that slide off a ledge, are cut
   by a ball or crumble go back as particles from the pile's top with the thing's own births (seeds from its birth
   counter, so replays are exact), at most `MOST_THROWN` 200 a step. A pile whose shape starts moving or is gone
   crumbles. Falling particles meet a pile's top as they would a shape (`meet_piles`, before the walls).
9. **Balls and piles.** Decoration: every particle step each ball cuts the columns it overlaps down to its circle's
   bottom there (a trench), and **Thrown up** (share, default 0.5) of the cut sand flies back up as particles at the
   ball's speed. Two-way with Holds balls up: in each substep's Settle stage (`ThingSim::settle`), a ball overlapping a
   pile is pushed straight up out of it (the most it sinks over its columns) and its downward speed reflected with the
   Bounciness on piles (`pile_bounce`, default 0.1); landing faster than `DIG_SPEED` 300 px/s it digs (the cut as in
   decoration, Thrown up of it as particles). No Hit event is logged for a pile (so no sound and no rule hears it).
10. **Two-way** (`MeetsSettings.two_way`, off; not keyable; group "Two-way"): **Weight** (each particle as heavy as a
    ball of its size × Weight: mass `Weight × size²` against a ball's `radius²`, the pairs' masses), and switches
    **Pushes balls** (the momentum a particle loses or gains touching a ball, Bounce off / Vanish / Pop / Stick, goes to
    the ball), **Slows balls** (the momentum a ball's Push gives particles is taken from the ball: drag by how many it
    wades through), **Holds balls up** (decision 9), **Weighs balls down** (each stuck particle pulls its ball by the
    scene's gravity × its mass ÷ the ball's). Pushes and the slowing are summed per ball (per chunk, then in chunk order)
    in the particle step and applied once, at the first substep of the next world step (`ThingSim::forces`); the weight
    is an acceleration applied every substep until the next particle step. Each ball's change in one particle step is
    clamped to the world's ceiling, the slowing never more than turns a ball's speed to 0, and non-finite sums are
    dropped. A two-way thing is `in_main` (decision 4 aside), so it steps with the physics everywhere; its own Source and
    its Motion and Meets cards then count in the fingerprint whole (new `ThingSim::owns_cards`, true for particles:
    `fingerprint_of` hashes a Motion or Bounce card when its kind has a module sim **or** the thing's sim owns its
    cards, layers included).
11. **The floor.** With At the edges Bounce and At shapes Stop / Settle / Pile up, the bottom edge (Edge margin under
    the shot) does what At shapes says instead of bouncing; the other edges bounce. Its piles are in world space: a
    moving camera leaves them where they formed.
12. **Piles layer** (`particles.piles`, new kind on `look.particles`, own settings `PilesLayer`: Where, Colours (paint
    following place / order / time), Opacity, Softness, Blend, Blend amount; takes the sub-layers): each column is one
    vertical streak sprite from its base to its top (half width 1.2 px, so neighbours overlap into one band). It draws
    nothing when there are no piles. Particles' own layers keep drawing the moving grains.
13. **Editor.** All new settings show through the settings grids as any. The status line counts the particles of
    things stepped in the main world too (two-way), beside the particle playback's. The Particles menu gains "Empty,
    fed by <name>" when a Particles thing is selected (adds an Empty under it, born when its particles Die). An Empty
    added inside a Particles thing any other way is fed too (`looks::recipe`).
14. **Looks** add their fed children in the same edit (`Recipe.children`, added under the new thing). Two-way is on in
    the looks whose spec line says so: Sand that piles up (Holds balls up), Thick fog (Slows balls), Waterfall and mist
    (Pushes balls), Sticky dust (Weighs balls down). Rain changes (its drops now end on balls and shapes, and splash), so
    part 1's "Rain" golden lines are blessed again. Thick fog uses part 1 and 4 pieces only (a wandering crowd with drag;
    parts 2 and 3 can add the air and Spring home to its recipe when they land). Falling leaves use the Confetti shape
    until part 5's leaf shape.

## Files

### `bpe-runtime`
- `plugins.rs`: `ThingSim::owns_cards(&self) -> bool` (default false).
- `world.rs`: `fingerprint_of` hashes a Motion/Bounce card (kind, layers, params) when `sims.module(kind)` is some or
  the thing's sim `owns_cards()`. Test: a thing that owns its cards changes the fingerprint when a card param changes.

### `bpe-kit/src/things/particles/`
- `mod.rs`: `BornFrom::ItsParentsParticles`, `FedWhen`, `ParticlesSettings.fed_when`, `when`s of Emit / Rate / Count;
  `pub mod moments; pub mod piles; pub mod two_way;`.
- `state.rs`: `Speck` fields (decision 5), flags `RESTING`, `STUCK`, `LOOSE`; `Speck::age` stands still while
  resting; `ParticlesState.moments`, `wants`, `piles`, `two_way` (`TwoWayOut`: pushes and weights by ball).
- `moments.rs` (new): `Moment`, `MomentKind`, `MomentKind::bit`, `FedWhen::wants`, `fed_births`.
- `births.rs`: fed births (`births` reads the parent's moments or particles), `MOST_FED`.
- `meets.rs`: `AtBalls::Pop`, `Stick`; `AtShapes::Settle`, `PileUp`; settings `fall_off`, `fade_after`, `most_kept`,
  `repose`, `grain`, `thrown_up`, `pile_bounce`, the two-way group; `BallAt.thing`/`row`/`mass`; `meet_balls`,
  `meet_walls`, `meet_edges` taking a `ChunkOut` for moments and pushes; `rest_on_wall`, `wake`.
- `piles.rs` (new): `Pile`, `Piles`, `meet_piles`, `add_grain`, `cut_by_ball`, `surface_below`, `crumble_moved`.
- `two_way.rs` (new): `TwoWayOut`, `apply_pushes` (the `forces` hook), `hold_up` (the `settle` hook).
- `step.rs`: `ChunkOut`, `run_chunks_with`, the resting moves (stuck, settled), `family` (decision 1), landings into
  piles, throwing sand, Most kept, the two-way sums.
- `sim.rs`: `after_step` skips fed things whose parent steps them; `forces`, `settle`, `in_main`, `owns_cards`.
- `look.rs`: `ColourFrom::ParentParticle`, `PILES`, `PilesLayer`, `Drawn::Piles`.
- `draw.rs`: piles drawn, parent-particle colours, the resting fade.
- `looks.rs`: the part 4 looks, `Recipe.children`, fed Empty.
- `tests.rs` (+ `tests_part4.rs`, new, for this part's unit tests), `scenes.rs`: `particle_scenes_meeting`.
- `tests/golden_particles_meeting.rs` + `tests/golden-particles-meeting.txt` / `-debug.txt` (new).

### `bpe-studio`
- `tree.rs`: "Empty, fed by <name>"; `app.rs`: the status line counts main-world particles too.

## Settings (labels as shown; spans, not hard ranges, except where the maths breaks)

Source: From gains **Its parent's particles**; **When they** (Die) shown for it; Rate shown for Every step, Count for the
other moments.

Meets things:
- Other balls: On touching a ball gains Pop and Stick; **Fall off when it lands** (Stick) 0.5 (0..1).
- Against shapes: At shapes gains Settle and Pile up; **Fade after** (Settle) 0 s (0 never, span 0..30); **Most kept**
  (Settle) 3000 (hard 1..1 000 000); **Angle of repose** (Pile up) 34° (hard 0..89); **Each adds** (Pile up) 0.4 px
  (hard ≥ 0, span 0..5); **Thrown up** (Pile up) 0.5 (span 0..1); **Bounciness on piles** (Pile up, two-way) 0.1.
- Two-way: **Two-way** (off); **Weight** 1 (hard ≥ 0, span 0..10); **Pushes balls** (on), **Slows balls** (on), **Holds
  balls up** (on), **Weighs balls down** (on): all shown only when Two-way is on.

Look: Colour from gains **Its parent particle**. New layer **Piles**: Where (Behind the balls), Colours (sand,
following place), Opacity 1, Softness 0.3, Blend (Normal), Blend amount 1.

## Steps and how each is verified

1. Runtime `owns_cards` + fingerprint: runtime test. `cargo test -p bpe-runtime`.
2. Kit state, moments, families, fed births: unit tests: a child fed on Die gets one particle per death at the death's
   place; Hit a shape splashes; Stop rising fires once per rocket; Every step rate; Colour from parent particle; a
   grandchild chain; 1 thread equals many; seek back and forth equals a straight run; at Physics rate 60 a child still
   sees every moment (families step together).
3. Pop, Stick (sticks, rides, falls off on a hit at about its share, never sticks again at once), Settle (rests,
   rides a moving shape, kicked up by a ball, Fade after, Most kept): unit tests.
4. Piles: a pour makes a heap whose slopes are at most the angle of repose (+1 column), sand pours off a ledge, a ball
   cuts a trench and throws sand, the floor pile with edges Bounce, a shape that moves crumbles its pile.
5. Two-way: off changes no ball (part 1's test still); on: rain pushes a still ball down, fog slows a moving ball,
   a ball rests on a sand pile above the shape, stuck dust weighs a ball down; finite and clamped; fingerprint changes
   with a two-way card edit and not with a decoration one; `main_part` keeps two-way things, drops fed ones.
6. Look: Piles layer draws a sprite per column; parent-particle colour.
7. Looks: every look (old and new) resolves and makes particles in 2 s (the existing test); fed children are added
   under their parent; the two-way looks have it on.
8. Goldens: `golden_particles_meeting.rs` (scenes: Rain splashing, Bubbles popping, Sand piling (two-way, balls hashed),
   Sticky dust (two-way), Leaves settling, Fireworks); `golden-particles*.txt` re-blessed for Rain only; ball goldens
   (`cargo test -p bpe-kit --test golden`) unchanged.
9. Editor: kittest adds "Empty, fed by …"; the status line counts a two-way thing's particles.
10. `cargo fmt`, `cargo clippy --workspace`, full `cargo test --workspace` (debug), release goldens, CLAUDE.md map,
    Windows exes build.

## Not doing (part 4)

- No sounds or rule blocks for moments (part 7), no air (part 2), no Spring home or other part 3 forces, no new sprite
  shapes or soft looks (parts 5, 6). No Hit events for piles. No particle-to-particle collisions beyond piles.
- Piles don't form on moving or spinning shapes (particles settle there instead), and don't follow a moving camera.
- No change to `layer.particles` / `shape.particles`; no built-in or example scenes; no demo videos.

## As built

- Step 1's runtime test became kit tests (`tests_part4.rs`: `a_two_way_thing_steps_with_the_physics_and_counts_in_the_fingerprint`),
  since only the kit has a thing that owns its cards; the runtime change is the `owns_cards` hook and the fingerprint
  hashing such a thing's Motion/Bounce params and layers.
- `add_sand` relaxes with a stack of columns to recheck, each slide moving half a column's excess past the angle;
  `MOST_SLIDES` is 4096 a grain (256 left slopes steeper than the angle on a tall heap), and slides under
  `SLIDE_LEAST` (0.01 px) are left, so a slope may stand that much past its angle.
- The scene settings' section list gains "Two-way" (after "Edges of the picture"), the Meets things card's new group.
- The goldens' meeting scenes are ten looks on the blank scene (Rain splashing, Fire and smoke, Bubbles, Falling leaves,
  Waterfall and mist, Liquid splashes, Fireworks, Sand that piles up, Thick fog, Sticky dust) over 6 s; the two-way
  ones write the balls' hit log hash too and the rest check the balls hit as without the particles. Part 1's
  `golden-particles*.txt` moved for Rain only (it now vanishes on balls and shapes and splashes).
- The Liquid splashes look comes On hits, so on a blank scene it waits for the first hit; the every-look test lets a
  look that `needs_hits()` be empty there.
- After review: pushes not yet given carry over (`TwoWayOut::carry`), so at a Physics rate under 120 (two particle
  steps a world step) balls get every push; Most kept and Fade after also apply to Pile up (whose grains settle on
  moving shapes); sand pouring off a ledge starts `SPILL_CLEAR` past it going out at `SPILL_SPEED`, and a pile landing
  is a Land moment only from `LAND_SPEED`; piles that grow into each other on one surface are joined
  (`merge_touching`) after each step's landings; settled particles turn with the outline's own angle (keyed turns and
  slow motion too); a thing fed by a broken parent steps itself; the fingerprint hashes a two-way thing's layers
  without their names. "When they" isn't Simple (the Source card holds at most 6).
- Rebased onto part 2 (the air): part 2's lock-step loop (the first Particles thing due takes every thing's particle
  steps, one step at a time in tree order) replaced `step::family`/`catch_up_family`: a fed thing reads its parent's
  state straight after the parent's same step (`step::wants_of` gives each thing its children's kinds). The two parts'
  Thick fog is one look: part 2's (it follows the air) made two-way (Weight 0.02), and a two-way thing in the physics
  makes the fingerprint hash the Air card and its keys too, since such a thing may follow the air.
- Rebased onto part 3 (moving and forming): births keep part 3's batches (a fed thing's births are `Batch::At` its
  parent's moments), `move_one` takes the particle's index for part 3's forces (`run_chunks_with` hands each chunk its
  first index), a force that kills a particle (swallowed) is a Die moment. As the last of parts 2 to 5 to merge, it
  adds part 5's layers to the looks part 5 left: Fire and smoke burns as a Flame with Soft smoke for its Smoke child,
  Falling leaves are part 5's Leaves, and part 3's Flock draws Birds pointing the way they fly (Dust clouds came with
  part 2).
- **Rebased onto part 7.** Part 7 brought its own `moments.rs` (`Moment`/`MomentKind` per world step, for the sound and
  the When Particles rule block, from the `Speck.met` marks), so this part's per-particle-step moments for fed things are
  now **feeds**: `feeds.rs` (`Feed`, `FeedKind`, `Feeds`), `ParticlesState.feeds`, `ChunkOut.feeds`; `wants` and
  `LAND_SPEED` keep their names. Both run side by side: the move notes feeds into its chunk and marks the particle, and
  the serial retain turns the marks into part 7's moments. Stick marks a ball meeting, and Settle and Pile up mark a
  landing like Stop. `LANDED` is now 16 (2 is `RESTING`). `BallAt.thing` is a `ThingId` (with `row`), and `WallAt.id`
  is the shape's id that part 7's `thing` was. Part 7's `ThingSim::own_cards` replaces this part's `owns_cards`, hashing
  layers without their names. Part 7's lock-step loop moved into `sim::catch_up_all`, which now passes each fed thing
  its parent. A fed thing's births never count as a burst.

