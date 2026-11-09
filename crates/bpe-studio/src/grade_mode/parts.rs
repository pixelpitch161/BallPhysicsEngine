//! A mask's parts in Grade mode's right column: each part's box (fold, name, how it joins the
//! parts above it, on/off; right-click for Reset part, Move up or down and Remove part) over its
//! settings, and Add part (Add, Subtract or Intersect, then a kind). A click on a gradient's
//! name shows its handles on the preview. A Colour Range shows its picked colours with Pick and
//! Remove (the eyedropper, as Point Colour).

use super::GradeMode;
use super::cx::{GradeCx, GradeEdit, full_key};
use super::masks::part_name;
use super::panels::grid;
use crate::style;
use bpe_core::{Choice, Color, Layer, LayerId, SettingDef, Settings, Value};
use bpe_kit::modules::grade::{
    self, COLOUR_RANGE, ColourRangePart, Combine, LINEAR, LUMINANCE_RANGE, LinearPart, LuminanceRangePart, PART_KINDS,
    RADIAL, RadialPart, masks::RANGE_PICKS, masks::range_key,
};
use eframe::egui::{self, RichText, Sense, Stroke, Ui};

/// The parts heading, each part, and Add part.
pub fn section(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, mask: LayerId, edits: &mut Vec<GradeEdit>) {
    let parts = grade::parts_of(gx.card, mask);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Parts").font(style::semibold(13.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add_part(ui, mask, edits));
    });
    ui.add_space(4.0);
    if parts.is_empty() {
        ui.label(
            RichText::new("No parts: the mask covers nothing. Add part gives it one.").small().color(style::FAINT),
        );
    }
    let handles = super::handles::handle_part(gx.card, &mode.part, mask);
    for (index, layer) in parts.iter().enumerate() {
        part_box(ui, mode, gx, mask, layer, (index, parts.len()), handles == Some(layer.id), edits);
    }
}

/// Add part: Add, Subtract or Intersect, then the kind.
fn add_part(ui: &mut Ui, mask: LayerId, edits: &mut Vec<GradeEdit>) {
    ui.menu_button("Add part", |ui| {
        for combine in Combine::ALL {
            ui.menu_button(combine.word(), |ui| {
                for kind in PART_KINDS {
                    if ui.button(part_name(kind)).clicked() {
                        edits.push(GradeEdit::AddPart(mask, kind, combine));
                        ui.close();
                    }
                }
            });
        }
    })
    .response
    .on_hover_text("Another part, joined to the ones above: Add takes more in, Subtract takes it out, Intersect keeps only where both cover");
}

/// The settings of a part kind.
fn defs_of(kind: &str) -> &'static [SettingDef] {
    match kind {
        LINEAR => LinearPart::defs(),
        RADIAL => RadialPart::defs(),
        COLOUR_RANGE => ColourRangePart::defs(),
        LUMINANCE_RANGE => LuminanceRangePart::defs(),
        _ => &[],
    }
}

/// One part's box.
#[allow(clippy::too_many_arguments)]
fn part_box(
    ui: &mut Ui,
    mode: &mut GradeMode,
    gx: &GradeCx,
    mask: LayerId,
    layer: &Layer,
    (index, count): (usize, usize),
    handles: bool,
    edits: &mut Vec<GradeEdit>,
) {
    let id = layer.id;
    let open = !mode.folded.contains(&id);
    let fill = if handles { style::SELECTED } else { style::RAISED };
    let frame = egui::Frame::new().fill(fill).corner_radius(style::RADIUS).inner_margin(egui::Margin::symmetric(8, 6));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let name = ui
            .horizontal(|ui| {
                if crate::widgets::fold_toggle(ui, open, &layer.name) {
                    if open {
                        mode.folded.insert(id);
                    } else {
                        mode.folded.remove(&id);
                    }
                }
                let ink = if layer.enabled { style::TEXT } else { style::FAINT };
                let name = ui.add(
                    egui::Button::new(RichText::new(&layer.name).font(style::semibold(12.5)).color(ink)).frame(false),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = layer.enabled;
                    let switch = style::switch(ui, &mut on);
                    let words = format!("{} on", layer.name);
                    switch.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, words.clone()));
                    if switch.on_hover_text("Off, the part is left out (its settings are kept)").changed() {
                        edits.push(GradeEdit::Enable(id, on));
                    }
                    if index > 0 {
                        combine_menu(ui, gx, id, edits);
                    }
                });
                name
            })
            .inner;
        let name = name.on_hover_text(if layer.kind == LINEAR || layer.kind == RADIAL {
            "Click to show its handles on the picture. Right-click for more."
        } else {
            "Right-click for more."
        });
        if name.clicked() {
            mode.part.insert(mask, id);
        }
        name.context_menu(|ui| menu(ui, id, index, count, edits));
        if open {
            ui.add_space(4.0);
            body(ui, mode, gx, layer, edits);
        }
    });
    ui.add_space(4.0);
}

