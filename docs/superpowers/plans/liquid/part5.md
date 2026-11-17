# Liquid part 5: sound (plan)

Spec: `docs/superpowers/specs/2026-11-17-liquid-thing-design.md`, section 9 (Sound) and section 12 part 5.
Builds on parts 1 and 2 (`docs/notes/liquid.md`). Parts 3 (look) and 4 (stranger liquids) are built at the same
time in other threads, so GPU changes stay small and in their own places (a new shader file, one spare feel word).
No new built-in scenes (Aedan, 2026-11-17).

## Goal

A Liquid thing makes sound with the sound engine it already has (voices, chains, modulators, buses), so a producer can
make splashes, plops, drips, glugs, pours and bubbles; any sound can be muffled when its ball is under a liquid and
pitched by the ball's speed, its depth or the colour of the liquid round it; and any sound setting can be moved by how
the liquid sloshes, how mixed it is, how high it stands and how much of it there is.

## Decisions

1. **The Liquid thing gets the existing Sound card** (`sound.voice`, the Sound slot, not a default slot), as Balls and
   Emitters did in W10. One card, one voice per Liquid thing; the six "splash voices" of the spec are **starting
   sounds** (presets of that card: oscillators, noise, envelopes, chain layers and which moments play), picked from
   the Sound tab. More sound round the same liquid comes from the balls' and shapes' own voices (they hear the liquid's
   depth and colour too). Several voices on one thing would rework the whole sound engine and is not done.
2. **What a liquid's sound plays** (new `VoiceSettings` switches, shown only for a Liquid thing):
   - **Splashes** (`moments_splash`, on): the Splash event (code 2).
   - **Balls go in** (`moments_go_in`, off): Enter (code 0). **Balls come out** (`moments_come_out`, off): Leave (1).
   - **Pours** (`moments_pour`, off): a new event, code 3 (`POUR`), at most `POUR_RATE` = 30 a second for each Pour
     layer that is pouring (on liquid steps where it gave birth: `host.step % (rate / 30) == 0`), `a` = the liquid
     itself with the pour's index as item, at the pour's mouth, `speed` its pour speed (px/s).
   - **Bubbles** (`moments_bubbles`, off): a new event, code 4 (`BUBBLE`), for a ball inside the liquid: each world
     step it is in, a chance of `speed / 100 px/s × 8 / steps a second` (at most 60 a second a ball), rolled with
     `bpe_core::dice` from (step, ball thing, row), `speed` the ball's speed.
   - Pour and Bubble events are logged only for a liquid whose own Sound card is on and plays them, so a liquid
     without sound logs exactly what it did (goldens, rules and logs unchanged). Their switches are in the liquid's
     physics fingerprint (`physics_print_extra`) and `inputs_hash`, since they change the log.
   - Hits from (`play_heard_from`) picks the balls for codes 0, 1, 2 and 4; a pour has no ball and always plays.
   - The rules' When Liquid block ignores codes 3 and 4 (its `takes` matches only 0..2).
