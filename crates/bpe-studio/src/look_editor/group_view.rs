//! The Group view at the top of a Balls thing's Look in Group mode: the whole group where it
//! starts, drawn by the same renderer as the video, with what can be dragged on it: each ball of
//! a custom shape, the group gradient's line, and balls picked for layers of their own. It never steps the physics.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use bpe_core::{Address, LayerId, Resolved, Scene, Settings, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame};
use bpe_kit::modules::look::layers::{Easing, GroupGradient, Spread};
use bpe_kit::things::balls::shapes::{bounds, offsets};
use bpe_kit::things::emitter::EmitterSettings;
use bpe_kit::{BallsSettings, Kit};
use bpe_runtime::{CameraView, Log, World};
use eframe::{egui, egui_wgpu, wgpu};
use egui::{Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui};
use glam::Vec2;

use super::ball_view::{Ground, mini_scene};
use crate::inspector::Edit;
use crate::style;

/// The view's largest size, in points.
const MAX_SIZE: f32 = 300.0;
/// How far past the group the view reaches, as a share of the group's size.
const MARGIN: f32 = 0.1;
/// How close, in points, the pointer must be to grab a handle.
const GRAB: f32 = 10.0;

/// What is being dragged in the view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// A custom shape's ball, by its place in the list.
    Point(usize),
    /// One of the gradient line's end dots: the end (true) or the start.
    GradientEnd(bool),
    /// The gradient line's easing diamond.
    Diamond,
}

/// The Group view's state, one for the editor.
#[derive(Default)]
pub struct GroupView {
    pub ground: Ground,
    renderer: Option<(Renderer, egui::TextureId)>,
    frame: Frame,
    drawn: Option<u64>,
    /// Clicking balls picks them, for Odd ones out.
    pub picking: bool,
    /// The balls picked, by number, and the thing they were picked on.
    pub picked: BTreeSet<u32>,
    picked_on: Option<ThingId>,
    /// The gradient line is shown on the view and can be dragged.
    pub edit_gradient: bool,
    drag: Option<Drag>,
    /// The world point at the view's middle and how much of the world it shows across, kept
    /// still while something is dragged so the view doesn't rescale under the pointer.
    fit: Option<(Vec2, f32)>,
}

impl GroupView {
    /// Drops the picks and stops picking when the view shows another thing than they were
    /// picked on.
    pub fn keep_picks_for(&mut self, id: ThingId) {
        if self.picked_on != Some(id) {
            self.picked.clear();
            self.picking = false;
            self.picked_on = Some(id);
        }
    }

    /// The world point at the view's middle and how much it shows across, as last drawn.
    #[cfg(test)]
    pub fn fit(&self) -> Option<(Vec2, f32)> {
        self.fit
    }
}

/// What the view shows of thing `id`: its settings at the playhead and its group gradient.
pub struct ViewCx<'a> {
    pub scene: &'a Scene,
    pub kit: &'a Kit,
    pub id: ThingId,
    pub time: f64,
    /// The selected group gradient layer, when the view shows its line.
    pub gradient: Option<(LayerId, GroupGradient)>,
}

impl ViewCx<'_> {
    /// Whether the thing is an emitter, whose group is one round laid out a short way along its
    /// directions (nothing on it can be dragged but the gradient line).
    pub fn is_emitter(&self) -> bool {
        self.scene.things.get(&self.id).is_some_and(|t| t.kind == EMITTER)
    }

    /// The Balls settings as they are at the playhead. An emitter's stand in as a custom group
    /// of one round's places, with its size and variety.
    pub fn settings(&self) -> BallsSettings {
        let reg = &self.kit.registry;
        let mut p = bpe_core::Params::default();
        if self.is_emitter() {
            for d in EmitterSettings::defs() {
                if let Some(v) = self.scene.value_at_time(reg, &Address::own(self.id, d.key), self.time) {
                    p.set(d.key, v);
                }
            }
            let e = EmitterSettings::from_params(&p);
            let custom = bpe_kit::things::emitter::fire::layout(&e).iter().map(|v| [v.x, v.y]).collect();
            return BallsSettings { shape: bpe_kit::GroupShape::Custom, custom, ..e.variety() };
        }
        for d in BallsSettings::defs() {
            if let Some(v) = self.scene.value_at_time(reg, &Address::own(self.id, d.key), self.time) {
                p.set(d.key, v);
            }
        }
        BallsSettings::from_params(&p)
    }
}

