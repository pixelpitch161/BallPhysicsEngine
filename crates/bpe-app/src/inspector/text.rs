//! The Text tab: captions, counters and timers drawn over the video. Owned by the captions
//! workstream, which fills in this placeholder.

use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{RichText, Ui};

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {
    ui.label(
        RichText::new("Captions like \"Can it escape?\", ball counters and timers are on their way.")
            .color(theme::MUTED),
    );
}
