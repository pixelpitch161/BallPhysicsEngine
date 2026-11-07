//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.

use bpe_core::{Color, Registry, Scene, SettingDef, Target, ThingId, Value, ValueType, section_rank};
use eframe::egui::{
    self, Color32, DragValue, PointerButton, Response, RichText, Sense, Slider, Ui, Vec2, color_picker::Alpha,
};

use crate::gradient_bar::gradient_bar_named;
use crate::style;

/// What a Target setting can point at: the scene's things, tags and kinds, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetChoices {
    /// In tree order.
    pub things: Vec<(ThingId, String)>,
    /// Every tag any thing has, sorted.
    pub tags: Vec<String>,
    /// Thing kinds as (id, name), in registry order, without the scene itself.
    pub kinds: Vec<(String, String)>,
    /// The things with balls (a Balls group or an emitter): a rule's who can name one ball of them.
    pub ball_things: Vec<ThingId>,
    /// A rule block's who picker offers the firing's own parties (That ball, The thing it hit,
    /// That ball's group): true for If and Do blocks, false for When blocks, where they would mean
    /// anything.
    pub relative: bool,
}

impl TargetChoices {
    pub fn from_scene(scene: &Scene, reg: &Registry) -> TargetChoices {
        let things = scene.order.iter().filter_map(|id| scene.things.get(id).map(|t| (*id, t.name.clone()))).collect();
        let tags: std::collections::BTreeSet<String> =
            scene.things.values().flat_map(|t| t.tags.iter().cloned()).collect();
        let kinds =
            reg.things().filter(|k| k.id() != "scene").map(|k| (k.id().to_owned(), k.name().to_owned())).collect();
        let ball_things = scene
            .order
            .iter()
            .filter(|id| scene.things.get(id).is_some_and(|t| bpe_kit::things::balls::rows::has_balls(&t.kind)))
            .copied()
            .collect();
        TargetChoices { things, tags: tags.into_iter().collect(), kinds, ball_things, relative: true }
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

/// What a settings grid needs to know beyond the values: what targets can point at and what each
/// setting's keys are (none for a setting that can't be keyed or pulsed).
pub struct GridCx<'a> {
    pub choices: &'a TargetChoices,
    pub marks: &'a dyn Fn(&str) -> Option<KeyMark>,
    /// Whether the setting does anything here; rows that don't are left out.
    pub applies: &'a dyn Fn(&SettingDef) -> bool,
    /// Which group headings are folded shut. With none, headings have no fold arrow.
    pub folded: Option<&'a dyn Fn(&str) -> bool>,
    /// How wide the label column is, from [`label_width`]. Labels wrap at it.
    pub label_width: f32,
    /// The thing's track colour: the label colour of a setting with keys or a pulse.
    pub track: Color32,
    /// Follow sound for these settings (none: the menu doesn't offer it).
    pub follow: Option<&'a crate::follow::FollowCx<'a>>,
    /// Varying these settings across the thing's balls (none: the menu doesn't offer it).
    pub vary: Option<&'a crate::vary::VaryCx<'a>>,
}

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
fn grid_cell<R>(ui: &mut Ui, width: f32, layout: egui::Layout, add: impl FnOnce(&mut Ui) -> R) -> R {
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

/// What the user did in a settings grid: values changed (pasted ones too), keyframes asked for
/// from a row's menu, and groups folded.
#[derive(Default)]
pub struct GridOutcome {
    pub changes: Vec<(&'static str, Value)>,
    /// The settings whose menu's Create Keyframe was picked: a key at the playhead holding the
    /// value as it stands.
    pub new_keys: Vec<&'static str>,
    /// Groups folded shut (true) or opened from their heading's arrow.
    pub folds: Vec<(&'static str, bool)>,
    /// Settings that start following sound (or stop, with `None`) from their menu.
    pub follows: Vec<(&'static str, Option<bpe_core::PulseSource>)>,
    /// What was asked of the settings that vary across the balls (or start or stop varying).
    pub varies: Vec<(&'static str, crate::vary::VaryEdit)>,
}

/// The description of the setting `key` among `defs`: a companion key (`"colours~bands"`) is
/// answered through its gradient's description, as the companion's own.
pub fn def_in<'a>(defs: &'a [SettingDef], key: &str) -> Option<&'a SettingDef> {
    match bpe_core::gradient_extras::split(key) {
        Some((base, c)) => {
            let base = defs.iter().find(|d| d.key == base)?;
            (base.ty == ValueType::Gradient).then(|| crate::gradient_tabs::companion_def(base, c))
        }
        None => defs.iter().find(|d| d.key == key),
    }
}

/// `electric_piano` to "Electric piano".
pub fn pretty(name: &str) -> String {
    let mut text = name.replace('_', " ");
    // "flat2 d" (a choice named Flat2D) reads "flat 2D".
    for dims in ["2", "3"] {
        let split = format!("{dims} d");
        if let Some(at) = text.find(&split)
            && text[at + split.len()..].chars().next().is_none_or(|c| c == ' ')
        {
            let space = if at > 0 && !text[..at].ends_with(' ') { " " } else { "" };
            text.replace_range(at..at + split.len(), &format!("{space}{dims}D"));
        }
    }
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// One row of a settings grid: a setting, or a heading over the group that follows.
pub enum Row<'a> {
    Group(&'static str),
    Setting(&'a SettingDef),
}

/// The rows a settings grid shows: every setting that `applies`, ungrouped first, then the
/// groups each under its heading: the sections in [`SECTION_ORDER`](bpe_core::SECTION_ORDER),
/// then any other groups (a thing kind's own) in the order they appear. A group with no row
/// shown has no heading.
pub fn rows_for<'a>(defs: &'a [SettingDef], applies: &dyn Fn(&SettingDef) -> bool) -> Vec<Row<'a>> {
    let shown: Vec<&SettingDef> = defs.iter().filter(|d| applies(d)).collect();
    let mut rows: Vec<Row<'a>> = shown.iter().filter(|d| d.group.is_empty()).map(|d| Row::Setting(d)).collect();
    let mut groups: Vec<&str> = Vec::new();
    for d in &shown {
        if !d.group.is_empty() && !groups.contains(&d.group) {
            groups.push(d.group);
        }
    }
    // A stable sort, so a paint's own variation stays beside the others, in appearance order.
    groups.sort_by_key(|g| section_rank(g).unwrap_or(usize::MAX));
    for group in groups {
        let members: Vec<Row<'a>> = shown.iter().filter(|d| d.group == group).map(|d| Row::Setting(d)).collect();
        if !members.is_empty() {
            rows.push(Row::Group(group));
            rows.extend(members);
        }
    }
    rows
}

/// The unit `def` shows. A gradient's "Colours end at" (a key ending in `span`) is a distance in
/// pixels when its paint follows Place or Start; otherwise the def's own unit.
pub fn unit_for(def: &SettingDef, value_of: &dyn Fn(&str) -> Option<Value>) -> &'static str {
    if let Some(prefix) = def.key.strip_suffix("span")
        && let Some(Value::Choice(follow)) = value_of(&format!("{prefix}follow"))
        && matches!(follow.as_str(), "place" | "start")
    {
        return "px";
    }
    def.unit
}

/// Whether a group of `defs` holds changes: a shown row whose value differs from its default,
/// or that has keys or a pulse.
fn group_changed(defs: &[SettingDef], group: &str, get: &dyn Fn(&str) -> Value, cx: &GridCx) -> bool {
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

/// What a setting's keys look like, for its label and its right-click menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMark {
    /// Keys are allowed on the setting.
    pub can_key: bool,
    /// The setting has keys.
    pub keyed: bool,
    /// The setting pulses with the hits.
    pub pulsed: bool,
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
struct Section<'a> {
    group: &'static str,
    defs: Vec<&'a SettingDef>,
}

/// The sections of [`rows_for`], placed by [`section_rank`].
fn sections_for<'a>(defs: &'a [SettingDef], cx: &GridCx) -> Vec<Section<'a>> {
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
type TabRows<'a> = dyn FnMut(&mut Ui, egui::Id, &mut Value) -> bool + 'a;

/// What a gradient's own row has beyond its widget: its tabs, its companions as they read now
/// (for "Copy gradient with its settings", each by its name) and where a paste puts the
/// companions it brought.
struct GradientRow<'r, 't> {
    tabs: &'r mut TabRows<'t>,
    companions: &'r dyn Fn() -> Vec<(&'static str, Value)>,
    pasted: &'r mut Vec<(&'static str, Value)>,
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

/// How wide the room is that [`grid_row_with`] keeps at the end of a widget cell.
pub const TRAILING_W: f32 = 44.0;

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
        RowAction::Follow(source) => out.follows.push((def.key, source)),
    }
}

/// What one grid row asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowAction {
    Nothing,
    /// `value` holds the new value: edited, put back to its default, or pasted.
    Changed,
    /// A key at the playhead, holding the value as it stands.
    CreateKey,
    /// Follow this part of the sound (or stop following, with `None`).
    Follow(Option<bpe_core::PulseSource>),
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

