//! The video preview, plus the handles that let you set up a scene by dragging in it:
//! move a ball group, aim it by its arrow tip, and resize the arena by its edge.

use crate::inspector::ARROW_SECONDS;
use crate::theme;
use bpe_render::{EffectLibrary, FrameData, Renderer};
use bpe_scene::{Container, Scene, Spawner, Vec2};
use bpe_sim::{ArenaState, Outline, Playback};
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};

pub struct Preview {
    renderer: Renderer,
    texture: egui::TextureId,
    frame_data: FrameData,
    drag: Option<Drag>,
    /// What the picture on screen was drawn from, so it is only drawn again when that changes.
    drawn: Option<Drawn>,
    /// How long the parts of the last frame took.
    pub stats: Stats,
}

#[derive(PartialEq)]
struct Drawn {
    scene: Scene,
    time: f64,
    step_time: f64,
    size: (u32, u32),
    effects: u64,
}

/// How long the parts of a preview frame took, in milliseconds.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub simulate: f32,
    pub build: f32,
    pub draw: f32,
    /// Whether the picture was drawn this frame, or the last one was still current.
    pub drew: bool,
    pub balls: usize,
    /// Extra lines and shapes sent to the graphics card.
    pub shapes: usize,
}

/// Most time spent catching the simulation up in one editor frame. A long stretch, such as
/// after an edit with the playhead far in, is worked out over several frames instead of
/// freezing the editor.
const CATCH_UP_BUDGET: std::time::Duration = std::time::Duration::from_millis(14);

/// How sharp the preview is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewQuality {
    #[default]
    Full,
    Half,
    Quarter,
}

impl PreviewQuality {
    pub const ALL: [PreviewQuality; 3] = [PreviewQuality::Full, PreviewQuality::Half, PreviewQuality::Quarter];

