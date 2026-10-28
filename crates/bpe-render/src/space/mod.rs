//! The 3D picture: lit balls inside spheres, cubes and tunnels. When a scene is 3D this replaces
//! the flat background, walls, trails, balls and flashes; effects and bloom still run after it.
//! Owned by the 3D workstream, which fills this in.

use crate::FrameData;
use bpe_scene::Scene;
use bpe_sim::Playback;

/// What the 3D renderer needs from the CPU side for one frame.
#[derive(Clone, Debug, Default)]
pub struct SpaceFrame {}

impl SpaceFrame {
    pub fn fill(&mut self, _scene: &Scene, _playback: &Playback) {}
}

pub struct SpaceRenderer {}

impl SpaceRenderer {
    pub fn new(_device: &wgpu::Device, _hdr_format: wgpu::TextureFormat) -> Self {
        Self {}
    }

    /// Whether this scene is drawn in 3D.
    pub fn active(&self, _scene: &Scene) -> bool {
        false
    }

    /// Draws the whole 3D scene into the HDR target, clearing it first.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _hdr: &wgpu::TextureView,
        _scene: &Scene,
        _frame: &FrameData,
        _time: f64,
    ) {
    }
}
