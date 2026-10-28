//! Reads MIDI files into melodies: every note with its time in seconds (following the file's
//! tempo changes), split into parts by track and channel, and grouped into steps where notes
//! that start together play together as a chord.

use anyhow::Context;
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::path::Path;

/// Notes that start within this many seconds of each other are one chord step.
pub const CHORD_WINDOW: f64 = 0.03;

/// Melodies that come with the program, as (name used in `builtin:` paths, title, file).
pub const BUILTIN: &[(&str, &str, &[u8])] = &[
    ("ode-to-joy", "Ode to Joy (Beethoven)", include_bytes!("../../../scenes/midi/ode-to-joy.mid")),
    ("fur-elise", "Für Elise (Beethoven)", include_bytes!("../../../scenes/midi/fur-elise.mid")),
    ("twinkle", "Twinkle Twinkle Little Star", include_bytes!("../../../scenes/midi/twinkle.mid")),
];

/// One note of a MIDI file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Seconds from the start of the file.
    pub time: f64,
    pub duration: f64,
    pub key: u8,
    pub velocity: u8,
    /// Which part (see [`MidiFile::parts`]) the note belongs to.
    pub part: u32,
}

/// The notes of one track on one channel. Files that put everything in one track (type 0)
/// still split into a part per instrument this way.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub track: u32,
    /// 0 to 15. Channel 9 (10 when counting from 1) is drums in General MIDI.
    pub channel: u8,
    /// The track's name, or a made-up one.
    pub name: String,
    pub notes: usize,
    pub lowest: u8,
    pub highest: u8,
    pub drums: bool,
}

/// What plays at one bounce: a single note, or several that start together.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub time: f64,
    /// Lowest first, no repeats.
    pub keys: Vec<u8>,
    /// Loudest note's velocity, 1 to 127.
    pub velocity: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiFile {
    pub parts: Vec<Part>,
    /// Every note, by time then key.
    pub notes: Vec<Note>,
    /// Seconds until the last note ends.
    pub length: f64,
}

/// Reads a `.mid` file, or a bundled melody when `path` is `builtin:` and its name.
pub fn load(path: &str, base_dir: Option<&Path>) -> anyhow::Result<MidiFile> {
    if let Some(name) = path.strip_prefix("builtin:") {
        let (_, _, bytes) = BUILTIN
            .iter()
            .find(|(id, ..)| *id == name.trim())
            .with_context(|| format!("there is no built-in melody called {name:?}"))?;
        return parse(bytes);
    }
    let full = match base_dir {
        Some(dir) if Path::new(path).is_relative() => dir.join(path),
        _ => path.into(),
    };
    let bytes = std::fs::read(&full).with_context(|| format!("opening MIDI file {}", full.display()))?;
    parse(&bytes).with_context(|| format!("reading MIDI file {}", full.display()))
}

/// Seconds at each tick, from the tempo changes in all tracks.
struct TempoMap {
    /// (tick, seconds at that tick, seconds per tick from there on).
    segments: Vec<(u64, f64, f64)>,
}

impl TempoMap {
    fn new(timing: Timing, changes: &mut [(u64, u32)]) -> Self {
        match timing {
            Timing::Timecode(fps, subframe) => {
                let per_tick = 1.0 / (fps.as_f32() as f64 * subframe.max(1) as f64);
                Self { segments: vec![(0, 0.0, per_tick)] }
            }
            Timing::Metrical(ppq) => {
                let ppq = ppq.as_int().max(1) as f64;
                // 120 beats a minute until the file says otherwise.
                let mut segments = vec![(0u64, 0.0f64, 0.5 / ppq)];
                changes.sort_by_key(|c| c.0);
                for &(tick, micros) in changes.iter() {
                    let (start, seconds, per_tick) = *segments.last().unwrap();
                    let at = seconds + (tick - start) as f64 * per_tick;
                    let rate = micros.max(1) as f64 / 1e6 / ppq;
                    if tick == start {
                        *segments.last_mut().unwrap() = (tick, seconds, rate);
                    } else {
                        segments.push((tick, at, rate));
                    }
                }
                Self { segments }
            }
        }
    }

    fn seconds(&self, tick: u64) -> f64 {
        let index = self.segments.partition_point(|s| s.0 <= tick).saturating_sub(1);
        let (start, seconds, per_tick) = self.segments[index];
        seconds + (tick - start) as f64 * per_tick
    }
}

