//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.

use bpe_core::{Color, Depth, PulseSource, SettingDef, Value, ValueType};
use eframe::egui::{
    self, Color32, DragValue, Pos2, Response, RichText, Sense, Slider, Stroke, Ui, color_picker::Alpha,
};

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

/// What a setting's keyframe diamond shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMark {
    /// Keys are allowed on the setting.
    pub can_key: bool,
    /// A pulse is allowed on the setting.
    pub can_pulse: bool,
    /// The setting has keys.
    pub keyed: bool,
    /// A key sits at the playhead.
    pub on_key: bool,
    /// The setting pulses with the music or the hits.
    pub pulsed: bool,
    /// The setting moves the physics, so it can't pulse with hits.
    pub physics: bool,
}

/// What the user did in a settings grid, for the caller to write to the scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridChanges {
    /// (key, new value) for the rows that were edited.
    pub set: Vec<(&'static str, Value)>,
    /// The settings whose diamond was clicked: add a key at the playhead, or remove the one there.
    pub toggled_key: Vec<&'static str>,
    /// Pulse choices from the diamond's menu: a source, or `None` to stop pulsing.
    pub pulse: Vec<(&'static str, Option<PulseSource>)>,
}

/// The width of the diamond cell at the left of every row.
pub const DIAMOND_CELL: f32 = 16.0;

/// A settings grid at `depth`: a keyframe diamond (when `marks` says the setting can have
/// one), the label and the widget per row. `get` supplies each setting's value as it stands.
pub fn settings_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    depth: Depth,
    get: impl Fn(&str) -> Value,
    marks: &dyn Fn(&str) -> Option<KeyMark>,
) -> GridChanges {
    let mut changes = GridChanges::default();
    egui::Grid::new(id).num_columns(3).spacing([8.0, 6.0]).min_col_width(DIAMOND_CELL).show(ui, |ui| {
        for row in rows_at(defs, depth) {
            match row {
                Row::Group(group) => {
                    empty_cell(ui);
                    ui.label(RichText::new(group).strong().color(crate::theme::MUTED));
                    ui.end_row();
                }
                Row::Setting(def) => {
                    match marks(def.key) {
                        Some(mark) => match diamond(ui, def.label, mark) {
                            DiamondClick::None => {}
                            DiamondClick::Toggle => changes.toggled_key.push(def.key),
                            DiamondClick::Pulse(source) => changes.pulse.push((def.key, source)),
                        },
                        None => empty_cell(ui),
                    }
                    let mut value = get(def.key);
                    if setting(ui, def, &mut value, None) {
                        changes.set.push((def.key, value));
                    }
                }
            }
        }
    });
    changes
}

/// The diamond column's cell for a row without a diamond.
fn empty_cell(ui: &mut Ui) {
    ui.allocate_exact_size(egui::vec2(DIAMOND_CELL, DIAMOND_CELL), Sense::hover());
}

enum DiamondClick {
    None,
    Toggle,
    Pulse(Option<PulseSource>),
}

