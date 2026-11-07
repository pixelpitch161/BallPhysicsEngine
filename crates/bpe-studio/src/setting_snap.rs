//! Snapping inspector settings to the grid while they are dragged (plan
//! `docs/superpowers/plans/2026-11-07-snap-settings.md`). The toolbar's Snap button turns it on
//! for the preview's drags and the inspector's alike. Lengths in px land on whole grid squares
//! (eighths of one under one square), places on whole squares and angles on the angle step. Alt
//! held drags freely; typed numbers are never snapped.
//!
//! The app puts the frame's snap into egui's memory once a frame ([`put`]) and the number widgets
//! read it ([`now`]), so no widget call needs another argument. Without a `put` nothing snaps.

use eframe::egui::{self, Context, Id, Response, Ui};

/// What the grid gives settings to snap to this frame: the grid square and the angle step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingSnap {
    /// One grid square, in video pixels.
    pub length: f64,
    /// The angle step, in degrees.
    pub angle: f64,
}

fn key() -> Id {
    Id::unique("setting snap")
}

/// Puts this frame's snap where the widgets read it: `None` when the Snap button is off.
pub fn put(ctx: &Context, snap: Option<SettingSnap>) {
    ctx.data_mut(|d| match snap {
        Some(s) => {
            d.insert_temp(key(), s);
        }
        None => d.remove::<SettingSnap>(key()),
    });
}

/// The snap settings take right now: none when it is off or Alt is held.
pub fn now(ui: &Ui) -> Option<SettingSnap> {
    if ui.input(|i| i.modifiers.alt) {
        return None;
    }
    ui.ctx().data(|d| d.get_temp::<SettingSnap>(key()))
}

/// What a setting measures, for snapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    /// A size or distance: whole squares, eighths of one under one square.
    Length,
    /// A place (a pair, or the Placement card's Position and Offset): whole squares.
    Place,
    /// An angle: whole angle steps.
    Angle,
}

impl Measure {
    /// What a setting shown in `unit` measures: `px` a length (a place for a `pair`), `°` or `deg`
    /// an angle, anything else (speeds, times, shares) nothing.
    pub fn of(unit: &str, pair: bool) -> Option<Measure> {
        match unit.trim() {
            "px" if pair => Some(Measure::Place),
            "px" => Some(Measure::Length),
            "°" | "deg" => Some(Measure::Angle),
            _ => None,
        }
    }
}

/// A setting's snapping: what it measures and the step it snaps by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapper {
    pub measure: Measure,
    pub step: f64,
}

impl Snapper {
    /// How a setting shown in `unit` snaps right now, if it does.
    pub fn for_unit(ui: &Ui, unit: &str, pair: bool) -> Option<Snapper> {
        let measure = Measure::of(unit, pair)?;
        now(ui).and_then(|s| Snapper::with(s, measure))
    }

    /// A place's snapping right now (Position, Offset).
    pub fn place(ui: &Ui) -> Option<Snapper> {
        now(ui).and_then(|s| Snapper::with(s, Measure::Place))
    }

    /// An angle's snapping right now (Rotation).
    pub fn angle(ui: &Ui) -> Option<Snapper> {
        now(ui).and_then(|s| Snapper::with(s, Measure::Angle))
    }

    fn with(snap: SettingSnap, measure: Measure) -> Option<Snapper> {
        let step = if measure == Measure::Angle { snap.angle } else { snap.length };
        (step.is_finite() && step > 0.0).then_some(Snapper { measure, step })
    }

    /// `v` on its nearest step.
    pub fn snap(&self, v: f64) -> f64 {
        match self.measure {
            Measure::Length => length(v, self.step),
            Measure::Place => place(v, self.step),
            Measure::Angle => angle(v, self.step),
        }
    }

    /// One step on from `v` (snapped first) in direction `dir` (its sign): an arrow key's move. A
    /// length moves by eighths of a square under one square, else by squares.
    pub fn nudge(&self, v: f64, dir: f64) -> f64 {
        let v = self.snap(v);
        let dir = dir.signum();
        let gap = match self.measure {
            Measure::Length => {
                let eighth = self.step / 8.0;
                let near = |x: f64| x.abs() < self.step - eighth * 0.5;
                if near(v) || near(v + dir * eighth) { eighth } else { self.step }
            }
            _ => self.step,
        };
        self.snap(v + dir * gap)
    }

    /// Whether a slider over `span` could stop at four or more of these steps: a length counts
    /// its eighths under one square.
    pub fn fits(&self, (a, b): (f64, f64)) -> bool {
        let finest = if self.measure == Measure::Length { self.step / 8.0 } else { self.step };
        (b - a).abs() / finest >= 4.0
    }

    /// How many decimals a number box needs to show this snapping's steps exactly (at most 3).
    pub fn decimals(&self) -> usize {
        let finest = if self.measure == Measure::Length { self.step / 8.0 } else { self.step };
        (0..3).find(|d| ((finest * 10f64.powi(*d as i32)).fract()).abs() < 1e-6).unwrap_or(3)
    }

    /// The gap between the ticks drawn under a rail from `lo` to `hi`, `width` points wide: the
    /// step, or none when they would be closer than 4 points or more than 200 of them.
    pub fn tick_gap(&self, lo: f64, hi: f64, width: f32) -> Option<f64> {
        let span = (hi - lo).abs();
        let count = span / self.step;
        (count > 0.0 && count <= 200.0 && width as f64 / count >= 4.0).then_some(self.step)
    }
}

/// A length on whole steps, or on eighths of a step under one step.
pub fn length(v: f64, step: f64) -> f64 {
    if v.abs() < step {
        let eighth = step / 8.0;
        (v / eighth).round() * eighth
    } else {
        (v / step).round() * step
    }
}

