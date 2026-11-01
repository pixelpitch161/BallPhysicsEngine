//! Turning hits into notes of the current chord. A module whose graph has been edited asks the
//! graph about every hit instead, with the native answer standing in for any output it leaves
//! unwired.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_4;

use bpe_core::{ModuleGraph, MusicKey, Params, Settings, SlotId, ThingId};
use bpe_mix::{ChordClock, MixCx, ModuleMix, Note, NoteInstrument, SAMPLE_RATE, candidates, mix64, voicing};
use bpe_runtime::{Event, EventKind, Party};
use bpe_vm::{Ball, Host, Program, Val, Vm};

use super::{NoteSound, Order, Plays, PlaysWhen};
use crate::graphs::{bind, compile_module};

pub struct NoteSoundMix;

/// What one hit plays: the native answer, and what the compiled path starts from.
struct Played {
    play: bool,
    /// Every note it plays at once, lowest first.
    keys: Vec<u8>,
    velocity: f32,
    volume: f32,
    pan: f32,
    /// The notes it could have picked from, for a graph asking for a chord note.
    candidates: Vec<u8>,
}

/// One event this sound speaks for, with what the mixer knows about it.
struct Heard<'a> {
    event: &'a Event,
    time: f64,
    /// The event's place in the whole log, for anything random.
    index: u64,
    /// How often the ball has hit so far, counting this one.
    count: u32,
}

/// Where this sound is in its progression and its arpeggio.
struct Picker {
    clock: ChordClock,
    key: MusicKey,
    /// Notes the arpeggio has played since the chord changed.
    arp: usize,
    /// The chord step the arpeggio started on.
    step: Option<u64>,
}

impl Picker {
    fn new(s: &NoteSound, cx: &MixCx) -> Picker {
        let p = cx.settings.progression(s.chords);
        Picker { clock: ChordClock::new(p, &cx.settings.beats), key: p.key, arp: 0, step: None }
    }
}

/// The native answer for one hit: whether it is loud enough, and which notes of the chord
/// playing at its video time it plays.
fn native(s: &NoteSound, picker: &mut Picker, h: &Heard, cx: &MixCx) -> Played {
    let span = (s.loudest_hit - s.quietest_hit).max(1.0);
    let mut velocity = ((h.event.speed - s.quietest_hit) / span).clamp(0.0, 1.0);
    let video = (cx.video_time)(h.time);
    let step = picker.clock.step(video);
    if picker.step != Some(step) {
        picker.step = Some(step);
        picker.arp = 0;
    }
    let chord = *picker.clock.at(video).1;
    let (low, high) = (s.low_note.min(127) as u8, s.high_note.min(127) as u8);
    let candidates = candidates(&chord, picker.key, s.picks_from, low, high);
    let loud = h.event.speed >= s.quietest_hit;
    let full = s.plays == Plays::FullChord && (s.full_chord_speed <= 0.0 || h.event.speed >= s.full_chord_speed);
    let keys = if full {
        velocity *= 0.8;
        voicing(&chord, low, high, s.chord_spread.clamp(1, 3) as u8)
    } else if s.plays == Plays::Root {
        let bass = bpe_mix::chords::bass_note(&chord);
        (low.min(high)..=low.max(high)).find(|k| k % 12 == bass).into_iter().collect()
    } else if candidates.is_empty() {
        Vec::new()
    } else {
        let n = candidates.len();
        let arp = picker.arp;
        let pick = match s.order {
            Order::Random => mix64(cx.settings.seed ^ h.index) as usize % n,
            Order::Speed => (velocity * (n - 1) as f32).round() as usize,
            Order::Hits => (h.count.max(1) as usize - 1) % n,
            Order::ArpUp => arp % n,
            Order::ArpDown => n - 1 - arp % n,
            Order::ArpUpDown if n == 1 => 0,
            Order::ArpUpDown => {
                let p = arp % (2 * n - 2);
                if p < n { p } else { 2 * n - 2 - p }
            }
            Order::Height => {
                let up = (1.0 - h.event.point.y / cx.settings.half_height.max(1.0)) * 0.5;
                (up.clamp(0.0, 1.0) * (n - 1) as f32).round() as usize
            }
        };
        vec![candidates[pick.min(n - 1)]]
    };
    let play = loud && !keys.is_empty();
    if play {
        picker.arp += 1;
    }
    Played { play, keys, velocity, volume: s.volume, pan: cx.pan_of(h.event, s.stereo_width), candidates }
}

