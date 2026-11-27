//! The look drawing (`bpe_water::draw::look`, Water mode part 6): that it repeats, where it draws
//! what, the lamps' light and shadows, light through the liquid, caustics, glow, the materials, the
//! back, bend, shine, bands, that the light doesn't depend on the view, and Quality. From drops laid
//! out by hand (`draw::sets`), never physics; on the software card at 270 x 480 or less.

use std::sync::Arc;

use bpe_water::draw::look::{
    BackLook, BackPicture, Lamp, LampKind, LiquidLook, Look, MaterialKind, MaterialLook, Quality,
};
use bpe_water::draw::sets::{self, REST};
use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
use bpe_water::{Drop, Layout, Part, Solid, SolidField};
use glam::Vec2;

fn card() -> Option<Arc<LookGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(LookGpu::new(&device, &queue))
}

/// A picture: rgba8 sRGB.
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
}

fn luma(c: [u8; 3]) -> f32 {
    0.2126 * f32::from(c[0]) + 0.7152 * f32::from(c[1]) + 0.0722 * f32::from(c[2])
}

fn apart(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap()
}

/// A drawing and its drops on the card.
struct Drawn {
    look: LookDraw,
    stored: StoredDrops,
}

impl Drawn {
    fn new(gpu: &Arc<LookGpu>, field: &SolidField, drops: &[Drop]) -> Drawn {
        let stored = StoredDrops::new(gpu.device(), gpu.queue(), drops);
        Drawn { look: LookDraw::new(gpu.clone(), field, REST), stored }
    }

    fn draw(&mut self, view: View, look: &Look) -> Picture {
        self.look.draw(&self.stored.buffers(), &view, look);
        let pixels = self.look.read();
        assert_eq!(pixels.len(), (view.width * view.height * 4) as usize);
        if let Ok(dir) = std::env::var("LOOK_DEBUG_DIR") {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let name = std::thread::current().name().unwrap_or("look").replace("::", "-");
            let path = std::path::Path::new(&dir).join(format!("{name}-{n}.png"));
            let file = std::fs::File::create(path).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), view.width, view.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        }
        Picture { pixels, width: view.width, height: view.height, view }
    }
}

fn draw(gpu: &Arc<LookGpu>, field: &SolidField, drops: &[Drop], view: View, look: &Look) -> Picture {
    Drawn::new(gpu, field, drops).draw(view, look)
}

fn screen() -> Layout {
    Layout::cover(Vec2::new(-540.0, -960.0), Vec2::new(540.0, 960.0), 8.0)
}

/// The screen's box carved in rock (its inside -500 to 500 across, -900 to 900 down), with `more`.
fn boxed(more: &[Solid]) -> SolidField {
    let mut solids =
        sets::carved(&[Solid::rect(Vec2::new(-500.0, -900.0), Vec2::new(500.0, 900.0), false, 16.0)], 12.0);
    solids.extend_from_slice(more);
    SolidField::build_least(screen(), &solids, 12.0)
}

fn spot(place: Vec2, aim: f32, spread: f32) -> Lamp {
    Lamp { kind: LampKind::Spot, place, aim, spread, colour: [1.0; 3], softness: 0.1, ..Lamp::default() }
}

fn dark(lamps: Vec<Lamp>, quality: Quality) -> Look {
    let mut look = Look { lamps, ..Look::default() };
    look.light.ambient = 0.1;
    look.light.bounce = 0.0;
    look.light.quality = quality;
    look
}

fn bright(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Test 1: The same drops draw the same picture to the bit, twice and from a second drawing, with every
/// pass on (lamps, glow, bounce, two liquids, ripples).
#[test]
fn the_same_drops_draw_the_same_picture() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    let glow = LiquidLook { colour: [1.0, 0.5, 0.2], glow: 1.5, glow_colour: [1.0, 0.4, 0.1], ..LiquidLook::default() };
    let mut look = dark(vec![spot(Vec2::new(0.0, -800.0), 90.0, 70.0)], Quality::Draft);
    look.liquids = vec![LiquidLook { ripples: 1.0, ..LiquidLook::default() }, glow];
    look.light.bounce = 0.5;
    look.light.shafts = 0.5;
    look.time = 1.25;
    let view = View::screen(0.25);
    let mut a = Drawn::new(&gpu, &bench.field, &bench.drops);
    let first = a.draw(view, &look);
    let again = a.draw(view, &look);
    let other = draw(&gpu, &bench.field, &bench.drops, view, &look);
    assert!(first.pixels == again.pixels, "drawn again, the same");
    assert!(first.pixels == other.pixels, "a second drawing draws the same");
}

