//! The round knob of the sound panels: a 40 × 40 dial with its label above and its value below.
//! Drag up or down to turn it (Shift for fine steps), roll the wheel to nudge it, double-click the
//! dial to put it back, double-click the value to type one.

use super::fonts::mono;
use super::tokens::*;
use eframe::egui::{self, Color32, Response, Sense, Shape, Stroke, Ui, Vec2, Widget};
use std::ops::RangeInclusive;

/// A modulation range drawn as a ring around the dial, in the knob's own units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModRing {
    pub from: f64,
    pub to: f64,
}

/// What a knob did this frame.
pub struct KnobResponse {
    /// The dial's response (named by the knob's label).
    pub response: Response,
    /// The value changed this frame.
    pub changed: bool,
    /// The dial was right-clicked: the caller opens its value menu.
    #[allow(dead_code)]
    pub context: bool,
}

/// A round knob over `range`.
pub struct Knob<'a> {
    value: &'a mut f64,
    range: RangeInclusive<f64>,
    default: f64,
    label: &'a str,
    format: Option<&'a dyn Fn(f64) -> String>,
    bipolar: bool,
    rings: &'a [ModRing],
    colour: Color32,
    log: bool,
    width: f32,
}

/// The dial's side.
const DIAL: f32 = 40.0;
/// The knob's whole width (label and value fit under it).
const WIDTH: f32 = 64.0;
/// A full turn is this many pixels of drag.
const FULL_DRAG: f64 = 200.0;
/// The dial sweeps this many degrees.
const SWEEP: f32 = 270.0;

impl<'a> Knob<'a> {
    pub fn new(value: &'a mut f64, range: RangeInclusive<f64>, default: f64, label: &'a str) -> Self {
        Knob {
            value,
            range,
            default,
            label,
            format: None,
            bipolar: false,
            rings: &[],
            colour: TEXT,
            log: false,
            width: WIDTH,
        }
    }

    /// How the value reads under the dial (a plain number when not given).
    pub fn format(mut self, format: &'a dyn Fn(f64) -> String) -> Self {
        self.format = Some(format);
        self
    }

    /// The value arc starts at the middle of the range, not its bottom.
    pub fn bipolar(mut self, bipolar: bool) -> Self {
        self.bipolar = bipolar;
        self
    }

    /// Modulation rings around the dial.
    #[allow(dead_code)]
    pub fn rings(mut self, rings: &'a [ModRing]) -> Self {
        self.rings = rings;
        self
    }

    /// The colour of the rings: the thing's track colour.
    #[allow(dead_code)]
    pub fn colour(mut self, colour: Color32) -> Self {
        self.colour = colour;
        self
    }

    /// Turning is even in ratios (frequencies, times), not in steps. Needs a range above 0.
    pub fn log(mut self, log: bool) -> Self {
        self.log = log && *self.range.start() > 0.0;
        self
    }

