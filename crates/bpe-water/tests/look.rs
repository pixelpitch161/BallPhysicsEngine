//! The look drawing (`bpe_water::draw::look`, Water mode part 6): that it repeats, where it draws
//! what, the lamps' light and shadows, light through the liquid, caustics, glow, the materials, the
//! back, bend, shine, bands, that the light doesn't depend on the view, Quality, the extras
//! (bubbles, fizz, foam, mist, sparks), the finish's Effects, a lone drop's whole shadow and no
//! caustics behind a stream. From drops laid
//! out by hand (`draw::sets`), never physics; on the software card at 270 x 480 or less.

use std::sync::Arc;

use bpe_core::maths::Maths;

use bpe_water::draw::look::{
    BackLook, BackPicture, Lamp, LampKind, LiquidLook, Look, MaterialKind, MaterialLook, Quality,
};
use bpe_water::draw::sets::{self, REST};
use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View};
use bpe_water::{Drop, Layout, Part, Solid, SolidField};
use glam::Vec2;

/// The unit way at angle `a` (radians).
fn turn(a: f32) -> Vec2 {
    let (s, c) = a.det_sin_cos();
    Vec2::new(c, s)
}

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

// ---- the extras (6b) ----

/// A pool in the screen's box from y 0 down, its drops at rest, no lamps: `liquid` drawn under
/// Ambient 1 over a dark red back.
fn extras_look(liquid: LiquidLook) -> Look {
    let mut look = Look { liquids: vec![liquid], back: BackLook::Colour([0.3, 0.02, 0.02]), ..Look::default() };
    look.light.ambient = 1.0;
    look.light.bounce = 0.0;
    look.light.quality = Quality::Draft;
    look
}

/// An opaque blue liquid, so the back never shows through it.
fn blue() -> LiquidLook {
    LiquidLook { colour: [0.1, 0.4, 0.9], deep: [0.1, 0.4, 0.9], solid: 1.0, ..LiquidLook::default() }
}

/// How many pixels of `a` and `b` (the same size) differ by more than `by` in some channel, among
/// those whose world point `keep` takes.
fn changed(a: &Picture, b: &Picture, by: u8, keep: impl Fn(Vec2) -> bool) -> usize {
    let mut n = 0;
    for y in 0..a.height {
        for x in 0..a.width {
            let p = a.view.lo + (Vec2::new(x as f32, y as f32) + 0.5) / a.view.scale;
            if keep(p) && apart(a.at(p), b.at(p)) > by {
                n += 1;
            }
        }
    }
    n
}

/// Test 14: Trapped air is drawn as a bubble with Bubbles on (the liquid, a little lighter, inside
/// a bright rim), and is the hole it is with Bubbles 0.
#[test]
fn trapped_air_draws_a_bubble_or_a_hole() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let (centre, r) = (Vec2::new(0.0, 300.0), 40.0);
    let mut drops = sets::lattice(&field, Vec2::new(-500.0, 0.0), Vec2::new(500.0, 900.0), 0);
    let layout = field.layout;
    drops.retain(|d| (layout.to_world(Vec2::from(d.x)) - centre).length() > r);
    let air = sets::lattice(&field, centre - Vec2::splat(r), centre + Vec2::splat(r), bpe_water::AIR);
    drops.extend(air.into_iter().filter(|d| (layout.to_world(Vec2::from(d.x)) - centre).length() <= r));
    let view = View { lo: Vec2::new(-100.0, -100.0), scale: 1.0, width: 200, height: 480 };
    let mut d = Drawn::new(&gpu, &field, &drops);
    let hole = d.draw(view, &extras_look(LiquidLook { bubbles: 0.0, ..blue() }));
    let bubble = d.draw(view, &extras_look(LiquidLook { bubbles: 1.0, ..blue() }));
    let back = hole.at(Vec2::new(0.0, -50.0));
    let body = hole.at(Vec2::new(70.0, 300.0));
    assert!(apart(hole.at(centre), back) < 12, "a hole shows the back: {:?} and {back:?}", hole.at(centre));
    let inside = bubble.at(centre);
    assert!(apart(inside, back) > 60, "a bubble is filled: {inside:?} against the back {back:?}");
    assert!(luma(inside) >= luma(body) && apart(inside, body) < 80, "a little lighter: {inside:?} and {body:?}");
    let rim = (0..64)
        .flat_map(|k| {
            let a = k as f32 / 64.0 * std::f32::consts::TAU;
            (0..10).map(move |j| centre + turn(a) * (32.0 + 2.0 * j as f32))
        })
        .map(|p| luma(bubble.at(p)))
        .fold(0.0, f32::max);
    assert!(rim > luma(inside) + 25.0 && rim > luma(body) + 40.0, "a bright rim: {rim} ({inside:?}, {body:?})");
}

