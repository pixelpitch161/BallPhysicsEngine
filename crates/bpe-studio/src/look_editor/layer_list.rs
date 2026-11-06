//! One row of the Look editor's lists, a layer's box with its sub-layers, and dragging the drawn
//! layers (and the sub-layers in their box) into another order.

use bpe_core::{Color, Layer, LayerId, LayerSection, Settings, Slot};
use bpe_kit::modules::look::layers::{
    self as kinds, CutFrom, CutLayer, CutShape, ShapeCut, gpu::choice_label, particles, trail,
};
use bpe_kit::modules::look::shape as shape_look;
use eframe::egui::{self, Color32, Key, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::LookCx;
use crate::inspector::{Edit, Inspector};
use crate::style;

/// The height of a layer's row.
const ROW_H: f32 = 28.0;
/// The height of a sub-layer's row.
const SUB_H: f32 = 26.0;
/// How far a sub-layer's row is indented.
const SUB_INDENT: f32 = 30.0;

/// What a dragged row carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dragged(bpe_core::ThingId, LayerId);

/// What a dragged sub-layer row carries: its thing, its layer and itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DraggedSub(bpe_core::ThingId, LayerId, LayerId);

/// The layers not inside another (sub-layers are inside the layer they change), in card order. A
/// layer's place among them is the place [`Edit::MoveLayer`] counts.
pub fn top_level(card: &Slot) -> impl Iterator<Item = &Layer> {
    card.layers.iter().filter(|l| l.parent.is_none())
}

/// Where layer `id` is among the [`top_level`] layers, the Body 0.
pub fn place_of(card: &Slot, id: LayerId) -> Option<usize> {
    top_level(card).position(|l| l.id == id)
}

/// Layer `parent`'s sub-layers, in card order: the order they work in, top to bottom in its box.
pub fn subs_of(card: &Slot, parent: LayerId) -> impl Iterator<Item = &Layer> {
    card.layers.iter().filter(move |l| l.parent == Some(parent))
}

/// The name of a sub-layer kind without its card's prefix: "cut" for `sub.cut` and
/// `shape.sub.cut`; `None` for a kind that isn't a sub-layer's.
pub fn sub_name(kind: &str) -> Option<&str> {
    kind.strip_prefix("shape.").unwrap_or(kind).strip_prefix("sub.")
}

/// Draws a top-level layer: its row alone, or, when it has sub-layers, one box holding its row,
/// its sub-layers' rows (unless folded) and a "+ Sub-layer" row. Returns the rectangle it covers,
/// where a dragged layer lands above or below it.
pub fn piece(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: &Slot,
    layer: &Layer,
    picked: LayerId,
    edits: &mut Vec<Edit>,
) -> Rect {
    let subs: Vec<&Layer> = subs_of(card, layer.id).collect();
    if subs.is_empty() {
        return row(ui, inspector, cx, card, layer, picked, edits);
    }
    let folded = inspector.folded_pieces.contains(&(cx.id, layer.id));
    let frame = egui::Frame::new()
        .fill(style::GROUND)
        .stroke(Stroke::new(1.0, style::LINE))
        .corner_radius(egui::CornerRadius::same(2))
        .inner_margin(egui::Margin::symmetric(0, 1));
    frame
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            row(ui, inspector, cx, card, layer, picked, edits);
            if folded {
                return;
            }
            let mut rows = Vec::new();
            for sub in &subs {
                rows.push((sub.id, row(ui, inspector, cx, card, sub, picked, edits)));
            }
            ui.horizontal(|ui| {
                ui.set_min_height(22.0);
                ui.add_space(SUB_INDENT + 12.0);
                let add = RichText::new("+ Sub-layer").small().color(style::MUTED);
                ui.menu_button(add, |ui| sub_kind_menu(ui, cx, card, layer.id, edits))
                    .response
                    .on_hover_text("Add a sub-layer: it changes this layer only");
            });
            sub_drop_target(ui, cx, layer.id, &rows, edits);
        })
        .response
        .rect
}

/// The sub-layer kinds of the card that layer `parent` takes, in the registry's order, each adding
/// one as its last sub-layer; greyed when it has 12 already.
pub fn sub_kind_menu(ui: &mut Ui, cx: &LookCx, card: &Slot, parent: LayerId, edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    let Some(kind) = card.layer(parent).and_then(|l| reg.layer(&l.kind)) else { return };
    let full = subs_of(card, parent).count() >= bpe_core::MAX_SUBS;
    ui.set_min_width(170.0);
    let fits = reg.layers_for(&card.kind).filter(|k| k.section() == LayerSection::Sub && kind.takes_sub(k.id()));
    for sub in fits {
        // The hit reactions after the rest.
        if sub_name(sub.id()) == Some("flash") {
            ui.separator();
        }
        let item = ui.add_enabled(!full, egui::Button::new(sub.name()));
        let item = item.on_hover_text(sub.help()).on_disabled_hover_text("Up to 12 sub-layers");
        if item.clicked() {
            edits.push(Edit::AddSubLayer(cx.slot, parent, sub.id()));
            ui.close();
        }
    }
}

/// Where a sub-layer of `parent` dragged over its box's `rows` (top first) lands: a line marks
/// the gap, and letting go moves it there. Dropped outside its box, it stays.
fn sub_drop_target(ui: &mut Ui, cx: &LookCx, parent: LayerId, rows: &[(LayerId, Rect)], edits: &mut Vec<Edit>) {
    let ctx = ui.ctx().clone();
    let Some(dragged) = egui::DragAndDrop::payload::<DraggedSub>(&ctx) else { return };
    let DraggedSub(thing, of, sub) = *dragged;
    if thing != cx.id || of != parent {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else { return };
    let (Some(&(_, first)), Some(&(_, last))) = (rows.first(), rows.last()) else { return };
    let list = Rect::from_min_max(first.min, last.max).expand2(Vec2::new(0.0, SUB_H * 0.5));
    if !list.contains(pointer) {
        return;
    }
    let gap = rows.iter().position(|(_, r)| pointer.y < r.center().y).unwrap_or(rows.len());
    let line_y = rows.get(gap).map_or(last.bottom(), |(_, r)| r.top());
    let x = Rect::from_min_max(Pos2::new(first.left() + SUB_INDENT, first.top()), first.max).x_range();
    ui.painter().hline(x, line_y - 1.0, Stroke::new(2.0, style::TEXT));
    if !ctx.input(|i| i.pointer.any_released()) {
        return;
    }
    if let Some(from) = rows.iter().position(|(l, _)| *l == sub) {
        let to = if from < gap { gap - 1 } else { gap };
        if to != from {
            edits.push(Edit::MoveSubLayer(cx.slot, sub, to));
        }
    }
    egui::DragAndDrop::clear_payload(&ctx);
}

/// The drawn layers under `parent` that a cut on it can follow, the Body first.
pub fn layers_below<'a>(cx: &LookCx, card: &'a Slot, parent: LayerId) -> Vec<&'a Layer> {
    let reg = cx.reg();
    let end = card.layer_index(parent).unwrap_or(0);
    card.layers[..end]
        .iter()
        .filter(|l| l.parent.is_none() && reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Drawn))
        .collect()
}

