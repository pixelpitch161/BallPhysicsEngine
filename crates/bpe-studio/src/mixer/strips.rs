//! The console strips: Bus 1, Bus 2, Bus 3 and the Master side by side. A bus strip has its
//! on switch and name (double-click to rename), its effects (click one to open it in the
//! inspector, drag to reorder or onto another bus, right-click for the rest), its sends, pan,
//! fader with a meter, and mute and solo. A bus that is off is drawn faint.

use super::cx::{MixCx, MixEdit, MixerPick, STRIP_KNOB_W};
use super::{Mixer, Renaming, slot_menu};
use crate::sound_tab::{db, name_it};
use crate::style;
use bpe_core::{LayerId, Settings, SlotId, ThingId, Value};
use bpe_kit::sound::{BUSES, MixerSettings};
use bpe_sound::{BusReading, Meters};
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// A slot row's height.
const ROW_H: f32 = 20.0;
/// Rows shown before the list scrolls.
const ROWS: usize = 5;
/// The fader's height.
const FADER_H: f32 = 150.0;
const FADER_W: f32 = 18.0;
/// A fader full turn is its height of drag; Shift for fine steps.
const FADER_LO: f64 = -60.0;
const FADER_HI: f64 = 6.0;

/// The effect being dragged between slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotDrag(pub LayerId);

/// The mixer card's settings as set (no keys), for the send graph.
pub fn mixer_settings(cx: &MixCx) -> MixerSettings {
    cx.scene.card(ThingId::ROOT, SlotId::Mixer).map(|c| MixerSettings::from_params(&c.params)).unwrap_or_default()
}

/// A plain-text pan: "C", "L 40", "R 100".
pub fn pan_text(v: f64) -> String {
    if v.abs() < 0.5 {
        "C".into()
    } else if v < 0.0 {
        format!("L {:.0}", -v)
    } else {
        format!("R {v:.0}")
    }
}

/// Draws the three bus strips; `width` is each column's.
pub fn buses(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, meters: Option<&Meters>, width: f32) {
    let graph = mixer_settings(cx);
    let mut hovered = None;
    for n in 1..=BUSES as u8 {
        let reading = meters.map_or(BusReading::SILENT, |m| m.read_bus(n as usize - 1));
        let column =
            ui.allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                bus(ui, cx, mixer, &graph, n, &reading);
            });
        if ui.rect_contains_pointer(column.response.rect) {
            hovered = Some(n);
        }
    }
    mixer.hover_bus = hovered;
}

