# Water mode part 4b: forces and ferrofluid (plan)

> **For agentic workers:** built natively in one thread (Aedan's project rule: one Opus review per part, no
> approval stops after the design). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a Water scene's Force things move the water: **Wind** blows spray and streams and drags the surface,
a **Whirlpool** spins the liquid round it, and a **Magnet** pulls its liquid in and stands it up in a crown of
**spikes** whose strength rides on keyframes; with the new **Ferrofluid** look (black, glossy) that is ferrofluid.

**Architecture:** forces are stateless inputs worked out on the host each frame from the Force cards
(`bpe-kit things/water/draft/forces.rs`) and handed to the engine (`bpe_water::Force`, `Water::set_forces`), which
puts them in the frame's numbers (a block appended to `Fr`) and adds their push to every liquid drop in the fall
(`shaders/forces.wgsl`, called from `fall()` in `sort.wgsl`). Nothing is kept from frame to frame, so takes and
restart points do not change (`TAKE_FORMAT` stays 2).

**Tech Stack:** Rust, wgpu/WGSL (`bpe-water`), egui 0.37 (`bpe-studio`).

**Spec:** `docs/superpowers/specs/2026-11-24-water-mode-design.md` (section 1's Force row, section 3 "Same water at
every detail", Parts 4b). Hand-over: `/mnt/project-files/ballphysics/water-mode/part4-5-4b-handover.md` ("Part 4b").

## Global Constraints

- Balls scenes byte-for-byte unchanged: no Balls golden or pinned picture moves.
- A scene with no Force plays the same water to the bit as before (the loop over no forces changes nothing).
- Every number in world pixels and seconds, so a force pushes alike at every detail ("same water at every detail").
- `bpe-kit` and bpe-water's host code use `bpe_core::maths::Maths` (`det_sin`, `det_atan2`, ...), never `f32::sin`.
- No pass binds more than 8 storage buffers: forces live in the frame's uniform, no new binding.
- Editor colours only from `style/` tokens; Windows-friendly; no exe built or installed; no demo videos.
- Picture tests through `bpe_gfx::test_device()`; the pinned looks refresh only through "Refresh GPU pictures".
- Test only what this part adds or changes; CI runs the rest.
- Parts 4 and 5 run beside this one: both also append a block to the end of `Fr`/`FrameGpu` (drains; movers) and
  touch `draft.rs`'s `FrameInputs`, `set_inputs`, the print and `inputs()`. Whoever merges second puts its block
  after the others' and rebases. Part 4 adds a third vec4 to `Feel`/`FeelGpu` and a ready-made liquids table
  (`modules/water/liquids.rs`, `ReadyLiquid`); part 5 changes `keep_off`, `St` and the solid buffer. This part
  touches none of those.

## Decisions (my calls; cheap to change before they are built)

1. **Where forces act.** In `fall()`, after gravity and the damping and before the speed cap and the sub-moves,
   on liquid drops only (air drops feel none: the water round a bubble carries it). A drop born this step takes
   its first push next step. Forces are read once a frame from their keyframes, as gravity and the pours are.
2. **The unit of Strength** is the course's usual gravity, `FORCE_UNIT` = 4000 px/s² (the Water card's default),
   not the scene's own gravity, so a magnet still pulls in a scene with no gravity.
3. **Which liquids.** The Force card gains **Liquid** (a `Target`, default `Kind(water_liquid)`, shown "Any
   Liquid": every Liquid; a Liquid by name; a tag). It becomes a bit mask on the card (`liquids`, bit k = liquid
   k). Not a Magnetic setting on the Feel card: that card is part 4's, and a default of 0 would make a new magnet
   do nothing. A force that moves no Liquid says so on the stage (`NO_LIQUID_TO_MOVE`).
4. **Magnet: a pull with a level the liquid rests on.** The push is the slope of a potential, so liquid comes to
   rest on a level of gravity plus magnet, at any detail, and nothing goes round and round for ever:
   without spikes `Φ(x) = −A·R·Ψ(u)`, `u = r / R` (R the Reach), `Ψ(u) = (1 − u²)³` (nothing at the magnet's
   middle and at its Reach, smooth at both), `A = S · FORCE_UNIT / 1.7173` (1.7173 is `Ψ'`'s largest size, so the
   strongest pull without spikes is Strength gravities); the push is `−∇Φ`, and decision 5 shapes `Φ` into
   spikes. Strength below 0 pushes liquid away.
