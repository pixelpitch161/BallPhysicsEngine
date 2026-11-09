//! The ball view at the top of the Look editor: the thing's look on one ball, still, moving or
//! hitting something, on a dark ground or the scene's own. It builds a frame for a small scene
//! of its own (the ball alone) and never steps the physics. The small scene is resolved, hashed
//! and built into a world only when it changes ([`MiniCache`]); each picture after that only
//! moves the ball, so Moving and Hit cost a frame build a picture, not a world.

use std::hash::{Hash, Hasher};

use bpe_core::{Address, Color, Resolved, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{CameraView, Event, EventKind, Log, Party, World};
use eframe::{egui, egui_wgpu, wgpu};
use egui::{Pos2, Rect, Ui};
use glam::Vec2;

use crate::style;

/// How the ball moves in the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    #[default]
    Still,
    /// Going sideways at [`MOVING_SPEED`], round a two second loop.
    Moving,
    /// Hitting something at the start of every [`HIT_LOOP`].
    Hit,
}

/// What is behind the ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Ground {
    #[default]
    Dark,
    /// The scene's own background, bloom and grade.
    Scene,
}

/// How fast the ball goes in Moving, in pixels per second.
pub const MOVING_SPEED: f32 = 700.0;
/// How long the Moving loop is, in seconds.
pub const MOVING_LOOP: f64 = 2.0;
/// How long the Hit loop is, in seconds.
pub const HIT_LOOP: f64 = 1.2;
/// How much of the view across the ball's radius takes.
pub const RADIUS_SHARE: f32 = 0.35;
/// The view's largest size, in points.
const MAX_SIZE: f32 = 300.0;
/// The dark ground.
pub(crate) const DARK: Color = Color::rgb(0x15, 0x15, 0x15); // scene colour
/// How fast the ball hits in Hit, in pixels per second (a full flash).
const HIT_SPEED: f32 = 600.0;

/// The ball view's state, one for the editor.
#[derive(Default)]
pub struct BallView {
    pub mode: ViewMode,
    pub ground: Ground,
    renderer: Option<(Renderer, egui::TextureId)>,
    frame: Frame,
    /// What the picture shows now, so a still one is drawn again only when that changes.
    drawn: Option<u64>,
    /// What the frame was built for, so it is built again only when that changes.
    built: Option<u64>,
    /// Where the picture was last drawn.
    rect: Option<Rect>,
    /// The brush strokes are painted with.
    pub brush: super::brush::Brush,
    /// The stroke being painted, until the button is let go.
    pub painting: Option<super::brush::Painting>,
    /// A layer whose cuts the view shows alone (white where they let it show, black where they
    /// hide it), until Alt-click again or another thing is picked.
    pub solo: Option<(ThingId, bpe_core::LayerId)>,
    /// The small scene last drawn and what was worked out from it.
    cache: MiniCache,
}

/// A view's small scene as last shown, resolved and hashed, and (for the ball view) the world
/// built from it: worked out again only when the small scene changes, not on every frame.
#[derive(Default)]
pub struct MiniCache {
    mini: Option<Scene>,
    pub resolved: Resolved,
    /// [`content_key`] of the small scene.
    pub content: u64,
    world: Option<World>,
    /// How many times it was worked out (tests).
    #[cfg(test)]
    pub builds: usize,
}

impl MiniCache {
    /// Takes `mini` (of thing `id`) as what the view shows now: resolved and hashed again only
    /// when it differs from the last one.
    pub fn update(&mut self, mini: Scene, kit: &Kit, id: ThingId) {
        if self.mini.as_ref() == Some(&mini) {
            return;
        }
        self.resolved = bpe_core::resolve(&mini, &kit.registry);
        self.content = content_key(&mini, &self.resolved, id);
        self.world = None;
        self.mini = Some(mini);
        #[cfg(test)]
        {
            self.builds += 1;
        }
    }

    /// The small scene, once [`MiniCache::update`] has been given one.
    pub fn mini(&self) -> Option<&Scene> {
        self.mini.as_ref()
    }
}

impl BallView {
    /// Where the picture was last drawn, in points.
    #[cfg(test)]
    pub fn last_rect(&self) -> Option<Rect> {
        self.rect
    }
}