    pub fn label(self) -> &'static str {
        match self {
            PreviewQuality::Full => "Full",
            PreviewQuality::Half => "Half",
            PreviewQuality::Quarter => "Quarter",
        }
    }

    fn scale(self) -> f32 {
        match self {
            PreviewQuality::Full => 1.0,
            PreviewQuality::Half => 0.5,
            PreviewQuality::Quarter => 0.25,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    /// Move a ball group.
    Body(usize),
    /// Aim a ball group by its launch arrow.
    Aim(usize),
    /// Resize the arena. For a box, which sides are being dragged.
    Edge { x: bool, y: bool },
}

#[derive(Clone, Copy)]
struct Drag {
    handle: Handle,
    /// Pointer position minus the dragged thing's position, in world pixels.
    offset: Vec2,
}

/// What happened in the preview this frame, for the app to act on.
#[derive(Default)]
pub struct Outcome {
    /// A drag began: the app pauses and jumps to the start so the starting layout is visible.
    pub drag_started: bool,
    /// The preview was clicked while playing.
    pub clicked: bool,
    /// A ball group was clicked or dragged.
    pub selected: Option<usize>,
    /// The simulation hasn't reached the playhead yet, and the picture shows an earlier moment.
    pub catching_up: bool,
}

/// Converts between world pixels (origin at the video centre, y down) and screen points.
struct View {
    rect: Rect,
    scale: f32,
}

impl View {
    fn to_screen(&self, p: Vec2) -> Pos2 {
        self.rect.center() + egui::vec2(p.x, p.y) * self.scale
    }

    fn to_world(&self, p: Pos2) -> Vec2 {
        let d = (p - self.rect.center()) / self.scale;
        Vec2::new(d.x, d.y)
    }
}

impl Preview {
    /// Effects the GPU refused even though they passed the shader check, as (id, message).
    pub fn effect_gpu_errors(&self) -> &[(String, String)] {
        self.renderer.effect_gpu_errors()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        slot: &mut Option<Self>,
        ui: &mut Ui,
        frame: &eframe::Frame,
        scene: &mut Scene,
        playback: &mut Playback,
        time: f64,
        playing: bool,
        selected: Option<usize>,
        effects: &EffectLibrary,
        quality: PreviewQuality,
    ) -> Outcome {
        let Some(render_state) = frame.wgpu_render_state() else {
            ui.label("The preview needs the wgpu renderer.");
            return Outcome::default();
        };
        let video = &scene.video;
        let aspect = video.width as f32 / video.height.max(1) as f32;
        let area = ui.available_rect_before_wrap().shrink(12.0);
        let mut size = egui::vec2(area.height() * aspect, area.height());
        if size.x > area.width() {
            size = egui::vec2(area.width(), area.width() / aspect);
        }
        let rect = Rect::from_center_size(area.center(), size);
        // Render at the screen's real pixel size, but never above the video's own size.
        let scale = (size.x * ui.ctx().pixels_per_point() / video.width as f32).min(1.0) * quality.scale();
        let (width, height) = bpe_export::output_size(scene, scale);

        let device = &render_state.device;
        let preview = slot.get_or_insert_with(|| {
            let mut renderer = Renderer::new(device, width, height);
            renderer.set_effect_library(effects.clone());
            let texture = render_state.renderer.write().register_native_texture(
                device,
                renderer.output_view(),
                bpe_render::wgpu::FilterMode::Linear,
            );
            Preview {
                renderer,
                texture,
                frame_data: FrameData::default(),
                drag: None,
                drawn: None,
                stats: Stats::default(),
            }
        });
        if preview.renderer.size() != (width, height) {
            preview.renderer.resize(device, width, height);
            render_state.renderer.write().update_egui_texture_from_wgpu_texture(
                device,
                preview.renderer.output_view(),
                bpe_render::wgpu::FilterMode::Linear,
                preview.texture,
            );
            preview.drawn = None;
        }

        let mut outcome = Outcome::default();
        let started = std::time::Instant::now();
        outcome.catching_up = !playback.seek_within(time, Some(CATCH_UP_BUDGET));
        let simulate = started.elapsed();
        // While catching up, the picture shows the moment the simulation has got to.
        let shown_time = if outcome.catching_up { playback.simulation().time() } else { time };
        let drawn = Drawn {
            scene: scene.clone(),
            time: shown_time,
            step_time: playback.simulation().time(),
            size: (width, height),
            effects: effects.revision(),
        };
        let stats = &mut preview.stats;
        stats.simulate = simulate.as_secs_f32() * 1000.0;
        stats.drew = preview.drawn.as_ref() != Some(&drawn);
        if stats.drew {
            let started = std::time::Instant::now();
            let pixel = preview.renderer.pixel_size(scene);
            preview.frame_data.fill(scene, playback, pixel);
            stats.build = started.elapsed().as_secs_f32() * 1000.0;
            let started = std::time::Instant::now();
            preview.renderer.render(device, &render_state.queue, scene, shown_time, &preview.frame_data);
            stats.draw = started.elapsed().as_secs_f32() * 1000.0;
            stats.balls = preview.frame_data.balls.len();
            stats.shapes = preview.frame_data.shapes.len() + preview.frame_data.trails.len() / 6;
            preview.drawn = Some(drawn);
        } else {
            (stats.build, stats.draw) = (0.0, 0.0);
        }

        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter().image(preview.texture, rect, uv, Color32::WHITE);

        if playing {
            preview.drag = None;
            outcome.clicked = response.clicked();
            return outcome;
        }
        let view = View { rect, scale: rect.width() / scene.video.width as f32 };
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let hovered = pointer.and_then(|p| hit_test(scene, &view, p));

        if response.drag_started()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && let Some(handle) = hit_test(scene, &view, press)
        {
            let anchor = match handle {
                Handle::Body(i) => spawner_center(&scene.spawners[i]),
                Handle::Aim(_) | Handle::Edge { .. } => view.to_world(press),
            };
            preview.drag = Some(Drag { handle, offset: view.to_world(press) - anchor });
            outcome.drag_started = true;
        }
        if response.drag_stopped() {
            preview.drag = None;
        }
        if let (Some(drag), Some(pos)) = (preview.drag, response.interact_pointer_pos()) {
            apply_drag(scene, drag, view.to_world(pos));
            if let Handle::Body(i) | Handle::Aim(i) = drag.handle {
                outcome.selected = Some(i);
            }
        } else if response.clicked()
            && let Some(Handle::Body(i) | Handle::Aim(i)) = hovered
        {
            outcome.selected = Some(i);
        }

        let active = preview.drag.map(|d| d.handle).or(hovered);
        if active.is_some() {
            ui.ctx().set_cursor_icon(if preview.drag.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab });
        }
        draw_handles(ui, scene, &view, active, outcome.selected.or(selected));
        draw_hint(ui, rect, "Drag balls to move them · drag arrow tips to aim · drag the edge to resize");
        outcome
    }
}