/// The fizz in a still pool at water time `t`, with Bubbles `bubbles`.
fn fizz_picture(d: &mut Drawn, bubbles: f32, t: f32) -> Picture {
    let view = View { lo: Vec2::new(-100.0, -100.0), scale: 1.0, width: 200, height: 400 };
    let mut look = extras_look(LiquidLook { bubbles, bubble_size: 6.0, ..blue() });
    look.time = t;
    d.draw(view, &look)
}

/// Test 15: Fizz shows inside a still pool and nowhere in the air, rises between two water times,
/// and doesn't show in a pool running fast.
#[test]
fn fizz_rises_inside_still_water() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let drops = sets::lattice(&field, Vec2::new(-500.0, 0.0), Vec2::new(500.0, 900.0), 0);
    let mut d = Drawn::new(&gpu, &field, &drops);
    let (plain, at, later) =
        (fizz_picture(&mut d, 0.0, 1.0), fizz_picture(&mut d, 1.0, 1.0), fizz_picture(&mut d, 1.0, 1.1));
    let deep = |p: Vec2| p.y > 30.0;
    let pixels = 200 * 270;
    let shown = changed(&plain, &at, 10, deep);
    assert!(shown > pixels / 200, "fizz in the pool: {shown} pixels of {pixels}");
    assert_eq!(changed(&plain, &at, 0, |p| p.y < -10.0), 0, "none in the air");
    // Each bubble 12 px higher a tenth of a second on (a few born again or fading).
    let fizz = |pic: &Picture, p: Vec2| apart(pic.at(p), plain.at(p)) > 10;
    let score = |shift: f32| {
        let mut n = 0;
        for y in 40..260 {
            for x in -95..95 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if fizz(&later, p) && fizz(&at, p + Vec2::new(0.0, shift)) {
                    n += 1;
                }
            }
        }
        n
    };
    let (up, still, down) = (score(12.0), score(0.0), score(-12.0));
    assert!(up > 2 * still && up > 2 * down, "it rose: {up} against {still} still and {down} down");
    // None where the pool runs fast (the bubbles would ride the flow).
    let mut running = drops.clone();
    for d in &mut running {
        d.v = [900.0 / field.layout.cell, 0.0];
    }
    let mut d = Drawn::new(&gpu, &field, &running);
    let (plain, at) = (fizz_picture(&mut d, 0.0, 1.0), fizz_picture(&mut d, 1.0, 1.0));
    assert_eq!(changed(&plain, &at, 10, deep), 0, "no fizz in a fast pool");
}