5. **Spikes are laid out, not left to chance.** Real ferrofluid's spikes come from its own surface pull; here the
   skin's reach is the drop spacing, so left to themselves the spikes would differ between the draft and the final.
   So the magnet's reach dips between **Spikes** teeth spread evenly across **Spike spread** (degrees, default 180)
   round **Spikes point** (degrees, default −90, up): `Φ = −A·R·Ψ(u / s)`, `s = 1 − depth·c(u)·(1 − T(x))`, `T =
   (1 − |x|)³`, `x` the turn from the tooth's middle over its half width `w = 0.5 · spread / N` (neighbouring teeth
   just meet), the point rounded off within 0.1 of the middle (`SPIKE_ROUND`, so the pull doesn't flip across a
   spike's middle and shake the drops on it), `c(u) = smoothstep(0.2, 0.45, u)` (a round, calm pull inside), past
   the spread the valleys' reach; `depth = SPIKE_DEPTH · size`, `SPIKE_DEPTH` 0.6, size the card's **Spike size**
   (0 to 1, keyframed). Every level past `u` 0.45 is the same star (sharp tips, round valleys), and along every
   line out of the middle the potential only rises, so the middle is always the deepest place (with too little
   liquid for the mound, a small star; never blobs out in the teeth). Defaults: Spikes 9, Spike size 0.75.
   *Measured in Task 1:* the first shape, `M = 1 + 16·size·c(u)·T` multiplying the pull with teeth from `u` 0.05,
   put the mound under a median 85 gravities, so the push met its guard and the liquid churned (rms 360 px/s); its
   deepest place was out in the teeth, not the middle. The star keeps the mound under about 10 gravities (most
   about 9 × Strength near a spike's middle).
5b. **Inside a magnet's reach the water holds itself up** (no deep part there, `in_magnet` in deep.wgsl's cells):
   the deep water rests no fuller than the drops just above it (C2), a magnet presses them fuller, and the deep
   pressure then stirred the mound at 100 px/s; without it the mound comes to rest (4 px/s inside at 6 px, 5 at
   3 px). The water inside a magnet's reach is a little more pressed than elsewhere (Known).
6. **Wind: air drag by how much of a drop meets the air.** `v += (W − v)·(1 − e^(−k·open·dt))`, `W` = Strength ×
   `WIND_SPEED` (1000 px/s) along Aim, times `1 + gusts·g(t)`, `k = WIND_RATE` 3/s, `open = smoothstep(0.75,
   0.35, fullness)` (the drop's neighbourhood's fullness, `nf.z`: a lone drop 1, a flat open top about 0.1,
   inside 0). It blows everywhere in the course; solids don't shelter (Known, later). `g(t) = (sin 1.3t +
   sin(2.9t + 1) + sin(4.7t + 2)) / 3` on the host (det maths), **Gusts** 0 to 1 on the card.
7. **Whirlpool: the speed round it is drawn toward a spin.** The speed round its middle (not toward or from it)
   is drawn toward `V(r) = W·2x/(1 + x²)`, `x = r / (0.3 R)` (turning as one inside, falling off outside), `W` =
   Strength × `SPIN_SPEED` (600 px/s, below 0 the other way), at `SPIN_RATE` 4/s × `1 − smoothstep(0.7, 1, u)`,
   by the same exact `1 − e^(−k dt)` as wind, so every step count gives the same change a second.
8. **Heater and Cooler** do nothing yet and say so (`HEAT_LATER`); heat is Later (checkpoint).
9. **Limits.** At most `MOST_FORCES` = 16 forces (more: `TOO_MANY_FORCES` on the stage). Each force's push on a
   drop is held to `MOST_PUSH` = 256 gravities (a guard only: held pushes are no longer down a slope, and a
   magnet that meets it churns, so Strength 4 with Spike size 1 stays well under it).
10. **Steps.** The frame's headroom (`steps_now`) adds each force's push over two frames, as it does for gravity
    (`Force::headroom`: a magnet's pull without its spikes, Strength gravities; a whirlpool's and a wind's draw on a
    drop at rest). Up to `most_steps` = 32. *Measured in Task 6:* counting a spike's sharper slopes (about 9 ×
    Strength gravities in a thin band) took a 6 px frame to 32 steps, and there the skin's bend stirred the mound
    (C1's limit: the skin is calm only at the trial's pace); more steps stirred it more, fewer calmed it.
11. **The calm skin past 32 steps** is built only if a strong magnet needs it: Task 6 measures Strength 4, Spike
    size 1 at 6 and 3 px (and 2 px on the card if the container can't), and the mark is "the spikes stand still
    (tips move under 2 px/s rms over the last second) and no drop is thrown past the reach". If the mark is met at
    32 steps the skin is left as it is and the notes say so. *Met* (Task 6, with decision 10's headroom): a strong
    magnet needs no more than the floor's steps, and more make it worse, so the calm skin is not built here.
12. **Ferrofluid's look** is an eighth ready-made look, **Ferrofluid** (Black glossy liquid, high shine with a
    small highlight, edge sheen, a little Smoothing so spikes stay sharp; pale flat plate and back; two soft spots
    from above; *as built*: no lamps, Task 5), set with `apply_look` and offered on the chooser and in Use a look.
    Its liquid's values also go to part 4's `ReadyLiquid` table as a Ferrofluid row when that lands (whoever merges
    second adds it; values in "As built").
13. **The stage shows a force** with its marker as today plus: Magnet and Whirlpool a ring at their Reach; a
    Magnet's spikes as short ticks on the ring; a Whirlpool's spin as an arrow head on the ring; Wind three arrows
    along its Aim (as a Sun's). At the playhead, with keyframes (`force_card_at`).
14. **What restarts.** Every Force card setting, a Force's place, and turning it off restart the draft and mark the
    final out of date; a Force's part of `WaterInputs` is `force:{k}` from time 0, its keyframes as other parts'.
15. **A Force's place** is its `transform.position` (no place keyframes: part 5 adds them only on the Moves card of
    Pieces and Walls), read through `ForceSetup::place_at(time)` so a later place track is a one-line change.

## Review Focus

1. A scene whose forces all have Strength 0 plays the same water to the bit as a scene with none (magnet,
   whirlpool and wind: the kit gives a wind and a whirlpool the rate times the Strength's size up to 1, so at 0
   they are not felt; `strength_zero_moves_nothing`, `no_force_at_strength_zero`).
2. A Magnet placed inside the rock (under a floor, the usual set-up) pulls through the rock: the field is not
   blocked by solids, the drops are still kept off them.
3. A Force naming a Liquid that is turned off or past the 16 the water tells apart moves nothing and says so.
4. Spikes 1, Spike spread 360 and Spike spread near 0 give a sane field (one tooth; teeth all round; no NaN).
5. A force's push never makes a drop's speed NaN or infinite (zero Reach, huge Strength, NaN from a hand-edited
   file): the push is dropped, the drop kept.

---

### Task 1: the engine's forces (`bpe-water`)

**Files:**
- Create: `crates/bpe-water/src/drops/forces.rs` (`Force`, `Push`, `ForceGpu`, `MOST_FORCES`, `MOST_PUSH`,
  `SPIKE_WIDTH` 0.5, `SPIKE_ROUND` 0.1, `MOST_DEPTH` 0.9, `SPIKE_FROM` 0.2, `SPIKE_WHOLE` 0.45, the host's own `magnet_potential`/`push_at` for tests)
- Create: `crates/bpe-water/src/drops/shaders/forces.wgsl` (`fn forced(i: u32, x: vec2<f32>, v: vec2<f32>, kind:
  u32, dt: f32) -> vec2<f32>`)
- Modify: `crates/bpe-water/src/drops/mod.rs` (`FrameGpu` gains `forces_n: [u32; 4]` and `forces: [ForceGpu;
  MOST_FORCES]` at its end; `Water.forces: Vec<Force>`; `set_forces`, `forces`; `copy` carries them; `frame_gpu`
  fills them; `steps_now` headroom), `drops/passes.rs` (`SOURCE` joins `forces.wgsl` after `sort.wgsl`),
  `drops/shaders/common.wgsl` (`Fr` gains `forces_n: vec4<u32>`, `forces: array<Force, 16>`, struct `Force`),
  `drops/shaders/sort.wgsl` (one call in `fall`), `src/lib.rs` (re-export `Force`, `Push`, `MOST_FORCES`)
- Test: `crates/bpe-water/tests/forces.rs` (new), unit tests in `drops/forces.rs`

**Interfaces (produces):**
- `pub enum Push { Magnet { pull: f32, spikes: u32, depth: f32, aim: f32, spread: f32 }, Whirlpool { speed:
  f32, rate: f32 }, Wind { velocity: Vec2, rate: f32 } }` (px/s², px/s, 1/s, radians), `Clone, Copy, Debug,
  PartialEq`.
- `pub struct Force { pub push: Push, pub place: Vec2, pub reach: f32, pub liquids: u32 }` (world px; bit k of
  `liquids` = liquid k), with `fn headroom(&self) -> f32` (px/s², the push the frame's steps make room for, at most
  `MOST_PUSH`·4000; as built a magnet's pull without its spikes, Task 6).
- `Water::set_forces(&mut self, forces: &[Force])` (from the next frame, at most 16), `Water::forces(&self) ->
  &[Force]`.
- `ForceGpu { a: [f32; 4] /* place (grid), reach (cells), most push (cells/s²) */, b: [f32; 4] /* by kind */, c:
  [u32; 4] /* kind 1 magnet 2 whirlpool 3 wind, liquids, spikes, 0 */ }`; WGSL `Force` matches.

- [ ] **Step 1: unit tests in `drops/forces.rs`**: `the_magnet_pulls_down_its_own_slope` (`push_at` equals minus
  the finite difference of `magnet_potential` within 1 % at 200 points in the reach, spikes on and off);
  `the_magnet_has_its_teeth` (along a ring at u = 0.5 the pull's size has exactly N maxima inside the spread for
  N in 1, 5, 9 and spreads 60, 180, 360, and none outside it); `nothing_past_the_reach` (push 0 at u ≥ 1 and at
  the middle); `odd_numbers_push_nothing` (NaN or infinite strength, reach 0: `push_at` is zero and finite);
  `the_card_layout_is_48_bytes`.
- [ ] **Step 2: engine tests in `tests/forces.rs`** (6 px cells unless said, lavapipe in the container):
  `no_force_at_strength_zero` (a pour with a magnet and a whirlpool at Strength 0 reads back the same bits as with
  no forces after 60 frames); `wind_blows_a_lone_drop_its_way` (no gravity, wind 1000 px/s right: after 1 s the
  drop moves right at 900 to 1000 px/s); `wind_moves_spray_more_than_a_pool` (a resting pool and a lone drop
  above it, wind right: the drop's speed over 5× the pool's mean after 1 s, the pool's deep drops under 20 px/s);
  `a_whirlpool_turns_the_water_round_it` (no gravity, a blob round it: angular momentum about its middle takes the
  sign of Strength, and the other sign the other way); `a_magnet_pulls_only_its_liquid` (no gravity, two blobs of
  liquids 0 and 1 beside it, mask liquid 1: liquid 1's mean distance to it falls by over a third in 2 s, liquid
  0's moves under 2 px); `a_magnet_stands_spikes_up` (a pool on a floor, a magnet 40 px under it, Strength 3,
  Spikes 5, spread 120: after 3 s the furthest drop by angle round the magnet has 5 peaks inside the spread,
  each over 12 px above the valleys beside it); `forces_play_the_same_twice` (wind with gusts' values, whirlpool
  and magnet: two bodies read back the same bits after 60 frames, and a copy at frame 20 too).
- [ ] **Step 3: run them**: `cargo test -p bpe-water --lib forces` and `cargo test -p bpe-water --test forces --
  --test-threads=2`; they fail (no `Force`).
- [ ] **Step 4: build** the engine side as in Decisions 1, 4 to 7, 9, 10: `fall()` calls `v = forced(i, p.xy, v,
  kind, dt)` when `!is_air(kind) && fr.forces_n.x > 0u`; magnets' pushes are added (`v += a·dt`, each held to its
  `a.w`), then whirlpools' and winds' drags in the list's order; a push that is not finite is left out.
  `frame_gpu` writes each force's place by `layout.to_grid`, lengths and speeds over `layout.cell`.
- [ ] **Step 5: run the tests** of Steps 1 and 2: pass. `cargo test -p bpe-water --lib` (numbers, passes) passes.
- [ ] **Step 6: commit** "Water part 4b: forces in the engine".

### Task 2: Force cards as the engine's forces (`bpe-kit`)

**Files:**
- Create: `crates/bpe-kit/src/things/water/draft/forces.rs` (`ForceSetup`, `gust`, `force_card_at`, the unit
  numbers `FORCE_UNIT`, `WIND_SPEED`, `WIND_RATE`, `SPIN_SPEED`, `SPIN_RATE`, `SPIKE_DEPTH`, notes `HEAT_LATER`,
  `NO_LIQUID_TO_MOVE`, `TOO_MANY_FORCES`), its tests inline
- Modify: `crates/bpe-kit/src/modules/water/places.rs` (`ForceCard`: `liquid: Target` "Liquid", `gusts` "Gusts"
  when wind, `spike_aim` "Spikes point" when magnet and spikes, `spike_spread` "Spike spread" when magnet and
  spikes, `spike_size` keyframed, Spikes default 9, docs per kind), `things/water/draft.rs` (`WaterSetup.forces`,
  notes, the print, `FrameInputs.forces`, `set_inputs`, `start`, `resume` set them), `draft/inputs.rs` (a
  `force:{k}` part each), `draft/tests.rs` (`what_restarts_the_draft`: the Force card, moving and turning off a
  Force restart)

**Interfaces:**
- Consumes: Task 1's `Force`, `Push`, `Water::set_forces`, `MOST_FORCES`.
- Produces: `pub struct ForceSetup { pub id: ThingId, pub kind: ForceKind, pub liquids: u32, card: Keyed, place:
  Vec2 }`, `ForceSetup::place_at(&self, time: f64) -> Vec2`, `ForceSetup::force_at(&self, time: f64) ->
  Option<Force>` (None for Heater, Cooler, no liquids); `pub fn force_card_at(scene: &Scene, resolved: &Resolved,
  id: ThingId, time: f64) -> Option<ForceCard>`; `pub fn gust(time: f64) -> f32` in −1..1.

- [ ] **Step 1: tests (inline in `draft/forces.rs`)**: `a_force_reads_its_card_each_frame` (a magnet whose
  Strength is keyed 0 at 0 s and 2 at 1 s: frame 30's pull is half frame 60's; its place, reach and spikes as
  set); `a_force_moves_the_liquids_it_names` (Any Liquid with three liquids: mask 0b111; a Liquid by name: its bit;
  a tag: the tagged; a turned-off Liquid: none, and the stage note); `wind_and_whirlpool_units` (Strength 1 wind
  aimed 90: velocity (0, 1000); whirlpool Strength −1: speed −600); `gusts_vary_the_wind_smoothly` (|g(t)| ≤ 1,
  changes under 0.1 between frames, Gusts 0 leaves the speed as it is); `heaters_say_they_come_later`;
  `seventeen_forces_say_too_many`. In `draft/tests.rs`: `what_restarts_the_draft` counts `cards::FORCE` as
  physics, moving and turning off a FORCE restart; `the_earliest_change_is_where_the_water_can_first_differ` gains
  a Force Strength key moved at 2 s (earliest change 0 or the key before).
- [ ] **Step 2: run** `cargo test -p bpe-kit --lib things::water`: the new tests fail.
- [ ] **Step 3: build** as in Decisions 2, 3, 6 to 9, 14, 15. The card-to-engine numbers: magnet `pull = S ·
  FORCE_UNIT / 1.7173`, `depth = SPIKE_DEPTH · size` (0 when Spikes 0), `aim`, `spread` in radians;
  whirlpool `speed = S · SPIN_SPEED`, `rate = SPIN_RATE`; wind `velocity = S · WIND_SPEED · (1 + gusts · gust(t))
  · (cos aim, sin aim)`, `rate = WIND_RATE`. The print hashes each force's id, kind, place, mask and card.
- [ ] **Step 4: run** the kit's water tests again: pass; `cargo test -p bpe-kit --test water_draft --test
  water_final -- --test-threads=2` still pass.
- [ ] **Step 5: commit** "Water part 4b: Force cards drive the water".

### Task 3: the stage shows a force (`bpe-studio`)

**Files:** Modify `crates/bpe-studio/src/water_mode/stage.rs` (`force_marks`, called beside `lamp_marks`); Test
`crates/bpe-studio/src/water_mode/tests.rs`.

- [ ] **Step 1: test** `every_force_kind_draws_its_marks` (a Water scene with one Force of each kind drawn in a
  kittest harness without a panic, and the force's note shown in its tooltip for a Heater).
- [ ] **Step 2: build** Decision 13 (colours: the marker's `ink`, `style::LINE_SOFT` for the ring).
- [ ] **Step 3: run** `cargo test -p bpe-studio water_mode`: pass. Commit "Water part 4b: forces on the stage".

### Task 4: the Ferrofluid look (`bpe-kit modules/water/looks.rs`)

**Files:** Modify `looks.rs` (`ReadyLook::Ferrofluid`, `ALL` of 8, title "Ferrofluid", words "Black glossy
ferrofluid on a pale plate, in soft even light." as built, liquid name "Ferrofluid", its row);
`modules/water/mod.rs` tests if they count looks; `crates/bpe-studio/src/water_mode/chooser.rs` only if its
layout needs it (two rows of four).

- [ ] **Step 1:** run `cargo test -p bpe-kit --lib modules::water` and `-p bpe-studio chooser` after adding the
  variant: every look test that walks `ALL` passes; `every_ready_look_draws_as_pinned` fails on a line count only
  where the golden is compared (the owner's card), so the golden is refreshed in Task 7.
- [ ] **Step 2: commit** "Water part 4b: the Ferrofluid look".

### Task 5: full-size look check of ferrofluid spikes

**Files:** Modify `crates/bpe-kit/tests/full_size_look.rs` (`water-ferrofluid`: a carved chamber, a pool of
liquid on its floor, a Magnet 40 px under the floor, Spikes 9, spread 160, Strength keyed 0 at 0 s, 3 at 1.5 s;
Coarse final 4 s; drawn in the Ferrofluid look at 1, 2.5 and 4 s: `water-ferrofluid-<s>`; it is also in
`water-looks` through `ALL`).

- [ ] **Step 1:** `LOOK_OUT=<scratch> LOOK_ONLY=water-ferrofluid LOOK_TAKES=<scratch>/takes cargo test --release
  -p bpe-kit --test full_size_look -- --ignored --nocapture`; open the full pictures and their quarter crops; look
  for spike tips rounded away by Smoothing, jagged edges, a highlight that doesn't sit on the spikes, flicker.
- [ ] **Step 2:** tune the Ferrofluid row and `SPIKE_DEPTH`/`SPIKE_WIDTH`/`SPIKE_FROM` until the spikes read as
  ferrofluid at full size; write the numbers in "As built". Commit.

### Task 6: strong forces and the steps a frame (Decision 11)

- [ ] **Step 1:** a by-hand measuring run in `tests/forces.rs` (`#[ignore]`d like `shots.rs`'s, run with
  `--ignored`): the spikes scene at Strength 4, Spike size 1, at 6 and 3 px, printing steps a frame, the fastest
  drop, the drops past the reach, and the spike tips' rms speed over the last second.
