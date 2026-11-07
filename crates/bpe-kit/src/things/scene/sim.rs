//! Scene settings in the world: the step rate, gravity, the seed and the video size, and the
//! air's settings when the scene has an Air card (a sleeping [`crate::air::Air`]: a Particles
//! thing that reads it wakes it).

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
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
        if let Some(spec) = crate::modules::air::spec_of(resolved) {
            world.states.insert(ThingId::ROOT, Box::new(crate::air::Air::new(spec)));
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