/// The screen's pool from y 0 down with speeds of `most` world px a second spread every way (a fixed
/// hash of each drop), or none.
fn churning_pool(field: &SolidField, most: f32) -> Vec<Drop> {
    let mut drops = sets::lattice(field, Vec2::new(-500.0, 0.0), Vec2::new(500.0, 900.0), 0);
    for (k, d) in drops.iter_mut().enumerate() {
        let h = (k as u32).wrapping_mul(0x9e37_79b9).rotate_left(13).wrapping_mul(0x85eb_ca6b);
        let a = (h >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        d.v = (turn(a) * most / field.layout.cell).to_array();
    }
    drops
}

/// Test 16: Churning water (speeds spread every way) is drawn with foam, still water with none.
#[test]
fn churning_water_draws_foam_and_still_water_none() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let view = View { lo: Vec2::new(-135.0, -20.0), scale: 1.0, width: 270, height: 270 };
    let liquid = LiquidLook { colour: [0.02, 0.05, 0.2], deep: [0.02, 0.05, 0.2], ..blue() };
    let mut foamed = 0;
    for (most, foam_seen) in [(600.0, true), (0.0, false)] {
        let mut d = Drawn::new(&gpu, &field, &churning_pool(&field, most));
        let none = d.draw(view, &extras_look(liquid.clone()));
        let foam = d.draw(view, &extras_look(LiquidLook { foam: 1.0, foam_size: 8.0, ..liquid.clone() }));
        let n = changed(&none, &foam, 20, |_| true);
        if foam_seen {
            assert!(n > 270 * 270 / 5, "foam on churning water: {n} pixels");
            foamed = n;
        } else {
            assert_eq!(n, 0, "no foam on still water (churning drew {foamed})");
        }
    }
}

/// A few lone drops in the screen's box, 100 px apart, all at `speed` world px a second to the right.
fn lone_drops(field: &SolidField, speed: f32) -> Vec<Drop> {
    let l = field.layout;
    (0..4)
        .map(|k| {
            let p = Vec2::new(-150.0 + 100.0 * k as f32, -300.0 + 7.0 * k as f32);
            Drop::new(l.to_grid(p), Vec2::new(speed / l.cell, 0.0), 0, k, bpe_water::WATER)
        })
        .collect()
}

/// Test 17: Fast lone drops draw mist round them and sparks streaking behind them; slow ones draw
/// neither, and nor do the edges of a fast band moving along itself.
#[test]
fn fast_lone_drops_draw_mist_and_sparks() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let view = View { lo: Vec2::new(-270.0, -400.0), scale: 1.0, width: 270, height: 200 };
    let extras = |speed: f32, mist: f32, sparks: f32| {
        let mut look = extras_look(LiquidLook { mist, sparks, ..blue() });
        look.back = BackLook::Colour([0.01; 3]);
        look.light.ambient = 0.5;
        draw(&gpu, &field, &lone_drops(&field, speed), view, &look)
    };
    let fast_none = extras(1500.0, 0.0, 0.0);
    let mist = extras(1500.0, 1.0, 0.0);
    let sparks = extras(1500.0, 0.0, 1.0);
    let head = Vec2::new(-50.0, -293.0);
    let near = |p: Vec2| (p - head).length() < 6.0 && (p - head).length() > 2.5;
    assert!(changed(&fast_none, &mist, 8, near) > 10, "mist round a fast lone drop");
    let behind = head - Vec2::new(60.0, 0.0);
    let lit = luma(sparks.at(behind)) - luma(fast_none.at(behind));
    assert!(lit > 40.0, "a spark's trail 60 px behind: {lit} brighter");
    assert!(luma(sparks.at(head + Vec2::new(40.0, 0.0))) < luma(fast_none.at(head)) + 10.0, "nothing ahead");
    let (slow_none, slow) = (extras(100.0, 0.0, 0.0), extras(100.0, 1.0, 1.0));
    assert!(slow_none.pixels == slow.pixels, "slow drops draw neither");
    // A fast band's edges, whose drops move along it, don't spark.
    let mut band = sets::lattice(&field, Vec2::new(-500.0, -330.0), Vec2::new(500.0, -270.0), 0);
    for d in &mut band {
        d.v = [1500.0 / field.layout.cell, 0.0];
    }
    let band_look = |sparks: f32| {
        let mut look = extras_look(LiquidLook { sparks, ..blue() });
        look.back = BackLook::Colour([0.01; 3]);
        look.light.ambient = 0.5;
        draw(&gpu, &field, &band, view, &look)
    };
    let edged = changed(&band_look(0.0), &band_look(1.0), 8, |_| true);
    assert!(edged == 0, "a fast band's edges: {edged} pixels sparked");
}

