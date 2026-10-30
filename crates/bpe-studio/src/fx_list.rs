//! The effects list: an effects graph shown as a column of blocks, each with its knobs, a
//! bypass switch and buttons to move or remove it, for the inspector. A graph that isn't a
//! straight chain is edited in the node panel instead.

use crate::theme;
use bpe_core::{BusId, InstrumentId, ModuleId, Scene};
use bpe_graph::{Catalog, Graph, Param, ParamSpec};
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeMap;
use std::hash::Hash;

/// Where an effects graph lives in the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxPlace {
    /// An instrument's own effects.
    Instrument(InstrumentId),
    /// A sound module's channel inserts.
    Channel(ModuleId),
    Bus(BusId),
    Master,
}

impl FxPlace {
    /// What the place is called, for headers: the instrument, module or bus name, or "Master".
    pub fn label(self, scene: &Scene) -> String {
        match self {
            FxPlace::Instrument(id) => {
                scene.library.instruments.get(&id).map_or("Instrument".into(), |i| i.name.clone())
            }
            FxPlace::Channel(id) => scene.library.modules.get(&id).map_or("Channel".into(), |m| m.name.clone()),
            FxPlace::Bus(id) => {
                scene.library.buses.iter().find(|b| b.id == id).map_or("Bus".into(), |b| b.name.clone())
            }
            FxPlace::Master => "Master".into(),
        }
    }

    /// The effects graph there, when the place still exists.
    pub fn graph(self, scene: &Scene) -> Option<&Graph> {
        match self {
            FxPlace::Instrument(id) => scene.library.instruments.get(&id).map(|i| &i.fx),
            FxPlace::Channel(id) => scene.library.modules.get(&id).map(|m| &m.channel.inserts),
            FxPlace::Bus(id) => scene.library.buses.iter().find(|b| b.id == id).map(|b| &b.fx),
            FxPlace::Master => Some(&scene.library.master_fx),
        }
    }

    /// Puts `graph` there. False when the place is gone.
    pub fn set(self, scene: &mut Scene, graph: Graph) -> bool {
        let slot = match self {
            FxPlace::Instrument(id) => scene.library.instruments.get_mut(&id).map(|i| &mut i.fx),
            FxPlace::Channel(id) => scene.library.modules.get_mut(&id).map(|m| &mut m.channel.inserts),
            FxPlace::Bus(id) => scene.library.buses.iter_mut().find(|b| b.id == id).map(|b| &mut b.fx),
            FxPlace::Master => Some(&mut scene.library.master_fx),
        };
        slot.map(|s| *s = graph).is_some()
    }
}

/// What the list asks for after a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FxEdit {
    #[default]
    None,
    /// The graph after an edit.
    Set(Graph),
    /// Open the graph in the node panel.
    OpenNodes,
}

/// One block of a chain: its kind, knobs and whether it's on.
type ChainBlock = (String, BTreeMap<String, Param>, bool);

/// Shows `graph` as a list of blocks, or as "Custom routing" when it isn't a straight chain.
pub fn show(ui: &mut Ui, graph: &Graph, salt: impl Hash + std::fmt::Debug) -> FxEdit {
    ui.push_id(salt, |ui| list(ui, graph)).inner
}

