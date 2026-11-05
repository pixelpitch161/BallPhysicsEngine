//! A shape's Distance zones in the preview: while its Sound tab is open, each Distance that is
//! on shows its Near and Far as dotted lines in the shape's colour (circles round the middle
//! for Centre, the outline grown and shrunk for Edge). Dragging a line sets Near or Far.

use super::{Handle, View};
use crate::app::Studio;
use crate::inspector::InspectorTab;
use crate::style;
use bpe_core::{LayerId, Params, Scene, Settings, Slot, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{DISTANCE, DistanceSettings, Measure};
use bpe_runtime::{Outline, World};
use eframe::egui::{self, Pos2, Shape};
use glam::Vec2;
use std::f32::consts::TAU;

/// How close to a line, in screen points, counts as grabbing it.
const LINE_GRAB: f32 = 6.0;

/// One Distance's zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub shape: ThingId,
    pub layer: LayerId,
    pub measure: Measure,
    pub near: f32,
    pub far: f32,
}

/// The zones shown: the selected shape's Distances that are on, while its Sound tab is open.
pub fn zones(studio: &Studio) -> Vec<Zone> {
    let Some(shape) = studio.selection.primary else { return Vec::new() };
    if studio.panels.inspector.tab.get(&shape) != Some(&InspectorTab::Sound) {
        return Vec::new();
    }
    let Some(card) = studio.scene.card(shape, SlotId::Sound) else { return Vec::new() };
    let reg = &studio.kit.registry;
    let time = studio.sim_time();
    card.layers
        .iter()
        .filter(|l| l.kind == DISTANCE && l.enabled)
        .map(|l| {
            let s = settings_of(card, l.id);
            let at = |key: &str, set: f32| {
                let a = bpe_core::Address::on(shape, SlotId::Sound, &layered_key(l.id, key));
                match studio.scene.value_at_time(reg, &a, time) {
                    Some(Value::Number(v)) => v as f32,
                    _ => set,
                }
            };
            Zone { shape, layer: l.id, measure: s.measure, near: at("near", s.near), far: at("far", s.far) }
        })
        .collect()
}

/// Distance `layer`'s settings as set on the card.
fn settings_of(card: &Slot, layer: LayerId) -> DistanceSettings {
    let prefix = format!("{layer}/");
    let own = card.params.iter().filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_owned(), v.clone())));
    DistanceSettings::from_params(&Params(own.collect()))
}

/// The shape's outline and middle as the world has them now.
fn outline(world: &World, shape: ThingId) -> Option<&Outline> {
    world.walls.get(&shape).map(|w| &w.outline)
}

/// The lines `d` pixels from the shape, in world pixels: one circle round the middle for Centre;
/// for Edge every line where the distance to the outline is `d`, inside and out (for a hollow
/// circle the two circles, for any other outline traced from the real distance).
fn lines(o: &Outline, measure: Measure, d: f32) -> Vec<Vec<Vec2>> {
    let circle = |c: Vec2, r: f32| (0..=96).map(|i| c + Vec2::from_angle(i as f32 / 96.0 * TAU) * r).collect();
    match (measure, o) {
        (Measure::Centre, _) => vec![circle(o.center(), d)],
        (Measure::Edge, &Outline::Ring { center, radius, sides: 0..3, .. }) => {
            let mut out = vec![circle(center, radius + d)];
            if d > 0.0 && d < radius {
                out.push(circle(center, radius - d));
            }
            out
        }
        (Measure::Edge, _) => edge_lines(o, d),
    }
}

/// How many cells the Edge field has along its longer side.
const FIELD_CELLS: usize = 144;

