//! A selected Particles thing's handles in the preview: where its particles are born, drawn as
//! its area (a rectangle, circle or ring), its line or its point, with a dot to size the area (a
//! rectangle's corner, a circle's or ring's edge, a ring's inside) or move a line's end (its
//! length and turn). Its place drags as any thing's does (the spot in its middle). Every dot keys
//! at the playhead when the setting has keys.

use super::{Handle, View};
use crate::app::Studio;
use crate::style;
use bpe_core::{Address, Registry, Scene, Settings, ThingId, Value};
use bpe_kit::things::particles::{AreaShape, BornFrom, KIND, ParticlesSettings};
use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use glam::Vec2;
use std::f32::consts::TAU;

/// How close to a dot, in screen points, counts as grabbing it.
const DOT_GRAB: f32 = 9.0;

/// A part of a Particles thing's handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticlePart {
    /// A rectangle's corner: its width and height.
    Corner,
    /// A circle's or ring's edge: its radius.
    Radius,
    /// A ring's inside edge.
    Inner,
    /// One end of a line: its length and turn.
    End,
}

/// The Particles thing's settings at the playhead: each keyed setting at its value there.
fn settings_at(scene: &Scene, reg: &Registry, id: ThingId, time: f64) -> Option<ParticlesSettings> {
    let thing = scene.things.get(&id)?;
    let mut params = thing.params.clone();
    for def in ParticlesSettings::defs() {
        let a = Address::own(id, def.key);
        if scene.keyed(&a)
            && let Some(v) = scene.value_at_time(reg, &a, time)
        {
            params.set(def.key, v);
        }
    }
    Some(ParticlesSettings::from_params(&params))
}

/// The selected Particles thing, its settings, its place and turn (radians), when its births
/// have a shape to show.
fn source(studio: &Studio) -> Option<(ThingId, ParticlesSettings, Vec2, f32)> {
    let id = studio.selection.primary?;
    let thing = studio.scene.things.get(&id).filter(|t| t.kind == KIND)?;
    let s = settings_at(&studio.scene, &studio.kit.registry, id, studio.sim_time())?;
    matches!(s.from, BornFrom::AnArea | BornFrom::ALine | BornFrom::APoint).then_some(())?;
    let at = super::place_of(&studio.scene, studio.playback.world(), id).unwrap_or(thing.transform.position);
    Some((id, s, at, thing.transform.rotation))
}

/// The way the area's or line's x axis points.
fn axis(s: &ParticlesSettings, turn: f32) -> Vec2 {
    Vec2::from_angle(turn + s.line_turn.to_radians())
}

/// The dots of the source, in world pixels.
fn dots(s: &ParticlesSettings, at: Vec2, turn: f32) -> Vec<(ParticlePart, Vec2)> {
    let x = axis(s, turn);
    let y = x.perp();
    match s.from {
        BornFrom::AnArea => match s.area {
            AreaShape::Rectangle => {
                vec![(ParticlePart::Corner, at + x * 0.5 * s.width.max(0.0) + y * 0.5 * s.height.max(0.0))]
            }
            AreaShape::Circle => vec![(ParticlePart::Radius, at + x * s.radius.max(0.0))],
            AreaShape::Ring => vec![
                (ParticlePart::Radius, at + x * s.radius.max(0.0)),
                (ParticlePart::Inner, at - x * s.inner.max(0.0).min(s.radius.max(0.0))),
            ],
        },
        BornFrom::ALine => vec![(ParticlePart::End, at + x * 0.5 * s.length.max(0.0))],
        _ => Vec::new(),
    }
}

/// The handle under `pos`, for the selected Particles thing.
pub fn hit(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let (id, s, at, turn) = source(studio)?;
    let v = view.of(studio, id)?;
    dots(&s, at, turn)
        .into_iter()
        .find(|(_, p)| (v.to_screen(*p) - pos).length() <= DOT_GRAB)
        .map(|(part, _)| Handle::Particles(id, part))
}

