//! The targeted adjustment tool (Lightroom's): a Target switch on a Tone Curve or HSL / Colour
//! panel, then a drag up or down on the picture moves what the colour under the pointer is in.
//!
//! - Tone Curve, Parametric: the region slider (Shadows, Darks, Lights, Highlights) the pixel's
//!   brightness falls in, by the panel's splits.
//! - Tone Curve, Point: the shown channel's curve point nearest the pixel's level (within
//!   [`NEAR`]), or a new one there, moved up or down.
//! - HSL / Colour: the shown tab's sliders (Hue, Saturation or Luminance; All moves Saturation)
//!   of the bands the pixel's hue is in, each by its share of the hue.
//!
//! The colour is read once at the press, from the picture as shown (as the eyedropper reads it);
//! the drag moves values from where they were at the press. Each drag is one undo step.

use super::cx::{GradeCx, GradeEdit, card_or_start, full_key};
use super::{Channel, GradeMode, HslView};
use crate::app::Studio;
use crate::style;
use bpe_core::{CurvePoint, LayerId, MAX_CURVE_POINTS, Value};
use bpe_gfx::grade::maths;
use bpe_kit::modules::grade::{CurveMode, HSL, TONE_CURVE, hsl::BANDS};
use eframe::egui::{self, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};

/// How near (0..1) a curve point must be to the pixel's level to be the one moved.
pub const NEAR: f64 = 0.04;
/// How far (0..1) a new curve point keeps from the points either side.
pub const ROOM: f64 = 0.01;
/// How much a slider moves a screen point dragged.
pub const SLIDER_RATE: f64 = 0.5;
/// How much a curve point moves (0..1) a screen point dragged.
pub const CURVE_RATE: f64 = 1.0 / 300.0;

/// What a drag moves, as it was at the press.
#[derive(Clone, Debug, PartialEq)]
pub enum Aim {
    /// Sliders (full keys), each with its value at the press and its share of the move.
    Sliders(Vec<(String, f64, f64)>),
    /// A curve (its full key) as it was at the press, with point `index` the one moved (perhaps
    /// just added).
    Point { key: String, points: Vec<CurvePoint>, index: usize },
}

/// A drag of the tool on the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub panel: LayerId,
    /// The press's height on the screen, and the last height a change was asked for.
    pub press: f32,
    pub last: f32,
    pub aim: Aim,
}

/// An 8-bit sRGB-encoded byte as 0..1.
fn unit(b: u8) -> f64 {
    f64::from(b) / 255.0
}

/// The brightness of an encoded pixel, 0..1 (Rec. 709 weights on the encoded values, as the
/// Histogram's).
pub fn brightness(rgb: [u8; 3]) -> f64 {
    0.2126 * unit(rgb[0]) + 0.7152 * unit(rgb[1]) + 0.0722 * unit(rgb[2])
}

fn decode(e: f64) -> f64 {
    if e <= 0.040_45 { e / 12.92 } else { ((e + 0.055) / 1.055).powf(2.4) }
}

/// The number panel `panel`'s setting `key` holds at the playhead.
fn number(gx: &GradeCx, panel: LayerId, key: &str) -> f64 {
    match gx.get(Some(panel), key) {
        Value::Number(v) if v.is_finite() => v,
        _ => 0.0,
    }
}

