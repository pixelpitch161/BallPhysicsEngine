//! The Rules workspace: the scene's rule graphs on a node canvas.

use crate::node_editor::{LiveView, NodeEditor};
use crate::theme;
use bpe_graph::{Catalog, Graph, SpecList};
use bpe_scene::Scene;
use eframe::egui::{self, RichText, Ui};
use std::sync::LazyLock;

pub struct State {
    pub editor: NodeEditor,
    /// Which of the scene's rule graphs is open.
    pub graph: usize,
}

impl Default for State {
    fn default() -> Self {
        Self { editor: NodeEditor::new("rules"), graph: 0 }
    }
}

/// The blocks rules can use.
pub fn catalog() -> &'static dyn Catalog {
    static CATALOG: LazyLock<SpecList> = LazyLock::new(SpecList::default);
    &*CATALOG
}

pub fn show(ui: &mut Ui, scene: &mut Scene, state: &mut State, live: Option<&dyn LiveView>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Rule set").color(theme::MUTED));
        let names: Vec<String> = scene
            .logic
            .iter()
            .enumerate()
            .map(|(i, g)| if g.name.is_empty() { format!("Rules {}", i + 1) } else { g.name.clone() })
            .collect();
        let current = names.get(state.graph).cloned().unwrap_or_else(|| "None yet".into());
        egui::ComboBox::from_id_salt("rule-graph").selected_text(current).width(180.0).show_ui(ui, |ui| {
            for (i, name) in names.iter().enumerate() {
                if ui.selectable_label(state.graph == i, name).clicked() {
                    state.graph = i;
                    state.editor.refit();
                }
            }
        });
        if ui.button("New rule set").clicked() {
            scene.logic.push(Graph::new(format!("Rules {}", scene.logic.len() + 1)));
            state.graph = scene.logic.len() - 1;
            state.editor.refit();
        }
        if let Some(graph) = scene.logic.get_mut(state.graph) {
            ui.add(egui::TextEdit::singleline(&mut graph.name).desired_width(140.0).hint_text("Name"));
            ui.checkbox(&mut graph.enabled, "On").on_hover_text("Rule sets that are off do nothing");
            if ui.button("Delete").on_hover_text("Remove this rule set (Ctrl+Z brings it back)").clicked() {
                scene.logic.remove(state.graph);
                state.graph = state.graph.saturating_sub(1);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Add block").on_hover_text("Space").clicked() {
                state.editor.open_palette();
            }
        });
    });
    ui.add_space(4.0);
    if scene.logic.is_empty() {
        // Looking at the workspace isn't an edit; the first block added makes the rule set.
        let mut graph = Graph::new("Rules 1");
        if state.editor.show(ui, &mut graph, catalog(), live).changed {
            scene.logic.push(graph);
            state.graph = 0;
        }
        return;
    }
    state.graph = state.graph.min(scene.logic.len() - 1);
    let graph = &mut scene.logic[state.graph];
    state.editor.show(ui, graph, catalog(), live);
}