    /// The knob's whole width (the dial stays the same size); narrower knobs fit six in a row of
    /// the Sound tab.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(DIAL);
        self
    }

    /// Where `v` sits along the dial, 0 at the bottom of the range and 1 at the top.
    fn to_turn(&self, v: f64) -> f64 {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        if hi <= lo {
            return 0.0;
        }
        let t = if self.log { (v / lo).ln() / (hi / lo).ln() } else { (v - lo) / (hi - lo) };
        if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The value at turn `t`.
    fn value_at_turn(&self, t: f64) -> f64 {
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let t = t.clamp(0.0, 1.0);
        if self.log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
    }

    fn text(&self, v: f64) -> String {
        match self.format {
            Some(f) => f(v),
            None => format!("{v:.2}"),
        }
    }

    pub fn show(self, ui: &mut Ui) -> KnobResponse {
        let before = *self.value;
        let inner = ui.allocate_ui_with_layout(
            Vec2::new(self.width, DIAL + 36.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_width(self.width);
                ui.spacing_mut().item_spacing.y = 2.0;
                let label = ui.add(
                    egui::Label::new(egui::RichText::new(self.label).small().color(MUTED)).selectable(false).truncate(),
                );
                let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(DIAL), Sense::click_and_drag());
                response = response.labelled_by(label.id).on_hover_cursor(egui::CursorIcon::ResizeVertical);

                if response.dragged_by(egui::PointerButton::Primary) {
                    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                    let dy = response.drag_delta().y as f64;
                    let t = self.to_turn(*self.value) - dy / FULL_DRAG * fine;
                    *self.value = self.value_at_turn(t);
                }
                if response.hovered() {
                    // Each wheel notch nudges 1 %; the scroll is kept from the panel underneath.
                    let notches: f64 = ui.input(|i| {
                        i.events
                            .iter()
                            .filter_map(|e| match e {
                                egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => {
                                    Some(delta.y.signum() as f64)
                                }
                                _ => None,
                            })
                            .sum()
                    });
                    if notches != 0.0 {
                        let t = self.to_turn(*self.value) + 0.01 * notches;
                        *self.value = self.value_at_turn(t);
                    }
                    ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
                }
                if response.double_clicked() {
                    *self.value = self.default;
                }
                let context = response.secondary_clicked();

                if ui.is_rect_visible(rect) {
                    self.paint(ui, rect, response.hovered() || response.dragged());
                }

                // The value under the dial, or the box to type one.
                let edit_id = response.id.with("typing");
                let typing: Option<String> = ui.data(|d| d.get_temp(edit_id));
                match typing {
                    Some(mut text) => {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .hint_text(self.text(*self.value))
                                .font(mono(11.0))
                                .desired_width(self.width - 6.0),
                        );
                        let name = format!("Type {}", self.label);
                        ui.ctx().accesskit_node_builder(edit.id, |node| node.set_label(name));
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                        }
                        if edit.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && let Some(v) = parse(&text)
                            {
                                *self.value = v.clamp(*self.range.start(), *self.range.end());
                            }
                            ui.data_mut(|d| d.remove::<String>(edit_id));
                        } else {
                            ui.data_mut(|d| d.insert_temp(edit_id, text));
                        }
                    }
                    None => {
                        let shown = ui.add(
                            egui::Label::new(egui::RichText::new(self.text(*self.value)).font(mono(11.0)).color(TEXT))
                                .selectable(false)
                                .sense(Sense::click()),
                        );
                        if shown.double_clicked() {
                            ui.data_mut(|d| d.insert_temp(edit_id, String::new()));
                            ui.ctx().request_repaint();
                        }
                    }
                }

                let value = *self.value;
                let name = self.label.to_owned();
                response.widget_info(|| egui::WidgetInfo::slider(true, value, &name));
                if value != before {
                    response.mark_changed();
                }
                (response, context)
            },
        );
        let (response, context) = inner.inner;
        KnobResponse { changed: *self.value != before, response, context }
    }

    /// Draws the dial: the track, the value arc, the pointer and the modulation rings.
    fn paint(&self, ui: &Ui, rect: egui::Rect, hot: bool) {
        let painter = ui.painter();
        let c = rect.center();
        let t = self.to_turn(*self.value) as f32;
        painter.add(Shape::line(arc(c, 15.0, 0.0, 1.0), Stroke::new(3.0, if hot { FAINT } else { LINE })));
        let start = if self.bipolar { 0.5 } else { 0.0 };
        if (t - start).abs() > 1e-4 {
            painter.add(Shape::line(arc(c, 15.0, start.min(t), start.max(t)), Stroke::new(3.0, MUTED)));
        }
        for ring in self.rings {
            let (a, b) = (self.to_turn(ring.from) as f32, self.to_turn(ring.to) as f32);
            if (a - b).abs() > 1e-4 {
                painter.add(Shape::line(arc(c, 18.5, a.min(b), a.max(b)), Stroke::new(2.0, self.colour)));
            }
        }
        let dir = direction(t);
        painter.line_segment([c + dir * 5.0, c + dir * 13.0], Stroke::new(2.0, TEXT_STRONG));
    }
}

/// The screen direction of turn `t` (0 at the bottom left, 1 at the bottom right, clockwise).
fn direction(t: f32) -> Vec2 {
    let a = (135.0 + SWEEP * t).to_radians();
    Vec2::new(a.cos(), a.sin())
}

/// Points along the dial's circle of radius `r` from turn `t0` to `t1`.
fn arc(c: egui::Pos2, r: f32, t0: f32, t1: f32) -> Vec<egui::Pos2> {
    let n = ((t1 - t0) * 48.0).ceil().max(2.0) as usize;
    (0..=n).map(|i| c + direction(t0 + (t1 - t0) * i as f32 / n as f32) * r).collect()
}

