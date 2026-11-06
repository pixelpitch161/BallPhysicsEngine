//! Free points in the preview (gradients part 2): while a shape is selected, a dot for each stop
//! of its Look layers' gradients laid out as Free points; with the scene settings selected, the
//! background's when it follows the place. Dragging a dot moves its stop's point
//! (`~points`, the whole list at once).

use bpe_core::{Address, LayerId, Scene, SlotId, ThingId, Value, layered_key};
use bpe_runtime::{CameraView, World};
use eframe::egui::{Painter, Pos2, Stroke};
use glam::Vec2;

use super::{Handle, View};
use crate::app::Studio;
use crate::look_editor::free_points::{FreeOn, free_on, points_key};
use crate::shape_handles;
use crate::style;

/// How close to a dot, in screen points, counts as grabbing it.
const DOT_GRAB: f32 = 9.0;

/// One gradient's dots: whose (a shape's layer, or the scene settings' own with `layer` None),
/// and where its form's space sits in the world.
struct Dots {
    thing: ThingId,
    slot: SlotId,
    layer: Option<LayerId>,
    on: FreeOn,
    /// The shape's frame (its form space is in shape units), or none (already in the world).
    frame: Option<shape_handles::ShapeFrame>,
    /// The background measured on the screen (pixels from the video's middle), not the world.
    screen: bool,
}

impl Dots {
    fn world(&self, q: Vec2) -> Vec2 {
        let at = self.on.to_ball(q);
        self.frame.map_or(at, |f| f.to_world(at))
    }

    fn form_point(&self, world: Vec2) -> Vec2 {
        self.frame.map_or(world, |f| f.to_shape(world))
    }

    fn key(&self) -> String {
        let k = points_key(&self.on);
        self.layer.map_or(k.clone(), |l| layered_key(l, &k))
    }
}

/// Every set of dots the selection shows.
fn dots(studio: &Studio) -> Vec<Dots> {
    let Some(id) = studio.selection.primary else { return Vec::new() };
    dots_of(&studio.scene, &studio.kit.registry, studio.playback.world(), id, studio.sim_time())
}

fn dots_of(scene: &Scene, reg: &bpe_core::Registry, world: &World, id: ThingId, time: f64) -> Vec<Dots> {
    let Some(thing) = scene.things.get(&id) else { return Vec::new() };
    let value = |a: Address| scene.value_at_time(reg, &a, time);
    if id == ThingId::ROOT {
        let Some(kind) = reg.thing(&thing.kind) else { return Vec::new() };
        let get = |k: &str| value(Address::own(id, k));
        let screen = !matches!(get("background_on_screen"), Some(Value::Bool(false)));
        return free_on(kind.settings(), &get)
            .into_iter()
            .map(|on| Dots { thing: id, slot: SlotId::Look, layer: None, on, frame: None, screen })
            .collect();
    }
    let Some(seen) = shape_handles::seen(world, id) else { return Vec::new() };
    let Some(card) = scene.card(id, SlotId::Look) else { return Vec::new() };
    let mut out = Vec::new();
    for layer in card.layers.iter().filter(|l| l.enabled) {
        let Some(kind) = reg.layer(&layer.kind) else { continue };
        let get = |k: &str| value(Address::on(id, SlotId::Look, &layered_key(layer.id, k)));
        for on in free_on(kind.settings(), &get) {
            out.push(Dots {
                thing: id,
                slot: SlotId::Look,
                layer: Some(layer.id),
                on,
                frame: Some(seen.frame),
                screen: false,
            });
        }
    }
    out
}

/// The view `d`'s dots are seen through: the camera's, or none for a background on the screen.
fn view_for(studio: &Studio, view: &View, d: &Dots) -> Option<View> {
    if d.screen {
        return (!view.world).then_some(View { camera: CameraView::default(), stage: None, ..*view });
    }
    view.of(studio, d.thing)
}

/// Where every dot the selection shows sits, in the world (or on the screen for the background).
#[cfg(test)]
pub fn dots_for_test(studio: &Studio) -> Vec<Vec2> {
    dots(studio).iter().flat_map(|d| d.on.points.iter().map(|(q, _)| d.world(*q)).collect::<Vec<_>>()).collect()
}

/// The dots, each in its stop's colour.
pub fn draw(painter: &Painter, studio: &Studio, view: &View, active: Option<Handle>) {
    for (n, d) in dots(studio).iter().enumerate() {
        let Some(v) = view_for(studio, view, d) else { continue };
        for (i, (q, colour)) in d.on.points.iter().enumerate() {
            let at = v.to_screen(d.world(*q));
            let on = active == Some(Handle::FreePoint(d.thing, n as u8, i as u8));
            let r = if on { 7.0 } else { 5.5 };
            let fill = eframe::egui::Color32::from_rgb(colour.r, colour.g, colour.b); // scene colour
            painter.circle_filled(at, r, fill);
            painter.circle_stroke(at, r, Stroke::new(1.5, style::TEXT_STRONG));
        }
    }
}

/// The dot under `pos`, if any.
pub fn hit(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    dots(studio).iter().enumerate().find_map(|(n, d)| {
        let v = view_for(studio, view, d)?;
        d.on.points
            .iter()
            .position(|(q, _)| (v.to_screen(d.world(*q)) - pos).length() <= DOT_GRAB)
            .map(|i| Handle::FreePoint(d.thing, n as u8, i as u8))
    })
}

/// Where the pointer at `pos` is for dragging free point `handle`: in the world, or on the
/// screen for a background that stays there.
pub fn pointer(studio: &Studio, view: &View, handle: Handle, pos: Pos2) -> Vec2 {
    let Handle::FreePoint(_, n, _) = handle else { return view.pointer_for(studio, handle, pos) };
    match dots(studio).get(n as usize) {
        Some(d) => view_for(studio, view, d).unwrap_or(*view).to_world(pos),
        None => view.pointer_for(studio, handle, pos),
    }
}

/// Dot `i` of set `n` of `thing` dragged to `pointer` (world pixels): its point moves there.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    world: &World,
    thing: ThingId,
    n: u8,
    i: u8,
    pointer: Vec2,
    time: f64,
) -> bool {
    let all = dots_of(scene, reg, world, thing, time);
    let Some(d) = all.get(n as usize) else { return false };
    let moved: Value = d.on.moved(i as usize, d.form_point(pointer));
    let a = match d.layer {
        Some(_) => Address::on(d.thing, d.slot, &d.key()),
        None => Address::own(d.thing, &d.key()),
    };
    let mut keyed = false;
    crate::inspector::set_or_key(scene, reg, &a, moved, time, &mut keyed);
    keyed
}