/// The small scene the view draws: the scene's settings (or plain ones on a dark ground) and a
/// copy of `id` alone with its Look and nothing else. A group keeps its balls, every one let go
/// at once so each has its number in the group; the view shows one of them.
pub fn mini_scene(scene: &Scene, kit: &Kit, id: ThingId, ground: Ground) -> Option<Scene> {
    let reg = &kit.registry;
    let thing = scene.things.get(&id)?;
    let mut mini = Scene::empty(reg);
    let root = mini.things.get_mut(&ThingId::ROOT)?;
    match ground {
        Ground::Scene => root.params = scene.things.get(&ThingId::ROOT)?.params.clone(),
        Ground::Dark => {
            let plain = SceneSettings { background: bpe_kit::paint::Paint::solid(DARK), ..SceneSettings::default() };
            root.params = plain.to_params();
        }
    }
    let mut ball = thing.clone();
    ball.parent = Some(ThingId::ROOT);
    ball.attach = bpe_core::Attach::Group;
    ball.transform = bpe_core::Transform::default();
    ball.enabled = true;
    ball.screen = None;
    ball.slots.retain(|slot, _| *slot == SlotId::Look);
    ball.params.set("release", Value::Choice("all_at_once".into()));
    if ball.kind == bpe_kit::things::emitter::KIND {
        // Its first round, all at once at the start, wherever the emitter would be; no marker.
        for (key, v) in [
            ("emits", Value::Bool(true)),
            ("fire_when", Value::Choice("at_start".into())),
            ("start_delay", Value::Number(0.0)),
            ("gap", Value::Number(0.0)),
            ("sits_on", Value::Int(0)),
            ("catches", Value::Choice("nothing".into())),
            // Unaimed: aiming through bounces would test-fire, and fire on the first step.
            ("aim", Value::Choice("off".into())),
            ("marker", Value::Choice("none".into())),
            ("ripple", Value::Bool(false)),
        ] {
            ball.params.set(key, v);
        }
    }
    // The Look's keys and pulses still move it, and on the scene's ground the scene's own.
    let kept = |a: &Address| {
        (a.thing == id && a.slot == Some(SlotId::Look))
            || (ground == Ground::Scene && a.thing == ThingId::ROOT && a.slot.is_none())
    };
    mini.tracks = scene.tracks.iter().filter(|(a, _)| kept(a)).map(|(a, t)| (a.clone(), t.clone())).collect();
    mini.pulses = scene.pulses.iter().filter(|(a, _)| kept(a)).map(|(a, p)| (a.clone(), p.clone())).collect();
    mini.things.insert(id, ball);
    mini.order.push(id);
    mini.next_thing = mini.next_thing.max(id.0 + 1);
    Some(mini)
}

/// How many world pixels across the view shows for a ball of `radius`.
pub fn view_size(radius: f32) -> u32 {
    (radius.max(1.0) / RADIUS_SHARE).round().max(2.0) as u32
}

/// The frame of `mini` (from [`mini_scene`]) for ball `number` of thing `id` (0 for a single
/// ball) with radius `radius`, at `time` seconds into the scene (`clock` of them into the view's
/// loop, after `loops` whole loops: in Hit the ball has hit once a loop).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn build_view_frame(
    mini: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    id: ThingId,
    number: u32,
    radius: f32,
    mode: ViewMode,
    time: f64,
    clock: f64,
    loops: u32,
    frame: &mut Frame,
) {
    let mut world = World::build(mini, resolved, &kit.sims);
    build_view_frame_in(&mut world, mini, resolved, kit, id, number, radius, mode, time, clock, loops, frame);
}

