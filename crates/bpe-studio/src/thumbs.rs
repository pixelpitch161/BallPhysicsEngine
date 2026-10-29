//! Small pictures of scenes: a shared thumbnail-sized renderer, and textures the explorer, the
//! batch window and the start window show them in.

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
#[allow(dead_code)] // The start window's pictures of the starter scenes.
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

    /// Draws `frame` and copies the picture into `into`, a [`THUMB`]-sized texture that can be
    /// copied to (a [`Thumb`]'s, or any other).
    pub fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, into: &wgpu::Texture) {
        self.renderer.render(device, queue, frame);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("thumbnail copy") });
        encoder.copy_texture_to_texture(
            self.renderer.output_texture().as_image_copy(),
            into.as_image_copy(),
            wgpu::Extent3d { width: THUMB.0, height: THUMB.1, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
    }
}

/// A [`THUMB`]-sized texture with `usage`, in the renderer's output format.
pub fn texture(device: &wgpu::Device, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("thumbnail"),
        size: wgpu::Extent3d { width: THUMB.0, height: THUMB.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage,
        view_formats: &[],
    })
}

/// A picture egui can show: a texture of [`THUMB`] size registered with egui's renderer.
pub struct Thumb {
    texture: wgpu::Texture,
    id: egui::TextureId,
}

impl Thumb {
    pub fn new(state: &RenderState) -> Thumb {
        let texture = texture(&state.device, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING);
        let view = texture.create_view(&Default::default());
        let id = state.renderer.write().register_native_texture(&state.device, &view, wgpu::FilterMode::Linear);
        Thumb { texture, id }
    }

    pub fn id(&self) -> egui::TextureId {
        self.id
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
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

    #[test]
    fn the_thumbnailer_draws_the_ball_into_a_small_texture() {
        let Ok((device, queue)) = bpe_gfx::headless_device() else {
            eprintln!("no graphics device; skipping");
            return;
        };
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let (_, frame) = still(&scene, &kit, 0.5);
        let mut thumbnailer = Thumbnailer::new(&device);
        let target = texture(&device, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC);
        thumbnailer.draw(&device, &queue, &frame, &target);
        let pixels = read_back(&device, &queue, &target);
        assert_eq!(pixels.len(), (THUMB.0 * THUMB.1 * 4) as usize);
        let colours: std::collections::BTreeSet<[u8; 4]> = pixels.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        assert!(colours.len() > 1, "a ball on a background, not one flat colour: {colours:?}");
    }

    /// The texture's pixels, row by row, RGBA.
    fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
        let row = (THUMB.0 * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("thumbnail read-back"),
            size: (row * THUMB.1) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            wgpu::Extent3d { width: THUMB.0, height: THUMB.1, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = slice.get_mapped_range().unwrap();
        data.chunks(row as usize).flat_map(|r| r[..(THUMB.0 * 4) as usize].to_vec()).collect()
    }
}
