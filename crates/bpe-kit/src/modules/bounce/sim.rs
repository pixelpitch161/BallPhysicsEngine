//! Answering a contact: keep some speed, gain some, grow a little, rub some off with friction,
//! and bump into other balls.

use bpe_core::{Params, Settings, Target};
use bpe_runtime::{Bodies, Contact, Limits, ModuleSim, PairRule, ThingState, Touch, reflect, state};
use glam::Vec2;

use super::{BasicBounce, WallTouch};

pub struct BasicBounceSim;

/// What a bounce module keeps while the world runs.
#[derive(Clone, Debug)]
pub struct BounceState {
    pub settings: BasicBounce,
}

/// The native answer: the velocity after the bounce and the radius after it. Reflect with the
/// bounciness (the special one against a special target), rub sideways speed off with friction,
/// then on a real hit gain speed and grow, and never go faster than the top speed.
fn native(
    b: &BasicBounce,
    contact: &Contact,
    velocity: Vec2,
    radius: f32,
    hit: bool,
    limits: &Limits,
    special: bool,
) -> (Vec2, f32) {
    let (bounciness, grow) = if special { (b.special_bounciness, b.special_grow) } else { (b.bounciness, b.grow) };
    let mut v = reflect(velocity, contact, bounciness);
    // Friction rubs sideways speed off, up to what the push off the wall allows.
    if b.friction > 0.0 {
        let relative = v - contact.wall_velocity;
        let sideways = relative - contact.normal * relative.dot(contact.normal);
        let speed = sideways.length();
        let most = b.friction * contact.speed.max(0.0) * (1.0 + bounciness);
        if speed > 1e-6 {
            v -= sideways * (speed.min(most) / speed);
        }
    }
    let mut r = radius;
    // Resting touches push the ball out but are not hits: no gain, no growth.
    if hit {
        v *= b.speed_gain;
        let max = b.max_size.max(0.5);
        r = (radius + grow).clamp(0.5, max);
    }
    let top = b.top_speed.min(limits.ceiling);
    let speed = v.length();
    if speed > top {
        v *= top / speed;
    }
    (v, r)
}

