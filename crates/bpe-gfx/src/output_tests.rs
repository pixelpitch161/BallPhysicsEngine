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