/// One grid row for `def`, in `unit`: the label, wrapped to the grid's label width (help on
/// hover, in the accent colour when `mark` says the setting has keys or a pulse; a double-click
/// or Alt+click puts the default back), and the widget for its type, ending by `right`.
///
/// A right-click on the label or the widget opens the row's menu ([`row_menu`]) and never
/// changes the value, though sliders and number boxes otherwise move under any mouse button.
/// A colour or gradient keeps its own clicks, so only its label has the menu.
///
/// What was asked of its varying across the balls is added to `varies`: the menu's Vary across
/// balls or Stop varying, and, while it varies, its two-ended slider (or a To row under it for a
/// pair, colour or gradient) and the "Varies by" line.
#[allow(clippy::too_many_arguments)]
fn setting(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    mark: Option<KeyMark>,
    cx: &GridCx,
    right: f32,
    varies: &mut Vec<crate::vary::VaryEdit>,
    mut grad_row: Option<GradientRow>,
    trailing: Option<&mut dyn FnMut(&mut Ui)>,
) -> RowAction {
    use crate::vary::VaryEdit;
    // Where a gradient's bar and its tabs keep their memory (the selected stop, the open tab):
    // under the grid and the setting, so it holds however the rows around it change.
    let bar = ui.scope_id().with(("gradient bar", def.key));
    let offered = cx.vary.and_then(|v| v.use_of(def).map(|u| (v, u)));
    let now = offered.and_then(|(v, _)| v.now(def.key));
    let mut to = now.as_ref().and_then(|n| n.to.clone());
    let two_ended = matches!((&def.ty, def.range.or(def.span)), (ValueType::Number | ValueType::Int, Some(_)));
    let ink = label_colour(mark, cx.track);
    let differs = *value != def.default;
    let name = label_cell(ui, cx.label_width, |ui| {
        // The cell's full height, so the label is centred in it rather than hanging from its middle.
        let room = ui.available_size();
        ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            // A ring before the label when this thing changed the setting from its default.
            let (dot, _) = ui.allocate_exact_size(Vec2::new(6.0, 10.0), Sense::hover());
            if differs {
                style::changed_dot(ui.painter(), dot.center());
            }
            label(ui, def.label, def.help, ink)
        })
        .inner
    });
    let before = value.clone();
    let mut changed = false;
    let mut to_response: Option<Response> = None;
    let response = widget_cell(ui, right, |ui| {
        let mut draw = |ui: &mut Ui| {
            style::wells(ui, |ui| match (two_ended, to.as_mut()) {
                (true, Some(end)) => {
                    let range = def.range.or(def.span).unwrap_or((0.0, 1.0));
                    let feel = setting_feel(def).with_snap(setting_snapper(ui, def, unit));
                    let (a, b) = crate::vary::two_ended(ui, def, (value, end), range, feel, suffix(unit), name.id);
                    to_response = Some(b);
                    a
                }
                _ => widget(ui, def, unit, value, name.id, bar, cx.choices, grad_row.is_some()),
            })
        };
        let r = match trailing {
            Some(trailing) => {
                // The widget in all but the room kept for the trailing control.
                let room = (ui.available_width() - TRAILING_W).max(40.0);
                let layout = egui::Layout::left_to_right(egui::Align::Center);
                let r = ui
                    .allocate_ui_with_layout(Vec2::new(room, 0.0), layout, |ui| {
                        ui.set_max_width(room);
                        draw(ui)
                    })
                    .inner;
                trailing(ui);
                r
            }
            None => draw(ui),
        };
        changed |= r.changed();
        r
    });
    let own_clicks = matches!(def.ty, ValueType::Color | ValueType::Gradient);
    if changed && !own_clicks && right_button_busy(ui) {
        *value = before.clone();
        changed = false;
    }
    // A gradient bar's clicks add and pick stops, so only its label resets it.
    let fallback = def.default.clone();
    let gradient = def.ty == ValueType::Gradient;
    // Alt on the widget itself drags freely past the grid, so there only a double-click resets.
    if resettable(&name, value, &fallback, true) || (!gradient && resettable(&response, value, &fallback, false)) {
        changed = true;
    }
    let follow = cx.follow.map(|f| (f, def.key));
    if let Some(now) = follow.and_then(|(f, key)| (f.now)(key)) {
        follow_band(ui, def, value, &response, &now);
    }
    // Vary across balls (or Stop varying) under the menu's other items.
    let vary_pick = std::cell::Cell::new(None::<VaryEdit>);
    let mut vary_items = |ui: &mut Ui| {
        if offered.is_none() {
            return;
        }
        if now.is_some() || !cx.vary.is_some_and(|v| v.offered) {
            if now.is_some() && ui.button("Stop varying").on_hover_text("Every ball takes this value again").clicked() {
                vary_pick.set(Some(VaryEdit::Stop));
                ui.close();
            }
        } else if ui
            .button("Vary across balls")
            .on_hover_text("Give each ball its own value, between this one and another")
            .clicked()
        {
            vary_pick.set(Some(VaryEdit::Start));
            ui.close();
        }
    };
    let with = grad_row.as_ref().map(|g| g.companions);
    let mut picks =
        vec![row_menu_following(&name, def, unit, &before, mark, cx.choices, Some(&mut vary_items), follow, with)];
    if !own_clicks {
        picks.push(row_menu_following(
            &response,
            def,
            unit,
            &before,
            mark,
            cx.choices,
            Some(&mut vary_items),
            follow,
            with,
        ));
    }
    varies.extend(vary_pick.take());
    ui.end_row();
    // A gradient's tabs, in rows under it.
    if let Some(row) = grad_row.as_mut()
        && (row.tabs)(ui, bar, value)
    {
        changed = true;
    }
    if let (Some((vcx, use_)), Some(v), Some(end)) = (offered, now.as_ref(), to.as_mut()) {
        let to_mark = vcx.to_mark(def);
        let end_before = end.clone();
        // A pair, colour or gradient sets its To in a row of its own.
        if !two_ended {
            let to_name = label_cell(ui, cx.label_width, |ui| {
                let room = ui.available_size();
                ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(9.0);
                    let words = format!("{} to", def.label);
                    let r = ui.add(egui::Label::new(RichText::new("to").color(label_colour(Some(to_mark), cx.track))));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words.clone()));
                    r.on_hover_text("The other end: what the last ball (or the far end of the spread) gets")
                })
                .inner
            });
            let r = widget_cell(ui, right, |ui| {
                style::wells(ui, |ui| widget(ui, def, unit, end, to_name.id, bar.with("to"), cx.choices, false))
            });
            if r.changed() {
                varies.push(VaryEdit::To(end.clone()));
            }
            to_response = Some(to_name);
            ui.end_row();
        } else if to_response.as_ref().is_some_and(|r| r.changed()) && !right_button_busy(ui) {
            varies.push(VaryEdit::To(end.clone()));
        }
        if let Some(r) = &to_response {
            match row_menu(r, def, unit, &end_before, Some(to_mark), cx.choices, None) {
                MenuPick::Paste(pasted) => varies.push(VaryEdit::To(pasted)),
                MenuPick::CreateKey => varies.push(VaryEdit::KeyTo),
                MenuPick::Nothing | MenuPick::Follow(_) => {}
            }
        }
        label_cell(ui, cx.label_width, |_| {});
        widget_cell(ui, right, |ui| {
            varies.extend(crate::vary::varies_line(ui, def, v, use_, vcx.rounds(), vcx.lit(def), vcx.healthy()));
        });
        ui.end_row();
    }
    let mut action = if changed { RowAction::Changed } else { RowAction::Nothing };
    for pick in picks {
        match pick {
            MenuPick::Nothing => {}
            MenuPick::Paste(pasted) => {
                *value = pasted;
                action = RowAction::Changed;
                // A gradient copied with its settings brings the ones this row has too.
                if let Some(row) = grad_row.as_mut()
                    && let Some(copy) = copied(ui.ctx())
                {
                    row.pasted.extend(companions_pasted(def, &copy.extras));
                }
            }
            MenuPick::CreateKey => action = RowAction::CreateKey,
            MenuPick::Follow(source) => action = RowAction::Follow(source),
        }
    }
    action
}

/// The companions of `extras` (each by its name, as "Copy gradient with its settings" copied
/// them) that gradient setting `def` has, each made to fit; the others are left out.
fn companions_pasted(def: &SettingDef, extras: &[(&'static str, Value)]) -> Vec<(&'static str, Value)> {
    extras
        .iter()
        .filter_map(|(name, v)| {
            let c = bpe_core::gradient_extras::companions_for(def).find(|c| c.name == *name)?;
            let v = pasted(&c.def, v)?;
            Some((c.name, v))
        })
        .collect()
}

/// The band under a following setting's slider: from its value to where the sound takes it at
/// full level, in the colour of what it follows, named "Follows <what>" for the screen reader.
fn follow_band(ui: &mut Ui, def: &SettingDef, value: &Value, response: &Response, now: &crate::follow::Following) {
    let rail = ui.ctx().data(|d| d.get_temp::<egui::Rect>(response.id.with("rail"))).unwrap_or(response.rect);
    let x = match value {
        Value::Number(x) => *x,
        Value::Int(x) => *x as f64,
        _ => 0.0,
    };
    let (lo, hi) = def.span.or(def.range).unwrap_or((x.min(x + now.amount), x.max(x + now.amount)));
    let at = |v: f64| {
        let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        rail.left() + rail.width() * t as f32
    };
    let (a, b) = (at(x), at(x + now.amount));
    let band =
        egui::Rect::from_x_y_ranges(a.min(b)..=a.max(b).max(a.min(b) + 2.0), rail.bottom() - 3.0..=rail.bottom());
    let r = ui.interact(band, response.id.with("follow band"), Sense::hover());
    let words = format!("Follows {}", now.words);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, words.clone()));
    r.on_hover_text(&words);
    ui.painter().rect_filled(band, 1.5, now.colour);
}

/// Whether the right mouse button is down or was just let go: a slider or number box under it is
/// opening the row's menu, not being dragged.
pub fn right_button_busy(ui: &Ui) -> bool {
    ui.input(|i| {
        let right = PointerButton::Secondary;
        i.pointer.button_down(right) || i.pointer.button_released(right) || i.pointer.button_pressed(right)
    })
}

/// A value copied from a setting's menu: the value and how it reads in the menu's Paste Value.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    pub value: Value,
    pub words: String,
    /// A gradient copied with its settings: each companion's name and value. Empty for every
    /// other copy.
    pub extras: Vec<(&'static str, Value)>,
}

/// Where the copied value lives: the editor's own clipboard, shared by every card and thing,
/// until the editor closes.
fn clipboard_id() -> egui::Id {
    egui::Id::unique("setting value clipboard")
}

/// The value last copied from a setting's menu, if any.
pub fn copied(ctx: &egui::Context) -> Option<Copied> {
    ctx.data(|d| d.get_temp::<Copied>(clipboard_id()))
}

/// Puts `value` on the editor's clipboard.
pub fn copy_value(ctx: &egui::Context, value: Value, words: String) {
    copy_with(ctx, value, words, Vec::new());
}

