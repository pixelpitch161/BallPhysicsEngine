//! The ball view at the top of the Look editor: the thing's look on one ball, still, moving or
//! hitting something, on a dark ground or the scene's own. It builds a frame for a small scene
//! of its own (the ball alone) and never steps the physics.

use std::hash::{Hash, Hasher};

use bpe_core::{Address, Color, Resolved, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{CameraView, Event, EventKind, Log, Party, World};
use eframe::{egui, egui_wgpu, wgpu};
use egui::{Pos2, Rect, RichText, Ui};
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
const DARK: Color = Color::rgb(0x15, 0x15, 0x15); // scene colour
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
    /// A layer whose masks the view shows alone (white where they let it show, black where they
    /// hide it), until Alt-click again or another thing is picked.
    pub solo: Option<(ThingId, bpe_core::LayerId)>,
}

/// The small scene the view draws: the scene's settings (or plain ones on a dark ground) and a
/// copy of `id` alone, a single ball at the middle with its Look and nothing else.
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
    ball.params.set("arrangement", Value::Choice("single".into()));
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

/// The frame of `mini` (from [`mini_scene`]) for thing `id` with radius `radius`, at `time`
/// seconds into the scene (`clock` of them into the view's loop, after `loops` whole loops: in
/// Hit the ball has hit once a loop).
#[allow(clippy::too_many_arguments)]
pub fn build_view_frame(
    mini: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    id: ThingId,
    radius: f32,
    mode: ViewMode,
    time: f64,
    clock: f64,
    loops: u32,
    frame: &mut Frame,
) {
    let mut world = World::build(mini, resolved, &kit.sims);
    let mut log = Log::default();
    let rate = world.steps_per_second;
    if let Some(bodies) = world.bodies.get_mut(&id) {
        for i in 0..bodies.len() {
            bodies.position[i] = Vec2::ZERO;
            bodies.radius[i] = radius;
            bodies.velocity[i] = if mode == ViewMode::Moving { Vec2::new(MOVING_SPEED, 0.0) } else { Vec2::ZERO };
        }
        if mode == ViewMode::Hit && !bodies.hits.is_empty() {
            bodies.hits[0] = loops + 1;
            let start = time - clock;
            log.push(Event {
                step: (start * rate as f64).round().max(0.0) as u64,
                kind: EventKind::Hit,
                a: Party { thing: id, item: 0 },
                b: None,
                point: Vec2::new(0.0, radius),
                normal: Vec2::new(0.0, -1.0),
                speed: HIT_SPEED,
            });
        }
    }
    build_frame(mini, resolved, &world, &log, time, &kit.draws, frame);
    let size = view_size(radius);
    frame.set_view(CameraView::default(), (size, size));
    frame.overlay = None;
}