/// Test 2: With the plain inputs the pool is drawn where its drops are, the back and the rock where they
/// are, in their own colours (Ambient 1 shows every colour as it is).
#[test]
fn with_the_plain_inputs_everything_is_where_it_is() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let drops = sets::lattice(&field, Vec2::new(-200.0, 300.0), Vec2::new(200.0, 892.0), 0);
    let plain = sets::look();
    let pic = draw(&gpu, &field, &drops, View::screen(0.25), &Look::plain(&plain));
    let srgb8 = |c: [f32; 3]| c.map(|v| (v * 255.0).round() as u8);
    let back = pic.at(Vec2::new(300.0, -300.0));
    assert!(apart(back, srgb8(plain.back)) <= 2, "the back: {back:?}");
    let rock = pic.at(Vec2::new(-530.0, 0.0));
    assert!(apart(rock, srgb8(plain.rock)) <= 2, "the rock: {rock:?}");
    let top = pic.at(Vec2::new(0.0, 310.0));
    assert!(apart(top, srgb8(plain.liquids[0].colour)) <= 12, "the pool's top is its Colour: {top:?}");
    let deep = pic.at(Vec2::new(0.0, 860.0));
    assert!(luma(deep) < luma(top), "deeper is darker: {deep:?} under {top:?}");
    assert!(apart(pic.at(Vec2::new(-300.0, 600.0)), back) <= 2, "no water beside the pool");
}

/// Test 3: A Spot lights open space in its cone and not outside it, and a wall between it and a point
/// shadows the point (with Shadows on; with them off, the point is lit).
#[test]
fn a_spot_lights_its_cone_and_walls_shadow_it() {
    let Some(gpu) = card() else { return };
    let wall = Solid::rect(Vec2::new(-120.0, -300.0), Vec2::new(120.0, -260.0), true, 0.0);
    let field = boxed(&[wall]);
    let lamp = spot(Vec2::new(0.0, -800.0), 90.0, 50.0);
    let light_at = |look: &Look, p: Vec2| {
        let mut d = Drawn::new(&gpu, &field, &[]);
        d.draw(View::screen(0.25), look);
        d.look.light_at(p).expect("in the light map")
    };
    let look = dark(vec![lamp.clone()], Quality::Good);
    let in_cone = bright(light_at(&look, Vec2::new(250.0, -150.0)));
    let outside = bright(light_at(&look, Vec2::new(420.0, -700.0)));
    let behind = bright(light_at(&look, Vec2::new(0.0, -100.0)));
    assert!(in_cone > 0.2, "lit in its cone: {in_cone}");
    assert!(outside < 0.02 * in_cone, "dark outside its cone: {outside} against {in_cone}");
    assert!(behind < 0.1 * in_cone, "shadowed behind the wall: {behind} against {in_cone}");
    let open = dark(vec![Lamp { shadows: false, ..lamp }], Quality::Good);
    let unshadowed = bright(light_at(&open, Vec2::new(0.0, -100.0)));
    assert!(unshadowed > 0.5 * in_cone, "with Shadows off the wall doesn't stop it: {unshadowed}");
}

