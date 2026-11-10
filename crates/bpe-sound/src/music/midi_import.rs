//! Import MIDI into slots (W11): a `.mid` file's chords become a sound's note slots, its tempo
//! and time signature the scene's, and its timing either By tempo (every chord as long, in order)
//! or a Jump to slot key at each chord. Pure: the editor applies what [`import`] gives.

use bpe_core::NoteSlot;
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

/// The most slots a sound has.
const MAX_SLOTS: usize = 16;
/// The drum channel (channel 10, counted from 0).
const DRUMS: u8 = 9;

/// When the imported slots change.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotTiming {
    /// Every chord lasts `every` bars (or beats), in slot order from the start: By tempo.
    ByTempo { every: u32, bars: bool },
    /// Only on keyframes: Jump to slot `slot` (from 1) at `seconds` from the file's start.
    Keys(Vec<(f64, u32)>),
}

/// What a MIDI file comes to.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiImport {
    /// The distinct chords, in the order they first play (at most 16).
    pub slots: Vec<NoteSlot>,
    /// How many chords the file plays, and how many distinct ones were left out (past 16).
    pub chords: usize,
    pub left_out: usize,
    /// The file's first tempo (quarter notes a minute), clamped to the scene's 20..300.
    pub bpm: f64,
    /// Its first time signature: beats in a bar and beat note (2, 4, 8 or 16).
    pub time_signature: (u32, u32),
    /// The tempo changes later in the file (the chords' times follow them; the scene keeps one).
    pub tempo_changes: bool,
    pub timing: SlotTiming,
    /// The keyboard range that shows every note: its lowest octave and how many octaves.
    pub low_octave: i32,
    pub octaves: i32,
    /// Every chord in the order it plays (at most [`bpe_kit::sound::MAX_MELODY`]), for Import
    /// MIDI as a melody, how many chords past that were left out, and the keyboard range that
    /// shows all of them.
    pub melody: Vec<NoteSlot>,
    pub melody_left_out: usize,
    pub melody_low_octave: i32,
    pub melody_octaves: i32,
}

struct Note {
    start: u64,
    end: u64,
    key: u8,
    channel: u8,
}

