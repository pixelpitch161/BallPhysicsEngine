//! Ball Studio Next: design a scene of things, watch it play, and export it.

// No console window behind the editor in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio_out;
mod export_window;
mod follow;
mod gradient_bar;
mod grid;
mod history;
mod inspector;
mod look_editor;
mod mixer;
mod palettes;
mod preview;
mod selection;
mod settings;
mod shape_handles;
mod sound_job;
mod sound_tab;
mod style;
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
            // Opens filling the screen; F11 goes fully fullscreen and back.
            .with_maximized(true)
            .with_title("Ball Studio Next"),
        ..Default::default()
    };
    eframe::run_native("Ball Studio Next", options, Box::new(move |cc| Ok(Box::new(app::Studio::new(cc, scene_path)))))
}