/// One bus strip.
fn bus(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, graph: &MixerSettings, n: u8, reading: &BusReading) {
    let key = |k: &str| MixerSettings::bus_key(n, k);
    let on = cx.flag(&key("on"));
    let name = cx.text(&key("name"));
    let shown = if name.trim().is_empty() { format!("Bus {n}") } else { name.clone() };
    ui.spacing_mut().item_spacing = Vec2::new(2.0, 4.0);

    // The on switch and the name.
    ui.horizontal(|ui| {
        cx.switch(ui, &key("on"), &format!("Bus {n} on"));
        let editing = matches!(&mixer.renaming, Some((Renaming::Bus(b), _)) if *b == n);
        if editing {
            let Some((_, text)) = mixer.renaming.as_mut() else { return };
            let edit = ui.add(egui::TextEdit::singleline(text).hint_text(&shown).desired_width(ui.available_width()));
            name_it(ui, &edit, &format!("Name of Bus {n}"));
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() {
                let typed = text.trim().to_owned();
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) && !typed.is_empty() {
                    cx.set(&key("name"), Value::Text(typed));
                }
                mixer.renaming = None;
            }
        } else {
            let label = ui.add(
                egui::Label::new(RichText::new(&shown).font(style::semibold(13.0)).color(if on {
                    style::TEXT
                } else {
                    style::FAINT
                }))
                .truncate()
                .sense(Sense::click()),
            );
            let label = label.on_hover_text("Double-click to rename");
            if label.double_clicked() {
                mixer.renaming = Some((Renaming::Bus(n), String::new()));
            }
        }
    });

    ui.add_enabled_ui(on, |ui| {
        // The effects.
        caption(ui, "Layers");
        slots(ui, cx, mixer, n - 1);

        // The sends to the other two buses.
        caption(ui, "Sends");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let half = (ui.available_width() / 2.0).floor();
            for to in (1..=BUSES as u8).filter(|to| *to != n) {
                let looping = bpe_sound::would_loop(graph, n, to);
                let to_on = cx.flag(&MixerSettings::bus_key(to, "on"));
                let send = key(&format!("send{to}"));
                let label = format!("To {to}");
                let r = ui.add_enabled_ui(!looping, |ui| cx.knob_w(ui, &send, &label, &db, half)).inner;
                if looping {
                    r.on_disabled_hover_text("That would feed back into itself");
                } else if !to_on {
                    r.on_hover_text(format!("Bus {to} is off: this send is silent until it's on"));
                }
            }
        });

        // Pan, then the fader with its meter.
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add_space(((w - STRIP_KNOB_W) / 2.0).max(0.0));
            cx.knob_w(ui, &key("pan"), "Pan", &pan_text, STRIP_KNOB_W);
        });
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add_space(((w - FADER_W - 18.0) / 2.0).max(0.0));
            fader(ui, cx, &key("fader"), &format!("Bus {n} fader"));
            ui.add_space(4.0);
            style::level_meter(ui, reading.peak_l, reading.peak_r, FADER_H).on_hover_text("Peak level, left and right");
        });
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(db(cx.number(&key("fader")))).font(style::mono(11.0)).color(style::TEXT));
            if reading.reduction < -0.1 {
                ui.label(RichText::new(format!("GR {:.1}", reading.reduction)).small().color(style::MUTED))
                    .on_hover_text("How far this bus's effects are turning it down");
            }
        });

        // Mute and solo.
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add_space(((w - 2.0 * 28.0 - 2.0) / 2.0).max(0.0));
            for (k, chip, what) in [("mute", "M", "mute"), ("solo", "S", "solo")] {
                let lit = cx.flag(&key(k));
                let r = ui.add(style::toggle_button(chip, lit).min_size(egui::vec2(28.0, 22.0)));
                name_it(ui, &r, &format!("Bus {n} {what}"));
                if r.clicked() {
                    cx.set(&key(k), Value::Bool(!lit));
                }
                let r = r.on_hover_text(if k == "mute" {
                    "Mute: silences the bus and its sends"
                } else {
                    "Solo: while any bus is soloed, only soloed buses play"
                });
                if let Some(def) = cx.def(&key(k)) {
                    let now = cx.get(&key(k));
                    cx.menu(&r, &key(k), def, &now);
                }
            }
        });
    });
}

/// A small grey heading inside a strip.
fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(style::MUTED));
}

/// Bus `lane`'s effects as rows, scrolling past five, with the add row last.
fn slots(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, lane: u8) {
    let effects = cx.effects(lane);
    // Always as tall as five effects and the add row, so the strips line up side by side.
    let height = ROW_H * (ROWS + 1) as f32;
    egui::Frame::new().fill(style::GROUND).corner_radius(style::RADIUS).inner_margin(egui::Margin::same(2)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(height);
            egui::ScrollArea::vertical()
                .id_salt(("bus slots", lane))
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, layer) in effects.iter().enumerate() {
                        slot_row(ui, cx, mixer, lane, i, layer);
                    }
                    let add = slot_menu::add_row(ui, cx, lane);
                    // Dropped on the add row: to the end of this bus.
                    if let Some(drag) = egui::DragAndDrop::payload::<SlotDrag>(ui.ctx())
                        && add.contains_pointer()
                        && ui.input(|i| i.pointer.any_released())
                    {
                        let index = effects.len();
                        cx.edits.push(MixEdit::Move { id: drag.0, lane, index });
                        egui::DragAndDrop::clear_payload(ui.ctx());
                    }
                });
        },
    );
}

