# Faster liquid: fewer, lighter passes for the same drops

Part 2 ("speed") of the liquid review of 2026-11-21 (`same-water-every-detail.md`, which lists the ideas this plan
starts from). The owner's words: "the playback is extremely slow and rendering takes a very long time even with my
PC specs. I understand that this type of simulation is very taxing and likely won't ever run at real-time preview on
Extreme, but I would guess there's at least some small performance improvements that can be made."

**The rule for this part: the liquid must not behave differently.** Every change here leaves every drop exactly
where it was, bit for bit, on the same machine and driver. That is checked, not argued: `liquid_speed` (below)
prints a hash of every number of every drop, and the prints before and after must be the same. A change that can't
meet that (bigger time steps, half-rate preview) is not in this part.

## Where the time goes

All in `crates/bpe-gfx/src/liquid_sim/mod.rs` (`Body::step`) and `crates/bpe-gfx/src/shaders/liquid_sim.wgsl`. A
liquid takes 240 liquid steps a second, each split into substeps (38 at Normal to the cap of 64 at Extreme in the
owner's scene), so a second of video is up to 15,360 substeps. Before this part a substep was five passes on the
card:

| Pass | Runs over | What it did |
| --- | --- | --- |
| `cs_clear_grid` | every node | zeroed the grid's sums |
| `cs_p2g` | every drop | read the drop (112 bytes), added mass, momentum, volume to 9 nodes (36 atomic adds) |
| `cs_grid` | every node | sums to speeds, held off the walls |
| `cs_g2p` | every drop | read 9 nodes (three atomic loads each) and each node's walls' share (two more reads and two splines, six for a liquid that fills the scene), moved the drop, wrote all 112 bytes back |
| `cs_tick` | one thread | counted the substep |

Two things follow. **The node passes cost the same however little liquid there is**: at Extreme the grid over the
picture is 365 x 645 = 235,000 nodes, swept twice a substep, while a pour's first seconds hold a few thousand
drops. **A drop is read twice and written once a substep**, in two passes, and most of what the gather works out for
it is thrown away (the walls' share of nodes nowhere near a wall, "holes close" for a drop in sound liquid).

Measured here on the software card (lavapipe, 4 cores, release; see "Numbers" under "As built" for the table):
a pour with 1,900 drops at Normal took 6.3 s for a second of liquid, a box with 19,800 drops 23.4 s. So about
5 s of every second is the grid and the passes themselves, whatever the drops. A real card is a different machine
(passes cost more against the work in them, memory traffic matters more, arithmetic less), and nothing here could
be timed on one: see "Timing on the owner's card".

## What changes

### 1. Two passes a substep (the big one)

A substep becomes **grid pass, then one pass over the drops**:

- There are two buffers of sums (`Scratch.grids`), used in turn, and a new buffer `nodes` (binding 16): four floats
  a node, speed x and y, volume, walls' share.
- **`cs_grid`** reads the sums the drops just added (binding 4), writes the node's speeds, volume and walls' share
  to `nodes`, and **clears the other sums** (binding 17, `next_sums`) for the scatter that follows. So no pass does
  only the clearing.
- **`cs_g2p2g`** (new) gathers from `nodes`, moves the drop, writes it back, and scatters it straight into the
  just-cleared sums for the next substep. This is the "G2P2G" transfer of Wang et al. 2020 (*A massively parallel
  and scalable multi-GPU material point method*): a drop is read once and written once a substep.
- A liquid step's first scatter is still `cs_p2g` (births, mixing and packing changed the drops since the last
  gather) and its last gather `cs_g2p` (so the mixing and packing see gathered drops and the grid's last sums stay
  for the wet pass).
- A liquid step of S substeps is `1 + 2 S` passes where it was `5 S`.

How it stays exact: `scatter(p)` and `gather(p)` are the old `cs_p2g` and `cs_g2p` bodies as functions, unchanged;
the combined pass calls one after the other on the same numbers the two passes passed through the drop buffer. The
sums are integers, so which pass adds them makes no difference. `Scratch.clean` says which sums buffer is all zero
(the next liquid step's first scatter's); inside a step the two swap with every scatter (`a` in `Body::step`), and
`clean` is set again as the step ends. It travels with the scratch when a dropped body's scratch is reused
(`LiquidCard::spares`).

- The wet pass (`wet.rs`) reads masses from the sums of the last substep: `scratch.grids[a]` after the loop.
- `cs_feel` reads `nodes` (speeds, volume) where it read the grid.
- Memory: a node takes 80 bytes of scratch where it took 48 (18.8 MB at Extreme's 235,000 nodes, 160 MB at the
  2,000,000 cap, where it was 96 MB). Scratch is not part of a checkpoint (`Body::bytes` is the drops and the wet
  grid), so playback's checkpoints are as before; the card's up to four spare scratches (`MOST_SPARES`) grow by
  the same share.

### 2. The gather reads one thing a node

`nodes[k]` holds what the gather needs of node k in one 16-byte read: `(v.x, v.y, volume, walls' share)`. Before,
it was three atomic loads from the grid plus `solid_share` per node per drop: one or two reads of `solid` and two
splines, and four more splines for a drop of a liquid that fills the scene (`STOPS`), every substep.

- The walls' share of a node is worked out once a substep per node in `cs_grid` (`wall_share(k)`, the same
  function), not nine times per drop.