/// Test 18: Every extra at once (a bubble, fizz, foam, mist and sparks) draws the same picture to
/// the bit twice.
#[test]
fn the_extras_draw_the_same_twice() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let mut drops = churning_pool(&field, 600.0);
    drops.extend(lone_drops(&field, 1500.0));
    let liquid = LiquidLook { foam: 0.8, bubbles: 1.0, mist: 1.0, sparks: 1.0, ..blue() };
    let mut look = extras_look(liquid);
    look.time = 2.5;
    let view = View::screen(0.25);
    let mut d = Drawn::new(&gpu, &field, &drops);
    let (a, b) = (d.draw(view, &look), d.draw(view, &look));
    assert!(a.pixels == b.pixels, "the same twice");
}

/// Test 19: Wet marks: rock just inside its face where the pool's drops have been is darker by the
/// frame drawn with Wet marks 1, the same with Wet marks 0, and dry at a frame before the marks.
#[test]
fn wet_marks_darken_the_rock_the_water_has_touched() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let drops = sets::lattice(&field, Vec2::new(-500.0, 700.0), Vec2::new(500.0, 900.0), 0);
    let mut d = Drawn::new(&gpu, &field, &drops);
    let wet = bpe_water::draw::WetField::new(gpu.device(), gpu.queue(), field.layout);
    wet.mark(&d.stored.buffers(), 3);
    let view = View { lo: Vec2::new(-100.0, 880.0), scale: 2.0, width: 200, height: 100 };
    let mut drawn = |marks: f32, frame: Option<u32>| {
        let mut look = extras_look(blue());
        look.rock.wet_marks = marks;
        d.look.set_wet(frame.map(|_| &wet), frame.unwrap_or(0));
        d.draw(view, &look)
    };
    let (dry, wet_now, before, none) =
        (drawn(1.0, None), drawn(1.0, Some(10)), drawn(1.0, Some(1)), drawn(0.0, Some(10)));
    // (The floor's face, from the field.)
    let face = (0..400).map(|k| 880.0 + 0.1 * k as f32).find(|&y| field.at(Vec2::new(-50.0, y)) < 0.0).unwrap();
    let p = Vec2::new(-50.0, face + 2.0);
    assert!(luma(wet_now.at(p)) < 0.8 * luma(dry.at(p)), "darker: {:?} against {:?}", wet_now.at(p), dry.at(p));
    let deep = Vec2::new(-50.0, face + 12.0);
    assert!(wet_now.at(deep) == dry.at(deep), "only near the face");
    assert!(before.pixels == dry.pixels, "dry at a frame before the marks");
    let dry_none = drawn(0.0, None);
    assert!(none.pixels == dry_none.pixels, "Wet marks 0 draws no marks");
}

/// Test 20: The finish runs the scene's Effects over the picture, as a Balls frame's: Mirror copies
/// the left half onto the right and leaves the left as it was.
#[test]
fn the_finish_runs_the_effects_over_the_picture() {
    let Some(gpu) = card() else { return };
    let (field, mut look) = materials(&[MaterialKind::Flat, MaterialKind::Rock]);
    let plain = draw(&gpu, &field, &[], View::screen(0.25), &look);
    let mirror = bpe_gfx::effects::Builtin::find("mirror").expect("built in");
    look.finish.effects.push(bpe_gfx::EffectCall {
        params: bpe_gfx::effects::defaults(mirror.header()),
        ..bpe_gfx::EffectCall::new(1, mirror.wgsl, bpe_gfx::Stage::Picture)
    });
    let mirrored = draw(&gpu, &field, &[], View::screen(0.25), &look);
    // (The back between the blocks and the box's sides: dark at the left, light at the right.)
    let (left, right) = (Vec2::new(-475.0, -850.0), Vec2::new(475.0, -850.0));
    assert!(apart(plain.at(left), plain.at(right)) > 40, "{:?} and {:?}", plain.at(left), plain.at(right));
    assert!(apart(mirrored.at(left), mirrored.at(right)) <= 2, "{:?} and {:?}", mirrored.at(left), mirrored.at(right));
    assert!(apart(mirrored.at(left), plain.at(left)) <= 1, "the left half as it was");
}

