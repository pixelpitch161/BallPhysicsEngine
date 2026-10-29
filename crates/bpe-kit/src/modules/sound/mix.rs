//! Turning hits into notes of the current chord.

use std::f32::consts::FRAC_PI_4;

use bpe_core::{Params, Settings, ThingId};
use bpe_mix::{Chords, MixCx, ModuleMix, Note, SAMPLE_RATE, mix64};
use bpe_runtime::{Event, EventKind};

use super::NoteSound;

pub struct NoteSoundMix;

impl ModuleMix for NoteSoundMix {
    fn kind(&self) -> &'static str {
        "sound.notes"
    }

    fn notes(&self, params: &Params, _thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = NoteSound::from_params(params);
        let chords = Chords::parse(&cx.settings.chords, s.low_note.min(127) as u8, s.high_note.min(127) as u8);
        let rate = cx.settings.steps_per_second.max(1);
        let gain_db = 10f32.powf(s.volume / 20.0);
        let span = (s.loudest_hit - s.quietest_hit).max(1.0);
        for e in events {
            if e.kind != EventKind::Hit || e.speed < s.quietest_hit {
                continue;
            }
            let time = e.time(rate);
            let frame = ((cx.video_time)(time) * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let candidates = chords.notes_at(time);
            if candidates.is_empty() {
                continue;
            }
            let pick = mix64(cx.settings.seed ^ (cx.hit_index)(e)) as usize % candidates.len();
            let velocity = ((e.speed - s.quietest_hit) / span).clamp(0.0, 1.0);
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
