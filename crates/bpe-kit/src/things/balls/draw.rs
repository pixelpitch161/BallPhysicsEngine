//! Drawing the balls and their hit flashes.

use bpe_core::{SlotId, ThingId};
use bpe_gfx::{BallInstance, DrawCx, FlashInstance, Frame, HitMark, ThingDraw, blend_alpha, thing_code};
use bpe_runtime::EventKind;

use crate::modules::look::basic::{self, BallColours};
use crate::modules::look::draw::{BallInputs, colors, for_balls, look_of};
use crate::paint::{Pattern, colour_at, inside_gpu, vary_gpu};

pub struct BallsDraw;

/// The speed at which a flash is at full strength.
const FULL_FLASH_SPEED: f32 = 600.0;
/// How far back hits are listed for particles and shaders, in seconds.
pub const HIT_MEMORY: f64 = 3.0;

/// A ball's own number in 0 to 1, the same every frame and on every machine, for patterns and
/// effects that vary from ball to ball.
pub fn ball_seed(thing: ThingId, index: usize) -> f32 {
    let mut x = (index as u32).wrapping_mul(0x9e37_79b9) ^ thing.0.wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2f;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x8a7d_9c3b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The linear-light colour of each ball of the thing being drawn, from its look's Colours
/// (painted by place, start, order, speed, hits, steps, beats or time) and its hue shift per hit,
/// when it has a look and bodies, with any tint a rule gave the ball laid over it as strongly as
/// the tint's alpha says. Trails and particles take their colours from here too.
pub fn ball_colors<'a>(cx: &DrawCx<'a>) -> Option<impl Fn(usize) -> [f32; 4] + 'a> {
    let look = look_of(cx)?;
    let bodies = cx.world.bodies.get(&cx.thing.id)?;
    let look_color = colors(look, cx, bodies);
    Some(move |i: usize| {
        let c = look_color(i);
        let Some(t) = bodies.tint.get(i) else { return c };
        [c[0] + (t[0] - c[0]) * t[3], c[1] + (t[1] - c[1]) * t[3], c[2] + (t[2] - c[2]) * t[3], c[3]]
    })
}

