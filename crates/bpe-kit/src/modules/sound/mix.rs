//! Turning hits into notes of the current chord. A module whose graph has been edited asks the
//! graph about every hit instead, with the native answer standing in for any output it leaves
//! unwired.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_4;

use bpe_core::{ModuleGraph, Params, Settings, SlotId, ThingId};
use bpe_mix::{Chords, MixCx, ModuleMix, Note, SAMPLE_RATE, mix64};
use bpe_runtime::{Event, EventKind};
use bpe_vm::{Ball, Host, Program, Val, Vm};

use super::NoteSound;
use crate::graphs::{bind, compile_module};

pub struct NoteSoundMix;

/// What one hit plays: the native answer, and what the compiled path starts from.
struct Played {
    play: bool,
    key: u8,
    velocity: f32,
    volume: f32,
    pan: f32,
    decay: f32,
    brightness: f32,
}

fn native(s: &NoteSound, chords: &Chords, e: &Event, time: f64, index: u64, cx: &MixCx) -> Played {
    let span = (s.loudest_hit - s.quietest_hit).max(1.0);
    let candidates = chords.notes_at(time);
    let key =
        if candidates.is_empty() { 0 } else { candidates[mix64(cx.settings.seed ^ index) as usize % candidates.len()] };
    Played {
        play: e.speed >= s.quietest_hit && !candidates.is_empty(),
        key,
        velocity: ((e.speed - s.quietest_hit) / span).clamp(0.0, 1.0),
        volume: s.volume,
        pan: (e.point.x / cx.settings.half_width.max(1.0)).clamp(-1.0, 1.0) * s.stereo_width,
        decay: s.decay,
        brightness: s.brightness,
    }
}

fn note(s: &NoteSound, frame: usize, p: &Played) -> Note {
    let gain = p.velocity * p.velocity.sqrt() * 10f32.powf(p.volume / 20.0);
    let angle = (p.pan + 1.0) * FRAC_PI_4;
    Note {
        frame,
        instrument: s.instrument,
        key: p.key,
        velocity: p.velocity,
        left: gain * angle.cos(),
        right: gain * angle.sin(),
        decay: p.decay,
        brightness: p.brightness,
    }
}

/// A sound graph's view of the chords, for one hit.
struct SoundHost<'a> {
    chords: &'a Chords,
    seed: u64,
    /// The hit's time, kept exact so the chord is the one the native path hears.
    time: f64,
}

impl Host for SoundHost<'_> {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn chord_note(&self, _time: f32, index: u64) -> Option<f32> {
        let candidates = self.chords.notes_at(self.time);
        (!candidates.is_empty()).then(|| candidates[mix64(self.seed ^ index) as usize % candidates.len()] as f32)
    }
}

impl NoteSoundMix {
    fn compiled(params: &Params, graph: Option<&ModuleGraph>) -> Option<Program> {
        let g = graph.filter(|g| g.custom)?;
        compile_module(SlotId::Sound, &bind(&g.graph, params)).ok().filter(|p| !p.is_empty())
    }
}

