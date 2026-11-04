//! The Mixer's editors in the wide inspector: a bus effect's (picked in its strip) or the
//! Master's. Each effect kind has its own view where one helps (the EQ's curve, the
//! compressor's level graph, the delay's echoes, the reverb's tail); the rest of an effect's
//! settings are drawn from its settings list.

pub mod comp;
pub mod delay;
pub mod eq;
pub mod master;
pub mod reverb;

use super::Mixer;
use super::cx::{self, MixCx, MixEdit, MixerPick};
use crate::sound_tab::{name_it, panel};
use crate::style;
use crate::widgets::{self, TargetChoices};
use bpe_core::{Params, Scene, SettingDef, SlotId, Target, ThingId, Value, ValueType};
use bpe_kit::Kit;
use bpe_kit::sound::fx;
use bpe_sound::{MeterReading, Meters};
use eframe::egui::{self, RichText, Ui};

/// What the editor reads besides the scene.
pub struct Readings<'a> {
    pub meters: Option<&'a Meters>,
    pub master: MeterReading,
}

/// The title of an effect's editor: "Bus 1 · Equalizer".
pub fn title(scene: &Scene, kit: &Kit, pick: MixerPick) -> String {
    match pick {
        MixerPick::Master => "Master".into(),
        MixerPick::Layer(id) => {
            let Some(card) = scene.card(ThingId::ROOT, SlotId::Mixer) else { return String::new() };
            let Some(layer) = card.layer(id) else { return String::new() };
            let bus = layer.lane.unwrap_or(0) + 1;
            let bus_key = bpe_kit::sound::MixerSettings::bus_key(bus, "name");
            let bus_name = match card.params.get(&bus_key) {
                Some(Value::Text(t)) if !t.trim().is_empty() => t.clone(),
                _ => format!("Bus {bus}"),
            };
            let kind = kit.registry.layer(&layer.kind).map_or("Effect", |k| k.name());
            let name = if layer.name.trim().is_empty() { kind } else { layer.name.as_str() };
            format!("{bus_name} \u{b7} {name}")
        }
    }
}

/// The picked editor. Edits apply after drawing, through the usual path. Returns whether a
/// keyed setting was edited.
pub fn show(ui: &mut Ui, mixer: &mut Mixer, scene: &mut Scene, kit: &Kit, readings: &Readings, time: f64) -> bool {
    let Some(pick) = mixer.pick else { return false };
    let mut edits = Vec::new();
    let choices = TargetChoices::default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(title(scene, kit, pick)).font(style::semibold(15.0)).color(style::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").on_hover_text("Back to the selected thing's settings").clicked() {
                edits.push(MixEdit::Pick(None));
            }
        });
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match pick {
        MixerPick::Master => {
            master::show(ui, scene, kit, &readings.master, &mixer.loudness, time);
        }
        MixerPick::Layer(id) => {
            let Some(layer) = scene.card(ThingId::ROOT, SlotId::Mixer).and_then(|c| c.layer(id)).cloned() else {
                return;
            };
            let mut cx = MixCx { scene, kit, choices: &choices, time, layer: Some(id), edits: Vec::new() };
            ui.horizontal(|ui| {
                let mut on = layer.enabled;
                let r = style::switch(ui, &mut on);
                name_it(ui, &r, "Effect on");
                ui.label(RichText::new(if on { "On" } else { "Bypassed" }).color(style::MUTED));
                if r.changed() {
                    cx.edits.push(MixEdit::Enable(id, on));
                }
            });
            ui.add_space(4.0);
            let defs = kit.registry.layer(&layer.kind).map_or(&[][..], |k| k.settings());
            match layer.kind.as_str() {
                fx::EQ => eq::show(ui, &mut cx, mixer, readings.meters, id),
                fx::COMPRESSOR => comp::show(ui, &mut cx, readings.meters, id, defs),
                fx::DELAY => delay::show(ui, &mut cx, defs),
                fx::REVERB => reverb::show(ui, &mut cx, defs),
                kind => {
                    let reading = readings.meters.map_or(bpe_sound::FxReading::SILENT, |m| m.read_fx(id.0));
                    panel(ui, |ui| {
                        // The Width shows how alike left and right are; the Limiter how far it
                        // is turning the sound down.
                        let readout = match kind {
                            fx::WIDTH => Some(("Correlation", format!("{:+.2}", reading.value))),
                            fx::LIMITER => Some(("Turned down", format!("{:.1} dB", reading.reduction.min(0.0)))),
                            _ => None,
                        };
                        if let Some((name, value)) = readout {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).color(style::MUTED));
                                ui.label(RichText::new(value).font(style::mono(13.0)));
                            });
                        }
                        controls(ui, &mut cx, defs, &[]);
                    });
                }
            }
            edits.append(&mut cx.edits);
        }
    });
    let applied = cx::apply(scene, kit, time, edits, &mut mixer.copied);
    if let Some(p) = applied.pick {
        mixer.pick = p;
    }
    applied.keyed
}

