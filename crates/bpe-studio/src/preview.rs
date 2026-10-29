//! The video preview, plus the handles that set a scene up by dragging in it: move a thing,
//! aim balls by their arrow tip, and resize the arena by its edge.

use crate::app::{CATCH_UP_BUDGET, Studio};
use crate::theme;
use bpe_core::{Resolved, Scene, Settings, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, wgpu};
use bpe_kit::{ArenaSettings, ArenaShape, BallsSettings, GIZMO_GRAB, Gizmo, gizmos};
use bpe_runtime::{Outline, World};
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;

/// The launch arrow's tip sits this many seconds of launch speed from the balls.
pub const ARROW_SECONDS: f32 = 0.25;

/// How close to an arena's outline, in screen points, counts as grabbing its edge.
const EDGE_GRAB: f32 = 12.0;

/// How close to an arrow tip, in screen points, counts as grabbing it.
const TIP_GRAB: f32 = 10.0;

pub struct Preview {
    renderer: Renderer,
    texture: egui::TextureId,
    frame: Frame,
    drag: Option<Drag>,
    /// What the picture on screen was drawn from, so it is only drawn again when that changes.
    drawn: Option<Drawn>,
}

#[derive(PartialEq)]
struct Drawn {
    scene: Scene,
    time: f64,
    step: u64,
    size: (u32, u32),
}

/// Something in the preview that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// Move a thing.
    Body(ThingId),
    /// Aim balls by their launch arrow.
    Aim(ThingId),
    /// Resize an arena by its edge.
    Edge(ThingId),
}

