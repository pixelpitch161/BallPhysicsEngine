//! A Text in the preview (Text part 1): any text is grabbed and moved by its letters; the selected
//! one shows its box with a corner dot that sets Size and an edge bar that sets Wrap width (on a
//! straight line or a wave), or its circle as a ring that sets Radius (round a circle). Each keys
//! at the playhead when the setting has keys.

use super::{Handle, View};
use crate::app::Studio;
use crate::style;
use bpe_core::{Address, Registry, Scene, Settings, ThingId, Value};
use bpe_kit::things::text::{KIND, TextAt, TextNow, TextPath, TextSettings, text_copies};
use bpe_runtime::{Log, World};
use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use glam::Vec2;
use std::f32::consts::TAU;

/// How close to a dot or the ring, in screen points, counts as grabbing it.
const GRAB: f32 = 9.0;

/// A part of a Text's handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPart {
    /// The box's bottom-right corner: the Size.
    Size,
    /// The box's right edge: the Wrap width.
    Wrap,
    /// The circle the letters go round: its Radius.
    Radius,
}

/// Text `id`'s own settings at `time`: each keyed one at its value there.
fn params_at(scene: &Scene, reg: &Registry, id: ThingId, time: f64) -> Option<bpe_core::Params> {
    let thing = scene.things.get(&id).filter(|t| t.kind == KIND)?;
    let mut params = thing.params.clone();
    for def in TextSettings::defs() {
        let a = Address::own(id, def.key);
        if scene.keyed(&a)
            && let Some(v) = scene.value_at_time(reg, &a, time)
        {
            params.set(def.key, v);
        }
    }
    Some(params)
}

/// Every copy of Text `id` as the preview shows it now (one a ball for a Text riding each ball),
/// and its settings. `log` lets a show timed by hits see them.
pub fn copies_of(
    scene: &Scene,
    reg: &Registry,
    world: &World,
    log: Option<&Log>,
    id: ThingId,
    time: f64,
) -> Option<(Vec<TextNow>, TextSettings)> {
    let params = params_at(scene, reg, id, time)?;
    let at = TextAt { log, ..TextAt::new(scene, &params, world, id, time) };
    Some((text_copies(&at), TextSettings::from_params(&params)))
}

/// Text `id` as the preview shows it now (its first copy), and its settings.
pub fn now_of(
    scene: &Scene,
    reg: &Registry,
    world: &World,
    log: Option<&Log>,
    id: ThingId,
    time: f64,
) -> Option<(TextNow, TextSettings)> {
    let (copies, s) = copies_of(scene, reg, world, log, id, time)?;
    Some((copies.into_iter().next()?, s))
}

fn copies(studio: &Studio, id: ThingId) -> Option<(Vec<TextNow>, TextSettings)> {
    let world = studio.playback.world();
    copies_of(&studio.scene, &studio.kit.registry, world, Some(studio.playback.log()), id, studio.sim_time())
}

fn now(studio: &Studio, id: ThingId) -> Option<(TextNow, TextSettings)> {
    let (copies, s) = copies(studio, id)?;
    Some((copies.into_iter().next()?, s))
}

/// The selected Text, when there is one.
fn selected(studio: &Studio) -> Option<ThingId> {
    let id = studio.selection.primary?;
    studio.scene.things.get(&id).filter(|t| t.kind == KIND && t.enabled).map(|_| id)
}

/// The dots of a Text, in its own frame: the box's corner and right edge (laid out in lines or on a
/// wave).
fn dots(now: &TextNow, s: &TextSettings) -> Vec<(TextPart, Vec2)> {
    if !matches!(s.path, TextPath::Straight | TextPath::AlongAWave) || now.layout.letters.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = (now.layout.min, now.layout.max);
    let mut out = vec![(TextPart::Size, hi)];
    if s.path == TextPath::Straight {
        out.push((TextPart::Wrap, Vec2::new(hi.x, 0.5 * (lo.y + hi.y))));
    }
    out
}

/// The handle of the selected Text under `pos`.
pub fn hit(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let id = selected(studio)?;
    let v = view.of(studio, id)?;
    let (now, s) = now(studio, id)?;
    if let Some((part, _)) =
        dots(&now, &s).into_iter().find(|(_, p)| (v.to_screen(now.to_world(*p)) - pos).length() <= GRAB)
    {
        return Some(Handle::Text(id, part));
    }
    if s.path == TextPath::RoundACircle {
        let middle = v.to_screen(now.to_world(Vec2::ZERO));
        if ((pos - middle).length() - s.radius.max(0.0) * v.world_scale()).abs() <= GRAB {
            return Some(Handle::Text(id, TextPart::Radius));
        }
    }
    None
}

