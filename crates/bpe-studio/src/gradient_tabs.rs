//! The tabs under a gradient setting's bar, as rows of its settings grid: Colours (the selected
//! stop's colour, place and the gap after it), Blend (how the colours mix, Loop, the Blend tab's
//! companions and every gap's blend at once), and Shape and Move when the gradient has
//! companions on those tabs. A companion (`bpe_core::gradient_extras`) shows as an ordinary
//! settings row under its full key (`"colours~bands"`), so its menu, keys, pulses and Follow
//! sound work like any setting's.
//!
//! The Colours tab ends with the tools (`gradient_tools.rs`); the big editor's window
//! (`gradient_editor.rs`) is drawn from here too, so its Flip can move a value kept per stop.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use bpe_core::gradient_extras::{self, Companion, ExtraTab};
use bpe_core::{Color, GapBlend, Gradient, Mix, SettingDef, Value, ValueType};
use eframe::egui::{self, Ui};

use crate::gradient_bar::{select_stop, selected_stop};
use crate::gradient_tools::{ToolsOutcome, points_after_flip};
use crate::widgets::{GridCx, GridOutcome, color_button, label_cell, widget_cell};
use crate::{eyedropper, gradient_editor, gradient_tools, style};

/// A tab of the gradient row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Colours,
    Extra(ExtraTab),
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Colours => "Colours",
            Self::Extra(t) => t.label(),
        }
    }
}

/// Every gap blend, with its chip's words.
pub(crate) const BLENDS: [(GapBlend, &str); 5] = [
    (GapBlend::Smooth, "Smooth"),
    (GapBlend::Eased, "Eased"),
    (GapBlend::EaseIn, "Ease in"),
    (GapBlend::EaseOut, "Ease out"),
    (GapBlend::Steps, "Steps"),
];

/// Every mix, with its chip's words and help.
pub(crate) const MIXES: [(Mix, &str, &str); 5] = [
    (Mix::Light, "Light", "A straight blend in light: true to how light adds up, can dip grey in the middle"),
    (Mix::Plain, "Plain", "A straight blend of the colours' numbers, like most paint apps"),
    (Mix::Even, "Even", "Brightness changes evenly and the middle never goes muddy"),
    (Mix::RoundTheWheel, "Round the wheel", "Through the hues between, the short way round the colour wheel"),
    (Mix::LongWayRound, "Long way round", "Through the other hues, the long way round the colour wheel"),
];

/// Names the control `r` for the screen reader (and the tests).
pub(crate) fn name_it(ui: &Ui, r: &egui::Response, name: String) {
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(name));
}

/// A chip, lit when `on`, named `name`. True when clicked.
pub(crate) fn chip(ui: &mut Ui, text: &str, on: bool, name: String, help: &str) -> bool {
    let r = ui.add(style::toggle_button(text, on).min_size(egui::vec2(0.0, 22.0)));
    name_it(ui, &r, name);
    r.on_hover_text(help).clicked()
}

/// A muted label in the grid's label column, lined up with the settings' labels.
fn row_label(ui: &mut Ui, cx: &GridCx, text: &str) -> egui::Response {
    crate::widgets::grid_label(ui, cx.label_width, text)
}

/// The tabs Colours and Blend, then Shape and Move when the gradient has companions there.
fn tabs_for(def: &SettingDef) -> Vec<Tab> {
    let mut tabs = vec![Tab::Colours, Tab::Extra(ExtraTab::Blend)];
    for t in [ExtraTab::Shape, ExtraTab::Move] {
        if gradient_extras::companions_for(def).any(|c| c.tab == t) {
            tabs.push(Tab::Extra(t));
        }
    }
    tabs
}

