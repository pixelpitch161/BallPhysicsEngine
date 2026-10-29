//! Ball Studio Next: design a scene from shared modules, watch it with sound, and export it.

// No console window behind the editor in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_out;
mod export_window;
mod history;
mod inspector;
mod library;
mod preview;
mod selection;
mod settings;
mod sound_job;
mod theme;
mod timeline;
mod transport;
mod tree;
mod widgets;

fn main() -> eframe::Result {
    let scene_path = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_title("Ball Studio Next"),
        ..Default::default()
    };
    eframe::run_native("Ball Studio Next", options, Box::new(move |cc| Ok(Box::new(app::Studio::new(cc, scene_path)))))
}
