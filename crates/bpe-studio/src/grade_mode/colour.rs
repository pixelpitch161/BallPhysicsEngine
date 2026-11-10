//! The colour panels' bodies in Grade mode: HSL / Colour (Lightroom's Hue / Saturation / Luminance
//! / All tabs), Point Colour (picked swatches, Pick from the picture, the selected pick's
//! sliders), Colour Grading (3-way or one region's wheel, then Blending and Balance) and
//! Calibration; and the colour strips under the sliders that move a colour (the video's colours,
//! which the design system allows).

use super::cx::{GradeCx, GradeEdit, full_key};
use super::panels::grid;
use super::{GradeMode, HslView, WheelView, wheel};
use crate::style;
use bpe_core::{Color, LayerId, SettingDef, Settings, Value};
use bpe_gfx::grade::maths;
use bpe_kit::modules::grade::{
    CalibrationPanel, ColourGradingPanel, HslPanel, PICKS, PointColourPanel, colour_grading::REGIONS, hsl::BANDS,
    pick_prefix,
};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Ui};

/// The wheels' names, in [`REGIONS`]' order.
const REGION_NAMES: [&str; 4] = ["Shadows", "Midtones", "Highlights", "Global"];

/// The colour at Oklab lightness `l`, chroma `c` and hue `h` (radians), as the screen shows it.
fn lch(l: f64, c: f64, h: f64) -> Color32 {
    style::video(maths::unoklab(maths::from_lch([l, c, h])))
}

/// A grey of `v` (linear) tinted `amount` (0..1) towards the colour at HSV hue `deg`.
fn tinted(deg: f64, amount: f64, v: f64) -> Color32 {
    let c = maths::hsv_colour(deg);
    style::video([0, 1, 2].map(|i| v + (c[i] * v * 2.0 - v) * amount))
}

/// Strips that run round a hue: Hue rows turn it by `turn` (radians) each way, Saturation rows
/// go from grey to colourful, Luminance rows from dark to light.
fn hue_rail(part: &str, h: f64, turn: f64) -> Option<Vec<Color32>> {
    match part {
        "hue" => Some((0..=6).map(|i| lch(0.72, 0.13, h + turn * (i as f64 / 3.0 - 1.0))).collect()),
        "saturation" => Some(vec![lch(0.72, 0.0, h), lch(0.72, 0.13, h), lch(0.72, 0.26, h)]),
        "luminance" => Some(vec![lch(0.35, 0.1, h), lch(0.65, 0.12, h), lch(0.92, 0.08, h)]),
        _ => None,
    }
}

/// Basic's Temp (blue to yellow) and Tint (green to magenta).
pub fn basic_rails(key: &str) -> Option<Vec<Color32>> {
    match key {
        "temp" => Some(vec![tinted(215.0, 0.6, 0.35), tinted(0.0, 0.0, 0.35), tinted(52.0, 0.6, 0.35)]),
        "tint" => Some(vec![tinted(120.0, 0.6, 0.35), tinted(0.0, 0.0, 0.35), tinted(300.0, 0.6, 0.35)]),
        _ => None,
    }
}

/// HSL / Colour's rows: each band's own colours.
pub fn hsl_rails(key: &str) -> Option<Vec<Color32>> {
    let (band, part) = key.split_once('_')?;
    let i = BANDS.iter().position(|b| *b == band)?;
    let h = maths::band_hues()[i];
    let turn = maths::band_turn(i, 100.0).max(-maths::band_turn(i, -100.0));
    hue_rail(part, h, turn)
}

/// Calibration's rows: the shadows green to magenta, each primary's own colours.
pub fn calibration_rails(key: &str) -> Option<Vec<Color32>> {
    if key == "shadows_tint" {
        return Some(vec![tinted(120.0, 0.6, 0.2), tinted(0.0, 0.0, 0.2), tinted(300.0, 0.6, 0.2)]);
    }
    let (primary, part) = key.split_once('_')?;
    let i = ["red", "green", "blue"].iter().position(|p| *p == primary)?;
    let mut e = [0.0; 3];
    e[i] = 1.0;
    let h = maths::lch(maths::oklab(e))[2];
    hue_rail(part, h, 30f64.to_radians())
}

/// A row of chips, one on; returns the one clicked.
fn chips<T: Copy + PartialEq>(ui: &mut Ui, now: T, all: &[(T, &str, &str)]) -> Option<T> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (v, name, help) in all {
            if ui.add(style::toggle_button(name, now == *v).small()).on_hover_text(*help).clicked() {
                picked = Some(*v);
            }
        }
    });
    ui.add_space(4.0);
    picked
}

/// `defs` with their group heading taken off (the chips already say what they are).
fn ungrouped(defs: impl Iterator<Item = SettingDef>) -> Vec<SettingDef> {
    defs.map(|d| SettingDef { group: "", ..d }).collect()
}

