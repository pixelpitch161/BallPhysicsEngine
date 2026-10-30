//! Scene defaults in the picture: its size, background, bloom and exposure.

use bpe_core::Settings;
use bpe_gfx::{DrawCx, Frame, ThingDraw};

use super::SceneSettings;
use crate::things::camera::CameraSettings;

pub struct SceneDraw;

impl ThingDraw for SceneDraw {
    fn kind(&self) -> &'static str {
        "scene"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let s = SceneSettings::from_params(&cx.resolved.params);
        frame.width = s.width.max(2);
        frame.height = s.height.max(2);
        frame.background_top = s.background_top.to_linear();
        frame.background_bottom = s.background_bottom.to_linear();
        frame.bloom = s.bloom;
        frame.exposure = s.exposure;
        frame.time = cx.time as f32;
        frame.background_on_screen = s.background_on_screen;
        frame.background_span = background_span(cx.scene, s.height.max(2) as f32 * 0.5);
    }
}

/// The world heights the background spans when it is painted across the world: the camera's
/// world box, or one shot's height around where the camera is set.
fn background_span(scene: &bpe_core::Resolved, half_height: f32) -> [f32; 2] {
    let camera = scene.things.values().find(|r| r.kind == "camera").map(|r| CameraSettings::from_params(&r.params));
    match camera {
        Some(c) if c.stay_inside => [c.box_centre.y - c.box_size.y.abs() * 0.5, c.box_centre.y + c.box_size.y.abs() * 0.5],
        Some(c) => {
            let half = half_height / c.zoom.max(0.01);
            [c.position.y - half, c.position.y + half]
        }
        None => [-half_height, half_height],
    }
}
