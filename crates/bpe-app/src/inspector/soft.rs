//! Soft bodies: jelly balls, ropes, cloth and fluid (Balls tab), and ball-to-ball settings (Bounces
//! tab). Owned by the physics workstream, which fills in this placeholder.

use bpe_scene::Scene;
use eframe::egui::Ui;

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(_ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {}

/// The ball-to-ball settings in the Bounces tab: friction, spin, stacking and presets.
pub fn bounces_section(_ui: &mut Ui, _scene: &mut Scene, _state: &mut State) {}
