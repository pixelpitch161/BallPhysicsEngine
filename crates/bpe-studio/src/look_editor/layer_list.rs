//! One row of the Look editor's lists, and dragging the drawn layers into another order.

use bpe_core::{Color, Layer, LayerId, LayerSection, Settings, Slot};
use bpe_kit::modules::look::layers::{self as kinds, MaskFrom, MaskLayer, MaskShape, gpu::choice_label};
use bpe_kit::modules::look::shape as shape_look;
use eframe::egui::{self, Color32, Key, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::LookCx;
use crate::inspector::{Edit, Inspector};
use crate::style;

/// The height of a layer's row.
const ROW_H: f32 = 28.0;

/// What a dragged row carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dragged(bpe_core::ThingId, LayerId);

/// The layers not inside another (masks are inside the layer they limit), in card order. A
/// layer's place among them is the place [`Edit::MoveLayer`] counts.
pub fn top_level(card: &Slot) -> impl Iterator<Item = &Layer> {
    card.layers.iter().filter(|l| l.parent.is_none())
}

/// Where layer `id` is among the [`top_level`] layers, the Body 0.
pub fn place_of(card: &Slot, id: LayerId) -> Option<usize> {
    top_level(card).position(|l| l.id == id)
}

/// Layer `parent`'s masks, in card order.
pub fn masks_of(card: &Slot, parent: LayerId) -> impl Iterator<Item = &Layer> {
    card.layers.iter().filter(move |l| l.parent == Some(parent))
}

/// The drawn layers under `parent` that a mask on it can follow, the Body first.
pub fn layers_below<'a>(cx: &LookCx, card: &'a Slot, parent: LayerId) -> Vec<&'a Layer> {
    let reg = cx.reg();
    let end = card.layer_index(parent).unwrap_or(0);
    card.layers[..end]
        .iter()
        .filter(|l| l.parent.is_none() && reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Drawn))
        .collect()
}