/// Whether `pos` is on Text `id`'s letters (within the box round them), on any of its copies.
pub fn on_body(studio: &Studio, view: &View, id: ThingId, pos: Pos2) -> bool {
    let Some(v) = view.of(studio, id) else { return false };
    let Some((copies, _)) = copies(studio, id) else { return false };
    // A little room round thin letters.
    let room = 4.0 / v.world_scale().max(1e-6);
    copies.iter().any(|now| {
        let Some((lo, hi)) = now.bounds() else { return false };
        let p = now.to_own(v.to_world(pos));
        p.x >= lo.x - room && p.x <= hi.x + room && p.y >= lo.y - room && p.y <= hi.y + room
    })
}

/// The Text under `pos`, the last drawn first.
pub fn under(studio: &Studio, view: &View, pos: Pos2) -> Option<ThingId> {
    studio
        .scene
        .order
        .iter()
        .rev()
        .copied()
        .filter(|id| studio.scene.things.get(id).is_some_and(|t| t.kind == KIND && t.enabled))
        .find(|id| on_body(studio, view, *id, pos))
}

/// Applies a drag of `part` of Text `id` to `pointer` (in the text's world, or on the screen for a
/// screen text). Settings with keys get a key at `time`; true when one did.
pub fn apply(
    scene: &mut Scene,
    reg: &Registry,
    world: &World,
    id: ThingId,
    part: TextPart,
    pointer: Vec2,
    time: f64,
) -> bool {
    let Some((now, s)) = now_of(scene, reg, world, None, id, time) else { return false };
    let p = now.to_own(pointer);
    let mut keyed = false;
    let mut set = |scene: &mut Scene, key: &str, value: f32| {
        let a = Address::own(id, key);
        crate::inspector::set_or_key(scene, reg, &a, Value::Number(value as f64), time, &mut keyed);
    };
    match part {
        TextPart::Size => {
            // The corner's distance from the middle grows with the Size.
            let corner = now.layout.max.length();
            if corner > 1e-3 {
                let size = (s.size * p.length() / corner).round().max(1.0);
                set(scene, "size", size);
            }
        }
        // 0 would mean no wrapping at all.
        TextPart::Wrap => set(scene, "wrap", (2.0 * p.x.abs()).round().max(1.0)),
        TextPart::Radius => set(scene, "radius", p.length().round().max(1.0)),
    }
    keyed
}

