//! Painting a Brush part on the preview in Grade mode (colour grading part 5): Lightroom's brush.
//! Brushes A, B and Erase each keep their own Size, Feather, Flow and Density (the editor's, not
//! saved; each stroke keeps the ones it was painted with). While a Brush part shows its handles
//! the preview shows the brush circle (its size, and where its feather starts) and paints: a press
//! starts a stroke, dragging adds to it, letting go keeps it as one edit (one undo step). Alt
//! held erases. While a stroke is painted the preview shows it from the stroke held here
//! ([`GradeMode::show_on`]), with no edit per frame.
//!
//! Points are in the part's first place's units (video pixels from where it sits, turned and
//! sized with what it follows), as the gradients' X and Y.

use super::GradeMode;
use super::cx::{GradeEdit, full_key};
use crate::app::Studio;
use crate::style;
use bpe_core::{LayerId, SlotId, Stroke, Strokes, ThingId, Value};
use bpe_gfx::{GradePlace, GradeShape, GradeStack};
use bpe_kit::modules::grade;
use eframe::egui::{self, CursorIcon, Pos2, Rect, RichText, Ui};

/// Brushes A and B and the Erase brush.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    A,
    B,
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::A, Tool::B, Tool::Erase];

    pub fn name(self) -> &'static str {
        match self {
            Tool::A => "A",
            Tool::B => "B",
            Tool::Erase => "Erase",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One brush's settings, as Lightroom shows them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    /// Its radius, in video pixels.
    pub size: f32,
    /// 0..100: how far in from its edge it fades.
    pub feather: f32,
    /// 0..100: how much one stroke paints.
    pub flow: f32,
    /// 0..100: the most paint strokes build up to.
    pub density: f32,
}

/// The three brushes and which one paints.
#[derive(Clone, Debug, PartialEq)]
pub struct Brushes {
    pub tool: Tool,
    pub settings: [BrushSettings; 3],
}

impl Default for Brushes {
    fn default() -> Self {
        Self {
            tool: Tool::A,
            settings: [
                BrushSettings { size: 80.0, feather: 50.0, flow: 100.0, density: 100.0 },
                BrushSettings { size: 25.0, feather: 0.0, flow: 100.0, density: 100.0 },
                BrushSettings { size: 60.0, feather: 50.0, flow: 100.0, density: 100.0 },
            ],
        }
    }
}

impl Brushes {
    /// The chosen brush's settings.
    pub fn chosen(&self) -> BrushSettings {
        self.settings[self.tool.index()]
    }

    pub fn chosen_mut(&mut self) -> &mut BrushSettings {
        &mut self.settings[self.tool.index()]
    }

    /// A stroke through `points` (in a place's units) painted with the chosen brush on a place of
    /// scale `scale`; `alt` (Alt held) erases with any brush.
    pub fn stroke(&self, points: Vec<[f32; 2]>, scale: f32, alt: bool) -> Stroke {
        let b = self.chosen();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Stroke {
            points,
            size: (b.size / scale).max(1e-3),
            softness: (b.feather / 100.0).clamp(0.0, 1.0),
            strength: (b.flow / 100.0).clamp(0.0, 1.0),
            erase: alt || self.tool == Tool::Erase,
            density: (b.density / 100.0).clamp(0.0, 1.0),
        }
    }
}

/// A stroke being painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Painting {
    pub mask: LayerId,
    pub part: LayerId,
    /// The place it is painted through (the part's first, at the press).
    pub place: GradePlace,
    /// The part's strokes when it started.
    pub before: Strokes,
    pub stroke: Stroke,
}

impl Painting {
    /// The strokes with this one added.
    pub fn strokes(&self) -> Strokes {
        let mut out = self.before.clone();
        out.list.push(self.stroke.clone());
        out
    }

    /// Adds screen point `p` (video pixels from the picture's middle), unless it is under a video
    /// pixel from the last point. Returns whether it was added.
    pub fn add(&mut self, p: [f32; 2]) -> bool {
        let q = unplaced(&self.place, p);
        let far = 1.0 / self.place.scale.max(1e-6);
        let near = self.stroke.points.last().is_some_and(|l| (l[0] - q[0]).hypot(l[1] - q[1]) < far);
        if near || !q[0].is_finite() || !q[1].is_finite() {
            return false;
        }
        self.stroke.points.push(q);
        true
    }
}

