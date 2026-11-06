//! One strip of the Lights tab: the track tag, the name, M and S, the light's colour dot and its
//! Strength fader.

use bpe_core::{Registry, SlotId, ThingId, Value};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui, UiBuilder};

use super::{LightSource, Source};
use crate::app::Studio;
use crate::sound_tab::name_it;
use crate::style;
use crate::widgets::{self, KeyMark, ValueMenu};

/// Draws `source`'s strip and does what was asked of it.
pub fn show(ui: &mut Ui, studio: &mut Studio, reg: &Registry, source: &Source) {
    let selected = match source.source {
        LightSource::Light(id) => studio.selection.primary == Some(id),
        LightSource::Layer(id, _, layer) => {
            studio.selection.primary == Some(id) && studio.panels.inspector.selected_layer.get(&id) == Some(&layer)
        }
        LightSource::Ambient => studio.selection.primary == Some(ThingId::ROOT),
    };
    let stroke = if selected { Stroke::new(1.5, style::TEXT) } else { Stroke::new(1.0, style::LINE_SOFT) };
    let tag = source.thing.map_or(style::FAINT, |id| style::colour_of(&studio.scene, id));
    let frame = egui::Frame::new()
        .fill(style::STAGE)
        .stroke(stroke)
        .corner_radius(style::RADIUS)
        .inner_margin(egui::Margin { left: 12, right: 8, top: 6, bottom: 6 });
    let shown = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let top = ui.horizontal(|ui| {
                let name = ui.add(
                    egui::Label::new(RichText::new(&source.name).color(style::TEXT_STRONG))
                        .truncate()
                        .selectable(false)
                        .sense(Sense::click()),
                );
                name_it(ui, &name, &source.name);
                if !source.what.is_empty() {
                    ui.label(RichText::new(&source.what).small().color(style::MUTED));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    solo_button(ui, studio, source);
                    mute_button(ui, studio, reg, source);
                });
                name
            });
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                if let Some([r, g, b]) = source.colour {
                    let c = Color32::from_rgb(r, g, b); // scene colour
                    ui.painter().circle(dot.center(), 5.5, c, Stroke::new(1.0, style::LINE));
                }
                fader(ui, studio, reg, source);
            });
            top.inner
        })
    });
    // The track tag down the strip's left edge.
    let rect = shown.inner.response.rect;
    let name = shown.inner.inner;
    let bar = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 6.0), egui::vec2(4.0, rect.height() - 12.0));
    ui.painter().rect_filled(bar, 2.0, tag);
    let r = shown.response.on_hover_text("Click to show this light in the inspector");
    if r.clicked() || name.clicked() {
        match source.source {
            LightSource::Light(id) => studio.selection.set(id),
            LightSource::Layer(id, slot, layer) => {
                studio.selection.set(id);
                // A sub-layer is edited in its layer's box: pick the layer it belongs to.
                let parent = studio.scene.card(id, slot).and_then(|c| c.layer(layer)).and_then(|l| l.parent);
                studio.panels.inspector.selected_layer.insert(id, parent.unwrap_or(layer));
            }
            LightSource::Ambient => studio.selection.set(ThingId::ROOT),
        }
    }
}

/// M: turns the light off (a Light thing's own on/off, a layer's, or Ambient on). Saved.
fn mute_button(ui: &mut Ui, studio: &mut Studio, reg: &Registry, source: &Source) {
    let r = ui.add(style::toggle_button("M", !source.on).min_size(egui::vec2(20.0, 20.0)));
    name_it(ui, &r, &format!("{} mute", source.name));
    let r = r.on_hover_text("Mute: turns this light off (saved with the scene)");
    if !r.clicked() {
        return;
    }
    let on = !source.on;
    let done = match source.source {
        LightSource::Light(id) => match studio.scene.things.get_mut(&id) {
            Some(t) => {
                t.enabled = on;
                true
            }
            None => false,
        },
        LightSource::Layer(id, slot, layer) => studio.scene.set_layer_enabled(reg, id, slot, layer, on).is_ok(),
        LightSource::Ambient => {
            let a = bpe_core::Address::on(ThingId::ROOT, SlotId::Lighting, "ambient_on");
            studio.scene.set_at(reg, &a, Value::Bool(on)).is_ok()
        }
    };
    if done {
        studio.panels.inspector.own_step = true;
    }
}

/// S: solo, the editor's alone. While any strip is soloed the preview shows only soloed lights.
fn solo_button(ui: &mut Ui, studio: &mut Studio, source: &Source) {
    let on = studio.light_solo.contains(&source.source);
    let r = ui.add(style::toggle_button("S", on).min_size(egui::vec2(20.0, 20.0)));
    name_it(ui, &r, &format!("{} solo", source.name));
    let r = r.on_hover_text("Solo: see only the soloed lights in the preview (never saved or exported)");
    if r.clicked() {
        if on {
            studio.light_solo.remove(&source.source);
        } else {
            studio.light_solo.insert(source.source);
        }
    }
}

/// The Strength fader: the real setting at the playhead, keyed when it has keys.
fn fader(ui: &mut Ui, studio: &mut Studio, reg: &Registry, source: &Source) {
    let Some(a) = source.strength.clone() else { return };
    let Some(def) = studio.scene.def_at(reg, &a) else { return };
    let time = studio.sim_time();
    let Some(now) = studio.scene.value_at_time(reg, &a, time) else { return };
    let Value::Number(mut x) = now else { return };
    let span = def.span.or(def.range).unwrap_or((0.0, 4.0));
    let feel = widgets::setting_feel(def);
    let words = format!("{} strength", source.name);
    let r = style::wells(ui, |ui| {
        let unit = widgets::suffix(def.unit);
        widgets::span_slider(ui, &mut x, span, def.range, feel, false, unit, widgets::Named::Words(&words))
    });
    let mut keyed = false;
    if r.changed() && !widgets::right_button_busy(ui) {
        crate::inspector::set_or_key(&mut studio.scene, reg, &a, Value::Number(x), time, &mut keyed);
    }
    let mark = Some(KeyMark { can_key: def.flags.keyframe, keyed: studio.scene.keyed(&a), pulsed: false });
    let choices = widgets::TargetChoices::default();
    match widgets::value_menu(&r, def, def.unit, &now, mark, &choices) {
        ValueMenu::Nothing => {}
        ValueMenu::Paste(v) => {
            crate::inspector::set_or_key(&mut studio.scene, reg, &a, v, time, &mut keyed);
            studio.panels.inspector.own_step = true;
        }
        ValueMenu::CreateKey => {
            if studio.scene.set_key(reg, &a, time, now.clone()).is_ok() {
                keyed = true;
                studio.panels.inspector.own_step = true;
            }
        }
    }
    if keyed {
        studio.panels.inspector.keyed_edit = true;
    }
}
