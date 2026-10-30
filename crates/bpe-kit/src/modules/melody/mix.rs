//! Turning a thing's hits into the steps of its tune, in order, round and round.

use std::f32::consts::FRAC_PI_4;

use bpe_core::{Params, Settings, ThingId};
use bpe_mix::{MixCx, ModuleMix, Note, NoteInstrument, SAMPLE_RATE};
use bpe_runtime::{Event, EventKind};

use super::{MelodySound, load_tune};

pub struct MelodySoundMix;

impl ModuleMix for MelodySoundMix {
    fn kind(&self) -> &'static str {
        "sound.melody"
    }

    fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = MelodySound::from_params(params);
        let Some(tune) = load_tune(&s.file) else { return };
        let part = (s.track > 0).then(|| s.track - 1);
        let steps = tune.steps(part, s.transpose);
        if steps.is_empty() {
            return;
        }
        // The thing's hits in log order, so each hit knows which step of the tune it is.
        let hits: Vec<&Event> =
            cx.log.events().iter().filter(|e| e.a.thing == thing && e.kind == EventKind::Hit).collect();
        let rate = cx.settings.steps_per_second.max(1);
        let span = (s.loudest_hit - s.quietest_hit).max(1.0);
        let gain_db = 10f32.powf(s.volume / 20.0);
        for e in events {
            if e.kind != EventKind::Hit || e.speed < s.quietest_hit {
                continue;
            }
            let frame = ((cx.video_time)(e.time(rate)) * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let first = hits.partition_point(|h| h.step < e.step);
            let Some(k) = hits[first..].iter().position(|h| *h == e).map(|i| first + i) else { continue };
            let step = &steps[k % steps.len()];
            let velocity = if s.velocity_from_hit {
                ((e.speed - s.quietest_hit) / span).clamp(0.0, 1.0)
            } else {
                step.velocity as f32 / 127.0
            };
            let fade = cx.gain_of(e);
            if fade <= 0.0 {
                continue;
            }
            let gain = (0.3 + 0.7 * velocity) * gain_db * fade;
            let pan = cx.pan_of(e, s.stereo_width);
            let angle = (pan + 1.0) * FRAC_PI_4;
            let keys: &[u8] = if s.chords { &step.keys } else { &step.keys[..1] };
            let share = 1.0 / (keys.len() as f32).sqrt();
            for &key in keys {
                out.push(Note {
                    frame,
                    instrument: NoteInstrument::Library(s.instrument.0),
                    key,
                    velocity,
                    left: gain * share * angle.cos(),
                    right: gain * share * angle.sin(),
                    channel: 0,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, mix_settings, new_scene};
    use bpe_core::{SlotId, Value, resolve};
    use bpe_runtime::{Log, Party};
    use glam::Vec2;

    fn hit(step: u64, speed: f32) -> Event {
        Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(2), item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed,
        }
    }

    fn melody_scene(file: &str) -> (bpe_core::Scene, bpe_core::Resolved) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let melody = scene.add_module(&kit.registry, "sound.melody", "Melody").unwrap();
        scene.library.modules.get_mut(&melody).unwrap().params.set("file", Value::Text(file.into()));
        scene.link(&kit.registry, ThingId::ROOT, SlotId::Sound, melody).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved)
    }

    #[test]
    fn hits_walk_through_the_tune_and_loop() {
        let kit = kit();
        let (scene, resolved) = melody_scene("twinkle");
        let tune = load_tune("builtin:twinkle").unwrap();
        let steps = tune.steps(None, 0);
        assert!(steps.len() > 4);
        let mut log = Log::default();
        for k in 0..=steps.len() {
            log.push(hit(24 * (k as u64 + 1), 400.0));
        }
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &log,
            &kit.mixes,
            None,
            &|t| t,
        );
        let keys_at = |frame: usize| -> Vec<u8> { notes.iter().filter(|n| n.frame == frame).map(|n| n.key).collect() };
        assert_eq!(keys_at(4_800), steps[0].keys);
        assert_eq!(keys_at(9_600), steps[1].keys);
        assert_eq!(keys_at(4_800 * (steps.len() + 1)), steps[0].keys, "the tune starts over");
        assert!(notes.iter().all(|n| n.instrument == bpe_mix::NoteInstrument::Library(None)));
        // A quiet hit plays nothing but still counts as a step? No: it is skipped, yet the
        // following hit keeps its place in the tune, so a melody never drifts with volume.
        let mut quiet = Log::default();
        quiet.push(hit(240, 400.0));
        quiet.push(hit(480, 5.0));
        quiet.push(hit(720, 400.0));
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &quiet,
            &kit.mixes,
            None,
            &|t| t,
        );
        assert_eq!(notes.iter().filter(|n| n.frame == 96_000).count(), 0);
        let third: Vec<u8> = notes.iter().filter(|n| n.frame == 144_000).map(|n| n.key).collect();
        assert_eq!(third, steps[2].keys);
    }

    #[test]
    fn a_missing_file_plays_nothing() {
        let kit = kit();
        let (scene, resolved) = melody_scene("/nowhere/tune.mid");
        let mut log = Log::default();
        log.push(hit(240, 400.0));
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &log,
            &kit.mixes,
            None,
            &|t| t,
        );
        assert!(notes.is_empty());
        assert!(load_tune("").is_none());
        assert!(load_tune("builtin:nothing-here").is_none());
        assert!(load_tune("ode-to-joy").is_some(), "a bare built-in name works");
    }
}