/// Draws `layer`'s row and returns the rectangle it covers: its grip, its fold arrow when it has
/// sub-layers, thumbnail, name and summary, and its eye. A sub-layer's row sits indented in its
/// layer's box. A click picks it; a double click renames it; the right-click menu duplicates,
/// copies, pastes, moves, adds sub-layers to and deletes it. Alt-click on a Cut's picture shows
/// its layer's cuts alone in the ball view.
pub fn row(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: &Slot,
    layer: &Layer,
    picked: LayerId,
    edits: &mut Vec<Edit>,
) -> Rect {
    let reg = cx.reg();
    let kind = reg.layer(&layer.kind);
    let base = kind.is_some_and(|k| k.base());
    let reaction = kind.is_some_and(|k| k.section() == LayerSection::Reaction);
    let sub = layer.parent.is_some();
    let group = kind.is_some_and(|k| k.section() == LayerSection::Group);
    // The drawn layers above the Body are dragged into another order, sub-layers in their box.
    let draggable = !base && !reaction && !group;
    let subs = if sub { 0 } else { subs_of(card, layer.id).count() };
    let selected = layer.id == picked;
    let ink = if layer.enabled { style::MUTED } else { style::FAINT };
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = ui.horizontal(|ui| {
        ui.set_min_height(if sub { SUB_H } else { ROW_H });
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(if sub { SUB_INDENT } else { 4.0 });
        // The grip, on rows that can be dragged; its empty place on the others.
        let (grip, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
        if draggable {
            paint_grip(ui.painter(), grip, style::FAINT);
        }
        if subs > 0 {
            fold_arrow(ui, inspector, cx, layer);
        }
        let solo = layer.parent.unwrap_or(layer.id);
        let (thumb, thumb_response) =
            ui.allocate_exact_size(Vec2::splat(if sub { 14.0 } else { 16.0 }), Sense::click());
        if sub {
            paint_sub_thumbnail(ui.painter(), thumb, cx, layer);
            // Only balls have a view to show a layer's cuts alone in.
            if layer.kind == kinds::SUB_CUT {
                alt_click_solo(ui, inspector, cx, &thumb_response, solo, layer);
            }
        } else {
            paint_thumbnail(ui.painter(), thumb, cx, card, layer);
        }
        if thumb_response.clicked() && !ui.input(|i| i.modifiers.alt) {
            // As the name does: a group layer is picked in Group mode's own list.
            let picks = if group { &mut inspector.selected_group_layer } else { &mut inspector.selected_layer };
            picks.insert(cx.id, layer.id);
        }

        let renaming = inspector.renaming_layer.as_ref().is_some_and(|(t, l, _)| *t == cx.id && *l == layer.id);
        if renaming {
            rename_field(ui, inspector, cx, layer, edits);
        } else {
            let text = match (selected, layer.enabled) {
                (_, false) => RichText::new(&layer.name).color(style::FAINT),
                (true, true) => RichText::new(&layer.name).font(style::medium(13.0)).color(style::TEXT_STRONG),
                (false, true) => RichText::new(&layer.name).color(style::TEXT),
            };
            let mut button = egui::Button::new(text).frame(false);
            if draggable {
                // A drag-only layer over the button would swallow its clicks, so the button drags.
                button = button.sense(Sense::click_and_drag());
            }
            let response = ui.add(button);
            if draggable {
                match layer.parent {
                    Some(parent) => response.dnd_set_drag_payload(DraggedSub(cx.id, parent, layer.id)),
                    None => response.dnd_set_drag_payload(Dragged(cx.id, layer.id)),
                }
            }
            if subs > 0 && inspector.folded_pieces.contains(&(cx.id, layer.id)) {
                let words = if subs == 1 { "1 sub-layer".to_owned() } else { format!("{subs} sub-layers") };
                ui.label(RichText::new(words).small().color(style::FAINT));
            }
            if response.double_clicked() {
                inspector.renaming_layer = Some((cx.id, layer.id, layer.name.clone()));
                inspector.focus_layer_rename = true;
            } else if response.clicked() && group {
                inspector.selected_group_layer.insert(cx.id, layer.id);
            } else if response.clicked() {
                inspector.selected_layer.insert(cx.id, layer.id);
            }
            let help = kind.map_or("A layer of a kind that isn't installed", |k| k.help());
            let response = response.on_hover_text(help);
            // A shape's Body is the shape itself: nothing to copy, move or delete.
            let shape_body = layer.kind == shape_look::BODY;
            if !shape_body {
                response.context_menu(|ui| {
                    if sub {
                        sub_menu(ui, inspector, cx, card, layer, edits);
                    } else {
                        menu(ui, inspector, cx, card, layer, edits);
                    }
                });
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            if !base {
                let (rect, response) = ui.allocate_exact_size(Vec2::new(18.0, 16.0), Sense::click());
                let name = format!("{} {}", if layer.enabled { "Hide" } else { "Show" }, layer.name);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
                let eye = if response.hovered() { style::TEXT } else { ink };
                crate::tree::paint_eye(ui.painter(), rect, layer.enabled, eye);
                let help = if layer.enabled { "Shown. Click to hide it." } else { "Hidden. Click to show it." };
                if response.on_hover_text(help).clicked() {
                    edits.push(Edit::LayerEnabled(cx.slot, layer.id, !layer.enabled));
                }
            }
            let words = summary(cx, card, layer);
            if !words.is_empty() {
                ui.add(egui::Label::new(RichText::new(words).small().color(ink)).truncate());
            }
        });
    });
    let rect = inner.response.rect;
    let radius = egui::CornerRadius::same(style::RADIUS_ROW);
    if selected {
        let shape = egui::epaint::RectShape::new(
            rect,
            radius,
            style::SELECTED,
            Stroke::new(1.5, style::TEXT),
            egui::StrokeKind::Inside,
        );
        ui.painter().set(background, shape);
    } else if ui.rect_contains_pointer(rect) {
        ui.painter().set(background, egui::epaint::RectShape::filled(rect, radius, style::HOVER));
    }
    rect
}

/// The in-place rename field, taking the keyboard on its first frame. Enter or clicking away
/// keeps the name, Escape drops it, and an empty name goes back to the kind's.
fn rename_field(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, layer: &Layer, edits: &mut Vec<Edit>) {
    let Some((_, _, text)) = inspector.renaming_layer.as_mut() else { return };
    let edit_id = ui.make_persistent_id(("rename layer", cx.id, layer.id.0));
    if std::mem::take(&mut inspector.focus_layer_rename) {
        // The whole name is selected, so typing replaces it.
        let mut state = egui::text_edit::TextEditState::default();
        let end = egui::text::CCursor::new(text.chars().count());
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
        state.store(ui.ctx(), edit_id);
        ui.memory_mut(|m| m.request_focus(edit_id));
    }
    let label = ui.label(RichText::new("Name").small().color(style::MUTED));
    let response = ui.add(egui::TextEdit::singleline(text).id(edit_id).desired_width(140.0)).labelled_by(label.id);
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        inspector.renaming_layer = None;
    } else if (response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)))
        && let Some((_, id, name)) = inspector.renaming_layer.take()
        && name != layer.name
    {
        edits.push(Edit::RenameLayer(cx.slot, id, name));
    }
}

