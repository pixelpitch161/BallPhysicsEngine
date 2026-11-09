//! The inspector for a rule block: what it is, what is wrong with it, and its settings, with the
//! who picker, Change setting's setting menu and numbers that can come from the firing.

use bpe_core::rules::{FromPart, from_def, from_key, plus_key, times_key};
use bpe_core::{BlockId, BlockSection, Registry, Rule, RuleId, Scene, SettingDef, Settings, Value, ValueType};
use bpe_kit::Kit;
use bpe_kit::rules::change::ChangeSettingSettings;
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeSet;

use super::face::value_name;
use super::setting_menu;
use crate::style;
use crate::widgets::{
    GridCx, GridOutcome, TargetChoices, grid_label, grid_row, grid_row_with, label_width, settings_like_grid,
    widget_cell,
};

/// The values a Compare reads of a thing (Of: A thing).
const THING_VALUES: [&str; 8] = ["speed", "size", "hits", "ball", "time", "health", "share", "start"];

/// Every value any When kind's firings carry, in registry order.
pub fn all_values(reg: &Registry) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for k in reg.blocks().filter(|k| k.section() == BlockSection::When) {
        for v in k.values() {
            if !out.contains(v) {
                out.push(v);
            }
        }
    }
    out
}

/// The values the firings reaching `block` carry: those of the When blocks it is fed from, walked
/// back through its inputs. Every known value when none is known.
pub fn upstream_values(reg: &Registry, rule: &Rule, block: BlockId) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut todo: Vec<BlockId> = rule.block(block).map(|b| b.inputs.clone()).unwrap_or_default();
    while let Some(at) = todo.pop() {
        if !seen.insert(at) {
            continue;
        }
        let Some(b) = rule.block(at) else { continue };
        if let Some(k) = reg.block(&b.kind) {
            for v in k.values() {
                if !out.contains(v) {
                    out.push(v);
                }
            }
        }
        todo.extend(b.inputs.iter().copied());
    }
    if out.is_empty() { all_values(reg) } else { out }
}

/// A menu of value names, showing `now`. The name picked, if any.
fn value_picker(ui: &mut Ui, now: &str, offered: &[&'static str], name: egui::Id) -> Option<String> {
    let mut picked = None;
    let words = if now.is_empty() { "Pick a value".to_owned() } else { value_name(now) };
    let menu = ui.menu_button(words, |ui| {
        ui.set_min_width(140.0);
        for &v in offered {
            if ui.selectable_label(v == now, value_name(v)).clicked() {
                picked = Some(v.to_owned());
                ui.close();
            }
        }
    });
    menu.response.labelled_by(name).on_hover_text("Which of the firing's values this reads");
    picked
}

/// The chip that turns a number's "from the firing" on or off. True when clicked.
fn from_chip(ui: &mut Ui, label: &str, on: bool) -> bool {
    let r = ui.add(style::toggle_button("from", on));
    let words = format!("{label} from the firing");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
    let hint = if on {
        "This number comes from the firing. Click to set it by hand again."
    } else {
        "Take this number from the firing: the hit's strength, the ball's speed…"
    };
    r.on_hover_text(hint).clicked()
}

/// Change setting's To, as the picked setting itself shows (its span, range and unit; a choice as
/// its choices), or none when this To isn't the picked setting's type.
fn change_def(def: &SettingDef, target: Option<&'static SettingDef>) -> Option<SettingDef> {
    let t = target?;
    let mine = matches!(
        (&t.ty, def.key),
        (ValueType::Number | ValueType::Int, "to_number")
            | (ValueType::Vec2, "to_pair")
            | (ValueType::Bool, "to_switch")
            | (ValueType::Color, "to_colour")
            | (ValueType::Gradient, "to_gradient")
            | (ValueType::Choice(_), "to_choice")
    );
    if !mine {
        return None;
    }
    let mut d = def.clone();
    d.help = "What the setting becomes (or what is added, for Add)";
    match &t.ty {
        ValueType::Number | ValueType::Int => {
            d.span = t.span.or(t.range);
            d.unit = t.unit;
        }
        ValueType::Vec2 => d.unit = t.unit,
        ValueType::Choice(_) => {
            d.ty = t.ty;
            d.default = t.default.clone();
        }
        _ => {}
    }
    Some(d)
}

/// Shows the block `(rule, block)`; edits go straight into `scene`. `runtime` is what the running
/// rules say is wrong with the rule, if anything.
pub fn show(ui: &mut Ui, scene: &mut Scene, kit: &Kit, (rule_id, block_id): (RuleId, BlockId), runtime: Option<&str>) {
    let reg = &kit.registry;
    let panel = ui.available_width();
    ui.set_max_width(panel);
    let lw = label_width(panel);
    let Some(rule) = scene.rules.rule(rule_id).cloned() else { return };
    let Some(block) = rule.block(block_id).cloned() else { return };
    let margin = egui::Margin { left: 10, right: 10, top: 6, bottom: 6 };
    let Some(kind) = reg.block(&block.kind) else {
        egui::Frame::new().inner_margin(margin).show(ui, |ui| {
            let words = format!("This version of Ball Studio doesn't know the block \"{}\".", block.kind);
            ui.label(RichText::new(words).color(style::TEXT));
        });
        return;
    };
    let section = kind.section();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(margin).show(ui, |ui| {
            ui.label(RichText::new(format!("{} · {}", section.name(), kind.name())).font(style::semibold(14.0)));
            ui.label(RichText::new(kind.help()).color(style::MUTED));
            ui.label(RichText::new(format!("In the rule \"{}\"", rule.name)).small().color(style::FAINT));
            let mut problems: Vec<String> = scene
                .rule_problems(reg, rule_id)
                .iter()
                .filter(|p| p.block.is_none_or(|b| b == block_id))
                .map(|p| p.words())
                .collect();
            if let Some(w) = runtime.filter(|w| !problems.iter().any(|p| p == w)) {
                problems.push(w.to_owned());
            }
            for p in problems {
                ui.label(RichText::new(p).color(style::PROBLEM));
            }
        });
        style::divider(ui);
        egui::Frame::new().inner_margin(margin).show(ui, |ui| {
            let changes = settings(ui, scene, kit, &rule, block_id, lw);
            for (key, value) in changes {
                let _ = scene.set_block_param(reg, rule_id, block_id, &key, value);
            }
        });
    });
}

