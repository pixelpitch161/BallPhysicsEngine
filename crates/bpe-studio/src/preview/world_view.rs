//! The preview's World view: a free canvas over the whole world, with the camera drawn as the
//! frame of its shot that can be moved, zoomed and turned by dragging.

use bpe_core::{Address, Owner, Registry, Resolved, Scene, Settings, Value};
use bpe_kit::CameraSettings;
use bpe_runtime::{CameraView, Log, World};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Stroke};
use glam::Vec2;

use super::{Drag, Handle, View};
use crate::grid::snap_point;
use crate::theme;

/// What the preview shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewMode {
    /// Exactly what the video shows.
    #[default]
    Camera,
    /// A free view of the whole world, with the camera's shot drawn on it.
    World,
}

/// Where the World view looks: its own pan and zoom, never turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldNav {
    pub center: Vec2,
    pub zoom: f32,
}

impl Default for WorldNav {
    fn default() -> Self {
        Self { center: Vec2::ZERO, zoom: 0.4 }
    }
}

/// How far the World view zooms in and out.
const NAV_ZOOM: std::ops::RangeInclusive<f32> = 0.01..=8.0;
/// How close to a camera handle, in screen points, counts as grabbing it.
const CAMERA_GRAB: f32 = 10.0;
/// How far the turn handle sits above the shot's top edge, in screen points.
const TURN_REACH: f32 = 28.0;

impl WorldNav {
    pub fn camera(&self) -> CameraView {
        CameraView { center: self.center, zoom: self.zoom, turn: 0.0 }
    }

    /// Zooms by `factor`, keeping the world point under `pointer` where it is on screen.
    pub fn zoom_about(&mut self, view: &View, pointer: Pos2, factor: f32) {
        let under = view.to_world(pointer);
        self.zoom = (self.zoom * factor).clamp(*NAV_ZOOM.start(), *NAV_ZOOM.end());
        let d = (pointer - view.rect.center()) / (view.scale * self.zoom);
        self.center = under - Vec2::new(d.x, d.y);
    }

    /// Moves the view by a drag of `delta` screen points.
    pub fn pan(&mut self, view: &View, delta: egui::Vec2) {
        self.center -= Vec2::new(delta.x, delta.y) / (view.scale * self.zoom);
    }

    /// Looks at `at`, close enough to show about `span` world units across.
    pub fn frame_on(&mut self, view: &View, at: Vec2, span: f32) {
        self.center = at;
        self.zoom = (view.rect.width() / view.scale / span.max(1.0)).clamp(*NAV_ZOOM.start(), *NAV_ZOOM.end());
    }
}

/// The corners of the camera's shot in the world, clockwise from the top left.
pub fn shot_corners(camera: &CameraView, video: Vec2) -> [Vec2; 4] {
    let h = video * 0.5;
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| camera.to_world(Vec2::new(x, y) * h))
}

/// Where the turn handle is on screen: a little above the middle of the shot's top edge.
fn turn_handle(view: &View, camera: &CameraView, video: Vec2) -> Pos2 {
    let center = view.to_screen(camera.center);
    let top = view.to_screen(camera.to_world(Vec2::new(0.0, -video.y * 0.5)));
    top + (top - center).normalized() * TURN_REACH
}

