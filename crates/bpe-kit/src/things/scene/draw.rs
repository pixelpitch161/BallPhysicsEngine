//! Scene defaults in the picture: its size, background, bloom and exposure.

use bpe_core::Settings;
use bpe_gfx::{DrawCx, Frame, ThingDraw};

use super::SceneSettings;
use crate::paint::{Follow, Inputs, background_gpu, beats_at};

pub struct SceneDraw;

impl ThingDraw for SceneDraw {
    fn kind(&self) -> &'static str {
        "scene"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let s = SceneSettings::from_params(&cx.resolved.params);
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
        frame.background = background_gpu(&s.background, &inputs, &mut frame.gradients);
        frame.bloom = s.bloom;
        frame.exposure = s.exposure;
        frame.time = cx.time as f32;
        frame.background_on_screen = s.background_on_screen;
    }
}