/// The fold arrow of a layer with sub-layers: pointing down while they show, right while they are
/// folded away. A click folds or opens the box.
fn fold_arrow(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, layer: &Layer) {
    let key = (cx.id, layer.id);
    let folded = inspector.folded_pieces.contains(&key);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(10.0, 14.0), Sense::click());
    let name = format!("{} sub-layers of {}", if folded { "Show" } else { "Fold" }, layer.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let ink = if response.hovered() { style::TEXT } else { style::MUTED };
    let c = rect.center();
    let points = if folded {
        vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(3.0, 0.0), c + Vec2::new(-2.0, 4.0)]
    } else {
        vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(4.0, -2.0), c + Vec2::new(0.0, 3.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
    if response.on_hover_text(if folded { "Show its sub-layers" } else { "Fold its sub-layers away" }).clicked() {
        if folded {
            inspector.folded_pieces.remove(&key);
        } else {
            inspector.folded_pieces.insert(key);
        }
    }
}

/// Alt-click on a Cut's picture shows layer `solo`'s cuts alone in the ball view, or stops.
fn alt_click_solo(
    ui: &Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    response: &egui::Response,
    solo: LayerId,
    layer: &Layer,
) {
    let name = format!("Cut picture of {}", layer.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let response = response.clone().on_hover_text("Alt-click to see the cuts alone in the view above");
    if response.clicked() && ui.input(|i| i.modifiers.alt) {
        let view = &mut inspector.ball_view;
        view.solo = if view.solo == Some((cx.id, solo)) { None } else { Some((cx.id, solo)) };
    }
}

/// A Cut's settings as they are at the playhead, a shape's read as a ball's.
pub fn cut_params(cx: &LookCx, layer: &Layer) -> CutLayer {
    if layer.kind == shape_look::SUB_CUT {
        CutLayer::from(ShapeCut::from_params(&cx.params_of(layer.id, ShapeCut::defs())))
    } else {
        CutLayer::from_params(&cx.params_of(layer.id, CutLayer::defs()))
    }
}

/// The copied layer when it is a sub-layer that layer `parent` of `card` takes, with room for it.
fn copied_sub(inspector: &Inspector, cx: &LookCx, card: &Slot, parent: &Layer) -> Option<bpe_core::CopiedLayer> {
    let reg = cx.reg();
    let takes = reg
        .layer(&parent.kind)
        .filter(|k| matches!(k.section(), LayerSection::Drawn | LayerSection::Around) && k.takes_subs())?;
    let room = subs_of(card, parent.id).count() < bpe_core::MAX_SUBS;
    inspector.clipboard.layer.clone().filter(|c| {
        room && takes.takes_sub(&c.kind)
            && reg.layer(&c.kind).is_some_and(|k| k.section() == LayerSection::Sub && k.card() == card.kind)
    })
}

/// A sub-layer row's right-click menu: rename, duplicate, copy, paste a copied sub-layer after it,
/// or delete it. Its values copy and paste in its settings.
fn sub_menu(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, card: &Slot, layer: &Layer, edits: &mut Vec<Edit>) {
    let (slot, id) = (cx.slot, layer.id);
    let Some(parent) = layer.parent.and_then(|p| card.layer(p)) else { return };
    let room = subs_of(card, parent.id).count() < bpe_core::MAX_SUBS;
    ui.set_min_width(170.0);
    if ui.button("Rename").clicked() {
        inspector.renaming_layer = Some((cx.id, id, layer.name.clone()));
        inspector.focus_layer_rename = true;
        ui.close();
    }
    let duplicate = ui.add_enabled(room, egui::Button::new("Duplicate"));
    if duplicate.on_disabled_hover_text("Up to 12 sub-layers").clicked() {
        edits.push(Edit::DuplicateLayer(slot, id));
        ui.close();
    }
    if ui.button("Copy layer").clicked() {
        inspector.clipboard.layer = cx.scene.copy_layer(cx.id, slot, id);
        ui.close();
    }
    let copied = copied_sub(inspector, cx, card, parent);
    let paste = ui.add_enabled(copied.is_some(), egui::Button::new("Paste layer"));
    let paste = paste.on_disabled_hover_text("Copy a sub-layer this layer takes first");
    if let Some(copied) = copied
        && paste.clicked()
    {
        // After this one.
        let at = subs_of(card, parent.id).position(|l| l.id == id).map(|i| i + 1);
        edits.push(Edit::PasteSubLayer(slot, parent.id, copied, at));
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        edits.push(Edit::RemoveLayer(slot, id));
        ui.close();
    }
}

/// The row's right-click menu. What can't apply to the row is greyed: the Body can't be
/// duplicated, moved, given sub-layers or deleted, and a reaction doesn't move.
fn menu(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, card: &Slot, layer: &Layer, edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    let kind = reg.layer(&layer.kind);
    let base = kind.is_some_and(|k| k.base());
    let drawn = kind.is_some_and(|k| k.section() == LayerSection::Drawn) && !base;
    let around = kind.is_some_and(|k| k.section() == LayerSection::Around);
    // The Effects card's effects move among themselves; they have no sub-layers or whole look.
    let effect = kind.is_some_and(|k| k.section() == LayerSection::Effect);
    // The Body takes the sub-layers its kind says (Gives off light).
    let takes_subs = (drawn || around || base) && kind.is_some_and(|k| k.takes_subs());
    let group = kind.is_some_and(|k| k.section() == LayerSection::Group);
    let repeats = kind.is_some_and(|k| k.repeats());
    let index = place_of(card, layer.id).unwrap_or(0);
    let section = |l: &Layer| reg.layer(&l.kind).map(|k| k.section());
    let tops: Vec<&Layer> = top_level(card).collect();
    let top_drawn = tops.iter().filter(|l| section(l) == Some(LayerSection::Drawn)).count().saturating_sub(1);
    // A group layer moves among the group layers, at the end of the card.
    let group_start = tops.iter().position(|l| section(l) == Some(LayerSection::Group)).unwrap_or(tops.len());
    // A trail or particles layer moves among the others of its list.
    let around_start = tops.iter().position(|l| section(l) == Some(LayerSection::Around)).unwrap_or(tops.len());
    let around_end = around_start + tops.iter().filter(|l| section(l) == Some(LayerSection::Around)).count();
    let (can_up, can_down) = if effect {
        (index + 1 < tops.len(), index > 0)
    } else if group {
        (index + 1 < tops.len(), index > group_start)
    } else if around {
        (index + 1 < around_end, index > around_start)
    } else {
        (drawn && index < top_drawn, drawn && index > 1)
    };
    let (slot, id) = (cx.slot, layer.id);
    ui.set_min_width(170.0);
    if ui.add_enabled(!base && repeats, egui::Button::new("Duplicate")).clicked() {
        edits.push(Edit::DuplicateLayer(slot, id));
        ui.close();
    }
    if ui.button("Copy layer").clicked() {
        inspector.clipboard.layer = cx.scene.copy_layer(cx.id, slot, id);
        ui.close();
    }
    // A copied sub-layer pastes onto this layer, as its last; a copied layer beside it.
    let sub = copied_sub(inspector, cx, card, layer);
    let copied = inspector
        .clipboard
        .layer
        .clone()
        .filter(|c| reg.layer(&c.kind).is_some_and(|k| k.card() == card.kind && k.section() != LayerSection::Sub));
    let paste = ui.add_enabled(copied.is_some() || sub.is_some(), egui::Button::new("Paste layer"));
    if paste.clicked() {
        if let Some(sub) = sub {
            edits.push(Edit::PasteSubLayer(slot, id, sub, None));
        } else if let Some(copied) = copied {
            edits.push(Edit::PasteLayer(slot, copied));
        }
        ui.close();
    }
    if effect {
        ui.separator();
        if ui.add_enabled(can_up, egui::Button::new("Move up")).clicked() {
            edits.push(Edit::MoveLayer(slot, id, index + 1));
            ui.close();
        }
        if ui.add_enabled(can_down, egui::Button::new("Move down")).clicked() {
            edits.push(Edit::MoveLayer(slot, id, index - 1));
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            edits.push(Edit::RemoveLayer(slot, id));
            ui.close();
        }
        return;
    }
    if ui.button("Copy whole look").clicked() {
        inspector.clipboard.look = cx.scene.copy_look(cx.reg(), cx.id, slot);
        ui.close();
    }
    // A look pastes onto a card of its own kind: balls' onto balls, shapes' onto shapes.
    let fits = |look: &bpe_core::CopiedLook| {
        look.layers.iter().all(|l| cx.reg().layer(&l.kind).is_some_and(|k| k.card() == card.kind))
    };
    let look = inspector.clipboard.look.clone().filter(fits);
    let paste = ui.add_enabled(look.is_some(), egui::Button::new("Paste whole look"));
    if let Some(look) = look
        && paste.clicked()
    {
        edits.push(Edit::PasteLook(slot, look));
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(can_up, egui::Button::new("Move up")).clicked() {
        edits.push(Edit::MoveLayer(slot, id, index + 1));
        ui.close();
    }
    if ui.add_enabled(can_down, egui::Button::new("Move down")).clicked() {
        edits.push(Edit::MoveLayer(slot, id, index - 1));
        ui.close();
    }
    if !layer.only.is_empty() {
        let every = ui
            .button("Apply to every ball")
            .on_hover_text("Every ball of the group wears it, not just the picked ones");
        if every.clicked() {
            edits.push(Edit::LayerOnly(slot, id, Vec::new()));
            ui.close();
        }
    }
    ui.separator();
    let add = ui.add_enabled_ui(takes_subs, |ui| {
        ui.menu_button("Add sub-layer", |ui| sub_kind_menu(ui, cx, card, id, edits));
    });
    add.response.on_disabled_hover_text("This layer takes no sub-layers");
    ui.separator();
    if ui.add_enabled(!base, egui::Button::new("Delete")).clicked() {
        edits.push(Edit::RemoveLayer(slot, id));
        ui.close();
    }
}

/// Where a drawn layer dragged over `rows` (top first, the Body last) lands: a line marks the
/// gap, and letting go moves it there. The Body stays at the bottom.
pub fn drop_target(ui: &mut Ui, cx: &LookCx, card: &Slot, rows: &[(LayerId, Rect)], edits: &mut Vec<Edit>) {
    let ctx = ui.ctx().clone();
    let Some(dragged) = egui::DragAndDrop::payload::<Dragged>(&ctx) else { return };
    let Dragged(thing, layer) = *dragged;
    if thing != cx.id || section_of(cx, card, layer) != Some(LayerSection::Drawn) {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else { return };
    let Some(&(_, list_top)) = rows.first() else { return };
    let Some(&(_, list_bottom)) = rows.last() else { return };
    let list = Rect::from_min_max(list_top.min, list_bottom.max).expand2(Vec2::new(0.0, ROW_H * 0.5));
    if !list.contains(pointer) {
        return;
    }
    // Rows above the Body: the gap the pointer is nearest, never under the Body.
    let above_body = &rows[..rows.len().saturating_sub(1)];
    let gap = above_body.iter().position(|(_, r)| pointer.y < r.center().y).unwrap_or(above_body.len());
    let line_y = match above_body.get(gap) {
        Some((_, r)) => r.top(),
        None => rows.last().map_or(list.bottom(), |(_, r)| r.top()),
    };
    ui.painter().hline(list_top.x_range(), line_y - 1.0, Stroke::new(2.0, style::TEXT));
    if !ctx.input(|i| i.pointer.any_released()) {
        return;
    }
    // Counted from the top among the others, the gap gives the index it takes.
    let others: Vec<LayerId> = above_body.iter().map(|(l, _)| *l).filter(|l| *l != layer).collect();
    let dragged_at = above_body.iter().position(|(l, _)| *l == layer);
    let place = match dragged_at {
        Some(at) if at < gap => gap - 1,
        _ => gap,
    };
    let to = others.len() + 1 - place;
    if place_of(card, layer) != Some(to) {
        edits.push(Edit::MoveLayer(cx.slot, layer, to));
    }
    egui::DragAndDrop::clear_payload(&ctx);
}

/// The section of `card`'s layer `id`.
fn section_of(cx: &LookCx, card: &Slot, id: LayerId) -> Option<LayerSection> {
    card.layer(id).and_then(|l| cx.reg().layer(&l.kind)).map(|k| k.section())
}

/// Where a trail or particles layer dragged over the `rows` of their list (top first) lands: a
/// line marks the gap, and letting go moves it there, among the others of the list only.
pub fn around_drop_target(ui: &mut Ui, cx: &LookCx, card: &Slot, rows: &[(LayerId, Rect)], edits: &mut Vec<Edit>) {
    section_drop_target(ui, cx, card, rows, LayerSection::Around, edits);
}

/// Where a layer of `section` dragged over the `rows` of its list (top first) lands, among the
/// others of its list only (the trails and particles, the Effects card's effects).
pub fn section_drop_target(
    ui: &mut Ui,
    cx: &LookCx,
    card: &Slot,
    rows: &[(LayerId, Rect)],
    section: LayerSection,
    edits: &mut Vec<Edit>,
) {
    let ctx = ui.ctx().clone();
    let Some(dragged) = egui::DragAndDrop::payload::<Dragged>(&ctx) else { return };
    let Dragged(thing, layer) = *dragged;
    if thing != cx.id || section_of(cx, card, layer) != Some(section) {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else { return };
    let (Some(&(_, list_top)), Some(&(_, list_bottom))) = (rows.first(), rows.last()) else { return };
    let list = Rect::from_min_max(list_top.min, list_bottom.max).expand2(Vec2::new(0.0, ROW_H * 0.5));
    if !list.contains(pointer) {
        return;
    }
    let gap = rows.iter().position(|(_, r)| pointer.y < r.center().y).unwrap_or(rows.len());
    let line_y = rows.get(gap).map_or(list_bottom.bottom(), |(_, r)| r.top());
    ui.painter().hline(list_top.x_range(), line_y - 1.0, Stroke::new(2.0, style::TEXT));
    if !ctx.input(|i| i.pointer.any_released()) {
        return;
    }
    // The layers before the list (the drawn ones), then the others of the list from the bottom.
    let before = top_level(card).take_while(|l| section_of(cx, card, l.id) != Some(section)).count();
    let others = rows.iter().filter(|(l, _)| *l != layer).count();
    let place = match rows.iter().position(|(l, _)| *l == layer) {
        Some(at) if at < gap => gap - 1,
        _ => gap,
    };
    let to = before + others - place;
    if place_of(card, layer) != Some(to) {
        edits.push(Edit::MoveLayer(cx.slot, layer, to));
    }
    egui::DragAndDrop::clear_payload(&ctx);
}

/// A few words on the right of a row about its main settings: "4 px · Inside", "Add 100 %", and
/// "only 3 balls" for a layer only some balls of the group wear.
fn summary(cx: &LookCx, card: &Slot, layer: &Layer) -> String {
    let words = settings_summary(cx, card, layer);
    match layer.only.len() {
        0 => words,
        n => {
            let only = if n == 1 { "only 1 ball".to_owned() } else { format!("only {n} balls") };
            if words.is_empty() { only } else { format!("{only} · {words}") }
        }
    }
}

fn settings_summary(cx: &LookCx, card: &Slot, layer: &Layer) -> String {
    let Some(kind) = cx.reg().layer(&layer.kind) else { return String::new() };
    let p = cx.params_of(layer.id, kind.settings());
    let percent = |v: f32| format!("{:.0} %", v * 100.0);
    if let Some(parent) = layer.parent {
        return sub_words(cx, card, parent, layer, &p);
    }
    if bpe_kit::modules::effects::is_effect(&layer.kind) {
        return effect_words(cx, layer);
    }
    match layer.kind.as_str() {
        shape_look::BODY => shape_words(cx),
        shape_look::PAINT => {
            let l = shape_look::PaintLayer::from_params(&p);
            let stops = &l.colours.stops;
            let words = if stops.len() > 1 {
                let form = p
                    .get(&bpe_core::gradient_extras::key("colours", "form"))
                    .and_then(<bpe_core::GradientForm as bpe_core::SettingValue>::from_value)
                    .unwrap_or_default();
                format!("{} colours · {}", stops.len(), form.label())
            } else {
                colour_word(l.colours.first())
            };
            with_blend(words, l.blend)
        }
        shape_look::OUTLINE => {
            let l = shape_look::ShapeOutline::from_params(&p);
            with_blend(format!("{:.0} px · {}", l.width, choice_label(l.sits)), l.blend)
        }
        shape_look::GLOW => {
            let l = shape_look::ShapeGlow::from_params(&p);
            with_blend(format!("{:.1} · {:.0} px", l.strength, l.size), l.blend)
        }
        shape_look::SHADER => {
            let l = kinds::ShaderLayer::from_params(&p);
            let broken = l.effect == kinds::shader::OWN && super::code_box::problem(&l.source).is_some();
            let name = if broken { "Own code that doesn't run".to_owned() } else { l.effect_name() };
            with_blend(name, l.blend)
        }
        shape_look::BLEND => {
            let l = kinds::BlendLayer::from_params(&p);
            format!("{} {}", choice_label(l.blend), percent(l.blend_amount))
        }
        kinds::BODY => {
            let b = kinds::BodyLayer::from_params(&p);
            let stops = &b.fill.colours.stops;
            let colour =
                if stops.len() > 1 { format!("{} colours", stops.len()) } else { colour_word(b.fill.colours.first()) };
            format!("{} · {colour}", choice_label(b.ball_shape))
        }
        kinds::PATTERN => {
            let l = kinds::PatternLayer::from_params(&p);
            with_blend(choice_label(l.inside.pattern), l.blend)
        }
        kinds::OUTLINE => {
            let l = kinds::OutlineLayer::from_params(&p);
            with_blend(format!("{:.0} px · {}", l.width, choice_label(l.sits)), l.blend)
        }
        kinds::SHINE => {
            with_blend(percent(kinds::ShineLayer::from_params(&p).strength), kinds::ShineLayer::from_params(&p).blend)
        }
        kinds::GLOW => {
            let l = kinds::GlowLayer::from_params(&p);
            with_blend(format!("{:.1} · size {:.1}", l.strength, l.size), l.blend)
        }
        kinds::SHADING => {
            let l = kinds::ShadingLayer::from_params(&p);
            with_blend(format!("{} · {}", choice_label(l.kind), percent(l.amount)), l.blend)
        }
        kinds::SHADER => {
            let l = kinds::ShaderLayer::from_params(&p);
            let broken = l.effect == kinds::shader::OWN && super::code_box::problem(&l.source).is_some();
            let name = if broken { "Own code that doesn't run".to_owned() } else { l.effect_name() };
            with_blend(name, l.blend)
        }
        kinds::BLEND => {
            let l = kinds::BlendLayer::from_params(&p);
            format!("{} {}", choice_label(l.blend), percent(l.blend_amount))
        }
        kinds::TRAIL => trail_words(&trail::TrailLayer::from_params(&p)),
        kinds::PARTICLES => {
            let l = particles::ParticlesLayer::from_params(&p);
            let when = match l.emit {
                particles::Emit::OnHit => "on hit".to_owned(),
                particles::Emit::WhenItPops => "when it pops".to_owned(),
                _ => a_second(l.rate),
            };
            format!("{} · {when}", choice_label(l.made_of))
        }
        kinds::SHAPE_PARTICLES => {
            let l = particles::ShapeParticlesLayer::from_params(&p);
            let when = match l.emit {
                particles::ShapeEmit::OnHit => "on hit".to_owned(),
                particles::ShapeEmit::Steady => a_second(l.rate),
            };
            format!("{} · {when}", choice_label(l.made_of))
        }
        kinds::FLASH => format!("{:.1}", kinds::FlashOnHit::from_params(&p).flash),
        kinds::HUE => format!("{:.0}° a hit", kinds::ColourOnHit::from_params(&p).hue_per_hit),
        kinds::STRETCH => percent(kinds::StretchWhenFast::from_params(&p).stretch),
        kinds::RAINBOW => format!("{:.2} turns/s", kinds::ColourOverTime::from_params(&p).rainbow),
        kinds::TURNING => {
            let t = kinds::Turning::from_params(&p);
            if t.face_motion { "Faces its way".into() } else { format!("{:.0}°/s", t.spin) }
        }
        kinds::GROUP_SHADER => {
            let g = kinds::GroupShader::from_params(&p);
            let broken = g.shader.effect == kinds::shader::OWN && super::code_box::problem(&g.shader.source).is_some();
            let name = if broken { "Own code that doesn't run".to_owned() } else { g.shader.effect_name() };
            format!("{name} · {} {}", choice_label(g.mix), percent(g.amount))
        }
        kinds::GROUP_GRADIENT => {
            let g = kinds::GroupGradient::from_params(&p);
            match g.mix {
                kinds::GroupMix::Off => format!("{} · off", choice_label(g.spread)),
                mix => format!("{} · {} {}", choice_label(g.spread), choice_label(mix), percent(g.amount)),
            }
        }
        _ => String::new(),
    }
}

/// An effect of the Effects card in a few words: "Light only · Add 60 %".
fn effect_words(cx: &LookCx, layer: &Layer) -> String {
    let Some(kind) = cx.reg().layer(&layer.kind) else { return String::new() };
    let p = cx.params_of(layer.id, kind.settings());
    let mix = match p.get(bpe_kit::modules::effects::MIX) {
        Some(bpe_core::Value::Number(m)) => *m as f32,
        _ => 1.0,
    };
    let blend = p.get("blend").and_then(<bpe_core::Blend as bpe_core::SettingValue>::from_value).unwrap_or_default();
    let mut words = Vec::new();
    if layer.kind == bpe_kit::modules::effects::OWN
        && let Some(bpe_core::Value::Text(code)) = p.get("code")
        && bpe_kit::modules::effects::own_problem(code).is_some()
    {
        words.push("doesn't run".to_owned());
    }
    match bpe_kit::modules::effects::takes_of(&p) {
        bpe_kit::modules::effects::TakesChoice::WholePicture => {}
        t => words.push(choice_label(t)),
    }
    if blend != bpe_core::Blend::Normal || (mix - 1.0).abs() > 1e-4 {
        let name = if blend == bpe_core::Blend::Normal { "Mix".to_owned() } else { choice_label(blend) };
        words.push(format!("{name} {:.0} %", mix * 100.0));
    }
    words.join(" · ")
}

/// A trail in a few words: "Streak · 0.5 s", "Dots · 300 px", "Line · stays", "Ghosts · 4".
fn trail_words(l: &trail::TrailLayer) -> String {
    let length = match (l.kind, l.length_by) {
        (trail::TrailKind::Ghosts, _) => format!("{}", l.ghosts.max(0)),
        (trail::TrailKind::Line, _) if l.forever => "stays".to_owned(),
        (_, trail::LengthBy::Time) => format!("{} s", trim(l.seconds)),
        (_, trail::LengthBy::Distance) => format!("{:.0} px", l.pixels),
    };
    format!("{} · {length}", choice_label(l.kind))
}

/// "60 a second", "2.5 a second".
fn a_second(rate: f32) -> String {
    format!("{} a second", trim(rate))
}

/// A number with at most two places and no trailing zeros: 0.5, 2, 1.25.
fn trim(x: f32) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A shape's outline in a few words: "Circle", "Ellipse", "6 sides", "7 points, 1 curve", then "filled" when
/// it is solid.
fn shape_words(cx: &LookCx) -> String {
    let Some(thing) = cx.scene.things.get(&cx.id) else { return String::new() };
    let s = bpe_kit::ShapeSettings::from_params(&thing.params);
    let outline = match (s.start, s.moulded()) {
        (bpe_kit::ShapeStart::Ellipse, _) => "Ellipse".to_owned(),
        (_, Some(path)) => path.words(),
        (bpe_kit::ShapeStart::Polygon, None) => format!("{} sides", s.sides),
        (bpe_kit::ShapeStart::Circle, None) => "Circle".to_owned(),
    };
    if s.fill == bpe_kit::ShapeFill::Filled { format!("{outline} · filled") } else { outline }
}

/// A sub-layer's main settings in a few words: a Cut's [`cut_words`], "0.8 strong", "6 px ·
/// Inside", "45°", "Tint", "0.3 s", "30° a hit".
fn sub_words(cx: &LookCx, card: &Slot, parent: LayerId, layer: &Layer, p: &bpe_core::Params) -> String {
    let percent = |v: f32| format!("{:.0} %", v * 100.0);
    let shape = layer.kind.starts_with("shape.");
    match sub_name(&layer.kind).unwrap_or("") {
        "cut" => cut_words(cx, card, parent, &cut_params(cx, layer)),
        "glow" => format!("{:.1} strong", kinds::SubGlow::from_params(p).strength),
        "outline" => {
            let o = kinds::SubOutline::from_params(p);
            format!("{:.0} px · {}", o.width, choice_label(o.sits))
        }
        "shadow" => format!("{:.0}°", kinds::SubShadow::from_params(p).angle),
        "shading" if shape => percent(shape_look::ShapeShading::from_params(p).amount),
        "shading" => {
            let l = kinds::ShadingLayer::from_params(p);
            format!("{} · {}", choice_label(l.kind), percent(l.amount))
        }
        "shine" => percent(kinds::ShineLayer::from_params(p).strength),
        "shader" => {
            let l = kinds::ShaderLayer::from_params(p);
            let broken = l.effect == kinds::shader::OWN && super::code_box::problem(&l.source).is_some();
            if broken { "Own code that doesn't run".to_owned() } else { l.effect_name() }
        }
        "recolour" => choice_label(kinds::SubRecolour::from_params(p).mode),
        "flash" => format!("{:.1} s", kinds::SubFlash::from_params(p).length),
        "hue" => format!("{:.0}° a hit", kinds::SubHue::from_params(p).turn_per_hit),
        _ => String::new(),
    }
}

/// What a cut on `parent` shows by: "Layer below", "Top half", "Brush · 3 strokes", or a
/// warning when the layer it follows can't be followed.
fn cut_words(cx: &LookCx, card: &Slot, parent: LayerId, m: &CutLayer) -> String {
    let words = match m.from {
        CutFrom::LayerBelow => "Layer below".to_owned(),
        CutFrom::Layer => {
            let below = layers_below(cx, card, parent);
            match below.iter().find(|l| i64::from(l.id.0) == i64::from(m.target) && l.enabled) {
                Some(l) => l.name.clone(),
                None => return "cut by a missing layer".to_owned(),
            }
        }
        CutFrom::Shape if m.shape == CutShape::Half && m.angle == 0.0 => "Top half".to_owned(),
        CutFrom::Shape => choice_label(m.shape),
        CutFrom::Brush => format!("Brush · {}", crate::widgets::strokes_words(&m.strokes).to_lowercase()),
        CutFrom::Light => "Light parts".to_owned(),
        CutFrom::Dark => "Dark parts".to_owned(),
    };
    if m.invert { format!("{words} · inverted") } else { words }
}

/// `words`, then the blend when it isn't Normal.
fn with_blend(words: String, blend: bpe_core::Blend) -> String {
    if blend == bpe_core::Blend::Normal { words } else { format!("{words} · {}", choice_label(blend)) }
}

/// A colour in a word or two: white, black, or its hex.
fn colour_word(c: Color) -> String {
    match (c.r.min(c.g).min(c.b), c.r.max(c.g).max(c.b)) {
        (250.., _) => "white".into(),
        (_, ..=5) => "black".into(),
        _ => format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
    }
}

/// A colour of the scene as a thumbnail shows it.
fn shown(c: Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b) // scene colour
}

/// The body's colour as its thumbnail and same-as-body colours show it.
fn body_colour(cx: &LookCx, card: &Slot) -> Color {
    let Some(body) = card.layers.first() else { return Color::WHITE };
    let p = cx.params_of(body.id, kinds::BodyLayer::defs());
    kinds::BodyLayer::from_params(&p).fill.colours.first()
}

/// A small picture of what the layer does, in its main colour.
fn paint_thumbnail(painter: &egui::Painter, rect: Rect, cx: &LookCx, card: &Slot, layer: &Layer) {
    let c = rect.center();
    let r = rect.width() * 0.42;
    let body = shown(body_colour(cx, card));
    let fade = |col: Color32| if layer.enabled { col } else { col.gamma_multiply(0.4) };
    let Some(kind) = cx.reg().layer(&layer.kind) else { return };
    let p = cx.params_of(layer.id, kind.settings());
    let own =
        |from: kinds::ColourFrom, colour: Color| if from == kinds::ColourFrom::Own { shown(colour) } else { body };
    match layer.kind.as_str() {
        kinds::BODY => {
            painter.circle_filled(c, r, fade(body));
        }
        shape_look::BODY => {
            // A hexagon outline: the shape, plain white.
            painter.add(egui::Shape::closed_line(hexagon(c, r), Stroke::new(1.5, fade(style::TEXT_STRONG))));
        }
        shape_look::PAINT => {
            // A hexagon in its colours, in bands from top to bottom.
            let l = shape_look::PaintLayer::from_params(&p);
            painter.add(egui::Shape::convex_polygon(hexagon(c, r), fade(shown(l.colours.first())), Stroke::NONE));
            if l.colours.stops.len() > 1 {
                let last = shown(l.colours.stops.last().map_or(l.colours.first(), |s| s.color));
                let lower: Vec<Pos2> = hexagon(c, r).into_iter().filter(|p| p.y >= c.y - 0.01).collect();
                painter.add(egui::Shape::convex_polygon(lower, fade(last), Stroke::NONE));
            }
        }
        shape_look::OUTLINE => {
            let l = shape_look::ShapeOutline::from_params(&p);
            let col =
                if l.colour_from == kinds::ColourFrom::Own { shown(l.colour.first()) } else { style::TEXT_STRONG };
            painter.add(egui::Shape::closed_line(hexagon(c, r - 1.0), Stroke::new(2.0, fade(col))));
        }
        shape_look::GLOW => {
            let l = shape_look::ShapeGlow::from_params(&p);
            let col =
                if l.colour_from == kinds::ColourFrom::Own { shown(l.colour.first()) } else { style::TEXT_STRONG };
            for (k, a) in [(1.0, 0.25), (0.7, 0.45), (0.4, 1.0)] {
                painter.add(egui::Shape::convex_polygon(hexagon(c, r * k), fade(col.gamma_multiply(a)), Stroke::NONE));
            }
        }
        shape_look::SHADER => {
            // A wave over a hexagon.
            painter.add(egui::Shape::convex_polygon(hexagon(c, r), fade(style::RAISED), Stroke::NONE));
            let points: Vec<Pos2> = (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    c + Vec2::new((t - 0.5) * 1.6 * r, (t * std::f32::consts::TAU).sin() * r * 0.35)
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.3, fade(style::TEXT_STRONG))));
        }
        shape_look::BLEND => {
            let stroke = Stroke::new(1.3, fade(style::TEXT));
            painter.circle_stroke(c + Vec2::new(-r * 0.3, 0.0), r * 0.6, stroke);
            painter.circle_stroke(c + Vec2::new(r * 0.3, 0.0), r * 0.6, stroke);
        }
        kinds::OUTLINE => {
            let l = kinds::OutlineLayer::from_params(&p);
            painter.circle_stroke(c, r - 1.0, Stroke::new(2.0, fade(own(l.colour_from, l.colour.first()))));
        }
        kinds::GLOW => {
            let l = kinds::GlowLayer::from_params(&p);
            let col = own(l.colour_from, l.colour.first());
            for (k, a) in [(1.0, 0.25), (0.7, 0.45), (0.4, 1.0)] {
                painter.circle_filled(c, r * k, fade(col.gamma_multiply(a)));
            }
        }
        kinds::SHINE => {
            painter.circle_filled(c, r, fade(style::RAISED));
            painter.circle_filled(c + Vec2::new(-r * 0.35, -r * 0.35), r * 0.35, fade(style::UNTINTED));
        }
        kinds::PATTERN => {
            painter.circle_filled(c, r, fade(style::RAISED));
            for dx in [-0.45, 0.15] {
                let stripe = Rect::from_center_size(c + Vec2::new(dx * r, 0.0), Vec2::new(r * 0.35, r * 1.5));
                painter.rect_filled(stripe, 1.0, fade(body));
            }
        }
        kinds::SHADING => {
            // The body, darker towards one side.
            painter.circle_filled(c, r, fade(body));
            let dark = style::OUTLINE.gamma_multiply(0.55);
            for (k, a) in [(1.0, 0.35), (0.7, 0.6)] {
                painter.circle_filled(c + Vec2::new(r * 0.35, r * 0.35), r * k * 0.75, fade(dark.gamma_multiply(a)));
            }
        }
        kinds::SHADER => {
            // A wave over the body.
            painter.circle_filled(c, r, fade(style::RAISED));
            let points: Vec<Pos2> = (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    c + Vec2::new((t - 0.5) * 1.6 * r, (t * std::f32::consts::TAU).sin() * r * 0.35)
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.3, fade(body))));
        }
        kinds::GROUP_SHADER => {
            // A wave across three balls.
            for dx in [-0.62, 0.0, 0.62] {
                painter.circle_filled(c + Vec2::new(dx * r, 0.0), r * 0.36, fade(style::RAISED));
            }
            let points: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let t = i as f32 / 10.0;
                    c + Vec2::new((t - 0.5) * 2.0 * r, (t * std::f32::consts::TAU).sin() * r * 0.3)
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.3, fade(body))));
        }
        kinds::GROUP_GRADIENT => {
            // The gradient's colours, as a bar.
            let g = kinds::GroupGradient::from_params(&p);
            let bar = Rect::from_center_size(c, Vec2::new(r * 2.0, r * 1.2));
            let steps = 8;
            for k in 0..steps {
                let t = (k as f32 + 0.5) / steps as f32;
                let colour = shown(Color::from_linear(g.colours.sample(t)));
                let x0 = bar.left() + bar.width() * k as f32 / steps as f32;
                let piece = Rect::from_min_max(
                    Pos2::new(x0, bar.top()),
                    Pos2::new(x0 + bar.width() / steps as f32 + 0.5, bar.bottom()),
                );
                painter.rect_filled(piece, 0.0, fade(colour));
            }
        }
        kinds::BLEND => {
            let stroke = Stroke::new(1.3, fade(style::TEXT));
            painter.circle_stroke(c + Vec2::new(-r * 0.3, 0.0), r * 0.6, stroke);
            painter.circle_stroke(c + Vec2::new(r * 0.3, 0.0), r * 0.6, stroke);
        }
        kinds::TRAIL => {
            let l = trail::TrailLayer::from_params(&p);
            let col = if l.source == trail::TrailSource::Own { shown(l.colour.colours.first()) } else { body };
            paint_trail_thumbnail(painter, c, r, l.kind, &|a| fade(col.gamma_multiply(a)));
        }
        kinds::PARTICLES => {
            let l = particles::ParticlesLayer::from_params(&p);
            let col = if l.source == particles::ParticleSource::Own { shown(l.colour.colours.first()) } else { body };
            paint_particles_thumbnail(painter, c, r, fade(col));
        }
        kinds::SHAPE_PARTICLES => {
            let l = particles::ShapeParticlesLayer::from_params(&p);
            let own = l.source == particles::ParticleSource::Own;
            let col = if own { shown(l.colour.colours.first()) } else { style::TEXT_STRONG };
            paint_particles_thumbnail(painter, c, r, fade(col));
        }
        _ => {
            // A reaction: a small spark.
            let stroke = Stroke::new(1.3, fade(style::MUTED));
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4 * 2.0 + std::f32::consts::FRAC_PI_4;
                let d = Vec2::angled(a);
                painter.line_segment([c + d * r * 0.3, c + d * r], stroke);
            }
        }
    }
}

