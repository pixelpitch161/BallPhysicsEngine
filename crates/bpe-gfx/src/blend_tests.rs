//! Picture tests for blend layers and their composite, on the software GPU.

use crate::paint_tests::gpu;
use crate::{BackgroundPaint, BallInstance, BlendLayer, Frame, LiquidLayer, PaintGpu, Points, Renderer, TrailVertex};
use bpe_core::{Blend, ThingId};

const N: u32 = 64;
const D: [f32; 3] = [0.6, 0.3, 0.2];
const S: [f32; 3] = [0.2, 0.5, 0.9];

const ALL: [Blend; 12] = [
    Blend::Normal,
    Blend::Add,
    Blend::Screen,
    Blend::Multiply,
    Blend::Lighten,
    Blend::Darken,
    Blend::Subtract,
    Blend::Overlay,
    Blend::SoftLight,
    Blend::Difference,
    Blend::Hue,
    Blend::Colour,
];

/// A 64×64 world on a 64×64 picture, its background the flat colour `d`.
fn flat(d: [f32; 3]) -> Frame {
    let c = [d[0], d[1], d[2], 1.0];
    Frame {
        width: N,
        height: N,
        video: (N, N),
        bloom: 0.0,
        background: BackgroundPaint { row: 0, ends: Some([c, c]), ..BackgroundPaint::default() },
        ..Frame::default()
    }
}

/// A square `half` world pixels each way from the centre in one plain colour (alpha -1 adds it).
fn square(half: f32, colour: [f32; 4]) -> Vec<TrailVertex> {
    let v = |x: f32, y: f32| TrailVertex { position: [x, y], edge: [0.0, 1.0e4], color: colour };
    let (a, b, c, d) = (v(-half, -half), v(half, -half), v(half, half), v(-half, half));
    vec![a, b, c, a, c, d]
}

/// `frame` with one layer holding `trails`.
fn with_layer(mut frame: Frame, blend: Blend, amount: f32, trails: Vec<TrailVertex>) -> Frame {
    let layer = Frame { trails, ..flat([0.0; 3]) };
    frame.layers.push(BlendLayer { blend, amount, each: false, frame: layer });
    frame
}

fn render(renderer: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> Vec<u8> {
    renderer.render(device, queue, frame);
    renderer.read_output(device, queue).unwrap()
}

fn at(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * N + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= 2)
}

