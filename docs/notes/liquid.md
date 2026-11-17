# Liquid thing

Spec: `docs/superpowers/specs/2026-11-17-liquid-thing-design.md`. Plans: `docs/superpowers/plans/liquid/`.

## Part 1, as built: the liquid itself, with colour and mixing

**What the owner sees.** "+ Add thing" has a **Liquids** heading with **Liquid** (a drop icon). A
Liquid thing has its own **How it flows** settings (Thickness, Weight, Surface pull, Springiness,
Spray, Settles, Gravity), **Detail** (Detail, Export detail, Steps a second) and **Area** (Shot at
the start, Fit the shapes, Custom with a Margin), and three cards: **Fill** (the Motion slot: Fill
at the start, Level, Colour, Colour strength, Most drops, and a list of **Pours**), **Meets** (the
Bounce slot: Balls push it, and a list of **Other liquid** rules: Mix or Layer, a mix style, its own
speed) and **Look** (Colour from, Mix style Paint / Light / Blend / Marble / Keep apart, Mix speed,
Mixes when, Swirl detail, the surface). Put under a hollow shape it fills the shape; anywhere else
it fills the Area and stops at its edges. The status line says "· Liquid: N drops, X ms", or
"· Liquid needs the graphics card" when there is none. No built-in scene uses it (the owner asked
for none); the test scenes are `scenes::liquid_scenes` ("Liquid water", "Liquid paint mixing").

**The solver** (`bpe-gfx/src/liquid_sim/`, `shaders/liquid_sim.wgsl`) is MLS-MPM, weakly
compressible, on the graphics card, in grid units (a cell is 1). Particle-to-grid uses fixed-point
`i32` atomics (mass and volume × 2^20, momentum × 2^12, each contribution clamped), so a step is
the same whatever order the card runs it in: two runs, a seek and a copy of the world give exactly
the same drops on one machine. J comes from the grid's volume density. Pressure is `E(1 − J)`,
cohesion `−E · pull · (J − 1)` up to `1 + tear`, viscosity `μ(C + Cᵀ)`. Damping, a speed cap and
substeps (`ceil(c · dt / 0.15)`, `c² = g · H / ε`; at 0.4 still water gained energy and tore itself into foam within a second) keep it stable. Every 4 liquid steps the drops are
binned by cell and mixed (fixed-point sums again): Paint (Kubelka-Munk over 8 bands with a
pseudo-inverse back to rgb), Light, Blend (OKLab), Marble and Keep apart pass no colour; Layer
pushes two liquids apart and lifts the lighter. The dead (out of the Area, past Most drops by a
serial floor, above a falling Level's cut line) are removed by a stable prefix compaction, as the
Huge swarm does. `Body` holds the drops; `Clone` copies them on the card (the kit keeps it in an
`Arc` and copies on write). A late count read gives the status line its count and milliseconds; at
most 3 steps are in flight.

**Walls** (`kit things/liquid/walls.rs`) are a band of grid nodes near every wall balls meet: each
node has the wall's normal (off a filled wall, out of a hollow one's outline), its signed distance
from the wall's line, half the wall's thickness (−1 for a filled wall) and the wall's velocity.
A drop is kept on *its own* side of a hollow wall's line, worked out from wherever the nearest node
is; an earlier version that trusted the nearest node's side let drops through a thin ring. The band
is put on again when the walls change or move, and on a copied body (whose band is gone).

