//! The modular Ball Studio's renderer.
//!
//! Each thing's drawing part writes instances into a [`Frame`]; the [`Renderer`] draws the frame
//! into an HDR texture, adds bloom, and writes the result as sRGB-encoded `Rgba8Unorm`, which
//! egui can show directly and video encoders read as ordinary RGBA bytes.
//!
//! Where things are (`docs/notes/file-map.md` has the whole map): `frame/` is what to draw,
//! `renderer/` draws it (`new.rs` builds the pipelines, `scene.rs` runs the passes, `draw.rs` the
//! draw calls), `lighting/` and `shaders/lighting/` are the lighting passes, `light_pass.rs`
//! names the light-pass numbers, `effects/` the effects stack, and `readback.rs` reads frames
//! back for export. This file holds the shader sources shared by several pipelines and the
//! graphics devices for tests.

mod accumulate;
pub mod bake;
pub mod bent_light;
mod blend;
#[cfg(test)]
mod blend_tests;
pub mod dichroic;
mod each;
pub mod effects;
mod fields;
#[cfg(test)]
mod fields_tests;
mod frame;
pub mod grade;
mod gradients;
mod light_pass;
mod lighting;
mod liquid;
#[cfg(test)]
mod output_tests;
pub mod pipes;
pub use liquid::{Liquid, LiquidBlob, LiquidStyle, MOST_BLOBS};
mod ripples;
pub mod stage_camera;
mod stage_camera_pass;
pub use stage_camera::{FillEdges, StageCamera, StageCameraGpu, StageLayout, StageView};
#[cfg(test)]
mod lighting_tests;
mod timing;
pub use lighting::TILED_FROM as LIGHTS_TILED_FROM;
pub use timing::FEATURES as TIMING_FEATURES;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod lens_tests;
mod masks;
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod paths_tests;
#[cfg(test)]
mod pictures;
mod programs;
#[cfg(test)]
mod programs_tests;
mod readback;
mod renderer;
pub mod sprites;
#[cfg(test)]
mod sprites_tests;
#[cfg(test)]
mod sub_layers_tests;
pub mod swarm;
mod util;
pub mod yuv;

pub use frame::{
    Around, BENDS_FROM, BackgroundPaint, BallMark, Bender, BlendLayer, DrawCx, Draws, EffectCall, FIELD_FLAME,
    FIELD_HEAT_STOPS, FIELD_SMOKE, FREE_BENDER, Field, FieldLook, FlashInstance, Frame, GradeArea, GradeBasic,
    GradeCalibration, GradeColourGrading, GradeColourRange, GradeCombine, GradeDetail, GradeEffects, GradeHsl,
    GradeLinear, GradeLuminanceRange, GradeMask, GradePanel, GradePart, GradePick, GradePlace, GradePointColour,
    GradeRadial, GradeShape, GradeStack, GradeToneCurve, GradeWheel, HitMark, LayerGpu, LayeredInstance, LightGpu,
    LightScene, LightShow, LightingGpu, MAX_LAYERS, MOST_MIRRORS, MOST_PIPES, MOST_RIPPLE_SOURCES, MOST_THROWERS,
    MOST_TRAIL_VERTICES, MirrorGpu, ModuleDraw, NO_HIT, PATH_GLYPH, Particle, PathChannel, PathInstance, PathStyle,
    Pipe, Place, RibbonGpu, SphereGpu, Splat, Stage, StreakStyle, SurfaceGpu, Takes, ThingDraw, Thrower, TrailVertex,
    blend_alpha, build_frame, build_frame_sensing, build_frame_with, hit_strength, pack_tags, push_streak,
    push_streak_colours, push_trail, straight_curve, tag_parts, thing_code,
};
pub use gradients::{ATLAS_WIDTH, GradientAtlas, NO_VARY, NO_WAVE, PaintGpu, wave_texels};
pub use masks::{MASK_TILE, MASK_TILE_BYTES, MAX_MASK_TILES, TILE_DISTANCE_REACH, tile_distance};
pub use programs::{LayeredRun, LookProgram, ProgramStore, in_ball_problem, picture_problem};
pub use readback::Readback;
pub use renderer::*;
pub use swarm::draw::{SwarmLook, SwarmRun};
pub use wgpu;

