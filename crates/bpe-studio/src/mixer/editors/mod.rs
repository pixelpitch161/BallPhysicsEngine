//! The Mixer's editors, drawn in the Mixer under the strips' tops: a bus effect's (picked in its
//! strip) or the Master's. Each effect kind has its own view where one helps (the EQ's curve, the
//! compressor's level graph, the delay's echoes, the reverb's tail, the filter's response and
//! the distortion's curve); the rest of an effect's settings are drawn from its settings list.

pub mod comp;
pub mod delay;
pub mod eq;
pub mod master;
pub mod reverb;
pub mod shaped;

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
    let mut master_keyed = false;
    let choices = TargetChoices::default();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").on_hover_text("Close this editor: the sends and pans show again").clicked() {
                edits.push(MixEdit::Pick(None));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let title = RichText::new(title(scene, kit, pick)).font(style::semibold(15.0)).color(style::TEXT);
                ui.add(egui::Label::new(title).truncate());
            });
        });
    });
    // (The Mixer around it scrolls.)
    ui.scope(|ui| match pick {
        MixerPick::Master => {
            master_keyed = master::show(ui, scene, kit, &readings.master, &mixer.loudness, time);
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
                fx::FILTER | fx::DISTORTION => shaped::show(ui, &mut cx, &layer.kind, defs),
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
    applied.keyed || master_keyed
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
        "x1" | "x2" | "x4" | "x8" => format!("{}x", &name[1..]),
        n if n.len() > 1 && n.starts_with('n') && n[1..].chars().all(|c| c.is_ascii_digit()) => {
            format!("1/{}", &n[1..])
        }
        n => widgets::pretty(n),
    }
}

/// A labelled row: a grey name, then `add`. The name takes less room in a narrow editor.
fn row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    let name_w = (ui.available_width() * 0.3).clamp(60.0, 110.0);
    ui.horizontal(|ui| {
        let l = ui.add_sized([name_w, 22.0], egui::Label::new(RichText::new(label).color(style::MUTED)).truncate());
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
            // Shapes and Texts: what balls hit.
            cx.scene
                .things
                .get(id)
                .filter(|t| t.kind == bpe_kit::things::shape::KIND || t.kind == bpe_kit::things::text::KIND)
                .map(|t| (*id, t.name.clone()))
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

    /// The Mixer on the left and the inspector on the right; the state keeps the left panel's
    /// and the inspector's widths as drawn.
    fn harness() -> Harness<'static, (Studio, [f32; 2])> {
        let mut studio = Studio::headless();
        studio.left = LeftPanel::Mixer;
        let mut h = Harness::builder()
            .with_size(egui::vec2(1600.0, 1000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, widths): &mut (Studio, [f32; 2])| {
                    let before = studio.scene.clone();
                    studio.recorded = false;
                    widths[0] = studio.left_panel(ui, &before);
                    widths[1] = studio.inspector_panel(ui);
                    egui::CentralPanel::default().show(ui, |_| {});
                    if !std::mem::take(&mut studio.recorded) {
                        studio.after_edit(before);
                    }
                },
                (studio, [0.0; 2]),
            );
        h.run();
        h
    }

    /// Adds an EQ to Bus 1 and opens it.
    fn with_eq(h: &mut Harness<'_, (Studio, [f32; 2])>) -> bpe_core::LayerId {
        let s = &mut h.state_mut().0;
        let kit = s.kit.clone();
        let id = s.scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, "fx.eq", Some(0)).unwrap();
        h.run();
        h.query_all_by_label("Equalizer").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        id
    }

    fn eq_value(h: &Harness<'_, (Studio, [f32; 2])>, id: bpe_core::LayerId, key: &str) -> Option<Value> {
        h.state().0.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().params.get(&layered_key(id, key)).cloned()
    }

    fn number(v: Option<Value>) -> f64 {
        match v {
            Some(Value::Number(x)) => x,
            other => panic!("{other:?}"),
        }
    }

    /// How many send knobs show.
    fn sends(h: &Harness<'_, (Studio, [f32; 2])>) -> usize {
        ["To 1", "To 2", "To 3"]
            .iter()
            .map(|name| h.query_all_by_label(name).filter(|n| n.accesskit_node().role() == Role::Slider).count())
            .sum()
    }

    #[test]
    fn clicking_a_slot_opens_its_editor_in_the_mixer() {
        let mut h = harness();
        let [left, inspector] = h.state().1;
        assert_eq!(sends(&h), 6, "the sends show while nothing is open");
        let id = with_eq(&mut h);
        assert_eq!(h.state().0.panels.mixer.pick, Some(MixerPick::Layer(id)));
        assert!(h.query_by_label("Bus 1 \u{b7} Equalizer").is_some(), "titled with its bus");
        let curve = h.get_by_label("EQ curve").rect();
        assert!(curve.right() <= left + 0.5, "the curve is in the Mixer ({curve:?}, panel {left} wide)");
        // Across the three bus columns, under their tops and over their faders.
        let bus1 = h.get_by_label("Bus 1").rect();
        let bus3 = h.query_all_by_label("Bus 3").next().unwrap().rect();
        assert!(curve.left() <= bus1.left() + 12.0 && curve.right() > bus3.left(), "{curve:?}");
        let slots = h.get_by_label("Add to Bus 1").rect();
        let fader = h.query_all_by_label("Bus 1 fader").next().unwrap().rect();
        assert!(slots.bottom() < curve.top() && curve.bottom() < fader.top(), "{slots:?} {curve:?} {fader:?}");
        assert_eq!(sends(&h), 0, "the editor takes the sends' place");
        assert!(h.query_all_by_label("Ceiling").next().is_some(), "the Master keeps its knobs");
        assert_eq!(h.state().1[1], inspector, "the inspector stays as it was");
        h.get_by_label("Close").click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, None);
        assert!(h.query_by_label("EQ curve").is_none());
        assert_eq!(sends(&h), 6, "the sends are back");
    }

    #[test]
    fn clicking_the_open_slot_again_closes_it() {
        let mut h = harness();
        with_eq(&mut h);
        assert!(h.query_by_label("EQ curve").is_some());
        // Long enough after the click that opened it, so this is not a double-click.
        for _ in 0..30 {
            h.step();
        }
        h.query_all_by_label("Equalizer").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, None);
        assert!(h.query_by_label("EQ curve").is_none());
        // And the Master's title opens and closes the Master's editor.
        h.query_all_by_label("Master").next().unwrap().click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, Some(MixerPick::Master));
        for _ in 0..30 {
            h.step();
        }
        h.query_all_by_label("Master").next().unwrap().click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, None);
    }

    fn set_bus_on(h: &mut Harness<'_, (Studio, [f32; 2])>, n: u8) {
        let card = h.state_mut().0.scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
        card.params.set(&format!("bus{n}_on"), Value::Bool(true));
        h.run();
    }

    #[test]
    fn hovering_rings_the_bus_under_the_pointer() {
        let mut h = harness();
        set_bus_on(&mut h, 2);
        let s = &mut h.state_mut().0;
        let kit = s.kit.clone();
        s.scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, "fx.reverb", Some(1)).unwrap();
        h.run();
        h.query_all_by_label("Reverb").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        // Over Bus 1's column, but the editor is Bus 2's.
        let tail = h.get_by_label("Reverb tail").rect();
        h.hover_at(egui::pos2(tail.left() + 4.0, tail.center().y));
        h.run();
        assert_eq!(h.state().0.panels.mixer.hover_bus, Some(2));
        let fader = h.query_all_by_label("Bus 3 fader").next().unwrap().rect();
        h.hover_at(fader.center());
        h.run();
        assert_eq!(h.state().0.panels.mixer.hover_bus, Some(3));
        let name = h.get_by_label("Bus 1").rect();
        h.hover_at(name.center());
        h.run();
        assert_eq!(h.state().0.panels.mixer.hover_bus, Some(1));
    }

    /// Every editor draws inside a Mixer as wide as the tree's default.
    #[test]
    fn every_editor_fits_the_mixer() {
        let mut h = harness();
        let left = h.state().1[0];
        assert!((left - style::TREE_W).abs() < 1.0, "{left}");
        let s = &mut h.state_mut().0;
        let kit = s.kit.clone();
        for kind in bpe_kit::sound::fx::FX_KINDS {
            s.scene.add_layer_in(&kit.registry, ThingId::ROOT, SlotId::Mixer, kind, Some(0)).unwrap();
        }
        let ids: Vec<_> = s.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().layers.iter().map(|l| l.id).collect();
        h.run();
        let picks = ids.into_iter().map(MixerPick::Layer).chain([MixerPick::Master]);
        let master = h.query_all_by_label("Master").next().unwrap().rect().left();
        for pick in picks {
            h.state_mut().0.panels.mixer.pick = Some(pick);
            h.run();
            let close = h.get_by_label("Close").rect();
            assert!(close.right() <= left, "{pick:?}: Close is in the Mixer ({close:?})");
            assert_eq!(h.state().1[0], left, "{pick:?}: the Mixer keeps its width");
            // Everything between the editor's title and the faders stays left of the Master.
            let faders = h.query_all_by_label("Bus 1 fader").next().unwrap().rect().top();
            for node in h.root().children_recursive() {
                let a = node.accesskit_node();
                let (Some(name), Some(b)) = (a.label().filter(|l| !l.is_empty()), a.bounding_box()) else { continue };
                if b.y0 as f32 >= close.top() && b.y1 as f32 <= faders {
                    assert!(b.x1 as f32 <= master, "{pick:?}: {name} reaches {} past the Master at {master}", b.x1);
                }
            }
        }
        // The Master's readout keeps its words on one line (not squeezed beside the knobs).
        let down = h.query_all_by_label("Turned down").next().unwrap().rect();
        assert!(down.width() > down.height() * 2.0, "{down:?}");
    }

    #[test]
    fn bus_filter_and_distortion_show_their_pictures() {
        let mut h = harness();
        let s = &mut h.state_mut().0;
        let kit = s.kit.clone();
        let reg = &kit.registry;
        s.scene.add_layer_in(reg, ThingId::ROOT, SlotId::Mixer, "fx.filter", Some(0)).unwrap();
        let dist = s.scene.add_layer_in(reg, ThingId::ROOT, SlotId::Mixer, "fx.distortion", Some(0)).unwrap();
        h.run();
        h.query_all_by_label("Filter").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        assert!(h.query_by_label("Filter response").is_some());
        assert!(h.query_by_label("Cutoff").is_some(), "its knobs");
        assert!(h.query_by_label("Key track").is_none(), "a bus has no note to track");
        h.query_all_by_label("Distortion").find(|n| n.accesskit_node().role() == Role::Button).unwrap().click();
        h.run();
        assert_eq!(h.state().0.panels.mixer.pick, Some(MixerPick::Layer(dist)));
        assert!(h.query_by_label("Distortion curve").is_some());
        assert!(h.query_by_label("4x").is_some(), "Quality chips");
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
    fn selecting_a_thing_leaves_the_bus_editor_open() {
        let mut h = harness();
        with_eq(&mut h);
        assert!(h.state().0.panels.mixer.pick.is_some());
        h.state_mut().0.selection.set(ThingId(1));
        h.run();
        assert!(h.state().0.panels.mixer.pick.is_some(), "the editor is in the Mixer, beside the inspector");
        assert!(h.query_by_label("EQ curve").is_some());
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
