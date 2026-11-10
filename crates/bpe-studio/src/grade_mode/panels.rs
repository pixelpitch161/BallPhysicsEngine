//! Grade mode's panel column: the selected mask (the Whole picture, or a mask with its Amount and
//! parts, `parts.rs`), then its panels in the order they work, each with a header (fold, name,
//! on/off, dragged to a new place; right-click for Reset panel, Move up or down, Remove and Reset
//! order) over its settings, and Add panel for another (a copy is "Tone Curve 2"). The colour
//! panels' bodies are in `colour.rs`.

use super::cx::{GradeCx, GradeEdit, card_or_start, full_key};
use super::{Channel, GradeMode};
use crate::app::Studio;
use crate::style;
use crate::widgets::{GridCx, KeyMark, TargetChoices, settings_grid};
use bpe_core::{Layer, LayerId, SettingDef, Settings, ThingId, Value};
use bpe_kit::modules::grade::{
    self, BASIC, CALIBRATION, COLOUR_GRADING, CurveMode, DETAIL, EFFECTS, HSL, PANEL_ORDER, POINT_COLOUR, TONE_CURVE,
};
use eframe::egui::{self, RichText, Sense, Ui};

/// The column, with its edits applied.
pub fn column(ui: &mut Ui, studio: &mut Studio) {
    // An Alt view of clipping lasts while Alt and the button are held (`basic`).
    if !ui.input(|i| i.modifiers.alt && i.pointer.primary_down()) {
        studio.grade.alt_clip = bpe_gfx::grade::ClipView::Off;
    }
    let start = studio.grade.start(&studio.kit.registry);
    let time = studio.sim_time();
    let mut edits = Vec::new();
    {
        let Studio { scene, kit, grade: mode, .. } = &mut *studio;
        let card = card_or_start(scene, &start);
        let gx = GradeCx { scene, kit, card, time };
        let Some(mask) = mode.mask_of(card) else { return };
        let Some(mask_layer) = card.layer(mask) else { return };
        let panels: Vec<&Layer> = grade::panels_of(card, mask);
        title_row(ui, &gx, mask_layer, &mut edits);
        ui.add_space(4.0);
        if mask_layer.kind == grade::MASK {
            grid(ui, &gx, mask, grade::MaskSettings::defs(), None, &mut edits);
            ui.add_space(6.0);
            super::parts::section(ui, mode, &gx, mask, &mut edits);
            ui.add_space(6.0);
            ui.label(RichText::new("Panels").font(style::semibold(13.0)));
            ui.add_space(4.0);
        }
        let can_remove = panels.len() > 1;
        let mut headers = Vec::new();
        for (index, layer) in panels.iter().enumerate() {
            let place = Place { mask, index, count: panels.len(), can_remove };
            let header = panel(ui, mode, &gx, layer, place, &mut edits);
            headers.push((layer.id, header));
        }
        drop_line(ui, mode, &headers);
        mode.headers = headers;
    }
    super::apply(studio, edits);
}

/// The mask's name ("Whole picture") and Add panel.
fn title_row(ui: &mut Ui, gx: &GradeCx, mask: &Layer, edits: &mut Vec<GradeEdit>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&mask.name).font(style::semibold(14.0)));
        if !gx.on() {
            ui.label(RichText::new("(grade off)").small().color(style::MUTED));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("Add panel", |ui| {
                for kind in PANEL_ORDER {
                    let name = gx.kit.registry.layer(kind).map_or(kind, |k| k.name());
                    if ui.button(name).clicked() {
                        edits.push(GradeEdit::AddPanel(mask.id, kind));
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Another panel at the end, after the others (a second one works on what the first made)");
        });
    });
}

/// Where a panel is among its mask's panels.
#[derive(Clone, Copy)]
struct Place {
    mask: LayerId,
    index: usize,
    count: usize,
    can_remove: bool,
}