/// A screen point back in a place's own units.
pub fn unplaced(place: &GradePlace, p: [f32; 2]) -> [f32; 2] {
    let q = bpe_gfx::grade::maths::unplaced(place, [f64::from(p[0]), f64::from(p[1])]);
    [q[0] as f32, q[1] as f32]
}

/// Puts the stroke being painted into `grade` (the preview's), on its part.
pub fn show_painting(painting: &Painting, grade: &mut GradeStack) {
    let part = grade
        .masks
        .iter_mut()
        .filter(|m| m.id == painting.mask.0)
        .filter_map(|m| m.area.as_mut())
        .flat_map(|a| a.parts.iter_mut())
        .find(|p| p.id == painting.part.0);
    if let Some(p) = part
        && let GradeShape::Brush(b) = &mut p.shape
    {
        b.strokes.list.push(painting.stroke.clone());
    }
}

/// The first place of Brush part `part` of mask `mask` as the preview last drew it.
pub fn place_of(stack: &GradeStack, mask: LayerId, part: LayerId) -> Option<GradePlace> {
    let m = stack.masks.iter().find(|m| m.id == mask.0)?;
    let p = m.area.as_ref()?.parts.iter().find(|p| p.id == part.0)?;
    match &p.shape {
        GradeShape::Brush(b) => b.places.first().copied().filter(bpe_gfx::grade::maths::place_ok),
        _ => None,
    }
}

/// Part `part`'s strokes on the card, as saved.
fn strokes_of(studio: &Studio, part: LayerId) -> Strokes {
    let time = studio.sim_time();
    let value =
        studio.scene.value_at_time(&studio.kit.registry, &super::cx::at(&full_key(Some(part), "strokes")), time);
    match value {
        Some(Value::Strokes(s)) => s,
        _ => Strokes::default(),
    }
}

/// The brush on the preview: its circle, and painting. `view_scale` is preview pixels per video
/// pixel.
pub fn preview(
    ui: &mut Ui,
    response: &egui::Response,
    rect: Rect,
    view_scale: f32,
    studio: &mut Studio,
    (mask, part): (LayerId, LayerId),
) {
    let place = studio.grade.painting.as_ref().filter(|p| p.part == part).map(|p| p.place);
    let Some(place) = place.or_else(|| place_of(&studio.grade.shown.borrow(), mask, part)) else {
        studio.grade.painting = None;
        return;
    };
    if studio.grade.painting.as_ref().is_some_and(|p| p.part != part) {
        studio.grade.painting = None;
    }
    let px = 1.0 / view_scale.max(1e-6);
    let units = |p: Pos2| [(p.x - rect.center().x) * px, (p.y - rect.center().y) * px];
    let alt = ui.input(|i| i.modifiers.alt);

    // A press starts a stroke (a click is a dot).
    let dragged = response.drag_started_by(egui::PointerButton::Primary);
    let start = if dragged {
        ui.input(|i| i.pointer.press_origin()).or_else(|| response.interact_pointer_pos())
    } else if response.clicked() {
        response.interact_pointer_pos()
    } else {
        None
    };
    if studio.grade.painting.is_none()
        && let Some(at) = start.map(units)
    {
        let stroke = studio.grade.brushes.stroke(Vec::new(), place.scale, alt);
        let mut painting = Painting { mask, part, place, before: strokes_of(studio, part), stroke };
        painting.add(at);
        studio.grade.painting = Some(painting);
    }
    let mut done = None;
    if let Some(painting) = studio.grade.painting.as_mut() {
        if let Some(p) = response.interact_pointer_pos().map(units) {
            painting.add(p);
        }
        if response.clicked() || response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            done = studio.grade.painting.take();
        }
    }
    if let Some(painting) = done {
        // The whole stroke is one undo step.
        studio.panels.inspector.own_step = true;
        let edit = GradeEdit::Set(full_key(Some(part), "strokes"), Value::Strokes(painting.strokes()));
        super::apply(studio, vec![edit]);
    }

    // The brush circle: its size, and where its feather starts.
    let Some(hover) = response.hover_pos().or_else(|| response.interact_pointer_pos()) else { return };
    ui.ctx().set_cursor_icon(CursorIcon::None);
    let b = studio.grade.brushes.chosen();
    let radius = b.size.max(0.5) * view_scale;
    let inner = radius * (1.0 - (b.feather / 100.0).clamp(0.0, 1.0));
    let painter = ui.painter_at(rect);
    let ink = if alt || studio.grade.brushes.tool == Tool::Erase { style::MUTED } else { style::TEXT_STRONG };
    for (r, w) in [(radius, 1.5), (inner, 1.0)] {
        if r > 0.5 {
            painter.circle_stroke(hover, r, egui::Stroke::new(w + 2.0, style::OUTLINE));
            painter.circle_stroke(hover, r, egui::Stroke::new(w, ink));
        }
    }
    if alt || studio.grade.brushes.tool == Tool::Erase {
        painter.line_segment([hover - egui::vec2(3.0, 0.0), hover + egui::vec2(3.0, 0.0)], egui::Stroke::new(1.5, ink));
    } else {
        painter.circle_filled(hover, 1.5, ink);
    }
}