/// A place on whole steps.
pub fn place(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// An angle on whole steps.
pub fn angle(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

/// Snaps `value` when `response` was dragged (or let go) or clicked, or it has the keyboard focus and an arrow
/// key went down, this frame: an egui slider or number box keeps its own drag going, so snapping
/// what it wrote never holds it back. An arrow key moves one snapping step on from `before` the
/// way the widget moved it. Typing a number is left exact. Integers round; `range` (either way
/// round) clamps. True when it changed the value.
pub fn after_drag(
    ui: &Ui,
    response: &Response,
    before: f64,
    value: &mut f64,
    snapper: Option<Snapper>,
    integer: bool,
    range: Option<(f64, f64)>,
) -> bool {
    let Some(snapper) = snapper else { return false };
    let arrows = response.has_focus()
        && ui.input(|i| {
            [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::ArrowUp, egui::Key::ArrowDown]
                .iter()
                .any(|k| i.key_pressed(*k))
        });
    // An egui slider still follows the pointer on the frame the button comes up.
    let dragging = response.dragged() || response.drag_stopped();
    if !(dragging || response.clicked() || arrows) || !response.changed() {
        return false;
    }
    let mut v = if arrows && !response.dragged() && *value != before {
        snapper.nudge(before, *value - before)
    } else {
        snapper.snap(*value)
    };
    if integer {
        v = v.round();
    }
    if let Some((a, b)) = range {
        v = v.clamp(a.min(b), a.max(b));
    }
    let changed = v != *value;
    *value = v;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_land_on_squares_and_eighths_under_one() {
        assert_eq!(length(40.0, 60.0), 37.5);
        assert_eq!(length(89.0, 60.0), 60.0);
        assert_eq!(length(91.0, 60.0), 120.0);
        assert_eq!(length(-100.0, 60.0), -120.0);
        assert_eq!(length(4.0, 60.0), 7.5);
        assert_eq!(length(1.0, 60.0), 0.0);
        assert_eq!(length(59.0, 60.0), 60.0);
    }

    #[test]
    fn places_and_angles_land_on_whole_steps() {
        assert_eq!(place(29.0, 60.0), 0.0);
        assert_eq!(place(-31.0, 60.0), -60.0);
        assert_eq!(angle(44.0, 15.0), 45.0);
        assert_eq!(angle(-8.0, 15.0), -15.0);
    }

    #[test]
    fn only_lengths_places_and_angles_snap() {
        assert_eq!(Measure::of("px", false), Some(Measure::Length));
        assert_eq!(Measure::of(" px", false), Some(Measure::Length));
        assert_eq!(Measure::of("px", true), Some(Measure::Place));
        assert_eq!(Measure::of("°", false), Some(Measure::Angle));
        assert_eq!(Measure::of(" deg", false), Some(Measure::Angle));
        for unit in ["px/s", "px/s²", "deg/s", "°/s", "%", "s", "", "×"] {
            assert_eq!(Measure::of(unit, false), None, "{unit}");
        }
    }

    #[test]
    fn arrow_keys_move_a_step_at_a_time() {
        let length = Snapper { measure: Measure::Length, step: 60.0 };
        assert_eq!(length.nudge(60.0, -1.0), 52.5);
        assert_eq!(length.nudge(60.0, 1.0), 120.0);
        assert_eq!(length.nudge(52.5, 1.0), 60.0);
        assert_eq!(length.nudge(0.0, -1.0), -7.5);
        assert_eq!(length.nudge(121.0, 1.0), 180.0);
        let turn = Snapper { measure: Measure::Angle, step: 15.0 };
        assert_eq!(turn.nudge(44.0, 1.0), 60.0);
        assert_eq!(length.decimals(), 1, "7.5 shows as it is");
        assert_eq!(Snapper { measure: Measure::Length, step: 80.0 }.decimals(), 0);
        assert_eq!(turn.decimals(), 0);
        assert!(length.fits((0.5, 200.0)), "a ball's size snaps");
        assert!(!length.fits((0.25, 10.0)), "a 10 px width would only take 0 and 7.5");
        assert!(turn.fits((-180.0, 180.0)));
        assert!(!turn.fits((0.0, 20.0)), "a 20° sun only takes 0 and 15");
    }

    #[test]
    fn ticks_only_when_they_are_apart() {
        let s = Snapper { measure: Measure::Length, step: 60.0 };
        assert_eq!(s.tick_gap(0.0, 600.0, 200.0), Some(60.0));
        assert_eq!(s.tick_gap(0.0, 6000.0, 200.0), None, "100 ticks in 200 points is too tight");
        assert_eq!(s.tick_gap(0.0, 60000.0, 2000.0), None, "too many");
    }

    #[test]
    fn nothing_snaps_without_a_put_or_with_alt_held() {
        let ctx = Context::default();
        let mut seen = Vec::new();
        ctx.run_ui(Default::default(), |ui| seen.push(now(ui))).textures_delta.clear();
        put(&ctx, Some(SettingSnap { length: 60.0, angle: 15.0 }));
        ctx.run_ui(Default::default(), |ui| seen.push(now(ui))).textures_delta.clear();
        let alt =
            egui::RawInput { events: vec![egui::Event::ModifiersChanged(egui::Modifiers::ALT)], ..Default::default() };
        ctx.run_ui(alt, |ui| seen.push(now(ui))).textures_delta.clear();
        let free =
            egui::RawInput { events: vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)], ..Default::default() };
        ctx.run_ui(free, |_| {}).textures_delta.clear();
        put(&ctx, None);
        ctx.run_ui(Default::default(), |ui| seen.push(now(ui))).textures_delta.clear();
        assert_eq!(seen, vec![None, Some(SettingSnap { length: 60.0, angle: 15.0 }), None, None]);
    }
}
