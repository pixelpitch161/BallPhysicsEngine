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


## Part 3, as built: looking real

Plan: `docs/superpowers/plans/liquid/part3.md`.

**What the owner sees.** The Liquid's **Look** card: Its colour and Colour mixing (with **Mixing
shows**, Shows as Shimmer / Glow / Foam), then the **Surface** chips (the 15 a ball wears; picking
one sets its numbers, the Particles Liquid layer's set: no Throws light or facets; a new Liquid is
**Glass**), **Shape of the surface** (Edge, Edge softness, Roundness, **Smoothing**, Opacity, In
front), **Inside** (**Clarity** 160 px, **Deep colour**, **Light soaks in**), **On its surface**
(**Ripples** and size, **Thin film** and size, **Outline** and colour, **Gives off light**), **Its own
light** (folded: Shine, Light from, Rim light, Reflects the sky, Sky top/bottom, **Bend**, Tint,
**Rainbow split**; used without a Lighting card or at Lit 0) and **With lighting** (**Lit**, **Blocks
light**), and a **Layers** list: **White water and foam**, **Bubbles**, **Things inside**, **Mist**,
**Caustics**, **Wet walls**, **Wet balls** (each at most once).

**Drawing** (`bpe-gfx/src/liquid.rs`, `shaders/liquid.wgsl::full_surface`). `LiquidStyle` grew
`h..w` (STRIDE 512); `v.w = 1` marks a Liquid thing's full look, and the Particles Liquid layer
(all of them 0) takes the old code path unchanged. The colour-weights target is now Rgba16Float:
r the colour weight, g foam (`w × foaminess`: from speed, and from the drop's `kick` for
Splashes foam), b speed / 1000, a kick. **Depth** is how far below the top a pixel is: marched up
the screen's "world up" in 24 steps (range from Clarity × 3, the caustics' reach, foam's On top,
bubble size), then halved down to a pixel between the last two steps (a straight guess between
them is off by an amount that repeats every step, which showed as stripes). **How far in from the
edge** (`inside_far`): the field's height over the level by its slope holds only near the edge
(drops resting in rows leave dips between the rows where it comes out small, which showed as
thin rainbow and highlight stripes), so further in it is the most of 3, 6, 12 and 24 px that has
the field over the level in eight directions. Foam's On top and the film's rings follow depth. Clarity: `through = exp(−depth / clarity)`, cover `1 − 0.85 × through`, colour ×
`mix(1, deep, 1 − through)`. With Bend (and Clarity, or a Glass kind) and its own light, the pass
reads the picture behind it (`LiquidStyle::reads_behind`, which the renderer uses instead of
`glass()` to copy it first) bent by the normal, split into a rainbow by Rainbow split. Ripples
(two octaves of value noise in world units) tilt the normal and add to the height written for the
real lights. Thin film is a cosine rainbow by noise, depth and time. Caustics are ridged value
noise, warped, `pow(a × b, 8)`, fading by Reach down. Bubbles are world-space cells rising at Rise
speed, popping within their size of the top. Mixing shows reads the colour 3 px either side.
Smoothing sets the drop reach (`1 + 2 × smoothing` cells; the default 0.25 is part 1's 1.5) and
the inside level `π reach²`. **Things inside** and **Mist** are drawn after the surface by
`liquid_dots.wgsl` straight from the drops (`DROP_FLOATS` = 20, the one number to change if the
`Drop` grows): a drop whose `hash(serial) < share` and whose middle is inside the surface is a
fleck (glinting by `hash(serial, floor(time × 8))`); a drop outside the surface is a mist dot.

**With a Lighting card** (Lit > 0): `style.d = [lit, stamp, blocks, link]` and `real_lit` (now with
the light it gives off and its ripples' height): picture = colour + glow, pass 1 colour × Lit,
pass 2 height (rim + ripples), pass 3 (emission) = Gives off light + caustics + soak + Mixing shows
Glow, so the liquid lights the scene. Surface index: after `surface::taken` and every Particles
Liquid layer (`particles::liquid::layers_taken`), by its place among Liquid things
(`things::liquid::draw::surface_index`). `LiquidDraw::light` puts `gpu(SHEET)` in the table, sets
`refracts` for see-through kinds, and for a liquid that blocks light with a Surface that bends
light a `Bender` box round the host's Area. Blocks light = `keeps_of(passes(s))`.

**Wet walls** (`bpe-gfx/src/liquid_sim/wet.rs`, `shaders/liquid_wet.wgsl`). Each body keeps a wet
grid (`4 × cols × rows` f32: two halves of wetness swapped each liquid step, then each node's face
distance and side; copied by `Clone`, counted in `bytes()`): after each step's last substep a node
in the walls band with mass (over 0.05) is 1, any other band node keeps `exp(−dt / 6 s)` and moves
`6 dt` of the way towards 0.97 × the node above, off the band 0. A band node knows which side of a
hollow wall the liquid is on (more mass two cells along the normal or against it, else what it
knew before; never another node's side, which is written in the same pass) and its distance from the wall's face on that side; a node past the face (under
−0.5 cells) stays dry, since the drops' weights reach across a thin wall, and `fs_wet` shows the
film only from the face to about 2 cells in (interpolated, so it follows the wall smoothly; at
first it was drawn by whole cells outside the ring, in steps). It is always worked out (one thread a node), read only by the draw
(`fs_wet`, before the splat, its own group), so it needs no fingerprint. Dries in reads it as a
floor `exp(−dries / 6)`; Streaks break it up by a column noise. `DropsSource` carries the buffer,
the grid size and the newest half.

**Wet balls** (`kit things/liquid/wet.rs`): wetness from the log alone (`wetness`: 1 after a
ball's In, `exp(−t / dries)` after its Out), so seeking and exports agree; drawn into
`particles_front` as a tint disc, an added highlight and a hanging drip. Only a liquid in the main
world (Pushes balls) logs In/Out, so only it wets balls.

**Tests.** `bpe-gfx liquid_sim::tests::the_wet_grid_marks_walls_under_the_liquid_dries_and_is_copied`,
`only_the_liquids_side_of_a_thin_wall_gets_wet`;
`bpe-kit things::liquid::tests_look` (Look defs, layer kinds, style numbers only for layers that
are on, the Particles layer's style untouched, surface index, real lighting and the table, wet
balls from a log); `bpe-kit/tests/liquid_look_pictures.rs` (clear water near its top is nearer
what is behind than solid water and shows less of it deep down, glow brightens, foam whitens, the real lights
change it, every layer draws). Test scenes `scenes::liquid_look_scenes` ("Liquid looks", "Liquid
lit pool", "Liquid glow"); look shots `liquid-looks`, `liquid-lit`, `liquid-glow`.

**Known gaps (part 3).** No Shader layers (picture effects) on the Liquid's Look: it is drawn by its
own pass. Foam doesn't last after a drop slows (that needs per-drop memory). Wet balls need the
liquid to push balls. The wet grid is the body's, shared by every Liquid thing: Wet walls on one
liquid shows where any liquid has been, in its own colour. The depth march looks straight up the screen, so a liquid under an overhang
reads as shallow there. Caustics and bubbles are patterns in the world, not traced from the
surface or moved by the flow.

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
compaction skips stains, and part 5's stats (`liquid_stats.wgsl`, which reads the same 96-byte drop) leave them out.

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
reacting neighbours go to the lower liquid number. Fades run at each mixing. Changes the ball sums shrink × mass and dissolves × mass into feel words 14 and 15 (12 is part 5's depth).

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
no roundness, shine, ripples, film, glow or outline, no Look layers, never lit, opacity × Stain strength, behind the balls), only for a liquid with Dries up and Stains.

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

## Part 5, as built: Sound (2026-11-17)

**Sound card on a Liquid thing.** `liquid::slots()` includes Sound, and the inspector's Sound tab
(`bpe_kit::sound::makes_sound`, `hears_liquid`) shows on Liquid things. Enable sound gives the
**Splash** starting sound (`bpe_kit::sound::presets::liquid_sound`); the Moments panel
(`sound_tab/moments.rs::show_liquid`) has switches for *A ball splashes in* (`moments_splash`,
on), *goes in*, *comes out*, *It pours* and *Balls bubble under it*, and a **Start from** row of
the six starting sounds (Splash, Plop, Drip, Glug, Pour, Bubbles; `TabEdit::StartFrom`, one undo
step). Each is an ordinary Sound card built on the starter voice (noise or sine/triangle, plucked
envelopes, a Filter chain layer id 6 and a Hit modulator id 7 where needed). The voice hears its
liquid's events (`LiquidMoments` in `hit_filter.rs`): Splash/Enter/Leave name the liquid as `b`,
pours and bubbles come only from its own liquid.

**New events.** Item 3 **POURING** (`sound.rs`): each pour that gave birth on a step logs at most
30 a second (`POUR_RATE`), at the pour's place with its speed. Item 4 **BUBBLE**: a ball inside a
liquid (`feel::apply`) gives off bubbles at 8 a second per 100 px/s (at most 60), rolled on the
shared dice by step, ball and liquid (chance = rate × the seconds the feel covered). Pours are
pushed to `host.news` by the liquid steps and logged at the end of the same `after_step`, so the log
stays in step order. Both are logged only when the liquid's own Sound card asks
(`LiquidSound` in `LiquidState`, in `inputs_hash` and `physics_print_extra`), so goldens don't
move.

**Staying with the physics.** `ThingSim::kept_by_scene` (new hook, read by `is_beside`) keeps
every Liquid thing in the main world when `scene_hears_liquid`: a Liquid thing's sound is on, or any
sound has Muffled under liquid above 0, Pitch from Depth or Colour, or an enabled Liquid modulator.
A kept liquid that doesn't push balls still sends no drag, so the balls move as without it (test).

**Readings from the card.** `cs_feel` also walks up against gravity from just above each ball's top
while cells hold > 0.3 of a drop, up to 255 cells, and stores depth×16 at feel word 12
(`feel_at::DEPTH`). A new pass `liquid_stats.wgsl` sums per liquid (12 words: drops, speed×4 up to 4095,
row, colour 0..63 and its square per channel), read back with the feel (`Feel::stats`). The
feel is also sent after a main batch with no balls when the scene hears a liquid (`Host::heard`,
read from `World::tree`; the Liquid thing now `reads_tree`). The host keeps `wet` (by ball:
depth px, contact colour, the top four liquids' shares) and `sums`. `wet_of(world, ball)` and
`stats_of(world, pick)` (drops, mean speed, height 0..1, colour, mixed 0..1 from the largest
channel's spread, amount = drops / kept) read them without allocating.

**Sound side** (also in `docs/notes/sound.md`): Pitch from (Off/Speed/Depth/Colour) with Pitch
range, Deepest at, Muffled under liquid and Muffled to in the Playing panel; the Liquid modulator
(Reads Slosh / Mix amount / Level / Amount, which liquid, Full at, Smooth, Invert).

**Tests.** `liquid_sim::tests` (`a_ball_deeper_in_the_water_has_more_above_it`,
`the_stats_count_every_drop_and_their_colour`), `things::liquid::tests_sound` (hearing only when
asked; a sounding non-pushing liquid is in the main world, logs splashes and leaves the ball alone;
pours ~30/s only for a Pour sound; a sinking ball is wet, bubbles, and the stats read right; paint
reads less mixed; seek), `sound::presets` (each plays one moment), `bpe-sound/tests/liquid_sound.rs`
(splash plays and is silent without; muffled is duller; colour moves the note; the modulator hears
Amount), `alloc.rs` (a liquid scene's blocks don't allocate), `voices::liquid` unit tests, and the
studio's `a_liquid_starts_from_its_splash_and_from_any_other_sound`.

**Known gaps (part 5).** Follow sound reads the Liquid modulator unsmoothed. Depth is measured
straight up against gravity from the ball's top in cells, so a ball under an overhang of liquid
reads the liquid above it only. Colour chords need two liquids each at least 20 % of a ball's
contacts. Pitch from Depth/Colour and the muffle read the sound world as the note starts (up to a
block past the hit), as Pitch follows size does. Height reads wrong past 4094 rows.

## Fix: exports and Preview final frame with a liquid (2026-11-17)

In the editor, exporting a scene with a Liquid thing (and Preview final frame) failed with a graphics card
validation error, while `bpe-render-next` worked. The export draws on its own `headless_device()`, made from a new
wgpu instance, while the editor's window card stays named on the main thread. `wgpu::Device`'s `==` compares only
the device id, and every instance numbers its devices from the same start, so `SwarmGpu::for_device` and
`LiquidCard::for_device` (which looked up a registry of cards by device) handed the export the editor's card: the
liquid stepped on the editor's device and was drawn with the export's. Huge swarms had the same flaw.

As built: there is no registry. `SwarmGpu::for_device` and `LiquidCard::for_device` always make a new card (the swarm pipelines are made on first use); the
caller keeps the card and names it with `swarm::use_for_now`. A `SwarmGpu` owns its `LiquidCard`
(`SwarmGpu::liquid()`, made on first use), and `LiquidCard::on_this_thread()` takes the named card's. Never match
GPU objects of different wgpu instances with `==` (the renderer's `DropsSource.device == *device` check has the same
blind spot, harmless today because each world draws with the device it stepped on). Regression test:
`bpe-cli/tests/liquid_export.rs`, which keeps an "editor" card named while it draws a final frame on a second device.


## Water feel, as built (liquid fixes part A)

Plan: `docs/superpowers/plans/liquid/water-feel.md` (from the 2026-11-17 liquid review).

**What the owner sees.** A Liquid's How it flows group starts with **Size** (Glass, **Bucket**, Bath,
Pool): how big the liquid is in real life (the first Liquid thing's is used for all, as Detail, kept in
`Host::size`: liquids of different sizes sharing a body would fall at different gravities). A smaller liquid falls at more times the scene's gravity
(Glass 16, Bucket 8, Bath 3, Pool 1; Pool is how liquids moved before), so a ring of water sloshes in
about a second instead of six. New liquids are thinner and settle less (Thickness 0.02, Surface pull
0.2, Spray 0.4, Settles 0.1). Single drops and thin streams now draw (spray, drips, a Very fine pour).

**How.** Gravity × `Host::size` is each liquid's gravity factor (`lq.b.z`) and goes into its
stiffness. The push on balls (`lq.e.x`) is divided by the Size, so a ball still floats
or sinks by its Ball weight. A pour's speed (and its births a second) is × √Size, so its arc keeps its
shape on screen. The stiffness's column (`sim.rs::column`) is the liquid's shape's size (its bounds'
longest side) instead of the whole grid, which keeps Bucket's substeps under `MOST_SUBSTEPS` (64): at
the cap a Springiness 1 Bucket pour blew apart, so at the cap the stiffness is held at 0.8 of what the
substeps keep stable (a Glass, Springiness 1 pour then stays whole).

**Holes close** (`cs_g2p`): a drop with liquid nearly all round it (`exposure` under `CLOSE_OPEN`
0.5) moves `CLOSE_RATE` × the substep (120 cells a second at a slope of 1, a substep at most half its
share) down the slope of
the grid's volume (`slopes`, the weights' derivatives). Cracks behind a pour are thinner than a cell,
so they have no pressure of their own to close them. Tried first and dropped: giving inner drops a
pull back for J > 1 (the full equation of state). Even at 0.15 of the stiffness it threw the whole
liquid apart (tensile instability).

**Lone drops** (`liquid_drops.wgsl`): a drop whose volume round it (1 / `j`, float 12) is under
0.45 has its splat lifted (full at 0.25) so its peak is 1.6 × the Edge level and it draws as a small
droplet. Field and colour weights are lifted together, so colours don't change.

**Known gaps.** The push on balls is divided by the Size as a whole, so a ball crashing into Glass
water also feels 1/16 of the impact (drag still slows it): buoyancy and impact can't be told apart
in a drop's contact. Liquid that spills out of its shape into a taller free area gets less stiffness
than it needs there. Pours fill √Size times faster than before.

**Tried and dropped: shaking stretched surfaces into spray.** It only roughened the surface. A ball's
splash crown is a few cells thick, so shaking it doesn't break it into drops.

## Lighting and look, as built (liquid fixes part B)

Plan: `docs/superpowers/plans/liquid/lighting-look.md` (from the 2026-11-17 liquid review).

**What the owner sees.** Lots of liquid under a lamp no longer has dark shafts through it, bright
columns under splashes or a ladder of rings under holes; ripples sit on the top (and break up the
waterline's highlight) instead of speckling the whole body; the liquid fills right up to a wall
instead of stopping short of it with a rounded corner. No setting changed.

**No self shadow** (`shadows.wgsl::march`). `LiquidDraw::light` pushes each Liquid thing that blocks
light into `LightScene::liquid_surfaces`; the lighting uniform's `bent.z..bent.w` is their surface
index range (consecutive). A ray starting on one (`liquid_at`) leaves it as a solid blocker (see-
through too), and when it meets the same liquid again it walks out of it by the field
(`walk_out`) and carries on. It still shadows everything else and everything else still shadows
it. Known gap: as with the old leave loop, a ball inside the liquid between a pixel and where the
ray leaves the liquid doesn't shadow that pixel.

**Depth** (`liquid.wgsl`): for a full look, after the splat, `fs_coarse` takes the field's mean over
13 × 13 px round each coarse texel (a quarter of the field each way, R16Float) and `fs_depth` marches
that smooth field up the screen's world up a texel a step (at most 256, to `depth_range()`),
crossing the level between samples. `full_surface` samples that depth map, and within 24 px of the
top blends (`smoothstep(12 px, 24 px)`) to the old exact march limited to 24 px (foam's On top,
film rings, bubbles need the top to the pixel). Thin gaps, spikes and holes under ~8 px, and rows
of resting drops, don't reach the depth at all.

**Ripples** (`ripple_noise`, `ripple_height`): faded out with depth (`1 − smoothstep(0.5 s, 2 s)`, s
the ripple size), no longer phase-shifted by the flow (that slid a stream's ripples as a band; flow
now adds up to 50% strength), and they move the waterline (`fs_surface`, where the edge faces up)
by their height. For the real lights the height is up to 5× steeper within a ripple size of the top,
so the top catches small glints.

**Walls** (`liquid_drops.wgsl`): the drops group also binds the body's wet grid (its face distances
are always kept). A drop whose bilinear face distance is −0.25 to its reach (all four nodes in the
band, side known) gets its wall's plane (normal from the face distance's slope, accepted at length
0.5 to 1.5); a second draw (`vs_mirror`) splats its mirror image across that plane, and both are cut
at the plane, softly over a pixel (`fs_drop`). The field is whole up to the face and nothing shows
past it. Spilled drops past a face and walls whose side isn't known yet draw as before.

**Tests.** `liquid_look_pictures.rs`: `deep_water_is_even_across_its_body_and_meets_the_wall`,
`the_liquid_never_shadows_its_own_body`. Look shot `liquid-deep-lit` (clear water at Level 0.7,
a lamp inside the ring, five balls).

**Seen but not this part's.** In `liquid-deep-lit` at 2.5 s the balls' soft shadows in the air
near the lamp show faint contour bands (Good quality); air pixels never take the liquid branch, so
it's the lighting's soft shadows, not the liquid.

## Spray and bouncing fixed, as built (2026-11-21)

Plan: `docs/superpowers/plans/liquid/spray-and-bounce.md` (what was measured, and what was tried and dropped).
The owner's scene: a pour onto a plate in a box threw drops at up to 18,000 px/s (the pour arrives at about
3,000), and still water in a box simmered and after 3 s burst upwards by itself. More steps a second was
measured and does not help. Three causes, three changes, all in `liquid_sim.wgsl` (no setting changed):

**Walls count as full liquid** (`weight_below`, `wall_share`, `edge_share`). A drop's squeeze J is 1 / the
liquid the grid holds round it. Beside a wall half of that was missing, so the drop took itself for the open
surface and pushed back at nothing, and the liquid packed against every wall and floor until three times too
many drops sat there (12 to 14 a square cell against 4), then burst when that layer was disturbed. Now each
node adds the share of its weight that lies where no drop can be: inside the wall and the margin drops are kept
out of (from `0.5 − WALL_GAP` cells off the face; for a band only as far as the margin on its other side; for
the picture's edges the same). It is a flat-wall estimate from the node's nearest face: a little low in an
inside corner, a little high round an outside one. `cs_g2p` gathers it into `vd` and the `slope`; `cs_p2g`
gathers it again at the drop's place for the pressure (so `cs_p2g` now binds the walls, binding 5).
**`p.j` is still the liquid alone**: the picture draws a drop with little liquid round it as a droplet, and
a stream down a wall must keep drawing. Cooling, wind and drying (`open`) also still go by the liquid alone, as
before (so the row on a pool's floor still counts as half open to them: older than this fix, left alone). The
grid's volume channel is left as real liquid too (the balls' wet and depth readings use it). A drop not
measured yet has `p.j` 0 (`Drop::new`, `cs_rewind`; it was 1) and is taken as at rest, so the walls' share
doesn't squeeze a newborn drop for a substep. A wall at the picture's edge and the edge join as
1 − (1 − wall)(1 − edge) (`solid_share`).

**Squeezing is damped** (`cs_p2g`, `BULK` 0.7): a bulk viscosity of 0.7 × the speed of sound × one cell on
tr(C), while `j < 1 + tear`. The pressure was a spring with nothing to calm it. Flow that keeps its volume
feels nothing, a slosh almost nothing; a cell-sized pile-up is calmed in about one step of its own springing.
ζ × the substep is at most 0.105 by the substeps' own rule (`CFL`), so it needs no cap. Past the tear it is off:
there it smeared a thin pour into a fan.

**"Holes close" only closes holes** (`cs_g2p`): it moves a drop only when there is liquid `CLOSE_ACROSS`
(2.5 cells) down the slope (so not at the open surface, which it used to pump), never into a wall, and not
at all within 0.6 cells of a wall's face (full from 1.6). Before, drops it moved into a wall were put back on
the wall's line and ended on exactly the same spot as each other, one drop from then on: 973 of 7,240 drops
in the owner's scene.

**Tests** (`things/liquid/tests.rs`): still water in a closed box comes to rest, throws nothing, has no stacked
drops and no packed layer on the floor; a pour onto a shape never has a drop faster than a fall gives it.
Their scenes are `scenes::liquid_box` and `scenes::liquid_pour_on_a_shape`; the second is also the look
check's `liquid-pour-on-shape`.

**Known gaps.** Balls are not counted as full as walls are. Two drops can still end on one spot where two
flows meet along a wall, or where Surface pull draws the top rows of perfectly still liquid together (about
0.2 % of drops): they stay one drop, which is harmless now nothing springs.

## The same water at every Detail, as built (2026-11-21)

Plan: `docs/superpowers/plans/liquid/same-water-every-detail.md` (what was measured, what was tried and
dropped, and parts 2 (speed) and 3 (look), which are not built yet). The owner's complaint: at Normal the
liquid was "viscous, like sludge", at Extreme it was water. Six changes, all in `liquid_sim.wgsl` plus one
new number on each drop. No setting changed.

**The grid's sums round to nearest** (`fixed()`). `i32(x * scale)` rounds toward zero, so every one of a
drop's 36 adds a substep lost up to a unit in the direction that slows it: a dry friction (the same bite
however slowly the liquid moves; 8 % of the liquid's gravity at Normal, Bucket, and a lone drop under gentle
gravity hung in the air), stronger the more substeps, so every Detail level had its own. The mass sum was
biased low too, which fed fast liquid (+11 % a second at Normal, +26 % at Extreme). Any new fixed-point sum
must use `fixed()` (the mixing sums' `q()` rounds the same way now).

**Each drop carries its squeeze** (`Drop.more[0]`, J − 1: 0 at rest, below 0 squeezed, above 0 stretched; the
drop is 112 bytes now, `DROP_FLOATS` 28). The pressure (`cs_p2g`) goes by it and no longer by `p.j`. `cs_g2p`
moves it with the flow (`e += (1 + e) dt tr(C)`) and holds it loosely to what the grid says is round the drop:
`jref = 1 / (vd / full)`, where `full` (from how steeply the liquid falls away, `|slope| / vd`) is the share of
a drop's reach inside the liquid at a flat open edge, so the surface, a thin sheet and the inside all read
about 1. Within `CARRY_SLACK` (15 %) of `jref` nothing happens; a drop that thinks itself less packed than
the grid says is moved to that edge at `CARRY_DOWN` 30 a second (over-packing is what bursts), one that thinks
itself more packed at `CARRY_UP` 4 a second (the normal state near an edge the estimate gets wrong: answered
fast, corners simmered). Stretched to `CARRY_LOOSE` 0.3 (or its liquid's tear, if that is more) a drop is
loose: it carries nothing and takes `jref` again as soon as that is under it, kept in [-0.9, loose]. That is
well past the slack on purpose: Spray takes a liquid's tear down to a hundredth, and at that drops inside
sound liquid came loose and joined again all the time, each time taking the grid's noisy reading. Between its
tear and loose a drop has no pressure and no pull, as before. Drops are born at 0 (`Drop::new`; Un-mix's
`cs_rewind` too).
Why: read straight off the grid, the pressure heard the grid's sampling noise, which depends on where each
wall's face falls between nodes (the still test box simmered at 5,000 px²/s² at 594 px wide and rested at
600; Detail moves the grid, so each Detail had its own calm and restless scenes), and the top two cells of any
liquid, or a whole sheet thinner than four cells, carried no pressure at all (at Normal that is a 30 px
sheet). `p.j` is unchanged (1 / the liquid alone): the picture still draws lone drops by it. `cs_p2g` no
longer gathers the walls' share (it took back the 8 % a step that cost); `cs_g2p` still does, for `jref`
and "holes close".

**Nodes across a thin wall move along it** (`cs_grid`, `GHOST_LOOK`, `GHOST_MORE`). A drop beside a hollow
shape's wall spreads onto nodes on the wall's far side. Those were ordinary wall nodes (speed into the wall
removed, speed away kept), and "away" on their side is toward nothing, which they fed back to the drop. Now
such a node loses all of its speed along the normal and keeps its speed along the wall. Which nodes: band
nodes near enough the wall's line to hold weight from across it (`|s.z| < 1.5 (|n.x| + |n.y|) - 0.5 - t.z`)
whose other side, read 1.5 cells off the wall's face straight out from the node's nearest point on the line,
holds more than 4 times the liquid of its own. Filled shapes are unchanged. (As first built it probed two
cells either side of the node itself and also fired on nodes 2 to 3 cells inside the liquid: see the plan's
"As built". Never calm liquid by taking speed off nodes inside it.)

**A wall pushes back as the liquid would** (`cs_p2g`, after a drop's own nine nodes). The pressure force on
a node is a sum over the drops round it, and within two cells of a wall that sum is short of drops on the
wall's side (drops stop half a cell off the face) while only nodes within 0.75 of a cell of the face are held.
So the liquid packed against every wall until that layer was squeezed enough to hold the rest (half as many
drops again in the half cell by the wall), and a layer squeezed more than its depth asks is lighter than the
liquid beside it: it rose up every side wall at 140 to 190 px/s, for good. Now each drop within a cell and a
half of a wall's mirror plane (`WALL_GAP`, a quarter cell, short of where drops stop: drops at rest and their
mirrors are evenly spaced) scatters once more from its mirrored place: only to nodes on the liquid's side of
the plane, only the push along the wall's normal, with its own pressure plus what the depth between it and
its mirror adds (so a floor carries the liquid's weight). It must be the drops' own sum again: a smooth
stand-in for the wall was 1 to 3 % out, and at the bottom of a pool thirty cells deep a hundredth of the
pressure is a third of gravity. Only the nearest wall is mirrored; balls and the picture's edges are not (the
edges need none: drops there stop on the very nodes that stop them).

**A drop a wall stops is squeezed by it** (`cs_g2p`, `WALL_SQUEEZE` 2). The grid's flow carries drops into a
wall's margin and they are put back on the line; the squeeze they carry goes by the grid's flow and knew
nothing of that, so drops collected by walls unsqueezed until the safety net noticed and stirred the pool (36
px walls: 2,300 px²/s² and rising). A drop moving into the wall when it is put back is squeezed by twice how
far it was put back (its own room is half a cell), never by more than twice what it moves into the wall in
that substep (so a drop two walls pass between them in a corner is not squeezed for that).

**"Holes close" only where liquid is missing** (`cs_g2p`, `HOLE_BELOW` 0.95): it acts fully where the liquid
round the drop reads under 0.8 of full and not at all above 0.95. The pressure used to iron the density flat
to a hundredth, so in sound liquid its slope was nothing; with the carried squeeze the density sits within a
tenth of full by how the drops lie, and "holes close" stirred that for good (6,000 px²/s² and rising in the
600 px box). Turned off altogether, the pool under a pour is full of holes.

**Measured** (release, software Vulkan, Normal). A slosh in the test box: each half swing keeps 0.89, 0.86,
0.96, 0.94, 0.93 of the last (it kept 0.66, 0.59, 0.46, 0.15 and stopped); the period is 1.55 s (theory 1.46).
Still water at 5 s: 30 to 106 px²/s² (5 to 10 px/s rms) at widths 594, 600 and 604 and walls 3, 36 and 40 px
thick, fastest drop 14 to 44 px/s, at most 16 of 9,768 drops stacked; shallow water, a liquid filling the
picture and a hollow box standing in a pool rest at under 10. The owner's scene, mean speed² a drop at 6 s:
Normal 533k px²/s² (it was 436k). At Normal the pool now covers the whole floor by 6 s, sloshing and rolling
back off each wall, where it stood in a stepped heap under the pour.

**Tests**: `a_lone_drop_falls_as_fast_as_gravity_says` (bpe-gfx: it fell 0.05 cells of 30),
`a_slosh_in_a_box_keeps_swinging` and `still_water_rests_wherever_the_walls_sit_on_the_grid` (bpe-kit).

**What Detail still changes, and always will**: how thin a sheet the liquid can make. Under the owner's pour
a layer about 15 px thick runs along the floor to the walls and rolls back over; Extreme has five cells
across it and Normal two, and a grid smears anything under about three cells into the liquid above. So Normal
shows the same flow, slower. Very fine and Extreme also hit the 64-substep cap, which caps their stiffness.

**Known gaps.** A Fill starts with a jolt (30 to 45 px/s rms for the first second): its lattice does not
know where the walls' margins are, so the first rows by a wall start up to half a cell off them. Balls are
not mirrored as walls are, so liquid still packs a little round a sunk ball. A corner mirrors only its nearer
wall. Bigger time steps (`CFL` 0.2: a quarter fewer substeps) were measured and left alone:
still water simmers at about twice the energy and creeps up. Speed and look: parts 2 and 3 of the plan.


## The look at walls, thin streams, pinholes and the pour, as built (2026-11-22)

Plan: `docs/superpowers/plans/liquid/look-walls-and-streams.md` (what was wrong at full size, the decisions,
and an "As built" list of what changed while building). Part 3 of the owner's liquid review; only the
picture and the Pour's births changed, nothing in the solver. One new setting: Look, **Streaks**.

**The picture's walls come from the wet grid, with no guessed side.** The wet grid is 6 numbers a node now
(`liquid_wet.wgsl`, `wet.rs::WET_FLOATS`): after the two halves of wetness, `face` and `side` (still the wet
film's) come `line`, the node's signed distance from its wall's line exactly as the walls band has it, and
`band` (0 off the band, 1 a filled wall, 2 + its half thickness a hollow one). `wall_at(q)` (the same
function in `liquid_drops.wgsl` and `liquid.wgsl`) reads them between a point's four nodes and is only valid
when all four are in the band, all filled or all hollow, and one wall's (`line` steps by at most 1 from a
node to the next along one wall; each node keeps its nearer wall, and between two walls the blend would read
as a face in the middle of the liquid); the face distance is `line` for a filled wall and `|line| − half`
for a hollow one, on the point's own side. Do not use `face`/`side` for anything but the
film: `side` is a coin toss two or three cells inside the liquid, which is what notched every curved wall.

**Drops are mirrored a quarter cell off the face** (`liquid_drops.wgsl`, `MIRROR_OFF` 0.25, the plane the
physics mirrors them in, `WALL_GAP`). Drops stop half a cell off a face and rest half a cell apart, so
mirrored in the face itself a row was missing and the field at the wall was only just over the Edge level: a
pale scalloped edge along every wall. The wall's normal is the slope of `line` between the drop's own four
nodes (no neighbours needed), so drops up to `reach + 0.5` cells off the face find it. A drop and its mirror
are splatted up to `SPILL` 3 px past the face, not cut at it (through a hollow wall thinner than that, only
its thickness less half a pixel, `DropOut.spill`: past its far face the surface pass could not tell the
spill from liquid there), and the surface pass makes the cut
(`liquid.wgsl::wall_cover`: whole half a pixel outside the face, gone a pixel inside it). The field has no
edge of its own at a wall, so there is no rim, highlight or lit bevel there.

**A gap under a body of liquid at a wall closes** (`liquid.wgsl::wall_lift`, `whole_near`, in `fs_surface`).
Drops rest up to a cell or so off a wall, unevenly, and sloshing liquid lifts off the floor in pockets. For a
pixel within `LIFT` 1.5 cells of a face, the liquid is looked at `LIFT` cells off the face straight out
(twice over in a corner). By as much as there is a body there, the pixel is covered and flat, with that
liquid's colour if it has none of its own and its depth read from there. A body: the coarse field `LIFT_FAR`
5 px further out is 0.5 to 0.75 of full, and the field is over the Edge level 1, 2 and 3 cells further out
(`LIFT_DEEP`). The smooth coarse field decides how much on purpose: read off the field alone, every hole and
edge near a wall was extruded into a hard-edged block. The three probes are there for streams: one a cell or
two wide running down a side wall is not a body, and filling between it and the wall drew frayed blocks.

**The picture behind is never read from inside a wall** (`liquid.wgsl::wall_bend`): the bent step stops
`BEND_OFF` 0.4 cells short of a face. Glass (and any clear look with Bend) read the white wall through every
rim near it: pale patches along the floor.

**Thin liquid is drawn as streaks** (`liquid_drops.wgsl::drop_out`). By the liquid round a drop (`vd = 1 / j`):
under `THIN_END` 0.65 of full (wholly under `THIN` 0.35) its splat is stretched along its velocity by the
distance it goes in Streaks / 120 s over a droplet's width (at most `MOST_STRETCH` 6 times) and narrowed by
that to the power −0.4. Liquid at rest and the inside of a body are round as ever (a body's own surface
drops are half thin, so a surface that moves smears a little). The lone-drop boost has no blind band now:
under `SPARSE` 0.38 of full the splat is lifted to `SPARSE_TO` 0.9 of full, fading out by `SPARSE_END` 0.54
(the Edge level is half of full and a flat surface's own drops read about half, where an eighth of the lift
is left: every free surface sits a fraction of a pixel further out than it did). **Streaks** (`LookSettings::streaks`, default 1, 0 draws round drops) rides
in the style's `c.w`; it only draws, so it restarts nothing, and the stains' flat style sets it to 0.

**Pinholes close** (`liquid.wgsl::pinhole_fill`, full looks only): where the coarse field (the 13 × 13 px
mean) is nearly whole (`hi = clamp(1 − 0.003 × cell_px², 0.8, 0.97)`, from `hi − 0.12`), the pixel is
covered and flat. Voids smaller than a cell, which the physics cannot see, drew as dark dots with a bright
rim. A straight free edge reads a half and a hole a cell across reads under it: neither is touched.

**A Pour under Rate 1 is a narrower, solid stream** (`births::pour_width`: Width × min(Rate, 1)). As many
drops as before, born at resting density. Over the whole Width it was a curtain of spray three times too
wide for the water in it, drawn only by the lone-drop boost. Rate 1 and above are unchanged.

**Wiring** (`liquid.rs`): the surface pass's group has the wet grid (binding 5) and the coarse field
(binding 7); each Liquid thing gets its own surface group with its body's wet grid (`Placed.surface`), the
Particles Liquid layer's shared one binds a small empty buffer (its `st.v.x` is 0, so it is never read).

**Looking at it**: `full_size_look.rs` takes `LOOK_FILE=<a saved scene file>` with `LOOK_ONLY=file` and
`LOOK_TIMES=2,4,6` (seconds), which is how the owner's own scene was checked at 1:1.

**Tests**: `the_wet_grid_keeps_each_wall_nodes_line_and_band` (bpe-gfx),
`a_pour_under_rate_1_is_a_narrower_stream_not_a_curtain_of_spray` (bpe-kit unit), and in
`liquid_look_pictures.rs`
`the_liquid_meets_the_ring_all_the_way_round_with_no_pale_line_or_notch` (fails with the old shaders),
`a_thin_falling_stream_draws_whole_not_as_beads` and
`streaks_smear_a_falling_stream_and_leave_still_water_alone`.

**Left** (the solver's, which the speed part is changing): balls are not mirrored, so liquid packs a little
round a sunk ball; liquid climbs a ring's wall in a hard pour; a Fill's first-second jolt. In the picture:
under a hole in the body the depth colour starts again (a paler column), and the wet film is brighter than a
lit body beside it.
