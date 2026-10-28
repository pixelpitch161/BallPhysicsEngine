//! Captions, counters and timers, drawn on the finished picture after bloom. Owned by the
//! captions workstream, which fills this in.

use crate::FrameData;
use bpe_scene::Scene;
use bpe_sim::{Counts, Playback};

/// The captions to draw this frame, with their text filled in.
#[derive(Clone, Debug, Default)]
pub struct TextFrame {}

impl TextFrame {
    pub fn fill(&mut self, _scene: &Scene, _playback: &Playback, _counts: &Counts) {}
}

pub struct Captions {}

impl Captions {
    pub fn new(_device: &wgpu::Device, _output_format: wgpu::TextureFormat) -> Self {
        Self {}
    }

    pub fn prepare(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _scene: &Scene, _frame: &FrameData) {}

    /// Whether there is anything to draw, so the pass can be skipped.
    pub fn visible(&self, _frame: &FrameData) -> bool {
        false
    }

    /// Draws onto the output picture.
    pub fn draw(&self, _pass: &mut wgpu::RenderPass) {}
}
