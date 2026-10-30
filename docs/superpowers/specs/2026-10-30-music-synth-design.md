# Chords, synth patches and the mix: design

Date: 2026-10-30. Asked for by Aedan (music producer) in the "Design chords and synth sounds"
thread. Mockups of the chord picker: https://claude.ai/artifact/YbxbzQ7UcHw8mv8sk2bjS5

## 0. What we're building and why

Today the chords are one line of text on Scene defaults ("C 4.2, Dsus4 4, Am 5…", lengths in
seconds) that every Notes module reads. Instruments are a fixed list of 13 built-in sounds, and
only Pluck has knobs. The mix is: notes rendered one-shot, summed, pushed to a loudness target,
then through a hidden peak limiter.

Aedan wants the sound side to be as deep as the visuals, at producer level:

- Chord progressions that each sound module can own or share, not just one on Scene defaults.
- A streamlined, more sophisticated way to pick chords.
- A modular synth for the sounds balls play, with oscillators, filters and envelopes as node
  blocks, shared between modules or owned by one.
- Effects as node blocks, on instruments, channels, return buses and the master.
- Physics driving any synth or effect knob.
- MIDI and stems export, to finish tracks in a DAW.
- A built-in clipper and limiter on the master so levels never get too loud.

**Decisions (all Aedan's picks):** modular synth (C over simple or fixed synth); synths are
library Instruments that modules link, with Make unique; chords work the same way as library
Progressions; chord lengths in bars and beats on the scene tempo; chord lane (mockup A) with the
preset list (mockup B) behind a button; our own DSP blocks, not fundsp; effects are node blocks
too. In this round: note choice per module, effects, physics modulation, MIDI and stems. Later:
sustained pads and basslines from the chords, a mixer view, note quantize, humanise.

**Compatibility:** old scenes need not load or sound the same (Aedan, 2026-10-30). Starter
scenes, presets and golden soundtrack hashes are updated to the new model.

**Success looks like:** Aedan can build a progression in a few clicks, design a patch from
oscillators up, hear physics shaping the sound, keep levels safe without thinking about it, and
take MIDI and stems into a DAW. Everything renders the same every time.

## 1. Chords and note choice

### 1.1 Progressions

A **Progression** is a library item, beside modules:

```
Progression {
    id: ProgressionId,
    name: String,
    key: Key { root: u8 /* pitch class */, mode: Mode },   // Major, Minor, Dorian, Phrygian,
                                                             // Lydian, Mixolydian, Locrian
    meter: (u8, u8),                                         // 4/4 by default
    chords: Vec<Chord>,                                      // repeats when it runs out
}
Chord {
    root: u8,            // pitch class, absolute (so borrowed chords are just chords)
    quality: Quality,    // Major, Minor, Diminished, Augmented
    ext: Extension,      // Triad, Seven, Maj7, Sus2, Sus4, Add9, Nine, Eleven, Thirteen
    bass: Option<u8>,    // pitch class for slash chords and inversions; None = root
    beats: u16,          // length on the scene tempo; 1 beat minimum
}
```

- The tempo comes from Scene defaults (`bpm`; 120 when 0 and no song; the song's tempo when 0 and
  a song is loaded). Chord changes are placed on the **video clock**, like beats (two-clock rule:
  a hit's chord is the chord at the hit's video time).
- Numerals, function colours (home, moving away, pulling home, borrowed) and chord names are
  worked out from the key, never stored.
- Changing the key transposes every chord (roots move with it). Changing the mode keeps roots.

**Where progressions are used.** Scene defaults gets a **Chords** setting that points at one
progression (new scenes get a starter one). Every sound module that picks notes gets a
**Chords** setting with three states:

- *Scene's* (default): uses Scene defaults' progression.
- *Linked*: points at another library progression.
- *Own*: Make unique copies the progression into a new library item only this module uses.

The old `chords` text setting and `bpe_mix::Chords::parse` go away. `pitch_classes` stays as the
chord-name parser for typing a chord by name in the chord editor.

### 1.2 The chord lane (inspector)

Matches mockup A:

- A **key bar**: root, mode, meter, a Play button that auditions the progression through the
  module's own instrument, and a **Presets** button.
- The **lane**: chords as blocks on a bar ruler, width by length. Drag the right edge to resize
  (snaps to beats; Shift snaps to bars). Drag a block to move it. Click to select. Delete key
  removes. The lane scrolls sideways when the progression is longer than the inspector.
- The **palette**: the seven chords of the key as numerals with their names, coloured by
  function. Click to add after the selected chord. A second row shows common borrowed chords
  for the mode (for minor: V, IV, bII; for major: iv, bVI, bVII, II).
- The **chord editor** for the selected block: quality, extension, bass note (root, each chord
  tone, or any note), length (1 beat, 2 beats, 1 bar, 2 bars, or type), and a name field that
  accepts typed chord names ("F#m7b5").
- **Presets** opens the list from mockup B: named progressions filtered by mood, each shown as
  numerals. Picking one fills the lane in the current key (switching mode if the preset needs
  it). At least 20 presets ship.
- The dice roll a preset in the current key, and can also roll the key (the setting is
  `random`).

### 1.3 Chords on the timeline

The timeline gets a **Chords** track showing the progression of the selected thing's sound
module (or Scene defaults'), as coloured blocks with chord names, on the video clock. It is
read-only in this round; editing stays in the inspector.

### 1.4 Note choice

The Notes module (`sound.notes`) gains these settings, with `when` conditions so only the ones
that apply show:

- **Picks from**: chord tones (default), chord + scale tones, scale only.
- **Order**: random (seeded as now), by hit speed, by hit count, arp up, arp down, arp up-down,
  by height in the frame. Arps keep one position per module, advancing on each hit it plays.
- **Plays**: single note (default), full chord, root only (bass).
  - *Full chord above speed*: hits faster than this play the full chord; slower ones play a
    single note. 0 means always the full chord.
  - *Chord spread*: 1 to 3 octaves for the full chord's voicing.
- Low and high note stay. A hit with no candidate notes in range stays silent (as now).

The Melody module keeps working from MIDI files and gets the same Instrument and channel
features below. The Drum machine module gets the channel features; its instrument stays the
Drums built-in.

## 2. Synth patches

### 2.1 Instruments

An **Instrument** is a library item:

```
Instrument {
    id: InstrumentId,
    name: String,
    source: InstrumentSource,
    fx: bpe_graph::Graph,        // the instrument's own effects graph (2.6), In -> Out by default
    voices: VoiceSettings,
}
enum InstrumentSource {
    BuiltIn { which: bpe_mix::Instrument, params: Params },   // the 13 we have; Pluck keeps
                                                              // decay and brightness here
    Patch { graph: bpe_graph::Graph },
}
VoiceSettings {
    note_length: Length,         // Beats(f32) or Seconds(f32): how long the gate stays open
    limit: u16,                  // most voices at once, oldest stolen first; default 16
    mono: bool,                  // one voice; a new note glides from the last
    glide: f32,                  // seconds, mono only
}
```

Every sound module has an **Instrument** setting with the same *Linked* / *Own* behaviour as
Chords (no *Scene's* state: Scene defaults' own Sound module already gives the default). The
list shows built-ins first, then library instruments. Make unique copies the instrument.
`NoteSound.instrument`, `decay` and `brightness` go away; the Pluck's knobs live on its
built-in instrument.

### 2.2 The patch graph

A Patch is a `bpe_graph::Graph` with the **synth catalog**, edited in the existing node panel
under the preview. One graph is one voice. Blocks:

| Group | Blocks |
|---|---|
| Sources | **Oscillator** (sine, triangle, saw, square, pulse with width; polyBLEP anti-aliasing; pitch in semitones and cents; unison 1 to 8 with detune and stereo spread; FM input with amount; hard sync input; phase reset on note or free), **Noise** (white, pink), **Sampler** (a WAV file with root note, or a folder of notes; start, loop) |
| Shaping | **Filter** (state-variable: low, high, band, notch; cutoff, resonance, key tracking, cutoff modulation input), **Ladder** (4-pole low-pass with drive), **Drive** (soft, hard, fold; amount, bias), **Amp** (gain with a modulation input) |
| Modulators | **ADSR** (attack, decay, sustain, release, curve; gated by the note), **LFO** (sine, triangle, saw, square, random; rate in Hz or tempo-synced divisions; retrigger or free; phase), **Sample and hold** (from noise or an input, rate in Hz or synced) |
| Maths | **Mix** (up to 4 inputs with levels), **Add**, **Multiply**, **Scale and offset**, **Crossfade**, **Pitch to Hz** |
| Inputs | **Note** (pitch, velocity, gate, time since note on), physics inputs (section 4), **Macro** (one of the patch's macros) |
| Output | **Voice out** (stereo; mono inputs go to both sides). Exactly one per patch. |
| Effects | every effect block from 2.6 also works inside a patch, where it runs per voice |

Pins carry either **audio** (a 64-sample block per voice) or **control** (one value per block,
smoothed across the block). Control outputs plug into audio inputs and the reverse (an audio
signal used as control takes its block average). Compile errors (no Voice out, a loop with no
delay) show on the block and the patch plays silence, as rules graphs do now.

**Macros**: any block knob can be exposed (`Node.exposed`, as shader graphs already do). Exposed
knobs show in the module card's inspector as sliders under the Instrument row, named
"<block> <knob>" until renamed, and they take keyframes and pulses. A linked instrument's macros
can be overridden per module (marked like other overrides).

**Audition**: a one-octave keyboard strip under the Instrument row plays the patch (through the
instrument's effects, not the channel's), with octave buttons. It plays through the preview
audio output.

### 2.3 Voices

- Each note the module plays starts a voice. The gate is open for **Note length**, then the
  release phase of every ADSR runs. A voice ends once its output stays under -90 dBFS for 50 ms
  after the gate closes, or 10 s after the gate closes, whichever is first.
- Over the voice limit, the oldest voice is stolen with a 5 ms fade.
- Mono mode: one voice; a new note while the gate is open retriggers envelopes only if
  "Retrigger" is on, and glides pitch over Glide seconds.
- Velocity and physics values captured at the hit are fixed for the voice's life (section 4).

### 2.4 Engine

A new crate, **bpe-synth**, holds all DSP and knows nothing about scenes:

- `catalog()`: the synth catalog (a `bpe_graph::Catalog`), shared by patch and effects graphs,
  with each block flagged as voice-only, effect-only or both.
- `compile(graph, Role::Patch | Role::Effects) -> Result<Program, Problems>`: topologically
  sorts the blocks into a flat list of ops over numbered buffers (registers). Feedback loops are
  allowed only through a delay block.
- `Voice` runs a `Program` for one note: `process(&mut self, inputs, out: &mut [f32; 128])`
  (64 stereo frames).
- `Chain` runs an effects `Program` over a stereo stream, block by block.
- `master` (2.7) and `meter` (LUFS and true peak).
- Sample rate is `bpe_mix::SAMPLE_RATE` (48 kHz). Everything is f32 and single-threaded per
  voice or chain, so a render is the same every time on the same machine.

bpe-mix then changes from "render each note one-shot and add it" to "render each **channel**":
gather the channel's notes, run its voices in note order, sum them, run the instrument's
effects, then the channel's effects (section 3). Channels render in parallel on all cores and
are summed in a fixed order. Built-in instruments keep their one-shot cache path inside a
channel, so scenes that only use built-ins stay fast.

### 2.5 Starter patches

About ten, as library instruments any scene can add from a menu: Soft pluck, Glass FM bell,
Supersaw, Warm pad, Sub bass, 808, Chip square, Noise snap, Wobble bass (LFO into the filter),
Marimba-ish (FM). Each exposes 2 to 4 macros.

### 2.6 Effects as blocks

Effect blocks, usable in any effects graph and inside patches:

- **EQ**: 3 bands (low shelf, peak, high shelf) with low and high cut.
- **Filter**: as the patch filter, plus a built-in LFO or tempo-synced sweep.
- **Compressor**: threshold, ratio, attack, release, knee, makeup; sidechain input.
- **Drive**: soft, hard, tube-ish; amount, tone, mix.
- **Bitcrush**: bits, sample-rate reduction, mix.
- **Chorus**: rate, depth, voices, mix.
- **Delay**: time in ms or tempo divisions (straight, dotted, triplet), feedback, ping-pong,
  low and high cut in the feedback, mix.
- **Reverb**: size, decay, damping, pre-delay, width, mix (a Freeverb-style network with
  modulated delay lines).
- **Stereo width**: 0 (mono) to 2.
- **Envelope follower**: an input's level as a control output (attack, release), for
  level-driven modulation inside one graph. Pumping from another channel uses the Channel level
  input (section 4).
- **Clipper** and **Limiter** (as in 2.7).
- **Split / Merge** helpers: Mix and Crossfade from the patch blocks cover parallel and dry/wet
  routing.

Inside a patch, Reverb and Delay show a small "runs on every voice" note on the block.

**Effects graphs** live in four places, each starting as a straight In → Out chain:

1. an Instrument's own effects (`Instrument.fx`), run once over all its voices on a channel;
2. each sound module's **channel** inserts;
3. each **return bus**;
4. the **master**.

**The list view**: when an effects graph is a straight chain (In → one block → … → Out, each
block with one audio in and out, nothing else wired), the inspector shows it as a list: an
"Add effect" menu, drag to reorder, a bypass toggle and the knobs for each. Editing the list
edits the graph, and the reverse. Once the graph is anything other than a straight chain, the list
is replaced by "Custom routing" and an Open nodes button, the same way edited module graphs work
today.

### 2.7 Master output stage

This is always on, after the master effects graph, and can't be removed:

1. **Loudness target**: real integrated loudness (ITU-R BS.1770 K-weighting, gated), default
   -14 LUFS, range -30 to -6. Replaces the RMS percentile in `level.rs`. The whole render is
   measured first, then one gain is applied.
2. **Clipper**: off (default), soft or hard, with drive in dB. Clipping is done 4x oversampled.
3. **Limiter**: true-peak (4x oversampled detection), lookahead 5 ms, ceiling default -1 dBTP
   (range -6 to 0), release in ms, or auto-release that lengthens on dense material. Replaces
   `limit_peaks`.

These live in a **Master** group on Scene defaults. The preview shows a gain-reduction meter and a
short-term LUFS meter beside the transport, and the export window shows the final integrated
LUFS and true peak.

## 3. Channels and buses

**One channel per sound module in the library.** Things linking the same module share its
channel, and Make unique splits it off. Each channel has:

- **Volume** (dB, keyframe, pulse) and **Pan** (-1 to 1, keyframe), added to the per-note pan
  from hit position and stereo width.
- **Inserts**: its effects graph.
- **Sends**: one knob per return bus (dB, -inf by default), post-fader.
- The camera thread's off-shot fade and pan apply per note, before the channel (they depend on
  each hit's place). Coordinate with that thread and merge main before building.

**Signal path**: voices → instrument effects → channel inserts → volume and pan → sends → master
effects graph → master stage (2.7). Return buses: their effects graph → master effects graph.
The song: its own volume and ducking, as now, into the master effects graph.

**Return buses** are a list on Scene defaults (a **Buses** group). New scenes start with two,
"Reverb" (a Reverb block) and "Delay" (a quarter-note ping-pong Delay). You can add, rename and
remove them; each has a volume and an effects graph.

**Tails**: effect tails run past the last note and fade out over the last 0.5 s of the video.

## 4. Physics as modulation

Input blocks in the synth catalog. Each outputs 0 to 1, with **In low / In high** (the physics
range that maps to 0 and 1), **Curve** (linear, exponential, S) and **Smoothing** (ms).

**At the hit** (patch graphs only; fixed for the voice): hit speed, hit angle (relative to the
surface), side hit (top, bottom, left, right as 0, 1/3, 2/3, 1), hit thing's tag (1 when the
other thing has a tag you name), ball size, spin, colour hue, colour brightness, height in the
frame, left-right in the frame, hit count, age.

**Live** (patch graphs only; follows the ball while the voice sounds): speed, height in the frame,
left-right in the frame, spin, distance from the camera centre. The run records these at 60 Hz
for a ball for up to 12 s after each of its hits, only for scenes whose patches use a live input
(`Log.tracks`, keyed by ball and hit). The voice interpolates them to block rate. Once the
recording ends, the last value holds.

**Scene-wide** (any graph; on the video clock): ball count, total motion energy, hits per second
(over a 0.5 s window), beat phase, bar phase, current chord's root and quality, the song's
bass, mid and high levels, and **Channel level** (another channel's level after its inserts,
for sidechain pumping; also feeds the Compressor's sidechain input). Channels that read another
channel's level render after it; a loop between channels is a compile problem on the block.

**Follow… menu**: right-click any macro, instrument knob or effect knob in the inspector and
choose Follow…, then an input. This wires the input block into that knob in the graph, the same
way "Pulse with music" adds a pulse. A followed knob shows a small icon, as pulsing knobs do.

**Rule kept**: sound never feeds back into physics. Physics inputs only read the run's log.

## 5. MIDI and stems export

The export window and batch export get two checkboxes.

**MIDI** (`<name>.mid`, SMF type 1, 960 ticks per quarter, written with `midly`, which we already
use to read MIDI):

- One track per channel, named after its module and the things using it. Notes come from the
  played notes: pitch, velocity from hit strength (1 to 127), length from the voice's gate time.
- Tempo from Scene defaults. Notes sit at their video time, so beat-synced scenes line up with
  the grid.
- Drum machine channels go on MIDI channel 10.
- A **Chords** track with block chords (root position, around middle C) and a marker with the
  chord name at each change.

**Stems** (a folder `<name>-stems/`, 48 kHz 24-bit WAV):

- One file per channel (after inserts and volume, before the master), one per return bus, and
  the song. All start at 0 and run the full length.
- Option: channel stems include their send returns ("wet"), or not ("dry", default).
- Stems are not run through the master stage. The full mix is still exported as normal.

## 6. Preview speed

The background sound job caches each channel's rendered audio keyed by a hash of its notes,
instrument, effects graphs and keyed settings. An edit to one channel re-renders that channel,
the buses and the master only. The keyboard strip and chord-lane Play render on the fly through
the same engine with their own small buffer.

## 7. Errors and limits

- A patch or effects graph that fails to compile plays silence (patch) or passes audio through
  (effects), shows the problem on the block, and adds a line to the status bar, as rules do.
- Non-finite samples from any block are replaced with 0 and the voice is ended; the status
  line says which instrument did it.
- The voice limit applies per channel. A hard cap of 256 voices sounding across all channels
  steals the oldest across the whole mix.
- Missing sample files play silence with a status line, as now.

## 8. Testing

- **Blocks**: oscillator pitch within 1 cent; saw and square aliasing below -60 dB at 5 kHz;
  filter response at cutoff within 1 dB of the design; ADSR stage timing within one block;
  LFO sync to tempo; delay time exact to the sample; reverb tail decays to -60 dB within the
  set decay ±10%.
- **Compile**: topological order, loops without delay rejected, list view round-trips to the
  same graph.
- **Master**: true peak never above the ceiling on a test set (including square waves and
  inter-sample peaks); integrated LUFS within 0.5 LU of the target on reference material.
- **Determinism**: every starter patch and every starter scene renders to the same hash twice;
  golden soundtrack hashes re-blessed for the new model (Windows run once, as before).
- **Physics inputs**: a ball falling through a filter-follows-height patch closes the filter
  monotonically.
- **Export**: a MIDI round trip through our reader gives the played notes back; stems summed
  through the master stage match the full mix within -60 dB.
- **Performance**: a 60 s scene with 2,000 notes on a 3-oscillator unison patch with filter, two
  ADSRs, channel reverb and delay renders in under 4 s on 8 cores (release build). Preview
  re-render after changing one channel's knob is under 1 s for that scene.
- **Inspector**: kittest checks for the chord lane (add, resize, delete, preset), the list view,
  and Follow….

## 9. Build order

Each part merges on its own, with the app working at the end of each.

1. **Progressions and note choice**: Progression library items, the Chords setting on modules
   and Scene defaults, the chord lane, palette, chord editor, presets, the timeline Chords
   track, note choice settings, dice. Removes the chords text.
2. **Synth engine and Instruments**: bpe-synth (blocks, catalog, compile, Voice), Instrument
   library items with built-ins and patches, the patch graph in the node panel, macros, keyboard
   strip, voices, starter patches, channel rendering in bpe-mix.
3. **Effects, channels, buses and master**: effect blocks, effects graphs in four places, the
   list view, channel volume/pan/sends, return buses, the master stage with real LUFS, clipper
   and limiter, meters, tails.
4. **Physics inputs**: input blocks, live tracks in the log, the Follow… menu.
5. **MIDI and stems export**: export window and batch options.

Before part 1: merge main, including the camera thread's mixer changes.
