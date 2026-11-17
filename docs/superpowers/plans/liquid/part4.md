# Liquid part 4: stranger liquids (plan)

Spec: `docs/superpowers/specs/2026-11-17-liquid-thing-design.md` sections 6.1 (Stretch, Tears at), 6.2 (Heat), 6.3
(Changes the ball), 6.4 (Rain, From balls, Drains, Currents, Magnets), 6.5 (React), 8 (Shapes paint it, Picture in
the liquid, Fades, Un-mix) and 12 (part 4). Builds on parts 1 and 2 (`docs/notes/liquid.md`,
`plans/liquid/part1.md`, `part2.md`). Aedan asked for the whole liquid feature to be finished and merged without
approval stops; every decision below is the builder's, from the spec. Parts 3 (look) and 5 (sound) are built at the
same time: this part keeps to the solver, the Fill / Meets cards, the Liquid's own settings and one Look group.

## Goal

Slime that stretches into strands and snaps, lava that cools into a thick crust and freezes, water that dries and
leaves stains, two liquids that react into a third with fizz and heat, rain, balls that leak and burst into liquid,
drains with whirlpools, pumps and wind, magnets with ferrofluid spikes, balls that dissolve, shrink or grow in a
liquid, shapes that paint the liquid they touch, a Fill made of a picture or a word, ink that fades, and **Un-mix**:
the liquid plays its own last seconds backwards. All deterministic on one machine (seek and replay give the same
drops), all settings keyframable unless said.

## Decisions

### Per drop (Drop grows from 80 to 96 bytes)

1. `Drop` gains `f: [f32; 4]` (the elastic stretch F, xx xy yx yy; identity at birth) **after** `spare`, and
   `spare` becomes `[temperature, dryness]`. 96 B × 2^20 = 96 MiB, still under the 128 MiB binding limit.
   `liquid_drops.wgsl` reads drops 24 numbers apart (part 3 reads them through `DROP_FLOATS`; whoever merges second
   sets it to 24). Offsets of x, v, colour, kind, serial, kick don't move.
2. `kind` bits: 0..7 the liquid's number; **stains** are 16 + the liquid's number (so the normal draw, which keeps
   only `kind & 255 == index`, never shows them); bit 8 `STOPS` (as before); bit 9 `FROZEN`. `liquid_of` becomes
   `(kind & 255) & 15`, so a stain reads its own liquid's numbers. Every kernel but compaction and the draw skips
   stains (`is_stain`): they carry no mass, don't move, don't mix.

### Stretch and Tears at (Liquid settings, "How it flows")

