//! The video preview, plus the handles that set a scene up by dragging in it: move a thing (a
//! rider by its offset), aim balls by their arrow tip, and resize the arena by its edge.

use crate::app::{CATCH_UP_BUDGET, Studio};
use crate::grid::{snap_length, snap_point};
use crate::theme;
use bpe_core::{Attach, Resolved, Scene, Settings, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame_at, wgpu};
use bpe_kit::{ArenaSettings, ArenaShape, BallsSettings, GIZMO_GRAB, Gizmo, gizmos};
use bpe_runtime::{Outline, Pose, World};
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;

/// The launch arrow's tip sits this many seconds of launch speed from the balls.
pub const ARROW_SECONDS: f32 = 0.25;

/// How close to an arena's outline, in screen points, counts as grabbing its edge.
const EDGE_GRAB: f32 = 12.0;

/// How close to an arrow tip, in screen points, counts as grabbing it.
const TIP_GRAB: f32 = 10.0;

/// How close to a thing with nothing of its own to show (a folder), in screen points, counts
/// as grabbing it.
const SPOT_GRAB: f32 = 12.0;

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
    revision: u64,
    time: f64,
    /// The video clock, which beats and captions follow; a new mix can move it.
    video: f64,
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
    /// What is wrong with a shader module the picture skipped, when one is.
    pub shader_problem: Option<String>,
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

    let catching_up = draw_picture(
        studio,
        device,
        &render_state.queue,
        &mut preview.renderer,
        &mut preview.frame,
        &mut preview.drawn,
    );
    let mut outcome = Outcome { catching_up, ..Outcome::default() };
    outcome.shader_problem = preview.renderer.shader_problems().into_iter().next();

    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.painter().image(preview.texture, rect, uv, Color32::WHITE);
    studio.grid.draw(&ui.painter_at(rect), rect, rect.center(), rect.width() / video_width as f32);

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
            Handle::Body(id) => place_of(&studio.scene, studio.playback.world(), id).unwrap_or(view.to_world(press)),
            Handle::Aim(_) | Handle::Edge(_) => view.to_world(press),
        };
        preview.drag = Some(Drag { handle, offset: view.to_world(press) - anchor });
        outcome.drag_started = true;
    }
    if response.drag_stopped() {
        preview.drag = None;
    }
    if let (Some(drag), Some(pos)) = (preview.drag, response.interact_pointer_pos()) {
        let step = studio.grid.step(ui.input(|i| i.modifiers.alt));
        apply_drag(&mut studio.scene, studio.playback.world(), drag, view.to_world(pos), step);
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
    draw_handles(ui, studio, &view, active, outcome.selected.or(studio.selection.primary));
    let hint = if studio.grid.snap {
        "Drags snap to the grid · hold Alt to place freely"
    } else {
        "Drag things to move them · drag the arrow tip to aim · drag the arena's edge to resize"
    };
    draw_hint(ui, rect, hint);
    studio.panels.preview = Some(preview);
    outcome
}

/// Draws the playhead's moment into `renderer`, unless `drawn` says the picture already shows
/// it. Returns whether the picture is still catching up with the playhead.
fn draw_picture(
    studio: &mut Studio,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    frame: &mut Frame,
    drawn: &mut Option<Drawn>,
) -> bool {
    // The playhead is in video time; the beat sync's warp says which moment of the run to show.
    let mut catching_up = !studio.playback.seek_within(studio.sim_time(), Some(CATCH_UP_BUDGET));
    // While catching up, the picture shows the moment the simulation has got to.
    let shown_time = if catching_up { studio.playback.time() } else { studio.sim_time() };
    let shown_video = if catching_up { studio.warp.video_time(shown_time) } else { studio.time };
    let now = Drawn {
        revision: studio.revision,
        time: shown_time,
        video: shown_video,
        step: studio.playback.world().step,
        size: renderer.size(),
    };
    if drawn.as_ref() != Some(&now) {
        build_frame_at(
            &studio.scene,
            &studio.resolved,
            studio.playback.world(),
            studio.playback.log(),
            shown_time,
            shown_video,
            &studio.kit.draws,
            frame,
        );
        // Liquids and swarms on the graphics card step towards the playhead within the same
        // budget as the physics; until they are there the picture is drawn again next frame.
        let source = bpe_kit::flow_source(&studio.scene, &studio.resolved, &studio.kit.sims, Some(CATCH_UP_BUDGET));
        let status = renderer.render_with_flow(device, queue, frame, &source);
        if status.caught_up {
            *drawn = Some(now);
        } else {
            *drawn = None;
            catching_up = true;
        }
    }
    catching_up
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
    let angle = outline.angle();
    let u = if offset.length_squared() > 1e-6 { offset.to_angle() - angle } else { 0.0 };
    outline.point(u)
}

