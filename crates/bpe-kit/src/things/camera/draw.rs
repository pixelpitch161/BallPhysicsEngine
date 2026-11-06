//! The camera draws nothing; in a lit Stage scene its Stage camera (lighting part 5) tells the
//! renderer where the picture is seen from.

use bpe_core::Settings;
use bpe_gfx::{DrawCx, Frame, LightScene, ThingDraw};

use super::CameraSettings;

pub struct CameraDraw;

impl ThingDraw for CameraDraw {
    fn kind(&self) -> &'static str {
        "camera"
    }

    fn draw(&self, _cx: &DrawCx, _frame: &mut Frame) {}

    fn light(&self, cx: &DrawCx, scene: &mut LightScene) {
        // Only a Stage has depth to swing round; Flat keeps the front shot.
        if scene.settings.stage[0] < 0.5 {
            return;
        }
        scene.camera = CameraSettings::from_params(&cx.resolved.params).stage();
    }
}