/// Applies a drag of `part` of Particles thing `id` to `pointer` (world pixels). Settings with
/// keys get a key at `time`; true when one did.
pub fn apply(scene: &mut Scene, reg: &Registry, id: ThingId, part: ParticlePart, pointer: Vec2, time: f64) -> bool {
    let Some(thing) = scene.things.get(&id) else { return false };
    let (at, turn) = (thing.transform.position, thing.transform.rotation);
    let Some(s) = settings_at(scene, reg, id, time) else { return false };
    let mut keyed = false;
    let mut set = |scene: &mut Scene, key: &str, value: f32| {
        let a = Address::own(id, key);
        crate::inspector::set_or_key(scene, reg, &a, Value::Number(value.round() as f64), time, &mut keyed);
    };
    let local = Vec2::from_angle(-(turn + s.line_turn.to_radians())).rotate(pointer - at);
    match part {
        ParticlePart::Corner => {
            set(scene, "width", 2.0 * local.x.abs());
            set(scene, "height", 2.0 * local.y.abs());
        }
        ParticlePart::Radius => set(scene, "radius", local.length()),
        ParticlePart::Inner => set(scene, "inner", local.length().min(s.radius)),
        ParticlePart::End => {
            set(scene, "length", 2.0 * local.length());
            let d = pointer - at;
            if d.length_squared() > 1e-6 {
                let deg = d.y.atan2(d.x).to_degrees() - turn.to_degrees();
                set(scene, "line_turn", (deg + 180.0).rem_euclid(360.0) - 180.0);
            }
        }
    }
    keyed
}

/// Draws the selected Particles thing's source and its dots. `active` is the handle under the
/// pointer or being dragged.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View, active: Option<Handle>) {
    let Some((id, s, at, turn)) = source(studio) else { return };
    let Some(v) = view.of(studio, id) else { return };
    let ink = style::colour_of(&studio.scene, id);
    let x = axis(&s, turn);
    let y = x.perp();
    let line = |points: Vec<Vec2>| -> Vec<Pos2> { points.into_iter().map(|p| v.to_screen(p)).collect() };
    let round = |r: f32| line((0..=96).map(|i| at + Vec2::from_angle(i as f32 / 96.0 * TAU) * r).collect());
    let stroke = Stroke::new(1.2, ink);
    match s.from {
        BornFrom::AnArea => match s.area {
            AreaShape::Rectangle => {
                let h = Vec2::new(0.5 * s.width.max(0.0), 0.5 * s.height.max(0.0));
                let corners = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (1.0, 1.0)];
                let outline = line(corners.iter().map(|(a, b)| at + x * h.x * *a + y * h.y * *b).collect());
                painter.add(Shape::dashed_line(&outline, stroke, 6.0, 4.0));
            }
            AreaShape::Circle => {
                painter.add(Shape::dashed_line(&round(s.radius.max(0.0)), stroke, 6.0, 4.0));
            }
            AreaShape::Ring => {
                painter.add(Shape::dashed_line(&round(s.radius.max(0.0)), stroke, 6.0, 4.0));
                painter.add(Shape::dashed_line(&round(s.inner.max(0.0).min(s.radius.max(0.0))), stroke, 6.0, 4.0));
            }
        },
        BornFrom::ALine => {
            let half = x * 0.5 * s.length.max(0.0);
            painter.line_segment([v.to_screen(at - half), v.to_screen(at + half)], Stroke::new(2.0, ink));
        }
        _ => {}
    }
    for (part, p) in dots(&s, at, turn) {
        let on = active == Some(Handle::Particles(id, part));
        let fill: Color32 = if on { style::TEXT_STRONG } else { ink };
        painter.circle(v.to_screen(p), if on { 6.5 } else { 5.0 }, fill, Stroke::new(1.5, style::OUTLINE));
    }
}