/// How a part joins the ones above it: a small menu of Add, Subtract and Intersect.
fn combine_menu(ui: &mut Ui, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let now = match gx.get(Some(id), "combine") {
        Value::Choice(n) => Combine::from_name(&n).unwrap_or_default(),
        _ => Combine::Add,
    };
    ui.menu_button(RichText::new(now.word()).small(), |ui| {
        for c in Combine::ALL {
            if ui.add(style::toggle_button(c.word(), c == now)).clicked() {
                if c != now {
                    edits.push(GradeEdit::Set(full_key(Some(id), "combine"), Value::Choice(c.name().into())));
                }
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("How this part joins the parts above it");
}

/// The right-click menu on a part's name.
fn menu(ui: &mut Ui, id: LayerId, index: usize, count: usize, edits: &mut Vec<GradeEdit>) {
    if ui.button("Reset part").on_hover_text("Every setting of this part back to where it starts").clicked() {
        edits.push(GradeEdit::ResetPanel(id));
        ui.close();
    }
    if ui.add_enabled(index > 0, egui::Button::new("Move up")).clicked() {
        edits.push(GradeEdit::MovePart(id, index - 1));
        ui.close();
    }
    if ui.add_enabled(index + 1 < count, egui::Button::new("Move down")).clicked() {
        edits.push(GradeEdit::MovePart(id, index + 1));
        ui.close();
    }
    ui.separator();
    if ui.button("Remove part").clicked() {
        edits.push(GradeEdit::RemovePart(id));
        ui.close();
    }
}

/// A part's settings (How it joins is in its header).
fn body(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, layer: &Layer, edits: &mut Vec<GradeEdit>) {
    let id = layer.id;
    if layer.kind == COLOUR_RANGE {
        picks(ui, mode, gx, id, edits);
    }
    let hidden = |d: &&SettingDef| {
        d.key == "combine" || (layer.kind == COLOUR_RANGE && (d.key.starts_with("used") || d.key.starts_with("colour")))
    };
    let defs: Vec<SettingDef> = defs_of(&layer.kind).iter().filter(|d| !hidden(d)).cloned().collect();
    grid(ui, gx, id, &defs, None, edits);
}

/// The id the eyedropper hands a Colour Range's pick to.
pub fn range_target(id: LayerId) -> egui::Id {
    egui::Id::unique(("grade colour range pick", id.0))
}

/// A Colour Range's picked colours, Pick and Remove.
fn picks(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let used: Vec<bool> =
        (0..RANGE_PICKS).map(|s| matches!(gx.get(Some(id), &range_key(s, "used")), Value::Bool(true))).collect();
    let colour = |s: usize| match gx.get(Some(id), &range_key(s, "colour")) {
        Value::Color(c) => c,
        _ => Color::rgb(255, 0, 0),
    };
    let free = used.iter().position(|u| !u);
    let target = range_target(id);
    let ctx = ui.ctx().clone();
    crate::eyedropper::seen(&ctx, target);
    if free.is_none() && crate::eyedropper::target(&ctx) == Some(target) {
        crate::eyedropper::stop(&ctx);
    }
    if let Some(slot) = free
        && let Some(c) = crate::eyedropper::take(&ctx, target)
    {
        edits.push(GradeEdit::Set(full_key(Some(id), &range_key(slot, "used")), Value::Bool(true)));
        edits.push(GradeEdit::Set(full_key(Some(id), &range_key(slot, "colour")), Value::Color(Color { a: 255, ..c })));
        mode.pick.insert(id, slot);
    }
    let selected = mode.pick.get(&id).copied().filter(|s| used.get(*s) == Some(&true));
    let selected = selected.or_else(|| used.iter().position(|u| *u));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for slot in (0..RANGE_PICKS).filter(|s| used[*s]) {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            let words = format!("Range colour {}", slot + 1);
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected == Some(slot), &words));
            ui.painter().rect_filled(rect.shrink(2.0), style::RADIUS, style::video_bytes(colour(slot)));
            let ring = if selected == Some(slot) {
                Stroke::new(2.0, style::TEXT_STRONG)
            } else {
                Stroke::new(1.0, style::LINE)
            };
            ui.painter().rect_stroke(rect, style::RADIUS, ring, egui::StrokeKind::Inside);
            if response.on_hover_text(format!("{words}: select it")).clicked() {
                mode.pick.insert(id, slot);
            }
        }
        let picking = crate::eyedropper::target(&ctx) == Some(target);
        let help = if picking {
            "Click the picture to pick the colour there, or click here to stop"
        } else {
            "Pick a colour from the picture: the colours near it are covered"
        };
        let pick = ui
            .add_enabled(free.is_some() || picking, style::toggle_button("Pick", picking))
            .on_hover_text(help)
            .on_disabled_hover_text("Five colours are picked: remove one first");
        if pick.clicked() {
            if picking {
                crate::eyedropper::stop(&ctx);
            } else {
                crate::eyedropper::start(&ctx, target);
            }
        }
        let remove = ui
            .add_enabled(selected.is_some(), egui::Button::new("Remove"))
            .on_hover_text("Give up the selected colour");
        if remove.clicked()
            && let Some(slot) = selected
        {
            edits.push(GradeEdit::RemoveRangePick(id, slot));
            mode.pick.remove(&id);
        }
    });
    if !used.iter().any(|u| *u) {
        ui.label(
            RichText::new("Click Pick, then a colour in the picture: the colours near it are covered.")
                .small()
                .color(style::FAINT),
        );
    }
    ui.add_space(4.0);
}
