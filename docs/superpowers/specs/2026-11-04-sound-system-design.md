# The sound system (shapes make sound)

Date: 2026-11-04. Status: design approved by the owner (Aedan), all six parts, in the "Design the shape sound
system" thread.

- Review doc with every decision, idea table and part design: https://claude.ai/artifact/7TeVQgjvAuUyGFdV1YR5UF
- Mockup canvas: https://claude.ai/artifact/UdZNVDyXauvYoAThKe37qA. The chosen boards are Wide (Sound tab, layout
  B), ChainFold (layer chain C), Mods (modulators), NotesGrid (Notes B), MixerStrips (Mixer B) and EqEditor.

This spec replaces nothing in the code. Version 1's sound (`bpe-synth`, `bpe-mix` on the `version-1` branch) is
history; a few of its DSP blocks are ported (see "Reuse from Version 1").

## Goal

Shapes play sound when balls hit them. Each shape is a two-oscillator synth with a layer chain per oscillator,
hit, distance, LFO and envelope modulators, a piano-roll note grid with slots, and an output that feeds one of
three effect buses and a true-peak master. It sounds like a real plugin: no aliasing, no clicks, no zipper noise.
What the editor plays is exactly what the export carries.

## What the owner decided

| # | Question | Decision |
| --- | --- | --- |
| – | Split | Six parts, built in order 1, 2, then 3, 5 and 6 in parallel, then 4 after 3. |
| – | Engine | **C: render ahead** of the playhead in short blocks on a background thread, with the same code as export. |
| 12 | Master ceiling | True-peak limiter at **-1 dBTP** by default, plus a LUFS meter with a -14 LUFS mark. |
| – | Tempo | One fixed tempo and time signature per scene, kept in one place so tempo changes can come later. |
| 1 | Note length | A **Gate** on each On-Hit ADSR: Fixed (ms or beats, default) or Until next hit on this shape. |
| 2 | Voices | **Poly** (limit 16, 1 to 32, oldest stolen with a fade) plus **Mono** with optional Glide. No Legato. |
| 3 | Which hits count | Min hit strength, per-ball Retrigger guard (30 ms), Heard from (all balls or chosen Balls things). |
| 10 | Note names | **FL Studio**: middle C = C5, lowest octave C0. |
| 11 | Keyframes | **Every** sound knob takes timeline keyframes. |
| – | Sound tab layout | **B. Wide plugin**: the inspector widens to 760 px, Osc A and Osc B side by side. |
| – | Osc gain | **Gain before the chain** (drives the layers), plus a post-chain **Level**; both take modulators. |
| 7 | FM and PD lower in a chain | **Phase modulation of the sound reaching the layer.** PD is one-sided (CZ-style). A/B test renders before locking. |
| 8 | A and B feeding each other | **Allowed as feedback**: the return path runs one sample late, with a loop mark. |
| – | Chain look | **C. Folded strips** with a mini view, summary and Mix knob. |
| 4 | Drones | **Plays**: On hit (default), Always, or While a ball is in range. |
| 5 | Distance follows | **Follows**: Nearest ball (default), Each note's own ball, Average of all, How many in range. |
| 6 | Combining modulations | **Add** on most knobs; on Gain and Level each one **scales** (a VCA each). |
| 32 | Sound moves the picture | **Any setting of any thing** can Follow sound. |
| – | Hit values | A new **Hit** modulator kind. |
| – | Key | **Per shape only** (no scene key). |
| – | Notes layout | **B. Slot grid** lined up with the keys. |
| 43 | Sends | **Yes**: the main bus plus post-fader Send knobs; buses can send to each other, no loops. |
| 48 | Sidechain | Compressor sidechain from **Self, another bus, or Hits** on a chosen shape, with a sidechain filter. |
| – | Mixer layout | **B. Console strips**, FL-mixer style; a slot opens its editor in the wide inspector. |

The owner moved these ideas to Now: 17 (more Warp modes), 18 (Sub osc), 22 (Ladder, Comb, Formant), 24 (Wavefold,
Bitcrush, Downsample, Rectify), 28 (ADSR Delay and Hold), 31 (more than two of each modulator), 32 (Follow sound),
50 (Chorus, Phaser, Flanger, Width), 54 (stems and MIDI). Every other idea in the doc keeps its pick: 41 (pitch
from ball size) is Later, 51's metronome click is Later, 53's sound files are Later.

