//! One layer of an oscillator's chain as a strip. Folded it is one row: the fold arrow, the on
//! switch, the name and a one-line summary, a live mini view and its Mix mini-knob (and a loop
//! mark when its source feeds back). Click the row to open it: the type, slope or quality
//! chips, Solo, a 64 px view and the knobs.

use super::strip_views::{self, View};
use super::{SoundTab, TabCx, TabEdit, db, name_it, number};
use crate::style;
use bpe_core::{Choice, Layer, LayerId, Params, SlotId, Value, layered_key, split_layered};
use bpe_kit::sound::chain::{hz_text, osc_name, vowel_name};
use bpe_kit::sound::{
    CHAIN_KINDS, DISTORTION, DistKind, DistortionSettings, FILTER, FilterKind, FilterSettings, MODULATION, ModKind,
    ModulationSettings, Quality, Slope, Source, layer_summary,
};
use eframe::egui::{self, Align, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, Vec2};

/// The folded row's height.
pub const ROW: f32 = 40.0;
/// The Mix mini-knob's side.
const MINI_KNOB: f32 = 28.0;
/// What the loop mark says.
pub const LOOP_TEXT: &str = "Feeds back one sample late";

/// A layer's own values by plain key, as they are at the playhead.
pub fn plain(cx: &TabCx, layer: LayerId) -> Params {
    let mut p = Params::default();
    let Some(card) = cx.scene.card(cx.id, SlotId::Sound) else { return p };
    let keys: Vec<String> = card
        .params
        .iter()
        .filter_map(|(k, _)| split_layered(k).filter(|(id, _)| *id == layer).map(|(_, rest)| rest.to_owned()))
        .collect();
    for k in keys {
        let v = cx.get(&layered_key(layer, &k));
        p.set(&k, v);
    }
    p
}

/// The layer's settings as its kind reads them.
enum Settings {
    Modulation(ModulationSettings),
    Filter(FilterSettings),
    Distortion(DistortionSettings),
}

impl Settings {
    fn of(kind: &str, p: &Params) -> Option<Settings> {
        use bpe_core::Settings as _;
        Some(match kind {
            MODULATION => Settings::Modulation(ModulationSettings::from_params(p)),
            FILTER => Settings::Filter(FilterSettings::from_params(p)),
            DISTORTION => Settings::Distortion(DistortionSettings::from_params(p)),
            _ => return None,
        })
    }

    fn view(&self) -> View<'_> {
        match self {
            Settings::Modulation(m) => View::Modulation(m),
            Settings::Filter(f) => View::Filter(f),
            Settings::Distortion(d) => View::Distortion(d),
        }
    }
}

