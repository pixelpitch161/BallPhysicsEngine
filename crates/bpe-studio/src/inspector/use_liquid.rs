//! A Water Liquid's **Use a liquid…** menu (Water mode part 4.3a): the ready-made liquids, each
//! put on every selected Liquid as plain values it owns from then on (`bpe_kit::modules::water::
//! apply_liquid`), as an undo step of its own.

use bpe_kit::modules::water::ReadyLiquid;
use eframe::egui::Ui;

use super::CARD_MARGIN;
use super::edits::Edit;

/// Draws the menu under the Liquid's name and tags; a liquid picked pushes [`Edit::UseLiquid`].
pub(super) fn menu(ui: &mut Ui, edits: &mut Vec<Edit>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(CARD_MARGIN as f32);
        ui.menu_button("Use a liquid…", |ui| {
            for liquid in ReadyLiquid::ALL {
                if ui.button(liquid.title()).on_hover_text(liquid.words()).clicked() {
                    edits.push(Edit::UseLiquid(liquid));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Set how this liquid flows, mixes and looks to a ready-made liquid");
    });
}