3. **Stretch** (0..∞, span 0..1, default 0): an elastic shear part. Each substep in `cs_g2p`, F ← (I + dt·C)·F.
   In `cs_p2g`, with b = F Fᵀ and J = det F, the deviatoric neo-Hookean stress σ = μ (b/J − ½ tr(b/J) I) is added
   to the fluid stress (pressure still comes from the grid's J, so the two never fight). μ = min(¼·Stretch, 1) ×
   the liquid's stiffness, so the shear wave is never faster than the pressure wave the substeps are chosen for.
   F stays the identity (no work) for a liquid with Stretch 0 that isn't frozen.
4. **Tears at** (1..∞ ×, span 1..5, default 2): a drop stretched further than Tears at times its length (the
   larger of |ln σ₁|, |ln σ₂|, σ from b's eigenvalues) snaps: F ← I (it forgets its shape and flows on). A det F
   under 0.05 or not finite also resets.

### Heat and change (Liquid settings, new "Heat" group)

5. **Temperature** (0..1, default 0.5): each new drop's heat (`spare[0]`).
   **Cools** (0..∞, span 0..2, /s, default 0): heat lost at the surface: T ← T − Cools·exposure·T·dt, exposure =
   clamp((0.9 − vd) / 0.5, 0, 1), vd the gathered grid volume (1 inside the liquid, less at its edges).
   **Warms** (0..∞, span 0..2, /s, default 1): how fast heat sources heat it. Heat sources are **Heat** layers on the
   Fill card (place, radius, strength −1..1: below 0 cools) and the Meets card's **Balls heat it** (0..∞, span 0..1):
   a touched drop's T moves to 1 at 1 − e^(−4·heat·dt). (The spec's "balls with a Heat glow sub-layer" and "Light
   things with heat" are left out: Heat glow is draw-only, worked out from hits when drawn; noted in Not doing.)
   Drops share heat with their neighbours at each mixing (conduction, ¼ of the way, every liquid with every other).
6. **Cold thickens it** (0..∞, span 0..1, default 0): viscosity × (1 + 30·k·(1 − T)²), clamped to the cap.
7. **Freezes** (switch, default off), **Freezes below** (0..1, default 0.2), **Melts above** (0..1, default 0.3):
   a drop under Freezes below gets `FROZEN` (F reset to I, so its shape then is its rest shape) and is a stiff
   solid: μ = ½ stiffness, viscosity the cap, no tearing; over Melts above it melts. Balls skid on it (contact keeps
   only the normal change, as before).
8. **Dries up** (0..∞, span 0..1, /s, default 0): dryness += Dries·exposure·dt; a drop goes when its dryness passes
   its own threshold (0.5..1.5 from a hash of its serial, so a puddle dries from its edges, not all at once).
   **Stains** (switch, default off) and **Stain strength** (0..1, default 0.5): a dried drop instead becomes a stain
   (kind 16 + its number, speed 0) that stays where it dried. `draw.rs` pushes a second `Liquid` for the stains
   with index 16 + n, a flat style (edge level so one lone drop shows, no roundness or shine, opacity = Stain
   strength) built from the Look through the same `style_of`. Stains count towards Most drops (the oldest go).

### React (Meets card, Other liquid rules)

9. **Meeting** gains **React**, with **Makes** (a Target naming the Liquid thing it turns into), **Speed** (0..1,
   default 0.3), **Heat** (0..1, default 0) and **Fizz** (0..1, default 0). A pair reacts both ways when either side's
   rule says React (as Layer wins); the first liquid's rule wins when both do. Pair word: mode 2 = react, target
   liquid in bits 7..10; a second table `pairs2` holds heat and fizz (8 bits each). In `cs_mix` each drop finds the
   reacting neighbour with the most weight; chance = 1 − e^(−8·speed·w) a mixing, against a hash of (serial, kind,
   liquid step) — no shared state, so it is the same on every play. A converted drop takes the target's number
   (keeps STOPS, loses FROZEN), the target's Fill colour (worked out once a step from its Paint at time 0, strength
   its Colour strength), its own Weight/viscosity offsets reset, T + Heat, and a kick up of Fizz × 40 cells/s with a
   hashed sideways part (the fizz). A target past the 16th liquid never reacts. Sound for reactions is part 5's.

### Fades (Look card, "Colour mixing" group)

10. **Fades** (0..∞, span 0..1, /s, default 0) and **Fades to** (colour, default white): at each mixing a drop's
    colour moves to Fades to at 1 − e^(−fades·dt_mix). Stepped: added to `LookSettings::stepped()`,
    `card_key_restarts_beside` and `beside_print_extra`.

### Shapes paint it (Meets card, new group "With the shapes")

11. **Shapes paint it** (0..∞, span 0..1, default 0), **Paint colour** (default orange), **Painting shapes**
    (Target, default Anything). Each walls band node gets an 8th word: its wall's number (0..63 in `walls_now`
    order; 255 for later walls). Each step `paints: [[u32; 4]; 16]` in the uniform holds, per wall number, the mask
    of liquids it paints (from the live settings, so keyframes work without rebuilding the band). A drop within half
    a cell of a painting wall's face moves to its liquid's Paint colour at 1 − e^(−12·paint·dt).

