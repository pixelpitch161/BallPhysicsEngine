//! Drawing streaks from the runtime's position history.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{DrawCx, Frame, ModuleDraw, push_streak};
use glam::Vec2;

use super::{StreakTrail, TrailBlend, TrailSource};
use crate::things::balls::draw::ball_colors;

pub struct StreakDraw;

impl ModuleDraw for StreakDraw {
    fn kind(&self) -> &'static str {
        "trail.streak"
    }

    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
        let Some(rslot) = cx.resolved.slots.get(&slot) else { return };
        let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
        let Some(history) = cx.world.history.get(&cx.thing.id) else { return };
        let trail = StreakTrail::from_params(&rslot.params);
        if trail.brightness <= 0.0 || trail.width <= 0.0 {
            return;
        }
        let samples = (trail.seconds / history.interval(cx.world.dt())).ceil().max(1.0) as usize;
        let ball_color = ball_colors(cx);
        let own = trail.color.to_linear();
        let mut points: Vec<Vec2> = Vec::with_capacity(samples + 1);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            points.clear();
            points.push(bodies.position[i]);
            points.extend(history.points(i).take(samples));
            let base = match trail.source {
                TrailSource::Ball => ball_color.as_ref().map_or(own, |color| color(i)),
                TrailSource::Own => own,
            };
            let k = trail.brightness;
            // An additive colour glows whatever the blend says.
            let color = match trail.blend {
                TrailBlend::Cover if base[3] > 0.0 => [base[0], base[1], base[2], (base[3] * k).min(1.0)],
                _ => [base[0] * k, base[1] * k, base[2] * k, -1.0],
            };
            push_streak(&mut frame.trails, &points, trail.width * bodies.radius[i], trail.taper, color, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene, registry};
    use bpe_core::{SlotId, ThingId, Value};
    use bpe_gfx::{Frame, build_frame};

    #[test]
    fn a_root_streak_follows_every_ball_in_its_colour() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let trail = scene.add_module(&reg, "trail.streak", "Streak").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Trail, trail).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
        // Half a second at 60 samples a second: 30 segments of 6 vertices, less any the ball
        // didn't move for.
        assert!(!frame.trails.is_empty());
        assert!(frame.trails.len() <= 30 * 6);
        assert_eq!(frame.trails.len() % 6, 0);
        let ball = playback.world().bodies[&ThingId(2)].position[0];
        let head = glam::Vec2::from(frame.trails[0].position);
        assert!((head - ball).length() <= 24.0 * 0.5 + 1.0 + 1e-3, "{head} is not at the ball {ball}");
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        // The first vertex is at or just behind the head, where the fade has barely begun.
        let c = frame.trails[0].color;
        assert_eq!(c[3], -1.0);
        let fade = c[0] / (pink[0] * 0.5);
        assert!(fade > 0.9 && fade <= 1.0 + 1e-6, "{c:?}");
        assert!((c[2] / c[0] - pink[2] / pink[0]).abs() < 1e-4, "{c:?}");

        // Its own colour, covering, at the brightness as opacity.
        let m = scene.library.modules.get_mut(&trail).unwrap();
        m.params.set("source", Value::Choice("own".into()));
        m.params.set("blend", Value::Choice("cover".into()));
        m.params.set("color", Value::Color(bpe_core::Color::rgb(0, 255, 0)));
        let resolved = bpe_core::resolve(&scene, &reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
        let c = frame.trails[0].color;
        assert_eq!(c[..3], [0.0, 1.0, 0.0]);
        assert!(c[3] > 0.45 && c[3] <= 0.5, "{c:?}");
        // Without a trail nothing is drawn and no history is kept.
        scene.set_slot_off(ThingId::ROOT, SlotId::Trail);
        let resolved = bpe_core::resolve(&scene, &reg);
        let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        assert!(world.history.is_empty());
        build_frame(&scene, &resolved, &world, &bpe_runtime::Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.trails.is_empty());
    }
}