/// Reads a MIDI file from its bytes.
pub fn parse(bytes: &[u8]) -> anyhow::Result<MidiFile> {
    let smf = Smf::parse(bytes).map_err(|e| anyhow::anyhow!("not a MIDI file I can read: {e}"))?;
    let mut tempo_changes = Vec::new();
    for track in &smf.tracks {
        let mut tick = 0u64;
        for event in track {
            tick += event.delta.as_int() as u64;
            if let TrackEventKind::Meta(MetaMessage::Tempo(micros)) = event.kind {
                tempo_changes.push((tick, micros.as_int()));
            }
        }
    }
    let tempo = TempoMap::new(smf.header.timing, &mut tempo_changes);

    let mut file = MidiFile::default();
    for (track_index, track) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        let mut name = None;
        // Notes still sounding, as (channel, key) -> (start tick, velocity), as a stack so a key
        // struck twice before letting go ends in order.
        let mut held: Vec<(u8, u8, u64, u8)> = Vec::new();
        let mut finished: Vec<(u8, u8, u64, u64, u8)> = Vec::new();
        for event in track {
            tick += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(text)) if name.is_none() => {
                    name = Some(String::from_utf8_lossy(text).trim().to_string());
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    let (key, velocity, on) = match message {
                        MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int(), vel.as_int() > 0),
                        MidiMessage::NoteOff { key, vel } => (key.as_int(), vel.as_int(), false),
                        _ => continue,
                    };
                    if on {
                        held.push((channel, key, tick, velocity));
                    } else if let Some(i) = held.iter().position(|h| h.0 == channel && h.1 == key) {
                        let (_, _, start, velocity) = held.remove(i);
                        finished.push((channel, key, start, tick, velocity));
                    }
                }
                _ => {}
            }
        }
        // Notes never let go end with the track.
        finished.extend(held.into_iter().map(|(c, k, start, v)| (c, k, start, tick, v)));
        let mut channels: Vec<u8> = finished.iter().map(|n| n.0).collect();
        channels.sort_unstable();
        channels.dedup();
        for channel in channels {
            let part = file.parts.len() as u32;
            let mut count = 0;
            let (mut lowest, mut highest) = (127u8, 0u8);
            for &(_, key, start, end, velocity) in finished.iter().filter(|n| n.0 == channel) {
                let time = tempo.seconds(start);
                let duration = tempo.seconds(end) - time;
                file.notes.push(Note { time, duration, key, velocity: velocity.max(1), part });
                file.length = file.length.max(time + duration);
                count += 1;
                lowest = lowest.min(key);
                highest = highest.max(key);
            }
            let drums = channel == 9;
            let base = match &name {
                Some(n) if !n.is_empty() => n.clone(),
                _ if drums => "Drums".to_string(),
                _ => format!("Track {}", track_index + 1),
            };
            file.parts.push(Part {
                track: track_index as u32,
                channel,
                name: base,
                notes: count,
                lowest,
                highest,
                drums,
            });
        }
    }
    // Two parts from the same track get their channel added to tell them apart.
    let names: Vec<String> = file.parts.iter().map(|p| p.name.clone()).collect();
    for part in &mut file.parts {
        if names.iter().filter(|n| **n == part.name).count() > 1 {
            part.name = format!("{} (channel {})", part.name, part.channel + 1);
        }
    }
    file.notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.key.cmp(&b.key)).then(a.part.cmp(&b.part)));
    Ok(file)
}