/// The description of companion `c` of the gradient `base`, under its full key
/// (`"colours~bands"`), made once and kept for the program's life like every setting's.
pub fn companion_def(base: &SettingDef, c: &'static Companion) -> &'static SettingDef {
    static MADE: LazyLock<Mutex<HashMap<(&'static str, &'static str), &'static SettingDef>>> =
        LazyLock::new(Default::default);
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    made.entry((base.key, c.name)).or_insert_with(|| {
        let key: &'static str = Box::leak(gradient_extras::key(base.key, c.name).into_boxed_str());
        Box::leak(Box::new(SettingDef { key, ..c.def.clone() }))
    })
}

/// The rows under gradient setting `def`'s bar (the bar's memory is under `bar`): the tab
/// chips, then the open tab's rows. Edits to the gradient itself are made on `value` (true when
/// it changed); a companion row's edits go to `out` like any row's. Nothing for a gradient of
/// fewer than two stops.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tab_rows(
    ui: &mut Ui,
    def: &SettingDef,
    value: &mut Value,
    bar: egui::Id,
    cx: &GridCx,
    right: f32,
    get: &dyn Fn(&str) -> Value,
    out: &mut GridOutcome,
) -> bool {
    let Value::Gradient(g) = value else { return false };
    let mut changed = false;
    // A colour picked from the picture goes on the selected stop, which keeps its opacity.
    if let Some(c) = eyedropper::take(ui.ctx(), bar)
        && !g.stops.is_empty()
    {
        let i = selected_stop(ui.ctx(), bar, g);
        let c = Color { a: g.stops[i].color.a, ..c };
        if g.stops[i].color != c {
            g.stops[i].color = c;
            changed = true;
        }
    }
    // The big editor, while it is open (it stays up if the stops drop to one).
    let tools = gradient_editor::window(ui.ctx(), def.label, bar, g);
    changed |= after_tools(def, tools, get, out);
    if g.stops.len() < 2 {
        return changed;
    }
    let tabs = tabs_for(def);
    let memory = bar.with("tab");
    let picked: Option<usize> = ui.data(|d| d.get_temp(memory));
    let mut tab = picked.and_then(|i| tabs.get(i).copied()).unwrap_or(Tab::Colours);
    label_cell(ui, cx.label_width, |_| {});
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for &t in &tabs {
                if chip(ui, t.label(), tab == t, format!("{} tab {}", def.label, t.label()), "Show these settings") {
                    tab = t;
                }
            }
        });
    });
    ui.end_row();
    if let Some(i) = tabs.iter().position(|&t| t == tab) {
        ui.data_mut(|d| d.insert_temp(memory, i));
    }
    match tab {
        Tab::Colours => {
            changed |= colours_tab(ui, def, g, bar, cx, right);
            row_label(ui, cx, "Tools");
            let tools = widget_cell(ui, right, |ui| gradient_tools::tools(ui, g, bar, def.label));
            ui.end_row();
            changed |= after_tools(def, tools, get, out);
        }
        Tab::Extra(ExtraTab::Blend) => {
            changed |= mix_rows(ui, def, g, cx, right);
            companion_rows(ui, def, ExtraTab::Blend, cx, right, get, out);
            changed |= every_gap(ui, def, g, cx, right);
        }
        Tab::Extra(t) => companion_rows(ui, def, t, cx, right, get, out),
    }
    changed
}

/// What the tools did, finished: after a Flip, each Points companion of `def` (a point per stop)
/// follows its stops into their new order, through `out`. True when the gradient changed.
fn after_tools(def: &SettingDef, tools: ToolsOutcome, get: &dyn Fn(&str) -> Value, out: &mut GridOutcome) -> bool {
    if let Some(order) = &tools.flip {
        for c in gradient_extras::companions_for(def).filter(|c| c.def.ty == ValueType::Points) {
            let key = companion_def(def, c).key;
            if let Some(v) = points_after_flip(&get(key), order) {
                out.changes.push((key, v));
            }
        }
    }
    tools.changed
}