/// Draws the view and its buttons for thing `id` of `scene` at the playhead `time`.
pub fn show(
    ui: &mut Ui,
    view: &mut BallView,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    time: f64,
) {
    buttons(ui, view);
    let side = ui.available_width().clamp(40.0, MAX_SIZE);
    let Some(render) = render else {
        ui.label(RichText::new("The ball view needs the wgpu renderer.").small().color(style::MUTED));
        return;
    };
    let Some(mini) = mini_scene(scene, kit, id, view.ground) else { return };
    let resolved = bpe_core::resolve(&mini, &kit.registry);
    let radius = match scene.value_at_time(&kit.registry, &Address::own(id, "radius"), time) {
        Some(Value::Number(r)) => r as f32,
        _ => 24.0,
    };
    let pixels = (side * ui.ctx().pixels_per_point()).round().max(2.0) as u32;
    let device = &render.device;
    let (renderer, texture) = view.renderer.get_or_insert_with(|| {
        let renderer = Renderer::new(device, pixels, pixels);
        let texture =
            render.renderer.write().register_native_texture(device, renderer.output_view(), wgpu::FilterMode::Linear);
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
    let looped = match view.mode {
        ViewMode::Still => None,
        ViewMode::Moving => Some(MOVING_LOOP),
        ViewMode::Hit => Some(HIT_LOOP),
    };
    let now = ui.input(|i| i.time);
    let clock = looped.map_or(0.0, |len| now % len);
    let loops = looped.map_or(0, |len| (now / len) as u32);
    let solo = view.solo.filter(|(t, _)| *t == id).map(|(_, layer)| layer);
    let key = view_key(&mini, &resolved, id, radius, time, clock, pixels, view.mode, view.ground)
        ^ solo.map_or(0, |l| (u64::from(l.0) + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    if view.drawn != Some(key) {
        let at = time + clock;
        build_view_frame(&mini, &resolved, kit, id, radius, view.mode, at, clock, loops, &mut view.frame);
        if let Some(layer) = solo {
            show_masks_alone(&mut view.frame, &resolved, id, layer);
        }
        renderer.render(device, &render.queue, &view.frame);
        view.drawn = Some(key);
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.painter().image(*texture, rect, uv, style::UNTINTED);
    if looped.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// Makes the frame of [`build_view_frame`] show layer `layer`'s masks alone: white where they let
/// it show, black where they hide it. Nothing changes when the layer draws nothing.
pub fn show_masks_alone(frame: &mut Frame, resolved: &Resolved, id: ThingId, layer: bpe_core::LayerId) {
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
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", resolved.things.get(&id)).hash(&mut h);
    format!("{:?}", resolved.things.get(&ThingId::ROOT)).hash(&mut h);
    format!("{:?} {:?}", mini.tracks, mini.pulses).hash(&mut h);
    (radius.to_bits(), time.to_bits(), clock.to_bits(), pixels, mode, ground).hash(&mut h);
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
fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, choices: &[(T, &str, &str)]) {
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
    fn masks_alone_mark_their_layer_s_record_only() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let ball = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::OUTLINE).unwrap();
        let pattern = scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::PATTERN).unwrap();
        scene.add_mask(reg, ball, SlotId::Look, pattern).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 30.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
        show_masks_alone(&mut frame, &resolved, ball, pattern);
        let solo: Vec<bool> = frame.look_layers.iter().map(|r| r.head[2] & bpe_gfx::LayerGpu::SOLO != 0).collect();
        assert_eq!(solo, [false, false, true, false], "body, outline, pattern, its mask");
    }

    #[test]
    fn the_view_puts_one_ball_at_its_middle_at_a_third_of_its_size() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        scene.things.get_mut(&ball).unwrap().params.set("arrangement", Value::Choice("block".into()));
        scene.things.get_mut(&ball).unwrap().transform.position = Vec2::new(300.0, -50.0);
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 30.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
        assert_eq!(frame.layered.len(), 1, "one ball, whatever the arrangement");
        assert_eq!(frame.layered[0].center, [0.0, 0.0]);
        assert_eq!(frame.layered[0].radius, 30.0);
        assert_eq!((frame.width, frame.height), (view_size(30.0), view_size(30.0)));
        assert!((30.0 / frame.width as f32 - RADIUS_SHARE).abs() < 0.01);
        // Moving: the ball goes sideways; Hit: it flashes once it has Flash on hit.
        build_view_frame(&mini, &resolved, &kit, ball, 30.0, ViewMode::Moving, 0.0, 0.0, 0, &mut frame);
        assert_eq!(frame.layered[0].center, [0.0, 0.0], "it stays in the middle");
        let reg = &kit.registry;
        scene.add_layer(reg, ball, SlotId::Look, bpe_kit::modules::look::layers::FLASH).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        build_view_frame(&mini, &resolved, &kit, ball, 30.0, ViewMode::Hit, 0.1, 0.1, 0, &mut frame);
        assert_eq!(frame.flashes.len(), 1);
    }

    #[test]
    fn the_view_draws_a_white_ball_on_the_dark_ground() {
        let Ok((device, queue)) = bpe_gfx::headless_device() else {
            eprintln!("no GPU adapter available, skipping");
            return;
        };
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let mini = mini_scene(&scene, &kit, ball, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, &kit.registry);
        let mut frame = Frame::default();
        build_view_frame(&mini, &resolved, &kit, ball, 24.0, ViewMode::Still, 0.0, 0.0, 0, &mut frame);
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
            build_view_frame(&mini, &resolved, &kit, ball, 24.0, ViewMode::Hit, 0.5, 0.5, loops, &mut frame);
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