impl MidiFile {
    /// The part a melody plays by default: the one with the most notes that isn't drums, or
    /// the drums if that's all there is.
    pub fn default_part(&self) -> Option<u32> {
        let busiest = |drums: bool| {
            self.parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.drums == drums && p.notes > 0)
                .max_by(|(i, a), (j, b)| a.notes.cmp(&b.notes).then(j.cmp(i)))
                .map(|(i, _)| i as u32)
        };
        busiest(false).or_else(|| busiest(true))
    }

    /// The notes of one part (or the default part) as steps, shifted by `transpose` semitones.
    /// Notes starting within [`CHORD_WINDOW`] of a step's first note join that step.
    pub fn steps(&self, part: Option<u32>, transpose: i32) -> Vec<Step> {
        let Some(part) = part.filter(|p| (*p as usize) < self.parts.len()).or_else(|| self.default_part()) else {
            return Vec::new();
        };
        let mut steps: Vec<Step> = Vec::new();
        for note in self.notes.iter().filter(|n| n.part == part) {
            let key = (note.key as i32 + transpose).clamp(0, 127) as u8;
            match steps.last_mut() {
                Some(step) if note.time - step.time <= CHORD_WINDOW => {
                    if !step.keys.contains(&key) {
                        step.keys.push(key);
                        step.keys.sort_unstable();
                    }
                    step.velocity = step.velocity.max(note.velocity);
                }
                _ => steps.push(Step { time: note.time, keys: vec![key], velocity: note.velocity }),
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vlq(mut v: u32) -> Vec<u8> {
        let mut out = vec![(v & 0x7f) as u8];
        v >>= 7;
        while v > 0 {
            out.push((v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
        out.reverse();
        out
    }

    /// A type-0 file from (delta ticks, event bytes) pairs, at 480 ticks a beat.
    fn file(events: &[(u32, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        for (delta, body) in events {
            data.extend(vlq(*delta));
            data.extend(*body);
        }
        data.extend([0, 0xff, 0x2f, 0]);
        let mut out = b"MThd".to_vec();
        out.extend([0, 0, 0, 6, 0, 0, 0, 1, 0x01, 0xe0]);
        out.extend(b"MTrk");
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn bundled_melodies_load_and_pick_the_tune() {
        for (id, title, _) in BUILTIN {
            let file = load(&format!("builtin:{id}"), None).unwrap();
            let part = file.default_part().unwrap();
            assert!(!file.parts[part as usize].drums, "{title}");
            let steps = file.steps(None, 0);
            assert!(steps.len() >= 30, "{title} has {} steps", steps.len());
            assert!(steps.windows(2).all(|w| w[0].time < w[1].time), "{title}");
        }
        let ode = load("builtin:ode-to-joy", None).unwrap();
        assert_eq!(ode.parts[ode.default_part().unwrap() as usize].name, "Melody");
        let first: Vec<u8> = ode.steps(None, 0).iter().take(4).map(|s| s.keys[0]).collect();
        assert_eq!(first, vec![64, 64, 65, 67]); // E E F G
        // The accompaniment is chords: both notes of each start together, so one step each.
        let accompaniment = ode.parts.iter().position(|p| p.name == "Accompaniment").unwrap() as u32;
        let chords = ode.steps(Some(accompaniment), 0);
        assert_eq!(chords[0].keys, vec![48, 55]);
        // 120 beats a minute: the second melody note starts half a second in.
        assert!((ode.steps(None, 0)[1].time - 0.5).abs() < 1e-9);
        let twinkle = load("builtin:twinkle", None).unwrap();
        assert!(twinkle.parts.iter().any(|p| p.drums && p.notes > 40));
        assert!(load("builtin:nope", None).is_err());
    }

    #[test]
    fn tempo_changes_move_later_notes() {
        let bytes = file(&[
            (0, &[0x90, 60, 100]),
            (480, &[0x80, 60, 0]),
            // Tempo halves to 60 beats a minute (1,000,000 microseconds a beat).
            (0, &[0xff, 0x51, 0x03, 0x0f, 0x42, 0x40]),
            (0, &[0x90, 62, 100]),
            (480, &[0x90, 62, 0]), // note on with velocity 0 ends a note too
            (0, &[0x90, 64, 90]),
        ]);
        let file = parse(&bytes).unwrap();
        let times: Vec<f64> = file.notes.iter().map(|n| n.time).collect();
        assert_eq!(times, vec![0.0, 0.5, 1.5]);
        assert!((file.notes[1].duration - 1.0).abs() < 1e-9);
        // The last note is never let go, so it lasts until the track ends.
        assert_eq!(file.notes[2].duration, 0.0);
        assert_eq!(file.parts.len(), 1);
        assert_eq!(file.parts[0].notes, 3);
    }

    #[test]
    fn chords_group_within_the_window_and_transpose_clamps() {
        let bytes = file(&[
            (0, &[0x90, 60, 80]),
            (5, &[0x90, 64, 100]), // 5 ticks is about 5 ms: same chord
            (0, &[0x90, 60, 70]),  // the same key again joins without repeating
            (480, &[0x90, 67, 60]),
            (0, &[0x99, 36, 100]), // a kick on the drum channel is its own part
        ]);
        let file = parse(&bytes).unwrap();
        assert_eq!(file.parts.len(), 2);
        assert!(file.parts[1].drums);
        let steps = file.steps(None, 0);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].keys, vec![60, 64]);
        assert_eq!(steps[0].velocity, 100);
        assert_eq!(file.steps(None, 100)[0].keys, vec![127]);
        assert_eq!(file.steps(None, -100)[0].keys, vec![0]);
        assert_eq!(file.steps(Some(1), 0)[0].keys, vec![36]);
        // A part that doesn't exist falls back to the default one.
        assert_eq!(file.steps(Some(9), 0), steps);
    }

    #[test]
    fn empty_and_broken_files() {
        let empty = parse(&file(&[])).unwrap();
        assert!(empty.parts.is_empty() && empty.default_part().is_none());
        assert!(empty.steps(None, 0).is_empty());
        assert!(parse(b"not midi").is_err());
        assert!(parse(&[]).is_err());
        assert!(load("/no/such/file.mid", None).is_err());
    }
}
