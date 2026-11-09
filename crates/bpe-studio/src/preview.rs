//! The video preview, plus the handles that set a scene up by dragging in it: move a thing (a
//! rider by its offset), aim balls by their arrow tip, and mould a shape (see `shape_handles`).

use crate::app::{CATCH_UP_BUDGET, Studio};
use crate::grid::snap_point;
use crate::lights_panel::{self, LightSource};
use crate::shape_handles::{self, MenuTarget, Mould};
use crate::style;
use bpe_core::{Attach, Registry, Resolved, Scene, Settings, ThingId, Value};
use bpe_gfx::{Frame, LightShow, Renderer, build_frame_sensing, wgpu};
use bpe_kit::BallsSettings;
use bpe_runtime::{CameraView, Pose, World};
use eframe::egui::{self, Align2, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;

pub mod air_view;
pub mod free_points;
pub mod light_handles;
pub mod particle_handles;
pub mod text_handles;
pub mod text_typing;
mod warm;
pub mod world_view;
pub mod zones;

pub use light_handles::LightPart;

pub use world_view::{PreviewMode, WorldNav};

/// The launch arrow's tip sits this many seconds of launch speed from the balls.
pub const ARROW_SECONDS: f32 = 0.25;

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
    /// What the open right-click menu on a shape is for.
    menu: Option<MenuTarget>,
    /// What the picture on screen was drawn from, so it is only drawn again when that changes.
    drawn: Option<Drawn>,
    /// The effects' warm-up after a jump in time, and the renderer it draws on.
    warm: Option<warm::Warmup>,
    spare: Option<Renderer>,
    /// What the last picture showed and whether its effects' memory is short, to see jumps by.
    shown: warm::Shown,
}

impl Preview {
    /// Whether a handle in the picture is being dragged.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[derive(PartialEq)]
struct Drawn {
    revision: u64,
    time: f64,
    step: u64,
    size: (u32, u32),
    /// The World view's own camera and picture size, when it is the one shown.
    look: Option<(CameraView, (u32, u32))>,
    /// What a lit picture shows, and the soloed lights.
    light: (LightShow, Vec<LightSource>),
    /// The world step of the particles drawn (the particle playback's), when there are some.
    particles: Option<u64>,
    /// Grade mode's compare view and Split line, in Grade mode.
    grade: Option<(crate::grade_mode::Compare, u32)>,
}

/// Something in the preview that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// Move a thing.
    Body(ThingId),
    /// Aim balls by their launch arrow.
    Aim(ThingId),
    /// Mould a shape by one of its corners.
    Corner(ThingId, usize),
    /// Mould a shape by one of its sides: both its corners move.
    Side(ThingId, usize),
    /// Bend a shape's curved side by one of its two handles.
    Bend(ThingId, usize, u8),
    /// Size a circle by its edge (an ellipse: both its width and height together).
    Radius(ThingId),
    /// Set an ellipse's Width (0) or Height (1) by the dot at the end of that axis.
    Axis(ThingId, u8),
    /// Move the camera by its frame's middle or edge, in the World view.
    CameraMove(ThingId),
    /// Zoom the camera by a corner of its frame.
    CameraZoom(ThingId),
    /// Turn the camera by the handle above its frame.
    CameraTurn(ThingId),
    /// Set a shape's Distance's Far (true) or Near by its dotted line.
    Zone(ThingId, bpe_core::LayerId, bool),
    /// A light's lamp, reach, cone, aim, panel corner or strip end.
    Light(ThingId, LightPart),
    /// Point `.2` of the `.1`th gradient laid out as Free points that thing `.0` shows.
    FreePoint(ThingId, u8, u8),
    /// A Particles thing's area corner or edge, or its line's end.
    Particles(ThingId, particle_handles::ParticlePart),
    /// A Text's Size dot, Wrap bar or Radius ring.
    Text(ThingId, text_handles::TextPart),
}

impl Handle {
    pub fn thing(self) -> ThingId {
        match self {
            Handle::Body(id)
            | Handle::Aim(id)
            | Handle::Corner(id, _)
            | Handle::Side(id, _)
            | Handle::Bend(id, _, _)
            | Handle::Radius(id)
            | Handle::Axis(id, _)
            | Handle::CameraMove(id)
            | Handle::CameraZoom(id)
            | Handle::CameraTurn(id)
            | Handle::Zone(id, _, _)
            | Handle::Light(id, _)
            | Handle::FreePoint(id, _, _)
            | Handle::Particles(id, _)
            | Handle::Text(id, _) => id,
        }
    }

    /// A shape's corner, side, bend handle, circle or ellipse edge, or an ellipse's axis dot.
    fn is_mould(self) -> bool {
        matches!(self, Handle::Corner(..) | Handle::Side(..) | Handle::Bend(..) | Handle::Radius(_) | Handle::Axis(..))
    }

    fn is_camera(self) -> bool {
        matches!(self, Handle::CameraMove(_) | Handle::CameraZoom(_) | Handle::CameraTurn(_))
    }
}

#[derive(Clone, Debug)]
pub struct Drag {
    pub handle: Handle,
    /// Pointer position minus the dragged thing's position, in world pixels.
    pub offset: Vec2,
    /// A shape's outline when a corner, side or bend drag began.
    pub mould: Option<Mould>,
    /// Shift is held: a moulding drag mirrors across the shape's middle line.
    pub shift: bool,
    /// A rider grabbed by its body: where the pointer was pressed (world pixels) and its ride
    /// offset then. The drag moves the offset by how far the pointer has gone since.
    pub ride: Option<(Vec2, Vec2)>,
    /// A light's handle: the light as it was when grabbed.
    pub light: Option<light_handles::LightGrab>,
}

impl Drag {
    pub fn new(handle: Handle, offset: Vec2) -> Drag {
        Drag { handle, offset, mould: None, shift: false, ride: None, light: None }
    }
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
    /// The drag that began stays at the playhead: the camera's, so keys land there, and a
    /// shape's moulding, so a turned or shrunk shape is moulded where it is seen.
    pub keep_playhead: bool,
    /// The preview was clicked while playing.
    pub clicked: bool,
    /// What is wrong with a shader module the picture skipped, when one is.
    pub shader_problem: Option<String>,
    /// The picture's lighting, when it is lit: how many lights and the GPU's time on them (ms).
    pub lighting: Option<(usize, Option<f32>)>,
}

/// Converts between world pixels and screen points, through the camera the picture is drawn
/// with: the scene's in the Camera view, the free one in the World view.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub rect: Rect,
    /// Screen points per screen unit of the picture (a 1080-wide video is 1080 units across).
    pub scale: f32,
    pub camera: CameraView,
    /// The World view's free canvas, rather than the shot.
    pub world: bool,
    /// The moved Stage camera the shot is seen through (lighting part 5): points on the picture
    /// plane map through it.
    pub stage: Option<bpe_gfx::StageView>,
}

impl View {
    pub fn to_screen(self, p: Vec2) -> Pos2 {
        let q = match self.stage {
            // Behind the moved camera: far off the preview.
            Some(s) => s.screen_of(p).unwrap_or(Vec2::splat(1e6)),
            None => self.camera.to_screen(p),
        };
        self.rect.center() + egui::vec2(q.x, q.y) * self.scale
    }

    pub fn to_world(self, p: Pos2) -> Vec2 {
        let d = (p - self.rect.center()) / self.scale;
        let d = Vec2::new(d.x, d.y);
        self.stage.and_then(|s| s.plane_at(d)).unwrap_or_else(|| self.camera.to_world(d))
    }

    /// Screen points per world pixel.
    pub fn world_scale(&self) -> f32 {
        self.scale * self.camera.zoom * self.stage.map_or(1.0, |s| s.fill)
    }

    /// The view `id` is seen through. A thing stuck to the screen sits on the picture, where
    /// the camera doesn't move it, so it is seen without the camera; the World view shows the
    /// world only, so it isn't seen there at all.
    pub fn of(&self, studio: &Studio, id: ThingId) -> Option<View> {
        if !studio.resolved.things.get(&id).is_some_and(|r| r.screen) {
            return Some(*self);
        }
        (!self.world).then_some(View { camera: CameraView::default(), stage: None, ..*self })
    }

    /// Where the pointer at `pos` is, in the space of the thing `handle` holds.
    pub fn pointer_for(&self, studio: &Studio, handle: Handle, pos: Pos2) -> Vec2 {
        self.of(studio, handle.thing()).unwrap_or(*self).to_world(pos)
    }
}

