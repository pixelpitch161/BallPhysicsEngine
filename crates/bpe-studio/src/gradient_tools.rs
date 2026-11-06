//! The gradient tools, in the Colours tab and the big editor: Flip, Spread evenly, Random and
//! Pick from picture, and the stop the big editor's Add colour makes. The Palette menu sits
//! beside the bar (`gradient_bar.rs`).

use bpe_core::{Color, GapBlend, Gradient, Mix, Stop, Value};
use bpe_kit::paint::palette_gen;
use bpe_kit::rng::Rng;
use eframe::egui::{self, Ui};

use crate::eyedropper;
use crate::gradient_bar::{gaps, select_stop, selected_stop};
use crate::gradient_tabs::name_it;
use crate::style;

/// What the tools did: whether the gradient changed, and after a Flip which old stop each new
/// one is (so a value kept per stop can follow, [`points_after_flip`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolsOutcome {
    pub changed: bool,
    pub flip: Option<Vec<usize>>,
}

impl ToolsOutcome {
    pub fn merge(&mut self, other: ToolsOutcome) {
        self.changed |= other.changed;
        if other.flip.is_some() {
            self.flip = other.flip;
        }
    }
}

/// The gradient turned end to end: the colour at `t` is the one that was at `1 − t` (round the
/// loop, when it loops). Each stop moves to `1 − at` (a looped one's stop at 0 stays there), and
/// the gap after it is the old gap before it, mirrored: Ease in and Ease out swap and the middle
/// goes to `1 − middle`. Unlooped, the old first stop (now the last) keeps the old last stop's
/// unused gap, so flipping twice gives the gradient back. Also gives which old stop each new
/// stop is.
///
/// The mirror is exact where each middle sits in the middle; an off-centre middle keeps its
/// halfway colour at the mirrored place, but the bend either side of it (a power curve) isn't
/// symmetric, so the colours between move a little.
pub fn flipped(g: &Gradient) -> (Gradient, Vec<usize>) {
    let n = g.stops.len();
    if n < 2 {
        return (g.clone(), (0..n).collect());
    }
    // A looped stop at the very end is one at 0, just before whatever sits there: put it first.
    let ends = g.stops.iter().rev().take_while(|s| g.looped && s.at >= 1.0).count();
    if ends > 0 && ends < n {
        let mut turned = g.clone();
        turned.stops.rotate_right(ends);
        for s in &mut turned.stops[..ends] {
            s.at = 0.0;
        }
        let (f, order) = flipped(&turned);
        // `turned`'s stop k is `g`'s stop (k + n − ends) % n.
        return (f, order.into_iter().map(|k| (k + n - ends) % n).collect());
    }
    let mirror = |b: GapBlend| match b {
        GapBlend::EaseIn => GapBlend::EaseOut,
        GapBlend::EaseOut => GapBlend::EaseIn,
        other => other,
    };
    let mut made: Vec<(Stop, usize)> = (0..n)
        .map(|k| {
            let old = g.stops[k];
            let at = if g.looped { (1.0 - old.at).rem_euclid(1.0) } else { 1.0 - old.at };
            let before = if k > 0 { Some(k - 1) } else { g.looped.then_some(n - 1) };
            let (blend, middle) = match before {
                Some(b) => (mirror(g.stops[b].blend), 1.0 - g.stops[b].middle),
                None => (g.stops[n - 1].blend, g.stops[n - 1].middle),
            };
            (Stop { at, color: old.color, blend, middle }, k)
        })
        .collect();
    // Backwards, then a stable sort by place: two stops sharing a place swap round, so the hard
    // edge between them still has the right colour on each side.
    made.reverse();
    made.sort_by(|a, b| a.0.at.total_cmp(&b.0.at));
    let order = made.iter().map(|(_, k)| *k).collect();
    let stops = made.into_iter().map(|(s, _)| s).collect();
    (Gradient { stops, ..g.clone() }, order)
}

/// A value kept per stop (a Points companion: a point for each stop) put in the stops' new
/// order after a Flip. `None` when it isn't one point per stop.
pub fn points_after_flip(value: &Value, order: &[usize]) -> Option<Value> {
    match value {
        Value::Points(ps) if ps.len() == order.len() && !ps.is_empty() => {
            Some(Value::Points(order.iter().map(|&k| ps[k]).collect()))
        }
        _ => None,
    }
}

