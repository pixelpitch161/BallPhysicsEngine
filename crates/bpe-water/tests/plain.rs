//! The plain drawing (`bpe_water::draw::plain`): what it draws where, that it repeats, and its look's
//! parts (depth, the rims, the rock), from drops laid out by hand (`draw::sets`), never physics.

use std::sync::Arc;

use bpe_water::draw::sets::{self, REST};
use bpe_water::draw::{Plain, PlainGpu, PlainLiquid, PlainLook, StoredDrops, View};
use bpe_water::{Drop, Feel, Layout, Numbers, SolidField, WATER, Water, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<PlainGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(PlainGpu::new(&device, &queue))
}

/// A picture: rgba8 sRGB, `width` x `height`.
struct Picture {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    view: View,
}

impl Picture {
    /// The pixel at world point `p`.
    fn at(&self, p: Vec2) -> [u8; 3] {
        let q = (p - self.view.lo) * self.view.scale;
        let (x, y) = (q.x.floor() as u32, q.y.floor() as u32);
        assert!(x < self.width && y < self.height, "{p} is in the picture");
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }

    fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2]]
    }
}

fn draw_with(gpu: &Arc<PlainGpu>, field: &SolidField, drops: &[Drop], view: View, look: &PlainLook) -> Picture {
    let stored = StoredDrops::new(gpu.device(), gpu.queue(), drops);
    let mut plain = Plain::new(gpu.clone(), field, REST);
    plain.draw(&stored.buffers(), &view, look);
    let pixels = plain.read();
    assert_eq!(pixels.len(), (view.width * view.height * 4) as usize);
    Picture { pixels, width: view.width, height: view.height, view }
}

fn draw(gpu: &Arc<PlainGpu>, field: &SolidField, drops: &[Drop], view: View) -> Picture {
    draw_with(gpu, field, drops, view, &sets::look())
}

/// The largest difference of one channel.
fn apart(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap()
}

fn srgb8(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v * 255.0).round() as u8)
}

fn luma(c: [u8; 3]) -> f32 {
    0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])
}

/// An open course (no solids) round the screen at `cell`.
fn open(cell: f32) -> SolidField {
    SolidField::build(Layout::cover(Vec2::new(-540.0, -960.0), Vec2::new(540.0, 960.0), cell), &[])
}

const BACK: fn() -> [u8; 3] = || srgb8(PlainLook::default().back);

/// A block of drops is drawn over the block and nowhere far from it.
#[test]
fn a_pool_is_drawn_where_its_drops_are() {
    let Some(gpu) = card() else { return };
    let field = open(8.0);
    let (lo, hi) = (Vec2::new(-200.0, -100.0), Vec2::new(200.0, 100.0));
    let drops = sets::lattice(&field, lo, hi, 0);
    let pic = draw(&gpu, &field, &drops, View::screen(0.5));
    let back = BACK();
    let mut y = -300.0;
    while y < 300.0 {
        let mut x = -400.0;
        while x < 400.0 {
            let p = Vec2::new(x, y);
            let c = pic.at(p);
            if p.cmpgt(lo + 4.0).all() && p.cmplt(hi - 4.0).all() {
                assert!(apart(c, back) > 40, "liquid at {p}: {c:?}");
            } else if p.cmplt(lo - 8.0).any() || p.cmpgt(hi + 8.0).any() {
                assert!(apart(c, back) <= 1, "back at {p}: {c:?}");
            }
            x += 2.0;
        }
        y += 2.0;
    }
}

/// The same drops in the same order draw the same picture, to the bit.
#[test]
fn the_same_drops_draw_the_same_picture_twice() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    let a = draw(&gpu, &bench.field, &bench.drops, View::screen(0.25));
    let b = draw(&gpu, &bench.field, &bench.drops, View::screen(0.25));
    assert!(a.pixels == b.pixels, "two drawings differ");
}

/// With no drops the rock and the back are still drawn.
#[test]
fn an_empty_body_draws_the_rock_and_the_back() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    let pic = draw(&gpu, &bench.field, &[], View::screen(0.5));
    let rock = srgb8(PlainLook::default().rock);
    assert!(apart(pic.at(Vec2::new(250.0, -280.0)), rock) <= 2, "the disc's middle is rock");
    assert!(apart(pic.at(Vec2::new(-400.0, -800.0)), BACK()) <= 1, "the box's corner is back");
}

