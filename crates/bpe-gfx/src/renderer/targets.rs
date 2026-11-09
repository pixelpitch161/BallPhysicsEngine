//! The renderer's pictures: the HDR scene, bloom levels, the object picture and layer pictures.

use super::*;

/// Size-dependent GPU resources, rebuilt when the output size changes.
pub(crate) struct Targets {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) hdr_texture: wgpu::Texture,
    pub(crate) hdr: wgpu::TextureView,
    /// A copy of the HDR image under a blend layer, for the composite to read.
    pub(crate) behind: wgpu::Texture,
    pub(crate) behind_view: wgpu::TextureView,
    /// What the things of one blend layer draw into, before it is mixed into the HDR image;
    /// made the first time a frame has a layer.
    pub(crate) layer: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The object picture, made the first time an effect needs it.
    pub(crate) object: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub(crate) bloom: Vec<wgpu::TextureView>,
    pub(crate) output: wgpu::Texture,
    pub(crate) output_view: wgpu::TextureView,
    /// Bind groups reading the HDR image, then each bloom level, for the down and up passes.
    pub(crate) down_sources: Vec<wgpu::BindGroup>,
    pub(crate) up_sources: Vec<wgpu::BindGroup>,
    pub(crate) composite: wgpu::BindGroup,
}

impl Targets {
    pub(crate) fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        post_layout: &wgpu::BindGroupLayout,
        composite_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let texture = |label, w: u32, h: u32, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let render_and_sample = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        // Effects read the HDR picture while they draw their result beside it, then copy it back;
        // a blend layer copies it too, to mix with what is under it.
        let copyable = render_and_sample | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        let hdr_texture = texture("hdr", width, height, HDR_FORMAT, copyable);
        let hdr = hdr_texture.create_view(&Default::default());
        let behind = texture(
            "behind",
            width,
            height,
            HDR_FORMAT,
            // Drawn into by a bake's average (lighting part 3).
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let behind_view = behind.create_view(&Default::default());
        let bloom: Vec<_> = (0..BLOOM_LEVELS)
            .map(|level| {
                let shift = level as u32 + 1;
                texture("bloom", width >> shift, height >> shift, HDR_FORMAT, render_and_sample)
                    .create_view(&Default::default())
            })
            .collect();
        let output = texture("output", width, height, OUTPUT_FORMAT, render_and_sample | wgpu::TextureUsages::COPY_SRC);
        let output_view = output.create_view(&Default::default());

        let source = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post source"),
                layout: post_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        let down_sources = std::iter::once(&hdr).chain(&bloom[..BLOOM_LEVELS - 1]).map(source).collect();
        let up_sources = bloom[1..].iter().map(source).collect();
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&bloom[0]) },
            ],
        });
        Self {
            width,
            height,
            hdr_texture,
            hdr,
            behind,
            behind_view,
            layer: None,
            object: None,
            bloom,
            output,
            output_view,
            down_sources,
            up_sources,
            composite,
        }
    }

    /// Makes the blend layer picture if it is missing. Effects on layered things run over it in
    /// place, so it copies like the HDR picture.
    pub(crate) fn ensure_layer(&mut self, device: &wgpu::Device) {
        if self.layer.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("blend layer"),
                size: wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.layer = Some((texture, view));
        }
    }

    /// Makes the object picture if it is missing.
    pub(crate) fn ensure_object(&mut self, device: &wgpu::Device) {
        if self.object.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("objects"),
                size: wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OBJECT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.object = Some((texture, view));
        }
    }
}