/// Draws `layer`'s strip; `layers` are the card's (to name sources), `loops` the layers whose
/// source feeds back. Returns the folded row's response (for dragging and its menu).
pub fn show(
    ui: &mut Ui,
    cx: &mut TabCx,
    tab: &mut SoundTab,
    layer: &Layer,
    layers: &[Layer],
    loops: &[LayerId],
) -> egui::Response {
    let params = plain(cx, layer.id);
    let Some(settings) = Settings::of(&layer.kind, &params) else {
        return ui.label(RichText::new(format!("{}?", layer.name)).small().color(style::MUTED));
    };
    let open_key = (cx.id, layer.id);
    let open = tab.open_strips.contains(&open_key);
    let key = |k: &str| layered_key(layer.id, k);
    let name = layer.name.as_str();
    let ink = if layer.enabled { style::TEXT } else { style::FAINT };

    // The folded row: the whole row opens and closes the strip; the controls on it sit on top.
    let (rect, row) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click_and_drag());
    name_it(ui, &row, &format!("{name} strip"));
    let row = row.on_hover_text(if open { "Click to fold" } else { "Click to open" });
    if ui.is_rect_visible(rect) {
        let fill = if row.hovered() { style::HOVER } else { style::RAISED };
        ui.painter().rect_filled(rect, style::RADIUS, fill);
    }
    let mut inside = ui.new_child(
        UiBuilder::new().max_rect(rect.shrink2(egui::vec2(6.0, 4.0))).layout(Layout::left_to_right(Align::Center)),
    );
    {
        let ui = &mut inside;
        ui.spacing_mut().item_spacing.x = 6.0;
        let (arrow, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        style::paint_chevron(ui.painter(), arrow, open, style::MUTED);
        let mut on = layer.enabled;
        let switch = style::switch(ui, &mut on);
        name_it(ui, &switch, &format!("{name} on"));
        if switch.changed() {
            cx.edits.push(TabEdit::LayerOn(layer.id, on));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            mini_knob(ui, cx, &key("mix"), &format!("{name} mix"));
            let (view, _) = ui.allocate_exact_size(strip_views::MINI, Sense::hover());
            if ui.is_rect_visible(view) {
                strip_views::paint(ui.painter(), view, &settings.view(), ink);
            }
            if loops.contains(&layer.id) {
                loop_mark(ui);
            }
            // The name and summary fill what is left; the name is a text box while renaming.
            let text = ui.available_rect_before_wrap();
            let renaming = tab.renaming.as_ref().filter(|(t, l, _)| *t == cx.id && *l == layer.id).is_some();
            if renaming {
                rename_box(ui, cx, tab, layer, text);
            }
            if ui.is_rect_visible(text) {
                let painter = ui.painter().with_clip_rect(text);
                let summary = layer_summary(layer, &params, layers);
                if !renaming {
                    painter.text(
                        Pos2::new(text.left(), text.center().y - 1.0),
                        egui::Align2::LEFT_BOTTOM,
                        name,
                        style::semibold(12.0),
                        ink,
                    );
                }
                painter.text(
                    Pos2::new(text.left(), text.center().y + 1.0),
                    egui::Align2::LEFT_TOP,
                    summary,
                    egui::FontId::proportional(11.0),
                    style::MUTED,
                );
            }
        });
    }
    row.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            tab.renaming = Some((cx.id, layer.id, layer.name.clone()));
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            cx.edits.push(TabEdit::DuplicateLayer(layer.id));
            ui.close();
        }
        if ui.button("Copy layer").clicked() {
            cx.edits.push(TabEdit::CopyLayer(layer.id));
            ui.close();
        }
        let paste = ui
            .add_enabled(tab.can_paste_layer, egui::Button::new("Paste layer"))
            .on_disabled_hover_text("Copy a layer first");
        if paste.clicked() {
            cx.edits.push(TabEdit::PasteLayer { lane: layer.lane.unwrap_or(0), after: Some(layer.id) });
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            cx.edits.push(TabEdit::DeleteLayer(layer.id));
            ui.close();
        }
    });
    if row.clicked() {
        if open {
            tab.open_strips.remove(&open_key);
        } else {
            tab.open_strips.insert(open_key);
        }
    }
    if open {
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 4, top: 6, bottom: 8 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            body(ui, cx, layer, layers, &settings);
        });
    }
    row
}