- The edges' share (`edge_share`) is exactly 0 for a node more than 3.3 cells in from every edge
  (`weight_below` is 0 at -1.5 and below). The gather works out `by_edge` once per drop (its base node within 4 of
  an edge) and only then calls `edge_share`; otherwise it uses 0.0 in the same expression
  (`1 - (1 - wall) * (1 - 0)`), which gives the same bits.

### 3. "Holes close" only where it can move the drop

The block worked out a look across the gap (four node reads 2.5 cells away), the four nearest wall nodes and a
projection for every drop every substep, then multiplied the move by `inner * hole`, which is 0 for every drop in
sound liquid (`hole` is 0 at `vd >= HOLE_BELOW`) and for every drop at the open surface (`inner`). Now the block
runs only when `inner > 0 && hole > 0`. With either 0 the old code added a zero shift to `p.x`. (The one case
where adding zero is not nothing is a coordinate of exactly -0.0, which becomes +0.0. A drop does not get
there: a sum that comes to nothing is +0.0, and a drop that far out is outside the Area and removed.)

### 4. A drop's numbers that a gather can't change are not written back

`keep(i, g)` writes x, v, c, j, kind, kick, spare, more; colour only when a ball's ink or a shape's paint ran into
it (`tinted`); F only when the liquid stretches or is frozen (`stretched`); never serial or mat. 68 bytes for plain
water where it was 112.

### 5. Bind groups kept

`Body::step` made 23 bind groups every liquid step. They are kept in `Scratch.bound` (`Bound { key, groups }`) and
made again only when a buffer they name is another buffer (`key` is the 16 buffers, compared by identity): the
drops grew, a bigger batch of births or balls, the feel buffer grew, or the scratch came from another body. Each
entry point that names the sums has two groups (which buffer is at binding 4, which at 17).

### 6. Only the tiles of the grid that hold liquid

The grid pass still swept every node. Now the grid is cut into **tiles of 16 x 16 nodes, one workgroup each**
(`cs_grid` is `@workgroup_size(16, 16)`, dispatched `ceil(cols / 16) x ceil(rows / 16)`), and a workgroup whose
tile no drop touched in this scatter or the one before returns at once.

- `stamps` (binding 18, one `atomic<u32>` a tile): a scatter writes its number into the stamp of every tile it adds
  to (the drop's 3 x 3 nodes and its wall mirror's; a load first, a store only when it differs, so the stamps are
  nearly read-only).
