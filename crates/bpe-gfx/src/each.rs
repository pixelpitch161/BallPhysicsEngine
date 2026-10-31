//! Things that blend with each other: a blend layer whose `each` is set starts as its mode's
//! identity, and every instance is mixed into it by the mode with a fixed-function blend, so
//! overlapping things meet by the mode too (see each.wgsl). The pipelines for that differ from
//! the usual ones only in their blend state and fragment entry point, and are made the first
//! time a layer needs them.

use std::collections::BTreeMap;

use bpe_core::Blend;
use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op};

use crate::{Frame, HDR_FORMAT};

/// The kinds of instance a layer that blends with each other draws with their own pipelines.
/// Liquids and particles keep their usual ones: no look that blends with each other draws them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Trail,
    Shape,
    Soft,
    Ball,
    Look,
    Flash,
    Glyph,
}

impl Kind {
    const ALL: [Kind; 7] = [Kind::Trail, Kind::Shape, Kind::Soft, Kind::Ball, Kind::Look, Kind::Flash, Kind::Glyph];

    fn used_by(self, f: &Frame) -> bool {
        match self {
            Kind::Trail => !f.trails.is_empty(),
            Kind::Shape => !f.shapes.is_empty(),
            Kind::Soft => !f.soft.is_empty(),
            Kind::Ball => !f.balls.is_empty(),
            Kind::Look => !f.looks.is_empty(),
            Kind::Flash => !f.flashes.is_empty(),
            Kind::Glyph => !f.glyphs.is_empty(),
        }
    }
}

/// The blend a layer of `blend` mixes each of its things in with, for a mode that
/// [combines with itself](Blend::combines_with_itself). Alpha keeps the largest coverage.
pub(crate) fn blend_state(blend: Blend) -> wgpu::BlendState {
    let component = |src_factor, dst_factor, operation| BlendComponent { src_factor, dst_factor, operation };
    let color = match blend {
        Blend::Screen => component(F::One, F::OneMinusSrc, Op::Add),
        Blend::Multiply => component(F::Dst, F::Zero, Op::Add),
        Blend::Lighten => component(F::One, F::One, Op::Max),
        Blend::Darken => component(F::One, F::One, Op::Min),
        // Add, and Subtract, which gathers the light it takes away.
        _ => component(F::One, F::One, Op::Add),
    };
    wgpu::BlendState { color, alpha: component(F::One, F::One, Op::Max) }
}

/// What a layer of `blend` that blends with each other starts as: the mode's identity, with
/// nothing covered. Matches `EACH_FAR` in each.wgsl for Darken.
pub(crate) fn clear(blend: Blend) -> wgpu::Color {
    let v = match blend {
        Blend::Multiply => 1.0,
        Blend::Darken => 60000.0,
        _ => 0.0,
    };
    wgpu::Color { r: v, g: v, b: v, a: 0.0 }
}

/// How one kind of instance is drawn: its shader, pipeline layout, vertex step and buffer
/// layout. Filled by the renderer with the same values as its usual pipelines.
pub(crate) struct Recipe {
    pub module: wgpu::ShaderModule,
    pub layout: wgpu::PipelineLayout,
    pub vs: &'static str,
    pub fs: &'static str,
    pub stride: u64,
    pub step: wgpu::VertexStepMode,
    pub attributes: &'static [wgpu::VertexAttribute],
    pub topology: wgpu::PrimitiveTopology,
}

/// The pipelines for things that blend with each other, by kind and mode, made on first use.
pub(crate) struct EachPipelines {
    recipes: BTreeMap<Kind, Recipe>,
    made: BTreeMap<(Kind, u32), wgpu::RenderPipeline>,
}

impl EachPipelines {
    pub(crate) fn new(recipes: impl IntoIterator<Item = (Kind, Recipe)>) -> Self {
        Self { recipes: recipes.into_iter().collect(), made: BTreeMap::new() }
    }

    /// Makes the pipelines `f`'s lists need to blend with each other by `blend`.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, blend: Blend, f: &Frame) {
        for kind in Kind::ALL.into_iter().filter(|k| k.used_by(f)) {
            let key = (kind, blend.shader_code());
            if self.made.contains_key(&key) {
                continue;
            }
            let r = &self.recipes[&kind];
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blend with each other"),
                layout: Some(&r.layout),
                vertex: wgpu::VertexState {
                    module: &r.module,
                    entry_point: Some(r.vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: r.stride,
                        step_mode: r.step,
                        attributes: r.attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState { topology: r.topology, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &r.module,
                    entry_point: Some(r.fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: Some(blend_state(blend)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.made.insert(key, pipeline);
        }
    }

    /// The pipeline [`EachPipelines::ensure`] made for `kind` in a layer of mode `code`.
    pub(crate) fn get(&self, kind: Kind, code: u32) -> &wgpu::RenderPipeline {
        &self.made[&(kind, code)]
    }
}

/// The globals a layer that blends with each other is drawn with, by mode and by whether it is
/// stuck to the screen: the frame's, with `post.w` the mode (each.wgsl reads it). Each is its
/// own buffer, as every layer is recorded before the frame is sent.
#[derive(Default)]
pub(crate) struct EachGlobals {
    made: BTreeMap<(u32, bool), (wgpu::Buffer, wgpu::BindGroup)>,
}

impl EachGlobals {
    pub(crate) fn group(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        key: (u32, bool),
        bytes: &[u8],
    ) -> wgpu::BindGroup {
        let (buffer, group) = self.made.entry(key).or_insert_with(|| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("blend with each other globals"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("blend with each other globals"),
                layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            (buffer, group)
        });
        queue.write_buffer(buffer, 0, bytes);
        group.clone()
    }
}
