//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.

use bpe_core::{Color, Depth, SettingDef, Value, ValueType};
use eframe::egui::{self, Color32, DragValue, Response, RichText, Sense, Slider, Ui, color_picker::Alpha};

/// `electric_piano` to "Electric piano".
pub fn pretty(name: &str) -> String {
    let text = name.replace('_', " ");
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// One row of a settings grid: a setting, or a heading over the group that follows.
pub enum Row<'a> {
    Group(&'a str),
    Setting(&'a SettingDef),
}

/// The rows a settings grid shows at `depth`: the simple settings alone, or every setting with
/// the grouped ones under their headings (ungrouped first, then groups in the order they appear).
pub fn rows_at(defs: &[SettingDef], depth: Depth) -> Vec<Row<'_>> {
    if depth == Depth::Simple {
        return defs.iter().filter(|d| d.depth == Depth::Simple).map(Row::Setting).collect();
    }
    let mut rows: Vec<Row<'_>> = defs.iter().filter(|d| d.group.is_empty()).map(Row::Setting).collect();
    let mut groups: Vec<&str> = Vec::new();
    for d in defs {
        if !d.group.is_empty() && !groups.contains(&d.group) {
            groups.push(d.group);
        }
    }
    for group in groups {
        rows.push(Row::Group(group));
        rows.extend(defs.iter().filter(|d| d.group == group).map(Row::Setting));
    }
    rows
}

/// A settings grid at `depth`. `get` supplies each setting's value as it stands; the rows the
/// user changed come back as (key, new value) for the caller to write through `set_at`.
pub fn settings_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    depth: Depth,
    get: impl Fn(&str) -> Value,
) -> Vec<(&'static str, Value)> {
    let mut changes = Vec::new();
    egui::Grid::new(id).num_columns(2).spacing([12.0, 6.0]).min_col_width(96.0).show(ui, |ui| {
        for row in rows_at(defs, depth) {
            match row {
                Row::Group(group) => {
                    ui.add_space(4.0);
                    ui.label(RichText::new(group).strong().color(crate::theme::MUTED));
                    ui.end_row();
                }
                Row::Setting(def) => {
                    let mut value = get(def.key);
                    if setting(ui, def, &mut value, None) {
                        changes.push((def.key, value));
                    }
                }
            }
        }
    });
    changes
}

/// One grid row for `def`: the label (help on hover; a double-click or Alt+click puts the value
/// back to `inherited`, or the setting's default) and the widget for its type. True when
/// `value` changed.
pub fn setting(ui: &mut Ui, def: &SettingDef, value: &mut Value, inherited: Option<&Value>) -> bool {
    let fallback = inherited.cloned().unwrap_or_else(|| def.default.clone());
    let name = label(ui, def.label, def.help);
    let mut changed = false;
    let response = ui
        .horizontal(|ui| {
            let r = widget(ui, def, value, name.id);
            changed |= r.changed();
            r
        })
        .inner;
    if resettable(&name, value, &fallback) || resettable(&response, value, &fallback) {
        changed = true;
    }
    ui.end_row();
    changed
}

/// The label cell, clickable for the reset shortcut.
fn label(ui: &mut Ui, text: &str, help: &str) -> Response {
    let response = ui.add(egui::Label::new(text).selectable(false).sense(Sense::click()));
    if help.is_empty() { response } else { response.on_hover_text(help) }
}

/// Whether the response was double-clicked or Alt+clicked, which puts a setting back.
fn reset_clicked(response: &Response) -> bool {
    let primary = egui::PointerButton::Primary;
    // egui counts a double-click that comes soon after a click elsewhere as a triple-click.
    let (alt, clicked, many_clicks) = response.ctx.input(|i| {
        let many = i.pointer.button_double_clicked(primary) || i.pointer.button_triple_clicked(primary);
        (i.modifiers.alt, i.pointer.button_clicked(primary), many)
    });
    // Sliders only sense drags, so a click on one shows up as a drag that stops where it started.
    let clicked = response.clicked() || (response.drag_stopped() && clicked);
    (clicked && alt) || (clicked && many_clicks && !response.ctx.text_edit_focused())
}

/// Puts `value` back to `fallback` on a double-click or Alt+click. True when it changed.
fn resettable(response: &Response, value: &mut Value, fallback: &Value) -> bool {
    if reset_clicked(response) && value != fallback {
        *value = fallback.clone();
        true
    } else {
        false
    }
}

fn suffix(unit: &str) -> String {
    if unit.is_empty() { String::new() } else { format!(" {unit}") }
}