/// The screen's box carved in rock on a 3 px layout (the full-size look check's detail).
fn boxed_fine() -> SolidField {
    let layout = Layout::cover(Vec2::new(-540.0, -960.0), Vec2::new(540.0, 960.0), 3.0);
    let solids = sets::carved(&[Solid::rect(Vec2::new(-500.0, -900.0), Vec2::new(500.0, 900.0), false, 16.0)], 12.0);
    SolidField::build_least(layout, &solids, 12.0)
}

/// Test 21: A lone small drop high in the air casts one shadow under a Sun, deep near it and softening
/// smoothly further down: the march through open air never steps over it (its shadow came out dashed
/// while a coarse texel holding a lone drop counted as empty).
#[test]
fn a_lone_drop_casts_a_whole_shadow() {
    let Some(gpu) = card() else { return };
    let field = boxed_fine();
    let drop = Vec2::new(0.0, -600.0);
    let drops = sets::lattice(&field, drop - Vec2::splat(1.5), drop + Vec2::splat(1.5), 0);
    assert!(!drops.is_empty() && drops.len() <= 9, "a few drops: {}", drops.len());
    // (A Sun in a closed course reaches only with Shadows off; the liquid still takes its light.)
    let sun = Lamp { kind: LampKind::Sun, aim: 90.0, colour: [1.0; 3], shadows: false, ..Lamp::default() };
    let mut look = dark(vec![sun], Quality::Good);
    look.liquids = vec![LiquidLook { clarity: 1.0, ..LiquidLook::default() }];
    let mut d = Drawn::new(&gpu, &field, &drops);
    d.draw(View::screen(0.25), &look);
    let mut shares = Vec::new();
    for k in 0..80 {
        let y = -580.0 + 8.0 * k as f32;
        let under = bright(d.look.light_at(Vec2::new(0.0, y)).unwrap());
        let beside = bright(d.look.light_at(Vec2::new(200.0, y)).unwrap());
        assert!(beside > 0.5, "open air is lit at y {y}: {beside}");
        shares.push(under / beside);
    }
    // Near the drop its shadow is deep; further down it softens, as the march's coarser levels average
    // the drop over more room. A dashed shadow had points fully lit between dark ones.
    let near = &shares[..25];
    assert!(near.iter().all(|&s| s < 0.4), "the drop shadows the 200 px under it: {near:?}");
    let jumps = shares.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(jumps < 0.3, "the shadow softens smoothly, with no dashes: {shares:?}");
}

