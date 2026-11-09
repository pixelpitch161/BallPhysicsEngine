//! The settings grid: label and widget cells, group headings and folds, and one row per setting.

use super::*;

/// How wide the label column of every settings grid in a panel `panel` wide is: the Design
/// System's 108, or 96 in a narrow panel so the widgets still fit beside the labels. Every grid in
/// the panel uses the same width, so the sliders line up from card to card.
pub fn label_width(panel: f32) -> f32 {
    if panel >= 330.0 { style::LABEL_W } else { 96.0 }
}

/// The colour of a setting's label: its thing's track colour when it has keys or a pulse, so it
/// matches its lane on the timeline; else the grey of every label.
pub fn label_colour(mark: Option<KeyMark>, track: Color32) -> Color32 {
    if mark.is_some_and(|m| m.keyed || m.pulsed) { track } else { style::MUTED }
}

/// The start of a row in a [`settings_like_grid`]: a label in the label column, `width` wide,
/// wrapping when it is longer.
pub fn grid_label(ui: &mut Ui, width: f32, text: &str) -> Response {
    label_cell(ui, width, |ui| {
        // The changed dot's place, so these labels line up with the settings'.
        ui.add_space(9.0);
        ui.add(egui::Label::new(RichText::new(text).color(style::MUTED)).wrap())
    })
}

/// A grid laid out like a settings grid (the label column, then the widgets), so its labels and
/// widgets line up with the settings' when its rows start with [`grid_label`].
pub fn settings_like_grid(id: impl egui::AsIdSalt) -> egui::Grid {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 6.0])
}

/// A grid cell exactly `width` wide, laid out left to right. What it holds wraps rather than
/// widening the column, and makes its row as tall as it needs.
pub fn label_cell<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    grid_cell(ui, width, layout, |ui| {
        ui.set_min_width(width);
        add(ui)
    })
}

/// A grid row's widget cell, from where the grid has got to up to `right`, laid out left to
/// right from its top. Something in it more than a line tall (x and y wrapped onto two lines)
/// makes its row as tall as it needs.
pub fn widget_cell<R>(ui: &mut Ui, right: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = (right - ui.cursor().left()).max(0.0);
    grid_cell(ui, width, egui::Layout::left_to_right(egui::Align::Min), add)
}

/// A grid cell at most `width` wide. A grid centres a cell in its row before the cell is
/// drawn, so the cell asks for the height it took last time: something taller than a line then
/// sits centred in a row tall enough for it, instead of hanging out of the bottom of its row
/// into the next.
pub(super) fn grid_cell<R>(ui: &mut Ui, width: f32, layout: egui::Layout, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.next_auto_id().with("grid cell");
    let height = ui.ctx().data(|d| d.get_temp::<f32>(id)).filter(|h| h.is_finite()).unwrap_or(0.0);
    let cell = ui.allocate_ui_with_layout(Vec2::new(width, height), layout, |ui| {
        ui.set_max_width(width);
        add(ui)
    });
    // An empty cell has no height to remember.
    let took = cell.response.rect.height();
    if took.is_finite() && (took - height).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, took));
        ui.ctx().request_discard("a grid cell's height changed");
    }
    cell.inner
}

/// Whether a group of `defs` holds changes: a shown row whose value differs from its default,
/// or that has keys or a pulse.
pub(super) fn group_changed(defs: &[SettingDef], group: &str, get: &dyn Fn(&str) -> Value, cx: &GridCx) -> bool {
    defs.iter()
        .filter(|d| d.group == group && (cx.applies)(d))
        .any(|d| get(d.key) != d.default || (cx.marks)(d.key).is_some_and(|m| m.keyed || m.pulsed))
}

