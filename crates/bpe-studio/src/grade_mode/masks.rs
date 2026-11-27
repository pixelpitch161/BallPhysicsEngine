//! Grade mode's masks list (the left column): the Whole picture, then each mask in the order they
//! work, with its kind's icon, its name, a note when what it follows isn't there, and its eye. A
//! click shows the mask's parts and panels on the right; a drag moves it; a double-click renames
//! it; right-click for Rename, Duplicate, Invert, Move up or down and Delete. Under the list, Add
//! mask and the overlay switch (O); then the snapshots and grade files (`snapshots.rs`).

use super::GradeMode;
use super::cx::{self, GradeCx, GradeEdit, full_key};
use crate::app::Studio;
use crate::style;
use bpe_core::{Layer, LayerId, Params, Slot, Value};
use bpe_kit::modules::grade::{self, BRUSH, COLOUR_RANGE, LINEAR, LUMINANCE_RANGE, PART_KINDS, RADIAL, THINGS};
use eframe::egui::{self, RichText, Sense, Stroke, Ui};

/// The column: masks, Add mask, the overlay switch, then the snapshots, with its edits applied.
pub fn column(ui: &mut Ui, studio: &mut Studio) {
    ui.label(RichText::new("Masks").font(style::semibold(13.0)));
    ui.add_space(4.0);
    let start = studio.grade.start(&studio.kit.registry);
    let time = studio.sim_time();
    let mut edits = Vec::new();
    {
        let Studio { scene, kit, grade: mode, playback, .. } = &mut *studio;
        let card = cx::card_or_start(scene, &start);
        let gx = GradeCx { scene, kit, card, time };
        let shown = mode.mask_of(card);
        whole_row(ui, mode, &gx, shown == grade::whole_of(card), &mut edits);
        let masks = grade::masks_of(card);
        let mut rows = Vec::new();
        for (index, layer) in masks.iter().enumerate() {
            let note = note_of(card, layer.id, playback.world());
            let place = Place { index, count: masks.len() };
            let rect = mask_row(ui, mode, &gx, layer, place, shown == Some(layer.id), note, &mut edits);
            rows.push((layer.id, rect));
        }
        drop_line(ui, mode, &rows);
        mode.mask_rows = rows;
        ui.add_space(4.0);
        add_mask(ui, &mut edits);
        ui.add_space(6.0);
        overlay_switch(ui, mode);
    }
    super::apply(studio, edits);
    ui.add_space(14.0);
    super::snapshots::section(ui, studio);
}

/// The Whole picture's row: its eye is the whole grade's switch.
fn whole_row(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, selected: bool, edits: &mut Vec<GradeEdit>) {
    let on = gx.on();
    let panels = grade::whole_of(gx.card)
        .map(|w| grade::panels_of(gx.card, w).iter().filter(|p| gx.changed(p.id)).count())
        .unwrap_or(0);
    row_frame(selected).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let ink = if on { style::TEXT } else { style::FAINT };
            let name = ui.add(egui::Button::new(RichText::new("Whole picture").color(ink)).frame(false));
            if name.on_hover_text("The grade of the whole picture, before the masks").clicked() {
                mode.mask = None;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let help = if on {
                    "The grade is on. Click to see the picture without it (every setting is kept)."
                } else {
                    "The grade is off: the video has no grade. Click to turn it on."
                };
                if eye(ui, on, "Grade on", help) {
                    edits.push(GradeEdit::Set("on".into(), Value::Bool(!on)));
                }
                let count = if panels == 1 { "1 panel changed".to_owned() } else { format!("{panels} panels changed") };
                ui.label(RichText::new(count).small().color(style::MUTED));
            });
        });
    });
}

fn row_frame(selected: bool) -> egui::Frame {
    let frame = egui::Frame::new().corner_radius(style::RADIUS).inner_margin(egui::Margin::symmetric(8, 5));
    if selected { frame.fill(style::SELECTED) } else { frame }
}

/// An eye switch named `name`; true when clicked.
fn eye(ui: &mut Ui, on: bool, name: &str, help: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, name));
    let ink = if response.hovered() { style::TEXT } else { style::MUTED };
    crate::tree::paint_eye(ui.painter(), rect, on, ink);
    response.on_hover_text(help).clicked()
}

/// Where a mask is among the masks.
#[derive(Clone, Copy)]
struct Place {
    index: usize,
    count: usize,
}

