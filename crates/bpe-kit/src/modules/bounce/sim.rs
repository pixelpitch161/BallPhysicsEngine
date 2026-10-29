//! Answering a contact: keep some speed, gain some, grow a little.

use bpe_core::{Params, Settings};
use bpe_runtime::{Bodies, Contact, Limits, ModuleSim, ThingState, reflect, state};

use super::BasicBounce;

pub struct BasicBounceSim;

impl ModuleSim for BasicBounceSim {
    fn kind(&self) -> &'static str {
        "bounce.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(BasicBounce::from_params(params))
    }

    fn respond(&self, st: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, limits: &Limits) {
        let b = state::<BasicBounce>(st);
        let mut v = reflect(bodies.velocity[item], contact, b.bounciness);
        // Resting touches push the ball out but are not hits: no gain, no growth.
        if contact.speed >= limits.resting {
            v *= b.speed_gain;
            let max = b.max_size.max(0.5);
            bodies.radius[item] = (bodies.radius[item] + b.grow).clamp(0.5, max);
        }
        let top = b.top_speed.min(limits.ceiling);
        let speed = v.length();
        if speed > top {
            v *= top / speed;
        }
        bodies.velocity[item] = v;
    }

    fn takes_over(&self, st: &dyn ThingState) -> bool {
        state::<BasicBounce>(st).takes_over
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
