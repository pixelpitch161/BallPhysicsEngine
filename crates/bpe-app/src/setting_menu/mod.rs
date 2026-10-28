//! The one right-click menu every setting shares: Reset, Keyframe, Randomize, Pulse with music
//! and Drive with a rule. An item only shows once its feature says it can handle the setting,
//! so unfinished features stay out of the way.

pub mod drive;
pub mod pulse;

use crate::keys;
use bpe_scene::Property;
use eframe::egui::{self, Ui};

/// The setting a menu is for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    /// The animatable setting, when it is one.
    pub property: Option<Property>,
    /// The setting's name as shown.
    pub label: String,
    /// Where the setting lives in the scene, when dice can change it.
    pub lock: Option<crate::setting_locks::Path>,
}

/// Fills an open context menu for `target`. Returns true when "Reset to default" was clicked;
/// the caller resets, since only it knows the default.
pub fn items(ui: &mut Ui, target: &Target, can_reset: bool) -> bool {
    let mut reset = false;
    if ui.add_enabled(can_reset, egui::Button::new("Reset to default")).clicked() {
        reset = true;
        ui.close();
    }
    if let Some(property) = &target.property
        && ui.button("Keyframe at the playhead").on_hover_text("Animate this setting from here").clicked()
    {
        keys::request(ui.ctx(), keys::Request::Toggle(property.clone()));
        ui.close();
    }
    if let Some(path) = &target.lock {
        crate::setting_locks::menu_item(ui, path);
    }
    if pulse::available(target) {
        ui.menu_button("Pulse with music", |ui| pulse::menu(ui, target));
    }
    if drive::available(target) && ui.button("Drive with a rule").clicked() {
        drive::request(ui.ctx(), target);
        ui.close();
    }
    reset
}

fn row_id() -> egui::Id {
    egui::Id::unique("setting menu row property")
}

/// Remembers the animatable setting of the row being drawn, for its menu.
pub fn set_row_property(ctx: &egui::Context, property: Option<Property>) {
    ctx.data_mut(|d| d.insert_temp(row_id(), property));
}

/// The setting of the row being drawn, once. Rows without a keyframe diamond have none.
pub fn take_row_property(ctx: &egui::Context) -> Option<Property> {
    ctx.data_mut(|d| d.remove_temp::<Option<Property>>(row_id())).flatten()
}
