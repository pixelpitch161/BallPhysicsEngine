//! Instruments in the inspector: what a sound module plays through.

use crate::theme;
use crate::widgets::{extra_grid, label_cell, widget_cell};
use bpe_core::{Choice, Instrument, InstrumentId, InstrumentSource, ModuleId, NoteLength, Scene, VoiceSettings};
use bpe_mix::InstrumentChoice;
use eframe::egui::{self, RichText, Sense, Ui};

/// What a module pointing at `id` plays: that instrument with the module's own macros, else
/// the built-in pluck.
pub fn choice(scene: &Scene, id: Option<InstrumentId>, module: Option<ModuleId>) -> InstrumentChoice {
    let lib = &scene.library;
    let found = id.and_then(|id| lib.instruments.get(&id));
    let empty = Default::default();
    let macros = module.and_then(|m| lib.modules.get(&m)).map_or(&empty, |m| &m.macros);
    found.map_or_else(InstrumentChoice::pluck, |i| InstrumentChoice::from_library(i, macros))
}

/// A change the instrument section asks for, applied once the drawing is done.
#[derive(Debug, Clone, PartialEq)]
pub enum InstrumentEdit {
    /// The library instrument changed: its knobs, voices or patch.
    Set(InstrumentId, Box<Instrument>),
    /// The module's own value for a macro, or `None` to follow the instrument again.
    Macro(String, Option<f32>),
    /// "Edit patch" was pressed: the node panel should open on this patch.
    EditPatch(InstrumentId),
    /// The instrument's own effects changed.
    Fx(InstrumentId, bpe_graph::Graph),
    /// "Open nodes" under its effects: the node panel should open on them.
    OpenFx(InstrumentId),
}

/// Under a sound card's Instrument row: the instrument's name, its knobs (a built-in's own, or
/// a patch's macros, which the module can change for itself), and how its notes become voices.
/// The knobs' labels are `label_width` wide, as a settings grid's are, so drawn where a
/// section's own widgets go their sliders line up with the settings' sliders.
pub fn show(ui: &mut Ui, scene: &Scene, module: ModuleId, id: InstrumentId, label_width: f32) -> Vec<InstrumentEdit> {
    let mut edits = Vec::new();
    let right = ui.available_rect_before_wrap().right();
    let lw = label_width;
    let Some(instrument) = scene.library.instruments.get(&id) else { return edits };
    let mut i = instrument.clone();
    let own = scene.library.modules.get(&module).map(|m| &m.macros);
    let channels = crate::follow_menu::channel_names(scene, Some(module));
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
        extra_grid("knobs").show(ui, |ui| {
            match &mut i.source {
                InstrumentSource::BuiltIn { which, knobs } => {
                    if bpe_mix::Instrument::from_name(which) == Some(bpe_mix::Instrument::Pluck) {
                        for (key, label, min, max, default) in
                            [("decay", "Decay", 0.05, 5.0, 0.9), ("brightness", "Brightness", 0.0, 1.0, 0.5)]
                        {
                            let mut v = knobs.get(key).copied().unwrap_or(default);
                            let name = label_cell(ui, lw, |ui| ui.label(label));
                            let suffix = if key == "decay" { " s" } else { "" };
                            let slider = widget_cell(ui, right, |ui| slider(ui, &mut v, min, max, suffix));
                            if slider.labelled_by(name.id).changed() {
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
                        let kind = graph.node(knob.node).map_or("", |n| n.kind.as_str());
                        let can_follow = bpe_synth::followable(kind).contains(&knob.param.as_str());
                        let follows = bpe_synth::followed(graph, knob.node, &knob.param);
                        let name = label_cell(ui, lw, |ui| {
                            if overridden.is_some() {
                                let (dot, response) = ui.allocate_exact_size(egui::Vec2::splat(8.0), Sense::hover());
                                response
                                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Changed here"));
                                ui.painter().circle_filled(dot.center(), 3.0, theme::OVERRIDE);
                                response.on_hover_text("Changed here: this module's own value, not the instrument's.");
                            }
                            let label = ui.add(egui::Label::new(&knob.label).sense(Sense::click()));
                            crate::follow_menu::mark(ui, follows.as_deref());
                            label
                        });
                        let set_graph = |g: bpe_graph::Graph| {
                            InstrumentEdit::Set(
                                id,
                                Box::new(Instrument {
                                    source: InstrumentSource::Patch { graph: g },
                                    ..instrument.clone()
                                }),
                            )
                        };
                        if can_follow || overridden.is_some() {
                            name.context_menu(|ui| {
                                if let Some(value) = overridden {
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
                                        edits.push(set_graph(g));
                                        edits.push(InstrumentEdit::Macro(key.clone(), None));
                                        ui.close();
                                    }
                                }
                                if can_follow {
                                    if let Some(with) = crate::follow_menu::menu(ui, true, &channels)
                                        && let Some(g) = bpe_synth::follow(graph, knob.node, &knob.param, &with)
                                    {
                                        edits.push(set_graph(g));
                                    }
                                    if follows.is_some() && ui.button("Stop following").clicked() {
                                        edits.push(set_graph(bpe_synth::unfollow(graph, knob.node, &knob.param)));
                                        ui.close();
                                    }
                                }
                            });
                        }
                        let slider = widget_cell(ui, right, |ui| slider(ui, &mut v, knob.min, knob.max, ""));
                        if slider.labelled_by(name.id).changed() {
                            edits.push(InstrumentEdit::Macro(key, Some(v)));
                        }
                        ui.end_row();
                    }
                }
            }
            let patch = matches!(i.source, InstrumentSource::Patch { .. });
            changed |= voices(ui, &mut i.voices, patch, lw, right);
        });
        if changed {
            edits.push(InstrumentEdit::Set(id, Box::new(i)));
        }
        let fx = &instrument.fx;
        let count = bpe_synth::straight_chain(fx).map_or("custom".to_owned(), |c| c.len().to_string());
        let title = RichText::new(format!("Instrument effects ({count})")).strong().color(theme::MUTED);
        egui::CollapsingHeader::new(title).id_salt("instrument effects").show(ui, |ui| {
            match crate::fx_list::show(ui, fx, ("instrument fx", id), &channels) {
                crate::fx_list::FxEdit::Set(g) => edits.push(InstrumentEdit::Fx(id, g)),
                crate::fx_list::FxEdit::OpenNodes => edits.push(InstrumentEdit::OpenFx(id)),
                crate::fx_list::FxEdit::None => {}
            }
        });
    });
    edits
}