/// Test 3b: A lamp far up a tall course of fine nodes (1 px) still shadows a floor far below it with
/// a ledge just under the lamp: the march through open air gets all the way to the lamp.
#[test]
fn a_far_lamp_still_casts_shadows() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::new(-100.0, -960.0), Vec2::new(100.0, 960.0), 1.0);
    let mut solids = sets::carved(&[Solid::rect(Vec2::new(-80.0, -900.0), Vec2::new(80.0, 900.0), false, 0.0)], 12.0);
    let ledge = Solid::rect(Vec2::new(-100.0, -720.0), Vec2::new(100.0, -690.0), true, 0.0);
    let lamp = spot(Vec2::new(0.0, -880.0), 90.0, 60.0);
    let light_at = |field: &SolidField, p: Vec2| {
        let mut d = Drawn::new(&gpu, field, &[]);
        d.draw(View::screen(0.25), &dark(vec![lamp.clone()], Quality::Draft));
        bright(d.look.light_at(p).expect("in the light map"))
    };
    let floor = Vec2::new(0.0, 850.0);
    let open = light_at(&SolidField::build_least(layout, &solids, 12.0), floor);
    solids.push(ledge);
    let shut = light_at(&SolidField::build_least(layout, &solids, 12.0), floor);
    assert!(open > 0.05, "the floor is lit with no ledge: {open}");
    assert!(shut < 0.05 * open, "the ledge 1,600 px up shadows it: {shut} against {open}");
}

/// Test 4: A lamp's light under a deep pool is dimmer than over it, and bluer for blue water.
#[test]
fn light_under_deep_water_is_dimmer_and_takes_its_colour() {
    let Some(gpu) = card() else { return };
    let field = sets::open_top(8.0);
    let drops = sets::lattice(&field, Vec2::new(-500.0, 100.0), Vec2::new(500.0, 900.0), 0);
    let sun = Lamp { kind: LampKind::Sun, aim: 90.0, colour: [1.0; 3], ..Lamp::default() };
    let mut d = Drawn::new(&gpu, &field, &drops);
    d.draw(View::screen(0.25), &dark(vec![sun], Quality::Good));
    let over = d.look.light_at(Vec2::new(0.0, 0.0)).unwrap();
    let under = d.look.light_at(Vec2::new(0.0, 700.0)).unwrap();
    assert!(bright(under) < 0.5 * bright(over), "dimmer under: {under:?} against {over:?}");
    assert!(under[2] / under[0].max(1e-6) > 2.0 * over[2] / over[0].max(1e-6), "bluer under: {under:?}");
}

/// The light along the floor of a waved pool, just above it.
fn floor_light(gpu: &Arc<LookGpu>, caustics: f32) -> Vec<f32> {
    let bench = sets::wavy_pool(4.0, 200.0, 10.0, 90.0);
    let sun = Lamp { kind: LampKind::Sun, aim: 90.0, colour: [1.0; 3], ..Lamp::default() };
    let mut look = dark(vec![sun], Quality::Good);
    look.light.caustics = caustics;
    look.liquids = vec![LiquidLook { clarity: 4000.0, ..LiquidLook::default() }];
    let mut d = Drawn::new(gpu, &bench.field, &bench.drops);
    d.draw(View::screen(0.25), &look);
    (0..60).map(|k| bright(d.look.light_at(Vec2::new(-300.0 + 10.0 * k as f32, 880.0)).unwrap())).collect()
}

/// Test 5: Caustics: under a waved surface the floor's brightest light is well over its mean with
/// Caustics 2, and even with Caustics 0.
#[test]
fn a_waved_surface_throws_caustics_on_the_floor() {
    let Some(gpu) = card() else { return };
    let stats = |v: &[f32]| {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        (mean, lo, hi)
    };
    let (mean, _, hi) = stats(&floor_light(&gpu, 2.0));
    assert!(hi > 1.5 * mean, "caustics: brightest {hi} against a mean of {mean}");
    let (mean, lo, hi) = stats(&floor_light(&gpu, 0.0));
    assert!(hi - lo < 0.1 * mean, "even with no caustics: {lo} to {hi} round {mean}");
}

