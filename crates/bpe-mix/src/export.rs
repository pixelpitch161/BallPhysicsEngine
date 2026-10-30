//! Exports beside the video: the played notes and the chords as a MIDI file, and each channel,
//! bus and the song as its own WAV stem.

use crate::buses::{add, db_to_gain, fx_ctx, run_fx};
use crate::chords::{ChordClock, chord_name, voicing};
use crate::{
    Instrument, InstrumentChoice, MixSettings, Note, NoteInstrument, PhysicsInputs, SAMPLE_RATE, Soundtrack,
    instrument_for,
};
use bpe_core::ProgressionRef;
use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// MIDI ticks per quarter note.
pub const TICKS_PER_QUARTER: u16 = 960;
/// The MIDI channel drums play on (10, counting from 1).
const DRUM_CHANNEL: u8 = 9;

/// Whether a note plays the drum machine.
fn is_drums(settings: &MixSettings, n: &Note) -> bool {
    match n.instrument {
        NoteInstrument::BuiltIn(which) => which == Instrument::Drums,
        NoteInstrument::Library(id) => {
            matches!(
                instrument_for(settings, id, n.channel),
                InstrumentChoice::BuiltIn { which: Instrument::Drums, .. }
            )
        }
    }
}

/// How long a note is held, in seconds: a patch's note length, a pluck's decay, a drum hit's
/// tenth of a second, a quarter second for the other built-ins.
pub fn gate_seconds(settings: &MixSettings, note: &Note) -> f32 {
    let built_in = |which: Instrument, decay: f32| match which {
        Instrument::Drums => 0.1,
        Instrument::Pluck => decay.clamp(0.05, 2.0),
        _ => 0.25,
    };
    match note.instrument {
        NoteInstrument::BuiltIn(which) => built_in(which, 0.9),
        NoteInstrument::Library(id) => match instrument_for(settings, id, note.channel) {
            InstrumentChoice::Patch { voices, .. } => {
                voices.note_length.frames(settings.seconds_per_beat(), SAMPLE_RATE as f32) as f32 / SAMPLE_RATE as f32
            }
            InstrumentChoice::BuiltIn { which, decay, .. } => built_in(which, decay),
        },
    }
}

/// 1 to 127.
fn midi_velocity(v: f32) -> u8 {
    (1.0 + 126.0 * if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }).round() as u8
}

/// One event on a track, at an absolute tick. Ends sort before starts on the same tick.
#[derive(Clone, Debug)]
struct At<'a> {
    tick: u64,
    order: u8,
    kind: TrackEventKind<'a>,
}

fn note(channel: u8, key: u8, velocity: u8, on: bool) -> TrackEventKind<'static> {
    let (key, vel) = (u7::new(key.min(127)), u7::new(velocity.min(127)));
    let message = if on { MidiMessage::NoteOn { key, vel } } else { MidiMessage::NoteOff { key, vel: u7::new(0) } };
    TrackEventKind::Midi { channel: u4::new(channel), message }
}

/// Absolute-tick events into a track: a name first, in tick order, an end last.
fn track<'a>(name: &'a str, mut events: Vec<At<'a>>) -> Vec<TrackEvent<'a>> {
    events.sort_by_key(|e| (e.tick, e.order));
    let mut out =
        vec![TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())) }];
    let mut last = 0u64;
    for e in events {
        let delta = e.tick.saturating_sub(last).min(0x0FFF_FFFF) as u32;
        last = e.tick;
        out.push(TrackEvent { delta: u28::new(delta), kind: e.kind });
    }
    out.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
    out
}

