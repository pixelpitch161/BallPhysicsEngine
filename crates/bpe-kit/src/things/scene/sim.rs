//! Scene defaults in the world: the step rate and gravity, the beat grid and the song's levels
//! for pulses, and a Beat event on every beat.

use std::sync::Arc;

use bpe_core::{PulseInputs, ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party, ThingSim, World, state_mut};
use glam::Vec2;

use super::SceneSettings;
use crate::music::{bands_of, beats_for, load_song};

pub struct SceneSim;

/// What the scene keeps between steps: its beats and which one is next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneState {
    pub beats: Vec<f64>,
    pub next_beat: usize,
}

impl ThingSim for SceneSim {
    fn kind(&self) -> &'static str {
        "scene"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = SceneSettings::from_params(&resolved.params);
        world.steps_per_second = s.steps_per_second.max(1);
        world.gravity = s.gravity;
        world.seed = s.seed as u64;
        world.video = glam::Vec2::new(s.width.max(1) as f32, s.height.max(1) as f32);
        let beats = beats_for(&s);
        let song = load_song(&s.song);
        if !beats.is_empty() || song.is_some() {
            let bands = song.as_ref().map(|song| bands_of(&song.features, s.song_offset));
            world.pulse_inputs = Some(Arc::new(PulseInputs { beats: beats.clone(), bands }));
        }
        world.states.insert(thing.id, Box::new(SceneState { beats, next_beat: 0 }));
    }

    /// Logs a Beat for every beat the step reached.
    fn after_step(&self, thing: ThingId, world: &mut World, log: &mut Log) {
        let time = world.time();
        let step = world.step;
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<SceneState>(st.as_mut());
        while st.next_beat < st.beats.len() && st.beats[st.next_beat] <= time + 1e-9 {
            log.push(Event {
                step,
                kind: EventKind::Beat,
                a: Party { thing, item: st.next_beat as u32 },
                b: None,
                point: Vec2::ZERO,
                normal: Vec2::ZERO,
                speed: 0.0,
            });
            st.next_beat += 1;
        }
    }

    /// A keyframe moved gravity.
    fn settings_changed(&self, _thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        world.gravity = SceneSettings::from_params(&resolved.params).gravity;
    }

    fn affects_physics(&self, key: &str) -> bool {
        matches!(
            key,
            "gravity"
                | "steps_per_second"
                | "duration"
                | "seed"
                | "bpm"
                | "beat_sync"
                | "song"
                | "song_offset"
                | "subdivisions"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, new_scene};
    use bpe_core::{Value, resolve};

    #[test]
    fn the_scene_seed_reaches_the_world() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("seed", Value::Int(77));
        let resolved = resolve(&scene, &kit.registry);
        assert_eq!(World::build(&scene, &resolved, &kit.sims).seed, 77);
    }

    /// Random blocks and chance rules read the seed, so a new seed is a new clip.
    #[test]
    fn changing_the_seed_rebuilds_the_world() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let before = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("seed", Value::Int(77));
        let resolved = resolve(&scene, &kit.registry);
        assert_ne!(bpe_runtime::fingerprint(&scene, &resolved, &kit.sims), before);
    }

    #[test]
    fn a_tempo_logs_a_beat_event_every_half_second() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("bpm", Value::Number(120.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(world.pulse_inputs.as_ref().map(|p| p.beats.len()), Some(42));
        let mut log = Log::default();
        world.advance_to(2.0, &kit.sims, &mut log);
        let beats: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Beat).collect();
        assert_eq!(beats.len(), 5, "0, 0.5, 1, 1.5 and 2 s");
        assert_eq!(beats[0].step, 1, "the beat at 0 is logged by the first step");
        assert_eq!(beats[2].step, 240);
        assert_eq!(beats[4].a, Party { thing: ThingId::ROOT, item: 4 });
        assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step));
    }

    #[test]
    fn a_scene_without_music_logs_no_beats() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        assert!(world.pulse_inputs.is_none());
        let mut log = Log::default();
        world.advance_to(2.0, &kit.sims, &mut log);
        assert!(log.events().iter().all(|e| e.kind != EventKind::Beat));
    }
}
