//! The Shaders workspace: shader graphs on a node canvas. A stub until shader nodes land.

use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{self, RichText, Ui};

#[derive(Default)]
pub struct State {}

pub fn show(ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new("Shader blocks are coming soon.").size(16.0).color(theme::MUTED));
    });
    let _ = egui::Id::NULL;
}