/// Reads `bytes` as a MIDI file into slots. An error says, in plain words, why it can't.
pub fn import(bytes: &[u8]) -> Result<MidiImport, String> {
    let smf = Smf::parse(bytes).map_err(|_| "That file isn't a MIDI file the program can read.".to_string())?;
    let ppq = match smf.header.timing {
        Timing::Metrical(t) => t.as_int().max(1) as u64,
        Timing::Timecode(..) => {
            return Err("That MIDI file counts time in SMPTE frames; save it with bars and beats instead.".into());
        }
    };
    let mut notes = Vec::new();
    let mut tempos: Vec<(u64, u32)> = Vec::new();
    let mut signature: Option<(u64, u8, u8)> = None;
    for track in &smf.tracks {
        let mut tick = 0u64;
        // Notes sounding, by channel and key, oldest first.
        let mut open: Vec<(u8, u8, u64)> = Vec::new();
        for ev in track {
            tick += ev.delta.as_int() as u64;
            match ev.kind {
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    let (key, on) = match message {
                        MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int() > 0),
                        MidiMessage::NoteOff { key, .. } => (key.as_int(), false),
                        _ => continue,
                    };
                    if on {
                        open.push((ch, key, tick));
                    } else if let Some(i) = open.iter().position(|(c, k, _)| *c == ch && *k == key) {
                        let (_, _, start) = open.remove(i);
                        notes.push(Note { start, end: tick, key, channel: ch });
                    }
                }
                TrackEventKind::Meta(MetaMessage::Tempo(t)) => tempos.push((tick, t.as_int())),
                TrackEventKind::Meta(MetaMessage::TimeSignature(num, denom, _, _))
                    if signature.is_none_or(|(at, _, _)| tick < at) =>
                {
                    signature = Some((tick, num, denom));
                }
                _ => {}
            }
        }
        // A note never let go lasts to the track's end.
        for (channel, key, start) in open {
            notes.push(Note { start, end: tick.max(start), key, channel });
        }
    }
    // Drums only when there is nothing else.
    if notes.iter().any(|n| n.channel != DRUMS) {
        notes.retain(|n| n.channel != DRUMS);
    }
    if notes.is_empty() {
        return Err("That MIDI file has no notes in it.".into());
    }
    notes.sort_by_key(|n| (n.start, n.key));
    tempos.sort_by_key(|(t, _)| *t);

    // Chords: the notes starting within a 32nd note of the first.
    let tolerance = (ppq / 8).max(1);
    let mut chords: Vec<(u64, u64, Vec<u8>)> = Vec::new();
    let mut i = 0;
    while i < notes.len() {
        let start = notes[i].start;
        let mut keys = Vec::new();
        let mut end = start;
        while i < notes.len() && notes[i].start - start <= tolerance {
            keys.push(notes[i].key);
            end = end.max(notes[i].end);
            i += 1;
        }
        keys.sort_unstable();
        keys.dedup();
        chords.push((start, end, keys));
    }
    // Each chord lasts until the next one starts.
    for k in 0..chords.len().saturating_sub(1) {
        chords[k].1 = chords[k + 1].0;
    }

    let mut distinct: Vec<Vec<u8>> = Vec::new();
    let mut slot_of = Vec::with_capacity(chords.len());
    for (_, _, keys) in &chords {
        let at = match distinct.iter().position(|d| d == keys) {
            Some(at) => at,
            None => {
                distinct.push(keys.clone());
                distinct.len() - 1
            }
        };
        slot_of.push(at);
    }
    let left_out = distinct.len().saturating_sub(MAX_SLOTS);
    let slots: Vec<NoteSlot> =
        distinct.iter().take(MAX_SLOTS).map(|n| NoteSlot { notes: n.clone(), order_clicked: Vec::new() }).collect();

    // A file that says nothing is 120 BPM in 4/4 (MIDI's own default), and the scene is set so too,
    // so the chords are counted as the scene will play them.
    let first_bpm = tempos.first().map_or(120.0, |(_, us)| 60_000_000.0 / (*us).max(1) as f64);
    let bpm = first_bpm.clamp(20.0, 300.0);
    let tempo_changes = tempos.iter().skip(1).any(|(_, us)| Some(*us) != tempos.first().map(|t| t.1));
    let time_signature = signature
        .and_then(|(_, num, denom)| {
            let bottom = 1u32.checked_shl(denom as u32)?;
            [2, 4, 8, 16].contains(&bottom).then_some(((num as u32).clamp(1, 16), bottom))
        })
        .unwrap_or((4, 4));
    let (top, bottom) = time_signature;

    // Uniform: every chord as long, a whole number of beats, from the start, through the slots
    // in order.
    let first_len = chords[0].1 - chords[0].0;
    let in_order = slot_of.iter().enumerate().all(|(k, s)| *s == k % distinct.len());
    // The last chord lasts as long as its notes are held, so only the ones before it count.
    let even = chords[..chords.len() - 1].iter().all(|(s, e, _)| e - s == first_len);
    let beat_ticks = 4 * ppq;
    let whole_beats = first_len > 0 && (first_len * bottom as u64).is_multiple_of(beat_ticks);
    let beats = if whole_beats { first_len * bottom as u64 / beat_ticks } else { 0 };
    let uniform =
        chords[0].0 == 0 && in_order && even && whole_beats && left_out == 0 && !tempo_changes && bpm == first_bpm;
    let timing = if uniform && beats % top as u64 == 0 && beats / top as u64 <= 64 {
        SlotTiming::ByTempo { every: (beats / top as u64) as u32, bars: true }
    } else if uniform && beats <= 64 {
        SlotTiming::ByTempo { every: beats as u32, bars: false }
    } else {
        let seconds = |tick: u64| seconds_at(tick, &tempos, ppq);
        let mut keys: Vec<(f64, u32)> = Vec::new();
        for ((start, _, _), slot) in chords.iter().zip(&slot_of) {
            if *slot >= MAX_SLOTS {
                continue;
            }
            let slot = *slot as u32 + 1;
            if keys.last().is_some_and(|(_, s)| *s == slot) {
                continue;
            }
            keys.push((seconds(*start), slot));
        }
        SlotTiming::Keys(keys)
    };

    let range = |all: &mut dyn Iterator<Item = u8>| {
        let (lo, hi) = all.fold((127u8, 0u8), |(lo, hi), n| (lo.min(n), hi.max(n)));
        let low = (lo / 12) as i32;
        (low, ((hi / 12) as i32 - low + 1).clamp(1, 8))
    };
    let (low_octave, octaves) = range(&mut slots.iter().flat_map(|s| s.notes.iter().copied()));
    let max_melody = bpe_kit::sound::MAX_MELODY;
    let melody: Vec<NoteSlot> = chords
        .iter()
        .take(max_melody)
        .map(|(_, _, keys)| NoteSlot { notes: keys.clone(), order_clicked: Vec::new() })
        .collect();
    let melody_left_out = chords.len().saturating_sub(max_melody);
    let (melody_low_octave, melody_octaves) = range(&mut melody.iter().flat_map(|s| s.notes.iter().copied()));
    Ok(MidiImport {
        slots,
        chords: chords.len(),
        left_out,
        bpm,
        time_signature,
        tempo_changes,
        timing,
        low_octave,
        octaves,
        melody,
        melody_left_out,
        melody_low_octave,
        melody_octaves,
    })
}