/// Puts a gradient and its companions (`extras`, each by its name) on the editor's clipboard.
pub fn copy_with(ctx: &egui::Context, value: Value, words: String, extras: Vec<(&'static str, Value)>) {
    ctx.data_mut(|d| d.insert_temp(clipboard_id(), Copied { value, words, extras }));
}

/// What a row's menu asked for.
#[derive(Clone, Debug, PartialEq)]
enum MenuPick {
    Nothing,
    /// The copied value, made to fit the setting.
    Paste(Value),
    CreateKey,
    /// Follow sound ▸ …, or Stop following sound (`None`).
    Follow(Option<bpe_core::PulseSource>),
}

/// The right-click menu of one setting's row: Copy Value, Paste Value (naming what is copied) and
/// Create Keyframe. Paste is greyed out when nothing is copied or the copied value doesn't fit
/// this setting; Create Keyframe when the setting can't have keys.
fn row_menu(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) -> MenuPick {
    row_menu_following(response, def, unit, value, mark, choices, extra, None, None)
}

/// [`row_menu`] with Follow sound under Create Keyframe, when `follow` is given, and "Copy
/// gradient with its settings" under Copy Value when `with` gives a gradient's companions.
#[allow(clippy::too_many_arguments)]
fn row_menu_following(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    mut extra: Option<&mut dyn FnMut(&mut Ui)>,
    follow: Option<(&crate::follow::FollowCx, &str)>,
    with: Option<&dyn Fn() -> Vec<(&'static str, Value)>>,
) -> MenuPick {
    let mut pick = MenuPick::Nothing;
    let open = if response.container_secondary_clicked() {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.container_clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    // An id of its own: a menu button's own popup (a target or a choice) already uses the
    // response's usual popup id.
    let popup = egui::Popup::menu(response).id(response.id.with("setting menu")).open_memory(open).at_pointer_fixed();
    popup.show(|ui| {
        ui.set_min_width(160.0);
        if ui.button("Copy Value").on_hover_text("Copy this setting's value").clicked() {
            copy_value(ui.ctx(), value.clone(), value_words(value, unit, choices));
            ui.close();
        }
        if let Some(companions) = with {
            let help = "Copy the colours with how they blend, lie and move, to paste onto another gradient";
            if ui.button("Copy gradient with its settings").on_hover_text(help).clicked() {
                let words = format!("{} with its settings", value_words(value, unit, choices));
                copy_with(ui.ctx(), value.clone(), words, companions());
                ui.close();
            }
        }
        let clip = copied(ui.ctx());
        let text = match &clip {
            Some(c) => format!("Paste Value ({})", c.words),
            None => "Paste Value".to_owned(),
        };
        let fits = clip.as_ref().and_then(|c| pasted(def, &c.value));
        let why = if clip.is_none() { "Copy a value first" } else { "The copied value doesn't fit this setting" };
        let paste = ui.add_enabled(fits.is_some(), egui::Button::new(text)).on_disabled_hover_text(why);
        if paste.clicked()
            && let Some(v) = fits
        {
            pick = MenuPick::Paste(v);
            ui.close();
        }
        let can_key = mark.is_some_and(|m| m.can_key);
        let key = ui
            .add_enabled(can_key, egui::Button::new("Create Keyframe"))
            .on_hover_text("A keyframe at the playhead holding the value as it is")
            .on_disabled_hover_text("This setting can't have keyframes");
        if key.clicked() {
            pick = MenuPick::CreateKey;
            ui.close();
        }
        if let Some((f, key)) = follow {
            follow_items(ui, def, f, (f.now)(key), (f.light)(key), &mut pick);
        }
        if let Some(extra) = extra.as_mut() {
            extra(ui);
        }
    });
    pick
}

/// Follow sound ▸ (a shape ▸ its modulators and Level; the buses), and Stop following sound
/// when the setting follows already; then Follow light ▸ (a thing ▸ its light) with a Lighting
/// card, offered where `light_ok` (the lighting doesn't read the setting); then Follow health ▸
/// (a thing with a Health card).
fn follow_items(
    ui: &mut Ui,
    def: &SettingDef,
    f: &crate::follow::FollowCx,
    now: Option<crate::follow::Following>,
    light_ok: bool,
    pick: &mut MenuPick,
) {
    use bpe_core::{PulseSource, SoundSource};
    let can = crate::follow::can_follow(def);
    // A setting the lighting reads can't follow a sound that hears the light (one-way).
    let lighting_reads = !f.menu.lit.is_empty() && !light_ok;
    let shapes: Vec<_> = f.menu.shapes.iter().filter(|s| !(lighting_reads && s.hears_light)).collect();
    let buses: &[(u8, String)] =
        if lighting_reads && f.menu.shapes.iter().any(|s| s.hears_light) { &[] } else { &f.menu.buses };
    let any = !shapes.is_empty() || !buses.is_empty();
    let why = if !can {
        "This setting can't follow sound"
    } else if !f.menu.shapes.is_empty() {
        "The lighting reads this setting, and the sound hears the light"
    } else {
        "No shape makes sound yet"
    };
    ui.add_enabled_ui(can && any, |ui| {
        let r = ui.menu_button("Follow sound", |ui| {
            for shape in &shapes {
                ui.menu_button(&shape.name, |ui| {
                    for (part, name) in &shape.parts {
                        let source = PulseSource::Sound { shape: shape.id, source: *part };
                        let on = now.as_ref().is_some_and(|n| n.source == source);
                        if ui.add(egui::Button::selectable(on, name)).clicked() {
                            *pick = MenuPick::Follow(Some(source));
                            ui.close();
                        }
                    }
                });
            }
            for (n, name) in buses {
                let shape = f.menu.shapes.first().map_or(ThingId::ROOT, |s| s.id);
                let source = PulseSource::Sound { shape, source: SoundSource::Bus(*n) };
                let on = now.as_ref().is_some_and(
                    |x| matches!(x.source, PulseSource::Sound { source: SoundSource::Bus(m), .. } if m == *n),
                );
                let label = if name.trim().is_empty() { format!("Bus {n}") } else { name.clone() };
                if ui.add(egui::Button::selectable(on, label)).clicked() {
                    *pick = MenuPick::Follow(Some(source));
                    ui.close();
                }
            }
        });
        r.response.on_hover_text("Move this setting with a shape's sound").on_disabled_hover_text(why);
    });
    let following_light = now.as_ref().is_some_and(|n| matches!(n.source, PulseSource::Light { .. }));
    let following_health = now.as_ref().is_some_and(|n| matches!(n.source, PulseSource::Health { .. }));
    if now.is_some() && !following_light && !following_health && ui.button("Stop following sound").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
    if !f.menu.lit.is_empty() {
        let may = can && light_ok;
        let why = if !can { "This setting can't follow light" } else { "The lighting reads this setting" };
        ui.add_enabled_ui(may, |ui| {
            let r = ui.menu_button("Follow light", |ui| {
                for thing in &f.menu.lit {
                    ui.menu_button(&thing.name, |ui| {
                        for (part, name) in &thing.parts {
                            let source = PulseSource::Light { thing: thing.id, source: *part };
                            let on = now.as_ref().is_some_and(|n| n.source == source);
                            if ui.add(egui::Button::selectable(on, *name)).clicked() {
                                *pick = MenuPick::Follow(Some(source));
                                ui.close();
                            }
                        }
                    });
                }
            });
            r.response.on_hover_text("Move this setting with how lit a thing is").on_disabled_hover_text(why);
        });
    }
    if following_light && ui.button("Stop following light").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
    if !f.menu.health.is_empty() {
        ui.add_enabled_ui(can, |ui| {
            let r = ui.menu_button("Follow health", |ui| {
                for (thing, name) in &f.menu.health {
                    let source = PulseSource::Health { thing: *thing };
                    let on = now.as_ref().is_some_and(|n| n.source == source);
                    if ui.add(egui::Button::selectable(on, name)).clicked() {
                        *pick = MenuPick::Follow(Some(source));
                        ui.close();
                    }
                }
            });
            r.response
                .on_hover_text("Move this setting with how much health a thing has left")
                .on_disabled_hover_text("This setting can't follow health");
        });
    }
    if following_health && ui.button("Stop following health").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
}

/// What the value menu on a control outside a settings grid (a knob) asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ValueMenu {
    Nothing,
    Paste(Value),
    CreateKey,
}

/// The same right-click menu as a settings row's (Copy Value, Paste Value, Create Keyframe), on
/// any control.
pub(crate) fn value_menu(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
) -> ValueMenu {
    value_menu_with(response, def, unit, value, mark, choices, None)
}

/// [`value_menu`] with more items under Create Keyframe, drawn by `extra` (Modulate by).
pub(crate) fn value_menu_with(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) -> ValueMenu {
    match row_menu(response, def, unit, value, mark, choices, extra) {
        MenuPick::Nothing | MenuPick::Follow(_) => ValueMenu::Nothing,
        MenuPick::Paste(v) => ValueMenu::Paste(v),
        MenuPick::CreateKey => ValueMenu::CreateKey,
    }
}

/// `copied` as a value of `def`, kept inside its range, or `None` when it is another kind of
/// value or a choice this setting doesn't have.
fn pasted(def: &SettingDef, copied: &Value) -> Option<Value> {
    let value = match (&def.ty, copied) {
        // A whole number pastes into a number and back, rounded.
        (ValueType::Number, Value::Int(i)) => Value::Number(*i as f64),
        (ValueType::Int, Value::Number(x)) if x.is_finite() => Value::Int(x.round() as i64),
        // A colour pastes into colours as one, and a one-colour gradient into a colour.
        (ValueType::Gradient, Value::Color(c)) => Value::Gradient(bpe_core::Gradient::solid(*c)),
        (ValueType::Color, Value::Gradient(g)) if g.stops.len() == 1 => Value::Color(g.stops[0].color),
        _ => copied.clone(),
    };
    def.ty.accepts(&value).then(|| def.clamp(value))
}

/// A value in a few words for the menu's Paste Value: a number with its unit, a colour as hex,
/// a choice as its words, On or Off, a gradient's colours.
pub fn value_words(value: &Value, unit: &str, choices: &TargetChoices) -> String {
    let number = |x: f64| {
        let text = format!("{x:.3}");
        let text = text.trim_end_matches('0').trim_end_matches('.');
        if text == "-0" { "0".to_owned() } else { text.to_owned() }
    };
    let hex = |c: &Color| {
        if c.a == 255 {
            format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
        }
    };
    match value {
        Value::Number(x) => format!("{}{}", number(*x), suffix(unit)),
        Value::Int(i) => format!("{i}{}", suffix(unit)),
        Value::Bool(b) => if *b { "On" } else { "Off" }.to_owned(),
        Value::Vec2([x, y]) => format!("{}, {}{}", number(f64::from(*x)), number(f64::from(*y)), suffix(unit)),
        Value::Color(c) => hex(c),
        Value::Gradient(g) => match g.stops.as_slice() {
            [one] => hex(&one.color),
            stops => format!("{} colours", stops.len()),
        },
        Value::Choice(c) => pretty(c),
        Value::Text(t) if t.chars().count() > 24 => format!("\"{}…\"", t.chars().take(23).collect::<String>()),
        Value::Text(t) => format!("\"{t}\""),
        Value::Target(t) => target_words(t, choices),
        Value::Strokes(s) => strokes_words(s),
        Value::Points(ps) if ps.len() == 1 => "1 point".to_owned(),
        Value::Points(ps) => format!("{} points", ps.len()),
        Value::Path(p) => path_words(p),
        Value::Routes(r) if r.len() == 1 => "1 route".to_owned(),
        Value::Routes(r) => format!("{} routes", r.len()),
        Value::Slots(n) if n.len() == 1 => "1 slot".to_owned(),
        Value::Slots(n) => format!("{} slots", n.len()),
        Value::Curve(c) => format!("{} points", c.len()),
        Value::LightScenes(l) => scenes_words(l.len()),
        Value::Who(w) => who_words(w, choices),
    }
}

/// A rule block's Who in a few words, naming things and kinds as the scene does: "That ball",
/// "Bricks", "Balls, ball 3", "Tagged breakable", "Any Shape".
pub fn who_words(w: &bpe_core::Who, choices: &TargetChoices) -> String {
    use bpe_core::Who;
    let thing = |id: &ThingId| match choices.things.iter().find(|(t, _)| t == id) {
        Some((_, name)) => name.clone(),
        None => format!("A missing thing (id {})", id.0),
    };
    match w {
        Who::That => "That ball".to_owned(),
        Who::Other => "The thing it hit".to_owned(),
        Who::Group => "That ball's group".to_owned(),
        Who::Thing { thing: id, ball: Some(n) } => format!("{}, ball {n}", thing(id)),
        Who::Thing { thing: id, ball: None } => thing(id),
        Who::Tag(t) => format!("Tagged {t}"),
        Who::Kind(k) => {
            let name = choices.kinds.iter().find(|(id, _)| id == k).map(|(_, n)| n.as_str()).unwrap_or(k);
            format!("Any {name}")
        }
        Who::Anything => "Anything".to_owned(),
    }
}

/// A rule block's who: a menu naming it, with the firing's own parties (when `choices.relative`),
/// Anything, and the scene's things, tags and kinds. A thing with balls gets a "ball" number box
/// beside it (0 = the whole thing). Marked changed when something is picked.
pub(crate) fn who_picker(ui: &mut Ui, w: &mut bpe_core::Who, choices: &TargetChoices) -> Response {
    use bpe_core::Who;
    let mut picked: Option<Who> = None;
    let menu = ui.menu_button(who_words(w, choices), |ui| {
        ui.set_min_width(170.0);
        let mut item = |ui: &mut Ui, who: Who, text: &str, help: &str| {
            if ui.selectable_label(*w == who, text).on_hover_text(help).clicked() {
                picked = Some(who);
                ui.close();
            }
        };
        if choices.relative {
            item(ui, Who::That, "That ball", "The ball (or thing) this firing is about");
            item(ui, Who::Other, "The thing it hit", "The other side of the firing");
            item(ui, Who::Group, "That ball's group", "Every ball of that ball's thing");
        }
        item(ui, Who::Anything, "Anything", "Every thing");
        ui.menu_button("Thing", |ui| {
            for (id, name) in &choices.things {
                let on = matches!(w, Who::Thing { thing, .. } if thing == id);
                if ui.selectable_label(on, name).clicked() {
                    picked = Some(Who::Thing { thing: *id, ball: None });
                    ui.close();
                }
            }
        });
        if choices.tags.is_empty() {
            ui.add_enabled(false, egui::Button::new("Tagged"))
                .on_disabled_hover_text("No tags yet. Give things tags in their header.");
        } else {
            ui.menu_button("Tagged", |ui| {
                for tag in &choices.tags {
                    if ui.selectable_label(matches!(w, Who::Tag(x) if x == tag), tag).clicked() {
                        picked = Some(Who::Tag(tag.clone()));
                        ui.close();
                    }
                }
            });
        }
        ui.menu_button("Any", |ui| {
            for (id, name) in &choices.kinds {
                if ui.selectable_label(matches!(w, Who::Kind(x) if x == id), name).clicked() {
                    picked = Some(Who::Kind(id.clone()));
                    ui.close();
                }
            }
        });
    });
    let mut response = menu.response.on_hover_text("Who this block is about");
    if let Some(p) = picked {
        *w = p;
        response.mark_changed();
    }
    // One ball of a thing with balls, by its number (0: the whole thing).
    if let Who::Thing { thing, ball } = w
        && choices.ball_things.contains(thing)
    {
        let mut n = ball.unwrap_or(0);
        let number = ui
            .add(
                DragValue::new(&mut n)
                    .range(0..=u32::MAX)
                    .speed(0.1)
                    .custom_formatter(|v, _| if v < 0.5 { "every ball".to_owned() } else { format!("ball {v:.0}") })
                    .custom_parser(|t| {
                        let t = t.trim().to_lowercase();
                        if t.starts_with("every") {
                            return Some(0.0);
                        }
                        t.trim_start_matches("ball").trim().parse::<f64>().ok()
                    }),
            )
            .on_hover_text("One ball of it, by its number; 0 for every ball");
        if number.changed() {
            *ball = (n > 0).then_some(n);
            response.mark_changed();
        }
        response |= number;
    }
    response
}

/// "Even" for a shape that isn't moulded, else its corners and curves: "7 points, 1 curve".
pub fn path_words(p: &bpe_core::ShapePath) -> String {
    if p.is_empty() { "Even".to_owned() } else { p.words() }
}

/// "No strokes", "1 stroke", "3 strokes".
pub fn strokes_words(s: &bpe_core::Strokes) -> String {
    match s.list.len() {
        0 => "No strokes".to_owned(),
        1 => "1 stroke".to_owned(),
        n => format!("{n} strokes"),
    }
}

/// The label cell, clickable for the reset shortcut, in the accent colour when `lit`.
fn label(ui: &mut Ui, text: &str, help: &str, ink: Color32) -> Response {
    let text = RichText::new(text).color(ink);
    let response = ui.add(egui::Label::new(text).wrap().selectable(false).sense(Sense::click()));
    if help.is_empty() { response } else { response.on_hover_text(help) }
}

/// Whether the response was double-clicked or (with `alt_too`) Alt+clicked, which puts a setting
/// back.
fn reset_clicked(response: &Response, alt_too: bool) -> bool {
    let primary = egui::PointerButton::Primary;
    // egui counts a double-click that comes soon after a click elsewhere as a triple-click.
    let (alt, clicked, many_clicks) = response.ctx.input(|i| {
        let many = i.pointer.button_double_clicked(primary) || i.pointer.button_triple_clicked(primary);
        (i.modifiers.alt, i.pointer.button_clicked(primary), many)
    });
    // Sliders only sense drags, so a click on one shows up as a drag that stops where it started.
    let clicked = response.clicked() || (response.drag_stopped() && clicked);
    (clicked && alt && alt_too) || (clicked && many_clicks && !response.ctx.text_edit_focused())
}

/// Puts `value` back to `fallback` on a double-click or (with `alt_too`) Alt+click. True when it
/// changed.
fn resettable(response: &Response, value: &mut Value, fallback: &Value, alt_too: bool) -> bool {
    if reset_clicked(response, alt_too) && value != fallback {
        *value = fallback.clone();
        true
    } else {
        false
    }
}

pub fn suffix(unit: &str) -> String {
    if unit.is_empty() { String::new() } else { format!(" {unit}") }
}

/// The widget for `def`'s type, editing `value` in place. Every control is labelled by the
/// row's name, for screen readers and tests.
/// How a number moves under the mouse: the step it snaps to, how many decimals it shows, and
/// how far a drag on its number box goes per pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberFeel {
    pub step: f64,
    pub decimals: usize,
    pub drag_speed: f64,
    /// Where drags land while the grid's Snap is on: none for a setting that doesn't measure a
    /// distance or an angle, or while snapping is off or Alt is held.
    pub snap: Option<crate::setting_snap::Snapper>,
}

impl NumberFeel {
    /// This feel snapping drags with `snap`, showing enough decimals for its steps.
    pub fn with_snap(mut self, snap: Option<crate::setting_snap::Snapper>) -> Self {
        if let Some(s) = snap {
            self.decimals = self.decimals.max(s.decimals());
        }
        self.snap = snap;
        self
    }
}

/// The feel of a number spanning `span`: the step is the power of ten nearest a two-hundredth of
/// the span, but never more than 1, so every slider moves in round, even steps (0 to 1 by
/// 0.001, 0 to 40 by 0.1, 0 to 2000 and 1 to 5000 by 1). A drag on the number box crosses a
/// small span in about 400 pixels and counts a wide one in ones (Shift is ten times finer).
/// The cap matters for typing too: the slider snaps what is typed to its steps.
pub fn number_feel(span: f64, integer: bool) -> NumberFeel {
    let span = if span.is_finite() && span > 0.0 { span } else { 1.0 };
    let power = (span / 200.0).log10().floor().min(0.0);
    let mut step = 10f64.powf(power).max(1e-4);
    if integer {
        step = step.max(1.0).round();
    }
    let decimals = if integer { 0 } else { (-power).max(0.0) as usize };
    let drag_speed = (span / 400.0).min(1.0).max(step / 4.0);
    NumberFeel { step, decimals, drag_speed, snap: None }
}

/// The feel of a setting's number: from its range, else its usual span, else from its unit
/// (pixels move by whole pixels, other things by hundredths).
pub fn setting_feel(def: &SettingDef) -> NumberFeel {
    let integer = def.ty == ValueType::Int;
    // A slider covers the usual span when there is one, so it moves in the span's steps.
    match def.span.or(def.range) {
        Some((min, max)) => number_feel(max - min, integer),
        None if def.unit.starts_with("px") => number_feel(400.0, integer),
        None => number_feel(2.0, integer),
    }
}

/// A slider for a number setting: as wide as the row allows, moving in the setting's even steps,
/// with its number box beside it.
pub fn number_slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    x: &mut N,
    (min, max): (f64, f64),
    feel: NumberFeel,
    unit: String,
) -> Response {
    // Leave room for the number box at its widest, so the row never pushes the panel wider.
    let widest = format!("{}{:.*}{unit}", if min < 0.0 { "-" } else { "" }, feel.decimals, min.abs().max(max.abs()));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let number = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, style::TEXT).size().x);
    let room = number + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x + 4.0;
    ui.spacing_mut().slider_width = (ui.available_width() - room).clamp(48.0, 320.0);
    let mut slider = Slider::new(x, N::from_f64(min)..=N::from_f64(max))
        .suffix(unit)
        .smart_aim(false)
        .step_by(feel.step)
        .fixed_decimals(feel.decimals)
        .drag_value_speed(feel.drag_speed)
        // Only edits snap and clamp: a stored value is shown as it is, never rewritten by drawing it.
        .clamping(egui::SliderClamping::Edits);
    // A range spanning more than a thousandfold is easier to use on a log scale.
    if min > 0.0 && max / min > 1000.0 {
        slider = slider.logarithmic(true);
    }
    // The rail left of the knob is a lighter grey; nothing else here uses the selection colour.
    let width = ui.spacing().slider_width;
    let r = ui
        .scope(|ui| {
            ui.visuals_mut().selection.bg_fill = style::SLIDER_FILL;
            ui.add(slider.trailing_fill(true))
        })
        .inner;
    // Where the rail is, for a following setting's band.
    let rail = egui::Rect::from_min_size(r.rect.min, Vec2::new(width.min(r.rect.width()), r.rect.height()));
    ui.ctx().data_mut(|d| d.insert_temp(r.id.with("rail"), rail));
    if let Some(snap) = feel.snap
        && !(min > 0.0 && max / min > 1000.0)
    {
        // egui's handle travels the rail inset by its radius (`SliderGeometry::position_range`).
        let mut inset = rail.height() / 2.5;
        if let egui::style::HandleShape::Rect { aspect_ratio } = ui.style().visuals.handle_shape {
            inset *= aspect_ratio;
        }
        let x = rail.left() + inset..=rail.right() - inset;
        snap_ticks(&ui.painter_at(rail.expand(8.0)), snap, (min, max), x, rail.center().y);
    }
    r
}

