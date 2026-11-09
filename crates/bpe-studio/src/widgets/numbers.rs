//! Number widgets: how a number drags (its feel), sliders over a setting's span and past it.

use super::*;

/// The widget for `def`'s type, editing `value` in place. Every control is labelled by the
/// row's name, for screen readers and tests.
/// How a number moves under the mouse: the step it snaps to, how many decimals it shows, and
/// how far a drag on its number box goes per pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberFeel {
    pub step: f64,
    pub decimals: usize,
    pub drag_speed: f64,
    /// Where drags land while the grid's Snap is on: none for a setting that doesn't measure a
    /// distance or an angle, or while snapping is off or Alt is held.
    pub snap: Option<crate::setting_snap::Snapper>,
}

impl NumberFeel {
    /// This feel snapping drags with `snap`, showing enough decimals for its steps.
    pub fn with_snap(mut self, snap: Option<crate::setting_snap::Snapper>) -> Self {
        if let Some(s) = snap {
            self.decimals = self.decimals.max(s.decimals());
        }
        self.snap = snap;
        self
    }
}

/// The feel of a number spanning `span`: the step is the power of ten nearest a two-hundredth of
/// the span, but never more than 1, so every slider moves in round, even steps (0 to 1 by
/// 0.001, 0 to 40 by 0.1, 0 to 2000 and 1 to 5000 by 1). A drag on the number box crosses a
/// small span in about 400 pixels and counts a wide one in ones (Shift is ten times finer).
/// The cap matters for typing too: the slider snaps what is typed to its steps.
pub fn number_feel(span: f64, integer: bool) -> NumberFeel {
    let span = if span.is_finite() && span > 0.0 { span } else { 1.0 };
    let power = (span / 200.0).log10().floor().min(0.0);
    let mut step = 10f64.powf(power).max(1e-4);
    if integer {
        step = step.max(1.0).round();
    }
    let decimals = if integer { 0 } else { (-power).max(0.0) as usize };
    let drag_speed = (span / 400.0).min(1.0).max(step / 4.0);
    NumberFeel { step, decimals, drag_speed, snap: None }
}

/// The feel of a setting's number: from its range, else its usual span, else from its unit
/// (pixels move by whole pixels, other things by hundredths).
pub fn setting_feel(def: &SettingDef) -> NumberFeel {
    let integer = def.ty == ValueType::Int;
    // A slider covers the usual span when there is one, so it moves in the span's steps.
    match def.span.or(def.range) {
        Some((min, max)) => number_feel(max - min, integer),
        None if def.unit.starts_with("px") => number_feel(400.0, integer),
        None => number_feel(2.0, integer),
    }
}

/// A slider for a number setting: as wide as the row allows, moving in the setting's even steps,
/// with its number box beside it.
pub fn number_slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    x: &mut N,
    (min, max): (f64, f64),
    feel: NumberFeel,
    unit: String,
) -> Response {
    // Leave room for the number box at its widest, so the row never pushes the panel wider.
    let widest = format!("{}{:.*}{unit}", if min < 0.0 { "-" } else { "" }, feel.decimals, min.abs().max(max.abs()));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let number = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, style::TEXT).size().x);
    let room = number + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x + 4.0;
    ui.spacing_mut().slider_width = (ui.available_width() - room).clamp(48.0, 320.0);
    let mut slider = Slider::new(x, N::from_f64(min)..=N::from_f64(max))
        .suffix(unit)
        .smart_aim(false)
        .step_by(feel.step)
        .fixed_decimals(feel.decimals)
        .drag_value_speed(feel.drag_speed)
        // Only edits snap and clamp: a stored value is shown as it is, never rewritten by drawing it.
        .clamping(egui::SliderClamping::Edits);
    // A range spanning more than a thousandfold is easier to use on a log scale.
    if min > 0.0 && max / min > 1000.0 {
        slider = slider.logarithmic(true);
    }
    // The rail left of the knob is a lighter grey; nothing else here uses the selection colour.
    let width = ui.spacing().slider_width;
    let r = ui
        .scope(|ui| {
            ui.visuals_mut().selection.bg_fill = style::SLIDER_FILL;
            ui.add(slider.trailing_fill(true))
        })
        .inner;
    // Where the rail is, for a following setting's band.
    let rail = egui::Rect::from_min_size(r.rect.min, Vec2::new(width.min(r.rect.width()), r.rect.height()));
    ui.ctx().data_mut(|d| d.insert_temp(r.id.with("rail"), rail));
    if let Some(snap) = feel.snap
        && !(min > 0.0 && max / min > 1000.0)
    {
        // egui's handle travels the rail inset by its radius (`SliderGeometry::position_range`).
        let mut inset = rail.height() / 2.5;
        if let egui::style::HandleShape::Rect { aspect_ratio } = ui.style().visuals.handle_shape {
            inset *= aspect_ratio;
        }
        let x = rail.left() + inset..=rail.right() - inset;
        snap_ticks(&ui.painter_at(rail.expand(8.0)), snap, (min, max), x, rail.center().y);
    }
    r
}