/// A lone drop shows, a picture pixel or more, however far out the view is.
#[test]
fn a_lone_drop_shows_at_any_zoom() {
    let Some(gpu) = card() else { return };
    let field = open(2.0);
    let at = Vec2::new(13.3, 7.7);
    let drops = [Drop::new(field.layout.to_grid(at), Vec2::ZERO, 0, 0, WATER)];
    for scale in [1.0f32, 0.25, 0.05] {
        let view = View { lo: at - Vec2::splat(20.0 / scale), scale, width: 40, height: 40 };
        let pic = draw(&gpu, &field, &drops, view);
        let shown =
            (0..40).flat_map(|y| (0..40).map(move |x| (x, y))).filter(|&(x, y)| apart(pic.px(x, y), BACK()) > 20);
        let near: Vec<(u32, u32)> = shown
            .filter(|&(x, y)| (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - Vec2::splat(20.0)).length() <= 2.5)
            .collect();
        assert!(!near.is_empty(), "the drop shows at scale {scale}");
    }
}

/// A drop that is not a number, or past the layout (it left the course), is not drawn.
#[test]
fn drops_that_left_are_not_drawn() {
    let Some(gpu) = card() else { return };
    let field = open(8.0);
    let gone = [
        Drop::new(Vec2::new(f32::NAN, 40.0), Vec2::ZERO, 0, 0, WATER),
        Drop::new(Vec2::new(-3.0, 50.0), Vec2::ZERO, 0, 1, WATER),
        Drop::new(Vec2::new(40.0, field.layout.rows as f32 + 2.0), Vec2::ZERO, 0, 2, WATER),
    ];
    let with = draw(&gpu, &field, &gone, View::screen(0.25));
    let without = draw(&gpu, &field, &[], View::screen(0.25));
    assert!(with.pixels == without.pixels);
}

/// Air (part 2c) is not drawn: a pool with a pocket of air in it draws as the same pool with the
/// pocket empty, to the bit, and the pocket shows as a hole.
#[test]
fn air_is_not_drawn() {
    let Some(gpu) = card() else { return };
    let field = open(8.0);
    let (lo, hi) = (Vec2::new(-200.0, -100.0), Vec2::new(200.0, 100.0));
    let pool = sets::lattice(&field, lo, hi, 0);
    let in_pocket = |d: &Drop| field.layout.to_world(Vec2::from(d.x)).length() < 48.0;
    let with_air: Vec<Drop> =
        pool.iter().map(|d| if in_pocket(d) { Drop { kind: d.kind | bpe_water::AIR, ..*d } } else { *d }).collect();
    let without: Vec<Drop> = pool.iter().filter(|d| !in_pocket(d)).copied().collect();
    let a = draw(&gpu, &field, &with_air, View::screen(0.5));
    let b = draw(&gpu, &field, &without, View::screen(0.5));
    assert!(a.pixels == b.pixels, "air draws nothing");
    assert!(apart(a.at(Vec2::ZERO), BACK()) <= 1, "the pocket is a hole: {:?}", a.at(Vec2::ZERO));
}

/// Pictures of odd, tiny and full sizes draw.
#[test]
fn odd_and_tiny_pictures_draw() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    for (w, h) in [(1u32, 1u32), (3, 7), (1081, 1921)] {
        let view = View { lo: Vec2::new(-540.0, -960.0), scale: w as f32 / 1080.0, width: w, height: h };
        let pic = draw(&gpu, &bench.field, &bench.drops, view);
        assert_eq!(pic.pixels.len(), (w * h * 4) as usize);
    }
    // Wider than the card's widest picture: drawn as wide as it can be, never a panic.
    let most = gpu.device().limits().max_texture_dimension_2d;
    let view = View { lo: Vec2::new(-540.0, 0.0), scale: (most + 100) as f32 / 1080.0, width: most + 100, height: 1 };
    let stored = StoredDrops::new(gpu.device(), gpu.queue(), &bench.drops);
    let mut plain = Plain::new(gpu.clone(), &bench.field, REST);
    plain.draw(&stored.buffers(), &view, &sets::look());
    assert_eq!(plain.read().len(), (most * 4) as usize);
}

