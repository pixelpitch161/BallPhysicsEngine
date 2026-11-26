//! Water mode part 2b: a Water scene's draft on the graphics card, as the editor plays it.
//!
//! `drops_stay_out_of_the_rock` checks a carved course with a fill and a tap plays, and that
//! nothing lands in the rock. `draft_pace` (by hand, on the owner's card) times a full screen of
//! draft water: one screen carved as one chamber, two thirds filled at the start, a tap pouring in,
//! seconds 1 to 6 of it:
//! `cargo test --release -p bpe-kit --test water_draft -- --ignored --nocapture draft_pace`.

use std::time::Instant;

use bpe_core::{Registry, Scene, SlotId, Stroke, Strokes, ThingId, Value};
use bpe_kit::things::water::draft::{DRAFT_MOST, WaterSetup, plain_look};
use bpe_kit::things::water::{LIQUID, POUR, StartingLook, course_of, new_water_scene};
use bpe_water::draw::{Plain, PlainGpu, View};
use bpe_water::{Water, WaterGpu};
use glam::Vec2;

fn set(scene: &mut Scene, id: ThingId, slot: SlotId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

fn add(scene: &mut Scene, reg: &Registry, kind: &str, at: Vec2) -> ThingId {
    let id = bpe_kit::add_thing(scene, reg, kind, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().transform.position = at;
    id
}

/// One screen carved as one chamber (24 px of rock left round it), two thirds filled at the
/// start, and a tap (Width 40, Rate 1, Speed 300) pouring in from the top: about 86,000 drops at
/// 8 px. `detail` is the Water card's Draft detail ("coarse", "normal", "fine").
fn screen_course(reg: &Registry, detail: &str) -> Scene {
    let mut scene = new_water_scene(reg, StartingLook::Plain, "");
    set(&mut scene, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice(detail.into()));
    let course = course_of(&scene).unwrap();
    let r = 540.0 - 24.0;
    let chamber = Stroke {
        points: vec![[0.0, -960.0 + 24.0 + r], [0.0, 960.0 - 24.0 - r]],
        size: r,
        strength: 1.0,
        square: true,
        ..Default::default()
    };
    set(&mut scene, course, SlotId::Form, "carved", Value::Strokes(Strokes { list: vec![chamber] }));
    add(&mut scene, reg, LIQUID, Vec2::ZERO);
    let top = -936.0 + 1872.0 / 3.0;
    let fill = add(&mut scene, reg, POUR, Vec2::new(0.0, 0.5 * (top + 936.0)));
    set(&mut scene, fill, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(&mut scene, fill, SlotId::Main, "fill_size", Value::Vec2([2.0 * r, 936.0 - top]));
    add(&mut scene, reg, POUR, Vec2::new(0.0, -800.0));
    scene
}

fn setup(scene: &Scene, reg: &Registry) -> WaterSetup {
    WaterSetup::of(scene, &bpe_core::resolve(scene, reg))
}

/// Plays `water` on to `frame` as the editor's draft does: each frame's inputs read first.
fn play_to(water: &mut Water, setup: &WaterSetup, frame: u32) {
    while water.frames() < frame {
        let inputs = setup.inputs_at(water.frames());
        water.set_gravity(inputs.gravity);
        if !inputs.feels.is_empty() {
            water.set_feels(&inputs.feels);
        }
        water.set_pours(&inputs.pours);
        water.frame();
    }
}

#[test]
fn drops_stay_out_of_the_rock() {
    let reg = bpe_kit::registry();
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let scene = screen_course(&reg, "coarse");
    let s = setup(&scene, &reg);
    let field = s.field();
    let mut water = s.start(WaterGpu::new(&device, &queue), &field, DRAFT_MOST);
    let filled = water.count();
    assert!(filled > 10_000, "{filled}");
    play_to(&mut water, &s, 120);
    water.wait();
    assert!(water.count() > filled, "the tap poured: {} after {filled}", water.count());
    let layout = water.layout();
    let drops = water.read();
    let cell = layout.cell;
    for d in &drops {
        let at = layout.to_world(Vec2::from(d.x));
        assert!(field.at(at) > -0.5 * cell, "a drop in the rock at {at}: {}", field.at(at));
    }
    // The tap's drops reached the pool: some born after the fill are below the fill's top.
    let top = -936.0 + 1872.0 / 3.0;
    let low = drops.iter().filter(|d| d.serial >= filled && layout.to_world(Vec2::from(d.x)).y > top).count();
    assert!(low > 0, "none of the tap's {} drops reached the pool", drops.len() as u32 - filled);
    // And the plain drawing draws it from the card.
    let mut plain = Plain::new(PlainGpu::new(&device, &queue), &field, s.numbers().wall_off);
    let look = plain_look(&scene, &bpe_core::resolve(&scene, &reg), 2.0);
    plain.draw(&water.drop_buffers(), &View::screen(0.25), &look);
    assert_eq!(plain.read().len(), 270 * 480 * 4);
}

/// The 12 ms mark: a frame's physics and one stage-sized drawing together, leaving 4.7 ms of the
/// 16.7 ms frame to the window.
const LIVE_MS: f64 = 12.0;

#[test]
#[ignore = "a measuring run, by hand on the owner's card"]
fn draft_pace() {
    let reg = bpe_kit::registry();
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let info = device.adapter_info();
    println!("# card\t{}\t{:?}\t{}", info.name, info.backend, info.driver_info);
    println!(
        "# detail\tcell\tdrops\tsteps a frame (mean, most)\tphysics ms a frame\tdraw ms 600x1067\tdraw ms 1080x1920\tplays live"
    );
    let gpu = WaterGpu::new(&device, &queue);
    let plain_gpu = PlainGpu::new(&device, &queue);
    // (`WATER_DRAFT_DETAILS` and `WATER_DRAFT_SECONDS` shorten a check that it runs.)
    let details = std::env::var("WATER_DRAFT_DETAILS").unwrap_or_else(|_| "coarse,normal,fine".into());
    let seconds: u32 = std::env::var("WATER_DRAFT_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    for detail in details.split(',') {
        let scene = screen_course(&reg, detail);
        let s = setup(&scene, &reg);
        let field = s.field();
        let mut water = s.start(gpu.clone(), &field, DRAFT_MOST);
        play_to(&mut water, &s, 60);
        water.wait();
        let (mut took, mut most, steps) = (0.0f64, 0u32, water.steps());
        let mut frames = 0u32;
        while water.frames() < 60 + 60 * seconds {
            let at = Instant::now();
            for _ in 0..60 {
                let next = water.frames() + 1;
                play_to(&mut water, &s, next);
                most = most.max(water.steps_last_frame());
                frames += 1;
            }
            water.wait();
            took += at.elapsed().as_secs_f64();
        }
        let physics = 1000.0 * took / f64::from(frames);
        let mean = (water.steps() - steps) as f64 / f64::from(frames);
        let look = plain_look(&scene, &bpe_core::resolve(&scene, &reg), 6.0);
        let mut plain = Plain::new(plain_gpu.clone(), &field, s.numbers().wall_off);
        let small = plain.draw_many(&water.drop_buffers(), &View::screen(600.0 / 1080.0), &look, 30);
        let full = plain.draw_many(&water.drop_buffers(), &View::screen(1.0), &look, 30);
        let small = 1000.0 * small.as_secs_f64();
        let full = 1000.0 * full.as_secs_f64();
        let live = if physics + small <= LIVE_MS { "yes" } else { "no" };
        println!(
            "{detail}\t{}\t{}\t{mean:.1}, {most}\t{physics:.2}\t{small:.2}\t{full:.2}\t{live}",
            s.cell,
            water.count()
        );
    }
}