/// The open strip: the type and its chips with Solo at the end, the view and the knobs.
fn body(ui: &mut Ui, cx: &mut TabCx, layer: &Layer, layers: &[Layer], settings: &Settings) {
    let key = |k: &str| layered_key(layer.id, k);
    let name = layer.name.as_str();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        match settings {
            Settings::Modulation(m) => {
                kind_menu(ui, cx, &key("kind"), &ModKind::ALL, ModKind::label, m.kind, &format!("{name} type"));
                source_menu(ui, cx, layer, layers, &m.source);
            }
            Settings::Filter(f) => {
                kind_menu(ui, cx, &key("kind"), &FilterKind::ALL, FilterKind::label, f.kind, &format!("{name} type"));
                if f.kind.has_slope() {
                    cx.chips(ui, &key("slope"), &[(Slope::Db12, "12 dB"), (Slope::Db24, "24 dB")]);
                }
            }
            Settings::Distortion(d) => {
                kind_menu(ui, cx, &key("kind"), &DistKind::ALL, DistKind::label, d.kind, &format!("{name} type"));
                if d.kind.oversampled() {
                    cx.chips(
                        ui,
                        &key("quality"),
                        &[(Quality::X1, "1x"), (Quality::X2, "2x"), (Quality::X4, "4x"), (Quality::X8, "8x")],
                    );
                }
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let solo = cx.flag(&key("solo"));
            let chip = ui.add(style::toggle_button("S", solo).min_size(egui::vec2(22.0, 22.0)));
            name_it(ui, &chip, &format!("{name} solo"));
            let chip = chip.on_hover_text("Solo: hear the chain cut at this layer");
            if chip.clicked() {
                cx.set(&key("solo"), Value::Bool(!solo));
            }
        });
    });
    view(ui, cx, layer, settings);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let pct = |v: f64| format!("{v:.0} %");
        match settings {
            Settings::Modulation(_) => {
                cx.knob(ui, &key("amount"), "Amount", &pct);
                cx.knob(ui, &key("output"), "Output", &db);
            }
            Settings::Filter(f) => {
                if f.kind == FilterKind::Formant {
                    cx.knob(ui, &key("vowel"), "Vowel", &|v| vowel_name(v).to_owned());
                    cx.knob(ui, &key("shift"), "Shift", &|v| format!("{v:+.1} st"));
                } else {
                    cx.log_knob(ui, &key("cutoff"), "Cutoff", &hz_text);
                }
                cx.knob(ui, &key("reso"), "Reso", &pct);
                cx.knob(ui, &key("drive"), "Drive", &pct);
                if f.kind != FilterKind::Formant {
                    cx.knob(ui, &key("key_track"), "Key track", &pct);
                }
                if f.kind.has_gain() {
                    cx.knob(ui, &key("gain"), "Gain", &db);
                }
            }
            Settings::Distortion(d) => {
                match d.kind {
                    DistKind::Bitcrush => cx.knob(ui, &key("bits"), "Bits", &|v| format!("{v:.0} bits")),
                    DistKind::Downsample => cx.log_knob(ui, &key("rate"), "Rate", &hz_text),
                    _ => cx.knob(ui, &key("drive"), "Drive", &|v| format!("{v:+.1} dB")),
                };
                cx.knob(ui, &key("bias"), "Bias", &pct);
                cx.knob(ui, &key("tone"), "Tone", &|v| format!("{v:+.0}"));
                cx.knob(ui, &key("output"), "Output", &db);
            }
        }
    });
}

/// The open strip's view; a filter's has a handle (sideways Cutoff, or Vowel for a formant; up
/// and down Reso).
fn view(ui: &mut Ui, cx: &mut TabCx, layer: &Layer, settings: &Settings) {
    let key = |k: &str| layered_key(layer.id, k);
    let filter = match settings {
        Settings::Filter(f) => Some(f),
        _ => None,
    };
    let sense = if filter.is_some() { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), strip_views::HEIGHT), sense);
    name_it(ui, &r, &format!("{} view", layer.name));
    let ink = if layer.enabled { style::TEXT } else { style::FAINT };
    if ui.is_rect_visible(rect) {
        strip_views::paint(ui.painter(), rect, &settings.view(), ink);
    }
    let Some(f) = filter else { return };
    let at = strip_views::handle_at(rect, f);
    let hot = r.hovered() || r.dragged();
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.circle_filled(at, if hot { 5.5 } else { 4.5 }, style::TEXT_STRONG);
        painter.circle_stroke(at, 5.5, Stroke::new(1.0, style::OUTLINE));
    }
    let r = r.on_hover_text("Drag sideways for Cutoff, up and down for Reso");
    // Only a drag that starts on the handle moves it, and by how far the pointer moved (a click
    // elsewhere on the view, or grabbing it a little off its middle, changes nothing).
    let grab_id = r.id.with("grab");
    if r.drag_started() {
        let origin = ui.input(|i| i.pointer.press_origin());
        match origin.filter(|o| (*o - at).length() <= HANDLE_GRAB) {
            Some(o) => ui.data_mut(|d| {
                d.insert_temp(grab_id, at - o);
            }),
            None => {
                ui.data_mut(|d| d.remove::<Vec2>(grab_id));
            }
        }
    }
    let grab: Option<Vec2> = ui.data(|d| d.get_temp(grab_id));
    if !r.dragged() {
        ui.data_mut(|d| d.remove::<Vec2>(grab_id));
    }
    if r.dragged()
        && let Some(offset) = grab
        && let Some(pointer) = r.interact_pointer_pos()
    {
        let p = pointer + offset;
        let (x, reso) = strip_views::handle_values(rect, p);
        if f.kind == FilterKind::Formant {
            let vowel = (x * 4.0 * 100.0).round() / 100.0;
            if (vowel - f.vowel as f64).abs() > 1e-6 {
                cx.set(&key("vowel"), Value::Number(vowel));
            }
        } else {
            let hz = strip_views::hz_at(x).round();
            if (hz - f.cutoff as f64).abs() >= 1.0 {
                cx.set(&key("cutoff"), Value::Number(hz));
            }
        }
        if (reso - f.reso as f64).abs() > 1e-3 {
            cx.set(&key("reso"), Value::Number(reso));
        }
    }
}