/// Water deep in a pool is nearer its Deep colour than a thin stream, and it darkens going down from
/// the top.
#[test]
fn deep_water_is_darker_than_thin_water() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(3.0);
    let pic = draw(&gpu, &bench.field, &bench.drops, View::screen(1.0));
    let water = PlainLiquid::default();
    let deep = srgb8(water.deep);
    let stream = pic.at(Vec2::new(60.0, -300.0));
    let low = pic.at(Vec2::new(200.0, 820.0));
    assert!(apart(low, deep) + 30 < apart(stream, deep), "deep {low:?} against the stream's {stream:?}");
    let mut last = f32::MAX;
    for d in [12.0f32, 60.0, 120.0, 200.0] {
        let l = luma(pic.at(Vec2::new(200.0, 445.0 + d)));
        assert!(l < last, "darker {d} px down: {l} after {last}");
        last = l;
    }
}

/// The pool's top: lighter just inside its outline than a little further in.
#[test]
fn the_rim_is_lighter_than_the_body_under_it() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(3.0);
    let pic = draw(&gpu, &bench.field, &bench.drops, View::screen(1.0));
    // (The pool's top at x = 0 is at 430.)
    let top = (420..460).map(|y| y as f32).find(|&y| apart(pic.at(Vec2::new(0.5, y + 0.5)), BACK()) > 40).unwrap();
    let rim = luma(pic.at(Vec2::new(0.5, top + 1.5)));
    let under = luma(pic.at(Vec2::new(0.5, top + 12.5)));
    assert!(rim > under + 8.0, "rim {rim} against {under}");
}

/// The rock's edge is smoothed, and it is darker just inside its face.
#[test]
fn rock_has_a_smooth_edge_and_a_darker_rim() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(3.0);
    let pic = draw(&gpu, &bench.field, &[], View::screen(1.0));
    // Across the disc's top right edge (radius 100 round (250, -280)).
    let way = Vec2::new(1.0, -1.0).normalize();
    let centre = Vec2::new(250.0, -280.0);
    let (rock, back) = (luma(srgb8(PlainLook::default().rock)), luma(BACK()));
    let between = (0..40)
        .map(|k| luma(pic.at(centre + way * (95.0 + 0.25 * k as f32))))
        .filter(|&l| l > back + 10.0 && l < rock - 10.0)
        .count();
    assert!(between >= 1, "an in-between edge");
    let edge = luma(pic.at(centre + way * 99.0));
    let inner = luma(pic.at(centre + way * 80.0));
    assert!(edge + 8.0 < inner, "darker by the face: {edge} against {inner}");
}

/// A pool resting on its floor is drawn down to the floor, with no line of back between.
#[test]
fn a_pool_is_drawn_up_to_its_floor() {
    let Some(gpu) = card() else { return };
    for cell in [8.0f32, 3.0, 2.0] {
        let bench = sets::screen_pool(cell, 0.3);
        let pic = draw(&gpu, &bench.field, &bench.drops, View::screen(1.0));
        let back = BACK();
        // Up from the floor (its face at 960) at a few places across.
        for x in [-300.5f32, 0.5, 211.5] {
            for y in 900..960 {
                let c = pic.at(Vec2::new(x, y as f32 + 0.5));
                assert!(apart(c, back) > 30, "{cell} px cells: back at ({x}, {y}): {c:?}");
            }
        }
    }
}

/// The film one drop thick on the bench's slope is drawn as one piece, never a row of dots.
#[test]
fn a_film_one_drop_thick_is_one_piece() {
    let Some(gpu) = card() else { return };
    for cell in [8.0f32, 3.0] {
        let bench = sets::bench(cell);
        let pic = draw(&gpu, &bench.field, &bench.drops, View::screen(1.0));
        let (a, b) = (Vec2::new(-470.0, -330.0), Vec2::new(-80.0, -150.0));
        let along = (b - a).normalize();
        let up = Vec2::new(along.y, -along.x);
        // Just above the face, all along the film: always water (never back).
        let mut t = 30.0;
        while t < (b - a).length() - 30.0 {
            let c = pic.at(a + along * t + up * (sets::REST * cell + 0.3));
            assert!(apart(c, BACK()) > 30, "{cell} px: a gap in the film {t} px along: {c:?}");
            t += 0.5;
        }
    }
}

/// A body drawn from draft drops and from final drops has the same outline within a draft spacing.
#[test]
fn draft_and_final_outlines_agree() {
    let Some(gpu) = card() else { return };
    let cover = |cell: f32| {
        let bench = sets::screen_pool(cell, 0.4);
        let pic = draw(&gpu, &bench.field, &bench.drops, View::screen(0.5));
        let back = BACK();
        (0..pic.height).map(|y| apart(pic.px(270, y), back) > 30).collect::<Vec<bool>>()
    };
    let (draft, fine) = (cover(8.0), cover(2.0));
    let differ: Vec<usize> = (0..draft.len()).filter(|&y| draft[y] != fine[y]).collect();
    // (Half scale: a 4 px draft spacing is 2 picture px.)
    assert!(differ.len() <= 2, "rows that differ: {differ:?}");
}