/// How far past its usual span a value is: below it (-1), within it (0) or above it (1).
pub fn past_span(value: f64, (lo, hi): (f64, f64)) -> i8 {
    if value < lo.min(hi) {
        -1
    } else if value > lo.max(hi) {
        1
    } else {
        0
    }
}

/// A slider over a setting's usual `span` whose value can go past it (the lighting spec's "no
/// limits"): dragging the knob moves the value by how far the pointer goes, at the span's rate,
/// so it keeps going past either end; a click on the rail jumps there; the box beside it takes
/// any number. Only the hard `range`, when there is one, clamps. Past the span the knob sits at
/// that end with a small arrow beyond it, and the box is bright with a light border. Both parts
/// are named by `name`. Integer settings move and land on whole numbers.
#[allow(clippy::too_many_arguments)]
pub fn span_slider(
    ui: &mut Ui,
    value: &mut f64,
    span: (f64, f64),
    range: Option<(f64, f64)>,
    feel: NumberFeel,
    integer: bool,
    unit: String,
    name: Named,
) -> Response {
    let (lo, hi) = (span.0.min(span.1), span.0.max(span.1));
    let clamp = |v: f64| {
        let v = if integer { v.round() } else { v };
        match range {
            Some((a, b)) => v.clamp(a.min(b), a.max(b)),
            None => v,
        }
    };
    // Drags, clicks and keys land on the slider's steps, or on the grid while it snaps.
    let fit = |v: f64| match feel.snap {
        Some(s) => clamp(s.snap(v)),
        None => clamp(if feel.step > 0.0 { (v / feel.step).round() * feel.step } else { v }),
    };
    // Room for the number box at its widest within the span, so the row never pushes the panel wider.
    let widest = format!("{}{:.*}{unit}", if lo < 0.0 { "-" } else { "" }, feel.decimals, lo.abs().max(hi.abs()));
    let font = egui::TextStyle::Button.resolve(ui.style());
    let number = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, style::TEXT).size().x);
    let room = number + ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x + 10.0;
    let width = (ui.available_width() - room).clamp(48.0, 320.0);
    let height = ui.spacing().interact_size.y;
    let (rect, mut rail) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
    let before = *value;
    let rate = (hi - lo) / width.max(1.0) as f64;
    let start_id = rail.id.with("drag start");
    if rail.drag_started_by(PointerButton::Primary) {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, (*value, 0.0f64)));
    }
    if rail.dragged_by(PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let (start, moved) = ui.ctx().data(|d| d.get_temp::<(f64, f64)>(start_id)).unwrap_or((*value, 0.0));
        let moved = moved + rail.drag_delta().x as f64 * fine;
        ui.ctx().data_mut(|d| d.insert_temp(start_id, (start, moved)));
        *value = fit(start + moved * rate);
    } else if rail.clicked()
        && let Some(p) = rail.interact_pointer_pos()
    {
        let t = ((p.x - rect.left()) / width.max(1.0)).clamp(0.0, 1.0) as f64;
        *value = fit(lo + t * (hi - lo));
    }
    // Keys nudge it by a step, past the ends too.
    if rail.has_focus() {
        let (left, right) =
            ui.input(|i| (i.key_pressed(egui::Key::ArrowLeft) as i32, i.key_pressed(egui::Key::ArrowRight) as i32));
        if left != right {
            let dir = (right - left) as f64;
            *value = match feel.snap {
                Some(s) => clamp(s.nudge(*value, dir)),
                None => fit(*value + dir * feel.step.max(1e-9)),
            };
        }
    }
    if *value != before {
        rail.mark_changed();
    }
    let past = past_span(*value, (lo, hi));
    let t = if hi > lo { ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let painter = ui.painter_at(rect.expand(8.0));
    let y = rect.center().y;
    let left = rect.left() + 1.0;
    let right = rect.right() - 7.0;
    let x = left + (right - left) * t;
    let groove = egui::Rect::from_x_y_ranges(left..=right, y - 1.0..=y + 1.0);
    painter.rect_filled(groove, 0.0, style::LINE);
    if let Some(s) = feel.snap {
        snap_ticks(&painter, s, (lo, hi), left..=right, y);
    }
    let zero = if lo <= 0.0 && hi >= 0.0 && lo < 0.0 { left + (right - left) * (-lo / (hi - lo)) as f32 } else { left };
    let filled = egui::Rect::from_x_y_ranges(zero.min(x)..=zero.max(x), y - 1.0..=y + 1.0);
    painter.rect_filled(filled, 0.0, style::SLIDER_FILL);
    let hot = rail.hovered() || rail.dragged() || rail.has_focus();
    let knob = egui::Rect::from_center_size(egui::pos2(x, y), Vec2::new(3.0, if hot { 16.0 } else { 14.0 }));
    painter.rect_filled(knob, 0.0, style::TEXT_STRONG);
    if past != 0 {
        // A small arrow just past the end it has gone beyond.
        let (tip, back) = if past > 0 { (right + 7.0, right + 2.0) } else { (left - 7.0, left - 2.0) };
        painter.add(egui::Shape::convex_polygon(
            vec![egui::pos2(tip, y), egui::pos2(back, y - 4.0), egui::pos2(back, y + 4.0)],
            style::TEXT_STRONG,
            egui::Stroke::NONE,
        ));
    }
    let words = format!("{:.*}{unit}", feel.decimals, *value);
    rail.widget_info(|| egui::WidgetInfo::slider(true, *value, &words));
    let rail = name.on(ui, rail);
    // Where the rail is, for a following setting's band.
    let line = egui::Rect::from_x_y_ranges(left..=right, rect.top()..=rect.bottom());
    ui.ctx().data_mut(|d| d.insert_temp(rail.id.with("rail"), line));
    let mut typed = *value;
    let boxed = ui
        .scope(|ui| {
            if past != 0 {
                let w = &mut ui.visuals_mut().widgets;
                for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                    state.fg_stroke.color = style::TEXT_STRONG;
                    state.bg_stroke = egui::Stroke::new(1.0, style::TEXT);
                }
            }
            let mut field =
                DragValue::new(&mut typed).speed(feel.drag_speed).fixed_decimals(feel.decimals).suffix(unit.clone());
            if let Some((a, b)) = range {
                // A value already past the range (keyed or from a file) shows as it is.
                field = field.range(a.min(b)..=a.max(b)).clamp_existing_to_range(false);
            }
            ui.add(field)
        })
        .inner;
    let boxed = name.on(ui, boxed);
    // Dragging the box lands on the grid too; a typed number stays exact.
    crate::setting_snap::after_drag(ui, &boxed, *value, &mut typed, feel.snap, integer, range);
    if boxed.changed() {
        *value = if integer { typed.round() } else { typed };
    }
    if past != 0 {
        boxed.clone().on_hover_text("Past its usual span");
    }
    // The slider's own rail rect, for whoever reads `response.rect`.
    let mut both = rail.union(boxed.clone());
    if boxed.changed() {
        both.mark_changed();
    }
    both
}

/// Small ticks under a rail from `x` (the span `lo..hi` across it, centre line at `y`) where a
/// snapping slider's steps are, when they are far enough apart to read.
pub(super) fn snap_ticks(
    painter: &egui::Painter,
    snap: crate::setting_snap::Snapper,
    (lo, hi): (f64, f64),
    x: std::ops::RangeInclusive<f32>,
    y: f32,
) {
    let (left, right) = (*x.start(), *x.end());
    let Some(gap) = snap.tick_gap(lo, hi, right - left) else { return };
    if hi <= lo {
        return;
    }
    let mut k = (lo / gap).ceil();
    while k * gap <= hi {
        let t = ((k * gap - lo) / (hi - lo)) as f32;
        let at = left + (right - left) * t;
        painter.vline(at, y + 3.0..=y + 6.0, egui::Stroke::new(1.0, style::FAINT));
        k += 1.0;
    }
}