/// The emitter kind.
const EMITTER: &str = bpe_kit::things::emitter::KIND;

/// How the view maps the world to the screen: the view's rectangle, the world point at its
/// middle and how much of the world it shows across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewMap {
    pub rect: Rect,
    pub centre: Vec2,
    pub size: f32,
}

impl ViewMap {
    pub fn to_screen(self, world: Vec2) -> Pos2 {
        let s = (world - self.centre) / self.size * self.rect.width();
        self.rect.center() + egui::vec2(s.x, s.y)
    }

    pub fn to_world(self, p: Pos2) -> Vec2 {
        let d = p - self.rect.center();
        self.centre + Vec2::new(d.x, d.y) / self.rect.width() * self.size
    }

    /// World lengths in points.
    pub fn scale(&self) -> f32 {
        self.rect.width() / self.size
    }
}

/// The world point the view centres on and how much it shows across, for balls of `radius` at
/// `places` (every size spread allowed for).
pub fn fit(places: &[Vec2], radius: f32) -> (Vec2, f32) {
    let (lo, hi) = bounds(places);
    let reach = radius * 1.5;
    let (lo, hi) = (lo - Vec2::splat(reach), hi + Vec2::splat(reach));
    let size = (hi - lo).max_element().max(radius * 4.0).max(1.0) * (1.0 + 2.0 * MARGIN);
    ((lo + hi) * 0.5, size)
}

/// The frame of `mini` (from [`mini_scene`]) showing the whole group at its start, the view
/// centred on `centre` showing `size` of the world across.
pub fn build_group_frame(
    mini: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    time: f64,
    centre: Vec2,
    size: f32,
    frame: &mut Frame,
) {
    let mut world = World::build(mini, resolved, &kit.sims);
    lay_out_rounds(&mut world);
    build_frame(mini, resolved, &world, &Log::default(), time, &kit.draws, frame);
    let pixels = size.round().max(2.0) as u32;
    // The frame is `size` world pixels across at zoom 1, centred on the group.
    frame.set_view(CameraView { center: centre, ..CameraView::default() }, (pixels, pixels));
    frame.overlay = None;
}

/// Puts every emitter's balls in `world` at their places in the round, where the view shows its
/// group (they all start at the emitter's middle).
fn lay_out_rounds(world: &mut World) {
    let emitters: Vec<ThingId> = world.order().iter().copied().filter(|id| world.kind(*id) == Some(EMITTER)).collect();
    for id in emitters {
        let Some(places) = bpe_kit::things::balls::rows::rows(world, id).map(|r| r.place.clone()) else { continue };
        let at = world.poses.get(&id).map_or(Vec2::ZERO, |p| p.position);
        if let Some(bodies) = world.bodies.get_mut(&id) {
            for (i, place) in places.iter().enumerate().take(bodies.len()) {
                bodies.position[i] = at + *place;
                bodies.previous[i] = at + *place;
            }
        }
    }
}

/// The places the group's balls start at, from the thing (turned by the group's Turn).
fn places(s: &BallsSettings) -> Vec<Vec2> {
    offsets(s)
}

/// The places of a custom shape before its Turn: what `custom` holds.
fn unturned(s: &BallsSettings, world: Vec2) -> [f32; 2] {
    let p = if s.turn != 0.0 { Vec2::from_angle(-s.turn.to_radians()).rotate(world) } else { world };
    [p.x, p.y]
}

/// The group gradient line's ends for `g` over `places`: its start (colour 0) and end (colour 1).
pub fn gradient_ends(g: &GroupGradient, places: &[Vec2]) -> (Vec2, Vec2) {
    let info = bpe_kit::modules::look::layers::group::GroupShapeInfo::of(places, g.angle, 1);
    let dir = Vec2::from_angle(g.angle.to_radians());
    let (lo, hi) = info.along;
    let half = (hi - lo).max(1e-3) * g.stretch.clamp(0.1, 4.0) * 0.5;
    let mid = info.middle + dir * (lo + hi) * 0.5;
    (mid - dir * half, mid + dir * half)
}

/// Where along the line (0 to 1) the gradient's middle colour lands, where the diamond sits.
pub fn diamond_at(g: &GroupGradient) -> f32 {
    let k = g.ease_strength.clamp(1.0, 6.0);
    match g.ease {
        Easing::Even => 0.5,
        // t^k = 0.5
        Easing::EaseIn => 0.5f32.powf(1.0 / k),
        // 1 - (1-t)^k = 0.5
        Easing::EaseOut => 1.0 - 0.5f32.powf(1.0 / k),
        Easing::EaseBoth => 0.5 + (k - 1.0) / 12.0,
    }
}

