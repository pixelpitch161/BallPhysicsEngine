//! "Drive with a rule" in the setting menu: makes a rule with a block that sets this setting.

use super::Target;
use eframe::egui;

/// Whether a rule can set this setting. The item stays hidden while this is false.
pub fn available(_target: &Target) -> bool {
    false
}

/// Asks the app to create the rule; it is picked up after the panels are drawn.
pub fn request(_ctx: &egui::Context, _target: &Target) {}