/// What a press on a pixel of colour `rgb` would move on panel `panel`, if anything.
pub fn aim(gx: &GradeCx, mode: &GradeMode, panel: LayerId, rgb: [u8; 3]) -> Option<Aim> {
    let kind = gx.card.layer(panel)?.kind.clone();
    match kind.as_str() {
        TONE_CURVE => {
            let curve_mode = match gx.get(Some(panel), "mode") {
                Value::Choice(name) => <CurveMode as bpe_core::Choice>::from_name(&name).unwrap_or_default(),
                _ => CurveMode::default(),
            };
            match curve_mode {
                CurveMode::Parametric => {
                    let x = brightness(rgb);
                    let raw =
                        ["split_shadows", "split_middle", "split_highlights"].map(|k| number(gx, panel, k) as f32);
                    let [a, b, c] = maths::splits(raw);
                    let key = if x < a {
                        "shadows"
                    } else if x < b {
                        "darks"
                    } else if x < c {
                        "lights"
                    } else {
                        "highlights"
                    };
                    let start = number(gx, panel, key);
                    Some(Aim::Sliders(vec![(full_key(Some(panel), key), start, 1.0)]))
                }
                CurveMode::Point => {
                    let channel = mode.channel.get(&panel).copied().unwrap_or_default();
                    let x = match channel {
                        Channel::Rgb => brightness(rgb),
                        Channel::Red => unit(rgb[0]),
                        Channel::Green => unit(rgb[1]),
                        Channel::Blue => unit(rgb[2]),
                    };
                    let points = match gx.get(Some(panel), channel.key()) {
                        Value::Curve(c) if bpe_core::curve_is_valid(&c) => c,
                        _ => bpe_gfx::straight_curve(),
                    };
                    let (points, index) = point_at(points, x);
                    Some(Aim::Point { key: full_key(Some(panel), channel.key()), points, index })
                }
            }
        }
        HSL => {
            let lab = maths::oklab([0, 1, 2].map(|i| decode(unit(rgb[i]))));
            let weights = maths::band_weights(lab[2].atan2(lab[1]));
            let part = match mode.hsl_view.get(&panel).copied().unwrap_or_default() {
                HslView::Hue => "hue",
                HslView::Luminance => "luminance",
                HslView::Saturation | HslView::All => "saturation",
            };
            let sliders = BANDS
                .iter()
                .zip(weights)
                .filter(|(_, w)| *w > 0.01)
                .map(|(band, w)| {
                    let key = format!("{band}_{part}");
                    (full_key(Some(panel), &key), number(gx, panel, &key), w)
                })
                .collect();
            Some(Aim::Sliders(sliders))
        }
        _ => None,
    }
}

/// The curve point to move for level `x`: the nearest within [`NEAR`], else a new one on the
/// curve at `x` when there is room, else the nearest.
pub fn point_at(mut points: Vec<CurvePoint>, x: f64) -> (Vec<CurvePoint>, usize) {
    let x = x.clamp(0.0, 1.0);
    let nearest = points
        .iter()
        .enumerate()
        .min_by(|a, b| (f64::from(a.1.x) - x).abs().total_cmp(&(f64::from(b.1.x) - x).abs()))
        .map(|(i, p)| (i, (f64::from(p.x) - x).abs()));
    let Some((near, gap)) = nearest else { return (bpe_gfx::straight_curve(), 0) };
    if gap <= NEAR {
        return (points, near);
    }
    let at = points.partition_point(|p| f64::from(p.x) < x);
    let roomy = points.len() < MAX_CURVE_POINTS
        && points.get(at).is_none_or(|p| f64::from(p.x) - x >= ROOM)
        && (at == 0 || x - f64::from(points[at - 1].x) >= ROOM);
    if !roomy {
        return (points, near);
    }
    let y = maths::point_curve(&points).at(x).clamp(0.0, 1.0);
    points.insert(at, CurvePoint::new(x as f32, y as f32, 0.0));
    (points, at)
}

/// The edits a drag `up` screen points up (down is negative) from the press asks for.
pub fn moved(aim: &Aim, up: f64) -> Vec<GradeEdit> {
    match aim {
        Aim::Sliders(sliders) => sliders
            .iter()
            .map(|(key, start, share)| {
                let v = (start + SLIDER_RATE * share * up).clamp(-100.0, 100.0);
                GradeEdit::Set(key.clone(), Value::Number((v * 10.0).round() / 10.0))
            })
            .collect(),
        Aim::Point { key, points, index } => {
            let mut points = points.clone();
            if let Some(p) = points.get_mut(*index) {
                p.y = (f64::from(p.y) + CURVE_RATE * up).clamp(0.0, 1.0) as f32;
            }
            vec![GradeEdit::Set(key.clone(), Value::Curve(points))]
        }
    }
}