/// The played notes and Scene defaults' chords as a type 1 MIDI file: a tempo track, one track
/// per channel named from `names` (else the module's name), and a Chords track.
pub fn midi_file(settings: &MixSettings, notes: &[Note], names: &BTreeMap<u32, String>) -> Vec<u8> {
    let spb = settings.seconds_per_beat().max(1e-3) as f64;
    let ticks = |seconds: f64| (seconds.max(0.0) / spb * TICKS_PER_QUARTER as f64).round() as u64;
    let end = ticks(settings.duration as f64);

    let mut channels: BTreeMap<u32, Vec<&Note>> = BTreeMap::new();
    for n in notes {
        channels.entry(n.channel).or_default().push(n);
    }
    let mut names_owned: Vec<String> = Vec::new();
    let mut tracks_events: Vec<Vec<At>> = Vec::new();
    let mut next_channel = 0u8;
    for (ch, list) in &channels {
        let drums = list.iter().any(|n| is_drums(settings, n));
        let midi_channel = if drums {
            DRUM_CHANNEL
        } else {
            let c = next_channel;
            next_channel = (next_channel + 1) % 16;
            if next_channel == DRUM_CHANNEL {
                next_channel += 1;
            }
            c
        };
        let mut events = Vec::new();
        for n in list {
            let seconds = n.frame as f64 / SAMPLE_RATE as f64;
            let on = ticks(seconds);
            if on >= end {
                continue;
            }
            let off = ticks(seconds + gate_seconds(settings, n) as f64).clamp(on + 1, end.max(on + 1));
            let v = midi_velocity(n.velocity);
            events.push(At { tick: on, order: 1, kind: note(midi_channel, n.key, v, true) });
            events.push(At { tick: off, order: 0, kind: note(midi_channel, n.key, 0, false) });
        }
        names_owned.push(
            names
                .get(ch)
                .or_else(|| settings.channel_names.get(ch))
                .cloned()
                .unwrap_or_else(|| format!("Channel {ch}")),
        );
        tracks_events.push(events);
    }

    // Chords: a marker and the chord voiced around middle C at each change.
    let progression = settings.progression(ProgressionRef(None));
    let spans = ChordClock::new(progression, &settings.beats).spans(settings.duration as f64);
    let chord_names: Vec<String> = spans.iter().map(|(_, _, c)| chord_name(c, progression.key)).collect();
    let mut chord_events = Vec::new();
    for (i, (start, stop, c)) in spans.iter().enumerate() {
        let (on, off) = (ticks(*start), ticks(*stop));
        if on >= end || off <= on {
            continue;
        }
        chord_events.push(At {
            tick: on,
            order: 1,
            kind: TrackEventKind::Meta(MetaMessage::Marker(chord_names[i].as_bytes())),
        });
        for key in voicing(c, 48, 72, 1) {
            chord_events.push(At { tick: on, order: 2, kind: note(0, key, 80, true) });
            chord_events.push(At { tick: off, order: 0, kind: note(0, key, 0, false) });
        }
    }

    let tempo = (spb * 1_000_000.0).round().clamp(1.0, 16_777_215.0) as u32;
    let mut tracks = vec![track(
        "Tempo",
        vec![
            At { tick: 0, order: 0, kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo))) },
            At { tick: 0, order: 1, kind: TrackEventKind::Meta(MetaMessage::TimeSignature(4, 2, 24, 8)) },
        ],
    )];
    for (name, events) in names_owned.iter().zip(tracks_events) {
        tracks.push(track(name, events));
    }
    tracks.push(track("Chords", chord_events));
    let smf = Smf { header: Header::new(Format::Parallel, Timing::Metrical(u15::new(TICKS_PER_QUARTER))), tracks };
    let mut bytes = Vec::new();
    smf.write_std(&mut bytes).expect("writing to memory");
    bytes
}

/// One stem: a name for its file and interleaved stereo at [`SAMPLE_RATE`].
#[derive(Clone, Debug, PartialEq)]
pub struct Stem {
    pub name: String,
    pub samples: Vec<f32>,
}