/// The easing and its strength that put the diamond at `u` (0 to 1 along the line).
pub fn ease_for(current: Easing, u: f32) -> (Easing, f32) {
    let u = u.clamp(0.02, 0.98);
    let strength = |k: f32| (k.clamp(1.0, 6.0) * 10.0).round() / 10.0;
    match current {
        Easing::EaseBoth => (Easing::EaseBoth, strength(1.0 + (u - 0.5).max(0.0) * 12.0)),
        _ if (u - 0.5).abs() < 0.02 => (Easing::Even, 2.0),
        _ if u > 0.5 => (Easing::EaseIn, strength(0.5f32.ln() / u.ln())),
        _ => (Easing::EaseOut, strength(0.5f32.ln() / (1.0 - u).ln())),
    }
}

/// The next easing round the cycle, for a double click on the diamond.
pub fn next_ease(e: Easing) -> Easing {
    match e {
        Easing::Even => Easing::EaseIn,
        Easing::EaseIn => Easing::EaseOut,
        Easing::EaseOut => Easing::EaseBoth,
        Easing::EaseBoth => Easing::Even,
    }
}

/// The tag by the gradient line: "−14° · ease out".
pub fn gradient_tag(g: &GroupGradient) -> String {
    let ease = match g.ease {
        Easing::Even => "even",
        Easing::EaseIn => "ease in",
        Easing::EaseOut => "ease out",
        Easing::EaseBoth => "ease both",
    };
    let angle = g.angle.round();
    let sign = if angle < 0.0 { "−" } else { "" };
    format!("{sign}{:.0}° · {ease}", angle.abs())
}

/// Draws the view for thing `id` and handles what is dragged and clicked on it, pushing edits.
/// `slot` is the Look card the gradient lives in. A drag's start sets `own_step`, so the drag is
/// an undo step of its own.
pub fn show(
    ui: &mut Ui,
    view: &mut GroupView,
    render: Option<&egui_wgpu::RenderState>,
    cx: &ViewCx,
    slot: bpe_core::SlotId,
    own_step: &mut bool,
    edits: &mut Vec<Edit>,
) {
    ground_buttons(ui, view);
    let s = cx.settings();
    let places = places(&s);
    let side = ui.available_width().clamp(40.0, MAX_SIZE);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Group view"));
    // Kept still through a drag, fitted again otherwise.
    let (centre, size) = match (view.drag, view.fit) {
        (Some(_), Some(f)) => f,
        _ => fit(&places, s.radius),
    };
    view.fit = Some((centre, size));
    let map = ViewMap { rect, centre, size };
    picture(ui, view, render, cx, rect, centre, size);
    interact(ui, view, cx, &s, &places, map, &response, slot, own_step, edits);
    overlays(ui, view, cx, &s, &places, map);
}

