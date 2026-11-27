//! Water mode part 8: Make final. A small Water scene's final made at 8 px into a temporary finals
//! folder: it reads back as the draft plays, a remake from a restart point equals a make in one go,
//! a stopped make keeps its whole chunks, slow motion keeps more moments, and a make the disk has no
//! room for is refused. `water_final_speed` (by hand, and in the card job's timing step) times a
//! one-screen final at 2 px:
//! `cargo test --release -p bpe-kit --test water_final -- --ignored --nocapture water_final_speed`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bpe_core::{Address, Registry, Scene, SlotId, Stroke, Strokes, ThingId, Value};
use bpe_kit::things::water::draft::{FINAL_MOST, Speeds, WaterSetup};
use bpe_kit::things::water::finals::Finals;
use bpe_kit::things::water::make::{MakeFinal, MakeProgress, this_card};
use bpe_kit::things::water::{LIQUID, POUR, StartingLook, course_of, new_water_scene};
use bpe_water::WaterGpu;
use bpe_water::take::{Chunk, FrameCodec, chunk};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

fn set(scene: &mut Scene, id: ThingId, slot: SlotId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

fn add(scene: &mut Scene, reg: &Registry, kind: &str, at: Vec2) -> ThingId {
    let id = bpe_kit::add_thing(scene, reg, kind, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().transform.position = at;
    id
}

/// A 400 px box carved in the rock, a 160 px fill at its foot and a tap (from `starts` s) above it.
fn small(reg: &Registry, starts: f64) -> (Scene, ThingId) {
    let mut scene = new_water_scene(reg, StartingLook::Plain, "");
    let course = course_of(&scene).unwrap();
    let hole = Stroke { points: vec![[0.0, 0.0]], size: 200.0, strength: 1.0, square: true, ..Default::default() };
    set(&mut scene, course, SlotId::Form, "carved", Value::Strokes(Strokes { list: vec![hole] }));
    add(&mut scene, reg, LIQUID, Vec2::ZERO);
    let fill = add(&mut scene, reg, POUR, Vec2::new(0.0, 120.0));
    set(&mut scene, fill, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(&mut scene, fill, SlotId::Main, "fill_size", Value::Vec2([160.0, 160.0]));
    let tap = add(&mut scene, reg, POUR, Vec2::new(-60.0, -150.0));
    set(&mut scene, tap, SlotId::Main, "starts", Value::Number(starts));
    (scene, tap)
}

/// `scene`'s final at 8 px, `seconds` long.
fn final_of(scene: &Scene, reg: &Registry, seconds: f64) -> MakeFinal {
    let resolved = bpe_core::resolve(scene, reg);
    let mut make = MakeFinal::of(scene, &resolved, seconds);
    make.setup = Arc::new(WaterSetup::final_of(scene, &resolved).with_cell(8.0));
    make
}

fn folder(name: &str) -> Finals {
    let d = std::env::temp_dir().join(format!("bpe-final-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    Finals::at(d)
}

fn chunk_bytes(dir: &PathBuf, second: u32) -> Vec<u8> {
    std::fs::read(chunk::chunk_path(dir, second)).unwrap()
}

/// A final made reads back moment by moment, in order, and its frame 60 is the draft's.
#[test]
fn a_final_is_made_and_reads_back() {
    let Some(gpu) = card() else { return };
    let reg = bpe_kit::registry();
    let (scene, _) = small(&reg, 0.5);
    let make = final_of(&scene, &reg, 2.0);
    let finals = folder("made");
    let progress = MakeProgress::default();
    let head = make.make(&gpu, &finals, &progress, &AtomicBool::new(false)).unwrap();
    assert_eq!(head.frames, 120);
    assert_eq!(head.chunks.len(), 3, "seconds 0, 1 and the last frame's end");
    assert_eq!(head.points, vec![], "no restart point before 2 s");
    assert!(make.status(&finals, this_card()).complete());
    assert!((progress.share() - 1.0).abs() < 1e-6);
    let dir = finals.take_dir(head.print);
    let mut c = Chunk::open(&dir, 1).unwrap();
    let times: Vec<f64> = c.moments().map(|m| m.time()).collect();
    assert_eq!(times.len(), 60);
    assert!(times.windows(2).all(|w| w[0] < w[1]));
    // (The draft at 8 px is the same physics.)
    let field = make.setup.field();
    let mut body = make.setup.start(gpu.clone(), &field, FINAL_MOST);
    while body.frames() < 60 {
        make.setup.set_inputs(&mut body, &field, FINAL_MOST);
        body.frame();
    }
    let frame = FrameCodec::new().decode(&c.bytes(0).unwrap(), &head.layout()).unwrap();
    let drops = body.read();
    assert_eq!(frame.len(), drops.len(), "every drop of frame 60");
    assert!(drops.len() > 1000);
    std::fs::remove_dir_all(finals.dir()).ok();
}

/// A physics edit after the first restart point remakes only from it, and the chunks it remakes
/// are, byte for byte, those of the changed scene's final made from the start.
#[test]
fn a_final_remade_from_a_restart_point_equals_one_made_in_one_go() {
    let Some(gpu) = card() else { return };
    let reg = bpe_kit::registry();
    let (mut scene, tap) = small(&reg, 2.5);
    let finals = folder("remade");
    let stop = AtomicBool::new(false);
    let first = final_of(&scene, &reg, 4.0).make(&gpu, &finals, &MakeProgress::default(), &stop).unwrap();
    assert_eq!(first.points, vec![120]);
    set(&mut scene, tap, SlotId::Main, "rate", Value::Number(2.0));
    let changed = final_of(&scene, &reg, 4.0);
    let status = changed.status(&finals, this_card());
    assert!(!status.own && status.restart == 120, "{status:?}");
    let progress = MakeProgress::default();
    let second = changed.make(&gpu, &finals, &progress, &stop).unwrap();
    assert_eq!(progress.from.load(Ordering::Relaxed), 120, "made from the point at 2 s");
    let fresh = folder("fresh");
    let whole = changed.make(&gpu, &fresh, &MakeProgress::default(), &stop).unwrap();
    assert_eq!(second.print, whole.print);
    let (a, b) = (finals.take_dir(second.print), fresh.take_dir(whole.print));
    for s in 0..=4 {
        assert!(chunk_bytes(&a, s) == chunk_bytes(&b, s), "chunk {s} is the same");
    }
    assert!(chunk_bytes(&a, 0) == chunk_bytes(&finals.take_dir(first.print), 0), "the first chunks are shared");
    assert!(chunk_bytes(&a, 3) != chunk_bytes(&finals.take_dir(first.print), 3), "the tail is remade");
    std::fs::remove_dir_all(finals.dir()).ok();
    std::fs::remove_dir_all(fresh.dir()).ok();
}

/// A make stopped part way keeps every whole chunk, and the next make goes on from there.
#[test]
fn a_stopped_make_keeps_its_whole_chunks() {
    let Some(gpu) = card() else { return };
    let reg = bpe_kit::registry();
    let (scene, _) = small(&reg, 0.0);
    let make = final_of(&scene, &reg, 3.0);
    let finals = folder("stopped");
    let progress = MakeProgress::default();
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            while progress.at.load(Ordering::Relaxed) < 150 && progress.phase.load(Ordering::Relaxed) < 2 {
                std::thread::yield_now();
            }
            stop.store(true, Ordering::Release);
        });
        make.make(&gpu, &finals, &progress, &stop).unwrap();
    });
    let status = make.status(&finals, this_card());
    assert!(status.own && !status.complete());
    assert!(status.made >= 120 && status.made < 180, "{status:?}");
    assert_eq!(status.restart, 120);
    let again = make.make(&gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap();
    assert!(make.status(&finals, this_card()).complete(), "{again:?}");
    std::fs::remove_dir_all(finals.dir()).ok();
}

/// Slow motion keeps four moments a frame at a quarter speed, and asks for no physics again: a
/// final made at real time is remade only to keep more moments.
#[test]
fn slow_motion_keeps_more_moments_there() {
    let Some(gpu) = card() else { return };
    let reg = bpe_kit::registry();
    let (mut scene, _) = small(&reg, 0.0);
    let finals = folder("slow");
    let stop = AtomicBool::new(false);
    let real = final_of(&scene, &reg, 1.0).make(&gpu, &finals, &MakeProgress::default(), &stop).unwrap();
    assert_eq!(real.chunks[0].keep, vec![1u8; 60]);
    let speed = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "speed".into() };
    scene.set_key(&reg, &speed, 0.0, Value::Number(0.25)).unwrap();
    let slow = final_of(&scene, &reg, 1.0);
    assert_eq!(slow.print(), final_of(&scene, &reg, 1.0).print());
    assert_eq!(slow.print(), real.print, "Speed is no physics");
    assert!(!slow.status(&finals, this_card()).complete(), "too few moments for a quarter speed");
    let head = slow.make(&gpu, &finals, &MakeProgress::default(), &stop).unwrap();
    assert_eq!(head.chunks[0].keep, vec![4u8; 60]);
    assert_eq!(head.chunks[1].keep[0], 1);
    let speeds = Speeds::of(&scene, &bpe_core::resolve(&scene, &reg), 1.0);
    assert!((speeds.video_length(1.0) - 4.0).abs() < 1e-6);
    std::fs::remove_dir_all(finals.dir()).ok();
}

/// A make that needs more room than the disk has is refused, in plain words.
#[test]
fn a_make_the_disk_has_no_room_for_is_refused() {
    let Some(gpu) = card() else { return };
    let reg = bpe_kit::registry();
    let (scene, _) = small(&reg, 0.0);
    let mut make = final_of(&scene, &reg, 1.0);
    make.free_room = Some(1 << 20);
    let finals = folder("room");
    let err = make.make(&gpu, &finals, &MakeProgress::default(), &AtomicBool::new(false)).unwrap_err();
    assert!(err.to_string().contains("disk has"), "{err}");
    std::fs::remove_dir_all(finals.dir()).ok();
}
