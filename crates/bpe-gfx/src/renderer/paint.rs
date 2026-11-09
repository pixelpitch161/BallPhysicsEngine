//! The paint texture: gradients, masks and ripples as the shaders read them.

use super::*;

/// The gradient atlas on the GPU, remade only when the frame has more gradients than it has rows,
/// bound with the Ripples field (rebound when either changes).
pub(crate) struct PaintTexture {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    /// Each row's Hit wave, two texels a row (`gradients::wave_texels`).
    pub(crate) waves: wgpu::Texture,
    pub(crate) waves_view: wgpu::TextureView,
    pub(crate) rows: usize,
    pub(crate) group: wgpu::BindGroup,
    /// The Ripples generation `group` binds (see [`ripples::Ripples::generation`]).
    pub(crate) bound: u64,
    /// What each row of the texture holds ([`GradientAtlas::keys`]): a row is uploaded again only
    /// when the frame's atlas holds something else there (review S21).
    pub(crate) uploaded: Vec<u64>,
}

impl PaintTexture {
    pub(crate) fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        rows: usize,
        ripples: &ripples::Ripples,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gradient atlas"),
            size: wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: rows as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let waves = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gradient waves"),
            size: wgpu::Extent3d { width: 2, height: rows as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let waves_view = waves.create_view(&Default::default());
        let group = Self::group(device, layout, sampler, &view, &waves_view, ripples);
        Self { texture, view, waves, waves_view, rows, group, bound: ripples.generation(), uploaded: Vec::new() }
    }

    pub(crate) fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        view: &wgpu::TextureView,
        waves: &wgpu::TextureView,
        ripples: &ripples::Ripples,
    ) -> wgpu::BindGroup {
        let [a, b, c] = ripples.entries();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gradient atlas"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(waves) },
                a,
                b,
                c,
            ],
        })
    }

    /// Binds the Ripples field again when it changed since the group was made.
    pub(crate) fn bind_ripples(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        ripples: &ripples::Ripples,
    ) {
        if self.bound != ripples.generation() {
            self.group = Self::group(device, layout, sampler, &self.view, &self.waves_view, ripples);
            self.bound = ripples.generation();
        }
    }

    /// Uploads `atlas`, making the texture taller first if its gradients do not fit.
    pub(crate) fn sync(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        atlas: &GradientAtlas,
        ripples: &ripples::Ripples,
    ) {
        let max = device.limits().max_texture_dimension_2d as usize;
        if atlas.len() > max {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!("{} gradients in one frame; past {max} they reuse the last one", atlas.len());
            });
        }
        let rows = gradients::uploaded_rows(atlas.len(), max);
        if rows > self.rows {
            *self = Self::new(device, layout, sampler, gradients::atlas_height(rows, max), ripples);
        }
        if rows == 0 {
            return;
        }
        let keys = &atlas.keys()[..rows];
        let row_bytes = (ATLAS_WIDTH * 4 * 2) as u32;
        // Each run of rows that changed, uploaded on its own.
        let mut r = 0;
        while r < rows {
            if self.uploaded.get(r) == Some(&keys[r]) {
                r += 1;
                continue;
            }
            let start = r;
            while r < rows && self.uploaded.get(r) != Some(&keys[r]) {
                r += 1;
            }
            fn origin(texture: &wgpu::Texture, row: usize) -> wgpu::TexelCopyTextureInfo<'_> {
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: row as u32, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                }
            }
            let n = (r - start) as u32;
            queue.write_texture(
                origin(&self.texture, start),
                &gradients::half_texels(&atlas.rows()[start..r]),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: None },
                wgpu::Extent3d { width: ATLAS_WIDTH as u32, height: n, depth_or_array_layers: 1 },
            );
            queue.write_texture(
                origin(&self.waves, start),
                bytemuck::cast_slice(&atlas.waves()[start..r]),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(2 * 16), rows_per_image: None },
                wgpu::Extent3d { width: 2, height: n, depth_or_array_layers: 1 },
            );
        }
        if self.uploaded.len() < rows {
            self.uploaded.resize(rows, 0);
        }
        self.uploaded[..rows].copy_from_slice(keys);
    }
}
