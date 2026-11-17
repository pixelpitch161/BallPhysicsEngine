# Liquid part 2: how it acts with balls (plan)

Spec: `docs/superpowers/specs/2026-11-17-liquid-thing-design.md` sections 5 (Detail), 6.3, 10 and 12 (part 2).
Builds on part 1 (`docs/notes/liquid.md`, `plans/liquid/part1.md`). Aedan asked for the whole liquid feature to be
finished and merged without approval stops; decisions below are the builder's, from the spec.

## Goal

Balls float, bob, sink, are slowed and carried by the liquid; balls throw splashes, drag wakes, take the liquid's
colour and leave ink in it; Enter liquid / Leave liquid / Splash events reach the Rules. Two plays of a scene, and a
seek, give exactly the same balls on one machine.

## Decisions

1. **Where it steps.** A Liquid thing whose Meets card has *Balls push it* and *Pushes balls* on steps in the main
   world (`ThingSim::in_main` true). Because all liquids share one body held by the first, such a liquid **keeps**
   every other Liquid thing (`ThingSim::keeps(scene, me, other)`: me pushes balls, other is a Liquid), so the whole
   shared liquid is in the main world or beside it, never split. *Pushes balls* is not keyframable (a plain switch),
   so the saved value is the value. A rule's When **Liquid** block keeps the liquids it names too
   (`BlockSim::keeps`). Default: Pushes balls **on** (a liquid that ignores the balls is an anti-goal).
2. **Read-back, one step late.** The host knows it is in the main world when `!world.beside.contains(&id)`. Then
   each liquid step records into a per-ball **feel** buffer on the card (fixed-point `i32` atomics: contact
   impulse, touching mass, colour, drag and carry weights, per-liquid contact counts) and a per-ball ring sample
   (`cs_feel`, after the substeps: 32 points at radius r + 1.25 cells, wet where the grid's volume > 0.4, the mean
   grid speed of the wet ones). After the after-step's batch of liquid steps the host sends one copy of the feel
   buffer to a map-read (`Body::send_feel -> Arc<Feel>`) and clears it for the next batch. At the next world step's
   first `forces` call the host waits for it (`Feel::wait`, which polls the card until mapped and keeps the words in
   a `OnceLock`, shared by every clone of the world, so a checkpoint or snapshot taken in between reads the same
   words). Deterministic: the sums are integers, and the wait never depends on timing.