/// [`build_view_frame`] in a `world` already built from `mini`: every ball's place, size, speed,
/// life and hit count is set afresh, so one world serves every picture of the same small scene.
#[allow(clippy::too_many_arguments)]
pub fn build_view_frame_in(
    world: &mut World,
    mini: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    id: ThingId,
    number: u32,
    radius: f32,
    mode: ViewMode,
    time: f64,
    clock: f64,
    loops: u32,
    frame: &mut Frame,
) {
    let mut log = Log::default();
    let rate = world.steps_per_second;
    // Every ball is let go at once, so a ball's row is its number.
    let shown = |n: usize| n.min(world.bodies.get(&id).map_or(1, |b| b.len()).max(1) - 1);
    let row = shown(number as usize);
    if let Some(bodies) = world.bodies.get_mut(&id) {
        for i in 0..bodies.len() {
            bodies.position[i] = Vec2::ZERO;
            bodies.radius[i] = radius;
            bodies.velocity[i] = if mode == ViewMode::Moving { Vec2::new(MOVING_SPEED, 0.0) } else { Vec2::ZERO };
            bodies.alive[i] = i == row;
            if i < bodies.hits.len() {
                bodies.hits[i] = 0;
            }
        }
        if mode == ViewMode::Hit && row < bodies.hits.len() {
            bodies.hits[row] = loops + 1;
            let start = time - clock;
            log.push(Event {
                step: (start * rate as f64).round().max(0.0) as u64,
                kind: EventKind::Hit,
                a: Party { thing: id, item: row as u32 },
                b: None,
                point: Vec2::new(0.0, radius),
                normal: Vec2::new(0.0, -1.0),
                speed: HIT_SPEED,
                at: 0.0,
            });
        }
    }
    build_frame(mini, resolved, world, &log, time, &kit.draws, frame);
    let size = view_size(radius);
    frame.set_view(CameraView::default(), (size, size));
    frame.overlay = None;
}

/// Where the ball sits in the view: ball space (radii, y down, before the ball turns) to view
/// points and back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewMap {
    pub centre: Pos2,
    /// The ball's radius in points.
    pub radius: f32,
    /// How far the ball has turned, in radians.
    pub turn: f32,
}

impl ViewMap {
    /// Ball point `q` in the view.
    pub fn to_view(self, q: Vec2) -> Pos2 {
        let v = Vec2::from_angle(self.turn).rotate(q) * self.radius;
        self.centre + egui::vec2(v.x, v.y)
    }

    /// View point `p` on the ball.
    pub fn to_ball(self, p: Pos2) -> Vec2 {
        let v = Vec2::new(p.x - self.centre.x, p.y - self.centre.y) / self.radius.max(1e-3);
        Vec2::from_angle(-self.turn).rotate(v)
    }
}

/// What the view gives the editor for handles, the brush and clicks.
pub struct ViewOut {
    /// The picture's place and what the pointer did on it.
    pub response: egui::Response,
    /// Where the ball is; `None` while it moves, when it has no handles.
    pub map: Option<ViewMap>,
    /// The ball's radius in pixels of the scene.
    pub radius: f32,
    /// The layer a click on the picture picked, when it was clicked.
    pub picked: Option<bpe_core::LayerId>,
}

/// Draws the view and its buttons for ball `number` of thing `id` of `scene` at the playhead
/// `time`, with layered key `swap.0` of its Look holding `swap.1` instead (a stroke being painted).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    view: &mut BallView,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    number: u32,
    time: f64,
    swap: Option<(&str, Value)>,
) -> Option<ViewOut> {
    let dress = |mini: &mut Scene| {
        if let Some((key, value)) = &swap
            && let Some(card) = mini.card_mut(id, SlotId::Look)
        {
            card.params.set(key, value.clone());
        }
    };
    show_dressed(ui, view, render, scene, kit, id, number, time, &dress)
}