/// A drop whose kind has no look takes the last look given; with none given, the default.
#[test]
fn a_kind_without_a_look_takes_the_last() {
    let Some(gpu) = card() else { return };
    let field = open(8.0);
    let (lo, hi) = (Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0));
    let five = sets::lattice(&field, lo, hi, 5);
    let one = sets::lattice(&field, lo, hi, 1);
    let look = sets::look();
    let a = draw_with(&gpu, &field, &five, View::screen(0.25), &look);
    let b = draw_with(&gpu, &field, &one, View::screen(0.25), &look);
    assert!(a.pixels == b.pixels, "kind 5 is drawn as the last look (kind 1)");
    let none = PlainLook { liquids: Vec::new(), ..look };
    let c = draw_with(&gpu, &field, &five, View::screen(0.25), &none);
    let d = draw_with(&gpu, &field, &sets::lattice(&field, lo, hi, 0), View::screen(0.25), &PlainLook::default());
    assert!(c.pixels == d.pixels, "with no looks every kind is the default water");
}

/// Clarity 0: the liquid is its Colour all through, however deep.
#[test]
fn clarity_zero_is_flat() {
    let Some(gpu) = card() else { return };
    let bench = sets::screen_pool(8.0, 0.5);
    let look =
        PlainLook { liquids: vec![PlainLiquid { clarity: 0.0, ..PlainLiquid::default() }], ..PlainLook::default() };
    let pic = draw_with(&gpu, &bench.field, &bench.drops, View::screen(0.5), &look);
    let a = pic.at(Vec2::new(0.0, 40.0));
    let b = pic.at(Vec2::new(0.0, 600.0));
    assert!(apart(a, b) <= 1, "{a:?} and {b:?}");
    assert!(apart(b, srgb8(PlainLiquid::default().colour)) <= 2, "the Colour itself: {b:?}");
}

/// Far zoomed in on a pool's middle, it is all water.
#[test]
fn zooming_in_far_keeps_the_body_whole() {
    let Some(gpu) = card() else { return };
    let bench = sets::screen_pool(8.0, 0.5);
    let view = View { lo: Vec2::new(-30.0, 500.0), scale: 8.0, width: 480, height: 480 };
    let pic = draw(&gpu, &bench.field, &bench.drops, view);
    let back = BACK();
    let holes =
        (0..480).flat_map(|y| (0..480).map(move |x| (x, y))).filter(|&(x, y)| apart(pic.px(x, y), back) <= 30).count();
    assert_eq!(holes, 0);
}