fn notes(s: &NoteSound, frame: usize, hit: u64, p: &Played, out: &mut Vec<Note>) {
    let gain = p.velocity * p.velocity.sqrt() * 10f32.powf(p.volume / 20.0);
    let angle = (p.pan + 1.0) * FRAC_PI_4;
    out.extend(p.keys.iter().map(|&key| Note {
        frame,
        instrument: NoteInstrument::Library(s.instrument.0),
        key,
        velocity: p.velocity,
        left: gain * angle.cos(),
        right: gain * angle.sin(),
        channel: 0,
        hit: Some(hit),
    }));
}

/// Walks the events this sound speaks for, in order: the hits "Plays when" and "Only hits
/// against" ask for, and the Note events a rule logs for the thing's own balls. Each one goes
/// to `play` with its native answer, which says what plays, if anything; a note closer than
/// "Shortest gap" to the last one played is dropped first.
fn each_note(
    s: &NoteSound,
    thing: ThingId,
    events: &[Event],
    cx: &MixCx,
    mut play: impl FnMut(&Heard, Played) -> Option<Played>,
    out: &mut Vec<Note>,
) {
    let mut picker = Picker::new(s, cx);
    let rate = cx.settings.steps_per_second.max(1);
    // How often each ball has hit, as far as this thing has seen.
    let mut hits: BTreeMap<Party, u32> = BTreeMap::new();
    let mut last_note: Option<f64> = None;
    for e in events {
        if !matches!(e.kind, EventKind::Hit | EventKind::Note) {
            continue;
        }
        let hitting = e.a.thing == thing;
        let other = if hitting { e.b } else { Some(e.a) };
        let ball = if hitting { e.a } else { e.b.unwrap_or(e.a) };
        let count = hits.entry(ball).or_insert(0);
        if e.kind == EventKind::Hit {
            *count += 1;
        }
        let count = *count;
        if e.kind == EventKind::Hit {
            // Two balls of this thing hitting each other: the other one has hit too.
            if let Some(other) = other
                && other != ball
                && other.thing == thing
            {
                *hits.entry(other).or_insert(0) += 1;
            }
            let wanted = match s.plays_when {
                PlaysWhen::Hitting => hitting,
                PlaysWhen::BeingHit => !hitting,
                PlaysWhen::Both => true,
            };
            if !wanted {
                continue;
            }
            if let Some(other) = other
                && !(cx.matches)(&s.sounds_for, thing, other.thing)
            {
                continue;
            }
        } else if !hitting {
            // A rule's note is for the ball it names, whoever else hears about it.
            continue;
        }
        let time = e.time(rate);
        if let Some(last) = last_note
            && s.min_gap > 0.0
            && time - last < s.min_gap as f64
        {
            continue;
        }
        let frame = ((cx.video_time)(time) * SAMPLE_RATE as f64).round() as usize;
        if frame >= cx.frames {
            continue;
        }
        let heard = Heard { event: e, time, index: (cx.hit_index)(e), count };
        let n = native(s, &mut picker, &heard, cx);
        let Some(p) = play(&heard, n) else { continue };
        // Hits away from the camera's shot fade out.
        let fade = cx.gain_of(e);
        if fade <= 0.0 {
            continue;
        }
        last_note = Some(time);
        let first = out.len();
        notes(s, frame, heard.index, &p, out);
        if fade < 1.0 {
            for n in &mut out[first..] {
                n.left *= fade;
                n.right *= fade;
            }
        }
    }
}

/// A sound graph's view of the chords, for one hit.
struct SoundHost<'a> {
    candidates: &'a [u8],
    seed: u64,
}

