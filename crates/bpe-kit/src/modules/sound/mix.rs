//! Turning hits into notes of the current chord.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_4;

use bpe_core::{Params, Settings, ThingId};
use bpe_mix::{Chords, MixCx, ModuleMix, Note, SAMPLE_RATE, mix64};
use bpe_runtime::{Event, EventKind, Party};

use super::{NoteSound, PitchBy, PlaysWhen};

pub struct NoteSoundMix;

impl ModuleMix for NoteSoundMix {
    fn kind(&self) -> &'static str {
        "sound.notes"
    }

    fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = NoteSound::from_params(params);
        let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
        let rate = cx.settings.steps_per_second.max(1);
        let gain_db = 10f32.powf(s.volume / 20.0);
        let span = (s.loudest_hit - s.quietest_hit).max(1.0);
        // How often each ball has hit, as far as this thing has seen.
        let mut hits: BTreeMap<Party, u32> = BTreeMap::new();
        let mut last_note: Option<f64> = None;
        for e in events {
            if e.kind != EventKind::Hit {
                continue;
            }
            let hitting = e.a.thing == thing;
            let other = if hitting { e.b } else { Some(e.a) };
            let ball = if hitting { e.a } else { e.b.unwrap_or(e.a) };
            let count = hits.entry(ball).or_insert(0);
            *count += 1;
            let count = *count;
            let wanted = match s.plays_when {
                PlaysWhen::Hitting => hitting,
                PlaysWhen::BeingHit => !hitting,
                PlaysWhen::Both => true,
            };
            if !wanted || e.speed < s.quietest_hit {
                continue;
            }
            if let Some(other) = other
                && !(cx.matches)(&s.sounds_for, thing, other.thing)
            {
                continue;
            }
            let time = e.time(rate);
            if let Some(last) = last_note
                && s.min_gap > 0.0
                && time - last < s.min_gap as f64
            {
                continue;
            }
            let frame = (time * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let candidates = chords.notes_at(time);
            if candidates.is_empty() {
                continue;
            }
            let velocity = ((e.speed - s.quietest_hit) / span).clamp(0.0, 1.0);
            let pick = match s.pitch_by {
                PitchBy::Random => mix64(cx.settings.seed ^ (cx.hit_index)(e)) as usize % candidates.len(),
                PitchBy::Speed => (velocity * (candidates.len() - 1) as f32).round() as usize,
                PitchBy::Hits => (count as usize - 1) % candidates.len(),
            };
            last_note = Some(time);
            let gain = velocity * velocity.sqrt() * gain_db;
            let pan = (e.point.x / cx.settings.half_width.max(1.0)).clamp(-1.0, 1.0) * s.stereo_width;
            let angle = (pan + 1.0) * FRAC_PI_4;
            out.push(Note {
                frame,
                instrument: s.instrument,
                key: candidates[pick],
                velocity,
                left: gain * angle.cos(),
                right: gain * angle.sin(),
                decay: s.decay,
                brightness: s.brightness,
            });
        }
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
        let _ = log;
        MixCx { settings, frames: 20 * 48_000, hit_index: &index, matches: &any }
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
