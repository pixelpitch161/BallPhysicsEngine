//! Instruments in the inspector: what a sound module plays through.

use crate::theme;
use bpe_core::{
    Choice, Instrument, InstrumentId, InstrumentRef, InstrumentSource, ModuleId, NoteLength, Scene, SettingValue,
    SlotId, SlotLink, ThingId, VoiceSettings,
};
use bpe_mix::InstrumentChoice;
use eframe::egui::{self, RichText, Sense, Ui};

/// The instrument Scene defaults' sound plays through, if it points at one.
pub fn scene_instrument(scene: &Scene) -> Option<InstrumentId> {
    let root = scene.things.get(&ThingId::ROOT)?;
    let SlotLink::Linked { module, overrides } = root.slots.get(&SlotId::Sound)? else { return None };
    let m = scene.library.modules.get(module)?;
    let v = overrides.get("instrument").or_else(|| m.params.get("instrument"))?;
    InstrumentRef::from_value(v)?.0
}

/// What a module pointing at `id` plays: that instrument with the module's own macros, else
/// Scene defaults' one, else the built-in pluck.
pub fn choice(scene: &Scene, id: Option<InstrumentId>, module: Option<ModuleId>) -> InstrumentChoice {
    let lib = &scene.library;
    let found = id
        .and_then(|id| lib.instruments.get(&id))
        .or_else(|| scene_instrument(scene).and_then(|id| lib.instruments.get(&id)));
    let empty = Default::default();
    let macros = module.and_then(|m| lib.modules.get(&m)).map_or(&empty, |m| &m.macros);
    found.map_or_else(InstrumentChoice::pluck, |i| InstrumentChoice::from_library(i, macros))
}

/// A change the instrument section asks for, applied once the drawing is done.
#[derive(Debug, Clone, PartialEq)]
pub enum InstrumentEdit {
    /// The library instrument changed: its knobs, voices or patch.
    Set(InstrumentId, Instrument),
    /// The module's own value for a macro, or `None` to follow the instrument again.
    Macro(String, Option<f32>),
    /// "Edit patch" was pressed: the node panel should open on this patch.
    EditPatch(InstrumentId),
}

/// Under a sound card's Instrument row: the instrument's name, its knobs (a built-in's own, or
/// a patch's macros, which the module can change for itself), and how its notes become voices.
pub fn show(ui: &mut Ui, scene: &Scene, module: ModuleId, id: InstrumentId) -> Vec<InstrumentEdit> {
    let mut edits = Vec::new();
    let Some(instrument) = scene.library.instruments.get(&id) else { return edits };
    let mut i = instrument.clone();
    let own = scene.library.modules.get(&module).map(|m| &m.macros);
    ui.push_id(("instrument", module, id), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&i.name).strong());
            if matches!(i.source, InstrumentSource::Patch { .. })
                && ui.button("Edit patch").on_hover_text("Open this patch in the node panel").clicked()
            {
                edits.push(InstrumentEdit::EditPatch(id));
            }
        });
        let mut changed = false;
        egui::Grid::new("knobs").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            match &mut i.source {
                InstrumentSource::BuiltIn { which, knobs } => {
                    if bpe_mix::Instrument::from_name(which) == Some(bpe_mix::Instrument::Pluck) {
                        for (key, label, min, max, default) in
                            [("decay", "Decay", 0.05, 5.0, 0.9), ("brightness", "Brightness", 0.0, 1.0, 0.5)]
                        {
                            let mut v = knobs.get(key).copied().unwrap_or(default);
                            let name = ui.label(label);
                            if slider(ui, &mut v, min, max, if key == "decay" { " s" } else { "" })
                                .labelled_by(name.id)
                                .changed()
                            {
                                knobs.insert(key.into(), v);
                                changed = true;
                            }
                            ui.end_row();
                        }
                    }
                }
                InstrumentSource::Patch { graph } => {
                    for knob in bpe_synth::macros(graph) {
                        let key = knob.key();
                        let overridden = own.and_then(|m| m.get(&key)).copied();
                        let mut v = overridden.unwrap_or(knob.value);
                        let name = ui
                            .horizontal(|ui| {
                                if overridden.is_some() {
                                    let (dot, response) =
                                        ui.allocate_exact_size(egui::Vec2::splat(8.0), Sense::hover());
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::Label, true, "Changed here")
                                    });
                                    ui.painter().circle_filled(dot.center(), 3.0, theme::OVERRIDE);
                                    response
                                        .on_hover_text("Changed here: this module's own value, not the instrument's.");
                                }
                                ui.add(egui::Label::new(&knob.label).sense(Sense::click()))
                            })
                            .inner;
                        if let Some(value) = overridden {
                            name.context_menu(|ui| {
                                if ui
                                    .button("Reset to instrument")
                                    .on_hover_text("Follow the instrument again")
                                    .clicked()
                                {
                                    edits.push(InstrumentEdit::Macro(key.clone(), None));
                                    ui.close();
                                }
                                if ui
                                    .button("Push to instrument")
                                    .on_hover_text("Give every sound playing this instrument this value")
                                    .clicked()
                                {
                                    let mut g = graph.clone();
                                    if let Some(n) = g.node_mut(knob.node) {
                                        n.params.insert(knob.param.clone(), bpe_graph::Param::Number(value));
                                    }
                                    edits.push(InstrumentEdit::Set(
                                        id,
                                        Instrument {
                                            source: InstrumentSource::Patch { graph: g },
                                            ..instrument.clone()
                                        },
                                    ));
                                    edits.push(InstrumentEdit::Macro(key.clone(), None));
                                    ui.close();
                                }
                            });
                        }
                        if slider(ui, &mut v, knob.min, knob.max, "").labelled_by(name.id).changed() {
                            edits.push(InstrumentEdit::Macro(key, Some(v)));
                        }
                        ui.end_row();
                    }
                }
            }
            changed |= voices(ui, &mut i.voices, matches!(i.source, InstrumentSource::Patch { .. }));
        });
        if changed {
            edits.push(InstrumentEdit::Set(id, i));
        }
    });
    edits
}

