//! Small pictures of scenes: a shared thumbnail-sized renderer, and textures the explorer, the
//! batch window and the start window show them in.

// Used by the explorer and the start window, which come next.
#![allow(dead_code)]

use crate::theme;
use bpe_core::{Resolved, Scene};
use bpe_gfx::{Frame, OUTPUT_FORMAT, Renderer, build_frame_at, wgpu};
use bpe_kit::{Kit, resolve_scene};
use bpe_runtime::{Log, World};
use eframe::egui::{self, Ui, Vec2};
use eframe::egui_wgpu::RenderState;

/// A thumbnail's size in pixels: the video's shape, an eighth of its size.
pub const THUMB: (u32, u32) = (135, 240);

/// A fresh run of `scene` to `time`, and the picture at that moment (sim and video clocks the
/// same, as there is no beat warp to follow here).
pub fn still(scene: &Scene, kit: &Kit, time: f64) -> (Resolved, Frame) {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    let mut frame = Frame::default();
    build_frame_at(scene, &resolved, &world, &log, time, time, &kit.draws, &mut frame);
    (resolved, frame)
}

/// One thumbnail-sized renderer, drawn into and copied out for every thumbnail in turn.
/// Liquids and swarms live on the graphics card's own layers, which the thumbnails skip.
pub struct Thumbnailer {
    renderer: Renderer,
}

impl Thumbnailer {
    pub fn new(device: &wgpu::Device) -> Self {
        Self { renderer: Renderer::new(device, THUMB.0, THUMB.1) }
    }

    /// Draws `frame` and copies the picture into `into`.
    pub fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, into: &Thumb) {
        self.renderer.render(device, queue, frame);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("thumbnail copy") });
        encoder.copy_texture_to_texture(
            self.renderer.output_texture().as_image_copy(),
            into.texture.as_image_copy(),
            wgpu::Extent3d { width: THUMB.0, height: THUMB.1, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
    }
}

/// A picture egui can show: a texture of [`THUMB`] size registered with egui's renderer.
pub struct Thumb {
    texture: wgpu::Texture,
    id: egui::TextureId,
}

impl Thumb {
    pub fn new(state: &RenderState) -> Thumb {
        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("thumbnail"),
            size: wgpu::Extent3d { width: THUMB.0, height: THUMB.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = state.renderer.write().register_native_texture(&state.device, &view, wgpu::FilterMode::Linear);
        Thumb { texture, id }
    }

    pub fn id(&self) -> egui::TextureId {
        self.id
    }

    /// Lets egui forget the texture; the texture itself goes with the value.
    pub fn free(self, state: &RenderState) {
        state.renderer.write().free_texture(&self.id);
    }
}

/// The size a thumbnail is shown at for a given height, keeping the video's shape.
pub fn shown_size(height: f32) -> Vec2 {
    Vec2::new(height * THUMB.0 as f32 / THUMB.1 as f32, height)
}

/// Shows a thumbnail, or a dark box with `text` in it when there is no picture (no graphics
/// device, or not drawn yet).
pub fn show(ui: &mut Ui, thumb: Option<&Thumb>, size: Vec2, text: &str) -> egui::Response {
    match thumb {
        Some(thumb) => ui.add(egui::Image::new((thumb.id(), size)).corner_radius(6.0)),
        None => placeholder(ui, size, text),
    }
}

/// A dark box with `text` in it, where a picture would be.
pub fn placeholder(ui: &mut Ui, size: Vec2, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, 6.0, theme::BACKGROUND);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.0), theme::MUTED);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::{kit, new_scene};

    #[test]
    fn still_builds_a_frame_at_the_time() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let (resolved, frame) = still(&scene, &kit, 1.0);
        assert_eq!(resolved.things.len(), 3);
        assert_eq!(frame.time, 1.0);
        assert_eq!(frame.balls.len(), 1, "the new scene's one ball");
        assert_eq!((frame.width, frame.height), (1080, 1920));
        assert_eq!(THUMB, (135, 240), "the video's shape, an eighth of its size");
    }
}
