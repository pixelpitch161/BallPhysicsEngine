//! Picture tests for sub-layers (sub_layers.wgsl, worked by the walks), on the software GPU.

use crate::pictures::{pixel_sized, render_sized, save};
use crate::{Frame, LayerGpu, LayeredInstance, PaintGpu};
use bpe_core::{Blend, Color, Gradient};
use glam::Vec2;

const W: u32 = 540;
const H: u32 = 960;
const GREY: [f32; 4] = [0.2, 0.2, 0.2, 1.0];

fn ball(center: Vec2, radius: f32, body: [f32; 4], first: u32, count: u32) -> LayeredInstance {
    LayeredInstance {
        center: center.into(),
        radius,
        seed: 0.3,
        body,
        stretch: [1.0, 0.0, 1.0, 1.0],
        form: [0.0, 0.15, 0.0, 1.6],
        misc: [1.0, 3.0, 0.0, 0.0],
        vary: crate::NO_VARY,
        range: [first, count, 0, 0],
        group: [0.0; 4],
        place: [0.0; 4],
        hit: crate::NO_HIT,
    }
}

fn record(code: u32) -> LayerGpu {
    LayerGpu { head: [code, Blend::Normal.shader_code(), 0, 0], amount: [1.0, 0.0, 0.0, 0.0], ..LayerGpu::ZERO }
}

fn body() -> LayerGpu {
    record(LayerGpu::BODY)
}

/// A pattern of one colour covering the ball, with `subs` sub-layer records after it.
fn plain_pattern(frame: &mut Frame, colour: Color, subs: u32) -> LayerGpu {
    let row = frame.gradients.row(&Gradient::solid(colour));
    LayerGpu {
        head: [LayerGpu::PATTERN, 0, 0, subs],
        paint: PaintGpu { row: row as f32, ..PaintGpu::default() },
        ..record(LayerGpu::PATTERN)
    }
}

/// An Outline layer `width` px wide inside the edge, in blue, with `subs` sub-layer records after it.
fn outline(width: f32, subs: u32) -> LayerGpu {
    LayerGpu {
        head: [LayerGpu::OUTLINE, 0, 0, subs],
        p0: [width, 0.0, 0.0, 0.0],
        colour: [0.0, 0.0, 1.0, 1.0],
        ..record(LayerGpu::OUTLINE)
    }
}

/// A Shape cut letting the top half show.
fn top_half() -> LayerGpu {
    LayerGpu {
        head: [LayerGpu::SUB_CUT, 0, 0, 0],
        amount: [1.0, -1.0, 0.0, 0.0],
        p0: [2.0, 0.0, 0.01, 1.0],
        p1: [0.0, 0.5, 0.5, -1.0],
        ..LayerGpu::ZERO
    }
}

/// A Glow sub-layer in `colour`, `size` radii, shining outside (0), inside (1) or both (2).
fn glow(colour: [f32; 4], size: f32, side: f32) -> LayerGpu {
    LayerGpu {
        head: [LayerGpu::SUB_GLOW, 0, 0, 0],
        amount: [1.0, 1.0, 0.0, 0.0],
        p0: [size, side, 0.0, 0.0],
        colour,
        ..LayerGpu::ZERO
    }
}

/// One frame of balls, each `(centre, radius, body, records)`.
fn balls(f: impl FnOnce(&mut Frame) -> Vec<(Vec2, f32, [f32; 4], Vec<LayerGpu>)>) -> Frame {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    for (centre, radius, colour, records) in f(&mut frame) {
        let first = frame.look_layers.len() as u32;
        frame.layered.push(ball(centre, radius, colour, first, records.len() as u32));
        frame.look_layers.extend(records);
    }
    frame
}

fn at(pixels: &[u8], p: Vec2) -> [u8; 4] {
    pixel_sized(pixels, W, H, p)
}

