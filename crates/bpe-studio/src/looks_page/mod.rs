//! The Looks tab (ready-made looks, spec `docs/superpowers/specs/2026-11-08-ready-made-looks-design.md`): a library
//! page in the inspector for Balls, Emitters and Shapes. On the left the families, in the middle a tile for each look,
//! on the right the hovered or picked look worn by this thing, with Use this look and Add its layers on top. A look is
//! pasted onto the thing as ordinary layers (`bpe_kit::ready_looks::apply`), which it owns from then on.

pub(crate) mod grid;
mod preview;
pub(crate) mod shape_view;
mod thumbs;

use std::collections::BTreeMap;

use bpe_core::{Scene, ThingId};
use bpe_kit::Kit;
use bpe_kit::ready_looks::{self, Colours, How, ReadyLook, Wears};
use eframe::egui::{self, RichText, Ui};

use crate::inspector::Inspector;
use crate::look_editor::BallView;
use crate::style;

/// The Looks tab's state, one for the editor.
#[derive(Default)]
pub struct LooksPage {
    /// The family shown; `None` is All.
    pub family: Option<&'static str>,
    /// What is typed in Find a look.
    pub find: String,
    /// The look picked for each kind of wearer.
    pub picked: BTreeMap<Wears, &'static str>,
    /// Whose colours the buttons put on.
    pub colours: Colours,
    /// The preview of a ball look.
    pub view: BallView,
    /// The preview of a shape look.
    pub shape: shape_view::ShapeView,
    thumbs: thumbs::Thumbs,
    /// What the last button did, for the line under them: (thing, words).
    pub done: Option<(ThingId, String)>,
}

/// Below this width the preview goes under the grid.
const NARROW: f32 = 600.0;
const FAMILIES_WIDTH: f32 = 112.0;
const PREVIEW_WIDTH: f32 = 250.0;

/// The look picked for `w`, else its first.
pub fn picked(page: &LooksPage, w: Wears) -> &'static ReadyLook {
    page.picked
        .get(&w)
        .and_then(|id| ready_looks::find(id))
        .filter(|l| l.wears == w)
        .unwrap_or_else(|| ready_looks::all().iter().find(|l| l.wears == w).expect("looks of every kind"))
}

/// Whether Keep mine can apply to `look` on thing `id`.
pub fn keeps_mine(scene: &Scene, id: ThingId, look: &ReadyLook) -> bool {
    look.takes_colour && ready_looks::own_colours(scene, id).is_some()
}

/// The colours `look` would go on with from the page's choice.
fn colours_for(page: &LooksPage, scene: &Scene, id: ThingId, look: &ReadyLook) -> Colours {
    if page.colours == Colours::Mine && keeps_mine(scene, id, look) { Colours::Mine } else { Colours::Look }
}

/// Draws the Looks tab for thing `id` (the selection's primary; `selected` all of it) and puts a look on when a
/// button is pressed. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    selected: &[ThingId],
    id: ThingId,
    time: f64,
) -> bool {
    let Some(w) = scene.things.get(&id).and_then(|t| ready_looks::wears(&t.kind)) else { return false };
    let reg = &kit.registry;
    let mut hovered: Option<&'static ReadyLook> = None;
    let mut pressed: Option<How> = None;
    let width = ui.available_width();
    {
        let Inspector { looks: page, render, .. } = &mut *inspector;
        let render = render.as_ref();
        // Its own scroll place, apart from the Settings tab's.
        egui::ScrollArea::vertical().id_salt("looks page").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            if width < NARROW {
                ui.horizontal_wrapped(|ui| families(ui, page, w));
                ui.add_space(6.0);
                hovered = grid::show(ui, page, render, scene, kit, id, w);
                ui.add_space(8.0);
                style::divider(ui);
                pressed = preview::show(ui, page, render, scene, kit, selected, id, w, hovered, time);
            } else {
                ui.horizontal_top(|ui| {
                    let gap = ui.spacing().item_spacing.x;
                    ui.vertical(|ui| {
                        ui.set_width(FAMILIES_WIDTH);
                        families(ui, page, w);
                    });
                    ui.vertical(|ui| {
                        ui.set_width((width - FAMILIES_WIDTH - PREVIEW_WIDTH - 2.0 * gap).max(120.0));
                        hovered = grid::show(ui, page, render, scene, kit, id, w);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(PREVIEW_WIDTH);
                        pressed = preview::show(ui, page, render, scene, kit, selected, id, w, hovered, time);
                    });
                });
            }
        });
    }
    let Some(how) = pressed else { return false };
    let page = &mut inspector.looks;
    // The buttons put on the picked look (the pointer has left the tiles to press them).
    let look = picked(page, w);
    let mut reached = 0;
    let mut layers = 0;
    let mut all: Vec<ThingId> = vec![id];
    all.extend(selected.iter().copied().filter(|t| *t != id));
    for t in all {
        // Each thing keeps its own colours when it has them.
        let theirs = colours_for(page, scene, t, look);
        if let Ok(n) = ready_looks::apply(scene, reg, t, look, theirs, how) {
            reached += 1;
            layers = n;
            if how == How::Replace {
                inspector.selected_layer.remove(&t);
            }
        }
    }
    if reached == 0 {
        return false;
    }
    // Each look put on is an undo step of its own, however soon after another edit.
    inspector.own_step = true;
    let name = scene.things.get(&id).map_or(String::new(), |t| t.name.clone());
    let words = match (how, reached) {
        (How::Replace, 1) => format!("{name} wears {}. Ctrl+Z undoes it.", look.name),
        (How::Replace, n) => format!("{n} things wear {}. Ctrl+Z undoes it.", look.name),
        (How::OnTop, 1) => format!("Added {} of {} to {name}. Ctrl+Z undoes it.", layers_words(layers), look.name),
        (How::OnTop, n) => format!("Added {} of {} to {n} things. Ctrl+Z undoes it.", layers_words(layers), look.name),
    };
    inspector.looks.done = Some((id, words));
    true
}

/// "1 layer", "3 layers".
fn layers_words(n: usize) -> String {
    if n == 1 { "1 layer".to_owned() } else { format!("{n} layers") }
}

/// The families, All first, each with how many looks it has; a click shows only that family.
fn families(ui: &mut Ui, page: &mut LooksPage, w: Wears) {
    let count = |fam: Option<&str>| {
        ready_looks::all().iter().filter(|l| l.wears == w && fam.is_none_or(|f| f == l.family)).count()
    };
    // A family of the other kind of wearer (picked on a shape, now a ball) shows All.
    if page.family.is_some_and(|f| !ready_looks::families(w).contains(&f)) {
        page.family = None;
    }
    let choices = std::iter::once(None).chain(ready_looks::families(w).iter().map(|f| Some(*f)));
    for fam in choices {
        let on = page.family == fam;
        let text = format!("{}  {}", fam.unwrap_or("All"), count(fam));
        let button = style::toggle_button(&text, on).frame_when_inactive(on).min_size(egui::vec2(FAMILIES_WIDTH, 24.0));
        let help = match fam {
            None => "Every look".to_owned(),
            Some(f) => format!("The {f} looks"),
        };
        if ui.add(button).on_hover_text(help).clicked() {
            page.family = fam;
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Hover a look to try it. It comes as layers you can change.").small().color(style::MUTED));
}

#[cfg(test)]
mod tests;
