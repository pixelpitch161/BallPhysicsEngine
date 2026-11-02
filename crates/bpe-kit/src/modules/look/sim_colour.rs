//! A ball's colour as the simulation sees it, for marks: the same inputs and the same hue
//! shift the drawing uses, read from the world instead of a frame.

use bpe_core::{Params, Settings, ThingId};
use bpe_runtime::{World, with_picked};

use super::BasicLook;
use super::draw::{ball_inputs, for_balls, hue_shifted};
use crate::paint::{Follow, Paint, colour_at};

/// The linear-light colour ball `i` of `thing` has right now under `look` (a Look slot's
/// settings), with any tint a rule gave it laid over it as strongly as the tint's alpha says.
///
/// Known gap: a Step colour that takes the colour of what was hit reads as the ball's own
/// colour here, and a thing stuck to the screen is read as a world thing.
pub fn ball_colour_in_sim(look: &Params, world: &World, thing: ThingId, i: usize) -> [f32; 4] {
    SimLook::new(look, world).colour(world, thing, i)
}

/// A look's settings read once for a step, so many balls' colours can be worked out from it
/// without reading them again: see [`ball_colour_in_sim`].
#[derive(Clone, Debug)]
pub struct SimLook {
    fill: Paint,
    hue_per_hit: f32,
    beats: f32,
}

impl SimLook {
    pub fn new(look: &Params, world: &World) -> Self {
        let look = BasicLook::from_params(look);
        let fill = for_balls(look.fill);
        // The beats so far, counted by the world: the log may have been trimmed. In the
        // simulation no later beat is logged yet, so there is no fraction to the next one.
        let beats = if fill.follow == Follow::Beat { world.beats as f32 } else { 0.0 };
        Self { fill, hue_per_hit: look.hue_per_hit, beats }
    }

    /// The colour ball `i` of `thing` has under this look right now.
    pub fn colour(&self, world: &World, thing: ThingId, i: usize) -> [f32; 4] {
        let Some(bodies) = world.bodies.get(&thing) else { return [0.0; 4] };
        let place = bodies.position.get(i).copied().unwrap_or_default();
        let inputs = ball_inputs(thing, bodies, i, world.camera.to_screen(place), world.time(), self.beats);
        let base = colour_at(&self.fill, &inputs);
        let c = hue_shifted(base, self.hue_per_hit, bodies.hits.get(i).copied().unwrap_or(0));
        let tinted = match bodies.tint.get(i) {
            Some(t) => [c[0] + (t[0] - c[0]) * t[3], c[1] + (t[1] - c[1]) * t[3], c[2] + (t[2] - c[2]) * t[3], c[3]],
            None => c,
        };
        // Paint picked up from wet paint goes over the rule's tint.
        with_picked(world, thing, i, tinted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::things::balls::draw::ball_colors;
    use bpe_core::{Color, Gradient, SlotId, Value};
    use bpe_gfx::DrawCx;

    #[test]
    fn sim_colour_matches_draw_colour() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(2));
        });
        let m =
            bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set(
            "colours",
            Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), Color::rgb(0, 255, 0)])),
        );
        params.set("follow", Value::Choice("speed".into()));
        params.set("span", Value::Number(600.0));
        params.set("hue_per_hit", Value::Number(30.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        // Different launch speeds, so following speed gives different colours.
        for (i, v) in world.bodies.get_mut(&ThingId(2)).unwrap().velocity.iter_mut().enumerate() {
            *v = glam::Vec2::new(40.0 * i as f32, -30.0);
        }
        let mut log = bpe_runtime::Log::default();
        world.advance_to(1.0, &kit.sims, &mut log);
        let (world, log) = (&world, &log);
        let thing = &scene.things[&ThingId(2)];
        let r = &resolved.things[&ThingId(2)];
        let cx = DrawCx { thing, resolved: r, scene: &resolved, world, log, time: 1.0, video_time: 1.0 };
        let drawn = ball_colors(&cx).unwrap();
        let look = &r.slots[&SlotId::Look].params;
        let n = world.bodies[&ThingId(2)].len();
        assert_eq!(n, 6);
        let mut distinct = Vec::new();
        for i in 0..n {
            let c = ball_colour_in_sim(look, world, ThingId(2), i);
            assert_eq!(c, drawn(i), "ball {i}");
            if !distinct.contains(&c) {
                distinct.push(c);
            }
        }
        assert!(distinct.len() > 1, "the speed should vary the colours");
    }

    #[test]
    fn drawn_colour_equals_sim_colour_with_pickup() {
        let (scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
        });
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let log = bpe_runtime::Log::default();
        let thing = &scene.things[&ThingId(2)];
        let r = &resolved.things[&ThingId(2)];
        let look = &r.slots[&SlotId::Look].params;
        let plain = ball_colour_in_sim(look, &world, ThingId(2), 0);
        world
            .picked
            .insert(ThingId(2), vec![bpe_runtime::Picked { rgb: [0.0, 0.2, 1.0], amount: 0.6, step: 0, fade: 0.0 }]);
        let world = &world;
        let cx = DrawCx { thing, resolved: r, scene: &resolved, world, log: &log, time: 0.0, video_time: 0.0 };
        let drawn = ball_colors(&cx).unwrap();
        let sim = ball_colour_in_sim(look, world, ThingId(2), 0);
        assert_eq!(drawn(0), sim);
        assert_ne!(sim, plain);
        // The ball with no entry is unchanged.
        assert_eq!(drawn(1), ball_colour_in_sim(look, world, ThingId(2), 1));
    }
}
