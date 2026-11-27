//! Picture tests for the last pass (`post.wgsl::fs_composite`): colours up to 1 are kept (white is
//! white), brighter ones keep their hue, and the dither breaks up banding without moving the mean
//! or speckling pure black and white. On the software GPU.

use crate::paint_tests::gpu;
use crate::{BackgroundPaint, Frame, Renderer};

const N: u32 = 64;

/// A 64×64 picture of one flat background colour (linear light), no bloom.
fn flat(c: [f32; 3]) -> Frame {
    let c = [c[0], c[1], c[2], 1.0];
    Frame {
        width: N,
        height: N,
        video: (N, N),
        bloom: 0.0,
        background: BackgroundPaint { row: 0, ends: Some([c, c]), mode: 0.0, ..BackgroundPaint::default() },
        ..Frame::default()
    }
}

fn draw(frame: &Frame) -> Option<Vec<[u8; 3]>> {
    let (device, queue) = gpu()?;
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    Some(pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).collect())
}

#[test]
fn pure_white_and_pure_black_come_out_exact() {
    let Some(white) = draw(&flat([1.0; 3])) else { return };
    assert!(white.iter().all(|p| *p == [255; 3]), "white: {:?}", white.iter().find(|p| **p != [255; 3]));
    let Some(black) = draw(&flat([0.0; 3])) else { return };
    assert!(black.iter().all(|p| *p == [0; 3]), "black: {:?}", black.iter().find(|p| **p != [0; 3]));
}

#[test]
fn a_dark_grey_is_dithered_round_its_own_value() {
    // Linear 0.01 is sRGB 25.2 steps: between two steps, so the dither must mix 25 and 26.
    let Some(grey) = draw(&flat([0.01; 3])) else { return };
    let values: Vec<f64> = grey.iter().map(|p| p[1] as f64).collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let want = (1.055 * 0.01f64.powf(1.0 / 2.4) - 0.055) * 255.0;
    assert!((mean - want).abs() < 0.25, "mean {mean} against {want}");
    let below = values.iter().filter(|v| **v <= want.floor()).count();
    assert!(below > 0 && below < values.len(), "{below} of {} pixels at or under {want}", values.len());
    // The same on every channel: a grey stays grey.
    assert!(grey.iter().all(|p| p[0] == p[1] && p[1] == p[2]), "{:?}", grey.iter().find(|p| p[0] != p[1]));
}

#[test]
fn an_overbright_colour_keeps_its_hue_and_burns_toward_white() {
    let Some(blue) = draw(&flat([0.3, 0.2, 3.0])) else { return };
    let p = blue[(N * N / 2 + N / 2) as usize];
    assert!(p[2] >= 254, "blue at the top: {p:?}");
    assert!(p[0] > p[1] && (p[0] as i32) < p[2] as i32 - 40, "still clearly blue, red above green: {p:?}");
    // A little over 1 is barely whiter; far over, nearly white.
    let Some(hot) = draw(&flat([0.3, 0.2, 30.0])) else { return };
    let q = hot[(N * N / 2 + N / 2) as usize];
    assert!(q[0] > p[0] + 40 && q[1] > p[1] + 40, "hotter is whiter: {p:?} then {q:?}");
}

