//! Turning hits and beats into drum notes.

use std::f32::consts::FRAC_PI_4;

use bpe_core::{Params, Settings, ThingId};
use bpe_mix::{Instrument, MixCx, ModuleMix, Note, NoteInstrument, SAMPLE_RATE};
use bpe_runtime::{Event, EventKind};

use super::{DrumSound, Piece};

pub struct DrumSoundMix;

/// Which drum a hit of `share` (0 to 1 of the way from the quietest to the loudest hit) plays
/// when the piece is Auto: a hi-hat, a snare, or a kick.
fn auto_hit(share: f32) -> u8 {
    if share < 1.0 / 3.0 {
        42
    } else if share < 2.0 / 3.0 {
        38
    } else {
        36
    }
}

/// Which drum beat number `item` plays when the piece is Auto: a kick on the one, hi-hats
/// between.
fn auto_beat(item: u32) -> u8 {
    if item.is_multiple_of(4) { 36 } else { 42 }
}

impl ModuleMix for DrumSoundMix {
    fn kind(&self) -> &'static str {
        "sound.drums"
    }

    fn notes(&self, params: &Params, _thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
        let s = DrumSound::from_params(params);
        let rate = cx.settings.steps_per_second.max(1);
        let span = (s.loudest_hit - s.quietest_hit).max(1.0);
        let hit_gain = 10f32.powf(s.volume / 20.0);
        let beat_gain = 10f32.powf(s.beat_volume / 20.0);
        for e in events {
            let (key, velocity, gain, pan) = match e.kind {
                EventKind::Hit if e.speed >= s.quietest_hit => {
                    let share = ((e.speed - s.quietest_hit) / span).clamp(0.0, 1.0);
                    let key = match s.piece {
                        Piece::Auto => auto_hit(share),
                        Piece::Silent => continue,
                        other => other.key().unwrap_or(42),
                    };
                    let velocity = 0.4 + 0.6 * share;
                    let fade = cx.gain_of(e);
                    if fade <= 0.0 {
                        continue;
                    }
                    let gain = (0.5 + 0.5 * share) * hit_gain * fade;
                    let pan = cx.pan_of(e, s.stereo_width);
                    (key, velocity, gain, pan)
                }
                EventKind::Beat => {
                    let key = match s.on_beat {
                        Piece::Auto => auto_beat(e.a.item),
                        Piece::Silent => continue,
                        other => other.key().unwrap_or(42),
                    };
                    (key, 0.9, beat_gain, 0.0)
                }
                _ => continue,
            };
            // Beats are on the video's clock already (the grid is the song's), so only hits, which
            // are moments of the run, go through the warp.
            let at = if e.kind == EventKind::Beat { e.time(rate) } else { (cx.video_time)(e.time(rate)) };
            let frame = (at * SAMPLE_RATE as f64).round() as usize;
            if frame >= cx.frames {
                continue;
            }
            let angle = (pan + 1.0) * FRAC_PI_4;
            out.push(Note {
                frame,
                instrument: NoteInstrument::BuiltIn(Instrument::Drums),
                key,
                velocity,
                left: gain * angle.cos(),
                right: gain * angle.sin(),
                channel: 0,
                hit: (e.kind != EventKind::Beat).then(|| (cx.hit_index)(e)),
            });
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

    fn event(kind: EventKind, thing: ThingId, item: u32, step: u64, speed: f32) -> Event {
        Event { step, kind, a: Party { thing, item }, b: None, point: Vec2::ZERO, normal: Vec2::Y, speed }
    }

    /// The new scene with a drum machine as the shared Sound module, its "on_beat" set.
    fn drum_scene(on_beat: &str) -> (bpe_core::Scene, bpe_core::Resolved) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let drums = scene.add_module(&kit.registry, "sound.drums", "Drums").unwrap();
        scene.library.modules.get_mut(&drums).unwrap().params.set("on_beat", Value::Choice(on_beat.into()));
        scene.link(&kit.registry, ThingId::ROOT, SlotId::Sound, drums).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved)
    }

    #[test]
    fn hits_pick_a_drum_by_speed_and_beats_play_the_beat_drum() {
        let kit = kit();
        let (scene, resolved) = drum_scene("kick");
        let mut log = Log::default();
        log.push(event(EventKind::Beat, ThingId::ROOT, 0, 1, 0.0));
        log.push(event(EventKind::Hit, ThingId(2), 0, 240, 60.0));
        log.push(event(EventKind::Hit, ThingId(2), 0, 480, 900.0));
        log.push(event(EventKind::Beat, ThingId::ROOT, 1, 720, 0.0));
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &log,
            &kit.mixes,
            None,
            &|t| t,
        );
        let keys: Vec<(usize, u8)> = notes.iter().map(|n| (n.frame, n.key)).collect();
        assert_eq!(keys, [(200, 36), (144_000, 36), (48_000, 42), (96_000, 36)], "root's beats first, then the ball");
        assert!(notes.iter().all(|n| n.instrument == NoteInstrument::BuiltIn(Instrument::Drums)));
        assert!((notes[0].left - notes[0].right).abs() < 1e-6, "beats are centred");
        assert!(notes[2].velocity < notes[3].velocity);
        // Auto beats alternate a kick on the one with hi-hats.
        assert_eq!((auto_beat(0), auto_beat(1), auto_beat(4)), (36, 42, 36));
        assert_eq!((auto_hit(0.0), auto_hit(0.5), auto_hit(1.0)), (42, 38, 36));
    }

    #[test]
    fn beat_drums_stay_on_the_beat_while_hits_go_through_the_warp() {
        let kit = kit();
        let (scene, resolved) = drum_scene("kick");
        let mut log = Log::default();
        log.push(event(EventKind::Hit, ThingId(2), 0, 240, 500.0));
        log.push(event(EventKind::Beat, ThingId::ROOT, 0, 720, 0.0));
        let stretched = |t: f64| t * 2.0;
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &log,
            &kit.mixes,
            None,
            &stretched,
        );
        let frames: Vec<usize> = notes.iter().map(|n| n.frame).collect();
        assert_eq!(frames, [144_000, 96_000], "the beat at 3 s stays there; the hit at 1 s plays at 2 s");
    }

    #[test]
    fn silent_pieces_play_nothing() {
        let kit = kit();
        let (mut scene, _) = drum_scene("silent");
        let drums = *scene.library.modules.keys().last().unwrap();
        scene.library.modules.get_mut(&drums).unwrap().params.set("piece", Value::Choice("silent".into()));
        let resolved = resolve(&scene, &kit.registry);
        let mut log = Log::default();
        log.push(event(EventKind::Beat, ThingId::ROOT, 0, 1, 0.0));
        log.push(event(EventKind::Hit, ThingId(2), 0, 240, 500.0));
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
        // A named piece plays that drum for every hit.
        scene.library.modules.get_mut(&drums).unwrap().params.set("piece", Value::Choice("clap".into()));
        let resolved = resolve(&scene, &kit.registry);
        let notes = bpe_mix::notes_with(
            &mix_settings(&scene, &resolved),
            &resolved,
            &scene.library,
            &log,
            &kit.mixes,
            None,
            &|t| t,
        );
        assert_eq!(notes.iter().map(|n| n.key).collect::<Vec<_>>(), [39]);
    }
}