/// Test 6: Glowing liquid lights the rock beside it; rock behind a closed wall stays at its ambient.
#[test]
fn glow_lights_the_rock_beside_it_and_not_behind_a_wall() {
    let Some(gpu) = card() else { return };
    // Two chambers: the left one holds a glowing blob, the right is closed off from it.
    let field = boxed(&[Solid::rect(Vec2::new(-20.0, -900.0), Vec2::new(20.0, 900.0), true, 0.0)]);
    let blob = Vec2::new(-260.0, 0.0);
    let drops: Vec<Drop> = sets::lattice(&field, blob - Vec2::splat(60.0), blob + Vec2::splat(60.0), 0);
    let mut look = dark(Vec::new(), Quality::Good);
    look.light.ambient = 0.2;
    look.liquids =
        vec![LiquidLook { colour: [1.0, 0.4, 0.1], glow: 3.0, glow_colour: [1.0, 0.5, 0.2], ..LiquidLook::default() }];
    let pic = draw(&gpu, &field, &drops, View::screen(0.25), &look);
    // The dividing wall's two faces (red: the glow's colour).
    let near = f32::from(pic.at(Vec2::new(-14.0, 0.0))[0]);
    let walled = f32::from(pic.at(Vec2::new(14.0, 0.0))[0]);
    let mut unlit = dark(Vec::new(), Quality::Good);
    unlit.light.ambient = 0.2;
    let ambient = f32::from(draw(&gpu, &field, &[], View::screen(0.25), &unlit).at(Vec2::new(14.0, 0.0))[0]);
    assert!(near > walled + 20.0, "the rock beside the glow is lit: {near} against {walled}");
    assert!((walled - ambient).abs() <= 2.0, "the walled-off side stays at its ambient: {walled} against {ambient}");
}

/// The pieces set with each piece made of `kinds` in turn.
fn materials(kinds: &[MaterialKind]) -> (SolidField, Look) {
    let mut field = boxed(&[]);
    let v = Vec2::new;
    let blocks = [
        (v(-450.0, -800.0), v(-50.0, -400.0)),
        (v(50.0, -800.0), v(450.0, -400.0)),
        (v(-450.0, -300.0), v(-50.0, 100.0)),
        (v(50.0, -300.0), v(450.0, 100.0)),
        (v(-450.0, 200.0), v(-50.0, 600.0)),
    ];
    let mut solids = Vec::new();
    for (k, (lo, hi)) in blocks.iter().enumerate() {
        field.add_part(
            &Part { solids: vec![Solid::rect(*lo, *hi, true, 0.0)], cut: vec![], wetting: 1.0 },
            k as u16 + 1,
            12.0,
        );
        solids.push(MaterialLook {
            kind: kinds[k % kinds.len()],
            colour: [0.7, 0.5, 0.3],
            second: [0.2, 0.3, 0.6],
            texture: 60.0,
            ..MaterialLook::default()
        });
    }
    let look = Look {
        solids,
        back: BackLook::Gradient { samples: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], angle: 0.0 },
        ..Look::default()
    };
    (field, look)
}

/// Test 7: Each Material draws: Flat one colour across its inside, Rock and Paper vary, Glass shows the
/// back's gradient through it, and an Outline draws its colour along the face.
#[test]
fn each_material_draws_its_own_way() {
    let Some(gpu) = card() else { return };
    let kinds =
        [MaterialKind::Flat, MaterialKind::Rock, MaterialKind::Paper, MaterialKind::Glass, MaterialKind::Basalt];
    let (field, mut look) = materials(&kinds);
    look.solids[0].outline = 8.0;
    look.solids[3].colour = [0.9, 0.95, 1.0];
    look.solids[0].outline_colour = [1.0, 0.0, 1.0];
    let pic = draw(&gpu, &field, &[], View::screen(0.25), &look);
    let spread = |lo: Vec2| {
        let values: Vec<f32> = (0..8)
            .flat_map(|i| (0..8).map(move |j| (i, j)))
            .map(|(i, j)| luma(pic.at(lo + Vec2::new(i as f32, j as f32) * 30.0)))
            .collect();
        let (a, b) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        b - a
    };
    assert!(spread(Vec2::new(-400.0, -750.0)) <= 1.0, "flat is one colour inside");
    assert!(spread(Vec2::new(100.0, -750.0)) > 4.0, "rock varies");
    assert!(spread(Vec2::new(-400.0, -250.0)) > 2.0, "paper varies");
    let glass_left = luma(pic.at(Vec2::new(80.0, -100.0)));
    let glass_right = luma(pic.at(Vec2::new(420.0, -100.0)));
    assert!(glass_right > glass_left + 10.0, "glass shows the back's gradient: {glass_left} then {glass_right}");
    let edge = pic.at(Vec2::new(-448.0, -600.0));
    assert!(edge[0] > 150 && edge[1] < 80 && edge[2] > 150, "the outline's colour at the face: {edge:?}");
}

