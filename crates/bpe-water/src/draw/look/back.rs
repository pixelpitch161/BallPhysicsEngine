//! The Back card's Picture: a PNG the scene keeps (or a file), read once, kept on the card,
//! cover-fitted to the course.

use std::sync::Arc;

use glam::Vec2;

use super::inputs::{BackLook, BackPicture as Named};
use super::numbers::course_box;
use crate::solid::Layout;

/// A picture is read at most this many pixels a side (larger ones are scaled down).
const MOST_SIDE: u32 = 4096;
/// The most bytes a PNG may decode to.
const MOST_BYTES: usize = 512 << 20;

/// The last back picture read: what it was read from, and its texture (`None` when it couldn't
/// be read).
#[derive(Default)]
pub(crate) struct BackPicture {
    read: Option<Named>,
    texture: Option<(wgpu::TextureView, u32, u32)>,
}

impl BackPicture {
    /// The picture `back` names on the card (read when it changes), and its box in the world (lo,
    /// size), cover-fitted to the course and centred; `None` when the back isn't a picture or it
    /// can't be read.
    pub(crate) fn picture_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        back: &BackLook,
        layout: &Layout,
    ) -> Option<(Vec2, Vec2)> {
        let BackLook::Picture(named) = back else {
            // (Nothing to tell about a back that isn't a picture.)
            self.read = None;
            return None;
        };
        if self.read.as_ref() != Some(named) {
            self.read = Some(named.clone());
            let pixels = match &named.kept {
                Some(bytes) => read_png(std::io::Cursor::new(Arc::clone(bytes))),
                None if named.name.trim().is_empty() => None,
                None => std::fs::File::open(&named.name).ok().and_then(|f| read_png(std::io::BufReader::new(f))),
            };
            self.texture = pixels.map(|(rgba, w, h)| (upload(device, queue, &rgba, w, h), w, h));
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
        self.read.is_some() && self.texture.is_none()
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
fn read_png(from: impl std::io::BufRead + std::io::Seek) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new_with_limits(from, png::Limits { bytes: MOST_BYTES });
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
            1 | 2 => [px[0], px[0], px[0], 255],
            _ => [px[0], px[1], px[2], 255],
        });
    }
    Some(scaled_down(rgba, w, h))
}

/// RGBA pixels of `w` x `h` scaled down by a whole number with a box filter to [`MOST_SIDE`] a side
/// at most (a side shorter than the step keeps one pixel, from the pixels it has).
fn scaled_down(rgba: Vec<u8>, w: usize, h: usize) -> (Vec<u8>, u32, u32) {
    let step = w.max(h).div_ceil(MOST_SIDE as usize).max(1);
    if step == 1 {
        return (rgba, w as u32, h as u32);
    }
    let (sw, sh) = ((w / step).max(1), (h / step).max(1));
    let mut out = Vec::with_capacity(sw * sh * 4);
    for y in 0..sh {
        for x in 0..sw {
            let (mut sum, mut n) = ([0u32; 4], 0u32);
            for sy in (y * step..(y + 1) * step).filter(|&sy| sy < h) {
                for sx in (x * step..(x + 1) * step).filter(|&sx| sx < w) {
                    let at = (sy * w + sx) * 4;
                    for c in 0..4 {
                        sum[c] += u32::from(rgba[at + c]);
                    }
                    n += 1;
                }
            }
            out.extend(sum.map(|s| (s / n.max(1)) as u8));
        }
    }
    (out, sw as u32, sh as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_thinner_than_its_step_keeps_a_row() {
        // 8193 x 2: the step is 3, so the 2 rows become one, and the size matches the pixels.
        let (w, h) = (8193usize, 2usize);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 256) as u8, 0, 0, 255]).collect();
        let (out, sw, sh) = scaled_down(rgba, w, h);
        assert_eq!((sw, sh), (2731, 1));
        assert_eq!(out.len(), (sw * sh * 4) as usize);
        let (out, sw, sh) = scaled_down(vec![9; 5000 * 4], 5000, 1);
        assert_eq!((sw, sh), (2500, 1));
        assert_eq!(out.len(), (sw * sh * 4) as usize);
        assert!(out.iter().all(|&v| v == 9));
    }
}