/// One effect's row: its on dot and name.
fn slot_row(ui: &mut Ui, cx: &mut MixCx, mixer: &mut Mixer, lane: u8, index: usize, layer: &bpe_core::Layer) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
    let kind_name = cx.kit.registry.layer(&layer.kind).map_or("Effect", |k| k.name());
    let name = if layer.name.trim().is_empty() { kind_name.to_owned() } else { layer.name.clone() };
    let picked = mixer.pick == Some(MixerPick::Layer(layer.id));
    let renaming = matches!(&mixer.renaming, Some((Renaming::Layer(id), _)) if *id == layer.id);
    let dot = Rect::from_center_size(Pos2::new(rect.left() + 9.0, rect.center().y), Vec2::splat(12.0));

    if response.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), SlotDrag(layer.id));
    }
    // Something dropped here takes this row's place.
    let hovering = egui::DragAndDrop::payload::<SlotDrag>(ui.ctx()).filter(|p| p.0 != layer.id);
    if let Some(drag) = hovering.as_ref()
        && response.contains_pointer()
        && ui.input(|i| i.pointer.any_released())
    {
        let from = cx.effects(lane).iter().position(|l| l.id == drag.0);
        let index = match from {
            Some(from) if from < index => index - 1,
            _ => index,
        };
        cx.edits.push(MixEdit::Move { id: drag.0, lane, index });
        egui::DragAndDrop::clear_payload(ui.ctx());
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if picked {
            style::SELECTED
        } else if response.hovered() {
            style::HOVER
        } else {
            style::GROUND
        };
        painter.rect_filled(rect, style::RADIUS, fill);
        if hovering.is_some() && response.contains_pointer() {
            painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, style::TEXT));
        }
        if layer.enabled {
            painter.circle_filled(dot.center(), 3.5, style::TEXT);
        } else {
            painter.circle_stroke(dot.center(), 3.5, Stroke::new(1.0, style::FAINT));
        }
    }
    if !renaming {
        let ink = if layer.enabled { style::TEXT } else { style::FAINT };
        let text = Rect::from_min_max(Pos2::new(dot.right() + 4.0, rect.top()), rect.max);
        ui.put(text, egui::Label::new(RichText::new(&name).size(12.0).color(ink)).truncate().selectable(false));
    }
    let state = layer.enabled;
    let label = name.clone();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, state, &label));

    if renaming {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.with_min_x(dot.right() + 2.0)));
        if let Some((_, text)) = mixer.renaming.as_mut() {
            let edit = child.add(egui::TextEdit::singleline(text).hint_text(&name).desired_width(rect.width() - 20.0));
            name_it(&child, &edit, "Effect name");
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() {
                if !child.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cx.edits.push(MixEdit::Rename(layer.id, text.clone()));
                }
                mixer.renaming = None;
            }
        }
    } else if response.clicked() {
        let on_dot = response.interact_pointer_pos().is_some_and(|p| p.x < dot.right() + 2.0);
        if on_dot {
            cx.edits.push(MixEdit::Enable(layer.id, !layer.enabled));
        } else {
            cx.edits.push(MixEdit::Pick(Some(MixerPick::Layer(layer.id))));
        }
    } else if response.double_clicked() {
        mixer.renaming = Some((Renaming::Layer(layer.id), String::new()));
    }
    let response = response.on_hover_text(format!("{kind_name}: click to open, drag to move, right-click for more"));
    slot_menu::row_menu(&response, cx, mixer, lane, layer);
}

