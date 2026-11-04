# Plan: Trail and Particles layers (part 2)

Spec: `docs/superpowers/specs/2026-11-04-sublayers-trails-physics-design.md`, "Part 2: Trail and Particles
layers" (the settings tables are there; this plan doesn't repeat them). **Starts after part 1 (sub-layers) is
merged**: it uses `LayerSection::Sub`, `LayerKind::takes_sub`, `Registry::add_upgrade`, the `sub.*` kinds and the
editor's piece box (`look_editor/layer_list.rs::piece`). Read part 1's `ballphysics/sublayers/decisions.md` in project
files first: rulings made while building part 1 win over this plan where they differ.

Branch: the build thread's own, from `main` with part 1 in. Scene format **10**.

## Decisions (fixed)

1. New `LayerSection::Around` for the list "Trails and particles" (between Layers and Reactions in the editor; in
   `Slot.layers` the Around layers sit after the drawn layers and their subs, before the reactions: extend the
   section-order rule in `bpe-core/src/layers.rs` that keeps Drawn, Reaction, Group in order). Around layers
   repeat and move among themselves (`move_layer` treats them like group layers: within their section).
2. Kinds: `layer.trail` and `layer.particles` on `look.layers`; `shape.particles` on `look.shape` (no shape
   trails). Each `takes_subs() == true` and `takes_sub(k)` true only for Glow, Outline, Shadow, Recolour, Flash on
   hit, Colour on hit (`sub.glow`, `sub.outline`, `sub.shadow`, `sub.recolour`, `sub.flash`, `sub.hue` and the
   `shape.sub.*` ones).
3. Sub-layers on trails and particles are worked on the **CPU** into what is drawn: Recolour, Flash on hit and
   Colour on hit change vertex/instance colours; Glow is an extra wider soft copy drawn under it (additive);
   Outline an extra edge-only copy over it; Shadow an extra offset dark soft copy drawn first. The GPU gains only
   a softness and a band parameter (below). The in-ball `Piece` walk is not used for them.