/// One mask's row. Returns its rectangle.
#[allow(clippy::too_many_arguments)]
fn mask_row(
    ui: &mut Ui,
    mode: &mut GradeMode,
    gx: &GradeCx,
    layer: &Layer,
    place: Place,
    selected: bool,
    note: Option<String>,
    edits: &mut Vec<GradeEdit>,
) -> egui::Rect {
    let id = layer.id;
    let first = grade::parts_of(gx.card, id).first().map(|p| p.kind.to_string());
    let inner = row_frame(selected).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            kind_icon(ui, first.as_deref(), layer.enabled);
            if let Some((_, typed)) = mode.renaming.as_mut().filter(|(r, _)| *r == id) {
                let label = ui.label(RichText::new("Name").small().color(style::MUTED));
                let field = ui.add(egui::TextEdit::singleline(typed).desired_width(110.0)).labelled_by(label.id);
                let done = field.lost_focus();
                // Focus once when it opens (asking every frame would take it back from Enter).
                if !done && !field.has_focus() {
                    field.request_focus();
                }
                if done {
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        edits.push(GradeEdit::Rename(id, typed.clone()));
                    }
                    mode.renaming = None;
                }
            } else {
                let ink = if layer.enabled { style::TEXT } else { style::FAINT };
                let name = ui.add(
                    egui::Button::new(RichText::new(&layer.name).color(ink)).frame(false).sense(Sense::click_and_drag()),
                );
                let name = name.on_hover_text("Click to grade with this mask. Drag to change when it works; double-click to rename; right-click for more.");
                if name.clicked() {
                    mode.mask = Some(id);
                }
                if name.double_clicked() {
                    mode.renaming = Some((id, layer.name.clone()));
                }
                drag(ui, mode, id, place, &name, edits);
                name.context_menu(|ui| menu(ui, mode, gx, layer, place, edits));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let words = format!("{} shown", layer.name);
                let help = if layer.enabled {
                    "The mask is on. Click to hide it (its settings are kept)."
                } else {
                    "The mask is hidden: it does nothing. Click to show it."
                };
                if eye(ui, layer.enabled, &words, help) {
                    edits.push(GradeEdit::Enable(id, !layer.enabled));
                }
                if let Some(note) = &note {
                    ui.label(RichText::new(note).small().color(style::PROBLEM));
                }
            });
        });
    });
    ui.add_space(2.0);
    inner.response.rect
}

/// A drag of a mask's name moves it among the masks.
fn drag(ui: &Ui, mode: &mut GradeMode, id: LayerId, place: Place, name: &egui::Response, edits: &mut Vec<GradeEdit>) {
    if name.drag_started() {
        mode.mask_drag = Some(id);
    }
    if name.dragged() && mode.mask_drag == Some(id) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if name.drag_stopped() && mode.mask_drag == Some(id) {
        mode.mask_drag = None;
        if let Some(to) = ui.ctx().pointer_latest_pos().map(|p| super::panels::drop_index(&mode.mask_rows, id, p.y))
            && to != place.index
        {
            edits.push(GradeEdit::MoveMask(id, to));
        }
    }
}

/// While a mask row is dragged, a line where it would land.
fn drop_line(ui: &Ui, mode: &GradeMode, rows: &[(LayerId, egui::Rect)]) {
    let Some(id) = mode.mask_drag else { return };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else { return };
    let to = super::panels::drop_index(rows, id, pointer.y);
    let others: Vec<&egui::Rect> = rows.iter().filter(|(r, _)| *r != id).map(|(_, r)| r).collect();
    let y = match others.get(to) {
        Some(r) => r.top() - 2.0,
        None => others.last().map_or(pointer.y, |r| r.bottom() + 1.0),
    };
    ui.painter().hline(ui.min_rect().x_range(), y, Stroke::new(2.0, style::TEXT));
}

/// The right-click menu on a mask's name.
fn menu(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, layer: &Layer, place: Place, edits: &mut Vec<GradeEdit>) {
    let id = layer.id;
    if ui.button("Rename").clicked() {
        mode.renaming = Some((id, layer.name.clone()));
        ui.close();
    }
    if ui.button("Duplicate").on_hover_text("A copy of the mask, its parts and its panels, just after it").clicked() {
        edits.push(GradeEdit::DuplicateMask(id));
        ui.close();
    }
    let inverted = matches!(gx.get(Some(id), "invert"), Value::Bool(true));
    let word = if inverted { "Invert back" } else { "Invert" };
    if ui.button(word).on_hover_text("Cover where the mask doesn't, and not where it does").clicked() {
        edits.push(GradeEdit::Set(full_key(Some(id), "invert"), Value::Bool(!inverted)));
        ui.close();
    }
    if ui.add_enabled(place.index > 0, egui::Button::new("Move up")).clicked() {
        edits.push(GradeEdit::MoveMask(id, place.index - 1));
        ui.close();
    }
    if ui.add_enabled(place.index + 1 < place.count, egui::Button::new("Move down")).clicked() {
        edits.push(GradeEdit::MoveMask(id, place.index + 1));
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").on_hover_text("The mask, its parts and its panels go").clicked() {
        edits.push(GradeEdit::RemoveMask(id));
        if mode.mask == Some(id) {
            mode.mask = None;
        }
        ui.close();
    }
}