/// Where a thing is on screen right now: its pose in the world, or its own position.
fn place_of(scene: &Scene, world: &World, id: ThingId) -> Option<Vec2> {
    world.poses.get(&id).map(|p| p.position).or_else(|| scene.things.get(&id).map(|t| t.transform.position))
}

/// The pose a rider's offset is measured from: its parent's, or one ball of its parent's.
fn parent_pose(scene: &Scene, world: &World, parent: ThingId, item: Option<u32>) -> Pose {
    if let Some(i) = item
        && let Some(b) = world.bodies.get(&parent)
        && (i as usize) < b.len()
    {
        return Pose { position: b.position[i as usize], ..Pose::default() };
    }
    world
        .poses
        .get(&parent)
        .copied()
        .or_else(|| scene.things.get(&parent).map(|t| Pose::from_transform(&t.transform)))
        .unwrap_or_default()
}

/// What is under `pos`: an arrow tip first, then a ball, then an obstacle, then a gizmo's
/// middle, then a selected thing with nothing of its own to show, then an arena's edge.
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
    for (id, st) in world.states.iter().filter(|(id, _)| shown(id)) {
        let Some(soft) = st.as_any().downcast_ref::<bpe_kit::SoftState>() else { continue };
        for point in &soft.world.points {
            let grab = (point.radius * 1.5).max(8.0 / view.scale);
            let distance = (point.position - p).length();
            if distance <= grab && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, *id));
            }
        }
    }
    if let Some((_, id)) = best {
        return Some(Handle::Body(id));
    }
    for (id, solids) in world.solids.iter().filter(|(id, _)| shown(id)) {
        // Each part's own circle, so the gaps of a peg grid or the inside of a ring stay free.
        let grab = SPOT_GRAB / view.scale;
        let on_part = solids.bounds.iter().any(|(center, bound)| (*center - p).length() <= bound + grab);
        if on_part || (solids.pose.position - p).length() <= grab {
            return Some(Handle::Body(*id));
        }
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
    if let Some(id) = studio.selection.primary.filter(|id| shown(id) && spot_only(world, *id))
        && let Some(place) = place_of(&studio.scene, world, id)
        && (view.to_screen(place) - pos).length() <= SPOT_GRAB
    {
        return Some(Handle::Body(id));
    }
    for (id, wall) in world.walls.iter().filter(|(id, _)| shown(id)) {
        let edge = outline_point_towards(&wall.outline, p);
        if (edge - p).length() * view.scale <= EDGE_GRAB {
            return Some(Handle::Edge(*id));
        }
    }
    None
}

/// A thing with no balls, wall or solids of its own: a folder, or an empty obstacle.
fn spot_only(world: &World, id: ThingId) -> bool {
    !world.bodies.contains_key(&id) && !world.walls.contains_key(&id) && !world.solids.contains_key(&id)
}