3. **What the balls feel** (per ball, worked out on the processor from the feel words):
   - **Push**: Σ over every drop contact of −m·Δv (the contact's inelastic change, before any splash extra), each
     liquid's contacts scaled by `1 / ball density` for that liquid. Δv_ball = push / (π r²) (grid units), × cell.
   - **Ball density** (Floats or sinks): *By ball weight* uses Ball weight (water's Weight is 1); *Floats* 0.4 ×
     the liquid's Weight; *Sinks* 2.5 ×; *Bobs* 0.85 ×. A ball lighter than the liquid floats, by the liquid's own
     pressure on it (no buoyancy formula: the push is the pressure).
   - **Drag and carry**: under = wet samples / 32. Δv = −(v_ball − carry · v_liquid)(1 − e^(−k·under·dt)), with
     k = 3/s × Drag × (1 + 8 × Thickness) averaged over the touching liquids by touching mass, dt the batch's span.
   - Both clamped to the world's speed ceiling; held and removed balls are left alone; Δv applied once (the first
     substep), as `TwoWayOut` does.
4. **Events** (new `EventKind::Liquid`, tag 23): `a` the ball (thing, row), `b` the Liquid thing with item 0 Enter,
   1 Leave, 2 Splash; `point` the ball's place, `normal` its way, `speed` its speed. Per (ball, liquid): under share
   = under × that liquid's contacts / all contacts; Enter when it passes 0.55, Leave when it falls under 0.35
   (or the ball goes), Splash with an Enter whose speed is at least the liquid's *Splashes from*. Worked out when the
   feel is taken, logged in the host's next `after_step` (the log is there). Rules see them the same step.
5. **On the drops** (in `cs_g2p`'s ball loop, per liquid, for a ball whose mask names that liquid):
   - **Splash size** s: a contact faster than *Splashes from* (hit = −vn − from) throws the drop out by
     (s − 1) · 0.5 · hit when s > 1; when s < 1 its sideways speed against the ball is scaled by s.
   - **Sticks to balls**: drops within 1.5 cells of the surface are pulled in (40 cells/s² × stick) and take the
     ball's speed at 1 − e^(−20·stick·dt); recorded as push (the ball carries the clinging liquid).
   - **Wakes**: drops within 3 cells behind a moving ball (off · v_ball < 0) take its speed at
     1 − e^(−4·wakes·dt), fading with distance. Not recorded (it is a look of the liquid).
   - **Balls drop ink**: a touched drop's colour moves to the Ink colour at 1 − e^(−12·ink·dt).
   - Drops never leave their wall side (part 1's `side0` stays).
6. **Balls take its colour**: the host keeps per ball (thing, row) a taken colour and amount; while touched it moves
   to the mean colour of the touching drops at 3/s × Take × under. `balls::draw::colours_with` mixes it into the
   ball's base colour (so trails and particles that take the ball's colour see it too). No map, no change: a frame
   without a liquid stays byte-identical.
7. **Which balls**: Meets card *Meets balls* (Target, default Anything). A ball is uploaded with a 16-bit mask of the
   liquids that meet it (Balls push it on and their Target names its thing); a ball no liquid meets isn't uploaded,
   nor one wholly outside the grid.
8. **Export detail** (spec 5): when the liquid is in the main world, export uses the preview Detail unless the
   first Liquid thing's **Finer export** is on ("Balls will take different paths from the preview").
9. **Every playback has the card**: the editor's sound thread and its warm-up helper and the aim job name the card
   the editor's thread has (as the particle job does); `bpe-cli`'s sound thread names the export's card and
   `exporting_for_now` (so a Finer export's sound hears the same liquid).
10. **Fingerprints**: in the main world every Liquid setting restarts the physics (already `key_restarts_physics`
    true; Fill and Meets are hashed whole as `steps_own_cards`); `physics_print_extra` adds the Look's stepped
    mixing keys and the thing's place among the scene's Liquid things (the first sets the area).
    `restarts.rs` also hashes `inputs_hash` of main-world liquids.
11. **Status line**: the readout also comes from the main playback's liquid (`app::main_liquid`).

## Settings (Meets card, group "With the balls")

| Key | Label | Type, default | when |
| --- | --- | --- | --- |
| balls_push | Balls push it | bool, on | |
| pushes_balls | Pushes balls | bool, on (not keyframed) | balls_push |
| target | Meets balls | Target, Anything | balls_push |
| floats | Floats or sinks | By ball weight / Floats / Sinks / Bobs | balls_push and pushes_balls |
| ball_weight | Ball weight | 0.5 ×, span 0–4 | … and floats=by_ball_weight |
| drag | Drag | 1, span 0–4 | balls_push and pushes_balls |
| carries | Carries balls | 1, span 0–2 | balls_push and pushes_balls |
| splash | Splash size | 1, span 0–4 | balls_push |
| splash_from | Splashes from | 400 px/s, span 0–3000 | balls_push |
| sticks | Sticks to balls | 0, span 0–1 | balls_push |
| wakes | Wakes | 0, span 0–4 | balls_push |
| ink | Balls drop ink | 0, span 0–1 | balls_push |
| ink_colour | Ink colour | magenta | balls_push and ink>0 (as `ink!=0` if the grammar has no `>`) |
| take | Balls take its colour | 0, span 0–1 | balls_push and pushes_balls |

Liquid's own: `finer_export` "Finer export" bool, off, group Detail.

## Files

- `bpe-gfx/src/liquid_sim/mod.rs`: `LiquidGpu` gains `e` (push scale, drag k, carry, sticks), `f` (ink rgb, ink),
  `g` (splash, splash from in cells/s, wakes, take); `BallGpu.mask`; binding 12 `feel`; `StepIn { feel, fresh }`;
  `Body::send_feel`, `Feel`, `FEEL_WORDS` = 32, `RING` = 32; `cs_feel` entry; feel buffer in `Scratch`.
- `bpe-gfx/src/shaders/liquid_sim.wgsl`: the structs, the ball loop (5), `cs_feel`.
- `bpe-gfx/src/liquid_sim/tests.rs`: a still ball in still water feels an upward push; a ball moving into still
  water is pushed back; masks leave balls out; the feel is the same twice.
- `bpe-kit/src/things/liquid/cards.rs`: `MeetsSettings` (table), `Floats`.
- `bpe-kit/src/things/liquid/feel.rs` (new): `Felt` (the uploaded balls), `apply` (3), `events` (4), taken colours.
- `bpe-kit/src/things/liquid/sim.rs`: `in_main`, `keeps`, `forces`, `physics_print_extra`, after_step's batch with
  one ball list, `send_feel`, logging the news; layout's Finer export rule.
- `bpe-kit/src/things/liquid/state.rs`: `Host` gains `felt`, `pending`, `inside`, `news`, `taken`.
- `bpe-kit/src/things/liquid/rules.rs` (new): When **Liquid** (`when.liquid`): Who, Liquid, When it (Goes in /
  Comes out / Splashes / Any); values speed, size, ball, way; keeps the liquids it names. Registered beside
  `when.particles`.
- `bpe-runtime/src/events.rs`: `EventKind::Liquid` (tag 23) and every match that needs it.
- `bpe-kit/src/things/balls/draw.rs`: taken colour in `colours_with`.
- `bpe-studio`: sound_job, aim_job name the card; status line reads the main liquid.
- `bpe-cli/src/lib.rs`: the sound thread names the card and the export guard.
- Tests: `things/liquid/tests.rs` (behaviour), `tests/golden_liquid.rs` (+ a test scene), `tests/restarts.rs`.
- Notes: `docs/notes/liquid.md` part 2 as built.

## Tests (what passing looks like)

- `bpe-gfx` lib `liquid_sim` tests (`--test-threads=2`).
- Kit behaviour on the software card (`cargo test -p bpe-kit --lib things::liquid -- --test-threads=2`):
  - a ball of Ball weight 0.3 dropped into the water ends near the surface (its centre within a radius of the
    level after 4 s); one of 3.0 ends on the ring's bottom; a still ball held nowhere sinks slower in honey than
    water (Drag × Thickness);
  - a ball dropped in logs Enter liquid then a Splash (fast enough); a floating ball doesn't Leave; a rule When
    Liquid → Break breaks the ball when it goes in;
  - Pushes balls off: the balls hit exactly as without the liquid (the liquid then steps beside);
  - two runs and a seek give the same ball positions and log;
  - a ball with Balls take its colour 1 comes out tinted; Balls drop ink 1 turns the drops it touched;
  - `in_main`/`keeps`: one pushing liquid keeps a non-pushing one in the main part.
- `golden_liquid.rs`: births and counts as before (bless debug and release); per scene: a seek and a second run
  give the same ball log and positions; with Pushes balls on the balls hit *differently* from without the liquid;
  a scene copy with Pushes balls off hits exactly as without.
- `restarts.rs` (no card; checks the fingerprint wiring), `rules` block list test, `cargo clippy -D warnings`,
  `cargo fmt`.
- Full-size look check `LOOK_ONLY=liquid-water,liquid-mixing`: the water with its ball must still look calm.

## Not doing in part 2

Wet balls drying, shine (part 3); Changes the ball, heat, From balls (part 4); liquid sounds (part 5); ink of the
ball's own drawn colour (the sim can't see draw colours: an Ink colour setting instead); aim test flights ignore the
liquid (a lone copy keeps only the flying thing's world).

## Risks

- Lag + stiff contact may make a floating ball jitter: Drag damps it; if not enough, average the push over the batch.
- Weakly compressible pressure may over- or under-float: calibrate with the float test before tuning anything else.
- Blocking wait per world step costs the card's latency (≈0.2–1 ms); only main-world liquids pay it.