impl ThingDraw for BallsDraw {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let id = cx.thing.id;
        let Some(bodies) = cx.world.bodies.get(&id) else { return };
        let Some(look) = look_of(cx) else { return };
        let Some(color) = ball_colors(cx) else { return };
        let classic = look.is_classic_ball();
        // The outline of a styled ball reads the same balls as the fill.
        let fill = for_balls(look.fill.clone());
        let outline = if classic { Default::default() } else { for_balls(look.outline_paint.clone()) };
        let inputs = BallInputs::new(cx, bodies, &[&fill, &outline]);
        let time = cx.time as f32;
        let painted = look.inside.pattern != Pattern::None;
        let inside = if painted { look.inside_at(time) } else { look.inside.clone() };
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let seed = ball_seed(id, i);
            let code = thing_code(id) + seed * 0.5;
            let (center, velocity, radius) = (bodies.position[i], bodies.velocity[i], bodies.radius[i]);
            let mut paint = bpe_gfx::PaintGpu::default();
            if painted {
                let at = inputs.get(&fill, i);
                paint = inside_gpu(&inside, vary_gpu(&fill, at.seed, at.time), &mut frame.gradients, time);
            }
            if !classic {
                if painted {
                    // Measured on the turned shape: the pattern follows the ball's way only when
                    // it turns with the ball, not when the shape faces its motion.
                    paint.angle += look.inside_turn_on_shape(velocity);
                }
                let colours =
                    BallColours { fill: color(i), outline: colour_at(&outline, &inputs.get(&outline, i)), paint };
                frame.looks.push(basic::instance(&look, colours, center, velocity, radius, time, code, seed));
                continue;
            }
            if painted {
                // The pattern turns with the ball's own Turn and Spin, as on a styled ball.
                paint.angle += look.inside_turn(velocity, time);
            }
            frame.balls.push(BallInstance {
                center: center.into(),
                radius,
                glow: look.glow,
                color: blend_alpha(color(i)),
                code: [code, seed, 0.0, 0.0],
                paint,
            });
        }

        // Recent hits, for the flashes here and for the particle and shader modules. Only the
        // flashes' worth is scanned when nothing else wants them.
        let rate = cx.world.steps_per_second;
        let flashing = look.flash > 0.0 && look.flash_seconds > 0.0;
        // The scene settings' effects aren't passed down but still pulse on every hit.
        let scene_effects = cx.scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Shader));
        let wants_hits =
            scene_effects || [SlotId::Particles, SlotId::Shader].iter().any(|s| cx.resolved.slots.contains_key(s));
        let memory = if wants_hits {
            HIT_MEMORY
        } else if flashing {
            look.flash_seconds as f64
        } else {
            return;
        };
        let since = ((cx.time - memory) * rate as f64).floor().max(0.0) as u64;
        for e in cx.log.since(since) {
            if e.kind != EventKind::Hit || e.a.thing != id {
                continue;
            }
            let time = e.time(rate);
            if time > cx.time {
                continue;
            }
            frame.hits.push(HitMark {
                thing: id,
                other: e.b.map(|b| b.thing),
                point: e.point.into(),
                normal: e.normal.into(),
                time: time as f32,
                speed: e.speed,
                color: color(e.a.item as usize),
            });
            if !flashing {
                continue;
            }
            let age = (cx.time - time) as f32 / look.flash_seconds;
            if !(0.0..=1.0).contains(&age) {
                continue;
            }
            let intensity = (e.speed / FULL_FLASH_SPEED).min(1.5) * (-3.0 * age).exp() * 0.35 * look.flash / 0.4;
            if intensity < 0.002 {
                continue;
            }
            let c = color(e.a.item as usize);
            frame.flashes.push(FlashInstance {
                center: e.point.into(),
                size: look.flash_size,
                color: [
                    (c[0] + 1.0) * 0.5 * intensity,
                    (c[1] + 1.0) * 0.5 * intensity,
                    (c[2] + 1.0) * 0.5 * intensity,
                    1.0,
                ],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene, registry};
    use bpe_core::{ModuleId, Params, SlotId, SlotLink, ThingId, Value};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    #[test]
    fn a_missing_look_module_falls_back_to_the_folder_above() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        // The ball sits in a folder with a plain look, and points at a module that doesn't exist.
        let folder = crate::add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        crate::fill_slot(&mut scene, &reg, folder, SlotId::Look).unwrap();
        scene.set_parent(ThingId(2), folder, bpe_core::Attach::Group).unwrap();
        scene
            .things
            .get_mut(&ThingId(2))
            .unwrap()
            .slots
            .insert(SlotId::Look, SlotLink::Linked { module: ModuleId(99), overrides: Params::default() });
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(!resolved.problems.is_empty());
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls.len(), 1);
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.balls[0].color, [1.0, 1.0, 1.0, 1.0]); // the folder's plain look
        assert_eq!(frame.balls[0].glow, 0.12);
        assert_eq!((frame.width, frame.height), (1080, 1920));
    }

    #[test]
    fn a_styled_look_draws_looks_instead_of_balls_and_still_flashes() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let styled = scene.add_module(&reg, "look.basic", "Star look").unwrap();
        let module = scene.library.modules.get_mut(&styled).unwrap();
        module.params.set("ball_shape", bpe_core::Value::Choice("star".into()));
        module.params.set("flash", bpe_core::Value::Number(1.0));
        scene.link(&reg, ThingId(2), SlotId::Look, styled).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert!(frame.balls.is_empty());
        assert_eq!(frame.looks.len(), 1);
        assert_eq!(frame.looks[0].form[0], 5.0);
        assert_eq!(frame.looks[0].extent[1], bpe_gfx::thing_code(ThingId(2)) + frame.looks[0].seed * 0.5);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent);
    }

    /// The frame for one ball wearing a look.basic module set up by `f`.
    fn basic_frame(f: impl FnOnce(&mut Params)) -> Frame {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let look = scene.add_module(&reg, "look.basic", "Look").unwrap();
        f(&mut scene.library.modules.get_mut(&look).unwrap().params);
        scene.link(&reg, ThingId(2), SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn shiny_circle_draws_through_the_classic_ball_path() {
        let frame = basic_frame(|_| {});
        assert!(!frame.balls.is_empty());
        assert!(frame.looks.is_empty());
    }

    #[test]
    fn flat_or_see_through_or_another_shape_draws_a_styled_instance() {
        let flat = basic_frame(|p| p.set("style", Value::Choice("flat".into())));
        assert!(!flat.looks.is_empty() && flat.balls.is_empty());
        assert_eq!(flat.looks[0].form[1], 0.0, "flat is unlit");
        // A shiny circle the classic ball can't draw: half see-through.
        let shiny = basic_frame(|p| {
            p.set("style", Value::Choice("shiny".into()));
            p.set("opacity", Value::Number(0.5));
        });
        assert!(!shiny.looks.is_empty() && shiny.balls.is_empty());
        assert_eq!(shiny.looks[0].form[1], 1.0, "shiny is lit like a sphere");
        let star = basic_frame(|p| {
            p.set("ball_shape", Value::Choice("star".into()));
            p.set("style", Value::Choice("shiny".into()));
        });
        assert!(!star.looks.is_empty() && star.balls.is_empty());
        assert_eq!(star.looks[0].form[..2], [5.0, 1.0], "a shiny star is lit like the classic ball");
    }

    #[test]
    fn a_shiny_circle_with_an_outline_draws_a_styled_shiny_instance() {
        let frame = basic_frame(|p| p.set("outline", Value::Number(3.0)));
        assert!(frame.balls.is_empty());
        assert_eq!(frame.looks.len(), 1);
        assert_eq!(frame.looks[0].form[..2], [0.0, 1.0]);
        assert_eq!(frame.looks[0].look[0], 3.0);
        // Its default sibling still takes the classic path.
        let plain = basic_frame(|_| {});
        assert!(plain.looks.is_empty() && plain.balls.len() == 1);
    }

    #[test]
    fn inside_linear_on_a_ball_sets_its_paint() {
        let frame = basic_frame(|p| p.set("in_pattern", Value::Choice("linear".into())));
        assert_eq!(frame.balls.len(), 1);
        assert_eq!(frame.balls[0].paint.pattern, 1.0);
        assert_eq!(frame.balls[0].paint.row, -2.0, "a shade of the ball's own colour by default");
    }

    #[test]
    fn a_tinted_ball_is_drawn_in_its_tint() {
        let (scene, resolved, kit) = crate::testing::scene_with_balls("single", |_| {});
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let plain = {
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
            frame.balls[0].color
        };
        world.bodies.get_mut(&ThingId(2)).unwrap().tint[0] = [1.0, 0.0, 0.0, 1.0];
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        world.bodies.get_mut(&ThingId(2)).unwrap().tint[0] = [1.0, 0.0, 0.0, 0.5];
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        let half = frame.balls[0].color;
        for i in 0..3 {
            assert!(
                (half[i] - (plain[i] + ([1.0, 0.0, 0.0][i] - plain[i]) * 0.5)).abs() < 1e-5,
                "{half:?} vs {plain:?}"
            );
        }
    }

    #[test]
    fn a_look_that_is_off_draws_nothing() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.set_slot_off(ThingId(1), SlotId::Look);
        scene.set_slot_off(ThingId(2), SlotId::Look);
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.balls.is_empty() && frame.shapes.is_empty());
    }

    #[test]
    fn colours_can_follow_hits_and_shift_hue_per_hit() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(1));
            p.set("velocity", bpe_core::Value::Vec2([0.0, 0.0]));
        });
        let m =
            bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
        let look = scene.library.modules.get_mut(&m).unwrap();
        look.params.set(
            "colours",
            bpe_core::Value::Gradient(bpe_core::Gradient::even(&[
                bpe_core::Color::rgb(255, 0, 0),
                bpe_core::Color::rgb(0, 0, 255),
            ])),
        );
        look.params.set("follow", bpe_core::Value::Choice("hits".into()));
        look.params.set("span", bpe_core::Value::Number(2.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&ThingId(2)).unwrap().hits[1] = 2;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.balls[1].color, [0.0, 0.0, 1.0, 1.0]);
        // By order: first and last colour.
        scene.library.modules.get_mut(&m).unwrap().params.set("follow", bpe_core::Value::Choice("order".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!((frame.balls[0].color, frame.balls[1].color), ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        // By speed: both still, so both red.
        scene.library.modules.get_mut(&m).unwrap().params.set("follow", bpe_core::Value::Choice("speed".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[1].color, [1.0, 0.0, 0.0, 1.0]);
        // Red turned 240° (two hits of 120°) is blue; the first ball with no hits stays red.
        scene.library.modules.get_mut(&m).unwrap().params.set("hue_per_hit", bpe_core::Value::Number(120.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert!(frame.balls[1].color[2] > 0.9 && frame.balls[1].color[0] < 0.1, "{:?}", frame.balls[1].color);
    }

    #[test]
    fn balls_leave_a_hit_mark_in_their_colour() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        // Hits are listed for the modules that use them: here the ball's own burst.
        let m = scene.add_module(&kit.registry, "particles.burst", "Sparks").unwrap();
        scene.link(&kit.registry, ThingId(2), SlotId::Particles, m).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).collect();
        assert!(!recent.is_empty(), "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent.len());
        let mark = frame.hits[0];
        assert_eq!((mark.thing, mark.other), (ThingId(2), Some(ThingId(1))));
        assert_eq!(mark.point, <[f32; 2]>::from(recent[0].point));
        assert_eq!(mark.speed, recent[0].speed);
        assert!((mark.time - recent[0].time(rate) as f32).abs() < 1e-6);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(mark.color, pink);
        // A hit further back than the memory is not listed.
        playback.seek(10.0);
        let old: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 7.0).collect();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), old.len());
        // Without a module that wants them, only the flashes' quarter second is listed.
        scene.set_slot_off(ThingId(2), SlotId::Particles);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        let flash_worth = playback.log().events().iter().filter(|e| e.time(rate) > 10.0 - 0.25).count();
        assert_eq!(frame.hits.len(), flash_worth);
    }

    #[test]
    fn an_effect_on_the_scene_settings_still_gets_the_balls_hits() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        let fx = scene.add_module(&kit.registry, "shader.kaleidoscope", "Slices").unwrap();
        scene.link(&kit.registry, ThingId::ROOT, SlotId::Shader, fx).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(!resolved.things[&ThingId(2)].slots.contains_key(&SlotId::Shader), "the effect is not passed down");
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).count();
        assert!(recent > 0, "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent, "the scene's effect pulses on every hit it remembers");
    }

    #[test]
    fn stepping_on_hits_reads_the_log_and_the_hit_looks_once_a_frame() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(3));
        });
        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        params.set("follow", Value::Choice("step".into()));
        params.set("take_hit_colour", Value::Bool(true));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(6.0);
        let hit = playback.world().bodies[&ThingId(2)].hits.iter().filter(|&&h| h > 0).count();
        assert!(hit >= 4, "only {hit} balls hit something");
        let (searches, reads) = (&crate::paint::eval::LOG_SEARCHES, &crate::modules::look::draw::LOOK_READS);
        let (s0, r0) = (searches.with(|n| n.get()), reads.with(|n| n.get()));
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 6.0, &kit.draws, &mut frame);
        let (s1, r1) = (searches.with(|n| n.get()) - s0, reads.with(|n| n.get()) - r0);
        // The balls' draw reads their colours twice (once for the balls, once for their paints);
        // each reads the log once, and each other thing's look once, whatever the number of balls.
        assert!(s1 <= 2, "{s1} searches of the log for {hit} balls that hit");
        assert!(r1 <= 2, "{r1} reads of the hit things' looks for {hit} balls that hit");
    }

    /// The look module of the balls in `scene`.
    fn ball_look(scene: &bpe_core::Scene, kit: &crate::Kit) -> bpe_core::ModuleId {
        bpe_core::resolve_slot(scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module
    }

    const RED: bpe_core::Color = bpe_core::Color::rgb(255, 0, 0);
    const BLUE: bpe_core::Color = bpe_core::Color::rgb(0, 0, 255);

    #[test]
    fn row_follow_is_place_from_spawn_at_90_degrees() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(3));
            p.set("spacing", Value::Number(40.0));
        });
        scene.things.get_mut(&ThingId(2)).unwrap().transform.position = glam::Vec2::new(30.0, -200.0);
        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        params.set("follow", Value::Choice("start".into()));
        params.set("angle", Value::Number(90.0));
        params.set("centre", Value::Vec2([30.0, -200.0]));
        // From the first row's middle to the last's.
        params.set("span", Value::Number(80.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls.len(), 6);
        let near = |a: [f32; 4], b: [f32; 4]| (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5);
        let (red, blue) = (RED.to_linear(), BLUE.to_linear());
        // Where the balls started counts, not where they are now.
        assert!(near(frame.balls[0].color, red) && near(frame.balls[1].color, red), "{:?}", frame.balls);
        let middle = bpe_core::Gradient::even(&[RED, BLUE]).sample(0.5);
        assert!(near(frame.balls[2].color, middle) && near(frame.balls[3].color, middle), "{:?}", frame.balls);
        assert!(near(frame.balls[4].color, blue) && near(frame.balls[5].color, blue), "{:?}", frame.balls);
        // Hits flash: the flashes match the log.
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent, "{:?}", playback.log().events());
    }

    #[test]
    fn a_plain_look_paints_nothing_inside_and_rings_paint_from_the_ball_s_place() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.balls.iter().all(|b| b.paint == bpe_gfx::PaintGpu::default()));
        assert!(frame.gradients.len() == 1, "nothing is baked for a plain look");

        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        params.set("follow", Value::Choice("order".into()));
        params.set("in_pattern", Value::Choice("rings".into()));
        params.set("in_spin", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 1, "a shade of each ball's own colour bakes nothing");
        let paint = frame.balls[1].paint;
        assert_eq!((paint.row, paint.pattern, paint.angle), (-2.0, 6.0, 270.0));
        assert_eq!(paint.t0, -0.5, "the default shade");
        // Its own gradient takes one row the balls share.
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("in_from", Value::Choice("own_gradient".into()));
        params.set("in_colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 2, "the balls share one row");
        assert!(frame.balls.iter().all(|b| b.paint.row == 1.0 && b.paint.pattern == 6.0));
    }

    #[test]
    fn a_styled_ball_paints_its_inside_and_outline_from_their_own_colours() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let styled = scene.add_module(&reg, "look.basic", "Dotted").unwrap();
        let params = &mut scene.library.modules.get_mut(&styled).unwrap().params;
        params.set("style", Value::Choice("flat".into()));
        params.set("outline", Value::Number(3.0));
        params.set("outline_colours", Value::Gradient(bpe_core::Gradient::even(&[BLUE, RED])));
        params.set("outline_follow", Value::Choice("hits".into()));
        params.set("outline_span", Value::Number(4.0));
        params.set("in_pattern", Value::Choice("dots".into()));
        params.set("in_from", Value::Choice("own_gradient".into()));
        params.set("in_colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        scene.link(&reg, ThingId(2), SlotId::Look, styled).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&ThingId(2)).unwrap().hits[0] = 2;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        let ball = frame.looks[0];
        assert_eq!(ball.outline_color, bpe_core::Gradient::even(&[BLUE, RED]).sample(0.5));
        assert_eq!((ball.paint.row, ball.paint.pattern), (1.0, 7.0), "the dots inside");
        assert_eq!(ball.form[1], 0.0, "flat");
    }

    #[test]
    fn stepping_on_hits_can_take_the_colour_of_what_was_hit() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        params.set("follow", Value::Choice("step".into()));
        params.set("take_hit_colour", Value::Bool(true));
        // The arena wears the root's look; paint it green.
        let root = bpe_core::resolve_slot(&scene, &kit.registry, ThingId(1), SlotId::Look, &mut Vec::new()).unwrap();
        let green = bpe_core::Color::rgb(0, 255, 0);
        scene
            .library
            .modules
            .get_mut(&root.module)
            .unwrap()
            .params
            .set("colours", Value::Gradient(bpe_core::Gradient::solid(green)));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, RED.to_linear(), "no hits yet: the first colour");
        playback.seek(4.0);
        assert!(playback.world().bodies[&ThingId(2)].hits[0] > 0, "the ball hit the wall");
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, green.to_linear());
        // Without taking it, the ball steps through its own colours.
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("take_hit_colour", Value::Bool(false));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_ne!(frame.balls[0].color, green.to_linear());
    }

    #[test]
    fn a_ball_group_with_vary_hue_has_different_colours() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(2));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[RED])));
        params.set("vary_hue", Value::Number(40.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let build = || {
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
            frame.balls.iter().map(|b| b.color).collect::<Vec<_>>()
        };
        let colours = build();
        assert_eq!(colours.len(), 8);
        let mut distinct: Vec<[f32; 4]> = Vec::new();
        for c in &colours {
            if !distinct.iter().any(|d| (0..3).all(|k| (d[k] - c[k]).abs() < 1e-4)) {
                distinct.push(*c);
            }
        }
        assert!(distinct.len() >= 6, "only {} distinct colours: {colours:?}", distinct.len());
        // The same frame built twice is the same.
        assert_eq!(build(), colours);
    }

    #[test]
    fn an_inside_pattern_carries_each_ball_s_variation_to_the_gpu() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let m = ball_look(&scene, &kit);
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("in_pattern", Value::Choice("stripes".into()));
        params.set("in_from", Value::Choice("own_gradient".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        assert!(frame.balls.iter().all(|b| b.paint.vary == [0.0, 0.0, 1.0, 0.0]), "no variation is the identity");
        let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
        params.set("vary_hue", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        let turns: Vec<f32> = frame.balls.iter().map(|b| b.paint.vary[0]).collect();
        assert!(turns.iter().all(|t| t.abs() <= 0.25), "{turns:?}");
        assert!(turns[0] != turns[1] && turns[1] != turns[2], "{turns:?}");
    }
}