use bpe_runtime::{CameraView, step_at};
use bytemuck::{Pod, Zeroable};

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The object picture shader effects on things read: own position, thing code, coverage.
pub(crate) const OBJECT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// The lighting passes' shader source: the files in `shaders/lighting/`, one per pass, in this
/// order (later files use what earlier ones declare), then the shared helpers (common.wgsl).
/// - `bindings`: the `Lighting` uniform, the lights, every binding, the full-screen triangle;
/// - `field`: where things are, the jump flood and the blockers' field (`fs_seed`, `fs_jump`, `fs_field`);
/// - `shadows`: falloff, light shapes and the shadow march (`march`, `visibility`);
/// - `direct`: the light from Light things: gobos, links, tiles, `lamps`, `fs_direct`;
/// - `light_field`: the light field (radiance cascades, `fs_gi_*`, `fs_cascade`);
/// - `haze_bloom`: moving haze and the memory (`fs_stir`, `fs_extras`), the lights' bloom, sun rays and lasers;
/// - `stage`: surfaces and the Stage: spheres, the slab march, sphere shadows;
/// - `shade`: highlights, films, dichroic, `shade`, `trace_scene`, light through glass;
/// - `mix`: the mix (`surfaced`, the rainbow, `lit_at`, `fs_mix`) and mirror walls (`fs_mirror`);
/// - `dof_floor`: depth of field and the glossy floor;
/// - `bake`: the bake's traced bounce light and its clean-up;
/// - `caustics`: caustics through glass balls and the light beams' bands.
pub(crate) const LIGHTING_WGSL: &str = concat!(
    include_str!("shaders/lighting/bindings.wgsl"),
    include_str!("shaders/lighting/field.wgsl"),
    include_str!("shaders/lighting/shadows.wgsl"),
    include_str!("shaders/lighting/direct.wgsl"),
    include_str!("shaders/lighting/light_field.wgsl"),
    include_str!("shaders/lighting/haze_bloom.wgsl"),
    include_str!("shaders/lighting/stage.wgsl"),
    include_str!("shaders/lighting/shade.wgsl"),
    include_str!("shaders/lighting/mix.wgsl"),
    include_str!("shaders/lighting/dof_floor.wgsl"),
    include_str!("shaders/lighting/bake.wgsl"),
    include_str!("shaders/lighting/caustics.wgsl"),
    include_str!("shaders/common.wgsl"),
);

/// The scene shader: everything the scene's pipelines draw with, its layered balls walking their
/// stacks as [`layers_source`] says for `effects`.
pub(crate) fn scene_source(effects: Option<&str>) -> String {
    [
        gradients::paint_wgsl(1).as_str(),
        light_pass::WGSL,
        include_str!("shaders/tint.wgsl"),
        include_str!("shaders/scene.wgsl"),
        include_str!("shaders/ball_look.wgsl"),
        include_str!("shaders/trail.wgsl"),
        include_str!("shaders/each.wgsl"),
        include_str!("shaders/paths.wgsl"),
        include_str!("shaders/blend_maths.wgsl"),
        layers_source(effects).as_str(),
    ]
    .concat()
}

