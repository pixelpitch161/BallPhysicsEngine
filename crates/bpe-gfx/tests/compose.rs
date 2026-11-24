//! The compose pass on the graphics card matches its CPU twin (`compose::reference`).

use bpe_gfx::compose::{Compose, Look, overlay_params, overlay_reference, params, reference};
use bpe_gfx::{OUTPUT_FORMAT, Readback, test_device};

fn flat(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flat"),
        size: wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: Vec<u8> = (0..16 * 16).flat_map(|_| rgba).collect();
    queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64), rows_per_image: Some(16) },
        wgpu::Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
    );
    texture.create_view(&Default::default())
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, compose: &Compose) -> Vec<u8> {
    let (w, h) = (compose.output_texture().width(), compose.output_texture().height());
    let mut readback = Readback::new(device, w, h, 1);
    assert!(readback.push_texture(device, queue, compose.output_texture(), compose.output_view()).unwrap().is_none());
    readback.finish(device).unwrap().pop().unwrap()
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[test]
fn the_shader_matches_the_reference() {
    let (device, queue) = test_device().unwrap();
    let (red, blue) = (flat(&device, &queue, [255, 0, 0, 255]), flat(&device, &queue, [0, 0, 255, 255]));
    let (w, h) = (32u32, 64u32);
    let mut compose = Compose::new(&device, &queue, w, h);
    let looks = [
        None,
        Some(Look::Crossfade),
        Some(Look::FadeThrough([0.0, 0.0, 0.0])),
        Some(Look::Flash([1.0, 1.0, 1.0])),
        Some(Look::Wipe { direction: 2, softness: 0.05 }),
        Some(Look::Wipe { direction: 0, softness: 0.0 }),
        Some(Look::Push { direction: 3 }),
        Some(Look::Push { direction: 1 }),
        Some(Look::ZoomThrough),
    ];
    for look in looks {
        for p in [0.25, 0.5, 0.75] {
            let params = params(look, p, WHOLE, WHOLE);
            compose.draw(&device, &queue, Some(&red), Some(&blue), &params);
            let pixels = read(&device, &queue, &compose);
            for y in (0..h).step_by(3) {
                for x in (0..w).step_by(3) {
                    let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                    let want = reference(&params, uv, |_| RED, |_| BLUE);
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        let got = pixels[i + c] as f32 / 255.0;
                        assert!(
                            (got - want[c]).abs() <= 2.0 / 255.0,
                            "{look:?} at {p}, pixel ({x}, {y}) channel {c}: {got} vs {}",
                            want[c]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn push_texture_reads_back_a_compose() {
    let (device, queue) = test_device().unwrap();
    let (red, blue) = (flat(&device, &queue, [255, 0, 0, 255]), flat(&device, &queue, [0, 0, 255, 255]));
    let mut compose = Compose::new(&device, &queue, 8, 8);
    compose.draw(&device, &queue, Some(&red), Some(&blue), &params(Some(Look::Crossfade), 0.5, WHOLE, WHOLE));
    let pixels = read(&device, &queue, &compose);
    assert_eq!(pixels.len(), 8 * 8 * 4);
    for px in pixels.chunks(4) {
        assert!((px[0] as i32 - 128).abs() <= 1 && px[1] == 0 && (px[2] as i32 - 128).abs() <= 1, "{px:?}");
    }
    // Nothing on either side is black.
    compose.draw(&device, &queue, None, None, &params(None, 0.0, WHOLE, WHOLE));
    assert!(read(&device, &queue, &compose).chunks(4).all(|px| px == [0, 0, 0, 255]));
}

#[test]
fn the_overlay_shader_matches_its_reference() {
    let (device, queue) = test_device().unwrap();
    let (red, blue) = (flat(&device, &queue, [255, 0, 0, 255]), flat(&device, &queue, [0, 0, 255, 255]));
    let (w, h) = (48u32, 80u32);
    let mut compose = Compose::new(&device, &queue, w, h);
    for (rect, corner, border, alpha) in [
        ([6.0, 10.0, 30.0, 50.0], 8.0, 3.0, 1.0),
        ([0.0, 0.0, 24.0, 40.0], 0.0, 0.0, 0.5),
        ([10.5, 20.0, 37.5, 60.0], 40.0, 1.5, 0.8),
    ] {
        compose.draw(&device, &queue, None, Some(&red), &params(None, 0.0, WHOLE, WHOLE));
        let p = overlay_params(rect, WHOLE, corner, border, [0.0, 1.0, 0.0], alpha);
        compose.overlay(&device, &queue, &blue, &p);
        let pixels = read(&device, &queue, &compose);
        for y in 0..h {
            for x in 0..w {
                let want = overlay_reference(&p, [x as f32 + 0.5, y as f32 + 0.5], RED, |_| BLUE);
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    let got = pixels[i + c] as f32 / 255.0;
                    assert!(
                        (got - want[c]).abs() <= 2.0 / 255.0,
                        "{rect:?}, pixel ({x}, {y}) channel {c}: {got} vs {}",
                        want[c]
                    );
                }
                assert_eq!(pixels[i + 3], 255, "the output stays opaque");
            }
        }
    }
}
