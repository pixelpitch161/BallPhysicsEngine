//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.

use bpe_core::{Color, Depth, PulseSource, Registry, Scene, SettingDef, Target, ThingId, Value, ValueType};
use eframe::egui::{
    self, Color32, DragValue, Pos2, Response, RichText, Sense, Slider, Stroke, Ui, Vec2, color_picker::Alpha,
};

use crate::theme;

/// What a Target setting can point at: the scene's things, tags and kinds, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetChoices {
    /// In tree order.
    pub things: Vec<(ThingId, String)>,
    /// Every tag any thing has, sorted.
    pub tags: Vec<String>,
    /// Thing kinds as (id, name), in registry order, without the scene itself.
    pub kinds: Vec<(String, String)>,
}

impl TargetChoices {
    pub fn from_scene(scene: &Scene, reg: &Registry) -> TargetChoices {
        let things = scene.order.iter().filter_map(|id| scene.things.get(id).map(|t| (*id, t.name.clone()))).collect();
        let tags: std::collections::BTreeSet<String> =
            scene.things.values().flat_map(|t| t.tags.iter().cloned()).collect();
        let kinds =
            reg.things().filter(|k| k.id() != "scene").map(|k| (k.id().to_owned(), k.name().to_owned())).collect();
        TargetChoices { things, tags: tags.into_iter().collect(), kinds }
    }
}

/// A target in plain words: "Anything", "Itself", "Thing: Ball", "Tag: walls", "Kind: Arena".
pub fn target_words(t: &Target, choices: &TargetChoices) -> String {
    match t {
        Target::Anything => "Anything".to_owned(),
        Target::Me => "Itself".to_owned(),
        Target::Thing(id) => match choices.things.iter().find(|(t, _)| t == id) {
            Some((_, name)) => format!("Thing: {name}"),
            None => format!("Thing: missing (id {})", id.0),
        },
        Target::Tag(tag) => format!("Tag: {tag}"),
        Target::Kind(kind) => {
            let name = choices.kinds.iter().find(|(id, _)| id == kind).map(|(_, n)| n.as_str()).unwrap_or(kind);
            format!("Kind: {name}")
        }
    }
}

/// What a settings grid needs to know beyond the values: what targets can point at, which
/// settings this thing changed for itself and what they would be without that change, and
/// what each setting's keyframe diamond shows (none for a row without a diamond).
pub struct GridCx<'a> {
    pub choices: &'a TargetChoices,
    pub overridden: &'a dyn Fn(&str) -> bool,
    pub inherited: &'a dyn Fn(&str) -> Option<Value>,
    pub marks: &'a dyn Fn(&str) -> Option<KeyMark>,
    /// Whether the setting is locked against the dice.
    pub locked: &'a dyn Fn(&str) -> bool,
}