/// Test 22: A Spot just beside a falling stream lights the rock behind the stream the same with
/// Caustics 2 as with none: the stream's two surfaces aren't one lens over the rock (its caustics
/// there were wide bright and dark stripes). Close behind the stream the lens fades out, with no
/// line where it ends. A pool's floor keeps its caustics (test 5).
#[test]
fn a_stream_throws_no_caustics_on_the_rock_behind_it() {
    let Some(gpu) = card() else { return };
    let field = boxed_fine();
    let drops = sets::lattice(&field, Vec2::new(-6.0, -500.0), Vec2::new(6.0, 300.0), 0);
    let lamp = spot(Vec2::new(40.0, -100.0), 180.0, 150.0);
    let rows = [-200.0f32, -100.0, 0.0, 100.0];
    // Far behind the stream, then close behind it in rows, from where the lens starts to fade (28 px
    // past the stream's middle, its liquid's edge about 8 px out) to 106 px.
    let behind = |caustics: f32| -> (Vec<f32>, Vec<Vec<f32>>) {
        let mut look = dark(vec![lamp.clone()], Quality::Good);
        look.light.caustics = caustics;
        look.liquids = vec![LiquidLook { clarity: 4000.0, ..LiquidLook::default() }];
        let mut d = Drawn::new(&gpu, &field, &drops);
        d.draw(View::screen(0.25), &look);
        let light = |p: Vec2| bright(d.look.light_at(p).unwrap());
        let far = (0..40).map(|k| light(Vec2::new(-300.0, -400.0 + 15.0 * k as f32))).collect();
        let near =
            rows.iter().map(|&y| (0..40).map(|k| light(Vec2::new(-28.0 - 2.0 * k as f32, y))).collect()).collect();
        (far, near)
    };
    let ((with, near_with), (without, near_without)) = (behind(2.0), behind(0.0));
    assert!(without.iter().all(|&l| l > 0.05), "the rock behind is lit: {without:?}");
    for (k, (a, b)) in with.iter().zip(&without).enumerate() {
        assert!((a - b).abs() <= 0.02 * b.max(1e-3), "point {k}: {a} with caustics against {b} without");
    }
    for (y, (a, b)) in rows.iter().zip(near_with.iter().zip(&near_without)) {
        let shares: Vec<f32> = a.iter().zip(b).map(|(a, b)| a / b.max(1e-4)).collect();
        let jump = shares.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 0.1, "at y {y} the lens fades out with no line: {shares:?}");
    }
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

/// Test 23: The finish's Grade measures in the video's pixels: a Radial mask on a block of the shot
/// grades the same block in a quarter-size picture and a half-size one, and leaves the block under it
/// as it was; and the same for a video of another shape (measured across, as the masks' handles are).
#[test]
fn the_grade_measures_in_the_videos_pixels() {
    use bpe_gfx::{GradeArea, GradeBasic, GradeCombine, GradeMask, GradePanel, GradePart, GradePlace, GradeRadial};
    let Some(gpu) = card() else { return };
    let (field, plain) = materials(&[MaterialKind::Flat]);
    let radial = GradeRadial {
        x: 250.0,
        y: -600.0,
        width: 200.0,
        height: 200.0,
        angle: 0.0,
        roundness: 0.0,
        feather: 10.0,
        places: vec![GradePlace::SCREEN],
    };
    let mut graded = plain.clone();
    graded.finish.grade.masks = vec![GradeMask {
        amount: 1.0,
        area: Some(GradeArea {
            parts: vec![GradePart {
                combine: GradeCombine::Add,
                invert: false,
                shape: bpe_gfx::GradeShape::Radial(radial),
                id: 0,
            }],
            invert: false,
        }),
        panels: vec![GradePanel::Basic(GradeBasic { exposure: 1.0, ..GradeBasic::default() })],
        ..GradeMask::default()
    }];
    let (inside, below) = (Vec2::new(250.0, -600.0), Vec2::new(250.0, -50.0));
    for (scale, video) in [(0.25, (1080, 1920)), (0.5, (1080, 1920)), (0.25, (1080, 1350))] {
        graded.finish.video = video;
        let before = draw(&gpu, &field, &[], View::screen(scale), &plain);
        let after = draw(&gpu, &field, &[], View::screen(scale), &graded);
        let (a, b) = (luma(after.at(inside)), luma(before.at(inside)));
        assert!(a > b + 20.0, "at {scale}, {video:?}: the mask's middle is a stop brighter: {a} vs {b}");
        assert!(apart(after.at(below), before.at(below)) <= 1, "at {scale}, {video:?}: the block under it as it was");
    }
}

// ---- the grade's Things masks and the Light only take (6e) ----