/// The voice settings rows, labels `lw` wide and widgets up to `right`; only patches play
/// through voices, so built-ins leave them out.
fn voices(ui: &mut Ui, v: &mut VoiceSettings, patch: bool, lw: f32, right: f32) -> bool {
    if !patch {
        return false;
    }
    let mut changed = false;
    let name = label_cell(ui, lw, |ui| ui.label("Note length"));
    widget_cell(ui, right, |ui| {
        let (mut amount, beats) = match v.note_length {
            NoteLength::Beats(b) => (b, true),
            NoteLength::Seconds(s) => (s, false),
        };
        // A plain slider, so the unit button fits beside it.
        let feel = crate::widgets::number_feel(16.0, false);
        ui.spacing_mut().slider_width = 100.0;
        let slider = egui::Slider::new(&mut amount, 0.05..=16.0).step_by(feel.step).max_decimals(feel.decimals);
        changed |= ui.add(slider).labelled_by(name.id).changed();
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
    let name = label_cell(ui, lw, |ui| ui.label("Voice limit"));
    let mut limit = v.limit.clamp(1, 64);
    let feel = crate::widgets::number_feel(63.0, true);
    let limit_slider =
        widget_cell(ui, right, |ui| crate::widgets::number_slider(ui, &mut limit, (1.0, 64.0), feel, String::new()));
    if limit_slider.labelled_by(name.id).changed() {
        v.limit = limit;
        changed = true;
    }
    ui.end_row();
    label_cell(ui, lw, |ui| ui.label("Mono"));
    changed |= widget_cell(ui, right, |ui| {
        ui.checkbox(&mut v.mono, "").on_hover_text("One voice; a new note glides from the last").changed()
    });
    ui.end_row();
    if v.mono {
        let name = label_cell(ui, lw, |ui| ui.label("Glide"));
        changed |= widget_cell(ui, right, |ui| slider(ui, &mut v.glide, 0.0, 2.0, " s")).labelled_by(name.id).changed();
        ui.end_row();
        label_cell(ui, lw, |ui| ui.label("Retrigger"));
        changed |= widget_cell(ui, right, |ui| {
            ui.checkbox(&mut v.retrigger, "")
                .on_hover_text("A new note while one is held starts the envelopes again")
                .changed()
        });
        ui.end_row();
    }
    changed
}

/// A slider with its number, as wide as the row allows, as a setting's slider is.
fn slider(ui: &mut Ui, v: &mut f32, min: f32, max: f32, suffix: &str) -> egui::Response {
    let feel = crate::widgets::number_feel((max - min) as f64, false);
    crate::widgets::number_slider(ui, v, (min as f64, max as f64), feel, suffix.to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        scene: Scene,
        module: ModuleId,
        id: InstrumentId,
        edits: Vec<InstrumentEdit>,
    }

    /// The new scene, its sound playing a patch with the filter's cutoff exposed.
    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let id = *scene.library.instruments.keys().next().expect("a new scene has the shared Pluck");
        let mut graph = bpe_synth::blank_patch();
        graph.nodes.iter_mut().find(|n| n.kind == "filter").unwrap().exposed.insert("cutoff".into());
        scene.library.instruments.get_mut(&id).unwrap().source = InstrumentSource::Patch { graph };
        let module =
            scene.library.modules.iter().find(|(_, m)| m.slot == bpe_core::SlotId::Sound).map(|(id, _)| *id).unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 900.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    for e in show(ui, &s.scene, s.module, s.id, crate::widgets::label_width(420.0)) {
                        if let InstrumentEdit::Set(i, instrument) = &e {
                            s.scene.library.instruments.insert(*i, (**instrument).clone());
                        }
                        s.edits.push(e);
                    }
                },
                State { scene, module, id, edits: Vec::new() },
            );
        h.run();
        h
    }

    fn patch(s: &State) -> &bpe_graph::Graph {
        match &s.scene.library.instruments[&s.id].source {
            InstrumentSource::Patch { graph } => graph,
            _ => panic!("a patch"),
        }
    }

    fn click_through(h: &mut Harness<'_, State>, path: &[&str]) {
        h.get_by_role_and_label(egui::accesskit::Role::Label, "Filter Cutoff").click_secondary();
        h.run();
        for step in path {
            h.get_by_label(step).hover();
            h.run();
            h.get_by_label(step).click();
            h.run();
        }
    }

    #[test]
    fn follow_a_macro() {
        let mut h = harness();
        click_through(&mut h, &["Follow…", "Live", "Height in the frame"]);
        assert!(h.state().edits.iter().any(|e| matches!(e, InstrumentEdit::Set(..))));
        assert!(patch(h.state()).nodes.iter().any(|n| n.kind == "live_input"));
        h.get_by_label("Follows Height in the frame");
        click_through(&mut h, &["Stop following"]);
        assert!(!patch(h.state()).nodes.iter().any(|n| n.kind == "live_input"));
    }
}
