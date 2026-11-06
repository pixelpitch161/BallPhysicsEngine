//! Tests for the trail styles and the particle sprites, on the software GPU.

use crate::pictures::{pixel_sized, render_sized, save};
use crate::{BallInstance, Frame, Particle, StreakStyle, TrailVertex, push_streak, push_trail};
use glam::Vec2;

const W: u32 = 540;
const H: u32 = 960;
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn red(p: [u8; 4]) -> bool {
    p[0] > 150 && p[2] < 60
}

fn dark(p: [u8; 4]) -> bool {
    p[0] < 20 && p[1] < 20 && p[2] < 20
}

#[test]
fn a_plain_streak_keeps_its_old_vertices() {
    // A straight streak along x, 10 wide at the head, tapering by half, fading out.
    let points = [Vec2::new(0.0, 0.0), Vec2::new(-10.0, 0.0), Vec2::new(-20.0, 0.0)];
    let mut out = Vec::new();
    push_streak(&mut out, &points, 10.0, 0.5, RED, 1.0);
    assert_eq!(out.len(), 12, "two triangles a segment");
    // The head: 10 wide plus a pixel out either side, fully on.
    assert_eq!(
        out[0],
        TrailVertex { position: [0.0, -11.0], edge: [11.0, 10.0], color: RED, style: [0.0, 0.0], light: [0.0; 2] }
    );
    assert_eq!(out[1].position, [0.0, 11.0]);
    assert_eq!(out[1].edge, [-11.0, 10.0]);
    // The middle point: three quarters wide and a quarter of the alpha ((1 - 0.5)²).
    assert_eq!(out[2].edge, [8.5, 7.5]);
    assert_eq!(out[2].color, [1.0, 0.0, 0.0, 0.25]);
    // The tail: half wide and gone.
    assert_eq!(out[10].edge, [-6.0, 5.0]);
    assert_eq!(out[10].color[3], 0.0);
    // The same strip through push_trail with the widths and colours given.
    let mut same = Vec::new();
    let colors = [RED, [1.0, 0.0, 0.0, 0.25], [1.0, 0.0, 0.0, 0.0]];
    push_trail(&mut same, &points, &[10.0, 7.5, 5.0], &colors, StreakStyle::default(), 1.0);
    assert_eq!(same, out);
}

#[test]
fn a_soft_streak_reaches_further_and_its_edge_falls_off() {
    let points: Vec<Vec2> = (0..=20).map(|i| Vec2::new(200.0 - 20.0 * i as f32, -300.0)).collect();
    let soft_points: Vec<Vec2> = points.iter().map(|p| *p + Vec2::new(0.0, 600.0)).collect();
    let widths = [40.0];
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    push_trail(&mut frame.trails, &points, &widths, &[RED], StreakStyle::default(), 2.0);
    let soft = StreakStyle { softness: 1.0, band: 0.0 };
    push_trail(&mut frame.trails, &soft_points, &widths, &[RED], soft, 2.0);
    assert!(frame.trails.iter().any(|v| v.edge[0] > 80.0), "a soft edge reaches past the width");
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("soft_trail", &pixels, W, H);
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    let (crisp, soft) = (Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0));
    // Both are full in the middle.
    assert!(red(at(crisp)) && red(at(soft)), "{:?} {:?}", at(crisp), at(soft));
    // Near the edge the crisp one is still full, the soft one dimmer.
    let near = Vec2::new(0.0, 32.0);
    assert!(at(soft + near)[0] + 20 < at(crisp + near)[0], "{:?} {:?}", at(soft + near), at(crisp + near));
    // Past the edge the crisp one is gone and the soft one still glows.
    let past = Vec2::new(0.0, 48.0);
    assert!(dark(at(crisp + past)), "{:?}", at(crisp + past));
    assert!(at(soft + past)[0] > 20, "{:?}", at(soft + past));
}

#[test]
fn a_banded_streak_shows_only_its_edge() {
    let points: Vec<Vec2> = (0..=20).map(|i| Vec2::new(200.0 - 20.0 * i as f32, 0.0)).collect();
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    push_trail(&mut frame.trails, &points, &[40.0], &[RED], StreakStyle { softness: 0.0, band: 0.25 }, 2.0);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let at = |p: Vec2| pixel_sized(&pixels, W, H, p);
    assert!(dark(at(Vec2::new(0.0, 1.0))), "the middle is empty: {:?}", at(Vec2::new(0.0, 1.0)));
    assert!(red(at(Vec2::new(0.0, 35.0))), "the outer band shows: {:?}", at(Vec2::new(0.0, 35.0)));
}