/// layers.wgsl with its stack walk, and shape_layers.wgsl with the shapes', both with the
/// sub-layers' maths (sub_layers.wgsl): `None` for stacks without shader layers (they are left
/// out); `Some(effects)` adds the walks shader layers sample (`walk_plain`, `walk_shape_plain`,
/// leaving them out) and the walks that run them, with `effects` (the effect code and
/// `run_effect`) after.
pub(crate) fn layers_source(effects: Option<&str>) -> String {
    let walk = include_str!("shaders/layers_walk.wgsl");
    let named = |name: &str, cov: &str, case: &str, shaded: &str| {
        walk.replace("WALK", name)
            .replace("COV_IN", &format!("{cov}_in"))
            .replace("COV_OUT", &format!("{cov}_out"))
            .replace("//SHADER_CASE", case)
            .replace("//GROUP_SHADED", shaded)
    };
    let shape_walk = include_str!("shaders/shape_walk.wgsl");
    let shape_named = |name: &str, cov: &str, case: &str| {
        shape_walk
            .replace("SHAPE_WALK", name)
            .replace("SHAPE_COV_IN", &format!("{cov}_in"))
            .replace("SHAPE_COV_OUT", &format!("{cov}_out"))
            .replace("//SHAPE_SHADER_CASE", case)
    };
    let mut out = String::from(include_str!("shaders/layers.wgsl"));
    out.push_str(include_str!("shaders/sub_layers.wgsl"));
    out.push_str(include_str!("shaders/shape_layers.wgsl"));
    match effects {
        None => {
            out.push_str(&named("walk_layers", "cov", "", ""));
            out.push_str(&shape_named("walk_shape", "shape_cov", ""));
        }
        Some(code) => {
            // The walks shader layers sample see the things' own colours in the emission pass
            // (mode 3), so a glowing shader layer gives off its effect of them.
            let plain = |s: String| s.replace("g.light.x", "plain_mode()");
            out.push_str(&plain(named("walk_plain", "cov_plain", "", "")));
            out.push_str(&named(
                "walk_layers",
                "cov",
                include_str!("shaders/layers_shader_case.wgsl"),
                include_str!("shaders/layers_group_shaded.wgsl"),
            ));
            out.push_str(&plain(shape_named("walk_shape_plain", "shape_cov_plain", "")));
            out.push_str(&shape_named("walk_shape", "shape_cov", include_str!("shaders/shape_shader_case.wgsl")));
            out.push_str(code);
        }
    }
    out
}

/// Creates a GPU device without a window, for exporting from the command line and for tests.
pub fn headless_device() -> anyhow::Result<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))?;
    let (device, queue) = pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor { label: Some("bpe headless"), ..Default::default() }),
    )?;
    Ok((device, queue))
}

/// Set to `1` to let picture tests skip on a machine without a graphics adapter.
pub const SKIP_GPU_TESTS: &str = "BPE_SKIP_GPU_TESTS";

/// What a test does when no graphics device can be made.
#[derive(Debug, PartialEq, Eq)]
pub enum NoGpu {
    /// Skipping was asked for: the test returns early.
    Skip,
    /// The test fails with this message.
    Fail(String),
}

/// Decides what a test does without a graphics device: skip only when `skip_asked`, else fail naming the fix.
pub fn no_gpu(skip_asked: bool, why: &str) -> NoGpu {
    if skip_asked {
        NoGpu::Skip
    } else {
        NoGpu::Fail(format!(
            "no graphics device for a picture test ({why}). Install a Vulkan driver (in a Linux container: \
             apt-get install mesa-vulkan-drivers), or set {SKIP_GPU_TESTS}=1 to skip the picture tests on purpose."
        ))
    }
}

/// The graphics device for a test. Same as [`headless_device`], but without a device the test fails loudly unless
/// `BPE_SKIP_GPU_TESTS=1` is set, when the error comes back and the test skips. Every test that draws uses this.
pub fn test_device() -> anyhow::Result<(wgpu::Device, wgpu::Queue)> {
    match headless_device() {
        Ok(pair) => Ok(pair),
        Err(e) => {
            let skip = std::env::var(SKIP_GPU_TESTS).is_ok_and(|v| v == "1");
            match no_gpu(skip, &e.to_string()) {
                NoGpu::Skip => Err(e),
                NoGpu::Fail(message) => panic!("{message}"),
            }
        }
    }
}

#[cfg(test)]
mod test_device_tests {
    use super::*;

    #[test]
    fn a_missing_gpu_fails_unless_skipping_was_asked_for() {
        assert_eq!(no_gpu(true, "none"), NoGpu::Skip);
        let NoGpu::Fail(message) = no_gpu(false, "no adapter") else { panic!("should fail") };
        assert!(message.contains("no adapter") && message.contains(SKIP_GPU_TESTS) && message.contains("mesa"));
    }
}
