//! The add-a-block search: type a few letters, pick with the arrow keys and Enter.

use super::{PinRef, category_color, pin_type};
use crate::theme;
use bpe_graph::{Catalog, Graph, NodeSpec};
use eframe::egui::{self, Id, Key, Pos2, RichText, vec2};

pub struct Palette {
    /// Where the new block goes, in canvas units.
    pub at: Pos2,
    /// The pin a wire was dragged from; only blocks that fit it are listed.
    pub from: Option<PinRef>,
    query: String,
    highlighted: usize,
    frames: u32,
}

pub enum Result {
    Open,
    Close,
    Pick(String),
}

impl Palette {
    pub fn new(at: Pos2, from: Option<PinRef>) -> Self {
        Self { at, from, query: String::new(), highlighted: 0, frames: 0 }
    }

    fn matches<'a>(&self, graph: &Graph, catalog: &'a dyn Catalog) -> Vec<&'a NodeSpec> {
        let ty = self.from.and_then(|from| pin_type(graph, catalog, from));
        let words: Vec<String> = self.query.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut found: Vec<(u8, &NodeSpec)> = catalog
            .all()
            .iter()
            .filter(|spec| match (self.from, ty) {
                (Some(from), Some(ty)) if from.output => spec.inputs.iter().any(|p| catalog.accepts(p.ty, ty)),
                (Some(_), Some(ty)) => spec.outputs.iter().any(|p| catalog.accepts(ty, p.ty)),
                _ => true,
            })
            .filter_map(|spec| {
                let title = spec.title.to_lowercase();
                let haystack =
                    format!("{title} {} {} {}", spec.category.to_lowercase(), spec.kind, spec.help.to_lowercase());
                if !words.iter().all(|w| haystack.contains(w.as_str())) {
                    return None;
                }
                // Titles that start with what was typed come first, then titles that contain it.
                let rank = match words.first() {
                    Some(w) if title.starts_with(w.as_str()) => 0,
                    Some(w) if title.contains(w.as_str()) => 1,
                    Some(_) => 2,
                    None => 0,
                };
                Some((rank, spec))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().map(|(_, spec)| spec).collect()
    }

    pub fn show(&mut self, ctx: &egui::Context, id: Id, pos: Pos2, graph: &Graph, catalog: &dyn Catalog) -> Result {
        self.frames += 1;
        let found = self.matches(graph, catalog);
        self.highlighted = self.highlighted.min(found.len().saturating_sub(1));
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            return Result::Close;
        }
        if down && !found.is_empty() {
            self.highlighted = (self.highlighted + 1) % found.len();
        }
        if up && !found.is_empty() {
            self.highlighted = (self.highlighted + found.len() - 1) % found.len();
        }
        if enter && let Some(spec) = found.get(self.highlighted) {
            return Result::Pick(spec.kind.clone());
        }
        let mut result = Result::Open;
        let area = egui::Area::new(id).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(300.0);
                let hint = if self.from.is_some() { "Search blocks that fit this wire" } else { "Search blocks" };
                let edit =
                    ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(hint).desired_width(f32::INFINITY));
                if self.frames <= 2 {
                    edit.request_focus();
                }
                if edit.changed() {
                    self.highlighted = 0;
                }
                ui.add_space(4.0);
                if found.is_empty() {
                    ui.label(RichText::new("No block matches.").color(theme::MUTED));
                }
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    let mut category = "";
                    for (index, spec) in found.iter().enumerate() {
                        if self.query.is_empty() && spec.category != category {
                            category = &spec.category;
                            ui.add_space(2.0);
                            ui.label(RichText::new(category).small().strong().color(category_color(category)));
                        }
                        let selected = index == self.highlighted;
                        let row = ui.add(
                            egui::Button::selectable(selected, spec.title.as_str())
                                .shortcut_text(RichText::new(&spec.category).small())
                                .min_size(vec2(ui.available_width(), 24.0)),
                        );
                        if selected && (up || down) {
                            row.scroll_to_me(None);
                        }
                        let row = if spec.help.is_empty() { row } else { row.on_hover_text(&spec.help) };
                        if row.clicked() {
                            result = Result::Pick(spec.kind.clone());
                        }
                    }
                });
            });
        });
        if self.frames > 2 && area.response.clicked_elsewhere() {
            return Result::Close;
        }
        result
    }
}