/// Draws the video at the playhead, aspect-fit in the panel, and handles dragging in it. In the
/// World view the whole panel is a free canvas over the world instead.
pub fn show(ui: &mut Ui, frame: &eframe::Frame, studio: &mut Studio) -> Outcome {
    let Some(render_state) = frame.wgpu_render_state() else {
        ui.label("The preview needs the wgpu renderer.");
        return Outcome::default();
    };
    let grading = studio.mode == crate::grade_mode::EditorMode::Grade;
    // A two-part switch in the corner: the on part is inverted. Grade mode compares instead.
    if grading {
        compare_switch(ui, studio);
    } else {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let frame = egui::Frame::new().stroke(Stroke::new(1.0, style::LINE)).corner_radius(style::RADIUS);
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let modes = [
                    (PreviewMode::Camera, "Camera", "Exactly what the video shows"),
                    (
                        PreviewMode::World,
                        "World",
                        "The whole world: scroll to zoom, drag with the middle button to pan, F to frame",
                    ),
                ];
                for (mode, name, help) in modes {
                    let on = studio.preview_mode == mode;
                    let button = style::toggle_button(name, on).frame_when_inactive(on).min_size(egui::vec2(0.0, 24.0));
                    if ui.add(button).on_hover_text(help).clicked() {
                        studio.preview_mode = mode;
                    }
                }
            });
        });
    }
    let settings = studio.settings();
    let (video_width, video_height) = (settings.width.max(1), settings.height.max(1));
    let mut area = ui.available_rect_before_wrap().shrink(12.0);
    // A lit scene gets the Show bar under the picture.
    let lit = studio
        .scene
        .card(ThingId::ROOT, bpe_core::SlotId::Lighting)
        .is_some_and(|c| c.kind == bpe_kit::modules::lighting::LIGHTING);
    let bar = lit.then(|| {
        let bar = Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 24.0), area.max);
        area.max.y -= 32.0;
        bar
    });
    // Grade mode grades the video's picture: always the Camera view.
    let world_mode = studio.preview_mode == PreviewMode::World && !grading;
    // The picture's size in screen units, and where it goes on screen.
    let (picture, rect) = if world_mode {
        let w = (video_height as f32 * area.width() / area.height().max(1.0)).round().max(2.0) as u32;
        ((w, video_height), area)
    } else {
        let aspect = video_width as f32 / video_height as f32;
        let mut size = egui::vec2(area.height() * aspect, area.height());
        if size.x > area.width() {
            size = egui::vec2(area.width(), area.width() / aspect);
        }
        ((video_width, video_height), Rect::from_center_size(area.center(), size))
    };
    // Render at the screen's real pixel size, but never above the video's own size.
    let scale = (rect.width() * ui.ctx().pixels_per_point() / picture.0 as f32).min(1.0);
    let (width, height) = bpe_cli::output_size(picture.0, picture.1, scale);
    let view_scale = rect.width() / picture.0 as f32;
    let look = world_mode.then(|| (studio.world_nav.camera(), picture));

    let device = &render_state.device;
    let mut preview = studio.panels.preview.take().unwrap_or_else(|| {
        let mut renderer = Renderer::new(device, width, height);
        renderer.share_programs(shared_programs());
        renderer.preview_field_like_video(true);
        let texture = render_state.renderer.write().register_native_texture(
            device,
            renderer.output_view(),
            wgpu::FilterMode::Linear,
        );
        Preview {
            renderer,
            texture,
            frame: Frame::default(),
            drag: None,
            menu: None,
            drawn: None,
            warm: None,
            spare: None,
            shown: warm::Shown::default(),
        }
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

    let drawn_before = preview.drawn.as_ref().map(|d| (d.revision, d.time.to_bits(), d.step, d.grade));
    let catching_up = draw_picture(
        studio,
        device,
        &render_state.queue,
        &mut preview.renderer,
        &mut preview.frame,
        &mut preview.drawn,
        look,
    );
    let mut redrawn = preview.drawn.as_ref().map(|d| (d.revision, d.time.to_bits(), d.step, d.grade)) != drawn_before;
    if warm::update(&mut preview, studio, device, &render_state.queue, catching_up, look) {
        // The warm renderer took the cold one's place: the picture shows its output now.
        render_state.renderer.write().update_egui_texture_from_wgpu_texture(
            device,
            preview.renderer.output_view(),
            wgpu::FilterMode::Linear,
            preview.texture,
        );
        redrawn = true;
    }
    if grading {
        // After the warm-up's swap, so the renderer counted is the one shown.
        count_histogram(studio, &mut preview.renderer, device, &render_state.queue, redrawn, ui.ctx());
    }
    if warm::busy(&preview) {
        ui.ctx().request_repaint();
    } else if let Some(due) = warm::spare_due(&preview) {
        // Looked at again when the kept spare renderer is let go.
        ui.ctx().request_repaint_after(due);
    }
    // Looked at again until the particle playback is where the playhead is.
    if !studio.particles_there() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    if redraw_when_built(&preview.renderer, ui.ctx()) {
        preview.drawn = None;
    }
    let mut outcome = Outcome { catching_up, ..Outcome::default() };
    outcome.shader_problem = preview.renderer.shader_problems().into_iter().next();
    outcome.lighting = preview.renderer.lighting_stats();

    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.painter().image(preview.texture, rect, uv, style::UNTINTED);
    if let Some(done) = studio.aiming() {
        aiming_pill(&ui.painter_at(rect), rect, done);
    } else if let Some(done) = studio.particles_waiting() {
        pill(&ui.painter_at(rect), rect, &format!("Particles\u{2026} {:.0}%", (done * 100.0).floor()));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    } else if let Some(w) = &preview.warm {
        pill(&ui.painter_at(rect), rect, &format!("Warming up effects\u{2026} {:.0}%", (w.done() * 100.0).floor()));
    }
    // Files held over the window, and a scene with nothing in it yet, each get a word in the middle.
    if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
        middle_note(&ui.painter_at(rect), rect, DROP_HINT);
    } else if is_empty_scene(&studio.scene) {
        middle_note(&ui.painter_at(rect), rect, EMPTY_HINT);
    }
    if let Some(bar) = bar {
        show_bar(ui, bar, studio);
        if studio.light_show == LightShow::HeatMap {
            heat_legend(&ui.painter_at(rect), rect);
        }
    }
    // Solo only what is still there (a soloed light deleted or undone away ends its solo).
    lights_panel::prune_solo(studio);
    if !studio.light_solo.is_empty() {
        solo_pill(ui, rect, studio, studio.aiming().is_some());
    }

    let camera = if world_mode { studio.world_nav.camera() } else { studio.playback.world().camera };
    let stage = if world_mode { None } else { studio.stage_view };
    let mut view = View { rect, scale: view_scale, camera, world: world_mode, stage };
    if world_mode && navigate(ui, &response, studio, &view) {
        view.camera = studio.world_nav.camera();
        preview.drawn = None;
        ui.ctx().request_repaint();
    }
    studio.view_centre = view.camera.center;
    // The grid's lines are square to the picture, so a turned camera shows none (snapping still works).
    if view.camera.turn == 0.0 {
        studio.grid.draw(&ui.painter_at(rect), rect, view.to_screen(Vec2::ZERO), view.world_scale());
    }
    if world_mode {
        let painter = ui.painter_at(rect);
        let world = studio.playback.world();
        let active = preview.drag.as_ref().map(|d| d.handle);
        world_view::draw_camera(&painter, &studio.scene, &studio.resolved, world, studio.playback.log(), &view, active);
    }

    // Pick from picture: the click picks a colour and does nothing else (in Grade mode too: Point
    // Colour picks from the picture as it is shown).
    if crate::eyedropper::target(ui.ctx()).is_some() {
        preview.drag = None;
        pick_colour(ui, &response, rect, &preview.renderer, device, &render_state.queue);
        studio.panels.preview = Some(preview);
        return outcome;
    }

    // Grade mode only looks: no handles; Split has its line.
    if grading {
        preview.drag = None;
        split_line(ui, &response, rect, studio);
        studio.panels.preview = Some(preview);
        return outcome;
    }

    if studio.playing {
        preview.drag = None;
        outcome.clicked = response.clicked();
        studio.panels.preview = Some(preview);
        return outcome;
    }

    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let grab = |studio: &Studio, at: Pos2| {
        let on_camera =
            world_mode.then(|| world_view::camera_hit(&studio.scene, studio.playback.world(), &view, at)).flatten();
        on_camera.or_else(|| hit_test(studio, &view, at))
    };
    let hovered = pointer.and_then(|p| grab(studio, p));
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && let Some(handle) = grab(studio, press)
    {
        preview.drag = Some(begin_drag(studio, &view, handle, press));
        // Each drag is its own undo step.
        studio.panels.inspector.own_step = true;
        outcome.drag_started = true;
        // A light keeps the playhead too: its keys land where it is seen.
        outcome.keep_playhead = handle.is_camera()
            || handle.is_mould()
            || matches!(
                handle,
                Handle::Zone(..) | Handle::Light(..) | Handle::FreePoint(..) | Handle::Particles(..) | Handle::Text(..)
            );
    }
    if response.drag_stopped() {
        preview.drag = None;
    }
    if let Some(drag) = preview.drag.as_mut() {
        drag.shift = ui.input(|i| i.modifiers.shift);
    }
    if let (Some(drag), Some(pos)) = (preview.drag.clone(), response.interact_pointer_pos()) {
        let step = studio.grid.step(ui.input(|i| i.modifiers.alt));
        if drag.handle.is_camera() {
            let time = studio.sim_time();
            let reg = &studio.kit.registry;
            let keyed = world_view::apply_camera_drag(
                &mut studio.scene,
                reg,
                studio.playback.world(),
                drag.clone(),
                view.to_world(pos),
                step,
                time,
            );
            studio.panels.inspector.keyed_edit |= keyed;
        } else if let (Handle::Light(id, part), Some(grab)) = (drag.handle, drag.light.as_ref()) {
            let pointer = view.pointer_for(studio, drag.handle, pos);
            let (time, reg) = (studio.sim_time(), &studio.kit.registry);
            let keyed = light_handles::apply(&mut studio.scene, reg, id, part, grab, pointer, step, time);
            studio.panels.inspector.keyed_edit |= keyed;
        } else if let Handle::Zone(shape, layer, far) = drag.handle {
            // Near and Far take keys: a keyed one keys at the playhead (and playing pauses).
            let pointer = view.pointer_for(studio, drag.handle, pos);
            let (time, reg) = (studio.sim_time(), &studio.kit.registry);
            let keyed = zones::apply(&mut studio.scene, reg, studio.playback.world(), shape, layer, far, pointer, time);
            studio.panels.inspector.keyed_edit |= keyed;
        } else if let Handle::FreePoint(thing, n, i) = drag.handle {
            // A keyed points list keys at the playhead (and playing pauses).
            let pointer = free_points::pointer(studio, &view, drag.handle, pos);
            let (time, reg) = (studio.sim_time(), &studio.kit.registry);
            let keyed = free_points::apply(&mut studio.scene, reg, studio.playback.world(), thing, n, i, pointer, time);
            studio.panels.inspector.keyed_edit |= keyed;
        } else {
            // A keyed circle's Size keys at the playhead (and playing pauses).
            let pointer = view.pointer_for(studio, drag.handle, pos);
            let (time, reg) = (studio.sim_time(), &studio.kit.registry);
            let keyed = apply_drag(&mut studio.scene, reg, studio.playback.world(), &drag, pointer, step, time);
            studio.panels.inspector.keyed_edit |= keyed;
        }
        outcome.selected = Some(drag.handle.thing());
    } else if response.clicked()
        && let Some(handle) = hovered
    {
        outcome.selected = Some(handle.thing());
    }

    // A double-click on a Text types into it.
    if response.double_clicked()
        && let Some(Handle::Body(id)) = hovered
        && studio.scene.things.get(&id).is_some_and(|t| t.kind == bpe_kit::things::text::KIND)
    {
        preview.drag = None;
        text_typing::start(studio, id);
    }
    if studio.typing.is_some() {
        text_typing::show(ui, studio, &view);
    }

    shape_menu(&response, studio, &view, &mut preview.menu);

    let active = preview.drag.as_ref().map(|d| d.handle).or(hovered);
    if active.is_some() {
        ui.ctx().set_cursor_icon(if preview.drag.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }
    draw_handles(ui, studio, &view, active, outcome.selected.or(studio.selection.primary));
    studio.panels.preview = Some(preview);
    outcome
}

/// The eyedropper over the picture at `rect`: a crosshair and a pill saying what to do; a click
/// reads the drawn picture back and hands the colour under the pointer to the gradient row that
/// asked; a right-click or Esc stops.
fn pick_colour(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    renderer: &Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) {
    let ctx = ui.ctx();
    // The row that asked has gone (another thing or tab): nothing could take the colour.
    if !crate::eyedropper::alive(ctx) {
        crate::eyedropper::stop(ctx);
        return;
    }
    // At the bottom, clear of the pills at the top.
    let text = "Click the picture to pick a colour (Esc to stop)";
    let painter = ui.painter_at(rect);
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), style::TEXT_STRONG);
    let back = Rect::from_min_size(
        rect.left_bottom() + egui::vec2(10.0, -18.0 - galley.size().y),
        galley.size() + egui::vec2(16.0, 8.0),
    );
    painter.rect_filled(back, style::RADIUS, style::PILL);
    painter.galley(back.min + egui::vec2(8.0, 4.0), galley, style::TEXT_STRONG);
    if response.hovered() {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
    }
    if response.secondary_clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        crate::eyedropper::stop(ctx);
        return;
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let uv = (p - rect.min) / rect.size();
        let picked = renderer
            .read_output(device, queue)
            .ok()
            .and_then(|pixels| crate::eyedropper::pixel_at(&pixels, renderer.size(), egui::vec2(uv.x, uv.y)));
        match picked {
            Some(c) => crate::eyedropper::deliver(ctx, c),
            None => crate::eyedropper::stop(ctx),
        }
    }
}