/// [`show`] with the small scene changed by `dress` before it is drawn (the Looks tab tries a look on it).
#[allow(clippy::too_many_arguments)]
pub fn show_dressed(
    ui: &mut Ui,
    view: &mut BallView,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    number: u32,
    time: f64,
    dress: &dyn Fn(&mut Scene),
) -> Option<ViewOut> {
    buttons(ui, view);
    let side = ui.available_width().clamp(40.0, MAX_SIZE);
    let mut mini = mini_scene(scene, kit, id, view.ground)?;
    dress(&mut mini);
    view.cache.update(mini, kit, id);
    let radius = match scene.value_at_time(&kit.registry, &Address::own(id, "radius"), time) {
        Some(Value::Number(r)) => r as f32,
        _ => 24.0,
    };
    let pixels = (side * ui.ctx().pixels_per_point()).round().max(2.0) as u32;
    let looped = match view.mode {
        ViewMode::Still => None,
        ViewMode::Moving => Some(MOVING_LOOP),
        ViewMode::Hit => Some(HIT_LOOP),
    };
    let now = ui.input(|i| i.time);
    let clock = looped.map_or(0.0, |len| now % len);
    let loops = looped.map_or(0, |len| (now / len) as u32);
    let solo = view.solo.filter(|(t, _)| *t == id).map(|(_, layer)| layer);
    let key = frame_key(view.cache.content, radius, time, clock, pixels, view.mode, view.ground)
        ^ solo.map_or(0, |l| (u64::from(l.0) + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15))
        ^ u64::from(number).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    // The frame is built without a GPU too, so handles and clicks work everywhere.
    if view.built != Some(key) {
        let at = time + clock;
        let cache = &mut view.cache;
        let mini = cache.mini.as_ref().expect("given above");
        let world = cache.world.get_or_insert_with(|| World::build(mini, &cache.resolved, &kit.sims));
        let (resolved, mode) = (&cache.resolved, view.mode);
        build_view_frame_in(world, mini, resolved, kit, id, number, radius, mode, at, clock, loops, &mut view.frame);
        if let Some(layer) = solo {
            show_cuts_alone(&mut view.frame, resolved, id, layer);
        }
        view.built = Some(key);
    }
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    view.rect = Some(rect);
    match render {
        Some(render) => {
            let device = &render.device;
            let (renderer, texture) = view.renderer.get_or_insert_with(|| {
                let mut renderer = Renderer::new(device, pixels, pixels);
                renderer.share_programs(crate::preview::shared_programs());
                let texture = render.renderer.write().register_native_texture(
                    device,
                    renderer.output_view(),
                    wgpu::FilterMode::Linear,
                );
                (renderer, texture)
            });
            if renderer.size() != (pixels, pixels) {
                renderer.resize(device, pixels, pixels);
                render.renderer.write().update_egui_texture_from_wgpu_texture(
                    device,
                    renderer.output_view(),
                    wgpu::FilterMode::Linear,
                    *texture,
                );
                view.drawn = None;
            }
            if view.drawn != Some(key) {
                renderer.render(device, &render.queue, &view.frame);
                view.drawn = Some(key);
            }
            if crate::preview::redraw_when_built(renderer, ui.ctx()) {
                view.drawn = None;
            }
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.painter().image(*texture, rect, uv, style::UNTINTED);
        }
        None => {
            ui.painter().rect_filled(rect, 0.0, style::STAGE);
            let at = rect.center_bottom() - egui::vec2(0.0, 12.0);
            let font = egui::FontId::proportional(11.0);
            let words = "The ball view needs the wgpu renderer.";
            ui.painter().text(at, egui::Align2::CENTER_CENTER, words, font, style::MUTED);
        }
    }
    if looped.is_some() {
        // A moving ball needs a new picture about 30 times a second while the window is in
        // front; behind other windows it slows right down, so an idle editor stays quiet.
        let wait = if ui.input(|i| i.focused) { 33 } else { 250 };
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(wait));
    }
    let ball = view.frame.layered.first()?;
    let world = view_size(radius) as f32;
    let map = ViewMap { centre: rect.center(), radius: radius * side / world, turn: ball.form[2] };
    let picked = response.interact_pointer_pos().filter(|_| response.clicked()).and_then(|at| {
        let slot = view.cache.resolved.things.get(&id)?.slots.get(&SlotId::Look)?;
        bpe_kit::modules::look::layers::gpu::Stack::of(slot)?.topmost_at(map.to_ball(at), radius)
    });
    let map = (view.mode != ViewMode::Moving).then_some(map);
    Some(ViewOut { response, map, radius, picked })
}

/// Makes the frame of [`build_view_frame`] show layer `layer`'s cuts alone: white where they let
/// it show, black where they hide it. Nothing changes when the layer draws nothing.
pub fn show_cuts_alone(frame: &mut Frame, resolved: &Resolved, id: ThingId, layer: bpe_core::LayerId) {
    let Some(slot) = resolved.things.get(&id).and_then(|t| t.slots.get(&SlotId::Look)) else { return };
    let Some(stack) = bpe_kit::modules::look::layers::gpu::Stack::of(slot) else { return };
    let (Some(at), Some(ball)) = (stack.record_of(layer), frame.layered.first()) else { return };
    if let Some(record) = frame.look_layers.get_mut(ball.range[0] as usize + at) {
        record.head[2] |= bpe_gfx::LayerGpu::SOLO;
    }
}

