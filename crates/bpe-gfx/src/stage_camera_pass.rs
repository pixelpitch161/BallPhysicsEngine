//! The Stage camera pass (lighting part 5, plan decisions 4 to 10): after the light passes, the
//! lit front picture is seen again through the moved camera ([`StageView`]). The background is
//! drawn alone (by the renderer, into [`StageCameraPass::background`]) to tell the bare wall from
//! the things over it; the bare wall is filled in under the things by a pull–push; then a ray per
//! pixel finds the ball, shape, wall or floor it meets and the flat things it crosses on the
//! picture plane, and pixels on an edge between two things look again with four more rays. The
//! object picture, when effects read it, is seen again too. Nothing is kept between frames.

use crate::lighting::Lighting;

/// The Stage camera's pipelines and layouts, built once with the lighting's (they read its bind
/// groups 0 and 2) and shared by every renderer on the device.
#[derive(Clone)]
pub(crate) struct StageKit {
    layout: wgpu::BindGroupLayout,
    objects_layout: wgpu::BindGroupLayout,
    empty: wgpu::BindGroup,
    pub(crate) seed: wgpu::RenderPipeline,
    pull: wgpu::RenderPipeline,
    push: wgpu::RenderPipeline,
    view: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    objects: wgpu::RenderPipeline,
}
use crate::{HDR_FORMAT, OBJECT_FORMAT, StageView};

type Picture = (wgpu::Texture, wgpu::TextureView);

/// Where each pixel read the front picture: two full floats (the four targets of the first look
/// must fit in 32 bytes a pixel).
const UV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;

struct Pictures {
    size: (u32, u32),
    background: Picture,
    /// The bare wall, level 0 the full picture, each level half the one before, down to 1 × 1.
    pull: Vec<Picture>,
    /// The same levels filled in, the coarsest being `pull`'s.
    push: Vec<Picture>,
    /// The first look: its colour and what it hit.
    first: Picture,
    /// Where each pixel read the front picture (two full floats, for the object picture).
    uv: Picture,
    /// What the camera sees: the picture, the picture before lighting, the lit colour.
    seen: [Picture; 3],
    /// The object picture seen again, made the first time a moved frame's effects read it.
    objects: Option<Picture>,
}

pub(crate) struct StageCameraPass {
    layout: wgpu::BindGroupLayout,
    objects_layout: wgpu::BindGroupLayout,
    /// Group 1 left empty: the light pass's glow pictures aren't read here, and binding them
    /// too would pass the device's count of pictures a shader may read.
    empty: wgpu::BindGroup,
    seed: wgpu::RenderPipeline,
    pull: wgpu::RenderPipeline,
    push: wgpu::RenderPipeline,
    view: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    objects: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    black: wgpu::TextureView,
    pictures: Option<Pictures>,
}

/// The sizes of the pull–push levels of a `w` × `h` picture: halving (rounding up) to 1 × 1.
pub(crate) fn levels(w: u32, h: u32) -> Vec<(u32, u32)> {
    let mut out = vec![(w.max(1), h.max(1))];
    while let Some(&(a, b)) = out.last() {
        if a == 1 && b == 1 {
            break;
        }
        out.push((a.div_ceil(2), b.div_ceil(2)));
    }
    out
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl StageKit {
    /// Compiles the Stage camera's shader against the lighting's layouts `l0` and `l2`.
    pub(crate) fn build(device: &wgpu::Device, l0: &wgpu::BindGroupLayout, l2: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage camera"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, true),
                texture_entry(2, true),
                texture_entry(3, true),
                texture_entry(4, true),
                texture_entry(5, true),
            ],
        });
        let objects_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stage camera objects"),
            entries: &[texture_entry(6, false), texture_entry(7, false)],
        });
        let empty_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("nothing"), entries: &[] });
        let empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nothing"),
            layout: &empty_layout,
            entries: &[],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage camera"),
            bind_group_layouts: &[Some(l0), Some(&empty_layout), Some(l2), Some(&layout)],
            immediate_size: 0,
        });
        let objects_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stage camera objects"),
            bind_group_layouts: &[Some(&empty_layout), Some(&empty_layout), Some(&empty_layout), Some(&objects_layout)],
            immediate_size: 0,
        });
        let source = format!("{}\n{}", crate::LIGHTING_WGSL, include_str!("shaders/stage_camera.wgsl"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stage camera"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let target = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let pipeline = |entry: &str, layout: &wgpu::PipelineLayout, formats: &[wgpu::TextureFormat]| {
            let targets: Vec<_> = formats.iter().map(|f| target(*f)).collect();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pl = &pipeline_layout;
        Self {
            seed: pipeline("fs_cam_seed", pl, &[HDR_FORMAT]),
            pull: pipeline("fs_cam_pull", pl, &[HDR_FORMAT]),
            push: pipeline("fs_cam_push", pl, &[HDR_FORMAT]),
            view: pipeline("fs_cam_view", pl, &[HDR_FORMAT, HDR_FORMAT, HDR_FORMAT, UV_FORMAT]),
            edges: pipeline("fs_cam_edges", pl, &[HDR_FORMAT]),
            objects: pipeline("fs_cam_objects", &objects_pipeline_layout, &[OBJECT_FORMAT]),
            layout,
            objects_layout,
            empty,
        }
    }
}

