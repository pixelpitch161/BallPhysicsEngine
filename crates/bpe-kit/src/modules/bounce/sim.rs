//! Answering a contact: keep some speed, gain some, grow a little, and bump into other balls.

use bpe_core::{Params, Settings, Target};
use bpe_runtime::{Bodies, Contact, Limits, ModuleSim, PairRule, ThingState, reflect, state};

use super::BasicBounce;

pub struct BasicBounceSim;

/// The bounce itself: reflect with `bounciness`, then on a real hit gain speed and grow by
/// `grow`, and never go faster than the top speed.
fn answer(
    b: &BasicBounce,
    contact: &Contact,
    bodies: &mut Bodies,
    item: usize,
    limits: &Limits,
    bounciness: f32,
    grow: f32,
) {
    let mut v = reflect(bodies.velocity[item], contact, bounciness);
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
    // Resting touches push the ball out but are not hits: no gain, no growth.
    if contact.speed >= limits.resting {
        v *= b.speed_gain;
        let max = b.max_size.max(0.5);
        bodies.radius[item] = (bodies.radius[item] + grow).clamp(0.5, max);
    }
    let top = b.top_speed.min(limits.ceiling);
    let speed = v.length();
    if speed > top {
        v *= top / speed;
    }
    bodies.velocity[item] = v;
}

impl ModuleSim for BasicBounceSim {
    fn kind(&self) -> &'static str {
        "bounce.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(BasicBounce::from_params(params))
    }

    fn respond(&self, st: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, limits: &Limits) {
        let b = state::<BasicBounce>(st);
        answer(b, contact, bodies, item, limits, b.bounciness, b.grow);
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
        let b = state::<BasicBounce>(st);
        if special {
            answer(b, contact, bodies, item, limits, b.special_bounciness, b.special_grow);
        } else {
            answer(b, contact, bodies, item, limits, b.bounciness, b.grow);
        }
    }

    fn takes_over(&self, st: &dyn ThingState) -> bool {
        state::<BasicBounce>(st).takes_over
    }

    fn bounciness(&self, st: &dyn ThingState) -> f32 {
        state::<BasicBounce>(st).bounciness
    }

    fn pairs(&self, st: &dyn ThingState) -> Option<PairRule> {
        let b = state::<BasicBounce>(st);
        b.hit_balls.then(|| PairRule { target: b.balls_to_hit.clone(), friction: b.friction })
    }

    fn special_target(&self, st: &dyn ThingState) -> Option<Target> {
        let b = state::<BasicBounce>(st);
        b.special.then(|| b.special_target.clone())
    }

    fn respond_pair(&self, st: &dyn ThingState, bodies: &mut Bodies, item: usize, _speed: f32, limits: &Limits) {
        let b = state::<BasicBounce>(st);
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
        let b = state::<BasicBounce>(st);
        b.pops_after > 0 && bodies.hits[item] >= b.pops_after
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{ARENA, BALLS, one_ball_with_bounce};
    use bpe_core::{SlotId, Target, Value, resolve, resolve_slot};
    use bpe_runtime::{EventKind, Log, Party, Playback, World};
    use glam::Vec2;

    #[test]
    fn hit_balls_makes_a_block_collide_and_the_log_names_both_balls() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(60.0));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let m = resolve_slot(&scene, &kit.registry, BALLS, SlotId::Bounce, &mut Vec::new()).unwrap().module;
        scene.library.modules.get_mut(&m).unwrap().params.set("hit_balls", Value::Bool(true));
        scene.library.modules.get_mut(&m).unwrap().params.set("grow", Value::Number(3.0));
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
        scene.library.modules.get_mut(&m).unwrap().params.remove("hit_balls");
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        w.bodies.get_mut(&BALLS).unwrap().velocity[0] = Vec2::new(200.0, 0.0);
        w.bodies.get_mut(&BALLS).unwrap().velocity[1] = Vec2::new(-200.0, 0.0);
        let mut log = Log::default();
        w.advance_to(0.2, &kit.sims, &mut log);
        assert!(log.events().iter().all(|e| e.b.is_none_or(|b| b.thing != BALLS)));
    }

    #[test]
    fn friction_rubs_speed_off_against_the_arena() {
        // A ball sliding along the floor of a box, with no bounce to lift it off.
        let speed_after = |friction: f64| {
            let (mut scene, _, kit) = one_ball_with_bounce(|p| {
                p.set("bounciness", Value::Number(0.0));
                p.set("friction", Value::Number(friction));
            });
            let arena = scene.things.get_mut(&ARENA).unwrap();
            arena.params.set("shape", Value::Choice("box".into()));
            arena.params.set("width", Value::Number(900.0));
            arena.params.set("height", Value::Number(900.0));
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
            p.set("special_target", Value::Target(Target::Kind("arena".into())));
            p.set("special_bounciness", Value::Number(0.0));
        });
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(20.0);
        let body = &playback.world().bodies[&BALLS];
        assert!((body.position[0].length() - 482.5).abs() < 1.0, "dead against the arena: {:?}", body.position[0]);
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