/// The effect's settings as they stand at the playhead.
pub fn params(cx: &MixCx, defs: &[SettingDef]) -> Params {
    Params(defs.iter().map(|d| (d.key.to_owned(), cx.get(d.key))).collect())
}

/// Whether `def` applies with the settings as they stand.
fn applies(cx: &MixCx, def: &SettingDef) -> bool {
    def.applies(None, &|k| Some(cx.get(k)))
}

/// A choice's name as the chips show it.
pub fn choice_label(name: &str) -> String {
    match name {
        "bar" => "1 bar".into(),
        "two_bars" => "2 bars".into(),
        "self_input" => "Self".into(),
        "db12" => "12 dB".into(),
        "db24" => "24 dB".into(),
        "db48" => "48 dB".into(),
        "ping_pong" => "Ping-pong".into(),
        "multi_tap" => "Multi-tap".into(),
        "eight" => "8".into(),
        "sixteen" => "16".into(),
        n if n.len() > 1 && n.starts_with('n') && n[1..].chars().all(|c| c.is_ascii_digit()) => {
            format!("1/{}", &n[1..])
        }
        n => widgets::pretty(n),
    }
}

/// A labelled row: a grey name, then `add`.
fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    ui.horizontal(|ui| {
        let l = ui.add_sized([110.0, 22.0], egui::Label::new(RichText::new(label).color(style::MUTED)).truncate());
        add(ui, l.id)
    })
    .inner
}

/// The chips of the choice setting `def`.
pub fn choice_chips(ui: &mut Ui, cx: &mut MixCx, def: &SettingDef, names: &[&str]) {
    let now = match cx.get(def.key) {
        Value::Choice(c) => c,
        _ => String::new(),
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for name in names {
            let on = now == *name;
            let chip = ui.add(style::toggle_button(&choice_label(name), on).min_size(egui::vec2(0.0, 22.0)));
            if chip.clicked() && !on {
                cx.set(def.key, Value::Choice((*name).into()));
            }
        }
    });
}

/// A picker of shapes for the target setting `def` (a Hits sidechain's shape).
pub fn shape_picker(ui: &mut Ui, cx: &mut MixCx, def: &SettingDef, label: egui::Id) {
    let now = match cx.get(def.key) {
        Value::Target(Target::Thing(id)) => Some(id),
        _ => None,
    };
    let shapes: Vec<(ThingId, String)> = cx
        .scene
        .order
        .iter()
        .filter_map(|id| {
            cx.scene.things.get(id).filter(|t| t.kind == bpe_kit::things::shape::KIND).map(|t| (*id, t.name.clone()))
        })
        .collect();
    let shown = now
        .and_then(|id| shapes.iter().find(|(s, _)| *s == id).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| if now.is_some() { "Gone (listens to itself)".into() } else { "Pick a shape".into() });
    let r = ui.menu_button(shown, |ui| {
        if shapes.is_empty() {
            ui.label(RichText::new("There are no shapes yet").color(style::MUTED));
        }
        for (id, name) in &shapes {
            if ui.selectable_label(now == Some(*id), name).clicked() {
                cx.set(def.key, Value::Target(Target::Thing(*id)));
                ui.close();
            }
        }
    });
    let _ = r.response.labelled_by(label);
}