/// Draws the selected Text's box (or circle) and its handles. `active` is the handle under the
/// pointer or being dragged.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View, active: Option<Handle>) {
    let Some(id) = selected(studio) else { return };
    let Some(v) = view.of(studio, id) else { return };
    let Some((copies, s)) = copies(studio, id) else { return };
    let Some(now) = copies.first() else { return };
    let ink = style::colour_of(&studio.scene, id);
    let stroke = Stroke::new(1.2, ink);
    let screen = |p: Vec2| v.to_screen(now.to_world(p));
    // A box round every copy; the handles on the first.
    for c in &copies {
        if let Some((lo, hi)) = c.bounds() {
            let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y), lo];
            painter.add(Shape::dashed_line(&corners.map(|p| v.to_screen(c.to_world(p))), stroke, 6.0, 4.0));
        }
    }
    let on = |part| active == Some(Handle::Text(id, part));
    if s.path == TextPath::RoundACircle {
        let r = s.radius.max(0.0);
        let ring: Vec<Pos2> = (0..=96).map(|i| screen(Vec2::from_angle(i as f32 / 96.0 * TAU) * r)).collect();
        let width = if on(TextPart::Radius) { 2.5 } else { 1.2 };
        painter.add(Shape::dashed_line(&ring, Stroke::new(width, ink), 6.0, 4.0));
    }
    for (part, p) in dots(now, &s) {
        let fill: Color32 = if on(part) { style::TEXT_STRONG } else { ink };
        let at = screen(p);
        match part {
            TextPart::Wrap => {
                let bar = egui::Rect::from_center_size(at, egui::vec2(5.0, 18.0));
                painter.rect(bar, 2.0, fill, Stroke::new(1.5, style::OUTLINE), egui::StrokeKind::Outside);
            }
            _ => {
                painter.circle(at, if on(part) { 6.5 } else { 5.0 }, fill, Stroke::new(1.5, style::OUTLINE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{Drag, apply_drag, hit_test};
    use bpe_runtime::CameraView;
    use eframe::egui::{Rect, vec2};

    fn view() -> View {
        View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
            stage: None,
        }
    }

    /// A blank studio with one Text saying `words`, selected, on the screen or in the world.
    fn studio_with_text(words: &str, screen: bool) -> (Studio, ThingId) {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let id = bpe_kit::add_thing(&mut studio.scene, reg, KIND, ThingId::ROOT).unwrap();
        let t = studio.scene.things.get_mut(&id).unwrap();
        t.params.set("words", Value::Text(words.into()));
        t.screen = Some(screen);
        t.transform.position = Vec2::new(0.0, 400.0);
        studio.scene_changed();
        studio.selection.set(id);
        (studio, id)
    }

    fn settings(studio: &Studio, id: ThingId) -> TextSettings {
        TextSettings::from_params(&studio.scene.things[&id].params)
    }

    /// Where own point `p` of Text `id` shows on the screen.
    fn screen_of(studio: &Studio, id: ThingId, p: Vec2) -> (Pos2, View) {
        let v = view().of(studio, id).unwrap();
        let (now, _) = now(studio, id).unwrap();
        (v.to_screen(now.to_world(p)), v)
    }

    fn drag_to(studio: &mut Studio, handle: Handle, pointer: Vec2) -> bool {
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let (time, drag) = (studio.sim_time(), Drag::new(handle, Vec2::ZERO));
        let world = studio.playback.world().clone();
        let keyed = apply_drag(&mut studio.scene, reg, &world, &drag, pointer, None, time);
        studio.scene_changed();
        keyed
    }

    #[test]
    fn clicking_a_text_selects_it_and_dragging_moves_it() {
        for screen in [false, true] {
            let (mut studio, id) = studio_with_text("HELLO", screen);
            let (middle, v) = screen_of(&studio, id, Vec2::ZERO);
            studio.selection.clear();
            assert_eq!(hit_test(&studio, &view(), middle), Some(Handle::Body(id)), "screen {screen}");
            assert_eq!(hit_test(&studio, &view(), middle + vec2(0.0, 60.0)), None, "off the letters");
            // Dragged by its middle to a new place.
            let to = v.to_world(middle + vec2(20.0, -40.0));
            drag_to(&mut studio, Handle::Body(id), to);
            assert_eq!(studio.scene.things[&id].transform.position, to.round(), "screen {screen}");
        }
    }

    #[test]
    fn dragging_the_corner_dot_scales_size() {
        let (mut studio, id) = studio_with_text("HELLO", false);
        let (now0, _) = now(&studio, id).unwrap();
        let corner = now0.layout.max;
        let (at, _) = screen_of(&studio, id, corner);
        assert_eq!(hit(&studio, &view(), at), Some(Handle::Text(id, TextPart::Size)));
        let size = settings(&studio, id).size;
        drag_to(&mut studio, Handle::Text(id, TextPart::Size), now0.to_world(corner * 2.0));
        assert!((settings(&studio, id).size - 2.0 * size).abs() <= 1.0, "{}", settings(&studio, id).size);
        // A keyed Size keys at the playhead.
        let kit = studio.kit.clone();
        let reg = &kit.registry;
        let a = Address::own(id, "size");
        studio.scene.set_key(reg, &a, 0.0, Value::Number(100.0)).unwrap();
        studio.scene_changed();
        studio.time = 1.0;
        let (now1, _) = now(&studio, id).unwrap();
        assert!(drag_to(&mut studio, Handle::Text(id, TextPart::Size), now1.to_world(now1.layout.max * 1.5)));
        let track = &studio.scene.tracks[&a];
        assert!(track.keys.iter().any(|k| (k.time - 1.0).abs() < 1e-9), "a key at the playhead");
    }

    #[test]
    fn dragging_the_edge_bar_sets_the_wrap_width() {
        let (mut studio, id) = studio_with_text("ONE TWO THREE", false);
        let (now0, _) = now(&studio, id).unwrap();
        let edge = Vec2::new(now0.layout.max.x, 0.5 * (now0.layout.min.y + now0.layout.max.y));
        let (at, _) = screen_of(&studio, id, edge);
        assert_eq!(hit(&studio, &view(), at), Some(Handle::Text(id, TextPart::Wrap)));
        drag_to(&mut studio, Handle::Text(id, TextPart::Wrap), now0.to_world(Vec2::new(300.0, 0.0)));
        assert_eq!(settings(&studio, id).wrap, 600.0);
        // Narrower than the narrowest word: every word is still there, one a line.
        drag_to(&mut studio, Handle::Text(id, TextPart::Wrap), now0.to_world(Vec2::new(5.0, 0.0)));
        let (now1, _) = now(&studio, id).unwrap();
        assert_eq!(now1.layout.letters.len(), 11, "every letter kept");
        assert_eq!(now1.layout.lines, 3);
    }

    #[test]
    fn the_radius_ring_shows_only_on_a_circle_path_and_sets_radius() {
        let (mut studio, id) = studio_with_text("ROUND", false);
        let top = |studio: &Studio| {
            let r = settings(studio, id).radius;
            screen_of(studio, id, Vec2::new(0.0, -r)).0
        };
        assert_ne!(hit(&studio, &view(), top(&studio)), Some(Handle::Text(id, TextPart::Radius)));
        studio.scene.things.get_mut(&id).unwrap().params.set("path", Value::Choice("round_a_circle".into()));
        studio.scene_changed();
        assert_eq!(hit(&studio, &view(), top(&studio)), Some(Handle::Text(id, TextPart::Radius)));
        let (now0, _) = now(&studio, id).unwrap();
        drag_to(&mut studio, Handle::Text(id, TextPart::Radius), now0.to_world(Vec2::new(450.0, 0.0)));
        assert_eq!(settings(&studio, id).radius, 450.0);
    }
}