/// A PNG of `w` x `h` from `pixels` (rgb) in a fresh temporary file.
fn png_file(name: &str, w: u32, h: u32, pixels: &[[u8; 3]]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("bpe-water-look-{}-{name}.png", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels.concat()).unwrap();
    path
}

/// Test 8: The Back: a Colour, a Gradient along Direction 0, a Picture in its quarters (from a file,
/// and from the bytes the scene keeps under a name that is no file), and a missing Picture drawn as
/// Darker rock (and told, until the back is something else).
#[test]
fn the_back_shows_what_its_card_says() {
    let Some(gpu) = card() else { return };
    let field = SolidField::build(screen(), &[]);
    let view = View::screen(0.25);
    let with = |back: BackLook| Look { back, ..Look::default() };
    let pic = draw(&gpu, &field, &[], view, &with(BackLook::Colour([0.2, 0.6, 0.3])));
    assert!(apart(pic.at(Vec2::new(250.0, 250.0)), [51, 153, 77]) <= 2);
    let gradient = BackLook::Gradient { samples: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], angle: 0.0 };
    let pic = draw(&gpu, &field, &[], view, &with(gradient));
    assert!(luma(pic.at(Vec2::new(300.0, 0.0))) > luma(pic.at(Vec2::new(-300.0, 0.0))) + 40.0, "left to right");
    let quarters = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
    let eight: Vec<[u8; 3]> = (0..64).map(|i| quarters[(i % 8 / 4) + 2 * (i / 32)]).collect();
    let path = png_file("quarters", 8, 8, &eight);
    let mut d = Drawn::new(&gpu, &field, &[]);
    let pic = d.draw(view, &with(BackLook::Picture(BackPicture::file(path.to_string_lossy()))));
    let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
    let _ = std::fs::remove_file(&path);
    let kept = BackPicture { name: "Quarters".into(), kept: Some(bytes) };
    let from_scene = draw(&gpu, &field, &[], view, &with(BackLook::Picture(kept)));
    assert!(from_scene.pixels == pic.pixels, "the picture the scene keeps shows as the file does");
    for (p, want) in [
        (Vec2::new(-400.0, -600.0), 0),
        (Vec2::new(400.0, -600.0), 1),
        (Vec2::new(-400.0, 600.0), 2),
        (Vec2::new(400.0, 600.0), 3),
    ] {
        let got = pic.at(p);
        let want = quarters[want];
        assert!(apart(got, want) <= 3, "the picture's quarter at {p}: {got:?} against {want:?}");
    }
    assert!(d.look.notes().is_empty());
    let mut d = Drawn::new(&gpu, &field, &[]);
    let missing = d.draw(view, &with(BackLook::Picture(BackPicture::file("/no/such/picture.png"))));
    let darker = draw(&gpu, &field, &[], view, &with(BackLook::DarkerRock(0.5)));
    assert!(missing.pixels == darker.pixels, "a missing picture shows Darker rock");
    assert_eq!(d.look.notes(), vec!["The back's picture can't be read".to_string()]);
    d.draw(view, &with(BackLook::Colour([0.2, 0.6, 0.3])));
    assert!(d.look.notes().is_empty(), "nothing to tell once the back is a colour");
}

