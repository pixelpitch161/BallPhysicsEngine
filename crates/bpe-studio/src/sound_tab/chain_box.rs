//! An oscillator's layer box: "Osc A layers" with its "+ Add layer" menu, then the oscillator's
//! chain as folded strips, top to bottom in the order the sound goes through them.

use super::{SoundTab, TabCx, TabEdit, name_it, strip};
use crate::style;
use bpe_core::{CopiedLayer, Layer, LayerId, Registry, Scene, SlotId, ThingId, Value};
use bpe_kit::sound::chain::osc_name;
use bpe_kit::sound::{CHAIN_KINDS, DISTORTION, FILTER, MODULATION, Source};
use eframe::egui::{self, Rect, RichText, Ui};

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
                                picked = Some(TabEdit::AddLayer { lane, kind });
                                ui.close();
                            }
                        }
                        ui.separator();
                        let paste = ui
                            .add_enabled(tab.can_paste_layer, egui::Button::new("Paste layer"))
                            .on_disabled_hover_text("Copy a layer first");
                        if paste.clicked() {
                            picked = Some(TabEdit::PasteLayer { lane, after: None });
                            ui.close();
                        }
                    });
                    name_it(ui, &menu.response, &format!("Add layer to {osc}"));
                    cx.edits.extend(picked);
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
            let mut rows = Vec::with_capacity(strips.len());
            let mut dragged = None;
            for layer in &strips {
                let row = strip::show(ui, cx, tab, layer, &card.layers, &loops);
                if row.dragged() || row.drag_stopped() {
                    dragged = Some((layer.id, row.drag_stopped()));
                }
                rows.push((layer.id, row.rect));
            }
            if let Some((id, dropped)) = dragged {
                reorder(ui, cx, card, lane, &rows, id, dropped);
            }
        });
}

/// While strip `id` is dragged by its row: a line where it would go, and on letting go the move.
fn reorder(
    ui: &mut Ui,
    cx: &mut TabCx,
    card: &bpe_core::Slot,
    lane: u8,
    rows: &[(LayerId, Rect)],
    id: LayerId,
    dropped: bool,
) {
    let Some(at) = ui.input(|i| i.pointer.latest_pos()) else { return };
    let others: Vec<Rect> = rows.iter().filter(|(l, _)| *l != id).map(|(_, r)| *r).collect();
    let place = others.iter().filter(|r| r.center().y < at.y).count();
    let now = rows.iter().position(|(l, _)| *l == id).unwrap_or(0);
    let y = match place {
        0 => others.first().map_or(at.y, |r| r.top() - 2.0),
        p => others[p - 1].bottom() + 2.0,
    };
    let x = rows.first().map_or(at.x..=at.x, |(_, r)| r.left()..=r.right());
    if place != now {
        ui.painter().hline(x, y, egui::Stroke::new(2.0, style::TEXT));
    }
    if dropped && place != now {
        let indices: Vec<usize> = card
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| CHAIN_KINDS.contains(&l.kind.as_str()) && l.lane.unwrap_or(0) == lane)
            .map(|(i, _)| i)
            .collect();
        if let Some(&to) = indices.get(place) {
            cx.edits.push(TabEdit::MoveLayer(id, to));
        }
    }
}