/// The bench at full size, for looking at (by hand): `PLAIN_OUT=<folder>`, `PLAIN_CELLS=8,3,2`, and
/// `PLAIN_DROPS=<a.drops,...>` (drops files from the measuring runs' `WATER_DUMP`, drawn on the bench's
/// course at `PLAIN_DROPS_CELL`, default 6, unless `PLAIN_SHOT` names the shot they came from).
#[test]
#[ignore = "pictures to look at: by hand"]
fn bench_pictures() {
    let gpu = card().expect("a graphics device");
    let out = std::path::PathBuf::from(std::env::var("PLAIN_OUT").expect("PLAIN_OUT, a folder"));
    std::fs::create_dir_all(&out).unwrap();
    let cells: Vec<f32> = std::env::var("PLAIN_CELLS")
        .unwrap_or_else(|_| "8,3,2".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let view = View::screen(1.0);
    for cell in cells {
        let bench = sets::bench(cell);
        let pic = draw(&gpu, &bench.field, &bench.drops, view);
        bpe_water::probe::save_png(&out.join(format!("bench-c{cell}.png")), &pic.pixels, pic.width, pic.height)
            .unwrap();
    }
    if let Ok(files) = std::env::var("PLAIN_DROPS") {
        for file in files.split(',').filter(|f| !f.is_empty()) {
            let drops = bpe_water::draw::stored::load_drops(std::path::Path::new(file)).unwrap();
            let cell: f32 = std::env::var("PLAIN_DROPS_CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
            let shot = std::env::var("PLAIN_SHOT").unwrap_or_default();
            let field = match bpe_water::scenes::shot(&shot, cell) {
                Some(scene) => {
                    let least = bpe_water::drops::LEAST_WALL * cell;
                    SolidField::build_least(scene.layout, &sets::carved(&scene.solids, least), least)
                }
                None => sets::bench(cell).field,
            };
            let pic = draw_with(&gpu, &field, &drops, view, &PlainLook::default());
            let name = std::path::Path::new(file).file_stem().unwrap().to_string_lossy().to_string();
            bpe_water::probe::save_png(&out.join(format!("{name}.png")), &pic.pixels, pic.width, pic.height).unwrap();
        }
    }
}

/// Where two liquids that differ only in Clarity meet deep in a pool, the darkness goes from one to
/// the other with no lighter or darker line between them.
#[test]
fn liquids_of_different_clarity_meet_without_a_line() {
    let Some(gpu) = card() else { return };
    let field = open(3.0);
    let (lo, hi) = (Vec2::new(-400.0, -300.0), Vec2::new(400.0, 500.0));
    let drops: Vec<Drop> = sets::lattice(&field, lo, hi, 0)
        .into_iter()
        .map(|mut d| {
            if field.layout.to_world(Vec2::from(d.x)).x < 0.0 {
                d.kind = 1;
            }
            d
        })
        .collect();
    let water = PlainLiquid::default();
    let look = PlainLook { liquids: vec![water, PlainLiquid { clarity: 60.0, ..water }], ..PlainLook::default() };
    let pic = draw_with(&gpu, &field, &drops, View::screen(1.0), &look);
    for y in [200.0f32, 350.0, 450.0] {
        let left = luma(pic.at(Vec2::new(-30.0, y)));
        let right = luma(pic.at(Vec2::new(30.0, y)));
        let (least, most) = (left.min(right) - 3.0, left.max(right) + 3.0);
        let mut x = -30.0;
        while x <= 30.0 {
            let l = luma(pic.at(Vec2::new(x, y)));
            assert!((least..=most).contains(&l), "at ({x}, {y}) {l} is outside {least}..{most}");
            x += 1.0;
        }
    }
}

/// A body that grows between drawings (the engine makes its buffers anew when drops are added past
/// its room) draws all of them, from the same drawing.
#[test]
fn a_grown_body_draws_its_new_drops() {
    let Some(gpu) = card() else { return };
    let field = open(8.0);
    let first = sets::lattice(&field, Vec2::new(-200.0, -100.0), Vec2::new(-100.0, 0.0), 0);
    let more = sets::lattice(&field, Vec2::new(100.0, -300.0), Vec2::new(400.0, 300.0), 0);
    let engine = WaterGpu::new(gpu.device(), gpu.queue());
    let mut water = Water::new(engine, &field, Vec2::ZERO, Numbers::default(), &[Feel::default()]);
    water.add(&first);
    let view = View::screen(0.5);
    let mut plain = Plain::new(gpu.clone(), &field, REST);
    let look = sets::look();
    for (add, shown) in [(None, false), (Some(&more), true)] {
        if let Some(more) = add {
            water.add(more);
        }
        plain.draw(&water.drop_buffers(), &view, &look);
        let pic = Picture { pixels: plain.read(), width: view.width, height: view.height, view };
        assert!(apart(pic.at(Vec2::new(-150.0, -50.0)), BACK()) > 40, "the first block is drawn");
        assert_eq!(apart(pic.at(Vec2::new(250.0, 0.0)), BACK()) > 40, shown, "the new block is drawn only once added");
    }
    assert!(water.count() as usize > 4 * first.len(), "the body grew past its first room");
}

/// A pool filling the bottom half of an open course and its mirror image filling the top half darken
/// alike with depth, right out to the picture's edges, at any Clarity on the Water Look card; and the
/// course's side edges (like any wall) neither light the water beside them nor darken it.
#[test]
fn depth_reaches_the_edges_at_any_clarity() {
    let Some(gpu) = card() else { return };
    let field = open(4.0);
    // Filled right up to the course's edge, where drops stop (2.05 cells in from the grid's edge).
    let l = field.layout;
    let (lo, hi) = (l.to_world(Vec2::splat(2.05)), l.to_world(Vec2::new(l.cols as f32, l.rows as f32) - 3.05));
    let below = sets::lattice(&field, Vec2::new(lo.x, 0.0), hi, 0);
    let above = sets::lattice(&field, lo, Vec2::new(hi.x, 0.0), 0);
    for clarity in [100.0f32, 200.0, 400.0, 600.0] {
        let look =
            PlainLook { liquids: vec![PlainLiquid { clarity, ..PlainLiquid::default() }], ..PlainLook::default() };
        let a = draw_with(&gpu, &field, &below, View::screen(1.0), &look);
        let b = draw_with(&gpu, &field, &above, View::screen(1.0), &look);
        for x in [-520.0f32, 0.0, 520.0] {
            let at = |pic: &Picture, y: f32| luma(pic.at(Vec2::new(x, y)));
            for d in [20.0f32, 100.0, 300.0, 600.0, 900.0] {
                let (la, lb) = (at(&a, d), at(&b, -d));
                // (The ladder's coarse levels are not quite the same either way up: 3.4 at most.)
                assert!((la - lb).abs() <= 4.0, "Clarity {clarity}, x {x}, {d} px deep: {la} below, {lb} above");
                let mid = luma(a.at(Vec2::new(0.0, d)));
                assert!((la - mid).abs() <= 3.0, "Clarity {clarity}, {d} px deep: {la} at x {x}, {mid} in the middle");
            }
            let (near, far) = (at(&a, 100.0), at(&a, 900.0));
            assert!(far + 10.0 < near, "Clarity {clarity}, x {x}: {far} at 900 px deep against {near} at 100");
        }
    }
}

/// The same water has the same colour wherever the camera is and however near it is: depth is
/// worked out over the whole course, not the picture.
#[test]
fn depth_stays_put_when_the_view_moves_or_zooms() {
    let Some(gpu) = card() else { return };
    // A course two screens tall, the pool its lower three quarters.
    let layout = Layout::cover(Vec2::new(-540.0, -960.0), Vec2::new(540.0, 2880.0), 4.0);
    let field = SolidField::build(layout, &[]);
    let drops = sets::lattice(&field, Vec2::new(-540.0, 0.0), Vec2::new(540.0, 2880.0), 0);
    let views = [
        View { lo: Vec2::new(-540.0, -960.0), scale: 0.5, width: 540, height: 960 },
        View { lo: Vec2::new(-540.0, -400.0), scale: 0.5, width: 540, height: 960 },
        View { lo: Vec2::new(-100.0, 150.0), scale: 2.0, width: 400, height: 400 },
        View { lo: Vec2::new(-200.0, 200.0), scale: 4.0, width: 200, height: 200 },
    ];
    let pics: Vec<Picture> = views.iter().map(|v| draw(&gpu, &field, &drops, *v)).collect();
    for p in [Vec2::new(0.0, 230.0), Vec2::new(-160.0, 220.0), Vec2::new(-180.0, 240.0)] {
        let seen: Vec<[u8; 3]> = pics
            .iter()
            .filter(|pic| {
                let q = (p - pic.view.lo) * pic.view.scale;
                q.x >= 0.0 && q.y >= 0.0 && q.x < pic.width as f32 && q.y < pic.height as f32
            })
            .map(|pic| pic.at(p))
            .collect();
        assert!(seen.len() >= 3, "{p} is in three pictures or more");
        for c in &seen {
            assert!(apart(*c, seen[0]) <= 3, "at {p}: {seen:?}");
        }
    }
}

/// How long a picture takes on this card (a measuring run, by hand, in release): a screen two
/// thirds full at 2 px cells (about 1.4 million drops, the final course) at 1080 x 1920, and at
/// 8 px (about 86,000, the draft course) at 1080 x 1920 and 540 x 960. 20 pictures in one
/// submission after one to warm up; the spec's targets are 8 ms and 2 ms on the owner's card.
///
/// `cargo test --release -p bpe-water --test plain -- --ignored --nocapture plain_pace`
#[test]
#[ignore = "a measuring run: by hand"]
fn plain_pace() {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    let gpu = PlainGpu::new(&device, &queue);
    let look = sets::look();
    println!("# cell px\tdrops\tpicture\tms a picture");
    for (cell, scale) in [(2.0f32, 1.0f32), (8.0, 1.0), (8.0, 0.5)] {
        let set = sets::screen_pool(cell, 2.0 / 3.0);
        let stored = StoredDrops::new(&device, &queue, &set.drops);
        let mut plain = Plain::new(gpu.clone(), &set.field, REST);
        let view = View::screen(scale);
        let took = plain.draw_many(&stored.buffers(), &view, &look, 20);
        println!("{cell}\t{}\t{}x{}\t{:.2}", set.drops.len(), view.width, view.height, took.as_secs_f64() * 1000.0);
    }
}