/// Test 9: Bend moves a striped back seen through clear liquid; Bend 0 doesn't, and away from the
/// liquid nothing moves.
#[test]
fn bend_moves_what_is_seen_through_the_liquid() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let stripes: Vec<[u8; 3]> =
        (0..64 * 64).map(|i| if (i % 64) / 4 % 2 == 0 { [250, 250, 250] } else { [10, 10, 10] }).collect();
    let path = png_file("stripes", 64, 64, &stripes);
    let blob = Vec2::new(0.0, 0.0);
    let drops: Vec<Drop> = sets::lattice(&field, blob - Vec2::splat(150.0), blob + Vec2::splat(150.0), 0);
    let water = |bend: f32| Look {
        liquids: vec![LiquidLook { clarity: 4000.0, bend, colour: [0.9, 0.95, 1.0], ..LiquidLook::default() }],
        back: BackLook::Picture(BackPicture::file(path.to_string_lossy())),
        ..Look::default()
    };
    let view = View::screen(0.25);
    let flat = draw(&gpu, &field, &drops, view, &water(0.0));
    let bent = draw(&gpu, &field, &drops, view, &water(40.0));
    let _ = std::fs::remove_file(&path);
    let moved = (0..40)
        .filter(|k| {
            apart(
                flat.at(Vec2::new(-135.0, -140.0 + 7.0 * *k as f32)),
                bent.at(Vec2::new(-135.0, -140.0 + 7.0 * *k as f32)),
            ) > 30
        })
        .count();
    assert!(moved >= 5, "the stripes move near its edge: {moved} of 40");
    let far: Vec<_> = (0..40)
        .map(|k| Vec2::new(-140.0 + 7.0 * k as f32, 400.0))
        .filter(|&p| apart(flat.at(p), bent.at(p)) > 0)
        .map(|p| (p.x, flat.at(p), bent.at(p)))
        .collect();
    assert!(far.is_empty(), "nothing moves away from the liquid: {far:?}");
}

/// Test 10: Shine: a lamp gives the surface facing it a highlight brighter than the body below it.
#[test]
fn shine_catches_the_lamp_on_the_surface() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let drops = sets::lattice(&field, Vec2::new(-500.0, 200.0), Vec2::new(500.0, 900.0), 0);
    let lamp = spot(Vec2::new(0.0, -600.0), 90.0, 120.0);
    let mut look = dark(vec![lamp], Quality::Good);
    look.liquids = vec![LiquidLook { shine: 1.0, shine_size: 0.6, solid: 1.0, clarity: 0.0, ..LiquidLook::default() }];
    // Near, so the rounded surface is a few pixels deep.
    let view = View { lo: Vec2::new(-100.0, 150.0), scale: 2.0, width: 200, height: 200 };
    let pic = draw(&gpu, &field, &drops, view, &look);
    let top = (0..16).map(|k| luma(pic.at(Vec2::new(-50.0, 199.0 + 0.5 * k as f32)))).fold(0.0, f32::max);
    let below = luma(pic.at(Vec2::new(-50.0, 230.0)));
    assert!(top > below + 20.0, "a highlight at the surface: {top} over {below}");
}

/// Test 11: Bands 3 shades a lit body in a few flat steps where it is smooth with Bands 0.
#[test]
fn bands_shade_in_flat_steps() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let drops = sets::lattice(&field, Vec2::new(-500.0, -300.0), Vec2::new(500.0, 900.0), 0);
    let lamp = Lamp { brightness: 1.2, ..spot(Vec2::new(0.0, -350.0), 90.0, 170.0) };
    let levels = |bands: u32| {
        let mut look = dark(vec![lamp.clone()], Quality::Good);
        look.light.ambient = 0.3;
        look.liquids = vec![LiquidLook {
            colour: [0.2, 0.45, 0.7],
            solid: 1.0,
            clarity: 0.0,
            shine: 0.0,
            edge_sheen: 0.0,
            bands,
            band_softness: 0.0,
            ..LiquidLook::default()
        }];
        let pic = draw(&gpu, &field, &drops, View::screen(0.25), &look);
        let mut bins = std::collections::BTreeMap::new();
        for j in 0..60 {
            for i in 0..40 {
                let l = luma(pic.at(Vec2::new(-400.0 + 20.0 * i as f32, -250.0 + 18.0 * j as f32)));
                *bins.entry((l / 4.0) as u32).or_insert(0u32) += 1;
            }
        }
        bins.values().filter(|n| **n >= 24).count()
    };
    let smooth = levels(0);
    let banded = levels(3);
    assert!(banded <= 6, "three bands make a few levels: {banded}");
    assert!(smooth >= banded + 4, "smooth shading spans many more: {smooth} against {banded}");
}