/// How far past its usual span a value is: below it (-1), within it (0) or above it (1).
pub fn past_span(value: f64, (lo, hi): (f64, f64)) -> i8 {
    if value < lo.min(hi) {
        -1
    } else if value > lo.max(hi) {
        1
    } else {
        0
    }
}

/// A slider over a setting's usual `span` whose value can go past it (the lighting spec's "no
/// limits"): dragging the knob moves the value by how far the pointer goes, at the span's rate,
/// so it keeps going past either end; a click on the rail jumps there; the box beside it takes
/// any number. Only the hard `range`, when there is one, clamps. Past the span the knob sits at
/// that end with a small arrow beyond it, and the box is bright with a light border. Both parts
/// are named by `name`. Integer settings move and land on whole numbers.
#[allow(clippy::too_many_arguments)]
pub fn span_slider(
    ui: &mut Ui,
    value: &mut f64,
    span: (f64, f64),
    range: Option<(f64, f64)>,
    feel: NumberFeel,
    integer: bool,
    unit: String,
    name: Named,
) -> Response {
    let (lo, hi) = (span.0.min(span.1), span.0.max(span.1));
    let clamp = |v: f64| {
        let v = if integer { v.round() } else { v };
        match range {
            Some((a, b)) => v.clamp(a.min(b), a.max(b)),
            None => v,
        }
    };
    // Drags, clicks and keys land on the slider's steps, or on the grid while it snaps.
    let fit = |v: f64| match feel.snap {
        Some(s) => clamp(s.snap(v)),
        None => clamp(if feel.step > 0.0 { (v / feel.step).round() * feel.step } else { v }),
    };
    // Room for the number box at its widest within the span, so the row never pushes the panel wider.
    let widest = format!("{}{:.*}{unit}", if lo < 0.0 { "-" } else { "" }, feel.decimals, lo.abs().max(hi.abs()));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let number = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, style::TEXT).size().x);
    let room = number + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x + 10.0;
    let width = (ui.available_width() - room).clamp(48.0, 320.0);
    let height = ui.spacing().interact_size.y;
    let (rect, mut rail) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let before = *value;
    let rate = (hi - lo) / width.max(1.0) as f64;
    let start_id = rail.id.with("drag start");
    if rail.drag_started_by(PointerButton::Primary) {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, (*value, 0.0f64)));
    }
    if rail.dragged_by(PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let (start, moved) = ui.ctx().data(|d| d.get_temp::<(f64, f64)>(start_id)).unwrap_or((*value, 0.0));
        let moved = moved + rail.drag_delta().x as f64 * fine;
        ui.ctx().data_mut(|d| d.insert_temp(start_id, (start, moved)));
        *value = fit(start + moved * rate);
    } else if rail.clicked()
        && let Some(p) = rail.interact_pointer_pos()
    {
        let t = ((p.x - rect.left()) / width.max(1.0)).clamp(0.0, 1.0) as f64;
        *value = fit(lo + t * (hi - lo));
    }
    // Keys nudge it by a step, past the ends too.
    if rail.has_focus() {
        let (left, right) =
            ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft) as i32, i.key_pressed(egui::Key::ArrowRight) as i32));
        if left != right {
            let dir = (right - left) as f64;
            *value = match feel.snap {
                Some(s) => clamp(s.nudge(*value, dir)),
                None => fit(*value + dir * feel.step.max(1e-9)),
            };
        }
    }
    if *value != before {
        rail.mark_changed();
    }
    let past = past_span(*value, (lo, hi));
    let t = if hi > lo { ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let painter = ui.painter_at(rect.expand(8.0));
    let y = rect.center().y;
    let left = rect.left() + 1.0;
    let right = rect.right() - 7.0;
    let x = left + (right - left) * t;
    let groove = egui::Rect::from_x_y_ranges(left..=right, y - 1.0..=y + 1.0);
    painter.rect_filled(groove, 0.0, style::LINE);
    if let Some(s) = feel.snap {
        snap_ticks(&painter, s, (lo, hi), left..=right, y);
    }
    let zero = if lo <= 0.0 && hi >= 0.0 && lo < 0.0 { left + (right - left) * (-lo / (hi - lo)) as f32 } else { left };
    let filled = egui::Rect::from_x_y_ranges(zero.min(x)..=zero.max(x), y - 1.0..=y + 1.0);
    painter.rect_filled(filled, 0.0, style::SLIDER_FILL);
    let hot = rail.hovered() || rail.dragged() || rail.has_focus();
    let knob = egui::Rect::from_center_size(egui::pos2(x, y), Vec2::new(3.0, if hot { 16.0 } else { 14.0 }));
    painter.rect_filled(knob, 0.0, style::TEXT_STRONG);
    if past != 0 {
        // A small arrow just past the end it has gone beyond.
        let (tip, back) = if past > 0 { (right + 7.0, right + 2.0) } else { (left - 7.0, left - 2.0) };
        painter.add(egui::Shape::convex_polygon(
            vec![egui::pos2(tip, y), egui::pos2(back, y - 4.0), egui::pos2(back, y + 4.0)],
            style::TEXT_STRONG,
            egui::Stroke::NONE,
        ));
    }
    let words = format!("{:.*}{unit}", feel.decimals, *value);
    rail.widget_info(|| egui::WidgetInfo::slider(true, *value, &words));
    let rail = name.on(ui, rail);
    // Where the rail is, for a following setting's band.
    let line = egui::Rect::from_x_y_ranges(left..=right, rect.top()..=rect.bottom());
    ui.ctx().data_mut(|d| d.insert_temp(rail.id.with("rail"), line));
    let mut typed = *value;
    let boxed = ui
        .scope(|ui| {
            if past != 0 {
                let w = &mut ui.visuals_mut().widgets;
                for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                    state.fg_stroke.color = style::TEXT_STRONG;
                    state.bg_stroke = egui::Stroke::new(1.0, style::TEXT);
                }
            }
            let mut field =
                DragValue::new(&mut typed).speed(feel.drag_speed).fixed_decimals(feel.decimals).suffix(unit.clone());
            if let Some((a, b)) = range {
                // A value already past the range (keyed or from a file) shows as it is.
                field = field.range(a.min(b)..=a.max(b)).clamp_existing_to_range(false);
            }
            ui.add(field)
        })
        .inner;
    let boxed = name.on(ui, boxed);
    // Dragging the box lands on the grid too; a typed number stays exact.
    crate::setting_snap::after_drag(ui, &boxed, *value, &mut typed, feel.snap, integer, range);
    if boxed.changed() {
        *value = if integer { typed.round() } else { typed };
    }
    if past != 0 {
        boxed.clone().on_hover_text("Past its usual span");
    }
    // The slider's own rail rect, for whoever reads `response.rect`.
    let mut both = rail.union(boxed.clone());
    if boxed.changed() {
        both.mark_changed();
    }
    both
}