/// What the user did in a marked grid: values changed, overrides reset to the module, overrides
/// pushed into the module, keyframe diamonds clicked and pulses picked.
#[derive(Default)]
pub struct GridOutcome {
    pub changes: Vec<(&'static str, Value)>,
    pub resets: Vec<&'static str>,
    pub pushes: Vec<&'static str>,
    /// The settings whose diamond was clicked: add a key at the playhead, or remove the one there.
    pub toggled_key: Vec<&'static str>,
    /// Pulse choices from the diamond's menu: a source, or `None` to stop pulsing.
    pub pulse: Vec<(&'static str, Option<PulseSource>)>,
    /// Settings locked (true) or unlocked against the dice from the row's menu.
    pub locks: Vec<(&'static str, bool)>,
}

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

/// The width of the diamond cell at the left of every row.
pub const DIAMOND_CELL: f32 = 16.0;

/// A settings grid at `depth` with no marks and nothing for targets to point at. `get` supplies
/// each setting's value as it stands; the rows the user changed come back as (key, new value)
/// for the caller to write through `set_at`.
pub fn settings_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    depth: Depth,
    get: impl Fn(&str) -> Value,
) -> Vec<(&'static str, Value)> {
    let choices = TargetChoices::default();
    let cx = GridCx {
        choices: &choices,
        overridden: &|_| false,
        inherited: &|_| None,
        marks: &|_| None,
        locked: &|_| false,
    };
    settings_grid_marked(ui, id, defs, depth, get, &cx).changes
}

/// A settings grid at `depth` with a keyframe diamond before each setting that can have one
/// (click: a key at the playhead; right-click: pulses), and a dot before the settings this thing
/// changed for itself. Such a row's double-click or Alt+click goes back to the module's value,
/// and its right-click menu offers to reset or push the change.
pub fn settings_grid_marked(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    depth: Depth,
    get: impl Fn(&str) -> Value,
    cx: &GridCx,
) -> GridOutcome {
    let mut out = GridOutcome::default();
    egui::Grid::new(id).num_columns(3).spacing([8.0, 6.0]).min_col_width(DIAMOND_CELL).show(ui, |ui| {
        for row in rows_at(defs, depth) {
            match row {
                Row::Group(group) => {
                    empty_cell(ui);
                    // A grid can't take spacing, so the heading carries its own air above.
                    ui.add(egui::Label::new(RichText::new(format!("\n{group}")).strong().color(theme::MUTED)));
                    ui.end_row();
                }
                Row::Setting(def) => {
                    match (cx.marks)(def.key) {
                        Some(mark) => match diamond(ui, def.label, mark) {
                            DiamondClick::None => {}
                            DiamondClick::Toggle => out.toggled_key.push(def.key),
                            DiamondClick::Pulse(source) => out.pulse.push((def.key, source)),
                        },
                        None => empty_cell(ui),
                    }
                    let mut value = get(def.key);
                    let overridden = (cx.overridden)(def.key);
                    let inherited = (cx.inherited)(def.key);
                    let locked = (cx.locked)(def.key);
                    let row = setting(ui, def, &mut value, inherited.as_ref(), overridden, locked, cx.choices);
                    match row {
                        RowAction::Nothing => {}
                        RowAction::Changed => out.changes.push((def.key, value)),
                        RowAction::Reset => out.resets.push(def.key),
                        RowAction::Push => out.pushes.push(def.key),
                        RowAction::Lock(lock) => out.locks.push((def.key, lock)),
                    }
                }
            }
        }
    });
    out
}

/// What one grid row asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowAction {
    Nothing,
    /// `value` holds the new value.
    Changed,
    /// Drop this thing's own change, so the module's value shows again.
    Reset,
    /// Write this thing's own change into the module.
    Push,
    /// Lock (true) or unlock the setting against the dice.
    Lock(bool),
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
    let accent = theme::ACCENT_TEXT;
    let color = if mark.keyed || response.hovered() { accent } else { theme::MUTED };
    paint_diamond(ui.painter(), rect.center(), 5.0, mark.on_key, color);
    if mark.pulsed {
        let painter = ui.painter();
        let x0 = rect.right() - 4.0;
        let y = rect.center().y;
        let points: Vec<Pos2> =
            (0..=6).map(|i| Pos2::new(x0 + i as f32 * 0.8, y + ((i as f32) * 1.6).sin() * 3.0)).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, theme::WARNING)));
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
/// back to `inherited`, or the setting's default), a dot when the setting is `overridden`, a
/// padlock when it is `locked` against the dice, and the widget for its type. The label's menu
/// offers the lock on any setting the dice may pick.
pub fn setting(
    ui: &mut Ui,
    def: &SettingDef,
    value: &mut Value,
    inherited: Option<&Value>,
    overridden: bool,
    locked: bool,
    choices: &TargetChoices,
) -> RowAction {
    let fallback = inherited.cloned().unwrap_or_else(|| def.default.clone());
    let mut action = RowAction::Nothing;
    let name = ui
        .horizontal(|ui| {
            if overridden {
                let (dot, response) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Changed here"));
                ui.painter().circle_filled(dot.center(), 3.0, theme::OVERRIDE);
                response.on_hover_text("Changed here: this thing's own value, not the module's.");
            }
            let name = label(ui, def.label, def.help);
            if locked {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Locked when randomizing"));
                paint_padlock(ui.painter(), rect, theme::MUTED);
                response.on_hover_text("Locked: the dice leave this setting alone.");
            }
            if overridden || def.flags.random {
                name.context_menu(|ui| {
                    if overridden {
                        if ui.button("Reset to module").on_hover_text("Follow the module's value again").clicked() {
                            action = RowAction::Reset;
                            ui.close();
                        }
                        if ui
                            .button("Push to module")
                            .on_hover_text("Give every thing using the module this value")
                            .clicked()
                        {
                            action = RowAction::Push;
                            ui.close();
                        }
                    }
                    if def.flags.random {
                        if locked {
                            if ui.button("Unlock").on_hover_text("Let the dice pick this setting again").clicked() {
                                action = RowAction::Lock(false);
                                ui.close();
                            }
                        } else if ui
                            .button("Lock when randomizing")
                            .on_hover_text("Keep this setting as it is when the dice roll")
                            .clicked()
                        {
                            action = RowAction::Lock(true);
                            ui.close();
                        }
                    }
                });
            }
            name
        })
        .inner;
    let mut changed = false;
    let response = ui
        .horizontal(|ui| {
            let r = widget(ui, def, value, name.id, choices);
            changed |= r.changed();
            r
        })
        .inner;
    if resettable(&name, value, &fallback) || resettable(&response, value, &fallback) {
        changed = true;
    }
    ui.end_row();
    if action == RowAction::Nothing && changed {
        action = RowAction::Changed;
    }
    action
}

