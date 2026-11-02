//! Drawing a Meter: reads its canvas's numbers from the log at the drawn step and lays out a
//! split bar, a ring or a ranking with the renderer's filled box and arc band shapes.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bpe_core::{Settings, ThingId};
use bpe_gfx::text::{TextAlign, TextStyle, layout_text, measure_text};
use bpe_gfx::{DrawCx, Frame, ShapeInstance, ThingDraw, blend_alpha};
use bpe_runtime::{CanvasNumbers, Log};
use glam::Vec2;

use super::{MeterLook, MeterReads, MeterSettings};

pub struct MeterDraw;

/// A clip wide enough to show all of a box.
const ALL: [f32; 2] = [-1e9, 1e9];
/// Pieces narrower than this (in px) get no label inside a split bar.
const LABEL_MIN_WIDTH: f32 = 90.0;

/// One value on the meter: a team's share, or the single number the meter reads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Piece {
    pub name: String,
    /// 0 to 1.
    pub value: f32,
    /// Linear light.
    pub colour: [f32; 4],
}

impl ThingDraw for MeterDraw {
    fn kind(&self) -> &'static str {
        "meter"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let s = MeterSettings::from_params(&cx.resolved.params);
        let Some(canvas) = cx
            .world
            .order()
            .iter()
            .copied()
            .find(|&id| cx.world.kind_of(id) == Some("canvas") && cx.world.matches(cx.thing.id, id, &s.canvas))
        else {
            return;
        };
        let rate = cx.world.steps_per_second.max(1);
        let last_step = (cx.time * rate as f64 + 1e-6).floor().max(0.0) as u64;
        let window = (s.smooth.clamp(0.0, 1.0) * 0.5 * rate as f32).round() as u64;
        let numbers = averaged(cx.log, canvas, last_step, window);
        let pieces = pieces(&s, &numbers, cx.log, canvas);
        let single = s.reads != MeterReads::Shares;
        let at = cx.thing.transform.position;
        match s.look {
            MeterLook::SplitBar => split_bar(frame, &s, at, &pieces, single),
            MeterLook::Ring => ring(frame, &s, at, &pieces, single),
            MeterLook::Ranking => ranking(frame, &s, at, &pieces, single),
        }
    }
}

/// `canvas`'s numbers at run step `last_step`, averaged over the `window` steps up to it (just the
/// latest sample at or before it when `window` is 0 or the window holds none). Shares average per
/// team, a missing share counting 0. Nothing sampled at all reads 0 everywhere.
pub(crate) fn averaged(log: &Log, canvas: ThingId, last_step: u64, window: u64) -> CanvasNumbers {
    let all = log.canvas_samples();
    let high = all.partition_point(|x| x.step <= last_step);
    // The first sample with step > last_step - window.
    let low = all.partition_point(|x| x.step + window <= last_step);
    let inside: Vec<&CanvasNumbers> =
        all[low.min(high)..high].iter().filter(|x| x.canvas == canvas).map(|x| &x.numbers).collect();
    if window == 0 || inside.is_empty() {
        return log.canvas_at(canvas, last_step).cloned().unwrap_or_default();
    }
    let n = inside.len() as f32;
    let teams = inside.iter().map(|x| x.shares.len()).max().unwrap_or(0);
    CanvasNumbers {
        covered: inside.iter().map(|x| x.covered).sum::<f32>() / n,
        revealed: inside.iter().map(|x| x.revealed).sum::<f32>() / n,
        wet: inside.iter().map(|x| x.wet).sum::<f32>() / n,
        shares: (0..teams).map(|i| inside.iter().map(|x| x.share(i)).sum::<f32>() / n).collect(),
    }
}

/// What the meter shows: one piece per team for Shares (in team order), else a single piece.
pub(crate) fn pieces(s: &MeterSettings, numbers: &CanvasNumbers, log: &Log, canvas: ThingId) -> Vec<Piece> {
    let single = |name: &str, value: f32| {
        vec![Piece { name: name.into(), value: value.clamp(0.0, 1.0), colour: s.fill_colour.to_linear() }]
    };
    match s.reads {
        MeterReads::Shares => log
            .teams_of(canvas)
            .iter()
            .enumerate()
            .map(|(i, t)| Piece { name: t.name.clone(), value: numbers.share(i).clamp(0.0, 1.0), colour: t.colour })
            .collect(),
        MeterReads::Revealed => single("Revealed", numbers.revealed),
        MeterReads::Covered => single("Covered", numbers.covered),
        MeterReads::Wet => single("Wet", numbers.wet),
    }
}