/// A short trail of `kind` running up to a ball at the top right, `col(a)` its colour `a` strong.
fn paint_trail_thumbnail(
    painter: &egui::Painter,
    c: Pos2,
    r: f32,
    kind: trail::TrailKind,
    col: &dyn Fn(f32) -> Color32,
) {
    let head = c + Vec2::new(r * 0.55, -r * 0.55);
    let tail = c + Vec2::new(-r, r);
    let along = |t: f32| tail + (head - tail) * t;
    let side = (head - tail).normalized().rot90();
    match kind {
        trail::TrailKind::Streak | trail::TrailKind::Ribbon => {
            let w = r * 0.38;
            let wide = if kind == trail::TrailKind::Ribbon { 0.8 } else { 1.0 };
            let points = vec![along(0.0), along(1.0) + side * w * wide, along(1.0) - side * w * wide];
            painter.add(egui::Shape::convex_polygon(points, col(0.55), Stroke::NONE));
        }
        trail::TrailKind::Line => {
            painter.line_segment([along(0.0), along(1.0)], Stroke::new(1.2, col(0.8)));
        }
        trail::TrailKind::Dots => {
            for (t, a) in [(0.15, 0.35), (0.45, 0.6), (0.75, 0.85)] {
                painter.circle_filled(along(t), r * 0.13, col(a));
            }
        }
        trail::TrailKind::Ghosts => {
            for (t, a) in [(0.2, 0.25), (0.55, 0.5)] {
                painter.circle_filled(along(t), r * 0.3, col(a));
            }
        }
        trail::TrailKind::Smoke => {
            for (t, k, a) in [(0.1, 0.42, 0.25), (0.45, 0.33, 0.4), (0.75, 0.25, 0.55)] {
                painter.circle_filled(along(t), r * k, col(a));
            }
        }
    }
    painter.circle_filled(head, r * 0.36, col(1.0));
}

