//! Scene settings in the world: the step rate, gravity, the seed and the video size, and the
//! tempo for things born on the beat (`World.beat_seconds`, `World.beats_in_bar`), and the air's
//! settings when the scene has an Air card (a sleeping [`crate::air::Air`]: a Particles thing that
//! reads it wakes it).

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{ThingSim, World};

use super::SceneSettings;

pub struct SceneSim;

impl ThingSim for SceneSim {
    fn kind(&self) -> &'static str {
        "scene"
    }

    fn build(&self, _thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let s = SceneSettings::from_params(&resolved.params);
        world.steps_per_second = s.steps_per_second.max(1);
        world.gravity = s.gravity;
        world.seed = s.seed as u64;
        world.video = glam::Vec2::new(s.width.max(1) as f32, s.height.max(1) as f32);
        let (beat, per_bar) = crate::things::rig::pattern::beat_seconds(&s);
        (world.beat_seconds, world.beats_in_bar) = (beat, per_bar);
        world.first_beat = super::first_beat_seconds(s.first_beat);
        if let Some(spec) = crate::modules::air::spec_of(resolved) {
            world.states.insert(ThingId::ROOT, Box::new(crate::air::Air::new(spec)));
        }
    }

    /// Haze that moves with the air drifts as far as the playback's air has carried it (W13): the
    /// Air card and its keys then count in every world, so an edit of them starts the playback
    /// again and the haze follows the wind as it is now.
    fn reads_air(&self, resolved: &ResolvedThing) -> bool {
        use crate::modules::lighting::{HazeMoves, LightingSettings};
        resolved.slots.contains_key(&SlotId::Air)
            && resolved
                .slots
                .get(&SlotId::Lighting)
                .is_some_and(|l| LightingSettings::from_params(&l.params).haze_moves == HazeMoves::WithTheAir)
    }

    /// The wind carries the air on: what moving haze drifts by (W13). Cheap, and never physics.
    fn after_step(&self, _thing: ThingId, world: &mut World, _log: &mut bpe_runtime::Log) {
        let (time, dt) = (world.time() as f32, world.sim_dt());
        if let Some(air) = crate::air::air_mut(world) {
            let carried = air.spec.ambient(glam::Vec2::ZERO, time) * dt;
            if carried.is_finite() {
                air.drift += carried;
            }
        }
    }

    /// A keyframe moved gravity, or one of the Air card's settings.
    fn settings_changed(&self, _thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        world.gravity = SceneSettings::from_params(&resolved.params).gravity;
        if let (Some(spec), Some(air)) = (crate::modules::air::spec_of(resolved), crate::air::air_mut(world))
            && air.spec != spec
        {
            air.spec = spec;
        }
    }

    /// The video's size counts too: the camera's Stay inside, Slack and Fit all read it.
    fn affects_physics(&self, key: &str) -> bool {
        matches!(key, "gravity" | "steps_per_second" | "duration" | "seed" | "width" | "height")
    }

    /// Particles born or pushed on the beat read the tempo.
    fn affects_beside(&self, key: &str) -> bool {
        matches!(key, "tempo" | "time_top" | "time_bottom" | "first_beat")
    }
}

/// The scene's tempo as the world has it: seconds in a beat and beats in a bar, the same as the
/// sound's `TempoMap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBeat {
    pub beat: f64,
    pub per_bar: u32,
    /// Where the first beat is, in seconds.
    pub first: f64,
}

impl Default for SceneBeat {
    /// 120 beats a minute, four in a bar, from 0 s.
    fn default() -> Self {
        SceneBeat { beat: 0.5, per_bar: 4, first: 0.0 }
    }
}

/// The scene's tempo in `world` (120 beats a minute, four in a bar, without a scene thing).
pub fn beat_of(world: &World) -> SceneBeat {
    SceneBeat { beat: world.beat_seconds, per_bar: world.beats_in_bar, first: world.first_beat }
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

    /// The camera reads the video's size (Stay inside, Slack, Fit all), so a new size is a new
    /// run.
    #[test]
    fn changing_the_video_size_rebuilds_the_world() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let before = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("width", Value::Int(720));
        let resolved = resolve(&scene, &kit.registry);
        let wide = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
        assert_ne!(wide, before);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("height", Value::Int(1280));
        let resolved = resolve(&scene, &kit.registry);
        assert_ne!(bpe_runtime::fingerprint(&scene, &resolved, &kit.sims), wide);
    }

    /// The camera's shake reads the seed, so a new seed is a new run.
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
}