- [ ] **Step 2:** if the mark holds at 32 steps, note it in "As built" and stop. If not, build the calm skin
  (its own plan section first) and measure again.

### Task 7: notes, review, pull request, merge, hand-over

- [ ] Notes: `docs/notes/water.md` "Part 4b (as built)"; spec Parts 4b gains its **Built** line; this plan's "As
  built and measured".
- [ ] Lint: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] One Opus review of the branch; fix what it finds.
- [ ] Draft PR, then the "Refresh GPU pictures" workflow on the branch for `golden-water-looks.txt` (Ferrofluid's
  line); check its commit's diff moves only that line; every check green; merge (squash).
- [ ] Hand-over for parts 9 and 10 (`/mnt/project-files/ballphysics/water-mode/part9-10-handover.md`, or add to
  part 4's and 5's if they wrote one), project memory `water-4b`, message the coordinator.

## Things not to do

- No forces on air drops, no shelter from solids, no moving forces (place keyframes are part 5's, on Pieces and
  Walls), no heat.
- No new storage binding, no change to `Feel`/`FeelGpu`, `St`, `keep_off` or the solid buffer (parts 4 and 5).
- No Magnetic setting on the Feel card.
- Don't bless pinned looks locally; don't build or install the exe.

## As built and measured

- **Task 1 (engine)** as planned, but for the magnet's shape (decision 5, the star: the first shape's deepest place
  was out in the teeth and its pushes met their guard) and one thing found on the way: **no deep part inside a
  magnet's reach** (decision 5b, `in_magnet` in `deep.wgsl`): with it the mound stirred at about 100 px/s
  (`a_magnet_holds_its_mound_still` fails without it at 105). With it: the mound at Strength 3, Spike size 0.75,
  Spikes 9 over 160° holds still (rms 22 px/s at 6 px, 13 at 3 px; under 5 px/s inside), sharp cones with round
  valleys at both details.