/// How near the filter view's handle a press must be to drag it.
const HANDLE_GRAB: f32 = 10.0;

/// A menu of a layer's types, named `name`.
fn kind_menu<T: Choice + Copy + PartialEq>(
    ui: &mut Ui,
    cx: &mut TabCx,
    key: &str,
    all: &[T],
    label: fn(T) -> &'static str,
    now: T,
    name: &str,
) {
    let mut picked = None;
    let menu = ui.menu_button(label(now), |ui| {
        ui.set_min_width(130.0);
        for &kind in all {
            if ui.selectable_label(kind == now, label(kind)).clicked() {
                picked = Some(kind);
                ui.close();
            }
        }
    });
    name_it(ui, &menu.response, name);
    if let Some(kind) = picked.filter(|k| *k != now) {
        cx.set(key, Value::Choice(kind.name().into()));
    }
}

/// A Modulation layer's source: either oscillator as it reaches its first layer, or the sound
/// after any other layer of either chain.
fn source_menu(ui: &mut Ui, cx: &mut TabCx, layer: &Layer, layers: &[Layer], source: &str) {
    let lane = layer.lane.unwrap_or(0);
    let now = Source::parse(source).in_lane(lane);
    let mut options: Vec<(Source, String)> = vec![(Source::Raw(0), "Osc A".into()), (Source::Raw(1), "Osc B".into())];
    for l in layers.iter().filter(|l| l.id != layer.id && CHAIN_KINDS.contains(&l.kind.as_str())) {
        options.push((Source::After(l.id), format!("{} · after {}", osc_name(l.lane.unwrap_or(0)), l.name)));
    }
    let shown = options.iter().find(|(s, _)| *s == now).map_or_else(|| osc_name(lane).to_owned(), |(_, t)| t.clone());
    let mut picked = None;
    let menu = ui.menu_button(format!("From {shown}"), |ui| {
        ui.set_min_width(170.0);
        for (s, text) in &options {
            if ui.selectable_label(*s == now, text).clicked() {
                picked = Some(*s);
                ui.close();
            }
        }
    });
    name_it(ui, &menu.response, &format!("{} source", layer.name));
    menu.response.on_hover_text("What moves the sound: an oscillator, or the sound after a layer");
    if let Some(s) = picked.filter(|s| *s != now) {
        cx.set(&layered_key(layer.id, "source"), Value::Text(s.text()));
    }
}

/// The name's text box while renaming in place: Enter (or clicking away) keeps the name, Escape
/// leaves it.
fn rename_box(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab, layer: &Layer, text: Rect) {
    let Some((_, _, name)) = tab.renaming.as_mut() else { return };
    let line = Rect::from_min_size(Pos2::new(text.left(), text.center().y - 19.0), Vec2::new(text.width(), 20.0));
    let edit = ui.put(line, egui::TextEdit::singleline(name).font(style::semibold(12.0)));
    name_it(ui, &edit, "Rename layer");
    if !edit.has_focus() && !edit.lost_focus() {
        // Starting: the whole name is picked, so typing replaces it.
        edit.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(name.chars().count()),
            );
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), edit.id);
        }
    }
    if edit.lost_focus() {
        let keep = !ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = std::mem::take(name);
        tab.renaming = None;
        if keep && name.trim() != layer.name {
            cx.edits.push(TabEdit::RenameLayer(layer.id, name));
        }
    }
}