/// The block's settings grid; the changes asked for, by key.
fn settings(ui: &mut Ui, scene: &Scene, kit: &Kit, rule: &Rule, block_id: BlockId, lw: f32) -> Vec<(String, Value)> {
    let reg = &kit.registry;
    let block = rule.block(block_id).expect("the caller found it");
    let kind = reg.block(&block.kind).expect("the caller found it");
    let mut choices = TargetChoices::from_scene(scene, reg);
    choices.relative = kind.section() != BlockSection::When;
    let fixed = kind.settings();
    let default_of = |k: &str| fixed.iter().find(|d| d.key == k).map(|d| d.default.clone());
    let get_opt = |k: &str| block.params.get(k).cloned().or_else(|| default_of(k));
    let mut defs: Vec<SettingDef> = fixed.to_vec();
    defs.extend(kind.dynamic_defs(&get_opt).iter().cloned());
    let get = |k: &str| -> Value {
        get_opt(k)
            .or_else(|| bpe_core::rules::block_def(kind, &get_opt, k).map(|d| d.default.clone()))
            .unwrap_or(Value::Number(0.0))
    };
    let change = kind.id() == "do.change";
    let compare = kind.id() == "if.compare";
    let s = change.then(|| ChangeSettingSettings::from_params(&block.params));
    let target =
        s.as_ref().and_then(|s| setting_menu::picked_def(scene, kit, &s.who, s.card, s.setting.trim()).map(|(_, d)| d));
    let numeric = target.is_some_and(|t| matches!(t.ty, ValueType::Number | ValueType::Int | ValueType::Vec2));
    let multiply = matches!(get("how"), Value::Choice(c) if c == "multiply");
    // Change setting's rows: To only as the picked setting's type, How only for numbers.
    let shown: Vec<SettingDef> = defs
        .into_iter()
        .filter_map(|d| {
            if !change {
                return Some(d);
            }
            match d.key {
                "setting" => None,
                "how" | "times" if !numeric => None,
                // Multiply uses Times, not To.
                "to_number" | "to_pair" if multiply => None,
                k if k.starts_with("to_") => change_def(&d, target),
                _ => Some(d),
            }
        })
        .filter(|d| change && d.key == "card" || d.applies(None, &get_opt))
        .collect();
    let upstream = upstream_values(reg, rule, block_id);
    let marks = |_: &str| None;
    let applies = |_: &SettingDef| true;
    let cx = GridCx {
        choices: &choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width: lw,
        track: style::MUTED,
        follow: None,
        vary: None,
        offers: None,
        rails: None,
    };
    let right = ui.available_rect_before_wrap().right();
    let mut out: Vec<(String, Value)> = Vec::new();
    // The picked setting's choice is saved as text.
    let get_shown = |k: &str| -> Value {
        match (k, target.map(|t| &t.ty)) {
            ("to_choice", Some(ValueType::Choice(names))) => match get(k) {
                Value::Text(t) if names.contains(&t.as_str()) => Value::Choice(t),
                _ => target.map(|t| t.default.clone()).unwrap_or(Value::Text(String::new())),
            },
            _ => get(k),
        }
    };
    settings_like_grid(("rule block", rule.id, block_id)).show(ui, |ui| {
        for def in &shown {
            let fromable = kind.firing_keys().contains(&def.key);
            let from_now = match get(&from_key(def.key)) {
                Value::Text(t) if fromable && !t.is_empty() => Some(t),
                _ => None,
            };
            if change && def.key == "card" {
                // Card and Setting are one row with the setting menu.
                grid_label(ui, lw, "Setting");
                let s = s.as_ref().expect("a Change setting");
                widget_cell(ui, right, |ui| {
                    if let Some((card, key)) =
                        setting_menu::setting_menu(ui, scene, kit, &s.who, (s.card, s.setting.trim()))
                    {
                        // The new setting starts at its own default choice, and a setting that
                        // isn't a number can only be set.
                        let picked = setting_menu::picked_def(scene, kit, &s.who, card, &key).map(|(_, d)| d);
                        let to = match picked.map(|d| (&d.ty, &d.default)) {
                            Some((ValueType::Choice(_), Value::Choice(c))) => c.clone(),
                            _ => String::new(),
                        };
                        let number = picked
                            .is_some_and(|d| matches!(d.ty, ValueType::Number | ValueType::Int | ValueType::Vec2));
                        if !number {
                            out.push(("how".into(), Value::Choice("set".into())));
                        }
                        out.push(("to_choice".into(), Value::Text(to)));
                        out.push(("card".into(), setting_menu::card_value(card)));
                        out.push(("setting".into(), Value::Text(key)));
                    }
                });
                ui.end_row();
                continue;
            }
            if compare && def.key == "value" {
                let name = grid_label(ui, lw, def.label);
                let of_thing = matches!(get("of"), Value::Choice(c) if c == "a_thing");
                let offered: Vec<&'static str> = if of_thing { THING_VALUES.to_vec() } else { upstream.clone() };
                let now = match get("value") {
                    Value::Text(t) => t,
                    _ => String::new(),
                };
                widget_cell(ui, right, |ui| {
                    if let Some(v) = value_picker(ui, &now, &offered, name.id) {
                        out.push(("value".into(), Value::Text(v)));
                    }
                });
                ui.end_row();
                continue;
            }
            if let Some(now) = from_now {
                // From the firing: the value's name and the chip, then its factor and offset.
                let name = grid_label(ui, lw, def.label);
                widget_cell(ui, right, |ui| {
                    let room = (ui.available_width() - crate::widgets::TRAILING_W).max(40.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(room, 0.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if let Some(v) = value_picker(ui, &now, &upstream, name.id) {
                                out.push((from_key(def.key), Value::Text(v)));
                            }
                        },
                    );
                    if from_chip(ui, def.label, true) {
                        out.push((from_key(def.key), Value::Text(String::new())));
                    }
                });
                ui.end_row();
                for (part, key) in [(FromPart::Times, times_key(def.key)), (FromPart::Plus, plus_key(def.key))] {
                    let Some(part_def) = from_def(Some(def_static(kind, def.key)), part) else { continue };
                    let read = |k: &str| if k == part_def.key { get(&key) } else { get(k) };
                    let mut o = GridOutcome::default();
                    grid_row(ui, part_def, &read, &cx, right, &mut o);
                    out.extend(o.changes.into_iter().map(|(_, v)| (key.clone(), v)));
                }
                continue;
            }
            let mut o = GridOutcome::default();
            if fromable {
                let first = upstream.first().copied().unwrap_or("strength");
                let mut chip = |ui: &mut Ui| {
                    if from_chip(ui, def.label, false) {
                        out.push((from_key(def.key), Value::Text(first.to_owned())));
                    }
                };
                grid_row_with(ui, def, &get_shown, &cx, right, &mut o, Some(&mut chip));
            } else {
                grid_row(ui, def, &get_shown, &cx, right, &mut o);
            }
            for (key, v) in o.changes {
                // Compare's Value is one the new Of offers.
                if compare && key == "of" {
                    let of_thing = matches!(&v, Value::Choice(c) if c == "a_thing");
                    let first = if of_thing { THING_VALUES.first() } else { upstream.first() };
                    out.push(("value".into(), Value::Text(first.copied().unwrap_or_default().to_owned())));
                }
                let v = match (key, v) {
                    ("to_choice", Value::Choice(c)) => Value::Text(c),
                    (_, v) => v,
                };
                out.push((key.to_owned(), v));
            }
        }
    });
    out
}

/// A fixed setting of `kind` by key, as a `'static` description (from-the-firing parts are
/// described through it).
fn def_static(kind: &dyn bpe_core::RuleBlockKind, key: &str) -> &'static SettingDef {
    kind.settings().iter().find(|d| d.key == key).expect("from-keys are fixed settings")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_values_walk_back_to_the_whens() {
        let kit = bpe_sound::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_scene(reg);
        let r = scene.add_rule("R");
        let when = scene.add_block(reg, r, "when.time", None).unwrap();
        let check = scene.add_block(reg, r, "if.chance", Some(when)).unwrap();
        let act = scene.add_block(reg, r, "do.motion", Some(check)).unwrap();
        let rule = scene.rules.rule(r).unwrap().clone();
        assert_eq!(upstream_values(reg, &rule, act), vec!["time", "count"]);
        // A block fed by nothing knows every value.
        let lone = scene.add_block(reg, r, "do.motion", None).unwrap();
        let rule = scene.rules.rule(r).unwrap().clone();
        assert!(upstream_values(reg, &rule, lone).contains(&"strength"));
    }
}