/// The drag of `handle` pressed at `press` on the screen. A rider keeps its ride offset at the
/// press, and the drag moves it by how far the pointer goes, so it doesn't jump when its own
/// position (a ball's, which is its parent's) or its parent's place at the playhead differ from
/// where the drag goes on.
fn begin_drag(studio: &Studio, view: &View, handle: Handle, press: Pos2) -> Drag {
    let world = studio.playback.world();
    let at = view.pointer_for(studio, handle, press);
    let anchor = match handle {
        Handle::Body(id) => place_of(&studio.scene, world, id).unwrap_or(at),
        Handle::CameraMove(_) => world.camera.center,
        _ => at,
    };
    let ride = match (handle, studio.scene.things.get(&handle.thing()).map(|t| t.attach)) {
        (Handle::Body(_), Some(Attach::Ride { offset, .. })) => Some((at, offset)),
        _ => None,
    };
    let mould = shape_handles::start(world, handle, at);
    let light = match handle {
        Handle::Light(id, part) => light_handles::grab(studio, id, part, at),
        _ => None,
    };
    Drag { mould, ride, light, ..Drag::new(handle, at - anchor) }
}

/// The right-click menu on the selected shape: a side's, a corner's or a circle's edge's. What it
/// was opened on is kept in `menu` while it is open; the item picked is its own undo step.
fn shape_menu(response: &egui::Response, studio: &mut Studio, view: &View, menu: &mut Option<MenuTarget>) {
    if response.secondary_clicked() {
        *menu = studio.selection.primary.and_then(|id| {
            let v = view.of(studio, id)?;
            let pos = response.interact_pointer_pos()?;
            shape_handles::menu_target(studio.playback.world(), &v, id, pos)
        });
    }
    let Some(target) = *menu else { return };
    let mut picked = None;
    let open = response.context_menu(|ui| picked = shape_handles::menu(ui, &studio.scene, target));
    if let Some(action) = picked
        && shape_handles::act(&mut studio.scene, target, action)
    {
        studio.panels.inspector.own_step = true;
    }
    if open.is_none() || picked.is_some() {
        *menu = None;
    }
}

/// Pans and zooms the World view: scroll zooms about the pointer, the middle button pans (Space
/// is left to play and pause), F frames the selected thing (or the camera's shot). Returns
/// whether the view moved.
fn navigate(ui: &Ui, response: &egui::Response, studio: &mut Studio, view: &View) -> bool {
    let before = studio.world_nav;
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(pointer) = response.hover_pos()
        {
            studio.world_nav.zoom_about(view, pointer, (scroll * 0.003).exp());
        }
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        studio.world_nav.pan(view, response.drag_delta());
    }
    if !ui.ctx().text_edit_focused() && ui.input(|i| i.key_pressed(egui::Key::F)) {
        let world = studio.playback.world();
        let at = studio.selection.primary.and_then(|id| place_of(&studio.scene, world, id));
        match at {
            Some(at) => studio.world_nav.frame_on(view, at, 1200.0),
            None => studio.world_nav.frame_on(view, world.camera.center, world.video.y / world.camera.zoom * 1.4),
        }
    }
    studio.world_nav != before
}

/// The editor's in-ball shader programs, shared by its renderers (the preview, the ball view and
/// the group view, which all draw on the one GPU) and built on a background thread, so adding or
/// changing a shader never stalls the editor: until a program is ready its balls draw without
/// their shader layers.
pub fn shared_programs() -> &'static bpe_gfx::ProgramStore {
    static STORE: std::sync::OnceLock<bpe_gfx::ProgramStore> = std::sync::OnceLock::new();
    STORE.get_or_init(bpe_gfx::ProgramStore::in_background)
}

/// True when `renderer`'s last picture left out shader layers whose program was still building:
/// it is drawn again in a moment, so the effect shows as soon as it is ready.
pub fn redraw_when_built(renderer: &Renderer, ctx: &egui::Context) -> bool {
    let pending = renderer.programs_waiting();
    if pending {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    pending
}

/// Draws the playhead's moment into `renderer`, unless `drawn` says the picture already shows
/// it. Returns whether the picture is still catching up with the playhead. Glow in the dark
/// counts as remembering (`Frame::memory`), so `warm` replays it after a jump like the effects.
fn draw_picture(
    studio: &mut Studio,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    frame: &mut Frame,
    drawn: &mut Option<Drawn>,
    look: Option<(CameraView, (u32, u32))>,
) -> bool {
    // The particle playback follows the playhead (as far as the aiming lets the playback go) on
    // its own thread. Its latest particles are read once: the playback steps to their moment
    // and they are drawn only there.
    let particles = studio.particle_job.as_ref().and_then(|job| {
        job.follow(studio.aim_target());
        job.latest()
    });
    let reached = studio.particle_job.as_ref().map(|_| particles.as_ref().map(|p| p.time));
    // The background aiming holds the playback back until it has aimed the rounds before it, and
    // the particle playback until it has worked out the particles.
    let held = studio.aiming().is_some() || studio.particles_waiting().is_some();
    let target = studio.sim_target_with(reached);
    let catching_up = !studio.playback.seek_within(target, Some(CATCH_UP_BUDGET)) || held;
    // With particles the playback may wait a frame for them: the picture is then its moment.
    let step = 1.0 / f64::from(studio.playback.world().steps_per_second.max(1));
    let behind = studio.particle_job.is_some() && studio.playback.time() + step < studio.sim_time();
    // While catching up, the picture shows the moment the simulation has got to.
    let shown_time = if catching_up || behind { studio.playback.time() } else { studio.sim_time() };
    // Particles from another moment (on their way after an edit or a jump back) aren't drawn.
    let particles = particles.filter(|p| p.step == studio.playback.world().step);
    let now = Drawn {
        revision: studio.revision,
        time: shown_time,
        step: studio.playback.world().step,
        size: renderer.size(),
        look,
        light: (studio.light_show, studio.light_solo.iter().copied().collect()),
        particles: None,
        grade: (studio.mode == crate::grade_mode::EditorMode::Grade)
            .then(|| (studio.grade.compare, studio.grade.split.to_bits())),
    };
    let now = Drawn { particles: particles.as_ref().map(|p| p.step), ..now };
    if drawn.as_ref() != Some(&now) {
        // The particles are drawn from the particle playback's world, lent to this one.
        let lent = particles.map(|p| studio.playback.lend_states(p.lent()));
        compose(studio, studio.playback.world(), studio.playback.log(), shown_time, look, frame);
        if look.is_none() {
            studio.stage_view = frame.stage_view().filter(|v| v.motion > 0.0);
        }
        if let Some(lent) = lent {
            studio.playback.take_back(lent);
        }
        frame.cull();
        renderer.render(device, queue, frame);
        *drawn = Some(now);
    }
    catching_up
}

/// Builds the picture of `world` at `time` as the preview shows it: soloed lights alone (from a
/// copy with the others left out, never the scene's), the sound and light the picture reads, the
/// World view's camera and the Show bar's light show. The preview and the effects' warm-up both
/// build through it, so the picture a warm-up swaps in is the preview's own.
pub(crate) fn compose(
    studio: &Studio,
    world: &World,
    log: &bpe_runtime::Log,
    time: f64,
    look: Option<(CameraView, (u32, u32))>,
    frame: &mut Frame,
) {
    let solo = lights_panel::soloed(&studio.scene, &studio.resolved, &studio.light_solo);
    build_frame_sensing(
        &studio.scene,
        solo.as_ref().unwrap_or(&studio.resolved),
        world,
        log,
        time,
        &studio.kit.draws,
        studio.follow.as_deref().map(|f| f as &dyn bpe_runtime::SoundAt),
        studio.light_probe.as_deref(),
        frame,
    );
    // The World view looks through its own camera at a picture the panel's shape, and things
    // stuck to the screen belong to the video, not the world, so they are left out. Blend layers
    // move with the frame.
    if let Some((camera, size)) = look {
        world_look(frame, camera, size);
    }
    show_light(frame, studio.light_show);
    if studio.mode == crate::grade_mode::EditorMode::Grade {
        studio.grade.show_on(&mut frame.grade);
    }
}

/// Grade mode's corner switch: After, Before, or Split.
fn compare_switch(ui: &mut Ui, studio: &mut Studio) {
    use crate::grade_mode::Compare;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let frame = egui::Frame::new().stroke(Stroke::new(1.0, style::LINE)).corner_radius(style::RADIUS);
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (compare, name, help) in [
                (Compare::After, "After", "The graded picture"),
                (Compare::Before, "Before", "The picture without the grade (\\ switches)"),
                (
                    Compare::Split,
                    "Split",
                    "Without the grade left of the line, graded right of it: drag the line (Y switches)",
                ),
            ] {
                let on = studio.grade.compare == compare;
                let button = style::toggle_button(name, on).frame_when_inactive(on).min_size(egui::vec2(0.0, 24.0));
                if ui.add(button).on_hover_text(help).clicked() {
                    studio.grade.compare = compare;
                }
            }
        });
    });
}