/// HSL / Colour: the tab chips, then the tab's eight sliders (or all 24).
pub fn hsl(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let now = mode.hsl_view.get(&id).copied().unwrap_or_default();
    let all: Vec<(HslView, &str, &str)> = HslView::ALL
        .iter()
        .map(|v| {
            let help = match v {
                HslView::Hue => "Turn each colour band's hue",
                HslView::Saturation => "Each colour band more or less colourful",
                HslView::Luminance => "Each colour band brighter or darker",
                HslView::All => "Every band's hue, saturation and luminance",
            };
            (*v, v.name(), help)
        })
        .collect();
    let picked = ui
        .horizontal(|ui| {
            super::target::toggle(ui, mode, id);
            ui.add_space(4.0);
            chips(ui, now, &all)
        })
        .inner;
    if let Some(v) = picked {
        mode.hsl_view.insert(id, v);
    }
    let view = mode.hsl_view.get(&id).copied().unwrap_or_default();
    let defs: Vec<SettingDef> = match view {
        HslView::All => HslPanel::defs().to_vec(),
        one => ungrouped(HslPanel::defs().iter().filter(|d| d.group == one.name()).cloned()),
    };
    grid(ui, gx, id, &defs, Some(&hsl_rails), edits);
}

/// Pick slot `slot`'s setting `key`.
fn pick_key(slot: usize, key: &str) -> String {
    format!("{}{key}", pick_prefix(slot))
}

/// The id the eyedropper hands a Point Colour panel's pick to.
pub fn pick_target(id: LayerId) -> egui::Id {
    egui::Id::unique(("grade point colour pick", id.0))
}

/// Point Colour: the picked swatches with Pick and Remove, then the selected pick's sliders.
pub fn point_colour(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let used: Vec<bool> =
        (0..PICKS).map(|s| matches!(gx.get(Some(id), &pick_key(s, "on")), Value::Bool(true))).collect();
    let colour = |s: usize| match gx.get(Some(id), &pick_key(s, "colour")) {
        Value::Color(c) => c,
        _ => Color::rgb(255, 0, 0),
    };
    let free = used.iter().position(|u| !u);
    let target = pick_target(id);
    let ctx = ui.ctx().clone();
    crate::eyedropper::seen(&ctx, target);
    // A full panel takes no more colours (Pick is off then too).
    if free.is_none() && crate::eyedropper::target(&ctx) == Some(target) {
        crate::eyedropper::stop(&ctx);
    }
    if let Some(slot) = free
        && let Some(c) = crate::eyedropper::take(&ctx, target)
    {
        edits.push(GradeEdit::Set(full_key(Some(id), &pick_key(slot, "on")), Value::Bool(true)));
        edits.push(GradeEdit::Set(full_key(Some(id), &pick_key(slot, "colour")), Value::Color(Color { a: 255, ..c })));
        mode.pick.insert(id, slot);
    }
    let selected = mode.pick.get(&id).copied().filter(|s| used.get(*s) == Some(&true));
    let selected = selected.or_else(|| used.iter().position(|u| *u));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for slot in (0..PICKS).filter(|s| used[*s]) {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            let words = format!("Colour {}", slot + 1);
            response
                .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected == Some(slot), &words));
            ui.painter().rect_filled(rect.shrink(2.0), style::RADIUS, style::video_bytes(colour(slot)));
            let ring = if selected == Some(slot) {
                Stroke::new(2.0, style::TEXT_STRONG)
            } else {
                Stroke::new(1.0, style::LINE)
            };
            ui.painter().rect_stroke(rect, style::RADIUS, ring, egui::StrokeKind::Inside);
            if response.on_hover_text(format!("{words}: show its sliders")).clicked() {
                mode.pick.insert(id, slot);
            }
        }
        let picking = crate::eyedropper::target(&ctx) == Some(target);
        let help = if picking {
            "Click the picture to pick the colour there, or click here to stop"
        } else {
            "Pick a colour from the picture: the colours near it can then be moved"
        };
        let pick = ui
            .add_enabled(free.is_some() || picking, style::toggle_button("Pick", picking))
            .on_hover_text(help)
            .on_disabled_hover_text("Eight colours are picked: remove one first");
        if pick.clicked() {
            if picking {
                crate::eyedropper::stop(&ctx);
            } else {
                crate::eyedropper::start(&ctx, target);
            }
        }
        let remove = ui
            .add_enabled(selected.is_some(), egui::Button::new("Remove"))
            .on_hover_text("Give up the selected colour and its settings");
        if remove.clicked()
            && let Some(slot) = selected
        {
            edits.push(GradeEdit::RemovePick(id, slot));
            mode.pick.remove(&id);
        }
    });
    ui.add_space(4.0);
    let Some(slot) = selected else {
        ui.label(
            RichText::new(
                "Click Pick, then a colour in the picture. Picking with Before (\\) shown takes the colour as it was.",
            )
            .small()
            .color(style::FAINT),
        );
        return;
    };
    let prefix = pick_prefix(slot);
    let defs: Vec<SettingDef> = PointColourPanel::defs()
        .iter()
        .filter(|d| d.key.starts_with(&prefix) && !d.key.ends_with("_on"))
        .cloned()
        .collect();
    let picked = maths::lch(maths::oklab(
        [colour(slot).r, colour(slot).g, colour(slot).b].map(|v| maths::untone(v as f64 / 255.0)),
    ));
    let rails = |key: &str| {
        let part = key.strip_prefix(prefix.as_str())?;
        hue_rail(part, picked[2], maths::PICK_TURN)
    };
    grid(ui, gx, id, &defs, Some(&rails), edits);
}