/// The Colours tab: the selected stop's colour, hex and place, then the gap after it (left out
/// on the last stop of a gradient that doesn't loop, which has no gap after it).
fn colours_tab(ui: &mut Ui, def: &SettingDef, g: &mut Gradient, bar: egui::Id, cx: &GridCx, right: f32) -> bool {
    let name = def.label;
    let i = selected_stop(ui.ctx(), bar, g);
    let mut changed = false;
    row_label(ui, cx, &format!("Stop {}", i + 1));
    widget_cell(ui, right, |ui| {
        ui.horizontal(|ui| {
            let mut c = g.stops[i].color;
            let r = color_button(ui, &mut c);
            name_it(ui, &r, format!("{name} stop colour"));
            if r.changed() {
                g.stops[i].color = c;
                changed = true;
            }
            changed |= hex_box(ui, format!("{name} hex"), bar, i, &mut g.stops[i].color);
            let mut at = g.stops[i].at * 100.0;
            let r = ui.add(egui::DragValue::new(&mut at).range(0.0..=100.0).speed(0.25).fixed_decimals(1).suffix("%"));
            name_it(ui, &r, format!("{name} at"));
            let r = r.on_hover_text("Where along the gradient this colour sits");
            if r.changed() {
                let mut stop = g.stops.remove(i);
                stop.at = (at / 100.0).clamp(0.0, 1.0);
                let k = g.stops.partition_point(|s| s.at < stop.at);
                g.stops.insert(k, stop);
                select_stop(ui.ctx(), bar, k);
                changed = true;
            }
        });
    });
    ui.end_row();
    let i = selected_stop(ui.ctx(), bar, g);
    // Only a gap with room has a blend to set (none after the last stop unless it loops round).
    if !crate::gradient_bar::gaps(g).iter().any(|(gap, ..)| *gap == i) {
        return changed;
    }
    row_label(ui, cx, "Gap after");
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (blend, words) in BLENDS {
                let help = "How this gap blends to the next colour";
                if chip(ui, words, g.stops[i].blend == blend, format!("{name} gap {words}"), help)
                    && g.stops[i].blend != blend
                {
                    g.stops[i].blend = blend;
                    changed = true;
                }
            }
        });
    });
    ui.end_row();
    row_label(ui, cx, "Middle");
    widget_cell(ui, right, |ui| {
        let mut m = g.stops[i].middle * 100.0;
        let slider = egui::Slider::new(&mut m, 1.0..=99.0).fixed_decimals(0).suffix("%");
        let r = style::wells(ui, |ui| ui.add(slider));
        name_it(ui, &r, format!("{name} middle"));
        let r = r.on_hover_text("Where the halfway colour of this gap sits (or its hard edge, in Steps)");
        if r.changed() {
            g.stops[i].middle = (m / 100.0).clamp(0.01, 0.99);
            changed = true;
        }
    });
    ui.end_row();
    changed
}

/// Stop `i`'s hex box, named `name`, keeping what is being typed under `bar`: `#rrggbb` or
/// `#rrggbbaa`. Text that isn't a colour leaves the colour as it is. True when it changed the
/// colour.
pub(crate) fn hex_box(ui: &mut Ui, name: String, bar: egui::Id, i: usize, color: &mut Color) -> bool {
    let draft_id = bar.with(("hex", i));
    let edit_id = bar.with(("hex box", i));
    let typing = ui.memory(|m| m.has_focus(edit_id));
    let mut text: String =
        if typing { ui.data(|d| d.get_temp(draft_id)) } else { None }.unwrap_or_else(|| color.to_hex());
    let r =
        ui.add(egui::TextEdit::singleline(&mut text).id(edit_id).desired_width(76.0).font(egui::TextStyle::Monospace));
    name_it(ui, &r, name);
    let r = r.on_hover_text("The colour as a hex code: #rrggbb, or #rrggbbaa with its opacity");
    ui.data_mut(|d| d.insert_temp(draft_id, text.clone()));
    if r.changed()
        && let Some(c) = Color::from_hex(text.trim())
        && c != *color
    {
        *color = c;
        return true;
    }
    false
}

/// The Mix chips, named "{name} mix {Mix}". True when the mix changed.
pub(crate) fn mix_chips(ui: &mut Ui, g: &mut Gradient, name: &str) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (mix, words, help) in MIXES {
            if chip(ui, words, g.mix == mix, format!("{name} mix {words}"), help) && g.mix != mix {
                g.mix = mix;
                changed = true;
            }
        }
    });
    changed
}

/// The Loop switch, named "{name} loop". True when it was switched.
pub(crate) fn loop_switch(ui: &mut Ui, g: &mut Gradient, name: &str) -> bool {
    let mut on = g.looped;
    let r = style::switch(ui, &mut on);
    name_it(ui, &r, format!("{name} loop"));
    let r = r.on_hover_text("Blend from the last colour back round to the first, so it wraps with no cut");
    if r.changed() {
        g.set_looped(on);
        return true;
    }
    false
}

/// The Blend tab's own rows: Mix and Loop.
fn mix_rows(ui: &mut Ui, def: &SettingDef, g: &mut Gradient, cx: &GridCx, right: f32) -> bool {
    let name = def.label;
    let mut changed = false;
    row_label(ui, cx, "Mix");
    widget_cell(ui, right, |ui| changed |= mix_chips(ui, g, name));
    ui.end_row();
    row_label(ui, cx, "Loop");
    widget_cell(ui, right, |ui| changed |= loop_switch(ui, g, name));
    ui.end_row();
    changed
}