/// A few particles flying out from the middle.
fn paint_particles_thumbnail(painter: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    for (dx, dy, k) in [(-0.6, -0.5, 0.16), (0.55, -0.6, 0.13), (0.7, 0.35, 0.17), (-0.5, 0.6, 0.12), (0.05, 0.05, 0.2)]
    {
        painter.circle_filled(c + Vec2::new(dx * r, dy * r), r * k * 1.4, col);
    }
}

/// The corners of a hexagon around `c` reaching `r`, a corner on each side.
fn hexagon(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..6).map(|k| c + Vec2::angled(k as f32 * std::f32::consts::FRAC_PI_3) * r).collect()
}

/// A small picture of what a sub-layer does: a Cut's [`paint_cut_thumbnail`], a halo, a ring, an
/// offset disc, a half-dark disc, a shine spot, "fx", a swatch, a star or a colour wheel.
fn paint_sub_thumbnail(painter: &egui::Painter, rect: Rect, cx: &LookCx, layer: &Layer) {
    let c = rect.center();
    let r = rect.width() * 0.42;
    let fade = |col: Color32| if layer.enabled { col } else { col.gamma_multiply(0.4) };
    let Some(kind) = cx.reg().layer(&layer.kind) else { return };
    let p = cx.params_of(layer.id, kind.settings());
    let own = |from: kinds::SubColourFrom, colour: Color| {
        if from == kinds::SubColourFrom::Own { shown(colour) } else { style::TEXT_STRONG }
    };
    match sub_name(&layer.kind).unwrap_or("") {
        "cut" => paint_cut_thumbnail(painter, rect, &cut_params(cx, layer), layer.enabled),
        "glow" => {
            let g = kinds::SubGlow::from_params(&p);
            let col = own(g.colour_from, g.colour.first());
            for (k, a) in [(1.0, 0.2), (0.75, 0.4)] {
                painter.circle_filled(c, r * k, fade(col.gamma_multiply(a)));
            }
            painter.circle_stroke(c, r * 0.5, Stroke::new(1.2, fade(col)));
        }
        "outline" => {
            let o = kinds::SubOutline::from_params(&p);
            painter.circle_stroke(c, r - 1.0, Stroke::new(2.0, fade(own(o.colour_from, o.colour.first()))));
        }
        "shadow" => {
            let s = kinds::SubShadow::from_params(&p);
            let way = Vec2::angled(s.angle.to_radians()) * r * 0.35;
            painter.circle_filled(c + way, r * 0.7, fade(shown(s.colour).gamma_multiply(0.8)));
            painter.circle_filled(c - way * 0.5, r * 0.7, fade(style::TEXT_STRONG));
        }
        "shading" => {
            painter.circle_filled(c, r, fade(style::TEXT_STRONG));
            let half: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = std::f32::consts::FRAC_PI_4 + std::f32::consts::PI * i as f32 / 12.0;
                    c + Vec2::angled(t) * r
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, fade(style::OUTLINE.gamma_multiply(0.7)), Stroke::NONE));
        }
        "shine" => {
            painter.circle_filled(c, r, fade(style::RAISED));
            painter.circle_filled(c + Vec2::new(-r * 0.35, -r * 0.35), r * 0.38, fade(style::UNTINTED));
        }
        "shader" => {
            let font = egui::FontId::proportional(rect.height() * 0.75);
            painter.text(c, egui::Align2::CENTER_CENTER, "fx", font, fade(style::TEXT_STRONG));
        }
        "recolour" => {
            let l = kinds::SubRecolour::from_params(&p);
            let (a, b) = match l.mode {
                kinds::RecolourMode::Gradient => {
                    let last = l.colours.stops.last().map_or(l.colours.first(), |s| s.color);
                    (shown(l.colours.first()), shown(last))
                }
                _ => (shown(l.tint), shown(l.tint)),
            };
            let square = Rect::from_center_size(c, Vec2::splat(r * 1.7));
            painter.rect_filled(square.with_max_x(c.x), 1.0, fade(a));
            painter.rect_filled(square.with_min_x(c.x), 1.0, fade(b));
        }
        "flash" => {
            let col = fade(shown(kinds::SubFlash::from_params(&p).colour));
            let star: Vec<Pos2> = (0..8)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::FRAC_PI_4;
                    c + Vec2::angled(t) * if i % 2 == 0 { r } else { r * 0.3 }
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(star[..3].to_vec(), col, Stroke::NONE));
            for k in 0..4 {
                let tip = star[2 * k];
                let left = star[(2 * k + 7) % 8];
                let right = star[2 * k + 1];
                painter.add(egui::Shape::convex_polygon(vec![c, left, tip, right], col, Stroke::NONE));
            }
        }
        "hue" => {
            // Three slices of the colour wheel.
            let wheel = [Color::rgb(0xff, 0x55, 0x55), Color::rgb(0x55, 0xdd, 0x66), Color::rgb(0x55, 0x88, 0xff)];
            for (k, colour) in wheel.into_iter().enumerate() {
                let start = k as f32 * std::f32::consts::TAU / 3.0 - std::f32::consts::FRAC_PI_2;
                let mut points = vec![c];
                for i in 0..=8 {
                    points.push(c + Vec2::angled(start + std::f32::consts::TAU / 3.0 * i as f32 / 8.0) * r);
                }
                painter.add(egui::Shape::convex_polygon(points, fade(shown(colour)), Stroke::NONE));
            }
        }
        _ => {}
    }
}