/// Counts the picture into the Histogram after it was drawn again (or until it has counts), and
/// takes the counts once the GPU has them.
fn count_histogram(
    studio: &mut Studio,
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    redrawn: bool,
    ctx: &egui::Context,
) {
    if let Some(bins) = renderer.histogram(device) {
        studio.grade.bins = Some(bins);
    }
    // A picture drawn since the last count is counted once that count is in. The renderer itself
    // knows whether a count is on its way (a swapped-in renderer, or a failed read, has none).
    studio.grade.recount |= redrawn || studio.grade.bins.is_none();
    if studio.grade.recount && !renderer.histogram_pending() {
        renderer.request_histogram(device, queue);
        studio.grade.recount = false;
    }
    if renderer.histogram_pending() {
        // Looked at again soon: the counts come back without anything else asking for a frame.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// The Split view's line across the picture at `rect`: white, dragged left and right.
fn split_line(ui: &mut Ui, response: &egui::Response, rect: Rect, studio: &mut Studio) {
    if studio.grade.compare != crate::grade_mode::Compare::Split {
        return;
    }
    let x = rect.left() + rect.width() * studio.grade.split.clamp(0.0, 1.0);
    let grab = Rect::from_x_y_ranges(x - 6.0..=x + 6.0, rect.y_range());
    let near = response.hover_pos().is_some_and(|p| grab.contains(p));
    if response.drag_started() && ui.input(|i| i.pointer.press_origin()).is_some_and(|p| grab.contains(p)) {
        studio.grade.split_drag = true;
    }
    // The preview draws again by itself when the line moves: `Drawn` holds where it is.
    if studio.grade.split_drag
        && let Some(p) = response.interact_pointer_pos()
    {
        studio.grade.split = ((p.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
    }
    if response.drag_stopped() {
        studio.grade.split_drag = false;
    }
    if near || studio.grade.split_drag {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let x = rect.left() + rect.width() * studio.grade.split.clamp(0.0, 1.0);
    let painter = ui.painter_at(rect);
    painter.vline(x, rect.y_range(), Stroke::new(3.0, style::OUTLINE));
    painter.vline(x, rect.y_range(), Stroke::new(1.5, style::TEXT_STRONG));
    let knob = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(10.0, 28.0));
    painter.rect_filled(knob, style::RADIUS, style::TEXT_STRONG);
    painter.text(
        egui::pos2(x - 8.0, rect.top() + 8.0),
        egui::Align2::RIGHT_TOP,
        "Before",
        egui::FontId::proportional(11.0),
        style::TEXT_STRONG,
    );
    painter.text(
        egui::pos2(x + 8.0, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        "After",
        egui::FontId::proportional(11.0),
        style::TEXT_STRONG,
    );
}

/// Makes `frame` the World view's: seen through `camera` at `size`, without what is stuck to the
/// screen, the grade (it belongs to the finished video) or the Stage camera (the World view looks
/// at the world itself).
pub(crate) fn world_look(frame: &mut Frame, camera: CameraView, size: (u32, u32)) {
    frame.set_view(camera, size);
    frame.overlay = None;
    frame.grade = Default::default();
    if let Some(light) = frame.light.as_mut() {
        light.camera = None;
    }
}

/// Sets what a lit frame shows. Showing the light alone also leaves out the picture effects,
/// bloom, exposure and grade, so the light reads as it is.
fn show_light(frame: &mut Frame, show: LightShow) {
    let Some(light) = frame.light.as_mut() else { return };
    light.show = show;
    if show != LightShow::Picture {
        frame.effects.retain(|e| e.stage != bpe_gfx::Stage::Picture);
        frame.bloom = 0.0;
        frame.exposure = 1.0;
        frame.grade = Default::default();
    }
}

/// The Show bar under a lit picture: the picture, the light alone, or a heat map of it.
fn show_bar(ui: &mut Ui, bar: Rect, studio: &mut Studio) {
    let width = 300.0f32.min(bar.width());
    let at =
        Rect::from_min_max(egui::pos2(bar.center().x - width / 2.0, bar.top()), egui::pos2(bar.right(), bar.bottom()));
    let mut row =
        ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.spacing_mut().item_spacing.x = 2.0;
    row.label(egui::RichText::new("Show").color(style::MUTED))
        .on_hover_text("What the preview shows (never the video)");
    row.add_space(4.0);
    let shows = [
        (LightShow::Picture, "Picture", "The lit picture, as the video shows it"),
        (LightShow::LightOnly, "Light only", "Only the light falling on each spot, as on a white wall"),
        (
            LightShow::HeatMap,
            "Heat map",
            "How bright the light is: blue dim, green as unlit, red bright, white burnt out",
        ),
    ];
    for (show, name, help) in shows {
        let on = studio.light_show == show;
        let b = row.add(style::toggle_button(name, on).min_size(egui::vec2(0.0, 22.0))).on_hover_text(help);
        if b.clicked() {
            studio.light_show = show;
        }
    }
}

/// The heat map's key in the picture's bottom-right corner: the colours from no light to burnt
/// out (the same stops as the light pass's).
fn heat_legend(painter: &egui::Painter, rect: Rect) {
    let (w, h) = (150.0, 10.0);
    let bar = Rect::from_min_size(rect.right_bottom() + egui::vec2(-w - 16.0, -h - 30.0), egui::vec2(w, h));
    painter.rect_filled(bar.expand2(egui::vec2(8.0, 6.0)).with_max_y(bar.bottom() + 22.0), style::RADIUS, style::PILL);
    let top = style::HEAT[style::HEAT.len() - 1].0;
    let mut mesh = egui::Mesh::default();
    for (k, (at, colour)) in style::HEAT.iter().enumerate() {
        let x = bar.left() + bar.width() * at / top;
        mesh.colored_vertex(egui::pos2(x, bar.top()), *colour);
        mesh.colored_vertex(egui::pos2(x, bar.bottom()), *colour);
        if k > 0 {
            let i = (2 * k) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    for (at, words) in [(0.0, "0"), (1.0, "1"), (2.0, "2"), (top, "3+")] {
        let x = bar.left() + bar.width() * at / top;
        painter.text(
            egui::pos2(x, bar.bottom() + 3.0),
            Align2::CENTER_TOP,
            words,
            FontId::proportional(10.0),
            style::TEXT,
        );
    }
}

/// "Solo: Torch, Ambient" in the picture's top-left corner while lights are soloed; a click
/// ends the solo.
fn solo_pill(ui: &mut Ui, rect: Rect, studio: &mut Studio, below_aiming: bool) {
    let names = lights_panel::solo_names(&studio.scene, &studio.kit.registry, &studio.light_solo);
    if names.is_empty() {
        return;
    }
    let text = format!("Solo: {} (click to end)", names.join(", "));
    let painter = ui.painter_at(rect);
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), style::TEXT_STRONG);
    let at = rect.min + egui::vec2(10.0, if below_aiming { 40.0 } else { 10.0 });
    let back = Rect::from_min_size(at, galley.size() + egui::vec2(16.0, 8.0));
    let r = ui.interact(back, ui.make_persistent_id("solo pill"), Sense::click());
    painter.rect_filled(back, style::RADIUS, if r.hovered() { style::RAISED } else { style::PILL });
    painter.galley(at + egui::vec2(8.0, 4.0), galley, style::TEXT_STRONG);
    if r.on_hover_text("The preview shows only the soloed lights. Click to see every light again.").clicked() {
        studio.light_solo.clear();
    }
}

/// "Aiming… 37%" in the picture's top-left corner while the background aiming holds it back.
fn aiming_pill(painter: &egui::Painter, rect: Rect, done: f32) {
    pill(painter, rect, &format!("Aiming\u{2026} {:.0}%", (done * 100.0).floor()));
}

/// What the preview says over a scene with nothing in it yet.
pub const EMPTY_HINT: &str = "Nothing here yet.\nAdd a thing with + Add thing at the top of the list,\nor drop a scene, a font or a PNG picture here.";
/// What the preview says while files are held over the window.
pub const DROP_HINT: &str = "Drop to open it";

/// Whether the scene has nothing but its settings and cameras in it.
pub fn is_empty_scene(scene: &Scene) -> bool {
    scene.things.values().all(|t| t.id == bpe_core::ThingId::ROOT || t.kind == "camera")
}

/// `text` in a pill in the middle of `rect`.
fn middle_note(painter: &egui::Painter, rect: Rect, text: &str) {
    let galley = painter.layout(
        text.to_owned(),
        FontId::proportional(14.0),
        style::TEXT_STRONG,
        (rect.width() - 40.0).max(80.0),
    );
    let size = galley.size() + egui::vec2(24.0, 16.0);
    let back = Rect::from_center_size(rect.center(), size);
    painter.rect_filled(back, style::RADIUS, style::PILL);
    painter.galley(back.min + egui::vec2(12.0, 8.0), galley, style::TEXT_STRONG);
}

/// `text` in a pill in the picture's top-left corner.
fn pill(painter: &egui::Painter, rect: Rect, text: &str) {
    let text = text.to_owned();
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), style::TEXT_STRONG);
    let at = rect.min + egui::vec2(10.0, 10.0);
    let back = Rect::from_min_size(at, galley.size() + egui::vec2(16.0, 8.0));
    painter.rect_filled(back, style::RADIUS, style::PILL);
    painter.galley(at + egui::vec2(8.0, 4.0), galley, style::TEXT_STRONG);
}

/// The launch arrow's tip of a Balls thing, in world pixels.
fn arrow_tip(scene: &Scene, resolved: &Resolved, id: ThingId) -> Option<Vec2> {
    let thing = scene.things.get(&id)?;
    let r = resolved.things.get(&id)?;
    (r.kind == "balls")
        .then(|| thing.transform.position + BallsSettings::from_params(&r.params).velocity * ARROW_SECONDS)
}

/// Where a thing is on screen right now: its pose in the world, or its own position.
fn place_of(scene: &Scene, world: &World, id: ThingId) -> Option<Vec2> {
    world.poses.get(&id).map(|p| p.position).or_else(|| scene.things.get(&id).map(|t| t.transform.position))
}

/// The pose a rider's offset is measured from: its parent's, or one ball of its parent's.
fn parent_pose(scene: &Scene, world: &World, parent: ThingId, item: Option<u32>) -> Pose {
    // `item` is the ball's number in its group, as the world's ride reads it, not its row.
    if let Some(i) = item
        && let Some(b) = world.bodies.get(&parent)
        && let Some(row) = b.row_of(i)
    {
        return Pose { position: b.position[row], ..Pose::default() };
    }
    world
        .poses
        .get(&parent)
        .copied()
        .or_else(|| scene.things.get(&parent).map(|t| Pose::from_transform(&t.transform)))
        .unwrap_or_default()
}

/// What is under `pos`: the selected balls' arrow tip first, then a ball, then a selected thing
/// with nothing of its own to show, then the selected shape's handles, then the Sound tab's zone
/// lines, then any shape itself.
pub fn hit_test(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let world: &World = studio.playback.world();
    // Each thing through its own view: stuck things aren't seen through the camera.
    let seen = |id: &ThingId| {
        if studio.scene.things.get(id).is_some_and(|t| t.enabled) { view.of(studio, *id) } else { None }
    };
    // Only the selected thing shows its launch arrow.
    if let Some(id) = studio.selection.primary.as_ref()
        && let Some(v) = seen(id)
        && let Some(tip) = arrow_tip(&studio.scene, &studio.resolved, *id)
        && (v.to_screen(tip) - pos).length() <= TIP_GRAB
    {
        return Some(Handle::Aim(*id));
    }
    // The selected light's handles before the balls: a lamp riding a ball sits on it.
    if let Some(handle) = light_handles::hit(studio, view, pos, light_handles::Pass::Selected) {
        return Some(handle);
    }
    // The selected Particles thing's dots.
    if let Some(handle) = particle_handles::hit(studio, view, pos) {
        return Some(handle);
    }
    // The selected Text's size dot, wrap bar and radius ring.
    if let Some(handle) = text_handles::hit(studio, view, pos) {
        return Some(handle);
    }
    // The selected shape's or background's free gradient points, small and over everything.
    if let Some(handle) = free_points::hit(studio, view, pos) {
        return Some(handle);
    }
    let mut best: Option<(f32, ThingId)> = None;
    for (id, bodies) in world.bodies.iter() {
        let Some(v) = seen(id) else { continue };
        let p = v.to_world(pos);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let grab = (bodies.radius[i] * 1.5).max(8.0 / v.world_scale());
            let distance = (bodies.position[i] - p).length() * v.camera.zoom;
            if distance <= grab * v.camera.zoom && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, *id));
            }
        }
    }
    if let Some((_, id)) = best {
        return Some(Handle::Body(id));
    }
    if let Some(id) = studio.selection.primary.filter(|id| spot_only(studio, world, *id))
        && let Some(v) = seen(&id)
        && let Some(place) = place_of(&studio.scene, world, id)
        && (v.to_screen(place) - pos).length() <= SPOT_GRAB
    {
        return Some(Handle::Body(id));
    }
    if let Some(id) = studio.selection.primary
        && let Some(v) = seen(&id)
        && let Some(handle) = shape_handles::hit_handle(world, &v, id, pos)
    {
        return Some(handle);
    }
    // The selected light's reach ring and cone edges cross balls: they come after them.
    if let Some(handle) = light_handles::hit(studio, view, pos, light_handles::Pass::SelectedLines) {
        return Some(handle);
    }
    // The Sound tab's zone lines come after the balls and the shape's own handles.
    if let Some(zone) = zones::hit(studio, view, pos) {
        return Some(zone);
    }
    // The other lights' lamps while the Lights tab is open.
    if let Some(handle) = light_handles::hit(studio, view, pos, light_handles::Pass::Others) {
        return Some(handle);
    }
    // Words, then shapes.
    if let Some(id) = text_handles::under(studio, view, pos) {
        return Some(Handle::Body(id));
    }
    world
        .walls
        .keys()
        .find(|id| seen(id).is_some_and(|v| shape_handles::on_body(world, &v, **id, pos)))
        .map(|id| Handle::Body(*id))
}