/// Review S23: the export's video colours worked out on the graphics card are the CPU's
/// (`yuv::yuv_of_rgba`) of the same RGBA picture, within one 8-bit step, for both layouts and for
/// sizes whose rows aren't a whole number of copy rows.
#[test]
fn video_colours_from_the_card_match_the_cpus() {
    use crate::Readback;
    use crate::yuv::{PixelLayout, yuv_of_rgba};
    let Some((device, queue)) = gpu() else { return };
    for (w, h) in [(64u32, 48u32), (62, 34)] {
        let frame = Frame {
            width: w,
            height: h,
            video: (w, h),
            bloom: 0.0,
            background: BackgroundPaint {
                row: 0,
                ends: Some([[1.0, 0.1, 0.0, 1.0], [0.0, 0.3, 1.0, 1.0]]),
                mode: 0.0,
                ..BackgroundPaint::default()
            },
            ..Frame::default()
        };
        let mut renderer = Renderer::new(&device, w, h);
        renderer.render(&device, &queue, &frame);
        let rgba = renderer.read_output(&device, &queue).unwrap();
        for layout in [PixelLayout::Yuv420p, PixelLayout::Nv12] {
            let mut readback = Readback::with_layout(&device, w, h, 2, layout);
            assert_eq!(readback.layout(), layout);
            let mut got = readback.push(&device, &queue, &renderer).unwrap();
            if got.is_none() {
                got = readback.finish(&device).unwrap().pop();
            }
            let got = got.expect("a frame");
            let want = yuv_of_rgba(&rgba, w, h, layout);
            assert_eq!(got.len(), (w * h * 3 / 2) as usize);
            assert_eq!(got.len(), want.len());
            let worst = got.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 1, "{layout:?} {w}x{h}: off by {worst}");
            // Not all one colour: the planes really carry the picture.
            assert!(want.iter().any(|&v| v != want[0]));
        }
    }
}

#[test]
fn odd_sizes_come_back_as_rgba() {
    use crate::yuv::PixelLayout;
    assert!(!PixelLayout::Yuv420p.fits(63, 48));
    assert!(PixelLayout::Rgba.fits(63, 47));
    let Some((device, _)) = gpu() else { return };
    assert_eq!(crate::Readback::with_layout(&device, 63, 48, 1, PixelLayout::Nv12).layout(), PixelLayout::Rgba);
}

/// Water mode part 6: a picture drawn elsewhere into the HDR target is finished as a Balls frame's
/// own picture is (the same grey to the bit), and its frame's Bloom spreads it.
#[test]
fn a_picture_drawn_elsewhere_is_finished_as_a_frames_own() {
    use crate::GivenPicture;
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, N, N);
    renderer.render(&device, &queue, &flat([0.5; 3]));
    let want = renderer.read_output(&device, &queue).unwrap();

    // Half floats: 0.5 is 0x3800, 8 is 0x4800.
    let fill = |at: &dyn Fn(u32, u32) -> u16| -> Vec<u8> {
        let mut out = Vec::with_capacity((N * N * 8) as usize);
        for y in 0..N {
            for x in 0..N {
                let v = at(x, y);
                for c in [v, v, v, 0x3c00] {
                    out.extend_from_slice(&c.to_le_bytes());
                }
            }
        }
        out
    };
    let finish = |renderer: &mut Renderer, pixels: &[u8], bloom: f32| -> Vec<u8> {
        queue.write_texture(
            renderer.targets.hdr_texture.as_image_copy(),
            pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(N * 8), rows_per_image: None },
            wgpu::Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        );
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("given") });
        let frame = Frame { bloom, ..flat([0.0; 3]) };
        renderer.finish_picture(&device, &queue, encoder, &frame, GivenPicture::default());
        renderer.read_output(&device, &queue).unwrap()
    };
    let got = finish(&mut renderer, &fill(&|_, _| 0x3800), 0.0);
    assert_eq!(got, want, "a grey finished from the HDR target is the frame's own grey");

    // A bright dot in the dark: Bloom 0 leaves the dark dark, Bloom 1 spreads the dot.
    let dot = fill(&|x, y| if (31..33).contains(&x) && (31..33).contains(&y) { 0x4800 } else { 0 });
    let near = ((32 * N + 37) * 4) as usize;
    let plain = finish(&mut renderer, &dot, 0.0);
    assert_eq!(plain[near], 0, "no bloom: the dark beside the dot stays black");
    let bloomed = finish(&mut renderer, &dot, 1.0);
    assert!(bloomed[near] > 8, "bloom spreads the dot: {} beside it", bloomed[near]);
}