fn list(ui: &mut Ui, graph: &Graph) -> FxEdit {
    let cat = bpe_synth::catalog();
    let Some(ids) = bpe_synth::straight_chain(graph) else {
        let mut edit = FxEdit::None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Custom routing").color(theme::MUTED))
                .on_hover_text("These effects are wired as a graph; edit them as nodes.");
            if ui.button("Open nodes").clicked() {
                edit = FxEdit::OpenNodes;
            }
        });
        return edit;
    };
    let mut chain: Vec<ChainBlock> =
        ids.iter().filter_map(|id| graph.node(*id)).map(|n| (n.kind.clone(), n.params.clone(), n.enabled)).collect();
    let mut changed = false;
    let mut action: Option<(usize, Move)> = None;
    for (i, (kind, params, enabled)) in chain.iter_mut().enumerate() {
        let Some(spec) = cat.spec(kind) else { continue };
        ui.push_id(i, |ui| {
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(spec.title.as_str()).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            action = Some((i, Move::Remove));
                        }
                        if ui.small_button("Down").on_hover_text("Later in the chain").clicked() {
                            action = Some((i, Move::Down));
                        }
                        if ui.small_button("Up").on_hover_text("Earlier in the chain").clicked() {
                            action = Some((i, Move::Up));
                        }
                        changed |= ui.checkbox(enabled, "On").on_hover_text("Off passes the sound through").changed();
                    });
                });
                egui::Grid::new("knobs").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    for p in &spec.params {
                        let value = params.entry(p.name.clone()).or_insert_with(|| p.default.clone());
                        let label = ui.label(p.label.as_str());
                        if !p.help.is_empty() {
                            label.clone().on_hover_text(p.help.as_str());
                        }
                        changed |= knob(ui, p, value, label.id);
                        ui.end_row();
                    }
                });
            });
        });
    }
    if let Some((i, how)) = action {
        match how {
            Move::Up if i > 0 => chain.swap(i, i - 1),
            Move::Down if i + 1 < chain.len() => chain.swap(i, i + 1),
            Move::Remove => _ = chain.remove(i),
            _ => {}
        }
        changed = true;
    }
    let mut open = false;
    ui.horizontal(|ui| {
        let mut picked: Option<&str> = None;
        ui.menu_button("Add effect", |ui| {
            ui.set_min_width(140.0);
            for kind in bpe_synth::effects_catalog_kinds() {
                let Some(spec) = cat.spec(kind) else { continue };
                if ui.button(spec.title.as_str()).on_hover_text(spec.help.as_str()).clicked() {
                    picked = Some(kind);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Add an effect at the end of the chain");
        if let Some(kind) = picked {
            chain.push((kind.to_owned(), BTreeMap::new(), true));
            changed = true;
        }
        open = ui.small_button("Open nodes").on_hover_text("Edit these effects as a graph").clicked();
    });
    if open {
        FxEdit::OpenNodes
    } else if changed {
        FxEdit::Set(bpe_synth::chain_graph(&chain))
    } else {
        FxEdit::None
    }
}

#[derive(Clone, Copy)]
enum Move {
    Up,
    Down,
    Remove,
}

/// The control for one knob: a slider for a number, a switch for a flag, a menu for a choice.
fn knob(ui: &mut Ui, spec: &ParamSpec, value: &mut Param, label: egui::Id) -> bool {
    match value {
        Param::Number(v) => {
            let (lo, hi) = spec.range;
            let feel = crate::widgets::number_feel((hi - lo).max(1e-3) as f64, false);
            ui.spacing_mut().slider_width = 110.0;
            let slider = egui::Slider::new(v, lo..=hi.max(lo + 1e-3))
                .smart_aim(false)
                .step_by(feel.step)
                .max_decimals(feel.decimals)
                .drag_value_speed(feel.drag_speed)
                .clamping(egui::SliderClamping::Edits);
            ui.add(slider).labelled_by(label).changed()
        }
        Param::Bool(b) => ui.checkbox(b, "").labelled_by(label).changed(),
        Param::Choice(choice) => {
            let before = choice.clone();
            egui::ComboBox::from_id_salt(label)
                .selected_text(choice.as_str())
                .width(90.0)
                .show_ui(ui, |ui| {
                    for option in &spec.choices {
                        ui.selectable_value(choice, option.clone(), option);
                    }
                })
                .response
                .labelled_by(label);
            *choice != before
        }
        _ => {
            ui.label("");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        graph: Graph,
        opened: bool,
    }

    fn harness(graph: Graph) -> Harness<'static, State> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 900.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    egui::ScrollArea::vertical().show(ui, |ui| match show(ui, &s.graph, "test") {
                        FxEdit::Set(g) => s.graph = g,
                        FxEdit::OpenNodes => s.opened = true,
                        FxEdit::None => {}
                    });
                },
                State { graph, opened: false },
            );
        h.run();
        h
    }

    fn kinds(g: &Graph) -> Vec<String> {
        bpe_synth::straight_chain(g).unwrap().iter().map(|id| g.node(*id).unwrap().kind.clone()).collect()
    }

    fn add(h: &mut Harness<'_, State>, title: &str) {
        h.get_by_label("Add effect").click();
        h.run();
        let item = h.query_all_by_label(title).last().unwrap();
        item.scroll_to_me();
        h.run();
        h.query_all_by_label(title).last().unwrap().click();
        h.run();
    }

    #[test]
    fn add_reorder_bypass_remove() {
        let mut h = harness(Graph::default());
        add(&mut h, "Delay");
        add(&mut h, "Reverb");
        assert_eq!(kinds(&h.state().graph), ["delay", "reverb"]);
        h.query_all_by_label("Up").nth(1).unwrap().click();
        h.run();
        assert_eq!(kinds(&h.state().graph), ["reverb", "delay"]);
        h.query_all_by_label("On").nth(1).unwrap().click();
        h.run();
        let g = &h.state().graph;
        assert!(!g.nodes.iter().find(|n| n.kind == "delay").unwrap().enabled);
        h.query_all_by_label("Remove").next().unwrap().click();
        h.run();
        assert_eq!(kinds(&h.state().graph), ["delay"]);
    }

    #[test]
    fn custom_routing_offers_open_nodes() {
        let mut g = bpe_synth::chain_graph(&[("amp".into(), BTreeMap::new(), true)]);
        let input = g.nodes.iter().find(|n| n.kind == "fx_in").unwrap().id;
        let mix = g.add(bpe_synth::catalog().spec("mix").unwrap(), [0.0, 0.0]);
        g.links.push(bpe_graph::Link { from: (input, 0), to: (mix, 0) });
        let mut h = harness(g);
        h.get_by_label("Custom routing");
        h.get_by_label("Open nodes").click();
        h.run();
        assert!(h.state().opened);
    }
}