/// A thing with no balls or wall of its own, such as a folder. The camera
/// has its own frame to grab instead.
fn spot_only(studio: &Studio, world: &World, id: ThingId) -> bool {
    let kind = studio.scene.things.get(&id).map(|t| t.kind.as_str());
    // An emitter can be grabbed at its middle even with no balls out.
    // A light's place is its own Position setting (its handles come with the Lights tab).
    // A rig only groups lights: it has no place to drag.
    let light = kind == Some(bpe_kit::things::light::LIGHT) || kind == Some(bpe_kit::things::rig::RIG);
    kind == Some(EMITTER)
        || (!world.bodies.contains_key(&id) && !world.walls.contains_key(&id) && kind != Some("camera") && !light)
}

/// The emitter kind.
const EMITTER: &str = bpe_kit::things::emitter::KIND;

/// How near, in world pixels at zoom 1, a dragged emitter must come to a focus to sit on it.
pub const SEAT_SNAP: f32 = 48.0;

/// The focus of a shape an emitter dropped at `place` sits on: the shape and which focus, when
/// one is within [`SEAT_SNAP`] (shrunk by the camera's zoom), nearest first.
pub fn seat_near(scene: &Scene, world: &World, place: Vec2) -> Option<(ThingId, usize)> {
    let reach = SEAT_SNAP / world.camera.zoom.max(1e-3);
    let mut best: Option<(f32, ThingId, usize)> = None;
    for id in world.walls.keys() {
        if !scene.things.get(id).is_some_and(bpe_kit::things::emitter::foci::can_sit_on) {
            continue;
        }
        let Some(foci) = bpe_kit::things::shape::foci(world, *id) else { continue };
        for (k, f) in foci.iter().enumerate() {
            let d = f.distance(place);
            if d <= reach && best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, *id, k));
            }
        }
    }
    best.map(|(_, id, k)| (id, k))
}