/// Small ticks under a rail from `x` (the span `lo..hi` across it, centre line at `y`) where a
/// snapping slider's steps are, when they are far enough apart to read.
fn snap_ticks(
    painter: &egui::Painter,
    snap: crate::setting_snap::Snapper,
    (lo, hi): (f64, f64),
    x: std::ops::RangeInclusive<f32>,
    y: f32,
) {
    let (left, right) = (*x.start(), *x.end());
    let Some(gap) = snap.tick_gap(lo, hi, right - left) else { return };
    if hi <= lo {
        return;
    }
    let mut k = (lo / gap).ceil();
    while k * gap <= hi {
        let t = ((k * gap - lo) / (hi - lo)) as f32;
        let at = left + (right - left) * t;
        painter.vline(at, y + 3.0..=y + 6.0, egui::Stroke::new(1.0, style::FAINT));
        k += 1.0;
    }
}

/// What names a control for the screen reader (and kittest): the label beside it, or words.
#[derive(Clone, Copy, Debug)]
pub enum Named<'a> {
    By(egui::Id),
    Words(&'a str),
}

impl Named<'_> {
    fn on(self, ui: &Ui, r: Response) -> Response {
        match self {
            Named::By(id) => r.labelled_by(id),
            Named::Words(words) => {
                let words = words.to_owned();
                ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(words));
                r
            }
        }
    }
}

/// How `def`, shown in `unit`, snaps to the grid while dragged right now: never the video's own
/// size (a size of the picture, not a distance in it), nor a slider that couldn't stop at four
/// steps across its span (a 0 to 10 px width at a 60 px grid would only take 0 and 7.5).
fn setting_snapper(ui: &Ui, def: &SettingDef, unit: &str) -> Option<crate::setting_snap::Snapper> {
    if def.group == "Video" {
        return None;
    }
    let snap = crate::setting_snap::Snapper::for_unit(ui, unit, def.ty == ValueType::Vec2)?;
    match def.span.or(def.range) {
        Some((a, b)) if !snap.fits((a, b)) => None,
        _ => Some(snap),
    }
}

/// The span slider for a setting's number: over its usual span, clamped only by its hard range.
fn spanned(
    ui: &mut Ui,
    def: &SettingDef,
    x: &mut f64,
    span: (f64, f64),
    feel: NumberFeel,
    unit: String,
    name: egui::Id,
) -> Response {
    let integer = def.ty == ValueType::Int;
    span_slider(ui, x, span, def.range, feel, integer, unit, Named::By(name))
}

#[allow(clippy::too_many_arguments)]
fn widget(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    name: egui::Id,
    bar: egui::Id,
    choices: &TargetChoices,
    big: bool,
) -> Response {
    use crate::setting_snap::after_drag;
    // Distances and angles land on the grid while it snaps (and Alt isn't held).
    let snap = setting_snapper(ui, def, unit);
    let unit = suffix(unit);
    let feel = setting_feel(def).with_snap(snap);
    match (&def.ty, value) {
        (ValueType::Number, Value::Number(x)) => match (def.span, def.range) {
            (Some(span), _) => spanned(ui, def, x, span, feel, unit, name),
            (None, Some(range)) => {
                let before = *x;
                let r = number_slider(ui, x, range, feel, unit).labelled_by(name);
                after_drag(ui, &r, before, x, snap, false, Some(range));
                r
            }
            (None, None) => {
                let before = *x;
                let r = ui
                    .add(DragValue::new(x).speed(feel.drag_speed).fixed_decimals(feel.decimals).suffix(unit))
                    .labelled_by(name);
                after_drag(ui, &r, before, x, snap, false, None);
                r
            }
        },
        (ValueType::Int, Value::Int(x)) => match (def.span, def.range) {
            (Some(span), _) => {
                let mut v = *x as f64;
                let r = spanned(ui, def, &mut v, span, feel, unit, name);
                *x = v.round() as i64;
                r
            }
            (None, Some(range)) => {
                let before = *x as f64;
                let r = number_slider(ui, x, range, feel, unit).labelled_by(name);
                let mut v = *x as f64;
                if after_drag(ui, &r, before, &mut v, snap, true, Some(range)) {
                    *x = v as i64;
                }
                r
            }
            (None, None) => {
                let before = *x as f64;
                let r = ui.add(DragValue::new(x).speed(feel.drag_speed).suffix(unit)).labelled_by(name);
                let mut v = *x as f64;
                if after_drag(ui, &r, before, &mut v, snap, true, None) {
                    *x = v as i64;
                }
                r
            }
        },
        (ValueType::Bool, Value::Bool(b)) => style::switch(ui, b).labelled_by(name),
        // In a narrow panel y goes under x.
        (ValueType::Vec2, Value::Vec2(v)) => {
            ui.horizontal_wrapped(|ui| {
                let (speed, decimals) = (feel.drag_speed, feel.decimals);
                let before = *v;
                let x =
                    DragValue::new(&mut v[0]).speed(speed).fixed_decimals(decimals).prefix("x ").suffix(unit.clone());
                let x = ui.add(x).labelled_by(name);
                let y = DragValue::new(&mut v[1]).speed(speed).fixed_decimals(decimals).prefix("y ").suffix(unit);
                let y = ui.add(y).labelled_by(name);
                // A place lands on grid points while the grid snaps.
                for (i, r) in [&x, &y].into_iter().enumerate() {
                    let mut c = v[i] as f64;
                    if after_drag(ui, r, before[i] as f64, &mut c, snap, false, None) {
                        v[i] = c as _;
                    }
                }
                x | y
            })
            .inner
        }
        (ValueType::Color, Value::Color(c)) => color_button(ui, c).labelled_by(name),
        (ValueType::Gradient, Value::Gradient(g)) => {
            // The bar and its handles carry their own names, after the setting's.
            gradient_bar_named(ui, bar, g, def.label, big)
        }
        (ValueType::Choice(names), Value::Choice(current)) => {
            let mut response = style::raised(ui, |ui| {
                egui::ComboBox::from_id_salt(def.key).selected_text(pretty(current)).show_ui(ui, |ui| {
                    let mut picked = false;
                    for choice in names.iter() {
                        picked |= ui.selectable_value(current, (*choice).to_owned(), pretty(choice)).changed();
                    }
                    picked
                })
            });
            if response.inner.unwrap_or(false) {
                response.response.mark_changed();
            }
            response.response.labelled_by(name)
        }
        (ValueType::Text, Value::Text(text)) if def.unit == "picture" => {
            let width = (ui.available_width() - 40.0).max(80.0);
            let mut response = ui.add(egui::TextEdit::singleline(text).desired_width(width)).labelled_by(name);
            let browse = ui.add(egui::Button::new("…").small()).on_hover_text("Choose a picture (PNG or JPG)");
            browse.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Browse {}", def.label)));
            if browse.clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file()
            {
                *text = path.to_string_lossy().into_owned();
                response.mark_changed();
            }
            response
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
        (ValueType::Who, Value::Who(w)) => ui.horizontal(|ui| who_picker(ui, w, choices)).inner.labelled_by(name),
        // Strokes are painted in the ball view; here they are counted and can be cleared.
        (ValueType::Strokes, Value::Strokes(strokes)) => {
            let words = strokes_words(strokes);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(words).color(style::TEXT))).labelled_by(name);
                let clear = ui.add_enabled(!strokes.list.is_empty(), egui::Button::new("Clear").small());
                let label = format!("Clear {}", def.label);
                clear.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !strokes.list.is_empty(), &label));
                let mut clear = clear.on_hover_text("Takes away every stroke. Paint them in the ball view above.");
                if clear.clicked() {
                    strokes.list.clear();
                    clear.mark_changed();
                }
                clear
            })
            .inner
        }
        // A shape's outline is moulded in the video; the row says what it is and can start it afresh.
        (ValueType::Path, Value::Path(path)) => {
            let words = path_words(path);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(words).color(style::TEXT))).labelled_by(name);
                let moulded = !path.is_empty();
                let reset = ui.add_enabled(moulded, egui::Button::new("Reset").small());
                let label = format!("Reset {}", def.label);
                reset.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, moulded, &label));
                let mut reset = reset
                    .on_hover_text("Puts the even polygon back")
                    .on_disabled_hover_text("Drag corners and sides in the video to mould it");
                if reset.clicked() {
                    *path = bpe_core::ShapePath::default();
                    reset.mark_changed();
                }
                reset
            })
            .inner
        }
        // Points are placed by dragging them in a view; the row only says how many there are.
        (ValueType::Points, Value::Points(ps)) => {
            let words = if ps.len() == 1 { "1 point".to_owned() } else { format!("{} points", ps.len()) };
            ui.label(RichText::new(words).color(style::MUTED)).labelled_by(name)
        }
        // Lighting scenes are listed and edited under the Lighting card's settings.
        (ValueType::LightScenes, Value::LightScenes(list)) => {
            ui.label(RichText::new(scenes_words(list.len())).color(style::MUTED)).labelled_by(name)
        }
        _ => ui.label(RichText::new("This setting holds a value of the wrong kind").color(style::TEXT)),
    }
}