impl Widget for Knob<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Reads a typed number, ignoring a unit after it ("−6 dB", "120bpm").
fn parse(text: &str) -> Option<f64> {
    let text = text.trim().replace('\u{2212}', "-");
    let run: String =
        text.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')).collect();
    (1..=run.len()).rev().find_map(|end| run[..end].parse::<f64>().ok()).filter(|v| v.is_finite())
}

/// A knob with a plain number readout.
#[allow(dead_code)]
pub fn knob(ui: &mut Ui, value: &mut f64, range: RangeInclusive<f64>, default: f64, label: &str) -> Response {
    ui.add(Knob::new(value, range, default, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn db(v: f64) -> String {
        format!("{v:.1} dB")
    }

    fn harness(start: f64) -> Harness<'static, f64> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(200.0, 200.0))
            .with_step_dt(1.0 / 60.0)
            .allow_missing_glyphs()
            .with_context_setup(super::super::apply)
            .build_ui_state(
                |ui, v: &mut f64| {
                    ui.add(Knob::new(v, -24.0..=24.0, 0.0, "Input").format(&db));
                },
                start,
            );
        h.run();
        h
    }

    fn dial(h: &Harness<'_, f64>) -> egui::Rect {
        h.query_all_by_label("Input")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .unwrap()
            .rect()
    }

    fn press(h: &mut Harness<'_, f64>, at: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
    }

    /// Drags from the dial's middle by `dy` pixels over a few frames.
    fn drag(h: &mut Harness<'_, f64>, dy: f32, modifiers: egui::Modifiers) {
        let from = dial(h).center();
        h.hover_at(from);
        h.run();
        h.event(egui::Event::ModifiersChanged(modifiers));
        press(h, from, true, modifiers);
        h.step();
        for k in 1..=4 {
            h.event(egui::Event::PointerMoved(from + egui::vec2(0.0, dy * k as f32 / 4.0)));
            h.step();
        }
        press(h, from + egui::vec2(0.0, dy), false, modifiers);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run();
    }

    fn double_click(h: &mut Harness<'_, f64>, at: egui::Pos2) {
        h.hover_at(at);
        h.run();
        for _ in 0..2 {
            press(h, at, true, egui::Modifiers::NONE);
            h.step();
            press(h, at, false, egui::Modifiers::NONE);
            h.step();
        }
        h.run();
    }

    #[test]
    fn dragging_up_raises_the_value() {
        let mut h = harness(0.0);
        drag(&mut h, -50.0, egui::Modifiers::NONE);
        // 50 px of a 200 px turn over a 48 dB range.
        assert!((*h.state() - 12.0).abs() < 0.01, "{}", h.state());
        drag(&mut h, 25.0, egui::Modifiers::NONE);
        assert!((*h.state() - 6.0).abs() < 0.01, "{}", h.state());
    }

    #[test]
    fn shift_drag_is_ten_times_finer() {
        let mut h = harness(0.0);
        drag(&mut h, -50.0, egui::Modifiers::SHIFT);
        assert!((*h.state() - 1.2).abs() < 0.01, "{}", h.state());
    }

    #[test]
    fn double_click_resets() {
        let mut h = harness(9.0);
        let at = dial(&h).center();
        double_click(&mut h, at);
        assert_eq!(*h.state(), 0.0);
    }

    #[test]
    fn typing_a_value_sets_it() {
        let mut h = harness(3.0);
        let at = h.get_by_label("3.0 dB").rect().center();
        double_click(&mut h, at);
        h.get_by_label("Type Input").type_text("\u{2212}6 dB");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(*h.state(), -6.0);
        assert!(h.query_by_label("Type Input").is_none(), "the box closes");
        assert!(h.query_by_label("-6.0 dB").is_some());
    }

    #[test]
    fn knob_is_named_by_its_label() {
        let h = harness(0.0);
        let node = h
            .query_all_by_label("Input")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .expect("the dial is named Input");
        assert_eq!(node.accesskit_node().numeric_value(), Some(0.0));
    }

    #[test]
    fn the_wheel_nudges_one_percent() {
        let mut h = harness(0.0);
        h.hover_at(dial(&h).center());
        h.run();
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(20);
        assert!((*h.state() - 0.48).abs() < 0.01, "{}", h.state());
    }

    #[test]
    fn parse_reads_units_and_the_minus_sign() {
        assert_eq!(parse("\u{2212}6 dB"), Some(-6.0));
        assert_eq!(parse(" 120.5bpm"), Some(120.5));
        assert_eq!(parse("-0.25"), Some(-0.25));
        assert_eq!(parse("dB"), None);
    }
}
