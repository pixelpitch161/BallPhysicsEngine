//! Ball Studio Next: design a scene of things, watch it play, and export it.

// No console window behind the editor in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod aim_job;
mod app;
mod audio_out;
mod eq_curve;
mod export_window;
mod eyedropper;
mod files;
mod final_frame;
mod follow;
mod font_picker;
mod gradient_bar;
mod gradient_editor;
mod gradient_form_chips;
mod gradient_tabs;
mod gradient_tools;
mod grid;
mod history;
mod inspector;
mod light_scenes_panel;
mod lights_panel;
mod look_editor;
mod mixer;
mod palettes;
mod particle_job;
mod preview;
mod recovery;
mod rules_panel;
mod selection;
mod setting_snap;
mod settings;
mod shape_handles;
mod sound_job;
mod sound_tab;
mod style;
mod things;
mod timeline;
mod transport;
mod tree;
mod vary;
mod widgets;
mod words_box;

fn main() -> eframe::Result {
    let scene_path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1000.0, 640.0])
            // Opens filling the screen; F11 goes fully fullscreen and back.
            .with_maximized(true)
            .with_title("Ball Studio"),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    eframe::run_native("Ball Studio Next", options, Box::new(move |cc| Ok(Box::new(app::Studio::new(cc, scene_path)))))
}

/// eframe's usual GPU set-up, asking also for GPU timestamps where the card has them, so the
/// status line can say how long lighting takes.
fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let base = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = base(adapter);
        if adapter.features().contains(bpe_gfx::TIMING_FEATURES) {
            descriptor.required_features |= bpe_gfx::TIMING_FEATURES;
        }
        descriptor
    });
    egui_wgpu::WgpuConfiguration { wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() }
}
