//! The all-drops engine's passes on the card: the entry points, the bindings each uses, and their
//! pipelines.

/// The shader: its parts joined in order (common.wgsl declares what the rest use).
pub(super) const SOURCE: &str = concat!(
    include_str!("shaders/common.wgsl"),
    include_str!("shaders/sort.wgsl"),
    include_str!("shaders/sweep.wgsl"),
    include_str!("shaders/finish.wgsl"),
    include_str!("shaders/deep.wgsl"),
    include_str!("shaders/air.wgsl"),
    include_str!("shaders/report.wgsl"),
);

/// A pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum P {
    Predict,
    SumA,
    SumB,
    SumC,
    Scatter,
    Order,
    LambdaFirst,
    Lambda,
    Apply,
    ApplyLast,
    Finish,
    FinishPredict,
    Born,
    DeepWalls,
    DeepGather,
    DeepCells,
    DeepColumn,
    DeepAcross,
    DeepWant,
    DeepSor,
    DeepDown,
    DeepUp,
    DeepPush,
    AirFind,
    AirSumA,
    AirSumB,
    AirDead,
    AirBorn,
    Report,
    Export,
}

/// The bindings: 0 the frame's numbers, 1 the step's (at its offset), 2 the places and speeds, 3
/// the liquid's ways and fullness, 4 and 5 the kinds and serials (as they were, sorted), 6 where
/// each drop would be, 7 the bins' counts, 8 their starts, 9 the blocks' sums, 10 the sorted order,
/// 11 and 12 the sorted places (in, out), 13 the multipliers, 14 where each was and then its new
/// speed, 15 the solid field, 16 the step's births, 17 the drops in `Drop`'s layout, 18 the
/// frame's report; for the deep water (deep.wgsl) 19 the cells' sums, 20 the solids by cell, 21 the
/// pressure, 22 the faces' open shares, 23 how full each cell is and then its depth share, 24 how far
/// down its column a cell that is not full is, 25 the cells deep at the frame's end, 26 how full the
/// drops-only water just above the deep water is; for trapped air (air.wgsl) 27 each cell's spots,
/// 28 the blocks' running sums and every spot.
pub(super) const ENTRIES: &[(&str, &[u32])] = &[
    ("cs_predict", &[0, 1, 2, 3, 4, 6, 7, 15, 16]),
    ("cs_sum_a", &[0, 1, 7, 8, 9]),
    ("cs_sum_b", &[0, 1, 9]),
    ("cs_sum_c", &[0, 1, 7, 8, 9]),
    ("cs_scatter", &[0, 1, 6, 7, 8, 10]),
    ("cs_order", &[0, 1, 3, 4, 5, 6, 8, 10, 11, 14]),
    ("cs_lambda_first", &[0, 1, 7, 8, 11, 13, 14, 15, 23]),
    ("cs_lambda", &[0, 1, 8, 11, 13, 14, 15, 23]),
    ("cs_apply", &[0, 1, 8, 11, 12, 13, 14, 15]),
    ("cs_apply_last", &[0, 1, 8, 11, 12, 13, 14, 15]),
    ("cs_finish", &[0, 1, 2, 3, 8, 11, 13, 14, 15]),
    ("cs_finish_predict", &[0, 1, 3, 6, 7, 8, 11, 13, 14, 15]),
    ("cs_born", &[0, 1, 2, 3, 4, 6, 7, 16]),
    ("cs_deep_walls", &[0, 1, 15, 20]),
    ("cs_deep_gather", &[0, 1, 11, 14, 19]),
    ("cs_deep_cells", &[0, 1, 19, 20, 21, 22, 23, 26]),
    ("cs_deep_column", &[0, 1, 23, 24]),
    ("cs_deep_across", &[0, 1, 19, 21, 23, 24, 25, 26]),
    ("cs_deep_want", &[0, 1, 21, 26]),
    ("cs_deep_sor", &[0, 1, 21, 22]),
    ("cs_deep_down", &[0, 1, 21, 22]),
    ("cs_deep_up", &[0, 1, 21]),
    ("cs_deep_push", &[0, 1, 11, 14, 15, 21, 22, 23]),
    ("cs_air_find", &[0, 1, 8, 11, 15, 19, 20, 27]),
    ("cs_air_sum_a", &[0, 1, 27, 28]),
    ("cs_air_sum_b", &[0, 1, 28]),
    ("cs_air_dead", &[0, 1, 2, 3, 5, 8, 28]),
    ("cs_air_born", &[0, 1, 2, 3, 5, 8, 27, 28]),
    ("cs_report", &[0, 1, 2, 5, 8, 15, 18, 25, 28]),
    ("cs_export", &[0, 1, 2, 3, 4, 17]),
];

/// The step's numbers' binding, and the room each step's take up in its buffer (the most a
/// dynamic offset must be a whole number of, on every card).
pub(super) const STEP: u32 = 1;
pub(super) const STEP_ROOM: u64 = 256;
/// The most storage buffers a pass binds (wgpu's default limit for a stage).
#[cfg(test)]
pub(super) const MOST_STORAGE: usize = 8;

fn binding_type(b: u32) -> wgpu::BindingType {
    match b {
        0 => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        STEP => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: std::num::NonZeroU64::new(16),
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
            label: Some("all drops"),
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
    fn every_pass_has_its_entry_and_fits_the_limits() {
        assert_eq!(ENTRIES.len(), P::Export as usize + 1);
        assert_eq!(ENTRIES[P::Order as usize].0, "cs_order");
        assert_eq!(ENTRIES[P::Finish as usize].0, "cs_finish");
        assert_eq!(ENTRIES[P::Born as usize].0, "cs_born");
        assert_eq!(ENTRIES[P::DeepWalls as usize].0, "cs_deep_walls");
        assert_eq!(ENTRIES[P::DeepPush as usize].0, "cs_deep_push");
        assert_eq!(ENTRIES[P::AirFind as usize].0, "cs_air_find");
        assert_eq!(ENTRIES[P::AirBorn as usize].0, "cs_air_born");
        assert_eq!(ENTRIES[P::Report as usize].0, "cs_report");
        for (name, bindings) in ENTRIES {
            let storage = bindings.iter().filter(|&&b| b > STEP).count();
            assert!(storage <= MOST_STORAGE, "{name} binds {storage} storage buffers");
        }
    }
}