/// "No scenes", "1 scene", "3 scenes".
pub fn scenes_words(n: usize) -> String {
    match n {
        0 => "No scenes".to_owned(),
        1 => "1 scene".to_owned(),
        n => format!("{n} scenes"),
    }
}

/// A menu naming the target, with Anything, Itself and the scene's things, tags and kinds to
/// pick from. Marked changed when something is picked.
pub(crate) fn target_picker(ui: &mut Ui, t: &mut Target, choices: &TargetChoices) -> Response {
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
pub(crate) fn picker_color(c: Color) -> Color32 {
    if c.is_additive() {
        Color32::from_rgba_premultiplied(c.r, c.g, c.b, 0) // scene colour
    } else {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a) // scene colour
    }
}

/// The picker's colour as a scene colour. An additive one comes back as its colour with no
/// alpha, which is how `Color` keeps it.
pub(crate) fn scene_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}

pub(crate) fn color_button(ui: &mut Ui, value: &mut Color) -> Response {
    let mut c = picker_color(*value);
    let response = egui::color_picker::color_edit_button_srgba(ui, &mut c, Alpha::OnlyBlend);
    if response.changed() {
        *value = scene_color(c);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    use bpe_core::Settings;

    #[test]
    fn the_label_column_is_108_wide_and_96_when_narrow() {
        assert_eq!(label_width(200.0), 96.0);
        assert_eq!(label_width(329.0), 96.0);
        assert_eq!(label_width(330.0), 108.0);
        assert_eq!(label_width(500.0), 108.0);
    }

    #[test]
    fn a_keyed_label_takes_the_track_colour() {
        let track = Color32::from_rgb(0x3f, 0xd6, 0xb5);
        let keyed = KeyMark { can_key: true, keyed: true, pulsed: false };
        let pulsed = KeyMark { can_key: true, keyed: false, pulsed: true };
        assert_eq!(label_colour(Some(keyed), track), track);
        assert_eq!(label_colour(Some(pulsed), track), track);
        assert_eq!(label_colour(Some(KeyMark::default()), track), style::MUTED);
        assert_eq!(label_colour(None, track), style::MUTED);
    }

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
        assert_eq!(target_words(&Target::Kind("shape".into()), &c), "Kind: Shape");
        assert_eq!(target_words(&Target::Kind("blob".into()), &c), "Kind: blob");
        assert_eq!(c.tags, vec!["walls".to_owned()]);
        assert!(c.kinds.iter().all(|(id, _)| id != "scene"));
        assert_eq!(c.things[0], (ThingId::ROOT, "Scene settings".to_owned()));
    }

    #[test]
    fn names_are_made_readable() {
        assert_eq!(pretty("electric_piano"), "Electric piano");
        assert_eq!(pretty("box"), "Box");
        assert_eq!(pretty(""), "");
        assert_eq!(pretty("flat2_d"), "Flat 2D");
        assert_eq!(pretty("x2_delay"), "X2 delay");
    }

    mod bar {
        use crate::gradient_bar::gradient_bar;
        use bpe_core::{Color, Gradient, Stop};
        use eframe::egui::{self, Modifiers, PointerButton};
        use egui_kittest::{Harness, kittest::Queryable};

        const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
        const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };
        const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

        fn harness(g: Gradient) -> Harness<'static, Gradient> {
            let mut h = Harness::builder()
                .with_size(egui::vec2(420.0, 2600.0))
                .allow_missing_glyphs()
                .with_context_setup(crate::style::apply)
                .build_ui_state(
                    |ui, g: &mut Gradient| {
                        let id = ui.make_persistent_id("bar");
                        gradient_bar(ui, id, g);
                    },
                    g,
                );
            h.run();
            h
        }

        fn press(h: &mut Harness<'_, Gradient>, pos: egui::Pos2, button: PointerButton, pressed: bool) {
            h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        }

        /// Presses at `from`, moves to `to` and lets go there.
        fn drag(h: &mut Harness<'_, Gradient>, from: egui::Pos2, to: egui::Pos2) {
            h.hover_at(from);
            press(h, from, PointerButton::Primary, true);
            h.step();
            h.event(egui::Event::PointerMoved(from + (to - from) * 0.5));
            h.step();
            h.event(egui::Event::PointerMoved(to));
            h.step();
            press(h, to, PointerButton::Primary, false);
            h.run();
        }

        fn bar_rect(h: &Harness<'_, Gradient>) -> egui::Rect {
            h.get_by_label("Colours bar").rect()
        }

        fn ats(g: &Gradient) -> Vec<f32> {
            g.stops.iter().map(|s| s.at).collect()
        }

        #[test]
        fn clicking_the_bar_adds_a_stop() {
            let two = Gradient::even(&[RED, BLUE]);
            let mut h = harness(two.clone());
            let bar = bar_rect(&h);
            assert!(bar.height() >= 18.0 && bar.width() >= 200.0, "{bar:?}");
            let at = egui::pos2(bar.left() + bar.width() * 0.25, bar.center().y);
            h.hover_at(at);
            press(&mut h, at, PointerButton::Primary, true);
            h.step();
            press(&mut h, at, PointerButton::Primary, false);
            h.run();
            let g = h.state();
            assert_eq!(g.stops.len(), 3, "{g:?}");
            assert!((g.stops[1].at - 0.25).abs() < 0.01, "{g:?}");
            assert_eq!(g.stops[1].color, Color::from_linear(two.sample(g.stops[1].at)), "the sampled colour");
            assert_eq!((g.mix, g.looped), (two.mix, two.looped));
        }

        #[test]
        fn dragging_a_handle_moves_its_stop() {
            let mut h = harness(Gradient::even(&[RED, GREEN, BLUE]));
            let bar = bar_rect(&h);
            // Red, the first stop, dragged past green: the list re-sorts and red's handle,
            // now the second, keeps the focus.
            let from = h.get_by_label("Colours stop 1").rect().center();
            drag(&mut h, from, egui::pos2(bar.left() + bar.width() * 0.75, from.y));
            let g = h.state().clone();
            assert_eq!(g.stops.iter().map(|s| s.color).collect::<Vec<_>>(), [GREEN, RED, BLUE], "{g:?}");
            assert!((g.stops[1].at - 0.75).abs() < 0.02, "{g:?}");
            assert_eq!(g.stops[0].at, 0.5);
            assert!(h.get_by_label("Colours stop 2").is_focused(), "the dragged handle keeps the focus");
            // Past the end it stops at the end.
            let from = h.get_by_label("Colours stop 1").rect().center();
            drag(&mut h, from, egui::pos2(bar.left() - 60.0, from.y));
            assert_eq!(ats(h.state()), [0.0, 0.75, 1.0]);
            // Dragged well below the bar, it comes off.
            let from = h.get_by_label("Colours stop 2").rect().center();
            drag(&mut h, from, egui::pos2(from.x, bar.bottom() + 60.0));
            assert_eq!(h.state().stops.iter().map(|s| s.color).collect::<Vec<_>>(), [GREEN, BLUE]);
            // A little below, it stays.
            let from = h.get_by_label("Colours stop 2").rect().center();
            drag(&mut h, from, egui::pos2(from.x - 20.0, bar.bottom() + 20.0));
            assert_eq!(h.state().stops.len(), 2);
        }

        #[test]
        fn right_click_remove_keeps_at_least_one() {
            let mut h = harness(Gradient::even(&[RED, BLUE]));
            h.get_by_label("Colours stop 2").click_secondary();
            h.run();
            assert!(h.query_by_label_contains("Alpha").is_none(), "a right-click opens no picker");
            h.get_by_label("Remove").click();
            h.run();
            assert_eq!(h.state().stops, [Stop::new(0.0, RED)]);
            // The last colour has no handle to take off, only its swatch.
            assert!(h.query_by_label("Colours stop 1").is_none());
            assert!(h.query_by_label("Remove").is_none());
            // Dragging the last stop of two off the bar leaves one.
            h.state_mut().stops.push(Stop::new(1.0, BLUE));
            h.run();
            let bar = bar_rect(&h);
            let from = h.get_by_label("Colours stop 1").rect().center();
            drag(&mut h, from, egui::pos2(from.x, bar.bottom() + 60.0));
            assert_eq!(h.state().stops, [Stop::new(1.0, BLUE)]);
        }

        #[test]
        fn one_stop_shows_a_swatch_and_plus() {
            let mut h = harness(Gradient::solid(RED));
            assert!(h.query_by_label("Colours bar").is_none(), "no bar for one colour");
            h.get_by_label("Colours colour").click();
            h.run();
            assert!(h.query_by_label_contains("Alpha").is_some(), "the swatch opens the picker");
            h.key_press(egui::Key::Escape);
            h.run();
            h.get_by_label("Add a colour").click();
            h.run();
            assert_eq!(h.state().stops, [Stop::new(0.0, RED), Stop::new(1.0, Color::WHITE)]);
            assert_eq!(h.state().mix, bpe_core::Mix::Even, "a new gradient mixes Even");
            assert!(h.query_by_label("Colours bar").is_some());
        }

        #[test]
        fn colour_handle_opens_on_left_click() {
            let mut h = harness(Gradient::even(&[RED, BLUE]));
            assert!(h.query_by_label_contains("Alpha").is_none());
            h.get_by_label("Colours stop 2").click();
            h.run();
            assert!(h.query_by_label_contains("Alpha").is_some(), "a click opens the picker");
            assert!(h.query_by_label_contains("Additive").is_none(), "Blend Add replaced the Additive checkbox");
            assert!(h.query_by_label("Remove").is_none());
            assert_eq!(h.state().stops.len(), 2, "a click on a handle adds nothing");
        }

        #[test]
        fn there_is_no_transition_menu() {
            let h = harness(Gradient::even(&[RED, BLUE]));
            assert!(h.query_by_label("Colours transition").is_none(), "gap blends are in the tabs now");
        }
    }

    /// The Balls settings' grid, its values in `State::values`, with Size keyed when `keyed`.
    #[derive(Default)]
    struct State {
        values: std::collections::BTreeMap<&'static str, Value>,
        new_keys: Vec<&'static str>,
        changes: usize,
    }

    fn balls_grid(keyed: bool) -> egui_kittest::Harness<'static, State> {
        let defs = bpe_kit::BallsSettings::defs();
        let values = defs.iter().map(|d| (d.key, d.default.clone())).collect();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(420.0, 1600.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                move |ui, state: &mut State| {
                    let marks = |key: &str| {
                        let def = def_in(defs, key)?;
                        def.flags.keyframe.then_some(KeyMark {
                            can_key: true,
                            keyed: keyed && key == "radius",
                            pulsed: false,
                        })
                    };
                    let choices = TargetChoices::default();
                    let cx = GridCx {
                        choices: &choices,
                        marks: &marks,
                        applies: &|_| true,
                        folded: None,
                        label_width: label_width(420.0),
                        track: style::TEXT,
                        follow: None,
                        vary: None,
                    };
                    let got = settings_grid(ui, "g", defs, |k| state.values[k].clone(), &cx);
                    for (key, value) in got.changes {
                        state.values.insert(key, value);
                        state.changes += 1;
                    }
                    state.new_keys.extend(got.new_keys);
                },
                State { values, ..State::default() },
            );
        h.run();
        h
    }

    fn right_click(h: &mut egui_kittest::Harness<'_, State>, at: egui::Pos2) {
        h.hover_at(at);
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn right_clicking_a_slider_opens_the_menu_and_leaves_the_value() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::kittest::Queryable;
        let mut h = balls_grid(false);
        let before = h.state().values["radius"].clone();
        let slider = h.get_by_role_and_label(Role::Slider, "Size").rect();
        // Near the slider's right end, far from where the value sits.
        right_click(&mut h, egui::pos2(slider.right() - 4.0, slider.center().y));
        assert_eq!(h.state().values["radius"], before, "a right-click never changes a value");
        assert_eq!(h.state().changes, 0);
        assert!(h.query_by_label("Copy Value").is_some(), "the menu is open");
        assert!(h.query_by_label("Paste Value").is_some());
        assert!(h.query_by_label("Create Keyframe").is_some());
        assert!(h.query_by_label("Pulse with hits").is_none(), "no pulses in the menu");
    }

    #[test]
    fn copy_then_paste_moves_a_value_between_settings() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::kittest::{NodeT, Queryable};
        let mut h = balls_grid(false);
        h.state_mut().values.insert("space", Value::Number(42.0));
        h.run();
        // Nothing copied yet: Paste is there but greyed out.
        let size = h.get_by_role_and_label(Role::Label, "Size").rect().center();
        right_click(&mut h, size);
        assert!(h.get_by_label("Paste Value").accesskit_node().is_disabled());
        h.key_press(egui::Key::Escape);
        h.run();
        let spacing = h.get_by_role_and_label(Role::Label, "Space between").rect().center();
        right_click(&mut h, spacing);
        h.get_by_label("Copy Value").click();
        h.run();
        assert_eq!(copied(&h.ctx).map(|c| c.words), Some("42 px".to_owned()));
        right_click(&mut h, size);
        h.get_by_label("Paste Value (42 px)").click();
        h.run();
        assert_eq!(h.state().values["radius"], Value::Number(42.0));
        // A number doesn't fit a choice.
        let arrangement = h.get_by_role_and_label(Role::Label, "Shape").rect().center();
        right_click(&mut h, arrangement);
        assert!(h.get_by_label("Paste Value (42 px)").accesskit_node().is_disabled());
    }

    #[test]
    fn create_keyframe_is_offered_only_where_keys_are_allowed() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::kittest::{NodeT, Queryable};
        let mut h = balls_grid(true);
        let speed = h.get_by_role_and_label(Role::Label, "Launch speed").rect().center();
        right_click(&mut h, speed);
        assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled(), "launch speed can't have keys");
        h.key_press(egui::Key::Escape);
        h.run();
        let size = h.get_by_role_and_label(Role::Label, "Size").rect().center();
        right_click(&mut h, size);
        h.get_by_label("Create Keyframe").click();
        h.run();
        assert_eq!(h.state().new_keys, ["radius"]);
        assert_eq!(h.state().changes, 0);
        assert!(h.query_by_label("Keyframe Size").is_none(), "no diamonds");
    }

    #[test]
    fn pasting_fits_the_value_to_the_setting() {
        let defs = bpe_kit::BallsSettings::defs();
        let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
        assert_eq!(pasted(def("radius"), &Value::Number(9999.0)), Some(Value::Number(200.0)), "kept in range");
        assert_eq!(pasted(def("columns"), &Value::Number(3.6)), Some(Value::Int(4)));
        assert_eq!(pasted(def("radius"), &Value::Bool(true)), None);
        assert_eq!(pasted(def("shape"), &Value::Choice("nowhere".into())), None);
    }

    #[test]
    fn copied_values_read_in_a_few_words() {
        let c = TargetChoices::default();
        assert_eq!(value_words(&Value::Number(12.5), "px", &c), "12.5 px");
        assert_eq!(value_words(&Value::Number(0.25), "", &c), "0.25");
        assert_eq!(value_words(&Value::Int(3), "", &c), "3");
        assert_eq!(value_words(&Value::Bool(false), "", &c), "Off");
        assert_eq!(value_words(&Value::Vec2([0.0, 980.0]), "px/s²", &c), "0, 980 px/s²");
        assert_eq!(value_words(&Value::Color(Color::rgb(0xff, 0x80, 0x00)), "", &c), "#FF8000");
        let g = bpe_core::Gradient::even(&[Color::WHITE, Color::rgb(0, 0, 0), Color::WHITE]);
        assert_eq!(value_words(&Value::Gradient(g), "", &c), "3 colours");
        assert_eq!(value_words(&Value::Choice("electric_piano".into()), "", &c), "Electric piano");
        assert_eq!(value_words(&Value::Target(Target::Me), "", &c), "Itself");
    }

    #[test]
    fn every_setting_that_applies_shows_with_groups_under_headings() {
        let defs = bpe_kit::SceneSettings::defs();
        let all = rows_for(defs, &|_| true);
        assert_eq!(all.iter().filter(|r| matches!(r, Row::Setting(_))).count(), defs.len());
        let headings: Vec<&str> =
            all.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect();
        // Each card's name heads its rows once (in the order the fields first name them).
        let mut sorted = headings.clone();
        sorted.sort_unstable();
        let mut cards = bpe_kit::ROOT_CARDS.to_vec();
        cards.sort_unstable();
        assert_eq!(sorted, cards);
        // A group whose settings all drop out loses its heading too.
        let no_world = rows_for(defs, &|d| d.group != "World");
        assert!(!no_world.iter().any(|r| matches!(r, Row::Group("World"))));
    }

    #[test]
    fn a_looks_colours_are_one_gradient_row() {
        let defs = bpe_kit::modules::look::BasicLook::defs();
        let rows = rows_for(defs, &|_| true);
        let colours: Vec<&SettingDef> =
            rows.iter().filter_map(|r| if let Row::Setting(d) = r { Some(*d) } else { None }).collect();
        let colours: Vec<&str> = colours.iter().filter(|d| d.ty == ValueType::Gradient).map(|d| d.key).collect();
        // One row for the colours, one for the outline's, one for those inside each one.
        assert_eq!(colours, ["outline_colours", "colours", "in_colours"]);
    }

    fn headings<'a>(rows: &[Row<'a>]) -> Vec<&'static str> {
        rows.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect()
    }

    #[test]
    fn rows_follow_the_section_order() {
        let base = &bpe_kit::modules::look::BasicLook::defs()[0];
        let def = |key: &'static str, group: &'static str| SettingDef { key, group, ..base.clone() };
        let defs = [def("a", "Blending"), def("b", "Light"), def("c", "Shape"), def("d", "")];
        let rows = rows_for(&defs, &|_| true);
        assert!(matches!(rows[0], Row::Setting(d) if d.key == "d"), "ungrouped rows come first");
        assert_eq!(headings(&rows), ["Shape", "Light", "Blending"]);
        // Groups that are not sections (a thing kind's own) keep their order, after the sections.
        let defs = [def("a", "Video"), def("b", "Light"), def("c", "Grade")];
        assert_eq!(headings(&rows_for(&defs, &|_| true)), ["Light", "Video", "Grade"]);
    }

    #[test]
    fn the_look_cards_sections_come_in_order() {
        let defs = bpe_kit::modules::look::BasicLook::defs();
        let headings = headings(&rows_for(defs, &|_| true));
        assert_eq!(
            headings,
            [
                "Shape",
                "Style",
                "Across the group",
                "Inside each one",
                "Light",
                "Motion",
                "Outline colours variation",
                "Variation",
                "Blending"
            ]
        );
    }

    #[test]
    fn the_bounce_card_s_sections_come_in_order() {
        let bounce = bpe_kit::BasicBounce::defs();
        assert_eq!(headings(&rows_for(bounce, &|_| true)), ["Bounce", "Growing", "Other balls", "Against some things"]);
    }

    /// A grid of `defs` at their defaults, every section open, in a panel 420 wide.
    fn open_grid(defs: &'static [SettingDef]) -> egui_kittest::Harness<'static> {
        let value = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(420.0, 2400.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui(move |ui| {
                let choices = TargetChoices::default();
                let cx = GridCx {
                    choices: &choices,
                    marks: &|_| None,
                    applies: &|_| true,
                    folded: Some(&|_| false),
                    label_width: label_width(420.0),
                    track: style::TEXT,
                    follow: None,
                    vary: None,
                };
                settings_grid(ui, "g", defs, |k| value(k).unwrap(), &cx);
            });
        h.run();
        h
    }

    #[test]
    fn a_heading_has_a_row_of_its_own() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::kittest::Queryable;
        let h = open_grid(bpe_kit::modules::look::BasicLook::defs());
        let heading = h.get_by_role_and_label(Role::Label, "Light").rect();
        let label = h.get_by_role_and_label(Role::Label, "Glow").rect();
        let slider = h.get_by_role_and_label(Role::Slider, "Glow").rect();
        assert!(heading.bottom() <= label.top(), "the heading {heading:?} is above the label {label:?}");
        let (l, s) = (label.center().y, slider.center().y);
        assert!((l - s).abs() <= 1.5, "the slider is level with its label: {l} and {s}");
    }

    #[test]
    fn a_wrapped_label_makes_its_row_taller() {
        use eframe::egui::accesskit::Role;
        use egui_kittest::kittest::Queryable;
        let h = open_grid(bpe_kit::SceneSettings::defs());
        let wrapped = h.get_by_role_and_label(Role::Label, "Background stays on screen").rect();
        assert!(wrapped.height() > 24.0, "the label wraps: {wrapped:?}");
        // The Background group's last row; the Picture heading comes next.
        let next = h.get_by_role_and_label(Role::Label, "Picture").rect();
        assert!(wrapped.bottom() <= next.top(), "{wrapped:?} runs into the next row {next:?}");
        let check = h.get_by_role_and_label(Role::CheckBox, "Background stays on screen").rect();
        let (l, c) = (wrapped.center().y, check.center().y);
        assert!((l - c).abs() <= 1.5, "the checkbox is level with its label: {l} and {c}");
    }

    #[test]
    fn a_section_with_no_shown_rows_has_no_heading() {
        use egui_kittest::{Harness, kittest::Queryable};
        let defs = bpe_kit::modules::look::BasicLook::defs();
        let value = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
        // On an arena, every Shape row is hidden by its condition.
        let applies = |d: &SettingDef| d.applies(Some("arena"), &value);
        let rows = rows_for(defs, &applies);
        assert!(!headings(&rows).contains(&"Shape"), "{:?}", headings(&rows));
        let mut h = Harness::builder().with_size(egui::vec2(420.0, 1600.0)).allow_missing_glyphs().build_ui(|ui| {
            let choices = TargetChoices::default();
            let applies = |d: &SettingDef| d.applies(Some("arena"), &value);
            let cx = GridCx {
                choices: &choices,
                marks: &|_| None,
                applies: &applies,
                folded: Some(&|_| false),
                label_width: label_width(420.0),
                track: style::TEXT,
                follow: None,
                vary: None,
            };
            settings_grid(ui, "g", defs, |k| value(k).unwrap(), &cx);
        });
        h.run();
        assert!(h.query_by_label("Hide Shape").is_none(), "no fold arrow for an empty section");
        assert!(h.query_by_label("Shape").is_none(), "no heading for an empty section");
        assert!(h.query_by_label("Hide Blending").is_some(), "other sections keep theirs");
    }

    #[test]
    fn span_reads_px_when_following_place() {
        let defs = bpe_kit::modules::look::BasicLook::defs();
        let span = defs.iter().find(|d| d.key == "span").expect("the fill's span");
        let outline_span = defs.iter().find(|d| d.key == "outline_span").expect("the outline's span");
        let following = |follow: &'static str| move |k: &str| (k == "follow").then(|| Value::Choice(follow.into()));
        assert_eq!(unit_for(span, &following("place")), "px");
        assert_eq!(unit_for(span, &following("start")), "px");
        assert_eq!(unit_for(span, &following("speed")), span.unit);
        // The outline's span follows the outline's own follow, not the fill's.
        assert_eq!(unit_for(outline_span, &following("place")), outline_span.unit);
        let outline = |k: &str| (k == "outline_follow").then(|| Value::Choice("place".into()));
        assert_eq!(unit_for(outline_span, &outline), "px");
        let glow = defs.iter().find(|d| d.key == "glow_size").unwrap();
        assert_eq!(unit_for(glow, &following("place")), glow.unit);
    }

    #[test]
    fn numbers_move_in_even_steps_for_their_span() {
        let feel = |span: f64| number_feel(span, false);
        assert_eq!(feel(1.0).step, 0.001);
        assert_eq!(feel(1.0).decimals, 3);
        assert_eq!(feel(1.5).step, 0.001);
        assert_eq!(feel(40.0).step, 0.1);
        assert_eq!(feel(40.0).decimals, 1);
        assert_eq!(feel(720.0).step, 1.0);
        assert_eq!(feel(1980.0).step, 1.0);
        assert_eq!(feel(1980.0).decimals, 0);
        assert_eq!(number_feel(126.0, true).step, 1.0);
        assert!((feel(40.0).drag_speed - 0.1).abs() < 1e-9, "a drag crosses a small span in 400 pixels");
        // Wide spans move by ones, on the slider and the number box alike.
        for span in [2000.0, 4999.0, 40000.0] {
            assert_eq!(feel(span).step, 1.0, "{span}");
            assert_eq!(number_feel(span, true).step, 1.0, "{span}");
            assert_eq!(feel(span).drag_speed, 1.0, "{span}");
        }
        let speed = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "velocity").unwrap();
        assert_eq!(setting_feel(speed).step, 1.0, "a speed without a range moves by its usual span");
    }

    /// A span slider over 0..4 (hard range `range`) holding `value`, named "Amount".
    fn span_harness(value: f64, range: Option<(f64, f64)>) -> egui_kittest::Harness<'static, f64> {
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(400.0, 80.0))
            .with_context_setup(style::apply)
            .build_ui_state(
                move |ui, v: &mut f64| {
                    let feel = number_feel(4.0, false);
                    span_slider(ui, v, (0.0, 4.0), range, feel, false, String::new(), Named::Words("Amount"));
                },
                value,
            );
        h.run();
        h
    }

    fn drag_rail(h: &mut egui_kittest::Harness<'static, f64>, dx: f32) {
        use egui_kittest::kittest::{NodeT, Queryable};
        let rail = h
            .query_all_by_label("Amount")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .unwrap()
            .rect()
            .center();
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.hover_at(rail);
        h.run();
        h.event(press(rail, true));
        h.step();
        for k in 1..=10 {
            h.event(egui::Event::PointerMoved(rail + egui::vec2(dx * k as f32 / 10.0, 0.0)));
            h.step();
        }
        h.event(press(rail + egui::vec2(dx, 0.0), false));
        h.run();
    }

    #[test]
    fn a_value_past_its_span_shows_as_it_is_and_drags_keep_going() {
        assert_eq!(past_span(14.0, (0.0, 4.0)), 1);
        assert_eq!(past_span(-1.0, (0.0, 4.0)), -1);
        assert_eq!(past_span(4.0, (0.0, 4.0)), 0);
        let mut h = span_harness(14.0, None);
        assert_eq!(*h.state(), 14.0, "drawing never pulls it into the span");
        // From the middle (2) a long drag right goes past the end at the same rate.
        *h.state_mut() = 2.0;
        h.run();
        drag_rail(&mut h, 600.0);
        assert!(*h.state() > 8.0, "kept going past 4: {}", h.state());
        drag_rail(&mut h, -2000.0);
        assert!(*h.state() < 0.0, "and below 0 the other way: {}", h.state());
    }

    /// A slider in `unit` over `span` (hard range `range`) holding `value`, named "Amount", with
    /// the grid snapping at 60 px and 15°.
    fn snapping_harness(
        value: f64,
        span: (f64, f64),
        range: Option<(f64, f64)>,
        unit: &'static str,
    ) -> egui_kittest::Harness<'static, f64> {
        use crate::setting_snap::{SettingSnap, Snapper, put};
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(400.0, 80.0))
            .with_context_setup(style::apply)
            .build_ui_state(
                move |ui, v: &mut f64| {
                    put(ui.ctx(), Some(SettingSnap { length: 60.0, angle: 15.0 }));
                    let feel = number_feel(span.1 - span.0, false).with_snap(Snapper::for_unit(ui, unit, false));
                    span_slider(ui, v, span, range, feel, false, suffix(unit), Named::Words("Amount"));
                },
                value,
            );
        h.run();
        h
    }

    /// [`drag_rail`] with `modifiers` held all the way.
    fn drag_rail_holding(h: &mut egui_kittest::Harness<'static, f64>, dx: f32, modifiers: egui::Modifiers) {
        use egui_kittest::kittest::{NodeT, Queryable};
        let rail = h
            .query_all_by_label("Amount")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .unwrap()
            .rect()
            .center();
        let press =
            |pos, pressed| egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers };
        h.event(egui::Event::ModifiersChanged(modifiers));
        h.hover_at(rail);
        h.run();
        h.event(press(rail, true));
        h.step();
        for k in 1..=10 {
            h.event(egui::Event::PointerMoved(rail + egui::vec2(dx * k as f32 / 10.0, 0.0)));
            h.step();
        }
        h.event(press(rail + egui::vec2(dx, 0.0), false));
        h.step();
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run();
    }

    fn on_the_grid(v: f64) -> bool {
        (v / 60.0).fract().abs() < 1e-9 || (v.abs() < 60.0 && (v / 7.5).fract().abs() < 1e-9)
    }

    #[test]
    fn a_length_dragged_while_the_grid_snaps_lands_on_it() {
        for dx in [37.0, 81.0, 123.0, -51.0] {
            let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
            drag_rail_holding(&mut h, dx, egui::Modifiers::NONE);
            let v = *h.state();
            assert!(v != 200.0 || dx.abs() < 10.0, "it moved: {dx} {v}");
            assert!(on_the_grid(v), "{dx}: {v}");
        }
    }

    #[test]
    fn dragging_the_number_box_lands_on_the_grid_too() {
        let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
        drag_named(&mut h, "Amount", egui::accesskit::Role::SpinButton, 90.0);
        let v = *h.state();
        assert!(v != 200.0 && on_the_grid(v), "{v}");
    }

    #[test]
    fn alt_held_drags_a_length_freely() {
        let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
        drag_rail_holding(&mut h, 37.0, egui::Modifiers::ALT);
        let v = *h.state();
        assert!(!on_the_grid(v), "free with Alt: {v}");
    }

    #[test]
    fn angles_land_on_the_angle_step_and_speeds_never_snap() {
        let mut h = snapping_harness(0.0, (-180.0, 180.0), None, "°");
        drag_rail_holding(&mut h, 41.0, egui::Modifiers::NONE);
        let v = *h.state();
        assert!(v != 0.0 && (v / 15.0).fract().abs() < 1e-9, "on 15° steps: {v}");
        let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px/s");
        drag_rail_holding(&mut h, 37.0, egui::Modifiers::NONE);
        assert!(!on_the_grid(*h.state()), "a speed isn't a distance: {}", h.state());
    }

    #[test]
    fn a_snapped_length_still_keeps_its_hard_range() {
        let mut h = snapping_harness(2.0, (0.5, 400.0), Some((0.5, 400.0)), "px");
        drag_rail_holding(&mut h, -300.0, egui::Modifiers::NONE);
        assert_eq!(*h.state(), 0.5, "0 is a step, the range lifts it to 0.5");
    }

    #[test]
    fn a_typed_length_stays_exact() {
        use egui_kittest::kittest::{NodeT, Queryable};
        let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
        let field = h
            .query_all_by_label("Amount")
            .find(|n| n.accesskit_node().role() != egui::accesskit::Role::Slider)
            .unwrap()
            .rect()
            .center();
        // A double-click opens the number box for typing.
        h.hover_at(field);
        h.run();
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton {
                    pos: field,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            h.step();
        }
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("123".into()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(*h.state(), 123.0);
    }

    /// A setting row's widget for `def` holding `value`, the grid snapping at 60 px and 15°.
    fn widget_harness(def: SettingDef, value: Value) -> egui_kittest::Harness<'static, Value> {
        use crate::setting_snap::{SettingSnap, put};
        let choices = Box::leak(Box::new(TargetChoices::default()));
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(400.0, 80.0))
            .with_context_setup(style::apply)
            .build_ui_state(
                move |ui, v: &mut Value| {
                    put(ui.ctx(), Some(SettingSnap { length: 60.0, angle: 15.0 }));
                    let name = ui.label(def.label).id;
                    let unit = def.unit;
                    widget(ui, &def, unit, v, name, egui::Id::unique("bar"), choices, false);
                },
                value,
            );
        h.run();
        h
    }

    /// Drags the first control named `name` of role `role` by `dx`.
    fn drag_named<S>(h: &mut egui_kittest::Harness<'static, S>, name: &str, role: egui::accesskit::Role, dx: f32) {
        use egui_kittest::kittest::{NodeT, Queryable};
        let at = h.query_all_by_label(name).find(|n| n.accesskit_node().role() == role).unwrap().rect().center();
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.hover_at(at);
        h.run();
        h.event(press(at, true));
        h.step();
        for k in 1..=10 {
            h.event(egui::Event::PointerMoved(at + egui::vec2(dx * k as f32 / 10.0, 0.0)));
            h.step();
        }
        h.event(press(at + egui::vec2(dx, 0.0), false));
        h.run();
    }

    #[test]
    fn a_ranged_length_and_a_place_land_on_the_grid() {
        let base = bpe_kit::modules::look::BasicLook::defs()[0].clone();
        let thick = SettingDef {
            key: "thickness",
            label: "Thickness",
            ty: ValueType::Number,
            default: Value::Number(100.0),
            range: Some((0.5, 400.0)),
            span: None,
            unit: "px",
            when: "",
            ..base.clone()
        };
        let mut h = widget_harness(thick, Value::Number(100.0));
        drag_named(&mut h, "Thickness", egui::accesskit::Role::Slider, 37.0);
        let Value::Number(v) = *h.state() else { panic!() };
        assert!(v != 100.0 && on_the_grid(v), "an egui slider snapped after: {v}");

        let at = SettingDef {
            key: "at",
            label: "At",
            ty: ValueType::Vec2,
            default: Value::Vec2([0.0, 0.0]),
            range: None,
            span: None,
            unit: "px",
            when: "",
            ..base
        };
        let mut h = widget_harness(at, Value::Vec2([10.0, 0.0]));
        drag_named(&mut h, "At", egui::accesskit::Role::SpinButton, 90.0);
        let Value::Vec2([x, y]) = *h.state() else { panic!() };
        assert!(x != 10.0 && (x / 60.0).fract() == 0.0, "x on a grid point: {x}");
        assert_eq!(y, 0.0, "y wasn't touched");
    }

    #[test]
    fn a_hard_range_still_holds() {
        let mut h = span_harness(2.0, Some((0.0, 5.0)));
        drag_rail(&mut h, 2000.0);
        assert_eq!(*h.state(), 5.0);
        drag_rail(&mut h, -4000.0);
        assert_eq!(*h.state(), 0.0);
    }
}