/// The gradient with `colours` on its stops in order, everything else kept: places, gaps, Mix,
/// Loop and each stop's opacity.
pub fn recoloured(g: &Gradient, colours: &[Color]) -> Gradient {
    let mut out = g.clone();
    for (s, c) in out.stops.iter_mut().zip(colours) {
        s.color = Color { a: s.color.a, ..*c };
    }
    out
}

/// New colours that go together, one for each stop.
pub fn random_colours(stops: usize, seed: u32) -> Vec<Color> {
    let mut dice = Rng::new(seed, 0);
    palette_gen::palette(&mut dice, stops)
}

/// Adds a colour halfway along stop `selected`'s gap (or the gap before it when it has none),
/// coloured as the gradient is there and blending as the gap it splits. A one-stop gradient gets
/// white at the other end, mixing Even like the bar's "+". Gives the new stop's index.
pub fn add_colour(g: &mut Gradient, selected: usize) -> usize {
    let all = gaps(g);
    let gap = all
        .iter()
        .find(|(i, ..)| *i == selected)
        .or_else(|| all.iter().find(|(i, ..)| *i + 1 == selected))
        .or(all.first())
        .copied();
    let Some((i, start, width)) = gap else {
        // No gap with room: one stop, or every stop at one place.
        let at = if g.stops.first().is_some_and(|s| s.at >= 0.5) { 0.0 } else { 1.0 };
        if g.stops.len() <= 1 {
            g.mix = Mix::Even;
        }
        let k = g.stops.partition_point(|s| s.at <= at);
        g.stops.insert(k, Stop::new(at, Color::WHITE));
        return k;
    };
    let at = (start + width * 0.5).rem_euclid(1.0);
    let color = Color::from_linear(g.sample(at));
    let blend = g.stops[i].blend;
    let k = g.stops.partition_point(|s| s.at <= at);
    g.stops.insert(k, Stop { blend, ..Stop::new(at, color) });
    k
}

/// One tool button, named `name`, lit when `on`.
fn tool(ui: &mut Ui, text: &str, on: bool, name: String, help: &str) -> bool {
    let r = ui.add(style::toggle_button(text, on).min_size(egui::vec2(0.0, 22.0)));
    name_it(ui, &r, name);
    r.on_hover_text(help).clicked()
}

