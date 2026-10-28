//! "Pulse with music" in the setting menu. Owned by the audio-reactive workstream, which fills
//! this in.

use super::Target;
use eframe::egui::Ui;

/// Whether this setting can pulse with the music. The item stays hidden while this is false.
pub fn available(_target: &Target) -> bool {
    false
}

/// The submenu: bass, mid, treble or beat.
pub fn menu(_ui: &mut Ui, _target: &Target) {}
