//! The Rules workspace: the scene's rule graphs on a node canvas.

use crate::node_editor::{LiveView, NodeEditor};
use crate::theme;
use bpe_graph::{Catalog, Graph};
use bpe_scene::Scene;
use bpe_sim::rules::RuleState;
use eframe::egui::{self, RichText, Ui};

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
    bpe_sim::rules::catalog()
}

/// Lights up the blocks of one rule set as they fire in the preview.
pub struct Live<'a> {
    pub state: &'a RuleState,
    pub rule: u16,
    /// The preview's clock.
    pub now: f64,
}

impl crate::node_editor::LiveView for Live<'_> {
    fn flash(&self, node: u32) -> f32 {
        match self.state.fired(self.rule, node) {
            Some((count, last)) if count > 0 && last <= self.now + 1e-6 => (-(self.now - last) as f32 * 4.0).exp(),
            _ => 0.0,
        }
    }

    fn badge(&self, node: u32) -> Option<String> {
        let (count, _) = self.state.fired(self.rule, node)?;
        (count > 0).then(|| if count >= 10_000 { format!("{}k", count / 1000) } else { count.to_string() })
    }
}

pub fn show(ui: &mut Ui, scene: &mut Scene, state: &mut State, rules: Option<(&RuleState, f64)>) {
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
                scene.removed_item(bpe_scene::List::Rules, state.graph);
                state.graph = state.graph.saturating_sub(1);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Add block").on_hover_text("Space").clicked() {
                state.editor.open_palette();
            }
            ui.menu_button("Recipes", |ui| {
                for recipe in bpe_sim::rules::recipes() {
                    if ui.button(recipe.name).on_hover_text(recipe.description).clicked() {
                        scene.logic.push(recipe.graph);
                        state.graph = scene.logic.len() - 1;
                        state.editor.refit();
                        ui.close();
                    }
                }
            });
        });
    });
    ui.add_space(4.0);
    let live = rules.map(|(rules, now)| Live { state: rules, rule: state.graph as u16, now });
    let live = live.as_ref().map(|l| l as &dyn LiveView);
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
    crate::keys::set_canvas(ui.ctx(), Some(crate::keys::Canvas::Rule(state.graph as u32)));
    state.editor.show(ui, graph, catalog(), live);
    crate::keys::set_canvas(ui.ctx(), None);
}