/// One panel: its header and, open, its settings. Returns the header's rectangle.
fn panel(
    ui: &mut Ui,
    mode: &mut GradeMode,
    gx: &GradeCx,
    layer: &Layer,
    place: Place,
    edits: &mut Vec<GradeEdit>,
) -> egui::Rect {
    let id = layer.id;
    let open = !mode.folded.contains(&id);
    let frame =
        egui::Frame::new().fill(style::RAISED).corner_radius(style::RADIUS).inner_margin(egui::Margin::symmetric(8, 6));
    let mut header_rect = egui::Rect::NOTHING;
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let header = ui.horizontal(|ui| {
            if crate::widgets::fold_toggle(ui, open, &layer.name) {
                if open {
                    mode.folded.insert(id);
                } else {
                    mode.folded.remove(&id);
                }
            }
            let ink = if layer.enabled { style::TEXT } else { style::FAINT };
            let name = ui.add(
                egui::Button::new(RichText::new(&layer.name).font(style::semibold(13.0)).color(ink))
                    .frame(false)
                    .sense(Sense::click_and_drag()),
            );
            if gx.changed(id) {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                style::changed_dot(ui.painter(), dot.center());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut on = layer.enabled;
                let switch = style::switch(ui, &mut on);
                let words = format!("{} on", layer.name);
                switch.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, words.clone()));
                let switch = switch.on_hover_text(if layer.enabled {
                    "The panel is on. Off, it does nothing and keeps its settings."
                } else {
                    "The panel is off. Turn it on to use it again."
                });
                if switch.changed() {
                    edits.push(GradeEdit::Enable(id, on));
                }
                grip(ui);
            });
            name
        });
        let name = header.inner;
        header_rect = header.response.rect;
        let name = name.on_hover_text("Drag to change when this panel works. Right-click for more.");
        if name.drag_started() {
            mode.dragging = Some(id);
        }
        if name.dragged() && mode.dragging == Some(id) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if name.drag_stopped() && mode.dragging == Some(id) {
            mode.dragging = None;
            if let Some(to) = ui.ctx().pointer_latest_pos().map(|p| drop_index(&mode.headers, id, p.y))
                && to != place.index
            {
                edits.push(GradeEdit::MovePanel(id, to));
            }
        }
        name.context_menu(|ui| menu(ui, id, place, edits));
        if open {
            ui.add_space(4.0);
            match layer.kind.as_str() {
                BASIC => basic(ui, mode, gx, id, edits),
                TONE_CURVE => tone_curve(ui, mode, gx, id, edits),
                HSL => super::colour::hsl(ui, mode, gx, id, edits),
                POINT_COLOUR => super::colour::point_colour(ui, mode, gx, id, edits),
                COLOUR_GRADING => super::colour::colour_grading(ui, mode, gx, id, edits),
                DETAIL => grid(ui, gx, id, grade::DetailPanel::defs(), None, edits),
                EFFECTS => grid(ui, gx, id, grade::EffectsPanel::defs(), None, edits),
                CALIBRATION => super::colour::calibration(ui, gx, id, edits),
                _ => {}
            }
        }
    });
    ui.add_space(6.0);
    header_rect
}

/// Six dots: the header can be dragged.
fn grip(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
    for row in 0..3 {
        for col in 0..2 {
            let at = egui::pos2(rect.left() + 2.5 + col as f32 * 5.0, rect.top() + 2.5 + row as f32 * 4.5);
            ui.painter().circle_filled(at, 1.1, style::FAINT);
        }
    }
}

/// The right-click menu on a panel's header.
fn menu(ui: &mut Ui, id: LayerId, place: Place, edits: &mut Vec<GradeEdit>) {
    if ui.button("Reset panel").on_hover_text("Every setting of this panel back to where it starts").clicked() {
        edits.push(GradeEdit::ResetPanel(id));
        ui.close();
    }
    if ui.add_enabled(place.index > 0, egui::Button::new("Move up")).clicked() {
        edits.push(GradeEdit::MovePanel(id, place.index - 1));
        ui.close();
    }
    if ui.add_enabled(place.index + 1 < place.count, egui::Button::new("Move down")).clicked() {
        edits.push(GradeEdit::MovePanel(id, place.index + 1));
        ui.close();
    }
    let remove = ui
        .add_enabled(place.can_remove, egui::Button::new("Remove panel"))
        .on_disabled_hover_text("A mask keeps at least one panel");
    if remove.clicked() {
        edits.push(GradeEdit::RemovePanel(id));
        ui.close();
    }
    ui.separator();
    if ui.button("Reset order").on_hover_text("The panels back in Lightroom's order").clicked() {
        edits.push(GradeEdit::ResetOrder(place.mask));
        ui.close();
    }
}

/// The place among `headers` a header dropped at screen height `y` goes to.
pub(super) fn drop_index(headers: &[(LayerId, egui::Rect)], id: LayerId, y: f32) -> usize {
    let others: Vec<&egui::Rect> = headers.iter().filter(|(h, _)| *h != id).map(|(_, r)| r).collect();
    others.iter().take_while(|r| r.center().y < y).count()
}

/// While a header is dragged, a line where it would land.
fn drop_line(ui: &mut Ui, mode: &GradeMode, headers: &[(LayerId, egui::Rect)]) {
    let Some(id) = mode.dragging else { return };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else { return };
    let to = drop_index(headers, id, pointer.y);
    let others: Vec<&egui::Rect> = headers.iter().filter(|(h, _)| *h != id).map(|(_, r)| r).collect();
    let y = match others.get(to) {
        Some(r) => r.top() - 9.0,
        None => others.last().map_or(pointer.y, |r| r.bottom() + 4.0).max(ui.min_rect().bottom() - 3.0),
    };
    let x = ui.min_rect().x_range();
    ui.painter().hline(x, y, egui::Stroke::new(2.0, style::TEXT));
}