impl Handle {
    fn thing(self) -> ThingId {
        match self {
            Handle::Body(id) | Handle::Aim(id) | Handle::Edge(id) => id,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub handle: Handle,
    /// Pointer position minus the dragged thing's position, in world pixels.
    pub offset: Vec2,
}

/// What happened in the preview this frame, for the app to act on.
#[derive(Default)]
pub struct Outcome {
    /// The simulation hasn't reached the playhead yet, and the picture shows an earlier moment.
    pub catching_up: bool,
    /// A thing was clicked or dragged.
    pub selected: Option<ThingId>,
    /// A drag began: the app pauses and jumps to the start so the starting layout is visible.
    pub drag_started: bool,
    /// The preview was clicked while playing.
    pub clicked: bool,
}

/// Converts between world pixels (origin at the video centre, y down) and screen points.
pub struct View {
    pub rect: Rect,
    pub scale: f32,
}

impl View {
    pub fn to_screen(&self, p: Vec2) -> Pos2 {
        self.rect.center() + egui::vec2(p.x, p.y) * self.scale
    }

    pub fn to_world(&self, p: Pos2) -> Vec2 {
        let d = (p - self.rect.center()) / self.scale;
        Vec2::new(d.x, d.y)
    }
}

/// Draws the video at the playhead, aspect-fit in the panel, and handles dragging in it.
pub fn show(ui: &mut Ui, frame: &eframe::Frame, studio: &mut Studio) -> Outcome {
    let Some(render_state) = frame.wgpu_render_state() else {
        ui.label("The preview needs the wgpu renderer.");
        return Outcome::default();
    };
    let settings = studio.settings();
    let (video_width, video_height) = (settings.width.max(1), settings.height.max(1));
    let aspect = video_width as f32 / video_height as f32;
    let area = ui.available_rect_before_wrap().shrink(12.0);
    let mut size = egui::vec2(area.height() * aspect, area.height());
    if size.x > area.width() {
        size = egui::vec2(area.width(), area.width() / aspect);
    }
    let rect = Rect::from_center_size(area.center(), size);
    // Render at the screen's real pixel size, but never above the video's own size.
    let scale = (size.x * ui.ctx().pixels_per_point() / video_width as f32).min(1.0);
    let (width, height) = bpe_cli::output_size(video_width, video_height, scale);

    let device = &render_state.device;
    let mut preview = studio.panels.preview.take().unwrap_or_else(|| {
        let renderer = Renderer::new(device, width, height);
        let texture = render_state.renderer.write().register_native_texture(
            device,
            renderer.output_view(),
            wgpu::FilterMode::Linear,
        );
        Preview { renderer, texture, frame: Frame::default(), drag: None, drawn: None }
    });
    if preview.renderer.size() != (width, height) {
        preview.renderer.resize(device, width, height);
        render_state.renderer.write().update_egui_texture_from_wgpu_texture(
            device,
            preview.renderer.output_view(),
            wgpu::FilterMode::Linear,
            preview.texture,
        );
        preview.drawn = None;
    }

    let catching_up = !studio.playback.seek_within(studio.time, Some(CATCH_UP_BUDGET));
    let mut outcome = Outcome { catching_up, ..Outcome::default() };
    // While catching up, the picture shows the moment the simulation has got to.
    let shown_time = if outcome.catching_up { studio.playback.time() } else { studio.time };
    let drawn = Drawn {
        scene: studio.scene.clone(),
        time: shown_time,
        step: studio.playback.world().step,
        size: (width, height),
    };
    if preview.drawn.as_ref() != Some(&drawn) {
        build_frame(
            &studio.scene,
            &studio.resolved,
            studio.playback.world(),
            studio.playback.log(),
            shown_time,
            &studio.kit.draws,
            &mut preview.frame,
        );
        // Liquids and swarms on the graphics card step towards the playhead within the same
        // budget as the physics; until they are there the picture is drawn again next frame.
        let source = bpe_kit::flow_source(&studio.scene, &studio.resolved, &studio.kit.sims, Some(CATCH_UP_BUDGET));
        let status = preview.renderer.render_with_flow(device, &render_state.queue, &preview.frame, &source);
        if status.caught_up {
            preview.drawn = Some(drawn);
        } else {
            preview.drawn = None;
            outcome.catching_up = true;
        }
    }

    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.painter().image(preview.texture, rect, uv, Color32::WHITE);

    if studio.playing {
        preview.drag = None;
        outcome.clicked = response.clicked();
        draw_hint(ui, rect, "Click to pause");
        studio.panels.preview = Some(preview);
        return outcome;
    }

    let view = View { rect, scale: rect.width() / video_width as f32 };
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let hovered = pointer.and_then(|p| hit_test(studio, &view, p));

    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && let Some(handle) = hit_test(studio, &view, press)
    {
        let anchor = match handle {
            Handle::Body(id) => studio.scene.things.get(&id).map_or(view.to_world(press), |t| t.transform.position),
            Handle::Aim(_) | Handle::Edge(_) => view.to_world(press),
        };
        preview.drag = Some(Drag { handle, offset: view.to_world(press) - anchor });
        outcome.drag_started = true;
    }
    if response.drag_stopped() {
        preview.drag = None;
    }
    if let (Some(drag), Some(pos)) = (preview.drag, response.interact_pointer_pos()) {
        apply_drag(&mut studio.scene, drag, view.to_world(pos));
        outcome.selected = Some(drag.handle.thing());
    } else if response.clicked()
        && let Some(handle) = hovered
    {
        outcome.selected = Some(handle.thing());
    }

    let active = preview.drag.map(|d| d.handle).or(hovered);
    if active.is_some() {
        ui.ctx().set_cursor_icon(if preview.drag.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }
    draw_handles(ui, studio, &view, active, outcome.selected.or(studio.selected));
    draw_hint(ui, rect, "Drag things to move them · drag the arrow tip to aim · drag the arena's edge to resize");
    studio.panels.preview = Some(preview);
    outcome
}

/// The launch arrow's tip of a Balls thing, in world pixels.
fn arrow_tip(scene: &Scene, resolved: &Resolved, id: ThingId) -> Option<Vec2> {
    let thing = scene.things.get(&id)?;
    let r = resolved.things.get(&id)?;
    (r.kind == "balls")
        .then(|| thing.transform.position + BallsSettings::from_params(&r.params).velocity * ARROW_SECONDS)
}

/// The point on `outline` in the direction of `world` from its centre.
fn outline_point_towards(outline: &Outline, world: Vec2) -> Vec2 {
    let offset = world - outline.center();
    let angle = match *outline {
        Outline::Ring { angle, .. } | Outline::Box { angle, .. } => angle,
    };
    let u = if offset.length_squared() > 1e-6 { offset.to_angle() - angle } else { 0.0 };
    outline.point(u)
}

/// What is under `pos`: an arrow tip first, then a ball, then a gizmo's middle, then an
/// arena's edge.
pub fn hit_test(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let world: &World = studio.playback.world();
    let p = view.to_world(pos);
    let shown = |id: &ThingId| studio.scene.things.get(id).is_some_and(|t| t.enabled);
    for id in studio.resolved.things.keys().filter(|id| shown(id)) {
        if let Some(tip) = arrow_tip(&studio.scene, &studio.resolved, *id)
            && (view.to_screen(tip) - pos).length() <= TIP_GRAB
        {
            return Some(Handle::Aim(*id));
        }
    }
    let mut best: Option<(f32, ThingId)> = None;
    for (id, bodies) in world.bodies.iter().filter(|(id, _)| shown(id)) {
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let grab = (bodies.radius[i] * 1.5).max(8.0 / view.scale);
            let distance = (bodies.position[i] - p).length();
            if distance <= grab && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, *id));
            }
        }
    }
    if let Some((_, id)) = best {
        return Some(Handle::Body(id));
    }
    // Things with nothing of their own to click are grabbed by their gizmo's middle.
    for (id, thing) in studio.scene.things.iter().filter(|(_, t)| t.enabled) {
        let Some(r) = studio.resolved.things.get(id) else { continue };
        for gizmo in gizmos(thing, r, world) {
            let middle = match gizmo {
                Gizmo::Dot(at) | Gizmo::Circle(at, _) | Gizmo::Rect { center: at, .. } | Gizmo::Arrow(at, _) => at,
                Gizmo::Line(a, b) => (a + b) * 0.5,
            };
            if (view.to_screen(middle) - pos).length() <= GIZMO_GRAB {
                return Some(Handle::Body(*id));
            }
        }
    }
    for (id, wall) in world.walls.iter().filter(|(id, _)| shown(id)) {
        let edge = outline_point_towards(&wall.outline, p);
        if (edge - p).length() * view.scale <= EDGE_GRAB {
            return Some(Handle::Edge(*id));
        }
    }
    None
}