/// The group's picture, drawn again only when what it shows changes.
fn picture(
    ui: &mut Ui,
    view: &mut GroupView,
    render: Option<&egui_wgpu::RenderState>,
    cx: &ViewCx,
    rect: Rect,
    centre: Vec2,
    size: f32,
) {
    let painter = ui.painter_at(rect);
    let Some(render) = render else {
        painter.rect_filled(rect, 0.0, style::GROUND);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "The group view needs the wgpu renderer.",
            egui::FontId::proportional(11.0),
            style::MUTED,
        );
        return;
    };
    let Some(mini) = group_scene(cx.scene, cx.kit, cx.id, view.ground) else { return };
    let resolved = bpe_core::resolve(&mini, &cx.kit.registry);
    let pixels = (rect.width() * ui.ctx().pixels_per_point()).round().max(2.0) as u32;
    let device = &render.device;
    let (renderer, texture) = view.renderer.get_or_insert_with(|| {
        let mut renderer = Renderer::new(device, pixels, pixels);
        renderer.share_programs(crate::preview::shared_programs());
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
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", resolved.things.get(&cx.id)).hash(&mut h);
    format!("{:?}", resolved.things.get(&ThingId::ROOT)).hash(&mut h);
    format!("{:?} {:?}", mini.tracks, mini.pulses).hash(&mut h);
    (cx.time.to_bits(), pixels, view.ground, centre.x.to_bits(), centre.y.to_bits(), size.to_bits()).hash(&mut h);
    let key = h.finish();
    if view.drawn != Some(key) {
        build_group_frame(&mini, &resolved, cx.kit, cx.time, centre, size, &mut view.frame);
        renderer.render(device, &render.queue, &view.frame);
        view.drawn = Some(key);
    }
    if crate::preview::redraw_when_built(renderer, ui.ctx()) {
        view.drawn = None;
    }
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(*texture, rect, uv, style::UNTINTED);
}

/// The small scene the view draws: as the ball view's, with the whole group, every ball let go
/// at once, so all of them show where they start.
pub fn group_scene(scene: &Scene, kit: &Kit, id: ThingId, ground: Ground) -> Option<Scene> {
    let mut mini = mini_scene(scene, kit, id, ground)?;
    // The thing's own keys too, so a keyed size draws as the handles say.
    for (a, t) in scene.tracks.iter().filter(|(a, _)| a.thing == id && a.slot.is_none()) {
        mini.tracks.insert(a.clone(), t.clone());
    }
    let thing = mini.things.get_mut(&id)?;
    thing.params.set("release", Value::Choice("all_at_once".into()));
    Some(mini)
}

/// Dark / Scene, on the right.
fn ground_buttons(ui: &mut Ui, view: &mut GroupView) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Where the balls start").small().color(style::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::ball_view::segmented(
                ui,
                &mut view.ground,
                &[
                    (Ground::Scene, "Scene", "On the scene's own background"),
                    (Ground::Dark, "Dark", "On a plain dark background"),
                ],
            );
        });
    });
}

