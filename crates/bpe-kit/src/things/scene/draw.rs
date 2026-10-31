//! Scene defaults in the picture: its size, background, bloom and exposure.

use bpe_core::Settings;
use bpe_gfx::{DrawCx, Frame, ThingDraw};
use glam::Vec2;

use super::SceneSettings;
use crate::paint::{Follow, Inputs, Paint, background_gpu, beats_at};
use crate::things::camera::CameraSettings;

pub struct SceneDraw;

impl ThingDraw for SceneDraw {
    fn kind(&self) -> &'static str {
        "scene"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let s = SceneSettings::from_params(&cx.resolved.params);
        frame.grade = s.grade();
        frame.width = s.width.max(2);
        frame.height = s.height.max(2);
        frame.video = (frame.width, frame.height);
        // Time runs on the simulation clock, beats on the video's.
        let beats = if s.background.follow == Follow::Beat {
            beats_at(cx.log, cx.video_time, cx.world.steps_per_second)
        } else {
            0.0
        };
        let inputs = Inputs { time: cx.time as f32, beats, ..Inputs::default() };
        let background = fit_background(s.background, cx.scene, frame.height as f32, s.background_on_screen);
        frame.background = background_gpu(&background, &inputs, &mut frame.gradients);
        frame.bloom = s.bloom;
        frame.exposure = s.exposure;
        frame.time = cx.time as f32;
        frame.background_on_screen = s.background_on_screen;
    }
}

/// The background with a span and centre left at their defaults fitted to the picture: on the
/// screen the span is the video's `height`; across the world the span and the centre's height
/// are the world heights the camera shows (its world box, or one shot's height around where it
/// is set), as the top-to-bottom background always did.
fn fit_background(mut p: Paint, scene: &bpe_core::Resolved, height: f32, on_screen: bool) -> Paint {
    let d = Paint::default();
    let (default_span, default_centre) = (p.span == d.span, p.centre == d.centre);
    if on_screen {
        if default_span {
            p.span = height;
        }
        return p;
    }
    let [low, high] = background_span(scene, height * 0.5);
    if default_span {
        p.span = high - low;
    }
    if default_centre {
        p.centre = Vec2::new(p.centre.x, (low + high) * 0.5);
    }
    p
}

/// The scene's camera, when it has one.
fn camera(scene: &bpe_core::Resolved) -> Option<CameraSettings> {
    scene.things.values().find(|r| r.kind == "camera").map(|r| CameraSettings::from_params(&r.params))
}