/// The lines where the distance to `o`'s outline is `d`, traced over a grid of the real
/// distance (marching squares) and joined into runs.
fn edge_lines(o: &Outline, d: f32) -> Vec<Vec<Vec2>> {
    let reach = o.reach() + d + 8.0;
    let (lo, side) = (o.center() - Vec2::splat(reach), 2.0 * reach);
    let n = FIELD_CELLS;
    let cell = side / n as f32;
    let at = |i: usize, j: usize| lo + Vec2::new(i as f32, j as f32) * cell;
    let field: Vec<f32> =
        (0..=n).flat_map(|j| (0..=n).map(move |i| (i, j))).map(|(i, j)| measured(o, Measure::Edge, at(i, j))).collect();
    contours(&field, n, d, at)
}

/// Marching squares over `field` ((n + 1)² values, rows of n + 1) at `level`: the crossing
/// lines, each a run of points (closed runs end where they start). `at` places grid point (i, j).
fn contours(field: &[f32], n: usize, level: f32, at: impl Fn(usize, usize) -> Vec2) -> Vec<Vec<Vec2>> {
    use std::collections::HashMap;
    let f = |i: usize, j: usize| field[j * (n + 1) + i] - level;
    // A crossing is named by its grid edge: 2·(corner index) for the edge to the right of the
    // corner, +1 for the edge below it.
    let corner = |i: usize, j: usize| j * (n + 1) + i;
    let right = |i: usize, j: usize| 2 * corner(i, j);
    let down = |i: usize, j: usize| 2 * corner(i, j) + 1;
    let point = |e: usize| {
        let (c, below) = (e / 2, e % 2 == 1);
        let (i, j) = (c % (n + 1), c / (n + 1));
        let (i2, j2) = if below { (i, j + 1) } else { (i + 1, j) };
        let (a, b) = (f(i, j), f(i2, j2));
        let t = if (a - b).abs() > 1e-12 { (a / (a - b)).clamp(0.0, 1.0) } else { 0.5 };
        at(i, j).lerp(at(i2, j2), t)
    };
    let mut links: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut link = |a: usize, b: usize| {
        links.entry(a).or_default().push(b);
        links.entry(b).or_default().push(a);
    };
    for j in 0..n {
        for i in 0..n {
            let (a, b, c, d) = (f(i, j), f(i + 1, j), f(i + 1, j + 1), f(i, j + 1));
            let case = (a > 0.0) as u8 | ((b > 0.0) as u8) << 1 | ((c > 0.0) as u8) << 2 | ((d > 0.0) as u8) << 3;
            // The cell's sides: top, right, bottom, left.
            let (top, rgt, bot, lft) = (right(i, j), down(i + 1, j), right(i, j + 1), down(i, j));
            match case {
                0 | 15 => {}
                1 | 14 => link(lft, top),
                2 | 13 => link(top, rgt),
                3 | 12 => link(lft, rgt),
                4 | 11 => link(rgt, bot),
                6 | 9 => link(top, bot),
                7 | 8 => link(lft, bot),
                5 | 10 => {
                    // A saddle: the middle decides which corners join.
                    let mid_high = (a + b + c + d) / 4.0 > 0.0;
                    if (case == 5) == mid_high {
                        link(lft, bot);
                        link(top, rgt);
                    } else {
                        link(lft, top);
                        link(rgt, bot);
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    // Walk the links into runs, starting from the open ends first so they come out whole.
    let mut starts: Vec<usize> = links.iter().filter(|(_, l)| l.len() == 1).map(|(e, _)| *e).collect();
    starts.sort_unstable();
    let mut rest: Vec<usize> = links.keys().copied().collect();
    rest.sort_unstable();
    starts.extend(rest);
    let mut used = std::collections::HashSet::new();
    let mut runs = Vec::new();
    for s in starts {
        if used.contains(&s) {
            continue;
        }
        let mut run = vec![point(s)];
        used.insert(s);
        let (mut prev, mut cur) = (usize::MAX, s);
        loop {
            let next = links[&cur].iter().copied().find(|e| *e != prev && !used.contains(e));
            match next {
                Some(e) => {
                    used.insert(e);
                    run.push(point(e));
                    (prev, cur) = (cur, e);
                }
                None => {
                    // A closed run joins back to where it began.
                    if links[&cur].contains(&s) && run.len() > 2 {
                        run.push(run[0]);
                    }
                    break;
                }
            }
        }
        if run.len() > 1 {
            runs.push(run);
        }
    }
    runs
}

/// A traced zone line: the outline, how it measures, the distance, and the runs of points.
type Traced = (Outline, Measure, f32, Vec<Vec<Vec2>>);

thread_local! {
    /// The last lines traced for each zone line, kept while the shape and the distance stay put.
    static TRACED: std::cell::RefCell<Vec<Traced>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `o` in its own frame (middle at 0, not turned), with where its middle is and how far it is
/// turned: a shape that moves or spins keeps the same lines.
fn own_frame(o: &Outline) -> (Outline, Vec2, f32) {
    let mut own = o.clone();
    let (center, angle) = match &mut own {
        Outline::Ring { center, angle, .. }
        | Outline::Box { center, angle, .. }
        | Outline::Ellipse { center, angle, .. }
        | Outline::Path { center, angle, .. } => (std::mem::take(center), std::mem::take(angle)),
    };
    (own, center, angle)
}

/// [`lines`], reusing the last tracing of the same outline (wherever it is and however it is
/// turned) at the same distance.
fn lines_kept(o: &Outline, measure: Measure, d: f32) -> Vec<Vec<Vec2>> {
    let (own, center, angle) = own_frame(o);
    let turn = Vec2::from_angle(angle);
    let place = |runs: &[Vec<Vec2>]| -> Vec<Vec<Vec2>> {
        runs.iter().map(|r| r.iter().map(|p| center + turn.rotate(*p)).collect()).collect()
    };
    TRACED.with(|t| {
        let mut t = t.borrow_mut();
        if let Some((.., kept)) = t.iter().find(|(ko, km, kd, _)| *ko == own && *km == measure && *kd == d) {
            return place(kept);
        }
        let made = lines(&own, measure, d);
        if t.len() >= 16 {
            t.remove(0);
        }
        let placed = place(&made);
        t.push((own, measure, d, made));
        placed
    })
}

/// How far `p` is from the shape as a zone of `measure` measures.
fn measured(o: &Outline, measure: Measure, p: Vec2) -> f32 {
    match measure {
        Measure::Centre => (p - o.center()).length(),
        Measure::Edge => (p - o.nearest(p).0).length(),
    }
}

/// Draws the zones.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View) {
    let world = studio.playback.world();
    for z in zones(studio) {
        let (Some(o), Some(v)) = (outline(world, z.shape), view.of(studio, z.shape)) else { continue };
        let colour = style::colour_of(&studio.scene, z.shape);
        for (d, alpha) in [(z.near, 1.0), (z.far, 0.6)] {
            if d <= 0.0 {
                // At the middle (Centre) or on the outline itself (Edge): nothing to draw.
                continue;
            }
            for run in lines_kept(o, z.measure, d) {
                let pts: Vec<Pos2> = run.into_iter().map(|p| v.to_screen(p)).collect();
                painter.extend(Shape::dotted_line(&pts, colour.gamma_multiply(alpha), 6.0, 1.3));
            }
        }
    }
}

/// The zone line under `pos`, if any: Far before Near when they sit together.
pub fn hit(studio: &Studio, view: &View, pos: Pos2) -> Option<Handle> {
    let world = studio.playback.world();
    zones(studio).into_iter().find_map(|z| {
        let (o, v) = (outline(world, z.shape)?, view.of(studio, z.shape)?);
        let d = measured(o, z.measure, v.to_world(pos));
        let reach = LINE_GRAB / v.world_scale();
        // A line that isn't drawn (at the middle, or on the outline) can't be grabbed.
        let drawn = |at: f32| at > 0.0;
        if drawn(z.far) && (d - z.far).abs() <= reach {
            Some(Handle::Zone(z.shape, z.layer, true))
        } else if drawn(z.near) && (d - z.near).abs() <= reach {
            Some(Handle::Zone(z.shape, z.layer, false))
        } else {
            None
        }
    })
}

/// A zone line dragged to `pointer` (world pixels): Far (or Near) becomes its distance, as a key
/// at `time` when the setting has keys. Returns whether it keyed.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    world: &World,
    shape: ThingId,
    layer: LayerId,
    far: bool,
    pointer: Vec2,
    time: f64,
) -> bool {
    let Some(o) = outline(world, shape) else { return false };
    let Some(card) = scene.card(shape, SlotId::Sound) else { return false };
    let measure = settings_of(card, layer).measure;
    let d = measured(o, measure, pointer).round().clamp(0.0, 4000.0) as f64;
    let a = bpe_core::Address::on(shape, SlotId::Sound, &layered_key(layer, if far { "far" } else { "near" }));
    let mut keyed = false;
    crate::inspector::set_or_key(scene, reg, &a, Value::Number(d), time, &mut keyed);
    keyed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Outline {
        Outline::Box { center: Vec2::new(300.0, 400.0), half: Vec2::new(200.0, 200.0), angle: 0.3, thickness: 4.0 }
    }

    #[test]
    fn edge_lines_sit_at_their_distance_inside_and_out() {
        let o = square();
        let runs = lines(&o, Measure::Edge, 50.0);
        assert_eq!(runs.len(), 2, "one line outside the square and one inside");
        let cell = 2.0 * (o.reach() + 58.0) / FIELD_CELLS as f32;
        for run in &runs {
            assert_eq!(run.first(), run.last(), "each line closes");
            for p in run {
                let d = measured(&o, Measure::Edge, *p);
                assert!((d - 50.0).abs() < cell * 0.5, "{d}");
            }
        }
        // The outer one goes all the way round: it reaches every side of the square.
        let outer = runs.iter().max_by(|a, b| a.len().cmp(&b.len())).unwrap();
        for k in 0..8 {
            let dir = Vec2::from_angle(k as f32 / 8.0 * TAU);
            assert!(outer.iter().any(|p| (*p - o.center()).normalize().dot(dir) > 0.99), "direction {k}");
        }
    }

    #[test]
    fn an_edge_line_deeper_than_the_shape_is_only_outside() {
        let runs = lines(&square(), Measure::Edge, 260.0);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn a_round_shape_has_two_circles() {
        let o = Outline::Ring { center: Vec2::ZERO, radius: 300.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let runs = lines(&o, Measure::Edge, 40.0);
        assert_eq!(runs.len(), 2);
        let radii: Vec<f32> = runs.iter().map(|r| r[10].length()).collect();
        assert!((radii[0] - 340.0).abs() < 0.01 && (radii[1] - 260.0).abs() < 0.01, "{radii:?}");
        let centre = lines(&o, Measure::Centre, 40.0);
        assert_eq!(centre.len(), 1);
        assert!((centre[0][5].length() - 40.0).abs() < 0.01);
    }

    #[test]
    fn a_free_outline_is_traced_too() {
        // An L shape: concave, so its inside line has a corner going the other way.
        let pts: Vec<Vec2> = [(-1.0, -1.0), (0.0, -1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|&(x, y)| Vec2::new(x, y) * 0.5)
            .collect();
        let o = Outline::Path { center: Vec2::ZERO, angle: 0.0, scale: 400.0, points: pts.into(), thickness: 4.0 };
        let runs = lines(&o, Measure::Edge, 30.0);
        assert_eq!(runs.len(), 2);
        let cell = 2.0 * (o.reach() + 38.0) / FIELD_CELLS as f32;
        assert!(runs.iter().flatten().all(|p| (measured(&o, Measure::Edge, *p) - 30.0).abs() < cell * 0.5));
    }
}
