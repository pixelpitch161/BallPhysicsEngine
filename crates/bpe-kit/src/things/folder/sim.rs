//! A folder has a place in the world, so things can ride on it.

use bpe_core::{ResolvedThing, Thing, ThingId};
use bpe_runtime::{ThingSim, World};

pub struct FolderSim;

impl ThingSim for FolderSim {
    fn kind(&self) -> &'static str {
        "folder"
    }

    fn build(&self, thing: &Thing, _resolved: &ResolvedThing, world: &mut World) {
        self.scripted(thing.id, world);
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let pose = world.anchor(thing);
        world.poses.insert(thing, pose);
    }

    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::{Attach, ThingId, resolve};
    use bpe_runtime::World;
    use glam::Vec2;

    use crate::testing::BALLS;
    use crate::{kit, new_scene};

    #[test]
    fn folders_add_with_no_slots_and_place_their_riders() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let f = scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
        assert!(scene.things[&f].slots.is_empty());
        assert_eq!(scene.things[&f].name, "Folder");
        scene.things.get_mut(&f).unwrap().transform.position = Vec2::new(100.0, 50.0);
        scene
            .set_parent(BALLS, f, Attach::Ride { offset: Vec2::new(10.0, 0.0), turn_with_parent: false, item: None })
            .unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(110.0, 50.0));
        assert_eq!(w.poses[&f].position, Vec2::new(100.0, 50.0));
        assert_eq!(bpe_core::check_registry(&kit.registry), Vec::<String>::new());
    }
}
