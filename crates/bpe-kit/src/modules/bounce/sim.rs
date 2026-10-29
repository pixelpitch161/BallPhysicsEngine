//! Answering a contact: keep some speed, gain some, grow a little. A module whose graph has been
//! edited runs the graph instead, with the native answer standing in for any output it leaves
//! unwired.

use bpe_core::{ModuleGraph, Params, Settings, SlotId};
use bpe_runtime::{Bodies, Contact, Limits, ModuleSim, SimCx, ThingState, reflect, state};
use bpe_vm::{Ball, NoHost, Program, Role, Val, Vm};
use glam::Vec2;

use super::BasicBounce;
use crate::graphs::{SeedHost, bind, compile_module};

pub struct BasicBounceSim;

/// What a bounce module keeps while the world runs.
#[derive(Clone, Debug)]
pub struct BounceState {
    pub settings: BasicBounce,
    /// The compiled graph, only for an edited module whose graph compiles.
    pub program: Option<Program>,
}

/// The native answer: the velocity after the bounce and the radius after it.
fn native(b: &BasicBounce, contact: &Contact, velocity: Vec2, radius: f32, hit: bool, limits: &Limits) -> (Vec2, f32) {
    let mut v = reflect(velocity, contact, b.bounciness);
    let mut r = radius;
    // Resting touches push the ball out but are not hits: no gain, no growth.
    if hit {
        v *= b.speed_gain;
        let max = b.max_size.max(0.5);
        r = (radius + b.grow).clamp(0.5, max);
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
        Box::new(BounceState { settings: BasicBounce::from_params(params), program: None })
    }

    fn prepare_graph(&self, params: &Params, graph: Option<&ModuleGraph>) -> Box<dyn ThingState> {
        let mut st = BounceState { settings: BasicBounce::from_params(params), program: None };
        if let Some(g) = graph
            && g.custom
            && let Ok(p) = compile_module(SlotId::Bounce, &bind(&g.graph, params))
            && !p.is_empty()
        {
            // "Takes over" is decided once, before anything moves.
            let zeros: Vec<Val> = Role::Bounce.inputs().iter().map(|(_, ty)| Val::zero(*ty)).collect();
            st.settings.takes_over = Vm::eval(&p, &NoHost, 0.0, &zeros, 0).boolean(2, st.settings.takes_over);
            st.program = Some(p);
        }
        Box::new(st)
    }

    fn respond(&self, st: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, limits: &Limits) {
        let b = &state::<BounceState>(st).settings;
        let hit = contact.speed >= limits.resting;
        let (v, r) = native(b, contact, bodies.velocity[item], bodies.radius[item], hit, limits);
        bodies.velocity[item] = v;
        bodies.radius[item] = r;
    }

    fn respond_at(
        &self,
        st: &dyn ThingState,
        contact: &Contact,
        bodies: &mut Bodies,
        item: usize,
        limits: &Limits,
        cx: &SimCx,
    ) {
        let st = state::<BounceState>(st);
        let Some(program) = &st.program else { return self.respond(st, contact, bodies, item, limits) };
        let velocity = bodies.velocity[item];
        let radius = bodies.radius[item];
        let hits = bodies.hits[item];
        let hit = contact.speed >= limits.resting;
        let inputs = [
            Val::Ball(Some(Ball { thing: cx.thing, item: item as u32 })),
            Val::Vector(velocity),
            Val::Vector(contact.normal),
            Val::Vector(contact.wall_velocity),
            Val::Number(contact.speed),
            Val::Number(radius),
            Val::Number(hits as f32),
            Val::Bool(hit),
            Val::Number(cx.time),
        ];
        let salt = (item as u64) << 32 | hits as u64;
        let out = Vm::eval(program, &SeedHost(cx.seed), cx.time, &inputs, salt);
        let (native_v, native_r) = native(&st.settings, contact, velocity, radius, hit, limits);
        bodies.velocity[item] = if out.wired(0) {
            let mut v = out.vector(0, native_v);
            let speed = v.length();
            if speed > limits.ceiling {
                v *= limits.ceiling / speed;
            }
            v
        } else {
            native_v
        };
        bodies.radius[item] = if out.wired(1) { out.number(1, radius).clamp(0.5, 400.0) } else { native_r };
    }

    fn takes_over(&self, st: &dyn ThingState) -> bool {
        state::<BounceState>(st).settings.takes_over
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{BALLS, one_ball_with_bounce};
    use bpe_core::Value;
    use bpe_runtime::Playback;

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
