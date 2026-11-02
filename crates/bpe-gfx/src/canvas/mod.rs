//! Canvases: rectangles of the world that marks are painted on. Each canvas keeps three textures
//! (filled by the flow layers, which replay its marks) and is drawn into the picture at its place
//! by the composite.

pub(crate) mod composite;
pub mod pictures;
pub(crate) mod stamp;

/// The most texels along a canvas texture's long side.
pub const MAX_TEXELS: u32 = 4096;

/// A canvas's three textures, `texels` in size.
pub struct CanvasTextures {
    /// Rgba16Float: the marks' colour times their amount, and the amount.
    pub paint: wgpu::Texture,
    /// Rgba16Float: r is the scene time the texel was last painted.
    pub aux: wgpu::Texture,
    /// R16Float: how thick the paint is (ink / 255, added up).
    pub body: wgpu::Texture,
    /// Views of the three, made once: marks are drawn into them and the composite samples them.
    pub paint_view: wgpu::TextureView,
    pub aux_view: wgpu::TextureView,
    pub body_view: wgpu::TextureView,
}

impl CanvasTextures {
    /// Three cleared textures of `texels` (each side at least 1, the long side at most
    /// [`MAX_TEXELS`]), which can be drawn into, sampled and copied.
    pub fn new(device: &wgpu::Device, texels: [u32; 2]) -> Self {
        let [w, h] = clamp_texels(texels);
        let make = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let rgba = wgpu::TextureFormat::Rgba16Float;
        let (paint, aux) = (make("canvas paint", rgba), make("canvas aux", rgba));
        let body = make("canvas body", wgpu::TextureFormat::R16Float);
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (paint_view, aux_view, body_view) = (view(&paint), view(&aux), view(&body));
        Self { paint, aux, body, paint_view, aux_view, body_view }
    }
}

/// The bytes a texture holds.
pub(crate) fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    texture.width() as u64 * texture.height() as u64 * texture.format().block_copy_size(None).unwrap_or(8) as u64
}

/// `texels` with each side at least 1 and both scaled down together so the long side is at most
/// [`MAX_TEXELS`].
pub fn clamp_texels([w, h]: [u32; 2]) -> [u32; 2] {
    let long = w.max(h);
    if long <= MAX_TEXELS {
        return [w.max(1), h.max(1)];
    }
    let scale = |side: u32| ((side as u64 * MAX_TEXELS as u64) / long as u64).max(1) as u32;
    [scale(w), scale(h)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texels_keep_their_shape_under_the_cap() {
        assert_eq!(clamp_texels([1080, 1920]), [1080, 1920]);
        assert_eq!(clamp_texels([0, 5]), [1, 5]);
        assert_eq!(clamp_texels([8192, 2048]), [4096, 1024]);
        assert_eq!(clamp_texels([2, 9000]), [1, 4096]);
    }
}
