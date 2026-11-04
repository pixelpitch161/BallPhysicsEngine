//! The notes the shapes played, saved as a MIDI file beside the export: format 1 at 960 ticks a
//! quarter note, the scene's tempo and time signature on the first track, then one track per
//! shape named after it, each note on the exact sample its hit made it.

use crate::{SAMPLE_RATE, TempoMap};
use bpe_core::{Scene, ThingId};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use std::path::Path;

/// Ticks in a quarter note.
pub const PPQ: u16 = 960;

/// A note a shape started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteOn {
    /// Its first sample, from the start of the sound.
    pub sample: u64,
    pub note: u8,
    /// How hard the hit was (0..1).
    pub strength: f32,
    /// Samples its gate stayed open (`u64::MAX`: until the shape's next note from another hit,
    /// or until the shape lets go of it).
    pub gate: u64,
    /// The notes one hit (or one drone change) started share it: a strum's notes don't end each
    /// other.
    pub group: u64,
    /// Plays nothing: the shape let go of its held note of this pitch here.
    pub ends: bool,
}

impl NoteOn {
    /// A note started by the hit (or drone change) `group`.
    pub fn new(sample: u64, note: u8, strength: f32, gate: u64, group: u64) -> NoteOn {
        NoteOn { sample, note, strength, gate, group, ends: false }
    }

    /// The shape lets go of its held `note` at `sample`.
    pub fn end(sample: u64, note: u8) -> NoteOn {
        NoteOn { sample, note, strength: 0.0, gate: 0, group: u64::MAX, ends: true }
    }
}

/// The notes every shape played, in the order they started.
#[derive(Clone, Debug, Default)]
pub struct NoteLog {
    pub notes: Vec<(ThingId, NoteOn)>,
}

impl NoteLog {
    pub fn push(&mut self, thing: ThingId, note: NoteOn) {
        self.notes.push((thing, note));
    }

    /// Each note as (shape, note, velocity 1..127, first sample, last sample), cut at `end`.
    /// A note whose gate waits lasts until the shape's next note from another hit, or until the
    /// shape lets go of it; any note ends where the shape plays the same pitch again.
    pub fn spans(&self, end: u64) -> Vec<(ThingId, u8, u8, u64, u64)> {
        // In sample order (a strum's later notes are logged before their samples come).
        let mut order: Vec<usize> = (0..self.notes.len()).collect();
        order.sort_by_key(|&i| self.notes[i].1.sample);
        let mut out = Vec::new();
        for (k, &i) in order.iter().enumerate() {
            let (thing, n) = &self.notes[i];
            if n.ends || n.sample >= end {
                continue;
            }
            let mut off = if n.gate == u64::MAX { end } else { n.sample.saturating_add(n.gate.max(1)) };
            for &j in &order[k + 1..] {
                let (t, m) = &self.notes[j];
                if m.sample >= off {
                    break;
                }
                if t != thing || m.sample <= n.sample {
                    continue;
                }
                let waits = n.gate == u64::MAX;
                if m.note == n.note || (waits && !m.ends && m.group != n.group) {
                    off = m.sample;
                    break;
                }
            }
            let velocity = (n.strength.clamp(0.0, 1.0) * 126.0).round() as u8 + 1;
            out.push((*thing, n.note.min(127), velocity, n.sample, off.min(end).max(n.sample + 1)));
        }
        out.sort_by_key(|s| (s.3, s.1));
        out
    }
}

/// Seconds in a quarter note at `tempo` (the tempo counts quarter notes, whatever the beat
/// note).
fn quarter_seconds(tempo: &TempoMap) -> f64 {
    60.0 / tempo.bpm()
}

/// The tick of sample `s`.
pub fn tick_of(s: u64, tempo: &TempoMap) -> u64 {
    (s as f64 / SAMPLE_RATE as f64 / quarter_seconds(tempo) * PPQ as f64).round() as u64
}

