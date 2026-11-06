//! Drawing a broken shape's or ball's look: its instance faded, swelled, or copied once per
//! Shatter piece, each copy clipped to its piece's cell (`PathInstance::clip`,
//! `LayeredInstance::clip`).

use bpe_gfx::{Frame, LayeredInstance, PathInstance};
use bpe_runtime::{Remains, Wall};
use glam::Vec2;

use super::shatter::{Edge, Material, Piece, PieceNow, Start, cut, piece_at, seed_of};
use super::{Showing, showing};
use crate::rules::breaks::{BreakLook, LookSettings};

/// Every wall of the world, for pieces to bounce off (a broken shape's is gone already).
pub fn walls_of(world: &bpe_runtime::World) -> Vec<&Wall> {
    world.walls.values().collect()
}

/// Puts `piece`'s cell into `frame.path_points` (going round the positive way), giving the
/// `clip` that names it, at `fade`.
fn clip_of(piece: &Piece, fade: f32, frame: &mut Frame) -> [f32; 4] {
    let first = frame.path_points.len();
    let cell = &piece.cell;
    let area: f32 = (0..cell.len()).map(|k| cell[k].perp_dot(cell[(k + 1) % cell.len()])).sum();
    if area >= 0.0 {
        frame.path_points.extend(cell.iter().map(|p| [p.x, p.y, 0.0, 0.0]));
    } else {
        frame.path_points.extend(cell.iter().rev().map(|p| [p.x, p.y, 0.0, 0.0]));
    }
    [first as f32, cell.len() as f32, fade.clamp(0.0, 1.0), 0.0]
}

/// How a Fade out or Pop look shows now (`None` for Shatter, which draws pieces, or once over).
pub fn whole_look(r: &Remains, look: &LookSettings, time: f64) -> Option<Showing> {
    match look.look {
        BreakLook::FadeOut | BreakLook::Pop => showing(look, r.age(time)),
        _ => None,
    }
}

/// The pieces of a Shatter at `time`, each with where it is and how much shows (only the ones
/// still showing).
fn pieces_now(
    r: &Remains,
    look: &LookSettings,
    material: &Material,
    bound: f32,
    start: &Start,
    time: f64,
    walls: &[&Wall],
) -> Vec<(Piece, PieceNow, f32)> {
    let age = r.age(time);
    let Some(shows) = showing(look, age) else { return Vec::new() };
    if shows.alpha <= 0.0 {
        return Vec::new();
    }
    cut(material, bound, look.pieces, start.seed)
        .into_iter()
        .enumerate()
        .map(|(k, piece)| {
            let now = piece_at(&piece, k as u32, start, look, age as f32, walls);
            (piece, now, shows.alpha)
        })
        .collect()
}

/// A broken shape's Shatter: the shape instance at `index` (pushed as the shape stood, with its
/// records) becomes its pieces: the first takes its place, the rest go after it.
#[allow(clippy::too_many_arguments)]
pub fn shatter_shape(
    r: &Remains,
    look: &LookSettings,
    thing: u32,
    edge: Edge,
    half_units: f32,
    index: usize,
    time: f64,
    walls: &[&Wall],
    frame: &mut Frame,
) {
    let base = frame.paths[index];
    let (centre, turn, scale) = (Vec2::new(base.a[0], base.a[1]), base.a[2], base.a[3].max(1e-3));
    let material = Material { edge, half: half_units };
    let bound = base.reach() / scale * 1.01 + 1e-3;
    let n = look.pieces.clamp(2, 256);
    let start = Start {
        centre,
        turn,
        scale,
        velocity: r.wall.as_ref().map_or(Vec2::ZERO, |w| w.velocity),
        from: Start::from_of(centre, r.point, bound * scale),
        gravity: r.gravity,
        radius: (material.area() * scale * scale / (n as f32 * std::f32::consts::PI)).max(0.0).sqrt(),
        seed: seed_of(thing, u32::MAX, r.step),
    };
    let pieces = pieces_now(r, look, &material, bound, &start, time, walls);
    if pieces.is_empty() {
        frame.paths.remove(index);
        return;
    }
    for (k, (piece, now, alpha)) in pieces.iter().enumerate() {
        let centre = now.centre(piece.site, scale);
        let clip = clip_of(piece, 1.0 - alpha, frame);
        let made = PathInstance { a: [centre.x, centre.y, now.turn, scale], clip, ..base };
        if k == 0 {
            frame.paths[index] = made;
        } else {
            frame.paths.push(made);
        }
    }
}

/// A broken ball's look as instances, from `ball` (made where it broke, standing still):
/// faded or swelled, or its Shatter pieces.
pub fn ball_remains(
    r: &Remains,
    look: &LookSettings,
    thing: u32,
    row: u32,
    ball: LayeredInstance,
    time: f64,
    walls: &[&Wall],
    frame: &mut Frame,
    out: &mut Vec<LayeredInstance>,
) {
    if let Some(s) = whole_look(r, look, time) {
        if s.alpha > 0.0 {
            out.push(LayeredInstance {
                radius: ball.radius * s.scale.max(0.0),
                misc: [ball.misc[0] * s.alpha, ball.misc[1], ball.misc[2], ball.misc[3]],
                ..ball
            });
        }
        return;
    }
    if look.look != BreakLook::Shatter {
        return;
    }
    let radius = ball.radius.max(1e-3);
    let centre = Vec2::from(ball.center);
    let n = look.pieces.clamp(2, 256);
    let bound = ball.form[3].max(1.0) * 1.01;
    let start = Start {
        centre,
        turn: ball.stretch[1].atan2(ball.stretch[0]),
        scale: radius,
        velocity: r.ball.map_or(Vec2::ZERO, |b| b.velocity),
        from: Start::from_of(centre, r.point, bound * radius),
        gravity: r.gravity,
        radius: radius / (n as f32).sqrt(),
        seed: seed_of(thing, row, r.step),
    };
    for (piece, now, alpha) in pieces_now(r, look, &Material::DISC, bound, &start, time, walls) {
        let at = now.centre(piece.site, radius);
        let dir = Vec2::from_angle(now.turn);
        let clip = clip_of(&piece, 1.0 - alpha, frame);
        out.push(LayeredInstance {
            center: at.into(),
            stretch: [dir.x, dir.y, ball.stretch[2], ball.stretch[3]],
            clip,
            ..ball
        });
    }
}
