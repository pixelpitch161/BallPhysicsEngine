//! The grade's lookup texture.

use super::*;

/// Samples in the grade map's row (post.wgsl reads 256).
pub(crate) const GRADE_WIDTH: usize = 256;

/// The colour grade's gradient map, baked into one 256×1 row the composite reads. It is always
/// there, and written only when a frame maps by an amount above 0 with a gradient it does not
/// hold yet.
pub(crate) struct GradeMap {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    /// The gradient baked into the texture, once one has been.
    pub(crate) baked: Option<bpe_core::Gradient>,
}

impl GradeMap {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grade map"),
            size: wgpu::Extent3d { width: GRADE_WIDTH as u32, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view, baked: None }
    }

    /// Bakes `grade`'s map into the texture when it is used and not there already.
    pub(crate) fn sync(&mut self, queue: &wgpu::Queue, grade: &Grade) {
        if grade.map_amount <= 0.0 || self.baked.as_ref() == Some(&grade.map) {
            return;
        }
        let mut row = [[0.0; 4]; GRADE_WIDTH];
        row.copy_from_slice(&grade.map.bake(GRADE_WIDTH));
        queue.write_texture(
            self.texture.as_image_copy(),
            &gradients::half_texels(&[row]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(GRADE_WIDTH as u32 * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: GRADE_WIDTH as u32, height: 1, depth_or_array_layers: 1 },
        );
        self.baked = Some(grade.map.clone());
    }
}