**The world** (`kit things/liquid/sim.rs`). Every Liquid thing shares one body, held by the first
Liquid thing in tree order (the host, `Host` in `state.rs`). It is stepped beside the physics
(`in_main` false), last in the world step, 240 or 120 liquid steps a second from the host's Steps
a second. The host's after-build lays the grid over the Area (from the camera's shot at the start,
grown to hold every liquid's shape; bigger cells past 2,000,000 nodes), makes the body and pours in
every Fill in one still step. Each liquid step gathers every liquid's Level changes and Pours
(`births.rs`; a pour makes width × speed × Rate ÷ (cell² / 4) drops a second), the walls band, the balls
(when some liquid's Balls push it) and the meeting rules (Layer wins when either side says so).
At most 16 Liquid things take part; the rest draw nothing. Export detail is used where
`exporting_for_now()` is held (`bpe-cli`'s video, final frame and frames exports).

**Drawing** (`kit things/liquid/draw.rs`, `bpe-gfx/src/liquid.rs`, `shaders/liquid_drops.wgsl`).
Each Liquid thing pushes a `Liquid` whose `drops` is the body's buffers: the liquid pass splats only
its own liquid's drops straight from the card (an indirect draw), into the field and a second
R16Float target of colour weights, and its surface pass reads the colour over those weights. Swirl
detail sharpens the colour weights' kernel (`k^(3 + 21 · detail)`, with a little of the plain
kernel so edges keep a colour in half floats). The Edge level is scaled by the field inside a liquid
(π · reach² for four drops a cell). Each drop reaches 1.5 cells: at 1 cell the random packing of poured drops showed as lumps and pits. Lighting the liquid with the scene's lights is part 3.

**Restarts and tests.** Every setting reaches `settings_changed`; the Look's mixing keys restart
the beside playback (`card_key_restarts_beside`) and are in `beside_print_extra`. `restarts.rs`
hashes a Liquid thing's `inputs_hash` (what its stepping reads), since that test names no card.
`golden_liquid.rs` pins the births (exact on the processor) and the drop counts per second; the
drops' places are checked on the machine only (a seek and a second run give the same), since a
graphics driver's arithmetic can differ. `full_size_look.rs` has `liquid-water` and `liquid-mixing`.

**Limits** (from the part 1 review). A body holds at most 2^20 drops (`MOST_DROPS`, 80 MiB: under
the 128 MiB a storage buffer may be bound with on every card). The kit keeps it there on its own,
so how many drops are kept never hangs on when the card's count came back: every liquid's Most
drops together are at most 800,000 (`MOST_KEPT`; each scaled down evenly past it, `share_most` sets
`LiquidState::kept`) and at most 200,000 drops are born a liquid step. The card clips births
against its own count too. Weight steps as at most 20 (the fixed-point sums clamp past it),
viscosity is clamped before mixing sums it, and the Surface pull's stiffness never passes what the
substeps keep stable. The beside fingerprint hashes beside things' tags and their order among
themselves (rules name tags; the first Liquid thing sets the area). The editor copies the body once
a picture (a snapshot shares it): dropped bodies give their step scratch (grid, walls, bins) back
to the card for the next copy (`LiquidCard::spares`), and the host keeps the walls band
(`Host::band`) so a copy puts it on again without working it out. Under Detail the Liquid's
settings say about how many drops fill 100 × 100 px.


## Part 2, as built: how it acts with balls

Plan: `docs/superpowers/plans/liquid/part2.md`.

**What the owner sees.** The Meets card's **With the balls** group: Balls push it, **Pushes balls**
(on by default), Meets balls (which balls), **Floats or sinks** (By ball weight / Floats / Sinks /
Bobs) with **Ball weight** (water's Weight is 1, so 0.5 floats half under), **Drag** (more in a
thicker liquid), **Carries balls**, **Splash size**, **Splashes from** (px/s), **Sticks to balls**,
**Wakes**, **Balls drop ink** with its **Ink colour**, and **Balls take its colour**. The Liquid's
Detail group has **Finer export**. The Rules have a When **Liquid** block (Who, Liquid, When it:
Goes in / Comes out / Splashes / Goes in or out; values speed, size, hits, ball, way).

**Where it steps.** A liquid with Balls push it and Pushes balls on is `in_main`, and its `keeps`
holds every other Liquid thing in the main world too (they share one body), so the shared liquid is
never split between worlds. Pushes balls is a plain switch (not keyframed), read from the saved
card. A When Liquid rule keeps every Liquid thing (they share one host, so naming one keeps them all). The host knows it is in the main world by
`!world.beside.contains(&id)`; only then does it record and read back. In the main world every
Liquid setting restarts the physics: Fill and Meets are hashed whole (`steps_own_cards`) and
`physics_print_extra` adds the Look's stepped mixing and the thing's place among the Liquid things.
Exporting, a main-world liquid uses the preview Detail unless the first Liquid thing's Finer
export is on.

**The feel** (`bpe-gfx liquid_sim`: `Feel`, `FEEL_WORDS` = 32 per ball, `feel_at`). Balls are
uploaded once per world step with a 16-bit mask of the liquids that meet them (6 words each); a
later liquid step of the same world step sees them moved on along their speed, and inside a liquid
step each substep moves them on too (`cs_tick` counts substeps in `counts[7]`, `ball_now`). Without
that, a ball held still for a whole liquid step was hit by the same drops every substep and felt
many times its push. In `cs_g2p`'s ball loop a contact records −m·Δv (the inelastic change, by
1 / the ball's density to that liquid) and the touching mass, colour, drag, carry and take, and one
count per liquid, all fixed-point `i32` atomics. Splash size throws hard-hit drops out harder (over
1) or lets them slide along the ball less (under 1); Sticks pulls drops within 1.5 cells in (and
records it); Wakes pull drops behind a moving ball along (not recorded); ink runs a touched drop's
colour to the Ink colour. `cs_feel` then samples 32 grid points two cells outside each ball (wet
where the volume is over 0.3; nothing for a ball no drop touched) and their mean grid speed. After
the batch, `Body::send_feel` copies the words to a map-read; the next world step's first `forces`
call waits for them (`Feel::wait`, a `OnceLock` shared by every copy of the world, so checkpoints
and snapshots read the same words).

**What the balls get** (`kit things/liquid/feel.rs::apply`). Δv = push / (π r²) plus drag towards
Carries × the liquid's speed at rate 3/s × Drag × (1 + 8 × Thickness) × how wet the ring is, over
the batch's span; held balls keep their speed. The push scale (1 / the ball's density) is capped at 10
so the fixed-point sums can't overflow, and a liquid that doesn't push balls sends no drag, carry or
take. When no ball is met any more the balls that were in a liquid get their Leave events at once
(`feel::all_left`), and taken colours of balls gone from the world are dropped. A light ball floats by the liquid's own pressure (no
buoyancy formula): in the water test scene Ball weight 0.3 floats at the top and 3.0 sinks, slower
in a thick liquid. Events: a ball is *in* a liquid once its share of the ring (wet × that liquid's
share of its contacts) passes 0.35 and *out* under 0.15 (or when no longer met); **Splash** is the
first touch at Splashes from or faster. They are worked out when the feel is taken and logged by the
host's next after-step (event kind `Liquid`, tag 23: `b` is the Liquid thing with 0 in, 1 out, 2
splash). Taken colours (`Host::taken`, rgb and amount by ball) move at 3/s × Take × wetness, and
`balls::draw::colours_with` mixes them into the ball's base colour, so trails and particles that
take the ball's colour see it too.