/// Draws `layer`'s row and returns the rectangle it covers: its grip, thumbnail, name and
/// summary, and its eye. A mask's row sits indented under its layer's. A click picks it; a
/// double click renames it; the right-click menu duplicates, copies, pastes, moves, masks and
/// deletes it. Alt-click on a mask's picture shows the masks alone in the ball view.
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
    let mask = layer.parent.is_some();
    let group = kind.is_some_and(|k| k.section() == LayerSection::Group);
    // Only the drawn layers above the Body are dragged into another order.
    let draggable = !base && !reaction && !mask && !group;
    let selected = layer.id == picked;
    let ink = if layer.enabled { style::MUTED } else { style::FAINT };
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = ui.horizontal(|ui| {
        ui.set_min_height(ROW_H);
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(if mask { 30.0 } else { 4.0 });
        // The grip, on rows that can be dragged; its empty place on the others.
        let (grip, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), Sense::hover());
        if draggable {
            paint_grip(ui.painter(), grip, style::FAINT);
        }
        let solo = layer.parent.unwrap_or(layer.id);
        let (thumb, thumb_response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
        if mask {
            paint_mask_thumbnail(ui.painter(), thumb, &mask_params(cx, layer), layer.enabled);
            alt_click_solo(ui, inspector, cx, &thumb_response, solo, layer);
        } else {
            paint_thumbnail(ui.painter(), thumb, cx, card, layer);
            // A masked layer shows its first mask's picture after its own.
            if let Some(first) = masks_of(card, layer.id).next() {
                let (pic, response) = ui.allocate_exact_size(Vec2::new(12.0, 16.0), Sense::click());
                let pic = Rect::from_center_size(pic.center(), Vec2::splat(12.0));
                paint_mask_thumbnail(ui.painter(), pic, &mask_params(cx, first), first.enabled);
                alt_click_solo(ui, inspector, cx, &response, solo, layer);
            }
        }
        if thumb_response.clicked() && !ui.input(|i| i.modifiers.alt) {
            inspector.selected_layer.insert(cx.id, layer.id);
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
                response.dnd_set_drag_payload(Dragged(cx.id, layer.id));
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
                    if mask {
                        mask_menu(ui, cx, layer, edits);
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

/// Alt-click on a mask's picture shows layer `solo`'s masks alone in the ball view, or stops.
fn alt_click_solo(
    ui: &Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    response: &egui::Response,
    solo: LayerId,
    layer: &Layer,
) {
    let name = format!("Mask picture of {}", layer.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    let response = response.clone().on_hover_text("Alt-click to see the mask alone in the view above");
    if response.clicked() && ui.input(|i| i.modifiers.alt) {
        let view = &mut inspector.ball_view;
        view.solo = if view.solo == Some((cx.id, solo)) { None } else { Some((cx.id, solo)) };
    }
}

/// A mask's settings as they are at the playhead.
fn mask_params(cx: &LookCx, layer: &Layer) -> MaskLayer {
    MaskLayer::from_params(&cx.params_of(layer.id, MaskLayer::defs()))
}

/// A mask row's right-click menu: it can only go. Its values copy and paste in its settings.
fn mask_menu(ui: &mut Ui, cx: &LookCx, layer: &Layer, edits: &mut Vec<Edit>) {
    ui.set_min_width(170.0);
    if ui.button("Delete").clicked() {
        edits.push(Edit::RemoveLayer(cx.slot, layer.id));
        ui.close();
    }
}

/// The row's right-click menu. What can't apply to the row is greyed: the Body can't be
/// duplicated, moved, masked or deleted, and a reaction doesn't move.
fn menu(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, card: &Slot, layer: &Layer, edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    let kind = reg.layer(&layer.kind);
    let base = kind.is_some_and(|k| k.base());
    let drawn = kind.is_some_and(|k| k.section() == LayerSection::Drawn) && !base;
    let maskable = drawn && kind.is_some_and(|k| k.maskable());
    let group = kind.is_some_and(|k| k.section() == LayerSection::Group);
    let repeats = kind.is_some_and(|k| k.repeats());
    let index = place_of(card, layer.id).unwrap_or(0);
    let section = |l: &Layer| reg.layer(&l.kind).map(|k| k.section());
    let tops: Vec<&Layer> = top_level(card).collect();
    let top_drawn = tops.iter().filter(|l| section(l) == Some(LayerSection::Drawn)).count().saturating_sub(1);
    // A group layer moves among the group layers, at the end of the card.
    let group_start = tops.iter().position(|l| section(l) == Some(LayerSection::Group)).unwrap_or(tops.len());
    let (can_up, can_down) = if group {
        (index + 1 < tops.len(), index > group_start)
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
    let copied =
        inspector.clipboard.layer.clone().filter(|c| reg.layer(&c.kind).is_some_and(|k| k.card() == card.kind));
    let paste = ui.add_enabled(copied.is_some(), egui::Button::new("Paste layer"));
    if let Some(copied) = copied
        && paste.clicked()
    {
        edits.push(Edit::PasteLayer(slot, copied));
        ui.close();
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
    let add = ui.add_enabled(maskable, egui::Button::new("Add mask"));
    let add = add.on_hover_text("Shows the layer on part of the ball only: half of it to start with");
    if add.on_disabled_hover_text("This layer can't be masked").clicked() {
        edits.push(Edit::AddMask(slot, id, "shape"));
        ui.close();
    }
    let below = ui.add_enabled(maskable, egui::Button::new("Mask with layer below"));
    let below = below.on_hover_text("Shows the layer only where the layer under it covers the ball");
    if below.on_disabled_hover_text("This layer can't be masked").clicked() {
        edits.push(Edit::AddMask(slot, id, "layer_below"));
        ui.close();
    }
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
    if thing != cx.id {
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
    if let Some(parent) = layer.parent {
        return mask_words(cx, card, parent, &MaskLayer::from_params(&p));
    }
    let percent = |v: f32| format!("{:.0} %", v * 100.0);
    match layer.kind.as_str() {
        shape_look::BODY => shape_words(cx),
        shape_look::PAINT => {
            let l = shape_look::PaintLayer::from_params(&p);
            let stops = &l.colours.stops;
            let words = if stops.len() > 1 {
                format!("{} colours · {}", stops.len(), choice_label(l.runs))
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

/// A shape's outline in a few words: "Circle", "6 sides", "7 points, 1 curve", then "filled" when
/// it is solid.
fn shape_words(cx: &LookCx) -> String {
    let Some(thing) = cx.scene.things.get(&cx.id) else { return String::new() };
    let s = bpe_kit::ShapeSettings::from_params(&thing.params);
    let outline = match (s.start, s.moulded()) {
        (_, Some(path)) => path.words(),
        (bpe_kit::ShapeStart::Polygon, None) => format!("{} sides", s.sides),
        (bpe_kit::ShapeStart::Circle, None) => "Circle".to_owned(),
    };
    if s.fill == bpe_kit::ShapeFill::Filled { format!("{outline} · filled") } else { outline }
}

/// What a mask on `parent` shows by: "Layer below", "Top half", "Brush · 3 strokes", or a
/// warning when the layer it follows can't be followed.
fn mask_words(cx: &LookCx, card: &Slot, parent: LayerId, m: &MaskLayer) -> String {
    let words = match m.from {
        MaskFrom::LayerBelow => "Layer below".to_owned(),
        MaskFrom::Layer => {
            let below = layers_below(cx, card, parent);
            match below.iter().find(|l| i64::from(l.id.0) == i64::from(m.target) && l.enabled) {
                Some(l) => l.name.clone(),
                None => return "masked by a missing layer".to_owned(),
            }
        }
        MaskFrom::Shape if m.shape == MaskShape::Half && m.angle == 0.0 => "Top half".to_owned(),
        MaskFrom::Shape => choice_label(m.shape),
        MaskFrom::Brush => format!("Brush · {}", crate::widgets::strokes_words(&m.strokes).to_lowercase()),
        MaskFrom::Light => "Light parts".to_owned(),
        MaskFrom::Dark => "Dark parts".to_owned(),
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

/// The corners of a hexagon around `c` reaching `r`, a corner on each side.
fn hexagon(c: Pos2, r: f32) -> Vec<Pos2> {
    (0..6).map(|k| c + Vec2::angled(k as f32 * std::f32::consts::FRAC_PI_3) * r).collect()
}

/// A small black disc, white where the mask lets its layer show.
fn paint_mask_thumbnail(painter: &egui::Painter, rect: Rect, m: &MaskLayer, enabled: bool) {
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
        MaskFrom::LayerBelow | MaskFrom::Layer => {
            painter.circle_filled(c, r * 0.55, on);
        }
        MaskFrom::Shape => match m.shape {
            MaskShape::Half => {
                painter.add(pie(angle, 0.5));
            }
            MaskShape::Wedge => {
                painter.add(pie(angle, m.size.clamp(0.05, 1.0)));
            }
            MaskShape::Quarters => {
                painter.add(pie(angle - std::f32::consts::FRAC_PI_4, 0.25));
                painter.add(pie(angle + std::f32::consts::PI - std::f32::consts::FRAC_PI_4, 0.25));
            }
            MaskShape::Ring => {
                painter.circle_stroke(c, r * 0.65, Stroke::new(r * 0.35, on));
            }
            MaskShape::Circle => {
                painter.circle_filled(c, r * m.size.clamp(0.15, 1.0), on);
            }
            MaskShape::Band => {
                let d = Vec2::new(angle.cos(), angle.sin()) * r;
                painter.line_segment([c - d, c + d], Stroke::new(r * m.size.clamp(0.2, 1.0), on));
            }
        },
        MaskFrom::Brush => {
            let points =
                [c + Vec2::new(-r * 0.6, r * 0.2), c + Vec2::new(-r * 0.1, -r * 0.4), c + Vec2::new(r * 0.5, r * 0.3)];
            painter.add(egui::Shape::line(points.to_vec(), Stroke::new(r * 0.35, on)));
        }
        MaskFrom::Light => {
            painter.circle_filled(c + Vec2::new(-r * 0.3, -r * 0.3), r * 0.45, on);
        }
        MaskFrom::Dark => {
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
