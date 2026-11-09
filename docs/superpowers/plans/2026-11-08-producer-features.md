# W11: producer features (samples, backing track, saved sounds, MIDI import)

Workstream W11 of the 2026-11-08 improvement review ("Ball Studio code review: what to improve"), findings A6, A9
and A10. Aedan picked everything but the Size section on 2026-11-08 and approved A9 (saved sounds) though he had
earlier said no presets, so the review's suggestions are the approved design:

- **A6** "Add a Sampler oscillator (WAV, FLAC or MP3 with root note, start, loop and pitch by note), and a
  'Backing track' on the Mixer with an offset that plays and exports with the mix."
- **A9** "Add 'Save sound…' and 'Load sound…' (the card, its layers and routes, saved as a small file)."
- **A10** "Add 'Import MIDI into slots', where each chord becomes a slot and the tempo comes across too. Live MIDI
  keyboard input could come later."

This file is the design and the plan. W10 (balls make sound) is merged. Rule for the whole part: a scene that uses
none of this sounds and looks exactly as before. The physics goldens and the sound goldens must not move.

## Design decisions

### Where sound files live

- A sound file the user loads (a sample or a song) is **kept inside the scene** as its bytes, like pictures and fonts
  (`Scene.audio`, `bpe-core/src/scene_audio.rs`, base64 in the file, left out when empty, pruned on save when no
  `sample` or `backing` setting names it). Moving or deleting the file never loses the sound, and a scene opens the
  same on any PC. Same bytes added twice keep one copy under one name; a clash of names gets "name (2)".
- Copying things (W5 clipboard) carries the audio their settings name, as it carries fonts and pictures.
- Readable formats: **WAV, FLAC, MP3, OGG Vorbis and AIFF**, decoded with `symphonia` (pure Rust, so the MinGW build is
  unaffected), gapless on (MP3 encoder delay trimmed, so a song lines up). Files over 200 MB are refused.
- Everything is decoded **once** to 48 kHz float stereo (mono doubled, more than two channels: the first two),
  resampled with `rubato` (already a dependency), and kept in a shared cache, `bpe_sound::SampleBank`, built like the
  `WaveBank`: `get` decodes there and then (export, tests), `ready` returns what is decoded and starts a background
  decode otherwise (the editor, which must never wait). The cache is keyed by the bytes' `Arc` (same allocation),
  checked through a `Weak`, so a scene's clones share one decode and dropped audio frees it. `decoded()` counts
  finished decodes so the editor rebuilds the patch when one lands (beside `WaveBank::baked`).
- Each decode also keeps **peaks** (largest absolute value of either channel per 64 frames) for the waveform views.

### A6a: the Sampler (a sixth wave, "Sample")

- `Wave::Sample` joins Sine, Triangle, Saw, Square and Noise on Osc A and B. New `OscSettings` fields (prefixed
  `a_`/`b_` like the rest, none keyframed: they are read when the patch is built):

  | Key | Label | Type, range | Default | Shown when |
  | --- | --- | --- | --- | --- |
  | `sample` | Sample | text (an audio name the scene keeps) | "" | wave=sample |
  | `sample_root` | Root note | int 0..127 | 60 (C4) | wave=sample |
  | `sample_follow` | Follow notes | bool | on | wave=sample |
  | `sample_start` | Start | 0..100 % | 0 | wave=sample |
  | `sample_loop` | Loop | Off / Forward / Ping-pong | Off | wave=sample |
  | `sample_loop_start` | Loop start | 0..100 % | 0 | wave=sample and sample_loop!=off |
  | `sample_loop_end` | Loop end | 0..100 % | 100 | wave=sample and sample_loop!=off |
  | `sample_loop_fade` | Loop fade | 0..1000 ms | 0 | wave=sample and sample_loop=forward |
  | `sample_reverse` | Reverse | bool | off | wave=sample |

- Warp, Warp mode, Unison (and Detune, Spread, Blend, Random phase), Phase, Phase mode and Sub don't apply to a
  sample: their `when` gains `wave!=sample` and the Sound tab hides them. Gain, Level, Octave, Semi, Cents and Pan
  work as on any wave, and so do the chain layers (filter, distortion, EQ, and FM/PD/AM/ring from the other
  oscillator, which may itself be a sample).
- **Pitch**: with Follow notes on, a note plays the sample at `2^((note + octave·12 + semi + cents/100 - root)/12)`
  times its speed; off, every note plays at `2^((octave·12 + semi + cents/100)/12)` (drum one-shots). Glide and pitch
  routes move it per block, as they move a wave.
