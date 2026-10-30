//! Drawing the swarm: one layer the renderer steps and draws on the graphics card, from the
//! thing's settings, its Glowing dots look and its Flocking motion.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{DrawCx, Flock, Frame, GradientAtlas, PaintGpu, SwarmLayer, SwarmLook, ThingDraw};

use super::{SwarmSettings, SwarmStart};
use crate::modules::look_swarm::SwarmLook as Look;
use crate::modules::motion_flock::Flocking;
use crate::paint::gpu::{along_gpu, place_in_world};
use crate::paint::{Follow, Inputs, Paint, PlaceShape, beats_at, colour_at};

/// `SwarmLook::follow` for every point in one colour.
const ONE_COLOUR: u32 = 5;

/// How the swarm's points are painted with `p`: its follow, paint, place and one colour. Speed
/// (over the paint's span),
/// order and place are worked out per point on the GPU and the nearest ball by the swarm's
/// step; a single stop, the time and the beat give every point one colour, worked out here.
fn swarm_paint(cx: &DrawCx, p: &Paint, atlas: &mut GradientAtlas) -> (u32, PaintGpu, [f32; 4], [f32; 4]) {
    let first = p.colours.first().to_linear();
    let time = cx.time as f32;
    let none = PaintGpu::default();
    if p.follow == Follow::NearestBall {
        return (1, none, [0.0; 4], first);
    }
    if p.colours.stops.len() <= 1 {
        return (ONE_COLOUR, none, [0.0; 4], first);
    }
    let mut along = |follow| (follow, along_gpu(p, time, atlas), place_in_world(p, &cx.world.camera), first);
    match p.follow {
        // A speed's span is never measured on the screen.
        Follow::Speed => {
            let span = if p.span.is_finite() { p.span.max(1e-3) } else { 1e-3 };
            (0, along_gpu(p, time, atlas), [0.0, span, 0.0, 0.0], first)
        }
        Follow::Order => along(2),
        Follow::Place if p.shape == PlaceShape::Centre => along(4),
        Follow::Place => along(3),
        Follow::Time | Follow::Beat => {
            let beats =
                if p.follow == Follow::Beat { beats_at(cx.log, cx.video_time, cx.world.steps_per_second) } else { 0.0 };
            (ONE_COLOUR, none, [0.0; 4], colour_at(p, &Inputs { time, beats, count: 1, ..Inputs::default() }))
        }
        _ => (ONE_COLOUR, none, [0.0; 4], first),
    }
}

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
        let (follow, paint, place, color) = swarm_paint(cx, &look.colours, &mut frame.gradients);
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
            look: SwarmLook { size: look.size, brightness: look.brightness, follow, paint, place, color },
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
    use bpe_core::{Color, Gradient, SlotId, Value};
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
        // By speed, along the three default colours baked into one row.
        assert_eq!(layer.look.follow, 0);
        assert_eq!(layer.look.paint.row, 1.0);
        assert_eq!(frame.gradients.len(), 2);
        assert_eq!(layer.look.color, bpe_core::Color::rgb(0x20, 0x40, 0xff).to_linear());
        assert_eq!((layer.look.size, layer.look.brightness), (1.5, 0.25));
        assert_eq!(layer.motion, bpe_gfx::Flock::default());
        assert_eq!(frame.balls.len(), 1, "only the ball is drawn as a ball");
    }

    #[test]
    fn swarm_colours_follow_speed_order_place_and_the_nearest_ball() {
        let (mut scene, _, kit, me) = scene_with("swarm", "Fireflies", Vec2::ZERO, |_| {});
        let look = scene.add_module(&kit.registry, "look.swarm", "Dots").unwrap();
        scene.link(&kit.registry, me, SlotId::Look, look).unwrap();
        let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let mut draw = |follow: &str, colours: Gradient| {
            let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
            params.set("colours", Value::Gradient(colours));
            params.set("follow", Value::Choice(follow.into()));
            params.set("shape", Value::Choice("centre".into()));
            params.set("centre", Value::Vec2([10.0, 20.0]));
            let resolved = bpe_core::resolve(&scene, &kit.registry);
            assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
            let world = World::build(&scene, &resolved, &kit.sims);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
            frame
        };
        let two = Gradient::even(&[red, blue]);
        let follow = |frame: &Frame| frame.swarms[0].look.follow;
        let frame = draw("speed", two.clone());
        assert_eq!((follow(&frame), frame.swarms[0].look.place[1]), (0, 260.0), "the speed of the last colour");
        assert_eq!(follow(&draw("order", two.clone())), 2);
        let frame = draw("place", two.clone());
        assert_eq!(follow(&frame), 4, "a place measured from a centre");
        assert_eq!(frame.swarms[0].look.place, [90.0, 260.0, 10.0, 20.0]);
        let frame = draw("nearest_ball", two.clone());
        assert_eq!((follow(&frame), frame.swarms[0].look.color), (1, red.to_linear()));
        assert!(frame.gradients.len() == 1, "the nearest ball paints with the ball's colour");
        // One colour is every point's, exactly, with nothing baked.
        let frame = draw("speed", Gradient::solid(blue));
        assert_eq!((follow(&frame), frame.swarms[0].look.color), (5, blue.to_linear()));
        assert!(frame.gradients.len() == 1);
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
