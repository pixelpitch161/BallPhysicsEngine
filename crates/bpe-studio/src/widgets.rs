//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.

use bpe_core::{
    Choice, Color, InstrumentId, InstrumentRef, InstrumentSource, ProgressionId, ProgressionRef, PulseSource, Registry,
    Scene, SettingDef, SettingValue, Target, ThingId, Value, ValueType, section_rank,
};
use eframe::egui::{
    self, Color32, DragValue, Pos2, Response, RichText, Sense, Slider, Stroke, Ui, Vec2, color_picker::Alpha,
};

use crate::gradient_bar::gradient_bar_named;
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
    /// Chord progressions as (id, name, how many settings use it), in library order.
    pub progressions: Vec<(ProgressionId, String, usize)>,
    /// Instruments as (id, name, how many settings use it), in library order.
    pub instruments: Vec<(InstrumentId, String, usize)>,
}

/// A Chords row's pick of "New progression": the inspector adds one and points the setting at it.
pub const NEW_PROGRESSION: i64 = -2;
/// A Chords row's pick of "Make unique": the inspector copies the progression the row shows.
pub const MAKE_UNIQUE: i64 = -3;
/// An Instrument row's pick of "New patch": the inspector adds a blank patch.
pub const NEW_PATCH: i64 = -4;
/// An Instrument row's pick of "Make unique": the inspector copies the instrument it shows.
pub const MAKE_UNIQUE_INSTRUMENT: i64 = -5;
/// An Instrument row's pick of built-in number `i` is `BUILT_IN_BASE - i`: the inspector finds
/// or adds a library instrument playing it.
pub const BUILT_IN_BASE: i64 = -100;
/// An Instrument row's pick of starter patch number `i` is `STARTER_BASE - i`.
pub const STARTER_BASE: i64 = -200;

impl TargetChoices {
    pub fn from_scene(scene: &Scene, reg: &Registry) -> TargetChoices {
        let things = scene.order.iter().filter_map(|id| scene.things.get(id).map(|t| (*id, t.name.clone()))).collect();
        let tags: std::collections::BTreeSet<String> =
            scene.things.values().flat_map(|t| t.tags.iter().cloned()).collect();
        let kinds =
            reg.things().filter(|k| k.id() != "scene").map(|k| (k.id().to_owned(), k.name().to_owned())).collect();
        let progressions = scene
            .library
            .progressions
            .iter()
            .map(|(id, p)| (*id, p.name.clone(), scene.progression_users(reg, *id).len()))
            .collect();
        let instruments = scene
            .library
            .instruments
            .iter()
            .map(|(id, i)| (*id, i.name.clone(), scene.instrument_users(reg, *id).len()))
            .collect();
        TargetChoices { things, tags: tags.into_iter().collect(), kinds, progressions, instruments }
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
    /// Whether the setting does anything here; rows that don't are left out.
    pub applies: &'a dyn Fn(&SettingDef) -> bool,
    /// Which group headings are folded shut. With none, headings have no fold arrow.
    pub folded: Option<&'a dyn Fn(&str) -> bool>,
    /// How wide the label column is, from [`label_width`]. Labels wrap at it.
    pub label_width: f32,
}

/// How wide the label column of every settings grid in a panel `panel` wide is. Every grid in
/// the panel uses the same width, so the sliders line up from card to card. In a narrow panel
/// the labels give up their share faster, down to 96, so the widgets still fit beside them.
pub fn label_width(panel: f32) -> f32 {
    const NARROW: f32 = 380.0;
    if panel >= NARROW {
        0.38 * panel
    } else {
        // From 96 at a 300-wide panel (the inspector at its narrowest) up to the share at 380.
        let at_narrow = 0.38 * NARROW;
        (96.0 + (panel - 300.0) * (at_narrow - 96.0) / (NARROW - 300.0)).clamp(96.0, at_narrow)
    }
}

/// The start of a row without a diamond in a [`settings_like_grid`]: an empty diamond cell,
/// then a label in the label column, `width` wide, wrapping when it is longer.
pub fn grid_label(ui: &mut Ui, width: f32, text: &str) -> Response {
    empty_cell(ui);
    label_cell(ui, width, |ui| ui.add(egui::Label::new(text).wrap()))
}

/// A grid laid out like a settings grid (a diamond column, the label column, the widgets), so
/// its labels and widgets line up with the settings' when its rows start with [`grid_label`].
pub fn settings_like_grid(id: impl egui::AsIdSalt) -> egui::Grid {
    egui::Grid::new(id).num_columns(3).spacing([8.0, 6.0]).min_col_width(DIAMOND_CELL)
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
    /// Groups folded shut (true) or opened from their heading's arrow.
    pub folds: Vec<(&'static str, bool)>,
}

/// `electric_piano` to "Electric piano".
pub fn pretty(name: &str) -> String {
    let text = name.replace('_', " ");
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
fn group_heading(ui: &mut Ui, group: &'static str, fold: bool, shut: bool, changed: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        if fold && fold_toggle(ui, !shut, group) {
            clicked = true;
        }
        let heading = RichText::new(group).strong().color(theme::MUTED);
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
            ui.painter().circle_filled(rect.center(), 3.0, theme::ACCENT);
            response.on_hover_text("Something in here is changed, keyed or pulsing.");
        }
    });
    clicked
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