/// Moves, aims or moulds by a drag to `pointer`, in world pixels. A rider moves by its offset
/// from the parent it rides, so it stays a rider; its own position is left alone. With a grid
/// `step`, the thing's middle, the arrow tip and a shape's corners and size land on the grid. A
/// keyframed circle's Size gets a key at `time`; true when a key was set.
pub fn apply_drag(
    scene: &mut Scene,
    reg: &Registry,
    world: &World,
    drag: &Drag,
    pointer: Vec2,
    step: Option<f32>,
    time: f64,
) -> bool {
    let id = drag.handle.thing();
    let place = snap_point(pointer - drag.offset, step);
    if let Handle::Body(_) = drag.handle
        && let Some(thing) = scene.things.get(&id)
        && let (Some(parent), Attach::Ride { turn_with_parent, item, .. }) = (thing.parent, thing.attach)
    {
        let anchor = parent_pose(scene, world, parent, item);
        let turn = |v: Vec2, angle: f32| if turn_with_parent { Vec2::from_angle(angle).rotate(v) } else { v };
        // Where it rode when grabbed, seen on the parent as it is now, moved with the pointer.
        let place = match drag.ride {
            Some((press, start)) => snap_point(anchor.position + turn(start, anchor.angle) + (pointer - press), step),
            None => place,
        };
        let offset = turn(place - anchor.position, -anchor.angle);
        let thing = scene.things.get_mut(&id).expect("looked up above");
        thing.attach = Attach::Ride { offset: offset.round(), turn_with_parent, item };
        return false;
    }
    let Some(thing) = scene.things.get_mut(&id) else { return false };
    match drag.handle {
        // The camera's handles are the World view's; see `world_view::apply_camera_drag`.
        Handle::CameraMove(_) | Handle::CameraZoom(_) | Handle::CameraTurn(_) => {}
        Handle::Body(_) if thing.kind == EMITTER => {
            // Dropped on a focus it sits there; dragged off one it stands where it is dropped.
            match seat_near(scene, world, place) {
                Some((shape, focus)) => {
                    let thing = scene.things.get_mut(&id).expect("looked up above");
                    thing.params.set("sits_on", Value::Int(shape.0 as i64));
                    let focus = if focus == 0 { "focus1" } else { "focus2" };
                    thing.params.set("sits_focus", Value::Choice(focus.into()));
                }
                None => {
                    let thing = scene.things.get_mut(&id).expect("looked up above");
                    thing.params.set("sits_on", Value::Int(0));
                    thing.transform.position = place.round();
                }
            }
        }
        Handle::Body(_) => thing.transform.position = place.round(),
        Handle::Aim(_) => {
            let tip = snap_point(pointer, step);
            let velocity = ((tip - thing.transform.position) / ARROW_SECONDS).round();
            thing.params.set("velocity", Value::Vec2([velocity.x, velocity.y]));
        }
        Handle::Corner(..) | Handle::Side(..) | Handle::Bend(..) | Handle::Radius(_) | Handle::Axis(..) => {
            return shape_handles::apply(scene, reg, drag, pointer, step, time);
        }
        // Applied by the preview itself, with the playhead (see `zones::apply`).
        Handle::Zone(..) | Handle::FreePoint(..) => {}
        Handle::Light(id, part) => {
            return match drag.light.as_ref() {
                Some(grab) => light_handles::apply(scene, reg, id, part, grab, pointer, step, time),
                None => false,
            };
        }
        Handle::Particles(id, part) => return particle_handles::apply(scene, reg, world, id, part, pointer, time),
        Handle::Text(id, part) => return text_handles::apply(scene, reg, world, id, part, pointer, time),
    }
    false
}

/// Whether shape `id` has sound going into bus `n` (1..=3): its main bus, or a send that is up.
pub fn plays_into(scene: &Scene, id: ThingId, n: u8) -> bool {
    let Some(card) = scene.card(id, bpe_core::SlotId::Sound).filter(|c| c.kind == bpe_kit::sound::VOICE) else {
        return false;
    };
    let number = |key: &str| match card.params.get(key) {
        Some(Value::Int(i)) => Some(*i as f64),
        Some(Value::Number(x)) => Some(*x),
        _ => None,
    };
    let main = number("out_bus").map_or(1, |b| b.round() as i64);
    main == n as i64 || number(&format!("out_send{n}")).is_some_and(|db| bpe_sound::sends(db as f32))
}

fn draw_handles(ui: &Ui, studio: &Studio, view: &View, active: Option<Handle>, selected: Option<ThingId>) {
    let painter = ui.painter_at(view.rect);
    let faint = style::FAINT_MARK;
    let world = studio.playback.world();
    let shown = |id: &ThingId| studio.scene.things.get(id).is_some_and(|t| t.enabled);

    let bus = studio.panels.mixer.hover_bus;
    for id in world.walls.keys().filter(|id| shown(id)) {
        let Some(view) = view.of(studio, *id) else { continue };
        let lit = bus.is_some_and(|n| plays_into(&studio.scene, *id, n));
        shape_handles::draw(&painter, &studio.scene, world, &view, *id, selected == Some(*id), active, lit);
    }
    zones::draw(&painter, studio, view);

    // A selected thing with nothing of its own to show, such as a folder: a dashed ring and its
    // name, so it can still be found and dragged.
    if let Some(id) = selected.filter(|id| shown(id) && studio.scene.things.get(id).is_some_and(|t| t.kind == EMITTER))
        && let Some(view) = view.of(studio, id)
        && let Some(place) = place_of(&studio.scene, world, id)
    {
        let color = if active.is_some_and(|h| h.thing() == id) {
            style::TEXT_STRONG
        } else {
            style::colour_of(&studio.scene, id)
        };
        emitter_overlay(&painter, studio, &view, id, place, color);
        // Dragged near a focus, the focus it will sit on.
        if active == Some(Handle::Body(id))
            && let Some((shape, k)) = seat_near(&studio.scene, world, place)
            && let Some(foci) = bpe_kit::things::shape::foci(world, shape)
        {
            painter.circle_stroke(view.to_screen(foci[k]), 10.0, Stroke::new(2.0, style::TEXT_STRONG));
        }
    } else if let Some(id) = selected.filter(|id| shown(id) && spot_only(studio, world, *id))
        && let Some(view) = view.of(studio, id)
        && let Some(thing) = studio.scene.things.get(&id)
        && let Some(place) = place_of(&studio.scene, world, id)
    {
        let center = view.to_screen(place);
        let color = if active.is_some_and(|h| h.thing() == id) {
            style::TEXT_STRONG
        } else {
            style::colour_of(&studio.scene, id)
        };
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
        let Some(view) = view.of(studio, *id) else { continue };
        let highlighted = selected == Some(*id) || active.is_some_and(|h| h.thing() == *id);
        let mark = style::colour_of(&studio.scene, *id);
        let color = if highlighted { mark } else { faint };
        let center = view.to_screen(place_of(&studio.scene, world, *id).unwrap_or(thing.transform.position));
        painter.circle_stroke(center, 6.0, Stroke::new(1.2, color));
        // The launch arrow shows only on the selected balls (or while it is being dragged).
        let aim_active = active == Some(Handle::Aim(*id));
        if selected != Some(*id) && !aim_active {
            continue;
        }
        let Some(tip) = arrow_tip(&studio.scene, &studio.resolved, *id) else { continue };
        let tip = view.to_screen(tip);
        let arrow_color = if aim_active {
            style::TEXT_STRONG
        } else if highlighted {
            mark
        } else {
            style::TEXT
        };
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
        painter.circle(tip, if aim_active { 7.0 } else { 5.5 }, arrow_color, Stroke::new(1.5, style::OUTLINE));
        if aim_active || highlighted {
            let speed =
                studio.resolved.things.get(id).map_or(0.0, |r| BallsSettings::from_params(&r.params).velocity.length());
            painter.text(
                tip + egui::vec2(10.0, -10.0),
                Align2::LEFT_BOTTOM,
                format!("{speed:.0} px/s"),
                FontId::proportional(12.0),
                style::TEXT_STRONG,
            );
        }
    }
    light_handles::draw(&painter, studio, view, active, selected);
    free_points::draw(&painter, studio, view, active);
    particle_handles::draw(&painter, studio, view, active);
    text_handles::draw(&painter, studio, view, active);
    air_view::draw(&painter, studio, view);
}

/// The most found ways a selected emitter's latest round shows in the preview.
const MOST_PATHS: usize = 400;

