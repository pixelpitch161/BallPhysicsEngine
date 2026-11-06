use bpe_runtime::{Outline, Wall};
use glam::Vec2;

use super::shatter::{Edge, Material, Piece, Start, cut, piece_at};
use super::showing;
use crate::rules::breaks::{BreakLook, LookSettings};

fn look(l: BreakLook) -> LookSettings {
    LookSettings { look: l, ..LookSettings::default() }
}

#[test]
fn each_look_shows_for_its_time() {
    let fade = LookSettings { fade_time: 2.0, ..look(BreakLook::FadeOut) };
    assert_eq!(showing(&fade, 0.0).unwrap().alpha, 1.0);
    assert!((showing(&fade, 1.0).unwrap().alpha - 0.5).abs() < 1e-6, "half way, half there");
    assert!(showing(&fade, 2.0).is_none() && showing(&fade, -0.1).is_none());
    let pop = LookSettings { pop_time: 1.0, swell: 2.0, ..look(BreakLook::Pop) };
    let mid = showing(&pop, 0.5).unwrap();
    assert!(mid.scale > 1.5 && mid.scale < 2.0 && mid.alpha < 1.0 && mid.alpha > 0.5, "{mid:?}");
    assert!(showing(&pop, 0.99).unwrap().scale > 1.99);
    assert!(showing(&pop, 1.0).is_none());
    let shatter = LookSettings { fade_after: 1.0, fade_over: 1.0, ..look(BreakLook::Shatter) };
    assert_eq!(showing(&shatter, 0.9).unwrap().alpha, 1.0, "holds until Fade after");
    assert!((showing(&shatter, 1.5).unwrap().alpha - 0.5).abs() < 1e-6);
    assert!(showing(&shatter, 2.0).is_none());
    assert!(showing(&look(BreakLook::Vanish), 0.0).is_none());
    let instant = LookSettings { fade_time: 0.0, ..look(BreakLook::FadeOut) };
    assert!(showing(&instant, 0.0).is_none(), "a zero time is over at once");
}

fn area(cell: &[Vec2]) -> f32 {
    (0..cell.len()).map(|k| cell[k].perp_dot(cell[(k + 1) % cell.len()])).sum::<f32>() * 0.5
}

/// Whether `p` is inside the convex `cell` (corners going the positive way), with some slack.
fn in_cell(cell: &[Vec2], p: Vec2) -> bool {
    (0..cell.len()).all(|k| {
        let (a, b) = (cell[k], cell[(k + 1) % cell.len()]);
        (b - a).perp_dot(p - a) >= -1e-4
    })
}

fn square() -> Vec<Vec2> {
    vec![Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)]
}

#[test]
fn the_cells_tile_the_square_round_their_sites() {
    let box_ = square();
    let materials = [
        Material::DISC,
        Material { edge: Edge::Points(&box_), half: 0.0 },
        Material { edge: Edge::Points(&box_), half: 0.05 },
        Material { edge: Edge::Ellipse(Vec2::new(1.0, 0.4)), half: 0.02 },
    ];
    for m in &materials {
        for n in [2, 12, 64] {
            let pieces = cut(m, 1.5, n, 77);
            assert!(pieces.len() as u32 <= n && pieces.len() as u32 >= n - 1, "{n}: {}", pieces.len());
            let total: f32 = pieces.iter().map(|p| area(&p.cell)).sum();
            assert!((total - 9.0).abs() < 1e-3, "{n}: the cells add up to the square, {total}");
            for p in &pieces {
                assert!(area(&p.cell) > 0.0, "each goes round the positive way");
                assert!(in_cell(&p.cell, p.site), "its site is in its cell");
                // Convex: every corner on the inside of every edge.
                assert!(p.cell.iter().all(|&c| in_cell(&p.cell, c)));
            }
        }
    }
    assert_eq!(cut(&Material::DISC, 1.5, 12, 77), cut(&Material::DISC, 1.5, 12, 77), "the same seed cuts the same");
    assert_ne!(cut(&Material::DISC, 1.5, 12, 77), cut(&Material::DISC, 1.5, 12, 78));
}

#[test]
fn sites_sit_on_what_is_drawn() {
    // A band along a circle: its sites are on the ring, not in the middle.
    let ring = Material { edge: Edge::Ellipse(Vec2::ONE), half: 0.05 };
    for p in cut(&ring, 1.2, 40, 3) {
        let r = p.site.length();
        assert!((0.94..=1.06).contains(&r), "{r}");
    }
    // Inside an L-shaped outline, not in its missing corner.
    let l = [
        Vec2::new(-1.0, -1.0),
        Vec2::new(0.0, -1.0),
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(-1.0, 1.0),
    ];
    for p in cut(&Material { edge: Edge::Points(&l), half: 0.0 }, 1.2, 40, 9) {
        assert!(!(p.site.x > 0.0 && p.site.y < 0.0), "{}", p.site);
    }
    assert!((Material { edge: Edge::Points(&l), half: 0.0 }.area() - 3.0).abs() < 1e-5);
}

fn start() -> Start {
    Start {
        centre: Vec2::new(0.0, -100.0),
        turn: 0.0,
        scale: 50.0,
        velocity: Vec2::ZERO,
        from: Vec2::new(0.0, -100.0),
        gravity: Vec2::new(0.0, 1000.0),
        radius: 8.0,
        seed: 5,
    }
}

#[test]
fn pieces_fly_out_and_fall_by_gravity() {
    let l = LookSettings { spread: 200.0, gravity: 2.0, ..look(BreakLook::Shatter) };
    let piece = Piece { site: Vec2::new(0.5, 0.0), cell: square() };
    let s = start();
    let at = |t: f32| piece_at(&piece, 3, &s, &l, t, &[]);
    assert_eq!(at(0.0).at, s.world_of(piece.site), "it sets off where it was");
    // Exact: x moves at a steady speed out from the middle, y falls by g t²/2 on top of that.
    let (a, b, c) = (at(0.1).at, at(0.2).at, at(0.3).at);
    let second = (c - 2.0 * b + a) / (0.1 * 0.1);
    assert!((second - Vec2::new(0.0, 2000.0)).length() < 1.0, "twice the scene's gravity: {second}");
    assert!(b.x > s.world_of(piece.site).x, "away from where it was hit");
    assert_eq!(at(0.25), at(0.25), "the same whenever it is worked out");
    assert!((at(1.0).turn - at(0.0).turn).abs() > 0.1, "it spins");
}

#[test]
fn pieces_bounce_off_the_shapes_and_stay_in() {
    let l = LookSettings { spread: 900.0, bounce: 0.5, ..look(BreakLook::Shatter) };
    let ring = Wall {
        outline: Outline::Ring { center: Vec2::ZERO, radius: 300.0, sides: 0, angle: 0.0, thickness: 4.0 },
        solid: false,
        spin: 0.0,
        growth: 0.0,
        velocity: Vec2::ZERO,
    };
    let walls = [&ring];
    let s = start();
    for k in 0..12 {
        let piece = Piece { site: Vec2::from_angle(k as f32).rotate(Vec2::new(0.8, 0.0)), cell: square() };
        for t in [0.2, 0.7, 1.4] {
            let p = piece_at(&piece, k, &s, &l, t, &walls).at;
            assert!(p.length() < 300.0, "piece {k} at {t} s: {p}");
        }
        let through = LookSettings { hit_shapes: false, ..l.clone() };
        assert!(piece_at(&piece, k, &s, &through, 3.0, &walls).at.length() > 300.0, "Hit shapes off falls out");
    }
}