impl ModuleSim for BasicBounceSim {
    fn kind(&self) -> &'static str {
        "bounce.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(BounceState { settings: BasicBounce::from_params(params) })
    }

    fn respond(&self, st: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, limits: &Limits) {
        self.respond_against(st, contact, bodies, item, limits, false);
    }

    fn respond_against(
        &self,
        st: &dyn ThingState,
        contact: &Contact,
        bodies: &mut Bodies,
        item: usize,
        limits: &Limits,
        special: bool,
    ) {
        let b = &state::<BounceState>(st).settings;
        let hit = contact.speed >= limits.resting_along(contact.normal);
        let (v, r) = native(b, contact, bodies.velocity[item], bodies.radius[item], hit, limits, special);
        bodies.velocity[item] = v;
        bodies.radius[item] = r;
    }

    fn touch(&self, st: &dyn ThingState) -> Touch {
        match state::<BounceState>(st).settings.touch {
            WallTouch::Edge => Touch::Edge,
            WallTouch::Middle => Touch::Middle,
        }
    }

    fn takes_over(&self, st: &dyn ThingState) -> bool {
        state::<BounceState>(st).settings.takes_over
    }

    fn bounciness(&self, st: &dyn ThingState) -> f32 {
        state::<BounceState>(st).settings.bounciness
    }

    fn pairs(&self, st: &dyn ThingState) -> Option<PairRule> {
        let b = &state::<BounceState>(st).settings;
        b.hit_balls.then(|| PairRule { target: b.balls_to_hit.clone(), friction: b.friction })
    }

    fn special_target(&self, st: &dyn ThingState) -> Option<Target> {
        let b = &state::<BounceState>(st).settings;
        b.special.then(|| b.special_target.clone())
    }

    fn respond_pair(&self, st: &dyn ThingState, bodies: &mut Bodies, item: usize, _speed: f32, limits: &Limits) {
        let b = &state::<BounceState>(st).settings;
        let mut v = bodies.velocity[item] * b.speed_gain;
        let max = b.max_size.max(0.5);
        bodies.radius[item] = (bodies.radius[item] + b.grow).clamp(0.5, max);
        let top = b.top_speed.min(limits.ceiling);
        let speed = v.length();
        if speed > top {
            v *= top / speed;
        }
        bodies.velocity[item] = v;
    }

    fn effects(&self, st: &dyn ThingState, bodies: &mut Bodies, item: usize, _speed: f32, _limits: &Limits) -> bool {
        let b = &state::<BounceState>(st).settings;
        b.pops_after > 0 && bodies.hits[item] >= b.pops_after
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{BALLS, SHAPE, one_ball_with_bounce};
    use bpe_core::Settings;
    use bpe_core::{SlotId, Target, Value, resolve};
    use bpe_runtime::ModuleSim;
    use bpe_runtime::{EventKind, Log, Party, Playback, World};
    use glam::Vec2;

    #[test]
    fn balls_touch_walls_with_their_edge_unless_set_to_their_middle() {
        assert_eq!(super::BasicBounce::default().touch, super::WallTouch::Edge);
        let st = super::BasicBounceSim.prepare(&super::BasicBounce::default().to_params());
        assert_eq!(super::BasicBounceSim.touch(st.as_ref()), bpe_runtime::Touch::Edge);
        let middle = super::BasicBounce { touch: super::WallTouch::Middle, ..Default::default() };
        let st = super::BasicBounceSim.prepare(&middle.to_params());
        assert_eq!(super::BasicBounceSim.touch(st.as_ref()), bpe_runtime::Touch::Middle);
    }

    /// Fast balls touching with their middles and bumping into each other under gravity, in a
    /// hollow box and a hollow ellipse, at speeds up to 3000 px/s for 10 s: none ever leaves.
    #[test]
    fn a_middle_ball_never_leaves_a_box_or_an_ellipse() {
        use crate::ShapeSettings;
        for shape in [ShapeSettings::rectangle(700.0, 1100.0, 6.0), ShapeSettings::ellipse(700.0, 1100.0, 6.0)] {
            let (mut scene, _, kit) = crate::testing::scene_with_balls("single", |_| {});
            scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 900.0]));
            scene.things.get_mut(&SHAPE).unwrap().params = shape.to_params();
            scene.things.get_mut(&SHAPE).unwrap().transform.position = Vec2::ZERO;
            let mut balls =
                crate::BallsSettings { radius: 14.0, velocity: Vec2::new(2900.0, 700.0), ..Default::default() };
            balls.group = true;
            balls.shape = crate::things::balls::GroupShape::Scatter;
            balls.count = 120;
            balls.area = Vec2::new(450.0, 700.0);
            balls.launch = crate::things::balls::Launch::Random;
            balls.vary_speed = 0.9;
            let thing = scene.things.get_mut(&BALLS).unwrap();
            thing.params = balls.to_params();
            thing.transform.position = Vec2::ZERO;
            let card = scene.card_mut(BALLS, SlotId::Bounce).unwrap();
            card.params.set("touch", Value::Choice("middle".into()));
            card.params.set("top_speed", Value::Number(5000.0));
            // Other balls nudge them about too, under gravity.
            card.params.set("hit_balls", Value::Bool(true));
            scene.card_mut(BALLS, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(1.0));
            let resolved = resolve(&scene, &kit.registry);
            let mut w = World::build(&scene, &resolved, &kit.sims);
            let mut log = Log::default();
            for k in 1..=100 {
                w.advance_to(k as f64 * 0.1, &kit.sims, &mut log);
                let wall = &w.walls[&SHAPE].outline;
                for p in &w.bodies[&BALLS].position {
                    assert!(
                        p.is_finite() && wall.inside(*p),
                        "{wall:?}: a ball got out to {p} by {} s",
                        k as f64 * 0.1
                    );
                }
            }
            assert!(log.events().iter().filter(|e| e.kind == EventKind::Hit).count() > 100);
        }
    }

    #[test]
    fn hit_balls_makes_a_block_collide_and_the_log_names_both_balls() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(60.0));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let m = (BALLS, SlotId::Bounce);
        scene.card_mut(m.0, m.1).unwrap().params.set("hit_balls", Value::Bool(true));
        scene.card_mut(m.0, m.1).unwrap().params.set("grow", Value::Number(3.0));
        scene.things.get_mut(&BALLS).unwrap().transform.position = Vec2::new(0.0, 300.0);
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        {
            let b = w.bodies.get_mut(&BALLS).unwrap();
            b.velocity[0] = Vec2::new(200.0, 0.0);
            b.velocity[1] = Vec2::new(-200.0, 0.0);
        }
        let mut log = Log::default();
        w.advance_to(0.2, &kit.sims, &mut log);
        let pair = log.events().iter().find(|e| e.b == Some(Party { thing: BALLS, item: 1 })).expect("the balls met");
        assert_eq!(pair.a, Party { thing: BALLS, item: 0 });
        assert!(pair.speed > 350.0, "{pair:?}");
        assert!(w.bodies[&BALLS].radius[0] >= 27.0, "a ball hit grows too");
        // Off by default: the same block passes straight through itself.
        scene.card_mut(m.0, m.1).unwrap().params.remove("hit_balls");
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        w.bodies.get_mut(&BALLS).unwrap().velocity[0] = Vec2::new(200.0, 0.0);
        w.bodies.get_mut(&BALLS).unwrap().velocity[1] = Vec2::new(-200.0, 0.0);
        let mut log = Log::default();
        w.advance_to(0.2, &kit.sims, &mut log);
        assert!(log.events().iter().all(|e| e.b.is_none_or(|b| b.thing != BALLS)));
    }

    #[test]
    fn friction_rubs_speed_off_against_a_shape() {
        // A ball sliding along the floor of a box, with no bounce to lift it off.
        let speed_after = |friction: f64| {
            let (mut scene, _, kit) = one_ball_with_bounce(|p| {
                p.set("bounciness", Value::Number(0.0));
                p.set("friction", Value::Number(friction));
            });
            scene.things.get_mut(&SHAPE).unwrap().params =
                bpe_core::Settings::to_params(&crate::ShapeSettings::rectangle(900.0, 900.0, 3.0));
            let balls = scene.things.get_mut(&BALLS).unwrap();
            balls.transform.position = Vec2::new(-300.0, 420.0);
            balls.params.set("velocity", Value::Vec2([100.0, 0.0]));
            let resolved = resolve(&scene, &kit.registry);
            let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
            playback.seek(3.0);
            playback.world().bodies[&BALLS].velocity[0].length()
        };
        let (slick, rough) = (speed_after(0.0), speed_after(0.6));
        assert!(slick > 90.0, "nothing slows a slick ball: {slick}");
        assert!(rough < 10.0, "friction stops the ball: {rough}");
    }

    #[test]
    fn a_special_target_changes_the_bounce_against_it_only() {
        let (scene, resolved, kit) = one_ball_with_bounce(|p| {
            p.set("special", Value::Bool(true));
            p.set("special_target", Value::Target(Target::Kind("shape".into())));
            p.set("special_bounciness", Value::Number(0.0));
        });
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(20.0);
        let body = &playback.world().bodies[&BALLS];
        assert!((body.position[0].length() - 482.5).abs() < 1.0, "dead against the shape: {:?}", body.position[0]);
        let (scene, resolved, kit) = one_ball_with_bounce(|p| {
            p.set("special", Value::Bool(true));
            p.set("special_target", Value::Target(Target::Tag("pegs".into())));
            p.set("special_bounciness", Value::Number(0.0));
        });
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(5.0);
        assert!(playback.world().bodies[&BALLS].velocity[0].length() > 300.0, "lively against everything else");
    }

    #[test]
    fn pops_after_removes_the_ball_and_logs_it() {
        let (scene, resolved, kit) = one_ball_with_bounce(|p| p.set("pops_after", Value::Int(3)));
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(20.0);
        let log = playback.log();
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Hit).count(), 3);
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Remove).count(), 1);
        assert!(!playback.world().bodies[&BALLS].alive[0]);
        assert_eq!(log.events().last().unwrap().kind, EventKind::Remove);
    }

    #[test]
    fn grow_and_speed_gain_apply_on_each_hit() {
        let (scene, resolved, kit) = one_ball_with_bounce(|p| {
            p.set("grow", Value::Number(2.0));
            p.set("speed_gain", Value::Number(1.1));
            p.set("max_size", Value::Number(30.0));
        });
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(10.0);
        let hits = playback.log().len() as f32;
        assert!(hits >= 3.0);
        assert_eq!(playback.world().bodies[&BALLS].radius[0], (24.0 + 2.0 * hits).min(30.0));
    }

    #[test]
    fn a_dead_bounce_keeps_the_ball_sliding_on_the_wall() {
        let (scene, resolved, kit) = one_ball_with_bounce(|p| p.set("bounciness", Value::Number(0.0)));
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(20.0);
        let body = &playback.world().bodies[&BALLS];
        // With no friction the ball swings around the bowl, but it never leaves the wall of a
        // 508 ring (thickness 3, radius 24) and never gains energy.
        assert!((body.position[0].length() - 482.5).abs() < 1.0, "{:?}", body.position[0]);
        assert!(body.velocity[0].length() < 1200.0, "{:?}", body.velocity[0]);
    }

    #[test]
    fn top_speed_caps_a_lively_ball() {
        let (scene, resolved, kit) = one_ball_with_bounce(|p| {
            p.set("bounciness", Value::Number(1.5));
            p.set("top_speed", Value::Number(400.0));
        });
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(10.0);
        for e in playback.log().events() {
            assert!(e.speed <= 400.0 + 500.0 * 0.6, "{e:?}");
        }
    }
}
