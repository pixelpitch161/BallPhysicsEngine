//! Scene defaults in the world: the step rate and gravity.

use bpe_core::{ResolvedThing, Settings, Thing};
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
    }

    fn affects_physics(&self, key: &str) -> bool {
        matches!(key, "gravity" | "steps_per_second" | "duration" | "seed")
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene};
    use bpe_core::{ThingId, Value, resolve};
    use bpe_runtime::World;

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
}
