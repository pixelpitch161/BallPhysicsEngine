//! The Back card's Picture: a PNG read once a path, kept on the card, cover-fitted to the course.

use glam::Vec2;

use super::inputs::BackLook;
use super::numbers::course_box;
use crate::solid::Layout;

/// A picture is read at most this many pixels a side (larger ones are scaled down).
const MOST_SIDE: u32 = 4096;
/// The most bytes a PNG may decode to.
const MOST_BYTES: usize = 512 << 20;

/// The last back picture read: its path, and its texture (`None` when it couldn't be read).
#[derive(Default)]
pub(crate) struct BackPicture {
    path: Option<String>,
    texture: Option<(wgpu::TextureView, u32, u32)>,
}

impl BackPicture {
    /// The picture `back` names on the card (read when its path changes), and its box in the world
    /// (lo, size), cover-fitted to the course and centred; `None` when the back isn't a picture or
    /// it can't be read.
    pub(crate) fn picture_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        back: &BackLook,
        layout: &Layout,
    ) -> Option<(Vec2, Vec2)> {
        let BackLook::Picture(path) = back else { return None };
        if self.path.as_deref() != Some(path.as_str()) {
            self.path = Some(path.clone());
            self.texture = read_png(path).map(|(rgba, w, h)| (upload(device, queue, &rgba, w, h), w, h));
        }
        let (_, w, h) = self.texture.as_ref()?;
        let (lo, hi) = course_box(layout);
        let size = hi - lo;
        let fit = (size.x / *w as f32).max(size.y / *h as f32);
        let shown = Vec2::new(*w as f32, *h as f32) * fit;
        Some((0.5 * (lo + hi) - 0.5 * shown, shown))
    }

    /// The picture's texture, when one is read.
    pub(crate) fn view(&self) -> Option<&wgpu::TextureView> {
        self.texture.as_ref().map(|(v, _, _)| v)
    }

    /// The back is a picture that can't be read.
    pub(crate) fn failed(&self) -> bool {
        self.path.is_some() && self.texture.is_none()
    }
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, rgba: &[u8], w: u32, h: u32) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water look back picture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // (Read as linear light.)
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: None },
        size,
    );
    texture.create_view(&Default::default())
}

/// A PNG's pixels as RGBA with its size, scaled down to [`MOST_SIDE`] a side; `None` when it can't
/// be read.
fn read_png(path: &str) -> Option<(Vec<u8>, u32, u32)> {
    if path.trim().is_empty() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new_with_limits(std::io::BufReader::new(file), png::Limits { bytes: MOST_BYTES });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    if w == 0 || h == 0 || buf.len() < w * h * channels {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in buf[..w * h * channels].chunks_exact(channels) {
        rgba.extend_from_slice(&match channels {
            1 => [px[0], px[0], px[0], 255],
            2 => [px[0], px[0], px[0], 255],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[1], px[2], 255],
        });
    }
    // Scaled down by a whole number with a box filter.
    let step = w.max(h).div_ceil(MOST_SIDE as usize).max(1);
    if step == 1 {
        return Some((rgba, w as u32, h as u32));
    }
    let (sw, sh) = (w / step, h / step);
    let mut out = Vec::with_capacity(sw * sh * 4);
    for y in 0..sh {
        for x in 0..sw {
            let mut sum = [0u32; 4];
            for j in 0..step {
                for i in 0..step {
                    let at = ((y * step + j) * w + x * step + i) * 4;
                    for c in 0..4 {
                        sum[c] += u32::from(rgba[at + c]);
                    }
                }
            }
            let n = (step * step) as u32;
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    Some((out, sw.max(1) as u32, sh.max(1) as u32))
}
