//! The Mixer: the left panel's other face. Part 1 has the tempo row and the Master strip; the
//! buses' strips go beside the Master (part 6).

pub mod master;

use crate::style;
use bpe_core::{Address, Resolved, Scene, ThingId, Value};
use bpe_kit::Kit;
use bpe_sound::MeterReading;
use eframe::egui::{self, RichText, Ui};

/// Readings the loudness graph keeps: 30 s at ten a second.
const HISTORY: usize = 300;
/// Seconds between two readings on the graph.
const HISTORY_STEP: f64 = 0.1;

/// What the Mixer remembers between frames.
#[derive(Default)]
pub struct Mixer {
    /// Short-term loudness readings, oldest first.
    pub loudness: Vec<f32>,
    /// The play time of the last reading.
    last: Option<f64>,
}

impl Mixer {
    /// Takes a reading for the graph every tenth of a second of play; a jump back starts over.
    fn record(&mut self, meters: Option<&MeterReading>, time: f64) {
        let Some(m) = meters else { return };
        if self.last.is_some_and(|last| time < last) {
            self.loudness.clear();
            self.last = None;
        }
        if self.last.is_none_or(|last| time - last >= HISTORY_STEP) {
            self.loudness.push(m.short_term);
            if self.loudness.len() > HISTORY {
                self.loudness.remove(0);
            }
            self.last = Some(time);
        }
    }
}

/// The Mixer panel.
pub fn show(
    ui: &mut Ui,
    mixer: &mut Mixer,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    meters: Option<&MeterReading>,
    time: f64,
) {
    let _ = resolved;
    mixer.record(meters, time);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        tempo_row(ui, scene, kit, time);
        style::divider(ui);
        ui.horizontal_top(|ui| {
            master::strip(ui, scene, kit, meters, &mixer.loudness, time);
            bus_strips(ui, scene, kit, time);
        });
    });
}

/// The buses' strips beside the Master. Part 6 fills this in.
pub fn bus_strips(_ui: &mut Ui, _scene: &mut Scene, _kit: &Kit, _time: f64) {}

/// A number setting's value as a number.
pub(crate) fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(x) => Some(*x),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// The scene setting `key` at the playhead.
fn root_value(scene: &Scene, kit: &Kit, key: &str, time: f64) -> Option<Value> {
    scene.value_at_time(&kit.registry, &Address::own(ThingId::ROOT, key), time)
}

/// Sets a scene setting (a key at the playhead when it has keys).
fn set_root(scene: &mut Scene, kit: &Kit, key: &str, value: Value, time: f64) {
    let mut keyed = false;
    crate::inspector::set_or_key(scene, &kit.registry, &Address::own(ThingId::ROOT, key), value, time, &mut keyed);
}

/// Tempo, Beats in a bar and Beat note.
fn tempo_row(ui: &mut Ui, scene: &mut Scene, kit: &Kit, time: f64) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let label = ui.label(RichText::new("Tempo").color(style::MUTED));
        let mut bpm = root_value(scene, kit, "tempo", time).as_ref().and_then(number).unwrap_or(120.0);
        let r = ui
            .add(egui::DragValue::new(&mut bpm).range(20.0..=300.0).speed(0.1).max_decimals(2).suffix(" BPM"))
            .labelled_by(label.id);
        if r.changed() {
            set_root(scene, kit, "tempo", Value::Number(bpm), time);
        }
        ui.add_space(8.0);
        let label = ui.label(RichText::new("Beats in a bar").color(style::MUTED));
        let mut top = root_value(scene, kit, "time_top", time).as_ref().and_then(number).map_or(4, |v| v as i64);
        let r = ui.add(egui::DragValue::new(&mut top).range(1..=16).speed(0.05)).labelled_by(label.id);
        if r.changed() {
            set_root(scene, kit, "time_top", Value::Int(top), time);
        }
        ui.add_space(8.0);
        let label = ui.label(RichText::new("Beat note").color(style::MUTED));
        let bottom = match root_value(scene, kit, "time_bottom", time) {
            Some(Value::Choice(c)) => c,
            _ => "4".to_owned(),
        };
        let mut picked = None;
        ui.menu_button(bottom.clone(), |ui| {
            for name in ["2", "4", "8", "16"] {
                if ui.selectable_label(name == bottom, name).clicked() {
                    picked = Some(name);
                    ui.close();
                }
            }
        })
        .response
        .labelled_by(label.id);
        if let Some(name) = picked {
            set_root(scene, kit, "time_bottom", Value::Choice(name.into()), time);
        }
    });
}
