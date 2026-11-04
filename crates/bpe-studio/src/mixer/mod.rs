//! The Mixer: the left panel's other face. The tempo row, then the console strips (Bus 1, Bus 2,
//! Bus 3 and the Master), then the loudness readouts and graph. A bus effect or the Master opens
//! its editor in the wide inspector.

pub mod cx;
pub mod editors;
pub mod master;
pub mod slot_menu;
pub mod strips;

use crate::style;
use bpe_core::LayerId;
use bpe_core::{Address, Resolved, Scene, ThingId, Value};
use bpe_kit::Kit;
use bpe_sound::{MeterReading, Meters};
pub use cx::{MixCx, MixerPick};
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
    /// The bus effect or the Master open in the inspector.
    pub pick: Option<MixerPick>,
    /// The effect last copied.
    pub copied: cx::Copied,
    /// A name being typed, and the text so far.
    pub renaming: Option<(Renaming, String)>,
    /// The bus strip under the pointer (1..=3): the preview rings the shapes playing into it.
    pub hover_bus: Option<u8>,
    /// The EQ band picked in the EQ editor (0..7), the one being dragged, and the one whose
    /// menu is open.
    pub eq_band: usize,
    pub eq_drag: Option<usize>,
    pub eq_menu: Option<usize>,
    /// The Master's last reading, for its editor.
    pub reading: Option<MeterReading>,
}

/// What is being renamed in the Mixer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renaming {
    Bus(u8),
    Layer(LayerId),
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

/// Space between two strips.
const GAP: f32 = 6.0;

/// The Mixer panel. `bus_meters` has the buses' and effects' levels while the sound plays.
/// Returns whether a keyed setting was edited (the studio pauses, so a drag keys one place).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    mixer: &mut Mixer,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    meters: Option<&MeterReading>,
    bus_meters: Option<&Meters>,
    time: f64,
) -> bool {
    let _ = resolved;
    mixer.record(meters, time);
    let reading = meters.copied().unwrap_or(MeterReading::SILENT);
    mixer.reading = meters.copied();
    // A picked effect that is gone (deleted, or undone) closes.
    if let Some(MixerPick::Layer(id)) = mixer.pick
        && scene.card(ThingId::ROOT, bpe_core::SlotId::Mixer).is_none_or(|c| c.layer(id).is_none())
    {
        mixer.pick = None;
    }
    let mut edits = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        tempo_row(ui, scene, kit, time);
        style::divider(ui);
        let width = ((ui.available_width() - 3.0 * GAP) / 4.0).floor();
        let choices = crate::widgets::TargetChoices::default();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let mut cx = MixCx { scene, kit, choices: &choices, time, layer: None, edits: Vec::new() };
            strips::buses(ui, &mut cx, mixer, bus_meters, width);
            edits = cx.edits;
            strips::master(ui, scene, kit, mixer, &reading, time, width);
        });
        style::divider(ui);
        master::loudness(ui, &reading, &mixer.loudness);
    });
    let applied = cx::apply(scene, kit, time, edits, &mut mixer.copied);
    if let Some(pick) = applied.pick {
        mixer.pick = pick;
    }
    applied.keyed
}

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