fn spawner_center(spawner: &Spawner) -> Vec2 {
    match spawner {
        Spawner::Grid { center, .. } => *center,
        Spawner::Single { position, .. } | Spawner::Stream { position, .. } => *position,
    }
}

fn spawner_velocity(spawner: &Spawner) -> Vec2 {
    match spawner {
        Spawner::Grid { velocity, .. } | Spawner::Single { velocity, .. } | Spawner::Stream { velocity, .. } => {
            *velocity
        }
    }
}

/// The round or polygon outline that the arena's edge handle resizes: the arena itself, or the
/// smallest of the escape rings. None for a box, which has its own handles.
fn edge_outline(scene: &Scene) -> Option<Outline> {
    let container = &scene.container;
    let start = ArenaState { scale: arena_size(scene), ..ArenaState::default() };
    match container {
        Container::Rect { .. } => None,
        Container::Rings { .. } => bpe_sim::ring_outline(container, 0, 0.0, start),
        _ => bpe_sim::closed_outline(container, start),
    }
}

/// How big the arena is drawn compared to its shape's own size.
fn arena_size(scene: &Scene) -> f32 {
    scene.arena_motion.size.max(0.01)
}

/// The point on a ring or polygon outline in the direction of `world` from its centre.
fn outline_point_towards(outline: &Outline, world: Vec2) -> Vec2 {
    let Outline::Ring { center, angle, .. } = *outline else { return outline.center() };
    let d = world - center;
    outline.point(d.y.atan2(d.x) - angle)
}

/// Half the width and height of a ball group, in world pixels.
fn spawner_half_size(spawner: &Spawner) -> Vec2 {
    match spawner {
        Spawner::Grid { columns, rows, spacing, radius, .. } => Vec2::new(
            (columns.saturating_sub(1)) as f32 * spacing / 2.0 + radius,
            (rows.saturating_sub(1)) as f32 * spacing / 2.0 + radius,
        ),
        Spawner::Single { radius, .. } | Spawner::Stream { radius, .. } => Vec2::splat(*radius),
    }
}

fn arrow_tip(spawner: &Spawner) -> Vec2 {
    spawner_center(spawner) + spawner_velocity(spawner) * ARROW_SECONDS
}

/// Screen points within which a handle can be grabbed.
const GRAB: f32 = 10.0;

fn hit_test(scene: &Scene, view: &View, pos: Pos2) -> Option<Handle> {
    let world = view.to_world(pos);
    let reach = GRAB / view.scale;
    for (i, spawner) in scene.spawners.iter().enumerate().rev() {
        if view.to_screen(arrow_tip(spawner)).distance(pos) <= GRAB + 2.0 {
            return Some(Handle::Aim(i));
        }
    }
    if let Some(outline) = edge_outline(scene) {
        let edge = outline_point_towards(&outline, world);
        if edge.distance(world) <= reach {
            return Some(Handle::Edge { x: true, y: true });
        }
    }
    match scene.container {
        Container::Circle { .. } | Container::Polygon { .. } | Container::Rings { .. } | Container::Custom { .. } => {}
        Container::Rect { center, size, .. } => {
            let d = (world - center).abs();
            let half = size * arena_size(scene) / 2.0;
            let x = (d.x - half.x).abs() <= reach && d.y <= half.y + reach;
            let y = (d.y - half.y).abs() <= reach && d.x <= half.x + reach;
            if x || y {
                return Some(Handle::Edge { x, y });
            }
        }
    }
    for (i, spawner) in scene.spawners.iter().enumerate().rev() {
        let d = (world - spawner_center(spawner)).abs();
        let half = spawner_half_size(spawner).max(Vec2::splat(reach));
        let inside = match spawner {
            Spawner::Grid { .. } => d.x <= half.x && d.y <= half.y,
            Spawner::Single { .. } | Spawner::Stream { .. } => d.length() <= half.x,
        };
        if inside {
            return Some(Handle::Body(i));
        }
    }
    None
}

