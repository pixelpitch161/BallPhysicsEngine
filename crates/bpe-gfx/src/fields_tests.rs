//! Tests for soft fields of particles (particles part 5) and part 5's sprite shapes, on the
//! software GPU; skipped without a graphics adapter.

use crate::pictures::{pixel_sized, render_sized, save};
use crate::{FIELD_FLAME, FIELD_HEAT_STOPS, Field, FieldLook, Frame, Particle, Splat};
use bpe_runtime::CameraView;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

fn splats_round(centre: Vec2, n: usize, colour: [f32; 3]) -> Vec<Splat> {
    (0..n)
        .map(|i| {
            let a = i as f32 * 2.399;
            let r = 60.0 * ((i as f32 + 0.5) / n as f32).sqrt();
            Splat {
                place: [centre.x + r * a.cos(), centre.y + r * a.sin(), 30.0, 0.6],
                color: [colour[0], colour[1], colour[2], 0.0],
            }
        })
        .collect()
}

fn brightness(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}

#[test]
fn a_smoke_field_covers_where_its_splats_are_and_nothing_far_away() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let look = FieldLook { kind: [0.0, 0.0, 1.0, 2.0], ..FieldLook::default() };
    frame.fields.push(Field { at: 0, splats: splats_round(Vec2::ZERO, 40, [0.9, 0.2, 0.1]), look });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("field-smoke", &px, W, H);
    let middle = pixel_sized(&px, W, H, Vec2::ZERO);
    let far = pixel_sized(&px, W, H, Vec2::new(400.0, 800.0));
    assert!(middle[0] > 150 && middle[0] > middle[2] + 100, "the smoke's own colour in the middle: {middle:?}");
    assert!(brightness(far) < 5, "nothing far away: {far:?}");
}

#[test]
fn a_field_sits_between_the_sprites_its_at_says() {
    // A red dot, then the field (blue, thick), then a green dot: green over blue over red.
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    frame.particles.push(Particle::new(6, Vec2::new(-30.0, 0.0), 0.0, 40.0, [1.0, 0.0, 0.0, 1.0]));
    frame.particles.push(Particle::new(6, Vec2::new(30.0, 0.0), 0.0, 20.0, [0.0, 1.0, 0.0, 1.0]));
    let look = FieldLook { kind: [0.0, 0.0, 1.0, 20.0], ..FieldLook::default() };
    frame.fields.push(Field { at: 1, splats: splats_round(Vec2::ZERO, 60, [0.0, 0.0, 1.0]), look });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("field-between", &px, W, H);
    let under = pixel_sized(&px, W, H, Vec2::new(-40.0, 0.0));
    let over = pixel_sized(&px, W, H, Vec2::new(30.0, 0.0));
    assert!(under[2] > 120 && under[0] < 60, "the field covers the red dot: {under:?}");
    assert!(over[1] > 150 && over[2] < 60, "the green dot is over the field: {over:?}");
}

#[test]
fn a_flame_adds_its_heat_colours_hottest_where_most_meet() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let mut heat = [[0.0; 4]; FIELD_HEAT_STOPS];
    for (i, h) in heat.iter_mut().enumerate() {
        let x = i as f32 / (FIELD_HEAT_STOPS - 1) as f32;
        *h = [x * 2.0, x * x, x * x * x * 0.5, 1.0];
    }
    let look =
        FieldLook { kind: [FIELD_FLAME, 1.0, 1.0, 1.5], shade: [0.0, 0.0, 0.0, 1.0], heat, ..FieldLook::default() };
    frame.fields.push(Field { at: 0, splats: splats_round(Vec2::ZERO, 60, [1.0, 1.0, 1.0]), look });
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("field-flame", &px, W, H);
    let middle = pixel_sized(&px, W, H, Vec2::ZERO);
    let edge = pixel_sized(&px, W, H, Vec2::new(85.0, 0.0));
    let far = pixel_sized(&px, W, H, Vec2::new(400.0, 800.0));
    assert!(brightness(middle) > brightness(edge) + 60, "hotter in the middle: {middle:?} {edge:?}");
    assert!(middle[1] > edge[1], "yellower where hotter: {middle:?} {edge:?}");
    assert!(brightness(far) < 5, "{far:?}");
}

#[test]
fn a_field_moves_with_the_camera() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    frame.camera = CameraView { center: Vec2::new(200.0, 0.0), zoom: 2.0, ..CameraView::default() };
    let look = FieldLook { kind: [0.0, 0.0, 1.0, 4.0], ..FieldLook::default() };
    frame.fields.push(Field { at: 0, splats: splats_round(Vec2::new(200.0, 0.0), 40, [1.0, 1.0, 1.0]), look });
    let Some(px) = render_sized(&frame, W, H) else { return };
    // The splats' middle is now in the picture's middle, twice as big.
    let middle = pixel_sized(&px, W, H, Vec2::ZERO);
    let out = pixel_sized(&px, W, H, Vec2::new(-400.0, 0.0));
    assert!(brightness(middle) > 400, "{middle:?}");
    assert!(brightness(out) < 5, "{out:?}");
}

#[test]
fn part_five_shapes_cover_their_middles_and_leave_their_corners() {
    // Codes 10 (leaf) to 16 (diamond), each in its own spot.
    let place =
        |code: u32| Vec2::new(-360.0 + ((code - 10) % 4) as f32 * 240.0, -400.0 + ((code - 10) / 4) as f32 * 400.0);
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    for code in 10..=16u32 {
        let mut p = Particle::new(code, place(code), 0.0, 40.0, [1.0, 1.0, 1.0, 1.0]);
        p.style[0] = if code == 12 { 40.0 } else { 70.0 };
        p.shape[1] = 32768;
        frame.particles.push(p);
    }
    let Some(px) = render_sized(&frame, W, H) else { return };
    save("more-shapes", &px, W, H);
    for code in 10..=16u32 {
        let c = place(code);
        // A bird's middle is its body; a flake's its arms' meeting.
        let middle = pixel_sized(&px, W, H, c);
        let corner = pixel_sized(&px, W, H, c + Vec2::new(66.0, 38.0));
        assert!(brightness(middle) > 500, "code {code} covers its middle: {middle:?}");
        assert!(brightness(corner) < 100, "code {code} leaves its corner: {corner:?}");
    }
}

#[test]
fn a_birds_pose_moves_its_wing_tips() {
    let draw = |pose: u32| {
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let mut p = Particle::new(14, Vec2::ZERO, 0.0, 40.0, [1.0, 1.0, 1.0, 1.0]);
        p.style[0] = 120.0;
        p.shape[1] = pose;
        frame.particles.push(p);
        render_sized(&frame, W, H)
    };
    let (Some(up), Some(down)) = (draw(65535), draw(0)) else { return };
    // Wings up: the tips are above the body (y down); wings down: below it.
    let tip_high = Vec2::new(112.0, -53.0);
    let tip_low = Vec2::new(112.0, 37.0);
    assert!(brightness(pixel_sized(&up, W, H, tip_high)) > brightness(pixel_sized(&down, W, H, tip_high)));
    assert!(brightness(pixel_sized(&down, W, H, tip_low)) > brightness(pixel_sized(&up, W, H, tip_low)));
}
