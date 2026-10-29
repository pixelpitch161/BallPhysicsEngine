//! Drawing the swarm: one layer the renderer steps and draws on the graphics card, from the
//! thing's settings, its Glowing dots look and its Flocking motion.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{DrawCx, Flock, Frame, SwarmLayer, SwarmLook, ThingDraw};

use super::{SwarmSettings, SwarmStart};
use crate::modules::look_swarm::{SwarmColors, SwarmLook as Look};
use crate::modules::motion_flock::Flocking;

pub struct SwarmDraw;

impl ThingDraw for SwarmDraw {
    fn kind(&self) -> &'static str {
        "swarm"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        // From the live settings, not the world's copy: changing a swarm never rebuilds the
        // physics, and the picture must follow the inspector all the same.
        let settings = SwarmSettings::from_params(&cx.resolved.params);
        let look = match cx.resolved.slots.get(&SlotId::Look) {
            Some(l) if l.kind == "look.swarm" => Look::from_params(&l.params),
            _ => Look::default(),
        };
        let motion = match cx.resolved.slots.get(&SlotId::Motion) {
            Some(m) if m.kind == "motion.flock" => Flocking::from_params(&m.params),
            _ => Flocking::default(),
        };
        frame.swarms.push(SwarmLayer {
            thing: cx.thing.id,
            count: settings.count,
            start: match settings.start {
                SwarmStart::Fill => bpe_gfx::SwarmStart::Fill,
                SwarmStart::Centre => bpe_gfx::SwarmStart::Centre,
                SwarmStart::Ring => bpe_gfx::SwarmStart::Ring,
            },
            center: cx.thing.transform.position.into(),
            seed: settings.seed,
            release_seconds: settings.release_seconds,
            look: SwarmLook {
                size: look.size,
                brightness: look.brightness,
                colors_by: match look.colors_by {
                    SwarmColors::Speed => 0,
                    SwarmColors::NearestBall => 1,
                    SwarmColors::Gradient => 2,
                },
                colors: look.colors.iter().map(|c| c.to_linear()).collect(),
            },
            motion: Flock {
                top_speed: motion.top_speed,
                drag: motion.drag,
                flow: motion.flow,
                swirl_size: motion.swirl_size,
                swirl_speed: motion.swirl_speed,
                flock: motion.flock,
                separation: motion.separation,
                attraction: motion.attraction,
                reach: motion.reach,
                avoid_walls: motion.avoid_walls,
                gravity_scale: motion.gravity_scale,
            },
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::scene_with;
    use bpe_core::Value;
    use bpe_gfx::{Frame, SwarmStart, build_frame};
    use bpe_runtime::{Log, World, fingerprint};
    use glam::Vec2;

    #[test]
    fn a_swarm_has_no_bodies_and_draws_one_layer() {
        let (scene, resolved, kit, me) = scene_with("swarm", "Fireflies", Vec2::new(10.0, -20.0), |_| {});
        let world = World::build(&scene, &resolved, &kit.sims);
        assert!(!world.bodies.contains_key(&me), "a swarm has no bodies in the processor's world");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.swarms.len(), 1);
        let layer = &frame.swarms[0];
        assert_eq!(layer.thing, me);
        assert_eq!(layer.count, 50_000);
        assert_eq!(layer.start, SwarmStart::Fill);
        assert_eq!(layer.center, [10.0, -20.0]);
        assert_eq!((layer.seed, layer.release_seconds), (1, 0.0));
        assert_eq!(layer.look.colors_by, 0);
        assert_eq!(layer.look.colors.len(), 3);
        assert_eq!((layer.look.size, layer.look.brightness), (1.5, 0.25));
        assert_eq!(layer.motion, bpe_gfx::Flock::default());
        assert_eq!(frame.balls.len(), 1, "only the ball is drawn as a ball");
    }

    #[test]
    fn swarm_settings_do_not_change_the_physics_fingerprint() {
        let (scene, resolved, kit, me) = scene_with("swarm", "Fireflies", Vec2::ZERO, |_| {});
        let before = fingerprint(&scene, &resolved, &kit.sims);
        let mut changed = scene.clone();
        changed.things.get_mut(&me).unwrap().params.set("count", Value::Int(123));
        changed.things.get_mut(&me).unwrap().params.set("start", Value::Choice("ring".into()));
        let resolved_changed = bpe_core::resolve(&changed, &kit.registry);
        assert_eq!(fingerprint(&changed, &resolved_changed, &kit.sims), before, "the swarm never touches the physics");
        let mut moved = scene.clone();
        moved.things.get_mut(&crate::testing::BALLS).unwrap().transform.position = Vec2::new(5.0, 5.0);
        let resolved_moved = bpe_core::resolve(&moved, &kit.registry);
        assert_ne!(fingerprint(&moved, &resolved_moved, &kit.sims), before);
    }

    #[test]
    fn a_flocking_change_does_not_change_the_physics_fingerprint() {
        // Flocking has no part in the processor's physics (only the graphics card reads it), so
        // editing it must not restart the playback.
        let kit = crate::kit();
        let mut scene = crate::new_scene(&kit.registry);
        let me = crate::add_thing(&mut scene, &kit.registry, "swarm", bpe_core::ThingId::ROOT).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let before = fingerprint(&scene, &resolved, &kit.sims);
        let flock = bpe_core::resolve_slot(&scene, &kit.registry, me, bpe_core::SlotId::Motion, &mut Vec::new())
            .expect("a new swarm flocks")
            .module;
        scene.library.modules.get_mut(&flock).unwrap().params.set("flow", Value::Number(3.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert_eq!(fingerprint(&scene, &resolved, &kit.sims), before, "flocking never touches the physics");
    }

    #[test]
    fn a_setting_change_shows_without_rebuilding_the_world() {
        // Swarm settings never touch the physics, so the editor keeps its world; the picture
        // must still follow the inspector.
        let (mut scene, resolved, kit, me) = scene_with("swarm", "Fireflies", Vec2::ZERO, |_| {});
        let world = World::build(&scene, &resolved, &kit.sims);
        scene.things.get_mut(&me).unwrap().params.set("count", Value::Int(777));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.swarms[0].count, 777);
    }
}