fn apply_drag(scene: &mut Scene, drag: Drag, world: Vec2) {
    match drag.handle {
        Handle::Body(i) => {
            let target = (world - drag.offset).round();
            match &mut scene.spawners[i] {
                Spawner::Grid { center, .. } => *center = target,
                Spawner::Single { position, .. } | Spawner::Stream { position, .. } => *position = target,
            }
        }
        Handle::Aim(i) => {
            let spawner = &mut scene.spawners[i];
            let center = spawner_center(spawner);
            let velocity = ((world - center) / ARROW_SECONDS).clamp_length_max(3000.0).round();
            match spawner {
                Spawner::Grid { velocity: v, .. }
                | Spawner::Single { velocity: v, .. }
                | Spawner::Stream { velocity: v, .. } => *v = velocity,
            }
        }
        Handle::Edge { x, y } => {
            // Scale the outline so it passes under the pointer.
            let shown_size = arena_size(scene);
            let scale = edge_outline(scene).map(|outline| {
                let edge = outline_point_towards(&outline, world);
                let center = outline.center();
                (world - center).length() / (edge - center).length().max(1.0)
            });
            match &mut scene.container {
                Container::Circle { radius, .. } | Container::Polygon { radius, .. } => {
                    *radius = (*radius * scale.unwrap_or(1.0)).clamp(20.0, 1200.0).round()
                }
                Container::Rings { inner_radius, .. } => {
                    *inner_radius = (*inner_radius * scale.unwrap_or(1.0)).clamp(10.0, 1000.0).round()
                }
                Container::Custom { .. } => {}
                Container::Rect { center, size, .. } => {
                    let d = ((world - *center).abs() * 2.0 / shown_size)
                        .clamp(Vec2::splat(40.0), Vec2::splat(2400.0))
                        .round();
                    if x {
                        size.x = d.x;
                    }
                    if y {
                        size.y = d.y;
                    }
                }
            }
        }
    }
}

fn draw_handles(ui: &Ui, scene: &Scene, view: &View, active: Option<Handle>, selected: Option<usize>) {
    let painter = ui.painter_at(view.rect);
    let faint = Color32::from_white_alpha(110);

    // Arena edge.
    let edge_active = matches!(active, Some(Handle::Edge { .. }));
    let edge_stroke = Stroke::new(if edge_active { 2.5 } else { 1.0 }, if edge_active { theme::ACCENT } else { faint });
    let edge_dot = match (&scene.container, edge_outline(scene)) {
        (Container::Rect { center, size, .. }, _) => {
            let size = *size * arena_size(scene);
            if edge_active {
                let r = Rect::from_center_size(view.to_screen(*center), egui::vec2(size.x, size.y) * view.scale);
                painter.rect_stroke(r, 0.0, edge_stroke, egui::StrokeKind::Middle);
            }
            Some(view.to_screen(*center + size / 2.0))
        }
        (_, Some(outline)) => {
            let center = outline.center();
            if edge_active {
                let points: Vec<Pos2> = (0..=96)
                    .map(|i| {
                        let towards = center + Vec2::from_angle(i as f32 / 96.0 * std::f32::consts::TAU);
                        view.to_screen(outline_point_towards(&outline, towards))
                    })
                    .collect();
                painter.add(egui::Shape::line(points, edge_stroke));
            }
            Some(view.to_screen(outline_point_towards(&outline, center + Vec2::X)))
        }
        (_, None) => None,
    };
    if let Some(edge_dot) = edge_dot {
        painter.circle(
            edge_dot,
            5.0,
            if edge_active { theme::ACCENT } else { theme::TEXT },
            Stroke::new(1.5, Color32::BLACK),
        );
    }

    for (i, spawner) in scene.spawners.iter().enumerate() {
        let highlighted = selected == Some(i) || matches!(active, Some(Handle::Body(j) | Handle::Aim(j)) if j == i);
        let color = if highlighted { theme::ACCENT } else { faint };
        let center = view.to_screen(spawner_center(spawner));
        let half = spawner_half_size(spawner) * view.scale + Vec2::splat(4.0);
        match spawner {
            Spawner::Grid { .. } => {
                let r = Rect::from_center_size(center, egui::vec2(half.x, half.y) * 2.0);
                let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.2, color), 5.0, 4.0));
            }
            Spawner::Single { .. } => {
                painter.circle_stroke(center, half.x, Stroke::new(1.2, color));
            }
            Spawner::Stream { .. } => {
                painter.circle_stroke(center, half.x, Stroke::new(1.2, color));
                painter.circle_stroke(center, half.x + 5.0, Stroke::new(1.0, color.gamma_multiply(0.6)));
            }
        }

        // Launch arrow.
        let tip = view.to_screen(arrow_tip(spawner));
        let aim_active = active == Some(Handle::Aim(i));
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
            let speed = spawner_velocity(spawner).length();
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