/// A Things mask choosing `things` (`None` every thing; `back` turned over), grown by `expand`
/// video pixels, two thirds of a stop darker inside.
fn things_mask(things: Option<Vec<u32>>, back: bool, expand: f32) -> bpe_gfx::GradeStack {
    use bpe_gfx::{GradeArea, GradeBasic, GradeCombine, GradeMask, GradePanel, GradePart, GradeShape, GradeThings};
    let things = GradeThings { things, background: back, expand, feather: 0.0 };
    bpe_gfx::GradeStack {
        masks: vec![GradeMask {
            amount: 1.0,
            area: Some(GradeArea {
                parts: vec![GradePart {
                    combine: GradeCombine::Add,
                    invert: false,
                    shape: GradeShape::Things(things),
                    id: 0,
                }],
                invert: false,
            }),
            panels: vec![GradePanel::Basic(GradeBasic { exposure: -1.0, ..GradeBasic::default() })],
            ..GradeMask::default()
        }],
        split: None,
    }
}

/// Test 24: A Things mask grades only what it chooses: the Liquids (and round them, with Expand), the
/// Solids (the course's rock and every piece), the Back, or one solid.
#[test]
fn a_things_mask_grades_only_the_things_it_chooses() {
    use bpe_water::draw::look::ThingCodes;
    let Some(gpu) = card() else { return };
    let (field, mut look) = materials(&[MaterialKind::Flat]);
    look.liquids = vec![blue()];
    look.light.ambient = 0.8;
    look.light.quality = Quality::Draft;
    look.finish.video = (1080, 1920);
    look.finish.things = ThingCodes { liquids: vec![101], rock: Some(1), solids: vec![11, 12, 13, 14, 15] };
    let drops = sets::lattice(&field, Vec2::new(-500.0, 650.0), Vec2::new(500.0, 900.0), 0);
    let plain = draw(&gpu, &field, &drops, View::screen(0.25), &look);
    let v = Vec2::new;
    let (pool, block, block_2, back, rock, near_pool) =
        (v(0.0, 800.0), v(-250.0, -600.0), v(250.0, -600.0), v(0.0, -600.0), v(-522.0, 0.0), v(200.0, 590.0));
    let graded = |things: Option<Vec<u32>>, back: bool, expand: f32| {
        let mut l = look.clone();
        l.finish.grade = things_mask(things, back, expand);
        draw(&gpu, &field, &drops, View::screen(0.25), &l)
    };
    let darker = |p: &Picture, at: Vec2| luma(plain.at(at)) - luma(p.at(at)) > 12.0;
    let same = |p: &Picture, at: Vec2| apart(plain.at(at), p.at(at)) <= 1;
    let liquids = graded(Some(vec![101]), false, 0.0);
    assert!(darker(&liquids, pool), "the liquid: {:?} from {:?}", liquids.at(pool), plain.at(pool));
    for (name, at) in [("a block", block), ("the back", back), ("the rock", rock), ("beside the pool", near_pool)] {
        assert!(same(&liquids, at), "Liquids leaves {name} as it was");
    }
    let grown = graded(Some(vec![101]), false, 100.0);
    assert!(darker(&grown, near_pool) && same(&grown, back), "Expand grows it round the liquid");
    let solids = graded(Some(vec![1, 11, 12, 13, 14, 15]), false, 0.0);
    assert!(darker(&solids, block) && darker(&solids, rock), "Solids: the blocks and the rock");
    assert!(same(&solids, pool) && same(&solids, back), "Solids leaves the liquid and the back");
    let back_only = graded(None, true, 0.0);
    assert!(darker(&back_only, back), "Back: the back");
    assert!(same(&back_only, pool) && same(&back_only, block) && same(&back_only, rock), "Back only");
    let one = graded(Some(vec![12]), false, 0.0);
    assert!(darker(&one, block_2) && same(&one, block) && same(&one, pool), "one block");
}