/// A vertical fader over a dB setting, with the value menu on right-click. Drag up to turn it
/// up (Shift for fine steps), double-click for 0 dB.
pub fn fader(ui: &mut Ui, cx: &mut MixCx, key: &str, name: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(FADER_W, FADER_H), Sense::click_and_drag());
    let now = cx.get(key);
    let before = crate::sound_tab::number(&now).unwrap_or(0.0);
    let mut v = before;
    let track = rect.shrink2(Vec2::new(0.0, 6.0));
    if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let dy = response.drag_delta().y as f64;
        v = (v - dy / track.height() as f64 * (FADER_HI - FADER_LO) * fine).clamp(FADER_LO, FADER_HI);
    }
    if response.double_clicked() {
        v = 0.0;
    }
    if v != before {
        cx.set(key, Value::Number((v * 10.0).round() / 10.0));
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let x = rect.center().x;
        painter.line_segment([Pos2::new(x, track.top()), Pos2::new(x, track.bottom())], Stroke::new(3.0, style::LINE));
        let y_of = |db: f64| track.bottom() - ((db - FADER_LO) / (FADER_HI - FADER_LO)) as f32 * track.height();
        // A tick at 0 dB.
        let zero = y_of(0.0);
        painter.line_segment(
            [Pos2::new(rect.left(), zero), Pos2::new(rect.right(), zero)],
            Stroke::new(1.0, style::FAINT),
        );
        let y = y_of(v);
        let ink = if !ui.is_enabled() {
            style::FAINT
        } else if response.hovered() || response.dragged() {
            style::TEXT_STRONG
        } else {
            style::TEXT
        };
        let cap = Rect::from_center_size(Pos2::new(x, y), Vec2::new(FADER_W, 10.0));
        painter.rect_filled(cap, style::RADIUS, style::RAISED);
        painter.rect_stroke(cap, style::RADIUS, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
        painter.line_segment([Pos2::new(cap.left() + 3.0, y), Pos2::new(cap.right() - 3.0, y)], Stroke::new(1.0, ink));
    }
    let label = name.to_owned();
    response.widget_info(|| egui::WidgetInfo::slider(true, v, &label));
    name_it(ui, &response, name);
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if let Some(def) = cx.def(key) {
        cx.menu(&response, key, def, &now);
    }
    response
}