- Each pass is told its number through a small uniform bound at a dynamic offset (`tick`, binding 19: entry n of a
  table written once holds n). With `base = Scratch.stamp` in the step's uniform (`st.feel.z`): a scatter in entry n
  stamps `base + n + 1`; grid pass n (1 to S) treats a tile as live when its stamp is `base + n` (this scatter) or
  `base + n - 1` (the one before), by equality, wrapping; a gather in entry n has taken `n - 1` substeps before it
  (the balls' place: this replaces the `counts[7]` tick and `cs_tick`).
- Why two scatters: grid pass n must clear the other sums, which scatter n - 1 wrote, and must put `nodes` back to
  empty where scatter n - 1 had liquid and scatter n has none. Everything else is already as a full sweep would
  leave it: sums zero, `nodes` empty (`(0, 0, 0, wall_share)`).
- That last point needs `nodes` to hold every node's walls' share to begin with, so **a liquid step that puts the
  walls** (`st.flags.y`: the first step of a body, of a copy, and every step while a shape moves) **sweeps every
  tile**, as before.
- A stale stamp (another body's scratch, a wrapped counter) can only make a tile live that needn't be, which costs
  time and changes nothing.

Why tiles and not "only the rows that hold liquid" (the first plan's idea): the owner's pour runs from the top of
the picture to the floor, so its rows are nearly all of them, while its tiles are the thin column and the pool.

### 7. The card's own timing in the status line

`Body::step_ms` was the time from sending a step to noticing its answer, which counts the two or three steps queued
before it. Where the device has timestamps (the editor asks for them already, `bpe_gfx::TIMING_FEATURES`), the
step's compute pass now carries a timestamp at each end (`StepTimer`), resolved into the count reader's staging
buffer, and `step_ms` is the card's own time. The status line reads "Liquid: 412,000 drops, 3.1 ms a step (64
small steps)" (`Body::substeps`, `Host::reading` giving a `Reading`, `liquid_readout`).

## What is not in this part, and why

- **Bigger time steps** (`CFL` in `sim.rs`): the substep count is the whole cost, but 0.2 already left the still
  box at 280 to 400 px^2/s^2 against 75 to 224 at 0.15 (measured in part 1). It changes the liquid, so it is not
  speed work under this part's rule. If it is ever tried again it needs part 1's tests and the owner's eye.
- **Preview at half the liquid steps**: at 120 steps a second every substep count doubles (they go by the time
  step), so nothing is saved below the cap of 64; at the cap the liquid is made softer instead. It would make the
  preview a different liquid from the export for almost no time.
- **Walls' share once when the walls are put**: it would need a word a node that no buffer has spare (`solid`'s
  eight are all read), and change 2 already takes it from nine times a drop to once a node.
- **Sorting the drops by cell** (so neighbours in the liquid are neighbours in memory): likely worth a good deal
  on a real card, but Un-mix's samples and the draw go by a drop's place in the buffer, and it can't be judged on
  the software card. An idea for when the owner's card can be timed.
- **Fewer atomic adds a drop** (volume from mass): changes the rounding, so not exact.

## Checking it

- **`crates/bpe-kit/tests/liquid_speed.rs`** (new, ignored, run by hand): plays a pour in a box, a filled box, the
  two liquid test scenes, a "strange" scene (ice, stretch, drying to stains, a clinging, inking, heating ball, one
  of each force) and, with `LIQUID_SCENE=<file>`, a saved scene, at the Detail levels in `LIQUID_DETAILS`, for
  `LIQUID_SECONDS`. One line each: drops, wall-clock seconds, and a hash of every number of every drop. **The
  hashes must match the ones from before each change** (kept in the "As built" table), on the same machine.
- `cargo test --release -p bpe-gfx --lib liquid_sim -- --test-threads=2` (the solver on the card directly, 25
  tests) and `cargo test --release -p bpe-kit --lib liquid -- --test-threads=2` (still boxes, slosh, balls,
  stranger liquids, sound), unchanged and passing.
- A new solver test: `the_card_times_a_step_when_it_can` (a device made with timestamps, when the adapter has them:
  `step_ms` is above 0 and under the wall-clock time of the run), and
  `skipping_the_empty_tiles_changes_no_drop` (a block of drops falls to a floor past a thin post, a second block is
  born far off after 60 steps, and from step 280 everything above a line is cut away, so tiles fill, empty and
  fill again: every drop of a body that skips tiles matches, bit for bit, a body given the walls again every step,
  which sweeps every tile; the switch is `st.flags.y`).
- `golden_liquid` (debug and release) must not move.
- The liquid picture tests (`liquid_pictures`, `liquid_look_pictures`, `lit_liquid_pictures`) and `restarts`
  pass unchanged. No full-size look check: nothing drawn changes and the drops are the same drops.
- clippy and fmt. CI runs the rest.

## What not to do

- Do not "tidy" `scatter` or `gather` while moving them: any change in the order of the arithmetic changes the
  drops' last bits and the prints with them.
- Do not let a pass both write a counter and have other threads of the same pass read it (the first try at
  folding `cs_tick` into the grid pass would have): the pass numbers come from the processor, through `tick`.
- Do not read the sums in the gather again: `nodes` is the only thing the gather reads of the grid.
- Do not skip a tile by anything but the stamps (not by the late drop count or a box round the drops from the
  processor: those arrive at no set time, and the liquid would differ from run to run).
- Do not make the first sweep after the walls are put a partial one: `nodes` must hold every node's walls' share.
- Do not add a storage buffer to `cs_g2p2g`: it names eight (1, 3, 4, 5, 6, 12, 16, 18), which is the most a
  device with default limits allows a shader stage. A ninth would fail to build on the owner's card.

## Timing on the owner's card

Nothing here was timed on a real card: the cloud has a software one, and so do the checks on the owner's PC (by
design, so pictures match). The status line now gives the card's own milliseconds a liquid step and the substeps,
so the owner can read the gain off their own scene the next time they ask for a build: a step must take under 4.2 ms
(1000 / 240) for the liquid to keep up with playback.

