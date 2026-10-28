//! Hit particles: sparks, smoke, confetti, drawn after balls. Owned by the GPU workstream,
//! which fills this in.

use crate::FrameData;
use crate::gpu::GpuLayers;
use bpe_scene::Scene;
use bpe_sim::{Event, Playback};

/// What the particles need from the CPU side for one frame.
#[derive(Clone, Debug, Default)]
pub struct ParticleFrame {}

impl ParticleFrame {
    /// `events` is [`FrameData::events`], already filled.
    pub fn fill(&mut self, _scene: &Scene, _playback: &Playback, _events: &[Event]) {}
}

pub struct Particles {}

impl Particles {
    pub fn new(_device: &wgpu::Device, _hdr_format: wgpu::TextureFormat) -> Self {
        Self {}
    }

    pub fn prepare(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _scene: &Scene,
        _frame: &FrameData,
        _gpu: &GpuLayers,
    ) {
    }

    /// Draws in the scene pass, after balls and flashes.
    pub fn draw(&self, _pass: &mut wgpu::RenderPass, _globals: &wgpu::BindGroup) {}
}