/// A selected emitter's catch circle (dashed) and the ring or line its balls start from, turned
/// as it is.
fn emitter_overlay(
    painter: &egui::Painter,
    studio: &Studio,
    view: &View,
    id: ThingId,
    place: Vec2,
    color: egui::Color32,
) {
    use bpe_kit::things::emitter::{Catches, EmitterSettings, StartFrom};
    let Some(r) = studio.resolved.things.get(&id) else { return };
    let s = EmitterSettings::from_params(&r.params);
    let angle = studio.playback.world().poses.get(&id).map_or(0.0, |p| p.angle);
    let at = |v: Vec2| view.to_screen(place + Vec2::from_angle(angle).rotate(v));
    let circle = |radius: f32| -> Vec<Pos2> {
        (0..=64).map(|i| at(Vec2::from_angle(i as f32 / 64.0 * std::f32::consts::TAU) * radius)).collect()
    };
    if s.catches != Catches::Nothing {
        painter.add(egui::Shape::dashed_line(&circle(s.catch_size.max(0.0)), Stroke::new(1.2, color), 6.0, 4.0));
    }
    match s.start_from {
        StartFrom::Point => {}
        StartFrom::Ring => {
            painter.add(egui::Shape::line(circle(s.start_radius.max(0.0)), Stroke::new(1.0, style::FAINT_MARK)));
        }
        StartFrom::Line => {
            let across = Vec2::from_angle(s.aim_angle.to_radians()).perp() * s.start_length.max(0.0) * 0.5;
            painter.line_segment([at(-across), at(across)], Stroke::new(1.0, style::FAINT_MARK));
        }
    }
    painter.circle_stroke(at(Vec2::ZERO), 9.0, Stroke::new(1.5, color));
    // Aimed through bounces: the latest round's ways as its test flights found them (worked out
    // once a round, never each frame), and where they meet.
    if s.aim == bpe_kit::things::emitter::Aim::ThroughBounces {
        let world = studio.playback.world();
        if let Some(round) = bpe_kit::things::emitter::sim::emitter_state(world, id).and_then(|st| st.aimed.last()) {
            for ball in round.balls.iter().flatten().filter(|b| b.reaches).take(MOST_PATHS) {
                let points: Vec<Pos2> = ball.path.iter().map(|p| view.to_screen(*p)).collect();
                painter.add(egui::Shape::line(points, Stroke::new(1.0, style::FAINT_MARK)));
            }
            painter.circle_stroke(view.to_screen(round.meet), 7.0, Stroke::new(1.5, color));
        }
    // Aimed: the next round's ways to the target, faint, and where they meet.
    } else if s.aim != bpe_kit::things::emitter::Aim::Off {
        use bpe_kit::things::emitter::aim;
        let world = studio.playback.world();
        let (aimer, balls) = aim::next_round(world, id, aim::MOST_CHECKED);
        if let Some(aimer) = aimer {
            for ball in balls.iter().filter(|b| b.reaches) {
                let points: Vec<Pos2> =
                    aim::path_points(&aimer, ball, aim::PATH_POINTS).into_iter().map(|p| view.to_screen(p)).collect();
                painter.add(egui::Shape::line(points, Stroke::new(1.0, style::FAINT_MARK)));
            }
            let meet = aimer.target_at((world.step + 1 + aimer.arrive) as f64);
            painter.circle_stroke(view.to_screen(meet), 7.0, Stroke::new(1.5, color));
        }
    }
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
            if !draw_picture(studio, &gpu.0, &gpu.1, renderer, &mut frame, drawn, None) {
                break;
            }
        }
        renderer.read_output(&gpu.0, &gpu.1).unwrap()
    }

    #[test]
    fn a_lit_picture_waits_for_the_lighting_shaders_then_draws_lit() {
        let Ok(gpu) = bpe_gfx::test_device() else { return };
        let mut studio = Studio::headless();
        let mut scene = studio.scene.clone();
        let kind = bpe_kit::things::light::LightType::Spot;
        bpe_kit::things::light::add_light(&mut scene, &studio.kit.registry, kind, ThingId::ROOT).unwrap();
        studio.replace_scene(scene, None);
        studio.seek(0.5);
        // As the editor draws: shaders built on their own thread.
        let store = bpe_gfx::ProgramStore::in_background();
        let mut renderer = Renderer::new(&gpu.0, 135, 240);
        renderer.share_programs(&store);
        let mut drawn = None;
        let first = shown(&mut studio, &gpu, &mut renderer, &mut drawn);
        // As an export draws: built at once.
        let mut now = Renderer::new(&gpu.0, 135, 240);
        let lit = shown(&mut studio, &gpu, &mut now, &mut None);
        if renderer.programs_waiting() {
            assert_ne!(first, lit, "drawn unlit while the lighting builds");
            assert!(redraw_when_built(&renderer, &egui::Context::default()), "and drawn again soon");
        }
        store.wait();
        drawn = None;
        let again = shown(&mut studio, &gpu, &mut renderer, &mut drawn);
        assert!(!renderer.programs_waiting());
        assert_eq!(again, lit, "once built, the picture is the export's");
    }

    /// Runs one editor frame around `edit` the way the panels do: the tree and the inspector
    /// change the scene before the preview is drawn, and the edit is resolved at the frame's
    /// end. Then the next frame draws. Returns that picture and a clean render of the edited
    /// scene, which it must match.
    fn edit_then_draw(edit: impl Fn(&mut Scene)) -> Option<(Vec<u8>, Vec<u8>)> {
        let gpu = bpe_gfx::test_device().ok()?;
        let mut studio = Studio::headless();
        studio.seek(1.0);
        let mut renderer = Renderer::new(&gpu.0, 135, 240);
        let mut drawn = None;
        shown(&mut studio, &gpu, &mut renderer, &mut drawn);

        let before = studio.scene.clone();
        edit(&mut studio.scene);
        draw_picture(&mut studio, &gpu.0, &gpu.1, &mut renderer, &mut Frame::default(), &mut drawn, None);
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
    fn changing_the_shape_in_the_inspector_redraws_it() {
        let Some((after, expected)) = edit_then_draw(|scene| {
            scene.things.get_mut(&ThingId(1)).unwrap().params.set("start", Value::Choice("polygon".into()));
        }) else {
            return eprintln!("no GPU; skipped");
        };
        assert!(after == expected, "the preview still shows the shape from before the edit");
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
    fn hit_testing_finds_balls_before_shapes() {
        let mut studio = Studio::headless();
        let view = View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        };
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, -200.0))), Some(Handle::Body(ThingId(2))));
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(508.0, 0.0))), Some(Handle::Body(ThingId(1))));
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::new(0.0, 300.0))), None);
        // The arrow tip sits a quarter second of launch speed from the ball, and only the
        // selected balls show it.
        let tip = view.to_screen(Vec2::new(75.0, -250.0));
        studio.selection.clear();
        assert_eq!(hit_test(&studio, &view, tip), None, "an unselected ball shows no arrow");
        studio.selection.set(ThingId(2));
        assert_eq!(hit_test(&studio, &view, tip), Some(Handle::Aim(ThingId(2))));
    }

    #[test]
    fn dragging_the_far_line_sets_far() {
        let mut studio = Studio::headless();
        let reg = bpe_kit::registry();
        let shape = ThingId(1);
        assert!(crate::sound_tab::write::enable(&mut studio.scene, &reg, shape));
        let card = studio.scene.card_mut(shape, bpe_core::SlotId::Sound).unwrap();
        let distance = card.layers.iter().find(|l| l.kind == bpe_kit::sound::DISTANCE).unwrap().id;
        bpe_kit::sound::turn_on_distance(card, distance);
        studio.selection.set(shape);
        let view = View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        };
        let on_far = view.to_screen(Vec2::new(200.0, 0.0));
        assert_ne!(
            hit_test(&studio, &view, on_far),
            Some(Handle::Zone(shape, distance, true)),
            "only on the Sound tab"
        );
        studio.panels.inspector.tab.insert(shape, crate::inspector::InspectorTab::Sound);
        assert_eq!(hit_test(&studio, &view, on_far), Some(Handle::Zone(shape, distance, true)));
        let middle = view.to_screen(Vec2::new(0.0, 0.0));
        assert_ne!(
            hit_test(&studio, &view, middle),
            Some(Handle::Zone(shape, distance, false)),
            "Near is 0 (the middle): not drawn, so nothing to grab"
        );
        // A ball, or the shape's own handles, come before a zone line on them.
        let key = format!("{distance}/far");
        let card = studio.scene.card_mut(shape, bpe_core::SlotId::Sound).unwrap();
        card.params.set(&key, Value::Number(508.0));
        let edge = view.to_screen(Vec2::new(508.0, 0.0));
        assert_eq!(hit_test(&studio, &view, edge), Some(Handle::Radius(shape)), "the shape's edge handle wins");
        let card = studio.scene.card_mut(shape, bpe_core::SlotId::Sound).unwrap();
        card.params.set(&key, Value::Number(200.0));
        let on_near = view.to_screen(Vec2::new(0.0, 50.0));
        card.params.set(&format!("{distance}/near"), Value::Number(50.0));
        assert_eq!(hit_test(&studio, &view, on_near), Some(Handle::Zone(shape, distance, false)));
        let world = studio.playback.world().clone();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let keyed = zones::apply(&mut studio.scene, reg, &world, shape, distance, true, Vec2::new(0.0, 300.0), 0.0);
        assert!(!keyed);
        let far =
            studio.scene.card(shape, bpe_core::SlotId::Sound).unwrap().params.get(&format!("{distance}/far")).cloned();
        assert_eq!(far, Some(Value::Number(300.0)));
        // Keyed: the drag keys at the playhead, and the line shows where it was dragged.
        let a = bpe_core::Address::on(shape, bpe_core::SlotId::Sound, &format!("{distance}/far"));
        studio.scene.set_key(reg, &a, 0.0, Value::Number(500.0)).unwrap();
        let keyed = zones::apply(&mut studio.scene, reg, &world, shape, distance, true, Vec2::new(0.0, 250.0), 2.0);
        assert!(keyed, "it keys");
        assert_eq!(studio.scene.value_at_time(reg, &a, 2.0), Some(Value::Number(250.0)));
    }

    #[test]
    fn a_shape_s_free_points_show_as_dots_that_drag_its_stops_points() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let shape = ThingId(1);
        let paint =
            studio.scene.add_layer(reg, shape, bpe_core::SlotId::Look, bpe_kit::modules::look::shape::PAINT).unwrap();
        let (red, blue) = (bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 0, 255));
        let card = studio.scene.card_mut(shape, bpe_core::SlotId::Look).unwrap();
        let colours = bpe_core::Gradient::even(&[red, bpe_core::Color::WHITE, blue]);
        card.params.set(&bpe_core::layered_key(paint, "colours"), Value::Gradient(colours));
        studio.selection.set(shape);
        let view = View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        };
        assert!(free_points::dots_for_test(&studio).is_empty(), "Straight: no dots");
        let form = bpe_core::layered_key(paint, "colours~form");
        let card = studio.scene.card_mut(shape, bpe_core::SlotId::Look).unwrap();
        card.params.set(&form, Value::Choice("free_points".into()));
        let dots = free_points::dots_for_test(&studio);
        assert_eq!(dots.len(), 3, "a dot per stop");
        let first = view.to_screen(dots[0]);
        assert_eq!(hit_test(&studio, &view, first), Some(Handle::FreePoint(shape, 0, 0)));
        let world = studio.playback.world().clone();
        let to = Vec2::new(100.0, 50.0);
        let keyed = free_points::apply(&mut studio.scene, reg, &world, shape, 0, 0, to, 0.0);
        assert!(!keyed);
        let points = studio
            .scene
            .card(shape, bpe_core::SlotId::Look)
            .unwrap()
            .params
            .get(&bpe_core::layered_key(paint, "colours~points"))
            .cloned();
        let Some(Value::Points(points)) = points else { panic!("the points are written: {points:?}") };
        assert_eq!(points.len(), 3, "the whole list, one point per stop");
        let moved = free_points::dots_for_test(&studio);
        assert!((moved[0] - to).length() < 6.0, "the first dot is where it was dragged: {:?}", moved[0]);
        assert!((moved[1] - dots[1]).length() < 6.0, "the others stay");
    }

    /// A drag on the blank project's circle by its edge.
    #[test]
    fn an_emitter_dropped_on_a_circles_middle_sits_there_and_dragged_off_stands_free() {
        let mut studio = Studio::headless();
        let reg = &studio.kit.registry;
        let id = bpe_kit::add_thing(&mut studio.scene, reg, bpe_kit::things::emitter::KIND, ThingId::ROOT).unwrap();
        let world = studio.playback.world().clone();
        assert!(spot_only(&studio, &world, id), "it can be grabbed at its middle with no balls out");
        let drag = Drag::new(Handle::Body(id), Vec2::ZERO);
        apply_drag(&mut studio.scene, &studio.kit.registry, &world, &drag, Vec2::new(12.0, -9.0), None, 0.0);
        let thing = &studio.scene.things[&id];
        assert_eq!(thing.params.get("sits_on"), Some(&Value::Int(1)));
        assert_eq!(thing.params.get("sits_focus"), Some(&Value::Choice("focus1".into())));
        apply_drag(&mut studio.scene, &studio.kit.registry, &world, &drag, Vec2::new(300.0, 250.0), None, 0.0);
        let thing = &studio.scene.things[&id];
        assert_eq!(thing.params.get("sits_on"), Some(&Value::Int(0)));
        assert_eq!(thing.transform.position, Vec2::new(300.0, 250.0));
        assert_eq!(seat_near(&studio.scene, &world, Vec2::new(0.0, SEAT_SNAP + 1.0)), None);
    }

    fn radius_drag(world: &World) -> Drag {
        let handle = Handle::Radius(ThingId(1));
        Drag { mould: shape_handles::start(world, handle, Vec2::new(508.0, 0.0)), ..Drag::new(handle, Vec2::ZERO) }
    }

    #[test]
    fn dragging_moves_aims_and_resizes() {
        let mut studio = Studio::headless();
        let world = studio.playback.world().clone();
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Body(ThingId(2)), Vec2::ZERO),
            Vec2::new(50.0, 60.0),
            None,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(50.0, 60.0));
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Aim(ThingId(2)), Vec2::ZERO),
            Vec2::new(150.0, 60.0),
            None,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([400.0, 0.0])));
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &radius_drag(&world),
            Vec2::new(300.0, 400.0),
            None,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(1)].params.get("size"), Some(&Value::Number(500.0)));
    }

    #[test]
    fn dragging_a_rider_moves_its_offset_and_a_selected_folder_is_grabbable() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let ride = Attach::Ride { offset: Vec2::new(0.0, -200.0), turn_with_parent: true, item: None };
        studio.scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        studio.after_edit(before);
        let view = View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        };
        let world = studio.playback.world().clone();
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Body(ThingId(2)), Vec2::ZERO),
            Vec2::new(30.0, -100.0),
            None,
            0.0,
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
    fn with_a_grid_drags_land_on_it() {
        let mut studio = Studio::headless();
        let world = studio.playback.world().clone();
        let step = Some(60.0);
        // Grabbed 7 px off the middle: the middle still lands on a grid point.
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Body(ThingId(2)), Vec2::new(7.0, -3.0)),
            Vec2::new(85.0, 50.0),
            step,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].transform.position, Vec2::new(60.0, 60.0));
        // The arrow tip lands on a grid point: 60 px right of the ball over a quarter second.
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Aim(ThingId(2)), Vec2::ZERO),
            Vec2::new(131.0, 52.0),
            step,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(2)].params.get("velocity"), Some(&Value::Vec2([240.0, 0.0])));
        // A circle's radius grows in whole steps.
        apply_drag(
            &mut studio.scene,
            &studio.kit.registry,
            &world,
            &radius_drag(&world),
            Vec2::new(0.0, 455.0),
            step,
            0.0,
        );
        assert_eq!(studio.scene.things[&ThingId(1)].params.get("size"), Some(&Value::Number(480.0)));
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
            &studio.kit.registry,
            &world,
            &Drag::new(Handle::Body(ThingId(2)), Vec2::ZERO),
            Vec2::new(70.0, -110.0),
            Some(60.0),
            0.0,
        );
        let thing = &studio.scene.things[&ThingId(2)];
        assert!(
            matches!(thing.attach, Attach::Ride { offset, .. } if offset == Vec2::new(45.0, -120.0)),
            "the world spot (60, -120) less the shape at (15, 0): {:?}",
            thing.attach
        );
    }

    fn plain_view() -> View {
        View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        }
    }

    #[test]
    fn a_moved_stage_camera_maps_the_pointer_both_ways() {
        let layout = bpe_gfx::StageLayout { back: 240.0, k: 0.25, floor: Some(800.0) };
        let cam = bpe_gfx::StageCamera { orbit: 25.0, tilt: 10.0, ..bpe_gfx::StageCamera::default() };
        let stage = bpe_gfx::StageView::solve(&cam, &CameraView::default(), Vec2::new(1080.0, 1920.0), layout);
        let view = View { stage: Some(stage), ..plain_view() };
        for p in [Vec2::ZERO, Vec2::new(300.0, -500.0), Vec2::new(-200.0, 700.0)] {
            let back = view.to_world(view.to_screen(p));
            assert!(back.distance(p) < 0.5, "{p} came back as {back}");
        }
        // The swing moves where things show.
        let p = Vec2::new(300.0, -500.0);
        assert!(view.to_screen(p).distance(plain_view().to_screen(p)) > 2.0);
    }

    fn ride_offset(studio: &Studio, id: ThingId) -> Vec2 {
        match studio.scene.things[&id].attach {
            Attach::Ride { offset, .. } => offset,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_riding_ball_moves_with_the_pointer_from_where_it_rides() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId(1)).unwrap().transform.position = Vec2::new(15.0, 0.0);
        let ride = Attach::Ride { offset: Vec2::new(0.0, -200.0), turn_with_parent: false, item: None };
        studio.scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        studio.after_edit(before);
        let view = plain_view();
        // Grabbed where it is drawn, on the shape at (15, 0), and moved 10 px right.
        let press = Vec2::new(15.0, -200.0);
        let drag = begin_drag(&studio, &view, Handle::Body(ThingId(2)), view.to_screen(press));
        let world = studio.playback.world().clone();
        let kit = studio.kit.clone();
        apply_drag(&mut studio.scene, &kit.registry, &world, &drag, press + Vec2::new(10.0, 0.0), None, 0.0);
        assert_eq!(ride_offset(&studio, ThingId(2)), Vec2::new(10.0, -200.0), "it moved as far as the pointer");
    }

    #[test]
    fn a_rider_grabbed_later_in_the_run_moves_from_where_it_starts() {
        // A folder riding the ball, which falls: grabbed at 1 s, the drag goes on at the start.
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let before = studio.scene.clone();
        let folder = studio.scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
        let ride = Attach::Ride { offset: Vec2::new(0.0, -100.0), turn_with_parent: false, item: Some(0) };
        studio.scene.set_parent(folder, ThingId(2), ride).unwrap();
        studio.after_edit(before);
        studio.selection.set(folder);
        let at_start = studio.playback.world().poses[&folder].position;
        studio.playback.seek_within(1.0, None);
        let at_one = studio.playback.world().poses[&folder].position;
        assert!((at_one - at_start).length() > 50.0, "the ball has moved by 1 s: {at_start} {at_one}");
        let view = plain_view();
        let drag = begin_drag(&studio, &view, Handle::Body(folder), view.to_screen(at_one));
        studio.playback.seek_within(0.0, None);
        let start = studio.playback.world().clone();
        apply_drag(&mut studio.scene, &kit.registry, &start, &drag, at_one + Vec2::new(0.0, 20.0), None, 0.0);
        let offset = ride_offset(&studio, folder);
        assert!((offset - Vec2::new(0.0, -80.0)).length() <= 1.0, "moved 20 px down from where it rode: {offset}");
    }

    #[test]
    fn a_stuck_ball_is_grabbed_where_it_is_drawn() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let ball = studio.scene.things.get_mut(&ThingId(2)).unwrap();
        ball.screen = Some(true);
        ball.transform.position = Vec2::new(100.0, 300.0);
        studio.after_edit(before);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0));
        let camera = CameraView { center: Vec2::new(4000.0, 0.0), zoom: 2.0, turn: 0.0 };
        let shot = View { rect, scale: 0.25, camera, world: false, stage: None };
        // Drawn on the picture as if there were no camera.
        let drawn = View { camera: CameraView::default(), ..shot }.to_screen(Vec2::new(100.0, 300.0));
        assert_eq!(hit_test(&studio, &shot, drawn), Some(Handle::Body(ThingId(2))));
        assert_eq!(shot.pointer_for(&studio, Handle::Body(ThingId(2)), drawn), Vec2::new(100.0, 300.0));
        // Through the camera, where it isn't, there's nothing.
        assert_eq!(hit_test(&studio, &shot, shot.to_screen(Vec2::new(100.0, 300.0))), None);
        // The World view shows the world only.
        let free = View { world: true, ..shot };
        assert_eq!(hit_test(&studio, &free, free.to_screen(Vec2::new(100.0, 300.0))), None);
        assert_eq!(hit_test(&studio, &free, drawn), None);
    }

    #[test]
    fn a_selected_camera_has_no_spot_to_grab() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let cam = bpe_kit::ensure_camera(&mut studio.scene, &studio.kit.registry);
        studio.after_edit(before);
        studio.selection.primary = Some(cam);
        let view = View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        };
        assert_eq!(hit_test(&studio, &view, view.to_screen(Vec2::ZERO)).map(Handle::thing), None);
    }

    #[test]
    fn hit_test_under_turned_zoomed_camera() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId(2)).unwrap().transform.position = Vec2::new(520.0, 790.0);
        studio.after_edit(before);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0));
        for camera in [
            CameraView { center: Vec2::new(500.0, 800.0), zoom: 3.0, turn: 0.6 },
            CameraView { center: Vec2::new(500.0, 800.0), zoom: 0.05, turn: 0.6 },
        ] {
            let view = View { rect, scale: 0.25, camera, world: false, stage: None };
            let at = view.to_screen(Vec2::new(520.0, 790.0));
            // Zoomed far out, the launch arrow's tip sits on the ball too: either grabs it.
            assert_eq!(hit_test(&studio, &view, at).map(Handle::thing), Some(ThingId(2)), "{camera:?}");
            assert!((view.to_world(at) - Vec2::new(520.0, 790.0)).length() < 0.5);
        }
    }
}
