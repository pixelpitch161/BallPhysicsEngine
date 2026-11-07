//! Scene settings in the world: the step rate, gravity, the seed and the video size, and the
//! tempo for things born on the beat (a [`SceneBeat`] at the root).

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
        let (beat, per_bar) = crate::things::rig::pattern::beat_seconds(&s);
        world.states.insert(ThingId::ROOT, Box::new(SceneBeat { beat, per_bar }));
    }

    /// A keyframe moved gravity.
    fn settings_changed(&self, _thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        world.gravity = SceneSettings::from_params(&resolved.params).gravity;
    }

    /// The video's size counts too: the camera's Stay inside, Slack and Fit all read it.
    fn affects_physics(&self, key: &str) -> bool {
        matches!(key, "gravity" | "steps_per_second" | "duration" | "seed" | "width" | "height")
    }

    /// Particles born or pushed on the beat read the tempo.
    fn affects_beside(&self, key: &str) -> bool {
        matches!(key, "tempo" | "time_top" | "time_bottom")
    }
}

/// The scene's tempo as the world has it, at [`ThingId::ROOT`] in `World.states`: seconds in a
/// beat and beats in a bar, the same as the sound's `TempoMap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneBeat {
    pub beat: f64,
    pub per_bar: u32,
}

impl Default for SceneBeat {
    /// 120 beats a minute, four in a bar.
    fn default() -> Self {
        SceneBeat { beat: 0.5, per_bar: 4 }
    }
}

/// The scene's tempo in `world` (the default without a scene thing).
pub fn beat_of(world: &World) -> SceneBeat {
    world
        .states
        .get(&ThingId::ROOT)
        .and_then(|s| (**s).as_any().downcast_ref::<SceneBeat>())
        .copied()
        .unwrap_or_default()
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
