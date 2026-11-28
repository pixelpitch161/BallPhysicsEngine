//! The ready-made Water looks keep drawing the pictures they draw (Water mode part 6c): part 3's
//! pieces course (`bpe_water::draw::sets::pieces`, drops laid out by hand, never physics) in a
//! scene of two Liquids, set to each of the eight looks with `apply_look` and drawn by the look
//! drawing at 270 x 480. The 10 x 10 pixel tile averages are in `tests/golden-water-looks.txt`,
//! made on the owner's graphics card, named on the file's first line (`bpe_gfx::card_golden`).
//! Each tile may move by at most 1/255 per channel. Another card draws a few bits differently, so
//! there the test only checks that the looks draw, and draw differently from each other.
//!
//! One file for debug and release: no physics is involved. Refresh it on the owner's card with the
//! "Refresh GPU pictures" workflow, and only when a change is meant to move a look.

use bpe_core::ThingId;
use bpe_gfx::{CardGolden, card_golden};
use bpe_kit::modules::water::{Keep, ReadyLook, apply_look};
use bpe_kit::things::water::{LIQUID, new_water_scene};
use bpe_water::draw::sets::{self, REST};
use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};

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
fn every_ready_look_draws_as_pinned() {
    let Ok((device, queue)) = bpe_gfx::test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let reg = bpe_kit::kit().registry;
    let gpu = LookGpu::new(&device, &queue);
    let view = View::screen(0.25);
    assert_eq!((view.width, view.height), (W, H));
    let set = sets::pieces(8.0);
    let stored = StoredDrops::new(&device, &queue, &set.drops);
    let mut drawing = LookDraw::new(gpu, &set.field, REST);
    // (The pool is the first Liquid, the basin's water the second; the pieces draw in the
    // course's Rock, which every look sets as it sets their Materials.)
    let mut base = new_water_scene(&reg, "Pinned looks");
    for _ in 0..2 {
        bpe_kit::add_thing(&mut base, &reg, LIQUID, ThingId::ROOT).unwrap();
    }
    let mut lines = String::from("# look\ttiles\n");
    let mut seen: Vec<Vec<[u8; 4]>> = Vec::new();
    for look in ReadyLook::ALL {
        let mut scene = base.clone();
        apply_look(&mut scene, &reg, look, Keep::default());
        let resolved = bpe_core::resolve(&scene, &reg);
        let l = bpe_kit::things::water::look::look_of(&scene, &resolved, 0.5);
        drawing.draw(&stored.buffers(), &view, &l);
        let tiles = tiles(&drawing.read());
        // Whatever the card, the look is drawn, and is a look of its own.
        assert!(tiles.iter().any(|t| *t != tiles[0]), "{} is drawn", look.title());
        assert!(!seen.contains(&tiles), "{} draws like another look", look.title());
        lines += &format!("{}\t{}\n", look.title(), hex(&tiles));
        seen.push(tiles);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden-water-looks.txt");
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
