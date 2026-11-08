//! The Mixer: the left panel's other face. The tempo row, then the console strips (Bus 1, Bus 2,
//! Bus 3 and the Master), then the loudness readouts and graph. A bus effect or the Master opens
//! its editor under the strips' tops, across the three buses, in place of their sends and pans.

pub mod cx;
pub mod editors;
pub mod master;
pub mod slot_menu;
pub mod strips;

use crate::style;
use bpe_core::LayerId;
use bpe_core::{Address, Resolved, Scene, ThingId, Value};
use bpe_kit::Kit;
use bpe_kit::sound::BUSES;
use bpe_sound::{BusReading, MeterReading, Meters};
pub use cx::{MixCx, MixerPick};
use eframe::egui::{self, Rect, RichText, Ui};

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
    /// The bus effect or the Master whose editor is open.
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
/// The narrowest a strip draws (a knob's dial and a little); a narrower Mixer scrolls sideways.
const MIN_STRIP_W: f32 = 48.0;

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
    let mut keyed = false;
    // Too narrow for four strips side by side, the Mixer scrolls sideways.
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        // Measured before anything is drawn: a row wider than the panel widens what is left.
        let width = ((ui.available_width() - 3.0 * GAP) / 4.0).floor().max(MIN_STRIP_W);
        tempo_row(ui, scene, kit, time);
        style::divider(ui);
        let choices = crate::widgets::TargetChoices::default();
        let graph =
            strips::mixer_settings(&MixCx { scene, kit, choices: &choices, time, layer: None, edits: Vec::new() });
        // The strips in three rows of columns that line up: the tops (name and effects; the
        // Master's title and knobs), then the sends and pans or the open editor across the
        // three buses, then the faders and meters.
        let mut tops = [Rect::NOTHING; BUSES];
        let mut bottoms = [Rect::NOTHING; BUSES];
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let mut cx = MixCx { scene, kit, choices: &choices, time, layer: None, edits: Vec::new() };
            for (lane, top) in tops.iter_mut().enumerate() {
                *top = strips::column(ui, width, |ui| strips::bus_top(ui, &mut cx, mixer, lane as u8 + 1)).1;
            }
            edits.append(&mut cx.edits);
            keyed |= strips::column(ui, width, |ui| strips::master_top(ui, scene, kit, mixer, time)).0;
        });
        let mut editor = None;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            if mixer.pick.is_some() {
                let readings = editors::Readings { meters: bus_meters, master: reading };
                let across = 3.0 * width + 2.0 * GAP;
                let (k, rect) = strips::column(ui, across, |ui| {
                    // The editor is spaced as the rest of the editor is, not as the strips.
                    ui.spacing_mut().item_spacing = ui.ctx().global_style().spacing.item_spacing;
                    editors::show(ui, mixer, scene, kit, &readings, time)
                });
                keyed |= k;
                editor = Some(rect);
            } else {
                let mut cx = MixCx { scene, kit, choices: &choices, time, layer: None, edits: Vec::new() };
                for n in 1..=BUSES as u8 {
                    strips::column(ui, width, |ui| strips::bus_sends(ui, &mut cx, &graph, n));
                }
                edits.append(&mut cx.edits);
            }
        });
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let mut cx = MixCx { scene, kit, choices: &choices, time, layer: None, edits: Vec::new() };
            for (lane, bottom) in bottoms.iter_mut().enumerate() {
                let bus = bus_meters.map_or(BusReading::SILENT, |m| m.read_bus(lane));
                *bottom = strips::column(ui, width, |ui| strips::bus_bottom(ui, &mut cx, lane as u8 + 1, &bus)).1;
            }
            edits.append(&mut cx.edits);
            strips::column(ui, width, |ui| strips::master_bottom(ui, &reading));
        });
        mixer.hover_bus = hovered_bus(ui, scene, mixer.pick, &tops, &bottoms, editor);
        style::divider(ui);
        master::loudness(ui, &reading, &mixer.loudness);
    });
    let applied = cx::apply(scene, kit, time, edits, &mut mixer.copied);
    if let Some(pick) = applied.pick {
        mixer.pick = pick;
        // The editor opens or closes on the next frame.
        ui.ctx().request_repaint();
    }
    applied.keyed || keyed
}

/// The bus (1..=3) whose strip is under the pointer, from the top of its column to the
/// bottom; over an open effect's editor, that effect's bus.
fn hovered_bus(
    ui: &Ui,
    scene: &Scene,
    pick: Option<MixerPick>,
    tops: &[Rect; BUSES],
    bottoms: &[Rect; BUSES],
    editor: Option<Rect>,
) -> Option<u8> {
    if let Some(rect) = editor
        && ui.rect_contains_pointer(rect)
    {
        let MixerPick::Layer(id) = pick? else { return None };
        let lane = scene.card(ThingId::ROOT, bpe_core::SlotId::Mixer)?.layer(id)?.lane.unwrap_or(0);
        return Some(lane + 1);
    }
    (0..BUSES).find_map(|lane| {
        let column = Rect::from_x_y_ranges(tops[lane].x_range(), tops[lane].top()..=bottoms[lane].bottom());
        ui.rect_contains_pointer(column).then_some(lane as u8 + 1)
    })
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
    ui.horizontal_wrapped(|ui| {
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
        let label = ui.label(RichText::new("First beat at").color(style::MUTED));
        let mut first = root_value(scene, kit, "first_beat", time).as_ref().and_then(number).unwrap_or(0.0);
        let r = ui
            .add(egui::DragValue::new(&mut first).range(0.0..=600.0).speed(0.005).max_decimals(3).suffix(" s"))
            .labelled_by(label.id)
            .on_hover_text("Where bar 1 starts: the grid, synced LFOs, slot changes and beat moves count from here");
        if r.changed() {
            set_root(scene, kit, "first_beat", Value::Number(first), time);
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
