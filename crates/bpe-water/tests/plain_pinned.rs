//! The plain drawing keeps drawing the pictures it draws: two sets of drops laid out by hand
//! (`draw::sets`, never physics) drawn at 270 x 480, their 10 x 10 pixel tile averages in
//! `tests/golden-plain.txt`, made on the owner's graphics card, named on the file's first line
//! (`bpe_gfx::card_golden`). Each tile may move by at most 1/255 per channel. Another card draws a
//! few bits differently, so there the test only checks that the pictures are drawn.
//!
//! One file for debug and release: no physics is involved. Refresh it on the owner's card with the
//! "Refresh GPU pictures" workflow, and only when a change is meant to move the plain drawing.

use bpe_gfx::{CardGolden, card_golden};
use bpe_water::draw::sets::{self, REST};
use bpe_water::draw::{Plain, PlainGpu, StoredDrops, View};

const W: u32 = 270;
const H: u32 = 480;
const TILE: u32 = 10;

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
fn the_plain_drawing_draws_as_pinned() {
    let Ok((device, queue)) = bpe_gfx::test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let gpu = PlainGpu::new(&device, &queue);
    let view = View::screen(0.25);
    assert_eq!((view.width, view.height), (W, H));
    let look = sets::look();
    let bench = sets::bench(8.0);
    let pool = sets::screen_pool(8.0, 0.4);
    let mut lines = String::from("# set\ttiles\n");
    for (name, set) in [("bench", &bench), ("screen_pool", &pool)] {
        let stored = StoredDrops::new(&device, &queue, &set.drops);
        let mut plain = Plain::new(gpu.clone(), &set.field, REST);
        plain.draw(&stored.buffers(), &view, &look);
        let tiles = tiles(&plain.read());
        // Whatever the card, the picture is drawn: the tiles are not all one colour.
        assert!(tiles.iter().any(|t| *t != tiles[0]), "{name} is drawn");
        lines += &format!("{name}\t{}\n", hex(&tiles));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden-plain.txt");
    let (expected, driver_note) = match card_golden(&path, &lines) {
        CardGolden::Blessed => return,
        CardGolden::NotCompared(why) => return eprintln!("{why}"),
        CardGolden::Compare { expected, driver_note } => (expected, driver_note.unwrap_or_default()),
    };
    assert_eq!(expected.lines().count(), lines.lines().count(), "{driver_note}");
    for (was, now) in expected.lines().zip(lines.lines()).skip(1) {
        let (a, b): (Vec<&str>, Vec<&str>) = (was.split('\t').collect(), now.split('\t').collect());
        assert_eq!(a[0], b[0]);
        let (ta, tb) = (unhex(a[1]), unhex(b[1]));
        let worst = ta
            .iter()
            .zip(&tb)
            .enumerate()
            .map(|(i, (x, y))| (i, (0..4).map(|c| (x[c] as i32 - y[c] as i32).abs()).max().unwrap()))
            .max_by_key(|(_, d)| *d)
            .unwrap();
        assert!(
            worst.1 <= 1,
            "{}: tile {} (x {}, y {}) moved by {}/255 {driver_note}",
            a[0],
            worst.0,
            worst.0 as u32 % (W / TILE),
            worst.0 as u32 / (W / TILE),
            worst.1
        );
    }
}
