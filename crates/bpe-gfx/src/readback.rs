//! Reading finished frames back to the CPU a few frames behind the drawing.

use crate::{Renderer, yuv};

/// Copies finished frames back to the CPU a few frames behind the drawing, so the graphics card
/// draws the next frame while the last one is being read and encoded. Frames come back in the
/// order they were drawn, tightly packed: RGBA bytes (sRGB-encoded), or as video colours worked out
/// on the card ([`Readback::with_layout`], review S23).
pub struct Readback {
    pub(crate) slots: Vec<ReadbackSlot>,
    pub(crate) next: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layout: yuv::PixelLayout,
    /// Each plane in the buffer: where it starts, its padded row, its row, its height.
    pub(crate) planes: Vec<(u64, u32, u32, u32)>,
    /// The conversion to video colours (none for RGBA).
    pub(crate) video: Option<yuv::VideoPlanes>,
}

pub(crate) struct ReadbackSlot {
    pub(crate) buffer: wgpu::Buffer,
    /// The copy in flight: its submission and the result of mapping the buffer.
    pub(crate) pending: Option<(wgpu::SubmissionIndex, std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>)>,
}

impl Readback {
    /// `depth` frames can be in flight at once, as RGBA.
    pub fn new(device: &wgpu::Device, width: u32, height: u32, depth: usize) -> Self {
        Self::with_layout(device, width, height, depth, yuv::PixelLayout::Rgba)
    }

    /// `depth` frames can be in flight at once, given back as `layout` (RGBA when the size doesn't
    /// fit it: see [`yuv::PixelLayout::fits`]).
    pub fn with_layout(device: &wgpu::Device, width: u32, height: u32, depth: usize, layout: yuv::PixelLayout) -> Self {
        let layout = if layout.fits(width, height) { layout } else { yuv::PixelLayout::Rgba };
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let mut planes = Vec::new();
        let mut size = 0u64;
        for (w, h, bytes) in layout.planes(width, height) {
            let row = (w * bytes) as usize;
            let padded = row.next_multiple_of(align);
            planes.push((size, padded as u32, row as u32, h));
            size = (size + (padded * h as usize) as u64).next_multiple_of(align as u64);
        }
        let slots = (0..depth.max(1))
            .map(|_| ReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: size.max(4),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: None,
            })
            .collect();
        let video = (layout != yuv::PixelLayout::Rgba).then(|| yuv::VideoPlanes::new(device, layout, width, height));
        Self { slots, next: 0, width, height, layout, planes, video }
    }

    /// How the frames come back.
    pub fn layout(&self) -> yuv::PixelLayout {
        self.layout
    }

    /// Starts copying the renderer's current output. If all slots are busy, the oldest frame is
    /// finished first and returned.
    pub fn push(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &Renderer,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        anyhow::ensure!(renderer.size() == (self.width, self.height), "readback size doesn't match the renderer");
        let index = self.next;
        let oldest = self.take(device, index)?;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        let sources: Vec<&wgpu::Texture> = match self.video.as_mut() {
            Some(video) => {
                video.convert(device, &mut encoder, renderer.output_view());
                video.textures.iter().collect()
            }
            None => vec![renderer.output_texture()],
        };
        let slot = &mut self.slots[index];
        for (texture, &(offset, padded, _, h)) in sources.into_iter().zip(&self.planes) {
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &slot.buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d { width: texture.width(), height: h, depth_or_array_layers: 1 },
            );
        }
        let submission = queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        slot.pending = Some((submission, receiver));
        self.next = (index + 1) % self.slots.len();
        Ok(oldest)
    }

    /// Waits for every frame still in flight and returns them, oldest first.
    pub fn finish(&mut self, device: &wgpu::Device) -> anyhow::Result<Vec<Vec<u8>>> {
        let mut frames = Vec::new();
        for k in 0..self.slots.len() {
            let index = (self.next + k) % self.slots.len();
            frames.extend(self.take(device, index)?);
        }
        Ok(frames)
    }

    pub(crate) fn take(&mut self, device: &wgpu::Device, index: usize) -> anyhow::Result<Option<Vec<u8>>> {
        let slot = &mut self.slots[index];
        let Some((submission, receiver)) = slot.pending.take() else { return Ok(None) };
        device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: None })?;
        receiver.recv()??;
        let total: usize = self.planes.iter().map(|&(_, _, row, h)| row as usize * h as usize).sum();
        let mut pixels = Vec::with_capacity(total);
        {
            let mapped = slot.buffer.slice(..).get_mapped_range()?;
            for &(offset, padded, row, h) in &self.planes {
                let plane = &mapped[offset as usize..offset as usize + padded as usize * h as usize];
                for chunk in plane.chunks(padded as usize) {
                    pixels.extend_from_slice(&chunk[..row as usize]);
                }
            }
        }
        slot.buffer.unmap();
        Ok(Some(pixels))
    }
}