/// The world heights the background spans when it is painted across the world: the camera's
/// world box, or one shot's height around where the camera is set.
fn background_span(scene: &bpe_core::Resolved, half_height: f32) -> [f32; 2] {
    match camera(scene) {
        Some(c) if c.stay_inside => {
            [c.box_centre.y - c.box_size.y.abs() * 0.5, c.box_centre.y + c.box_size.y.abs() * 0.5]
        }
        Some(c) => {
            let half = half_height / c.zoom.max(0.01);
            [c.position.y - half, c.position.y + half]
        }
        None => [-half_height, half_height],
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::{ThingId, Value};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    /// The background a frame of the new scene paints after `f`'s changes to the scene's
    /// settings, with a camera when `camera` gives its settings.
    fn background(f: impl FnOnce(&mut bpe_core::Params), camera: Option<&[(&str, Value)]>) -> bpe_gfx::BackgroundPaint {
        drawn(f, camera).background
    }

    /// A frame of the new scene after `f`'s changes to the scene's settings, with a camera when
    /// `camera` gives its settings.
    fn drawn(f: impl FnOnce(&mut bpe_core::Params), camera: Option<&[(&str, Value)]>) -> Frame {
        let kit = crate::kit();
        let mut scene = crate::new_scene(&kit.registry);
        f(&mut scene.things.get_mut(&ThingId::ROOT).unwrap().params);
        if let Some(settings) = camera {
            let id = crate::add_thing(&mut scene, &kit.registry, "camera", ThingId::ROOT).unwrap();
            for (k, v) in settings {
                scene.things.get_mut(&id).unwrap().params.set(k, v.clone());
            }
        }
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    /// Where along the background's place a point on the screen `y` pixels below the middle sits.
    fn along(b: &bpe_gfx::BackgroundPaint, y: f32) -> f32 {
        let [angle, span, cx, cy] = b.place;
        let a = angle.to_radians();
        (glam::Vec2::new(0.0, y) - glam::Vec2::new(cx, cy)).dot(glam::Vec2::new(a.cos(), a.sin())) / span + 0.5
    }

    #[test]
    fn a_default_background_reaches_both_colours_at_the_screen_s_top_and_bottom() {
        let wide = background(
            |p| {
                p.set("width", Value::Int(1920));
                p.set("height", Value::Int(1080));
            },
            None,
        );
        assert!(along(&wide, -540.0).abs() < 1e-4 && (along(&wide, 540.0) - 1.0).abs() < 1e-4, "{:?}", wide.place);
        // A span that was set is kept.
        let set = background(
            |p| {
                p.set("height", Value::Int(1080));
                p.set("background_span", Value::Number(700.0));
            },
            None,
        );
        assert_eq!(set.place[1], 700.0);
        // The new scene's tall video already spans its default.
        assert_eq!(background(|_| {}, None).place[1], 1920.0);
    }

    #[test]
    fn a_default_background_across_the_world_spans_what_the_camera_shows() {
        let camera = [("position", Value::Vec2([100.0, 300.0])), ("zoom", Value::Number(2.0))];
        let b = background(|p| p.set("background_on_screen", Value::Bool(false)), Some(&camera));
        assert_eq!(b.place, [90.0, 960.0, 0.0, 300.0]);
        let boxed = [
            ("stay_inside", Value::Bool(true)),
            ("box_size", Value::Vec2([4000.0, 5000.0])),
            ("box_centre", Value::Vec2([0.0, -500.0])),
        ];
        let b = background(|p| p.set("background_on_screen", Value::Bool(false)), Some(&boxed));
        assert_eq!(b.place, [90.0, 5000.0, 0.0, -500.0]);
    }

    #[test]
    fn the_scene_s_grade_reaches_the_frame() {
        let plain = drawn(|_| {}, None);
        assert!(plain.grade.is_default(), "{:?}", plain.grade);
        let graded = drawn(
            |p| {
                p.set("grade_warmth", Value::Number(0.25));
                p.set("grade_vignette", Value::Number(0.5));
                p.set("grade_vignette_size", Value::Number(0.3));
                p.set("grade_map_amount", Value::Number(0.75));
            },
            None,
        );
        let g = &graded.grade;
        assert_eq!((g.warmth, g.vignette, g.vignette_size, g.map_amount), (0.25, 0.5, 0.3, 0.75));
        assert!(!g.is_default());
        assert!(graded.overlay.as_ref().is_none_or(|o| o.grade.is_default()));
    }

    #[test]
    fn a_grade_key_at_its_default_value_gives_a_default_grade_at_that_time() {
        use bpe_core::{Address, Ease, Owner};
        let kit = crate::kit();
        let mut scene = crate::new_scene(&kit.registry);
        let a = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "grade_warmth".into() };
        for (t, v) in [(0.0, 0.5), (1.0, 0.0), (2.0, 0.5)] {
            let i = scene.set_key(&kit.registry, &a, t, Value::Number(v)).unwrap();
            scene.set_ease(&a, i, Ease::Linear);
        }
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let at = |t: f64| {
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), t, &kit.draws, &mut frame);
            frame
        };
        assert!(!at(0.5).grade.is_default());
        assert!(at(1.0).grade.is_default(), "{:?}", at(1.0).grade);
    }
}
