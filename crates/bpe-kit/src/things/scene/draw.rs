//! Scene defaults in the picture: its size, background, bloom and exposure.

use bpe_core::Settings;
use bpe_gfx::{DrawCx, Frame, ThingDraw};

use super::SceneSettings;

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
    }
}