fn percent(v: f32) -> String {
    format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// A label made of the name and the percent, whichever are switched on.
fn label(s: &MeterSettings, piece: &Piece, single: bool) -> String {
    let mut parts = Vec::new();
    if s.show_names && !single {
        parts.push(piece.name.clone());
    }
    if s.show_percent {
        parts.push(percent(piece.value));
    }
    parts.join(" ")
}

/// Lays `text` out around `anchor` and gives its letters the meter's text colour.
fn put_text(frame: &mut Frame, s: &MeterSettings, text: &str, size: f32, anchor: Vec2, max_width: f32) {
    if text.trim().is_empty() {
        return;
    }
    let style = TextStyle { color: [1.0; 4], outline: 0.0, outline_color: [1.0; 4], glow: 0.0 };
    let first = frame.glyphs.len();
    layout_text(text, s.font.face(), size, anchor, TextAlign::Center, max_width, usize::MAX, &style, &mut frame.glyphs);
    let colour = blend_alpha(s.text_colour.to_linear());
    for g in &mut frame.glyphs[first..] {
        g.color = colour;
        g.outline_color = colour;
    }
}

fn split_bar(frame: &mut Frame, s: &MeterSettings, at: Vec2, pieces: &[Piece], single: bool) {
    let (width, thickness) = (s.width.max(1.0), s.thickness.max(1.0));
    let (half, corner) = (Vec2::new(width * 0.5, thickness * 0.5), thickness * 0.5);
    frame.shapes.push(ShapeInstance::fill_box(at, 0.0, half, corner, ALL, s.empty_colour.to_linear()));
    let mut x0 = -width * 0.5;
    for piece in pieces {
        let x1 = x0 + piece.value * width;
        frame.shapes.push(ShapeInstance::fill_box(at, 0.0, half, corner, [x0, x1], piece.colour));
        // Unclaimed ground is the background showing. Labels sit in the pieces wide enough.
        if single {
            put_text(frame, s, &label(s, piece, true), s.text_size, at, width - 12.0);
        } else if x1 - x0 > LABEL_MIN_WIDTH {
            put_text(
                frame,
                s,
                &label(s, piece, false),
                s.text_size,
                at + Vec2::new((x0 + x1) * 0.5, 0.0),
                x1 - x0 - 12.0,
            );
        }
        x0 = x1;
    }
}

fn ring(frame: &mut Frame, s: &MeterSettings, at: Vec2, pieces: &[Piece], single: bool) {
    let (radius, half_width) = (s.ring_size.max(1.0) * 0.5, s.thickness.max(1.0) * 0.5);
    frame.shapes.push(ShapeInstance::arc_band(at, radius, half_width, 0.0, PI, s.empty_colour.to_linear()));
    // Clockwise from the top.
    let mut start = 0.0;
    for piece in pieces {
        let sweep = piece.value * TAU;
        if sweep >= 1e-4 {
            frame.shapes.push(ShapeInstance::arc_band(
                at,
                radius,
                half_width,
                -FRAC_PI_2 + start + sweep * 0.5,
                sweep * 0.5,
                piece.colour,
            ));
        }
        start += sweep;
    }
    // The middle says who leads (Shares) or the number itself.
    let lead = if single {
        pieces.first()
    } else {
        // The largest share above 0; the first team on a tie.
        let mut best: Option<&Piece> = None;
        for p in pieces {
            if p.value > 0.0 && best.is_none_or(|b| p.value > b.value) {
                best = Some(p);
            }
        }
        best
    };
    let Some(lead) = lead else { return };
    let name = if s.show_names && !single { lead.name.clone() } else { String::new() };
    let value = if s.show_percent { percent(lead.value) } else { String::new() };
    let inner = (radius - half_width) * 2.0 - 12.0;
    let rise = s.text_size * 0.6;
    match (name.is_empty(), value.is_empty()) {
        (false, false) => {
            put_text(frame, s, &name, s.text_size, at - Vec2::new(0.0, rise), inner);
            put_text(frame, s, &value, s.text_size, at + Vec2::new(0.0, rise), inner);
        }
        (false, true) => put_text(frame, s, &name, s.text_size, at, inner),
        (true, false) => put_text(frame, s, &value, s.text_size, at, inner),
        (true, true) => {}
    }
}

fn ranking(frame: &mut Frame, s: &MeterSettings, at: Vec2, pieces: &[Piece], single: bool) {
    let (width, thickness) = (s.width.max(1.0), s.thickness.max(1.0));
    let (half, corner) = (Vec2::new(width * 0.5, thickness * 0.5), thickness * 0.5);
    // Largest first; ties keep the order the teams claimed in.
    let mut rows: Vec<&Piece> = pieces.iter().collect();
    rows.sort_by(|a, b| b.value.total_cmp(&a.value));
    rows.truncate(s.max_rows.clamp(1, 8) as usize);
    let face = s.font.face();
    for (k, piece) in rows.into_iter().enumerate() {
        let centre = at + Vec2::new(0.0, k as f32 * thickness * 1.4);
        frame.shapes.push(ShapeInstance::fill_box(centre, 0.0, half, corner, ALL, s.empty_colour.to_linear()));
        let left = -width * 0.5;
        frame.shapes.push(ShapeInstance::fill_box(
            centre,
            0.0,
            half,
            corner,
            [left, left + piece.value * width],
            piece.colour,
        ));
        // The name inside the bar's left end, the percent inside its right end.
        if s.show_names || single {
            let w = measure_text(&piece.name, face, s.text_size).x;
            put_text(frame, s, &piece.name, s.text_size, centre + Vec2::new(left + 16.0 + w * 0.5, 0.0), 0.0);
        }
        if s.show_percent {
            let text = percent(piece.value);
            let w = measure_text(&text, face, s.text_size).x;
            put_text(frame, s, &text, s.text_size, centre + Vec2::new(width * 0.5 - 16.0 - w * 0.5, 0.0), 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, new_scene};
    use bpe_core::{Color, Params, Resolved, Scene, Value, resolve};
    use bpe_gfx::build_frame_at;
    use bpe_runtime::{CanvasSample, Log, Team, World};

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const GOLD: [f32; 4] = [1.0, 0.8, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    /// A scene with a canvas "Card" and a meter set by `f`; its world and a log with `samples`
    /// (step, numbers) and the teams written by hand.
    fn setup(
        f: impl FnOnce(&mut Params),
        teams: &[(&str, [f32; 4])],
        samples: &[(u64, CanvasNumbers)],
    ) -> (Scene, Resolved, World, Log) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let card = crate::add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap();
        scene.things.get_mut(&card).unwrap().name = "Card".into();
        let meter = crate::add_thing(&mut scene, &kit.registry, "meter", ThingId::ROOT).unwrap();
        scene.things.get_mut(&meter).unwrap().transform.position = Vec2::ZERO;
        f(&mut scene.things.get_mut(&meter).unwrap().params);
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        for (step, numbers) in samples {
            log.push_canvas(CanvasSample { step: *step, canvas: card, numbers: numbers.clone() });
        }
        log.set_teams(card, teams.iter().map(|(n, c)| Team { name: (*n).into(), colour: *c }).collect());
        (scene, resolved, world, log)
    }

    /// The overlay's shapes of kind 3 and 4 when the meter is drawn at `time`.
    fn shapes_at(
        time: f64,
        scene: &Scene,
        resolved: &Resolved,
        world: &World,
        log: &Log,
    ) -> (Vec<ShapeInstance>, usize) {
        let kit = kit();
        let mut frame = Frame::default();
        build_frame_at(scene, resolved, world, log, time, time, &kit.draws, &mut frame);
        let overlay = frame.overlay.expect("a meter draws into the overlay");
        (overlay.shapes.iter().filter(|s| s.c[0] > 2.5).copied().collect(), overlay.glyphs.len())
    }

    fn shares(v: &[f32]) -> CanvasNumbers {
        CanvasNumbers { shares: v.to_vec(), ..Default::default() }
    }

    fn two_teams() -> Vec<(&'static str, [f32; 4])> {
        vec![("Red", RED), ("Blue", BLUE)]
    }

    fn look(name: &str) -> impl FnOnce(&mut Params) + '_ {
        move |p| p.set("look", Value::Choice(name.into()))
    }

    #[test]
    fn split_bar_pieces_follow_the_shares() {
        let (scene, resolved, world, log) = setup(|_| {}, &two_teams(), &[(0, shares(&[0.25, 0.5]))]);
        let (shapes, glyphs) = shapes_at(0.0, &scene, &resolved, &world, &log);
        assert_eq!(shapes.len(), 3, "the background and one piece per team");
        assert!(shapes.iter().all(|s| s.c[0] == 3.0));
        // Width 800: Red takes the first quarter, Blue the next half.
        assert_eq!((shapes[1].a[3], shapes[1].b[3]), (-400.0, -200.0));
        assert_eq!((shapes[2].a[3], shapes[2].b[3]), (-200.0, 200.0));
        assert_eq!(shapes[1].color, blend_alpha(RED));
        assert!(glyphs > 0, "the pieces are labelled");
    }

    #[test]
    fn smooth_averages_the_window() {
        // 0 until step 116, 1 from step 120, every 4th step.
        let samples: Vec<(u64, CanvasNumbers)> = (0..=60u64)
            .map(|k| (k * 4, CanvasNumbers { revealed: if k * 4 >= 120 { 1.0 } else { 0.0 }, ..Default::default() }))
            .collect();
        let reveal = |smooth: f32| {
            move |p: &mut Params| {
                p.set("reads", Value::Choice("revealed".into()));
                p.set("smooth", Value::Number(smooth as f64));
            }
        };
        let (scene, resolved, world, log) = setup(reveal(1.0), &[], &samples);
        // Half a second at 240 Hz is 120 steps: drawn at step 240 the window (120, 240] is all 1.
        let (late, _) = shapes_at(1.0, &scene, &resolved, &world, &log);
        assert_eq!(late[1].b[3], -400.0 + 800.0, "full");
        // At step 180 the window (60, 180] is about half 0, half 1.
        let (mid, _) = shapes_at(0.75, &scene, &resolved, &world, &log);
        let filled = (mid[1].b[3] + 400.0) / 800.0;
        assert!((0.3..0.7).contains(&filled), "{filled}");
        // Without smoothing it follows the step exactly.
        let (scene, resolved, world, log) = setup(reveal(0.0), &[], &samples);
        let (sharp, _) = shapes_at(0.75, &scene, &resolved, &world, &log);
        assert_eq!(sharp[1].b[3], 400.0);
    }

    #[test]
    fn ring_starts_at_the_top() {
        let (scene, resolved, world, log) = setup(look("ring"), &[("Red", RED)], &[(0, shares(&[0.25]))]);
        let (shapes, _) = shapes_at(0.0, &scene, &resolved, &world, &log);
        assert_eq!(shapes.len(), 2, "the background ring and one piece");
        assert!(shapes.iter().all(|s| s.c[0] == 4.0));
        let piece = shapes[1];
        let quarter = std::f32::consts::FRAC_PI_4;
        assert!((piece.a[2] - (-std::f32::consts::FRAC_PI_2 + quarter)).abs() < 1e-5, "{}", piece.a[2]);
        assert!((piece.b[2] - quarter).abs() < 1e-5, "{}", piece.b[2]);
    }

    #[test]
    fn ranking_sorts_and_limits() {
        let teams = [("A", RED), ("B", BLUE), ("C", GOLD), ("D", GREEN)];
        let (scene, resolved, world, log) = setup(
            |p| {
                p.set("look", Value::Choice("ranking".into()));
                p.set("max_rows", Value::Int(3));
            },
            &teams,
            &[(0, shares(&[0.1, 0.4, 0.2, 0.3]))],
        );
        let (shapes, _) = shapes_at(0.0, &scene, &resolved, &world, &log);
        // A background and a piece per row, three rows: B (0.4), D (0.3), C (0.2).
        assert_eq!(shapes.len(), 6);
        let ends: Vec<f32> = shapes.iter().skip(1).step_by(2).map(|s| s.b[3]).collect();
        let expect: Vec<f32> = [0.4, 0.3, 0.2].iter().map(|v| -400.0 + v * 800.0).collect();
        for (got, want) in ends.iter().zip(&expect) {
            assert!((got - want).abs() < 1e-3, "{ends:?} vs {expect:?}");
        }
        assert!(shapes[2].a[1] > shapes[0].a[1], "rows go down the screen");
    }

    #[test]
    fn single_number_uses_the_fill_colour() {
        let (scene, resolved, world, log) = setup(
            |p| {
                p.set("reads", Value::Choice("revealed".into()));
                p.set("fill_colour", Value::Color(Color::rgb(0, 255, 0)));
            },
            &[],
            &[(0, CanvasNumbers { revealed: 0.6, ..Default::default() })],
        );
        let (shapes, _) = shapes_at(0.0, &scene, &resolved, &world, &log);
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[1].color, blend_alpha(Color::rgb(0, 255, 0).to_linear()));
        assert!((shapes[1].b[3] - (-400.0 + 0.6 * 800.0)).abs() < 1e-3);
    }

    #[test]
    fn no_canvas_draws_nothing() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        crate::add_thing(&mut scene, &kit.registry, "meter", ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame_at(&scene, &resolved, &world, &Log::default(), 0.0, 0.0, &kit.draws, &mut frame);
        assert!(frame.overlay.is_none_or(|o| o.shapes.is_empty() && o.glyphs.is_empty()));
    }

    #[test]
    fn meter_settings_round_trip_and_the_menu_lists_it() {
        let s = MeterSettings { look: MeterLook::Ring, reads: MeterReads::Wet, max_rows: 6, ..Default::default() };
        assert_eq!(MeterSettings::from_params(&s.to_params()), s);
        let kit = kit();
        let kind = kit.registry.thing("meter").expect("registered");
        assert_eq!(kind.family(), "On screen");
        assert_eq!(kind.name(), "Meter");
    }
}
