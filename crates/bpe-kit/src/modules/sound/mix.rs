//! Turning hits into notes of the current chord. A module whose graph has been edited asks the
//! graph about every hit instead, with the native answer standing in for any output it leaves
//! unwired.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_4;

use bpe_core::{ModuleGraph, Params, Settings, SlotId, ThingId};
use bpe_mix::{Chords, MixCx, ModuleMix, Note, SAMPLE_RATE, mix64};
use bpe_runtime::{Event, EventKind, Party};
use bpe_vm::{Ball, Host, Program, Val, Vm};

use super::{NoteSound, PitchBy, PlaysWhen};
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

/// One event this sound speaks for, with what the mixer knows about it.
struct Heard<'a> {
    event: &'a Event,
    time: f64,
    /// The event's place in the whole log, for anything random.
    index: u64,
    /// How often the ball has hit so far, counting this one.
    count: u32,
}

/// The native answer for one hit: whether it is loud enough, which note of the chord it plays
/// (at random, climbing with the hit's speed or with the ball's hits) and how.
fn native(s: &NoteSound, chords: &Chords, h: &Heard, cx: &MixCx) -> Played {
    let span = (s.loudest_hit - s.quietest_hit).max(1.0);
    let velocity = ((h.event.speed - s.quietest_hit) / span).clamp(0.0, 1.0);
    let candidates = chords.notes_at(h.time);
    let key = if candidates.is_empty() {
        0
    } else {
        let pick = match s.pitch_by {
            PitchBy::Random => mix64(cx.settings.seed ^ h.index) as usize % candidates.len(),
            PitchBy::Speed => (velocity * (candidates.len() - 1) as f32).round() as usize,
            PitchBy::Hits => (h.count.max(1) as usize - 1) % candidates.len(),
        };
        candidates[pick]
    };
    Played {
        play: h.event.speed >= s.quietest_hit && !candidates.is_empty(),
        key,
        velocity,
        volume: s.volume,
        pan: cx.pan_of(h.event, s.stereo_width),
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
    let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
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
        let n = native(s, &chords, &heard, cx);
        let Some(p) = play(&heard, n) else { continue };
        // Hits away from the camera's shot fade out.
        let fade = cx.gain_of(e);
        if fade <= 0.0 {
            continue;
        }
        last_note = Some(time);
        let mut n = note(s, frame, &p);
        if fade < 1.0 {
            n.left *= fade;
            n.right *= fade;
        }
        out.push(n);
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
        let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
        let half_width = cx.settings.half_width.max(1.0);
        each_note(
            &s,
            thing,
            events,
            cx,
            |h, n| {
                let e = h.event;
                let host = SoundHost { chords: &chords, seed: cx.settings.seed, time: h.time };
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
                o.boolean(0, n.play).then(|| Played {
                    play: true,
                    key: o.number(1, n.key as f32).round().clamp(0.0, 127.0) as u8,
                    velocity: o.number(2, n.velocity).clamp(0.0, 1.0),
                    volume: o.number(3, n.volume).clamp(-80.0, 24.0),
                    pan: o.number(4, n.pan).clamp(-1.0, 1.0),
                    decay: o.number(5, n.decay).clamp(0.01, 30.0),
                    brightness: o.number(6, n.brightness).clamp(0.0, 1.0),
                })
            },
            out,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::NoteSoundMix;
    use crate::{kit, mix_scene, new_scene};
    use bpe_core::{SlotId, Target, ThingId, Value, resolve, resolve_slot};
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
        let wall = &mut scene.library.modules.get_mut(&m).unwrap().params;
        wall.set("plays_when", Value::Choice("being_hit".into()));
        wall.set("instrument", Value::Choice("marimba".into()));
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
        assert!(mix_scene(&scene, &resolved, &log, &kit).samples == plain.samples, "one note, not two");
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
        scene.library.modules.get_mut(&m).unwrap().params.set("pitch_by", Value::Choice("hits".into()));
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
        scene.library.modules.get_mut(&m).unwrap().params.set("pitch_by", Value::Choice("speed".into()));
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