/// Writes `log` (cut at sample `end`) to `path`, one track per shape of `scene` that played.
pub fn write_midi(path: &Path, scene: &Scene, log: &NoteLog, tempo: &TempoMap, end: u64) -> anyhow::Result<()> {
    let spans = log.spans(end);
    let mut shapes: Vec<ThingId> = Vec::new();
    for (t, ..) in &spans {
        if !shapes.contains(t) {
            shapes.push(*t);
        }
    }
    // Shapes in the tree's order.
    shapes.sort_by_key(|t| scene.order.iter().position(|o| o == t).unwrap_or(usize::MAX));
    let names: Vec<Vec<u8>> = shapes
        .iter()
        .map(|t| scene.things.get(t).map_or_else(|| format!("Shape {}", t.0), |th| th.name.clone()).into_bytes())
        .collect();

    let mut tracks: Vec<Vec<TrackEvent>> = Vec::new();
    let micros = (quarter_seconds(tempo) * 1e6).round().clamp(1.0, 16_777_215.0) as u32;
    let denominator = (tempo.bottom().max(1) as f64).log2().round() as u8;
    tracks.push(vec![
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros))) },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(tempo.top().clamp(1, 255) as u8, denominator, 24, 8)),
        },
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ]);
    for (shape, name) in shapes.iter().zip(&names) {
        // (tick, off first, note, velocity)
        let mut events: Vec<(u64, bool, u8, u8)> = Vec::new();
        for &(t, note, vel, on, off) in spans.iter().filter(|s| s.0 == *shape) {
            let _ = t;
            let (a, b) = (tick_of(on, tempo), tick_of(off, tempo));
            events.push((a, false, note, vel));
            events.push((b.max(a + 1), true, note, 0));
        }
        // At the same tick a note ends before the next one starts.
        events.sort_by_key(|e| (e.0, !e.1));
        let mut track =
            vec![TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::TrackName(name)) }];
        let mut last = 0u64;
        for (tick, off, note, vel) in events {
            let message = if off {
                MidiMessage::NoteOff { key: u7::new(note), vel: u7::new(0) }
            } else {
                MidiMessage::NoteOn { key: u7::new(note), vel: u7::new(vel.min(127)) }
            };
            let delta = (tick - last).min(u28::max_value().as_int() as u64) as u32;
            track.push(TrackEvent {
                delta: u28::new(delta),
                kind: TrackEventKind::Midi { channel: u4::new(0), message },
            });
            last = tick;
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
        tracks.push(track);
    }
    let smf = Smf { header: Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ))), tracks };
    smf.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_follow_the_tempo_and_beat_note() {
        let t = TempoMap::new(120.0, 4, 4);
        assert_eq!(tick_of(24_000, &t), 960, "half a second is a quarter at 120");
        let eighths = TempoMap::new(120.0, 6, 8);
        assert_eq!(tick_of(24_000, &eighths), 960, "the tempo counts quarters in 6/8 too");
    }

    #[test]
    fn a_note_waiting_for_the_next_hit_lasts_until_it() {
        let mut log = NoteLog::default();
        let n = |sample, gate| NoteOn::new(sample, 60, 1.0, gate, sample);
        log.push(ThingId(1), n(100, u64::MAX));
        log.push(ThingId(1), n(500, 50));
        log.push(ThingId(2), n(200, u64::MAX));
        let spans = log.spans(1_000);
        assert_eq!(spans[0], (ThingId(1), 60, 127, 100, 500));
        assert_eq!(spans[1], (ThingId(2), 60, 127, 200, 1_000), "the last one ends with the clip");
        assert_eq!(spans[2], (ThingId(1), 60, 127, 500, 550));
    }

    #[test]
    fn a_strum_holds_together_until_the_next_hit() {
        let mut log = NoteLog::default();
        // One hit at 100 strums 60, 64, 67 (logged at the hit, sounding 10 samples apart); the
        // next hit at 400 plays 72.
        for (k, note) in [60u8, 64, 67].into_iter().enumerate() {
            log.push(ThingId(1), NoteOn::new(100 + 10 * k as u64, note, 1.0, u64::MAX, 100));
        }
        log.push(ThingId(1), NoteOn::new(400, 72, 1.0, u64::MAX, 400));
        let spans = log.spans(1_000);
        let ends: Vec<(u8, u64, u64)> = spans.iter().map(|s| (s.1, s.3, s.4)).collect();
        assert_eq!(ends, [(60, 100, 400), (64, 110, 400), (67, 120, 400), (72, 400, 1_000)]);
    }

    #[test]
    fn held_notes_end_when_let_go_and_a_pitch_ends_where_it_plays_again() {
        let mut log = NoteLog::default();
        log.push(ThingId(1), NoteOn::new(0, 60, 1.0, u64::MAX, 0));
        log.push(ThingId(1), NoteOn::new(0, 64, 1.0, u64::MAX, 0));
        log.push(ThingId(1), NoteOn::end(300, 60));
        log.push(ThingId(1), NoteOn::end(300, 64));
        // Fixed gates of 500 samples on the same pitch 100 apart: each ends at the next.
        log.push(ThingId(2), NoteOn::new(100, 50, 1.0, 500, 100));
        log.push(ThingId(2), NoteOn::new(200, 50, 1.0, 500, 200));
        let spans = log.spans(1_000);
        let ends: Vec<(u8, u64, u64)> = spans.iter().map(|s| (s.1, s.3, s.4)).collect();
        assert_eq!(ends, [(60, 0, 300), (64, 0, 300), (50, 100, 200), (50, 200, 700)]);
    }
}
