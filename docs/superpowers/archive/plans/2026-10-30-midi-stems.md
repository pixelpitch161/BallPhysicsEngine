# MIDI and Stems Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Video export (single and batch) can also write a type 1 MIDI file of the played notes and chords, and a folder of 24-bit WAV stems: one per channel, one per return bus, and the song.

**Architecture:** bpe-mix gains `export.rs`. It holds the MIDI writer, which takes the played notes and the chords and produces bytes. It also holds the stem builder, which uses the premaster's per-channel and per-bus audio, now kept on `Soundtrack`, and a 24-bit WAV writer. bpe-kit gathers what a scene needs: track names, the song stem and the tempo. bpe-cli's export writes the files next to the video. The studio's export and batch windows get checkboxes.

**Tech Stack:** Rust, midly 0.5 (already in bpe-music, now also used by bpe-mix), hound, egui and kittest.

**Spec:** `docs/superpowers/specs/2026-10-30-music-synth-design.md` §5, plus the §8 export tests.

## Global Constraints

- MIDI: `<name>.mid`, SMF type 1, 960 ticks per quarter, written with `midly`.
  - One track per channel, named after its module and the things using it.
  - Pitch comes from the played note. Velocity comes from hit strength, mapped to 1 to 127. Length is the voice's gate time.
  - Tempo comes from Scene defaults. Notes sit at their video time.
  - Drum machine channels go on MIDI channel 10, which is index 9.
  - A **Chords** track holds block chords (root position, around middle C), with a marker carrying the chord name at each change.
- Stems: a folder `<name>-stems/`, 48 kHz 24-bit WAV.
  - One file per channel (after inserts and volume, before the master), one per return bus, and one for the song.
  - All start at 0 and run the full length.
  - Option: channel stems include their send returns ("wet"), or not ("dry", the default).
  - Stems are not run through the master stage. The full mix is still exported as normal.
- Tests (§8):
  - A MIDI round trip through our reader gives the played notes back.
  - Stems summed through the master stage match the full mix within -60 dB.
- Plain words in the UI. No ♭/♯/▶ glyphs.
- Build with `CARGO_INCREMENTAL=0`.

## Design decisions (the spec is silent)

