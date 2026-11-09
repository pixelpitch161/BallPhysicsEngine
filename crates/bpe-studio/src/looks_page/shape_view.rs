//! The Looks tab's picture of a shape look: a small scene of the shape alone on a dark ground, wearing the look,
//! still, playing (its colours moving with time) or hit by a ball once a loop. Like the ball view it never steps
//! the physics: the world is built from the small scene and drawn at a moment.

use std::hash::{Hash, Hasher};

use bpe_core::{Resolved, Scene, Settings, SlotId, ThingId};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::things::shape::{ShapeFill, ShapeSettings, ShapeStart};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{CameraView, Event, EventKind, Log, Party, World};
use eframe::{egui, egui_wgpu, wgpu};
use egui::{Pos2, Rect, Ui};
use glam::Vec2;

use crate::look_editor::ball_view::{self, HIT_LOOP};
use crate::style;

/// How the shape shows in the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShapeMode {
    #[default]
    Still,
    /// Time runs, round a [`PLAY_LOOP`], so moving colours move.
    Playing,
    /// A ball hits it at the start of every [`HIT_LOOP`].
    Hit,
}

/// How long the Playing loop is, in seconds.
pub const PLAY_LOOP: f64 = 4.0;
/// How much room the picture leaves round the shape.
const MARGIN: f32 = 1.15;
/// The picture's largest size, in points.
const MAX_SIZE: f32 = 300.0;
/// The sample shape tiles wear a look on: a solid ellipse.
const SAMPLE: (f32, f32) = (260.0, 170.0);
/// How hard the ball hits in Hit, in pixels per second.
const HIT_SPEED: f32 = 600.0;

/// The shape picture's state, one for the editor.
#[derive(Default)]
pub struct ShapeView {
    pub mode: ShapeMode,
    renderer: Option<(Renderer, egui::TextureId)>,
    frame: Frame,
    /// The small scene last shown, what was worked out from it and the world built from it.
    mini: Option<Scene>,
    resolved: Resolved,
    world: Option<World>,
    content: u64,
    /// What the frame was built for and what the picture shows.
    built: Option<u64>,
    drawn: Option<u64>,
}

/// The small scene a shape picture draws: a dark ground and a copy of shape `id` at the middle with its Look and
/// nothing else, or (for a tile, `sample`) a solid ellipse wearing that Look.
pub fn mini(scene: &Scene, kit: &Kit, id: ThingId, sample: bool) -> Option<Scene> {
    let reg = &kit.registry;
    let thing = scene.things.get(&id)?;
    let mut mini = Scene::empty(reg);
    let root = mini.things.get_mut(&ThingId::ROOT)?;
    let plain = SceneSettings { background: bpe_kit::paint::Paint::solid(ball_view::DARK), ..SceneSettings::default() };
    root.params = plain.to_params();
    let mut shape = thing.clone();
    shape.parent = Some(ThingId::ROOT);
    shape.attach = bpe_core::Attach::Group;
    shape.transform = bpe_core::Transform::default();
    shape.enabled = true;
    shape.screen = None;
    shape.slots.retain(|slot, _| *slot == SlotId::Look);
    if sample {
        let own = ShapeSettings {
            start: ShapeStart::Ellipse,
            width: SAMPLE.0,
            height: SAMPLE.1,
            fill: ShapeFill::Filled,
            ..ShapeSettings::default()
        };
        shape.params = own.to_params();
    } else {
        // It stays where the picture puts it, the size it is now.
        for key in ["spin", "shrink"] {
            shape.params.0.remove(key);
        }
    }
    let kept = |a: &bpe_core::Address| a.thing == id && a.slot == Some(SlotId::Look);
    mini.tracks = scene.tracks.iter().filter(|(a, _)| kept(a)).map(|(a, t)| (a.clone(), t.clone())).collect();
    mini.pulses = scene.pulses.iter().filter(|(a, _)| kept(a)).map(|(a, p)| (a.clone(), p.clone())).collect();
    mini.things.insert(id, shape);
    mini.order.push(id);
    mini.next_thing = mini.next_thing.max(id.0 + 1);
    Some(mini)
}