/// The mix's parts before the master stage: each channel after its inserts and volume (named
/// after its sound module), then each bus after its effects and volume ("<name> bus"). With
/// `wet`, a channel's stem also holds its sends, each run alone through its bus.
pub fn stems(settings: &MixSettings, track: &Soundtrack, physics: &PhysicsInputs, wet: bool) -> Vec<Stem> {
    let frames = track.frames();
    let fit = |s: &[f32]| {
        let mut v = s.to_vec();
        v.resize(frames * 2, 0.0);
        v
    };
    let scene = Arc::new(physics.with_levels(&track.levels).scene);
    let cx = fx_ctx(settings);
    let mut out = Vec::new();
    for (ch, samples) in &track.channels {
        let mut mine = fit(samples);
        if wet && let Some(strip) = settings.strips.get(ch) {
            for bus in &settings.buses {
                let Some(&db) = strip.sends.get(&bus.id) else { continue };
                let mut send = vec![0.0; frames * 2];
                add(&mut send, samples, db_to_gain(db));
                if !bus.fx.nodes.is_empty() {
                    run_fx(&bus.fx, &mut send, cx, &scene);
                }
                add(&mut mine, &send, db_to_gain(bus.volume));
            }
        }
        let name = settings.channel_names.get(ch).cloned().unwrap_or_else(|| format!("Channel {ch}"));
        out.push(Stem { name, samples: mine });
    }
    for (id, samples) in &track.buses {
        let Some(bus) = settings.buses.iter().find(|b| b.id == *id) else { continue };
        let mut v = vec![0.0; frames * 2];
        add(&mut v, samples, db_to_gain(bus.volume));
        out.push(Stem { name: format!("{} bus", bus.name), samples: v });
    }
    out
}

/// A name that is safe as a file name on every system.
fn safe(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() { "Stem".into() } else { trimmed.to_string() }
}

/// The stems' file names: "NN name.wav", numbered in order so equal names never collide.
pub fn stem_file_names(stems: &[Stem]) -> Vec<String> {
    stems.iter().enumerate().map(|(i, s)| format!("{:02} {}.wav", i + 1, safe(&s.name))).collect()
}