**Playbacks.** The editor's sound thread and its warm-up helper and the aim job name the editor's
card (the particle job already did); `bpe-cli`'s sound thread names the export's card and the
export guard. The status line's liquid readout comes from the main playback when the liquid is
there. Aim test flights (`World::lone`) don't carry the liquid.

**Tests.** `liquid_sim::tests` (a ball held under still water is pushed up and is wet all round; a
ball out of the water or not in its mask feels nothing; the feel is the same twice),
`things::liquid::tests_balls` (in the main world and keeping the others; floats and sinks; drag;
goes in and splashes; Pushes balls off hits as without; seeking; taken colour and ink; a rule
hears it), `golden_liquid.rs` (a main-world liquid's balls and log match after a seek), and
`restarts.rs` hashes main-world liquids' `inputs_hash`.

**Known gaps (part 2).** A camera or sound that follows one liquid doesn't pull it into the main
world (only Pushes balls and a When Liquid rule do). Aim test flights copy the world with its liquid
card and wait on it, so aiming in a scene with a main-world liquid is slower. The preview and an
export may use different cards (an export with Finer export off still re-steps the liquid on the
export's card, so a tiny difference in the card's float order could move balls). With a particle job
running, a main-world liquid steps twice (once in the physics, once beside it for the particles).


## Part 4, as built: stranger liquids

Plan: `docs/superpowers/plans/liquid/part4.md` (with its As built).

**What the owner sees.** The Liquid's own settings gain **Stretch** and **Tears at** (How it flows), a **Heat** group
(Temperature, Cools, Warms, Cold thickens it, Freezes with Freezes below / Melts above, Dries up, Stains, Stain
strength) and an **Un-mix** group (Remember for Un-mix, Seconds, Un-mixing; the inspector shows about how much video
memory it keeps). The Fill card gains **Fill with** (Level / A picture / A word, with Picture, Picture width, Text,
Letter height), a **Rain** group (Rain, Raindrop size, Rain speed) and a **From balls** group (Balls, Leaks, Bursts),
and its list is **Pours and forces**: Pour, **Heat**, **Drain**, **Current** (Pump / Whirlpool / Wind) and **Magnet**
(at its place or on balls, with Spikes). The Meets card gains **Changes the ball** (Off / Dissolves / Shrinks /
Grows / Takes its colour) with Change speed, **Balls heat it**, and a **With the shapes** group (Shapes paint it,
Paint colour, Painting shapes); an Other liquid rule can **React** (Makes, Reaction speed, Heat, Fizz). The Look's
Colour mixing gains **Fades** and **Fades to**. The Rules have a Do **Un-mix** block.

**Drops** are 96 bytes now: `spare` is (temperature, dryness) and `f` is the elastic stretch F after it
(`liquid_drops.wgsl` reads 24 numbers a drop). `kind` bit 9 is `FROZEN`; a stain's liquid number is 16 + its
liquid's (`STAIN_BASE`), so a liquid's own draw never shows it; `liquid_of` is `kind & 15`. Every kernel but
compaction skips stains.

**The solver** (`liquid_sim.wgsl`). Stretch adds a deviatoric neo-Hookean stress μ(b/J − ½tr(b/J)I) in p2g,
μ = min(¼·Stretch, 1) × the stiffness (never a faster wave than the substeps are chosen for); g2p moves F with C and
resets it to I when a drop is stretched past Tears at (log of the larger singular value of b/J, Tears at capped at
20) or its J goes bad. Heat (`spare[0]`) is lost at the surface (exposure from the gathered volume), gained in Heat
layers and from touching balls, and shared with neighbours at each mixing. Cold thickens viscosity by
1 + 30·k·(1−T)². Freezing sets `FROZEN` (F reset, μ at least ½ the stiffness, viscosity at the cap, snaps only past
3×); melting clears it. Drying adds Dries × exposure a second; a drop passing its own threshold (0.5..1.5 by a hash
of its serial) goes, or becomes a stain with Stains on. Forces are a 48-entry table in the step uniform
(`ForceGpu`, kinds heat, drain, pump, whirlpool, wind, magnet; `forces.rs` fills it in grid units each step from
the layers, a ball-magnet one entry per named ball); drains also remove drops in `alive`. Shapes paint it through
the walls band's 8th word, each node's wall number in `walls_now` order (first 64), and a per-step `paints` table
of masks, so keyframed paint settings work without rebuilding the band. React is pair mode 2 (target in bits
7..10, heat and fizz in `pairs2`): at each mixing a drop's reacting contact weight gives a chance against a hash of
(serial, liquid, step); a turned drop takes the target's number, its Fill colour (`LiquidGpu.m`), heat and a fizz
kick; it takes the made liquid's newest serial (`LiquidGpu.d.y`) so its Most drops keeps it, and ties between
reacting neighbours go to the lower liquid number. Fades run at each mixing. Changes the ball sums shrink × mass and dissolves × mass into feel words 12 and 13.

**Un-mix.** With Remember on, every 1/60 s `cs_record` packs every drop (20 bytes: place as u16 fractions of the
grid, colour 10:10:10 square-root coded up to 4, kind and temperature, serial, strength and dryness) into a new card
buffer. `Body::history` is a `VecDeque<Arc<Sample>>`: samples never change, so world copies share them for free
(their bytes count in `gpu_memory`); past Seconds × 60 samples, or about 256 MiB worked out from every liquid's
kept Most drops (never the card's count, so every run keeps the same number), the oldest go. While Un-mixing is on (any
Liquid thing's switch) or a rule asked (`fire_now`, `Host::unmix_left` liquid steps), each liquid step is
`Body::rewind_step` instead: `cs_rewind` writes the drops between the newest sample and the one before, sliding
those with the same serial and kind at the same index, speed and stretch reset, then pushed out of the balls that
push it; past a sample the newest is dropped, so the liquid goes on from where it was played back to. No births,
mixing or feel while playing back; a batch that played back sends no feel.

**From balls, rain, pictures** (`births.rs`). Raindrops are sunflower discs of drops born at the first free place
down from the region's top. A ball the Fill's Balls names leaks Leaks × (speed / 500, at most 4) drops a second off
its back; one seen last world step and gone now bursts into Bursts × its area of drops (`LiquidState::seen`). A Fill
of a picture reads the PNG (or the scene's kept copy, through `particles::spots::read_png*`) on a half-cell
lattice, its solid parts in their own colours; a word fills its letters in the Fill colour. All births take the
liquid's Temperature.

**Changes the ball** (`feel.rs`): radius × e^(−rate·wet·span); `Host::sizes` keeps each ball's size when first
changed: Shrinks stops at 0.3 of it, Grows at 3 times, Dissolves removes the ball under 0.2 (a Remove event in the
next after-step, `bpe_runtime::pop`). Takes its colour lifts Balls take its colour to the change speed.

**Stains** draw as a second `Liquid` push from `draw.rs` (liquid number 16 + n, edge level 0.35 so one drop shows,
no roundness or shine, opacity × Stain strength, behind the balls), only for a liquid with Dries up and Stains.

**Restarts and tests.** `inputs_hash` adds the force layers; `fades` / `fades_to` are in
`card_key_restarts_beside` and `beside_print_extra`. Tests: `liquid_sim::tests` (stretch holds a blob and tears at
its limit, ice keeps its shape, a drain and a magnet, a reaction and fading, drying and stains, record and play
back with a shared copy), `things::liquid::tests_part4` (rain, bursts, leaks, a word and a picture, a rule's
Un-mix, dissolve / shrink / grow, shapes paint it, a reaction, a seek with everything on).

**Not done (part 4).** Balls with a Heat glow sub-layer and Light things don't heat the liquid (Heat glow is worked
out only when drawn); the Air card's wind isn't read (use a Wind current). Leaks and bursts are in the Fill colour
(a ball's colour is worked out only when drawn). Reaction sounds are part 5's, a glow by temperature part 3's. One
Liquid thing can't un-mix while the others go on (they are one body). The force table holds 48; later layers are
left out. A rule's Un-mix plays back all that is remembered (no For).
