//! Scene defaults in the world: the step rate and gravity.

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
    }

    /// A keyframe moved gravity.
    fn settings_changed(&self, _thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        world.gravity = SceneSettings::from_params(&resolved.params).gravity;
    }

    fn affects_physics(&self, key: &str) -> bool {
        matches!(key, "gravity" | "steps_per_second" | "duration")
    }
}
