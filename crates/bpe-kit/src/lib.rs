//! The modular Ball Studio's plugins. Each thing and module kind lives in its own folder with
//! its settings, its simulation, its drawing and its sound side by side.

pub mod modules;
pub mod scenes;
pub mod things;

use bpe_core::{Color, Registry, Scene, Settings, SlotId, ThingId};
use glam::Vec2;

pub use modules::bounce::{BasicBounce, BasicBounceKind};
pub use modules::look::{ClassicLook, ClassicLookKind};
pub use modules::motion::{BasicMotion, BasicMotionKind};
pub use modules::sound::{NoteSound, NoteSoundKind};
pub use things::arena::{ArenaKind, ArenaSettings, ArenaShape};
pub use things::balls::{Arrangement, BallsKind, BallsSettings};
pub use things::scene::{SceneKind, SceneSettings};

/// Every kind the kit has, for the core's resolve and the editor's lists.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.add_thing(SceneKind);
    r.add_thing(ArenaKind);
    r.add_thing(BallsKind);
    r.add_module(ClassicLookKind);
    r.add_module(BasicMotionKind);
    r.add_module(BasicBounceKind);
    r.add_module(NoteSoundKind);
    r
}

/// What a brand-new scene holds: a circle arena and one pink ball, sharing the root's plain
/// look, motion, bounce and sound.
pub fn new_scene(reg: &Registry) -> Scene {
    let mut scene = Scene::empty(reg);
    scene.name = "New scene".into();
    let root = ThingId::ROOT;
    let look = scene.add_module(reg, "look.classic", "Plain look").expect("kit kinds are registered");
    let motion = scene.add_module(reg, "motion.basic", "Default motion").expect("kit kinds are registered");
    let bounce = scene.add_module(reg, "bounce.basic", "Default bounce").expect("kit kinds are registered");
    let sound = scene.add_module(reg, "sound.notes", "Default sound").expect("kit kinds are registered");
    for (slot, m) in [(SlotId::Look, look), (SlotId::Motion, motion), (SlotId::Bounce, bounce), (SlotId::Sound, sound)] {
        scene.link(reg, root, slot, m).expect("the root has every slot");
    }
    scene.add_thing(reg, "arena", root).expect("kit kinds are registered");
    let ball = scene.add_thing(reg, "balls", root).expect("kit kinds are registered");
    let ball_look = scene.add_module(reg, "look.classic", "Ball look").expect("kit kinds are registered");
    scene.library.modules.get_mut(&ball_look).expect("just added").params =
        ClassicLook { color: Color::rgb(0xff, 0x5c, 0xa8), ..ClassicLook::default() }.to_params();
    scene.link(reg, ball, SlotId::Look, ball_look).expect("balls have a look slot");
    let thing = scene.things.get_mut(&ball).expect("just added");
    thing.name = "Ball".into();
    thing.transform.position = Vec2::new(0.0, -200.0);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, SlotId, ThingId, Value};

    #[test]
    fn every_kit_plugin_passes_the_contract() {
        assert_eq!(bpe_core::check_registry(&registry()), Vec::<String>::new());
    }

    #[test]
    fn a_new_scene_is_an_arena_and_one_ball_with_shared_modules() {
        let reg = registry();
        let s = new_scene(&reg);
        assert_eq!(s.name, "New scene");
        let kinds: Vec<&str> = s.order.iter().map(|id| s.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "arena", "balls"]);
        assert_eq!(s.things[&ThingId(2)].name, "Ball");
        assert_eq!(s.things[&ThingId(2)].transform.position, glam::Vec2::new(0.0, -200.0));
        assert_eq!(s.library.modules.len(), 5);
        let res = bpe_core::resolve(&s, &reg);
        assert!(res.problems.is_empty(), "{:?}", res.problems);
        let ball = &res.things[&ThingId(2)];
        assert_eq!(ball.slots[&SlotId::Look].params.get("color"), Some(&Value::Color(Color::rgb(0xff, 0x5c, 0xa8))));
        assert_eq!(ball.slots[&SlotId::Bounce].from, ThingId::ROOT);
        assert_eq!(ball.slots[&SlotId::Motion].from, ThingId::ROOT);
        assert_eq!(ball.slots[&SlotId::Sound].from, ThingId::ROOT);
        assert_eq!(res.things[&ThingId(1)].slots[&SlotId::Look].from, ThingId::ROOT);
    }
}