/// The ball nearest `p` (a screen point) within its radius, by number.
fn ball_at(places: &[Vec2], radius: f32, map: &ViewMap, p: Pos2) -> Option<u32> {
    let reach = (radius * map.scale()).max(GRAB);
    places
        .iter()
        .enumerate()
        .map(|(i, w)| (i, map.to_screen(*w).distance(p)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i as u32)
}

/// What a press at `p` grabs, in order: the gradient line, a custom ball.
fn grab(view: &GroupView, cx: &ViewCx, s: &BallsSettings, places: &[Vec2], map: &ViewMap, p: Pos2) -> Option<Drag> {
    if view.edit_gradient
        && let Some((_, g)) = &cx.gradient
        && g.spread == Spread::Place
    {
        let (a, b) = gradient_ends(g, places);
        let d = a.lerp(b, diamond_at(g));
        if map.to_screen(d).distance(p) <= GRAB {
            return Some(Drag::Diamond);
        }
        if map.to_screen(b).distance(p) <= GRAB {
            return Some(Drag::GradientEnd(true));
        }
        if map.to_screen(a).distance(p) <= GRAB {
            return Some(Drag::GradientEnd(false));
        }
    }
    if s.group && s.shape == bpe_kit::GroupShape::Custom && !cx.is_emitter() {
        let near = places
            .iter()
            .enumerate()
            .map(|(i, w)| (i, map.to_screen(*w).distance(p)))
            .filter(|(_, d)| *d <= (s.radius * map.scale()).max(GRAB))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near
            && i < s.custom.len()
        {
            return Some(Drag::Point(i));
        }
    }
    None
}

/// Presses, drags and clicks on the view.
#[allow(clippy::too_many_arguments)]
fn interact(
    ui: &mut Ui,
    view: &mut GroupView,
    cx: &ViewCx,
    s: &BallsSettings,
    places: &[Vec2],
    map: ViewMap,
    response: &Response,
    slot: bpe_core::SlotId,
    own_step: &mut bool,
    edits: &mut Vec<Edit>,
) {
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
    {
        view.drag = grab(view, cx, s, places, &map, press);
        *own_step |= view.drag.is_some();
    }
    if response.drag_stopped() {
        view.drag = None;
    }
    let pointer = response.interact_pointer_pos();
    if let (Some(drag), Some(p)) = (view.drag, pointer.filter(|_| response.dragged())) {
        let world = map.to_world(p);
        match drag {
            Drag::Point(i) => {
                let mut points = s.custom.clone();
                if let Some(slot) = points.get_mut(i) {
                    *slot = unturned(s, world);
                    edits.push(Edit::SetOwnHere("custom".into(), Value::Points(points)));
                }
            }
            Drag::GradientEnd(end) => {
                if let Some((layer, _)) = &cx.gradient {
                    use bpe_kit::modules::look::layers::group::GroupShapeInfo;
                    // The line turns about the group's middle and runs from the start to the end.
                    let middle = GroupShapeInfo::of(places, 0.0, 1).middle;
                    let out = if end { world - middle } else { middle - world };
                    if out.length() > 1e-3 {
                        let angle = out.to_angle().to_degrees().round();
                        let dir = Vec2::from_angle(angle.to_radians());
                        let info = GroupShapeInfo::of(places, angle, 1);
                        let extent = (info.along.1 - info.along.0).max(1e-3);
                        let mid = info.middle + dir * (info.along.0 + info.along.1) * 0.5;
                        let half = if end { (world - mid).dot(dir) } else { (mid - world).dot(dir) };
                        let stretch = ((2.0 * half / extent) * 100.0).round() / 100.0;
                        let at = |key: &str| layered_key(*layer, key);
                        edits.push(Edit::SetSlot(slot, at("angle"), Value::Number(angle as f64)));
                        edits.push(Edit::SetSlot(slot, at("stretch"), Value::Number(stretch.clamp(0.1, 4.0) as f64)));
                    }
                }
            }
            Drag::Diamond => {
                if let Some((layer, g)) = &cx.gradient {
                    let (a, b) = gradient_ends(g, places);
                    let along = (b - a).length_squared().max(1e-6);
                    let u = (world - a).dot(b - a) / along;
                    let (ease, strength) = ease_for(g.ease, u);
                    let at = |key: &str| layered_key(*layer, key);
                    if ease != g.ease {
                        edits.push(Edit::SetSlot(slot, at("ease"), Value::Choice(choice_name(ease).into())));
                    }
                    edits.push(Edit::SetSlot(slot, at("ease_strength"), Value::Number(strength as f64)));
                }
            }
        }
        ui.ctx().request_repaint();
    }
    // A double click on the diamond goes round the easings.
    if response.double_clicked()
        && let (Some(p), Some((layer, g))) = (pointer, &cx.gradient)
        && grab(view, cx, s, places, &map, p) == Some(Drag::Diamond)
    {
        let next = next_ease(g.ease);
        edits.push(Edit::SetSlot(slot, layered_key(*layer, "ease"), Value::Choice(choice_name(next).into())));
        return;
    }
    if response.clicked()
        && view.picking
        && let Some(p) = pointer
        && let Some(n) = ball_at(places, s.radius, &map, p)
        && !view.picked.remove(&n)
    {
        view.picked.insert(n);
    }
    // A right click on a custom ball takes it away.
    if response.secondary_clicked()
        && let Some(p) = pointer
        && let Some(Drag::Point(i)) = grab(&GroupView::default(), cx, s, places, &map, p)
    {
        let mut points = s.custom.clone();
        points.remove(i);
        edits.push(Edit::SetOwnHere("custom".into(), Value::Points(points)));
    }
}

/// The name an easing is saved under.
fn choice_name(e: Easing) -> &'static str {
    bpe_core::Choice::name(e)
}