/// A panel's settings grid over `defs`, its changes gathered as edits. `rails` gives a colour
/// strip under the sliders that show a colour range.
pub(super) fn grid(
    ui: &mut Ui,
    gx: &GradeCx,
    id: LayerId,
    defs: &[SettingDef],
    rails: Option<&crate::widgets::RailsOf>,
    edits: &mut Vec<GradeEdit>,
) {
    let (scene, reg) = (gx.scene, &gx.kit.registry);
    let get = |key: &str| gx.get(Some(id), key);
    let marks = |key: &str| {
        let def = crate::widgets::def_in(defs, key)?;
        let a = super::cx::at(&full_key(Some(id), key));
        Some(KeyMark { can_key: def.flags.keyframe, keyed: scene.keyed(&a), pulsed: scene.pulses.contains_key(&a) })
    };
    let applies = |d: &SettingDef| d.applies(Some("scene"), &|k| Some(gx.get(Some(id), k)));
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &super::cx::at(&full_key(Some(id), key)));
    let follow_light = |key: &str| {
        // The card may not be there yet: a setting that pulses can follow light.
        gx.def(Some(id), key).is_some_and(|d| d.flags.pulse)
            && (scene.card(ThingId::ROOT, bpe_core::SlotId::Grade).is_none()
                || crate::follow::can_light(scene, reg, &super::cx::at(&full_key(Some(id), key))))
    };
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now, light: &follow_light };
    let choices = TargetChoices::from_scene(scene, reg);
    let label_width = crate::widgets::label_width(ui.available_width());
    let cx = GridCx {
        choices: &choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width,
        track: style::colour_of(scene, ThingId::ROOT),
        follow: Some(&follow),
        vary: None,
        offers: None,
        rails,
    };
    let out = settings_grid(ui, ("grade panel", id.0), defs, get, &cx);
    for (key, value) in out.changes {
        edits.push(GradeEdit::Set(full_key(Some(id), key), value));
    }
    edits.extend(out.new_keys.into_iter().map(|key| GradeEdit::NewKey(full_key(Some(id), key))));
    edits.extend(out.cleared.into_iter().map(|key| GradeEdit::ClearKeys(full_key(Some(id), key))));
    edits.extend(out.follows.into_iter().map(|(key, source)| GradeEdit::Follow(full_key(Some(id), key), source)));
}

fn basic(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let before = edits.len();
    grid(ui, gx, id, grade::BasicPanel::defs(), Some(&super::colour::basic_rails), edits);
    // Lightroom's Alt-drag: dragging a tone slider with Alt held shows what clips.
    if ui.input(|i| i.modifiers.alt && i.pointer.primary_down()) {
        for edit in &edits[before..] {
            if let GradeEdit::Set(key, _) = edit
                && let Some(view) = key.strip_prefix(&format!("{id}/")).and_then(alt_view)
            {
                mode.alt_clip = view;
            }
        }
    }
}

/// The clipping view Alt shows while Basic's setting `key` is dragged, if any: Exposure,
/// Highlights and Whites show what reaches white, Shadows and Blacks what reaches black.
pub fn alt_view(key: &str) -> Option<bpe_gfx::grade::ClipView> {
    use bpe_gfx::grade::ClipView;
    match key {
        "exposure" | "highlights" | "whites" => Some(ClipView::Whites),
        "shadows" | "blacks" => Some(ClipView::Blacks),
        _ => None,
    }
}

fn tone_curve(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let curve_mode = match gx.get(Some(id), "mode") {
        Value::Choice(name) => <CurveMode as bpe_core::Choice>::from_name(&name).unwrap_or_default(),
        _ => CurveMode::default(),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (m, name, help) in [
            (CurveMode::Parametric, "Parametric", "Four sliders for the regions of brightness"),
            (CurveMode::Point, "Point", "Points on the curve, for all channels or one"),
        ] {
            if ui.add(style::toggle_button(name, curve_mode == m)).on_hover_text(help).clicked() && curve_mode != m {
                edits.push(GradeEdit::Set(full_key(Some(id), "mode"), Value::Choice(bpe_core::Choice::name(m).into())));
            }
        }
        ui.add_space(6.0);
        super::target::toggle(ui, mode, id);
        if curve_mode == CurveMode::Point {
            ui.add_space(6.0);
            let now = mode.channel.get(&id).copied().unwrap_or_default();
            for c in Channel::ALL {
                let chip = style::toggle_button(c.name(), now == c).small();
                if ui.add(chip).on_hover_text(format!("Show the {} curve", c.name())).clicked() {
                    mode.channel.insert(id, c);
                }
            }
        }
    });
    ui.add_space(4.0);
    super::curve::show(ui, mode, gx, id, curve_mode, edits);
    ui.add_space(4.0);
    let defs: Vec<SettingDef> = grade::ToneCurvePanel::defs()
        .iter()
        .filter(|d| d.key != "mode" && !grade::CURVE_KEYS.contains(&d.key))
        .cloned()
        .collect();
    grid(ui, gx, id, &defs, None, edits);
    if curve_mode == CurveMode::Point {
        ui.label(
            RichText::new("Click the curve to add a point, drag to move it, double-click a point to take it away.")
                .small()
                .color(style::FAINT),
        );
    }
}