Small calls made while designing (told to the owner): bus knobs take keyframes but no modulators (a bus has no
notes or hits of its own); the EQ starts with a 30 Hz low cut.

## Architecture

### Crates

Dependencies go bottom to top. Two new crates sit between `bpe-kit` and the apps.

- **`bpe-dsp`** (new): pure signal processing with no scene knowledge. Every block is a plain struct with
  `reset`, `set_*` (targets, smoothed inside) and `process(&mut self, buf)` over a block. No allocation after
  `prepare(sample_rate, max_block)`, no locks, no `std::time`. Holds: band-limited wavetable sets (wave × warp ×
  octave mip), noise, the ZDF/TPT state-variable filter, ladder, comb, formant, shelves and bells, waveshapers
  with an oversampler (polyphase halfband cascade 2×, 4×, 8×), bitcrush and downsample, delay lines with
  fractional reads (cubic Hermite / allpass), the phase-modulation delay, envelopes, LFOs, smoothers, the EQ,
  compressor, limiter with true-peak detection (4× oversampled peak), the three reverbs, chorus, phaser, flanger,
  width, the LUFS meter (EBU R128 K-weighting, momentary 400 ms, short-term 3 s, integrated with gating), and a
  denormal guard (FTZ/DAZ on x86 inside the render call, plus tiny DC offsets where feedback could decay to
  denormals). Unit tests per block.
- **`bpe-sound`** (new): the engine. Reads a `Scene` and its resolved settings, runs its own deterministic
  `bpe-runtime` world in step with the audio, turns hits and positions into notes and modulation, runs voices,
  buses and the master, and writes blocks. Also the offline renderer (export, stems, MIDI writer) and the
  analysis taps for meters and scopes. Depends on `bpe-core`, `bpe-runtime`, `bpe-kit` (for the settings structs
  and thing kinds) and `bpe-dsp`.
- `bpe-kit` gains the sound **settings schemas** (card kinds and layer kinds, `#[derive(Settings)]` structs) under
  `src/sound/`, registered in `kit()`. No DSP lives in `bpe-kit`.
- `bpe-studio` gains the audio output (`audio_out.rs`, cpal), the sound thread (`sound_job.rs`), the knob and
  scope widgets (`style/` for drawing), the Sound tab (`sound_tab/`) and the Mixer panel (`mixer/`).
- `bpe-cli` renders the sound offline and muxes it into the mp4.

Keep `CLAUDE.md`'s crate map updated when each part lands.

### Where the sound settings live (scene format 8)

Everything uses the existing card and layer machinery, so copy, paste, keyframes, undo and save work the way they
already do. Scene files go from format 7 to **format 8**; older files still load (sound simply absent).

- Cards are keyed by `SlotId` (one card per slot), so two slots are added: **`SlotId::Sound`** and
  **`SlotId::Mixer`**, both left out of the inspector's card list (shown by the Sound tab and the Mixer panel).
- **A shape's sound** is one card of kind **`sound.voice`** in `SlotId::Sound`. "Enable sound" adds the card
  with the starter patch; unticking sets its `enabled` param false and keeps everything. Copy sound / Paste sound
  copy the whole card.