/// The widget for `def`'s type, editing `value` in place. Every control is labelled by the
/// row's name, for screen readers and tests.
fn widget(ui: &mut Ui, def: &SettingDef, value: &mut Value, name: egui::Id) -> Response {
    let unit = suffix(def.unit);
    match (&def.ty, value) {
        (ValueType::Number, Value::Number(x)) => match def.range {
            Some((min, max)) => {
                let mut slider = Slider::new(x, min..=max).suffix(unit);
                // A range spanning more than a thousandfold is easier to use on a log scale.
                if min > 0.0 && max / min > 1000.0 {
                    slider = slider.logarithmic(true);
                }
                ui.add(slider).labelled_by(name)
            }
            None => ui.add(DragValue::new(x).speed(0.01).suffix(unit)).labelled_by(name),
        },
        (ValueType::Int, Value::Int(x)) => match def.range {
            Some((min, max)) => {
                let mut slider = Slider::new(x, (min as i64)..=(max as i64)).suffix(unit).integer();
                if min > 0.0 && max / min > 1000.0 {
                    slider = slider.logarithmic(true);
                }
                ui.add(slider).labelled_by(name)
            }
            None => ui.add(DragValue::new(x).suffix(unit)).labelled_by(name),
        },
        (ValueType::Bool, Value::Bool(b)) => ui.checkbox(b, "").labelled_by(name),
        (ValueType::Vec2, Value::Vec2(v)) => {
            let x = ui.add(DragValue::new(&mut v[0]).speed(1.0).prefix("x ").suffix(unit.clone())).labelled_by(name);
            let y = ui.add(DragValue::new(&mut v[1]).speed(1.0).prefix("y ").suffix(unit)).labelled_by(name);
            x | y
        }
        (ValueType::Color, Value::Color(c)) => color_button(ui, c).labelled_by(name),
        (ValueType::Colors, Value::Colors(colors)) => color_list(ui, colors, name),
        (ValueType::Choice(names), Value::Choice(current)) => {
            let mut response = egui::ComboBox::from_id_salt(def.key).selected_text(pretty(current)).show_ui(ui, |ui| {
                let mut picked = false;
                for choice in names.iter() {
                    picked |= ui.selectable_value(current, (*choice).to_owned(), pretty(choice)).changed();
                }
                picked
            });
            if response.inner.unwrap_or(false) {
                response.response.mark_changed();
            }
            response.response.labelled_by(name)
        }
        (ValueType::Text, Value::Text(text)) => {
            ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY)).labelled_by(name)
        }
        (ValueType::Target, _) => ui.label(RichText::new("Targets come in a later stage").weak()),
        _ => ui.label(RichText::new("This setting holds a value of the wrong kind").color(crate::theme::WARNING)),
    }
}

/// egui keeps an additive colour as premultiplied with no alpha. Built unmultiplied, no alpha
/// turns it into plain transparent black, so "Additive" would undo itself on the next frame.
fn picker_color(c: Color) -> Color32 {
    if c.is_additive() {
        Color32::from_rgba_premultiplied(c.r, c.g, c.b, 0)
    } else {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
    }
}

/// The picker's colour as a scene colour. An additive one comes back as its colour with no
/// alpha, which is how `Color` keeps it.
fn scene_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}

fn color_button(ui: &mut Ui, value: &mut Color) -> Response {
    let mut c = picker_color(*value);
    let response = egui::color_picker::color_edit_button_srgba(ui, &mut c, Alpha::BlendOrAdditive);
    if response.changed() {
        *value = scene_color(c);
    }
    response
}

/// A row of colour buttons with an add button. Right-click a colour to remove it.
fn color_list(ui: &mut Ui, colors: &mut Vec<Color>, name: egui::Id) -> Response {
    let mut changed = false;
    let mut response = ui
        .horizontal_wrapped(|ui| {
            let mut remove = None;
            for (i, c) in colors.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    let response =
                        color_button(ui, c).labelled_by(name).on_hover_text("Click to change. Right-click to remove.");
                    changed |= response.changed();
                    response.context_menu(|ui| {
                        if ui.button("Remove colour").clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                });
            }
            if let Some(i) = remove {
                colors.remove(i);
                changed = true;
            }
            if ui.button("+").on_hover_text("Add a colour. Right-click a colour to remove it.").clicked() {
                colors.push(colors.last().copied().unwrap_or(Color::WHITE));
                changed = true;
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Settings;

    #[test]
    fn names_are_made_readable() {
        assert_eq!(pretty("electric_piano"), "Electric piano");
        assert_eq!(pretty("box"), "Box");
        assert_eq!(pretty(""), "");
    }

    #[test]
    fn simple_depth_shows_only_simple_rows_and_more_groups_the_rest() {
        let defs = bpe_kit::SceneSettings::defs();
        let simple = rows_at(defs, Depth::Simple);
        let more = rows_at(defs, Depth::More);
        assert!(simple.iter().all(|r| matches!(r, Row::Setting(d) if d.depth == Depth::Simple)));
        assert!(more.iter().filter(|r| matches!(r, Row::Setting(_))).count() == defs.len());
        let headings: Vec<&str> =
            more.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect();
        assert_eq!(headings, ["Sound", "Video", "Song", "Beat"]);
        assert!(!simple.iter().any(|r| matches!(r, Row::Group(_))));
    }
}
