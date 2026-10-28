//! Swarms of up to a million points and big fluids, simulated on the graphics card. They only
//! look at the CPU world and never make sound. Owned by the GPU workstream, which fills this in.

use crate::FrameData;
use bpe_scene::Scene;
use bpe_sim::Playback;

/// What the GPU layers need from the CPU side for one frame.
#[derive(Clone, Debug, Default)]
pub struct GpuFrame {}

impl GpuFrame {
    pub fn fill(&mut self, _scene: &Scene, _playback: &Playback) {}
}

/// GPU resources for swarms and fluids.
pub struct GpuLayers {}

impl GpuLayers {
    pub fn new(_device: &wgpu::Device, _hdr_format: wgpu::TextureFormat) -> Self {
        Self {}
    }

    /// Runs the compute passes for this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _scene: &Scene,
        _frame: &FrameData,
        _time: f64,
        _size: (u32, u32),
    ) {
    }

    /// Whether the scene pass has to stop after the background and walls so that
    /// [`GpuLayers::draw_between`] can draw a fluid surface reading what is drawn so far.
    pub fn wants_split_pass(&self, _scene: &Scene) -> bool {
        false
    }

    /// Passes of its own between the two halves of a split scene pass.
    pub fn draw_between(
        &mut self,
        _device: &wgpu::Device,
        _encoder: &mut wgpu::CommandEncoder,
        _hdr_texture: &wgpu::Texture,
        _hdr: &wgpu::TextureView,
    ) {
    }

    /// Draws swarm points in the scene pass, before trails.
    pub fn draw(&self, _pass: &mut wgpu::RenderPass, _globals: &wgpu::BindGroup) {}
}