/// The voice settings rows; only patches play through voices, so built-ins leave them out.
fn voices(ui: &mut Ui, v: &mut VoiceSettings, patch: bool) -> bool {
    if !patch {
        return false;
    }
    let mut changed = false;
    let name = ui.label("Note length");
    ui.horizontal(|ui| {
        let (mut amount, beats) = match v.note_length {
            NoteLength::Beats(b) => (b, true),
            NoteLength::Seconds(s) => (s, false),
        };
        changed |= slider(ui, &mut amount, 0.05, 16.0, "").labelled_by(name.id).changed();
        // One small button names the unit and switches it.
        let unit = if beats { "beats" } else { "secs" };
        let hover =
            if beats { "Held for beats of the tempo; click for seconds" } else { "Held for seconds; click for beats" };
        let beats = if ui.small_button(unit).on_hover_text(hover).clicked() {
            changed = true;
            !beats
        } else {
            beats
        };
        v.note_length = if beats { NoteLength::Beats(amount) } else { NoteLength::Seconds(amount) };
    });
    ui.end_row();
    let name = ui.label("Voice limit");
    let mut limit = v.limit as f32;
    if slider(ui, &mut limit, 1.0, 64.0, "").labelled_by(name.id).changed() {
        v.limit = limit.round().clamp(1.0, 64.0) as u16;
        changed = true;
    }
    ui.end_row();
    ui.label("Mono");
    changed |= ui.checkbox(&mut v.mono, "").on_hover_text("One voice; a new note glides from the last").changed();
    ui.end_row();
    if v.mono {
        let name = ui.label("Glide");
        changed |= slider(ui, &mut v.glide, 0.0, 2.0, " s").labelled_by(name.id).changed();
        ui.end_row();
        ui.label("Retrigger");
        changed |= ui
            .checkbox(&mut v.retrigger, "")
            .on_hover_text("A new note while one is held starts the envelopes again")
            .changed();
        ui.end_row();
    }
    changed
}

/// A narrow slider with its number, so the section fits the panel.
fn slider(ui: &mut Ui, v: &mut f32, min: f32, max: f32, suffix: &str) -> egui::Response {
    let feel = crate::widgets::number_feel((max - min) as f64, false);
    ui.spacing_mut().slider_width = 100.0;
    ui.add(egui::Slider::new(v, min..=max).step_by(feel.step).max_decimals(feel.decimals).suffix(suffix))
}