// ---- The composite worked out on the CPU ----

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_colour(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_colour(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    if x <= n {
        return [0.0; 3];
    }
    c.map(|v| (v - n) * s / (x - n))
}

fn mode(blend: Blend, d: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let (dc, sc) = (d.map(|v| v.clamp(0.0, 1.0)), s.map(|v| v.clamp(0.0, 1.0)));
    let each = |f: &dyn Fn(f32, f32) -> f32| [f(d[0], s[0]), f(d[1], s[1]), f(d[2], s[2])];
    let each_clamped = |f: &dyn Fn(f32, f32) -> f32| [f(dc[0], sc[0]), f(dc[1], sc[1]), f(dc[2], sc[2])];
    match blend {
        Blend::Normal => s,
        Blend::Add => each(&|d, s| d + s),
        Blend::Screen => each_clamped(&|d, s| 1.0 - (1.0 - d) * (1.0 - s)),
        Blend::Multiply => each(&|d, s| d * s),
        Blend::Lighten => each(&f32::max),
        Blend::Darken => each(&f32::min),
        Blend::Subtract => each(&|d, s| (d - s).max(0.0)),
        Blend::Overlay => each_clamped(&|d, s| {
            if d <= 0.5 { 2.0 * s * d } else { 1.0 - (1.0 - s) * (1.0 - (2.0 * d - 1.0)) }
        }),
        Blend::SoftLight => each_clamped(&|d, s| {
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else {
                let k = if d <= 0.25 { ((16.0 * d - 12.0) * d + 4.0) * d } else { d.sqrt() };
                d + (2.0 * s - 1.0) * (k - d)
            }
        }),
        Blend::Difference => each(&|d, s| (d - s).abs()),
        Blend::Hue => set_lum(set_sat(sc, sat(dc)), lum(dc)),
        Blend::Colour => set_lum(sc, lum(dc)),
    }
}

/// What the composite makes of the picture `d` under a layer pixel of premultiplied colour `c`
/// and coverage `a`.
fn composite(blend: Blend, amount: f32, d: [f32; 3], c: [f32; 3], a: f32) -> [f32; 3] {
    if blend == Blend::Add {
        return [0, 1, 2].map(|k| d[k] + c[k] * amount);
    }
    let light = c[0].max(c[1]).max(c[2]);
    let a = if a < 1e-4 && light > 0.0 { light.clamp(0.0, 1.0) } else { a };
    let s = c.map(|v| v / a.max(1e-4));
    let b = mode(blend, d, s);
    let t = a * amount;
    [0, 1, 2].map(|k| d[k] + (b[k] - d[k]) * t)
}

/// The centre pixel of a picture that is just the flat colour `c`: what a blend that makes `c`
/// should show.
fn expected(renderer: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, c: [f32; 3]) -> [u8; 4] {
    at(&render(renderer, device, queue, &flat(c)), N / 2, N / 2)
}

#[test]
fn each_blend_mode_matches_its_formula() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let plain = render(&mut renderer, &device, &queue, &flat(D));
    for blend in ALL {
        let frame = with_layer(flat(D), blend, 1.0, square(16.0, [S[0], S[1], S[2], 1.0]));
        let pixels = render(&mut renderer, &device, &queue, &frame);
        crate::pictures::save(&format!("blend_{blend:?}"), &pixels, N, N);
        let want = expected(&mut renderer, &device, &queue, composite(blend, 1.0, D, S, 1.0));
        let got = at(&pixels, N / 2, N / 2);
        assert!(close(got, want), "{blend:?}: {got:?}, the formula gives {want:?}");
        assert_eq!(at(&pixels, 2, 2), at(&plain, 2, 2), "{blend:?}: the corner the square misses");
    }
}

#[test]
fn blend_amount_zero_leaves_the_picture_unchanged() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let plain = render(&mut renderer, &device, &queue, &flat(D));
    let frame = with_layer(flat(D), Blend::Multiply, 0.0, square(40.0, [S[0], S[1], S[2], 1.0]));
    assert!(plain == render(&mut renderer, &device, &queue, &frame), "amount 0 looks exactly as if absent");
}

#[test]
fn normal_at_half_amount_is_half_way() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let frame = with_layer(flat(D), Blend::Normal, 0.5, square(40.0, [S[0], S[1], S[2], 1.0]));
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| (D[k] + S[k]) * 0.5));
    assert!(close(got, want), "{got:?}, half way is {want:?}");
}

#[test]
fn darken_does_not_touch_uncovered_pixels() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let plain = render(&mut renderer, &device, &queue, &flat(D));
    let frame = with_layer(flat(D), Blend::Darken, 1.0, square(10.0, [S[0], S[1], S[2], 1.0]));
    let pixels = render(&mut renderer, &device, &queue, &frame);
    for y in 0..N {
        for x in 0..N {
            // The square covers pixels 22 to 41; one pixel either side is its soft edge.
            let outside = !(20..=43).contains(&x) || !(20..=43).contains(&y);
            if outside {
                assert_eq!(at(&pixels, x, y), at(&plain, x, y), "({x}, {y}) is not under the square");
            }
        }
    }
    let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| D[k].min(S[k])));
    let got = at(&pixels, N / 2, N / 2);
    assert!(close(got, want), "{got:?}, darker of the two is {want:?}");
}

#[test]
fn an_additive_colour_in_a_layer_stays_finite() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    // Alpha -1: light that covers nothing, which the layer holds with coverage 0.
    let frame = with_layer(flat(D), Blend::Overlay, 1.0, square(40.0, [S[0], S[1], S[2], -1.0]));
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    let want = composite(Blend::Overlay, 1.0, D, S, 0.0);
    assert!(want.iter().all(|v| v.is_finite()));
    let want = expected(&mut renderer, &device, &queue, want);
    assert!(close(got, want), "{got:?}, the formula gives {want:?}");
    let plain = expected(&mut renderer, &device, &queue, D);
    assert!(got != plain && got[..3].iter().any(|&v| v > 0), "{got:?}: the light shows");
}

