//! A shape's Distance zones in the preview: while its Sound tab is open, each Distance that is
//! on shows its Near and Far as dotted lines in the shape's colour (circles round the middle
//! for Centre, the outline grown and shrunk for Edge). Dragging a line sets Near or Far.

use super::{Handle, View};
use crate::app::Studio;
use crate::inspector::InspectorTab;
use crate::style;
use bpe_core::{LayerId, Params, Scene, Settings, Slot, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{DISTANCE, DistanceSettings, Measure};
use bpe_runtime::{Outline, World};
use eframe::egui::{self, Pos2, Shape};
use glam::Vec2;

/// How close to a line, in screen points, counts as grabbing it.
const LINE_GRAB: f32 = 6.0;
/// Points along an Edge zone.
const EDGE_POINTS: usize = 160;

/// One Distance's zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub shape: ThingId,
    pub layer: LayerId,
    pub measure: Measure,
    pub near: f32,
    pub far: f32,
}

/// The zones shown: the selected shape's Distances that are on, while its Sound tab is open.
pub fn zones(studio: &Studio) -> Vec<Zone> {
    let Some(shape) = studio.selection.primary else { return Vec::new() };
    if studio.panels.inspector.tab.get(&shape) != Some(&InspectorTab::Sound) {
        return Vec::new();
    }
    let Some(card) = studio.scene.card(shape, SlotId::Sound) else { return Vec::new() };
    let reg = &studio.kit.registry;
    let time = studio.sim_time();
    card.layers
        .iter()
        .filter(|l| l.kind == DISTANCE && l.enabled)
        .map(|l| {
            let s = settings_of(card, l.id);
            let at = |key: &str, set: f32| {
                let a = bpe_core::Address::on(shape, SlotId::Sound, &layered_key(l.id, key));
                match studio.scene.value_at_time(reg, &a, time) {
                    Some(Value::Number(v)) => v as f32,
                    _ => set,
                }
            };
            Zone { shape, layer: l.id, measure: s.measure, near: at("near", s.near), far: at("far", s.far) }
        })
        .collect()
}

/// Distance `layer`'s settings as set on the card.
fn settings_of(card: &Slot, layer: LayerId) -> DistanceSettings {
    let prefix = format!("{layer}/");
    let own = card.params.iter().filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_owned(), v.clone())));
    DistanceSettings::from_params(&Params(own.collect()))
}

/// The shape's outline and middle as the world has them now.
fn outline(world: &World, shape: ThingId) -> Option<&Outline> {
    world.walls.get(&shape).map(|w| &w.outline)
}

/// The points of a zone's line `d` pixels from the shape, in world pixels.
fn line(o: &Outline, measure: Measure, d: f32) -> Vec<Vec2> {
    match measure {
        Measure::Centre => {
            let c = o.center();
            (0..=96).map(|i| c + Vec2::from_angle(i as f32 / 96.0 * std::f32::consts::TAU) * d).collect()
        }
        Measure::Edge => (0..=EDGE_POINTS)
            .map(|i| {
                let p = o.point(i as f32 / EDGE_POINTS as f32);
                let out = (p - o.center()).normalize_or_zero();
                p + out * d
            })
            .collect(),
    }
}

/// How far `p` is from the shape as a zone of `measure` measures.
fn measured(o: &Outline, measure: Measure, p: Vec2) -> f32 {
    match measure {
        Measure::Centre => (p - o.center()).length(),
        Measure::Edge => (p - o.nearest(p).0).length(),
    }
}

/// Draws the zones.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View) {
    let world = studio.playback.world();
    for z in zones(studio) {
        let (Some(o), Some(v)) = (outline(world, z.shape), view.of(studio, z.shape)) else { continue };
        let colour = style::colour_of(&studio.scene, z.shape);
        for (d, alpha) in [(z.near, 1.0), (z.far, 0.6)] {
            if d <= 0.0 && z.measure == Measure::Centre {
                continue;
            }
            let pts: Vec<Pos2> = line(o, z.measure, d).into_iter().map(|p| v.to_screen(p)).collect();
            painter.extend(Shape::dotted_line(&pts, colour.gamma_multiply(alpha), 6.0, 1.3));
            if z.measure == Measure::Edge && d > 0.0 {
                let inner: Vec<Pos2> = line(o, z.measure, -d).into_iter().map(|p| v.to_screen(p)).collect();
                painter.extend(Shape::dotted_line(&inner, colour.gamma_multiply(alpha), 6.0, 1.3));
            }
        }
    }
}

/// The zone line under `pos`, if any: Far before Near when they sit together.
pub fn hit(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let world = studio.playback.world();
    zones(studio).into_iter().find_map(|z| {
        let (o, v) = (outline(world, z.shape)?, view.of(studio, z.shape)?);
        let d = measured(o, z.measure, v.to_world(pos));
        let reach = LINE_GRAB / v.world_scale();
        // A line that isn't drawn (at the middle) can't be grabbed.
        let drawn = |at: f32| at > 0.0 || z.measure == Measure::Edge;
        if drawn(z.far) && (d - z.far).abs() <= reach {
            Some(Handle::Zone(z.shape, z.layer, true))
        } else if drawn(z.near) && (d - z.near).abs() <= reach {
            Some(Handle::Zone(z.shape, z.layer, false))
        } else {
            None
        }
    })
}

/// A zone line dragged to `pointer` (world pixels): Far (or Near) becomes its distance.
pub fn apply(scene: &mut Scene, world: &World, shape: ThingId, layer: LayerId, far: bool, pointer: Vec2) {
    let Some(o) = outline(world, shape) else { return };
    let Some(card) = scene.card(shape, SlotId::Sound) else { return };
    let measure = settings_of(card, layer).measure;
    let d = measured(o, measure, pointer).round().clamp(0.0, 4000.0) as f64;
    let key = layered_key(layer, if far { "far" } else { "near" });
    if let Some(card) = scene.card_mut(shape, SlotId::Sound) {
        card.params.set(&key, Value::Number(d));
    }
}
