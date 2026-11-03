//! Shader layers inside balls: a stack with Shader layers draws with a pipeline built from the
//! scene shader plus each distinct effect it runs (see `effects::in_ball_effects`), one program
//! per distinct list of effect sources. Programs are built the first time a frame names them and
//! kept; one that doesn't build draws its balls with the plain pipeline (which leaves shader
//! layers out) and keeps its error for the editor.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use crate::{Frame, HDR_FORMAT, LAYERED_ATTRIBUTES, LayeredInstance};

/// The most programs kept; past it, the ones the last frame didn't use go.
const KEEP_PROGRAMS: usize = 64;

/// The effects one in-ball program runs, by slot: `sources[i]` is effect slot `i` of its shader
/// records. `key` tells programs apart: the same sources always give the same key.
#[derive(Clone, Debug, PartialEq)]
pub struct LookProgram {
    pub key: u64,
    pub sources: Vec<Cow<'static, str>>,
}

impl LookProgram {
    /// A program running `sources`, keyed by them.
    pub fn new(sources: Vec<Cow<'static, str>>) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for s in &sources {
            s.as_ref().hash(&mut h);
        }
        Self { key: h.finish(), sources }
    }
}

/// Layered instances `start..end` of a frame, drawn with program `program` (an index into the
/// frame's `programs`) or, for `None`, the plain pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayeredRun {
    pub program: Option<u32>,
    pub start: u32,
    pub end: u32,
}

/// One built program: its module and pipelines, normal and for each blend things blend with
/// each other by.
struct Made {
    module: wgpu::ShaderModule,
    normal: wgpu::RenderPipeline,
    each: BTreeMap<u32, wgpu::RenderPipeline>,
}

#[derive(Default)]
pub(crate) struct Programs {
    made: HashMap<u64, Result<Made, String>>,
    /// The keys the last frame named.
    used: Vec<u64>,
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    fs: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("layered balls with shader layers"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_layers"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<LayeredInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &LAYERED_ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Builds a program, or says why it can't be.
fn build(device: &wgpu::Device, layout: &wgpu::PipelineLayout, program: &LookProgram) -> Result<Made, String> {
    let sources: Vec<&str> = program.sources.iter().map(|s| s.as_ref()).collect();
    let effects = crate::effects::in_ball_effects(&sources)?;
    let full = crate::scene_source(Some(&effects));
    // Checked first, so a mistake in own code reads as a line about the code.
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| first_line(&e.emit_to_string(&full)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| first_line(&format!("{}", e.as_inner())))?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layered balls with shader layers"),
        source: wgpu::ShaderSource::Wgsl(full.into()),
    });
    let normal = pipeline(device, layout, &module, "fs_layers", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
    match pollster::block_on(scope.pop()) {
        None => Ok(Made { module, normal, each: BTreeMap::new() }),
        Some(error) => Err(first_line(&error.to_string())),
    }
}

fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("the GPU refused the shader").to_owned()
}

impl Programs {
    /// Builds every program `frames` name that isn't built yet, with the pipelines for blending
    /// with each other by `each` where a frame needs them.
    pub(crate) fn ensure<'a>(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        frames: impl IntoIterator<Item = (&'a Frame, Option<bpe_core::Blend>)>,
    ) {
        let mut used = Vec::new();
        for (frame, each) in frames {
            for program in &frame.programs {
                used.push(program.key);
                let made = self.made.entry(program.key).or_insert_with(|| build(device, layout, program));
                if let (Ok(made), Some(blend)) = (made, each) {
                    let code = blend.shader_code();
                    if !made.each.contains_key(&code) {
                        let p =
                            pipeline(device, layout, &made.module, "fs_layers_each", crate::each::blend_state(blend));
                        made.each.insert(code, p);
                    }
                }
            }
        }
        if self.made.len() > KEEP_PROGRAMS {
            self.made.retain(|k, _| used.contains(k));
        }
        self.used = used;
    }

    /// The pipeline program `key` draws with (in a layer whose things blend with each other by
    /// `each`), when it built.
    pub(crate) fn pipeline(&self, key: u64, each: Option<u32>) -> Option<&wgpu::RenderPipeline> {
        let made = self.made.get(&key)?.as_ref().ok()?;
        match each {
            Some(code) => made.each.get(&code),
            None => Some(&made.normal),
        }
    }

    /// Why programs the last frame named didn't build, by key.
    pub(crate) fn errors(&self) -> impl Iterator<Item = (u64, &str)> {
        self.used.iter().filter_map(|k| match self.made.get(k) {
            Some(Err(e)) => Some((*k, e.as_str())),
            _ => None,
        })
    }
}