/// The frame of `mini` (from [`mini`]) for shape `id` at `time` seconds, hit `clock` seconds ago when `hit`, framed
/// to the shape and what it draws round it.
#[allow(clippy::too_many_arguments)]
pub fn build(
    world: &World,
    mini: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    id: ThingId,
    time: f64,
    hit: Option<f64>,
    frame: &mut Frame,
) {
    let mut log = Log::default();
    let own = mini.things.get(&id).map(|t| ShapeSettings::from_params(&t.params)).unwrap_or_default();
    let reach = own.unit() + own.thickness * 0.5;
    if let Some(clock) = hit {
        let start = (time - clock).max(0.0);
        log.push(Event {
            step: (start * world.steps_per_second as f64).round() as u64,
            kind: EventKind::Hit,
            // A ball that is not in the small scene.
            a: Party { thing: ThingId(u32::MAX), item: 0 },
            b: Some(Party { thing: id, item: 0 }),
            point: Vec2::new(0.0, -reach),
            normal: Vec2::new(0.0, -1.0),
            speed: HIT_SPEED,
            at: 0.0,
        });
    }
    build_frame(mini, resolved, world, &log, time, &kit.draws, frame);
    let reach = frame.paths.first().map_or(reach, |p| p.reach().max(reach));
    let size = (reach * 2.0 * MARGIN).round().max(2.0) as u32;
    frame.set_view(CameraView::default(), (size, size));
    frame.overlay = None;
}

/// Draws the picture of shape `id` of `scene`, changed by `dress` (the look tried on), and its Still / Playing /
/// Hit switch, at the playhead `time`.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    view: &mut ShapeView,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    time: f64,
    dress: &dyn Fn(&mut Scene),
) {
    ui.horizontal(|ui| {
        ball_view::segmented(
            ui,
            &mut view.mode,
            &[
                (ShapeMode::Still, "Still", "The shape at the playhead"),
                (ShapeMode::Playing, "Playing", "Time runs, so moving colours move"),
                (ShapeMode::Hit, "Hit", "A ball hits it once a second or so"),
            ],
        );
    });
    let side = ui.available_width().clamp(40.0, MAX_SIZE);
    let Some(mut small) = mini(scene, kit, id, false) else { return };
    dress(&mut small);
    if view.mini.as_ref() != Some(&small) {
        view.resolved = bpe_core::resolve(&small, &kit.registry);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?} {:?} {:?}", view.resolved.things.get(&id), small.tracks, small.pulses).hash(&mut h);
        view.content = h.finish();
        view.world = None;
        view.mini = Some(small);
    }
    let pixels = (side * ui.ctx().pixels_per_point()).round().max(2.0) as u32;
    let looped = match view.mode {
        ShapeMode::Still => None,
        ShapeMode::Playing => Some(PLAY_LOOP),
        ShapeMode::Hit => Some(HIT_LOOP),
    };
    let now = ui.input(|i| i.time);
    let clock = looped.map_or(0.0, |len| now % len);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (view.content, time.to_bits(), clock.to_bits(), pixels, view.mode).hash(&mut h);
    let key = h.finish();
    if view.built != Some(key) {
        let small = view.mini.as_ref().expect("set above");
        let world = view.world.get_or_insert_with(|| World::build(small, &view.resolved, &kit.sims));
        let hit = (view.mode == ShapeMode::Hit).then_some(clock);
        build(world, small, &view.resolved, kit, id, time + clock, hit, &mut view.frame);
        view.built = Some(key);
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
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
            ui.painter().text(
                at,
                egui::Align2::CENTER_CENTER,
                "The picture needs the wgpu renderer.",
                font,
                style::MUTED,
            );
        }
    }
    if looped.is_some() {
        let wait = if ui.input(|i| i.focused) { 33 } else { 250 };
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(wait));
    }
}
