//! The modulator row: a tile per modulator, ADSR 1 and ADSR 2, then Distance 1 and 2 and LFO 1
//! and 2 (shown Off until modulators come). A tile names its modulator and how many knobs it
//! moves; clicking a live one picks it for the editor under the row.

use super::{SoundTab, TabCx};
use crate::style;
use bpe_core::{LayerId, ROUTES_KEY, SlotId, Value};
use bpe_kit::sound::ADSR;
use eframe::egui::{self, Rect, Sense, Stroke, Ui, Vec2};

/// The modulators that show Off until part 4.
const LATER: [&str; 4] = ["Distance 1", "Distance 2", "LFO 1", "LFO 2"];
/// A tile's height.
const TILE_H: f32 = 44.0;

/// The card's ADSR layers that are on, in order: the live modulators.
pub fn live(cx: &TabCx) -> Vec<(LayerId, String)> {
    let Some(card) = cx.scene.card(cx.id, SlotId::Sound) else { return Vec::new() };
    card.layers.iter().filter(|l| l.kind == ADSR && l.enabled).map(|l| (l.id, l.name.clone())).collect()
}

/// How many knobs `layer` moves.
pub fn targets_of(cx: &TabCx, layer: LayerId) -> usize {
    match cx.get(ROUTES_KEY) {
        Value::Routes(routes) => routes.iter().filter(|r| r.source == layer).count(),
        _ => 0,
    }
}

/// Draws the row and returns the modulator picked for the editor (the first live one when the
/// one picked before has gone).
pub fn show(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab) -> Option<LayerId> {
    let card = cx.scene.card(cx.id, SlotId::Sound)?;
    let live = live(cx);
    let mut picked = tab.picked.get(&cx.id).copied().filter(|p| live.iter().any(|(id, _)| id == p));
    if picked.is_none() {
        picked = live.first().map(|(id, _)| *id);
    }
    // Every ADSR layer of the card, on or not, then the modulators to come.
    let mut tiles: Vec<(Option<LayerId>, String)> =
        card.layers.iter().filter(|l| l.kind == ADSR).map(|l| (l.enabled.then_some(l.id), l.name.clone())).collect();
    tiles.extend(LATER.iter().map(|n| (None, (*n).to_owned())));
    let gap = 4.0;
    let w = ((ui.available_width() - gap * (tiles.len() as f32 - 1.0)) / tiles.len() as f32).max(56.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (layer, name) in &tiles {
            let state = match layer {
                Some(l) => match targets_of(cx, *l) {
                    1 => "1 target".to_owned(),
                    n => format!("{n} targets"),
                },
                None => "Off".to_owned(),
            };
            let selected = layer.is_some() && *layer == picked;
            if tile(ui, w, name, &state, selected, layer.is_some()).clicked()
                && let Some(l) = layer
            {
                picked = Some(*l);
            }
        }
    });
    if let Some(p) = picked {
        tab.picked.insert(cx.id, p);
    }
    picked
}

/// One tile; `live` ones can be clicked.
fn tile(ui: &mut Ui, w: f32, name: &str, state: &str, selected: bool, live: bool) -> egui::Response {
    let sense = if live { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, TILE_H), sense);
    let name_owned = name.to_owned();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, live, selected, &name_owned));
    let response = if live { response } else { response.on_hover_text("Modulators like this come in a later update") };
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected { style::RAISED } else { style::PANEL };
        let line = if selected {
            style::TEXT
        } else if response.hovered() && live {
            style::FAINT
        } else {
            style::LINE
        };
        painter.rect_filled(rect, style::RADIUS, fill);
        painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, line), egui::StrokeKind::Inside);
        let inner = Rect::from_min_max(rect.min + Vec2::new(6.0, 4.0), rect.max - Vec2::new(6.0, 4.0));
        let ink = if live { style::TEXT } else { style::FAINT };
        painter.text(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            name,
            style::semibold(12.0),
            ink,
        );
        painter.text(inner.left_bottom(), egui::Align2::LEFT_BOTTOM, state, egui::FontId::proportional(11.0), style::MUTED);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::super::tests::sounding;
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};

    #[test]
    fn the_row_shows_adsr_1_live_and_the_rest_off() {
        let h = sounding();
        let adsr1 = h.get_by_role_and_label(Role::Button, "ADSR 1");
        assert!(!adsr1.accesskit_node().is_disabled());
        for off in ["ADSR 2", "Distance 1", "LFO 2"] {
            assert!(h.get_by_role_and_label(Role::Button, off).accesskit_node().is_disabled(), "{off} is Off");
        }
    }
}