impl StageCameraPass {
    /// A Stage camera pass drawing with `kit`'s pipelines, with its own pictures.
    pub(crate) fn with_kit(device: &wgpu::Device, kit: &StageKit) -> Self {
        let StageKit { layout, objects_layout, empty, seed, pull, push, view, edges, objects } = kit.clone();
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stage camera"),
            size: std::mem::size_of::<crate::StageCameraGpu>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            seed,
            pull,
            push,
            view,
            edges,
            objects,
            layout,
            objects_layout,
            empty,
            uniform,
            black: crate::effects::chain::make_texture(device, 1, 1, HDR_FORMAT, "stage camera black").1,
            pictures: None,
        }
    }

    /// Makes the pictures for a `width` × `height` output, unless they are there.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.pictures.as_ref().is_some_and(|p| p.size == (width, height)) {
            return;
        }
        let make = |(w, h): (u32, u32), label| crate::effects::chain::make_texture(device, w, h, HDR_FORMAT, label);
        let sizes = levels(width, height);
        let full = (width, height);
        self.pictures = Some(Pictures {
            size: full,
            background: make(full, "background alone"),
            pull: sizes.iter().map(|&s| make(s, "stage wall")).collect(),
            push: sizes.iter().map(|&s| make(s, "stage wall filled")).collect(),
            first: make(full, "stage camera first look"),
            uv: crate::effects::chain::make_texture(device, width, height, UV_FORMAT, "stage camera uv"),
            seen: [
                make(full, "stage camera picture"),
                make(full, "stage camera unlit"),
                make(full, "stage camera lit colour"),
            ],
            objects: None,
        });
    }

    /// Where the renderer draws the background alone. Made by [`Self::ensure`].
    pub(crate) fn background(&self) -> &wgpu::TextureView {
        &self.pictures.as_ref().expect("ensured").background.1
    }

    /// What the camera saw: the picture, the picture before lighting, the lit colour.
    pub(crate) fn seen(&self) -> &[Picture; 3] {
        &self.pictures.as_ref().expect("ensured").seen
    }

    /// Sees the front object picture `front` through the camera of the last [`Self::run`], and
    /// gives back the picture it made.
    pub(crate) fn see_objects(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        front: &wgpu::TextureView,
    ) -> Option<wgpu::TextureView> {
        let p = self.pictures.as_mut()?;
        let (w, h) = p.size;
        let out = &p
            .objects
            .get_or_insert_with(|| {
                crate::effects::chain::make_texture(device, w, h, OBJECT_FORMAT, "stage camera objects")
            })
            .1;
        let g = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stage camera objects"),
            layout: &self.objects_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&p.uv.1) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(front) },
            ],
        });
        let mut pass =
            crate::begin_pass(encoder, out, "stage camera objects", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        pass.set_pipeline(&self.objects);
        for i in 0..3 {
            pass.set_bind_group(i, &self.empty, &[]);
        }
        pass.set_bind_group(3, &g, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        Some(out.clone())
    }

    /// Sees `lit` (the lit front picture) through `view`; `lit_colour` is the light pass's lit
    /// colour picture. The background alone must be drawn first.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        lighting: &Lighting,
        view: &StageView,
        samples: u32,
        lit: &wgpu::TextureView,
        lit_colour: &wgpu::TextureView,
    ) {
        let Some(bound) = lighting.bound() else { return };
        let Some(p) = self.pictures.as_ref() else { return };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&view.gpu(samples, p.size)));
        let black = &self.black;
        let group = |views: [&wgpu::TextureView; 5]| {
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() }];
            entries.extend(views.into_iter().enumerate().map(|(i, v)| wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: wgpu::BindingResource::TextureView(v),
            }));
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("stage camera"),
                layout: &self.layout,
                entries: &entries,
            })
        };
        let draw = |encoder: &mut wgpu::CommandEncoder,
                    targets: &[&wgpu::TextureView],
                    pipeline: &wgpu::RenderPipeline,
                    groups: [&wgpu::BindGroup; 4],
                    label: &str| {
            let attachments: Vec<_> = targets
                .iter()
                .map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            for (i, g) in groups.into_iter().enumerate() {
                pass.set_bind_group(i as u32, g, &[]);
            }
            pass.draw(0..3, 0..1);
        };
        // The bare wall, then coarser and coarser, then filled in from the coarsest back up.
        let g = group([lit, &p.background.1, black, black, black]);
        draw(encoder, &[&p.pull[0].1], &self.seed, [&bound[0], &self.empty, &bound[1], &g], "stage wall");
        for i in 1..p.pull.len() {
            let g = group([&p.pull[i - 1].1, black, black, black, black]);
            draw(encoder, &[&p.pull[i].1], &self.pull, [&bound[0], &self.empty, &bound[1], &g], "stage wall coarser");
        }
        let last = p.pull.len() - 1;
        for i in (0..last).rev() {
            let coarse = if i + 1 == last { &p.pull[last].1 } else { &p.push[i + 1].1 };
            let g = group([&p.pull[i].1, black, coarse, black, black]);
            draw(encoder, &[&p.push[i].1], &self.push, [&bound[0], &self.empty, &bound[1], &g], "stage wall filled");
        }
        let wall = if last == 0 { &p.pull[0].1 } else { &p.push[0].1 };
        let g = group([lit, &p.background.1, wall, lit_colour, black]);
        let targets = [&p.first.1, &p.seen[1].1, &p.seen[2].1, &p.uv.1];
        draw(encoder, &targets, &self.view, [&bound[0], &self.empty, &bound[1], &g], "stage camera");
        let g = group([lit, &p.background.1, wall, lit_colour, &p.first.1]);
        draw(encoder, &[&p.seen[0].1], &self.edges, [&bound[0], &self.empty, &bound[1], &g], "stage camera edges");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_down_to_one_texel() {
        assert_eq!(levels(5, 3), vec![(5, 3), (3, 2), (2, 1), (1, 1)]);
        assert_eq!(levels(1, 1), vec![(1, 1)]);
        assert_eq!(levels(1080, 1920).len(), 12);
    }
}