- The card's own params hold the oscillator, Playing, Notes and Output settings under fixed key prefixes:
  `a_…` and `b_…` for the oscillators (`a_wave`, `a_gain`, `a_level`, …; one `OscSettings` struct expanded twice
  with the derive's `prefix`), `play_…`, `notes_…`, `out_…` (`out_bus` 1..3, `out_send1`..`out_send3`). Plain
  keys never contain `/`, which marks a layered key.
- **Chain layers and modulators** are `Slot.layers` entries, using new `LayerSection`s:
  - `LayerSection::Chain` with a new `Layer.lane: Option<u8>` (0 = Osc A, 1 = Osc B; kept in chain order within
    each lane). Kinds: `chain.modulation`, `chain.filter`, `chain.distortion`.
  - `LayerSection::Modulator`. Kinds: `mod.adsr`, `mod.distance`, `mod.lfo`, `mod.hit`. Their order is the tile
    order. A new card starts with ADSR 1 and 2, Distance 1 and 2, LFO 1 and 2 (only ADSR 1 on).
  - Layer settings live under layered keys `"<layer id>/<key>"` as for Look layers.
- **Modulation routes** are a new `Value::Routes(Vec<ModRoute>)` at the card key `routes` (never keyframed):

  ```rust
  pub struct ModRoute {
      pub source: LayerId,        // a modulator layer in this card
      pub target: String,         // a card key, layered or not: "a_gain", "<layer id>/cutoff"
      pub amount: f32,            // -1..=1, fraction of the target's range (or dB-to-silence on scaling knobs)
      pub bipolar: bool,          // Alt-drag: centred on the knob value
      pub invert: bool,
  }
  ```

  Deleting a modulator or a layer deletes its routes in the same edit (in `edit.rs`).
- **Note slots** are a new `Value::Slots(Vec<NoteSlot>)` at `notes_slots` (never keyframed), where
  `NoteSlot { notes: Vec<u8> /* MIDI numbers, FL naming C5 = 60 */, order_clicked: Vec<u8> }`. A **Slot**
  keyframe is an Int setting `notes_jump` (1..=16, keys always `Ease::Hold`).
- **Follow sound** (idea 32) extends the existing pulses: `PulseSource` gains
  `Sound { shape: ThingId, source: SoundSource }` with `SoundSource::Modulator(LayerId)` or `SoundSource::Level`.
  The pulse's `amount` is the range. Modulators are worked out from the physics alone, so following sound never
  needs the sound engine to run (a picture-only export moves the same). A setting that changes physics may follow
  sound too: it reads its source as it was at the end of the **previous** physics step, so there is never a loop.
- **Tempo** is on the scene settings thing (`SceneSettings`, group "Sound"): `tempo` (20..300 BPM, decimals,
  default 120), `time_top` (1..16, default 4), `time_bottom` (Choice 2/4/8/16, default 4).
- **The mixer** is one card of kind **`sound.mixer`** in `SlotId::Mixer` on the scene settings thing, added by
  `new_studio_scene` and by `ensure_camera`'s sibling `ensure_mixer` for older files. Its params: the master under
  `master_…` (Input, Output, Ceiling, Release) and each bus under `bus1_…`, `bus2_…`, `bus3_…` (name, on, fader,
  pan, mute, solo, `send1`..`send3`). Bus 1 is on, Buses 2 and 3 off. Bus effect layers are its `Slot.layers`
  with section `LayerSection::Effect` and `Layer.lane` = bus index 0..2, kinds `fx.filter`, `fx.distortion`,
  `fx.eq`, `fx.compressor`, `fx.delay`, `fx.reverb`, `fx.limiter`, `fx.chorus`, `fx.phaser`, `fx.flanger`,
  `fx.width`.
- `Thing.colour` (the track colour) is what draws modulation rings; no new colours are made up (the design-system
  test still applies).

### The engine (part 1)

- **Format**: 48 kHz stereo, `f32` samples, `f64` where precision matters (oscillator phase accumulators, filter
  coefficients from cutoff, the bar clock). Blocks of **64 samples** (750 a second). Modulators and smoothed
  parameters update once a block and ramp per sample inside it.
- **One render call**: `Engine::render(&mut self, out: &mut [f32; 128])` renders the next block: steps its own
  physics world as needed (240 Hz by default, so a step is 200 samples), collects that step's hits, places each
  hit at its exact sample, advances notes and slots, renders voices into shapes, shapes into buses, buses into
  the master. The editor and export call the same function; nothing else produces sound.
- **Hit timing**: `Event` gains `at: f32`, the fraction (0..1) of the step at which the contact happened, worked
  out in the contact solver from the approach speed and depth. It is **not** fed into `Log::hash`, so physics
  goldens do not move. Sample offset = `step_start_sample + round(at × samples_per_step)`.
- **Determinism**: same scene → same samples, every time and on every machine the physics agrees on. Random
  things (noise, Random LFO, random phase, Random play order) draw from a `seed`-derived counter RNG keyed by
  (shape, voice index, note number), never from thread state.
- **The sound thread (editor)**: a background thread owns an `Engine` and renders about **50 ms ahead** into a
  lock-free ring buffer that the cpal callback drains. The cpal callback never allocates or locks. The audio
  device's played-sample counter is the **clock**; the picture shows the time the speakers are at, so picture and
  sound never drift. If rendering falls behind, the editor's clock waits (both pause together) instead of
  crackling. Exports are never affected.
- **Edits while playing**: a sound-only edit (any sound card setting) is sent to the thread as a new resolved
  sound patch and lands in the next block it renders (heard in about 50 ms). An edit that changes physics
  restarts the sound world from the current time, with pre-roll.
- **Pre-roll**: starting partway through, the engine first renders the 4 seconds before that point silently and
  as fast as it can (reverb tails and drones are already ringing). Played from the start, the editor's samples
  equal the export's sample for sample.
- **Output device**: the default Windows output device through cpal (WASAPI shared mode). If it isn't at 48 kHz,
  a high-quality windowed-sinc resampler (the `rubato` crate) converts it. A speaker button and Listening volume
  by the play controls change only what you hear.
- **No allocation or locks on the audio path**, denormals flushed, SIMD across unison voices (8 lanes, with the `wide`
  crate on stable Rust, so it cross-builds for `x86_64-pc-windows-gnu`).
- **Export**: `bpe-cli` renders the whole sound offline first (fast; written to a temporary 32-bit float WAV with
  `hound`), then ffmpeg takes it as the second input in place of `anullsrc`, encoded **AAC 320 kbps**, 48 kHz.
  A scene with no sound still gets a silent track.
- **Built for later**: a `Voice` source trait any thing kind can own (idea 55, only shapes use it now); tempo read
  through one `TempoMap` so tempo changes can come later.

### Reuse from Version 1

Port and adapt from `origin/version-1:crates/bpe-synth/src/blocks/`: `filter.rs` (Cytomic SVF), `tables.rs`
(band-limited tables), `ladder.rs`, `dynamics.rs` (true-peak limiter), `maths.rs`. Read `delay.rs`, `eq.rs`,
`chorus.rs`, `drive.rs`, `crush.rs` for ideas only. **Do not** port `reverb.rs` (Freeverb) or the old program/node
system. Version 1's `audio_out.rs` and `sound_job.rs` in the studio show cpal setup; its whole-track re-render is
replaced by the render-ahead thread.

## Part 1: engine and playback

What you see:

- A **Things / Mixer** switch at the top of the left panel. In part 1 the Mixer shows the Tempo row (Tempo, Time
  signature) and the Master strip only: Input, Output, Ceiling (-1 dBTP), Release, peak meters and the LUFS
  readouts (Momentary, Short-term, Whole clip, True peak) with a -14 LUFS mark. Part 6 adds the bus strips.
- A **bars and beats grid** on the timeline from the tempo (beat lines faint, bar lines stronger, bar numbers on
  the ruler).
- A speaker button and Listening volume by the play controls.
- Exports carry the real soundtrack.

Tests: render twice gives equal samples; editor-path render from 0 equals export render; a seek with pre-roll
matches a straight render after the pre-roll window to within -120 dB for a decaying test source; the limiter
never exceeds the ceiling as true peak (4× oversampled check) on a hot test signal; LUFS meter matches the EBU
Tech 3341 test signals within 0.1 LU; the cpal callback path is checked allocation-free with a counting
allocator in a test.

## Part 2: the shape's voice

Layout B (canvas board Wide). The inspector top row gets **Settings | Sound** buttons on a shape. Sound widens the
inspector to **760 px** (drag its edge 640..960); Settings restores the width.

- First it shows only **Enable sound**. Ticking adds the `sound.voice` card with the **starter patch**: Osc A
  Triangle on, Osc B off; ADSR 1 on, routed to Osc A Gain (Delay 0, Attack 2 ms, Hold 0, Decay 350 ms, Sustain 0,
  Release 200 ms, Gate Fixed 100 ms); note C5; Bus 1. Right-click Enable sound: Copy sound, Paste sound.
- Top to bottom: Osc A | Osc B side by side (each with its waveform view, knobs and layer box); the modulator row
  and the picked modulator's editor; Notes; Playing | Output side by side. Below the oscillators it scrolls.
- **Knob widget** (`style/knob.rs`): drag up/down, Shift fine, wheel nudges, double-click resets, double-click the
  value to type; right-click Copy value, Paste value, Create keyframe (part 4 adds Modulate by). Value arc in
  muted grey; modulation rings outside it in the shape's track colour. Named via `labelled_by` for kittest.
- **Each oscillator**: on switch; Wave Sine, Triangle, Saw, Square, Noise (White / Pink); Warp mode Sync, Bend +,
  Bend −, Asym +, Asym −, PWM, Mirror, Flip, Quantize, Squeeze; knobs Gain (before the chain, dB, −inf..+12),
  Level (after the chain, dB, −inf..+12), Warp (0..100 %), Octave (−4..+4), Semi (−24..+24), Cents (−100..+100),
  Phase (0..360°, with Restart on hit / Free), Unison (1..16), Detune (0..100 cents spread), Spread (stereo
  0..100 %), Blend (centre against sides), Random phase (0..100 %), Pan. **Sub**: off / Sine / Square, −1 or −2
  octaves, Level. The waveform view draws the warped wave live, moving with Warp and its modulation.
- **Wavetables**: each wave × warp mode is baked once at startup (or lazily, off the audio thread) into 64 warp
  steps × one band-limited copy per octave (2048-sample frames, mip by halving harmonics), crossfaded between warp
  steps and between mips. Sync is rendered band-limited in the tables too. Noise is generated, not tabled.
- **ADSR 1** (the only live modulator in part 2): Delay, Attack, Hold, Decay, Sustain, Release, Velocity (how much
  hit strength scales the depth); segment curves bent by dragging the segment in the envelope view; corner
  handles; Gate Fixed (ms, or beats with an ms/Beats switch) or Until next hit, drawn as a dashed line. Its
  Modulates list holds Osc A Gain. Other tiles show Off until part 4.
- **Notes (part 2 only)**: a keyboard C4..B6 with − and + octave buttons; clicking a key toggles it and plays it
  (heard even when stopped, idea 37). One slot; several notes on = random pick.
- **Playing**: Poly / Mono, Voices (1..32, default 16; oldest stolen with a 5 ms fade), Glide (Mono only), Min hit
  strength, Retrigger guard (default 30 ms, per ball), Heard from (all balls, or chosen Balls things — a
  `Value::Target`).
- **Output**: Volume, Pan, Follow hit (pan toward where the hit is across the screen, 0..100 %), Bus (only Bus 1
  until part 6, which goes straight to the master), a level meter.
- Every knob takes keyframes; the shape's timeline group lists sound keys as for other settings.

Tests: alias test (a saw and every warp mode swept to C8, folded components under −90 dB); declick (note start,
steal and fast knob moves have no step larger than a set bound); the starter patch renders a known golden; Poly
voice limit and stealing; Mono glide reaches pitch in the set time; Retrigger guard and Min hit strength filter
hits; kittest: Enable sound adds the card, the tab widens the inspector, knobs are named and reachable.

## Part 3: oscillator layers

Chain C (canvas board ChainFold). Each oscillator's layer box shows its chain as folded strips.

- Order: Osc → **Gain** → strips top to bottom → **Level** → Pan. Every note runs its own copy of the chain (per
  voice). **Solo** on a strip plays the chain cut at that layer.
- A strip: fold arrow, on switch, name and one-line summary, a live mini view, its **Mix** knob. Click to open
  (any number open). Opened: type, view, knobs, Solo. **+ Add layer**: Modulation, Filter, Distortion, any number
  of each. Drag to reorder. Right-click: Rename, Duplicate, Copy layer, Paste layer, Delete; pastes into either
  oscillator.
- **Modulation**: FM, Phase distortion, AM, Ring. Source: Osc A or Osc B raw, or after any layer of either chain.
  Knobs Amount, Output, Mix. FM and PD are **phase modulation of the sound reaching the layer**, done as a short
  moving fractional delay (max 2 ms, cubic interpolation); first in the chain it equals true FM on the
  oscillator. PD uses the source one-sided (squeezes the wave like a Casio CZ). A source that loops back runs one
  sample late and shows a loop mark. **Before locking FM and PD, render A/B test clips for the owner** (the
  owner was promised these).
- **Filter**: Low pass, High pass, Band pass, Notch, Peak, Low shelf, High shelf (12 or 24 dB), Ladder (24 dB
  with its own drive), Comb (pitched; Key track 100 % plays the note), Formant (Vowel A-E-I-O-U morph in place of
  Cutoff, plus Shift). Knobs Cutoff, Reso, Drive, Key track, Gain (Peak and shelves), Mix. The view draws the
  real response; drag its handle sideways for cutoff, up/down for resonance.
- **Distortion**: Soft clip, Hard clip, Tube, Tape, Diode, Overdrive, Wavefold, Bitcrush, Downsample, Rectify.
  Knobs Drive, Bias, Tone, Output, Mix (Bitcrush shows Bits, Downsample shows Rate in place of Drive). Quality
  1×, 2×, 4× (default), 8×; Bitcrush and Downsample always 1×. The view draws the transfer curve.

Tests: filter curves against textbook responses for every type and slope; a resonance sweep under fast
modulation stays bounded; a C8 saw through every distortion at 4× keeps aliasing under −70 dB; FM first in chain
matches an analytic FM sine pair; a loop renders and stays finite.

## Part 4: modulators

Canvas board Mods.

- **Row**: ADSR 1, ADSR 2, Distance 1, Distance 2, LFO 1, LFO 2, then **+** (another ADSR, Distance, LFO or Hit;
  up to 8 of each). Right-click a tile: Rename, Duplicate, Delete. A tile shows its name, target count and a drag
  handle (four arrows). Click opens its editor below; hover lights its rings.
- **Assigning**: drag a tile's handle onto any knob but its own (oscillators, layers, other modulators, Output),
  or right-click a knob → **Modulate by**. A route row: target name, range in real units, Amount −100..+100 %, Inv,
  remove. Drag a ring's end to set the range; Alt-drag makes it bipolar. A modulator driving another's knobs that
  makes a loop runs one block (1.3 ms) late.
- **Combining**: routes **add** on most knobs (each its own range); on **Gain and Level** each route **scales**
  (a VCA each, so ADSR × Distance works and either at −inf silences).
- **Per note or shared**: ADSR and Hit always per note; Distance per note only with Follows = Each note's own
  ball; LFO shared when Free, per note when Start = Per note.
- **Distance**: Measure from Centre or Edge (nearest edge, inside or outside); Near, Far (px), Curve, Smooth;
  output 1 at Near or closer, 0 at Far or beyond; Invert. Follows Nearest / Each note's own ball / Average / How
  many in range (Full at N balls). Heard from, a live Now meter. While the Sound tab is open the zone shows as
  dotted lines in the preview, draggable for Near and Far. Turning one on sets Far 200 px and routes it to Osc A
  and Osc B Gain, −inf..0 dB. Positions come from each physics step and are smoothed to audio rate.
- **Plays** (in Playing): On hit (default), Always (a held drone whose note follows the slots), While a ball is
  in range of a chosen Distance.
- **LFO**: Sine, Triangle, Saw, Square, Random (S&H), Smooth random; Rate in Hz or Tempo (1/32..8 bars, dotted,
  triplet); Rate, Phase, Fade in, Delay, Smooth; Start Restart on hit / Free / Per note.
- **Hit**: Strength, Place on edge (0..1 clockwise from the shape's first corner), Side, Ball number, Random;
  held per note. Low, High, Curve, Smooth; the view shows recent hits as bars.
- **Follow sound** (idea 32): right-click any setting of any thing → Follow sound → a shape → a modulator or its
  Level. The slider shows its range as a band in the shape's colour, and the row joins that modulator's Modulates
  list labelled with the thing's name. Modulators come from physics alone, so a picture export moves the same way
  as the editor even with sound off.

Tests: each modulator's output against hand-computed values; routing add vs scale; loop delay; per-note
independence; distance zones from a scripted ball path; Follow sound moves a Look setting identically in export
and preview.

## Part 5: notes

Canvas board NotesGrid.

- **Key** in the Notes header: Root C..B and Scale (Major, Minor, Dorian, Phrygian, Lydian, Mixolydian, Harmonic
  minor, Melodic minor, Major pentatonic, Minor pentatonic, Blues, Chromatic). In-scale keys get a faint mark.
  Changing the key asks whether to move the slots' notes into the new key.
- **Keyboard** C4..B6 by default, 1..8 octaves with − and +. **Slot grid** above it: one row per slot (1..16,
  + Slot), lined up with the keys. Click in a row toggles a note; clicking a key toggles it in the picked row and
  plays it. Each row shows its chord name (Am, F, C/G). The playing row is outlined during playback. Right-click a
  row: Duplicate, Clear, Delete, Copy notes, Paste notes (into any shape). Drag rows to reorder.
- **Plays**: Random, Shuffle, Up, Down, Up-down, Down-up, Converge, Diverge, Random walk, Order clicked, By ball
  (ball n plays the slot's nth note), By side (side 1 the lowest), All at once with Strum (0..200 ms; Up, Down,
  Random); Repeat each (N hits); Octave range +0..+3.
- **Slots change**: By tempo (every N beats or bars, on the scene's bar clock), By time (every N s), By hits
  (every N hits on this shape), Only on keyframes. Order Forward, Reverse, Ping-pong, Random, Shuffle. A Slot
  keyframe jumps to a chosen slot whatever the mode; slot changes show as marks on the shape's timeline group.
  Ringing notes finish; an Always drone moves to the new notes, gliding when Mono and Glide are on.
- **Chord presets**: about 40 progressions in 8 moods (Pop, Emotional, Epic, Dark, Dreamy, Jazzy, Lo-fi,
  Classic), each shown as numerals and as chords in the shape's key; choosing one fills the slots and sets By
  tempo every 1 bar; voice leading keeps chords close inside the keyboard's range; options Triads, Sevenths,
  Spread, Add bass.

Tests: every play order on a fixed note set and hit sequence gives a golden note list; slot changes on the bar
clock across shapes line up to the sample; voice leading keeps total movement minimal on known progressions;
chord naming on a table of voicings.

## Part 6: buses and the Mixer

Canvas boards MixerStrips and EqEditor.

- **Mixer panel** (left panel, Mixer mode, 420 px): Tempo and Time row, then strips Bus 1, Bus 2, Bus 3, Master.
  A bus strip: on switch and name (double-click renames), layer slots (any number, scroll past five), Sends to the
  other buses, Pan, fader with stereo meter, Mute, Solo, fader value. Buses 2 and 3 start off (dimmed). A shape
  sent to an off bus is silent and its Output warns. Click a slot to open its editor full size in the wide
  inspector (titled with its bus); click the empty slot to add; drag slots to reorder or move between buses;
  right-click Rename, Duplicate, Copy, Paste, Bypass, Delete. Hovering a bus lights its shapes in the tree and
  preview.
- **Routing**: a shape's Output picks its main Bus plus a post-fader Send per other bus (default off). Buses send
  to each other with post-fader Sends; a send that would loop is greyed with a note. Every bus ends at the master.
  Processing order: shapes, buses in send (topological) order, master. Bus knobs and faders take keyframes, no
  modulators; any setting can Follow a bus's level.
- **Bus layers** (stereo, once per bus, each with on switch, Mix and a slot mini view):
  - Filter and Distortion as in part 3.
  - **Equalizer**, 8 bands: Cut, Low shelf, Bell, Notch, High shelf; Frequency, Gain, Q; cut slopes 12/24/48 dB;
    Both / Mid / Side; analyser Post / Pre / Off behind the curve; drag a point (frequency, gain), scroll for Q,
    double-click the curve to add, right-click to turn off; Output. Starts with band 1 a 30 Hz low cut.
  - **Compressor**: Threshold, Ratio, Attack, Release, Knee, Makeup, Mix; gain-reduction meter and level graph.
    Sidechain Self (default), another bus, or Hits on a chosen shape (each hit a short duck shaped by Attack and
    Release); sidechain Low cut and High cut.
  - **Delay**: Standard, Ping-pong, Multi-tap (4 taps: time, level, pan); time in ms or synced (1/64..2 bars,
    dotted, triplet); Feedback; Low cut and High cut with Resonance in the loop; Width; Mix. Character Digital,
    Analog (darkens and softly saturates each repeat), Tape (wow, flutter, saturation, a little noise; pitch bends
    when the time changes).
  - **Reverb**: Plate (Dattorro), Hall (modulated FDN, 8 or 16 lines), Spring (dispersive allpass chain with
    drip). Shared: Pre-delay, Decay, Size, Damping, Low cut, High cut, Width, Mix. Plate: Diffusion, Modulation.
    Hall: Early reflections, Modulation. Spring: Springs (1..3), Tension, Drip. Freeze holds the tail.
  - **Limiter**: Ceiling, Release, Lookahead (fixed 1.5 ms), gain-reduction meter.
  - **Chorus, Phaser, Flanger**: Rate (Hz or synced), Depth, Feedback (Phaser, Flanger), Voices (Chorus 2..4),
    Stages (Phaser 2..12), Width, Mix.
  - **Width**: 0 (mono)..200 %, Mono below (Hz), a correlation meter.
- **Master strip**: limiter ceiling, Momentary / Short-term / Whole clip LUFS, True peak, Input knob, stereo
  meter; click to open the master editor (Input, Output, Ceiling −1 dBTP default, Release, loudness graph with
  the −14 mark, gain reduction).
- **Export** (idea 54): the export window gains a Sound section: Mixed sound (default), plus optional Stems (one
  WAV per bus and the master, 48 kHz 24-bit) and MIDI (one track per shape, the notes it played at the exact hit
  times, with the tempo), saved next to the video.

Tests: EQ and filter curves by sine sweep; compressor static curve and timing; delay times to the sample; reverb
tails decay at the set time and stay bounded at the longest decay; true peak never over the ceiling; send loops
refused; mute silences; solo plays only soloed buses; stems summed equal the mix before the master limiter;
the MIDI file round-trips to the played note list.

## Quality bars (all parts)

- **Aliasing**: oscillators (every wave and warp swept to C8) under −90 dB folded; distortion at 4× under −70 dB.
- **Clicks**: note starts, voice steals and fast knob moves checked for jumps.
- **Same every time**: sound goldens (hashes of rendered blocks for fixed test scenes, blessed like the physics
  goldens, debug and release); editor path equals export path.
- **Speed budget**: a heavy patch (16 voices, 16 unison on both oscillators, a filter and a distortion each) renders
  many times faster than real time in release; a benchmark test prints the real-time factor and fails under 8×.
- **Loudness**: master never over its ceiling as true peak; LUFS matches EBU R128.
- No demo audio unless the owner asks, except the FM/PD A/B clips in part 3.

## Build parts and order

| Part | Builds | Depends on | Notes |
| --- | --- | --- | --- |
| 1 | `bpe-dsp` core blocks used by the master, `bpe-sound` engine skeleton, `Event.at`, sound thread, cpal output, export audio, tempo settings, bars grid, Mixer switch with Master strip | – | Everything stands on it. |
| 2 | `sound.voice` card, starter patch, Sound tab layout B, knob widget, wavetable oscillators, sub, ADSR 1, single-slot notes, Playing, Output, voices | 1 | First thing you can hear. |
| 3 | Chain layers (Modulation, Filter, Distortion), strips, oversampler | 2 | Parallel with 5 and 6. |
| 5 | Key, slot grid, play orders, slot changes, chord presets | 2 | Parallel with 3 and 6. |
| 6 | Buses, all bus effects, sends, sidechain, Mixer strips, master editor, stems and MIDI | 2 | Parallel with 3 and 5. |
| 4 | All modulator kinds, routes, rings, drag to modulate, Plays, Follow sound | 3 | Needs the layer knobs. |

Merge order for the parallel parts: **3, then 5, then 6**; each later one rebases on main. Shared files they all
touch (`bpe-kit/src/sound/mod.rs` registration, `bpe-sound/src/lib.rs`, the Sound tab's `mod.rs`, the scene
format) get only additive changes, each part adding its own module files.

## Things not to do

- No built-in or example scenes or sounds, and no factory patches beyond the starter patch and the chord presets.
- No sound card on any thing kind but Shape yet (built so Balls can have one later).
- No shared sound library, presets or randomize (rework step 2 still holds).
- No node graphs or programmable sound (removed in the strip-down).
- No allocation, locks, logging or file access on the audio path.
- No colours outside `style/` in editor code; rings use the shape's track colour.
- Do not change physics goldens: `Event.at` stays out of the log hash.
- No backward compatibility work for Version 1 sound settings.