/// Every setting in `defs` that applies, except those in `skip`: the switches and choices in
/// rows, then the numbers as knobs.
pub fn controls(ui: &mut Ui, cx: &mut MixCx, defs: &[SettingDef], skip: &[&str]) {
    let shown: Vec<&SettingDef> = defs.iter().filter(|d| !skip.contains(&d.key) && applies(cx, d)).collect();
    for d in shown.iter().filter(|d| !matches!(d.ty, ValueType::Number | ValueType::Int) || d.key == "sc_bus") {
        match d.ty {
            ValueType::Bool => {
                row(ui, d.label, |ui, _| cx.switch(ui, d.key, d.label));
            }
            ValueType::Choice(names) => row(ui, d.label, |ui, _| choice_chips(ui, cx, d, names)),
            ValueType::Target => row(ui, d.label, |ui, l| shape_picker(ui, cx, d, l)),
            ValueType::Int if d.key == "sc_bus" => row(ui, d.label, |ui, _| {
                let now = cx.number(d.key).round() as i64;
                ui.spacing_mut().item_spacing.x = 2.0;
                for n in 1..=3i64 {
                    let chip =
                        ui.add(style::toggle_button(&format!("Bus {n}"), now == n).min_size(egui::vec2(0.0, 22.0)));
                    if chip.clicked() && now != n {
                        cx.set(d.key, Value::Int(n));
                    }
                }
            }),
            _ => {}
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 6.0);
        for d in shown.iter().filter(|d| matches!(d.ty, ValueType::Number | ValueType::Int) && d.key != "sc_bus") {
            cx.knob_auto(ui, d.key, d.label).on_hover_text(d.help);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::strips::tests::drag;
    use crate::app::{LeftPanel, Studio};
    use crate::mixer::MixerPick;
    use crate::style;
    use bpe_core::{SlotId, ThingId, Value, layered_key};
    use eframe::egui::{self, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    /// The Mixer on the left and the inspector on the right; the state keeps the inspector's
    /// width as drawn.
    fn harness() -> Harness<'static, (Studio, f32)> {
        let mut studio = Studio::headless();
        studio.left = LeftPanel::Mixer;
        let mut h = Harness::builder()
            .with_size(egui::vec2(1600.0, 1000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, width): &mut (Studio, f32)| {
                    let before = studio.scene.clone();
                    studio.recorded = false;
                    studio.left_panel(ui, &before);
                    *width = studio.inspector_panel(ui);
                    egui::CentralPanel::default().show(ui, |_| {});
                    if !std::mem::take(&mut studio.recorded) {
                        studio.after_edit(before);
                    }
                },
                (studio, 0.0),
            );
        h.run();
        h
    }

    /// Adds an EQ to Bus 1 and opens it.
    fn with_eq(h: &mut Harness<'_, (Studio, f32)>) -> bpe_core::LayerId {
        let s = &mut h.state_mut().0;
        let kit = s.kit.clone();
        let id = s.scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, "fx.eq", Some(0)).unwrap();
        h.run();
        h.query_all_by_label("Equalizer").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        id
    }

    fn eq_value(h: &Harness<'_, (Studio, f32)>, id: bpe_core::LayerId, key: &str) -> Option<Value> {
        h.state().0.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().params.get(&layered_key(id, key)).cloned()
    }

    fn number(v: Option<Value>) -> f64 {
        match v {
            Some(Value::Number(x)) => x,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn clicking_a_slot_opens_its_editor_wide() {
        let mut h = harness();
        let narrow = h.state().1;
        let id = with_eq(&mut h);
        assert_eq!(h.state().0.panels.mixer.pick, Some(MixerPick::Layer(id)));
        assert!(h.query_by_label("Bus 1 \u{b7} Equalizer").is_some(), "titled with its bus");
        assert!(h.query_by_label("EQ curve").is_some());
        let wide = h.state().1;
        assert!(wide > narrow + 100.0, "the inspector widens: {narrow} to {wide}");
        h.get_by_label("Close").click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, None);
    }

    #[test]
    fn dragging_an_eq_point_sets_freq_and_gain() {
        let mut h = harness();
        let id = with_eq(&mut h);
        // Band 5 on, a bell at 1 kHz.
        {
            let card = h.state_mut().0.scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
            card.params.set(&layered_key(id, "band5_on"), Value::Bool(true));
        }
        h.run();
        let rect = h.get_by_label("EQ curve").rect();
        let plot = super::eq::Plot { rect };
        drag(&mut h, plot.at(1000.0, 0.0), plot.at(2000.0, 6.0));
        let f = number(eq_value(&h, id, "band5_freq"));
        let g = number(eq_value(&h, id, "band5_gain"));
        assert!((f - 2000.0).abs() < 40.0, "{f}");
        assert!((g - 6.0).abs() < 0.3, "{g}");
        assert_eq!(h.state().0.panels.mixer.eq_band, 4, "the dragged band is picked");
    }

    #[test]
    fn double_click_curve_turns_on_a_band() {
        let mut h = harness();
        let id = with_eq(&mut h);
        let rect = h.get_by_label("EQ curve").rect();
        let plot = super::eq::Plot { rect };
        let at = plot.at(5000.0, -6.0);
        h.hover_at(at);
        // Long enough after the click that opened it, so the two clicks here are a double.
        for _ in 0..30 {
            h.step();
        }
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                h.step();
            }
        }
        h.run();
        // Band 7 starts at 6 kHz: the off band nearest 5 kHz.
        assert_eq!(eq_value(&h, id, "band7_on"), Some(Value::Bool(true)));
        assert!((number(eq_value(&h, id, "band7_freq")) - 5000.0).abs() < 100.0);
        assert!((number(eq_value(&h, id, "band7_gain")) + 6.0).abs() < 0.3);
    }

    #[test]
    fn selecting_a_thing_closes_the_bus_editor() {
        let mut h = harness();
        with_eq(&mut h);
        assert!(h.state().0.panels.mixer.pick.is_some());
        h.state_mut().0.selection.set(ThingId(1));
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, None);
        assert!(h.query_by_label("EQ curve").is_none());
    }

    #[test]
    fn master_editor_shows_the_minus_14_mark() {
        let mut h = harness();
        assert!(h.state().0.scene.card(ThingId::ROOT, SlotId::Mixer).is_some());
        h.query_all_by_label("Master").next().unwrap().click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, Some(MixerPick::Master));
        let graphs = h.query_all_by_label("Loudness graph").count();
        assert_eq!(graphs, 2, "the Mixer's graph and the editor's");
        assert!(h.query_all_by_label_contains("-14 LUFS").count() >= 2);
        // Its knobs edit the mixer card.
        assert!(h.query_all_by_label("Ceiling").filter(|n| n.accesskit_node().role() == Role::Slider).count() >= 2);
    }
}
