//! The step's passes on the card: the entry points, the bindings each uses, and their pipelines.

/// The shader: its parts joined in order (common.wgsl declares what the rest use).
pub(super) const SOURCE: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/walls.wgsl"),
    include_str!("../shaders/hold.wgsl"),
    include_str!("../shaders/drops.wgsl"),
    include_str!("../shaders/grid.wgsl"),
    include_str!("../shaders/pressure.wgsl"),
);

/// A pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum P {
    Walls,
    Clear,
    HoldSpread,
    HoldGrid,
    Spread,
    Grid,
    Sor,
    Down,
    Up,
    Proj,
    Slip,
    Gather,
}

/// The entry points in [`P`]'s order, and the bindings each uses: 0 the step, 1 the drops, 2 the
/// solid field, 3 the sums, 4 the solids by cell, 5 the faces' speeds, 6 the pressure, 7 the faces'
/// open shares, 8 the speeds the drops gather, 9 the pass's number, 10 the hold nodes' sums, 11 the
/// hold nodes, 12 the drops by spot.
pub(super) const ENTRIES: &[(&str, &[u32])] = &[
    ("cs_walls", &[0, 2, 4]),
    ("cs_clear", &[0, 3, 10, 12]),
    ("cs_hold_p2g", &[0, 1, 10, 12]),
    ("cs_hold_grid", &[0, 4, 10, 11]),
    ("cs_p2g", &[0, 1, 3, 11]),
    ("cs_grid", &[0, 3, 4, 5, 6, 7]),
    ("cs_sor", &[0, 6, 7, 9]),
    ("cs_down", &[0, 6, 7, 9]),
    ("cs_up", &[0, 6, 9]),
    ("cs_proj", &[0, 4, 5, 6]),
    ("cs_slip", &[0, 2, 4, 5, 8]),
    ("cs_g2p", &[0, 1, 2, 4, 8, 11, 12]),
];

/// Bytes of the pass's number (`Tick`).
pub(super) const TICK_BYTES: u64 = 16;
/// The pass's number's binding.
pub(super) const TICK: u32 = 9;

fn binding_type(b: u32) -> wgpu::BindingType {
    match b {
        0 => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        TICK => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: std::num::NonZeroU64::new(TICK_BYTES),
        },
        _ => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }
}

pub(super) struct Pipelines {
    pub(super) list: Vec<(wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
}

impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Pipelines {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water step"),
            source: wgpu::ShaderSource::Wgsl(SOURCE.into()),
        });
        let list = ENTRIES
            .iter()
            .map(|(name, bindings)| {
                let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                    .iter()
                    .map(|&b| wgpu::BindGroupLayoutEntry {
                        binding: b,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: binding_type(b),
                        count: None,
                    })
                    .collect();
                let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(name),
                    entries: &entries,
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(name),
                    bind_group_layouts: &[Some(&group)],
                    immediate_size: 0,
                });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: Default::default(),
                    cache: None,
                });
                (pipeline, group)
            })
            .collect();
        Pipelines { list }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pass_has_its_entry() {
        assert_eq!(ENTRIES.len(), P::Gather as usize + 1);
        assert_eq!(ENTRIES[P::Slip as usize].0, "cs_slip");
        assert_eq!(ENTRIES[P::HoldGrid as usize].0, "cs_hold_grid");
    }
}