/// Moves what `drag` holds to the pointer at `world`.
pub fn apply_drag(scene: &mut Scene, drag: Drag, world: Vec2) {
    let Some(thing) = scene.things.get_mut(&drag.handle.thing()) else { return };
    match drag.handle {
        Handle::Body(_) => thing.transform.position = (world - drag.offset).round(),
        Handle::Aim(_) => {
            let velocity = ((world - thing.transform.position) / ARROW_SECONDS).round();
            thing.params.set("velocity", Value::Vec2([velocity.x, velocity.y]));
        }
        Handle::Edge(_) => {
            let settings = ArenaSettings::from_params(&thing.params);
            let offset = world - thing.transform.position;
            match settings.shape {
                ArenaShape::Box => {
                    let local = Vec2::from_angle(-thing.transform.rotation).rotate(offset);
                    thing.params.set("width", Value::Number((local.x.abs() * 2.0).round().max(20.0) as f64));
                    thing.params.set("height", Value::Number((local.y.abs() * 2.0).round().max(20.0) as f64));
                }
                ArenaShape::Circle | ArenaShape::Polygon => {
                    thing.params.set("radius", Value::Number(offset.length().round().max(20.0) as f64));
                }
            }
        }
    }
}

fn draw_handles(ui: &Ui, studio: &Studio, view: &View, active: Option<Handle>, selected: Option<ThingId>) {
    let painter = ui.painter_at(view.rect);
    let faint = Color32::from_white_alpha(110);
    let world = studio.playback.world();
    let shown = |id: &ThingId| studio.scene.things.get(id).is_some_and(|t| t.enabled);
    draw_gizmos(&painter, studio, view, active, selected);

    for (id, wall) in world.walls.iter().filter(|(id, _)| shown(id)) {
        let edge_active = active == Some(Handle::Edge(*id));
        let highlighted = edge_active || selected == Some(*id);
        if highlighted {
            let stroke =
                Stroke::new(if edge_active { 2.5 } else { 1.0 }, if edge_active { theme::ACCENT } else { faint });
            let center = wall.outline.center();
            let points: Vec<Pos2> = (0..=96)
                .map(|i| {
                    let towards = center + Vec2::from_angle(i as f32 / 96.0 * std::f32::consts::TAU);
                    view.to_screen(outline_point_towards(&wall.outline, towards))
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        let dot = view.to_screen(outline_point_towards(&wall.outline, wall.outline.center() + Vec2::X));
        painter.circle(
            dot,
            5.0,
            if edge_active { theme::ACCENT } else { theme::TEXT },
            Stroke::new(1.5, Color32::BLACK),
        );
    }

    for (id, thing) in studio.scene.things.iter().filter(|(id, t)| t.enabled && world.bodies.contains_key(id)) {
        let highlighted = selected == Some(*id) || active.is_some_and(|h| h.thing() == *id);
        let color = if highlighted { theme::ACCENT } else { faint };
        let center = view.to_screen(thing.transform.position);
        painter.circle_stroke(center, 6.0, Stroke::new(1.2, color));
        let Some(tip) = arrow_tip(&studio.scene, &studio.resolved, *id) else { continue };
        let tip = view.to_screen(tip);
        let aim_active = active == Some(Handle::Aim(*id));
        let arrow_color = if aim_active { theme::ACCENT } else { Color32::WHITE };
        painter.line_segment([center, tip], Stroke::new(2.0, arrow_color));
        let dir = (tip - center).normalized();
        if dir.length() > 0.5 {
            let side = egui::vec2(-dir.y, dir.x);
            let back = tip - dir * 11.0;
            painter.add(egui::Shape::convex_polygon(
                vec![tip, back + side * 5.5, back - side * 5.5],
                arrow_color,
                Stroke::NONE,
            ));
        }
        painter.circle(tip, if aim_active { 7.0 } else { 5.5 }, theme::ACCENT, Stroke::new(1.5, Color32::WHITE));
        if aim_active || highlighted {
            let speed =
                studio.resolved.things.get(id).map_or(0.0, |r| BallsSettings::from_params(&r.params).velocity.length());
            painter.text(
                tip + egui::vec2(10.0, -10.0),
                Align2::LEFT_BOTTOM,
                format!("{speed:.0} px/s"),
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
    }
}

/// The dashed outlines of the things that have nothing visible of their own at rest: where an
/// emitter points, what a field covers, where a liquid starts, where a swarm gathers.
fn draw_gizmos(
    painter: &egui::Painter,
    studio: &Studio,
    view: &View,
    active: Option<Handle>,
    selected: Option<ThingId>,
) {
    let world = studio.playback.world();
    for (id, thing) in studio.scene.things.iter().filter(|(_, t)| t.enabled) {
        let Some(r) = studio.resolved.things.get(id) else { continue };
        let highlighted = selected == Some(*id) || active.is_some_and(|h| h.thing() == *id);
        let color = if highlighted { theme::ACCENT } else { theme::MUTED };
        let stroke = Stroke::new(if highlighted { 1.6 } else { 1.0 }, color);
        let dashed = |painter: &egui::Painter, points: &[Pos2]| {
            painter.extend(egui::Shape::dashed_line(points, stroke, 5.0, 4.0));
        };
        for gizmo in gizmos(thing, r, world) {
            match gizmo {
                Gizmo::Dot(at) => {
                    let at = view.to_screen(at);
                    painter.circle_stroke(at, 6.0, stroke);
                    painter.circle_filled(at, 2.0, color);
                }
                Gizmo::Circle(center, radius) => {
                    let points: Vec<Pos2> = (0..=72)
                        .map(|i| {
                            let a = i as f32 / 72.0 * std::f32::consts::TAU;
                            view.to_screen(center + Vec2::new(a.cos(), a.sin()) * radius)
                        })
                        .collect();
                    dashed(painter, &points);
                }
                Gizmo::Rect { center, half, angle } => {
                    let turn = Vec2::from_angle(angle);
                    let corners: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
                        .iter()
                        .map(|&(x, y)| view.to_screen(center + turn.rotate(Vec2::new(x, y) * half)))
                        .collect();
                    dashed(painter, &corners);
                }
                Gizmo::Arrow(from, to) => {
                    let (from, to) = (view.to_screen(from), view.to_screen(to));
                    painter.line_segment([from, to], stroke);
                    let dir = (to - from).normalized();
                    if dir.length() > 0.5 {
                        let side = egui::vec2(-dir.y, dir.x);
                        let back = to - dir * 9.0;
                        painter.add(egui::Shape::convex_polygon(
                            vec![to, back + side * 4.5, back - side * 4.5],
                            color,
                            Stroke::NONE,
                        ));
                    }
                }
                Gizmo::Line(a, b) => dashed(painter, &[view.to_screen(a), view.to_screen(b)]),
            }
        }
    }
}

fn draw_hint(ui: &Ui, rect: Rect, text: &str) {
    let painter = ui.painter_at(rect);
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.5), theme::TEXT);
    let size = galley.size() + egui::vec2(24.0, 10.0);
    if size.x > rect.width() {
        return;
    }
    let pill = Rect::from_center_size(rect.center_bottom() - egui::vec2(0.0, size.y / 2.0 + 10.0), size);
    painter.rect_filled(pill, size.y / 2.0, Color32::from_black_alpha(190));
    painter.galley(pill.min + egui::vec2(12.0, 5.0), galley, theme::TEXT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn hit_testing_finds_balls_before_arenas() {
        let studio = Studio::headless();
        let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, -200.0))), Some(Handle::Body(ThingId(2))));
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(508.0, 0.0))), Some(Handle::Edge(ThingId(1))));
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, 300.0))), None);
        // The arrow tip sits a quarter second of launch speed from the ball.
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(75.0, -250.0))), Some(Handle::Aim(ThingId(2))));
    }

    #[test]
    fn dragging_moves_aims_and_resizes() {
        let mut studio = Studio::headless();
        apply_drag(
            &mut studio.scene,
            Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(50.0, 60.0),
        );
        assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(50.0, 60.0));
        apply_drag(
            &mut studio.scene,
            Drag { handle: Handle::Aim(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(150.0, 60.0),
        );
        assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([400.0, 0.0])));
        apply_drag(
            &mut studio.scene,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(300.0, 400.0),
        );
        assert_eq!(studio.scene.things[&ThingId(1)].params.get("radius"), Some(&Value::Number(500.0)));
    }

    #[test]
    fn a_box_arena_resizes_by_width_and_height() {
        let mut studio = Studio::headless();
        studio.scene.things.get_mut(&ThingId(1)).unwrap().params.set("shape", Value::Choice("box".into()));
        apply_drag(
            &mut studio.scene,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(-300.0, 400.0),
        );
        let params = &studio.scene.things[&ThingId(1)].params;
        assert_eq!(params.get("width"), Some(&Value::Number(600.0)));
        assert_eq!(params.get("height"), Some(&Value::Number(800.0)));
    }

    #[test]
    fn hit_testing_finds_a_field_by_its_gizmo() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let field = bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, "field", ThingId::ROOT).unwrap();
        let thing = studio.scene.things.get_mut(&field).unwrap();
        thing.transform.position = Vec2::new(300.0, 0.0);
        thing.params.set("radius", Value::Number(100.0));
        studio.after_edit(before);
        let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(300.0, 0.0))), Some(Handle::Body(field)));
        // Just past the grab distance there is nothing: a field's circle has no edge handle.
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(300.0, 80.0))), None);
        apply_drag(
            &mut studio.scene,
            Drag { handle: Handle::Body(field), offset: Vec2::ZERO },
            Vec2::new(-100.0, 50.0),
        );
        assert_eq!(studio.scene.things[&field].transform.position, Vec2::new(-100.0, 50.0));
    }
}