/// What the view's picture shows: the ball's settings with their keys and pulses, the scene's
/// when on its ground, the size, the moment and the view's own choices. The picture is drawn
/// again only when this changes.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn view_key(
    mini: &Scene,
    resolved: &Resolved,
    id: ThingId,
    radius: f32,
    time: f64,
    clock: f64,
    pixels: u32,
    mode: ViewMode,
    ground: Ground,
) -> u64 {
    frame_key(content_key(mini, resolved, id), radius, time, clock, pixels, mode, ground)
}

/// What a small scene of thing `id` shows, whatever the moment: the thing's settings with their
/// keys and pulses, and the scene's when on its ground.
pub fn content_key(mini: &Scene, resolved: &Resolved, id: ThingId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", resolved.things.get(&id)).hash(&mut h);
    format!("{:?}", resolved.things.get(&ThingId::ROOT)).hash(&mut h);
    format!("{:?} {:?}", mini.tracks, mini.pulses).hash(&mut h);
    h.finish()
}

/// [`view_key`] from the small scene's [`content_key`] and this picture's own numbers.
#[allow(clippy::too_many_arguments)]
pub fn frame_key(content: u64, radius: f32, time: f64, clock: f64, pixels: u32, mode: ViewMode, ground: Ground) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (content, radius.to_bits(), time.to_bits(), clock.to_bits(), pixels, mode, ground).hash(&mut h);
    h.finish()
}

/// Still / Moving / Hit and Dark / Scene, as two segmented switches.
fn buttons(ui: &mut Ui, view: &mut BallView) {
    ui.horizontal(|ui| {
        segmented(
            ui,
            &mut view.mode,
            &[
                (ViewMode::Still, "Still", "The ball at rest"),
                (ViewMode::Moving, "Moving", "The ball going sideways fast, for Stretch when fast and Turning"),
                (ViewMode::Hit, "Hit", "The ball hitting something, over and over, for the reactions to hits"),
            ],
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right to left, so the last is drawn first.
            let mut ground = view.ground;
            segmented(
                ui,
                &mut ground,
                &[
                    (Ground::Scene, "Scene", "On the scene's own background"),
                    (Ground::Dark, "Dark", "On a plain dark background"),
                ],
            );
            view.ground = ground;
        });
    });
}