## As built

Built as planned, in one pull request (2026-11-22). What differs from the plan above, and what was found:

- **The first idea for clearing the grid was wrong and never built**: "clear the sums inside the grid pass of the
  substep before" can't work with one buffer of sums, because the gather still needed the volume word and the
  ghost test reads other nodes' volumes. Hence two buffers of sums and `nodes`.
- **The pass numbers**: the first try kept `counts[7]` and had the grid pass's first thread add to it. Once the
  grid pass also had to read the number (for the tiles) that was a pass reading what its own threads write, so
  the numbers come from the processor (`tick`).
- **The tiles test**: `skipping_the_empty_tiles_changes_no_drop` compares against a body that is given the
  walls again every step (which sweeps every tile), so it needs no test-only switch in the shader.
- The status line's reading is one value (`Reading { drops, step_ms, substeps, on_card }`, `Host::reading`),
  not the tuple the editor passed round.
- `counts[7]` is spare now (still zeroed where it was).
- **From the review** (one independent read of the whole change): a dropped body's scratch kept its bind groups,
  and with them the dropped body's drops buffer, while it waited as a spare; they are let go now (`Drop for
  Body`). The substeps the status line shows are those of the answered step the time belongs to
  (`CountReader.substeps`), not of the newest step sent. The tiles test runs on a grid that is not whole tiles
  (100 x 90) and reads the stamps back to see that tiles were skipped; the timing test checks a timer was made.
- **"Bit for bit" was shown on the software card only.** Some arithmetic now sits in another pass than it did
  (a node's volume and walls' share are worked out in `cs_grid` and stored; in `cs_g2p2g` the scatter takes the
  moved drop straight from the gather, not from the buffer). The numbers and their order are the same, but a
  card's driver may fuse a multiply and an add differently around them (nothing in the shader forbids it), so on
  a real card the last bits of a drop may differ from the old build on that card. That is the difference there
  already is between two drivers or two cards, not a different liquid. Comparing `liquid_speed` prints of main
  and this branch on a real card would settle it.

### Numbers

Seconds of work for one second of liquid (release, the cloud's software card lavapipe on 4 cores, old and
new run one after the other on a quiet machine; 2 s of liquid unless said):

| Scene | Detail | Drops at the end | Before | After | Less time | Print (the same before and after) |
| --- | --- | --- | --- | --- | --- | --- |
| A pour onto a shape | Normal | 1,909 | 6.32 | 4.45 | 30 % | `6ae6ac1d5ae0e4f7` |
| A pour onto a shape | Fine | 3,394 | 10.96 | 8.70 | 21 % | `be9f22bc8c34a07d` |
| A pour onto a shape | Extreme | 13,576 | 40.62 | 33.64 | 17 % | `3340751fd6a9e3c7` |
| The owner's scene (Water Weird) | Normal | 2,036 | 6.45 | 5.01 | 22 % | `2e6e6daa4dd84102` |
| The owner's scene | Fine | 3,620 | 11.18 | 10.28 | 8 % | `8313d4d264ee9c06` |
| The owner's scene | Extreme | 14,481 | 40.75 | 36.37 | 11 % | `e64ce38fdc0c20ab` |
| The owner's scene, 6 s | Normal | 6,109 | 11.70 | 10.54 | 10 % | `3ef9bc24a461e5ef` |
| A filled box | Normal | 19,832 | 23.43 | 22.15 | 5 % | `7496ce5e58cfb2f3` |
| A filled box | Fine | 34,888 | 53.70 | 51.00 | 5 % | `7fd41259d8014e93` |
| Liquid water (a ball drops in) | Normal | 18,560 | 26.21 | 25.62 | 2 % | `0559964d8407b491` |
| Liquid water | Fine | 32,920 | 55.27 | 55.46 | 0 % | `49f1858d986973ad` |
| Liquid paint mixing | Normal | 19,417 | 24.06 | 23.43 | 3 % | `c3855201c59d38ea` |
| Liquid paint mixing | Fine | 34,638 | 47.99 | 48.93 | -2 % | `ab3ff9c30915f76b` |
| Strange (ice, stains, forces) | Normal | 18,478 | 27.12 | 25.91 | 4 % | `fd1a0853fe08cdde` |
| Strange | Fine | 32,776 | 58.41 | 52.64 | 10 % | `085a5b3d7f9e3e51` |

So on the software card: a fifth to a third less time where the liquid covers little of the grid, about a tenth
on the owner's scene at Fine and Extreme, and next to nothing (0 to 5 %, inside the noise for some lines) where
liquid fills the grid. That card is a processor: for it a pass costs what the arithmetic in it costs, the
arithmetic is the same on purpose, and there is no cost to starting a pass. A real card is the other way round
(arithmetic is nearly free, starting 76,800 passes for a second of liquid is not), so its gain should be larger,
but **it has not been measured**: see "Timing on the owner's card".

All 15 prints are the same before and after, which is the check that the liquid did not change: 2 s at Normal
and Fine of every scene, 2 s at Extreme of the pour and the owner's scene, and 6 s of the owner's scene.

### Checked

- `cargo test --release -p bpe-gfx --lib liquid_sim` (27 tests: the 25 from before, unchanged, and the two new
  ones), `cargo test --release -p bpe-kit --lib liquid`, `golden_liquid`, `liquid_pictures`,
  `liquid_look_pictures`, `lit_liquid_pictures` and `restarts` in release; clippy and fmt. The debug goldens and
  the rest ran in CI.
- No golden moved and no picture test changed.