/// Pastes `copied` (from `from`'s sound) into oscillator `lane`'s chain of `thing`'s sound card,
/// just after `after` (at the bottom without one), and returns the new layer. A Modulation layer
/// whose source is a layer of another shape, or one this card no longer has, takes the
/// oscillator it lands on, as the sound would.
#[allow(clippy::too_many_arguments)]
pub fn paste(
    scene: &mut Scene,
    reg: &Registry,
    thing: ThingId,
    from: ThingId,
    copied: &CopiedLayer,
    lane: u8,
    after: Option<LayerId>,
) -> Option<LayerId> {
    let mut c = copied.clone();
    c.lane = Some(lane);
    if c.kind == MODULATION
        && let Some(Value::Text(t)) = c.params.get("source")
        && let Source::After(src) = Source::parse(t)
    {
        let there = from == thing
            && scene
                .card(thing, SlotId::Sound)
                .and_then(|card| card.layer(src))
                .is_some_and(|l| CHAIN_KINDS.contains(&l.kind.as_str()));
        if !there {
            c.params.set("source", Value::Text(Source::Raw(lane).text()));
        }
    }
    let new = scene.paste_layer(reg, thing, SlotId::Sound, &c).ok()?;
    if let Some(after) = after {
        let card = scene.card(thing, SlotId::Sound)?;
        let indices: Vec<usize> = card
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| CHAIN_KINDS.contains(&l.kind.as_str()) && l.lane.unwrap_or(0) == lane)
            .map(|(i, _)| i)
            .collect();
        let place = indices.iter().position(|&i| card.layers[i].id == after).map(|p| p + 1);
        if let Some(&to) = place.and_then(|p| indices.get(p))
            && card.layers[to].id != new
        {
            scene.move_layer(reg, thing, SlotId::Sound, new, to).ok()?;
        }
    }
    Some(new)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SHAPE, sounding};
    use super::*;
    use bpe_core::{Address, layered_key};
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

    #[test]
    fn drag_reorders_within_the_lane() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        let d = add(&mut h, 0, DISTORTION);
        add(&mut h, 1, FILTER);
        let from = h.get_by_label("Filter strip").rect().center();
        let below = h.get_by_label("Distortion strip").rect();
        drag(&mut h, from, egui::pos2(from.x, below.bottom() + 4.0));
        let order: Vec<LayerId> = lane_layers(&card(&h).layers, 0).iter().map(|l| l.id).collect();
        assert_eq!(order, vec![d, f], "the filter went under the distortion");
        assert_eq!(lane_layers(&card(&h).layers, 1).len(), 1, "Osc B's chain is left alone");
    }

    fn strip_menu(h: &mut H, strip: &str, item: &str) {
        h.get_by_label(&format!("{strip} strip")).click_secondary();
        h.run();
        h.get_by_label(item).click();
        h.run();
    }

    #[test]
    fn paste_into_the_other_osc_keeps_a_valid_source() {
        let mut h = sounding();
        let fb = add(&mut h, 1, FILTER);
        let m = add(&mut h, 0, MODULATION);
        let reg = bpe_kit::registry();
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(m, "source"));
        h.state_mut().0.scene.set_at(&reg, &a, Value::Text(fb.0.to_string())).unwrap();
        h.run();
        strip_menu(&mut h, "Modulation", "Copy layer");
        h.get_by_label("Add layer to Osc B").click();
        h.run();
        h.get_by_label("Paste layer").click();
        h.run();
        let c = card(&h);
        let b = lane_layers(&c.layers, 1);
        assert_eq!(b.len(), 2);
        let pasted = b[1];
        assert_eq!(pasted.kind, MODULATION);
        assert_eq!(value(&h, pasted.id, "source"), Some(Value::Text(fb.0.to_string())), "still after B's filter");
        assert!(bpe_sound::chain_graph::loops_of(&c).is_empty(), "B's filter is above it: no loop");
        // With the filter gone, a paste falls back to the oscillator it lands on.
        h.state_mut().0.scene.remove_layer(&reg, SHAPE, SlotId::Sound, fb).unwrap();
        h.run();
        strip_menu(&mut h, "Modulation", "Paste layer");
        let c = card(&h);
        let a_lane = lane_layers(&c.layers, 0);
        assert_eq!(a_lane.len(), 2, "pasted into Osc A after the strip");
        assert_eq!(value(&h, a_lane[1].id, "source"), Some(Value::Text("a".into())));
    }

    #[test]
    fn delete_removes_routes_to_its_knobs() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        let reg = bpe_kit::registry();
        let adsr = card(&h).layers.iter().find(|l| l.kind == bpe_kit::sound::ADSR).unwrap().id;
        let route = bpe_core::ModRoute {
            source: adsr,
            target: layered_key(f, "cutoff"),
            amount: 0.5,
            bipolar: false,
            invert: false,
        };
        let key = Address::on(SHAPE, SlotId::Sound, bpe_core::ROUTES_KEY);
        let mut routes = match card(&h).params.get(bpe_core::ROUTES_KEY) {
            Some(Value::Routes(r)) => r.clone(),
            _ => Vec::new(),
        };
        let before = routes.len();
        routes.push(route);
        h.state_mut().0.scene.set_at(&reg, &key, Value::Routes(routes)).unwrap();
        h.run();
        strip_menu(&mut h, "Filter", "Delete");
        let c = card(&h);
        assert!(c.layer(f).is_none(), "the strip is gone");
        let left = match c.params.get(bpe_core::ROUTES_KEY) {
            Some(Value::Routes(r)) => r.clone(),
            _ => Vec::new(),
        };
        assert_eq!(left.len(), before, "the route to its cutoff went with it");
        assert!(h.query_by_label("Filter strip").is_none());
    }

    #[test]
    fn rename_in_place() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        strip_menu(&mut h, "Filter", "Rename");
        let edit = h.get_by_label("Rename layer");
        edit.focus();
        h.run();
        h.get_by_label("Rename layer").type_text("Warm");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(card(&h).layer(f).unwrap().name, "Warm", "typing replaced the name");
        assert!(h.query_by_label("Warm strip").is_some());
    }

    #[test]
    fn duplicate_adds_a_copy_under_it() {
        let mut h = sounding();
        add(&mut h, 0, FILTER);
        strip_menu(&mut h, "Filter", "Duplicate");
        let names: Vec<String> = lane_layers(&card(&h).layers, 0).iter().map(|l| l.name.clone()).collect();
        assert_eq!(names, vec!["Filter".to_owned(), "Filter copy".to_owned()]);
    }

    #[test]
    fn deleting_a_source_points_fm_at_its_own_osc() {
        let mut h = sounding();
        let fb = add(&mut h, 1, FILTER);
        let m = add(&mut h, 0, MODULATION);
        let reg = bpe_kit::registry();
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(m, "source"));
        h.state_mut().0.scene.set_at(&reg, &a, Value::Text(fb.0.to_string())).unwrap();
        h.run();
        strip_menu(&mut h, "Filter", "Delete");
        assert_eq!(value(&h, m, "source"), Some(Value::Text("a".into())), "its own oscillator, as it sounds");
        // A new layer that takes the old number is not the FM's source.
        let d = add(&mut h, 0, DISTORTION);
        assert_eq!(value(&h, m, "source"), Some(Value::Text("a".into())));
        assert!(!bpe_sound::chain_graph::loops_of(&card(&h)).contains(&m), "{d:?} is not tapped");
    }

    #[test]
    fn paste_into_another_shape_drops_a_layer_source() {
        let mut h = sounding();
        let f = add(&mut h, 0, FILTER);
        let m = add(&mut h, 0, MODULATION);
        let reg = bpe_kit::registry();
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(m, "source"));
        h.state_mut().0.scene.set_at(&reg, &a, Value::Text(f.0.to_string())).unwrap();
        h.run();
        strip_menu(&mut h, "Modulation", "Copy layer");
        // Another shape whose own layers happen to carry the same numbers.
        let other = {
            let studio = &mut h.state_mut().0;
            let id = bpe_kit::add_thing(&mut studio.scene, &reg, bpe_kit::things::shape::KIND, ThingId::ROOT).unwrap();
            crate::sound_tab::write::enable(&mut studio.scene, &reg, id);
            for _ in 0..3 {
                studio.scene.add_layer_in(&reg, id, SlotId::Sound, DISTORTION, Some(1)).unwrap();
            }
            studio.selection.set(id);
            studio.panels.inspector.tab.insert(id, crate::inspector::InspectorTab::Sound);
            id
        };
        h.run();
        h.get_by_label("Add layer to Osc A").click();
        h.run();
        h.get_by_label("Paste layer").click();
        h.run();
        let c = h.state().0.scene.card(other, SlotId::Sound).unwrap().clone();
        let pasted = lane_layers(&c.layers, 0)[0];
        let key = layered_key(pasted.id, "source");
        assert_eq!(c.params.get(&key), Some(&Value::Text("a".into())), "not a layer of this shape");
    }
}