/// A group's heading: its name, which folds and opens the group when `fold` is given (as does
/// the arrow before it), and a dot after it when the group is `shut` holding `changed` rows.
/// True when the heading or its arrow was clicked.
pub(crate) fn group_heading(ui: &mut Ui, group: &'static str, fold: bool, shut: bool, changed: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        if fold && fold_toggle(ui, !shut, group) {
            clicked = true;
        }
        let heading = RichText::new(group).font(style::semibold(12.0)).color(style::MUTED);
        let label = egui::Label::new(heading).wrap().selectable(false);
        let response = ui.add(if fold { label.sense(Sense::click()) } else { label });
        let dot = shut && changed;
        if dot {
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{group} (changed)")));
        }
        if fold && response.clicked() {
            clicked = true;
        }
        if dot {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Changes inside"));
            ui.painter().circle_filled(rect.center(), 3.0, style::TEXT);
            response.on_hover_text("Something in here is changed, keyed or pulsing.");
        }
    });
    clicked
}

/// A settings grid of the settings that apply, each row's label in the accent colour when the
/// setting has keys or pulses, and a right-click menu on every row (see [`setting`]).
pub fn settings_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    get: impl Fn(&str) -> Value,
    cx: &GridCx,
) -> GridOutcome {
    let mut out = GridOutcome::default();
    // Where the widgets must end. A grid widens its last column to its widest row, so a row
    // that can't shrink would otherwise push every slider under it past the panel's edge.
    let right = ui.available_rect_before_wrap().right();
    let sections = sections_for(defs, cx);
    let shut = |group: &str| !group.is_empty() && cx.folded.is_some_and(|folded| folded(group));
    settings_like_grid(id).show(ui, |ui| {
        for section in &sections {
            let shut = shut(section.group);
            if !section.group.is_empty() {
                let group = section.group;
                let changed = shut && group_changed(defs, group, &get, cx);
                // A grid can't take spacing, so the heading carries its own air above.
                label_cell(ui, cx.label_width, |ui| {
                    let room = ui.available_size();
                    ui.allocate_ui_with_layout(room, egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.add_space(10.0);
                        if group_heading(ui, group, cx.folded.is_some(), shut, changed) {
                            out.folds.push((group, !shut));
                        }
                    });
                });
                ui.end_row();
            }
            if !shut {
                for def in &section.defs {
                    grid_row(ui, def, &get, cx, right, &mut out);
                }
            }
        }
    });
    out
}

/// One section of a settings grid: its group ("" for the ungrouped rows before the headings)
/// and the rows shown in it.
pub(super) struct Section<'a> {
    pub(super) group: &'static str,
    pub(super) defs: Vec<&'a SettingDef>,
}

/// The sections of [`rows_for`], placed by [`section_rank`].
pub(super) fn sections_for<'a>(defs: &'a [SettingDef], cx: &GridCx) -> Vec<Section<'a>> {
    let mut sections: Vec<Section<'a>> = Vec::new();
    for row in rows_for(defs, cx.applies) {
        match row {
            Row::Group(group) => sections.push(Section { group, defs: Vec::new() }),
            Row::Setting(def) => match sections.last_mut() {
                Some(section) => section.defs.push(def),
                None => sections.push(Section { group: "", defs: vec![def] }),
            },
        }
    }
    // A stable sort: the ungrouped rows first, then by rank, keeping `rows_for`'s order otherwise.
    sections.sort_by_key(|s| (!s.group.is_empty(), section_rank(s.group).unwrap_or(usize::MAX)));
    sections
}

/// Rows drawn under a setting's own row (a gradient's tabs), given the id its bar keeps its
/// memory under and the value to edit; true when they changed it.
pub(super) type TabRows<'a> = dyn FnMut(&mut Ui, egui::Id, &mut Value) -> bool + 'a;

/// What a gradient's own row has beyond its widget: its tabs, its companions as they read now
/// (for "Copy gradient with its settings", each by its name) and where a paste puts the
/// companions it brought.
pub(super) struct GradientRow<'r, 't> {
    pub(super) tabs: &'r mut TabRows<'t>,
    pub(super) companions: &'r dyn Fn() -> Vec<(&'static str, Value)>,
    pub(super) pasted: &'r mut Vec<(&'static str, Value)>,
}

/// One setting's row of a settings grid, with what the user did to it added to `out`. A
/// gradient of two or more stops gets its tabs in rows under it (`gradient_tabs`).
pub(crate) fn grid_row(
    ui: &mut Ui,
    def: &SettingDef,
    get: &dyn Fn(&str) -> Value,
    cx: &GridCx,
    right: f32,
    out: &mut GridOutcome,
) {
    grid_row_with(ui, def, get, cx, right, out, None);
}