#[test]
fn a_stuck_layer_blends_over_the_overlay() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let o = [0.1, 0.8, 0.4];
    let stuck = Frame { trails: square(40.0, [o[0], o[1], o[2], 1.0]), ..flat([0.0; 3]) };
    let stuck = with_layer(stuck, Blend::Multiply, 1.0, square(40.0, [S[0], S[1], S[2], 1.0]));
    let frame = Frame { overlay: Some(Box::new(stuck)), ..flat(D) };
    assert!(frame.has_overlay());
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| o[k] * S[k]));
    assert!(close(got, want), "{got:?}: the stuck layer multiplies the stuck square, {want:?}");
}

#[test]
fn a_liquid_in_a_layer_draws_into_it() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let mut points = Vec::new();
    for row in 0..16 {
        for col in 0..16 {
            points.push([-15.0 + col as f32 * 2.0, -15.0 + row as f32 * 2.0, 0.0, 0.0]);
        }
    }
    let liquid = LiquidLayer {
        thing: ThingId(1),
        points: Points::Cpu(points),
        radius: 1.0,
        color: [0.1, 0.3, 1.0, 1.0],
        follow: 0,
        paint: PaintGpu::default(),
        paint_place: [0.0; 4],
        clarity: 0.0,
        refraction: 0.0,
        highlight: 0.0,
        smoothing: 1.0,
        glow: 0.0,
    };
    let black = flat([0.0; 3]);
    let mut frame = with_layer(black.clone(), Blend::Screen, 1.0, Vec::new());
    frame.layers[0].frame.liquids.push(liquid);
    let plain = render(&mut renderer, &device, &queue, &black);
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    assert!(got[2] > 100 && got[2] > got[0] + 40, "{got:?}: the layered liquid shows blue");
    assert_eq!(at(&plain, N / 2, N / 2)[2], 0);
}

/// A square block of liquid points 30 world pixels across, centred on `at`.
fn liquid_block(at: [f32; 2], clarity: f32) -> LiquidLayer {
    let mut points = Vec::new();
    for row in 0..16 {
        for col in 0..16 {
            points.push([at[0] - 15.0 + col as f32 * 2.0, at[1] - 15.0 + row as f32 * 2.0, 0.0, 0.0]);
        }
    }
    LiquidLayer {
        thing: ThingId(1),
        points: Points::Cpu(points),
        radius: 1.0,
        color: [0.1, 0.3, 1.0, 1.0],
        follow: 0,
        paint: PaintGpu::default(),
        paint_place: [0.0; 4],
        clarity,
        refraction: 0.0,
        highlight: 0.0,
        smoothing: 1.0,
        glow: 0.0,
    }
}

#[test]
fn a_layered_liquid_is_placed_through_the_frames_camera() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    // The layer was made through the camera the video had when the frame was built...
    let mut frame = with_layer(flat([0.0; 3]), Blend::Screen, 1.0, Vec::new());
    frame.layers[0].frame.liquids.push(liquid_block([200.0, 0.0], 0.0));
    // ...and the editor's World view then looks at the liquid through its own camera.
    frame.camera = bpe_runtime::CameraView { center: glam::Vec2::new(200.0, 0.0), zoom: 1.0, turn: 0.0 };
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    assert!(got[2] > 100 && got[2] > got[0] + 40, "{got:?}: the liquid shows where the frame's camera looks");
}

#[test]
fn a_clear_liquid_added_in_a_layer_does_not_add_the_picture_to_itself() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let grey = flat([0.2; 3]);
    let plain = at(&render(&mut renderer, &device, &queue, &grey), N / 2, N / 2);
    for blend in [Blend::Add, Blend::Difference, Blend::Screen] {
        let mut frame = with_layer(grey.clone(), blend, 1.0, Vec::new());
        frame.layers[0].frame.liquids.push(liquid_block([0.0, 0.0], 1.0));
        let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
        assert!(close(got, plain), "{blend:?}: {got:?} vs {plain:?}: a fully clear liquid adds nothing of its own");
    }
}

