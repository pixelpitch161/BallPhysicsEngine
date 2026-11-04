//! An oscillator's layer box: "Osc A layers" with its "+ Add layer" menu, then the oscillator's
//! chain as folded strips, top to bottom in the order the sound goes through them.

use super::{SoundTab, TabCx, TabEdit, name_it, strip};
use crate::style;
use bpe_core::{Layer, SlotId};
use bpe_kit::sound::chain::osc_name;
use bpe_kit::sound::{CHAIN_KINDS, DISTORTION, FILTER, MODULATION};
use eframe::egui::{self, RichText, Ui};

/// The "+ Add layer" menu's items: kind and name.
pub const ADD: [(&str, &str); 3] = [(MODULATION, "Modulation"), (FILTER, "Filter"), (DISTORTION, "Distortion")];

/// The chain layers of `lane` in `layers`, in order.
pub fn lane_layers(layers: &[Layer], lane: u8) -> Vec<&Layer> {
    layers.iter().filter(|l| CHAIN_KINDS.contains(&l.kind.as_str()) && l.lane.unwrap_or(0) == lane).collect()
}

/// Draws oscillator `lane`'s layer box.
pub fn show(ui: &mut Ui, cx: &mut TabCx, lane: u8, tab: &mut SoundTab) {
    let osc = osc_name(lane);
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, style::LINE))
        .corner_radius(style::RADIUS)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{osc} layers")).small().color(style::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut picked = None;
                    let menu = ui.menu_button("+ Add layer", |ui| {
                        ui.set_min_width(130.0);
                        for (kind, name) in ADD {
                            if ui.button(name).clicked() {
                                picked = Some(kind);
                                ui.close();
                            }
                        }
                    });
                    name_it(ui, &menu.response, &format!("Add layer to {osc}"));
                    if let Some(kind) = picked {
                        cx.edits.push(TabEdit::AddLayer { lane, kind });
                    }
                });
            });
            let scene = cx.scene;
            let Some(card) = scene.card(cx.id, SlotId::Sound) else { return };
            let strips = lane_layers(&card.layers, lane);
            if strips.is_empty() {
                ui.label(
                    RichText::new("No layers: the oscillator goes straight to its Level.").small().color(style::FAINT),
                );
                return;
            }
            let loops = bpe_sound::chain_graph::loops_of(card);
            for layer in strips {
                strip::show(ui, cx, tab, layer, &card.layers, &loops);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SHAPE, sounding};
    use super::*;
    use bpe_core::{Address, LayerId, Value, layered_key};
    use eframe::egui::{Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::kittest::Queryable;

    type H = egui_kittest::Harness<'static, (crate::app::Studio, f32)>;

    fn card(h: &H) -> bpe_core::Slot {
        h.state().0.scene.card(SHAPE, SlotId::Sound).cloned().expect("a sound card")
    }

    /// Adds a `kind` layer to `lane` straight in the scene and returns its id.
    fn add(h: &mut H, lane: u8, kind: &str) -> LayerId {
        let reg = bpe_kit::registry();
        let id = h.state_mut().0.scene.add_layer_in(&reg, SHAPE, SlotId::Sound, kind, Some(lane)).unwrap();
        h.run();
        id
    }

    fn value(h: &H, layer: LayerId, key: &str) -> Option<Value> {
        let reg = bpe_kit::registry();
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(layer, key));
        h.state().0.scene.value_at_time(&reg, &a, 0.0)
    }

    fn drag(h: &mut H, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        h.event(egui::Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        for i in 1..=6 {
            h.event(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
            h.step();
        }
        h.event(egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn add_layer_adds_a_strip_to_that_osc() {
        let mut h = sounding();
        assert!(h.query_by_label("Filter strip").is_none());
        h.get_by_label("Add layer to Osc B").click();
        h.run();
        h.get_by_label("Filter").click();
        h.run();
        let c = card(&h);
        let filters: Vec<_> = c.layers.iter().filter(|l| l.kind == FILTER).collect();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].lane, Some(1), "on Osc B");
        assert!(h.query_by_label("Filter strip").is_some());
        assert!(h.query_by_label("Filter type").is_some(), "a new strip opens");
    }

    #[test]
    fn clicking_a_strip_opens_it_and_several_can_be_open() {
        let mut h = sounding();
        add(&mut h, 0, FILTER);
        add(&mut h, 0, DISTORTION);
        assert!(h.query_by_label("Filter type").is_none(), "strips start folded");
        h.get_by_label("Filter strip").click();
        h.run();
        assert!(h.query_by_label("Filter type").is_some());
        h.get_by_label("Distortion strip").click();
        h.run();
        assert!(h.query_by_label("Filter type").is_some(), "the first stays open");
        assert!(h.query_by_label("Distortion type").is_some());
        assert!(h.query_all_by_role_and_label(Role::Slider, "Cutoff").next().is_some());
        h.get_by_label("Filter strip").click();
        h.run();
        assert!(h.query_by_label("Filter type").is_none(), "clicked again, it folds");
    }

    #[test]
    fn dragging_the_filter_handle_sets_cutoff_and_reso() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        h.get_by_label("Filter strip").click();
        h.run();
        let rect = h.get_by_label("Filter view").rect();
        let from = rect.center();
        let to = egui::pos2(rect.left() + rect.width() * 0.2, rect.top() + 8.0);
        drag(&mut h, from, to);
        let cutoff = match value(&h, f, "cutoff") {
            Some(Value::Number(v)) => v,
            other => panic!("no cutoff: {other:?}"),
        };
        let reso = match value(&h, f, "reso") {
            Some(Value::Number(v)) => v,
            other => panic!("no reso: {other:?}"),
        };
        assert!(cutoff < 200.0, "dragged left, cutoff went down: {cutoff}");
        assert!(reso > 80.0, "dragged up, reso went up: {reso}");
    }

    #[test]
    fn solo_chip_sets_solo() {
        let mut h = sounding();
        let f = add(&mut h, 1, FILTER);
        h.get_by_label("Filter strip").click();
        h.run();
        h.get_by_label("Filter solo").click();
        h.run();
        assert_eq!(value(&h, f, "solo"), Some(Value::Bool(true)));
        h.get_by_label("Filter solo").click();
        h.run();
        assert_eq!(value(&h, f, "solo"), Some(Value::Bool(false)));
    }

    #[test]
    fn loop_mark_shows_on_a_looping_source() {
        let mut h = sounding();
        let m = add(&mut h, 0, MODULATION);
        let f = add(&mut h, 0, FILTER);
        assert!(h.query_by_label(strip::LOOP_TEXT).is_none(), "the other osc raw doesn't loop");
        // The FM taps the filter under it in its own lane: it feeds back.
        let reg = bpe_kit::registry();
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(m, "source"));
        h.state_mut().0.scene.set_at(&reg, &a, Value::Text(f.0.to_string())).unwrap();
        h.run();
        assert!(h.query_by_label(strip::LOOP_TEXT).is_some());
    }

    #[test]
    fn the_on_switch_turns_a_layer_off() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        h.get_by_role_and_label(Role::CheckBox, "Filter on").click();
        h.run();
        assert!(!card(&h).layer(f).unwrap().enabled);
    }

    #[test]
    fn the_source_menu_picks_a_layer() {
        let mut h = sounding();
        let m = add(&mut h, 0, MODULATION);
        let f = add(&mut h, 1, FILTER);
        h.get_by_label("Modulation strip").click();
        h.run();
        h.get_by_label("Modulation source").click();
        h.run();
        h.get_by_label("Osc B · after Filter").click();
        h.run();
        assert_eq!(value(&h, m, "source"), Some(Value::Text(f.0.to_string())));
    }
}