- **Task 2 (cards)** as planned. Merged with part 4.1 (drains): a Drain's whole card is in its drain's print and
  parts now, so a spinning Drain's whirlpool restarts what part 4.1's drain does; the restart tests list DRAIN with
  FORCE. `WaterRun::renote` (the stage's held setup takes notes that change with no physics) was added for the
  Heater's note.
- **Task 3 (stage)** as planned (`force_marks`, beside part 4.1's `drain_marks`).
- **Tasks 4 and 5 (the look and its check).** The first row (two Spots, Edge sheen 0.55, Shine 0.95 of size 0.1)
  failed the full-size look: the Spots, placed by their share of the shot, were in the rock above the carved
  chamber and lit nothing, so the spikes had no highlight, and the sheen made every thin spike grey (a spike under
  about 50 px across is all edge to the surface rounding: it is tilted across its width, with a faint line where it
  meets the body). Two shadowless Suns from 55° and 125°: the opaque liquid's own shadow on the back filled the
  valleys between the spikes. From 72° and 108°: the spikes were right, but in the shared looks scene every drop on
  the ceiling, the stream and the slope's film threw long stripes across the pale back, stepped at the light map's
  4 px. As built: **no lamps**, Ambient 1, Shine 1 of size 0.6 (the gloss is the sky's highlight on the rims that
  face up), Edge sheen 0.12 (values in `docs/notes/water.md`); the words "in soft even light". The look check is
  Normal detail (2 px) for 4 s, drawn at 3 s, the frame after and twice as near (not Coarse at 1, 2.5 and 4 s: 2 px
  is where the spikes' points show). The faint band and a dashed one-pixel line where a thin spike meets the body
  are the look drawing's (Known).
- **Task 6 (strong forces)**, `strong_spikes_measured` (Strength 4, Spike size 1, Spikes 9 over 160°, 4 s, the
  strength rising over the first second), with decision 10's headroom:

  | Detail | Steps a frame | rms speed, 1.5 to 4 s | Tips moved over the last second | Flying past the reach after 1 s |
  | --- | --- | --- | --- | --- |
  | 6 px | 16 | 36 to 49 px/s | 0.65 px rms (most 1.1) | none |
  | 3 px | 32 | 15 to 20 px/s | 0.27 px rms (most 0.5) | none |

  Before (the headroom counting the spikes): 6 px took 32 steps, rms 75 to 93 px/s, the tips moved 3.1 px. At 6 px
  with the steps held to 16, 20, 24, 32, 41 (`BPE_WATER_MOST_STEPS`) the tips moved 0.65, 1.2, 1.3, 3.1, 3.3 px;
  Strength 4 with Spike size 0.75 forced to 48 steps churned at 107 to 140 px/s. The drops flying past the reach in
  the first half second are the pool spreading to the box's corners, as it does with no magnet. The two outer teeth
  reach the reach's edge (a tooth lying near the floor fills to it).
- **Review fixes.** (1) A magnet's reach is the drops' own only in cells holding its liquids (the deep gather ORs
  each cell's liquids into a seventh sum): before, a deep tank of water over a magnet for another liquid sank 10 px
  and stirred at 13 to 29 px/s against 3 to 5 (`a_magnet_leaves_other_liquids_deep_water_alone`, which fails on the
  old shader at 17 px). (2) Narrow teeth: 9 over 30° or 60° at Strength 4, Spike size 1 stirred at 165 to 260
  px/s (the round slope grows as depth over the tooth's width); the depth is now held to `MOST_STEEP` 3.9 × the
  half width (radians), the steepness of 9 over 160° at 0.6, which leaves the card's defaults and the look check
  as they were: 30° then stirs at 7 to 12 px/s, 60° at 26 to 32, 100° at 37, 20 over 360° at 40 to 55. (3) A
  Drain past the 32 drains spins nothing, a Whirlpool keyed only at 0 isn't a force, and a spinning Drain past 16
  forces says it still drains (`TOO_MANY_SPINS`). (4) A pull too big for a float holds at the most push instead
  of none (size and way apart, on the card and the host).