impl Host for SoundHost<'_> {
    fn seed(&self) -> u64 {
        self.seed
    }

    fn chord_note(&self, _time: f32, index: u64) -> Option<f32> {
        let c = self.candidates;
        (!c.is_empty()).then(|| c[mix64(self.seed ^ index) as usize % c.len()] as f32)
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

    fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = NoteSound::from_params(params);
        each_note(&s, thing, events, cx, |_, n| n.play.then_some(n), out);
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
        let half_width = cx.settings.half_width.max(1.0);
        each_note(
            &s,
            thing,
            events,
            cx,
            |h, n| {
                let e = h.event;
                let host = SoundHost { candidates: &n.candidates, seed: cx.settings.seed };
                let inputs = [
                    Val::Ball(Some(Ball { thing: e.a.thing, item: e.a.item })),
                    Val::Number(e.speed),
                    Val::Number(h.index as f32),
                    Val::Number(h.time as f32),
                    Val::Number(e.point.x / half_width),
                    Val::Number(e.point.y / half_width),
                    Val::Number(h.count as f32),
                ];
                let o = Vm::eval(&program, &host, h.time as f32, &inputs, h.index);
                let first = n.keys.first().copied().unwrap_or(0);
                let key = o.number(1, first as f32).round().clamp(0.0, 127.0) as i32;
                let keys: Vec<u8> = if n.keys.is_empty() {
                    vec![key as u8]
                } else {
                    n.keys
                        .iter()
                        .map(|&k| k as i32 + key - first as i32)
                        .filter(|k| (0..=127).contains(k))
                        .map(|k| k as u8)
                        .collect()
                };
                o.boolean(0, n.play).then(|| Played {
                    play: true,
                    keys,
                    velocity: o.number(2, n.velocity).clamp(0.0, 1.0),
                    volume: o.number(3, n.volume).clamp(-80.0, 24.0),
                    pan: o.number(4, n.pan).clamp(-1.0, 1.0),
                    candidates: n.candidates.clone(),
                })
            },
            out,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NoteSound, Order, Plays};
    use super::NoteSoundMix;
    use crate::{kit, mix_scene, new_scene};
    use bpe_core::{
        Chord, Extension, Progression, ProgressionId, ProgressionRef, Quality, Settings, SlotId, Target, ThingId,
        Value, resolve, resolve_slot,
    };
    use bpe_mix::{MixCx, MixSettings, ModuleMix};
    use bpe_runtime::{Event, EventKind, Log, Party};
    use glam::Vec2;

    #[test]
    fn a_thing_being_hit_sounds_only_when_asked_and_only_for_its_targets() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push(hit(240, 0.0, 500.0)); // the ball (2) hits the arena (1)
        let plain = mix_scene(&scene, &resolved, &log, &kit);
        // The arena's inherited sound only speaks for its own hits, so a second sound that plays
        // when it is hit adds a note.
        let m = scene.add_module(&kit.registry, "sound.notes", "Wall sound").unwrap();
        let marimba = scene.add_instrument(crate::built_in_instrument(bpe_mix::Instrument::Marimba));
        let wall = &mut scene.library.modules.get_mut(&m).unwrap().params;
        wall.set("plays_when", Value::Choice("being_hit".into()));
        wall.set("instrument", Value::Int(marimba.0 as i64));
        scene.link(&kit.registry, ThingId(1), SlotId::Sound, m).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let both = mix_scene(&scene, &resolved, &log, &kit);
        assert!(both.samples != plain.samples, "the wall's own note is heard");
        // Pointing the wall's sound at a tag nothing has silences it again.
        scene.library.modules.get_mut(&m).unwrap().params.set("sounds_for", Value::Target(Target::Tag("pegs".into())));
        let resolved = resolve(&scene, &kit.registry);
        assert!(mix_scene(&scene, &resolved, &log, &kit).samples == plain.samples, "no tagged thing hit it");
        // Tagging the ball brings it back.
        scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("pegs".into());
        let resolved = resolve(&scene, &kit.registry);
        assert!(mix_scene(&scene, &resolved, &log, &kit).samples == both.samples, "the tagged ball hit it");
        // Hitting (the default) on the wall's sound stays silent: the wall hits nothing.
        scene.library.modules.get_mut(&m).unwrap().params.remove("plays_when");
        let resolved = resolve(&scene, &kit.registry);
        assert!(mix_scene(&scene, &resolved, &log, &kit).samples == plain.samples, "walls don't hit");
        // "Both" on the ball's own sound plays the ball's hit once, as the hitter only.
        scene.set_slot_inherit(ThingId(1), SlotId::Sound);
        let own = resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Sound, &mut Vec::new()).unwrap().module;
        scene.library.modules.get_mut(&own).unwrap().params.set("plays_when", Value::Choice("both".into()));
        let resolved = resolve(&scene, &kit.registry);
        // The same note, now on the ball's own channel: equal but for rounding in the loudness gain.
        let one = mix_scene(&scene, &resolved, &log, &kit);
        let diff = one.samples.iter().zip(&plain.samples).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(diff < 1e-6, "one note, not two ({diff})");
    }

    fn test_cx<'a>(settings: &'a MixSettings, log: &'a Log) -> MixCx<'a> {
        fn index(_: &Event) -> u64 {
            0
        }
        fn any(_: &Target, _: ThingId, _: ThingId) -> bool {
            true
        }
        fn same(t: f64) -> f64 {
            t
        }
        MixCx { settings, log, frames: 20 * 48_000, hit_index: &index, video_time: &same, matches: &any }
    }

    #[test]
    fn notes_can_climb_with_hits_and_quick_repeats_merge() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let m = resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Sound, &mut Vec::new()).unwrap().module;
        scene.library.modules.get_mut(&m).unwrap().params.set("order", Value::Choice("hits".into()));
        scene.library.modules.get_mut(&m).unwrap().params.set("low_note", Value::Int(60));
        scene.library.modules.get_mut(&m).unwrap().params.set("high_note", Value::Int(72));
        let resolved = resolve(&scene, &kit.registry);
        let mut log = Log::default();
        for step in [240, 480, 720] {
            log.push(hit(step, 0.0, 500.0));
        }
        let settings = crate::mix_settings(&scene, &resolved);
        let cx = test_cx(&settings, &log);
        let params = &resolved.things[&ThingId(2)].slots[&SlotId::Sound].params;
        let mut notes = Vec::new();
        NoteSoundMix.notes(params, ThingId(2), log.events(), &cx, &mut notes);
        assert_eq!(notes.len(), 3);
        let keys: Vec<u8> = notes.iter().map(|n| n.key).collect();
        assert!(keys[0] < keys[1] && keys[1] < keys[2], "{keys:?}");
        // By speed: a soft hit plays the lowest note, a hard one the highest.
        scene.library.modules.get_mut(&m).unwrap().params.set("order", Value::Choice("speed".into()));
        let resolved = resolve(&scene, &kit.registry);
        let params = &resolved.things[&ThingId(2)].slots[&SlotId::Sound].params;
        let mut soft_hard = Log::default();
        soft_hard.push(hit(240, 0.0, 30.0));
        soft_hard.push(hit(480, 0.0, 5000.0));
        let mut by_speed = Vec::new();
        NoteSoundMix.notes(params, ThingId(2), soft_hard.events(), &cx, &mut by_speed);
        assert!(by_speed[0].key < by_speed[1].key, "{:?}", by_speed.iter().map(|n| n.key).collect::<Vec<_>>());
        // A shortest gap of 1.5 s drops the middle of three notes a second apart.
        scene.library.modules.get_mut(&m).unwrap().params.set("min_gap", Value::Number(1.5));
        let resolved = resolve(&scene, &kit.registry);
        let params = &resolved.things[&ThingId(2)].slots[&SlotId::Sound].params;
        let mut few = Vec::new();
        NoteSoundMix.notes(params, ThingId(2), log.events(), &cx, &mut few);
        assert_eq!(few.len(), 2, "1.0 s, (2.0 s dropped), 3.0 s");
        assert_eq!((few[0].frame, few[1].frame), (48_000, 144_000));
    }

    fn progression(chords: &[(u8, Quality, Extension, Option<u8>, u16)]) -> Progression {
        Progression {
            chords: chords
                .iter()
                .map(|&(root, q, e, bass, beats)| Chord { bass, ..Chord::new(root, q, e, beats) })
                .collect(),
            ..Progression::default()
        }
    }

    /// Mix settings holding `p` as progression 0, on a beat every half second. `keys_of` plays
    /// a sound with no chords of its own on progression 0.
    fn with_chords(p: Progression) -> MixSettings {
        MixSettings {
            duration: 20.0,
            steps_per_second: 240,
            progressions: [(ProgressionId(0), p)].into(),
            chords: bpe_core::ProgressionRef(None),
            beats: (0..60).map(|b| b as f64 * 0.5).collect(),
            master: Default::default(),
            seed: 1,
            half_width: 540.0,
            half_height: 960.0,
            off_shot_fade: 0.0,
            tags: Default::default(),
            instruments: Default::default(),
            macros: Default::default(),
            strips: Default::default(),
            buses: Vec::new(),
            master_fx: Default::default(),
            channel_names: Default::default(),
        }
    }

    fn keys_of(s: &NoteSound, settings: &MixSettings, log: &Log) -> Vec<Vec<u8>> {
        let on_zero;
        let s = if s.chords.0.is_none() {
            on_zero = NoteSound { chords: ProgressionRef(Some(ProgressionId(0))), ..s.clone() };
            &on_zero
        } else {
            s
        };
        let cx = test_cx(settings, log);
        let mut notes = Vec::new();
        NoteSoundMix.notes(&s.to_params(), ThingId(2), log.events(), &cx, &mut notes);
        let mut by_frame: Vec<(usize, Vec<u8>)> = Vec::new();
        for n in notes {
            match by_frame.last_mut() {
                Some((f, keys)) if *f == n.frame => keys.push(n.key),
                _ => by_frame.push((n.frame, vec![n.key])),
            }
        }
        by_frame.into_iter().map(|(_, k)| k).collect()
    }

    /// Hits at these seconds (240 steps a second).
    fn hits_at(times: &[f64], speed: f32) -> Log {
        let mut log = Log::default();
        for &t in times {
            log.push(hit((t * 240.0).round() as u64, 0.0, speed));
        }
        log
    }

    fn c_triad(beats: u16) -> (u8, Quality, Extension, Option<u8>, u16) {
        (0, Quality::Major, Extension::Triad, None, beats)
    }

    #[test]
    fn notes_follow_the_chord_on_the_video_clock() {
        let settings = with_chords(progression(&[
            (9, Quality::Minor, Extension::Triad, None, 4),
            (5, Quality::Major, Extension::Triad, None, 4),
        ]));
        let log = hits_at(&[2.1], 500.0);
        let s = NoteSound { chords: ProgressionRef(Some(ProgressionId(0))), ..NoteSound::default() };
        let straight = keys_of(&s, &settings, &log);
        assert!([5, 9, 0].contains(&(straight[0][0] % 12)), "F chord at 2.1 s: {straight:?}");
        fn early(t: f64) -> f64 {
            t - 1.1
        }
        let cx = MixCx { video_time: &early, ..test_cx(&settings, &log) };
        let mut notes = Vec::new();
        NoteSoundMix.notes(&s.to_params(), ThingId(2), log.events(), &cx, &mut notes);
        assert!([9, 0, 4].contains(&(notes[0].key % 12)), "Am at video time 1.0 s: {}", notes[0].key);
    }

    #[test]
    fn a_module_can_use_its_own_progression() {
        let mut settings = with_chords(progression(&[(9, Quality::Minor, Extension::Triad, None, 4)]));
        settings.progressions.insert(ProgressionId(1), progression(&[(6, Quality::Major, Extension::Triad, None, 4)]));
        let log = hits_at(&[1.0, 2.0, 3.0, 4.0], 500.0);
        let scene_s = keys_of(&NoteSound::default(), &settings, &log);
        let own = NoteSound { chords: ProgressionRef(Some(ProgressionId(1))), ..NoteSound::default() };
        let own_s = keys_of(&own, &settings, &log);
        assert!(scene_s.iter().flatten().all(|k| [9, 0, 4].contains(&(k % 12))), "{scene_s:?}");
        assert!(own_s.iter().flatten().all(|k| [6, 10, 1].contains(&(k % 12))), "{own_s:?}");
        let missing = NoteSound { chords: ProgressionRef(Some(ProgressionId(7))), ..NoteSound::default() };
        let starter = with_chords(Progression::starter());
        assert_eq!(
            keys_of(&missing, &settings, &log),
            keys_of(&NoteSound::default(), &starter, &log),
            "a missing progression plays the starter"
        );
    }

    fn arp(order: Order, low: u32, high: u32) -> NoteSound {
        NoteSound { order, low_note: low, high_note: high, ..NoteSound::default() }
    }

    #[test]
    fn arp_up_climbs_and_wraps() {
        let settings = with_chords(progression(&[c_triad(64)]));
        let log = hits_at(&[0.1, 0.2, 0.3, 0.4, 0.5], 500.0);
        let keys: Vec<u8> = keys_of(&arp(Order::ArpUp, 60, 67), &settings, &log).concat();
        assert_eq!(keys, [60, 64, 67, 60, 64]);
        let down: Vec<u8> = keys_of(&arp(Order::ArpDown, 60, 67), &settings, &log).concat();
        assert_eq!(down, [67, 64, 60, 67, 64]);
    }

    #[test]
    fn arp_up_down_bounces_without_repeating_ends() {
        let settings = with_chords(progression(&[c_triad(64)]));
        let log = hits_at(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 500.0);
        let keys: Vec<u8> = keys_of(&arp(Order::ArpUpDown, 60, 67), &settings, &log).concat();
        assert_eq!(keys, [60, 64, 67, 64, 60, 64]);
    }

    #[test]
    fn the_arp_restarts_on_a_new_chord() {
        let settings = with_chords(progression(&[c_triad(1), (5, Quality::Major, Extension::Triad, None, 1)]));
        let log = hits_at(&[0.1, 0.3, 0.6], 500.0);
        let keys: Vec<u8> = keys_of(&arp(Order::ArpUp, 60, 72), &settings, &log).concat();
        assert_eq!(keys, [60, 64, 60], "F's lowest note in range is C again (60)");
    }

    #[test]
    fn root_only_plays_the_bass_note() {
        let settings = with_chords(progression(&[(0, Quality::Major, Extension::Maj7, Some(4), 8)]));
        let s = NoteSound { plays: Plays::Root, low_note: 36, high_note: 60, ..NoteSound::default() };
        assert_eq!(keys_of(&s, &settings, &hits_at(&[1.0], 500.0)), [vec![40]]);
    }

    #[test]
    fn full_chord_above_a_speed() {
        let settings = with_chords(progression(&[c_triad(64)]));
        let s = NoteSound {
            plays: Plays::FullChord,
            full_chord_speed: 500.0,
            low_note: 48,
            high_note: 84,
            ..NoteSound::default()
        };
        assert_eq!(keys_of(&s, &settings, &hits_at(&[1.0], 400.0))[0].len(), 1);
        assert_eq!(keys_of(&s, &settings, &hits_at(&[1.0], 600.0)), [vec![48, 52, 55]]);
    }

    #[test]
    fn full_chord_in_a_tiny_range_is_silent_when_nothing_fits() {
        let settings = with_chords(progression(&[c_triad(64)]));
        let s = NoteSound { plays: Plays::FullChord, low_note: 61, high_note: 62, ..NoteSound::default() };
        assert!(keys_of(&s, &settings, &hits_at(&[1.0], 600.0)).is_empty());
    }

    #[test]
    fn height_order_gives_high_notes_at_the_top() {
        let settings = with_chords(progression(&[c_triad(64)]));
        let s = arp(Order::Height, 60, 72);
        let mut log = Log::default();
        log.push(Event { point: Vec2::new(0.0, -960.0), ..hit(240, 0.0, 500.0) });
        log.push(Event { point: Vec2::new(0.0, 960.0), ..hit(480, 0.0, 500.0) });
        assert_eq!(keys_of(&s, &settings, &log), [vec![72], vec![60]]);
    }

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

    /// The (left, right) gains of the note one hit at world x makes, with the camera at
    /// `camera` and Off-shot fade `fade`; silence when it plays none.
    fn heard_at(x: f32, camera: bpe_runtime::CameraView, fade: f32) -> (f32, f32) {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push_camera(0, camera);
        log.push(hit(240, x, 900.0));
        let mut settings = crate::mix_settings(&scene, &resolved);
        settings.off_shot_fade = fade;
        let cx = test_cx(&settings, &log);
        let params = &resolved.things[&ThingId(2)].slots[&SlotId::Sound].params;
        let mut notes = Vec::new();
        NoteSoundMix.notes(params, ThingId(2), log.events(), &cx, &mut notes);
        notes.pop().map_or((0.0, 0.0), |n| (n.left, n.right))
    }

    #[test]
    fn off_shot_hits_fade() {
        let still = bpe_runtime::CameraView::default();
        // Constant-power panning: the loudness is the same wherever the note sits.
        let loud = |(l, r): (f32, f32)| (l * l + r * r).sqrt();
        let near = heard_at(0.0, still, 1.5);
        let far = heard_at(1500.0, still, 1.5);
        assert!(loud(far) > 0.0 && loud(far) < loud(near), "{far:?} vs {near:?}");
        assert_eq!(loud(heard_at(30_000.0, still, 1.5)), 0.0);
        let off = heard_at(3000.0, still, 0.0);
        let centred = bpe_runtime::CameraView { center: glam::Vec2::new(3000.0, 0.0), ..still };
        assert!(
            (loud(off) - loud(heard_at(3000.0, centred, 1.5))).abs() < 1e-5,
            "fade off, or in the shot: full volume"
        );
    }

    #[test]
    fn pan_uses_screen_position() {
        let centred = bpe_runtime::CameraView { center: glam::Vec2::new(2000.0, 0.0), zoom: 1.0, turn: 0.0 };
        let (l, r) = heard_at(2000.0, centred, 0.0);
        assert!((l - r).abs() < 1e-4, "{l} {r}");
        let (l, r) = heard_at(2000.0, bpe_runtime::CameraView::default(), 0.0);
        assert!(r > l, "off to the right of the shot: {l} {r}");
    }
}