// ---- Blend with each other ----

/// `frame` with one layer holding `layer`'s lists, its things blending with each other when
/// `each` is set.
fn with_each(mut frame: Frame, blend: Blend, amount: f32, each: bool, layer: Frame) -> Frame {
    frame.layers.push(BlendLayer { blend, amount, each, frame: layer });
    frame
}

/// Plain lit balls (no halo) of colour `c` at each of `centres`, in a layer frame.
fn balls(centres: &[[f32; 2]], radius: f32, c: [f32; 3]) -> Frame {
    let balls = centres
        .iter()
        .map(|&center| BallInstance { center, radius, glow: 0.0, color: [c[0], c[1], c[2], 1.0], ..Default::default() })
        .collect();
    Frame { balls, ..flat([0.0; 3]) }
}

fn sum(p: [u8; 4]) -> i32 {
    p[..3].iter().map(|&v| v as i32).sum()
}

/// The centre pixel with ball A alone, ball B alone and both, the two balls overlapping there.
fn overlap(blend: Blend, d: [f32; 3], c: [f32; 3], each: bool) -> Option<([u8; 4], [u8; 4], [u8; 4])> {
    let (device, queue) = gpu()?;
    let mut renderer = Renderer::new(&device, N, N);
    let (a, b) = ([-8.0, 0.0], [8.0, 0.0]);
    let mut shot = |centres: &[[f32; 2]]| {
        let frame = with_each(flat(d), blend, 1.0, each, balls(centres, 14.0, c));
        at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2)
    };
    Some((shot(&[a]), shot(&[b]), shot(&[a, b])))
}

#[test]
fn two_overlapping_screen_balls_blending_with_each_other_are_brighter_where_they_overlap() {
    let Some((a, b, both)) = overlap(Blend::Screen, [0.05; 3], [0.3; 3], true) else { return };
    assert!(sum(both) > sum(a) + 6 && sum(both) > sum(b) + 6, "{both:?} vs {a:?} and {b:?}");
    let Some((_, b, both)) = overlap(Blend::Screen, [0.05; 3], [0.3; 3], false) else { return };
    assert!(close(both, b), "switch off: the later ball covers the first, {both:?} vs {b:?}");
}

#[test]
fn multiply_balls_with_each_other_darken_their_overlap() {
    let Some((a, b, both)) = overlap(Blend::Multiply, [0.8; 3], [0.5; 3], true) else { return };
    assert!(sum(both) + 6 < sum(a) && sum(both) + 6 < sum(b), "{both:?} vs {a:?} and {b:?}");
    let Some((_, b, both)) = overlap(Blend::Multiply, [0.8; 3], [0.5; 3], false) else { return };
    assert!(close(both, b), "switch off: {both:?} vs {b:?}");
}

/// Two squares covering the whole picture, `s1` under `s2`.
fn two_squares(s1: [f32; 3], s2: [f32; 3]) -> Frame {
    let mut trails = square(40.0, [s1[0], s1[1], s1[2], 1.0]);
    trails.extend(square(40.0, [s2[0], s2[1], s2[2], 1.0]));
    Frame { trails, ..flat([0.0; 3]) }
}

#[test]
fn add_with_each_other_matches_adding_both() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let (d, s1, s2) = ([0.1, 0.08, 0.05], [0.05, 0.1, 0.02], [0.1, 0.05, 0.08]);
    let frame = with_each(flat(d), Blend::Add, 1.0, true, two_squares(s1, s2));
    let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
    let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| d[k] + s1[k] + s2[k]));
    assert!(close(got, want), "{got:?}, d + s1 + s2 is {want:?}");
}

