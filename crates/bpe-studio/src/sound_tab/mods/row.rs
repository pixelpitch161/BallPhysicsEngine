//! The modulator row: a tile per modulator (ADSR 1, ADSR 2, Distance 1, Distance 2, LFO 1,
//! LFO 2 to start) and **+** for more (up to eight of each kind). A tile names its modulator,
//! says how many knobs it moves (or Off) and carries the four-arrow handle that drags onto a
//! knob. Clicking a tile picks it for the editor under the row; hovering it lights its rings;
//! right-clicking it offers Rename, Duplicate and Delete.

use super::super::{SoundTab, TabCx, TabEdit, name_it};
use super::{ModDrag, ModInfo, list};
use crate::style;
use bpe_core::{LayerId, SlotId};
use bpe_kit::sound::{ADSR, DISTANCE, HIT, LFO, MAX_OF_EACH, count_of};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// A tile's size.
const TILE: Vec2 = Vec2::new(96.0, 44.0);
/// The handle's side.
const HANDLE: f32 = 16.0;

/// How many knobs `layer` moves.
pub fn targets_of(cx: &TabCx, layer: LayerId) -> usize {
    cx.routes().iter().filter(|r| r.source == layer).count()
}

/// Draws the row and returns the modulator picked for the editor (the first when the one picked
/// before has gone). Sets the tab's lit modulator: the tile hovered, or the handle dragged.
pub fn show(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab) -> Option<LayerId> {
    let mods = list(cx);
    let mut picked = tab.picked.get(&cx.id).copied().filter(|p| mods.iter().any(|m| m.id == *p));
    if picked.is_none() {
        picked = mods.first().map(|m| m.id);
    }
    let mut hot = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for m in &mods {
            let r = tile(ui, cx, tab, m, picked == Some(m.id));
            if r.clicked {
                picked = Some(m.id);
            }
            if r.hovered {
                hot = Some(m.id);
            }
        }
        plus(ui, cx);
    });
    if let Some(held) = egui::DragAndDrop::payload::<ModDrag>(ui.ctx())
        && held.thing == cx.id
    {
        hot = Some(held.layer);
    }
    if tab.hot != hot {
        tab.hot = hot;
        ui.ctx().request_repaint();
    }
    if let Some(p) = picked {
        tab.picked.insert(cx.id, p);
    }
    picked
}

struct TileOut {
    clicked: bool,
    hovered: bool,
}

/// One tile, its handle and its menu.
fn tile(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab, m: &ModInfo, selected: bool) -> TileOut {
    let (rect, response) = ui.allocate_exact_size(TILE, Sense::click());
    let name = m.name.clone();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    let handle_rect =
        Rect::from_min_size(Pos2::new(rect.right() - HANDLE - 4.0, rect.top() + 4.0), Vec2::splat(HANDLE));
    let handle = ui.interact(handle_rect, response.id.with("handle"), Sense::drag());
    name_it(ui, &handle, &format!("Drag {}", m.name));
    handle.dnd_set_drag_payload(ModDrag { thing: cx.id, layer: m.id });
    let handle = handle.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag onto a knob to modulate it");
    let renaming = tab.renaming.as_ref().is_some_and(|(t, l, _)| *t == cx.id && *l == m.id);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = response.hovered() || handle.hovered();
        let fill = if selected { style::RAISED } else { style::PANEL };
        let line = if selected {
            style::TEXT
        } else if hovered {
            style::FAINT
        } else {
            style::LINE
        };
        painter.rect_filled(rect, style::RADIUS, fill);
        painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, line), egui::StrokeKind::Inside);
        if m.on {
            // A strip of the shape's colour on the left: this one sounds.
            let strip = Rect::from_min_size(rect.min + Vec2::new(1.0, 6.0), Vec2::new(2.0, rect.height() - 12.0));
            painter.rect_filled(strip, 0.0, cx.colour);
        }
        let inner = Rect::from_min_max(rect.min + Vec2::new(7.0, 4.0), rect.max - Vec2::new(6.0, 4.0));
        let ink = if m.on { style::TEXT } else { style::MUTED };
        if !renaming {
            painter.text(inner.left_top(), egui::Align2::LEFT_TOP, &m.name, style::semibold(12.0), ink);
        }
        let state = if !m.on {
            "Off".to_owned()
        } else {
            match targets_of(cx, m.id) {
                1 => "1 target".to_owned(),
                n => format!("{n} targets"),
            }
        };
        painter.text(
            inner.left_bottom(),
            egui::Align2::LEFT_BOTTOM,
            state,
            egui::FontId::proportional(11.0),
            style::MUTED,
        );
        four_arrows(
            painter,
            handle_rect,
            if handle.hovered() || handle.dragged() { style::TEXT } else { style::FAINT },
        );
    }
    if renaming {
        rename_box(ui, cx, tab, m, rect);
    }
    let card = cx.scene.card(cx.id, SlotId::Sound);
    let room = card.is_some_and(|c| count_of(c, m.kind) < MAX_OF_EACH);
    let last_adsr = m.kind == ADSR && card.is_some_and(|c| count_of(c, ADSR) <= 1);
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            tab.renaming = Some((cx.id, m.id, m.name.clone()));
            ui.close();
        }
        let dup =
            ui.add_enabled(room, egui::Button::new("Duplicate")).on_disabled_hover_text("Eight is the most of a kind");
        if dup.clicked() {
            cx.edits.push(TabEdit::DuplicateMod(m.id));
            cx.edits.push(TabEdit::OwnStep);
            ui.close();
        }
        let del = ui
            .add_enabled(!last_adsr, egui::Button::new("Delete"))
            .on_disabled_hover_text("A sound keeps one envelope");
        if del.clicked() {
            cx.edits.push(TabEdit::DeleteLayer(m.id));
            cx.edits.push(TabEdit::OwnStep);
            ui.close();
        }
    });
    TileOut { clicked: response.clicked(), hovered: response.hovered() || handle.hovered() }
}