/// The camera handle under `pos`, if the scene has a camera: the turn handle, a corner (zoom),
/// then the middle or an edge (move).
pub fn camera_hit(scene: &Scene, world: &World, view: &View, pos: Pos2) -> Option<Handle> {
    let id = bpe_kit::camera_of(scene)?;
    let camera = world.camera;
    if (turn_handle(view, &camera, world.video) - pos).length() <= CAMERA_GRAB {
        return Some(Handle::CameraTurn(id));
    }
    let corners = shot_corners(&camera, world.video).map(|c| view.to_screen(c));
    if corners.iter().any(|c| (*c - pos).length() <= CAMERA_GRAB) {
        return Some(Handle::CameraZoom(id));
    }
    if (view.to_screen(camera.center) - pos).length() <= CAMERA_GRAB + 2.0 {
        return Some(Handle::CameraMove(id));
    }
    let near_edge = (0..4).any(|i| distance_to_segment(pos, corners[i], corners[(i + 1) % 4]) <= CAMERA_GRAB * 0.6);
    near_edge.then_some(Handle::CameraMove(id))
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

/// Moves, zooms or turns the camera by a drag to `pointer`. A setting with keys gets a key at
/// `time` instead of a new plain value; returns whether one did.
pub fn apply_camera_drag(
    scene: &mut Scene,
    reg: &Registry,
    world: &World,
    drag: Drag,
    pointer: Vec2,
    step: Option<f32>,
    time: f64,
) -> bool {
    let id = match drag.handle {
        Handle::CameraMove(id) | Handle::CameraZoom(id) | Handle::CameraTurn(id) => id,
        _ => return false,
    };
    let address = |key: &str| Address { owner: Owner::Thing(id), slot: None, key: key.into() };
    let center = world.camera.center;
    let (key, value) = match drag.handle {
        Handle::CameraMove(_) => {
            let at = snap_point(pointer - drag.offset, step).round();
            ("position", Value::Vec2([at.x, at.y]))
        }
        Handle::CameraZoom(_) => {
            // The corner sits half the video's diagonal from the middle at Zoom 1.
            let reach = (pointer - center).length().max(1.0);
            let zoom = (world.video.length() * 0.5 / reach).clamp(0.05, 20.0);
            ("zoom", Value::Number(((zoom * 100.0).round() / 100.0) as f64))
        }
        _ => {
            let towards = pointer - center;
            let mut degrees = (towards.y.atan2(towards.x) + std::f32::consts::FRAC_PI_2).to_degrees();
            degrees = ((degrees + 180.0).rem_euclid(360.0)) - 180.0;
            // With the grid on, turns land on 15 degree steps.
            let degrees = if step.is_some() { (degrees / 15.0).round() * 15.0 } else { (degrees * 10.0).round() / 10.0 };
            ("turn", Value::Number(degrees as f64))
        }
    };
    let a = address(key);
    if scene.keyed(&a) {
        scene.set_key(reg, &a, time, value).is_ok()
    } else {
        let _ = scene.set_at(reg, &a, value);
        false
    }
}

/// Draws the camera's shot as a frame with its handles, the world box when it has one, and
/// where the shot has been over the last two seconds.
pub fn draw_camera(
    painter: &Painter,
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    view: &View,
    active: Option<Handle>,
) {
    let Some(id) = bpe_kit::camera_of(scene) else { return };
    let camera = world.camera;
    let video = world.video;
    let on = |h: Handle| active == Some(h);
    let accent = theme::ACCENT;

    if let Some(settings) = resolved.things.get(&id).map(|r| CameraSettings::from_params(&r.params))
        && settings.stay_inside
    {
        let h = settings.box_size.abs() * 0.5;
        let c = settings.box_centre;
        let points: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
            .iter()
            .map(|&(x, y)| view.to_screen(c + Vec2::new(x, y) * h))
            .collect();
        painter.extend(egui::Shape::dashed_line(&points, Stroke::new(1.2, theme::MUTED), 8.0, 6.0));
    }

    // Where the shot's middle has been, fading out over two seconds.
    let rate = world.steps_per_second.max(1) as u64;
    let start = world.step.saturating_sub(rate * 2);
    let path: Vec<Pos2> = (start..=world.step)
        .step_by((rate / 30).max(1) as usize)
        .map(|s| view.to_screen(log.camera_at(s).center))
        .collect();
    if path.len() > 1 {
        painter.add(egui::Shape::line(path, Stroke::new(1.0, Color32::from_white_alpha(60))));
    }

    let corners = shot_corners(&camera, video).map(|c| view.to_screen(c));
    let framed = on(Handle::CameraMove(id)) || on(Handle::CameraZoom(id));
    let stroke = Stroke::new(if framed { 2.5 } else { 1.6 }, accent);
    painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
    for c in corners {
        let size = if on(Handle::CameraZoom(id)) { 6.0 } else { 4.5 };
        painter.rect_filled(egui::Rect::from_center_size(c, egui::vec2(size, size) * 2.0), 1.0, accent);
    }
    let middle = view.to_screen(camera.center);
    painter.circle(middle, 5.0, accent, Stroke::new(1.5, Color32::BLACK));
    let top = view.to_screen(camera.to_world(Vec2::new(0.0, -video.y * 0.5)));
    let turn = turn_handle(view, &camera, video);
    painter.line_segment([top, turn], Stroke::new(1.2, accent));
    painter.circle(turn, if on(Handle::CameraTurn(id)) { 7.0 } else { 5.5 }, Color32::WHITE, Stroke::new(1.5, accent));
    painter.text(corners[0] + egui::vec2(0.0, -6.0), Align2::LEFT_BOTTOM, "Camera", FontId::proportional(12.0), accent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Studio;
    use bpe_core::ThingId;
    use eframe::egui::{Rect, vec2};

    fn world_view(nav: WorldNav) -> View {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        View { rect, scale: 800.0 / 2560.0, camera: nav.camera() }
    }

    #[test]
    fn world_view_round_trips() {
        let view = world_view(WorldNav { center: Vec2::new(1000.0, 4000.0), zoom: 0.2 });
        for p in [Vec2::new(0.0, 0.0), Vec2::new(-3000.0, 9000.0)] {
            assert!((view.to_world(view.to_screen(p)) - p).length() < 1e-2);
        }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut nav = WorldNav::default();
        let pointer = Pos2::new(600.0, 150.0);
        let before = world_view(nav).to_world(pointer);
        nav.zoom_about(&world_view(nav), pointer, 1.7);
        let after = world_view(nav).to_world(pointer);
        assert!((before - after).length() < 1e-2, "{before:?} {after:?}");
    }

    fn studio_with_camera() -> (Studio, ThingId) {
        let mut studio = Studio::headless();
        let scene = bpe_kit::new_studio_scene(&studio.kit.registry);
        studio.replace_scene(scene, None);
        let cam = bpe_kit::camera_of(&studio.scene).unwrap();
        (studio, cam)
    }

    #[test]
    fn dragging_the_camera_frame_moves_the_camera() {
        let (mut studio, cam) = studio_with_camera();
        let view = world_view(WorldNav::default());
        let world = studio.playback.world().clone();
        assert_eq!(camera_hit(&studio.scene, &world, &view, view.to_screen(Vec2::ZERO)), Some(Handle::CameraMove(cam)));
        let drag = Drag { handle: Handle::CameraMove(cam), offset: Vec2::ZERO };
        let reg = bpe_kit::registry();
        apply_camera_drag(&mut studio.scene, &reg, &world, drag, Vec2::new(100.0, 0.0), None, 0.0);
        assert_eq!(studio.scene.things[&cam].params.get("position"), Some(&Value::Vec2([100.0, 0.0])));
    }

    #[test]
    fn dragging_a_corner_zooms_and_the_top_handle_turns() {
        let (mut studio, cam) = studio_with_camera();
        let world = studio.playback.world().clone();
        let reg = bpe_kit::registry();
        let corner = Vec2::new(540.0, 960.0);
        let drag = Drag { handle: Handle::CameraZoom(cam), offset: Vec2::ZERO };
        apply_camera_drag(&mut studio.scene, &reg, &world, drag, corner * 0.5, None, 0.0);
        assert_eq!(studio.scene.things[&cam].params.get("zoom"), Some(&Value::Number(2.0)));
        let drag = Drag { handle: Handle::CameraTurn(cam), offset: Vec2::ZERO };
        apply_camera_drag(&mut studio.scene, &reg, &world, drag, Vec2::new(100.0, 0.0), None, 0.0);
        assert_eq!(studio.scene.things[&cam].params.get("turn"), Some(&Value::Number(90.0)));
    }

    #[test]
    fn dragging_a_keyed_zoom_adds_a_key() {
        let (mut studio, cam) = studio_with_camera();
        let reg = bpe_kit::registry();
        let zoom = Address { owner: Owner::Thing(cam), slot: None, key: "zoom".into() };
        studio.scene.set_key(&reg, &zoom, 0.0, Value::Number(1.0)).unwrap();
        let world = studio.playback.world().clone();
        let drag = Drag { handle: Handle::CameraZoom(cam), offset: Vec2::ZERO };
        let keyed = apply_camera_drag(&mut studio.scene, &reg, &world, drag, Vec2::new(270.0, 480.0), None, 2.0);
        assert!(keyed);
        let times: Vec<f64> = studio.scene.tracks[&zoom].keys.iter().map(|k| k.time).collect();
        assert_eq!(times, [0.0, 2.0]);
    }

    #[test]
    fn grid_snaps_in_world_view() {
        let (mut studio, cam) = studio_with_camera();
        let world = studio.playback.world().clone();
        let reg = bpe_kit::registry();
        let drag = Drag { handle: Handle::CameraMove(cam), offset: Vec2::ZERO };
        apply_camera_drag(&mut studio.scene, &reg, &world, drag, Vec2::new(1234.0, -77.0), Some(60.0), 0.0);
        assert_eq!(studio.scene.things[&cam].params.get("position"), Some(&Value::Vec2([1260.0, -60.0])));
    }
}
