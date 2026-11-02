//! Drawing the canvas: one canvas layer in the top frame, at the canvas's place and turn now. The
//! renderer replays the canvas's marks into its textures and draws them with its style.

use bpe_core::Settings;
use bpe_gfx::{CanvasLayer, CanvasPlace, DrawCx, Frame, ThingDraw, clamp_texels};

use super::sim::canvas_area;
use super::{CanvasLayerPos, CanvasSettings, StartsAs};

pub struct CanvasDraw;

/// The renderer's place for the canvas's layer.
fn place(layer: CanvasLayerPos) -> CanvasPlace {
    match layer {
        CanvasLayerPos::BehindEverything => CanvasPlace::BehindEverything,
        CanvasLayerPos::BehindBalls => CanvasPlace::BehindBalls,
        CanvasLayerPos::OnTop => CanvasPlace::OnTop,
    }
}

impl ThingDraw for CanvasDraw {
    fn kind(&self) -> &'static str {
        "canvas"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some((pose, size)) = canvas_area(cx.world, cx.thing.id) else { return };
        // The look comes from the live settings: none of it rebuilds the world.
        let settings = CanvasSettings::from_params(&cx.resolved.params);
        let detail = if settings.detail.is_finite() { settings.detail.max(0.0) } else { 1.0 };
        let texels = clamp_texels([(size.x * detail).round() as u32, (size.y * detail).round() as u32]);
        // Always the top frame's list: the renderer draws no other frame's canvases, and mixes
        // each in by its own blend.
        frame.canvases.push(CanvasLayer {
            thing: cx.thing.id,
            center: pose.position.to_array(),
            angle: pose.angle,
            size: size.to_array(),
            texels,
            place: place(settings.layer),
            start: (settings.starts_as == StartsAs::Colour).then(|| settings.start_colour.to_linear()),
            fade: settings.fade.max(0.0),
            blend: settings.blend,
            opacity: settings.opacity.clamp(0.0, 1.0),
            time: cx.time as f32,
        });
    }
}