impl ModuleMix for NoteSoundMix {
    fn kind(&self) -> &'static str {
        "sound.notes"
    }

    fn notes(&self, params: &Params, _thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = NoteSound::from_params(params);
        let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
        let rate = cx.settings.steps_per_second.max(1);
        for e in events {
            // Hits play a note, and so do the Note events a rule's "Play a note" block logs.
            if !matches!(e.kind, EventKind::Hit | EventKind::Note) {
                continue;
            }
            let time = e.time(rate);
            let frame = (time * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let n = native(&s, &chords, e, time, (cx.hit_index)(e), cx);
            if !n.play {
                continue;
            }
            out.push(note(&s, frame, &n));
        }
    }

    fn notes_with(
        &self,
        params: &Params,
        graph: Option<&ModuleGraph>,
        thing: ThingId,
        events: &[Event],
        cx: &MixCx,
        out: &mut Vec<Note>,
    ) {
        let Some(program) = Self::compiled(params, graph) else { return self.notes(params, thing, events, cx, out) };
        let s = NoteSound::from_params(params);
        let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
        let rate = cx.settings.steps_per_second.max(1);
        let half_width = cx.settings.half_width.max(1.0);
        // How many hits each ball has had so far, counting this one.
        let mut hits: BTreeMap<u32, u32> = BTreeMap::new();
        for e in events {
            if !matches!(e.kind, EventKind::Hit | EventKind::Note) {
                continue;
            }
            let count = hits.entry(e.a.item).or_default();
            if e.kind == EventKind::Hit {
                *count += 1;
            }
            let time = e.time(rate);
            let frame = (time * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let index = (cx.hit_index)(e);
            let n = native(&s, &chords, e, time, index, cx);
            let host = SoundHost { chords: &chords, seed: cx.settings.seed, time };
            let inputs = [
                Val::Ball(Some(Ball { thing: e.a.thing, item: e.a.item })),
                Val::Number(e.speed),
                Val::Number(index as f32),
                Val::Number(time as f32),
                Val::Number(e.point.x / half_width),
                Val::Number(e.point.y / half_width),
                Val::Number(*count as f32),
            ];
            let o = Vm::eval(&program, &host, time as f32, &inputs, index);
            if !o.boolean(0, n.play) {
                continue;
            }
            let played = Played {
                play: true,
                key: o.number(1, n.key as f32).round().clamp(0.0, 127.0) as u8,
                velocity: o.number(2, n.velocity).clamp(0.0, 1.0),
                volume: o.number(3, n.volume).clamp(-80.0, 24.0),
                pan: o.number(4, n.pan).clamp(-1.0, 1.0),
                decay: o.number(5, n.decay).clamp(0.01, 30.0),
                brightness: o.number(6, n.brightness).clamp(0.0, 1.0),
            };
            out.push(note(&s, frame, &played));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, mix_scene, new_scene};
    use bpe_core::ThingId;
    use bpe_runtime::{Event, EventKind, Log, Party};
    use glam::Vec2;

    fn hit(step: u64, x: f32, speed: f32) -> Event {
        Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(2), item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::new(x, 0.0),
            normal: Vec2::X,
            speed,
        }
    }

    #[test]
    fn a_hit_plays_a_note_at_its_frame_and_silence_stays_silent() {
        let kit = kit();
        let reg = &kit.registry;
        let scene = new_scene(reg);
        let resolved = bpe_core::resolve(&scene, reg);
        let mut log = Log::default();
        log.push(hit(240, -540.0, 500.0));
        let track = mix_scene(&scene, &resolved, &log, &kit);
        assert_eq!(track.frames(), 20 * 48_000);
        let before: f32 = track.samples[..47_990 * 2].iter().map(|s| s.abs()).sum();
        let after: f32 = track.samples[48_000 * 2..48_100 * 2].iter().map(|s| s.abs()).sum();
        assert_eq!(before, 0.0);
        assert!(after > 0.0);
        let (l, r): (f32, f32) = track.samples[48_000 * 2..49_000 * 2]
            .chunks(2)
            .fold((0.0, 0.0), |(l, r), f| (l + f[0].abs(), r + f[1].abs()));
        assert!(l > r, "a hit on the left should sound mostly on the left");
        assert!(track.samples.iter().all(|s| s.abs() <= 0.9));
        assert!(mix_scene(&scene, &resolved, &Log::default(), &kit).samples.iter().all(|&s| s == 0.0));
        assert_eq!(mix_scene(&scene, &resolved, &log, &kit).samples, track.samples);
    }

    #[test]
    fn a_note_action_plays_a_note() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push(Event { kind: EventKind::Note, b: None, ..hit(240, 0.0, 600.0) });
        let track = mix_scene(&scene, &resolved, &log, &kit);
        assert!(track.samples[48_000 * 2..49_000 * 2].iter().any(|s| s.abs() > 0.0));
        log.push(Event { kind: EventKind::Spawn, b: None, ..hit(480, 0.0, 600.0) });
        let with_spawn = mix_scene(&scene, &resolved, &log, &kit);
        assert_eq!(with_spawn.samples, track.samples, "only hits and notes make a sound");
    }

    #[test]
    fn quiet_hits_and_hits_past_the_end_are_silent() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push(hit(240, 0.0, 10.0)); // under the quietest hit of 20 px/s
        log.push(hit(240 * 25, 0.0, 500.0)); // after the 20 s end
        let track = mix_scene(&scene, &resolved, &log, &kit);
        assert!(track.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_sound_slot_that_is_off_plays_nothing() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.set_slot_off(ThingId(2), bpe_core::SlotId::Sound);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push(hit(240, 0.0, 500.0));
        assert!(mix_scene(&scene, &resolved, &log, &kit).samples.iter().all(|&s| s == 0.0));
    }
}