/// One region's wheel `size` across with its name and numbers under it.
fn region_wheel(ui: &mut Ui, gx: &GradeCx, id: LayerId, region: usize, size: f32, edits: &mut Vec<GradeEdit>) {
    let key = REGIONS[region];
    let num = |k: &str| match gx.get(Some(id), &format!("{key}_{k}")) {
        Value::Number(x) => x as f32,
        _ => 0.0,
    };
    let (hue, sat) = (num("hue"), num("saturation"));
    ui.vertical_centered(|ui| {
        if let Some((h, s)) = wheel::wheel(ui, REGION_NAMES[region], size, hue, sat) {
            if h != hue {
                edits.push(GradeEdit::Set(full_key(Some(id), &format!("{key}_hue")), Value::Number(h as f64)));
            }
            if s != sat {
                edits.push(GradeEdit::Set(full_key(Some(id), &format!("{key}_saturation")), Value::Number(s as f64)));
            }
        }
        ui.label(RichText::new(REGION_NAMES[region]).small().color(style::MUTED));
        ui.label(RichText::new(format!("{hue:.0}° · {sat:.0}")).font(style::mono(11.0)).color(style::TEXT));
    });
}

/// Colour Grading: the view chips, the wheels, then the luminance, Blending and Balance sliders.
pub fn colour_grading(ui: &mut Ui, mode: &mut GradeMode, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    let now = mode.wheel_view.get(&id).copied().unwrap_or_default();
    let mut all = vec![(WheelView::ThreeWay, "3-way", "The shadows, midtones and highlights wheels side by side")];
    for (r, name) in REGION_NAMES.iter().enumerate() {
        all.push((WheelView::One(r), name, "One wheel, big, with its sliders"));
    }
    if let Some(v) = chips(ui, now, &all) {
        mode.wheel_view.insert(id, v);
    }
    let view = mode.wheel_view.get(&id).copied().unwrap_or_default();
    let defs = ColourGradingPanel::defs();
    let def = |key: &str| defs.iter().find(|d| d.key == key).cloned().expect("a Colour Grading setting");
    let mut shown: Vec<SettingDef> = Vec::new();
    match view {
        WheelView::ThreeWay => {
            let size = ((ui.available_width() - 12.0) / 3.0).clamp(60.0, 120.0);
            ui.columns(3, |cols| {
                for (r, col) in cols.iter_mut().enumerate() {
                    region_wheel(col, gx, id, r, size, edits);
                }
            });
            for r in 0..3 {
                let d = def(&format!("{}_luminance", REGIONS[r]));
                shown.push(SettingDef { label: REGION_NAMES[r], group: "Luminance", ..d });
            }
        }
        WheelView::One(r) => {
            let size = ui.available_width().clamp(100.0, 200.0);
            region_wheel(ui, gx, id, r, size, edits);
            for part in ["hue", "saturation", "luminance"] {
                shown.push(SettingDef { group: "", ..def(&format!("{}_{part}", REGIONS[r])) });
            }
        }
    }
    ui.add_space(4.0);
    shown.push(def("blending"));
    shown.push(def("balance"));
    let rails = |key: &str| {
        let (region, part) = key.split_once('_')?;
        let hue = match gx.get(Some(id), &format!("{region}_hue")) {
            Value::Number(x) => x,
            _ => 0.0,
        };
        match part {
            "hue" => Some((0..=12).map(|i| tinted(i as f64 * 30.0, 0.7, 0.4)).collect()),
            "saturation" => Some(vec![tinted(hue, 0.0, 0.4), tinted(hue, 0.85, 0.4)]),
            "luminance" => Some(vec![tinted(0.0, 0.0, 0.03), tinted(0.0, 0.0, 0.85)]),
            _ => None,
        }
    };
    grid(ui, gx, id, &shown, Some(&rails), edits);
}

/// Calibration: its sliders, coloured.
pub fn calibration(ui: &mut Ui, gx: &GradeCx, id: LayerId, edits: &mut Vec<GradeEdit>) {
    grid(ui, gx, id, CalibrationPanel::defs(), Some(&calibration_rails), edits);
}