/// A small black disc, white where the cut lets its layer show.
fn paint_cut_thumbnail(painter: &egui::Painter, rect: Rect, m: &CutLayer, enabled: bool) {
    let c = rect.center();
    let r = rect.width() * 0.42;
    let (mut on, mut off) = (style::TEXT_STRONG, style::OUTLINE);
    if m.invert {
        std::mem::swap(&mut on, &mut off);
    }
    if !enabled {
        on = on.gamma_multiply(0.4);
    }
    painter.circle_filled(c, r, off);
    // A pie from angle `a` (radians, 0 up, clockwise) across `share` of a turn.
    let pie = |a: f32, share: f32| {
        let mut points = vec![c];
        for i in 0..=16 {
            let t = a + share * std::f32::consts::TAU * (i as f32 / 16.0 - 0.5);
            points.push(c + Vec2::new(t.sin(), -t.cos()) * r);
        }
        egui::Shape::convex_polygon(points, on, Stroke::NONE)
    };
    let angle = m.angle.to_radians();
    match m.from {
        CutFrom::LayerBelow | CutFrom::Layer => {
            painter.circle_filled(c, r * 0.55, on);
        }
        CutFrom::Shape => match m.shape {
            CutShape::Half => {
                painter.add(pie(angle, 0.5));
            }
            CutShape::Wedge => {
                painter.add(pie(angle, m.size.clamp(0.05, 1.0)));
            }
            CutShape::Quarters => {
                painter.add(pie(angle - std::f32::consts::FRAC_PI_4, 0.25));
                painter.add(pie(angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_4, 0.25));
            }
            CutShape::Ring => {
                painter.circle_stroke(c, r * 0.65, Stroke::new(r * 0.35, on));
            }
            CutShape::Circle => {
                painter.circle_filled(c, r * m.size.clamp(0.15, 1.0), on);
            }
            CutShape::Band => {
                let d = Vec2::new(angle.cos(), angle.sin()) * r;
                painter.line_segment([c - d, c + d], Stroke::new(r * m.size.clamp(0.2, 1.0), on));
            }
        },
        CutFrom::Brush => {
            let points =
                [c + Vec2::new(-r * 0.6, r * 0.2), c + Vec2::new(-r * 0.1, -r * 0.4), c + Vec2::new(r * 0.5, r * 0.3)];
            painter.add(egui::Shape::line(points.to_vec(), Stroke::new(r * 0.35, on)));
        }
        CutFrom::Light => {
            painter.circle_filled(c + Vec2::new(-r * 0.3, -r * 0.3), r * 0.45, on);
        }
        CutFrom::Dark => {
            painter.circle_filled(c + Vec2::new(r * 0.3, r * 0.3), r * 0.45, on);
        }
    }
    painter.circle_stroke(c, r, Stroke::new(1.0, style::LINE));
}

/// The grip's two columns of dots.
fn paint_grip(painter: &egui::Painter, rect: Rect, ink: Color32) {
    for row in 0..3 {
        for col in 0..2 {
            let at = Pos2::new(rect.left() + 2.0 + col as f32 * 4.0, rect.center().y - 4.0 + row as f32 * 4.0);
            painter.circle_filled(at, 1.0, ink);
        }
    }
}
