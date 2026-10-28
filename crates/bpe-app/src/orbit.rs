//! Moving the 3D camera by dragging in the preview: drag to turn it around what it looks at,
//! Shift+drag to move it sideways, scroll to zoom, right-click to put it back.

use bpe_scene::{Camera, Scene, Vec2};
use eframe::egui::{self, CursorIcon, Rect, Response, Ui};

/// Degrees the camera turns per point dragged.
const TURN_PER_POINT: f32 = 0.35;
/// How much one point of scrolling zooms.
const ZOOM_PER_POINT: f32 = 0.0015;

pub const HINT: &str = "Drag to turn the camera · Shift+drag to move it · scroll to zoom · right-click to reset";

/// Handles camera dragging for a 3D scene. Returns false for a flat scene, which it leaves alone.
pub fn interact(ui: &Ui, response: &Response, scene: &mut Scene, rect: Rect) -> bool {
    let aspect = scene.video.width as f32 / scene.video.height.max(1) as f32;
    let Some(space) = scene.space_mut() else { return false };
    let camera = &mut space.camera;
    let shift = ui.input(|i| i.modifiers.shift);
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        if shift {
            camera.pan(Vec2::new(delta.x, delta.y) * world_per_point(camera, rect));
        } else {
            camera.orbit(-delta.x * TURN_PER_POINT, delta.y * TURN_PER_POINT);
        }
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            camera.zoom((-scroll * ZOOM_PER_POINT).exp());
        }
    }
    if response.secondary_clicked() {
        *camera = Camera { mode: camera.mode.clone(), ..Camera::fit(&space.container, aspect) };
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(if shift { CursorIcon::Move } else { CursorIcon::Grabbing });
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    true
}

/// World pixels at the camera's target per screen point, so a pan keeps the target under the pointer.
fn world_per_point(camera: &Camera, rect: Rect) -> f32 {
    2.0 * camera.distance() * (camera.fov.to_radians() * 0.5).tan() / rect.height().max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Space, View};

    #[test]
    fn panning_keeps_pace_with_the_pointer() {
        let camera = Camera::default();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 600.0));
        let per_point = world_per_point(&camera, rect);
        // The whole height of the preview shows this much of the world at the target.
        let seen = 2.0 * camera.distance() * (camera.fov.to_radians() * 0.5).tan();
        assert!((per_point * 600.0 - seen).abs() < 0.01);
    }

    #[test]
    fn flat_scenes_are_left_alone() {
        let ctx = egui::Context::default();
        let mut scene = Scene::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let rect = ui.max_rect();
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            assert!(!interact(ui, &response, &mut scene, rect));
            let mut space = Scene { view: View::Space(Box::new(Space::default())), ..Scene::default() };
            assert!(interact(ui, &response, &mut space, rect));
        });
        output.textures_delta.clear();
    }
}