/// The tools in a wrapped line, their names after `name` ("Colours flip"); the selected stop is
/// the one the bar keeps under `memory`, and Pick from picture picks for that bar.
pub fn tools(ui: &mut Ui, g: &mut Gradient, memory: egui::Id, name: &str) -> ToolsOutcome {
    let mut out = ToolsOutcome::default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if tool(ui, "Flip", false, format!("{name} flip"), "Turn the colours end to end") {
            let selected = selected_stop(ui.ctx(), memory, g);
            let (next, order) = flipped(g);
            // The selected stop stays selected where it went.
            if let Some(k) = order.iter().position(|&k| k == selected) {
                select_stop(ui.ctx(), memory, k);
            }
            out.changed = next != *g;
            *g = next;
            out.flip = Some(order);
        }
        let help = "Space the colours out evenly (round the loop, when it loops)";
        if tool(ui, "Spread evenly", false, format!("{name} spread evenly"), help) {
            let before = g.clone();
            g.spread_evenly();
            out.changed |= *g != before;
        }
        let help = "New colours that go together, keeping where each colour sits and how it blends";
        if tool(ui, "Random", false, format!("{name} random"), help) {
            let colours = random_colours(g.stops.len(), palette_gen::fresh_seed());
            let next = recoloured(g, &colours);
            out.changed |= next != *g;
            *g = next;
        }
        let picking = eyedropper::target(ui.ctx()) == Some(memory);
        let help = if picking {
            "Click the preview to pick the colour there, or click here to stop"
        } else {
            "Click the preview to set the selected colour to what is drawn there"
        };
        if tool(ui, "Pick from picture", picking, format!("{name} pick from picture"), help) {
            if picking {
                eyedropper::stop(ui.ctx());
            } else {
                eyedropper::start(ui.ctx(), memory);
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 128 };
    const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };
    const WHITE: Color = Color::WHITE;

    fn stop(at: f32, color: Color, blend: GapBlend, middle: f32) -> Stop {
        Stop { at, color, blend, middle }
    }

    /// Red at 0, green at 0.3 (Ease in, middle 0.7), blue at 0.8 (Steps, middle 0.25), white at 1
    /// (Eased).
    fn four(mix: Mix, looped: bool) -> Gradient {
        let stops = vec![
            stop(0.0, RED, GapBlend::Smooth, 0.5),
            stop(0.3, GREEN, GapBlend::EaseIn, 0.7),
            stop(0.8, BLUE, GapBlend::Steps, 0.25),
            stop(if looped { 0.9 } else { 1.0 }, WHITE, GapBlend::Eased, 0.4),
        ];
        Gradient { stops, mix, looped, grain: 0.0, moves: Default::default() }
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn flip_turns_an_unlooped_gradient_end_to_end() {
        let g = four(Mix::Even, false);
        let (f, order) = flipped(&g);
        assert_eq!(order, vec![3, 2, 1, 0]);
        let ats: Vec<f32> = f.stops.iter().map(|s| s.at).collect();
        assert!(ats.iter().zip([0.0, 0.2, 0.7, 1.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{ats:?}");
        assert_eq!(f.stops.iter().map(|s| s.color).collect::<Vec<_>>(), vec![WHITE, BLUE, GREEN, RED]);
        // White's gap is the old blue gap (Steps, middle 0.25 → 0.75); blue's is the old green one
        // (Ease in → Ease out, 0.7 → 0.3); green's the old red one.
        assert_eq!((f.stops[0].blend, f.stops[0].middle), (GapBlend::Steps, 0.75));
        assert_eq!(f.stops[1].blend, GapBlend::EaseOut);
        assert!((f.stops[1].middle - 0.3).abs() < 1e-6);
        assert_eq!((f.stops[2].blend, f.stops[2].middle), (GapBlend::Smooth, 0.5));
        assert_eq!((f.mix, f.looped), (Mix::Even, false));
    }

    #[test]
    fn a_flipped_gradient_reads_backwards() {
        // With every middle in the middle the mirror is exact for every blend and mix.
        for mix in [Mix::Light, Mix::Plain, Mix::Even, Mix::RoundTheWheel, Mix::LongWayRound] {
            let mut g = four(mix, false);
            for s in &mut g.stops {
                s.middle = 0.5;
            }
            let (f, _) = flipped(&g);
            for k in 0..=100 {
                let t = k as f32 / 100.0;
                // Skip the hard edge itself, where either side is right.
                if (t - 0.1).abs() < 0.011 {
                    continue;
                }
                assert!(close(f.sample(t), g.sample(1.0 - t)), "{mix:?} at {t}");
            }
        }
        // An off-centre middle keeps its halfway colour at the mirrored place.
        let g = four(Mix::Even, false);
        let (f, _) = flipped(&g);
        let half = 0.3 + 0.7 * 0.5;
        assert!(close(f.sample(1.0 - half), g.sample(half)));
    }

    #[test]
    fn a_looped_flip_reads_backwards_round_the_loop() {
        let mut g = four(Mix::Light, true);
        for s in &mut g.stops {
            s.middle = 0.5;
            if s.blend == GapBlend::Steps {
                s.blend = GapBlend::Smooth;
            }
        }
        let (f, order) = flipped(&g);
        assert_eq!(f.stops[0].at, 0.0, "the stop at 0 stays there");
        assert_eq!(order, vec![0, 3, 2, 1]);
        for k in 0..100 {
            let t = k as f32 / 100.0;
            assert!(close(f.sample(t), g.sample((-t).rem_euclid(1.0))), "at {t}");
        }
    }

    #[test]
    fn flipping_twice_gives_the_gradient_back() {
        for looped in [false, true] {
            let g = four(Mix::RoundTheWheel, looped);
            let (once, _) = flipped(&g);
            assert_ne!(once, g);
            let (twice, _) = flipped(&once);
            assert_eq!(twice.stops.len(), g.stops.len());
            for (a, b) in twice.stops.iter().zip(&g.stops) {
                assert!((a.at - b.at).abs() < 1e-6 && (a.middle - b.middle).abs() < 1e-6, "{a:?} {b:?}");
                assert_eq!((a.color, a.blend), (b.color, b.blend));
            }
        }
    }

    #[test]
    fn two_stops_at_one_place_keep_their_sides() {
        // A hard edge at 0.5: red before, blue after. Flipped, blue is before and red after.
        let g = Gradient::even(&[RED, RED, BLUE, BLUE]);
        let mut g = g;
        g.stops[1].at = 0.5;
        g.stops[2].at = 0.5;
        let (f, _) = flipped(&g);
        assert_eq!(Color::from_linear(f.sample(0.25)), BLUE);
        assert_eq!(Color::from_linear(f.sample(0.75)), RED);
    }

    #[test]
    fn a_looped_stop_at_the_end_flips_as_the_one_before_the_start() {
        // Red at 0, green at 0.5, blue at 1: blue sits just before red round the loop.
        let mut g = Gradient::spread(&[RED, GREEN, BLUE], Mix::Light, true);
        g.stops[1].at = 0.5;
        g.stops[2].at = 1.0;
        let (f, order) = flipped(&g);
        assert_eq!(order.len(), 3);
        for k in 1..100 {
            let t = k as f32 / 100.0;
            assert!(close(f.sample(t), g.sample(1.0 - t)), "at {t}: {f:?}");
        }
        let mut seen = order.clone();
        seen.sort();
        assert_eq!(seen, vec![0, 1, 2], "every old stop once");
        assert_eq!(f.stops[order.iter().position(|&k| k == 1).unwrap()].color, GREEN);
    }

    #[test]
    fn points_follow_their_stops() {
        let points = Value::Points(vec![[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]);
        assert_eq!(
            points_after_flip(&points, &[2, 0, 1]),
            Some(Value::Points(vec![[2.0, 2.0], [0.0, 0.0], [1.0, 1.0]]))
        );
        assert_eq!(points_after_flip(&points, &[1, 0]), None, "not one a stop");
        assert_eq!(points_after_flip(&Value::Number(1.0), &[0]), None);
    }

    #[test]
    fn random_colours_keep_everything_else() {
        let g = four(Mix::Plain, true);
        let colours = random_colours(4, 7);
        assert_eq!(colours.len(), 4);
        let r = recoloured(&g, &colours);
        for ((a, b), c) in r.stops.iter().zip(&g.stops).zip(&colours) {
            assert_eq!((a.at, a.blend, a.middle), (b.at, b.blend, b.middle));
            assert_eq!((a.color.r, a.color.g, a.color.b), (c.r, c.g, c.b));
            assert_eq!(a.color.a, b.color.a, "opacity kept");
        }
        assert_eq!((r.mix, r.looped), (Mix::Plain, true));
    }

    #[test]
    fn add_colour_splits_the_selected_gap() {
        let mut g = Gradient::even(&[RED, BLUE]);
        g.stops[0].blend = GapBlend::Eased;
        let k = add_colour(&mut g, 0);
        assert_eq!(k, 1);
        assert_eq!(g.stops.len(), 3);
        assert!((g.stops[1].at - 0.5).abs() < 1e-6);
        assert_eq!(g.stops[1].blend, GapBlend::Eased, "blends as the gap it split");
        // The last stop of an unlooped gradient has no gap: the one before it is split.
        let k = add_colour(&mut g, 2);
        assert_eq!(k, 2);
        assert!((g.stops[2].at - 0.75).abs() < 1e-6);
        // Looped, the last stop's gap runs back round through 1 = 0.
        let mut l = Gradient::spread(&[RED, BLUE], Mix::Even, true);
        let k = add_colour(&mut l, 1);
        assert!((l.stops[k].at - 0.75).abs() < 1e-6, "{l:?}");
        // One stop: white at the other end, mixing Even.
        let mut one = Gradient::solid(RED);
        assert_eq!(add_colour(&mut one, 0), 1);
        assert_eq!(one.stops[1], Stop::new(1.0, WHITE));
        assert_eq!(one.mix, Mix::Even);
    }
}