/// The compact Master strip beside the buses: its knobs two by two and its meter.
pub fn master(
    ui: &mut Ui,
    scene: &mut bpe_core::Scene,
    kit: &bpe_kit::Kit,
    mixer: &mut Mixer,
    reading: &bpe_sound::MeterReading,
    time: f64,
    width: f32,
) {
    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing = Vec2::new(2.0, 4.0);
        let picked = mixer.pick == Some(MixerPick::Master);
        let title = ui.add(
            egui::Label::new(RichText::new("Master").font(style::semibold(13.0)).color(if picked {
                style::TEXT_STRONG
            } else {
                style::TEXT
            }))
            .sense(Sense::click()),
        );
        if title.on_hover_text("Click to open the Master in the inspector").clicked() {
            mixer.pick = Some(MixerPick::Master);
        }
        let half = (width / 2.0).floor();
        let knobs = super::master::KNOBS;
        for pair in knobs.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for k in pair {
                    super::master::knob(ui, scene, kit, k, time, half);
                }
            });
        }
        ui.horizontal(|ui| {
            let w = ui.available_width();
            ui.add_space(((w - 14.0) / 2.0).max(0.0));
            style::level_meter(ui, reading.peak_l, reading.peak_r, FADER_H - 40.0)
                .on_hover_text("Peak level, left and right");
        });
        if reading.reduction < -0.1 {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("GR {:.1}", reading.reduction)).small().color(style::MUTED))
                    .on_hover_text("How far the ceiling is turning the sound down");
            });
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::app::{LeftPanel, Studio};
    use crate::style;
    use bpe_core::{Address, SlotId, ThingId, Value};
    use eframe::egui::{self, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    /// The studio with the Mixer open on the left, each frame's edit recorded as the studio
    /// records it.
    pub fn mixer_harness() -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        studio.left = LeftPanel::Mixer;
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400.0, 1000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, studio: &mut Studio| {
                    let before = studio.scene.clone();
                    studio.recorded = false;
                    studio.left_panel(ui, &before);
                    egui::CentralPanel::default().show(ui, |_| {});
                    if !std::mem::take(&mut studio.recorded) {
                        studio.after_edit(before);
                    }
                },
                studio,
            );
        h.run();
        h
    }

    pub fn mixer_value(s: &Studio, key: &str) -> Option<Value> {
        s.scene.card(ThingId::ROOT, SlotId::Mixer).and_then(|c| c.params.get(key).cloned())
    }

    pub fn set_mixer(h: &mut Harness<'_, Studio>, key: &str, v: Value) {
        h.state_mut().scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap().params.set(key, v);
        h.run();
    }

    fn effects(s: &Studio) -> Vec<(String, Option<u8>)> {
        s.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().layers.iter().map(|l| (l.kind.clone(), l.lane)).collect()
    }

    fn press<S>(h: &mut Harness<'_, S>, pos: egui::Pos2, pressed: bool) {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }

    /// Drags from `from` to `to` in a few steps.
    pub fn drag<S>(h: &mut Harness<'_, S>, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        h.run();
        press(h, from, true);
        h.step();
        for k in 1..=4 {
            h.event(egui::Event::PointerMoved(from + (to - from) * k as f32 / 4.0));
            h.step();
        }
        press(h, to, false);
        h.run();
    }

    fn slider(h: &Harness<'_, Studio>, name: &str, nth: usize) -> egui::Rect {
        h.query_all_by_label(name).filter(|n| n.accesskit_node().role() == Role::Slider).nth(nth).unwrap().rect()
    }

    fn slider_disabled(h: &Harness<'_, Studio>, name: &str, nth: usize) -> bool {
        h.query_all_by_label(name)
            .filter(|n| n.accesskit_node().role() == Role::Slider)
            .nth(nth)
            .unwrap()
            .accesskit_node()
            .is_disabled()
    }

    #[test]
    fn four_columns_bus2_and_3_dimmed() {
        let h = mixer_harness();
        for name in ["Bus 1", "Bus 2", "Bus 3", "Master"] {
            assert!(h.query_by_label(name).is_some(), "{name} has a strip");
        }
        let x = |name: &str| h.query_all_by_label(name).next().unwrap().rect().left();
        assert!(x("Bus 1") < x("Bus 2") && x("Bus 2") < x("Bus 3") && x("Bus 3") < x("Master"));
        assert!(!slider_disabled(&h, "Bus 1 fader", 0));
        assert!(slider_disabled(&h, "Bus 2 fader", 0), "Bus 2 is off: its controls are faint");
        assert!(slider_disabled(&h, "Bus 3 fader", 0));
        // The on switch stays live, so it can be turned on.
        let on = h.get_by_role_and_label(Role::CheckBox, "Bus 2 on");
        assert!(!on.accesskit_node().is_disabled());
        on.click();
        let mut h = h;
        h.run();
        assert_eq!(mixer_value(h.state(), "bus2_on"), Some(Value::Bool(true)));
        assert!(!slider_disabled(&h, "Bus 2 fader", 0));
    }

    #[test]
    fn adding_a_layer_to_bus1() {
        let mut h = mixer_harness();
        h.get_by_label("Add to Bus 1").click();
        h.run();
        h.query_all_by_label("Equalizer").next().unwrap().click();
        h.run();
        assert_eq!(effects(h.state()), [("fx.eq".to_owned(), Some(0))]);
        let id = h.state().scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().layers[0].id;
        assert_eq!(h.state().panels.mixer.pick, Some(super::MixerPick::Layer(id)), "it opens");
        assert!(h.query_all_by_label("Equalizer").any(|n| n.accesskit_node().role() == Role::Button), "its row shows");
    }

    #[test]
    fn dragging_a_slot_to_bus2_moves_it() {
        let mut h = mixer_harness();
        set_mixer(&mut h, "bus2_on", Value::Bool(true));
        for kind in ["fx.eq", "fx.compressor"] {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, kind, Some(0)).unwrap();
        }
        h.run();
        let row = h.query_all_by_label("Equalizer").find(|n| n.accesskit_node().role() == Role::Button).unwrap().rect();
        let target = h.get_by_label("Add to Bus 2").rect();
        drag(&mut h, row.center(), target.center());
        assert_eq!(
            effects(h.state()),
            [("fx.compressor".to_owned(), Some(0)), ("fx.eq".to_owned(), Some(1))],
            "the EQ moved to Bus 2"
        );
        // Dragging the compressor back onto the EQ's row puts it first on Bus 2.
        let row =
            h.query_all_by_label("Compressor").find(|n| n.accesskit_node().role() == Role::Button).unwrap().rect();
        let eq = h.query_all_by_label("Equalizer").find(|n| n.accesskit_node().role() == Role::Button).unwrap().rect();
        drag(&mut h, row.center(), eq.center());
        let lanes: Vec<_> = h
            .state()
            .scene
            .card(ThingId::ROOT, SlotId::Mixer)
            .unwrap()
            .layers
            .iter()
            .map(|l| (l.kind.clone(), l.lane))
            .collect();
        assert_eq!(lanes, [("fx.compressor".to_owned(), Some(1)), ("fx.eq".to_owned(), Some(1))]);
        assert!(h.state_mut().undo());
        assert_eq!(effects(h.state())[0].1, Some(0), "undo puts it back on Bus 1");
    }

    #[test]
    fn a_looping_send_is_greyed() {
        let mut h = mixer_harness();
        set_mixer(&mut h, "bus2_on", Value::Bool(true));
        set_mixer(&mut h, "bus3_on", Value::Bool(true));
        assert!(!slider_disabled(&h, "To 1", 0), "nothing sends yet");
        set_mixer(&mut h, "bus1_send2", Value::Number(0.0));
        // Bus 1 sends to Bus 2, so Bus 2 can't send back; Bus 3 still can.
        assert!(slider_disabled(&h, "To 1", 0), "Bus 2 to Bus 1 would feed back");
        assert!(!slider_disabled(&h, "To 1", 1), "Bus 3 to Bus 1 is fine");
        assert!(!slider_disabled(&h, "To 3", 1), "Bus 2 to Bus 3 is fine");
    }

    #[test]
    fn fader_and_pan_edit_and_undo() {
        let mut h = mixer_harness();
        let fader = slider(&h, "Bus 1 fader", 0);
        // 46 px down of the 138 px track over 66 dB.
        drag(&mut h, fader.center(), fader.center() + egui::vec2(0.0, 46.0));
        let v = match mixer_value(h.state(), "bus1_fader") {
            Some(Value::Number(v)) => v,
            other => panic!("{other:?}"),
        };
        assert!((v + 22.0).abs() < 0.3, "{v}");
        let pan = slider(&h, "Pan", 0);
        drag(&mut h, pan.center(), pan.center() + egui::vec2(0.0, -50.0));
        assert_eq!(mixer_value(h.state(), "bus1_pan"), Some(Value::Number(50.0)));
        // Undo takes them back (edits close together are one step).
        while mixer_value(h.state(), "bus1_fader").is_some() {
            assert!(h.state_mut().undo());
        }
        assert_eq!(mixer_value(h.state(), "bus1_pan"), None);
    }

    #[test]
    fn bus_rename_by_double_click() {
        let mut h = mixer_harness();
        let at = h.get_by_label("Bus 2").rect().center();
        h.hover_at(at);
        h.run();
        for _ in 0..2 {
            press(&mut h, at, true);
            h.step();
            press(&mut h, at, false);
            h.step();
        }
        h.run();
        assert!(h.query_by_label("Name of Bus 2").is_some(), "a box to type the name");
        h.event(egui::Event::Text("Drums".into()));
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(mixer_value(h.state(), "bus2_name"), Some(Value::Text("Drums".into())));
        assert!(h.query_by_label("Drums").is_some());
        let a = Address::on(ThingId::ROOT, SlotId::Mixer, "bus2_name");
        assert!(!h.state().scene.keyed(&a));
    }
}
