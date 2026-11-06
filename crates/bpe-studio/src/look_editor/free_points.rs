//! Free points on the ball view (gradients part 2): a dot for each stop of the picked layer's
//! gradients laid out as Free points, in the stop's colour, dragged to move the stop's point.
//! A drag writes the whole `~points` list, one point per stop.

use bpe_core::gradient_extras::{self, key};
use bpe_core::gradient_form::{Layout, defaults, free_points, turned};
use bpe_core::{Area, GradientForm, SettingDef, SettingValue, Value, ValueType};
use glam::Vec2;

/// One gradient of a layer laid out as Free points, and how its form's space sits on the ball
/// (or on a shape, in shape units, or in the world for a paint following the place).
#[derive(Clone, Debug, PartialEq)]
pub struct FreeOn {
    /// The gradient's key in the layer (`"colour"`, `"in_colours"`).
    pub key: &'static str,
    /// Each point (form space) and its stop's colour.
    pub points: Vec<(Vec2, bpe_core::Color)>,
    turn: f32,
    centre: Vec2,
    /// Ball radii per form unit.
    size: f32,
    /// Measured from a paint's Centre in Spans (in the world), not on the ball: the ball view
    /// shows no dots for it.
    pub placed: bool,
}

impl FreeOn {
    /// Form point `q` on the ball (radii, before the ball turns).
    pub fn to_ball(&self, q: Vec2) -> Vec2 {
        self.centre + turned(q * self.size, self.turn)
    }

    /// Ball point `b` in the form's space.
    pub fn ball_to_form(&self, b: Vec2) -> Vec2 {
        turned(b - self.centre, -self.turn) / self.size.max(1e-6)
    }

    /// The whole points list with point `i` moved to ball point `b` (to hundredths).
    pub fn moved(&self, i: usize, b: Vec2) -> Value {
        let q = self.ball_to_form(b).clamp(Vec2::splat(-4.0), Vec2::splat(4.0));
        let round = |x: f32| (x * 100.0).round() / 100.0;
        Value::Points(
            self.points
                .iter()
                .enumerate()
                .map(|(k, (p, _))| if k == i { [round(q.x), round(q.y)] } else { [p.x, p.y] })
                .collect(),
        )
    }
}

/// The gradients among a layer's settings `defs` (values through `get`, by key, companions too)
/// that are laid out as Free points.
pub fn free_on(defs: &'static [SettingDef], get: &dyn Fn(&str) -> Option<Value>) -> Vec<FreeOn> {
    let number = |k: &str, or: f32| get(k).and_then(|v| f32::from_value(&v)).filter(|x| x.is_finite()).unwrap_or(or);
    let mut out = Vec::new();
    for d in defs.iter().filter(|d| d.ty == ValueType::Gradient && d.flags.area.is_area()) {
        let Some(Value::Gradient(g)) = get(d.key) else { continue };
        if g.stops.len() < 2 {
            continue;
        }
        let form =
            get(&key(d.key, "form")).and_then(|v| GradientForm::from_value(&v)).unwrap_or(d.flags.area.default_form());
        if form != GradientForm::FreePoints {
            continue;
        }
        let layout: Layout = gradient_extras::layout_from(d.key, get);
        let mut in_world = false;
        let (turn, centre, size) = match d.flags.area {
            // Placed by the gradient's own Turn, Centre and Size.
            Area::Fills(_) => (
                layout.turn.unwrap_or(defaults::TURN),
                layout.centre.map_or(Vec2::ZERO, Vec2::from),
                layout.size.filter(|s| *s > 1e-6).unwrap_or(defaults::SIZE),
            ),
            // Placed by the paint it belongs to: its Angle, and its Size when it has one (inside
            // a ball); a paint following the place measures from its Centre in Spans.
            _ => {
                let prefix = d.key.strip_suffix("colours").unwrap_or("");
                let angle = number(&format!("{prefix}angle"), 90.0);
                let follow = get(&format!("{prefix}follow"));
                let placed = matches!(&follow, Some(Value::Choice(f)) if f == "place" || f == "start");
                if placed {
                    if matches!(get(&format!("{prefix}space")), Some(Value::Choice(s)) if s == "screen") {
                        // Measured on the screen: no dots in the world.
                        continue;
                    }
                    let centre = match get(&format!("{prefix}centre")) {
                        Some(Value::Vec2(c)) => Vec2::from(c),
                        _ => Vec2::ZERO,
                    };
                    in_world = true;
                    (angle, centre, number(&format!("{prefix}span"), 1000.0).max(1e-3))
                } else {
                    (angle, Vec2::ZERO, 1.0 / number(&format!("{prefix}size"), 1.0).max(1e-3))
                }
            }
        };
        let points = free_points(&g, &layout)
            .into_iter()
            .map(|(p, c)| (Vec2::from(p), bpe_core::Color::from_linear(c)))
            .collect();
        out.push(FreeOn { key: d.key, points, turn, centre, size, placed: in_world });
    }
    out
}

/// [`free_on`] from a layer's params as they stand (`p`, companions included).
#[cfg(test)]
pub fn free_on_params(defs: &'static [SettingDef], p: &bpe_core::Params) -> Vec<FreeOn> {
    free_on(defs, &|k| p.get(k).cloned().or_else(|| defs.iter().find(|d| d.key == k).map(|d| d.default.clone())))
}

/// The names of the dots: "Point 1" to "Point 16".
pub const NAMES: [&str; 16] = [
    "Point 1", "Point 2", "Point 3", "Point 4", "Point 5", "Point 6", "Point 7", "Point 8", "Point 9", "Point 10",
    "Point 11", "Point 12", "Point 13", "Point 14", "Point 15", "Point 16",
];

/// The key a drag of `f`'s dots writes.
pub fn points_key(f: &FreeOn) -> String {
    key(f.key, "points")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Gradient, Settings};
    use bpe_kit::modules::look::layers::OutlineLayer;

    #[test]
    fn an_outline_laid_out_as_free_points_has_a_dot_per_stop_and_a_drag_writes_them_all() {
        let defs = OutlineLayer::defs();
        let mut p = bpe_core::Params::default();
        p.set("colour_from", Value::Choice("own".into()));
        p.set("colour", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE, Color::rgb(0, 0, 255)])));
        assert!(free_on_params(defs, &p).is_empty(), "Straight: no dots");
        p.set("colour~form", Value::Choice("free_points".into()));
        let found = free_on_params(defs, &p);
        assert_eq!(found.len(), 1);
        let f = &found[0];
        assert_eq!((f.key, f.points.len()), ("colour", 3));
        // The first point sits at the top, 0.6 up, by default; the form turns 90° on the ball.
        let top = f.to_ball(f.points[0].0);
        assert!((top - f.to_ball(Vec2::new(0.0, -0.6))).length() < 1e-5);
        let Value::Points(moved) = f.moved(1, f.to_ball(Vec2::new(0.25, 0.5))) else { panic!() };
        assert_eq!(moved.len(), 3, "every stop keeps a point");
        assert_eq!(moved[1], [0.25, 0.5]);
        assert_eq!(points_key(f), "colour~points");
    }
}