### Forces: Heat, Drains, Currents, Magnets (Fill card layers, section Force, repeating)

12. A `forces: [ForceGpu; 48]` table in the uniform (`extra[1]` = how many), each `{a: [x, y, reach, strength],
    b: [dir x, dir y, more, more2], c: [kind, liquid mask, spikes, 0]}` in grid units. Applied in `cs_g2p` after
    the gather, before damping, to drops whose liquid's bit is in the mask. Past 48 the rest are left out (the
    editor's layer list has no cap; noted in the notes).
    - **Heat** (`liquid.heat`): place, Radius (px, 80), Strength (−1..1, 1). Inside: T moves to 1 (or 0 below 0)
      at Warms × |strength| × (1 − d/r).
    - **Drain** (`liquid.drain`): On, place, Radius (px, 16), Whirlpool (0..∞, span 0..4, 1). Drops within the
      radius go (in `alive`). Within 6 radii: a pull in (60 cells/s² × falloff) and a spin of Whirlpool × the pull.
    - **Current** (`liquid.current`): On, Kind (Pump / Whirlpool / Wind), place, Aim (°), Length (px, 200), Width
      (px, 60), Strength (span 0..4, 1). Pump: in the capsule along Aim, accelerate along Aim by 400 px/s² ×
      strength. Whirlpool: within Length/2 a spin round the place, falling off, and a slight pull in. Wind: a push
      along Aim on drops at the surface (× exposure), anywhere. (Wind from the scene's Air card is not read: it
      lives in the particles' air; noted.)
    - **Magnet** (`liquid.magnet`): On, At (Its place / Balls), Balls (Target), place, Strength (span 0..4, 1),
      Reach (px, 250), Spikes (0..24 lobes, 0), Spike size (0..1, 0.5). Pull toward it: 300 px/s² × strength ×
      (1 − d/reach)² × (1 + size·cos(n·θ)) for n spikes. At Balls: one force per named ball (radius added to
      reach), up to the table's room.
    - Heat, Drain, Current and Magnet places are from the Liquid thing's place (as Pours).

### Rain and From balls (Fill card)

13. **Rain** group: **Rain** (raindrops a second, 0 off), **Raindrop size** (px, 8), **Rain speed** (px/s, 500).
    Each raindrop is a little disc of drops (as many as fill it, at least 1) at a random x along the top of the
    liquid's region (its shape's box, or the Area), born only where a drop may be (`room_at`), falling at Rain speed,
    in the Fill colour. Dice: `Stream::new(id ^ RAIN, liquid step)`.
