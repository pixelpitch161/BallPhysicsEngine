//! Flat lighting with Matte surfaces draws the picture lighting part 1a drew: the tile averages in
//! `tests/golden-lit-flat*.txt` were made on main before part 2a (Stage and surfaces) changed the
//! light pass, on the cloud's software GPU (lavapipe). Each 10 × 10 pixel tile may move by at most
//! 1/255 per channel (part 2a rewrote the pass; the sums may round a hair differently). Another GPU
//! draws a few bits differently, so there the test only checks that the scenes draw.
//!
//! Bless with `BLESS_GOLDEN=1`, once in debug and once with `--release` (the frames come from the
//! physics, whose trig differs a hair between the two), and only when a change is meant to move
//! Flat lit pictures.

use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;
const TILE: u32 = 10;
const MOMENTS: [f64; 2] = [0.0, 1.5];

/// The average of each channel over each tile, row by row.
fn tiles(pixels: &[u8]) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for ty in 0..H / TILE {
        for tx in 0..W / TILE {
            let mut sum = [0u32; 4];
            for y in ty * TILE..(ty + 1) * TILE {
                for x in tx * TILE..(tx + 1) * TILE {
                    let i = ((y * W + x) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += pixels[i + c] as u32;
                    }
                }
            }
            let n = TILE * TILE;
            out.push(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    out
}

fn hex(tiles: &[[u8; 4]]) -> String {
    tiles.iter().flat_map(|t| t.iter().map(|c| format!("{c:02x}"))).collect()
}

fn unhex(s: &str) -> Vec<[u8; 4]> {
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
    bytes.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect()
}

#[test]
fn flat_matte_lighting_draws_as_part_1a_did() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, W, H);
    let mut lines = String::from("# scene\tmoment\ttiles\n");
    for (name, scene) in [("lit_room", scenes::lit_room(&kit.registry)), ("lit_demo", scenes::lit_demo(&kit.registry))] {
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in MOMENTS {
            playback.seek(t);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
            assert!(frame.light.is_some(), "{name} is lit");
            renderer.render(&device, &queue, &frame);
            if renderer.programs_waiting() {
                renderer.render(&device, &queue, &frame);
            }
            let pixels = renderer.read_output(&device, &queue).unwrap();
            lines += &format!("{name}\t{t}\t{}\n", hex(&tiles(&pixels)));
        }
    }
    let file = if cfg!(debug_assertions) { "tests/golden-lit-flat-debug.txt" } else { "tests/golden-lit-flat.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    if std::env::var_os("BPE_ANY_GPU").is_some() {
        return;
    }
    assert_eq!(expected.lines().count(), lines.lines().count());
    for (was, now) in expected.lines().zip(lines.lines()).skip(1) {
        let (a, b): (Vec<&str>, Vec<&str>) = (was.split('\t').collect(), now.split('\t').collect());
        assert_eq!(a[..2], b[..2]);
        let (ta, tb) = (unhex(a[2]), unhex(b[2]));
        let worst = ta
            .iter()
            .zip(&tb)
            .enumerate()
            .map(|(i, (x, y))| (i, (0..4).map(|c| (x[c] as i32 - y[c] as i32).abs()).max().unwrap()))
            .max_by_key(|(_, d)| *d)
            .unwrap();
        assert!(
            worst.1 <= 1,
            "{} at {}: tile {} (x {}, y {}) moved by {}/255",
            a[0],
            a[1],
            worst.0,
            worst.0 as u32 % (W / TILE),
            worst.0 as u32 / (W / TILE),
            worst.1
        );
    }
}