#[test]
fn a_dot_lights_only_its_own_place() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let p = Vec2::new(120.0, -240.0);
    frame.particles.push(Particle::new(6, p, 0.0, 12.0, RED));
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let at = |q: Vec2| pixel_sized(&pixels, W, H, q);
    assert!(red(at(p)), "{:?}", at(p));
    for off in [Vec2::new(30.0, 0.0), Vec2::new(-30.0, 0.0), Vec2::new(0.0, 30.0), Vec2::new(0.0, -30.0)] {
        assert!(dark(at(p + off)), "{off}: {:?}", at(p + off));
    }
    assert!(dark(at(Vec2::ZERO)) && dark(at(-p)));
}

#[test]
fn every_shape_covers_its_middle_and_not_its_corners() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let place = |i: u32| Vec2::new(-300.0 + 150.0 * (i % 5) as f32, -300.0 + 300.0 * (i / 5) as f32);
    let size = 40.0;
    for shape in 0..10u32 {
        let mut p = Particle::new(shape, place(shape), 0.0, size, RED);
        if matches!(shape, 0 | 2 | 4 | 9) {
            p.style[0] = 60.0;
        }
        frame.particles.push(p);
    }
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("sprites", &pixels, W, H);
    let at = |q: Vec2| pixel_sized(&pixels, W, H, q);
    for (i, p) in frame.particles.iter().enumerate() {
        let c = p.at();
        // A ring is empty in the middle and lit on its line; a bubble's middle is a faint fill.
        let lit = if i == 5 { c + Vec2::new(size, 1.0) } else { c };
        let seen = at(lit);
        let enough = if i == 8 { 25 } else { 100 };
        assert!(seen[0] > enough, "shape {i} at its middle: {seen:?}");
        if i == 5 {
            assert!(dark(at(c)), "a ring's middle: {:?}", at(c));
        }
        // Just inside the quad's corner.
        let reach = p.style[0].max(size);
        let margin = if i == 5 { 8.0 } else { 3.0 };
        for corner in [Vec2::new(reach + margin, size + margin), Vec2::new(-reach - margin, -size - margin)] {
            assert!(dark(at(c + corner)), "shape {i} at its corner {corner}: {:?}", at(c + corner));
        }
    }
}

#[test]
fn front_particles_draw_over_the_balls_and_behind_ones_under() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let (under, over) = (Vec2::new(-150.0, 0.0), Vec2::new(150.0, 0.0));
    for c in [under, over] {
        frame.balls.push(BallInstance { center: c.into(), radius: 60.0, color: BLUE, ..BallInstance::default() });
    }
    frame.particles.push(Particle::new(6, under, 0.0, 20.0, RED));
    frame.particles_front.push(Particle::new(6, over, 0.0, 20.0, RED));
    // Added light in front brightens what is under it without covering it.
    let lit = Vec2::new(0.0, 400.0);
    frame.balls.push(BallInstance { center: lit.into(), radius: 60.0, color: BLUE, ..BallInstance::default() });
    frame.particles_front.push(Particle::new(6, lit, 0.0, 20.0, [1.0, 0.0, 0.0, -1.0]));
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("sprites_front", &pixels, W, H);
    let at = |q: Vec2| pixel_sized(&pixels, W, H, q);
    let (u, o, l) = (at(under), at(over), at(lit));
    assert!(u[2] > 150 && u[0] < 120, "the ball covers the dot behind it: {u:?}");
    assert!(red(o), "the dot in front shows: {o:?}");
    assert!(l[0] > 150 && l[2] > 150, "added light keeps the ball under it: {l:?}");
}

#[test]
fn added_light_sprites_show_in_a_layer_whose_things_blend_with_each_other() {
    // A dot of added light (a Ghosts glow, say) in Screen and Lighten layers that blend with each
    // other: the light shows, as in a layer that doesn't.
    use crate::{BlendLayer, Place};
    use bpe_core::Blend;
    let glow = Particle::new(6, Vec2::ZERO, 0.0, 60.0, [0.45, 0.0, 0.0, -1.0]);
    for blend in [Blend::Screen, Blend::Lighten] {
        let lit = |each: bool| {
            let mut layer = Frame::default();
            layer.particles.push(glow);
            let mut frame = Frame { bloom: 0.0, ..Frame::default() };
            frame.layers.push(BlendLayer { blend, amount: 1.0, each, z: 0, place: Place::Blend, frame: layer });
            render_sized(&frame, W, H).map(|px| pixel_sized(&px, W, H, Vec2::ZERO))
        };
        let Some(apart) = lit(false) else { return };
        let each = lit(true).unwrap();
        assert!(red(apart), "{blend:?}: the glow shows on its own: {apart:?}");
        assert!(red(each), "{blend:?}: and blending with each other: {each:?}");
    }
}