/// "Every gap": a blend chip that sets every gap's blend at once (lit when they all have it).
fn every_gap(ui: &mut Ui, def: &SettingDef, g: &mut Gradient, cx: &GridCx, right: f32) -> bool {
    let name = def.label;
    let gaps = if g.looped { g.stops.len() } else { g.stops.len() - 1 };
    let mut changed = false;
    row_label(ui, cx, "Every gap");
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (blend, words) in BLENDS {
                let all = g.stops[..gaps].iter().all(|s| s.blend == blend);
                let help = "Give every gap this blend";
                if chip(ui, words, all, format!("{name} every gap {words}"), help) && !all {
                    for s in &mut g.stops {
                        s.blend = blend;
                    }
                    changed = true;
                }
            }
        });
    });
    ui.end_row();
    changed
}

/// Each companion of gradient `def` on tab `tab`, as an ordinary settings row.
fn companion_rows(
    ui: &mut Ui,
    def: &SettingDef,
    tab: ExtraTab,
    cx: &GridCx,
    right: f32,
    get: &dyn Fn(&str) -> Value,
    out: &mut GridOutcome,
) {
    debug_assert_eq!(def.ty, ValueType::Gradient);
    for c in gradient_extras::companions_for(def).filter(|c| c.tab == tab) {
        crate::widgets::grid_row(ui, companion_def(def, c), get, cx, right, out);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::widgets::{KeyMark, TargetChoices, settings_grid};
    use bpe_core::Color;
    use egui_kittest::{Harness, kittest::Queryable};
    use std::collections::BTreeMap;

    pub(crate) const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    pub(crate) const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };
    pub(crate) const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

    fn a_gradient(key: &'static str, label: &'static str) -> SettingDef {
        SettingDef {
            key,
            label,
            help: "",
            ty: ValueType::Gradient,
            default: Value::Gradient(Gradient::default()),
            range: None,
            span: None,
            unit: "",
            group: "",
            depth: bpe_core::Depth::Simple,
            flags: bpe_core::Flags { keyframe: true, pulse: false, hold: false },
            when: "",
        }
    }

    fn gradient_def() -> &'static [SettingDef] {
        static DEFS: LazyLock<Vec<SettingDef>> = LazyLock::new(|| vec![a_gradient("colours", "Colours")]);
        &DEFS
    }

    /// Two gradient settings, "Colours" and "Edge".
    fn two_gradients() -> &'static [SettingDef] {
        static DEFS: LazyLock<Vec<SettingDef>> =
            LazyLock::new(|| vec![a_gradient("colours", "Colours"), a_gradient("edge", "Edge")]);
        &DEFS
    }

    pub(crate) struct State {
        pub(crate) values: BTreeMap<String, Value>,
        pub(crate) changes: Vec<(String, Value)>,
    }

    impl State {
        pub(crate) fn gradient(&self) -> &Gradient {
            self.gradient_at("colours")
        }

        fn gradient_at(&self, key: &str) -> &Gradient {
            match &self.values[key] {
                Value::Gradient(g) => g,
                _ => panic!(),
            }
        }
    }

    /// One gradient setting in a settings grid; its changes are applied and noted.
    pub(crate) fn harness(g: Gradient) -> Harness<'static, State> {
        let mut values = BTreeMap::new();
        values.insert("colours".to_owned(), Value::Gradient(g));
        harness_with(gradient_def(), values)
    }

    /// The settings `defs` in a settings grid, holding `values`.
    fn harness_with(defs: &'static [SettingDef], values: BTreeMap<String, Value>) -> Harness<'static, State> {
        let state = State { values, changes: Vec::new() };
        let mut h = Harness::builder()
            .with_size(egui::vec2(460.0, 900.0))
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, state: &mut State| {
                    let choices = TargetChoices::default();
                    let marks = |_: &str| Some(KeyMark { can_key: true, keyed: false, pulsed: false });
                    let applies = |_: &SettingDef| true;
                    let cx = GridCx {
                        choices: &choices,
                        marks: &marks,
                        applies: &applies,
                        folded: None,
                        label_width: 108.0,
                        track: style::TEXT,
                        follow: None,
                        vary: None,
                    };
                    let values = state.values.clone();
                    let get = |k: &str| {
                        values.get(k).cloned().unwrap_or_else(|| gradient_extras::default_of(k).expect("a known key"))
                    };
                    let out = settings_grid(ui, "g", defs, get, &cx);
                    for (k, v) in out.changes {
                        state.values.insert(k.to_owned(), v.clone());
                        state.changes.push((k.to_owned(), v));
                    }
                },
                state,
            );
        h.run();
        h
    }

    pub(crate) fn three() -> Gradient {
        Gradient::even(&[RED, GREEN, BLUE])
    }

    pub(crate) fn click(h: &mut Harness<'_, State>, label: &str) {
        h.get_by_label(label).click();
        h.run();
    }

    #[test]
    fn colours_and_blend_tabs_show_and_the_pick_stays() {
        let mut h = harness(three());
        assert!(h.query_by_label("Colours tab Colours").is_some());
        assert!(h.query_by_label("Colours tab Blend").is_some());
        assert!(h.query_by_label("Colours tab Shape").is_none(), "nothing on Shape yet");
        assert!(h.query_by_label("Colours tab Move").is_none());
        assert!(h.query_by_label("Colours hex").is_some(), "Colours is open first");
        click(&mut h, "Colours tab Blend");
        for label in ["Colours mix Even", "Colours loop", "Bands", "Grain", "Colours every gap Steps"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label} shows");
        }
        assert!(h.query_by_label("Colours hex").is_none());
        h.run();
        assert!(h.query_by_label("Colours loop").is_some(), "the pick survives a frame");
    }

    #[test]
    fn a_one_colour_gradient_has_no_tabs() {
        let h = harness(Gradient::solid(RED));
        assert!(h.query_by_label("Colours tab Colours").is_none());
    }

    #[test]
    fn clicking_a_stop_selects_it_and_the_hex_sets_its_colour() {
        let mut h = harness(three());
        click(&mut h, "Colours stop 2");
        // The click opens the picker too; close it.
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.query_by_label("Stop 2").is_some(), "stop 2 is selected");
        h.get_by_label("Colours hex").click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.get_by_label("Colours hex").type_text("#ffff00");
        h.run();
        assert_eq!(h.state().gradient().stops[1].color, Color::rgb(255, 255, 0));
        assert_eq!(h.state().gradient().stops[0].color, RED);
    }

    #[test]
    fn the_gap_chips_and_middle_set_the_selected_stops_gap() {
        let mut h = harness(three());
        click(&mut h, "Colours gap Steps");
        assert_eq!(h.state().gradient().stops[0].blend, GapBlend::Steps);
        assert_eq!(h.state().gradient().stops[1].blend, GapBlend::Smooth);
        let middle = h.get_by_label("Colours middle").rect();
        h.drag_at(middle.center());
        h.run();
        h.hover_at(egui::pos2(middle.left() + 2.0, middle.center().y));
        h.run();
        h.drop_at(egui::pos2(middle.left() + 2.0, middle.center().y));
        h.run();
        let m = h.state().gradient().stops[0].middle;
        assert!((0.01..0.45).contains(&m), "dragged left: {m}");
    }

    #[test]
    fn the_last_stop_of_an_unlooped_gradient_has_no_gap_rows() {
        let mut h = harness(three());
        click(&mut h, "Colours stop 3");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.query_by_label("Stop 3").is_some());
        assert!(h.query_by_label("Colours gap Steps").is_none());
        assert!(h.query_by_label("Colours middle").is_none());
    }

    #[test]
    fn the_blend_tab_sets_mix_loop_bands_and_every_gap() {
        let mut h = harness(three());
        click(&mut h, "Colours tab Blend");
        click(&mut h, "Colours mix Round the wheel");
        assert_eq!(h.state().gradient().mix, Mix::RoundTheWheel);
        click(&mut h, "Colours loop");
        let g = h.state().gradient().clone();
        assert!(g.looped);
        assert!(g.stops.last().unwrap().at < 1.0, "respaced so the last stop isn't on the first: {g:?}");
        click(&mut h, "Colours every gap Steps");
        assert!(h.state().gradient().stops.iter().all(|s| s.blend == GapBlend::Steps));
        // A click on the Bands rail's far end (its span ends at 24).
        let rail = h.query_all_by_label("Bands").find(|n| n.rect().width() > 40.0).unwrap().rect();
        let end = egui::pos2(rail.right() - 1.0, rail.center().y);
        h.drag_at(end);
        h.run();
        h.drop_at(end);
        h.run();
        let bands = h.state().changes.iter().rev().find(|(k, _)| k == "colours~bands").map(|(_, v)| v.clone());
        assert!(matches!(bands, Some(Value::Number(x)) if x > 20.0), "{:?}", h.state().changes);
    }

    #[test]
    fn the_tools_flip_spread_and_roll_the_colours() {
        let mut start = three();
        start.stops[1].at = 0.2;
        start.stops[0].blend = GapBlend::EaseIn;
        let mut h = harness(start.clone());
        click(&mut h, "Colours flip");
        let g = h.state().gradient().clone();
        assert_eq!(g.stops.iter().map(|s| s.color).collect::<Vec<_>>(), vec![BLUE, GREEN, RED]);
        assert!((g.stops[1].at - 0.8).abs() < 1e-6);
        assert_eq!(g.stops[1].blend, GapBlend::EaseOut, "the old first gap, mirrored");
        click(&mut h, "Colours spread evenly");
        let ats: Vec<f32> = h.state().gradient().stops.iter().map(|s| s.at).collect();
        assert_eq!(ats, vec![0.0, 0.5, 1.0]);
        // Looped, spread evenly goes round the loop.
        h.state_mut().values.insert("colours".into(), Value::Gradient(Gradient { looped: true, ..start.clone() }));
        h.run();
        click(&mut h, "Colours spread evenly");
        let ats: Vec<f32> = h.state().gradient().stops.iter().map(|s| s.at).collect();
        assert!(ats.iter().zip([0.0, 1.0 / 3.0, 2.0 / 3.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{ats:?}");
        let before = h.state().gradient().clone();
        click(&mut h, "Colours random");
        let after = h.state().gradient().clone();
        assert_ne!(after.stops, before.stops, "new colours");
        for (a, b) in after.stops.iter().zip(&before.stops) {
            assert_eq!((a.at, a.blend, a.middle, a.color.a), (b.at, b.blend, b.middle, b.color.a));
        }
        assert!(after.looped);
    }

    #[test]
    fn pick_from_picture_puts_the_picked_colour_on_the_selected_stop() {
        let mut start = three();
        start.stops[1].color.a = 100;
        let mut h = harness(start);
        click(&mut h, "Colours stop 2");
        h.key_press(egui::Key::Escape);
        h.run();
        click(&mut h, "Colours pick from picture");
        assert!(eyedropper::target(&h.ctx).is_some(), "picking");
        eyedropper::deliver(&h.ctx, Color::rgb(10, 20, 30));
        h.run();
        let g = h.state().gradient().clone();
        assert_eq!(g.stops[1].color, Color { r: 10, g: 20, b: 30, a: 100 }, "opacity kept");
        assert_eq!(g.stops[0].color, RED);
        assert!(eyedropper::target(&h.ctx).is_none(), "one pick");
        // Clicking it again while picking stops.
        click(&mut h, "Colours pick from picture");
        click(&mut h, "Colours pick from picture");
        assert!(eyedropper::target(&h.ctx).is_none());
    }

    #[test]
    fn a_gradient_copied_with_its_settings_pastes_them_too() {
        let mut values = BTreeMap::new();
        values.insert("colours".to_owned(), Value::Gradient(three()));
        values.insert("colours~bands".to_owned(), Value::Number(5.0));
        values.insert("edge".to_owned(), Value::Gradient(Gradient::solid(RED)));
        let mut h = harness_with(two_gradients(), values);
        h.get_by_label("Colours").click_secondary();
        h.run();
        click(&mut h, "Copy gradient with its settings");
        h.get_by_label("Edge").click_secondary();
        h.run();
        click(&mut h, "Paste Value (3 colours with its settings)");
        assert_eq!(h.state().gradient_at("edge"), &three());
        assert_eq!(h.state().values.get("edge~bands"), Some(&Value::Number(5.0)), "{:?}", h.state().changes);
        // A plain Copy Value pastes the colours only.
        h.state_mut().values.insert("colours~bands".into(), Value::Number(9.0));
        h.state_mut().values.insert("edge".into(), Value::Gradient(Gradient::solid(BLUE)));
        h.run();
        h.get_by_label("Colours").click_secondary();
        h.run();
        click(&mut h, "Copy Value");
        h.get_by_label("Edge").click_secondary();
        h.run();
        click(&mut h, "Paste Value (3 colours)");
        assert_eq!(h.state().gradient_at("edge"), &three());
        assert_eq!(h.state().values.get("edge~bands"), Some(&Value::Number(5.0)), "left as it was");
    }
}