/// A settings grid of the settings that `applies`, with no marks and nothing for targets to
/// point at. `get` supplies each setting's value as it stands; the rows the user changed come
/// back as (key, new value) for the caller to write through `set_at`.
pub fn settings_grid(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    applies: &dyn Fn(&SettingDef) -> bool,
    label_width: f32,
    get: impl Fn(&str) -> Value,
) -> Vec<(&'static str, Value)> {
    let choices = TargetChoices::default();
    let cx = GridCx {
        choices: &choices,
        overridden: &|_| false,
        inherited: &|_| None,
        marks: &|_| None,
        locked: &|_| false,
        applies,
        folded: None,
        label_width,
    };
    settings_grid_marked(ui, id, defs, get, &cx).changes
}

/// A settings grid of the settings that apply, with a keyframe diamond before each setting that can have one
/// (click: a key at the playhead; right-click: pulses), and a dot before the settings this thing
/// changed for itself. Such a row's double-click or Alt+click goes back to the module's value,
/// and its right-click menu offers to reset or push the change.
pub fn settings_grid_marked(
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
    settings_like_grid(id).show(ui, |ui| {
        // Whether the group the rows are in is folded shut.
        let mut shut = false;
        for row in rows_for(defs, cx.applies) {
            match row {
                Row::Group(group) => {
                    empty_cell(ui);
                    shut = cx.folded.is_some_and(|folded| folded(group));
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
                Row::Setting(def) if shut && !def.group.is_empty() => {}
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
                    let unit = unit_for(def, &|k| Some(get(k)));
                    let row = setting(ui, def, unit, &mut value, inherited.as_ref(), overridden, locked, cx, right);
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

/// The arrow that folds a card or a group of settings away (pointing right) and back (pointing
/// down), named "Hide {title}" or "Show {title}". True when clicked.
pub fn fold_toggle(ui: &mut Ui, open: bool, title: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let name = if open { format!("Hide {title}") } else { format!("Show {title}") };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let response = response.on_hover_text(if open { "Hide these settings" } else { "Show these settings" });
    // The collapsing header's own triangle, pointing down when open.
    let mut icon = response.clone();
    icon.rect = rect;
    egui::collapsing_header::paint_default_icon(ui, if open { 1.0 } else { 0.0 }, &icon);
    response.clicked()
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

/// One grid row for `def`, in `unit`: the label, wrapped to the grid's label width (help on
/// hover; a double-click or Alt+click puts the value back to `inherited`, or the setting's
/// default), a dot when the setting is `overridden`, a padlock when it is `locked` against the
/// dice, and the widget for its type, ending by `right`. The label's menu offers the lock on any
/// setting the dice may pick.
#[allow(clippy::too_many_arguments)]
pub fn setting(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    inherited: Option<&Value>,
    overridden: bool,
    locked: bool,
    cx: &GridCx,
    right: f32,
) -> RowAction {
    let fallback = inherited.cloned().unwrap_or_else(|| def.default.clone());
    let mut action = RowAction::Nothing;
    let name = label_cell(ui, cx.label_width, |ui| {
        if overridden {
            let (dot, response) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Changed here"));
            ui.painter().circle_filled(dot.center(), 3.0, theme::OVERRIDE);
            response.on_hover_text("Changed here: this thing's own value, not the module's.");
        }
        // The padlock keeps its room after a label that wraps.
        let padlock = if locked { 12.0 + ui.spacing().item_spacing.x } else { 0.0 };
        // The cell's full height, so the label is centred in it rather than hanging from its middle.
        let room = Vec2::new((ui.available_width() - padlock).max(0.0), ui.available_height());
        let name = ui
            .allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                label(ui, def.label, def.help)
            })
            .inner;
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
    });
    let mut changed = false;
    let response = widget_cell(ui, right, |ui| {
        let r = widget(ui, def, unit, value, name.id, cx.choices);
        changed |= r.changed();
        r
    });
    // A gradient bar's clicks add and pick stops, so only its label resets it.
    let gradient = def.ty == ValueType::Gradient;
    if resettable(&name, value, &fallback) || (!gradient && resettable(&response, value, &fallback)) {
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
    let response = ui.add(egui::Label::new(text).wrap().selectable(false).sense(Sense::click()));
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
/// How a number moves under the mouse: the step it snaps to, how many decimals it shows, and
/// how far a drag on its number box goes per pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberFeel {
    pub step: f64,
    pub decimals: usize,
    pub drag_speed: f64,
}

/// The feel of a number spanning `span`: the step is the power of ten nearest a two-hundredth of
/// the span, so every slider moves in round, even steps (0 to 1 by 0.001, 0 to 40 by 0.1, 0 to
/// 2000 by 1), and a drag on the number box crosses the span in about 400 pixels (Shift is
/// ten times finer).
pub fn number_feel(span: f64, integer: bool) -> NumberFeel {
    let span = if span.is_finite() && span > 0.0 { span } else { 1.0 };
    let power = (span / 200.0).log10().floor();
    let mut step = 10f64.powf(power).max(1e-4);
    if integer {
        step = step.max(1.0).round();
    }
    let decimals = if integer { 0 } else { (-power).max(0.0) as usize };
    let drag_speed = (span / 400.0).max(step / 4.0);
    NumberFeel { step, decimals, drag_speed }
}

/// The feel of a setting's number: from its range, else where the dice roll it, else from its
/// unit (pixels move by whole pixels, other things by hundredths).
pub fn setting_feel(def: &SettingDef) -> NumberFeel {
    let integer = def.ty == ValueType::Int;
    match def.range.or(def.dice) {
        Some((min, max)) => number_feel(max - min, integer),
        None if def.unit.starts_with("px") => number_feel(400.0, integer),
        None => number_feel(2.0, integer),
    }
}

/// A slider for a number setting: as wide as the row allows, moving in the setting's even steps,
/// with its number box beside it.
fn number_slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    x: &mut N,
    (min, max): (f64, f64),
    feel: NumberFeel,
    unit: String,
) -> Response {
    // Leave room for the number box at its widest, so the row never pushes the panel wider.
    let widest = format!("{}{:.*}{unit}", if min < 0.0 { "-" } else { "" }, feel.decimals, min.abs().max(max.abs()));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let number = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, egui::Color32::WHITE).size().x);
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
    ui.add(slider)
}

fn widget(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    name: egui::Id,
    choices: &TargetChoices,
) -> Response {
    let unit = suffix(unit);
    let feel = setting_feel(def);
    match (&def.ty, value) {
        (ValueType::Number, Value::Number(x)) => match def.range {
            Some(range) => number_slider(ui, x, range, feel, unit).labelled_by(name),
            None => ui
                .add(DragValue::new(x).speed(feel.drag_speed).fixed_decimals(feel.decimals).suffix(unit))
                .labelled_by(name),
        },
        (ValueType::Int, Value::Int(x)) => match def.range {
            Some(range) => number_slider(ui, x, range, feel, unit).labelled_by(name),
            None => ui.add(DragValue::new(x).speed(feel.drag_speed).suffix(unit)).labelled_by(name),
        },
        (ValueType::Bool, Value::Bool(b)) => ui.checkbox(b, "").labelled_by(name),
        // In a narrow panel y goes under x.
        (ValueType::Vec2, Value::Vec2(v)) => {
            ui.horizontal_wrapped(|ui| {
                let (speed, decimals) = (feel.drag_speed, feel.decimals);
                let x =
                    DragValue::new(&mut v[0]).speed(speed).fixed_decimals(decimals).prefix("x ").suffix(unit.clone());
                let x = ui.add(x).labelled_by(name);
                let y = DragValue::new(&mut v[1]).speed(speed).fixed_decimals(decimals).prefix("y ").suffix(unit);
                let y = ui.add(y).labelled_by(name);
                x | y
            })
            .inner
        }
        (ValueType::Color, Value::Color(c)) => color_button(ui, c).labelled_by(name),
        (ValueType::Gradient, Value::Gradient(g)) => {
            let id = ui.make_persistent_id(("gradient", def.key));
            // The bar and its handles carry their own names, after the setting's.
            gradient_bar_named(ui, id, g, def.label)
        }
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
        (ValueType::Progression, Value::Int(x)) => progression_picker(ui, name, x, choices).labelled_by(name),
        (ValueType::Instrument, Value::Int(x)) => instrument_picker(ui, name, x, choices).labelled_by(name),
        _ => ui.label(RichText::new("This setting holds a value of the wrong kind").color(theme::WARNING)),
    }
}

/// A menu naming the chord progression a setting uses: any in the library (none picked plays
/// the starter chords), a new one, or a copy of the one shown. Marked changed when something is picked; a new one or
/// a copy comes back as [`NEW_PROGRESSION`] or [`MAKE_UNIQUE`] for the inspector to make.
fn progression_picker(ui: &mut Ui, row: egui::Id, x: &mut i64, choices: &TargetChoices) -> Response {
    let name_of = |id: ProgressionId| {
        choices.progressions.iter().find(|(p, ..)| *p == id).map_or("missing".to_owned(), |(_, n, _)| n.clone())
    };
    let shown = match ProgressionRef::from_value(&Value::Int(*x)).and_then(|r| r.0) {
        Some(id) if choices.progressions.iter().any(|(p, ..)| *p == id) => name_of(id),
        _ => "Starter (built in)".to_owned(),
    };
    let before = *x;
    let mut response = egui::ComboBox::from_id_salt(("progression", row)).selected_text(shown).show_ui(ui, |ui| {
        for (id, name, users) in &choices.progressions {
            let label = if *users > 1 { format!("{name} · used by {users}") } else { name.clone() };
            ui.selectable_value(x, id.0 as i64, label);
        }
        ui.separator();
        if ui.button("New progression").clicked() {
            *x = NEW_PROGRESSION;
        }
        if ui.button("Make unique").on_hover_text("A copy of these chords that only this uses").clicked() {
            *x = MAKE_UNIQUE;
        }
    });
    if *x != before {
        response.response.mark_changed();
    }
    response.response
}

/// A menu naming the instrument a sound plays: the built-in sounds (none picked plays the
/// built-in Pluck), the library's instruments, a new patch, a starter patch, or a copy of the one shown. Marked
/// changed when something is picked; the picks the inspector has to make come back as
/// [`NEW_PATCH`], [`MAKE_UNIQUE_INSTRUMENT`], or offsets from [`BUILT_IN_BASE`] and
/// [`STARTER_BASE`].
fn instrument_picker(ui: &mut Ui, row: egui::Id, x: &mut i64, choices: &TargetChoices) -> Response {
    let name_of = |id: InstrumentId| {
        choices.instruments.iter().find(|(i, ..)| *i == id).map_or("missing".to_owned(), |(_, n, _)| n.clone())
    };
    let shown = match InstrumentRef::from_value(&Value::Int(*x)).and_then(|r| r.0) {
        Some(id) if choices.instruments.iter().any(|(i, ..)| *i == id) => name_of(id),
        _ => "Pluck (built in)".to_owned(),
    };
    let before = *x;
    let mut response =
        egui::ComboBox::from_id_salt(("instrument", row)).selected_text(shown).height(480.0).show_ui(ui, |ui| {
            ui.label(RichText::new("Built-in").small().color(theme::MUTED));
            for (i, which) in bpe_mix::Instrument::ALL.iter().enumerate() {
                if ui.selectable_label(false, which.label()).clicked() {
                    *x = BUILT_IN_BASE - i as i64;
                }
            }
            if !choices.instruments.is_empty() {
                ui.separator();
                ui.label(RichText::new("In this scene").small().color(theme::MUTED));
            }
            for (id, name, users) in &choices.instruments {
                let label = if *users > 1 { format!("{name} · used by {users}") } else { name.clone() };
                ui.selectable_value(x, id.0 as i64, label);
            }
            ui.separator();
            if ui.button("New patch").on_hover_text("A blank synth patch to build in the node panel").clicked() {
                *x = NEW_PATCH;
            }
            ui.label(RichText::new("Starter patches").small().color(theme::MUTED));
            for (i, (name, ..)) in bpe_synth::starters().iter().enumerate() {
                if ui.selectable_label(false, *name).clicked() {
                    *x = STARTER_BASE - i as i64;
                }
            }
            ui.separator();
            if ui.button("Make unique").on_hover_text("A copy of this instrument that only this sound uses").clicked() {
                *x = MAKE_UNIQUE_INSTRUMENT;
            }
        });
    if *x != before {
        response.response.mark_changed();
    }
    response.response
}

/// Whether library instrument `i` plays the built-in `which`.
pub fn plays_built_in(i: &bpe_core::Instrument, which: bpe_mix::Instrument) -> bool {
    matches!(&i.source, InstrumentSource::BuiltIn { which: w, .. } if w == Choice::name(which))
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
pub(crate) fn picker_color(c: Color) -> Color32 {
    if c.is_additive() {
        Color32::from_rgba_premultiplied(c.r, c.g, c.b, 0)
    } else {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
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
    fn the_label_column_is_a_share_of_the_panel_but_never_narrow() {
        assert_eq!(label_width(200.0), 96.0);
        assert_eq!(label_width(300.0), 96.0);
        assert_eq!(label_width(500.0), 190.0);
        // It grows without a jump from narrow to wide.
        assert!((label_width(379.9) - label_width(380.0)).abs() < 0.1);
        assert!(label_width(340.0) > 96.0 && label_width(340.0) < label_width(380.0));
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
        assert_eq!(target_words(&Target::Kind("arena".into()), &c), "Kind: Arena");
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
                .with_context_setup(crate::theme::apply)
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
            assert_eq!(g.transition, two.transition);
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
            assert_eq!(h.state().stops, [Stop { at: 0.0, color: RED }]);
            // The last colour has no handle to take off, only its swatch.
            assert!(h.query_by_label("Colours stop 1").is_none());
            assert!(h.query_by_label("Remove").is_none());
            // Dragging the last stop of two off the bar leaves one.
            h.state_mut().stops.push(Stop { at: 1.0, color: BLUE });
            h.run();
            let bar = bar_rect(&h);
            let from = h.get_by_label("Colours stop 1").rect().center();
            drag(&mut h, from, egui::pos2(from.x, bar.bottom() + 60.0));
            assert_eq!(h.state().stops, [Stop { at: 1.0, color: BLUE }]);
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
            assert_eq!(h.state().stops, [Stop { at: 0.0, color: RED }, Stop { at: 1.0, color: Color::WHITE }]);
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
        fn the_transition_menu_picks_how_stops_blend() {
            let mut h = harness(Gradient::even(&[RED, BLUE]));
            h.get_by_label("Colours transition").click();
            h.run();
            h.get_by_label("Steps").click();
            h.run();
            assert_eq!(h.state().transition, bpe_core::Transition::Steps);
        }
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
                    applies: &|_| true,
                    folded: None,
                    label_width: label_width(420.0),
                };
                let got = settings_grid_marked(
                    ui,
                    "g",
                    defs,
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
    fn every_setting_that_applies_shows_with_groups_under_headings() {
        let defs = bpe_kit::SceneSettings::defs();
        let all = rows_for(defs, &|_| true);
        assert_eq!(all.iter().filter(|r| matches!(r, Row::Setting(_))).count(), defs.len());
        let headings: Vec<&str> =
            all.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect();
        assert_eq!(headings, ["Video", "Grade", "Master", "Sound", "Song", "Beat", "Camera sound"]);
        // A group whose settings all drop out loses its heading too.
        let no_song = rows_for(defs, &|d| d.group != "Song");
        assert!(!no_song.iter().any(|r| matches!(r, Row::Group("Song"))));
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
    fn the_trail_and_bounce_cards_sections_come_in_order() {
        let trail = bpe_kit::modules::trail::StreakTrail::defs();
        assert_eq!(headings(&rows_for(trail, &|_| true)), ["Shape", "Colours", "Light", "Variation", "Blending"]);
        let bounce = bpe_kit::BasicBounce::defs();
        assert_eq!(headings(&rows_for(bounce, &|_| true)), ["Basics", "Growing", "Other balls", "Against some things"]);
    }

    /// A grid of `defs` at their defaults, every section open, in a panel 420 wide.
    fn open_grid(defs: &'static [SettingDef]) -> egui_kittest::Harness<'static> {
        let value = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(420.0, 2400.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui(move |ui| {
                let choices = TargetChoices::default();
                let cx = GridCx {
                    choices: &choices,
                    overridden: &|_| false,
                    inherited: &|_| None,
                    marks: &|_| None,
                    locked: &|_| false,
                    applies: &|_| true,
                    folded: Some(&|_| false),
                    label_width: label_width(420.0),
                };
                settings_grid_marked(ui, "g", defs, |k| value(k).unwrap(), &cx);
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
        let next = h.get_by_role_and_label(Role::Label, "Frame rate").rect();
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
        // On a liquid, every Shape row is hidden by its condition.
        let applies = |d: &SettingDef| d.applies(Some("liquid"), &value);
        let rows = rows_for(defs, &applies);
        assert!(!headings(&rows).contains(&"Shape"), "{:?}", headings(&rows));
        let mut h = Harness::builder().with_size(egui::vec2(420.0, 1600.0)).allow_missing_glyphs().build_ui(|ui| {
            let choices = TargetChoices::default();
            let applies = |d: &SettingDef| d.applies(Some("liquid"), &value);
            let cx = GridCx {
                choices: &choices,
                overridden: &|_| false,
                inherited: &|_| None,
                marks: &|_| None,
                locked: &|_| false,
                applies: &applies,
                folded: Some(&|_| false),
                label_width: label_width(420.0),
            };
            settings_grid_marked(ui, "g", defs, |k| value(k).unwrap(), &cx);
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
        assert!((feel(2000.0).drag_speed - 5.0).abs() < 1e-9, "a drag crosses the span in 400 pixels");
        let speed = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "velocity").unwrap();
        assert_eq!(setting_feel(speed).step, 1.0, "a speed without a range moves by its dice range");
    }
}