3. **A liquid with sound is in the main world.** Events only reach the main log when the liquid steps there (part 2).
   New runtime hook `ThingSim::kept_by_scene(&self, scene, id) -> bool` (default false), read by `is_beside` beside the
   rules' and things' keeps. The Liquid sim says true when the scene **hears a liquid**
   (`things::liquid::scene_hears_liquid`): any enabled Liquid thing has its Sound card on, or any enabled thing's
   Sound card on has a Liquid modulator on. All Liquid things share one body, so all are kept. A kept liquid with
   Pushes balls off still pushes no balls (part 2's push scale is 0), so ball paths are as without sound
   (tested). Turning sound on restarts the physics (the beside set changes); sound edits don't.
4. **Wet balls** (per ball, read by every voice): the host keeps `wet: Arc<BTreeMap<(ThingId, u32), Wet>>` from each
   feel, `Wet { depth (px of liquid above the ball's top), colour (linear rgb of the drops touching it), liquids:
   [(ThingId, share); 4] }`. Depth is a new feel word (`feel_at::DEPTH` = 12, one of the 4 spare words): `cs_feel`
   walks up (against gravity) from one cell above the ball's top, a cell at a time up to 255, while the grid's volume
   is over 0.3, and stores the count × 16. `things::liquid::wet_of(world, ball)` reads it without allocating.
5. **Liquid stats** (per liquid, for the modulator and Colour notes): a new shader `liquid_stats.wgsl` (its own
   pipeline, made once on the `LiquidCard`, so the step's ENTRIES list is untouched) runs over the drops when the
   feel is sent and sums per liquid in `u32` atomics: count, speed (×16, at most 2047 a drop), row (at most 4094),
   colour channels quantised to 0..63 and their squares (all fit 2^20 drops in u32). `send_feel` now copies the feel
   and the 16 × 12 stat words into one read-back and is sent after every batch in the main world, also with no balls
   (so a pour-only scene's modulators move); `Feel::stats()` gives them. `host.stats` keeps per liquid `Stat { drops,
   speed (px/s), height (0 bottom of the area .. 1 top), colour (mean rgb), mixed (1 − largest channel spread) }`.
   `things::liquid::stats_of(world, pick)` sums the liquids `pick` takes.
6. **Pitch from** (every voice, Playing panel, read at patch build like Pitch follows size): `play_pitch_from`
   (`PitchFrom`: Off, Speed, Depth, Colour) with **Pitch range** (`play_pitch_range`, st, default 12):
   - Speed: + range × the hit's strength (0 slow .. range at Full strength).
   - Depth: − range × depth / **Deepest at** (`play_deep_at`, px, default 150), so deeper is lower.
   - Colour (**colour as notes**): the note is the hue of the colour round the ball (`Wet.colour`) spread over the
     range above the written note and moved into the voice's key (`music::scale::move_into_key`); a grey colour keeps
     the written note. When two or more liquids touch it (each at least a fifth of the contacts), each liquid's own
     colour (its `Stat.colour`) plays its note together, as a chord (strummed by the voice's Strum). A ball in no
     liquid plays as written.
7. **Underwater** (every voice, read at patch build): **Muffled under liquid** (`play_under`, 0..100 %, default 0 =
   off) and **Muffled to** (`play_under_cutoff`, Hz, default 300). A note whose ball is under a liquid gets a 12 dB
   low-pass (two one-poles, per note, set at the note's start) at a cutoff from 20 kHz down towards Muffled to by
   `play_under` × depth / Deepest at, in ratio (log) steps. Off or not under, the note is untouched (bit for bit).
8. **Liquid modulator** (`mod.liquid`, "Liquid", up to 8 a card, shared by the whole voice, no knobs): **Reads**
   (`LiquidReads`: Slosh, Mixed, Height, Amount), **Liquid** (Target, Anything = all liquids together), **Full at**
   (px/s, Slosh only, default 200), **Smooth** (ms, default 50), **Invert**. Slosh = mean drop speed / Full at;
   Mixed = 1 − spread; Height = mean height in the area; Amount = drops / the liquids' Most drops. Its settings are
   read when the patch is built (a choice of what to read can't be keyframed; numbers aren't routable targets, to keep
   the knob layout as it is). The voice reads it once a block from its world (`ModCx.world`); Follow sound reads the
   same from the editor's world (`control.rs`). No liquid: 0.
9. **Starting sounds** (`bpe_kit::sound::liquid_sound(reg, LiquidPreset)`): Splash (white noise through a band-pass
   sweeping down, short), Plop (sine with a quick pitch drop by an envelope, on Balls go in), Drip (sine chirping up,
   high, short), Glug (low triangle with a wobbling filter LFO, on Balls come out), Pour (pink noise grains on Pours,
   high-passed), Bubbles (sine blips rising, on Bubbles, random pitch over a range). Enable sound on a Liquid thing
   gives Splash. The Sound tab of a Liquid thing shows a **Start from** row of six buttons that replace the card
   (one undo step), keeping it on.
10. **Follow sound**: liquid settings already pulse; the Follow sound menu and the inspector's Sound tab list Liquid
    things (`sound::makes_sound(kind)` names every kind with a Sound tab).

## Files

- `crates/bpe-runtime/src/plugins.rs` (`kept_by_scene`), `world/prints.rs::is_beside`.
- `crates/bpe-gfx/src/shaders/liquid_sim.wgsl` (`cs_feel` depth), `shaders/liquid_stats.wgsl` (new),
  `liquid_sim/mod.rs` (`feel_at::DEPTH`, `STAT_WORDS`, `stat_at`, `Feel::stats`, `send_feel`), `liquid_sim/stats.rs`
  (new: pipeline, encode), `liquid_sim/tests.rs`.
- `crates/bpe-kit/src/things/liquid/`: `mod.rs` (Sound slot, exports), `sound.rs` (new: `LiquidSound`, `Wet`, `Stat`,
  `wet_of`, `stats_of`, `scene_hears_liquid`, pour and bubble events), `feel.rs` (depth, wet, stats, bubbles),
  `sim.rs` (pours, keep, prints, send with no balls), `state.rs` (Host fields, `inputs_hash`), `tests_sound.rs` (new).
- `crates/bpe-kit/src/sound/`: `voice.rs` (new settings, `PitchFrom`, `makes_sound`, `hears_liquid`, fits),
  `mods.rs` (`LIQUID_MOD`, `LiquidModSettings`, `LiquidReads`), `presets.rs` (new: `LiquidPreset`, `liquid_sound`).
- `crates/bpe-sound/src/`: `hits.rs` (`plays`), `voices/hit_filter.rs` (`LiquidMoments`), `patch.rs` (spec fields),
  `voices/slots.rs` (`NotesSpec` pitch from, under), `voices/shape.rs` (shift, colour chord, muffle, `Src::Liquid`),
  `mods/liquid.rs` (new), `control.rs`.
- `crates/bpe-studio/src/`: `inspector/mod.rs::sounds`, `follow.rs`, `sound_tab/moments.rs` (`show_liquid`, Start
  from), `sound_tab/mod.rs`, `sound_tab/playing.rs`, `sound_tab/write.rs` (enable with the Splash preset; preset
  edit), `sound_tab/mods/` (Liquid row and panel).
- Docs: `docs/notes/liquid.md` (Part 5 as built), `docs/notes/sound.md` (one paragraph).

## Tests (what passing looks like)

- `liquid_sim::tests`: a ball held under still water has a depth word > 0 and one at the surface 0; the stats count
  every drop and their mean colour is the water's.
- `things::liquid::tests_sound`: a liquid with Pushes balls off and sound on is in the main world, logs Splash, and
  the balls move exactly as with sound off (same bodies); Pours logs code 3 at about 30 a second while pouring and
  nothing with the switch off; Bubbles logs code 4 for a ball under water; `wet_of` gives depth and colour; `stats_of`
  gives a Slosh that rises after a ball drops in and Amount near the fill; same twice and after a seek.
- `bpe-sound/tests/liquid_sound.rs`: a Liquid thing's Splash plays a note at the splash's sample; Balls go in off plays
  none; Underwater lowers the high-frequency energy of a note whose ball is under; Pitch from Colour plays the hue's
  note; the Liquid modulator moves a routed knob; render allocates nothing with a Liquid modulator (alloc test).
- Editor: a Liquid thing has a Sound tab; Enable sound gives the Splash preset; Start from Plop switches Balls go in
  on; Add modulator lists Liquid.
- Unchanged: every existing golden (physics, liquid, sound), `restarts.rs` passes with the new switches.

## Not doing

- Several voices on one thing; Balls voices hearing their own balls' liquid moments (they hear depth/colour already).
- A per-hit filter that moves while the note plays (it is set at the note's start).
- Liquid modulator numbers as route targets or keyframes.
- Bubbles drawn (part 3's look), or bubbles from anything but balls moving under the liquid.

## Risks

- A main-world liquid now waits for a read-back every world step even without balls: a little slower, only when a
  liquid pushes balls or makes sound.
- Parallel parts 3 and 4 edit the same shader and files: rebase on main before merging, keep my edits in new files.