/// [`grid_row`] with `trailing` drawn at the end of the widget cell, in [`TRAILING_W`] kept free
/// for it (a rule block's "from" chip).
pub(crate) fn grid_row_with(
    ui: &mut Ui,
    def: &SettingDef,
    get: &dyn Fn(&str) -> Value,
    cx: &GridCx,
    right: f32,
    out: &mut GridOutcome,
    trailing: Option<&mut dyn FnMut(&mut Ui)>,
) {
    let mut value = get(def.key);
    let unit = unit_for(def, &|k| Some(get(k)));
    let mut varies = Vec::new();
    let mut companions = GridOutcome::default();
    let mut pasted = Vec::new();
    let action = {
        let mut tabs = |ui: &mut Ui, bar: egui::Id, value: &mut Value| {
            crate::gradient_tabs::tab_rows(ui, def, value, bar, cx, right, get, &mut companions)
        };
        let now = || {
            bpe_core::gradient_extras::companions_for(def)
                .map(|c| (c.name, get(crate::gradient_tabs::companion_def(def, c).key)))
                .collect()
        };
        let gradient = def.ty == ValueType::Gradient
            && !bpe_core::gradient_extras::is_companion_key(def.key)
            && bpe_core::vary::split(def.key).is_none();
        let row = gradient.then_some(GradientRow { tabs: &mut tabs, companions: &now, pasted: &mut pasted });
        setting(ui, def, unit, &mut value, (cx.marks)(def.key), cx, right, &mut varies, row, trailing)
    };
    for (name, v) in pasted {
        if let Some(c) = bpe_core::gradient_extras::companion(name) {
            out.changes.push((crate::gradient_tabs::companion_def(def, c).key, v));
        }
    }
    out.changes.extend(companions.changes);
    out.new_keys.extend(companions.new_keys);
    out.cleared.extend(companions.cleared);
    out.follows.extend(companions.follows);
    out.varies.extend(companions.varies);
    out.varies.extend(varies.into_iter().map(|v| (def.key, v)));
    // Free points follow their stops: a stop added or removed adds or removes its point.
    if action == RowAction::Changed
        && let (Value::Gradient(old), Value::Gradient(new)) = (get(def.key), &value)
        && let Some(c) = bpe_core::gradient_extras::companion("points")
        && bpe_core::gradient_extras::offers(def, c)
    {
        let points_def = crate::gradient_tabs::companion_def(def, c);
        if let Value::Points(points) = get(points_def.key)
            && let Some(moved) = bpe_core::gradient_form::points_after(&old, new, &points)
        {
            out.changes.push((points_def.key, Value::Points(moved)));
        }
    }
    match action {
        RowAction::Nothing => {}
        RowAction::Changed => out.changes.push((def.key, value)),
        RowAction::CreateKey => out.new_keys.push(def.key),
        RowAction::RemoveKeys => out.cleared.push(def.key),
        RowAction::Follow(source) => out.follows.push((def.key, source)),
    }
}

/// The arrow that folds a card or a group of settings away (pointing right) and back (pointing
/// down), named "Hide {title}" or "Show {title}". True when clicked.
pub fn fold_toggle(ui: &mut Ui, open: bool, title: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let name = if open { format!("Hide {title}") } else { format!("Show {title}") };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let response = response.on_hover_text(if open { "Hide these settings" } else { "Show these settings" });
    let ink = if response.hovered() { style::TEXT } else { style::MUTED };
    style::paint_chevron(ui.painter(), rect, open, ink);
    response.clicked()
}

/// The label cell, clickable for the reset shortcut, in the accent colour when `lit`.
pub(super) fn label(ui: &mut Ui, text: &str, help: &str, ink: Color32) -> Response {
    let text = RichText::new(text).color(ink);
    let response = ui.add(egui::Label::new(text).wrap().selectable(false).sense(Sense::click()));
    if help.is_empty() { response } else { response.on_hover_text(help) }
}