/// The Target switch for panel `panel`: on, a drag on the picture moves this panel's settings for
/// the colour under the pointer.
pub fn toggle(ui: &mut Ui, mode: &mut GradeMode, panel: LayerId) {
    let on = mode.targeted == Some(panel);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Targeted adjustment"));
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, style::RADIUS, style::SELECTED);
    }
    let ink = if on || response.hovered() { style::TEXT_STRONG } else { style::MUTED };
    let c = rect.center();
    painter.circle_stroke(c, 6.0, Stroke::new(1.2, ink));
    painter.circle_filled(c, 1.8, ink);
    for (a, b) in
        [((-8.5, 0.0), (-4.0, 0.0)), ((4.0, 0.0), (8.5, 0.0)), ((0.0, -8.5), (0.0, -4.0)), ((0.0, 4.0), (0.0, 8.5))]
    {
        painter.line_segment([c + egui::vec2(a.0, a.1), c + egui::vec2(b.0, b.1)], Stroke::new(1.2, ink));
    }
    let help = if on {
        "Drag up or down on the picture to change this panel for the colour under the pointer. Click to stop (or Esc)."
    } else {
        "Targeted adjustment: drag up or down on the picture to change this panel for the colour under the pointer"
    };
    if response.on_hover_text(help).clicked() {
        mode.targeted = if on { None } else { Some(panel) };
        mode.target_drag = None;
    }
}

/// Whether the tool is on for a panel the card still has (it turns off when the panel goes).
pub fn active(studio: &mut Studio) -> bool {
    let Some(panel) = studio.grade.targeted else { return false };
    let start = studio.grade.start(&studio.kit.registry);
    let card = card_or_start(&studio.scene, &start);
    let there = card.layer(panel).is_some_and(|l| takes(&l.kind));
    if !there {
        studio.grade.targeted = None;
        studio.grade.target_drag = None;
    }
    there
}

/// The tool on the preview at `rect`: a press reads the colour under it with `read` (the picture
/// as shown), a drag moves the panel's settings. Call only while [`active`].
pub fn preview(
    ui: &mut Ui,
    response: &egui::Response,
    rect: Rect,
    studio: &mut Studio,
    read: impl FnOnce(Pos2) -> Option<[u8; 3]>,
) {
    let Some(panel) = studio.grade.targeted else { return };
    if response.hovered() || studio.grade.target_drag.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    pill(ui, rect, "Drag up or down on the picture (Esc to stop)");
    if studio.grade.target_drag.is_none()
        && response.drag_started_by(egui::PointerButton::Primary)
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && let Some(rgb) = read(press)
    {
        let start = studio.grade.start(&studio.kit.registry);
        let time = studio.sim_time();
        let card = card_or_start(&studio.scene, &start);
        let gx = GradeCx { scene: &studio.scene, kit: &studio.kit, card, time };
        if let Some(aim) = aim(&gx, &studio.grade, panel, rgb) {
            studio.grade.target_drag = Some(Drag { panel, press: press.y, last: press.y, aim });
            // Each drag is its own undo step.
            studio.panels.inspector.own_step = true;
        }
    }
    let fine = ui.input(|i| i.modifiers.shift);
    if let Some(drag) = studio.grade.target_drag.as_mut() {
        if let Some(p) = response.interact_pointer_pos()
            && p.y != drag.last
        {
            drag.last = p.y;
            let up = f64::from(drag.press - p.y) * if fine { 0.1 } else { 1.0 };
            let edits = moved(&drag.aim, up);
            super::apply(studio, edits);
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            studio.grade.target_drag = None;
        }
    }
}

/// A pill at the bottom of the picture.
fn pill(ui: &Ui, rect: Rect, text: &str) {
    let painter = ui.painter_at(rect);
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), style::TEXT_STRONG);
    let back = Rect::from_min_size(
        rect.left_bottom() + egui::vec2(10.0, -18.0 - galley.size().y),
        galley.size() + egui::vec2(16.0, 8.0),
    );
    painter.rect_filled(back, style::RADIUS, style::PILL);
    painter.galley(back.min + egui::vec2(8.0, 4.0), galley, style::TEXT_STRONG);
}

/// The panels the tool works on.
pub fn takes(kind: &str) -> bool {
    kind == TONE_CURVE || kind == HSL
}