/// The kinds a mask can start with, and what Add mask calls them.
pub fn part_name(kind: &str) -> &'static str {
    match kind {
        LINEAR => "Linear Gradient",
        RADIAL => "Radial Gradient",
        BRUSH => "Brush",
        THINGS => "Things",
        COLOUR_RANGE => "Colour Range",
        LUMINANCE_RANGE => "Luminance Range",
        _ => "Part",
    }
}

fn add_mask(ui: &mut Ui, edits: &mut Vec<GradeEdit>) {
    ui.menu_button("Add mask", |ui| {
        for kind in PART_KINDS {
            if ui.button(part_name(kind)).clicked() {
                edits.push(GradeEdit::AddMask(kind));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("A mask over part of the picture, with its own panels, after the others");
}

/// The overlay switch: red over where the selected mask covers (O).
fn overlay_switch(ui: &mut Ui, mode: &mut GradeMode) {
    ui.horizontal(|ui| {
        let mut on = mode.overlay;
        let switch = style::switch(ui, &mut on);
        switch.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Show overlay"));
        if switch.on_hover_text("Red over where the selected mask covers (O)").changed() {
            mode.overlay = on;
        }
        ui.label(RichText::new("Show overlay (O)").small().color(style::MUTED));
    });
}

/// A small picture of a mask's first part's kind.
fn kind_icon(ui: &mut Ui, kind: Option<&str>, on: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
    let ink = if on { style::MUTED } else { style::FAINT };
    let p = ui.painter();
    let c = rect.center();
    match kind {
        Some(LINEAR) => {
            for (dy, w) in [(-4.0, 1.0), (0.0, 1.6), (4.0, 1.0)] {
                p.hline(rect.x_range().shrink(1.0), c.y + dy, Stroke::new(w, ink));
            }
        }
        Some(RADIAL) => {
            p.circle_stroke(c, 5.5, Stroke::new(1.2, ink));
            p.circle_filled(c, 1.5, ink);
        }
        Some(COLOUR_RANGE) => {
            p.circle_filled(c, 5.0, ink);
            p.circle_filled(c, 2.0, style::PANEL);
        }
        Some(BRUSH) => {
            // A brush dab: a soft round blob with a short stroke trailing off it.
            p.circle_filled(c + egui::vec2(1.5, -1.5), 3.5, ink);
            p.line_segment([c + egui::vec2(-1.0, 1.0), c + egui::vec2(-5.0, 5.0)], Stroke::new(1.6, ink));
        }
        Some(THINGS) => {
            // A ball over a square.
            let square = egui::Rect::from_min_size(rect.min + egui::vec2(1.5, 5.0), egui::vec2(7.5, 7.5));
            p.rect_stroke(square, 1.0, Stroke::new(1.2, ink), egui::StrokeKind::Inside);
            p.circle_filled(c + egui::vec2(2.0, -2.0), 3.8, ink);
        }
        Some(LUMINANCE_RANGE) => {
            p.circle_stroke(c, 5.5, Stroke::new(1.2, ink));
            let half =
                egui::Rect::from_min_max(egui::pos2(c.x, rect.top() + 1.5), egui::pos2(c.x + 5.5, rect.bottom() - 1.5));
            p.rect_filled(half, 0.0, ink);
        }
        _ => {
            p.rect_stroke(rect.shrink(2.0), 2.0, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
        }
    }
}

/// The plain-key settings of layer `id` on `card`, as saved (gradients don't key what they follow).
pub fn layer_params(card: &Slot, id: LayerId) -> Params {
    let prefix = format!("{id}/");
    Params(
        card.params
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|plain| (plain.to_owned(), v.clone())))
            .collect(),
    )
}

/// What a mask's row says: the first of its gradients' notes ("Thing not found").
fn note_of(card: &Slot, mask: LayerId, world: &bpe_runtime::World) -> Option<String> {
    grade::parts_of(card, mask)
        .iter()
        .filter(|p| p.enabled)
        .find_map(|p| grade::masks::follow_note(world, &p.kind, &layer_params(card, p.id)))
}