4. `SlotId::Trail` and `SlotId::Particles` **stay in the enum** so old files parse, but leave `SlotId::ALL`'s card
   list (`in_card_list()` false), no kind fits them, and the format 10 upgrade empties them. Don't renumber
   `SlotId` (it's `as usize` indexed).
5. Particles move to a deterministic CPU simulation (spec). The GPU draws one instance per particle.
6. Ghosts and Mini balls are extra `LayeredInstance`s with the ball's own records: same `range` (records), their
   own `center`, `radius`, `misc.x` (opacity), `hit` ([1e6, 0, 0, 0]).
7. Behind/In front: behind = drawn before the thing's balls (today's trail and burst place, and ghost/mini-ball
   instances pushed before the ball's own instance in `frame.layered`); in front = new `Frame.trails_front` and
   `Frame.particles_front` drawn after the balls in the same blend group, and ghost/mini-ball instances pushed after.
8. History: per-thing seconds, set from the thing's longest trail; "Stays forever" keeps all samples at 30 a second.
   The world fingerprint hashes (keeps history, seconds rounded up to 0.25 s, forever), never looks.

## Things not to do

- No physics change: particles never push balls; goldens must not move.
- No trails on shapes, no Cut/Shading/Shine/Shader subs on trails or particles.
- Don't keep the `trail.streak`/`particles.burst` card kinds registered (the upgrade replaces them).
- No demo renders; tests and goldens only.

## Tasks

### Task 1: core (`bpe-core`)

Files: `registry.rs`, `layers.rs`, `ids.rs`, `scene.rs`.

1. `LayerSection::Around`. In `layers.rs`, the helpers that find where drawn layers end (`group_start`, the
   "drawn ... end" helper near line 145) learn Around: order is Drawn (+ subs), Around (+ subs), Reaction, Group.
   `add_layer` of an Around kind goes at the end of the Around run. `move_layer` of an Around layer moves it among
   the Around layers (place counted from the first Around layer) with its subs. `copy_look`/`paste_look` carry
   Around layers and their subs.
2. `SlotId::in_card_list()` returns false for `Trail` and `Particles` (and keeps false for Sound and Mixer); doc
   comment: "kept so files before format 10 open".
3. `Scene::FORMAT` = 10.
4. Tests: Around ordering on add, move limits, copy/paste whole look keeps them, subs ride with them.

### Task 2: settings and kinds (`bpe-kit`)

Files: new folders `crates/bpe-kit/src/modules/look/layers/trail/` (`mod.rs` settings, `draw.rs`, `kinds.rs` per
trail kind maths) and `crates/bpe-kit/src/modules/look/layers/particles/` (`mod.rs` settings, `sim.rs`,
`emit.rs`, `draw.rs`); `shape/particles.rs` for the shape version; `layers/mod.rs` and `shape/mod.rs`
registration; remove `crates/bpe-kit/src/modules/trail/` and `crates/bpe-kit/src/modules/particles/` after moving
what's reused (`push_streak` callers, `OwnColours`, `FULL_HIT_SPEED`, colour helpers).

1. `TrailLayer` (spec table; key names: `kind`, `place` ("where": Behind/InFront enum `Place`), `length_by`,
   `seconds`, `pixels`, `forever`, `faster`, `least_speed`, `width`, `thickness`, `tail_width`, `width_curve`,
   `source`, `colour` (paint, prefix "", follows life|place|speed|time), `brightness`, `fade`, `wobble`,
   `wobble_waves`, `wobble_speed`, `restart`, `twist`, `twist_speed`, `ghosts`, `ghost_every`, `dot_every`,
   `dot_size`, `puffs`, `spreads`, `rises`, `softness`, `blend`, `blend_amount`), with `when`s by kind
   (`when = "kind=ribbon"` etc.) and groups ("Shape", "Colours", "Light", "Motion" for wobble, "Blending").
   Curves are a shared `Curve { Even, In, Out, Both }` choice with `fn at(self, t) -> f32` (smoothstep-family;
   unit-tested ends 0→0, 1→1).
2. `ParticlesLayer` (spec table; keys `when`... use `emit` for "When" to avoid the `when` attribute name clash,
   `count`, `rate`, `least_speed`, `follow_impact`, `made_of`, `direction`, `spread`, `speed`, `inherit`, `size`,
   `size_curve`, `spin`, `spin_spread`, `life`, `fade`, `source`, `colour`, `twinkle`, `twinkle_speed`,
   `gravity`, `drag`, `swirl`, `swirl_size`, `pull`, `bounce`, `bounciness`, `place`, `blend`, `blend_amount`).
   `MadeOf` keeps today's six codes 0..5 and adds Dots 6, Hearts 7, Bubbles 8, Streaks 9, MiniBalls 10.
   Shape version `ShapeParticlesLayer`: `emit` only OnHit/Steady, no MiniBalls (its own `ShapeMadeOf` without it).
3. Register kinds (section Around, repeats, `takes_subs` true, `takes_sub` as Decision 2). Unregister
   `trail.streak` and `particles.burst` module kinds and their `ModuleDraw`s in `lib.rs::kit()`.
4. `starters.rs`: drop Trail/Particles from any starter lists; `BALL_SLOTS`/`SLOTS` lists in
   `things/balls/mod.rs` and `things/shape/mod.rs` drop `SlotId::Trail`/`SlotId::Particles`.
5. Upgrade `reg.add_upgrade(10, cards_to_layers)`: for each thing, if it has a Trail card of kind `trail.streak`,
   add a `layer.trail` (Streak) to its Look card (`look.layers`; skip if none) at the end of Around with params
   mapped: `seconds`, `width`, `brightness`, `source`, colour keys (same paint keys), `blend`, `blend_amount`
   copied; `tail_width = 1 - taper`; `width_curve = Even`. A Particles card `particles.burst` becomes
   `layer.particles` (balls) or `shape.particles` (shapes) with `emit = OnHit`, `made_of` from `preset` (same
   names), every other same-named key copied. Then remove the two cards. Keyframes, pulses and Follow sound
   entries on those cards' addresses move to `Address { thing, slot: Look, key: layered_key(new_id, key) }`
   (rename `taper` keys' keyframes to `tail_width` with values `1 - v`). Find where animation is stored on
   `Scene` (`grep -n "animation" crates/bpe-core/src/scene.rs`) and rewrite it there.
6. Test scenes (`scenes.rs`, `testing.rs`): any scene that used Trail/Particles cards writes layers instead (use a
   helper `trail_layer(params)`/`particles_layer(params)`); goldens don't care (drawing only), but the picture
   tests that used them must keep passing.
7. Tests: registry order; upgrade from a format 9 RON with both cards and a keyframe on `seconds` and on `taper`
   gives the layers, the moved keyframes and no cards; settings `when`s show the right rows per kind.

### Task 3: history (`bpe-runtime`)

Files: `crates/bpe-runtime/src/history.rs`, `world.rs`, `plugins.rs` (if the thing sim asks for history).

1. `History::new(steps_per_second, seconds, forever)`: `SAMPLE_RATE` 60 normally; forever samples at 30 a second
   and never drops (`rings` becomes a `Vec<Vec<Vec2>>` per ball, or keep one buffer with a per-ball stride that
   grows: pick whichever keeps `points(i)` (newest first) and `len()` simple). `SAMPLES` becomes per history:
   `ceil(seconds * rate) + 1`, at least 2.
2. `World::keep_history(thing, seconds, forever)`; called at build from what the kit says the thing needs
   (`ThingSim` or a draw-side query: find how today's Trail card triggers `keep_history` and route it to the
   thing's Look card's trail layers: the longest `seconds` (or `pixels / 300 px/s`, at least 2 s, for By distance;
   `× (1 + faster × 3)` when Longer when fast is on), forever when any Line has Stays forever).
3. Fingerprint (world.rs ~line 640): replace the Trail-slot hash with (keeps, seconds rounded up to 0.25 s,
   forever) from the trail layers.
4. Snapshots (playback seek) clone the world, so a forever history is copied per snapshot: keep it; note it in
   `decisions.md`, and check memory with a 60 s, 100-ball forever Line (≈ 1.4 MB per snapshot; fine).
5. Tests: the history length follows the longest trail; forever keeps the first sample after 10 s; the same scene
   with and without a trail steps identically (today's `a_trail_keeps_a_history_that_follows_the_balls`, ported).

### Task 4: GPU for trails and particles (`bpe-gfx`)

Files: `frame.rs`, `particles.rs`, `shaders/particles.wgsl`, `shaders/trail.wgsl`, the renderer pass that draws
trails and bursts (grep `trails` and `bursts` in `crates/bpe-gfx/src`).

1. `TrailVertex` gains `style: [f32; 2]`: softness (0 = today's one-pixel edge; 1 = soft from the centre line out)
   and band (0 = filled; else only the outer `band` fraction of the half width shows: the Outline sub). Update the
   vertex layout and `trail.wgsl`'s coverage. `push_streak*` take a `StreakStyle { softness, band }` (default 0, 0
   keeps today's picture: a test compares bytes).
2. Particles: replace `Burst`/`BurstGpu` with `Particle { position: [f32;2], size: f32, angle: f32, color:
   [f32;4], shape: u32, additive: u32 (bool), soft: f32, band: f32 }` and `ParticleGpu` (Pod, 16-byte aligned).
   `Frame.bursts` → `Frame.particles: Vec<Particle>`; add `Frame.particles_front`, `Frame.trails_front`.
   `particles.wgsl`: vertex shader places a quad per instance from `position/size/angle`; fragment keeps today's
   shapes (sparks as short streaks along `angle`, smoke soft disc, confetti rectangle, stars, shards, rings) and
   adds dots, hearts, bubbles (ring with a highlight), streaks (a long thin rounded line along `angle`). `soft` and
   `band` as for trails. Keep `MAX_LIVE_PARTICLES`; drop `MAX_BURSTS` and burst picking.
3. Front lists draw right after the balls of the same blend group/place (follow how `frame.trails` is split into
   blend layers today, `Frame::lift`-style code near line 1011, and give the front lists the same treatment).
4. Tests: a streak with `StreakStyle::default()` gives the same vertices as before; a soft streak's edge alpha
   falls off; particle instances draw at their positions (GPU picture test: a red dot instance at a point lights
   that point only); each new shape covers its centre and not its quad corners; front particles draw over a ball,
   behind ones under.

### Task 5: trail drawing (`bpe-kit`)

Files: `modules/look/layers/trail/draw.rs`, `kinds.rs`; `things/balls/draw.rs` calls it.

1. In `balls/draw.rs`, after building the stack and before/after pushing ball instances (Decision 7), for each
   shown Around trail layer call `trail::draw(cx, stack, layer, frame, place)`.
2. Points: `bodies.position[i]` then `history.points(i)`, cut by Length by (time: samples ≤ seconds × factor;
   distance: running arc length ≤ pixels × factor), factor = `1 + faster * speed / 1000`. Hidden when
   `speed < least_speed`. Restart at each bounce: cut the list at the newest hit of ball `i` (from `frame.hits` or
   the log since the trail's start time; needs per-ball hits: `HitMark` gains `ball: u32`, set where the balls
   draw pushes hit marks).
3. Per kind:
   - Streak: `push_streak_colours` with per-point half widths from `Width`, `tail_width`, `width_curve`; wobble
     offsets each point along the local normal by `wobble * sin(2π (waves * s + speed * t))`, `s` = 0 head to 1
     tail; colours from Colour from/Colours and Fade curve × Brightness (today's `shade`).
   - Line: the same with half width `thickness / 2` px, no taper; Stays forever uses the whole history.
   - Ribbon: half width × `|cos(θ)|`, θ = the ball's turning angle at that sample (Turning reaction's angle at the
     sample's time: angle + spin × t; Spins: `twist_speed × t × 2π`), colour × 0.6 where `cos(θ) < 0` (the back).
   - Dots: points every `dot_every` px of arc, a `Particle` (shape dots, size `dot_size × radius × 2`, colour along,
     fading by Fade).
   - Smoke: puffs at fixed times (`1 / puffs` s apart, times aligned to multiples so scrubbing is stable) along the
     history, each a soft smoke `Particle` that grows (`size × (1 + spreads × age)`) and rises (`rises × age`) with
     age = its time behind the head.
   - Ghosts: `ghosts` copies at `ghost_every` s apart back along the history; each a `LayeredInstance` copy of the
     ball's instance at that position, opacity `(1 - k / (ghosts + 1))` × Fade curve, behind or in front.
4. Subs (Decision 3), applied in box order to what the layer drew: Recolour (vertex colours via the same maths as
   `sub_layers.wgsl`'s recolour, in Rust in a new `crates/bpe-kit/src/paint/recolour.rs`, unit-tested against
   known colours), Flash on hit (ball `i`'s last hit age → brighten), Colour on hit (hue turn × hits), Glow
   (another streak/particle pass, `half_width × (1 + size)`, `softness 1`, additive, glow colour × strength, drawn
   before the layer's own), Outline (another pass with `band = width_px / half_width`, its colour, drawn after),
   Shadow (another pass offset by angle/distance (radii × ball radius), shadow colour, `softness` from the sub,
   drawn first).
5. Tests (CPU, frame contents): Streak point count follows Length by time and by distance; Longer when fast; least
   speed hides; restart cuts at the hit; wobble moves points sideways but not the head; Ribbon widths follow cos;
   Dots spacing; Smoke puffs stable when drawing the same time twice and after drawing an earlier time; Ghosts count
   and fading opacities; Behind vs In front land in the right lists; Glow/Outline/Shadow subs add passes in the
   right order; Recolour changes colours.

### Task 6: particle simulation and drawing (`bpe-kit`, `bpe-runtime`)

Files: `modules/look/layers/particles/emit.rs`, `sim.rs`, `draw.rs`; `crates/bpe-runtime/src/walls.rs` (a query).

1. `emit.rs`: `fn emissions(cx, layer, ball/shape, now) -> Vec<Emission>` for the window `[now - life, now]`:
   - On hit: one per hit mark of the thing (and ball) in the window with speed ≥ least speed; count = `count` ×
     (impact factor when Follow impact: `(speed / FULL_HIT_SPEED).min(1.5)`).
   - When it pops: one per pop event of the thing's balls in the window (log `EventKind` for pops: grep), at the
     ball's last place.
   - While moving / Only when fast / Steady: births at fixed times `k / rate` (k integer) inside the window; the
     emitter place is the ball's position at that time (history interpolation; the current position for the newest);
     While moving skips births where the ball's speed (from history differences) < 1 px/s; Only when fast where
     speed < least speed. Steady on shapes: random points along the outline (`Outline::nearest` of a seeded point,
     or walk the path).
   - Each `Emission { time, place, normal (bounce normal / heading), ball_velocity, seed, colour }`.
2. `sim.rs`: `fn simulate(e: &Emission, k: u32, layer, world, history, now) -> Option<ParticleState>`: seeded
   start (direction by Direction: off the bounce = `normal` rotated by ±spread/2; all round; along the path =
   velocity dir; behind = −velocity dir; up = (0, −1); plus `inherit × ball_velocity`), then fixed 1/120 s steps
   from birth to `now` (the last step partial): gravity, drag (`v *= 1 - drag·dt` clamped), swirl (curl of a 2-D
   value-noise field at `place / swirl_size`, × `swirl`), pull back (`v += (ball_pos(t) - p) × pull × dt`, ball_pos
   from history interpolation), bounce off shapes (for each wall in `world.walls`: `outline.nearest(p)`; if the
   particle is within the band's half thickness + size/2 and moving into it, reflect `v` about the normal × 
   bounciness and push it out). Returns position, angle (spin × age with per-particle spread; streak shapes use
   the velocity's angle), size by Size over life (Grow 0.3→1, Shrink 1→0.2, Pop 1→1.5 then gone in the last 10%),
   alpha by Fade, twinkle `1 - twinkle × (0.5 + 0.5 sin(twinkle_speed·2π·t + seed))`.
   Determinism: every random number from `seed_of(thing, ball, emission time bits, k)` hashed; nothing kept between
   frames. Cap: at most 4000 particles per layer per frame (newest emissions first; oldest dropped) and at most
   `life × 120` steps per particle.
3. `draw.rs`: emissions → simulate → `Particle`s (colours: Colour from Ball = the emitting ball's colour, Own =
   the paint by its follow (Life: per particle along its life on the CPU now; Order: stop by particle index; Place,
   Speed, Time as today's `OwnColours`)), then the subs as for trails (Glow: an extra soft larger particle under
   each; Outline: an extra band particle over; Shadow: an offset soft dark particle under; colour subs on colours).
   Mini balls: `LayeredInstance`s with the ball's records at radius `size / 2`.
4. `walls.rs`: if `nearest` and the band's half thickness aren't enough to tell "inside the band", add
   `pub fn half_thickness(&self) -> f32` on `Outline` (read the struct first; filled walls count from their edge).
5. Tests: same emissions and particles when drawing time t twice, and t after drawing a later time; a particle with
   gravity falls (y grows) and with drag slows; swirl changes direction but not speed much (within 30% over 0.2 s);
   pull back brings particles towards the ball; bounce keeps a particle from passing through a box wall in a scene
   with one; While moving makes nothing for a still ball; Steady births at the same times regardless of frame
   rate; the 4000 cap; a shape's On hit particles come from its hits.

### Task 7: editor (`bpe-studio`)

Files: `look_editor/mod.rs`, `layer_list.rs`, `layer_rows.rs`, `inspector.rs`, `timeline/lanes.rs`.

1. `look_editor/mod.rs::show`: the "Trails and particles" list with `list_header(..., "Trails and particles",
   "+ Add trail or particles", LayerSection::Around, ...)` between Layers and Reactions, with an empty-state line
   "None yet: add a trail or particles." Rows use `layer_list::piece` (boxes for their subs). Shapes show the list
   with Particles only.
2. Rows: thumbnails (a short streak; dots for particles in the layer's colour), summaries (Trail: kind word + "0.5
   s" or "300 px"; Particles: made-of word + "on hit" / "60 a second").
3. The settings of the picked one use the settings grid with `when`s per kind; Made of and Kind as picture chips if
   part 1 left a chip widget for Shape cuts, else menus.
4. Drag to reorder within the Around list (as drawn layers drag within Layers).
5. `inspector.rs`: the card list no longer shows Trail/Particles (via `in_card_list`); remove any Trail/Particles
   special cases; update tests that used them (`a_ball_with_every_slot`, jump bar tests: read them).
6. `timeline/lanes.rs`: lanes for trail/particle settings now come from the Look card's layered keys like other
   layer settings; check the lane naming code handles `layer.trail`/`layer.particles` names.
7. Kittests: add a trail from the list; its settings change with Kind; a Glow sub on a trail is offered, a Cut is
   not; Particles on a shape has no Mini balls; the old cards are gone from "+ Add slot".

### Task 8: renderer/export check, docs, review, deliver

1. `bpe-cli` export of a scene with a trail and particles still works (an existing CLI test, or add one that
   renders 2 frames and checks the mp4 exists; no visual check).
2. `CLAUDE.md`: replace the Trail/Particles card mentions (slots list, kit module folders, frame lists, history)
   with the layer versions; add `LayerSection::Around`, the particle sim, `Frame.particles(_front)`,
   `trails_front`, `TrailVertex.style`, history seconds/forever, format 10.
3. Full suites debug and `--release`; goldens untouched (`git diff main -- crates/bpe-kit/tests/` empty).
4. One Opus review of the whole diff; fix with tests first. PR; exes to project files
   `ballphysics/trail-particle-layers/`.

## Done when

- Trails (six kinds) and particles (every When, Made of, Direction and motion option) are Look layers with
  Behind/In front and Glow/Outline/Shadow/Recolour/Flash/Colour-on-hit subs, on balls (and particles on shapes).
- Old scenes open with their trail and particle cards as layers, keyframes kept.
- Scrubbing and export give the same particles; all suites green; goldens untouched.