/// What is drawn over the picture: custom handles, picked balls, the gradient line.
fn overlays(ui: &mut Ui, view: &GroupView, cx: &ViewCx, s: &BallsSettings, places: &[Vec2], map: ViewMap) {
    let painter = ui.painter_at(map.rect);
    let r = (s.radius * map.scale()).max(3.0);
    // With no Look card nothing draws the balls, so their places are outlined.
    if cx.scene.card(cx.id, bpe_core::SlotId::Look).is_none() {
        for w in places {
            painter.circle_stroke(map.to_screen(*w), r, Stroke::new(1.0, style::MUTED));
        }
    }
    if s.group && s.shape == bpe_kit::GroupShape::Custom {
        for w in places.iter().take(s.custom.len()) {
            painter.circle_stroke(map.to_screen(*w), r + 2.0, Stroke::new(1.5, style::TEXT));
        }
    }
    for n in &view.picked {
        if let Some(w) = places.get(*n as usize) {
            painter.circle_stroke(map.to_screen(*w), r + 3.0, Stroke::new(2.0, style::TEXT_STRONG));
            painter.circle_stroke(map.to_screen(*w), r + 5.0, Stroke::new(1.0, style::OUTLINE));
        }
    }
    // The gradient line, its end dots and its easing diamond.
    if view.edit_gradient
        && let Some((_, g)) = &cx.gradient
        && g.spread == Spread::Place
    {
        let (a, b) = gradient_ends(g, places);
        let (pa, pb) = (map.to_screen(a), map.to_screen(b));
        painter.line_segment([pa, pb], Stroke::new(3.0, style::OUTLINE));
        painter.line_segment([pa, pb], Stroke::new(1.5, style::TEXT_STRONG));
        let colour = |t: f32| {
            let c = bpe_core::Color::from_linear(g.colours.sample(t));
            Color32::from_rgb(c.r, c.g, c.b) // scene colour
        };
        for (p, t) in [(pa, 0.0), (pb, 1.0)] {
            painter.circle_filled(p, 6.0, colour(t));
            painter.circle_stroke(p, 6.0, Stroke::new(1.5, style::TEXT_STRONG));
        }
        let d = map.to_screen(a.lerp(b, diamond_at(g)));
        style::paint_diamond(&painter, d, 7.0, view.drag == Some(Drag::Diamond));
        let tag = gradient_tag(g);
        let galley = painter.layout_no_wrap(tag, egui::FontId::proportional(11.0), style::TEXT_STRONG);
        // Beside the end dot, kept inside the view.
        let room = map.rect.shrink(4.0);
        let at = pb + egui::vec2(10.0, -galley.size().y - 6.0);
        let at = Pos2::new(
            at.x.min(room.right() - galley.size().x).max(room.left()),
            at.y.min(room.bottom() - galley.size().y).max(room.top()),
        );
        let back = Rect::from_min_size(at - egui::vec2(3.0, 2.0), galley.size() + egui::vec2(6.0, 4.0));
        painter.rect_filled(back, 2.0, style::PILL);
        painter.galley(at, galley, style::TEXT_STRONG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_fits_the_whole_group_with_room_round_it() {
        let places = [Vec2::new(-100.0, 0.0), Vec2::new(100.0, 50.0)];
        let (centre, size) = fit(&places, 10.0);
        assert_eq!(centre, Vec2::new(0.0, 25.0));
        assert!(size > 230.0 && size < 300.0, "{size}");
        let map = ViewMap { rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 200.0)), centre, size };
        for p in places {
            let s = map.to_screen(p);
            assert!(map.rect.contains(s));
            assert!((map.to_world(s) - p).length() < 1e-3);
        }
    }

    #[test]
    fn the_group_scene_lets_every_ball_go_at_once_and_draws_them_all() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let id = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let p = &mut scene.things.get_mut(&id).unwrap().params;
        p.set("group", Value::Bool(true));
        p.set("count", Value::Int(12));
        p.set("release", Value::Choice("one_by_one".into()));
        let mini = group_scene(&scene, &kit, id, Ground::Dark).unwrap();
        let resolved = bpe_core::resolve(&mini, reg);
        let mut frame = Frame::default();
        build_group_frame(&mini, &resolved, &kit, 0.0, Vec2::ZERO, 600.0, &mut frame);
        assert_eq!(frame.layered.len(), 12);
        assert_eq!((frame.width, frame.height), (600, 600));
    }

    #[test]
    fn the_diamond_sits_where_the_middle_colour_lands_and_dragging_it_eases() {
        let even = GroupGradient::default();
        assert_eq!(diamond_at(&even), 0.5);
        let (ease, k) = ease_for(Easing::Even, 0.7);
        assert_eq!(ease, Easing::EaseIn);
        let back = GroupGradient { ease, ease_strength: k, ..GroupGradient::default() };
        assert!((diamond_at(&back) - 0.7).abs() < 0.02, "{}", diamond_at(&back));
        let (ease, _) = ease_for(Easing::EaseIn, 0.3);
        assert_eq!(ease, Easing::EaseOut);
        assert_eq!(next_ease(Easing::EaseBoth), Easing::Even);
    }

    #[test]
    fn the_tag_reads_like_the_board() {
        let g = GroupGradient { angle: -14.0, ease: Easing::EaseOut, ..GroupGradient::default() };
        assert_eq!(gradient_tag(&g), "−14° · ease out");
    }

    #[test]
    fn the_gradient_line_spans_the_group_along_its_angle() {
        let places = [Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0)];
        let (a, b) = gradient_ends(&GroupGradient::default(), &places);
        assert!((a - Vec2::new(-50.0, 0.0)).length() < 1e-3 && (b - Vec2::new(50.0, 0.0)).length() < 1e-3);
        let wide = GroupGradient { stretch: 2.0, ..GroupGradient::default() };
        let (a, _) = gradient_ends(&wide, &places);
        assert!((a.x + 100.0).abs() < 1e-3);
    }
}
