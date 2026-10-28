//! The Batch workspace: render many variations and pick the best. A stub until the batch
//! factory lands.

use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{RichText, Ui};

#[derive(Default)]
pub struct State {}

pub fn show(ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new("The batch factory is coming soon.").size(16.0).color(theme::MUTED));
    });
}