/// Writes interleaved stereo as a 48 kHz 24-bit WAV.
pub fn write_wav24(path: &Path, samples: &[f32]) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    const FULL: f32 = 8_388_607.0;
    for &s in samples {
        let v = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        writer.write_sample((v * FULL).round() as i32)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::settings;
    use bpe_core::{Instrument as LibInstrument, InstrumentId, InstrumentSource, NoteLength, VoiceSettings};

    fn at(seconds: f64, key: u8, velocity: f32, channel: u32, instrument: NoteInstrument) -> Note {
        Note {
            frame: (seconds * SAMPLE_RATE as f64).round() as usize,
            instrument,
            key,
            velocity,
            left: 0.5,
            right: 0.5,
            channel,
            hit: None,
        }
    }

    /// Beats every half second; instrument 1 is a patch held for a quarter second.
    fn with_patch() -> MixSettings {
        let mut s = settings();
        s.beats = (0..20).map(|b| b as f64 * 0.5).collect();
        let patch = LibInstrument {
            name: "Saw".into(),
            source: InstrumentSource::Patch { graph: bpe_synth::blank_patch() },
            voices: VoiceSettings { note_length: NoteLength::Seconds(0.25), ..VoiceSettings::default() },
            ..Default::default()
        };
        s.instruments.insert(InstrumentId(1), patch);
        s
    }

    #[test]
    fn midi_round_trip_gives_the_notes_back() {
        let s = with_patch();
        let saw = NoteInstrument::Library(Some(InstrumentId(1)));
        let notes = vec![
            at(0.5, 60, 1.0, 1, saw),
            at(1.25, 64, 0.5, 1, saw),
            at(2.0, 67, 0.0, 2, NoteInstrument::BuiltIn(Instrument::Marimba)),
            at(3.0, 36, 0.8, 3, NoteInstrument::BuiltIn(Instrument::Drums)),
        ];
        let file = bpe_music::midi::parse(&midi_file(&s, &notes, &BTreeMap::new())).unwrap();
        let back: Vec<_> = file.notes.iter().filter(|n| file.parts[n.part as usize].name != "Chords").collect();
        assert_eq!(back.len(), 4, "{:?}", file.parts);
        for (want, got) in notes.iter().zip(&back) {
            let seconds = want.frame as f64 / SAMPLE_RATE as f64;
            assert!((got.time - seconds).abs() < 1e-3, "{} vs {}", got.time, seconds);
            assert_eq!(got.key, want.key);
            assert_eq!(got.velocity, midi_velocity(want.velocity));
            assert!((got.duration - gate_seconds(&s, want) as f64).abs() < 1e-3, "{}", got.duration);
        }
        assert_eq!(midi_velocity(1.0), 127);
        assert_eq!(midi_velocity(0.0), 1);
        let drums = file.parts.iter().find(|p| p.drums).expect("a drum part");
        assert_eq!(drums.channel, 9);
    }

    fn raw_tracks(bytes: &[u8]) -> Vec<Vec<TrackEvent<'_>>> {
        Smf::parse(bytes).unwrap().tracks
    }

    fn name_of<'a>(t: &'a [TrackEvent<'a>]) -> &'a [u8] {
        t.iter()
            .find_map(|e| match e.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => Some(n),
                _ => None,
            })
            .unwrap_or(b"")
    }

    #[test]
    fn midi_has_a_chords_track_with_markers() {
        let s = with_patch();
        let bytes = midi_file(&s, &[], &BTreeMap::new());
        let tracks = raw_tracks(&bytes);
        let chords = tracks.iter().find(|t| name_of(t) == b"Chords").expect("a Chords track");
        let markers: Vec<String> = chords
            .iter()
            .filter_map(|e| match e.kind {
                TrackEventKind::Meta(MetaMessage::Marker(m)) => Some(String::from_utf8_lossy(m).into_owned()),
                _ => None,
            })
            .collect();
        let progression = s.progression(ProgressionRef(None));
        let spans = ChordClock::new(progression, &s.beats).spans(s.duration as f64);
        assert_eq!(markers.len(), spans.len());
        assert_eq!(markers[0], chord_name(&spans[0].2, progression.key));
        let first_keys: Vec<u8> = chords
            .iter()
            .take_while(|e| e.delta == 0)
            .filter_map(|e| match e.kind {
                TrackEventKind::Midi { message: MidiMessage::NoteOn { key, .. }, .. } => Some(key.as_int()),
                _ => None,
            })
            .collect();
        assert_eq!(first_keys, voicing(&spans[0].2, 48, 72, 1));
    }

    #[test]
    fn midi_notes_stop_at_the_end() {
        let mut s = with_patch();
        s.duration = 2.0;
        let saw = NoteInstrument::Library(Some(InstrumentId(1)));
        let notes = vec![at(1.9, 60, 1.0, 1, saw), at(2.5, 62, 1.0, 1, saw)];
        let file = bpe_music::midi::parse(&midi_file(&s, &notes, &BTreeMap::new())).unwrap();
        let played: Vec<_> = file.notes.iter().filter(|n| file.parts[n.part as usize].name != "Chords").collect();
        assert_eq!(played.len(), 1, "the note after the end is left out");
        assert!(played[0].time + played[0].duration <= 2.0 + 1e-3);
    }

    #[test]
    fn midi_tempo_follows_the_beat_grid() {
        let bytes = midi_file(&with_patch(), &[], &BTreeMap::new());
        let tempo = raw_tracks(&bytes)[0].iter().find_map(|e| match e.kind {
            TrackEventKind::Meta(MetaMessage::Tempo(t)) => Some(t.as_int()),
            _ => None,
        });
        assert_eq!(tempo, Some(500_000));
    }

    fn plucks(channel: u32, key: u8) -> Vec<Note> {
        (0..8)
            .map(|i| at(i as f64 * 0.3, key + i as u8, 0.8, channel, NoteInstrument::BuiltIn(Instrument::Pluck)))
            .collect()
    }

    fn chain(kind: &str, knobs: &[(&str, f32)]) -> bpe_graph::Graph {
        let knobs: BTreeMap<String, bpe_graph::Param> =
            knobs.iter().map(|(k, v)| (k.to_string(), bpe_graph::Param::Number(*v))).collect();
        bpe_synth::chain_graph(&[(kind.into(), knobs, true)])
    }

    /// Two channels, channel 1 with a drive insert and a -6 dB send to a reverb bus at -3 dB.
    fn two_channels() -> (MixSettings, Soundtrack) {
        let mut s = settings();
        s.duration = 3.0;
        s.buses = vec![bpe_core::Bus {
            id: bpe_core::BusId(0),
            name: "Reverb".into(),
            volume: -3.0,
            fx: chain("reverb", &[("mix", 1.0)]),
        }];
        s.strips.insert(
            1,
            bpe_core::ChannelStrip {
                inserts: chain("drive", &[("amount", 0.5)]),
                sends: [(bpe_core::BusId(0), -6.0)].into(),
            },
        );
        s.channel_names.insert(1, "Lead".into());
        s.channel_names.insert(2, "Lead".into());
        let mut notes = plucks(1, 60);
        notes.extend(plucks(2, 48));
        notes.sort_by_key(|n| n.frame);
        let mut track = Soundtrack::silent(s.duration);
        let p = crate::buses::premaster_from_notes(
            &s,
            &notes,
            track.frames(),
            &mut Default::default(),
            &Default::default(),
        );
        track.samples = p.samples;
        track.channels = p.channels;
        track.buses = p.buses;
        (s, track)
    }

    fn sum(stems: &[&Stem]) -> Vec<f32> {
        let mut out = vec![0.0; stems[0].samples.len()];
        for s in stems {
            add(&mut out, &s.samples, 1.0);
        }
        out
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn stems_sum_to_the_premaster() {
        let (s, track) = two_channels();
        let stems = stems(&s, &track, &PhysicsInputs::default(), false);
        let names: Vec<&str> = stems.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Lead", "Lead", "Reverb bus"]);
        assert!(stems.iter().all(|s| s.samples.iter().any(|v| v.abs() > 1e-4)), "every stem sounds");
        assert!(max_diff(&sum(&stems.iter().collect::<Vec<_>>()), &track.samples) < 1e-5);
    }

    #[test]
    fn wet_stems_hold_their_sends() {
        let (s, track) = two_channels();
        let dry = stems(&s, &track, &PhysicsInputs::default(), false);
        let wet = stems(&s, &track, &PhysicsInputs::default(), true);
        // Channel 1 is the only one sending, so its wet extra is the whole bus stem.
        let extra: Vec<f32> = wet[0].samples.iter().zip(&dry[0].samples).map(|(w, d)| w - d).collect();
        assert!(max_diff(&extra, &dry[2].samples) < 1e-5);
        assert_eq!(wet[1], dry[1], "channel 2 sends nothing");
    }

    #[test]
    fn same_names_keep_both_stems() {
        let (s, track) = two_channels();
        let names = stem_file_names(&stems(&s, &track, &PhysicsInputs::default(), false));
        assert_eq!(names, ["01 Lead.wav", "02 Lead.wav", "03 Reverb bus.wav"]);
        assert_eq!(stem_file_names(&[Stem { name: "a/b:c?".into(), samples: vec![] }]), ["01 a_b_c_.wav"]);
    }

    #[test]
    fn wav24_round_trips() {
        let dir = std::env::temp_dir().join(format!("bpe-wav24-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let samples: Vec<f32> = (0..2000).map(|i| ((i as f32) * 0.01).sin() * 0.9).collect();
        write_wav24(&path, &samples).unwrap();
        let mut r = hound::WavReader::open(&path).unwrap();
        let spec = r.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 48_000, 24));
        let back: Vec<f32> = r.samples::<i32>().map(|v| v.unwrap() as f32 / 8_388_607.0).collect();
        assert!(max_diff(&back, &samples) <= 1.0 / 8_388_607.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