/// The small circular arrow of a source that feeds back.
fn loop_mark(ui: &mut Ui) {
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    name_it(ui, &r, LOOP_TEXT);
    r.on_hover_text(LOOP_TEXT);
    if ui.is_rect_visible(rect) {
        let c = rect.center();
        let points = arc(c, 5.0, 0.08, 0.85);
        let end = *points.last().expect("points");
        ui.painter().add(Shape::line(points, Stroke::new(1.5, style::MUTED)));
        // The arrowhead at the end of the turn, pointing on round it.
        let a = std::f32::consts::TAU * 0.85;
        let along = egui::vec2(-a.sin(), a.cos());
        let out = egui::vec2(a.cos(), a.sin());
        ui.painter().add(Shape::convex_polygon(
            vec![end + along * 3.0, end - along * 1.0 + out * 2.5, end - along * 1.0 - out * 2.5],
            style::MUTED,
            Stroke::NONE,
        ));
    }
}

/// Points round a circle from turn `t0` to `t1` (0 at the right, going clockwise on screen).
fn arc(c: Pos2, r: f32, t0: f32, t1: f32) -> Vec<Pos2> {
    let n = 24;
    (0..=n)
        .map(|i| {
            let a = std::f32::consts::TAU * (t0 + (t1 - t0) * i as f32 / n as f32);
            c + egui::vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// The folded strip's Mix knob: drag up and down, double-click for 100 %, right-click for the
/// value menu.
fn mini_knob(ui: &mut Ui, cx: &mut TabCx, key: &str, name: &str) {
    let Some(def) = cx.def(key) else { return };
    let now = cx.get(key);
    let (lo, hi) = def.range.unwrap_or((0.0, 100.0));
    let default = number(&def.default).unwrap_or(hi);
    let v = number(&now).unwrap_or(default);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(MINI_KNOB), Sense::click_and_drag());
    let r = r.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let mut next = v;
    // The value is kept unrounded while dragging, so a slow drag still gets somewhere.
    let held_id = r.id.with("held");
    if r.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let held: Option<f64> = ui.data(|d| d.get_temp(held_id));
        let from = held.filter(|h| (h.round() - v).abs() < 0.5).unwrap_or(v);
        let moved = (from - r.drag_delta().y as f64 / 150.0 * (hi - lo) * fine).clamp(lo, hi);
        ui.data_mut(|d| d.insert_temp(held_id, moved));
        next = moved.round();
    } else {
        ui.data_mut(|d| d.remove::<f64>(held_id));
    }
    if r.double_clicked() {
        next = default;
    }
    if (next - v).abs() > 1e-9 {
        cx.set(key, Value::Number(next));
    }
    let label = name.to_owned();
    r.widget_info(|| egui::WidgetInfo::slider(true, v, &label));
    name_it(ui, &r, name);
    let r = r.on_hover_text(format!("Mix {v:.0} %"));
    if ui.is_rect_visible(rect) {
        paint_mini(ui, rect, ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32, r.hovered() || r.dragged());
    }
    // A modulator dropped on it routes there; the menu offers Modulate by too.
    super::mods::drag::on_knob(cx, &r, key);
    cx.knob_menu(&r, key, def, &now);
}

fn paint_mini(ui: &Ui, rect: Rect, t: f32, hot: bool) {
    let painter = ui.painter();
    let c = rect.center();
    // 270 degrees from the bottom left, round over the top, to the bottom right.
    let turn = |t: f32| 0.375 + 0.75 * t;
    painter.add(Shape::line(
        arc(c, 10.0, turn(0.0), turn(1.0)),
        Stroke::new(2.5, if hot { style::FAINT } else { style::LINE }),
    ));
    if t > 1e-4 {
        painter.add(Shape::line(arc(c, 10.0, turn(0.0), turn(t)), Stroke::new(2.5, style::MUTED)));
    }
    let a = std::f32::consts::TAU * turn(t);
    painter.line_segment([c, c + egui::vec2(a.cos(), a.sin()) * 8.0], Stroke::new(1.5, style::TEXT));
}