/// Seconds from the file's start to `tick`, through its tempo changes (120 BPM before the first).
fn seconds_at(tick: u64, tempos: &[(u64, u32)], ppq: u64) -> f64 {
    let mut seconds = 0.0;
    let (mut at, mut us) = (0u64, 500_000u32);
    for &(t, u) in tempos {
        if t >= tick {
            break;
        }
        seconds += (t - at) as f64 * us as f64 / 1e6 / ppq as f64;
        at = t;
        us = u;
    }
    seconds + (tick - at) as f64 * us as f64 / 1e6 / ppq as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use midly::num::{u4, u7, u15, u24, u28};
    use midly::{Format, Header, TrackEvent};

    /// A MIDI file at 480 ticks a quarter: a tempo (µs a quarter), a time signature and notes
    /// (start tick, length, key, channel), in one track.
    fn file(tempos: &[(u64, u32)], sig: Option<(u8, u8)>, notes: &[(u64, u64, u8, u8)]) -> Vec<u8> {
        let mut evs: Vec<(u64, TrackEventKind<'static>)> = Vec::new();
        for &(t, us) in tempos {
            evs.push((t, TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us)))));
        }
        if let Some((n, d)) = sig {
            evs.push((0, TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, 24, 8))));
        }
        for &(start, len, key, ch) in notes {
            let on = MidiMessage::NoteOn { key: u7::new(key), vel: u7::new(100) };
            let off = MidiMessage::NoteOff { key: u7::new(key), vel: u7::new(0) };
            evs.push((start, TrackEventKind::Midi { channel: u4::new(ch), message: on }));
            evs.push((start + len, TrackEventKind::Midi { channel: u4::new(ch), message: off }));
        }
        evs.sort_by_key(|(t, k)| (*t, matches!(k, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. })));
        let mut track = Vec::new();
        let mut at = 0;
        for (t, kind) in evs {
            track.push(TrackEvent { delta: u28::new((t - at) as u32), kind });
            at = t;
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
        let smf =
            Smf { header: Header::new(Format::SingleTrack, Timing::Metrical(u15::new(480))), tracks: vec![track] };
        let mut out = Vec::new();
        smf.write_std(&mut out).unwrap();
        out
    }

    const BAR: u64 = 4 * 480;

    fn chord(at: u64, len: u64, keys: &[u8]) -> Vec<(u64, u64, u8, u8)> {
        keys.iter().map(|k| (at, len, *k, 0)).collect()
    }

    #[test]
    fn a_chord_a_bar_becomes_slots_changing_every_bar_with_the_tempo() {
        let mut notes = chord(0, BAR, &[60, 64, 67]);
        notes.extend(chord(BAR, BAR, &[57, 60, 64]));
        notes.extend(chord(2 * BAR, BAR, &[60, 64, 67]));
        notes.extend(chord(3 * BAR, BAR, &[57, 60, 64]));
        let m = import(&file(&[(0, 600_000)], Some((4, 2)), &notes)).unwrap();
        assert_eq!(m.slots.len(), 2, "repeated chords share a slot");
        assert_eq!(m.slots[0].notes, [60, 64, 67]);
        assert_eq!(m.slots[1].notes, [57, 60, 64]);
        assert_eq!(m.chords, 4);
        assert_eq!(m.bpm, 100.0);
        assert_eq!(m.time_signature, (4, 4));
        assert_eq!(m.timing, SlotTiming::ByTempo { every: 1, bars: true });
        assert_eq!((m.low_octave, m.octaves), (4, 2));
    }

    #[test]
    fn two_beat_chords_change_every_two_beats() {
        let mut notes = chord(0, 960, &[60]);
        notes.extend(chord(960, 960, &[62]));
        notes.extend(chord(1920, 960, &[64]));
        let m = import(&file(&[(0, 500_000)], Some((4, 2)), &notes)).unwrap();
        assert_eq!(m.timing, SlotTiming::ByTempo { every: 2, bars: false });
    }

    #[test]
    fn uneven_chords_become_jump_keys_at_their_times() {
        // At 120 BPM a quarter is 0.5 s: chords at 0, 1.5 s and 2 s.
        let mut notes = chord(0, 3 * 480, &[60, 64]);
        notes.extend(chord(3 * 480, 480, &[62, 65]));
        notes.extend(chord(4 * 480, BAR, &[60, 64]));
        let m = import(&file(&[(0, 500_000)], None, &notes)).unwrap();
        assert_eq!(m.time_signature, (4, 4), "4/4 when the file says nothing");
        match m.timing {
            SlotTiming::Keys(keys) => assert_eq!(keys, [(0.0, 1), (1.5, 2), (2.0, 1)]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn notes_a_little_apart_are_one_chord_and_a_late_start_uses_keys() {
        let notes = vec![(480, BAR, 60, 0), (500, BAR, 64, 0), (530, BAR, 67, 0)];
        let m = import(&file(&[], None, &notes)).unwrap();
        assert_eq!(m.slots.len(), 1);
        assert_eq!(m.slots[0].notes, [60, 64, 67]);
        assert_eq!(m.bpm, 120.0);
        assert_eq!(m.timing, SlotTiming::Keys(vec![(0.5, 1)]), "120 BPM when the file says nothing");
    }

    #[test]
    fn more_than_sixteen_chords_are_capped_and_counted() {
        let notes: Vec<_> = (0..20u64).map(|i| (i * 480, 480, 40 + i as u8, 0)).collect();
        let m = import(&file(&[(0, 500_000)], None, &notes)).unwrap();
        assert_eq!(m.slots.len(), 16);
        assert_eq!(m.left_out, 4);
        match m.timing {
            SlotTiming::Keys(keys) => assert_eq!(keys.len(), 16),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_melody_keeps_every_chord_in_time_order_with_repeats() {
        // C, E, C, G then a C-E chord: five steps, the repeated C kept.
        let notes = [(0, 480, 60, 0), (480, 480, 64, 0), (960, 480, 60, 0), (1440, 480, 67, 0), (1920, 480, 60, 0)];
        let mut notes = notes.to_vec();
        notes.push((1920, 480, 64, 0));
        let m = import(&file(&[(0, 500_000)], None, &notes)).unwrap();
        let steps: Vec<Vec<u8>> = m.melody.iter().map(|s| s.notes.clone()).collect();
        assert_eq!(steps, vec![vec![60], vec![64], vec![60], vec![67], vec![60, 64]]);
        assert_eq!(m.slots.len(), 4, "the slots still hold the distinct chords");
        assert_eq!((m.melody_left_out, m.melody_low_octave, m.melody_octaves), (0, 5, 1));
        let long: Vec<_> = (0..1030u64).map(|i| (i * 120, 120, 40 + (i % 30) as u8, 0)).collect();
        let m = import(&file(&[(0, 500_000)], None, &long)).unwrap();
        assert_eq!((m.melody.len(), m.melody_left_out), (1024, 6));
    }

    #[test]
    fn drums_are_left_out_unless_they_are_all_there_is() {
        let notes = vec![(0, 480, 36, DRUMS), (0, 480, 60, 0)];
        let m = import(&file(&[], None, &notes)).unwrap();
        assert_eq!(m.slots[0].notes, [60]);
        let m = import(&file(&[], None, &[(0, 480, 36, DRUMS)])).unwrap();
        assert_eq!(m.slots[0].notes, [36]);
    }

    #[test]
    fn tempo_changes_are_followed_and_said() {
        // 120 BPM for a bar (2 s), then 60 BPM: the third chord is 2 s + 1 s in.
        let mut notes = chord(0, BAR, &[60]);
        notes.extend(chord(BAR, 480, &[62]));
        notes.extend(chord(BAR + 480, 480, &[64]));
        let m = import(&file(&[(0, 500_000), (BAR, 1_000_000)], None, &notes)).unwrap();
        assert!(m.tempo_changes);
        assert_eq!(m.bpm, 120.0);
        match m.timing {
            SlotTiming::Keys(keys) => assert_eq!(keys, [(0.0, 1), (2.0, 2), (3.0, 3)]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rubbish_and_empty_files_say_why() {
        assert!(import(b"not a midi file").unwrap_err().contains("isn't a MIDI file"));
        assert!(import(&file(&[(0, 500_000)], None, &[])).unwrap_err().contains("no notes"));
        let smpte = Smf {
            header: Header::new(Format::SingleTrack, Timing::Timecode(midly::Fps::Fps25, 40)),
            tracks: vec![vec![TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) }]],
        };
        let mut out = Vec::new();
        smpte.write_std(&mut out).unwrap();
        assert!(import(&out).unwrap_err().contains("SMPTE"));
    }
}