/// The tile's name as a text box: Enter (or clicking away) keeps it, Escape doesn't.
fn rename_box(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab, m: &ModInfo, tile: Rect) {
    let Some((_, _, name)) = tab.renaming.as_mut() else { return };
    let line = Rect::from_min_size(tile.min + Vec2::new(5.0, 3.0), Vec2::new(tile.width() - HANDLE - 14.0, 20.0));
    let edit = ui.put(line, egui::TextEdit::singleline(name).font(style::semibold(12.0)));
    name_it(ui, &edit, "Rename modulator");
    if !edit.has_focus() && !edit.lost_focus() {
        edit.request_focus();
    }
    if edit.lost_focus() {
        let keep = !ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = std::mem::take(name);
        tab.renaming = None;
        if keep && !name.trim().is_empty() && name.trim() != m.name {
            cx.edits.push(TabEdit::RenameLayer(m.id, name.trim().to_owned()));
            cx.edits.push(TabEdit::OwnStep);
        }
    }
}

/// The handle: a cross with an arrowhead at each end.
fn four_arrows(painter: &egui::Painter, rect: Rect, ink: egui::Color32) {
    let c = rect.center();
    let r = rect.width() * 0.42;
    let stroke = Stroke::new(1.3, ink);
    painter.line_segment([c - Vec2::X * r, c + Vec2::X * r], stroke);
    painter.line_segment([c - Vec2::Y * r, c + Vec2::Y * r], stroke);
    for dir in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
        let tip = c + dir * r;
        let side = Vec2::new(-dir.y, dir.x) * 2.5;
        painter.line_segment([tip, tip - dir * 2.5 + side], stroke);
        painter.line_segment([tip, tip - dir * 2.5 - side], stroke);
    }
}

/// **+**: another ADSR, Distance, LFO or Hit (each greyed out at eight).
fn plus(ui: &mut Ui, cx: &mut TabCx) {
    let card = cx.scene.card(cx.id, SlotId::Sound);
    let r = ui.menu_button(egui::RichText::new("+").font(style::semibold(16.0)), |ui| {
        for (kind, label) in [(ADSR, "ADSR"), (DISTANCE, "Distance"), (LFO, "LFO"), (HIT, "Hit")] {
            let room = card.is_some_and(|c| count_of(c, kind) < MAX_OF_EACH);
            let b =
                ui.add_enabled(room, egui::Button::new(label)).on_disabled_hover_text("Eight is the most of a kind");
            if b.clicked() {
                cx.edits.push(TabEdit::AddMod(kind));
                cx.edits.push(TabEdit::OwnStep);
                ui.close();
            }
        }
    });
    name_it(ui, &r.response, "Add modulator");
}
