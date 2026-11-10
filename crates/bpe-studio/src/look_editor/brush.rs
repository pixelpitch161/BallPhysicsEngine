//! Painting brush strokes on the ball view, for a Brush shading or a Brush cut. The brush's
//! settings are the view's, not saved; each stroke keeps the ones it was painted with. A stroke
//! is one edit when the button is let go; while it is painted the view shows it from the stroke
//! held here.

use bpe_core::{Layer, LayerId, Stroke, Strokes, ThingId, Value, ValueType, layered_key};
use eframe::egui::{self, RichText, Slider, Stroke as Line, Ui};
use glam::Vec2;

use super::LookCx;
use super::ball_view::{BallView, ViewMap};
use crate::inspector::Edit;
use crate::style;

/// Points of a stroke closer than this (in radii) to the last one are skipped.
const SPACING: f32 = 0.01;

/// The brush the view paints with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub erase: bool,
    /// Its radius, in ball radii.
    pub size: f32,
    pub softness: f32,
    pub strength: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Brush { erase: false, size: 0.15, softness: 0.5, strength: 1.0 }
    }
}

/// A stroke being painted on a layer's strokes.
#[derive(Clone, Debug, PartialEq)]
pub struct Painting {
    pub thing: ThingId,
    pub layer: LayerId,
    /// The layered key of the strokes it adds to.
    pub key: String,
    /// The strokes as they were when it started.
    pub before: Strokes,
    pub points: Vec<[f32; 2]>,
}

impl Painting {
    /// The strokes with this one added, painted with `brush`.
    pub fn strokes(&self, brush: Brush) -> Strokes {
        let mut out = self.before.clone();
        out.list.push(Stroke {
            points: self.points.clone(),
            size: brush.size,
            softness: brush.softness,
            strength: brush.strength,
            erase: brush.erase,
            density: 1.0,
        });
        out
    }
}

/// The key of the strokes the picked layer paints into: its Strokes setting, when it shows (a
/// Brush shading or a Brush cut).
pub fn target(cx: &LookCx, layer: &Layer) -> Option<&'static str> {
    let kind = cx.reg().layer(&layer.kind)?;
    let get = |k: &str| cx.scene.value_at_time(cx.reg(), &cx.at(layer.id, k), cx.time);
    let thing = cx.scene.things.get(&cx.id).map(|t| t.kind.as_str());
    kind.settings().iter().find(|d| d.ty == ValueType::Strokes && d.applies(thing, &get)).map(|d| d.key)
}

/// The stroke being painted on `thing`'s layer `layer`, as the view should show it.
pub fn preview(view: &BallView, thing: ThingId, layer: LayerId) -> Option<(String, Value)> {
    let p = view.painting.as_ref().filter(|p| p.thing == thing && p.layer == layer)?;
    Some((p.key.clone(), Value::Strokes(p.strokes(view.brush))))
}

/// The brush's bar: Paint or Erase, then its size, softness and strength.
pub fn bar(ui: &mut Ui, brush: &mut Brush) {
    ui.horizontal_wrapped(|ui| {
        for (erase, words, help) in
            [(false, "Paint", "Strokes add to the cut or shading"), (true, "Erase", "Strokes take it away")]
        {
            let button = style::toggle_button(words, brush.erase == erase).frame_when_inactive(true);
            if ui.add(button).on_hover_text(help).clicked() {
                brush.erase = erase;
            }
        }
    });
    // A double-click on a slider's rail puts the brush's starting value back.
    let start = Brush::default();
    let row =
        |ui: &mut Ui, name: &str, value: &mut f32, start: f32, range: std::ops::RangeInclusive<f32>, help: &str| {
            ui.horizontal(|ui| {
                let label = ui.label(RichText::new(name).color(style::MUTED)).on_hover_text(help);
                let r = ui.add(Slider::new(value, range).fixed_decimals(2)).labelled_by(label.id);
                if crate::widgets::rail_double_clicked(&r, crate::widgets::slider_rail(ui, &r)) {
                    *value = start;
                }
            });
        };
    let size_help = "How wide the brush is, as a share of the ball's radius";
    row(ui, "Brush size", &mut brush.size, start.size, 0.02..=0.5, size_help);
    let softness_help = "0 paints a hard edge, 1 fades all the way in";
    row(ui, "Brush softness", &mut brush.softness, start.softness, 0.0..=1.0, softness_help);
    row(ui, "Brush strength", &mut brush.strength, start.strength, 0.0..=1.0, "How much one stroke paints");
}

/// Paints with the pointer on the view: a press starts a stroke, dragging adds to it, and
/// letting go keeps it as one edit of `key` on layer `layer`. A circle the size of the brush
/// follows the pointer over the view. True when a stroke was kept this frame.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    ui: &Ui,
    view: &mut BallView,
    response: &egui::Response,
    map: ViewMap,
    cx: &LookCx,
    layer: LayerId,
    key: &'static str,
    edits: &mut Vec<Edit>,
) -> bool {
    let on_ball = |p: egui::Pos2| {
        let q = map.to_ball(p).clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
        [q.x, q.y]
    };
    let layered = layered_key(layer, key);
    let start = |view: &mut BallView, at: egui::Pos2| {
        let before = match cx.scene.value_at_time(cx.reg(), &cx.at(layer, key), cx.time) {
            Some(Value::Strokes(s)) => s,
            _ => Strokes::default(),
        };
        view.painting = Some(Painting { thing: cx.id, layer, key: layered.clone(), before, points: vec![on_ball(at)] });
    };
    if response.drag_started()
        && let Some(at) = ui.input(|i| i.pointer.press_origin())
    {
        start(view, at);
    }
    if response.dragged()
        && let (Some(at), Some(p)) = (response.interact_pointer_pos(), view.painting.as_mut())
    {
        let q = on_ball(at);
        let far = p.points.last().is_none_or(|l| Vec2::from(*l).distance(Vec2::from(q)) >= SPACING);
        if far {
            p.points.push(q);
        }
    }
    let mut kept = false;
    // A click paints a dab.
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        start(view, at);
    }
    if (response.drag_stopped() || response.clicked())
        && let Some(p) = view.painting.take()
        && p.thing == cx.id
        && p.layer == layer
    {
        edits.push(Edit::SetSlot(cx.slot, p.key.clone(), Value::Strokes(p.strokes(view.brush))));
        kept = true;
    }
    if let Some(at) = response.hover_pos() {
        let ring = if view.brush.erase { Line::new(1.0, style::MUTED) } else { Line::new(1.0, style::TEXT_STRONG) };
        ui.painter().circle_stroke(at, view.brush.size * map.radius, ring);
    }
    kept
}
