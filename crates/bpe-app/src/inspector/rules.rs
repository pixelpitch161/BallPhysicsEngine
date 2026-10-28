//! The Rules tab: rules read as sentences ("when a ball escapes, spawn 3 balls"), recipes to
//! start from, and a button to open a rule as a graph.

use crate::node_editor::{category_color, param_control};
use crate::theme;
use bpe_graph::{Catalog, Graph, Param, PinType};
use bpe_scene::{List, Property, Scene};
use bpe_sim::rules::{self, WHEN};
use eframe::egui::{self, RichText, Ui};

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// A rule set to open in the Rules workspace, set by "Open in graph" for the app to act on.
    pub open: Option<usize>,
}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    ui.label(
        RichText::new(
            "Rules make things happen when something else does, like \"when a ball escapes, spawn 3 balls and \
             flash red\". Start from a recipe, change the numbers here, or open a rule as blocks to build your own.",
        )
        .color(theme::MUTED),
    );
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("+ Add a recipe", |ui| {
            for recipe in rules::recipes() {
                let button = egui::Button::new(recipe.name).shortcut_text(RichText::new(recipe.description).small());
                if ui.add(button).clicked() {
                    scene.logic.push(recipe.graph);
                    ui.close();
                }
            }
        });
        if ui.button("+ Empty rule set").on_hover_text("Opens a blank canvas of blocks").clicked() {
            scene.logic.push(Graph::new(format!("Rules {}", scene.logic.len() + 1)));
            state.open = Some(scene.logic.len() - 1);
        }
    });
    ui.add_space(8.0);
    let catalog = rules::catalog();
    let mut remove = None;
    for (index, graph) in scene.logic.iter_mut().enumerate() {
        // Block ids start again in every rule set, so each card gets its own id space.
        ui.push_id(index, |ui| card(ui, graph, index, catalog, state, &mut remove));
        ui.add_space(6.0);
    }
    if let Some(index) = remove {
        scene.logic.remove(index);
        scene.removed_item(List::Rules, index);
    }
}

fn card(
    ui: &mut Ui,
    graph: &mut Graph,
    index: usize,
    catalog: &dyn Catalog,
    state: &mut State,
    remove: &mut Option<usize>,
) {
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(8)
        .inner_margin(egui::Margin::same(10))
        .stroke(egui::Stroke::new(1.0, crate::timeline::rule_color(index as u16).gamma_multiply(0.6)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.checkbox(&mut graph.enabled, "").on_hover_text("Turn this rule set on or off");
                ui.add(egui::TextEdit::singleline(&mut graph.name).desired_width(150.0).hint_text("Name"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Remove").on_hover_text("Remove this rule set (Ctrl+Z brings it back)").clicked() {
                        *remove = Some(index);
                    }
                    if ui.button("Open in graph").on_hover_text("Edit it as blocks in the Rules workspace").clicked() {
                        state.open = Some(index);
                    }
                });
            });
            sentences(ui, graph, index as u32, catalog);
            let problems = bpe_graph::validate(graph, catalog);
            for problem in problems.iter().take(3) {
                ui.label(RichText::new(&problem.message).small().color(theme::ERROR));
            }
        });
}

/// Each "When" block and what follows it, as a line of words with the settings marked for
/// this tab right in it.
fn sentences(ui: &mut Ui, graph: &mut Graph, rule: u32, catalog: &dyn Catalog) {
    let triggers: Vec<u32> = graph
        .nodes
        .iter()
        .filter(|n| catalog.spec(&n.kind).is_some_and(|s| s.category == WHEN))
        .map(|n| n.id)
        .collect();
    if triggers.is_empty() {
        ui.label(RichText::new("No \"When\" block yet. Open it as a graph to add one.").color(theme::MUTED));
    }
    for trigger in triggers {
        ui.push_id(trigger, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                block(ui, graph, rule, catalog, trigger);
                follow(ui, graph, rule, catalog, trigger, 0);
            });
        });
    }
}

/// What a block's trigger outputs start, written after it.
fn follow(ui: &mut Ui, graph: &mut Graph, rule: u32, catalog: &dyn Catalog, node: u32, depth: u32) {
    if depth > 12 {
        ui.label("...");
        return;
    }
    let Some(spec) = graph.node(node).and_then(|n| catalog.spec(&n.kind)) else { return };
    let outputs: Vec<(u8, String)> = spec
        .outputs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.ty == PinType::Trigger)
        .map(|(i, p)| (i as u8, p.name.clone()))
        .collect();
    for (pin, name) in outputs {
        let next: Vec<u32> = graph.links_from((node, pin)).map(|(_, l)| l.to.0).collect();
        if next.is_empty() {
            continue;
        }
        if pin > 0 {
            ui.label(RichText::new(format!("{name}:")).color(theme::MUTED));
        }
        for (k, target) in next.into_iter().enumerate() {
            ui.label(RichText::new(if k == 0 { "then" } else { "and" }).color(theme::MUTED));
            // A block reached along two paths is drawn twice, so each drawing gets its own id.
            ui.push_id((depth, pin, k), |ui| {
                block(ui, graph, rule, catalog, target);
                follow(ui, graph, rule, catalog, target, depth + 1);
            });
        }
    }
}

/// A block's title, coloured by its kind, followed by the settings shown on this tab.
fn block(ui: &mut Ui, graph: &mut Graph, rule: u32, catalog: &dyn Catalog, id: u32) {
    let Some(node) = graph.node_mut(id) else { return };
    let Some(spec) = catalog.spec(&node.kind) else {
        ui.label(RichText::new(&node.kind).color(theme::ERROR));
        return;
    };
    let color = if node.enabled { category_color(&spec.category) } else { theme::MUTED };
    let title = ui.label(RichText::new(spec.title.to_lowercase()).color(color).strong());
    if !spec.help.is_empty() {
        title.on_hover_text(&spec.help);
    }
    for param in &spec.params {
        if !node.exposed.contains(&param.name) {
            continue;
        }
        let label = ui.label(RichText::new(param.label.to_lowercase()).small().color(theme::MUTED));
        param_control(ui, node, param, label.id);
        if matches!(param.default, Param::Number(_) | Param::Color(_)) {
            crate::keys::diamond(ui, Property::Rule(rule, id, param.name.clone()));
        }
    }
}
