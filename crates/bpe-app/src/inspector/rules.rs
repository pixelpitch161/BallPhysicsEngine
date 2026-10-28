//! The Rules tab: rules read as sentences ("when a ball escapes, spawn 3 balls"), recipes to
//! start from, and a button to open a rule as a graph.

use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{RichText, Ui};

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {
    ui.label(
        RichText::new("Rules make things happen when something else does, like \"when a ball escapes, spawn 3 balls and flash red\". They are on their way.")
            .color(theme::MUTED),
    );
}