/// Moves what `drag` holds to the pointer at `pointer`, in world pixels. A rider moves by its
/// offset from the parent it rides, so it stays a rider; its own position is left alone.
/// Moves, aims or resizes by a drag to `pointer`. With a grid `step`, the thing's middle, the
/// arrow tip and the arena's size land on the grid.
pub fn apply_drag(scene: &mut Scene, world: &World, drag: Drag, pointer: Vec2, step: Option<f32>) {
    let id = drag.handle.thing();
    let place = snap_point(pointer - drag.offset, step);
    if let Handle::Body(_) = drag.handle
        && let Some(thing) = scene.things.get(&id)
        && let (Some(parent), Attach::Ride { turn_with_parent, item, .. }) = (thing.parent, thing.attach)
    {
        let anchor = parent_pose(scene, world, parent, item);
        let mut offset = place - anchor.position;
        if turn_with_parent {
            offset = Vec2::from_angle(-anchor.angle).rotate(offset);
        }
        let thing = scene.things.get_mut(&id).expect("looked up above");
        thing.attach = Attach::Ride { offset: offset.round(), turn_with_parent, item };
        return;
    }
    let Some(thing) = scene.things.get_mut(&id) else { return };
    match drag.handle {
        Handle::Body(_) => thing.transform.position = place.round(),
        Handle::Aim(_) => {
            let tip = snap_point(pointer, step);
            let velocity = ((tip - thing.transform.position) / ARROW_SECONDS).round();
            thing.params.set("velocity", Value::Vec2([velocity.x, velocity.y]));
        }
        Handle::Edge(_) => {
            let settings = ArenaSettings::from_params(&thing.params);
            let offset = pointer - thing.transform.position;
            match settings.shape {
                ArenaShape::Box | ArenaShape::Capsule => {
                    let local = Vec2::from_angle(-thing.transform.rotation).rotate(offset);
                    let (half_width, half_height) =
                        (snap_length(local.x.abs(), step), snap_length(local.y.abs(), step));
                    thing.params.set("width", Value::Number((half_width * 2.0).round().max(20.0) as f64));
                    thing.params.set("height", Value::Number((half_height * 2.0).round().max(20.0) as f64));
                }
                ArenaShape::Circle | ArenaShape::Polygon => {
                    let radius = snap_length(offset.length(), step);
                    thing.params.set("radius", Value::Number(radius.round().max(20.0) as f64));
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

    // Obstacles: a ring at their place, like the balls below.
    for (id, solids) in world.solids.iter().filter(|(id, _)| shown(id)) {
        let highlighted = selected == Some(*id) || active.is_some_and(|h| h.thing() == *id);
        let color = if highlighted { theme::ACCENT } else { faint };
        painter.circle_stroke(view.to_screen(solids.pose.position), 6.0, Stroke::new(1.2, color));
    }
    // A selected thing with nothing of its own to show, such as a folder: a dashed ring and its
    // name, so it can still be found and dragged.
    if let Some(id) = selected.filter(|id| shown(id) && spot_only(world, *id))
        && let Some(thing) = studio.scene.things.get(&id)
        && let Some(place) = place_of(&studio.scene, world, id)
    {
        let center = view.to_screen(place);
        let color = if active.is_some_and(|h| h.thing() == id) { theme::ACCENT } else { Color32::WHITE };
        let ring: Vec<Pos2> =
            (0..=48).map(|i| center + egui::Vec2::angled(i as f32 / 48.0 * std::f32::consts::TAU) * 24.0).collect();
        painter.add(egui::Shape::dashed_line(&ring, Stroke::new(1.2, color), 4.0, 4.0));
        painter.text(
            center + egui::vec2(0.0, 30.0),
            Align2::CENTER_TOP,
            &thing.name,
            FontId::proportional(12.0),
            color,
        );
    }

    for (id, thing) in studio.scene.things.iter().filter(|(id, t)| t.enabled && world.bodies.contains_key(id)) {
        let highlighted = selected == Some(*id) || active.is_some_and(|h| h.thing() == *id);
        let color = if highlighted { theme::ACCENT } else { faint };
        let center = view.to_screen(place_of(&studio.scene, world, *id).unwrap_or(thing.transform.position));
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

    /// The picture the preview shows at the playhead once it has caught up, as RGBA bytes.
    fn shown(
        studio: &mut Studio,
        gpu: &(wgpu::Device, wgpu::Queue),
        renderer: &mut Renderer,
        drawn: &mut Option<Drawn>,
    ) -> Vec<u8> {
        let mut frame = Frame::default();
        for _ in 0..200 {
            if !draw_picture(studio, &gpu.0, &gpu.1, renderer, &mut frame, drawn) {
                break;
            }
        }
        renderer.read_output(&gpu.0, &gpu.1).unwrap()
    }

    /// Runs one editor frame around `edit` the way the panels do: the tree and the inspector
    /// change the scene before the preview is drawn, and the edit is resolved at the frame's
    /// end. Then the next frame draws. Returns that picture and a clean render of the edited
    /// scene, which it must match.
    fn edit_then_draw(edit: impl Fn(&mut Scene)) -> Option<(Vec<u8>, Vec<u8>)> {
        let gpu = bpe_gfx::headless_device().ok()?;
        let mut studio = Studio::headless();
        studio.seek(1.0);
        let mut renderer = Renderer::new(&gpu.0, 135, 240);
        let mut drawn = None;
        shown(&mut studio, &gpu, &mut renderer, &mut drawn);

        let before = studio.scene.clone();
        edit(&mut studio.scene);
        draw_picture(&mut studio, &gpu.0, &gpu.1, &mut renderer, &mut Frame::default(), &mut drawn);
        studio.after_edit(before);
        let after = shown(&mut studio, &gpu, &mut renderer, &mut drawn);

        let mut clean = Studio::headless();
        let scene = studio.scene.clone();
        clean.replace_scene(scene, None);
        clean.seek(1.0);
        let mut clean_renderer = Renderer::new(&gpu.0, 135, 240);
        let expected = shown(&mut clean, &gpu, &mut clean_renderer, &mut None);
        Some((after, expected))
    }

    #[test]
    fn moving_the_arena_in_the_inspector_redraws_it_where_it_went() {
        let Some((after, expected)) = edit_then_draw(|scene| {
            scene.things.get_mut(&ThingId(1)).unwrap().params.set("shape", Value::Choice("box".into()));
        }) else {
            return eprintln!("no GPU; skipped");
        };
        assert!(after == expected, "the preview still shows the arena from before the edit");
    }

    #[test]
    fn hiding_a_thing_in_the_tree_takes_it_out_of_the_picture() {
        let Some((after, expected)) = edit_then_draw(|scene| {
            scene.things.get_mut(&ThingId(2)).unwrap().enabled = false;
        }) else {
            return eprintln!("no GPU; skipped");
        };
        assert!(after == expected, "the preview still shows the hidden balls");
    }

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
    fn hit_testing_grabs_a_soft_body_by_its_points() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let soft = studio.scene.add_thing(&studio.kit.registry, "soft", ThingId::ROOT).unwrap();
        studio.scene.things.get_mut(&soft).unwrap().transform.position = Vec2::new(0.0, 300.0);
        studio.after_edit(before);
        let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
        // The top of the jelly's skin, 120 px above its middle.
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, 182.0))), Some(Handle::Body(soft)));
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, 300.0))), None, "the hollow middle");
        apply_drag(
            &mut studio.scene,
            studio.playback.world(),
            Drag { handle: Handle::Body(soft), offset: Vec2::ZERO },
            Vec2::new(10.0, 20.0),
            None,
        );
        assert_eq!(studio.scene.things[&soft].transform.position, Vec2::new(10.0, 20.0));
    }

    #[test]
    fn dragging_moves_aims_and_resizes() {
        let mut studio = Studio::headless();
        let world = studio.playback.world().clone();
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(50.0, 60.0),
            None,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(50.0, 60.0));
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Aim(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(150.0, 60.0),
            None,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([400.0, 0.0])));
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(300.0, 400.0),
            None,
        );
        assert_eq!(studio.scene.things[&ThingId(1)].params.get("radius"), Some(&Value::Number(500.0)));
    }

    #[test]
    fn dragging_a_rider_moves_its_offset_and_obstacles_are_grabbable() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let peg = studio.scene.add_thing(&studio.kit.registry, "obstacle", ThingId::ROOT).unwrap();
        studio.scene.things.get_mut(&peg).unwrap().transform.position = Vec2::new(100.0, 100.0);
        let ride = Attach::Ride { offset: Vec2::new(0.0, -200.0), turn_with_parent: true, item: None };
        studio.scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        studio.after_edit(before);
        let view = View { rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)), scale: 0.25 };
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(100.0, 100.0))), Some(Handle::Body(peg)));
        let world = studio.playback.world().clone();
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(30.0, -100.0),
            None,
        );
        let thing = &studio.scene.things[&ThingId(2)];
        assert!(
            matches!(thing.attach, Attach::Ride { offset, .. } if offset == Vec2::new(30.0, -100.0)),
            "{:?}",
            thing.attach
        );
        assert_eq!(thing.transform.position, Vec2::new(0.0, -200.0), "the own position is left alone");
        // A folder has nothing to show, so only the selected one can be grabbed, at its spot.
        let before = studio.scene.clone();
        let folder = studio.scene.add_thing(&studio.kit.registry, "folder", ThingId::ROOT).unwrap();
        studio.scene.things.get_mut(&folder).unwrap().transform.position = Vec2::new(-300.0, 300.0);
        studio.after_edit(before);
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(-300.0, 300.0))), None);
        studio.selection.set(folder);
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(-300.0, 300.0))), Some(Handle::Body(folder)));
    }

    #[test]
    fn a_box_arena_resizes_by_width_and_height() {
        let mut studio = Studio::headless();
        let world = studio.playback.world().clone();
        studio.scene.things.get_mut(&ThingId(1)).unwrap().params.set("shape", Value::Choice("box".into()));
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(-300.0, 400.0),
            None,
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
            studio.playback.world(),
            Drag { handle: Handle::Body(field), offset: Vec2::ZERO },
            Vec2::new(-100.0, 50.0),
            None,
        );
        assert_eq!(studio.scene.things[&field].transform.position, Vec2::new(-100.0, 50.0));
    }

    #[test]
    fn with_a_grid_drags_land_on_it() {
        let mut studio = Studio::headless();
        let world = studio.playback.world().clone();
        let step = Some(60.0);
        // Grabbed 7 px off the middle: the middle still lands on a grid point.
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::new(7.0, -3.0) },
            Vec2::new(85.0, 50.0),
            step,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(60.0, 60.0));
        // The arrow tip lands on a grid point: 60 px right of the ball over a quarter second.
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Aim(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(131.0, 52.0),
            step,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([240.0, 0.0])));
        // A circle's radius grows in whole steps.
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(0.0, 455.0),
            step,
        );
        assert_eq!(studio.scene.things[&ThingId(1)].params.get("radius"), Some(&Value::Number(480.0)));
        // A centred box's edges land on grid lines.
        studio.scene.things.get_mut(&ThingId(1)).unwrap().params.set("shape", Value::Choice("box".into()));
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Edge(ThingId(1)), offset: Vec2::ZERO },
            Vec2::new(-310.0, 395.0),
            step,
        );
        let params = &studio.scene.things[&ThingId(1)].params;
        assert_eq!(params.get("width"), Some(&Value::Number(600.0)));
        assert_eq!(params.get("height"), Some(&Value::Number(840.0)));
    }

    #[test]
    fn a_snapped_rider_lands_on_the_grid_in_the_world() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId(1)).unwrap().transform.position = Vec2::new(15.0, 0.0);
        let ride = Attach::Ride { offset: Vec2::new(0.0, -200.0), turn_with_parent: false, item: None };
        studio.scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        studio.after_edit(before);
        let world = studio.playback.world().clone();
        apply_drag(
            &mut studio.scene,
            &world,
            Drag { handle: Handle::Body(ThingId(2)), offset: Vec2::ZERO },
            Vec2::new(70.0, -110.0),
            Some(60.0),
        );
        let thing = &studio.scene.things[&ThingId(2)];
        assert!(
            matches!(thing.attach, Attach::Ride { offset, .. } if offset == Vec2::new(45.0, -120.0)),
            "the world spot (60, -120) less the arena at (15, 0): {:?}",
            thing.attach
        );
    }
}