/// The keyframe diamond of one row: hollow when the setting has no keys, outlined when it has
/// some, filled when one sits at the playhead, with a small wave beside it when it pulses. A
/// click toggles a key at the playhead; a right-click offers the pulses.
fn diamond(ui: &mut Ui, label: &str, mark: KeyMark) -> DiamondClick {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(DIAMOND_CELL, DIAMOND_CELL), Sense::click());
    let name = format!("Keyframe {label}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let accent = crate::theme::ACCENT_TEXT;
    let color = if mark.keyed || response.hovered() { accent } else { crate::theme::MUTED };
    paint_diamond(ui.painter(), rect.center(), 5.0, mark.on_key, color);
    if mark.pulsed {
        let painter = ui.painter();
        let x0 = rect.right() - 4.0;
        let y = rect.center().y;
        let points: Vec<Pos2> =
            (0..=6).map(|i| Pos2::new(x0 + i as f32 * 0.8, y + ((i as f32) * 1.6).sin() * 3.0)).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, crate::theme::WARNING)));
    }
    let hint = match (mark.can_key, mark.on_key, mark.can_pulse) {
        (false, _, _) => "Right-click to pulse with the music. This setting can't have keyframes.",
        (true, true, true) => "Remove the key here. Right-click for pulses.",
        (true, false, true) => "Add a keyframe at the playhead. Right-click to pulse with the music.",
        (true, true, false) => "Remove the key here.",
        (true, false, false) => "Add a keyframe at the playhead.",
    };
    let mut out = if response.clicked() && mark.can_key { DiamondClick::Toggle } else { DiamondClick::None };
    if mark.can_pulse {
        response.context_menu(|ui| {
            for source in PulseSource::NAMED {
                if source == PulseSource::Hits && mark.physics {
                    continue;
                }
                if ui.button(format!("Pulse with {}", source.name().to_lowercase())).clicked() {
                    out = DiamondClick::Pulse(Some(source));
                    ui.close();
                }
            }
            if mark.pulsed && ui.button("No pulse").clicked() {
                out = DiamondClick::Pulse(None);
                ui.close();
            }
        });
    }
    response.on_hover_text(hint);
    out
}

/// A keyframe diamond: filled, or just its outline.
pub fn paint_diamond(painter: &egui::Painter, center: Pos2, size: f32, filled: bool, color: Color32) {
    let points = vec![
        center + egui::vec2(0.0, -size),
        center + egui::vec2(size, 0.0),
        center + egui::vec2(0.0, size),
        center + egui::vec2(-size, 0.0),
    ];
    if filled {
        painter.add(egui::Shape::convex_polygon(points, color, Stroke::new(1.0, color)));
    } else {
        painter.add(egui::Shape::closed_line(points, Stroke::new(1.5, color)));
    }
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
        (ValueType::Text, Value::Text(text)) if def.unit == "file" => {
            let mut response = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY)).labelled_by(name);
            let browse = ui.add(egui::Button::new("…").small()).on_hover_text("Choose a file");
            browse.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Browse {}", def.label)));
            if browse.clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                *text = path.to_string_lossy().into_owned();
                response.mark_changed();
            }
            response
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
    fn a_keyframe_diamond_reports_clicks_and_pulse_menu_items() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        let defs = bpe_kit::BallsSettings::defs();
        let mut h = Harness::builder().with_size(egui::vec2(420.0, 600.0)).allow_missing_glyphs().build_ui_state(
            |ui, out: &mut GridChanges| {
                let marks = |key: &str| {
                    (key == "radius").then_some(KeyMark {
                        can_key: true,
                        can_pulse: true,
                        keyed: true,
                        on_key: false,
                        pulsed: true,
                        physics: true,
                    })
                };
                let changes = settings_grid(
                    ui,
                    "g",
                    defs,
                    Depth::Simple,
                    |k| defs.iter().find(|d| d.key == k).unwrap().default.clone(),
                    &marks,
                );
                out.set.extend(changes.set);
                out.toggled_key.extend(changes.toggled_key);
                out.pulse.extend(changes.pulse);
            },
            GridChanges::default(),
        );
        h.run();
        let diamonds = h
            .get_all_by_role(Role::Button)
            .filter(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with("Keyframe")))
            .count();
        assert_eq!(diamonds, 1);
        h.get_by_label("Keyframe Size").click();
        h.run();
        assert_eq!(h.state().toggled_key, ["radius"]);
        h.get_by_label("Keyframe Size").click_secondary();
        h.run();
        assert!(h.query_by_label("Pulse with hits").is_none(), "a physics setting can't follow the hits");
        h.get_by_label("Pulse with bass").click();
        h.run();
        assert_eq!(h.state().pulse, [("radius", Some(PulseSource::Bass))]);
        h.get_by_label("Keyframe Size").click_secondary();
        h.run();
        h.get_by_label("No pulse").click();
        h.run();
        assert_eq!(h.state().pulse.last(), Some(&("radius", None)));
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