- **Playback** (`bpe-dsp/src/sampler.rs`: `SampleData`, `SamplerVoice`, `SamplerParams`): 4-point Hermite
  interpolation. For samples up to 30 s, octave-down copies (mip levels, halfband-filtered, up to 4) are made at
  decode, and a block reads the level that keeps its own step at most 2^0.6 (about a fifth), so pitching up barely aliases and keeps its top end.
  Start is where each note begins. Loop Off plays once and goes silent at the end; Forward jumps from Loop end
  back to Loop start (with Loop fade, the last stretch before Loop end crossfades, equal power, into the stretch
  before Loop start, so the seam doesn't click); Ping-pong runs back and forth. A loop is entered only by playing
  into it (Start before Loop end). Reverse plays the sample back to front (Start, the loop points and the fade are
  counted on the reversed sample, so 0 % is the old end). Loop end at or before Loop start plays as Off.
- No allocation while playing: the note keeps a position and a direction; the data is an `Arc` in the spec.
- A sample that is missing or still decoding plays silence (the patch is rebuilt when the decode lands).
- **Editor**: the wave chips gain "Sample". With Sample picked the panel shows the sample's name and **Load…**
  (WAV, FLAC, MP3, OGG, AIFF), a note when it can't be found or read, the sample's waveform with the Start (a line)
  and loop (a shaded span) marked, the knobs Gain, Level, Octave, Semi, Cents, Pan, then Root (shown as a note name),
  Start, Loop start, Loop end, Loop fade, the Loop chips and the Follow notes and Reverse switches. Loading a file
  whose name ends in a note name ("Piano C3.wav", "Pad_F#2.flac") sets Root to it.

### A6b: the backing track

- Four Mixer card settings, group "Backing track": `backing` (text: the song's audio name, "" for none), `backing_on`
  ("On", default on), `backing_at` ("Starts at", seconds -600..600, span -30..30, default 0: where in the video the
  song's first sample plays; below 0 starts partway into the song) and `backing_level` ("Level", dB -60..12, default
  0). Not keyframed.
- The engine adds the song to the mix **before the Master** at the voices' sound time (the same clock the hits use),
  so it goes through Input, the limiter, the Ceiling and End fade, and stays lined up with hits through every seek,
  the speakers' delay compensation and the export. It is silent while stopped (keys clicked by hand still sound) and
  after its end. Not in any bus stem (the bus stems add up to the mix less the song); it is in the Master stem.
- `has_sound` is true for a scene with a backing track that is on and kept, so an export with only a song still has
  sound. A scene whose song is missing exports silence for it.
- **Editor**: a "Backing track" row under the Mixer's tempo row: the song's name and length, or "None"; **Load
  song…**; × to remove it; On; Starts at; Level. A sound file dropped on the window becomes the backing track (a scene
  had no other use for one). The timeline gets a **Song** lane under the Hits lane while there is a backing track: its
  waveform at its place in time, so balls can be lined up with the beat by eye.

### A9: saved sounds (`.ballsound`)

- **Save sound…** writes the Sound card as `<name>.ballsound`, a RON file: `(format: 1, card: <the card, layers
  numbered from 0 as Copy sound numbers them>, audio: {<name>: <base64>})`, the audio being only the samples its
  oscillators name. Keyframes, Follow-sound links and the card's place in the scene are not saved (a sound is its
  settings).
- **Load sound…** reads one and puts it on the thing in place of its Sound card, as Paste sound does (keys on the old
  card go with it), one undo step. Its samples join the scene's audio, renamed on a clash. A file that isn't a
  sound, or a newer format, is reported in the status line and changes nothing.
- Both sit in the Enable sound row (right side) and in its right-click menu beside Copy and Paste sound. Load sound
  works on a thing without a Sound card too (it adds one).
- Copy sound / Paste sound now carry their samples too (`bpe_core::SoundClip { card, audio }`), so pasting into a new
  scene keeps them.

### A10: Import MIDI into slots

- **Import MIDI…** in the Notes header (beside Chord presets) reads a `.mid` file (format 0 or 1, ticks per quarter
  timing; SMPTE timing is refused with a message) with `midly` (already a dependency), in `bpe-sound/src/music/
  midi_import.rs` (pure, tested):
  - Notes from every channel except the drum channel (10), unless it is the only one with notes. A note-on of
    velocity 0 is a note-off.
  - A **chord** is the notes starting within a 32nd note of the first one; it lasts until the next chord starts (the
    last until its last note ends). Single notes are one-note chords.
  - The distinct chords (by their set of notes), in the order they first play, fill the slots (at most 16; any more
    are left out and the message says how many).
  - The **tempo** comes across: the file's first tempo becomes the scene's Tempo (clamped 20..300), its first time
    signature Beats in a bar and Beat note (bottom 2, 4, 8 or 16; anything else keeps the scene's). Later tempo
    changes are followed for the chords' times but the scene keeps one tempo (the message says so).
  - **When the slots change**: if every chord lasts the same whole number of beats, the first starts at the file's
    start and the chords run through the slots in order (repeating is fine), Slots change becomes By tempo every that
    many bars (whole bars) or beats; otherwise it becomes Only on keyframes and each chord gets a Jump to slot key at
    its time (from First beat at), replacing the old Jump keys. Order becomes Forward.
  - The keyboard range widens to show every imported note.
- One undo step. The Sound tab shows a short line under the Notes header afterwards: "Imported 12 chords into 4
  slots from song.mid." (plus what was left out).

## Things not to do

- No live MIDI keyboard, no sample slicing or multi-sample key zones, no time-stretching, no keyframes on the sample
  settings, no per-bus backing track. (Later work.)
- Don't decode on the editor's UI thread for playing: only the quick format check (`probe`) runs there; decodes run
  on the "sound file decode" thread.
- Don't change any golden; don't add built-in scenes or sample files to the program.

## Steps and verification

1. **Deps**: `symphonia` (default-features off; `wav`, `pcm`, `flac`, `mp3`, `ogg`, `vorbis`, `aiff`) and `rubato` in
   `bpe-sound`. Check the Windows cross-build at the end.
2. **bpe-dsp `sampler.rs`**: `SampleData::new(l, r, mips)` (levels, peaks), `SamplerVoice::{note_on, render}`.
   Tests: a sine sample played at step 1 matches the input; step 2 doubles the frequency (zero crossings); a one-shot
   ends silent; Forward loop keeps sounding with period = loop length; ping-pong turns; reverse plays back to front;
   mips exist only for short samples and a pitched-up note reads a higher level.
3. **bpe-core `scene_audio.rs`**: `SceneAudio`, `Scene.audio`, `add_audio`, `AUDIO_KEYS`; pruning on save (pictures
   and audio together); `ThingClip.audio`; `SoundClip`. Tests: round trip, saving without audio as before, pruning,
   clip carries and renames.
4. **bpe-sound `audio.rs`**: `probe`, `decode`, `SampleBank`; `guess_root`. Tests: WAV (hound) at 44.1 kHz decodes to
   48 kHz with the right length and pitch; small FLAC, MP3 and OGG fixtures in `tests/data/` decode; garbage is an
   error; `ready` then `decoded()` counts up; the cache shares one decode.
5. **Patch and voice**: `OscSpec.sample` (+ params), `Tables` reads samples the same way (`Skip` none);
   `ShapeVoice` renders `Wave::Sample` with `SamplerVoice` per note per osc; `patch_fingerprint` hashes the audio
   names and lengths. Test (`tests/sampler.rs`): a shape with a sine-sample oscillator plays the hit's note at the
   right pitch (Follow notes on and off), Start skips in, the alloc test covers a sample voice.
6. **Backing track**: `MixerSettings.backing*`, `SoundPatch.backing`, `Engine` adds it in `render_voices`; `has_sound`.
   Tests: the song comes out at exactly `backing_at` (render_samples, no voices and with voices), Level scales it, Off
   silences it, a seek lands in the right place of the song, a negative Starts at skips into it, Master stem has it
   and bus stems don't, alloc test with a backing track.
7. **A9**: `bpe-studio/src/sound_tab/sound_file.rs` (`to_text`, `from_text`), TabEdits `SaveSound`/`LoadSound`,
   buttons. Tests: round trip of a card with layers, routes and a sample; bad text is an error; kittest: the buttons
   are there and Load sound replaces the card.
8. **A10**: `midi_import.rs` and the Notes header button, `TabEdit::ImportMidi`. Tests: a uniform progression becomes
   By tempo bars; uneven chords become Jump keys at the right seconds; repeated chords share slots; more than 16 are
   capped and counted; drums skipped unless alone; tempo and time signature come across; SMPTE refused.
9. **Editor**: Sample panel, Mixer backing row, Song lane, drops. Kittest: picking Sample shows Load…; the Mixer has
   the Backing track row; a dropped WAV becomes the backing track.
10. Goldens unchanged (debug and release), clippy `-D warnings`, fmt, the workspace tests with two threads, the
    Windows cross-build. As-built notes in `docs/notes/sound.md` and `editor.md`.

## As built (2026-11-08)

Everything above landed, with these differences:

- The A9 code is in `bpe-studio/src/sound_tab/write.rs` (`copy_clip`, `paste_clip`, `save_sound`, `load_sound`,
  `load_sound_text`) beside Copy and Paste sound, not a `sound_file.rs`; the file text itself is
  `bpe_core::{sound_file_text, read_sound_file}`.
- What a file action did (saved, loaded, imported, or why not) shows as one line under the Enable sound row, for
  that thing only (`SoundTab.note`, a `TabNote`), not under the Notes header.
- `bpe_sound::audio::sine_wav` makes the test WAVs for every crate's tests.
- The alloc test covers two sample oscillators (Forward loop with a fade; Ping-pong, Reverse, from 40 %) playing
  chords, with a backing track. `tests/sampler.rs` also checks the song is in the mix and in no bus stem.
- The as-built summary is in `docs/notes/sound.md` and `docs/notes/editor.md` ("W11").
