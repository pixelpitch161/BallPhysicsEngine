//! The video preview, plus the handles that let you set up a scene by dragging in it:
//! move a ball group or soft thing, aim it by its arrow tip, and resize the arena by its edge.

use crate::inspector::ARROW_SECONDS;
use crate::inspector::soft::SoftPick;
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
    /// 3D (WS4): whether the playback is 3D, so the frame after switching Flat/3D is drawn again.
    space: bool,
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

/// Something in the scene that can be picked and moved in the preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A ball group, by its place in the Balls tab.
    Balls(usize),
    /// Jelly balls, a rope, cloth or the liquid.
    Soft(SoftPick),
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    /// Move a ball group or soft thing.
    Body(Pick),
    /// Aim it by its launch arrow.
    Aim(Pick),
    /// Move one end of a rope: the start, or the end when `end` is true.
    RopeEnd { rope: usize, end: bool },
    /// Resize the arena. For a box, which sides are being dragged.
    Edge { x: bool, y: bool },
}

impl Handle {
    fn pick(self) -> Option<Pick> {
        match self {
            Handle::Body(pick) | Handle::Aim(pick) => Some(pick),
            Handle::RopeEnd { rope, .. } => Some(Pick::Soft(SoftPick::Rope(rope))),
            Handle::Edge { .. } => None,
        }
    }
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
    /// A ball group or soft thing was clicked or dragged.
    pub selected: Option<Pick>,
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
    /// (WS5) The finished soundtrack's analysis, for visuals that pulse with the mix.
    pub fn set_mix(&mut self, mix: Option<std::sync::Arc<bpe_music::AudioFeatures>>) {
        let same = match (&mix, &self.frame_data.mix) {
            (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.frame_data.mix = mix;
            self.drawn = None;
        }
    }

    /// Effects the GPU refused even though they passed the shader check, as (id, message).
    pub fn effect_gpu_errors(&self) -> &[(String, String)] {
        self.renderer.effect_gpu_errors()
    }

    /// Caption font files that couldn't be read, as (path, reason).
    pub fn font_errors(&self) -> &[(String, String)] {
        self.renderer.font_errors()
    }

    /// Sets the folder captions' relative font paths are looked up in.
    pub fn set_font_folder(&mut self, folder: Option<std::path::PathBuf>) {
        self.renderer.set_font_folder(folder);
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
        selected: Option<Pick>,
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
            // Editing an effect shows its last version until the new one is built.
            renderer.set_effect_background_builds(true);
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
            space: playback.space().is_some(),
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
            preview.frame_data.gpu.budget = Some(CATCH_UP_BUDGET);
            preview.renderer.render(device, &render_state.queue, scene, shown_time, &preview.frame_data);
            stats.draw = started.elapsed().as_secs_f32() * 1000.0;
            // While the swarm or liquid is still catching up, draw this moment again next frame.
            outcome.catching_up |= preview.frame_data.gpu.behind();
            stats.balls = preview.frame_data.balls.len();
            stats.shapes = preview.frame_data.shapes.len() + preview.frame_data.trails.len() / 6;
            // A changed effect still building elsewhere: draw again once it's ready.
            let building = preview.renderer.effects_building();
            if building {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            }
            preview.drawn = (!preview.frame_data.gpu.behind() && !building).then_some(drawn);
        } else {
            (stats.build, stats.draw) = (0.0, 0.0);
        }

        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        ui.painter().image(preview.texture, rect, uv, Color32::WHITE);

        // ---- 3D (WS4): dragging turns the camera instead of moving balls. ----
        if crate::orbit::interact(ui, &response, scene, rect) {
            preview.drag = None;
            outcome.clicked = playing && response.clicked();
            draw_hint(ui, rect, crate::orbit::HINT);
            return outcome;
        }
        if playing {
            preview.drag = None;
            outcome.clicked = response.clicked();
            return outcome;
        }
        let view = View { rect, scale: rect.width() / scene.video.width as f32 };
        if crate::draw_shape::active(ui.ctx()) {
            // Drawing a custom shape takes over the preview's clicks until it is done.
            preview.drag = None;
            crate::draw_shape::interact(ui, &response, scene, |p| view.to_world(p), |w| view.to_screen(w));
            return outcome;
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let hovered = pointer.and_then(|p| hit_test(scene, &view, p));

        if response.drag_started()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && let Some(handle) = hit_test(scene, &view, press)
        {
            let anchor = match handle {
                Handle::Body(pick) => pick_center(scene, pick).unwrap_or_else(|| view.to_world(press)),
                Handle::RopeEnd { rope, end } => {
                    let rope = &scene.bodies.ropes[rope];
                    if end { rope.to } else { rope.from }
                }
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
            outcome.selected = drag.handle.pick();
        } else if response.clicked()
            && let Some(pick) = hovered.and_then(Handle::pick)
        {
            outcome.selected = Some(pick);
        }

        let active = preview.drag.map(|d| d.handle).or(hovered);
        if active.is_some() {
            ui.ctx().set_cursor_icon(if preview.drag.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab });
        }
        draw_handles(ui, scene, &view, active, outcome.selected.or(selected));
        draw_hint(
            ui,
            rect,
            "Drag balls and soft things to move them · drag arrow tips to aim · drag the edge to resize",
        );
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

/// Every ball group and soft thing, in the order their handles are drawn.
fn picks(scene: &Scene) -> impl Iterator<Item = Pick> + '_ {
    let bodies = &scene.bodies;
    let soft = (0..bodies.cloths.len())
        .map(SoftPick::Cloth)
        .chain((bodies.fluid.count > 0).then_some(SoftPick::Liquid))
        .chain((0..bodies.ropes.len()).map(SoftPick::Rope))
        .chain((0..bodies.jelly.len()).map(SoftPick::Jelly))
        .map(Pick::Soft);
    soft.chain((0..scene.spawners.len()).map(Pick::Balls))
}

/// The middle of a ball group or soft thing, or None if it is gone.
fn pick_center(scene: &Scene, pick: Pick) -> Option<Vec2> {
    let bodies = &scene.bodies;
    Some(match pick {
        Pick::Balls(i) => spawner_center(scene.spawners.get(i)?),
        Pick::Soft(SoftPick::Jelly(i)) => bodies.jelly.get(i)?.center,
        Pick::Soft(SoftPick::Rope(i)) => {
            let rope = bodies.ropes.get(i)?;
            (rope.from + rope.to) / 2.0
        }
        Pick::Soft(SoftPick::Cloth(i)) => bodies.cloths.get(i)?.center,
        Pick::Soft(SoftPick::Liquid) => bodies.fluid.center,
    })
}

/// The launch speed of a ball group or soft thing, or None for one that isn't launched.
fn pick_velocity(scene: &Scene, pick: Pick) -> Option<Vec2> {
    match pick {
        Pick::Balls(i) => scene.spawners.get(i).map(spawner_velocity),
        Pick::Soft(SoftPick::Jelly(i)) => scene.bodies.jelly.get(i).map(|j| j.velocity),
        Pick::Soft(SoftPick::Liquid) => Some(scene.bodies.fluid.velocity),
        Pick::Soft(SoftPick::Rope(_) | SoftPick::Cloth(_)) => None,
    }
}

/// How a ball group or soft thing is grabbed and outlined: its half width and height in world
/// pixels, and whether it is a box (or else a circle). None for a rope, which is a line.
fn pick_extent(scene: &Scene, pick: Pick) -> Option<(Vec2, bool)> {
    let bodies = &scene.bodies;
    match pick {
        Pick::Balls(i) => {
            let spawner = scene.spawners.get(i)?;
            Some((spawner_half_size(spawner), matches!(spawner, Spawner::Grid { .. })))
        }
        Pick::Soft(SoftPick::Jelly(i)) => {
            let jelly = bodies.jelly.get(i)?;
            let cells = Vec2::new(jelly.columns.saturating_sub(1) as f32, jelly.rows.saturating_sub(1) as f32);
            Some((cells * jelly.spacing / 2.0 + Vec2::splat(jelly.radius), jelly.count() > 1))
        }
        Pick::Soft(SoftPick::Cloth(i)) => Some((bodies.cloths.get(i)?.size / 2.0, true)),
        Pick::Soft(SoftPick::Liquid) => {
            // The block the drops start in, laid out as the simulation lays them out.
            let fluid = &bodies.fluid;
            let spacing = fluid.radius.max(1.0) * 2.0;
            let count = fluid.count.max(1);
            let columns = ((fluid.size.x / spacing).floor() as u32).clamp(1, count);
            let rows = count.div_ceil(columns);
            let half = Vec2::new(columns as f32 * spacing, rows as f32 * spacing * 0.866) / 2.0;
            Some((half + Vec2::splat(fluid.radius), true))
        }
        Pick::Soft(SoftPick::Rope(_)) => None,
    }
}

/// Distance from `p` to the line from `a` to `b`.
fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
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

fn arrow_tip(scene: &Scene, pick: Pick) -> Option<Vec2> {
    Some(pick_center(scene, pick)? + pick_velocity(scene, pick)? * ARROW_SECONDS)
}

/// Screen points within which a handle can be grabbed.
const GRAB: f32 = 10.0;

fn hit_test(scene: &Scene, view: &View, pos: Pos2) -> Option<Handle> {
    let world = view.to_world(pos);
    let reach = GRAB / view.scale;
    let all: Vec<Pick> = picks(scene).collect();
    for &pick in all.iter().rev() {
        if let Some(tip) = arrow_tip(scene, pick)
            && view.to_screen(tip).distance(pos) <= GRAB + 2.0
        {
            return Some(Handle::Aim(pick));
        }
    }
    for (rope, r) in scene.bodies.ropes.iter().enumerate().rev() {
        for (end, at) in [(true, r.to), (false, r.from)] {
            if view.to_screen(at).distance(pos) <= GRAB + 2.0 {
                return Some(Handle::RopeEnd { rope, end });
            }
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
    // Ball groups first, then the smaller soft things before the big cloth and liquid.
    for &pick in all.iter().rev() {
        let inside = match (pick, pick_extent(scene, pick)) {
            (Pick::Soft(SoftPick::Rope(i)), _) => {
                let rope = &scene.bodies.ropes[i];
                distance_to_segment(world, rope.from, rope.to) <= rope.thickness / 2.0 + reach
            }
            (_, Some((half, boxy))) => {
                let Some(center) = pick_center(scene, pick) else { continue };
                let d = (world - center).abs();
                let half = half.max(Vec2::splat(reach));
                if boxy { d.x <= half.x && d.y <= half.y } else { d.length() <= half.x }
            }
            (_, None) => false,
        };
        if inside {
            return Some(Handle::Body(pick));
        }
    }
    None
}

fn apply_drag(scene: &mut Scene, drag: Drag, world: Vec2) {
    match drag.handle {
        Handle::Body(pick) => {
            let target = (world - drag.offset).round();
            let bodies = &mut scene.bodies;
            match pick {
                Pick::Balls(i) => match scene.spawners.get_mut(i) {
                    Some(Spawner::Grid { center, .. }) => *center = target,
                    Some(Spawner::Single { position, .. } | Spawner::Stream { position, .. }) => *position = target,
                    None => {}
                },
                Pick::Soft(SoftPick::Jelly(i)) => {
                    if let Some(jelly) = bodies.jelly.get_mut(i) {
                        jelly.center = target;
                    }
                }
                Pick::Soft(SoftPick::Rope(i)) => {
                    // The whole rope moves, keeping its length and slant.
                    if let Some(rope) = bodies.ropes.get_mut(i) {
                        let shift = target - (rope.from + rope.to) / 2.0;
                        rope.from = (rope.from + shift).round();
                        rope.to = (rope.to + shift).round();
                    }
                }
                Pick::Soft(SoftPick::Cloth(i)) => {
                    if let Some(cloth) = bodies.cloths.get_mut(i) {
                        cloth.center = target;
                    }
                }
                Pick::Soft(SoftPick::Liquid) => bodies.fluid.center = target,
            }
        }
        Handle::Aim(pick) => {
            let Some(center) = pick_center(scene, pick) else { return };
            let velocity = ((world - center) / ARROW_SECONDS).clamp_length_max(3000.0).round();
            match pick {
                Pick::Balls(i) => match scene.spawners.get_mut(i) {
                    Some(
                        Spawner::Grid { velocity: v, .. }
                        | Spawner::Single { velocity: v, .. }
                        | Spawner::Stream { velocity: v, .. },
                    ) => *v = velocity,
                    None => {}
                },
                Pick::Soft(SoftPick::Jelly(i)) => {
                    if let Some(jelly) = scene.bodies.jelly.get_mut(i) {
                        jelly.velocity = velocity;
                    }
                }
                Pick::Soft(SoftPick::Liquid) => scene.bodies.fluid.velocity = velocity,
                Pick::Soft(SoftPick::Rope(_) | SoftPick::Cloth(_)) => {}
            }
        }
        Handle::RopeEnd { rope, end } => {
            if let Some(r) = scene.bodies.ropes.get_mut(rope) {
                let at = if end { &mut r.to } else { &mut r.from };
                *at = (world - drag.offset).round();
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
                Container::Circle { radius, .. }
                | Container::Polygon { radius, .. }
                | Container::Custom { radius, .. } => {
                    *radius = (*radius * scale.unwrap_or(1.0)).clamp(20.0, 1200.0).round()
                }
                Container::Rings { inner_radius, .. } => {
                    *inner_radius = (*inner_radius * scale.unwrap_or(1.0)).clamp(10.0, 1000.0).round()
                }
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

fn draw_handles(ui: &Ui, scene: &Scene, view: &View, active: Option<Handle>, selected: Option<Pick>) {
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

    for pick in picks(scene) {
        let Some(world_center) = pick_center(scene, pick) else { continue };
        let highlighted = selected == Some(pick) || active.and_then(Handle::pick) == Some(pick);
        let color = if highlighted { theme::ACCENT } else { faint };
        let center = view.to_screen(world_center);
        match (pick, pick_extent(scene, pick)) {
            (Pick::Soft(SoftPick::Rope(i)), _) => {
                let rope = &scene.bodies.ropes[i];
                let ends = [view.to_screen(rope.from), view.to_screen(rope.to)];
                painter.extend(egui::Shape::dashed_line(&ends, Stroke::new(1.2, color), 5.0, 4.0));
                for (end, at) in [(false, ends[0]), (true, ends[1])] {
                    let end_active = active == Some(Handle::RopeEnd { rope: i, end });
                    painter.circle(
                        at,
                        if end_active { 7.0 } else { 5.5 },
                        if end_active || highlighted { theme::ACCENT } else { theme::TEXT },
                        Stroke::new(1.5, Color32::BLACK),
                    );
                }
            }
            (_, Some((half, boxy))) => {
                let half = half * view.scale + Vec2::splat(4.0);
                if boxy {
                    let r = Rect::from_center_size(center, egui::vec2(half.x, half.y) * 2.0);
                    let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(1.2, color), 5.0, 4.0));
                } else {
                    painter.circle_stroke(center, half.x, Stroke::new(1.2, color));
                }
                if matches!(pick, Pick::Balls(i) if matches!(scene.spawners[i], Spawner::Stream { .. })) {
                    painter.circle_stroke(center, half.x + 5.0, Stroke::new(1.0, color.gamma_multiply(0.6)));
                }
            }
            (_, None) => {}
        }

        // Launch arrow.
        let Some(tip) = arrow_tip(scene, pick) else { continue };
        let tip = view.to_screen(tip);
        let aim_active = active == Some(Handle::Aim(pick));
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
            let speed = pick_velocity(scene, pick).unwrap_or_default().length();
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Cloth, Jelly, Rope};

    /// A view showing the whole 1080 × 1920 video at its real size.
    fn view(scene: &Scene) -> View {
        let size = egui::vec2(scene.video.width as f32, scene.video.height as f32);
        View { rect: Rect::from_min_size(Pos2::ZERO, size), scale: 1.0 }
    }

    fn scene_with_soft_things() -> Scene {
        // A box bigger than the video, so its edge handles are out of the way.
        let container = Container::Rect {
            center: Vec2::ZERO,
            size: Vec2::new(3000.0, 4000.0),
            thickness: 4.0,
            color: bpe_scene::Color::rgb(255, 255, 255),
        };
        let mut scene = Scene { spawners: Vec::new(), container, ..Scene::default() };
        scene.bodies.jelly.push(Jelly {
            center: Vec2::new(-200.0, 0.0),
            velocity: Vec2::new(200.0, 0.0),
            ..Jelly::default()
        });
        scene.bodies.ropes.push(Rope { from: Vec2::new(-300.0, 500.0), to: Vec2::new(300.0, 500.0), ..Rope::default() });
        scene.bodies.cloths.push(Cloth { center: Vec2::new(0.0, -500.0), ..Cloth::default() });
        scene
    }

    fn drag(scene: &mut Scene, from: Vec2, to: Vec2) -> Handle {
        let view = view(scene);
        let handle = hit_test(scene, &view, view.to_screen(from)).expect("something to grab");
        let anchor = match handle {
            Handle::Body(pick) => pick_center(scene, pick).unwrap(),
            Handle::RopeEnd { rope, end } => {
                let rope = &scene.bodies.ropes[rope];
                if end { rope.to } else { rope.from }
            }
            _ => from,
        };
        apply_drag(scene, Drag { handle, offset: from - anchor }, to);
        handle
    }

    #[test]
    fn jelly_balls_can_be_dragged_and_aimed() {
        let mut scene = scene_with_soft_things();
        let jelly = Pick::Soft(SoftPick::Jelly(0));
        // Grab a little off the middle: it keeps that offset rather than jumping.
        let handle = drag(&mut scene, Vec2::new(-190.0, 10.0), Vec2::new(10.0, 110.0));
        assert!(handle == Handle::Body(jelly));
        assert_eq!(scene.bodies.jelly[0].center, Vec2::new(0.0, 100.0));

        let tip = arrow_tip(&scene, jelly).unwrap();
        let handle = drag(&mut scene, tip, Vec2::new(0.0, 100.0 - 0.35 * 400.0));
        assert!(handle == Handle::Aim(jelly));
        assert_eq!(scene.bodies.jelly[0].velocity, Vec2::new(0.0, -400.0));
    }

    #[test]
    fn ropes_move_by_their_ends_or_as_a_whole() {
        let mut scene = scene_with_soft_things();
        let handle = drag(&mut scene, Vec2::new(300.0, 500.0), Vec2::new(250.0, 400.0));
        assert!(handle == Handle::RopeEnd { rope: 0, end: true });
        assert_eq!(scene.bodies.ropes[0].to, Vec2::new(250.0, 400.0));
        assert_eq!(scene.bodies.ropes[0].from, Vec2::new(-300.0, 500.0));

        // Grabbing the line itself moves both ends together.
        let on_line = (scene.bodies.ropes[0].from + scene.bodies.ropes[0].to) / 2.0;
        let handle = drag(&mut scene, on_line, on_line + Vec2::new(0.0, 100.0));
        assert!(handle == Handle::Body(Pick::Soft(SoftPick::Rope(0))));
        assert_eq!(scene.bodies.ropes[0].from, Vec2::new(-300.0, 600.0));
        assert_eq!(scene.bodies.ropes[0].to, Vec2::new(250.0, 500.0));
    }

    #[test]
    fn cloth_and_liquid_can_be_dragged() {
        let mut scene = scene_with_soft_things();
        let handle = drag(&mut scene, Vec2::new(100.0, -500.0), Vec2::new(100.0, -400.0));
        assert!(handle == Handle::Body(Pick::Soft(SoftPick::Cloth(0))));
        assert_eq!(scene.bodies.cloths[0].center, Vec2::new(0.0, -400.0));

        scene.bodies.fluid = bpe_scene::Fluid { count: 500, center: Vec2::new(0.0, 300.0), ..Default::default() };
        // Off the middle, where a still liquid's arrow tip sits.
        let handle = drag(&mut scene, Vec2::new(40.0, 300.0), Vec2::new(-60.0, 300.0));
        assert!(handle == Handle::Body(Pick::Soft(SoftPick::Liquid)));
        assert_eq!(scene.bodies.fluid.center, Vec2::new(-100.0, 300.0));
    }

    #[test]
    fn ball_groups_win_over_soft_things_under_them() {
        let mut scene = scene_with_soft_things();
        scene.spawners.push(Spawner::Single {
            position: Vec2::new(0.0, -500.0),
            velocity: Vec2::new(300.0, 0.0),
            radius: 20.0,
            color: bpe_scene::Color::rgb(255, 255, 255),
            depth: 0.0,
            depth_velocity: 0.0,
            style: None,
        });
        let view = view(&scene);
        let handle = hit_test(&scene, &view, view.to_screen(Vec2::new(0.0, -500.0)));
        assert!(handle == Some(Handle::Body(Pick::Balls(0))));
    }
}