#[test]
fn every_mode_with_each_other_is_like_blending_each_in_turn() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let (d, s1, s2) = ([0.6, 0.3, 0.2], [0.2, 0.5, 0.1], [0.3, 0.2, 0.15]);
    for blend in ALL.into_iter().filter(|b| b.combines_with_itself()) {
        let frame = with_each(flat(d), blend, 1.0, true, two_squares(s1, s2));
        let got = at(&render(&mut renderer, &device, &queue, &frame), N / 2, N / 2);
        let want = expected(&mut renderer, &device, &queue, mode(blend, mode(blend, d, s1), s2));
        assert!(close(got, want), "{blend:?}: {got:?}, in turn gives {want:?}");
        let half = with_each(flat(d), blend, 0.5, true, two_squares(s1, s2));
        let got = at(&render(&mut renderer, &device, &queue, &half), N / 2, N / 2);
        let both = mode(blend, mode(blend, d, s1), s2);
        let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| (d[k] + both[k]) * 0.5));
        assert!(close(got, want), "{blend:?} at half: {got:?} vs {want:?}");
    }
}

#[test]
fn darken_each_leaves_uncovered_pixels_untouched() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let plain = render(&mut renderer, &device, &queue, &flat(D));
    for blend in ALL.into_iter().filter(|b| b.combines_with_itself()) {
        let mut layer = balls(&[[-6.0, 0.0], [6.0, 0.0]], 8.0, S);
        layer.trails = square(4.0, [S[0], S[1], S[2], 1.0]);
        let frame = with_each(flat(D), blend, 1.0, true, layer);
        let pixels = render(&mut renderer, &device, &queue, &frame);
        crate::pictures::save(&format!("blend_each_{blend:?}"), &pixels, N, N);
        for y in 0..N {
            for x in 0..N {
                // The balls reach from -14 to 14 across and -8 to 8 down; a pixel more is their
                // soft edge.
                let outside = !(16..=48).contains(&x) || !(22..=42).contains(&y);
                if outside {
                    assert_eq!(at(&pixels, x, y), at(&plain, x, y), "{blend:?}: ({x}, {y}) is not under a ball");
                }
            }
        }
        if blend == Blend::Darken {
            let got = at(&pixels, N / 2, N / 2);
            // The flat square there meets the lit (darker-edged) balls: at most the darker of the
            // picture and the square.
            let want = expected(&mut renderer, &device, &queue, [0, 1, 2].map(|k| D[k].min(S[k])));
            assert!((0..3).all(|k| got[k] <= want[k] + 2), "{got:?}, darker of the two is {want:?}");
        }
    }
}

#[test]
fn the_switch_on_a_mode_that_cannot_combine_changes_nothing() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let layer = two_squares(S, [0.1, 0.2, 0.3]);
    let off = render(&mut renderer, &device, &queue, &with_each(flat(D), Blend::Overlay, 1.0, false, layer.clone()));
    let on = render(&mut renderer, &device, &queue, &with_each(flat(D), Blend::Overlay, 1.0, true, layer));
    assert!(off == on);
}

#[test]
fn every_kind_of_thing_can_blend_with_each_other() {
    use crate::{FlashInstance, GlyphInstance, LookInstance, ShapeInstance, SoftVertex};
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    let plain = render(&mut renderer, &device, &queue, &flat(D));
    for blend in ALL.into_iter().filter(|b| b.combines_with_itself()) {
        let mut layer = balls(&[[0.0, 0.0]], 6.0, S);
        layer.trails = square(4.0, [S[0], S[1], S[2], 1.0]);
        layer.shapes.push(ShapeInstance::disc(glam::Vec2::ZERO, 5.0, [S[0], S[1], S[2], 1.0], 0.0));
        layer.looks.push(LookInstance { radius: 5.0, color: [S[0], S[1], S[2], 1.0], ..LookInstance::default() });
        layer.flashes.push(FlashInstance { center: [0.0; 2], size: 4.0, color: [0.1, 0.1, 0.1, 1.0] });
        layer.glyphs.push(GlyphInstance::default());
        layer.soft.extend([SoftVertex::default(); 3]);
        let frame = with_each(flat(D), blend, 1.0, true, layer);
        let pixels = render(&mut renderer, &device, &queue, &frame);
        assert_eq!(at(&pixels, 2, 2), at(&plain, 2, 2), "{blend:?}: the corner nothing reaches");
        assert!(at(&pixels, N / 2, N / 2) != at(&plain, N / 2, N / 2), "{blend:?}: the middle changes");
    }
}