/// A Brush part's box: the brushes, the chosen one's sliders, how many strokes and Clear.
pub fn bar(ui: &mut Ui, mode: &mut GradeMode, id: LayerId, strokes: &Strokes, edits: &mut Vec<GradeEdit>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for tool in Tool::ALL {
            let help = match tool {
                Tool::A => "Brush A: paint with its own size, feather, flow and density",
                Tool::B => "Brush B: a second brush with its own settings",
                Tool::Erase => "Erase: take paint away (or hold Alt with A or B)",
            };
            if ui.add(style::toggle_button(tool.name(), mode.brushes.tool == tool)).on_hover_text(help).clicked() {
                mode.brushes.tool = tool;
            }
        }
    });
    let b = mode.brushes.chosen_mut();
    let slider = |ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, help: &str| {
        ui.horizontal(|ui| {
            let l = ui.label(RichText::new(label).small().color(style::MUTED));
            ui.add(egui::Slider::new(value, range).max_decimals(0)).labelled_by(l.id).on_hover_text(help);
        });
    };
    slider(ui, "Size", &mut b.size, 1.0..=1000.0, "The brush's radius, in pixels of the video");
    slider(ui, "Feather", &mut b.feather, 0.0..=100.0, "How far in from its edge it fades");
    slider(ui, "Flow", &mut b.flow, 0.0..=100.0, "How much one stroke paints");
    slider(ui, "Density", &mut b.density, 0.0..=100.0, "The most paint strokes build up to");
    ui.horizontal(|ui| {
        let n = strokes.list.len();
        let words = match n {
            0 => "Paint on the picture".to_string(),
            1 => "1 stroke".to_string(),
            n => format!("{n} strokes"),
        };
        ui.label(RichText::new(words).small().color(style::FAINT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = ui.add_enabled(n > 0, egui::Button::new("Clear")).on_hover_text("Take every stroke away");
            if clear.clicked() {
                edits.push(GradeEdit::Set(full_key(Some(id), "strokes"), Value::Strokes(Strokes::default())));
            }
        });
    });
    ui.add_space(4.0);
}

/// Whether part `part` of the card is a Brush.
pub fn is_brush(card: &bpe_core::Slot, part: LayerId) -> bool {
    card.layer(part).is_some_and(|l| l.kind == grade::BRUSH)
}

/// The Brush part the preview paints on, when the selected mask's handle part is one.
pub fn painted(studio: &Studio) -> Option<(LayerId, LayerId)> {
    let card = studio.scene.card(ThingId::ROOT, SlotId::Grade).filter(|c| c.kind == grade::GRADE)?;
    let mask = studio.grade.mask.filter(|m| grade::masks_of(card).iter().any(|l| l.id == *m))?;
    let part = super::handles::handle_part(card, &studio.grade.part, mask)?;
    is_brush(card, part).then_some((mask, part))
}
