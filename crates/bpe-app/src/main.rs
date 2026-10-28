//! Ball Studio: design a scene, watch it with sound, and export it as an mp4.

// No console window behind the editor in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_out;
mod batch;
mod command_palette;
mod dice_panel;
mod draw_shape;
mod explorer;
mod export;
mod gallery;
mod inspector;
#[cfg(test)]
mod inspector_tests;
mod keys;
mod my_presets;
mod node_editor;
mod orbit;
mod preview;
mod rules_workspace;
mod setting_menu;
mod settings;
mod shader_workspace;
#[cfg(test)]
mod text_tests;
mod theme;
mod timeline;
mod workspace;

fn main() -> eframe::Result {
    let scene_path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_title("Ball Studio"),
        ..Default::default()
    };
    eframe::run_native("Ball Studio", options, Box::new(move |cc| Ok(Box::new(app::StudioApp::new(cc, scene_path)))))
}