/// A row of buttons, one per choice, the chosen one inverted.
pub(crate) fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, choices: &[(T, &str, &str)]) {
    let frame = egui::Frame::new().stroke(egui::Stroke::new(1.0, style::LINE)).corner_radius(style::RADIUS);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for &(choice, name, help) in choices {
            let on = *value == choice;
            let button = style::toggle_button(name, on).frame_when_inactive(on).min_size(egui::vec2(0.0, 22.0));
            if ui.add(button).on_hover_text(help).clicked() {
                *value = choice;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_points_go_to_the_ball_and_back() {
        let map = ViewMap { centre: Pos2::new(150.0, 160.0), radius: 105.0, turn: 0.7 };
        for p in [Pos2::new(150.0, 160.0), Pos2::new(10.0, 300.0), Pos2::new(260.0, 40.5)] {
            let back = map.to_view(map.to_ball(p));
            assert!((back - p).length() < 1e-3, "{p:?} came back as {back:?}");
        }
        let flat = ViewMap { turn: 0.0, ..map };
        assert_eq!(flat.to_view(Vec2::new(0.0, -1.0)), Pos2::new(150.0, 55.0), "y down: -1 is the top");
    }

    #[test]
    fn cuts_alone_mark_their_layer_s_record_only() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::OUTLINE).unwrap();
        let pattern = scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::PATTERN).unwrap();
        scene.add_sub_layer(reg, ball, SlotId::Look, pattern, bpe_kit::modules::look::layers::SUB_CUT).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 0, 30.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
        show_cuts_alone(&mut frame, &resolved, ball, pattern);
        let solo: Vec<bool> = frame.look_layers.iter().map(|r| r.head[2] & bpe_gfx::LayerGpu::SOLO != 0).collect();
        assert_eq!(solo, [false, false, true, false], "body, outline, pattern, its cut");
    }

    #[test]
    fn the_view_puts_one_ball_at_its_middle_at_a_third_of_its_size() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        scene.things.get_mut(&ball).unwrap().params.set("group", Value::Bool(true));
        scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(300.0, -50.0);
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 0, 30.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
        assert_eq!(frame.layered.len(), 1, "one ball, whatever the group");
        assert_eq!(frame.layered[0].center, [0.0, 0.0]);
        assert_eq!(frame.layered[0].radius, 30.0);
        assert_eq!((frame.width, frame.height), (view_size(30.0), view_size(30.0)));
        assert!((30.0 / frame.width as f32 - RADIUS_SHARE).abs() < 0.01);
        // Moving: the ball goes sideways; Hit: it flashes once it has Flash on hit.
        build_view_frame(&mini, &resolved, &kit, ball, 0, 30.0, ViewMode::Moving, 0.0, 0.0, 0, &mut frame);
        assert_eq!(frame.layered[0].center, [0.0, 0.0], "it stays in the middle");
        let reg = &kit.registry;
        scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::FLASH).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        build_view_frame(&mini, &resolved, &kit, ball, 0, 30.0, ViewMode::Hit, 0.1, 0.1, 0, &mut frame);
        assert_eq!(frame.flashes.len(), 1);
    }

    #[test]
    fn the_view_shows_the_ball_of_the_group_asked_for() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let p = &mut scene.things.get_mut(&ball).unwrap().params;
        p.set("group", Value::Bool(true));
        p.set("count", Value::Int(10));
        p.set("release", Value::Choice("one_by_one".into()));
        let g = scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::GROUP_GRADIENT).unwrap();
        let look = scene.card_mut(ball, SlotId::Look).unwrap();
        look.params.set(&bpe_core::layered_key(g, "spread"), Value::Choice("order".into()));
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        let body = |n| {
            let mut frame = Frame::default();
            build_view_frame(&mini, &resolved, &kit, ball, n, 24.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
            assert_eq!(frame.layered.len(), 1, "one ball of the ten");
            assert_eq!(frame.layered[0].center, [0.0, 0.0]);
            frame.layered[0].body
        };
        assert_ne!(body(0), body(9), "the first and last of the group take the gradient's two ends");
        assert_eq!(body(9), body(50), "past the last, the last");
    }

    #[test]
    fn the_view_draws_a_white_ball_on_the_dark_ground() {
        let Ok((device, queue)) = bpe_gfx::test_device() else {
            eprintln!("no GPU adapter available, skipping");
            return;
        };
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, &kit.registry);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 0, 24.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
        let mut renderer = Renderer::new(&device, 100, 100);
        renderer.render(&device, &queue, &frame);
        let pixels = renderer.read_output(&device, &queue).unwrap();
        let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3];
        assert!(at(50, 50).iter().all(|&c| c > 230), "white in the middle: {:?}", at(50, 50));
        assert!(at(50, 50 - 30).iter().all(|&c| c > 230), "the ball reaches 35 % of the way out");
        assert!(at(3, 3).iter().all(|&c| c < 40), "dark in the corner: {:?}", at(3, 3));
        assert!(at(50, 50 - 40).iter().all(|&c| c < 40), "and past the ball: {:?}", at(50, 10));
    }

    #[test]
    fn the_small_scene_is_worked_out_again_only_when_it_changes() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let mut cache = MiniCache::default();
        for _ in 0..3 {
            cache.update(mini_scene(&scene, &kit, ball, Ground::Dark).unwrap(), &kit, ball);
        }
        assert_eq!(cache.builds, 1, "the same small scene every frame is resolved once");
        let content = cache.content;
        let colours = Address::on(ball, SlotId::Look, "0/colours");
        let red = Value::Gradient(bpe_core::Gradient::solid(Color::rgb(255, 0, 0)));
        scene.set_at(reg, &colours, red).unwrap();
        cache.update(mini_scene(&scene, &kit, ball, Ground::Dark).unwrap(), &kit, ball);
        assert_eq!(cache.builds, 2, "a changed look is worked out again");
        assert_ne!(cache.content, content);
        // One world serves every picture: two pictures of it match two from fresh worlds.
        let mini = cache.mini().unwrap().clone();
        let mut world = World::build(&mini, &cache.resolved, &kit.sims);
        let (mut a, mut b) = (Frame::default(), Frame::default());
        build_view_frame_in(
            &mut world,
            &mini,
            &cache.resolved,
            &kit,
            ball,
            0,
            24.0,
            ViewMode::Hit,
            1.0,
            0.1,
            2,
            &mut a,
        );
        build_view_frame_in(
            &mut world,
            &mini,
            &cache.resolved,
            &kit,
            ball,
            0,
            24.0,
            ViewMode::Still,
            1.5,
            0.0,
            0,
            &mut a,
        );
        build_view_frame(&mini, &cache.resolved, &kit, ball, 0, 24.0, ViewMode::Still, 1.5, 0.0, 0, &mut b);
        assert_eq!(format!("{:?}", a.layered), format!("{:?}", b.layered), "a reused world draws as a fresh one");
    }

    #[test]
    fn a_still_view_draws_again_when_a_keyed_setting_or_the_size_changes() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let key_of = |scene: &Scene| {
            let mini = mini_scene(scene, &kit, ball, Ground::Dark).unwrap();
            let resolved = bpe_core::resolve(&mini, reg);
            view_key(&mini, &resolved, ball, 24.0, 0.0, 0.0, 100, ViewMode::Still, Ground::Dark)
        };
        let colours = Address::on(ball, SlotId::Look, "0/colours");
        let red = Value::Gradient(bpe_core::Gradient::solid(Color::rgb(255, 0, 0)));
        scene.set_key(reg, &colours, 0.0, red).unwrap();
        let before = key_of(&scene);
        let blue = Value::Gradient(bpe_core::Gradient::solid(Color::rgb(0, 0, 255)));
        scene.set_key(reg, &colours, 0.0, blue).unwrap();
        assert_ne!(key_of(&scene), before, "a new key's value draws the view again");
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        let at = |radius| view_key(&mini, &resolved, ball, radius, 0.0, 0.0, 100, ViewMode::Still, Ground::Dark);
        assert_ne!(at(24.0), at(30.0), "so does a new size");
    }

    #[test]
    fn the_view_keeps_the_look_s_pulses_and_counts_a_hit_a_loop() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let (hue, glow) = (bpe_kit::modules::look::layers::HUE, bpe_kit::modules::look::layers::GLOW);
        scene.add_layer(reg, ball, SlotId::Look, hue).unwrap();
        let glow = scene.add_layer(reg, ball, SlotId::Look, glow).unwrap();
        let strength = Address::on(ball, SlotId::Look, &bpe_core::layered_key(glow, "strength"));
        scene.set_pulse(reg, &strength, bpe_core::PulseSource::Hits, Value::Number(0.5), 0.25).unwrap();
        // A red body, since turning the hue of white changes nothing.
        let look = scene.things.get_mut(&ball).unwrap().slots.get_mut(&SlotId::Look).unwrap();
        look.params.set("0/colours", Value::Gradient(bpe_core::Gradient::solid(Color::rgb(255, 0, 0))));
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        assert!(mini.pulses.contains_key(&strength), "the Look's pulses come along");
        let resolved = bpe_core::resolve(&mini, reg);
        let body = |loops| {
            let mut frame = Frame::default();
            build_view_frame(&mini, &resolved, &kit, ball, 0, 24.0, ViewMode::Hit, 0.5, 0.5, loops, &mut frame);
            frame.layered[0].body
        };
        assert_ne!(body(0), body(1), "each loop's hit turns the colour once more");
    }

    #[test]
    fn the_scene_ground_keeps_the_scene_s_background() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let red = bpe_kit::paint::Paint::solid(Color::rgb(200, 0, 0));
        let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
        root.params = SceneSettings { background: red.clone(), ..SceneSettings::from_params(&root.params) }.to_params();
        let on = |ground| {
            let mini = mini_scene(&scene, &kit, ball, ground).unwrap();
            SceneSettings::from_params(&mini.things[&ThingId::ROOT].params).background
        };
        assert_eq!(on(Ground::Scene), red);
        assert_eq!(on(Ground::Dark), bpe_kit::paint::Paint::solid(DARK));
    }
}