/// A small padlock: a body with a shackle over it.
pub fn paint_padlock(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let c = rect.center();
    let body = egui::Rect::from_center_size(Pos2::new(c.x, c.y + 2.0), Vec2::new(8.0, 6.0));
    painter.rect_filled(body, 1.5, color);
    let shackle: Vec<Pos2> = (0..=8)
        .map(|i| {
            let t = std::f32::consts::PI * (1.0 - i as f32 / 8.0);
            Pos2::new(c.x + 2.5 * t.cos(), c.y - 1.0 - 3.0 * t.sin())
        })
        .collect();
    painter.add(egui::Shape::line(shackle, Stroke::new(1.5, color)));
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
fn widget(ui: &mut Ui, def: &SettingDef, value: &mut Value, name: egui::Id, choices: &TargetChoices) -> Response {
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
            // Leave room for the browse button, or the row grows past the panel.
            let width = (ui.available_width() - 40.0).max(80.0);
            let mut response = ui.add(egui::TextEdit::singleline(text).desired_width(width)).labelled_by(name);
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
        (ValueType::Target, Value::Target(t)) => target_picker(ui, t, choices).labelled_by(name),
        _ => ui.label(RichText::new("This setting holds a value of the wrong kind").color(theme::WARNING)),
    }
}

/// A menu naming the target, with Anything, Itself and the scene's things, tags and kinds to
/// pick from. Marked changed when something is picked.
fn target_picker(ui: &mut Ui, t: &mut Target, choices: &TargetChoices) -> Response {
    let mut picked: Option<Target> = None;
    let menu = ui.menu_button(target_words(t, choices), |ui| {
        ui.set_min_width(160.0);
        if ui.selectable_label(*t == Target::Anything, "Anything").clicked() {
            picked = Some(Target::Anything);
            ui.close();
        }
        if ui.selectable_label(*t == Target::Me, "Itself").on_hover_text("The thing this setting belongs to").clicked()
        {
            picked = Some(Target::Me);
            ui.close();
        }
        ui.menu_button("Thing", |ui| {
            for (id, name) in &choices.things {
                if ui.selectable_label(*t == Target::Thing(*id), name).clicked() {
                    picked = Some(Target::Thing(*id));
                    ui.close();
                }
            }
        });
        if choices.tags.is_empty() {
            ui.add_enabled(false, egui::Button::new("Tag"))
                .on_disabled_hover_text("No tags yet. Give things tags in their header.");
        } else {
            ui.menu_button("Tag", |ui| {
                for tag in &choices.tags {
                    if ui.selectable_label(matches!(t, Target::Tag(x) if x == tag), tag).clicked() {
                        picked = Some(Target::Tag(tag.clone()));
                        ui.close();
                    }
                }
            });
        }
        ui.menu_button("Kind", |ui| {
            for (id, name) in &choices.kinds {
                if ui.selectable_label(matches!(t, Target::Kind(x) if x == id), name).clicked() {
                    picked = Some(Target::Kind(id.clone()));
                    ui.close();
                }
            }
        });
    });
    let mut response = menu.response.on_hover_text("What this points at");
    if let Some(p) = picked {
        *t = p;
        response.mark_changed();
    }
    response
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
    fn target_words_name_things_tags_and_kinds() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(1)).unwrap().tags.insert("walls".into());
        let c = TargetChoices::from_scene(&scene, &kit.registry);
        assert_eq!(target_words(&Target::Anything, &c), "Anything");
        assert_eq!(target_words(&Target::Me, &c), "Itself");
        assert_eq!(target_words(&Target::Thing(ThingId(2)), &c), "Thing: Ball");
        assert_eq!(target_words(&Target::Thing(ThingId(9)), &c), "Thing: missing (id 9)");
        assert_eq!(target_words(&Target::Tag("walls".into()), &c), "Tag: walls");
        assert_eq!(target_words(&Target::Kind("arena".into()), &c), "Kind: Arena");
        assert_eq!(target_words(&Target::Kind("blob".into()), &c), "Kind: blob");
        assert_eq!(c.tags, vec!["walls".to_owned()]);
        assert!(c.kinds.iter().all(|(id, _)| id != "scene"));
        assert_eq!(c.things[0], (ThingId::ROOT, "Scene defaults".to_owned()));
    }

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
            |ui, out: &mut GridOutcome| {
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
                let choices = TargetChoices::default();
                let cx = GridCx {
                    choices: &choices,
                    overridden: &|_| false,
                    inherited: &|_| None,
                    marks: &marks,
                    locked: &|_| false,
                };
                let got = settings_grid_marked(
                    ui,
                    "g",
                    defs,
                    Depth::Simple,
                    |k| defs.iter().find(|d| d.key == k).unwrap().default.clone(),
                    &cx,
                );
                out.changes.extend(got.changes);
                out.toggled_key.extend(got.toggled_key);
                out.pulse.extend(got.pulse);
            },
            GridOutcome::default(),
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