#[test]
fn a_glow_on_an_outline_lights_the_body_inside_the_ring_and_a_glow_follows_a_cut_only_after_it() {
    let left = Vec2::new(-130.0, -250.0);
    let right = Vec2::new(130.0, -250.0);
    let below = Vec2::new(-130.0, 150.0);
    let below_right = Vec2::new(130.0, 150.0);
    let red = [1.0, 0.0, 0.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    let frame = balls(|f| {
        vec![
            // An outline 20 px wide inside the edge, lit red round its band.
            (left, 100.0, GREY, vec![body(), outline(20.0, 1), glow(red, 0.2, 0.0)]),
            // The same outline without its glow.
            (right, 100.0, GREY, vec![body(), outline(20.0, 0)]),
            // A red pattern cut to its top half, then lit green round what's left.
            (
                below,
                100.0,
                GREY,
                vec![body(), plain_pattern(f, Color::rgb(255, 0, 0), 2), top_half(), glow(green, 0.2, 0.0)],
            ),
            // Lit round the whole ball first, then cut.
            (
                below_right,
                100.0,
                GREY,
                vec![body(), plain_pattern(f, Color::rgb(255, 0, 0), 2), glow(green, 0.2, 0.0), top_half()],
            ),
        ]
    });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("sub-glow", &px, W, H);
    // Just inside the ring's inner edge (80 px from the middle).
    let lit = at(&px, left + Vec2::new(72.0, 0.5));
    let plain = at(&px, right + Vec2::new(72.0, 0.5));
    assert!(lit[0] > plain[0] + 40 && lit[1] < plain[1] + 10, "red light inside the ring: {lit:?} vs {plain:?}");
    assert!(plain[0].abs_diff(plain[1]) < 6, "grey without the glow: {plain:?}");
    // Just under the cut line, where the cut pattern leaves the grey body.
    let after = at(&px, below + Vec2::new(0.5, 8.0));
    let before = at(&px, below_right + Vec2::new(0.5, 8.0));
    assert!(after[1] > after[0] + 40, "green light along the cut edge: {after:?}");
    assert!(before[0].abs_diff(before[1]) < 6, "a glow before the cut doesn't follow it: {before:?}");
    assert!(at(&px, below + Vec2::new(0.5, -50.0))[0] > 200, "the red top half");
}

#[test]
fn a_shadow_falls_at_its_angle_and_takes_its_cuts() {
    let down = Vec2::new(-130.0, -200.0);
    let up = Vec2::new(130.0, -200.0);
    let shadow = |angle: f32| {
        let a = angle.to_radians();
        LayerGpu {
            head: [LayerGpu::SUB_SHADOW, 0, 0, 0],
            amount: [1.0, 0.0, 0.0, 0.0],
            p0: [0.3 * a.cos(), 0.3 * a.sin(), 0.01, 1.0],
            colour: [0.0, 0.0, 0.0, 1.0],
            ..LayerGpu::ZERO
        }
    };
    let white = [1.0; 4];
    let frame = balls(|f| {
        vec![
            (down, 100.0, white, vec![body(), plain_pattern(f, Color::rgb(255, 0, 0), 2), top_half(), shadow(90.0)]),
            (up, 100.0, white, vec![body(), plain_pattern(f, Color::rgb(255, 0, 0), 2), top_half(), shadow(-90.0)]),
        ]
    });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("sub-shadow", &px, W, H);
    // 90° falls downwards: the half's shadow darkens the white body just under the cut line, but
    // not past its 30 px.
    let shaded = at(&px, down + Vec2::new(0.5, 15.0));
    let clear = at(&px, down + Vec2::new(0.5, 50.0));
    assert!(shaded[1] < 40 && shaded[0] < 40, "the shadow under the cut: {shaded:?}");
    assert!(clear[1] > 240, "white past the shadow: {clear:?}");
    // Up, it hides under the pattern itself.
    assert!(at(&px, up + Vec2::new(0.5, 15.0))[1] > 240, "no shadow below when it falls up");
    let top = at(&px, up + Vec2::new(0.5, -50.0));
    assert!(top[0] > 200 && top[1] < 30, "the red half over its own shadow: {top:?}");
}

#[test]
fn a_recolour_an_outline_and_hits_change_only_their_layer() {
    let white = [1.0; 4];
    let recolour = LayerGpu {
        head: [LayerGpu::SUB_RECOLOUR, 0, 0, 0],
        amount: [1.0, 1.0, 0.0, 0.0],
        p0: [0.0, 0.0, 0.0, 1.0],
        p1: [1.0, 0.0, 0.0, 0.0],
        colour: [0.0, 0.0, 1.0, 1.0],
        ..LayerGpu::ZERO
    };
    let sub_outline = LayerGpu {
        head: [LayerGpu::SUB_OUTLINE, 0, 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        p0: [10.0, 1.0, 0.0, 0.0],
        colour: [0.0, 1.0, 0.0, 1.0],
        ..LayerGpu::ZERO
    };
    let flash = LayerGpu {
        head: [LayerGpu::SUB_FLASH, 0, 0, 0],
        amount: [1.0, 0.5, 0.0, 0.0],
        colour: [1.0, 1.0, 1.0, 1.0],
        ..LayerGpu::ZERO
    };
    let hue = LayerGpu { head: [LayerGpu::SUB_HUE, 0, 0, 0], amount: [1.0 / 3.0, 0.0, 0.0, 0.0], ..LayerGpu::ZERO };
    let spots = [
        Vec2::new(-130.0, -330.0),
        Vec2::new(130.0, -330.0),
        Vec2::new(-130.0, -80.0),
        Vec2::new(130.0, -80.0),
        Vec2::new(-130.0, 170.0),
        Vec2::new(130.0, 170.0),
    ];
    let mut frame = balls(|f| {
        let red = |f: &mut Frame, subs| plain_pattern(f, Color::rgb(255, 0, 0), subs);
        let grey = plain_pattern(f, Color::rgb(60, 60, 60), 1);
        vec![
            (spots[0], 100.0, white, vec![body(), red(f, 2), top_half(), recolour]),
            (spots[1], 100.0, white, vec![body(), red(f, 2), top_half(), sub_outline]),
            (spots[2], 100.0, white, vec![body(), grey, flash]),
            (spots[3], 100.0, white, vec![body(), grey, flash]),
            (spots[4], 100.0, white, vec![body(), red(f, 1), hue]),
            (spots[5], 100.0, white, vec![body(), red(f, 1), hue]),
        ]
    });
    // Hit just now, never hit; one hit so far, none.
    frame.layered[2].hit = [0.0, 1.0, 1.0, 0.0];
    frame.layered[4].hit = [3.0, 1.0, 1.0, 0.0];
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("sub-recolour-outline-hits", &px, W, H);
    let top = at(&px, spots[0] + Vec2::new(0.5, -50.0));
    let bottom = at(&px, spots[0] + Vec2::new(0.5, 50.0));
    assert!(top[2] > 200 && top[0] < 30, "the pattern turned blue: {top:?}");
    assert!(bottom.iter().take(3).all(|&c| c > 240), "the body under it stays white: {bottom:?}");
    // The outline runs along the cut line (and round the top half's edge).
    let line = at(&px, spots[1] + Vec2::new(-50.0, 0.5));
    assert!(line[1] > 200 && line[0] < 60 && line[2] < 60, "green along the cut: {line:?}");
    assert!(at(&px, spots[1] + Vec2::new(0.5, -50.0))[1] < 30, "red in the half");
    // The flash lights the ball hit just now; the other stays grey.
    let (hit, quiet) = (at(&px, spots[2] + Vec2::new(0.5, 0.5)), at(&px, spots[3] + Vec2::new(0.5, 0.5)));
    assert!(hit[0] > 200 && quiet[0] < 110, "flash {hit:?}, none {quiet:?}");
    // A third of a turn per hit: red becomes green after one hit.
    let (turned, red) = (at(&px, spots[4] + Vec2::new(0.5, 0.5)), at(&px, spots[5] + Vec2::new(0.5, 0.5)));
    assert!(turned[1] > 200 && turned[0] < 30, "green after a hit: {turned:?}");
    assert!(red[0] > 200 && red[1] < 30, "red before any: {red:?}");
}

// ---- Shapes ----

#[test]
fn a_shape_s_outline_takes_a_glow_sub_layer_lighting_inside_its_band_and_a_recolour() {
    const SW: u32 = 1080;
    const SH: u32 = 1920;
    let white = [1.0; 4];
    let square = [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)];
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let glowing = Vec2::new(-250.0, -300.0);
    let recoloured = Vec2::new(250.0, -300.0);
    frame.push_path(glowing, 0.0, 150.0, 0.0, Some(&square), white, 2.0);
    frame.push_path(recoloured, 0.0, 150.0, 0.0, Some(&square), white, 3.0);
    let shape_body = LayerGpu { amount: [1.0, 0.0, 0.0, 0.0], colour: [0.05, 0.05, 0.05, 1.0], ..LayerGpu::ZERO };
    // An outline 10 px wide inside the edge, with a red glow 30 px round its band (inside the
    // square too: outside the band).
    let line = LayerGpu {
        head: [LayerGpu::OUTLINE, 0, 0, 1],
        amount: [1.0, 0.0, 0.0, 0.0],
        p0: [10.0, 0.0, 0.0, 0.0],
        colour: [0.0, 0.0, 1.0, 1.0],
        ..LayerGpu::ZERO
    };
    let sub_glow = LayerGpu { p0: [30.0, 0.0, 0.0, 0.0], ..glow([1.0, 0.0, 0.0, 1.0], 30.0, 0.0) };
    let paint = LayerGpu {
        head: [LayerGpu::SHAPE_PAINT, 0, 0, 1],
        amount: [1.0, 0.0, 0.0, 0.0],
        colour: [1.0, 0.0, 0.0, 1.0],
        ..LayerGpu::ZERO
    };
    let recolour = LayerGpu {
        head: [LayerGpu::SUB_RECOLOUR, 0, 0, 0],
        amount: [1.0, 1.0, 0.0, 0.0],
        p0: [0.0, 0.0, 0.0, 1.0],
        p1: [1.0, 0.0, 0.0, 0.0],
        colour: [0.0, 1.0, 0.0, 1.0],
        ..LayerGpu::ZERO
    };
    for (i, records) in [vec![shape_body, line, sub_glow], vec![shape_body, paint, recolour]].into_iter().enumerate() {
        let first = frame.look_layers.len() as u32;
        frame.paths[i].layers = [first, records.len() as u32, crate::PathInstance::NO_PROGRAM, 0];
        frame.paths[i].c[2] = 70.0;
        frame.look_layers.extend(records);
    }
    let Some(px) = render_sized(&frame, SW, SH) else { return };
    save("shape-sub-layers", &px, SW, SH);
    let at = |p: Vec2| pixel_sized(&px, SW, SH, p);
    let band = at(glowing + Vec2::new(145.0, 0.5));
    let inside = at(glowing + Vec2::new(125.0, 0.5));
    let middle = at(glowing + Vec2::new(0.5, 0.5));
    let outside = at(glowing + Vec2::new(165.0, 0.5));
    assert!(band[2] > 200, "the blue band: {band:?}");
    assert!(inside[0] > middle[0] + 40, "red light inside the band: {inside:?} vs {middle:?}");
    assert!(outside[0] > 60 && outside[1] < 30, "and outside it: {outside:?}");
    let green = at(recoloured + Vec2::new(0.5, 0.5));
    assert!(green[1] > 200 && green[0] < 30, "the red paint turned green: {green:?}");
}