/// Test 12: The light at a world point is the same whatever the view shows.
#[test]
fn the_light_does_not_depend_on_the_view() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    let look = dark(vec![spot(Vec2::new(0.0, -800.0), 95.0, 80.0)], Quality::Good);
    let p = Vec2::new(-100.0, 450.0);
    let mut whole = Drawn::new(&gpu, &bench.field, &bench.drops);
    whole.draw(View::screen(0.25), &look);
    let mut near = Drawn::new(&gpu, &bench.field, &bench.drops);
    near.draw(View { lo: Vec2::new(-300.0, 300.0), scale: 1.0, width: 270, height: 270 }, &look);
    let (a, b) = (whole.look.light_at(p).unwrap(), near.look.light_at(p).unwrap());
    assert!(a == b, "the same light: {a:?} and {b:?}");
    assert!(bright(a) > 0.0);
}

/// Test 13: Draft and Good differ little, and both differ from no lamps.
#[test]
fn draft_and_good_differ_little() {
    let Some(gpu) = card() else { return };
    let bench = sets::bench(8.0);
    let lamps = vec![spot(Vec2::new(0.0, -800.0), 90.0, 80.0)];
    let mut good = dark(lamps.clone(), Quality::Good);
    good.light.ambient = 0.4;
    let draft = Look {
        light: bpe_water::draw::look::LightLook { quality: Quality::Draft, ..good.light.clone() },
        ..good.clone()
    };
    let unlit = Look { lamps: Vec::new(), ..good.clone() };
    let view = View::screen(0.25);
    let pics = [&good, &draft, &unlit].map(|l| draw(&gpu, &bench.field, &bench.drops, view, l));
    let mean = |a: &Picture, b: &Picture| {
        a.pixels.iter().zip(&b.pixels).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / a.pixels.len() as f64
    };
    let close = mean(&pics[0], &pics[1]);
    assert!(close < 6.0, "Draft is near Good: {close}");
    assert!(mean(&pics[0], &pics[2]) > 3.0 * close.max(0.5), "lamps change the picture");
}

/// The bench at 3 px under two warm Spots and a waved pool under a Sun, at full size, for looking at
/// (by hand): `LOOK_DEBUG_DIR=<folder>` (`bpe-kit`'s full-size look check draws more of the same).
#[test]
#[ignore = "pictures to look at: by hand"]
fn bench_look_pictures() {
    let gpu = card().expect("a graphics device");
    assert!(std::env::var("LOOK_DEBUG_DIR").is_ok(), "LOOK_DEBUG_DIR, a folder");
    let water = LiquidLook {
        colour: [0.31, 0.71, 0.91],
        deep: [0.02, 0.13, 0.23],
        clarity: 320.0,
        bend: 16.0,
        shine: 0.6,
        edge_sheen: 0.4,
        ripples: 0.5,
        ..LiquidLook::default()
    };
    let orange = LiquidLook { colour: [1.0, 0.54, 0.24], deep: [0.35, 0.1, 0.02], clarity: 120.0, ..water.clone() };
    let warm = |x: f32, aim: f32| Lamp {
        place: Vec2::new(x, -820.0),
        colour: [1.0, 0.82, 0.63],
        brightness: 1.6,
        softness: 0.4,
        ..spot(Vec2::ZERO, aim, 70.0)
    };
    let lit = Look {
        liquids: vec![water.clone(), orange],
        back: BackLook::DarkerRock(0.65),
        light: bpe_water::draw::look::LightLook {
            ambient: 0.12,
            bounce: 0.4,
            caustics: 1.2,
            shafts: 0.3,
            ..Default::default()
        },
        lamps: vec![warm(-350.0, 70.0), warm(350.0, 110.0)],
        time: 1.0,
        ..Look::default()
    };
    let bench = sets::bench(3.0);
    Drawn::new(&gpu, &bench.field, &bench.drops).draw(View::screen(1.0), &lit);
    let pool = sets::wavy_pool(3.0, 500.0, 14.0, 110.0);
    let sun = Look {
        liquids: vec![LiquidLook { clarity: 600.0, ripples: 1.0, ..water }],
        back: BackLook::Colour([0.55, 0.75, 0.9]),
        lamps: vec![Lamp { kind: LampKind::Sun, aim: 80.0, brightness: 1.2, ..Lamp::default() }],
        ..lit
    };
    Drawn::new(&gpu, &pool.field, &pool.drops).draw(View::screen(1.0), &sun);
}