14. **From balls** group: **Balls** (Target, default Anything), **Leaks** (drops a second per moving ball, 0 off,
    × (speed / 500 px/s, at most 4)), **Bursts** (0..∞, 0 off: a ball removed from the world bursts into Bursts ×
    its own area of liquid, thrown out at its speed). Leaked drops come off the back of the ball. Removal is seen by
    comparing the named balls alive last world step with now (a `BTreeMap` in the state, so the order is fixed).
    In the Fill colour (the ball's own colour is worked out only when drawn).

### Changes the ball (Meets card, "With the balls")

15. **Changes the ball**: Off / Dissolves / Shrinks / Grows / Takes its colour, and **Change speed** (0..∞, span
    0..2, /s, 0.5). Only when it pushes balls (main world). Feel words 12 (shrink rate × mass, grow below 0) and 13
    (dissolves × mass) are summed like Drag. In `feel::apply`: radius × e^(−k·wet·span); the host keeps each ball's
    size when first touched (`sizes`), Shrinks stops at 0.3 of it, Grows at 3 times, Dissolves removes the ball
    under 0.05 of it (or 0.5 px) with a Remove event (`bpe_runtime::pop`, in the next after-step with the log).
    Takes its colour lifts Balls take its colour to at least the change speed.

### Picture in the liquid (Fill card, "Fill")

16. **Fill with**: Level (as now) / A picture / A word. A picture: **Picture** (PNG, as the Particles one, or kept in
    the scene), **Picture width** (px, 400): a lattice of drops two a cell over its solid parts (alpha ≥ ½), centred
    on the Liquid thing's place, each the picture's colour there. A word: **Text**, **Letter height** (px, 200), in
    the Fill colour. Born at the start (they then fall and flow); Level keyframes do nothing for them.
    `particles::spots::read_png` / `read_png_from` become `pub(crate)`.

### Un-mix (Liquid settings, new "Un-mix" group, and a rule)

17. **Remember** (switch, default off: nothing kept, no memory), **Seconds** (0.1..60, span 1..20, default 5),
    **Un-mixing** (switch, keyframable: while on, the liquid plays backwards). A rule's Do **Un-mix** block
    (`do.unmix`: Liquid (Who)) plays back everything remembered (up to Seconds); it reaches the liquid through
    `ThingSim::fire_now`, and keeps every Liquid thing in the main world (`BlockSim::keeps`). The first Liquid
    thing's Remember and Seconds are used; any Liquid thing's Un-mixing plays back the whole shared liquid (all
    Liquid things are one body; rewinding one would tear the body apart).
18. **History**: every 1/60 s (every 4 liquid steps at 240 a second, 2 at 120) `cs_record` packs every drop into its
    own new card buffer (20 bytes: place as two u16 fractions of the grid, colour 10:10:10 square-root coded up to
    4, kind and temperature as f16, the serial, strength and dryness as f16), with the count in a 16-byte header.
    The body keeps a `VecDeque<Arc<Sample>>`: samples never change once written, so copies of the world (checkpoints,
    snapshots) share them for free; only bytes are counted (`world.gpu_memory`, so checkpoints thin as they do for
    the swarm). Past Seconds × 60 samples or 1 GiB the oldest go.
19. **Playing back**: each liquid step while un-mixing, `cs_rewind` writes the drops between the newest sample A and
    the one before B (t = steps into the gap / steps a sample): it takes B's drops when t ≥ ½, else A's, and where
    the other sample has a drop with the same serial and kind at the same index it slides between the two; speed,
    C, J, mixing offsets and F reset (they come back still), then pushes them out of the balls that push it (place
    only: balls still push it, it pushes no balls). Past each sample A is dropped, so history is used up and the
    liquid goes on from where it was rewound to. With one sample left it holds still there until Un-mixing goes off.
    No births, mixing or feel while un-mixing; balls keep whatever was in or out (no Leave events from the pause).
20. The editor shows Un-mix's memory beside Seconds (`inspector/own.rs`, as the drops line): about drops × 20 B × 60
    × Seconds.

### Restarts

21. New Liquid settings and Fill / Meets settings and layers are read in `read_into`; `inputs_hash` adds the force
    layers. Every stepped Look key (`fades`, `fades_to`) is in `card_key_restarts_beside` and `beside_print_extra`.
    `restarts.rs` must pass.

## Files

- `crates/bpe-gfx/src/liquid_sim/mod.rs`: `Drop.f`, `FROZEN`, `STAIN_BASE`, `LiquidGpu` h..m, `ForceGpu`,
  `FORCE_*`, `MOST_FORCES`, `StepGpu.extra/forces/paints/pairs2`, `set_pair` react, `WallNode.wall`, band 8 words,
  `Sample`, `Body::history`, `record`, `rewind_step`, `history_bytes`, entries `cs_record`, `cs_rewind`,
  `cs_rewind_done` (bindings 13 rw sample, 14 newer, 15 older).
- `crates/bpe-gfx/src/shaders/liquid_sim.wgsl`: everything above in the kernels.
- `crates/bpe-gfx/src/shaders/liquid_drops.wgsl`: stride 24.
- `crates/bpe-gfx/src/liquid_sim/tests.rs`: solver tests.
- `crates/bpe-kit/src/things/liquid/`: `mod.rs` (settings), `cards.rs` (Fill, Meets, Look fields; Heat, Drain,
  Current, Magnet layers; React), `forces.rs` (new: layers to `ForceGpu`), `births.rs` (rain, from balls,
  picture / word), `sim.rs` (step settings, un-mix, from balls, fire_now), `state.rs`, `feel.rs` (changes the ball),
  `walls.rs` (wall numbers), `draw.rs` (stains), `rules.rs` (`do.unmix`), `tests_part4.rs` (new).
- `crates/bpe-kit/src/lib.rs` (register `do.unmix`, the new layer kinds), `things/particles/spots.rs` (pub(crate)).
- `crates/bpe-studio/src/inspector/own.rs`: Un-mix memory line.
- `docs/notes/liquid.md`: "Part 4, as built".

## Tests (what passing looks like)

- `liquid_sim::tests` (graphics card): a stretchy blob held then dropped keeps together more than water (spread
  of drops smaller); a stretched drop past Tears at has F reset; frozen drops keep their relative places under
  gravity; a drain removes the drops in it; a magnet gathers drops near it; a React pair turns drops into the third
  liquid; Fades moves colour; record + rewind returns the drops to their places (within 0.01 cells); dries removes
  drops and leaves stains with Stains on; a body copy shares its history.
- `things::liquid::tests_part4` (kit, graphics card): rain makes drops each second; a burst ball makes drops; a
  picture fill makes drops in the picture's colours; Un-mixing (switch) brings the mean drop place back to where it
  was; a `do.unmix` rule does too; Dissolves removes a ball with a Remove event, Shrinks makes it smaller, Grows
  bigger; Shapes paint it turns drops near a wall to the paint colour; a seek gives the same drops hash with every
  new feature on.
- `golden_liquid.rs` lines unchanged (new features default off); all goldens unchanged.
- `restarts.rs` passes; clippy, fmt.

## Not doing in part 4

- Balls with a Heat glow sub-layer and Light things heating the liquid (Heat glow is draw-only); the Air card's wind.
- Pour or leak colour "From the ball" (a ball's colour is worked out only when drawn).
- Reaction sounds (part 5), glow by temperature (part 3's look).
- Un-mixing one Liquid thing while the others go on.

## Risks

- Stretch instability: μ bounded by the stiffness; Tears at resets F; det guard.
- Uniform size: Step grows to about 9 KB (limit 64 KiB).
- History memory: capped at 1 GiB and Seconds; shared by copies.

## As built

Built as planned, with these differences:

- The Un-mix rule has no For: `Action::Fire` carries no number, so a rule plays back all that is remembered (the
  first Liquid thing's Seconds). It reaches the liquid through `ThingSim::fire_now`, so the general Fire block
  naming a liquid un-mixes it too.
- Dissolves removes a ball at 0.2 of its first size (not 0.05): a ball much smaller than a grid cell is thrown about
  by the drops (the push is over its area), out of the liquid, and stopped shrinking.
- Rain looks down from the top of the region for the first place a drop may be (up to 400 cells), so it falls into
  round shapes, whose box top is mostly outside them.
- Reaction speed is squared before it reaches the card (gentler at low settings), as Mix speed is.
- A batch of liquid steps that played back sends no feel and no Leave events (`rewound` in `after_step`); the next
  normal step starts the feel afresh.
- The Fill card's list is now "Pours and forces" (+ Add pour or force), offering Pour, Heat, Drain, Current and
  Magnet.
- Review fixes: Un-mix keeps at most about 256 MiB, as a number of samples from the liquids' kept Most drops (a
  byte cap from the card's count would differ between runs); reaction ties go to the lower liquid number; made
  drops take the made liquid's newest serial; Shapes paint numbers the walls of the same step as the band; picture
  fills look at most 4000 × 4000 places; rain, leaks and bursts stop at the most one liquid step can be born.