/// Test 25: A Things mask on the Extras grades a spark's trail and not the back round it.
#[test]
fn a_things_mask_on_the_extras_grades_the_sparks() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let view = View { lo: Vec2::new(-270.0, -400.0), scale: 1.0, width: 270, height: 200 };
    let mut look = extras_look(LiquidLook { sparks: 1.0, ..blue() });
    look.back = BackLook::Colour([0.2; 3]);
    look.light.ambient = 0.5;
    look.finish.things.liquids = vec![101];
    let drops = lone_drops(&field, 1500.0);
    let plain = draw(&gpu, &field, &drops, view, &look);
    look.finish.grade = things_mask(Some(vec![bpe_water::draw::look::EXTRAS]), false, 0.0);
    let graded = draw(&gpu, &field, &drops, view, &look);
    let behind = Vec2::new(-110.0, -293.0);
    let (a, b) = (luma(graded.at(behind)), luma(plain.at(behind)));
    assert!(b - a > 12.0, "a spark's trail is darker: {a} from {b}");
    let open = Vec2::new(-250.0, -380.0);
    assert!(apart(graded.at(open), plain.at(open)) <= 1, "the back away from the drops as it was");
}

/// Test 26: An effect on the Light only reads the water's light: one that makes the light 2
/// everywhere lights the red back and the blue pool at 2 and keeps their colours, where on the
/// whole picture it makes them white.
#[test]
fn an_effect_on_the_light_only_reads_the_waters_light() {
    const TWO: &str = "// name: Two\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return vec3<f32>(2.0); }\n";
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let mut look = extras_look(blue());
    look.light.ambient = 0.5;
    let drops = sets::lattice(&field, Vec2::new(-500.0, 0.0), Vec2::new(500.0, 900.0), 0);
    let plain = draw(&gpu, &field, &drops, View::screen(0.25), &look);
    let with = |takes: bpe_gfx::Takes| {
        let mut l = look.clone();
        l.finish
            .effects
            .push(bpe_gfx::EffectCall { takes, ..bpe_gfx::EffectCall::new(1, TWO, bpe_gfx::Stage::Picture) });
        draw(&gpu, &field, &drops, View::screen(0.25), &l)
    };
    let (light, whole) = (with(bpe_gfx::Takes::Light), with(bpe_gfx::Takes::Whole));
    let (back, pool) = (Vec2::new(0.0, -600.0), Vec2::new(0.0, 600.0));
    assert!(whole.at(back).iter().all(|c| *c > 240), "the whole picture made white: {:?}", whole.at(back));
    let (lit, was) = (light.at(back), plain.at(back));
    assert!(
        lit[0] > was[0] + 40 && lit[1] < 100 && lit[2] < 100,
        "the back lit at 2 and still red: {lit:?} from {was:?}"
    );
    let (lit, was) = (light.at(pool), plain.at(pool));
    assert!(lit[2] > was[2] + 30 && lit[0] < lit[2], "the pool lit at 2 and still blue: {lit:?} from {was:?}");
}

/// A pool carrying half of another liquid's colour is drawn between the two liquids' pools (part
/// 4.2): the look's per-liquid weights share each drop between its two liquids.
#[test]
fn a_mixed_drop_draws_between_its_liquids() {
    let Some(gpu) = card() else { return };
    let field = boxed(&[]);
    let pool =
        |x: f32, kind: u32| sets::lattice(&field, Vec2::new(x - 120.0, 500.0), Vec2::new(x + 120.0, 892.0), kind);
    let mut drops = pool(-330.0, 0);
    drops.extend(pool(330.0, 1));
    drops.extend(pool(0.0, 0).into_iter().map(|d| d.with_mix(1, 0.5)));
    let pic = draw(&gpu, &field, &drops, View::screen(0.25), &Look::plain(&sets::look()));
    let at = |x: f32| pic.at(Vec2::new(x, 520.0));
    let (water, orange, half) = (at(-330.0), at(330.0), at(0.0));
    assert!(apart(water, orange) > 60, "the two liquids differ: {water:?} {orange:?}");
    for k in 0..3 {
        let (lo, hi) = (water[k].min(orange[k]), water[k].max(orange[k]));
        let mid = (f32::from(lo) + f32::from(hi)) / 2.0;
        assert!(
            (f32::from(half[k]) - mid).abs() <= 0.25 * f32::from(hi - lo) + 3.0,
            "channel {k}: the half mixed pool {half:?} is between {water:?} and {orange:?}"
        );
    }
}