1. **Tempo** = 60 / `MixSettings::seconds_per_beat()` (the beat grid's middle gap), written as one tempo event at tick 0. Seconds become ticks with `round(seconds × bpm / 60 × 960)`.
2. **Gate times:**
   - Patch instruments use `voices.note_length.frames(spb)`.
   - The built-in pluck uses its decay (clamped 0.05 to 2 s).
   - Drums use 0.1 s.
   - Other built-ins use 0.25 s.
3. **Velocity** = `round(1 + 126 × velocity.clamp(0, 1))`.
4. **Track names:** "<module name> (<thing names joined by ", ">)". With no things, just the module name. Channels are in module id order.
5. **MIDI channels:** the first non-drum channel is MIDI channel 0, the next 1, and so on, skipping 9 and wrapping at 16. Drum channels use 9.
6. **Chords track:** Scene defaults' progression on the beat grid (`ChordClock`). Each change gets a Marker meta event with `chord_name(chord, key)` and the chord's `voicing(chord, 48, 72, 0)` notes, velocity 80, lasting until the next change or the end.
7. **Stem names:** `NN <name>.wav` with a two-digit order. Channels come first, then buses ("<bus> bus"), then "Song". Names go through the same safe-file-name rule as videos.
8. **Wet stems** add each of the channel's sends, run alone through that bus's effects and the bus volume. Bus stems are always the buses as mixed: what every channel sends them, through their effects and volume.
9. **The song stem** is the song mixed alone into silence, with the scene's song offset, volume and loudness. Ducking is left off, because ducking reads the notes and belongs in the mix.

## Review Focus

1. A scene with no sound modules and no song: export with stems on must still write an empty folder or just a silent song stem, and never fail. Test `stems_of_a_silent_scene`.
2. Two channels with the same module name: their stem files must not overwrite each other. The NN prefix keeps them apart. Test `same_names_keep_both_stems`.
3. Notes past the end of the video, or with a gate longer than the video, must be cut at the end. Test `midi_notes_stop_at_the_end`.
4. Batch rejects must move or delete the `.mid` and the `-stems` folder with the video. Test `rejects_take_their_extras`.
5. Export cancelled midway must not leave a half-written stems folder pretending to be complete. Extras are written only after the video succeeds. Test `cancelled_export_writes_no_extras`.

---

### Task 1: MIDI writer (bpe-mix)

**Files:**
- Create: `crates/bpe-mix/src/export.rs`
- Modify: `crates/bpe-mix/Cargo.toml` (midly; dev-dep bpe-music), `crates/bpe-mix/src/lib.rs` (mod and pub use)

**Interfaces:**
- Produces:
  - `pub struct MidiTrackInfo { pub channel: u32, pub name: String }`
  - `pub fn midi_file(settings: &MixSettings, notes: &[Note], names: &BTreeMap<u32, String>) -> Vec<u8>`. `names` maps channel to track name; a channel missing from it is named from `settings.channel_names`.
  - `pub fn gate_seconds(settings: &MixSettings, note: &Note) -> f32`
- [ ] **Step 1: Failing tests:**
  - `midi_round_trip_gives_the_notes_back`: three notes on two channels plus one drum note. Parse them with `bpe_music::midi::parse`. Times are within 1 ms, keys are equal, velocities equal the mapped values, durations equal the gates (±1 ms), and the drum part has `channel == 9`.
  - `midi_has_a_chords_track_with_markers`: parse the raw bytes with midly. A track named "Chords" has one Marker per chord change, and its notes are each chord's voicing.
  - `midi_notes_stop_at_the_end`
  - `midi_tempo_follows_the_beat_grid`: beats every 0.5 s give tempo 500000 µs per quarter.
- [ ] **Step 2: Run** `cargo test -p bpe-mix export`. Expected: FAIL.
- [ ] **Step 3: Implement.** Per channel, the events are NoteOn and NoteOff, sorted by tick, with NoteOff before NoteOn on the same tick. Each track has a TrackName first and an EndOfTrack last. Track 0 holds the tempo and the time signature (4/4).
- [ ] **Step 4: Run.** Expected: pass.
- [ ] **Step 5: Commit** `feat(mix): MIDI export of the played notes and chords`.

### Task 2: Stems (bpe-mix)

**Files:**
- Modify: `crates/bpe-mix/src/buses.rs` (Premaster keeps `channels: BTreeMap<u32, Arc<Vec<f32>>>` and `buses: Vec<(BusId, Arc<Vec<f32>>)>`, the bus output before volume), `lib.rs` (Soundtrack gains `pub channels`, `pub buses` with the same types; `mix_premaster` fills them), `export.rs`

**Interfaces:**
- Produces:
  - `pub struct Stem { pub name: String, pub samples: Vec<f32> }`
  - `pub fn stems(settings: &MixSettings, track: &Soundtrack, physics: &PhysicsInputs, wet: bool) -> Vec<Stem>`: channels (named after the module), then buses (`"<name> bus"`, after volume)
  - `pub fn write_wav24(path: &Path, samples: &[f32]) -> anyhow::Result<()>`
  - `pub fn stem_file_names(stems: &[Stem]) -> Vec<String>` (decision 7)
- [ ] **Step 1: Failing tests:**
  - `stems_sum_to_the_premaster`: a scene with two channels, a send to a reverb bus at -6 dB, and an insert. The dry stems plus the bus stems equal `track.samples` within 1e-5.
  - `wet_stems_hold_their_sends`: a wet channel stem minus the dry one equals that channel's send run through the bus, within 1e-5.
  - `same_names_keep_both_stems`
  - `wav24_round_trips`: write, then read back with hound. 24 bits, 2 channels, 48 kHz, and samples within 1/2^23.
- [ ] **Step 2: Run** `cargo test -p bpe-mix stems wav24`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-mix`. Expected: pass, with golden mixes unchanged.
- [ ] **Step 5: Commit** `feat(mix): channel and bus stems`.

### Task 3: Scene extras (bpe-kit)

**Files:**
- Create: `crates/bpe-kit/src/music/extras.rs`
- Modify: `crates/bpe-kit/src/lib.rs`, `music/mod.rs`, `music/song_mix.rs` if needed

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `pub struct Extras { pub midi: bool, pub stems: bool, pub wet: bool }` (Default all false, Serialize/Deserialize, Copy)
  - `pub struct SceneExport { pub soundtrack: Soundtrack, pub midi: Option<Vec<u8>>, pub stems: Vec<Stem> }`
  - `pub fn mix_scene_export(scene, resolved, log, kit, extras: Extras) -> SceneExport`: one mix. The stems come from the premaster. The song stem goes after the buses when a song loads. Then song, master fx and master give the soundtrack, the same as `mix_scene`.
  - `pub fn write_extras(export: &SceneExport, base: &Path) -> anyhow::Result<Vec<PathBuf>>`: `<base>.mid`, plus `<base>-stems/` filled after removing any old stems folder.
- [ ] **Step 1: Failing tests:**
  - `export_soundtrack_matches_mix_scene`: same samples as `mix_scene`.
  - `stems_through_the_master_match_the_mix`: dry stems and song summed, then `run_master_fx` and `master`, match `soundtrack.samples` within -60 dB (max abs diff ≤ 0.001). This is spec §8.
  - `midi_names_tracks_after_modules_and_things`
  - `stems_of_a_silent_scene`
  - `write_extras_writes_the_files`: in a temp dir, the `.mid` parses and each stem file reads with hound.
- [ ] **Step 2: Run** `cargo test -p bpe-kit extras`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-kit`. Expected: pass.
- [ ] **Step 5: Commit** `feat(kit): MIDI and stems for a scene`.

### Task 4: Export writes them (bpe-cli)

**Files:**
- Modify: `crates/bpe-cli/src/lib.rs` (ExportOptions `extras: Extras`, `extras_base: Option<PathBuf>`), `main.rs` (`--midi`, `--stems`, `--wet-stems`), `batch.rs` (BatchSpec `#[serde(default)] extras`; base = folder/name stem; `pick` moves or deletes `.mid` and `-stems/`)

**Interfaces:**
- Produces: `ExportOptions::extras_path(&self) -> PathBuf` (`extras_base` or the output without extension). `export_video_reporting` returns its written extras in a new `ExportReport { encoder, master: Option<MasterReport>, extras: Vec<PathBuf> }`, and the old tuple callers are updated.
- [ ] **Step 1: Failing tests:**
  - `options_name_extras_after_the_video`
  - `rejects_take_their_extras`, which calls `pick` on a folder with fake files
  - `cancelled_export_writes_no_extras`: the extras are written after ffmpeg finishes. Test through a helper `finish_extras(ok: bool, ..)`.
- [ ] **Step 2: Run** `cargo test -p bpe-cli`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
- [ ] **Step 5: Commit** `feat(cli): --midi and --stems on export and batch`.

### Task 5: Checkboxes (bpe-studio)

**Files:**
- Modify: `crates/bpe-studio/src/export_window.rs`, `batch_window.rs`

**Interfaces:**
- Produces:
  - Export window rows: "MIDI file" (hover: "Also save the notes and chords as a .mid next to the video"), "Stems" (hover: "Also save each sound, bus and the song as its own WAV in a folder"), and when Stems is on, "Stems include sends" (hover: "Each sound's stem includes its reverb and delay sends").
  - The finished line lists what was also saved, for example "Also saved MIDI and 5 stems".
  - Batch window has the same three rows on its spec.
- [ ] **Step 1: Failing kittests:** `export_offers_midi_and_stems` (ticking them sets the options passed to the job, checked through a `fn options(&self, path) -> ExportOptions` helper) and `batch_offers_midi_and_stems`.
- [ ] **Step 2: Run** `cargo test -p bpe-studio --bin ball-studio-next export batch`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p bpe-studio --bin ball-studio-next`. Expected: pass.
- [ ] **Step 5: Commit** `feat(studio): MIDI and stems checkboxes on export`.

### Task 6: Demo, report

- [ ] Render height-sweep with `--midi --stems` at half size, check the files, and copy a screenshot of the export window with the boxes ticked into `/mnt/project-files/ballphysics/music-synth/part5/` along with the `.mid`, the stems folder and REPORT.md.
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings` and the full `cargo test --workspace` (debug). Expected: clean and green.
- [ ] Commit any repo changes: `docs: part 5 report`.
